//! Self-update, once a day, in a background process after Firefox has started.
//!
//! 1. Download `update.json` and `update.json.sig` from the latest GitHub release.
//! 2. Check the RSA signature with the public key compiled into Whisper. Nothing in an
//!    unsigned or wrongly signed manifest is used.
//! 3. If the manifest names a newer version, download the package for this install
//!    (`Whisper-Setup.exe`, `Whisper.dmg` or the Linux tarball) from that exact release and
//!    check its size and SHA-256 against the signed manifest.
//! 4. Install it for the next start: the Windows installer runs silently; on macOS and for
//!    a Linux install in the home folder the new copy is unpacked next to the old one and
//!    swapped in with renames. Installs from a .deb or .rpm need root, so Whisper only says
//!    that a new version exists.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::version::Version;
use crate::{info, net, paths, profile, signature, util};

pub const REPO: &str = "footprintsntrails/whisper-browser";
pub const MANIFEST: &str = "update.json";
pub const SIGNATURE: &str = "update.json.sig";
const CHECK_EVERY_SECS: u64 = 24 * 3600;
const LAST_CHECK: &str = "last-update-check";
const LOCK: &str = "update.lock";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Manifest {
    pub schema: u32,
    pub version: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

/// Checks the signature over the exact bytes of `update.json`, then parses it.
pub fn verify_manifest(public_key_pem: &str, json: &[u8], sig: &[u8]) -> Result<Manifest, String> {
    signature::verify_rsa_sha256(public_key_pem, json, sig)
        .map_err(|e| format!("update.json: {e}"))?;
    let manifest: Manifest =
        serde_json::from_slice(json).map_err(|e| format!("update.json is not valid: {e}"))?;
    if manifest.schema != 1 {
        return Err(format!(
            "update.json schema {} is not supported",
            manifest.schema
        ));
    }
    if Version::parse(&manifest.version).is_none() {
        return Err(format!(
            "update.json has an invalid version {:?}",
            manifest.version
        ));
    }
    for asset in &manifest.assets {
        let name_ok = !asset.name.is_empty()
            && !asset.name.starts_with('.')
            && asset
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        let hash_ok = asset.sha256.len() == 64
            && asset
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !name_ok || !hash_ok {
            return Err(format!(
                "update.json has an invalid entry for {:?}",
                asset.name
            ));
        }
    }
    Ok(manifest)
}

/// Checks a downloaded file's size and SHA-256 against its manifest entry.
pub fn check_asset(asset: &Asset, file: &Path) -> Result<(), String> {
    let size = fs::metadata(file).map_err(|e| e.to_string())?.len();
    if size != asset.size {
        return Err(format!(
            "{}: size {size}, expected {}",
            asset.name, asset.size
        ));
    }
    let hash = signature::sha256_file(file).map_err(|e| e.to_string())?;
    if hash != asset.sha256 {
        return Err(format!(
            "{}: SHA-256 does not match update.json",
            asset.name
        ));
    }
    Ok(())
}

/// How this copy of Whisper was installed, which decides how it updates.
#[derive(Debug, PartialEq, Eq)]
pub enum Kind {
    /// Whisper-Setup.exe (Windows): run the new installer silently.
    WindowsSetup,
    /// Whisper.app (macOS): swap in the app from the new disk image.
    MacApp(PathBuf),
    /// A folder with `whisper` and `share/` (Linux tarball or `whisper --install`).
    Folder(PathBuf),
    /// /usr/bin/whisper from a .deb or .rpm: needs root, so only tell the user.
    Package,
    /// Cannot update itself (development run, read-only location, …).
    Manual(String),
}

pub fn install_kind(exe: &Path) -> Kind {
    #[cfg(feature = "test-hooks")]
    {
        if let Some(dir) = paths::env_path("WHISPER_TEST_INSTALL_DIR") {
            return Kind::Folder(dir);
        }
        if let Some(app) = paths::env_path("WHISPER_TEST_INSTALL_APP") {
            return Kind::MacApp(app);
        }
        if paths::env_flag("WHISPER_TEST_INSTALL_SETUP") {
            return Kind::WindowsSetup;
        }
    }
    if paths::env_path("WHISPER_SHARE").is_some() {
        return Kind::Manual("WHISPER_SHARE is set (a development run)".into());
    }
    let Some(dir) = exe.parent() else {
        return Kind::Manual("unknown location".into());
    };
    if cfg!(windows) {
        return if dir.join("uninstall.exe").is_file() && util::dir_writable(dir) {
            Kind::WindowsSetup
        } else {
            Kind::Manual("not installed with Whisper-Setup.exe".into())
        };
    }
    if cfg!(target_os = "macos") {
        let app = dir.parent().and_then(Path::parent);
        return match app {
            Some(app) if app.extension().is_some_and(|e| e == "app") => {
                if app.to_string_lossy().contains("/AppTranslocation/") {
                    Kind::Manual(
                        "Whisper runs from a temporary copy; move it to Applications".into(),
                    )
                } else if app.parent().is_some_and(util::dir_writable) {
                    Kind::MacApp(app.to_path_buf())
                } else {
                    Kind::Manual(format!("cannot write to {}", app.display()))
                }
            }
            _ => Kind::Manual("not running from Whisper.app".into()),
        };
    }
    if paths::is_share(&dir.join("share")) {
        return match dir.parent() {
            Some(parent) if util::dir_writable(parent) && util::dir_writable(dir) => {
                Kind::Folder(dir.to_path_buf())
            }
            _ => Kind::Manual(format!("cannot write to {}", dir.display())),
        };
    }
    if paths::is_share(&dir.join("../share/whisper")) {
        return Kind::Package;
    }
    Kind::Manual("unknown install layout".into())
}

/// The release file that updates this kind of install.
pub fn asset_for(kind: &Kind) -> Option<&'static str> {
    match kind {
        Kind::WindowsSetup => Some("Whisper-Setup.exe"),
        Kind::MacApp(_) => Some("Whisper.dmg"),
        Kind::Folder(_) => Some(linux_tarball()),
        Kind::Package | Kind::Manual(_) => None,
    }
}

