//! The window's layout and what a click hits. No atlas: the cell box is given.

use desktop::draw::{
    Button, FILES_COLS, Hit, Layout, Rect, SideHeader, default_panel_cols, layout, layout_with,
    side_header, side_header_with,
};
use desktop::theme::{PANEL_MIN_COLS, SIDEBAR_COLS, TAB_MAX_COLS, Theme};
use desktop::viewer::SideLook;
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

    // The sidebar from the top: a band as high as the tab bar, the list's lines — each level
    // with a row of the terminal, as many as end above the legend's line —, that line, six
    // lines of legend, the buttons' two rows — the second level with the terminal's last row
    // (26 + 32 * 20) — and empty space down to the window's edge.
    assert_eq!(l.list_cols, SIDEBAR_COLS);
    assert_eq!((l.list.x, l.list.y, l.list.w), (0.0, 26.0, 280.0));
    assert_eq!(l.list_lines, 24);
    assert_eq!(l.list.h, 480.0);
    assert_eq!((l.legend.y, l.legend.h), (526.0, 120.0));
    assert_eq!((l.buttons.y, l.buttons.bottom()), (646.0, 686.0));
    assert_eq!(l.buttons.bottom(), l.terminal.y + l.grid.1 as f32 * 20.0);
    assert!(l.list.bottom() < l.legend.y);
    assert_eq!(
        l.list_lines_rect(2, 2),
        desktop::draw::Rect {
            x: 0.0,
            y: 66.0,
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
    // The list's lines are a cell high, from the terminal's first row down.
    assert_eq!(l.hit(50.0, 26.0, 2), Hit::List { line: 0, col: 5 });
    assert_eq!(l.hit(279.0, 46.0, 2), Hit::List { line: 1, col: 27 });
    assert_eq!(l.hit(0.0, 505.9, 2), Hit::List { line: 23, col: 0 });
    assert_eq!(l.hit(50.0, 506.0, 2), Hit::Sidebar, "under the last line");
    // The separator and the padding right of it are its handle, at any height.
    for y in [0.0, 26.0, 400.0, 725.0] {
        assert_ne!(l.hit(279.9, y, 2), Hit::Separator, "y {y}");
        assert_eq!(l.hit(280.0, y, 2), Hit::Separator, "y {y}");
        assert_eq!(l.hit(285.9, y, 2), Hit::Separator, "y {y}");
        assert_ne!(l.hit(286.0, y, 2), Hit::Separator, "y {y}");
    }
    assert_eq!(l.hit(50.0, 25.9, 2), Hit::Sidebar, "above the first row");
    assert_eq!(l.hit(50.0, 0.0, 2), Hit::Sidebar, "above the first row");
    // The last three cells of a line are the `+` of a host's header.
    assert!(!l.on_add(24) && l.on_add(25) && l.on_add(27));

    // The legend is not for clicking; the buttons are, each on its label: ?  !  ✕ / New  Open  Copy  Sketch
    assert_eq!(l.hit(50.0, 600.0, 2), Hit::Sidebar, "the legend");
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
        assert_eq!(l.hit(x, 670.0, 2), hit, "lower row, x {x}");
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
        assert_eq!(l.hit(x, 650.0, 2), hit, "upper row, x {x}");
    }
    assert_eq!(l.hit(15.0, 686.0, 2), Hit::Sidebar, "under the buttons");
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
    // 100 columns, less the seven of `Files` at the bar's right end.
    assert_eq!(l.tab_cols(10), 9);
    assert_eq!(l.tab_rect(9, 10).right(), 1181.0);
    assert_eq!(l.hit(1180.0, 10.0, 10), Hit::TabClose(9));
    assert_eq!(l.hit(1181.0, 10.0, 10), Hit::TabBar);
    // Too narrow for a close mark: every click selects.
    assert_eq!(l.tab_cols(40), 2);
    assert_eq!(l.hit(281.0 + 39.0, 10.0, 40), Hit::Tab(1));
    // More tabs than cells: nothing to hit, and nothing divides by zero.
    assert_eq!(l.tab_cols(500), 0);
    assert_eq!(l.hit(300.0, 10.0, 500), Hit::TabBar);
}

