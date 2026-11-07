/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Dark Reader, shipped with Whisper (gecko/darkreader.env, installed from
 * distribution/extensions/ on first start), kept offline.
 *
 * Whisper asks every site for its own dark design first
 * (layout.css.prefers-color-scheme.content-override = 0); Dark Reader recolours the
 * pages that have none. It is on in normal tabs and stays out of container tabs
 * (whisper.extensions.defaultContainerOnly, patches/0038), which later carry Whisper's
 * masks. Its updates come with Whisper (whisper.extensions.pinned, patches/0035).
 *
 * What Dark Reader 4.9.135 fetches on its own, and what Whisper does about it (every
 * request it triggers is cancelled in the parent process before it leaves):
 *   - darkreader.org/blog/posts.json, its news, at start and then periodically;
 *   - darkreader.org/help, opened in a new tab when it is installed (the tab is closed;
 *     Firefox looks up a page's host name as soon as the tab asks for it, so on the
 *     first start darkreader.org resolves locally for a few minutes, through the
 *     default of network.dns.localDomains, and no lookup leaves the computer);
 *   - darkreader.org/goodluck, opened when it is removed;
 *   - raw.githubusercontent.com/darkreader/... site fixes, if "synchronize site fixes"
 *     is turned on in its settings (off by default);
 *   - stylesheets and images of the page you are on, which it re-reads to recolour
 *     them when the page does not let it read them directly; without them it still
 *     recolours, a little less precisely.
 * Links the user clicks in its panel open as any link does.
 *
 * Only Firefox's public pieces: an "http-on-opening-request" observer that looks at
 * the request's triggering principal, AddonManager and ExtensionPermissions.
 */

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  AddonManager: "resource://gre/modules/AddonManager.sys.mjs",
  ExtensionPermissions: "resource://gre/modules/ExtensionPermissions.sys.mjs",
});

export const DARKREADER_ID = "addon@darkreader.org";
const SETUP_PREF = "whisper.darkreader.setUp";
const LOCAL_DOMAINS = "network.dns.localDomains";
const OWN_HOST = "darkreader.org";
// How long darkreader.org resolves locally on the first start (its help tab).
const FIRST_RUN_MS = 3 * 60 * 1000;
// Dark Reader's pages that open by themselves (on install and on removal).
const OWN_PAGES = /^https:\/\/darkreader\.org\/(help|goodluck)(\/|$|\?)/;

function addonOf(principal) {
  try {
    return principal?.addonId || "";
  } catch (e) {
    return "";
  }
}

export const WhisperDarkReader = {
  _started: false,
  // What was stopped, for the record (about:support-like checks and the tests).
  blocked: [],

  init() {
    if (this._started) {
      return;
    }
    this._started = true;
    if (!Services.prefs.getBoolPref(SETUP_PREF, false)) {
      this._resolveLocallyForAWhile();
    }
    // On opening, before Firefox looks up the host name (it may start that early).
    Services.obs.addObserver(this, "http-on-opening-request");
    this._setUp().catch(e => console.error("Whisper: Dark Reader setup", e));
  },

  observe(subject, topic) {
    if (topic != "http-on-opening-request") {
      return;
    }
    let channel;
    try {
      channel = subject.QueryInterface(Ci.nsIHttpChannel);
    } catch (e) {
      return;
    }
    const info = channel.loadInfo;
    const fromDarkReader =
      addonOf(info?.triggeringPrincipal) == DARKREADER_ID ||
      addonOf(info?.loadingPrincipal) == DARKREADER_ID;
    const ownPage = OWN_PAGES.test(channel.URI.spec);
    const document =
      info?.externalContentPolicyType == Ci.nsIContentPolicy.TYPE_DOCUMENT;
    // Dark Reader's own fetches, and its pages that open by themselves (Dark Reader
    // opens the help page; Firefox opens the removal page, as the system). A link the
    // user follows from its panel is a document load to any other page.
    const fetch = fromDarkReader && !document;
    const ownPageOpened =
      document && ownPage && (fromDarkReader || this._systemLoad(info));
    if (!fetch && !ownPageOpened) {
      return;
    }
    channel.cancel(Cr.NS_ERROR_ABORT);
    if (this.blocked.length < 50) {
      this.blocked.push(channel.URI.spec);
    }
    if (document) {
      this._closeTabOf(info);
    }
  },

  /** The uninstall page opens with the system principal, not Dark Reader's. */
  _systemLoad(info) {
    try {
      return info.triggeringPrincipal.isSystemPrincipal;
    } catch (e) {
      return false;
    }
  },

  /** A tab Dark Reader opened for a page that is not loaded: closed again. */
  _closeTabOf(info) {
    const id = info?.browsingContextID;
    if (!id) {
      return;
    }
    for (const win of Services.wm.getEnumerator("navigator:browser")) {
      const gBrowser = win.gBrowser;
      const tab = gBrowser?.tabs.find(
        t => t.linkedBrowser?.browsingContext?.id == id
      );
      if (tab) {
        win.setTimeout(() => {
          if (!tab.closing && gBrowser.tabs.length > 1) {
            gBrowser.removeTab(tab);
          }
        }, 0);
        return;
      }
    }
  },

  /**
   * The first start installs Dark Reader, which opens its help page at once; the page
   * is cancelled, but the host name would be looked up when the tab asks for it.
   * darkreader.org resolves to this computer meanwhile (the default branch, so nothing
   * is written to the profile; a value the user set wins).
   */
  _resolveLocallyForAWhile() {
    const defaults = Services.prefs.getDefaultBranch("");
    const before = defaults.getStringPref(LOCAL_DOMAINS, "");
    const list = before ? `${before},${OWN_HOST}` : OWN_HOST;
    defaults.setStringPref(LOCAL_DOMAINS, list);
    const timer = Cc["@mozilla.org/timer;1"].createInstance(Ci.nsITimer);
    timer.initWithCallback(
      () => defaults.setStringPref(LOCAL_DOMAINS, before),
      FIRST_RUN_MS,
      Ci.nsITimer.TYPE_ONE_SHOT
    );
    this._localTimer = timer;
  },

  /** Once per profile: Dark Reader may also work in private windows, as uBlock Origin. */
  async _setUp() {
    if (Services.prefs.getBoolPref(SETUP_PREF, false)) {
      return;
    }
    const addon = await lazy.AddonManager.getAddonByID(DARKREADER_ID);
    if (!addon) {
      return;
    }
    Services.prefs.setBoolPref(SETUP_PREF, true);
    await lazy.ExtensionPermissions.add(DARKREADER_ID, {
      permissions: ["internal:privateBrowsingAllowed"],
      origins: [],
    });
  },

  /** For Settings: whether Dark Reader is installed and on. */
  async state() {
    const addon = await lazy.AddonManager.getAddonByID(DARKREADER_ID);
    return { installed: !!addon, enabled: !!addon && !addon.userDisabled };
  },

  async setEnabled(on) {
    const addon = await lazy.AddonManager.getAddonByID(DARKREADER_ID);
    if (!addon) {
      return;
    }
    if (on) {
      await addon.enable();
    } else {
      await addon.disable();
    }
  },
};
