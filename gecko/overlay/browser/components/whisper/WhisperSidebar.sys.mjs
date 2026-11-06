/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Whisper's three tab sidebar modes, built on Firefox's own vertical tabs:
 *
 *   compact   Firefox's collapsed launcher: tab icons only, a narrow strip.
 *   autohide  Firefox's "expand on hover" launcher, with Whisper's CSS turning the
 *             collapsed launcher into an invisible edge strip. Touching the left edge
 *             of the window (or the screen, when the window is maximized) slides the
 *             sidebar in over the page; it hides again when the pointer leaves.
 *   expanded  Firefox's expanded launcher, narrower, with taller tabs, larger icons
 *             and larger names (cast.css).
 *
 * The mode lives in the pref whisper.sidebar.mode and applies to every window. It is
 * switched from the sidebar's right-click menus or with Ctrl+Alt+S (cycles through
 * the modes). Firefox's own controls stay in sync: choosing "Expand sidebar on hover"
 * in Firefox's Customize sidebar panel selects auto-hide, "Expand and collapse
 * sidebar" or the toolbar's sidebar button selects compact or expanded.
 *
 * Only Firefox's public pieces are used: the sidebar.visibility pref, SidebarController's
 * initialization promise and the same state call Firefox's sidebar button makes.
 */

export const MODE_PREF = "whisper.sidebar.mode";
const VISIBILITY_PREF = "sidebar.visibility";
export const MODES = ["expanded", "compact", "autohide"];
const DEFAULT_MODE = "expanded";
const MENU_ITEM_PREFIX = "whisper-sidebar-mode-";
// After Whisper changes the launcher itself, ignore the resulting attribute changes for
// this long, so they are not mistaken for the user's choice.
const QUIET_MS = 600;

function visibilityFor(mode) {
  return mode == "autohide" ? "expand-on-hover" : "always-show";
}

/**
 * Whether a node is inside the tab sidebar. Walks the flattened tree, so a click
 * inside a shadow root (the tab list's scroll box) counts too.
 */
function isInSidebar(node) {
  for (let n = node; n; n = n.flattenedTreeParentNode ?? n.parentNode ?? n.host) {
    if (n.id == "vertical-tabs" || n.id == "sidebar-container") {
      return true;
    }
  }
  return false;
}

