//! Native dialogs: message boxes on Windows (and a small WinForms list), AppleScript on
//! macOS, zenity or kdialog on Linux. When there is no way to ask (no display, no dialog
//! tool, or `WHISPER_NONINTERACTIVE=1`), the functions return `None` and Whisper carries on
//! with the safe default.

use crate::paths;

/// Whether dialogs can be shown at all.
pub fn has_display() -> bool {
    if paths::noninteractive() {
        return false;
    }
    if cfg!(target_os = "linux") {
        return std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
            || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    }
    true
}

pub fn info(title: &str, text: &str) {
    if has_display() {
        imp::message(title, text, false);
    }
}

pub fn error(title: &str, text: &str) {
    if has_display() {
        imp::message(title, text, true);
    }
}

/// A yes/no question. `None` if it could not be asked.
pub fn ask(title: &str, text: &str, yes: &str, no: &str) -> Option<bool> {
    if !has_display() {
        return None;
    }
    imp::ask(title, text, yes, no)
}

/// Lets the user pick one of `items`. `None` if cancelled or it could not be asked.
pub fn choose(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
    if !has_display() || items.is_empty() {
        return None;
    }
    imp::choose(title, prompt, items).filter(|&i| i < items.len())
}

/// A desktop notification that does not wait for the user.
pub fn notify(title: &str, text: &str) {
    if has_display() {
        imp::notify(title, text);
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use crate::firefox::search_path;
    use std::process::{Command, Stdio};

    enum Tool {
        Zenity,
        Kdialog,
    }

    fn tool() -> Option<(Tool, std::path::PathBuf)> {
        if let Some(p) = search_path("zenity") {
            return Some((Tool::Zenity, p));
        }
        search_path("kdialog").map(|p| (Tool::Kdialog, p))
    }

    // zenity's --text is Pango markup
    fn markup(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    pub fn message(title: &str, text: &str, error: bool) {
        let Some((kind, path)) = tool() else {
            notify(title, text);
            return;
        };
        let mut cmd = Command::new(path);
        match kind {
            Tool::Zenity => cmd.arg(if error { "--error" } else { "--info" }).args([
                "--width=440",
                "--title",
                title,
                "--text",
                &markup(text),
            ]),
            Tool::Kdialog => cmd
                .args(["--title", title])
                .arg(if error { "--error" } else { "--msgbox" })
                .arg(text),
        };
        let _ = cmd.stderr(Stdio::null()).status();
    }

    pub fn ask(title: &str, text: &str, yes: &str, no: &str) -> Option<bool> {
        let (kind, path) = tool()?;
        let mut cmd = Command::new(path);
        match kind {
            Tool::Zenity => cmd.args([
                "--question",
                "--width=440",
                "--title",
                title,
                "--text",
                &markup(text),
                "--ok-label",
                yes,
                "--cancel-label",
                no,
            ]),
            Tool::Kdialog => cmd.args([
                "--title",
                title,
                "--yes-label",
                yes,
                "--no-label",
                no,
                "--yesno",
                text,
            ]),
        };
        match cmd.stderr(Stdio::null()).status().ok()?.code()? {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }

    pub fn choose(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
        let (kind, path) = tool()?;
        let mut cmd = Command::new(path);
        match kind {
            Tool::Zenity => {
                cmd.args([
                    "--list",
                    "--radiolist",
                    "--width=560",
                    "--height=340",
                    "--hide-header",
                    "--title",
                    title,
                    "--text",
                    &markup(prompt),
                    "--column",
                    "",
                    "--column",
                    "id",
                    "--column",
                    "Option",
                    "--hide-column=2",
                    "--print-column=2",
                ]);
                for (i, item) in items.iter().enumerate() {
                    cmd.arg(if i == 0 { "TRUE" } else { "FALSE" })
                        .arg(i.to_string())
                        .arg(item);
                }
            }
            Tool::Kdialog => {
                cmd.args(["--title", title, "--radiolist", prompt]);
                for (i, item) in items.iter().enumerate() {
                    cmd.arg(i.to_string())
                        .arg(item)
                        .arg(if i == 0 { "on" } else { "off" });
                }
            }
        }
        let out = cmd.stderr(Stdio::null()).output().ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    pub fn notify(title: &str, text: &str) {
        if let Some(path) = search_path("notify-send") {
            let _ = Command::new(path)
                .args(["--app-name=Whisper", "--icon=whisper", title, text])
                .stderr(Stdio::null())
                .status();
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Command, Stdio};

    // Text goes in as arguments, so nothing needs AppleScript escaping.
    fn osascript(lines: &[&str], args: &[&str]) -> Option<std::process::Output> {
        let mut cmd = Command::new("osascript");
        cmd.args(["-e", "on run argv"]);
        for line in lines {
            cmd.args(["-e", line]);
        }
        cmd.args(["-e", "end run"]);
        cmd.args(args).stdin(Stdio::null()).output().ok()
    }

    pub fn message(title: &str, text: &str, error: bool) {
        let icon = if error { "stop" } else { "note" };
        let line = format!(
            "display dialog (item 2 of argv) with title (item 1 of argv) buttons {{\"OK\"}} default button 1 with icon {icon}"
        );
        let _ = osascript(&[&line], &[title, text]);
    }

    pub fn ask(title: &str, text: &str, yes: &str, no: &str) -> Option<bool> {
        let out = osascript(
            &[
                "display dialog (item 2 of argv) with title (item 1 of argv) buttons {item 4 of argv, item 3 of argv} default button 2 cancel button 1 with icon note",
                "return button returned of result",
            ],
            &[title, text, yes, no],
        )?;
        if out.status.success() {
            return Some(String::from_utf8_lossy(&out.stdout).trim() == yes);
        }
        // "User canceled" is error -128
        String::from_utf8_lossy(&out.stderr)
            .contains("-128")
            .then_some(false)
    }

    pub fn choose(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
        let mut args: Vec<&str> = vec![title, prompt];
        args.extend(items.iter().map(String::as_str));
        let out = osascript(
            &[
                "set choices to items 3 thru -1 of argv",
                "set picked to choose from list choices with title (item 1 of argv) with prompt (item 2 of argv) default items {item 1 of choices} OK button name \"Continue\"",
                "if picked is false then return \"\"",
                "return item 1 of picked",
            ],
            &args,
        )?;
        let picked = String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string();
        items.iter().position(|i| *i == picked)
    }

    pub fn notify(title: &str, text: &str) {
        let _ = osascript(
            &["display notification (item 2 of argv) with title (item 1 of argv)"],
            &[title, text],
        );
    }
}

#[cfg(windows)]
mod imp {
    use crate::winapi;

    pub fn message(title: &str, text: &str, error: bool) {
        winapi::message_box(
            title,
            text,
            if error {
                winapi::MB_ERROR
            } else {
                winapi::MB_INFO
            },
        );
    }

    pub fn ask(title: &str, text: &str, _yes: &str, _no: &str) -> Option<bool> {
        Some(winapi::message_box(title, text, winapi::MB_YESNO) == winapi::IDYES)
    }

    pub fn choose(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
        winapi::choose(title, prompt, items)
    }

    pub fn notify(_title: &str, _text: &str) {}
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    pub fn message(_: &str, _: &str, _: bool) {}
    pub fn ask(_: &str, _: &str, _: &str, _: &str) -> Option<bool> {
        None
    }
    pub fn choose(_: &str, _: &str, _: &[String]) -> Option<usize> {
        None
    }
    pub fn notify(_: &str, _: &str) {}
}
