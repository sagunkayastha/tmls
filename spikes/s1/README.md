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
