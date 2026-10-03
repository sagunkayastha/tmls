#!/usr/bin/env bash
# Runs inside tmls-android-build (see make.sh). Gradle tests, lints and packages; the release
# APK is then aligned and signed with the long-lived key mounted read-only at /keys.
set -euo pipefail
mode="${1:-release}"
cd /w
mkdir -p out "$TMPDIR"
gradle=(gradle --no-daemon --console=plain --project-cache-dir /scratch/project-cache -PtmlsBuildDir="$TMLS_BUILD_DIR")
if [ "$mode" = test ]; then
  "${gradle[@]}" -PtmlsVersionCode=1 -PtmlsVersionName=dev testDebugUnitTest lintDebug
  exit 0
fi
if [ "$mode" = profile ]; then
  "${gradle[@]}" -PtmlsVersionCode=1 -PtmlsVersionName=dev assembleProfile
  cp "$TMLS_BUILD_DIR/app/outputs/apk/profile/app-profile.apk" out/tmls-debug.apk
  exit 0
fi
if [ "$mode" = debug ]; then
  "${gradle[@]}" -PtmlsVersionCode=1 -PtmlsVersionName=dev assembleDebug
  cp "$TMLS_BUILD_DIR/app/outputs/apk/debug/app-debug.apk" out/tmls-debug.apk
  exit 0
fi
: "${VERSION_CODE:?}" "${VERSION_NAME:?}"
"${gradle[@]}" -PtmlsVersionCode="$VERSION_CODE" -PtmlsVersionName="$VERSION_NAME" \
  testReleaseUnitTest lintRelease assembleRelease
unsigned="$TMLS_BUILD_DIR/app/outputs/apk/release/app-release-unsigned.apk"
aligned="$TMLS_BUILD_DIR/tmls-aligned.apk"
zipalign -f -p 4 "$unsigned" "$aligned"
out="out/tmls-$VERSION_CODE.apk"
apksigner sign --ks /keys/tmls.keystore --ks-pass file:/keys/keystore.pw --ks-key-alias tmls \
  --v4-signing-enabled false --out "$out" "$aligned"
apksigner verify "$out"
