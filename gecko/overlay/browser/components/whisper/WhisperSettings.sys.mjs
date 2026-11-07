/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Settings (about:preferences) with a Whisper pane first, in Whisper's look, and
 * without Mozilla's own entries.
 *
 * Firefox 159's settings are built from panes registered with SettingPaneManager
 * (preferences.js does it when the page's DOM is ready). Whisper registers its pane
 * the same way, from a capturing DOMContentLoaded listener on the page's window, which
 * runs just before Firefox's own setup; no Firefox file is changed (Firefox's settings
 * files can be loaded by web pages, so changing them would tell pages this is not
 * Firefox). The pane's setting groups are in content/settings/whisper-settings.mjs.
 *
 * It also:
 *   - opens Settings on the Whisper pane when no pane is asked for;
 *   - moves "About Firefox" (Whisper's version and updates are in the Whisper pane),
 *     "Account and sync" (in the Whisper pane, off) and Firefox Labs out of the
 *     navigation; they still open from links that ask for them;
 *   - drops the link to Mozilla's support site, hides Mozilla's offers (VPN, Relay,
 *     suggestions from Mozilla's server; settings.css) and calls Firefox Home
 *     Whisper's page;
 *   - adds "Use this network's DNS until Whisper restarts" to Firefox's privacy
 *     pane, where Firefox's page for a site secure DNS could not find sends the user;
 *   - turns strict secure DNS back on at startup after the user chose the network's
 *     DNS "until Whisper restarts" (whisper.dns.sessionFallback).
 */

const PANE_ID = "whisper";
const NAV_ID = "category-whisper";
const STYLE = "chrome://whisper/content/settings/settings.css";
const ICON = "chrome://whisper/content/settings/whisper-16.svg";
const MODULE = "chrome://whisper/content/settings/whisper-settings.mjs";

// The pane's groups in order; groups that other features register are shown when
// their module is part of the build.
const GROUPS = [
  "whisperAbout",
  "whisperSearch",
  "whisperNewTabs",
  "whisperDefaultBrowser",
  "whisperDns",
  "whisperClipguard",
  "whisperRecoveryPhrase",
  "whisperSwappedAddress",
  "whisperDownloads",
  "whisperPrograms",
  "whisperCleanLinks",
  "whisperPages",
  "whisperLanguage",
  "whisperSync",
  "whisperData",
];

// Navigation entries that Whisper's pane replaces or that speak for Mozilla. They get
// Firefox's own "hidden-category" class, which keeps links to them working.
const MOVED = ["category-about-firefox", "category-sync", "category-experimental"];
const DROPPED = ["helpButton", "category-more-from-mozilla"];

// Firefox strings that name Firefox's own pages, and Whisper's words for them.
const RELABEL = {
  "home-mode-choice-default-fx": "whisper-settings-home-default",
};

const PRIVACY_PANE = "privacy";
const DNS_GROUP = "dnsOverHttps";
const DNS_FALLBACK_GROUP = "whisperDnsFallback";

const TRR_MODE = "network.trr.mode";
const DNS_SESSION = "whisper.dns.sessionFallback";

export const WhisperSettings = {
  _started: false,

  init() {
    if (this._started) {
      return;
    }
    this._started = true;
    this._endDnsSession();
    Services.obs.addObserver(this, "document-element-inserted");
  },

  /** The network's DNS was for one session only: strict secure DNS again. */
  _endDnsSession() {
    if (!Services.prefs.getBoolPref(DNS_SESSION, false)) {
      return;
    }
    Services.prefs.clearUserPref(DNS_SESSION);
    if (Services.prefs.getDefaultBranch("").getIntPref(TRR_MODE, 0) == 3) {
      Services.prefs.clearUserPref(TRR_MODE);
    } else {
      Services.prefs.setIntPref(TRR_MODE, 3);
    }
  },

  observe(doc, topic) {
    if (topic != "document-element-inserted") {
      return;
    }
    const uri = doc?.documentURI ?? "";
    if (!/^about:(preferences|settings)(?=$|[#?])/.test(uri)) {
      return;
    }
    const win = doc.defaultView;
    // Capturing on the window runs before preferences.js's own listener on the
    // document, which builds the panes.
    win?.addEventListener(
      "DOMContentLoaded",
      () => {
        try {
          this._setUp(win);
        } catch (e) {
          console.error("Whisper: settings pane", e);
        }
      },
      { capture: true, once: true }
    );
  },

  /**
   * Secure DNS settings, shared by the Whisper pane and Firefox's privacy pane, where
   * Firefox's page for a site that secure DNS could not find leads ("DNS settings"
   * opens about:preferences#privacy-doh): there Whisper's group comes right after
   * Firefox's DNS group, which the page scrolls to. Runs after Firefox has registered its panes (its listener
   * came first); only the privacy pane's list of groups changes.
   */
  _addDnsSettings(win) {
    const { Preferences, SettingGroupManager, SettingPaneManager } = win;
    for (const [id, type] of [
      [TRR_MODE, "int"],
      [DNS_SESSION, "bool"],
    ]) {
      if (!Preferences.get(id)) {
        Preferences.add({ id, type });
      }
    }
    Preferences.addSetting({ id: "whisperDnsMode", pref: TRR_MODE });
    Preferences.addSetting({ id: "whisperDnsSessionPref", pref: DNS_SESSION });
    Preferences.addSetting({
      id: "whisperDnsStrict",
      deps: ["whisperDnsMode", "whisperDnsSessionPref"],
      get: (_, { whisperDnsMode, whisperDnsSessionPref }) =>
        whisperDnsMode.value == 3 || !!whisperDnsSessionPref.value,
      set: (on, { whisperDnsMode, whisperDnsSessionPref }) => {
        whisperDnsSessionPref.value = false;
        whisperDnsMode.value = on ? 3 : 2;
      },
      disabled: ({ whisperDnsSessionPref }) => !!whisperDnsSessionPref.value,
    });
    Preferences.addSetting({
      id: "whisperDnsSession",
      deps: ["whisperDnsMode", "whisperDnsSessionPref"],
      visible: ({ whisperDnsMode, whisperDnsSessionPref }) =>
        whisperDnsMode.value == 3 && !whisperDnsSessionPref.value,
      // Strict again at the next start (_endDnsSession).
      onUserClick: (_, { whisperDnsMode, whisperDnsSessionPref }) => {
        whisperDnsSessionPref.value = true;
        whisperDnsMode.value = 2;
      },
    });
    Preferences.addSetting({
      id: "whisperDnsSessionOn",
      deps: ["whisperDnsSessionPref"],
      visible: ({ whisperDnsSessionPref }) => !!whisperDnsSessionPref.value,
    });
    SettingGroupManager.registerGroups({
      [DNS_FALLBACK_GROUP]: {
        l10nId: "whisper-settings-dns-fallback-group",
        headingLevel: 2,
        items: [
          {
            id: "whisperDnsSessionOn",
            l10nId: "whisper-settings-dns-session-on",
            control: "moz-message-bar",
            controlAttrs: { type: "warning" },
          },
          {
            id: "whisperDnsSession",
            l10nId: "whisper-settings-dns-session",
            control: "moz-box-button",
          },
        ],
      },
    });
    let privacy;
    try {
      privacy = SettingPaneManager.get(PRIVACY_PANE);
    } catch (e) {
      return;
    }
    const at = privacy.groupIds.indexOf(DNS_GROUP);
    if (at < 0 || privacy.groupIds.includes(DNS_FALLBACK_GROUP)) {
      return;
    }
    privacy.groupIds.splice(at + 1, 0, DNS_FALLBACK_GROUP);
    // Settings opened on the privacy pane (as the error page does) have drawn it
    // already: draw it again with the new group, then set the group up.
    const pane = win.document.querySelector(
      'setting-pane[data-category="panePrivacy"]'
    );
    if (pane?.initialized) {
      pane.requestUpdate();
      pane.updateComplete.then(() => win.initSettingGroup(DNS_FALLBACK_GROUP));
    }
  },

  _setUp(win) {
    const doc = win.document;
    const nav = doc.getElementById("categories");
    const manager = win.SettingPaneManager;
    if (!nav || !manager || !Services.prefs.getBoolPref("browser.settings-redesign.enabled", false)) {
      return;
    }
    doc.l10n.addResourceIds(["whisper/whisper-settings.ftl"]);
    win.windowUtils.loadSheetUsingURIString(
      STYLE,
      Ci.nsIDOMWindowUtils.AUTHOR_SHEET
    );

    manager.registerPane(PANE_ID, {
      l10nId: "whisper-settings-pane-title",
      iconSrc: ICON,
      groupIds: GROUPS,
      module: MODULE,
    });

    const button = doc.createElement("moz-page-nav-button");
    button.id = NAV_ID;
    button.setAttribute("view", "paneWhisper");
    button.setAttribute("iconsrc", ICON);
    doc.l10n.setAttributes(button, "whisper-settings-nav");
    nav.prepend(button);

    for (const id of MOVED) {
      doc.getElementById(id)?.classList.add("hidden-category");
    }
    for (const id of DROPPED) {
      doc.getElementById(id)?.remove();
    }

    // Firefox's settings re-render their controls, so the few strings that name
    // Firefox's own pages are swapped whenever they appear.
    const selector = Object.keys(RELABEL)
      .map(id => `[data-l10n-id="${id}"]`)
      .join(",");
    const relabel = () => {
      for (const el of doc.querySelectorAll(selector)) {
        doc.l10n.setAttributes(el, RELABEL[el.getAttribute("data-l10n-id")]);
      }
    };
    new win.MutationObserver(relabel).observe(doc.getElementById("mainPrefPane"), {
      childList: true,
      subtree: true,
    });

    // Firefox's page for a site that secure DNS could not find ("DNS settings")
    // opens Firefox's DNS pane: it gets Whisper's way out for this session too.
    doc.addEventListener(
      "DOMContentLoaded",
      () => {
        try {
          this._addDnsSettings(win);
        } catch (e) {
          console.error("Whisper: secure DNS settings", e);
        }
      },
      { once: true }
    );

    // Settings opened without a pane: Whisper's.
    if (!win.location.hash) {
      win.history.replaceState(win.history.state, "", `#${PANE_ID}`);
    }
  },
};
