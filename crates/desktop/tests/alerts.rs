//! Alerts: which changes of a session's mark alert, the bell's count, the list and its keys,
//! the notification's command, and the settings file. No window, no `notify-send`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use client_core::{Config, HostSnapshot, Seen, SessionInfo, SessionKey, Status, Tool};
use desktop::alerts::{
    Alerts, MAX_ALERTS, Notification, PanelResult, Settings, Watching, jump, notify_argv, send,
};
use desktop::forms::{self, Look};
use desktop::sidebar::{Row, Sidebar};
use s2_core::{Key, KeyEvent, Mods, Named};

const HOST: &str = "nas";

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

fn press(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        mods: Mods::default(),
        text: None,
        repeat: false,
        pressed: true,
    }
}

/// Nobody looks at anything: no tab in front.
const AWAY: Watching = Watching {
    front: None,
    window_focused: false,
};

/// One listing of `HOST` with these sessions.
fn list(alerts: &mut Alerts, sessions: &[(&str, Status)]) -> Vec<Notification> {
    list_as(alerts, sessions, AWAY)
}

fn list_as(alerts: &mut Alerts, sessions: &[(&str, Status)], who: Watching) -> Vec<Notification> {
    let sessions: Vec<SessionInfo> = sessions
        .iter()
        .map(|(name, status)| session(HOST, name, *status))
        .collect();
    alerts.observe(HOST, &sessions, who, Instant::now())
}

fn alerted(alerts: &Alerts) -> Vec<(&str, Status)> {
    alerts
        .entries()
        .iter()
        .map(|alert| (alert.key.name.as_str(), alert.status))
        .collect()
}

#[test]
fn a_session_turning_waiting_done_or_failed_alerts() {
    let mut alerts = Alerts::new(Settings::default());
    let working = [
        ("ask", Status::Running),
        ("build", Status::Running),
        ("crash", Status::Running),
        ("idle", Status::Idle),
    ];
    assert!(list(&mut alerts, &working).is_empty());
    assert_eq!(alerts.unread(), 0);

    let mut asks = session(HOST, "ask", Status::Waiting);
    asks.reason = Some("permission prompt".to_string());
    let now = [
        asks,
        session(HOST, "build", Status::Unseen),
        session(HOST, "crash", Status::Error),
        session(HOST, "idle", Status::Waiting),
    ];
    let notes = alerts.observe(HOST, &now, AWAY, Instant::now());
    // Newest first: the last of the listing is on top.
    assert_eq!(
        alerted(&alerts),
        [
            ("idle", Status::Waiting),
            ("crash", Status::Error),
            ("build", Status::Unseen),
            ("ask", Status::Waiting),
        ]
    );
    assert_eq!(alerts.unread(), 4);
    assert_eq!(
        alerts.entries()[3].reason.as_deref(),
        Some("permission prompt")
    );
    // A desktop notification for `?` and `◆`, not for `✕`; tmls's own words.
    let note = |body: &str| Notification {
        title: "tmls · nas".to_string(),
        body: body.to_string(),
    };
    assert_eq!(
        notes,
        [
            note("? ask · permission prompt"),
            note("◆ build · done"),
            note("? idle · waiting"),
        ]
    );
}

#[test]
fn what_is_not_news_does_not_alert() {
    let mut alerts = Alerts::new(Settings::default());
    let first = [
        ("a", Status::Running),
        ("b", Status::Waiting),
        ("c", Status::Unseen),
        ("d", Status::Idle),
        ("e", Status::Error),
    ];
    list(&mut alerts, &first);
    // The same marks again; a session that works again or simply ends; one that is new.
    for listing in [
        &first[..],
        &[
            ("a", Status::Idle),
            ("b", Status::Running),
            ("c", Status::Idle),
            ("d", Status::Running),
            ("e", Status::Idle),
            ("new", Status::Waiting),
        ],
    ] {
        assert!(list(&mut alerts, listing).is_empty(), "{listing:?}");
        assert_eq!(alerts.entries(), [], "{listing:?}");
    }
    assert_eq!(alerts.unread(), 0);
    // Known now, the new one is news when it changes.
    list(&mut alerts, &[("new", Status::Running)]);
    list(&mut alerts, &[("new", Status::Waiting)]);
    assert_eq!(alerted(&alerts), [("new", Status::Waiting)]);
}

