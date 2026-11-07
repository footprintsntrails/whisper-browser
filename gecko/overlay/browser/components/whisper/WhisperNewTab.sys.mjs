/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Whisper's new tab and home page: chrome://whisper/content/newtab/newtab.html, a
 * local page (no network request at all, site icons from the browser's icon cache)
 * with the Whisper mark, a search field that hands over to the address bar, and the
 * sites the user pins.
 *
 * Firefox's way to show another new tab page (the one extensions use) is
 * AboutNewTab.newTabURL; the home page is the pref browser.startup.homepage, whose
 * default 0-whisper.js sets to the same page. Whisper leaves the new tab page alone
 * when an extension has set its own, and shows a blank page when Settings says "Blank
 * page" (browser.newtabpage.enabled = false), as Firefox does.
 */

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  AboutNewTab: "resource:///modules/AboutNewTab.sys.mjs",
});

export const NEW_TAB_URL = "chrome://whisper/content/newtab/newtab.html";
const ENABLED_PREF = "browser.newtabpage.enabled";
const EXTENSION_PREF = "browser.newtab.extensionControlled";

export const WhisperNewTab = {
  _started: false,

  init() {
    if (this._started) {
      return;
    }
    this._started = true;
    Services.prefs.addObserver(ENABLED_PREF, this);
    Services.prefs.addObserver(EXTENSION_PREF, this);
    this.apply();
  },

  observe() {
    this.apply();
  },

  apply() {
    if (Services.prefs.getBoolPref(EXTENSION_PREF, false)) {
      return; // an extension provides the new tab page
    }
    const wanted = Services.prefs.getBoolPref(ENABLED_PREF, true)
      ? NEW_TAB_URL
      : "about:blank";
    if (lazy.AboutNewTab.newTabURL != wanted) {
      lazy.AboutNewTab.newTabURL = wanted;
    }
  },
};
