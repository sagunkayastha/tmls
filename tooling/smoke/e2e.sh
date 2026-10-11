#!/usr/bin/env bash
# End-to-end smoke test of tmls-rs. Runs INSIDE the tmls-rs-smoke container (see run.sh):
# a headless sway, software Vulkan, its own tmux server, keys through wtype, pictures by grim.
#
# Nothing of the user's is seen: HOME, the tmux socket dir and the Wayland socket are in
# $SMOKE_SCRATCH (a tmpfs in the container). Screenshots and logs go to $SMOKE_OUT.
#
# Numbered checks print PASS/FAIL and the run goes on; the exit status is 1 if any failed.
# Every wait has a timeout. To add a check: `check "what" command…` or pass/fail directly.
set -u

OUT=${SMOKE_OUT:-/out}
SCRATCH=${SMOKE_SCRATCH:-/scratch}
BIN=${SMOKE_BIN:-/work/target-docker/release/tmls-rs}
ONE='tmlsrs-test-one'
ODD="tmlsrs-test-a b'c"
NONCE="$$-$(date +%s)"

# The container must not be the user's machine: this script starts and stops a tmux server.
if [ ! -f /.dockerenv ] && [ "${SMOKE_ALLOW_HOST:-0}" != 1 ]; then
  echo "e2e.sh: runs in the tmls-rs-smoke container only (tooling/smoke/run.sh)" >&2
  exit 2
fi

export HOME="$SCRATCH/home"
export TMUX_TMPDIR="$SCRATCH/tmux"
export XDG_RUNTIME_DIR="$SCRATCH/xdg"
export XDG_CONFIG_HOME="$HOME/.config"
export SHELL=/bin/bash LANG=C.UTF-8 LC_ALL=C.UTF-8 TERM=xterm-256color
unset TMUX DISPLAY WAYLAND_DISPLAY SSH_AUTH_SOCK
mkdir -p "$HOME/.config/tmls" "$TMUX_TMPDIR" "$XDG_RUNTIME_DIR" "$OUT/bench"
chmod 700 "$XDG_RUNTIME_DIR" "$TMUX_TMPDIR"
# No `hosts` file is written: only this machine is listed.
: >"$OUT/app.stderr"
: >"$OUT/bench/latency.jsonl"

# Software Vulkan (lavapipe); the compositor draws with pixman and has no input devices.
export VK_ICD_FILENAMES=${VK_ICD_FILENAMES:-/usr/share/vulkan/icd.d/lvp_icd.x86_64.json}
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
export RUST_BACKTRACE=1

N=0
FAILED=0
pass() { N=$((N + 1)); printf 'PASS %2d  %s\n' "$N" "$1"; }
fail() {
  N=$((N + 1))
  FAILED=$((FAILED + 1))
  printf 'FAIL %2d  %s\n' "$N" "$1"
  [ -n "${2:-}" ] && printf '         %s\n' "$2"
  return 0
}
# check "description" command…  — passes if the command succeeds.
check() {
  local what=$1
  shift
  if "$@"; then pass "$what"; else fail "$what" "failed: $*"; fi
}
# wait_for SECONDS command…  — polls until the command succeeds; fails after SECONDS.
wait_for() {
  local deadline=$((SECONDS + $1))
  shift
  until "$@" >/dev/null 2>&1; do
    [ "$SECONDS" -ge "$deadline" ] && return 1
    sleep 0.2
  done
}

sway_msg() { timeout 5 swaymsg -s "$SWAYSOCK" "$@"; }
window_up() { sway_msg -t get_tree | grep -q '"tmls-rs"'; }
app_alive() { [ -n "${APP_PID:-}" ] && kill -0 "$APP_PID" 2>/dev/null; }
app_gone() { ! app_alive; }

# keys ARGS…  — wtype with a pause between keys. Each call is a new virtual keyboard, and the
# headless seat has no other: the seat gains its keyboard capability when wtype connects, and a
# key sent before the app has bound wl_keyboard is lost. `-s 250` waits for that.
keys() {
  timeout 10 wtype -s 250 -d 30 "$@"
  sleep 0.3
}
key_enter() { keys -k Return; }
key_focus_list() { keys -M alt -M shift -k Up -m shift -m alt; }
key_next_tab() { keys -M alt -M shift -k Right -m shift -m alt; }
key_prev_tab() { keys -M alt -M shift -k Left -m shift -m alt; }
key_quit() { keys -M ctrl -k q -m ctrl; }

