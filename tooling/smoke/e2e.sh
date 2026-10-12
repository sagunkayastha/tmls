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
NEW='tmlsrs-test-new'
RENAMED='tmlsrs-test-renamed'
BYE='tmlsrs-test-bye'
APPROVE='tmlsrs-test-0approve'
APPROVE_SID='1b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'
PUSH='tmlsrs-test-2push'
PUSH_SID='3b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'
ASK='tmlsrs-test-1ask'
ASK_SID='2b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'
ASK2='tmlsrs-test-1asked'
WIN='tmlsrs-test-00win'
# The stand-ins for Claude processes, by session id: their status files are named after them.
declare -A FAKE_PIDS=()
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
# Without its own HOME, tmux socket and Wayland socket the script would use the caller's.
mkdir -p "$HOME/.config/tmls" "$TMUX_TMPDIR" "$XDG_RUNTIME_DIR" "$OUT/bench" &&
  chmod 700 "$XDG_RUNTIME_DIR" "$TMUX_TMPDIR" || {
  echo "e2e.sh: cannot make the scratch directories under $SCRATCH and $OUT" >&2
  exit 2
}
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
  tmux kill-session -t "=$NEW" 2>/dev/null
  tmux kill-session -t "=$RENAMED" 2>/dev/null
  tmux kill-session -t "=$BYE" 2>/dev/null
  # The stand-in for a Claude process (the images' list), if it still runs.
  [ -n "${FAKE_CLAUDE:-}" ] && kill "$FAKE_CLAUDE" 2>/dev/null
  tmux kill-session -t "=$APPROVE" 2>/dev/null
  tmux kill-session -t "=$PUSH" 2>/dev/null
  tmux kill-session -t "=$ASK" 2>/dev/null
  tmux kill-session -t "=$ASK2" 2>/dev/null
  tmux kill-session -t "=$WIN" 2>/dev/null
  for pid in "${FAKE_PIDS[@]}"; do kill "$pid" 2>/dev/null; done
  [ -n "${SWAY_PID:-}" ] && kill "$SWAY_PID" 2>/dev/null
  return 0
}
trap cleanup EXIT

echo "== tmls-rs e2e: $(date -u +%FT%TZ)  bin $(sha256sum "$BIN" 2>/dev/null | cut -c1-12)  nonce $NONCE"

# --- compositor -------------------------------------------------------------------------------
# SMOKE_SCALE: the output's scale (default 1). The output keeps its 1280x800 logical pixels —
# its mode is that times the scale — so vptr.py's positions (it sends them as parts of 1280x800)
# mean the same places at any scale, and a screenshot is the scale times as large.
SCALE=${SMOKE_SCALE:-1}
MODE=$(python3 -c 'import sys; s = float(sys.argv[1]); print("%dx%d" % (round(1280 * s), round(800 * s)))' "$SCALE") || {
  echo "e2e.sh: SMOKE_SCALE=$SCALE is no number" >&2
  exit 2
}
echo "== output: mode $MODE, scale $SCALE"
cat >"$SCRATCH/sway.conf" <<EOF
output HEADLESS-1 resolution $MODE scale $SCALE
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
# SMOKE_OUTSIDE=1: also two Claude status files that name no tmux pane (this script's own
# process is "Claude"), to look at the list's section for sessions outside tmux. Off by
# default: the rows it adds are not what the checks below count on.
if [ "${SMOKE_OUTSIDE:-}" = 1 ]; then
  mkdir -p "$HOME/.claude/sessions"
  printf '{"pid":%s,"kind":"interactive","status":"waiting","waitingFor":"permission prompt","statusUpdatedAt":1,"name":"notes outside","cwd":"/work"}\n' "$$" >"$HOME/.claude/sessions/$$.json"
  printf '{"pid":%s,"kind":"interactive","status":"busy","statusUpdatedAt":1,"cwd":"/work/some-folder"}\n' "$PPID" >"$HOME/.claude/sessions/$PPID.json"
fi
check "tmux has both test sessions" bash -c \
  '[ "$(tmux list-sessions -F "#{session_name}" | sort | tr "\n" "|")" = "$0|$1|" ]' "$ODD" "$ONE"

# A stand-in for notify-send, first on PATH: one line per call, its arguments ending in "|".
# In $OUT, not $SCRATCH: that tmpfs is mounted noexec.
NOTIFY_LOG="$OUT/notify.log"
: >"$NOTIFY_LOG"
mkdir -p "$OUT/bin"
cat >"$OUT/bin/notify-send" <<EOF
#!/bin/sh
{ printf '%s|' "\$@"; echo; } >>"$NOTIFY_LOG"
EOF
chmod +x "$OUT/bin/notify-send"
# tmux as it is, but while $OUT/slow-capture is there a capture-pane takes a second first, as
# on a slow host (the approval check that keys typed on win over a late dialog read); while
# $OUT/slow-create is there a has-session takes three (a create that fails late).
cat >"$OUT/bin/tmux" <<EOF
#!/bin/sh
[ "\$1" = capture-pane ] && [ -e "$OUT/slow-capture" ] && sleep 1
[ "\$1" = has-session ] && [ -e "$OUT/slow-create" ] && sleep 3
exec /usr/bin/tmux "\$@"
EOF
chmod +x "$OUT/bin/tmux"
export PATH="$OUT/bin:$PATH"
# How often the stand-in was asked to say that $ONE is done.
notified() { grep -cxF -- "--|tmls · $(hostname)|◆ $ONE · done|" "$NOTIFY_LOG"; }
notified_is() { [ "$(notified)" -eq "$1" ]; }

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

# --- SMOKE_ONLY=measure: the text panel beside a full terminal, one screenshot, and out -----------
# For tooling/smoke/rows.py, at any SMOKE_SCALE: the first session attached and its pane filled
# with a path, the panel opened on that file by Ctrl+click, `measure-panel.png` taken. None of
# the checks further down run (their pixel checks are for scale 1).
if [ "${SMOKE_ONLY:-}" = measure ]; then
  keys -k Return
  wait_for 8 bash -c 'tmux list-clients -F "#{session_name}" | grep -qxF -- "$0"' "$ODD" &&
    pass "Enter attaches \"$ODD\"" || fail "Enter attaches \"$ODD\""
  VIEWED="$HOME/viewme.txt"
  seq -f 'line %g of the viewed file' 1 200 >"$VIEWED"
  tmux send-keys -t "=$ODD:" "clear; for i in \$(seq 1 120); do echo $VIEWED; done" Enter
  sleep 1.5
  COLS_BEFORE=$(tmux display -p -t "=$ODD:" '#{window_width}')
  check "screenshot measure-terminal" shot measure-terminal
  timeout 10 wtype -M ctrl -s 4000 -m ctrl &
  HOLD=$!
  sleep 0.8
  timeout 10 python3 /work/tooling/smoke/vptr.py 300 300 2>>"$OUT/vptr.log"
  if wait_for 8 bash -c '[ "$(tmux display -p -t "=$0:" "#{window_width}")" -lt "$1" ]' "$ODD" "$COLS_BEFORE"; then
    pass "Ctrl+click on the path opens the panel ($COLS_BEFORE -> $(tmux display -p -t "=$ODD:" '#{window_width}') columns)"
  else
    fail "Ctrl+click on the path opens the panel" "columns $COLS_BEFORE"
  fi
  wait "$HOLD" 2>/dev/null
  check "screenshot measure-panel" shot measure-panel
  grep -E 'tmls-rs: (adapter|scale)' "$OUT/app.stderr" | sed 's/^/    /'
  echo "== $FAILED of $N checks failed (SMOKE_ONLY=measure, scale $SCALE)"
  [ "$FAILED" -eq 0 ]
  exit
fi
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

# The same with the tab that writes the latency log in front ("$ODD", the first one opened):
# keys for the list add no `key` line.
key_lines() { grep -c '"ev":"key"' "$OUT/bench/latency.jsonl"; }
key_prev_tab
check_typed "the logged tab (\"$ODD\") is in front" "$ODD" logged
sleep 0.7
LINES_BEFORE=$(key_lines)
key_focus_list
keys "zzlogged$NONCE"
sleep 0.7
LINES_AFTER=$(key_lines)
if [ "$LINES_BEFORE" -gt 0 ] && [ "$LINES_AFTER" = "$LINES_BEFORE" ] && nowhere zzlogged; then
  pass "keys typed in the list with the logged tab in front add no key line to the latency log ($LINES_AFTER)"
