//! HTTPS downloads with timeouts and size limits.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

const USER_AGENT: &str = concat!("Whisper/", env!("CARGO_PKG_VERSION"));

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(!allow_plain_http())
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(timeout))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

// Release builds only ever talk HTTPS. The integration tests serve files from 127.0.0.1.
#[cfg(feature = "test-hooks")]
fn allow_plain_http() -> bool {
    true
}

#[cfg(not(feature = "test-hooks"))]
fn allow_plain_http() -> bool {
    false
}

/// Fetches a small file into memory; fails if it is larger than `limit` bytes.
pub fn get(url: &str, limit: u64, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut response = agent(timeout)
        .get(url)
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| format!("{url}: {e}"))
}

/// Downloads `url` to `dest` (through `dest.part`, so a broken download never looks finished).
pub fn download(url: &str, dest: &Path, limit: u64, timeout: Duration) -> Result<(), String> {
    let part = dest.with_extension(match dest.extension() {
        Some(ext) => format!("{}.part", ext.to_string_lossy()),
        None => "part".to_string(),
    });
    let result = (|| {
        let mut response = agent(timeout)
            .get(url)
            .call()
            .map_err(|e| format!("{url}: {e}"))?;
        let mut reader = response.body_mut().with_config().limit(limit).reader();
        let mut file = File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        io::copy(&mut reader, &mut file).map_err(|e| format!("{url}: {e}"))?;
        file.flush().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&part, dest).map_err(|e| format!("{}: {e}", dest.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}
