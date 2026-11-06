# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

# NSIS branding defines for Whisper Canary (gecko/variants/canary; the release ones are
# in gecko/overlay/browser/branding/whisper/branding.nsi).

# BrandFullNameInternal is used for some registry and file system values
# instead of BrandFullName and typically should not be modified.
!define BrandFullNameInternal "Whisper Canary"
!define BrandFullName         "Whisper Canary"
!define CompanyName           "Whisper"
!define URLInfoAbout          "https://github.com/footprintsntrails/whisper-browser"
!define HelpLink              "https://github.com/footprintsntrails/whisper-browser/issues"

!define URLStubDownloadX86 "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary"
!define URLStubDownloadAMD64 "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary"
!define URLStubDownloadAArch64 "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary"
!define URLManualDownload "https://github.com/footprintsntrails/whisper-browser/releases/tag/canary"
!define URLSystemRequirements "https://github.com/footprintsntrails/whisper-browser#download"
!define Channel "canary"

# The uninstaller's question (patches/0025-windows-install-per-user.patch); No keeps the
# profile. A silent uninstall keeps it too, unless it is run with /PURGE.
!define WhisperPurgeQuestion "Also delete your Whisper Canary profile from this computer? Your bookmarks, history, saved passwords and settings will be gone. Choose No to keep them for a later install."

# The installer's certificate name and issuer expected by the stub installer.
# Whisper builds no stub installer and is not code-signed yet (milestone 2).
!define CertNameDownload   "Whisper"
!define CertIssuerDownload "Whisper"

# Dialog units are used so the UI displays correctly with the system's DPI
# settings.
!define PROFILE_CLEANUP_LABEL_TOP "35u"
!define PROFILE_CLEANUP_LABEL_LEFT "0"
!define PROFILE_CLEANUP_LABEL_WIDTH "100%"
!define PROFILE_CLEANUP_LABEL_HEIGHT "80u"
!define PROFILE_CLEANUP_LABEL_ALIGN "center"
!define PROFILE_CLEANUP_CHECKBOX_LEFT "center"
!define PROFILE_CLEANUP_CHECKBOX_WIDTH "100%"
!define PROFILE_CLEANUP_BUTTON_LEFT "center"
!define INSTALL_BLURB_TOP "137u"
!define INSTALL_BLURB_WIDTH "60u"
!define INSTALL_FOOTER_TOP "-48u"
!define INSTALL_FOOTER_WIDTH "250u"
!define INSTALL_INSTALLING_TOP "70u"
!define INSTALL_INSTALLING_LEFT "0"
!define INSTALL_INSTALLING_WIDTH "100%"
!define INSTALL_PROGRESS_BAR_TOP "112u"
!define INSTALL_PROGRESS_BAR_LEFT "20%"
!define INSTALL_PROGRESS_BAR_WIDTH "60%"
!define INSTALL_PROGRESS_BAR_HEIGHT "12u"

!define PROFILE_CLEANUP_CHECKBOX_TOP_MARGIN "20u"
!define PROFILE_CLEANUP_BUTTON_TOP_MARGIN "20u"
!define PROFILE_CLEANUP_BUTTON_X_PADDING "40u"
!define PROFILE_CLEANUP_BUTTON_Y_PADDING "4u"

# Font settings that can be customized for each channel
!define INSTALL_HEADER_FONT_SIZE 28
!define INSTALL_HEADER_FONT_WEIGHT 400
!define INSTALL_INSTALLING_FONT_SIZE 28
!define INSTALL_INSTALLING_FONT_WEIGHT 400

# UI Colors that can be customized for each channel
!define COMMON_TEXT_COLOR 0xFFFFFF
!define COMMON_BACKGROUND_COLOR 0x161311
!define INSTALL_INSTALLING_TEXT_COLOR 0xFFFFFF
