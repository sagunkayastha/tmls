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
check_typed "the renamed tab takes typing" "$RENAMED" renamed
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
check_typed "after the kill the keys go to \"$ODD\"" "$ODD" afterkill
check "screenshot 28-killed" shot 28-killed

# --- the same by mouse; a session that ends from inside ---------------------------------------
# For the alerts checks further down: $ONE (no tab) prints something, so the next listing has
# it running; 30 s after that it counts as done, unwatched. Counted from here.
tmux send-keys -t "=$ONE:" "echo alert-$NONCE" Enter
sleep 7
NOTIFIED=$(notified)
# Right-click on "$ODD" (the list's second line): its menu at the pointer. Esc closes it.
PLAIN=$(picture)
click 100 28 right
check "screenshot 29-menu-by-mouse" shot 29-menu-by-mouse
if [ "$(picture)" != "$PLAIN" ]; then
  pass "a right-click on a session opens its menu (the picture changed)"
else
  fail "a right-click on a session opens its menu (the picture changed)" "same picture"
fi
keys -k Escape
check_typed "Esc closes the menu: the keys are the terminal's again" "$ODD" aftermenu
# A click on the `+` of the host's header opens the form; the session is made from it.
click 212 11
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
