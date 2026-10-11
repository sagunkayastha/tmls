# tmls-rs: right panel v2 and a proper review — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The right panel shows images, has a draggable divider and can list the images a session produced; then the whole app gets one structured review (alignment, real-window behaviour, code, speed) and a pick-list of cheap additions.

**Architecture:** The panel stays one `viewer::SidePanel` per window with a `Body` enum (`Text` exists; `Image` and `Made` are added). An image is decoded off the window's thread, uploaded once as a wgpu texture and drawn by a second, tiny pipeline in its own render pass after the existing one, so nothing changes on the key-to-frame path while the panel is closed. The "made here" list is one shell command on the session's host that greps the Claude transcript for image paths; it runs on demand, never on the poll loop.

**Tech Stack:** Rust, winit 0.30, wgpu 30, `s2-wgpu` renderer (glyph atlas, instanced quads), `client-core` (ssh + tmux argv builders). New crate dependency: `image` 0.25 with only `png`, `jpeg`, `gif`, `webp`, `bmp`.

**Spec:** the user's message of 2026-10-11 (quoted in "What was asked" below). There is no separate spec file.

**Starting point:** branch `feat/viewer-panel` at `568949a` (text panel works; headless e2e 122/122). Work continues on that branch. Nothing is merged to `main` or pushed.

## Who does the work (the user's instruction, 2026-10-11)

"Use one main fable agent and multiple opus subagents."

- **One main agent, model `fable`:** owns this plan from Task 1 to Task 7, keeps the ledger, makes the rulings, runs the verification itself (tests, clippy, fmt, e2e, looks at the screenshots) and reports. It writes no feature code itself.
- **Opus subagents (`model: opus`), dispatched by the main agent:** one implementer per task (Tasks 1, 2, 3 and the fixes of Task 4, one after another, never two at once: they share `draw.rs` and `app.rs`); one task reviewer after each; one whole-branch reviewer for Task 6. Reviewers get the diff as a file, not the main agent's conclusions.
- The main agent does not build on the PC, merge, push, delete or message anyone. It stops and reports for: a destructive step, a merge or push, anything on the PC, and Task 4 step 2 / Task 5 (they need the user).
- The session that wrote this plan (TMLS) starts the main agent only after the user has answered the open questions at the end, checks its report against the repo, builds on the PC and tells the user.

## What was asked

1. A plan for a proper review of the app.
2. "There are still a lot of alignment issues we can fix."
3. "There might be things we can add … without much performance loss."
4. First: the image panel with draggable dividers. Only images are viewed in the panel; docs, PDFs and the rest open in the native app.
5. On the right panel, an option to list the files this session created: not code, but images (plots, experiment results, sketches).

## Decisions I took (veto any)

| # | Decision | Why |
|---|---|---|
| D1 | Text files keep opening in the panel (already built). "Only images" is read as "no PDF/docx viewer", not "remove text". | Text view is done and cheap. Say so if you want it gone. |
| D2 | "Made here" = image paths that appear in the session's Claude transcript and still exist on that host, newest first. No "created after session start" filter. | Your sketches are made by the sketchpad, not by the session, and you want them listed. A grep of this session's 10 MB transcript took 0.02 s and found 67 existing images. |
| D3 | The list is for Claude sessions only. A tab without a Claude status file says "no Claude session in this tab". | Codex and opencode keep their logs elsewhere; add later if wanted. |
| D4 | Image extensions listed and viewed: `png jpg jpeg gif webp bmp`. `svg`, `tif`, `heic`, `avif`, `ico` go to the native app. | Keeps the dependency small. GIF shows its first frame. |
| D5 | Image view: fits the panel (never enlarged past 100 %); a click switches between fit and 100 %; at 100 % the wheel scrolls, Shift+wheel sideways. | Smallest thing that lets you read a dense plot. |
| D6 | The panel opens without a file via `Alt+Shift+F` and a `Files` label at the right end of the tab bar; it opens on the list. | The foot's button row is full at 20 columns. |
| D7 | Divider and list widths are saved in `~/.config/tmls-rs/ui` (a new file; not in `~/.config/tmls`, which tmls owns). | You resize once, not at each start. |
| D8 | Paths with spaces are not found by the list and not clickable (v1). | Needs quoting rules in three places; listed under "Additions". |

