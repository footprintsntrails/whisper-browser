/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * "+" and Ctrl+T open the address bar where it is, not an empty tab: the field is
 * emptied, focused and its results open (open tabs, history, bookmarks, the search
 * engines). Enter, or a click on a result, opens it in a NEW tab; Esc puts the address
 * bar back and leaves no tab behind. File > New Tab and the other menus still open a
 * new tab page. The pref whisper.newtab.addressBar (the Whisper pane of Settings)
 * turns it off.
 *
 * How, with Firefox's public pieces only:
 *   - a capturing "command" listener on the window sees the commands before Firefox's
 *     own handler (browser-sets.js): cmd_newNavigatorTabNoEvent is Ctrl+T's,
 *     cmd_newNavigatorTab with a "+" button as its source is the button's;
 *   - with the field still empty, a provider of the address bar's own kind
 *     (ProvidersManager.registerProvider) lists the open tabs and the pages visited
 *     last, from this profile only (no top sites, nothing from the network);
 *   - while the address bar is open this way, the default of browser.urlbar.openintab
 *     is true, Firefox's own setting for "Enter opens in a new tab" (the default
 *     branch, so nothing is written to the profile; a value the user set wins); it is
 *     put back when the address bar loses focus.
 */

import {
  UrlbarProvider,
  UrlbarUtils,
} from "moz-src:///browser/components/urlbar/UrlbarUtils.sys.mjs";

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  PlacesUtils: "resource://gre/modules/PlacesUtils.sys.mjs",
  PrivateBrowsingUtils: "resource://gre/modules/PrivateBrowsingUtils.sys.mjs",
  ProvidersManager:
    "moz-src:///browser/components/urlbar/UrlbarProvidersManager.sys.mjs",
  UrlbarResult: "chrome://browser/content/urlbar/UrlbarResult.mjs",
  UrlbarShared: "chrome://browser/content/urlbar/UrlbarShared.mjs",
});

const PREF = "whisper.newtab.addressBar";
const MAX_TABS = 5;
const MAX_HISTORY = 5;

/**
 * Results for the empty field that "+" opens: the open tabs (not the current one) and
 * the pages visited last that are not open, newest first.
 */
class WhisperRecentProvider extends UrlbarProvider {
  get name() {
    return "WhisperRecent";
  }

  get type() {
    return lazy.UrlbarShared.PROVIDER_TYPE.PROFILE;
  }

  getPriority() {
    return 1;
  }

  async isActive(queryContext) {
    return (
      WhisperQuickOpen.armedWindows > 0 &&
      !queryContext.searchString &&
      !queryContext.restrictSource &&
      !queryContext.restrictInSearchMode()
    );
  }

  async _recentHistory() {
    const db = await lazy.PlacesUtils.promiseDBConnection();
    return db.executeCached(
      `SELECT url, title FROM moz_places
       WHERE last_visit_date NOT NULL AND hidden = 0
         AND (url_hash BETWEEN hash('http', 'prefix_lo') AND hash('http', 'prefix_hi'))
       ORDER BY last_visit_date DESC LIMIT 30`
    );
  }

  async startQuery(queryContext, addCallback) {
    const open = new Set();
    let tabs = 0;
    for (const win of Services.wm.getEnumerator("navigator:browser")) {
      // A private window lists its own tabs only; a normal one, the normal ones.
      if (
        win.closed ||
        lazy.PrivateBrowsingUtils.isWindowPrivate(win) != queryContext.isPrivate
      ) {
        continue;
      }
      for (const tab of win.gBrowser.tabs) {
        const url = tab.linkedBrowser.currentURI.spec;
        open.add(url);
        if (
          tabs >= MAX_TABS ||
          !/^https?:/.test(url) ||
          url == queryContext.currentPage
        ) {
          continue;
        }
        tabs++;
        addCallback(
          this,
          new lazy.UrlbarResult({
            type: lazy.UrlbarShared.RESULT_TYPE.TAB_SWITCH,
            source: lazy.UrlbarShared.RESULT_SOURCE.TABS,
            payload: {
              url,
              title: tab.label,
              icon: `page-icon:${url}`,
              userContext: UrlbarUtils.getUserContextData(
                tab.userContextId ?? 0
              ),
            },
          })
        );
      }
    }
    if (queryContext.isPrivate) {
      return;
    }
    let rows = [];
    try {
      rows = await this._recentHistory();
    } catch (e) {
      console.error("Whisper: recent history for the address bar", e.message ?? e);
    }
    let added = 0;
    for (const row of rows) {
      const url = row.getResultByName("url");
      if (added >= MAX_HISTORY || open.has(url)) {
        continue;
      }
      added++;
      addCallback(
        this,
        new lazy.UrlbarResult({
          type: lazy.UrlbarShared.RESULT_TYPE.URL,
          source: lazy.UrlbarShared.RESULT_SOURCE.HISTORY,
          payload: {
            url,
            title: row.getResultByName("title") || url,
            icon: `page-icon:${url}`,
          },
        })
      );
    }
  }
}
const OPEN_IN_TAB = "openintab";
const PLUS_BUTTONS = new Set([
  "tabs-newtab-button",
  "vertical-tabs-newtab-button",
  "new-tab-button",
]);

