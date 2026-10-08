# PROGRESS — tmls-rs (2026-10-07)

Quick session log. Newest entry first.

## 2026-10-07 (night) — S1 emulator spike: libghostty-vt viable, Formatter snapshot needs worker state carry ✅
Branch `spike/s1` (commits `0813259`..`e23eba0` + this docs commit), run on archbox (Linux only). Results doc
`docs/spikes/S1-results.md`; PLAN §3 Emulator row, §5.2, §6 S1 row and status line updated. Plan
`docs/superpowers/plans/2026-10-07-s1-emulator.md`, ledger `.superpowers/sdd/2026-10-07-s1-emulator/progress.md`.
- **Harness** `spikes/s1` (`s1 gen|capture|run|portability|resize`): grid/state capture + diff, adversarial
  generator (13 streams), VT splitter mirroring ghostty `parse_table.zig`, Formatter snapshot/restore,
  split-matrix runner + taxonomy, pyte portability probe, resize-at-split. Corpus: 7 recordings at 120x40
  (vim btop less cargo gitlog nested unicode; `spikes/s1/corpus/` = `/data/scratch/s1/corpus/`) + 13 generated.
- **Snapshot path:** pinned ghostty a887df42 has **no `snapshot.h`** → only `fmt::Formatter` `Format::Vt`
  replayed into a fresh `Terminal` ("VT-reconstruction snapshot").
- **Split matrix** (run6, `/data/scratch/s1/results/2026-10-07-s1-run6/{summary,taxonomy}.md`; 20 streams, ≤ 400
  splits each, 3344 cases/strategy, wall 16.2 s):
  | strategy | identical | % |
  |---|---|---|
  | Naive (split at the byte) | 1688 | 50.5 |
  | Aligned (last VT boundary + tail carry) | 2628 | 78.6 |
  | AlignedUnwrap (Aligned + `unwrap(true)`) | 2618 | 78.3 |
  Aligned removes all 1315 `parser_state_split` cases. Recordings btop/less/nested/vim 100%, cargo 93.6, gitlog
  86.5, unicode 86.0. The 716 Aligned mismatches: hyperlinks 321 (VT Formatter never emits OSC 8,
  `formatter.zig:1246-1250`), trailing blank row → history row lost 126, primary screen lost behind alt 109, DECSC
  saved cursor 69, title 37, erased-cell bg 25, pending wrap 12, invisible-only 17.
- **Portability** (run3/portability): pyte(snapshot) = libghostty restore(snapshot) on 577/592 rows (97.5%); rest =
  pyte limits; pyte renders the snapshot better than the raw stream on 7 streams.
- **Resize** (run3/resize): restore at the snapshot's own size, then resize (restoring into another size breaks);
  `snapshot_unwrap` needed for reflow after restore, costs 10 wrap_pending cases at same size.
- **Cost:** snapshot p50 9–390 µs, p95 ≤ 982 µs (gitlog 55 KB); restore p50 31–332 µs incl. `Terminal::new`.
- **Gate:** S1 partially passed — differences explained; libghostty-vt viable as the authoritative emulator; the
  Formatter alone is not a sufficient snapshot (worker state carry + boundary alignment); native snapshot = follow-up.
