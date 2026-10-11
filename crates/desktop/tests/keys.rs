//! The window's own bindings per focus, and that everything else reaches the terminal.

use desktop::keys::{
    Focus, Global, ListKey, RepeatGate, Route, focus_after_close, global, list_key, route,
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
