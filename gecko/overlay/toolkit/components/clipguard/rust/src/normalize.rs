/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Folding: one copy of the text in the form the rules match against.
//!
//! * Invisible and format characters (zero-width space, bidi controls, soft hyphen, tag
//!   characters, …) are removed and counted when they sit next to ASCII text. Between
//!   other characters (a zero-width joiner inside an emoji) they are removed but not
//!   counted.
//! * Terminal control characters (ESC and the other C0 and C1 controls, except tab and
//!   line breaks) become spaces and are counted.
//! * Full-width ASCII, typographic dashes, quotes and spaces, and ligatures become their
//!   ASCII forms; PowerShell itself reads dashes and quotes this way.
//! * A Cyrillic or Greek letter that looks like a Latin one becomes that Latin letter
//!   when it touches an ASCII letter ("рowershell" with a Cyrillic "р"), and is counted.
//! * Shell obfuscation is undone and counted: carets inside words (p^o^w^e^r), PowerShell
//!   backticks inside words (Po`wer`Shell), empty quotes (i""ex) and joined string pieces
//!   ('iw'+'r').
//! * Runs of spaces are cut to [`MAX_SPACE_RUN`] so padding stays visible to the rules but
//!   cannot grow the text.
//!
//! This is not full Unicode NFKC: only the mappings that change how a shell, the Run box
//! or Explorer reads the text are applied, which keeps the crate free of Unicode tables.

use crate::rules::MAX_SCAN_BYTES;

/// Longest run of spaces or tabs kept in the folded text.
pub(crate) const MAX_SPACE_RUN: usize = 64;

/// The folded text and what folding found.
#[derive(Debug, Default)]
pub(crate) struct Folded {
    pub text: String,
    /// Invisible characters next to ASCII text.
    pub invisible: usize,
    /// Lines (0-based, of `text`) holding such a character.
    pub invisible_lines: Vec<usize>,
    /// Look-alike letters next to ASCII letters.
    pub lookalikes: usize,
    /// Lines holding such a letter.
    pub lookalike_lines: Vec<usize>,
    /// Terminal control characters.
    pub controls: usize,
    /// The text holds a bracketed-paste start or end marker (ESC [200~ / ESC [201~).
    pub paste_marker: bool,
    /// Carets, backticks and empty quotes inside words, and joined string pieces.
    pub obfuscation_marks: usize,
}

/// What a single character is, for folding.
enum Kind {
    Invisible,
    Control,
    LineBreak,
    Space,
    Map(char),
    Expand(&'static str),
    Lookalike(char),
    Keep,
}

fn kind(c: char) -> Kind {
    match c {
        '\n' => Kind::LineBreak,
        '\t' | '\r' => Kind::Keep,
        '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
            if c == '\u{85}' {
                Kind::LineBreak
            } else {
                Kind::Control
            }
        }
        '\u{2028}' | '\u{2029}' => Kind::LineBreak,
        '\u{ad}'
        | '\u{34f}'
        | '\u{61c}'
        | '\u{115f}'
        | '\u{1160}'
        | '\u{17b4}'
        | '\u{17b5}'
        | '\u{180b}'..='\u{180f}'
        | '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{206f}'
        | '\u{3164}'
        | '\u{fe00}'..='\u{fe0f}'
        | '\u{feff}'
        | '\u{ffa0}'
        | '\u{fff9}'..='\u{fffb}'
        | '\u{1d173}'..='\u{1d17a}'
        | '\u{e0000}'..='\u{e007f}'
        | '\u{e0100}'..='\u{e01ef}' => Kind::Invisible,
        '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' => {
            Kind::Space
        }
        '\u{2010}'..='\u{2015}' | '\u{2212}' | '\u{fe58}' | '\u{fe63}' => Kind::Map('-'),
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' | '\u{2032}' | '\u{2035}' => {
            Kind::Map('\'')
        }
        '\u{201c}'..='\u{201f}' | '\u{2033}' | '\u{2036}' => Kind::Map('"'),
        '\u{ff01}'..='\u{ff5e}' => match char::from_u32(c as u32 - 0xfee0) {
            Some(m) => Kind::Map(m),
            None => Kind::Keep,
        },
        '\u{fb00}' => Kind::Expand("ff"),
        '\u{fb01}' => Kind::Expand("fi"),
        '\u{fb02}' => Kind::Expand("fl"),
        '\u{fb03}' => Kind::Expand("ffi"),
        '\u{fb04}' => Kind::Expand("ffl"),
        '\u{fb05}' | '\u{fb06}' => Kind::Expand("st"),
        _ => match lookalike(c) {
            Some(m) => Kind::Lookalike(m),
            None => Kind::Keep,
        },
    }
}

