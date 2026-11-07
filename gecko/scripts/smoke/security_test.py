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
  programs     links to blocked schemes (ms-*, search-ms, which get a program here so
               the block is Whisper's) and to a program that is not installed do
               nothing, and nothing opens without a click; a click on a link to an
               installed program asks, naming the program and its path; Cancel starts
               nothing, Open works only a second after the dialog has the focus
  cleanlinks   following a link to another site drops utm_*, fbclid and gclid but keeps
               the real parameter; Copy Link copies the clean link
  settings     the settings fragment (settings/whisper-security-settings.mjs) renders
               in about:preferences with every switch on and its text, the address
               guard's text says outside copies can't be compared, nothing says
               "protected"
  downloads    the guard engine (Rust as WebAssembly in a worker) runs; a disc image,
               a disguised program, a program named as a picture and a ZIP holding a
               program are held (no file in Downloads), a PDF is not; a held file is
               only there once allowed

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


def make_pe():
    pe = bytearray(512)
    pe[:2] = b"MZ"
    pe[0x3C] = 0x80
    pe[0x80:0x84] = b"PE\0\0"
    return bytes(pe)


def make_zip(names):
    """A real ZIP (stored, no compression) with these file names."""
    import io
    import zipfile
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        for n in names:
            z.writestr(n, b"x")
    return buf.getvalue()


def make_iso(names):
    """A minimal ISO 9660 image whose root holds these (empty) files."""
    iso = bytearray(20 * 2048)
    pvd = 16 * 2048
    iso[pvd] = 1
    iso[pvd + 1:pvd + 6] = b"CD001"
    iso[pvd + 128:pvd + 130] = (2048).to_bytes(2, "little")
    iso[pvd + 156 + 2:pvd + 156 + 6] = (18).to_bytes(4, "little")
    iso[pvd + 156 + 10:pvd + 156 + 14] = (2048).to_bytes(4, "little")
    iso[17 * 2048] = 255
    iso[17 * 2048 + 1:17 * 2048 + 6] = b"CD001"
    at = 18 * 2048
    for raw in [b"\0", b"\1"] + [f"{n};1".encode() for n in names]:
        length = 33 + len(raw) + (len(raw) + 1) % 2
        iso[at] = length
        iso[at + 32] = len(raw)
        iso[at + 33:at + 33 + len(raw)] = raw
        at += length
    return bytes(iso)


def download_routes():
    def attach(name, body, mime="application/octet-stream"):
        return (200, {"Content-Type": mime, "Content-Disposition": f'attachment; filename="{name}"'}, body)
    return {
        "/dl/report.pdf": attach("report.pdf", b"%PDF-1.4\n%Whisper test\n", "application/pdf"),
        "/dl/disk.iso": attach("disk.iso", make_iso(["INVOICE.LNK", "README.TXT"])),
        "/dl/invoice.pdf.exe": attach("invoice.pdf.exe", make_pe()),
        "/dl/photo.jpg": attach("photo.jpg", make_pe(), "image/jpeg"),
        "/dl/tool.zip": attach("tool.zip", make_zip(["tool/setup.exe", "tool/readme.txt"]), "application/zip"),
    }


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

ENGINE_JS = r"""
// The guard engine: Rust compiled to WebAssembly, run in a ChromeWorker.
const { WhisperGuardEngine } = ChromeUtils.importESModule(
  "moz-src:///browser/components/whisper/security/WhisperGuardEngine.sys.mjs");
const started = Date.now();
const a = await WhisperGuardEngine.proto("search-ms");
const b = await WhisperGuardEngine.proto("zoommtg");
return { blocked: a.tier, asked: b.tier, ms: Date.now() - started };
"""

DOWNLOAD_JS = r"""
// Download a file through Firefox's download manager (the same verdict step as a
// click on a link: DownloadCopySaver and DownloadLegacySaver share it), into the
// downloads folder with a ".part" file, and wait until it stops.
const [url, name] = args;
const { Downloads } = ChromeUtils.importESModule("resource://gre/modules/Downloads.sys.mjs");
const dir = await Downloads.getPreferredDownloadsDirectory();
const path = PathUtils.join(dir, name);
const d = await Downloads.createDownload({ source: url, target: { path, partFilePath: path + ".part" } });
const list = await Downloads.getList(Downloads.ALL);
await list.add(d);
try {
  await d.start();
} catch (e) {
  // A held download ends with an error that says so.
}
const exists = async p => !!p && (await IOUtils.exists(p)) && (await IOUtils.stat(p)).size > 0;
return {
  succeeded: d.succeeded,
  hasBlockedData: d.hasBlockedData,
  verdict: d.error?.reputationCheckVerdict ?? null,
  targetHasData: await exists(d.target.path),
  partHasData: await exists(d.target.partFilePath),
};
"""

DESCRIBE_JS = r"""
// What the downloads panel would say about the last held download, then allow it.
const [allow] = args;
const { Downloads } = ChromeUtils.importESModule("resource://gre/modules/Downloads.sys.mjs");
const { WhisperDownloadGuard } = ChromeUtils.importESModule(
  "moz-src:///browser/components/whisper/security/WhisperDownloadGuard.sys.mjs");
const list = await Downloads.getList(Downloads.ALL);
const held = (await list.getAll()).filter(d => d.hasBlockedData);
const d = held[held.length - 1];
const text = WhisperDownloadGuard.describe(d);
let after = null;
if (allow) {
  await d.unblock();
  after = { succeeded: d.succeeded, targetHasData: (await IOUtils.stat(d.target.path)).size > 0 };
}
return { text, after };
"""

HANDLER_JS = r"""
// A program for the test scheme "whispertest:", as if one were installed: /bin/true.
const eps = Cc["@mozilla.org/uriloader/external-protocol-service;1"].getService(Ci.nsIExternalProtocolService);
const hs = Cc["@mozilla.org/uriloader/handler-service;1"].getService(Ci.nsIHandlerService);
// The blocked schemes get one too, so that the block is Whisper's and not Firefox's
// "no program for this" (on Linux nothing handles search-ms: or ms-*: at all).
// The program writes what it was started with into the profile, so the test can tell.
const program = PathUtils.join(PathUtils.profileDir, "whisper-test-program.sh");
const ran = PathUtils.join(PathUtils.profileDir, "whisper-test-program-ran.txt");
await IOUtils.writeUTF8(program, `#!/bin/sh\necho "$1" >> '${ran}'\n`);
await IOUtils.setPermissions(program, 0o700);
for (const scheme of ["whispertest", "search-ms", "ms-whispertest"]) {
  const info = eps.getProtocolHandlerInfo(scheme);
  const app = Cc["@mozilla.org/uriloader/local-handler-app;1"].createInstance(Ci.nsILocalHandlerApp);
  app.executable = new FileUtils.File(program);
  app.name = "Whisper Test Program";
  info.preferredApplicationHandler = app;
  info.preferredAction = Ci.nsIHandlerInfo.useHelperApp;
  info.alwaysAskBeforeHandling = false;
  hs.store(info);
}
return program;
"""

PROGRAM_RAN_JS = r"""
// What the test program was started with ("" when it never ran).
const [waitMs] = args;
const ran = PathUtils.join(PathUtils.profileDir, "whisper-test-program-ran.txt");
for (let i = 0; i <= waitMs / 100; i++) {
  if (await IOUtils.exists(ran)) {
    return (await IOUtils.readUTF8(ran)).trim();
  }
  await new Promise(r => setTimeout(r, 100));
}
return "";
"""

REFOCUS_JS = r"""
// The tab dialog's window loses and gets back the focus (as when the user switches to
// another window and back).
const d = gBrowser.getTabDialogBox(gBrowser.selectedBrowser)._tabDialogManager._topDialog;
const w = d.frameContentWindow;
w.dispatchEvent(new w.FocusEvent("blur"));
w.dispatchEvent(new w.FocusEvent("focus"));
return true;
"""

ANY_DIALOG_JS = r"""
// Any dialog over the tab or the window (Whisper's or Firefox's own).
const sleep = ms => new Promise(r => setTimeout(r, ms));
const [waitMs] = args;
const box = gBrowser.getTabDialogBox(gBrowser.selectedBrowser);
for (let i = 0; i < waitMs / 100; i++) {
  const d = box._tabDialogManager._topDialog ?? box.getContentDialogManager?.()._topDialog;
  if (d || window.gDialogBox?.isOpen) {
    await Promise.race([d?._dialogReady, sleep(3000)]);
    return { open: true, url: d?.frameContentWindow?.document.documentURI ?? "window dialog" };
  }
  await sleep(100);
}
return { open: false };
"""

CLIPBOARD_JS = r"""
const trans = Cc["@mozilla.org/widget/transferable;1"].createInstance(Ci.nsITransferable);
trans.init(null);
trans.addDataFlavor("text/plain");
Services.clipboard.getData(trans, Ci.nsIClipboard.kGlobalClipboard);
const out = {};
try {
  trans.getTransferData("text/plain", out);
  return out.value.QueryInterface(Ci.nsISupportsString).data;
} catch (e) {
  return null;
}
"""

COPY_LINK_JS = r"""
// Right-click the link like a user and choose Copy Link.
const [x, y] = args;
const sleep = ms => new Promise(r => setTimeout(r, ms));
const menu = document.getElementById("contentAreaContextMenu");
const shown = new Promise(r => menu.addEventListener("popupshown", r, { once: true }));
const box = gBrowser.selectedBrowser.getBoundingClientRect();
window.synthesizeMouseEvent("contextmenu", box.left + x, box.top + y, { button: 2 });
await Promise.race([shown, sleep(3000)]);
if (menu.state != "open") {
  return { error: "the context menu did not open" };
}
const visible = id => !document.getElementById(id).hidden;
const items = { copyLink: visible("context-copylink"), copyClean: visible("context-stripOnShareLink") };
document.getElementById("context-copylink").doCommand();
menu.hidePopup();
await sleep(300);
return items;
"""

LINK_RECT_JS = """
const r = document.getElementById(arguments[0]).getBoundingClientRect();
return [r.left + r.width / 2, r.top + r.height / 2];
"""

URL_JS = "return gBrowser.selectedBrowser.currentURI.spec;"

SETTINGS_JS = r"""
// Loads the settings fragment into about:preferences, as the Whisper pane will, renders
// its groups and returns what each shows.
const [groups] = args;
const HTML = "http://www.w3.org/1999/xhtml";
const sleep = ms => new Promise(r => setTimeout(r, ms));
const tab = gBrowser.addTrustedTab("about:preferences");
gBrowser.selectedTab = tab;
let win = null;
for (let i = 0; i < 150; i++) {
  win = tab.linkedBrowser.contentWindow;
  if (win?.document.readyState == "complete" && win.SettingGroupManager && win.Preferences) {
    break;
  }
  await sleep(100);
}
const doc = win.document;
doc.l10n.addResourceIds(["whisper/whisper-security-settings.ftl"]);
// As SettingPaneManager loads a pane's module: into the page's own module map.
win.ChromeUtils.importESModule(
  "chrome://whisper/content/security/settings/whisper-security-settings.mjs",
  { global: "current" }
);
const shown = {};
const elements = [];
for (const id of groups) {
  const config = win.SettingGroupManager.get(id);
  if (!config) {
    shown[id] = null;
    continue;
  }
  const group = doc.createElementNS(HTML, "setting-group");
  group.setAttribute("groupid", id);
  group.config = config;
  group.getSetting = win.Preferences.getSetting.bind(win.Preferences);
  (doc.getElementById("mainPrefPane") ?? doc.documentElement).append(group);
  elements.push([id, group]);
}
await sleep(1500);
const walk = (node, found) => {
  for (const el of node.querySelectorAll("*")) {
    found.push(el);
    if (el.shadowRoot) {
      walk(el.shadowRoot, found);
    }
  }
  return found;
};
for (const [id, group] of elements) {
  const all = walk(group, []);
  const attr = (el, name) => el[name] ?? el.getAttribute(name) ?? "";
  shown[id] = {
    label: attr(all.find(el => el.localName == "moz-fieldset") ?? group, "label"),
    description: attr(all.find(el => el.localName == "moz-fieldset") ?? group, "description"),
    checkboxes: all.filter(el => el.localName == "moz-checkbox").map(el => ({
      label: attr(el, "label"),
      description: attr(el, "description"),
      checked: !!el.checked,
      hidden: el.closest("setting-control")?.hidden ?? false,
    })),
    messages: all.filter(el => el.localName == "moz-message-bar").map(el => attr(el, "message")),
  };
}
gBrowser.removeTab(tab);
return shown;
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
        if os.environ.get("WHISPER_TEST_TRACE"):
            # Every Marionette command and how long it took, to find a hang.
            command = self.m.command

            def traced(name, params=None):
                started = time.time()
                print(f"  -> {name}", flush=True)
                try:
                    return command(name, params)
                finally:
                    print(f"  <- {name} {time.time() - started:.1f}s", flush=True)
            self.m.command = traced
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

    def navigate(self, url, timeout=None):
        """`timeout` (seconds) replaces the socket timeout for this one load."""
        self.m.command("Marionette:SetContext", {"value": "content"})
        old = self.m.sock.gettimeout()
        if timeout:
            self.m.sock.settimeout(timeout)
        try:
            self.m.command("WebDriver:Navigate", {"url": url})
        finally:
            self.m.sock.settimeout(old)
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

    # A page's copy button (navigator.clipboard.writeText) is a copy in Whisper too: the
    # clipboard guard's parent code tells the address guard (ClipboardGuard.cpp,
    # "whisper-script-copy"). The button copies the other address, so pasting the first
    # one now is the mismatch.
    click(b, "copybtn")
    time.sleep(0.5)
    paste_into(b, "to", COPIED)
    d = b.chrome(DIALOG_JS, 3000)
    report.facts["address_script_copy"] = d
    report.check("address: an address a page's copy button wrote (navigator.clipboard) is compared too",
                 d.get("open") and SWAPPED in (d.get("body") or ""), d)
    if d.get("open"):
        b.chrome(CLICK_JS, "button.primary")


def test_downloads(b, base, report):
    engine = b.chrome(ENGINE_JS)
    report.facts["engine"] = engine
    report.check("downloads: the guard engine (Rust, WebAssembly in a worker) answers",
                 engine.get("blocked") == "block" and engine.get("asked") == "ask", engine)
    results = {}
    for name in ["report.pdf", "disk.iso", "invoice.pdf.exe", "photo.jpg", "tool.zip"]:
        results[name] = b.chrome(DOWNLOAD_JS, f"{base}/dl/{name}", name)
    report.facts["downloads"] = results
    ok = results["report.pdf"]
    report.check("downloads: a PDF is not held", ok.get("succeeded") and ok.get("targetHasData") and not ok.get("verdict"), ok)
    expect = {"disk.iso": "disk-image", "invoice.pdf.exe": "double-extension",
              "photo.jpg": "content-mismatch", "tool.zip": "archive-holds"}
    for name, code in expect.items():
        r = results[name]
        report.check(f"downloads: {name} is held ({code}) and not in Downloads",
                     r.get("hasBlockedData") and not r.get("succeeded") and r.get("verdict") == f"WhisperHeld:{code}"
                     and not r.get("targetHasData") and r.get("partHasData"), r)
    shown = b.chrome(DESCRIBE_JS, True)
    report.facts["download_text"] = shown
    text = json.dumps(shown.get("text"))
    report.check("downloads: the panel says why, and that the check ran on this device",
                 "Whisper held this file" in text and "on this device" in text and "tool/setup.exe" in text, shown.get("text"))
    report.check("downloads: allowing a held file puts it in Downloads",
                 (shown.get("after") or {}).get("succeeded") and shown["after"].get("targetHasData"), shown.get("after"))


def click(b, element_id):
    b.m.command("Marionette:SetContext", {"value": "content"})
    try:
        el = b.m.command("WebDriver:FindElement", {"using": "css selector", "value": f"#{element_id}"})
        ref = el.get("value", el)
        ref = ref.get("element-6066-11e4-a52e-4f735466cecf", ref) if isinstance(ref, dict) else ref
        b.m.command("WebDriver:ElementClick", {"id": ref})
    finally:
        b.m.command("Marionette:SetContext", {"value": "chrome"})


def test_programs(b, base, report, out):
    program = b.chrome(HANDLER_JS)
    b.navigate(f"{base}/programs.html")
    for link, what in [("officecmd", "ms-officecmd"), ("searchms", "search-ms"), ("msother", "another ms-* scheme"),
                       ("unknown", "a scheme no program handles")]:
        click(b, link)
        d = b.chrome(ANY_DIALOG_JS, 1500)
        report.check(f"programs: a click on a {what} link opens nothing and asks nothing", not d.get("open"), d)
        if d.get("open"):
            b.chrome(CLICK_JS, "button.primary")
    # No click: a page script sends the tab to the installed program's scheme.
    b.navigate(f"{base}/programs.html?auto")
    d = b.chrome(ANY_DIALOG_JS, 2000)
    report.check("programs: without a click nothing opens and nothing asks, even for an installed program",
                 not d.get("open"), d)
    ran = b.chrome(PROGRAM_RAN_JS, 500)
    report.check("programs: none of these started a program (search-ms and ms-* do have one here)", ran == "", ran)
    b.navigate(f"{base}/programs.html")
    click(b, "known")
    d = b.chrome(DIALOG_JS, 3000)
    report.facts["programs_dialog"] = d
    body = (d.get("lead") or "") + " " + (d.get("body") or "")
    report.check("programs: a click on a link to an installed program asks, naming the exact site, "
                 "the program and its full path",
                 d.get("open") and base in body and "Whisper Test Program" in body and program in body
                 and d.get("focused") == "Cancel", d)
    open_button = "#guard-actions button:not(.primary)"
    # Marionette's click waits seconds for a page load, so the dialog has been open for
    # longer than a second by now; the same delay starts again whenever the dialog's
    # window gets the focus back, which is what this checks.
    b.chrome(REFOCUS_JS)
    b.chrome(CLICK_JS, open_button)
    report.check("programs: Open does nothing for a second after the dialog gets the focus",
                 b.chrome(DIALOG_JS, 300).get("open") and b.chrome(PROGRAM_RAN_JS, 0) == "")
    capture(b, os.path.join(out, "program-dialog.png"))
    b.chrome(CLICK_JS, "button.primary")
    report.check("programs: Cancel closes it and the program does not start",
                 not b.chrome(DIALOG_JS, 300).get("open") and b.chrome(PROGRAM_RAN_JS, 1000) == "")
    click(b, "known")
    b.chrome(DIALOG_JS, 3000)
    time.sleep(1.5)
    b.chrome(CLICK_JS, open_button)
    ran = b.chrome(PROGRAM_RAN_JS, 3000)
    report.check("programs: after a second Open starts the program with the link", ran == "whispertest:hello", ran)


SETTINGS_GROUPS = ["whisperRecoveryPhrase", "whisperSwappedAddress", "whisperDownloads", "whisperPrograms",
                   "whisperCleanLinks"]


def test_settings(b, report):
    shown = b.chrome(SETTINGS_JS, SETTINGS_GROUPS)
    report.facts["settings"] = shown
    missing = [g for g in SETTINGS_GROUPS if not shown.get(g) or not shown[g]["checkboxes"]]
    report.check("settings: every group of the fragment renders in about:preferences, with its switches",
                 not missing, missing or shown)
    text = json.dumps(shown, ensure_ascii=False)
    untranslated = [g for g in SETTINGS_GROUPS if not shown.get(g) or not shown[g]["messages"]
                    or not all(shown[g]["messages"]) or not all(c["label"] for c in shown[g]["checkboxes"])]
    report.check("settings: every switch and every limits note has its text", not untranslated, untranslated)
    report.check("settings: each feature is on by default",
                 all(c["checked"] for g in SETTINGS_GROUPS for c in (shown.get(g) or {}).get("checkboxes", [])
                     if not c["hidden"]), shown)
    address = json.dumps(shown.get("whisperSwappedAddress"))
    report.check("settings: the address guard says copies outside Whisper can't be compared",
                 "copied in another program" in address and "can't be compared" in address, address)
    report.check("settings: no text claims the user is protected or safe",
                 not re.search(r"\b(protected|you're safe|you are safe|keeps you safe)\b", text, re.I), text)


def test_cleanlinks(b, base, report):
    b.navigate(f"{base}/cleanlinks.html")
    click(b, "out")
    time.sleep(2)
    url = b.chrome(URL_JS)
    report.facts["cleanlinks_url"] = url
    report.check("cleanlinks: following a link to another site drops utm_*, fbclid and gclid, keeps id",
                 url.startswith("http://localhost:") and url.endswith("/landing.html?id=7"), url)
    b.navigate(f"{base}/cleanlinks.html")
    x, y = b.content(LINK_RECT_JS, "out")
    b.chrome(SET_CLIPBOARD_JS, "nothing copied yet")
    items = b.chrome(COPY_LINK_JS, x, y)
    copied = b.chrome(CLIPBOARD_JS)
    report.facts["cleanlinks_copy"] = {"items": items, "copied": copied}
    report.check("cleanlinks: Copy Link copies the clean link (Copy Clean Link stays in the menu)",
                 isinstance(copied, str) and copied.endswith("/landing.html?id=7") and "utm_" not in copied
                 and items.get("copyClean"), {"items": items, "copied": copied})


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
    srv = start_server(download_routes())
    base = f"http://127.0.0.1:{srv.server_address[1]}"
    try:
        b = Browser(a.app, a.out, "security")
        try:
            # On a fresh profile uBlock Origin holds every request until its filter lists
            # are loaded, which can take minutes on a slow machine: the first page waits
            # for that, so the tests below do not run into it.
            started = time.time()
            b.navigate(f"{base}/landing.html", timeout=600)
            report.facts["first_page_load_seconds"] = round(time.time() - started, 1)
            if want("prefs"):
                test_prefs(b, report)
            if want("seed"):
                test_seed(b, base, report, a.out)
            if want("address"):
                test_address(b, base, report, a.out)
            if want("programs"):
                test_programs(b, base, report, a.out)
            if want("downloads"):
                test_downloads(b, base, report)
            if want("cleanlinks"):
                test_cleanlinks(b, base, report)
            if want("settings"):
                test_settings(b, report)
        except (MarionetteError, OSError) as e:
            report.check("Marionette session", False, repr(e))
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
