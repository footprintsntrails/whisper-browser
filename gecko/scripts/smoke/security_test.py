#!/usr/bin/env python3
"""Whisper security smoke test: each security feature blocks what it should and lets a
legitimate case through, on local pages only (a server on 127.0.0.1, no network).

Usage: security_test.py --app DIR --out DIR [--only NAME[,NAME...]]

DIR is an unpacked Linux package (the folder holding the `whisper` program). The
browser runs under the X display in $DISPLAY (xvfb-run in CI) with a fresh profile and
is driven over Marionette. Checks:

  prefs        every pref of security/0-whisper-security.js is built in, with its value
               and lock
  environment  SSLKEYLOGFILE, MOZ_LOG/MOZ_LOG_FILE and MOZ_PROFILER_SHUTDOWN make the
               browser write nothing (patches/0061)
  seed         a recovery phrase pasted, typed or typed one word per field never
               reaches the page; normal text does; holding the button allows one field
  address      a pasted address that differs from the copied one is stopped and both
               are shown; pasting the copied address itself passes

Writes OUT/security-report.{json,md} and screenshots; exits 1 if a check fails.
"""

import argparse
import http.server
import json
import os
import re
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from marionette import Marionette, MarionetteError  # noqa: E402

REPO = os.path.normpath(os.path.join(HERE, "..", "..", ".."))
SECURITY_PREFS = os.path.join(REPO, "gecko", "overlay", "browser", "components", "whisper",
                              "security", "0-whisper-security.js")
PAGES = os.path.join(HERE, "pages", "security")

PHRASE = "legal winner thank year wave sausage worth useful legal winner thank yellow"
PHRASE_WORDS = PHRASE.split()
COPIED = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"
# A different, valid address of the same chain, as clipboard malware would put there.
SWAPPED = "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3"


class Report:
    def __init__(self):
        self.checks = []
        self.facts = {}

    def check(self, name, ok, detail=""):
        self.checks.append({"check": name, "ok": bool(ok), "detail": str(detail)[:2000]})
        print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  ({str(detail)[:300]})" if detail else ""), flush=True)

    @property
    def failed(self):
        return [c for c in self.checks if not c["ok"]]


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