pub fn linux_tarball() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "whisper-linux-arm64.tar.gz"
    } else {
        "whisper-linux.tar.gz"
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    UpToDate(String),
    Installed(String),
    Notified(String),
    Skipped(String),
}

struct Source {
    manifest_url: String,
    signature_url: String,
    public_key: String,
}

impl Source {
    fn asset_url(&self, version: &str, name: &str) -> String {
        #[cfg(feature = "test-hooks")]
        if let Ok(base) = std::env::var("WHISPER_TEST_UPDATE_URL") {
            return format!("{base}/{name}");
        }
        format!("https://github.com/{REPO}/releases/download/v{version}/{name}")
    }
}

fn source() -> Source {
    let latest = format!("https://github.com/{REPO}/releases/latest/download");
    #[cfg(feature = "test-hooks")]
    let latest = std::env::var("WHISPER_TEST_UPDATE_URL").unwrap_or(latest);
    #[cfg(feature = "test-hooks")]
    let public_key = match std::env::var_os("WHISPER_TEST_PUBKEY") {
        Some(path) => fs::read_to_string(path).unwrap_or_default(),
        None => signature::UPDATE_PUBLIC_KEY.to_string(),
    };
    #[cfg(not(feature = "test-hooks"))]
    let public_key = signature::UPDATE_PUBLIC_KEY.to_string();
    Source {
        manifest_url: format!("{latest}/{MANIFEST}"),
        signature_url: format!("{latest}/{SIGNATURE}"),
        public_key,
    }
}