## Global Constraints

- Never run or ask for compute allocation. Never delete or move files without the user's yes.
- Scratch output under `/data/scratch/tmls-rs-v0/`, never `/tmp`. `spikes/s1/build-env.sh` resets `TMPDIR`; use an explicit variable for log paths.
- Git: name `sgkayast`, email `sgn.kayastha@gmail.com`, no Claude attribution. Commit on `feat/viewer-panel` only. No push, no merge to `main` without a yes.
- Tests never touch the user's live tmux: private server, `tmlsrs-test-` prefix. No input injection into the live KDE session.
- `client-core` is not rustfmt-clean; do not reformat it wholesale. `cargo fmt -p desktop --check` must stay clean.
- Every image mentioned to the user is given by its full absolute path.
- After each verified change I build on the PC myself (`ssh pcl`, Docker, game mode checked first) and tell the user to restart `tmls-rs`.
- Commands: desktop tests `tooling/docker/build.sh test -p desktop`; lints after `source spikes/s1/build-env.sh`: `cargo clippy -p desktop -p client-core --all-targets -- -D warnings`; e2e `SMOKE_OUT=<new dir> tooling/smoke/run.sh`.

## Review Focus

1. A huge or broken image (100 MB PNG, 20000×20000, truncated JPEG, a `.png` that is text): the panel says why in the status line; no panic, no freeze, memory bounded.
2. A slow or dead ssh link while an image or the list loads: the window keeps drawing and typing; a newer click cancels the older answer; the timeout is reported.
3. A window too narrow for terminal + panel, or a divider dragged to either end: both sides keep at least 20 columns or the panel does not open; no zero-size texture or grid.
4. A transcript with thousands of image paths, deleted files, or none: the list is capped at 500, skips what no longer exists, and says "nothing yet" when empty.
5. The panel open during tab switch, zoom (Ctrl+=), scale change and window resize: rows stay on the grid and the image keeps its aspect.

Each line has its test in the task that owns the code (T1: 3, 5; T2: 1, 2, 5; T3: 2, 4).

---

## Part A — Right panel v2

### Task 1: Draggable divider, saved widths

**Files:**
- Modify: `crates/desktop/src/draw.rs` (`layout_with`, `Layout`, `Hit`), `crates/desktop/src/app.rs` (drag state, cursor), `crates/desktop/src/theme.rs`
- Create: `crates/desktop/src/uistate.rs`
- Test: `crates/desktop/tests/layout.rs`, `crates/desktop/tests/uistate.rs`

**Interfaces:**
- Produces: `draw::layout_with(window, scale, m, theme, side: Option<usize>)` — `side` is the panel's width in columns, `None` = closed (replaces the `bool`). `theme::PANEL_MIN_COLS: usize = 20`. `Hit::SideDivider`. `Layout.side_grab: Rect` (the divider widened by `Layout.grab` on both sides, as the list's separator is).
- Produces: `uistate::UiState { sidebar_cols: Option<usize>, panel_cols: Option<usize> }`, `uistate::path(xdg_config_home, home) -> Option<PathBuf>` (`…/tmls-rs/ui`), `uistate::parse(&str) -> UiState`, `uistate::render(&UiState) -> String` (lines `sidebar_cols=N`, `panel_cols=N`; unknown lines ignored).

