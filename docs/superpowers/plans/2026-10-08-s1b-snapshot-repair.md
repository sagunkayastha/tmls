# S1b Snapshot Repair Implementation Plan

> **For agentic workers (Codex / DeepSeek):** execute task-by-task; each task ends with tests passing and one commit. Steps use checkbox (`- [ ]`) syntax. Author every commit as `sgkayast <sgn.kayastha@gmail.com>`; no AI attribution lines. Never write under `/tmp` (use `/data/scratch/s1/`). Never delete or move files without the user's explicit consent.

**Goal:** Close the fidelity gaps S1 found in libghostty-vt's Formatter snapshot by carrying terminal state the Formatter does not emit, so that a snapshot taken at any VT sequence boundary and restored into a fresh terminal continues *identically under the S1 comparison* on every corpus stream — or each residual is a documented Formatter limit with an upstream issue filed.

**Architecture:** Add a `carry` module to `spikes/s1`: a `Tracker` that watches the byte stream the worker feeds (chunked at `splitter::boundaries`) and records state the Formatter drops — title/pwd, kitty keyboard push stack, DECSC saved cursor + pen, alt-screen entry (taking a primary-screen snapshot at that moment), pending-wrap and trailing blank rows (read from the terminal at snapshot time), per-cell hyperlinks (read via `GridRef::hyperlink_uri`). `carry::snapshot_full()` = Formatter VT + `\r\n`×k trailing-row pad + hyperlink overlay + saved-cursor re-emit + kitty stack re-emit + title/pwd + primary-screen prefix when on alt + final CUP (+ pending-wrap re-emit). A new strategy `AlignedCarry` runs in the existing split matrix. Then re-record the degraded corpus streams and run the matrix again.

**Tech Stack:** Rust 1.96, `libghostty-vt = "=0.2.2"` (Zig 0.15.2 via `spikes/s1/build-env.sh`), existing `spikes/s1` modules (`grid`, `splitter`, `snap`, `experiment`, `report`). Runs on archbox (small job; no PC needed).

**Spec:** `docs/PLAN.md` §5.2 (attach = Formatter VT reconstruction at a sequence boundary with tail carry + worker state carry — DESIGN, untested), §6 row **S1b**, §5.1 (one reply authority: the restored client terminal must never answer queries). Evidence: `docs/spikes/S1-results.md` (gate table, taxonomy, "Open (from reviews)"), `docs/reviews/2026-10-07-codex-sol-s1.md`.

## Global Constraints

- Keep the S1 comparison definition unchanged (`grid::capture` + `normalize` + `diff` + history text) so numbers are comparable with run6: Naive 1688, Aligned 2628, AlignedUnwrap 2618 of 3344.
- `max_scrollback` is **bytes** in this crate version; keep `--scrollback 2000` for comparability and note it.
- Run ids: `/data/scratch/s1/results/2026-10-08-s1b-run<N>/`. Never overwrite run6.
- Corpus changes go in their own task (Task 6) with new file names (`nested2.bin`, `vim_prompt.bin`, …); the original 7 recordings stay untouched so run6 remains reproducible.
- `cargo test -p s1` must pass after every task (`source spikes/s1/build-env.sh` first). `cargo test -p s0` must still build without Zig.
- Terminology: "identical under the S1 comparison"; "Formatter VT reconstruction"; "proposed cause" until a repair makes the class disappear.

## Review Focus

1. **Snapshot taken while on the alt screen, tail leaves it** — expected identical with `AlignedCarry` (primary prefix replayed first). Test: Task 3 `alt_exit_after_restore_identical`.
2. **Snapshot with the cursor on an empty bottom row, tail scrolls** — expected `scrollback_rows` equal (trailing-row pad). Test: Task 2 `trailing_blank_rows_padded`.
3. **Hyperlink run split across a snapshot** — expected OSC 8 URIs identical per cell after restore. Test: Task 2 `hyperlink_overlay_restores_uris`.
4. **DECSC before snapshot, DECRC in the tail** — expected cursor + pen identical. Test: Task 3 `saved_cursor_restored`.
5. **Pending wrap at snapshot, tail prints one char** — expected identical. Test: Task 2 `pending_wrap_reemitted`.

---

### Task 1: `carry::Tracker` — stream-derived state

