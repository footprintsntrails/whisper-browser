# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

## Settings for Whisper's security features (settings/whisper-security-settings.mjs,
## shown in the Whisper pane). Each group says what the feature does and what it
## cannot do. Never claim the user is "protected" or "safe".

## Recovery phrase guard

whisper-settings-seed-group =
    .label = Recovery phrases
    .description = Wallet recovery phrases (12 to 24 words) belong in your wallet, never on a website.
whisper-settings-seed-enabled =
    .label = Stop a recovery phrase before a website gets it
    .description = When you paste, drop or type a phrase from the standard word lists into a page, Whisper stops it and warns you. Holding a button for 3 seconds allows that one field.
whisper-settings-seed-limits =
    .message = While you type, the page sees the first words until Whisper pauses the field after 6 of them. Whisper only looks at pages in Whisper, not at other programs, and it doesn't keep or send what you typed. Wallet add-ons are not affected.

## Swapped address guard

whisper-settings-address-group =
    .label = Pasted wallet addresses
    .description = Some malware swaps a wallet address you copy for its own, so the payment goes to someone else.
whisper-settings-address-enabled =
    .label = Warn when a pasted address is not the one I copied
    .description = Whisper remembers the last wallet address you copied in Whisper for 10 minutes, in memory only. If the address you paste into a page is a different one, Whisper shows both and lets you choose.
whisper-settings-address-limits =
    .message = Whisper can only compare addresses you copied in Whisper. An address copied in another program or another browser can't be compared, and an address that was already wrong when you copied it looks fine. Check the first and last characters before you send.

## Download check

whisper-settings-downloads-group =
    .label = Risky downloads
    .description = Files that attacks often use are held until you decide, in the downloads panel.
whisper-settings-downloads-enabled =
    .label = Hold risky downloads until I decide
    .description = Disc images, shortcuts, scripts and app packages, names that hide the real file type, files whose content doesn't match their name, and ZIP archives that contain such files. The file can't be opened before you allow it.
whisper-settings-downloads-programs =
    .label = Also hold programs (.exe, .msi) from sites I haven't allowed one from in this session
whisper-settings-downloads-limits =
    .message = Whisper checks the kind of file on this device and sends nothing anywhere. It can't tell whether a file is harmful, only that it is a kind attacks use. Files saved with “Save Link As” are not checked. On Windows every download is marked as coming from the internet.

## External programs

whisper-settings-programs-group =
    .label = Links that open programs
    .description = Some links start a program on your computer instead of opening a page.
whisper-settings-programs-enabled =
    .label = Ask before a website opens a program
    .description = Whisper shows the exact site, the program and where it is on your computer. Dangerous kinds of links (Windows search, troubleshooters, installers and the other “ms-” links) never open, and nothing opens without a click on the page.
whisper-settings-programs-nm =
    .label = Ask before an add-on starts a program
    .description = Some add-ons talk to a program installed on your computer (native messaging). Whisper names the add-on and the program first.
whisper-settings-programs-limits =
    .message = Once a program runs, Whisper can't control what it does. “Always allow” is kept for that exact site or that add-on; clearing the site's data and permissions removes it.

## Clean links

whisper-settings-cleanlinks-group =
    .label = Clean links
    .description = Many links carry tracking codes that tell the next site where you came from.
whisper-settings-cleanlinks-enabled =
    .label = Remove known tracking codes from links to other sites
    .description = Codes such as utm_source, fbclid and gclid are removed when you follow a link to another site or open an address you pasted, in normal and private windows. Whisper uses its own list and doesn't download one.
whisper-settings-cleanlinks-copy =
    .label = Copy Link copies the clean link
    .description = “Copy Clean Link” stays in the menu as well.
whisper-settings-cleanlinks-limits =
    .message = Only codes on Whisper's list are removed; others stay, and sites can still track you in other ways. Links within the same site keep their codes. Removing them can break referral or affiliate credit, personalised landing pages and some newsletter “unsubscribe” or “preferences” links.
