//! The session list as drawn: what a row says (spans of text on the list's columns) and the
//! quads it becomes, plus one picture rendered without a window.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use client_core::config::tmls_slug;
use client_core::{Config, HostColor, HostSnapshot, SessionInfo, SessionKey, Status, Tool};
use desktop::draw::{
    Chrome, Layout, Rect, RowLook, Span, TabView, View, header_spans, layer_order, layout,
    session_spans,
};
use desktop::sidebar::{Row, Sidebar};
use desktop::theme::{self, SIDEBAR_COLS, Theme, host_rgb, mark_rgb};
use s2_wgpu::gfx::atlas::{Atlas, FontSpec};
use s2_wgpu::gfx::grid::{Instance, KIND_SOLID, clusters};
use s2_wgpu::gfx::render::{Gpu, OFFSCREEN_FORMAT, Renderer, render_offscreen};

const LOCAL: &str = "laptop";
const WINDOW: (u32, u32) = (900, 520);
/// Where the picture of the list goes, to be looked at beside tmls's.
const PICTURE: &str = "/data/scratch/tmls-rs-v0/sidebar.png";

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey {
        host: host.to_string(),
        name: name.to_string(),
    }
}

fn session(host: &str, name: &str, status: Status) -> SessionInfo {
    SessionInfo {
        key: key(host, name),
        status,
        reason: None,
        tool: Tool::Other,
        title: None,
        context_pct: None,
        attached: 0,
        last_output_age_s: None,
    }
}

fn claude(host: &str, name: &str, title: &str, pct: u8) -> SessionInfo {
    SessionInfo {
        tool: Tool::Claude,
        title: Some(title.to_string()),
        context_pct: Some(pct),
        ..session(host, name, Status::Running)
    }
}

fn snapshot(host: &str, sessions: Vec<SessionInfo>) -> HostSnapshot {
    HostSnapshot {
        host: host.to_string(),
        online: true,
        error: None,
        sessions,
    }
}

fn sidebar(hosts: &[&str]) -> Sidebar {
    sidebar_with_pins(hosts, &[])
}