else
  fail "keys typed in the list with the logged tab in front add no key line to the latency log" \
    "key lines before $LINES_BEFORE, after $LINES_AFTER"
fi
keys -k Escape
key_next_tab
check_typed "back on $ONE" "$ONE" unlogged

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
click() {
  local status=0
  timeout 10 python3 /work/tooling/smoke/vptr.py "$@" 2>>"$OUT/vptr.log" || status=$?
  sleep 0.3
  return "$status"
}
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
  # The list: line 0 is the host's header, line 1 "$ODD", line 2 $ONE. Its lines are the
  # terminal's rows: 17 px each from y 23, under a band as high as the tab bar.
  click 100 31
  sleep 0.5
  if [ "$(attached | wc -l)" -eq 1 ]; then
    pass "a click on the host's header attaches nothing"
  else
    fail "a click on the host's header attaches nothing" "clients: $(attached | tr '\n' '|')"
  fi
  click 100 65
  if wait_for 8 attached_to "$ONE"; then
    pass "a click on the list's third line attaches $ONE"
  else
    fail "a click on the list's third line attaches $ONE" "clients: $(attached | tr '\n' '|')"
  fi
  check_typed "after the click the keys go to the new tab" "$ONE" clickrow
  check "screenshot 12-click-attached" shot 12-click-attached
  # The wheel over the terminal ($ONE is in front; the pane is right of the list). tmux asks for
  # mouse reports only with `mouse on`: without, the wheel is the app's own and tmux hears nothing.
  in_mode() { [ "$(tmux display -p -t "=$ONE:" '#{pane_in_mode}' 2>/dev/null)" = "$1" ]; }
  tmux send-keys -t "=$ONE:" 'seq 1 200' Enter
  wait_for 5 pane_has_line "$ONE" 200
  click 700 300 wheel-up 2
  sleep 0.5
  check "with tmux's mouse off the wheel is not sent to tmux (no copy mode)" in_mode 0
  tmux set -g mouse on
  sleep 0.7
  click 700 300 wheel-up 2
  if wait_for 5 in_mode 1; then
    pass "with tmux's mouse on the wheel scrolls tmux's history (the pane is in copy mode)"
  else
    fail "with tmux's mouse on the wheel scrolls tmux's history (the pane is in copy mode)" \
      "pane_in_mode: $(tmux display -p -t "=$ONE:" '#{pane_in_mode}' 2>&1)"
  fi
  check "screenshot 12a-wheel-history" shot 12a-wheel-history
  click 700 300 wheel-down 4
  if wait_for 5 in_mode 0; then
    pass "wheeling back down to the end leaves copy mode"
  else
    fail "wheeling back down to the end leaves copy mode" \
      "pane_in_mode: $(tmux display -p -t "=$ONE:" '#{pane_in_mode}' 2>&1)"
    tmux send-keys -t "=$ONE:" -X cancel 2>/dev/null
  fi
  tmux set -g mouse off
  sleep 0.5
  check_typed "after the wheel typing still arrives" "$ONE" wheeled

  # --- the file panel: Ctrl+click on a path in the terminal ---------------------------------------
  # The pane is filled with the path, so the click lands on it whatever the row. Ctrl is held by
  # a wtype of its own for as long as the click takes.
  VIEWED="$HOME/viewme.txt"
  seq -f 'line %g of the viewed file' 1 60 >"$VIEWED"
  pane_cols() { tmux display -p -t "=$ONE:" '#{window_width}' 2>/dev/null; }
  narrower_than() { [ "$(pane_cols)" -lt "$1" ]; }
  as_wide_as() { [ "$(pane_cols)" -eq "$1" ]; }
  tmux send-keys -t "=$ONE:" "clear; for i in \$(seq 1 40); do echo $VIEWED; done" Enter
  wait_for 5 pane_has_line "$ONE" "$VIEWED"
  COLS_BEFORE=$(pane_cols)
  timeout 10 wtype -M ctrl -s 4000 -m ctrl &
  HOLD=$!
  sleep 0.8
  click 300 300
  if wait_for 8 narrower_than "$COLS_BEFORE"; then
    pass "Ctrl+click on a file's path opens the panel: the pane is narrower ($COLS_BEFORE -> $(pane_cols) columns)"
  else
    fail "Ctrl+click on a file's path opens the panel: the pane is narrower" "columns $COLS_BEFORE -> $(pane_cols)"
  fi
  wait "$HOLD" 2>/dev/null
  check "screenshot 12b-file-panel" shot 12b-file-panel
  click 1100 400 wheel-down 3
  check "screenshot 12c-file-panel-scrolled" shot 12c-file-panel-scrolled
  # The divider. Right of the list (28 cells and its line: 225 px) are 1055 px; the panel opens
  # with what fits in half of them, the line, 4 px of padding and 65 columns: 525 px, so the
  # divider is the pixel column 755. Dragged 200 px to the right it leaves the panel 40 columns
  # (1280 - 955 - 5 = 320 px) and the terminal 25 more; the width is saved when the button
  # comes up. Nothing was saved before: the panel opened with the default.
  UI="$HOME/.config/tmls-rs/ui"
  wider_than() { [ "$(pane_cols)" -gt "$1" ]; }
  ui_has() { grep -qxF -- "$1" "$UI" 2>/dev/null; }
  check "before a drag there is no ~/.config/tmls-rs/ui" test ! -e "$UI"
  COLS_PANEL=$(pane_cols)
  click drag 755 400 955
  if wait_for 8 wider_than "$COLS_PANEL"; then
    pass "dragging the panel's divider to the right widens the pane ($COLS_PANEL -> $(pane_cols) columns)"
  else
    fail "dragging the panel's divider to the right widens the pane" "columns $COLS_PANEL -> $(pane_cols)"
  fi
  if wait_for 3 ui_has "panel_cols=40"; then
    pass "after the drag ~/.config/tmls-rs/ui holds panel_cols=40"
  else
    fail "after the drag ~/.config/tmls-rs/ui holds panel_cols=40" "ui: $(tr '\n' '|' <"$UI" 2>&1)"
  fi
  check "screenshot 12d-divider-dragged" shot 12d-divider-dragged
  COLS_DRAGGED=$(pane_cols)
  # Back to the left, past what the terminal can give: it keeps its 20 columns.
  click drag 955 400 240
  if wait_for 8 as_wide_as 20; then
    pass "dragged far to the left the divider stops where the terminal has 20 columns"
  else
    fail "dragged far to the left the divider stops where the terminal has 20 columns" "columns $COLS_DRAGGED -> $(pane_cols)"
  fi
  check "screenshot 12e-divider-far-left" shot 12e-divider-far-left
  # And from there (the divider is the pixel column 395 now) to where it was dragged first.
  # Then a press on the handle's outermost pixel (it is 4 px either side of the divider) and
  # a pixel's move: the divider keeps its distance from the pointer, so no column changes.
  click drag 395 400 955
  click drag 951 400 950
  if wait_for 8 as_wide_as "$COLS_DRAGGED" && wait_for 3 ui_has "panel_cols=40"; then
    pass "dragged back, and pressed on the handle's edge, the pane has its $COLS_DRAGGED columns and ui holds panel_cols=40"
  else
    fail "dragged back, and pressed on the handle's edge, the pane has its $COLS_DRAGGED columns and ui holds panel_cols=40" \
      "columns $(pane_cols); ui: $(tr '\n' '|' <"$UI" 2>&1)"
  fi
  # --- a picture in the panel ---------------------------------------------------------------------
  # 400x300 pixels of one orange, written with python's zlib. The panel is open with its text
  # and 40 columns: the divider is the pixel column 955, as dragged above, so the panel's text
  # and the picture have the 320 pixel columns from 960 (1280 - 955 - 5 = 320). The picture is
  # wider than that: fitted it is 320x240, 80 % of its size.
  ORANGE="$HOME/orange.png"
  python3 - "$ORANGE" <<'PY'
