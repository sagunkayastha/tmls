//! The session list's model: host sections, selection by identity, pins, scrolling, and which
//! row a line of the list belongs to. No window, no fonts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use client_core::config::tmls_slug;
use client_core::{
    Config, HostColor, HostSnapshot, KITTY, Seen, SessionInfo, SessionKey, Status, Tool,
    default_color,
};
use desktop::sidebar::{Action, Row, Sidebar, renamed_outside, summary};

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

/// A config directory of this test's own under cargo's target tmp dir; never
/// `~/.config/tmls`, never /tmp.
fn scratch(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("tmlsrs-test-sidebar-{}-{test}", std::process::id()));
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
        pid: None,
        session_id: None,
        pane: None,
        activity: None,
        last_reply: None,
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
            Row::Host {
                host, online: true, ..
            } => format!("[{host}]"),
            Row::Host {
                host,
                online: false,
                ..
            } => format!("[{host} offline]"),
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
        [
            "[laptop]", "work", "-", "[nas]", "media", "backup", "-", "[pc]", "build"
        ]
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
    assert_eq!(
        selected(&s),
        at(LOCAL, "work"),
        "the host is back without the session: the cursor stays where it went"
    );

    // Offline for one poll, then back with the same sessions: the cursor returns to the
    // session the user had selected, whatever arrived in between.
    let mut s = sidebar(&[LOCAL, "nas"]);
    s.apply(idle(LOCAL, &["work"]));
    s.apply(idle("nas", &["media", "backup"]));
    s.select_key(&key("nas", "backup"));
    s.apply(offline("nas"));
    assert_eq!(selected(&s), at(LOCAL, "work"));
    s.apply(idle(LOCAL, &["other", "work"]));
    s.apply(offline("nas"));
    assert_eq!(selected(&s), at(LOCAL, "work"));
    s.apply(idle("nas", &["backup", "media"]));
    assert_eq!(
        selected(&s),
        at("nas", "backup"),
        "back on the session the user chose"
    );
    assert_eq!(s.activate(), Some(Action::Attach(key("nas", "backup"))));
    // Once only: it is an ordinary selection again.
    s.move_by(-1);
    assert_eq!(selected(&s), at(LOCAL, "work"));
    s.apply(idle("nas", &["backup", "media", "new"]));
    assert_eq!(selected(&s), at(LOCAL, "work"));

    // The user moved the cursor while the host was away: it stays where the user put it.
    for explicit in ["move", "select", "click", "menu"] {
        let mut s = sidebar(&[LOCAL, "nas"]);
        s.apply(idle(LOCAL, &["a", "b", "c"]));
        s.apply(idle("nas", &["media", "backup"]));
        s.select_key(&key("nas", "backup"));
        s.apply(offline("nas"));
        assert_eq!(selected(&s), at(LOCAL, "c"));
        match explicit {
            "move" => s.move_by(-1),
            "select" => s.select_key(&key(LOCAL, "b")),
            "click" => assert_eq!(s.click(2), Some(Action::Attach(key(LOCAL, "b")))),
            _ => assert_eq!(s.menu(2), Some(Action::Menu(key(LOCAL, "b")))),
        }
        s.apply(idle("nas", &["media", "backup"]));
        assert_eq!(selected(&s), at(LOCAL, "b"), "after an explicit {explicit}");
    }

    // A session killed on a host that answers is not waited for: one of that name started
    // later does not take the cursor.
    let mut s = sidebar(&[LOCAL, "nas"]);
    s.apply(idle(LOCAL, &["work"]));
    s.apply(idle("nas", &["media", "backup"]));
    s.select_key(&key("nas", "backup"));
    s.apply(idle("nas", &["media"]));
    assert_eq!(selected(&s), at("nas", "media"));
    s.apply(idle("nas", &["media", "backup"]));
    assert_eq!(selected(&s), at("nas", "media"));

    // The only host with sessions goes away: nothing is selected, and nothing can be opened.
    let mut s = sidebar(&["nas"]);
    s.apply(idle("nas", &["media"]));
    assert_eq!(selected(&s), at("nas", "media"));
    s.apply(offline("nas"));
    assert_eq!(s.selected(), None);
    assert_eq!(s.activate(), None);
    s.move_by(1);
    assert_eq!(s.selected(), None);
    assert!(
        !s.toggle_pin(),
        "nothing selected: nothing is pinned, nothing is written"
    );
    s.apply(idle("nas", &["first", "media"]));
    assert_eq!(selected(&s), at("nas", "media"), "and back");
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
    assert_eq!(
        selected(&s),
        at("pc", "c"),
        "over a gap, an empty host and two headers"
    );
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
    assert_eq!(
        selected(&s),
        at(LOCAL, "b"),
        "the menu is for the row under the pointer"
    );
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
    assert!(
        !dir.join("pins").exists(),
        "nothing is written until a pin changes"
    );

    s.select_key(&key("nas", "Agent Thing"));
    assert!(s.toggle_pin());
    assert_eq!(shape(&s)[5..], ["[nas]", "Agent Thing", "m"]);
    assert_eq!(
        selected(&s),
        at("nas", "Agent Thing"),
        "the selection moves with its row"
    );
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
fn a_pin_that_cannot_be_saved_is_not_shown() {
    let dir = scratch("pins-unwritable");
    // A regular file where the config directory should be: nothing can be written into it.
    let not_a_dir = dir.join("config");
    std::fs::write(&not_a_dir, "in the way").unwrap();
    let mut cfg = config(&[LOCAL]);
    let saved_before = tmls_slug(&key(LOCAL, "old"), LOCAL);
    cfg.pins.insert(saved_before);
    let mut s = Sidebar::new(&cfg, not_a_dir.clone());
    s.apply(idle(LOCAL, &["new", "old"]));
    assert_eq!(shape(&s), ["[laptop]", "old", "new"]);
    assert!(s.take_pin_error().is_ok(), "nothing went wrong yet");

    // Pinning fails: no ★, no reordering, and the reason is there to show, once.
    s.select_key(&key(LOCAL, "new"));
    assert!(!s.toggle_pin());
    assert!(!s.is_pinned(&key(LOCAL, "new")));
    assert_eq!(shape(&s), ["[laptop]", "old", "new"]);
    let problem = format!("{:#}", s.take_pin_error().unwrap_err());
    assert!(problem.starts_with("pin of new not saved: "), "{problem}");
    assert!(problem.contains("config"), "names the path: {problem}");
    assert!(s.take_pin_error().is_ok(), "reported once");

    // Unpinning fails: it stays pinned, as it will be after a restart.
    s.select_key(&key(LOCAL, "old"));
    assert!(s.toggle_pin());
    assert!(s.is_pinned(&key(LOCAL, "old")));
    assert_eq!(shape(&s), ["[laptop]", "old", "new"]);
    let problem = format!("{:#}", s.take_pin_error().unwrap_err());
    assert!(problem.starts_with("unpin of old not saved: "), "{problem}");
    assert_eq!(std::fs::read_to_string(&not_a_dir).unwrap(), "in the way");

    // The directory is fine again: the pin is saved, with the one from before and no other.
    let mut s = Sidebar::new(&cfg, dir.join("real"));
    s.apply(idle(LOCAL, &["new", "old"]));
    s.select_key(&key(LOCAL, "new"));
    assert!(s.toggle_pin());
    assert!(s.take_pin_error().is_ok());
    assert_eq!(
        std::fs::read_to_string(dir.join("real/pins"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pin_step_walks_the_pinned_sessions() {
    let dir = scratch("pin-step");
    let mut cfg = config(&[LOCAL, "nas"]);
    cfg.pins.insert("gone-session-0a0b0c0d".to_string());
    let mut s = Sidebar::new(&cfg, dir.clone());
    s.apply(idle(LOCAL, &["a", "b", "c"]));
    s.apply(idle("nas", &["x", "y"]));
    // Nothing listed is pinned: the keys are the tabs'.
    assert_eq!(s.pin_step(None, 1), None);
    assert_eq!(s.pin_step(Some(&key(LOCAL, "a")), 1), None);
    for pinned in [key(LOCAL, "c"), key(LOCAL, "a"), key("nas", "y")] {
        s.select_key(&pinned);
        assert!(s.toggle_pin());
    }
    // The ring is the list's order (a, c, y), not the order of pinning.
    let ring = [key(LOCAL, "a"), key(LOCAL, "c"), key("nas", "y")];
    for (at, from) in ring.iter().enumerate() {
        assert_eq!(
            s.pin_step(Some(from), 1).as_ref(),
            Some(&ring[(at + 1) % 3])
        );
        assert_eq!(
            s.pin_step(Some(from), -1).as_ref(),
            Some(&ring[(at + 2) % 3])
        );
    }
    // From a session that is not pinned, or from no tab: the first pinned one, either way.
    assert_eq!(s.pin_step(Some(&key(LOCAL, "b")), 1), Some(key(LOCAL, "a")));
    assert_eq!(
        s.pin_step(Some(&key(LOCAL, "b")), -1),
        Some(key(LOCAL, "a"))
    );
    assert_eq!(s.pin_step(None, -1), Some(key(LOCAL, "a")));
    // All the sessions in the list's order (a c b, y x: the pinned first in each host), for
    // when nothing is pinned and fewer than two tabs are open.
    assert_eq!(
        s.list_step(Some(&key(LOCAL, "c")), 1),
        Some(key(LOCAL, "b"))
    );
    assert_eq!(
        s.list_step(Some(&key(LOCAL, "b")), 1),
        Some(key("nas", "y"))
    );
    assert_eq!(
        s.list_step(Some(&key(LOCAL, "a")), -1),
        Some(key("nas", "x"))
    );
    assert_eq!(s.list_step(None, 1), Some(key(LOCAL, "a")));
    assert_eq!(sidebar(&[LOCAL]).list_step(None, 1), None);
    // A pinned session whose host went away is not in the ring.
    s.apply(HostSnapshot {
        host: "nas".to_string(),
        online: false,
        error: None,
        sessions: Vec::new(),
    });
    assert_eq!(s.pin_step(Some(&key(LOCAL, "c")), 1), Some(key(LOCAL, "a")));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rename_takes_the_pin_along() {
    let dir = scratch("pin-rename");
    let mut s = Sidebar::new(&config(&[LOCAL]), dir.clone());
    s.apply(idle(LOCAL, &["a", "b"]));
    s.select_key(&key(LOCAL, "b"));
    assert!(s.toggle_pin());
    // Not pinned: nothing to move, nothing written.
    let before = std::fs::read_to_string(dir.join("pins")).unwrap();
    assert!(!s.renamed(&key(LOCAL, "a"), &key(LOCAL, "a2")));
    assert_eq!(std::fs::read_to_string(dir.join("pins")).unwrap(), before);
    // Pinned: the file has the new name's slug only, and the next listing shows the ★.
    assert!(s.renamed(&key(LOCAL, "b"), &key(LOCAL, "b2")));
    assert!(s.take_pin_error().is_ok());
    assert_eq!(
        std::fs::read_to_string(dir.join("pins")).unwrap(),
        format!("{}\n", tmls_slug(&key(LOCAL, "b2"), LOCAL))
    );
    s.apply(idle(LOCAL, &["a", "b2"]));
    assert!(s.is_pinned(&key(LOCAL, "b2")) && !s.is_pinned(&key(LOCAL, "a")));
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
    assert_eq!(
        s.next_waiting(Some(&key(LOCAL, "ask1"))),
        Some(key("nas", "ask2"))
    );
    assert_eq!(
        s.next_waiting(Some(&key("nas", "ask2"))),
        Some(key(LOCAL, "ask1")),
        "wraps round to the other ?"
    );
    assert_eq!(
        s.next_waiting(Some(&key("nas", "failed"))),
        Some(key(LOCAL, "ask1"))
    );
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
    assert_eq!(
        s.next_waiting(Some(&key("nas", "ask2"))),
        Some(key("nas", "done2"))
    );
    s.apply(snapshot(
        "nas",
        vec![session("nas", "done2", Status::Unseen)],
    ));
    assert_eq!(
        s.next_waiting(Some(&key("nas", "done2"))),
        Some(key(LOCAL, "done1"))
    );
    assert_eq!(
        s.next_waiting(Some(&key(LOCAL, "done1"))),
        Some(key("nas", "done2"))
    );
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
    assert_eq!(
        s.view(5),
        9..14,
        "the last row at the bottom, not at the top"
    );
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
        [
            Some(0),
            Some(1),
            Some(1),
            Some(2),
            Some(3),
            Some(3),
            Some(4),
            None
        ]
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
    s.deliver(
        &mut seen,
        listing(Status::Running, Status::Running),
        Some(&front),
    );
    s.deliver(&mut seen, listing(Status::Idle, Status::Idle), Some(&front));
    assert_eq!(
        s.status(&front),
        Some(Status::Idle),
        "the user watched it finish"
    );
    assert_eq!(s.status(&key(LOCAL, "b")), Some(Status::Unseen));
    // It stays seen when the same listing comes again.
    s.deliver(&mut seen, listing(Status::Idle, Status::Idle), None);
    assert_eq!(s.status(&front), Some(Status::Idle));
    assert_eq!(s.status(&key(LOCAL, "b")), Some(Status::Unseen));

    // Opening the other one's tab clears its mark, in the list and for later listings.
    assert!(s.opened(&mut seen, &key(LOCAL, "b")));
    assert_eq!(s.status(&key(LOCAL, "b")), Some(Status::Idle));
    assert!(
        !s.opened(&mut seen, &key(LOCAL, "b")),
        "nothing changed the second time"
    );
    s.deliver(&mut seen, listing(Status::Idle, Status::Idle), None);
    assert_eq!(s.status(&key(LOCAL, "b")), Some(Status::Idle));
    assert_eq!(s.status(&key("nas", "none")), None);
}

/// A Claude session outside tmux, as the listing has it.
fn outside(name: &str, pid: u32, status: Status) -> SessionInfo {
    SessionInfo {
        tool: Tool::Claude,
        pid: Some(pid),
        session_id: None,
        pane: None,
        activity: None,
        last_reply: None,
        ..session(KITTY, name, status)
    }
}

fn kinds(s: &Sidebar) -> Vec<String> {
    s.rows()
        .iter()
        .map(|row| match row {
            Row::Host { host, .. } => format!("[{host}]"),
            Row::Session(info) => info.key.name.clone(),
            Row::Gap => "-".to_string(),
        })
        .collect()
}

#[test]
fn claude_outside_tmux_has_a_section_under_this_machine() {
    let dir = scratch("outside");
    let mut s = Sidebar::new(&config(&[LOCAL, "nas"]), dir.clone());
    s.apply(idle(LOCAL, &["a", "b"]));
    s.apply(idle("nas", &["x"]));
    assert_eq!(kinds(&s), ["[laptop]", "a", "b", "-", "[nas]", "x"]);

    // This machine lists two: a rule and the two, right under its tmux sessions.
    let mut local = idle(LOCAL, &["a", "b"]);
    local.sessions.push(outside("notes", 21, Status::Waiting));
    local.sessions.push(outside("thing", 22, Status::Running));
    s.apply(local.clone());
    assert_eq!(
        kinds(&s),
        [
            "[laptop]", "a", "b", "[kitty]", "notes", "thing", "-", "[nas]", "x"
        ]
    );
    assert_eq!(s.pid(&key(KITTY, "thing")), Some(22));
    assert_eq!(s.pid(&key(LOCAL, "a")), None);
    assert_eq!(s.status(&key(KITTY, "notes")), Some(Status::Waiting));

    // They can be selected and activated; they have no menu, no pin, no `+`, and the arrows'
    // rings pass them by.
    s.select_key(&key(KITTY, "thing"));
    assert_eq!(s.selected(), Some(&key(KITTY, "thing")));
    assert_eq!(s.activate(), Some(Action::Attach(key(KITTY, "thing"))));
    assert_eq!(s.click(4), Some(Action::Attach(key(KITTY, "notes"))));
    assert_eq!(s.menu(4), None);
    assert_eq!(s.click(3), None, "the rule is no host");
    assert!(!s.toggle_pin());
    assert!(s.take_pin_error().is_ok());
    assert!(!dir.join("pins").exists());
    assert_eq!(
        s.list_step(Some(&key(LOCAL, "b")), 1),
        Some(key("nas", "x"))
    );
    assert_eq!(s.next_waiting(None), Some(key(KITTY, "notes")));

    // Another host's are not shown.
    let mut nas = idle("nas", &["x"]);
    nas.sessions.push(outside("far", 31, Status::Idle));
    s.apply(nas);
    assert!(!kinds(&s).contains(&"far".to_string()));

    // The same listing again changes nothing; the cursor stays on its session.
    s.select_key(&key(KITTY, "thing"));
    s.apply(local.clone());
    assert_eq!(s.selected(), Some(&key(KITTY, "thing")));
    assert_eq!(kinds(&s).iter().filter(|k| *k == "[kitty]").count(), 1);

    // One ends: the cursor goes to what is left. Both gone: the section goes, the cursor to
    // the nearest session.
    local.sessions.pop();
    s.apply(local.clone());
    assert_eq!(s.selected(), Some(&key(KITTY, "notes")));
    local.sessions.pop();
    s.apply(local);
    assert_eq!(kinds(&s), ["[laptop]", "a", "b", "-", "[nas]", "x"]);
    assert_eq!(s.selected(), Some(&key(LOCAL, "b")));

    // This machine offline: its section goes too.
    let mut local = idle(LOCAL, &["a"]);
    local.sessions.push(outside("notes", 21, Status::Idle));
    s.apply(local);
    assert!(kinds(&s).contains(&"[kitty]".to_string()));
    s.apply(HostSnapshot {
        host: LOCAL.to_string(),
        online: false,
        error: None,
        sessions: Vec::new(),
    });
    assert_eq!(kinds(&s), ["[laptop]", "-", "[nas]", "x"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn claude_outside_tmux_on_another_host_marks_nothing_here() {
    let mut s = sidebar(&[LOCAL, "nas"]);
    let mut seen = Seen::new();
    let listing = |host: &str, notes: Status| {
        let mut snap = idle(host, &["a"]);
        snap.sessions.push(outside("notes", 21, notes));
        snap
    };
    // Both machines have one called `notes`. The other's works, this one's does nothing: it
    // did not finish anything.
    s.deliver(&mut seen, listing(LOCAL, Status::Idle), None);
    s.deliver(&mut seen, listing("nas", Status::Running), None);
    s.deliver(&mut seen, listing(LOCAL, Status::Idle), None);
    assert_eq!(s.status(&key(KITTY, "notes")), Some(Status::Idle));
    // Nor does the other's finishing end this one's work.
    s.deliver(&mut seen, listing(LOCAL, Status::Running), None);
    s.deliver(&mut seen, listing("nas", Status::Idle), None);
    assert_eq!(s.status(&key(KITTY, "notes")), Some(Status::Running));
    s.deliver(&mut seen, listing(LOCAL, Status::Running), None);
    assert_eq!(s.status(&key(KITTY, "notes")), Some(Status::Running));
    // This one's own end is marked as ever.
    s.deliver(&mut seen, listing(LOCAL, Status::Idle), None);
    assert_eq!(s.status(&key(KITTY, "notes")), Some(Status::Unseen));
    assert_eq!(
        kinds(&s),
        ["[laptop]", "a", "[kitty]", "notes", "-", "[nas]", "a"]
    );
}

#[test]
fn claude_outside_tmux_that_is_gone_is_forgotten() {
    let mut s = sidebar(&[LOCAL, "nas"]);
    let mut seen = Seen::new();
    let listing = |notes: Option<Status>| {
        let mut snap = idle(LOCAL, &["a"]);
        snap.sessions
            .extend(notes.map(|notes| outside("notes", 21, notes)));
        snap
    };
    s.deliver(&mut seen, listing(Some(Status::Running)), None);
    // It ends while it works; another host's listing in between changes nothing of that.
    s.deliver(&mut seen, listing(None), None);
    s.deliver(&mut seen, idle("nas", &["x"]), None);
    assert_eq!(s.status(&key(KITTY, "notes")), None);
    // One of its name that comes later did not finish anything.
    s.deliver(&mut seen, listing(Some(Status::Idle)), None);
    assert_eq!(s.status(&key(KITTY, "notes")), Some(Status::Idle));
}

#[test]
fn next_unread_wraps_and_never_returns_where_it_starts() {
    let mut s = sidebar(&[LOCAL]);
    s.apply(snapshot(
        LOCAL,
        vec![
            session(LOCAL, "a", Status::Idle),
            session(LOCAL, "b", Status::Idle),
            session(LOCAL, "c", Status::Idle),
        ],
    ));
    let unread = |k: &SessionKey| k.name == "a" || k.name == "c";
    assert_eq!(s.next_unread(None, &unread), Some(key(LOCAL, "a")));
    assert_eq!(
        s.next_unread(Some(&key(LOCAL, "a")), &unread),
        Some(key(LOCAL, "c"))
    );
    assert_eq!(
        s.next_unread(Some(&key(LOCAL, "c")), &unread),
        Some(key(LOCAL, "a")),
        "wraps round"
    );
    let only_a = |k: &SessionKey| k.name == "a";
    assert_eq!(
        s.next_unread(Some(&key(LOCAL, "a")), &only_a),
        None,
        "not the one it starts from"
    );
    assert_eq!(
        sidebar(&[LOCAL]).next_unread(None, &unread),
        None,
        "an empty list"
    );
}

#[test]
fn the_dim_line_is_the_reason_else_the_last_reply_else_the_title() {
    let mut info = claude(LOCAL, "work", "a title");
    info.status = Status::Idle;
    assert_eq!(summary(&info), Some(("a title", false)));
    info.last_reply = Some("All 12 tests pass.".to_string());
    assert_eq!(summary(&info), Some(("All 12 tests pass.", false)));
    info.status = Status::Waiting;
    info.reason = Some("permission prompt".to_string());
    assert_eq!(summary(&info), Some(("permission prompt", true)));
}

#[test]
fn renamed_outside_follows_claudes_session_id() {
    let id = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";
    let with = |name: &str, id: Option<&str>| SessionInfo {
        session_id: id.map(str::to_string),
        ..session("nas", name, Status::Idle)
    };
    let before = vec![with("old", Some(id)), with("plain", None)];
    let after = snapshot("nas", vec![with("new", Some(id)), with("plain2", None)]);
    assert_eq!(
        renamed_outside(&before, &after),
        [(key("nas", "old"), key("nas", "new"))]
    );
    // Still listed under its name: no rename. A host away says nothing. Another host's
    // listing is not this one's.
    let both = snapshot("nas", vec![with("old", Some(id)), with("new", Some(id))]);
    assert!(renamed_outside(&before, &both).is_empty());
    assert!(renamed_outside(&before, &offline("nas")).is_empty());
    assert!(renamed_outside(&before, &snapshot("ph", vec![with("new", Some(id))])).is_empty());
}

#[test]
fn renamed_outside_never_guesses_between_sessions() {
    let x = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";
    let with = |name: &str, id: Option<&str>| SessionInfo {
        session_id: id.map(str::to_string),
        ..session("nas", name, Status::Idle)
    };
    // A and B share a conversation (one resumed in two sessions); A is renamed to A2. The new
    // name is the one the list did not have: never B, which was there all along.
    let shared = vec![with("a", Some(x)), with("b", Some(x))];
    let after = snapshot("nas", vec![with("b", Some(x)), with("a2", Some(x))]);
    assert_eq!(
        renamed_outside(&shared, &after),
        [(key("nas", "a"), key("nas", "a2"))]
    );
    // Both renamed: which new name is whose is not known, so neither is followed.
    let both = snapshot("nas", vec![with("a2", Some(x)), with("b2", Some(x))]);
    assert!(renamed_outside(&shared, &both).is_empty());
    // Two old names, one new name with their id: ambiguous, nothing is followed.
    let one = snapshot("nas", vec![with("n", Some(x))]);
    assert!(renamed_outside(&shared, &one).is_empty());
    // Renamed onto a name the list already had (its session gone at the same time): that
    // name is another session's, not this one's.
    let before = vec![with("a", Some(x)), with("b", None)];
    let onto = snapshot("nas", vec![with("b", Some(x))]);
    assert!(renamed_outside(&before, &onto).is_empty());
    // Renamed, and the old name taken by a new session in the same listing: the old name is
    // still listed, so whatever has that name keeps it.
    let reused = snapshot(
        "nas",
        vec![with("a", None), with("a2", Some(x)), with("b", None)],
    );
    assert!(renamed_outside(&before, &reused).is_empty());
}
