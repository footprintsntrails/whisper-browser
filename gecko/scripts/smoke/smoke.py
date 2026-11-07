#!/usr/bin/env python3
"""Whisper smoke test: start the built browser with a fresh profile and check it.

Usage: smoke.py --app DIR --prefs 0-whisper.js --out DIR [--version 159.0.1]

DIR is an unpacked Linux package (the folder that holds the `whisper` program). The
browser runs under the X display in $DISPLAY (xvfb in CI). The test:

  1. `whisper --version` says Whisper;
  2. a headless start with an empty home folder creates Whisper's own profile folder and
     nothing under ~/.mozilla (Whisper never touches a Firefox profile);
  3. a normal start with a fresh profile, driven over Marionette (checks take about 20
     seconds; the browser then idles until --seconds, 150 by default, have passed):
     name, title, brand strings, User-Agent, build flags (no telemetry, crash reporter,
     Normandy; Firefox's updater on, without maintenance service, update agent or BITS),
     the update channel and Whisper's update address, the about dialog ("Whisper
     <version>", "Based on Firefox <version>"), every pref in 0-whisper.js active with
     its value and lock,
     uBlock Origin installed from the distribution folder, enabled, allowed in private
     windows and pinned to the toolbar, no welcome or terms tab, the Cast stylesheet
     loaded, the slim toolbar (back, forward, reload, address bar), the three sidebar
     modes (with a screenshot of each) with the reload button working in each, the
     auto-hide reveal in the 14 px strip at the left edge (the real X pointer with
     xdotool when installed), horizontal tabs and back
     keeping every tab, the sidebar menu items and the keyboard shortcut. A local web
     server on 127.0.0.1 serves the test pages;
  4. after the idle time the browser has checked for updates on its own (the update
     timer fired; the check itself finds no network here);
  5. the browser quits cleanly (exit code 0), and the profile on disk agrees
     (extensions.json, extension-preferences.json, no Telemetry client ID).

Writes report.json, report.md and screenshots to --out. Exits 1 if a check fails.
"""

import argparse
import functools
import http.server
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from marionette import Marionette, MarionetteError  # noqa: E402

UBO_ID = "uBlock0@raymondhill.net"


def read_env(name):
    """KEY=value lines of gecko/<name>."""
    env = {}
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", name)
    for line in open(path, encoding="utf-8"):
        m = re.match(r"^([A-Z0-9_]+)=(.*)$", line.strip())
        if m:
            env[m.group(1)] = m.group(2)
    return env


class Report:
    def __init__(self):
        self.checks = []
        self.facts = {}

    def check(self, name, ok, detail=""):
        self.checks.append({"check": name, "ok": bool(ok), "detail": str(detail)})
        print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  ({detail})" if detail else ""), flush=True)

    @property
    def failed(self):
        return [c for c in self.checks if not c["ok"]]


def parse_prefs(path):
    """pref("name", value[, locked]); lines of 0-whisper.js."""
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


CHECK_JS = r"""
const [expectedPrefs] = args;
const { AppConstants } = ChromeUtils.importESModule("resource://gre/modules/AppConstants.sys.mjs");
const { AddonManager } = ChromeUtils.importESModule("resource://gre/modules/AddonManager.sys.mjs");
const { ExtensionPermissions } = ChromeUtils.importESModule("resource://gre/modules/ExtensionPermissions.sys.mjs");
const { CustomizableUI } = ChromeUtils.importESModule("moz-src:///browser/components/customizableui/CustomizableUI.sys.mjs");
const r = {};
r.appinfo = {
  name: Services.appinfo.name,
  vendor: Services.appinfo.vendor,
  version: Services.appinfo.version,
  id: Services.appinfo.ID,
};
r.title = document.title;
r.brandProperties = Services.strings
  .createBundle("chrome://branding/locale/brand.properties")
  .GetStringFromName("brandFullName");
const l10n = new Localization(["branding/brand.ftl", "browser/aboutDialog.ftl", "browser/browser.ftl"]);
const [about] = await l10n.formatMessages([{ id: "aboutDialog-title" }]);
r.aboutTitle = about?.attributes?.find(a => a.name == "title")?.value ?? null;
r.userAgent = navigator.userAgent;
r.constants = {};
for (const k of ["MOZILLA_OFFICIAL", "MOZ_TELEMETRY_REPORTING", "MOZ_DATA_REPORTING",
                 "MOZ_CRASHREPORTER", "MOZ_NORMANDY", "MOZ_UPDATER", "MOZ_SERVICES_HEALTHREPORT",
                 "MOZ_REQUIRE_SIGNING", "MOZ_APP_NAME", "MOZ_APP_BASENAME", "MOZ_APP_VERSION_DISPLAY",
                 "MOZ_UPDATE_CHANNEL", "MOZ_SOURCE_URL", "MOZ_MAINTENANCE_SERVICE", "MOZ_UPDATE_AGENT",
                 "MOZ_BITS_DOWNLOAD"]) {
  r.constants[k] = AppConstants[k] ?? null;
}
{
  const { UpdateUtils } = ChromeUtils.importESModule("resource://gre/modules/UpdateUtils.sys.mjs");
  const aus = Cc["@mozilla.org/updates/update-service;1"].getService(Ci.nsIApplicationUpdateService);
  r.update = {
    channel: UpdateUtils.UpdateChannel,
    url: await UpdateUtils.formatUpdateURL(Services.appinfo.updateURL),
    disabled: aus.disabled,
    canCheck: aus.canCheckForUpdates,
    canApply: aus.canApplyUpdates,
    whisperVersion: Services.prefs.getDefaultBranch("").getStringPref("whisper.version", ""),
    whisperVersionLocked: Services.prefs.prefIsLocked("whisper.version"),
  };
}
r.telemetry = {
  canRecordBase: Services.telemetry.canRecordBase,
  canRecordExtended: Services.telemetry.canRecordExtended,
};
const def = Services.prefs.getDefaultBranch("");
r.prefs = expectedPrefs.map(([name, type, value, locked]) => {
  let actual = null, effective = null;
  try {
    actual = type == "bool" ? def.getBoolPref(name) : type == "int" ? def.getIntPref(name) : def.getStringPref(name);
    effective = type == "bool" ? Services.prefs.getBoolPref(name) : type == "int" ? Services.prefs.getIntPref(name) : Services.prefs.getStringPref(name);
  } catch (e) {
    actual = "<missing>";
  }
  return { name, expected: value, actual, effective, expectLocked: locked, locked: Services.prefs.prefIsLocked(name) };
});
const addon = await AddonManager.getAddonByID("uBlock0@raymondhill.net");
if (addon) {
  const perms = await ExtensionPermissions.get(addon.id);
  r.ubo = {
    version: addon.version,
    isActive: addon.isActive,
    userDisabled: addon.userDisabled,
    appDisabled: addon.appDisabled,
    signedState: addon.signedState,
    installSource: addon.installTelemetryInfo?.source ?? null,
    location: addon.location?.name ?? null,
    privateBrowsing: perms.permissions.includes("internal:privateBrowsingAllowed"),
    button: CustomizableUI.getPlacementOfWidget("ublock0_raymondhill_net-browser-action", false),
  };
}
r.tabs = gBrowser.tabs.map(t => t.linkedBrowser.currentURI.spec);
const c = document.getElementById("sidebar-container");
r.sidebar = {
  mode: document.documentElement.getAttribute("whisper-sidebar-mode"),
  modePref: Services.prefs.getStringPref("whisper.sidebar.mode", ""),
  visibility: Services.prefs.getStringPref("sidebar.visibility", ""),
  verticalTabs: Services.prefs.getBoolPref("sidebar.verticalTabs", false),
  expanded: c?.hasAttribute("sidebar-launcher-expanded") ?? null,
  width: c ? Math.round(c.getBoundingClientRect().width) : null,
};
const rootStyle = getComputedStyle(document.documentElement);
r.cast = {
  wBlack: rootStyle.getPropertyValue("--w-black").trim(),
  navbarBackground: getComputedStyle(document.getElementById("nav-bar")).backgroundColor,
  fontFamily: rootStyle.fontFamily,
  tabMinHeight: rootStyle.getPropertyValue("--tab-min-height").trim(),
};
r.shortcutKey = (() => {
  const k = document.getElementById("key_whisperSidebarMode");
  return k ? { key: k.getAttribute("key"), modifiers: k.getAttribute("modifiers") } : null;
})();
r.dirs = {
  profile: Services.dirsvc.get("ProfD", Ci.nsIFile).path,
  distribution: Services.dirsvc.get("XREAppDist", Ci.nsIFile).path,
};
return r;
"""

