# PROGRESS — tmls-rs (2026-10-08)

Quick session log. Newest entry first.

## 2026-10-11 (overnight) — `tmls-rs` desktop v0 built, reviewed, merged to main; waiting on the user's morning test ✅

One night, plan `docs/superpowers/plans/2026-10-11-tmls-rs-desktop-v0.md`, all tasks 0–10 done (02:36 → 05:45 MDT, before
the 06:30 cutoff). `tmls-rs` = a native wgpu window over **ssh + tmux**: tmls-style sidebar (hosts, marks, Claude title +
context %, pins), sessions attached in tabs, new / rename / kill, mouse to tmux, links, alerts. Branch `feat/desktop-v0`
(36 commits) merged `--no-ff`. **Start here: `docs/MORNING-TEST.md`** (15-minute test sheet + every decision taken).

- **Crates:** `crates/client-core` (no UI: `config`, `listing`, `poll`, `tmux`, `seen`; example `list`) and `crates/desktop`
  (binary `tmls-rs`: `app`, `tabs`, `sidebar`, `draw`, `keys`, `forms`, `mouse`, `links`, `alerts`, `theme`). Additive in
  the spikes: `s2-core` `InputMsg::Mouse` + mouse-mode flag on `FrameSlot`; `s2-wgpu` `Session::take_frames_with`,
  `meta::write_as`, `meta::GIT`, stderr line `s2-wgpu: focus true|false` (contract overview updated in the same commits).
- **Tests at the merged head (`0839d97` + docs):** `cargo test -p client-core -p s2-core -p s2-wgpu` 227 passed / 1
  ignored (archbox); `desktop` 93 passed (Docker); clippy on all four crates 0 warnings; headless end-to-end
  `tooling/smoke/run.sh` **117 / 117 three times in a row** (sway headless + lavapipe, own tmux server, `wtype` keys,
  virtual pointer `tooling/smoke/vptr.py`). Logs: `/data/scratch/tmls-rs-v0/t9final/`.
- **Real data, read-only:** `cargo run -p client-core --example list` gives the same session names as `ag ls` on both
  hosts (archbox 3, laanta-chy 4); marks consistent. No real session was attached, renamed or killed by any test.
- **PC (RTX 5070 Ti, KDE Wayland, scale 1.15):** Docker release build ok, `ldd` clean. Apps started without any typed
  input: `meta` hz 164.958, `hz_source current_monitor`, scale 1.15 in `s2-wgpu` (UI on / off) and `tmls-rs`. Screenshot
  `/data/scratch/tmls-rs-v0/pc/tmls-rs-sidebar.png` shows `tmls-rs` beside tmls with the same hosts, sessions and pins.
  **Finding:** emoji is drawn as a one-colour outline on the PC (`/data/scratch/tmls-rs-v0/pc/s2-wgpu-ui-crop.png`); CJK is right.
- **Reviews:** every task had an opus spec + quality review; fix rounds on tasks 1, 2, 4, 8 (one each). Final
  whole-branch review: 0 critical, 1 important — a kill / rename started from the list moved the keys into the
  neighbouring tab's terminal when the result arrived; fixed (`afca8f6`), with: result-while-a-form-is-open keeps its tab,
  stderr writes cannot panic (app no longer dies when started with `&` and its terminal is closed), pointer state reset
  on tab change. All four latency invariants hold in `crates/desktop/src/app.rs`.
- **Rulings that changed the plan** (full list with costs in `docs/MORNING-TEST.md`): pins are tmls slugs only (shared
  both ways with tmls); alerts survive a host's offline spell (tmls drops them); `--bench` logs the first tab opened;
  no `⋯` on rows (right-click or `m`); new list keys `c` `m` `a` and Alt+Shift+W; **no script typed keys into the live
  KDE session** (risk of typing into a running Claude session) → no unattended latency numbers, the bench rows in
  `docs/spikes/S2-checklist-wgpu.md` are filled only for hash / `ldd` / `meta` / emoji.
- **Never run for real** (headless or unit tests only): click / drag / Ctrl+click in the terminal, Ctrl+C copy, "Open in
  new terminal", clipboard, real `notify-send`, fcitx5.
- **Gotchas:** `cargo-clippy` is not in the Docker build image (clippy runs on archbox). 5 `s2-wgpu` GPU tests fail in
  the build image for lack of an adapter and pass on archbox. `tmux capture-pane -t '=NAME'` fails in tmux 3.3a — use
  `'=NAME:'`. `wtype` loses its first key on a headless seat without a short sleep. tmls and tmls-rs share
  `~/.config/tmls/pins` — tmls-rs writes the set it read at start.
- **Left on disk, nothing deleted:** `/data/scratch/tmls-rs-v0/` (archbox), `~/offload/tmls-rs-v0/` (PC), image
  `tmls-rs-smoke:latest` (3.45 GB, 0.32 GB new), `.superpowers/sdd/2026-10-11-tmls-rs-desktop-v0/` (ledger, briefs,
  reports), untracked `crates/desktop/tests/tabs.rs.new`, ignored `tooling/smoke/__pycache__/`, branch `feat/desktop-v0`.

