//! update.json, its signature and verification, SHA256SUMS and the release notes.
//!
//! Signing uses the `openssl` command, so the private key is only ever read by OpenSSL.
//! Verification uses the same Rust code Whisper runs before it installs an update.

use std::fs;
use std::path::Path;
use std::process::Command;

use whisper::signature::sha256_file;
use whisper::update::{
    Asset, MANIFEST, Manifest, SIGNATURE, check_asset, verify_manifest as verify,
};

use crate::fsutil::{self, err, run};
use crate::{Opts, build, dist, root};

const NOT_ASSETS: &[&str] = &[MANIFEST, SIGNATURE, "SHA256SUMS"];

fn assets(dir: &Path) -> Result<Vec<String>, String> {
    let names: Vec<String> = fsutil::walk(dir)?
        .into_iter()
        .filter(|n| !n.contains('/') && !NOT_ASSETS.contains(&n.as_str()))
        .collect();
    if names.is_empty() {
        return Err(format!("{} is empty", dir.display()));
    }
    Ok(names)
}

fn version_in(dir: &Path) -> Result<String, String> {
    fs::read_to_string(dir.join("VERSION"))
        .map(|v| v.trim().to_string())
        .map_err(err(dir.join("VERSION").display()))
}

/// update.json: the version and the size and SHA-256 of every release file.
pub fn manifest() -> Result<(), String> {
    let dir = dist();
    let mut list = Vec::new();
    for name in assets(&dir)? {
        let path = dir.join(&name);
        list.push(Asset {
            sha256: sha256_file(&path).map_err(err(&name))?,
            size: fs::metadata(&path).map_err(err(&name))?.len(),
            name,
        });
    }
    let manifest = Manifest {
        schema: 1,
        version: version_in(&dir)?,
        assets: list,
    };
    let mut json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    json.push('\n');
    fsutil::write(&dir.join(MANIFEST), json)?;
    println!("{MANIFEST} lists {} files", manifest.assets.len());
    Ok(())
}

/// Signs update.json with the private key in `--key` (RSA PKCS#1 v1.5, SHA-256).
pub fn sign(opts: &Opts) -> Result<(), String> {
    sign_with(&opts.path("key")?, &dist())
}

fn sign_with(key: &Path, dir: &Path) -> Result<(), String> {
    run(Command::new("openssl")
        .args(["dgst", "-sha256", "-sign"])
        .arg(key)
        .arg("-out")
        .arg(dir.join(SIGNATURE))
        .arg(dir.join(MANIFEST)))?;
    println!("{SIGNATURE} written");
    Ok(())
}

/// Checks update.json.sig with the public key in `--key`, then every file against update.json.
pub fn verify_manifest(opts: &Opts) -> Result<(), String> {
    let key = opts.path("key")?;
    let count = verify_dir(&key, &dist())?;
    println!("update.json: signature good, {count} files match");
    Ok(())
}

fn verify_dir(key_path: &Path, dir: &Path) -> Result<usize, String> {
    let key = fs::read_to_string(key_path).map_err(err(key_path.display()))?;
    let json = fs::read(dir.join(MANIFEST)).map_err(err(MANIFEST))?;
    let sig = fs::read(dir.join(SIGNATURE)).map_err(err(SIGNATURE))?;
    let manifest = verify(&key, &json, &sig)?;
    let expected = version_in(dir)?;
    if manifest.version != expected {
        return Err(format!(
            "update.json says {}, VERSION says {expected}",
            manifest.version
        ));
    }
    for asset in &manifest.assets {
        check_asset(asset, &dir.join(&asset.name))?;
    }
    for name in assets(dir)? {
        if !manifest.assets.iter().any(|a| a.name == name) {
            return Err(format!("{name} is not listed in update.json"));
        }
    }
    Ok(manifest.assets.len())
}

/// For dry runs and CI: signs update.json with a throwaway key made on the spot, then checks
/// that the whole verify path accepts it, rejects a changed update.json, rejects a package
/// that does not match, and that the real release key does not accept the throwaway signature.
pub fn signing_selftest() -> Result<(), String> {
    let work = build().join("signing-selftest");
    fsutil::fresh_dir(&work)?;
    let key = work.join("throwaway.pem");
    let public = work.join("throwaway.pub.pem");
    run(Command::new("openssl")
        .args([
            "genpkey",
            "-algorithm",
            "RSA",
            "-pkeyopt",
            "rsa_keygen_bits:3072",
            "-out",
        ])
        .arg(&key))?;
    run(Command::new("openssl")
        .args(["pkey", "-pubout", "-in"])
        .arg(&key)
        .arg("-out")
        .arg(&public))?;
    let dir = dist();
    let result = (|| {
        sign_with(&key, &dir)?;
        let count = verify_dir(&public, &dir)?;
        println!("ok: a correctly signed update.json and {count} matching files are accepted");

        let tampered = work.join("tampered");
        fsutil::copy_dir(&dir, &tampered)?;
        let json = fs::read_to_string(tampered.join(MANIFEST)).map_err(err(MANIFEST))?;
        let changed = json.replacen("\"version\": \"", "\"version\": \"9", 1);
        assert_ne!(json, changed);
        fsutil::write(&tampered.join(MANIFEST), changed)?;
        expect_rejected(verify_dir(&public, &tampered), "a changed update.json")?;

        let swapped = work.join("swapped");
        fsutil::copy_dir(&dir, &swapped)?;
        let victim = assets(&swapped)?
            .into_iter()
            .max_by_key(|n| fs::metadata(swapped.join(n)).map(|m| m.len()).unwrap_or(0))
            .ok_or("no files")?;
        let mut bytes = fs::read(swapped.join(&victim)).map_err(err(&victim))?;
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fsutil::write(&swapped.join(&victim), bytes)?;
        expect_rejected(
            verify_dir(&public, &swapped),
            &format!("a changed {victim}"),
        )?;

        let release_key = root().join("keys/update-signing.pub.pem");
        expect_rejected(
            verify_dir(&release_key, &dir),
            "the throwaway signature under the real release key",
        )
    })();
    let _ = fs::remove_file(&key);
    result
}

fn expect_rejected(result: Result<usize, String>, what: &str) -> Result<(), String> {
    match result {
        Ok(_) => Err(format!("{what} was accepted")),
        Err(e) => {
            println!("ok: {what} is rejected ({e})");
            Ok(())
        }
    }
}

/// SHA256SUMS for every file in dist/ (including update.json and its signature).
pub fn checksums() -> Result<(), String> {
    let dir = dist();
    let mut lines = String::new();
    for name in fsutil::walk(&dir)? {
        if name == "SHA256SUMS" || name.contains('/') {
            continue;
        }
        let hash = sha256_file(&dir.join(&name)).map_err(err(&name))?;
        lines.push_str(&format!("{hash}  {name}\n"));
    }
    fsutil::write(&dir.join("SHA256SUMS"), &lines)?;
    print!("{lines}");
    Ok(())
}

/// The release notes from .github/release-notes.md, for the version in dist/VERSION.
pub fn notes() -> Result<(), String> {
    let version = version_in(&dist())?;
    let template = fs::read_to_string(root().join(".github/release-notes.md"))
        .map_err(err("release-notes.md"))?;
    print!("{}", template.replace("{{VERSION}}", &version));
    Ok(())
}
