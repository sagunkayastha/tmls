#!/usr/bin/env bash
# Record raw PTY output streams of real terminal programs (spike S1).
# Usage: record.sh [name ...]   (default: all)
# Output: /data/scratch/s1/corpus/<name>.bin + <name>.meta.json; <200KB copied next to this script.
set -euo pipefail

REPO=/home/sgkayast/Others/tmls-rs
OUT=/data/scratch/s1/corpus
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOCK=s1rec
COLS=120
ROWS=40
export TMPDIR=/data/scratch/s1
mkdir -p "$OUT" "$TMPDIR"
STATUS="$OUT/status.tsv"
: > "$STATUS"

T() { tmux -L "$SOCK" "$@"; }

# start NAME CMD : fresh private tmux server, pipe-pane attached before CMD paints.
start() {
  local name=$1 cmd=$2
  : > "$OUT/$name.bin"
  tmux -L "$SOCK" kill-server 2>/dev/null || true
  T new-session -d -x $COLS -y $ROWS -s rec -c "${CWD:-$REPO}" \
    "env TERM=xterm-256color COLORTERM=truecolor TMPDIR=$TMPDIR sh -c 'sleep 1; $cmd'"
  T pipe-pane -o -t rec "cat >> $OUT/$name.bin"
}
alive() { T has-session -t rec 2>/dev/null; }
# waitdone SECS : wait until the session ends on its own
waitdone() { local i; for ((i=0; i<$1*5; i++)); do alive || return 0; sleep 0.2; done; return 0; }
keys() { T send-keys -t rec "$@"; sleep "${KD:-0.6}"; }
lit()  { T send-keys -t rec -l "$1"; sleep "${KD:-0.6}"; }
finish() {
  local name=$1 cmd=$2
  sleep 0.5
  T kill-session -t rec 2>/dev/null || true
  tmux -L "$SOCK" kill-server 2>/dev/null || true
  python3 - "$OUT" "$name" "$cmd" "$COLS" "$ROWS" <<'PY'
import sys, json, hashlib
out, name, cmd, cols, rows = sys.argv[1:]
b = open(f"{out}/{name}.bin", "rb").read()
json.dump({"cmd": cmd, "cols": int(cols), "rows": int(rows), "bytes": len(b),
           "sha256": hashlib.sha256(b).hexdigest()}, open(f"{out}/{name}.meta.json", "w"))
PY
}

