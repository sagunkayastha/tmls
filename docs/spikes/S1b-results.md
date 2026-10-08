# S1b results — round 2 (2026-10-08)

**S1b corpus gate: closed under the unchanged S1 comparison.** AlignedCarry is identical on **3901/3901** expanded-corpus splits. The original 3344 splits and all 557 v2 splits are identical. No S1 residual class remains, and no upstream issue draft needs filing. The simulated attach handoff is **480/480** identical both without resize and with an ordered resize epoch. Production §5.2 attach/reconnect remains provisional pending a real transport/atomic subscription test and phase-aware side-effect handling.

"Identical" retains the S1 definition: normalised viewport cells (including OSC 8 URI), selected `TermState` fields, and right-trimmed history text. `wrap_flags_equal` is a separate diagnostic and does not alter that definition.

## Round 2 measurements

| Strategy | Baseline S1b run6 identical | Round 2 run5 identical | Final identical | Immediately faithful at restore | Cases |
|---|---:|---:|---:|---:|---:|
| Naive | 1840 | 1840 | 1840 | — | 3901 |
| Aligned | 2792 | 2792 | 2792 | — | 3901 |
| AlignedUnwrap | 2782 | 2782 | 2782 | — | 3901 |
| AlignedCarry | 3828 | 3901 | **3901** | **3644** | 3901 |

The 257 AlignedCarry snapshots that need the tail to become faithful cluster in `less_prompt` (94), `alt_screen_cycle` (81), `kitty_kbd` (50), and `unicode` (19), with 13 elsewhere. In the kitty cases the immediate current flags can be D=6/7 versus C=1/2. This is an attach-time diagnostic; the final continuation comparison passes.

ECH overlaid erased empty cells with their background, skipping wide-cell spacers. The same carry snapshot now records the primary screen before alternate-screen entry. This closed the 70 immediate residuals and the three continuation-only `alt_screen_cycle` splits (1394/1395/1400). The latter were faithful on the active alternate screen but had lost empty-cell identity on the hidden primary screen; replaying the ECH-corrected primary snapshot fixed their later divergence. The tracker retains the entry mode (47, 1047, or 1049). `cases.jsonl` now includes `alt_mode_at_split` and `post_tail_first_mismatch`.

The carry path first emits Formatter VT with `unwrap(true)` and probes its row flags. If they differ, it builds a per-row variant from wrapped Formatter VT, removing CRLF only after source rows flagged wrapped and safe to join. Wide-spacer screens conservatively retain wrapped row boundaries. The earlier **whole-snapshot wrapped fallback was a reverted plan deviation**. ECH runs are followed by wrap repair on affected viewport rows. Row flags now match on **3883/3901** AlignedCarry snapshots; the remaining 18 are 6 `less`, 8 `less_prompt`, and 4 `utf8_wide_combining`. They remain diagnostic, outside S1 equality. The 300-column reflow test still passes after resizing 80×24 to 100×30.

The 14 `less`/`less_prompt` survivors occur while mode 1049's alternate screen is active. The Formatter reconstruction shifts wrapped rows on immediate restore; the later exit to the carried primary screen makes final S1 state equal. The four `utf8_wide_combining` survivors are splits 3537/3549/3557/3564: Formatter wide-spacer rows do not reconstruct their wrap metadata even when text and history match. Compact (≤200-byte) reproductions of these two survivor mechanisms have not been isolated; the exact corpus prefixes and effective splits are in the final `cases.jsonl`. This leaves that requested diagnostic artifact open.

| Handoff variant | Identical | Cases | Remaining mismatch | Client PTY reply attempts |
|---|---:|---:|---|---:|
| Plain tail | **480** | 480 | none | 0 |
| Tail with resize epoch | **480** | 480 | none | 0 |

