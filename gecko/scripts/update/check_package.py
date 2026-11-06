#!/usr/bin/env python3
"""Checks that a Whisper package can update itself, and only from Whisper.

Usage: check_package.py [--variant release|canary] PACKAGE [PACKAGE ...]
  PACKAGE  whisper-*.tar.xz (Linux) or whisper-*.zip (Windows)
  VARIANT  release (Whisper, the default) or canary (Whisper Canary)

For each package:
  - the updater is there, with update-settings.ini accepting only Whisper's MAR
    channel, and precomplete and removed-files (needed for complete updates);
  - application.ini has Firefox's version, a 14-digit build ID and Whisper's update
    address; channel-prefs.js sets the update channel;
  - the updater holds Whisper's two MAR certificates (gecko/overlay/.../whisper_*.der,
    pinned in gecko/release.env) byte for byte, once each, and not the canary one; a
    canary updater holds only the canary certificate (in both places, or once if the
    linker merged the two identical copies): these are the
    certificates it verifies MAR signatures with (patches/0020);
  - Whisper Canary has its own profile folder and program name (application.ini);
  - (Linux) every file is readable by everyone;
  - Whisper's own version (whisper.version) is built in;
  - Windows: no maintenance service.
Exits 1 if anything is wrong.
"""

import configparser
import hashlib
import os
import re
import shutil
import sys
import tarfile
import tempfile
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from updatelib import GECKO, read_env  # noqa: E402

CERTS = os.path.join(GECKO, "overlay", "toolkit", "mozapps", "update", "updater")


def fingerprint(der):
    return ":".join(f"{b:02X}" for b in hashlib.sha256(der).digest())


def unpack(package, dest):
    if package.endswith(".tar.xz"):
        with tarfile.open(package) as t:
            t.extractall(dest, filter="data")
    elif package.endswith(".zip"):
        with zipfile.ZipFile(package) as z:
            z.extractall(dest)
    else:
        raise SystemExit(f"{package}: not a .tar.xz or .zip")
    tops = os.listdir(dest)
    if len(tops) != 1:
        raise SystemExit(f"{package}: expected one top-level folder, found {tops}")
    return os.path.join(dest, tops[0])


# What each edition must say: update channel, MAR channel, which certificates the
# updater must hold how many times, and application.ini's Profile and RemotingName.
def variant_spec(release, variant):
    if variant == "canary":
        return {"channel": release["CANARY_UPDATE_CHANNEL"], "mar_channel": release["CANARY_MAR_CHANNEL_ID"],
                # Both places hold the same certificate; the linker may keep one copy.
                "certs": {"canary": (1, 2), "primary": (0,), "secondary": (0,)}, "profile": "Whisper Canary",
                "remoting": "whisper-canary"}
    return {"channel": release["UPDATE_CHANNEL"], "mar_channel": release["MAR_CHANNEL_ID"],
            "certs": {"primary": (1,), "secondary": (1,), "canary": (0,)}, "profile": "", "remoting": "whisper"}


