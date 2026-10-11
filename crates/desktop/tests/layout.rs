//! The window's layout and what a click hits. No atlas: the cell box is given.

use desktop::draw::{Hit, Layout, layout, list_top};
use desktop::theme::{SIDEBAR_COLS, TAB_MAX_COLS, Theme};
use s2_wgpu::gfx::atlas::CellMetrics;

/// A 10 x 20 pixel cell.
fn cell() -> CellMetrics {
    CellMetrics {
        width: 10.0,
        height: 20.0,
        baseline: 15.0,
        underline_y: 17.0,
        strike_y: 10.0,
        stroke: 1.0,
    }
}

fn at(width: u32, height: u32) -> Layout {
    layout((width, height), 1.0, &cell(), &Theme::default())
}

#[test]
fn sidebar_tab_bar_terminal_and_status_line_fill_the_window() {
    let l = at(1281, 726);
    assert_eq!(l.sidebar.w, SIDEBAR_COLS as f32 * 10.0);
    assert_eq!((l.separator.x, l.separator.w), (280.0, 1.0));
    // One row each for the tab bar and the status line: a cell and the inset above and below.
    assert_eq!((l.tab_bar.x, l.tab_bar.y, l.tab_bar.h), (281.0, 0.0, 26.0));
    assert_eq!((l.terminal.x, l.terminal.y), (281.0, 26.0));
    assert_eq!((l.terminal.w, l.terminal.h), (1000.0, 674.0));
    assert_eq!((l.status.y, l.status.bottom()), (700.0, 726.0));
    assert_eq!(
        l.grid,
        (100, 33),
        "the cells that fit the terminal's area, not the window"
    );
}

#[test]
fn a_tiny_window_still_lays_out() {
    for (width, height) in [(1, 1), (40, 30), (300, 10)] {
        let l = at(width, height);
        assert!(l.sidebar.w <= width as f32 / 2.0);
        assert!(l.terminal.w >= 0.0 && l.terminal.h >= 0.0);
        assert!(l.grid.0 >= 1 && l.grid.1 >= 1);
        assert_ne!(l.hit(0.0, 0.0, 0), Hit::Outside);
        assert_eq!(l.hit(width as f32, 0.0, 3), Hit::Outside);
    }
}

#[test]
fn clicks_hit_rows_tabs_and_their_close_marks() {
    let l = at(1281, 726);
    assert_eq!(l.hit(-1.0, 5.0, 2), Hit::Outside);
    assert_eq!(l.hit(5.0, 800.0, 2), Hit::Outside);
    // Rows are a cell and an inset high, below one inset of padding.
    assert_eq!(l.hit(50.0, 4.0, 2), Hit::ListRow(0));
    assert_eq!(l.hit(50.0, 26.0, 2), Hit::ListRow(1));
    assert_eq!(l.hit(280.5, 26.0, 2), Hit::Sidebar, "the separator");
    assert_eq!(l.hit(50.0, 2.0, 2), Hit::Sidebar, "above the first row");

    // Two tabs of TAB_MAX_COLS cells; the last three cells of each close it.
    assert_eq!(l.tab_cols(2), TAB_MAX_COLS);
    assert_eq!(l.hit(281.0, 10.0, 2), Hit::Tab(0));
    assert_eq!(l.hit(281.0 + 209.0, 10.0, 2), Hit::Tab(0));
    assert_eq!(l.hit(281.0 + 210.0, 10.0, 2), Hit::TabClose(0));
    assert_eq!(l.hit(281.0 + 239.0, 10.0, 2), Hit::TabClose(0));
    assert_eq!(l.hit(281.0 + 240.0, 10.0, 2), Hit::Tab(1));
    assert_eq!(l.hit(281.0 + 470.0, 10.0, 2), Hit::TabClose(1));
    assert_eq!(
        l.hit(281.0 + 480.0, 10.0, 2),
        Hit::TabBar,
        "right of the last tab"
    );
    assert_eq!(l.hit(300.0, 10.0, 0), Hit::TabBar, "no tab is open");

    assert_eq!(l.hit(281.0, 26.0, 2), Hit::Terminal);
    assert_eq!(l.hit(1280.0, 699.0, 2), Hit::Terminal);
    assert_eq!(l.hit(600.0, 700.0, 2), Hit::Status);
}

#[test]
fn many_tabs_share_the_bar() {
    let l = at(1281, 726);
    assert_eq!(l.tab_cols(10), 10);
    assert_eq!(l.tab_rect(9, 10).right(), 1281.0);
    assert_eq!(l.hit(1280.0, 10.0, 10), Hit::TabClose(9));
    // Too narrow for a close mark: every click selects.
    assert_eq!(l.tab_cols(40), 2);
    assert_eq!(l.hit(281.0 + 39.0, 10.0, 40), Hit::Tab(1));
    // More tabs than cells: nothing to hit, and nothing divides by zero.
    assert_eq!(l.tab_cols(500), 0);
    assert_eq!(l.hit(300.0, 10.0, 500), Hit::TabBar);
}

#[test]
fn the_list_scrolls_to_keep_its_cursor_in_view() {
    assert_eq!(list_top(0, 10), 0);
    assert_eq!(list_top(9, 10), 0);
    assert_eq!(list_top(10, 10), 1);
    assert_eq!(list_top(3, 0), 4, "no row fits: none is drawn");
}
