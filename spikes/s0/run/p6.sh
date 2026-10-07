#!/usr/bin/env bash
# P6: native kitty paths. Run this INSIDE a kitty window on pcl (automates README "Morning run: native kitty paths (P6)").
#   P6-kitty-local  kitty PTY -> s0 send -> PTY -> s0 recv, all on pcl
#   P6-kitty-ssh    ... -> ssh -t archbox -> s0 recv
#   P6-kitty-tmls   ... -> ssh -t archbox -> tmls -> tmux attach -> s0 recv (session s0-recv on archbox)
# env: EVENTS (10000)  P6_PATHS ("local ssh tmls", subset filter)  NAV (override tmls nav spec)
#      RATE (50)  ARCHBOX (archbox)
# Touches no tmux session except `s0-recv` on archbox (killed exactly, before and after).
# Logs: pcl ~/s0-logs/runs/<R>/ ; archbox /data/scratch/s0/runs/P6-<R>/ (+ recv logs P6-<R>-kitty-*.recv.jsonl)
set -euo pipefail
unset TMUX TMUX_PANE   # children must never think they are nested in the operator's tmux

N=${EVENTS:-10000}
RATE=${RATE:-50}
P6_PATHS=${P6_PATHS:-"local ssh tmls"}
ARCHBOX=${ARCHBOX:-archbox}
S0=${S0:-$HOME/.local/bin/s0}
A_S0=/home/sgkayast/Others/tmls-rs/target/release/s0
A_RUNS=/data/scratch/s0/runs
TMLS_CMD="cd ~/Others/tmls && uv run -q tmls archbox"

[[ -x "$S0" ]] || { echo "missing $S0" >&2; exit 1; }
for p in $P6_PATHS; do
  [[ $p == local || $p == ssh || $p == tmls ]] || { echo "unknown P6_PATHS entry: $p (local ssh tmls)" >&2; exit 2; }
done
[[ -n "${KITTY_WINDOW_ID:-}" ]] || echo "!! warning: not inside kitty (KITTY_WINDOW_ID unset) — these would not be P6 numbers" >&2

R=$(date -u +%Y%m%d-%H%M%S)
D=$HOME/s0-logs/runs/$R
O=$A_RUNS/P6-$R
mkdir -p "$D"
T=$(head -c8 /dev/urandom | od -An -tx1 | tr -d ' \n')
q() { printf '%q ' "$@"; }
want() { [[ " $P6_PATHS " == *" $1 "* ]]; }
ran=()   # paths that produced a send log
echo "P6 run id: $R   N=$N  token=$T  paths=[$P6_PATHS]  logs: $D"

# Kill ONLY the s0-recv tmux session on archbox (exact match). Never kill-server, never other sessions.
kill_recv() { ssh -o BatchMode=yes "$ARCHBOX" "tmux kill-session -t '=s0-recv' 2>/dev/null || true" || true; }
trap 'kill_recv' EXIT
ssh -o BatchMode=yes "$ARCHBOX" "$(q mkdir -p "$O" "$A_RUNS")"

if want local; then
  echo "== P6-kitty-local"
  rc=0
  "$S0" send --log "$D/P6-kitty-local.send.jsonl" --events "$N" --rate "$RATE" -- \
    "$S0" recv --log "$D/P6-kitty-local.recv.jsonl" --quit-after "$N" || rc=$?
  echo "   exit $rc"; ran+=(local)
fi

if want ssh; then
  echo "== P6-kitty-ssh"
  rc=0
  "$S0" send --log "$D/P6-kitty-ssh.send.jsonl" --events "$N" --rate "$RATE" -- \
    ssh -t "$ARCHBOX" "$A_S0" recv --log "$A_RUNS/P6-$R-kitty-ssh.recv.jsonl" --quit-after "$N" || rc=$?
  echo "   exit $rc"; ran+=(ssh)
fi

