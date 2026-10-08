# S1 results — emulator snapshot/restore fidelity (2026-10-07)

Scope: **libghostty-vt 0.2.2 on Linux (archbox) only.** One question: if the session worker snapshots its
authoritative emulator at byte offset S and a fresh emulator restores that snapshot and is then fed the rest of the
stream, is the result identical to an emulator fed the whole stream? Plus: is the snapshot portable to another
emulator (pyte), and does it survive a resize? No Windows build, no graphics, no recorded Claude TUI (see
[Not measured](#not-measured--next-steps)).

Method per case: D = terminal fed the whole stream; A = terminal fed `stream[..split]`; S = `snap::snapshot(A)`;
C = fresh terminal fed S, then `stream[effective_split..]`. Identical = normalised grids equal, no state-field
mismatch, history text equal (definitions: `spikes/s1/README.md` "Experiment"; run6 `summary.md` header).

## Summary

- **The pinned ghostty (a887df42) has no `include/ghostty/vt/snapshot.h`**; the only snapshot path is the
  Formatter's `Format::Vt` output replayed into a fresh `Terminal` ("VT-reconstruction snapshot"). Everything below
  measures that path, not a native snapshot.
- **Split matrix (run6, 20 streams, 3344 cases per strategy): identical Naive 1688 (50.5%), Aligned 2628 (78.6%),
  AlignedUnwrap 2618 (78.3%).** Snapping the split back to the last VT sequence boundary and carrying the tail
  (Aligned) removes every `parser_state_split` case (Naive 1315 → 0).
- **All 716 Aligned mismatches are attributed to a named cause** (table in the gate verdict): hyperlinks dropped by
  the VT Formatter (321 first-match / 334 cases with a hyperlink mismatch), primary screen lost when the snapshot is
  taken on the alt screen (109), trailing blank screen row not emitted → one history row lost (126), DECSC saved
  cursor not emitted (69), title not emitted (37), erased-cell background lost (25), pending wrap not emitted (12),
  invisible-only styled blanks (17).
- **Recordings fare better than the adversarial set:** btop, less, nested, vim 100% identical under Aligned;
  cargo 93.6%, gitlog 86.5%, unicode 86.0% (run6 `summary.md`).
- **The VT snapshot is portable:** pyte(snapshot) equals libghostty restore(snapshot) on 577/592 rows (97.5%),
  17/20 streams; the remaining rows are pyte limits (SGR colon sub-params, `CSI = 5;1 u`, no grapheme clustering).
  pyte renders the snapshot better than the raw stream on 7 streams.
- **Resize:** restoring into a different size is not faithful; restoring at the snapshot's own size and resizing
  afterwards is identical on all 4 streams tested. The default snapshot hard-breaks soft wraps; `snapshot_unwrap`
  fixes resize-after-restore but costs 10 cases on wrap_pending at same-size restore (96.0% → 87.9%).
- **Cost:** snapshot p50 9–390 µs, p95 ≤ 982 µs (gitlog, 55 KB snapshot, 649 history rows); restore p50 31–332 µs
  incl. `Terminal::new` (run6, Aligned).
- **Build gotchas:** libghostty-vt 0.2.2 needs **Zig 0.15.2** (upstream README says 0.16 — 0.16.0 fails);
  `Options::max_scrollback` is **bytes**, not lines (ghostty `Screen.zig:282`; the Rust doc says lines).
- **Gate S1 partially passed: differences are explained; libghostty-vt is viable as the authoritative emulator; the
  Formatter alone is NOT a sufficient snapshot — worker-side state carry + boundary alignment required; native
  snapshot path = follow-up.**

## Setup

| item | value |
|---|---|
| host | archbox: Arch, x86_64, 4 cores, Rust 1.96 (`/data/scratch/s1-probe/REPORT.md`) |
| crate | `libghostty-vt = "=0.2.2"` (MIT/Apache), safe wrapper over `libghostty-vt-sys` 0.2.2, static link |
| ghostty | `libghostty-vt-sys` `build.rs` pins `GHOSTTY_COMMIT a887df42c56f6de86c0fe6da9c4eeca37931e083` ("terminal: bound page map probe lengths without tombstones (#13294)"); checked out at `GHOSTTY_SOURCE_DIR` (`spikes/s1/build-env.sh`) |
| Zig | **0.15.2 exactly**, `/data/scratch/zig/y/zig-x86_64-linux-0.15.2` (tarball sha256 verified, minisig not). 0.16.0: `Your Zig version v0.16.0 does not meet the required build version of v0.15.2` (`REPORT.md`) |
| build time | cold: 1m09.6s for the first probe (incl. git clone + zig deps, `REPORT.md`); 29.7 s for `cargo test -p s1` with warm zig cache (task-1 report); incremental < 1 s. Workspace-wide cargo commands now need Zig; `-p s0` does not |
| artefacts | `libghostty-vt.a` 42,776,990 B; debug probe binary 26 MB; `target/` 648 MB after probe (`REPORT.md`) |
| snapshot path | no `snapshot.h` in `include/ghostty/vt/` (verified: allocator…wasm.h, formatter.h, render.h, terminal.h, grid_ref*.h); no whole-terminal serialize/restore C function. Snapshot = `fmt::Formatter` `Format::Vt` with every extra-state option on (palette, modes, scrolling region, tabstops, pwd, keyboard, cursor, style, hyperlink, protection, kitty keyboard, charsets), trim/unwrap off. Restore = `vt_write` into a fresh `Terminal` |
| snapshot fix-up | the Formatter emits CUP inside the screen section, then DECSTBM/DECSLRM and the tab-stop rebuild (`CSI 3g`, `CSI nG ESC H`) move the cursor; `snap::snapshot` appends a final absolute CUP (skipped under DECOM, where CUP is region-relative). `snap::snapshot_raw` = untouched Formatter output (`spikes/s1/src/snap.rs:26-45`) |
| harness | `spikes/s1` on branch `spike/s1`, commits `0813259`..`e23eba0`; run6 built at `e23eba0` (release). Tests: `cargo test -p s1` 30 passed, 2 ignored at `57151cb`'s working tree (task-5 report) |
| capture/compare | per cell: grapheme text, fg/bg (palette-resolved RGB), bold, italic, underline on/off, inverse, strike, faint, wide, OSC 8 URI (`GridRef::hyperlink_uri`). State: cursor x/y/visible/style/pending_wrap, alt screen, title, pwd, kitty flags, scrollback/total rows, 13 modes. History = Formatter Plain (unwrap off) first `scrollback_rows` lines, right-trimmed. Blank cells `""`/`" "` with no attributes normalised (`grid::normalize`) |
| not compared | underline style and colour, blink, invisible, overline, palette index vs equal RGB, hyperlink id, protection, semantic-prompt marks, DECSC saved cursor (as state), kitty push stack (run6 `summary.md` header) |
| strategies | **Naive**: S taken at the exact split byte. **Aligned**: split snapped back to `splitter::last_boundary_at_or_before` (a parser mirroring ghostty `parse_table.zig`: BEL ends only OSC; DCS/APC/PM/SOS end at ST; ESC aborts CSI/nF; CAN/SUB → ground); the tail from that boundary is fed to C. **AlignedUnwrap**: Aligned with Formatter `unwrap(true)` (`snap::snapshot_unwrap`) |
| sizes | recordings at their recorded 120x40; generated streams 80x24, except `wrap_pending` 120x24 (its generator assumes 120 columns); `max_scrollback 2000` (= bytes, rounded up to the PageList minimum; gitlog still kept 649 history rows) |
| split points | per stream ≤ 400: 0 and len, all ±2 windows around `?1049h/l` and `ESC]8;` anchors, 64 random boundaries, 64 random non-boundaries, then wide-char anchors in the remaining slots (seed 1; `experiment::split_plan`) |

## Corpus

7 recordings (tmux `pipe-pane` of the program's PTY on a private `-L s1rec` server, `TERM=xterm-256color`; method
in `spikes/s1/corpus/README.md`) + 13 generated streams (`spikes/s1/src/gen.rs`, deterministic, `s1 gen`). The
recorded `.bin` files in the repo (`spikes/s1/corpus/`) and in `/data/scratch/s1/corpus/` have identical sha256.
`claude` was not recorded (Claude Code's folder-trust dialog; 1.3 KB partial `claude.bin` = dialog only, skipped).

| stream | source | size | bytes | sha256 (12) | content |
|---|---|---|---|---|---|
| vim | recorded | 120x40 | 22852 | 4491826b2fa7 | `vim spikes/s0/src/send.rs`, `:set nu`, C-f ×3, search, `:q!` |
| btop | recorded | 120x40 | 74589 | 3fcdc319b371 | `btop` 5 s, `q` |
| less | recorded | 120x40 | 20309 | dbad0a394b9b | `less docs/spikes/S0-results.md`, `G g /Result n q` |
| cargo | recorded | 120x40 | 11762 | f1388620e89f | `cargo build -p s0 --color always`, full build (OSC 8 links) |
| gitlog | recorded | 120x40 | 51578 | fd6e87c0869b | `git log --color=always -p -5 \| cat` |
| nested | recorded | 120x40 | 4137 | 87dffd6724bb | inner tmux (`-L s1b`), 2 panes, ls/git |
| unicode | recorded | 120x40 | 13509 | 2ed3e3e6bf47 | `unicode_gen.py`: `\r` progress, CJK, ZWJ emoji, combining, 256/truecolor, curly underline, OSC 8, OSC 0, bracketed paste, 1049 |
| sgr_matrix | gen | 80x24 | 8356 | 4179f45f785c | every SGR attribute on/off, 16/256/truecolor, underline styles + colour; ends with a non-default pen |
| utf8_wide_combining | gen | 80x24 | 3564 | d9140d3c6b0d | CJK, Hangul, ZWJ emoji, flags, skin tones, VS16, combining; wide char at the last column |
| alt_screen_cycle | gen | 80x24 | 1712 | a4e36e524e0b | primary ↔ alt via 1049/1047/47, cycled 3×; ends on primary |
| scroll_region | gen | 80x24 | 1934 | 2c047219c88c | DECSTBM scrolling (LF, RI, SU, SD), DECLRMM + DECSLRM; region left set |
| scrollback_200 | gen | 80x24 | 14950 | b0b2422d04ab | 300 numbered lines (276 go to history at 24 rows) |
| modes_mix | gen | 80x24 | 1320 | 4e51c73e9da3 | DECCKM, DECAWM, DECTCEM, 2004, mouse 1000/1002/1006, 1004, 2026; mixed end state |
| kitty_kbd | gen | 80x24 | 1208 | dc3d40e8cbc1 | kitty keyboard push, set, modify, pop, query; ends pushed with flags 5 |
| osc_title_pwd_hyperlink | gen | 80x24 | 1902 | 6a0fd726bf4b | OSC 0/2 titles, OSC 7 pwd, OSC 8 (with/without id), OSC 52; BEL and ST; ends with a link open |
| tabs_and_cursor_save | gen | 80x24 | 1522 | 3a31e10bbe7f | HTS/TBC, HT, DECSC/DECRC (SGR + origin), cursor moves; ends with custom tabs, saved ≠ live cursor |
| erase_insert_delete | gen | 80x24 | 4567 | 471b982b96dd | full screen then ED/EL/ICH/DCH/IL/DL/ECH with a coloured background |
| wrap_pending | gen | 120x24 | 1420 | cfa063a0b826 | text ending exactly at col 120, more text, CRLF, CUP, SGR; ends in pending wrap |
| dcs_apc_noise | gen | 80x24 | 1935 | 48c4bea3fe46 | DCS (XTGETTCAP, DECRQSS, tmux passthrough), APC, PM, SOS ended by ST, incl. `APC … BEL … ST` |
| resize_marker | gen | 80x24 | 3505 | 13e41ff289f2 | numbered lines of varied length (some > 80 cols), for the resize-at-midpoint test |

sha256 from `sha256sum` of `/data/scratch/s1/corpus/{*.bin,gen/*.bin}`; recorded sizes also in `<name>.meta.json`;
generated-stream content from the doc comments in `spikes/s1/src/gen.rs`.

## Split matrix (run6)

Run `2026-10-07-s1-run6`: `target/release/s1 run --corpus-dir /data/scratch/s1/corpus --gen --out
/data/scratch/s1/results/2026-10-07-s1-run6 --cols 120 --rows 40 --scrollback 2000 --seed 1`, total wall 16.2 s.
Non-boundary split points: 762 inside CSI, 176 inside strings, 430 mid-UTF-8, 302 after ESC / in nF (1670 total).

### Totals

| strategy | cases | identical | identical % |
|---|---|---|---|
| Naive | 3344 | 1688 | 50.5 |
| Aligned | 3344 | 2628 | 78.6 |
| AlignedUnwrap | 3344 | 2618 | 78.3 |

### Per stream (identical %)

| stream | size | splits | D history rows | Naive | Aligned | AlignedUnwrap | top Aligned state fields |
|---|---|---|---|---|---|---|---|
| btop | 120x40 | 137 | 0 | 97.1 | **100.0** | 100.0 | – |
| cargo | 120x40 | 140 | 8 | 63.6 | 93.6 | 93.6 | scrollback_rows (4) |
| gitlog | 120x40 | 148 | 649 | 50.0 | 86.5 | 86.5 | scrollback_rows (20) |
| less | 120x40 | 137 | 0 | 94.2 | **100.0** | 100.0 | – |
| nested | 120x40 | 137 | 0 | 98.5 | **100.0** | 100.0 | – |
| unicode | 120x40 | 400 | 46 | 34.5 | 86.0 | 86.0 | title (56), scrollback_rows (31) |
| vim | 120x40 | 140 | 0 | 95.7 | **100.0** | 100.0 | – |
| sgr_matrix | 80x24 | 130 | 0 | 50.8 | **100.0** | 100.0 | – |
| utf8_wide_combining | 80x24 | 400 | 24 | 39.8 | 97.5 | 97.5 | scrollback_rows (5) |
| alt_screen_cycle | 80x24 | 140 | 0 | 16.4 | 27.1 | 27.1 | – |
| scroll_region | 80x24 | 130 | 0 | 60.8 | 99.2 | 99.2 | – |
| scrollback_200 | 80x24 | 130 | 277 | 48.5 | 97.7 | 97.7 | scrollback_rows (3) |
| modes_mix | 80x24 | 130 | 14 | 48.5 | 81.5 | 81.5 | scrollback_rows (24) |
| kitty_kbd | 80x24 | 130 | 58 | 41.5 | 53.1 | 53.1 | scrollback_rows (61) |
| osc_title_pwd_hyperlink | 80x24 | 335 | 0 | 1.2 | 2.1 | 2.1 | title (12) |
| tabs_and_cursor_save | 80x24 | 130 | 0 | 24.6 | 46.2 | 46.2 | pending_wrap (1) |
| erase_insert_delete | 80x24 | 130 | 0 | 80.8 | 80.8 | 80.8 | – |
| wrap_pending | 120x24 | 124 | 0 | 57.3 | 96.0 | **87.9** | pending_wrap (1) |
| dcs_apc_noise | 80x24 | 130 | 0 | 56.2 | **100.0** | 100.0 | – |
| resize_marker | 80x24 | 66 | 38 | 97.0 | 97.0 | 97.0 | scrollback_rows (2) |

Source: `/data/scratch/s1/results/2026-10-07-s1-run6/summary.md` (also mean cell mismatches, snapshot sizes, timings).

### Taxonomy (first match, priority order as listed)

| class | Naive | Aligned | AlignedUnwrap | example (run6 `taxonomy.md`) |
|---|---|---|---|---|
| (identical) | 1688 | 2628 | 2618 | |
| parser_state_split | 1315 | 0 | 0 | btop @ 1 [Naive]: cell (0,0): D="" C="" bg=(0,0,0) |
| alt_screen | 52 | 109 | 109 | unicode @ 8717: snapshot taken on alt screen, tail leaves it |
| trailing_row_drop | 7 | 7 | 6 | gitlog @ 51578: C = D shifted +1 row; scrollback_rows D=649 C=648 |
| scrollback | 22 | 122 | 123 | cargo @ 9613 [Aligned]: scrollback_rows D=8 C=7 |
| modes | 0 | 0 | 0 | |
| cursor | 2 | 2 | 2 | tabs_and_cursor_save @ 1522: pending_wrap D=true C=false |
| title_pwd | 21 | 37 | 37 | unicode @ 8548: title D="window title" C="" |
| kitty_kbd | 0 | 0 | 0 | |
| style_only | 2 | 3 | 3 | osc_title_pwd_hyperlink @ 98: cell (4,0) D="l" link=… C="l" (no link) |
| text | 235 | 436 | 446 | cargo @ 11731: cell (18,38) D=" " link=… C="" |
| other | 0 | 0 | 0 | |
| **total** | 3344 | 3344 | 3344 | |

All mismatching groups (a case counts in each group it has; `secondary_classes`):

| group | Naive | Aligned | AlignedUnwrap |
|---|---|---|---|
| hyperlink | 337 | 334 | 334 |
| text | 1051 | 601 | 611 |
| scrollback | 671 | 153 | 153 |
| style | 232 | 128 | 128 |
| title_pwd | 71 | 68 | 68 |
| cursor | 36 | 2 | 2 |
| alt_screen | 12 | 0 | 0 |
| modes | 9 | 0 | 0 |
| kitty_kbd | 1 | 0 | 0 |
| grid_size | 0 | 0 | 0 |

Aligned non-identical cases by stream × first-match class (counted from run6 `cases.jsonl`):

| stream | class (cases) | groups present | cause |
|---|---|---|---|
| osc_title_pwd_hyperlink | text 314, style_only 2, title_pwd 12 | hyperlink in all 328; title_pwd 12 | OSC 8 not emitted in VT; title not emitted |
| cargo | text 5, scrollback 4 | hyperlink 6; scrollback 4 | OSC 8 not emitted; trailing blank row |
| alt_screen_cycle | alt_screen 85, text 17 | text (+style 29) | primary lost behind alt; 17 text = invisible-only (blank gets fg, task-4 report) |
| unicode | alt_screen 24, title_pwd 25, scrollback 6, trailing_row_drop 1 | title_pwd 56, scrollback 31 | primary lost; title not emitted; trailing blank row |
| tabs_and_cursor_save | text 69, cursor 1 | style+text 69 | DECSC saved cursor/pen not emitted (tail's `ESC 8` restores defaults); pending wrap |
| erase_insert_delete | text 24, style_only 1 | style 20 | erased-cell background lost after tail ICH/DCH/ED |
| kitty_kbd | scrollback 60, trailing_row_drop 1 | scrollback 61 | trailing blank row |
| modes_mix | scrollback 23, trailing_row_drop 1 | scrollback 24 | trailing blank row |
| gitlog | scrollback 19, trailing_row_drop 1 | scrollback 20 | trailing blank row |
| utf8_wide_combining | scrollback 7, trailing_row_drop 1, text 2 | scrollback 8 | 4 trailing blank row; 3 history lines lose a wide char / regional indicator at row end + 2 wide `ｗ` at col 78 lost (pending wrap, task-4 attribution) |
| scrollback_200 | scrollback 2, trailing_row_drop 1 | scrollback 3 | trailing blank row |
| resize_marker | scrollback 1, trailing_row_drop 1 | scrollback 2 | trailing blank row |
| wrap_pending | text 4, cursor 1 | – | pending wrap not emitted: next char overwrites col 119 |
| scroll_region | text 1 | – | pending wrap (task-4 attribution) |

Trailing blank row evidence: of the 129 Aligned `scrollback` + `trailing_row_drop` cases, 126 had A's viewport ending
in ≥ 1 blank row with history present, and in all 129 C's history = D's minus one line (`scrollback_text_equal`
false). At a mid-stream split the viewport recovers but one history row is lost for good; at split = len the
viewport is shifted +1 row.

AlignedUnwrap vs Aligned: −10 identical, all in wrap_pending (`text` 4 → 14: joined rows re-wrap differently at the
same size); utf8_wide_combining moves 1 case from trailing_row_drop to scrollback.

## Formatter gaps (libghostty-vt 0.2.2 / ghostty a887df42, `src/terminal/formatter.zig`)

| gap | evidence | effect on restore |
|---|---|---|
| only the **active** screen is emitted | task-2-3 report; test `alt_screen_split_roundtrip` | snapshot on alt screen → primary comes back empty after `1049l` |
| **title** (OSC 0/2) never emitted, no option | task-2-3 report | title empty |
| OSC 7 pwd carries a stray NUL: `ESC]7;file://host/final/pwd\0ESC\` | task-2-3 report | pwd still restores (0 pwd-only mismatches) |
| kitty keyboard: current flags only (`CSI = n;1 u`), no push stack | task-2-3 report | a later pop by the app behaves differently; not exercised by a final-state compare (kitty_kbd class 0) |
| DECSC saved cursor and pen not emitted | task-4 report (tabs_and_cursor_save 69) | tail's `ESC 8` restores defaults |
| pending-wrap flag not emitted | task-2-3/task-4 reports | next printed char overwrites the last column instead of wrapping |
| cursor shape (DECSCUSR) not emitted | task-2-3 report | not a mismatch in run6 (`cursor_style` differs only in Naive) |
| trailing blank screen rows not emitted, even with `trim=false` | task-2-3/task-4 reports | one row fewer → one history row lost / viewport shift |
| blank cells come back as `" "` (styled space for gaps) | task-2-3 report | invisible; normalised away |
| erased-cell background lost after tail ICH/DCH/ED (some cases) | task-4 report (erase_insert_delete 24+1) | visible bg loss from ~col 39 |
| **OSC 8 hyperlinks never emitted in VT format** — `formatter.zig:1246-1250`: "We currently only emit hyperlinks for HTML" (verified in the checkout); only the cursor's open link is re-emitted (`formatter.zig:572`, task-4 fix round) | probe `a ESC]8;;u ST bc ESC]8;; ST d` → `abcd ESC[0m ESC[1;5H` | every in-content link lost. (The task-2-3 report's "hyperlinks are kept" was wrong.) |
| Formatter's own tab-stop / scroll-region output moves the cursor after its CUP | task-2-3 report | fixed by `snap::snapshot`'s final CUP; **not** fixed under DECOM |
| modes emitted before content as `CSI ? n h/l` | task-2-3 report | modes class 0; insert/origin/`?7l` before content replaying differently not tested |

## Timing (run6, Aligned)

Snapshot = Formatter + CUP fix-up on A; restore = `Terminal::new` + `vt_write(S)` (`snap::timed_restore`). Release
build, single thread, archbox.

| stream | size | D history rows | snap B p50 / max | snap µs p50 / p95 | restore µs p50 / p95 |
|---|---|---|---|---|---|
| gitlog | 120x40 | 649 | 20455 / 55284 | 390 / 982 | 332 / 735 |
| btop | 120x40 | 0 | 24063 / 32167 | 177 / 232 | 243 / 446 |
| scrollback_200 | 80x24 | 277 | 9573 / 17307 | 173 / 296 | 164 / 268 |
| vim | 120x40 | 0 | 4973 / 5883 | 68 / 87 | 112 / 151 |
| less | 120x40 | 0 | 3159 / 3629 | 69 / 94 | 75 / 109 |
| nested | 120x40 | 0 | 3238 / 3290 | 24 / 39 | 80 / 103 |
| sgr_matrix | 80x24 | 0 | 5014 / 10279 | 33 / 49 | 134 / 240 |
| unicode | 120x40 | 46 | 1039 / 11701 | 29 / 79 | 42 / 280 |
| cargo | 120x40 | 8 | 1601 / 2766 | 39 / 59 | 65 / 87 |
| utf8_wide_combining | 80x24 | 24 | 1881 / 3748 | 23 / 40 | 57 / 83 |
| kitty_kbd | 80x24 | 58 | 486 / 789 | 18 / 30 | 55 / 88 |
| osc_title_pwd_hyperlink | 80x24 | 0 | 250 / 392 | 9 / 11 | 33 / 43 |
| dcs_apc_noise | 80x24 | 0 | 197 / 303 | 10 / 11 | 31 / 43 |

Range over all 20 streams (Aligned): snapshot p50 9–390 µs, p95 ≤ 982 µs; restore p50 31–332 µs, p95 ≤ 735 µs;
snapshot size max 55284 B. Snapshot cost tracks snapshot bytes (screen content + history), not terminal size alone.
Not measured: 10k-line scrollback (largest history here: 649 rows).

## Portability (pyte) — run3

`s1 portability` (run3; matrix numbers in run3 are superseded by run6, portability/resize are not re-run):
snapshot = `snap::snapshot(D)` at len; compared line by line as right-trimmed Plain text; pyte 0.8.2 via
`spikes/s1/tools/pyte_restore.py` (tmls venv, `-I`). Source: `/data/scratch/s1/results/2026-10-07-s1-run3/portability/summary.md`.

| comparison | lines equal (of 592) | % | streams fully equal (of 20) | cursor match |
|---|---|---|---|---|
| ghostty restore(S) vs D (Formatter loss) | 352 | 59.5 | 12 | – |
| **pyte(S) vs ghostty restore(S)** (portability) | **577** | **97.5** | **17** | – |
| pyte(S) vs D | 351 | 59.3 | 11 | 20 |
| pyte(original stream) vs D | 426 | 72.0 | 11 | 15 |

| finding | streams | detail |
|---|---|---|
| all `0/N` rows vs D are the trailing-blank-row shift | cargo, gitlog, unicode, utf8_wide_combining, scrollback_200, modes_mix, kitty_kbd, resize_marker | libghostty's own restore = D shifted +1 row on the same 8 streams |
| pyte limit: SGR colon sub-params (`CSI 4:2 m`) drawn as text | sgr_matrix | 23/24 |
| pyte limit: kitty `CSI = 5;1 u` drawn as `5;1u` | kitty_kbd | 23/24 |
| pyte limit: no grapheme clustering (ZWJ, flags, skin tones, VS16) | utf8_wide_combining | 11/24 |
| snapshot more portable than the raw stream | less, vim, alt_screen_cycle, scroll_region, wrap_pending, dcs_apc_noise, kitty_kbd | pyte has no alt screen, draws DCS/APC payloads as text (210 ignored in dcs_apc_noise), crashes on private CSI (`CSI ? 6 n`, wrapped by the helper) |
| VT subset used by the snapshot | all | SGR, CUP, `CSI n G`, HTS, TBC, kitty `CSI = n;m u`, SGR `4:n` (+ OSC 4 palette, OSC 7, DEC modes) |

## Resize at split — run3

`s1 resize`: split = splitter boundary nearest len/2, intermediate size 100x30, `Terminal::resize(cols, rows, 8, 16)`.
Source: `/data/scratch/s1/results/2026-10-07-s1-run3/resize/summary.md`.

| variant | vim | less | scrollback_200 | wrap_pending |
|---|---|---|---|---|
| baseline (D resized at the split and back) | yes | yes | yes | yes |
| `restore_small` (restore S into 100x30) | yes | yes | **no** (history 271/277) | **no** (670 cells, cursor x 100 vs 0) |
| `restore_orig_resize_before_tail` (restore at own size, resize, tail) | yes | yes | yes | **no** (717 cells: soft wraps hard-broken) |
| `restore_orig_resize_after_tail` | yes | yes | yes | yes |
| `restore_small_unwrap` | yes | yes | **no** (271/277) | **no** (493 cells) |
| `restore_orig_resize_before_tail_unwrap` | yes | yes | yes | **yes** |

| finding | detail |
|---|---|
| restoring into a different size is not faithful | absolute CUP/regions for the original size: scrollback_200's final CUP to row 24 lands 6 rows above the bottom of 30 → the tail overwrites 6 lines |
| restore at the snapshot's own size, then resize | identical on all 4 streams with unwrap; vim/less/scrollback_200 also without |
| default snapshot (`unwrap(false)`) hard-breaks soft wraps | every row ends in CRLF → a later resize cannot reflow joined lines |
| `snapshot_unwrap(true)` fixes reflow-after-restore | but at same-size restore costs 10 wrap_pending cases (96.0% → 87.9%, run6) → use unwrap only when a resize is expected, or keep both |
| D itself is not resize-round-trip lossless mid-stream | wrap_pending: content written while narrow stays narrow (libghostty reflow, not a snapshot issue) |
| vim/less unaffected by alt-screen loss | split on alt screen, empty primary, tail repaints |

## Gate verdict vs PLAN §6 S1

PLAN §6 S1 pass gate: "Identical results; differences explained." Results are **not** identical (Aligned 78.6%);
every Aligned difference has a named cause. Categories: **(a)** fixed by the worker protocol (snapshot at a VT
sequence boundary, carry the tail); **(b)** recoverable by the worker tracking state outside the Formatter and
emitting it with the snapshot; **(c)** Formatter limitation needing an upstream patch or a newer ghostty with
`snapshot.h` + own bindings. (b) and (c) fixes are **not built or measured** in S1.

| difference class | Naive | Aligned (first-match) | groups (Aligned) | category | what fixes it |
|---|---|---|---|---|---|
| parser state at the split (inside CSI/string/UTF-8/ESC) | 1315 | 0 | – | **(a)** | measured: Aligned removes all 1315 |
| hyperlinks (OSC 8) in content not emitted in VT | – | 321 (text 319 + style_only 2) | 334 | **(c)**, (b) workaround | upstream VT emission; or worker re-emits OSC 8 per cell run from `GridRef::hyperlink_uri` (the harness reads URIs this way) — untested |
| primary screen lost when snapshot taken on alt screen | 52 | 109 | – | **(b)** | worker takes a primary-screen snapshot at alt-screen entry (`?1049h`/`1047h`/`47h` boundary) and replays it before the alt content |
| trailing blank screen row(s) not emitted → history row lost / viewport shift | 29 | 126 (scrollback 119 + trailing_row_drop 7) | 153 scrollback | **(c)** | upstream; a worker-side pad of the dropped blank rows is untested |
| DECSC saved cursor + pen not emitted | – | 69 | – | **(b)** | worker tracks DECSC/DECRC (`ESC 7/8`, `CSI s/u`) and re-emits position + pen + `ESC 7` |
| title (OSC 0/2) not emitted | 21 | 37 | 68 title_pwd | **(b)** | worker keeps title (`Terminal::title`/`on_title_changed`) and emits OSC 2 |
| erased-cell background lost after tail ICH/DCH/ED | – | 25 (text 24 + style_only 1) | – | **(c)** | upstream |
| pending wrap not emitted (incl. wide char / regional indicator at row end) | – | 12 (cursor 2, text 7, scrollback 3) | 2 cursor | **(b)** | worker reads `is_cursor_pending_wrap()` and re-creates it (re-print the last cell at the last column) |
| invisible-only (blank cell gains fg from styled-space output) | – | 17 | – | explained, invisible | none needed (counted as mismatch under the agreed rule) |
| kitty keyboard push stack (flags only emitted) | – | 0 (not exercised) | 0 | **(b)** | worker tracks push/pop (`CSI > u` / `CSI < u`) and replays the stack |
| pwd stray NUL in OSC 7 | – | 0 | – | cosmetic | pwd restores; strip NUL if re-emitted |
| restore into a different size (resize run) | – | 2/4 streams fail | – | **(a)** protocol rule | restore at the snapshot's size, then resize (identical on 4/4) |
| soft wraps hard-broken (resize after restore) | – | 1/4 streams | – | **(a)** option | `snapshot_unwrap` when a resize follows; costs 10 same-size cases |
| CUP under DECOM | – | 0 (no stream ends a split with DECOM set) | – | open | cursor fix-up skipped under DECOM |

Sum of the Aligned first-match column: 321 + 109 + 126 + 69 + 37 + 25 + 12 + 17 = 716 = 3344 − 2628.

**Gate S1 partially passed: differences are explained; libghostty-vt is viable as the authoritative emulator; the
Formatter alone is NOT a sufficient snapshot — worker-side state carry + boundary alignment required; native
snapshot path = follow-up.** Windows MSVC build (part of the S1 question) is pending.

## Consequences for PLAN §3 / §5.2

| item | consequence |
|---|---|
| §3 Emulator core | libghostty-vt 0.2.2 stays; build needs Zig 0.15.2 (not 0.16); `max_scrollback` must be sized in bytes |
| §5.2 snapshot point | the worker snapshots only at a VT sequence boundary (its own splitter mirroring ghostty `parse_table.zig`) and sends the bytes after that boundary as the tail |
| §5.2 snapshot size | restore at the session's size at offset S; resizes come after in the stream (resize epoch) |
| §5.2 state carry | worker emits, beside the Formatter output: title, pwd (without NUL), kitty keyboard stack, pending-wrap, DECSC saved cursor, hyperlinks (until VT emission exists), and a primary-screen snapshot taken on alt-screen entry |
| §5.2 "native snapshot" | not available at a887df42; needs a newer ghostty with `snapshot.h` + own bindings (S1 follow-up). Until then the same-version desktop also uses the VT reconstruction |
| §5.2 portable format | the VT snapshot is the portable format: pyte renders it as well as libghostty does (97.5% rows), better than the raw stream |
| §10 risk | "no snapshot compatibility guarantee" is moot for now (no native snapshot); the risk moves to Formatter gaps (c) |

## Not measured / next steps

Not measured:
- **Windows MSVC build** of libghostty-vt (part of the S1 question in PLAN §6).
- Graphics / kitty images (`kitty_graphics.h` exists; not exercised).
- **Recorded Claude TUI**: blocked at the folder-trust dialog. To record: `cd ~/Others/tmls-rs && claude`, accept
  "Yes, I trust this folder", exit, then `spikes/s1/corpus/record.sh claude` (`spikes/s1/corpus/README.md`).
- Long-scrollback performance (10k lines; largest here 649 rows) and snapshot cost under a byte-sized
  `max_scrollback`; whether any stream was pruned by the 2000-byte limit (C and D share it).
- Any of the (b) worker-side fixes or a worker-side trailing-row pad — designed above, not built.
- CUP fix-up under DECOM; modes emitted before content changing how content replays (insert/origin/`?7l`).
- Cell attributes outside the compare set (underline style/colour, blink, invisible, overline, palette index,
  hyperlink id, protection, semantic prompts).
- `snapshot_unwrap` beyond the 4 resize streams + the run6 matrix.

Next steps:
1. S2 GUI interaction slice on the PC.
2. S1 follow-up: native snapshot — newer ghostty with `snapshot.h` + own bindings; or upstream VT hyperlink /
   trailing-row / erased-bg fixes; re-run `s1 run` against it.
3. Windows MSVC build of the pinned crate.
4. Record the Claude TUI corpus stream once trust is accepted; re-run run6's command.

## Run inventory

All under `/data/scratch/s1/results/<id>/` (`summary.md`, `taxonomy.md`, `cases.jsonl`, `streams.json`).

| run id | what | used here |
|---|---|---|
| `2026-10-07-s1-run1`, `-run2` | Task 4 runs while diagnostics were added; same identical counts as run3 | — |
| `2026-10-07-s1-run3` | Task 4 matrix (Naive 55.6% / Aligned 88.4%, before hyperlink compare — **superseded**); Task 5 `portability/` and `resize/` | portability, resize |
| `2026-10-07-s1-run4`, `-run4-probe` | fix-round 1 intermediate (before link output in examples) — superseded | — |
| `2026-10-07-s1-run5` | fix round 1 (hyperlink compare, anchor-first plan), Naive/Aligned only, wall 10.9 s | — (same Naive/Aligned as run6) |
| **`2026-10-07-s1-run6`** | final matrix Naive / Aligned / AlignedUnwrap at `e23eba0`, wall 16.2 s | split matrix, taxonomy, timing |
| `trial-btop` | Task 4 trial | — |
| `/data/scratch/s1-probe/` | first build probe (`REPORT.md`, `build.log` 0.16 failure, `build2.log`, `run.log`) | setup |

Evidence: `.superpowers/sdd/2026-10-07-s1-emulator/task-{1,2-3,4,5}-report.md`, `progress.md`; harness docs
`spikes/s1/README.md`, `spikes/s1/corpus/README.md`. Commit `57151cb` does not build on its own (it carries part of
Task 4's fix-round hunks); `ddf25e0` and later build.
