/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Whisper's ClickFix protection — the settings section.
 *
 * Self-contained so it can drop into the "Whisper" preferences pane the other agent is
 * building. To mount it, from that pane's setup code (where Preferences and the
 * SettingGroupManager are in scope, as in browser/components/preferences/config/):
 *
 *   import { registerClipguardSettings }
 *     from "chrome://whisper/content/settings/clipguard-settings.mjs";
 *   registerClipguardSettings(Preferences, SettingGroupManager);
 *
 * and put the group in the pane's markup and init list:
 *
 *   <html:setting-group groupid="whisperClipguard" data-category="paneWhisper" .../>
 *   initSettingGroup("whisperClipguard");
 *
 * The strings are in browser/whisper/whisper-clipguard.ftl (add that resource to the
 * pane). Until then, the three prefs (clipguard.mode, clipguard.user_copy_notice,
 * clipguard.strict_script_copy) are still live and changeable in about:config.
 *
 * The section's text is honest: it says what Whisper stops and, just as plainly, what it
 * cannot stop.
 */

/** The three ClickFix settings and the group that shows them. */
export const clipguardGroupConfig = {
  l10nId: "clipguard-settings-group",
  headingLevel: 2,
  items: [
    {
      id: "clipguardMode",
      l10nId: "clipguard-settings-mode",
      control: "moz-radio-group",
      options: [
        { value: 0, l10nId: "clipguard-settings-mode-block" },
        { value: 1, l10nId: "clipguard-settings-mode-warn" },
        { value: 2, l10nId: "clipguard-settings-mode-off" },
      ],
    },
    {
      id: "clipguardUserCopyNotice",
      l10nId: "clipguard-settings-user-copy-notice",
    },
    {
      id: "clipguardStrictScriptCopy",
      l10nId: "clipguard-settings-strict-script-copy",
    },
    {
      id: "clipguardLimits",
      l10nId: "clipguard-settings-limits",
      control: "moz-message-bar",
      controlAttrs: { type: "info", role: "note" },
    },
  ],
};

/**
 * Registers the settings and the group. `Preferences` and `settingGroupManager` are the
 * about:preferences singletons.
 */
export function registerClipguardSettings(Preferences, settingGroupManager) {
  Preferences.addSetting({ id: "clipguardMode", pref: "clipguard.mode" });
  Preferences.addSetting({
    id: "clipguardUserCopyNotice",
    pref: "clipguard.user_copy_notice",
  });
  Preferences.addSetting({
    id: "clipguardStrictScriptCopy",
    pref: "clipguard.strict_script_copy",
  });
  Preferences.addSetting({ id: "clipguardLimits" });
  if (!settingGroupManager.has("whisperClipguard")) {
    settingGroupManager.registerGroup("whisperClipguard", clipguardGroupConfig);
  }
}
