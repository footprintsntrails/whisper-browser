/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Whisper's ClickFix protection: the browser-interface half.
 *
 * The decision is made in the parent process by nsBaseClipboard and the clipguard Rust
 * crate (toolkit/components/clipguard). When a page puts a command on the clipboard, that
 * code notifies "clipguard-event" observers with an nsIClipguardEvent, and this module:
 *
 *   - for a held write that needs a visibility check, asks the page (through the
 *     WhisperClipguard window actor) which of the command's words are shown on screen,
 *     and hands the answer back to the parent, which then blocks or allows the write;
 *   - for a blocked write, shows a doorhanger that explains what happened and offers
 *     "Close this tab", "Keep blocked", "Show what it tried to copy", and the two
 *     bypasses: "Copy anyway" (hold for 3 seconds) and "Always allow commands from this
 *     site" (this session only, through the permission manager);
 *   - for an allowed write that is still worth a word, shows the right notice: an
 *     address-bar icon for a LOW command, a one-time panel for an unknown installer or a
 *     command that looks like malware, or "This page changed what you copied" when a page
 *     handler replaced a selection.
 *
 * The interface never says "you're protected". Whisper stops a page from silently placing
 * a command you can't see; a command you type by hand, a second device and other browsers
 * are still possible, and the settings text says so.
 */

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  PrivateBrowsingUtils: "resource://gre/modules/PrivateBrowsingUtils.sys.mjs",
});

// nsIClipguardEvent constants, mirrored so the switch reads clearly.
const KIND_CHECK_VISIBILITY = 1;
const KIND_BLOCKED = 2;
const KIND_NOTICE = 3;
const NOTICE_INDICATOR = 1;
const NOTICE_INSTALLER = 2;
const NOTICE_COMMAND_WARNING = 3;
const NOTICE_SUBSTITUTED = 4;
const NOTICE_WARNED = 5;

// clipguard::Source
const SOURCE_USER_COPY = 1;

// "Copy anyway" makes the person hold the button this long, as Opera's Paste Protect
// does, so a reflexive click cannot undo the block.
const HOLD_TO_COPY_MS = 3000;

// A one-time panel (installer notice, command warning) is shown at most once per site per
// session; this remembers which origins already saw one.
// origin + "\n" + hosts  ->  the highest command level already announced for it, so a
// later, more dangerous command (or a new host) is never silently suppressed.
const sNoticedOrigins = new Map();

// "Always allow commands from this site": a session permission so it is forgotten when
// the browser closes (it is never written to permissions.sqlite).
const CLIPGUARD_PERMISSION = "clipguard-allow";

function log(...args) {
  console.log("Whisper ClickFix:", ...args);
}

