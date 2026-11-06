//! Finding, installing and starting Firefox.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// A normal install (or Ubuntu's `/usr/bin/firefox` wrapper, see `Snap`).
    Native,
    /// Firefox from the Snap Store; it can only read `~/snap/firefox/common`.
    Snap,
    /// Firefox from Flathub, started with `flatpak run org.mozilla.firefox`.
    Flatpak,
}

#[derive(Clone, Debug)]
pub struct Firefox {
    pub program: PathBuf,
    pub flavor: Flavor,
}

pub const FLATPAK_ID: &str = "org.mozilla.firefox";

/// Looks for Firefox: `WHISPER_FIREFOX` first, then the usual places for this system.
pub fn find() -> Option<Firefox> {
    if let Some(path) = paths::env_path("WHISPER_FIREFOX") {
        return path.is_file().then(|| Firefox {
            flavor: flavor_of(&path),
            program: path,
        });
    }
    find_installed()
}

#[cfg(target_os = "linux")]
fn find_installed() -> Option<Firefox> {
    for name in ["firefox", "firefox-esr"] {
        if let Some(path) = search_path(name) {
            return Some(Firefox {
                flavor: flavor_of(&path),
                program: path,
            });
        }
    }
    let flatpak = search_path("flatpak")?;
    let installed = Command::new(&flatpak)
        .args(["info", FLATPAK_ID])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    installed.then_some(Firefox {
        program: flatpak,
        flavor: Flavor::Flatpak,
    })
}

#[cfg(target_os = "macos")]
fn find_installed() -> Option<Firefox> {
    let home = paths::home();
    let mut apps = vec![
        PathBuf::from("/Applications/Firefox.app"),
        home.join("Applications/Firefox.app"),
    ];
    // Anywhere else Spotlight knows about
    if let Ok(out) = Command::new("mdfind")
        .arg("kMDItemCFBundleIdentifier == 'org.mozilla.firefox'")
        .output()
    {
        apps.extend(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| l.ends_with(".app"))
                .map(PathBuf::from),
        );
    }
    apps.into_iter()
        .map(|app| app.join("Contents/MacOS/firefox"))
        .find(|p| p.is_file())
        .map(|program| Firefox {
            program,
            flavor: Flavor::Native,
        })
}