- [ ] **Step 1: failing layout tests.** In `tests/layout.rs`: (a) `layout_with(.., Some(40))` gives `side_cols == 40` when it fits; (b) asked for more than fits, the terminal keeps `PANEL_MIN_COLS` columns; (c) asked for less than `PANEL_MIN_COLS`, the panel gets `PANEL_MIN_COLS`; (d) a window with fewer than `2 * PANEL_MIN_COLS` columns right of the list gives no panel (`side.w == 0.0`, `grid` as without panel); (e) `hit()` on the divider's x ± `grab` is `Hit::SideDivider`, and `Hit::Terminal` / `Hit::Side` beyond it; (f) cases a–d at scales 1.0, 1.25, 1.5, 2.0.
- [ ] **Step 2:** run `tooling/docker/build.sh test -p desktop --test layout`; expect compile errors on `Some(40)` and `Hit::SideDivider`.
- [ ] **Step 3: implement** in `layout_with`: `panel_w = cols * cell_w + pad + line`, clamped so that `terminal.w >= PANEL_MIN_COLS * cell_w` and `side_cols >= PANEL_MIN_COLS`; default when the panel opens with no saved width is half of the right area, in whole columns.
- [ ] **Step 4: drag** in `app.rs`, mirroring `drag_separator` (`app.rs:595`): `dragging_divider: bool`, `panel_cols: Option<usize>`, `drag_divider(x)` computes `cols = ((win_w - x) / cell_w).round()`, clamps, calls `relayout()` only when the value changed. Cursor `ColResize` on `Hit::SideDivider` (the code at `app.rs:1557` gains one more case).
- [ ] **Step 5: failing `uistate` tests:** `parse(render(x)) == x`; garbage lines and a missing file give `UiState::default()`; `path(None, Some("/h"))` is `/h/.config/tmls-rs/ui`.
- [ ] **Step 6: implement `uistate`;** load in `Running::new`, write when a drag ends (mouse release), errors to the status line, never a panic.
- [ ] **Step 7: e2e.** In `tooling/smoke/e2e.sh` after the panel opens: a press–move–release needs `vptr.py` to learn `drag X1 Y X2`; add it (press at X1, three motions, release at X2). Check: the pane's `#{window_width}` grows when the divider is dragged right, and `$HOME/.config/tmls-rs/ui` then holds `panel_cols=`. Update the existing check "at exit ~/.config/tmls holds pins and nothing else" only if it also looks at `tmls-rs/` (it should not).
- [ ] **Step 8:** all tests, clippy, fmt; commit `feat(desktop): the file panel's divider can be dragged; widths are remembered`.

### Task 2: Images in the panel

**Files:**
- Modify: `crates/desktop/Cargo.toml` (add `image = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "webp", "bmp"] }`), `crates/desktop/src/viewer.rs`, `crates/desktop/src/draw.rs`, `crates/desktop/src/app.rs`, `crates/desktop/src/lib.rs`
- Create: `crates/desktop/src/picture.rs`, `crates/desktop/src/picture.wgsl`
- Test: `crates/desktop/tests/viewer.rs`, `crates/desktop/tests/picture.rs`, `tooling/smoke/e2e.sh`

**Interfaces:**
- Consumes: `Layout.side_body: Rect` (exists), `viewer::open_link` and `client_core::files::read_argv` (exist).
- Produces in `viewer`: `IMAGES: [&str; 6] = ["png","jpg","jpeg","gif","webp","bmp"]`; `MAX_IMAGE_BYTES: u64 = 25_000_000`; `MAX_IMAGE_PIXELS: u64 = 50_000_000`; `MAX_SIDE: u32 = 4096`; `struct Picture { width: u32, height: u32, rgba: Vec<u8> }`; `fn decode(bytes: &[u8], max_side: u32) -> Result<Picture, String>`; `Opened::Image { host, path, picture: Picture }`; `Body::Image(ImageBody)`; `struct ImageBody { size: (u32, u32), actual: bool, scroll: (f32, f32) }`; `fn place(image: (u32, u32), area: Rect, actual: bool, scroll: (f32, f32)) -> (Rect, Rect)` returning (where on screen, which part of the image in texels).
- Produces in `picture`: `struct PictureRenderer`; `fn new(device, format) -> Self`; `fn set(&mut self, device, queue, picture: Option<&Picture>)`; `fn draw(&self, encoder, view, viewport: (u32, u32), dest: Rect, source: Rect, clip: Rect)` — one pass with `LoadOp::Load`, scissor = `clip`, linear sampler, premultiplied output, same sRGB convention as `spikes/s2-wgpu/src/gfx/shader.wgsl`.

