//! The session against the real emulator thread and a real PTY: bytes the child receives for
//! mapped keys, key ids and latency lines through a fake present path, photon toggle, IME
//! commit, focus loss during key repeat, scrolling, selection.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use s2_core::latency::{Event, Log, report};
use s2_core::{Pty, PtyWriter, now_ns};
use s2_wgpu::gfx::atlas::{Atlas, FontSpec};
use s2_wgpu::gfx::grid::{CellSpan, Preedit, Scene, Theme};
use s2_wgpu::input::{Action, key_action};
use s2_wgpu::marks::{Marks, PresentPath, WorkDone};
use s2_wgpu::session::Session;
use s2_wgpu::ui::focus::Focus;
use s2_wgpu::ui::layout::layout;
use s2_wgpu::ui::{self, Effect, Ui};
use winit::event::Ime;
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};

const WAIT: Duration = Duration::from_secs(20);
const COLS: u16 = 20;
const ROWS: u16 = 4;

struct Rig {
    session: Session,
    writer: PtyWriter,
    scene: Scene,
    atlas: Atlas,
    theme: Theme,
}

impl Rig {
    /// `script` runs under `sh -c`; bytes the PTY accepts are recorded from the start.
    fn new(script: &str, log: Option<Log>, photon: bool) -> Rig {
        let cmd = ["sh", "-c", script].map(str::to_string);
        let pty = Pty::spawn(&cmd, COLS, ROWS).expect("spawn sh");
        let writer = pty.writer();
        writer.start_recording();
        Rig {
            session: Session::start(pty, COLS, ROWS, log, photon).unwrap(),
            writer,
            scene: Scene::new(),
            atlas: Atlas::new(FontSpec::default(), 1.0),
            theme: Theme::default(),
        }
    }

    /// A child with the tty in raw mode and no echo, so every byte reaches it as sent and the
    /// only output is `cat` writing it back.
    fn raw(log: Option<Log>) -> Rig {
        let mut rig = Rig::new("stty raw -echo; printf ready; exec cat", log, false);
        rig.until("the child is ready", |s| row(s, 0).starts_with("ready"));
        rig
    }

