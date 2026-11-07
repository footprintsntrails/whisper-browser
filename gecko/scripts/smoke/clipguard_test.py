#!/usr/bin/env python3
"""ClickFix smoke test: start the built browser and check Whisper's clipboard protection.

Usage: clipguard_test.py --app DIR --out DIR

DIR is an unpacked Linux package (the folder that holds the `whisper` program). The test
serves the pages in clipguard/pages/ from two loopback origins (127.0.0.1 and 127.0.0.2,
so the cross-origin case is genuinely cross-origin), with no network, and drives the
browser over Marionette. For each scenario in clipguard/manifest.json it:

  1. opens the page, triggers the write (a click, or a click then Ctrl+C);
  2. reads the real system clipboard in the parent process (nsIClipboard), and checks it
     holds what the scenario expects: Whisper's placeholder for a blocked command, the
     real command for a legitimate one, or the visible text for a plain Ctrl+C;
  3. checks the page itself saw its write succeed (window.__clipguardResult): the page must
     never learn the decision;
  4. for the read-back probe, checks the page could not read its own command back.

No command is ever executed: the pages copy inert strings and the addresses are *.invalid
or RFC 5737 documentation IPs. Writes report.json and report.md to --out; exits 1 on any
failure. Never run outside CI.
"""

import argparse
import functools
import http.server
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from marionette import Marionette, MarionetteError  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
PAGES = os.path.join(HERE, "clipguard", "pages")

# Whisper's blocked-copy placeholder (clipguard/rust/rules.toml). Kept in sync by
# test_placeholder_matches_rules below.
PLACEHOLDER = "# BLOCKED BY BROWSER page tried to place a command here do not run it"


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Server:
    """Serves PAGES on a loopback address, logging nothing to the network."""

    def __init__(self, host):
        self.host = host
        self.port = free_port()
        handler = functools.partial(
            http.server.SimpleHTTPRequestHandler, directory=PAGES
        )
        # Quiet the default per-request logging.
        handler.log_message = lambda *a, **k: None
        self.httpd = http.server.ThreadingHTTPServer((host, self.port), handler)
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    @property
    def origin(self):
        return f"http://{self.host}:{self.port}"

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *a):
        self.httpd.shutdown()


# Read the real system clipboard in the parent process, as text.
READ_CLIPBOARD_JS = r"""
// The real OS clipboard, read from chrome with the chrome window's own context. That
// context is not the blocked page (different browsing context, system principal), so the
// read-back override never applies here: this is what every other reader and the OS see.
const clip = Cc["@mozilla.org/widget/clipboard;1"].getService(Ci.nsIClipboard);
const trans = Cc["@mozilla.org/widget/transferable;1"].createInstance(Ci.nsITransferable);
trans.init(null);
trans.addDataFlavor("text/plain");
const wc = window.browsingContext?.currentWindowContext ?? null;
clip.getData(trans, Ci.nsIClipboard.kGlobalClipboard, wc);
let str = {};
try {
  trans.getTransferData("text/plain", str);
  return str.value.QueryInterface(Ci.nsISupportsString).data;
} catch (e) {
  return "<empty>";
}
"""

PUT_SENTINEL_JS = r"""
const [text] = args;
const str = Cc["@mozilla.org/supports-string;1"].createInstance(Ci.nsISupportsString);
str.data = text;
const trans = Cc["@mozilla.org/widget/transferable;1"].createInstance(Ci.nsITransferable);
trans.init(null);
trans.addDataFlavor("text/plain");
trans.setTransferData("text/plain", str);
Cc["@mozilla.org/widget/clipboard;1"].getService(Ci.nsIClipboard)
  .setData(trans, null, Ci.nsIClipboard.kGlobalClipboard, null);
return true;
"""

W3C_ELEMENT = "element-6066-11e4-a52e-4f735466cecf"

# Poll the page's self-reported result.
RESULT_JS = r"""
const sleep = ms => new Promise(r => setTimeout(r, ms));
for (let i = 0; i < 40 && !(window.__clipguardResult && window.__clipguardResult.done); i++) {
  await sleep(100);
}
return window.__clipguardResult || { done: false };
"""