/// Cyrillic and Greek letters that look like Latin ones (from Unicode's confusables
/// list, letters only).
pub(crate) fn lookalike(c: char) -> Option<char> {
    Some(match c {
        // Cyrillic
        'а' => 'a',
        'в' => 'b',
        'е' => 'e',
        'һ' => 'h',
        'і' => 'i',
        'ј' => 'j',
        'к' => 'k',
        'ӏ' => 'l',
        'м' => 'm',
        'о' => 'o',
        'р' => 'p',
        'ԛ' => 'q',
        'ѕ' => 's',
        'с' => 'c',
        'т' => 't',
        'у' => 'y',
        'ԝ' => 'w',
        'х' => 'x',
        'ԁ' => 'd',
        'ɡ' => 'g',
        'А' => 'A',
        'В' => 'B',
        'Е' => 'E',
        'Н' => 'H',
        'І' => 'I',
        'Ј' => 'J',
        'К' => 'K',
        'М' => 'M',
        'О' => 'O',
        'Р' => 'P',
        'С' => 'C',
        'Ѕ' => 'S',
        'Т' => 'T',
        'Х' => 'X',
        'У' => 'Y',
        'Ԝ' => 'W',
        // Greek
        'α' => 'a',
        'ε' => 'e',
        'ι' => 'i',
        'κ' => 'k',
        'ν' => 'v',
        'ο' => 'o',
        'ρ' => 'p',
        'τ' => 't',
        'υ' => 'u',
        'χ' => 'x',
        'Α' => 'A',
        'Β' => 'B',
        'Ε' => 'E',
        'Ζ' => 'Z',
        'Η' => 'H',
        'Ι' => 'I',
        'Κ' => 'K',
        'Μ' => 'M',
        'Ν' => 'N',
        'Ο' => 'O',
        'Ρ' => 'P',
        'Τ' => 'T',
        'Υ' => 'Y',
        'Χ' => 'X',
        _ => return None,
    })
}

/// Whether `c` is one of the characters folding removes as invisible.
pub(crate) fn is_invisible(c: char) -> bool {
    matches!(kind(c), Kind::Invisible)
}

/// Folds `text` (see the module documentation).
pub(crate) fn fold(text: &str) -> Folded {
    let text = bounded(text, MAX_SCAN_BYTES * 4);
    let mut out = Folded {
        paste_marker: has_paste_marker(text),
        ..Folded::default()
    };

    // Pass 1: map characters, count what is hidden.
    let src: Vec<char> = text.chars().collect();
    let mut mapped: Vec<char> = Vec::with_capacity(src.len());
    let mut line = 0usize;
    for (i, &c) in src.iter().enumerate() {
        match kind(c) {
            Kind::Invisible => {
                let before = src[..i].iter().rev().find(|c| !is_invisible(**c));
                let after = src[i + 1..].iter().find(|c| !is_invisible(**c));
                if before.is_some_and(|c| c.is_ascii_graphic())
                    || after.is_some_and(|c| c.is_ascii_graphic())
                {
                    out.invisible += 1;
                    push_line(&mut out.invisible_lines, line);
                }
            }
            Kind::Control => {
                out.controls += 1;
                mapped.push(' ');
            }
            Kind::LineBreak => {
                mapped.push('\n');
                line += 1;
            }
            Kind::Space => mapped.push(' '),
            Kind::Map(m) => mapped.push(m),
            Kind::Expand(s) => mapped.extend(s.chars()),
            Kind::Lookalike(m) => {
                let touches_ascii = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphabetic());
                let prev = src[..i].iter().rev().find(|c| !is_invisible(**c));
                let next = src[i + 1..].iter().find(|c| !is_invisible(**c));
                if touches_ascii(prev) || touches_ascii(next) {
                    out.lookalikes += 1;
                    push_line(&mut out.lookalike_lines, line);
                    mapped.push(m);
                } else {
                    mapped.push(c);
                }
            }
            Kind::Keep => mapped.push(c),
        }
    }

    // Pass 2: undo shell obfuscation, cut space runs.
    let mut s = String::with_capacity(mapped.len());
    let mut space_run = 0usize;
    let mut i = 0;
    while i < mapped.len() {
        let c = mapped[i];
        let prev = s.chars().next_back();
        let next = mapped.get(i + 1).copied();
        match c {
            '^' if next.is_some_and(|n| n.is_ascii_alphanumeric() || "-/.".contains(n)) => {
                if prev.is_some_and(|p| p.is_ascii_alphanumeric()) {
                    out.obfuscation_marks += 1;
                }
                i += 1;
                continue;
            }
            '`' if prev.is_some_and(|p| p.is_ascii_alphabetic())
                && next.is_some_and(|n| n.is_ascii_alphabetic()) =>
            {
                out.obfuscation_marks += 1;
                i += 1;
                continue;
            }
            '"' | '\'' => {
                // i""ex
                if next == Some(c)
                    && prev.is_some_and(|p| p.is_ascii_alphanumeric())
                    && mapped.get(i + 2).is_some_and(|n| n.is_ascii_alphanumeric())
                {
                    out.obfuscation_marks += 1;
                    i += 2;
                    continue;
                }
                // 'iw' + 'r'  ->  'iwr'
                if let Some(end) = joined_quote(&mapped, i) {
                    out.obfuscation_marks += 1;
                    i = end + 1;
                    continue;
                }
            }
            _ => {}
        }
        if c == ' ' || c == '\t' {
            space_run += 1;
            if space_run > MAX_SPACE_RUN {
                i += 1;
                continue;
            }
        } else {
            space_run = 0;
        }
        s.push(c);
        i += 1;
    }

    out.text = bounded(&s, MAX_SCAN_BYTES).to_owned();
    if out.text.len() < s.len() {
        // Keep the start and the end: a payload is either the whole text or appended.
        let head = bounded(&s, MAX_SCAN_BYTES / 2);
        let tail = tail_bounded(&s, MAX_SCAN_BYTES / 2);
        out.text = format!("{head}\n{tail}");
    }
    out
}

