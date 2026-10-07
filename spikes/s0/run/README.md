# S0 path runner

Runs the S0 key-loss harness (`s0 send` → path → `s0 recv`) over each terminal path, then
joins the logs with `s0 report` into one Markdown table per run.

## Run

```bash
cd ~/Others/tmls-rs
spikes/s0/run/deploy.sh                      # build on archbox + copy to pcl:~/.local/bin/s0
spikes/s0/run/paths.sh quiet                 # full matrix, EVENTS=10000 RATE=50
spikes/s0/run/paths.sh flood                 # same, recv also floods stdout at 500000 B/s
EVENTS=200 PATHS="P1-local P2-ssh-loop" spikes/s0/run/paths.sh quiet   # quick subset
```

Env: `EVENTS` (10000), `RATE` Hz (50), `PATHS` (space-separated subset, default all),
`NAV` (override the tmls nav spec), `RUN_ID` (default UTC `YYYYmmdd-HHMMSS`), `PCL` (`pcl`),
`MARKER_GAP_MS` (0; passed as `--marker-gap-ms` to every `s0 send`), `SSH_FLAGS` (`-t`; the ssh
flags of the two P2 paths only). See [Controls](#controls).

Under `flood` the receiver starts flooding only **after the first event arrives**: until then the
screen stays quiet, so `S0READY` is still visible when tmls/tmux attaches to the session.
`--quit-after N` (N = `EVENTS`) stops the receiver once every seq in `0..N` has arrived at least
once; duplicates and out-of-range markers are logged but do not count.

## Reading a row

Columns: `events | arrived | missing | dup | corrupt | reorder | acked | sent | recv_exit |
saw_done | dropped_acks | one-way p50/p95/p99/max | ack p50/p95/p99/max | complete`.

- **`complete`** — `yes` only when `sent == events`, the receiver exited `quit_after` (not
  `eof`/`error`) and every log line parsed; otherwise `NO:<reasons>` (`sent<events`,
  `recv_exit=<x>`, `unparsed=<n>`). Treat only `complete = yes` rows as gate results.
  `saw_done` is **not** part of the rule: the receiver writes and flushes its done line
  (`exit=quit_after`) *before* it prints `S0DONE`, so `recv_exit = quit_after` already proves the
  receiver log is complete. `saw_done` (did `S0DONE` reach the sender's screen — often not through
  tmls) stays as its own column. (Reports from binaries before this change had a `no_done` reason.)
- `s0 report --events` defaults to the `events` in the send done line (what the sender was asked
  to send), else the number of send records. The `.report.json` has `unparsed_lines` (both logs).
- Loss/corruption come from the **receiver log** (`arrived`/`missing`/`corrupt` count distinct seqs
  in `0..events`; out-of-range seqs go to `out_of_range` in the `.report.json`).
- `acked` and the ack percentiles are *observed return-output timing* (ack text scanned from the
  path's output stream), not a delivery oracle; under flood, acks share the receiver's stdout queue
  and some are dropped (`dropped_acks`), so the ack sample is smaller than `events`.
- The send done line carries `done_seen_ms` (last write → `S0DONE` seen) and `marker_gap_ms`.
  After the last event the sender drains acks up to `--settle-ms` (5 s); once every event is acked
  it waits at most 1 s more for `S0DONE` (if acks are still missing, up to another `--settle-ms`)
  before killing the path. On tmls paths `S0DONE` may never be on screen; that shows as
  `saw_done = false` and does not make the row incomplete.

A path that fails does not stop the matrix: it gets a line under "Failed / notes" in
`summary.md` and the run continues. If `s0 send` exits non-zero but both logs exist, the row is
still reported (missing keys are a result, not a harness error) and a note records the exit code.

## Where logs land

- archbox: `/data/scratch/s0/runs/<RUN_ID>/`
  - `<path>-<load>.send.jsonl`, `.recv.jsonl`, `.report.json`, `.send.stderr` (screen dumps on timeout)
  - `summary.md` — header + one row per path, then failures/notes
  - `<path>-<load>.tmls-nav-dump.stderr`, `<path>-<load>.tmls-nav.txt` — per tmls path, the
    row-detection dump and the nav it produced (debug artefacts; never read back)
- pcl: `~/s0-logs/runs/<RUN_ID>/` (pcl has no `/data`; `deploy.sh` falls back to `~/s0-logs`, and
  `paths.sh` uses `/data/scratch/s0` there only if it exists and is writable). Remote logs are
  fetched back to the archbox run dir with `scp` after each path.

`TMPDIR=/data/scratch/s0` is exported by both scripts; nothing is written under `/tmp`.

## Paths

| path id | sender runs on | PTY child command |
|---|---|---|
| `P1-local` | archbox | `s0 recv --log <l> --quit-after N [--flood F]` |
| `P2-ssh-loop` | archbox | `ssh -t archbox s0 recv …` (loopback: ssh+pty hop only) |
| `P2-ssh-pcl` | archbox | `ssh -t pcl ~/.local/bin/s0 recv …` |
| `P3-tmux-local` | archbox | `tmux -L s0 new-session -s s0-recv "s0 recv …"` (private socket `-L s0`) |
| `P3-ssh-tmux-pcl` | archbox | `ssh -t pcl 'tmux -L s0 new-session -s s0-recv "~/.local/bin/s0 recv …"'` |
| `P4-tmls-local` | archbox | pre: `tmux new-session -d -s s0-recv -x 200 -y 50 "s0 recv …"` on the **default** server (so tmls lists it); child: `bash -lc 'cd ~/Others/tmls && uv run -q tmls archbox'` with the detected nav; after: `tmux kill-session -t s0-recv` |
| `P5-tmls-ssh-loop` | archbox | same pre; child: `ssh -t archbox "bash -lc 'cd ~/Others/tmls && uv run -q tmls archbox'"` |
| `P5-tmls-ssh-pcl` | **pcl** (`ssh pcl ~/.local/bin/s0 send … -- ssh -t archbox "bash -lc '…tmls archbox'"`) — the laptop path minus Windows | same pre |

Rules the script keeps: `--quit-after $EVENTS` and `--ready-token <run nonce>` on every recv,
the same `--ready-token` on every send; the only tmux session it ever
kills is `s0-recv` (default server or `-L s0`), never `kill-server`, never the user's sessions.
All `s0 send` children run with `TMUX`/`TMUX_PANE` unset, so tmls/tmux inside the harness PTY
never act on the operator's own tmux client.

### tmls nav (row detection)

Before **each** tmls path (no caching across paths — the session list can change), the script runs
tmls once with
`--dump-screen --events 1 --ready-timeout-ms 3000 --nav wait:6000` (times out by design), takes
the lines between `│ archbox` and ` ▸ background`, keeps session rows (`^  [^ ]` — Claude sub-lines
are indented four spaces), and counts the rows above `s0-recv` = k. tmls starts with **no cursor**
— the first `j` lands on row 0 — so the nav uses k+1 presses:
`wait:4000,expect:s0-recv:15000,key:j×(k+1),key:enter,wait:2000`.
If `s0-recv` is not found the script prints the dump and fails that path; override with `NAV=…`.
The awk heuristic assumes the session list is **unscrolled** (every row from the first session
down to `s0-recv` is on screen in the 120×40 PTY). With enough sessions/sub-lines to scroll the
list, the count is wrong — set `NAV=` by hand.
Safety (ready gate): each run draws a random nonce; every `s0 recv` prints `S0READY:<nonce>` and
every `s0 send` gets `--ready-token <nonce>`, so it types events only after it sees that exact text
(a plain `S0READY`, or another run's nonce, does not count) **in output that arrived after its last
nav key**. If the nav lands on the wrong row — including one of your own tmux sessions — that
screen cannot show the nonce, so the nav keys themselves (`j`…, `enter`) reach that session but no
events do; the path fails on the ready timeout. The nav keys are still typed blind: check the dump
when a tmls path times out.

## Controls

Re-run a subset with one variable changed; each run gets its own `RUN_ID` dir and its settings in
the `summary.md` title line.

| control | command | what it isolates |
|---|---|---|
| rate sweep | `RATE=10 …`, `RATE=100 …`, `RATE=200 … spikes/s0/run/paths.sh quiet` | loss that depends on inter-key interval (Textual/tmux batching, ESC timeouts) |
| no remote PTY | `SSH_FLAGS=-T PATHS="P2-ssh-loop P2-ssh-pcl" spikes/s0/run/paths.sh quiet` | ssh's remote PTY/line discipline vs plain byte transport |
| marker gap | `MARKER_GAP_MS=30 PATHS="P4-tmls-local P5-tmls-ssh-loop" spikes/s0/run/paths.sh quiet` | Textual ESC-sequence accumulation: with no gap, `ESC a` and `#000002` can land in one read and be parsed as one unknown sequence |

Notes:
- `SSH_FLAGS=-T`: ssh requests no remote PTY and therefore leaves the local tty cooked (ICANON would
  hold every key until a newline and `^U`/`^W` would edit them). The P2 child becomes
  `sh -c 'stty raw -echo && exec "$@"' sh ssh -T …`, so the harness PTY is raw like it is under
  `-t`. The remote `s0 recv` then reads a pipe (no termios) — the bytes it logs are exactly what
  ssh carried. Checked locally with `cat | s0 recv` standing in for ssh: 200/200 with the raw
  wrapper, 0/200 without it.
- `MARKER_GAP_MS` lengthens every event by the gap: at `RATE=50` (20 ms period) a 30 ms gap means
  the sender falls behind and rebases, so `effective_rate` in the send done line drops to ~30 Hz.
  Lower `RATE` (e.g. `RATE=20`) to keep the schedule.

## Clock skew caveat

One-way latency = recv wall clock − send wall clock. For `P2-ssh-pcl`, `P3-ssh-tmux-pcl` and
`P5-tmls-ssh-pcl` the two clocks are on different hosts. Both hosts report
`timedatectl show` → `NTPSynchronized=yes` (checked 2026-10-07), but NTP sync still leaves
millisecond-level offsets; those rows carry the note *"archbox↔pcl both NTP-synced; one-way
numbers cross-host carry clock skew; ack RTT is single-clock"*. Trust the **ack RTT** columns for
cross-host comparisons (in the 2026-10-07 dry runs, cross-host one-way p50 came out ~2 ms *above*
ack RTT p50 — that is skew, not physics).

## Morning run: native kitty paths (P6, user-run on the PC)

These exercise a real GUI terminal's PTY (kitty on pcl) + ssh. Open kitty on the PC (a normal
window, any size ≥ 120×40) and run:

```bash
R=$(date -u +%Y%m%d-%H%M%S); D=~/s0-logs/runs/$R; mkdir -p $D
N=10000

# P6-kitty-local — kitty PTY → s0 send → PTY → s0 recv, all on pcl
s0 send --log $D/P6-kitty-local.send.jsonl --events $N --rate 50 -- \
  s0 recv --log $D/P6-kitty-local.recv.jsonl --quit-after $N

# P6-kitty-ssh — over ssh to archbox (recv log lands on archbox)
s0 send --log $D/P6-kitty-ssh.send.jsonl --events $N --rate 50 -- \
  ssh -t archbox /home/sgkayast/Others/tmls-rs/target/release/s0 recv \
    --log /data/scratch/s0/runs/P6-$R-kitty-ssh.recv.jsonl --quit-after $N

# P6-kitty-tmls — through tmls on archbox. Pick a nonce, create the recv session on archbox:
T=$(head -c8 /dev/urandom | od -An -tx1 | tr -d ' \n')
ssh archbox "tmux new-session -d -s s0-recv -x 200 -y 50 '/home/sgkayast/Others/tmls-rs/target/release/s0 recv --log /data/scratch/s0/runs/P6-$R-kitty-tmls.recv.jsonl --quit-after $N --ready-token $T'"
#   NAV: find the row first — do NOT reuse a fixed count. Dump the tmls screen (times out by design):
s0 send --log $D/nav-dump.send.jsonl --events 1 --cols 120 --rows 40 --dump-screen \
  --ready-timeout-ms 3000 --nav 'wait:6000' -- ssh -t archbox "bash -lc 'cd ~/Others/tmls && uv run -q tmls archbox'" \
  2>&1 | sed -n '/│ archbox/,/▸ background/p'
#   Count the session rows (lines starting with two spaces then text; Claude sub-lines are indented
#   four) from the top down to and including s0-recv = K. tmls starts with no cursor, so the nav
#   needs K `key:j` (row index k = K-1 needs k+1 presses). Build it:
K=<count>; J=$(for i in $(seq $K); do printf 'key:j,'; done)
s0 send --log $D/P6-kitty-tmls.send.jsonl --events $N --rate 50 --ready-token $T \
  --nav "wait:4000,expect:s0-recv:15000,${J}key:enter,wait:2000" -- \
  ssh -t archbox "bash -lc 'cd ~/Others/tmls && uv run -q tmls archbox'"
ssh archbox 'tmux kill-session -t s0-recv 2>/dev/null || true'
#   A wrong K only costs a ready timeout: the gate waits for S0READY:$T after the last nav key,
#   which no other screen shows (the j/enter keys themselves do reach whatever row they select).
```

Then report on archbox (pull the pcl send logs first):

```bash
R=<the run id>; O=/data/scratch/s0/runs/P6-$R; mkdir -p $O
scp "pcl:s0-logs/runs/$R/*" $O/
S0=~/Others/tmls-rs/target/release/s0
$S0 report --header --path P6-kitty-local --load quiet --send $O/P6-kitty-local.send.jsonl --recv $O/P6-kitty-local.recv.jsonl
$S0 report --path P6-kitty-ssh  --load quiet --send $O/P6-kitty-ssh.send.jsonl  --recv /data/scratch/s0/runs/P6-$R-kitty-ssh.recv.jsonl  --skew-note "pcl→archbox cross-host one-way"
$S0 report --path P6-kitty-tmls --load quiet --send $O/P6-kitty-tmls.send.jsonl --recv /data/scratch/s0/runs/P6-$R-kitty-tmls.recv.jsonl --skew-note "pcl→archbox cross-host one-way"
```

Note: `s0 send` still owns its own PTY inside kitty, so these measure kitty-hosted process + ssh,
not kitty's keyboard handling. True keyboard-to-app latency additionally needs `ydotool type`
into the focused kitty window feeding a bare `s0 recv`; that is a user-run step (needs the
desktop session) and is not scripted here.

## Known result from the dry runs (2026-10-07, EVENTS=200, alphabet before d69ca3f)

All three tmls paths lose the stream at the same point: 59/200 arrive, 141 missing, 8 corrupt.
- Every `AltLetter` event (`ESC <letter>`) arrives re-encoded as caret text (`^a` = `5e61`), or
  in one case as `ESC[1;5C` → the 8 "corrupt".
- Event 59 is `^P` (Ctrl+P): Textual's default command-palette binding. tmls swallows it and every
  later key goes to the palette/UI instead of the session → everything after is "missing".
The non-tmls paths (P1–P3, local/ssh/tmux, quiet and flood) lose nothing.