#[test]
fn nothing_alerts_at_the_start_or_when_a_host_comes_back() {
    let mut alerts = Alerts::new(Settings::default());
    let needy = [
        ("a", Status::Waiting),
        ("b", Status::Unseen),
        ("c", Status::Error),
    ];
    // The first listing after the start: every session is unknown before.
    assert!(list(&mut alerts, &needy).is_empty());
    assert_eq!((alerts.unread(), alerts.entries().len()), (0, 0));
    // Offline (no sessions), then back with everything changed: unknown before again.
    list(
        &mut alerts,
        &[("a", Status::Running), ("b", Status::Running)],
    );
    list(&mut alerts, &[]);
    assert!(list(&mut alerts, &needy).is_empty());
    assert_eq!((alerts.unread(), alerts.entries().len()), (0, 0));
}

#[test]
fn a_listing_of_another_host_changes_nothing_here() {
    let mut alerts = Alerts::new(Settings::default());
    list(&mut alerts, &[("a", Status::Running)]);
    let other = [session("laptop", "a", Status::Waiting)];
    assert!(
        alerts
            .observe("laptop", &other, AWAY, Instant::now())
            .is_empty()
    );
    assert!(
        alerts
            .observe("laptop", &[], AWAY, Instant::now())
            .is_empty()
    );
    list(&mut alerts, &[("a", Status::Waiting)]);
    assert_eq!(alerts.entries().len(), 1);
    assert_eq!(alerts.entries()[0].key, key(HOST, "a"));
}

#[test]
fn the_bell_counts_what_is_new_and_opening_the_list_reads_it() {
    let mut alerts = Alerts::new(Settings::default());
    list(
        &mut alerts,
        &[("a", Status::Running), ("b", Status::Running)],
    );
    list(
        &mut alerts,
        &[("a", Status::Waiting), ("b", Status::Running)],
    );
    assert_eq!(alerts.unread(), 1);
    list(
        &mut alerts,
        &[("a", Status::Waiting), ("b", Status::Unseen)],
    );
    assert_eq!(alerts.unread(), 2);
    let panel = alerts.open(Instant::now());
    assert_eq!(alerts.unread(), 0, "opened: read");
    assert_eq!(alerts.entries().len(), 2, "and kept");
    assert_eq!(
        panel.selected().map(|alert| &alert.key),
        Some(&key(HOST, "b"))
    );
    // What comes while the list is open is new again.
    list(
        &mut alerts,
        &[("a", Status::Running), ("b", Status::Unseen)],
    );
    list(&mut alerts, &[("a", Status::Error), ("b", Status::Unseen)]);
    assert_eq!(alerts.unread(), 1);
    assert_eq!(desktop::draw::bell_label(0), "!");
    assert_eq!(desktop::draw::bell_label(alerts.unread()), "!1");
    assert_eq!(desktop::draw::bell_label(MAX_ALERTS), "!50");
}

#[test]
fn only_the_newest_fifty_are_kept() {
    let mut alerts = Alerts::new(Settings::default());
    for round in 0..MAX_ALERTS + 7 {
        list(&mut alerts, &[("a", Status::Running)]);
        let mut waits = session(HOST, "a", Status::Waiting);
        waits.reason = Some(format!("round {round}"));
        alerts.observe(HOST, &[waits], AWAY, Instant::now());
    }
    assert_eq!(alerts.entries().len(), MAX_ALERTS);
    assert_eq!(
        alerts.unread(),
        MAX_ALERTS,
        "the bell counts no more than the list has"
    );
    assert_eq!(alerts.entries()[0].reason.as_deref(), Some("round 56"));
    assert_eq!(
        alerts.entries()[MAX_ALERTS - 1].reason.as_deref(),
        Some("round 7")
    );
}

