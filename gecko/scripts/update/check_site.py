#!/usr/bin/env python3
"""Checks the update host after a deploy (pages.yml): installed copies of Whisper and
Whisper Canary can see the updates this run deployed.

Usage: check_site.py --site DIR [--base-url URL] [--timeout SECONDS] [--wait SECONDS]
  DIR         the site as this run deployed it (site.py --site DIR)
  --base-url  the host (default: UPDATE_BASE_URL in gecko/release.env)

For every update/1/<channel>/<build target>/update.xml in DIR it asks the host for the
same file until the host serves
  - this file: then the MAR it names is downloaded and must have the size and SHA-512
    that the file gives, as the browser checks them; or
  - an update.xml of a newer build (a higher buildID): a later deploy replaced this
    run's file. Every deploy carries both channels, built from what is published when
    its turn comes, and checks the files it deployed itself.
The host's cache (Cache-Control: max-age=600) can serve the file from before the deploy
for up to 10 minutes, so anything else (an HTTP error, no update.xml, an older build,
the same build with other contents) is asked for again until --timeout (13 minutes)
and then fails the check. The index page must be served as well, so a deploy without
update files (no release and no canary yet) is checked too. Exits 1 with the reasons.
"""

import argparse
import hashlib
import os
import re
import sys
import time
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from updatelib import read_env  # noqa: E402

USER_AGENT = "whisper-update-host-check"
INDEX_TITLE = b"<title>Whisper updates</title>"


def fetch(url, timeout=60):
    """(body, headers, None), or (None, headers, why not) if the host does not serve it."""
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.read(), r.headers, None
    except urllib.error.HTTPError as e:
        return None, e.headers, f"answers HTTP {e.code}"
    except (urllib.error.URLError, OSError) as e:
        return None, None, f"does not answer ({getattr(e, 'reason', e)})"


def parse(data):
    """Build ID, MAR URL, size and SHA-512 of an update.xml that offers one complete MAR
    (updatelib.update_xml), or None if `data` is not such a file."""
    try:
        root = ET.fromstring(data)
    except ET.ParseError:
        return None
    updates = root.findall("update") if root.tag == "updates" else []
    patches = updates[0].findall("patch") if len(updates) == 1 else []
    if len(patches) != 1:
        return None
    u, p = updates[0].attrib, patches[0].attrib
    if (not re.fullmatch(r"\d{14}", u.get("buildID", "")) or p.get("type") != "complete"
            or p.get("hashFunction") != "sha512" or not p.get("URL")
            or not p.get("size", "").isdigit() or not re.fullmatch(r"[0-9a-f]{128}", p.get("hashValue", ""))):
        return None
    return {"build": u["buildID"], "url": p["URL"], "size": int(p["size"]), "sha512": p["hashValue"]}


def judge(deployed, served):
    """Whether `served`, what the host answers for a file this run deployed as
    `deployed`, is acceptable: ("same", None), ("newer", its build) or (None, why not)."""
    if served == deployed:
        return "same", None
    mine, theirs = parse(deployed), parse(served)
    if theirs is None:
        return None, "serves something that is not an update.xml"
    # Build IDs are 14 digits (YYYYMMDDhhmmss), so they compare as numbers do.
    if theirs["build"] > mine["build"]:
        return "newer", theirs["build"]
    if theirs["build"] == mine["build"]:
        return None, f"serves build {theirs['build']}, but not the file this run deployed"
    return None, f"serves build {theirs['build']}, older than build {mine['build']} that this run deployed"


def judge_index(served):
    return ("same", None) if INDEX_TITLE in served else (None, "serves a page that is not the update host's index")