#[cfg(windows)]
fn find_installed() -> Option<Firefox> {
    let mut candidates: Vec<PathBuf> = crate::winapi::app_path("firefox.exe")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    for var in [
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
    ] {
        if let Some(dir) = paths::env_path(var) {
            candidates.push(dir.join("Mozilla Firefox").join("firefox.exe"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .map(|program| Firefox {
            program,
            flavor: Flavor::Native,
        })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn find_installed() -> Option<Firefox> {
    search_path("firefox").map(|program| Firefox {
        program,
        flavor: Flavor::Native,
    })
}

/// Snap Firefox lives under `/snap`; Ubuntu's `/usr/bin/firefox` is a small script that
/// starts the snap, so a short file mentioning `/snap/bin/firefox` counts too.
fn flavor_of(path: &Path) -> Flavor {
    if !cfg!(target_os = "linux") {
        return Flavor::Native;
    }
    if path.starts_with("/snap") {
        return Flavor::Snap;
    }
    let small = std::fs::metadata(path).is_ok_and(|m| m.len() < 64 * 1024);
    if small
        && std::fs::read(path).is_ok_and(|bytes| {
            bytes
                .windows(b"/snap/bin/firefox".len())
                .any(|w| w == b"/snap/bin/firefox")
        })
    {
        return Flavor::Snap;
    }
    Flavor::Native
}

pub fn search_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The command that starts Firefox with the Whisper profile. `extra` comes first
/// (for example `--migration`), then the user's own arguments.
pub fn command(ff: &Firefox, profile: &Path, extra: &[&str], user_args: &[OsString]) -> Command {
    let mut cmd = Command::new(&ff.program);
    if ff.flavor == Flavor::Flatpak {
        cmd.args(["run", FLATPAK_ID]);
    }
    if cfg!(target_os = "linux") {
        // The window is grouped under Whisper's own icon (StartupWMClass=Whisper)
        cmd.args(["--class", "Whisper", "--name", "Whisper"]);
    }
    cmd.arg("--profile").arg(profile);
    cmd.args(extra);
    // Old macOS versions pass a process serial number to apps opened from Finder
    cmd.args(
        user_args
            .iter()
            .filter(|a| !a.to_string_lossy().starts_with("-psn_")),
    );
    cmd
}

/// Installs Firefox from mozilla.org after asking. Linux has no installer here:
/// Firefox comes from the package manager there.
#[cfg(target_os = "linux")]
pub fn install() -> Result<Firefox, String> {
    const PAGE: &str = "https://www.mozilla.org/firefox/linux/";
    let message = "Whisper runs on Firefox, which is not installed.\n\n\
        Install Firefox from your package manager (for example: sudo apt install firefox, \
        sudo dnf install firefox, or flatpak install flathub org.mozilla.firefox), \
        then start Whisper again.";
    if !paths::noninteractive() && crate::ui::has_display() {
        let _ = Command::new("xdg-open")
            .arg(PAGE)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        crate::ui::error("Whisper needs Firefox", message);
    }
    Err(message.replace("\n\n", " "))
}

#[cfg(target_os = "macos")]
pub fn install() -> Result<Firefox, String> {
    const URL: &str = "https://download.mozilla.org/?product=firefox-latest-ssl&os=osx&lang=en-US";
    use crate::ui;
    if paths::noninteractive() {
        return Err("Firefox is not installed".into());
    }
    let ask = ui::ask(
        "Whisper",
        "Whisper runs on Firefox, which is not installed on this Mac.\n\nDownload Firefox from mozilla.org and install it now?",
        "Install Firefox",
        "Cancel",
    );
    if ask != Some(true) {
        return Err("Firefox is not installed".into());
    }
    ui::notify("Whisper", "Downloading Firefox from mozilla.org…");
    let tmp = crate::util::TempDir::new("firefox")?;
    let dmg = tmp.path().join("Firefox.dmg");
    let mount = tmp.path().join("mnt");
    let result = crate::net::download(URL, &dmg, 1 << 30, std::time::Duration::from_secs(900))
        .and_then(|()| {
            crate::macos::with_mounted_dmg(&dmg, &mount, || {
                let app = mount.join("Firefox.app");
                crate::macos::verify_signed_by(
                    &app,
                    "Developer ID Application: Mozilla Corporation",
                )?;
                let dest_dir = if crate::util::dir_writable(Path::new("/Applications")) {
                    PathBuf::from("/Applications")
                } else {
                    let d = paths::home().join("Applications");
                    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
                    d
                };
                let dest = dest_dir.join("Firefox.app");
                let copied = Command::new("ditto")
                    .arg(&app)
                    .arg(&dest)
                    .status()
                    .is_ok_and(|s| s.success());
                if !copied {
                    return Err(format!("could not copy Firefox to {}", dest_dir.display()));
                }
                Ok(Firefox {
                    program: dest.join("Contents/MacOS/firefox"),
                    flavor: Flavor::Native,
                })
            })
        });
    if let Err(e) = &result {
        ui::error("Whisper", &format!("Firefox could not be installed: {e}"));
    }
    result
}

#[cfg(windows)]
pub fn install() -> Result<Firefox, String> {
    const URL: &str =
        "https://download.mozilla.org/?product=firefox-latest-ssl&os=win64&lang=en-US";
    use crate::ui;
    if paths::noninteractive() {
        return Err("Firefox is not installed".into());
    }
    let ask = ui::ask(
        "Whisper",
        "Whisper runs on Firefox, which is not installed.\n\nDownload Firefox from mozilla.org and install it now? \
         This takes a minute; Whisper starts when it is done.",
        "Yes",
        "No",
    );
    if ask != Some(true) {
        return Err("Firefox is not installed".into());
    }
    let tmp = crate::util::TempDir::new("firefox")?;
    let setup = tmp.path().join("Firefox Setup.exe");
    let result = (|| {
        crate::net::download(URL, &setup, 1 << 30, std::time::Duration::from_secs(900))?;
        crate::winapi::verify_authenticode(&setup, "Mozilla Corporation")?;
        let ok = Command::new(&setup)
            .arg("/S")
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return Err("the Firefox installer did not finish".to_string());
        }
        find_installed().ok_or_else(|| "Firefox was installed but cannot be found".to_string())
    })();
    if let Err(e) = &result {
        ui::error("Whisper", &format!("Firefox could not be installed: {e}"));
    }
    result
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn install() -> Result<Firefox, String> {
    Err("Firefox is not installed".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn passes_profile_extra_and_user_arguments_in_order() {
        let ff = Firefox {
            program: PathBuf::from("firefox"),
            flavor: Flavor::Native,
        };
        let user = [
            OsString::from("https://example.org"),
            OsString::from("two words"),
        ];
        let cmd = command(&ff, Path::new("/p"), &["--migration"], &user);
        let mut expected = vec![];
        if cfg!(target_os = "linux") {
            expected.extend(["--class", "Whisper", "--name", "Whisper"]);
        }
        expected.extend([
            "--profile",
            "/p",
            "--migration",
            "https://example.org",
            "two words",
        ]);
        assert_eq!(args(&cmd), expected);
    }

    #[test]
    fn flatpak_runs_through_flatpak() {
        let ff = Firefox {
            program: PathBuf::from("/usr/bin/flatpak"),
            flavor: Flavor::Flatpak,
        };
        let cmd = command(&ff, Path::new("/p"), &[], &[]);
        assert_eq!(&args(&cmd)[..2], ["run", FLATPAK_ID]);
    }

    #[test]
    fn drops_macos_process_serial_numbers() {
        let ff = Firefox {
            program: PathBuf::from("firefox"),
            flavor: Flavor::Native,
        };
        let cmd = command(&ff, Path::new("/p"), &[], &[OsString::from("-psn_0_12345")]);
        assert!(!args(&cmd).iter().any(|a| a.starts_with("-psn_")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_ubuntu_snap_wrapper() {
        let tmp = tempfile::tempdir().unwrap();
        let wrapper = tmp.path().join("firefox");
        std::fs::write(&wrapper, "#!/bin/sh\nexec /snap/bin/firefox \"$@\"\n").unwrap();
        assert_eq!(flavor_of(&wrapper), Flavor::Snap);
        let plain = tmp.path().join("plain");
        std::fs::write(&plain, "#!/bin/sh\nexec /usr/lib/firefox/firefox \"$@\"\n").unwrap();
        assert_eq!(flavor_of(&plain), Flavor::Native);
        assert_eq!(flavor_of(Path::new("/snap/bin/firefox")), Flavor::Snap);
    }
}
