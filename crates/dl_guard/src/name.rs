//! Tricks in file names: right-to-left override and other direction marks, a document
//! extension in front of the real one ("invoice.pdf.exe"), and the traces of a direction
//! trick that Firefox has already neutralised.

use crate::kinds::{Kind, extension, kind_of_extension};

/// Unicode characters that change the display order of text (and zero-width marks
/// used with them). "invoice\u{202E}fdp.exe" is shown as "invoiceexe.pdf".
const DIRECTION_CHARS: &[char] = &[
    '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
];

/// A finding about a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameTrick {
    /// Characters that reorder how the name is displayed.
    DirectionMarks,
    /// Firefox replaces those characters with "_" when it saves a file; what is left
    /// still reads backwards: "invoice_fdp.exe" (from "invoice\u{202E}fdp.exe").
    ReversedExtension { shown: String },
    /// A document extension in front of the real one, maybe padded with spaces:
    /// "invoice.pdf.exe", "photo.jpg      .scr".
    DoubleExtension { shown: String, real: String },
}

/// Looks for name tricks in `name`. `real` is the kind of its last extension; tricks
/// only matter when that kind is risky.
pub fn tricks(name: &str) -> Vec<NameTrick> {
    let mut out = Vec::new();
    if name.contains(DIRECTION_CHARS) {
        out.push(NameTrick::DirectionMarks);
    }
    let real = extension(name);
    if !kind_of_extension(&real).is_risky() {
        return out;
    }
    let stem = strip_last_extension(name);
    // "invoice.pdf.exe", "photo.jpg   .exe", "a.pdf_.exe"
    if let Some(i) = stem.rfind('.') {
        let shown = stem[i + 1..]
            .trim_end_matches([' ', '_', '\u{3000}', '\u{00A0}'])
            .to_ascii_lowercase();
        if kind_of_extension(&shown) == Kind::Document {
            out.push(NameTrick::DoubleExtension { shown, real });
            return out;
        }
    }
    // "invoice_fdp.exe": an underscore, then a document extension written backwards.
    if let Some(i) = stem.rfind('_') {
        let tail: String = stem[i + 1..]
            .chars()
            .rev()
            .collect::<String>()
            .to_ascii_lowercase();
        if !tail.is_empty() && kind_of_extension(&tail) == Kind::Document {
            out.push(NameTrick::ReversedExtension { shown: tail });
        }
    }
    out
}

fn strip_last_extension(name: &str) -> &str {
    let trimmed = name.trim_end_matches(['.', ' ']);
    match trimmed.rfind('.') {
        Some(i) => &trimmed[..i],
        None => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_marks() {
        assert!(tricks("invoice\u{202E}fdp.exe").contains(&NameTrick::DirectionMarks));
        assert!(tricks("report\u{2067}.pdf").contains(&NameTrick::DirectionMarks));
        assert!(tricks("plain.pdf").is_empty());
    }

    #[test]
    fn double_extensions() {
        assert_eq!(
            tricks("invoice.pdf.exe"),
            vec![NameTrick::DoubleExtension {
                shown: "pdf".into(),
                real: "exe".into()
            }]
        );
        assert_eq!(
            tricks("photo.JPG        .scr"),
            vec![NameTrick::DoubleExtension {
                shown: "jpg".into(),
                real: "scr".into()
            }]
        );
        // Harmless in the end: no trick.
        assert!(tricks("archive.tar.gz").is_empty());
        assert!(tricks("notes.txt.pdf").is_empty());
        // Version numbers are not documents.
        assert!(tricks("setup-1.2.3.exe").is_empty());
    }

    #[test]
    fn neutralised_direction_trick() {
        assert_eq!(
            tricks("invoice_fdp.exe"),
            vec![NameTrick::ReversedExtension {
                shown: "pdf".into()
            }]
        );
        assert_eq!(
            tricks("Scan_gpj.scr"),
            vec![NameTrick::ReversedExtension {
                shown: "jpg".into()
            }]
        );
        assert!(tricks("my_setup.exe").is_empty());
    }
}