def check_mar(info, wait, attempts=3):
    """None if the MAR an update.xml names downloads with the size and SHA-512 it gives,
    else why not."""
    why = None
    for attempt in range(attempts):
        if attempt:
            time.sleep(wait)
        h = hashlib.sha512()
        size = 0
        req = urllib.request.Request(info["url"], headers={"User-Agent": USER_AGENT})
        try:
            with urllib.request.urlopen(req, timeout=300) as r:
                for chunk in iter(lambda: r.read(1 << 20), b""):
                    h.update(chunk)
                    size += len(chunk)
        except urllib.error.HTTPError as e:
            why = f"HTTP {e.code}"
            continue
        except (urllib.error.URLError, OSError) as e:
            why = f"no answer ({getattr(e, 'reason', e)})"
            continue
        if size != info["size"]:
            return f"{size} bytes, update.xml says {info['size']}"
        if h.hexdigest() != info["sha512"]:
            return "its SHA-512 is not the one in update.xml"
        return None
    return why


def took(start):
    s = time.monotonic() - start
    return f"{s / 60:.1f} minutes" if s >= 90 else f"{s:.0f} s"


def deployed_files(site):
    """{"update/1/<channel>/<target>/update.xml": contents} of the site this run deployed."""
    files = {}
    for dirpath, _, names in os.walk(os.path.join(site, "update")):
        if "update.xml" in names:
            path = os.path.join(dirpath, "update.xml")
            with open(path, "rb") as f:
                files[os.path.relpath(path, site).replace(os.sep, "/")] = f.read()
    return dict(sorted(files.items()))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--site", required=True)
    ap.add_argument("--base-url")
    ap.add_argument("--timeout", type=float, default=780, help="seconds (default 780)")
    ap.add_argument("--wait", type=float, default=10, help="seconds between attempts (default 10)")
    a = ap.parse_args()
    base = (a.base_url or read_env("release.env")["UPDATE_BASE_URL"]).rstrip("/")
    files = deployed_files(os.path.abspath(a.site))
    broken = [rel for rel, data in files.items() if parse(data) is None]
    problems = [f"{rel}: this run deployed something that is not an update.xml" for rel in broken]
    if not files:
        print("this run deployed no update files (no release and no canary yet): checking the index page only")

    # name -> (URL, judge of what the host serves)
    checks = {rel: (f"{base}/{rel}", lambda served, d=data: judge(d, served))
              for rel, data in files.items() if rel not in broken}
    checks["index page"] = (f"{base}/", judge_index)
    results, last = {}, {}
    start = time.monotonic()
    while True:
        for name, (url, ok) in checks.items():
            if name in results:
                continue
            body, headers, why = fetch(url)
            verdict = None
            if body is not None:
                verdict, why = ok(body)
            if verdict:
                results[name] = (verdict, why)
                if verdict == "newer":
                    print(f"{name}: a newer deploy replaced it (build {why}); that deploy checks it")
            elif last.get(name) != why:
                last[name] = why
                age = headers.get("Age") if headers is not None else None
                cached = f" (from the host's cache, {age} s old)" if age else ""
                print(f"{name}: the host {why}{cached}; asking again every {a.wait:g} s")
        if len(results) == len(checks) or time.monotonic() - start > a.timeout:
            break
        time.sleep(a.wait)
    for name, (url, _) in checks.items():
        if name not in results:
            problems.append(f"{url}: after {took(start)} the host still {last.get(name)}")

    mars = {}
    for name in checks:
        verdict, _ = results.get(name, (None, None))
        if verdict != "same":
            continue
        if name == "index page":
            print(f"index page: served at {base}/")
            continue
        info = parse(files[name])
        if info["url"] not in mars:
            mars[info["url"]] = check_mar(info, a.wait)
        if mars[info["url"]]:
            problems.append(f"{name}: the MAR it offers, {info['url']}: {mars[info['url']]}")
        else:
            print(f"{name}: served as deployed, build {info['build']}; "
                  f"its MAR downloads with the {info['size']} bytes and the SHA-512 it gives")

    for p in problems:
        print(f"::error::{p}" if os.environ.get("GITHUB_ACTIONS") else f"error: {p}")
    print(f"update host: {len(problems)} problem(s)" if problems else
          f"update host: every file this run deployed is served, or a newer deploy replaced it ({took(start)})")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
