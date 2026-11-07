#!/usr/bin/env bash
# Runs network_test.py for an unpacked Linux build inside a network namespace with no
# way out, where every host name resolves to 127.0.0.1 (dnsstub.py --answer) and a
# local sink (sink.py) answers every connection there, including TLS and DNS over
# HTTPS. So the test sees the requests that follow a server's reply, not only the
# first lookups (run-smoke.sh covers those, with "no such name" for everything).
#
# Usage (as a user who may use sudo, as on GitHub's runners):
#   run-network.sh APP_DIR OUT_DIR [SECONDS]
# Writes OUT_DIR/network-report.{json,md}, network-hostnames.txt, dns-queries.log and
# sink.jsonl. Exits 1 when a host outside gecko/network-allowlist.txt shows up.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
app=$(realpath "$1")
out=$(realpath -m "$2")
seconds=${3:-420}
mkdir -p "$out"
: > "$out/dns-queries.log"
: > "$out/sink.jsonl"

status=0
# shellcheck disable=SC2016  # expanded by the inner shell
sudo unshare --net -- env HERE="$here" APP="$app" OUT="$out" SECONDS_RUN="$seconds" \
  RUN_AS="$(id -un)" RUN_HOME="$HOME" PATH="$PATH" bash -euo pipefail -c '
  ip link set lo up
  Xvfb :98 -screen 0 1440x900x24 -nolisten tcp -ac >"$OUT/xvfb-network.log" 2>&1 &
  xvfb=$!
  sleep 2
  python3 "$HERE/dnsstub.py" --listen 127.0.0.53 --listen 127.0.0.1 --answer 127.0.0.1 \
    --log "$OUT/dns-queries.log" &
  stub=$!
  python3 "$HERE/sink.py" --log "$OUT/sink.jsonl" --ca "$OUT/ca" >"$OUT/sink-out.log" 2>&1 &
  sink=$!
  for _ in $(seq 1 50); do [ -f "$OUT/ca/ca.pem" ] && break; sleep 0.2; done
  # The browser runs as the normal user and must read the CA it is told to trust.
  chmod 755 "$OUT/ca" && chmod 644 "$OUT/ca/ca.pem"
  sleep 1
  rc=0
  runuser -u "$RUN_AS" -- env HOME="$RUN_HOME" PATH="$PATH" DISPLAY=:98 \
    python3 "$HERE/network_test.py" --app "$APP" --out "$OUT" --ca "$OUT/ca" \
      --sink-log "$OUT/sink.jsonl" --dns-log "$OUT/dns-queries.log" --seconds "$SECONDS_RUN" || rc=$?
  kill "$stub" "$sink" "$xvfb" || true
  exit "$rc"
' || status=$?

sudo chown -R "$(id -u):$(id -g)" "$out"
# The private key of the throw-away CA is of no use after the run.
rm -f "$out"/ca/*.key
exit "$status"
