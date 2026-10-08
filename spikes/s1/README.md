# Spike S1: emulator snapshot/restore fidelity

Question: can `libghostty-vt` 0.2.2 be tmls-rs's authoritative emulator and snapshot source?

## Building

```sh
source spikes/s1/build-env.sh     # Zig 0.15.2 on PATH, GHOSTTY_SOURCE_DIR, TMPDIR=/data/scratch/s1
cargo test -p s1
cargo run -p s1 -- capture --file <bytes> [--cols 80 --rows 24]   # prints (Grid, TermState) as JSON
```

- `libghostty-vt = "=0.2.2"` is pinned exactly. Its `-sys` crate builds ghostty commit
  `a887df42c56f6de86c0fe6da9c4eeca37931e083` with `zig build`.
- **Zig 0.15.2 exactly** is required. Zig 0.16.x fails (`Your Zig version v0.16.0 does not meet the
  required build version of v0.15.2`), despite the upstream README saying 0.16.
- `GHOSTTY_SOURCE_DIR` points at an existing checkout of that commit, so `build.rs` skips its git
  clone. Without it the build clones ghostty over the network into `OUT_DIR`.
- Cold build of the crate on this 4-core box: about 30 s (the zig package cache was already warm
  from the probe). `cargo test -p s0` needs no Zig; only `-p s1` / workspace-wide builds do.
- All libghostty-vt handles are `!Send`/`!Sync`; the spike is single-threaded throughout.

## No native snapshot API

The pinned ghostty has **no `snapshot.h`** (and no whole-terminal serialize/restore C function).
`include/ghostty/vt/` has `formatter.h`, `render.h`, `terminal.h`, `grid_ref*.h`, etc. The only
candidate snapshot path is the formatter (`Format::Vt`), which emits a replayable VT byte stream;
restore means `vt_write`-ing that stream into a fresh `Terminal`. S1 measures how faithful that is.

## Capture (`src/grid.rs`)

`capture(&mut Terminal) -> (Grid, TermState)` reads the viewport through
`RenderState::update` → `RowIterator` → `CellIterator`, plus `Terminal` getters.

- `text`: `graphemes_utf8` (empty string for blank cells and wide-char spacer tails).
- `fg`/`bg`: `CellIterator::fg_color/bg_color`, **palette-resolved RGB**; `None` = default colour.
  Palette index vs. an equal RGB literal are therefore indistinguishable. Inverse is not applied.
- `bold/italic/inverse/faint`: the `Style` booleans; `strike` = `strikethrough`;
  `underline` = `style.underline != Underline::None` (underline kind is collapsed).
- `wide`: `raw_cell().wide() == CellWide::Wide`. The spacer tail that follows is a cell with
  empty text and `wide == false` (no separate spacer field in `Cell`).
- `cursor_style`: `"<CursorVisualStyle>;blink=<bool>;pen=<Style>"`. Shape and blink come from the
  render snapshot (DECSCUSR); `Terminal::cursor_style()` in 0.2.2 is the SGR *pen*, not the shape,
  so both are recorded.
- `alt_screen`: `active_screen() == Screen::Alternate`. `kitty_flags`: `kitty_keyboard_flags().bits()`.

`diff` compares cells over the overlapping region (`(x, y, a, b)`), reports a size mismatch as
`grid_size`, and compares every `TermState` field plus each mode by name (`mode.<name>`).

## Modes captured

All 13 modes from the plan exist as `Mode` consts in 0.2.2; none are skipped.

| name | DEC | `Mode` const |
|---|---|---|
| cursor_keys | 1 | `DECCKM` |
| origin | 6 | `ORIGIN` |
| autowrap | 7 | `WRAPAROUND` |
| cursor_visible | 25 | `CURSOR_VISIBLE` |
| alt_screen | 1049 | `ALT_SCREEN_SAVE` |
| bracketed_paste | 2004 | `BRACKETED_PASTE` |
| mouse_normal | 1000 | `NORMAL_MOUSE` |
| mouse_button | 1002 | `BUTTON_MOUSE` |
| mouse_any | 1003 | `ANY_MOUSE` |
| mouse_sgr | 1006 | `SGR_MOUSE` |
| focus_events | 1004 | `FOCUS_EVENT` |
| synchronized_output | 2026 | `SYNC_OUTPUT` |
| grapheme_cluster | 2027 | `GRAPHEME_CLUSTER` |

Not captured (exist in the crate, outside the fixed list): 47/1047 alt-screen variants, 1005/1015/1016
mouse encodings, X10 mouse (9), insert (ANSI 4), cursor blinking (12, captured via `cursor_style`
instead), keypad, left/right margin, etc.

## Experiment (`src/experiment.rs`, `src/report.rs`, `s1 run`)

```sh
cargo run -p s1 --release -- run --corpus-dir /data/scratch/s1/corpus --gen \
    --out /data/scratch/s1/results/<id> --cols 120 --rows 40 --scrollback 2000 --seed 1
```

