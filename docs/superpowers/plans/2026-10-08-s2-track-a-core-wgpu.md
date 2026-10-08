# S2 Track A — `s2-core` + `s2-wgpu` (for Codex)

> **Read first:** `docs/superpowers/plans/2026-10-08-s2-split-overview.md` (rules, ownership, sync points, **shared
> contract**) and the full S2 plan `docs/superpowers/plans/2026-10-08-s2-interaction-slice.md` (Tasks 1–3 give detail;
> this file overrides it where they differ). Spec: `docs/PLAN.md` §2, §3 Desktop GUI, §5.1, §6 S2, §9, §10.

**Branch:** `spike/s2-wgpu` from `main` after Task 0. **You own:** `spikes/s2-core/**`, `spikes/s2-wgpu/**`.
Do not edit root `Cargo.toml`, `tooling/docker/**`, `spikes/s2-gpui/**`, `spikes/s2-bench/**`.
If the contract must change, edit the overview's contract section in the same commit and say so in the commit body.

Commit author `sgkayast <sgn.kayastha@gmail.com>`, no AI attribution. No `/tmp` (use `/data/scratch/s2/`). No deletes/moves
without the user's yes. PC jobs: write the exact commands, then wait for the user's yes.

---

## A1 — `s2-core` (S2 plan Task 1) — **priority: merge first, Track B waits on it**

Replace the Task 0 stubs with the real implementation of the contract API.

- Modules: `pty.rs`, `emu.rs`, `frame.rs`, `keys.rs`, `latency.rs`, `bench.rs`, `lib.rs`.
- `Emu` thread owns `libghostty_vt::Terminal` + `RenderState` + `key::Encoder` (not `Send`; create them on the thread).
  It reads PTY bytes (reader thread → `InputMsg::Bytes` with `t_output_ns` taken right after `read()` returns), feeds the
  terminal, builds a `Frame` from `RenderState` dirty rows, publishes to `FrameSlot` (latest wins), calls the waker.
- `on_pty_write` replies → the same `PtyWriter` as keys (single input path, PLAN §5.1).
- Keys: `KeyEvent` → `key::Encoder` honouring the terminal's kitty flags + DECCKM; `Paste` → bracketed when mode 2004.
  Log `write { id, t_write_ns }` right after the PTY write returns. Attribute keys to frames per the contract rule.