**Files:**
- Create: `spikes/s1/src/carry.rs`; modify `spikes/s1/src/lib.rs` (`pub mod carry;`)
- Test: inline

**Interfaces:**
- `pub struct Tracker { pub title: Option<String>, pub pwd: Option<String>, pub kitty_stack: Vec<u32>, pub saved_cursor: Option<SavedCursor>, pub alt_entry: Option<AltEntry> }`
- `pub struct SavedCursor { pub x: u16, pub y: u16, pub pen_sgr: Vec<u8> /* raw SGR bytes rebuilt from Terminal::cursor_style() */, pub origin_mode: bool }`
- `pub struct AltEntry { pub mode: u16 /* 47 | 1047 | 1049 */, pub primary_snapshot: Vec<u8> /* Formatter VT of the primary screen taken just BEFORE the switch */, pub primary_cursor: (u16,u16) }`
- `pub fn feed(&mut self, term: &mut Terminal, bytes: &[u8])` — splits `bytes` at `splitter::boundaries`, feeds chunk by chunk; before feeding a chunk that is exactly `ESC 7` / `CSI s` records `saved_cursor` from the terminal (`cursor_x/y`, `cursor_style()` → SGR bytes via a helper `sgr_from_style(&Style) -> Vec<u8>`), on `ESC 8` / `CSI u` (restore) leaves it; before a chunk `CSI ? 47|1047|1049 h` while on the primary screen takes `snap::snapshot(term, SnapOpts{palette:true})` into `alt_entry`; on `… l` clears `alt_entry`; kitty `CSI > n u` pushes `n`, `CSI < n u` pops `n` (default 1), `CSI = n ; m u` sets top; OSC 0/2 → `title`, OSC 7 → `pwd` (strip the NUL the Formatter adds). Everything else is just fed.
- Rationale comment: the worker owns the byte stream, so it can derive state the emulator does not expose; `feed` is the single entry point the worker would call.

- [ ] **Step 1: Failing tests** — `tracker_records_title_pwd` (OSC 0 + OSC 7 → fields), `tracker_kitty_stack_push_pop` (`CSI > 1 u`, `CSI > 5 u`, `CSI < u` → `[1]`), `tracker_saved_cursor_position` (move to (5,2), `ESC 7` → saved (5,2); pen after `CSI 1;31 m` → `pen_sgr` contains `1` and `31`), `tracker_alt_entry_takes_primary_snapshot` (print "primary", `CSI ?1049 h` → `alt_entry.primary_snapshot` restores to a grid whose row 0 starts with "primary"), `tracker_chunking_matches_boundaries` (feeding all at once vs byte-by-byte gives the same Tracker state).
- [ ] **Step 2: Run, verify fail. Step 3: Implement. Step 4: Pass.**
- [ ] **Step 5: Commit** — `s1b: carry::Tracker (title, pwd, kitty stack, DECSC, alt-entry primary snapshot)`

---

### Task 2: `carry::snapshot_full` — terminal-derived fixes

**Files:**
- Modify: `spikes/s1/src/carry.rs`, `spikes/s1/src/snap.rs` (expose `trailing_blank_rows(term) -> u16` and the CUP builder)
- Test: inline

**Interfaces:**
- `pub fn snapshot_full(term: &mut Terminal, tr: &Tracker, opts: SnapOpts) -> Vec<u8>`:
  1. If `tr.alt_entry` is `Some` and the terminal is on the alt screen: output = `primary_snapshot` + `CSI ? <mode> h` + Formatter VT of the alt screen; else Formatter VT.
  2. Append `"\r\n"` × `trailing_blank_rows(term)` (count of blank rows at the bottom of the viewport below the last non-blank row **and** below the cursor row — define precisely and test).
  3. Hyperlink overlay: for every maximal run of cells in the viewport sharing the same `hyperlink_uri` (via `grid::capture` → `Cell.hyperlink`): `CUP(row+1, col+1)` + `OSC 8 ; ; <uri> ST` + the run's text re-printed with each cell's SGR (`sgr_from_cell`) + `OSC 8 ; ; ST`. Wide cells printed once (skip spacer).
  4. Title: `OSC 0 ; <title> ST` if `tr.title`; pwd: `OSC 7 ; <pwd> ST`.
  5. Kitty stack: for each entry bottom→top `CSI > <n> u`; (the Formatter already emits the current flags — verify no double push; if it does, emit `CSI < u` first).
  6. Saved cursor: `CUP(saved)` + `pen_sgr` + `ESC 7` + `SGR 0`.
  7. Final `CUP(cursor)`; if `term.is_cursor_pending_wrap()`: re-print the cell under the cursor's column (last column) with its SGR to re-arm pending wrap.
  8. Origin mode: if DECOM set, temporarily `CSI ? 6 l` before absolute CUPs and `CSI ? 6 h` after (fixes the S1 DECOM caveat).
