/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The classifier: folded text in, [`Verdict`] out.

use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use regex::bytes::{Regex, RegexBuilder};

use crate::normalize::{fold, Folded};
use crate::rules::{Class, COMMAND_WORDS, FEATURES};
use crate::{hosts, Level, Reason, Reasons, Verdict};

/// Compiled rules. Nothing is built at startup: the anchor automaton is built on the first
/// write, and each rule's pattern the first time its anchors occur, so a write of plain
/// text never compiles a pattern.
pub(crate) struct Engine {
    anchors: Option<AhoCorasick>,
    /// For each anchor pattern, the feature it belongs to.
    anchor_owner: Vec<usize>,
    /// One per feature; `None` inside if the pattern did not compile (the tests forbid that).
    regexes: Vec<OnceLock<Option<Regex>>>,
    command_words: OnceLock<Option<Regex>>,
}

impl Engine {
    pub(crate) fn regex(&self, feature: usize) -> Option<&Regex> {
        self.regexes[feature]
            .get_or_init(|| compile(FEATURES[feature].pattern))
            .as_ref()
    }

    fn command_words(&self) -> Option<&Regex> {
        self.command_words
            .get_or_init(|| {
                let words: Vec<String> = COMMAND_WORDS.iter().map(|w| regex::escape(w)).collect();
                // A word is not part of a longer name: "sh" counts in "/bin/sh -c" and
                // "| sh", not in "shell", "install.sh" or "zsh-autosuggestions".
                compile(&format!(
                    r"(?:^|[^a-z0-9_.\-])({})(?:$|[^a-z0-9_\-])",
                    words.join("|")
                ))
            })
            .as_ref()
    }
}

/// Obfuscation marks (see `normalize::Folded::obfuscation_marks`) that count as obfuscation.
const OBFUSCATION_MARKS: usize = 2;

/// Upper bound for one compiled pattern and for its lazy DFA cache.
const REGEX_SIZE_LIMIT: usize = 8 << 20;

pub(crate) fn compile(pattern: &str) -> Option<Regex> {
    RegexBuilder::new(&format!("(?i-u){pattern}"))
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .build()
        .ok()
}

pub(crate) fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut patterns = Vec::new();
        let mut anchor_owner = Vec::new();
        for (i, f) in FEATURES.iter().enumerate() {
            for a in f.anchors {
                patterns.push(*a);
                anchor_owner.push(i);
            }
        }
        let anchors = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&patterns)
            .ok();
        Engine {
            anchors,
            anchor_owner,
            regexes: FEATURES.iter().map(|_| OnceLock::new()).collect(),
            command_words: OnceLock::new(),
        }
    })
}

/// Line numbers (0-based) of `text` that hold a command word.
fn command_lines(e: &Engine, text: &[u8]) -> Vec<usize> {
    let Some(re) = e.command_words() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let (mut line, mut counted_to) = (0usize, 0usize);
    for caps in re.captures_iter(text) {
        let Some(word) = caps.get(1) else { continue };
        line += memchr::memchr_iter(b'\n', &text[counted_to..word.start()]).count();
        counted_to = word.start();
        if lines.last() != Some(&line) {
            lines.push(line);
        }
    }
    lines
}

fn has_command_word(e: &Engine, text: &[u8]) -> bool {
    e.command_words().is_some_and(|re| re.is_match(text))
}

/// Classifies clipboard text.
pub fn classify(text: &str) -> Verdict {
    classify_folded(&fold(text), text)
}

/// Classifies the text of an HTML clipboard flavor (see [`html_to_text`]).
pub fn classify_html(html: &str) -> Verdict {
    classify(&html_to_text(html))
}

