//! Checks a Whisper that was installed from a real package (the .deb, .rpm, Linux tarball,
//! Whisper-Setup.exe or Whisper.app). CI installs the package, then runs this with:
//!
//!   WHISPER_INSTALLED_BIN    the installed whisper program
//!   WHISPER_INSTALLED_SHARE  its share folder
//!   WHISPER_FAKE_FIREFOX     a test build of whisper (feature test-hooks) named firefox,
//!                            which records its arguments instead of starting a browser
//!   WHISPER_TARBALL_DIR      (optional) an unpacked whisper-linux tarball, to test
//!                            `--install` and `--uninstall --purge`
//!
//! Without these variables the tests do nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A relative path written with `/`, turned into the platform's separators (so paths
/// compare equal to the ones Whisper builds, also on Windows).
fn rel(path: &str) -> PathBuf {
    path.split('/').collect()
}

struct Env {
    bin: PathBuf,
    share: PathBuf,
    fake_firefox: PathBuf,
}

fn env() -> Option<Env> {
    let get = |k: &str| std::env::var_os(k).map(PathBuf::from);
    Some(Env {
        bin: get("WHISPER_INSTALLED_BIN")?,
        share: get("WHISPER_INSTALLED_SHARE")?,
        fake_firefox: get("WHISPER_FAKE_FIREFOX")?,
    })
}

struct Home {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    args: PathBuf,
}

