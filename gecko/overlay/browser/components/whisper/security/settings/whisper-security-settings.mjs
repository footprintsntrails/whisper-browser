/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Settings for Whisper's security features, as a fragment for the Whisper pane of
 * about:preferences (the pane is the design track's; this file only registers setting
 * groups, Firefox's own config format for the redesigned settings).
 *
 * The pane loads it as a config module and names the groups, for example:
 *
 *   whisper: {
 *     l10nId: "...",
 *     groupIds: ["whisperRecoveryPhrase", "whisperSwappedAddress", "whisperDownloads",
 *                "whisperPrograms", "whisperCleanLinks"],
 *     module: "chrome://whisper/content/security/settings/whisper-security-settings.mjs",
 *   },
 *
 * and links "whisper/whisper-security-settings.ftl" in preferences.xhtml.
 *
 * Where each group belongs in the Whisper pane:
 *   whisperRecoveryPhrase, whisperSwappedAddress  "Crypto" (next to the wallet tools)
 *   whisperDownloads, whisperPrograms             "Files and programs"
 *   whisperCleanLinks                             "Privacy", with tracking protection
 *
 * Every text says what the feature does and what it cannot do; none says "you are
 * protected". A locked pref shows its checkbox disabled.
 */

import { SettingGroupManager } from "chrome://browser/content/preferences/config/SettingGroupManager.mjs";
import { Preferences } from "chrome://global/content/preferences/Preferences.mjs";

const SETTINGS = {
  whisperSeedGuard: "whisper.security.seedguard.enabled",
  whisperAddressGuard: "whisper.security.addressguard.enabled",
  whisperDownloadCheck: "whisper.security.downloads.enabled",
  whisperDownloadPrograms: "whisper.security.downloads.hold_programs",
  whisperProtocolGuard: "whisper.security.protocols.enabled",
  whisperNativeMessagingAsk: "whisper.security.nativemessaging.ask",
  whisperQueryStripping: "privacy.query_stripping.enabled",
  whisperCopyLinkClean: "whisper.cleanlinks.copy_link_clean",
};

// A sub-option is shown while its feature is on.
const PARENTS = {
  whisperDownloadPrograms: "whisperDownloadCheck",
  whisperCopyLinkClean: "whisperQueryStripping",
};

for (const [id, pref] of Object.entries(SETTINGS)) {
  // A setting reads its pref through a Preference, which another pane may have added.
  if (!Preferences.get(pref)) {
    Preferences.add({ id: pref, type: "bool" });
  }
  const parent = PARENTS[id];
  Preferences.addSetting(
    parent ? { id, pref, deps: [parent], visible: deps => deps[parent].value } : { id, pref }
  );
}

// A note under a group's switches; every item needs a setting, even one without a pref.
function limits(id, l10nId) {
  Preferences.addSetting({ id });
  return { id, l10nId, control: "moz-message-bar", controlAttrs: { type: "info" } };
}

SettingGroupManager.registerGroups({
  whisperRecoveryPhrase: {
    l10nId: "whisper-settings-seed-group",
    headingLevel: 2,
    items: [
      { id: "whisperSeedGuard", l10nId: "whisper-settings-seed-enabled" },
      limits("whisperSeedGuardLimits", "whisper-settings-seed-limits"),
    ],
  },
  whisperSwappedAddress: {
    l10nId: "whisper-settings-address-group",
    headingLevel: 2,
    items: [
      { id: "whisperAddressGuard", l10nId: "whisper-settings-address-enabled" },
      limits("whisperAddressGuardLimits", "whisper-settings-address-limits"),
    ],
  },
  whisperDownloads: {
    l10nId: "whisper-settings-downloads-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperDownloadCheck",
        l10nId: "whisper-settings-downloads-enabled",
        items: [{ id: "whisperDownloadPrograms", l10nId: "whisper-settings-downloads-programs" }],
      },
      limits("whisperDownloadLimits", "whisper-settings-downloads-limits"),
    ],
  },
  whisperPrograms: {
    l10nId: "whisper-settings-programs-group",
    headingLevel: 2,
    items: [
      { id: "whisperProtocolGuard", l10nId: "whisper-settings-programs-enabled" },
      { id: "whisperNativeMessagingAsk", l10nId: "whisper-settings-programs-nm" },
      limits("whisperProgramsLimits", "whisper-settings-programs-limits"),
    ],
  },
  whisperCleanLinks: {
    l10nId: "whisper-settings-cleanlinks-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperQueryStripping",
        l10nId: "whisper-settings-cleanlinks-enabled",
        items: [{ id: "whisperCopyLinkClean", l10nId: "whisper-settings-cleanlinks-copy" }],
      },
      limits("whisperCleanLinksLimits", "whisper-settings-cleanlinks-limits"),
    ],
  },
});