import struct, sys, zlib
w, h = 400, 300
def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
rows = (b"\x00" + bytes((0xFF, 0x88, 0x00)) * w) * h
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY
  # orange  — of the pixels on the screen that are exactly #FF8800: how many are in the panel's
  # pixel columns (from 960), the box around those (WxH+X+Y), and how many are left of them.
  orange() {
    timeout 10 grim -t ppm - 2>>"$OUT/grim.log" | python3 -c '
import sys
d = sys.stdin.buffer.read()
# P6\nW H\n255\n then RGB triples
parts = d.split(b"\n", 3)
w, h = map(int, parts[1].split())
px = parts[3]
row = b"\xff\x88\x00" * w
inside = outside = 0
xs, ys = [], []
for y in range(h):
    line = px[3 * w * y:3 * w * (y + 1)]
    if b"\xff\x88\x00" not in line:
        continue
    hit = [x for x in range(w) if line[3 * x:3 * x + 3] == b"\xff\x88\x00"]
    here = [x for x in hit if x >= 960]
    outside += len(hit) - len(here)
    if here:
        inside += len(here)
        xs += [here[0], here[-1]]
        ys.append(y)
box = "%dx%d+%d+%d" % (max(xs) - min(xs) + 1, ys[-1] - ys[0] + 1, min(xs), ys[0]) if xs else "none"
print(inside, box, outside)
' 2>/dev/null || echo "0 none 0"
    return 0
  }
  orange_at_least() { [ "$(orange | cut -d' ' -f1)" -ge "$1" ]; }
  orange_is() { case "$(orange)" in $1) return 0 ;; *) return 1 ;; esac; }
  tmux send-keys -t "=$ONE:" "clear; for i in \$(seq 1 40); do echo $ORANGE; done" Enter
  wait_for 5 pane_has_line "$ONE" "$ORANGE"
  check "before the click on the picture's path no pixel of the screen is its orange" orange_is "0 none 0"
  timeout 10 wtype -M ctrl -s 4000 -m ctrl &
  HOLD=$!
  sleep 0.8
  click 300 300
  if wait_for 8 orange_at_least 10000; then
    pass "Ctrl+click on a picture's path shows it in the panel: $(orange | cut -d' ' -f1) pixels of its orange right of the divider"
  else
    fail "Ctrl+click on a picture's path shows it in the panel" "orange pixels, their box, outside the panel: $(orange)"
  fi
  wait "$HOLD" 2>/dev/null
  check "screenshot 12f-image-panel" shot 12f-image-panel
  # Fitted: as wide as the panel's 320 pixels, its shape kept, nothing of it outside the panel.
  if orange_is "76800 320x240+960+* 0"; then
    pass "the picture is fitted into the panel with its shape kept: $(orange | cut -d' ' -f2)"
  else
    fail "the picture is fitted into the panel with its shape kept (320x240 from x=960)" "orange pixels, their box, outside the panel: $(orange)"
  fi
  check "with the picture open the pane keeps its $COLS_DRAGGED columns" as_wide_as "$COLS_DRAGGED"
  # A click on it: its own size, one texel a pixel. 320 of its 400 columns fit, all 300 rows.
  click 1100 400
  if wait_for 5 orange_is "96000 320x300+960+* 0"; then
    pass "a click on the picture shows it at its own size: $(orange | cut -d' ' -f2)"
  else
    fail "a click on the picture shows it at its own size (320x300 from x=960)" "orange pixels, their box, outside the panel: $(orange)"
  fi
  check "screenshot 12g-image-actual" shot 12g-image-actual
  # And a click again fits it.
  click 1100 400
  if wait_for 5 orange_is "76800 320x240+960+* 0"; then
    pass "a click again fits the picture"
  else
    fail "a click again fits the picture" "orange pixels, their box, outside the panel: $(orange)"
  fi
  keys -M alt -M shift -k x -m shift -m alt
  if wait_for 8 as_wide_as "$COLS_BEFORE"; then
    pass "Alt+Shift+X closes the panel: the pane has its $COLS_BEFORE columns again"
  else
    fail "Alt+Shift+X closes the panel: the pane has its columns again" "columns $COLS_BEFORE -> $(pane_cols)"
  fi
  check "with the panel closed nothing of the picture is left on the screen" orange_is "0 none 0"
  tmux send-keys -t "=$ONE:" 'clear' Enter
  sleep 0.5
  check_typed "after the panel typing still arrives" "$ONE" viewed

  # --- the images a session made: Alt+Shift+F and `Files` ----------------------------------------
  # $ONE is in front and no Claude session yet: the panel opens (with the 40 columns dragged
  # above, so the pane has its $COLS_DRAGGED) and says so; no command runs for it.
  keys -M alt -M shift -k f -m shift -m alt
  if wait_for 8 as_wide_as "$COLS_DRAGGED"; then
    pass "Alt+Shift+F opens the panel without a file: the pane is narrower ($COLS_BEFORE -> $(pane_cols) columns)"
  else
    fail "Alt+Shift+F opens the panel without a file: the pane is narrower" "columns $COLS_BEFORE -> $(pane_cols)"
  fi
  check "screenshot 12j-made-no-claude (no Claude session in this tab)" shot 12j-made-no-claude
  # panel_picture  — a hash of the panel's pixel columns only (from 960): not of tmux's clock in
  # the pane, nor of the session's mark in the list and on its tab, which change by themselves.
  panel_picture() {
    timeout 10 grim -t ppm - 2>>"$OUT/grim.log" | python3 -c '
import hashlib, sys
d = sys.stdin.buffer.read()
parts = d.split(b"\n", 3)
w, h = map(int, parts[1].split())
px = parts[3]
crop = hashlib.sha256()
for y in range(h):
    crop.update(px[3 * (w * y + 960):3 * w * (y + 1)])
print(crop.hexdigest()[:16])
' 2>/dev/null || echo none
  }
  NO_CLAUDE_PICTURE=$(panel_picture)
  # Now $ONE is a Claude session for the listing: a status file named after a live process (a
  # sleep of this script's; the listing skips the file once that is gone) that names $ONE's
  # pane and a session id, and that id's transcript, which names the orange picture and one
  # that does not exist. The next listing (every 5 s) tells the app, which then asks for the
  # list by itself: the panel is open on this tab.
  SID='0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'
  sleep 900 &
  FAKE_CLAUDE=$!
  mkdir -p "$HOME/.claude/sessions" "$HOME/.claude/projects/-work"
  printf '{"type":"assistant","message":"saved %s and %s"}\n' "$ORANGE" "$HOME/missing.png" \
    >"$HOME/.claude/projects/-work/$SID.jsonl"
  printf '{"pid":%s,"sessionId":"%s","status":"idle","statusUpdatedAt":1,"tmux":"%s:@0.%%0","cwd":"/work"}\n' \
    "$FAKE_CLAUDE" "$SID" "$ONE" >"$HOME/.claude/sessions/$FAKE_CLAUDE.json"
  listed_now() {
    local now
    now=$(panel_picture)
    [ "$now" != none ] && [ "$NO_CLAUDE_PICTURE" != none ] && [ "$now" != "$NO_CLAUDE_PICTURE" ]
  }
  if wait_for 15 listed_now; then
    pass "once the listing has $ONE as a Claude session the panel lists its images by itself (the panel's pixels changed)"
  else
    fail "once the listing has $ONE as a Claude session the panel lists its images by itself" "the panel's pixels are as without a Claude session"
  fi
  check "screenshot 12h-made-list" shot 12h-made-list
  check "the list shows no picture yet: no pixel of the screen is the orange" orange_is "0 none 0"
  check "with the list open the pane keeps its $COLS_DRAGGED columns" as_wide_as "$COLS_DRAGGED"
  # The panel's header is in the tab bar's band (y 0-22), so the list's rows are the terminal's:
  # the first is y 23-39.
  # Clicked until the picture shows or 20 s passed: a click that came before the row was there
  # (on the one line a panel without a list has) does nothing, and the next one is the row's.
  # Never a click once the picture shows: that one would be the picture's.
  row_opens() {
    local deadline=$((SECONDS + 20))
    until orange_at_least 10000; do
      [ "$SECONDS" -ge "$deadline" ] && return 1
      click 1100 31
      wait_for 4 orange_at_least 10000 && return 0
    done
  }
  if row_opens && wait_for 5 orange_is "76800 320x240+960+* 0"; then
    pass "a click on the list's first row shows the picture the transcript names, fitted: $(orange | cut -d' ' -f2)"
  else
    fail "a click on the list's first row shows the picture the transcript names" "orange pixels, their box, outside the panel: $(orange)"
  fi
  check "screenshot 12i-made-image" shot 12i-made-image
  # `‹` is the header's first column (x 960-967) and the cell after it.
  click 964 11
  if wait_for 5 orange_is "0 none 0"; then
    pass "a click on the ‹ of the picture's header leads back to the list"
  else
    fail "a click on the ‹ of the picture's header leads back to the list" "orange pixels: $(orange)"
  fi
  # The click gave the panel the keys: Enter opens the row under the cursor (and is no line in
  # the session behind), Backspace leads back.
  sleep 0.5
  PROMPTS=$(pane_text "$ONE" | grep -c .)
  key_enter
  if wait_for 8 orange_at_least 10000; then
    pass "Enter in the list opens the picture under the cursor"
  else
    fail "Enter in the list opens the picture under the cursor" "orange pixels: $(orange)"
  fi
  keys -k BackSpace
  if wait_for 5 orange_is "0 none 0"; then
    pass "Backspace at the picture leads back to the list"
  else
    fail "Backspace at the picture leads back to the list" "orange pixels: $(orange)"
  fi
  if [ "$(pane_text "$ONE" | grep -c .)" -eq "$PROMPTS" ]; then
    pass "Enter and Backspace in the panel reach no session (the pane has its $PROMPTS lines)"
  else
    fail "Enter and Backspace in the panel reach no session" "lines in the pane: $PROMPTS -> $(pane_text "$ONE" | grep -c .)"
  fi
  # Any other key is the terminal's: typed with the list focused, a line arrives whole (its
  # first letter takes the keys back and is sent too).
  check_typed "what is typed with the list focused goes to the terminal, first letter and all" "$ONE" inlist
  # A click on the list gives it the keys again, and Esc there hands them back without a trace.
  click 1100 300
  keys -k Escape
  check_typed "after a click on the list and Esc the keys are the terminal's" "$ONE" afterlist
  keys -M alt -M shift -k f -m shift -m alt
  if wait_for 8 as_wide_as "$COLS_BEFORE"; then
    pass "Alt+Shift+F again closes the panel: the pane has its $COLS_BEFORE columns again"
  else
    fail "Alt+Shift+F again closes the panel: the pane has its columns again" "columns $COLS_BEFORE -> $(pane_cols)"
  fi
  # `Files` is the tab bar's last seven columns: x 1224-1279 with no panel, and left of the
  # panel's divider (the pixel column 955, as dragged above) with one: x 899-954.
  click 1250 11
  if wait_for 8 as_wide_as "$COLS_DRAGGED"; then
    pass "a click on Files at the tab bar's right end opens the panel"
  else
    fail "a click on Files at the tab bar's right end opens the panel" "columns $(pane_cols)"
  fi
  click 925 11
  if wait_for 8 as_wide_as "$COLS_BEFORE"; then
    pass "a click on Files again closes it"
  else
    fail "a click on Files again closes it" "columns $(pane_cols)"
  fi
  # $ONE is no Claude session any more for what follows.
  kill "$FAKE_CLAUDE" 2>/dev/null
  wait "$FAKE_CLAUDE" 2>/dev/null
  check_typed "after the list typing still arrives" "$ONE" listed
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

