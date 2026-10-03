#!/usr/bin/env bash
# adb against the phone (see below for picking it), plus a screenshot shortcut.
#   android/adb.sh shell getprop ro.build.version.release
#   android/adb.sh shot NAME     -> android/out/shots/NAME.png
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# The phone's adb serial (`adb devices`): DEVICE=, else ~/.config/tmls-android/device.
device="${DEVICE:-$(cat "${TMLS_ANDROID_KEYS:-$HOME/.config/tmls-android}/device" 2>/dev/null || true)}"
[ -n "$device" ] || { echo "set DEVICE=<serial> (adb devices) or write it to ~/.config/tmls-android/device" >&2; exit 1; }
[ $# -ge 1 ] || { echo "usage: adb.sh ARGS... | shot NAME" >&2; exit 1; }
if [ "$1" = shot ]; then
  mkdir -p "$here/out/shots"
  out="$here/out/shots/${2:?usage: adb.sh shot NAME}.png"
  adb -s "$device" exec-out screencap -p > "$out"
  echo "$out"
else
  exec adb -s "$device" "$@"
fi
