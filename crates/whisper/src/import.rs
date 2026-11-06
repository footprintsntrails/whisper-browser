//! Bringing bookmarks, history and passwords over from another browser. Everything is
//! copied locally; nothing is uploaded anywhere.
//!
//! * **Firefox family** (Firefox, Zen, LibreWolf, Floorp, Waterfox): profiles have the
//!   same format as Whisper's, so the data files are copied from the browser's default
//!   profile (found through its `profiles.ini`) into a new Whisper profile, only while
//!   that browser is closed. Settings and extensions are never copied.
//! * **Chrome, Edge, Brave, Opera, Vivaldi, Chromium and Safari**: Firefox's own import
//!   wizard handles these. Starting Firefox with `--migration` opens it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::paths;

/// Files that hold bookmarks and history, passwords, and form history.
const DATA_FILES: &[&str] = &[
    "places.sqlite",
    "favicons.sqlite",
    "logins.json",
    "key4.db",
    "formhistory.sqlite",
];
const COOKIE_FILES: &[&str] = &["cookies.sqlite"];

/// Where to look for a browser's `profiles.ini`.
pub struct Ctx {
    pub home: PathBuf,
    pub appdata: PathBuf,
    pub local_appdata: PathBuf,
    pub xdg_config: PathBuf,
}

impl Ctx {
    pub fn from_env() -> Ctx {
        let home = paths::home();
        Ctx {
            appdata: paths::env_path("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming")),
            local_appdata: paths::env_path("LOCALAPPDATA")
                .unwrap_or_else(|| home.join("AppData/Local")),
            xdg_config: paths::xdg_config_home(),
            home,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// A Firefox-family browser: key (for `WHISPER_IMPORT`), name, and profile roots.
struct Family {
    key: &'static str,
    name: &'static str,
    linux: &'static [&'static str],
    mac: &'static [&'static str],
    windows: &'static [&'static str],
}

// Linux paths start at $HOME, or at $XDG_CONFIG_HOME when they begin with "@config/".
// macOS paths start at ~/Library/Application Support; Windows paths at %APPDATA%.
const FAMILY: &[Family] = &[
    Family {
        key: "firefox",
        name: "Firefox",
        linux: &[
            ".mozilla/firefox",
            "@config/mozilla/firefox",
            "snap/firefox/common/.mozilla/firefox",
            ".var/app/org.mozilla.firefox/.mozilla/firefox",
            ".var/app/org.mozilla.firefox/config/mozilla/firefox",
        ],
        mac: &["Firefox"],
        windows: &["Mozilla/Firefox"],
    },
    Family {
        key: "zen",
        name: "Zen",
        linux: &[".zen", "@config/zen", ".var/app/app.zen_browser.zen/.zen"],
        mac: &["zen"],
        windows: &["zen"],
    },
    Family {
        key: "librewolf",
        name: "LibreWolf",
        linux: &[
            ".librewolf",
            "@config/librewolf/librewolf",
            ".var/app/io.gitlab.librewolf-community/.librewolf",
        ],
        mac: &["librewolf"],
        windows: &["librewolf"],
    },
    Family {
        key: "floorp",
        name: "Floorp",
        linux: &[".floorp", ".var/app/one.ablaze.floorp/.floorp"],
        mac: &["Floorp"],
        windows: &["Floorp"],
    },
    Family {
        key: "waterfox",
        name: "Waterfox",
        // Waterfox's vendor is BrowserWorks; builds without a fixed profile folder use it
        linux: &[
            ".waterfox",
            ".browserworks/waterfox",
            ".var/app/net.waterfox.waterfox/.waterfox",
        ],
        mac: &["Waterfox"],
        windows: &["Waterfox", "BrowserWorks/Waterfox"],
    },
];

/// A browser Firefox's import wizard can read, found by the folder it keeps its data in.
/// Linux paths are relative to $XDG_CONFIG_HOME, macOS paths to ~/Library/Application
/// Support, Windows paths to %LOCALAPPDATA% (or %APPDATA% with "@roaming/").
struct WizardBrowser {
    name: &'static str,
    linux: &'static [&'static str],
    mac: &'static [&'static str],
    windows: &'static [&'static str],
}