- Keep `snap::snapshot` unchanged (baseline).

- [ ] **Step 1: Failing tests** — `trailing_blank_rows_padded` (300 lines ending CRLF on 24 rows → restore via `snapshot_full` → `scrollback_rows` equal to D and history text equal), `hyperlink_overlay_restores_uris` (two linked runs + plain text → per-cell `hyperlink` equal after restore), `pending_wrap_reemitted` (print exactly `cols` chars → `pending_wrap` true after restore; then feed "x" to both → identical), `title_pwd_restored`, `kitty_stack_restored` (push 1, push 5 → after restore `kitty_keyboard_flags()` equal AND popping twice in both gives equal flags), `decom_cup_ok` (origin mode + scroll region → cursor equal after restore).
- [ ] **Step 2–4: fail → implement → pass.**
- [ ] **Step 5: Commit** — `s1b: carry::snapshot_full (trailing-row pad, hyperlink overlay, pending wrap, title/pwd, kitty stack, DECSC, DECOM-safe CUP)`

---

### Task 3: `AlignedCarry` strategy + matrix run

**Files:**
- Modify: `spikes/s1/src/experiment.rs` (`Strategy::AlignedCarry`), `report.rs` (4th column), `main.rs` (`run` includes it)
- Test: inline

**Interfaces:**
- `AlignedCarry`: like `Aligned`, but A is fed through a `Tracker` (`tr.feed(&mut a, &bytes[..q])` chunked), snapshot = `carry::snapshot_full(&mut a, &tr, opts)`; C = `restore` then `bytes[q..]`.
- Add to each case record: `carry_bytes` (snapshot size) and `carry_overlay_runs` (hyperlink runs emitted).

- [ ] **Step 1: Failing tests** — `alt_exit_after_restore_identical` (primary text, 1049h, alt text; split inside alt; tail = `1049l` + more text → `AlignedCarry` identical; `Aligned` classed `alt_screen`), `saved_cursor_restored` (DECSC before split, DECRC in tail → identical), `strategy_matrix_includes_carry` (run_case for all four strategies on `"ab\x1b[31mcd"`).
- [ ] **Step 2–4: fail → implement → pass.**
- [ ] **Step 5: Run** `cargo run --release -p s1 -- run --corpus-dir /data/scratch/s1/corpus --gen --out /data/scratch/s1/results/2026-10-08-s1b-run1 --cols 120 --rows 40 --scrollback 2000 --seed 1` (same flags as run6). Expected: Naive/Aligned/AlignedUnwrap totals unchanged (1688/2628/2618); `AlignedCarry` ≥ 3200 identical; residual classes listed with counts. If `AlignedCarry` < 3100, stop and report the top-3 residual classes with examples before continuing.
- [ ] **Step 6: Commit** — `s1b: AlignedCarry strategy in the split matrix` (+ paste the totals in the commit body).

---

### Task 4: Residual analysis + upstream issues

**Files:**
- Create: `docs/spikes/S1b-residuals.md`

