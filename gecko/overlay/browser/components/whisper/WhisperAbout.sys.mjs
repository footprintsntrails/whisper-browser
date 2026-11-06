/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The about dialog (Help > About Whisper).
 *
 * Firefox's dialog shows the application version, which in Whisper stays Firefox's
 * (159.0.1) so that web pages and add-ons see Firefox. aboutDialog.js calls init()
 * right after it has written that line (patches/0024-about-dialog-whisper-version.patch),
 * and Whisper replaces it with its own version and the Firefox version it is based on:
 *
 *   Whisper 0.1.0 (64-bit)
 *   Based on Firefox 159.0.1
 *
 * Whisper's version is the locked pref whisper.version, which gecko/scripts/prepare.sh
 * writes from gecko/release.env. Firefox's links to Mozilla's help and feedback pages,
 * which come with the update section, are hidden: Whisper is not Mozilla's.
 */

import { AppConstants } from "resource://gre/modules/AppConstants.sys.mjs";

const VERSION_PREF = "whisper.version";

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

    const field = doc.getElementById("version");
    const arch = Services.sysinfo.get("arch");
    if (["x86", "x86-64"].includes(arch)) {
      doc.l10n.setAttributes(field, "whisper-about-version", {
        version,
        bits: Services.appinfo.is64Bit ? 64 : 32,
      });
    } else {
      doc.l10n.setAttributes(field, "whisper-about-version-arch", {
        version,
        arch,
      });
    }

    const basedOn = doc.createXULElement("label");
    basedOn.id = "whisperBasedOn";
    basedOn.className = "text-blurb";
    // Whisper Canary has a new build for every change under the same version, so it
    // also says which build it is.
    if (AppConstants.MOZ_UPDATE_CHANNEL == "canary") {
      doc.l10n.setAttributes(basedOn, "whisper-about-based-on-build", {
        firefoxVersion: Services.appinfo.version,
        buildID: Services.appinfo.appBuildID,
      });
    } else {
      doc.l10n.setAttributes(basedOn, "whisper-about-based-on", {
        firefoxVersion: Services.appinfo.version,
      });
    }
    // After the line with the version and the "What's new" link.
    field.parentNode.after(basedOn);
    const root = doc.documentElement;
    root.setAttribute(
      "aria-describedby",
      `${root.getAttribute("aria-describedby") ?? ""} whisperBasedOn`.trim()
    );

    for (const id of ["aboutDialogHelpLink", "submit-feedback"]) {
      const link = doc.getElementById(id);
      if (link) {
        link.hidden = true;
        link.parentNode.hidden = true;
      }
    }
  },
};
