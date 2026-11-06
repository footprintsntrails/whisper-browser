//! Build tooling for Whisper. Run `cargo xtask help`.

mod art;
mod fsutil;
mod package;
mod release;
mod scan;

use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "\
cargo xtask <command> [options]

Release files (everything goes to dist/, or --dist DIR; work files to build/):
  bundle [--version V]                 share folder (build/share), whisper-profile.tar.gz/.zip, VERSION
  tarball --bin PATH [--arch A]        whisper-linux[-arm64].tar.gz      (A: x86_64 | aarch64)
  deb --bin PATH [--arch A]            whisper[-arm64].deb  (needs dpkg-deb)
  rpm --bin PATH [--arch A]            whisper[-aarch64].rpm (needs rpmbuild)
  nsis --bin PATH                      Whisper-Setup.exe     (needs makensis)
  dmg --bin PATH                       Whisper.dmg           (macOS: needs hdiutil, codesign)
  manifest                             update.json listing every file in dist/ with its SHA-256
  sign --key PRIVATE.pem               update.json.sig (openssl, RSA PKCS#1 v1.5 + SHA-256)
  verify-manifest --key PUBLIC.pem     check update.json.sig and every file against update.json
  signing-selftest                     sign with a throwaway key and check accept/reject end to end
  checksums                            SHA256SUMS for every file in dist/
  notes                                release notes for the version in dist/VERSION (stdout)

Artwork (results are committed):
  icons                                assets/icon/* from assets/logo/*.svg
  banner --font-dir DIR                assets/readme/banner.png (Inter TTFs in DIR)
  installer-art --font-dir DIR         assets/installer/*.bmp for the Windows installer

Checks:
  scan                                 look for secrets, personal data and local paths in tracked files
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        print!("{HELP}");
        return ExitCode::FAILURE;
    };
    let opts = Opts(args[1..].to_vec());
    if let Some(dir) = opts.get("dist") {
        let _ = DIST.set(PathBuf::from(dir));
    }
    let result = match command.as_str() {
        "bundle" => package::bundle(&opts),
        "tarball" => package::tarball(&opts),
        "deb" => package::deb(&opts),
        "rpm" => package::rpm(&opts),
        "nsis" => package::nsis(&opts),
        "dmg" => package::dmg(&opts),
        "manifest" => release::manifest(),
        "sign" => release::sign(&opts),
        "verify-manifest" => release::verify_manifest(&opts),
        "signing-selftest" => release::signing_selftest(),
        "checksums" => release::checksums(),
        "notes" => release::notes(),
        "icons" => art::icons(),
        "banner" => art::banner(&opts),
        "installer-art" => art::installer_art(&opts),
        "scan" => scan::scan(),
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}; see cargo xtask help")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {command}: {e}");
            ExitCode::FAILURE
        }
    }
}

pub struct Opts(Vec<String>);

impl Opts {
    pub fn get(&self, name: &str) -> Option<String> {
        let flag = format!("--{name}");
        let mut it = self.0.iter();
        while let Some(a) = it.next() {
            if *a == flag {
                return it.next().cloned();
            }
            if let Some(v) = a.strip_prefix(&format!("{flag}=")) {
                return Some(v.to_string());
            }
        }
        None
    }

    pub fn path(&self, name: &str) -> Result<PathBuf, String> {
        self.get(name)
            .map(PathBuf::from)
            .ok_or_else(|| format!("--{name} is required"))
    }
}

/// The repository root.
pub fn root() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    dir.canonicalize().unwrap_or(dir)
}

static DIST: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Where release files go: dist/, or the folder given with --dist.
pub fn dist() -> PathBuf {
    DIST.get().cloned().unwrap_or_else(|| root().join("dist"))
}

pub fn build() -> PathBuf {
    root().join("build")
}
