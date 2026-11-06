//! Deterministic archives and small file helpers.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::{Compression, GzBuilder};

/// Timestamp for files in archives and packages: `SOURCE_DATE_EPOCH`, else 2020-01-01,
/// so the same commit always gives the same files.
pub fn epoch() -> u64 {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_577_836_800)
}

pub fn err(context: impl std::fmt::Display) -> impl Fn(io::Error) -> String {
    move |e| format!("{context}: {e}")
}

/// All files under `dir`, as sorted relative paths with `/` separators.
pub fn walk(dir: &Path) -> Result<Vec<String>, String> {
    fn go(base: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
        let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            if e.file_type()?.is_dir() {
                go(base, &path, out)?;
            } else {
                let rel = path.strip_prefix(base).unwrap();
                out.push(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    go(dir, dir, &mut out).map_err(err(dir.display()))?;
    Ok(out)
}

pub fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    for rel in walk(src)? {
        let target = dest.join(&rel);
        fs::create_dir_all(target.parent().unwrap()).map_err(err(target.display()))?;
        fs::copy(src.join(&rel), &target).map_err(err(target.display()))?;
    }
    Ok(())
}

pub fn fresh_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(err(dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(err(dir.display()))
}

pub fn write(path: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(err(parent.display()))?;
    }
    fs::write(path, data).map_err(err(path.display()))
}

/// Files that get the executable bit in archives and packages.
fn is_executable(rel: &str) -> bool {
    rel == "whisper" || rel.ends_with("/whisper") || rel.ends_with(".sh")
}

/// Writes `dir` as a .tar.gz with every path under `prefix/`, sorted, owned by root and
/// with fixed timestamps.
pub fn tar_gz(dir: &Path, prefix: &str, out: &Path) -> Result<(), String> {
    let file = File::create(out).map_err(err(out.display()))?;
    let gz = GzBuilder::new().mtime(0).write(file, Compression::best());
    let mut tar = tar::Builder::new(gz);
    tar.mode(tar::HeaderMode::Deterministic);
    let files = walk(dir)?;
    let mut dirs: Vec<String> = Vec::new();
    for rel in &files {
        let mut parts: Vec<&str> = rel.split('/').collect();
        parts.pop();
        for i in 1..=parts.len() {
            let d = parts[..i].join("/");
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    }
    let header = |path: &str, size: u64, mode: u32, kind: tar::EntryType| {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(kind);
        h.set_size(size);
        h.set_mode(mode);
        h.set_mtime(epoch());
        h.set_uid(0);
        h.set_gid(0);
        h.set_cksum();
        (h, format!("{prefix}/{path}"))
    };
    let (mut h, name) = header("", 0, 0o755, tar::EntryType::Directory);
    tar.append_data(&mut h, name.trim_end_matches('/'), io::empty())
        .map_err(err(out.display()))?;
    let mut entries: Vec<(String, bool)> = dirs.into_iter().map(|d| (d, true)).collect();
    entries.extend(files.into_iter().map(|f| (f, false)));
    entries.sort();
    for (rel, is_dir) in entries {
        if is_dir {
            let (mut h, name) = header(&rel, 0, 0o755, tar::EntryType::Directory);
            tar.append_data(&mut h, name, io::empty())
                .map_err(err(out.display()))?;
        } else {
            let data = fs::read(dir.join(&rel)).map_err(err(&rel))?;
            let mode = if is_executable(&rel) { 0o755 } else { 0o644 };
            let (mut h, name) = header(&rel, data.len() as u64, mode, tar::EntryType::Regular);
            tar.append_data(&mut h, name, data.as_slice())
                .map_err(err(out.display()))?;
        }
    }
    tar.into_inner()
        .and_then(|gz| gz.finish())
        .and_then(|mut f| f.flush())
        .map_err(err(out.display()))
}

/// Writes `dir` as a .zip with every path under `prefix/`, sorted, with fixed timestamps.
pub fn zip(dir: &Path, prefix: &str, out: &Path) -> Result<(), String> {
    use zip::write::SimpleFileOptions;
    let file = File::create(out).map_err(err(out.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let (y, m, d) = civil_from_days((epoch() / 86_400) as i64);
    let time = zip::DateTime::from_date_and_time(y.max(1980) as u16, m as u8, d as u8, 0, 0, 0)
        .map_err(|e| e.to_string())?;
    for rel in walk(dir)? {
        let mode = if is_executable(&rel) { 0o755 } else { 0o644 };
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(time)
            .unix_permissions(mode);
        zip.start_file(format!("{prefix}/{rel}"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&fs::read(dir.join(&rel)).map_err(err(&rel))?)
            .map_err(err(&rel))?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Sets the modification time of everything under `dir` (packages built from it are then
/// reproducible).
pub fn set_times(dir: &Path) -> Result<(), String> {
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch());
    fn go(path: &Path, time: std::time::SystemTime) -> io::Result<()> {
        if path.is_dir() {
            for e in fs::read_dir(path)? {
                go(&e?.path(), time)?;
            }
        }
        File::open(path)?.set_modified(time)
    }
    go(dir, time).map_err(err(dir.display()))
}

#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(err(path.display()))
}

#[cfg(not(unix))]
pub fn set_mode(_: &Path, _: u32) -> Result<(), String> {
    Ok(())
}

/// Sets 0755 on folders and executables and 0644 on everything else under `dir`.
pub fn normalize_modes(dir: &Path) -> Result<(), String> {
    set_mode(dir, 0o755)?;
    for rel in walk(dir)? {
        let path = dir.join(&rel);
        let mut parent = path.parent();
        while let Some(p) = parent {
            if p == dir {
                break;
            }
            set_mode(p, 0o755)?;
            parent = p.parent();
        }
        set_mode(&path, if is_executable(&rel) { 0o755 } else { 0o644 })?;
    }
    Ok(())
}

/// Runs a command and fails with its output if it does not succeed.
pub fn run(cmd: &mut Command) -> Result<(), String> {
    let shown = format!("{cmd:?}");
    let out = cmd.output().map_err(|e| format!("{shown}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{shown} failed ({})\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

pub fn read_version() -> Result<String, String> {
    let path = crate::dist().join("VERSION");
    fs::read_to_string(&path)
        .map(|v| v.trim().to_string())
        .map_err(|e| format!("{}: {e} (run cargo xtask bundle first)", path.display()))
}

pub fn share_dir() -> Result<PathBuf, String> {
    let share = crate::build().join("share");
    if share.join("VERSION").is_file() {
        Ok(share)
    } else {
        Err("build/share is missing; run cargo xtask bundle first".into())
    }
}

/// Total size in KiB (for the Debian Installed-Size field and the Windows uninstall entry).
pub fn size_kib(dir: &Path) -> Result<u64, String> {
    let mut total = 0;
    for rel in walk(dir)? {
        total += fs::metadata(dir.join(rel))
            .map_err(|e| e.to_string())?
            .len();
    }
    Ok(total.div_ceil(1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_are_reproducible() {
        let tmp = std::env::temp_dir().join(format!("xtask-test-{}", std::process::id()));
        let src = tmp.join("src");
        write(&src.join("b/c.txt"), "c").unwrap();
        write(&src.join("a.txt"), "a").unwrap();
        write(&src.join("whisper"), "bin").unwrap();
        for (kind, name) in [("tar", "x.tar.gz"), ("zip", "x.zip")] {
            let one = tmp.join(format!("1-{name}"));
            let two = tmp.join(format!("2-{name}"));
            for out in [&one, &two] {
                if kind == "tar" {
                    tar_gz(&src, "top", out).unwrap();
                } else {
                    zip(&src, "top", out).unwrap();
                }
            }
            assert_eq!(fs::read(&one).unwrap(), fs::read(&two).unwrap(), "{kind}");
        }
        let _ = fs::remove_dir_all(&tmp);
        assert_eq!(civil_from_days(18_262), (2020, 1, 1));
    }
}
