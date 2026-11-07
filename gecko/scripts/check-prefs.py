#!/usr/bin/env python3
"""Checks Whisper's built-in prefs against their manifest and against Firefox.

Usage: check-prefs.py FIREFOX_SRC [--manifest gecko/prefs-manifest.json] [PREF_FILE...]
       check-prefs.py --write-manifest [PREF_FILE...]   (adds missing entries, "why" from
                                                          the comments above each pref)

For every pref(...) line of Whisper's default pref files (by default
browser/components/whisper/0-whisper.js and security/0-whisper-security.js):
  1. the manifest (gecko/prefs-manifest.json) gives a non-empty reason ("why"), and the
     manifest names no pref that is gone from the files;
  2. a Firefox pref (anything not under "whisper.") still exists in FIREFOX_SRC: it is
     declared in modules/libpref/init/StaticPrefList.yaml, set in one of Firefox's own
     pref files, or read by name somewhere in the code (also as a branch plus a name,
     as "doh-rollout" + "trr-selection.enabled"). A pref Mozilla removed would
     otherwise be set forever and do nothing, while its comment still says it protects.
     A pref the manifest marks as "gone" (with the Firefox version) is reported but does
     not fail, so a known leftover can wait for its file's owner; one marked gone that
     exists again fails.
Exits 1 and lists every problem.
"""

import json
import os
import re
import sys

REPO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
WHISPER = os.path.join(REPO, "gecko", "overlay", "browser", "components", "whisper")
DEFAULT_FILES = [os.path.join(WHISPER, "0-whisper.js"),
                 os.path.join(WHISPER, "security", "0-whisper-security.js")]
DEFAULT_MANIFEST = os.path.join(REPO, "gecko", "prefs-manifest.json")
PREF_LINE = re.compile(r'^\s*pref\("([^"]+)",\s*(.+?)(,\s*locked)?\);')
# Where Firefox declares, sets or reads prefs. Tests are left out: a pref only a test
# reads is not one the browser uses.
CODE_DIRS = ["browser", "toolkit", "modules", "netwerk", "dom", "security", "services", "widget",
             "xpcom", "layout", "gfx", "js", "image", "uriloader", "docshell", "caps", "extensions",
             "ipc", "media", "parser", "editor", "intl", "remote", "accessible", "storage",
             "devtools", "third_party/application-services"]
CODE_EXT = (".js", ".mjs", ".jsm", ".cpp", ".h", ".rs", ".yaml", ".py", ".inc", ".idl", ".json", ".toml")
QUOTED = re.compile(r'["\']([A-Za-z0-9_.\-@{}]+)["\']')


def read_prefs(path):
    prefs = []
    comment = []
    for n, line in enumerate(open(path, encoding="utf-8"), start=1):
        stripped = line.strip()
        if stripped.startswith("//"):
            text = stripped.lstrip("/").strip()
            if text.startswith("---"):
                comment = [text.strip("- ").strip()]
            else:
                comment.append(text)
            continue
        m = PREF_LINE.match(stripped)
        if m:
            prefs.append({"name": m.group(1), "value": m.group(2), "locked": bool(m.group(3)),
                          "file": os.path.relpath(path, REPO), "line": n, "comment": " ".join(comment).strip()})
        elif not stripped:
            comment = [comment[0]] if comment and not comment[0].endswith(".") else []
    return prefs


def split_candidates(name):
    """(branch, rest) pairs for a dotted name, where rest has a '.' or '-' in it (so a
    common word such as "enabled" alone never counts as evidence)."""
    parts = name.split(".")
    for i in range(1, len(parts) - 1):
        rest = ".".join(parts[i:])
        if "." in rest or "-" in rest:
            yield ".".join(parts[:i]), rest


