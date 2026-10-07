#!/usr/bin/env bash
# K1: real keyboard events injected by ydotool into a FOCUSED kitty window running a bare `s0 recv --plain`.
# Run this on pcl from a SECOND terminal / ssh session (not the kitty that runs the receiver).
#   env: EVENTS (2000)  KEY_DELAY_MS (20)  K1_ID (UTC timestamp)  YDOTOOL_SOCKET  K1_TIMEOUT_S (wait for recv log; 600)
# Measures keyboard -> kitty -> PTY -> recv: loss/corruption/dup/reorder only (no latency: ydotool has no timestamps).
# Do not touch the keyboard or mouse during the run: ydotool types into whatever has focus.
set -euo pipefail
unset TMUX TMUX_PANE

N=${EVENTS:-2000}
KEY_DELAY_MS=${KEY_DELAY_MS:-20}
ID=${K1_ID:-$(date -u +%Y%m%d-%H%M%S)}
S0=${S0:-$HOME/.local/bin/s0}
LOG=$HOME/s0-logs/runs/K1-$ID.recv.jsonl
mkdir -p "$(dirname "$LOG")"
[[ -x "$S0" ]] || { echo "missing $S0" >&2; exit 1; }

# ydotoold must be running (socket reachable).
if [[ -z "${YDOTOOL_SOCKET:-}" ]]; then
  for s in "/run/user/$(id -u)/.ydotool_socket" /tmp/.ydotool_socket; do [[ -S $s ]] && { export YDOTOOL_SOCKET=$s; break; }; done
fi
if ! ydotool debug >/dev/null 2>&1; then
  cat >&2 <<MSG
ydotoold is not running (no reachable socket: ${YDOTOOL_SOCKET:-/run/user/$(id -u)/.ydotool_socket}). Start it with ONE of:
  1) systemctl --user start ydotool        # user unit /usr/lib/systemd/user/ydotool.service (there is no system unit)
     (system-wide variant, if you have one:  sudo systemctl start ydotool)
  2) ydotoold --socket-path=/run/user/$(id -u)/.ydotool_socket --socket-perm=0600 &
     export YDOTOOL_SOCKET=/run/user/$(id -u)/.ydotool_socket
     (only works if /dev/uinput is writable by you)
then re-run this script.
MSG
  exit 1
fi

CHARS=$(( N * 8 ))   # 8 chars per event: 1 letter + '#' + 6 digits
EST=$(( CHARS * (KEY_DELAY_MS + 20) / 1000 ))
cat <<MSG
K1 run $ID   events=$N   key-delay=${KEY_DELAY_MS}ms   (~${CHARS} keys, est. ~${EST}s)

1) In the kitty window (focused, normal size), paste this receiver command and press Enter:

   $S0 recv --plain --log $LOG --quit-after $N --ack false

2) Then DO NOT touch keyboard/mouse. This script starts typing 3 s after the log appears.
MSG

# Wait for the receiver to create its log.
t0=$SECONDS
until [[ -e $LOG ]]; do
  (( SECONDS - t0 < ${K1_TIMEOUT_S:-600} )) || { echo "timed out waiting for $LOG" >&2; exit 1; }
  sleep 0.2
done
for i in 3 2 1; do echo "typing in $i ..."; sleep 1; done

echo "typing $N events ..."
"$S0" keys --plain --events "$N" | ydotool type --key-delay "$KEY_DELAY_MS" --file -
echo "typed; waiting for the receiver's done line ..."

t0=$SECONDS
until grep -q '"done"' "$LOG" 2>/dev/null; do
  (( SECONDS - t0 < 30 + EST / 2 )) || { echo "no done line in $LOG after typing — report below will say NO (loss)" >&2; break; }
  sleep 0.5
done

echo
"$S0" report --recv-only --header --events "$N" --path K1-kitty-ydotool --load quiet --recv "$LOG" --json "${LOG%.recv.jsonl}.report.json"
echo
echo "recv log: $LOG"
