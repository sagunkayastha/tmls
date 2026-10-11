//! Pixel → cell hit-testing and selection → copied text (wide characters, trailing spaces).

mod common;

use common::{cell, frame, lead, spacer, text_frame};
use s2_core::{Cell, Frame};
use s2_wgpu::gfx::atlas::{Atlas, FontSpec};
use s2_wgpu::gfx::grid::{CellSpan, Scene, Theme};
use s2_wgpu::select::Selection;

fn span(row: u16, col_start: u16, col_end: u16) -> CellSpan {
    CellSpan {
        row,
        col_start,
        col_end,
    }
}

/// Row 0: `ab你好cd` + 4 blanks (cols 0..12; 你 = 2,3; 好 = 4,5). Row 1: blank.
/// Row 2: `  xy` + blanks. Row 3: `end` then a run of trailing blanks.
fn sample() -> Frame {
    let blank = Cell::default;
    let mut row0 = vec![
        cell("a"),
        cell("b"),
        lead("你"),
        spacer(),
        lead("好"),
        spacer(),
        cell("c"),
        cell("d"),
    ];
    row0.resize_with(12, blank);
    let rest = text_frame(&["", "  xy", "end"], 12);
    let mut rows = vec![row0];
    rows.extend(rest.cells.chunks(12).map(<[Cell]>::to_vec));
    frame(rows)
}

fn select(from: (u16, u16), to: (u16, u16)) -> Selection {
    let mut s = Selection::start(from);
    s.drag_to(to);
    s
}

#[test]
fn pixels_map_to_cells() {
    let mut atlas = Atlas::new(FontSpec::default(), 1.0);
    let m = atlas.metrics();
    let (w, h) = (m.width, m.height);
    let mut scene = Scene::new();
    assert_eq!(
        scene.cell_at(5.0, 5.0),
        None,
        "no grid before the first frame"
    );
    assert_eq!(scene.nearest_cell(5.0, 5.0), None);
    scene.set_origin(40.0, 10.0);
    scene.update_cells(&sample(), &mut atlas, &Theme::default());

    // Inside: every pixel of a cell maps to it, edges belong to the cell they start.
    assert_eq!(scene.cell_at(40.0, 10.0), Some((0, 0)));
    assert_eq!(scene.cell_at(40.0 + w - 0.5, 10.0 + h - 0.5), Some((0, 0)));
    assert_eq!(scene.cell_at(40.0 + w, 10.0 + h), Some((1, 1)));
    assert_eq!(
        scene.cell_at(40.0 + 11.5 * w, 10.0 + 3.5 * h),
        Some((11, 3))
    );
    // Outside: nothing for a click …
    for (x, y) in [
        (39.0, 10.0),
        (40.0, 9.0),
        (40.0 + 12.0 * w, 10.0),
        (40.0, 10.0 + 4.0 * h),
    ] {
        assert_eq!(scene.cell_at(x, y), None, "({x}, {y})");
    }
    // … and the nearest cell for a drag that left the grid (or the window).
    assert_eq!(
        scene.nearest_cell(40.0 + 2.5 * w, 10.0 + 1.5 * h),
        Some((2, 1))
    );
    assert_eq!(scene.nearest_cell(-500.0, -500.0), Some((0, 0)));
    assert_eq!(scene.nearest_cell(0.0, 10.0 + 2.5 * h), Some((0, 2)));
    assert_eq!(scene.nearest_cell(1e6, 10.0 + 1.5 * h), Some((11, 1)));
    assert_eq!(scene.nearest_cell(40.0 + 3.5 * w, 1e6), Some((3, 3)));
    assert_eq!(scene.nearest_cell(f32::NAN, f32::NAN), Some((0, 0)));
}

#[test]
fn a_click_selects_nothing() {
    let f = sample();
    let mut s = Selection::start((1, 0));
    assert!(s.is_empty());
    assert_eq!(s.spans(&f), []);
    assert_eq!(s.text(&f), "");
    assert_eq!(s.rows(), None);
    // The pointer moved inside the same cell: still a click.
    assert!(!s.drag_to((1, 0)));
    assert!(s.is_empty());
    // Out and back: that is a one-cell selection.
    assert!(s.drag_to((2, 0)));
    assert!(s.drag_to((1, 0)));
    assert!(!s.is_empty());
    assert_eq!(s.spans(&f), [span(0, 1, 2)]);
    assert_eq!(s.text(&f), "b");
}

#[test]
fn selection_within_a_row_covers_whole_wide_characters() {
    let f = sample();
    // b .. the lead of 好: its spacer column is included.
    let s = select((1, 0), (4, 0));
    assert_eq!(s.spans(&f), [span(0, 1, 6)]);
    assert_eq!(s.text(&f), "b你好");
    assert_eq!(s.rows(), Some((0, 0)));
    // Starting on a spacer starts at its lead; ending on a spacer ends after it.
    let s = select((3, 0), (6, 0));
    assert_eq!(s.spans(&f), [span(0, 2, 7)]);
    assert_eq!(s.text(&f), "你好c");
    let s = select((0, 0), (3, 0));
    assert_eq!(s.spans(&f), [span(0, 0, 4)]);
    assert_eq!(s.text(&f), "ab你");
    // Dragging right to left gives the same selection.
    assert_eq!(select((4, 0), (1, 0)).spans(&f), [span(0, 1, 6)]);
    assert_eq!(select((4, 0), (1, 0)).text(&f), "b你好");
}

#[test]
fn selection_over_rows_trims_trailing_spaces() {
    let f = sample();
    // From 好 on row 0 to the `y` on row 2.
    let s = select((4, 0), (3, 2));
    assert_eq!(s.spans(&f), [span(0, 4, 12), span(1, 0, 12), span(2, 0, 4)]);
    // Row 0 loses its 4 trailing blanks, the blank row is an empty line, leading blanks stay.
    assert_eq!(s.text(&f), "好cd\n\n  xy");
    assert_eq!(s.rows(), Some((0, 2)));
    // Bottom-up drag: same thing.
    assert_eq!(select((3, 2), (4, 0)).text(&f), "好cd\n\n  xy");

    // A line with a trailing-space run, selected past its end.
    let s = select((0, 3), (11, 3));
    assert_eq!(s.spans(&f), [span(3, 0, 12)]);
    assert_eq!(s.text(&f), "end");
    // Only blanks selected: nothing to copy.
    assert_eq!(select((8, 0), (11, 0)).text(&f), "");
    // The whole grid.
    assert_eq!(select((0, 0), (11, 3)).text(&f), "ab你好cd\n\n  xy\nend");
}

#[test]
fn selection_outside_a_smaller_frame_is_clamped() {
    // The grid shrank after the selection was made.
    let s = select((2, 0), (40, 9));
    let small = text_frame(&["abcd", "efgh"], 4);
    assert_eq!(s.spans(&small), [span(0, 2, 4), span(1, 0, 4)]);
    assert_eq!(s.text(&small), "cd\nefgh");
    let s = select((10, 5), (40, 9));
    assert_eq!(s.spans(&small), []);
    assert_eq!(s.text(&small), "");
}
