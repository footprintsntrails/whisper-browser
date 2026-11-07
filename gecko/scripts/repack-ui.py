#!/usr/bin/env python3
"""Development shortcut: put the current interface files into an already built Whisper.

Rebuilding the browser takes hours; Whisper's own interface is plain CSS, JavaScript,
Fluent and prefs that end up unchanged inside omni.ja. This script replaces those files
in an unpacked Linux package, so a change can be smoke-tested in minutes
(.github/workflows/gecko-ui-quick.yml). Release packages always come from a full build.

Usage: repack-ui.py APP_DIR      (the folder that holds the `whisper` program)
       repack-ui.py APP_DIR [--wasm FILE | --no-wasm]
                                    (the guard engine: by default built now with
                                     gecko/scripts/build-wasm.sh; --wasm takes a built
                                     one, --no-wasm keeps the package's own)
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
import subprocess
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


# Whisper's security features (gecko/overlay/.../whisper/security) are new files, not
# replacements: a build made before they existed lacks them, so the repack adds the
# missing entries (and WhisperSecurity.manifest's lines to the jar's components
# manifest) instead of refusing. Where each file goes follows security/jar.mn and
# security/moz.build.
SECURITY = f"{WHISPER}/security"
SECURITY_MANIFEST = f"{SECURITY}/WhisperSecurity.manifest"


def security_files():
    """(jar, entry, source relative to gecko/overlay) for every security file."""
    out = []
    root = os.path.join(OVERLAY, SECURITY)
    for dirpath, _, files in os.walk(root):
        for f in sorted(files):
            rel = os.path.relpath(os.path.join(dirpath, f), root)
            src = f"{SECURITY}/{rel}"
            if rel.endswith(".sys.mjs") and "/" not in rel:
                out.append(("omni.ja", f"moz-src/{SECURITY}/{rel}", src))
            elif rel.startswith("content/"):
                out.append(("browser/omni.ja", f"chrome/browser/content/browser/whisper/security/{rel[8:]}", src))
            elif rel.startswith("locales/en-US/"):
                out.append(("browser/omni.ja", f"localization/en-US/whisper/{os.path.basename(rel)}", src))
            elif rel == "0-whisper-security.js":
                out.append(("browser/omni.ja", "defaults/preferences/0-whisper-security.js", src))
            elif rel.startswith("settings/"):
                out.append(("browser/omni.ja", f"chrome/browser/content/browser/whisper/security/{rel}", src))
    return out


# What a full build is made from (the paths in gecko.yml's push filter). A change
# there, other than to a file in FILES or to NOT_BUILT, needs the full build.
BUILT = ("gecko/", "patches/", "profile/chrome/fonts/", "assets/icon/", "assets/installer/",
         ".github/workflows/gecko.yml", ".github/workflows/gecko-build.yml")
NOT_BUILT = ("gecko/scripts/smoke/", "gecko/scripts/update/", "gecko/scripts/repack-ui.py",
             "gecko/network-allowlist.txt")


def can_repack(changed):
    """The changed repository files that a repack cannot bring into a build."""
    sources = {f"gecko/overlay/{src}" for _, _, src in FILES + security_files()}
    sources.add(f"gecko/overlay/{SECURITY_MANIFEST}")
    # The guard engine is rebuilt by every repack.
    engine = ("crates/dl_guard/", "crates/proto_guard/", "crates/wcore_wasm/", "gecko/scripts/build-wasm.sh",
              "Cargo.lock")
    blocking = []
    for path in changed:
        if path in sources or path.startswith(engine) or not path.startswith(BUILT) or path.startswith(NOT_BUILT):
            continue
        blocking.append(path)
    return blocking


def repack(jar, replacements, additions=None, manifest_lines=None):
    """Rewrites `jar` with `replacements` (entry name -> file), keeping everything else
    of every entry as mach package wrote it: order, compression (omni.ja entries are
    stored, not deflated), flags and attributes, and the jar's own file mode. A
    replacement started with mkstemp's 0600 once, which made omni.ja unreadable for
    anyone but the user who repacked it: Firefox then stops with "The installation
    seems to be corrupt".

    `additions` (entry name -> file) replace an entry when the jar has it and are
    appended, stored like their neighbours, when it does not. `manifest_lines` are
    added to components/components.manifest if missing."""
    additions = additions or {}
    manifest_lines = manifest_lines or []
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
        everything = {**replacements, **additions}
        with zipfile.ZipFile(tmp, "w") as zout:
            for info in infos:
                data = zin.read(info.filename)
                if info.filename in everything:
                    data = read_source(info.filename, everything[info.filename])
                    print(f"  {os.path.basename(jar)}: {info.filename}")
                elif info.filename == "components/components.manifest" and manifest_lines:
                    have = data.decode().splitlines()
                    extra = [line for line in manifest_lines if line not in have]
                    if extra:
                        data = (data.decode().rstrip("\n") + "\n" + "\n".join(extra) + "\n").encode()
                        print(f"  {os.path.basename(jar)}: {info.filename} (+{len(extra)} lines)")
                zout.writestr(info, data, compress_type=info.compress_type)
            template = infos[-1]
            for name, src in additions.items():
                if name in names:
                    continue
                info = zipfile.ZipInfo(name, date_time=template.date_time)
                info.external_attr = template.external_attr
                info.create_system = template.create_system
                zout.writestr(info, read_source(name, src), compress_type=zipfile.ZIP_STORED)
                print(f"  {os.path.basename(jar)}: {name} (new)")
    check_same_layout(jar, tmp)
    os.chmod(tmp, mode)
    os.replace(tmp, jar)


def read_source(name, src):
    with open(src, "rb") as f:
        data = f.read()
    if name.endswith(".ftl"):
        # As mach package does: Fluent comments (license header too) are left out.
        data = b"".join(line for line in data.splitlines(keepends=True) if not line.startswith(b"#"))
    return data


def check_same_layout(original, rewritten):
    """The rewritten jar has the same entries in the same order, each with the same
    compression, flags and attributes; new entries may only follow them."""
    keys = ("filename", "compress_type", "flag_bits", "extra", "external_attr", "create_system", "date_time")
    with zipfile.ZipFile(original) as a, zipfile.ZipFile(rewritten) as b:
        if b.testzip() is not None:
            raise SystemExit(f"{rewritten}: a damaged entry")
        old = [tuple(getattr(i, k) for k in keys) for i in a.infolist()]
        new = [tuple(getattr(i, k) for k in keys) for i in b.infolist()][:len(old)]
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
    wasm = None
    if "--wasm" in sys.argv:
        wasm = sys.argv[sys.argv.index("--wasm") + 1]
    elif "--no-wasm" not in sys.argv:
        wasm = os.path.join(tempfile.mkdtemp(prefix="whisper-wasm-"), "wcore.wasm")
        subprocess.run([os.path.join(REPO, "gecko", "scripts", "build-wasm.sh"), wasm], check=True)
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
    additions = {}
    for jar, inner, src in security_files():
        additions.setdefault(os.path.join(app, jar), {})[inner] = os.path.join(OVERLAY, src)
    if wasm:
        additions.setdefault(os.path.join(app, "browser/omni.ja"), {})[
            "chrome/browser/content/browser/whisper/security/wcore.wasm"] = wasm
    lines = [line for line in open(os.path.join(OVERLAY, SECURITY_MANIFEST), encoding="utf-8").read().splitlines()
             if line.startswith("category ")]
    for jar in sorted(set(by_jar) | set(additions)):
        repack(jar, by_jar.get(jar, {}), additions.get(jar),
               lines if jar.endswith(os.path.join("browser", "omni.ja")) else None)


if __name__ == "__main__":
    main()