The former `wrap_pending` resize failures at splits 679, 681, and 688 now match. The short query filter includes secondary DA (`CSI > c`), which caused two reply attempts in an intermediate run. Zero attempts applies to the measured streams and does not establish a general emulator reply-disable API. Bell/title/clipboard callbacks remain inert in the simulation; phase-aware suppression, network delivery, and atomic replay-subscription races were not tested. The final matrix spent 5.74 s in snapshot-plus-restore calls across AlignedCarry cases, versus 4.50 s after the previous fallback and 3.75 s before it. The prior `snapshot_full` path used two or three temporary restores; this implementation uses three on the primary attempt and up to five when it retries, including at every alternate entry in `Tracker::feed`.

Final code-commit artifacts (8eaf614): `/data/scratch/s1/results/2026-10-08-s1b-r2-final/matrix/` and `/data/scratch/s1/results/2026-10-08-s1b-r2-final/handoff/`. Each `summary.md` header records the hash. Intermediate round-2 run1 was **3104/3901**, including 765 scrollback-class failures; run2 fixed ECH touching wide spacers, run3 closed the corpus before unwrapped emission, and run4 exposed 14 unwrap regressions. The final per-row approach retains the unchanged comparison.

## Round 1 baseline (historical)

The following setup, matrix, and taxonomy describe the 2026-10-07 S1b run6 before the round-2 repair. They are retained for comparison and do not describe the final 8eaf614 artifacts.

## Setup and method

| Item | Value |
|---|---|
| Host | archbox, Linux x86_64 |
| Emulator | `libghostty-vt = 0.2.2`, Ghostty `a887df42c56f6de86c0fe6da9c4eeca37931e083` |
| Build | Rust 1.96, Zig 0.15.2 sourced through `spikes/s1/build-env.sh` |
| Terminal size | recorded 120×40; generated 80×24 except `wrap_pending` 120×24 |
| Scrollback | `--scrollback 2000`; **bytes**, not lines, in this crate version |
| Splits | S1 seeded split plan, seed 1, ≤400 points per stream; Aligned variants snapshot at the last complete VT boundary and carry all bytes after it |
| Snapshot | Formatter `Format::Vt` reconstruction plus primary-screen prefix, trailing-row correction, per-cell OSC 8 overlay, saved cursor/pen, kitty stack, title/pwd, origin-safe cursor, pending wrap, active link and mode reconciliation |
| Result paths | `/data/scratch/s1/results/2026-10-07-s1-run6/`, `/data/scratch/s1/results/2026-10-08-s1b-run1/`, `/data/scratch/s1/results/2026-10-08-s1b-run5/`, `/data/scratch/s1/results/2026-10-08-s1b-run6/` |

Run1 exposed a carry padding bug (2987/3344), so non-overwriting correction runs 2–5 were made. Run5 is the final original-corpus measurement. Run6 adds the four v2 recordings; its 73 residuals are the same generated-stream cases as run5. The output files (`cases.jsonl`, `summary.md`, `taxonomy.md`) are under each result path. Run6 from 2026-10-07 is the original S1 baseline, a different dated directory.

## Split matrix

| Strategy | S1 run6 (3344) | S1b run1 (3344) | S1b run5 (3344) | S1b run6 (3901) |
|---|---:|---:|---:|---:|
| Naive | 1688 | 1688 | 1688 | 1840 |
| Aligned | 2628 | 2628 | 2628 | 2792 |
| AlignedUnwrap | 2618 | 2618 | 2618 | 2782 |
| AlignedCarry | — | 2987 | **3271** | **3828** |

The baseline columns remain exactly equal across all original-corpus runs. Run6's extra 557 cases change their totals, so only the 3344-case columns are directly comparable to S1 run6.

### Final taxonomy (S1b run6)

First-match class counts; the classifier can call an alt-screen tail `alt_screen` even when the actual mismatch is a styled blank on the primary screen.

| Class | Naive | Aligned | AlignedUnwrap | AlignedCarry |
|---|---:|---:|---:|---:|
| Identical | 1840 | 2792 | 2782 | 3828 |
| Parser state split | 1526 | 0 | 0 | 0 |
| Alt screen | 246 | 502 | 502 | 31 |
| Trailing row drop | 7 | 7 | 6 | 0 |
| Scrollback | 22 | 122 | 123 | 0 |
| Modes | 0 | 0 | 0 | 0 |
| Cursor | 2 | 2 | 2 | 0 |
| Title/pwd | 21 | 37 | 37 | 0 |
| Kitty keyboard | 0 | 0 | 0 | 0 |
| Style only | 2 | 3 | 3 | 1 |
| Text | 235 | 436 | 446 | 41 |

