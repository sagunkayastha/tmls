//! Keys typed while the focus is not on the terminal reach no PTY and no latency log.
//!
//! Through `keys::dispatch`, the one place the window sends a key to a session, against real
//! PTYs (`cat > file`); no window. With the focus in a form the keys are given to a real
//! new-session form, as the window does.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use client_core::SessionKey;
use desktop::forms::{Form, FormResult, Request};
use desktop::keys::{self, Focus, RepeatGate, Route};
use desktop::tabs::Tabs;
use s2_core::latency::Log;
use s2_core::{Key, KeyEvent, Mods, Named, now_ns};

const WAIT: Duration = Duration::from_secs(20);
const GRID: (u16, u16) = (60, 6);
/// 39 keys, the list's own `j k p n` among them; with Enter that is 40.
const LINE: &str = "jkpn sidebar and form keys reach no pty";

fn key(name: &str) -> SessionKey {
    SessionKey {
        host: "here".to_string(),
        name: name.to_string(),
    }
}

/// A directory of this test's own under the build's temporary directory.
fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("routing-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < WAIT, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The presses of `LINE` and Enter.
fn line_keys() -> Vec<KeyEvent> {
    let press = |key, text: Option<String>| KeyEvent {
        key,
        mods: Mods::default(),
        text,
        repeat: false,
        pressed: true,
    };
    let mut keys: Vec<KeyEvent> = LINE
        .chars()
        .map(|c| press(Key::Char(c), Some(c.to_string())))
        .collect();
    keys.push(press(Key::Named(Named::Enter), None));
    keys
}

/// Sends the 40 keys the way the window does. Returns how many went to the terminal.
fn type_with(focus: Focus, gate: &mut RepeatGate, tabs: &mut Tabs) -> usize {
    line_keys()
        .into_iter()
        .filter(|ev| {
            let (route, back) = keys::dispatch(now_ns(), ev.clone(), focus, gate, tabs);
            assert_eq!(
                back.is_none(),
                route == Route::Terminal,
                "a key is sent exactly when its route is the terminal: {ev:?}"
            );
            route == Route::Terminal
        })
        .count()
}

/// `key` lines in the latency log so far.
fn key_lines(log: &Log, path: &Path) -> usize {
    log.flush();
    read(path).matches(r#""ev":"key""#).count()
}

#[test]
fn sidebar_and_form_keys_reach_no_pty() {
    let dir = scratch("no-pty");
    let (file_a, file_b) = (dir.join("a"), dir.join("b"));
    let (script_a, script_b) = (
        format!("exec cat > '{}'", file_a.display()),
        format!("exec cat > '{}'", file_b.display()),
    );
    let mut tabs = Tabs::with_spawner(Box::new(move |key| {
        let script = if key.name == "a" {
            &script_a
        } else {
            &script_b
        };
        vec!["sh".to_string(), "-c".to_string(), script.clone()]
    }));
    // The first tab opened writes the latency log.
    let log = Log::create(&dir).unwrap();
    let log_path = dir.join("latency.jsonl");
    tabs.set_log(log.clone());
    tabs.open(&key("a"), GRID).unwrap();
    tabs.open(&key("b"), GRID).unwrap();
    until("both children have started", || {
        file_a.exists() && file_b.exists()
    });
    assert!(tabs.logged(0) && !tabs.logged(1));
    assert_eq!(line_keys().len(), 40);
    let mut gate = RepeatGate::default();

    // The list or a form has the keys: with either tab in front, nothing is sent.
    for front in [1, 0] {
        tabs.select(front);
        for focus in [Focus::Sidebar, Focus::Form] {
            assert_eq!(
                type_with(focus, &mut gate, &mut tabs),
                0,
                "{focus:?}, tab {front} in front"
            );
        }
    }

    // A real form has the keys: a session name typed into it, and the Enter that creates,
    // arrive in the form — and in neither session behind it.
    for front in [1, 0] {
        tabs.select(front);
        let mut form = Form::new_session(&["here".to_string()], "here", &[]);
        let tab = KeyEvent {
            key: Key::Named(Named::Tab),
            mods: Mods::default(),
            text: None,
            repeat: false,
            pressed: true,
        };
        let mut last = FormResult::Pending;
        for ev in [tab.clone(), tab].into_iter().chain(line_keys()) {
            let (route, back) = keys::dispatch(now_ns(), ev, Focus::Form, &mut gate, &mut tabs);
            assert_eq!(route, Route::Form);
            last = form.key(&back.expect("a key for the form comes back"));
        }
        let FormResult::Submit(Request::New(asked)) = last else {
            panic!("Enter in the name field creates: {last:?}");
        };
        assert_eq!(
            asked.name, LINE,
            "the form got every key, tab {front} in front"
        );
    }
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        read(&file_a) + &read(&file_b),
        "",
        "nothing was typed behind the form"
    );
    assert_eq!(
        key_lines(&log, &log_path),
        0,
        "no key line for a key that went to the list or a form"
    );

    // The terminal has the keys: the same keys land in the tab in front, and only there. The
    // files hold exactly one line each, so none of the 160 keys before arrived late.
    let line = format!("{LINE}\n");
    assert_eq!(type_with(Focus::Terminal, &mut gate, &mut tabs), 40);
    until("a has the line", || read(&file_a) == line);
    assert_eq!(key_lines(&log, &log_path), 40, "the logged tab's 40 keys");
    assert_eq!(read(&file_b), "", "b got nothing while a was in front");

    tabs.select(1);
    assert_eq!(type_with(Focus::Terminal, &mut gate, &mut tabs), 40);
    until("b has the line", || read(&file_b) == line);
    assert_eq!(read(&file_a), line, "a got nothing more");
    assert_eq!(
        key_lines(&log, &log_path),
        40,
        "the other tab does not write the log"
    );
}
