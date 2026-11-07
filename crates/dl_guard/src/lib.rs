//! Whisper's download check (`dl_guard`).
//!
//! Decides whether a finished download is held until the user decides, from its name,
//! its first and last bytes and, for archives and disc images, their table of contents.
//! It reads nothing itself: the caller passes the byte ranges it asks for, so this code
//! has no file system or network access and runs unchanged in a WebAssembly sandbox
//! (crates/wcore_wasm) or in tests. No unsafe code; every length and offset that comes
//! from the file is checked before use.
//!
//! Held (verdict `Hold`), always: disc images and virtual disks, shortcuts, scripts and
//! script-like programs (`.hta`, `.ps1`, `.bat`, `.cmd`, `.vbs`, `.js`, `.wsf`, `.scr`,
//! `.msc`, ...), app packages (`.appx`, `.msix`, ...); a name that hides its real type
//! (direction marks, "invoice.pdf.exe"); content that is a program, shortcut or disk
//! image while the name or the server says otherwise; a ZIP that holds any of these, or
//! a program from a page (`blob:`, `data:`) or a local file. Programs (`.exe`, `.msi`)
//! are held unless the caller says their site is one the user already allowed in this
//! session and the download came over HTTPS.
//!
//! This is not an antivirus: a harmful file with an ordinary type passes.

#![forbid(unsafe_code)]

pub mod iso;
pub mod kinds;
pub mod magic;
pub mod name;
pub mod zip;

pub use kinds::Kind;
pub use magic::Format;
use name::NameTrick;

/// A byte range of the file that the caller read.
#[derive(Clone, Copy, Debug)]
pub struct Chunk<'a> {
    pub offset: u64,
    pub data: &'a [u8],
}

/// The parts of the file read so far.
#[derive(Clone, Copy, Debug)]
pub struct Bytes<'a> {
    pub size: u64,
    pub chunks: &'a [Chunk<'a>],
}

impl Bytes<'_> {
    /// `len` bytes at `offset`, if one chunk holds all of them.
    pub fn at(&self, offset: u64, len: usize) -> Option<&[u8]> {
        let end = offset.checked_add(len as u64)?;
        if end > self.size {
            return None;
        }
        self.chunks.iter().find_map(|c| {
            let c_end = c.offset.checked_add(c.data.len() as u64)?;
            if offset >= c.offset && end <= c_end {
                let start = usize::try_from(offset - c.offset).ok()?;
                c.data.get(start..start + len)
            } else {
                None
            }
        })
    }
}

/// Where the download came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    #[default]
    Https,
    /// Plain HTTP (or FTP): anyone on the way could have changed the file.
    Insecure,
    /// Built by a page script (`blob:`, `data:`): the way HTML smuggling delivers.
    MadeByPage,
    /// A local file, or a download started by a local page (`file:`).
    Local,
}

/// What the caller knows about the download.
#[derive(Clone, Copy, Debug)]
pub struct Input<'a> {
    /// The name the file will have.
    pub file_name: &'a str,
    /// The last part of the download's address, decoded: often the name the server
    /// sent before Firefox cleaned it.
    pub url_name: &'a str,
    /// The type the server claimed (Content-Type).
    pub mime: &'a str,
    pub origin: Origin,
    /// The user already allowed a program from this site in this session.
    pub site_allowed: bool,
    pub bytes: Bytes<'a>,
}

/// Why a file is held. `code()` names it for the caller's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    RiskyType {
        kind: Kind,
        ext: String,
    },
    Program {
        ext: String,
    },
    ProgramMadeByPage {
        ext: String,
    },
    DirectionMarks,
    ReversedExtension {
        shown: String,
        real: String,
    },
    DoubleExtension {
        shown: String,
        real: String,
    },
    ContentMismatch {
        claimed: String,
        found: Format,
    },
    ArchiveHolds {
        entry: String,
        kind: Kind,
        encrypted: bool,
    },
    DiscImageHolds {
        entry: String,
    },
}

impl Reason {
    pub fn code(&self) -> &'static str {
        match self {
            Reason::RiskyType { kind, .. } => kind.name(),
            Reason::Program { .. } => "program",
            Reason::ProgramMadeByPage { .. } => "program-made-by-page",
            Reason::DirectionMarks => "direction-marks",
            Reason::ReversedExtension { .. } => "reversed-extension",
            Reason::DoubleExtension { .. } => "double-extension",
            Reason::ContentMismatch { .. } => "content-mismatch",
            Reason::ArchiveHolds {
                encrypted: true, ..
            } => "encrypted-archive-holds",
            Reason::ArchiveHolds { .. } => "archive-holds",
            Reason::DiscImageHolds { .. } => "disc-image-holds",
        }
    }
}

