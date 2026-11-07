//! Whisper's rules for links that open other programs ("external protocols", such as
//! `zoommtg:` or `search-ms:`).
//!
//! Two answers only (the program is never started from here: the caller shows the
//! dialog and starts it):
//!   - `Block`: never opened from a web page, without a prompt. The list follows the
//!     research notes (code_execution_defense.md, "Внешние протоколы"): Firefox's own
//!     block list, every `ms-*` scheme (Windows features that have been used to run
//!     code: `ms-msdt` "Follina", `ms-officecmd`, `ms-appinstaller`, `search-ms`), help
//!     viewers (`ghelp`, `help`, `yelp`), file sharing (`smb`, `nfs`, `webdav`, ...)
//!     and script runners (`applescript`, `osascript`, `vbscript`, ...).
//!   - `Ask`: everything else; the caller asks with the program's name and path.
//!
//! `program_from_command` takes the program out of a command line such as
//! `"/usr/bin/zoom" %u` (GIO on Linux) or `"C:\Program Files\Zoom\Zoom.exe" "%1"`.
//!
//! No unsafe code, no dependencies.

#![forbid(unsafe_code)]

/// What to do with a scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Block,
    Ask,
}

/// Schemes never opened from the web. Lower case, without ":".
const BLOCKED: &[&str] = &[
    // Firefox 159's own list (modules/libpref/init/all.js, external.* = false)
    "hcp",
    "vbscript",
    "javascript",
    "data",
    "ie.http",
    "iehistory",
    "ierss",
    "mk",
    "res",
    "search",
    "search-ms",
    "shell",
    "vnd.ms.radio",
    "disk",
    "disks",
    "afp",
    "moz-icon",
    "ttp",
    "htp",
    "ttps",
    "tps",
    "ps",
    "htps",
    "ile",
    "le",
    // Windows: search, help, installer, Office and settings handlers used in attacks
    "searchconnector-ms",
    "its",
    "mhtml",
    "jscript",
    "wscript",
    "cscript",
    "mshta",
    "calculator",
    "windowsdefender",
    "microsoft-edge-holographic",
    "appinstaller",
    // Linux and macOS help viewers (CVE-2025-3155 was Yelp through ghelp:)
    "ghelp",
    "help",
    "yelp",
    "man",
    "info",
    "x-man-page",
    // File sharing that can run or fetch files outside the browser
    "file",
    "smb",
    "nfs",
    "dav",
    "davs",
    "webdav",
    "webdavs",
    "cifs",
    "afs",
    // Scripts and system settings on macOS
    "applescript",
    "osascript",
    "x-apple.systempreferences",
    "x-apple-helpbasic",
];

/// Prefixes blocked as a family: every Windows `ms-` handler (`ms-msdt`,
/// `ms-officecmd`, `ms-appinstaller`, `ms-cxh`, ...) unless explicitly allowed.
const BLOCKED_PREFIXES: &[&str] = &["ms-"];

/// A scheme as a page wrote it, normalised: lower case, no trailing ":", only the
/// characters a scheme may have. None if it is not a valid scheme.
pub fn normalize(scheme: &str) -> Option<String> {
    let s = scheme.trim().trim_end_matches(':').to_ascii_lowercase();
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() || s.len() > 64 {
        return None;
    }
    if chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        Some(s)
    } else {
        None
    }
}

/// The tier of a scheme. `allowed_ms` lists `ms-` schemes the user chose to allow
/// (for example "ms-teams"); they are asked about instead of blocked.
pub fn classify(scheme: &str, allowed_ms: &[&str]) -> Tier {
    let Some(s) = normalize(scheme) else {
        return Tier::Block;
    };
    if BLOCKED.contains(&s.as_str()) {
        return Tier::Block;
    }
    for prefix in BLOCKED_PREFIXES {
        if s.starts_with(prefix)
            && !allowed_ms
                .iter()
                .any(|a| normalize(a).as_deref() == Some(&s))
        {
            return Tier::Block;
        }
    }
    Tier::Ask
}

/// The program a command line starts: the first token, with its quotes removed.
pub fn program_from_command(command: &str) -> Option<String> {
    let c = command.trim_start();
    let program = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next()?
    } else if let Some(rest) = c.strip_prefix('\'') {
        rest.split('\'').next()?
    } else {
        c.split_whitespace().next()?
    };
    let program = program.trim();
    if program.is_empty() || program.starts_with('%') {
        None
    } else {
        Some(program.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangerous_schemes_are_blocked() {
        for s in [
            "search-ms",
            "SEARCH-MS:",
            "ms-msdt",
            "ms-officecmd",
            "ms-appinstaller",
            "ms-cxh",
            "ms-settings",
            "search",
            "ghelp",
            "yelp",
            "smb",
            "applescript",
            "osascript",
            "x-apple.systempreferences",
            "vbscript",
            "javascript",
            "file",
        ] {
            assert_eq!(classify(s, &[]), Tier::Block, "{s}");
        }
    }

    #[test]
    fn other_schemes_are_asked_about() {
        for s in [
            "zoommtg", "slack", "steam", "tg", "magnet", "vscode", "mailto", "msteams",
        ] {
            assert_eq!(classify(s, &[]), Tier::Ask, "{s}");
        }
        assert_eq!(classify("ms-teams", &["ms-teams"]), Tier::Ask);
        assert_eq!(classify("ms-word", &["ms-teams"]), Tier::Block);
    }

    #[test]
    fn invalid_schemes_are_blocked() {
        for s in ["", "1abc", "a b", "ab\u{202E}c", "a/b", &"a".repeat(65)] {
            assert_eq!(classify(s, &[]), Tier::Block, "{s:?}");
        }
    }

    #[test]
    fn programs_from_command_lines() {
        assert_eq!(
            program_from_command("/usr/bin/zoom %u").as_deref(),
            Some("/usr/bin/zoom")
        );
        assert_eq!(
            program_from_command("\"C:\\Program Files\\Zoom\\Zoom.exe\" \"%1\"").as_deref(),
            Some("C:\\Program Files\\Zoom\\Zoom.exe")
        );
        assert_eq!(
            program_from_command("  'my app' --url %U").as_deref(),
            Some("my app")
        );
        assert_eq!(program_from_command("%1"), None);
        assert_eq!(program_from_command(""), None);
    }
}
