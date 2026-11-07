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
 *   - while the address bar is open this way, the default of browser.urlbar.openintab
 *     is true, Firefox's own setting for "Enter opens in a new tab" (the default
 *     branch, so nothing is written to the profile; a value the user set wins); it is
 *     put back when the address bar loses focus.
 */

const PREF = "whisper.newtab.addressBar";
const OPEN_IN_TAB = "openintab";
const PLUS_BUTTONS = new Set([
  "tabs-newtab-button",
  "vertical-tabs-newtab-button",
  "new-tab-button",
]);

export const WhisperQuickOpen = {
  initWindow(win) {
    if (win.whisperQuickOpen) {
      return;
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
    urlbar.view.autoOpen({ event });
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