# shot NAME  — $OUT/NAME.png; fails the run's check if grim cannot take it.
shot() {
  sleep "${2:-0.7}"
  timeout 10 grim "$OUT/$1.png" 2>>"$OUT/grim.log"
}
# colours NAME  — how many different colours the screenshot has (1 = blank).
colours() {
  timeout 10 grim -t ppm - 2>>"$OUT/grim.log" | python3 -c '
import sys
d = sys.stdin.buffer.read()
# P6\nW H\n255\n then RGB triples
parts = d.split(b"\n", 3)
px = parts[3]
print(len({px[i:i+3] for i in range(0, len(px), 3 * 7)}))
' 2>/dev/null || echo 0
}
picture_drawn() { [ "$(colours)" -ge 4 ]; }

# The session a tmux client of the app is attached to (one name per line).
attached() { tmux list-clients -F '#{session_name}' 2>/dev/null; }
attached_to() { attached | grep -qxF -- "$1"; }
# `=NAME:` — the exact session, its current window and pane (a bare `=NAME` is no pane target).
pane_text() { tmux capture-pane -p -t "=$1:" 2>&1; }
pane_has_line() { pane_text "$1" | grep -qxF -- "$2"; }

cleanup() {
  if app_alive; then
    kill "$APP_PID" 2>/dev/null
    wait_for 3 app_gone || kill -9 "$APP_PID" 2>/dev/null
  fi
  # Only the sessions this script made, on its own socket.
  tmux kill-session -t "=$ONE" 2>/dev/null
  tmux kill-session -t "=$ODD" 2>/dev/null
  [ -n "${SWAY_PID:-}" ] && kill "$SWAY_PID" 2>/dev/null
  return 0
}
trap cleanup EXIT

echo "== tmls-rs e2e: $(date -u +%FT%TZ)  bin $(sha256sum "$BIN" 2>/dev/null | cut -c1-12)  nonce $NONCE"

# --- compositor -------------------------------------------------------------------------------
cat >"$SCRATCH/sway.conf" <<'EOF'
output HEADLESS-1 resolution 1280x800 scale 1
default_border none
default_floating_border none
focus_follows_mouse no
EOF
sway -c "$SCRATCH/sway.conf" >"$OUT/sway.log" 2>&1 &
SWAY_PID=$!
sway_socket() { ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock >/dev/null 2>&1 && ls "$XDG_RUNTIME_DIR"/wayland-? >/dev/null 2>&1; }
if wait_for 15 sway_socket; then
  SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock | head -1)
  WAYLAND_DISPLAY=$(basename "$(ls -d "$XDG_RUNTIME_DIR"/wayland-? | head -1)")
  export SWAYSOCK WAYLAND_DISPLAY
  pass "headless compositor is up ($WAYLAND_DISPLAY)"
else
  fail "headless compositor is up" "see $OUT/sway.log: $(tail -3 "$OUT/sway.log" | tr '\n' ' ')"
  echo "== $FAILED of $N checks failed (no compositor: stopping)"
  exit 1
fi

# --- tmux sessions ----------------------------------------------------------------------------
tmux new-session -d -s "$ONE" -x 100 -y 30
tmux new-session -d -s "$ODD" -x 100 -y 30
check "tmux has both test sessions" bash -c \
  '[ "$(tmux list-sessions -F "#{session_name}" | sort | tr "\n" "|")" = "$0|$1|" ]' "$ODD" "$ONE"

# --- the app ----------------------------------------------------------------------------------
"$BIN" --bench "$OUT/bench" 2>"$OUT/app.stderr" &
APP_PID=$!
if wait_for 30 window_up; then
  pass "the window opened"
else
  fail "the window opened" "app $(app_alive && echo 'still runs' || echo 'exited'); stderr: $(tail -5 "$OUT/app.stderr" | tr '\n' ' ')"
fi
check "the app reports its adapter on stderr" grep -q 'tmls-rs: adapter' "$OUT/app.stderr"

# The first listing arrives right after start; the next one 5 s later.
sleep 2
check "screenshot 01-start (list, nothing open)" shot 01-start
check "the window is drawn (not one flat colour)" picture_drawn

# --- attach with Enter (the list has the focus at start) ---------------------------------------
key_enter
if wait_for 8 bash -c '[ -n "$(tmux list-clients -F x 2>/dev/null)" ]'; then
  pass "Enter in the list attaches (client on: $(attached | tr '\n' ' '))"
