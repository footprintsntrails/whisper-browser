# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

## The about dialog (WhisperAbout.sys.mjs). Whisper keeps its own version and says
## which Firefox version it is built from.

# The heading. Example: Whisper 0.2.0
# Variables:
#   $version (String): Whisper's version, for example 0.2.0
whisper-about-title = { -brand-product-name } { $version }

# The pill next to the heading: which edition this is.
whisper-about-channel-release = Release
whisper-about-channel-canary = Canary

# Under the heading. Example: 64-bit · Built November 6, 2026
# Variables:
#   $bits (Number): 64 or 32
#   $date (String): the day the build was made, already in the user's date format
whisper-about-build = { $bits }-bit · Built { $date }

# The same on other processors. Example: aarch64 · Built November 6, 2026
# Variables:
#   $arch (String): the processor architecture, for example aarch64
#   $date (String): the day the build was made, already in the user's date format
whisper-about-build-arch = { $arch } · Built { $date }

# After "up to date" or a failed check: check again.
whisper-about-check-again =
    .label = Check for updates

# Variables:
#   $firefoxVersion (String): the Firefox version Whisper is built from, for example 159.0.1
whisper-about-engine = Engine: Firefox { $firefoxVersion }

# Under the update state, as visible as the lines above.
whisper-about-not-mozilla = { -brand-product-name } is not affiliated with or endorsed by Mozilla. Firefox is a trademark of the Mozilla Foundation.
