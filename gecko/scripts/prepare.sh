#!/usr/bin/env bash
# Turns a verified Firefox source tarball into the Whisper source tree:
#   1. unpacks it (once) into SRC,
#   2. removes agent configuration that ships in the tarball (.claude/, .mcp.json),
#   3. applies patches/*.patch in order, with no fuzz,
#   4. copies gecko/overlay/ into the tree; overlay files must be NEW files, so a
#      Firefox file is only ever changed by a patch that says why,
#   5. adds what lives elsewhere in this repository (Inter fonts, icons, installer art,
#      the installer's self-extractor with Whisper's icon) and the verified uBlock Origin XPI,
#   6. checks the MAR signing certificates against the fingerprints in release.env and
#      writes Whisper's own version into the branding prefs,
#   7. writes SRC/mozconfig for TARGET (with the MAR channel from release.env).
# For the canary VARIANT it also makes Whisper Canary's branding directory
# (browser/branding/whisper-canary: the release one with gecko/variants/canary/ over it)
# and builds with it, on the canary update and MAR channels (the channel also gives it
# its own profile folder, patches/0029); release.env has the canary values.
#
# Usage: prepare.sh TARBALL UBO_XPI SRC TARGET [VARIANT]
#   TARGET:  linux-x86_64 | windows-x86_64
#   VARIANT: release (default) | canary
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
gecko="$repo/gecko"
# shellcheck source=gecko/firefox.env
. "$gecko/firefox.env"
# shellcheck source=gecko/ubo.env
. "$gecko/ubo.env"
# shellcheck source=gecko/release.env
. "$gecko/release.env"

tarball=${1:?usage: prepare.sh TARBALL UBO_XPI SRC TARGET}
ubo=${2:?usage: prepare.sh TARBALL UBO_XPI SRC TARGET}
src=${3:?usage: prepare.sh TARBALL UBO_XPI SRC TARGET}
target=${4:?usage: prepare.sh TARBALL UBO_XPI SRC TARGET}
variant=${5:-release}
case "$variant" in
  release) update_channel=$UPDATE_CHANNEL mar_channel=$MAR_CHANNEL_ID ;;
  canary) update_channel=$CANARY_UPDATE_CHANNEL mar_channel=$CANARY_MAR_CHANNEL_ID ;;
  *) echo "unknown variant $variant" >&2; exit 1 ;;
esac

[ -f "$gecko/mozconfigs/$target" ] || { echo "unknown target $target" >&2; exit 1; }
echo "$FIREFOX_SOURCE_SHA512  $tarball" | sha512sum -c --quiet -
echo "$UBO_SHA256  $ubo" | sha256sum -c --quiet -

# SRC is a new folder (prepare.sh makes it) in an existing one. Every file this script
# writes goes inside it, so it is checked hard: not empty, not "/", not there yet, its
# parent there; from here on it is an absolute path.
case "$src" in
  "" | / | . | ..) echo "SRC '$src' is not a usable folder for the source tree" >&2; exit 1 ;;
esac
if [ -e "$src" ] || [ -L "$src" ]; then
  echo "$src already exists; remove it first" >&2
  exit 1
fi
parent=$(dirname -- "$src")
if [ ! -d "$parent" ]; then
  echo "$parent (where SRC goes) does not exist" >&2
  exit 1
fi
mkdir -- "$src"
src=$(cd -- "$src" && pwd -P)
case "$src" in
  / | "") echo "SRC resolved to '$src'; refusing" >&2; exit 1 ;;
esac
echo "Unpacking $(basename "$tarball")"
tar -xJf "$tarball" -C "$src" --strip-components=1
[ -f "$src/browser/config/version.txt" ] || { echo "not a Firefox source tree" >&2; exit 1; }
[ "$(cat "$src/browser/config/version.txt")" = "$FIREFOX_VERSION" ] || {
  echo "tarball is Firefox $(cat "$src/browser/config/version.txt"), firefox.env says $FIREFOX_VERSION" >&2
  exit 1
}

