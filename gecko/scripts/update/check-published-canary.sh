#!/usr/bin/env bash
# After a canary publish: downloads the Linux tarball back from the canary release,
# checks it is byte for byte the one that was published from DIST, and starts it
# (start-check-package.sh). If it does not start, the build's MARs are deleted from
# the release at once, so no installed canary can update to it (the update host still
# points at them only until the next deploy, which this failure stops), and the job
# fails.
#
# Usage: check-published-canary.sh DIST OUT_DIR      (gh with GH_TOKEN; sudo)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
dist=$(realpath "$1")
out=$(realpath -m "$2")
# shellcheck source=gecko/release.env
. "$here/../../release.env"
repo=${GITHUB_REPOSITORY:-footprintsntrails/whisper-browser}

mkdir -p "$out/download"
gh release download "$CANARY_TAG" --repo "$repo" --dir "$out/download" --clobber \
  --pattern whisper-canary-linux-x86_64.tar.xz --pattern canary.json
want=$(sha256sum "$dist/whisper-canary-linux-x86_64.tar.xz" | cut -d' ' -f1)
have=$(sha256sum "$out/download/whisper-canary-linux-x86_64.tar.xz" | cut -d' ' -f1)
if [ "$want" != "$have" ]; then
  echo "::error::the release serves another whisper-canary-linux-x86_64.tar.xz ($have) than was published ($want)"
  exit 1
fi
build_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["builds"]["linux-x86_64"]["build_id"])' \
  "$out/download/canary.json")
echo "published tarball: SHA-256 $have, build $build_id"
if ! "$here/start-check-package.sh" "$out/download/whisper-canary-linux-x86_64.tar.xz" "$build_id" \
  "$CANARY_UPDATE_CHANNEL" "$out/start"; then
  echo "::error::the published canary (build $build_id) does not start; withdrawing its MARs"
  for mar in $(python3 -c 'import json,sys; print(" ".join(b["mar"] for b in json.load(open(sys.argv[1]))["builds"].values()))' \
    "$out/download/canary.json"); do
    gh release delete-asset "$CANARY_TAG" "$mar" --repo "$repo" --yes || true
  done
  exit 1
fi
echo "the published canary (build $build_id) starts"
