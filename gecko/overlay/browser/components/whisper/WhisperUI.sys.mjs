/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Whisper's single entry point into the browser.
 *
 * Whisper.manifest registers the functions below with Firefox's own window lifecycle
 * categories (the same ones Firefox uses for its own features), so no Firefox file
 * has to be edited to load Whisper's interface:
 *
 *   browser-before-ui-startup               WhisperUI.init()            once per run
 *   browser-window-before-initial-xul-layout WhisperUI.onWindowBeforeLayout(win)
 *   browser-window-delayed-startup          WhisperUI.onWindowDelayedStartup(win)
 *   browser-window-unload                   WhisperUI.onWindowUnload(win)
 *
 * Each feature runs on its own: if one fails (say, a future Firefox renames something
 * it relies on), it is logged to the browser console and the others carry on; the
 * browser then simply behaves like Firefox there.
 */

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  AddonManager: "resource://gre/modules/AddonManager.sys.mjs",
  CustomizableUI:
    "moz-src:///browser/components/customizableui/CustomizableUI.sys.mjs",
  ExtensionPermissions: "resource://gre/modules/ExtensionPermissions.sys.mjs",
  WhisperSidebar: "moz-src:///browser/components/whisper/WhisperSidebar.sys.mjs",
});

const CAST_CHROME_SHEET = "chrome://browser/content/whisper/cast.css";
const CAST_PAGES_SHEET = "chrome://browser/content/whisper/cast-pages.css";

const UBO_ID = "uBlock0@raymondhill.net";
const UBO_WIDGET_ID = "ublock0_raymondhill_net-browser-action";
// Set once Whisper has done its one-time setup of uBlock Origin in this profile, so a
// user who later unpins it or turns it off in private windows keeps that choice.
const UBO_SETUP_PREF = "whisper.ubo.firstRunDone";

function log(...args) {
  console.log("Whisper:", ...args);
}

function guarded(name, fn) {
  try {
    const result = fn();
    if (result && typeof result.catch == "function") {
      result.catch(e => console.error(`Whisper: ${name} failed`, e));
    }
  } catch (e) {
    console.error(`Whisper: ${name} failed`, e);
  }
}

export const WhisperUI = {
  _initialized: false,
  _uboSetupStarted: false,

  /** Once per run, before the first window. */
  init() {
    if (this._initialized) {
      return;
    }
    this._initialized = true;
    guarded("page colours", () => this._registerPageSheet());
  },

  /** Before the window's first layout: styles must be in place before anything paints. */
  onWindowBeforeLayout(win) {
    guarded("Cast stylesheet", () =>
      win.windowUtils.loadSheetUsingURIString(
        CAST_CHROME_SHEET,
        Ci.nsIDOMWindowUtils.AUTHOR_SHEET
      )
    );
    guarded("sidebar mode attribute", () =>
      lazy.WhisperSidebar.markWindow(win)
    );
  },

  /** After the window is up; nothing here delays the first paint. */
  onWindowDelayedStartup(win) {
    guarded("sidebar modes", () => lazy.WhisperSidebar.initWindow(win));
    if (!this._uboSetupStarted) {
      this._uboSetupStarted = true;
      guarded("uBlock Origin first run", () => this._setUpUBlockOrigin());
    }
  },

  onWindowUnload(win) {
    guarded("sidebar modes cleanup", () =>
      lazy.WhisperSidebar.uninitWindow(win)
    );
  },

  /**
   * Colours for the browser's own pages (new tab, home, private window page). The
   * sheet only has rules for those about: pages, so web pages are not affected.
   */
  _registerPageSheet() {
    const sss = Cc["@mozilla.org/content/style-sheet-service;1"].getService(
      Ci.nsIStyleSheetService
    );
    const uri = Services.io.newURI(CAST_PAGES_SHEET);
    if (!sss.sheetRegistered(uri, sss.USER_SHEET)) {
      sss.loadAndRegisterSheet(uri, sss.USER_SHEET);
    }
  },

  /**
   * uBlock Origin comes with Whisper as a distribution add-on (installed and enabled
   * by Firefox on first start). Whisper then does two things once per profile:
   * lets it run in private windows, and pins its button to the toolbar.
   */
  async _setUpUBlockOrigin() {
    if (Services.prefs.getBoolPref(UBO_SETUP_PREF, false)) {
      return;
    }
    const addon = await lazy.AddonManager.getAddonByID(UBO_ID);
    if (!addon) {
      log("uBlock Origin is not installed; nothing to set up");
      return;
    }
    Services.prefs.setBoolPref(UBO_SETUP_PREF, true);

    this._pinUBlockOriginButton();

    const perms = await lazy.ExtensionPermissions.get(UBO_ID);
    if (!perms.permissions.includes("internal:privateBrowsingAllowed")) {
      await lazy.ExtensionPermissions.add(UBO_ID, {
        permissions: ["internal:privateBrowsingAllowed"],
        origins: [],
      });
      // As about:addons does: a running extension only picks up the private
      // browsing permission when it restarts.
      if (addon.isActive) {
        await addon.reload();
      }
      log("uBlock Origin may now run in private windows");
    }
  },

  _pinUBlockOriginButton() {
    const CUI = lazy.CustomizableUI;
    // The widget may not be registered yet (the extension is still starting);
    // CustomizableUI keeps the placement and uses it when the button appears.
    const placement = CUI.getPlacementOfWidget(UBO_WIDGET_ID, false);
    if (placement?.area == CUI.AREA_NAVBAR) {
      return;
    }
    const navbar = CUI.getWidgetIdsInArea(CUI.AREA_NAVBAR);
    const before = navbar.indexOf("unified-extensions-button");
    CUI.addWidgetToArea(
      UBO_WIDGET_ID,
      CUI.AREA_NAVBAR,
      before >= 0 ? before : undefined
    );
    log("pinned the uBlock Origin button to the toolbar");
  },
};