/// The verdict.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Allow {
        kind: Kind,
        format: Option<Format>,
    },
    Hold {
        kind: Kind,
        format: Option<Format>,
        reasons: Vec<Reason>,
    },
    /// Read these ranges as well, then call `inspect` again with everything read.
    Need(Vec<(u64, u32)>),
}

/// What to read before the first call: the start and the end of the file.
pub fn first_ranges(size: u64) -> Vec<(u64, u32)> {
    let mut out = magic::wanted(size);
    let tail = zip::wanted(size);
    if tail.1 > 0 {
        out.push(tail);
    }
    out
}

/// Server types that say "a document, a picture, a video".
fn claims_document(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m.starts_with("image/")
        || m.starts_with("audio/")
        || m.starts_with("video/")
        || m.starts_with("text/")
        || m == "application/pdf"
        || m.starts_with("application/vnd.openxmlformats")
        || m == "application/msword"
}

/// Whether a format may stand behind an extension of this kind.
fn format_fits(found: Format, ext_kind: Kind, ext: &str) -> bool {
    match found.kind() {
        Kind::Program => {
            matches!(ext_kind, Kind::Program | Kind::Script | Kind::AppPackage)
                || matches!(
                    ext,
                    "dll" | "sys" | "efi" | "so" | "bin" | "elf" | "appimage" | "run" | ""
                )
        }
        Kind::Shortcut => ext_kind == Kind::Shortcut,
        Kind::DiskImage => ext_kind == Kind::DiskImage || matches!(ext, "bin" | "raw" | ""),
        // An archive named as a picture or a document is a disguise; office files,
        // packages and the like are ZIPs and named as what they are.
        Kind::Archive => {
            ext_kind != Kind::Document
                || matches!(
                    ext,
                    "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "epub"
                )
        }
        _ => true,
    }
}

