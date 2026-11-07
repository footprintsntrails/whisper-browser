#!/usr/bin/env bash
# Downloads the Firefox source tarball named in gecko/firefox.env into DIR and checks it:
#   1. SHA512SUMS is signed by Mozilla's release key (primary key fingerprint pinned in
#      firefox.env; the key itself comes from the release's KEY file and is only trusted
#      if its fingerprint matches),
#   2. the tarball's SHA-512 matches SHA512SUMS and the value pinned in firefox.env.
# Nothing is unpacked unless every check passes.
#
# With --firefox-linux it fetches Mozilla's own Linux x86_64 build of the same version
# instead (firefox-<version>.tar.xz, checked against the signed SHA512SUMS), for the
# checks that compare Whisper with Firefox (gecko/scripts/webexposed.py).
#
# Usage: fetch-source.sh DIR [--firefox-linux]
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=gecko/firefox.env
. "$here/firefox.env"

dir=${1:?usage: fetch-source.sh DIR [--firefox-linux]}
what=${2:-source}
mkdir -p "$dir"
cd "$dir"

base="https://archive.mozilla.org/pub/firefox/releases/$FIREFOX_VERSION"
tarball="firefox-$FIREFOX_VERSION.source.tar.xz"

fetch() { curl -fsSL --retry 5 --retry-delay 10 --proto '=https' --tlsv1.2 -o "$2" "$1"; }

fetch "$base/KEY" KEY
fetch "$base/SHA512SUMS" SHA512SUMS
fetch "$base/SHA512SUMS.asc" SHA512SUMS.asc

gnupg=$(mktemp -d)
trap 'rm -rf "$gnupg"' EXIT
export GNUPGHOME="$gnupg"
gpg --batch --quiet --import KEY
primaries=$(gpg --batch --with-colons --fingerprint | awk -F: '$1 == "pub" { want = 1; next } want && $1 == "fpr" { print $10; want = 0 }')
if [ "$primaries" != "$MOZILLA_RELEASE_KEY_FPR" ]; then
  echo "KEY holds primary key(s) '$primaries', expected only $MOZILLA_RELEASE_KEY_FPR" >&2
  exit 1
fi
# The signature must verify and be made by the pinned primary key or one of its subkeys.
status=$(gpg --batch --status-fd 1 --verify SHA512SUMS.asc SHA512SUMS 2>/dev/null)
echo "$status" | grep -q '^\[GNUPG:\] GOODSIG ' || { echo "SHA512SUMS: bad signature" >&2; exit 1; }
validsig=$(echo "$status" | awk '$2 == "VALIDSIG" { print $NF }')
if [ "$validsig" != "$MOZILLA_RELEASE_KEY_FPR" ]; then
  echo "SHA512SUMS is signed by a key whose primary is '$validsig', not $MOZILLA_RELEASE_KEY_FPR" >&2
  exit 1
fi
echo "SHA512SUMS: good signature from $(echo "$status" | awk '$2 == "VALIDSIG" { print $3 }') (primary $validsig)"

if [ "$what" = --firefox-linux ]; then
  build="firefox-$FIREFOX_VERSION.tar.xz"
  sum=$(awk -v f="linux-x86_64/en-US/$build" '$2 == f { print $1 }' SHA512SUMS)
  [ -n "$sum" ] || { echo "SHA512SUMS has no linux-x86_64/en-US/$build" >&2; exit 1; }
  fetch "$base/linux-x86_64/en-US/$build" "$build.part"
  echo "$sum  $build.part" | sha512sum -c --quiet -
  mv "$build.part" "$build"
  echo "$build: SHA-512 verified against the signed SHA512SUMS"
  exit 0
elif [ "$what" != source ]; then
  echo "unknown option $what" >&2
  exit 1
fi

listed=$(awk -v f="source/$tarball" '$2 == f { print $1 }' SHA512SUMS)
if [ "$listed" != "$FIREFOX_SOURCE_SHA512" ]; then
  echo "SHA512SUMS lists '$listed' for $tarball, firefox.env pins $FIREFOX_SOURCE_SHA512" >&2
  exit 1
fi

if [ -f "$tarball" ] && echo "$FIREFOX_SOURCE_SHA512  $tarball" | sha512sum -c --quiet - 2>/dev/null; then
  echo "$tarball: already here and verified"
  exit 0
fi
fetch "$base/source/$tarball" "$tarball.part"
echo "$FIREFOX_SOURCE_SHA512  $tarball.part" | sha512sum -c --quiet -
mv "$tarball.part" "$tarball"
echo "$tarball: SHA-512 verified"
