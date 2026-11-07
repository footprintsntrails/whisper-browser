/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// Whisper's security and trace prefs, built into the browser (browser/omni.ja,
// defaults/preferences/0-whisper-security.js). They load after 0-whisper.js (Firefox
// reads the files of a directory in reverse alphabetical order), so a value here wins.
//
// Every pref says why it is set. (A manifest of these reasons with a CI check that each
// Firefox pref still exists comes with 0.3.0.)
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

// --- Recovery phrase guard (WhisperInputGuardChild.sys.mjs) ------------------------
// A wallet recovery phrase (12-24 words of an official BIP39 list with a valid
// checksum, or an Electrum seed) typed, pasted or dropped into a web page is stopped
// before the page receives it, with a warning; holding a button for 3 seconds allows
// that one field. Wallet extensions are not affected.
pref("whisper.security.seedguard.enabled", true);
// While typing, the field is paused after this many complete words that are all from
// one list (0: only the whole phrase is stopped). At 6 of 12 words the rest cannot be
// guessed; ordinary text starts with 6 list words in about 1 field in 8,700.
pref("whisper.security.seedguard.pause_after_words", 6);

// --- Swapped address guard (WhisperInputGuardChild.sys.mjs) ------------------------
// A pasted cryptocurrency address is compared with the last address copied in
// Whisper (Copy or Ctrl+C on a page) within the minutes below; a different address of
// the same chain is stopped with a warning that shows both. In memory only.
pref("whisper.security.addressguard.enabled", true);
pref("whisper.security.addressguard.minutes", 10);

// --- Download check (WhisperDownloadGuard.sys.mjs, crates/dl_guard) ----------------
// Risky downloads are held until you decide: disc images, shortcuts, scripts, app
// packages, names that hide their real type, content that does not match its name,
// ZIP archives holding such files. The check runs on this device; nothing is sent.
pref("whisper.security.downloads.enabled", true);
// Programs (.exe, .msi) are held too, unless you already allowed one from that site in
// this session. Off: programs are held only for the reasons above.
pref("whisper.security.downloads.hold_programs", true);

// --- External programs (WhisperProtocolGuard.sys.mjs, crates/proto_guard) ----------
// Links from pages that open another program: dangerous schemes (every ms-* handler,
// search, help viewers, file sharing, script runners) never open; for the rest a dialog
// shows the program's name and full path, and "always" is remembered per exact origin.
// Without a click on the page nothing opens, the same as for a blocked scheme.
pref("whisper.security.protocols.enabled", true);
// ms-* schemes to ask about instead of blocking, comma separated (e.g. "ms-teams").
pref("whisper.security.protocols.allowed_ms_schemes", "");
// Ask before an add-on starts a program through native messaging.
pref("whisper.security.nativemessaging.ask", true);
// Windows schemes Firefox itself does not block, stopped before any JavaScript runs
// as well (the guard above blocks them too; this is the second lock).
pref("network.protocol-handler.external.ms-officecmd", false, locked);
pref("network.protocol-handler.external.ms-appinstaller", false, locked);
pref("network.protocol-handler.external.ms-settings", false, locked);
pref("network.protocol-handler.external.searchconnector-ms", false, locked);
pref("network.protocol-handler.external.ghelp", false, locked);
pref("network.protocol-handler.external.help", false, locked);
pref("network.protocol-handler.external.applescript", false, locked);
pref("network.protocol-handler.external.osascript", false, locked);

// --- Clean links (patches/0066, 0067) ----------------------------------------------
// Tracking parameters are removed from a link when you follow it to another site
// (Firefox's query stripping, normal and private windows) and when you copy it.
pref("privacy.query_stripping.enabled", true);
pref("privacy.query_stripping.enabled.pbmode", true);
pref("privacy.query_stripping.strip_on_share.enabled", true);
// The list, kept here instead of Mozilla's remote one (patches/0066). Only names that
// mean "who sent you here" on every site; each can break a site only by what it
// carried (attribution, a personalised landing page, some mailing-list "unsubscribe"
// or "preferences" links). Matched without regard to case.
// Known to break: the "manage preferences" and "unsubscribe" pages of mailing-list
// services no longer recognise the reader and ask for the address (Mailchimp mc_eid,
// HubSpot _hsenc/_hsmi, Marketo mkt_tok, MailerLite ml_subscriber*, Omeda oly_*_id,
// Vero vero_id); ad and affiliate credit for the site you visit is lost (gclid,
// msclkid, fbclid, ...). Other pages work without them.
pref("privacy.query_stripping.strip_list", "utm_source utm_medium utm_campaign utm_term utm_content utm_id utm_name utm_cid utm_reader utm_referrer utm_social utm_social-type utm_brand utm_place utm_pubreferrer utm_swu utm_viz_id utm_campaign_id utm_source_platform utm_creative_format utm_marketing_tactic fbclid gclid gclsrc gbraid wbraid dclid msclkid yclid ysclid twclid ttclid igshid igsh li_fat_id epik sccid rb_clickid wickedid mc_cid mc_eid _hsenc _hsmi __hssc __hstc __hsfp hsctatracking mkt_tok oly_anon_id oly_enc_id __s vero_id vero_conv _openstat _kx ml_subscriber ml_subscriber_hash");
// No list from Remote Settings (patches/0066): set to true to use Mozilla's as well.
pref("whisper.cleanlinks.remote_list", false);
// Copy Link in the page menu copies the clean link (patches/0067).
pref("whisper.cleanlinks.copy_link_clean", true);
