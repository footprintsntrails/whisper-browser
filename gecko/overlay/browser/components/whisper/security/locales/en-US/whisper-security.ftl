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
