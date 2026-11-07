/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * The setting groups of Settings' Whisper pane, in Firefox's own config format for
 * its redesigned settings (SettingGroupManager, Preferences.addSetting). Firefox
 * imports this module into the settings page when the pane is first shown; the pane
 * itself is registered by WhisperSettings.sys.mjs, which also lists the groups in
 * order (GROUPS there).
 *
 * Where Firefox already has the setting (default engine, search suggestions, the
 * add-engine dialog, the default browser state), the group reuses Firefox's setting
 * with Whisper's words; Firefox's search module is imported for that.
 *
 * Slots for groups other branches register (each module calls registerGroups for its
 * own ids; WhisperSettings lists them, and a group that is not registered is skipped):
 *   whisperClipguard   clipboard protection (clipguard-settings.mjs)
 *   whisperRecoveryPhrase, whisperSwappedAddress, whisperDownloads, whisperPrograms,
 *   whisperCleanLinks  the security features (whisper-security-settings.mjs)
 */

/* global gotoPref */

import { Preferences } from "chrome://global/content/preferences/Preferences.mjs";
import { SettingGroupManager } from "chrome://browser/content/preferences/config/SettingGroupManager.mjs";
// Firefox's search settings: defaultEngineNormal, suggestionsInSearchFieldsCheckbox,
// addEngineButton.
import "chrome://browser/content/preferences/config/search.mjs";

const lazy = {};
ChromeUtils.defineESModuleGetters(lazy, {
  MigrationUtils: "resource:///modules/MigrationUtils.sys.mjs",
});

const UBO_ID = "uBlock0@raymondhill.net";
const LIMITS_URL =
  "https://github.com/footprintsntrails/whisper-browser#honest-limits";
const NETWORK_URL =
  "https://github.com/footprintsntrails/whisper-browser#who-whisper-talks-to";

const browserWindow = () => window.browsingContext.topChromeWindow;

function prefSetting(id, pref, type = "bool") {
  if (!Preferences.get(pref)) {
    Preferences.add({ id: pref, type });
  }
  Preferences.addSetting({ id, pref });
}

// --- Whisper itself -----------------------------------------------------------------

Preferences.addSetting({ id: "whisperVersion" });
Preferences.addSetting({
  id: "whisperCheckUpdates",
  onUserClick: () => browserWindow()?.openAboutDialog(),
});
Preferences.addSetting({ id: "whisperCollectsNothing" });
Preferences.addSetting({ id: "whisperLimits" });

// --- Search ---------------------------------------------------------------------------

Preferences.addSetting({
  id: "whisperManageEngines",
  onUserClick: () => gotoPref("paneSearch"),
});

// --- New tabs -------------------------------------------------------------------------

prefSetting("whisperQuickOpen", "whisper.newtab.addressBar");

// --- Secure DNS -----------------------------------------------------------------------
// The DNS settings (whisperDnsStrict, whisperDnsSession, whisperDnsSessionOn) are
// added by WhisperSettings.sys.mjs, because Firefox's privacy pane shows them too.

Preferences.addSetting({ id: "whisperDnsProvider" });

// --- Sync -----------------------------------------------------------------------------

Preferences.addSetting({
  id: "whisperSync",
  onUserClick: () => gotoPref("paneSync"),
});

// --- Language -------------------------------------------------------------------------

// The language the interface started in: another one needs a restart.
const STARTED_IN = Services.locale.appLocaleAsBCP47;

Preferences.addSetting({
  id: "whisperLanguage",
  get: () => Services.locale.requestedLocale || Services.locale.appLocaleAsBCP47,
  set: value => {
    Services.locale.requestedLocales = [value];
  },
  disabled: () => Services.locale.availableLocales.length < 2,
  getControlConfig(config) {
    const locales = Services.locale.availableLocales;
    const names = Services.intl.getLocaleDisplayNames(undefined, locales, {
      preferNative: true,
    });
    return {
      ...config,
      options: locales.map((value, i) => ({
        value,
        controlAttrs: { label: names[i] },
      })),
    };
  },
});
Preferences.addSetting({
  id: "whisperLanguageRestart",
  deps: ["whisperLanguage"],
  visible: () => (Services.locale.requestedLocale || STARTED_IN) != STARTED_IN,
});
Preferences.addSetting({
  id: "whisperRestart",
  onUserClick: () => {
    const cancel = Cc["@mozilla.org/supports-PRBool;1"].createInstance(
      Ci.nsISupportsPRBool
    );
    Services.obs.notifyObservers(cancel, "quit-application-requested", "restart");
    if (!cancel.data) {
      Services.startup.quit(
        Ci.nsIAppStartup.eAttemptQuit | Ci.nsIAppStartup.eRestart
      );
    }
  },
});

// --- Pages ----------------------------------------------------------------------------

prefSetting("whisperDrm", "media.eme.enabled");

// --- Your data ------------------------------------------------------------------------

