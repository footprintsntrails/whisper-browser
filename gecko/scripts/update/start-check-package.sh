#!/usr/bin/env bash
# Starts a Linux package the way an installed copy is run, and checks it runs
# (start_check.py): unpacked by root with the owners and modes the archive gives (as
# for an install in /opt), run by a different, unprivileged user, non-headless under
# Xvfb. A file the archive makes readable for its owner only fails here, as it would
# for that user.
#
# Usage (as a user who may use sudo, as on GitHub's runners):
#   start-check-package.sh PACKAGE.tar.xz BUILD_ID CHANNEL OUT_DIR
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
package=$(realpath "$1")
build_id=$2
channel=$3
out=$(realpath -m "$4")
mkdir -p "$out"

user=whisper-start-check
if ! id -u "$user" > /dev/null 2>&1; then
  sudo useradd --create-home --shell /usr/sbin/nologin "$user"
fi
# The browser's content sandbox needs user namespaces (Ubuntu 24.04 restricts them).
sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0 > /dev/null || true

work=$(sudo mktemp -d /opt/whisper-start-check.XXXXXX)
sudo chmod 755 "$work"
sudo tar -xJf "$package" -C "$work" --same-owner --same-permissions
scripts=$(sudo mktemp -d /opt/whisper-start-scripts.XXXXXX)
sudo mkdir -p "$scripts/update" "$scripts/smoke"
sudo cp "$here/start_check.py" "$scripts/update/"
sudo cp "$here/../smoke/marionette.py" "$scripts/smoke/"
sudo chmod -R a+rX "$scripts"
sudo chmod 755 "$scripts"

Xvfb :97 -screen 0 1440x900x24 -nolisten tcp -ac > "$out/xvfb.log" 2>&1 &
xvfb=$!
trap 'kill "$xvfb" 2> /dev/null || true' EXIT
sleep 2

status=0
sudo -u "$user" env -i PATH=/usr/bin:/bin HOME="/home/$user" DISPLAY=:97 \
  python3 "$scripts/update/start_check.py" --app "$work/whisper" --build-id "$build_id" --channel "$channel" \
  --out "/tmp/$user-out" || status=$?
sudo cp -r "/tmp/$user-out/." "$out/" 2> /dev/null || true
sudo chown -R "$(id -u):$(id -g)" "$out"
exit "$status"
