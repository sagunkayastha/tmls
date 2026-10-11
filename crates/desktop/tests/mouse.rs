//! The mouse over the terminal: what goes to the program in the tab and what is done here.
//! `Pointer` alone, and with real PTYs behind `Tabs` (the child is `cat -v`, so the screen
//! shows the bytes the program read). No window, GPU or font.

use std::time::{Duration, Instant};

use client_core::SessionKey;
use desktop::keys::{self, Focus, RepeatGate, Route};
use desktop::mouse::{self, Act, Effect, Pointer};
use desktop::tabs::Tabs;
use s2_core::{Key, KeyEvent, Mods, MouseButton, MouseKind, now_ns};

const WAIT: Duration = Duration::from_secs(20);
const GRID: (u16, u16) = (40, 6);
const NONE: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: false,
    logo: false,
};
const CTRL: Mods = Mods { ctrl: true, ..NONE };
const SHIFT: Mods = Mods {
    shift: true,
    ..NONE
};
const LEFT: MouseButton = MouseButton::Left;

fn report(kind: MouseKind, button: Option<MouseButton>, at: (u16, u16), mods: Mods) -> Act {
    Act::Report {
        kind,
        button,
        at,
        mods,
    }
}

// ---- `Pointer`: who gets a mouse event ----

#[test]
fn wheel_goes_to_the_program_when_it_reports_and_scrolls_here_when_not() {
    let mut pointer = Pointer::default();
    let at = Some((3, 2));
    // One notch is three rows (`s2_wgpu::input::wheel_rows`); negative is up.
    let up = report(MouseKind::Press, Some(MouseButton::WheelUp), (3, 2), NONE);
    let down = report(MouseKind::Press, Some(MouseButton::WheelDown), (3, 2), NONE);
    assert_eq!(pointer.wheel(-3, at, NONE, true), [up]);
    assert_eq!(pointer.wheel(6, at, NONE, true), [down, down]);
    // A touchpad's single rows add up to notches.
    assert_eq!(pointer.wheel(-1, at, NONE, true), []);
    assert_eq!(pointer.wheel(-1, at, NONE, true), []);
    assert_eq!(pointer.wheel(-1, at, NONE, true), [up]);
    assert_eq!(pointer.wheel(0, at, NONE, true), []);
    assert_eq!(
        pointer.wheel(-3000, at, NONE, true).len(),
        20,
        "a fling is capped"
    );

    // No reports asked for: the viewport scrolls, as before.
    assert_eq!(pointer.wheel(-3, at, NONE, false), [Act::Scroll(-3)]);
    assert_eq!(pointer.wheel(1, at, NONE, false), [Act::Scroll(1)]);
    assert_eq!(pointer.wheel(0, at, NONE, false), []);
    // Shift keeps the wheel here even when the program reports.
    assert_eq!(pointer.wheel(-3, at, SHIFT, true), [Act::Scroll(-3)]);
}

#[test]
fn plain_drag_goes_to_the_program_when_it_reports() {
    let mut pointer = Pointer::default();
    assert_eq!(
        pointer.press(LEFT, Some((2, 1)), NONE, true),
        [
            Act::SelectClear,
            report(MouseKind::Press, Some(LEFT), (2, 1), NONE)
        ]
    );
    assert!(pointer.held());
    assert_eq!(pointer.motion((2, 1), NONE, true), [], "same cell");
    assert_eq!(
        pointer.motion((5, 3), NONE, true),
        [report(MouseKind::Motion, Some(LEFT), (5, 3), NONE)]
    );
    assert_eq!(pointer.motion((5, 3), NONE, true), []);
    assert_eq!(
        pointer.release(LEFT, Some((5, 3)), NONE),
        [report(MouseKind::Release, Some(LEFT), (5, 3), NONE)]
    );
    assert!(!pointer.held());
    assert_eq!(
        pointer.release(LEFT, Some((5, 3)), NONE),
        [],
        "nothing is held"
    );

    // The right and the middle button too.
    let right = MouseButton::Right;
    assert_eq!(
        pointer.press(right, Some((0, 0)), NONE, true)[1],
        report(MouseKind::Press, Some(right), (0, 0), NONE)
    );
    // The release of another button is not this press's.
    assert_eq!(pointer.release(LEFT, Some((0, 0)), NONE), []);
    assert_eq!(
        pointer.release(right, None, NONE),
        [report(MouseKind::Release, Some(right), (0, 0), NONE)],
        "off the grid: released where it was last reported"
    );
    // With no button held the program hears motion (it gets it only under mode 1003).
    assert_eq!(
        pointer.motion((1, 1), NONE, true),
        [report(MouseKind::Motion, None, (1, 1), NONE)]
    );
}