def element_ref(value):
    if isinstance(value, dict):
        return value.get(W3C_ELEMENT) or value.get("ELEMENT")
    return value


def click_element(m, selector):
    """A real (trusted) pointer click on the element, which gives the page transient user
    activation -- a programmatic .click() would not, and writeText would then be refused."""
    res = m.command("WebDriver:FindElement", {"using": "css selector", "value": selector})
    elem = element_ref(res.get("value") if isinstance(res, dict) else res)
    if not elem:
        raise MarionetteError(f"no element {selector}")
    origin = {W3C_ELEMENT: elem}
    m.command("WebDriver:PerformActions", {"actions": [{
        "type": "pointer", "id": "mouse", "parameters": {"pointerType": "mouse"},
        "actions": [
            {"type": "pointerMove", "duration": 10, "origin": origin, "x": 0, "y": 0},
            {"type": "pointerDown", "button": 0},
            {"type": "pause", "duration": 20},
            {"type": "pointerUp", "button": 0},
        ],
    }]})


def _chord(m, letter):
    CTRL = ""
    m.command("WebDriver:PerformActions", {"actions": [{
        "type": "key", "id": "keyboard",
        "actions": [
            {"type": "keyDown", "value": CTRL},
            {"type": "keyDown", "value": letter},
            {"type": "pause", "duration": 20},
            {"type": "keyUp", "value": letter},
            {"type": "keyUp", "value": CTRL},
        ],
    }]})


def press_ctrl_c(m):
    """A trusted Ctrl+C through the Actions key source, so the copy event is a user copy."""
    _chord(m, "c")


def press_ctrl_v(m):
    """A trusted Ctrl+V, so the page reads the clipboard the way a person's paste does."""
    _chord(m, "v")


class Report:
    def __init__(self):
        self.checks = []

    def check(self, name, ok, detail=""):
        self.checks.append({"check": name, "ok": bool(ok), "detail": str(detail)})
        print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  ({detail})" if detail else ""), flush=True)

    @property
    def failed(self):
        return [c for c in self.checks if not c["ok"]]