# --- new session, rename, kill (Task 6) ---------------------------------------------------------
# "$ODD" is the only tab, in front and logged; the list's cursor is on it.
has() { tmux has-session -t "=$1" 2>/dev/null; }
gone() { ! has "$1"; }
sessions() { tmux list-sessions -F '#{session_name}' 2>/dev/null | sort | tr '\n' '|'; }
key_menu() { key_focus_list; keys -k m; }
# in_no_pane TEXT SESSION…  — none of the sessions shows TEXT.
in_no_pane() {
  local text=$1 session
  shift
  for session in "$@"; do
    pane_text "$session" | grep -qF -- "$text" && return 1
  done
  return 0
}
clients_are() { [ "$(attached | wc -l)" -eq "$1" ]; }

# c in the list: the new-session form for the cursor's host. Tab Tab: the name, selected.
BEFORE=$(pane_text "$ODD")
sleep 0.7
LINES_BEFORE=$(key_lines)
PLAIN=$(picture)
key_focus_list
keys -k c
check "screenshot 18-new-form" shot 18-new-form
keys -k Tab -k Tab
keys "$NEW"
check "screenshot 19-new-form-name" shot 19-new-form-name
if [ "$(pane_text "$ODD")" = "$BEFORE" ] && [ "$(key_lines)" = "$LINES_BEFORE" ] && nowhere "$NEW"; then
  pass "a name typed into the form reaches no session (the pane behind is unchanged, no key line added)"
else
  fail "a name typed into the form reaches no session (the pane behind is unchanged, no key line added)" \
    "key lines $LINES_BEFORE -> $(key_lines); pane: $(pane_text "$ODD" | grep -v '^$' | tail -2 | tr '\n' '|')"
fi
# Tab: Start (claude is the default; there is none here). Up: shell. Enter creates.
keys -k Tab -k Up
check "screenshot 20-new-form-shell" shot 20-new-form-shell
check "no session is made before Create" gone "$NEW"
key_enter
if wait_for 8 has "$NEW"; then
  pass "the form creates $NEW (tmux has-session)"
else
  fail "the form creates $NEW (tmux has-session)" "sessions: $(sessions); stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
if wait_for 8 attached_to "$NEW"; then
  pass "the new session's tab opens (a tmux client is attached to it)"
else
  fail "the new session's tab opens (a tmux client is attached to it)" "clients: $(attached | tr '\n' '|')"
fi
check_typed "the new tab takes typing" "$NEW" new
check "the new session started a shell, not claude" bash -c '! tmux capture-pane -p -t "=$0:" | grep -q "claude"' "$NEW"
check "screenshot 21-created" shot 21-created

# An invalid name blocks Create: the form stays, nothing is made.
key_focus_list
keys -k c
keys -k Tab -k Tab
keys "tmlsrs-test-a.b"
key_enter
sleep 0.5
check "screenshot 22-new-form-invalid" shot 22-new-form-invalid
if [ "$(sessions)" = "$ODD|$NEW|$ONE|" ]; then
  pass "a name with a dot creates nothing (sessions unchanged)"
else
  fail "a name with a dot creates nothing (sessions unchanged)" "sessions: $(sessions)"
fi
keys -k Escape
keys -k Escape
check_typed "Esc closes the form and then leaves the list: typing goes to $NEW" "$NEW" afterform

# A create that fails while the user waits: the form comes back with what was typed.
NOFOLDER="/nonexistent-$NONCE"
nofolder_lines() { grep -cF -- "no such folder: $NOFOLDER" "$OUT/app.stderr"; }
nofolder_lines_are() { [ "$(nofolder_lines)" -eq "$1" ]; }
key_focus_list
keys -k c
keys -k Tab
keys "$NOFOLDER"
keys -k Tab
keys "tmlsrs-test-nofolder"
key_enter
if wait_for 8 nofolder_lines_are 1; then
  pass "a create in a folder that does not exist fails and says why"
else
  fail "a create in a folder that does not exist fails and says why" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
sleep 0.5
check "screenshot 22b-new-form-failed (the form again, with the folder and the reason)" shot 22b-new-form-failed
# Enter on the form that came back creates again with the same values: the same failure.
key_enter
if wait_for 8 nofolder_lines_are 2; then
  pass "the failed form comes back with its values when nothing was typed (Enter tries again)"
else
  fail "the failed form comes back with its values when nothing was typed (Enter tries again)" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
sleep 0.5
keys -k Escape
keys -k Escape
check_typed "Esc closes the failed form and leaves the list: typing goes to $NEW" "$NEW" afterfailed

# A create that fails late, after the user typed on: no form over what they typed into; the
# reason is in the status line, and c brings the form back with the values.
exists_lines() { grep -cF -- "a session named $ONE already exists" "$OUT/app.stderr"; }
exists_lines_are() { [ "$(exists_lines)" -eq "$1" ]; }
key_focus_list
keys -k c
keys -k Tab -k Tab
keys "$ONE"
: >"$OUT/slow-create"
key_enter
keys -k Escape
keys "echo typed-on-$NONCE"
key_enter
if wait_for 8 exists_lines_are 1; then
  pass "a create that fails late says why in the status line"
else
  fail "a create that fails late says why in the status line" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
rm -f "$OUT/slow-create"
sleep 0.5
keys "echo after-failed-$NONCE"
key_enter
if wait_for 5 pane_has_line "$NEW" "after-failed-$NONCE"; then
  pass "a create that fails after the user typed on opens no form: the keys stay with the terminal"
else
  fail "a create that fails after the user typed on opens no form: the keys stay with the terminal" \
    "pane: $(pane_text "$NEW" | grep -v '^$' | tail -3 | tr '\n' '|'); stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
