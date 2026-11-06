#!/usr/bin/env bash
# Runs the update test (update_test.py run) for an unpacked Linux build inside a network
# namespace with no way out, like the smoke test: the only server the browser can reach
# is the local update server the test starts on 127.0.0.1, and a DNS server that answers
# every other name with "no such name" (and writes it down).
#
# Usage (as a user who may use sudo, as on GitHub's runners):
#   run-update-test.sh BUNDLE APP_DIR WORK_DIR OUT_DIR [CASES]
#   BUNDLE    made by `update_test.py prepare`
#   APP_DIR   unpacked package A, already patched with `update_test.py patch`
#   CASES     comma-separated (default: all the bundle has)
# Environment: CANARY_APP, an unpacked or installed Whisper Canary A, already patched
# with `update_test.py patch --canary`, for the canary cases.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
bundle=$(realpath "$1")
app=$(realpath "$2")
work=$(realpath -m "$3")
out=$(realpath -m "$4")
cases=${5:-}
canary=${CANARY_APP:+$(realpath "$CANARY_APP")}
mkdir -p "$work" "$out"

status=0
# shellcheck disable=SC2016  # expanded by the inner shell
sudo unshare --net -- env HERE="$here" BUNDLE="$bundle" APP="$app" WORK="$work" OUT="$out" CASES="$cases" \
  CANARY="$canary" \
  RUN_AS="$(id -un)" RUN_HOME="$HOME" PATH="$PATH" bash -euo pipefail -c '
  ip link set lo up
  # -noreset: by default the X server resets when its last client goes away, which is
  # exactly when "Restart to update" ends the old browser and the new one connects; a
  # browser that connects during the reset fails with "cannot open display".
  Xvfb :99 -screen 0 1440x900x24 -nolisten tcp -ac -noreset >"$OUT/xvfb.log" 2>&1 &
  xvfb=$!
  sleep 2
  python3 "$HERE/../smoke/dnsstub.py" --listen 127.0.0.53 --listen 127.0.0.1 --log "$OUT/dns-queries.log" &
  stub=$!
  sleep 1
  rc=0
  runuser -u "$RUN_AS" -- env HOME="$RUN_HOME" PATH="$PATH" DISPLAY=:99 \
    python3 "$HERE/update_test.py" run --bundle "$BUNDLE" --app "$APP" --work "$WORK" --out "$OUT" \
      --cases "$CASES" ${CANARY:+--canary-app "$CANARY"} || rc=$?
  kill "$stub" "$xvfb" || true
  exit "$rc"
' || status=$?

sudo chown -R "$(id -u):$(id -g)" "$out" "$work" 2>/dev/null || true
exit "$status"