#[test]
fn silence_focused_skips_the_tab_the_user_is_looking_at() {
    let front = key(HOST, "a");
    let looking = Watching {
        front: Some(&front),
        window_focused: true,
    };
    let elsewhere = Watching {
        front: Some(&front),
        window_focused: false,
    };
    let working = [("a", Status::Running), ("b", Status::Running)];
    let waiting = [("a", Status::Waiting), ("b", Status::Waiting)];

    // Looking at `a`: only `b` alerts, and `a` is not kept for later either.
    let mut alerts = Alerts::new(Settings::default());
    list_as(&mut alerts, &working, looking);
    let notes = list_as(&mut alerts, &waiting, looking);
    assert_eq!(alerted(&alerts), [("b", Status::Waiting)]);
    assert_eq!((notes.len(), alerts.unread()), (1, 1));
    list_as(&mut alerts, &waiting, elsewhere);
    assert_eq!(
        alerts.entries().len(),
        1,
        "no late alert when the window loses the focus"
    );

    // The window is not focused: the tab in front alerts like any other.
    let mut alerts = Alerts::new(Settings::default());
    list_as(&mut alerts, &working, elsewhere);
    assert_eq!(list_as(&mut alerts, &waiting, elsewhere).len(), 2);
    // The list gives the tab in front no ◆; unwatched, its end is an alert all the same.
    list_as(&mut alerts, &working, elsewhere);
    let notes = list_as(
        &mut alerts,
        &[("a", Status::Idle), ("b", Status::Idle)],
        elsewhere,
    );
    assert_eq!(alerts.entries()[0].key, front);
    assert_eq!(alerts.entries()[0].status, Status::Unseen);
    assert_eq!(notes.len(), 1, "b only went idle");
    assert_eq!(notes[0].body, "◆ a · done");
    assert_eq!(alerts.entries().len(), 3);
    // Idle stays idle: once.
    list_as(
        &mut alerts,
        &[("a", Status::Idle), ("b", Status::Idle)],
        elsewhere,
    );
    assert_eq!(alerts.entries().len(), 3);

    // The switch off: the watched tab alerts when it waits, but it does not "finish unwatched".
    let all = Settings {
        silence_focused: false,
        ..Settings::default()
    };
    let mut alerts = Alerts::new(all);
    list_as(&mut alerts, &working, looking);
    assert_eq!(list_as(&mut alerts, &waiting, looking).len(), 2);
    list_as(&mut alerts, &working, looking);
    list_as(
        &mut alerts,
        &[("a", Status::Idle), ("b", Status::Running)],
        looking,
    );
    assert_eq!(alerts.entries().len(), 2);
}

#[test]
fn the_desktop_switch_off_keeps_the_list_and_sends_nothing() {
    let quiet = Settings {
        desktop: false,
        ..Settings::default()
    };
    let mut alerts = Alerts::new(quiet);
    list(&mut alerts, &[("a", Status::Running)]);
    assert!(list(&mut alerts, &[("a", Status::Waiting)]).is_empty());
    assert_eq!(alerts.unread(), 1);
}

#[test]
fn the_notification_is_one_command_without_a_shell() {
    let mut alerts = Alerts::new(Settings::default());
    let hostile = "-u critical; $(reboot) 'a b\"c";
    let host = "--help";
    alerts.observe(
        host,
        &[session(host, hostile, Status::Running)],
        AWAY,
        Instant::now(),
    );
    let notes = alerts.observe(
        host,
        &[session(host, hostile, Status::Unseen)],
        AWAY,
        Instant::now(),
    );
    assert_eq!(notes.len(), 1);
    // Title and body are an argument each, after `--`: neither is an option, whatever the name.
    assert_eq!(
        notify_argv(&notes[0]),
        [
            "notify-send",
            "--",
            "tmls · --help",
            "◆ -u critical; $(reboot) 'a b\"c · done",
        ]
    );
}

