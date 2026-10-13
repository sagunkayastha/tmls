#!/usr/bin/env bash
# Checks (or, with arguments, runs any cargo command on) the desktop app for Windows, cross from
# Linux, in the tmls-rs-win-probe image (tooling/win/Dockerfile: the build image + mingw-w64 +
# the x86_64-pc-windows-gnu target). Nothing is installed on the host or on the Windows machine.
#   tooling/win/check.sh                      # cargo check -p desktop, errors in short form
#   tooling/win/check.sh build --release -p desktop --target x86_64-pc-windows-gnu
# The target directory is WIN_TARGET (default /data/scratch/tmls-rs-v0/win/target).
set -euo pipefail
cd "$(dirname "$0")/../.."
TARGET_DIR=${WIN_TARGET:-/data/scratch/tmls-rs-v0/win/target}
mkdir -p "$TARGET_DIR"
if [ $# -eq 0 ]; then
  set -- check --target x86_64-pc-windows-gnu -p desktop --message-format short
fi
exec docker run --rm -v "$PWD":/work:ro -v tmls-rs-cargo:/cargo -v "$TARGET_DIR":/t \
  -e CARGO_TARGET_DIR=/t -w /work tmls-rs-win-probe:latest cargo "$@"