#[test]
fn ctrl_or_shift_drag_always_selects_here_and_copies_on_release() {
    for (mods, reporting) in [(CTRL, true), (SHIFT, true), (CTRL, false), (SHIFT, false)] {
        let mut pointer = Pointer::default();
        let what = format!("{mods:?} reporting={reporting}");
        assert_eq!(
            pointer.press(LEFT, Some((2, 1)), mods, reporting),
            [Act::SelectStart(Some((2, 1)))],
            "{what}"
        );
        assert_eq!(
            pointer.motion((7, 1), mods, reporting),
            [Act::SelectDrag((7, 1))],
            "{what}"
        );
        // Letting go of the modifier in the middle of the drag changes nothing.
        assert_eq!(
            pointer.motion((9, 2), NONE, reporting),
            [Act::SelectDrag((9, 2))],
            "{what}"
        );
        assert_eq!(
            pointer.release(LEFT, Some((9, 2)), NONE),
            [Act::SelectEnd, Act::Copy],
            "{what}"
        );
    }
    // The other buttons with a modifier do nothing: not a selection, not the program's.
    let mut pointer = Pointer::default();
    assert_eq!(
        pointer.press(MouseButton::Right, Some((2, 1)), CTRL, true),
        []
    );
    assert_eq!(
        pointer.press(MouseButton::Middle, Some((2, 1)), SHIFT, true),
        []
    );
    assert!(!pointer.held());
}

#[test]
fn without_reports_a_plain_drag_selects_and_other_buttons_do_nothing() {
    let mut pointer = Pointer::default();
    assert_eq!(
        pointer.press(LEFT, Some((0, 0)), NONE, false),
        [Act::SelectStart(Some((0, 0)))]
    );
    assert_eq!(
        pointer.motion((4, 0), NONE, false),
        [Act::SelectDrag((4, 0))]
    );
    assert_eq!(
        pointer.release(LEFT, Some((4, 0)), NONE),
        [Act::SelectEnd, Act::Copy]
    );
    // A click selects nothing, copies nothing and opens nothing.
    pointer.press(LEFT, Some((0, 0)), NONE, false);
    assert_eq!(pointer.release(LEFT, Some((0, 0)), NONE), [Act::SelectEnd]);
    assert_eq!(
        pointer.press(MouseButton::Right, Some((0, 0)), NONE, false),
        []
    );
    assert_eq!(pointer.motion((3, 3), NONE, false), []);
}

#[test]
fn ctrl_click_opens_the_link_and_reaches_no_program() {
    for reporting in [true, false] {
        let mut pointer = Pointer::default();
        assert_eq!(
            pointer.press(LEFT, Some((6, 2)), CTRL, reporting),
            [Act::SelectStart(Some((6, 2)))]
        );
        assert_eq!(
            pointer.release(LEFT, Some((6, 2)), CTRL),
            [Act::SelectEnd, Act::OpenLink((6, 2))]
        );
    }
    // Shift+click is no link click; Ctrl+drag is a selection, not a link.
    let mut pointer = Pointer::default();
    pointer.press(LEFT, Some((6, 2)), SHIFT, true);
    assert_eq!(pointer.release(LEFT, Some((6, 2)), SHIFT), [Act::SelectEnd]);
    pointer.press(LEFT, Some((6, 2)), CTRL, true);
    pointer.motion((8, 2), CTRL, true);
    pointer.motion((6, 2), CTRL, true);
    assert_eq!(
        pointer.release(LEFT, Some((6, 2)), CTRL),
        [Act::SelectEnd, Act::Copy]
    );
}

// ---- with a program behind it ----

fn key(name: &str) -> SessionKey {
    SessionKey {
        host: "here".to_string(),
        name: name.to_string(),
    }
}

/// One tab running `sh -c script`.
fn tab(script: &str) -> Tabs {
    let script = script.to_string();
    let mut tabs = Tabs::with_spawner(Box::new(move |_| {
        vec!["sh".to_string(), "-c".to_string(), script.clone()]
    }));
    tabs.open(&key("a"), GRID).unwrap();
    tabs
}

/// `cat -v` on a tty that neither echoes, buffers lines nor makes signals of Ctrl+C, after
/// printing `intro`.
fn cat(intro: &str) -> Tabs {
    tab(&format!(
        "stty -echo -icanon -isig min 1 time 0; printf '{intro}'; exec cat -v"
    ))
}

fn screen(tabs: &mut Tabs) -> String {
    let session = tabs.session_mut(0);
    session.take_frames_with(|_| {});
    let Some(frame) = session.frame() else {
        return String::new();
    };
    (0..frame.rows)
        .map(|row| frame.row_text(row))
        .collect::<Vec<_>>()
        .join("\n")
}