MODE_JS = r"""
const [mode] = args;
const sleep = ms => new Promise(res => setTimeout(res, ms));
if (mode) {
  Services.prefs.setStringPref("whisper.sidebar.mode", mode);
}
await sleep(1500);
const c = document.getElementById("sidebar-container");
return {
  mode: document.documentElement.getAttribute("whisper-sidebar-mode"),
  visibility: Services.prefs.getStringPref("sidebar.visibility", ""),
  expanded: c.hasAttribute("sidebar-launcher-expanded"),
  width: Math.round(c.getBoundingClientRect().width),
  tabboxLeft: Math.round(document.getElementById("tabbrowser-tabbox").getBoundingClientRect().left),
};
"""

SYNTH_MOVE_JS = r"""
const [x, y] = args;
const sleep = ms => new Promise(res => setTimeout(res, ms));
// Screen pixels to window coordinates; a synthesized pointer move, which Firefox's
// own hover tracking reacts to.
const cx = x - window.mozInnerScreenX, cy = y - window.mozInnerScreenY;
window.synthesizeMouseEvent("mousemove", cx, cy);
await sleep(150);
window.synthesizeMouseEvent("mousemove", cx, cy + 6);
return true;
"""

SIDEBAR_STATE_JS = r"""
const sleep = ms => new Promise(res => setTimeout(res, ms));
await sleep(1400);
const c = document.getElementById("sidebar-container");
return {
  expanded: c.hasAttribute("sidebar-launcher-expanded"),
  width: Math.round(c.getBoundingClientRect().width),
  tabboxLeft: Math.round(document.getElementById("tabbrowser-tabbox").getBoundingClientRect().left),
};
"""

LOAD_JS = r"""
const [url] = args;
const sleep = ms => new Promise(res => setTimeout(res, ms));
gBrowser.selectedBrowser.fixupAndLoadURIString(url, {
  triggeringPrincipal: Services.scriptSecurityManager.getSystemPrincipal(),
});
for (let i = 0; i < 50; i++) {
  await sleep(200);
  if (gBrowser.selectedBrowser.currentURI.spec == url && !gBrowser.selectedBrowser.webProgress?.isLoadingDocument) {
    break;
  }
}
return gBrowser.selectedBrowser.currentURI.spec;
"""

TOOLBAR_JS = r"""
// The toolbar row (WhisperToolbar.sys.mjs): what is placed and what shows.
const { CustomizableUI } = ChromeUtils.importESModule("moz-src:///browser/components/customizableui/CustomizableUI.sys.mjs");
const shown = id => {
  const e = document.getElementById(id);
  return !!e && e.getBoundingClientRect().width > 0 && getComputedStyle(e).visibility != "hidden";
};
return {
  placements: CustomizableUI.getWidgetIdsInArea("nav-bar"),
  shown: ["sidebar-button", "back-button", "forward-button", "reload-button", "stop-button", "urlbar-container",
          "home-button", "firefox-view-button", "fxa-toolbar-menu-button", "alltabs-button", "new-tab-button",
          "unified-extensions-button", "PanelUI-menu-button"].filter(shown),
  layoutVersion: Services.prefs.getIntPref("whisper.toolbar.layoutVersion", 0),
  navbarHeight: Math.round(document.getElementById("nav-bar").getBoundingClientRect().height),
};
"""

RELOAD_JS = r"""
// Clicks the toolbar's reload button and checks that the page loaded again.
const sleep = ms => new Promise(res => setTimeout(res, ms));
const browser = gBrowser.selectedBrowser;
for (let i = 0; i < 25 && browser.webProgress?.isLoadingDocument; i++) {
  await sleep(200);
}
const button = document.getElementById("reload-button");
if (!button || !button.getBoundingClientRect().width) {
  return { shown: false };
}
const before = browser.browsingContext.currentWindowGlobal?.innerWindowId;
let after = before;
let clicks = 0;
// A second click if the first lands while the sidebar is still moving.
for (let attempt = 0; attempt < 2 && after == before; attempt++) {
  const r = button.getBoundingClientRect();
  window.synthesizeMouseEvent("mousedown", r.left + r.width / 2, r.top + r.height / 2, { button: 0 });
  window.synthesizeMouseEvent("mouseup", r.left + r.width / 2, r.top + r.height / 2, { button: 0 });
  clicks++;
  for (let i = 0; i < 25 && after == before; i++) {
    await sleep(200);
    after = browser.browsingContext.currentWindowGlobal?.innerWindowId;
  }
}
return { shown: true, reloaded: after != before, clicks, url: browser.currentURI.spec };
"""

TABS_JS = r"""
// Switches the tab orientation (WhisperLayout) and reports what shows.
const [tabs] = args;
const sleep = ms => new Promise(res => setTimeout(res, ms));
const { WhisperLayout } = ChromeUtils.importESModule("moz-src:///browser/components/whisper/WhisperLayout.sys.mjs");
if (tabs) {
  WhisperLayout.set({ tabs });
  await sleep(2500);
}
const shown = e => !!e && e.getBoundingClientRect().width > 0 && e.getBoundingClientRect().height > 0;
const tabstrip = document.getElementById("TabsToolbar");
const vertical = document.getElementById("vertical-tabs");
return {
  tabs: WhisperLayout.state.tabs,
  sidebarMode: WhisperLayout.state.sidebarMode,
  rootTabs: document.documentElement.getAttribute("whisper-tabs"),
  tabstripShown: shown(tabstrip) && tabstrip.getBoundingClientRect().height > 20,
  tabsInTabstrip: !!tabstrip.querySelector("#tabbrowser-tabs"),
  verticalTabListShown: shown(vertical) && !!vertical.querySelector("#tabbrowser-tabs"),
  sidebarButtonShown: shown(document.getElementById("sidebar-button")),
  count: gBrowser.tabs.length,
  pinned: gBrowser.pinnedTabCount,
};
"""

