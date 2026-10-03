#!/usr/bin/env bash
# Install an APK on the phone over USB adb (the host's adb), launch it, take a screenshot.
#   android/install.sh                         newest out/tmls-<code>.apk (release)
#   APK=out/tmls-debug.apk android/install.sh  the debug build (dev.sagun.tmls.debug)
# DEVICE picks the adb serial (default: the Samsung). Always -s: the TV box is on adb too.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
device="${DEVICE:-<phone-serial>}"
apk="${APK:-$(ls -t "$here"/out/tmls-[0-9]*.apk 2>/dev/null | head -n 1 || true)}"
case "$apk" in /*) ;; ?*) apk="$here/$apk" ;; esac
[ -n "$apk" ] && [ -f "$apk" ] || { echo "no APK - run make.sh (or make.sh debug) first" >&2; exit 1; }
pkg="dev.sagun.tmls"
case "$apk" in *-debug.apk) pkg="dev.sagun.tmls.debug" ;; esac
adb -s "$device" install -r "$apk"
adb -s "$device" shell am start -W -n "$pkg/dev.sagun.tmls.MainActivity" | grep -E "TotalTime|WaitTime" || true
sleep 4
adb -s "$device" exec-out screencap -p > "$here/out/launch.png"
echo "installed $(basename "$apk") ($pkg); screenshot at $here/out/launch.png"
