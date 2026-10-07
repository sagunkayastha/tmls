#!/usr/bin/env bash
# S0 path matrix runner. Usage: paths.sh <quiet|flood>
#   env: EVENTS (10000)  RATE (50)  PATHS ("P1-local P2-ssh-loop ..." subset; default all)
#        NAV (override the tmls nav spec)  RUN_ID (default: UTC timestamp)  PCL (pcl)
# Logs: /data/scratch/s0/runs/<RUN_ID>/  (pcl side: ~/s0-logs/<RUN_ID>/, fetched back with scp)
# Only ever kills the tmux session `s0-recv` (default server or private `-L s0`); never kill-server.
set -euo pipefail

LOAD_ARG=${1:-}
case "$LOAD_ARG" in
  quiet) FLOOD_ARGS=(); LOAD=quiet ;;
  flood) FLOOD_ARGS=(--flood 500000); LOAD=flood-500000 ;;
  *) echo "usage: $0 <quiet|flood>" >&2; exit 2 ;;
esac

EVENTS=${EVENTS:-10000}
RATE=${RATE:-50}
ALL_PATHS="P1-local P2-ssh-loop P2-ssh-pcl P3-tmux-local P3-ssh-tmux-pcl P4-tmls-local P5-tmls-ssh-loop P5-tmls-ssh-pcl"
PATHS=${PATHS:-$ALL_PATHS}
PCL=${PCL:-pcl}
ARCHBOX=${ARCHBOX:-archbox}
SKEW_NOTE="archbox↔pcl both NTP-synced (timedatectl NTPSynchronized=yes); one-way numbers cross-host carry clock skew; ack RTT is single-clock"
TMLS_CMD="cd ~/Others/tmls && uv run -q tmls archbox"

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
S0=$REPO/target/release/s0
export TMPDIR=/data/scratch/s0
RUN_ID=${RUN_ID:-$(date -u +%Y%m%d-%H%M%S)}
OUT=/data/scratch/s0/runs/$RUN_ID
mkdir -p "$TMPDIR" "$OUT"
SUMMARY=$OUT/summary.md
ROWS=$OUT/.rows.md        # report rows (first one carries the header)
FAILED=$OUT/.failed.md    # FAILED rows + notes

[[ -x "$S0" ]] || { echo "missing $S0 — run deploy.sh (or cargo build --release -p s0)" >&2; exit 1; }
for p in $PATHS; do
  [[ " $ALL_PATHS " == *" $p "* ]] || { echo "unknown path id: $p (known: $ALL_PATHS)" >&2; exit 2; }
done

# Quote an argv into one shell-safe string (for ssh remote commands / tmux shell-commands).
q() { printf '%q ' "$@"; }
# Never let a child think it is nested in the operator's tmux (tmls would switch-client the real client).
nontmux() { env -u TMUX -u TMUX_PANE "$@"; }

PCL_S0="" PCL_DIR=""
pcl_init() {
  [[ -n "$PCL_DIR" ]] && return 0
  local home base
  home=$(ssh -o BatchMode=yes "$PCL" 'printf %s "$HOME"')
  base=$(ssh -o BatchMode=yes "$PCL" 'if [ -w /data/scratch/s0 ]; then echo /data/scratch/s0; else echo "$HOME/s0-logs"; fi')
  PCL_S0=$home/.local/bin/s0
  PCL_DIR=$base/runs/$RUN_ID
  ssh -o BatchMode=yes "$PCL" "$(q mkdir -p "$PCL_DIR")"
  ssh -o BatchMode=yes "$PCL" "$(q test -x "$PCL_S0")" || { echo "s0 not deployed on $PCL — run deploy.sh" >&2; return 1; }
}

write_summary() {
  {
    echo "# S0 run $RUN_ID — load $LOAD, EVENTS=$EVENTS RATE=$RATE"
    echo
    echo "host $(uname -n), started $(date -u -d @"$START" +%FT%TZ)"
    echo
    [[ -s "$ROWS" ]] && cat "$ROWS"
    if [[ -s "$FAILED" ]]; then echo; echo "## Failed / notes"; echo; cat "$FAILED"; fi
  } > "$SUMMARY"
}

# send_local <name> [send-extra-args...] -- <child cmd...>
send_local() {
  local name=$1; shift
  local extra=()
  while [[ $1 != -- ]]; do extra+=("$1"); shift; done; shift
  nontmux "$S0" send --log "$OUT/$name.send.jsonl" --events "$EVENTS" --rate "$RATE" \
    --cols 120 --rows 40 "${extra[@]}" -- "$@" 2> "$OUT/$name.send.stderr"
}

