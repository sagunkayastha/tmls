# S1b residual analysis — round 2 (2026-10-08)

The **unchanged S1 comparison is identical on all 3901/3901 AlignedCarry splits** in `/data/scratch/s1/results/2026-10-08-s1b-r2-run5/`. The 73 round-1 residuals are closed. There is no remaining S1 class and no upstream issue draft to file.

| Round-1 class | Cases | Round-2 resolution |
|---|---:|---|
| Styled empty vs printed space, including alternate-screen exits | 47 | ECH overlay restores empty-cell identity; the ECH-corrected primary snapshot is carried through alt entry. |
| Erased-cell background / empty identity | 25 | ECH overlay uses the source background and skips wide-cell spacers. |
| Background-only style mismatch | 1 | ECH overlay restores the erased background. |

The three `alt_screen_cycle` splits 1394, 1395, and 1400 were faithful immediately after restore on the active alternate screen, then diverged after exiting mode 47. Their hidden primary snapshot had the same Formatter empty-cell loss. Replaying the full ECH-corrected primary snapshot closes them. The focused repros in `experiment.rs` now assert identical results. The saved `AltEntry.mode` remains the mode used for re-entry; the pinned Ghostty source copies the cursor on 47/1047 screen transitions and saves/restores it on 1049.

## Remaining resize handoff limit

The final simulated handoff is 480/480 identical without resize and 477/480 after a 100×30 resize. The three mismatches are `wrap_pending` splits 679, 681, and 688. Each first differs at viewport cell (20,1): reference `"m"`, restored client empty. They are reflow text mismatches, outside the all-stream S1 corpus gate. The 28 AlignedCarry snapshots with unequal row-wrap flags remain diagnostic evidence; the unchanged S1 comparison still matches for all of them. The resize cases need further repair before production attach/reconnect is frozen. They are not attributed to an upstream Formatter limit, so there is no Draft C.

Round-1 Drafts A and B are superseded by the ECH and primary-snapshot repair. No issue has been filed.