fn until_screen(tabs: &mut Tabs, what: &str, done: impl Fn(&str) -> bool) -> String {
    let start = Instant::now();
    loop {
        let text = screen(tabs);
        if done(&text) {
            return text;
        }
        assert!(
            start.elapsed() < WAIT,
            "timed out waiting until {what}; screen:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn press(c: char, mods: Mods) -> KeyEvent {
    KeyEvent {
        key: Key::Char(c),
        mods,
        text: (!mods.ctrl).then(|| c.to_string()),
        repeat: false,
        pressed: true,
    }
}

/// Does `acts` to the tab in front and returns what is left for the window.
fn perform(tabs: &mut Tabs, acts: Vec<Act>) -> Vec<Effect> {
    acts.into_iter()
        .filter_map(|act| mouse::perform(act, tabs.session_mut(0)))
        .collect()
}

/// Types `z` and waits for it: everything sent before it is on the screen by then.
fn flushed(tabs: &mut Tabs) -> String {
    tabs.send_key(now_ns(), press('z', NONE)).unwrap();
    until_screen(tabs, "z is echoed", |text| text.contains('z')).replace('\n', "")
}

#[test]
fn wheel_and_drag_reach_a_program_that_reports() {
    let mut tabs = cat("\\033[?1002h\\033[?1006hready\\n");
    until_screen(&mut tabs, "ready", |text| text.contains("ready"));
    assert!(tabs.mouse_reporting());
    let mut pointer = Pointer::default();
    let reporting = tabs.mouse_reporting();

    let acts = pointer.wheel(-3, Some((4, 2)), NONE, reporting);
    assert_eq!(perform(&mut tabs, acts), []);
    let acts = pointer.press(LEFT, Some((1, 0)), NONE, reporting);
    assert_eq!(perform(&mut tabs, acts), []);
    let acts = pointer.motion((3, 1), NONE, reporting);
    assert_eq!(perform(&mut tabs, acts), []);
    let acts = pointer.release(LEFT, Some((3, 1)), NONE);
    assert_eq!(perform(&mut tabs, acts), []);

    // Wheel up at column 5, row 3; press, drag and release of the left button (1-based).
    let expected = "^[[<64;5;3M^[[<0;2;1M^[[<32;4;2M^[[<0;4;2mz";
    let text = flushed(&mut tabs);
    assert!(text.contains(expected), "expected {expected} in {text}");
    assert_eq!(
        tabs.session(0).selected_text(),
        None,
        "nothing was selected here"
    );
}

#[test]
fn ctrl_drag_selects_here_and_sends_the_program_nothing() {
    // Trailing blanks on the first line: copied text loses them.
    let mut tabs = cat("\\033[?1002h\\033[?1006hfirst line   \\nsecond\\n");
    until_screen(&mut tabs, "both lines", |text| text.contains("second"));
    assert!(tabs.mouse_reporting());
    let mut pointer = Pointer::default();

    let acts = pointer.press(LEFT, Some((6, 0)), CTRL, true);
    perform(&mut tabs, acts);
    let acts = pointer.motion((3, 1), CTRL, true);
    assert_eq!(perform(&mut tabs, acts), [Effect::Redraw]);
    let acts = pointer.release(LEFT, Some((3, 1)), CTRL);
    assert_eq!(
        perform(&mut tabs, acts),
        [Effect::Copy("line\nseco".to_string())],
        "copied on release, without the trailing blanks"
    );
    // A whole row selected to its end is trimmed too.
    let acts = pointer.press(LEFT, Some((0, 0)), SHIFT, true);
    perform(&mut tabs, acts);
    let acts = pointer.motion((39, 0), SHIFT, true);
    perform(&mut tabs, acts);
    let acts = pointer.release(LEFT, Some((39, 0)), SHIFT);
    assert_eq!(
        perform(&mut tabs, acts).last(),
        Some(&Effect::Copy("first line".to_string()))
    );

    let text = flushed(&mut tabs);
    assert!(
        !text.contains("^[["),
        "no report reached the program: {text}"
    );
}

#[test]
fn without_reports_the_wheel_scrolls_the_viewport() {
    let mut tabs = tab("stty -echo; seq 1 30; echo end; exec cat");
    until_screen(&mut tabs, "the output is in", |text| text.contains("end"));
    assert!(!tabs.mouse_reporting());
    let mut pointer = Pointer::default();
    let before = screen(&mut tabs);
    let acts = pointer.wheel(-3, Some((0, 0)), NONE, tabs.mouse_reporting());
    assert_eq!(acts, [Act::Scroll(-3)]);
    perform(&mut tabs, acts);
    let after = until_screen(&mut tabs, "the viewport moved", |text| text != before);
    let first = |text: &str| text.lines().next().unwrap().parse::<i32>().unwrap();
    assert_eq!(first(&after), first(&before) - 3, "three rows up:\n{after}");
}

#[test]
fn an_ended_tab_does_not_report() {
    let mut tabs = tab("printf '\\033[?1000h\\033[?1006hbye\\n'");
    until_screen(&mut tabs, "bye", |text| text.contains("bye"));
    let start = Instant::now();
    while tabs.active_state() == Some(desktop::tabs::TabState::Live) {
        assert!(start.elapsed() < WAIT, "the command did not end");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(tabs.session(0).mouse_reporting(), "the mode was left set");
    assert!(
        !tabs.mouse_reporting(),
        "but nobody is there to read reports"
    );
}

#[test]
fn ctrl_click_on_a_url_gives_the_link_to_open() {
    let mut tabs = cat("see http://localhost:8080/x, ok\\n");
    until_screen(&mut tabs, "the line", |text| text.contains("ok"));
    let mut pointer = Pointer::default();
    let acts = pointer.press(LEFT, Some((9, 0)), CTRL, false);
    perform(&mut tabs, acts);
    let acts = pointer.release(LEFT, Some((9, 0)), CTRL);
    assert_eq!(
        perform(&mut tabs, acts),
        [Effect::Open("http://localhost:8080/x".to_string())]
    );
    // Ctrl+click on plain text opens nothing.
    let acts = pointer.press(LEFT, Some((1, 0)), CTRL, false);
    perform(&mut tabs, acts);
    let acts = pointer.release(LEFT, Some((1, 0)), CTRL);
    assert_eq!(perform(&mut tabs, acts), []);
    let text = flushed(&mut tabs);
    assert!(!text.contains("^["), "the click reached no program: {text}");
}

#[test]
fn ctrl_c_copies_a_selection_and_interrupts_without_one() {
    let mut tabs = cat("hello world\\n");
    until_screen(&mut tabs, "hello", |text| text.contains("hello world"));
    let mut gate = RepeatGate::default();
    let mut pointer = Pointer::default();
    let ctrl_c = press('c', CTRL);

    // Text is selected: Ctrl+C copies it, ends the selection and is not sent.
    let acts = pointer.press(LEFT, Some((0, 0)), NONE, false);
    perform(&mut tabs, acts);
    let acts = pointer.motion((4, 0), NONE, false);
    perform(&mut tabs, acts);
    let acts = pointer.release(LEFT, Some((4, 0)), NONE);
    perform(&mut tabs, acts);
    // Not for the list, a form, another key, or the key's release.
    for focus in [Focus::Sidebar, Focus::Form] {
        assert_eq!(
            keys::copy_instead(&ctrl_c, focus, &mut gate, &mut tabs),
            None
        );
    }
    let other = press('d', CTRL);
    assert_eq!(
        keys::copy_instead(&other, Focus::Terminal, &mut gate, &mut tabs),
        None
    );
    let release = KeyEvent {
        pressed: false,
        ..ctrl_c.clone()
    };
    assert_eq!(
        keys::copy_instead(&release, Focus::Terminal, &mut gate, &mut tabs),
        None
    );
    assert_eq!(
        keys::copy_instead(&ctrl_c, Focus::Terminal, &mut gate, &mut tabs),
        Some("hello".to_string())
    );
    assert_eq!(tabs.session(0).selected_text(), None, "the selection ended");
    // Held down, its repeats do not interrupt after all.
    let repeat = KeyEvent {
        repeat: true,
        ..ctrl_c.clone()
    };
    assert_eq!(
        keys::copy_instead(&repeat, Focus::Terminal, &mut gate, &mut tabs),
        None
    );
    let (route, _) = keys::dispatch(now_ns(), repeat, Focus::Terminal, &mut gate, &mut tabs);
    assert_eq!(route, Route::Drop);
    let text = flushed(&mut tabs);
    assert!(!text.contains("^C"), "the copy sent no interrupt: {text}");

    // Nothing selected: Ctrl+C is the terminal's key and the program reads 0x03.
    assert_eq!(
        keys::copy_instead(&ctrl_c, Focus::Terminal, &mut gate, &mut tabs),
        None
    );
    let (route, _) = keys::dispatch(now_ns(), ctrl_c, Focus::Terminal, &mut gate, &mut tabs);
    assert_eq!(route, Route::Terminal);
    until_screen(&mut tabs, "^C is shown", |text| text.contains("^C"));
}
