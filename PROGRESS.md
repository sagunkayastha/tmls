# PROGRESS — tmls-rs (2026-10-08)

Quick session log. Newest entry first.

## 2026-10-08 — S1b round-2 fix wave at code commit 8eaf614

Branch `spike/s1b-r2`. The focused ECH-on-wrapped-row and resize-epoch regressions now pass, as do wide-glyph hyperlink, alt-mode edge, and CSI-X parsing tests. `Tracker` carries the worker's scrollback setting into temporary restores. The whole-snapshot wrapped fallback was replaced by a row-aware path with conservative wide-spacer handling. `cargo test -p s1`: 73 passed, 2 ignored.

Final matrix: **3901/3901** AlignedCarry cases, **3644/3901** immediately faithful at restore, and **3883/3901** equal row-wrap flags (18 diagnostic survivors). Final handoff: **480/480** plain and **480/480** resized, zero client reply attempts; the three former `wrap_pending` resize failures are gone. Exact artifacts: `/data/scratch/s1/results/2026-10-08-s1b-r2-final/matrix/` and `/data/scratch/s1/results/2026-10-08-s1b-r2-final/handoff/`, both stamped with `8eaf614`. Production attach remains provisional pending transport/atomic subscription, side effects, and broader reply suppression. Compact ≤200-byte repros for the 18 wrap-flag survivors remain open. Phase B waits for Claude re-review and merge.

## 2026-10-08 — S1b round 2: corpus gate closed

Branch `spike/s1b-r2`, Linux archbox. ECH overlays erased empty cells with their background and skips wide-cell spacers; the same corrected snapshot is carried behind alternate-screen entries. All 73 round-1 residuals closed, including the three continuation-only mode-47 cases. AlignedCarry is **3901/3901** identical under the unchanged S1 comparison; no upstream issue remains to file. `cases.jsonl` adds post-tail mismatch, alt-entry mode, and diagnostic row-wrap flag equality (3873/3901 equal).

Unwrapped emission plus row-flag fallback improves simulated handoff to **480/480 plain and 477/480 resized** across 24 streams, 20 seeded splits each. The three remaining resize text mismatches are `wrap_pending` splits 679/681/688. A secondary-DA query was added to the short replica query filter; final client PTY reply attempts are zero. Production §5.2 remains provisional because the three resize cases, real transport/atomic subscription, and phase-aware side-effect suppression remain open. Results: `/data/scratch/s1/results/2026-10-08-s1b-r2-run5/` and `/data/scratch/s1/results/2026-10-08-s1b-r2-handoff-verified/`. See `docs/spikes/S1b-results.md` and `docs/spikes/S1b-residuals.md`.

## 2026-10-08 — S1b snapshot repair measured; gate remains open

Branch `spike/s1b-snapshot-repair`, Linux archbox. Results: `docs/spikes/S1b-results.md`; residuals and upstream issue drafts: `docs/spikes/S1b-residuals.md`. The unchanged S1 comparison reports AlignedCarry 3271/3344 (97.8%) on the original corpus, 3828/3901 (98.1%) with four new recordings; all 557 prompt-first/nested2 splits are identical. Naive/Aligned/AlignedUnwrap original-corpus totals remain 1688/2628/2618. The 73 residuals are generated-stream styled/erased blank cells; three pass the immediate snapshot comparison and diverge after continuation, so their cause remains open. There are three ≤200-byte ignored repro tests and two proposed issue drafts; no issue was filed.

`carry::Tracker` and `snapshot_full` now preserve title/pwd, kitty stack, saved cursor/pen, primary screen behind alt (including combined private-mode sequences), hyperlinks and active OSC 8, trailing blank history rows, origin-safe cursor, and pending wrap including wide glyphs. `s1 handoff` simulated 20 aligned splits × 20 streams × plain/resize: 385/400 plain and 309/400 resize identical, zero client PTY reply attempts on those streams, 586 observed side-effect callback invocations with inert handlers. Resize reflow is unresolved; the short client query filter and inert callback are a simulation rather than a general Ghostty reply-disable switch or phase-aware side-effect suppression. No real network or atomic replay-subscription race was tested.

Corpus v2 (`vim_prompt`, `less_prompt`, `btop_prompt`, `nested2`) preserves the original seven recordings. Claude remains skipped pending repo trust confirmation; Windows MSVC, graphics, and 10k-line scrollback performance remain. PLAN §3/§5.2/§6 marks S1b measured but open; §5.2 attach/reconnect is not frozen. Runs: `/data/scratch/s1/results/2026-10-08-s1b-run5/` (original corpus), `/data/scratch/s1/results/2026-10-08-s1b-run6/` (expanded corpus), and `run5/handoff/`.

