//! The go-to picker: what matches what is typed, the status filter, the order, and its keys.
//! No window.

use client_core::{SessionInfo, SessionKey, Status, Tool};
use desktop::forms::{Look, Target};
use desktop::goto::{Entry, Filter, GoTo, PickResult, results, score};
use s2_core::{Key, KeyEvent, Mods, Named};

const NONE: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: false,
    logo: false,
};
const SHIFT: Mods = Mods {
    shift: true,
    ..NONE
};
const CTRL: Mods = Mods { ctrl: true, ..NONE };

fn press(key: Key, mods: Mods) -> KeyEvent {
    let text = match key {
        Key::Char(c) if !mods.ctrl => Some(c.to_string()),
        _ => None,
    };
    KeyEvent {
        key,
        mods,
        text,
        repeat: false,
        pressed: true,
    }
}

fn named(named: Named, mods: Mods) -> KeyEvent {
    press(Key::Named(named), mods)
}

fn ch(c: char, mods: Mods) -> KeyEvent {
    press(Key::Char(c), mods)
}

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey {
        host: host.to_string(),
        name: name.to_string(),
    }
}

fn entry(host: &str, name: &str, status: Status, detail: Option<&str>) -> Entry {
    Entry {
        key: key(host, name),
        status,
        detail: detail.map(str::to_string),
        stale: false,
    }
}

fn names(entries: &[Entry], shown: &[usize]) -> Vec<String> {
    shown
        .iter()
        .map(|at| entries[*at].key.name.clone())
        .collect()
}

fn typed(goto: &mut GoTo, text: &str) {
    for c in text.chars() {
        assert_eq!(goto.key(&ch(c, NONE)), PickResult::Pending);
    }
}

fn shown(goto: &GoTo) -> Vec<String> {
    goto.shown().map(|entry| entry.key.name.clone()).collect()
}

#[test]
fn a_word_matches_the_name_the_host_the_detail_or_the_letters_in_order() {
    let fizz = entry("nas", "fizz", Status::Idle, Some("refactor the parser"));
    assert_eq!(score("", &fizz), Some(0), "nothing typed matches all");
    assert_eq!(score("fizz", &fizz), Some(0));
    assert_eq!(score("FI", &fizz), Some(1), "case does not count");
    assert_eq!(score("izz", &fizz), Some(2));
    assert_eq!(score("nas", &fizz), Some(3), "the host");
    assert_eq!(score("parser", &fizz), Some(3), "the dim line");
    assert_eq!(score("fz", &fizz), Some(4), "the letters in order");
    assert_eq!(score("zf", &fizz), None);
    assert_eq!(
        score("fizz nas", &fizz),
        Some(3),
        "every word, scores added"
    );
    assert_eq!(score("fizz pc", &fizz), None, "a word that matches nothing");
}

#[test]
fn waiting_sessions_come_first_then_the_better_match_then_the_list_order() {
    let entries = vec![
        entry("laptop", "api-tests", Status::Idle, None),
        entry("laptop", "api", Status::Running, None),
        entry("nas", "my-api", Status::Waiting, Some("permission prompt")),
        entry("nas", "media", Status::Idle, Some("api docs")),
        entry("pc", "apixel", Status::Idle, None),
        entry("pc", "build", Status::Unseen, None),
    ];
    assert_eq!(
        names(&entries, &results(&entries, "api", Filter::All)),
        ["my-api", "api", "api-tests", "apixel", "media"]
    );
    assert_eq!(
        names(&entries, &results(&entries, "", Filter::All)),
        ["my-api", "api-tests", "api", "media", "apixel", "build"],
        "nothing typed: what waits, then the list's order"
    );
}

#[test]
fn the_filter_narrows_by_status() {
    let entries = vec![
        entry("a", "wait", Status::Waiting, None),
        entry("a", "fail", Status::Error, None),
        entry("a", "run", Status::Running, None),
        entry("a", "rest", Status::Idle, None),
        entry("a", "fresh", Status::Unseen, None),
    ];
    let with = |filter| names(&entries, &results(&entries, "", filter));
    assert_eq!(with(Filter::Blocked), ["wait", "fail"]);
    assert_eq!(with(Filter::Working), ["run"]);
    assert_eq!(with(Filter::Idle), ["rest"]);
    assert_eq!(with(Filter::Done), ["fresh"]);
    assert_eq!(with(Filter::All).len(), 5);
    assert_eq!(Filter::All.step(true), Filter::Blocked);
    assert_eq!(Filter::Done.step(true), Filter::All, "round the end");
    assert_eq!(Filter::All.step(false), Filter::Done);
}

