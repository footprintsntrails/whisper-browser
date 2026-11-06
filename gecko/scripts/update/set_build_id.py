#!/usr/bin/env python3
"""Gives an unpacked Whisper a new build ID, without building it again.

Usage: set_build_id.py APP_DIR NEW_BUILD_ID

The build ID (14 digits, the time the build started) is compiled into the program
(whisper, whisper-bin / whisper.exe), libxul (libxul.so / xul.dll) and the updater, and
written into application.ini, platform.ini and AppConstants.sys.mjs in omni.ja. Firefox
compares it to decide whether an update is newer (patches/0022), and it throws its
startup caches away when the build ID changes, so a package whose interface files were
replaced (gecko/scripts/repack-ui.py) must get a new one before it is published.

Every occurrence of the old build ID is replaced, as ASCII and as UTF-16 (Windows
version resources), in every file of APP_DIR; omni.ja files are rewritten entry by
entry. The new ID must be newer and have the same length, so no file changes size.
Fails if the build ID is missing from any of the files that must carry it.
"""

import configparser
import os
import re
import shutil
import sys
import tempfile
import zipfile


def replace_in_jar(path, old, new):
    """Rewrites a zip (omni.ja) with `old` replaced in every entry; returns the count."""
    count = 0
    with zipfile.ZipFile(path) as zin:
        entries = [(info, zin.read(info.filename)) for info in zin.infolist()]
    if not any(old in data for _, data in entries):
        return 0
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(path), suffix=".ja")
    os.close(fd)
    with zipfile.ZipFile(tmp, "w") as zout:
        for info, data in entries:
            if old in data:
                count += data.count(old)
                data = data.replace(old, new)
            zout.writestr(info, data, compress_type=info.compress_type)
    keys = ("filename", "compress_type", "flag_bits", "extra", "external_attr", "create_system", "date_time")
    with zipfile.ZipFile(tmp) as check:
        if [tuple(getattr(i, k) for k in keys) for i in check.infolist()] != \
                [tuple(getattr(i, k) for k in keys) for i, _ in entries]:
            raise SystemExit(f"{path}: the rewritten jar's entries differ from the original's")
    shutil.copymode(path, tmp)
    os.replace(tmp, path)
    return count


def set_build_id(app, new):
    ini = configparser.RawConfigParser()
    ini.optionxform = str
    ini.read(os.path.join(app, "application.ini"), encoding="utf-8")
    old = ini.get("App", "BuildID")
    if not re.fullmatch(r"\d{14}", new) or not re.fullmatch(r"\d{14}", old):
        raise SystemExit(f"build IDs must have 14 digits: {old} -> {new}")
    if new <= old:
        raise SystemExit(f"the new build ID {new} is not newer than {old}")
    forms = [(old.encode(), new.encode()), (old.encode("utf-16-le"), new.encode("utf-16-le"))]
    changed = {}
    for root, _, files in os.walk(app):
        for name in files:
            path = os.path.join(root, name)
            if os.path.islink(path):
                continue
            rel = os.path.relpath(path, app).replace(os.sep, "/")
            if name.endswith(".ja"):
                n = sum(replace_in_jar(path, o, w) for o, w in forms)
            else:
                with open(path, "rb") as f:
                    data = f.read()
                n = sum(data.count(o) for o, _ in forms)
                if n:
                    for o, w in forms:
                        data = data.replace(o, w)
                    with open(path, "r+b") as f:
                        f.write(data)
            if n:
                changed[rel] = n
    # Everyone who may run the program must be able to read it (an omni.ja readable by
    # its owner only stops Firefox with "The installation seems to be corrupt").
    unreadable = []
    for root, dirs, files in os.walk(app):
        for name in files:
            path = os.path.join(root, name)
            if not os.path.islink(path) and os.stat(path).st_mode & 0o444 != 0o444:
                unreadable.append(os.path.relpath(path, app))
    if unreadable:
        raise SystemExit(f"not readable by everyone: {unreadable[:10]}")
    windows = os.path.exists(os.path.join(app, "whisper.exe"))
    must = (["whisper.exe", "xul.dll", "updater.exe"] if windows else ["whisper", "libxul.so", "updater"]) + [
        "application.ini", "platform.ini", "omni.ja"]
    missing = [m for m in must if m not in changed]
    if missing:
        raise SystemExit(f"build ID {old} was not found in {missing}; changed: {changed}")
    for rel, n in sorted(changed.items()):
        print(f"  {rel}: {n}")
    print(f"build ID {old} -> {new} in {len(changed)} files")
    return old


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    set_build_id(sys.argv[1], sys.argv[2])


if __name__ == "__main__":
    main()
