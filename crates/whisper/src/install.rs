//! `whisper --install` and `whisper --uninstall` for the Linux tarball: puts Whisper in
//! the home folder (no root needed) with a menu entry and icon.
//!
//! Layout: `~/.local/share/whisper/app/` (program and share folder, swapped as a whole on
//! updates), `~/.local/bin/whisper` (link), `~/.local/share/applications/whisper.desktop`
//! and icons under `~/.local/share/icons/hicolor/`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{paths, util};

const DESKTOP_TEMPLATE: &str = include_str!("../../../packaging/linux/whisper.desktop");

pub fn app_dir() -> PathBuf {
    paths::xdg_data_home().join("whisper").join("app")
}

fn bin_link() -> PathBuf {
    paths::home().join(".local").join("bin").join("whisper")
}

fn desktop_file() -> PathBuf {
    paths::xdg_data_home()
        .join("applications")
        .join("whisper.desktop")
}

fn icon_files() -> Vec<(PathBuf, &'static str)> {
    let icons = paths::xdg_data_home().join("icons").join("hicolor");
    vec![
        (icons.join("256x256/apps/whisper.png"), "whisper-256.png"),
        (icons.join("512x512/apps/whisper.png"), "whisper-512.png"),
        (icons.join("scalable/apps/whisper.svg"), "whisper.svg"),
    ]
}

/// The desktop entry with `Exec=whisper` pointed at `program`.
pub fn desktop_entry(program: &Path) -> String {
    let quoted = format!(
        "\"{}\"",
        program
            .to_string_lossy()
            .replace('\\', "\\\\\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`")
    );
    DESKTOP_TEMPLATE
        .lines()
        .map(|line| match line.strip_prefix("Exec=whisper") {
            Some(rest) => format!("Exec={quoted}{rest}"),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Installs the copy of Whisper this program belongs to into the home folder.
pub fn install(exe: &Path) -> Result<PathBuf, String> {
    if !cfg!(target_os = "linux") {
        return Err(
            "--install is for the Linux tarball; use Whisper.dmg or Whisper-Setup.exe".into(),
        );
    }
    let src = exe.parent().ok_or("unknown program location")?;
    if !paths::is_share(&src.join("share")) {
        return Err(format!("{} has no share folder next to it", exe.display()));
    }
    let dest = app_dir();
    let program = dest.join("whisper");
    let same = dest.canonicalize().ok() == src.canonicalize().ok();
    if !same {
        fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        let staged = util::sibling(&dest, "new");
        let _ = fs::remove_dir_all(&staged);
        util::copy_dir(&src.join("share"), &staged.join("share")).map_err(|e| e.to_string())?;
        fs::copy(exe, staged.join("whisper")).map_err(|e| e.to_string())?;
        set_executable(&staged.join("whisper"))?;
        util::swap_in(&staged, &dest).map_err(|e| e.to_string())?;
    }

    // ~/.local/bin/whisper, unless something else already has that name
    let link = bin_link();
    fs::create_dir_all(link.parent().unwrap()).map_err(|e| e.to_string())?;
    match fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::remove_file(&link).map_err(|e| e.to_string())?;
            symlink(&program, &link)?;
        }
        Ok(_) => crate::warn!(
            "{} exists and is not Whisper's; leaving it alone",
            link.display()
        ),
        Err(_) => symlink(&program, &link)?,
    }

    for (target, name) in icon_files() {
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::copy(dest.join("share/assets/icon").join(name), &target)
            .map_err(|e| format!("{}: {e}", target.display()))?;
    }
    let desktop = desktop_file();
    fs::create_dir_all(desktop.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&desktop, desktop_entry(&program)).map_err(|e| e.to_string())?;
    refresh_menus();
    Ok(program)
}

/// Removes the home-folder install. `purge` also deletes the Whisper profile.
pub fn uninstall(purge: bool) -> Result<Vec<PathBuf>, String> {
    let mut removed = Vec::new();
    let link = bin_link();
    if fs::read_link(&link).is_ok_and(|t| t.starts_with(app_dir())) {
        fs::remove_file(&link).map_err(|e| e.to_string())?;
        removed.push(link);
    }
    let desktop = desktop_file();
    if fs::read_to_string(&desktop).is_ok_and(|t| t.contains("StartupWMClass=Whisper")) {
        fs::remove_file(&desktop).map_err(|e| e.to_string())?;
        removed.push(desktop);
    }
    for (icon, _) in icon_files() {
        if fs::remove_file(&icon).is_ok() {
            removed.push(icon);
        }
    }
    if app_dir().exists() {
        fs::remove_dir_all(app_dir()).map_err(|e| e.to_string())?;
        removed.push(app_dir());
    }
    if purge {
        for dir in paths::all_data_dirs() {
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                removed.push(dir);
            }
        }
    }
    refresh_menus();
    Ok(removed)
}

fn refresh_menus() {
    if let Some(tool) = crate::firefox::search_path("update-desktop-database") {
        let _ = std::process::Command::new(tool)
            .arg(paths::xdg_data_home().join("applications"))
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|e| format!("{}: {e}", link.display()))
}

#[cfg(not(unix))]
fn symlink(_: &Path, _: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn set_executable(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_points_every_exec_at_the_program() {
        let text = desktop_entry(Path::new("/opt/my apps/whisper"));
        let execs: Vec<&str> = text.lines().filter(|l| l.starts_with("Exec=")).collect();
        assert_eq!(
            execs,
            [
                "Exec=\"/opt/my apps/whisper\" %u",
                "Exec=\"/opt/my apps/whisper\" --new-window",
                "Exec=\"/opt/my apps/whisper\" --private-window",
                "Exec=\"/opt/my apps/whisper\" --import",
            ]
        );
        assert!(text.contains("StartupWMClass=Whisper"));
    }
}