/// One update check: fetch, verify, download and install if there is a newer version.
pub fn check(data: &Path, exe: &Path) -> Result<Outcome, String> {
    let src = source();
    let json = net::get(&src.manifest_url, 256 * 1024, Duration::from_secs(30))?;
    let sig = net::get(&src.signature_url, 4096, Duration::from_secs(30))?;
    let manifest = verify_manifest(&src.public_key, &json, &sig)?;
    let remote = Version::parse(&manifest.version).expect("checked in verify_manifest");
    if remote <= Version::current() {
        return Ok(Outcome::UpToDate(manifest.version));
    }

    let kind = install_kind(exe);
    info!(
        "update: {} is available; this install: {kind:?}",
        manifest.version
    );
    let name = match (&kind, asset_for(&kind)) {
        (Kind::Package, _) => {
            notify_package_update(data, &manifest.version);
            return Ok(Outcome::Notified(manifest.version));
        }
        (Kind::Manual(why), _) => return Ok(Outcome::Skipped(why.clone())),
        (_, Some(name)) => name,
        (_, None) => return Ok(Outcome::Skipped("no package for this install".into())),
    };
    let asset = manifest
        .assets
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| format!("update.json lists no {name}"))?;

    let downloads = data.join("updates");
    let _ = fs::remove_dir_all(&downloads);
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let file = downloads.join(name);
    net::download(
        &src.asset_url(&manifest.version, name),
        &file,
        asset.size + 1,
        Duration::from_secs(20 * 60),
    )?;
    if let Err(e) = check_asset(asset, &file) {
        let _ = fs::remove_file(&file);
        return Err(e);
    }
    install(&kind, &file, &remote)?;
    Ok(Outcome::Installed(manifest.version))
}

fn install(kind: &Kind, file: &Path, version: &Version) -> Result<(), String> {
    match kind {
        Kind::Folder(dir) => install_folder(dir, file, version),
        Kind::MacApp(app) => install_mac_app(app, file, version),
        Kind::WindowsSetup => install_windows_setup(file),
        Kind::Package | Kind::Manual(_) => Ok(()),
    }
}

