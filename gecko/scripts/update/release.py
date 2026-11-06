#!/usr/bin/env python3
"""Puts a Whisper release together (.github/workflows/release.yml).

  assemble  copy the packages under their release names, check that each can update
            itself from Whisper only, and make the complete MARs (unsigned).
              release.py assemble --linux TAR.XZ --windows-zip ZIP --windows-installer EXE --dist DIR
              environment: MAR_TOOLS (see make-mar.sh)
  sign      sign the MARs (sign-mar.sh), check them, write update.xml for every build
            target, SHA256SUMS and the release notes.
              release.py sign --key KEY.pem --cert CERT.der --dist DIR --tag v0.1.0
              environment: MAR_TOOLS and NSS_LIBS (see sign-mar.sh)
The update host (GitHub Pages) is laid out by site.py from the update.xml files of the
newest release and of Whisper Canary (.github/workflows/pages.yml).

The release files (DIR):
  whisper-linux-x86_64.tar.xz   Linux, unpack and run (updates itself if the folder
                                is writable by the user)
  Whisper-Setup.exe             Windows installer (per user, no administrator rights)
  whisper-windows-x86_64.zip    Windows, unpack and run
  whisper-linux-x86_64.mar      complete updates, signed with Whisper's primary key
  whisper-windows-x86_64.mar
  update-linux-x86_64.xml       the update.xml files the update host serves
  update-windows-x86_64.xml
  SHA256SUMS
"""

import argparse
import configparser
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from updatelib import GECKO, check_mar, read_env, read_info, sha512, update_xml  # noqa: E402

REPO = os.path.dirname(GECKO)
CERTS = os.path.join(GECKO, "overlay", "toolkit", "mozapps", "update", "updater")

# Release name of each package, its MAR, and the build targets Firefox's update service
# puts into the update address (Services.appinfo.OS + "_" + XPCOM ABI, plus the CPU on
# Windows: an x64 Whisper on an ARM64 Windows asks for ...-msvc-aarch64).
PLATFORMS = {
    "linux-x86_64": {
        "package": "whisper-linux-x86_64.tar.xz",
        "mar": "whisper-linux-x86_64.mar",
        "xml": "update-linux-x86_64.xml",
        "targets": ["Linux_x86_64-gcc3"],
    },
    "windows-x86_64": {
        "package": "whisper-windows-x86_64.zip",
        "installer": "Whisper-Setup.exe",
        "mar": "whisper-windows-x86_64.mar",
        "xml": "update-windows-x86_64.xml",
        "targets": ["WINNT_x86_64-msvc-x64", "WINNT_x86_64-msvc-aarch64"],
    },
}


def log(*args):
    print(*args, flush=True)


def package_ini(package):
    """[App] Version and BuildID from the application.ini inside a package."""
    if package.endswith(".tar.xz"):
        with tarfile.open(package) as t:
            data = t.extractfile("whisper/application.ini").read()
    else:
        with zipfile.ZipFile(package) as z:
            data = z.read("whisper/application.ini")
    cp = configparser.RawConfigParser()
    cp.read_string(data.decode("utf-8"))
    return cp.get("App", "Version"), cp.get("App", "BuildID")


def assemble(a):
    dist = os.path.abspath(a.dist)
    os.makedirs(os.path.join(dist, "unsigned"))
    shutil.copy(a.linux, os.path.join(dist, PLATFORMS["linux-x86_64"]["package"]))
    shutil.copy(a.windows_zip, os.path.join(dist, PLATFORMS["windows-x86_64"]["package"]))
    shutil.copy(a.windows_installer, os.path.join(dist, PLATFORMS["windows-x86_64"]["installer"]))
    subprocess.run([sys.executable, os.path.join(HERE, "check_package.py"),
                    *(os.path.join(dist, p["package"]) for p in PLATFORMS.values())], check=True)
    for p in PLATFORMS.values():
        subprocess.run([os.path.join(HERE, "make-mar.sh"), os.path.join(dist, p["package"]),
                        os.path.join(dist, "unsigned", p["mar"])], check=True)