- [ ] **Step 1: failing `decode` tests** (images are made in the test with the `image` crate's encoder, no files in the repo): a 3×2 PNG decodes to 3×2 with the right four corner pixels; a 6000×10 PNG comes back 4096 wide with its aspect kept; text bytes named `.png` give `Err` containing "not an image"; a header that claims 20000×20000 gives `Err("image too large")` **before** any pixel buffer is allocated (use the decoder's dimensions first); a truncated JPEG gives `Err`, no panic.
- [ ] **Step 2: failing `place` tests:** 100×50 in a 400×400 area in fit mode is drawn 100×50, centred (never enlarged); 1000×500 in 400×400 is 400×200, centred; in actual mode the source rect is the area's size in texels, clamped by scroll at both ends; a zero-size area gives a zero-size dest (no division by zero).
- [ ] **Step 3: implement `decode` and `place`.** Run the two test files; all pass.
- [ ] **Step 4: routing.** `opens_outside` is false for `IMAGES`. `open_link` reads an image with `read_argv(host, path, MAX_IMAGE_BYTES + 1, ..)` and a 60 s timeout, then `decode`; a file over the cap is not decoded and goes to the status line as "too large for the panel: Open in app". Tests with the fake `run` closure that `tests/viewer.rs` already uses: a `.png` gives `Opened::Image`; an oversize one gives the message; a `.pdf` still gives `Opened::Outside`.
- [ ] **Step 5: failing GPU test** `tests/picture.rs`, using `s2_wgpu::gfx::render::Gpu::headless()` as the offscreen tests of `s2-wgpu` do: a 2×2 picture (red, green, blue, white) drawn to fill a 64×64 target reads back red at (8,8), green at (56,8), blue at (8,56), white at (56,56); with `clip` set to the left half, (56,8) keeps the clear colour. Skips with a message when no adapter exists, as the existing GPU tests do.
- [ ] **Step 6: implement `picture.rs` + `picture.wgsl`.** Texture `Rgba8UnormSrgb`, one bind group (uniform: viewport, dest, source; texture; sampler), four-vertex strip.
- [ ] **Step 7: wire into `app.rs`.** `viewed()` handles `Opened::Image`: `picture.set(.., Some(&p))`, `side = Some(SidePanel::image(..))`. In `redraw`, after `self.renderer.draw(..)`: if the body is an image, `picture.draw(..)` into the same `view`. `close_side()` and a replaced file call `picture.set(.., None)` so the texture is freed. A click in `Hit::Side` on an image toggles `actual`; the wheel scrolls only when `actual`.
- [ ] **Step 8: header.** The header's title gets ` 1280×800 · 43 %` after the path (percent = drawn/actual). Unit test in `tests/side_draw.rs`.
- [ ] **Step 9: e2e.** Make a 400×300 PNG in the scratch home with `python3` (stdlib `zlib` + `struct`, solid orange `#FF8800`), fill the pane with its path, Ctrl+click, screenshot `12d-image-panel`, and check with the `grim -t ppm` reader already in the script that the screenshot contains at least 10 000 pixels of exactly that orange inside the panel's rectangle. Then Alt+Shift+X.
- [ ] **Step 10:** all tests, clippy, fmt; look at the screenshot; commit `feat(desktop): images open in the panel`.

### Task 3: "Made here" — the session's images

**Files:**
- Modify: `crates/client-core/src/listing.rs` (keep `sessionId`), `crates/client-core/src/files.rs`, `crates/desktop/src/viewer.rs`, `crates/desktop/src/draw.rs`, `crates/desktop/src/keys.rs`, `crates/desktop/src/app.rs`
- Test: `crates/client-core/tests/listing.rs`, `crates/client-core/tests/files.rs`, `crates/desktop/tests/viewer.rs`, `crates/desktop/tests/side_draw.rs`, `crates/desktop/tests/keys.rs`, `tooling/smoke/e2e.sh`