- Repeat safety: a focus-lost marker drops queued repeat events (`keys_no_repeat_after_focus_loss`).
- `latency::report(path)` → p50/p95/p99 for key→write, write→output, output→submit, submit→present, **key→present**
  (used by tests; Track B's `analyze.py` is the canonical analysis — keep the numbers equal on the same log, see A1 step 4).
- `bench::Sampler` (500 ms): `/proc/self/stat` CPU %, `/proc/self/status` VmRSS, fps from frames logged, `nvidia-smi
  --query-gpu=utilization.gpu,power.draw --format=csv,noheader,nounits` if on PATH else `null`.

Steps:
- [ ] 1. Failing tests: `pty_echo_roundtrip` (spawn `cat`), `emu_frame_after_output` (feed `printf ok` → row 0 has "ok";
      100 feeds → ≤100 frames, last frame = final state), `keys_encode_letter_and_ctrl` (`a`→`a`, Ctrl+C→`\x03`,
      Up+DECCKM→`\x1bOA`), `keys_no_repeat_after_focus_loss`, `paste_bracketed_when_mode_set`, `reply_goes_through_writer`
      (`printf '\e[6n'` → a CPR reply appears on the PTY writer, nowhere else), `latency_report_percentiles` (synthetic).
- [ ] 2. Implement until green on archbox (`source spikes/s1/build-env.sh; cargo test -p s2-core`).
- [ ] 3. Write the golden log `spikes/s2-core/tests/data/golden-latency.jsonl` (~50 keys, hand-checkable numbers) and its
      expected report `golden-report.json` — Track B tests `analyze.py` against the same pair.
- [ ] 4. Commit `s2-core: PTY + emulator thread + Frame + key encoding + latency log + sampler`. Tell the user: **"A1 ready
      for Claude review"** — stop until it is merged to `main` (Claude reviews + merges; B rebases).

## A2 — `s2-wgpu` grid + input + latency marks (S2 plan Task 2)

Needs: A1 merged; B1 Docker image (GUI libs) built on the PC. Until B1 lands, build and test on archbox (headless tests
only; `cargo build -p s2-wgpu` needs the system wayland/xkb dev libs — if missing on archbox, compile-check with
`cargo check` and leave the full build to Docker).

- winit 0.30 `ApplicationHandler`, wgpu surface (`--present`, default `fifo`), log `refresh_rate_millihertz` + scale in meta.
- Atlas: `cosmic-text` + `swash`, key (font, size, glyph, subpixel x, scale); instanced bg quads + glyph quads; wide cells
  span 2 columns; cursor overlay; fallback fonts.
- Input: winit `KeyEvent` → contract `KeyEvent` (log `key` at `window_event` entry); `Ime::{Preedit,Commit}` (preedit
  drawn underlined at cursor; commit → bytes); mouse select (cell coords) + `Ctrl+Shift+C/V` via `arboard`; wheel → viewport.
- Marks: `t_submit_ns` at `queue.submit`; one `frame` line per frame with `t_present_ns` = `present()` return
  (`present_kind:"present_return"`) plus an extra field `t_work_done_ns` from `queue.on_submitted_work_done`
  (the contract allows extra fields). Neither is photon — say so in results.
- `--photon` square drawn in the same frame as the echo. All CLI flags per the contract.

Steps:
- [ ] 1. Failing tests: `atlas_cache_and_scale_invalidate`, `shader_validates` (naga), `cell_quads_for_wide_char_span_two_columns`.
- [ ] 2. Implement → green.
- [ ] 3. PC build (**ask user**): `ssh pcl 'cd ~/Others/tmls-rs && git fetch && git checkout spike/s2-wgpu && tooling/docker/build.sh build --release -p s2-wgpu && ldd target-docker/release/s2-wgpu | grep "not found"'` → must print nothing.
- [ ] 4. **User smoke run at the PC** (ask; user present): `~/Others/tmls-rs/target-docker/release/s2-wgpu --bench ~/s2-logs/wgpu-smoke`
      → type a few lines, resize, quit. Copy log to archbox `/data/scratch/s2/wgpu-smoke/`; check: frames arrive, key→present
      p50 < 1 frame, no panic. Iterate until it passes.
- [ ] 5. Commit `s2-wgpu: winit+wgpu terminal grid, glyph atlas, input, IME preedit, latency marks, photon square`.

## A3 — `s2-wgpu` interaction slice (S2 plan Task 3)

Sidebar list (≥ 30 items, keyboard + mouse), "New session" form with ONE editable text field + Create/Cancel, focus
traversal Tab/Shift-Tab (list ↔ field ↔ terminal), right-click context menu on terminal (Copy / Paste / Clear), theme
tokens, font zoom Ctrl+= / Ctrl+−, HiDPI re-rasterise on scale change. Text field: caret, selection, IME preedit.

- [ ] 1. Failing tests for the pure logic (focus ring order, text-field edit ops incl. wide chars + preedit, menu hit-test).
- [ ] 2. Implement → green → Docker build on PC (ask).
- [ ] 3. **User checklist at the PC** (ask): fcitx5 pinyin compose + commit in the field; select a wide-char line in the
      terminal + copy; paste into the field; move window to the other monitor (if any); set scaling 125 % → glyph
      crispness + latency (bench log). Record answers in `docs/spikes/S2-checklist-wgpu.md`.
- [ ] 4. Record implementation cost in `docs/spikes/S2-checklist-wgpu.md`: LOC (`tokei spikes/s2-wgpu` or `wc -l`), hours,
      upstream workarounds (list each with link).
- [ ] 5. Commit `s2-wgpu: sidebar list, editable field, focus traversal, context menu, zoom, HiDPI`.
- [ ] 6. One `PROGRESS.md` entry for Track A. Tell the user: **"Track A ready for review"**.

## Done when

`cargo test -p s2-core -p s2-wgpu` green; Docker release build on PC with empty `ldd | grep "not found"`; smoke log +
checklist file present; contract unchanged (or changes recorded in the overview).
