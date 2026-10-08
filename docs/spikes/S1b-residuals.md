# S1b residual analysis (2026-10-08)

Scope: Linux archbox, `libghostty-vt = 0.2.2`, Ghostty source `a887df42c56f6de86c0fe6da9c4eeca37931e083`. Comparison is the unchanged S1 normalised viewport, selected terminal state, and right-trimmed history text. `max_scrollback=2000` is **bytes**. Source: `/data/scratch/s1/results/2026-10-08-s1b-run5/` (20 streams, 3344 splits per strategy). No upstream issue has been filed.

## Measured outcome

| Strategy | Identical / 3344 |
|---|---:|
| Naive | 1688 |
| Aligned | 2628 |
| AlignedUnwrap | 2618 |
| AlignedCarry | 3271 |

The carry work fixed active OSC 8 continuation, primary-prefix mode leakage, trailing blank rows, and pending wrap after a wide glyph. Run5 has 73 non-identical AlignedCarry cases: 31 `alt_screen`, 41 `text`, 1 `style_only`. The first-match `alt_screen` class means the snapshot was on alt and the tail left it; in these 31 cases the actual mismatch is a styled blank in primary content. No residual `scrollback`, `title_pwd`, `cursor`, or `kitty_kbd` class remains.

| Class | Count | Streams | Observed first mismatch | Decision |
|---|---:|---|---|---|
| `alt_screen` | 31 | `alt_screen_cycle` | cell (29,8): D `""`, C `" "` with foreground `(204,102,102)` | Formatter reconstruction of primary styled blank; issue draft below |
| `text` | 41 | `erase_insert_delete` 24; `alt_screen_cycle` 17 | erased cell: D `""` with background, C `" "` with same background; alt case has D `""`, C styled `" "` | Formatter's text representation of styled empty cells; issue draft below |
| `style_only` | 1 | `erase_insert_delete` | D empty cell background `(181,189,104)`, C empty cell with no background | Formatter omits erase background; issue draft below |

`grid::normalize` intentionally does not erase style on a styled blank or collapse an erased empty cell and a printed space. Changing the S1 comparison here would hide state that later ICH/DCH/ED operations can expose, so it remains unchanged. Some empty-vs-space cells may render alike, but the gate is **identical under the S1 comparison**.

## Reproduction tests

`spikes/s1/src/experiment.rs` has ignored `residual_alt_screen_repro`, `residual_text_repro`, and `residual_style_only_repro` tests. Each stream is under 200 bytes and checks its measured class. Run them with `cargo test -p s1 residual_ -- --ignored --nocapture` after sourcing `spikes/s1/build-env.sh`.

## Upstream issue drafts — user files, no issue filed here

### Draft A: VT Formatter loses erase-cell background and empty-cell identity

- **Title:** VT Formatter reconstruction drops backgrounds and changes erased empty cells into printed spaces
- **Version:** `libghostty-vt 0.2.2`, Ghostty `a887df42c56f6de86c0fe6da9c4eeca37931e083`
- **Repro:** 8×3 terminal; feed `\x1b[1;1Habcdefgh\x1b[2;1Hijklmnop\x1b[48;2;181;189;104m\x1b[2;4H\x1b[2J\x1b[0m`, format as `Format::Vt` with style/modes enabled, restore into fresh 8×3, then feed `X`. A second variant replaces `\x1b[2J` with `\x1b[0K`.
- **Expected:** Cells after restore have the same grapheme text and background as before formatting, including erased cells. Further ICH/DCH/ED operations produce the same grid.
- **Actual:** The 2J variant loses background at (0,0), D empty with RGB `(181,189,104)` vs C empty without bg. The 0K variant yields D empty at (4,1) vs C a printed space; both have the same bg. The two ignored Rust tests assert these differences.
- **Impact:** 25 run5 cases directly from `erase_insert_delete`; the same text-vs-space representation appears in 47 alt-cycle cases. The snapshot cannot encode an erased empty cell with VT printable text while preserving its exact cell identity under this comparison.

### Draft B: VT Formatter primary-screen snapshot changes a styled blank after 47-mode cycle

- **Title:** VT Formatter replay changes primary-screen styled blank after alternate-screen cycle
- **Version:** `libghostty-vt 0.2.2`, Ghostty `a887df42c56f6de86c0fe6da9c4eeca37931e083`
- **Repro:** 80×24 terminal; feed `\n\n\x1b[?1049h\x1b[?1049l\n\n\n\n\n\nprimary cycle 1 line 5 \x1b[35mcolour\x1b[?1047hs\x1b[?1047lb\x1b[?47h`, snapshot at that point, then feed `\x1b[?47lback` to both terminals. The whole stream is 93 bytes. See ignored `residual_alt_screen_repro`.
- **Expected:** Primary-screen cell (29,8) remains empty and the continued grid matches uninterrupted parsing.
- **Actual:** Restored cell (29,8) is a printed space with foreground `(204,102,102)`; uninterrupted cell is empty. Run5 reports 31 `alt_screen` first-match cases and 17 `text` cases with the same first mismatch. This is a proposed Formatter reconstruction cause; it needs upstream confirmation that the primary Formatter VT, rather than the 47 mode itself, creates the space.
- **Impact:** The five carried state paths pass their focused tests, but this class keeps the S1b all-stream gate open.