/// Decides about one download. Call with what `first_ranges` named; when the answer is
/// `Need`, read those ranges too and call again with all chunks.
pub fn inspect(input: &Input<'_>) -> Outcome {
    let bytes = &input.bytes;
    let ext = kinds::extension(input.file_name);
    let ext_kind = kinds::kind_of_extension(&ext);
    let format = magic::detect(bytes);
    let mut reasons = Vec::new();

    // The name, and the name the server sent.
    for (i, name) in [input.file_name, input.url_name].into_iter().enumerate() {
        if i == 1 && (name.is_empty() || name == input.file_name) {
            continue;
        }
        for trick in name::tricks(name) {
            let reason = match trick {
                NameTrick::DirectionMarks => Reason::DirectionMarks,
                NameTrick::ReversedExtension { shown } => Reason::ReversedExtension {
                    shown,
                    real: kinds::extension(name),
                },
                NameTrick::DoubleExtension { shown, real } => {
                    Reason::DoubleExtension { shown, real }
                }
            };
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
    }

    // The type by name.
    match ext_kind {
        Kind::Script | Kind::Shortcut | Kind::AppPackage | Kind::DiskImage => {
            reasons.push(Reason::RiskyType {
                kind: ext_kind,
                ext: ext.clone(),
            })
        }
        Kind::Program => {
            if matches!(input.origin, Origin::MadeByPage | Origin::Local) {
                reasons.push(Reason::ProgramMadeByPage { ext: ext.clone() });
            } else if !input.site_allowed || input.origin == Origin::Insecure {
                reasons.push(Reason::Program { ext: ext.clone() });
            }
        }
        _ => {}
    }

    // The content against the name and the server's type.
    if let Some(found) = format {
        let disguised_by_name = !format_fits(found, ext_kind, &ext);
        let disguised_by_server = claims_document(input.mime) && found.kind().is_risky();
        if disguised_by_name || disguised_by_server {
            let claimed = if disguised_by_name {
                if ext.is_empty() {
                    "no type".to_string()
                } else {
                    format!(".{ext}")
                }
            } else {
                input.mime.to_string()
            };
            reasons.push(Reason::ContentMismatch { claimed, found });
        }
    }

    // What an archive or a disc image holds.
    match format {
        Some(Format::Zip) => match zip::toc(bytes) {
            zip::Toc::Need(ranges) => return Outcome::Need(ranges),
            zip::Toc::Entries { entries, .. } => {
                let program_ok = input.site_allowed && input.origin == Origin::Https;
                let mut shown = 0;
                for entry in entries {
                    let kind = kinds::kind_of_name(&entry.name);
                    let tricky = !name::tricks(&entry.name).is_empty();
                    let held = match kind {
                        Kind::Program => !program_ok || entry.encrypted,
                        k => k.is_risky(),
                    };
                    if (held || tricky) && shown < 3 {
                        shown += 1;
                        reasons.push(Reason::ArchiveHolds {
                            entry: entry.name,
                            kind,
                            encrypted: entry.encrypted,
                        });
                    }
                }
            }
            zip::Toc::Unreadable => {}
        },
        Some(Format::Iso9660) => match iso::root(bytes) {
            iso::Root::Need(ranges) => return Outcome::Need(ranges),
            iso::Root::Names(names) => {
                for entry in names
                    .into_iter()
                    .filter(|n| kinds::kind_of_name(n).is_risky())
                    .take(3)
                {
                    reasons.push(Reason::DiscImageHolds { entry });
                }
                if !reasons
                    .iter()
                    .any(|r| matches!(r, Reason::RiskyType { .. }))
                {
                    reasons.push(Reason::RiskyType {
                        kind: Kind::DiskImage,
                        ext: ext.clone(),
                    });
                }
            }
            iso::Root::Unreadable => {}
        },
        _ => {}
    }

    // A disc image under another name is held as one.
    let kind = match format {
        Some(f) if f.kind().is_risky() && !ext_kind.is_risky() => f.kind(),
        _ => ext_kind,
    };
    if reasons.is_empty() {
        Outcome::Allow { kind, format }
    } else {
        Outcome::Hold {
            kind,
            format,
            reasons,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(file_name: &str, data: &[u8], origin: Origin, site_allowed: bool) -> Outcome {
        run_mime(file_name, data, "", origin, site_allowed)
    }

    fn run_mime(
        file_name: &str,
        data: &[u8],
        mime: &str,
        origin: Origin,
        site_allowed: bool,
    ) -> Outcome {
        let chunks = [Chunk { offset: 0, data }];
        inspect(&Input {
            file_name,
            url_name: "",
            mime,
            origin,
            site_allowed,
            bytes: Bytes {
                size: data.len() as u64,
                chunks: &chunks,
            },
        })
    }

    fn codes(o: &Outcome) -> Vec<&'static str> {
        match o {
            Outcome::Hold { reasons, .. } => reasons.iter().map(Reason::code).collect(),
            _ => vec![],
        }
    }

    fn pe() -> Vec<u8> {
        let mut pe = vec![0u8; 512];
        pe[..2].copy_from_slice(b"MZ");
        pe
    }

    #[test]
    fn ordinary_files_pass() {
        assert!(matches!(
            run("report.pdf", b"%PDF-1.7\n...", Origin::Https, false),
            Outcome::Allow { .. }
        ));
        assert!(matches!(
            run("photo.jpg", &[0xFF, 0xD8, 0xFF, 0xE0], Origin::Https, false),
            Outcome::Allow { .. }
        ));
        assert!(matches!(
            run("notes.txt", b"hello", Origin::Insecure, false),
            Outcome::Allow { .. }
        ));
        let zip = zip::tests::make_zip(&[("photos/1.jpg", false), ("readme.txt", false)]);
        assert!(matches!(
            run("photos.zip", &zip, Origin::Https, false),
            Outcome::Allow { .. }
        ));
    }

    #[test]
    fn risky_types_are_held() {
        for name in [
            "disk.iso",
            "setup.vhdx",
            "invoice.lnk",
            "run.ps1",
            "a.hta",
            "b.bat",
            "c.vbs",
            "d.js",
            "e.wsf",
            "f.scr",
            "g.msc",
            "h.msix",
            "i.appx",
        ] {
            assert!(
                matches!(run(name, b"x", Origin::Https, true), Outcome::Hold { .. }),
                "{name}"
            );
        }
    }

    #[test]
    fn programs_depend_on_the_site() {
        assert_eq!(
            codes(&run("setup.exe", &pe(), Origin::Https, false)),
            vec!["program"]
        );
        assert!(matches!(
            run("setup.exe", &pe(), Origin::Https, true),
            Outcome::Allow {
                kind: Kind::Program,
                ..
            }
        ));
        assert_eq!(
            codes(&run("setup.exe", &pe(), Origin::Insecure, true)),
            vec!["program"]
        );
        assert_eq!(
            codes(&run("setup.msi", b"x", Origin::MadeByPage, true)),
            vec!["program-made-by-page"]
        );
    }

    #[test]
    fn name_tricks() {
        assert_eq!(
            codes(&run("invoice.pdf.exe", &pe(), Origin::Https, true)),
            vec!["double-extension"]
        );
        assert_eq!(
            codes(&run("invoice_fdp.exe", &pe(), Origin::Https, true)),
            vec!["reversed-extension"]
        );
        assert!(
            codes(&run("invoice\u{202E}fdp.exe", &pe(), Origin::Https, true))
                .contains(&"direction-marks")
        );
        // The server's original name had the trick even though Firefox cleaned it.
        let chunks = [Chunk {
            offset: 0,
            data: b"MZ",
        }];
        let o = inspect(&Input {
            file_name: "invoice_fdp.exe",
            url_name: "invoice\u{202E}fdp.exe",
            mime: "",
            origin: Origin::Https,
            site_allowed: true,
            bytes: Bytes {
                size: 2,
                chunks: &chunks,
            },
        });
        assert!(codes(&o).contains(&"direction-marks"));
    }

    #[test]
    fn disguised_content() {
        assert_eq!(
            codes(&run("invoice.pdf", &pe(), Origin::Https, true)),
            vec!["content-mismatch"]
        );
        assert_eq!(
            codes(&run_mime(
                "download",
                &pe(),
                "application/pdf",
                Origin::Https,
                true
            )),
            vec!["content-mismatch"]
        );
        let zip = zip::tests::make_zip(&[("a.txt", false)]);
        assert_eq!(
            codes(&run("cat.jpg", &zip, Origin::Https, true)),
            vec!["content-mismatch"]
        );
        // Office documents are ZIPs, and that is fine.
        assert!(matches!(
            run("letter.docx", &zip, Origin::Https, false),
            Outcome::Allow { .. }
        ));
        // A disc image named as a document is held as a disc image.
        let iso = iso::tests::make_iso(&["SETUP.EXE"]);
        match run("scan.pdf", &iso, Origin::Https, true) {
            Outcome::Hold { kind, reasons, .. } => {
                assert_eq!(kind, Kind::DiskImage);
                assert!(reasons.iter().any(|r| r.code() == "disc-image-holds"));
            }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn archives_that_hide_programs() {
        let zip = zip::tests::make_zip(&[("docs/Invoice.pdf.lnk", false), ("readme.txt", false)]);
        assert_eq!(
            codes(&run("docs.zip", &zip, Origin::Https, true)),
            vec!["archive-holds"]
        );
        let zip = zip::tests::make_zip(&[("setup.exe", false)]);
        assert_eq!(
            codes(&run("tool.zip", &zip, Origin::Https, false)),
            vec!["archive-holds"]
        );
        assert!(matches!(
            run("tool.zip", &zip, Origin::Https, true),
            Outcome::Allow { .. }
        ));
        let zip = zip::tests::make_zip(&[("setup.exe", true)]);
        assert_eq!(
            codes(&run("tool.zip", &zip, Origin::Https, true)),
            vec!["encrypted-archive-holds"]
        );
    }

    #[test]
    fn asks_for_more_and_never_reads_out_of_bounds() {
        // Only the start was read; the ZIP's end record is elsewhere.
        let zip = zip::tests::make_zip(&[("a.exe", false)]);
        let chunks = [Chunk {
            offset: 0,
            data: &zip[..10],
        }];
        let o = inspect(&Input {
            file_name: "a.zip",
            url_name: "",
            mime: "",
            origin: Origin::Https,
            site_allowed: false,
            bytes: Bytes {
                size: zip.len() as u64,
                chunks: &chunks,
            },
        });
        assert!(matches!(o, Outcome::Need(_)));
        // Ranges beyond the file are never served.
        let b = Bytes {
            size: 10,
            chunks: &chunks,
        };
        assert!(b.at(8, 4).is_none());
        assert!(b.at(u64::MAX, 2).is_none());
    }

    #[test]
    fn first_ranges_cover_head_and_tail() {
        assert_eq!(first_ranges(100), vec![(0, 100), (0, 100)]);
        let r = first_ranges(10_000_000);
        assert!(r.contains(&(0, 65536)) && r.contains(&(10_000_000 - 512, 512)));
    }
}
