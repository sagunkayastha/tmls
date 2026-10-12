#!/usr/bin/env bash
# The drop check: files dragged from another window and dropped on tmls-rs (crates/desktop/src/dnd.rs).
#
# On the host (builds the app and the drag source in the build image, then runs the rest in the
# tmls-rs-smoke container; DND_OUT is where the log and the screenshots go, default
# /data/scratch/tmls-rs-v0/dnd; DND_NO_BUILD=1 uses the binaries as they are; DND_WAYLAND_DEBUG=1
# logs the app's Wayland messages to app.stderr):
#
#   tooling/smoke/dnd.sh
#
# In the container: a headless sway, a tmux session attached in the app, and a second window
# (`examples/dragsrc.rs`) that starts a drag when a button goes down on it. The virtual pointer
# of vptr.py presses there, moves over the app and lets go. Three drags:
#   1. a `text/uri-list` of one file   → its path is typed into the session, the source is told
#      `finished`
#   2. a `text/uri-list` of two files  → both paths, quoted where a shell needs it
#   3. `text/plain`                    → refused: the source is told `cancelled`, nothing is typed
# After each the app must still run (a protocol error on the shared connection would end it)
# and must not have said `drops are off`.
set -u

if [ ! -f /.dockerenv ]; then
  set -e
  ROOT=$(cd "$(dirname "$0")/../.." && pwd)
  OUT=${DND_OUT:-/data/scratch/tmls-rs-v0/dnd}
  cd "$ROOT"
  if [ "${DND_NO_BUILD:-0}" != 1 ]; then
    tooling/docker/build.sh build --release -p desktop
    tooling/docker/build.sh build --release -p desktop --example dragsrc
  fi
  mkdir -p "$OUT"
  exec docker run --rm --network none --hostname smoke \
    -v "$ROOT":/work:ro -v "$OUT":/out --tmpfs /scratch:mode=1777 \
    -e DND_WAYLAND_DEBUG="${DND_WAYLAND_DEBUG:-}" \
    -w /work tmls-rs-smoke:latest timeout --kill-after=10 "${DND_TIMEOUT:-180}" bash /work/tooling/smoke/dnd.sh
fi

OUT=/out
SCRATCH=/scratch
BIN=/work/target-docker/release/tmls-rs
SRC=/work/target-docker/release/examples/dragsrc
ONE='tmlsrs-dnd'
export HOME="$SCRATCH/home" TMUX_TMPDIR="$SCRATCH/tmux" XDG_RUNTIME_DIR="$SCRATCH/xdg"
export XDG_CONFIG_HOME="$HOME/.config" SHELL=/bin/bash LANG=C.UTF-8 LC_ALL=C.UTF-8 TERM=xterm-256color
unset TMUX DISPLAY WAYLAND_DISPLAY SSH_AUTH_SOCK
mkdir -p "$HOME/.config/tmls" "$TMUX_TMPDIR" "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR" "$TMUX_TMPDIR"
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 RUST_BACKTRACE=1

N=0
FAILED=0
pass() { N=$((N + 1)); printf 'PASS %2d  %s\n' "$N" "$1"; }
fail() { N=$((N + 1)); FAILED=$((FAILED + 1)); printf 'FAIL %2d  %s\n         %s\n' "$N" "$1" "${2:-}"; }
wait_for() {
  local deadline=$((SECONDS + $1))
  shift
  until "$@" >/dev/null 2>&1; do
    [ "$SECONDS" -ge "$deadline" ] && return 1
    sleep 0.2
  done
}
pane_text() { tmux capture-pane -p -J -t "=$ONE:" 2>/dev/null; }
pane_has() { pane_text | grep -qF -- "$1"; }
app_alive() { kill -0 "$APP_PID" 2>/dev/null; }
drops_on() { ! grep -q 'drops are off\|no drag and drop' "$OUT/app.stderr"; }

cat >"$SCRATCH/sway.conf" <<EOF
output HEADLESS-1 resolution 1280x800 scale 1
default_border none
default_floating_border none
focus_follows_mouse no
EOF
sway -c "$SCRATCH/sway.conf" >"$OUT/sway.log" 2>&1 &
wait_for 15 bash -c 'ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock && ls "$XDG_RUNTIME_DIR"/wayland-?' || {
  echo "dnd.sh: no compositor (see $OUT/sway.log)" >&2
  exit 1
}
SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock | head -1)
WAYLAND_DISPLAY=$(basename "$(ls -d "$XDG_RUNTIME_DIR"/wayland-? | head -1)")
export SWAYSOCK WAYLAND_DISPLAY

tmux new-session -d -s "$ONE" -x 100 -y 30
: >"$OUT/app.stderr"
# DND_WAYLAND_DEBUG=1: every Wayland message of the app goes to app.stderr.
TMLS_RS_FOREGROUND=1 WAYLAND_DEBUG="${DND_WAYLAND_DEBUG:-}" "$BIN" 2>"$OUT/app.stderr" &
APP_PID=$!
wait_for 30 bash -c 'swaymsg -s "$SWAYSOCK" -t get_tree | grep -q "\"tmls-rs\""' &&
  pass "the window opened" || fail "the window opened" "$(tail -3 "$OUT/app.stderr" | tr '\n' ' ')"
sleep 2
timeout 10 wtype -s 250 -d 30 -k Return
wait_for 8 bash -c 'tmux list-clients -F "#{session_name}" | grep -qxF -- "$0"' "$ONE" &&
  pass "Enter attaches the session" || fail "Enter attaches the session"
