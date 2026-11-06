//! The `whisper` command.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::firefox::{self, Firefox};
use crate::import::{self, Choice};
use crate::version::Version;
use crate::{info, paths, profile, ui, update, warn};

const HELP: &str = "\
Whisper: Firefox with a quiet profile. It can't make you invisible. It just won't snitch.

Usage: whisper [FIREFOX ARGUMENTS…]   start Whisper; arguments (URLs, --private-window, …) go to Firefox
       whisper --import               bring bookmarks, history and passwords from another browser
       whisper --update               check for a new version now
       whisper --install              Linux tarball: install into your home folder
       whisper --uninstall [--purge]  Linux tarball: remove it (--purge also deletes the Whisper profile)
       whisper --version | --help

Environment: WHISPER_OFFLINE=1 (no downloads by Whisper itself), WHISPER_FIREFOX=<path>,
WHISPER_IMPORT=<firefox|zen|librewolf|floorp|waterfox|wizard|none>, WHISPER_NONINTERACTIVE=1.
";

/// Runs Whisper with the given arguments and returns the process exit code.
pub fn run(args: Vec<OsString>) -> i32 {
    let first = args.first().map(|a| a.to_string_lossy().into_owned());
    match first.as_deref() {
        Some("--help" | "-help" | "-h") => print(HELP),
        Some("--version" | "-version" | "-v") => {
            print(&format!("Whisper {}\n", Version::current()))
        }
        Some("--update") => update_now(),
        Some("--background-update") => {
            background_update();
            0
        }
        Some("--install") => {
            console();
            match crate::install::install(&paths::current_exe()) {
                Ok(program) => {
                    println!("Whisper is installed: {}", program.display());
                    println!("Start it from your applications menu, or run: whisper");
                    0
                }
                Err(e) => fail(&e),
            }
        }
        Some("--uninstall" | "--purge") => {
            console();
            let purge = args.iter().any(|a| a == "--purge");
            match crate::install::uninstall(purge) {
                Ok(removed) => {
                    for path in removed {
                        println!("removed {}", path.display());
                    }
                    if !purge {
                        println!(
                            "Your Whisper profile is kept; use --uninstall --purge to delete it."
                        );
                    }
                    0
                }
                Err(e) => fail(&e),
            }
        }
        Some("--import") => launch(&args[1..], true),
        _ => launch(&args, false),
    }
}

fn console() {
    #[cfg(windows)]
    crate::winapi::attach_parent_console();
}

fn print(text: &str) -> i32 {
    console();
    print!("{text}");
    0
}

fn fail(message: &str) -> i32 {
    warn!("{message}");
    1
}

/// Starts Whisper: set up the profile, maybe import, start Firefox, and leave the update
/// check to a background process.
fn launch(user_args: &[OsString], import_requested: bool) -> i32 {
    let exe = paths::current_exe();
    let Some(share) = paths::find_share(&exe) else {
        let message = "Whisper's settings folder is missing. Please reinstall Whisper.";
        ui::error("Whisper", message);
        return fail(message);
    };
    let firefox = match firefox::find() {
        Some(ff) => ff,
        None => match firefox::install() {
            Ok(ff) => ff,
            Err(e) => return fail(&e),
        },
    };
    let data = paths::data_dir(firefox.flavor);
    if let Err(e) = crate::util::create_private_dir(&data) {
        return fail(&format!("cannot create {}: {e}", data.display()));
    }
    crate::log::init(&data.join("whisper.log"));
    info!(
        "Whisper {} starting; Firefox {} ({:?}); share {}",
        Version::current(),
        firefox.program.display(),
        firefox.flavor,
        share.display()
    );
    clean_up_after_windows_update(&exe);

    let profile_dir = profile::profile_dir(&data);
    let first_run = match profile::prepare(&share, &data) {
        Ok(p) => {
            if p.theme_updated {
                info!("settings and theme applied from {}", share.display());
            }
            p.first_run
        }
        Err(e) => {
            warn!("could not update the Whisper profile: {e}");
            if !profile_dir.join("user.js").is_file() {
                return 1;
            }
            false
        }
    };
    let offline = paths::offline(&data);
    if first_run && !offline {
        // Fetched before the first start so uBlock Origin is there from the beginning;
        // if this fails the background job tries again.
        match profile::fetch_ubo(&data, std::time::Duration::from_secs(60)) {
            Ok(()) => info!("uBlock Origin downloaded from addons.mozilla.org"),
            Err(e) => info!("uBlock Origin download failed, will retry later: {e}"),
        }
    }

    let mut extra: Vec<&str> = Vec::new();
    if (first_run || import_requested) && run_import(&profile_dir, import_requested) {
        extra.push("--migration");
    }

    if !offline && background_allowed() && (update::due(&data) || profile::ubo_pending(&data)) {
        spawn_background(&exe);
    }
    start_firefox(&firefox, &profile_dir, &extra, user_args)
}