const WIZARD: &[WizardBrowser] = &[
    WizardBrowser {
        name: "Chrome",
        linux: &[
            "google-chrome",
            "../.var/app/com.google.Chrome/config/google-chrome",
        ],
        mac: &["Google/Chrome"],
        windows: &["Google/Chrome/User Data"],
    },
    WizardBrowser {
        name: "Edge",
        linux: &[
            "microsoft-edge",
            "../.var/app/com.microsoft.Edge/config/microsoft-edge",
        ],
        mac: &["Microsoft Edge"],
        windows: &["Microsoft/Edge/User Data"],
    },
    WizardBrowser {
        name: "Brave",
        linux: &[
            "BraveSoftware/Brave-Browser",
            "../.var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser",
        ],
        mac: &["BraveSoftware/Brave-Browser"],
        windows: &["BraveSoftware/Brave-Browser/User Data"],
    },
    WizardBrowser {
        name: "Opera",
        linux: &["opera"],
        mac: &["com.operasoftware.Opera"],
        windows: &["@roaming/Opera Software/Opera Stable"],
    },
    WizardBrowser {
        name: "Vivaldi",
        linux: &["vivaldi"],
        mac: &["Vivaldi"],
        windows: &["Vivaldi/User Data"],
    },
    WizardBrowser {
        name: "Chromium",
        linux: &[
            "chromium",
            "../snap/chromium/common/chromium",
            "../.var/app/org.chromium.Chromium/config/chromium",
        ],
        mac: &["Chromium"],
        windows: &["Chromium/User Data"],
    },
    WizardBrowser {
        name: "Safari",
        linux: &[],
        mac: &["../Safari"],
        windows: &[],
    },
];

/// A Firefox-family profile that can be imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub key: &'static str,
    pub name: String,
    pub profile: PathBuf,
}

/// Firefox-family profiles found on this machine, newest data first per browser.
pub fn find_sources(ctx: &Ctx, os: Os) -> Vec<Source> {
    let mut found = Vec::new();
    for family in FAMILY {
        let roots: Vec<(PathBuf, &str)> = match os {
            Os::Linux => family
                .linux
                .iter()
                .map(|p| {
                    let path = match p.strip_prefix("@config/") {
                        Some(rest) => ctx.xdg_config.join(rest),
                        None => ctx.home.join(p),
                    };
                    let label = if p.starts_with("snap/") {
                        " (Snap)"
                    } else if p.starts_with(".var/app/") {
                        " (Flatpak)"
                    } else {
                        ""
                    };
                    (path, label)
                })
                .collect(),
            Os::Mac => family
                .mac
                .iter()
                .map(|p| (ctx.home.join("Library/Application Support").join(p), ""))
                .collect(),
            Os::Windows => family
                .windows
                .iter()
                .map(|p| (ctx.appdata.join(p), ""))
                .collect(),
        };
        for (root, label) in roots {
            if let Some(profile) = default_profile(&root)
                && !found
                    .iter()
                    .any(|s: &Source| same_path(&s.profile, &profile))
            {
                found.push(Source {
                    key: family.key,
                    name: format!("{}{label}", family.name),
                    profile,
                });
            }
        }
    }
    found
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Names of installed browsers that Firefox's import wizard can read.
pub fn wizard_browsers(ctx: &Ctx, os: Os) -> Vec<&'static str> {
    WIZARD
        .iter()
        .filter(|b| {
            let paths: Vec<PathBuf> = match os {
                Os::Linux => b.linux.iter().map(|p| ctx.xdg_config.join(p)).collect(),
                Os::Mac => b
                    .mac
                    .iter()
                    .map(|p| ctx.home.join("Library/Application Support").join(p))
                    .collect(),
                Os::Windows => b
                    .windows
                    .iter()
                    .map(|p| match p.strip_prefix("@roaming/") {
                        Some(rest) => ctx.appdata.join(rest),
                        None => ctx.local_appdata.join(p),
                    })
                    .collect(),
            };
            paths.iter().any(|p| p.is_dir())
        })
        .map(|b| b.name)
        .collect()
}

#[derive(Debug, Default)]
struct IniSection {
    name: String,
    values: Vec<(String, String)>,
}

