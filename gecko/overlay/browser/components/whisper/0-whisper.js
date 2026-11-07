/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// Whisper's default preferences, built into the browser (browser/omni.ja,
// defaults/preferences/0-whisper.js).
//
// Why the "0-" in the name: Firefox loads the default pref files of a directory in
// reverse alphabetical order, so this file loads after firefox.js and
// firefox-branding.js and its values win.
//
// pref(name, value)          a default; the user can change it in Settings or about:config.
// pref(name, value, locked)  fixed; about:config shows it as locked and nothing can change
//                            it (no add-on, no user.js, no Settings page).
//
// Rule for this file, as for the profile kit's user.js it replaces: nothing here may
// make the browser contact a new server, and each group says what it turns off.
// Telemetry, studies and crash reporting are also removed when Whisper is built
// (gecko/mozconfigs/common, patches/); the locked prefs below are the second lock.

// --- Telemetry, usage data, studies, crash reports: off and locked ------------------
pref("datareporting.healthreport.uploadEnabled", false, locked);
pref("datareporting.policy.dataSubmissionEnabled", false, locked);
pref("datareporting.usage.uploadEnabled", false, locked);
pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true, locked);
pref("datareporting.policy.firstRunURL", "", locked);
pref("toolkit.telemetry.enabled", false, locked);
pref("toolkit.telemetry.unified", false, locked);
pref("toolkit.telemetry.archive.enabled", false, locked);
pref("toolkit.telemetry.newProfilePing.enabled", false, locked);
pref("toolkit.telemetry.shutdownPingSender.enabled", false, locked);
pref("toolkit.telemetry.firstShutdownPing.enabled", false, locked);
pref("toolkit.telemetry.updatePing.enabled", false, locked);
pref("toolkit.telemetry.bhrPing.enabled", false, locked);
pref("toolkit.telemetry.reportingpolicy.firstRun", false, locked);
pref("toolkit.telemetry.server", "data:,", locked);
pref("app.shield.optoutstudies.enabled", false, locked);
pref("app.normandy.enabled", false, locked);
pref("app.normandy.api_url", "", locked);
// Nimbus experiments: studies (above, and they need telemetry, which is off) and
// rollouts are its two switches in Firefox 159.
pref("nimbus.rollouts.enabled", false, locked);
pref("breakpad.reportURL", "", locked);
pref("browser.tabs.crashReporting.sendReport", false, locked);
pref("browser.crashReports.unsubmittedCheck.enabled", false, locked);
pref("browser.crashReports.unsubmittedCheck.autoSubmit2", false, locked);
pref("browser.newtabpage.activity-stream.feeds.telemetry", false, locked);
pref("browser.newtabpage.activity-stream.telemetry", false, locked);

// --- Sponsored content, recommendations, Mozilla promotions: off and locked ---------
pref("browser.newtabpage.activity-stream.showSponsored", false, locked);
pref("browser.newtabpage.activity-stream.showSponsoredTopSites", false, locked);
pref("browser.newtabpage.activity-stream.showSponsoredCheckboxes", false, locked);
pref("browser.newtabpage.activity-stream.unifiedAds.tiles.enabled", false, locked);
pref("browser.newtabpage.activity-stream.unifiedAds.spocs.enabled", false, locked);
pref("browser.newtabpage.activity-stream.unifiedAds.adsFeed.enabled", false, locked);
pref("browser.topsites.contile.enabled", false, locked);
pref("browser.urlbar.suggest.quicksuggest.sponsored", false, locked);
pref("browser.newtabpage.activity-stream.asrouter.userprefs.cfr.addons", false, locked);
pref("browser.newtabpage.activity-stream.asrouter.userprefs.cfr.features", false, locked);
pref("browser.discovery.enabled", false, locked);
pref("browser.preferences.moreFromMozilla", false, locked);
pref("browser.vpn_promo.enabled", false, locked);
pref("browser.promo.pin.enabled", false, locked);
pref("browser.promo.relay.enabled", false, locked);
pref("identity.fxaccounts.toolbar.pxiToolbarEnabled", false);
pref("extensions.getAddons.showPane", false);
pref("extensions.htmlaboutaddons.recommendations.enabled", false);
pref("browser.uitour.enabled", false);
// Mozilla's VPN offer inside the browser
pref("browser.ipProtection.enabled", false);