check "screenshot 22c-late-failure (no form; the reason in the status line)" shot 22c-late-failure
key_focus_list
keys -k c
key_enter
if wait_for 8 exists_lines_are 2; then
  pass "c after a late failure brings the form back with its values (Enter tries again)"
else
  fail "c after a late failure brings the form back with its values (Enter tries again)" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
sleep 0.5
keys -k Escape
keys -k Escape
check_typed "Esc closes that form and leaves the list: typing goes to $NEW" "$NEW" afterlate

# m in the list: the session's menu; Enter on its first row: rename.
key_menu
check "screenshot 23-menu" shot 23-menu
key_enter
check "screenshot 24-rename" shot 24-rename
keys "$RENAMED"
key_enter
if wait_for 8 has "$RENAMED" && gone "$NEW"; then
  pass "rename: tmux has $RENAMED and no $NEW"
else
  fail "rename: tmux has $RENAMED and no $NEW" "sessions: $(sessions); stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
if wait_for 8 attached_to "$RENAMED" && wait_for 5 clients_are 2; then
  pass "the tab is reopened under the new name (two clients, one on $RENAMED)"
else
  fail "the tab is reopened under the new name (two clients, one on $RENAMED)" "clients: $(attached | tr '\n' '|')"
fi
# The rename was asked from the list: the keys stay there. Letters that are no list key,
# typed now, reach no session; Esc in the list then gives the keys to the terminal.
keys "zqzq"
sleep 0.5
if in_no_pane zqzq "$RENAMED" "$ODD" "$ONE"; then
  pass "after a rename from the list the keys stay in the list: typing reaches no session"
else
  fail "after a rename from the list the keys stay in the list: typing reaches no session" "pane of $RENAMED: $(pane_text "$RENAMED" | grep -v '^$' | tail -2 | tr '\n' '|') pane of $ODD: $(pane_text "$ODD" | grep -v '^$' | tail -2 | tr '\n' '|')"
fi
keys -k Escape
check_typed "Esc in the list after the rename: the renamed tab takes typing" "$RENAMED" renamed
check "screenshot 25-renamed" shot 25-renamed

# Kill asks first. Something runs in the session, so the question names it.
keys "sleep 300"
key_enter
sleep 0.5
key_menu
keys -k Down
key_enter
sleep 1
check "screenshot 26-kill-confirm" shot 26-kill-confirm
# Enter on the default (Cancel) kills nothing.
key_enter
sleep 1
if has "$RENAMED" && attached_to "$RENAMED"; then
  pass "Enter on the confirm's default cancels: $RENAMED lives and its tab is attached"
else
  fail "Enter on the confirm's default cancels: $RENAMED lives and its tab is attached" "sessions: $(sessions)"
fi
# Esc cancels too.
keys -k m
keys -k Down
key_enter
keys -k Escape
sleep 1
check "Esc on the confirm cancels: $RENAMED lives" has "$RENAMED"
# Tab to Kill, Enter: now it is killed, and nothing else.
keys -k m
keys -k Down
key_enter
keys -k Tab
check "screenshot 27-kill-armed" shot 27-kill-armed
key_enter
if wait_for 8 gone "$RENAMED" && [ "$(sessions)" = "$ODD|$ONE|" ]; then
  pass "Tab and Enter on the confirm kill $RENAMED and no other session"
else
  fail "Tab and Enter on the confirm kill $RENAMED and no other session" "sessions: $(sessions)"
fi
if wait_for 5 clients_are 1; then
  pass "the killed session's tab is closed (one client left)"
else
  fail "the killed session's tab is closed (one client left)" "clients: $(attached | tr '\n' '|')"
fi
check "the app still runs after the kill" app_alive
# The kill was asked from the list: the tab that came to the front does not take the keys.
keys "wxwx"
sleep 0.5
if in_no_pane wxwx "$ODD" "$ONE"; then
  pass "after a kill from the list the keys stay in the list: typing reaches no session"
else
  fail "after a kill from the list the keys stay in the list: typing reaches no session" "pane of $ODD: $(pane_text "$ODD" | grep -v '^$' | tail -2 | tr '\n' '|')"
fi
keys -k Escape
check_typed "Esc in the list after the kill: the keys go to \"$ODD\"" "$ODD" afterkill
check "screenshot 28-killed" shot 28-killed

# --- the same by mouse; a session that ends from inside ---------------------------------------
# For the alerts checks further down: $ONE (no tab) prints something, so the next listing has
# it running; 30 s after that it counts as done, unwatched. Counted from here.
tmux send-keys -t "=$ONE:" "echo alert-$NONCE" Enter
sleep 7
NOTIFIED=$(notified)
# Right-click on "$ODD" (the list's second line): its menu at the pointer. Esc closes it.
PLAIN=$(picture)
click 100 48 right
check "screenshot 29-menu-by-mouse" shot 29-menu-by-mouse
if [ "$(picture)" != "$PLAIN" ]; then
  pass "a right-click on a session opens its menu (the picture changed)"
else
  fail "a right-click on a session opens its menu (the picture changed)" "same picture"
fi
keys -k Escape
check_typed "Esc closes the menu: the keys are the terminal's again" "$ODD" aftermenu
# A click on the `+` of the host's header opens the form; the session is made from it.
click 212 31
keys -k Tab -k Tab
keys "$BYE"
keys -k Tab -k Up
check "screenshot 30-form-by-mouse" shot 30-form-by-mouse
# The Create button: the form is 60x10 cells of 8x17 px in the middle of 1280x800 (corner at
# 400,315); the button is on its line 8, columns 50-57.
click 832 459
if wait_for 8 has "$BYE" && wait_for 8 attached_to "$BYE"; then
  pass "the + of a host opens the form and a click on Create makes and attaches $BYE"
else
  fail "the + of a host opens the form and a click on Create makes and attaches $BYE" \
    "sessions: $(sessions) clients: $(attached | tr '\n' '|')"
fi
check_typed "the tab of $BYE takes typing" "$BYE" bye
# The session's only shell exits: the session is gone and its tab says so.
keys "exit"
key_enter
if wait_for 8 gone "$BYE"; then
  pass "exit in the tab's shell ends $BYE (tmux has-session fails)"
else
  fail "exit in the tab's shell ends $BYE (tmux has-session fails)" "sessions: $(sessions)"
fi
sleep 1.5
check "the app still runs after a session ended from inside" app_alive
check "screenshot 31-ended-from-inside" shot 31-ended-from-inside
keys "gone$NONCE"
sleep 0.5
check "keys typed on the tab that ended from inside reach no session" nowhere gone
key_prev_tab
check_typed "the other tab still takes typing" "$ODD" afterexit
key_next_tab
key_close_tab
check_typed "the ended tab is closed: the keys go to \"$ODD\"" "$ODD" closedbye

# --- alerts (Task 8) ----------------------------------------------------------------------------
# "$ODD" is the only tab and in front; $ONE went quiet in the back.
if wait_for 50 notified_is $((NOTIFIED + 1)) && sleep 6 && notified_is $((NOTIFIED + 1)); then
  pass "$ONE finishing in the back runs notify-send once: --, the title, '◆ $ONE · done'"
else
  fail "$ONE finishing in the back runs notify-send once: --, the title, '◆ $ONE · done'" \
    "was $NOTIFIED, now $(notified); log: $(tail -4 "$NOTIFY_LOG" | tr '\n' ' ')"
fi
check "screenshot 32-bell (the bell has a count)" shot 32-bell
PLAIN=$(picture)
key_focus_list
keys -k a
check "screenshot 33-alerts" shot 33-alerts
if [ "$(picture)" != "$PLAIN" ]; then
  pass "a in the list opens the alerts (the picture changed)"
else
  fail "a in the list opens the alerts (the picture changed)" "same picture"
fi
# The newest alert is $ONE's and has the cursor.
key_enter
if wait_for 8 attached_to "$ONE"; then
  pass "Enter on the newest alert opens the tab of $ONE"
else
  fail "Enter on the newest alert opens the tab of $ONE" "clients: $(attached | tr '\n' '|')"
fi
key_close_tab
check_typed "the alert's tab is closed: the keys go to \"$ODD\"" "$ODD" afteralert

# --- the keys' help (F1) -------------------------------------------------------------------------
# "$ODD" is the only tab, in front, with the keys.
if "$BIN" keys | grep -qF 'Alt+Shift+N'; then
  pass "tmls-rs keys prints the key table"
