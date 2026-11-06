//! On Windows, embeds the icon, version information and application manifest into
//! whisper.exe.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let icon = root.join("assets/icon/whisper.ico");
    let manifest = root.join("packaging/windows/whisper.manifest");
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-changed={}", manifest.display());

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numbers = [
        env::var("CARGO_PKG_VERSION_MAJOR").unwrap(),
        env::var("CARGO_PKG_VERSION_MINOR").unwrap(),
        env::var("CARGO_PKG_VERSION_PATCH").unwrap(),
        "0".to_string(),
    ]
    .join(",");
    let path = |p: PathBuf| {
        p.canonicalize()
            .unwrap_or(p)
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('\\', "\\\\")
    };
    let rc = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Whisper"
      VALUE "FileDescription", "Whisper"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "whisper"
      VALUE "LegalCopyright", "MPL-2.0"
      VALUE "OriginalFilename", "whisper.exe"
      VALUE "ProductName", "Whisper"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = path(icon),
        manifest = path(manifest),
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("whisper.rc");
    fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