#[test]
fn typing_narrows_and_enter_opens_the_first() {
    let mut goto = GoTo::new(vec![
        entry("laptop", "work", Status::Idle, None),
        entry("nas", "media", Status::Idle, None),
        entry("nas", "backup", Status::Running, None),
    ]);
    assert_eq!(shown(&goto), ["work", "media", "backup"]);
    typed(&mut goto, "nas");
    assert_eq!(shown(&goto), ["media", "backup"]);
    goto.key(&named(Named::Down, NONE));
    assert_eq!(
        goto.key(&named(Named::Enter, NONE)),
        PickResult::Open(key("nas", "backup"))
    );
    goto.key(&named(Named::Backspace, NONE));
    assert_eq!(
        shown(&goto),
        ["media", "backup"],
        "\"na\": in their host only"
    );
    // A held Enter opens nothing.
    let held = KeyEvent {
        repeat: true,
        ..named(Named::Enter, NONE)
    };
    assert_eq!(goto.key(&held), PickResult::Pending);
    assert_eq!(goto.key(&named(Named::Escape, NONE)), PickResult::Closed);
}

#[test]
fn up_down_and_ctrl_j_k_move_within_the_results() {
    let mut goto = GoTo::new(vec![
        entry("a", "one", Status::Idle, None),
        entry("a", "two", Status::Idle, None),
    ]);
    goto.key(&named(Named::Up, NONE));
    assert_eq!(goto.selected().unwrap().key.name, "one", "stops at the top");
    goto.key(&ch('j', CTRL));
    assert_eq!(goto.selected().unwrap().key.name, "two");
    goto.key(&named(Named::Down, NONE));
    assert_eq!(
        goto.selected().unwrap().key.name,
        "two",
        "stops at the bottom"
    );
    goto.key(&ch('k', CTRL));
    assert_eq!(goto.selected().unwrap().key.name, "one");
    // A plain j is typed: no session has a j.
    typed(&mut goto, "j");
    assert_eq!(goto.selected(), None);
    assert_eq!(goto.key(&named(Named::Enter, NONE)), PickResult::Pending);
}

#[test]
fn tab_and_ctrl_letters_choose_the_filter() {
    let mut goto = GoTo::new(vec![
        entry("a", "wait", Status::Waiting, None),
        entry("a", "run", Status::Running, None),
    ]);
    goto.key(&named(Named::Tab, NONE));
    assert_eq!(goto.filter(), Filter::Blocked);
    assert_eq!(shown(&goto), ["wait"]);
    goto.key(&named(Named::Tab, SHIFT));
    assert_eq!(goto.filter(), Filter::All);
    goto.key(&ch('w', CTRL));
    assert_eq!(goto.filter(), Filter::Working);
    assert_eq!(shown(&goto), ["run"]);
    goto.key(&ch('w', CTRL));
    assert_eq!(goto.filter(), Filter::All, "the same one again: all");
    goto.key(&ch('d', CTRL));
    assert_eq!(goto.filter(), Filter::Done);
    assert!(shown(&goto).is_empty());
    goto.key(&ch('i', CTRL));
    assert_eq!(goto.filter(), Filter::Idle);
    goto.key(&ch('b', CTRL));
    assert_eq!(goto.filter(), Filter::Blocked);
    let title = &goto.panel().cells[0].text;
    assert!(
        title.contains("blocked"),
        "the title shows the filter: {title}"
    );
}

#[test]
fn the_panel_lists_mark_name_host_and_the_detail_at_the_right() {
    let mut info = SessionInfo {
        key: key("nas", "my-api"),
        status: Status::Waiting,
        reason: Some("permission prompt".to_string()),
        tool: Tool::Claude,
        title: None,
        context_pct: None,
        attached: 0,
        last_output_age_s: None,
        pid: None,
        session_id: None,
        pane: None,
        activity: None,
        last_reply: None,
    };
    let waiting = Entry::of(&info, false);
    assert_eq!(waiting.detail.as_deref(), Some("permission prompt"));
    info.key.name = "old".to_string();
    let offline = Entry::of(&info, true);
    let goto = GoTo::new(vec![waiting, offline]);
    let panel = goto.panel();
    let items: Vec<_> = panel
        .cells
        .iter()
        .filter(|cell| matches!(cell.target, Some(Target::Item(_))))
        .collect();
    assert_eq!(items[0].text.trim_end(), " ? my-api · nas");
    assert_eq!(
        items[0].look,
        Look::Item {
            hovered: true,
            danger: false
        }
    );
    let detail = panel
        .cells
        .iter()
        .find(|cell| cell.text == "permission prompt")
        .expect("the detail");
    assert_eq!(detail.look, Look::Dim);
    assert_eq!(detail.col + detail.cols, panel.cols - 1, "right-aligned");
    assert!(
        panel
            .cells
            .iter()
            .any(|c| c.text == "offline" && c.line == items[1].line),
        "a session of a host that does not answer says so"
    );
    let empty = GoTo::new(Vec::new()).panel();
    assert_eq!(
        empty.lines, panel.lines,
        "as tall with no result: typing moves nothing"
    );
}
