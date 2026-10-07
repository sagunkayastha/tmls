# Codex (gpt-6-sol) review — S0 harness, 2026-10-07 (before the 10k matrix results)

Brief: `.superpowers/sdd/2026-10-07-s0-key-loss/codex-harness-brief.md` (harness code on branch spike/s0 at d69ca3f).

## Verdict
Partial S0 measurement. Scripted PTY injection exercises tmls's input path but not a keyboard or a GUI terminal; cannot give key-to-present; does not close the full PLAN §6 S0 gate.

## Critical
1. A complete-looking row can describe an incomplete run — report does not require `sent == events`, recv `exit == quit_after`, `saw_done`. Show completeness before treating a row as a gate result. (paths.sh:212, report.rs:36/121)
2. Ack is not a reliable tmls delivery oracle (stream scanning vs cell redraw; can hide or fabricate a match). Use the receiver log for loss/corruption; call ack percentiles "observed return-output timing" with sample count + dropped-ack count.

## Important
- recv `--quit-after` counts parsed markers, not distinct in-range seqs → duplicates/out-of-range can stop early; report `arrived` includes out-of-range seqs. (recv.rs:68, report.rs:42)
- Final ack can race receiver-log completion: after all acks send waits only 100 ms then kills. Wait for S0DONE. (send.rs:166, recv.rs:68)
- Latency = wall clock at two observation points; all markers in one read share a timestamp; cross-host skew; SystemTime adjustable. Keep the caveat beside every comparison.
- Flood shares the ack queue → flood can selectively drop ack samples; the Markdown row omits dropped_acks.

## Minor
- Tokenizer attributes all bytes since the previous marker to the next marker (lost marker → next event "corrupt" with concatenated payload).
- corrupt_examples: expected includes marker, got excludes it — align.
- tmls setup kills any `s0-recv` session by name (ownership by name only); nav depends on an unscrolled list.

## Unmeasured
Real keyboard / GUI terminal encoding / focus / IME / Windows laptop; Esc→key interval not varied; no presentation timestamp (30 Hz redraw contribution not measurable from these logs).

## Suggested controls
- Repeat P1/P2 at several rates and with jittered intervals; `ssh -t` vs no-remote-PTY control.
- Separate real-keyboard run into a bare receiver; separate presentation measurement.
- A recorded Claude-output workload alongside the regular flood.

## Wording for results
"Receiver-observed delivery for scripted PTY input"; report completion status, denominators, dropped acks, host direction, skew caveat; never call ack RTT one-way latency; do not infer visible-response latency or the cause of a missed physical key.