### NEXT
1. **User:** `docs/MORNING-TEST.md` on the PC (build line inside). Blocks everything below.
2. From its answers: fix what fails; decide the look (colours / spacing vs tmls) and the vetoes in its decision list.
3. Latency with the sidebar on a real GPU (step 10 of the sheet, 2 minutes of typing) → the 115 % rows of the checklist.
4. Open minors, ranked: ssh keepalive on attach (frozen tab after a silent network drop); emoji colour on the PC;
   a session renamed outside the app keeps its old tab label; no "in progress" note for a running request; forms take
   no IME; the ~300 lines copied from `s2-wgpu/src/app.rs`. Full list: the ledger's `minor (deferred)` lines.
5. Still deferred by plan: gpui comparison (Track B), PLAN phases 1a–7 (workers / hostd / transport).

## 2026-10-11 (later) — Track A merged to main at `bd09e33`; waiting on the user's A3 checklist run ✅

`main` = `spike/s2-wgpu` = `bd09e33` (merge, `--no-ff`). On the merged tree `cargo test -p s2-core -p s2-wgpu`: 155 passed,
0 failed, 1 ignored. The entry below was written mid-run; its NEXT 1, 2 (hz part) and 4 are done.

- **Task 4 (A3)** reviewed: 3 important findings fixed in one round (`b60715d`..`79e7e97`) — held key that moves focus
  into the terminal no longer writes orphan `key` lines; focus decisions moved to a pure tested `Ui::sync_focus`;
  env switch **`S2_WGPU_NO_UI=1`** drops the sidebar for A/B runs (`meta.ui` records it); Ctrl+_ returned to the
  terminal; `meta.hz` fixed (cause: on Wayland the monitor is unknown until the surface is mapped, after the first
  present — `meta` now waits up to 1 s, so it is **no longer the first log line**; new `meta.hz_source`). Core gained
  `MetaExtra` + `Log::meta_with` (additive). Headless A/B on lavapipe: submit→present p50 2.4 ms without UI, 6.0–6.4 ms
  with; CPU side unchanged. **UI cost on a real GPU is still unmeasured.**
- **Final whole-branch review (opus):** 0 critical; no path found that loses, double-counts or mis-attributes a key.
  Fix wave `3e3b355`..`80ae53b` + `0f08621`: checklist made runnable (update-and-build step, bench commands, analysis
  snippet, results table), stale scale/refresh removed from startup stderr, `meta.git` gets `-dirty` for a changed
  tracked file, photon square = one flip per presented frame with a key press, contract drift fixed
  (`keys::Encoder` signatures, optional fields in the JSON example, focus binding, analysis conventions).
- **Decisions Track B must follow** (all in the split overview): focus traversal is Ctrl+Tab / Ctrl+Shift+Tab (plain Tab
  only outside the terminal — plan said Tab/Shift-Tab); `Focus(false)` goes to the core when focus leaves the terminal
  inside the window (an open context menu does not count); photon rule above; `meta` found by `"ev":"meta"`, optional
  `ui` / `hz_source`; `Event::Frame` has `t_redraw_ns`; analysis counts only keys with a `write` line and drops the first second.
- **Still open:** the single 38 ms key of PC run 1 (1 of 312, unexplained; visible as `t_redraw_ns − t_output_ns`);
  B6 kitty Shift+letter encoding; IME edge cases (commit while the menu is open, composition across a focus move) —
  decide after the fcitx5 checklist items; `build.rs` `-dirty` reflects the tree at the last build-script run.
- **Cleaned (user's yes, 2026-10-11):** SDD workspace `.superpowers/sdd/2026-10-08-s2-track-a-core-wgpu/` (ledger,
  task reports, review packages — the record is now git + this file; the checklist's line 14 still names
  `task-4-report.md` there, which no longer exists), `/data/scratch/s2/{task2,task3,task4,final-fix,final-review,
  review-final-fix,smoke,tmp}`, archbox `target-docker/` (1.2 GB, on `/`), image `s2-wgpu-smoke:latest`.
  `/data` freed only ~0.4 GB (still 89 %): the image and `target-docker/` lived on `/`. Kept: `/data/scratch/s2/wgpu-smoke/`
  (PC logs `pc/`, `pc2/` + headless runs), `/data/scratch/s2/review/` (888 MB, A1 review experiments — not on the
  approved list), `wt-b1` (Track B worktree), image `tmls-rs-build:latest`.
- **Headless smoke setup is gone with `smoke/` and the image:** to smoke-test on archbox again, rebuild a derived
  image (sway or weston headless + lavapipe + `wtype` + a monospace and a CJK font on top of `tmls-rs-build`) and
  `tooling/docker/build.sh build --release -p s2-wgpu`.
- **`PROGRESS.md` is uncommitted on purpose:** committing would move `main` past `bd09e33`, the hash the user was told
  to expect on the PC. Commit it after the checklist run.

