#!/usr/bin/env bash
# One measured run of tmls-rs for the speed pass. Runs INSIDE the tmls-rs-smoke container (see
# bench-run.sh), with the setup of e2e.sh: a headless sway, software Vulkan, its own tmux
# server, keys through wtype, clicks through vptr.py, pictures by grim.
#
#   BENCH_STATE     closed | text | image | list   what the panel shows while the keys are typed
#   BENCH_WLDEBUG   1: run the app with WAYLAND_DEBUG=1 and count its wl_surface commits while
#                   idle (every presented frame is one; the latency numbers of such a run are
#                   not for the table: the protocol log costs time)
#   BENCH_KEY_MS    pause between two keys of a line (default 60)
#   BENCH_IMAGE_SIDE  the side in pixels of the square picture of the state `image` (default 4096)
#   SMOKE_BIN       the binary (default /work/target-docker/release/tmls-rs)
#   SMOKE_OUT       where the log, the marks and the screenshot go (default /out)
#
# The flow: start with --bench, wait for the window, attach one tab, open the panel as the
# state says, type three fixed lines one key at a time, stay idle for 10 s, close the panel,
# wait 3 s, quit. Along the way `marks.json` gets times of CLOCK_MONOTONIC (the clock of the
# latency log's `now_ns`), the resident memory from /proc/PID/status and the process's CPU
# ticks; bench-report.py reads it with bench/latency.jsonl.
#
# The exit status is 1 if a step did not happen (the panel did not open, a typed line did not
# arrive, the app did not quit): such a run is not a measurement.
set -u

OUT=${SMOKE_OUT:-/out}
SCRATCH=${SMOKE_SCRATCH:-/scratch}
BIN=${SMOKE_BIN:-/work/target-docker/release/tmls-rs}
STATE=${BENCH_STATE:-closed}
KEY_MS=${BENCH_KEY_MS:-60}
IMAGE_SIDE=${BENCH_IMAGE_SIDE:-4096}
ONE='tmlsrs-test-one'
# 56 keys and Enter; with the prompt `$ ` it fits the 60-odd columns the pane has beside the panel.
LINE='echo the quick brown fox jumps over the lazy dog 0123456'
LINES_TYPED=3
IDLE_SECS=10

case "$STATE" in closed | text | image | list) ;; *)
  echo "bench.sh: BENCH_STATE is closed, text, image or list" >&2
  exit 2
  ;;
esac
# The container must not be the user's machine: this script starts and stops a tmux server.
if [ ! -f /.dockerenv ]; then
  echo "bench.sh: runs in the tmls-rs-smoke container only (tooling/smoke/bench-run.sh)" >&2
  exit 2
fi

export HOME="$SCRATCH/home"
export TMUX_TMPDIR="$SCRATCH/tmux"
export XDG_RUNTIME_DIR="$SCRATCH/xdg"
export XDG_CONFIG_HOME="$HOME/.config"
export SHELL=/bin/bash LANG=C.UTF-8 LC_ALL=C.UTF-8 TERM=xterm-256color
unset TMUX DISPLAY WAYLAND_DISPLAY SSH_AUTH_SOCK
mkdir -p "$HOME/.config/tmls" "$TMUX_TMPDIR" "$XDG_RUNTIME_DIR" "$OUT/bench" &&
  chmod 700 "$XDG_RUNTIME_DIR" "$TMUX_TMPDIR" || {
  echo "bench.sh: cannot make the scratch directories under $SCRATCH and $OUT" >&2
  exit 2
}
: >"$OUT/app.stderr"
: >"$OUT/bench/latency.jsonl"
: >"$OUT/marks.txt"
: >"$OUT/vptr.log"

export VK_ICD_FILENAMES=${VK_ICD_FILENAMES:-/usr/share/vulkan/icd.d/lvp_icd.x86_64.json}
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
export RUST_BACKTRACE=1

