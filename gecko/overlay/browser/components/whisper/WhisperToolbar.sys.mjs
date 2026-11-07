/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Whisper's toolbar: one slim row.
 *
 *   back  forward  reload  [ address bar, with its page actions inside ]  uBlock Origin
 *   extensions  downloads (only while it has something to show)  menu  (window controls)
 *
 * Firefox's "Customize toolbar" still works; Whisper sets this layout once per profile
 * (whisper.toolbar.layoutVersion) and never again, so a layout the user arranges
 * afterwards stays as it is. The first version also repairs the toolbars of Whisper
 * 0.1.0, which lost the reload button and came out in reverse order: Firefox moves
 * the tab strip's buttons into the toolbar when vertical tabs are on at the very
 * first start, and its own default placements were lost on the way.
 *
 * With horizontal tabs the tab strip keeps only the tabs and the new tab button, and
 * the sidebar button shows at the start of the toolbar (the bookmarks, history and
 * passwords panels open from it). With vertical tabs it stays placed but hidden
 * (cast.css): the sidebar has its own buttons, and Firefox turns vertical tabs off
 * when the sidebar button is removed from the toolbar.
 *
 * whisper.toolbar.style "compact" (default) uses Firefox's compact density and a lower
 * row (cast.css, :root[whisper-toolbar="compact"]); "standard" uses Firefox's normal
 * density.
 */

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  CustomizableUI:
    "moz-src:///browser/components/customizableui/CustomizableUI.sys.mjs",
  WhisperLayout: "moz-src:///browser/components/whisper/WhisperLayout.sys.mjs",
});

const LAYOUT_VERSION_PREF = "whisper.toolbar.layoutVersion";
const LAYOUT_VERSION = 1;
const HORIZONTAL_TABSTRIP_PREF = "browser.uiCustomization.horizontalTabstrip";
const DENSITY_PREF = "browser.uidensity";

const UBO_WIDGET_ID = "ublock0_raymondhill_net-browser-action";

// Left of the address bar, in this order. The sidebar button shows with horizontal
// tabs only (cast.css).
const LEADING = [
  "sidebar-button",
  "back-button",
  "forward-button",
  "stop-reload-button",
];
// Right of it, in this order (uBlock Origin is placed when its add-on starts, see
// WhisperUI; downloads hides itself while empty, browser.download.autohideButton).
const TRAILING = [
  UBO_WIDGET_ID,
  "unified-extensions-button",
  "downloads-button",
  // Private windows only: ends the private session.
  "reset-pbm-toolbar-button",
];
// Not in the default layout (they stay in Customize toolbar).
const REMOVED = [
  "home-button",
  "firefox-view-button",
  "fxa-toolbar-menu-button",
  "alltabs-button",
  "new-tab-button",
  "smartwindow-group-tabs-button",
  "ai-window-toggle",
  "save-to-pocket-button",
  "ipprotection-button",
];

export const WhisperToolbar = {
  _observing: false,

  /** Once per run, before the first window. */
  init() {
    this._migrateWhenReady();
    this._applyDensity();
    if (!this._observing) {
      this._observing = true;
      lazy.WhisperLayout.addListener((state, changed) =>
        this._onLayoutChanged(state, changed)
      );
    }
  },

  /**
   * The toolbar's placements can only change once CustomizableUI has restored them,
   * which it does when the first window registers its toolbar, before that window
   * first paints.
   */
  _migrateWhenReady() {
    if (Services.prefs.getIntPref(LAYOUT_VERSION_PREF, 0) >= LAYOUT_VERSION) {
      return;
    }
    const CUI = lazy.CustomizableUI;
    try {
      CUI.getWidgetIdsInArea(CUI.AREA_NAVBAR);
      this.migrate();
      return;
    } catch (e) {
      // "Area nav-bar not yet restored": wait for the first window's toolbar.
    }
    const listener = {
      onAreaNodeRegistered: area => {
        if (area != CUI.AREA_NAVBAR) {
          return;
        }
        CUI.removeListener(listener);
        try {
          this.migrate();
        } catch (e) {
          console.error("Whisper: toolbar layout failed", e);
        }
      },
    };
    CUI.addListener(listener);
  },

  /** The one-time layout (see the top of this file). */
  migrate() {
    if (Services.prefs.getIntPref(LAYOUT_VERSION_PREF, 0) >= LAYOUT_VERSION) {
      return;
    }
    const CUI = lazy.CustomizableUI;
    const NAVBAR = CUI.AREA_NAVBAR;
    CUI.beginBatchUpdate();
    try {
      for (const id of CUI.getWidgetIdsInArea(NAVBAR)) {
        const special = CUI.isSpecialWidget(id) && id.includes("spring");
        if ((REMOVED.includes(id) || special) && CUI.isWidgetRemovable(id)) {
          CUI.removeWidgetFromArea(id);
        }
      }
      let position = 0;
      for (const id of LEADING) {
        CUI.addWidgetToArea(id, NAVBAR, position++);
      }
      for (const id of TRAILING) {
        const placement = CUI.getPlacementOfWidget(id, false);
        if (
          (id == UBO_WIDGET_ID || id == "reset-pbm-toolbar-button") &&
          !placement
        ) {
          continue; // uBlock Origin: WhisperUI pins it when the add-on starts
        }
        CUI.addWidgetToArea(id, NAVBAR);
      }
      // The tab strip for horizontal tabs: the tabs and the new tab button.
      if (this._vertical) {
        Services.prefs.setStringPref(
          HORIZONTAL_TABSTRIP_PREF,
          JSON.stringify(["tabbrowser-tabs", "new-tab-button"])
        );
      } else {
        for (const id of CUI.getWidgetIdsInArea(CUI.AREA_TABSTRIP)) {
          if (
            id != "tabbrowser-tabs" &&
            id != "new-tab-button" &&
            CUI.isWidgetRemovable(id) &&
            !CUI.isWebExtensionWidget(id)
          ) {
            CUI.removeWidgetFromArea(id);
          }
        }
      }
    } finally {
      CUI.endBatchUpdate();
    }
    Services.prefs.setIntPref(LAYOUT_VERSION_PREF, LAYOUT_VERSION);
    console.log("Whisper: toolbar layout", CUI.getWidgetIdsInArea(NAVBAR));
  },

  get _vertical() {
    return Services.prefs.getBoolPref("sidebar.verticalTabs", false);
  },

  _onLayoutChanged(state, changed) {
    if (changed.includes("toolbarStyle")) {
      this._applyDensity();
    }
  },

  _applyDensity() {
    const want =
      lazy.WhisperLayout.state.toolbarStyle == "compact"
        ? 1 /* compact */
        : 0; /* normal */
    if (Services.prefs.getIntPref(DENSITY_PREF, 0) != want) {
      Services.prefs.setIntPref(DENSITY_PREF, want);
    }
  },
};