### NEXT (in order)
0. **Overnight 2026-10-11 (user's request, supersedes 1–4 below for now):** execute
   `docs/superpowers/plans/2026-10-11-tmls-rs-desktop-v0.md` unattended — a tmls-like native app (`tmls-rs`: real
   sessions over ssh+tmux, tmls sidebar, tabs) on `feat/desktop-v0`; machine-doable checklist items run on the PC;
   hand-off in `docs/MORNING-TEST.md`. The user said the S2 spike "looks nothing like tmls" and does the main test
   in the morning. wgpu chosen for now; the gpui comparison is deferred.
1. **User at the PC:** follow `docs/spikes/S2-checklist-wgpu.md` from the top (it starts with fetch → `git checkout
   --detach origin/main` → Docker build → `ldd`; expected hash `bd09e33`; the command blocks are bash). Fill in the
   answers, the results table and "hours". After the 100 % / 125 % runs restore the normal 115 % scaling.
2. Copy `~/s2-logs/` from the PC to `/data/scratch/s2/pc-checklist/` and read the logs: `meta.hz` should be 165 with
   `hz_source current_monitor`; compare `-ui` against `-noui` runs for the sidebar's cost on the GPU.
3. Track B: rebase on `main`, gpui slice against the updated contract, bench scripts, `analyze.py` vs the golden pair.
4. The 16-run matrix + photon video → `S2-results.md`, wgpu-vs-gpui decision in PLAN §3.

## 2026-10-11 — A2 `s2-wgpu` terminal app built, reviewed, passed the PC smoke run; A3 built (review running) ✅

Same subagent-driven run (ledger `.superpowers/sdd/2026-10-08-s2-track-a-core-wgpu/progress.md`). `spike/s2-wgpu` =
`443dc5a`, 11 commits ahead of `main` (`1199fb4`); not merged. PC clone is detached at `3593613` (reviewed A2 state).

- **Task 2 (A2 part a)** `7d1edc4` — `spikes/s2-wgpu/src/gfx/`: glyph atlas (cosmic-text + swash), WGSL shader, `Frame` →
  instanced quads, offscreen render. Review: approved, 0 critical / 0 important. archbox has a GL adapter (Mesa, no
  Vulkan), so offscreen tests run for real. Deps: wgpu 30.0.1, cosmic-text 0.19.0, swash 0.2.10, winit 0.30.13, arboard 3.6.1.
- **Task 3 (A2 part b)** `7b45d0e`..`3593613` — winit window, key/IME/mouse/clipboard input, latency marks, `--photon`,
  contract CLI; three `gfx` fixes (origin snapped to whole pixels, `update_overlays` rebuilds after an atlas restart,
  atlas height capped at the device limit). Review: approved; 1 important (plan-mandated): `t_key_ns` is the time the
  event is dequeued, so a key arriving during a redraw is stamped late → fix round added frame field **`t_redraw_ns`**
  (core: `Log::frame_with_marks`, optional field, golden pair unchanged), redraw after a surface timeout, exit only
  after the last present, and "Contract notes from A2" in the split overview. Re-review clean.
- **PC smoke (user at the PC, `3593613`, release, fifo, scale 1.15, 165 Hz monitor)** — Docker release build 1m01s,
  `ldd` clean on the host. Run 2, `/data/scratch/s2/wgpu-smoke/pc2/wgpu-smoke2/latency.jsonl`: 120 s, **309 keys, 309
  attributed**, 0 duplicated, 1010 frame lines (524 real, 486 re-presents), 0 ordering violations. **key→present
  (to `present()` return, not photon) p50 0.563 / p95 0.912 / p99 2.062 / max 4.238 ms.** Frame gaps cluster at 6 ms
  (165 Hz cadence). User: "very smooth". Run 1 (`…/pc/wgpu-smoke/`, only 3 keys) had one 38.1 ms key: 37.6 ms between
  output read and redraw entry — not seen again (1 of 312 keys), unexplained.
- **Bug found on the PC:** `meta.hz` is `0.0` on the real desktop (refresh rate not read; should be 165). Not fixed.
- **Task 4 (A3)** `561b650` + `443dc5a` — `spikes/s2-wgpu/src/ui/` (sidebar list, one-field form, focus, context menu,
  theme, zoom, HiDPI), 103 tests, headless smoke OK (296 terminal keys attributed; list/field keys leave no `key`
  line). Checklist + cost file `docs/spikes/S2-checklist-wgpu.md`; render `/data/scratch/s2/task4/ui-125.png`.
  **Task review was running at log time — not yet approved.** Open points handed to the reviewer: focus keys are
  Ctrl+Tab / Ctrl+Shift+Tab (plan says Tab/Shift-Tab), and the terminal no longer receives Ctrl+Tab, Ctrl+=, Ctrl+−,
  Ctrl+_ (readline undo). Sidebar raised submit→present 2.4 → ~5.5–6 ms under software rendering; unmeasured on a GPU.
- **For Track B:** `Event::Frame` gained `t_redraw_ns` (name it or use `..`); re-present and UI-only frame lines have
  `t_output_ns: 0` and no keys — `analyze.py` must skip them; `hz: 0.0` = unknown; `t_work_done_ns` is an upper
  bound; key ids are not dense; `Emu::exited()` can be true before the last frame (send `ScrollToBottom`, wait for the
  frame after it — see the contract notes).
- **Comparison with Python tmls is not like-for-like:** S0 measured tmls's input path only (p50 5.7 ms quiet, 13.5 ms
  under flood); tonight's number is a bare shell in a window, no session layer or transport. The 16-run matrix is the
  fair comparison.
- **Session mechanics:** the permission classifier blocks `ssh pcl '…'` build commands from the session (plain `scp`
  from `pcl` works), so PC commands are given to the user. The user reads this session on the PC: `!` commands run on
  archbox, PC steps need a local PC terminal and explicit step-by-step instructions.
- **Left on archbox, not removed:** image `s2-wgpu-smoke:latest` (3.77 GB), `target-docker/` (1.2 GB),
  `/data/scratch/s2/{task2,task3,task4,smoke,wgpu-smoke,tmp}`. `/data` was at 89 %.