impl Home {
    fn new() -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        Home {
            args: tmp.path().join("args.txt"),
            home,
            _tmp: tmp,
        }
    }

    fn command(&self, e: &Env, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy().to_string();
            if key.starts_with("WHISPER_")
                || key.starts_with("XDG_")
                || key == "DISPLAY"
                || key == "WAYLAND_DISPLAY"
            {
                cmd.env_remove(key);
            }
        }
        cmd.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("APPDATA", self.home.join(rel("AppData/Roaming")))
            .env("LOCALAPPDATA", self.home.join(rel("AppData/Local")))
            .env("WHISPER_NONINTERACTIVE", "1")
            .env("WHISPER_OFFLINE", "1")
            .env("WHISPER_TEST_ARGS_FILE", &self.args);
        if cfg!(target_os = "linux") {
            // Found on PATH, as on a real system
            let dir = e.fake_firefox.parent().unwrap();
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut dirs = vec![dir.to_path_buf()];
            dirs.extend(std::env::split_paths(&path));
            cmd.env("PATH", std::env::join_paths(dirs).unwrap());
        } else {
            cmd.env("WHISPER_FIREFOX", &e.fake_firefox);
        }
        cmd
    }

    fn data(&self) -> PathBuf {
        if cfg!(windows) {
            self.home.join(rel("AppData/Local/Whisper"))
        } else if cfg!(target_os = "macos") {
            self.home.join(rel("Library/Application Support/Whisper"))
        } else {
            self.home.join(rel(".local/share/whisper"))
        }
    }

    /// What the fake Firefox was started with. On Windows Whisper starts Firefox and exits,
    /// so wait a little for the file.
    fn firefox_args(&self) -> Vec<String> {
        for _ in 0..100 {
            if self.args.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        fs::read_to_string(&self.args)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn zen_profile(&self) -> PathBuf {
        let root = if cfg!(windows) {
            self.home.join(rel("AppData/Roaming/zen"))
        } else if cfg!(target_os = "macos") {
            self.home.join(rel("Library/Application Support/zen"))
        } else {
            self.home.join(".zen")
        };
        let profile = root.join(rel("Profiles/x.Default (release)"));
        fs::create_dir_all(&profile).unwrap();
        for f in ["places.sqlite", "logins.json", "key4.db", "prefs.js"] {
            fs::write(profile.join(f), format!("zen {f}")).unwrap();
        }
        fs::write(
            root.join("profiles.ini"),
            "[Profile0]\nName=Default (release)\nIsRelative=1\nPath=Profiles/x.Default (release)\nDefault=1\n",
        )
        .unwrap();
        profile
    }
}

fn check_first_run(e: &Env, program: &Path) {
    let h = Home::new();
    h.zen_profile();
    let out = h
        .command(e, program)
        .env("WHISPER_IMPORT", "zen")
        .args(["https://example.org", "two words"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let profile = h.data().join("Profile");
    assert_eq!(
        fs::read(profile.join("user.js")).unwrap(),
        fs::read(e.share.join(rel("profile/user.js"))).unwrap()
    );
    assert_eq!(
        fs::read(profile.join("prefs.js")).unwrap(),
        fs::read(e.share.join(rel("profile/first-run-prefs.js"))).unwrap()
    );
    assert!(profile.join(rel("chrome/userChrome.css")).is_file());
    assert!(profile.join("extensions").is_dir());
    assert_eq!(
        fs::read_to_string(profile.join("places.sqlite")).unwrap(),
        "zen places.sqlite"
    );
    assert_eq!(
        fs::read_to_string(profile.join("key4.db")).unwrap(),
        "zen key4.db"
    );

    let args = h.firefox_args();
    let profile_arg = profile.to_string_lossy().into_owned();
    let tail: Vec<&str> = args.iter().map(String::as_str).skip(1).collect();
    let mut expected: Vec<&str> = Vec::new();
    if cfg!(target_os = "linux") {
        expected.extend(["--class", "Whisper", "--name", "Whisper"]);
    }
    expected.extend([
        "--profile",
        &profile_arg,
        "https://example.org",
        "two words",
    ]);
    assert_eq!(tail, expected);

    // The import wizard for Chrome, Edge, Safari and the like
    let h = Home::new();
    let out = h
        .command(e, program)
        .env("WHISPER_IMPORT", "wizard")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(h.firefox_args().contains(&"--migration".to_string()));
}

#[test]
fn installed_program_sets_up_imports_and_starts_firefox() {
    let Some(e) = env() else {
        eprintln!("WHISPER_INSTALLED_BIN is not set; skipping");
        return;
    };
    let out = Command::new(&e.bin).arg("--version").output().unwrap();
    let version = String::from_utf8_lossy(&out.stdout);
    let expected = fs::read_to_string(e.share.join("VERSION")).unwrap();
    // A Windows GUI program has no console of its own, so --version may print nothing there
    if !cfg!(windows) {
        assert_eq!(version.trim(), format!("Whisper {}", expected.trim()));
    }
    check_first_run(&e, &e.bin);
}

#[cfg(unix)]
#[test]
fn tarball_installs_into_the_home_folder_and_uninstalls() {
    let (Some(e), Some(dir)) = (
        env(),
        std::env::var_os("WHISPER_TARBALL_DIR").map(PathBuf::from),
    ) else {
        eprintln!("WHISPER_TARBALL_DIR is not set; skipping");
        return;
    };
    let h = Home::new();
    let out = h
        .command(&e, &dir.join("whisper"))
        .arg("--install")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let app = h.home.join(rel(".local/share/whisper/app"));
    let program = app.join("whisper");
    assert!(program.is_file());
    assert!(app.join(rel("share/VERSION")).is_file());
    assert_eq!(
        fs::read_link(h.home.join(rel(".local/bin/whisper"))).unwrap(),
        program
    );
    let desktop = fs::read_to_string(
        h.home
            .join(rel(".local/share/applications/whisper.desktop")),
    )
    .unwrap();
    assert!(
        desktop.contains(&format!("Exec=\"{}\" %u", program.display())),
        "{desktop}"
    );
    assert!(desktop.contains("StartupWMClass=Whisper"));
    assert!(
        h.home
            .join(rel(".local/share/icons/hicolor/256x256/apps/whisper.png"))
            .is_file()
    );

    // The installed copy works, and keeps itself up to date (it lives in a writable folder)
    check_first_run(
        &Env {
            bin: program.clone(),
            share: app.join("share"),
            fake_firefox: e.fake_firefox.clone(),
        },
        &program,
    );

    let out = h
        .command(&e, &program)
        .args(["--uninstall", "--purge"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!app.exists());
    assert!(!h.home.join(rel(".local/bin/whisper")).exists());
    assert!(
        !h.home
            .join(rel(".local/share/applications/whisper.desktop"))
            .exists()
    );
    assert!(!h.home.join(rel(".local/share/whisper")).exists());
}
