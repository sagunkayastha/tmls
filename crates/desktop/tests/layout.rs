//! The window's layout and what a click hits. No atlas: the cell box is given.

use desktop::draw::{Button, Hit, Layout, Rect, SideHeader, layout, layout_with, side_header};
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
    // the buttons' two rows, an inset.
    assert_eq!(l.list_cols, SIDEBAR_COLS);
    assert_eq!((l.list.x, l.list.y, l.list.w), (0.0, 3.0, 280.0));
    assert_eq!(l.list_lines, 28);
    assert_eq!(l.list.h, 560.0);
    assert_eq!((l.legend.y, l.legend.h), (583.0, 100.0));
    assert_eq!((l.buttons.y, l.buttons.bottom()), (683.0, 723.0));
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
    assert_eq!(l.hit(0.0, 562.9, 2), Hit::List { line: 27, col: 0 });
    assert_eq!(l.hit(50.0, 563.0, 2), Hit::Sidebar, "under the last line");
    // The separator and the padding right of it are its handle, at any height.
    for y in [0.0, 26.0, 400.0, 725.0] {
        assert_ne!(l.hit(279.9, y, 2), Hit::Separator, "y {y}");
        assert_eq!(l.hit(280.0, y, 2), Hit::Separator, "y {y}");
        assert_eq!(l.hit(285.9, y, 2), Hit::Separator, "y {y}");
        assert_ne!(l.hit(286.0, y, 2), Hit::Separator, "y {y}");
    }
    assert_eq!(l.hit(50.0, 2.0, 2), Hit::Sidebar, "above the first row");
    // The last three cells of a line are the `+` of a host's header.
    assert!(!l.on_add(24) && l.on_add(25) && l.on_add(27));

    // The legend is not for clicking; the buttons are, each on its label: New  Open  Copy  Sketch / ?  !  ✕
    assert_eq!(l.hit(50.0, 650.0, 2), Hit::Sidebar, "the legend");
    for (x, hit) in [
        (9.0, Hit::Sidebar),
        (10.0, Hit::Button(Button::New)),
        (39.0, Hit::Button(Button::New)),
        (40.0, Hit::Sidebar),
        (60.0, Hit::Button(Button::Open)),
        (99.0, Hit::Button(Button::Open)),
        (120.0, Hit::Button(Button::Copy)),
        (179.0, Hit::Sidebar),
        (180.0, Hit::Button(Button::Sketch)),
        (239.0, Hit::Button(Button::Sketch)),
        (240.0, Hit::Sidebar),
    ] {
        assert_eq!(l.hit(x, 690.0, 2), hit, "upper row, x {x}");
    }
    for (x, hit) in [
        (9.0, Hit::Sidebar),
        (10.0, Hit::Button(Button::Help)),
        (20.0, Hit::Sidebar),
        // The bell: its mark and two columns for the count.
        (39.0, Hit::Sidebar),
        (40.0, Hit::Button(Button::Alerts)),
        (69.0, Hit::Button(Button::Alerts)),
        (70.0, Hit::Sidebar),
        // Quit, one column wide, the last but one of the list's 28, away from the others.
        (259.0, Hit::Sidebar),
        (260.0, Hit::Button(Button::Quit)),
        (269.0, Hit::Button(Button::Quit)),
        (270.0, Hit::Sidebar),
    ] {
        assert_eq!(l.hit(x, 710.0, 2), hit, "lower row, x {x}");
    }
    assert_eq!(l.hit(15.0, 724.0, 2), Hit::Sidebar, "under the buttons");

    // Two tabs of TAB_MAX_COLS cells; the last three cells of each close it.
    assert_eq!(l.tab_cols(2), TAB_MAX_COLS);
    // (Its first five pixels are the separator's handle.)
    assert_eq!(l.hit(286.0, 10.0, 2), Hit::Tab(0));
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
        l.hit(287.0, 26.0, 2),
        Hit::Terminal,
        "right of the separator's handle"
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
    // Four lines for the list, or the foot goes: 3 + 4 * 20 + (1 + 7 * 20 + 3) = 227.
    let l = at(1281, 227);
    assert_eq!((l.legend.h, l.buttons.h, l.list_lines), (100.0, 40.0, 4));
    let l = at(1281, 226);
    assert_eq!((l.legend.h, l.buttons.h), (0.0, 0.0));
    assert_eq!(l.list_lines, 11, "the list has the whole height");
    assert_eq!(l.hit(15.0, 210.0, 0), Hit::List { line: 10, col: 1 });
    assert_eq!(l.hit(15.0, 225.0, 0), Hit::Sidebar);
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

