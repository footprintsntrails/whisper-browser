#!/bin/sh
# Installs this copy of Whisper for you alone, without root:
#   - the program folder goes to ~/.local/share/whisper (or $XDG_DATA_HOME/whisper), a
#     folder you own, so Whisper updates itself there;
#   - a menu entry (~/.local/share/applications/whisper.desktop) and icons;
#   - the command `whisper` in ~/.local/bin.
# Whisper Canary (the test edition) installs next to it under its own name,
# whisper-canary, everywhere above, with its own profile; the name comes from the
# program's application.ini (RemotingName, CodeName, Profile).
#
#   ./install.sh                     install, or replace an older copy there
#   ./install.sh --uninstall         remove Whisper; your profile stays
#   ./install.sh --uninstall --purge remove Whisper and your profile (bookmarks,
#                                    history, passwords, settings)
#
# This file is part of Whisper (gecko/overlay/browser/components/whisper/linux/).
set -eu

here=$(cd "$(dirname "$0")" && pwd -P)

ini() { sed -n "s/^$1=//p" "$here/application.ini" | tr -d '\r' | head -n 1; }

# whisper (Whisper) or whisper-canary (Whisper Canary): the folder, menu entry, icon and
# command name, and the window class the desktop matches the menu entry with.
id=$(ini RemotingName)
name=$(ini CodeName)
case "$id" in
  whisper | whisper-canary) ;;
  *) echo "$here/application.ini names an unknown program '$id'" >&2; exit 1 ;;
esac
[ -n "$name" ] || name=Whisper

data=${XDG_DATA_HOME:-$HOME/.local/share}
config=${XDG_CONFIG_HOME:-$HOME/.config}
cache=${XDG_CACHE_HOME:-$HOME/.cache}
dest="$data/$id"
bin="$HOME/.local/bin"
desktop="$data/applications/$id.desktop"
sizes="16 32 48 64 128"

# The profile and its cache: Whisper keeps them in config/whisper/whisper (vendor and
# name), the canary in "config/whisper canary" (application.ini Profile, lower case).
profile=$(ini Profile | tr '[:upper:]' '[:lower:]')
if [ -n "$profile" ]; then
  profile_config="$config/$profile"
  profile_cache="$cache/$profile"
  profile_shown="$config/$profile"
else
  profile_config="$config/whisper/whisper"
  profile_cache="$cache/whisper/whisper"
  profile_shown="$config/whisper"
fi

is_whisper() {
  [ -f "$1/application.ini" ] && grep -qx 'Vendor=Whisper' "$1/application.ini" &&
    grep -qx "RemotingName=$id" "$1/application.ini"
}

remove_entries() {
  rm -f "$desktop"
  for size in $sizes; do
    rm -f "$data/icons/hicolor/${size}x$size/apps/$id.png"
  done
  if [ -L "$bin/$id" ]; then
    rm -f "$bin/$id"
  fi
}

# Whether a running program was started from this folder (Linux: /proc).
running_from() {
  for exe in /proc/[0-9]*/exe; do
    target=$(readlink "$exe" 2> /dev/null) || continue
    case "$target" in
      "$1"/*) return 0 ;;
    esac
  done
  return 1
}

refresh_caches() {
  if command -v update-desktop-database > /dev/null 2>&1; then
    update-desktop-database -q "$data/applications" 2> /dev/null || true
  fi
  if command -v gtk-update-icon-cache > /dev/null 2>&1; then
    gtk-update-icon-cache -q -t "$data/icons/hicolor" 2> /dev/null || true
  fi
}

uninstall() {
  purge=$1
  if running_from "$dest"; then
    echo "$name is running from $dest; close it first" >&2
    exit 1
  fi
  remove_entries
  if [ -d "$dest" ]; then
    if ! is_whisper "$dest"; then
      echo "$dest does not look like $name; not removing it" >&2
      exit 1
    fi
    rm -rf "$dest"
  fi
  if [ "$purge" = yes ]; then
    # Only this program's own folders: the profile and its cache. Other programs'
    # files next to them (in config/whisper or cache/whisper) stay.
    rm -rf "$profile_config" "$profile_cache"
    if [ -z "$profile" ]; then
      rmdir "$config/whisper" "$cache/whisper" 2> /dev/null || true
    fi
    echo "$name and your $name profile are removed."
  else
    echo "$name is removed. Your profile stays in $profile_shown; run with --uninstall --purge to remove it too."
  fi
  refresh_caches
}

install() {
  if ! is_whisper "$here"; then
    echo "$here is not a $name folder" >&2
    exit 1
  fi
  mkdir -p "$data"
  if [ "$here" != "$(cd "$data" && pwd -P)/$id" ]; then
    if [ -e "$dest" ] && ! is_whisper "$dest"; then
      echo "$dest exists and is not $name; not replacing it" >&2
      exit 1
    fi
    if running_from "$dest"; then
      echo "$name is running from $dest; close it first" >&2
      exit 1
    fi
    # Copy next to the old copy first, then swap, so a failed copy changes nothing.
    rm -rf "$dest.new"
    cp -R "$here" "$dest.new"
    rm -rf "$dest"
    mv "$dest.new" "$dest"
  fi

  mkdir -p "$data/applications" "$bin"
  for size in $sizes; do
    icon="$dest/browser/chrome/icons/default/default$size.png"
    if [ -f "$icon" ]; then
      mkdir -p "$data/icons/hicolor/${size}x$size/apps"
      cp "$icon" "$data/icons/hicolor/${size}x$size/apps/$id.png"
    fi
  done
  cat > "$desktop" << EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=$name
GenericName=Web Browser
Comment=A quiet browser built on Firefox
Exec="$dest/whisper" %u
Icon=$id
Terminal=false
StartupNotify=true
StartupWMClass=$id
Categories=Network;WebBrowser;
MimeType=text/html;application/xhtml+xml;x-scheme-handler/http;x-scheme-handler/https;
Keywords=Internet;WWW;Browser;Web;
Actions=new-window;new-private-window;

[Desktop Action new-window]
Name=New Window
Exec="$dest/whisper" --new-window %u

[Desktop Action new-private-window]
Name=New Private Window
Exec="$dest/whisper" --private-window %u
EOF
  if [ -e "$bin/$id" ] && [ ! -L "$bin/$id" ]; then
    echo "$bin/$id exists and is not a link; leaving it alone" >&2
  else
    ln -sf "$dest/whisper" "$bin/$id"
  fi
  refresh_caches
  echo "$name is installed in $dest, with a menu entry and the command $bin/$id."
  echo "It updates itself there. To remove it: $dest/install.sh --uninstall"
}

case "${1:-}" in
  "") install ;;
  --uninstall)
    case "${2:-}" in
      "") uninstall no ;;
      --purge) uninstall yes ;;
      *) echo "usage: install.sh [--uninstall [--purge]]" >&2; exit 2 ;;
    esac ;;
  -h | --help) sed -n '2,12p' "$0" ;;
  *) echo "usage: install.sh [--uninstall [--purge]]" >&2; exit 2 ;;
esac
