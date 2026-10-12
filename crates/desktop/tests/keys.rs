//! The window's own bindings per focus, and that everything else reaches the terminal.

use client_core::SessionKey;
use client_core::tmux::{NewSession, Start};
use desktop::forms::Request;
use desktop::keys::{
    Focus, Global, HELP, ListKey, PanelKey, RepeatGate, Route, focus_after_close,
    focus_after_panel, global, help_panel, help_text, keys_at, leaves_panel, list_keeps_keys,
    list_key, panel_key, route,
};
use desktop::tabs::TabState;
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
const ALT: Mods = Mods { alt: true, ..NONE };
const ALT_SHIFT: Mods = Mods {
    alt: true,
    shift: true,
    ..NONE
};
const CTRL_SHIFT: Mods = Mods {
    ctrl: true,
    shift: true,
    ..NONE
};

const FOCUSES: [Focus; 3] = [Focus::Terminal, Focus::Sidebar, Focus::Form];
const ENDED: TabState = TabState::Ended { exit: None };

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

fn repeat(ev: &KeyEvent) -> KeyEvent {
    KeyEvent {
        repeat: true,
        ..ev.clone()
    }
}

fn release(ev: &KeyEvent) -> KeyEvent {
    KeyEvent {
        pressed: false,
        text: None,
        ..ev.clone()
    }
}

#[test]
fn alt_shift_keys_work_in_the_terminal_and_the_list_but_not_in_a_form() {
    let bound = [
        (named(Named::Right, ALT_SHIFT), Global::NextTab),
        (named(Named::Left, ALT_SHIFT), Global::PrevTab),
        (named(Named::Up, ALT_SHIFT), Global::FocusSidebar),
        (ch('N', ALT_SHIFT), Global::NextWaiting),
        (ch('n', ALT_SHIFT), Global::NextWaiting),
        (ch('W', ALT_SHIFT), Global::CloseTab),
    ];
    for (ev, expected) in bound {
        assert_eq!(
            global(&ev, Focus::Terminal),
            Some(expected),
            "{ev:?} in the terminal"
        );
        assert_eq!(
            global(&ev, Focus::Sidebar),
            Some(expected),
            "{ev:?} in the list"
        );
        assert_eq!(global(&ev, Focus::Form), None, "{ev:?} in a form");
        assert_eq!(route(&ev, Focus::Form, Some(TabState::Live)), Route::Form);
    }
}

#[test]
fn quit_and_zoom_work_with_every_focus() {
    let bound = [
        (ch('q', CTRL), Global::Quit),
        (ch('=', CTRL), Global::ZoomIn),
        (ch('+', CTRL_SHIFT), Global::ZoomIn),
        (ch('-', CTRL), Global::ZoomOut),
    ];
    for (ev, expected) in bound {
        for focus in FOCUSES {
            assert_eq!(global(&ev, focus), Some(expected), "{ev:?} with {focus:?}");
        }
    }
}

#[test]
fn esc_leaves_the_list_and_nothing_else() {
    let esc = named(Named::Escape, NONE);
    assert_eq!(global(&esc, Focus::Sidebar), Some(Global::FocusTerminal));
    assert_eq!(global(&esc, Focus::Terminal), None);
    assert_eq!(global(&esc, Focus::Form), None);
    assert_eq!(
        route(&esc, Focus::Terminal, Some(TabState::Live)),
        Route::Terminal
    );
    assert_eq!(route(&esc, Focus::Form, Some(TabState::Live)), Route::Form);
}

#[test]
fn a_binding_fires_on_the_press_and_its_release_goes_nowhere() {
    let quit = ch('q', CTRL);
    let next = named(Named::Right, ALT_SHIFT);
    for focus in [Focus::Terminal, Focus::Sidebar] {
        assert_eq!(global(&release(&quit), focus), None);
        assert_eq!(
            route(&release(&quit), focus, Some(TabState::Live)),
            Route::Drop
        );
        assert_eq!(
            route(&release(&next), focus, Some(TabState::Live)),
            Route::Drop
        );
        // Holding the key: tabs keep switching, the window does not quit twice.
        assert_eq!(global(&repeat(&next), focus), Some(Global::NextTab));
        assert_eq!(global(&repeat(&quit), focus), None);
        assert_eq!(
            route(&repeat(&quit), focus, Some(TabState::Live)),
            Route::Drop
        );
    }
}