#[test]
fn the_list_is_as_wide_as_the_theme_says() {
    let wide = Theme {
        sidebar_cols: 40,
        ..Theme::default()
    };
    let l = layout((1281, 726), 1.0, &cell(), &wide);
    assert_eq!(
        (l.sidebar.w, l.list_cols, l.separator.x),
        (400.0, 40, 400.0)
    );
    assert_eq!(l.terminal.right(), 1281.0);
    assert_eq!(
        l.grid.0, 87,
        "the terminal gives the columns up: (1281 - 401 - 5) / 10"
    );
    // Quit stays the last but one column; the host's `+` the last three.
    assert_eq!(l.hit(385.0, 710.0, 0), Hit::Button(Button::Quit));
    assert_eq!(l.hit(265.0, 710.0, 0), Hit::Sidebar);
    assert!(!l.on_add(36) && l.on_add(37));
    assert_eq!(l.hit(403.0, 300.0, 0), Hit::Separator);

    let narrow = Theme {
        sidebar_cols: 20,
        ..Theme::default()
    };
    let l = layout((1281, 726), 1.0, &cell(), &narrow);
    assert_eq!((l.sidebar.w, l.list_cols), (200.0, 20));
    assert_eq!(l.hit(185.0, 710.0, 0), Hit::Button(Button::Quit));
    // Never more than half the window.
    let greedy = Theme {
        sidebar_cols: 500,
        ..Theme::default()
    };
    assert_eq!(layout((1281, 726), 1.0, &cell(), &greedy).sidebar.w, 640.0);
}

// --- the file panel -----------------------------------------------------------------------------

fn with_panel(width: u32, height: u32) -> Layout {
    layout_with((width, height), 1.0, &cell(), &Theme::default(), true)
}

#[test]
fn without_a_panel_nothing_is_laid_out_for_one() {
    for (width, height) in [(1281, 726), (640, 480), (40, 30)] {
        let l = at(width, height);
        assert_eq!(
            l,
            layout_with((width, height), 1.0, &cell(), &Theme::default(), false)
        );
        assert_eq!(
            (l.side.w, l.side_divider.w, l.side_cols, l.side_lines),
            (0.0, 0.0, 0, 0)
        );
        assert_eq!(
            (l.side_header.w, l.side_body.w, l.side_body.h),
            (0.0, 0.0, 0.0)
        );
        for (x, y) in [(width as f32 - 1.0, 30.0), (width as f32 - 1.0, 60.0)] {
            assert!(!matches!(
                l.hit(x, y, 1),
                Hit::Side | Hit::SideOpen | Hit::SideClose
            ));
        }
    }
}

#[test]
fn an_open_panel_has_the_right_half_of_the_terminals_area() {
    let (closed, l) = (at(1281, 726), with_panel(1281, 726));
    // Everything around the terminal stays where it is.
    assert_eq!(
        (
            l.sidebar,
            l.separator,
            l.tab_bar,
            l.status,
            l.list,
            l.legend,
            l.buttons
        ),
        (
            closed.sidebar,
            closed.separator,
            closed.tab_bar,
            closed.status,
            closed.list,
            closed.legend,
            closed.buttons
        )
    );
    // 1000 px right of the list's line: the panel has 500 of them, its divider included.
    let rect = |x, y, w, h| Rect { x, y, w, h };
    assert_eq!(l.side, rect(781.0, 26.0, 500.0, 674.0));
    assert_eq!(l.side_divider, rect(781.0, 26.0, 1.0, 674.0));
    assert_eq!(l.terminal, rect(286.0, 26.0, 495.0, 674.0));
    assert_eq!(
        l.terminal.right(),
        l.side_divider.x,
        "nothing between, nothing shared"
    );
    assert_eq!(
        l.grid,
        (49, 33),
        "the sessions are resized to the narrower terminal"
    );
    assert_eq!(closed.grid, (99, 33));
    // The header is a row like the tab bar; the text is half a cell right of the divider.
    assert_eq!(l.side_header, rect(782.0, 26.0, 499.0, 26.0));
    assert_eq!(l.side_body, rect(787.0, 52.0, 490.0, 640.0));
    assert_eq!((l.side_cols, l.side_lines), (49, 32));
    assert!(l.side_body.right() <= 1281.0 && l.side_body.bottom() <= l.status.y);
}

