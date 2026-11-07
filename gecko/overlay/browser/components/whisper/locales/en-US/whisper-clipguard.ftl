# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

## Whisper's ClickFix protection. These strings are honest on purpose: Whisper stops a
## page from silently placing a command you did not select and cannot see. Typing a
## command by hand, following instructions on a second device and using another browser
## are still possible, so nothing here says "you are protected".

clipguard-this-site = this site

# The blocked-copy doorhanger. { $origin } is the site's host.
clipguard-blocked-message = { $origin } copied a command you didn’t select. Pages that ask you to paste something into Run, Terminal or File Explorer to “verify” or “fix” something are a common way to install malware. Whisper replaced your clipboard with a warning.

clipguard-blocked-close-tab =
    .label = Close this tab
clipguard-blocked-close-tab-key = C
clipguard-blocked-keep =
    .label = Keep blocked
clipguard-blocked-keep-key = K
clipguard-blocked-show =
    .label = Show what it tried to copy
clipguard-blocked-show-key = S
clipguard-blocked-copy-anyway =
    .label = Copy anyway (hold for 3 seconds)
clipguard-blocked-copy-anyway-key = a
clipguard-blocked-always-allow =
    .label = Always allow commands from this site
clipguard-blocked-always-allow-key = w

# Shown while the person holds the "Copy anyway" button.
clipguard-hold-message = Hold the button for 3 seconds to put the command on your clipboard anyway.
clipguard-hold-button =
    .label = Hold to copy

# "Show what it tried to copy" opens this.
clipguard-details-title = What the page tried to copy
clipguard-details-intro = This is what the page put on your clipboard. Hidden characters are shown, like <U+200B>. It was not run.
clipguard-details-hosts = Addresses in it
clipguard-details-text = The command
clipguard-details-close =
    .label = Close

# Notices for writes that were allowed.
clipguard-notice-ok =
    .label = OK
clipguard-notice-show =
    .label = Show what it copied
clipguard-notice-show-key = S
clipguard-notice-ok-key = O

# A page copy handler replaced the selection (P2).
clipguard-notice-substituted = This page changed what you copied. You selected one thing, but the page put something else on your clipboard.
# A download-and-run command from an unknown source, shown on the page (MED). { $host }
# is the host the command downloads from.
clipguard-notice-installer = This command downloads and runs a script from { $host }. Run it only if you trust this source.
# A command that looks like malware, written by a script and shown on the page (HIGH).
clipguard-notice-command = The command this page copied looks like one used to install malware. Check it before pasting.
# The person copied such a command themselves (HIGH, P1).
clipguard-notice-user-command = The command you copied looks like one used to install malware. Check it before pasting.
# clipguard.mode = warn: this would have been blocked. { $origin } is the site.
clipguard-notice-warned = { $origin } copied a command you didn’t select. Whisper would normally block this; it is set to warn only.

# The LOW address-bar indicator.
clipguard-indicator-message = A command was copied from this page. Check it before pasting.

## Settings section (Whisper preferences pane; see settings/clipguard-settings.mjs).

clipguard-settings-group =
    .label = ClickFix protection
clipguard-settings-mode =
    .label = When a page copies a command you didn’t select and it isn’t on screen
clipguard-settings-mode-block =
    .label = Block it and show a warning (recommended)
clipguard-settings-mode-warn =
    .label = Copy it, but warn me
clipguard-settings-mode-off =
    .label = Do nothing
clipguard-settings-user-copy-notice =
    .label = Warn me when I copy a command that looks like malware
clipguard-settings-strict-script-copy =
    .label = Ask about every command a page copies, even one that is on screen
    .description = Off by default: ordinary “copy” buttons on documentation would ask every time.
clipguard-settings-limits =
    .message = Whisper stops a page from quietly putting a command on your clipboard that you can’t see, and keeps the clipboard text on this device. It can’t stop a command you type by hand, instructions you follow on a phone or another device, or another browser.
