#!/usr/bin/env python3
"""Publishes Whisper Canary: one rolling GitHub pre-release (tag CANARY_TAG in
gecko/release.env) that always holds the newest canary build, and the canary's
update.xml files, which pages.yml puts on the update host next to the release ones.

  prepare  the canary packages under their release names, checked, and complete MARs
           (unsigned, named with the build ID so an update.xml never points at a file
           that a later publish replaced):
             canary.py prepare --linux PKG|DIR --windows PKG|DIR [--installer EXE] --dist DIR
             environment: MAR_TOOLS (see make-mar.sh)
  sign     sign the MARs with the canary key (sign-mar.sh), check them, write
           update-canary-<platform>.xml, canary.json (what is published, from which
           commit), the release notes and SHA256SUMS:
             canary.py sign --key KEY.pem --cert CERT.der --dist DIR --commit SHA
                            --engine-commit SHA --kind full|quick --branch NAME [--dry-run]
             environment: MAR_TOOLS and NSS_LIBS (see sign-mar.sh)
  publish  put DIR on the rolling pre-release (made if missing): upload and replace the
           files, move the tag to COMMIT, keep the MARs of this and the previous build
           and delete older ones:
             canary.py publish --dist DIR --commit SHA      (gh, with GH_TOKEN)
  state    print canary.json of the published canary ({} if there is none):
             canary.py state

Release files of a canary (DIR):
  Whisper-Canary-Setup.exe                    Windows installer (full builds only; an
                                              interface update keeps the last one, which
                                              updates itself after installing)
  whisper-canary-windows-x86_64.zip
  whisper-canary-linux-x86_64.tar.xz
  whisper-canary-<platform>-<build ID>.mar    complete updates, signed with the canary key
  update-canary-linux-x86_64.xml, update-canary-windows-x86_64.xml
  canary.json, SHA256SUMS
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from updatelib import GECKO, app_ini, check_mar, read_env, read_info, sha512, update_xml  # noqa: E402

CERTS = os.path.join(GECKO, "overlay", "toolkit", "mozapps", "update", "updater")
REPO_URL = "https://github.com/footprintsntrails/whisper-browser"
TITLE = "Whisper Canary (unstable, for testing)"
PACKAGES = {"linux-x86_64": "whisper-canary-linux-x86_64.tar.xz", "windows-x86_64": "whisper-canary-windows-x86_64.zip"}
INSTALLER = "Whisper-Canary-Setup.exe"
MAR_RE = re.compile(r"^whisper-canary-(linux|windows)-x86_64-\d{14}\.mar$")


def log(*args):
    # stderr: `state` prints JSON on stdout for the workflows to read.
    print(*args, file=sys.stderr, flush=True)


def run(cmd, **kw):
    log("+", " ".join(cmd))
    return subprocess.run(cmd, check=True, **kw)


def fingerprint(der):
    return ":".join(f"{b:02X}" for b in hashlib.sha256(der).digest())


def pack(src, dest):
    """A package file from a package (copied) or an unpacked package folder (packed with
    the folder name `whisper`, like mach package)."""
    if os.path.isfile(src):
        shutil.copy(src, dest)
        return
    src = os.path.abspath(src)
    if not os.path.isfile(os.path.join(src, "application.ini")) or os.path.basename(src) != "whisper":
        raise SystemExit(f"{src} is not an unpacked package (a folder named whisper with application.ini)")
    if dest.endswith(".tar.xz"):
        run(["tar", "-cJf", dest, "-C", os.path.dirname(src), "whisper"], env=dict(os.environ, XZ_OPT="-T0 -6"))
    else:
        with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as z:
            for root, _, files in os.walk(src):
                for f in sorted(files):
                    path = os.path.join(root, f)
                    z.write(path, "whisper/" + os.path.relpath(path, src).replace(os.sep, "/"))


def package_ini(package):
    if package.endswith(".tar.xz"):
        with tarfile.open(package) as t:
            data = t.extractfile("whisper/application.ini").read()
    else:
        with zipfile.ZipFile(package) as z:
            data = z.read("whisper/application.ini")
    tmp = tempfile.NamedTemporaryFile("wb", suffix=".ini", delete=False)
    tmp.write(data)
    tmp.close()
    try:
        return app_ini(tmp.name)
    finally:
        os.unlink(tmp.name)


def prepare(a):
    env = read_env("release.env")
    dist = os.path.abspath(a.dist)
    os.makedirs(os.path.join(dist, "unsigned"))
    pack(a.linux, os.path.join(dist, PACKAGES["linux-x86_64"]))
    pack(a.windows, os.path.join(dist, PACKAGES["windows-x86_64"]))
    if a.installer:
        shutil.copy(a.installer, os.path.join(dist, INSTALLER))
    run([sys.executable, os.path.join(HERE, "check_package.py"), "--variant", "canary",
         *(os.path.join(dist, p) for p in PACKAGES.values())])
    for platform, package in PACKAGES.items():
        _, build = package_ini(os.path.join(dist, package))
        mar = f"whisper-canary-{platform}-{build}.mar"
        run([os.path.join(HERE, "make-mar.sh"), os.path.join(dist, package), os.path.join(dist, "unsigned", mar),
             env["CANARY_MAR_CHANNEL_ID"]])


def sign(a):
    env = read_env("release.env")
    dist = os.path.abspath(a.dist)
    if not a.dry_run:
        real = open(os.path.join(CERTS, "whisper_canary.der"), "rb").read()
        if open(a.cert, "rb").read() != real:
            raise SystemExit("a canary must be signed for whisper_canary.der")
        if fingerprint(real) != env["MAR_CANARY_SHA256"]:
            raise SystemExit(f"whisper_canary.der is {fingerprint(real)}, release.env pins {env['MAR_CANARY_SHA256']}")
    unsigned = os.path.join(dist, "unsigned")
    mars = sorted(os.listdir(unsigned))
    pairs = []
    for m in mars:
        pairs += [os.path.join(unsigned, m), os.path.join(dist, m)]
    run([os.path.join(HERE, "sign-mar.sh"), a.key, a.cert, *pairs])
    shutil.rmtree(unsigned)

    downloads = f"{env['RELEASE_DOWNLOAD_URL']}/{env['CANARY_TAG']}"
    details = f"{REPO_URL}/releases/tag/{env['CANARY_TAG']}"
    builds = {}
    for platform, package in PACKAGES.items():
        version, build = package_ini(os.path.join(dist, package))
        mar = f"whisper-canary-{platform}-{build}.mar"
        path = os.path.join(dist, mar)
        problems = check_mar(path, version, build, env["CANARY_MAR_CHANNEL_ID"], signatures=1)
        if problems:
            raise SystemExit(f"{mar}: " + "; ".join(problems))
        info = read_info(path)
        with open(os.path.join(dist, f"update-canary-{platform}.xml"), "w", encoding="utf-8") as f:
            f.write(update_xml(version, build, env["WHISPER_VERSION"], f"{downloads}/{mar}", info["size"],
                               sha512(path), details))
        builds[platform] = {"build_id": build, "firefox": version, "mar": mar, "mar_size": info["size"]}
        log(f"{platform}: Whisper Canary {env['WHISPER_VERSION']}, Firefox {version}, build {build}; {mar} "
            f"{info['size']} bytes, channel {info['channel']}")
    ids = {b["build_id"] for b in builds.values()}
    state = {
        "commit": a.commit, "engine_commit": a.engine_commit, "kind": a.kind, "branch": a.branch,
        "whisper_version": env["WHISPER_VERSION"], "builds": builds, "newest_build_id": max(ids),
        "installer": os.path.isfile(os.path.join(dist, INSTALLER)),
        "published_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "run_url": a.run_url or "", "dry_run": bool(a.dry_run),
    }
    with open(os.path.join(dist, "canary.json"), "w") as f:
        json.dump(state, f, indent=2)
        f.write("\n")
    files = sorted(f for f in os.listdir(dist) if os.path.isfile(os.path.join(dist, f)) and f != "SHA256SUMS")
    with open(os.path.join(dist, "SHA256SUMS"), "w") as out:
        for f in files:
            h = hashlib.sha256()
            with open(os.path.join(dist, f), "rb") as fh:
                for chunk in iter(lambda: fh.read(1 << 20), b""):
                    h.update(chunk)
            out.write(f"{h.hexdigest()}  {f}\n")
    with open(os.path.join(os.path.dirname(dist), "canary-notes.md"), "w", encoding="utf-8") as f:
        f.write(notes(env, state))
    for f in files + ["SHA256SUMS"]:
        log(f"  {os.path.getsize(os.path.join(dist, f)):>11}  {f}")


def notes(env, state):
    tag = env["CANARY_TAG"]
    dl = f"{REPO_URL}/releases/download/{tag}"
    kind = ("a full build" if state["kind"] == "full" else
            f"an interface update: this commit's interface files in the build of {state['engine_commit'][:10]}")
    return f"""**{TITLE}.** The test edition of Whisper, rebuilt from every green build of `{state['branch']}` \