// --- New tab: Whisper's own page, and Firefox's new tab quiet behind it -------------
// Whisper's new tab and home page (WhisperNewTab.sys.mjs) is a local page that makes
// no request. Firefox's new tab code still starts in the background; these keep it
// from fetching anything: no stories, weather, sponsored or recommended shortcuts,
// messages, widgets, remote layouts or site icons.
pref("browser.startup.homepage", "chrome://whisper/content/newtab/newtab.html");
pref("browser.newtabpage.activity-stream.feeds.system.topsites", false);
pref("browser.newtabpage.activity-stream.feeds.favicon", false);
pref("browser.newtabpage.activity-stream.asrouter.useRemoteL10n", false);
pref("browser.newtabpage.activity-stream.asrouter.devtoolsEnabled", false);
pref("browser.newtabpage.activity-stream.remote-renderer.enabled", false);
pref("browser.newtabpage.activity-stream.widgets.system.pictureOfTheDay.enabled", false);
pref("browser.newtabpage.activity-stream.widgets.system.crossword.enabled", false);
pref("browser.newtabpage.trainhopAddon.xpiBaseURL", "");
pref("browser.topsites.useRemoteSetting", false);
// Firefox's in-browser messages (feature tips, promotions) from Remote Settings
pref("browser.newtabpage.activity-stream.asrouter.providers.cfr", "{\"id\":\"cfr\",\"enabled\":false,\"type\":\"remote-settings\",\"collection\":\"cfr\",\"updateCycleInMs\":3600000}");
pref("browser.newtabpage.activity-stream.asrouter.providers.message-groups", "{\"id\":\"message-groups\",\"enabled\":false,\"type\":\"remote-settings\",\"collection\":\"message-groups\",\"updateCycleInMs\":3600000}");
pref("browser.newtabpage.activity-stream.feeds.section.topstories", false);
pref("browser.newtabpage.activity-stream.feeds.topsites", false);
pref("browser.newtabpage.activity-stream.default.sites", "");
pref("browser.newtabpage.activity-stream.showWeather", false);
pref("browser.newtabpage.activity-stream.system.showWeather", false);
pref("browser.newtabpage.activity-stream.newtabWallpapers.enabled", false);

// --- Search suggestions: off ---------------------------------------------------------
// Suggestions send what you type to the search engine while you type, before you
// press Enter. Settings turns them on.
pref("browser.search.suggest.enabled", false);

// --- Address bar: no Firefox Suggest, no trending searches --------------------------
pref("browser.urlbar.suggest.quicksuggest.nonsponsored", false);
pref("browser.urlbar.quicksuggest.enabled", false);
pref("browser.urlbar.trending.featureGate", false);
// Whisper's results panel (cast.css, WhisperUrlbar.sys.mjs): at most 8 rows, grouped,
// with the search engines as a row of chips at the bottom (Tab moves between them).
// Firefox 159's "scotch bonnet" bundle replaces those chips with a search button inside
// the field; the parts of it Whisper keeps are turned on one by one.
pref("browser.urlbar.maxRichResults", 8);
pref("browser.urlbar.scotchBonnet.enableOverride", false);
pref("browser.urlbar.trimHttps", true);
pref("browser.urlbar.untrimOnUserInteraction.featureGate", true);
pref("browser.urlbar.searchRestrictKeywords.featureGate", true);

// --- Tracking protection ------------------------------------------------------------
pref("browser.contentblocking.category", "strict");
pref("privacy.globalprivacycontrol.enabled", true);

// --- DNS over HTTPS through Quad9 ---------------------------------------------------
// Mode 3: DoH only, the network never sees or answers the lookups. Where a network
// blocks DoH, Firefox's error page says so and its "DNS settings" button leads to
// "Use this network's DNS until Whisper restarts" (WhisperSettings.sys.mjs); the
// Whisper pane of Settings switches between 3 and 2 (DoH first, the network's DNS if
// DoH fails).
pref("network.trr.mode", 3);
pref("network.trr.uri", "https://dns.quad9.net/dns-query");
pref("network.trr.custom_uri", "https://dns.quad9.net/dns-query");
// Firefox's automatic DoH rollout and its test lookups stay out of it.
pref("doh-rollout.disable-heuristics", true);
pref("doh-rollout.trr-selection.enabled", false);
pref("doh-rollout.provider-steering.enabled", false);