def start_server(routes):
    """Serves PAGES plus `routes` (path -> (status, headers, body)) on 127.0.0.1."""

    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *a, **kw):
            super().__init__(*a, directory=PAGES, **kw)

        def log_message(self, *a):
            pass

        def do_GET(self):
            path = self.path.split("?")[0]
            if path in routes:
                status, headers, body = routes[path]
                self.send_response(status)
                for k, v in headers.items():
                    self.send_header(k, v)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            super().do_GET()

    srv = Server(("127.0.0.1", 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv


def parse_prefs(path):
    prefs = []
    rx = re.compile(r'^pref\("([^"]+)",\s*(.+?)(,\s*locked)?\);')
    for line in open(path, encoding="utf-8"):
        m = rx.match(line.strip())
        if not m:
            continue
        name, raw, locked = m.group(1), m.group(2).strip(), bool(m.group(3))
        if raw in ("true", "false"):
            prefs.append([name, "bool", raw == "true", locked])
        elif re.fullmatch(r"-?\d+", raw):
            prefs.append([name, "int", int(raw), locked])
        else:
            prefs.append([name, "string", json.loads(raw), locked])
    return prefs


PREFS_JS = r"""
const [expected] = args;
const def = Services.prefs.getDefaultBranch("");
return expected.map(([name, type, value, locked]) => {
  let actual = "<missing>";
  try {
    actual = type == "bool" ? def.getBoolPref(name) : type == "int" ? def.getIntPref(name) : def.getStringPref(name);
  } catch (e) {}
  return { name, expected: value, actual, expectLocked: locked, locked: Services.prefs.prefIsLocked(name) };
});
"""

SET_CLIPBOARD_JS = r"""
const [text] = args;
Cc["@mozilla.org/widget/clipboardhelper;1"].getService(Ci.nsIClipboardHelper).copyString(text);
return true;
"""

COMMAND_JS = r"""
const [command] = args;
const sleep = ms => new Promise(r => setTimeout(r, ms));
gBrowser.selectedBrowser.focus();
window.goDoCommand(command);
await sleep(700);
return true;
"""

DIALOG_JS = r"""
// The top tab-modal dialog over the selected tab, if any, with what it shows.
const sleep = ms => new Promise(r => setTimeout(r, ms));
const [waitMs] = args;
const mgr = gBrowser.getTabDialogBox(gBrowser.selectedBrowser)._tabDialogManager;
let d = null;
for (let i = 0; i < waitMs / 100 && !(d = mgr._topDialog); i++) {
  await sleep(100);
}
if (!d) {
  return { open: false, tabDialogs: mgr._dialogs.length, windowDialog: !!window.gDialogBox?.isOpen,
           boxes: [...gBrowser.browsers].map(br => br.tabDialogBox?._tabDialogManager?._dialogs.length ?? null) };
}
await d._dialogReady;
const doc = d.frameContentWindow.document;
await doc.l10n.ready;
await sleep(300);
const text = id => doc.getElementById(id)?.textContent.replace(/[\u2068\u2069]/g, "").trim() ?? null;
return {
  open: true,
  url: doc.documentURI,
  title: text("guard-title"),
  lead: text("guard-lead"),
  body: text("guard-body"),
  focused: doc.activeElement?.textContent?.trim() ?? null,
  buttons: [...doc.querySelectorAll("#guard-actions button")].map(b => b.textContent.trim()),
  marks: [...doc.querySelectorAll(".address-value mark")].map(m => m.textContent),
};
"""

CLICK_JS = r"""
const [selector] = args;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const d = gBrowser.getTabDialogBox(gBrowser.selectedBrowser)._tabDialogManager._topDialog;
if (!d) {
  return false;
}
d.frameContentWindow.document.querySelector(selector).click();
await sleep(500);
return true;
"""

HOLD_JS = r"""
// Press and hold the hold button with the mouse for `ms`, as a user would.
const [ms] = args;
const sleep = t => new Promise(r => setTimeout(r, t));
const d = gBrowser.getTabDialogBox(gBrowser.selectedBrowser)._tabDialogManager._topDialog;
if (!d) {
  return "no dialog";
}
const w = d.frameContentWindow;
const b = w.document.querySelector("button.hold");
const r = b.getBoundingClientRect();
const x = r.left + r.width / 2, y = r.top + r.height / 2;
const hit = w.document.elementFromPoint(x, y)?.className;
const r1 = w.synthesizeMouseEvent("mousedown", x, y, { button: 0 });
await sleep(400);
const during = { hit, r1, classes: b.className, status: w.document.getElementById("guard-status")?.textContent };
await sleep(ms - 400);
try {
  w.synthesizeMouseEvent("mouseup", x, y, { button: 0 });
} catch (e) {
  // The dialog closed itself when the 3 seconds were up.
}
await sleep(700);
return { open: mgr_open(), during };
function mgr_open() {
  return !!gBrowser.getTabDialogBox(gBrowser.selectedBrowser)._tabDialogManager._topDialog;
}
"""

PAGE_STATE_JS = r"""
const [ids] = args;
const values = {};
for (const id of ids) {
  values[id] = document.getElementById(id).value;
}
const page = window.wrappedJSObject || window;
return { values, log: JSON.parse(JSON.stringify(page.pageLog || [])) };
"""

RESET_JS = r"""
for (const el of document.querySelectorAll("input, textarea")) {
  el.value = "";
}
(window.wrappedJSObject || window).pageLog.length = 0;
const [id] = args;
if (id) {
  document.getElementById(id).focus();
}
return true;
"""

CONSOLE_JS = r"""
// Errors and warnings in the browser console (chrome and content), for diagnosis.
return Services.console.getMessageArray()
  .filter(m => m instanceof Ci.nsIScriptError && !(m.flags & Ci.nsIScriptError.infoFlag))
  .map(m => `${m.flags & Ci.nsIScriptError.warningFlag ? "warning" : "error"}: ${m.errorMessage} (${m.sourceName}:${m.lineNumber})`)
  .slice(-300);
"""

STATE_JS = r"""
// Is the security entry point running, and does the selected tab have the input guard?
const { WhisperSecurity } = ChromeUtils.importESModule(
  "moz-src:///browser/components/whisper/security/WhisperSecurity.sys.mjs");
let actor = null;
try {
  actor = !!gBrowser.selectedBrowser.browsingContext.currentWindowGlobal.getActor("WhisperInputGuard");
} catch (e) {
  actor = String(e);
}
return { initialized: WhisperSecurity._initialized, actor };
"""

SELECT_JS = r"""
const [id] = args;
const el = document.getElementById(id);
// Out of any field first, or Copy copies the field's (empty) selection.
document.activeElement?.blur();
const range = document.createRange();
range.selectNodeContents(el);
const sel = window.getSelection();
sel.removeAllRanges();
sel.addRange(range);
return sel.toString();
"""


class Browser:
    def __init__(self, app, out, name, user_prefs=None, env=None):
        self.profile = tempfile.mkdtemp(prefix=f"whisper-{name}-")
        with open(os.path.join(self.profile, "user.js"), "w") as f:
            f.write('user_pref("whisper.import.offerOnFirstRun", false);\n')
            # Marionette's "recommended" prefs would change some of the prefs checked here
            # (extensions.autoDisableScopes, for one).
            f.write('user_pref("remote.prefs.recommended", false);\n')
            for k, v in (user_prefs or {}).items():
                f.write(f"user_pref({json.dumps(k)}, {json.dumps(v)});\n")
        self.log = open(os.path.join(out, f"{name}-browser.log"), "w")
        e = dict(os.environ, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS="1")
        e.update(env or {})
        self.proc = subprocess.Popen([os.path.join(app, "whisper"), "--marionette", "--no-remote",
                                      "--profile", self.profile], stdout=self.log, stderr=subprocess.STDOUT, env=e)
        self.m = Marionette(alive=lambda: self.proc.poll() is None)
        self.m.start_session()
        self.m.command("WebDriver:SetWindowRect", {"x": 0, "y": 0, "width": 1280, "height": 900})
        time.sleep(3)

    def chrome(self, js, *args):
        return self.m.run(js, *args)

    def content(self, js, *args):
        self.m.command("Marionette:SetContext", {"value": "content"})
        try:
            r = self.m.command("WebDriver:ExecuteScript", {"script": js, "args": list(args)})
            return r.get("value") if isinstance(r, dict) else r
        finally:
            self.m.command("Marionette:SetContext", {"value": "chrome"})

    def navigate(self, url):
        self.m.command("Marionette:SetContext", {"value": "content"})
        try:
            self.m.command("WebDriver:Navigate", {"url": url})
        finally:
            self.m.command("Marionette:SetContext", {"value": "chrome"})

    def type_into(self, element_id, text):
        self.m.command("Marionette:SetContext", {"value": "content"})
        try:
            el = self.m.command("WebDriver:FindElement", {"using": "css selector", "value": f"#{element_id}"})
            ref = el.get("value", el)
            ref = ref.get("element-6066-11e4-a52e-4f735466cecf", ref) if isinstance(ref, dict) else ref
            self.m.command("WebDriver:ElementSendKeys", {"id": ref, "text": text})
        finally:
            self.m.command("Marionette:SetContext", {"value": "chrome"})

    def quit(self):
        try:
            self.m.quit()
        except MarionetteError:
            pass
        try:
            code = self.proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            code = "killed"
        self.log.close()
        return code


def wrap(js):
    # Content scripts: Marionette runs them as a function body with `arguments`.
    return "const args = Array.from(arguments);\n" + js


def test_prefs(b, report):
    expected = parse_prefs(SECURITY_PREFS)
    got = b.chrome(PREFS_JS, expected)
    bad = [p for p in got if p["actual"] != p["expected"] or p["locked"] != p["expectLocked"]]
    report.check(f"all {len(got)} prefs of 0-whisper-security.js are built in "
                 f"({sum(p['expectLocked'] for p in got)} locked)", not bad, json.dumps(bad)[:1500])


def paste_into(b, element_id, text):
    b.content(wrap(RESET_JS), element_id)
    b.chrome(SET_CLIPBOARD_JS, text)
    b.chrome(COMMAND_JS, "cmd_paste")


def test_seed(b, base, report, out):
    b.navigate(f"{base}/inputs.html")
    # 1. A pasted phrase: stopped before the page's own paste listener.
    paste_into(b, "single", PHRASE)
    d = b.chrome(DIALOG_JS, 3000)
    state = b.content(wrap(PAGE_STATE_JS), ["single"])
    report.facts["seed_paste_dialog"] = d
    report.check("seed: a pasted recovery phrase opens the warning", d.get("open") and "recovery phrase" in (d.get("title") or ""), d)
    report.check("seed: the warning says what to do and has the safe choice focused",
                 "Never enter your recovery phrase on a website" in (d.get("lead") or "")
                 and d.get("focused") == "Leave the field empty", d)
    report.check("seed: the page saw no paste event and the field is empty",
                 not any(e["type"] == "paste" for e in state["log"]) and state["values"]["single"] == "", state)
    capture(b, os.path.join(out, "seed-warning.png"))
    b.chrome(CLICK_JS, "button.primary")
    report.check("seed: 'Leave the field empty' closes the warning", not b.chrome(DIALOG_JS, 300).get("open"))

    # 2. Ordinary text passes untouched.
    text = "Please send me the report by Friday, thanks"
    paste_into(b, "single", text)
    d = b.chrome(DIALOG_JS, 1000)
    state = b.content(wrap(PAGE_STATE_JS), ["single"])
    report.check("seed: ordinary pasted text reaches the page and the field",
                 not d.get("open") and state["values"]["single"] == text
                 and any(e["type"] == "paste" and e["data"] == text for e in state["log"]), state)

    # 3. Holding the button allows that field; the next paste then goes through.
    paste_into(b, "single", PHRASE)
    d = b.chrome(DIALOG_JS, 3000)
    still = b.chrome(HOLD_JS, 1200)
    report.check("seed: letting go of the hold button early keeps the warning",
                 still.get("open") is True and "holding" in (still.get("during") or {}).get("classes", ""), still)
    gone = b.chrome(HOLD_JS, 3400)
    report.check("seed: holding the button for 3 seconds closes the warning", gone.get("open") is False, gone)
    b.chrome(SET_CLIPBOARD_JS, PHRASE)
    b.chrome(COMMAND_JS, "cmd_paste")
    state = b.content(wrap(PAGE_STATE_JS), ["single"])
    report.check("seed: after holding, that one field accepts the phrase",
                 state["values"]["single"] == PHRASE and not b.chrome(DIALOG_JS, 500).get("open"), state["values"])

    # 4. Typing: paused after six list words, the field emptied while the warning is up.
    b.content(wrap(RESET_JS), "area")
    b.type_into("area", " ".join(PHRASE_WORDS[:6]) + " ")
    d = b.chrome(DIALOG_JS, 3000)
    state = b.content(wrap(PAGE_STATE_JS), ["area"])
    report.facts["seed_typed"] = {"dialog": d, "values": state["values"]}
    report.check("seed: typing six words of a phrase pauses the field and warns",
                 d.get("open") and "after 6 words" in (d.get("body") or "") and state["values"]["area"] == "",
                 {"body": d.get("body"), "value": state["values"]["area"]})
    report.check("seed: the page did not get the input event after the sixth word",
                 not any(e["type"] == "input" and (e["value"] or "").count(" ") >= 6 for e in state["log"]),
                 [e for e in state["log"] if e["type"] == "input"][-2:])
    b.chrome(CLICK_JS, "button.primary")

    # 5. Ordinary typing passes.
    b.content(wrap(RESET_JS), "area")
    sentence = "the meeting moved to monday at noon so bring the slides "
    b.type_into("area", sentence)
    d = b.chrome(DIALOG_JS, 800)
    state = b.content(wrap(PAGE_STATE_JS), ["area"])
    report.check("seed: ordinary typing is not paused", not d.get("open") and state["values"]["area"] == sentence, state["values"])

    # 6. One word per field, as wallet import forms ask.
    b.content(wrap(RESET_JS), "w1")
    for i, word in enumerate(PHRASE_WORDS[:7], start=1):
        b.type_into(f"w{i}", word)
        if b.chrome(DIALOG_JS, 200).get("open"):
            break
    d = b.chrome(DIALOG_JS, 2000)
    state = b.content(wrap(PAGE_STATE_JS), [f"w{i}" for i in range(1, 8)])
    # (Marionette types straight into the page even while the tab-modal warning is
    # up, which a user cannot; so only the first six fields are looked at.)
    report.check("seed: a phrase typed one word per field is paused at the seventh field",
                 d.get("open") and all(state["values"][f"w{i}"] == "" for i in range(1, 7)), state["values"])
    b.chrome(CLICK_JS, "button.primary")


def test_address(b, base, report, out):
    b.navigate(f"{base}/inputs.html")
    # The user copies the address on the page...
    selected = b.content(wrap(SELECT_JS), "addr")
    b.chrome(COMMAND_JS, "cmd_copy")
    # ... and malware replaces it on the clipboard before the paste.
    paste_into(b, "to", SWAPPED)
    d = b.chrome(DIALOG_JS, 3000)
    state = b.content(wrap(PAGE_STATE_JS), ["to"])
    report.facts["address_dialog"] = d
    report.check("address: a swapped address is stopped and the warning opens",
                 selected == COPIED and d.get("open") and "not the one you copied" in (d.get("title") or ""), d)
    report.check("address: both addresses are shown with the difference marked",
                 COPIED in (d.get("body") or "") and SWAPPED in (d.get("body") or "") and d.get("marks"), d.get("marks"))
    report.check("address: the page did not see the paste", not any(e["type"] == "paste" for e in state["log"]), state)
    capture(b, os.path.join(out, "address-warning.png"))
    b.chrome(CLICK_JS, "button.primary")
    state = b.content(wrap(PAGE_STATE_JS), ["to"])
    report.check("address: 'Paste the address I copied' puts the copied address in the field",
                 state["values"]["to"] == COPIED, state["values"])

    # Pasting what was copied passes untouched.
    b.content(wrap(SELECT_JS), "addr")
    b.chrome(COMMAND_JS, "cmd_copy")
    b.content(wrap(RESET_JS), "to")
    b.chrome(COMMAND_JS, "cmd_paste")
    d = b.chrome(DIALOG_JS, 1000)
    state = b.content(wrap(PAGE_STATE_JS), ["to"])
    report.check("address: pasting the copied address itself goes through",
                 not d.get("open") and state["values"]["to"] == COPIED
                 and any(e["type"] == "paste" for e in state["log"]), state["values"])


def test_environment(app, out, report):
    """No keys, logs or profiles because of environment variables (patches/0061)."""
    trap = tempfile.mkdtemp(prefix="whisper-env-")
    env = {"SSLKEYLOGFILE": os.path.join(trap, "keys.txt"),
           "MOZ_LOG": "nsHttp:5,nsHostResolver:5,timestamp", "MOZ_LOG_FILE": os.path.join(trap, "moz.log"),
           "MOZ_PROFILER_STARTUP": "1", "MOZ_PROFILER_SHUTDOWN": os.path.join(trap, "profile.json")}
    b = Browser(app, out, "environment", env=env)
    try:
        # A TLS handshake would write a key line: try one (it fails, there is no server,
        # but a TLS client hello to a local port is enough to exercise NSS).
        b.navigate("https://127.0.0.1:9/")
    except MarionetteError:
        pass
    time.sleep(2)
    code = b.quit()
    written = sorted(os.listdir(trap))
    report.facts["environment_files"] = written
    report.check("environment: SSLKEYLOGFILE, MOZ_LOG and MOZ_PROFILER_SHUTDOWN write nothing",
                 not written and code == 0, {"files": written, "exit": code})
    shutil.rmtree(trap, ignore_errors=True)


def capture(b, path):
    try:
        b.m.screenshot(path)
    except MarionetteError:
        pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--only", default="")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    only = set(filter(None, a.only.split(",")))
    want = lambda name: not only or name in only  # noqa: E731
    report = Report()
    srv = start_server({})
    base = f"http://127.0.0.1:{srv.server_address[1]}"
    try:
        b = Browser(a.app, a.out, "security")
        try:
            if want("prefs"):
                test_prefs(b, report)
            if want("seed"):
                test_seed(b, base, report, a.out)
            if want("address"):
                test_address(b, base, report, a.out)
        except MarionetteError as e:
            report.check("Marionette session", False, e)
        finally:
            try:
                report.facts["security_state"] = b.chrome(STATE_JS)
                with open(os.path.join(a.out, "console.txt"), "w") as f:
                    f.write("\n".join(b.chrome(CONSOLE_JS)) + "\n")
            except MarionetteError as e:
                report.facts["console_error"] = str(e)
            code = b.quit()
            report.check("browser exits cleanly", code == 0, f"exit {code}")
        if want("environment"):
            test_environment(a.app, a.out, report)
    finally:
        srv.shutdown()
    with open(os.path.join(a.out, "security-report.json"), "w") as f:
        json.dump({"checks": report.checks, "facts": report.facts}, f, indent=2, default=str)
    with open(os.path.join(a.out, "security-report.md"), "w") as f:
        f.write("| | Check | Detail |\n|---|---|---|\n")
        for c in report.checks:
            detail = c["detail"].replace("|", "\\|").replace("\n", " ")[:300]
            f.write(f"| {'✅' if c['ok'] else '❌'} | {c['check']} | {detail} |\n")
    print(f"\n{len(report.checks) - len(report.failed)}/{len(report.checks)} checks passed")
    return 1 if report.failed else 0


if __name__ == "__main__":
    sys.exit(main())