def check(package, release, firefox, certs, spec):
    results = []

    def ok(name, good, detail=""):
        results.append((name, bool(good), str(detail)))

    if package.endswith(".tar.xz"):
        # Everyone who may run it must be able to read every file and open every folder
        # (an omni.ja readable by its owner only stops Firefox with "The installation
        # seems to be corrupt").
        with tarfile.open(package) as t:
            bad = [m.name for m in t.getmembers()
                   if (m.isfile() and m.mode & 0o444 != 0o444) or (m.isdir() and m.mode & 0o555 != 0o555)]
        ok("every file is readable by everyone", not bad, bad[:10])
    tmp = tempfile.mkdtemp(prefix="whisper-pkg-")
    try:
        app = unpack(package, tmp)
        windows = os.path.exists(os.path.join(app, "whisper.exe"))
        updater = os.path.join(app, "updater.exe" if windows else "updater")
        ok("updater is in the package", os.path.isfile(updater), os.path.basename(updater))
        settings = configparser.RawConfigParser()
        settings.read(os.path.join(app, "update-settings.ini"), encoding="utf-8")
        accepted = settings.get("Settings", "ACCEPTED_MAR_CHANNEL_IDS", fallback=None)
        ok(f"update-settings.ini accepts only the MAR channel {spec['mar_channel']}",
           accepted == spec["mar_channel"], accepted)
        for f in ("precomplete", "removed-files"):
            ok(f"{f} is in the package", os.path.isfile(os.path.join(app, f)))

        ini = configparser.RawConfigParser()
        ini.read(os.path.join(app, "application.ini"), encoding="utf-8")
        version = ini.get("App", "Version", fallback="")
        build_id = ini.get("App", "BuildID", fallback="")
        url = ini.get("AppUpdate", "URL", fallback="")
        profile = ini.get("App", "Profile", fallback="")
        remoting = ini.get("App", "RemotingName", fallback="")
        ok(f"profile folder is {spec['profile'] or 'the default (vendor/name)'}", profile == spec["profile"], profile)
        ok(f"program name (remoting, WM_CLASS) is {spec['remoting']}", remoting == spec["remoting"], remoting)
        ok(f"application version is Firefox's ({firefox['FIREFOX_VERSION']})",
           version == firefox["FIREFOX_VERSION"], version)
        ok("build ID has 14 digits", re.fullmatch(r"\d{14}", build_id), build_id)
        # The build ID update.xml and the MAR take from application.ini must be the one
        # the program reports (compiled into it and libxul) and the updater compares.
        platform = configparser.RawConfigParser()
        platform.read(os.path.join(app, "platform.ini"), encoding="utf-8")
        ok("platform.ini has the same build ID", platform.get("Build", "BuildID", fallback="") == build_id,
           platform.get("Build", "BuildID", fallback=""))
        for name in (["whisper.exe", "xul.dll", "updater.exe"] if windows else ["whisper", "libxul.so", "updater"]):
            path = os.path.join(app, name)
            found = os.path.isfile(path) and build_id.encode() in open(path, "rb").read()
            ok(f"{name} was built with this build ID", found)
        want_url = release["UPDATE_BASE_URL"] + "/update/1/%CHANNEL%/%BUILD_TARGET%/update.xml"
        ok("update address is Whisper's", url == want_url, url)
        channel_prefs = os.path.join(app, "defaults", "pref", "channel-prefs.js")
        text = open(channel_prefs, encoding="utf-8").read() if os.path.isfile(channel_prefs) else ""
        ok(f"update channel is {spec['channel']}",
           f'pref("app.update.channel", "{spec["channel"]}");' in text, text.strip()[-80:])

        if os.path.isfile(updater):
            binary = open(updater, "rb").read()
            for name, der in certs.items():
                want = spec["certs"][name]
                ok(f"updater holds the {name} certificate ({fingerprint(der)[:23]}...) "
                   f"{' or '.join(map(str, want))} times", binary.count(der) in want, f"found {binary.count(der)} times")

        omni = os.path.join(app, "browser", "omni.ja")
        version_pref = None
        if os.path.isfile(omni):
            with zipfile.ZipFile(omni) as z:
                for name in z.namelist():
                    if name.startswith("defaults/preferences/") and name.endswith(".js"):
                        m = re.search(r'pref\("whisper\.version", "([^"]*)", locked\);', z.read(name).decode())
                        if m:
                            version_pref = m.group(1)
        ok(f"Whisper's version is built in ({release['WHISPER_VERSION']})",
           version_pref == release["WHISPER_VERSION"], version_pref)
        if windows:
            service = [f for f in os.listdir(app) if f.lower().startswith("maintenanceservice")]
            ok("no maintenance service", not service, service)
        else:
            script = os.path.join(app, "install.sh")
            ok("install.sh is in the package and executable",
               os.path.isfile(script) and os.access(script, os.X_OK))
        return {"package": os.path.basename(package), "version": version, "build_id": build_id,
                "checks": results}
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main():
    args = sys.argv[1:]
    variant = "release"
    if args[:1] == ["--variant"] and len(args) > 1:
        variant, args = args[1], args[2:]
    if variant not in ("release", "canary") or not args:
        raise SystemExit(__doc__)
    release = read_env("release.env")
    firefox = read_env("firefox.env")
    spec = variant_spec(release, variant)
    certs = {}
    for which in ("primary", "secondary", "canary"):
        der = open(os.path.join(CERTS, f"whisper_{which}.der"), "rb").read()
        pinned = release[f"MAR_{which.upper()}_SHA256"]
        if fingerprint(der) != pinned:
            raise SystemExit(f"whisper_{which}.der is {fingerprint(der)}, release.env pins {pinned}")
        certs[which] = der
    failed = 0
    for package in args:
        r = check(package, release, firefox, certs, spec)
        print(f"{r['package']} ({variant}): Firefox {r['version']}, build {r['build_id']}")
        for name, good, detail in r["checks"]:
            print(f"  {'PASS' if good else 'FAIL'}  {name}" + (f"  ({detail})" if detail and not good else ""))
            failed += not good
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
