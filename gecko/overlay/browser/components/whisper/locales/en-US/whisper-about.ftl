# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

## The about dialog (WhisperAbout.sys.mjs). Whisper keeps its own version and says
## which Firefox version it is built from.

# Example: Whisper 0.1.0 (64-bit)
# Variables:
#   $version (String): Whisper's version, for example 0.1.0
#   $bits (Number): 64 or 32
whisper-about-version = { -brand-short-name } { $version } ({ $bits }-bit)

# Example: Whisper 0.1.0 (aarch64)
# Variables:
#   $version (String): Whisper's version, for example 0.1.0
#   $arch (String): the processor architecture, for example aarch64
whisper-about-version-arch = { -brand-short-name } { $version } ({ $arch })

# Variables:
#   $firefoxVersion (String): the Firefox version Whisper is built from, for example 159.0.1
whisper-about-based-on = Based on Firefox { $firefoxVersion }

# Whisper Canary (the test edition), which has a new build for every change.
# Example: Based on Firefox 159.0.1, build 20261106134502
# Variables:
#   $firefoxVersion (String): the Firefox version Whisper is built from, for example 159.0.1
#   $buildID (String): the build ID, the time the build was made (YYYYMMDDhhmmss)
whisper-about-based-on-build = Based on Firefox { $firefoxVersion }, build { $buildID }

# Shown under "Based on Firefox", as visibly as that line.
whisper-about-not-mozilla = { -brand-short-name } is not affiliated with or endorsed by Mozilla. Firefox is a trademark of the Mozilla Foundation.