#[test]
fn the_header_has_the_path_then_open_in_app_then_the_close_mark() {
    let header = |title_cols, open, close| SideHeader {
        title_cols,
        open,
        close,
    };
    // 49 columns: × on 47 (cells 46-48), a column, `Open in app` on 34-44, two columns.
    assert_eq!(side_header(49), header(32, Some(34), Some(47)));
    // The label goes when the path would have fewer than eight columns beside it.
    assert_eq!(side_header(25), header(8, Some(10), Some(23)));
    assert_eq!(side_header(24), header(19, None, Some(22)));
    assert_eq!(side_header(4), header(0, None, Some(2)));
    assert_eq!(side_header(3), header(0, None, Some(1)));
    assert_eq!(side_header(2), header(0, None, None));
    assert_eq!(side_header(0), header(0, None, None));
}

#[test]
fn clicks_hit_the_panels_close_mark_and_open_in_app() {
    let l = with_panel(1281, 726);
    // The divider is the panel's; left of it is the terminal.
    assert_eq!(l.hit(780.9, 300.0, 1), Hit::Terminal);
    assert_eq!(l.hit(781.0, 300.0, 1), Hit::Side);
    assert_eq!(l.hit(1000.0, 300.0, 1), Hit::Side, "the text");
    assert_eq!(l.hit(1270.0, 699.0, 1), Hit::Side, "under the last line");
    // The header's columns start at x 787, ten pixels each.
    let col = |col: usize| 787.0 + col as f32 * 10.0;
    for (x, hit) in [
        (781.0, Hit::Side),
        (786.0, Hit::Side),
        (col(0), Hit::Side),
        (col(33) + 9.0, Hit::Side),
        (col(34), Hit::SideOpen),
        (col(44) + 9.0, Hit::SideOpen),
        (col(45), Hit::Side),
        (col(46), Hit::SideClose),
        (col(48) + 9.0, Hit::SideClose),
        // The four pixels right of the last column.
        (1280.0, Hit::SideClose),
    ] {
        assert_eq!(l.hit(x, 30.0, 1), hit, "header, x {x}");
        assert_eq!(l.hit(x, 51.9, 1), hit, "header's last pixel row, x {x}");
        assert_eq!(l.hit(x, 52.0, 1), Hit::Side, "first line of text, x {x}");
    }
    // Above and below it the window is as before.
    assert_eq!(l.hit(1270.0, 10.0, 0), Hit::TabBar);
    assert_eq!(l.hit(1270.0, 25.9, 0), Hit::TabBar);
    assert_eq!(l.hit(1270.0, 700.0, 1), Hit::Status);
    assert_eq!(l.hit(1281.0, 30.0, 1), Hit::Outside);
    assert_eq!(l.hit(283.0, 30.0, 1), Hit::Separator);
    assert_eq!(l.hit(50.0, 30.0, 1), Hit::List { line: 1, col: 5 });
}

#[test]
fn a_narrow_window_has_a_narrow_panel_or_none() {
    // 159 px right of the list: 79 for the panel, seven columns each side.
    let l = with_panel(320, 200);
    assert_eq!((l.side.x, l.side.w), (241.0, 79.0));
    assert_eq!((l.grid.0, l.side_cols), (7, 7));
    assert_eq!(
        side_header(7),
        SideHeader {
            title_cols: 2,
            open: None,
            close: Some(5)
        }
    );
    assert_eq!(l.hit(300.0, 30.0, 0), Hit::SideClose);
    assert_eq!(l.hit(260.0, 30.0, 0), Hit::Side);
    // No room for a column of text on both sides: no panel, and the terminal keeps it all.
    for (width, height) in [(1, 1), (40, 30), (56, 300)] {
        let (closed, l) = (at(width, height), with_panel(width, height));
        assert_eq!(l, closed, "{width}x{height}");
    }
    // Any size lays out, with whole pixels and nothing negative.
    for width in (1..700).step_by(7) {
        for height in [1, 50, 400] {
            let l = with_panel(width, height);
            for r in [
                l.terminal,
                l.side,
                l.side_divider,
                l.side_header,
                l.side_body,
            ] {
                assert!(r.w >= 0.0 && r.h >= 0.0, "{width}x{height}: {r:?}");
                assert_eq!(
                    (r.x.fract(), r.w.fract()),
                    (0.0, 0.0),
                    "{width}x{height}: {r:?}"
                );
            }
            assert!(l.side_body.right() <= width as f32 || l.side.w == 0.0);
            assert!(l.terminal.right() <= l.side.x, "{width}x{height}");
            assert!(l.grid.0 >= 1);
        }
    }
}
