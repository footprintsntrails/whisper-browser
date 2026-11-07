/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Icons for pages that have none: a monogram, the first letter of the site's
 * registrable domain on a small tile whose muted colour comes from the domain, so a
 * site keeps its tile everywhere (tab sidebar, new tab page). Firefox shows its globe
 * there; addresses without a domain (an IP address, a file, a browser page) keep it.
 *
 * In the tab sidebar the tab carries the letter and the hue (the attribute
 * whisper-mono and two custom properties) and cast.css draws the tile where Firefox
 * would draw the globe, only while the tab has no icon of its own and is not loading.
 * The pref whisper.tabs.monograms turns them off (Firefox's globe again).
 *
 * Only public pieces of Firefox: the tab container's events and a tabs progress
 * listener (gBrowser.addTabsProgressListener).
 */

/**
 * FNV-1a, 32 bits: a stable number for a domain.
 *
 * @param {string} text
 */
function hash(text) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

/**
 * The monogram for an address, or null when it has no domain to name.
 *
 * @param {nsIURI|string} uriOrSpec
 * @returns {{letter: string, hue: number, domain: string}|null}
 */
export function monogramFor(uriOrSpec) {
  let uri;
  try {
    uri =
      typeof uriOrSpec == "string" ? Services.io.newURI(uriOrSpec) : uriOrSpec;
  } catch (e) {
    return null;
  }
  if (!uri || !["http", "https"].includes(uri.scheme)) {
    return null;
  }
  let host;
  try {
    host = uri.asciiHost;
  } catch (e) {
    return null;
  }
  if (!host) {
    return null;
  }
  let domain;
  try {
    domain = Services.eTLD.getBaseDomainFromHost(host);
  } catch (e) {
    if (e.result == Cr.NS_ERROR_HOST_IS_IP_ADDRESS) {
      return null;
    }
    // A name without a public suffix (localhost, an intranet host) is its own domain.
    domain = host;
  }
  // The letter as people read the domain (an internationalized name in its own
  // script, where Firefox would show it so).
  let shown = domain;
  try {
    shown = Services.io.newURI(`http://${domain}/`).displayHost || domain;
  } catch (e) {}
  const letter = [...shown][0]?.toLocaleUpperCase();
  if (!letter || !/[\p{L}\p{N}]/u.test(letter)) {
    return null;
  }
  return { letter, hue: hash(domain) % 360, domain };
}

const EVENTS = ["TabOpen", "TabAttrModified", "SSTabRestored"];
const PREF = "whisper.tabs.monograms";

export const WhisperTabIcons = {
  initWindow(win) {
    const gBrowser = win.gBrowser;
    const container = gBrowser?.tabContainer;
    if (!container || win.whisperTabIcons) {
      return;
    }
    const onEvent = event => {
      if (
        event.type == "TabAttrModified" &&
        !event.detail?.changed?.some(a => a == "image" || a == "label")
      ) {
        return;
      }
      this.updateTab(event.target);
    };
    const progress = {
      onLocationChange: browser => {
        const tab = gBrowser.getTabForBrowser(browser);
        if (tab) {
          this.updateTab(tab);
        }
      },
    };
    for (const type of EVENTS) {
      container.addEventListener(type, onEvent);
    }
    gBrowser.addTabsProgressListener(progress);
    const prefObserver = () => {
      for (const tab of gBrowser.tabs) {
        this.updateTab(tab);
      }
    };
    Services.prefs.addObserver(PREF, prefObserver);
    win.whisperTabIcons = () => {
      for (const type of EVENTS) {
        container.removeEventListener(type, onEvent);
      }
      gBrowser.removeTabsProgressListener(progress);
      Services.prefs.removeObserver(PREF, prefObserver);
    };
    for (const tab of gBrowser.tabs) {
      this.updateTab(tab);
    }
  },

  uninitWindow(win) {
    win.whisperTabIcons?.();
    delete win.whisperTabIcons;
  },

  /** @param {MozTabbrowserTab} tab */
  updateTab(tab) {
    let mono = null;
    if (Services.prefs.getBoolPref(PREF, true)) {
      try {
        mono = monogramFor(tab.linkedBrowser?.currentURI);
      } catch (e) {}
    }
    if (!mono) {
      if (tab.hasAttribute("whisper-mono")) {
        tab.removeAttribute("whisper-mono");
        tab.style.removeProperty("--whisper-mono");
        tab.style.removeProperty("--whisper-mono-hue");
      }
      return;
    }
    if (
      tab.getAttribute("whisper-mono") == mono.letter &&
      tab.style.getPropertyValue("--whisper-mono-hue") == String(mono.hue)
    ) {
      return;
    }
    tab.setAttribute("whisper-mono", mono.letter);
    // A CSS string for `content` (letters and digits only, see monogramFor).
    tab.style.setProperty("--whisper-mono", `"${mono.letter}"`);
    tab.style.setProperty("--whisper-mono-hue", String(mono.hue));
  },
};