impl IniSection {
    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

fn parse_ini(text: &str) -> Vec<IniSection> {
    let mut sections: Vec<IniSection> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push(IniSection {
                name: name.trim().to_string(),
                values: Vec::new(),
            });
        } else if let (Some((key, value)), Some(section)) =
            (line.split_once('='), sections.last_mut())
        {
            section
                .values
                .push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    sections
}

/// The profiles listed in a `profiles.ini`, default ones first: the per-install default
/// (`[Install…] Default=`), then `Default=1`, then the rest in file order.
pub fn profiles_from_ini(text: &str, root: &Path) -> Vec<PathBuf> {
    let sections = parse_ini(text);
    let mut relative_of = Vec::new();
    let mut ordered: Vec<(u8, PathBuf)> = Vec::new();
    for s in &sections {
        if s.name.starts_with("Profile")
            && let Some(path) = s.get("Path")
        {
            let relative = s.get("IsRelative") != Some("0");
            relative_of.push((path.to_string(), relative));
            let rank = if s.get("Default") == Some("1") { 1 } else { 2 };
            ordered.push((rank, resolve(root, path, relative)));
        }
    }
    for s in &sections {
        if s.name.starts_with("Install")
            && let Some(path) = s.get("Default")
        {
            let relative = relative_of
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, r)| *r)
                .unwrap_or(!Path::new(path).is_absolute());
            ordered.push((0, resolve(root, path, relative)));
        }
    }
    ordered.sort_by_key(|(rank, _)| *rank);
    let mut out: Vec<PathBuf> = Vec::new();
    for (_, p) in ordered {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

fn resolve(root: &Path, path: &str, relative: bool) -> PathBuf {
    if relative {
        path.split(['/', '\\'])
            .filter(|c| !c.is_empty())
            .fold(root.to_path_buf(), |acc, c| acc.join(c))
    } else {
        PathBuf::from(path)
    }
}

/// The profile to import from in a browser's profile folder: the first default profile that
/// has bookmarks and history, or else the one whose history was used most recently.
pub fn default_profile(root: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(root.join("profiles.ini")).ok()?;
    let candidates: Vec<PathBuf> = profiles_from_ini(&text, root)
        .into_iter()
        .filter(|p| p.join("places.sqlite").is_file())
        .collect();
    let first = candidates.first()?.clone();
    let newest = candidates
        .iter()
        .filter_map(|p| {
            let modified = fs::metadata(p.join("places.sqlite"))
                .ok()?
                .modified()
                .ok()?;
            Some((modified, p.clone()))
        })
        .max_by_key(|(m, _)| *m)
        .map(|(_, p)| p);
    // The listed default wins unless another profile is clearly the one in use.
    match newest {
        Some(n) if n != first && modified_secs(&n) > modified_secs(&first) + 86_400 => Some(n),
        _ => Some(first),
    }
}

fn modified_secs(profile: &Path) -> u64 {
    fs::metadata(profile.join("places.sqlite"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The files that will be copied from `profile`, with their SQLite `-wal` files.
pub fn files_to_copy(profile: &Path, cookies: bool) -> Vec<String> {
    let names = DATA_FILES
        .iter()
        .chain(if cookies { COOKIE_FILES } else { &[] });
    let mut out = Vec::new();
    for name in names {
        if profile.join(name).is_file() {
            out.push(name.to_string());
            let wal = format!("{name}-wal");
            if profile.join(&wal).is_file() {
                out.push(wal);
            }
        }
    }
    out
}

/// Copies the data files from `source` into the new Whisper profile `dest`.
pub fn copy_profile(source: &Path, dest: &Path, cookies: bool) -> io::Result<Vec<String>> {
    let files = files_to_copy(source, cookies);
    for name in &files {
        let target = dest.join(name);
        let staged = crate::util::sibling(&target, "import");
        fs::copy(source.join(name), &staged)?;
        fs::rename(&staged, &target)?;
    }
    Ok(files)
}

/// What the user picked.
#[derive(Debug, PartialEq, Eq)]
pub enum Choice {
    Nothing,
    Copy { source: Source, cookies: bool },
    Wizard,
}

/// Reads the choice from `WHISPER_IMPORT` (`none`, `wizard` or a browser key such as `zen`)
/// and `WHISPER_IMPORT_COOKIES=1`, for scripted installs and tests.
pub fn choice_from_env(sources: &[Source]) -> Option<Choice> {
    let wanted = std::env::var("WHISPER_IMPORT").ok()?;
    let wanted = wanted.trim().to_ascii_lowercase();
    Some(match wanted.as_str() {
        "" | "none" | "no" => Choice::Nothing,
        "wizard" | "chrome" | "chromium" | "edge" | "brave" | "opera" | "vivaldi" | "safari" => {
            Choice::Wizard
        }
        key => match sources.iter().find(|s| s.key == key) {
            Some(source) => Choice::Copy {
                source: source.clone(),
                cookies: paths::env_flag("WHISPER_IMPORT_COOKIES"),
            },
            None => {
                crate::warn!("WHISPER_IMPORT={key}: no {key} profile found");
                Choice::Nothing
            }
        },
    })
}

/// Asks the user what to import. `None` when there is nothing to offer or no way to ask.
pub fn ask(sources: &[Source], wizard: &[&str], on_demand: bool) -> Choice {
    if paths::noninteractive() {
        return Choice::Nothing;
    }
    let mut items: Vec<String> = sources
        .iter()
        .map(|s| format!("{}: bookmarks, history and passwords", s.name))
        .collect();
    let wizard_offered = !wizard.is_empty() || on_demand;
    if wizard_offered {
        let names = if wizard.is_empty() {
            "Chrome, Edge, Safari and others".to_string()
        } else {
            wizard.join(", ")
        };
        items.push(format!("{names} (opens Firefox's import window)"));
    }
    if items.is_empty() {
        return Choice::Nothing;
    }
    items.push("Don't import anything".to_string());
    let prompt = "Bring your bookmarks, history and passwords into Whisper?\n\
                  Everything is copied on this computer; nothing is uploaded.";
    let Some(index) = crate::ui::choose("Import into Whisper", prompt, &items) else {
        return Choice::Nothing;
    };
    if let Some(source) = sources.get(index) {
        let cookies = crate::ui::ask(
            "Import into Whisper",
            &format!(
                "Also bring over cookies from {}? You would stay signed in to sites, but sites \
                 could also recognise you as before.",
                source.name
            ),
            "Bring cookies",
            "Skip cookies",
        ) == Some(true);
        return Choice::Copy {
            source: source.clone(),
            cookies,
        };
    }
    if wizard_offered && index == sources.len() {
        return Choice::Wizard;
    }
    Choice::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "\
[Profile1]
Name=old
IsRelative=1
Path=Profiles/old.default

[Install4F96D1932A9F858E]
Default=Profiles/abcd.default-release
Locked=1

[Profile0]
Name=default-release
IsRelative=1
Path=Profiles/abcd.default-release

[Profile2]
Name=elsewhere
IsRelative=0
Path=/data/ff/elsewhere
Default=1

[General]
StartWithLastProfile=1
Version=2
";

    #[test]
    fn install_default_comes_first_then_default_flag_then_the_rest() {
        let root = Path::new("/r/.mozilla/firefox");
        let profiles = profiles_from_ini(INI, root);
        assert_eq!(
            profiles,
            vec![
                root.join("Profiles").join("abcd.default-release"),
                PathBuf::from("/data/ff/elsewhere"),
                root.join("Profiles").join("old.default"),
            ]
        );
    }

    #[test]
    fn handles_bom_crlf_and_comments() {
        let text = "\u{feff}; comment\r\n[Profile0]\r\nName=a\r\nIsRelative=1\r\nPath=Profiles\\x.default\r\nDefault=1\r\n";
        let root = Path::new("/r");
        assert_eq!(
            profiles_from_ini(text, root),
            vec![root.join("Profiles").join("x.default")]
        );
    }

    fn fake_profile(dir: &Path, with: &[&str]) {
        fs::create_dir_all(dir).unwrap();
        for f in with {
            fs::write(dir.join(f), f.as_bytes()).unwrap();
        }
    }

    #[test]
    fn picks_files_and_wal_and_cookies_only_when_asked() {
        let tmp = tempfile::tempdir().unwrap();
        fake_profile(
            tmp.path(),
            &[
                "places.sqlite",
                "places.sqlite-wal",
                "places.sqlite-shm",
                "logins.json",
                "key4.db",
                "cookies.sqlite",
                "cookies.sqlite-wal",
                "prefs.js",
                "extensions.json",
            ],
        );
        assert_eq!(
            files_to_copy(tmp.path(), false),
            [
                "places.sqlite",
                "places.sqlite-wal",
                "logins.json",
                "key4.db"
            ]
        );
        assert_eq!(
            files_to_copy(tmp.path(), true),
            [
                "places.sqlite",
                "places.sqlite-wal",
                "logins.json",
                "key4.db",
                "cookies.sqlite",
                "cookies.sqlite-wal"
            ]
        );
    }

    #[test]
    fn copies_into_the_new_profile_without_prefs_or_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        fake_profile(
            &src,
            &[
                "places.sqlite",
                "favicons.sqlite",
                "formhistory.sqlite",
                "prefs.js",
            ],
        );
        fs::create_dir_all(src.join("extensions")).unwrap();
        fs::create_dir_all(&dest).unwrap();
        let copied = copy_profile(&src, &dest, false).unwrap();
        assert_eq!(
            copied,
            ["places.sqlite", "favicons.sqlite", "formhistory.sqlite"]
        );
        assert_eq!(
            fs::read(dest.join("places.sqlite")).unwrap(),
            b"places.sqlite"
        );
        assert!(!dest.join("prefs.js").exists());
        assert!(!dest.join("extensions").exists());
    }

    fn ctx(home: &Path) -> Ctx {
        Ctx {
            home: home.to_path_buf(),
            appdata: home.join("AppData/Roaming"),
            local_appdata: home.join("AppData/Local"),
            xdg_config: home.join(".config"),
        }
    }

    fn browser_root(home: &Path, os: Os, rel: &str) -> PathBuf {
        match os {
            Os::Linux => home.join(rel),
            Os::Mac => home.join("Library/Application Support").join(rel),
            Os::Windows => home.join("AppData/Roaming").join(rel),
        }
    }

    #[test]
    fn finds_family_profiles_per_os() {
        for (os, zen, firefox) in [
            (Os::Linux, ".zen", ".mozilla/firefox"),
            (Os::Mac, "zen", "Firefox"),
            (Os::Windows, "zen", "Mozilla/Firefox"),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path();
            for rel in [zen, firefox] {
                let root = browser_root(home, os, rel);
                fake_profile(&root.join("Profiles/p.default"), &["places.sqlite"]);
                fs::write(
                    root.join("profiles.ini"),
                    "[Profile0]\nName=p\nIsRelative=1\nPath=Profiles/p.default\nDefault=1\n",
                )
                .unwrap();
            }
            // A Firefox-family folder without places.sqlite is skipped
            let empty = browser_root(home, os, if os == Os::Linux { ".floorp" } else { "Floorp" });
            fs::create_dir_all(empty.join("Profiles/q")).unwrap();
            fs::write(empty.join("profiles.ini"), "[Profile0]\nPath=Profiles/q\n").unwrap();

            let found = find_sources(&ctx(home), os);
            let keys: Vec<&str> = found.iter().map(|s| s.key).collect();
            assert_eq!(keys, ["firefox", "zen"], "{os:?}");
            assert!(found[1].profile.ends_with("Profiles/p.default"));
        }
    }

    #[test]
    fn labels_snap_and_flatpak_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        for rel in [
            "snap/firefox/common/.mozilla/firefox",
            ".var/app/org.mozilla.firefox/.mozilla/firefox",
        ] {
            let root = home.join(rel);
            fake_profile(&root.join("p"), &["places.sqlite"]);
            fs::write(root.join("profiles.ini"), "[Profile0]\nPath=p\n").unwrap();
        }
        let names: Vec<String> = find_sources(&ctx(home), Os::Linux)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["Firefox (Snap)", "Firefox (Flatpak)"]);
    }

    #[test]
    fn detects_wizard_browsers() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        fs::create_dir_all(home.join(".config/google-chrome")).unwrap();
        fs::create_dir_all(home.join("Library/Application Support/BraveSoftware/Brave-Browser"))
            .unwrap();
        fs::create_dir_all(home.join("Library/Safari")).unwrap();
        fs::create_dir_all(home.join("AppData/Local/Microsoft/Edge/User Data")).unwrap();
        fs::create_dir_all(home.join("AppData/Roaming/Opera Software/Opera Stable")).unwrap();
        let c = ctx(home);
        assert_eq!(wizard_browsers(&c, Os::Linux), ["Chrome"]);
        assert_eq!(wizard_browsers(&c, Os::Mac), ["Brave", "Safari"]);
        assert_eq!(wizard_browsers(&c, Os::Windows), ["Edge", "Opera"]);
    }
}
