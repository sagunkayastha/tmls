# S2 — two-track split (overview + shared contract)

Source plan: `docs/superpowers/plans/2026-10-08-s2-interaction-slice.md` (the "S2 plan"; tasks 1–7). It is split into two
tracks that run **in parallel** on separate branches, plus one shared step done first on `main`.

| Track | Plan file | Branch | Default owner | S2-plan tasks |
|---|---|---|---|---|
| 0 — contract | this file, §Task 0 | `main` | Claude | (new) |
| A — core + wgpu | `2026-10-08-s2-track-a-core-wgpu.md` | `spike/s2-wgpu` | Codex | 1, 2, 3 |
| B — gpui + bench + image | `2026-10-08-s2-track-b-gpui-bench.md` | `spike/s2-gpui` | Claude (or a 2nd Codex) | 4, 5, image rebuild |
| joint | S2 plan Tasks 6–7 | `main` | user + Claude, Codex review | 6, 7 |

## Rules for every agent (both tracks)

- Commits authored `sgkayast <sgn.kayastha@gmail.com>`. **No AI attribution lines** (no `Co-Authored-By`, no "Generated with").
- Never write under `/tmp`; scratch → `/data/scratch/s2/` (archbox), logs → `~/s2-logs/` (PC). Set `TMPDIR` accordingly.
- Never delete or move files without the user's explicit yes for that action.
- PC (`ssh pcl`): Docker only (`tooling/docker/build.sh`), no host installs. **Every PC job needs the user's yes first**; a
  GPU window runs only with the user at the PC. Agents prepare commands and wait.
- archbox builds of `s1`/`s2-core` need Zig 0.15.2: `source spikes/s1/build-env.sh` (sets `PATH` + `GHOSTTY_SOURCE_DIR`).
- **Ownership (avoids merge conflicts):** Track A owns `spikes/s2-core/**`, `spikes/s2-wgpu/**`. Track B owns
  `spikes/s2-gpui/**`, `spikes/s2-bench/**`, `tooling/docker/**`, `docs/licences.md`. Root `Cargo.toml` is fixed by Task 0;
  nobody edits it afterwards. `Cargo.lock`: on merge conflict, take main's and run `cargo metadata` to regenerate.
- Each task ends with tests passing and one commit; update `PROGRESS.md` only at track end (one entry per track).

## Sync points

1. **Task 0 on `main`** → both tracks branch from it.
2. **A1 (s2-core) merges to `main`** (after Claude review) → B rebases; B4 (`s2-gpui`) needs real `s2-core`.
   Until then B builds only against the Task 0 stubs.
3. **B1 (Docker image with GUI libs) built on the PC** (user yes) → A2 can build `s2-wgpu` in Docker.
4. User-run sessions at the PC (A2 smoke, A3 checklist, B4 smoke/checklist, Task 6 matrix) — one at a time, user present.

```
main ─ T0 ─┬─ A1 s2-core ──(review, merge)──┬─ A2 wgpu grid ─ A3 wgpu UI ─────────┐
           │                                 │                                     ├─ Task 6 matrix ─ Task 7 review ─ merge
           └─ B1 image ─ B2 gpui probe ─ B3 bench ─(rebase)─ B4 gpui slice ────────┘
```

## Task 0 — shared contract on `main` (Claude, before handoff)

- Root `Cargo.toml` members: `spikes/s0`, `spikes/s1`, `spikes/s2-core`, `spikes/s2-wgpu`, `spikes/s2-gpui`;
  `default-members` = all except `s2-gpui` and `s2-wgpu` (archbox `cargo build`/`test` stays headless; GUI crates are built
  with `-p` in Docker). `spikes/s2-bench` is scripts only (bash + Python), not a workspace member.
- `spikes/s2-core` **stub crate** with the public API below; bodies `todo!()`; compiles on archbox.
- Stub `spikes/s2-wgpu` / `spikes/s2-gpui` crates: `main.rs` that prints "stub" (so the workspace compiles).
- This file's contract is the source of truth; changing it needs a note here and a ping to the other track.