FAILED=0
step() { printf '%s  %s\n' "$1" "$2"; }
ok() { step ok "$1"; }
bad() {
  FAILED=$((FAILED + 1))
  step FAIL "$1"
}
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
# As in e2e.sh: each wtype is a new virtual keyboard; `-s 250` waits until the app has bound it.
keys() {
  timeout 10 wtype -s 250 -d 30 "$@"
  sleep 0.3
}
click() {
  local status=0
  timeout 10 python3 /work/tooling/smoke/vptr.py "$@" 2>>"$OUT/vptr.log" || status=$?
  sleep 0.3
  return "$status"
}
shot() {
  sleep "${2:-0.7}"
  timeout 10 grim "$OUT/$1.png" 2>>"$OUT/grim.log"
}
picture() { timeout 10 grim -t ppm - 2>>"$OUT/grim.log" | sha256sum | cut -c1-16; }
# How many pixels of the screen are exactly #FF8800 (the 4096x4096 picture's colour).
orange() {
  timeout 10 grim -t ppm - 2>>"$OUT/grim.log" | python3 -c '
import sys
d = sys.stdin.buffer.read()
px = d.split(b"\n", 3)[3]
print(sum(1 for i in range(0, len(px), 3) if px[i:i+3] == b"\xff\x88\x00"))' 2>/dev/null || echo 0
}
orange_at_least() { [ "$(orange)" -ge "$1" ]; }
pane_text() { tmux capture-pane -p -t "=$ONE:" 2>&1; }
pane_has_line() { pane_text | grep -qxF -- "$1"; }
pane_cols() { tmux display -p -t "=$ONE:" '#{window_width}' 2>/dev/null; }
narrower_than() { [ "$(pane_cols)" -lt "$1" ]; }
as_wide_as() { [ "$(pane_cols)" -eq "$1" ]; }
attached_to() { tmux list-clients -F '#{session_name}' 2>/dev/null | grep -qxF -- "$1"; }

# --- marks -----------------------------------------------------------------------------------
# mark NAME VALUE  — one line of marks.txt; made into marks.json at the end.
mark() { printf '%s %s\n' "$1" "$2" >>"$OUT/marks.txt"; }
# CLOCK_MONOTONIC in ns: what `s2_core::now_ns` reads, and a container shares it with the host.
mono() { python3 -c 'import time; print(time.clock_gettime_ns(time.CLOCK_MONOTONIC))'; }
proc_kb() { awk -v k="$1:" '$1 == k { print $2 }' "/proc/$APP_PID/status" 2>/dev/null; }
# utime + stime of the app, in clock ticks.
cpu_ticks() { python3 -c '
import sys
rest = open("/proc/%s/stat" % sys.argv[1]).read().rsplit(")", 1)[1].split()
print(int(rest[11]) + int(rest[12]))' "$APP_PID" 2>/dev/null; }
# moment NAME  — the time, the resident memory, its peak so far and the CPU ticks, now.
moment() {
  mark "t_${1}_ns" "$(mono)"
  mark "rss_${1}_kb" "$(proc_kb VmRSS)"
  mark "hwm_${1}_kb" "$(proc_kb VmHWM)"
  mark "cpu_${1}_ticks" "$(cpu_ticks)"
}
commits() { grep -c 'wl_surface[#@][0-9]*\.commit(' "$OUT/app.stderr"; }

cleanup() {
  if app_alive; then
    kill "$APP_PID" 2>/dev/null
    wait_for 3 app_gone || kill -9 "$APP_PID" 2>/dev/null
  fi
  tmux kill-session -t "=$ONE" 2>/dev/null
  [ -n "${FAKE_CLAUDE:-}" ] && kill "$FAKE_CLAUDE" 2>/dev/null
  [ -n "${WATCH_PID:-}" ] && kill "$WATCH_PID" 2>/dev/null
  [ -n "${SWAY_PID:-}" ] && kill "$SWAY_PID" 2>/dev/null
  return 0
}
trap cleanup EXIT

echo "== tmls-rs bench: state $STATE  $(date -u +%FT%TZ)  bin $(sha256sum "$BIN" 2>/dev/null | cut -c1-12)  key pause $KEY_MS ms"
mark state "$STATE"
mark bin_sha "$(sha256sum "$BIN" 2>/dev/null | cut -c1-12)"
mark key_ms "$KEY_MS"
mark clk_tck "$(getconf CLK_TCK)"
mark load_before "$(cut -d' ' -f1 /proc/loadavg)"

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
  ok "headless compositor ($WAYLAND_DISPLAY)"
else
  bad "headless compositor: $(tail -3 "$OUT/sway.log" | tr '\n' ' ')"
  exit 1
fi

# --- one tmux session ---------------------------------------------------------------------------
# The status line's clock would draw a frame at each full minute: it is left out, and the line
# is not drawn again by a timer, so an idle pane changes nothing by itself.
tmux new-session -d -s "$ONE" -x 100 -y 30
tmux set -g status-right ''
tmux set -g status-interval 0

# A stand-in for notify-send, first on PATH (in $OUT: the scratch tmpfs is mounted noexec).
mkdir -p "$OUT/bin"
printf '#!/bin/sh\nexit 0\n' >"$OUT/bin/notify-send"
chmod +x "$OUT/bin/notify-send"
export PATH="$OUT/bin:$PATH"

# --- the files the states open -------------------------------------------------------------------
png() { # png PATH WIDTH HEIGHT  — one orange, written with python's zlib
  python3 - "$@" <<'PY'
import struct, sys, zlib
w, h = int(sys.argv[2]), int(sys.argv[3])
def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
rows = (b"\x00" + bytes((0xFF, 0x88, 0x00)) * w) * h
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY
}
VIEWED="$HOME/viewme.txt"
BIG="$HOME/big.png"
SID='0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'
case "$STATE" in
text) seq -f 'line %g of the viewed file' 1 200 >"$VIEWED" ;;
image)
  png "$BIG" "$IMAGE_SIDE" "$IMAGE_SIDE"
  mark image_side "$IMAGE_SIDE"
  mark image_bytes "$(stat -c %s "$BIG")"
  ;;
