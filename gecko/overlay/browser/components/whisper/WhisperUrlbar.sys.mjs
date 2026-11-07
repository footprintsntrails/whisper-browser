/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The address bar's results panel in Whisper's look (cast.css draws it):
 *
 *   - rows grouped under small labels: Open tabs, Bookmarks, History, Search;
 *   - the search engines as named chips at the bottom (Firefox's one-off buttons;
 *     Tab moves between them), under the hint "Tab — switch engine".
 *
 * Firefox builds the rows and buttons; this module only labels them. It watches the
 * panel's DOM (a MutationObserver, no Firefox method is replaced) and sets
 * data-whisper-group / data-whisper-label on rows and a label on each engine button.
 */

// UrlbarShared.RESULT_SOURCE and RESULT_TYPE (browser/components/urlbar/content).
const RESULT_SOURCE = {
  BOOKMARKS: 1,
  HISTORY: 2,
  SEARCH: 3,
  TABS: 4,
};
const RESULT_TYPE_TAB_SWITCH = 1;

function groupOf(result) {
  if (!result) {
    return null;
  }
  if (result.type == RESULT_TYPE_TAB_SWITCH || result.source == RESULT_SOURCE.TABS) {
    return "tabs";
  }
  switch (result.source) {
    case RESULT_SOURCE.BOOKMARKS:
      return "bookmarks";
    case RESULT_SOURCE.HISTORY:
      return "history";
    case RESULT_SOURCE.SEARCH:
      return "search";
  }
  return null;
}

export const WhisperUrlbar = {
  _strings: null,

  async initWindow(win) {
    const urlbar = win.gURLBar;
    const panel = urlbar?.view?.panel;
    if (!panel || win.whisperUrlbarObserver) {
      return;
    }
    if (!this._strings) {
      const l10n = new Localization(["whisper/whisper.ftl"]);
      const [tabs, bookmarks, history, search, hint] = await l10n.formatValues([
        "whisper-urlbar-group-tabs",
        "whisper-urlbar-group-bookmarks",
        "whisper-urlbar-group-history",
        "whisper-urlbar-group-search",
        "whisper-urlbar-engines-hint",
      ]);
      this._strings = { tabs, bookmarks, history, search, hint };
    }
    let scheduled = false;
    const update = () => {
      scheduled = false;
      this._labelRows(panel);
      this._labelEngines(panel);
    };
    const observer = new win.MutationObserver(() => {
      if (!scheduled) {
        scheduled = true;
        win.requestAnimationFrame(update);
      }
    });
    observer.observe(panel, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["type", "label", "hidden", "tooltiptext"],
    });
    win.whisperUrlbarObserver = observer;
    update();
  },

  uninitWindow(win) {
    win.whisperUrlbarObserver?.disconnect();
    delete win.whisperUrlbarObserver;
  },

  /** The first row of each group carries the group's label. */
  _labelRows(panel) {
    let previous = null;
    for (const row of panel.querySelectorAll(".urlbarView-row")) {
      if (row.hidden || row.offsetParent === null) {
        continue;
      }
      const group = groupOf(row.result);
      if (group) {
        row.setAttribute("data-whisper-group", group);
      } else {
        row.removeAttribute("data-whisper-group");
      }
      const label = group && group != previous ? this._strings[group] : null;
      if (label) {
        row.setAttribute("data-whisper-label", label);
      } else {
        row.removeAttribute("data-whisper-label");
      }
      previous = group;
    }
  },

  /** Engine buttons get their engine's name; the header says how to switch. */
  _labelEngines(panel) {
    const header = panel.querySelector(".search-panel-one-offs-header-label");
    if (header && header.getAttribute("data-whisper-hint") != this._strings.hint) {
      header.setAttribute("data-whisper-hint", this._strings.hint);
    }
    for (const button of panel.querySelectorAll(
      ".search-panel-one-offs .searchbar-engine-one-off-item"
    )) {
      // "DuckDuckGo (@duckduckgo)" -> "DuckDuckGo"
      const name = (button.getAttribute("tooltiptext") ?? "").replace(
        /\s*\([^)]*\)\s*$/,
        ""
      );
      if (name && button.getAttribute("data-whisper-name") != name) {
        button.setAttribute("data-whisper-name", name);
        button.setAttribute("label", name);
      }
    }
  },
};
