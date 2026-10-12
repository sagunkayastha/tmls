#!/usr/bin/env bash
# Builds the smoke image and the release binary, then runs e2e.sh in the container.
#
#   tooling/smoke/run.sh
#
# Environment:
#   SMOKE_OUT        where screenshots and logs go (default /data/scratch/tmls-rs-v0/e2e)
#   SMOKE_NO_BUILD   set to 1 to use target-docker/release/tmls-rs as it is
#   SMOKE_TIMEOUT    seconds the whole run may take in the container (default 1200: the third
#                    run and the round-2 blocks add about two minutes)
#   SMOKE_SCALE      the headless output's scale (default 1). The checks' pixel positions and
#                    sizes are for scale 1: at another scale run with SMOKE_ONLY=measure
#   SMOKE_ONLY       `measure`: start, attach, open the text panel, take `measure-panel.png`
#                    (for tooling/smoke/rows.py) and stop
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
# The wl-clipboard fallback's own test (crates/desktop/tests/clip_wayland.rs): e2e.sh runs it
# against the compositor. Its binary has a hash in its name: cargo says which.
if [ "${SMOKE_NO_BUILD:-0}" != 1 ]; then
  CLIP_TEST=$(tooling/docker/build.sh test -p desktop --test clip_wayland --no-run 2>&1 |
    sed -n 's/.*Executable .*(\(target-docker\/[^)]*\)).*/\1/p' | tail -1)
else
  CLIP_TEST=$(ls -t target-docker/debug/deps/clip_wayland-* 2>/dev/null | grep -v '\.d$' | head -1 || true)
fi

mkdir -p "$OUT"
# A short host name: the list shows HOST:SESSION in 28 cells.
exec docker run --rm --network none --hostname smoke \
  -v "$ROOT":/work:ro \
  -v "$OUT":/out \
  --tmpfs /scratch:mode=1777 \
  -e SMOKE_OUT=/out -e SMOKE_SCRATCH=/scratch -e SMOKE_OUTSIDE="${SMOKE_OUTSIDE:-}" \
  -e SMOKE_SCALE="${SMOKE_SCALE:-1}" -e SMOKE_ONLY="${SMOKE_ONLY:-}" \
  -e SMOKE_CLIP_TEST="${CLIP_TEST:+/work/$CLIP_TEST}" \
  -w /work \
  "$IMAGE" timeout --kill-after=10 "${SMOKE_TIMEOUT:-1200}" bash /work/tooling/smoke/e2e.sh