else
  fail "tmls-rs keys prints the key table" "$("$BIN" keys 2>&1 | head -3 | tr '\n' ' ')"
fi
key_focus_list
NO_HELP=$(picture)
keys "?"
HELP_PICTURE=$(picture)
check "screenshot 34-help" shot 34-help
if [ "$HELP_PICTURE" != "$NO_HELP" ]; then
  pass "? in the list opens the help (the picture changed)"
else
  fail "? in the list opens the help (the picture changed)" "same picture"
fi
keys -k Escape
sleep 0.5
if [ "$(picture)" != "$HELP_PICTURE" ]; then
  pass "Esc closes the help"
else
  fail "Esc closes the help" "the picture is the help's"
fi
# Esc again: the keys go back to the terminal of "$ODD".
keys -k Escape
check_typed "after the help the keys go to \"$ODD\" again" "$ODD" afterhelp

# --- copy and paste (F2) -------------------------------------------------------------------------
# "$ODD" is in front with the keys; tmux's mouse is off, so a drag selects here. wl-paste reads
# the compositor's clipboard on its own, apart from the app.
clipboard() { timeout 5 wl-paste --no-newline 2>/dev/null; }
clipboard_has() { clipboard | grep -qF -- "$1"; }
keys "clear; printf '  COPYME-%s\n' $NONCE"
key_enter
if wait_for 5 pane_has_line "$ODD" "  COPYME-$NONCE"; then
  pass "the line to copy is on the pane's first row"
else
  fail "the line to copy is on the pane's first row" "pane: $(pane_text "$ODD" | head -2 | tr '\n' '|')"
fi
# Shift held while the pointer drags along the first row (y 31), from its start.
timeout 10 wtype -M shift -s 3000 -m shift &
HOLD=$!
sleep 0.8
click drag 230 31 700
wait "$HOLD" 2>/dev/null
# Something else on the clipboard first: only Ctrl+Shift+C can put the line back.
timeout 5 wl-copy "not-it-$NONCE"
keys -M ctrl -M shift -k c -m shift -m ctrl
if wait_for 5 clipboard_has "COPYME-$NONCE"; then
  pass "Shift+drag, then Ctrl+Shift+C puts the selection on the clipboard (wl-paste)"
else
  fail "Shift+drag, then Ctrl+Shift+C puts the selection on the clipboard (wl-paste)" \
    "clipboard: $(clipboard | head -c 80); stderr: $(grep -E 'copy|clipboard' "$OUT/app.stderr" | tail -3 | tr '\n' ' ')"
fi
check "screenshot 35-copied (the status line says copied N chars)" shot 35-copied
keys "echo PASTED:"
keys -M ctrl -M shift -k v -m shift -m ctrl
key_enter
pasted() { pane_text "$ODD" | grep -qE "^PASTED: ?COPYME-$NONCE\$"; }
if wait_for 5 pasted; then
  pass "Ctrl+Shift+V pastes it into the tab"
else
  fail "Ctrl+Shift+V pastes it into the tab" "pane: $(pane_text "$ODD" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
check "no copy or paste failure on stderr" bash -c '! grep -qE "(copy|paste) failed" "$0"' "$OUT/app.stderr"
# The fallback for real (crates/desktop/tests/clip_wayland.rs, built by run.sh): arboard off,
# wl-copy and wl-paste do the copy and the paste, each in under 300 ms (the UI thread waits).
CLIP_WL="arboard off: wl-copy and wl-paste copy and paste, each under 300 ms (tests/clip_wayland.rs)"
if [ -n "${SMOKE_CLIP_TEST:-}" ] && [ -x "$SMOKE_CLIP_TEST" ]; then
  if CLIP_SAID=$(TMLS_RS_NO_ARBOARD=1 timeout 30 "$SMOKE_CLIP_TEST" --nocapture 2>&1) &&
    grep -q "the wl-clipboard fallback: copy and paste" <<<"$CLIP_SAID"; then
    pass "$CLIP_WL"
  else
    fail "$CLIP_WL" "$(grep -E 'panicked|took|left|right|error' <<<"$CLIP_SAID" | head -4 | tr '\n' ' ')"
  fi
else
  fail "$CLIP_WL" "no test binary (SMOKE_CLIP_TEST=${SMOKE_CLIP_TEST:-})"
fi

# fake_claude NAME SID STATUS [WAITING_FOR] — (re)writes the status file of a Claude with session
# id SID that runs in NAME's pane and is STATUS. The file is named after a sleep of this script's
# (the listing skips files of dead processes); statusUpdatedAt is now.
fake_claude() {
  local name=$1 sid=$2 status=$3 waiting=${4:-} pid ids
  pid=${FAKE_PIDS[$sid]:-}
  if [ -z "$pid" ]; then
    sleep 900 &
    pid=$!
    FAKE_PIDS[$sid]=$pid
  fi
  ids=$(tmux display -p -t "=$name:" '#{window_id}.#{pane_id}')
  mkdir -p "$HOME/.claude/sessions"
  printf '{"pid":%s,"kind":"interactive","sessionId":"%s","status":"%s","waitingFor":"%s","statusUpdatedAt":%s,"tmux":"%s:%s","cwd":"/work"}\n' \
    "$pid" "$sid" "$status" "$waiting" "$(date +%s%3N)" "$name" "$ids" >"$HOME/.claude/sessions/$pid.json"
}
# top_of_list  — the list's cursor to its first session (pins are empty here).
top_of_list() { keys -k k -k k -k k -k k -k k -k k -k k -k k; }
# said_lines — what the app wrote on stderr, but its focus reports.
said_lines() { grep -v '^tmls-rs: focus ' "$OUT/app.stderr"; }
said_count() { said_lines | wc -l; }
said_since() { [ "$(said_count)" -gt "$1" ]; }
# select_row NAME — the list's cursor (the list has the keys) to the row of the session NAME,
# found by its name, not its place, so a session another block left behind moves nothing: y on
# a row that is not asking says "NAME isn't asking anything" at once; a row that asks opens its
# form instead and says nothing, and Esc closes that.
select_row() {
  local name=$1 n before
  top_of_list
  for n in 1 2 3 4 5 6 7 8 9 10; do
    before=$(said_count)
    keys y
    if wait_for 2 said_since "$before"; then
      [ "$(said_lines | tail -1)" = "tmls-rs: $name isn't asking anything" ] && return 0
    else
      keys -k Escape
    fi
    keys -k j
  done
  return 1
}

# --- approvals from the list (F5) ----------------------------------------------------------------
# A stand-in for Claude asking: the dialog of client-core's fixture, then one key read and shown.
# $APPROVE sorts first ('0' < 'a'), so it is the list's first session.
cp /work/crates/client-core/tests/data/approve-dialog.txt "$SCRATCH/dialog.txt"
cat >"$SCRATCH/dialog.sh" <<'EOF'
clear
cat "$1"
IFS= read -rsn1 k
printf 'GOT[%s]\n' "$k"
sleep 600
EOF
tmux new-session -d -s "$APPROVE" -x 100 -y 40 "bash '$SCRATCH/dialog.sh' '$SCRATCH/dialog.txt'"
fake_claude "$APPROVE" "$APPROVE_SID" waiting "permission prompt"
# `y` is refused at once, in stderr, while the list's first row is not one that asks (the
# listing has not shown $APPROVE yet); a `y` that is not refused within a second was taken.
refusals() { grep -cE "isn't asking anything|is not listed any more|runs outside tmux" "$OUT/app.stderr"; }
refused_since() { [ "$(refusals)" -gt "$1" ]; }
y_taken() {
  local before
  before=$(refusals)
  top_of_list
  keys y
  ! wait_for 1 refused_since "$before"
}
key_focus_list
if wait_for 20 y_taken; then
  pass "the listing shows $APPROVE asking: y on its row is taken"
else
  fail "the listing shows $APPROVE asking: y on its row is taken" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
check "screenshot 36-approve (the dialog, Yes / No / Cancel)" shot 36-approve
key_enter
if wait_for 8 pane_has_line "$APPROVE" "GOT[1]"; then
  pass "y on a row that asks, then Enter, presses 1 in its pane"
else
  fail "y on a row that asks, then Enter, presses 1 in its pane" \
    "pane: $(pane_text "$APPROVE" | grep -v '^$' | tail -3 | tr '\n' '|'); stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