ADD_TAB_JS = r"""
const [pin] = args;
const tab = gBrowser.addTrustedTab("about:blank");
if (pin) {
  gBrowser.pinTab(tab);
}
await new Promise(res => setTimeout(res, 500));
return gBrowser.tabs.length;
"""

MENU_JS = r"""
const [x, y] = args;
const sleep = ms => new Promise(res => setTimeout(res, ms));
// Right-click the empty part of the vertical tab strip, as a user would.
let opened = null;
const shown = new Promise(res => document.addEventListener("popupshown", e => {
  if (e.target.localName == "menupopup") { opened = e.target; res(); }
}, { once: true }));
window.synthesizeMouseEvent("contextmenu", x, y, { button: 2 });
await Promise.race([shown, sleep(3000)]);
if (!opened) {
  return { state: "closed", menu: null, items: [] };
}
await sleep(300);
const items = [...opened.querySelectorAll("menuitem.whisper-sidebar-mode-item")]
  .map(i => ({ label: i.getAttribute("label"), hidden: i.hidden, checked: i.getAttribute("checked") == "true",
               mode: i.dataset.whisperSidebarMode }));
return { state: opened.state, menu: opened.id, items };
"""

BAR_JS = r"""
// The sidebar's bottom bar (WhisperSidebar.sys.mjs) and Firefox's own gear in sidebar-main.
const bar = document.getElementById("whisper-sidebar-bar");
const main = document.querySelector("sidebar-main");
const gear = main?.shadowRoot?.querySelector('moz-button[view="viewCustomizeSidebar"]');
const shown = e => !!e && e.getBoundingClientRect().width > 0;
return {
  shown: shown(bar),
  buttons: [...(bar?.querySelectorAll(".whisper-sidebar-bar-button") ?? [])]
    .filter(shown).map(b => b.className.match(/whisper-sidebar-(\w+)$/)?.[1]),
  firefoxGearShown: shown(gear),
};
"""

TAB_ICONS_JS = r"""
// Tab rows and icons in the sidebar (cast.css, WhisperTabIcons.sys.mjs): rows 30 px
// tall and a few pixels apart, a monogram for a page without an icon (a tab that is
// not loaded, so nothing is looked up), Firefox's globe for an address without a
// domain, and a ring while a page loads, never a dot.
const sleep = ms => new Promise(res => setTimeout(res, ms));
const { monogramFor } = ChromeUtils.importESModule("moz-src:///browser/components/whisper/WhisperTabIcons.sys.mjs");
const lazyTab = gBrowser.addTrustedTab("https://en.wikipedia.org/wiki/Web_browser", { createLazyBrowser: true });
const ipTab = gBrowser.addTrustedTab("http://192.0.2.1/", { createLazyBrowser: true });
await sleep(800);
const rows = gBrowser.visibleTabs.filter(t => !t.pinned).map(t => t.querySelector(".tab-background").getBoundingClientRect());
const pitches = rows.slice(1).map((r, i) => Math.round(r.top - rows[i].top));
const stack = lazyTab.querySelector(".tab-icon-stack");
const mono = getComputedStyle(stack, "::before");
const out = {
  rowHeights: [...new Set(gBrowser.visibleTabs.filter(t => !t.pinned).map(t => Math.round(t.querySelector(".tab-background").getBoundingClientRect().height)))],
  pitches: [...new Set(pitches)],
  monogram: lazyTab.getAttribute("whisper-mono"),
  monogramContent: mono.content,
  monogramSize: [mono.width, mono.height],
  iconHidden: getComputedStyle(lazyTab.querySelector(".tab-icon-image")).visibility == "hidden",
  ipMonogram: ipTab.getAttribute("whisper-mono"),
  ipIcon: getComputedStyle(ipTab.querySelector(".tab-icon-image")).content,
  helper: [monogramFor("https://news.ycombinator.com/"), monogramFor("http://127.0.0.1:8080/"), monogramFor("about:blank")],
};
lazyTab.setAttribute("busy", "true");
await sleep(200);
const ring = getComputedStyle(lazyTab.querySelector(".tab-throbber"), "::before");
out.loading = { background: ring.backgroundImage, radius: ring.borderTopLeftRadius, animation: ring.animationName };
lazyTab.removeAttribute("busy");
gBrowser.removeTab(lazyTab);
gBrowser.removeTab(ipTab);
return out;
"""

SEARCH_JS = r"""
// The search engines and their addresses as Whisper builds them (patches/0033, 0036).
const { SearchService } = ChromeUtils.importESModule("moz-src:///toolkit/components/search/SearchService.sys.mjs");
await SearchService.init();
const submission = id => SearchService.getEngineById(id)?.getSubmission("whisper test")?.uri.spec ?? null;
return {
  ddg: submission("ddg"),
  google: submission("google"),
  bing: submission("bing"),
  defaultEngine: SearchService.defaultEngine?.id ?? null,
  appDefaultEngine: SearchService.appDefaultEngine?.id ?? null,
  engines: SearchService.getVisibleEngines ? (await SearchService.getVisibleEngines()).map(e => e.id) : null,
};
"""

TYPING_JS = r"""
// Typing in the address bar with the default settings: which web requests start.
const sleep = ms => new Promise(res => setTimeout(res, ms));
const seen = [];
const observer = {
  observe(subject) {
    try {
      seen.push(subject.QueryInterface(Ci.nsIChannel).URI.spec);
    } catch (e) {}
  },
};
Services.obs.addObserver(observer, "http-on-opening-request");
try {
  gURLBar.focus();
  for (const text of ["b", "bv", "bvg", "bvgsdf", "bvgsdf whisper"]) {
    gURLBar.search(text);
    await sleep(400);
  }
  await sleep(2500);
} finally {
  Services.obs.removeObserver(observer, "http-on-opening-request");
  gURLBar.view.close();
  gURLBar.handleRevert();
  gBrowser.selectedBrowser.focus();
}
return {
  suggestEnabled: Services.prefs.getBoolPref("browser.search.suggest.enabled"),
  requests: seen,
};
"""