// --- Safe Browsing: off (it sends hashed lookups to Google); uBlock Origin lists instead
pref("browser.safebrowsing.malware.enabled", false);
pref("browser.safebrowsing.phishing.enabled", false);
pref("browser.safebrowsing.downloads.enabled", false);
pref("browser.safebrowsing.downloads.remote.enabled", false);
pref("browser.safebrowsing.blockedURIs.enabled", false);

// --- Background connections nobody asked for: off -----------------------------------
// Link prefetching, DNS prefetching and connections opened ahead to a link the pointer
// is on keep Firefox's values: they only reach hosts the page itself names, and a page
// can measure whether they happen, so turning them off would set Whisper apart.
// Connections the address bar opens ahead while you type are off: those follow what
// you type, and no page can see them.
pref("browser.places.speculativeConnect.enabled", false);
pref("browser.urlbar.speculativeConnect.enabled", false);
pref("network.captive-portal-service.enabled", false);
pref("network.connectivity-service.enabled", false);
// Region lookup by IP address
pref("browser.region.update.enabled", false);
pref("browser.region.network.url", "");
// Location: no network location provider. Firefox asks Google (googleapis.com) where
// you are from the Wi-Fi networks around you when a page you allowed asks for your
// location. Pages may still ask; they get the operating system's location where it
// has one, or nothing.
pref("geo.provider.network.url", "", locked);
// Updates of Mozilla's system add-ons (Whisper updates as a whole)
pref("extensions.systemAddon.update.enabled", false, locked);
pref("extensions.systemAddon.update.url", "", locked);
// Media plugins (Widevine for DRM video, OpenH264 for H.264 video calls) download only
// with consent: DRM is off until it is turned on in Settings or the user agrees on a
// site that needs it (Firefox's "enable DRM" bar), and OpenH264 is off until it is
// turned on in Settings or in Add-ons > Plugins. Then they come from the list built
// into Whisper (with SHA-512 of each file), not from Mozilla's update server
// (aus5.mozilla.org): the address below is never reachable.
pref("media.eme.enabled", false);
pref("media.gmp-gmpopenh264.enabled", false);
pref("media.gmp-manager.url", "data:,", locked);
// Web push: no permanent connection to Mozilla's push server
// (push.services.mozilla.com). Sites cannot send notifications while they are closed.
pref("dom.push.connection.enabled", false);
// Add-on details are fetched from addons.mozilla.org only when you open them
pref("extensions.getAddons.cache.enabled", false);
// The add-ons Whisper ships update with Whisper only: no update check at
// addons.mozilla.org asks for them (patches/0035). Add-ons you install update as in
// Firefox.
pref("whisper.extensions.pinned", "uBlock0@raymondhill.net,addon@darkreader.org", locked);
// Dark Reader recolours pages in normal tabs only, not in container tabs, which later
// carry Whisper's masks (patches/0038; WhisperDarkReader.sys.mjs keeps it offline).
pref("whisper.extensions.defaultContainerOnly", "addon@darkreader.org");

// --- First start: no welcome tour, no terms dialog, no default-browser question -----
pref("browser.aboutwelcome.enabled", false);
pref("browser.preonboarding.enabled", false);
pref("termsofuse.bypassNotification", true);
pref("browser.startup.homepage_override.mstone", "ignore");
pref("browser.shell.checkDefaultBrowser", false);
pref("browser.aboutConfig.showWarning", false);
// The alternative app icons Firefox offers in Settings are Firefox logos (whisper.exe
// carries Whisper's icon in their place, patches/0030).
pref("browser.shell.customIcon.enabled", false, locked);
// No Mozilla account button in the toolbar (Sync stays available in Settings)
pref("identity.fxaccounts.toolbar.defaultVisible", false);

// --- Look: dark, tabs on the left in a sidebar (see whisper.sidebar.mode) -----------
pref("extensions.activeThemeID", "firefox-compact-dark@mozilla.org");
pref("browser.theme.toolbar-theme", 0);
pref("browser.theme.content-theme", 0);
pref("layout.css.prefers-color-scheme.content-override", 0);
pref("sidebar.revamp", true);
pref("sidebar.verticalTabs", true);
pref("sidebar.visibility", "always-show");
pref("sidebar.main.tools", "history,bookmarks");
pref("browser.toolbars.bookmarks.visibility", "never");
pref("browser.compactmode.show", true);
pref("browser.uidensity", 1);
pref("browser.tabs.inTitlebar", 1);

