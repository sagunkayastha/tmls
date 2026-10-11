//! The window's layout and what a click hits. No atlas: the cell box is given.

use desktop::draw::{Button, Hit, Layout, layout};
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
    // Half a cell between the separator and the terminal's text.
    assert_eq!((l.terminal.x, l.terminal.y), (286.0, 26.0));
    assert_eq!((l.terminal.w, l.terminal.h), (995.0, 674.0));
    assert_eq!(l.terminal.right(), 1281.0);
    assert_eq!(
        (l.status.x, l.status.y, l.status.bottom()),
        (281.0, 700.0, 726.0)
    );
    assert_eq!(
        l.grid,
        (99, 33),
        "the cells that fit the terminal's area, not the window"
    );

    // The sidebar from the top: an inset, the list's lines, a line, five lines of legend,
    // the buttons, an inset.
    assert_eq!(l.list_cols, SIDEBAR_COLS);
    assert_eq!((l.list.x, l.list.y, l.list.w), (0.0, 3.0, 280.0));
    assert_eq!(l.list_lines, 29);
    assert_eq!(l.list.h, 580.0);
    assert_eq!((l.legend.y, l.legend.h), (603.0, 100.0));
    assert_eq!((l.buttons.y, l.buttons.bottom()), (703.0, 723.0));
    assert!(l.list.bottom() < l.legend.y);
    assert_eq!(
        l.list_lines_rect(2, 2),
        desktop::draw::Rect {
            x: 0.0,
            y: 43.0,
            w: 280.0,
            h: 40.0
        }
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
    // The list's lines are a cell high, below one inset of padding.
    assert_eq!(l.hit(50.0, 3.0, 2), Hit::List { line: 0, col: 5 });
    assert_eq!(l.hit(279.0, 23.0, 2), Hit::List { line: 1, col: 27 });
    assert_eq!(l.hit(0.0, 582.9, 2), Hit::List { line: 28, col: 0 });
    assert_eq!(l.hit(50.0, 583.0, 2), Hit::Sidebar, "under the last line");
    assert_eq!(l.hit(280.5, 26.0, 2), Hit::Sidebar, "the separator");
    assert_eq!(l.hit(50.0, 2.0, 2), Hit::Sidebar, "above the first row");
    // The last three cells of a line are the `+` of a host's header.
    assert!(!l.on_add(24) && l.on_add(25) && l.on_add(27));

    // The legend is not for clicking; the buttons are, each on its label: New  Open  Copy  ?  !  ✕
    assert_eq!(l.hit(50.0, 650.0, 2), Hit::Sidebar, "the legend");
    for (x, hit) in [
        (9.0, Hit::Sidebar),
        (10.0, Hit::Button(Button::New)),
        (39.0, Hit::Button(Button::New)),
        (40.0, Hit::Sidebar),
        (60.0, Hit::Button(Button::Open)),
        (99.0, Hit::Button(Button::Open)),
        (120.0, Hit::Button(Button::Copy)),
        (180.0, Hit::Button(Button::Help)),
        (190.0, Hit::Sidebar),
        // The bell: its mark and two columns for the count.
        (209.0, Hit::Sidebar),
        (210.0, Hit::Button(Button::Alerts)),
        (239.0, Hit::Button(Button::Alerts)),
        (240.0, Hit::Sidebar),
        // Quit, one column wide, the last but one of the list's 28.
        (259.0, Hit::Sidebar),
        (260.0, Hit::Button(Button::Quit)),
        (269.0, Hit::Button(Button::Quit)),
        (270.0, Hit::Sidebar),
    ] {
        assert_eq!(l.hit(x, 710.0, 2), hit, "x {x}");
    }
    assert_eq!(l.hit(15.0, 724.0, 2), Hit::Sidebar, "under the buttons");

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

    assert_eq!(
        l.hit(281.0, 26.0, 2),
        Hit::Terminal,
        "the padding left of the text"
    );
    assert_eq!(l.hit(286.0, 26.0, 2), Hit::Terminal);
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
fn a_low_window_has_no_legend_and_no_buttons() {
    // Four lines for the list, or the foot goes: 3 + 4 * 20 + (1 + 6 * 20 + 3) = 207.
    let l = at(1281, 207);
    assert_eq!((l.legend.h, l.buttons.h, l.list_lines), (100.0, 20.0, 4));
    let l = at(1281, 206);
    assert_eq!((l.legend.h, l.buttons.h), (0.0, 0.0));
    assert_eq!(l.list_lines, 10, "the list has the whole height");
    assert_eq!(l.hit(15.0, 190.0, 0), Hit::List { line: 9, col: 1 });
    assert_eq!(l.hit(15.0, 205.0, 0), Hit::Sidebar);
}

#[test]
fn a_form_is_in_the_middle_and_a_menu_at_the_pointer_inside_the_window() {
    let l = at(1280, 720);
    // 60 x 10 cells of 10 x 20 px.
    let form = l.panel_rect(60, 10, None);
    assert_eq!(
        (form.x, form.y, form.w, form.h),
        (340.0, 260.0, 600.0, 200.0)
    );
    let menu = l.panel_rect(24, 5, Some((100, 50)));
    assert_eq!((menu.x, menu.y), (100.0, 50.0));
    // Too close to the right and bottom edges: left of and above the pointer.
    let menu = l.panel_rect(24, 5, Some((1200, 700)));
    assert_eq!((menu.x, menu.y), (960.0, 600.0));
    assert!(menu.right() <= 1280.0 && menu.bottom() <= 720.0);
    // A window smaller than the panel: never past the top-left corner.
    let small = at(300, 120);
    let form = small.panel_rect(60, 10, None);
    assert_eq!((form.x, form.y), (0.0, 0.0));
    let menu = small.panel_rect(24, 5, Some((20, 10)));
    assert!(menu.x >= 0.0 && menu.y >= 0.0);

    // A pixel in cells from the panel's corner, as `Form::click` takes it.
    let form = l.panel_rect(60, 10, None);
    assert_eq!(l.panel_cell(form, 340.0, 260.0), (0.0, 0.0));
    assert_eq!(l.panel_cell(form, 345.0, 290.0), (0.5, 1.5));
    assert!(l.panel_cell(form, 330.0, 250.0).0 < 0.0);
}
