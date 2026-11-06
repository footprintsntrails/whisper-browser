#!/usr/bin/env bash
# Makes a complete, unsigned MAR from a Whisper package with Mozilla's own tools from
# the build: the `mar` program and tools/update-packaging/make_full_update.sh (and its
# common.sh), as Mozilla makes its complete MARs.
#
# The MAR's product version is the Firefox version in three parts followed by the
# build date and time from the package's application.ini, for example
# 159.0.1.20261106.093414: Whisper keeps Firefox's version, so the build ID is what
# orders Whisper's builds, and the updater refuses a MAR older than its own build
# (patches/0022-updates-in-build-order.patch, which builds the same string).
#
# Usage: make-mar.sh PACKAGE OUT.mar [MAR_CHANNEL_ID]
#   PACKAGE         whisper-*.tar.xz (Linux), whisper-*.zip (Windows), or an unpacked
#                   package folder (the one that holds application.ini)
#   MAR_CHANNEL_ID  default: MAR_CHANNEL_ID from gecko/release.env
# Environment:
#   MAR_TOOLS       folder with mar, make_full_update.sh and common.sh (the
#                   whisper-mar-tools artifact of the Linux build)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=gecko/release.env
. "$here/../../release.env"

package=$(realpath "${1:?usage: make-mar.sh PACKAGE OUT.mar [MAR_CHANNEL_ID]}")
out=$(realpath -m "${2:?usage: make-mar.sh PACKAGE OUT.mar [MAR_CHANNEL_ID]}")
channel=${3:-$MAR_CHANNEL_ID}
tools=${MAR_TOOLS:?set MAR_TOOLS to the folder with mar, make_full_update.sh and common.sh}

for f in mar make_full_update.sh common.sh; do
  [ -f "$tools/$f" ] || { echo "$tools/$f is missing" >&2; exit 1; }
done
chmod +x "$tools/mar" "$tools/make_full_update.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if [ -d "$package" ]; then
  # make_full_update.sh works next to the folder it packs, so pack a copy.
  mkdir "$work/pkg"
  cp -a "$package" "$work/pkg/app"
  app="$work/pkg/app"
else
  mkdir "$work/pkg"
  case "$package" in
    *.tar.xz) tar -xJf "$package" -C "$work/pkg" ;;
    *.zip) python3 -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])' "$package" "$work/pkg" ;;
    *) echo "$package: not a .tar.xz, .zip or folder" >&2; exit 1 ;;
  esac
  # A package holds exactly one top folder ("whisper").
  set -- "$work"/pkg/*
  if [ $# != 1 ] || [ ! -d "$1" ]; then
    echo "$package: expected one top-level folder" >&2
    exit 1
  fi
  app=$1
fi

ini() { sed -n "s/^$1=//p" "$app/application.ini" | tr -d '\r' | head -n 1; }
[ -f "$app/application.ini" ] || { echo "no application.ini in $package" >&2; exit 1; }
version=$(ini Version)
build=$(ini BuildID)
echo "$version" | grep -Eqx '[0-9]+(\.[0-9]+){1,2}' || { echo "unexpected Version '$version'" >&2; exit 1; }
echo "$build" | grep -Eqx '[0-9]{14}' || { echo "unexpected BuildID '$build'" >&2; exit 1; }
for f in precomplete update-settings.ini; do
  [ -f "$app/$f" ] || { echo "$f is missing: was this built with the updater?" >&2; exit 1; }
done

# 159.0.1 -> 159.0.1, 160.0 -> 160.0.0; then .YYYYMMDD.HHMMSS from the build ID
three=$version
while [ "$(echo "$three" | tr -cd . | wc -c)" -lt 2 ]; do three="$three.0"; done
product_version="$three.${build:0:8}.${build:8:6}"

echo "MAR: $(basename "$out") from Whisper $version, build $build"
echo "     product version $product_version, MAR channel $channel"
MAR="$tools/mar" MOZ_PRODUCT_VERSION="$product_version" MAR_CHANNEL_ID="$channel" \
  BCJ_OPTIONS="--x86" "$tools/make_full_update.sh" -q "$work/out.mar" "$app"
mkdir -p "$(dirname "$out")"
mv "$work/out.mar" "$out"
"$tools/mar" -T "$out" | grep -E 'MAR channel|Product version|signatures|entries|^Total' || true
ls -l "$out"