rec_vim() {
  local c="vim spikes/s0/src/send.rs"
  start vim "$c"; sleep 2.5
  lit ':set nu'; keys Enter
  keys C-f; keys C-f; keys C-f
  lit '/fn '; keys Enter; keys n
  lit ':q!'; keys Enter; waitdone 5
  finish vim "$c"
}
rec_btop() {
  local c="btop"
  start btop "$c"; sleep 6
  keys q; waitdone 5
  finish btop "$c"
}
rec_less() {
  local c="less docs/spikes/S0-results.md"
  start less "$c"; sleep 2
  keys G; keys g; lit '/Result'; keys Enter; keys n; keys q; waitdone 5
  finish less "$c"
}
rec_cargo() {
  # private target dir (not the shared ./target) so the build is a full, deps-included one
  # and does not collide with other work. Re-recording a full build needs a NEW dir:
  #   S1_CARGO_TARGET=/data/scratch/s1/cargo-target-2 record.sh cargo
  local c="CARGO_TARGET_DIR=${S1_CARGO_TARGET:-/data/scratch/s1/cargo-target} cargo build -p s0 --color always"
  touch "$REPO/spikes/s0/src/main.rs"
  start cargo "$c"; waitdone 180
  finish cargo "$c"
}
rec_gitlog() {
  local c="git log --color=always -p -5 | cat"
  start gitlog "$c"; waitdone 20
  finish gitlog "$c"
}
rec_nested() {
  local c="env -u TMUX tmux -L s1b new-session -s inner \; split-window -h \; send-keys 'ls --color=always -la spikes/s0/src' Enter \; select-pane -L \; send-keys 'git -C /home/sgkayast/Others/tmls-rs status' Enter"
  start nested "$c"; sleep 4
  # extra traffic in both inner panes so the stream is >= 4 KB
  tmux -L s1b send-keys -t inner:0.0 'git -C /home/sgkayast/Others/tmls-rs log --oneline --color=always | head -30' Enter 2>/dev/null || true
  tmux -L s1b send-keys -t inner:0.1 'ls --color=always -laR docs spikes | head -60' Enter 2>/dev/null || true
  sleep 2
  tmux -L s1b kill-server 2>/dev/null || true
  waitdone 5
  finish nested "$c"
}
rec_unicode() {
  local py="$HERE/unicode_gen.py"
  local c="python3 $py"
  cat > "$py" <<'PY'
import sys, time
w = sys.stdout.write
w("\x1b[?2004h")                       # bracketed paste on
w("plain header\r\n")
for i in range(0, 101, 5):
    w("\rprogress [%-40s] %3d%%" % ("#" * (i * 40 // 100), i)); sys.stdout.flush(); time.sleep(0.02)
w("\r\n")
w("CJK: 中文宽字符测试 日本語のテキスト 한국어 안녕하세요\r\n")
for _ in range(8):
    w("ＡＢＣ こんにちは世界 " * 4 + "\r\n")
w("emoji ZWJ: \U0001F468‍\U0001F469‍\U0001F467‍\U0001F466 \U0001F469‍\U0001F4BB \U0001F3F3️‍\U0001F308 \U0001F44D\U0001F3FD \U0001F1EF\U0001F1F5\r\n")
w("combining: é ä ñ ộ नमस्ते z̖̗͡ͅ\r\n")
for i in range(0, 256, 1):
    w("\x1b[48;5;%dm  " % i)
    if i % 32 == 31: w("\x1b[0m\r\n")
w("\x1b[0m")
for i in range(0, 120):
    w("\x1b[38;2;%d;%d;%dm█" % (255 - i * 2, i * 2, 128))
w("\x1b[0m\r\n")
w("\x1b[1mbold\x1b[0m \x1b[2mdim\x1b[0m \x1b[3mital\x1b[0m \x1b[4munder\x1b[0m \x1b[7mrev\x1b[0m \x1b[9mstrike\x1b[0m \x1b[4:3m\x1b[58;2;255;0;0mcurly\x1b[0m\r\n")
w("\x1b]8;;https://example.com/a\x1b\\link one\x1b]8;;\x1b\\ and \x1b]8;id=x;https://example.com/b\x07link two\x1b]8;;\x07\r\n")
w("\x1b]0;window title\x07")
w("\x1b[?1049h\x1b[2J\x1b[H")          # alt screen
for r in range(1, 41):
    w("\x1b[%d;1H\x1b[3%dmalt row %02d %s" % (r, r % 8, r, "x" * (r * 2)))
w("\x1b[20;60H\x1b[1;44;33m centre \x1b[0m")
sys.stdout.flush(); time.sleep(0.3)
w("\x1b[?1049l")                       # leave alt screen
w("\x1b[?2004l")                       # bracketed paste off
w("after alt screen\r\n")
for i in range(60):
    w("\x1b[%dmline %d ─│┌┐└┘ \x1b[0m\r\n" % (31 + i % 7, i))
PY
  start unicode "$c"; waitdone 20
  finish unicode "$c"
}
rec_claude() {
  local c="claude"
  CWD=/home/sgkayast   # trusted folder (the repo dir shows a trust dialog)
  start claude "$c"
  local i ok=0
  for ((i=0; i<20; i++)); do
    sleep 1
    if T capture-pane -p -t rec 2>/dev/null | grep -qiE 'trust this folder|do you trust|log ?in|sign in|select login'; then
      echo "claude needs interactive trust/login" >&2; T capture-pane -p -t rec > "$OUT/claude.pane.txt" || true
      finish claude "$c"; return 1
    fi
    if T capture-pane -p -t rec 2>/dev/null | grep -qE '^[[:space:]]*[│|]?[[:space:]]*>|❯|\? for shortcuts'; then ok=1; break; fi
  done
  [ $ok = 1 ] || { T capture-pane -p -t rec > "$OUT/claude.pane.txt" 2>/dev/null || true; finish claude "$c"; echo "no prompt UI within 20s" >&2; return 1; }
  sleep 2
  lit 'say hi'; keys Enter; sleep 30
  lit '/exit'; keys Enter; sleep 5
  finish claude "$c"
}

ALL=(vim btop less cargo gitlog nested unicode claude)
SEL=("$@"); [ ${#SEL[@]} -gt 0 ] || SEL=("${ALL[@]}")
for n in "${SEL[@]}"; do
  echo "== $n"
  if ( "rec_$n" ); then
    printf '%s\tOK\n' "$n" >> "$STATUS"
  else
    printf '%s\tFAILED\n' "$n" >> "$STATUS"
    tmux -L "$SOCK" kill-server 2>/dev/null || true
  fi
done

echo; printf '%-10s %10s  %s\n' name bytes sha256
for m in "$OUT"/*.meta.json; do
  python3 - "$m" <<'PY'
import json, sys, os
m = json.load(open(sys.argv[1])); n = os.path.basename(sys.argv[1])[:-10]
print("%-10s %10d  %s%s" % (n, m["bytes"], m["sha256"][:12], "" if m["bytes"] >= 4096 else "  (<4KB!)"))
PY
done
for b in "$OUT"/*.bin; do
  grep -q "^$(basename "${b%.bin}")	OK" "$STATUS" || continue
  [ "$(stat -c %s "$b")" -lt 204800 ] && cp "$b" "${b%.bin}.meta.json" "$HERE/"
done
cat "$STATUS"