DOWNLOAD_JS = r"""
// Downloads a file the way a click on a link does and watches the toolbar's
// downloads button: it must appear, play Firefox's "download started" animation and
// list the finished download in its panel.
const [url] = args;
const sleep = ms => new Promise(res => setTimeout(res, ms));
const { Downloads } = ChromeUtils.importESModule("resource://gre/modules/Downloads.sys.mjs");
const list = await Downloads.getList(Downloads.ALL);
const before = (await list.getAll()).length;
const button = document.getElementById("downloads-button");
const shown = e => !!e && e.getBoundingClientRect().width > 0 && getComputedStyle(e).visibility != "hidden";
const result = { placed: !!button, shownBefore: shown(button), notifications: [] };
const observer = new MutationObserver(() => {
  const n = button.getAttribute("notification");
  if (n && !result.notifications.includes(n)) {
    result.notifications.push(n);
  }
});
if (button) {
  observer.observe(button, { attributes: true, attributeFilter: ["notification", "animate"] });
}
gBrowser.selectedBrowser.fixupAndLoadURIString(url, {
  triggeringPrincipal: Services.scriptSecurityManager.getSystemPrincipal(),
});
let download = null;
for (let i = 0; i < 60; i++) {
  await sleep(250);
  const all = await list.getAll();
  download = all.length > before ? all.at(-1) : null;
  if (download?.succeeded) {
    break;
  }
}
await sleep(1000);
observer.disconnect();
result.shownAfter = shown(button);
result.succeeded = !!download?.succeeded;
result.target = download?.target?.path?.split(/[\\/]/).pop() ?? null;
result.panelOpened = document.getElementById("downloadsPanel")?.state == "open";
if (!result.panelOpened) {
  // Firefox opens the panel by itself only when the window has the focus.
  DownloadsPanel.showPanel();
  for (let i = 0; i < 20 && document.getElementById("downloadsPanel")?.state != "open"; i++) {
    await sleep(100);
  }
  await sleep(500);
}
result.panelItems = [...document.querySelectorAll("#downloadsListBox richlistitem")]
  .map(i => i.querySelector(".downloadTarget")?.getAttribute("value") ?? i.textContent.trim());
result.reducedMotion = matchMedia("(prefers-reduced-motion)").matches;
return result;
"""

CONSOLE_JS = r"""
// Errors and warnings in the browser console, for diagnosis.
return Services.console.getMessageArray()
  .filter(m => m instanceof Ci.nsIScriptError && !(m.flags & Ci.nsIScriptError.infoFlag))
  .map(m => `${m.flags & Ci.nsIScriptError.warningFlag ? "warning" : "error"}: ${m.errorMessage} (${m.sourceName}:${m.lineNumber})`)
  .slice(-200);
"""

PAGE_JS = r"""
// The first tab's page, seen from inside it.
return {
  url: document.URL,
  readyState: document.readyState,
  title: document.title,
  bodyElements: document.body ? document.body.querySelectorAll("*").length : -1,
  hasSearch: !!document.querySelector("input, [role=search], .search-wrapper, content-search-handoff-ui"),
  hasLogo: !!document.querySelector(".logo-and-wordmark, .logo, .wordmark"),
  background: document.body ? getComputedStyle(document.body).backgroundColor : null,
};
"""

ABOUT_JS = r"""
// Help > About Whisper: Whisper's version, and the Firefox version it is based on.
const sleep = ms => new Promise(res => setTimeout(res, ms));
window.openAboutDialog();
let win = null;
for (let i = 0; i < 50 && !win; i++) {
  await sleep(200);
  win = Services.wm.getMostRecentWindow("Browser:About");
}
if (!win) {
  return { error: "the about dialog did not open" };
}
if (win.document.readyState != "complete") {
  await new Promise(res => win.addEventListener("load", res, { once: true }));
}
await win.document.l10n.ready;
await sleep(1500);
const doc = win.document;
// Without Fluent's bidi isolation marks, if any.
const text = id => doc.getElementById(id)?.textContent?.replace(/[\u2068\u2069]/g, "").trim() ?? null;
const shown = id => {
  const e = doc.getElementById(id);
  return !!e && e.getBoundingClientRect().width > 0;
};
return {
  title: doc.title,
  version: text("version"),
  basedOn: text("whisperBasedOn"),
  notMozilla: text("whisperNotMozilla"),
  notMozillaShown: shown("whisperNotMozilla"),
  helpLinkShown: shown("aboutDialogHelpLink"),
  feedbackLinkShown: shown("submit-feedback"),
};
"""

CLOSE_ABOUT_JS = r"""
Services.wm.getMostRecentWindow("Browser:About")?.close();
return true;
"""

UPDATE_TIMER_JS = r"""
return Services.prefs.getIntPref("app.update.lastUpdateTime.background-update-timer", 0);
"""

SHORTCUT_JS = r"""
const sleep = ms => new Promise(res => setTimeout(res, ms));
for (const m of document.querySelectorAll("menupopup[open], menupopup")) {
  if (m.state == "open") m.hidePopup();
}
const before = Services.prefs.getStringPref("whisper.sidebar.mode", "");
document.getElementById("key_whisperSidebarMode").doCommand();
await sleep(1500);
const after = Services.prefs.getStringPref("whisper.sidebar.mode", "");
return { before, after };
"""


# Browser files a web page might try to load to tell Whisper from Firefox, and what it
# must see: Whisper's own files fail as files that do not exist do (they are in a package
# pages cannot load, chrome://whisper/), Firefox's branding files load with Firefox's
# sizes, and the about:crashes files Firefox has load too.
# (url, how the page loads it, expected result: "error" or the expected fields)
PROBES = [
    ("chrome://whisper/content/cast.css", "style", "error"),
    ("chrome://whisper/content/aboutDialog.css", "style", "error"),
    ("chrome://whisper/content/fonts/Inter-Regular.woff2", "font", "error"),
    ("chrome://browser/content/whisper/cast.css", "style", "error"),
    ("chrome://browser/content/whisper/fonts/Inter-Regular.woff2", "font", "error"),
    ("moz-src:///browser/components/whisper/WhisperUI.sys.mjs", "script", "error"),
    ("chrome://branding/content/about-logo.svg", "image", {"state": "loaded", "w": 512, "h": 512}),
    ("chrome://branding/content/about-logo.png", "image", {"state": "loaded", "w": 192, "h": 192}),
    # No width or height: 300 px wide, as tall as Firefox's aspect ratio (359:80) makes it.
    ("chrome://branding/content/about-wordmark.svg", "image", {"state": "loaded", "w": 300, "h": 67}),
    ("chrome://global/content/crashes.css", "style", {"state": "loaded"}),
]


def probe_mismatches(results):
    """The probes whose result is not what a page sees in Firefox."""
    bad = {}
    for url, _kind, want in PROBES:
        got = results.get(url, {})
        if want == "error":
            ok = got.get("state") == "error"
        else:
            ok = got.get("state") == want["state"]
            for k in ("w", "h"):
                if k in want:
                    ok = ok and got.get(k) == want[k]
        if not ok:
            bad[url] = got
    return bad