# Keys typed after the y belong to what they were typed into: a dialog read that comes back
# after them opens no form, so an Enter meant for the shell never answers Yes. The same prompt
# again (the pane id stays), then y, Esc to the terminal and a command line at once.
tmux respawn-pane -k -t "=$APPROVE:" "bash '$SCRATCH/dialog.sh' '$SCRATCH/dialog.txt'"
fake_claude "$APPROVE" "$APPROVE_SID" waiting "permission prompt"
wait_for 5 pane_has_line "$APPROVE" " Do you want to proceed?"
key_focus_list
top_of_list
# One burst (1 ms between keys; wtype refuses 0): Esc and the text are handled before the
# capture is back, as when a slow host answers late. The capture is made slow (the tmux
# stand-in): an idle machine's took a few ms and sometimes came back before the Esc, and then
# the form opened and the Esc closed it, leaving the text to the list.
: >"$OUT/slow-capture"
timeout 10 wtype -s 250 -d 1 y -k Escape "echo moved-on-$NONCE"
sleep 0.3
key_enter
if wait_for 5 pane_has_line "$ODD" "moved-on-$NONCE"; then
  pass "y, then Esc and a command line at once: the keys reach the terminal"
else
  fail "y, then Esc and a command line at once: the keys reach the terminal" \
    "pane: $(pane_text "$ODD" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
sleep 2
check "screenshot 37-approve-moved-on (no form over the terminal)" shot 37-approve-moved-on
if pane_text "$APPROVE" | grep -q '^GOT\['; then
  fail "a dialog read that comes back after the user typed on answers nothing" \
    "pane: $(pane_text "$APPROVE" | grep '^GOT' | tr '\n' '|')"
else
  pass "a dialog read that comes back after the user typed on answers nothing"
fi
# ... and the read did come back: the app says it dropped the form (not a read that never ran).
if wait_for 5 grep -qF -- "tmls-rs: $APPROVE: the prompt's answer form was dropped — keys moved on" "$OUT/app.stderr"; then
  pass "the late dialog read was dropped because keys moved on (said on stderr)"
else
  fail "the late dialog read was dropped because keys moved on (said on stderr)" "stderr: $(tail -3 "$OUT/app.stderr" | tr '\n' ' ')"
fi
rm -f "$OUT/slow-capture"

# --- send or queue a message (F7) ----------------------------------------------------------------
# $ASK is a shell that a status file calls Claude's.
tmux new-session -d -s "$ASK" -x 100 -y 30
fake_claude "$ASK" "$ASK_SID" idle
sleep 6
# The F5 block left the keys with the terminal.
key_focus_list
check "the list's cursor finds $ASK by its name" select_row "$ASK"
keys s
check "screenshot 37-ask (Send to $ASK, the default saved prompt)" shot 37-ask
keys "echo tmls-ask-ok-$NONCE"
key_enter
if wait_for 8 pane_has_line "$ASK" "tmls-ask-ok-$NONCE"; then
  pass "s on an idle Claude row sends the message at once"
else
  fail "s on an idle Claude row sends the message at once" "pane: $(pane_text "$ASK" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
fake_claude "$ASK" "$ASK_SID" busy
sleep 6
keys s
keys "echo tmls-queued-$NONCE"
key_enter
sleep 2
check "a message for a busy session waits in the queue" \
  bash -c '! tmux capture-pane -p -t "=$0:" | grep -qxF -- "$1"' "$ASK" "tmls-queued-$NONCE"
check "screenshot 38-queued (1✉ on the row)" shot 38-queued
fake_claude "$ASK" "$ASK_SID" idle
if wait_for 12 pane_has_line "$ASK" "tmls-queued-$NONCE"; then
  pass "when its turn ends the queued message is sent"
else
  fail "when its turn ends the queued message is sent" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
# Claude exits while a message waits for it: the session is a plain shell again, and a message
# typed there would run as a command. It stays in the queue and the app says why.
fake_claude "$ASK" "$ASK_SID" busy
sleep 6
keys s
keys "echo tmls-gone-\$(echo RAN)-$NONCE"
key_enter
sleep 2
check "a message for the busy session waits in the queue again" \
  bash -c '! tmux capture-pane -p -t "=$0:" | grep -qF -- "$1"' "$ASK" "tmls-gone-"
# Its process ends: the listing drops the status file of a dead process (kill -0), so the row is
# the shell left behind, busy until 30 s without output, then idle.
kill "${FAKE_PIDS[$ASK_SID]}" 2>/dev/null
wait "${FAKE_PIDS[$ASK_SID]}" 2>/dev/null
unset "FAKE_PIDS[$ASK_SID]"
gone_said() { grep -qF -- "tmls-rs: Claude is gone from $ASK; 1 kept" "$OUT/app.stderr"; }
if wait_for 60 gone_said; then
  pass "Claude gone, its turn over: the app says so and keeps the message"
else
  fail "Claude gone, its turn over: the app says so and keeps the message" "stderr: $(tail -3 "$OUT/app.stderr" | tr '\n' ' ')"
fi
sleep 1
if pane_text "$ASK" | grep -qF -- "tmls-gone-"; then
  fail "a queued message is never typed into the shell Claude left" "pane: $(pane_text "$ASK" | grep -v '^$' | tail -3 | tr '\n' '|')"
else
  pass "a queued message is never typed into the shell Claude left"
fi
check "screenshot 39-claude-gone (the row keeps 1✉)" shot 39-claude-gone
# Claude again in $ASK (a new process), for the rename below; the cursor back on its row.
fake_claude "$ASK" "$ASK_SID" idle
sleep 6
check "the list's cursor finds $ASK again" select_row "$ASK"

# --- a session renamed outside the app keeps its tab (F8) ---------------------------------------------
key_enter
if wait_for 8 attached_to "$ASK"; then pass "Enter attaches $ASK"; else fail "Enter attaches $ASK" "clients: $(attached | tr '\n' '|')"; fi
tmux rename-session -t "=$ASK" "$ASK2"
# Claude writes its status file again under the new name (here: the stand-in does).
fake_claude "$ASK2" "$ASK_SID" idle
titled() { sway_msg -t get_tree | grep -qF -- "$ASK2"; }
if wait_for 12 titled; then
  pass "a session renamed outside the app keeps its tab: the window is titled $ASK2"
else
  fail "a session renamed outside the app keeps its tab: the window is titled $ASK2" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
check "screenshot 41-renamed-outside" shot 41-renamed-outside
check_typed "the renamed session's tab still takes the keys" "$ASK2" renamedout
key_close_tab
check_typed "its tab closed: the keys go to \"$ODD\"" "$ODD" afterrename

# --- the row menu's window and pane entries (G1) -------------------------------------------------
# $WIN is a plain shell, so its menu is: Rename, Kill…, Open in new terminal, Copy attach command,
# New window, Rename window…, Split side by side, Split top / bottom, Kill pane….
panes_are() { [ "$(tmux list-panes -s -t "=$1" -F '#{pane_id}' 2>/dev/null | wc -l)" -eq "$2" ]; }
windows_are() { [ "$(tmux list-windows -t "=$1" -F '#{window_id}' 2>/dev/null | wc -l)" -eq "$2" ]; }
tmux new-session -d -s "$WIN" -x 100 -y 30
sleep 6
key_focus_list
check "the list's cursor finds $WIN by its name" select_row "$WIN"
keys -k m
check "screenshot 42-window-menu (nine entries, Kill… and Kill pane… red)" shot 42-window-menu
keys -k j -k j -k j -k j -k j -k j
key_enter
if wait_for 8 panes_are "$WIN" 2; then
  pass "Split side by side from the menu: the session has two panes"
else
  fail "Split side by side from the menu: the session has two panes" "panes: $(tmux list-panes -s -t "=$WIN" 2>&1 | tr '\n' '|')"
fi
keys -k m
keys -k j -k j -k j -k j
key_enter
if wait_for 8 windows_are "$WIN" 2; then
  pass "New window from the menu: the session has two windows"
else
  fail "New window from the menu: the session has two windows" "windows: $(tmux list-windows -t "=$WIN" 2>&1 | tr '\n' '|')"
fi
# Kill pane… on the new window's only pane: the confirm, Tab to Kill pane, Enter.
keys -k m
keys -k j -k j -k j -k j -k j -k j -k j -k j
key_enter
sleep 1
check "screenshot 43-kill-pane (the question, what runs)" shot 43-kill-pane
keys -k Tab
key_enter
if wait_for 8 windows_are "$WIN" 1 && panes_are "$WIN" 2; then
  pass "Kill pane… then Kill pane ends the active pane only (one window, two panes left)"
