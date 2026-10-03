#!/usr/bin/env bash
# adb against the phone (DEVICE, default the Samsung), plus a screenshot shortcut.
#   android/adb.sh shell getprop ro.build.version.release
#   android/adb.sh shot NAME     -> android/out/shots/NAME.png
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
device="${DEVICE:-<phone-serial>}"
[ $# -ge 1 ] || { echo "usage: adb.sh ARGS... | shot NAME" >&2; exit 1; }
if [ "$1" = shot ]; then
  mkdir -p "$here/out/shots"
  out="$here/out/shots/${2:?usage: adb.sh shot NAME}.png"
  adb -s "$device" exec-out screencap -p > "$out"
  echo "$out"
else
  exec adb -s "$device" "$@"
fi
