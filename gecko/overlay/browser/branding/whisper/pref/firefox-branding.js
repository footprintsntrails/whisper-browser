/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// Branding prefs for Whisper. Everything else Whisper changes is in
// browser/components/whisper/0-whisper.js.

pref("startup.homepage_override_url", "");
pref("startup.homepage_welcome_url", "");
pref("startup.homepage_welcome_url.additional", "");

// Updates: Firefox's own updater, pointed at Whisper's update host (the address is
// in application.ini, see patches/0021-update-url.patch).
// Check every 6 hours while the browser runs, as Firefox does.
pref("app.update.interval", 21600);
// Show "Restart to update Whisper" in the menu as soon as an update is ready (Firefox
// waits 4 days), and ask in a pop-up only after 8 days, as Firefox does.
pref("app.update.badgeWaitTime", 0);
pref("app.update.promptWaitTime", 691200);
// A build older than this many days checks for updates right at start, as Firefox's.
pref("app.update.checkInstallTime.days", 63);
// Where update messages send people to download Whisper by hand.
pref("app.update.url.manual", "https://github.com/footprintsntrails/whisper-browser/releases/latest");
pref("app.update.url.details", "https://github.com/footprintsntrails/whisper-browser/releases");
pref("app.releaseNotesURL", "https://github.com/footprintsntrails/whisper-browser/releases");
pref("app.releaseNotesURL.aboutDialog", "https://github.com/footprintsntrails/whisper-browser/releases");
pref("app.releaseNotesURL.prompt", "https://github.com/footprintsntrails/whisper-browser/releases");

// Keep the developer tools' guard against pasting code into the console, as in
// Firefox's official branding (a common way to trick people into running code).
pref("devtools.selfxss.count", 0);