### NEXT (in order)
1. Read the Task 4 review (reviewer package `review-3593613..443dc5a.diff`) → fix loop → ledger. Rule on the focus keys.
2. Fix `meta.hz` on the PC desktop (ask the user which compositor); look at the 38 ms key with a longer fifo run.
3. User at the PC: rebuild at the new head, then the A3 checklist in `docs/spikes/S2-checklist-wgpu.md` (fcitx5
   compose in the field, wide-char select + copy, paste into the field, right-click menu, wheel, 125 % scaling,
   monitor move) and fill in "hours". Mouse paths and IME have never run in a real window.
4. Final whole-branch review of Track A (opus), triaging the ledger's "minor (deferred)" lines; then merge
   `spike/s2-wgpu` → `main` (user already said yes to the merges).
5. Track B: rebase on `main`, gpui probe, bench scripts, `analyze.py` against the golden pair; then the 16-run matrix
   and the wgpu-vs-gpui decision in PLAN §3.

## 2026-10-10 (late) — A1 `s2-core` fix wave reviewed and merged to main; B1 merged; A2 part a running ✅

*(NEXT 1–3 below are superseded by the entry above: Tasks 2–3 are done and the PC build + smoke ran.)*

Subagent-driven run of the Track A plan (ledger `.superpowers/sdd/2026-10-08-s2-track-a-core-wgpu/progress.md`).
`main` = `1199fb4` (merge of `spike/s2-wgpu`, includes B1 `37d949b`); `spike/s2-wgpu` fast-forwarded to the same commit.

- **A1 fix wave** — 7 commits `2061569`..`0ba295a`. All must-fix items of `docs/reviews/2026-10-10-opus-s2-core.md`
  (B1–B5, B7, B8) fixed, each regression test seen failing on the pre-fix code. Opus task review: concurrency rework
  sound, 0 critical, 2 test defects (vacuous reader-EOF check, racy thread count) → fixed in `0ba295a`, re-review clean.
  `cargo test -p s2-core` on the merged tree: 33 pass, 1 ignored (release-only literal 4 MiB paste into `cat`; passes
  with `cargo test --release -p s2-core -- --include-ignored`).
- **What changed in the core:** PTY master is non-blocking; writes go inline, a `s2-pty-write` thread finishes what a
  full PTY refuses (`PtyWriter::write_all` no longer blocks); reader back-pressure is a condvar, reader + emu joined in
  `Emu::drop`, writer in `Pty::drop`; frames without PTY output no longer satisfy key attribution; `set_waker` fires
  at once if a frame is pending or the child exited; `log.frame(seq)` folds in keys of taken-but-unlogged frames
  (`taken` capped at 256 entries); new `InputMsg::Scroll(i32)` + `ScrollToBottom`; Ctrl+punctuation and C0 `text`
  encode correctly. Contract (split overview) updated in the same commits.
- **For Track B:** the waker can fire spuriously — do not assert one wake per frame. A key or paste that sends bytes
  returns a scrolled viewport to the bottom. Under flood, key→present means "next frame after the key".
- **B1** merged (`main` fast-forward to `37d949b`). **PC image rebuilt** by the user (the session's ssh command was
  blocked by the permission classifier): PC clone on `main` at `1199fb4`, `tmls-rs-build:latest` now has the GUI libs.
- **Open, deferred to the final branch review** (ledger "minor (deferred)" lines): `pty.rs:165` write-begin time is
  stamped even when the first attempt is refused (affects only keys queued behind a full PTY); `write_then` returns
  `Ok` on the call that finds the PTY dead; child not reaped on a late spawn error; writer queue unbounded; `scrolled`
  stays true after a delta reaches the bottom; duplicated test helpers. B6 (kitty Shift+letter) still open.
- **Removed (user's yes):** untracked probe `spikes/s2-core/tests/zz_keyprobe.rs`, `/data/scratch/s2/fix/` (781 MB).
- **A2 part a** (Task 2: `s2-wgpu` atlas, shader, quad building, offscreen render, tests) — implementer running at
  log time, uncommitted changes in `Cargo.lock` + `spikes/s2-wgpu/`. Brief: `task-2-brief.md` in the ledger directory.

### NEXT (in order)
1. Task 2: read the implementer's report → task review → fix loop → ledger. Then Task 3 (A2 part b: winit window,
   input, IME, clipboard, latency marks, `--photon`, CLI; archbox smoke in Docker) and Task 4 (A3 logic).
2. (done) PC image rebuild. ssh to `pcl` from the session is blocked by the permission classifier, so PC commands
   are handed to the user to run (`! ssh pcl '…'`) unless a Bash permission rule is added.
3. PC: release build of `s2-wgpu`, then the user-run smoke run and A3 checklist (user at the PC). The user gave a
   yes in advance for the merges and the plan's PC builds/runs; check `game-mode status` first (`off` at 21:27).
4. Track B can rebase on `main` now (B2 gpui probe, B3 bench scripts in worktree `/data/scratch/s2/wt-b1`).
5. Final whole-branch review of Track A, triaging the deferred list above; then A3 user checklist, B4, the 16-run matrix.

## 2026-10-10 — S2 resumed: Task 0 on main, A1 `s2-core` built (review pending), B1 image deps

