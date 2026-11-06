#!/usr/bin/env python3
"""Writes the update.xml that offers a complete MAR to installed copies of Whisper.

Firefox's update service reads this file from Whisper's update host (one file per
channel and build target, see gecko/release.env), compares appVersion and buildID with
its own (patches/0022-updates-in-build-order.patch: same version, newer build ID),
downloads the MAR from URL and checks its size and SHA-512 before the updater checks
its signature. Version and build ID come from the package's application.ini and must
match the MAR's own signed product information, so the file cannot describe another
build.

Usage:
  update-xml.py --mar FILE --app-ini application.ini --url URL --display-version 0.1.0
                [--details-url URL] [--channel whisper-release] [--signatures 1] [--out FILE]
"""

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from updatelib import app_ini, check_mar, read_info, sha512, update_xml  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mar", required=True)
    ap.add_argument("--app-ini", required=True)
    ap.add_argument("--url", required=True)
    ap.add_argument("--display-version", required=True)
    ap.add_argument("--details-url", default="")
    ap.add_argument("--channel", default=None, help="expected MAR channel")
    ap.add_argument("--signatures", type=int, default=None, help="expected number of signatures")
    ap.add_argument("--out", default="-")
    a = ap.parse_args()

    version, build_id = app_ini(a.app_ini)
    problems = check_mar(a.mar, version, build_id, a.channel, a.signatures)
    if problems:
        raise SystemExit(f"{a.mar}: " + "; ".join(problems))
    info = read_info(a.mar)
    xml = update_xml(version, build_id, a.display_version, a.url, info["size"], sha512(a.mar), a.details_url)
    if a.out == "-":
        sys.stdout.write(xml)
    else:
        with open(a.out, "w", encoding="utf-8") as f:
            f.write(xml)
    print(f"update.xml: Whisper {a.display_version} (Firefox {version}, build {build_id}), "
          f"{info['size']} bytes, MAR channel {info['channel']}, {len(info['signatures'])} signature(s)",
          file=sys.stderr)


if __name__ == "__main__":
    main()