#[test]
fn enter_on_an_ended_tab_reattaches() {
    let enter = named(Named::Enter, NONE);
    assert_eq!(
        route(&enter, Focus::Terminal, Some(ENDED)),
        Route::Global(Global::Reattach)
    );
    assert_eq!(
        route(&repeat(&enter), Focus::Terminal, Some(ENDED)),
        Route::Drop
    );
    assert_eq!(
        route(&release(&enter), Focus::Terminal, Some(ENDED)),
        Route::Drop
    );
    // On a live tab Enter is the program's; in the list it is the list's.
    assert_eq!(
        route(&enter, Focus::Terminal, Some(TabState::Live)),
        Route::Terminal
    );
    assert_eq!(route(&enter, Focus::Sidebar, Some(ENDED)), Route::Sidebar);
    assert_eq!(global(&enter, Focus::Terminal), None);
}

#[test]
fn keys_for_an_ended_tab_or_no_tab_are_dropped() {
    for ev in [
        ch('a', NONE),
        named(Named::Tab, NONE),
        ch('c', CTRL),
        named(Named::Enter, SHIFT),
    ] {
        assert_eq!(
            route(&ev, Focus::Terminal, Some(ENDED)),
            Route::Drop,
            "{ev:?}"
        );
        assert_eq!(route(&ev, Focus::Terminal, None), Route::Drop, "{ev:?}");
    }
    // The window's own keys still work there.
    let next = named(Named::Right, ALT_SHIFT);
    assert_eq!(
        route(&next, Focus::Terminal, Some(ENDED)),
        Route::Global(Global::NextTab)
    );
    assert_eq!(
        route(&ch('q', CTRL), Focus::Terminal, None),
        Route::Global(Global::Quit)
    );
}

/// Thirty keys a program in the terminal must get.
fn terminal_keys() -> Vec<KeyEvent> {
    vec![
        named(Named::Tab, NONE),
        named(Named::Tab, SHIFT),
        named(Named::Tab, CTRL),
        named(Named::Tab, CTRL_SHIFT),
        named(Named::Escape, NONE),
        named(Named::Enter, NONE),
        named(Named::Backspace, NONE),
        ch('c', CTRL),
        ch('d', CTRL),
        ch('z', CTRL),
        ch('w', CTRL),
        ch('_', CTRL_SHIFT),
        ch('f', ALT),
        ch('b', ALT),
        ch('n', ALT),
        ch('w', ALT),
        ch('N', SHIFT),
        ch('a', NONE),
        ch(' ', NONE),
        ch('=', NONE),
        ch('-', NONE),
        named(Named::Up, NONE),
        named(Named::Right, ALT),
        named(Named::Left, SHIFT),
        named(Named::Up, CTRL),
        named(Named::F(1), NONE),
        named(Named::F(5), NONE),
        named(Named::F(12), SHIFT),
        named(Named::PageUp, NONE),
        named(Named::Delete, NONE),
    ]
}

#[test]
fn terminal_focus_passes_everything_else() {
    let keys = terminal_keys();
    assert_eq!(keys.len(), 30);
    for ev in keys {
        for ev in [ev.clone(), repeat(&ev), release(&ev)] {
            assert_eq!(global(&ev, Focus::Terminal), None, "{ev:?}");
            assert_eq!(
                route(&ev, Focus::Terminal, Some(TabState::Live)),
                Route::Terminal,
                "{ev:?}"
            );
        }
    }
}

#[test]
fn list_and_form_focus_keep_those_keys_from_the_terminal() {
    for ev in terminal_keys() {
        // Esc is the list's own way out.
        if ev != named(Named::Escape, NONE) {
            assert_eq!(
                route(&ev, Focus::Sidebar, Some(TabState::Live)),
                Route::Sidebar,
                "{ev:?}"
            );
        }
        assert_eq!(
            route(&ev, Focus::Form, Some(TabState::Live)),
            Route::Form,
            "{ev:?}"
        );
    }
}

