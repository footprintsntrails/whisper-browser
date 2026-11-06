//! End-to-end tests of the `whisper` program. A copy of the test build named `firefox`
//! (or `flatpak`) records the arguments it was started with instead of starting a browser,
//! and a small HTTP server on 127.0.0.1 stands in for addons.mozilla.org and GitHub.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::{EncodePublicKey, LineEnding};
use rsa::signature::{SignatureEncoding, Signer};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};

/// A relative path written with `/`, turned into the platform's separators (so paths
/// compare equal to the ones Whisper builds, also on Windows).
fn rel(path: &str) -> PathBuf {
    path.split('/').collect()
}

const EXE: &str = env!("CARGO_BIN_EXE_whisper");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const UBO: &[u8] = b"PK\x03\x04 pretend this is uBlock Origin";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel("../.."))
}

// ---------------------------------------------------------------------------------------
// A tiny HTTP server

#[derive(Clone)]
struct Server {
    base: String,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = Server {
            base,
            files: Arc::default(),
            hits: Arc::default(),
        };
        let s = server.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&request);
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                s.hits.lock().unwrap().push(path.clone());
                let body = s.files.lock().unwrap().get(&path).cloned();
                let response = match body {
                    Some(body) => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        r.extend_from_slice(&body);
                        r
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&response);
            }
        });
        server
    }

    fn serve(&self, path: &str, body: &[u8]) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), body.to_vec());
    }

    fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------------------
// A fake machine: home folder, Whisper share folder, fake Firefox

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    share: PathBuf,
    bin: PathBuf,
    args_file: PathBuf,
    server: Server,
}

fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();

        // The share folder, built from the repository like the release does
        let share = root.join("share");
        copy_dir(&repo().join("profile"), &share.join("profile"));
        copy_dir(
            &repo().join(rel("assets/icon")),
            &share.join(rel("assets/icon")),
        );
        fs::write(share.join("VERSION"), format!("{VERSION}\n")).unwrap();

        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::copy(EXE, bin.join(exe_name("firefox"))).unwrap();

        let server = Server::start();
        server.serve("/ubo.xpi", UBO);
        World {
            _tmp: tmp,
            args_file: root.join("firefox-args.txt"),
            root,
            home,
            share,
            bin,
            server,
        }
    }

    fn firefox(&self) -> PathBuf {
        self.bin.join(exe_name("firefox"))
    }

    /// The command with a clean, predictable environment.
    fn whisper(&self, exe: &Path) -> Command {
        let mut cmd = Command::new(exe);
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
            .env("XDG_DATA_HOME", self.home.join(rel(".local/share")))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("WHISPER_SHARE", &self.share)
            .env("WHISPER_NONINTERACTIVE", "1")
            .env("WHISPER_TEST_NO_BACKGROUND", "1")
            .env("WHISPER_TEST_ARGS_FILE", &self.args_file)
            .env(
                "WHISPER_TEST_UBO_URL",
                format!("{}/ubo.xpi", self.server.base),
            );
        if cfg!(target_os = "linux") {
            // Linux: found on PATH like a real install
            cmd.env("PATH", &self.bin);
        } else {
            cmd.env("WHISPER_FIREFOX", self.firefox());
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

    fn profile(&self) -> PathBuf {
        self.data().join("Profile")
    }

    /// What the fake Firefox was started with. On Windows Whisper starts Firefox and exits,
    /// so wait a little for the file.
    fn firefox_args(&self) -> Vec<String> {
        for _ in 0..100 {
            if self.args_file.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        fs::read_to_string(&self.args_file)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    /// Creates a closed Firefox-family profile (`folder` is the browser folder for this OS).
    fn make_source(&self, folder: &str, files: &[&str]) -> PathBuf {
        let root = if cfg!(windows) {
            self.home.join(rel("AppData/Roaming")).join(folder)
        } else if cfg!(target_os = "macos") {
            self.home
                .join(rel("Library/Application Support"))
                .join(folder)
        } else {
            self.home.join(folder)
        };
        let profile = root.join(rel("Profiles/abc.default-release"));
        fs::create_dir_all(&profile).unwrap();
        for f in files {
            fs::write(profile.join(f), format!("from {folder}: {f}")).unwrap();
        }
        fs::write(
            root.join("profiles.ini"),
            "[Install1234]\nDefault=Profiles/abc.default-release\n\n[Profile0]\nName=default-release\n\
             IsRelative=1\nPath=Profiles/abc.default-release\n",
        )
        .unwrap();
        profile
    }
}

fn copy_dir(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dest.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn run(cmd: &mut Command) -> Output {
    let out = cmd.output().unwrap();
    if !out.status.success() {
        eprintln!(
            "exit {:?}\nstdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    out
}

fn expected_args(world: &World, program: &str, profile: &Path, rest: &[&str]) -> Vec<String> {
    let mut args = vec![program.to_string()];
    if cfg!(target_os = "linux") {
        args.extend(["--class", "Whisper", "--name", "Whisper"].map(String::from));
    }
    args.push("--profile".into());
    args.push(profile.to_string_lossy().into_owned());
    args.extend(rest.iter().map(|s| s.to_string()));
    let _ = world;
    args
}

// ---------------------------------------------------------------------------------------

#[test]
fn first_run_sets_up_the_profile_and_passes_arguments() {
    let w = World::new();
    let out = run(w
        .whisper(Path::new(EXE))
        .args(["https://example.org", "two words"]));
    assert!(out.status.success());

    assert_eq!(
        w.firefox_args(),
        expected_args(
            &w,
            "firefox",
            &w.profile(),
            &["https://example.org", "two words"]
        )
    );
    let p = w.profile();
    assert_eq!(
        fs::read(p.join("user.js")).unwrap(),
        fs::read(w.share.join(rel("profile/user.js"))).unwrap()
    );
    assert_eq!(
        fs::read(p.join("prefs.js")).unwrap(),
        fs::read(w.share.join(rel("profile/first-run-prefs.js"))).unwrap()
    );
    assert!(p.join(rel("chrome/userChrome.css")).is_file());
    assert!(p.join(rel("chrome/userContent.css")).is_file());
    assert!(p.join(rel("chrome/fonts/Inter-Regular.woff2")).is_file());
    assert_eq!(
        fs::read(p.join(rel("extensions/uBlock0@raymondhill.net.xpi"))).unwrap(),
        UBO
    );
    assert!(!w.data().join("ubo-pending").exists());
    assert!(w.data().join("whisper.log").is_file());
    assert_eq!(w.server.hits(), ["/ubo.xpi"]);
}

#[test]
fn later_runs_keep_prefs_and_apply_overrides() {
    let w = World::new();
    assert!(run(&mut w.whisper(Path::new(EXE))).status.success());
    let p = w.profile();
    fs::write(p.join("prefs.js"), "// Firefox's own state\n").unwrap();
    fs::write(p.join("user.js"), "// edited\n").unwrap();
    fs::write(
        p.join("user-overrides.js"),
        "user_pref(\"network.trr.mode\", 3);\n",
    )
    .unwrap();

    assert!(run(&mut w.whisper(Path::new(EXE))).status.success());
    assert_eq!(
        fs::read_to_string(p.join("prefs.js")).unwrap(),
        "// Firefox's own state\n"
    );
    let user_js = fs::read_to_string(p.join("user.js")).unwrap();
    assert!(
        user_js.starts_with(&fs::read_to_string(w.share.join(rel("profile/user.js"))).unwrap())
    );
    assert!(user_js.ends_with("user_pref(\"network.trr.mode\", 3);\n\n"));
    assert_eq!(
        w.server.hits(),
        ["/ubo.xpi"],
        "uBlock Origin is downloaded once"
    );
}

#[test]
fn offline_mode_downloads_nothing_but_still_starts() {
    let w = World::new();
    let out = run(w.whisper(Path::new(EXE)).env("WHISPER_OFFLINE", "1"));
    assert!(out.status.success());
    assert!(w.profile().join("user.js").is_file());
    assert!(w.profile().join("extensions").is_dir());
    assert!(w.data().join("ubo-pending").exists());
    assert!(w.server.hits().is_empty());
    assert!(!w.firefox_args().is_empty());
}

#[test]
fn imports_a_closed_zen_profile_into_the_new_profile() {
    let w = World::new();
    let folder = if cfg!(target_os = "linux") {
        ".zen"
    } else {
        "zen"
    };
    w.make_source(
        folder,
        &[
            "places.sqlite",
            "places.sqlite-wal",
            "favicons.sqlite",
            "logins.json",
            "key4.db",
            "formhistory.sqlite",
            "cookies.sqlite",
            "prefs.js",
        ],
    );
    let out = run(w.whisper(Path::new(EXE)).env("WHISPER_IMPORT", "zen"));
    assert!(out.status.success());
    let p = w.profile();
    for f in [
        "places.sqlite",
        "places.sqlite-wal",
        "favicons.sqlite",
        "logins.json",
        "key4.db",
        "formhistory.sqlite",
    ] {
        assert_eq!(
            fs::read_to_string(p.join(f)).unwrap(),
            format!("from {folder}: {f}"),
            "{f}"
        );
    }
    assert!(
        !p.join("cookies.sqlite").exists(),
        "cookies only when asked"
    );
    assert_eq!(
        fs::read(p.join("prefs.js")).unwrap(),
        fs::read(w.share.join(rel("profile/first-run-prefs.js"))).unwrap(),
        "the source's settings are never copied"
    );
    assert!(!w.firefox_args().contains(&"--migration".to_string()));
}

#[test]
fn imports_cookies_when_asked() {
    let w = World::new();
    let folder = if cfg!(target_os = "linux") {
        ".librewolf"
    } else {
        "librewolf"
    };
    w.make_source(
        folder,
        &["places.sqlite", "cookies.sqlite", "cookies.sqlite-wal"],
    );
    let out = run(w
        .whisper(Path::new(EXE))
        .env("WHISPER_IMPORT", "librewolf")
        .env("WHISPER_IMPORT_COOKIES", "1"));
    assert!(out.status.success());
    assert!(w.profile().join("cookies.sqlite").is_file());
    assert!(w.profile().join("cookies.sqlite-wal").is_file());
}

#[test]
fn does_not_copy_from_a_running_browser() {
    let w = World::new();
    let folder = if cfg!(target_os = "linux") {
        ".zen"
    } else {
        "zen"
    };
    let source = w.make_source(folder, &["places.sqlite"]);
    let _lock = hold_profile_lock(&source);
    let out = run(w.whisper(Path::new(EXE)).env("WHISPER_IMPORT", "zen"));
    assert!(out.status.success());
    assert!(!w.profile().join("places.sqlite").exists());
}

#[test]
fn chromium_choice_opens_firefox_import_wizard() {
    let w = World::new();
    let out = run(w
        .whisper(Path::new(EXE))
        .env("WHISPER_IMPORT", "chrome")
        .arg("https://example.org"));
    assert!(out.status.success());
    assert_eq!(
        w.firefox_args(),
        expected_args(
            &w,
            "firefox",
            &w.profile(),
            &["--migration", "https://example.org"]
        )
    );
}

#[test]
fn import_on_demand_needs_a_new_profile() {
    let w = World::new();
    assert!(run(&mut w.whisper(Path::new(EXE))).status.success());
    fs::write(w.profile().join("places.sqlite"), "whisper's own history").unwrap();
    let folder = if cfg!(target_os = "linux") {
        ".zen"
    } else {
        "zen"
    };
    w.make_source(folder, &["places.sqlite"]);
    let out = run(w
        .whisper(Path::new(EXE))
        .arg("--import")
        .env("WHISPER_IMPORT", "zen"));
    assert!(out.status.success());
    assert_eq!(
        fs::read_to_string(w.profile().join("places.sqlite")).unwrap(),
        "whisper's own history"
    );
}

#[test]
fn missing_firefox_is_an_error() {
    let w = World::new();
    let empty = w.root.join("empty-bin");
    fs::create_dir_all(&empty).unwrap();
    let out = w
        .whisper(Path::new(EXE))
        .env("PATH", &empty)
        .env("WHISPER_FIREFOX", w.root.join("no-such-firefox"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!w.args_file.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn flatpak_firefox_uses_its_own_data_folder() {
    let w = World::new();
    fs::remove_file(w.firefox()).unwrap();
    fs::copy(EXE, w.bin.join("flatpak")).unwrap();
    assert!(run(&mut w.whisper(Path::new(EXE))).status.success());
    let profile = w
        .home
        .join(rel(".var/app/org.mozilla.firefox/data/whisper/Profile"));
    assert!(profile.join("user.js").is_file());
    let mut expected = vec![
        "flatpak".to_string(),
        "run".into(),
        "org.mozilla.firefox".into(),
    ];
    expected.extend(expected_args(&w, "", &profile, &[]).into_iter().skip(1));
    assert_eq!(w.firefox_args(), expected);
}

#[cfg(target_os = "linux")]
#[test]
fn snap_firefox_uses_its_own_data_folder() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    let real = w.root.join("real");
    fs::create_dir_all(&real).unwrap();
    fs::rename(w.firefox(), real.join("firefox")).unwrap();
    // Like Ubuntu's /usr/bin/firefox, a small script that starts the snap
    let wrapper = w.firefox();
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n# /snap/bin/firefox stand-in\nexec \"{}\" \"$@\"\n",
            real.join("firefox").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let mut cmd = w.whisper(Path::new(EXE));
    cmd.env("PATH", format!("{}:/usr/bin:/bin", w.bin.display()));
    assert!(run(&mut cmd).status.success());
    assert!(
        w.home
            .join(rel("snap/firefox/common/whisper/Profile/user.js"))
            .is_file()
    );
}

// ---------------------------------------------------------------------------------------
// Self-update, end to end with a throwaway signing key

struct Release {
    key: RsaPrivateKey,
    pubkey: PathBuf,
}

impl Release {
    fn new(dir: &Path) -> Release {
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        let pem = RsaPublicKey::from(&key)
            .to_public_key_pem(LineEnding::LF)
            .unwrap();
        let pubkey = dir.join("test-update-key.pub.pem");
        fs::write(&pubkey, pem).unwrap();
        Release { key, pubkey }
    }

    fn sign(&self, data: &[u8]) -> Vec<u8> {
        SigningKey::<Sha256>::new(self.key.clone())
            .sign(data)
            .to_vec()
    }
}

fn tarball_name() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "whisper-linux-arm64.tar.gz"
    } else {
        "whisper-linux.tar.gz"
    }
}

fn tarball(version: &str) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut out, flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (name, data, mode) in [
            ("whisper-linux/whisper", b"the new whisper".to_vec(), 0o755),
            (
                "whisper-linux/share/VERSION",
                format!("{version}\n").into_bytes(),
                0o644,
            ),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(mode);
            h.set_cksum();
            b.append_data(&mut h, name, data.as_slice()).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
    }
    out
}

fn manifest(version: &str, name: &str, package: &[u8]) -> Vec<u8> {
    let hash: String = Sha256::digest(package)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!(
        r#"{{"schema":1,"version":"{version}","assets":[{{"name":"{name}","sha256":"{hash}","size":{}}}]}}"#,
        package.len()
    )
    .into_bytes()
}

/// An installed copy (program and share folder) for the update to replace.
fn installed_copy(w: &World) -> PathBuf {
    let app = w.root.join("installed");
    copy_dir(&w.share, &app.join("share"));
    fs::write(app.join("whisper"), "the old whisper").unwrap();
    app
}

fn update(w: &World, release: &Release, app: &Path) -> Output {
    update_as(w, release, "WHISPER_TEST_INSTALL_DIR", app.as_os_str())
}

/// Runs `whisper --update` against the test server, as the kind of install `kind` names.
fn update_as(w: &World, release: &Release, kind: &str, value: &std::ffi::OsStr) -> Output {
    let mut cmd = w.whisper(Path::new(EXE));
    cmd.arg("--update")
        .env_remove("WHISPER_SHARE")
        .env("WHISPER_TEST_UPDATE_URL", &w.server.base)
        .env("WHISPER_TEST_PUBKEY", &release.pubkey)
        .env(kind, value)
        .env("WHISPER_FIREFOX", w.firefox());
    cmd.output().unwrap()
}

#[test]
fn update_installs_a_correctly_signed_release() {
    let w = World::new();
    let release = Release::new(&w.root);
    let app = installed_copy(&w);
    let package = tarball("9.9.9");
    let json = manifest("9.9.9", tarball_name(), &package);
    w.server.serve("/update.json", &json);
    w.server.serve("/update.json.sig", &release.sign(&json));
    w.server.serve(&format!("/{}", tarball_name()), &package);

    let out = update(&w, &release, &app);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("9.9.9 is installed"), "{stdout}");
    assert_eq!(
        fs::read_to_string(app.join("whisper")).unwrap(),
        "the new whisper"
    );
    assert_eq!(
        fs::read_to_string(app.join(rel("share/VERSION"))).unwrap(),
        "9.9.9\n"
    );
}

#[test]
fn update_rejects_a_tampered_manifest() {
    let w = World::new();
    let release = Release::new(&w.root);
    let app = installed_copy(&w);
    let package = tarball("9.9.9");
    let json = manifest("9.9.9", tarball_name(), &package);
    let signature = release.sign(&json);
    let tampered = String::from_utf8(json).unwrap().replace("9.9.9", "9.9.8");
    w.server.serve("/update.json", tampered.as_bytes());
    w.server.serve("/update.json.sig", &signature);
    w.server.serve(&format!("/{}", tarball_name()), &package);

    let out = update(&w, &release, &app);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("signature"));
    assert_eq!(
        fs::read_to_string(app.join("whisper")).unwrap(),
        "the old whisper"
    );
    assert!(
        !w.server.hits().iter().any(|h| h.ends_with(".tar.gz")),
        "nothing else is downloaded"
    );
}

#[test]
fn update_rejects_a_package_that_does_not_match_the_manifest() {
    let w = World::new();
    let release = Release::new(&w.root);
    let app = installed_copy(&w);
    let package = tarball("9.9.9");
    let json = manifest("9.9.9", tarball_name(), &package);
    w.server.serve("/update.json", &json);
    w.server.serve("/update.json.sig", &release.sign(&json));
    let mut swapped = package.clone();
    let last = swapped.len() - 1;
    swapped[last] ^= 0xff;
    w.server.serve(&format!("/{}", tarball_name()), &swapped);

    let out = update(&w, &release, &app);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("SHA-256"));
    assert_eq!(
        fs::read_to_string(app.join("whisper")).unwrap(),
        "the old whisper"
    );
}

#[test]
fn update_does_nothing_when_up_to_date() {
    let w = World::new();
    let release = Release::new(&w.root);
    let app = installed_copy(&w);
    let json = manifest(VERSION, tarball_name(), b"unused");
    w.server.serve("/update.json", &json);
    w.server.serve("/update.json.sig", &release.sign(&json));

    let out = update(&w, &release, &app);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("up to date"));
    assert_eq!(w.server.hits(), ["/update.json", "/update.json.sig"]);
}

#[test]
fn the_release_key_does_not_accept_a_test_signature() {
    let w = World::new();
    let release = Release::new(&w.root);
    let app = installed_copy(&w);
    let json = manifest("9.9.9", tarball_name(), b"x");
    w.server.serve("/update.json", &json);
    w.server.serve("/update.json.sig", &release.sign(&json));
    let mut cmd = w.whisper(Path::new(EXE));
    cmd.arg("--update")
        .env_remove("WHISPER_SHARE")
        .env("WHISPER_TEST_UPDATE_URL", &w.server.base)
        .env("WHISPER_TEST_INSTALL_DIR", &app)
        .env("WHISPER_FIREFOX", w.firefox());
    let out = cmd.output().unwrap();
    assert!(
        !out.status.success(),
        "the embedded release key must reject a test signature"
    );
}

#[cfg(target_os = "macos")]
fn make_app(app: &Path, share: &Path, version: &str) {
    copy_dir(share, &app.join(rel("Contents/Resources/share")));
    fs::write(
        app.join(rel("Contents/Resources/share/VERSION")),
        format!("{version}\n"),
    )
    .unwrap();
    fs::create_dir_all(app.join(rel("Contents/MacOS"))).unwrap();
    fs::copy(EXE, app.join(rel("Contents/MacOS/whisper"))).unwrap();
    fs::write(
        app.join(rel("Contents/Info.plist")),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\
         <key>CFBundleExecutable</key><string>whisper</string>\
         <key>CFBundleIdentifier</key><string>io.github.footprintsntrails.whisper.test</string>\
         <key>CFBundlePackageType</key><string>APPL</string></dict></plist>\n",
    )
    .unwrap();
    let signed = Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(app)
        .status()
        .unwrap();
    assert!(signed.success());
}

#[cfg(target_os = "macos")]
#[test]
fn update_swaps_in_the_app_from_a_new_disk_image() {
    let w = World::new();
    let release = Release::new(&w.root);
    let apps = w.root.join("Applications");
    let app = apps.join("Whisper.app");
    make_app(&app, &w.share, VERSION);

    let image_root = w.root.join("image");
    make_app(&image_root.join("Whisper.app"), &w.share, "9.9.9");
    let dmg = w.root.join("Whisper.dmg");
    let made = Command::new("hdiutil")
        .args([
            "create",
            "-quiet",
            "-volname",
            "Whisper",
            "-fs",
            "HFS+",
            "-format",
            "UDZO",
            "-srcfolder",
        ])
        .arg(&image_root)
        .arg(&dmg)
        .status()
        .unwrap();
    assert!(made.success());
    let package = fs::read(&dmg).unwrap();
    let json = manifest("9.9.9", "Whisper.dmg", &package);
    w.server.serve("/update.json", &json);
    w.server.serve("/update.json.sig", &release.sign(&json));
    w.server.serve("/Whisper.dmg", &package);

    let out = update_as(&w, &release, "WHISPER_TEST_INSTALL_APP", app.as_os_str());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("9.9.9 is installed"), "{stdout}");
    assert_eq!(
        fs::read_to_string(app.join(rel("Contents/Resources/share/VERSION"))).unwrap(),
        "9.9.9\n"
    );
    assert!(!apps.join("Whisper.app.new").exists());
    assert!(!apps.join("Whisper.app.old").exists());
}