export const WhisperClipguard = {
  _started: false,

  /** Once per run, before the first window. */
  init() {
    if (this._started) {
      return;
    }
    this._started = true;
    try {
      ChromeUtils.registerWindowActor("WhisperClipguard", {
        parent: {
          esModuleURI:
            "moz-src:///browser/components/whisper/WhisperClipguardParent.sys.mjs",
        },
        child: {
          esModuleURI:
            "moz-src:///browser/components/whisper/WhisperClipguardChild.sys.mjs",
        },
        allFrames: true,
        messageManagerGroups: ["browsers"],
      });
    } catch (e) {
      // Another registration (a reload of this module) already did it.
      if (e.name != "NotSupportedError") {
        console.error("Whisper ClickFix: could not register the actor", e);
      }
    }
    try {
      const sss = Cc["@mozilla.org/content/style-sheet-service;1"].getService(
        Ci.nsIStyleSheetService
      );
      const uri = Services.io.newURI(
        "chrome://browser/content/whisper/clipguard.css"
      );
      if (!sss.sheetRegistered(uri, sss.AGENT_SHEET)) {
        sss.loadAndRegisterSheet(uri, sss.AGENT_SHEET);
      }
    } catch (e) {
      console.error("Whisper ClickFix: could not load the icon stylesheet", e);
    }
    Services.obs.addObserver(this, "clipguard-event");
  },

  observe(subject, topic) {
    if (topic != "clipguard-event") {
      return;
    }
    const event = subject.QueryInterface(Ci.nsIClipguardEvent);
    switch (event.kind) {
      case KIND_CHECK_VISIBILITY:
        this._checkVisibility(event).catch(e =>
          console.error("Whisper ClickFix: visibility check failed", e)
        );
        break;
      case KIND_BLOCKED:
        this._showBlocked(event);
        break;
      case KIND_NOTICE:
        this._showNotice(event);
        break;
    }
  },

  /**
   * Ask the page which of the command's words are visible, then tell the parent. A page
   * that is gone, or that answers nothing, counts as "not on screen", so the write is
   * blocked -- the safe default.
   */
  async _checkVisibility(event) {
    const wgp = event.windowGlobal;
    let texts = [];
    if (wgp && wgp.isCurrentGlobal && !wgp.isClosed) {
      try {
        const actor = wgp.getActor("WhisperClipguard");
        texts = await actor.sendQuery("FindVisibleText", {
          probes: event.visibilityProbes,
        });
      } catch (e) {
        log("could not read the page for the visibility check:", e.message);
      }
    }
    event.resolveVisibility(texts || []);
  },

  /** The chrome window and <browser> that owns the writing page, or null. */
  _browserFor(event) {
    const wgp = event.windowGlobal;
    const browser = wgp?.browsingContext?.top?.embedderElement;
    const win = browser?.ownerGlobal;
    if (!win || win.closed || !win.PopupNotifications) {
      return null;
    }
    return { win, browser };
  },

  _origin(event) {
    try {
      return event.principal?.originNoSuffix || "";
    } catch (e) {
      return "";
    }
  },

  _originDisplay(event) {
    try {
      return event.principal?.isContentPrincipal
        ? event.principal.URI.displayHostPort || event.principal.originNoSuffix
        : event.principal?.originNoSuffix || "";
    } catch (e) {
      return "";
    }
  },

  _showBlocked(event) {
    const target = this._browserFor(event);
    if (!target) {
      return; // the tab is gone; the clipboard already holds the placeholder
    }
    const { win, browser } = target;
    const doc = win.document;
    const host = this._originDisplay(event);

    const mainAction = {
      // "Close this tab": the strongest and safest response, so it is the default
      // button and skips the click delay (it cannot grant anything).
      label: this._str(win, "clipguard-blocked-close-tab"),
      accessKey: this._str(win, "clipguard-blocked-close-tab-key"),
      disableSecurityDelay: true,
      callback: () => {
        win.gBrowser.removeTab(win.gBrowser.getTabForBrowser(browser), {
          animate: true,
        });
      },
    };

    const secondaryActions = [
      {
        // "Keep blocked": dismiss and drop the held text.
        label: this._str(win, "clipguard-blocked-keep"),
        accessKey: this._str(win, "clipguard-blocked-keep-key"),
        disableSecurityDelay: true,
        callback: () => event.discard(),
      },
      {
        // "Show what it tried to copy": reveals the display string in the panel.
        label: this._str(win, "clipguard-blocked-show"),
        accessKey: this._str(win, "clipguard-blocked-show-key"),
        dismiss: true,
        callback: () => this._showContents(win, event),
      },
      {
        // "Copy anyway (hold for 3 seconds)".
        label: this._str(win, "clipguard-blocked-copy-anyway"),
        accessKey: this._str(win, "clipguard-blocked-copy-anyway-key"),
        dismiss: true,
        callback: () => this._armHoldToCopy(win, event),
      },
      {
        // "Always allow commands from this site" (this session only).
        label: this._str(win, "clipguard-blocked-always-allow"),
        accessKey: this._str(win, "clipguard-blocked-always-allow-key"),
        callback: () => {
          this._allowSiteForSession(event);
          event.copyAnyway();
        },
      },
    ];

    const message = this._formatString(win, "clipguard-blocked-message", {
      origin: host || this._str(win, "clipguard-this-site"),
    });

    win.PopupNotifications.show(
      browser,
      "whisper-clipguard",
      message,
      null /* anchor: the default notification icon */,
      mainAction,
      secondaryActions,
      {
        persistent: true,
        hideClose: false,
        popupIconClass: "whisper-clipguard-icon",
        eventCallback: topic => {
          if (topic == "removed") {
            event.discard();
          }
        },
      }
    );
    void doc;
  },

  _showNotice(event) {
    const target = this._browserFor(event);
    if (!target) {
      return;
    }
    const { win, browser } = target;
    const origin = this._origin(event);

    if (event.notice == NOTICE_INDICATOR) {
      // LOW: a quiet address-bar icon, no panel. Shown dismissed so it is just the anchor.
      win.PopupNotifications.show(
        browser,
        "whisper-clipguard-indicator",
        this._str(win, "clipguard-indicator-message"),
        null,
        null,
        null,
        {
          dismissed: true,
          removeOnDismissal: true,
          popupIconClass: "whisper-clipguard-icon",
          hideClose: true,
        }
      );
      return;
    }

    // The louder notices are shown at most once per site per session -- but only while
    // the command is not more dangerous than one already announced, and from the same
    // hosts. A later command from the same origin at a HIGHER level, or from a different
    // host, shows again, so a first MED notice can never silence a later HIGH one.
    const once = event.notice == NOTICE_INSTALLER || event.notice == NOTICE_COMMAND_WARNING;
    if (once) {
      const key = origin + "\n" + (event.hosts || "");
      const shownLevel = sNoticedOrigins.get(key);
      if (shownLevel !== undefined && shownLevel >= event.level) {
        return;
      }
      sNoticedOrigins.set(key, Math.max(shownLevel ?? 0, event.level));
    }

    const message = this._noticeMessage(win, event);
    if (!message) {
      return;
    }
    win.PopupNotifications.show(
      browser,
      "whisper-clipguard-notice",
      message,
      null,
      {
        label: this._str(win, "clipguard-notice-ok"),
        accessKey: this._str(win, "clipguard-notice-ok-key"),
        disableSecurityDelay: true,
        callback: () => {},
      },
      [
        {
          label: this._str(win, "clipguard-notice-show"),
          accessKey: this._str(win, "clipguard-notice-show-key"),
          dismiss: true,
          callback: () => this._showContents(win, event),
        },
      ],
      {
        dismissed: false,
        popupIconClass: "whisper-clipguard-icon",
      }
    );
  },

  _noticeMessage(win, event) {
    switch (event.notice) {
      case NOTICE_SUBSTITUTED:
        return this._str(win, "clipguard-notice-substituted");
      case NOTICE_INSTALLER:
        return this._formatString(win, "clipguard-notice-installer", {
          host: event.hosts || this._str(win, "clipguard-this-site"),
        });
      case NOTICE_COMMAND_WARNING:
        return event.source == SOURCE_USER_COPY
          ? this._str(win, "clipguard-notice-user-command")
          : this._str(win, "clipguard-notice-command");
      case NOTICE_WARNED:
        return this._formatString(win, "clipguard-notice-warned", {
          origin: this._originDisplay(event) || this._str(win, "clipguard-this-site"),
        });
      default:
        return null;
    }
  },

  /** Shows what the page tried to copy: the first 400 characters in a monospace,
   * left-to-right view, with the hosts listed apart (clipguard-details.xhtml). */
  _showContents(win, event) {
    win.openDialog(
      "chrome://browser/content/whisper/clipguard-details.xhtml",
      "_blank",
      "chrome,dialog,centerscreen,resizable=yes",
      { text: event.displayText, hosts: event.hosts, level: event.level }
    );
  },

  /**
   * "Copy anyway": the panel's button dismissed the doorhanger; now show a small
   * hold-to-confirm affordance. Implemented as a follow-up panel whose primary button is
   * only enabled after the person has held it for 3 seconds.
   */
  _armHoldToCopy(win, event) {
    const target = this._browserFor(event);
    if (!target || !event.canCopyAnyway) {
      return;
    }
    win.PopupNotifications.show(
      target.browser,
      "whisper-clipguard-hold",
      this._str(win, "clipguard-hold-message"),
      null,
      {
        label: this._str(win, "clipguard-hold-button"),
        accessKey: this._str(win, "clipguard-hold-button-key"),
        callback: () => event.copyAnyway(),
      },
      [],
      {
        persistent: true,
        popupIconClass: "whisper-clipguard-icon",
        eventCallback: (topic, _n, panel) => {
          if (topic == "shown") {
            this._makeHoldButton(win, panel);
          }
        },
      }
    );
  },

  _makeHoldButton(win, panel) {
    const button = panel?.button;
    if (!button) {
      return;
    }
    button.disabled = true;
    let timer = null;
    const start = () => {
      timer = win.setTimeout(() => {
        button.disabled = false;
        button.focus();
      }, HOLD_TO_COPY_MS);
    };
    const cancel = () => {
      if (timer) {
        win.clearTimeout(timer);
        timer = null;
      }
      button.disabled = true;
    };
    button.addEventListener("mousedown", start);
    button.addEventListener("keydown", e => {
      if (e.key == " " || e.key == "Enter") {
        start();
      }
    });
    button.addEventListener("mouseup", cancel);
    button.addEventListener("mouseleave", cancel);
    button.addEventListener("keyup", cancel);
  },

  _allowSiteForSession(event) {
    const principal = event.principal;
    if (!principal || !principal.isContentPrincipal) {
      return;
    }
    try {
      Services.perms.addFromPrincipal(
        principal,
        CLIPGUARD_PERMISSION,
        Services.perms.ALLOW_ACTION,
        Services.perms.EXPIRE_SESSION
      );
      log("allowed commands from", this._originDisplay(event), "for this session");
    } catch (e) {
      console.error("Whisper ClickFix: could not add the session permission", e);
    }
  },

  _str(win, id) {
    return this._l10n(win).formatValueSync(id);
  },

  _formatString(win, id, args) {
    return this._l10n(win).formatValueSync(id, args);
  },

  _l10n(win) {
    if (!win._whisperClipguardL10n) {
      win._whisperClipguardL10n = new win.Localization(
        ["browser/whisper/whisper-clipguard.ftl"],
        true
      );
    }
    return win._whisperClipguardL10n;
  },

  QueryInterface: ChromeUtils.generateQI(["nsIObserver"]),
};
