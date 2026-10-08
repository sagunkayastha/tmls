# S2 Interaction Slice Implementation Plan

> **For agentic workers (Codex / DeepSeek):** execute task-by-task; each task ends with tests/builds passing and one commit. Steps use checkbox (`- [ ]`) syntax. Author every commit as `sgkayast <sgn.kayastha@gmail.com>`; no AI attribution lines. Builds run in the PC's Docker image (`tooling/docker/build.sh`, no host installs); **running a GPU window happens only on the user's PC desktop with the user present and driving** — agents prepare binaries, scripts and instructions, then wait. Never write under `/tmp`. Never delete or move files without explicit consent.

**Goal:** Decide the desktop GUI stack for tmls-rs (PLAN §3 "Desktop GUI", gate S2) with measurements on the real PC: raw `winit`+`wgpu` vs `gpui`, each rendering the same interaction slice — a libghostty-vt-backed terminal grid attached to a local PTY, a sidebar list, one editable text field, mouse selection + clipboard, IME preedit — compared on key-to-present latency (p50/p95/p99 at 60 Hz and the monitor's max refresh), a photon check, CPU/RSS/GPU idle and streaming cost, text quality, and implementation coverage/cost.

**Architecture:** Shared crate `spikes/s2-core` (no GUI): PTY (`portable-pty`) + libghostty-vt `Terminal` on its own thread, `RenderState` → a `Frame` (rows × cells, cursor, dirty flags), `key::KeyEncoder` mapping from a toolkit-neutral `KeyEvent`, latency log (`key_id, t_key_received, t_bytes_written, t_pty_output, t_frame_submitted, t_frame_presented`), a `--bench` mode that reads `/proc/self` CPU/RSS and `nvidia-smi` GPU util. Two front-ends: `spikes/s2-wgpu` (winit 0.30 + wgpu 30 + `cosmic-text`/`swash` glyph atlas, own tiny UI layer) and `spikes/s2-gpui` (gpui, terminal grid as a custom element, native widgets for sidebar/field). A `s2-bench` script drives `ydotool` key injection (proven in S0/K1) against the focused window and collects the logs; photon check = top-left square flips white on each key, filmed with a phone at 240 fps.

**Tech Stack:** Rust 1.96; `winit = "0.30"`, `wgpu = "30"`, `cosmic-text`, `swash`, `arboard` (clipboard), `portable-pty`, `libghostty-vt = "=0.2.2"` (Zig 0.15.2), `gpui` (Zed, Apache-2.0 — git dependency pinned to a commit; check its wayland/vulkan deps build in the image). Docker image additions: `libxkbcommon-dev libwayland-dev libvulkan-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev pkg-config cmake clang` (gpui needs more — discover and add). Runtime on the PC host: Vulkan via the NVIDIA driver, Wayland (KDE), `fcitx5` for IME, `ydotool` for injection.

**Spec:** `docs/PLAN.md` §2 (native executable with its own GPU window; performance and visuals first), §3 Desktop GUI row, §6 row S2 ("Grid + sidebar + one editable form + selection + clipboard + IME (fcitx5, Windows TSF), in raw wgpu vs gpui; 60 Hz + high refresh, fractional scaling, monitor move. Pass: p50/p95/p99 key-to-present (and camera-checked photon sample), CPU, idle power, text quality; pick on latency **and** coverage"), §9 benchmark contract, §10 (Wayland + NVIDIA frame pacing). Codex review: `docs/reviews/2026-10-07-codex-astra.md` Important #4 (the slice must include the expensive parts: editable fields, IME, selection, focus, menus, DPI changes; `present()` is not photon).

## Global Constraints

- Linux/Wayland on the PC first; Windows TSF deferred (laptop). Note it in results.
- One canonical size per terminal; the client emulator **never writes to the PTY** except through the single input path (PLAN §5.1) — enforce with an `on_pty_write` callback that routes replies through the same writer (this is the authoritative emulator for a *local* PTY in S2; fine).
- Frame budget targets to report against (not pass/fail): 60 Hz → 16.7 ms; monitor max (record Hz from `kscreen-doctor -o` or `wlr-randr`); key-to-present p99 budget "≤ 1.5 frames".
- Every measurement run records: binary git hash, build profile (`--release`), monitor refresh Hz, scaling factor (`kscreen-doctor -o`), compositor (`plasmashell --version`, `kwin_wayland --version`), NVIDIA driver version (`nvidia-smi`), GPU power state, whether `game-mode status` is off.
- No host installs on the PC. Binaries are built in Docker (`tooling/docker/build.sh build --release -p s2-wgpu`) and copied to `~/Others/tmls-rs/target-docker/release/` (already on the PC via the bind mount). If a binary fails to start on the host for a missing shared lib, report the `ldd` output — do not install libs.
- The PC is used only when the user says yes for that run (PC rule); agents send the exact command list and wait for the logs.
- All logs under `~/s2-logs/` on the PC; copied to `/data/scratch/s2/` on archbox for analysis.

## Review Focus

1. **Key pressed while the terminal is scrolling a flood** — expected: key-to-present p99 reported separately for quiet vs flood; the flood never starves input (Task 2 instrumentation + Task 6 run matrix).
2. **IME composition (fcitx5, e.g. pinyin) in the terminal and in the text field** — expected: preedit shown inline, commit sends the composed string once, no stray bytes (Task 3/4 manual checklist with expected bytes logged by the core).
3. **Fractional scaling (125%/150%) and moving the window to a monitor with a different Hz** — expected: no blurry glyphs (atlas re-rasterised at the new scale), no latency cliff (Task 6 checklist).
4. **Selection + copy of a line containing wide chars and a trailing-space run** — expected: copied text matches the grid text, trailing spaces trimmed (Task 3/4 tests on the shared selection model).
5. **Window loses focus mid-key-repeat** — expected: no repeated keys after focus loss (Task 2 test on the key pipeline with synthetic events).

---

### Task 1: `spikes/s2-core` — PTY + emulator thread + Frame + latency log

**Files:**
- Create: `spikes/s2-core/Cargo.toml`, `src/lib.rs`, `src/pty.rs`, `src/emu.rs`, `src/frame.rs`, `src/keys.rs`, `src/latency.rs`, `src/bench.rs`; modify root `Cargo.toml` members; `tooling/docker/Dockerfile.build` (dev libs listed above)
- Test: unit tests per module + one integration test spawning `sh -c 'printf "ok\\n"'`

**Interfaces:**
- `pty::Pty::spawn(cmd: &[String], cols, rows) -> Pty` with `write(&[u8])`, `resize(cols, rows)`, `reader() -> impl Read + Send`.
- `emu::Emu` runs on a dedicated thread owning `Terminal` + `RenderState`; channels: `input_tx: Sender<InputMsg>` (`Bytes(Vec<u8>)`, `Resize(cols, rows)`, `Snapshot`), `frame_rx: Receiver<Frame>` (coalesced: at most one pending frame; a new frame replaces the unconsumed one — rendered frames may drop, PTY bytes never). `on_pty_write` replies go to the PTY writer (single path).
- `frame::Frame { cols, rows, cells: Vec<Cell> /* reuse s1::grid::Cell shape: text, fg, bg, flags, wide, hyperlink */, cursor: Option<(u16,u16,CursorShape,bool)>, dirty_rows: Vec<bool>, seq: u64, t_output_ns: u128 /* when the bytes that produced it were read from the PTY */ }`.
- `keys::KeyEvent { key: Key /* toolkit-neutral enum: Char(char), Named(Named), … */, mods: Mods, text: Option<String>, repeat: bool, state: Pressed|Released }` → `keys::encode(ev, &KittyFlags, &Modes) -> Vec<u8>` using `libghostty_vt::key::KeyEncoder` (respect kitty flags from the emulator and DECCKM/bracketed paste modes); `keys::paste(text) -> Vec<u8>` (bracketed if mode 2004).
- `latency::Log` (append-only JSONL): `key { id, t_key_ns }`, `write { id, t_write_ns }`, `frame { seq, t_output_ns, t_submit_ns, t_present_ns, keys: Vec<id> /* keys whose echo is in this frame — attribute by "first frame submitted after t_write" */ }`; `latency::report(path) -> p50/p95/p99 of (t_present − t_key)` and of (t_write − t_key), (t_output − t_write), (t_submit − t_output), (t_present − t_submit).
- `bench::Sampler` every 500 ms: process CPU %, RSS, `nvidia-smi --query-gpu=utilization.gpu,power.draw --format=csv,noheader` (if available), frames/s.

- [ ] **Step 1: Failing tests** — `pty_echo_roundtrip` (spawn `cat`, write "hi\n", read back), `emu_frame_after_output` (feed bytes → a Frame with "ok" in row 0 arrives; coalescing: 100 feeds → ≤ 100 frames and the last frame has the final state), `keys_encode_letter_and_ctrl` (`a` → `b"a"`, Ctrl+C → `b"\x03"`, Up with DECCKM → `b"\x1bOA"`), `keys_no_repeat_after_focus_loss` (a `Released`/focus-lost marker drops pending repeats), `latency_report_percentiles` (synthetic log → expected numbers), `paste_bracketed_when_mode_set`.
- [ ] **Step 2–4: fail → implement → pass** (archbox build is fine for s2-core: no GUI deps).
- [ ] **Step 5: Docker image rebuild on the PC with the new dev libs** (`docker build -t tmls-rs-build:latest -f tooling/docker/Dockerfile.build tooling/docker`) — run over ssh; record the time.
- [ ] **Step 6: Commit** — `s2-core: PTY + emulator thread + Frame + key encoding + latency log + bench sampler`

---

### Task 2: `spikes/s2-wgpu` — grid renderer + input + latency marks

**Files:**
- Create: `spikes/s2-wgpu/Cargo.toml`, `src/main.rs`, `src/gfx/{atlas.rs,grid.rs,shader.wgsl}`, `src/app.rs`, `src/photon.rs`
- Test: `atlas` unit tests (glyph cache hit/miss, scale change invalidates); shader compiles (`naga` validate in a test)

**Interfaces / behaviour:**
- winit 0.30 `ApplicationHandler`; `wgpu` surface with `PresentMode::AutoVsync` default and `--present immediate|mailbox|fifo` flag; `--hz` logging of `monitor.refresh_rate_millihertz()`.
- Atlas: `cosmic-text` `FontSystem` + `swash` rasterisation into an RGBA/alpha atlas texture, keyed by (font, size, glyph id, subpixel offset, scale); cell grid drawn as instanced quads (bg) + glyph quads (fg); cursor + selection overlays; wide cells span 2 columns; fallback fonts via cosmic-text.
- Input: winit `KeyEvent` → `s2-core::keys::KeyEvent` (incl. `text_with_all_modifiers`), `Ime::{Preedit, Commit}` → preedit drawn at the cursor with underline, commit → bytes; mouse: press/drag/release → selection (cell coords), `Ctrl+Shift+C`/`Ctrl+Shift+V` → `arboard`; wheel → viewport scroll (`Terminal::scroll_viewport`).
- Latency marks: `t_key_ns` at `window_event` entry; `t_submit_ns` at `queue.submit`; `t_present_ns`: after `surface_texture.present()` register `queue.on_submitted_work_done` callback timestamp **and** record `present()` return time — report both, label them (`present()` is not photon).
- `photon.rs`: `--photon` flag → a 64×64 top-left square toggles black/white on every key press (for the camera check). The square is drawn in the same frame as the key's echo.
- `--bench <log dir>` enables the latency log + sampler; `--cmd <program>` for the PTY (default `$SHELL`); `--flood` spawns `s0 flood --bytes-per-sec N` inside the PTY instead of a shell (reuse `spikes/s0` binary path via `--cmd`).
- Sidebar/form/selection polish is Task 3; Task 2 is the grid + keys + marks only.

- [ ] **Step 1: Failing tests** (`atlas_cache_and_scale_invalidate`, `shader_validates`, `cell_quads_for_wide_char_span_two_columns`).
- [ ] **Step 2–4: fail → implement → pass**; build release in Docker on the PC: `ssh pcl 'cd ~/Others/tmls-rs && tooling/docker/build.sh build --release -p s2-wgpu'`; `ldd target-docker/release/s2-wgpu | grep "not found"` on the PC host must be empty (Vulkan/Wayland are dlopen'ed by wgpu/winit).
- [ ] **Step 5: First user-run** (ask; user present): `~/Others/tmls-rs/target-docker/release/s2-wgpu --bench ~/s2-logs/wgpu-smoke` → user types a few lines, resizes the window, quits. Agent reads the log: frames arrive, p50 latency plausible (< 1 frame), no panics. Fix and iterate until the smoke run passes.
- [ ] **Step 6: Commit** — `s2-wgpu: winit+wgpu terminal grid, glyph atlas, input, IME preedit, latency marks, photon square`

---

### Task 3: `s2-wgpu` — sidebar, editable field, selection/clipboard, menus (the "expensive parts")

**Files:**
- Create: `spikes/s2-wgpu/src/ui/{mod.rs,layout.rs,list.rs,textfield.rs,menu.rs,theme.rs}`; modify `app.rs`
- Test: unit tests for the text field editing model (cursor movement, selection, IME preedit insertion, UTF-8/wide chars), list keyboard navigation, selection→clipboard text (Review Focus 4)

**Behaviour:** left sidebar (240 px) with a static list of 6 "sessions" (name, status glyph, context %), keyboard nav `j/k`/arrows + Enter, mouse click; a "New session" row opens a form with ONE editable text field (name) + Create/Cancel buttons; focus traversal Tab/Shift-Tab between list / field / terminal; a right-click context menu on the terminal (Copy / Paste / Clear); theme tokens (fonts, colours). Font zoom Ctrl+=/−. HiDPI: scale factor from winit, atlas re-rasterised on change.
- [ ] Steps: failing tests → implement → pass → Docker build → **user-run checklist** (ask): type in the field with fcitx5 pinyin, compose and commit; select a wide-char line in the terminal and copy; paste into the field; move the window to the other monitor (if any) and change scaling to 125% → report glyph crispness and any latency change (bench log). Record answers in `docs/spikes/S2-checklist-wgpu.md`.
- [ ] Commit — `s2-wgpu: sidebar list, editable field, focus traversal, context menu, zoom, HiDPI`

---

### Task 4: `spikes/s2-gpui` — the same slice in gpui

**Files:**
- Create: `spikes/s2-gpui/Cargo.toml` (gpui git dep pinned to a commit; Apache-2.0 — record licence in `docs/licences.md`), `src/main.rs`, `src/terminal_element.rs`, `src/ui.rs`
- Test: terminal element layout unit tests where gpui allows headless tests; otherwise the shared `s2-core` tests carry the logic

**Behaviour:** Same slice: terminal grid as a custom `Element` painting cells via gpui text runs (or glyph atlas if gpui exposes it), sidebar list, text input (gpui has none built in — use/adapt Zed's `editor`-less minimal input or write one; record the cost), focus, context menu, IME via gpui's input handler, clipboard via gpui. Same latency marks (gpui `on_next_frame` / present callback — document what gpui exposes; if it cannot timestamp presentation, say so and use frame-callback time). `--bench`, `--photon`, `--flood` flags identical to s2-wgpu so the bench script is shared.
- [ ] Steps: build in Docker (expect missing dev libs → add to the Dockerfile, rebuild); user-run smoke + the same checklist → `docs/spikes/S2-checklist-gpui.md`. Record **implementation cost**: LOC, hours, number of upstream workarounds, for both front-ends.
- [ ] Commit — `s2-gpui: interaction slice in gpui`

---

### Task 5: `s2-bench` — driver + analysis

**Files:**
- Create: `spikes/s2-bench/run.sh` (PC side), `spikes/s2-bench/analyze.py` (archbox side), `spikes/s2-bench/README.md`

**Behaviour:** `run.sh <wgpu|gpui> <quiet|flood> <hz-label>`: starts the app with `--bench ~/s2-logs/<id>`, `--photon`; prints "focus the window, hands off"; after 5 s types a fixed 2000-event plain stream with `ydotool type --key-delay 20` (the K1 probe-first pattern: a probe key, verify it appears in the latency log within 3 s, else abort); for `flood`, `--cmd "<path>/s0 flood --bytes-per-sec 500000"`; samples CPU/RSS/GPU; 60 s idle sampling at the end (idle power proxy). `analyze.py` → Markdown row: app | load | Hz | keys | key→write p50/p95/p99 | write→output | output→submit | submit→present | **key→present p50/p95/p99** | frames | dropped frames | CPU % (typing / flood / idle) | RSS | GPU util/power (if available). Photon: user films the top-left square at 240 fps during a 10-key run; `analyze.py photon <video>` is **out of scope** — the user reads frame counts by hand from the video (document the method: count frames between keypress sound/LED and square flip), enter 10 samples into `docs/spikes/S2-results.md`.
- [ ] Commit — `s2-bench: ydotool driver, sampler, analysis`

---

### Task 6: Measurement runs (user present) + results + decision

- [ ] Matrix (ask before each PC session): {wgpu, gpui} × {quiet, flood} × {60 Hz, max Hz} × {100 %, 125 % scaling} — 16 runs × ~1.5 min + photon samples for each app at max Hz. Record environment per Global Constraints.
- [ ] `docs/spikes/S2-results.md`: tables; text-quality screenshots (user-provided PNGs under `docs/spikes/s2/`, paths listed); coverage/cost table (features done, LOC, workarounds, IME behaviour, menus, DPI); **decision** per PLAN §3: pick winit+wgpu, gpui, or "custom grid inside toolkit X" with the reasons tied to rows; what remains (Windows TSF, screen reader = out of scope, multi-window).
- [ ] `docs/PLAN.md` §3 Desktop GUI row → decided; §6 S2 status; §9 baseline numbers added (compare with tmls's ack RTT p50 37 ms from S0 as the thing to beat).
- [ ] `PROGRESS.md` entry. Commit — `docs: S2 results and GUI stack decision`

---

### Task 7: Codex review gate

- [ ] Codex (gpt-6-sol) review of `s2-core`, both front-ends and `S2-results.md` (brief: is key-to-present measured honestly (present vs photon); are the two slices equivalent in scope so the comparison is fair; does the decision follow from the rows; unmeasured). Save `docs/reviews/<date>-codex-sol-s2.md`; one fix wave; re-review; merge.
