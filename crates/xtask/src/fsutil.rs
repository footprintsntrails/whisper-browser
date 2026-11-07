//! Small file helpers.

use std::fs;
use std::io;
use std::path::Path;

pub fn err(context: impl std::fmt::Display) -> impl Fn(io::Error) -> String {
    move |e| format!("{context}: {e}")
}

pub fn write(path: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(err(parent.display()))?;
    }
    fs::write(path, data).map_err(err(path.display()))
}
