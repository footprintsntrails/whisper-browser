//! Where Whisper's files live.
//!
//! * The **share** folder is read-only and comes with the install: `VERSION`,
//!   `profile/user.js`, `profile/first-run-prefs.js`, `profile/chrome/` and `assets/icon/`.
//! * The **data** folder is per user and holds the Whisper Firefox profile, the log and
//!   downloaded updates.

use std::env;
use std::path::{Path, PathBuf};

use crate::firefox::Flavor;

pub const PROFILE_DIR: &str = "Profile";

/// The user's home folder.
pub fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// An environment variable that holds an absolute path.
pub fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// `true` when the variable is set to anything other than empty or `0`.
pub fn env_flag(name: &str) -> bool {
    env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

/// The running executable, with symlinks resolved.
pub fn current_exe() -> PathBuf {
    canonical(&env::current_exe().unwrap_or_else(|_| PathBuf::from("whisper")))
}

/// `path` with symlinks and `..` resolved. On Windows the `\\?\` prefix that
/// `canonicalize` adds is dropped again, so the path stays usable everywhere.
pub fn canonical(path: &Path) -> PathBuf {
    let Ok(full) = path.canonicalize() else {
        return path.to_path_buf();
    };
    if cfg!(windows) {
        let text = full.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
        {
            return PathBuf::from(rest);
        }
    }
    full
}

pub fn is_share(dir: &Path) -> bool {
    dir.join("VERSION").is_file()
        && dir.join("profile").join("user.js").is_file()
        && dir.join("profile").join("first-run-prefs.js").is_file()
        && dir.join("profile").join("chrome").is_dir()
}

/// Finds the share folder that came with this install.
///
/// `WHISPER_SHARE` wins; otherwise it is looked for next to the executable
/// (`share/` for the Windows installer and the Linux tarball, `../Resources/share`
/// inside `Whisper.app`, `../share/whisper` for `/usr/bin/whisper`).
pub fn find_share(exe: &Path) -> Option<PathBuf> {
    if let Some(dir) = env_path("WHISPER_SHARE") {
        return is_share(&dir).then_some(dir);
    }
    let dir = exe.parent()?;
    [
        dir.join("share"),
        dir.join("..").join("Resources").join("share"),
        dir.join("..").join("share").join("whisper"),
    ]
    .into_iter()
    .find(|c| is_share(c))
    .map(|c| canonical(&c))
}

/// The per-user data folder. Snap and Flatpak Firefox can only read their own folders,
/// so the profile goes there for those.
pub fn data_dir(flavor: Flavor) -> PathBuf {
    if let Some(dir) = env_path("WHISPER_DATA") {
        return dir;
    }
    default_data_dir(flavor)
}

fn default_data_dir(flavor: Flavor) -> PathBuf {
    if cfg!(windows) {
        return env_path("LOCALAPPDATA")
            .unwrap_or_else(|| home().join("AppData").join("Local"))
            .join("Whisper");
    }
    if cfg!(target_os = "macos") {
        return home()
            .join("Library")
            .join("Application Support")
            .join("Whisper");
    }
    match flavor {
        Flavor::Snap => home().join("snap/firefox/common/whisper"),
        Flavor::Flatpak => home().join(".var/app/org.mozilla.firefox/data/whisper"),
        Flavor::Native => xdg_data_home().join("whisper"),
    }
}

/// Every data folder Whisper may have used on this machine (for `--uninstall --purge`).
pub fn all_data_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        default_data_dir(Flavor::Native),
        default_data_dir(Flavor::Snap),
        default_data_dir(Flavor::Flatpak),
    ];
    if let Some(dir) = env_path("WHISPER_DATA") {
        dirs.insert(0, dir);
    }
    dirs.dedup();
    dirs
}

pub fn xdg_data_home() -> PathBuf {
    env_path("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local").join("share"))
}

pub fn xdg_config_home() -> PathBuf {
    env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config"))
}

/// Whisper's own downloads (uBlock Origin, the daily update check) are off when
/// `WHISPER_OFFLINE=1` is set or a file named `offline` exists in the data folder.
pub fn offline(data: &Path) -> bool {
    env_flag("WHISPER_OFFLINE") || data.join("offline").exists()
}

/// Non-interactive mode: no dialogs (set by tests and scripted installs).
pub fn noninteractive() -> bool {
    env_flag("WHISPER_NONINTERACTIVE")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_share(dir: &Path) {
        fs::create_dir_all(dir.join("profile/chrome")).unwrap();
        fs::write(dir.join("VERSION"), "0.1.0\n").unwrap();
        fs::write(dir.join("profile/user.js"), "").unwrap();
        fs::write(dir.join("profile/first-run-prefs.js"), "").unwrap();
    }

    fn rel(path: &str) -> PathBuf {
        path.split('/').collect()
    }

    #[test]
    fn finds_share_in_each_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root = canonical(tmp.path());

        // Windows installer and Linux tarball: share/ next to the binary
        make_share(&root.join(rel("a/share")));
        assert_eq!(
            find_share(&root.join(rel("a/whisper"))),
            Some(root.join(rel("a/share")))
        );

        // macOS app bundle
        fs::create_dir_all(root.join(rel("Whisper.app/Contents/MacOS"))).unwrap();
        make_share(&root.join(rel("Whisper.app/Contents/Resources/share")));
        assert_eq!(
            find_share(&root.join(rel("Whisper.app/Contents/MacOS/whisper"))),
            Some(root.join(rel("Whisper.app/Contents/Resources/share")))
        );

        // deb and rpm: /usr/bin/whisper and /usr/share/whisper
        fs::create_dir_all(root.join(rel("usr/bin"))).unwrap();
        make_share(&root.join(rel("usr/share/whisper")));
        assert_eq!(
            find_share(&root.join(rel("usr/bin/whisper"))),
            Some(root.join(rel("usr/share/whisper")))
        );

        // nothing
        fs::create_dir_all(root.join("empty")).unwrap();
        assert_eq!(find_share(&root.join(rel("empty/whisper"))), None);
    }

    #[test]
    fn incomplete_share_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        make_share(tmp.path());
        fs::remove_file(tmp.path().join("profile/first-run-prefs.js")).unwrap();
        assert!(!is_share(tmp.path()));
    }
}
