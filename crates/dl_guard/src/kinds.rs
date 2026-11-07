//! File type classes by extension, and the extension of a file name.

/// What a file of a given extension can do when opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A Windows program or installer: `.exe`, `.msi`. Held unless its site is known.
    Program,
    /// Runs code when opened, rarely downloaded for good reasons: scripts, `.scr`,
    /// `.hta`, `.msc`, ... Always held.
    Script,
    /// Points somewhere else, or runs a command, when opened: `.lnk`, `.url`, ... Held.
    Shortcut,
    /// A store or ClickOnce package that installs an app: `.msix`, `.appx`, ... Held.
    AppPackage,
    /// A disk image whose files lose the mark of the web: `.iso`, `.vhd`, ... Held.
    DiskImage,
    /// An archive; held when it holds one of the kinds above.
    Archive,
    /// Documents, pictures and media: what a disguised program pretends to be.
    Document,
    /// Anything else.
    Other,
}

impl Kind {
    /// Short name, used in the verdict and the warning text.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Program => "program",
            Kind::Script => "script",
            Kind::Shortcut => "shortcut",
            Kind::AppPackage => "app-package",
            Kind::DiskImage => "disk-image",
            Kind::Archive => "archive",
            Kind::Document => "document",
            Kind::Other => "other",
        }
    }

    /// Kinds that run something, or hide what does, when the user opens the file.
    pub fn is_risky(self) -> bool {
        matches!(
            self,
            Kind::Program | Kind::Script | Kind::Shortcut | Kind::AppPackage | Kind::DiskImage
        )
    }
}

const PROGRAM: &[&str] = &["exe", "msi", "msp"];
const SCRIPT: &[&str] = &[
    // Windows Script Host, PowerShell, cmd, HTML applications, consoles
    "hta",
    "ps1",
    "psm1",
    "psd1",
    "ps1xml",
    "psc1",
    "bat",
    "cmd",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "wsc",
    "sct",
    "msc",
    "reg",
    "inf",
    "chm",
    "hlp",
    "xll",
    // programs under other names
    "scr",
    "com",
    "pif",
    "cpl",
    // Java, Unix and macOS scripts
    "jar",
    "jnlp",
    "sh",
    "bash",
    "command",
    "tool",
    "applescript",
    "scpt",
];
const SHORTCUT: &[&str] = &[
    "lnk",
    "url",
    "scf",
    "website",
    "library-ms",
    "searchconnector-ms",
    "settingcontent-ms",
    "desktop",
    "webloc",
    "inetloc",
    "theme",
    "themepack",
];
const APP_PACKAGE: &[&str] = &[
    "appx",
    "appxbundle",
    "msix",
    "msixbundle",
    "appinstaller",
    "application",
    "xbap",
    "vsix",
];
const DISK_IMAGE: &[&str] = &["iso", "img", "vhd", "vhdx", "vmdk", "dmg", "udf"];
const ARCHIVE: &[&str] = &[
    "zip", "7z", "rar", "cab", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "arj", "lzh",
    "lha", "ace", "z",
];
const DOCUMENT: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "txt", "csv",
    "epub", "jpg", "jpeg", "png", "gif", "bmp", "webp", "heic", "tif", "tiff", "svg", "mp3", "wav",
    "flac", "ogg", "m4a", "mp4", "mov", "avi", "mkv", "webm", "html", "htm",
];

/// The kind of a lower-case extension (without the dot).
pub fn kind_of_extension(ext: &str) -> Kind {
    let has = |list: &[&str]| list.contains(&ext);
    if has(PROGRAM) {
        Kind::Program
    } else if has(SCRIPT) {
        Kind::Script
    } else if has(SHORTCUT) {
        Kind::Shortcut
    } else if has(APP_PACKAGE) {
        Kind::AppPackage
    } else if has(DISK_IMAGE) {
        Kind::DiskImage
    } else if has(ARCHIVE) {
        Kind::Archive
    } else if has(DOCUMENT) {
        Kind::Document
    } else {
        Kind::Other
    }
}

/// The last extension of a file name, lower case, without the dot; "" if none.
/// Windows drops trailing dots and spaces from names, so they are ignored here too.
pub fn extension(name: &str) -> String {
    let trimmed = name.trim_end_matches(['.', ' ']);
    match trimmed.rfind('.') {
        Some(i) if i > 0 || trimmed.len() > 1 => trimmed[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// The kind of a file name, by its last extension.
pub fn kind_of_name(name: &str) -> Kind {
    kind_of_extension(&extension(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions() {
        assert_eq!(extension("setup.EXE"), "exe");
        assert_eq!(extension("invoice.pdf.exe"), "exe");
        assert_eq!(extension("archive.tar.gz"), "gz");
        assert_eq!(extension("README"), "");
        assert_eq!(extension("trailing.exe. . "), "exe");
        assert_eq!(extension(".bashrc"), "bashrc");
    }

    #[test]
    fn kinds() {
        assert_eq!(kind_of_name("a.iso"), Kind::DiskImage);
        assert_eq!(kind_of_name("a.VHDX"), Kind::DiskImage);
        assert_eq!(kind_of_name("run.ps1"), Kind::Script);
        assert_eq!(kind_of_name("x.scr"), Kind::Script);
        assert_eq!(kind_of_name("y.lnk"), Kind::Shortcut);
        assert_eq!(kind_of_name("z.msix"), Kind::AppPackage);
        assert_eq!(kind_of_name("setup.msi"), Kind::Program);
        assert_eq!(kind_of_name("photos.zip"), Kind::Archive);
        assert_eq!(kind_of_name("cv.pdf"), Kind::Document);
        assert_eq!(kind_of_name("data.bin"), Kind::Other);
        assert!(Kind::Program.is_risky());
        assert!(!Kind::Archive.is_risky());
    }
}
