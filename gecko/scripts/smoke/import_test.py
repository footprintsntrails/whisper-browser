#!/usr/bin/env python3
"""Import test: Whisper brings over bookmarks, history, passwords and form history from
a Firefox-family browser (gecko/overlay/browser/components/whisper/WhisperImport.sys.mjs).

Usage: import_test.py --app DIR --out DIR

DIR is an unpacked Linux package. With a home folder of its own, the test:
  1. makes a "Firefox" profile in ~/.mozilla/firefox (profiles.ini, Default=1) by
     running Whisper on it and adding a bookmark with a tag, a visit, a saved password,
     a form history entry and a cookie (the format is Firefox's), and copies it to
     ~/.zen as a Zen profile;
  2. starts Whisper with a new profile: the import window opens by itself (first run);
  3. imports from "Firefox" the way the import window does, and checks that the
     bookmark, tag, visit, password (decrypted with the source profile's own key4.db)
     and form history arrived and the cookie did not; then imports the cookie on its
     own (opt-in);
  4. holds the source profile's lock as a running Firefox would: the import is refused;
  5. finds Zen too, and no LibreWolf, Floorp or Waterfox.
Writes report.json and report.md to --out. Exits 1 if a check fails.
"""

import argparse
import fcntl
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from marionette import Marionette, MarionetteError  # noqa: E402

URL = "https://example.org/whisper-import-test"
VISITED = "https://example.org/whisper-import-visited"
PASSWORD = "s3cret-whisper-import"

PREFS = """user_pref("remote.prefs.recommended", false);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.dom.window.dump.enabled", true);
user_pref("devtools.console.stdout.chrome", true);
user_pref("whisper.import.offerOnFirstRun", {offer});
"""

FILL_JS = r"""
const { PlacesUtils } = ChromeUtils.importESModule("resource://gre/modules/PlacesUtils.sys.mjs");
const { FormHistory } = ChromeUtils.importESModule("resource://gre/modules/FormHistory.sys.mjs");
const [url, visited, password] = args;
// Each step says its name when it fails, so a failure names what broke.
const step = async (name, fn) => {
  try {
    return await fn();
  } catch (e) {
    throw new Error(`${name}: ${e}`);
  }
};
await step("bookmark", () =>
  PlacesUtils.bookmarks.insert({ parentGuid: PlacesUtils.bookmarks.toolbarGuid, url, title: "Whisper import test" }));
await step("tag", () => PlacesUtils.tagging.tagURI(Services.io.newURI(url), ["whisper-tag"]));
await step("history", () =>
  PlacesUtils.history.insert({ url: visited, title: "Visited page", visits: [{ date: new Date() }] }));
// As Firefox's own code makes logins.
const login = await step("login object", () => {
  const l = Cc["@mozilla.org/login-manager/loginInfo;1"].createInstance(Ci.nsILoginInfo);
  l.init("https://example.org", "https://example.org", null, "alice", password, "user", "pass");
  return l;
});
await step("save login", () => Services.logins.addLoginAsync(login));
await step("form history", () => FormHistory.update({ op: "add", fieldname: "q", value: "whisper import test" }));
await step("cookie", () =>
  Services.cookies.add("example.org", "/", "sid", "whisper", true, false, false, Date.now() + 86400000, {},
                       Ci.nsICookie.SAMESITE_LAX, Ci.nsICookie.SCHEME_HTTPS));
return { logins: (await step("count logins", () => Services.logins.getAllLogins())).length };
"""

WIZARD_JS = r"""
// The first-run offer: Firefox's import window, opened by Whisper on its own.
const sleep = ms => new Promise(res => setTimeout(res, ms));
const find = () => [...Services.wm.getEnumerator(null)].find(
  w => w.location.href.startsWith("chrome://browser/content/migration/migration-dialog-window.html"));
let win = null;
for (let i = 0; i < 60 && !(win = find()); i++) {
  await sleep(500);
}
const offered = Services.prefs.getBoolPref("whisper.import.offered", false);
if (win) {
  win.close();
}
return { opened: !!win, offered };
"""