- **Gotchas:** libghostty-vt 0.2.2 needs **Zig 0.15.2** (README's 0.16 fails); `Options::max_scrollback` is
  **bytes**, not lines (ghostty `Screen.zig:282`); no `snapshot.h` at the pin; commit **`57151cb` does not build
  alone** (it swept in Task 4's in-progress hunks; `ddf25e0`+ build); workspace-wide cargo now needs Zig
  (`source spikes/s1/build-env.sh`).

### NEXT
1. S2 GUI interaction slice on the PC (live desktop, user driving).
2. S1 follow-up: native snapshot (newer ghostty with `snapshot.h` + own bindings) or upstream VT fixes (OSC 8,
   trailing blank rows, erased bg); re-run `s1 run`.
3. Windows MSVC build of libghostty-vt (S1's Windows half).
4. Record the Claude TUI stream after accepting folder trust (`spikes/s1/corpus/README.md`).

## 2026-10-07 (eve) — S0 gate closed: kitty-hosted + real-keyboard runs on the PC ✅
User-run measurements on the PC (pcl, CachyOS, KDE Wayland, kitty 0.49.2); results folded into
`docs/spikes/S0-results.md` (new P6 and K1 sections, Summary, Not measured, Run inventory), PLAN §1/§6 updated.
- **P6 — kitty-hosted P1/P2/P5** (run `20261007-211414`, 10k @ 50 Hz, quiet, nonce `b06d92954b70e4a4`; summary
  `/data/scratch/s0/runs/P6-20261007-211414/summary.md`, JSON beside it; tmls row detected K=5). Receiver-observed;
  same scripted-PTY bytes, so kitty's keyboard handling is NOT exercised; cross-host one-way carries unrechecked
  skew, compare by ack:
  | path | arrived | missing | dup | corrupt | acked | one-way p50/p95/p99/max | ack p50/p95/p99/max |
  |---|---|---|---|---|---|---|---|
  | P6-kitty-local | 10000 | 0 | 0 | 0 | 10000 | 0.0/0.0/0.0/0.9 | 0.0/0.0/0.1/2.3 |
  | P6-kitty-ssh (pcl→archbox) | 10000 | 0 | 0 | 0 | 10000 | 5.7/14.6/16.1/26.6 | 11.0/19.9/21.5/31.2 |
  | P6-kitty-tmls (pcl→archbox) | 10000 | 0 | 0 | 1250 | 9936 | 16.9/55.0/62.7/88.8 | 49.5/93.3/108.9/215.2 |
- **K1 — real keyboard events** (run `K1-20261008-035007`, `/data/scratch/s0/runs/K1-20261008-035007/{recv.jsonl,report.json}`):
  `ydotool type` (uinput, key-delay 20 ms, ~3.4 events/s) into a focused kitty running
  `s0 recv --plain --quit-after 2000 --ack false`. Plain letters + markers only (no Alt/Ctrl/F-keys/paste).
  `K1-kitty-ydotool`: 2000 events, 2000 arrived, 0 missing/dup/corrupt, complete=yes; 2001 arrivals = 2000 + the
  focus probe `x#999999` (out_of_range 1, intact=false by design). No latency (ydotool has no timestamps).
  Aborted/misfocused attempts `K1-20261008-023228`, `-025903`, `-033637` are not measurements.
- **Lesson:** `ydotool` types into whatever window is focused — the K1 script now probes focus first (and the
  receiver prints a CLICK THIS WINDOW banner).
- **Gate:** S0 closed for key loss (none found outside tmls); presentation latency and modifier keys via a real
  keyboard remain for S2.

### NEXT
1. S1 (emulator) on the PC Docker image (`tmls-rs-build:latest`): snapshot/restore round-trip via libghostty VT
   Formatter at adversarial byte boundaries.
2. S2 GUI slice on the PC's live desktop with the user driving (key-to-photon, modifier/Alt keys via real keyboard).
3. tmls quick fixes: `ENABLE_COMMAND_PALETTE = False`; Alt-key handling per defect #2; then re-run P4/P5.
4. Open mechanism controls (results doc "Open (from reviews)"): Ctrl+P with palette disabled; Esc→next-key interval
   independent of rate / `ESCDELAY`; ssh obfuscation on/off across the LAN with fresh skew samples.

## 2026-10-07 — S0 key-loss spike built, run and reviewed ✅ (gate not closed: GUI/keyboard paths pending)
Overnight autonomous run on branch `spike/s0` (from `d949858`). Harness + full path matrix + results doc; two Codex
(gpt-6-sol) reviews and a final whole-branch review folded in. Nothing merged to main yet.
- **Harness** `spikes/s0` (Rust, 48 tests): `s0 recv|flood|send|report`; numbered 8-kind key alphabet with `#dddddd`
  markers; PTY driver with TUI nav + nonce-gated `S0READY:<tok>` handshake; receiver log = loss/corruption oracle,
  acks = return-output timing only. Runner `spikes/s0/run/paths.sh` (env `EVENTS RATE PATHS LOAD MARKER_GAP_MS SSH_FLAGS NAV`),
  `deploy.sh` → `pcl:~/.local/bin/s0`. Plan: `docs/superpowers/plans/2026-10-07-s0-key-loss.md`.
- **Results** `docs/spikes/S0-results.md`; PLAN §1/§6 updated. Raw logs `/data/scratch/s0/runs/<id>/` (ids in the doc's
  Run inventory), per-kind analysis `/data/scratch/s0/analysis/tmls-per-kind.md`, re-reports `/data/scratch/s0/fixwave-rereport/`.
- **Measured (scripted PTY input, receiver-observed):** 0 missing / 0 dup on every path (P1 local, P2 ssh loop/pcl,
  P3 tmux local/pcl, P4 tmls, P5 tmls over ssh loop/pcl), 10k events each, quiet 50 Hz + 500 kB/s flood + 15 Hz.
  ssh loopback one-way p50 10.6 ms uniform 0–21 → **0.4 ms with `-o ObscureKeystrokeTiming=no`** (OpenSSH default
  20 ms keystroke obfuscation). tmls: every Alt+letter corrupted (`^x`; Alt+f → `ESC[1;5C`) at 15/50 Hz, intact but
  held ~100 ms at 5 Hz (Textual `ESCAPE_DELAY`, hypothesis: next key inside the window merges); Ctrl+P opens Textual's
  command palette and tmls swallows everything after (dry run 141/200 missing) → Ctrl+P dropped from the alphabet
  (`d69ca3f`); tmls ack RTT p50 37 ms vs 0.8 ms plain tmux; flood raises tmls one-way p50/p99 5.7/50.8 → 13.5/84.1 ms,
  others unchanged.
- **Reviews:** `docs/reviews/2026-10-07-codex-sol-s0-harness.md`, `…-s0-results.md`. Final branch review → fix wave
  `437b743` (ready-token gate, portable test dirs, `complete` rule w/o saw_done, report defaults, deploy mv) +
  `a2390b4` (doc wording). Scoped re-review of the fix wave was still running at log time.
- **PC as build host** (user approved, PC CLAUDE.md = Docker-only): image `tmls-rs-build:latest` (rust 1.96-bookworm +
  zig 0.15.2), `tooling/docker/build.sh`, clone `pcl:~/Others/tmls-rs`; warm test 0.2 s, incremental release 0.5 s.
- **S1 prep:** libghostty-vt 0.2.2 builds with **Zig 0.15.2 (README's 0.16 is wrong)**, pins ghostty a887df42, cold
  build 1m10s, `.a` 43 MB; VT Formatter = replayable stream, no native restore. `/data/scratch/s1-probe/REPORT.md`,
  Zig at `/data/scratch/zig/`.
- **Side effects (tell user):** archbox `~/.ssh/authorized_keys` line 8 = own key with `from="127.0.0.1,::1"` (loopback
  ssh for P2/P5); stray `pcl:~/Others/.t_cold.log` + `.t_warm.log`. pcl rebooted twice during the first matrix (user's
  Wake-on-LAN test) → pcl rows re-run as `r2-*`.
- **DO NOT** run `paths.sh` tmls paths while the user's tmux list is changing (row detection runs per path, but the
  nonce gate is the only thing preventing typing into the wrong session). **DO NOT** build on archbox while a
  measurement matrix runs (latency noise).

### NEXT
1. Read the fix-wave re-review; if clean, merge `spike/s0` → main (user decision; rulings list in the session summary).
2. User, on the PC: `spikes/s0/run/README.md` P6 runs (kitty as parent — same bytes path) and the real gap-closer:
   `ydotool type` into a focused kitty running a bare `s0 recv` (keyboard → app). Still unmeasured after that:
   key-to-photon, Windows laptop, IME.
3. Open controls (results doc "Open (from reviews)"): Ctrl+P rerun with `ENABLE_COMMAND_PALETTE=False`; vary Esc→next-key
   interval independently of rate (and `ESCDELAY`); ssh obfuscation on/off across the LAN with fresh skew samples.
4. Quick tmls fixes implied: `ENABLE_COMMAND_PALETTE = False`; lower `ESCDELAY` / bypass Textual key parsing for Alt;
   client-side `ObscureKeystrokeTiming=no` (security trade-off) — tmls-rs's own transport avoids it entirely.
5. S1 (emulator) on the PC build image: snapshot/restore round-trip via libghostty VT Formatter at adversarial byte
   boundaries; S2 (GPU slice) needs the PC's live desktop with the user driving.