class LocalSite:
    """Test pages served on 127.0.0.1 (the test's network namespace has loopback only),
    so page-level checks run without any network."""

    PAGES = {
        "page.html": "<!doctype html><title>Whisper test page</title><h1>Whisper test page</h1>"
                     "<p id=loaded>loaded</p>",
        # A web page probing for browser files (see PROBES): what it can load and measure.
        "probe.html": "<!doctype html><title>probe</title><pre id=result>pending</pre><script>"
                      "const probes = PROBES_JSON;"
                      "const out = {};"
                      "const one = ([url, kind]) => new Promise(done => {"
                      "  const finish = r => { out[url] = r; done(); };"
                      "  setTimeout(() => finish({ state: 'timeout' }), 5000);"
                      "  if (kind == 'image') {"
                      "    const i = new Image();"
                      "    i.onload = () => finish({ state: 'loaded', w: i.naturalWidth, h: i.naturalHeight });"
                      "    i.onerror = () => finish({ state: 'error' }); i.src = url;"
                      "  } else if (kind == 'style') {"
                      "    const l = document.createElement('link'); l.rel = 'stylesheet';"
                      "    l.onload = () => finish({ state: 'loaded' }); l.onerror = () => finish({ state: 'error' });"
                      "    l.href = url; document.head.append(l);"
                      "  } else if (kind == 'script') {"
                      "    const sc = document.createElement('script');"
                      "    sc.onload = () => finish({ state: 'loaded' }); sc.onerror = () => finish({ state: 'error' });"
                      "    sc.src = url; document.head.append(sc);"
                      "  } else if (kind == 'font') {"
                      "    new FontFace('probe', `url(${url})`).load().then(() => finish({ state: 'loaded' }),"
                      "      () => finish({ state: 'error' }));"
                      "  }"
                      "});"
                      "Promise.all(probes.map(one)).then(() => {"
                      "  document.getElementById('result').textContent = JSON.stringify(out); });"
                      "</script>",
    }

    def __init__(self):
        self.root = tempfile.mkdtemp(prefix="whisper-site-")
        for name, body in self.PAGES.items():
            body = body.replace("PROBES_JSON", json.dumps([[u, k] for u, k, _ in PROBES]))
            with open(os.path.join(self.root, name), "w", encoding="utf-8") as f:
                f.write(body)
        # A file the browser downloads instead of showing (application/octet-stream).
        with open(os.path.join(self.root, "whisper-download-test.bin"), "wb") as f:
            f.write(os.urandom(256 * 1024))
        handler = functools.partial(_QuietHandler, directory=self.root)
        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()
        self.base = f"http://127.0.0.1:{self.httpd.server_address[1]}"

    def url(self, name):
        return f"{self.base}/{name}"


class _QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


class Pointer:
    """The real X pointer (xdotool) when it is installed, so the checks see what a mouse
    does, including leaving the window; otherwise events synthesized in the window."""

    def __init__(self, m):
        self.m = m
        self.xdotool = shutil.which("xdotool") if os.environ.get("DISPLAY") else None

    def move(self, x, y):
        """x, y: screen pixels (the window is placed at a known spot)."""
        if self.xdotool:
            subprocess.run([self.xdotool, "mousemove", str(x), str(y)], check=False, timeout=10)
            time.sleep(0.15)
            subprocess.run([self.xdotool, "mousemove", str(x), str(y + 6)], check=False, timeout=10)
        else:
            self.m.run(SYNTH_MOVE_JS, x, y)


def capture(m, path, report):
    """The whole X screen when ImageMagick is there (shows the page content too),
    otherwise Marionette's capture of the browser window."""
    if shutil.which("import") and os.environ.get("DISPLAY"):
        r = subprocess.run(["import", "-window", "root", path], capture_output=True, timeout=30)
        if r.returncode == 0:
            return
    try:
        m.screenshot(path)
    except MarionetteError as e:
        report.facts.setdefault("screenshot_errors", []).append(str(e))


