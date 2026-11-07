/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! What the person is shown of a blocked command, and the trailing line break that is
//! cut from commands a page writes.

use crate::normalize::{is_invisible, lookalike};

/// How many characters of the original text are shown.
pub const DISPLAY_CHARS: usize = 400;

/// A run of this many spaces or more is shown as `<+N spaces>`.
const SPACE_RUN: usize = 4;

/// The text as the browser shows it: the first [`DISPLAY_CHARS`] characters, with
/// everything that would otherwise hide part of the command made visible:
///
/// * invisible and bidi control characters as `<U+200B>`;
/// * terminal control characters as `<ESC>` or `<U+0007>`, a carriage return as `<CR>`,
///   a tab as `<TAB>`;
/// * a run of four or more spaces as `<+212 spaces>`, and an unusual space (no-break,
///   ideographic, …) as `<U+00A0>`;
/// * a Cyrillic or Greek letter next to a Latin letter as `<U+0440>`;
/// * line breaks kept, a line break at the very end as `<line break>`;
/// * the rest cut with `<+N more characters>`.
///
/// The browser shows the result left to right in a monospace font.
pub fn display_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let shown = chars.len().min(DISPLAY_CHARS);
    let mut out = String::with_capacity(shown * 2);
    let mut i = 0;
    while i < shown {
        let c = chars[i];
        if is_space_like(c) {
            let run = chars[i..].iter().take_while(|c| is_space_like(**c)).count();
            if run >= SPACE_RUN {
                out.push_str(&format!("<+{run} spaces>"));
                i += run;
                continue;
            }
            if c == ' ' {
                out.push(' ');
            } else {
                out.push_str(&code_point(c));
            }
            i += 1;
            continue;
        }
        match c {
            '\n' => {
                if chars[i + 1..].iter().all(|c| matches!(c, '\n' | '\r')) {
                    out.push_str("<line break>");
                } else {
                    out.push('\n');
                }
            }
            '\r' => out.push_str("<CR>"),
            '\t' => out.push_str("<TAB>"),
            '\u{1b}' => out.push_str("<ESC>"),
            c if c.is_control() || is_invisible(c) => out.push_str(&code_point(c)),
            c if lookalike(c).is_some() && touches_latin(&chars, i) => out.push_str(&code_point(c)),
            c => out.push(c),
        }
        i += 1;
    }
    let rest = chars.len() - i.min(chars.len());
    if rest > 0 {
        out.push_str(&format!("<+{rest} more characters>"));
    }
    out
}

fn is_space_like(c: char) -> bool {
    matches!(
        c,
        ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

fn touches_latin(chars: &[char], i: usize) -> bool {
    let latin = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphabetic());
    latin(i.checked_sub(1).and_then(|p| chars.get(p))) || latin(chars.get(i + 1))
}

fn code_point(c: char) -> String {
    format!("<U+{:04X}>", c as u32)
}

/// For text a page writes: if it ends with a line break or ESC (optionally followed by
/// spaces), the length in UTF-16 units without them, so pasting it into a terminal does
/// not run it at once. `None` if there is nothing to cut.
pub fn trailing_strip_len_utf16(text: &[u16]) -> Option<usize> {
    const BREAKS: [u16; 8] = [0x0a, 0x0d, 0x1b, 0x0b, 0x0c, 0x85, 0x2028, 0x2029];
    let mut end = text.len();
    let mut cut_break = false;
    while end > 0 {
        let u = text[end - 1];
        if BREAKS.contains(&u) {
            cut_break = true;
        } else if u != 0x20 && u != 0x09 {
            break;
        }
        end -= 1;
    }
    (cut_break && end < text.len()).then_some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn shows_hidden_characters() {
        assert_eq!(display_text("a\u{200b}b"), "a<U+200B>b");
        assert_eq!(display_text("x\u{1b}[201~"), "x<ESC>[201~");
        assert_eq!(display_text("\u{440}owershell"), "<U+0440>owershell");
        assert_eq!(display_text("привет"), "привет");
        assert_eq!(display_text("a\tb\r\n"), "a<TAB>b<CR><line break>");
        assert_eq!(display_text("a\u{a0}b"), "a<U+00A0>b");
    }

    #[test]
    fn shortens_space_runs_and_long_text() {
        let padded = format!("ping x{}# C:\\docs\\a.docx", " ".repeat(212));
        assert_eq!(
            display_text(&padded),
            "ping x<+212 spaces># C:\\docs\\a.docx"
        );
        let long = "a".repeat(DISPLAY_CHARS + 25);
        let shown = display_text(&long);
        assert!(shown.ends_with("<+25 more characters>"), "{shown}");
        assert_eq!(shown.matches('a').count(), DISPLAY_CHARS + 2); // "characters" has two
    }

    #[test]
    fn keeps_inner_line_breaks() {
        assert_eq!(display_text("a\nb\n"), "a\nb<line break>");
    }

    #[test]
    fn cuts_trailing_line_breaks_and_esc_only() {
        assert_eq!(trailing_strip_len_utf16(&utf16("ls -la\n")), Some(6));
        assert_eq!(trailing_strip_len_utf16(&utf16("ls\r\n \n\t")), Some(2));
        assert_eq!(trailing_strip_len_utf16(&utf16("ls\u{1b}")), Some(2));
        assert_eq!(trailing_strip_len_utf16(&utf16("ls  ")), None);
        assert_eq!(trailing_strip_len_utf16(&utf16("ls")), None);
        assert_eq!(trailing_strip_len_utf16(&utf16("\n")), Some(0));
        assert_eq!(trailing_strip_len_utf16(&[]), None);
    }
}