Preferences.addSetting({
  id: "whisperImport",
  onUserClick: () =>
    lazy.MigrationUtils.showMigrationWizard(browserWindow(), {
      entrypoint: lazy.MigrationUtils.MIGRATION_ENTRYPOINTS.PREFERENCES,
    }),
});
Preferences.addSetting({
  id: "whisperUblock",
  visible: () => !!WebExtensionPolicy.getByID(UBO_ID),
  onUserClick: () => {
    const policy = WebExtensionPolicy.getByID(UBO_ID);
    if (policy) {
      browserWindow()?.openTrustedLinkIn(policy.getURL("dashboard.html"), "tab");
    }
  },
});

function version() {
  return Services.prefs.getDefaultBranch("").getStringPref("whisper.version", "");
}

SettingGroupManager.registerGroups({
  whisperAbout: {
    l10nId: "whisper-settings-about-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperVersion",
        l10nId: "whisper-settings-version",
        l10nArgs: {
          version: version(),
          firefoxVersion: Services.appinfo.version,
        },
        control: "moz-box-item",
        options: [
          {
            id: "whisperCheckUpdates",
            control: "moz-button",
            l10nId: "whisper-settings-check-updates",
            slot: "actions",
          },
        ],
      },
      {
        id: "whisperCollectsNothing",
        l10nId: "whisper-settings-collects-nothing",
        control: "moz-box-link",
        controlAttrs: { href: NETWORK_URL },
      },
      {
        id: "whisperLimits",
        l10nId: "whisper-settings-limits",
        control: "moz-box-link",
        controlAttrs: { href: LIMITS_URL },
      },
    ],
  },
  whisperSearch: {
    l10nId: "whisper-settings-search-group",
    headingLevel: 2,
    items: [
      {
        id: "defaultEngineNormal",
        l10nId: "search-default-engine",
        control: "moz-select",
      },
      {
        id: "suggestionsInSearchFieldsCheckbox",
        l10nId: "whisper-settings-suggestions",
        control: "moz-toggle",
      },
      {
        id: "addEngineButton",
        l10nId: "whisper-settings-add-engine",
        control: "moz-box-button",
      },
      {
        id: "whisperManageEngines",
        l10nId: "whisper-settings-manage-engines",
        control: "moz-box-button",
      },
    ],
  },
  whisperNewTabs: {
    l10nId: "whisper-settings-newtabs-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperQuickOpen",
        l10nId: "whisper-settings-quick-open",
        control: "moz-toggle",
      },
    ],
  },
  whisperDefaultBrowser: {
    l10nId: "whisper-settings-default-group",
    headingLevel: 2,
    items: [
      {
        id: "isDefaultPane",
        l10nId: "whisper-settings-default-yes",
        control: "moz-box-item",
      },
      {
        id: "isNotDefaultPane",
        l10nId: "whisper-settings-default-no",
        control: "moz-box-item",
        options: [
          {
            id: "setDefaultButton",
            control: "moz-button",
            l10nId: "whisper-settings-make-default",
            slot: "actions",
            controlAttrs: { type: "primary" },
          },
        ],
      },
    ],
  },
  whisperDns: {
    l10nId: "whisper-settings-dns-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperDnsStrict",
        l10nId: "whisper-settings-dns-strict",
        control: "moz-toggle",
      },
      {
        id: "whisperDnsSessionOn",
        l10nId: "whisper-settings-dns-session-on",
        control: "moz-message-bar",
        controlAttrs: { type: "warning" },
      },
      {
        id: "whisperDnsSession",
        l10nId: "whisper-settings-dns-session",
        control: "moz-box-button",
      },
      {
        id: "whisperDnsProvider",
        l10nId: "whisper-settings-dns-provider",
        control: "moz-box-button",
        loadPane: "dnsOverHttps",
      },
    ],
  },
  whisperSync: {
    l10nId: "whisper-settings-sync-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperSync",
        l10nId: "whisper-settings-sync",
        control: "moz-box-button",
      },
    ],
  },
  whisperLanguage: {
    l10nId: "whisper-settings-language-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperLanguage",
        l10nId: "whisper-settings-language",
        control: "moz-select",
      },
      {
        id: "whisperLanguageRestart",
        l10nId: "whisper-settings-language-restart",
        control: "moz-message-bar",
        controlAttrs: { type: "info" },
        options: [
          {
            id: "whisperRestart",
            control: "moz-button",
            l10nId: "whisper-settings-restart",
            slot: "actions",
          },
        ],
      },
    ],
  },
  whisperPages: {
    l10nId: "whisper-settings-pages-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperDrm",
        l10nId: "whisper-settings-drm",
        control: "moz-toggle",
      },
    ],
  },
  whisperData: {
    l10nId: "whisper-settings-data-group",
    headingLevel: 2,
    items: [
      {
        id: "whisperImport",
        l10nId: "whisper-settings-import",
        control: "moz-box-button",
      },
      {
        id: "whisperUblock",
        l10nId: "whisper-settings-ublock",
        control: "moz-box-button",
      },
    ],
  },
});
