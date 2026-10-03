#!/usr/bin/env bash
# Install an APK on the phone over USB adb (the host's adb), launch it, take a screenshot.
#   android/install.sh                         newest out/tmls-<code>.apk (release)
#   APK=out/tmls-debug.apk android/install.sh  the debug build (dev.sagun.tmls.debug)
# Always -s: other adb devices (a TV box, say) may be connected too.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# The phone's adb serial (`adb devices`): DEVICE=, else ~/.config/tmls-android/device.
device="${DEVICE:-$(cat "${TMLS_ANDROID_KEYS:-$HOME/.config/tmls-android}/device" 2>/dev/null || true)}"
[ -n "$device" ] || { echo "set DEVICE=<serial> (adb devices) or write it to ~/.config/tmls-android/device" >&2; exit 1; }
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