    /// Takes frames as the app does until `done` holds.
    fn until(&mut self, what: &str, done: impl Fn(&Session) -> bool) {
        let start = Instant::now();
        loop {
            self.session
                .take_frames(&mut self.scene, &mut self.atlas, &self.theme);
            if done(&self.session) {
                return;
            }
            assert!(start.elapsed() < WAIT, "timed out waiting until {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn until_recorded(&mut self, expected: &[u8]) {
        let start = Instant::now();
        while self.writer.recorded() != expected {
            assert!(
                start.elapsed() < WAIT,
                "the PTY got {:?}, expected {:?}",
                String::from_utf8_lossy(&self.writer.recorded()),
                String::from_utf8_lossy(expected)
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// A key as the window would deliver it: winit parts → `key_action` → the session.
    fn key(&mut self, key: WKey, text: Option<&str>, mods: ModifiersState, repeat: bool) -> u64 {
        match key_action(&key, text, mods, true, repeat) {
            Action::Send(ev) => self.session.key(now_ns(), ev),
            other => panic!("{key:?} is not sent to the child: {other:?}"),
        }
    }

    fn letter(&mut self, c: &str, repeat: bool) -> u64 {
        self.key(
            WKey::Character(c.into()),
            Some(c),
            ModifiersState::empty(),
            repeat,
        )
    }
}

fn row(session: &Session, y: u16) -> String {
    session.frame().map(|f| f.row_text(y)).unwrap_or_default()
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

fn events(dir: &std::path::Path) -> Vec<Event> {
    std::fs::read_to_string(dir.join("latency.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// A present path that does nothing but count calls and keep the work-done handle.
#[derive(Default)]
struct FakeGpu {
    submits: usize,
    presents: usize,
    done: Vec<WorkDone>,
}

impl PresentPath for FakeGpu {
    fn submit(&mut self, done: WorkDone) {
        self.submits += 1;
        self.done.push(done);
    }
    fn present(&mut self) {
        self.presents += 1;
    }
}

#[test]
fn mapped_keys_reach_the_child_as_terminal_bytes() {
    let mut rig = Rig::raw(None);
    let none = ModifiersState::empty();
    let ch = |s: &str| WKey::Character(s.into());
    let mut expected: Vec<u8> = Vec::new();
    let cases: Vec<(WKey, Option<&str>, ModifiersState, &[u8])> = vec![
        (ch("a"), Some("a"), none, b"a"),
        (ch("A"), Some("A"), ModifiersState::SHIFT, b"A"),
        (ch("é"), Some("é"), none, "é".as_bytes()),
        (WKey::Named(NamedKey::Space), Some(" "), none, b" "),
        // winit's text for these chords is the control character or the plain key; either way
        // the child must get the C0 byte, not a CSI-u sequence.
        (ch("c"), Some("\u{3}"), ModifiersState::CONTROL, b"\x03"),
        (ch("["), Some("["), ModifiersState::CONTROL, b"\x1b"),
        (ch("d"), Some("d"), ModifiersState::CONTROL, b"\x04"),
        (ch("x"), Some("x"), ModifiersState::ALT, b"\x1bx"),
        (WKey::Named(NamedKey::Enter), Some("\r"), none, b"\r"),
        (WKey::Named(NamedKey::Tab), Some("\t"), none, b"\t"),
        (WKey::Named(NamedKey::Backspace), None, none, b"\x7f"),
        (WKey::Named(NamedKey::Escape), None, none, b"\x1b"),
        (WKey::Named(NamedKey::ArrowUp), None, none, b"\x1b[A"),
        (WKey::Named(NamedKey::ArrowLeft), None, none, b"\x1b[D"),
        (WKey::Named(NamedKey::Home), None, none, b"\x1b[H"),
        (WKey::Named(NamedKey::Delete), None, none, b"\x1b[3~"),
        (WKey::Named(NamedKey::PageUp), None, none, b"\x1b[5~"),
        (WKey::Named(NamedKey::F5), None, none, b"\x1b[15~"),
    ];
    for (key, text, mods, bytes) in cases {
        let what = format!("{key:?} + {mods:?}");
        rig.key(key.clone(), text, mods, false);
        expected.extend_from_slice(bytes);
        let start = Instant::now();
        while rig.writer.recorded() != expected {
            assert!(
                start.elapsed() < WAIT,
                "{what}: the PTY got {:?}, expected it to end with {bytes:?}",
                rig.writer.recorded()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        // The release sends nothing in legacy mode.
        if let Action::Send(release) = key_action(&key, None, mods, false, false) {
            rig.session.key(now_ns(), release);
        }
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(rig.writer.recorded(), expected, "releases sent nothing");
}

#[test]
fn key_echo_frame_is_logged_once_with_its_key_and_flips_the_photon_square() {
    let dir = scratch("session_marks");
    let log = Log::create(&dir).unwrap();
    // Cooked tty: the echo of the typed character is the output that carries the key.
    let mut rig = Rig::new("printf ready; exec cat", Some(log.clone()), true);
    rig.until("the child is ready", |s| row(s, 0).starts_with("ready"));
    assert_eq!(rig.session.overlays().photon, Some(false), "starts black");
    let mut marks = Marks::new(log.clone());
    let mut gpu = FakeGpu::default();
    // The frames so far were presented, as the app would have.
    marks.present_frame(rig.session.frame().unwrap().seq, &mut gpu);

    let t_before = now_ns();
    let id = rig.letter("x", false);
    assert_eq!(id, 1, "key ids start at 1");
    // The square flips in the same `take_frames` that brings the echo: whenever the echo is
    // visible, the square already shows the new colour.
    let start = Instant::now();
    loop {
        // As the app does: the redraw begins before the frames are taken.
        marks.begin_redraw();
        rig.session
            .take_frames(&mut rig.scene, &mut rig.atlas, &rig.theme);
        let echoed = row(&rig.session, 0) == "readyx";
        let flipped = rig.session.overlays().photon == Some(true);
        assert_eq!(
            echoed, flipped,
            "echo visible and square flipped go together"
        );
        if echoed {
            break;
        }
        assert!(start.elapsed() < WAIT);
        std::thread::sleep(Duration::from_millis(1));
    }
    let seq = rig.session.frame().unwrap().seq;
    marks.present_frame(seq, &mut gpu);
    assert_eq!((gpu.submits, gpu.presents), (2, 2));
    let t_presented = now_ns();
    for done in &gpu.done {
        done.mark(now_ns());
    }
    assert_eq!(marks.settle(), 0);
    let t_after = now_ns();

    // A second key flips it back; a release does not count as a key.
    let second = rig.letter("y", false);
    assert_eq!(second, 2);
    rig.until("the second echo", |s| row(s, 0) == "readyxy");
    assert_eq!(rig.session.overlays().photon, Some(false));
    marks.present_frame(rig.session.frame().unwrap().seq, &mut gpu);
    // This one's callback never comes: the line is written at shutdown, without the field.
    marks.finish();
    log.flush();

    let events = events(&dir);
    let key_time = |want: u64| {
        events.iter().find_map(|e| match e {
            Event::Key { id, t_key_ns } if *id == want => Some(*t_key_ns),
            _ => None,
        })
    };
    let write_time = |want: u64| {
        events.iter().find_map(|e| match e {
            Event::Write { id, t_write_ns } if *id == want => Some(*t_write_ns),
            _ => None,
        })
    };
    let frames: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Frame {
                seq,
                t_output_ns,
                t_submit_ns,
                t_present_ns,
                present_kind,
                keys,
                t_work_done_ns,
                t_redraw_ns,
            } => Some((
                *seq,
                *t_output_ns,
                *t_submit_ns,
                *t_present_ns,
                present_kind.clone(),
                keys.clone(),
                *t_work_done_ns,
                *t_redraw_ns,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(frames.len(), 3, "one line per presented frame");
    assert!(frames.iter().all(|f| f.4 == "present_return"));
    assert!(
        frames[0].5.is_empty(),
        "the frames before the key carry none"
    );

    let (f_seq, t_output, t_submit, t_present, _, keys, t_work_done, t_redraw) = frames[1].clone();
    assert_eq!(
        (f_seq, keys),
        (seq, vec![1]),
        "key 1 is on the frame that showed its echo"
    );
    let (t_key, t_write) = (key_time(1).unwrap(), write_time(1).unwrap());
    let t_work_done = t_work_done.expect("t_work_done_ns is on the line");
    assert!(
        t_before as u64 <= t_key && t_key <= t_write,
        "key {t_key}, write {t_write}"
    );
    assert!(
        t_key <= t_output && t_output <= t_submit,
        "output {t_output}, submit {t_submit}"
    );
    assert!(t_submit <= t_present && t_present <= t_work_done);
    assert!(t_work_done <= t_after as u64);

    // The redraw entry is on the written line: at or after the key (its echo was taken in that
    // redraw), never after the submit.
    let t_redraw = t_redraw.expect("t_redraw_ns is on the line");
    assert!(
        t_key <= t_redraw && t_redraw <= t_submit,
        "redraw {t_redraw}, submit {t_submit}"
    );
    assert!(t_present <= t_presented as u64);
    assert!(
        frames.iter().all(|f| f.7.is_some_and(|t| t <= f.2)),
        "every line has it, also frames presented without `begin_redraw`"
    );

    assert_eq!(frames[2].5, [2]);
    assert_eq!(
        frames[2].6, None,
        "no work-done time was ever reported for it"
    );
    // Every key counts in the report: none was lost between take and present.
    let report = report(&dir.join("latency.jsonl")).unwrap();
    assert_eq!(report.key_to_present.n, 2);
}

#[test]
fn frames_taken_but_not_presented_keep_their_keys_for_the_next_present() {
    let dir = scratch("session_skipped_present");
    let log = Log::create(&dir).unwrap();
    let mut rig = Rig::new("printf ready; exec cat", Some(log.clone()), false);
    rig.until("the child is ready", |s| row(s, 0).starts_with("ready"));
    assert_eq!(
        rig.session.overlays().photon,
        None,
        "no --photon, no square"
    );
    // Two keys, each echoed in a frame that is taken but never presented (surface not ready).
    rig.letter("a", false);
    rig.until("first echo", |s| row(s, 0) == "readya");
    rig.letter("b", false);
    rig.until("second echo", |s| row(s, 0) == "readyab");
    let mut marks = Marks::new(log.clone());
    let mut gpu = FakeGpu::default();
    marks.present_frame(rig.session.frame().unwrap().seq, &mut gpu);
    marks.finish();
    log.flush();
    let keys: Vec<Vec<u64>> = events(&dir)
        .into_iter()
        .filter_map(|e| match e {
            Event::Frame { keys, .. } => Some(keys),
            _ => None,
        })
        .collect();
    assert_eq!(keys, [vec![1, 2]]);
}

#[test]
fn ime_preedit_is_an_overlay_and_commit_sends_the_text_once() {
    let dir = scratch("session_ime");
    let log = Log::create(&dir).unwrap();
    let mut rig = Rig::raw(Some(log.clone()));
    assert_eq!(rig.session.overlays().preedit, None);
    assert!(rig.session.set_preedit("ni"));
    assert!(!rig.session.set_preedit("ni"), "unchanged");
    assert_eq!(
        rig.session.overlays().preedit,
        Some(Preedit {
            text: "ni".to_string()
        })
    );
    assert!(rig.writer.recorded().is_empty(), "composing sends nothing");

    let id = rig.session.ime_commit(now_ns(), "你好");
    assert_eq!(id, Some(1));
    assert_eq!(
        rig.session.overlays().preedit,
        None,
        "the commit ends the composition"
    );
    rig.until_recorded("你好".as_bytes());
    rig.until("the echo", |s| row(s, 0) == "ready你好");
    assert_eq!(rig.session.ime_commit(now_ns(), ""), None);
    assert!(rig.session.set_preedit("hao"));
    assert!(rig.session.set_preedit(""), "an empty preedit clears it");
    assert_eq!(rig.session.overlays().preedit, None);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        rig.writer.recorded(),
        "你好".as_bytes(),
        "once, no stray bytes"
    );
    log.flush();
    assert!(
        events(&dir).contains(&Event::Write {
            id: 1,
            t_write_ns: match events(&dir)
                .iter()
                .find(|e| matches!(e, Event::Write { .. }))
            {
                Some(Event::Write { t_write_ns, .. }) => *t_write_ns,
                _ => panic!("no write line for the commit"),
            }
        })
    );
    assert!(
        events(&dir)
            .iter()
            .any(|e| matches!(e, Event::Key { id: 1, .. }))
    );
}

#[test]
fn key_repeat_stops_when_the_window_loses_focus() {
    let mut rig = Rig::raw(None);
    rig.letter("a", false);
    rig.letter("a", true);
    rig.letter("a", true);
    rig.until_recorded(b"aaa");
    // Focus goes away while the key is held: repeats still in flight must not be typed, and
    // none may follow when focus returns until a key is pressed again.
    rig.session.focus(false);
    for _ in 0..3 {
        rig.letter("a", true);
    }
    rig.session.focus(true);
    rig.letter("a", true);
    rig.letter("b", false);
    rig.letter("b", true);
    rig.until_recorded(b"aaabb");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(rig.writer.recorded(), b"aaabb");
}

#[test]
fn wheel_scrolls_the_viewport_and_typing_returns_to_the_bottom() {
    let mut rig = Rig::new("seq 1 50; exec cat", None, false);
    rig.until("the last line", |s| row(s, 2) == "50");
    assert!(rig.session.frame().unwrap().cursor.is_some());
    // Two rows up, into the scrollback.
    rig.session.scroll(-2);
    rig.until("scrolled up", |s| row(s, 0) == "46");
    assert_eq!(row(&rig.session, 3), "49");
    assert_eq!(
        rig.session.frame().unwrap().cursor,
        None,
        "the cursor row is out of view"
    );
    rig.session.scroll(0);
    // A key press needs nothing from the front-end: the core returns to the live screen.
    rig.letter("x", false);
    rig.until("back at the bottom", |s| row(s, 3) == "x");
    assert_eq!(row(&rig.session, 2), "50");
    assert!(rig.session.frame().unwrap().cursor.is_some());
}

#[test]
fn selection_gives_spans_and_text_and_ends_with_the_content_it_covered() {
    let mut rig = Rig::new("printf 'ab你好cd\\r\\nsecond\\r\\n'; exec cat", None, false);
    rig.until("the text", |s| row(s, 1) == "second");
    assert_eq!(rig.session.selected_text(), None);

    // A press outside the grid selects nothing; a drag without a press is ignored.
    assert!(!rig.session.select_start(None));
    assert!(!rig.session.selecting());
    assert!(!rig.session.select_drag((3, 0)));
    assert_eq!(rig.session.selected_text(), None);

    rig.session.select_start(Some((1, 0)));
    assert!(rig.session.selecting());
    assert_eq!(rig.session.selected_text(), None, "a click selects nothing");
    assert!(rig.session.select_drag((4, 0)));
    assert!(
        !rig.session.select_drag((4, 0)),
        "same cell: nothing to redraw"
    );
    rig.session.select_end();
    assert!(!rig.session.selecting());
    assert!(!rig.session.select_drag((9, 1)), "the button is up");
    let wide = CellSpan {
        row: 0,
        col_start: 1,
        col_end: 6,
    };
    assert_eq!(rig.session.overlays().selection, [wide]);
    assert_eq!(rig.session.selected_text().as_deref(), Some("b你好"));

    // Output on another row leaves it alone …
    rig.letter("z", false);
    rig.until("echo on row 2", |s| row(s, 2) == "z");
    assert_eq!(rig.session.selected_text().as_deref(), Some("b你好"));
    // … scrolling, or a new grid size, ends it (it is in viewport cells).
    rig.session.scroll(-1);
    assert_eq!(rig.session.selected_text(), None);
    assert_eq!(rig.session.overlays().selection, []);

    rig.session.select_start(Some((0, 1)));
    rig.session.select_drag((5, 1));
    assert_eq!(rig.session.selected_text().as_deref(), Some("second"));
    assert!(!rig.session.resize(COLS, ROWS), "same size: nothing sent");
    assert_eq!(rig.session.selected_text().as_deref(), Some("second"));
    assert!(rig.session.resize(30, 6));
    assert_eq!(rig.session.selected_text(), None);
    rig.until("the resized grid", |s| {
        s.frame().is_some_and(|f| (f.cols, f.rows) == (30, 6))
    });
}

#[test]
fn selection_is_dropped_when_its_rows_are_redrawn() {
    // Raw `cat` writes back what is typed, so typed escape sequences move the cursor.
    let mut rig = Rig::raw(None);
    let none = ModifiersState::empty();
    let type_seq = |rig: &mut Rig, tail: &str| {
        rig.key(WKey::Named(NamedKey::Escape), None, none, false);
        for c in tail.chars() {
            rig.letter(&c.to_string(), false);
        }
    };
    rig.session.select_start(Some((0, 0)));
    rig.session.select_drag((1, 0));
    assert_eq!(rig.session.selected_text().as_deref(), Some("re"));
    // Cursor down, then a letter: row 1 changes, the selection on row 0 stays.
    type_seq(&mut rig, "[BQ");
    rig.until("output on row 1", |s| row(s, 1).ends_with('Q'));
    assert_eq!(rig.session.selected_text().as_deref(), Some("re"));
    // Cursor home, then a letter over the selected text.
    type_seq(&mut rig, "[HQ");
    rig.until("output on row 0", |s| row(s, 0) == "Qeady");
    assert_eq!(rig.session.selected_text(), None);
    assert_eq!(rig.session.overlays().selection, []);
}

#[test]
fn paste_goes_through_the_core_and_the_last_output_is_shown_before_exit() {
    let dir = scratch("session_exit");
    let log = Log::create(&dir).unwrap();
    // The child ends with a burst of output and is gone: `exited()` turns true at the PTY's end
    // of file, while the emulator thread still has most of the burst to parse.
    let mut rig = Rig::new(
        "stty raw -echo; printf ready; head -c 5; stty sane; seq 1 1000; printf ' bye'",
        Some(log.clone()),
        false,
    );
    rig.until("the child is ready", |s| row(s, 0).starts_with("ready"));
    assert!(!rig.session.exited());
    assert!(
        !rig.session.output_drained(),
        "not before the child has exited"
    );
    rig.session.paste("");
    rig.session.paste("hell");
    rig.until_recorded(b"hell");
    // The key that ends the child: its echo is in the last frame.
    let id = rig.letter("o", false);
    let start = Instant::now();
    while !rig.session.exited() {
        assert!(start.elapsed() < WAIT, "the child never exited");
        std::thread::sleep(Duration::from_millis(1));
    }
    // What the app does on exit: ask once, then draw frames until the emulator has caught up.
    assert!(rig.session.begin_exit());
    assert!(!rig.session.begin_exit(), "asked once");
    rig.until("the output is drained", |s| s.output_drained());
    assert_eq!(
        (row(&rig.session, ROWS - 2), row(&rig.session, ROWS - 1)),
        ("1000".to_string(), " bye".to_string()),
        "the frame shown last has the child's last output"
    );

    let mut marks = Marks::new(log.clone());
    marks.present_frame(rig.session.frame().unwrap().seq, &mut FakeGpu::default());
    marks.finish();
    log.flush();
    let shown: Vec<u64> = events(&dir)
        .into_iter()
        .filter_map(|e| match e {
            Event::Frame { keys, .. } => Some(keys),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(shown.contains(&id), "the last key has its frame: {shown:?}");
}

#[test]
fn keys_reach_the_child_and_the_log_only_while_the_terminal_has_the_focus() {
    // Through `ui::key_event` / `ui::ime_event`, the functions the window calls for every key
    // and every IME event. A key that goes to the list or the field would never be echoed in a
    // frame, so it must leave no `key` line; a terminal key must leave exactly what it did
    // before the UI existed.
    let dir = scratch("session_focus_routing");
    let log = Log::create(&dir).unwrap();
    let mut rig = Rig::raw(Some(log.clone()));
    let mut ui = Ui::new();
    let l = layout((1280, 800), 1.0, &rig.atlas.metrics(), &ui.theme);
    let none = ModifiersState::empty();
    let ctrl = ModifiersState::CONTROL;
    let tab = WKey::Named(NamedKey::Tab);
    let enter = WKey::Named(NamedKey::Enter);
    let send = |ui: &mut Ui, rig: &mut Rig, key: &WKey, text: Option<&str>, mods| {
        let effect = ui::key_event(
            ui,
            &mut rig.session,
            &l,
            now_ns(),
            key,
            text,
            mods,
            true,
            false,
        );
        ui::key_event(
            ui,
            &mut rig.session,
            &l,
            now_ns(),
            key,
            None,
            mods,
            false,
            false,
        );
        effect
    };
    let letter = |c: &str| WKey::Character(c.into());

    // Terminal focused (as the app starts): the key and Tab reach the child.
    assert_eq!(ui.focus, Focus::Terminal);
    assert_eq!(
        send(&mut ui, &mut rig, &letter("a"), Some("a"), none),
        Effect::None
    );
    assert_eq!(
        send(&mut ui, &mut rig, &tab, Some("\t"), none),
        Effect::None
    );
    rig.until_recorded(b"a\t");
    assert_eq!(ui.focus, Focus::Terminal, "Tab did not move the focus");

    // Zoom and the traversal chord are the app's, even here.
    assert!(matches!(
        send(&mut ui, &mut rig, &letter("="), None, ctrl),
        Effect::Zoom(_)
    ));
    assert_eq!(send(&mut ui, &mut rig, &tab, None, ctrl), Effect::Redraw);
    assert_eq!(ui.focus, Focus::List);

    // List focused: letters, Enter-less navigation, an IME commit — nothing for the child.
    send(&mut ui, &mut rig, &letter("j"), Some("j"), none);
    send(&mut ui, &mut rig, &letter("q"), Some("q"), none);
    send(
        &mut ui,
        &mut rig,
        &WKey::Named(NamedKey::ArrowDown),
        None,
        none,
    );
    send(&mut ui, &mut rig, &letter("c"), None, ctrl);
    assert_eq!(
        ui::ime_event(
            &mut ui,
            &mut rig.session,
            now_ns(),
            &Ime::Commit("列".to_string())
        ),
        Effect::None
    );
    assert_eq!(ui.list.selected(), 2);

    // Field focused: typing, a composition and its commit, Enter — all stay in the UI.
    send(&mut ui, &mut rig, &tab, Some("\t"), none);
    assert_eq!(ui.focus, Focus::Field);
    send(&mut ui, &mut rig, &letter("n"), Some("n"), none);
    assert_eq!(
        ui::ime_event(
            &mut ui,
            &mut rig.session,
            now_ns(),
            &Ime::Preedit("ni".to_string(), None)
        ),
        Effect::Redraw
    );
    assert_eq!(
        rig.session.overlays().preedit,
        None,
        "not the terminal's preedit"
    );
    assert_eq!(ui.field.preedit(), "ni");
    ui::ime_event(
        &mut ui,
        &mut rig.session,
        now_ns(),
        &Ime::Commit("你".to_string()),
    );
    assert_eq!(ui.field.text(), "n你");
    let items = ui.list.items().len();
    send(&mut ui, &mut rig, &enter, Some("\r"), none);
    assert_eq!(ui.list.items().len(), items + 1);
    assert_eq!(ui.focus, Focus::List);

    // The context menu takes keys too, with the terminal focused under it.
    ui.context(l.terminal.x + 50.0, 50.0, &l);
    assert_eq!(ui.focus, Focus::Terminal);
    send(&mut ui, &mut rig, &letter("m"), Some("m"), none);
    assert!(ui.menu.is_none(), "the key closed the menu");

    // Back on the terminal: keys and IME commits flow again, the preedit is the terminal's.
    send(&mut ui, &mut rig, &letter("b"), Some("b"), none);
    assert_eq!(
        ui::ime_event(
            &mut ui,
            &mut rig.session,
            now_ns(),
            &Ime::Preedit("hao".to_string(), None)
        ),
        Effect::Redraw
    );
    assert_eq!(
        rig.session.overlays().preedit,
        Some(Preedit {
            text: "hao".to_string()
        })
    );
    assert_eq!(ui.field.preedit(), "");
    ui::ime_event(
        &mut ui,
        &mut rig.session,
        now_ns(),
        &Ime::Commit("好".to_string()),
    );
    assert_eq!(rig.session.overlays().preedit, None);
    rig.until_recorded("a\tb好".as_bytes());
    rig.until("the echo", |s| row(s, 0).contains("b好"));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        rig.writer.recorded(),
        "a\tb好".as_bytes(),
        "only the four terminal keys, nothing typed into the list, the field or the menu"
    );

    // The log: one `key` line and one `write` line per terminal press, none for the others.
    log.flush();
    let events = events(&dir);
    let keys: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Event::Key { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    let writes: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Event::Write { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(keys.len(), 4, "a, Tab, b, the commit: {keys:?}");
    assert_eq!(writes, keys, "every logged key was written to the PTY");
}

/// `ui::key_event` with a real session, as the window calls it.
fn key_ev(
    ui: &mut Ui,
    rig: &mut Rig,
    key: &WKey,
    text: Option<&str>,
    mods: ModifiersState,
    pressed: bool,
    repeat: bool,
) -> Effect {
    let l = ui.layout((1280, 800), 1.0, &rig.atlas.metrics());
    ui::key_event(
        ui,
        &mut rig.session,
        &l,
        now_ns(),
        key,
        text,
        mods,
        pressed,
        repeat,
    )
}

fn key_and_write_ids(dir: &std::path::Path) -> (Vec<u64>, Vec<u64>) {
    let events = events(dir);
    let keys = events
        .iter()
        .filter_map(|e| match e {
            Event::Key { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    let writes = events
        .iter()
        .filter_map(|e| match e {
            Event::Write { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    (keys, writes)
}

#[test]
fn a_key_held_while_the_focus_moves_to_the_terminal_types_nothing_there() {
    // Tab in the field moves the focus to the terminal. The key is still down: its repeats
    // now arrive with the terminal focused. They are not terminal keys — no `key` line (the
    // core drops repeats after a focus change, so the line would never get a frame) and no
    // bytes for the child.
    let dir = scratch("session_held_key");
    let log = Log::create(&dir).unwrap();
    let mut rig = Rig::raw(Some(log.clone()));
    let mut ui = Ui::new();
    let none = ModifiersState::empty();
    let tab = WKey::Named(NamedKey::Tab);
    let enter = WKey::Named(NamedKey::Enter);
    let a = WKey::Character("a".into());

    // As the app does on a focus change: the core is told (this is what makes it drop repeats).
    let told = |ui: &mut Ui, rig: &mut Rig| {
        if let Some(on) = ui.sync_focus(true).tell_core {
            rig.session.focus(on);
        }
    };
    told(&mut ui, &mut rig);
    ui.set_focus(Focus::Field);
    told(&mut ui, &mut rig);

    assert_eq!(
        key_ev(&mut ui, &mut rig, &tab, Some("\t"), none, true, false),
        Effect::Redraw
    );
    assert_eq!(ui.focus, Focus::Terminal);
    told(&mut ui, &mut rig);
    for _ in 0..5 {
        assert_eq!(
            key_ev(&mut ui, &mut rig, &tab, Some("\t"), none, true, true),
            Effect::None
        );
    }
    key_ev(&mut ui, &mut rig, &tab, None, none, false, false);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        rig.writer.recorded().is_empty(),
        "{:?}",
        rig.writer.recorded()
    );
    log.flush();
    assert_eq!(
        key_and_write_ids(&dir).0,
        Vec::<u64>::new(),
        "no key line for the held Tab"
    );

    // Enter held on a list item: the same.
    ui.set_focus(Focus::List);
    told(&mut ui, &mut rig);
    key_ev(&mut ui, &mut rig, &enter, Some("\r"), none, true, false);
    assert_eq!(ui.focus, Focus::Terminal);
    told(&mut ui, &mut rig);
    key_ev(&mut ui, &mut rig, &enter, Some("\r"), none, true, true);
    key_ev(&mut ui, &mut rig, &enter, None, none, false, false);

    // A fresh press: the terminal takes keys again, repeats included, as far as this
    // front-end and the core are concerned.
    let b = WKey::Character("b".into());
    key_ev(&mut ui, &mut rig, &b, Some("b"), none, true, false);
    key_ev(&mut ui, &mut rig, &b, None, none, false, false);
    rig.until_recorded(b"b");

    // The menu over a focused terminal: Enter picks an item, any other key closes it; held,
    // neither types into the terminal (no focus change is involved here: the core would
    // have passed these repeats on).
    let l = ui.layout((1280, 800), 1.0, &rig.atlas.metrics());
    ui.context(l.terminal.x + 50.0, 50.0, &l);
    key_ev(
        &mut ui,
        &mut rig,
        &WKey::Named(NamedKey::ArrowDown),
        None,
        none,
        true,
        false,
    );
    assert_eq!(
        key_ev(&mut ui, &mut rig, &enter, Some("\r"), none, true, false),
        Effect::TermCopy
    );
    assert!(ui.menu.is_none());
    key_ev(&mut ui, &mut rig, &enter, Some("\r"), none, true, true);
    key_ev(&mut ui, &mut rig, &enter, None, none, false, false);
    ui.context(l.terminal.x + 50.0, 50.0, &l);
    key_ev(&mut ui, &mut rig, &a, Some("a"), none, true, false);
    assert!(ui.menu.is_none());
    key_ev(&mut ui, &mut rig, &a, Some("a"), none, true, true);
    key_ev(&mut ui, &mut rig, &a, Some("a"), none, true, true);
    key_ev(&mut ui, &mut rig, &a, None, none, false, false);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        rig.writer.recorded(),
        b"b",
        "nothing from the keys that closed the menu"
    );

    // A fresh press ends it: the key and its own repeats are terminal keys again.
    key_ev(&mut ui, &mut rig, &a, Some("a"), none, true, false);
    key_ev(&mut ui, &mut rig, &a, Some("a"), none, true, true);
    key_ev(&mut ui, &mut rig, &a, Some("a"), none, true, true);
    key_ev(&mut ui, &mut rig, &a, None, none, false, false);
    rig.until_recorded(b"baaa");
    rig.until("the echo", |s| row(s, 0).contains("baaa"));
    log.flush();
    let (keys, writes) = key_and_write_ids(&dir);
    assert_eq!(keys.len(), 4, "b, then a and its two repeats: {keys:?}");
    assert_eq!(writes, keys, "every key line has its write line");
}

#[test]
fn without_the_ui_every_key_is_a_terminal_key() {
    // `S2_WGPU_NO_UI=1`: the app's shortcuts for the UI do not exist; routing is
    // `input::key_action` alone, as before the UI.
    let mut rig = Rig::raw(None);
    let mut ui = Ui::with_ui(false);
    let none = ModifiersState::empty();
    let ctrl = ModifiersState::CONTROL;
    let tab = WKey::Named(NamedKey::Tab);
    assert_eq!(
        key_ev(&mut ui, &mut rig, &tab, None, ctrl, true, false),
        Effect::None
    );
    assert_eq!(ui.focus, Focus::Terminal, "Ctrl+Tab did not move the focus");
    assert_eq!(
        key_ev(
            &mut ui,
            &mut rig,
            &WKey::Character("=".into()),
            None,
            ctrl,
            true,
            false
        ),
        Effect::None,
        "no zoom"
    );
    key_ev(
        &mut ui,
        &mut rig,
        &WKey::Character("x".into()),
        Some("x"),
        none,
        true,
        false,
    );
    key_ev(
        &mut ui,
        &mut rig,
        &WKey::Character("x".into()),
        Some("x"),
        none,
        true,
        true,
    );
    // Ctrl+Tab reaches the child (the core encodes it as CSI 27;5;9~), Ctrl+= has no legacy
    // encoding and sends nothing, then the two x.
    rig.until("the echo", |s| row(s, 0).contains("xx"));
    assert_eq!(rig.writer.recorded(), b"\x1b[27;5;9~xx");
    // The copy / paste chords are still the app's.
    let c = WKey::Character("C".into());
    let chord = ctrl | ModifiersState::SHIFT;
    assert_eq!(
        key_ev(&mut ui, &mut rig, &c, None, chord, true, false),
        Effect::TermCopy
    );
}