and installed next to Whisper. It can break at any time; use Whisper for anything that matters.

Now: build **{state['newest_build_id']}**, {kind}, commit {state['commit'][:10]}, published {state['published_at']}.

### Install

| System | Download | Notes |
|---|---|---|
| Windows 10/11 (x64) | [{INSTALLER}]({dl}/{INSTALLER}) | Installs for you only, next to Whisper: %LOCALAPPDATA%\\Programs\\Whisper Canary, its own profile, Start menu entry and yellow icon. Not code-signed: SmartScreen shows "Windows protected your PC"; choose **More info → Run anyway**. Also as a [zip]({dl}/{PACKAGES['windows-x86_64']}). |
| Linux (x86-64) | [{PACKAGES['linux-x86_64']}]({dl}/{PACKAGES['linux-x86_64']}) | Unpack and run `whisper/install.sh`: installs into ~/.local/share/whisper-canary with its own menu entry and profile. |

### Updates

Whisper Canary looks for a new build every 5 minutes while it runs, downloads it in the \
background and shows **Restart to update Whisper Canary** in the menu. Its updates are \
signed with the canary key (certificate SHA-256 `{env['MAR_CANARY_SHA256']}`): Whisper \
does not accept them, and Whisper Canary does not accept Whisper's. The installer may be \
older than the newest build; the installed canary updates itself right after.