## Shared contract

### `s2-core` public API (Track A implements; Track B consumes)

```rust
// pty
pub struct Pty;                                   // portable-pty
impl Pty {
    pub fn spawn(cmd: &[String], cols: u16, rows: u16) -> anyhow::Result<Pty>;
    pub fn writer(&self) -> PtyWriter;            // Clone + Send; the ONLY path that writes to the PTY
    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()>;
}
// emulator thread (libghostty-vt objects are not Send → they live on this thread only)
pub struct Emu { pub input_tx: Sender<InputMsg>, pub frames: FrameSlot }
pub enum InputMsg { Bytes(Vec<u8>, u128 /* t_output_ns */), Resize(u16, u16), Key(KeyEvent, u64 /* key id */), Paste(String), Focus(bool),
                    Scroll(i32 /* rows; negative = up, into scrollback */), ScrollToBottom, Shutdown }
impl Emu { pub fn start(pty: Pty, cols: u16, rows: u16, log: Option<latency::Log>) -> anyhow::Result<Emu>; }
// FrameSlot = latest-wins single slot (Mutex<Option<Frame>> + wake callback). Frames may drop; PTY bytes never.
pub struct FrameSlot; impl FrameSlot { pub fn take(&self) -> Option<Frame>; pub fn set_waker(&self, f: Box<dyn Fn() + Send + Sync>); }

pub struct Frame { pub cols: u16, pub rows: u16, pub cells: Vec<Cell>, pub cursor: Option<Cursor>,
                   pub dirty_rows: Vec<bool>, pub seq: u64, pub t_output_ns: u128, pub keys: Vec<u64> }
pub struct Cell { pub text: String, pub fg: Option<(u8,u8,u8)>, pub bg: Option<(u8,u8,u8)>, pub flags: CellFlags /* bold italic underline inverse strike faint */, pub wide: Wide /* Narrow|Lead|Spacer */, pub hyperlink: Option<String> }
pub struct Cursor { pub col: u16, pub row: u16, pub shape: CursorShape, pub visible: bool }

// keys (toolkit-neutral; front-ends map winit/gpui events into this)
pub struct KeyEvent { pub key: Key, pub mods: Mods, pub text: Option<String>, pub repeat: bool, pub pressed: bool }
pub enum Key { Char(char), Named(Named) }        // Named: Enter, Tab, Backspace, Escape, arrows, Home/End/PgUp/PgDn, Insert/Delete, F1–F12
pub fn now_ns() -> u128;                          // CLOCK_MONOTONIC; every timestamp in every log uses this
```

Front-end duties: send `InputMsg::Key` with `t_key_ns` already logged; on each rendered frame call
`log.frame(seq, t_submit_ns, t_present_ns, t_present_kind)`.

**Contract notes from A1 (2026-10-10, `s2-core` as built; all additive):**
- `InputMsg::Focus(bool)` — the front-end sends it on window focus change. `false` drops queued key repeats, and no
  repeat is sent until the next fresh press; both values report `CSI I` / `CSI O` to the PTY under mode 1004.
- `Emu::exited() -> bool` — true once the child closed the PTY; the waker fires when it turns true.
- `Pty::reader()`; `PtyWriter::write_all(&[u8])` (plus `start_recording()` / `recorded()` for tests).
- `Frame::row_text(y)`. When the slot replaces an unread frame, the new frame inherits its `keys` and dirty rows.
- `latency::Log`: `create(dir)`, `key(id, t_key_ns)`, `write(id, t_write_ns)` (called by the core), `frame(…)`,
  `frame_with_work_done(…, Option<t_work_done_ns>)`, `frame_with_marks(…, Option<t_work_done_ns>, Option<t_redraw_ns>)`
  (added in A2, see below), `event(&Event)` for `meta`, `meta_with(&Event, &MetaExtra)` for a `meta` line with the
  optional fields `ui` / `hz_source` (added in A3, see below), `flush()`. `latency::Event` is the serde
  type of a log line. Times are written as unsigned 64-bit ns; `frame` may carry an optional `t_work_done_ns`.
