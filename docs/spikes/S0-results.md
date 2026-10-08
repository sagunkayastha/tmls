# S0 results — key loss / latency (2026-10-07/08)

Scope: **receiver-observed delivery.** Almost every row is scripted PTY input: a sender (`s0 send`) writes
numbered events into a PTY whose child is the path under test; a receiver (`s0 recv`) at the far end logs each
event it parses. The P6 rows re-run three of those paths from a kitty window on the PC (same scripted bytes). The
one real-keyboard row (K1) is plain letters only. No IME, Windows laptop or modifier/Alt/F-keys via a real
keyboard is in any row (see [Not measured](#not-measured--next-steps)).
Ack columns are **observed return-output timing** (ack text scanned from the path's output), never one-way latency.

## Summary

- **No scripted event was lost or duplicated on any 10k row**: 0 missing / 0 dup across P1–P5, quiet 50 Hz,
  quiet 15 Hz and 500 kB/s flood (10,000 events per row, alphabet without Ctrl+P; completeness in the tables).
- **tmls swallows all keys after Ctrl+P** (Textual command palette): from event 59 onward (141 missing incl.
  event 59, the `^P`) on each of P4, P5-loop and P5-pcl in the 200-event dry runs; Ctrl+P was then removed from the
  alphabet, so the 10k rows do not measure it. The palette attribution is not yet confirmed by a palette-disabled
  control.
- **tmls corrupts Alt+letter at 15/50 Hz**: 1249–1250/1250 `AltLetter` events arrive as literal `^`+letter
  (19 patterns) or, for Alt+f, as `ESC[1;5C`, on every 10k tmls row (P4, P5-loop, P5-pcl; quiet and flood);
  0 corrupt on every non-tmls row. At 5 Hz only Alt+f is corrupt (4/75), with or without a 120 ms marker gap
  (Alt+f → Ctrl+Right at every rate: `c-tmls-gap120-r50` `corrupt_examples` `expected_hex=1b66`,
  `got_hex=1b5b313b3543`).
- **tmls delays Alt+letter and the key after it**: P4 quiet one-way p50 `AltLetter` 45.6 / 140.1 / 102.4 ms at
  50 / 15 / 5 Hz, `CtrlLetter` (always next) 28.2 / 77.4 / 5.6 ms, vs 3.3–5.7 ms for Letter/Digit/FKey/Repeat/Paste.
  That Textual *holds* the pending ESC sequence and the next key's bytes *merge* into it is a **supported
  hypothesis**, not isolated: send rate and next-key interval co-vary in these runs, and CtrlLetter always follows
  AltLetter.
- **On the measured loopback interactive ssh path, `ObscureKeystrokeTiming` adds a uniform 0–~21 ms**:
  P2-ssh-loop one-way p50/p95/p99 10.6/19.5/20.7 ms (flat over 0–20 ms, every kind); with
  `-o ObscureKeystrokeTiming=no` 0.4/0.6/1.5 ms (2000/2000, loopback). P1-local 0.1/0.2/0.7 ms, P3-tmux-local
  0.3/0.5/1.3 ms. The archbox↔pcl ssh rows are *consistent with* the same spread (ack RTT p50 9.4–10.8 ms; their
  one-way numbers carry unrechecked clock skew); the on/off pair was not run across the LAN.
- **tmls return-output timing is far above tmux**: ack p50 37.2 ms (P4) vs 0.8 ms (P3-tmux-local), quiet 50 Hz,
  same host and clock.
- **Flood (500 kB/s) hurts only tmls** (receiver-observed one-way): P4 one-way p50/p99 5.7/50.8 → 13.5/84.1 ms;
  P1/P2/P3 unchanged (P1 0.1/0.3 ms, P3-tmux-local 0.4/0.5 ms). tmls flood ack percentiles come from a subsample —
  1855/10000 (P4), 1079 (P5-loop), 1042 (P5-pcl) acks seen, `dropped_acks=0` — and are not comparable with the
  quiet ack percentiles.

- **From a kitty-hosted sender on the PC: same picture.** P6 re-ran P1/P2/P5 from pcl with kitty 0.49.2 as the
  parent process (10,000 events @ 50 Hz, quiet): 0 missing / 0 dup on all three; tmls row again 1250 corrupt
  AltLetter (detected K=5). Receiver-observed ack p50 0.0 ms (local), 5.7 ms (ssh to archbox), 16.9 ms (tmls
  via ssh). Same scripted-PTY bytes, so this does not exercise kitty's keyboard handling.
- **Real keyboard events (uinput→KDE Wayland→kitty→PTY→receiver): 2000/2000, no loss/corruption, plain letters
  only.** K1: `ydotool type` (key-delay 20 ms, ~3.4 events/s) into a focused kitty running `s0 recv --plain`;
  receiver-observed, no latency (ydotool has no timestamps). No Alt/Ctrl/F-keys/paste (ydotool types printable text).

## Setup

| item | value |
|---|---|
| archbox | Arch, kernel 7.0.14, 4 cores, headless, OpenSSH 10.3p1, tmux 3.7. Runs the sender for all paths except P5-tmls-ssh-pcl. |
| pcl (laanta-chy) | CachyOS, kernel 7.2.9, OpenSSH 10.5p1, tmux 3.7c. LAN to archbox. Rebooted twice during the first matrix (user's Wake-on-LAN test) → its rows failed there (`ssh: … No route to host`, exit 255) and were re-run as `r2-*`. |
| tmls | `~/Others/tmls` commit d5e9f59 + uncommitted changes, Textual 8.2.8, pyte 0.8.2, run as `uv run -q tmls archbox`. Attaches to the `s0-recv` tmux session on archbox's default server, so P4/P5 = tmls → tmux client → tmux server → recv. |
| ssh | `ObscureKeystrokeTiming` default on (`ssh -G archbox` → `obscurekeystroketiming yes`); man ssh_config: 20 ms packet interval for interactive sessions. |
| harness | `spikes/s0`, commits 1551e2d..e1a4b8a. First matrix (`20261007-*`) used the binary built at d69ca3f; `r2-*` and `c-*` used md5 `8275578f997ff41f79d776317553e8e7` (e1a4b8a). The `complete` / `dropped_acks` cells of every row below were re-computed from the original logs with `s0 report` at 437b743 (md5 `c301feccc25ab39a10c0cff2483b0538`, revised completeness rule); counts and percentiles are unchanged. Runner + column docs: `spikes/s0/run/README.md`. |
| events | 8 kinds in a fixed cycle (seq % 8): Letter, Digit, AltLetter, CtrlLetter, ModArrow, FKey, Repeat, Paste → 1250 each per 10k row. Each event = payload bytes + `#dddddd` marker in one write (`MARKER_GAP_MS=0` unless stated). Ctrl+P removed from the alphabet at d69ca3f. |
| loads | quiet; flood = receiver writes 500,000 B/s of regular 99-byte coloured lines (not a recorded Claude workload). |

**What a row means.** `arrived`/`missing`/`dup`/`corrupt` come from the receiver log (distinct seqs in `0..events`).
One-way = receiver wall clock at parse − sender wall clock at write end (all markers in one read share a timestamp).
Ack = sender wall clock when `A<seq>` is seen in the path's output − write end: single clock, includes the return
path and, for tmls, tmls rendering the ack row. Under flood, acks share the receiver's stdout queue
(`dropped_acks` = acks the receiver could not queue) and, through tmls, many queued acks are never seen in tmls's
output, so the ack sample is a subsample.

**Completeness (revised rule).** `complete=yes` needs `sent == events`, `recv_exit=quit_after` and no unparsable
log lines. `saw_done` is no longer part of the rule: the receiver writes and flushes its done line
(`exit=quit_after`) *before* it prints `S0DONE`, so `recv_exit=quit_after` already proves the receiver log is
complete; whether `S0DONE` then reached the sender's screen is a separate fact (through tmls it usually did not).
The original runner (d69ca3f / e1a4b8a) also required `saw_done=true`, so its summaries showed `NO:no_done` on the
tmls rows. All rows in this document were re-reported with the revised rule (binary 437b743, `s0 report --events
10000 --header …` on the run A/B/C logs, `--events` from the done line for `r2-*`/`c-*`; output in
`/data/scratch/s0/fixwave-rereport/`): **every row is `complete=yes`**, `dropped_acks=0`, `unparsed_lines=0`.
`saw_done=false` (shown as "no S0DONE") on every tmls row except P5-tmls-ssh-pcl quiet 15 Hz (`r2-quiet15-pcl`),
the one tmls row where `S0DONE` was seen.

**Clock caveat.** Rows marked ⇄ cross hosts; one-way there = difference of two NTP-synced clocks. Skew sampled
once, before the first matrix (2026-10-07T11:10Z): pcl **+3.14 ms ahead** of archbox (5 samples, ±rtt/2 ≈ ±0.7 ms).
Not re-sampled after pcl's two reboots, and every pcl row used here except P2-ssh-pcl quiet 50 Hz was run after
them. Direction: archbox→pcl rows (P2-ssh-pcl, P3-ssh-tmux-pcl) read high by the skew; pcl→archbox (P5-tmls-ssh-pcl)
reads low. Example: P2-ssh-pcl quiet (before reboots) one-way p50 13.4 vs ack p50 10.8; P2-ssh-pcl flood (after)
8.9 vs 9.7. Use ack columns to compare across hosts.

## Results

Run key: **A** = `20261007-111023` (quiet 50 Hz), **B** = `20261007-113151` (flood), **C** = `20261007-114338`
(quiet 15 Hz) — first matrix (logs carry the done-line fields; `complete`/`dropped_acks` re-reported, see
Completeness). **R** = `r2-*` re-runs. ⇄ = cross-host one-way. All rows: events = 10000, reorder = 0,
out_of_range = 0.

### Quiet, 50 Hz

| path | run | arrived | missing | dup | corrupt | acked | dropped_acks | complete | one-way p50/p95/p99/max ms | ack p50/p95/p99/max ms |
|---|---|---|---|---|---|---|---|---|---|---|
| P1-local | A | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 0.1/0.2/0.7/16.5 | 0.3/0.4/1.3/17.8 |
| P2-ssh-loop | A | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 10.6/19.5/20.7/29.8 | 11.1/20.0/21.3/32.4 |
| P2-ssh-pcl ⇄ | A | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 13.4/22.6/23.8/37.1 | 10.8/20.0/21.2/49.7 |
| P3-tmux-local | A | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 0.3/0.5/1.3/12.8 | 0.8/1.0/2.0/13.0 |
| P3-ssh-tmux-pcl ⇄ | R `r2-quiet50-pcl` | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 9.2/18.0/19.2/21.8 | 10.8/19.5/20.9/32.0 |
| P4-tmls-local | A | 10000 | 0 | 0 | 1250 | 9953 | 0 | yes (no S0DONE) | 5.7/46.8/50.8/87.4 | 37.2/81.6/98.4/164.1 |
| P5-tmls-ssh-loop | A | 10000 | 0 | 0 | 1250 | 9943 | 0 | yes (no S0DONE) | 20.0/58.2/66.0/99.1 | 49.4/92.4/107.1/182.7 |
| P5-tmls-ssh-pcl ⇄ (pcl→archbox) | R `r2-quiet50-pcl` | 10000 | 0 | 0 | 1250 | 9940 | 0 | yes (no S0DONE) | 20.8/59.0/66.7/97.6 | 49.0/91.9/106.1/183.2 |

Run A's P3-ssh-tmux-pcl and P5-tmls-ssh-pcl failed (pcl unreachable, exit 255); replaced by R.

### Flood, 500 kB/s, 50 Hz

| path | run | arrived | missing | dup | corrupt | acked | dropped_acks | complete | one-way p50/p95/p99/max ms | ack p50/p95/p99/max ms |
|---|---|---|---|---|---|---|---|---|---|---|
| P1-local | B | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 0.1/0.2/0.3/4.1 | 0.3/0.5/0.7/5.4 |
| P2-ssh-loop | B | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 9.9/19.4/20.7/22.1 | 10.4/19.9/21.2/23.6 |
| P2-ssh-pcl ⇄ | R `r2-flood50` | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 8.9/19.0/20.2/25.6 | 9.7/19.8/21.1/32.4 |
| P3-tmux-local | B | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 0.4/0.5/0.5/5.8 | 0.8/1.0/1.1/15.6 |
| P3-ssh-tmux-pcl ⇄ | R `r2-flood50` | 10000 | 0 | 0 | 0 | 9999 | 0 | yes | 8.8/19.1/20.3/28.5 | 9.4/19.8/21.1/33.9 |
| P4-tmls-local | R `r2-flood50` | 10000 | 0 | 0 | 1250 | **1855** | 0 | yes (no S0DONE) | 13.5/59.9/84.1/136.8 | 29.1/75.1/102.9/126.0 (n=1855) |
| P5-tmls-ssh-loop | R `r2-flood50` | 10000 | 0 | 0 | 1250 | **1079** | 0 | yes (no S0DONE) | 23.5/68.7/90.3/135.2 | 51.1/103.4/142.3/234.3 (n=1079) |
| P5-tmls-ssh-pcl ⇄ (pcl→archbox) | R `r2-flood50` | 10000 | 0 | 0 | 1250 | **1042** | 0 | yes (no S0DONE) | 23.4/67.8/92.2/136.2 | 48.5/99.0/119.2/140.4 (n=1042) |

Run B's tmls rows (P4, P5-loop) **failed from a harness gap**, not tmls: the flood started at receiver launch, so
`S0READY` had scrolled off before tmls attached (`Error: S0READY not seen within 30000 ms`, 0 events sent). Fixed
by starting the flood only after the first event arrives (commit 7b0237e), then re-run in R. Run B's pcl rows
failed (pcl unreachable). On tmls flood rows `dropped_acks=0`: the receiver queued every ack; 81–90% of them were
never seen in tmls's output, so tmls ack percentiles here describe a minority subsample (10–19% of events) and are
not comparable with the quiet rows.

### Quiet, 15 Hz

| path | run | arrived | missing | dup | corrupt | acked | dropped_acks | complete | one-way p50/p95/p99/max ms | ack p50/p95/p99/max ms |
|---|---|---|---|---|---|---|---|---|---|---|
| P2-ssh-pcl ⇄ | R `r2-quiet15-pcl` | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 8.5/17.4/19.0/124.9 | 10.4/19.0/20.3/126.1 |
| P4-tmls-local | C | 10000 | 0 | 0 | 1250 | 9966 | 0 | yes (no S0DONE) | 5.5/140.8/144.2/182.6 | 39.4/171.8/173.7/208.0 |
| P5-tmls-ssh-pcl ⇄ (pcl→archbox) | R `r2-quiet15-pcl` | 10000 | 0 | 0 | **1249** | 9946 | 0 | yes (S0DONE seen) | 20.6/153.5/161.3/223.5 | 57.5/189.4/203.8/239.9 |

Run C's P5-tmls-ssh-pcl and P2-ssh-pcl failed (pcl unreachable). P5-tmls-ssh-pcl 15 Hz: one AltLetter (seq 3394,
Alt+h, `1b68`) arrived intact; this is the only tmls row where `S0DONE` was seen (`saw_done=true`; under the
original rule it was the only tmls row with `complete=yes`). Its per-kind
one-way p50 (computed from the logs, cross-host): AltLetter 151.4, CtrlLetter 87.3, ModArrow 22.4, other kinds
16.3–17.2 ms. P2-ssh-pcl's max 124.9 ms is one event (seq 5918, then 69.8 ms for seq 5919); every other event
≤ 60 ms.

## P6 — kitty-hosted paths on the PC (run `20261007-211414`)

Run `P6-20261007-211414`, 10,000 events @ 50 Hz, quiet, nonce `b06d92954b70e4a4`. `s0 send` ran **inside a kitty
0.49.2 window on pcl** (CachyOS); summary `/data/scratch/s0/runs/P6-20261007-211414/summary.md`, JSON reports
beside it. The tmls row detected K=5. All rows: events = 10000, reorder = 0, out_of_range = 0.

| path | arrived | missing | dup | corrupt | acked | dropped_acks | complete | one-way p50/p95/p99/max ms | ack p50/p95/p99/max ms |
|---|---|---|---|---|---|---|---|---|---|
| P6-kitty-local | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 0.0/0.0/0.0/0.9 | 0.0/0.0/0.1/2.3 |
| P6-kitty-ssh (pcl→archbox) ⇄ | 10000 | 0 | 0 | 0 | 10000 | 0 | yes | 5.7/14.6/16.1/26.6 | 11.0/19.9/21.5/31.2 |
| P6-kitty-tmls (pcl→archbox tmls) ⇄ | 10000 | 0 | 0 | 1250 | 9936 | 0 | yes | 16.9/55.0/62.7/88.8 | 49.5/93.3/108.9/215.2 |

Caveats:
- P6 = P1/P2/P5 re-run from pcl with kitty as the parent process; the scripted-PTY bytes are the same, so it
  **does not exercise kitty's keyboard handling**.
- Cross-host one-way (⇄) carries clock skew that was **not re-sampled**: compare by the ack columns. Direction is
  pcl→archbox. (The one-way p50 of 5.7 ms for P6-kitty-ssh is below its ack p50 of 11.0 ms, consistent with skew
  and/or the ~20 ms ssh obfuscation spread; not resolved here.)
- The tmls row's 1250 corrupt events are the AltLetter defect (#2), as in every earlier 10k tmls row.

## K1 — real keyboard events (run `K1-20261008-035007`)

Logs `/data/scratch/s0/runs/K1-20261008-035007/{recv.jsonl,report.json}`. `ydotool type` (uinput virtual keyboard,
key-delay 20 ms, ~3.4 events/s) into a **focused kitty window on pcl (KDE Wayland)** running
`s0 recv --plain --quit-after 2000 --ack false`. Alphabet = **plain mode**: letters + `#dddddd` markers only,
because ydotool types printable text (no Alt/Ctrl/F-keys/paste). Recv-only report; no sender log, so no latency
(ydotool has no timestamps) and no ack.

| path | load | events | arrived | missing | dup | corrupt | reorder | acked | dropped_acks | recv_exit | saw_done | out_of_range | one-way | ack | complete |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| K1-kitty-ydotool | quiet | 2000 | 2000 | 0 | 0 | 0 | 0 | – | – | quit_after | – | 0 | – | – | yes |

- Receiver arrivals were 2001 = 2000 + the focus probe `x#999999` (out_of_range 1, `intact=false` by design).
- Earlier K1 attempts `K1-20261008-023228`, `-025903` and `-033637` were aborted or misfocused by the user and are
  **not measurements**; `-033637` had 280/280 arrived before the abort.
- **Operational lesson:** `ydotool` types into whatever window is focused, so a misfocused run types into the wrong
  program. The K1 script now sends a focus probe first (and the receiver prints a "CLICK THIS WINDOW" banner).
- What K1 shows: for plain letters, uinput → KDE Wayland → kitty → PTY → receiver lost, duplicated and corrupted
  nothing in 2000 events at ~3.4 events/s. It says nothing about modifier/Alt keys, higher rates, paste or IME.

## tmls per-kind breakdown

Source: `/data/scratch/s0/analysis/tmls-per-kind.md` (script `tmls_per_kind.py`, read-only on the logs). Runs A
(P4, P5-loop) and C (P4 15 Hz). n = 1250 per kind per run; missing = 0 for every kind.

| kind | corrupt (all 3) | P4 50 Hz one-way p50/p99 | P4 50 Hz ack p50 | P5-loop 50 Hz one-way p50/p99 | P4 15 Hz one-way p50/p99 | P4 15 Hz ack p50 |
|---|---|---|---|---|---|---|
| Letter | 0 | 3.4/24.3 | 28.9 | 15.1/38.5 | 4.8/8.4 | 39.3 |
| Digit | 0 | 3.3/23.7 | 30.7 | 16.1/36.6 | 4.9/8.8 | 39.3 |
| AltLetter | **1250** | **45.6/65.5** | 77.3 | **55.9/71.6** | **140.1/148.1** | 171.3 |
| CtrlLetter | 0 | **28.2/49.9** | 57.4 | **38.1/55.4** | **77.4/83.5** | 104.5 |
| ModArrow | 0 | 9.2/32.2 | 38.2 | 19.6/40.2 | 12.1/19.1 | 38.0 |
| FKey | 0 | 4.4/22.6 | 31.9 | 16.4/40.9 | 5.0/9.3 | 39.1 |
| Repeat | 0 | 3.6/23.3 | 31.7 | 16.8/39.9 | 4.4/7.8 | 27.0 |
| Paste | 0 | 4.0/21.9 | 30.0 | 16.1/39.2 | 4.8/8.5 | 38.9 |
| ALL | 1250 | 5.7/50.8 | 37.2 | 20.0/66.0 | 5.5/144.2 | 39.4 |

Observations (from the analysis):
- Corruption is confined to AltLetter, identical in P4, P5-loop, P5-pcl and at 15/50 Hz and under flood. Got bytes:
  `^`+letter (hex `5e61`…), 19 letters, 62–63 times each, plus `ESC[1;5C` for Alt+f (report `corrupt_examples`:
  seq 18 expected `1b66`, got `1b5b313b3543`).
- ESC prefix alone does not predict delay: FKey and Paste (also ESC-prefixed) are as fast as Letter.
- AltLetter latency **grows as the send rate drops** (45.6 → 140.1 ms p50, 50 → 15 Hz); at 15 Hz 1180/1250 sit in
  130–150 ms. At 50 Hz it is below 100 ms, so it is not a fixed 100 ms hold at that rate; at 5 Hz it is 102.4 ms
  (Controls). Supported hypothesis (rate and next-key interval co-vary): the next event's bytes, arriving inside
  the 100 ms window, end the pending sequence.
- CtrlLetter always follows AltLetter (seq%8 = 3). At 15 Hz its p50 (77.4) ≈ AltLetter p50 − 66.7 ms send gap
  (73.4): consistent with being queued behind the held AltLetter. With the fixed cycle this cannot be separated
  from kind.
- P5-loop adds ~11–12 ms to every kind's p50 over P4, matching P2-ssh-loop's p50 10.6 ms; the AltLetter/CtrlLetter
  offsets persist.
- P2-ssh-loop shape: one-way roughly uniform over 0–21 ms (each 1 ms bin 4.1–5.5% through 0–20 ms), same for
  every kind (p50 10.1–10.8), independent of seq%k (k = 2..10) and send phase (r = −0.015), lag-1 autocorrelation
  0.496 (slow sawtooth wrapping at ~20 ms); 7.8% of consecutive events arrive < 1 ms apart.

## Controls

All controls: run ids `c-*` (driver `/data/scratch/s0/runs/driver2.sh`), binary e1a4b8a, quiet, all completed
(driver log `END … exit=0` for every run). Columns as above; events < 10000 by design.

### ssh keystroke-timing obfuscation and rate (P2-ssh-loop, single clock)

| run | setting | events | arrived | missing | corrupt | complete | effective rate | one-way p50/p95/p99/max ms | ack p50/p95/p99/max ms |
|---|---|---|---|---|---|---|---|---|---|
| A (reference) | default, 50 Hz | 10000 | 10000 | 0 | 0 | yes | 50 Hz | 10.6/19.5/20.7/29.8 | 11.1/20.0/21.3/32.4 |
| `c-ssh-noobscure` | `-o ObscureKeystrokeTiming=no`, 50 Hz | 2000 | 2000 | 0 | 0 | yes | 50.0 Hz | **0.4/0.6/1.5/8.5** | **0.9/1.3/2.2/15.1** |
| `c-ssh-rate10` | default, 10 Hz | 1000 | 1000 | 0 | 0 | yes | 10.0 Hz | 9.6/19.2/20.6/21.9 | 10.1/19.7/21.2/29.6 |
| `c-ssh-rate200` | default, 200 Hz requested | 4000 | 4000 | 0 | 0 | yes | 174.4 Hz | 6.7/18.5/20.3/21.6 | 7.3/19.0/20.9/22.1 |

- Turning obfuscation off removes the 0–21 ms spread on loopback: p50 10.6 → 0.4 ms, every kind p50 0.4–0.5 ms
  (Repeat measured from write end; its five bytes are 2 ms apart). Loopback only; not run on archbox↔pcl.
- With obfuscation on, the spread stays bounded at ~21 ms p99 from 10 Hz to 174 Hz; p50 falls at 174 Hz (6.7 ms).
- No control lost or corrupted an event.

### Marker gap / rate (P4-tmls-local; `MARKER_GAP_MS` = ms between an event's payload bytes and its `#dddddd` marker)

One-way is measured from the **marker** write end, so with a gap the payload had already been in tmls for ≥ 120 ms;
the "from payload" column is recv − write_start (computed from the logs).

| run | gap / rate (effective) | events | arrived | missing | AltLetter corrupt | complete | one-way p50/p95/p99/max ms | AltLetter one-way p50 (from marker / from payload) | CtrlLetter p50 | ack p50 |
|---|---|---|---|---|---|---|---|---|---|---|
| C (reference) | 0 / 15 Hz | 10000 | 10000 | 0 | 1250/1250 | yes (no S0DONE) | 5.5/140.8/144.2/182.6 | 140.1 | 77.4 | 39.4 |
| `c-tmls-gap0-r5` | 0 / 5 Hz (5.0) | 600 | 600 | 0 | **4/75** | yes (no S0DONE) | 5.5/102.7/106.4/108.5 | **102.4** / 102.4 | 5.6 | 53.7 |
| `c-tmls-gap120-r5` | 120 / 5 Hz (5.0) | 600 | 600 | 0 | **4/75** | yes (no S0DONE) | 2.2/5.5/7.2/10.4 | 2.0 / 122.2 | 2.0 | 40.6 |
| `c-tmls-gap120-r50` | 120 / 50 Hz requested (8.2) | 2000 | 2000 | 0 | **13/250** | yes (no S0DONE) | 3.5/6.5/10.5/39.9 | 3.8 / 124.0 | 4.3 | 34.1 |

- The 4 / 4 / 13 corrupt events are all Alt+f (seq ≡ 18 mod 160), each arriving as `ESC[1;5C`
  (`c-tmls-gap120-r50` report `corrupt_examples`: seq 18/178/338…, `expected_hex=1b66`, `got_hex=1b5b313b3543`);
  every other Alt+letter arrived intact in all three controls. Alt+f → Ctrl+Right is therefore independent of
  timing.
- **The marker in the same write is not what triggers the `^`+letter corruption**: with gap 0 at 5 Hz the
  ESC+letter+marker write arrives intact. What differs from the 15/50 Hz runs is that the next event's bytes arrive
  66.7 / 20 ms later — inside the 100 ms `ESCAPE_DELAY` window. Supported hypothesis (not isolated: rate and next-event
  interval were not varied independently): the pending candidate sequence absorbs the next event's bytes, becomes
  unrecognisable, and is reissued with ESC → `^`.
- At 5 Hz with no gap, AltLetter is held 102.4 ms p50 (p95 107.2) ≈ the 100 ms `ESCAPE_DELAY`, and the following
  CtrlLetter is no longer delayed (5.6 ms p50 vs 77.4 at 15 Hz).
- With a 120 ms gap the hold falls inside the gap (AltLetter from payload 122.2 ms ≈ other kinds 122–123 ms), so
  these runs cannot show the hold itself.
- All three are `complete=yes` under the revised rule (every event sent and arrived, `recv_exit=quit_after`);
  `S0DONE` was not seen on tmls's screen (`saw_done=false`; the original runner showed these as `NO:no_done`).

## Interpretation vs PLAN §1

PLAN §1 hypothesis: *"fewer nested terminal translations + native input + off-loop parsing removes most lag."*

| claim | status | rows |
|---|---|---|
| Keys are lost somewhere on the laptop → tmls → ssh → tmux path | **Partly confirmed, for scripted input**: the only losses measured are in tmls (Ctrl+P swallows all later keys — from event 59 onward, 141 missing incl. event 59; palette attribution not yet confirmed by a palette-disabled control; Alt+letter is re-encoded). ssh, tmux and the PTY hops lost / corrupted 0 of 10,000 at every load. The cause of the user's *physical* missed keys is not established (no keyboard in any row). | dry run 141/200; all tables |
| Nested terminal translation is a cost | **Confirmed for tmls's translation only**: Textual parse → tmls `key_to_bytes` changes Alt+letter bytes and delays them and the next key. That Textual holds the pending ESC sequence and the next key's bytes merge into it is a **supported hypothesis** (rate and next-key interval co-vary in every run; CtrlLetter always follows AltLetter). tmux translation (P3-tmux-local) adds ≈ 0.2 ms one-way p50 and 0 corruption. | per-kind; P1 vs P3 |
| tmls adds lag | **Confirmed** (scripted, receiver-observed): ack p50 37.2 ms (P4) vs 0.8 ms (P3-tmux-local); AltLetter/CtrlLetter one-way +25–140 ms (mechanism: supported hypothesis, above); plain keys +3 ms one-way over tmux. How much of the ack figure is the 30 Hz redraw timer is **unresolved** (no presentation timestamp). | quiet 50/15 Hz |
| Off-loop parsing would help under output load | **Consistent, not proven**: only tmls degrades under flood (receiver-observed: P4 one-way p99 50.8 → 84.1 ms, max 136.8; tmls flood ack percentiles are an 18.6% subsample, not comparable with quiet); P1/P2/P3 do not. The flood was not varied, and on-loop pyte parsing was not isolated from rendering. | flood |
| ssh is a latency source | **Measured on loopback, not in the original hypothesis**: on the measured loopback interactive ssh path (P2-ssh-loop) the hop adds a uniform 0–~21 ms, independent of kind and load; control `c-ssh-noobscure` (loopback, `ObscureKeystrokeTiming=no`) drops it to p50/p99 0.4/1.5 ms, so the spread is the ssh client's keystroke-timing obfuscation. The pcl hops (P2-ssh-pcl, P3-ssh-tmux-pcl, P5-tmls-ssh-pcl) and P5-loop's extra ~11–12 ms over P4 are *consistent with* it; cross-host one-way numbers carry clock skew (not re-sampled after pcl's reboots) and the on/off pair was not run across the LAN. | P2-loop; controls; P2/P3-pcl/P5 consistent |
| Native input removes most lag | **Unresolved**: tmls-specific defects would go away with any client that does not route keys through Textual; the ssh 0–21 ms hop remains for any client that sends keystrokes through an interactive ssh session with obfuscation on; key-to-photon latency was not measured. | — |

## tmls defects found

| # | defect | evidence | code | quick fix it implies |
|---|---|---|---|---|
| 1 | **Ctrl+P opens Textual's command palette**; every later key goes to the palette, not the session | dry run: 59/200 arrive, from event 59 onward (141 missing incl. event 59, the `^P`), all three tmls paths; no palette-disabled control yet | Textual `app.py:427` `ENABLE_COMMAND_PALETTE = True`, `app.py:441` `COMMAND_PALETTE_BINDING = "ctrl+p"`, `app.py:874–881` adds the binding; tmls `src/tmls/app.py:223` `class Tmls(App)` does not override it | `ENABLE_COMMAND_PALETTE = False` on `Tmls` (Ctrl+P then reaches `key_to_bytes` → `0x10`; not re-measured) |
| 2 | **Alt+letter is corrupted**: ESC+letter arrives as `^`+letter (19 letters) when the next key follows within ~100 ms, and Alt+f always arrives as Ctrl+Right `ESC[1;5C` | 1249–1250/1250 AltLetter corrupt on every 10k tmls row (15/50 Hz); at 5 Hz only Alt+f (4/75) | Textual `_xterm_parser.py:242` reads into one candidate sequence until `ESCAPE_DELAY` (`constants.py:155`, `ESCDELAY` env, default 100 ms); an unrecognised sequence is reissued as keys, with ESC emitted as `Key("circumflex_accent", "^")` (`_xterm_parser.py:196`); `"\x1bf"` is mapped to `Keys.ControlRight` (`_ansi_sequences.py:274`, "iTerm natural editing keys"); tmls `term.py:84` `key_to_bytes` then writes `^` / `ESC[1;5C` (`term.py:290`) | Textual-side: not fixable in tmls's `key_to_bytes` alone, because the ESC is lost before tmls sees the key. Options: lower `ESCDELAY` (shortens the hold; does not by itself prevent the candidate sequence from absorbing following bytes), or read the PTY input bytes for the Terminal widget before Textual's key parser. Controls: at 5 Hz the Alt+letter arrives intact with or without a marker gap (except Alt+f), held ≈ 102 ms (= `ESCAPE_DELAY`); a lower `ESCDELAY` would shorten that hold and the window in which the next key can be absorbed (not measured). Alt+f → Ctrl+Right needs tmls to bypass or override Textual's `"\x1bf"` mapping. |
| 3 | **Alt+letter is delayed, and the next key behind it** (hold + next-key merge: supported hypothesis) | AltLetter p50 45.6 ms (50 Hz) / 140.1 ms (15 Hz) / 102.4 ms (5 Hz); CtrlLetter 28.2 / 77.4 / 5.6 ms | same parser path (`_xterm_parser.py:242`) | as #2 |
| 4 | **Latency added per hop** (scripted, quiet 50 Hz, single-clock rows) | one-way p50: P1 0.1 → +tmux 0.3 → +tmls 5.7 (Letter 3.4) → +ssh-loop 20.0; ack p50: P1 0.3, P3-tmux 0.8, P4 37.2, P5-loop 49.4 | tmls `term.py:183` `set_interval(1/30, self._flush)` (redraw at most every 33 ms); `term.py:211–222` `_read` reads up to 64 KiB (`term.py:213`) and feeds pyte on the event loop | no measured fix; the 30 Hz timer and on-loop parsing are suspects (PLAN §1), not isolated by S0 |
| — | ssh keystroke-timing obfuscation (not a tmls defect) | P2-ssh-loop uniform 0–21 ms | OpenSSH client `ObscureKeystrokeTiming` (default yes) | only by not sending each keystroke through an interactive ssh session, or `ObscureKeystrokeTiming=no` on the client. **Trade-off**: the option exists to hide keystroke timing (e.g. typed passwords) from a network observer; turning it off re-exposes inter-key timing. |

tmls line numbers are in the working tree (d5e9f59 + uncommitted changes); Textual line numbers are Textual 8.2.8 in
`~/Others/tmls/.venv`. Correction to the session notes: Alt+f → Ctrl+Right comes from `_ansi_sequences.py:274`,
not from `KEY_NAME_REPLACEMENTS` (`keys.py:205`, which only renames Unicode key names such as `solidus`).

## Not measured / next steps

Not measured (Codex review, `docs/reviews/2026-10-07-codex-sol-s0-harness.md`):
- Modifier/Alt/Ctrl/F-keys and paste via a real keyboard (K1 covers plain letters only), IME, the Windows laptop.
  Every other row is scripted bytes into a PTY (P6 included). The cause of a missed *physical* key is therefore
  not inferred.
- Presentation (key-to-photon) latency; the 30 Hz redraw's share of tmls ack time.
- Esc→next-key interval independently of send rate (the controls varied rate and marker gap only); jittered send
  intervals; `ObscureKeystrokeTiming=no` across the LAN (loopback only).
- A recorded Claude-output workload (the flood is regular 99-byte coloured lines at one rate).
- Ctrl+P after the palette fix; tmls `key_to_bytes` returning `b""` for unmapped keys; PTY short writes.
- Clock skew after pcl's reboots.

Next steps:
1. *(done: P6 kitty-hosted and K1 ydotool plain-letter runs, see above.)*
2. tmls quick fixes to try (then re-run P4/P5): `ENABLE_COMMAND_PALETTE = False`; Alt-key handling per defect #2.
3. Re-sample archbox↔pcl skew before any further cross-host one-way comparison.
4. Presentation timing and the Claude-output workload belong with S2 / §9, not S0.

## Open (from reviews)

Controls not yet run (Codex review `docs/reviews/2026-10-07-codex-sol-s0-results.md`):
- **Ctrl+P with the palette disabled**: rerun the Ctrl+P stream with `ENABLE_COMMAND_PALETTE = False` to confirm the
  palette (not something else at event 59) swallows the later keys.
- **Esc→next-key interval varied independently** of send rate and marker gap, and with a lower `ESCDELAY`, to
  separate the hold from the next-key merge (and from CtrlLetter's fixed position after AltLetter).
- **`ObscureKeystrokeTiming` on/off across the LAN** (archbox↔pcl) with fresh clock-skew samples.
- **Modifier/Alt keys via a real keyboard**: K1 (ydotool) covered plain letters only; Alt/Ctrl/F-keys need another
  injector (e.g. `ydotool key` raw codes) or manual typing.
- **Key-to-photon** (presentation) latency.

## Run inventory

All under `/data/scratch/s0/runs/<id>/` (`summary.md`, `*.send.jsonl`, `*.recv.jsonl`, `*.report.json`). pcl-side
logs were fetched back to the same dirs.

| run id | what | rows used here |
|---|---|---|
| `20261007-110103` … `20261007-110954` | dry runs, EVENTS=200 (50 for 110954), alphabet still with Ctrl+P. `20261007-110237` (P4) and `20261007-110347` (P5-loop, P5-pcl): 59/200 arrived, 141 missing, 8 corrupt each; non-tmls dry rows 0 missing | Ctrl+P result |
| `t9-dry-flood` | flood dry check (P1, P4) | — |
| `20261007-111023` (A) | quiet, 10k, 50 Hz, binary d69ca3f | P1, P2-loop, P2-pcl, P3-local, P4, P5-loop (P3-pcl, P5-pcl failed: pcl down) |
| `20261007-113151` (B) | flood 500 kB/s, 10k, 50 Hz | P1, P2-loop, P3-local (pcl rows failed: pcl down; tmls rows failed: S0READY harness gap) |
| `20261007-114338` (C) | quiet, 10k, 15 Hz | P4 (P5-pcl, P2-pcl failed: pcl down) |
| `r2-quiet50-pcl` | quiet 50 Hz re-run, binary e1a4b8a | P3-ssh-tmux-pcl, P5-tmls-ssh-pcl |
| `r2-flood50` | flood re-run with lazy flood (7b0237e) | P2-pcl, P3-pcl, P4, P5-loop, P5-pcl |
| `r2-quiet15-pcl` | quiet 15 Hz re-run | P5-tmls-ssh-pcl, P2-ssh-pcl |
| `c-ssh-noobscure` | P2-ssh-loop, 2000 ev, 50 Hz, `ObscureKeystrokeTiming=no` | controls |
| `c-ssh-rate10` / `c-ssh-rate200` | P2-ssh-loop, 1000 ev @ 10 Hz / 4000 ev @ 200 Hz (174 eff.) | controls |
| `c-tmls-gap0-r5` / `c-tmls-gap120-r5` | P4-tmls-local, 600 ev @ 5 Hz, marker gap 0 / 120 ms | controls |
| `c-tmls-gap120-r50` | P4-tmls-local, 2000 ev @ 50 Hz requested (8.2 eff.), gap 120 ms | controls |
| `P6-20261007-211414` | P6 kitty-hosted P1/P2/P5, 10k @ 50 Hz, kitty 0.49.2 on pcl, nonce `b06d92954b70e4a4` | P6 |
| `K1-20261008-035007` | K1 ydotool plain letters into kitty, 2000 events (`recv.jsonl`, `report.json`) | K1 |
| `K1-20261008-023228`, `-025903`, `-033637` | aborted / misfocused K1 attempts (`-033637`: 280/280 before abort) | — (not measurements) |
| `skew.txt` | NTP status + archbox↔pcl offset (only the pre-A sample succeeded) | clock caveat |
| `/data/scratch/s0/analysis/tmls-per-kind.md` | per-kind analysis of A and C | per-kind section |