recv_argv() { # recv_argv <s0 binary> <log>
  printf '%s\n' "$1" recv --log "$2" --quit-after "$EVENTS" "${FLOOD_ARGS[@]}"
}

# Find how many `j` presses reach the s0-recv row in tmls; sets TMLS_NAV.
TMLS_NAV=""
detect_nav() {
  [[ -n "$TMLS_NAV" ]] && return 0
  if [[ -n "${NAV:-}" ]]; then TMLS_NAV=$NAV; return 0; fi
  # Paths run in subshells: cache the detected nav for the rest of this run.
  if [[ -s "$OUT/tmls-nav.txt" ]]; then TMLS_NAV=$(<"$OUT/tmls-nav.txt"); return 0; fi
  local dump=$OUT/tmls-nav-dump.stderr k
  echo "   detecting tmls row for s0-recv (dump run, times out by design)..."
  nontmux "$S0" send --log "$OUT/tmls-nav-dump.send.jsonl" --events 1 --rate "$RATE" \
    --cols 120 --rows 40 --dump-screen --ready-timeout-ms 3000 --nav 'wait:6000' \
    -- bash -lc "$TMLS_CMD" 2> "$dump" || true
  k=$(awk '
    /│ archbox/ { inside = 1; next }
    / ▸ background/ { inside = 0 }
    inside && /^  [^ ]/ { if (index($0, "s0-recv")) { print n; found = 1; exit } n++ }
    END { if (!found) exit 1 }' "$dump") || {
    echo "!! could not find the s0-recv row in the tmls screen; dump follows. Set NAV=... to override." >&2
    cat "$dump" >&2
    return 1
  }
  # tmls starts with no cursor: the first `j` lands on row 0, so row k needs k+1 presses
  # (tmls app.py SessionList.action_move: cursor None → index 0).
  local keys="" i
  for ((i = 0; i <= k; i++)); do keys+="key:j,"; done
  TMLS_NAV="wait:4000,expect:s0-recv:15000,${keys}key:enter,wait:2000"
  echo "$TMLS_NAV" > "$OUT/tmls-nav.txt"
  echo "   s0-recv is row index $k → NAV=$TMLS_NAV"
}

# Recv inside a default-server tmux session named s0-recv (what tmls lists).
tmls_pre() { # tmls_pre <recv log>
  if nontmux tmux has-session -t '=s0-recv' 2>/dev/null; then
    echo "   stale s0-recv session on default server — killing it (ours)"
    nontmux tmux kill-session -t '=s0-recv'
  fi
  local argv; mapfile -t argv < <(recv_argv "$S0" "$1")
  nontmux tmux new-session -d -s s0-recv -x 200 -y 50 "$(q "${argv[@]}")"
}
tmls_post() { nontmux tmux kill-session -t '=s0-recv' 2>/dev/null || true; }

# Private-socket (-L s0) s0-recv cleanup, local and on pcl. The remote target is explicitly
# single-quoted so a zsh login shell does not `=word`-expand it (printf %q leaves `=s0-recv` bare).
p3_local_post() { nontmux tmux -L s0 kill-session -t '=s0-recv' 2>/dev/null || true; }
p3_pcl_post() {
  ssh -o BatchMode=yes "$PCL" "tmux -L s0 kill-session -t '=s0-recv' 2>/dev/null || true" || true
}

# ---- paths: each sets SEND_LOG / RECV_LOG (local paths) and SKEW (empty or note) ----

path_P1-local() {
  local argv; mapfile -t argv < <(recv_argv "$S0" "$RECV_LOG")
  send_local "$NAME" -- "${argv[@]}"
}

path_P2-ssh-loop() {
  local argv; mapfile -t argv < <(recv_argv "$S0" "$RECV_LOG")
  send_local "$NAME" -- ssh -t "$ARCHBOX" "$(q "${argv[@]}")"
}

path_P2-ssh-pcl() {
  pcl_init; SKEW=$SKEW_NOTE
  local rlog=$PCL_DIR/$NAME.recv.jsonl argv rc=0
  mapfile -t argv < <(recv_argv "$PCL_S0" "$rlog")
  send_local "$NAME" -- ssh -t "$PCL" "$(q "${argv[@]}")" || rc=$?
  scp -q "$PCL:$rlog" "$RECV_LOG"
  return $rc
}