#[test]
fn a_low_window_has_no_legend_and_no_buttons() {
    // Four lines for the list, or the foot goes. At 312 the terminal has thirteen rows, the
    // buttons start at its last but one (26 + 11 * 20 = 246), the legend at 126: the list has
    // the 99 pixels from 26 to the legend's line.
    let l = at(1281, 312);
    assert_eq!((l.legend.h, l.buttons.h, l.list_lines), (120.0, 40.0, 4));
    assert_eq!((l.list.y, l.legend.y, l.buttons.y), (26.0, 126.0, 246.0));
    let l = at(1281, 311);
    assert_eq!((l.legend.h, l.buttons.h), (0.0, 0.0));
    assert_eq!(
        l.list_lines, 14,
        "the list has the whole height under the bar's band"
    );
    assert_eq!(l.hit(15.0, 250.0, 0), Hit::List { line: 11, col: 1 });
    assert_eq!(l.hit(15.0, 306.0, 0), Hit::Sidebar);
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
    assert_eq!(l.hit(385.0, 650.0, 0), Hit::Button(Button::Quit));
    assert_eq!(l.hit(265.0, 650.0, 0), Hit::Sidebar);
    assert!(!l.on_add(36) && l.on_add(37));
    assert_eq!(l.hit(403.0, 300.0, 0), Hit::Separator);

    let narrow = Theme {
        sidebar_cols: 20,
        ..Theme::default()
    };
    let l = layout((1281, 726), 1.0, &cell(), &narrow);
    assert_eq!((l.sidebar.w, l.list_cols), (200.0, 20));
    assert_eq!(l.hit(185.0, 650.0, 0), Hit::Button(Button::Quit));
    // Never more than half the window.
    let greedy = Theme {
        sidebar_cols: 500,
        ..Theme::default()
    };
    assert_eq!(layout((1281, 726), 1.0, &cell(), &greedy).sidebar.w, 640.0);
}

// --- the file panel -----------------------------------------------------------------------------

/// The panel as it opens: no width was chosen yet.
fn with_panel(width: u32, height: u32) -> Layout {
    let (m, theme) = (cell(), Theme::default());
    let cols = default_panel_cols((width, height), 1.0, &m, &theme);
    layout_with((width, height), 1.0, &m, &theme, Some(cols))
}

/// A panel asked to be `cols` columns wide.
fn panel_of(width: u32, height: u32, cols: usize) -> Layout {
    layout_with((width, height), 1.0, &cell(), &Theme::default(), Some(cols))
}

#[test]
fn without_a_panel_nothing_is_laid_out_for_one() {
    for (width, height) in [(1281, 726), (640, 480), (40, 30)] {
        let l = at(width, height);
        assert_eq!(
            l,
            layout_with((width, height), 1.0, &cell(), &Theme::default(), None)
        );
        assert_eq!(
            (l.side.w, l.side_divider.w, l.side_cols, l.side_lines),
            (0.0, 0.0, 0, 0)
        );
        assert_eq!(l.side_grab.w, 0.0);
        assert_eq!(
            (l.side_header.w, l.side_body.w, l.side_body.h),
            (0.0, 0.0, 0.0)
        );
        for (x, y) in [(width as f32 - 1.0, 30.0), (width as f32 - 1.0, 60.0)] {
            assert!(!matches!(
                l.hit(x, y, 1),
                Hit::Side | Hit::SideOpen | Hit::SideClose | Hit::SideDivider
            ));
        }
    }
}

#[test]
fn an_open_panel_has_the_right_half_of_the_terminals_area() {
    let (closed, l) = (at(1281, 726), with_panel(1281, 726));
    // Everything around the terminal stays where it is, but the tab bar: it ends where the
    // terminal does, and the panel's header is beside it.
    assert_eq!(
        (
            l.sidebar,
            l.separator,
            l.status,
            l.list,
            l.legend,
            l.buttons
        ),
        (
            closed.sidebar,
            closed.separator,
            closed.status,
            closed.list,
            closed.legend,
            closed.buttons
        )
    );
    // 1000 px right of the list's line. Half of them hold the divider, the padding and 49
    // whole columns: the panel is 1 + 5 + 490 px and the terminal has the rest.
    assert_eq!(
        default_panel_cols((1281, 726), 1.0, &cell(), &Theme::default()),
        49
    );
    let rect = |x, y, w, h| Rect { x, y, w, h };
    assert_eq!(l.side, rect(785.0, 0.0, 496.0, 700.0));
    assert_eq!(l.side_divider, rect(785.0, 0.0, 1.0, 700.0));
    assert_eq!(closed.tab_bar, rect(281.0, 0.0, 1000.0, 26.0));
    assert_eq!(l.tab_bar, rect(281.0, 0.0, 504.0, 26.0));
    assert_eq!(l.terminal, rect(286.0, 26.0, 499.0, 674.0));
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
    // The header is in the tab bar's band; the text is half a cell right of the divider and
    // has the terminal's rows.
    assert_eq!(l.side_header, rect(786.0, 0.0, 495.0, 26.0));
    assert_eq!(l.side_body, rect(791.0, 26.0, 490.0, 660.0));
    assert_eq!((l.side_cols, l.side_lines), (49, 33));
    assert_eq!(
        l.side_body.right(),
        1281.0,
        "the last column ends the window"
    );
    assert!(l.side_body.bottom() <= l.status.y);
}

