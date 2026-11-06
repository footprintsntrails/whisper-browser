//! The share folder and the packages built around it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fsutil::{self, err, run};
use crate::{Opts, build, dist, root};

const ICONS: &[&str] = &[
    "whisper.ico",
    "whisper.icns",
    "whisper-256.png",
    "whisper-512.png",
    "whisper.svg",
];

/// build/share (what every package installs) plus the profile bundles and VERSION in dist/.
pub fn bundle(opts: &Opts) -> Result<(), String> {
    let version = opts
        .get("version")
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    if whisper::version::Version::parse(&version).is_none() {
        return Err(format!("{version:?} is not a version"));
    }
    let share = build().join("share");
    fsutil::fresh_dir(&share)?;
    fsutil::fresh_dir(&dist())?;
    fsutil::copy_dir(&root().join("profile"), &share.join("profile"))?;
    for icon in ICONS {
        let src = root().join("assets/icon").join(icon);
        fsutil::write(
            &share.join("assets/icon").join(icon),
            fs::read(&src).map_err(err(src.display()))?,
        )?;
    }
    fs::copy(root().join("LICENSE"), share.join("LICENSE")).map_err(err("LICENSE"))?;
    fsutil::write(&share.join("VERSION"), format!("{version}\n"))?;
    fsutil::write(&dist().join("VERSION"), format!("{version}\n"))?;
    fsutil::tar_gz(
        &share,
        "whisper-profile",
        &dist().join("whisper-profile.tar.gz"),
    )?;
    fsutil::zip(
        &share,
        "whisper-profile",
        &dist().join("whisper-profile.zip"),
    )?;
    println!("share folder {version}: {}", share.display());
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    fn from_opts(opts: &Opts) -> Result<Arch, String> {
        match opts.get("arch").as_deref() {
            None | Some("x86_64" | "amd64") => Ok(Arch::X86_64),
            Some("aarch64" | "arm64") => Ok(Arch::Aarch64),
            Some(other) => Err(format!("unknown --arch {other}")),
        }
    }

    fn deb(self) -> &'static str {
        match self {
            Arch::X86_64 => "amd64",
            Arch::Aarch64 => "arm64",
        }
    }

    fn rpm(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        }
    }
}

fn binary(opts: &Opts) -> Result<PathBuf, String> {
    let bin = opts.path("bin")?;
    if bin.is_file() {
        Ok(bin)
    } else {
        Err(format!("{} does not exist", bin.display()))
    }
}

const TARBALL_README: &str = "\
Whisper for Linux
=================

Run it from here:      ./whisper
Install it for you:    ./whisper --install
                       (into ~/.local, with a menu entry; no root needed;
                        this copy then keeps itself up to date)
Remove it again:       whisper --uninstall          (keeps your Whisper profile)
                       whisper --uninstall --purge  (deletes it too)

Whisper needs Firefox from your package manager (native, Snap or Flatpak).
More: https://github.com/footprintsntrails/whisper-browser
";

/// whisper-linux.tar.gz: the program, the share folder and a README, for any distribution.
pub fn tarball(opts: &Opts) -> Result<(), String> {
    let arch = Arch::from_opts(opts)?;
    let bin = binary(opts)?;
    let share = fsutil::share_dir()?;
    let stage = build().join(format!("tarball-{}", arch.rpm()));
    fsutil::fresh_dir(&stage)?;
    fs::copy(&bin, stage.join("whisper")).map_err(err(bin.display()))?;
    fsutil::copy_dir(&share, &stage.join("share"))?;
    fsutil::write(&stage.join("README.txt"), TARBALL_README)?;
    fs::copy(root().join("LICENSE"), stage.join("LICENSE")).map_err(err("LICENSE"))?;
    let name = match arch {
        Arch::X86_64 => "whisper-linux.tar.gz",
        Arch::Aarch64 => "whisper-linux-arm64.tar.gz",
    };
    fsutil::tar_gz(&stage, "whisper-linux", &dist().join(name))?;
    println!("{}", dist().join(name).display());
    Ok(())
}

