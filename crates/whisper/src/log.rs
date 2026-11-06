//! A small log file in the Whisper data folder (`whisper.log`), useful for bug reports.
//! Nothing in it leaves the machine.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG: Mutex<Option<(File, PathBuf)>> = Mutex::new(None);
const MAX_SIZE: u64 = 256 * 1024;

/// Starts logging to `path`, keeping one older file (`whisper.log.1`) once it grows large.
pub fn init(path: &Path) {
    if let Ok(meta) = fs::metadata(path)
        && meta.len() > MAX_SIZE
    {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(path)
        && let Ok(mut slot) = LOG.lock()
    {
        *slot = Some((file, path.to_path_buf()));
    }
}

/// The current log file, if logging has started.
pub fn path() -> Option<PathBuf> {
    LOG.lock().ok()?.as_ref().map(|(_, p)| p.clone())
}

pub fn write(level: &str, message: &str) {
    let line = format!(
        "{} [{}] {} {}\n",
        timestamp(),
        std::process::id(),
        level,
        message
    );
    if let Ok(mut slot) = LOG.lock()
        && let Some((file, _)) = slot.as_mut()
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Logs a line to the log file.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::write("info", &format!($($arg)*)) };
}

/// Logs a line to the log file and to stderr.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        let message = format!($($arg)*);
        eprintln!("whisper: {message}");
        $crate::log::write("warn", &message);
    }};
}

/// UTC time as `YYYY-MM-DD HH:MM:SSZ`, without pulling in a date crate.
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_763), (2026, 11, 6));
    }
}