drops_on && pass "the drop listener started" || fail "the drop listener started" "$(cat "$OUT/app.stderr" | tr '\n' ' ')"

# The files the drags name. They need not be images: a session of this machine gets the path.
mkdir -p "$HOME/drop"
: >"$HOME/drop/one pic.png"
: >"$HOME/drop/two.txt"

# The rectangle of a window by its app id, as "x y width height".
rect_of() {
  swaymsg -s "$SWAYSOCK" -t get_tree | python3 -c '
import json, sys
def walk(node):
    if node.get("app_id") == sys.argv[1]:
        r = node["rect"]; print(r["x"], r["y"], r["width"], r["height"]); raise SystemExit(0)
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        walk(child)
walk(json.load(sys.stdin)); raise SystemExit(1)' "$1"
}

# drag KIND TEXT  — starts the source, drags from its middle to the app's terminal, and leaves
# what the source printed in $SRC_OUT.
drag() {
  SRC_OUT="$OUT/dragsrc-$N.log"
  "$SRC" "$1" "$2" >"$SRC_OUT" 2>&1 &
  local src_pid=$!
  wait_for 10 grep -q ready "$SRC_OUT" || { echo "dragsrc did not come up: $(cat "$SRC_OUT")"; return 1; }
  sleep 0.5
  local sx sy sw sh ax ay aw ah
  read -r sx sy sw sh < <(rect_of dragsrc)
  read -r ax ay aw ah < <(rect_of tmls-rs)
  # The same row for both ends (vptr.py drags along a row); the app's right part is the terminal.
  local y=$((ay + ah / 2)) from=$((sx + sw / 2)) to=$((ax + aw * 3 / 4))
  echo "   drag: source $sx,$sy ${sw}x$sh  app $ax,$ay ${aw}x$ah  pointer $from,$y -> $to,$y"
  timeout 15 python3 /work/tooling/smoke/vptr.py drag "$from" "$y" "$to" 2>>"$OUT/vptr.log"
  wait_for 5 grep -q 'finished\|cancelled' "$SRC_OUT"
  kill "$src_pid" 2>/dev/null
  wait "$src_pid" 2>/dev/null
  sleep 0.5
}

# 1. one file
drag text/uri-list "$(printf 'file://%s/drop/one%%20pic.png\r\n' "$HOME")"
grep -q finished "$SRC_OUT" && pass "one file: the source is told the drop was taken" ||
  fail "one file: the source is told the drop was taken" "source said: $(tr '\n' '|' <"$SRC_OUT")"
wait_for 5 pane_has "$HOME/drop/one pic.png" && pass "one file: its path is typed into the session" ||
  fail "one file: its path is typed into the session" "pane: $(pane_text | grep -v '^$' | tail -3 | tr '\n' '|')"
app_alive && drops_on && pass "the app still runs and listens" || fail "the app still runs and listens" "$(tail -3 "$OUT/app.stderr" | tr '\n' ' ')"
timeout 10 grim "$OUT/dnd-1.png" 2>/dev/null
tmux send-keys -t "=$ONE:" C-u

# 2. two files
drag text/uri-list "$(printf '# two of them\r\nfile://%s/drop/one%%20pic.png\r\nfile://%s/drop/two.txt\r\n' "$HOME" "$HOME")"
grep -q finished "$SRC_OUT" && pass "two files: the source is told the drop was taken" ||
  fail "two files: the source is told the drop was taken" "source said: $(tr '\n' '|' <"$SRC_OUT")"
wait_for 5 pane_has "'$HOME/drop/one pic.png' $HOME/drop/two.txt" && pass "two files: both paths, the one with a space quoted" ||
  fail "two files: both paths, the one with a space quoted" "pane: $(pane_text | grep -v '^$' | tail -3 | tr '\n' '|')"
app_alive && drops_on && pass "the app still runs and listens" || fail "the app still runs and listens" "$(tail -3 "$OUT/app.stderr" | tr '\n' ' ')"
tmux send-keys -t "=$ONE:" C-u
sleep 0.3

# 3. not files
drag text/plain "not-a-file-$$"
grep -q cancelled "$SRC_OUT" && pass "text/plain: refused (the source is told cancelled)" ||
  fail "text/plain: refused (the source is told cancelled)" "source said: $(tr '\n' '|' <"$SRC_OUT")"
sleep 0.5
! pane_has "not-a-file-$$" && pass "text/plain: nothing is typed" || fail "text/plain: nothing is typed"
app_alive && drops_on && pass "the app still runs and listens" || fail "the app still runs and listens" "$(tail -3 "$OUT/app.stderr" | tr '\n' ' ')"

# 4. and a drop of files still works after a refused one
drag text/uri-list "$(printf 'file://%s/drop/two.txt\r\n' "$HOME")"
wait_for 5 pane_has "$HOME/drop/two.txt" && pass "a file again after the refused drag" ||
  fail "a file again after the refused drag" "source said: $(tr '\n' '|' <"$SRC_OUT") pane: $(pane_text | grep -v '^$' | tail -2 | tr '\n' '|')"
app_alive && pass "the app still runs" || fail "the app still runs" "$(tail -3 "$OUT/app.stderr" | tr '\n' ' ')"
timeout 10 grim "$OUT/dnd-4.png" 2>/dev/null

kill "$APP_PID" 2>/dev/null
tmux kill-server 2>/dev/null
echo "== $FAILED of $N checks failed"
[ "$FAILED" -eq 0 ]