- **Key attribution is `frame.keys`, not the timestamps.** The core assigns a key to the first frame *with PTY output*
  whose output was read at or after the key's PTY write *began*: the tty echo can be read before `write()` returns, so
  `t_output_ns` may be a few microseconds earlier than the logged `t_write_ns`. Treat a negative write→output span as 0.
- Only keys with a `key` line, a `write` line and a presented frame count. Percentiles are nearest-rank:
  `sorted[ceil(p/100 · n) − 1]`. Golden pair: `spikes/s2-core/tests/data/golden-latency.jsonl` + `golden-report.json`
  (generator `make-golden.py` beside them computes the report independently of the Rust code).

**Contract changes from the A1 fix wave (2026-10-10, review `docs/reviews/2026-10-10-opus-s2-core.md`):**
- **New: viewport scrolling.** `InputMsg::Scroll(i32)` moves the viewport by that many rows — negative is up, into the
  scrollback (wheel up); positive is back towards the live screen — clamped at both ends; `InputMsg::ScrollToBottom`
  returns to the live screen. Each is followed by a frame (rows that now show other text are dirty; `cursor` is `None`
  while the cursor row is outside the viewport). PTY output does not move a scrolled viewport; a `Key` or `Paste` that
  sends bytes returns it to the bottom, so front-ends need not send `ScrollToBottom` before typing.
- **`Frame::t_output_ns` as implemented:** the read time of the *last* PTY chunk parsed into the frame (a frame can
  cover several chunks). A frame published without new PTY output — the first frame, a resize, a scroll — carries the
  time it was built instead, and **its `keys` are always empty**: only a frame with output can show a key's echo
  (review B3). Such frames still get a `frame` line if presented; they just carry no keys.
- **`set_waker` never loses a wake-up (B7):** if a frame is already waiting in the slot or the child has already exited,
  the waker is called once from inside `set_waker`, before it returns. The waker can run on any thread and can run
  spuriously; it must not call `set_waker` itself. No "poll once after `set_waker`" is needed.
- **`log.frame(seq, …)` covers every frame taken up to `seq` (B8):** the line carries the keys of all frames taken from
  the slot with a seq ≤ `seq` that no earlier `frame` line covered, and the `t_output_ns` of the newest of them. So a
  front-end may take several frames and present only the last; call `log.frame` once per *presented* frame with the seq
  that was drawn. Frames taken but not yet covered are bounded (256 entries; older ones merge, keys kept).
- **Input never blocks.** `PtyWriter::write_all` returns at once: what the PTY takes now is written inline, the rest is
  queued and written in order by a writer thread (large pastes, a child that does not read stdin). `t_write_ns` is when
  the key's last byte was accepted by the kernel. `Pty::reader()` returns `Ok(0)` when the child side is closed.
  Dropping `Emu` joins its threads (`s2-emu`, `s2-pty-read`, `s2-pty-write`) and drops input still queued.
