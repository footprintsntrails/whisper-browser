#!/usr/bin/env python3
"""Lays out Whisper's update host (GitHub Pages) from the update.xml files of the
newest release and of Whisper Canary, so every deploy carries both channels: a canary
publish never drops the release files, and a release never drops the canary's.

Usage: site.py --site OUT [--release DIR] [--canary DIR]
  --release DIR  update-linux-x86_64.xml and update-windows-x86_64.xml of the newest
                 release (release.py sign writes them; they are assets of the release)
  --canary DIR   update-canary-linux-x86_64.xml and update-canary-windows-x86_64.xml
                 (canary.py sign; assets of the rolling canary pre-release)

  -> OUT/update/1/<channel>/<build target>/update.xml, OUT/index.html
A channel whose folder is not given, or is empty, is left out (no release yet).
"""

import argparse
import html
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from release import PLATFORMS  # noqa: E402
from updatelib import read_env  # noqa: E402


def xml_name(platform, canary):
    return f"update-canary-{platform}.xml" if canary else PLATFORMS[platform]["xml"]


def lay_out(src, channel, canary, out, base_url):
    written = []
    present = [p for p in PLATFORMS if os.path.isfile(os.path.join(src, xml_name(p, canary)))]
    if present and len(present) != len(PLATFORMS):
        raise SystemExit(f"{src}: update files for {present} only; expected {list(PLATFORMS)}")
    for platform in present:
        text = open(os.path.join(src, xml_name(platform, canary)), encoding="utf-8").read()
        if not text.lstrip().startswith("<?xml") or "<updates>" not in text:
            raise SystemExit(f"{xml_name(platform, canary)} is not an update.xml")
        for target in PLATFORMS[platform]["targets"]:
            d = os.path.join(out, "update", "1", channel, target)
            os.makedirs(d, exist_ok=True)
            shutil.copy(os.path.join(src, xml_name(platform, canary)), os.path.join(d, "update.xml"))
            written.append(f"{base_url}/update/1/{channel}/{target}/update.xml")
    return written


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--site", required=True)
    ap.add_argument("--release")
    ap.add_argument("--canary")
    a = ap.parse_args()
    env = read_env("release.env")
    out = os.path.abspath(a.site)
    os.makedirs(out)
    written = []
    if a.release:
        written += lay_out(a.release, env["UPDATE_CHANNEL"], False, out, env["UPDATE_BASE_URL"])
    if a.canary:
        written += lay_out(a.canary, env["CANARY_UPDATE_CHANNEL"], True, out, env["UPDATE_BASE_URL"])
    items = "".join(f"<li><code>{html.escape(u)}</code></li>\n" for u in written)
    with open(os.path.join(out, "index.html"), "w", encoding="utf-8") as f:
        f.write('<!doctype html><meta charset="utf-8"><title>Whisper updates</title>\n'
                "<p>This is where installed copies of Whisper and Whisper Canary look for updates. "
                '<a href="https://github.com/footprintsntrails/whisper-browser">Whisper on GitHub</a></p>\n'
                f"<ul>\n{items}</ul>\n")
    for u in written:
        print(u)
    if not written:
        print("no update files: the site has only its index page")


if __name__ == "__main__":
    main()
