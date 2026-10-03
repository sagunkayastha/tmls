#!/usr/bin/env bash
# Build the tmls app: unit tests, lint, release APK signed with the long-lived key, published
# where tmls-web serves it to the app's updater.
#   android/make.sh          -> android/out/tmls-<versionCode>.apk, then $TMLS_APK_DIR/{tmls.apk,latest.json}
#   android/make.sh debug    -> android/out/tmls-debug.apk (dev.sagun.tmls.debug, debug key)
#   android/make.sh profile  -> the same file and package, built like a release (R8, not debuggable),
#                               for measuring smoothness on the phone
# Everything runs in the tmls-android-build image (in-docker.sh); nothing is installed on the host.
# The signing key lives outside the repo and must never change, or Android refuses the upgrade
# and the app has to be uninstalled first.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
keys="${TMLS_ANDROID_KEYS:-$HOME/.config/tmls-android}"
publish="${TMLS_APK_DIR:-$HOME/stacks/tmls/apk}"
mode="${1:-release}"
mkdir -p "$keys" "$here/out" && chmod 700 "$keys"
# Default addresses built into the app (no Setup screen): ~/.config/tmls-android/servers, home on
# the first line, away (optional) on the second. Local only: the repo is public.
if [ -f "$keys/servers" ]; then
  export TMLS_HOME_URL="$(sed -n 1p "$keys/servers")" TMLS_AWAY_URL="$(sed -n 2p "$keys/servers")"
fi

if [ "$mode" = debug ] || [ "$mode" = profile ]; then
  "$here/in-docker.sh" bash /w/build.sh "$mode"
  echo "built $here/out/tmls-debug.apk"
  exit 0
fi

if [ ! -f "$keys/tmls.keystore" ]; then
  (umask 077 && head -c 24 /dev/urandom | base64 | tr -d '/+=' > "$keys/keystore.pw")
  TMLS_MOUNT_KEYS=rw "$here/in-docker.sh" keytool -genkeypair -keystore /keys/tmls.keystore \
    -storepass:file /keys/keystore.pw -keypass:file /keys/keystore.pw -alias tmls \
    -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=tmls"
fi
# Version code = max(last + 1, minutes since 2024-01-01 UTC): strictly increasing even for two
# builds in one minute or a clock that went backwards. The last one is kept beside the key.
last_file="$keys/last-version-code"
[ -f "$last_file" ] || (umask 077 && echo 0 > "$last_file")
last="$(cat "$last_file")"
case "$last" in ''|*[!0-9]*) echo "$last_file does not hold a number" >&2; exit 1 ;; esac
minutes=$(( ($(date -u +%s) - 1704067200) / 60 ))
code=$(( minutes > last ? minutes : last + 1 ))
name="1.$(date -u +%y%m%d).$code"
TMLS_MOUNT_KEYS=ro VERSION_CODE="$code" VERSION_NAME="$name" "$here/in-docker.sh" bash /w/build.sh release
echo "$code" > "$last_file"
apk="$here/out/tmls-$code.apk"
echo "built $apk"

# The manifest the in-app updater reads (/app/latest.json), renamed into place after the APK so
# it never describes a build that is not there yet.
mkdir -p "$publish"
cp "$apk" "$publish/tmls.apk.tmp"
printf '{"versionCode":%s,"versionName":"%s","size":%s,"sha256":"%s"}\n' \
  "$code" "$name" "$(stat -c %s "$apk")" "$(sha256sum "$apk" | cut -d' ' -f1)" > "$publish/latest.json.tmp"
mv -f "$publish/tmls.apk.tmp" "$publish/tmls.apk" && mv -f "$publish/latest.json.tmp" "$publish/latest.json"
echo "published $name -> $publish (served at /app/tmls.apk)"
