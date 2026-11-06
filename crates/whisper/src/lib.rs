//! Whisper: starts Firefox with the Whisper profile, keeps Whisper up to date and imports
//! from other browsers. `main.rs` only calls [`cli::run`].

pub mod log;

pub mod archive;
pub mod cli;
pub mod firefox;
pub mod import;
pub mod install;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod net;
pub mod paths;
pub mod profile;
pub mod signature;
pub mod ui;
pub mod update;
pub mod util;
pub mod version;
#[cfg(windows)]
pub mod winapi;