#[test]
fn a_machine_without_notify_send_shows_no_notification_and_no_error() {
    let notes = [
        Notification {
            title: "tmls · nas".to_string(),
            body: "? a · waiting".to_string(),
        },
        Notification {
            title: "tmls · nas".to_string(),
            body: "◆ b · done".to_string(),
        },
    ];
    // The stand-in: what was asked for, and "no such file" as the answer.
    let mut asked = Vec::new();
    send(&notes, &mut |argv| {
        asked.push(argv.to_vec());
        Err(std::io::ErrorKind::NotFound.into())
    });
    assert_eq!(asked, [notify_argv(&notes[0]), notify_argv(&notes[1])]);
    // The app's own spawner with a program that does not exist: an error to ignore, no panic.
    let missing = ["tmlsrs-test-no-such-notify-send".to_string()];
    assert!(forms::spawn_detached(&missing).is_err());
    send(&notes, &mut |_| forms::spawn_detached(&missing));
}

fn scratch(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("tmlsrs-test-alerts-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn settings_are_read_from_notifications_json_and_never_written() {
    let on = Settings {
        desktop: true,
        silence_focused: true,
    };
    assert_eq!(Settings::default(), on);
    // No file: the defaults, and still no file.
    let dir = scratch("settings");
    assert_eq!(Settings::load(&dir), on);
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "nothing is created"
    );
    assert_eq!(Settings::load(&dir.join("no-such-dir")), on);
    assert!(!dir.join("no-such-dir").exists());

    // tmls's own file.
    let file = dir.join("notifications.json");
    let text = "{\"desktop\": false, \"sound\": true, \"silence_focused\": false}\n";
    std::fs::write(&file, text).unwrap();
    let off = Settings {
        desktop: false,
        silence_focused: false,
    };
    assert_eq!(Settings::load(&dir), off);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        text,
        "left as it is"
    );

    // Damaged, not an object, a value that is no boolean, a key missing: that key's default.
    for (text, want) in [
        ("{\"desktop\": fal", on),
        ("", on),
        ("[false, false]", on),
        ("\"desktop\"", on),
        ("{\"desktop\": 0, \"silence_focused\": \"no\"}", on),
        (
            "{\"desktop\": false}",
            Settings {
                desktop: false,
                silence_focused: true,
            },
        ),
        (
            "{\"silence_focused\": false, \"desktop\": null}",
            Settings {
                desktop: true,
                silence_focused: false,
            },
        ),
    ] {
        assert_eq!(Settings::parse(text), want, "{text}");
    }
    std::fs::write(&file, "{\"desktop\": fal").unwrap();
    assert_eq!(Settings::load(&dir), on);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
}

/// Alerts for `count` sessions `s0`.., the last one newest; the second one an error.
fn with_alerts(count: usize) -> Alerts {
    let mut alerts = Alerts::new(Settings::default());
    let names: Vec<String> = (0..count).map(|n| format!("s{n}")).collect();
    let all = |status: Status| -> Vec<SessionInfo> {
        names
            .iter()
            .map(|name| session(HOST, name, status))
            .collect()
    };
    let start = Instant::now();
    alerts.observe(HOST, &all(Status::Running), AWAY, start);
    let mut now = all(Status::Waiting);
    if let Some(second) = now.get_mut(1) {
        second.status = Status::Error;
    }
    alerts.observe(HOST, &now, AWAY, start);
    alerts
}

fn lines(panel: &forms::Panel) -> Vec<&str> {
    panel.cells.iter().map(|cell| cell.text.as_str()).collect()
}

