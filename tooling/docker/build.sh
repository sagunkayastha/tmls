#!/usr/bin/env bash
# Run cargo (or a shell) inside the tmls-rs-build image. Usage:
#   tooling/docker/build.sh <cargo args...>
#   tooling/docker/build.sh --shell
set -euo pipefail
IMAGE=tmls-rs-build:latest
TTY=()
[ -t 0 ] && [ -t 1 ] && TTY=(-it)
COMMON=(--rm "${TTY[@]}" -v "$PWD":/work -v tmls-rs-cargo:/cargo -w /work -e CARGO_TARGET_DIR=/work/target-docker)
if [ "${1:-}" = "--shell" ]; then
  exec docker run "${COMMON[@]}" "$IMAGE" bash
fi
exec docker run "${COMMON[@]}" "$IMAGE" cargo "$@"