export const WhisperQuickOpen = {
  // Windows whose address bar is open from "+" (the provider above answers then).
  armedWindows: 0,
  _provider: null,

  initWindow(win) {
    if (win.whisperQuickOpen) {
      return;
    }
    if (!this._provider && win.gURLBar?.sapName) {
      this._provider = new WhisperRecentProvider();
      lazy.ProvidersManager.getInstanceForSap(win.gURLBar.sapName).registerProvider(
        this._provider
      );
    }
    const onCommand = event => this._onCommand(win, event);
    win.addEventListener("command", onCommand, true);
    win.whisperQuickOpen = () => {
      win.removeEventListener("command", onCommand, true);
      this._disarm(win);
    };
  },

  uninitWindow(win) {
    win.whisperQuickOpen?.();
    delete win.whisperQuickOpen;
  },

  _onCommand(win, event) {
    if (!Services.prefs.getBoolPref(PREF, true)) {
      return;
    }
    const id = event.target?.id;
    const source = event.sourceEvent?.target;
    const ours =
      id == "cmd_newNavigatorTabNoEvent" ||
      (id == "cmd_newNavigatorTab" && PLUS_BUTTONS.has(source?.id));
    if (!ours || !win.gURLBar || win.gURLBar.readOnly) {
      return;
    }
    event.stopPropagation();
    event.preventDefault();
    this.open(win, event);
  },

  /** Opens the address bar in place; the next result picked opens in a new tab. */
  open(win, event = null) {
    const urlbar = win.gURLBar;
    this._arm(win);
    // An empty typed value, as in a new tab: a page still loading in this tab does not
    // write its address back into the field.
    win.gBrowser.userTypedValue = "";
    urlbar.value = "";
    urlbar.select();
    if (!event || !urlbar.view.autoOpen({ event })) {
      urlbar.startQuery({ event });
    }
  },

  _arm(win) {
    if (win.whisperQuickOpenArmed) {
      return;
    }
    const urlbar = win.gURLBar;
    const defaults = Services.prefs.getDefaultBranch("browser.urlbar.");
    const before = defaults.getBoolPref(OPEN_IN_TAB, false);
    defaults.setBoolPref(OPEN_IN_TAB, true);
    const onKey = e => {
      if (e.key == "Escape") {
        // Firefox's first Esc only closes the results; here one Esc puts everything
        // back as it was.
        e.preventDefault();
        e.stopPropagation();
        urlbar.view.close();
        urlbar.handleRevert();
        win.gBrowser.selectedBrowser.focus();
      }
    };
    this.armedWindows++;
    const disarm = () => {
      this._disarm(win);
      // Left without typing anything: the field shows the page's address again.
      if (!urlbar.value) {
        urlbar.handleRevert();
      }
    };
    urlbar.inputField.addEventListener("keydown", onKey, true);
    urlbar.inputField.addEventListener("blur", disarm);
    win.whisperQuickOpenArmed = () => {
      this.armedWindows--;
      urlbar.inputField.removeEventListener("keydown", onKey, true);
      urlbar.inputField.removeEventListener("blur", disarm);
      defaults.setBoolPref(OPEN_IN_TAB, before);
    };
  },

  _disarm(win) {
    win.whisperQuickOpenArmed?.();
    delete win.whisperQuickOpenArmed;
  },
};
