/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The about dialog (Help > About Whisper), in Whisper's look.
 *
 * Firefox's dialog shows the application version, which in Whisper stays Firefox's
 * (159.0.1) so that web pages and add-ons see Firefox. WhisperUI calls init() when the
 * dialog has loaded, and Whisper rearranges it around its own version:
 *
 *   [logo]  Whisper 0.2.0  [Canary]
 *           64-bit · Built November 6, 2026 · What's new
 *           Engine: Firefox 159.0.1
 *           <Firefox's update state and its button, e.g. "Check for updates">
 *           Whisper is not affiliated with or endorsed by Mozilla. Firefox is a
 *           trademark of the Mozilla Foundation.
 *           ─────────
 *           Licensing Information
 *
 * The update part is Firefox's own (aboutDialog.js and AppUpdater drive it); only its
 * look changes. The channel shows as the small pill next to the version instead of
 * Firefox's "You are currently on the ... update channel" line. "What's new" opens
 * this version's release notes on GitHub; Whisper Canary, which has a new build for
 * every change, has none.
 *
 * Its look comes from chrome://whisper/content/aboutDialog.css; the branding folder's
 * aboutDialog.css is Firefox's own (web pages can load that one).
 *
 * Whisper's version is the locked pref whisper.version, which gecko/scripts/prepare.sh
 * writes from gecko/release.env. Firefox's links to Mozilla's help and feedback pages,
 * which come with the update section, are hidden: Whisper is not Mozilla's.
 */

import { AppConstants } from "resource://gre/modules/AppConstants.sys.mjs";

const VERSION_PREF = "whisper.version";
const STYLE = "chrome://whisper/content/aboutDialog.css";
const RELEASE_NOTES =
  "https://github.com/footprintsntrails/whisper-browser/releases/tag/v";

/**
 * The build date from the build ID (YYYYMMDDhhmmss, UTC), in the dialog's language.
 *
 * @param {string} buildID
 */
function buildDate(buildID) {
  const m = /^(\d{4})(\d{2})(\d{2})/.exec(buildID ?? "");
  if (!m) {
    return "";
  }
  const date = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3]));
  return new Intl.DateTimeFormat(Services.locale.appLocalesAsBCP47, {
    dateStyle: "long",
    timeZone: "UTC",
  }).format(date);
}

export const WhisperAbout = {
  init(win) {
    const doc = win.document;
    const version = Services.prefs
      .getDefaultBranch("")
      .getStringPref(VERSION_PREF, "");
    if (!version) {
      return;
    }
    doc.l10n.addResourceIds(["whisper/whisper-about.ftl"]);
    win.windowUtils.loadSheetUsingURIString(
      STYLE,
      Ci.nsIDOMWindowUtils.AUTHOR_SHEET
    );
    const canary = AppConstants.MOZ_UPDATE_CHANNEL == "canary";

    // The heading: the version, and the channel as a pill.
    const field = doc.getElementById("version");
    const versionRow = field.parentNode;
    doc.l10n.setAttributes(field, "whisper-about-title", { version });
    const channel = doc.createXULElement("label");
    channel.id = "whisperChannel";
    channel.toggleAttribute("canary", canary);
    doc.l10n.setAttributes(
      channel,
      canary ? "whisper-about-channel-canary" : "whisper-about-channel-release"
    );
    const header = doc.createXULElement("hbox");
    header.id = "whisperAboutHeader";
    header.setAttribute("align", "center");
    header.append(field, channel);

    // Bits, build date and, for a release, its release notes.
    const meta = doc.createXULElement("hbox");
    meta.id = "whisperAboutMeta";
    meta.setAttribute("align", "baseline");
    const metaText = doc.createXULElement("label");
    metaText.id = "whisperBuild";
    const arch = Services.sysinfo.get("arch");
    const date = buildDate(Services.appinfo.appBuildID);
    if (["x86", "x86-64"].includes(arch)) {
      doc.l10n.setAttributes(metaText, "whisper-about-build", {
        bits: Services.appinfo.is64Bit ? 64 : 32,
        date,
      });
    } else {
      doc.l10n.setAttributes(metaText, "whisper-about-build-arch", {
        arch,
        date,
      });
    }
    meta.append(metaText);
    const notes = doc.getElementById("releasenotes");
    if (notes) {
      if (canary) {
        notes.remove();
      } else {
        notes.href = RELEASE_NOTES + version;
        notes.hidden = false;
        meta.append(notes);
      }
    }

    const basedOn = doc.createXULElement("label");
    basedOn.id = "whisperBasedOn";
    doc.l10n.setAttributes(basedOn, "whisper-about-engine", {
      firefoxVersion: Services.appinfo.version,
    });

    // As visible as the version lines: Whisper is not Mozilla's.
    const notMozilla = doc.createXULElement("description");
    notMozilla.id = "whisperNotMozilla";
    doc.l10n.setAttributes(notMozilla, "whisper-about-not-mozilla");

    const rightBox = doc.getElementById("rightBox");
    const lines = doc.createXULElement("vbox");
    lines.id = "whisperAboutLines";
    lines.append(header, meta, basedOn);
    rightBox.prepend(lines);
    doc.getElementById("detailsBox")?.after(notMozilla);
    // Firefox's version row is empty now; its channel line is the pill.
    versionRow.hidden = true;
    const channelText = doc.getElementById("currentChannelText");
    if (channelText) {
      channelText.hidden = true;
    }
    const root = doc.documentElement;
    root.setAttribute(
      "aria-describedby",
      `${root.getAttribute("aria-describedby") ?? ""} whisperBuild whisperBasedOn whisperNotMozilla`.trim()
    );

    const checkAgain = this._addCheckAgain(win);

    for (const id of ["aboutDialogHelpLink", "submit-feedback"]) {
      const link = doc.getElementById(id);
      if (link) {
        link.hidden = true;
        link.parentNode.hidden = true;
      }
    }

    // The dialog sized itself to its content before these lines came in.
    doc.l10n
      .translateElements(
        [field, channel, metaText, basedOn, notMozilla, checkAgain].filter(Boolean)
      )
      .then(() => win.requestAnimationFrame(() => win.sizeToContent()))
      .catch(e => console.error("Whisper: about dialog size", e));
  },

  /**
   * Firefox offers its "Check for updates" button only before the first check. After
   * "up to date" or a failed check, Whisper's own button checks again, through the
   * dialog's updater (gAppUpdater.checkForUpdates()).
   */
  _addCheckAgain(win) {
    const doc = win.document;
    const deck = doc.getElementById("updateDeck");
    const box = doc.getElementById("updateBox");
    if (!deck || !box || !win.gAppUpdater) {
      return null;
    }
    const button = doc.createXULElement("button");
    button.id = "whisperCheckAgain";
    doc.l10n.setAttributes(button, "whisper-about-check-again");
    button.addEventListener("command", () => win.gAppUpdater.checkForUpdates());
    box.append(button);
    const update = () => {
      const panel = deck.selectedPanel?.id;
      button.hidden = !["noUpdatesFound", "checkingFailed"].includes(panel);
    };
    new win.MutationObserver(update).observe(deck, {
      attributes: true,
      attributeFilter: ["selectedIndex"],
    });
    update();
    return button;
  },
};