#[cfg(windows)]
#[test]
fn update_runs_the_new_installer_silently() {
    let w = World::new();
    let release = Release::new(&w.root);
    // The "installer" is a copy of this test build, which records how it was started
    let package = fs::read(EXE).unwrap();
    let json = manifest("9.9.9", "Whisper-Setup.exe", &package);
    w.server.serve("/update.json", &json);
    w.server.serve("/update.json.sig", &release.sign(&json));
    w.server.serve("/Whisper-Setup.exe", &package);

    let out = update_as(
        &w,
        &release,
        "WHISPER_TEST_INSTALL_SETUP",
        std::ffi::OsStr::new("1"),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("9.9.9 is installed"), "{stdout}");
    assert_eq!(w.firefox_args(), ["Whisper-Setup", "/S"]);
}

// ---------------------------------------------------------------------------------------

/// Holds the profile lock the way Firefox does until dropped.
#[cfg(unix)]
fn hold_profile_lock(profile: &Path) -> fs::File {
    use std::os::fd::AsRawFd;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(profile.join(".parentlock"))
        .unwrap();
    // SAFETY: plain fcntl lock on a descriptor we own.
    unsafe {
        let mut lock: libc::flock = std::mem::zeroed();
        lock.l_type = libc::F_WRLCK as _;
        lock.l_whence = libc::SEEK_SET as _;
        assert_eq!(libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &lock), 0);
    }
    file
}

#[cfg(windows)]
fn hold_profile_lock(profile: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(profile.join("parent.lock"))
        .unwrap()
}
