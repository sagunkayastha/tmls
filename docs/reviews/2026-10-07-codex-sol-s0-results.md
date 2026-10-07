# Codex (gpt-6-sol) review — S0 results doc, 2026-10-07 (branch spike/s0 @ 43f8e13)

Brief: `.superpowers/sdd/2026-10-07-s0-key-loss/codex-results-brief.md`.

## Verdict
Useful evidence for receiver-observed delivery of scripted PTY input; does not close the full S0 gate. Counts/timings trace to the tables; corrections are about interpretation strength.

## Must fix before relying on the doc
1. Reconcile the completeness rule with the conclusions: runner README says only `complete=yes` rows are gate results, yet every newer tmls flood row is `NO:no_done` (sent=10000, arrived=10000, recv_exit=quit_after, saw_done=false). Label those as receiver-log evidence outside the formal completion gate, or revise the rule and justify. Older A/B/C rows lack completeness fields — say so per row.
2. Narrow "every ssh hop adds 0–20 ms" to the measured path (loopback, interactive ssh); cross-host one-way carries unrechecked skew; LAN on/off pair not run. Describe P5's extra-hop difference as *consistent with* it.

## Should fix
- Textual hold + next-key merge: a supported hypothesis, not proven — rate and next-key interval co-vary; CtrlLetter always follows AltLetter. Carry the qualification into Summary and PLAN §1.
- Ctrl+P: compelling (cutoff at event 59 on all three tmls paths) but no palette-disabled control yet; "from event 59 onward" (141 missing include event 59).
- Flood conclusion: receiver timing only; tmls flood ack p50 from 1,855/10,000 samples, dropped_acks=0 — not comparable with quiet ack percentiles; keep the limit attached to any short form.

## Minor
- PLAN heading still says S0 is "next".
- Cite the gap control (`expected_hex=1b66` for Alt+f) when naming the Alt+f→Ctrl+Right mapping.

## Still unmeasured
Cause of missed *physical* keys; key-to-photon latency; GUI/keyboard/IME/Windows paths. Marker-gap interpretation (one-way from marker vs payload; 2.0 ms marker-based = 122.2 ms from payload) confirmed correct.

## Recommended next controls
1. PC morning: P6-kitty-local/-ssh/-tmls from a kitty window (check the `j` count vs the live tmls list first). Still does not exercise kitty's keyboard handling.
2. `ydotool type` into a focused kitty + bare `s0 recv` for keyboard→app delivery.
3. Mechanisms: rerun Ctrl+P stream with the palette disabled; vary Esc→next-key interval independently of rate and marker gap (and lower ESCDELAY); obfuscation on/off across the LAN with fresh skew samples.
