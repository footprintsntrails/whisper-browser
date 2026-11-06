# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

# Whisper Canary branding (gecko/variants/canary). MOZ_MACBUNDLE_ID stays the release
# one: it goes into the build configuration Rust code is compiled with, which release
# and canary builds share (patches/0028-rust-config-without-update-channel.patch).
MOZ_APP_DISPLAYNAME="Whisper Canary"
MOZ_MACBUNDLE_ID=whisper
