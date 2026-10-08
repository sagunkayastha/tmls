# S1b residual analysis — round 2 (2026-10-08)

The **unchanged S1 comparison is identical on all 3901/3901 AlignedCarry splits** in `/data/scratch/s1/results/2026-10-08-s1b-r2-final/matrix/` at code commit `8eaf614`. The 73 round-1 residuals are closed. There is no remaining S1 class and no upstream issue draft to file.

| Round-1 class | Cases | Round-2 resolution |
|---|---:|---|
| Styled empty vs printed space, including alternate-screen exits | 47 | ECH overlay restores empty-cell identity; the ECH-corrected primary snapshot is carried through alt entry. |
| Erased-cell background / empty identity | 25 | ECH overlay uses the source background and skips wide-cell spacers. |
| Background-only style mismatch | 1 | ECH overlay restores the erased background. |

The three `alt_screen_cycle` splits 1394, 1395, and 1400 were faithful immediately after restore on the active alternate screen, then diverged after exiting mode 47. Their hidden primary snapshot had the same Formatter empty-cell loss. Replaying the full ECH-corrected primary snapshot closes them. The focused repros in `experiment.rs` now assert identical results. The saved `AltEntry.mode` remains the mode used for re-entry; the pinned Ghostty source copies the cursor on 47/1047 screen transitions and saves/restores it on 1049.

## Remaining diagnostic and attach limits

The final simulated handoff is **480/480** identical without resize and **480/480** after a 100×30 resize. The previous `wrap_pending` splits 679, 681, and 688 now pass. This is still an in-process simulation, not atomic transport proof.

Row-wrap flags disagree on **18/3901** snapshots (6 `less`, 8 `less_prompt`, 4 `utf8_wide_combining`) even though final S1 identity holds. The alternate-screen `less` snapshots have immediate row shifts in Formatter reconstruction that disappear after exit to the carried primary screen. The wide-spacer snapshots retain text/history but lose row-wrap metadata. Exact corpus split and effective-split pairs are listed in the final `cases.jsonl`; a ≤200-byte reproducer for these remaining diagnostic cases was not isolated, so the requested compact repro remains open.

The pinned Ghostty has one saved-cursor slot **per screen**. In the focused 1049 test, ESC 7 at (3,2), movement, and `1049h` result in (1,1) after both `1049l` and a later ESC 8 in the reference and restored terminals: 1049 entry overwrites the primary slot. This differs from the proposed (3,2) expectation. `Tracker` still has a single `saved_cursor` and `kitty_stack`; their per-screen behavior remains open beyond the tested transitions. Repeated 47 entry/exit, 1047 re-entry with a cleared alternate screen, and a 1049 entry followed by 47 exit match the reference in focused tests.

The handoff client query filter is a short list. It removes CSI 5n/6n, primary/secondary DA, DECRQM, and XTVERSION in the measured paths. It does **not** simulate kitty `CSI ?u`, OSC 10/11/4, DECRQSS, XTGETTCAP, CSI ?6n, CSI =c, or CSI 14t/18t. `Client::attach` also passes snapshot bytes through this filter. Thus zero client PTY reply attempts on these streams is not evidence of general reply suppression. History rows also retain Formatter empty-cell/background loss until a taller resize pulls them into the viewport; this remains outside the measured comparison.

Round-1 Drafts A and B are superseded by the ECH and primary-snapshot repair. No issue has been filed.