**Interfaces:**
- Produces in `client-core`: `SessionInfo.session_id: Option<String>` (from the status file's `sessionId`; only `[0-9a-f-]{36}` is accepted, anything else is `None`); `files::made_argv(host: &str, session_id: &str, cwd: Option<&str>, local_name: &str) -> Vec<String>`; `files::Made { path: String, modified_s: u64, bytes: u64 }`; `files::parse_made(out: &str) -> Vec<Made>`; `files::MADE_MAX: usize = 500`.
- The command `made_argv` runs on the host (shown for the reviewer; the session id is validated, so nothing typed by a person reaches the shell):

```sh
f=$(ls -t "$HOME"/.claude/projects/*/SESSION_ID.jsonl 2>/dev/null | head -n 1)
[ -n "$f" ] || { echo "no-transcript"; exit 0; }
grep -aoE '/[A-Za-z0-9_.~+@%=,/-]+\.(png|jpe?g|gif|webp|bmp)' "$f" | sort -u |
while IFS= read -r p; do
  [ -f "$p" ] && stat -c '%Y	%s	%n' -- "$p"
done | sort -rn | head -n 500
```

- Produces in `desktop`: `Global::ToggleFiles` on `Alt+Shift+F`; `Hit::Files` (the `Files` label at the tab bar's right end); `Hit::SideBack`; `Opened::Made { host, session: String, files: Vec<Made> }`; `Body::Made(MadeBody)` with `MadeBody::rows(visible, cols, now_s) -> Vec<MadeRow>`, `MadeRow { when: String, name: String, folder: String, selected: bool }`; `SidePanel::back: Option<MadeBody>` (the list an image was opened from).

- [ ] **Step 1: failing parser tests** (`client-core`): a status file with `sessionId` gives `session_id == Some(..)`; one with `"sessionId":"$(reboot)"` gives `None`. `parse_made` of three `mtime\tsize\tpath` lines keeps their order; a line with a tab missing is skipped; `no-transcript` gives an empty list plus a flag (return `Result<Vec<Made>, NoTranscript>`).
- [ ] **Step 2: failing real-shell test** in `crates/client-core/tests/files.rs` (runs `sh` locally, as `tmux_real.rs` runs tmux): a temp `HOME` under `/data/scratch` with a fake `.claude/projects/x/<id>.jsonl` naming three image paths (two exist, one does not, one is a `.py`), run `made_argv`'s script, expect exactly the two existing images, newest first.
- [ ] **Step 3: implement** `session_id`, `made_argv`, `parse_made`. Tests pass.
- [ ] **Step 4: the list's model.** `MadeBody` holds the files, a cursor and a top line. `when` is `HH:MM` for today, else `Oct 11`; `name` is the file's name; `folder` is the parent path, cut at its start with `…` (`draw::keep_end`). Tests: order, cursor clamps at both ends, scrolling, an empty list renders the one line `no images from this session yet`, and a tab with no Claude session renders `no Claude session in this tab`.
- [ ] **Step 5: drawing and hits.** Header in list mode: `Images · <session name> · N` and `Refresh` in place of `Open in app`. In image mode opened from the list, a `‹` at the header's start (`Hit::SideBack`). The `Files` label is the tab bar's last 7 columns; tabs lose that width. Tests in `tests/side_draw.rs` and `tests/layout.rs` for the columns and the hit rectangles.
- [ ] **Step 6: wiring.** `Alt+Shift+F` or a click on `Files`: panel closed → opens on the list of the front tab's session and loads it on a thread (`self.view(..)`, the same stale-answer counter as file clicks); panel open → closes. A click or Enter on a row opens that image through Task 2's path with `back` set. `‹`, Backspace or Escape (panel focused by a click) returns to the list. The list reloads when: `Refresh` is clicked, the front tab changes while the list shows, or the poll reports that this session's status turned `done` or `idle` (the event already exists; no new polling).
- [ ] **Step 7: e2e.** Write a fake status file and transcript for `$ONE` in the scratch home (the script already fakes status files for `SMOKE_OUTSIDE`), naming the orange PNG of Task 2 and one missing file. `Alt+Shift+F`: pane narrower; screenshot `12e-made-list`; click the first row: orange pixels present; `Alt+Shift+F`: pane wide again.
- [ ] **Step 8:** all tests, clippy, fmt; commit `feat(desktop): the panel lists the images a Claude session made`.
- [ ] **Step 9: real check without the user's desktop.** From the PC, by hand: run the `made_argv` command for one live archbox session over ssh and for one PC session; confirm both lists are sensible and return in under a second. Record the two timings in `PROGRESS.md`.

---

## Part B — The review

Four passes. Each ends in a written list (`docs/reviews/2026-10-review.md`), not in fixes; fixes are then picked from the list with the user, cheapest first.

### Task 4: Alignment pass

Rule to enforce: **one row grid.** Every row of text in the list, the terminal, the panel, the legend and the buttons sits at `terminal.y + k * cell_h`; every bar (tab bar, panel header, status line) has the same height and the same text baseline; every left edge of text is on a whole column from its area's origin.

Seen so far, by eye, in the 1× headless screenshot `/data/scratch/tmls-rs-v0/viewer/e2e-1/12b-file-panel.png` (to be measured, and the real PC runs at another scale):

| # | Where | What looks off |
|---|---|---|
| A1 | List vs terminal | List rows start at y = 0 with 17 px rows; terminal rows start under the 22 px tab bar. Rows of the two never line up. |
| A2 | Panel vs terminal | The panel's header sits beside terminal row 1, so panel rows are about 5 px lower than terminal rows. |
| A3 | Host header | A coloured line runs through the top of the host's name. |
| A4 | Status line | Its text starts about 4 px right of the terminal's text. |
| A5 | Legend | Third line holds `X Codex` alone; the two columns are uneven. |
| A6 | Buttons | `? !` left and `✕` right on one row, `New Open Copy Sketch` on the next; spacing differs between the rows. |
| A7 | Close marks | The tab's `×` and the panel's `×` sit at different distances from their right edges. |
| A8 | `Open in app` | Bold blue, while the foot's buttons are green/blue/normal weight: three button styles in one window. |

- [ ] **Step 1: measure, not guess.** Add `tooling/smoke/rows.py` (stdlib only): reads a `grim -t ppm` dump and prints, for a given x range, the y of the first and last inked pixel of each text row. Run it on the list, terminal and panel columns of screenshots taken at scale 1 and 2 (`SMOKE_SCALE`, a new env passed to sway's `output scale`). Write the numbers into the review file.
- [ ] **Step 2: the user's own list.** Ask the user to mark what bothers them on a real screenshot from the PC with the sketchpad; add each mark as a row (`A9…`) with the sketch's full path.
- [ ] **Step 3: failing grid tests** in `tests/layout.rs`, at scales 1, 1.25, 1.5, 2 and font sizes 11, 13, 16: for every row origin `y` of list, panel body, legend and buttons, `(y - terminal.y) % cell_h == 0`; `tab_bar.h == side_header.h == status.h`; text x of status == text x of terminal.
- [ ] **Step 4: fix** `layout_with` until they pass; then A3, A5–A8 one commit each, with a before/after screenshot path in the commit body.
- [ ] **Step 5:** e2e with a fresh `SMOKE_OUT`; the click coordinates in `e2e.sh` are pixel positions and will move — update them in the same commit as the layout change.

### Task 5: Real-window pass (needs the user, about 15 minutes)

Nothing below has been run in a real window by me; all of it is only unit- and headless-tested.

- [ ] **Step 1:** write `docs/REVIEW-TEST.md`, one line per check with a box for ok / not ok / note, in the order a person meets them: start; list (pin walk Alt+Shift+←/→, drag the list's edge, `not in tmux` row and kitty focus); tabs; foot buttons (`✕`, `Sketch`, positions against the auto-hide taskbar); terminal mouse (click, drag, wheel); copy and paste (Shift+drag, Ctrl+Shift+C, Ctrl+Shift+V; note any line on the launching terminal's stderr); links; text panel; image panel; divider; made-here list; a session on archbox for each of the last four; alerts and `notify-send`; fcitx5; rename/kill; typing feel.
- [ ] **Step 2:** the user fills it in; every "not ok" becomes a row in the review file with the user's words.
- [ ] **Step 3 (only with the user's yes):** a second headless sway on the PC itself, so the same e2e runs against the PC's GPU driver and fonts instead of lavapipe in Docker. No input reaches the live KDE session.

### Task 6: Code pass

- [ ] **Step 1:** one fresh Opus reviewer over `git diff main..feat/viewer-panel`, given the diff as a file and this focus list, not my conclusions: threads and stale answers (`views` counter); quoting of host paths into ssh/sh (`files.rs`); what is written under `~/.cache/tmls-rs/view` and whether a remote path can escape it; every timeout; memory bounds (text 2 MB, image 25 MB / 50 MP / 4096 px); panics on tiny windows and zero sizes; anything drawn or computed per frame that could be per change.
- [ ] **Step 2:** known debts to rule on, each "fix now / later / never": `client-core` not rustfmt-clean; window code copied from `s2-wgpu` (MORNING-TEST "Known gaps" 12); no ssh keepalive; `--bench` logs only the first tab; untracked `crates/desktop/tests/tabs.rs.new`.
- [ ] **Step 3:** findings into the review file, most severe first; fix the confirmed ones, one commit each, test first.

### Task 7: Speed pass

The budget: with the panel closed, nothing measurable changes. With it open, typing must feel the same.

- [ ] **Step 1: baseline at `main` (`0103f4b`)** in the headless e2e, three runs: key→frame latency from `bench/latency.jsonl` (median, p95), frames drawn while idle for 10 s (must be 0), resident memory after start, start-to-first-frame time.
- [ ] **Step 2: same numbers at the branch head** in four states: panel closed; text panel open; a 4096×4096 image open; list of 500 open.
- [ ] **Step 3: pass marks.** Panel closed: median and p95 within run-to-run noise of the baseline. Any panel state: 0 frames while idle; typing p95 no more than 1 ms over baseline; memory growth ≤ 80 MB with the largest image and back to baseline after close.
- [ ] **Step 4:** the table goes into the review file; a miss is a finding, fixed before merge.

---

## Part C — Additions to pick from

Not planned in detail; each gets its own small plan once picked. Cost is my estimate.

| Addition | What you get | Cost | Speed risk |
|---|---|---|---|
| Copy/paste fix (OSC 52, errors in the status line) | Ctrl+Shift+C/V work; you see why when they do not | small | none |
| Link hover | Path or URL under the pointer is underlined with Ctrl held, so you see what is clickable | small | none (redraw on pointer move over terminal only while Ctrl is down) |
| Paths with spaces and `~/…` | More paths clickable and listed | small | none |
| `?` help panel | The `?` button shows the keys | small | none |
| ssh keepalive | Remote tabs survive a sleeping link | small | none |
| Unread mark | Sessions whose output changed since you looked, as in tmls | small | none (data is already polled) |
| Filter the list | Type to narrow the sessions | small | none |
| Thumbnails in the made-here list | A grid of small pictures instead of names | medium | decode on a thread, capped cache |
| Panel remembers per tab | Each tab brings back its own file or list | small | none |
| Approvals (Yes/No on Claude's prompt from the list) | Answer without opening the tab | medium | none |
| Quick select (Alt+Shift+S) | Pick a path, hash or URL by letter | medium | none |
| Image paste | Paste a picture into Claude | medium | none |
| Rendered Markdown in the panel | Headings, lists, code blocks | medium–large | none when closed |
| Ask/queue, `ag`, `ct` | The rest of tmls | large | to be measured |
| Native sketch board | Draw inside tmls-rs | large; four open design questions parked | to be measured |

## Order and size

1. Task 1 divider — small.
2. Task 2 images — medium (one new dependency, one new GPU pipeline).
3. Task 3 made-here list — medium.
4. PC build, user tries Part A.
5. Task 4 alignment (needs the user's marks), Task 7 speed, Task 6 code, Task 5 real-window sheet.
6. Fix what the review found; pick additions.
7. With the user's yes: merge `fix/pin-ring` and `feat/viewer-panel` into `main`; decide on the leftovers listed in `PROGRESS.md`.

## Open questions for the user

1. D1: keep the text panel, or images only?
2. D2: is "every image this session mentioned that still exists" the right list, or only images made after the session started?
3. D6: `Files` at the tab bar's right end and `Alt+Shift+F` — fine?
4. Task 2 adds the Rust crate `image` (compiled into the binary; nothing installed on any host). Fine?
5. Task 4 step 2: will you mark the alignment issues you see on a screenshot?