IMPORT_JS = r"""
const { MigrationUtils } = ChromeUtils.importESModule("moz-src:///browser/components/migration/MigrationUtils.sys.mjs");
const { PlacesUtils } = ChromeUtils.importESModule("resource://gre/modules/PlacesUtils.sys.mjs");
const { FormHistory } = ChromeUtils.importESModule("resource://gre/modules/FormHistory.sys.mjs");
const [url, visited, password, types] = args;
const T = MigrationUtils.resourceTypes;
const r = { keys: MigrationUtils.availableMigratorKeys };
const m = await MigrationUtils.getMigrator("firefox");
r.found = !!m;
if (!m) {
  return r;
}
const profiles = await m.getSourceProfiles();
r.profiles = profiles.map(p => p.name);
r.offered = await m.getMigrateData(profiles[0]);
let items = 0;
for (const t of types) {
  items |= T[t];
}
try {
  await m.migrate(items, false, profiles[0]);
  r.migrated = true;
} catch (e) {
  r.migrated = false;
  r.error = String(e);
}
const bookmark = await PlacesUtils.bookmarks.fetch({ url });
r.bookmark = bookmark ? { title: bookmark.title, parent: bookmark.parentGuid } : null;
r.tags = PlacesUtils.tagging.getTagsForURI(Services.io.newURI(url));
r.visited = !!(await PlacesUtils.history.fetch(visited));
const logins = await Services.logins.searchLoginsAsync({ origin: "https://example.org" });
r.login = logins.map(l => ({ username: l.username, passwordMatches: l.password == password }));
r.formHistory = await FormHistory.count({ fieldname: "q", value: "whisper import test" });
r.cookies = Services.cookies.getCookiesFromHost("example.org", {}).map(c => c.name);
return r;
"""

OTHERS_JS = r"""
const { MigrationUtils } = ChromeUtils.importESModule("moz-src:///browser/components/migration/MigrationUtils.sys.mjs");
const r = {};
for (const key of ["zen", "librewolf", "floorp", "waterfox"]) {
  r[key] = !!(await MigrationUtils.getMigrator(key));
}
return r;
"""



class Report:
    def __init__(self):
        self.checks = []
        self.facts = {}

    def check(self, name, ok, detail=""):
        self.checks.append({"check": name, "ok": bool(ok), "detail": str(detail)})
        print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  ({detail})" if detail else ""), flush=True)


