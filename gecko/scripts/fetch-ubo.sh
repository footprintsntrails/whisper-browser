#!/usr/bin/env bash
# Downloads the uBlock Origin XPI pinned in gecko/ubo.env and checks its SHA-256.
# The XPI stays exactly as addons.mozilla.org signed it.
#
# Usage: fetch-ubo.sh DIR      (writes DIR/$UBO_ID.xpi)
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=gecko/ubo.env
. "$here/ubo.env"

dir=${1:?usage: fetch-ubo.sh DIR}
mkdir -p "$dir"
out="$dir/$UBO_ID.xpi"
if [ -f "$out" ] && echo "$UBO_SHA256  $out" | sha256sum -c --quiet - 2>/dev/null; then
  echo "$out: already here and verified"
  exit 0
fi
curl -fsSL --retry 5 --retry-delay 10 --proto '=https' --tlsv1.2 -o "$out.part" "$UBO_URL"
echo "$UBO_SHA256  $out.part" | sha256sum -c --quiet -
mv "$out.part" "$out"
echo "$out: uBlock Origin $UBO_VERSION, SHA-256 verified"