## 2026-10-08 — S1b round 2 under review; next-step sequence recorded (continue tomorrow)
Codex implemented `docs/superpowers/plans/2026-10-08-s1b-round2.md` on `spike/s1b-r2` (cf5abc0 ECH overlay, 76ed938
alt re-entry, 6654a00 soft-wrap carry, 56a63d0 docs; f653028 Codex self-review). Claimed + verified by the opus
whole-branch review: AlignedCarry **3901/3901** identical under the unchanged S1 comparison; handoff 480/480 plain,
477/480 resize; 0 client reply attempts; 65 tests. Both reviews: merge-with-fixes. Open: ECH resets ghostty's soft-wrap
flag (Terminal.zig:3036) and the fallback was whole-snapshot → 28 unequal-row-flag snapshots + the 3 resize failures
(carry bug, not Formatter limit); PLAN §6 gate text was rewritten (must be restored); 257/3901 snapshots not faithful
until the tail (report as attach-time fidelity). Fix-wave prompt (Phase A) + S2 Task 1–2 prompt (Phase B) handed to
Codex in chat; Codex login on archbox expired (`codex login` needed) — user runs Codex reviews from the PC meanwhile.

### NEXT (in order)
1. Codex: Phase A fix wave on `spike/s1b-r2` (per-row unwrap + wrap restore after ECH; CSI-X test parse; hyperlink
   wide-spacer skip; scrollback param; alt-mode edge tests 1047/1049/nested; rerun `s1 run` + `s1 handoff` with the
   commit hash stamped; PLAN §6 S1b text restored; faithful-at-restore column; residuals doc updates).
2. Claude: scoped re-review (opus), rerun matrix/handoff at HEAD, merge `spike/s1b-r2` → main, sync PC, PROGRESS/memory.
   Gate S1b closed only if 3901/3901 + 480/480 + row flags equal; else open with named residuals.
3. Codex: Phase B — `spikes/s2-core` + `spikes/s2-wgpu` on `spike/s2` (compile-only; Dockerfile GUI libs added).
4. Claude: review B; rebuild the PC Docker image (ssh, Docker only).
5. User at PC (yes per session): `s2-wgpu` smoke run (`tooling/docker/build.sh build --release -p s2-wgpu`, ldd check,
   `target-docker/release/s2-wgpu --bench ~/s2-logs/wgpu-smoke`); Claude reads the latency log; iterate.
6. Codex: S2 Task 3 (sidebar/field/menu/IME) → user checklist (fcitx5 compose, wide-char copy, 125 % scaling,
   monitor move) → `docs/spikes/S2-checklist-wgpu.md`.
7. Codex: Task 4 `s2-gpui` → smoke + checklist. 8. Codex: Task 5 `s2-bench`.
9. User + Claude: 16-run matrix on the PC + photon video (phone 240 fps) → `S2-results.md`, GUI decision in PLAN §3,
   Codex gate, merge.
10. Then S3–S7 (worker survival, transport, Claude status, mixed clients, ConPTY) → re-estimate → Phase 1a headless core.
Side tasks for the user: accept Claude Code trust in `~/Others/tmls-rs` once (enables `record.sh claude`); file the
upstream ghostty issue drafts left in `S1b-residuals.md` after Phase A; `codex login` on archbox.

## 2026-10-07 (late) — S1 merged; plans for S1b + S2 written for Codex/DeepSeek ✅
`spike/s1` merged into main (`7fd556b`), partial `claude.bin`/`claude.meta.json` removed (`d8fca23`, user consent).
PC clone synced. Reviews: `docs/reviews/2026-10-07-codex-sol-s1.md` + final branch review → one fix wave
(`f2f3437`, `697ad9f`), re-review clean, 31 tests pass on HEAD.
- **Plans (hand-off to Codex + DeepSeek; Claude plans/verifies):**
  - `docs/superpowers/plans/2026-10-08-s1b-snapshot-repair.md` — worker state carry (`carry::Tracker`,
    `snapshot_full`: trailing-row pad, hyperlink overlay, pending wrap, title/pwd, kitty stack, DECSC, DECOM-safe
    CUP, primary-at-alt-entry prefix) → `AlignedCarry` strategy in the split matrix; residual analysis + upstream
    issue drafts; handoff simulation (offset, resize epoch, client never replies); corpus v2 (prompt-first,
    nested2, claude). Headless → archbox.
  - `docs/superpowers/plans/2026-10-08-s2-interaction-slice.md` — `s2-core` (PTY + emulator thread + Frame + key
    encoding + latency log), `s2-wgpu` (winit+wgpu grid, atlas, IME, sidebar, field, menu) vs `s2-gpui`, `s2-bench`
    (ydotool driver, CPU/RSS/GPU sampler, photon square) → 16-run matrix on the PC with the user present; decision
    for PLAN §3. Builds in the PC Docker image; runs need the user's yes per PC session.