- Per stream: D = fresh terminal fed the whole stream. For each split point `p` and strategy,
  Naive snapshots `bytes[..p]`, Aligned snapshots `bytes[..q]` with
  `q = last_boundary_at_or_before(p)`. Each restores into C and feeds C the rest of the stream.
  The run compares C with D.
- `split_points`: 0 and len, 64 random boundaries, 64 random non-boundaries, and ±2 around every
  `ESC[?1049h`, `ESC[?1049l`, `ESC]8;` and lead byte of a (CJK/emoji-range) wide char. The list is
  capped at 400 (`--stream-cap name=N` sets a cap per stream).
- Sizes: recorded streams use their `.meta.json` cols/rows (120x40). Generated streams use 80x24,
  except `wrap_pending`, which is 120x24 because it assumes 120 columns. `claude` is skipped
  (`--skip`) because its recording is invalid.
- Grids are compared after `grid::normalize`, which turns a plain blank `""` or `" "` into one
  canonical blank. `TermState.pending_wrap` is compared.
- **Scrollback text** is compared through Formatter `Format::Plain` (`unwrap(false)`). The output
  covers the whole PageList, history first, with trailing blank rows always trimmed. The first
  `scrollback_rows` lines (right-trimmed) are the history, compared as `scrollback_text_equal`.
- **`max_scrollback` is in BYTES** in ghostty (`Screen.zig`: "amount of scrollback to keep in
  bytes"), even though the Rust doc says "lines". PageList rounds it up to its minimum size. That
  minimum is still more than every corpus stream needs (gitlog keeps 649 history rows at 2000).
- Taxonomy: each non-identical case gets the first class that matches, in this priority order:
  `parser_state_split > alt_screen > trailing_row_drop > scrollback > modes > cursor > title_pwd > kitty_kbd > style_only > text > other`.
- Outputs: `cases.jsonl` (one record per case, with diagnostics `snap_faithful`,
  `a_trailing_blank_rows`, `cells_blank_equivalent` and `first_visible_cell_mismatch`),
  `streams.json`, `summary.md` and `taxonomy.md`.
- The splitter mirrors ghostty's `parse_table.zig`: BEL ends only OSC, DCS/APC/PM/SOS end only at
  ST, ESC aborts CSI/nF, and CAN/SUB return to ground.

## Portability probe (`src/portability.rs`, `tools/pyte_restore.py`, `s1 portability`)

```sh
cargo run -p s1 --release -- portability --corpus-dir /data/scratch/s1/corpus --gen --out /data/scratch/s1/results/<id>
S1_PYTE=1 cargo test -p s1 pyte_renders_snapshot -- --nocapture   # the only test that runs Python
```

- Per stream, D = libghostty fed the whole stream at the `run` sizes. S = `snap::snapshot(D)` (the
  Aligned snapshot at `len`), written to `<out>/portability/<stream>.vt`. The original bytes go to
  `<stream>.orig.bin`.
- `tools/pyte_restore.py <vt> <cols> <rows>` runs under `~/Others/tmls/.venv/bin/python` (pyte
  0.8.2, read-only). It prints `screen.display` on stdout and a JSON line on stderr: the cursor, plus
  a count of the events pyte sent to `Screen.debug` (sequences it does not implement).
- pyte 0.8.2 crashes (`TypeError: ... unexpected keyword argument 'private'`) on a private CSI whose
  handler does not take `private`, for example `CSI ? 6 n` or `CSI ? 4 m`. The helper wraps those
  handlers and counts such a sequence as ignored.
- Comparisons are line by line, with every line right-trimmed:
  - pyte(S) vs libghostty restore(S). This is the portability measure.
  - pyte(S) vs D.
  - pyte(original) vs D.
  - libghostty restore(S) vs D, which is the Formatter's own loss. A one-row shift is detected
    and reported.
- The pyte cursor is compared with D's `cursor_x/y`. With pending wrap, pyte reports x = cols where
  ghostty reports cols-1.
- Outputs: `portability/summary.md` and `portability/portability.jsonl`.

## Resize at split (`src/resize.rs`, `s1 resize`)

```sh
cargo run -p s1 --release -- resize --corpus-dir /data/scratch/s1/corpus --gen --out /data/scratch/s1/results/<id>
```

- Streams: `vim`, `less`, `scrollback_200` and `wrap_pending`. The split `q` is the splitter
  boundary nearest `len/2`. The intermediate size is 100x30.
- `Terminal::resize(cols, rows, cell_w_px, cell_h_px)` is called with an 8x16 px cell.
- Variants are listed in the `resize.rs` module docs:
  - `restore_small`: restore into 100x30.
  - `restore_orig_resize_before_tail` / `_after_tail`: restore at the original size and resize
    later.
  - `*_unwrap`: the same, but the snapshot is taken with the Formatter's `unwrap` option
    (`snap::snapshot_unwrap`).
- Outputs: `resize/summary.md` and `resize/resize.jsonl`.