if want tmls; then
  echo "== P6-kitty-tmls"
  kill_recv   # a stale s0-recv is ours; nothing else is touched
  ssh -o BatchMode=yes "$ARCHBOX" "tmux new-session -d -s s0-recv -x 200 -y 50 '$A_S0 recv --log $A_RUNS/P6-$R-kitty-tmls.recv.jsonl --quit-after $N --ready-token $T'"
  if [[ -n "${NAV:-}" ]]; then
    TNAV=$NAV
  else
    echo "   detecting tmls row for s0-recv (dump run, times out by design)..."
    DUMP=$D/nav-dump.stderr
    "$S0" send --log "$D/nav-dump.send.jsonl" --events 1 --rate "$RATE" --ready-token "$T" --cols 120 --rows 40 \
      --dump-screen --ready-timeout-ms 3000 --nav 'wait:6000' -- \
      ssh -t "$ARCHBOX" "bash -lc 'cd ~/Others/tmls && uv run -q tmls archbox'" 2> "$DUMP" || true
    # Same heuristic as detect_nav in paths.sh: rows between `│ archbox` and ` ▸ background` that start
    # with two spaces + text; k = 0-based index of the s0-recv row.
    k=$(awk '
      /│ archbox/ { inside = 1; next }
      / ▸ background/ { inside = 0 }
      inside && /^  [^ ]/ { if (index($0, "s0-recv")) { print n; found = 1; exit } n++ }
      END { if (!found) exit 1 }' "$DUMP") || { echo "!! could not find the s0-recv row; dump follows. Set NAV=... to override." >&2; cat "$DUMP" >&2; exit 1; }
    K=$((k + 1))   # tmls starts with no cursor: row index k needs k+1 presses of j
    J=""; for ((i = 0; i < K; i++)); do J+="key:j,"; done
    TNAV="wait:4000,expect:s0-recv:15000,${J}key:enter,wait:2000"
    echo "   s0-recv is row index $k → K=$K"
  fi
  echo "   NAV=$TNAV"; echo "$TNAV" > "$D/tmls-nav.txt"
  rc=0
  "$S0" send --log "$D/P6-kitty-tmls.send.jsonl" --events "$N" --rate "$RATE" --ready-token "$T" --cols 120 --rows 40 \
    --nav "$TNAV" -- ssh -t "$ARCHBOX" "bash -lc 'cd ~/Others/tmls && uv run -q tmls archbox'" || rc=$?
  echo "   exit $rc"; ran+=(tmls)
  kill_recv
fi

echo "== report (on $ARCHBOX)"
scp -q "$D"/* "$ARCHBOX:$O/"
SKEW="pcl-to-archbox cross-host one-way"
hdr=(--header); rows=()
for p in "${ran[@]}"; do
  case $p in
    local) rl=$O/P6-kitty-local.recv.jsonl; skew=() ;;
    ssh)   rl=$A_RUNS/P6-$R-kitty-ssh.recv.jsonl; skew=(--skew-note "$SKEW") ;;
    tmls)  rl=$A_RUNS/P6-$R-kitty-tmls.recv.jsonl; skew=(--skew-note "$SKEW") ;;
  esac
  row=$(ssh -o BatchMode=yes "$ARCHBOX" "$(q "$A_S0" report "${hdr[@]}" --events "$N" --path "P6-kitty-$p" --load quiet \
        --send "$O/P6-kitty-$p.send.jsonl" --recv "$rl" --json "$O/P6-kitty-$p.report.json" "${skew[@]}")") \
    || row="| P6-kitty-$p | quiet | FAILED (report error) |"
  rows+=("$row"); hdr=()
done
{ echo "# P6 run $R (N=$N, token $T)"; echo; printf '%s\n' "${rows[@]}"; } | tee "$D/summary.md"
ssh -o BatchMode=yes "$ARCHBOX" "cat > $(q "$O/summary.md")" < "$D/summary.md" || true
echo
echo "run id: $R   (pcl logs $D ; archbox $O)"
