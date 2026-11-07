//! What a file really is, from its first bytes (and, for some formats, its last).

use crate::Bytes;
use crate::kinds::Kind;

/// A recognised file format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Pe,
    Elf,
    MachO,
    Lnk,
    Msi,
    Zip,
    SevenZip,
    Rar,
    Cab,
    Gzip,
    Xz,
    Bzip2,
    Zstd,
    Iso9660,
    Udf,
    Vhd,
    Vhdx,
    Vmdk,
    Dmg,
    Pdf,
    Png,
    Jpeg,
    Gif,
    Webp,
    Mp4,
    Html,
    Shebang,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Pe => "Windows program",
            Format::Elf => "Linux program",
            Format::MachO => "macOS program",
            Format::Lnk => "Windows shortcut",
            Format::Msi => "Windows installer or Office document",
            Format::Zip => "ZIP archive",
            Format::SevenZip => "7-Zip archive",
            Format::Rar => "RAR archive",
            Format::Cab => "cabinet archive",
            Format::Gzip => "gzip archive",
            Format::Xz => "xz archive",
            Format::Bzip2 => "bzip2 archive",
            Format::Zstd => "zstd archive",
            Format::Iso9660 | Format::Udf => "disc image",
            Format::Vhd | Format::Vhdx | Format::Vmdk => "virtual disk",
            Format::Dmg => "macOS disk image",
            Format::Pdf => "PDF",
            Format::Png | Format::Jpeg | Format::Gif | Format::Webp => "picture",
            Format::Mp4 => "video",
            Format::Html => "web page",
            Format::Shebang => "script",
        }
    }

    /// The file kind this format behaves as.
    pub fn kind(self) -> Kind {
        match self {
            Format::Pe | Format::Elf | Format::MachO => Kind::Program,
            Format::Lnk => Kind::Shortcut,
            Format::Iso9660
            | Format::Udf
            | Format::Vhd
            | Format::Vhdx
            | Format::Vmdk
            | Format::Dmg => Kind::DiskImage,
            Format::Zip
            | Format::SevenZip
            | Format::Rar
            | Format::Cab
            | Format::Gzip
            | Format::Xz
            | Format::Bzip2
            | Format::Zstd => Kind::Archive,
            Format::Shebang => Kind::Script,
            Format::Msi => Kind::Other,
            Format::Pdf
            | Format::Png
            | Format::Jpeg
            | Format::Gif
            | Format::Webp
            | Format::Mp4
            | Format::Html => Kind::Document,
        }
    }
}

const LNK_HEADER: [u8; 20] = [
    0x4C, 0, 0, 0, 0x01, 0x14, 0x02, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46,
];

/// The ranges `detect` wants to look at: the first 64 KiB (covers the disc image
/// descriptors at 32 KiB) and the last 512 bytes (virtual disk and DMG footers).
pub fn wanted(size: u64) -> Vec<(u64, u32)> {
    let mut out = vec![(0, size.min(65536) as u32)];
    if size > 512 {
        out.push((size - 512, 512));
    }
    out
}

/// The format of the file, if recognised.
pub fn detect(bytes: &Bytes<'_>) -> Option<Format> {
    let starts = |magic: &[u8]| bytes.at(0, magic.len()) == Some(magic);
    if starts(b"MZ") {
        // A Windows program (PE), or a DOS one: both run when opened.
        return Some(Format::Pe);
    }
    let table: &[(&[u8], Format)] = &[
        (b"\x7FELF", Format::Elf),
        (&[0xFE, 0xED, 0xFA, 0xCE], Format::MachO),
        (&[0xFE, 0xED, 0xFA, 0xCF], Format::MachO),
        (&[0xCE, 0xFA, 0xED, 0xFE], Format::MachO),
        (&[0xCF, 0xFA, 0xED, 0xFE], Format::MachO),
        (&LNK_HEADER, Format::Lnk),
        (
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
            Format::Msi,
        ),
        (b"PK\x03\x04", Format::Zip),
        (b"PK\x05\x06", Format::Zip),
        (b"PK\x07\x08", Format::Zip),
        (&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C], Format::SevenZip),
        (b"Rar!\x1A\x07", Format::Rar),
        (b"MSCF", Format::Cab),
        (&[0x1F, 0x8B], Format::Gzip),
        (&[0xFD, b'7', b'z', b'X', b'Z', 0x00], Format::Xz),
        (b"BZh", Format::Bzip2),
        (&[0x28, 0xB5, 0x2F, 0xFD], Format::Zstd),
        (b"vhdxfile", Format::Vhdx),
        (b"conectix", Format::Vhd),
        (b"KDMV", Format::Vmdk),
        (b"%PDF-", Format::Pdf),
        (&[0x89, b'P', b'N', b'G'], Format::Png),
        (&[0xFF, 0xD8, 0xFF], Format::Jpeg),
        (b"GIF8", Format::Gif),
        (b"#!", Format::Shebang),
    ];
    for (magic, format) in table {
        if starts(magic) {
            return Some(*format);
        }
    }
    if bytes.at(0, 4) == Some(b"RIFF") && bytes.at(8, 4) == Some(b"WEBP") {
        return Some(Format::Webp);
    }
    if bytes.at(4, 4) == Some(b"ftyp") {
        return Some(Format::Mp4);
    }
    // ISO 9660 and UDF volume descriptors start at 32 KiB, one per 2 KiB sector.
    for sector in [0x8000u64, 0x8800, 0x9000] {
        match bytes.at(sector + 1, 5) {
            Some(b"CD001") => return Some(Format::Iso9660),
            Some(b"BEA01") | Some(b"NSR02") | Some(b"NSR03") => return Some(Format::Udf),
            _ => {}
        }
    }
    // Fixed-size VHDs and DMGs keep their signature in the last 512 bytes.
    let size = bytes.size;
    if size > 512 {
        match bytes.at(size - 512, 8) {
            Some(b"conectix") => return Some(Format::Vhd),
            Some(tail) if &tail[..4] == b"koly" => return Some(Format::Dmg),
            _ => {}
        }
    }
    if let Some(head) = bytes.at(0, (size.min(512)) as usize) {
        let text = String::from_utf8_lossy(head).to_ascii_lowercase();
        let text = text.trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n']);
        if text.starts_with("<!doctype html")
            || text.starts_with("<html")
            || text.starts_with("<hta:")
        {
            return Some(Format::Html);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Chunk;

    fn detect_in(data: &[u8]) -> Option<Format> {
        let chunks = [Chunk { offset: 0, data }];
        detect(&Bytes {
            size: data.len() as u64,
            chunks: &chunks,
        })
    }

    #[test]
    fn formats() {
        let mut pe = vec![0u8; 256];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3C] = 0x80;
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        assert_eq!(detect_in(&pe), Some(Format::Pe));
        assert_eq!(detect_in(&LNK_HEADER), Some(Format::Lnk));
        assert_eq!(detect_in(b"PK\x03\x04rest"), Some(Format::Zip));
        assert_eq!(detect_in(b"%PDF-1.7\n"), Some(Format::Pdf));
        assert_eq!(detect_in(b"\n<!DOCTYPE html><html>"), Some(Format::Html));
        assert_eq!(detect_in(b"plain text"), None);
        let mut iso = vec![0u8; 0x8800];
        iso[0x8001..0x8006].copy_from_slice(b"CD001");
        assert_eq!(detect_in(&iso), Some(Format::Iso9660));
        let mut vhd = vec![0u8; 4096];
        vhd[4096 - 512..4096 - 504].copy_from_slice(b"conectix");
        assert_eq!(detect_in(&vhd), Some(Format::Vhd));
    }
}
