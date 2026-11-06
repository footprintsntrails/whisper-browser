//! The Whisper Firefox profile: first-run setup and applying Whisper's settings and theme.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::util;
use crate::version::Version;

pub const UBO_ID: &str = "uBlock0@raymondhill.net";
const UBO_URL: &str =
    "https://addons.mozilla.org/firefox/downloads/latest/ublock-origin/latest.xpi";
const UBO_PENDING: &str = "ubo-pending";
const APPLIED_VERSION: &str = "applied-version";

pub fn profile_dir(data: &Path) -> PathBuf {
    data.join(crate::paths::PROFILE_DIR)
}

#[derive(Debug)]
pub struct Prepared {
    pub first_run: bool,
    pub theme_updated: bool,
}

/// Brings the profile in `data/Profile` up to date with the share folder.
///
/// * First run (no `user.js` yet): creates the profile, copies `first-run-prefs.js` to
///   `prefs.js` if there is none, and marks uBlock Origin for download.
/// * Every start: writes `user.js` (Whisper's prefs followed by the user's own
///   `user-overrides.js`, if any) and replaces `chrome/` when the share is a different
///   version from the one last applied.
pub fn prepare(share: &Path, data: &Path) -> io::Result<Prepared> {
    let profile = profile_dir(data);
    let first_run = !profile.join("user.js").is_file();
    util::create_private_dir(data)?;
    util::create_private_dir(&profile)?;
    fs::create_dir_all(profile.join("extensions"))?;

    let share_version = fs::read_to_string(share.join("VERSION"))
        .ok()
        .and_then(|v| Version::parse(&v))
        .ok_or_else(|| io::Error::other("the share folder has no valid VERSION"))?;
    let applied = fs::read_to_string(data.join(APPLIED_VERSION))
        .ok()
        .and_then(|v| Version::parse(&v));

    let theme_updated = first_run
        || applied.as_ref().map(Version::as_str) != Some(share_version.as_str())
        || !profile.join("chrome").join("userChrome.css").is_file();
    if theme_updated {
        util::replace_dir_with_copy(
            &share.join("profile").join("chrome"),
            &profile.join("chrome"),
        )?;
    }

    let mut user_js = fs::read(share.join("profile").join("user.js"))?;
    let overrides = profile.join("user-overrides.js");
    if let Ok(extra) = fs::read(&overrides) {
        user_js.extend_from_slice(b"\n// --- Your own settings, from user-overrides.js ---\n");
        user_js.extend_from_slice(&extra);
        user_js.push(b'\n');
    }
    util::write_atomic(&profile.join("user.js"), &user_js)?;

    if first_run {
        let prefs = profile.join("prefs.js");
        if !prefs.exists() {
            fs::copy(share.join("profile").join("first-run-prefs.js"), &prefs)?;
        }
        fs::write(data.join(UBO_PENDING), b"")?;
    }
    if theme_updated {
        fs::write(data.join(APPLIED_VERSION), format!("{share_version}\n"))?;
    }
    Ok(Prepared {
        first_run,
        theme_updated,
    })
}

pub fn ubo_pending(data: &Path) -> bool {
    data.join(UBO_PENDING).exists()
}

/// Downloads uBlock Origin (signed by Mozilla) from addons.mozilla.org into the profile.
/// Firefox installs it on its next start (`extensions.autoDisableScopes` is 0 in user.js).
pub fn fetch_ubo(data: &Path, timeout: Duration) -> Result<(), String> {
    let dest = profile_dir(data)
        .join("extensions")
        .join(format!("{UBO_ID}.xpi"));
    crate::net::download(&ubo_url(), &dest, 64 << 20, timeout)?;
    let is_zip = fs::read(&dest).is_ok_and(|b| b.starts_with(b"PK\x03\x04"));
    if !is_zip {
        let _ = fs::remove_file(&dest);
        return Err("the uBlock Origin download is not an add-on package".into());
    }
    let _ = fs::remove_file(data.join(UBO_PENDING));
    Ok(())
}

#[cfg(feature = "test-hooks")]
fn ubo_url() -> String {
    std::env::var("WHISPER_TEST_UBO_URL").unwrap_or_else(|_| UBO_URL.to_string())
}

#[cfg(not(feature = "test-hooks"))]
fn ubo_url() -> String {
    UBO_URL.to_string()
}

/// A profile that Firefox has never opened has no `places.sqlite` yet.
pub fn is_unused(profile: &Path) -> bool {
    !profile.join("places.sqlite").exists()
}

/// Whether a Firefox-family browser currently has this profile open.
///
/// Firefox holds an fcntl lock on `.parentlock` on Linux and macOS, and keeps
/// `parent.lock` open without sharing on Windows.
pub fn in_use(profile: &Path) -> bool {
    lock_held(profile)
}

#[cfg(unix)]
fn lock_held(profile: &Path) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(file) = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(profile.join(".parentlock"))
    else {
        return false;
    };
    // SAFETY: `flock` is plain data; F_GETLK only reads the descriptor's lock state.
    unsafe {
        let mut lock: libc::flock = std::mem::zeroed();
        lock.l_type = libc::F_WRLCK as _;
        lock.l_whence = libc::SEEK_SET as _;
        if libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut lock) == -1 {
            return false;
        }
        i64::from(lock.l_type) != i64::from(libc::F_UNLCK)
    }
}

