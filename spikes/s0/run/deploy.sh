#!/usr/bin/env bash
# Build s0 on archbox and deploy it to the PC (pcl) at ~/.local/bin/s0.
# Creates directories only; never deletes anything on either host.
set -euo pipefail

PCL=${PCL:-pcl}
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
export TMPDIR=/data/scratch/s0
mkdir -p "$TMPDIR"

echo "== build (archbox)"
(cd "$REPO" && cargo build --release -p s0)
BIN=$REPO/target/release/s0

echo "== deploy to $PCL:~/.local/bin/s0"
ssh -o BatchMode=yes "$PCL" 'mkdir -p "$HOME/.local/bin"'
scp -q "$BIN" "$PCL:.local/bin/s0"
ssh -o BatchMode=yes "$PCL" 'chmod 755 "$HOME/.local/bin/s0"'

echo "== log dir on $PCL"
# pcl normally has no /data: fall back to ~/s0-logs (paths.sh uses the same rule).
LOGDIR=$(ssh -o BatchMode=yes "$PCL" \
  'if mkdir -p /data/scratch/s0 2>/dev/null && [ -w /data/scratch/s0 ]; then echo /data/scratch/s0; else mkdir -p "$HOME/s0-logs" && echo "$HOME/s0-logs"; fi')
echo "pcl log dir: $LOGDIR"

echo "== verify"
local_sum=$(sha256sum "$BIN" | cut -d' ' -f1)
remote_sum=$(ssh -o BatchMode=yes "$PCL" 'sha256sum "$HOME/.local/bin/s0"' | cut -d' ' -f1)
[[ "$local_sum" == "$remote_sum" ]] || { echo "checksum mismatch: $local_sum vs $remote_sum" >&2; exit 1; }
ssh -o BatchMode=yes "$PCL" '"$HOME/.local/bin/s0" --help' | head -n 1
echo "deploy OK (sha256 $local_sum)"