- **Gotchas carried forward:** Zig 0.15.2 only; `max_scrollback` = bytes; no `snapshot.h` at a887df42; `57151cb`
  does not build alone (bisect caveat); ydotool types into whatever is focused (probe first).

### NEXT
1. Codex/DeepSeek execute S1b (archbox) and S2 (PC Docker builds; user-run measurements) from the two plans;
   Claude reviews each task + runs the Codex gate.
2. User: accept Claude Code trust in `~/Others/tmls-rs` once so `record.sh claude` can record the TUI (S1b Task 6).
3. Later: Windows MSVC build of libghostty-vt (laptop); native snapshot path (newer ghostty + own bindings).

## 2026-10-07 (night) — S1 emulator spike: partially passed (Linux, Formatter path); snapshot repair → S1b
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
  "Identical" = under the S1 comparison (normalised viewport cells + listed TermState fields + right-trimmed history
  text). Over 2833 distinct Aligned snapshot points: 2310 identical (81.5%). At the 1670 non-boundary splits Naive
  fails 1315; under Aligned 940 of those become identical and 375 still fail (text 201, scrollback 100, alt_screen
  57, title 16, style 1) — Formatter gaps hidden by Naive's first-match label. Recordings btop/less/nested/vim 100%,
  cargo 93.6, gitlog 86.5, unicode 86.0 (weak: no prompt-first recordings; `nested.bin` mostly blank, record.sh
  quoting bug fixed, not re-recorded). The 716 Aligned mismatches, each assigned a proposed cause (first match;
  DECSC and erased bg are hypotheses): hyperlinks 321 (VT Formatter never emits OSC 8,
  `formatter.zig:1246-1250`), trailing blank row → history row lost 126, primary screen lost behind alt 109, DECSC
  saved cursor 69, title 37, erased-cell bg 25, pending wrap 12, invisible-only 17.
- **Portability** (run3/portability), text only: right-trimmed visible text of one end-of-stream snapshot per stream,
  pyte(snapshot) = libghostty restore(snapshot) on 577/592 rows (97.5%); 4/20 snapshots near-empty; pyte(snapshot)
  vs uninterrupted D only 351/592. No colours/attributes/history/mid-stream.
- **Resize** (run3/resize): restore at the snapshot's own size, then resize (restoring into another size breaks) —
  4 streams, tested order only; `snapshot_unwrap` needed for resize-before-tail on wrap_pending, costs 10
  wrap_pending cases at same size → not a blanket default.
- **Cost:** snapshot p50 9–390 µs, p95 ≤ 982 µs (gitlog 55 KB); restore p50 31–332 µs incl. `Terminal::new`.
- **Gate:** S1 partially passed under the S1 comparison — the pinned Linux build parsed the S1 corpus and exposes the
  capture API S1 needed; authoritative-worker operation and recovery were not exercised. The Formatter alone is not
  a sufficient snapshot: boundary alignment measured; worker state carry designed, untested → new gate **S1b**
  (PLAN §6). Native snapshot = follow-up.
- **Reviews** (Codex `docs/reviews/2026-10-07-codex-sol-s1.md` + final review) folded in: results doc wording/numbers,
  PLAN §3/§5.2/§6 (S1b row)/§10; `build-env.sh` guard, `record.sh` nested quoting + claude cwd (repo dir).
  Fix-wave report `.superpowers/sdd/2026-10-07-s1-emulator/fixwave-report.md`.
- **Gotchas:** libghostty-vt 0.2.2 needs **Zig 0.15.2** (README's 0.16 fails); `Options::max_scrollback` is
  **bytes**, not lines (ghostty `Screen.zig:282`); no `snapshot.h` at the pin; commit **`57151cb` does not build
  alone** (it swept in Task 4's in-progress hunks; `ddf25e0`+ build); workspace-wide cargo now needs Zig
  (`source spikes/s1/build-env.sh`).

### NEXT
1. S2 GUI interaction slice on the PC (live desktop, user driving).
2. **S1b snapshot repair** (PLAN §6): atomic handoff (snapshot + offset + resize epoch + subscription), client restore
   with replies/side effects off, continuation tests (primary-at-alt-entry, hyperlinks, DECSC, kitty stack, pending
   wrap); re-record `nested` + prompt-first vim/less/btop; DECOM fix-up; 10k-line scrollback.
3. S1 follow-up: native snapshot (newer ghostty with `snapshot.h` + own bindings) or upstream VT fixes (OSC 8,
   trailing blank rows, erased bg); re-run `s1 run`.
4. Windows MSVC build of libghostty-vt (S1's Windows half).
5. Record the Claude TUI stream: accept folder trust in `~/Others/tmls-rs`, then `spikes/s1/corpus/record.sh claude`.

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
