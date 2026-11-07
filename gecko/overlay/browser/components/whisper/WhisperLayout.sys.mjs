/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The one place for Whisper's window layout state. Read and change the layout here;
 * the modules that draw it (WhisperSidebar, WhisperToolbar) follow its changes.
 *
 *   tabs              "vertical" | "horizontal"   Firefox's own sidebar.verticalTabs
 *   sidebarMode       "expanded" | "compact" | "autohide"   (vertical tabs only)
 *                                                   whisper.sidebar.mode
 *   toolbarStyle      "compact" | "standard"        whisper.toolbar.style
 *   toolbarPosition   "top" | "sidebar"             whisper.toolbar.position
 *                                                   ("sidebar": the toolbar's controls
 *                                                   move into the vertical sidebar; not
 *                                                   built yet, reads back as "top")
 *
 * Every browser window's root element carries the state as attributes, for CSS:
 *   whisper-tabs, whisper-sidebar-mode (vertical tabs only), whisper-toolbar,
 *   whisper-toolbar-position
 * and the colour of its space as the CSS variable --whisper-space-color (Cast black,
 * #111316, until spaces exist). The new tab page reads the same variable.
 *
 * API (see README.md next to this file):
 *   WhisperLayout.state                 a frozen snapshot of the values above
 *   WhisperLayout.set({ tabs: "horizontal" })    changes one or more values
 *   WhisperLayout.addListener(fn)       fn(state, changedKeys) after every change;
 *   WhisperLayout.removeListener(fn)    also the observer topic "whisper-layout-changed"
 *   WhisperLayout.spaceColor(win)       the window's space colour
 *   WhisperLayout.setSpaceColor(win, color)   sets it (null: back to Cast black)
 */

const PREFS = {
  tabs: "sidebar.verticalTabs",
  sidebarMode: "whisper.sidebar.mode",
  toolbarStyle: "whisper.toolbar.style",
  toolbarPosition: "whisper.toolbar.position",
};

export const VALUES = {
  tabs: ["vertical", "horizontal"],
  sidebarMode: ["expanded", "compact", "autohide"],
  toolbarStyle: ["compact", "standard"],
  toolbarPosition: ["top", "sidebar"],
};

// Values that exist in the state but are not built yet read back as the default.
const BUILT = {
  toolbarPosition: ["top"],
};

export const TOPIC = "whisper-layout-changed";
export const CAST_BLACK = "#111316";
const SPACE_COLOR_VAR = "--whisper-space-color";

function readPref(key) {
  switch (key) {
    case "tabs":
      return Services.prefs.getBoolPref(PREFS.tabs, false)
        ? "vertical"
        : "horizontal";
    default: {
      const value = Services.prefs.getStringPref(PREFS[key], "");
      const allowed = BUILT[key] ?? VALUES[key];
      return allowed.includes(value) ? value : allowed[0];
    }
  }
}

export const WhisperLayout = {
  _listeners: new Set(),
  _observing: false,
  _windows: new Set(),

  get state() {
    const state = {};
    for (const key of Object.keys(PREFS)) {
      state[key] = readPref(key);
    }
    return Object.freeze(state);
  },

  /**
   * @param {object} changes for example { tabs: "horizontal", sidebarMode: "compact" }
   */
  set(changes) {
    for (const [key, value] of Object.entries(changes)) {
      if (!VALUES[key]?.includes(value)) {
        throw new Error(`WhisperLayout: ${key} cannot be ${value}`);
      }
    }
    for (const [key, value] of Object.entries(changes)) {
      if (key == "tabs") {
        Services.prefs.setBoolPref(PREFS.tabs, value == "vertical");
      } else {
        Services.prefs.setStringPref(PREFS[key], value);
      }
    }
  },

  addListener(fn) {
    this._listeners.add(fn);
  },

  removeListener(fn) {
    this._listeners.delete(fn);
  },

  /** Called for every browser window before its first layout (WhisperUI). */
  initWindow(win) {
    this._windows.add(win);
    this._startObserving();
    this._markWindow(win);
  },

  uninitWindow(win) {
    this._windows.delete(win);
  },

  spaceColor(win) {
    return (
      win.document.documentElement.style.getPropertyValue(SPACE_COLOR_VAR) ||
      CAST_BLACK
    );
  },

  setSpaceColor(win, color) {
    const root = win.document.documentElement;
    if (color) {
      root.style.setProperty(SPACE_COLOR_VAR, color);
    } else {
      root.style.removeProperty(SPACE_COLOR_VAR);
    }
    this._notify(["spaceColor"], win);
  },

  _markWindow(win) {
    const root = win.document.documentElement;
    const state = this.state;
    root.setAttribute("whisper-tabs", state.tabs);
    root.setAttribute("whisper-toolbar", state.toolbarStyle);
    root.setAttribute("whisper-toolbar-position", state.toolbarPosition);
  },

  _startObserving() {
    if (this._observing) {
      return;
    }
    this._observing = true;
    for (const pref of Object.values(PREFS)) {
      Services.prefs.addObserver(pref, this);
    }
  },

  observe(_subject, topic, data) {
    if (topic != "nsPref:changed") {
      return;
    }
    const key = Object.keys(PREFS).find(k => PREFS[k] == data);
    if (!key) {
      return;
    }
    for (const win of this._windows) {
      if (!win.closed) {
        this._markWindow(win);
      }
    }
    this._notify([key]);
  },

  _notify(changedKeys, win = null) {
    const state = this.state;
    for (const fn of this._listeners) {
      try {
        fn(state, changedKeys, win);
      } catch (e) {
        console.error("Whisper: layout listener failed", e);
      }
    }
    Services.obs.notifyObservers(win, TOPIC, JSON.stringify(changedKeys));
  },
};
