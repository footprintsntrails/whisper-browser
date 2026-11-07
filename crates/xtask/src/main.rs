//! Build tooling for Whisper. Run `cargo xtask help`.

mod art;
mod fsutil;
mod scan;

use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "\
cargo xtask <command> [options]

Artwork (results are committed):
  icons                                assets/icon/* from assets/logo/*.svg
  banner --font-dir DIR                assets/readme/banner.png (Inter TTFs in DIR)
  installer-art --font-dir DIR         assets/installer/*.bmp for the Windows installer
  gecko-branding --font-dir DIR        images in gecko/overlay/browser/branding/whisper
  canary-art --font-dir DIR            Whisper Canary's logo, icon, installer art and branding
                                       images in gecko/variants/canary

Checks:
  scan                                 look for secrets, personal data and local paths in tracked files
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        print!("{HELP}");
        return ExitCode::FAILURE;
    };
    let opts = Opts(args[1..].to_vec());
    let result = match command.as_str() {
        "icons" => art::icons(),
        "banner" => art::banner(&opts),
        "installer-art" => art::installer_art(&opts),
        "gecko-branding" => art::gecko_branding(&opts),
        "canary-art" => art::canary_art(&opts),
        "scan" => scan::scan(),
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}; see cargo xtask help")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {command}: {e}");
            ExitCode::FAILURE
        }
    }
}

pub struct Opts(Vec<String>);

impl Opts {
    pub fn get(&self, name: &str) -> Option<String> {
        let flag = format!("--{name}");
        let mut it = self.0.iter();
        while let Some(a) = it.next() {
            if *a == flag {
                return it.next().cloned();
            }
            if let Some(v) = a.strip_prefix(&format!("{flag}=")) {
                return Some(v.to_string());
            }
        }
        None
    }

    pub fn path(&self, name: &str) -> Result<PathBuf, String> {
        self.get(name)
            .map(PathBuf::from)
            .ok_or_else(|| format!("--{name} is required"))
    }
}

/// The repository root.
pub fn root() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    dir.canonicalize().unwrap_or(dir)
}

/// Work files that are not committed (build/, ignored by git).
pub fn build() -> PathBuf {
    root().join("build")
}
