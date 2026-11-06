#!/usr/bin/env python3
"""Starts an unpacked Whisper (or Whisper Canary) for real and checks that it runs:
non-headless (run it under Xvfb on a server), with a fresh profile, driven over
Marionette. It must open its window, report the expected build ID and update channel,
show the start page, and quit cleanly. Used on the exact files that are about to be
published, and on the published tarball downloaded back from the release.

Usage: start_check.py --app DIR [--build-id ID] [--channel NAME] [--out DIR]
  DIR  the folder with the `whisper` program (whisper.exe on Windows)
Exits 1 if the browser does not start or reports something else. Prints the end of
the browser's output on failure (for example Firefox's "The installation seems to be
corrupt").
"""

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "smoke"))
from marionette import Marionette, MarionetteError  # noqa: E402

WINDOWS = platform.system() == "Windows"

PREFS = """user_pref("remote.prefs.recommended", false);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("app.update.disabledForTesting", true);
user_pref("whisper.import.offerOnFirstRun", false);
user_pref("marionette.port", 2850);
"""

CHECK_JS = r"""
const { UpdateUtils } = ChromeUtils.importESModule("resource://gre/modules/UpdateUtils.sys.mjs");
const win = Services.wm.getMostRecentWindow("navigator:browser");
const tab = win.gBrowser.addTab("about:home", { triggeringPrincipal: Services.scriptSecurityManager.getSystemPrincipal() });
win.gBrowser.selectedTab = tab;
const deadline = Date.now() + 30000;
let url = "";
while (Date.now() < deadline) {
  url = tab.linkedBrowser.currentURI.spec;
  if (url == "about:home" && !tab.hasAttribute("busy")) {
    break;
  }
  await new Promise(r => setTimeout(r, 250));
}
return {
  buildID: Services.appinfo.appBuildID,
  version: Services.appinfo.version,
  channel: UpdateUtils.UpdateChannel,
  name: Services.appinfo.name,
  remotingName: Services.appinfo.remotingName,
  windowTitle: win.document.title,
  page: url,
  pageLoaded: url == "about:home" && !tab.hasAttribute("busy"),
};
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--app", required=True)
    ap.add_argument("--build-id")
    ap.add_argument("--channel")
    ap.add_argument("--out", default=None)
    a = ap.parse_args()
    out = a.out or tempfile.mkdtemp(prefix="whisper-start-")
    os.makedirs(out, exist_ok=True)
    profile = tempfile.mkdtemp(prefix="whisper-start-profile-")
    with open(os.path.join(profile, "user.js"), "w") as f:
        f.write(PREFS)
    exe = os.path.join(a.app, "whisper.exe" if WINDOWS else "whisper")
    logfile = os.path.join(out, "browser.log")
    env = dict(os.environ, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS="1")
    env.pop("MOZ_HEADLESS", None)
    log = open(logfile, "w")
    proc = subprocess.Popen([exe, "--marionette", "--no-remote", "--profile", profile],
                            stdout=log, stderr=subprocess.STDOUT, env=env)
    problems = []
    result = {}
    try:
        m = Marionette(port=2850, timeout=120, alive=(lambda: True) if WINDOWS else (lambda: proc.poll() is None))
        m.start_session()
        result = m.run(CHECK_JS)
        try:
            m.command("Marionette:Quit", {"flags": ["eAttemptQuit"]})
        except (MarionetteError, OSError):
            pass
        try:
            proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            problems.append("the browser did not quit within 60 s")
            proc.kill()
    except (MarionetteError, OSError) as e:
        problems.append(f"the browser did not start: {e}")
        if proc.poll() is None:
            proc.kill()
    log.close()
    text = open(logfile, errors="replace").read()
    if "installation seems to be corrupt" in text.lower():
        problems.append("the browser says the installation is corrupt")
    if result:
        if a.build_id and result.get("buildID") != a.build_id:
            problems.append(f"build ID {result.get('buildID')}, expected {a.build_id}")
        if a.channel and result.get("channel") != a.channel:
            problems.append(f"update channel {result.get('channel')}, expected {a.channel}")
        if not result.get("pageLoaded"):
            problems.append(f"the start page did not load ({result.get('page')})")
    if not WINDOWS and proc.returncode not in (0, None) and not problems:
        problems.append(f"the browser exited with {proc.returncode}")
    print(json.dumps({"app": a.app, "result": result, "exit": proc.returncode, "problems": problems}, indent=2))
    if problems:
        print("--- end of the browser's output:")
        print("".join(text.splitlines(keepends=True)[-30:]))
        sys.exit(1)
    print(f"OK: {result.get('name')} build {result.get('buildID')} ({result.get('channel')}) starts and shows its start page")


if __name__ == "__main__":
    main()