fn classify_folded(folded: &Folded, original: &str) -> Verdict {
    let e = engine();
    let bytes = folded.text.as_bytes();

    let mut candidate = vec![false; FEATURES.len()];
    if let Some(ac) = &e.anchors {
        for m in ac.find_overlapping_iter(bytes) {
            candidate[e.anchor_owner[m.pattern().as_usize()]] = true;
        }
    }

    let mut reasons = Reasons::default();
    let mut features = Vec::new();
    let (mut chain, mut strong, mut aggravated, mut low_alone) = (false, false, false, false);
    let (mut lure, mut padding, mut tool) = (false, false, false);
    for (i, f) in FEATURES.iter().enumerate() {
        if !candidate[i] {
            continue;
        }
        let Some(re) = e.regex(i) else { continue };
        let hit = match f.class {
            Class::Padding => padding_hit(e, re, bytes),
            _ => re.is_match(bytes),
        };
        if !hit {
            continue;
        }
        features.push(f.id);
        reasons.insert(f.reason);
        match f.class {
            Class::Chain => {
                chain = true;
                strong |= f.strong;
            }
            Class::Aggravator => {
                aggravated = true;
                low_alone |= f.low_alone;
            }
            Class::Lure => lure = true,
            Class::Padding => padding = true,
            Class::Tool => tool = true,
        }
    }

    // Carets, backticks and empty quotes inside words, and joined string pieces: two or
    // more are obfuscation (one alone is too common in ordinary code).
    if folded.obfuscation_marks >= OBFUSCATION_MARKS {
        reasons.insert(Reason::Obfuscated);
        aggravated = true;
        low_alone = true;
    }

    let command = has_command_word(e, bytes);
    let mut hidden_in_command = false;
    if command && (folded.invisible > 0 || folded.lookalikes > 0) {
        let lines = command_lines(e, bytes);
        if folded.invisible_lines.iter().any(|l| lines.contains(l)) {
            reasons.insert(Reason::InvisibleChars);
            hidden_in_command = true;
        }
        if folded.lookalike_lines.iter().any(|l| lines.contains(l)) {
            reasons.insert(Reason::LookalikeChars);
            hidden_in_command = true;
        }
    }
    if folded.controls > 0 {
        reasons.insert(Reason::ControlChars);
    }

    let mut level = if folded.paste_marker
        || padding
        || (command && (folded.controls > 0 || lure || hidden_in_command))
    {
        Level::Critical
    } else if chain && (strong || aggravated) {
        Level::High
    } else if chain {
        Level::Med
    } else if tool || (command && low_alone) || folded.controls > 0 || lure {
        Level::Low
    } else {
        Level::None
    };

    // A write larger than the browser scans could be hiding a command in the unscanned
    // part, so it is treated as at least MED: a script write of that size is not something
    // Whisper will quietly let through (see normalize::fold).
    if folded.oversize && level < Level::Med {
        level = Level::Med;
        reasons.insert(Reason::Oversize);
    }

    if level >= Level::Low && ends_with_line_break(original) {
        reasons.insert(Reason::TrailingNewline);
    }

    let known_installer =
        level == Level::Med && !folded.oversize && hosts::all_known_installers(&folded.text);
    Verdict {
        level,
        reasons,
        known_installer,
        features,
    }
}

/// A padding rule matched, and the part of its line before the spaces has a command word.
fn padding_hit(e: &Engine, re: &Regex, text: &[u8]) -> bool {
    re.captures_iter(text).any(|caps| {
        caps.iter()
            .skip(1)
            .flatten()
            .next()
            .is_some_and(|before| has_command_word(e, before.as_bytes()))
    })
}

fn ends_with_line_break(text: &str) -> bool {
    text.trim_end_matches([' ', '\t']).ends_with([
        '\n', '\r', '\u{1b}', '\u{0b}', '\u{0c}', '\u{85}', '\u{2028}', '\u{2029}',
    ])
}

/// The text of an HTML clipboard flavor: tags removed (block ends become line breaks),
/// the common entities decoded. Good enough to find a command in it; not a renderer.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find(['<', '&']) {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        if rest.starts_with('<') {
            let Some(end) = rest.find('>').map(|e| e + 1) else {
                // Not a tag: a lone "<" is text.
                break;
            };
            let tag = rest[1..end.saturating_sub(1).max(1)].to_ascii_lowercase();
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if name == "br"
                || (tag.starts_with('/')
                    && matches!(
                        name.as_str(),
                        "p" | "div"
                            | "li"
                            | "pre"
                            | "tr"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "blockquote"
                    ))
            {
                out.push('\n');
            }
            if matches!(name.as_str(), "script" | "style") && !tag.starts_with('/') {
                let close = format!("</{name}");
                rest = match rest.to_ascii_lowercase().find(&close) {
                    Some(c) => &rest[c..],
                    None => "",
                };
                continue;
            }
            rest = &rest[end..];
        } else {
            let end = rest[1..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
                .map(|e| e + 1);
            match end {
                Some(e) if rest[e..].starts_with(';') => {
                    match entity(&rest[1..e]) {
                        Some(c) => out.push(c),
                        None => out.push_str(&rest[..=e]),
                    }
                    rest = &rest[e + 1..];
                }
                _ => {
                    out.push('&');
                    rest = &rest[1..];
                }
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        "shy" => Some('\u{ad}'),
        "zwsp" => Some('\u{200b}'),
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_compiles() {
        let e = engine();
        for (i, f) in FEATURES.iter().enumerate() {
            assert!(
                e.regex(i).is_some(),
                "rule {} does not compile: {:?}",
                f.id,
                f.pattern
            );
        }
        assert!(e.command_words().is_some());
        assert!(e.anchors.is_some());
    }

    #[test]
    fn command_words_are_whole_words() {
        let e = engine();
        assert!(has_command_word(e, b"run curl now"));
        assert!(has_command_word(e, b"cmd.exe /c"));
        assert!(!has_command_word(e, b"shell scripts"));
        assert!(!has_command_word(e, b"curly braces"));
        assert_eq!(command_lines(e, b"one\ntwo curl\nthree\nsh x"), vec![1, 3]);
    }

    #[test]
    fn html_to_text_keeps_the_text() {
        assert_eq!(
            html_to_text("<pre><code>curl -fsSL https://x &amp;&amp; sh</code></pre><p>a</p>b"),
            "curl -fsSL https://x && sh\na\nb"
        );
        assert_eq!(html_to_text("a&#x200B;b&#105;"), "a\u{200b}bi");
        assert_eq!(html_to_text("<script>iex</script>x"), "x");
        assert_eq!(html_to_text("a & b <"), "a & b <");
    }
}