All 73 AlignedCarry residuals occur in `alt_screen_cycle` (48) and `erase_insert_delete` (25). The five specific carry paths in PLAN §6 have focused tests: alt exit to primary, OSC 8 URI and continued open link, DECSC/DECRC with pen, kitty stack pop behavior, and pending wrap including a wide final glyph. DECOM cursor handling, combined private-mode alt entry/exit, and trailing blank history rows have focused tests as well. The comparison still sees 47 styled-empty-vs-space cases and 25 erased-cell cases. Three alt-cycle cases have a faithful immediate snapshot, then diverge after continuation; their cause is not established. [Residual analysis and draft upstream issues](S1b-residuals.md) give byte-level reproductions under 200 bytes and expected/actual cells.

## Corpus v2

The original seven `.bin` recordings were unchanged. New recordings and hashes are in [corpus/README.md](../../spikes/s1/corpus/README.md). Each prompt-first stream starts in an interactive bash prompt, runs `ls --color=always` and `echo $RANDOM`, enters vim/less/btop, quits, and runs a final command. `nested2` contains the inner `ls` and `git -C … status` output that was missing from `nested.bin`. The loader reads `skip.txt`; it excludes the 1.3 KB partial Claude trust-dialog capture. No Claude Code TUI stream was recorded because trust acceptance was not confirmed.

| New stream | Splits | Aligned | AlignedCarry |
|---|---:|---:|---:|
| `btop_prompt` | 140 | 8 | **140** |
| `less_prompt` | 140 | 11 | **140** |
| `vim_prompt` | 140 | 8 | **140** |
| `nested2` | 137 | 137 | **137** |

## Handoff simulation

`/data/scratch/s1/results/2026-10-08-s1b-run5/handoff/` has `cases.jsonl` and `summary.md`. For each of 20 original-corpus streams, 20 seeded aligned splits were tested twice: plain tail and a tail with an ordered resize epoch to 100×30. The worker consumed the tail while the fixed snapshot was in flight; the client attached at the snapshot size and applied bytes after its offset. `on_pty_write` callbacks counted reply attempts. The client filtered a short list of recognized query units before feeding its replica. The current code also buffers queries split across delivery calls; the saved 800-case artifact predates that fix. An inert callback emits no client PTY writes in this simulation, but the zero-attempt measurement covers only the tested streams and query filter. This is not a proven generic reply-disable API.

| Variant | Cases | Identical | Mismatches | Client PTY reply attempts |
|---|---:|---:|---:|---:|
| Plain tail | 400 | 385 | 15 (13 text, 2 alt) | 0 |
| Tail with resize epoch | 400 | 309 | 91 (55 scrollback, 34 text, 2 alt) | 0 |
| Total | 800 | 694 | 106 | **0** |

The plain-tail mismatches match the styled-blank residual classes. The resize mismatches include history row-count changes (`gitlog`, `utf8_wide_combining`, `resize_marker`) and wrap/reflow text changes (`wrap_pending`, Unicode). They are **not** accepted as covered by the Formatter issue drafts; resize continuation needs its own repair or an explicit product limitation. The simulation observed 586 bell/title/clipboard callback invocations with inert callbacks; it does not distinguish snapshot replay from live delivery or establish phase-aware side-effect suppression. It does not perform network delivery, replay retention, or atomic subscription races.

## Historical gate status after round 1

At the end of round 1, PLAN §6 S1b asked for all-stream equality or a filed upstream issue for each remaining Formatter limit. The original corpus still had 73 residuals, issue drafts were not filed, and resize handoff had additional gaps; the gate was open then. The round-2 results above supersede that status. Windows MSVC, a real Claude TUI recording, graphics, a 10k-line scrollback performance run, and query suppression beyond the simulated filter remain unmeasured.
