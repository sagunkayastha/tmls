# S1b independent review — Codex gpt-6-sol (2026-10-08)

Scope: read-only review of `spikes/s1/src/carry.rs`, `handoff.rs`, the S1b results, and saved run6 artifacts against the implementation plan and PLAN §5.1/§5.2. The reviewer did not run tests or reproduce the upstream issue drafts.

## Initial findings

No Critical findings. S1b is correctly left open.

1. **Important — residual causation:** Three of 73 run6 AlignedCarry failures (`alt_screen_cycle` splits 1394, 1395, 1400) have `snap_faithful=true` and diverge only after continuation. These are not proven Formatter output failures; 47-mode or unmeasured state may be involved. The ignored alt repro does not isolate that cause.
2. **Important — combined private modes:** `Tracker::before_unit` parsed the whole parameter field as one `u16`, missing sequences such as `CSI ? 1049 ; 25 h` and their resets. The emulator enters alt without a captured primary prefix.
3. **Important — reply authority:** `Client::apply_bytes` filtered a fixed query list; an unlisted reply-producing query can reach the callback. The 0/800 measurement covers the corpus and filter, not general one-reply authority.
4. **Important — side effects:** Bell/title/clipboard callbacks were installed before snapshot parsing and stayed installed for the tail. The 586 count is observed inert callback invocations, not proof of replay suppression or live-only delivery.
5. **Minor — scrollback parameter:** `snapshot_full` uses 2000 for its temporary baseline restore even though `Worker::new` accepts other scrollback limits. The matrix used 2000.

The reviewer assessed the 3828/3901 expanded split result, including 557/557 new recording splits, as strong measured support for the carry design under the S1 comparison. Resize 309/400 and the residuals keep §5.2 provisional. The sequential probe does not establish atomic subscription, retention, concurrent encoding, or lock behavior.

## Fix wave and disposition

- Added a failing regression for combined 1049/25 entry and exit, then parsed private-mode parameters individually. The focused test now passes.
- Added a failing regression for DSR split across two `Client::apply` calls, then buffered incomplete VT units. The focused test now passes. The finite query filter remains a stated limitation.
- Corrected residual, results, PLAN, and PROGRESS wording to distinguish three continuation-only failures and to call the 586 count observed callback invocations. The probe still does not implement phase-aware delivery or a general reply-disable mechanism; S1b remains open.
- The scrollback baseline remains at 2000; all reported matrix and handoff runs use that value. Other configurations are unverified.

## Re-review

The same reviewer performed a read-only re-review after the fix wave. There are no remaining Critical or review-blocking Important findings. The combined-mode regression covers the alt-entry gap, the client buffers incomplete VT units across delivery calls, and the documents distinguish measured results from proposed causes and unproven production suppression. The hard-coded 2000-byte baseline remains Minor and outside the measured configuration. The review gate may close **with S1b explicitly open**; 73 residuals, resize mismatches, and production handoff guarantees remain unresolved. The reviewer relied on the reported passing tests.
