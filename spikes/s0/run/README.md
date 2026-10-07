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
`NAV` (override the tmls nav spec), `RUN_ID` (default UTC `YYYYmmdd-HHMMSS`), `PCL` (`pcl`).

A path that fails does not stop the matrix: it gets a line under "Failed / notes" in
`summary.md` and the run continues. If `s0 send` exits non-zero but both logs exist, the row is
still reported (missing keys are a result, not a harness error) and a note records the exit code.

## Where logs land

- archbox: `/data/scratch/s0/runs/<RUN_ID>/`
  - `<path>-<load>.send.jsonl`, `.recv.jsonl`, `.report.json`, `.send.stderr` (screen dumps on timeout)
  - `summary.md` — header + one row per path, then failures/notes
  - `tmls-nav-dump.stderr`, `tmls-nav.txt` — the tmls row-detection dump and the nav it produced
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

Rules the script keeps: `--quit-after $EVENTS` on every recv; the only tmux session it ever
kills is `s0-recv` (default server or `-L s0`), never `kill-server`, never the user's sessions.
All `s0 send` children run with `TMUX`/`TMUX_PANE` unset, so tmls/tmux inside the harness PTY
never act on the operator's own tmux client.

### tmls nav (row detection)

Before the first tmls path, the script runs tmls once with
`--dump-screen --events 1 --ready-timeout-ms 3000 --nav wait:6000` (times out by design), takes
the lines between `│ archbox` and ` ▸ background`, keeps session rows (`^  [^ ]` — Claude sub-lines
are indented four spaces), and counts the rows above `s0-recv` = k. tmls starts with **no cursor**
— the first `j` lands on row 0 — so the nav uses k+1 presses:
`wait:4000,expect:s0-recv:15000,key:j×(k+1),key:enter,wait:2000`.
If `s0-recv` is not found the script prints the dump and fails that path; override with `NAV=…`.
Safety: `s0 send` sends no events until it sees `S0READY`, so a wrong row only costs a timeout.

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

# P6-kitty-tmls — through tmls on archbox. First create the recv session on archbox:
ssh archbox "tmux new-session -d -s s0-recv -x 200 -y 50 '/home/sgkayast/Others/tmls-rs/target/release/s0 recv --log /data/scratch/s0/runs/P6-$R-kitty-tmls.recv.jsonl --quit-after $N'"
#   NAV: one key:j per session row up to and including s0-recv (4 with Queue/drive/home/s0-recv).
s0 send --log $D/P6-kitty-tmls.send.jsonl --events $N --rate 50 \
  --nav 'wait:4000,expect:s0-recv:15000,key:j,key:j,key:j,key:j,key:enter,wait:2000' -- \
  ssh -t archbox "bash -lc 'cd ~/Others/tmls && uv run -q tmls archbox'"
ssh archbox 'tmux kill-session -t s0-recv 2>/dev/null || true'
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

## Known result from the dry runs (2026-10-07, EVENTS=200)

All three tmls paths lose the stream at the same point: 59/200 arrive, 141 missing, 8 corrupt.
- Every `AltLetter` event (`ESC <letter>`) arrives re-encoded as caret text (`^a` = `5e61`), or
  in one case as `ESC[1;5C` → the 8 "corrupt".
- Event 59 is `^P` (Ctrl+P): Textual's default command-palette binding. tmls swallows it and every
  later key goes to the palette/UI instead of the session → everything after is "missing".
The non-tmls paths (P1–P3, local/ssh/tmux, quiet and flood) lose nothing.
