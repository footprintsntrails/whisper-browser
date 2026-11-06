#!/usr/bin/env python3
"""Development shortcut: put the current interface files into an already built Whisper.

Rebuilding the browser takes hours; Whisper's own interface is plain CSS, JavaScript,
Fluent and prefs that end up unchanged inside omni.ja. This script replaces those files
in an unpacked Linux package, so a change can be smoke-tested in minutes
(.github/workflows/gecko-ui-quick.yml). Release packages always come from a full build.

Usage: repack-ui.py APP_DIR      (the folder that holds the `whisper` program)
       repack-ui.py --check FILE... (exit 0 if a change to these repository files
                                     reaches the browser completely by repacking:
                                     each is one of the files below, or changes
                                     nothing that is built)

Whisper Canary uses the same repack for changes to the interface only: the files of the
current commit go into the last canary build, which then gets a new build ID
(gecko/scripts/update/set_build_id.py) and is published as an update
(.github/workflows/gecko-ui-quick.yml). Any other change to what is built waits for
the full build (gecko.yml).
"""

import os
import stat
import sys
import tempfile
import zipfile

REPO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
OVERLAY = os.path.join(REPO, "gecko", "overlay")
WHISPER = "browser/components/whisper"
BRANDING = "browser/branding/whisper"
CANARY_BRANDING = os.path.join(REPO, "gecko", "variants", "canary", "branding")

# (omni.ja relative to APP_DIR, path inside it, source relative to gecko/overlay)
# Whisper.manifest is not here: the packager merges it into the jar's chrome.manifest.
FILES = [
    ("browser/omni.ja", "chrome/browser/content/browser/whisper/cast.css", f"{WHISPER}/content/cast.css"),
    ("browser/omni.ja", "chrome/browser/content/browser/whisper/cast-pages.css", f"{WHISPER}/content/cast-pages.css"),
    ("browser/omni.ja", "defaults/preferences/0-whisper.js", f"{WHISPER}/0-whisper.js"),
    ("browser/omni.ja", "localization/en-US/whisper/whisper.ftl", f"{WHISPER}/locales/en-US/whisper.ftl"),
    ("browser/omni.ja", "chrome/browser/content/branding/aboutDialog.css", f"{BRANDING}/content/aboutDialog.css"),
    ("browser/omni.ja", "chrome/browser/content/branding/about.png", f"{BRANDING}/content/about.png"),
    ("omni.ja", "moz-src/browser/components/whisper/WhisperUI.sys.mjs", f"{WHISPER}/WhisperUI.sys.mjs"),
    ("omni.ja", "moz-src/browser/components/whisper/WhisperSidebar.sys.mjs", f"{WHISPER}/WhisperSidebar.sys.mjs"),
]


# What a full build is made from (the paths in gecko.yml's push filter). A change
# there, other than to a file in FILES or to NOT_BUILT, needs the full build.
BUILT = ("gecko/", "patches/", "profile/chrome/fonts/", "assets/icon/", "assets/installer/",
         ".github/workflows/gecko.yml", ".github/workflows/gecko-build.yml")
NOT_BUILT = ("gecko/scripts/smoke/", "gecko/scripts/update/", "gecko/scripts/repack-ui.py",
             "gecko/network-allowlist.txt")


def can_repack(changed):
    """The changed repository files that a repack cannot bring into a build."""
    sources = {f"gecko/overlay/{src}" for _, _, src in FILES}
    blocking = []
    for path in changed:
        if path in sources or not path.startswith(BUILT) or path.startswith(NOT_BUILT):
            continue
        blocking.append(path)
    return blocking


def repack(jar, replacements):
    """Rewrites `jar` with `replacements` (entry name -> file), keeping everything else
    of every entry as mach package wrote it: order, compression (omni.ja entries are
    stored, not deflated), flags and attributes, and the jar's own file mode. A
    replacement started with mkstemp's 0600 once, which made omni.ja unreadable for
    anyone but the user who repacked it: Firefox then stops with "The installation
    seems to be corrupt"."""
    mode = stat.S_IMODE(os.stat(jar).st_mode)
    with zipfile.ZipFile(jar) as zin:
        infos = zin.infolist()
        names = [i.filename for i in infos]
        missing = [p for p in replacements if p not in names]
        if missing:
            near = [n for n in names if "whisper" in n.lower() or any(os.path.basename(m) in n for m in missing)]
            raise SystemExit(f"{jar}: no entry {missing}; similar entries: {near[:20]}")
        fd, tmp = tempfile.mkstemp(dir=os.path.dirname(jar), suffix=".ja")
        os.close(fd)
        with zipfile.ZipFile(tmp, "w") as zout:
            for info in infos:
                data = zin.read(info.filename)
                if info.filename in replacements:
                    with open(replacements[info.filename], "rb") as f:
                        data = f.read()
                    if info.filename.endswith(".ftl"):
                        # As mach package does: Fluent comments (license header too) are
                        # left out of the package.
                        data = b"".join(line for line in data.splitlines(keepends=True) if not line.startswith(b"#"))
                    print(f"  {os.path.basename(jar)}: {info.filename}")
                zout.writestr(info, data, compress_type=info.compress_type)
    check_same_layout(jar, tmp)
    os.chmod(tmp, mode)
    os.replace(tmp, jar)


def check_same_layout(original, rewritten):
    """The rewritten jar has the same entries in the same order, each with the same
    compression, flags and attributes."""
    keys = ("filename", "compress_type", "flag_bits", "extra", "external_attr", "create_system", "date_time")
    with zipfile.ZipFile(original) as a, zipfile.ZipFile(rewritten) as b:
        if b.testzip() is not None:
            raise SystemExit(f"{rewritten}: a damaged entry")
        old = [tuple(getattr(i, k) for k in keys) for i in a.infolist()]
        new = [tuple(getattr(i, k) for k in keys) for i in b.infolist()]
    if old != new:
        bad = next(i for i, (x, y) in enumerate(zip(old, new)) if x != y) if len(old) == len(new) else None
        raise SystemExit(f"{rewritten}: entries differ from {original}"
                         + (f" at {old[bad][0]}: {old[bad]} != {new[bad]}" if bad is not None else ""))


def main():
    if sys.argv[1:2] == ["--check"]:
        blocking = can_repack([f for f in sys.argv[2:] if f])
        for f in blocking:
            print(f"needs a full build: {f}")
        sys.exit(1 if blocking else 0)
    app = sys.argv[1]
    # Whisper Canary's branding is the release one with gecko/variants/canary/branding
    # over it (prepare.sh); take its own version of a branding file where it has one.
    with open(os.path.join(app, "application.ini"), encoding="utf-8") as f:
        canary = "RemotingName=whisper-canary" in f.read().splitlines()
    by_jar = {}
    for jar, inner, src in FILES:
        path = os.path.join(OVERLAY, src)
        if canary and src.startswith(BRANDING + "/"):
            own = os.path.join(CANARY_BRANDING, src[len(BRANDING) + 1:])
            if os.path.isfile(own):
                path = own
        by_jar.setdefault(os.path.join(app, jar), {})[inner] = path
    for jar, replacements in by_jar.items():
        repack(jar, replacements)


if __name__ == "__main__":
    main()
