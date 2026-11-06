//! Unpacking the Linux release tarball for a self-update.

use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

use tar::EntryType;

/// Unpacks `archive` (a .tar.gz with one top-level folder) into `dest`, dropping that
/// folder. Only plain files and folders are accepted: no links, no absolute paths, no `..`.
pub fn extract_tar_gz_strip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    tar.set_preserve_permissions(true);
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let entries = tar.entries().map_err(|e| format!("broken archive: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("broken archive: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("broken archive: {e}"))?
            .into_owned();
        let rel: PathBuf = path.components().skip(1).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(format!("unsafe path in archive: {}", path.display()));
        }
        let target = dest.join(&rel);
        match entry.header().entry_type() {
            EntryType::Directory => fs::create_dir_all(&target).map_err(|e| e.to_string())?,
            EntryType::Regular | EntryType::Continuous => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                entry
                    .unpack(&target)
                    .map_err(|e| format!("{}: {e}", target.display()))?;
            }
            other => {
                return Err(format!(
                    "unexpected {other:?} entry in archive: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;

    /// Builds a .tar.gz from (path, contents, mode) entries.
    pub(crate) fn make_tar_gz(path: &Path, files: &[(&str, &[u8], u32)]) {
        let gz = GzEncoder::new(File::create(path).unwrap(), Compression::fast());
        let mut builder = tar::Builder::new(gz);
        for (name, data, mode) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(*mode);
            header.set_entry_type(EntryType::Regular);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn strips_the_top_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("a.tar.gz");
        make_tar_gz(
            &archive,
            &[
                ("whisper-linux/whisper", b"bin", 0o755),
                ("whisper-linux/share/VERSION", b"0.2.0\n", 0o644),
            ],
        );
        let dest = tmp.path().join("out");
        extract_tar_gz_strip(&archive, &dest).unwrap();
        assert_eq!(fs::read(dest.join("whisper")).unwrap(), b"bin");
        assert_eq!(fs::read(dest.join("share/VERSION")).unwrap(), b"0.2.0\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dest.join("whisper"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn rejects_links() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("a.tar.gz");
        let gz = GzEncoder::new(File::create(&archive).unwrap(), Compression::fast());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        builder
            .append_link(&mut header, "top/evil", "/etc/passwd")
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();
        let err = extract_tar_gz_strip(&archive, &tmp.path().join("out")).unwrap_err();
        assert!(err.contains("unexpected"), "{err}");
    }
}
