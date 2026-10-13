//! The tab model against real PTYs. The children are `sh -c` scripts, so no tmux is needed,
//! and no window, GPU or font: frames are read as text.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use client_core::SessionKey;
use desktop::tabs::{TabState, Tabs};
use s2_core::{Key, KeyEvent, Mods, Named, now_ns};

const WAIT: Duration = Duration::from_secs(20);
const GRID: (u16, u16) = (40, 6);

/// One test at a time: `close_drops_the_emu_and_keeps_the_others` counts this process's
/// threads.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn key(name: &str) -> SessionKey {
    SessionKey {
        host: "here".to_string(),
        name: name.to_string(),
    }
}

/// Tabs whose command for session NAME is `sh -c scripts[NAME]`.
fn tabs(scripts: &[(&str, String)]) -> Tabs {
    let scripts: HashMap<String, String> = scripts
        .iter()
        .map(|(name, script)| (name.to_string(), script.clone()))
        .collect();
    Tabs::with_spawner(Box::new(move |key| {
        let script = scripts
            .get(&key.name)
            .unwrap_or_else(|| panic!("no script for {key:?}"));
        vec!["sh".to_string(), "-c".to_string(), script.clone()]
    }))
}

/// A directory of this test's own under the build's temporary directory.
fn scratch(test: &str) -> PathBuf {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("tabs-{test}-{}", std::process::id()));
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

/// Types `text` and Enter into the tab in front. Returns how many of the keys were taken.
fn type_line(tabs: &mut Tabs, text: &str) -> usize {
    let press = |key, text: Option<String>| KeyEvent {
        key,
        mods: Mods::default(),
        text,
        repeat: false,
        pressed: true,
    };
    let mut keys: Vec<KeyEvent> = text
        .chars()
        .map(|c| press(Key::Char(c), Some(c.to_string())))
        .collect();
    keys.push(press(Key::Named(Named::Enter), None));
    keys.into_iter()
        .filter_map(|ev| tabs.send_key(now_ns(), ev))
        .count()
}

/// The screen of a tab as text, with every frame that is waiting taken first.
fn screen(tabs: &mut Tabs, index: usize) -> String {
    let session = tabs.session_mut(index);
    session.take_frames_with(|_| {});
    let Some(frame) = session.frame() else {
        return String::new();
    };
    (0..frame.rows)
        .map(|row| frame.row_text(row))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Threads of this process named `s2-pty-…` (each live session has a reader and a writer).
fn pty_threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(|task| std::fs::read_to_string(task.ok()?.path().join("comm")).ok())
        .filter(|name| name.starts_with("s2-pty-"))
        .count()
}

/// Waits until this process has exactly `count` `s2-pty-…` threads. Waits, because a thread
/// names itself after it has started and leaves `/proc` a moment after it was joined.
fn until_pty_threads(count: usize) {
    until(&format!("{count} s2-pty threads are left"), || {
        pty_threads() == count
    });
}

#[test]
fn open_twice_selects_the_existing_tab() {
    let _serial = serial();
    let mut tabs = tabs(&[("a", "exec cat".into()), ("b", "exec cat".into())]);
    assert_eq!((tabs.len(), tabs.active()), (0, None));
    assert_eq!(tabs.open(&key("a"), GRID).unwrap(), 0);
    assert_eq!(tabs.open(&key("b"), GRID).unwrap(), 1);
    assert_eq!(tabs.active(), Some(1));

    assert_eq!(
        tabs.open(&key("a"), GRID).unwrap(),
        0,
        "the tab that is there, not a third one"
    );
    assert_eq!((tabs.len(), tabs.active()), (2, Some(0)));
    assert_eq!((tabs.key(0), tabs.key(1)), (&key("a"), &key("b")));

    // Next and previous wrap round; an index that is no tab changes nothing.
    tabs.prev();
    assert_eq!(tabs.active(), Some(1));
    tabs.next();
    assert_eq!(tabs.active(), Some(0));
    tabs.select(7);
    assert_eq!(tabs.active(), Some(0));
}