def sign(a):
    release = read_env("release.env")
    dist = os.path.abspath(a.dist)
    tag = a.tag
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        raise SystemExit(f"tag {tag} is not like v0.1.0")
    version = tag[1:]
    if not a.dry_run and version != release["WHISPER_VERSION"]:
        raise SystemExit(f"tag {tag} does not match WHISPER_VERSION={release['WHISPER_VERSION']} in gecko/release.env")
    if not a.dry_run:
        # A real release is signed with the key of the certificate the updater trusts.
        real = os.path.join(CERTS, "whisper_primary.der")
        if open(a.cert, "rb").read() != open(real, "rb").read():
            raise SystemExit("a release must be signed for whisper_primary.der")
        fp = ":".join(f"{b:02X}" for b in hashlib.sha256(open(real, "rb").read()).digest())
        if fp != release["MAR_PRIMARY_SHA256"]:
            raise SystemExit(f"whisper_primary.der is {fp}, release.env pins {release['MAR_PRIMARY_SHA256']}")
    pairs = []
    for p in PLATFORMS.values():
        pairs += [os.path.join(dist, "unsigned", p["mar"]), os.path.join(dist, p["mar"])]
    subprocess.run([os.path.join(HERE, "sign-mar.sh"), a.key, a.cert, *pairs], check=True)

    downloads = f"{release['RELEASE_DOWNLOAD_URL']}/{tag}"
    details = release["RELEASE_DOWNLOAD_URL"].replace("/releases/download", "/releases/tag") + f"/{tag}"
    for name, p in PLATFORMS.items():
        mar = os.path.join(dist, p["mar"])
        app_version, build_id = package_ini(os.path.join(dist, p["package"]))
        problems = check_mar(mar, app_version, build_id, release["MAR_CHANNEL_ID"], signatures=1)
        if problems:
            raise SystemExit(f"{p['mar']}: " + "; ".join(problems))
        info = read_info(mar)
        xml = update_xml(app_version, build_id, version, f"{downloads}/{p['mar']}", info["size"], sha512(mar),
                         details)
        with open(os.path.join(dist, p["xml"]), "w", encoding="utf-8") as f:
            f.write(xml)
        log(f"{name}: Whisper {version} = Firefox {app_version} build {build_id}; {p['mar']} {info['size']} bytes, "
            f"channel {info['channel']}, product version {info['product_version']}")
    shutil.rmtree(os.path.join(dist, "unsigned"))

    files = sorted(f for f in os.listdir(dist) if os.path.isfile(os.path.join(dist, f)) and f != "SHA256SUMS")
    with open(os.path.join(dist, "SHA256SUMS"), "w") as out:
        for f in files:
            h = hashlib.sha256()
            with open(os.path.join(dist, f), "rb") as fh:
                for chunk in iter(lambda: fh.read(1 << 20), b""):
                    h.update(chunk)
            out.write(f"{h.hexdigest()}  {f}\n")
    firefox = read_env("firefox.env")
    notes = open(os.path.join(REPO, ".github", "release-notes.md"), encoding="utf-8").read()
    for k, v in {"VERSION": version, "TAG": tag, "FIREFOX_VERSION": firefox["FIREFOX_VERSION"],
                 "MAR_PRIMARY_SHA256": release["MAR_PRIMARY_SHA256"],
                 "MAR_SECONDARY_SHA256": release["MAR_SECONDARY_SHA256"]}.items():
        notes = notes.replace("{{" + k + "}}", v)
    if "{{" in notes:
        raise SystemExit("release-notes.md has a placeholder left: " + notes[notes.index("{{"):][:40])
    with open(os.path.join(os.path.dirname(dist), "release-notes.md"), "w", encoding="utf-8") as f:
        f.write(notes)
    for f in files + ["SHA256SUMS"]:
        log(f"  {os.path.getsize(os.path.join(dist, f)):>11}  {f}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("assemble")
    p.add_argument("--linux", required=True)
    p.add_argument("--windows-zip", required=True)
    p.add_argument("--windows-installer", required=True)
    p.add_argument("--dist", required=True)
    p = sub.add_parser("sign")
    p.add_argument("--key", required=True)
    p.add_argument("--cert", required=True)
    p.add_argument("--dist", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--dry-run", action="store_true", help="a throwaway key; nothing is published")
    a = ap.parse_args()
    {"assemble": assemble, "sign": sign}[a.cmd](a)


if __name__ == "__main__":
    main()