export const WhisperSidebar = {
  _windows: new Set(),
  _observing: false,
  _applying: 0,

  get mode() {
    const mode = Services.prefs.getStringPref(MODE_PREF, DEFAULT_MODE);
    return MODES.includes(mode) ? mode : DEFAULT_MODE;
  },

  set mode(mode) {
    if (!MODES.includes(mode)) {
      throw new Error(`unknown sidebar mode ${mode}`);
    }
    Services.prefs.setStringPref(MODE_PREF, mode);
  },

  /** The next mode for the keyboard shortcut. */
  cycle() {
    this.mode = MODES[(MODES.indexOf(this.mode) + 1) % MODES.length];
  },

  /**
   * Called before the window's first layout, so the stylesheet already knows the mode
   * when the sidebar first paints.
   */
  markWindow(win) {
    this._markRoot(win);
  },

  async initWindow(win) {
    this._windows.add(win);
    this._startObserving();
    this._addShortcut(win);
    this._addMenuItems(win);
    await this._applyToWindow(win, this.mode, { respectFirefoxMode: true });
    this._watchLauncher(win);
  },

  uninitWindow(win) {
    this._windows.delete(win);
    win.whisperLauncherObserver?.disconnect();
    delete win.whisperLauncherObserver;
    if (!this._windows.size) {
      this._stopObserving();
    }
  },

  _markRoot(win) {
    const root = win.document.documentElement;
    const visibility = Services.prefs.getStringPref(
      VISIBILITY_PREF,
      "always-show"
    );
    // Firefox's "Show and hide sidebar" setting is not one of Whisper's modes; the
    // sidebar then looks and works exactly as in Firefox.
    if (visibility == "hide-sidebar") {
      root.removeAttribute("whisper-sidebar-mode");
    } else {
      root.setAttribute("whisper-sidebar-mode", this.mode);
    }
  },

  _startObserving() {
    if (this._observing) {
      return;
    }
    this._observing = true;
    Services.prefs.addObserver(MODE_PREF, this);
    Services.prefs.addObserver(VISIBILITY_PREF, this);
  },

  _stopObserving() {
    if (!this._observing) {
      return;
    }
    this._observing = false;
    Services.prefs.removeObserver(MODE_PREF, this);
    Services.prefs.removeObserver(VISIBILITY_PREF, this);
  },

  observe(_subject, topic, data) {
    if (topic != "nsPref:changed") {
      return;
    }
    if (data == MODE_PREF) {
      const mode = this.mode;
      for (const win of this._windows) {
        this._applyToWindow(win, mode).catch(e =>
          console.error("Whisper: could not apply the sidebar mode", e)
        );
      }
    } else if (data == VISIBILITY_PREF && !this._applying) {
      this._followFirefoxVisibility();
    }
  },

  /** Firefox's own Customize sidebar panel changed sidebar.visibility. */
  _followFirefoxVisibility() {
    const visibility = Services.prefs.getStringPref(
      VISIBILITY_PREF,
      "always-show"
    );
    let mode = null;
    if (visibility == "expand-on-hover") {
      mode = "autohide";
    } else if (visibility == "always-show" && this.mode == "autohide") {
      // Firefox expands the launcher when this setting is chosen.
      mode = "expanded";
    }
    if (mode && mode != this.mode) {
      this.mode = mode;
    } else {
      for (const win of this._windows) {
        this._markRoot(win);
      }
    }
  },

  /**
   * @param {Window} win
   * @param {string} mode
   * @param {object} [options]
   * @param {boolean} [options.respectFirefoxMode] when the user picked Firefox's own
   *   "Show and hide sidebar" setting, keep it (used when a window opens). When the
   *   user picks a Whisper mode, it replaces that setting.
   */
  async _applyToWindow(win, mode, { respectFirefoxMode = false } = {}) {
    this._markRoot(win);
    this._updateMenuItems(win);
    const controller = win.SidebarController;
    if (!controller) {
      return;
    }
    await controller.promiseInitialized;
    if (
      win.closed ||
      controller.inSingleTabWindow ||
      !controller.sidebarRevampEnabled ||
      !controller.sidebarVerticalTabsEnabled
    ) {
      return;
    }
    const visibility = Services.prefs.getStringPref(
      VISIBILITY_PREF,
      "always-show"
    );
    if (visibility == "hide-sidebar" && respectFirefoxMode) {
      return;
    }
    win.whisperQuietUntil = Date.now() + QUIET_MS;
    this._applying++;
    try {
      const wanted = visibilityFor(mode);
      if (visibility != wanted) {
        // Every window's SidebarController reacts to this pref on its own.
        Services.prefs.setStringPref(VISIBILITY_PREF, wanted);
      }
      this._markRoot(win);
      if (mode != "autohide") {
        const expanded = mode == "expanded";
        if (controller._state.launcherExpanded != expanded) {
          // The same call Firefox's sidebar toolbar button makes.
          controller._state.updateVisibility(true, expanded);
          controller.updateToolbarButton();
        }
      }
    } finally {
      this._applying--;
    }
  },

  /**
   * In Firefox's "Expand and collapse sidebar" setting, the toolbar's sidebar button
   * expands and collapses the launcher. Follow that as compact and expanded.
   */
  _watchLauncher(win) {
    if (win.closed || win.whisperLauncherObserver) {
      return;
    }
    const container = win.document.getElementById("sidebar-container");
    if (!container) {
      return;
    }
    const observer = new win.MutationObserver(() => {
      if (this._applying || Date.now() < (win.whisperQuietUntil ?? 0)) {
        return;
      }
      const visibility = Services.prefs.getStringPref(
        VISIBILITY_PREF,
        "always-show"
      );
      if (
        visibility != "always-show" ||
        !win.SidebarController?.sidebarVerticalTabsEnabled
      ) {
        return;
      }
      const mode = container.hasAttribute("sidebar-launcher-expanded")
        ? "expanded"
        : "compact";
      if (mode != this.mode) {
        this.mode = mode;
      }
    });
    observer.observe(container, {
      attributes: true,
      attributeFilter: ["sidebar-launcher-expanded"],
    });
    win.whisperLauncherObserver = observer;
  },

  _addShortcut(win) {
    const doc = win.document;
    if (doc.getElementById("whisper-keyset")) {
      return;
    }
    // A new <keyset> is picked up by the key handler when it is added to the window.
    const keyset = doc.createXULElement("keyset");
    keyset.id = "whisper-keyset";
    const key = doc.createXULElement("key");
    key.id = "key_whisperSidebarMode";
    key.setAttribute("key", "S");
    key.setAttribute("modifiers", "accel,alt");
    key.addEventListener("command", () => this.cycle());
    keyset.appendChild(key);
    doc.documentElement.appendChild(keyset);
  },

  /**
   * Adds the three modes to the right-click menu of the sidebar's buttons
   * (#sidebar-context-menu) and of the tab strip's empty space
   * (#toolbar-context-menu, only when opened in the vertical tab strip).
   */
  _addMenuItems(win) {
    const doc = win.document;
    win.MozXULElement.insertFTLIfNeeded("whisper/whisper.ftl");
    const menus = [
      ["sidebar-context-menu", "sidebar-context-menu-customize-sidebar"],
      ["toolbar-context-menu", "toolbar-context-customize-sidebar"],
    ];
    for (const [menuId, beforeId] of menus) {
      const menu = doc.getElementById(menuId);
      if (!menu || menu.querySelector(`.${MENU_ITEM_PREFIX}item`)) {
        continue;
      }
      const before = doc.getElementById(beforeId);
      const separator = doc.createXULElement("menuseparator");
      separator.classList.add(`${MENU_ITEM_PREFIX}item`);
      const items = [separator];
      for (const mode of ["compact", "autohide", "expanded"]) {
        const item = doc.createXULElement("menuitem");
        item.id = `${MENU_ITEM_PREFIX}${mode}-${menuId}`;
        item.classList.add(`${MENU_ITEM_PREFIX}item`);
        item.setAttribute("type", "radio");
        item.setAttribute("name", `${MENU_ITEM_PREFIX}${menuId}`);
        item.dataset.whisperSidebarMode = mode;
        doc.l10n.setAttributes(item, `whisper-sidebar-mode-${mode}`);
        item.addEventListener("command", () => {
          this.mode = mode;
        });
        items.push(item);
      }
      for (const item of items) {
        menu.insertBefore(item, before?.parentNode == menu ? before : null);
      }
      menu.addEventListener("popupshowing", event => {
        if (event.target != menu) {
          return;
        }
        const controller = win.SidebarController;
        const inSidebar =
          menuId == "sidebar-context-menu" || isInSidebar(menu.triggerNode);
        // A sidebar tool (History, Bookmarks…) brings its own menu items; stay out of it.
        const toolMenu = !!menu.querySelector(
          "[customized-tool='true']:not([hidden])"
        );
        const show =
          inSidebar && !toolMenu && !!controller?.sidebarVerticalTabsEnabled;
        for (const item of menu.querySelectorAll(`.${MENU_ITEM_PREFIX}item`)) {
          item.hidden = !show;
        }
        this._updateMenuItems(win);
      });
    }
  },

  _updateMenuItems(win) {
    const mode = this.mode;
    const visibility = Services.prefs.getStringPref(
      VISIBILITY_PREF,
      "always-show"
    );
    for (const item of win.document.querySelectorAll(
      `menuitem.${MENU_ITEM_PREFIX}item`
    )) {
      const checked =
        visibility != "hide-sidebar" && item.dataset.whisperSidebarMode == mode;
      if (checked) {
        item.setAttribute("checked", "true");
      } else {
        item.removeAttribute("checked");
      }
    }
  },
};
