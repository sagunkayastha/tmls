//! Alerts: which changes of a session's mark alert, the bell's count, the list and its keys,
//! the notification's and the sound's command, the switches and the settings file. No window,
//! no `notify-send`, no sound.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use client_core::{Config, HostSnapshot, Seen, SessionInfo, SessionKey, Status, Tool};
use desktop::alerts::{
    self, Alerts, MAX_ALERTS, Notification, PanelResult, SOUND_PLAYER, Settings, Watching, emit,
    jump, notify_argv, send, sound_argv,
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
        pid: None,
        session_id: None,
        pane: None,
        activity: None,
        last_reply: None,
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
    alerts.observe(HOST, true, &sessions, who, Instant::now())
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
    let notes = alerts.observe(HOST, true, &now, AWAY, Instant::now());
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
fn nothing_alerts_at_the_start_and_an_offline_listing_loses_no_alert() {
    let mut alerts = Alerts::new(Settings::default());
    let needy = [
        ("a", Status::Waiting),
        ("b", Status::Unseen),
        ("c", Status::Error),
    ];
    // The first listing after the start: every session is unknown before.
    assert!(list(&mut alerts, &needy).is_empty());
    assert_eq!((alerts.unread(), alerts.entries().len()), (0, 0));

    // The host does not answer once (an ssh timeout): nothing alerts, nothing is forgotten.
    let working = [
        ("a", Status::Running),
        ("b", Status::Running),
        ("c", Status::Running),
        ("d", Status::Waiting),
        ("e", Status::Running),
    ];
    list(&mut alerts, &working);
    assert!(
        alerts
            .observe(HOST, false, &[], AWAY, Instant::now())
            .is_empty()
    );
    assert!(
        alerts
            .observe(HOST, false, &[], AWAY, Instant::now())
            .is_empty()
    );
    assert_eq!(alerts.entries().len(), 0);
    // It is back: what changed across the gap alerts, once; what did not change does not,
    // nor does a session that is new. `e` is gone and forgotten.
    let back = [
        ("a", Status::Waiting),
        ("b", Status::Unseen),
        ("c", Status::Running),
        ("d", Status::Waiting),
        ("new", Status::Waiting),
    ];
    let notes = list(&mut alerts, &back);
    assert_eq!(
        alerted(&alerts),
        [("b", Status::Unseen), ("a", Status::Waiting)]
    );
    assert_eq!((notes.len(), alerts.unread()), (2, 2));
    assert!(list(&mut alerts, &back).is_empty());
    // `e` again, already waiting: unknown before, as it was forgotten when the host was back.
    list(&mut alerts, &[("e", Status::Waiting)]);
    assert_eq!(alerts.entries().len(), 2);
}

#[test]
fn a_listing_of_another_host_changes_nothing_here() {
    let mut alerts = Alerts::new(Settings::default());
    list(&mut alerts, &[("a", Status::Running)]);
    let other = [session("laptop", "a", Status::Waiting)];
    assert!(
        alerts
            .observe("laptop", true, &other, AWAY, Instant::now())
            .is_empty()
    );
    assert!(
        alerts
            .observe("laptop", false, &[], AWAY, Instant::now())
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
        alerts.observe(HOST, true, &[waits], AWAY, Instant::now());
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
        true,
        &[session(host, hostile, Status::Running)],
        AWAY,
        Instant::now(),
    );
    let notes = alerts.observe(
        host,
        true,
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

/// A directory of this test's own under cargo's target tmp dir, new at every run (in a
/// container the process id is the same each time); never `~/.config/tmls`, never /tmp.
fn scratch(test: &str) -> PathBuf {
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "tmlsrs-test-alerts-{}-{run}-{test}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn settings_are_read_from_notifications_json_and_written_when_switched() {
    let on = Settings {
        desktop: true,
        sound: false,
        silence_focused: true,
    };
    assert_eq!(Settings::default(), on);
    // No file: the defaults, and loading makes none.
    let dir = scratch("settings");
    assert_eq!(Settings::load(&dir), on);
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "loading creates nothing"
    );
    // tmls's own file.
    let file = dir.join("notifications.json");
    std::fs::write(
        &file,
        "{\"desktop\": false, \"sound\": true, \"silence_focused\": false}\n",
    )
    .unwrap();
    let off = Settings {
        desktop: false,
        sound: true,
        silence_focused: false,
    };
    assert_eq!(Settings::load(&dir), off);
    // Damaged, not an object, a value that is no boolean, a key missing: that key's default.
    for (text, want) in [
        ("{\"desktop\": fal", on),
        ("", on),
        ("[false, false]", on),
        (
            "{\"desktop\": 0, \"sound\": \"yes\", \"silence_focused\": \"no\"}",
            on,
        ),
        ("{\"sound\": true}", Settings { sound: true, ..on }),
        (
            "{\"silence_focused\": false, \"desktop\": null}",
            Settings {
                silence_focused: false,
                ..on
            },
        ),
    ] {
        assert_eq!(Settings::parse(text), want, "{text}");
    }
    // Written as tmls writes it, whole, with no temp file left beside it.
    let saved = Settings {
        desktop: true,
        sound: true,
        silence_focused: false,
    };
    let nothing_else = serde_json::Map::new();
    saved.save(&dir, &nothing_else).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{\"desktop\": true, \"sound\": true, \"silence_focused\": false}\n"
    );
    assert_eq!(Settings::load(&dir), saved);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    // A config directory that does not exist yet is made.
    let fresh = dir.join("fresh");
    on.save(&fresh, &nothing_else).unwrap();
    assert_eq!(Settings::load(&fresh), on);
}

#[test]
fn a_switch_that_cannot_be_saved_stays_and_says_why() {
    let on = Settings::default();
    let wanted = Settings { sound: true, ..on };
    const ON: &str = "{\"desktop\": true, \"sound\": false, \"silence_focused\": true}\n";
    // A read-only config directory: the save fails, the old file stays.
    // (The test container runs as uid 1000: the mode is enforced.)
    let dir = scratch("readonly");
    let file = dir.join("notifications.json");
    std::fs::write(&file, ON).unwrap();
    std::fs::set_permissions(&dir, Permissions::from_mode(0o500)).unwrap();
    let (kept, said) = alerts::switch(on, wanted, &dir, true);
    std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).unwrap();
    assert_eq!(kept, on);
    assert!(
        said.unwrap_err()
            .starts_with("couldn't save the alert switches: ")
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), ON);
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        1,
        "no temp file left"
    );
    // Saved: the new settings, and the status line says which switch.
    assert_eq!(
        alerts::switch(on, wanted, &dir, true),
        (wanted, Ok("Sound ON".to_string()))
    );
    assert_eq!(Settings::load(&dir), wanted);
    assert_eq!(
        alerts::switch(wanted, on, &dir, true),
        (on, Ok("Sound OFF".to_string()))
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), ON);
    // Sound with no player: on all the same, and it says there will be none.
    assert_eq!(
        alerts::switch(on, wanted, &dir, false),
        (
            wanted,
            Ok(format!(
                "Sound ON, but {SOUND_PLAYER} is not installed: no sound"
            ))
        )
    );
    // No file at all: the defaults, and the flip makes it.
    let fresh = scratch("first-flip");
    let quiet = Settings {
        desktop: false,
        ..on
    };
    assert_eq!(
        alerts::switch(on, quiet, &fresh, false),
        (quiet, Ok("Desktop OFF".to_string()))
    );
    assert_eq!(
        std::fs::read_to_string(fresh.join("notifications.json")).unwrap(),
        "{\"desktop\": false, \"sound\": false, \"silence_focused\": true}\n"
    );
}

