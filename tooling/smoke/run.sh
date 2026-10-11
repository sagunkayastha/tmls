#!/usr/bin/env bash
# Builds the smoke image and the release binary, then runs e2e.sh in the container.
#
#   tooling/smoke/run.sh
#
# Environment:
#   SMOKE_OUT        where screenshots and logs go (default /data/scratch/tmls-rs-v0/e2e)
#   SMOKE_NO_BUILD   set to 1 to use target-docker/release/tmls-rs as it is
#   SMOKE_TIMEOUT    seconds the whole run may take in the container (default 300)
#
# The repo is mounted read-only; the container has no network; HOME, the tmux socket and the
# Wayland socket live on a tmpfs inside the container, so nothing of the host's is seen.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${SMOKE_OUT:-/data/scratch/tmls-rs-v0/e2e}
IMAGE=tmls-rs-smoke:latest
cd "$ROOT"

docker build -t "$IMAGE" -f tooling/smoke/Dockerfile.smoke tooling/smoke
if [ "${SMOKE_NO_BUILD:-0}" != 1 ]; then
  tooling/docker/build.sh build --release -p desktop
fi
[ -x target-docker/release/tmls-rs ] || { echo "run.sh: target-docker/release/tmls-rs is missing" >&2; exit 2; }

mkdir -p "$OUT"
# A short host name: the list shows HOST:SESSION in 28 cells.
exec docker run --rm --network none --hostname smoke \
  -v "$ROOT":/work:ro \
  -v "$OUT":/out \
  --tmpfs /scratch:mode=1777 \
  -e SMOKE_OUT=/out -e SMOKE_SCRATCH=/scratch \
  -w /work \
  "$IMAGE" timeout --kill-after=10 "${SMOKE_TIMEOUT:-300}" bash /work/tooling/smoke/e2e.sh
