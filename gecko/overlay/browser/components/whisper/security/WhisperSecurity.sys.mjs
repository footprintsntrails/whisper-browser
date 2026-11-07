/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Entry point of Whisper's security features (WhisperSecurity.manifest):
 *
 *   browser-before-ui-startup      WhisperSecurity.init()                 once per run
 *   browser-window-delayed-startup WhisperSecurity.onWindowDelayedStartup(win)
 *   browser-window-unload          WhisperSecurity.onWindowUnload(win)
 *
 * As in WhisperUI: each feature starts on its own, and one that fails is logged and
 * leaves the others running. Every feature has its own pref to switch it off.
 */

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  WhisperDownloadGuard:
    "moz-src:///browser/components/whisper/security/WhisperDownloadGuard.sys.mjs",
  WhisperAddressMemory:
    "moz-src:///browser/components/whisper/security/WhisperAddressMemory.sys.mjs",
});

const MODULES = "moz-src:///browser/components/whisper/security/";

function guarded(name, fn) {
  try {
    const result = fn();
    if (result && typeof result.catch == "function") {
      result.catch(e => console.error(`Whisper security: ${name} failed`, e));
    }
  } catch (e) {
    console.error(`Whisper security: ${name} failed`, e);
  }
}

export const WhisperSecurity = {
  _initialized: false,

  init() {
    if (this._initialized) {
      return;
    }
    this._initialized = true;
    guarded("input guard", () => this._registerInputGuard());
    guarded("download check", () => lazy.WhisperDownloadGuard.init());
  },

  onWindowDelayedStartup(_win) {},

  onWindowUnload(_win) {},

  /**
   * Recovery phrases and swapped addresses (WhisperInputGuardChild.sys.mjs explains the
   * event order). Tabs only ("browsers" group): add-on pages and popups are not
   * touched, so wallet extensions keep working; the child also skips the browser's own
   * pages.
   */
  _registerInputGuard() {
    ChromeUtils.registerWindowActor("WhisperInputGuard", {
      parent: { esModuleURI: `${MODULES}WhisperInputGuardParent.sys.mjs` },
      child: {
        esModuleURI: `${MODULES}WhisperInputGuardChild.sys.mjs`,
        events: {
          // Capturing on the window root: before every listener of the page.
          paste: { capture: true },
          drop: { capture: true },
          beforeinput: { capture: true },
          // System group: after the page's handlers, even when one stopped the event,
          // to see what really goes to the clipboard.
          copy: { mozSystemGroup: true },
          cut: { mozSystemGroup: true },
        },
      },
      allFrames: true,
      messageManagerGroups: ["browsers"],
      // Firefox 159 only creates actors in web content processes when they say so
      // (dom.jsipc.check_safeForUntrustedWebProcess). This one may: its parent treats
      // every message as coming from a compromised page (WhisperInputGuardParent).
      safeForUntrustedWebProcess: true,
    });
  },

  /** For Panic and Burn: what the input guards hold in memory. */
  forgetInputGuardState() {
    lazy.WhisperAddressMemory.forget();
    lazy.WhisperDownloadGuard.forgetSession();
  },
};
