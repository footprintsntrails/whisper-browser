#!/usr/bin/env bash
# Downloads the add-ons Whisper ships, each pinned with its SHA-256: uBlock Origin
# (gecko/ubo.env) and Dark Reader (gecko/darkreader.env). The XPIs stay exactly as
# addons.mozilla.org signed them.
#
# Usage: fetch-ubo.sh DIR      (writes DIR/$UBO_ID.xpi and DIR/$DARKREADER_ID.xpi)
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=gecko/ubo.env
. "$here/ubo.env"
# shellcheck source=gecko/darkreader.env
. "$here/darkreader.env"

dir=${1:?usage: fetch-ubo.sh DIR}
mkdir -p "$dir"

# fetch NAME ID VERSION URL SHA256
fetch() {
  local out="$dir/$2.xpi"
  if [ -f "$out" ] && echo "$5  $out" | sha256sum -c --quiet - 2>/dev/null; then
    echo "$out: already here and verified"
    return
  fi
  curl -fsSL --retry 5 --retry-delay 10 --proto '=https' --tlsv1.2 -o "$out.part" "$4"
  echo "$5  $out.part" | sha256sum -c --quiet -
  mv "$out.part" "$out"
  echo "$out: $1 $3, SHA-256 verified"
}
fetch "uBlock Origin" "$UBO_ID" "$UBO_VERSION" "$UBO_URL" "$UBO_SHA256"
fetch "Dark Reader" "$DARKREADER_ID" "$DARKREADER_VERSION" "$DARKREADER_URL" "$DARKREADER_SHA256"