list)
  # 500 pictures of one pixel, and a transcript that names each of them.
  mkdir -p "$HOME/made" "$HOME/.claude/sessions" "$HOME/.claude/projects/-work"
  png "$HOME/made/img-001.png" 1 1
  for i in $(seq -w 2 500); do cp "$HOME/made/img-001.png" "$HOME/made/img-$i.png"; done
  for i in $(seq -w 1 500); do
    printf '{"type":"assistant","message":"saved %s"}\n' "$HOME/made/img-$i.png"
  done >"$HOME/.claude/projects/-work/$SID.jsonl"
  mark list_files "$(find "$HOME/made" -name '*.png' | wc -l)"
  ;;
esac

# --- the app -------------------------------------------------------------------------------------
# The first frame: sway tells of a new window when its first buffer is committed (the surface is
# mapped). A listener on sway's socket writes CLOCK_MONOTONIC at that event; the app is started
# by a python that writes the same clock and then becomes the app (exec), so the two are a
# launch time and a first-frame time on one clock.
python3 - "$SWAYSOCK" "$OUT/t_window_ns" "$OUT/watch.ready" <<'PY' &
import json, socket, struct, sys, time
MAGIC = b"i3-ipc"
def send(s, kind, payload):
    s.sendall(MAGIC + struct.pack("<II", len(payload), kind) + payload)
def read_exact(s, n):
    data = b""
    while len(data) < n:
        part = s.recv(n - len(data))
        if not part:
            raise SystemExit(1)
        data += part
    return data
def recv(s):
    head = read_exact(s, 14)
    size, kind = struct.unpack("<II", head[6:])
    return kind, read_exact(s, size)
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
send(s, 2, b'["window"]')  # SUBSCRIBE
recv(s)
open(sys.argv[3], "w").write("ready")
s.settimeout(60)
while True:
    kind, body = recv(s)
    now = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
    if kind == 0x80000003 and json.loads(body).get("change") == "new":
        open(sys.argv[2], "w").write(str(now))
        break
PY
WATCH_PID=$!
wait_for 10 test -s "$OUT/watch.ready" || bad "the listener for sway's window events is not ready"

[ "${BENCH_WLDEBUG:-0}" = 1 ] && export WAYLAND_DEBUG=1
python3 -c '
import os, sys, time
open(sys.argv[1], "w").write(str(time.clock_gettime_ns(time.CLOCK_MONOTONIC)))
os.execv(sys.argv[2], [sys.argv[2], "--bench", sys.argv[3]])' \
  "$OUT/t_launch_ns" "$BIN" "$OUT/bench" 2>"$OUT/app.stderr" &
APP_PID=$!
unset WAYLAND_DEBUG
if wait_for 30 window_up; then ok "the window opened"; else bad "the window opened: $(tail -5 "$OUT/app.stderr" | tr '\n' ' ')"; fi
wait_for 5 test -s "$OUT/t_window_ns"
mark t_launch_ns "$(cat "$OUT/t_launch_ns" 2>/dev/null)"
mark t_window_ns "$(cat "$OUT/t_window_ns" 2>/dev/null)"
mark wldebug "${BENCH_WLDEBUG:-0}"

# The first listing arrives right after start; the next one 5 s later.
sleep 3
moment start

# --- one tab -------------------------------------------------------------------------------------
# The list has the focus at start and its cursor is on the only session.
keys -k Return
if wait_for 8 attached_to "$ONE"; then ok "Enter attaches $ONE"; else bad "Enter attaches $ONE"; fi
sleep 1
COLS_BEFORE=$(pane_cols)
mark cols_closed "$COLS_BEFORE"

# --- the panel -----------------------------------------------------------------------------------
# ctrl_click_on PATH  — fills the pane with PATH and Ctrl+clicks on it (as e2e.sh does).
ctrl_click_on() {
  tmux send-keys -t "=$ONE:" "clear; for i in \$(seq 1 40); do echo $1; done" Enter
  wait_for 5 pane_has_line "$1"
  timeout 10 wtype -M ctrl -s 4000 -m ctrl &
  local hold=$!
  sleep 0.8
  click 300 300
  wait_for 8 narrower_than "$COLS_BEFORE"
  local status=$?
  wait "$hold" 2>/dev/null
  return "$status"
}
case "$STATE" in
closed) ok "no panel" ;;
text)
  if ctrl_click_on "$VIEWED"; then ok "the text panel is open ($COLS_BEFORE -> $(pane_cols) columns)"; else bad "the text panel is open"; fi
  ;;
