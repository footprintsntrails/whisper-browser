//! The table of contents of a ZIP archive (the central directory), read from the end
//! of the file. Only names and flags are read; nothing is decompressed.

use crate::Bytes;

/// One file in the archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub encrypted: bool,
}

/// Where the central directory is, or what to read to find out.
#[derive(Debug, PartialEq, Eq)]
pub enum Toc {
    /// The entries (at most `MAX_ENTRIES`), and whether some were left out.
    Entries {
        entries: Vec<Entry>,
        truncated: bool,
    },
    /// Read these ranges, then ask again.
    Need(Vec<(u64, u32)>),
    /// Not a readable ZIP (damaged, or the table of contents is too large).
    Unreadable,
}

pub const MAX_ENTRIES: usize = 20_000;
/// The central directory read at most (names of about 40,000 files).
pub const MAX_CD_BYTES: u64 = 4 * 1024 * 1024;
const EOCD_SIG: u32 = 0x0605_4b50;
const EOCD64_LOCATOR_SIG: u32 = 0x0706_4b50;
const EOCD64_SIG: u32 = 0x0606_4b50;
const CDH_SIG: u32 = 0x0201_4b50;
/// The end record is 22 bytes plus a comment of up to 65,535 bytes.
pub const TAIL: u64 = 22 + 65_535;

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u64le(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The ranges to read first: the end of the file, where the end record is.
pub fn wanted(size: u64) -> (u64, u32) {
    let len = size.min(TAIL);
    (size - len, len as u32)
}

/// Reads the table of contents from what is in `bytes`.
pub fn toc(bytes: &Bytes<'_>) -> Toc {
    let size = bytes.size;
    let (tail_start, tail_len) = wanted(size);
    let Some(tail) = bytes.at(tail_start, tail_len as usize) else {
        return Toc::Need(vec![(tail_start, tail_len)]);
    };
    // The end record, searching backwards (a comment may follow it).
    let Some(pos) = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| u32le(tail, i) == Some(EOCD_SIG))
    else {
        return Toc::Unreadable;
    };
    let eocd = &tail[pos..];
    let (Some(count), Some(cd_size), Some(cd_offset)) =
        (u16le(eocd, 10), u32le(eocd, 12), u32le(eocd, 16))
    else {
        return Toc::Unreadable;
    };
    let (mut count, mut cd_size, mut cd_offset) = (count as u64, cd_size as u64, cd_offset as u64);
    // ZIP64: the real values are in a second end record that a locator points to.
    if count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        let locator_at = tail_start + pos as u64;
        if locator_at < 20 {
            return Toc::Unreadable;
        }
        let Some(locator) = bytes.at(locator_at - 20, 20) else {
            return Toc::Need(vec![(locator_at - 20, 20)]);
        };
        if u32le(locator, 0) != Some(EOCD64_LOCATOR_SIG) {
            return Toc::Unreadable;
        }
        let Some(eocd64_at) = u64le(locator, 8) else {
            return Toc::Unreadable;
        };
        let Some(eocd64) = bytes.at(eocd64_at, 56) else {
            return if eocd64_at.saturating_add(56) <= size {
                Toc::Need(vec![(eocd64_at, 56)])
            } else {
                Toc::Unreadable
            };
        };
        if u32le(eocd64, 0) != Some(EOCD64_SIG) {
            return Toc::Unreadable;
        }
        match (u64le(eocd64, 32), u64le(eocd64, 40), u64le(eocd64, 48)) {
            (Some(c), Some(s), Some(o)) => (count, cd_size, cd_offset) = (c, s, o),
            _ => return Toc::Unreadable,
        }
    }
    if cd_offset.saturating_add(cd_size) > size || cd_size > MAX_CD_BYTES {
        return Toc::Unreadable;
    }
    let Some(cd) = bytes.at(cd_offset, cd_size as usize) else {
        return Toc::Need(vec![(cd_offset, cd_size as u32)]);
    };
    let mut entries = Vec::new();
    let mut at = 0usize;
    let mut truncated = false;
    for _ in 0..count {
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        if u32le(cd, at) != Some(CDH_SIG) {
            break;
        }
        let (Some(flags), Some(name_len), Some(extra_len), Some(comment_len)) = (
            u16le(cd, at + 8),
            u16le(cd, at + 28),
            u16le(cd, at + 30),
            u16le(cd, at + 32),
        ) else {
            break;
        };
        let name_start = at + 46;
        let Some(raw) = cd.get(name_start..name_start + name_len as usize) else {
            break;
        };
        // Bit 11: UTF-8. Otherwise code page 437; the non-ASCII letters do not matter
        // for an extension, so a lossy reading is enough.
        entries.push(Entry {
            name: String::from_utf8_lossy(raw).into_owned(),
            encrypted: flags & 1 == 1,
        });
        at = name_start + name_len as usize + extra_len as usize + comment_len as usize;
    }
    Toc::Entries { entries, truncated }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Chunk;

    /// A minimal ZIP: local headers are not needed, only the central directory.
    pub fn make_zip(names: &[(&str, bool)]) -> Vec<u8> {
        let mut out = b"PK\x03\x04".to_vec();
        out.extend_from_slice(&[0u8; 26]);
        let cd_offset = out.len() as u32;
        let mut cd = Vec::new();
        for (name, encrypted) in names {
            cd.extend_from_slice(&CDH_SIG.to_le_bytes());
            cd.extend_from_slice(&[20, 0, 20, 0]);
            cd.extend_from_slice(&(if *encrypted { 1u16 } else { 0 }).to_le_bytes());
            cd.extend_from_slice(&[0u8; 18]);
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0u8; 16]);
            cd.extend_from_slice(name.as_bytes());
        }
        out.extend_from_slice(&cd);
        out.extend_from_slice(&EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(b"\x05\x00hello");
        out
    }

    #[test]
    fn reads_names_and_encryption() {
        let zip = make_zip(&[("docs/readme.txt", false), ("Invoice.pdf.lnk", true)]);
        let chunks = [Chunk {
            offset: 0,
            data: &zip,
        }];
        let toc = toc(&Bytes {
            size: zip.len() as u64,
            chunks: &chunks,
        });
        assert_eq!(
            toc,
            Toc::Entries {
                entries: vec![
                    Entry {
                        name: "docs/readme.txt".into(),
                        encrypted: false
                    },
                    Entry {
                        name: "Invoice.pdf.lnk".into(),
                        encrypted: true
                    },
                ],
                truncated: false
            }
        );
    }

    #[test]
    fn asks_for_what_it_needs() {
        let zip = make_zip(&[("a.exe", false)]);
        let size = zip.len() as u64;
        // Nothing read yet: it wants the end of the file.
        assert_eq!(
            toc(&Bytes { size, chunks: &[] }),
            Toc::Need(vec![(0, size as u32)])
        );
    }

    #[test]
    fn garbage_is_unreadable() {
        let junk = vec![0x41u8; 5000];
        let chunks = [Chunk {
            offset: 0,
            data: &junk,
        }];
        assert_eq!(
            toc(&Bytes {
                size: 5000,
                chunks: &chunks
            }),
            Toc::Unreadable
        );
        // An end record that points outside the file.
        let mut zip = make_zip(&[("a.exe", false)]);
        let n = zip.len();
        zip[n - 7 - 4..n - 7].copy_from_slice(&u32::MAX.to_le_bytes());
        let chunks = [Chunk {
            offset: 0,
            data: &zip,
        }];
        assert_eq!(
            toc(&Bytes {
                size: n as u64,
                chunks: &chunks
            }),
            Toc::Unreadable
        );
    }
}