def run_browser(app, profile, home, out, offer, steps):
    with open(os.path.join(profile, "user.js"), "w") as f:
        f.write(PREFS.format(offer="true" if offer else "false"))
    env = dict(os.environ, HOME=home, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS="1")
    env.pop("XDG_CONFIG_HOME", None)
    log = open(os.path.join(out, "browser.log"), "a")
    proc = subprocess.Popen([os.path.join(app, "whisper"), "--marionette", "--no-remote", "--profile", profile],
                            stdout=log, stderr=subprocess.STDOUT, env=env)
    try:
        m = Marionette(alive=lambda: proc.poll() is None)
        m.start_session()
        result = steps(m)
        m.quit()
    finally:
        try:
            proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            proc.kill()
    return result


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    report = Report()
    home = tempfile.mkdtemp(prefix="whisper-import-home-")
    root = os.path.join(home, ".mozilla", "firefox")
    source = os.path.join(root, "src1.default-release")
    os.makedirs(source)
    with open(os.path.join(root, "profiles.ini"), "w") as f:
        f.write("[General]\nStartWithLastProfile=1\nVersion=2\n\n"
                "[Profile0]\nName=default-release\nIsRelative=1\nPath=src1.default-release\nDefault=1\n")
    try:
        filled = run_browser(a.app, source, home, a.out, False, lambda m: m.run(FILL_JS, URL, VISITED, PASSWORD))
        report.facts["source"] = filled
        report.check("source profile made (Firefox's format)", filled.get("logins") == 1, filled)
        zen_root = os.path.join(home, ".zen")
        # Without the profile's lock files (on Linux "lock" is a symlink to
        # "<address>:+<pid>", which points nowhere).
        shutil.copytree(source, os.path.join(zen_root, "zen1.Default"), symlinks=True,
                        ignore=shutil.ignore_patterns("lock", ".parentlock", "parent.lock"))
        with open(os.path.join(zen_root, "profiles.ini"), "w") as f:
            f.write("[Profile0]\nName=Default (release)\nIsRelative=1\nPath=zen1.Default\nDefault=1\n")

        target = tempfile.mkdtemp(prefix="whisper-import-target-")

        def steps(m):
            wizard = m.run(WIZARD_JS)
            report.facts["first_run"] = wizard
            report.check("first start of a new profile opens the import window", wizard["opened"]
                         and wizard["offered"], wizard)
            r = m.run(IMPORT_JS, URL, VISITED, PASSWORD, ["BOOKMARKS", "HISTORY", "PASSWORDS", "FORMDATA"])
            report.facts["import"] = r
            for key in ("firefox", "zen", "librewolf", "floorp", "waterfox"):
                report.check(f"import window knows {key}", key in r.get("keys", []), r.get("keys"))
            report.check("the Firefox profile is found", r.get("found") and r.get("profiles") == ["default-release"], r)
            report.check("import finished", r.get("migrated"), r.get("error"))
            report.check("bookmark arrived, in the toolbar", (r.get("bookmark") or {}).get("parent") == "toolbar_____",
                         r.get("bookmark"))
            report.check("its tag arrived", "whisper-tag" in (r.get("tags") or []), r.get("tags"))
            report.check("history arrived", r.get("visited"))
            report.check("password arrived, decrypted with the source profile's key",
                         r.get("login") == [{"username": "alice", "passwordMatches": True}], r.get("login"))
            report.check("form history arrived", r.get("formHistory") == 1, r.get("formHistory"))
            report.check("cookies did not come along unasked", r.get("cookies") == [], r.get("cookies"))
            c = m.run(IMPORT_JS, URL, VISITED, PASSWORD, ["COOKIES"])
            report.facts["cookies"] = c
            report.check("cookies arrive when picked", c.get("migrated") and c.get("cookies") == ["sid"], c)
            # A running Firefox holds an fcntl lock on .parentlock: hold it the same way.
            with open(os.path.join(source, ".parentlock"), "w") as lock:
                fcntl.lockf(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                refused = m.run(IMPORT_JS, URL, VISITED, PASSWORD, ["HISTORY"])
                # The "close it first" alert is window-modal: Marionette runs no script
                # while it is open, so it is read and accepted with WebDriver's own
                # alert commands, once it is there.
                # (Its text is filled in a moment after the dialog opens; wait for it.)
                alert = None
                for attempt in range(60):
                    try:
                        reply = m.command("WebDriver:GetAlertText")
                        alert = reply.get("value") if isinstance(reply, dict) else reply
                        if alert or attempt == 59:
                            m.command("WebDriver:AcceptAlert")
                            break
                    except MarionetteError as e:
                        if "no such alert" not in str(e):
                            raise
                    time.sleep(0.5)
            report.facts["locked"] = {"import": refused, "alert": alert}
            report.check("no import while the browser has the profile open",
                         refused.get("migrated") is False and "open" in (refused.get("error") or ""), refused)
            report.check("and the user is told to close it", alert and "Close it" in alert, alert)
            others = m.run(OTHERS_JS)
            report.facts["others"] = others
            report.check("Zen's profile is found too; no LibreWolf, Floorp or Waterfox here",
                         others == {"zen": True, "librewolf": False, "floorp": False, "waterfox": False}, others)

        run_browser(a.app, target, home, a.out, True, steps)
    except (MarionetteError, OSError, KeyError) as e:
        report.check("import test ran", False, repr(e))
    with open(os.path.join(a.out, "report.json"), "w") as f:
        json.dump({"checks": report.checks, "facts": report.facts}, f, indent=2, default=str)
    with open(os.path.join(a.out, "report.md"), "w") as f:
        f.write("| | Check | Detail |\n|---|---|---|\n")
        for c in report.checks:
            detail = c["detail"].replace("|", "\\|").replace("\n", " ")[:300]
            f.write(f"| {'✅' if c['ok'] else '❌'} | {c['check']} | {detail} |\n")
    failed = [c for c in report.checks if not c["ok"]]
    print(f"\n{len(report.checks) - len(failed)}/{len(report.checks)} checks passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
