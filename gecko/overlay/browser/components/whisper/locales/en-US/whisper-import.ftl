# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

## Importing from Firefox, Zen, LibreWolf, Floorp or Waterfox (WhisperImport.sys.mjs).
## The browser names themselves are in migrationWizard.ftl.

# Shown when the browser to import from still has its profile open.
# Variables:
#   $browser (String): the browser, for example Zen
whisper-import-close-browser-title = Close { $browser } first
# Variables:
#   $browser (String): the browser, for example Zen
whisper-import-close-browser-message = { $browser } is still running. Close it, then import again: { -brand-short-name } copies its bookmarks, history and passwords only while it is closed, so nothing is half-written.