/// Unpacks the tarball next to `dir` and swaps it in; the running copy keeps working
/// because only folder names change.
fn install_folder(dir: &Path, tarball: &Path, version: &Version) -> Result<(), String> {
    let staged = util::sibling(dir, "new");
    let _ = fs::remove_dir_all(&staged);
    let result = (|| {
        crate::archive::extract_tar_gz_strip(tarball, &staged)?;
        expect_version(&staged.join("share"), version)?;
        if !staged.join("whisper").is_file() && !staged.join("whisper.exe").is_file() {
            return Err("the new package has no whisper program".to_string());
        }
        util::swap_in(&staged, dir).map_err(|e| format!("could not replace {}: {e}", dir.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staged);
    }
    let _ = fs::remove_file(tarball);
    result
}

fn expect_version(share: &Path, version: &Version) -> Result<(), String> {
    let found = fs::read_to_string(share.join("VERSION"))
        .ok()
        .and_then(|v| Version::parse(&v));
    match found {
        Some(v) if v == *version => Ok(()),
        other => Err(format!(
            "the new package is version {}, update.json says {version}",
            other.map(|v| v.to_string()).unwrap_or_else(|| "?".into())
        )),
    }
}

#[cfg(target_os = "macos")]
fn install_mac_app(app: &Path, dmg: &Path, version: &Version) -> Result<(), String> {
    let mount = util::TempDir::new("dmg")?;
    let staged = util::sibling(app, "new");
    let _ = fs::remove_dir_all(&staged);
    let result = crate::macos::with_mounted_dmg(dmg, mount.path(), || {
        let copied = std::process::Command::new("ditto")
            .arg(mount.path().join("Whisper.app"))
            .arg(&staged)
            .status()
            .is_ok_and(|s| s.success());
        if !copied {
            return Err("could not copy Whisper.app from the disk image".to_string());
        }
        crate::macos::verify(&staged)?;
        expect_version(&staged.join("Contents/Resources/share"), version)?;
        util::swap_in(&staged, app).map_err(|e| format!("could not replace {}: {e}", app.display()))
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staged);
    }
    let _ = fs::remove_file(dmg);
    result
}

#[cfg(not(target_os = "macos"))]
fn install_mac_app(_: &Path, _: &Path, _: &Version) -> Result<(), String> {
    Err("not macOS".into())
}

/// The installer renames the running whisper.exe out of the way, so it can run right now.
#[cfg(windows)]
fn install_windows_setup(setup: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(setup)
        .arg("/S")
        .creation_flags(crate::winapi::DETACHED)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start the installer: {e}"))
}

#[cfg(not(windows))]
fn install_windows_setup(_: &Path) -> Result<(), String> {
    Err("not Windows".into())
}

fn notify_package_update(data: &Path, version: &str) {
    let marker = data.join("notified-version");
    if fs::read_to_string(&marker).is_ok_and(|v| v.trim() == version) {
        return;
    }
    let ext = if Path::new("/usr/bin/rpm").exists() && !Path::new("/usr/bin/dpkg").exists() {
        "rpm"
    } else {
        "deb"
    };
    crate::ui::notify(
        &format!("Whisper {version} is available"),
        &format!(
            "Download whisper.{ext} from github.com/{REPO}/releases/latest and open it to update."
        ),
    );
    let _ = fs::write(marker, version);
}

/// Whether the daily check is due.
pub fn due(data: &Path) -> bool {
    util::age_secs(&data.join(LAST_CHECK)).is_none_or(|age| age >= CHECK_EVERY_SECS)
}

/// The background job: retries the uBlock Origin download if the first run could not get
/// it, then runs the daily update check. Only one runs at a time.
pub fn run_background(data: &Path, exe: &Path) {
    let lock = data.join(LOCK);
    if util::age_secs(&lock).is_some_and(|age| age < 30 * 60) {
        return;
    }
    let _ = fs::remove_file(&lock);
    if fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .is_err()
    {
        return;
    }
    if profile::ubo_pending(data) {
        match profile::fetch_ubo(data, Duration::from_secs(300)) {
            Ok(()) => info!("uBlock Origin downloaded; Firefox adds it on the next start"),
            Err(e) => info!("uBlock Origin download failed, will retry: {e}"),
        }
    }
    if due(data) {
        let _ = util::touch(&data.join(LAST_CHECK));
        log_outcome(check(data, exe));
    }
    let _ = fs::remove_file(&lock);
}

pub fn log_outcome(result: Result<Outcome, String>) {
    match result {
        Ok(Outcome::UpToDate(v)) => info!("update: up to date (latest release is {v})"),
        Ok(Outcome::Installed(v)) => {
            info!("update: Whisper {v} installed; it is used from the next start")
        }
        Ok(Outcome::Notified(v)) => {
            info!("update: Whisper {v} is available; the user was told how to update")
        }
        Ok(Outcome::Skipped(why)) => info!("update: not updating this copy: {why}"),
        Err(e) => info!("update: check failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::tests::{sign, test_key};

    fn manifest_json(version: &str, assets: &[(&str, &str, u64)]) -> Vec<u8> {
        serde_json::to_vec_pretty(&Manifest {
            schema: 1,
            version: version.into(),
            assets: assets
                .iter()
                .map(|(n, h, s)| Asset {
                    name: n.to_string(),
                    sha256: h.to_string(),
                    size: *s,
                })
                .collect(),
        })
        .unwrap()
    }

    #[test]
    fn good_manifest_is_accepted_and_tampering_is_not() {
        let (private, public) = test_key();
        let hash = signature::sha256_bytes(b"package");
        let json = manifest_json("0.2.0", &[("whisper-linux.tar.gz", &hash, 7)]);
        let sig = sign(&private, &json);

        let m = verify_manifest(&public, &json, &sig).unwrap();
        assert_eq!(m.version, "0.2.0");
        assert_eq!(m.assets[0].name, "whisper-linux.tar.gz");

        // One byte changed: the version, then the hash
        let text = String::from_utf8(json.clone()).unwrap();
        let flipped = format!(
            "{}{}",
            if hash.starts_with('0') { '1' } else { '0' },
            &hash[1..]
        );
        for tampered in [
            text.replace("0.2.0", "9.2.0"),
            text.replacen(&hash, &flipped, 1),
        ] {
            assert!(verify_manifest(&public, tampered.as_bytes(), &sig).is_err());
        }
        // Signed by someone else
        let (other, _) = test_key();
        assert!(verify_manifest(&public, &json, &sign(&other, &json)).is_err());
        // Truncated signature
        assert!(verify_manifest(&public, &json, &sig[..100]).is_err());
    }

    #[test]
    fn signed_but_malformed_manifests_are_rejected() {
        let (private, public) = test_key();
        let hash = signature::sha256_bytes(b"x");
        for json in [
            manifest_json("not a version", &[]),
            manifest_json("0.2.0", &[("../evil", &hash, 1)]),
            manifest_json("0.2.0", &[("ok.tar.gz", "ABC", 1)]),
            br#"{"schema":2,"version":"0.2.0","assets":[]}"#.to_vec(),
        ] {
            let sig = sign(&private, &json);
            assert!(verify_manifest(&public, &json, &sig).is_err());
        }
    }

    #[test]
    fn asset_must_match_size_and_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("pkg");
        fs::write(&file, b"package").unwrap();
        let good = Asset {
            name: "pkg".into(),
            sha256: signature::sha256_bytes(b"package"),
            size: 7,
        };
        assert!(check_asset(&good, &file).is_ok());
        let wrong_hash = Asset {
            sha256: signature::sha256_bytes(b"packagf"),
            ..good.clone()
        };
        assert!(
            check_asset(&wrong_hash, &file)
                .unwrap_err()
                .contains("SHA-256")
        );
        let wrong_size = Asset { size: 8, ..good };
        assert!(
            check_asset(&wrong_size, &file)
                .unwrap_err()
                .contains("size")
        );
    }

    #[test]
    fn folder_update_swaps_in_the_new_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        fs::create_dir_all(app.join("share")).unwrap();
        fs::write(app.join("whisper"), "old").unwrap();
        fs::write(app.join("share/VERSION"), "0.1.0\n").unwrap();
        let tarball = tmp.path().join("whisper-linux.tar.gz");
        crate::archive::tests::make_tar_gz(
            &tarball,
            &[
                ("whisper-linux/whisper", b"new", 0o755),
                ("whisper-linux/share/VERSION", b"0.2.0\n", 0o644),
            ],
        );
        install_folder(&app, &tarball, &Version::parse("0.2.0").unwrap()).unwrap();
        assert_eq!(fs::read_to_string(app.join("whisper")).unwrap(), "new");
        assert!(!util::sibling(&app, "new").exists());
        assert!(!util::sibling(&app, "old").exists());
    }

    #[test]
    fn folder_update_refuses_a_package_of_another_version() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("app");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("whisper"), "old").unwrap();
        let tarball = tmp.path().join("t.tar.gz");
        crate::archive::tests::make_tar_gz(
            &tarball,
            &[
                ("w/whisper", b"new", 0o755),
                ("w/share/VERSION", b"0.3.0\n", 0o644),
            ],
        );
        let err = install_folder(&app, &tarball, &Version::parse("0.2.0").unwrap()).unwrap_err();
        assert!(err.contains("0.3.0"), "{err}");
        assert_eq!(fs::read_to_string(app.join("whisper")).unwrap(), "old");
    }

    #[test]
    fn daily_check_is_due_without_a_stamp_and_not_right_after_one() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(due(tmp.path()));
        util::touch(&tmp.path().join(LAST_CHECK)).unwrap();
        assert!(!due(tmp.path()));
    }
}