#[test]
fn list_keys() {
    assert_eq!(list_key(&ch('j', NONE)), Some(ListKey::Down));
    assert_eq!(list_key(&named(Named::Down, NONE)), Some(ListKey::Down));
    assert_eq!(list_key(&ch('k', NONE)), Some(ListKey::Up));
    assert_eq!(list_key(&named(Named::Up, NONE)), Some(ListKey::Up));
    assert_eq!(list_key(&repeat(&ch('j', NONE))), Some(ListKey::Down));
    let enter = named(Named::Enter, NONE);
    assert_eq!(list_key(&enter), Some(ListKey::Open));
    assert_eq!(list_key(&repeat(&enter)), None);
    assert_eq!(list_key(&release(&enter)), None);
    assert_eq!(list_key(&ch('j', CTRL)), None);
    assert_eq!(list_key(&ch('x', NONE)), None);
    // Pin and "next waiting" act once per press.
    assert_eq!(list_key(&ch('p', NONE)), Some(ListKey::Pin));
    assert_eq!(list_key(&ch('n', NONE)), Some(ListKey::NextWaiting));
    assert_eq!(list_key(&ch('c', NONE)), Some(ListKey::New));
    assert_eq!(list_key(&ch('m', NONE)), Some(ListKey::Menu));
    assert_eq!(list_key(&ch('a', NONE)), Some(ListKey::Alerts));
    assert_eq!(list_key(&repeat(&ch('a', NONE))), None);
    assert_eq!(list_key(&ch('a', ALT_SHIFT)), None);
    assert_eq!(list_key(&repeat(&ch('c', NONE))), None);
    assert_eq!(list_key(&repeat(&ch('m', NONE))), None);
    assert_eq!(list_key(&repeat(&ch('p', NONE))), None);
    assert_eq!(list_key(&repeat(&ch('n', NONE))), None);
    assert_eq!(list_key(&ch('P', SHIFT)), None);
}

#[test]
fn a_held_key_does_not_type_into_the_session_that_just_got_the_keys() {
    let enter = named(Named::Enter, NONE);
    let mut gate = RepeatGate::default();
    assert!(
        gate.passes(&repeat(&enter)),
        "nothing changed: repeats pass"
    );
    gate.hold();
    assert!(!gate.passes(&repeat(&enter)));
    assert!(gate.passes(&release(&enter)), "a release types nothing");
    assert!(!gate.passes(&repeat(&enter)));
    assert!(gate.passes(&ch('a', NONE)), "a fresh press ends it");
    assert!(gate.passes(&repeat(&ch('a', NONE))));
}

#[test]
fn closing_a_tab_in_the_back_leaves_the_keys_where_they_are() {
    // The `×` of a tab in the back, clicked while the list has the keys: the tab in front
    // stays in front, and `j` / `k` stay the list's.
    assert_eq!(
        focus_after_close(Focus::Sidebar, false, false),
        Focus::Sidebar
    );
    assert_eq!(
        focus_after_close(Focus::Terminal, false, false),
        Focus::Terminal
    );
    assert_eq!(focus_after_close(Focus::Form, false, false), Focus::Form);
    // The tab in front was closed: the one that took its place gets the keys.
    for focus in FOCUSES {
        assert_eq!(focus_after_close(focus, true, false), Focus::Terminal);
        // The last tab: there is no terminal to give them to.
        assert_eq!(focus_after_close(focus, true, true), Focus::Sidebar);
    }
    // And the keys the list then gets reach no session.
    let j = ch('j', NONE);
    let focus = focus_after_close(Focus::Sidebar, false, false);
    assert_eq!(route(&j, focus, Some(TabState::Live)), Route::Sidebar);
}