image)
  if ctrl_click_on "$BIG" && wait_for 30 orange_at_least 10000; then
    ok "the picture is in the panel ($(orange) pixels of its orange, $COLS_BEFORE -> $(pane_cols) columns)"
  else
    bad "the picture is in the panel (orange pixels: $(orange), columns $(pane_cols))"
  fi
  ;;
list)
  keys -M alt -M shift -k f -m shift -m alt
  if wait_for 8 narrower_than "$COLS_BEFORE"; then ok "Alt+Shift+F opens the panel"; else bad "Alt+Shift+F opens the panel"; fi
  sleep 0.7
  NO_CLAUDE_PICTURE=$(picture)
  # The session becomes a Claude session for the listing (every 5 s): a status file named after
  # a live process that names the session's pane and the transcript's id.
  sleep 900 &
  FAKE_CLAUDE=$!
  printf '{"pid":%s,"sessionId":"%s","status":"idle","statusUpdatedAt":1,"tmux":"%s:@0.%%0","cwd":"/work"}\n' \
    "$FAKE_CLAUDE" "$SID" "$ONE" >"$HOME/.claude/sessions/$FAKE_CLAUDE.json"
  listed_now() { [ "$(picture)" != "$NO_CLAUDE_PICTURE" ]; }
  if wait_for 20 listed_now; then ok "the panel lists the session's images"; else bad "the panel lists the session's images"; fi
  sleep 2
  ;;
esac
mark cols_open "$(pane_cols)"

# The same prompt and an empty pane in every state; the keys are the terminal's.
tmux send-keys -t "=$ONE:" "PS1='\$ '; clear" Enter
sleep 1.5
shot state
moment open

# --- typing --------------------------------------------------------------------------------------
# One wtype a line, a pause of $KEY_MS ms after each key, then Enter by a wtype of its own.
: >"$OUT/typed.log"
mark t_type_start_ns "$(mono)"
for n in $(seq 1 "$LINES_TYPED"); do
  timeout 30 wtype -s 250 -d "$KEY_MS" "$LINE"
  sleep 0.3
  timeout 10 wtype -s 250 -k Return
  sleep 0.5
done
sleep 1
mark t_type_end_ns "$(mono)"
ARRIVED=$(pane_text | grep -cxF -- "${LINE#echo }")
mark lines_arrived "$ARRIVED"
if [ "$ARRIVED" -eq "$LINES_TYPED" ]; then ok "the $LINES_TYPED typed lines ran in the pane"; else bad "the typed lines ran in the pane: $ARRIVED of $LINES_TYPED"; fi
moment typed

# --- idle ----------------------------------------------------------------------------------------
sleep 1.5
[ "${BENCH_WLDEBUG:-0}" = 1 ] && mark commits_idle_start "$(commits)"
moment idle_start
sleep "$IDLE_SECS"
moment idle_end
[ "${BENCH_WLDEBUG:-0}" = 1 ] && mark commits_idle_end "$(commits)"

# --- close the panel -----------------------------------------------------------------------------
if [ "$STATE" != closed ]; then
  keys -M alt -M shift -k x -m shift -m alt
  if wait_for 8 as_wide_as "$COLS_BEFORE"; then ok "Alt+Shift+X closes the panel"; else bad "Alt+Shift+X closes the panel (columns $(pane_cols))"; fi
else
  sleep 0.6
fi
mark t_close_ns "$(mono)"
sleep 3
moment closed
shot closed 0

# --- quit ----------------------------------------------------------------------------------------
keys -M ctrl -k q -m ctrl
if wait_for 10 app_gone; then
  wait "$APP_PID"
  status=$?
  mark exit_status "$status"
  if [ "$status" -eq 0 ]; then ok "Ctrl+Q quits, exit status 0"; else bad "Ctrl+Q quits: exit status $status"; fi
else
  bad "Ctrl+Q quits: still running after 10 s"
fi
if grep -q "panicked" "$OUT/app.stderr"; then bad "a panic on stderr"; fi
mark load_after "$(cut -d' ' -f1 /proc/loadavg)"
mark failed "$FAILED"

python3 - "$OUT/marks.txt" "$OUT/marks.json" <<'PY'
import json, sys
marks = {}
for line in open(sys.argv[1]):
    name, _, value = line.rstrip("\n").partition(" ")
    try:
        marks[name] = int(value)
    except ValueError:
        try:
            marks[name] = float(value)
        except ValueError:
            marks[name] = value
json.dump(marks, open(sys.argv[2], "w"), indent=1, sort_keys=True)
PY
echo "== $FAILED steps failed"
[ "$FAILED" -eq 0 ]
