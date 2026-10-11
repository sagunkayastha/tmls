//! The session list's model: host sections, selection by identity, pins, scrolling, and which
//! row a line of the list belongs to. No window, no fonts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use client_core::config::tmls_slug;
use client_core::{
    Config, HostColor, HostSnapshot, Seen, SessionInfo, SessionKey, Status, Tool, default_color,
};
use desktop::sidebar::{Action, Row, Sidebar};

const LOCAL: &str = "laptop";

fn config(hosts: &[&str]) -> Config {
    Config {
        local_name: LOCAL.to_string(),
        hosts: hosts.iter().map(|host| host.to_string()).collect(),
        host_colors: BTreeMap::from([("nas".to_string(), HostColor::Rose)]),
        presets: Vec::new(),
        pins: BTreeSet::new(),
    }
}

/// A config directory of this test's own; never `~/.config/tmls`.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tmlsrs-test-sidebar-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sidebar(hosts: &[&str]) -> Sidebar {
    // Nothing is written unless a pin is toggled; the tests that do have their own directory.
    Sidebar::new(&config(hosts), PathBuf::from("/nonexistent/tmlsrs-test"))
}

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

fn claude(host: &str, name: &str, title: &str) -> SessionInfo {
    SessionInfo {
        tool: Tool::Claude,
        title: Some(title.to_string()),
        context_pct: Some(58),
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

fn idle(host: &str, names: &[&str]) -> HostSnapshot {
    let sessions = names
        .iter()
        .map(|name| session(host, name, Status::Idle))
        .collect();
    snapshot(host, sessions)
}

fn offline(host: &str) -> HostSnapshot {
    HostSnapshot {
        host: host.to_string(),
        online: false,
        error: Some("timed out".to_string()),
        sessions: Vec::new(),
    }
}

/// The rows as text: `[host]`, `[host offline]`, a session's name, `-` for a gap.
fn shape(sidebar: &Sidebar) -> Vec<String> {
    sidebar
        .rows()
        .iter()
        .map(|row| match row {
            Row::Host { host, online: true, .. } => format!("[{host}]"),
            Row::Host { host, online: false, .. } => format!("[{host} offline]"),
            Row::Session(info) => info.key.name.clone(),
            Row::Gap => "-".to_string(),
        })
        .collect()
}

fn selected(sidebar: &Sidebar) -> Option<(String, String)> {
    sidebar
        .selected()
        .map(|key| (key.host.clone(), key.name.clone()))
}

fn at(host: &str, name: &str) -> Option<(String, String)> {
    Some((host.to_string(), name.to_string()))
}

#[test]
fn hosts_in_config_order_local_first() {
    let mut s = sidebar(&[LOCAL, "nas", "pc"]);
    // Every host has its header before it has answered; the answers come in any order.
    assert_eq!(shape(&s), ["[laptop]", "-", "[nas]", "-", "[pc]"]);
    assert_eq!(s.selected(), None);
    s.apply(idle("pc", &["build"]));
    s.apply(idle("nas", &["media", "backup"]));
    s.apply(idle(LOCAL, &["work"]));
    s.apply(idle("stranger", &["x"]));
    assert_eq!(
        shape(&s),
        ["[laptop]", "work", "-", "[nas]", "media", "backup", "-", "[pc]", "build"]
    );
    let colours: Vec<HostColor> = s
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Host { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(
        colours,
        [default_color(LOCAL), HostColor::Rose, default_color("pc")],
        "the user's colour for a host, else the one tmls picks"
    );
}

#[test]
fn offline_host_keeps_its_header_and_drops_sessions() {
    let mut s = sidebar(&[LOCAL, "nas"]);
    s.apply(idle(LOCAL, &["work"]));
    s.apply(idle("nas", &["media", "backup"]));
    s.select_key(&key("nas", "backup"));
    s.apply(offline("nas"));
    assert_eq!(shape(&s), ["[laptop]", "work", "-", "[nas offline]"]);
    assert_eq!(selected(&s), at(LOCAL, "work"), "the nearest session left");
    assert_eq!(s.click(3), None, "no new session on a host that is offline");
    s.apply(idle("nas", &["media"]));
    assert_eq!(shape(&s), ["[laptop]", "work", "-", "[nas]", "media"]);
    assert_eq!(s.click(3), Some(Action::NewSessionOn("nas".to_string())));

    // The only host with sessions goes away: nothing is selected, and nothing can be opened.
    let mut s = sidebar(&["nas"]);
    s.apply(idle("nas", &["media"]));
    assert_eq!(selected(&s), at("nas", "media"));
    s.apply(offline("nas"));
    assert_eq!(s.selected(), None);
    assert_eq!(s.activate(), None);
    s.move_by(1);
    assert_eq!(s.selected(), None);
}

#[test]
fn selection_follows_the_session_when_rows_move() {
    let mut s = sidebar(&[LOCAL, "nas"]);
    s.apply(idle(LOCAL, &["b", "d"]));
    s.apply(idle("nas", &["m", "n", "o"]));
    assert_eq!(selected(&s), at(LOCAL, "b"), "the first session to appear");
    s.select_key(&key("nas", "n"));

    // A session inserted above, on another host and on its own.
    s.apply(idle(LOCAL, &["a", "b", "c", "d"]));
    assert_eq!(selected(&s), at("nas", "n"));
    s.apply(idle("nas", &["l", "m", "n", "o"]));
    assert_eq!(selected(&s), at("nas", "n"));

    // A rename elsewhere: the old name goes, a new one comes.
    s.apply(idle("nas", &["l", "renamed", "n", "o"]));
    assert_eq!(selected(&s), at("nas", "n"));
    s.apply(idle(LOCAL, &["a", "z"]));
    assert_eq!(selected(&s), at("nas", "n"));

    // The selected one is killed: the row that took its place, on the same host.
    s.apply(idle("nas", &["l", "renamed", "o"]));
    assert_eq!(selected(&s), at("nas", "o"));
    // The last of the host is killed: the one before it, not the next host's header or a gap.
    s.apply(idle("nas", &["l", "renamed"]));
    assert_eq!(selected(&s), at("nas", "renamed"));
    // Renamed while selected is, by identity, killed: the selection stays at its place.
    s.apply(idle("nas", &["l", "again"]));
    assert_eq!(selected(&s), at("nas", "again"));
    // The host's last session goes: the nearest session of another host.
    s.apply(idle("nas", &[]));
    assert_eq!(selected(&s), at(LOCAL, "z"));
    for row in s.rows() {
        if let Row::Session(info) = row {
            assert_ne!(info.key.host, "nas");
        }
    }
    // Nothing left at all, then something again.
    s.apply(idle(LOCAL, &[]));
    assert_eq!(s.selected(), None);
    s.apply(idle("nas", &["back"]));
    assert_eq!(selected(&s), at("nas", "back"));
}

#[test]
fn move_by_skips_headers_and_clamps() {
    let mut s = sidebar(&[LOCAL, "nas", "pc"]);
    s.apply(idle(LOCAL, &["a", "b"]));
    s.apply(idle("pc", &["c"]));
    // [laptop] a b - [nas] - [pc] c
    assert_eq!(selected(&s), at(LOCAL, "a"));
    s.move_by(-1);
    assert_eq!(selected(&s), at(LOCAL, "a"), "clamped at the top");
    s.move_by(1);
    assert_eq!(selected(&s), at(LOCAL, "b"));
    s.move_by(1);
    assert_eq!(selected(&s), at("pc", "c"), "over a gap, an empty host and two headers");
    s.move_by(1);
    assert_eq!(selected(&s), at("pc", "c"), "clamped at the bottom");
    s.move_by(-2);
    assert_eq!(selected(&s), at(LOCAL, "a"));
    s.move_by(40);
    assert_eq!(selected(&s), at("pc", "c"));
    assert_eq!(s.activate(), Some(Action::Attach(key("pc", "c"))));

    // Clicks: a session is selected and attached; a header asks for a new session; a gap
    // and a row that is not there do nothing.
    assert_eq!(s.click(1), Some(Action::Attach(key(LOCAL, "a"))));
    assert_eq!(selected(&s), at(LOCAL, "a"));
    assert_eq!(s.click(0), Some(Action::NewSessionOn(LOCAL.to_string())));
    assert_eq!(s.click(3), None);
    assert_eq!(s.click(99), None);
    assert_eq!(s.menu(2), Some(Action::Menu(key(LOCAL, "b"))));
    assert_eq!(selected(&s), at(LOCAL, "b"), "the menu is for the row under the pointer");
    assert_eq!(s.menu(0), None);
    assert_eq!(selected(&s), at(LOCAL, "b"));
}

#[test]
fn pins_sort_and_persist() {
    let dir = scratch("pins");
    let mut cfg = config(&[LOCAL, "nas"]);
    // A session pinned in tmls, which calls this machine "local".
    let from_tmls = tmls_slug(&key(LOCAL, "c"), LOCAL);
    assert!(from_tmls.starts_with("local-c-"));
    cfg.pins.insert(from_tmls.clone());
    cfg.pins.insert("gone-session-0a0b0c0d".to_string());
    let mut s = Sidebar::new(&cfg, dir.clone());
    s.apply(idle(LOCAL, &["a", "b", "c"]));
    s.apply(idle("nas", &["m", "Agent Thing"]));
    assert_eq!(
        shape(&s),
        ["[laptop]", "c", "a", "b", "-", "[nas]", "m", "Agent Thing"],
        "pinned first, the rest in the listing's order"
    );
    assert!(s.is_pinned(&key(LOCAL, "c")));
    assert!(!dir.join("pins").exists(), "nothing is written until a pin changes");

    s.select_key(&key("nas", "Agent Thing"));
    assert!(s.toggle_pin());
    assert_eq!(shape(&s)[5..], ["[nas]", "Agent Thing", "m"]);
    assert_eq!(selected(&s), at("nas", "Agent Thing"), "the selection moves with its row");
    let written = std::fs::read_to_string(dir.join("pins")).unwrap();
    let mut expected = vec![
        from_tmls.clone(),
        "gone-session-0a0b0c0d".to_string(),
        tmls_slug(&key("nas", "Agent Thing"), LOCAL),
    ];
    expected.sort();
    assert_eq!(
        written.lines().collect::<Vec<_>>(),
        expected,
        "tmls's format: every slug, one per line, sorted; pins of sessions not listed are kept"
    );
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, ["pins"], "the only file written");

    // A restart reads them back.
    let reloaded = Config::load(&dir, &[]);
    assert!(reloaded.is_pinned(&key("nas", "Agent Thing")));

    // Unpinning puts the row back where the listing has it.
    s.select_key(&key(LOCAL, "c"));
    assert!(!s.toggle_pin());
    assert_eq!(shape(&s)[..4], ["[laptop]", "a", "b", "c"]);
    assert_eq!(selected(&s), at(LOCAL, "c"));
    let written = std::fs::read_to_string(dir.join("pins")).unwrap();
    assert!(!written.contains(&from_tmls));
    // A new listing keeps the pins' order.
    s.apply(idle("nas", &["k", "m", "Agent Thing"]));
    assert_eq!(shape(&s)[5..], ["[nas]", "Agent Thing", "k", "m"]);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn next_waiting_order() {
    let mut s = sidebar(&[LOCAL, "nas"]);
    assert_eq!(s.next_waiting(None), None);
    s.apply(snapshot(
        LOCAL,
        vec![
            session(LOCAL, "done1", Status::Unseen),
            session(LOCAL, "ask1", Status::Waiting),
            session(LOCAL, "busy", Status::Running),
        ],
    ));
    s.apply(snapshot(
        "nas",
        vec![
            session("nas", "ask2", Status::Waiting),
            session("nas", "done2", Status::Unseen),
            session("nas", "failed", Status::Error),
        ],
    ));
    // ? before ◆, whatever comes first in the list.
    assert_eq!(s.next_waiting(None), Some(key(LOCAL, "ask1")));
    assert_eq!(s.next_waiting(Some(&key(LOCAL, "ask1"))), Some(key("nas", "ask2")));
    assert_eq!(
        s.next_waiting(Some(&key("nas", "ask2"))),
        Some(key(LOCAL, "ask1")),
        "wraps round to the other ?"
    );
    assert_eq!(s.next_waiting(Some(&key("nas", "failed"))), Some(key(LOCAL, "ask1")));
    assert_eq!(
        s.next_waiting(Some(&key("gone", "x"))),
        Some(key(LOCAL, "ask1")),
        "a session the list does not have: from the top"
    );

    // No ? but the one in front: the ◆ after it, wrapping.
    s.apply(snapshot(
        LOCAL,
        vec![
            session(LOCAL, "done1", Status::Unseen),
            session(LOCAL, "ask1", Status::Idle),
            session(LOCAL, "busy", Status::Running),
        ],
    ));
    assert_eq!(s.next_waiting(Some(&key("nas", "ask2"))), Some(key("nas", "done2")));
    s.apply(snapshot("nas", vec![session("nas", "done2", Status::Unseen)]));
    assert_eq!(s.next_waiting(Some(&key("nas", "done2"))), Some(key(LOCAL, "done1")));
    assert_eq!(s.next_waiting(Some(&key(LOCAL, "done1"))), Some(key("nas", "done2")));
    s.apply(idle("nas", &["done2"]));
    assert_eq!(
        s.next_waiting(Some(&key(LOCAL, "done1"))),
        None,
        "the one in front is not next after itself"
    );
}

#[test]
fn view_keeps_the_selection_visible() {
    let names: Vec<String> = (0..10).map(|n| format!("s{n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut s = sidebar(&[LOCAL, "nas"]);
    s.apply(idle(LOCAL, &names));
    s.apply(idle("nas", &["m"]));
    // 14 rows: [laptop] s0..s9 - [nas] m
    assert_eq!(s.view(5), 0..5);
    assert_eq!(s.view(100), 0..14);
    assert_eq!(s.view(0), 0..0);

    s.move_by(6); // s6, row 7
    assert_eq!(s.view(5), 3..8, "scrolled just far enough");
    s.move_by(-1);
    assert_eq!(s.view(5), 3..8, "still in view: nothing moves");
    s.move_by(-5); // s0, row 1
    assert_eq!(s.view(5), 0..5, "back up, and the host's header with it");
    s.move_by(40); // m, the last row
    assert_eq!(s.view(5), 9..14);

    // The wheel scrolls without moving the selection, and stops at both ends.
    s.scroll(-3);
    assert_eq!(s.view(5), 6..11);
    assert_eq!(selected(&s), at("nas", "m"));
    s.scroll(-100);
    assert_eq!(s.view(5), 0..5);
    s.scroll(100);
    assert_eq!(s.view(5), 9..14, "the last row at the bottom, not at the top");
    s.scroll(-100);
    // The next move of the selection brings it back into view.
    s.move_by(-1); // s9, row 10
    assert_eq!(s.view(5), 6..11);
    // So does a listing that moves it.
    s.scroll(-100);
    assert_eq!(s.view(5), 0..5);
    s.apply(idle(LOCAL, &["s0", "s1"]));
    // [laptop] s0 s1 - [nas] m : the selection went to the nearest row of its host
    assert_eq!(selected(&s), at(LOCAL, "s1"));
    assert_eq!(s.view(2), 1..3);
    // A window that grows shows what fits again.
    assert_eq!(s.view(100), 0..6);
}

#[test]
fn two_line_rows_count_two() {
    let mut s = sidebar(&[LOCAL]);
    let waiting = SessionInfo {
        tool: Tool::Claude,
        reason: Some("permission prompt".to_string()),
        ..session(LOCAL, "ask", Status::Waiting)
    };
    let own_name = claude(LOCAL, "same", "same");
    s.apply(snapshot(
        LOCAL,
        vec![
            claude(LOCAL, "agent", "Fix the flaky test"),
            session(LOCAL, "shell", Status::Idle),
            waiting,
            own_name,
        ],
    ));
    let lines: Vec<usize> = s.rows().iter().map(Row::lines).collect();
    assert_eq!(
        lines,
        [1, 2, 1, 2, 1],
        "a title or what Claude waits for is a second line; a title that is the name is not"
    );

    // Lines of the list, top to bottom: header, agent, its title, shell, ask, its reason, same.
    let rows: Vec<Option<usize>> = (0..8).map(|line| s.row_at(line, 20)).collect();
    assert_eq!(
        rows,
        [Some(0), Some(1), Some(1), Some(2), Some(3), Some(3), Some(4), None]
    );
    // A row is in view only when all of it fits (the wheel, so the view is not the selection's).
    s.scroll(-1);
    assert_eq!(s.view(2), 0..1);
    assert_eq!(s.row_at(1, 2), None);
    assert_eq!(s.view(3), 0..2);

    // Scrolled by one row, the lines count from the first row in view.
    s.select_key(&key(LOCAL, "same"));
    assert_eq!(s.view(4), 2..5);
    assert_eq!(s.row_at(0, 4), Some(2));
    assert_eq!(s.row_at(2, 4), Some(3));
    assert_eq!(s.row_at(3, 4), Some(4));
    assert_eq!(s.row_at(4, 4), None);
}

#[test]
fn the_session_in_front_never_gets_the_done_mark() {
    let mut s = sidebar(&[LOCAL]);
    let mut seen = Seen::new();
    let listing = |a: Status, b: Status| {
        snapshot(LOCAL, vec![session(LOCAL, "a", a), session(LOCAL, "b", b)])
    };
    let front = key(LOCAL, "a");
    s.deliver(&mut seen, listing(Status::Running, Status::Running), Some(&front));
    s.deliver(&mut seen, listing(Status::Idle, Status::Idle), Some(&front));
    assert_eq!(s.status(&front), Some(Status::Idle), "the user watched it finish");
    assert_eq!(s.status(&key(LOCAL, "b")), Some(Status::Unseen));
    // It stays seen when the same listing comes again.
    s.deliver(&mut seen, listing(Status::Idle, Status::Idle), None);
    assert_eq!(s.status(&front), Some(Status::Idle));
    assert_eq!(s.status(&key(LOCAL, "b")), Some(Status::Unseen));

    // Opening the other one's tab clears its mark, in the list and for later listings.
    assert!(s.opened(&mut seen, &key(LOCAL, "b")));
    assert_eq!(s.status(&key(LOCAL, "b")), Some(Status::Idle));
    assert!(!s.opened(&mut seen, &key(LOCAL, "b")), "nothing changed the second time");
    s.deliver(&mut seen, listing(Status::Idle, Status::Idle), None);
    assert_eq!(s.status(&key(LOCAL, "b")), Some(Status::Idle));
    assert_eq!(s.status(&key("nas", "none")), None);
}