/// The /usr tree shared by the .deb and the .rpm.
fn usr_tree(stage: &Path, bin: &Path) -> Result<(), String> {
    let share = fsutil::share_dir()?;
    fsutil::fresh_dir(stage)?;
    let usr = stage.join("usr");
    fsutil::write(
        &usr.join("bin/whisper"),
        fs::read(bin).map_err(err(bin.display()))?,
    )?;
    fsutil::copy_dir(&share, &usr.join("share/whisper"))?;
    fsutil::write(
        &usr.join("share/applications/whisper.desktop"),
        fs::read(root().join("packaging/linux/whisper.desktop")).map_err(err("whisper.desktop"))?,
    )?;
    let icons = usr.join("share/icons/hicolor");
    for (dir, file, name) in [
        ("256x256/apps", "whisper-256.png", "whisper.png"),
        ("512x512/apps", "whisper-512.png", "whisper.png"),
        ("scalable/apps", "whisper.svg", "whisper.svg"),
    ] {
        fsutil::write(
            &icons.join(dir).join(name),
            fs::read(share.join("assets/icon").join(file)).map_err(err(file))?,
        )?;
    }
    let doc = usr.join("share/doc/whisper-browser");
    fsutil::write(
        &doc.join("copyright"),
        fs::read(root().join("packaging/linux/copyright")).map_err(err("copyright"))?,
    )?;
    fsutil::normalize_modes(stage)?;
    Ok(())
}

fn package_version(raw: &str) -> String {
    // Debian and RPM sort "~" before anything, so 0.2.0~rc1 comes before 0.2.0.
    raw.replace('-', "~")
}

/// whisper.deb (dpkg-deb).
pub fn deb(opts: &Opts) -> Result<(), String> {
    let arch = Arch::from_opts(opts)?;
    let bin = binary(opts)?;
    let version = fsutil::read_version()?;
    let stage = build().join(format!("deb-{}", arch.deb()));
    usr_tree(&stage, &bin)?;
    let control = fs::read_to_string(root().join("packaging/linux/control"))
        .map_err(err("packaging/linux/control"))?
        .replace("@VERSION@", &package_version(&version))
        .replace("@ARCH@", arch.deb())
        .replace("@SIZE@", &fsutil::size_kib(&stage)?.to_string());
    fsutil::write(&stage.join("DEBIAN/control"), control)?;
    fsutil::normalize_modes(&stage)?;
    fsutil::set_times(&stage)?;
    let name = match arch {
        Arch::X86_64 => "whisper.deb",
        Arch::Aarch64 => "whisper-arm64.deb",
    };
    let out = dist().join(name);
    run(Command::new("dpkg-deb")
        .args(["--root-owner-group", "-Zxz", "--build"])
        .arg(&stage)
        .arg(&out)
        .env("SOURCE_DATE_EPOCH", fsutil::epoch().to_string()))?;
    println!("{}", out.display());
    Ok(())
}

/// whisper.rpm (rpmbuild, with the spec in packaging/linux).
pub fn rpm(opts: &Opts) -> Result<(), String> {
    let arch = Arch::from_opts(opts)?;
    let bin = binary(opts)?;
    let version = fsutil::read_version()?;
    let work = build().join(format!("rpm-{}", arch.rpm()));
    let stage = work.join("root");
    usr_tree(&stage, &bin)?;
    fsutil::set_times(&stage)?;
    let out_dir = work.join("out");
    fsutil::fresh_dir(&out_dir)?;
    let define = |k: &str, v: &str| format!("{k} {v}");
    run(Command::new("rpmbuild")
        .arg("-bb")
        .args(["--target", arch.rpm()])
        .args([
            "--define",
            &define("_topdir", &work.join("top").to_string_lossy()),
        ])
        .args(["--define", &define("_rpmdir", &out_dir.to_string_lossy())])
        .args([
            "--define",
            &define("whisper_version", &package_version(&version)),
        ])
        .args([
            "--define",
            &define("whisper_root", &stage.to_string_lossy()),
        ])
        .args(["--define", "_binary_payload w9.xzdio"])
        .args(["--define", "use_source_date_epoch_as_buildtime 1"])
        .arg(root().join("packaging/linux/whisper.spec"))
        .env("SOURCE_DATE_EPOCH", fsutil::epoch().to_string()))?;
    let built = fsutil::walk(&out_dir)?
        .into_iter()
        .find(|f| f.ends_with(".rpm"))
        .ok_or("rpmbuild made no package")?;
    let name = match arch {
        Arch::X86_64 => "whisper.rpm",
        Arch::Aarch64 => "whisper-aarch64.rpm",
    };
    let out = dist().join(name);
    fs::copy(out_dir.join(built), &out).map_err(err(out.display()))?;
    println!("{}", out.display());
    Ok(())
}