# Agent settings in the tarball can start mach and other tools; they are not part of the
# build. Vendored Rust crates are left alone: cargo checks every file in them against
# .cargo-checksum.json (and their settings only describe that crate's own repository).
find "$src" -path "$src/third_party/rust" -prune -o \
  \( -name .claude -type d -prune -exec rm -rf {} + \) -o \( -name .mcp.json -type f -exec rm -f {} + \)

echo "Applying patches"
count=0
for p in "$repo"/patches/*.patch; do
  [ -e "$p" ] || continue
  echo "  $(basename "$p")"
  patch -d "$src" -p1 --forward --batch --fuzz=0 --no-backup-if-mismatch --quiet < "$p"
  count=$((count + 1))
done
echo "  $count patches applied"

echo "Copying the overlay (new files only)"
(cd "$gecko/overlay" && find . -type f | sort) | while read -r f; do
  if [ -e "$src/$f" ]; then
    echo "overlay file $f already exists in Firefox; change Firefox files with a patch" >&2
    exit 1
  fi
  mkdir -p "$(dirname "$src/$f")"
  cp "$gecko/overlay/$f" "$src/$f"
done

# Firefox's own about dialog stylesheet in Whisper's branding folder: chrome://branding/
# is content accessible, so web pages could tell a changed copy apart. Whisper's look
# for the dialog is chrome://whisper/content/aboutDialog.css (WhisperAbout.sys.mjs).
cp "$src/browser/branding/official/content/aboutDialog.css" "$src/browser/branding/whisper/content/aboutDialog.css"

# overlay/browser/app/whisper.exe.manifest is Firefox's firefox.exe.manifest under the
# name the build looks for when the program is called whisper (without it Windows runs
# whisper.exe as a program for an old Windows version; LibreWolf found this breaks the
# title bar). Windows refuses to start a program whose manifest is not valid XML
# ("side-by-side configuration is incorrect"), so check it here.
python3 -c 'import sys, xml.dom.minidom; xml.dom.minidom.parse(sys.argv[1])' \
  "$src/browser/app/whisper.exe.manifest"

branding="$src/browser/branding/whisper"
whisper="$src/browser/components/whisper"
icon="$repo/assets/icon/whisper.ico"
art="$repo/assets/installer"

if [ "$variant" = canary ]; then
  # Whisper Canary: the release branding with the canary's own files over it (names,
  # NSIS defines and the images from `cargo xtask canary-art`); every canary file
  # replaces a release one, so both directories hold the same set of files.
  canary="$gecko/variants/canary"
  cp -R "$branding" "$src/browser/branding/whisper-canary"
  branding="$src/browser/branding/whisper-canary"
  (cd "$canary/branding" && find . -type f ! -path ./pref/canary.js | sort) | while read -r f; do
    if [ ! -e "$branding/$f" ]; then
      echo "gecko/variants/canary/branding/$f replaces nothing in the release branding" >&2
      exit 1
    fi
    cp "$canary/branding/$f" "$branding/$f"
  done
  {
    echo
    cat "$canary/branding/pref/canary.js"
  } >> "$branding/pref/firefox-branding.js"
  icon="$canary/icon/whisper-canary.ico"
  art="$canary/installer"
  echo "Whisper Canary branding in browser/branding/whisper-canary"
fi

# Fonts: Inter, with its license (SIL Open Font License), from assets/fonts.
mkdir -p "$whisper/content/fonts"
cp "$repo"/assets/fonts/Inter-{Regular,Medium,SemiBold}.woff2 "$repo/assets/fonts/OFL.txt" \
  "$whisper/content/fonts/"

# Windows icons and installer artwork come from assets/ (rendered by `cargo xtask icons`
# and `cargo xtask installer-art`; the canary's from `cargo xtask canary-art`). setup.ico
# is the installer's and uninstaller's own icon (patches/0030).
for ico in firefox firefox64 document document_pdf newtab newwindow pbmode setup; do
  cp "$icon" "$branding/$ico.ico"
done
cp "$art/header.bmp" "$branding/wizHeader.bmp"
cp "$art/header.bmp" "$branding/wizHeaderRTL.bmp"
cp "$art/sidebar.bmp" "$branding/wizWatermark.bmp"

# uBlock Origin, unchanged, as a distribution add-on.
mkdir -p "$whisper/distribution"
cp "$ubo" "$whisper/distribution/$UBO_ID.xpi"

# The updater trusts exactly these certificates (patches/0020: release builds the
# primary and secondary ones, canary builds the canary one); any other file here would
# mean trusting updates signed by someone else.
check_mar_cert() {
  local have
  have=$(openssl x509 -inform DER -in "$src/toolkit/mozapps/update/updater/whisper_$1.der" \
    -noout -fingerprint -sha256 | cut -d= -f2)
  if [ "$have" != "$2" ]; then
    echo "whisper_$1.der has SHA-256 $have, release.env pins $2" >&2
    exit 1
  fi
  echo "MAR $1 certificate: SHA-256 $have"
}
check_mar_cert primary "$MAR_PRIMARY_SHA256"
check_mar_cert secondary "$MAR_SECONDARY_SHA256"
check_mar_cert canary "$MAR_CANARY_SHA256"

# Whisper's own version, for the about dialog; the application version stays Firefox's.
if ! echo "$WHISPER_VERSION" | grep -Eqx '[0-9]+\.[0-9]+\.[0-9]+'; then
  echo "WHISPER_VERSION '$WHISPER_VERSION' in release.env is not like 0.1.0" >&2
  exit 1
fi
{
  echo
  echo "// Whisper's own version (added by gecko/scripts/prepare.sh from gecko/release.env)."
  echo "pref(\"whisper.version\", \"$WHISPER_VERSION\", locked);"
} >> "$branding/pref/firefox-branding.js"
# ... and for Apps & Features on Windows (patches/0025).
{
  echo
  echo "# Whisper's own version (added by gecko/scripts/prepare.sh from gecko/release.env)."
  echo "!define WhisperVersion \"$WHISPER_VERSION\""
} >> "$branding/branding.nsi"

# The downloaded installer starts with Firefox's 7-Zip self-extractor, a ready-made
# program with Firefox's icon and file details inside; Windows shows those for
# Whisper-Setup.exe. The branding folder gets a copy with Whisper's icon and details,
# which the build uses instead (patches/0030).
case "$target" in
  windows-*)
    case "$variant" in
      release) product="Whisper" setup_name="Whisper-Setup.exe" ;;
      canary) product="Whisper Canary" setup_name="Whisper-Canary-Setup.exe" ;;
    esac
    python3 "$gecko/scripts/winres.py" rebrand-sfx --ico "$icon" --product "$product" \
      --version "$WHISPER_VERSION" --filename "$setup_name" \
      "$src/other-licenses/7zstub/firefox/7zSD.Win32.sfx" "$branding/7zSD.Win32.sfx"
    ;;
esac

# The mozconfig names the release branding and update channel once each; the canary
# changes exactly those two lines.
replace_once() {
  local count
  count=$(grep -cxF -- "$1" "$src/mozconfig" || true)
  if [ "$count" != 1 ]; then
    echo "mozconfig: expected the line '$1' once, found it $count times" >&2
    exit 1
  fi
  python3 -c 'import sys; p, a, b = sys.argv[1:]; s = open(p).read().split("\n"); open(p, "w").write("\n".join(b if l == a else l for l in s))' \
    "$src/mozconfig" "$1" "$2"
}
[ -f "$src/browser/config/version.txt" ] || { echo "$src is not the source tree" >&2; exit 1; }
{
  cat "$gecko/mozconfigs/common" "$gecko/mozconfigs/$target"
  echo
  echo "# --- Channels (gecko/release.env, added by prepare.sh) ---------------------------"
  echo "ac_add_options MAR_CHANNEL_ID=$mar_channel"
  echo "ac_add_options ACCEPTED_MAR_CHANNEL_IDS=$mar_channel"
} > "$src/mozconfig"
if [ "$variant" = canary ]; then
  replace_once "ac_add_options --with-branding=browser/branding/whisper" \
    "ac_add_options --with-branding=browser/branding/whisper-canary"
  # The canary channel also gives it its own profile folder (patches/0029) and
  # program name (whisper-canary).
  replace_once "ac_add_options --enable-update-channel=release" \
    "ac_add_options --enable-update-channel=$update_channel"
fi
echo "Wrote $src/mozconfig for $target ($variant: update channel $update_channel, MAR channel $mar_channel)"