- **Keys:** with Ctrl held, pass `text: None` or the control character the toolkit reports (winit's `"\x03"`); control
  characters in `text` are ignored and key + modifiers decide. Ctrl+`[ \ ] / _ @ ^ ?` and Ctrl+`i`/`m` send their C0
  byte in legacy mode. *Printable* text with Ctrl (`text: Some("[")`) is passed to libghostty-vt as is, which then uses
  its fixterms CSI-u form for a few chords (`ESC[91;5u` for Ctrl+`[`) — so do not pass it.
- **Public API Track B calls that the notes above omit:** `bench::Sampler::start(Log) -> Sampler` (writes a `sample`
  line every 500 ms and flushes the log; stops when dropped), `Log::frames_logged() -> u64` (what the sampler's `fps`
  counts — one per `log.frame` call), `keys::Encoder` (`new() -> anyhow::Result<Encoder>`,
  `encode(&mut self, &KeyEvent, &Terminal) -> anyhow::Result<Vec<u8>>`; front-ends normally send `InputMsg::Key`
  instead) and `keys::paste_bytes(text: &str, bracketed: bool) -> anyhow::Result<Vec<u8>>`, and the `Mods` fields
  `shift`, `ctrl`, `alt`, `logo` (all `bool`, `Default` = none). *(Signatures corrected in the final fix wave,
  2026-10-11: all three return `anyhow::Result`, and `encode` takes `&mut self`.)*
- **Measurement validity — flood runs:** under `--load-label flood`, key→present (and the `--photon` square) means
  "the next frame with output after the key was written", not "the frame in which the echo is visible": any PTY output
  read after the write began counts. Quiet runs measure echo-visible latency. State this next to flood results.

**Contract notes from A2 (2026-10-10, `s2-wgpu` as built; for `s2-gpui` and `analyze.py`):**
- **`t_key_ns` is when the app dequeued the event, not when the key arrived.** It is taken at `window_event` entry. A key
  that arrives while the main thread is inside a redraw (quad build, texture acquire, submit, present) is stamped only
  when the redraw returns, so key→present is **under-reported** by the time it waited. Negligible for quiet runs under
  paced fifo on Wayland; not negligible for flood with `--present mailbox|immediate`, for X11 (fifo can block in
  acquire/present for up to a refresh) and for the first frame (submit→present ≈ 52 ms in the smoke logs). State this
  next to flood and X11 results.
- **New optional `frame` field `t_redraw_ns`** (contract change, additive): when the redraw that presented the frame
  began, i.e. from when the front-end's main thread took no events until `present()` returned. `t_redraw_ns ≤
  t_submit_ns`. Analysis can flag keys whose `t_key_ns` falls just after a frame's `t_redraw_ns..t_present_ns` window as
  possibly stamped late. A front-end that does not have the mark omits the field (`s2-gpui` may); lines without it,
  the golden pair and `latency::report` are unchanged. `s2-wgpu` writes it on every `frame` line.
  New `s2-core` API: `Log::frame_with_marks(seq, t_submit_ns, t_present_ns, t_present_kind, Option<t_work_done_ns>,
  Option<t_redraw_ns>)`; `frame` and `frame_with_work_done` keep their signatures and write no `t_redraw_ns`.
  `latency::Event::Frame` has the new field `t_redraw_ns: Option<u64>` — code that builds or fully destructures that
  variant must name it (or use `..`).
- **Re-presents:** presenting an already logged frame again (selection, preedit, resize repaint, and since A3 any
  redraw caused by the UI alone: a focus change, typing in the text field, the list, the menu, zoom) writes a `frame`
  line with the same `seq`, `t_output_ns: 0` and no keys. Per-frame analysis must skip lines with `t_output_ns == 0`.
- **`meta.hz` is `0.0` when the refresh rate is unknown.** Treat it as unknown; never divide by it.
  *(A3 fix round, 2026-10-11)* How `s2-wgpu` finds it, in this order, never a guess: the monitor the window is on
  (winit `Window::current_monitor()`); else, if exactly one monitor is connected, that one; else unknown. With several
  monitors and no current one the rate stays unknown (the fastest is not a safe guess). On Wayland a window is on no
  monitor until the compositor has mapped it, i.e. until after its first present — a front-end that asks before
  that gets nothing (this is why `hz` was `0.0` on the PC; `s2-gpui` should check the same). `spikes/s2-wgpu/src/meta.rs`.
- **When `s2-wgpu` writes `meta`** *(A3 fix round)*: exactly once, at the first of these: before the first present
  if the rate is known then; else as soon as it becomes known (checked before every present and after every batch of
  window events); else 1 s after the first present, or at exit, with `hz: 0.0`. So **`meta` is not necessarily the
  first line and can come after the first `frame` lines** (`key`, `write` and `sample` lines could precede it
  before, too). Readers must find it by `"ev":"meta"`, not by position. It is never rewritten.
- **`S2_WGPU_NO_UI=1`** *(A3 fix round; environment, not a flag — the CLI above is unchanged)*: runs `s2-wgpu`
  without its sidebar / form / menu: the terminal alone at the window's corner, every key a terminal key. For A/B
  runs that measure what the UI costs per frame. Unset, empty, `0` or `false` = UI on.
- **Optional `meta` fields `ui` and `hz_source`** *(contract change, additive, 2026-10-11)*: `"ui": true|false`
  (false = `S2_WGPU_NO_UI`) and `"hz_source": "current_monitor"|"only_monitor"|"unknown"`, written after the fixed
  fields. `s2-wgpu` writes both on every `meta` line; a front-end that does not know them omits them (`s2-gpui` may),
  and a line without them is byte for byte what it was — the golden pair and `latency::report` are unchanged.
  `analyze.py` must treat both as optional (a missing `ui` means "not recorded", not "on").
  New `s2-core` API: `latency::MetaExtra { ui: Option<bool>, hz_source: Option<String> }` and
  `Log::meta_with(&Event, &MetaExtra)`. `latency::Event::Meta` and `Log::event` are unchanged: code that builds or
  destructures `Event::Meta` needs no edit; reading a `meta` line as `Event` ignores the extras, reading the same
  line as `MetaExtra` yields them (both `None` for an old line).
- **`t_work_done_ns` is an upper bound.** The callback runs inside `device.poll`, which the app calls on the main thread
  after `present()` has returned (then every 500 µs while a line waits), so it is never earlier than `t_present_ns` and
  is later when the main thread is busy. The line is written without the field if the callback has not run within
  100 ms or by exit.
- **Key ids are not dense.** Every key event sent to the core takes an id; only presses and repeats get a `key` line
  (releases do not). An IME commit is one press with the whole text and counts as a key.
- **Pacing:** under fifo `s2-wgpu` calls `Window::pre_present_notify` before each present, so on Wayland redraws
  follow the compositor's frame callbacks; mailbox and immediate are unpaced. A mode the surface does not offer falls
  back to fifo and `meta.present_mode` names the mode in use. State the pacing next to results.
- **`Emu::exited()` can turn true before the child's last output is in a frame** (the PTY reader sets it at end of
  file; the emulator thread may still hold the last bytes). A front-end must not exit on it alone: after seeing
  `exited()`, send `InputMsg::ScrollToBottom` and keep taking frames until one built after it arrives
  (`t_output_ns` ≥ the send time; if a frame with an earlier time arrives, send the marker again), bounded by a
  deadline, and exit only once that frame has been presented. Reference: `spikes/s2-wgpu/src/session.rs`,
  `Session::begin_exit` / `Session::output_drained` (re-send in `Session::take_frames`); the exit decision is
  `spikes/s2-wgpu/src/surface.rs`, `may_exit`, with the 2 s deadline `EXIT_WAIT` in `spikes/s2-wgpu/src/app.rs`.

**Contract notes from A3 and the final fix wave (2026-10-11, `s2-wgpu` as merged; `s2-gpui` must match the first two):**
- **Focus traversal binding.** Ctrl+Tab / Ctrl+Shift+Tab move the keyboard focus to the next / previous stop
  (list → field → terminal → list) from anywhere, the terminal included. Plain Tab / Shift+Tab do the same **only
  while the focus is outside the terminal** (list, field). **With the terminal focused, Tab and Shift+Tab always go
  to the program** (shell completion) and never move the focus. `s2-gpui` must bind the same keys the same way, or
  the two checklists and bench runs are not comparable. Ctrl+Tab and Ctrl+Shift+Tab get no `key` line.
- **`InputMsg::Focus(false)` is sent to the core when the keyboard focus leaves the terminal inside the window**
  (to the list, the field or an open menu), not only when the window loses focus; `Focus(true)` when it comes back.
  What the core is sent is "window focused AND terminal focused". Consequences: under mode 1004 the program sees
  focus-out when the user tabs to the sidebar, and queued key repeats are dropped then.
- **Ctrl+Space is sent with `text: None`** (`Key::Char(' ')`, `mods.ctrl`), like every key with Ctrl held; the core
  encodes it from key and modifiers (NUL in legacy mode).
- **Queued writes and attribution.** When the PTY input buffer is full (the child is not reading), a key's bytes wait
  in the writer queue; its write-begin time and its entry into the core's unseen-key list are then taken when the
  queued write runs, which can be late, so the key's attribution can slip to a later frame than the one that shows
  its echo. Quiet runs and the 2000-key flood runs stay below that buffer and are not affected.
- **Analysis conventions for both front-ends** (`analyze.py` and any ad-hoc script): count only keys that have a
  `write` line (a key the core encoded to no bytes has a `key` line and nothing else); and drop the first second of
  a run — the first frame is slow (submit→present ≈ 52 ms in the smoke logs), and `s2-wgpu` queries the monitors
  inside the redraw window until `meta` is written (up to 1 s after the first present).
- **`meta.git` may end in `-dirty`** (`<short hash>-dirty`): the binary was built from a tree in which a tracked file
  differed from that commit. Results for the record come from a build without it.

### Latency log — `<dir>/latency.jsonl` (Track A writes; Track B's `analyze.py` reads)

One JSON object per line, `"ev"` discriminator, all times `u128` ns from `now_ns()`:

```json
{"ev":"meta","app":"wgpu","git":"<hash>","profile":"release","hz":144.0,"scale":1.25,"present_mode":"fifo","load":"quiet","ui":true,"hz_source":"current_monitor"}
{"ev":"key","id":1,"t_key_ns":0}
{"ev":"write","id":1,"t_write_ns":0}
{"ev":"frame","seq":7,"t_output_ns":0,"t_submit_ns":0,"t_present_ns":0,"present_kind":"present_return|work_done|frame_callback","keys":[1],"t_work_done_ns":0,"t_redraw_ns":0}
{"ev":"sample","t_ns":0,"cpu_pct":0.0,"rss_kb":0,"fps":0.0,"gpu_util":null,"gpu_w":null}
```

**Optional fields** — a line may omit them, and a reader must accept lines with and without: on `meta`, `ui`
(`true|false`) and `hz_source` (`"current_monitor"|"only_monitor"|"unknown"`); on `frame`, `t_work_done_ns` and
`t_redraw_ns`. `s2-wgpu` writes `ui`, `hz_source` and `t_redraw_ns` on every such line and `t_work_done_ns` when the
callback ran in time; `s2-gpui` may omit any of them. `git` is `<short hash>` or `<short hash>-dirty`. The `meta`
line is not necessarily the first line (see the A2 notes).

Key attribution is the `keys` array of the `frame` line (the emulator fills it; rule in "Contract notes from A1" and
"A1 fix wave" above) — do not re-derive it from the timestamps. `present_kind` must be honest: `present()` return ≠ photon; gpui may only offer a frame callback.

### App CLI (identical for `s2-wgpu` and `s2-gpui`, so `run.sh` is shared)

`--bench <dir>` · `--photon` (64×64 top-left square, black at start; rule below) · `--cmd <program> [args…]`
(default `$SHELL`) · `--present fifo|mailbox|immediate` (wgpu only; gpui ignores + logs) · `--load-label quiet|flood`
(written to meta). Exit cleanly on window close, flushing the log.

**`--photon` rule** *(contract change, final fix wave 2026-10-11; was "toggles per key")*: **one flip per presented
frame that carries at least one key press** — press or repeat; a release never counts, although it has a key id and
can be on a frame's `keys` (kitty protocol). The flip is in the same presented frame as the echo. Two keys echoed in
one frame flip the square once, so every frame that shows an echo changes the square; a camera counts frames with an
echo, not keys. Frames taken but not presented (surface not ready) and the next frame are one presented frame: one
flip. `s2-gpui` must implement the same rule. Reference: `spikes/s2-wgpu/src/session.rs`, `Photon`.
