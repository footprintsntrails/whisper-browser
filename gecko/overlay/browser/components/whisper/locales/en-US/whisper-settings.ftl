# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

## The Whisper pane of Settings (WhisperSettings.sys.mjs,
## content/settings/whisper-settings.mjs). Every text says what a setting does and,
## where it matters, what it cannot do.

whisper-settings-nav = { -brand-product-name }
    .title = { -brand-product-name }
whisper-settings-pane-title =
    .heading = { -brand-product-name }

# In Home and startup: the home page and new tab choice for Whisper's own page.
whisper-settings-home-default =
    .label = { -brand-product-name } page (default)

## Whisper itself

whisper-settings-about-group =
    .label = { -brand-product-name }
# Variables:
#   $version (String): Whisper's version, for example 0.2.0
#   $firefoxVersion (String): the Firefox version Whisper is built from
whisper-settings-version =
    .label = { -brand-product-name } { $version }
    .description = Engine: Firefox { $firefoxVersion }. Whisper is not affiliated with or endorsed by Mozilla.
whisper-settings-check-updates =
    .label = Check for updates
whisper-settings-collects-nothing =
    .label = Whisper collects nothing
    .description = No telemetry, no studies, no crash reports, no account. The servers Whisper talks to, and why, are listed in its README.
whisper-settings-limits =
    .label = What Whisper can’t protect you from
    .description = A browser cannot hide you from the sites you sign in to, from your network’s operator for sites that are not encrypted, or from software on your computer.

## Search

whisper-settings-search-group =
    .label = Search
    .description = DuckDuckGo is the default. Whisper adds no partner or tracking codes to any search.
whisper-settings-suggestions =
    .label = Show suggestions while you type
    .description = Off: nothing you type leaves Whisper until you press Enter. On: every letter is sent to your search engine as you type it.
whisper-settings-add-engine =
    .label = Add your own search engine
    .description = A self-hosted SearXNG, Kagi or any search page: its address with %s where the search words go.
whisper-settings-manage-engines =
    .label = All search engines and shortcuts

## Default browser

whisper-settings-default-group =
    .label = Default browser
whisper-settings-default-yes =
    .label = { -brand-short-name } is your default browser
    .description = Links from other programs open here.
whisper-settings-default-no =
    .label = { -brand-short-name } is not your default browser
    .description = Links from other programs open in another browser.
whisper-settings-make-default =
    .label = Make default

## Secure DNS

whisper-settings-dns-group =
    .label = Secure DNS
    .description = Which server finds the address of each site you visit. Whisper asks Quad9 over an encrypted connection, so your network cannot read or change the answers.
whisper-settings-dns-strict =
    .label = Only use secure DNS
    .description = If a network blocks it, sites will not open and Whisper says so; you can then use the network’s own DNS until Whisper restarts. Off: Whisper quietly falls back to the network’s DNS, which the network can read.
whisper-settings-dns-session =
    .label = Use this network’s DNS until Whisper restarts
    .description = For a network that blocks secure DNS, such as some hotel and office networks. The network can see which sites you look up.
whisper-settings-dns-session-on =
    .message = The network’s DNS is in use until Whisper restarts.
whisper-settings-dns-provider =
    .label = Provider and exceptions

## Sync

whisper-settings-sync-group =
    .label = Sync
whisper-settings-sync =
    .label = Sync is off
    .description = With a Mozilla account, bookmarks, history, passwords and settings are encrypted on this device and stored on Mozilla’s servers. Whisper works fully without it.

## Language

whisper-settings-language-group =
    .label = Language
whisper-settings-language =
    .label = Interface language
    .description = Menus and settings. Websites are still asked for English, so the language does not set you apart.
whisper-settings-language-restart =
    .message = Restart { -brand-short-name } to use the new language.
whisper-settings-restart =
    .label = Restart now

## Pages

whisper-settings-pages-group =
    .label = Pages
whisper-settings-drm =
    .label = Play DRM-protected video and music
    .description = Netflix, Spotify and similar services. Google’s Widevine module is downloaded from Google once and runs in a sandbox. Off: those services won’t play.

## Your data

whisper-settings-data-group =
    .label = Your data
whisper-settings-import =
    .label = Import from another browser
    .description = Bookmarks, history and passwords from Firefox, Zen, LibreWolf, Floorp, Waterfox, Chrome and others.
whisper-settings-ublock =
    .label = uBlock Origin
    .description = The content blocker that comes with Whisper: its filter lists and your own rules.
