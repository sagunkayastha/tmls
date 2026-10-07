# PROGRESS — tmls-rs (2026-10-07)

Quick session log. Newest entry first.

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