#[test]
fn a_flip_changes_its_one_switch_in_the_file_as_it_is_now() {
    // Read at start: all defaults.
    let dir = scratch("flip-now");
    let file = dir.join("notifications.json");
    let at_start = Settings::load(&dir);
    // Since then tmls turned Desktop off, and the file has keys tmls-rs does not know.
    std::fs::write(
        &file,
        "{\"desktop\": false, \"sound\": false, \"silence_focused\": true, \"volume\": 3, \"extra\": {\"a\": [1, \"x\"]}}\n",
    )
    .unwrap();
    let wanted = Settings {
        sound: true,
        ..at_start
    };
    let (now, said) = alerts::switch(at_start, wanted, &dir, true);
    assert_eq!(said, Ok("Sound ON".to_string()));
    assert_eq!(
        now,
        Settings {
            desktop: false,
            sound: true,
            silence_focused: true
        },
        "Sound as flipped; Desktop as tmls left it, not as it was at start"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{\"desktop\": false, \"sound\": true, \"silence_focused\": true, \"extra\": {\"a\":[1,\"x\"]}, \"volume\": 3}\n",
        "tmls's three keys as tmls writes them, then what else the file had"
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn a_settings_file_that_cannot_be_understood_is_left_alone_and_the_flip_refused() {
    let on = Settings::default();
    let wanted = Settings { sound: true, ..on };
    for damaged in ["{\"desktop\": fal", "", "[false, false]", "\"on\""] {
        let dir = scratch("damaged");
        let file = dir.join("notifications.json");
        std::fs::write(&file, damaged).unwrap();
        assert_eq!(
            alerts::switch(on, wanted, &dir, true),
            (
                on,
                Err(
                    "couldn't switch: notifications.json is not a JSON object; it is left as it is"
                        .to_string()
                )
            ),
            "{damaged:?}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), damaged);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }
    // There, but not to be read (uid 1000: the mode is enforced).
    let dir = scratch("unreadable");
    let file = dir.join("notifications.json");
    std::fs::write(&file, "{\"sound\": false}").unwrap();
    std::fs::set_permissions(&file, Permissions::from_mode(0o000)).unwrap();
    let (kept, said) = alerts::switch(on, wanted, &dir, true);
    std::fs::set_permissions(&file, Permissions::from_mode(0o600)).unwrap();
    assert_eq!(kept, on);
    let said = said.unwrap_err();
    assert!(
        said.starts_with("couldn't switch: notifications.json cannot be read: ")
            && said.ends_with("; it is left as it is"),
        "{said}"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{\"sound\": false}"
    );
}

#[test]
fn a_symlinked_settings_file_stays_a_link_and_keeps_its_mode() {
    // Dotfiles kept elsewhere: notifications.json is a link to the real file.
    let dir = scratch("linked");
    std::fs::create_dir_all(dir.join("real")).unwrap();
    let real = dir.join("real/notes.json");
    std::fs::write(&real, "{\"sound\": false}\n").unwrap();
    std::fs::set_permissions(&real, Permissions::from_mode(0o600)).unwrap();
    let config = dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    let link = config.join("notifications.json");
    std::os::unix::fs::symlink("../real/notes.json", &link).unwrap();
    let on = Settings::default();
    let wanted = Settings { sound: true, ..on };
    assert_eq!(
        alerts::switch(on, wanted, &config, true),
        (wanted, Ok("Sound ON".to_string()))
    );
    assert!(link.is_symlink(), "the link is not replaced by a file");
    assert_eq!(
        std::fs::read_to_string(&real).unwrap(),
        "{\"desktop\": true, \"sound\": true, \"silence_focused\": true}\n"
    );
    assert_eq!(
        std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let count = |dir: &std::path::Path| std::fs::read_dir(dir).unwrap().count();
    assert_eq!(
        (count(&config), count(&dir.join("real"))),
        (1, 1),
        "no temp file left"
    );
}

#[test]
fn each_alert_runs_what_the_switches_ask() {
    let note = Notification {
        title: "tmls · nas".to_string(),
        body: "? s · waiting".to_string(),
    };
    assert_eq!(
        sound_argv(),
        [
            "canberra-gtk-play",
            "-i",
            "message-new-instant",
            "-d",
            "tmls"
        ]
    );
    let run = |settings: Settings, player: bool| {
        let mut started: Vec<Vec<String>> = Vec::new();
        emit(
            std::slice::from_ref(&note),
            settings,
            player,
            &mut |argv: &[String]| {
                started.push(argv.to_vec());
                Ok(())
            },
        );
        started
    };
    let on = Settings::default();
    assert_eq!(run(on, true), [notify_argv(&note)]);
    let both = Settings { sound: true, ..on };
    assert_eq!(run(both, true), [notify_argv(&note), sound_argv()]);
    assert_eq!(
        run(both, false),
        [notify_argv(&note)],
        "no player: no sound, no error"
    );
    let sound_only = Settings {
        desktop: false,
        sound: true,
        ..on
    };
    assert_eq!(run(sound_only, true), [sound_argv()]);
    // Sound alone still makes `observe` hand out the alerts' notes.
    let mut alerts = Alerts::new(sound_only);
    list(&mut alerts, &[("s", Status::Running)]);
    assert_eq!(list(&mut alerts, &[("s", Status::Waiting)]).len(), 1);
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
    alerts.observe(HOST, true, &all(Status::Running), AWAY, start);
    let mut now = all(Status::Waiting);
    if let Some(second) = now.get_mut(1) {
        second.status = Status::Error;
    }
    alerts.observe(HOST, true, &now, AWAY, start);
    alerts
}

fn lines(panel: &forms::Panel) -> Vec<&str> {
    panel.cells.iter().map(|cell| cell.text.as_str()).collect()
}

#[test]
fn the_panel_shows_the_switches_then_the_alerts_newest_first() {
    let mut alerts = with_alerts(3);
    let opened = alerts.entries()[0].at + Duration::from_secs(5 * 60 + 10);
    let panel = alerts.open(opened).panel();
    assert_eq!((panel.cols, panel.lines), (60, 7));
    assert_eq!(
        lines(&panel),
        [
            "Alerts · Enter jumps · Space switches · Esc closes",
            " Desktop ON",
            " Sound OFF",
            " Silence focused ON",
            "  5m  ? s2 · nas  waiting",
            "  5m  ✕ s1 · nas  error",
            "  5m  ? s0 · nas  waiting",
        ]
    );
    // The cursor is on the newest alert; the error is red.
    let looks: Vec<Look> = panel.cells[1..].iter().map(|cell| cell.look).collect();
    let item = |hovered, danger| Look::Item { hovered, danger };
    assert_eq!(
        looks,
        [
            item(false, false),
            item(false, false),
            item(false, false),
            item(true, false),
            item(false, true),
            item(false, false)
        ]
    );
    // No alert yet: the cursor is on Desktop; Enter does nothing there, Space flips it.
    let mut none = Alerts::new(Settings::default());
    let mut empty = none.open(Instant::now());
    assert_eq!(lines(&empty.panel())[4], "nothing yet");
    assert_eq!(empty.panel().lines, 5);
    assert_eq!(
        empty.key(&press(Key::Named(Named::Enter))),
        PanelResult::Pending
    );
    assert_eq!(
        empty.key(&press(Key::Char(' '))),
        PanelResult::Settings(Settings {
            desktop: false,
            ..Settings::default()
        })
    );
    assert_eq!(lines(&empty.panel())[1], " Desktop OFF");
    // Down stops at the last switch: "nothing yet" is no row.
    for _ in 0..5 {
        empty.key(&press(Key::Named(Named::Down)));
    }
    assert_eq!(
        empty.key(&press(Key::Char(' '))),
        PanelResult::Settings(Settings {
            desktop: false,
            silence_focused: false,
            ..Settings::default()
        })
    );
    assert_eq!(
        empty.key(&press(Key::Named(Named::Escape))),
        PanelResult::Closed
    );
}

#[test]
fn keys_and_clicks_in_the_panel_jump_to_the_session_or_flip_a_switch() {
    let mut alerts = with_alerts(3);
    let mut panel = alerts.open(Instant::now());
    let name =
        |panel: &desktop::alerts::AlertsPanel| panel.selected().map(|alert| alert.key.name.clone());
    assert_eq!(name(&panel), Some("s2".to_string()));
    panel.key(&press(Key::Char('j')));
    assert_eq!(name(&panel), Some("s1".to_string()));
    panel.key(&press(Key::Named(Named::Down)));
    panel.key(&press(Key::Named(Named::Down)));
    assert_eq!(name(&panel), Some("s0".to_string()));
    panel.key(&press(Key::Char('k')));
    assert_eq!(name(&panel), Some("s1".to_string()));
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
    // Up past the newest alert: the switches. Space flips the one under the cursor; the
    // panel shows it at once.
    panel.key(&press(Key::Char('k')));
    panel.key(&press(Key::Named(Named::Up)));
    assert_eq!(name(&panel), None);
    assert_eq!(
        panel.key(&enter),
        PanelResult::Pending,
        "Enter on a switch does nothing"
    );
    let quiet = Settings {
        silence_focused: false,
        ..Settings::default()
    };
    assert_eq!(
        panel.key(&press(Key::Char(' '))),
        PanelResult::Settings(quiet)
    );
    assert_eq!(lines(&panel.panel())[3], " Silence focused OFF");
    for _ in 0..4 {
        panel.key(&press(Key::Named(Named::Up)));
    }
    assert_eq!(
        panel.key(&press(Key::Char(' '))),
        PanelResult::Settings(Settings {
            desktop: false,
            ..quiet
        }),
        "the cursor stops at Desktop"
    );
    assert_eq!(
        panel.key(&press(Key::Named(Named::Escape))),
        PanelResult::Closed
    );
    // The pointer: line 0 the title, 1–3 the switches, 4–6 the alerts.
    assert!(panel.hover(5.0, 6.5));
    assert!(!panel.hover(6.0, 6.2));
    assert_eq!(name(&panel), Some("s0".to_string()));
    assert!(!panel.hover(5.0, 0.5), "the title");
    assert_eq!(panel.click(59.5, 4.0), PanelResult::Jump(key(HOST, "s2")));
    assert_eq!(
        panel.click(3.0, 2.5),
        PanelResult::Settings(Settings {
            desktop: false,
            sound: true,
            silence_focused: false
        })
    );
    assert_eq!(panel.click(3.0, 0.5), PanelResult::Pending);
    assert_eq!(panel.click(60.5, 4.0), PanelResult::Closed);
    assert_eq!(panel.click(3.0, 7.0), PanelResult::Closed);
    assert_eq!(panel.click(-0.5, 4.0), PanelResult::Closed);
}

#[test]
fn a_long_list_follows_the_cursor() {
    let mut alerts = with_alerts(MAX_ALERTS);
    let mut panel = alerts.open(Instant::now());
    assert_eq!(
        panel.panel().lines,
        16,
        "the title, three switches and twelve alerts"
    );
    assert!(lines(&panel.panel())[4].contains(" s49 "));
    for _ in 0..MAX_ALERTS + 3 {
        panel.key(&press(Key::Named(Named::Down)));
    }
    assert_eq!(panel.selected().unwrap().key, key(HOST, "s0"));
    let shown = panel.panel();
    assert_eq!(shown.lines, 16);
    assert!(lines(&shown)[4].contains(" s11 "));
    assert!(lines(&shown)[15].contains(" s0 "));
    assert_eq!(panel.click(2.0, 15.5), PanelResult::Jump(key(HOST, "s0")));
    assert_eq!(panel.click(2.0, 4.5), PanelResult::Jump(key(HOST, "s11")));
}

#[test]
fn a_jump_to_a_session_that_is_gone_says_so() {
    let gone = key(HOST, "old name");
    // Listed, or a tab of it is still open (ended, perhaps): its tab is opened.
    assert_eq!(jump(&gone, true, false, true), Ok(()));
    assert_eq!(jump(&gone, false, true, true), Ok(()));
    assert_eq!(jump(&gone, false, true, false), Ok(()));
    // Renamed or killed since the alert: a line for the status line, nothing opened.
    assert_eq!(
        jump(&gone, false, false, true),
        Err("old name on nas is gone (renamed or ended)".to_string())
    );
    // Its host does not answer now: the session may well be there.
    assert_eq!(
        jump(&gone, false, false, false),
        Err("nas is offline".to_string())
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
    alerts.observe(HOST, true, listed, who, Instant::now())
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
        jump(&to, list.status(&to).is_some(), false, true),
        Err("back on nas is gone (renamed or ended)".to_string())
    );
    assert_eq!(
        jump(&front, list.status(&front).is_some(), false, true),
        Ok(())
    );
}

#[test]
fn y_on_an_alert_asks_to_approve_its_session() {
    let mut alerts = Alerts::new(Settings::default());
    let idle = session(HOST, "ask", Status::Idle);
    alerts.observe(HOST, true, [&idle], AWAY, Instant::now());
    let asking = SessionInfo {
        reason: Some("permission prompt".to_string()),
        ..session(HOST, "ask", Status::Waiting)
    };
    alerts.observe(HOST, true, [&asking], AWAY, Instant::now());
    let mut panel = alerts.open(Instant::now());
    assert_eq!(
        panel.key(&press(Key::Char('y'))),
        PanelResult::Approve(key(HOST, "ask"))
    );
}
