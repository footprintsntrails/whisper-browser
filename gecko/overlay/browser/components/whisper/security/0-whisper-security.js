/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// Whisper's security and trace prefs, built into the browser (browser/omni.ja,
// defaults/preferences/0-whisper-security.js). They load after 0-whisper.js (Firefox
// reads the files of a directory in reverse alphabetical order), so a value here wins.
//
// Every pref has a "why" in gecko/prefs-manifest.json; gecko/scripts/check-prefs.py
// fails CI when a pref has no reason, or when a Firefox pref named here no longer
// exists in the Firefox version being built.
//
// pref(name, value)          a default; Settings or about:config can change it.
// pref(name, value, locked)  fixed; nothing can change it.

// --- Traces on this device --------------------------------------------------------
// The session file keeps tabs and history for restore, but never cookies or what was
// typed into forms (0 would store both for every site, 1 for http only).
pref("browser.sessionstore.privacy_level", 2, locked);
// Downloads do not appear in the system's "recent documents" (Windows Start menu,
// GTK recent files).
pref("browser.download.manager.addToRecentDocs", false, locked);
// Desktop shortcuts made from a page do not store the site's icon on disk.
pref("browser.shell.shortcutFavicons", false, locked);
// Windows Restart Manager: Whisper does not register its command line (with the
// profile path) to be restarted after a Windows update.
pref("toolkit.winRegisterApplicationRestart", false, locked);
// A download is written to the temporary folder until it is complete (and, when the
// download check holds it, until you decide), not half-written into Downloads.
pref("browser.download.start_downloads_in_tmp_dir", true);
// Files opened straight from a download ("open with") are deleted when Whisper quits.
pref("browser.helperApps.deleteTempFileOnExit", true);
// No form history: what is typed into search boxes and forms is not remembered.
pref("browser.formfill.enable", false);
// Media in private windows is cached in memory only, never on disk.
pref("browser.privatebrowsing.forceMediaMemoryCache", true, locked);
// Windows taskbar Jump Lists would list recently and frequently visited pages.
pref("browser.taskbar.lists.enabled", false, locked);
pref("browser.taskbar.lists.frequent.enabled", false, locked);
pref("browser.taskbar.lists.recent.enabled", false, locked);
pref("browser.taskbar.lists.tasks.enabled", false, locked);
// TLS session tickets stay in memory and are not saved with the profile.
pref("network.ssl_tokens_cache_persistence", false, locked);
// Glean (the telemetry library) keeps no copies of pings on disk.
pref("telemetry.fog.enable_store_submitted_pings", false, locked);
// No page screenshots are saved for the new tab page.
pref("browser.pagethumbnails.capturing_disabled", true, locked);
// Copies from private windows stay out of Windows clipboard history and the cloud
// clipboard (Firefox's default, locked here).
pref("clipboard.copyPrivateDataToClipboardCloudOrHistory", false, locked);

// --- Code execution and network hardening ------------------------------------------
// PDF files open without running their JavaScript.
pref("pdfjs.enableScripting", false, locked);
// Add-ons only from the profile and from Whisper itself (5 = profile + application):
// nothing a program drops into a user or system extension folder is loaded.
pref("extensions.enabledScopes", 5, locked);
pref("extensions.autoDisableScopes", 15);
// No 0-RTT early data: a network attacker could replay it (TLS 1.3 and HTTP/3).
pref("security.tls.enable_0rtt_data", false, locked);
pref("network.http.http3.enable_0rtt", false, locked);
// A sub-resource from another site cannot pop up a login prompt (1: only from the same
// site as the page).
pref("network.auth.subresource-http-auth-allow", 1, locked);
// Pages do not make the browser send reports to servers the page chose: no CSP
// violation reports, no Reporting API deliveries. Pages behave the same (the
// securitypolicyviolation event still fires); only the outgoing request is gone.
pref("security.csp.reporting.enabled", false);
pref("dom.reporting.header.enabled", false);
// No OCSP: an OCSP request tells the certificate authority which site you are about to
// open. Revocation of public web certificates is checked on this device with CRLite
// (from Remote Settings), which Firefox 159 already prefers over OCSP for them; OCSP
// answers stapled by the site itself are still used. What is lost: revocation checks
// for certificates from roots you added yourself (company or private CAs).
pref("security.OCSP.enabled", 0);
pref("security.pki.crlite_mode", 2, locked);

