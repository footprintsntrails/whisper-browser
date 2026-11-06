#!/usr/bin/env bash
# Runs smoke.py for an unpacked Linux build inside a network namespace that has no
# network at all, only a DNS server (dnsstub.py) that answers every query with
# "no such name" and writes the name down. So the smoke test also records every host
# name the browser tries to look up on its first start, without contacting anything.
#
# Usage (as a user who may use sudo, as on GitHub's runners):
#   run-smoke.sh APP_DIR PREFS_FILE OUT_DIR [VERSION]
# Writes OUT_DIR/report.{json,md}, screenshots, dns-queries.log and
# first-start-hostnames.txt.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
app=$(realpath "$1")
prefs=$(realpath "$2")
out=$(realpath -m "$3")
version=${4:-}
mkdir -p "$out"
: > "$out/dns-queries.log"

status=0
# shellcheck disable=SC2016  # expanded by the inner shell
sudo unshare --net -- env HERE="$here" APP="$app" PREFS="$prefs" OUT="$out" VERSION="$version" \
  RUN_AS="$(id -un)" RUN_HOME="$HOME" PATH="$PATH" bash -euo pipefail -c '
  ip link set lo up
  # The X server is private to this namespace and this test, so it needs no access control.
  Xvfb :99 -screen 0 1440x900x24 -nolisten tcp -ac >"$OUT/xvfb.log" 2>&1 &
  xvfb=$!
  sleep 2
  # /etc/resolv.conf points at 127.0.0.53 (systemd-resolved) on Ubuntu; in this
  # namespace that address is ours. Started last, so only the browser is logged.
  python3 "$HERE/dnsstub.py" --listen 127.0.0.53 --listen 127.0.0.1 --log "$OUT/dns-queries.log" &
  stub=$!
  sleep 1
  rc=0
  runuser -u "$RUN_AS" -- env HOME="$RUN_HOME" PATH="$PATH" DISPLAY=:99 \
    python3 "$HERE/smoke.py" --app "$APP" --prefs "$PREFS" --out "$OUT" --version "$VERSION" || rc=$?
  kill "$stub" "$xvfb" || true
  exit "$rc"
' || status=$?

sudo chown -R "$(id -u):$(id -g)" "$out"

# Every distinct name the browser asked the DNS server for, with the number of queries
# (A, AAAA and HTTPS queries for one name count separately). The system resolver also
# retries each name with the runner's DNS search domain appended; those copies are left
# out here (they are in dns-queries.log).
search=$(awk '$1 == "search" { for (i = 2; i <= NF; i++) print $i }' /etc/resolv.conf)
awk '{ print $3 }' "$out/dns-queries.log" | while read -r name; do
  keep=1
  for d in $search; do
    case "$name" in *."$d") keep=0 ;; esac
  done
  if [ "$keep" = 1 ]; then echo "$name"; fi
done | sort | uniq -c | sort -k2 > "$out/first-start-hostnames.txt"
# The browser's own resolver log names hosts it looked up over DNS-over-HTTPS too.
cat "$out"/mozlog* 2>/dev/null | grep -o 'Resolving host \[[^]]*\]' | sed 's/Resolving host \[\(.*\)\]/\1/' \
  | sort | uniq -c | sort -k2 > "$out/resolver-hostnames.txt" || true

echo "Host names looked up on the first start (DNS server log):"
cat "$out/first-start-hostnames.txt"

# The network gate: every name must be in gecko/network-allowlist.txt.
allowlist="$here/../../network-allowlist.txt"
awk '{ print $2 }' "$out/first-start-hostnames.txt" | while read -r name; do
  if ! grep -v '^[[:space:]]*#' "$allowlist" | grep -qxF "$name"; then echo "$name"; fi
done > "$out/unexpected-hostnames.txt"
if [ -s "$out/unexpected-hostnames.txt" ]; then
  echo "::error::Host names outside gecko/network-allowlist.txt: $(tr '\n' ' ' < "$out/unexpected-hostnames.txt")"
  printf '| ❌ | first start contacts only allowed hosts | not allowed: %s |\n' \
    "$(tr '\n' ' ' < "$out/unexpected-hostnames.txt")" >> "$out/report.md"
  status=1
else
  printf '| ✅ | first start contacts only allowed hosts | %s names, all in network-allowlist.txt |\n' \
    "$(wc -l < "$out/first-start-hostnames.txt")" >> "$out/report.md"
fi
echo "Host names in the browser's resolver log:"
cat "$out/resolver-hostnames.txt"
exit "$status"