#[test]
fn the_header_has_the_path_then_open_in_app_then_the_close_mark() {
    let header = |title_cols, open, close| SideHeader {
        title_col: 0,
        title_cols,
        open,
        close,
        back: None,
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
    // The divider (x 785) and five pixels either side of it are the handle for dragging.
    assert_eq!(l.hit(779.9, 300.0, 1), Hit::Terminal);
    assert_eq!(l.hit(780.0, 300.0, 1), Hit::SideDivider);
    assert_eq!(l.hit(1000.0, 300.0, 1), Hit::Side, "the text");
    assert_eq!(l.hit(1270.0, 699.0, 1), Hit::Side, "under the last line");
    // The header's columns start at x 791, ten pixels each.
    let col = |col: usize| 791.0 + col as f32 * 10.0;
    for (x, hit) in [
        (785.0, Hit::SideDivider),
        (790.0, Hit::SideDivider),
        (col(0), Hit::Side),
        (col(33) + 9.0, Hit::Side),
        (col(34), Hit::SideOpen),
        (col(44) + 9.0, Hit::SideOpen),
        (col(45), Hit::Side),
        (col(46), Hit::SideClose),
        (col(48) + 9.0, Hit::SideClose),
        (1280.0, Hit::SideClose),
    ] {
        assert_eq!(l.hit(x, 0.0, 1), hit, "header's first pixel row, x {x}");
        assert_eq!(l.hit(x, 10.0, 1), hit, "header, x {x}");
        assert_eq!(l.hit(x, 25.9, 1), hit, "header's last pixel row, x {x}");
        let text = match hit {
            Hit::SideDivider => hit,
            _ => Hit::Side,
        };
        assert_eq!(l.hit(x, 26.0, 1), text, "first line of text, x {x}");
    }
    // Left of it and below it the window is as before; `Files` is the last of the tab bar.
    assert_eq!(l.hit(700.0, 10.0, 0), Hit::TabBar);
    assert_eq!(l.hit(700.0, 25.9, 0), Hit::TabBar);
    assert_eq!(l.hit(750.0, 25.9, 0), Hit::Files);
    assert_eq!(l.hit(1270.0, 700.0, 1), Hit::Status);
    assert_eq!(l.hit(1281.0, 30.0, 1), Hit::Outside);
    assert_eq!(l.hit(283.0, 30.0, 1), Hit::Separator);
    assert_eq!(l.hit(50.0, 30.0, 1), Hit::List { line: 0, col: 5 });
}

#[test]
fn a_narrow_window_has_a_narrow_panel_or_none() {
    // The least that has a panel: right of the list's line the terminal's padding and
    // `PANEL_MIN_COLS` columns, then the divider, the panel's padding and as many columns.
    let least = 280 + 1 + 5 + 200 + 1 + 5 + 200;
    let l = with_panel(least, 300);
    assert_eq!((l.side.x, l.side.w), (486.0, 206.0));
    assert_eq!(
        (l.grid.0, l.side_cols),
        (PANEL_MIN_COLS as u16, PANEL_MIN_COLS)
    );
    assert_eq!(l.hit(680.0, 10.0, 0), Hit::SideClose);
    assert_eq!(l.hit(500.0, 10.0, 0), Hit::Side);
    // A pixel less, or much less: no panel, and the terminal keeps it all.
    for (width, height) in [(1, 1), (40, 30), (56, 300), (320, 200), (least - 1, 300)] {
        let (closed, l) = (at(width, height), with_panel(width, height));
        assert_eq!(l, closed, "{width}x{height}");
        assert_eq!(l, panel_of(width, height, 40), "{width}x{height}");
    }
    // Any size lays out, with whole pixels and nothing negative.
    for width in (1..1400).step_by(7) {
        for height in [1, 50, 400] {
            for l in [with_panel(width, height), panel_of(width, height, 33)] {
                for r in [
                    l.terminal,
                    l.side,
                    l.side_divider,
                    l.side_grab,
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
                assert!(
                    l.side.w == 0.0
                        || (l.side_cols >= PANEL_MIN_COLS && l.grid.0 as usize >= PANEL_MIN_COLS),
                    "{width}x{height}: {} and {} columns",
                    l.grid.0,
                    l.side_cols
                );
            }
        }
    }
}

// --- the panel's width ------------------------------------------------------------------------

/// The window, the scale and the cell of a display at `scale`: 1281 x 726 and 10 x 20 logical
/// pixels.
fn display(scale: f32) -> ((u32, u32), CellMetrics) {
    let px = |logical: f32| (logical * scale).round();
    let m = CellMetrics {
        width: px(10.0),
        height: px(20.0),
        baseline: px(15.0),
        underline_y: px(17.0),
        strike_y: px(10.0),
        stroke: px(1.0),
    };
    ((px(1281.0) as u32, px(726.0) as u32), m)
}

const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

#[test]
fn the_panel_is_as_wide_as_asked_when_that_fits() {
    for scale in SCALES {
        let (window, m) = display(scale);
        let theme = Theme::default();
        let closed = layout_with(window, scale, &m, &theme, None);
        let l = layout_with(window, scale, &m, &theme, Some(40));
        assert_eq!(l.side_cols, 40, "scale {scale}");
        // The divider, the padding left of the text, the columns.
        let (line, pad) = (l.side_divider.w, l.side_body.x - l.side_header.x);
        assert!(line >= 1.0 && pad > 0.0, "scale {scale}");
        assert_eq!(l.side.w, line + pad + 40.0 * m.width, "scale {scale}");
        assert_eq!(l.side.right(), window.0 as f32, "scale {scale}");
        assert_eq!(l.side_body.right(), window.0 as f32, "scale {scale}");
        assert_eq!(l.terminal.right(), l.side.x, "scale {scale}");
        assert_eq!(l.terminal.x, closed.terminal.x, "scale {scale}");
        assert_eq!(
            l.grid.0,
            ((closed.terminal.w - l.side.w) / m.width).floor() as u16,
            "scale {scale}"
        );
        // One column more for the panel is one less for the terminal, or the same pixels.
        let wider = layout_with(window, scale, &m, &theme, Some(41));
        assert_eq!(wider.side.x, l.side.x - m.width, "scale {scale}");
    }
}

#[test]
fn a_panel_asked_too_wide_leaves_the_terminal_its_least_columns() {
    for scale in SCALES {
        let (window, m) = display(scale);
        let theme = Theme::default();
        for asked in [80, 500, usize::MAX] {
            let l = layout_with(window, scale, &m, &theme, Some(asked));
            assert_eq!(
                l.grid.0, PANEL_MIN_COLS as u16,
                "scale {scale}, asked {asked}"
            );
            assert!(
                l.terminal.w >= PANEL_MIN_COLS as f32 * m.width,
                "scale {scale}"
            );
            assert!(l.side_cols >= PANEL_MIN_COLS && l.side_cols < asked);
            // The widest that fits: a column more would take one of the terminal's.
            assert!(
                l.terminal.w - m.width < PANEL_MIN_COLS as f32 * m.width,
                "scale {scale}"
            );
        }
    }
}

#[test]
fn a_panel_asked_too_narrow_gets_its_least_columns() {
    for scale in SCALES {
        let (window, m) = display(scale);
        let theme = Theme::default();
        let least = layout_with(window, scale, &m, &theme, Some(PANEL_MIN_COLS));
        assert_eq!(least.side_cols, PANEL_MIN_COLS, "scale {scale}");
        for asked in [0, 1, PANEL_MIN_COLS - 1] {
            let l = layout_with(window, scale, &m, &theme, Some(asked));
            assert_eq!(l, least, "scale {scale}, asked {asked}");
        }
    }
}

#[test]
fn a_window_with_too_few_columns_right_of_the_list_has_no_panel() {
    for scale in SCALES {
        let (window, m) = display(scale);
        let theme = Theme::default();
        // The list and one column short of `PANEL_MIN_COLS` twice; and exactly twice, which
        // the divider and the padding do not fit beside.
        for cols in [2 * PANEL_MIN_COLS - 1, 2 * PANEL_MIN_COLS] {
            let width = ((SIDEBAR_COLS + cols) as f32 * m.width) as u32;
            let closed = layout_with((width, window.1), scale, &m, &theme, None);
            assert_eq!(closed.sidebar.w, SIDEBAR_COLS as f32 * m.width);
            for asked in [0, PANEL_MIN_COLS, 40, 500] {
                let l = layout_with((width, window.1), scale, &m, &theme, Some(asked));
                assert_eq!(l.side.w, 0.0, "scale {scale}, {cols} columns");
                assert_eq!(l.grid, closed.grid, "scale {scale}, {cols} columns");
                assert_eq!(l, closed, "scale {scale}, {cols} columns");
            }
            assert_eq!(
                default_panel_cols((width, window.1), scale, &m, &theme),
                0,
                "scale {scale}: nothing to open with"
            );
        }
    }
}

#[test]
fn what_the_app_says_of_a_click_in_a_window_with_no_room_for_the_panel() {
    // `app` opens the panel, lays the window out and looks at `side.w`: nothing there means
    // no panel is kept and the status line says why, in one line.
    let l = panel_of(40 + SIDEBAR_COLS as u32 * 10, 726, 40);
    assert_eq!((l.side.w, l.side_cols), (0.0, 0));
    assert_eq!(
        desktop::viewer::TOO_NARROW,
        "window too narrow for the file panel"
    );
    // With room for both there is a panel, and nothing of the kind to say.
    assert!(panel_of(1281, 726, 40).side.w > 0.0);
}

#[test]
fn a_panel_opens_with_half_the_columns_right_of_the_list() {
    for scale in SCALES {
        let (window, m) = display(scale);
        let theme = Theme::default();
        let cols = default_panel_cols(window, scale, &m, &theme);
        let l = layout_with(window, scale, &m, &theme, Some(cols));
        assert_eq!(l.side_cols, cols, "scale {scale}");
        // The terminal has as many, or the one column that is left over.
        assert!(
            l.grid.0 as usize == cols || l.grid.0 as usize == cols + 1,
            "scale {scale}: {} and {cols}",
            l.grid.0
        );
    }
    // Half of 411 px has 19 columns beside the divider and the padding: the least it is.
    let width = 280 + 1 + 5 + 200 + 1 + 5 + 200;
    assert_eq!(
        default_panel_cols((width, 400), 1.0, &cell(), &Theme::default()),
        PANEL_MIN_COLS
    );
}

#[test]
fn the_divider_and_a_few_pixels_either_side_of_it_are_its_handle() {
    let l = panel_of(1281, 726, 40);
    // 406 px of panel: the divider is the pixel column 875, the handle five more each way.
    let rect = |x, y, w, h| Rect { x, y, w, h };
    // It is as high as the panel: from the window's top to the status line.
    assert_eq!(l.side_divider, rect(875.0, 0.0, 1.0, 700.0));
    assert_eq!(l.grab, 5.0);
    assert_eq!(l.side_grab, rect(870.0, 0.0, 11.0, 700.0));
    for y in [0.0, 10.0, 26.0, 30.0, 300.0, 699.9] {
        // Left of the handle: `Files` in the tab bar's band, the terminal under it.
        let left = if y < 26.0 { Hit::Files } else { Hit::Terminal };
        assert_eq!(l.hit(869.9, y, 1), left, "y {y}");
        assert_eq!(l.hit(870.0, y, 1), Hit::SideDivider, "y {y}");
        assert_eq!(l.hit(875.0, y, 1), Hit::SideDivider, "y {y}");
        assert_eq!(l.hit(880.9, y, 1), Hit::SideDivider, "y {y}");
        assert_eq!(l.hit(881.0, y, 1), Hit::Side, "y {y}");
    }
    // Not below the panel.
    assert_eq!(l.hit(875.0, 700.0, 1), Hit::Status);
    // The handle follows the scale, and the divider is in the middle of it.
    for scale in SCALES {
        let (window, m) = display(scale);
        let l = layout_with(window, scale, &m, &Theme::default(), Some(40));
        let (x, w, grab) = (l.side_divider.x, l.side_divider.w, l.grab);
        assert!(grab >= 1.0, "scale {scale}");
        assert_eq!(
            (l.side_grab.x, l.side_grab.w),
            (x - grab, w + 2.0 * grab),
            "scale {scale}"
        );
        assert_eq!(l.hit(x - grab, 300.0, 1), Hit::SideDivider, "scale {scale}");
        assert_eq!(
            l.hit(x + w + grab - 0.1, 300.0, 1),
            Hit::SideDivider,
            "scale {scale}"
        );
        assert_eq!(
            l.hit(x - grab - 0.1, 300.0, 1),
            Hit::Terminal,
            "scale {scale}"
        );
        assert_eq!(l.hit(x + w + grab, 300.0, 1), Hit::Side, "scale {scale}");
    }
    // No panel, no handle.
    assert_eq!(at(1281, 726).hit(875.0, 300.0, 1), Hit::Terminal);
}

#[test]
fn a_drag_keeps_the_pixel_of_the_handle_that_was_pressed() {
    // 40 columns: the divider is the pixel column 875, its handle 870 to 880.
    let l = panel_of(1281, 726, 40);
    for offset in [-5.0, -3.0, 0.0, 0.5, 5.9] {
        let pressed = l.side_divider.x + offset;
        assert_eq!(
            l.hit(pressed, 300.0, 1),
            Hit::SideDivider,
            "offset {offset}"
        );
        // Not moved, or by less than half a column: the width that is.
        for moved in [0.0, 1.0, -1.0, 4.9, -4.9] {
            assert_eq!(
                l.divider_cols(pressed + moved, offset),
                40,
                "offset {offset}, moved {moved}"
            );
        }
        // A column to the right is one fewer for the panel, to the left one more.
        assert_eq!(
            l.divider_cols(pressed + 10.0, offset),
            39,
            "offset {offset}"
        );
        assert_eq!(
            l.divider_cols(pressed - 10.0, offset),
            41,
            "offset {offset}"
        );
        assert_eq!(
            l.divider_cols(pressed - 206.0, offset),
            61,
            "offset {offset}"
        );
    }
    // Past the window's edges: no columns, or all there could be. The layout says what fits.
    assert_eq!(l.divider_cols(5000.0, 0.0), 0);
    assert_eq!(l.divider_cols(-5000.0, 0.0), 628);
    assert_eq!(l.divider_cols(0.0, 0.0), 128, "(1281 - 6) / 10, rounded");
    // At another scale the handle and the columns are wider.
    for scale in SCALES {
        let (window, m) = display(scale);
        let l = layout_with(window, scale, &m, &Theme::default(), Some(40));
        for offset in [-l.grab, 0.0, l.grab] {
            let pressed = l.side_divider.x + offset;
            assert_eq!(l.divider_cols(pressed, offset), 40, "scale {scale}");
            assert_eq!(l.divider_cols(pressed + 1.0, offset), 40, "scale {scale}");
            assert_eq!(
                l.divider_cols(pressed - 3.0 * m.width, offset),
                43,
                "scale {scale}"
            );
        }
    }
}

#[test]
fn files_is_the_tab_bars_last_seven_columns_and_the_tabs_lose_them() {
    let rect = |x, y, w, h| Rect { x, y, w, h };
    // The bar is 1000 px from x 281, or 504 px beside an open panel (its divider is the pixel
    // column 785): its last 70 are the label's, whatever is under it.
    for (l, right, tab_cols) in [
        (at(1281, 726), 1281.0, 93),
        (with_panel(1281, 726), 785.0, 43),
    ] {
        assert_eq!(FILES_COLS, 7);
        assert_eq!(l.tab_bar.right(), right);
        assert_eq!(l.files_rect(), rect(right - 70.0, 0.0, 70.0, 26.0));
        for tabs in [0, 1, 2, 10, 500] {
            assert_eq!(l.hit(right - 70.1, 10.0, tabs), Hit::TabBar, "{tabs} tabs");
            assert_eq!(l.hit(right - 70.0, 0.0, tabs), Hit::Files, "{tabs} tabs");
            // (The bar's last five pixels are the divider's handle when a panel is open.)
            assert_eq!(l.hit(right - 5.1, 25.9, tabs), Hit::Files, "{tabs} tabs");
            assert_ne!(l.hit(right - 30.0, 26.0, tabs), Hit::Files, "under the bar");
        }
        // The other columns are the tabs': no tab reaches into the label.
        assert_eq!(l.tab_cols(1), TAB_MAX_COLS);
        assert_eq!(l.tab_cols(4), (tab_cols / 4).min(TAB_MAX_COLS));
        assert_eq!(l.tab_cols(tab_cols), 1);
        assert_eq!(l.tab_cols(tab_cols + 1), 0);
        for tabs in 1..120 {
            let last = l.tab_rect(tabs - 1, tabs);
            assert!(last.right() <= l.files_rect().x, "{tabs} tabs");
        }
    }
    // A bar of fewer than seven columns has no label, and its tabs keep what there is.
    let narrow = at(100, 400);
    assert_eq!(narrow.tab_bar.w, 49.0);
    assert_eq!(narrow.files_rect().w, 0.0);
    assert_eq!(narrow.tab_cols(1), 4);
    for x in 0..100 {
        assert_ne!(narrow.hit(x as f32, 5.0, 1), Hit::Files, "x {x}");
    }
    // Seven and more: it is there, at the right end.
    let enough = at(160, 400);
    assert_eq!(enough.tab_bar.w, 79.0);
    assert_eq!(enough.files_rect(), rect(90.0, 0.0, 70.0, 26.0));
    assert_eq!(enough.tab_cols(1), 0);
    assert_eq!(enough.hit(100.0, 5.0, 1), Hit::Files);
}

const LIST: SideLook = SideLook {
    refresh: true,
    back: false,
};
const FROM_LIST: SideLook = SideLook {
    refresh: false,
    back: true,
};

#[test]
fn the_lists_header_has_refresh_and_a_picture_from_the_list_a_way_back() {
    assert_eq!(side_header_with(49, SideLook::default()), side_header(49));
    // 49 columns: × on 47, a column, `Refresh` on 38-44, two columns, the title.
    let list = side_header_with(49, LIST);
    assert_eq!(
        list,
        SideHeader {
            title_col: 0,
            title_cols: 36,
            open: Some(38),
            close: Some(47),
            back: None,
        }
    );
    // The label goes when the title would have fewer than eight columns beside it.
    assert_eq!(side_header_with(21, LIST).open, Some(10));
    assert_eq!(side_header_with(20, LIST).open, None);
    // From the list: `‹` on the first column, a column, then the path.
    assert_eq!(
        side_header_with(49, FROM_LIST),
        SideHeader {
            title_col: 2,
            title_cols: 30,
            open: Some(34),
            close: Some(47),
            back: Some(0),
        }
    );
    // Narrow: the way back stays as long as the × does not need its cells.
    let narrow = side_header_with(5, FROM_LIST);
    assert_eq!(
        (narrow.back, narrow.close, narrow.title_cols),
        (Some(0), Some(3), 0)
    );
    let none = side_header_with(4, FROM_LIST);
    assert_eq!((none.back, none.close, none.title_col), (None, Some(2), 0));
    for cols in 0..80 {
        for look in [SideLook::default(), LIST, FROM_LIST] {
            let h = side_header_with(cols, look);
            let end = h.open.or(h.close).unwrap_or(cols);
            assert!(h.title_col + h.title_cols <= end, "{cols} {look:?}");
        }
    }
}

#[test]
fn clicks_hit_refresh_and_the_way_back() {
    let l = with_panel(1281, 726);
    let col = |col: usize| 791.0 + col as f32 * 10.0;
    // The list: `Refresh` where the label is; nothing opens in an app, nothing goes back.
    for (x, hit) in [
        (col(0), Hit::Side),
        (col(1), Hit::Side),
        (col(34), Hit::Side),
        (col(37) + 9.0, Hit::Side),
        (col(38), Hit::SideRefresh),
        (col(44) + 9.0, Hit::SideRefresh),
        (col(45), Hit::Side),
        (col(46), Hit::SideClose),
    ] {
        assert_eq!(l.hit_with(x, 10.0, 1, LIST), hit, "list header, x {x}");
        assert_eq!(l.hit_with(x, 60.0, 1, LIST), Hit::Side, "list body, x {x}");
    }
    // A picture from the list: `‹` and the cell after it.
    for (x, hit) in [
        (790.0, Hit::SideDivider),
        (col(0), Hit::SideBack),
        (col(1) + 9.0, Hit::SideBack),
        (col(2), Hit::Side),
        (col(34), Hit::SideOpen),
        (col(46), Hit::SideClose),
    ] {
        assert_eq!(
            l.hit_with(x, 10.0, 1, FROM_LIST),
            hit,
            "picture header, x {x}"
        );
        let body = match hit {
            Hit::SideDivider => hit,
            _ => Hit::Side,
        };
        assert_eq!(
            l.hit_with(x, 60.0, 1, FROM_LIST),
            body,
            "picture body, x {x}"
        );
    }
    // Without a look it is the header of a file.
    assert_eq!(l.hit(col(0), 10.0, 1), Hit::Side);
    assert_eq!(l.hit(col(38), 10.0, 1), Hit::SideOpen);
    // The look changes nothing outside the panel.
    for (x, y) in [(50.0, 30.0), (500.0, 300.0), (750.0, 10.0), (600.0, 710.0)] {
        assert_eq!(l.hit_with(x, y, 2, LIST), l.hit(x, y, 2));
        assert_eq!(l.hit_with(x, y, 2, FROM_LIST), l.hit(x, y, 2));
    }
}

// --- one row grid -------------------------------------------------------------------------------

/// The window and the cell of a display at `scale` with a font of `font` logical pixels: a
/// cell is about 0.6 x 1.3 of the font's size, on whole physical pixels (8 x 17 at 13 px and
/// scale 1, as the app's own font gives).
fn display_with(scale: f32, font: f32) -> ((u32, u32), CellMetrics) {
    let px = |logical: f32| (logical * scale).round().max(1.0);
    let m = CellMetrics {
        width: px(font * 0.6),
        height: px(font * 1.3),
        baseline: px(font),
        underline_y: px(font * 1.1),
        strike_y: px(font * 0.65),
        stroke: px(1.0),
    };
    ((px(1280.0) as u32, px(800.0) as u32), m)
}

const FONTS: [f32; 3] = [11.0, 13.0, 16.0];

/// Every display of the grid tests, without a panel and with one, and what it is called in
/// a failure.
fn grids() -> Vec<(String, Layout)> {
    let theme = Theme::default();
    let mut out = Vec::new();
    for scale in SCALES {
        for font in FONTS {
            let (window, m) = display_with(scale, font);
            let cols = default_panel_cols(window, scale, &m, &theme);
            assert!(cols > 0, "scale {scale} font {font}: a panel fits");
            for side in [None, Some(cols)] {
                let name = format!("scale {scale}, font {font}, panel {side:?}");
                out.push((name, layout_with(window, scale, &m, &theme, side)));
            }
        }
    }
    out
}

/// How far `y` is from a row of the terminal, in pixels.
fn off_grid(l: &Layout, y: f32) -> f32 {
    let rows = (y - l.terminal.y) / l.cell.1;
    (rows - rows.round()).abs() * l.cell.1
}

#[test]
fn every_row_of_text_is_on_a_row_of_the_terminal() {
    for (name, l) in grids() {
        // The list: its first line is the terminal's first row, and so on down.
        assert_eq!(l.list.y, l.terminal.y, "{name}: the list's first line");
        assert!(l.list_lines > 4, "{name}");
        for line in 0..l.list_lines {
            let y = l.list_lines_rect(line, 1).y;
            assert!(off_grid(&l, y) <= 0.01, "{name}: list line {line} at {y}");
        }
        // The legend's lines and the buttons' rows.
        assert!(l.legend.h > 0.0 && l.buttons.h > 0.0, "{name}: a foot");
        assert!(
            off_grid(&l, l.legend.y) <= 0.01,
            "{name}: legend at {}",
            l.legend.y
        );
        assert_eq!(
            (l.legend.h / l.cell.1).fract(),
            0.0,
            "{name}: whole legend lines"
        );
        assert!(
            off_grid(&l, l.buttons.y) <= 0.01,
            "{name}: buttons at {}",
            l.buttons.y
        );
        assert!(
            l.list.bottom() < l.legend.y,
            "{name}: the list ends above the legend"
        );
        // The panel's text: the terminal's rows, all of them.
        if l.side.w > 0.0 {
            assert_eq!(
                l.side_body.y, l.terminal.y,
                "{name}: the panel's first line"
            );
            assert_eq!(
                l.side_lines, l.grid.1 as usize,
                "{name}: as many lines as rows"
            );
            assert!(l.side_body.bottom() <= l.status.y, "{name}");
        }
        // All of it on whole pixels.
        for y in [l.terminal.y, l.list.y, l.legend.y, l.buttons.y, l.status.y] {
            assert_eq!(y.fract(), 0.0, "{name}");
        }
    }
}

#[test]
fn the_bars_are_equally_high_and_set_their_text_alike() {
    for (name, l) in grids() {
        assert_eq!(l.tab_bar.h, l.status.h, "{name}");
        assert!(
            l.tab_bar.h >= l.cell.1,
            "{name}: a bar holds a line of text"
        );
        let in_bar = l.bar_text_y(l.tab_bar) - l.tab_bar.y;
        assert_eq!(l.bar_text_y(l.status) - l.status.y, in_bar, "{name}");
        assert_eq!(in_bar.fract(), 0.0, "{name}");
        assert_eq!(l.tab_bar.bottom(), l.terminal.y, "{name}");
        if l.side.w > 0.0 {
            // The panel's header is in the tab bar's band, right of it.
            assert_eq!(l.side_header.h, l.tab_bar.h, "{name}");
            assert_eq!(l.side_header.y, l.tab_bar.y, "{name}");
            assert_eq!(
                l.bar_text_y(l.side_header),
                l.bar_text_y(l.tab_bar),
                "{name}"
            );
            assert_eq!(l.tab_bar.right(), l.side_divider.x, "{name}");
            assert_eq!(l.side_divider.right(), l.side_header.x, "{name}");
            assert_eq!(l.files_rect().right(), l.tab_bar.right(), "{name}");
        } else {
            assert_eq!(l.tab_bar.right(), l.window.0, "{name}");
        }
    }
}

#[test]
fn text_starts_on_a_whole_column_of_its_area() {
    for (name, l) in grids() {
        // The status line's text starts where the terminal's does.
        assert_eq!(l.status_text_x(), l.terminal.x, "{name}");
        let end = l.status_text_x() + l.status_cols() as f32 * l.cell.0;
        assert!(end <= l.status.right() && l.status_cols() > 20, "{name}");
        // The list's columns are from the window's edge.
        assert_eq!(l.list.x, 0.0, "{name}");
        if l.side.w > 0.0 {
            // The panel's text and its header's are on the columns from `side_body.x`: whole
            // ones up to the window's edge, half a cell right of the divider as the
            // terminal's are right of the list's line.
            assert_eq!(l.side_body.w, l.side_cols as f32 * l.cell.0, "{name}");
            assert_eq!(l.side_body.right(), l.window.0, "{name}");
            assert_eq!(
                l.side_body.x - l.side_divider.right(),
                l.terminal.x - l.separator.right(),
                "{name}: the same padding"
            );
            assert_eq!(l.side_body.x.fract(), 0.0, "{name}");
        }
    }
}
