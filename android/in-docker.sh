#!/usr/bin/env bash
# Run one command inside the tmls-android-build image (JDK 17 + Android SDK + Gradle). Nothing
# is installed on the host: Gradle's downloads live in the Docker volume tmls-android-gradle,
# the debug signing key, build output and temp files under $TMLS_ANDROID_SCRATCH (default /data/scratch/tmls-android).
#   android/in-docker.sh gradle --console=plain tasks
# TMLS_MOUNT_KEYS=ro|rw mounts ~/.config/tmls-android at /keys (make.sh only).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
scratch="${TMLS_ANDROID_SCRATCH:-/data/scratch/tmls-android}"
keys="${TMLS_ANDROID_KEYS:-$HOME/.config/tmls-android}"
mkdir -p "$scratch/home" "$scratch/tmp" "$scratch/build" "$scratch/project-cache" "$here/out"
docker build -q -t tmls-android-build - < "$here/Dockerfile.build" > /dev/null
docker volume create tmls-android-gradle > /dev/null
extra=()
case "${TMLS_MOUNT_KEYS:-}" in
  ro|rw) extra+=(-v "$keys:/keys:$TMLS_MOUNT_KEYS") ;;
  "") ;;
  *) echo "TMLS_MOUNT_KEYS must be ro or rw" >&2; exit 1 ;;
esac
exec docker run --rm -u "$(id -u):$(id -g)" \
  -e HOME=/scratch/home -e TMPDIR=/scratch/tmp -e GRADLE_USER_HOME=/gradle \
  -e ANDROID_USER_HOME=/scratch/home/.android \
  -e TMLS_BUILD_DIR=/scratch/build -e JAVA_TOOL_OPTIONS="-Djava.io.tmpdir=/scratch/tmp -Duser.home=/scratch/home" \
  -e VERSION_CODE -e VERSION_NAME -e TMLS_HOME_URL -e TMLS_AWAY_URL \
  -v "$here:/w" -v "$scratch:/scratch" -v tmls-android-gradle:/gradle "${extra[@]}" \
  -w /w tmls-android-build "$@"
