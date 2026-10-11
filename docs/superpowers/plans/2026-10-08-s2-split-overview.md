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
pub enum InputMsg { Bytes(Vec<u8>, u128 /* t_output_ns */), Resize(u16, u16), Key(KeyEvent, u64 /* key id */), Paste(String), Focus(bool), Shutdown }
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
  `frame_with_work_done(…, Option<t_work_done_ns>)`, `event(&Event)` for `meta`, `flush()`. `latency::Event` is the serde
  type of a log line. Times are written as unsigned 64-bit ns; `frame` may carry an optional `t_work_done_ns`.
- **Key attribution is `frame.keys`, not the timestamps.** The core assigns a key to the first frame whose output was
  read at or after the key's PTY write *began*: the tty echo can be read before `write()` returns, so `t_output_ns` may
  be a few microseconds earlier than the logged `t_write_ns`. Treat a negative write→output span as 0.
- Only keys with a `key` line, a `write` line and a presented frame count. Percentiles are nearest-rank:
  `sorted[ceil(p/100 · n) − 1]`. Golden pair: `spikes/s2-core/tests/data/golden-latency.jsonl` + `golden-report.json`
  (generator `make-golden.py` beside them computes the report independently of the Rust code).

### Latency log — `<dir>/latency.jsonl` (Track A writes; Track B's `analyze.py` reads)

One JSON object per line, `"ev"` discriminator, all times `u128` ns from `now_ns()`:

```json
{"ev":"meta","app":"wgpu","git":"<hash>","profile":"release","hz":144.0,"scale":1.25,"present_mode":"fifo","load":"quiet"}
{"ev":"key","id":1,"t_key_ns":0}
{"ev":"write","id":1,"t_write_ns":0}
{"ev":"frame","seq":7,"t_output_ns":0,"t_submit_ns":0,"t_present_ns":0,"present_kind":"present_return|work_done|frame_callback","keys":[1]}
{"ev":"sample","t_ns":0,"cpu_pct":0.0,"rss_kb":0,"fps":0.0,"gpu_util":null,"gpu_w":null}
```

Key attribution: a key belongs to the **first frame whose `t_output_ns` ≥ its `t_write_ns`** (emulator fills `keys`).
`present_kind` must be honest: `present()` return ≠ photon; gpui may only offer a frame callback.

### App CLI (identical for `s2-wgpu` and `s2-gpui`, so `run.sh` is shared)

`--bench <dir>` · `--photon` (64×64 top-left square toggles per key, same frame as the echo) · `--cmd <program> [args…]`
(default `$SHELL`) · `--present fifo|mailbox|immediate` (wgpu only; gpui ignores + logs) · `--load-label quiet|flood`
(written to meta). Exit cleanly on window close, flushing the log.