#[test]
fn the_panel_lists_the_alerts_newest_first() {
    let mut alerts = with_alerts(3);
    let opened = alerts.entries()[0].at + Duration::from_secs(5 * 60 + 10);
    let panel = alerts.open(opened).panel();
    assert_eq!((panel.cols, panel.lines), (60, 4));
    assert_eq!(
        lines(&panel),
        [
            "Alerts · Enter jumps · Esc closes",
            "  5m  ? s2 · nas  waiting",
            "  5m  ✕ s1 · nas  error",
            "  5m  ? s0 · nas  waiting",
        ]
    );
    // The cursor is on the newest; the error is red.
    let looks: Vec<Look> = panel.cells[1..].iter().map(|cell| cell.look).collect();
    let item = |hovered, danger| Look::Item { hovered, danger };
    assert_eq!(
        looks,
        [item(true, false), item(false, true), item(false, false)]
    );

    // No alert yet: it says so, and Enter does nothing.
    let mut none = Alerts::new(Settings::default());
    let mut empty = none.open(Instant::now());
    assert_eq!(lines(&empty.panel())[1], "nothing yet");
    assert_eq!(empty.panel().lines, 2);
    assert_eq!(
        empty.key(&press(Key::Named(Named::Enter))),
        PanelResult::Pending
    );
    assert_eq!(empty.key(&press(Key::Char('j'))), PanelResult::Pending);
    assert_eq!(
        empty.key(&press(Key::Named(Named::Escape))),
        PanelResult::Closed
    );
}

#[test]
fn keys_and_clicks_in_the_panel_jump_to_the_session() {
    let mut alerts = with_alerts(3);
    let mut panel = alerts.open(Instant::now());
    let name = |panel: &desktop::alerts::AlertsPanel| panel.selected().unwrap().key.name.clone();
    // Down / j and Up / k move and stop at the ends; other keys do nothing and close nothing.
    assert_eq!(
        panel.key(&press(Key::Named(Named::Up))),
        PanelResult::Pending
    );
    assert_eq!(name(&panel), "s2");
    panel.key(&press(Key::Char('j')));
    assert_eq!(name(&panel), "s1");
    panel.key(&press(Key::Named(Named::Down)));
    panel.key(&press(Key::Named(Named::Down)));
    assert_eq!(name(&panel), "s0");
    panel.key(&press(Key::Char('k')));
    assert_eq!(name(&panel), "s1");
    assert_eq!(panel.key(&press(Key::Char('x'))), PanelResult::Pending);
    assert_eq!(
        panel.key(&press(Key::Named(Named::Tab))),
        PanelResult::Pending
    );
    // Enter jumps, once per press; a release does nothing.
    let enter = press(Key::Named(Named::Enter));
    let held = KeyEvent {
        repeat: true,
        ..enter.clone()
    };
    let released = KeyEvent {
        pressed: false,
        ..enter.clone()
    };
    assert_eq!(panel.key(&held), PanelResult::Pending);
    assert_eq!(panel.key(&released), PanelResult::Pending);
    assert_eq!(panel.key(&enter), PanelResult::Jump(key(HOST, "s1")));
    assert_eq!(
        panel.key(&press(Key::Named(Named::Escape))),
        PanelResult::Closed
    );

    // The pointer: over a line the cursor follows, a click jumps; the title does nothing; a
    // click beside the panel closes it.
    assert!(panel.hover(5.0, 3.5));
    assert!(!panel.hover(6.0, 3.2));
    assert_eq!(name(&panel), "s0");
    assert!(!panel.hover(5.0, 0.5), "the title");
    assert_eq!(panel.click(59.5, 1.0), PanelResult::Jump(key(HOST, "s2")));
    assert_eq!(panel.click(3.0, 0.5), PanelResult::Pending);
    assert_eq!(panel.click(60.5, 1.0), PanelResult::Closed);
    assert_eq!(panel.click(3.0, 4.0), PanelResult::Closed);
    assert_eq!(panel.click(-0.5, 1.0), PanelResult::Closed);
}

#[test]
fn a_long_list_follows_the_cursor() {
    let mut alerts = with_alerts(MAX_ALERTS);
    let mut panel = alerts.open(Instant::now());
    assert_eq!(panel.panel().lines, 13, "the title and twelve alerts");
    assert!(lines(&panel.panel())[1].contains(" s49 "));
    for _ in 0..MAX_ALERTS + 3 {
        panel.key(&press(Key::Named(Named::Down)));
    }
    assert_eq!(panel.selected().unwrap().key, key(HOST, "s0"));
    let shown = panel.panel();
    assert_eq!(shown.lines, 13);
    assert!(lines(&shown)[1].contains(" s11 "));
    assert!(lines(&shown)[12].contains(" s0 "));
    // A click on the last line shown is that alert.
    assert_eq!(panel.click(2.0, 12.5), PanelResult::Jump(key(HOST, "s0")));
    assert_eq!(panel.click(2.0, 1.5), PanelResult::Jump(key(HOST, "s11")));
}