*(superseded by the entry above: the review's must-fix list is fixed and merged; NEXT 1–2 here are done.)*

The 10-08 "NEXT" list below is stale: its steps 1–2 were already done (`8eaf614` fix wave, merge `cfd0164`). The
project was paused on 10-09 and resumed today on the user's decision (PLAN header). The user uses Python tmls
(`feat/native-attach`) on the PC meanwhile.

- **Task 0** — `b13133b` on `main`: workspace members + stub crates `s2-core`, `s2-wgpu`, `s2-gpui`.
- **A1 `s2-core`** — `5f34f6b` on `spike/s2-wgpu`, **not merged**. PTY, emulator thread (libghostty-vt), `Frame` +
  latest-wins slot, key encoding, latency log + report, sampler. 12 integration tests + 1 unit test; 150 consecutive
  clean runs; clippy clean; golden log/report pair in `spikes/s2-core/tests/data/` (report computed independently in
  Python). Found while testing: the tty echo can be read before `write()` returns, so attribution uses the time the
  write began. Additive contract changes are listed in the split overview ("Contract notes from A1").
  Tests and code were written together, not failing-tests-first.
- **B1** — `37d949b` on `spike/s2-gpui` (worktree `/data/scratch/s2/wt-b1`): GUI build libs in `Dockerfile.build`.
  Image built on archbox in 53 s (3.13 GB) to validate it; the PC image is not rebuilt yet (needs the user's yes).
- **Review done** (opus): `docs/reviews/2026-10-10-opus-s2-core.md` — merge with fixes. Verified must-fix bugs: paste
  ≥ 160 KiB deadlocks the emulator thread (B1), reader thread leak under flood (B2), resize steals key attribution
  (B3), Ctrl+punctuation sends nothing (B4), control-char text mis-encoded (B5), wake-ups before `set_waker` lost
  (B7), taken-but-unlogged frames lose keys (B8). Dirty-row handling was fuzzed and found correct. **None fixed yet.**

### NEXT (in order)
1. Fix the must-fix list in `docs/reviews/2026-10-10-opus-s2-core.md` (B1–B5, B7, B8), add the missing tests it
   names, update the contract notes (and remove the stale attribution sentence), short re-review, then merge
   `spike/s2-wgpu` → `main`. Add an `InputMsg` for viewport scrolling (needed by A2).
2. Merge B1 (`37d949b`) to `main`.
3. A2 `s2-wgpu` on `spike/s2-wgpu` (Track A plan). Smoke-test on archbox first: build in the Docker image, render
   offscreen and compare pixels, and try a headless Wayland compositor inside a container for the window + key path
   (untried). Nothing is installed on a host.
4. Only then the PC: image rebuild, release build, user-run latency runs and visual/IME checklist (each needs a yes).
5. B2 gpui probe, B3 bench scripts (worktree `wt-b1`), then A3, B4, the 16-run matrix.

## 2026-10-08 — S1b round-2 fix wave at code commit 8eaf614

Branch `spike/s1b-r2`. The focused ECH-on-wrapped-row and resize-epoch regressions now pass, as do wide-glyph hyperlink, alt-mode edge, and CSI-X parsing tests. `Tracker` carries the worker's scrollback setting into temporary restores. The whole-snapshot wrapped fallback was replaced by a row-aware path with conservative wide-spacer handling. `cargo test -p s1`: 73 passed, 2 ignored.

Final matrix: **3901/3901** AlignedCarry cases, **3644/3901** immediately faithful at restore, and **3883/3901** equal row-wrap flags (18 diagnostic survivors). Final handoff: **480/480** plain and **480/480** resized, zero client reply attempts; the three former `wrap_pending` resize failures are gone. Exact artifacts: `/data/scratch/s1/results/2026-10-08-s1b-r2-final/matrix/` and `/data/scratch/s1/results/2026-10-08-s1b-r2-final/handoff/`, both stamped with `8eaf614`. Production attach remains provisional pending transport/atomic subscription, side effects, and broader reply suppression. Compact ≤200-byte repros for the 18 wrap-flag survivors remain open. Phase B waits for Claude re-review and merge.

## 2026-10-08 — S1b round 2: corpus gate closed

Branch `spike/s1b-r2`, Linux archbox. ECH overlays erased empty cells with their background and skips wide-cell spacers; the same corrected snapshot is carried behind alternate-screen entries. All 73 round-1 residuals closed, including the three continuation-only mode-47 cases. AlignedCarry is **3901/3901** identical under the unchanged S1 comparison; no upstream issue remains to file. `cases.jsonl` adds post-tail mismatch, alt-entry mode, and diagnostic row-wrap flag equality (3873/3901 equal).

Unwrapped emission plus row-flag fallback improves simulated handoff to **480/480 plain and 477/480 resized** across 24 streams, 20 seeded splits each. The three remaining resize text mismatches are `wrap_pending` splits 679/681/688. A secondary-DA query was added to the short replica query filter; final client PTY reply attempts are zero. Production §5.2 remains provisional because the three resize cases, real transport/atomic subscription, and phase-aware side-effect suppression remain open. Results: `/data/scratch/s1/results/2026-10-08-s1b-r2-run5/` and `/data/scratch/s1/results/2026-10-08-s1b-r2-handoff-verified/`. See `docs/spikes/S1b-results.md` and `docs/spikes/S1b-residuals.md`.

## 2026-10-08 — S1b snapshot repair measured; gate remains open

Branch `spike/s1b-snapshot-repair`, Linux archbox. Results: `docs/spikes/S1b-results.md`; residuals and upstream issue drafts: `docs/spikes/S1b-residuals.md`. The unchanged S1 comparison reports AlignedCarry 3271/3344 (97.8%) on the original corpus, 3828/3901 (98.1%) with four new recordings; all 557 prompt-first/nested2 splits are identical. Naive/Aligned/AlignedUnwrap original-corpus totals remain 1688/2628/2618. The 73 residuals are generated-stream styled/erased blank cells; three pass the immediate snapshot comparison and diverge after continuation, so their cause remains open. There are three ≤200-byte ignored repro tests and two proposed issue drafts; no issue was filed.

`carry::Tracker` and `snapshot_full` now preserve title/pwd, kitty stack, saved cursor/pen, primary screen behind alt (including combined private-mode sequences), hyperlinks and active OSC 8, trailing blank history rows, origin-safe cursor, and pending wrap including wide glyphs. `s1 handoff` simulated 20 aligned splits × 20 streams × plain/resize: 385/400 plain and 309/400 resize identical, zero client PTY reply attempts on those streams, 586 observed side-effect callback invocations with inert handlers. Resize reflow is unresolved; the short client query filter and inert callback are a simulation rather than a general Ghostty reply-disable switch or phase-aware side-effect suppression. No real network or atomic replay-subscription race was tested.

Corpus v2 (`vim_prompt`, `less_prompt`, `btop_prompt`, `nested2`) preserves the original seven recordings. Claude remains skipped pending repo trust confirmation; Windows MSVC, graphics, and 10k-line scrollback performance remain. PLAN §3/§5.2/§6 marks S1b measured but open; §5.2 attach/reconnect is not frozen. Runs: `/data/scratch/s1/results/2026-10-08-s1b-run5/` (original corpus), `/data/scratch/s1/results/2026-10-08-s1b-run6/` (expanded corpus), and `run5/handoff/`.

## 2026-10-08 — S1b round 2 under review; next-step sequence recorded (continue tomorrow)
Codex implemented `docs/superpowers/plans/2026-10-08-s1b-round2.md` on `spike/s1b-r2` (cf5abc0 ECH overlay, 76ed938
alt re-entry, 6654a00 soft-wrap carry, 56a63d0 docs; f653028 Codex self-review). Claimed + verified by the opus
whole-branch review: AlignedCarry **3901/3901** identical under the unchanged S1 comparison; handoff 480/480 plain,
477/480 resize; 0 client reply attempts; 65 tests. Both reviews: merge-with-fixes. Open: ECH resets ghostty's soft-wrap
flag (Terminal.zig:3036) and the fallback was whole-snapshot → 28 unequal-row-flag snapshots + the 3 resize failures
(carry bug, not Formatter limit); PLAN §6 gate text was rewritten (must be restored); 257/3901 snapshots not faithful
until the tail (report as attach-time fidelity). Fix-wave prompt (Phase A) + S2 Task 1–2 prompt (Phase B) handed to
Codex in chat; Codex login on archbox expired (`codex login` needed) — user runs Codex reviews from the PC meanwhile.

### NEXT (in order)
1. Codex: Phase A fix wave on `spike/s1b-r2` (per-row unwrap + wrap restore after ECH; CSI-X test parse; hyperlink
   wide-spacer skip; scrollback param; alt-mode edge tests 1047/1049/nested; rerun `s1 run` + `s1 handoff` with the
   commit hash stamped; PLAN §6 S1b text restored; faithful-at-restore column; residuals doc updates).
2. Claude: scoped re-review (opus), rerun matrix/handoff at HEAD, merge `spike/s1b-r2` → main, sync PC, PROGRESS/memory.
   Gate S1b closed only if 3901/3901 + 480/480 + row flags equal; else open with named residuals.
3. Codex: Phase B — `spikes/s2-core` + `spikes/s2-wgpu` on `spike/s2` (compile-only; Dockerfile GUI libs added).
4. Claude: review B; rebuild the PC Docker image (ssh, Docker only).
5. User at PC (yes per session): `s2-wgpu` smoke run (`tooling/docker/build.sh build --release -p s2-wgpu`, ldd check,
   `target-docker/release/s2-wgpu --bench ~/s2-logs/wgpu-smoke`); Claude reads the latency log; iterate.
6. Codex: S2 Task 3 (sidebar/field/menu/IME) → user checklist (fcitx5 compose, wide-char copy, 125 % scaling,
   monitor move) → `docs/spikes/S2-checklist-wgpu.md`.
7. Codex: Task 4 `s2-gpui` → smoke + checklist. 8. Codex: Task 5 `s2-bench`.
9. User + Claude: 16-run matrix on the PC + photon video (phone 240 fps) → `S2-results.md`, GUI decision in PLAN §3,
   Codex gate, merge.
10. Then S3–S7 (worker survival, transport, Claude status, mixed clients, ConPTY) → re-estimate → Phase 1a headless core.
Side tasks for the user: accept Claude Code trust in `~/Others/tmls-rs` once (enables `record.sh claude`); file the
upstream ghostty issue drafts left in `S1b-residuals.md` after Phase A; `codex login` on archbox.

## 2026-10-07 (late) — S1 merged; plans for S1b + S2 written for Codex/DeepSeek ✅
`spike/s1` merged into main (`7fd556b`), partial `claude.bin`/`claude.meta.json` removed (`d8fca23`, user consent).
PC clone synced. Reviews: `docs/reviews/2026-10-07-codex-sol-s1.md` + final branch review → one fix wave
(`f2f3437`, `697ad9f`), re-review clean, 31 tests pass on HEAD.
- **Plans (hand-off to Codex + DeepSeek; Claude plans/verifies):**
  - `docs/superpowers/plans/2026-10-08-s1b-snapshot-repair.md` — worker state carry (`carry::Tracker`,
    `snapshot_full`: trailing-row pad, hyperlink overlay, pending wrap, title/pwd, kitty stack, DECSC, DECOM-safe
    CUP, primary-at-alt-entry prefix) → `AlignedCarry` strategy in the split matrix; residual analysis + upstream
    issue drafts; handoff simulation (offset, resize epoch, client never replies); corpus v2 (prompt-first,
    nested2, claude). Headless → archbox.
  - `docs/superpowers/plans/2026-10-08-s2-interaction-slice.md` — `s2-core` (PTY + emulator thread + Frame + key
    encoding + latency log), `s2-wgpu` (winit+wgpu grid, atlas, IME, sidebar, field, menu) vs `s2-gpui`, `s2-bench`
    (ydotool driver, CPU/RSS/GPU sampler, photon square) → 16-run matrix on the PC with the user present; decision
    for PLAN §3. Builds in the PC Docker image; runs need the user's yes per PC session.
- **Gotchas carried forward:** Zig 0.15.2 only; `max_scrollback` = bytes; no `snapshot.h` at a887df42; `57151cb`
  does not build alone (bisect caveat); ydotool types into whatever is focused (probe first).

### NEXT
1. Codex/DeepSeek execute S1b (archbox) and S2 (PC Docker builds; user-run measurements) from the two plans;
   Claude reviews each task + runs the Codex gate.
2. User: accept Claude Code trust in `~/Others/tmls-rs` once so `record.sh claude` can record the TUI (S1b Task 6).
3. Later: Windows MSVC build of libghostty-vt (laptop); native snapshot path (newer ghostty + own bindings).

## 2026-10-07 (night) — S1 emulator spike: partially passed (Linux, Formatter path); snapshot repair → S1b
Branch `spike/s1` (commits `0813259`..`e23eba0` + this docs commit), run on archbox (Linux only). Results doc
`docs/spikes/S1-results.md`; PLAN §3 Emulator row, §5.2, §6 S1 row and status line updated. Plan
`docs/superpowers/plans/2026-10-07-s1-emulator.md`, ledger `.superpowers/sdd/2026-10-07-s1-emulator/progress.md`.
- **Harness** `spikes/s1` (`s1 gen|capture|run|portability|resize`): grid/state capture + diff, adversarial
  generator (13 streams), VT splitter mirroring ghostty `parse_table.zig`, Formatter snapshot/restore,
  split-matrix runner + taxonomy, pyte portability probe, resize-at-split. Corpus: 7 recordings at 120x40
  (vim btop less cargo gitlog nested unicode; `spikes/s1/corpus/` = `/data/scratch/s1/corpus/`) + 13 generated.
- **Snapshot path:** pinned ghostty a887df42 has **no `snapshot.h`** → only `fmt::Formatter` `Format::Vt`
  replayed into a fresh `Terminal` ("VT-reconstruction snapshot").
- **Split matrix** (run6, `/data/scratch/s1/results/2026-10-07-s1-run6/{summary,taxonomy}.md`; 20 streams, ≤ 400
  splits each, 3344 cases/strategy, wall 16.2 s):
  | strategy | identical | % |
  |---|---|---|
  | Naive (split at the byte) | 1688 | 50.5 |
  | Aligned (last VT boundary + tail carry) | 2628 | 78.6 |
  | AlignedUnwrap (Aligned + `unwrap(true)`) | 2618 | 78.3 |
  "Identical" = under the S1 comparison (normalised viewport cells + listed TermState fields + right-trimmed history
  text). Over 2833 distinct Aligned snapshot points: 2310 identical (81.5%). At the 1670 non-boundary splits Naive
  fails 1315; under Aligned 940 of those become identical and 375 still fail (text 201, scrollback 100, alt_screen
  57, title 16, style 1) — Formatter gaps hidden by Naive's first-match label. Recordings btop/less/nested/vim 100%,
  cargo 93.6, gitlog 86.5, unicode 86.0 (weak: no prompt-first recordings; `nested.bin` mostly blank, record.sh
  quoting bug fixed, not re-recorded). The 716 Aligned mismatches, each assigned a proposed cause (first match;
  DECSC and erased bg are hypotheses): hyperlinks 321 (VT Formatter never emits OSC 8,
  `formatter.zig:1246-1250`), trailing blank row → history row lost 126, primary screen lost behind alt 109, DECSC
  saved cursor 69, title 37, erased-cell bg 25, pending wrap 12, invisible-only 17.
- **Portability** (run3/portability), text only: right-trimmed visible text of one end-of-stream snapshot per stream,
  pyte(snapshot) = libghostty restore(snapshot) on 577/592 rows (97.5%); 4/20 snapshots near-empty; pyte(snapshot)
  vs uninterrupted D only 351/592. No colours/attributes/history/mid-stream.
- **Resize** (run3/resize): restore at the snapshot's own size, then resize (restoring into another size breaks) —
  4 streams, tested order only; `snapshot_unwrap` needed for resize-before-tail on wrap_pending, costs 10
  wrap_pending cases at same size → not a blanket default.
- **Cost:** snapshot p50 9–390 µs, p95 ≤ 982 µs (gitlog 55 KB); restore p50 31–332 µs incl. `Terminal::new`.
- **Gate:** S1 partially passed under the S1 comparison — the pinned Linux build parsed the S1 corpus and exposes the
  capture API S1 needed; authoritative-worker operation and recovery were not exercised. The Formatter alone is not
  a sufficient snapshot: boundary alignment measured; worker state carry designed, untested → new gate **S1b**
  (PLAN §6). Native snapshot = follow-up.
- **Reviews** (Codex `docs/reviews/2026-10-07-codex-sol-s1.md` + final review) folded in: results doc wording/numbers,
  PLAN §3/§5.2/§6 (S1b row)/§10; `build-env.sh` guard, `record.sh` nested quoting + claude cwd (repo dir).
  Fix-wave report `.superpowers/sdd/2026-10-07-s1-emulator/fixwave-report.md`.
- **Gotchas:** libghostty-vt 0.2.2 needs **Zig 0.15.2** (README's 0.16 fails); `Options::max_scrollback` is
  **bytes**, not lines (ghostty `Screen.zig:282`); no `snapshot.h` at the pin; commit **`57151cb` does not build
  alone** (it swept in Task 4's in-progress hunks; `ddf25e0`+ build); workspace-wide cargo now needs Zig
  (`source spikes/s1/build-env.sh`).

### NEXT
1. S2 GUI interaction slice on the PC (live desktop, user driving).
2. **S1b snapshot repair** (PLAN §6): atomic handoff (snapshot + offset + resize epoch + subscription), client restore
   with replies/side effects off, continuation tests (primary-at-alt-entry, hyperlinks, DECSC, kitty stack, pending
   wrap); re-record `nested` + prompt-first vim/less/btop; DECOM fix-up; 10k-line scrollback.
3. S1 follow-up: native snapshot (newer ghostty with `snapshot.h` + own bindings) or upstream VT fixes (OSC 8,
   trailing blank rows, erased bg); re-run `s1 run`.
4. Windows MSVC build of libghostty-vt (S1's Windows half).
5. Record the Claude TUI stream: accept folder trust in `~/Others/tmls-rs`, then `spikes/s1/corpus/record.sh claude`.

## 2026-10-07 (eve) — S0 gate closed: kitty-hosted + real-keyboard runs on the PC ✅
User-run measurements on the PC (pcl, CachyOS, KDE Wayland, kitty 0.49.2); results folded into
`docs/spikes/S0-results.md` (new P6 and K1 sections, Summary, Not measured, Run inventory), PLAN §1/§6 updated.
- **P6 — kitty-hosted P1/P2/P5** (run `20261007-211414`, 10k @ 50 Hz, quiet, nonce `b06d92954b70e4a4`; summary
  `/data/scratch/s0/runs/P6-20261007-211414/summary.md`, JSON beside it; tmls row detected K=5). Receiver-observed;
  same scripted-PTY bytes, so kitty's keyboard handling is NOT exercised; cross-host one-way carries unrechecked
  skew, compare by ack:
  | path | arrived | missing | dup | corrupt | acked | one-way p50/p95/p99/max | ack p50/p95/p99/max |
  |---|---|---|---|---|---|---|---|
  | P6-kitty-local | 10000 | 0 | 0 | 0 | 10000 | 0.0/0.0/0.0/0.9 | 0.0/0.0/0.1/2.3 |
  | P6-kitty-ssh (pcl→archbox) | 10000 | 0 | 0 | 0 | 10000 | 5.7/14.6/16.1/26.6 | 11.0/19.9/21.5/31.2 |
  | P6-kitty-tmls (pcl→archbox) | 10000 | 0 | 0 | 1250 | 9936 | 16.9/55.0/62.7/88.8 | 49.5/93.3/108.9/215.2 |
- **K1 — real keyboard events** (run `K1-20261008-035007`, `/data/scratch/s0/runs/K1-20261008-035007/{recv.jsonl,report.json}`):
  `ydotool type` (uinput, key-delay 20 ms, ~3.4 events/s) into a focused kitty running
  `s0 recv --plain --quit-after 2000 --ack false`. Plain letters + markers only (no Alt/Ctrl/F-keys/paste).
  `K1-kitty-ydotool`: 2000 events, 2000 arrived, 0 missing/dup/corrupt, complete=yes; 2001 arrivals = 2000 + the
  focus probe `x#999999` (out_of_range 1, intact=false by design). No latency (ydotool has no timestamps).
  Aborted/misfocused attempts `K1-20261008-023228`, `-025903`, `-033637` are not measurements.
- **Lesson:** `ydotool` types into whatever window is focused — the K1 script now probes focus first (and the
  receiver prints a CLICK THIS WINDOW banner).
- **Gate:** S0 closed for key loss (none found outside tmls); presentation latency and modifier keys via a real
  keyboard remain for S2.

### NEXT
1. S1 (emulator) on the PC Docker image (`tmls-rs-build:latest`): snapshot/restore round-trip via libghostty VT
   Formatter at adversarial byte boundaries.
2. S2 GUI slice on the PC's live desktop with the user driving (key-to-photon, modifier/Alt keys via real keyboard).
3. tmls quick fixes: `ENABLE_COMMAND_PALETTE = False`; Alt-key handling per defect #2; then re-run P4/P5.
4. Open mechanism controls (results doc "Open (from reviews)"): Ctrl+P with palette disabled; Esc→next-key interval
   independent of rate / `ESCDELAY`; ssh obfuscation on/off across the LAN with fresh skew samples.

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