/// At `mapped[i]`, a quote that closes a piece joined to the next one with `+`: returns
/// the index of the quote that opens the next piece.
fn joined_quote(mapped: &[char], i: usize) -> Option<usize> {
    let mut j = i + 1;
    while mapped.get(j).is_some_and(|c| *c == ' ') {
        j += 1;
    }
    if mapped.get(j) != Some(&'+') {
        return None;
    }
    j += 1;
    while mapped.get(j).is_some_and(|c| *c == ' ') {
        j += 1;
    }
    match mapped.get(j) {
        Some('"') | Some('\'') if i > 0 && mapped[i - 1] != ' ' => Some(j),
        _ => None,
    }
}

fn push_line(lines: &mut Vec<usize>, line: usize) {
    if lines.last() != Some(&line) {
        lines.push(line);
    }
}

fn has_paste_marker(text: &str) -> bool {
    ["\u{1b}[200~", "\u{1b}[201~", "\u{9b}200~", "\u{9b}201~"]
        .iter()
        .any(|m| text.contains(m))
}

/// The longest prefix of `s` that fits in `max` bytes, cut at a character boundary.
pub(crate) fn bounded(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn tail_bounded(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// Folds `text` for comparing what was copied with what the page shows: invisible
/// characters removed, look-alikes, dashes, quotes, full-width letters and spaces mapped,
/// ASCII letters lower-cased. Unlike [`fold`], nothing else changes.
pub(crate) fn fold_for_compare(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match kind(c) {
            Kind::Invisible => {}
            Kind::Control | Kind::Space => s.push(' '),
            Kind::LineBreak => s.push('\n'),
            Kind::Map(m) | Kind::Lookalike(m) => s.push(m.to_ascii_lowercase()),
            Kind::Expand(e) => s.push_str(e),
            Kind::Keep => s.push(c.to_ascii_lowercase()),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_and_counts_invisible_characters_in_ascii_text() {
        let f = fold("power\u{200b}shell -c x");
        assert_eq!(f.text, "powershell -c x");
        assert_eq!(f.invisible, 1);
        assert_eq!(f.invisible_lines, vec![0]);
    }

    #[test]
    fn zero_width_joiner_inside_emoji_is_not_counted() {
        let f = fold("family \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}");
        assert_eq!(f.invisible, 0);
    }

    #[test]
    fn maps_lookalikes_only_next_to_latin_letters() {
        let f = fold("\u{440}owershell и привет");
        assert_eq!(f.lookalikes, 1);
        assert!(f.text.starts_with("powershell"));
        assert!(f.text.contains("привет"), "{}", f.text);
    }

    #[test]
    fn undoes_cmd_and_powershell_obfuscation() {
        assert_eq!(fold("p^o^w^e^r^s^h^e^l^l").text, "powershell");
        assert_eq!(fold("Po`wer`Shell").text, "PowerShell");
        assert_eq!(fold("i\"\"ex").text, "iex");
        assert_eq!(fold("('iw'+'r') ").text, "('iwr') ");
        assert_eq!(fold("'po' + 'wer'").text, "'power'");
        assert!(fold("p^o^w^e^r").obfuscation_marks >= 4);
    }

    #[test]
    fn keeps_ordinary_code_alone() {
        let js = "const s = 'a' + b;";
        assert_eq!(fold(js).text, js);
        assert_eq!(fold("grep '^foo' file").text, "grep 'foo' file");
        assert_eq!(fold("grep '^foo' file").obfuscation_marks, 0);
        assert_eq!(fold("cmd ^\nnext").text, "cmd ^\nnext");
    }

    #[test]
    fn maps_typographic_and_full_width_forms() {
        assert_eq!(fold("iwr \u{2013}useb x").text, "iwr -useb x");
        assert_eq!(fold("\u{201c}a\u{201d}").text, "\"a\"");
        assert_eq!(fold("\u{ff49}\u{ff45}\u{ff58}").text, "iex");
        assert_eq!(fold("a\u{a0}b").text, "a b");
    }

    #[test]
    fn counts_controls_and_paste_markers() {
        let f = fold("ls\u{1b}[201~rm");
        assert_eq!(f.controls, 1);
        assert!(f.paste_marker);
    }

    #[test]
    fn cuts_long_space_runs() {
        let f = fold(&format!("a{}b", " ".repeat(500)));
        assert_eq!(f.text.len(), 2 + MAX_SPACE_RUN);
    }
}
