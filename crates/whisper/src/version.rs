//! Release numbers such as `0.1.0`.
//!
//! Up to four numeric parts are compared; anything after `-` or `+` is kept for display
//! but ignored when ordering, so `0.2.0-rc1` and `0.2.0` count as the same release.

use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Debug)]
pub struct Version {
    parts: [u64; 4],
    text: String,
}

impl Version {
    /// Parses a release number; returns `None` for anything that is not one
    /// (an HTML error page, an empty file, a path).
    pub fn parse(input: &str) -> Option<Version> {
        let text = input.trim();
        if text.is_empty() || text.len() > 64 {
            return None;
        }
        if !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        {
            return None;
        }
        let core = text.split(['-', '+']).next()?;
        let mut parts = [0u64; 4];
        for (i, piece) in core.split('.').enumerate() {
            if i == 4
                || piece.is_empty()
                || piece.len() > 9
                || !piece.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            parts[i] = piece.parse().ok()?;
        }
        Some(Version {
            parts,
            text: text.to_string(),
        })
    }

    /// The version this binary was built as.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is a valid version")
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn orders_numerically_not_alphabetically() {
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0") > v("0.99.99"));
        assert!(v("0.1.1") > v("0.1.0"));
        assert!(v("2") > v("1.9"));
    }

    #[test]
    fn missing_parts_count_as_zero() {
        assert_eq!(v("1"), v("1.0.0"));
        assert_eq!(v("1.2"), v("1.2.0.0"));
    }

    #[test]
    fn suffixes_are_ignored_for_ordering() {
        assert_eq!(v("0.2.0-rc1"), v("0.2.0"));
        assert!(v("0.2.0+build.5") > v("0.1.9"));
        assert_eq!(v("0.2.0-rc1").as_str(), "0.2.0-rc1");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(v(" 0.1.0\n").as_str(), "0.1.0");
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            " ",
            "v1.0",
            "1..0",
            ".1",
            "1.",
            "1.0.0.0.0",
            "<html>",
            "1.0/../x",
            "1 0",
            "1234567890.0",
            "-1",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn current_version_parses() {
        assert_eq!(Version::current().as_str(), env!("CARGO_PKG_VERSION"));
    }
}