/// A list in which `pinned` are pinned from the start (nothing is written for that).
fn sidebar_with_pins(hosts: &[&str], pinned: &[SessionKey]) -> Sidebar {
    let cfg = Config {
        local_name: LOCAL.to_string(),
        hosts: hosts.iter().map(|host| host.to_string()).collect(),
        host_colors: BTreeMap::from([
            (LOCAL.to_string(), HostColor::Teal),
            ("nas".to_string(), HostColor::Rose),
        ]),
        presets: Vec::new(),
        pins: pinned
            .iter()
            .map(|key| tmls_slug(key, LOCAL))
            .collect::<BTreeSet<_>>(),
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

fn tab(key: &SessionKey) -> TabView {
    TabView {
        key: key.clone(),
        mark: None,
        host: [1, 2, 3],
        ended: false,
    }
}

fn view(tabs: &[&SessionKey], active: Option<usize>, sidebar_focused: bool) -> View {
    View {
        sidebar_focused,
        tabs: tabs.iter().map(|key| tab(key)).collect(),
        active,
        status: String::new(),
        notice: None,
        panel: None,
        unread: 0,
    }
}

/// The quads of the window's chrome for this list.
fn built(view: &View, list: &Sidebar, theme: &Theme) -> (Vec<Instance>, Layout, Atlas) {
    let mut atlas = atlas();
    let l = layout(WINDOW, 1.0, &atlas.metrics(), theme);
    let mut chrome = Chrome::new();
    assert!(
        chrome.update(view, list, &l, theme, &mut atlas),
        "the first update builds"
    );
    (chrome.base().to_vec(), l, atlas)
}

/// The rectangle of a row of the list, when nothing is scrolled.
fn row_rect(list: &Sidebar, l: &Layout, row: usize) -> Rect {
    let line: usize = list.rows()[..row].iter().map(Row::lines).sum();
    l.list_lines_rect(line, list.rows()[row].lines())
}

fn rgba(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

fn inside(i: &Instance, r: Rect) -> bool {
    let (x, y) = (i.pos[0] + i.size[0] / 2.0, i.pos[1] + i.size[1] / 2.0);
    r.contains(x, y)
}

/// The glyph quads of a colour whose centre is in `r`.
fn glyphs(quads: &[Instance], r: Rect, colour: [u8; 3]) -> Vec<Instance> {
    let wanted = |i: &&Instance| i.kind != KIND_SOLID && i.color == rgba(colour) && inside(i, r);
    quads.iter().filter(wanted).copied().collect()
}

/// The rectangles of a colour that lie in `r`.
fn solids(quads: &[Instance], r: Rect, colour: [u8; 3]) -> Vec<Instance> {
    let wanted = |i: &&Instance| i.kind == KIND_SOLID && i.color == rgba(colour) && inside(i, r);
    quads.iter().filter(wanted).copied().collect()
}

/// Columns `r` spans from column `from`, `count` of them.
fn columns(r: Rect, l: &Layout, from: usize, count: usize) -> Rect {
    Rect {
        x: r.x + from as f32 * l.cell.0,
        w: count as f32 * l.cell.0,
        ..r
    }
}

fn width(text: &str) -> usize {
    clusters(text).iter().map(|(_, width)| width).sum()
}

fn spans(info: &SessionInfo, look: RowLook) -> Vec<Span<'_>> {
    session_spans(info, look, SIDEBAR_COLS, &Theme::default())
}

fn find<'a, 'b>(spans: &'b [Span<'a>], text: &str) -> &'b Span<'a> {
    spans
        .iter()
        .find(|span| span.text == text)
        .unwrap_or_else(|| panic!("no span {text:?} in {spans:?}"))
}

/// Every span stays on the list's 26 columns of text: column 1 to 26 of the 28.
fn assert_within(spans: &[Span<'_>]) {
    for span in spans {
        let end = span.col + width(&span.shown());
        assert!(
            span.col >= 1 && end < SIDEBAR_COLS,
            "{span:?} ends at {end}"
        );
    }
    // And no two on a line overlap.
    for (n, a) in spans.iter().enumerate() {
        for b in &spans[n + 1..] {
            let (a_end, b_end) = (a.col + width(&a.shown()), b.col + width(&b.shown()));
            let apart = a.line != b.line || a_end <= b.col || b_end <= a.col;
            assert!(apart, "{a:?} and {b:?} overlap");
        }
    }
}

#[test]
fn mark_colour_per_status() {
    let statuses = [
        (Status::Running, "●", [0x4e, 0xbf, 0x71]),
        (Status::Waiting, "?", [0xe5, 0xc0, 0x7b]),
        (Status::Unseen, "◆", [0xff, 0x8c, 0x00]),
        (Status::Error, "✕", [0xe0, 0x6c, 0x75]),
        (Status::Idle, "○", Theme::default().dim_fg),
    ];
    let mut list = sidebar(&[LOCAL]);
    let sessions = statuses
        .iter()
        .map(|(status, glyph, _)| session(LOCAL, &format!("s{glyph}"), *status))
        .collect();
    list.apply(snapshot(LOCAL, sessions));
    let theme = Theme::default();
    // No tab is open and the list is not focused: the rows are plain.
    let (quads, l, _atlas) = built(&view(&[], None, false), &list, &theme);

    for (n, (status, glyph, colour)) in statuses.iter().enumerate() {
        assert_eq!(mark_rgb(*status), *colour, "tmls's colour of {glyph}");
        assert_eq!(theme::mark_glyph(*status), *glyph);
        let info = session(LOCAL, "x", *status);
        let said = spans(&info, RowLook::default());
        assert_eq!(
            (said[0].text.as_ref(), said[0].col, said[0].line),
            (*glyph, 1, 0)
        );
        assert_eq!(said[0].colour, *colour);
        assert_eq!(find(&said, "x").col, 5, "mark, tool letter, name");

        // The quads: the mark's cell has ink of the mark's colour and of no other mark's.
        let cell = columns(row_rect(&list, &l, n + 1), &l, 1, 1);
        assert!(
            !glyphs(&quads, cell, *colour).is_empty(),
            "{glyph} is drawn in its colour"
        );
        for (_, other, other_colour) in statuses.iter().filter(|(_, _, c)| c != colour) {
            assert!(
                glyphs(&quads, cell, *other_colour).is_empty(),
                "{glyph} in the colour of {other}"
            );
        }
    }

    // The tool's letter, in tmls's colour for it, between mark and name.
    for (tool, letter, colour) in [
        (Tool::Claude, "C", [0xd9, 0x77, 0x57]),
        (Tool::Opencode, "O", [0x61, 0xaf, 0xef]),
        (Tool::Codex, "X", [0x98, 0xc3, 0x79]),
    ] {
        let info = SessionInfo {
            tool,
            ..session(LOCAL, "name", Status::Idle)
        };
        let said = spans(&info, RowLook::default());
        let span = find(&said, letter);
        assert_eq!((span.col, span.colour), (3, colour));
    }
    let plain = session(LOCAL, "name", Status::Idle);
    assert_eq!(
        spans(&plain, RowLook::default()).len(),
        2,
        "a shell: mark and name"
    );
}

#[test]
fn name_is_cut_with_an_ellipsis_at_28_columns() {
    // 26 columns of text; mark, letter and their spaces take 4: 22 for a name.
    let fits = session(LOCAL, "a-name-of-22-columns-xy", Status::Idle);
    assert_eq!(width(&fits.key.name), 23);
    let said = spans(&fits, RowLook::default());
    let name = said.last().unwrap();
    assert_eq!(name.shown(), "a-name-of-22-columns-…");
    assert_eq!(width(&name.shown()), 22);
    assert_within(&said);
    let exact = session(LOCAL, "a-name-of-22-columns-x", Status::Idle);
    let said = spans(&exact, RowLook::default());
    assert_eq!(
        said.last().unwrap().shown(),
        "a-name-of-22-columns-x",
        "what fits is not cut"
    );

    // The ★ survives a long name, one column after it.
    let pinned = RowLook {
        pinned: true,
        ..RowLook::default()
    };
    let said = spans(&fits, pinned);
    let (name, star) = (find(&said, &fits.key.name), find(&said, "★"));
    assert_eq!(name.shown(), "a-name-of-22-column…");
    assert_eq!((star.col, star.colour), (26, [0xe5, 0xc0, 0x7b]));
    assert_within(&said);
    let short = session(LOCAL, "ab", Status::Idle);
    assert_eq!(
        find(&spans(&short, pinned), "★").col,
        8,
        "right after a short name"
    );

    // Wide characters take two columns each and are never cut in half.
    let wide = session(LOCAL, "会議の記録を全部ここに書いておく", Status::Idle);
    assert_eq!(width(&wide.key.name), 32);
    let said = spans(&wide, RowLook::default());
    assert_eq!(said.last().unwrap().shown(), "会議の記録を全部ここ…");
    assert_within(&said);
    let said = spans(&wide, pinned);
    assert_eq!(find(&said, &wide.key.name).shown(), "会議の記録を全部こ…");
    assert_within(&said);

    // A title, the context and other clients: everything stays inside, on its own line.
    let mut busy = claude(
        LOCAL,
        "a-name-of-22-columns-xy",
        "A title much longer than the list is wide",
        100,
    );
    busy.attached = 12;
    let look = RowLook {
        pinned: true,
        others: 12,
        ..RowLook::default()
    };
    let said = spans(&busy, look);
    assert_within(&said);
    let title = find(&said, "A title much longer than the list is wide");
    assert_eq!((title.line, title.col), (1, 5));
    assert_eq!(
        title.shown(),
        "A title much longer than the list is wide"[..16].to_string() + "…"
    );
    let (pct, others) = (find(&said, "100%"), find(&said, "◉12"));
    assert_eq!((pct.line, pct.col), (1, 23));
    assert_eq!((others.line, others.col), (0, 24));
    assert_eq!(find(&said, &busy.key.name).shown(), "a-name-of-22-co…");

    // The quads of a list full of long names stay left of the separator.
    let mut list = sidebar_with_pins(&[LOCAL], std::slice::from_ref(&wide.key));
    list.apply(snapshot(
        LOCAL,
        vec![fits.clone(), wide.clone(), busy.clone()],
    ));
    assert!(list.is_pinned(&wide.key));
    let theme = Theme::default();
    let (quads, l, _atlas) = built(&view(&[], None, true), &list, &theme);
    let text: Vec<&Instance> = quads
        .iter()
        .filter(|i| i.kind != KIND_SOLID && i.pos[0] < l.sidebar.right())
        .collect();
    assert!(
        text.len() > 60,
        "the names are drawn: {} glyphs",
        text.len()
    );
    for glyph in text {
        assert!(
            glyph.pos[0] + glyph.size[0] <= l.sidebar.right(),
            "{glyph:?} leaves the list"
        );
        assert!(glyph.pos[0] >= 0.0);
    }
}

#[test]
fn context_colour_thresholds() {
    let theme = Theme::default();
    let orange = [0xff, 0x8c, 0x00];
    let red = [0xe0, 0x6c, 0x75];
    for (pct, colour) in [
        (0, theme.dim_fg),
        (58, theme.dim_fg),
        (69, theme.dim_fg),
        (70, orange),
        (89, orange),
        (90, red),
        (100, red),
    ] {
        assert_eq!(theme::context_rgb(pct, &theme), colour, "{pct}%");
        // Beside the title when there is one, else at the end of the name's line.
        let titled = claude(LOCAL, "agent", "Fix the flaky test", pct);
        let said = spans(&titled, RowLook::default());
        let span = find(&said, &format!("{pct}%"));
        assert_eq!((span.line, span.colour), (1, colour), "{pct}%");
        assert_eq!(span.col + width(&span.text), 27, "right-aligned");
        let title = find(&said, "Fix the flaky test");
        assert_eq!((title.line, title.col, title.colour), (1, 5, theme.dim_fg));

        let untitled = SessionInfo {
            title: None,
            ..titled.clone()
        };
        let said = spans(&untitled, RowLook::default());
        let span = find(&said, &format!("{pct}%"));
        assert_eq!((span.line, span.colour), (0, colour));
        assert_eq!(span.col + width(&span.text), 27);
        assert!(said.iter().all(|span| span.line == 0));
    }
    // No reply yet: no number.
    let fresh = SessionInfo {
        context_pct: None,
        ..claude(LOCAL, "agent", "Fix the flaky test", 0)
    };
    assert!(
        spans(&fresh, RowLook::default())
            .iter()
            .all(|span| !span.text.ends_with('%'))
    );

    // What Claude waits for replaces the title, in the colour of the ? mark.
    let waiting = SessionInfo {
        status: Status::Waiting,
        reason: Some("permission prompt".to_string()),
        ..claude(LOCAL, "agent", "Fix the flaky test", 71)
    };
    let said = spans(&waiting, RowLook::default());
    let reason = find(&said, "permission prompt");
    assert_eq!((reason.line, reason.colour), (1, mark_rgb(Status::Waiting)));
    assert!(said.iter().all(|span| span.text != "Fix the flaky test"));

    // On screen: the number's cells have ink of the threshold's colour.
    let mut list = sidebar(&[LOCAL]);
    list.apply(snapshot(
        LOCAL,
        vec![
            claude(LOCAL, "calm", "t", 12),
            claude(LOCAL, "filling", "t", 75),
            claude(LOCAL, "full", "t", 95),
        ],
    ));
    let (quads, l, _atlas) = built(&view(&[], None, false), &list, &theme);
    for (row, colour, others) in [(2, orange, red), (3, red, orange)] {
        let r = row_rect(&list, &l, row);
        let second = Rect {
            y: r.y + l.cell.1,
            h: l.cell.1,
            ..r
        };
        let number = columns(second, &l, 24, 3);
        assert_eq!(glyphs(&quads, number, colour).len(), 3, "row {row}");
        assert!(glyphs(&quads, r, others).is_empty(), "row {row}");
    }
}

#[test]
fn header_colour_is_the_host_colour() {
    let theme = Theme::default();
    let mut list = sidebar(&[LOCAL, "nas", "pc"]);
    list.apply(snapshot(LOCAL, vec![session(LOCAL, "work", Status::Idle)]));
    list.apply(HostSnapshot {
        host: "nas".to_string(),
        online: false,
        error: None,
        sessions: Vec::new(),
    });
    let (quads, l, _atlas) = built(&view(&[], None, false), &list, &theme);
    // [laptop] work - [nas offline] - [pc]
    for (row, colour) in [
        (0, HostColor::Teal),
        (3, HostColor::Rose),
        (5, client_core::default_color("pc")),
    ] {
        let r = row_rect(&list, &l, row);
        let rule = solids(&quads, r, host_rgb(colour));
        assert_eq!(rule.len(), 1, "row {row}: one line in the host's colour");
        assert_eq!(
            (rule[0].pos, rule[0].size[0]),
            ([0.0, r.y], l.sidebar.w),
            "full width, on top"
        );
        assert!(rule[0].size[1] >= 1.0 && rule[0].size[1] < l.cell.1 / 4.0);
    }
    // Nothing of a host's colour anywhere else in the list.
    let work = row_rect(&list, &l, 1);
    assert!(solids(&quads, work, host_rgb(HostColor::Teal)).is_empty());

    // What a header says: the name as tmls writes it, and `+` at the right end.
    let said = header_spans(LOCAL, true, SIDEBAR_COLS, &theme);
    assert_eq!(said[0].text, "LAPTOP");
    assert_eq!(
        (said[0].col, said[0].colour, said[0].bold),
        (1, theme.muted_fg, true)
    );
    let plus = find(&said, "+");
    assert_eq!((plus.col, plus.colour), (26, [0x4e, 0xbf, 0x71]));
    assert_within(&said);
    // Offline: dimmed, says so, and no `+`.
    let said = header_spans("nas", false, SIDEBAR_COLS, &theme);
    assert_eq!(said[0].text, "NAS");
    assert_eq!(find(&said, "· offline").col, 5);
    assert!(
        said.iter()
            .all(|span| span.colour == theme.dim_fg && span.text != "+")
    );
    let long = header_spans(
        "a-host-with-a-very-long-name.example",
        false,
        SIDEBAR_COLS,
        &theme,
    );
    assert_eq!(long[0].shown(), "A-HOST-WITH-A-V…");
    assert_within(&long);
    let long = header_spans(
        "a-host-with-a-very-long-name.example",
        true,
        SIDEBAR_COLS,
        &theme,
    );
    assert_eq!(long[0].shown(), "A-HOST-WITH-A-VERY-LONG…");
    assert_within(&long);

    // On screen the `+` is green on the online hosts' headers only.
    let plus_at = |row| columns(row_rect(&list, &l, row), &l, 26, 1);
    assert_eq!(glyphs(&quads, plus_at(0), theme::ADD_RGB).len(), 1);
    assert_eq!(glyphs(&quads, plus_at(5), theme::ADD_RGB).len(), 1);
    assert!(glyphs(&quads, plus_at(3), theme::ADD_RGB).is_empty());
}

#[test]
fn selected_and_active_tab_rows_differ() {
    let theme = Theme::default();
    let names = ["cursor", "front", "plain", "both"];
    // The quads with the cursor on one session and another's tab in front.
    let looks = |selected: &str, active: &str, focused: bool| {
        let mut list = sidebar(&[LOCAL]);
        let sessions = names
            .iter()
            .map(|name| session(LOCAL, name, Status::Running))
            .collect();
        list.apply(snapshot(LOCAL, sessions));
        list.select_key(&key(LOCAL, selected));
        let active = key(LOCAL, active);
        let (quads, l, _atlas) = built(&view(&[&active], Some(0), focused), &list, &theme);
        let rows = [1, 2, 3, 4].map(|row| row_rect(&list, &l, row));
        (quads, l, rows)
    };
    // What marks a row: the bar of the cursor, the background and name colour of the tab in front.
    let count = |quads: &[Instance], r: Rect, colour| solids(quads, r, colour).len();

    let (quads, l, [cursor, front, plain, _]) = looks("cursor", "front", true);
    let name = |r: Rect| columns(r, &l, 5, 5);
    assert_eq!(
        count(&quads, cursor, theme::CURSOR_RGB),
        1,
        "the cursor's bar"
    );
    assert_eq!(count(&quads, cursor, theme.row_active_bg), 0);
    assert_eq!(
        count(&quads, front, theme.row_active_bg),
        1,
        "the front tab's row"
    );
    assert_eq!(count(&quads, front, theme::CURSOR_RGB), 0);
    assert!(
        !glyphs(&quads, name(front), theme::CURRENT_RGB).is_empty(),
        "its name in orange"
    );
    assert!(glyphs(&quads, name(cursor), theme::CURRENT_RGB).is_empty());
    assert!(!glyphs(&quads, name(cursor), theme.sidebar_fg).is_empty());
    for colour in [theme::CURSOR_RGB, theme.row_active_bg, theme.cursor_idle] {
        assert_eq!(count(&quads, plain, colour), 0, "a row that is neither");
    }
    let active = solids(&quads, front, theme.row_active_bg)[0];
    assert_eq!(
        (active.pos, active.size),
        ([0.0, front.y], [l.sidebar.w, front.h]),
        "the whole row"
    );
    let bar = solids(&quads, cursor, theme::CURSOR_RGB)[0];
    assert!(
        bar.size[0] < l.cell.0,
        "a bar in the padding's column: the text keeps its place"
    );
    assert_eq!((bar.pos, bar.size[1]), ([0.0, cursor.y], cursor.h));

    // Both on one row: both marks.
    let (quads, _, [.., both]) = looks("both", "both", true);
    assert_eq!(count(&quads, both, theme::CURSOR_RGB), 1);
    assert_eq!(count(&quads, both, theme.row_active_bg), 1);

    // The list without the keyboard focus: the cursor is still there, quieter.
    let (quads, _, [cursor, front, ..]) = looks("cursor", "front", false);
    assert_eq!(count(&quads, cursor, theme::CURSOR_RGB), 0);
    assert_eq!(count(&quads, cursor, theme.cursor_idle), 1);
    assert_eq!(count(&quads, front, theme.row_active_bg), 1);
}

#[test]
fn other_clients_show_beside_the_name() {
    let mut info = session(LOCAL, "shared", Status::Idle);
    info.attached = 3;
    let look = RowLook {
        others: 2,
        ..RowLook::default()
    };
    let said = spans(&info, look);
    let others = find(&said, "◉2");
    assert_eq!((others.line, others.col + 2), (0, 27));
    assert!(
        spans(&info, RowLook::default())
            .iter()
            .all(|span| !span.text.starts_with('◉'))
    );

    // The window's own tab is a client too, and is not counted.
    let mut list = sidebar(&[LOCAL]);
    let mut mine = session(LOCAL, "mine", Status::Idle);
    mine.attached = 1;
    list.apply(snapshot(LOCAL, vec![info.clone(), mine.clone()]));
    let theme = Theme::default();
    let (quads, l, _atlas) = built(
        &view(&[&mine.key, &info.key], Some(0), false),
        &list,
        &theme,
    );
    let count_at = |row| columns(row_rect(&list, &l, row), &l, 25, 2);
    assert!(
        !glyphs(&quads, count_at(1), theme.muted_fg).is_empty(),
        "◉2 of 3: one is this window"
    );
    assert!(
        glyphs(&quads, count_at(2), theme.muted_fg).is_empty(),
        "only this window"
    );
    // With no tab on it, every client is another one.
    let (quads, l, _atlas) = built(&view(&[], None, false), &list, &theme);
    let count_at = |row| columns(row_rect(&list, &l, row), &l, 25, 2);
    assert!(!glyphs(&quads, count_at(2), theme.muted_fg).is_empty());
}

#[test]
fn legend_and_buttons_sit_at_the_bottom() {
    let theme = Theme::default();
    let mut list = sidebar(&[LOCAL]);
    list.apply(snapshot(LOCAL, vec![session(LOCAL, "work", Status::Idle)]));
    let (quads, l, _atlas) = built(&view(&[], None, false), &list, &theme);
    assert_eq!(
        l.buttons.bottom(),
        l.sidebar.bottom() - (theme.inset).round()
    );
    assert_eq!(l.legend.bottom(), l.buttons.y);
    assert_eq!(l.legend.h, 3.0 * l.cell.1);
    assert!(l.list.bottom() <= l.legend.y);
    // One mark of each status's colour in the legend, and a line above it.
    for status in [
        Status::Running,
        Status::Waiting,
        Status::Unseen,
        Status::Error,
    ] {
        assert_eq!(
            glyphs(&quads, l.legend, mark_rgb(status)).len(),
            1,
            "{status:?}"
        );
    }
    let above = Rect {
        y: l.legend.y - 4.0,
        h: 4.0,
        ..l.legend
    };
    assert_eq!(solids(&quads, above, theme.sidebar_rule).len(), 1);
    // The buttons' labels: 3 + 4 + 4 + 1 letters and the bell.
    let labels = |quads: &[Instance]| -> usize {
        [theme::ADD_RGB, theme::COPY_RGB, theme.muted_fg]
            .iter()
            .map(|colour| glyphs(quads, l.buttons, *colour).len())
            .sum()
    };
    assert_eq!(labels(&quads), 13, "New  Open  Copy  ?  !");
    assert!(glyphs(&quads, l.buttons, theme::CURSOR_RGB).is_empty());
    // With alerts not looked at yet the bell has their count, in the accent colour, and
    // still ends inside the sidebar.
    let mut unread = view(&[], None, false);
    unread.unread = 50;
    let (quads, l, _atlas) = built(&unread, &list, &theme);
    assert_eq!(labels(&quads), 12);
    let bell = glyphs(&quads, l.buttons, theme::CURSOR_RGB);
    assert_eq!(bell.len(), 3, "!50");
    let right = bell.iter().map(|quad| quad.pos[0] + quad.size[0]);
    assert!(right.fold(0.0, f32::max) <= l.sidebar.right());
}

/// A minimal PNG: 8-bit RGB, stored (not compressed) deflate blocks.
fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for byte in bytes {
            crc ^= *byte as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & (!(crc & 1)).wrapping_add(1));
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let mut raw = Vec::new();
    for row in rgba.chunks_exact(width as usize * 4) {
        raw.push(0);
        raw.extend(row.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (n, block) in blocks.iter().enumerate() {
        zlib.push((n + 1 == blocks.len()) as u8);
        zlib.extend((block.len() as u16).to_le_bytes());
        zlib.extend((!(block.len() as u16)).to_le_bytes());
        zlib.extend(*block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend(((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// The list as tmls would show the same sessions, rendered without a window and saved to
/// `PICTURE`. Needs a wgpu adapter (a software one will do); without one the test says so and
/// passes: the quads are checked by the tests above.
#[test]
fn the_sidebar_renders_offscreen() {
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(e) => {
            println!("SKIPPED the_sidebar_renders_offscreen: no wgpu adapter here ({e:#})");
            return;
        }
    };
    let theme = Theme::default();
    let mut list = sidebar_with_pins(&[LOCAL, "nas", "pc"], &[key("nas", "media")]);
    let mut agent = claude(LOCAL, "tmls-rs", "Sidebar that looks like tmls", 58);
    agent.attached = 1;
    let mut ask = claude(LOCAL, "Agent Thing", "Refactor the poller", 74);
    ask.status = Status::Waiting;
    ask.reason = Some("permission prompt".to_string());
    let mut shared = session(LOCAL, "notes", Status::Idle);
    shared.attached = 2;
    let full = SessionInfo {
        status: Status::Unseen,
        ..claude(
            LOCAL,
            "a-session-with-quite-a-long-name",
            "会議の記録 — context nearly full",
            93,
        )
    };
    let codex = SessionInfo {
        tool: Tool::Codex,
        ..session(LOCAL, "review", Status::Error)
    };
    list.apply(snapshot(
        LOCAL,
        vec![agent.clone(), ask, shared, full, codex],
    ));
    let opencode = SessionInfo {
        tool: Tool::Opencode,
        ..session("nas", "media", Status::Running)
    };
    list.apply(snapshot(
        "nas",
        vec![opencode, session("nas", "backup", Status::Idle)],
    ));
    list.apply(HostSnapshot {
        host: "pc".to_string(),
        online: false,
        error: Some("timed out".to_string()),
        sessions: Vec::new(),
    });
    list.select_key(&key(LOCAL, "notes"));

    let mut shown = view(&[&agent.key], Some(0), true);
    shown.tabs[0].mark = Some(('●', mark_rgb(Status::Running)));
    shown.tabs[0].host = host_rgb(HostColor::Teal);
    shown.status = "laptop · tmls-rs".to_string();
    let mut atlas = atlas();
    let l = layout(WINDOW, 1.0, &atlas.metrics(), &theme);
    let mut chrome = Chrome::new();
    chrome.update(&shown, &list, &l, &theme, &mut atlas);
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut atlas,
        &layer_order(None, &chrome),
        WINDOW,
    );
    let rgba = render_offscreen(&gpu, &renderer, WINDOW.0, WINDOW.1, theme.terminal.bg).unwrap();
    assert_eq!(rgba.len(), (WINDOW.0 * WINDOW.1 * 4) as usize);

    let pixel = |x: f32, y: f32| {
        let at = (y as usize * WINDOW.0 as usize + x as usize) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2]]
    };
    let close = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2);
    // The list's background, the first host's line, the front tab's row, the cursor's bar.
    assert!(close(
        pixel(l.sidebar.w - 2.0, l.list.bottom() - 2.0),
        theme.sidebar_bg
    ));
    assert!(close(
        pixel(l.sidebar.w / 2.0, l.list.y),
        host_rgb(HostColor::Teal)
    ));
    let front = row_rect(&list, &l, 1);
    assert!(close(
        pixel(front.right() - 2.0, front.y + 2.0),
        theme.row_active_bg
    ));
    let cursor = row_rect(&list, &l, 3);
    assert!(close(pixel(0.0, cursor.y + 2.0), theme::CURSOR_RGB));
    // Text is there: ink in the first session's name.
    let name = columns(front, &l, 5, 7);
    let inked = (0..name.w as u32).any(|dx| {
        (0..l.cell.1 as u32).any(|dy| {
            !close(
                pixel(name.x + dx as f32, name.y + dy as f32),
                theme.row_active_bg,
            )
        })
    });
    assert!(inked, "the name has ink");

    if let Some(dir) = std::path::Path::new(PICTURE)
        .parent()
        .filter(|dir| dir.is_dir())
    {
        std::fs::write(PICTURE, png(WINDOW.0, WINDOW.1, &rgba)).unwrap();
        println!("wrote {PICTURE} (in {})", dir.display());
    } else {
        println!("NOT SAVED: {PICTURE} (its directory is not here)");
    }
}
