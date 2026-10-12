//! The file panel as drawn: what its header and its rows say (spans of text on the panel's
//! columns) and the quads they become. No window; the quads need a font.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use client_core::Config;
use client_core::SessionKey;
use client_core::files::Made;
use desktop::draw::{
    BACK_MARK, Chrome, FILES, Layout, OPEN_FOLDER, OPEN_IN_APP, REFRESH, Rect, Span, View,
    default_panel_cols, keep_end, layout_with, side_header, side_header_spans,
    side_header_spans_focused, side_header_spans_with, side_header_with, side_made_spans,
    side_text_spans,
};
use desktop::sidebar::Sidebar;
use desktop::theme::{CLOSE_MARK, COPY_RGB, CURSOR_RGB, Theme};
use desktop::viewer::{
    Document, MadeBody, MadeRow, NO_CLAUDE, NO_IMAGES, SideBody, SideLook, SidePanel, SideView,
    TextBody, TextRow,
};
use s2_wgpu::gfx::atlas::{Atlas, FontSpec};
use s2_wgpu::gfx::grid::{Instance, KIND_SOLID, clusters};

const LOCAL: &str = "laptop";
const WINDOW: (u32, u32) = (1000, 520);

fn width(text: &str) -> usize {
    clusters(text).iter().map(|(_, width)| width).sum()
}

fn rows_of(text: &str, line: Option<usize>, visible: usize, cols: usize) -> (usize, Vec<TextRow>) {
    let mut body = TextBody::new(text, line);
    body.reveal(visible);
    (body.gutter_cols(), body.rows(visible, cols))
}

