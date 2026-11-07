/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Opens Whisper's guard dialog (content/guard-dialog.xhtml) and waits for the answer.
 *
 * Over the tab when the request comes from a page (tab-modal, the way Firefox asks
 * before opening an external program: the page stays behind it and can neither draw
 * over it nor reach it), otherwise over the browser window (an add-on's background
 * page, for native messaging).
 *
 * The dialog writes its answer into `args.result`; closing it any other way (Escape,
 * the tab closing) leaves null, which every caller treats as "no".
 */

const URL = "chrome://whisper/content/security/guard-dialog.xhtml";

export const WhisperGuardDialog = {
  URL,

  /**
   * @param {BrowsingContext|null} browsingContext where the request came from
   * @param {string} kind "seed", "address", "protocol" or "native-messaging"
   * @param {object} info what the dialog shows (plain values only)
   * @returns {Promise<string|null>}
   */
  async open(browsingContext, kind, info) {
    const args = { kind, info, result: null };
    // As ContentDispatchChooser finds the tab of a request.
    const browser = browsingContext?.topFrameElement;
    const win = browsingContext?.topChromeWindow;
    if (browser?.tagName == "browser" && win?.gBrowser?.getTabDialogBox) {
      const { closedPromise } = win.gBrowser
        .getTabDialogBox(browser)
        .open(URL, { features: "resizable=no", allowDuplicateDialogs: false }, args);
      await closedPromise;
      return args.result;
    }
    const top = Services.wm.getMostRecentWindow("navigator:browser");
    if (!top?.gDialogBox) {
      return null;
    }
    await top.gDialogBox.open(URL, args);
    return args.result;
  },
};
