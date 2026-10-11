#!/usr/bin/env bash
# End-to-end smoke test of tmls-rs. Runs INSIDE the tmls-rs-smoke container (see run.sh):
# a headless sway, software Vulkan, its own tmux server, keys through wtype, clicks through
# vptr.py, pictures by grim.
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

# Software Vulkan (lavapipe); the compositor draws with pixman and has no input devices: keys
# come from wtype, clicks from vptr.py, each a virtual device for as long as it runs.
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
key_close_tab() { keys -M alt -M shift -k w -m shift -m alt; }
key_zoom_in() { keys -M ctrl -k equal -m ctrl; }
key_zoom_out() { keys -M ctrl -k minus -m ctrl; }

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
# picture  — a hash of what is on the screen now.
picture() { timeout 10 grim -t ppm - 2>>"$OUT/grim.log" | sha256sum | cut -c1-16; }

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
START_PICTURE=$(picture)

# --- with no tab open: pin, zoom, unpin ---------------------------------------------------------
# The list has the focus at start and its cursor is on the first session, "$ODD".
PINS="$HOME/.config/tmls/pins"
SLUG=$(python3 -c '
import hashlib, sys
name = sys.argv[1]
readable = "".join(c if (c.isascii() and c.isalnum()) or c in "_-" else "_" for c in "local-" + name)
print(readable + "-" + hashlib.sha1(("local\0" + name).encode()).hexdigest()[:8])' "$ODD")
pins_are() { [ "$(cat "$PINS" 2>/dev/null)" = "$1" ]; }
only_pins_written() { [ "$(find "$HOME/.config/tmls" -mindepth 1 | sort | tr '\n' '|')" = "$PINS|" ]; }
keys -k p
if wait_for 3 pins_are "$SLUG"; then
  pass "p in the list pins: pins holds exactly $SLUG"
else
  fail "p in the list pins: pins holds exactly $SLUG" "pins: $(tr '\n' '|' <"$PINS" 2>&1)"
fi
check "nothing but pins was written to ~/.config/tmls" only_pins_written
check "screenshot 02-pinned" shot 02-pinned
if [ "$(picture)" != "$START_PICTURE" ]; then
  pass "the window was drawn again with no tab open (the picture changed with the pin)"
else
  fail "the window was drawn again with no tab open (the picture changed with the pin)" "same picture as at start"
fi
key_zoom_in
check "screenshot 03-zoomed-no-tab" shot 03-zoomed-no-tab
key_zoom_out
keys -k p
if wait_for 3 pins_are ""; then pass "p again unpins: pins is empty"; else fail "p again unpins: pins is empty" "pins: $(tr '\n' '|' <"$PINS")"; fi
sleep 0.5
if [ "$(picture)" = "$START_PICTURE" ] && app_alive; then
  pass "after zoom in and out and the unpin the picture is the one from the start, pixel for pixel"
else
  fail "after zoom in and out and the unpin the picture is the one from the start, pixel for pixel" \
    "app $(app_alive && echo runs || echo exited)"
  shot 03b-not-as-start
fi

# --- attach with Enter ---------------------------------------------------------------------------
# typed SESSION TAG  — types `echo TAG-nonce` and Enter; passes when that line is in SESSION.
typed() {
  keys "echo $2-$NONCE"
  key_enter
  wait_for 5 pane_has_line "$1" "$2-$NONCE"
}
# check_typed "what" SESSION TAG
check_typed() {
  if typed "$2" "$3"; then
    pass "$1"
  else
    fail "$1" "clients: $(attached | tr '\n' '|') pane of $2: $(pane_text "$2" | grep -v '^$' | tail -3 | tr '\n' '|')"
  fi
}
nowhere() { ! pane_text "$ONE" | grep -q -- "$1" && ! pane_text "$ODD" | grep -q -- "$1"; }

key_enter
if wait_for 8 attached_to "$ODD" && [ "$(attached | wc -l)" -eq 1 ]; then
  pass "Enter on the first row attaches \"$ODD\" (one tmux client)"
else
  fail "Enter on the first row attaches \"$ODD\" (one tmux client)" "clients: $(attached | tr '\n' '|')"
fi
check "screenshot 04-attached" shot 04-attached
check_typed "typed 'echo hi-$NONCE' ran in the first tab (tmux capture-pane)" "$ODD" hi
check "screenshot 05-typed" shot 05-typed

# --- the latency log: --bench logs the first tab opened ------------------------------------------
# latency_ok MIN  — at least MIN key lines, and every key id has a write line and is in the
# keys of a frame line.
latency_ok() {
  python3 - "$OUT/bench/latency.jsonl" "$1" <<'EOF'
import json, sys
keys, writes, framed = [], set(), set()
for line in open(sys.argv[1]):
    try:
        ev = json.loads(line)
    except ValueError:
        continue  # a line still being written
    if ev["ev"] == "key":
        keys.append(ev["id"])
    elif ev["ev"] == "write":
        writes.add(ev["id"])
    elif ev["ev"] == "frame":
        framed.update(ev["keys"])
no_write = [k for k in keys if k not in writes]
no_frame = [k for k in keys if k not in framed]
print(f"{len(keys)} key lines, {len(no_write)} without a write line, {len(no_frame)} in no frame line")
sys.exit(0 if len(keys) >= int(sys.argv[2]) and len(set(keys)) == len(keys) and not no_write and not no_frame else 1)
EOF
}
FIRST_LINE="echo hi-$NONCE"
sleep 0.7
if detail=$(latency_ok $((${#FIRST_LINE} + 1))); then
  pass "every key of the nonce line is in the latency log with its write and its frame ($detail)"
else
  fail "every key of the nonce line is in the latency log with its write and its frame" "$detail; wanted >= $((${#FIRST_LINE} + 1))"
fi

# --- the second tab, from the list's second row ---------------------------------------------------
key_focus_list
keys -k j
key_enter
if wait_for 8 attached_to "$ONE" && [ "$(attached | wc -l)" -eq 2 ]; then
  pass "j and Enter in the list open a second tab on $ONE (two tmux clients)"
else
  fail "j and Enter in the list open a second tab on $ONE (two tmux clients)" "clients: $(attached | tr '\n' '|')"
fi
check_typed "typing goes to the second tab" "$ONE" two
check "screenshot 06-second-tab" shot 06-second-tab
key_prev_tab
check_typed "Alt+Shift+Left: typing goes to \"$ODD\"" "$ODD" left
key_next_tab
check_typed "Alt+Shift+Right: typing goes to $ONE" "$ONE" right

# --- text outside ASCII: CJK and an emoji, through the keyboard ---------------------------------
keys "echo uni-$NONCE 日本語 🦀"
key_enter
if wait_for 5 pane_has_line "$ONE" "uni-$NONCE 日本語 🦀"; then
  pass "typed CJK and emoji arrive unchanged"
else
  fail "typed CJK and emoji arrive unchanged" "pane: $(pane_text "$ONE" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
check "screenshot 07-unicode" shot 07-unicode

# --- keys in the list reach no session; Esc gives the keys back ----------------------------------
key_focus_list
keys "zzlist$NONCE"
sleep 0.5
check "keys typed with the focus in the list reach no session" nowhere zzlist
check "screenshot 08-list-focus" shot 08-list-focus
keys -k Escape
check_typed "Esc in the list gives the keys back to the terminal" "$ONE" esc

# --- zoom, then a tab switch ----------------------------------------------------------------------
key_zoom_in
key_zoom_in
key_prev_tab
check_typed "after Ctrl+= twice and a tab switch typing still arrives" "$ODD" zoom
check "screenshot 09-zoomed" shot 09-zoomed
key_zoom_out
key_zoom_out
key_next_tab
check_typed "after Ctrl+- twice and a tab switch typing still arrives" "$ONE" unzoom

# --- the mouse (tooling/smoke/vptr.py; cells are 8x17 px at this size) ---------------------------
# Tab 0 is "$ODD", tab 1 is $ONE and in front. Tabs are 24 cells wide from x=225.
click() { timeout 10 python3 /work/tooling/smoke/vptr.py "$@" 2>>"$OUT/vptr.log"; sleep 0.3; }
: >"$OUT/vptr.log"
if click 300 11; then
  check_typed "a click on the first tab brings it to the front" "$ODD" clicktab
  check "screenshot 10-clicked-tab" shot 10-clicked-tab
  click 597 11
  if wait_for 5 bash -c '! tmux list-clients -F "#{session_name}" | grep -qxF -- "$0"' "$ONE" && tmux has-session -t "=$ONE"; then
    pass "a click on the × of the second tab closes it; the session lives on"
  else
    fail "a click on the × of the second tab closes it; the session lives on" "clients: $(attached | tr '\n' '|')"
  fi
  check "screenshot 11-closed-by-click" shot 11-closed-by-click
  # The list: line 0 is the host's header, line 1 "$ODD", line 2 $ONE.
  click 100 11
  sleep 0.5
  if [ "$(attached | wc -l)" -eq 1 ]; then
    pass "a click on the host's header attaches nothing"
  else
    fail "a click on the host's header attaches nothing" "clients: $(attached | tr '\n' '|')"
  fi
  click 100 45
  if wait_for 8 attached_to "$ONE"; then
    pass "a click on the list's third line attaches $ONE"
  else
    fail "a click on the list's third line attaches $ONE" "clients: $(attached | tr '\n' '|')"
  fi
  check_typed "after the click the keys go to the new tab" "$ONE" clickrow
  check "screenshot 12-click-attached" shot 12-click-attached
else
  fail "the headless seat takes a pointer (vptr.py)" "$(tail -2 "$OUT/vptr.log" | tr '\n' ' ')"
  # The later checks need $ONE in front.
  key_next_tab
fi

# --- the session of the front tab ends ------------------------------------------------------------
tmux kill-session -t "=$ONE"
sleep 1.5
check "the app still runs after the front tab's session was killed" app_alive
check "screenshot 13-ended" shot 13-ended
keys "lost$NONCE"
sleep 0.5
check "keys typed on the ended tab reach no session" nowhere lost
key_prev_tab
check_typed "the other tab still takes typing" "$ODD" other

# --- Enter on the ended tab reattaches ------------------------------------------------------------
tmux new-session -d -s "$ONE" -x 100 -y 30
key_next_tab
key_enter
if wait_for 8 attached_to "$ONE"; then
  pass "Enter on the ended tab reattaches (the session was made again)"
else
  fail "Enter on the ended tab reattaches (the session was made again)" "clients: $(attached | tr '\n' '|')"
fi
check_typed "the reattached tab takes typing" "$ONE" again
check "screenshot 14-reattached" shot 14-reattached

# --- a session ends while its tab is in the back ---------------------------------------------------
key_prev_tab
check_typed "\"$ODD\" is in front" "$ODD" front
BEFORE=$(pane_text "$ODD")
tmux kill-session -t "=$ONE"
sleep 1.5
check "screenshot 15-ended-in-back" shot 15-ended-in-back
if [ "$(pane_text "$ODD")" = "$BEFORE" ] && app_alive; then
  pass "a tab ending in the back types nothing into the front tab"
else
  fail "a tab ending in the back types nothing into the front tab" "pane changed or app gone"
fi
check_typed "the front tab takes typing after a tab in the back ended" "$ODD" back
key_next_tab
check "screenshot 16-ended-came-to-front" shot 16-ended-came-to-front
tmux new-session -d -s "$ONE" -x 100 -y 30
key_enter
if wait_for 8 attached_to "$ONE"; then
  pass "Enter reattaches the tab that ended in the back"
else
  fail "Enter reattaches the tab that ended in the back" "clients: $(attached | tr '\n' '|')"
fi

# --- Alt+Shift+W closes the tab, not the session ---------------------------------------------------
key_close_tab
if wait_for 5 bash -c '! tmux list-clients -F "#{session_name}" | grep -qxF -- "$0"' "$ONE" && tmux has-session -t "=$ONE"; then
  pass "Alt+Shift+W closes the tab; tmux still has $ONE"
else
  fail "Alt+Shift+W closes the tab; tmux still has $ONE" "clients: $(attached | tr '\n' '|')"
fi
check_typed "the keys go to the tab that is left" "$ODD" left2
check "screenshot 17-closed" shot 17-closed

# --- quit ---------------------------------------------------------------------------------------
sleep 0.7
key_quit
if wait_for 10 app_gone; then
  wait "$APP_PID"
  status=$?
  if [ "$status" -eq 0 ]; then pass "Ctrl+Q quits, exit status 0"; else fail "Ctrl+Q quits, exit status 0" "exit status $status"; fi
else
  fail "Ctrl+Q quits, exit status 0" "still running 10 s after Ctrl+Q"
fi
check "both sessions outlive the app" bash -c 'tmux has-session -t "=$0" && tmux has-session -t "=$1"' "$ONE" "$ODD"
if detail=$(latency_ok 1); then
  pass "at exit every key in the latency log has its write and its frame ($detail)"
else
  fail "at exit every key in the latency log has its write and its frame" "$detail"
fi
check "at exit ~/.config/tmls holds pins and nothing else" only_pins_written
check "pins is empty again" pins_are ""
if grep -nE "panicked|stack backtrace|RUST_BACKTRACE" "$OUT/app.stderr" >/dev/null; then
  fail "no panic on stderr" "$(grep -nE 'panicked' -A3 "$OUT/app.stderr" | head -6 | tr '\n' ' ')"
else
  pass "no panic on stderr"
fi

echo "--- files under the scratch HOME"
find "$HOME" -type f | sed 's/^/    /' | head -20
echo "--- app stderr ($OUT/app.stderr)"
sed 's/^/    /' "$OUT/app.stderr" | head -40
echo "== $FAILED of $N checks failed"
[ "$FAILED" -eq 0 ]