// --- Whisper ------------------------------------------------------------------------
// How the tab sidebar looks: "compact" (icons only, a narrow strip), "autohide" (hidden,
// slides in from the left edge) or "expanded" (names and icons, larger tabs).
// Switch with Ctrl+Alt+S or from the sidebar's right-click menu.
pref("whisper.sidebar.mode", "expanded");

// "+" and Ctrl+T open the address bar where it is; Enter opens the result in a new tab,
// Esc leaves no tab behind (WhisperQuickOpen.sys.mjs). false: they open a new tab page.
pref("whisper.newtab.addressBar", true);

// --- No AI features and no translations, locked (the owner's decision) --------------
// Firefox's own switch first: every AI feature blocked, as "Block AI" in Firefox's AI
// controls would set it; that pane is gone from Settings (WhisperSettings.sys.mjs).
pref("browser.preferences.aiControls", false, locked);
pref("browser.ai.control.default", "blocked", locked);
pref("browser.ai.control.translations", "blocked", locked);
pref("browser.ai.control.pdfjsAltText", "blocked", locked);
pref("browser.ai.control.smartTabGroups", "blocked", locked);
pref("browser.ai.control.linkPreviewKeyPoints", "blocked", locked);
pref("browser.ai.control.speechRecognition", "blocked", locked);
pref("browser.ai.control.sidebarChatbot", "blocked", locked);
pref("browser.ai.control.smartWindow", "blocked", locked);
// ... and each feature's own switch: the machine-learning engine and the model hub
pref("browser.ml.enable", false, locked);
pref("browser.ml.modelHubRootUrl", "", locked);
pref("extensions.ml.enabled", false, locked);
// AI chatbot: sidebar, "Ask an AI chatbot" menus, shortcuts on selected text
pref("browser.ml.chat.enabled", false, locked);
pref("browser.ml.chat.menu", false, locked);
pref("browser.ml.chat.page", false, locked);
pref("browser.ml.chat.shortcuts", false, locked);
pref("browser.ml.chat.sidebar", false, locked);
pref("browser.ml.chat.provider", "", locked);
// Link previews with AI key points, page assist
pref("browser.ml.linkPreview.enabled", false, locked);
pref("browser.ml.pageAssist.enabled", false, locked);
// Smart tab groups (suggested tabs and names)
pref("browser.tabs.groups.smart.enabled", false, locked);
pref("browser.tabs.groups.smart.userEnabled", false, locked);
// Smart window (Firefox's AI window) and its agent, form fill and tab grouping
pref("browser.smartwindow.enabled", false, locked);
pref("browser.smartwindow.isDefaultWindow", false, locked);
pref("browser.smartwindow.agent.enabled", false, locked);
pref("browser.smartwindow.smartformfill.enabled", false, locked);
pref("browser.smartwindow.autoTabGrouping.enabled", false, locked);
pref("browser.smartwindow.autoTabGrouping.preloadModels", false, locked);
pref("browser.smartwindow.endpoint", "", locked);
// PDF viewer: no AI alt text for images, no model download for it
pref("pdfjs.enableAltText", false, locked);
pref("pdfjs.enableGuessAltText", false, locked);
pref("pdfjs.enableAltTextModelDownload", false, locked);
// Translations: no translate button, no "Translate selection", no about:translations,
// no language models downloaded (one place to turn them back on if it is ever wanted)
pref("browser.translations.enable", false, locked);
pref("browser.translations.select.enable", false, locked);
pref("browser.translations.automaticallyPopup", false, locked);
pref("browser.translations.quickAction.enabled", false, locked);

// --- Settings (WhisperSettings.sys.mjs): Whisper's pane, Mozilla's offers out --------
// Firefox Relay (Mozilla's email masks) is not offered when you save a password.
pref("signon.firefoxRelay.feature", "disabled");
// Websites are asked for English whatever the interface language, so the interface
// language does not set a Whisper user apart.
pref("intl.accept_languages", "en-US, en");