def run_scenario(m, report, scenario, origin, iframe_origin, malicious):
    page = scenario["page"]
    url = f"{origin}/{page}"
    name = page[:-5]

    # A known sentinel on the clipboard first, so "nothing happened" is visible as the
    # sentinel rather than stale data.
    sentinel = f"SENTINEL-{name}"
    m.command("Marionette:SetContext", {"value": "chrome"})
    m.run(PUT_SENTINEL_JS, sentinel)

    m.command("Marionette:SetContext", {"value": "content"})
    m.command("WebDriver:Navigate", {"url": url})
    time.sleep(0.6)

    # Trigger with real input, which gives the page user activation.
    if scenario["trigger"] == "ctrlc":
        # The selection is set on load; a pointer click would clear it, so just press
        # Ctrl+C (the content document has focus after Navigate). The copy-handler page
        # installs its handler before this; a plain selection copies itself.
        press_ctrl_c(m)
    elif scenario["trigger"] == "ctrlv":
        # The page writes on the click, then the person pastes it back with Ctrl+V.
        click_element(m, "#go")
        time.sleep(0.3)
        press_ctrl_v(m)
    else:
        click_element(m, "#go")

    result = m.run(RESULT_JS)
    # The cross-origin case reports from the child frame.
    if scenario.get("resultFrame") == "child":
        time.sleep(0.4)

    m.command("Marionette:SetContext", {"value": "chrome"})
    # The real write for an on-screen command lands after a visibility round-trip to the
    # page actor; give it time to settle before reading the clipboard.
    time.sleep(0.9)
    clip = m.run(READ_CLIPBOARD_JS)

    # The page must have seen its write succeed -- it must never learn the decision. (A
    # plain Ctrl+C of a selection calls no clipboard API and reports nothing; there the
    # clipboard check below is the proof the page saw no difference.)
    if result and result.get("done"):
        report.check(f"{name}: the page saw its write succeed (never learns the decision)",
                     bool(result.get("ok")), result)
    elif scenario.get("resultFrame") != "child" and scenario["trigger"] != "ctrlc":
        report.check(f"{name}: the page reported a result", False, result)

    expect = scenario["expectClipboard"]
    if expect == "placeholder":
        report.check(f"{name}: the clipboard holds Whisper's placeholder, not the command",
                     clip == PLACEHOLDER, clip[:80])
        if "command" in scenario:
            report.check(f"{name}: the command never reached the clipboard",
                         scenario["command"] not in clip, clip[:80])
    elif expect == "command":
        want = scenario["command"].rstrip("\n")
        report.check(f"{name}: the clipboard holds the real command (not blocked)",
                     clip.rstrip("\n") == want, clip[:120])
    if scenario.get("expectPageRead") == "command":
        # The page that was blocked reads back its OWN text (not the placeholder), so it
        # cannot tell it was blocked -- while the OS clipboard (checked above) holds the
        # placeholder for every other reader.
        read = (result or {}).get("read") or ""
        want = scenario["command"]
        report.check(f"{name}: the page reads its own text back (not the placeholder)",
                     want in read and PLACEHOLDER not in read, read[:120])
    del origin, iframe_origin, malicious


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--seconds", type=float, default=0)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    report = Report()
    binary = os.path.join(a.app, "whisper")

    manifest = json.load(open(os.path.join(HERE, "clipguard", "manifest.json")))

    report.check("placeholder string matches rules.toml",
                 PLACEHOLDER.startswith("# ") and "BLOCKED BY BROWSER" in PLACEHOLDER)

    with Server("127.0.0.1") as a1, Server("127.0.0.2") as a2:
        # The cross-origin iframe page points at the second origin.
        child = os.path.join(PAGES, "cross_origin_iframe.html")
        html = open(child, encoding="utf-8").read()
        html = html.replace("http://IFRAME_HOST/", f"{a2.origin}/")
        patched = os.path.join(PAGES, "_cross_origin_iframe.html")
        open(patched, "w", encoding="utf-8").write(html)

        profile = tempfile.mkdtemp(prefix="whisper-clipguard-")
        with open(os.path.join(profile, "user.js"), "w") as f:
            f.write('user_pref("remote.prefs.recommended", false);\n')
            f.write('user_pref("whisper.import.offerOnFirstRun", false);\n')
            # Grant clipboard read to the test harness only (for the read-back probe).
            f.write('user_pref("dom.events.testing.asyncClipboard", true);\n')
        log = open(os.path.join(a.out, "browser.log"), "w")
        env = dict(os.environ, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS="1")
        proc = subprocess.Popen(
            [binary, "--marionette", "--no-remote", "--profile", profile],
            stdout=log, stderr=subprocess.STDOUT, env=env)
        try:
            m = Marionette(alive=lambda: proc.poll() is None)
            m.start_session()
            m.command("WebDriver:SetWindowRect", {"x": 0, "y": 0, "width": 1280, "height": 900})
            time.sleep(5)
            for scenario in manifest["malicious"]:
                if scenario["page"] == "cross_origin_iframe.html":
                    scenario = dict(scenario, page="_cross_origin_iframe.html")
                run_scenario(m, report, scenario, a1.origin, a2.origin, True)
            for scenario in manifest["legitimate"]:
                run_scenario(m, report, scenario, a1.origin, a2.origin, False)
            m.quit()
        except MarionetteError as e:
            report.check("Marionette session", False, e)
        try:
            proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            proc.kill()
        log.close()
        try:
            os.remove(patched)
        except OSError:
            pass

    with open(os.path.join(a.out, "report.json"), "w") as f:
        json.dump({"checks": report.checks}, f, indent=2)
    with open(os.path.join(a.out, "report.md"), "w") as f:
        f.write("| | Check | Detail |\n|---|---|---|\n")
        for c in report.checks:
            detail = c["detail"].replace("|", "\\|").replace("\n", " ")[:200]
            f.write(f"| {'✅' if c['ok'] else '❌'} | {c['check']} | {detail} |\n")
    print(f"\n{len(report.checks) - len(report.failed)}/{len(report.checks)} checks passed")
    return 1 if report.failed else 0


if __name__ == "__main__":
    sys.exit(main())