def headless_profile_check(app, out, report):
    binary = os.path.join(app, "whisper")
    home = tempfile.mkdtemp(prefix="whisper-home-")
    env = dict(os.environ, HOME=home, XDG_CONFIG_HOME="", XDG_CACHE_HOME="", MOZ_HEADLESS="1")
    env.pop("XDG_CONFIG_HOME")
    env.pop("XDG_CACHE_HOME")
    shot = os.path.join(out, "headless-logo.png")
    with open(os.path.join(out, "headless.log"), "w") as hlog:
        proc = subprocess.Popen([binary, "--headless", "--screenshot", shot, "about:logo"],
                                env=env, stdout=hlog, stderr=subprocess.STDOUT)
        try:
            proc.wait(timeout=90)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            report.check("headless start finishes", False, "timed out after 90 s, see headless.log")
            return
    r = proc
    report.facts["headless_exit"] = r.returncode
    found = []
    for root, dirs, files in os.walk(home):
        if "profiles.ini" in files:
            found.append(os.path.relpath(root, home))
    report.facts["profile_roots"] = found
    report.check("headless start exits cleanly", r.returncode == 0, f"exit {r.returncode}")
    report.check("profile lives in Whisper's own folder",
                 bool(found) and all("whisper" in f.lower() for f in found), found)
    report.check("nothing is written under ~/.mozilla",
                 not os.path.exists(os.path.join(home, ".mozilla")))
    report.check("headless screenshot written", os.path.isfile(shot))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", required=True)
    ap.add_argument("--prefs", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--version", default="")
    # The checks take about 20 s; the browser then stays open until this many seconds
    # have passed, so the background work Firefox schedules for a minute or two after
    # start (media plugin checks, add-on update checks) also shows up in the host list.
    ap.add_argument("--seconds", type=float, default=150)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    report = Report()
    binary = os.path.join(a.app, "whisper")

    v = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=60)
    report.facts["version_output"] = v.stdout.strip()
    report.check("whisper --version names Whisper", v.stdout.strip().startswith("Whisper "), v.stdout.strip())

    headless_profile_check(a.app, a.out, report)

    profile = tempfile.mkdtemp(prefix="whisper-profile-")
    # Marionette's "recommended" prefs would hide first-run behaviour (welcome pages and
    # so on) that this test wants to see as a user would.
    with open(os.path.join(profile, "user.js"), "w") as f:
        f.write('user_pref("remote.prefs.recommended", false);\n')
        # The import offer on a first start (WhisperImport.sys.mjs) is tested on its own
        # (import_test.py); here it must not open a window over the checks.
        f.write('user_pref("whisper.import.offerOnFirstRun", false);\n')
    log = open(os.path.join(a.out, "browser.log"), "w")
    # MOZ_REMOTE_ALLOW_SYSTEM_ACCESS: Marionette may run scripts in the browser's own
    # (chrome) context only when this is set; the test reads prefs and state there.
    env = dict(os.environ, MOZ_LOG="nsHostResolver:5,timestamp",
               MOZ_LOG_FILE=os.path.join(a.out, "mozlog"), MOZ_REMOTE_ALLOW_SYSTEM_ACCESS="1")
    started = time.monotonic()
    proc = subprocess.Popen([binary, "--marionette", "--no-remote", "--profile", profile],
                            stdout=log, stderr=subprocess.STDOUT, env=env)
    expected = parse_prefs(a.prefs)
    try:
        m = Marionette(alive=lambda: proc.poll() is None)
        session = m.start_session()
        report.facts["session"] = {k: session.get("capabilities", {}).get(k) for k in
                                   ("browserName", "browserVersion", "moz:profile")}
        m.command("WebDriver:SetWindowRect", {"x": 0, "y": 0, "width": 1440, "height": 900})
        time.sleep(6)  # first start: distribution add-on install, Whisper's one-time setup
        r = m.run(CHECK_JS, expected)
        report.facts["state"] = r

        report.check("application name is Whisper", r["appinfo"]["name"] == "Whisper", r["appinfo"])
        report.check("vendor is Whisper", r["appinfo"]["vendor"] == "Whisper")
        if a.version:
            report.check(f"version is {a.version}", r["appinfo"]["version"] == a.version, r["appinfo"]["version"])
        report.check("window title says Whisper", "Whisper" in r["title"] and "Firefox" not in r["title"], r["title"])
        report.check("brand name is Whisper", r["brandProperties"] == "Whisper", r["brandProperties"])
        report.check("about dialog title is 'About Whisper'", r["aboutTitle"] == "About Whisper", r["aboutTitle"])
        ua = r["userAgent"]
        report.check("User-Agent is Firefox's", re.search(r"Gecko/20100101 Firefox/\d+\.0$", ua) is not None
                     and "Whisper" not in ua, ua)
        k = r["constants"]
        for name, want in [("MOZILLA_OFFICIAL", True), ("MOZ_TELEMETRY_REPORTING", False),
                           ("MOZ_DATA_REPORTING", False), ("MOZ_CRASHREPORTER", False),
                           ("MOZ_NORMANDY", False), ("MOZ_UPDATER", True),
                           ("MOZ_MAINTENANCE_SERVICE", False), ("MOZ_UPDATE_AGENT", False),
                           ("MOZ_BITS_DOWNLOAD", False),
                           ("MOZ_SERVICES_HEALTHREPORT", False), ("MOZ_REQUIRE_SIGNING", True)]:
            report.check(f"build flag {name} is {want}", bool(k.get(name)) == want, k.get(name))
        report.check("app name is whisper", k.get("MOZ_APP_NAME") == "whisper", k.get("MOZ_APP_NAME"))
        release = read_env("release.env")
        u = r["update"]
        report.check(f"update channel is {release['UPDATE_CHANNEL']}", u["channel"] == release["UPDATE_CHANNEL"],
                     u["channel"])
        want_url = f"{release['UPDATE_BASE_URL']}/update/1/{release['UPDATE_CHANNEL']}/Linux_x86_64-gcc3/update.xml"
        report.check("updates come from Whisper's update host", u["url"] == want_url, u["url"])
        report.check("updates are on and can be applied (the folder is writable)",
                     not u["disabled"] and u["canCheck"] and u["canApply"], u)
        report.check(f"Whisper's version is {release['WHISPER_VERSION']} (locked)",
                     u["whisperVersion"] == release["WHISPER_VERSION"] and u["whisperVersionLocked"], u)
        report.check("Telemetry does not record", not r["telemetry"]["canRecordBase"]
                     and not r["telemetry"]["canRecordExtended"], r["telemetry"])

        bad = [p for p in r["prefs"] if p["actual"] != p["expected"] or p["locked"] != p["expectLocked"]]
        report.check(f"all {len(r['prefs'])} prefs of 0-whisper.js are built in "
                     f"({sum(p['expectLocked'] for p in r['prefs'])} locked)", not bad,
                     json.dumps(bad)[:1500] if bad else "")

        u = r.get("ubo")
        report.check("uBlock Origin is installed", u is not None)
        if u:
            report.check("uBlock Origin is enabled", u["isActive"] and not u["userDisabled"], u)
            report.check("uBlock Origin came from the distribution folder", u["installSource"] == "distribution",
                         u["installSource"])
            report.check("uBlock Origin is signed", u["signedState"] is not None and u["signedState"] > 0,
                         u["signedState"])
            report.check("uBlock Origin may run in private windows", u["privateBrowsing"])
            report.check("uBlock Origin button is in the toolbar",
                         (u["button"] or {}).get("area") == "nav-bar", u["button"])
        tabs = r["tabs"]
        report.check("no welcome, terms or privacy notice tab",
                     not any(re.search(r"welcome|privacy|terms|firstrun|mozilla\.(org|com)", t) for t in tabs), tabs)
        report.check("Cast stylesheet is loaded", r["cast"]["wBlack"] == "#111316", r["cast"])
        report.check("vertical tab rows are 30 px high", r["cast"]["tabMinHeight"] == "30px", r["cast"]["tabMinHeight"])
        s = r["sidebar"]
        report.check("sidebar starts in expanded mode", s["mode"] == "expanded" and s["expanded"]
                     and s["visibility"] == "always-show" and s["verticalTabs"], s)
        bar = m.run(BAR_JS)
        report.facts["sidebar_bar"] = bar
        report.check("the sidebar has one bottom bar with one settings entry (downloads, bookmarks, history, "
                     "passwords, settings) and no second settings gear",
                     bar["buttons"] == ["downloads", "bookmarks", "history", "passwords", "settings"]
                     and bar["shown"] and not bar["firefoxGearShown"], bar)
        icons = m.run(TAB_ICONS_JS)
        report.facts["tab_icons"] = icons
        report.check("sidebar tab rows are 30 px tall and 2 to 4 px apart",
                     icons["rowHeights"] == [30] and all(32 <= p <= 34 for p in icons["pitches"]), icons)
        report.check("a page without an icon shows its domain's monogram (W for wikipedia.org); "
                     "an IP address keeps Firefox's globe",
                     icons["monogram"] == "W" and icons["monogramContent"] == '"W"' and icons["iconHidden"]
                     and icons["ipMonogram"] is None and "defaultFavicon" in (icons["ipIcon"] or "")
                     and icons["helper"][0]["letter"] == "Y" and icons["helper"][1] is None
                     and icons["helper"][2] is None, icons)
        report.check("a loading tab shows a ring, not Firefox's sliding dot",
                     icons["loading"]["background"] == "none" and icons["loading"]["radius"] == "50%"
                     and icons["loading"]["animation"] == "whisper-tab-loading", icons["loading"])
        report.check("Ctrl+Alt+S shortcut is registered",
                     r["shortcutKey"] == {"key": "S", "modifiers": "accel,alt"}, r["shortcutKey"])

        capture(m, os.path.join(a.out, "1-expanded.png"), report)
        # What the start page looks like from inside, and what the console says.
        try:
            m.command("Marionette:SetContext", {"value": "content"})
            page = m.run(PAGE_JS)
            m.screenshot(os.path.join(a.out, "1b-start-page.png"))
        except MarionetteError as e:
            page = {"error": str(e)}
        finally:
            m.command("Marionette:SetContext", {"value": "chrome"})
        report.facts["start_page"] = page
        report.check("the start page renders (search box and Whisper logo)",
                     isinstance(page, dict) and page.get("hasSearch") and page.get("hasLogo"), page)
        search = m.run(SEARCH_JS)
        report.facts["search"] = search
        codes = re.compile(r"[?&](t|client|pc|form|sourceid|campid)=|firefox|mozilla|ffab", re.I)
        report.check("search addresses carry no Mozilla partner or attribution codes (DuckDuckGo, Google, Bing)",
                     all(search.get(e) and "whisper" in search[e] and not codes.search(search[e].split("?", 1)[1])
                         for e in ("ddg", "google", "bing")),
                     {e: search.get(e) for e in ("ddg", "google", "bing")})
        engines = search.get("engines") or []
        report.check("built-in search engines are DuckDuckGo (the default), Google, Bing and Wikipedia",
                     search.get("appDefaultEngine") == "ddg" and search.get("defaultEngine") == "ddg"
                     and engines[:3] == ["ddg", "google", "bing"] and len(engines) == 4
                     and engines[3].startswith("wikipedia"),
                     {k: search.get(k) for k in ("defaultEngine", "appDefaultEngine", "engines")})
        typing = m.run(TYPING_JS)
        report.facts["typing"] = typing
        # Background work (uBlock Origin's lists, Remote Settings) may run meanwhile; what
        # matters is that nothing goes to a search engine or carries what was typed.
        typed = [u for u in typing["requests"]
                 if "bvgsdf" in u or re.search(r"duckduckgo\.com|google\.|bing\.com", u)]
        report.check("typing in the address bar sends nothing to a search engine (suggestions are off)",
                     not typing["suggestEnabled"] and not typed, typed or typing)
        menu = m.run(MENU_JS, 100, 450)
        report.facts["menu"] = menu
        labels = [i["label"] for i in menu["items"]]
        report.check("right-click on the tab sidebar offers the three modes",
                     menu["menu"] == "toolbar-context-menu" and len(menu["items"]) == 3 and all(labels)
                     and not any(i["hidden"] for i in menu["items"])
                     and [i["mode"] for i in menu["items"] if i["checked"]] == ["expanded"], menu)
        capture(m, os.path.join(a.out, "2-menu.png"), report)
        about = m.run(ABOUT_JS)
        report.facts["about"] = about
        capture(m, os.path.join(a.out, "2b-about.png"), report)
        m.run(CLOSE_ABOUT_JS)
        whisper_line = f"Whisper {release['WHISPER_VERSION']} (64-bit)"
        report.check(f"about dialog says '{whisper_line}'", about.get("version") == whisper_line, about)
        if a.version:
            report.check(f"about dialog says 'Based on Firefox {a.version}'",
                         about.get("basedOn") == f"Based on Firefox {a.version}", about.get("basedOn"))
        report.check("about dialog says Whisper is not affiliated with or endorsed by Mozilla",
                     about.get("notMozillaShown") and "not affiliated with or endorsed by Mozilla" in
                     (about.get("notMozilla") or "") and "Firefox is a trademark of the Mozilla Foundation"
                     in (about.get("notMozilla") or ""), about.get("notMozilla"))
        report.check("about dialog has no Mozilla help or feedback links",
                     not about.get("helpLinkShown") and not about.get("feedbackLinkShown"), about)
        sc = m.run(SHORTCUT_JS)
        report.check("the shortcut switches the mode", sc["before"] == "expanded" and sc["after"] == "compact", sc)

        # The toolbar: one slim row with reload after back and forward, set once per
        # profile (WhisperToolbar.sys.mjs); 0.1.0 had lost the reload button.
        tb = m.run(TOOLBAR_JS)
        report.facts["toolbar"] = tb
        place = tb["placements"]
        report.check("toolbar: back, forward and reload come first, then the address bar",
                     [i for i in place if i in ("back-button", "forward-button", "stop-reload-button",
                                                "urlbar-container")]
                     == ["back-button", "forward-button", "stop-reload-button", "urlbar-container"]
                     and place.index("stop-reload-button") < place.index("urlbar-container"), place)
        report.check("toolbar: no home, Firefox View, account, list-all-tabs or new tab buttons",
                     not set(tb["shown"]) & {"home-button", "firefox-view-button", "fxa-toolbar-menu-button",
                                             "alltabs-button", "new-tab-button", "sidebar-button"}, tb["shown"])
        report.check("toolbar: the layout is set once (whisper.toolbar.layoutVersion)", tb["layoutVersion"] >= 1,
                     tb["layoutVersion"])

        site = LocalSite()
        page = site.url("probe.html")
        m.run(LOAD_JS, page)
        try:
            m.command("Marionette:SetContext", {"value": "content"})
            probed = None
            for _ in range(30):
                text = m.run('return document.getElementById("result")?.textContent;')
                if text and text != "pending":
                    probed = json.loads(text)
                    break
                time.sleep(0.5)
        finally:
            m.command("Marionette:SetContext", {"value": "chrome"})
        report.facts["web_probes"] = probed
        bad = probe_mismatches(probed or {})
        report.check("a web page cannot load Whisper's own files and sees Firefox's branding sizes",
                     probed is not None and not bad, bad or probed)

        page = site.url("page.html")
        report.check("a local test page loads", m.run(LOAD_JS, page) == page, page)

        pointer = Pointer(m)
        report.facts["pointer"] = "xdotool" if pointer.xdotool else "synthesized"
        widths = {}
        for i, mode in enumerate(["compact", "autohide", "expanded"], start=3):
            st = m.run(MODE_JS, mode)
            report.facts[f"mode_{mode}"] = st
            widths[mode] = st["width"]
            capture(m, os.path.join(a.out, f"{i}-{mode}.png"), report)
            rl = m.run(RELOAD_JS)
            report.check(f"{mode}: the reload button shows and reloads the page",
                         rl.get("shown") and rl.get("reloaded"), rl)
            if mode == "autohide":
                report.check("auto-hide: sidebar collapses to an edge strip",
                             st["visibility"] == "expand-on-hover" and not st["expanded"] and st["width"] <= 16, st)
                # The window 100 px from the screen's left edge, as a window that is not
                # maximized: the pointer has to stop in the strip at its edge.
                m.command("WebDriver:SetWindowRect", {"x": 100, "y": 0, "width": 1340, "height": 900})
                time.sleep(1.5)
                for name, x in (("touching the left edge", 102), ("near the left edge", 112)):
                    pointer.move(800, 400)
                    m.run(SIDEBAR_STATE_JS)
                    pointer.move(x, 400)
                    hover = m.run(SIDEBAR_STATE_JS)
                    report.facts[f"autohide_{name}"] = hover
                    if name == "touching the left edge":
                        capture(m, os.path.join(a.out, "5b-autohide-revealed.png"), report)
                    report.check(f"auto-hide: {name} slides the sidebar in over the page",
                                 hover["expanded"] and hover["width"] > 150 and hover["tabboxLeft"] == st["tabboxLeft"],
                                 hover)
                    pointer.move(800, 400)
                    away = m.run(SIDEBAR_STATE_JS)
                    report.check(f"auto-hide: after {name}, it hides again when the pointer leaves",
                                 not away["expanded"] and away["width"] <= 16, away)
                m.command("WebDriver:SetWindowRect", {"x": 0, "y": 0, "width": 1440, "height": 900})
                time.sleep(1)
        report.check("compact mode is a narrow icon strip (at most 60 px)", 36 <= widths["compact"] <= 60, widths)
        report.check("expanded mode is wider than compact and at most 240 px",
                     widths["compact"] < widths["expanded"] <= 240, widths)

        # Downloads: 0.1.0's toolbar had no downloads button, so a download showed nothing.
        try:
            dl = m.run(DOWNLOAD_JS, site.url("whisper-download-test.bin"))
        except MarionetteError as e:
            dl = {"error": str(e)[:500]}
        capture(m, os.path.join(a.out, "5c-download.png"), report)
        m.run('document.getElementById("downloadsPanel")?.hidePopup?.(); return 1;')
        report.facts["download"] = dl
        report.check("a download shows the downloads button, its start animation and the finished file in the panel",
                     dl.get("placed") and dl.get("shownAfter") and dl.get("succeeded")
                     and ("start" in dl.get("notifications", []) or dl.get("reducedMotion"))
                     and any("whisper-download-test" in i for i in dl.get("panelItems", [])), dl)

        # Horizontal tabs on Firefox's own tab strip, and back, keeping every tab.
        m.run(ADD_TAB_JS, True)
        before = m.run(TABS_JS, None)
        horizontal = m.run(TABS_JS, "horizontal")
        report.facts["tabs_horizontal"] = horizontal
        capture(m, os.path.join(a.out, "6-horizontal.png"), report)
        report.check("horizontal tabs: the tab strip shows on top and the sidebar tab list is gone",
                     horizontal["rootTabs"] == "horizontal" and horizontal["tabstripShown"]
                     and horizontal["tabsInTabstrip"] and not horizontal["verticalTabListShown"], horizontal)
        report.check("horizontal tabs: the sidebar button shows (bookmarks, history, passwords)",
                     horizontal["sidebarButtonShown"], horizontal)
        rl = m.run(RELOAD_JS)
        report.check("horizontal: the reload button shows and reloads the page", rl.get("shown") and rl.get("reloaded"),
                     rl)
        m.run(ADD_TAB_JS, False)
        vertical = m.run(TABS_JS, "vertical")
        report.facts["tabs_vertical_again"] = vertical
        report.check("back to vertical tabs: every tab and pinned tab is still there, the mode is kept",
                     vertical["rootTabs"] == "vertical" and vertical["verticalTabListShown"]
                     and vertical["count"] == before["count"] + 1 and vertical["pinned"] == before["pinned"]
                     and vertical["sidebarMode"] == "expanded", {"before": before, "after": vertical})

        try:
            with open(os.path.join(a.out, "console.txt"), "w") as f:
                f.write("\n".join(m.run(CONSOLE_JS)) + "\n")
        except MarionetteError as e:
            report.facts["console_error"] = str(e)
        elapsed = time.monotonic() - started
        if elapsed < a.seconds:
            time.sleep(a.seconds - elapsed)
        last_check = m.run(UPDATE_TIMER_JS)
        report.facts["update_timer_last_fired"] = last_check
        report.check("the browser checked for updates on its own", last_check > 0, last_check)
        try:
            m.quit()
        except MarionetteError as e:
            report.facts["quit_error"] = str(e)
    except MarionetteError as e:
        report.check("Marionette session", False, e)
    try:
        code = proc.wait(timeout=90)
    except subprocess.TimeoutExpired:
        proc.kill()
        code = "killed after timeout"
    report.facts["seconds_running"] = round(time.monotonic() - started, 1)
    report.check("browser exits cleanly", code == 0, f"exit {code}")
    log.close()
    blog = open(os.path.join(a.out, "browser.log"), errors="replace").read()
    report.check("content sandbox can use user namespaces (no CanCreateUserNamespace error)",
                 "CanCreateUserNamespace" not in blog)

    # The profile on disk
    ext = os.path.join(profile, "extensions.json")
    if os.path.isfile(ext):
        addons = json.load(open(ext)).get("addons", [])
        ubo = [x for x in addons if x.get("id") == UBO_ID]
        report.check("extensions.json: uBlock Origin active",
                     bool(ubo) and ubo[0].get("active") and ubo[0].get("location") == "app-profile",
                     {k: ubo[0].get(k) for k in ("version", "active", "location", "signedState")} if ubo else "missing")
    else:
        report.check("extensions.json written", False)
    ep = os.path.join(profile, "extension-preferences.json")
    perms = json.load(open(ep)).get(UBO_ID, {}) if os.path.isfile(ep) else {}
    report.check("extension-preferences.json: private windows allowed for uBlock Origin",
                 "internal:privateBrowsingAllowed" in perms.get("permissions", []), perms.get("permissions"))
    prefs_js = open(os.path.join(profile, "prefs.js")).read() if os.path.isfile(os.path.join(profile, "prefs.js")) else ""
    # Only Firefox's fixed placeholder ("canary") IDs may appear (patch 0008), never
    # random ones, neither in prefs.js nor in datareporting/state.json.
    canary = {"c0ffeec0-ffee-c0ff-eec0-ffeec0ffeec0", "decafdec-afde-cafd-ecaf-decafdecafde",
              "beefbeef-beef-beef-beef-beeefbeefbee", "b0bacafe-b0ba-cafe-b0ba-cafeb0bacafe"}
    ids = dict(re.findall(r'user_pref\("((?:toolkit\.telemetry|datareporting\.dau)\.cached[A-Za-z]*ID)", "([^"]*)"\)',
                          prefs_js))
    state_file = os.path.join(profile, "datareporting", "state.json")
    if os.path.isfile(state_file):
        state = json.load(open(state_file))
        ids.update({f"state.json:{k}": v for k, v in state.items() if k.lower().endswith("id")})
    report.facts["telemetry_ids"] = ids
    report.check("no random Telemetry IDs (prefs.js, datareporting/state.json)",
                 all(v in canary for v in ids.values()), ids)
    report.facts["profile_files"] = sorted(os.listdir(profile))
    report.facts["datareporting"] = sorted(os.listdir(os.path.join(profile, "datareporting"))) \
        if os.path.isdir(os.path.join(profile, "datareporting")) else []

    with open(os.path.join(a.out, "report.json"), "w") as f:
        json.dump({"checks": report.checks, "facts": report.facts}, f, indent=2, default=str)
    with open(os.path.join(a.out, "report.md"), "w") as f:
        f.write("| | Check | Detail |\n|---|---|---|\n")
        for c in report.checks:
            detail = c["detail"].replace("|", "\\|").replace("\n", " ")[:300]
            f.write(f"| {'✅' if c['ok'] else '❌'} | {c['check']} | {detail} |\n")
    print(f"\n{len(report.checks) - len(report.failed)}/{len(report.checks)} checks passed")
    return 1 if report.failed else 0


if __name__ == "__main__":
    sys.exit(main())