#[test]
fn a_jump_to_a_session_that_is_gone_says_so() {
    let gone = key(HOST, "old name");
    // Listed, or a tab of it is still open (ended, perhaps): its tab is opened.
    assert_eq!(jump(&gone, true, false), Ok(()));
    assert_eq!(jump(&gone, false, true), Ok(()));
    // Renamed or killed since the alert: a line for the status line, nothing opened.
    assert_eq!(
        jump(&gone, false, false),
        Err("old name on nas is gone (renamed or ended)".to_string())
    );
}

/// A listing the way the app handles it: `Sidebar::deliver`, then the host's rows to `observe`.
fn deliver(
    list: &mut Sidebar,
    seen: &mut Seen,
    alerts: &mut Alerts,
    sessions: &[(&str, Status)],
    who: Watching,
) -> Vec<Notification> {
    let snapshot = HostSnapshot {
        host: HOST.to_string(),
        online: true,
        error: None,
        sessions: sessions
            .iter()
            .map(|(name, status)| session(HOST, name, *status))
            .collect(),
    };
    list.deliver(seen, snapshot, who.front);
    let listed = list.rows().iter().filter_map(|row| match row {
        Row::Session(info) if info.key.host == HOST => Some(info),
        _ => None,
    });
    alerts.observe(HOST, listed, who, Instant::now())
}

#[test]
fn a_session_that_finishes_in_the_list_alerts_and_its_alert_outlives_it() {
    let cfg = Config {
        local_name: "laptop".to_string(),
        hosts: vec!["laptop".to_string(), HOST.to_string()],
        host_colors: BTreeMap::new(),
        presets: Vec::new(),
        pins: BTreeSet::new(),
    };
    let mut list = Sidebar::new(&cfg, PathBuf::from("/nonexistent/tmlsrs-test"));
    let (mut seen, mut alerts) = (Seen::new(), Alerts::new(Settings::default()));
    let front = key(HOST, "front");
    let looking = Watching {
        front: Some(&front),
        window_focused: true,
    };
    let working = [("front", Status::Running), ("back", Status::Running)];
    let idle = [("front", Status::Idle), ("back", Status::Idle)];
    assert!(deliver(&mut list, &mut seen, &mut alerts, &working, looking).is_empty());
    // Both end: the one in the back finished unwatched (◆), the one in front was watched.
    let notes = deliver(&mut list, &mut seen, &mut alerts, &idle, looking);
    assert_eq!(list.status(&key(HOST, "back")), Some(Status::Unseen));
    assert_eq!(alerted(&alerts), [("back", Status::Unseen)]);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body, "◆ back · done");
    // The ◆ stays in the list; it alerts once.
    assert!(deliver(&mut list, &mut seen, &mut alerts, &idle, looking).is_empty());
    assert_eq!(alerts.unread(), 1);

    // The session is renamed elsewhere: its alert is still listed, and a jump says it is gone.
    let renamed = [("front", Status::Idle), ("other name", Status::Idle)];
    assert!(deliver(&mut list, &mut seen, &mut alerts, &renamed, looking).is_empty());
    let mut panel = alerts.open(Instant::now());
    let PanelResult::Jump(to) = panel.key(&press(Key::Named(Named::Enter))) else {
        panic!("Enter on an alert jumps");
    };
    assert_eq!(to, key(HOST, "back"));
    assert_eq!(
        jump(&to, list.status(&to).is_some(), false),
        Err("back on nas is gone (renamed or ended)".to_string())
    );
    assert_eq!(jump(&front, list.status(&front).is_some(), false), Ok(()));
}