/// Returns true when Firefox should open its import wizard.
fn run_import(profile_dir: &Path, on_demand: bool) -> bool {
    let ctx = import::Ctx::from_env();
    let os = import::Os::current();
    let sources = import::find_sources(&ctx, os);
    let choice = import::choice_from_env(&sources)
        .unwrap_or_else(|| import::ask(&sources, &import::wizard_browsers(&ctx, os), on_demand));
    match choice {
        Choice::Nothing => false,
        Choice::Wizard => {
            if profile::in_use(profile_dir) {
                ui::info(
                    "Whisper",
                    "Whisper is open. Close it, then choose Import again.",
                );
                info!("import: Whisper is running, so the wizard cannot open");
                return false;
            }
            info!("import: opening Firefox's import wizard");
            true
        }
        Choice::Copy { source, cookies } => {
            copy_from(&source, profile_dir, cookies);
            false
        }
    }
}

fn copy_from(source: &import::Source, profile_dir: &Path, cookies: bool) {
    if !profile::is_unused(profile_dir) {
        let message = format!(
            "{} data can only be copied into a new Whisper profile, and this one is already in use. \
             Use the import window in Whisper's settings instead.",
            source.name
        );
        ui::info("Whisper", &message);
        info!("import: {message}");
        return;
    }
    if profile::in_use(profile_dir) {
        ui::info(
            "Whisper",
            "Whisper is open. Close it, then choose Import again.",
        );
        info!("import: Whisper is running");
        return;
    }
    while profile::in_use(&source.profile) {
        let retry = ui::ask(
            "Whisper",
            &format!(
                "{0} is open. Close {0} so its data can be copied.\n\nTry again now?",
                source.name
            ),
            "Try again",
            "Skip",
        );
        if retry != Some(true) {
            info!("import: {} is running; skipped", source.name);
            return;
        }
    }
    match import::copy_profile(&source.profile, profile_dir, cookies) {
        Ok(files) => info!(
            "import: copied {} from {} ({})",
            files.join(", "),
            source.name,
            source.profile.display()
        ),
        Err(e) => warn!("import from {} failed: {e}", source.name),
    }
}

#[cfg(feature = "test-hooks")]
fn background_allowed() -> bool {
    !paths::env_flag("WHISPER_TEST_NO_BACKGROUND")
}

#[cfg(not(feature = "test-hooks"))]
fn background_allowed() -> bool {
    true
}

fn spawn_background(exe: &Path) {
    let mut cmd = Command::new(exe);
    cmd.arg("--background-update")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::winapi::DETACHED | crate::winapi::CREATE_NO_WINDOW);
    }
    match cmd.spawn() {
        Ok(_) => info!("background update check started"),
        Err(e) => info!("could not start the background update check: {e}"),
    }
}

fn background_update() {
    let exe = paths::current_exe();
    let flavor = firefox::find().map_or(firefox::Flavor::Native, |f| f.flavor);
    let data = paths::data_dir(flavor);
    if paths::offline(&data) || !data.is_dir() {
        return;
    }
    crate::log::init(&data.join("whisper.log"));
    update::run_background(&data, &exe);
}

fn update_now() -> i32 {
    console();
    let exe = paths::current_exe();
    let flavor = firefox::find().map_or(firefox::Flavor::Native, |f| f.flavor);
    let data = paths::data_dir(flavor);
    if let Err(e) = crate::util::create_private_dir(&data) {
        return fail(&format!("cannot create {}: {e}", data.display()));
    }
    crate::log::init(&data.join("whisper.log"));
    let result = update::check(&data, &exe);
    let code = match &result {
        Ok(update::Outcome::UpToDate(v)) => {
            println!(
                "Whisper {} is up to date (latest release: {v}).",
                Version::current()
            );
            0
        }
        Ok(update::Outcome::Installed(v)) => {
            println!("Whisper {v} is installed and starts next time.");
            0
        }
        Ok(update::Outcome::Notified(v)) => {
            println!(
                "Whisper {v} is available. Download the new package from https://github.com/{}/releases/latest",
                update::REPO
            );
            0
        }
        Ok(update::Outcome::Skipped(why)) => {
            println!("This copy of Whisper does not update itself: {why}.");
            0
        }
        Err(e) => {
            eprintln!("whisper: update check failed: {e}");
            1
        }
    };
    update::log_outcome(result);
    let _ = crate::util::touch(&data.join("last-update-check"));
    code
}

/// The Windows installer renames a running whisper.exe to whisper.exe.old.
fn clean_up_after_windows_update(exe: &Path) {
    if cfg!(windows) {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }
}

#[cfg(unix)]
fn start_firefox(ff: &Firefox, profile: &Path, extra: &[&str], user_args: &[OsString]) -> i32 {
    use std::os::unix::process::CommandExt;
    let mut cmd = firefox::command(ff, profile, extra, user_args);
    info!("starting {:?}", cmd);
    // exec: Whisper becomes Firefox, so the desktop sees one program
    let err = cmd.exec();
    fail(&format!(
        "could not start Firefox ({}): {err}",
        ff.program.display()
    ))
}

#[cfg(not(unix))]
fn start_firefox(ff: &Firefox, profile: &Path, extra: &[&str], user_args: &[OsString]) -> i32 {
    let mut cmd = firefox::command(ff, profile, extra, user_args);
    info!("starting {:?}", cmd);
    match cmd.spawn() {
        Ok(_) => 0,
        Err(e) => {
            let message = format!("could not start Firefox ({}): {e}", ff.program.display());
            ui::error("Whisper", &message);
            fail(&message)
        }
    }
}
