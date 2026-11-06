// No console window on Windows: Whisper is started from shortcuts.
#![windows_subsystem = "windows"]

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    #[cfg(feature = "test-hooks")]
    test_double(&args);
    std::process::exit(whisper::cli::run(args));
}

/// Test builds only: a copy of this program named `firefox`, `flatpak` or `Whisper-Setup`
/// records its arguments instead of doing anything, so the tests need no real browser.
#[cfg(feature = "test-hooks")]
fn test_double(args: &[std::ffi::OsString]) {
    let exe = std::env::current_exe().unwrap_or_default();
    let name = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !["firefox", "flatpak", "Whisper-Setup"].contains(&name.as_str()) {
        return;
    }
    if name == "flatpak" && args.first().is_some_and(|a| a == "info") {
        std::process::exit(0);
    }
    if let Some(out) = std::env::var_os("WHISPER_TEST_ARGS_FILE") {
        let text: String = std::iter::once(name)
            .chain(args.iter().map(|a| a.to_string_lossy().into_owned()))
            .map(|a| a + "\n")
            .collect();
        // Written whole, then renamed, so a test never reads half of it
        let tmp = std::path::PathBuf::from(&out).with_extension("partial");
        std::fs::write(&tmp, text).expect("write the argument file");
        std::fs::rename(&tmp, &out).expect("rename the argument file");
    }
    std::process::exit(0);
}