/// Whisper-Setup.exe (NSIS), from the Windows whisper.exe.
pub fn nsis(opts: &Opts) -> Result<(), String> {
    let bin = binary(opts)?;
    let share = fsutil::share_dir()?;
    let version = fsutil::read_version()?;
    let stage = build().join("nsis");
    fsutil::fresh_dir(&stage)?;
    fs::copy(&bin, stage.join("whisper.exe")).map_err(err(bin.display()))?;
    fsutil::copy_dir(&share, &stage.join("share"))?;
    // The installer shows the license with Windows line endings
    let license = fs::read_to_string(root().join("LICENSE")).map_err(err("LICENSE"))?;
    fsutil::write(
        &stage.join("LICENSE.txt"),
        license.replace("\r\n", "\n").replace('\n', "\r\n"),
    )?;
    for (src, name) in [
        ("assets/icon/whisper.ico", "whisper.ico"),
        ("assets/installer/sidebar.bmp", "sidebar.bmp"),
        ("assets/installer/header.bmp", "header.bmp"),
    ] {
        fs::copy(root().join(src), stage.join(name)).map_err(err(src))?;
    }
    let core = version.split(['-', '+']).next().unwrap_or("0.0.0");
    let mut numbers: Vec<&str> = core.split('.').collect();
    numbers.resize(4, "0");
    let out = dist().join("Whisper-Setup.exe");
    run(Command::new("makensis")
        .arg("-V2")
        .arg("-INPUTCHARSET")
        .arg("UTF8")
        .arg(format!("-DVERSION={version}"))
        .arg(format!("-DVIVERSION={}", numbers.join(".")))
        .arg(format!("-DSTAGE={}", stage.display()))
        .arg(format!("-DOUTFILE={}", out.display()))
        .arg(format!("-DSIZE_KB={}", fsutil::size_kib(&stage)?))
        .arg(root().join("packaging/windows/whisper.nsi")))?;
    println!("{}", out.display());
    Ok(())
}

/// Whisper.dmg: Whisper.app (ad-hoc signed) and a link to /Applications.
pub fn dmg(opts: &Opts) -> Result<(), String> {
    let bin = binary(opts)?;
    let share = fsutil::share_dir()?;
    let version = fsutil::read_version()?;
    let stage = build().join("dmg");
    fsutil::fresh_dir(&stage)?;
    let app = stage.join("Whisper.app/Contents");
    fsutil::write(
        &app.join("MacOS/whisper"),
        fs::read(&bin).map_err(err(bin.display()))?,
    )?;
    fsutil::set_mode(&app.join("MacOS/whisper"), 0o755)?;
    fsutil::copy_dir(&share, &app.join("Resources/share"))?;
    fs::copy(
        root().join("assets/icon/whisper.icns"),
        app.join("Resources/whisper.icns"),
    )
    .map_err(err("whisper.icns"))?;
    let short = version
        .split(['-', '+'])
        .next()
        .unwrap_or("0.0.0")
        .to_string();
    let plist = fs::read_to_string(root().join("packaging/macos/Info.plist"))
        .map_err(err("Info.plist"))?
        .replace("@VERSION@", &short)
        .replace("@BUILD@", &version);
    fsutil::write(&app.join("Info.plist"), plist)?;
    fsutil::write(&app.join("PkgInfo"), "APPL????")?;
    let bundle = stage.join("Whisper.app");
    run(Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&bundle))?;
    run(Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&bundle))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("/Applications", stage.join("Applications"))
        .map_err(err("Applications link"))?;
    let out = dist().join("Whisper.dmg");
    let _ = fs::remove_file(&out);
    run(Command::new("hdiutil")
        .args([
            "create", "-volname", "Whisper", "-fs", "HFS+", "-format", "UDZO",
        ])
        .args(["-imagekey", "zlib-level=9", "-srcfolder"])
        .arg(&stage)
        .arg(&out))?;
    run(Command::new("hdiutil").arg("verify").arg(&out))?;
    println!("{}", out.display());
    Ok(())
}