#[test]
fn a_rename_or_kill_asked_from_the_list_leaves_the_keys_in_the_list() {
    let key = SessionKey {
        host: "here".into(),
        name: "one".into(),
    };
    let rename = Request::Rename(key.clone(), "two".into());
    let kill = Request::Kill(key);
    let new = Request::New(NewSession {
        host: "here".into(),
        folder: String::new(),
        name: "three".into(),
        start: Start::Shell,
        size: None,
    });

    // Where the keys are: with a form or menu open, where they go when it closes.
    assert_eq!(
        keys_at(Focus::Sidebar, Focus::Terminal, false),
        Focus::Sidebar
    );
    assert_eq!(
        keys_at(Focus::Terminal, Focus::Sidebar, false),
        Focus::Terminal
    );
    assert_eq!(keys_at(Focus::Form, Focus::Sidebar, true), Focus::Sidebar);
    assert_eq!(keys_at(Focus::Form, Focus::Terminal, true), Focus::Terminal);

    // The answer arrives with the keys in the list, or in a form opened from the list since:
    // the tab that comes to the front does not take them.
    for modal in [false, true] {
        let in_list = if modal {
            keys_at(Focus::Form, Focus::Sidebar, true)
        } else {
            keys_at(Focus::Sidebar, Focus::Terminal, false)
        };
        assert!(
            list_keeps_keys(&rename, in_list),
            "rename, form open: {modal}"
        );
        assert!(list_keeps_keys(&kill, in_list), "kill, form open: {modal}");
        // A new session is asked for to work in it: its tab takes the keys, as before.
        assert!(!list_keeps_keys(&new, in_list), "new, form open: {modal}");

        // The keys were in the terminal: as before, they go where the tabs put them.
        let in_terminal = if modal {
            keys_at(Focus::Form, Focus::Terminal, true)
        } else {
            keys_at(Focus::Terminal, Focus::Sidebar, false)
        };
        for request in [&rename, &kill, &new] {
            assert!(!list_keeps_keys(request, in_terminal), "{request:?}");
        }
    }
}

#[test]
fn alt_shift_x_closes_the_file_panel_from_the_terminal_and_the_list() {
    for ev in [ch('X', ALT_SHIFT), ch('x', ALT_SHIFT)] {
        assert_eq!(global(&ev, Focus::Terminal), Some(Global::ClosePanel));
        assert_eq!(global(&ev, Focus::Sidebar), Some(Global::ClosePanel));
        assert_eq!(global(&ev, Focus::Form), None, "a form keeps its keys");
        // Bound: it is not typed into the session, and holding it does it once.
        assert_eq!(
            route(&ev, Focus::Terminal, Some(TabState::Live)),
            Route::Global(Global::ClosePanel)
        );
        assert_eq!(
            route(&repeat(&ev), Focus::Terminal, Some(TabState::Live)),
            Route::Drop
        );
        assert_eq!(
            route(&release(&ev), Focus::Terminal, Some(TabState::Live)),
            Route::Drop
        );
    }
    // Alone, x is the session's.
    let plain = ch('x', Mods::default());
    assert_eq!(
        route(&plain, Focus::Terminal, Some(TabState::Live)),
        Route::Terminal
    );
}

#[test]
fn alt_shift_f_opens_and_closes_the_files_panel() {
    for ev in [ch('F', ALT_SHIFT), ch('f', ALT_SHIFT)] {
        for focus in [Focus::Terminal, Focus::Sidebar, Focus::Panel] {
            assert_eq!(global(&ev, focus), Some(Global::ToggleFiles), "{focus:?}");
        }
        assert_eq!(global(&ev, Focus::Form), None, "a form keeps its keys");
        // Bound: it is not typed into the session, and holding it does it once.
        assert_eq!(
            route(&ev, Focus::Terminal, Some(TabState::Live)),
            Route::Global(Global::ToggleFiles)
        );
        assert_eq!(
            route(&repeat(&ev), Focus::Terminal, Some(TabState::Live)),
            Route::Drop
        );
        assert_eq!(
            route(&release(&ev), Focus::Terminal, Some(TabState::Live)),
            Route::Drop
        );
    }
    // Alone, or with one of the two, f is the session's.
    for mods in [NONE, SHIFT, ALT, CTRL] {
        assert_eq!(
            route(&ch('f', mods), Focus::Terminal, Some(TabState::Live)),
            Route::Terminal,
            "{mods:?}"
        );
    }
}

#[test]
fn with_the_keys_in_the_panel_no_key_reaches_a_session() {
    for tab in [Some(TabState::Live), Some(ENDED), None] {
        for ev in [
            ch('j', NONE),
            ch('x', NONE),
            ch('c', CTRL),
            named(Named::Enter, NONE),
            named(Named::Escape, NONE),
            named(Named::Backspace, NONE),
            named(Named::Tab, NONE),
        ] {
            assert_eq!(route(&ev, Focus::Panel, tab), Route::Panel, "{ev:?}");
            assert_eq!(route(&release(&ev), Focus::Panel, tab), Route::Panel);
        }
        // The window's own bindings work from there.
        assert_eq!(
            route(&ch('q', CTRL), Focus::Panel, tab),
            Route::Global(Global::Quit)
        );
        assert_eq!(
            route(&ch('x', ALT_SHIFT), Focus::Panel, tab),
            Route::Global(Global::ClosePanel)
        );
        assert_eq!(
            route(&named(Named::Up, ALT_SHIFT), Focus::Panel, tab),
            Route::Global(Global::FocusSidebar)
        );
    }
}