#[cfg(windows)]
fn lock_held(profile: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    let lock = profile.join("parent.lock");
    if !lock.exists() {
        return false;
    }
    match fs::OpenOptions::new().read(true).share_mode(0).open(&lock) {
        Ok(_) => false,
        Err(e) => e.raw_os_error() == Some(ERROR_SHARING_VIOLATION),
    }
}

#[cfg(not(any(unix, windows)))]
fn lock_held(_profile: &Path) -> bool {
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn make_share(dir: &Path, version: &str) {
        fs::create_dir_all(dir.join("profile/chrome/fonts")).unwrap();
        fs::write(dir.join("VERSION"), format!("{version}\n")).unwrap();
        fs::write(
            dir.join("profile/user.js"),
            format!("// whisper {version}\n"),
        )
        .unwrap();
        fs::write(dir.join("profile/first-run-prefs.js"), "// first run\n").unwrap();
        fs::write(
            dir.join("profile/chrome/userChrome.css"),
            format!("/* {version} */"),
        )
        .unwrap();
        fs::write(dir.join("profile/chrome/fonts/x.woff2"), "font").unwrap();
    }

    #[test]
    fn first_run_sets_up_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let share = tmp.path().join("share");
        let data = tmp.path().join("data");
        make_share(&share, "0.1.0");

        let p = prepare(&share, &data).unwrap();
        assert!(p.first_run && p.theme_updated);
        let profile = profile_dir(&data);
        assert_eq!(
            fs::read_to_string(profile.join("user.js")).unwrap(),
            "// whisper 0.1.0\n"
        );
        assert_eq!(
            fs::read_to_string(profile.join("prefs.js")).unwrap(),
            "// first run\n"
        );
        assert!(profile.join("chrome/userChrome.css").is_file());
        assert!(profile.join("chrome/fonts/x.woff2").is_file());
        assert!(profile.join("extensions").is_dir());
        assert!(ubo_pending(&data));
        assert!(is_unused(&profile));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&profile).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }

    #[test]
    fn later_runs_keep_prefs_and_add_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let share = tmp.path().join("share");
        let data = tmp.path().join("data");
        make_share(&share, "0.1.0");
        prepare(&share, &data).unwrap();
        let profile = profile_dir(&data);
        fs::remove_file(data.join(UBO_PENDING)).unwrap();

        fs::write(profile.join("prefs.js"), "// changed by Firefox\n").unwrap();
        fs::write(profile.join("user.js"), "// edited by hand\n").unwrap();
        fs::write(profile.join("user-overrides.js"), "user_pref(\"a\", 1);").unwrap();

        let p = prepare(&share, &data).unwrap();
        assert!(!p.first_run && !p.theme_updated);
        assert_eq!(
            fs::read_to_string(profile.join("prefs.js")).unwrap(),
            "// changed by Firefox\n"
        );
        let user_js = fs::read_to_string(profile.join("user.js")).unwrap();
        assert!(user_js.starts_with("// whisper 0.1.0\n"));
        assert!(user_js.contains("user_pref(\"a\", 1);"));
        assert!(
            !ubo_pending(&data),
            "uBlock Origin is fetched on the first run only"
        );
    }

    #[test]
    fn new_share_version_replaces_the_theme() {
        let tmp = tempfile::tempdir().unwrap();
        let share = tmp.path().join("share");
        let data = tmp.path().join("data");
        make_share(&share, "0.1.0");
        prepare(&share, &data).unwrap();
        let chrome = profile_dir(&data).join("chrome");
        fs::write(chrome.join("stale.css"), "").unwrap();

        make_share(&share, "0.2.0");
        let p = prepare(&share, &data).unwrap();
        assert!(p.theme_updated);
        assert_eq!(
            fs::read_to_string(chrome.join("userChrome.css")).unwrap(),
            "/* 0.2.0 */"
        );
        assert!(!chrome.join("stale.css").exists());
        assert_eq!(
            fs::read_to_string(data.join(APPLIED_VERSION)).unwrap(),
            "0.2.0\n"
        );
    }

    #[test]
    fn existing_prefs_are_never_replaced_on_first_run() {
        let tmp = tempfile::tempdir().unwrap();
        let share = tmp.path().join("share");
        let data = tmp.path().join("data");
        make_share(&share, "0.1.0");
        fs::create_dir_all(profile_dir(&data)).unwrap();
        fs::write(profile_dir(&data).join("prefs.js"), "// mine\n").unwrap();
        prepare(&share, &data).unwrap();
        assert_eq!(
            fs::read_to_string(profile_dir(&data).join("prefs.js")).unwrap(),
            "// mine\n"
        );
    }

    #[test]
    fn unlocked_profile_is_not_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!in_use(tmp.path()));
        #[cfg(unix)]
        fs::write(tmp.path().join(".parentlock"), "").unwrap();
        #[cfg(windows)]
        fs::write(tmp.path().join("parent.lock"), "").unwrap();
        assert!(!in_use(tmp.path()));
    }
}