/// No span goes past `cols`, and no two on a line share a column.
fn assert_within(spans: &[Span<'_>], cols: usize) {
    for span in spans {
        let end = span.col + width(&span.shown());
        assert!(end <= cols, "{span:?} ends at {end} of {cols}");
        assert!(width(&span.shown()) <= span.cols, "{span:?}");
    }
    for (n, a) in spans.iter().enumerate() {
        for b in &spans[n + 1..] {
            let (a_end, b_end) = (a.col + width(&a.shown()), b.col + width(&b.shown()));
            let apart = a.line != b.line || a_end <= b.col || b_end <= a.col;
            assert!(apart, "{a:?} and {b:?} overlap");
        }
    }
}

#[test]
fn a_long_path_keeps_its_end() {
    assert_eq!(keep_end("/w/a.rs", 20), "/w/a.rs");
    assert_eq!(keep_end("/w/a.rs", 7), "/w/a.rs");
    assert_eq!(keep_end("/home/u/src/main.rs", 10), "…c/main.rs");
    assert_eq!(keep_end("/home/u/src/main.rs", 2), "…s");
    assert_eq!(keep_end("/home/u/src/main.rs", 1), "…");
    assert_eq!(keep_end("/home/u/src/main.rs", 0), "");
    // A wide character is kept whole or not at all.
    assert_eq!(keep_end("/日本語.txt", 7), "…語.txt");
    assert_eq!(keep_end("/日本語.txt", 6), "….txt");
    for cols in 0..30 {
        assert!(width(&keep_end("nas:/日本/語 x/ファイル.txt", cols)) <= cols);
    }
}

#[test]
fn the_header_says_the_path_open_in_app_and_the_close_mark() {
    let theme = Theme::default();
    let spans = side_header_spans("/w/src/main.rs", 49, &theme);
    assert_eq!(spans.len(), 4);
    assert_eq!(
        (spans[0].col, spans[0].shown().as_str()),
        (0, "/w/src/main.rs")
    );
    assert_eq!((spans[1].col, spans[1].text.as_ref()), (26, OPEN_FOLDER));
    assert_eq!((spans[1].colour, spans[1].bold), (COPY_RGB, true));
    assert_eq!((spans[2].col, spans[2].text.as_ref()), (34, OPEN_IN_APP));
    assert_eq!((spans[2].colour, spans[2].bold), (COPY_RGB, true));
    assert_eq!((spans[3].col, spans[3].text.as_ref()), (47, CLOSE_MARK));
    assert_within(&spans, 49);

    // Too long: cut at its start, two columns clear of the label.
    let long = "nas:/home/someone/projects/tmls-rs/crates/desktop/src/viewer.rs";
    let spans = side_header_spans(long, 49, &theme);
    let shown = spans[0].shown();
    assert!(
        shown.starts_with('…') && shown.ends_with("/src/viewer.rs"),
        "{shown}"
    );
    assert_eq!(width(&shown), 24);
    assert_within(&spans, 49);

    // Every width: the pieces stay apart and inside, and are where `side_header` says.
    for cols in 0..80 {
        let spans = side_header_spans(long, cols, &theme);
        assert_within(&spans, cols);
        let header = side_header(cols);
        let has = |text: &str| spans.iter().any(|span| span.text == text);
        assert_eq!(has(OPEN_IN_APP), header.open.is_some(), "{cols} columns");
        assert_eq!(has(OPEN_FOLDER), header.folder.is_some(), "{cols} columns");
        assert_eq!(has(CLOSE_MARK), header.close.is_some(), "{cols} columns");
        // `Folder` is the first to go: never without `Open in app`.
        assert!(
            header.folder.is_none() || header.open.is_some(),
            "{cols} columns"
        );
    }
    // Too narrow for both labels: `Open in app` stays.
    let narrow = side_header(30);
    assert_eq!((narrow.folder, narrow.open), (None, Some(15)));
}

#[test]
fn rows_have_their_number_right_aligned_and_the_text_after_the_gutter() {
    let theme = Theme::default();
    let text: String = (1..=120).map(|n| format!("line {n}\n")).collect();
    let (gutter, rows) = rows_of(&text, Some(100), 12, 40);
    assert_eq!(gutter, 4);
    let spans = side_text_spans(gutter, &rows, 40, &theme);
    assert_eq!(
        spans.len(),
        24,
        "a number and a text for each of the 12 rows"
    );
    // Lines 96 to 107; the 100th is the fifth.
    assert_eq!(
        (spans[0].line, spans[0].col, spans[0].text.as_ref()),
        (0, 0, " 96")
    );
    assert_eq!(
        (spans[1].line, spans[1].col, spans[1].text.as_ref()),
        (0, 4, "line 96")
    );
    assert_eq!((spans[8].line, spans[8].text.as_ref()), (4, "100"));
    assert_eq!(spans[8].colour, CURSOR_RGB, "the marked line's number");
    assert_eq!(spans[0].colour, theme.dim_fg);
    assert_eq!(spans[9].text.as_ref(), "line 100");
    assert_within(&spans, 40);
}

#[test]
fn long_wide_and_tabbed_lines_stay_in_their_columns() {
    let theme = Theme::default();
    let text = [
        "short",
        "a long line that does not fit the panel at all, not by far, no",
        "日本語のテキストは一文字で二桁を使うので半分しか入らない",
        "\tif x {\t// 日本\ttab",
        "mixed 日本 and ascii and 語 and more and more and more text",
        "",
        "e\u{301}\u{301}\u{301} marks and \u{1b}[31m an escape",
    ]
    .join("\n");
    for cols in [0, 1, 2, 3, 5, 12, 23, 40] {
        let (gutter, rows) = rows_of(&text, Some(3), 20, cols);
        let spans = side_text_spans(gutter, &rows, cols, &theme);
        assert_eq!(spans.len(), 14);
        assert_within(&spans, cols);
        for span in spans.iter().skip(1).step_by(2).filter(|_| cols > gutter) {
            assert_eq!(span.col, gutter, "every line starts after the gutter");
        }
    }
    let (gutter, rows) = rows_of(&text, Some(3), 20, 23);
    let spans = side_text_spans(gutter, &rows, 23, &theme);
    let shown: Vec<String> = spans.iter().skip(1).step_by(2).map(Span::shown).collect();
    assert_eq!(shown[0], "short");
    // 21 columns of text: cut with the mark; a wide character is never cut in half.
    assert_eq!(shown[1], "a long line that doe…");
    assert_eq!(shown[2], "日本語のテキストは一…");
    assert_eq!(width(&shown[2]), 21);
    assert_eq!(shown[3], "    if x {  // 日本 …");
    assert_eq!(shown[4], "mixed 日本 and ascii…");
    assert_eq!(shown[5], "");
    // No control character reaches a row: the escape is dropped, its text stays.
    assert!(shown[6].contains("marks and [31m"), "{:?}", shown[6]);
}

// --- quads ----------------------------------------------------------------------------------

fn sidebar() -> Sidebar {
    let cfg = Config {
        local_name: LOCAL.to_string(),
        hosts: vec![LOCAL.to_string()],
        host_colors: BTreeMap::new(),
        presets: Vec::new(),
        pins: BTreeSet::new(),
    };
    Sidebar::new(&cfg, PathBuf::from("/nonexistent/tmlsrs-test"))
}

fn atlas() -> Atlas {
    let atlas = Atlas::new(FontSpec::default(), 1.0);
    assert!(
        atlas.primary_font().is_some(),
        "these tests need at least one system font"
    );
    atlas
}

fn panel(text: &str, line: Option<usize>, l: &Layout) -> SideView {
    let document = Document {
        host: LOCAL.to_string(),
        path: "/w/src/main.rs".to_string(),
        line,
        text: text.to_string(),
    };
    let mut panel = SidePanel::text(&document, LOCAL);
    panel.reveal(l.side_lines);
    panel.view(l.side_lines, l.side_cols, l.side_body)
}

/// The layout with or without room for the panel, and the chrome's two layers for `side`.
fn built(
    open: bool,
    side: impl Fn(&Layout) -> Option<SideView>,
) -> (Vec<Instance>, Vec<Instance>, Layout) {
    let theme = Theme::default();
    let mut atlas = atlas();
    let cols = default_panel_cols(WINDOW, 1.0, &atlas.metrics(), &theme);
    let l = layout_with(WINDOW, 1.0, &atlas.metrics(), &theme, open.then_some(cols));
    let view = View {
        side: side(&l),
        ..View::default()
    };
    let mut chrome = Chrome::new();
    assert!(chrome.update(&view, &sidebar(), &l, &theme, &mut atlas));
    (chrome.base().to_vec(), chrome.top().to_vec(), l)
}

fn rgba(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

fn inside(i: &Instance, r: Rect) -> bool {
    let (x, y) = (i.pos[0] + i.size[0] / 2.0, i.pos[1] + i.size[1] / 2.0);
    r.contains(x, y)
}

fn glyphs(quads: &[Instance], r: Rect, colour: [u8; 3]) -> usize {
    let wanted = |i: &&Instance| i.kind != KIND_SOLID && i.color == rgba(colour) && inside(i, r);
    quads.iter().filter(wanted).count()
}

fn solids(quads: &[Instance], r: Rect, colour: [u8; 3]) -> Vec<Instance> {
    let wanted = |i: &&Instance| i.kind == KIND_SOLID && i.color == rgba(colour) && inside(i, r);
    quads.iter().filter(wanted).copied().collect()
}

/// Line `line` of the panel's text, all its columns.
fn row(l: &Layout, line: usize) -> Rect {
    Rect {
        y: l.side_body.y + line as f32 * l.cell.1,
        h: l.cell.1,
        ..l.side_body
    }
}

#[test]
fn the_panel_is_drawn_right_of_its_divider_and_nowhere_else() {
    let theme = Theme::default();
    let text: String = (1..=300).map(|n| format!("line {n}\n")).collect();
    let (base, top, l) = built(true, |l| Some(panel(&text, Some(150), l)));
    assert!(l.side.w > 0.0 && l.side_lines >= 9 && l.side_cols >= 30);
    // The divider, the header's band and the panel's own background.
    assert_eq!(solids(&top, l.side_divider, theme.sidebar_rule).len(), 1);
    assert_eq!(solids(&top, l.side_header, theme.bar_bg).len(), 1);
    let ground = solids(&top, l.side, theme.sidebar_bg);
    assert_eq!(ground.len(), 1);
    assert_eq!(
        (ground[0].pos, ground[0].size),
        ([l.side.x, l.side.y], [l.side.w, l.side.h])
    );
    // Nothing of it is over the terminal, the tab bar, the status line or past the window.
    for quad in &top {
        let (left, right) = (quad.pos[0], quad.pos[0] + quad.size[0]);
        let (up, down) = (quad.pos[1], quad.pos[1] + quad.size[1]);
        assert!(
            left >= l.side.x - 0.5 && right <= WINDOW.0 as f32 + 0.5,
            "{quad:?}"
        );
        assert!(up >= l.side.y - 0.5 && down <= l.status.y + 0.5, "{quad:?}");
        assert!(!inside(quad, l.terminal), "{quad:?} is over the terminal");
    }
    // The chrome under the terminal draws nothing of the panel.
    assert!(
        glyphs(&base, l.side, theme.sidebar_fg) == 0 && glyphs(&base, l.side, theme.dim_fg) == 0
    );

    // The header: the path, the label, the mark.
    let header = side_header(l.side_cols);
    let cells = |from: usize, count: usize| Rect {
        x: l.side_body.x + from as f32 * l.cell.0,
        w: count as f32 * l.cell.0,
        ..l.side_header
    };
    assert_eq!(
        glyphs(&top, cells(0, header.title_cols), theme.sidebar_fg),
        14,
        "/w/src/main.rs"
    );
    assert_eq!(
        glyphs(&top, cells(header.open.unwrap(), 11), COPY_RGB),
        9,
        "Open in app"
    );
    assert_eq!(
        glyphs(&top, cells(header.close.unwrap(), 1), theme.muted_fg),
        1
    );

    // Every row: three digits in the gutter's first three columns, the text after the fourth.
    let third = l.side_lines / 3;
    for line in 0..l.side_lines {
        let r = row(&l, line);
        let gutter = Rect {
            w: 3.0 * l.cell.0,
            ..r
        };
        let text = Rect {
            x: r.x + 4.0 * l.cell.0,
            w: r.w - 4.0 * l.cell.0,
            ..r
        };
        let number = if line == third {
            CURSOR_RGB
        } else {
            theme.dim_fg
        };
        assert_eq!(glyphs(&top, gutter, number), 3, "line {line}");
        assert_eq!(
            glyphs(&top, text, theme.sidebar_fg),
            7,
            "line {line}: `line NNN`"
        );
        let gap = Rect {
            x: r.x + 3.0 * l.cell.0,
            w: l.cell.0,
            ..r
        };
        assert_eq!(
            glyphs(&top, gap, theme.sidebar_fg) + glyphs(&top, gap, theme.dim_fg),
            0
        );
    }
    // Line 150 is a third down, on a band from the divider to the window's edge.
    let bands = solids(&top, l.side, theme.row_selected_bg);
    assert_eq!(bands.len(), 1);
    assert_eq!(bands[0].pos, [l.side_header.x, row(&l, third).y]);
    assert_eq!(bands[0].size, [l.side_header.w, l.cell.1]);
}

#[test]
fn wide_characters_and_tabs_do_not_move_the_columns() {
    let theme = Theme::default();
    let text = "abcdef\n\tx\n日本語\n日\tx\n";
    let (_, top, l) = built(true, |l| Some(panel(text, None, l)));
    // Four lines: the gutter is one digit and a space.
    let cell = |line: usize, col: usize, count: usize| {
        let r = row(&l, line);
        Rect {
            x: r.x + (2 + col) as f32 * l.cell.0,
            w: count as f32 * l.cell.0,
            ..r
        }
    };
    assert_eq!(glyphs(&top, cell(0, 0, 6), theme.sidebar_fg), 6);
    // The tab: nothing on four columns, then the x on the fifth.
    assert_eq!(glyphs(&top, cell(1, 0, 4), theme.sidebar_fg), 0);
    assert_eq!(glyphs(&top, cell(1, 4, 1), theme.sidebar_fg), 1);
    // Three wide characters on six columns, each on its own two.
    for n in 0..3 {
        assert_eq!(
            glyphs(&top, cell(2, 2 * n, 2), theme.sidebar_fg),
            1,
            "character {n}"
        );
    }
    assert_eq!(glyphs(&top, cell(2, 6, 20), theme.sidebar_fg), 0);
    // A wide character, the tab to column 4, the x.
    assert_eq!(glyphs(&top, cell(3, 0, 2), theme.sidebar_fg), 1);
    assert_eq!(glyphs(&top, cell(3, 2, 2), theme.sidebar_fg), 0);
    assert_eq!(glyphs(&top, cell(3, 4, 1), theme.sidebar_fg), 1);
    assert!(
        solids(&top, l.side, theme.row_selected_bg).is_empty(),
        "no line was asked for"
    );
}

#[test]
fn no_panel_no_quads() {
    // Nothing open.
    let (_, top, l) = built(false, |_| None);
    assert!(top.is_empty() && l.side.w == 0.0);
    // A layout with room for one, and none to show.
    let (_, top, _) = built(true, |_| None);
    assert!(top.is_empty());
    // A panel and a window too narrow for it: nothing is drawn over the terminal.
    let theme = Theme::default();
    let mut atlas = atlas();
    let l = layout_with((40, 200), 1.0, &atlas.metrics(), &theme, Some(40));
    assert_eq!(l.side.w, 0.0);
    let view = View {
        side: Some(SideView {
            title: "/x".to_string(),
            look: SideLook::default(),
            body: SideBody::Text {
                gutter: 2,
                rows: Vec::new(),
            },
        }),
        ..View::default()
    };
    let mut chrome = Chrome::new();
    chrome.update(&view, &sidebar(), &l, &theme, &mut atlas);
    assert!(chrome.top().is_empty());
}

#[test]
fn a_pictures_header_says_its_size_and_how_large_it_is_drawn() {
    let theme = Theme::default();
    // 1280 x 800 in a body of 550 x 400 pixels: as wide as the body, 43 % of its size.
    let body = Rect {
        x: 700.0,
        y: 20.0,
        w: 550.0,
        h: 400.0,
    };
    // A photo of 6000 x 4000 pixels kept as 4096 x 2731: the header says the file's size,
    // and how large a pixel of the file is drawn (550 for 6000).
    let photo = SidePanel::image(LOCAL, "/w/p.jpg", (4096, 2731), (6000, 4000), LOCAL);
    assert_eq!(photo.view(20, 60, body).title, "/w/p.jpg 6000×4000 · 9 %");
    let panel = SidePanel::image(
        LOCAL,
        "/w/shots/screen.png",
        (1280, 800),
        (1280, 800),
        LOCAL,
    );
    let view = panel.view(20, 60, body);
    assert_eq!(view.title, "/w/shots/screen.png 1280×800 · 43 %");
    let spans = side_header_spans(&view.title, 60, &theme);
    assert_eq!(spans[0].shown(), "/w/shots/screen.png 1280×800 · 43 %");
    assert_within(&spans, 60);
    // A narrow panel cuts the path, not what is said of the picture.
    let spans = side_header_spans(&view.title, 34, &theme);
    let shown = spans[0].shown();
    assert!(
        shown.starts_with('…') && shown.ends_with(" 1280×800 · 43 %"),
        "{shown}"
    );
    assert_within(&spans, 34);

    // Drawn: the header's text and the body's background, and no row of text. The picture
    // itself is not the chrome's (`picture::PictureRenderer` draws it over the body).
    let (_, top, l) = built(true, |l| {
        Some(panel.view(l.side_lines, l.side_cols, l.side_body))
    });
    assert!(glyphs(&top, l.side_header, theme.sidebar_fg) > 10);
    assert_eq!(glyphs(&top, l.side_body, theme.sidebar_fg), 0);
    assert_eq!(glyphs(&top, l.side_body, theme.dim_fg), 0);
    assert_eq!(solids(&top, l.side, theme.sidebar_bg).len(), 1);
    assert!(top.iter().all(|quad| inside(quad, l.side)));
}

// --- the images a session made ---------------------------------------------------------------

const LIST: SideLook = SideLook {
    refresh: true,
    back: false,
};
const FROM_LIST: SideLook = SideLook {
    refresh: false,
    back: true,
};

fn work() -> SessionKey {
    SessionKey {
        host: LOCAL.to_string(),
        name: "work".to_string(),
    }
}

fn made(count: usize) -> MadeBody {
    let files = (0..count)
        .map(|n| Made {
            path: format!("/w/plots/fig-{n}.png"),
            modified_s: 1_760_000_000 - 86_400 * n as u64,
            bytes: 1,
        })
        .collect();
    MadeBody::listed(work(), files)
}

fn made_row(when: &str, name: &str, folder: &str, selected: bool) -> MadeRow {
    MadeRow {
        when: when.to_string(),
        name: name.to_string(),
        folder: folder.to_string(),
        selected,
    }
}

#[test]
fn the_lists_header_says_whose_images_and_has_refresh_in_place_of_open_in_app() {
    let theme = Theme::default();
    let spans = side_header_spans_with("Images · work · 3", 49, LIST, &theme);
    assert_eq!(spans.len(), 3);
    assert_eq!(
        (spans[0].col, spans[0].shown().as_str()),
        (0, "Images · work · 3")
    );
    assert_eq!((spans[1].col, spans[1].text.as_ref()), (38, REFRESH));
    assert_eq!((spans[1].colour, spans[1].bold), (COPY_RGB, true));
    assert_eq!((spans[2].col, spans[2].text.as_ref()), (47, CLOSE_MARK));
    assert!(
        spans
            .iter()
            .all(|span| span.text != OPEN_IN_APP && span.text != OPEN_FOLDER)
    );
    // A picture opened from the list: the way back first, then the path, and `Open in app`.
    let title = "/w/plots/fig-0.png 400×300 · 80 %";
    let spans = side_header_spans_with(title, 49, FROM_LIST, &theme);
    assert_eq!(spans.len(), 5);
    assert_eq!((spans[0].col, spans[0].text.as_ref()), (0, BACK_MARK));
    assert_eq!((spans[0].colour, spans[0].bold), (COPY_RGB, true));
    assert_eq!(spans[1].col, 2);
    let shown = spans[1].shown();
    assert!(
        shown.starts_with('…') && shown.ends_with("400×300 · 80 %"),
        "{shown}"
    );
    assert_eq!(width(&shown), 22);
    assert_eq!((spans[2].col, spans[2].text.as_ref()), (26, OPEN_FOLDER));
    assert_eq!((spans[3].col, spans[3].text.as_ref()), (34, OPEN_IN_APP));
    assert_eq!((spans[4].col, spans[4].text.as_ref()), (47, CLOSE_MARK));
    // Without a look it is the header of a file, as before.
    assert_eq!(
        side_header_spans(title, 49, &theme),
        side_header_spans_with(title, 49, SideLook::default(), &theme)
    );
    // Every width: the pieces stay apart and inside, and are where `side_header_with` says.
    for look in [LIST, FROM_LIST] {
        for cols in 0..80 {
            let spans = side_header_spans_with(title, cols, look, &theme);
            assert_within(&spans, cols);
            let header = side_header_with(cols, look);
            let has = |text: &str| spans.iter().any(|span| span.text == text);
            let label = if look.refresh { REFRESH } else { OPEN_IN_APP };
            assert_eq!(has(label), header.open.is_some(), "{cols} columns");
            assert_eq!(has(OPEN_FOLDER), header.folder.is_some(), "{cols} columns");
            assert!(
                !look.refresh || header.folder.is_none(),
                "a list has no folder"
            );
            assert_eq!(has(CLOSE_MARK), header.close.is_some(), "{cols} columns");
            assert_eq!(has(BACK_MARK), header.back.is_some(), "{cols} columns");
        }
    }
}

#[test]
fn a_row_of_the_list_is_the_time_the_name_and_the_folder() {
    let theme = Theme::default();
    let rows = [
        made_row("14:05", "fig-0.png", "/w/plots", true),
        made_row(
            "Oct 10",
            "a-rather-long-name-for-a-figure.png",
            "…/out",
            false,
        ),
        made_row("Jan 3", "x.jpg", "", false),
    ];
    let spans = side_made_spans(&rows, None, 49, &theme);
    let on = |line: usize| -> Vec<(usize, String, [u8; 3], bool)> {
        let of_line = spans.iter().filter(|span| span.line == line);
        of_line
            .map(|span| (span.col, span.shown(), span.colour, span.bold))
            .collect()
    };
    // The cursor's row: the name in the accent colour.
    assert_eq!(
        on(0),
        [
            (0, "14:05".to_string(), theme.dim_fg, false),
            (7, "fig-0.png".to_string(), CURSOR_RGB, true),
            (18, "/w/plots".to_string(), theme.dim_fg, false),
        ]
    );
    assert_eq!(
        on(1),
        [
            (0, "Oct 10".to_string(), theme.dim_fg, false),
            (
                7,
                "a-rather-long-name-for-a-figure.png".to_string(),
                theme.sidebar_fg,
                false
            ),
            (44, "…/out".to_string(), theme.dim_fg, false),
        ]
    );
    // No folder: no span for one.
    assert_eq!(on(2).len(), 2);
    assert_within(&spans, 49);
    // Every width, with the rows as the list makes them for it.
    let body = made(6);
    for cols in 0..80 {
        let rows = body.rows(4, cols, 1_760_000_000);
        let spans = side_made_spans(&rows, None, cols, &theme);
        assert_within(&spans, cols);
        if cols >= 30 {
            assert_eq!(spans.len(), 12, "{cols} columns: time, name and folder");
        }
    }
    // What is no list is one line, dimmed, cut where the panel ends.
    for line in [NO_IMAGES, NO_CLAUDE] {
        let spans = side_made_spans(&[], Some(line), 49, &theme);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            (spans[0].line, spans[0].col, spans[0].shown().as_str()),
            (0, 0, line)
        );
        assert_eq!(spans[0].colour, theme.muted_fg);
        for cols in 0..40 {
            assert_within(&side_made_spans(&[], Some(line), cols, &theme), cols);
        }
    }
}

#[test]
fn the_list_is_drawn_with_a_band_under_the_cursors_row() {
    let theme = Theme::default();
    let mut body = made(40);
    body.move_by(2, 10);
    let panel = SidePanel::made(body);
    let (base, top, l) = built(true, |l| {
        Some(panel.view_at(l.side_lines, l.side_cols, l.side_body, 1_760_000_000))
    });
    assert!(l.side_lines >= 9 && l.side_cols >= 30);
    // The header: its text, and `Refresh` where the hit is.
    assert!(glyphs(&top, l.side_header, theme.sidebar_fg) >= 10);
    let header = side_header_with(l.side_cols, LIST);
    let label = Rect {
        x: l.side_body.x + header.open.unwrap() as f32 * l.cell.0,
        w: REFRESH.len() as f32 * l.cell.0,
        ..l.side_header
    };
    assert_eq!(glyphs(&top, label, COPY_RGB), REFRESH.len());
    assert_eq!(glyphs(&top, l.side_header, COPY_RGB), REFRESH.len());
    // One band, on the third row; every row in view has its time, name and folder.
    let bands = solids(&top, l.side_body, theme.row_selected_bg);
    assert_eq!(bands.len(), 1);
    assert_eq!(bands[0].pos[1], row(&l, 2).y);
    assert_eq!(bands[0].size[1], l.cell.1);
    for line in 0..l.side_lines {
        let name = if line == 2 {
            CURSOR_RGB
        } else {
            theme.sidebar_fg
        };
        assert!(glyphs(&top, row(&l, line), name) >= 9, "row {line}");
        assert!(glyphs(&top, row(&l, line), theme.dim_fg) >= 5, "row {line}");
    }
    assert!(top.iter().all(|quad| inside(quad, l.side)));
    // `Files` at the tab bar's right end, lit while the panel is open.
    assert_eq!(
        glyphs(&base, l.files_rect(), theme.tab_active_fg),
        FILES.len()
    );

    // Closed: the label is there, dimmed, and nothing else of the panel.
    let (base, top, l) = built(false, |_| None);
    assert_eq!(glyphs(&base, l.files_rect(), theme.muted_fg), FILES.len());
    assert_eq!(glyphs(&base, l.files_rect(), theme.tab_active_fg), 0);
    assert!(top.is_empty());
}

#[test]
fn a_list_with_nothing_in_it_draws_its_one_line() {
    let theme = Theme::default();
    for (body, line) in [
        (made(0), NO_IMAGES),
        (MadeBody::no_claude(work()), NO_CLAUDE),
    ] {
        let panel = SidePanel::made(body);
        let (_, top, l) = built(true, |l| {
            Some(panel.view(l.side_lines, l.side_cols, l.side_body))
        });
        let words = line.chars().filter(|c| *c != ' ').count();
        assert_eq!(glyphs(&top, row(&l, 0), theme.muted_fg), words, "{line}");
        assert_eq!(glyphs(&top, l.side_body, theme.sidebar_fg), 0);
        assert!(solids(&top, l.side_body, theme.row_selected_bg).is_empty());
    }
}

#[test]
fn the_divider_shows_that_the_panel_has_the_keys() {
    let theme = Theme::default();
    let panel = SidePanel::made(made(3));
    for (focused, colour) in [(false, theme.sidebar_rule), (true, theme.focus_ring)] {
        let mut atlas = atlas();
        let cols = default_panel_cols(WINDOW, 1.0, &atlas.metrics(), &theme);
        let l = layout_with(WINDOW, 1.0, &atlas.metrics(), &theme, Some(cols));
        let view = View {
            side: Some(panel.view(l.side_lines, l.side_cols, l.side_body)),
            side_focused: focused,
            ..View::default()
        };
        let mut chrome = Chrome::new();
        assert!(chrome.update(&view, &sidebar(), &l, &theme, &mut atlas));
        assert_eq!(
            solids(chrome.top(), l.side_divider, colour).len(),
            1,
            "{focused}"
        );
        // And the header's title: in the accent colour with the keys, plain without.
        let title = "Images·work·3".chars().count();
        let (lit, plain) = if focused { (title, 0) } else { (0, title) };
        let top = chrome.top();
        assert_eq!(glyphs(top, l.side_header, CURSOR_RGB), lit, "{focused}");
        assert_eq!(glyphs(top, l.side_header, theme.sidebar_fg), plain);
    }
}

#[test]
fn the_headers_title_is_in_the_accent_colour_while_the_panel_has_the_keys() {
    let theme = Theme::default();
    let title = "Images · work · 3";
    for look in [LIST, FROM_LIST] {
        let plain = side_header_spans_with(title, 49, look, &theme);
        assert_eq!(
            plain,
            side_header_spans_focused(title, 49, look, false, &theme)
        );
        let focused = side_header_spans_focused(title, 49, look, true, &theme);
        assert_eq!(plain.len(), focused.len());
        let mut changed = 0;
        for (a, b) in plain.iter().zip(&focused) {
            // Nothing moves: the same text on the same columns.
            assert_eq!((a.col, a.cols, &a.text), (b.col, b.cols, &b.text));
            if a.text.starts_with("Images") {
                assert_eq!((a.colour, a.bold), (theme.sidebar_fg, false));
                assert_eq!((b.colour, b.bold), (CURSOR_RGB, true));
                changed += 1;
            } else {
                assert_eq!((a.colour, a.bold), (b.colour, b.bold), "{a:?}");
            }
        }
        assert_eq!(changed, 1);
    }
}