path_P3-tmux-local() {
  trap p3_local_post EXIT   # path runs in its own subshell
  p3_local_post
  local argv; mapfile -t argv < <(recv_argv "$S0" "$RECV_LOG")
  send_local "$NAME" -- tmux -L s0 new-session -s s0-recv "$(q "${argv[@]}")"
}

path_P3-ssh-tmux-pcl() {
  pcl_init; SKEW=$SKEW_NOTE
  local rlog=$PCL_DIR/$NAME.recv.jsonl argv rc=0
  trap p3_pcl_post EXIT   # path runs in its own subshell
  p3_pcl_post
  mapfile -t argv < <(recv_argv "$PCL_S0" "$rlog")
  send_local "$NAME" -- ssh -t "$PCL" "$(q tmux -L s0 new-session -s s0-recv "$(q "${argv[@]}")")" || rc=$?
  scp -q "$PCL:$rlog" "$RECV_LOG"
  return $rc
}

path_P4-tmls-local() {
  trap tmls_post EXIT   # path runs in its own subshell
  tmls_pre "$RECV_LOG"
  detect_nav
  send_local "$NAME" --nav "$TMLS_NAV" -- bash -lc "$TMLS_CMD"
}

path_P5-tmls-ssh-loop() {
  trap tmls_post EXIT   # path runs in its own subshell
  tmls_pre "$RECV_LOG"
  detect_nav
  send_local "$NAME" --nav "$TMLS_NAV" -- ssh -t "$ARCHBOX" "$(q bash -lc "$TMLS_CMD")"
}

path_P5-tmls-ssh-pcl() {
  pcl_init; SKEW=$SKEW_NOTE
  trap tmls_post EXIT   # path runs in its own subshell
  tmls_pre "$RECV_LOG"
  detect_nav
  local slog=$PCL_DIR/$NAME.send.jsonl rc=0
  # Sender runs on pcl: pcl's s0 send → ssh -t archbox → tmls → tmux attach → recv (archbox).
  ssh -o BatchMode=yes "$PCL" "$(q "$PCL_S0" send --log "$slog" --events "$EVENTS" --rate "$RATE" \
      --cols 120 --rows 40 --nav "$TMLS_NAV" -- ssh -t "$ARCHBOX" "$(q bash -lc "$TMLS_CMD")")" \
      2> "$OUT/$NAME.send.stderr" || rc=$?
  scp -q "$PCL:$slog" "$SEND_LOG"
  return $rc
}

# ---- matrix ----

START=$(date +%s)
: > "$ROWS"; : > "$FAILED"
echo "S0 run $RUN_ID  load=$LOAD  EVENTS=$EVENTS RATE=$RATE  → $OUT"
for p in $PATHS; do
  NAME=$p-$LOAD
  SEND_LOG=$OUT/$NAME.send.jsonl
  RECV_LOG=$OUT/$NAME.recv.jsonl
  SKEW=""
  echo "== $p ($LOAD)"
  set +e
  ( set -e; "path_$p" )
  rc=$?
  set -e
  [[ $p == P2-ssh-pcl || $p == P3-ssh-tmux-pcl || $p == P5-tmls-ssh-pcl ]] && SKEW=$SKEW_NOTE
  if [[ $rc -ne 0 ]]; then
    echo "   send/path exited $rc — stderr tail:"; tail -n 15 "$OUT/$NAME.send.stderr" 2>/dev/null | sed 's/^/   | /' || true
    echo "- $p ($LOAD): path exited $rc; see $OUT/$NAME.send.stderr" >> "$FAILED"
  fi
  if [[ -s "$SEND_LOG" && -s "$RECV_LOG" ]]; then
    hdr=(); [[ -s "$ROWS" ]] || hdr=(--header)
    skew=(); [[ -n "$SKEW" ]] && skew=(--skew-note "$SKEW")
    if row=$("$S0" report --send "$SEND_LOG" --recv "$RECV_LOG" --path "$p" --load "$LOAD" \
          --events "$EVENTS" --json "$OUT/$NAME.report.json" "${hdr[@]}" "${skew[@]}" 2> "$OUT/$NAME.report.stderr"); then
      echo "$row" >> "$ROWS"; echo "$row" | tail -n 1
    else
      echo "| $p | $LOAD | FAILED (report error; see $OUT/$NAME.report.stderr) |" >> "$FAILED"
    fi
  else
    echo "| $p | $LOAD | FAILED (no send/recv log; see $OUT/$NAME.send.stderr) |" >> "$FAILED"
    echo "   FAILED: missing send or recv log"
  fi
  write_summary
done
write_summary
echo "summary: $SUMMARY"