#[test]
fn typing_goes_only_to_the_active_tab() {
    let _serial = serial();
    let dir = scratch("typing");
    let (file_a, file_b) = (dir.join("a"), dir.join("b"));
    let script = |file: &Path| format!("exec cat > '{}'", file.display());
    let mut tabs = tabs(&[("a", script(&file_a)), ("b", script(&file_b))]);
    tabs.open(&key("a"), GRID).unwrap();
    tabs.open(&key("b"), GRID).unwrap();
    until("both children have started", || {
        file_a.exists() && file_b.exists()
    });

    assert_eq!(type_line(&mut tabs, "to-b"), 5);
    until("b has its line", || read(&file_b) == "to-b\n");
    tabs.select(0);
    assert_eq!(type_line(&mut tabs, "to-a"), 5);
    until("a has its line", || read(&file_a) == "to-a\n");
    assert_eq!(
        read(&file_b),
        "to-b\n",
        "b got nothing of what was typed into a"
    );
}

#[test]
fn a_tab_whose_child_exits_stays_and_can_reattach() {
    let _serial = serial();
    let dir = scratch("exits");
    let file = dir.join("stay");
    // "gone" ends at once the first time it is started and stays the second time.
    let starts = Arc::new(AtomicUsize::new(0));
    let counted = starts.clone();
    let stay = format!("exec cat > '{}'", file.display());
    let mut tabs = Tabs::with_spawner(Box::new(move |key| {
        let script = match key.name.as_str() {
            "stay" => stay.clone(),
            _ if counted.fetch_add(1, Ordering::SeqCst) == 0 => "true".to_string(),
            _ => "echo back; exec cat".to_string(),
        };
        vec!["sh".to_string(), "-c".to_string(), script]
    }));
    tabs.open(&key("stay"), GRID).unwrap();
    tabs.open(&key("gone"), GRID).unwrap();
    until("the child of the second tab has exited", || {
        tabs.state(1) != TabState::Live
    });

    assert_eq!(tabs.state(1), TabState::Ended { exit: None });
    assert_eq!(
        (tabs.len(), tabs.active()),
        (2, Some(1)),
        "the tab stays, and stays in front"
    );
    assert_eq!(tabs.key(1), &key("gone"));
    assert_eq!(
        tabs.state(0),
        TabState::Live,
        "the other tab is not affected"
    );
    assert_eq!(
        tabs.state_changes(),
        [key("gone")],
        "the end is reported, with the session whose host is to be listed again"
    );
    assert!(tabs.state_changes().is_empty(), "once");
    assert_eq!(
        type_line(&mut tabs, "lost"),
        0,
        "an ended tab takes no keys"
    );

    tabs.reattach(1).unwrap();
    assert_eq!(tabs.state(1), TabState::Live);
    assert_eq!((tabs.len(), tabs.active()), (2, Some(1)));
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    until("the new child's output is on the tab", || {
        screen(&mut tabs, 1).contains("back")
    });
    assert_eq!(type_line(&mut tabs, "typed"), 6);
    until("the reattached tab echoes", || {
        screen(&mut tabs, 1).contains("typed")
    });

    // Reattaching a live tab starts nothing.
    tabs.reattach(1).unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 2);

    tabs.select(0);
    assert_eq!(type_line(&mut tabs, "still-here"), 11);
    until("the other tab still works", || {
        read(&file) == "still-here\n"
    });
}

#[test]
fn close_drops_the_emu_and_keeps_the_others() {
    let _serial = serial();
    let dir = scratch("close");
    let file = dir.join("kept");
    let pid_file = dir.join("closed-pid");
    let mut tabs = tabs(&[
        (
            "closed",
            format!("echo $$ > '{}'; exec cat", pid_file.display()),
        ),
        ("kept", format!("exec cat > '{}'", file.display())),
        ("third", "exec cat".into()),
    ]);
    // No session is left over from another test.
    until_pty_threads(0);
    tabs.open(&key("closed"), GRID).unwrap();
    tabs.open(&key("kept"), GRID).unwrap();
    tabs.open(&key("third"), GRID).unwrap();
    until_pty_threads(6);
    until("the first child has written its pid", || {
        read(&pid_file).ends_with('\n')
    });
    let pid = read(&pid_file).trim().to_string();
    assert!(Path::new("/proc").join(&pid).exists());

    // Closing a tab left of the one in front: the one in front stays in front.
    tabs.close(0);
    // The closed tab's reader and writer threads are gone.
    until_pty_threads(4);
    until("the closed tab's child is gone", || {
        !Path::new("/proc").join(&pid).exists()
    });
    assert_eq!((tabs.len(), tabs.active()), (2, Some(1)));
    assert_eq!((tabs.key(0), tabs.key(1)), (&key("kept"), &key("third")));

    // Closing the one in front: its neighbour comes to the front and takes the keys.
    tabs.close(1);
    until_pty_threads(2);
    assert_eq!((tabs.len(), tabs.active()), (1, Some(0)));
    assert_eq!(tabs.state(0), TabState::Live);
    assert_eq!(type_line(&mut tabs, "kept"), 5);
    until("the tab that was kept works", || read(&file) == "kept\n");

    tabs.close(0);
    until_pty_threads(0);
    assert_eq!((tabs.len(), tabs.active()), (0, None));
    assert_eq!(type_line(&mut tabs, "nobody"), 0);
    tabs.close(0);
    tabs.next();
    tabs.prev();
    assert_eq!(tabs.active(), None);
}

