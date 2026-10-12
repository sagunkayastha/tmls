#!/usr/bin/env bash
# Runs bench.sh once in the smoke container, with a binary the caller built.
#
#   SMOKE_BIN=/path/to/tmls-rs SMOKE_OUT=/data/scratch/.../head-text-1 BENCH_STATE=text \
#     tooling/smoke/bench-run.sh
#
# Environment:
#   SMOKE_BIN       the binary to measure (required; mounted read-only)
#   SMOKE_OUT       where the latency log, marks.json and the screenshots go (required)
#   BENCH_STATE     closed | text | image | list (default closed)
#   BENCH_WLDEBUG   1: count the app's surface commits while idle (see bench.sh)
#   BENCH_KEY_MS    pause between keys (default 60)
#   BENCH_IMAGE_SIDE  side of the picture of the state image, in pixels (default 4096)
#   SMOKE_TIMEOUT   seconds the run may take in the container (default 240)
#
# The image tmls-rs-smoke:latest must exist (tooling/smoke/run.sh builds it). The repo is
# mounted read-only for bench.sh and vptr.py; the container has no network; HOME, the tmux
# socket and the Wayland socket live on a tmpfs inside it.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
IMAGE=tmls-rs-smoke:latest
: "${SMOKE_BIN:?bench-run.sh: SMOKE_BIN is the binary to measure}"
: "${SMOKE_OUT:?bench-run.sh: SMOKE_OUT is where the results go}"
[ -x "$SMOKE_BIN" ] || { echo "bench-run.sh: $SMOKE_BIN is no executable" >&2; exit 2; }
docker image inspect "$IMAGE" >/dev/null 2>&1 || { echo "bench-run.sh: no image $IMAGE (tooling/smoke/run.sh builds it)" >&2; exit 2; }
BIN=$(readlink -f "$SMOKE_BIN")

mkdir -p "$SMOKE_OUT"
exec docker run --rm --network none --hostname smoke \
  -v "$ROOT":/work:ro \
  -v "$BIN":/bench-bin/tmls-rs:ro \
  -v "$SMOKE_OUT":/out \
  --tmpfs /scratch:mode=1777 \
  -e SMOKE_OUT=/out -e SMOKE_SCRATCH=/scratch -e SMOKE_BIN=/bench-bin/tmls-rs \
  -e BENCH_STATE="${BENCH_STATE:-closed}" -e BENCH_WLDEBUG="${BENCH_WLDEBUG:-0}" \
  -e BENCH_KEY_MS="${BENCH_KEY_MS:-60}" \
  -e BENCH_IMAGE_SIDE="${BENCH_IMAGE_SIDE:-4096}" \
  -w /work \
  "$IMAGE" timeout --kill-after=10 "${SMOKE_TIMEOUT:-240}" bash /work/tooling/smoke/bench.sh