- [ ] For every non-identical `AlignedCarry` case class: minimal reproducer stream (≤ 200 bytes) in a Rust test `residual_<class>_repro` marked `#[ignore]` with the observed vs expected grid in the test's doc comment; decide: (a) fixable in carry (then fix in Task 2/3 and rerun), (b) Formatter limit → draft an upstream issue text (title, repro bytes, expected/actual, ghostty commit a887df42, libghostty-vt 0.2.2) into the doc — **do not file it**; the user files.
- [ ] Expected residuals from S1: erased-cell background after tail ICH/DCH/ED (25 cases) — probably (b); invisible-only styled blanks (17) — decide whether `normalize` should treat styled-but-invisible blanks as equal (ruling needed → propose, don't change the comparison).
- [ ] Commit — `docs: S1b residual analysis + upstream issue drafts`

---

### Task 5: Handoff protocol test (offset + resize epoch + side effects)

**Files:**
- Create: `spikes/s1/src/handoff.rs`; test module inline; `s1 handoff` subcommand

**Interfaces:**
- Simulate the worker: `Worker { term, tracker, log: Vec<u8> /* all bytes so far */, offset: u64, epoch: u32 }` with `feed(bytes)`, `resize(cols, rows)` (bumps `epoch`), `snapshot() -> Handoff { bytes, offset, epoch, cols, rows }` (taken via `snapshot_full`); `Client::attach(h: &Handoff) -> Terminal` (restore at `h.cols × h.rows`; **install `on_pty_write` callback that records any reply attempt — must stay empty**; bell/clipboard/title callbacks counted as suppressed); `Client::apply(&mut self, bytes_after_offset, resizes_after_epoch)`.
- Test scenario: for each corpus stream and 20 seeded split points: worker feeds up to p, snapshot; then worker feeds the tail while the "network" delivers the snapshot (i.e. the snapshot bytes are fixed at offset S; tail bytes [S..] are buffered); client attaches, applies tail; compare client vs worker under the S1 comparison; also a variant with a `resize(100,30)` at a random point in the tail delivered as an epoch event (client resizes at the same point). Assert: identical (or classed into a known residual), `reply_attempts == 0` on the client.

- [ ] **Step 1: Failing tests** — `handoff_identical_quiet`, `handoff_with_resize_epoch`, `client_never_replies` (feed `CSI 6 n` (DSR) in the tail: worker's `on_pty_write` fires once, client's never).
- [ ] **Step 2–4: fail → implement → pass. Step 5: Run `s1 handoff --out …s1b-run1/handoff/` → summary table.**
- [ ] **Step 6: Commit** — `s1b: handoff protocol simulation (offset, resize epoch, client reply suppression)`

---

### Task 6: Corpus v2 — prompt-first recordings, nested fixed, Claude

**Files:**
- Modify: `spikes/s1/corpus/record.sh` (new names), `spikes/s1/corpus/README.md`

- [ ] `record.sh` v2 recordings (new names, originals untouched): `vim_prompt`, `less_prompt`, `btop_prompt` — start a `bash --norc` prompt, type two commands (`ls --color`, `echo $RANDOM`) so the primary screen has content, THEN launch the program, drive as before, quit, type one more command; `nested2` with the fixed quoting (verify the inner `ls`/`git status` text is in the bytes); `claude` — only after the user has accepted the trust dialog in `~/Others/tmls-rs` (ask; skip if not done).
- [ ] Add the new recordings to the matrix (`--corpus-dir` picks up all `.bin` except those listed in a `skip.txt`), rerun → `s1b-run2`; expect `AlignedCarry` identical on the prompt-first streams (alt-screen + primary content path exercised for real).
- [ ] Commit — `s1b: corpus v2 (prompt-first recordings, nested2, claude) + run2`

---

### Task 7: Results doc + PLAN

- [ ] `docs/spikes/S1b-results.md`: setup; per-strategy totals (run6 vs s1b-run1 vs s1b-run2); taxonomy with AlignedCarry column; residuals + upstream drafts; handoff results; corpus v2 inventory; gate verdict vs PLAN §6 S1b (pass gate text there); what remains (Windows MSVC; graphics; 10k-line scrollback perf; erased-bg upstream).
- [ ] `docs/PLAN.md`: §5.2 — flip "DESIGN, untested" to measured where S1b proved it; §6 S1b status; §3 Emulator row status.
- [ ] `PROGRESS.md` entry. Commit — `docs: S1b results; PLAN §5.2/§6 updated`

---

### Task 8: Codex review gate

- [ ] Codex (gpt-6-sol) review of `carry.rs`/`handoff.rs` + `S1b-results.md` (brief: does AlignedCarry prove §5.2's carry design; are residuals truly Formatter limits; is the handoff simulation faithful to §5.1/§5.2; wording). Save `docs/reviews/<date>-codex-sol-s1b.md`; one fix wave; re-review; merge.
