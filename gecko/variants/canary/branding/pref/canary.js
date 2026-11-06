/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// Whisper Canary: appended to the release branding prefs by gecko/scripts/prepare.sh;
// a pref set again here replaces the release value.

// Look for a new canary build every 5 minutes while the browser runs (release: every
// 6 hours), so a change reaches the canary a few minutes after it is published.
pref("app.update.interval", 300);
// Where update messages send people to download Whisper Canary by hand.
pref("app.update.url.manual", "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary");
pref("app.update.url.details", "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary");
pref("app.releaseNotesURL", "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary");
pref("app.releaseNotesURL.aboutDialog", "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary");
pref("app.releaseNotesURL.prompt", "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary");
