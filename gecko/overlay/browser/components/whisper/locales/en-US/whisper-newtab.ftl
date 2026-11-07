# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

## Whisper's new tab page (content/newtab/).

whisper-newtab-title = New Tab
whisper-newtab-logo =
    .aria-label = { -brand-short-name }
# The name next to the logo: the product, also in Whisper Canary (its yellow logo
# says which edition it is).
whisper-newtab-wordmark = { -brand-product-name }
whisper-newtab-search =
    .aria-label = Search or enter address
whisper-newtab-search-placeholder = Search or enter address
whisper-newtab-search-key = Ctrl L
whisper-newtab-sites =
    .aria-label = Your sites
whisper-newtab-add = Add
whisper-newtab-add-url =
    .placeholder = Address
    .aria-label = Address of the site
whisper-newtab-add-name =
    .placeholder = Name (optional)
    .aria-label = Name of the site
whisper-newtab-add-save = Add site
whisper-newtab-add-cancel = Cancel
# Variables:
#   $name (String): the site's name
whisper-newtab-remove =
    .title = Remove { $name }
    .aria-label = Remove { $name }
# The space the window belongs to (spaces come later; until then every window is in this one).
whisper-newtab-space-default = Personal