`canary.json` says which commit this is; every file is listed in `SHA256SUMS`.
"""


def gh(*args, capture=False):
    cmd = ["gh", *args]
    log("+", " ".join(cmd))
    r = subprocess.run(cmd, check=not capture, capture_output=capture, text=True)
    return r


REPO = os.environ.get("GITHUB_REPOSITORY", "footprintsntrails/whisper-browser")


def current_state(tag):
    tmp = tempfile.mkdtemp()
    try:
        r = gh("release", "download", tag, "--repo", REPO, "--pattern", "canary.json", "--dir", tmp, capture=True)
        if r.returncode != 0:
            return {}
        return json.load(open(os.path.join(tmp, "canary.json")))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def state(a):
    env = read_env("release.env")
    print(json.dumps(current_state(env["CANARY_TAG"]), indent=2))


def publish(a):
    env = read_env("release.env")
    tag = env["CANARY_TAG"]
    dist = os.path.abspath(a.dist)
    new = json.load(open(os.path.join(dist, "canary.json")))
    if new.get("dry_run"):
        raise SystemExit("this canary was signed with a throwaway key (--dry-run); not publishing it")
    repo = REPO
    old = current_state(tag)
    if old and old.get("newest_build_id", "") >= new["newest_build_id"]:
        raise SystemExit(f"the published canary has build {old['newest_build_id']}, not older than "
                         f"{new['newest_build_id']}; not publishing")
    files = [os.path.join(dist, f) for f in sorted(os.listdir(dist))]
    notes_file = os.path.join(os.path.dirname(dist), "canary-notes.md")
    exists = gh("release", "view", tag, "--repo", repo, "--json", "tagName", capture=True).returncode == 0
    if not exists:
        gh("release", "create", tag, *files, "--repo", repo, "--prerelease", "--latest=false",
           "--target", a.commit, "--title", TITLE, "--notes-file", notes_file)
    else:
        gh("release", "upload", tag, *files, "--repo", repo, "--clobber")
        gh("api", "-X", "PATCH", f"repos/{repo}/git/refs/tags/{tag}", "-f", f"sha={a.commit}", "-F", "force=true")
        gh("release", "edit", tag, "--repo", repo, "--prerelease", "--latest=false", "--title", TITLE,
           "--notes-file", notes_file)
        # Keep the MARs of this build and the previous one (an installed canary may still
        # be downloading that one); delete older ones.
        keep = {b["mar"] for b in new["builds"].values()} | {b["mar"] for b in old.get("builds", {}).values()}
        names = gh("release", "view", tag, "--repo", repo, "--json", "assets", "-q", ".assets[].name",
                   capture=True).stdout.split()
        for name in names:
            if MAR_RE.match(name) and name not in keep:
                gh("release", "delete-asset", tag, name, "--repo", repo, "--yes")
    log(f"published Whisper Canary build {new['newest_build_id']} ({new['kind']}, commit {a.commit[:10]})")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("--linux", required=True)
    p.add_argument("--windows", required=True)
    p.add_argument("--installer")
    p.add_argument("--dist", required=True)
    p = sub.add_parser("sign")
    p.add_argument("--key", required=True)
    p.add_argument("--cert", required=True)
    p.add_argument("--dist", required=True)
    p.add_argument("--commit", required=True)
    p.add_argument("--engine-commit", required=True)
    p.add_argument("--kind", choices=["full", "quick"], required=True)
    p.add_argument("--branch", required=True)
    p.add_argument("--run-url")
    p.add_argument("--dry-run", action="store_true", help="a throwaway key; publish refuses it")
    p = sub.add_parser("publish")
    p.add_argument("--dist", required=True)
    p.add_argument("--commit", required=True)
    sub.add_parser("state")
    a = ap.parse_args()
    {"prepare": prepare, "sign": sign, "publish": publish, "state": state}[a.cmd](a)


if __name__ == "__main__":
    main()