#[test]
fn the_panels_keys_move_open_and_go_back() {
    for (ev, key) in [
        (named(Named::Up, NONE), PanelKey::Up),
        (ch('k', NONE), PanelKey::Up),
        (named(Named::Down, NONE), PanelKey::Down),
        (ch('j', NONE), PanelKey::Down),
        (named(Named::Enter, NONE), PanelKey::Open),
        (named(Named::Backspace, NONE), PanelKey::Back),
        (named(Named::Escape, NONE), PanelKey::Back),
    ] {
        assert_eq!(panel_key(&ev), Some(key), "{ev:?}");
        assert_eq!(panel_key(&release(&ev)), None, "{ev:?} released");
        // Held, the cursor keeps moving; nothing is opened or left twice.
        let moves = matches!(key, PanelKey::Up | PanelKey::Down);
        assert_eq!(panel_key(&repeat(&ev)), moves.then_some(key), "{ev:?} held");
    }
    for ev in [
        ch('x', NONE),
        ch('j', SHIFT),
        ch('j', CTRL),
        named(Named::Enter, SHIFT),
        named(Named::Escape, ALT),
        named(Named::Tab, NONE),
    ] {
        assert_eq!(panel_key(&ev), None, "{ev:?}");
    }
}

#[test]
fn a_key_the_panel_does_not_bind_takes_the_keys_back_to_the_terminal() {
    // Typed text, Tab, Ctrl+C, a capital, a chord: none of them is the panel's, none may vanish.
    for ev in [
        ch('x', NONE),
        ch('l', NONE),
        ch(' ', NONE),
        ch('J', SHIFT),
        ch('j', ALT),
        ch('c', CTRL),
        ch('d', CTRL),
        named(Named::Tab, NONE),
        named(Named::Enter, SHIFT),
        named(Named::Escape, ALT),
        named(Named::Left, NONE),
        named(Named::Up, SHIFT),
    ] {
        assert_eq!(panel_key(&ev), None, "{ev:?}");
        assert!(leaves_panel(&ev), "{ev:?}");
        // Its release and its repeats come after the keys moved: they move nothing again.
        assert!(!leaves_panel(&release(&ev)), "{ev:?} released");
        assert!(!leaves_panel(&repeat(&ev)), "{ev:?} held");
    }
    // The panel's own keys stay, pressed, held or released: Enter held opens once and types
    // no line into the session behind.
    for ev in [
        ch('j', NONE),
        ch('k', NONE),
        named(Named::Up, NONE),
        named(Named::Down, NONE),
        named(Named::Enter, NONE),
        named(Named::Backspace, NONE),
        named(Named::Escape, NONE),
    ] {
        for ev in [ev.clone(), repeat(&ev), release(&ev)] {
            assert!(!leaves_panel(&ev), "{ev:?}");
        }
    }
    // The window's own bindings are done where the keys are.
    for ev in [
        ch('f', ALT_SHIFT),
        ch('x', ALT_SHIFT),
        ch('w', ALT_SHIFT),
        named(Named::Right, ALT_SHIFT),
        ch('q', CTRL),
        ch('=', CTRL),
    ] {
        assert!(!leaves_panel(&ev), "{ev:?}");
        assert!(!leaves_panel(&repeat(&ev)), "{ev:?} held");
    }
}

