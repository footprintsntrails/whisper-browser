// Whisper prototype: preferences for the Whisper profile.
// Firefox reads this file on every start, so the values here win over changes made in
// about:config. Whisper writes this file into the profile at every start; to change a value
// for good, put your own user_pref lines in user-overrides.js in the profile folder. Whisper
// adds them after these, so yours win.
//
// Rule for this file: nothing here may make the browser contact a new server, and each
// group says what it turns off.

// --- Telemetry, studies, crash reports: off -----------------------------------------
user_pref("datareporting.healthreport.uploadEnabled", false);
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("datareporting.usage.uploadEnabled", false);
user_pref("toolkit.telemetry.enabled", false);
user_pref("toolkit.telemetry.unified", false);
user_pref("toolkit.telemetry.archive.enabled", false);
user_pref("toolkit.telemetry.newProfilePing.enabled", false);
user_pref("toolkit.telemetry.shutdownPingSender.enabled", false);
user_pref("toolkit.telemetry.firstShutdownPing.enabled", false);
user_pref("toolkit.telemetry.updatePing.enabled", false);
user_pref("toolkit.telemetry.bhrPing.enabled", false);
user_pref("toolkit.telemetry.server", "data:,");
user_pref("toolkit.coverage.opt-out", true);
user_pref("toolkit.coverage.endpoint.base", "");
user_pref("app.shield.optoutstudies.enabled", false);
user_pref("app.normandy.enabled", false);
user_pref("app.normandy.api_url", "");
user_pref("messaging-system.rsexperimentloader.enabled", false);
user_pref("nimbus.rollouts.enabled", false);
user_pref("breakpad.reportURL", "");
user_pref("browser.tabs.crashReporting.sendReport", false);
user_pref("browser.crashReports.unsubmittedCheck.autoSubmit2", false);
user_pref("dom.private-attribution.submission.enabled", false);

// --- Sponsored content, Pocket, recommendations: off --------------------------------
user_pref("browser.newtabpage.activity-stream.feeds.telemetry", false);
user_pref("browser.newtabpage.activity-stream.telemetry", false);
user_pref("browser.newtabpage.activity-stream.showSponsored", false);
user_pref("browser.newtabpage.activity-stream.showSponsoredTopSites", false);
user_pref("browser.newtabpage.activity-stream.showSponsoredCheckboxes", false);
user_pref("browser.newtabpage.activity-stream.feeds.section.topstories", false);
user_pref("browser.newtabpage.activity-stream.default.sites", "");
user_pref("browser.newtabpage.activity-stream.feeds.topsites", false);
user_pref("browser.topsites.contile.enabled", false);
user_pref("browser.toolbars.bookmarks.visibility", "never");
user_pref("browser.newtabpage.activity-stream.asrouter.userprefs.cfr.addons", false);
user_pref("browser.newtabpage.activity-stream.asrouter.userprefs.cfr.features", false);
user_pref("extensions.pocket.enabled", false);
user_pref("browser.urlbar.suggest.quicksuggest.sponsored", false);
user_pref("browser.urlbar.suggest.quicksuggest.nonsponsored", false);
user_pref("browser.urlbar.quicksuggest.enabled", false);
user_pref("browser.urlbar.trending.featureGate", false);
user_pref("browser.discovery.enabled", false);
user_pref("extensions.getAddons.showPane", false);
user_pref("extensions.htmlaboutaddons.recommendations.enabled", false);
user_pref("browser.ml.chat.enabled", false);

// --- Tracking protection ------------------------------------------------------------
user_pref("browser.contentblocking.category", "strict");
user_pref("privacy.globalprivacycontrol.enabled", true);

// --- DNS over HTTPS through Quad9 ---------------------------------------------------
// Mode 2: DoH first, the system resolver only if DoH fails. Mode 3 (DoH only) is
// stricter but breaks browsing on networks that block DoH; switch to 3 if yours does not.
user_pref("network.trr.mode", 2);
user_pref("network.trr.uri", "https://dns.quad9.net/dns-query");
user_pref("network.trr.custom_uri", "https://dns.quad9.net/dns-query");

// --- Safe Browsing: off (it sends hashed lookups to Google); uBlock Origin lists instead
user_pref("browser.safebrowsing.malware.enabled", false);
user_pref("browser.safebrowsing.phishing.enabled", false);
user_pref("browser.safebrowsing.downloads.enabled", false);
user_pref("browser.safebrowsing.downloads.remote.enabled", false);
user_pref("browser.safebrowsing.blockedURIs.enabled", false);

// --- Background connections nobody asked for: off -----------------------------------
user_pref("network.prefetch-next", false);
user_pref("network.dns.disablePrefetch", true);
user_pref("network.predictor.enabled", false);
user_pref("browser.places.speculativeConnect.enabled", false);
user_pref("browser.urlbar.speculativeConnect.enabled", false);
user_pref("network.captive-portal-service.enabled", false);
user_pref("network.connectivity-service.enabled", false);

// --- First run: no welcome pages, no default-browser nag ----------------------------
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.preonboarding.enabled", false);
user_pref("termsofuse.bypassNotification", true);
user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("startup.homepage_welcome_url", "");
user_pref("startup.homepage_welcome_url.additional", "");
user_pref("datareporting.policy.firstRunURL", "");
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutConfig.showWarning", false);

// --- uBlock Origin: Whisper puts it in this profile; enable it without a prompt
user_pref("extensions.autoDisableScopes", 0);

// --- Look: dark, vertical tabs on the left, Whisper theme from chrome/userChrome.css ----
user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);
user_pref("extensions.activeThemeID", "firefox-compact-dark@mozilla.org");
user_pref("browser.theme.toolbar-theme", 0);
user_pref("browser.theme.content-theme", 0);
user_pref("layout.css.prefers-color-scheme.content-override", 0);
user_pref("sidebar.revamp", true);
user_pref("sidebar.verticalTabs", true);
user_pref("sidebar.visibility", "always-show");
user_pref("browser.compactmode.show", true);
user_pref("browser.uidensity", 1);
user_pref("browser.tabs.inTitlebar", 1);
