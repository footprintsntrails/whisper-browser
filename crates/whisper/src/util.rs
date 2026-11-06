//! Small file helpers.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A temporary folder that is removed when dropped.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Result<TempDir, String> {
        Self::new_in(&std::env::temp_dir(), label)
    }

    pub fn new_in(parent: &Path, label: &str) -> Result<TempDir, String> {
        let path = parent.join(format!(".whisper-{label}-{}", unique()));
        fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(TempDir(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn unique() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(
        "{}-{nanos:x}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Copies a folder tree. Symlinks are skipped: nothing Whisper ships uses them.
pub fn copy_dir(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Replaces folder `dest` with a copy of `src`: the copy is made next to it first,
/// so `dest` is never left half-written.
pub fn replace_dir_with_copy(src: &Path, dest: &Path) -> io::Result<()> {
    let staged = sibling(dest, "new");
    let _ = fs::remove_dir_all(&staged);
    copy_dir(src, &staged)?;
    swap_in(&staged, dest)
}

/// Moves the folder `staged` to `dest`, keeping the old `dest` until the move worked.
pub fn swap_in(staged: &Path, dest: &Path) -> io::Result<()> {
    let old = sibling(dest, "old");
    let _ = fs::remove_dir_all(&old);
    let had_old = dest.exists();
    if had_old {
        fs::rename(dest, &old)?;
    }
    if let Err(e) = fs::rename(staged, dest) {
        if had_old {
            let _ = fs::rename(&old, dest);
        }
        return Err(e);
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(())
}

/// `name.new`, `name.old` next to `path`.
pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

/// Writes a file through a temporary name in the same folder, then renames it.
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = sibling(path, "tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Creates a folder that only the current user can read (the profile holds history and passwords).
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".whisper-write-test-{}", unique()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// How long ago a file was modified, in seconds (`None` if it does not exist).
pub fn age_secs(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

/// Sets a file's modification time to now (creating it if needed).
pub fn touch(path: &Path) -> io::Result<()> {
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.set_modified(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_dir_swaps_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/a.txt"), "new").unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("stale.txt"), "old").unwrap();

        replace_dir_with_copy(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(dest.join("sub/a.txt")).unwrap(), "new");
        assert!(!dest.join("stale.txt").exists());
        assert!(!sibling(&dest, "new").exists());
        assert!(!sibling(&dest, "old").exists());
    }

    #[test]
    fn write_atomic_replaces_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("f");
        write_atomic(&file, b"one").unwrap();
        write_atomic(&file, b"two").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"two");
        assert!(!sibling(&file, "tmp").exists());
    }
}
