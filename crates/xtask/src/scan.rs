//! A last line of defence before anything is pushed: no tokens, private keys, personal
//! e-mail addresses or paths from someone's machine in the files git tracks.

use std::fs;
use std::process::Command;

use regex::Regex;

use crate::root;

// Written so that these lines do not match themselves.
const RULES: &[(&str, &str)] = &[
    ("GitHub token", r"\bgh[pousr]_[A-Za-z0-9]{30,}"),
    ("GitHub token", r"\bgithub_pat_[A-Za-z0-9_]{30,}"),
    (
        "private key",
        r"-----BEGIN [A-Z ]*PRIVATE KEY( BLOCK)?-----",
    ),
    ("AWS key", r"\bAKIA[0-9A-Z]{16}\b"),
    ("Slack token", r"\bxox[abprs]-[A-Za-z0-9-]{10,}"),
    ("Google API key", r"\bAIza[0-9A-Za-z_-]{35}\b"),
    ("Stripe key", r"\b[sr]k_live_[0-9A-Za-z]{20,}"),
    ("JWT", r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\."),
    (
        "local path",
        r"/r[o]ot/|/h[o]me/[a-z]|/U[s]ers/[A-Za-z]|[A-Za-z]:\\U[s]ers\\[A-Za-z]",
    ),
    (
        "local path",
        r"/t[m]p/claude|\.research-c[a]che|scratch[p]ad",
    ),
    (
        "e-mail address",
        r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}",
    ),
];

/// Addresses that are identifiers, not people.
const ALLOWED_ADDRESSES: &[&str] = &[
    "uBlock0@raymondhill.net",
    "firefox-compact-dark@mozilla.org",
    "noreply@anthropic.com",
];

/// Third-party license texts are kept as published.
const SKIP: &[&str] = &["LICENSE", "assets/fonts/OFL.txt", "Cargo.lock"];

pub fn scan() -> Result<(), String> {
    let out = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("git ls-files failed".into());
    }
    let rules: Vec<(&str, Regex)> = RULES
        .iter()
        .map(|(name, re)| (*name, Regex::new(re).expect("valid rule")))
        .collect();
    let mut findings = Vec::new();
    let mut scanned = 0;
    for file in String::from_utf8_lossy(&out.stdout).split('\0') {
        if file.is_empty() || SKIP.contains(&file) {
            continue;
        }
        let Ok(bytes) = fs::read(root().join(file)) else {
            continue;
        };
        if bytes.contains(&0) {
            continue; // binary
        }
        scanned += 1;
        let text = String::from_utf8_lossy(&bytes);
        for (n, line) in text.lines().enumerate() {
            for (name, re) in &rules {
                for m in re.find_iter(line) {
                    // An add-on ID followed by a file extension (uBlock0@raymondhill.net.xpi) is fine too
                    if *name == "e-mail address"
                        && (ALLOWED_ADDRESSES
                            .iter()
                            .any(|a| m.as_str() == *a || m.as_str().starts_with(&format!("{a}.")))
                            || is_hidpi_image_name(m.as_str()))
                    {
                        continue;
                    }
                    findings.push(format!("{file}:{}: {name}: {}", n + 1, m.as_str()));
                }
            }
        }
    }
    report(findings, scanned)
}

/// `about-logo@2x.png`: the name of a high-resolution image, not an address.
fn is_hidpi_image_name(found: &str) -> bool {
    let Some((_, domain)) = found.split_once('@') else {
        return false;
    };
    let Some((scale, ext)) = domain.split_once('.') else {
        return false;
    };
    scale
        .strip_suffix('x')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && ["png", "jpg", "jpeg", "svg", "webp", "gif"].contains(&ext)
}

fn report(findings: Vec<String>, scanned: usize) -> Result<(), String> {
    if findings.is_empty() {
        println!("scan: {scanned} text files, nothing found");
        Ok(())
    } else {
        for f in &findings {
            eprintln!("{f}");
        }
        Err(format!("{} finding(s)", findings.len()))
    }
}
