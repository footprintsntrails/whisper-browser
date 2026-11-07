//! The files in the top folder of an ISO 9660 disc image (Joliet names when present).
//! Disc images are held anyway; the names only make the warning concrete
//! ("it contains INVOICE.LNK").

use crate::Bytes;

/// What the top folder holds, or what to read next.
#[derive(Debug, PartialEq, Eq)]
pub enum Root {
    Names(Vec<String>),
    Need(Vec<(u64, u32)>),
    Unreadable,
}

const MAX_ROOT_BYTES: u32 = 64 * 1024;

/// Reads the names in the root directory.
pub fn root(bytes: &Bytes<'_>) -> Root {
    // Volume descriptors from sector 16; prefer the Joliet one (UCS-2 names).
    let mut chosen: Option<(u64, bool)> = None;
    for sector in 16u64..32 {
        let at = sector * 2048;
        let Some(vd) = bytes.at(at, 2048) else {
            return if at + 2048 <= bytes.size {
                Root::Need(vec![(at, 2048)])
            } else {
                Root::Unreadable
            };
        };
        if &vd[1..6] != b"CD001" {
            break;
        }
        match vd[0] {
            1 if chosen.is_none() => chosen = Some((at, false)),
            2 if matches!(&vd[88..91], b"%/@" | b"%/C" | b"%/E") => chosen = Some((at, true)),
            255 => break,
            _ => {}
        }
    }
    let Some((vd_at, joliet)) = chosen else {
        return Root::Unreadable;
    };
    let Some(vd) = bytes.at(vd_at, 2048) else {
        return Root::Unreadable;
    };
    let block = u16::from_le_bytes([vd[128], vd[129]]) as u64;
    let record = &vd[156..190];
    let extent = u32::from_le_bytes([record[2], record[3], record[4], record[5]]) as u64;
    let length =
        u32::from_le_bytes([record[10], record[11], record[12], record[13]]).min(MAX_ROOT_BYTES);
    if block == 0 || extent.saturating_mul(block).saturating_add(length as u64) > bytes.size {
        return Root::Unreadable;
    }
    let Some(dir) = bytes.at(extent * block, length as usize) else {
        return Root::Need(vec![(extent * block, length)]);
    };
    let mut names = Vec::new();
    let mut at = 0usize;
    while at < dir.len() {
        let len = dir[at] as usize;
        if len == 0 {
            // Records do not cross sectors; the rest of this one is padding.
            at = (at / 2048 + 1) * 2048;
            continue;
        }
        let Some(rec) = dir.get(at..at + len) else {
            break;
        };
        let name_len = *rec.get(32).unwrap_or(&0) as usize;
        if let Some(raw) = rec.get(33..33 + name_len)
            && raw != [0]
            && raw != [1]
        {
            let name = if joliet {
                let units: Vec<u16> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes(*c))
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                String::from_utf8_lossy(raw).into_owned()
            };
            // "SETUP.EXE;1": the version suffix is not part of the name.
            let name = name
                .split(';')
                .next()
                .unwrap_or_default()
                .trim_end_matches('.')
                .to_string();
            names.push(name);
        }
        at += len;
    }
    Root::Names(names)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Chunk;

    pub fn make_iso(names: &[&str]) -> Vec<u8> {
        let mut iso = vec![0u8; 20 * 2048];
        let pvd = 16 * 2048;
        iso[pvd] = 1;
        iso[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        iso[pvd + 128..pvd + 130].copy_from_slice(&2048u16.to_le_bytes());
        let root_extent = 18u32;
        iso[pvd + 156 + 2..pvd + 156 + 6].copy_from_slice(&root_extent.to_le_bytes());
        iso[pvd + 156 + 10..pvd + 156 + 14].copy_from_slice(&2048u32.to_le_bytes());
        let term = 17 * 2048;
        iso[term] = 255;
        iso[term + 1..term + 6].copy_from_slice(b"CD001");
        let mut at = 18 * 2048;
        for name in [".", ".."].iter().chain(names) {
            let raw: Vec<u8> = match *name {
                "." => vec![0],
                ".." => vec![1],
                n => format!("{n};1").into_bytes(),
            };
            let len = 33 + raw.len() + (raw.len() + 1) % 2;
            iso[at] = len as u8;
            iso[at + 32] = raw.len() as u8;
            iso[at + 33..at + 33 + raw.len()].copy_from_slice(&raw);
            at += len;
        }
        iso
    }

    #[test]
    fn lists_the_root() {
        let iso = make_iso(&["INVOICE.LNK", "README.TXT"]);
        let chunks = [Chunk {
            offset: 0,
            data: &iso,
        }];
        assert_eq!(
            root(&Bytes {
                size: iso.len() as u64,
                chunks: &chunks
            }),
            Root::Names(vec!["INVOICE.LNK".into(), "README.TXT".into()])
        );
    }

    #[test]
    fn asks_for_the_root_directory() {
        let iso = make_iso(&["A.EXE"]);
        let head = &iso[..18 * 2048];
        let chunks = [Chunk {
            offset: 0,
            data: head,
        }];
        assert_eq!(
            root(&Bytes {
                size: iso.len() as u64,
                chunks: &chunks
            }),
            Root::Need(vec![(18 * 2048, 2048)])
        );
    }
}