def firefox_names(src, wanted):
    """The wanted pref names that Firefox 'knows': StaticPrefList, pref files, code."""
    known = set()
    pieces = {}
    for name in wanted:
        for branch, rest in split_candidates(name):
            pieces.setdefault(branch, set()).add(name)
            pieces.setdefault(branch + ".", set()).add(name)
            pieces.setdefault(rest, set()).add(name)
    found_pieces = set()
    spl = os.path.join(src, "modules", "libpref", "init", "StaticPrefList.yaml")
    for line in open(spl, encoding="utf-8"):
        if line.startswith("- name: "):
            name = line[len("- name: "):].strip()
            if name in wanted:
                known.add(name)
    for top in CODE_DIRS:
        root = os.path.join(src, top)
        for dirpath, dirnames, files in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in ("test", "tests", "testing", "node_modules")]
            for f in files:
                if not f.endswith(CODE_EXT):
                    continue
                try:
                    text = open(os.path.join(dirpath, f), encoding="utf-8", errors="ignore").read()
                except OSError:
                    continue
                for m in QUOTED.finditer(text):
                    word = m.group(1)
                    if word in wanted:
                        known.add(word)
                    if word in pieces:
                        found_pieces.add(word)
    for name in wanted - known:
        if any((branch in found_pieces or branch + "." in found_pieces) and rest in found_pieces
               for branch, rest in split_candidates(name)):
            known.add(name)
    return known


def main():
    args = sys.argv[1:]
    manifest_path = DEFAULT_MANIFEST
    if "--manifest" in args:
        i = args.index("--manifest")
        manifest_path = args[i + 1]
        del args[i:i + 2]
    write = "--write-manifest" in args
    if write:
        args.remove("--write-manifest")
        src = None
    else:
        if not args:
            print(__doc__)
            return 2
        src = args.pop(0)
    files = args or DEFAULT_FILES
    prefs = [p for f in files for p in read_prefs(f)]
    manifest = json.load(open(manifest_path)) if os.path.exists(manifest_path) else {"prefs": {}}

    if write:
        for p in prefs:
            entry = manifest["prefs"].setdefault(p["name"], {})
            entry.setdefault("why", p["comment"])
            entry["file"] = p["file"]
        manifest["prefs"] = dict(sorted(manifest["prefs"].items()))
        with open(manifest_path, "w", encoding="utf-8") as f:
            json.dump(manifest, f, indent=2, ensure_ascii=False)
            f.write("\n")
        print(f"{manifest_path}: {len(manifest['prefs'])} prefs")
        return 0

    problems = []
    names = {p["name"] for p in prefs}
    seen = {}
    for p in prefs:
        where = f"{p['file']}:{p['line']}"
        if p["name"] in seen:
            problems.append(f"{where}: {p['name']} is also set at {seen[p['name']]}")
        seen[p["name"]] = where
        why = manifest["prefs"].get(p["name"], {}).get("why", "").strip()
        if not why:
            problems.append(f"{where}: {p['name']} has no 'why' in {os.path.relpath(manifest_path, REPO)}")
    for name in manifest["prefs"]:
        if name not in names:
            problems.append(f"{os.path.relpath(manifest_path, REPO)}: {name} is not set in any pref file")
    firefox = {n for n in names if not n.startswith("whisper.")}
    known = firefox_names(src, firefox)
    warnings = []
    for p in prefs:
        gone = manifest["prefs"].get(p["name"], {}).get("gone")
        if p["name"] in firefox and p["name"] not in known:
            msg = (f"{p['file']}:{p['line']}: {p['name']} does not exist in {src} "
                   "(not in StaticPrefList.yaml, Firefox's pref files or its code)")
            (warnings if gone else problems).append(msg + (f"; known gone since {gone}" if gone else ""))
        elif gone:
            problems.append(f"{p['file']}:{p['line']}: {p['name']} is marked gone ({gone}) but exists")
    for line in warnings:
        print("warning: " + line)
    for line in problems:
        print(line)
    print(f"check-prefs: {len(prefs)} prefs ({len(firefox)} Firefox, {len(names) - len(firefox)} Whisper), "
          f"{len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