#[test]
fn a_panel_that_is_not_drawn_does_not_keep_the_keys() {
    // The panel's list has the keys and the window is made so narrow that the layout has no
    // panel any more: `j` would move a cursor nobody sees. The terminal gets them.
    assert_eq!(
        focus_after_panel(Focus::Panel, true, false, true),
        Focus::Terminal
    );
    // No tab: the list of sessions.
    assert_eq!(
        focus_after_panel(Focus::Panel, true, false, false),
        Focus::Sidebar
    );
    // It is drawn and takes keys: nothing moves.
    assert_eq!(
        focus_after_panel(Focus::Panel, true, true, true),
        Focus::Panel
    );
    // Drawn, but what it shows now takes none (a file from the terminal, nothing).
    assert_eq!(
        focus_after_panel(Focus::Panel, false, true, true),
        Focus::Terminal
    );
    // Whoever else has the keys keeps them, whatever becomes of the panel.
    for focus in [Focus::Terminal, Focus::Sidebar, Focus::Form] {
        for (takes, drawn, tabs) in [
            (false, false, true),
            (true, false, false),
            (true, true, true),
        ] {
            assert_eq!(focus_after_panel(focus, takes, drawn, tabs), focus);
        }
    }
    // And the key that follows goes to the session.
    let focus = focus_after_panel(Focus::Panel, true, false, true);
    assert_eq!(
        route(&ch('j', NONE), focus, Some(TabState::Live)),
        Route::Terminal
    );
}

#[test]
fn question_mark_in_the_list_opens_the_help() {
    assert_eq!(list_key(&ch('?', SHIFT)), Some(ListKey::Help));
    assert_eq!(list_key(&ch('?', NONE)), Some(ListKey::Help));
    assert_eq!(list_key(&repeat(&ch('?', SHIFT))), None);
    assert_eq!(list_key(&ch('?', CTRL)), None);
    assert_eq!(list_key(&ch('?', ALT_SHIFT)), None);
}

/// The chords of a group, split into their words (`j / k  ↑ / ↓` → j, k, ↑, ↓).
fn chords(group: &str) -> Vec<&'static str> {
    let rows = HELP.iter().find(|(name, _)| *name == group).expect(group).1;
    rows.iter()
        .flat_map(|(chord, _)| chord.split([' ', '/']))
        .filter(|word| !word.is_empty())
        .collect()
}

#[test]
fn the_help_names_every_key_of_the_list_and_every_alt_shift_letter() {
    let list = chords("List");
    for c in ['j', 'k', 'p', 'n', 'c', 'm', 'a', 'y', '?'] {
        let mods = if c == '?' { SHIFT } else { NONE };
        assert!(list_key(&ch(c, mods)).is_some(), "{c} is a key of the list");
        assert!(list.contains(&c.to_string().as_str()), "{c} is in the help");
    }
    let everywhere: Vec<&str> = HELP[0].1.iter().map(|(chord, _)| *chord).collect();
    assert_eq!(HELP[0].0, "Everywhere");
    for c in ['n', 'w', 'x', 'f'] {
        assert!(
            global(&ch(c, ALT_SHIFT), Focus::Terminal).is_some(),
            "Alt+Shift+{c} is bound"
        );
        let chord = format!("Alt+Shift+{}", c.to_ascii_uppercase());
        assert!(
            everywhere.contains(&chord.as_str()),
            "{chord} is in the help"
        );
    }
}

#[test]
fn the_help_panel_shows_every_row_and_fits_a_small_window() {
    let panel = help_panel();
    let texts: Vec<&str> = panel.cells.iter().map(|cell| cell.text.as_str()).collect();
    for (group, rows) in HELP {
        assert!(texts.contains(group), "{group}");
        for (chord, what) in *rows {
            assert!(texts.contains(chord), "{chord} uncut");
            assert!(texts.contains(what), "{what} uncut");
        }
    }
    assert!(panel.lines <= 40, "{} lines", panel.lines);
    assert!(panel.cols <= 110, "{} columns", panel.cols);
}

#[test]
fn help_text_is_the_same_table() {
    let text = help_text();
    for (group, rows) in HELP {
        assert!(text.contains(&format!("{group}\n")));
        for (chord, what) in *rows {
            assert!(
                text.lines()
                    .any(|line| line.trim_start().starts_with(chord) && line.ends_with(what)),
                "{chord}"
            );
        }
    }
}

#[test]
fn y_in_the_list_approves() {
    assert_eq!(list_key(&ch('y', NONE)), Some(ListKey::Approve));
    assert_eq!(list_key(&repeat(&ch('y', NONE))), None);
    assert_eq!(list_key(&ch('Y', SHIFT)), None);
}
