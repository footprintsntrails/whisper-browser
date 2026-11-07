# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

## Whisper's security features. House rules for these strings: plain words, say what
## happened and what the user can do; never claim the user is "protected" or "safe".

## The hold-to-allow button (guard dialogs)

# $seconds (Number) - seconds left while the button is held
whisper-hold-status =
    { $seconds ->
        [0] Done
        [one] Keep holding: { $seconds } second
       *[other] Keep holding: { $seconds } seconds
    }

## Recovery phrase warning

whisper-seed-title = This looks like a wallet recovery phrase
whisper-seed-lead = Never enter your recovery phrase on a website. Real wallets never ask for it here.
whisper-seed-pasted = Whisper stopped the paste before this page could read it. The field is unchanged.
whisper-seed-dropped = Whisper stopped the drop before this page could read it. The field is unchanged.
# $count (Number) - number of words typed
whisper-seed-typed-start = Whisper paused this field after { $count } words that are all from a recovery phrase word list, and emptied it for now. The page could see the words typed so far, but not the rest.
# $count (Number) - number of words typed
whisper-seed-typed-complete = The page could see each word as you typed it, before Whisper emptied the field. If these { $count } words are a real recovery phrase, move the funds to a new wallet that you set up yourself.
whisper-seed-advice = If a page, a message or a support agent asked you to enter it to “verify”, “sync”, “unlock” or “claim” something, close this page. Anyone with the phrase can take everything in the wallet.
whisper-seed-go-back = Leave the field empty
whisper-seed-hold = Hold to use this field anyway
whisper-seed-hold-hint-paste = Hold the button for 3 seconds (or hold Space) to allow this one field until you leave the page, then paste again.
whisper-seed-hold-hint-typed = Hold the button for 3 seconds (or hold Space) to put the words back and allow this one field until you leave the page.

## Swapped address warning

whisper-address-title = The address you pasted is not the one you copied
whisper-address-lead = Malware on this computer may have changed it.
whisper-address-copied = You copied:
whisper-address-pasted = You pasted:
# $count (Number) - characters that differ between the two addresses
whisper-address-difference =
    { $count ->
        [one] { $count } character differs (marked).
       *[other] { $count } characters differ (marked).
    }
whisper-address-scope = Whisper compares only with the last address copied in Whisper in the last few minutes. If you copied this one in another program after that, it can be correct.
whisper-address-use-copied = Paste the address I copied
whisper-address-use-pasted = Paste it anyway
whisper-address-cancel = Cancel

## Download check (downloads panel, about:downloads)

whisper-download-held-title = Whisper held this file
whisper-download-held-tip = The check ran on this device; nothing was sent anywhere. Allow it only if you expected this file from this site.
whisper-download-unblock-message = Whisper held this file because files of this kind can start programs or hide what they contain. Open it only if you expected it and trust where it came from.
# Variables in the reasons below:
#   $ext (String) - the file's extension, with its dot, such as ".iso"
#   $shown (String) - the extension the name pretends to have, such as ".pdf"
#   $real (String) - the extension the file really has, such as ".exe"
#   $claimed (String) - what the name or the server says the file is
#   $found (String) - what the file really is, such as "Windows program"
#   $entry (String) - the name of a file inside an archive or disc image
whisper-download-reason-disk-image = Disc images and virtual disks ({ $ext }) can hide programs and shortcuts from Windows' safety checks.
whisper-download-reason-script = Files of this type ({ $ext }) run commands as soon as they are opened.
whisper-download-reason-shortcut = Shortcut files ({ $ext }) can start programs or open shared folders on other computers.
whisper-download-reason-app-package = App packages ({ $ext }) install software.
whisper-download-reason-program = This is a program ({ $ext }). Whisper holds programs until you allow one from this site; run programs only from their maker's own site.
whisper-download-reason-program-made-by-page = This program was put together by the web page itself instead of coming from a server, a common way to slip past checks.
whisper-download-reason-direction-marks = The name contains hidden characters that change the order of its letters, so it can pass for another type of file.
whisper-download-reason-reversed-extension = The name is made to look like a { $shown } file, but it is a { $real } file.
whisper-download-reason-double-extension = The name looks like a { $shown } file, but it is a { $real } file.
whisper-download-reason-content-mismatch = It is presented as { $claimed }, but it is a { $found }.
whisper-download-reason-archive-holds = This archive contains { $entry }, a file that can start programs.
whisper-download-reason-encrypted-archive-holds = This password-protected archive contains { $entry }. A password keeps checks from looking inside, which is why such archives are used to deliver programs.
whisper-download-reason-disc-image-holds = This disc image contains { $entry }.
whisper-download-reason-unchecked = Whisper could not check this file, so it is held until you decide.

## External programs: a link that opens a program, an add-on that starts one

whisper-program-title = This site wants to start a program
# Variables:
#   $origin (String) - the exact site, such as "https://example.com"
#   $scheme (String) - the kind of link, such as "zoommtg"
#   $program (String) - the program's name
whisper-program-lead = { $origin } wants to open a { $scheme }: link in { $program }.
whisper-program-lead-unnamed = { $origin } wants to open a { $scheme }: link in another program.
whisper-program-path-label = Program on this computer:
whisper-program-path-unknown = Whisper could not find where this program is on disk.
whisper-program-warning = Whisper can't control what the program does next. Cancel if you did not expect this.
# $origin (String), $scheme (String): as above
whisper-program-remember = Always allow { $origin } (this exact address) to open { $scheme }: links
whisper-program-cancel = Cancel
whisper-program-open = Open
whisper-program-nm-title = An add-on wants to start a program on this computer
# $addon (String) - the add-on's name, $host (String) - the name it uses for the program
whisper-program-nm-lead = { $addon } wants to start { $host }.
# $description (String) - how the program describes itself
whisper-program-nm-description = It describes itself as: { $description }
whisper-program-nm-warning = The program runs outside the browser, with your permissions. Whisper can't control what it does.
whisper-program-nm-remember = Always allow { $addon } to start this program
whisper-program-nm-allow = Allow