else
  fail "Enter in the list attaches" "no tmux client after Enter"
fi

# Bring tmlsrs-test-one to the front: try every row from the top until its client is there.
open_session() {
  local want=$1 row i
  for row in 0 1 2 3; do
    attached_to "$want" && return 0
    key_focus_list
    keys -k k -k k -k k -k k
    for ((i = 0; i < row; i++)); do keys -k j; done
    key_enter
    wait_for 3 attached_to "$want" && return 0
  done
  return 1
}
check "a tab is attached to $ONE" open_session "$ONE"
check "screenshot 02-attached" shot 02-attached

# --- type into the session ---------------------------------------------------------------------
typed_arrives() {
  local try
  for try in 1 2; do
    keys "echo hi-$NONCE"
    key_enter
    wait_for 5 pane_has_line "$ONE" "hi-$NONCE" && return 0
    # The other tab may be in front: switch and try once more.
    key_next_tab
  done
  return 1
}
if typed_arrives; then
  pass "typed 'echo hi-$NONCE' ran in $ONE (tmux capture-pane)"
else
  fail "typed 'echo hi-$NONCE' ran in $ONE (tmux capture-pane)" \
    "pane: $(pane_text "$ONE" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
check "screenshot 03-typed" shot 03-typed

# --- the other tab: Alt+Shift+Left, and a session name with a space and a quote ------------------
other_tab_takes_keys() {
  local try
  attached_to "$ODD" || return 1
  for try in 1 2; do
    key_prev_tab
    keys "echo odd-$NONCE"
    key_enter
    wait_for 5 pane_has_line "$ODD" "odd-$NONCE" && return 0
  done
  return 1
}
if other_tab_takes_keys; then
  pass "Alt+Shift+Left goes to the tab of \"$ODD\"; typed text ran there"
else
  fail "Alt+Shift+Left goes to the tab of \"$ODD\"; typed text ran there" \
    "clients: $(attached | tr '\n' '|') pane: $(pane_text "$ODD" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
check "screenshot 04-other-tab" shot 04-other-tab
# The first tab opened writes the log; both tabs were typed into.
check "latency log has key lines" grep -q '"ev":"key"' "$OUT/bench/latency.jsonl"

# --- text outside ASCII: CJK and an emoji, through the keyboard ---------------------------------
keys "echo uni-$NONCE 日本語 🦀"
key_enter
if wait_for 5 bash -c 'tmux capture-pane -p -t "=$0:" | grep -qxF -- "uni-$1 日本語 🦀"' "$ODD" "$NONCE"; then
  pass "typed CJK and emoji arrive unchanged"
else
  fail "typed CJK and emoji arrive unchanged" "pane: $(pane_text "$ODD" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
check "screenshot 05-unicode" shot 05-unicode

# --- keys in the list reach no session ----------------------------------------------------------
key_focus_list
keys "zzlist$NONCE"
sleep 0.5
if pane_text "$ONE" | grep -q "zzlist" || pane_text "$ODD" | grep -q "zzlist"; then
  fail "keys typed with the focus in the list reach no session" "found zzlist in a pane"
else
  pass "keys typed with the focus in the list reach no session"
fi
check "screenshot 06-list-focus" shot 06-list-focus

# --- quit ---------------------------------------------------------------------------------------
key_quit
if wait_for 10 app_gone; then
  wait "$APP_PID"
  status=$?
  if [ "$status" -eq 0 ]; then pass "Ctrl+Q quits, exit status 0"; else fail "Ctrl+Q quits, exit status 0" "exit status $status"; fi
else
  fail "Ctrl+Q quits, exit status 0" "still running 10 s after Ctrl+Q"
fi
check "both sessions outlive the app" bash -c 'tmux has-session -t "=$0" && tmux has-session -t "=$1"' "$ONE" "$ODD"
if grep -nE "panicked|stack backtrace|RUST_BACKTRACE" "$OUT/app.stderr" >/dev/null; then
  fail "no panic on stderr" "$(grep -nE 'panicked' -A3 "$OUT/app.stderr" | head -6 | tr '\n' ' ')"
else
  pass "no panic on stderr"
fi

echo "--- app stderr ($OUT/app.stderr)"
sed 's/^/    /' "$OUT/app.stderr" | head -40
echo "== $FAILED of $N checks failed"
[ "$FAILED" -eq 0 ]