#[test]
fn inactive_tabs_keep_parsing_output() {
    let _serial = serial();
    let dir = scratch("inactive");
    let go = dir.join("go");
    let done = dir.join("done");
    // Prints only once `go` exists, i.e. while its tab is in the back.
    let waits = format!(
        "while [ ! -e '{}' ]; do sleep 0.02; done; echo printed-in-the-back; touch '{}'; exec cat",
        go.display(),
        done.display()
    );
    let mut tabs = tabs(&[("back", waits), ("front", "echo in-front; exec cat".into())]);
    tabs.open(&key("back"), GRID).unwrap();
    tabs.open(&key("front"), GRID).unwrap();
    until("the tab in front shows its output", || {
        screen(&mut tabs, 1).contains("in-front")
    });

    std::fs::write(&go, "").unwrap();
    until("the tab in the back has printed", || done.exists());
    tabs.select(0);
    until("its screen is current after switching back", || {
        screen(&mut tabs, 0).contains("printed-in-the-back")
    });
    assert_eq!(tabs.state(0), TabState::Live);
}

#[test]
fn resize_reaches_the_active_pty() {
    let _serial = serial();
    // Each child prints its terminal's size whenever a line is typed.
    let sizes = "while read -r _; do stty size; done".to_string();
    let mut tabs = tabs(&[("a", sizes.clone()), ("b", sizes)]);
    tabs.open(&key("a"), GRID).unwrap();
    tabs.open(&key("b"), GRID).unwrap();
    type_line(&mut tabs, "");
    until("b reports the size it was opened with", || {
        screen(&mut tabs, 1).contains("6 40")
    });

    tabs.resize_all((52, 9));
    type_line(&mut tabs, "");
    until("the tab in front has the new size", || {
        screen(&mut tabs, 1).contains("9 52")
    });

    // The tab in the back gets it when it comes to the front.
    tabs.select(0);
    type_line(&mut tabs, "");
    until("the other tab has the new size once selected", || {
        screen(&mut tabs, 0).contains("9 52")
    });

    // A tab opened later starts at the size it is opened with.
    tabs.close(1);
    tabs.open(&key("b"), (30, 5)).unwrap();
    type_line(&mut tabs, "");
    until("a new tab starts at the size given", || {
        screen(&mut tabs, 1).contains("5 30")
    });
}

#[test]
fn rekey_gives_a_tab_a_new_key_and_keeps_its_session() {
    let _serial = serial();
    let mut tabs = tabs(&[("old", "sleep 30".to_string())]);
    let index = tabs.open(&key("old"), GRID).unwrap();
    tabs.rekey(index, key("new"));
    assert_eq!(tabs.key(index), &key("new"));
    assert_eq!(tabs.index_of(&key("old")), None);
    assert_eq!(tabs.index_of(&key("new")), Some(index));
    assert_eq!(tabs.state(index), TabState::Live);
}

/// What tmux does when a mouse selection in it ends (`set-clipboard external`): OSC 52. The
/// text is the tab's to hand over, in front or behind, and once.
#[test]
fn a_program_that_writes_the_clipboard_is_heard_in_front_and_behind() {
    let _serial = serial();
    // "front" and "behind" in base64.
    let mut tabs = tabs(&[
        (
            "a",
            "printf '\\033]52;;YmVoaW5k\\007'; sleep 30".to_string(),
        ),
        (
            "b",
            "printf '\\033]52;c;ZnJvbnQ=\\033\\\\'; sleep 30".to_string(),
        ),
    ]);
    tabs.open(&key("a"), GRID).unwrap();
    tabs.open(&key("b"), GRID).unwrap();
    assert_eq!(tabs.active(), Some(1));

    let mut copied = Vec::new();
    until("both copies arrived", || {
        copied.extend(tabs.take_copied());
        copied.len() == 2
    });
    copied.sort();
    assert_eq!(copied, ["behind", "front"]);
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        tabs.take_copied().is_empty(),
        "each copy is handed over once"
    );
}
