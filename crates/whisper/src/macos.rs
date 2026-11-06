//! macOS helpers: code signature checks and disk images.

use std::path::Path;
use std::process::{Command, Stdio};

/// Checks an app's signature with `codesign --verify --deep --strict` and that it was
/// signed by `authority` (for example "Developer ID Application: Mozilla Corporation").
pub fn verify_signed_by(app: &Path, authority: &str) -> Result<(), String> {
    verify(app)?;
    let out = Command::new("codesign")
        .args(["-dv", "--verbose=2"])
        .arg(app)
        .output()
        .map_err(|e| format!("codesign: {e}"))?;
    // codesign prints the details on stderr
    let details = String::from_utf8_lossy(&out.stderr);
    if details.lines().any(|l| {
        l.strip_prefix("Authority=")
            .is_some_and(|a| a.starts_with(authority))
    }) {
        Ok(())
    } else {
        Err(format!("{} is not signed by {authority}", app.display()))
    }
}

/// `codesign --verify --deep --strict`: the bundle is intact (an ad-hoc signature passes).
pub fn verify(app: &Path) -> Result<(), String> {
    let ok = Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        Ok(())
    } else {
        Err(format!(
            "the signature of {} does not verify",
            app.display()
        ))
    }
}

/// Mounts a disk image read-only at `mount`, runs `f`, and detaches it again.
pub fn with_mounted_dmg<T>(
    dmg: &Path,
    mount: &Path,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let attached = Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-quiet",
            "-mountpoint",
        ])
        .arg(mount)
        .arg(dmg)
        .status()
        .is_ok_and(|s| s.success());
    if !attached {
        return Err(format!("could not open {}", dmg.display()));
    }
    let result = f();
    let _ = Command::new("hdiutil")
        .args(["detach", "-quiet", "-force"])
        .arg(mount)
        .status();
    result
}