else
  fail "Kill pane… then Kill pane ends the active pane only" "windows: $(tmux list-windows -t "=$WIN" 2>&1 | wc -l), panes: $(tmux list-panes -s -t "=$WIN" 2>&1 | wc -l)"
fi
tmux kill-session -t "=$WIN"

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

# --- push: events.jsonl moves the marks without a poll (F6) ----------------------------------------
# A second run of the app that lists every 60 s: what changes within seconds came by push.
PUSH_START_NOTIFIED=$(wc -l <"$NOTIFY_LOG")
tmux new-session -d -s "$PUSH" -x 100 -y 30
fake_claude "$PUSH" "$PUSH_SID" idle
EVENTS="$HOME/.cache/tmls/events.jsonl"
mkdir -p "$HOME/.cache/tmls" && : >"$EVENTS"
PUSH_PANE=$(tmux display -p -t "=$PUSH:" '#{pane_id}')
# push_event EV [MORE_JSON] — one line as tmls's hook writes it, timed now.
push_event() {
  printf '{"t":%s,"ev":"%s","sid":"%s","pane":"%s","cwd":"/work"%s}\n' \
    "$(date +%s.%N)" "$1" "$PUSH_SID" "$PUSH_PANE" "${2:-}" >>"$EVENTS"
}
# list_picture — a hash of the list's pixel columns only (0-223).
list_picture() {
  timeout 10 grim -t ppm - 2>>"$OUT/grim.log" | python3 -c '
import hashlib, sys
d = sys.stdin.buffer.read()
parts = d.split(b"\n", 3)
w, h = map(int, parts[1].split())
px = parts[3]
crop = hashlib.sha256()
for y in range(h):
    crop.update(px[3 * w * y:3 * (w * y + 224)])
print(crop.hexdigest()[:16])
' 2>/dev/null || echo none
}
# In the foreground (launch.rs): a run without --bench detaches from its terminal otherwise.
TMLS_RS_FOREGROUND=1 TMLS_RS_POLL_S=60 "$BIN" 2>"$OUT/app2.stderr" &
APP_PID=$!
if wait_for 30 window_up; then pass "the second run's window opened (TMLS_RS_POLL_S=60)"; else fail "the second run's window opened (TMLS_RS_POLL_S=60)"; fi
sleep 3 # its first listing, at start; the next one is a minute away
push_event PermissionRequest ',"tool":"Bash"'
if wait_for 4 grep -qxF -- "--|tmls · $(hostname)|? $PUSH · permission prompt|" "$NOTIFY_LOG"; then
  pass "a PermissionRequest event alerts within seconds, with no poll ('? $PUSH · permission prompt')"
else
  fail "a PermissionRequest event alerts within seconds, with no poll" "log: $(tail -n +"$((PUSH_START_NOTIFIED + 1))" "$NOTIFY_LOG" | tr '\n' ' ')"
fi
check "screenshot 39-push-waiting" shot 39-push-waiting
push_event Stop ',"msg":"first reply","bg":0'
sleep 1.5
REPLY_BEFORE=$(list_picture)
push_event Stop ',"msg":"tmls-push-reply-two","bg":0'
list_changed() { [ "$(list_picture)" != "$REPLY_BEFORE" ]; }
if [ "$REPLY_BEFORE" != none ] && wait_for 3 list_changed; then
  pass "a Stop event's reply changes the row's dim line, with no poll"
else
  fail "a Stop event's reply changes the row's dim line, with no poll" "the list's pixels did not change"
fi
check "screenshot 40-push-reply (the dim line is the reply)" shot 40-push-reply
key_quit
wait_for 10 app_gone || kill "$APP_PID" 2>/dev/null
if grep -nE "panicked|stack backtrace" "$OUT/app2.stderr" >/dev/null; then
  fail "no panic on the second run's stderr" "$(grep -n 'panicked' -A3 "$OUT/app2.stderr" | head -6 | tr '\n' ' ')"
else
  pass "no panic on the second run's stderr"
fi

# --- a third run: a remote host, game mode (G2), sign-in (G6), other tmux servers (G5) ----------
# Its own tmux socket directory (no default server: this machine lists no session) and, first on
# PATH, stand-ins for ssh (every call logged, every sign-in refused), game-mode (says on) and the
# terminal (logs its arguments). TMLS_RS_FOREGROUND: without --bench the app would detach.
THIRD_TMUX="$SCRATCH/tmux3"
mkdir -p "$THIRD_TMUX" "$OUT/bin3" && chmod 700 "$THIRD_TMUX"
SSH_LOG="$OUT/ssh3.log"
TERM_LOG="$OUT/term3.log"
: >"$SSH_LOG"
: >"$TERM_LOG"
cat >"$OUT/bin3/ssh" <<EOF
#!/bin/sh
echo "\$*" >>"$SSH_LOG"
echo 'tmlsrs@signhost: Permission denied (publickey,keyboard-interactive).' >&2
exit 255
EOF
cat >"$OUT/bin3/game-mode" <<'EOF'
#!/bin/sh
[ "$1" = status ] && echo on
EOF
cat >"$OUT/bin3/fake-term" <<EOF
#!/bin/sh
{ printf '%s|' "\$@"; echo; } >>"$TERM_LOG"
EOF
chmod +x "$OUT/bin3/ssh" "$OUT/bin3/game-mode" "$OUT/bin3/fake-term"
ssh_calls() { wc -l <"$SSH_LOG"; }
PATH="$OUT/bin3:$PATH" TMUX_TMPDIR="$THIRD_TMUX" TERMINAL="$OUT/bin3/fake-term" TMLS_RS_FOREGROUND=1 \
  "$BIN" signhost 2>"$OUT/app3.stderr" &
APP_PID=$!
if wait_for 30 window_up; then
  pass "the third run's window opened (signhost, game mode on)"
else
  fail "the third run's window opened (signhost, game mode on)" "stderr: $(tail -3 "$OUT/app3.stderr" | tr '\n' ' ')"
fi
if wait_for 10 grep -qF 'tmls-rs: game mode on' "$OUT/app3.stderr"; then
  pass "game-mode status says on: the app is in game mode"
else
  fail "game-mode status says on: the app is in game mode" "stderr: $(tail -3 "$OUT/app3.stderr" | tr '\n' ' ')"
fi
sleep 2
CALLS=$(ssh_calls)
sleep 12
if [ "$(ssh_calls)" -eq "$CALLS" ]; then
  pass "in game mode the remote host is not listed again within 12 s (without it: every 5 s)"
else
  fail "in game mode the remote host is not listed again within 12 s" "ssh calls $CALLS -> $(ssh_calls): $(tail -3 "$SSH_LOG" | tr '\n' '|')"
fi
check "screenshot 44-third-run (SIGNHOST ⚠ sign in)" shot 44-third-run
# The list: line 0 this machine's header, line 1 the gap, line 2 SIGNHOST · ⚠ sign in (y 65).
# The terminal is the stand-in: it logs its arguments and starts nothing.
if click 100 65 && wait_for 5 grep -qF -- "|-t|signhost|true|" "$TERM_LOG"; then
  pass "a click on ⚠ sign in opens a terminal running ssh -t signhost true"
else
  fail "a click on ⚠ sign in opens a terminal running ssh -t signhost true" "terminal log: $(tr '\n' ' ' <"$TERM_LOG")"
fi
check "the sign-in terminal is given ssh after -e" grep -qF -- "-e|ssh|" "$TERM_LOG"
check "screenshot 45-sign-in (the status line says sign in to signhost)" shot 45-sign-in
# --- third run: more checks above this line ---
key_quit
wait_for 10 app_gone || kill "$APP_PID" 2>/dev/null
if grep -nE "panicked|stack backtrace" "$OUT/app3.stderr" >/dev/null; then
  fail "no panic on the third run's stderr" "$(grep -n 'panicked' -A3 "$OUT/app3.stderr" | head -6 | tr '\n' ' ')"
else
  pass "no panic on the third run's stderr"
fi

echo "--- files under the scratch HOME"
find "$HOME" -type f | sed 's/^/    /' | head -20
echo "--- app stderr ($OUT/app.stderr)"
sed 's/^/    /' "$OUT/app.stderr" | head -40
echo "== $FAILED of $N checks failed"
[ "$FAILED" -eq 0 ]
