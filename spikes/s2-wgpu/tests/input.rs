//! winit key parts → the contract `KeyEvent`; app shortcuts; IME commit; wheel.

use s2_core::{Key, KeyEvent, Mods, Named};
use s2_wgpu::input::{Action, ime_commit, key_action, wheel_rows};
use winit::dpi::PhysicalPosition;
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};

const NONE: ModifiersState = ModifiersState::empty();
const SHIFT: ModifiersState = ModifiersState::SHIFT;
const CTRL: ModifiersState = ModifiersState::CONTROL;
const ALT: ModifiersState = ModifiersState::ALT;
const LOGO: ModifiersState = ModifiersState::SUPER;

fn ch(s: &str) -> WKey {
    WKey::Character(s.into())
}

fn press(key: Key, mods: Mods, text: Option<&str>) -> Action {
    Action::Send(KeyEvent {
        key,
        mods,
        text: text.map(str::to_string),
        repeat: false,
        pressed: true,
    })
}

fn mods(shift: bool, ctrl: bool, alt: bool, logo: bool) -> Mods {
    Mods {
        shift,
        ctrl,
        alt,
        logo,
    }
}

#[test]
fn printable_keys_keep_their_text() {
    assert_eq!(
        key_action(&ch("a"), Some("a"), NONE, true, false),
        press(Key::Char('a'), Mods::default(), Some("a"))
    );
    assert_eq!(
        key_action(&ch("A"), Some("A"), SHIFT, true, false),
        press(Key::Char('A'), mods(true, false, false, false), Some("A"))
    );
    assert_eq!(
        key_action(&ch("é"), Some("é"), NONE, true, false),
        press(Key::Char('é'), Mods::default(), Some("é"))
    );
    // winit reports no text (some Alt chords): the logical character stands in.
    assert_eq!(
        key_action(&ch("a"), None, ALT, true, false),
        press(Key::Char('a'), mods(false, false, true, false), Some("a"))
    );
    assert_eq!(
        key_action(&ch("a"), Some("a"), ALT | SHIFT | LOGO, true, false),
        press(Key::Char('a'), mods(true, false, true, true), Some("a"))
    );
    // Space is a named key in winit and a character in the contract.
    assert_eq!(
        key_action(&WKey::Named(NamedKey::Space), Some(" "), NONE, true, false),
        press(Key::Char(' '), Mods::default(), Some(" "))
    );
}

#[test]
fn ctrl_combinations_carry_no_text() {
    // Contract (A1 fix wave): with Ctrl pass `None` or the control character, never printable
    // text — `Some("[")` would make the core emit CSI-u for Ctrl+[.
    let ctrl = mods(false, true, false, false);
    for (key, text) in [
        ("c", "\u{3}"),
        ("c", "c"),
        ("[", "["),
        ("[", "\u{1b}"),
        ("/", "/"),
    ] {
        let first = key.chars().next().unwrap();
        assert_eq!(
            key_action(&ch(key), Some(text), CTRL, true, false),
            press(Key::Char(first), ctrl, None),
            "Ctrl+{key} with text {text:?}"
        );
    }
    assert_eq!(
        key_action(&WKey::Named(NamedKey::Space), Some(" "), CTRL, true, false),
        press(Key::Char(' '), ctrl, None)
    );
    assert_eq!(
        key_action(&ch("_"), Some("_"), CTRL | SHIFT, true, false),
        press(Key::Char('_'), mods(true, true, false, false), None)
    );
    assert_eq!(
        key_action(&ch("x"), Some("x"), CTRL | ALT, true, false),
        press(Key::Char('x'), mods(false, true, true, false), None)
    );
}

#[test]
fn named_keys() {
    let table = [
        (NamedKey::Enter, Named::Enter),
        (NamedKey::Tab, Named::Tab),
        (NamedKey::Backspace, Named::Backspace),
        (NamedKey::Escape, Named::Escape),
        (NamedKey::ArrowUp, Named::Up),
        (NamedKey::ArrowDown, Named::Down),
        (NamedKey::ArrowLeft, Named::Left),
        (NamedKey::ArrowRight, Named::Right),
        (NamedKey::Home, Named::Home),
        (NamedKey::End, Named::End),
        (NamedKey::PageUp, Named::PageUp),
        (NamedKey::PageDown, Named::PageDown),
        (NamedKey::Insert, Named::Insert),
        (NamedKey::Delete, Named::Delete),
        (NamedKey::F1, Named::F(1)),
        (NamedKey::F5, Named::F(5)),
        (NamedKey::F12, Named::F(12)),
    ];
    for (winit_key, named) in table {
        // winit attaches "\r", "\t", "\x7f", "\x1b" to some of these; the key decides.
        assert_eq!(
            key_action(&WKey::Named(winit_key), Some("\r"), NONE, true, false),
            press(Key::Named(named), Mods::default(), None),
            "{winit_key:?}"
        );
    }
    assert_eq!(
        key_action(&WKey::Named(NamedKey::Tab), None, SHIFT, true, false),
        press(
            Key::Named(Named::Tab),
            mods(true, false, false, false),
            None
        )
    );
    assert_eq!(
        key_action(
            &WKey::Named(NamedKey::ArrowUp),
            None,
            CTRL | ALT,
            true,
            false
        ),
        press(Key::Named(Named::Up), mods(false, true, true, false), None)
    );
}

#[test]
fn keys_that_send_nothing() {
    for key in [
        WKey::Named(NamedKey::Shift),
        WKey::Named(NamedKey::Control),
        WKey::Named(NamedKey::Alt),
        WKey::Named(NamedKey::Super),
        WKey::Named(NamedKey::CapsLock),
        WKey::Named(NamedKey::F13),
        WKey::Named(NamedKey::AudioVolumeUp),
        WKey::Dead(Some('´')),
        ch(""),
    ] {
        assert_eq!(
            key_action(&key, None, NONE, true, false),
            Action::Ignore,
            "{key:?}"
        );
        assert_eq!(key_action(&key, None, NONE, false, false), Action::Ignore);
    }
}

#[test]
fn repeat_and_release() {
    assert_eq!(
        key_action(&ch("a"), Some("a"), NONE, true, true),
        Action::Send(KeyEvent {
            key: Key::Char('a'),
            mods: Mods::default(),
            text: Some("a".to_string()),
            repeat: true,
            pressed: true,
        })
    );
    // A release has no text and is never a repeat.
    for repeat in [false, true] {
        assert_eq!(
            key_action(&ch("a"), Some("a"), SHIFT, false, repeat),
            Action::Send(KeyEvent {
                key: Key::Char('a'),
                mods: mods(true, false, false, false),
                text: None,
                repeat: false,
                pressed: false,
            })
        );
    }
    assert_eq!(
        key_action(&WKey::Named(NamedKey::Enter), None, NONE, true, true),
        Action::Send(KeyEvent {
            key: Key::Named(Named::Enter),
            mods: Mods::default(),
            text: None,
            repeat: true,
            pressed: true,
        })
    );
}

#[test]
fn copy_and_paste_shortcuts_stay_in_the_app() {
    let both = CTRL | SHIFT;
    assert_eq!(
        key_action(&ch("C"), Some("C"), both, true, false),
        Action::Copy
    );
    assert_eq!(
        key_action(&ch("V"), Some("V"), both, true, false),
        Action::Paste
    );
    // Caps lock on, or a layout that reports the unshifted letter.
    assert_eq!(key_action(&ch("c"), None, both, true, false), Action::Copy);
    assert_eq!(key_action(&ch("v"), None, both, true, false), Action::Paste);
    // Holding the chord does not paste again, and the child sees neither repeat nor release.
    assert_eq!(key_action(&ch("V"), None, both, true, true), Action::Ignore);
    assert_eq!(
        key_action(&ch("V"), None, both, false, false),
        Action::Ignore
    );
    assert_eq!(
        key_action(&ch("C"), None, both, false, false),
        Action::Ignore
    );
    // Only exactly Ctrl+Shift: Ctrl+C is still an interrupt, Ctrl+Shift+Alt+C goes to the child.
    assert!(matches!(
        key_action(&ch("c"), None, CTRL, true, false),
        Action::Send(_)
    ));
    assert!(matches!(
        key_action(&ch("C"), None, both | ALT, true, false),
        Action::Send(_)
    ));
    assert!(matches!(
        key_action(&ch("X"), None, both, true, false),
        Action::Send(_)
    ));
}

#[test]
fn ime_commit_is_one_key_event_with_the_whole_text() {
    assert_eq!(
        ime_commit("你好"),
        Some(KeyEvent {
            key: Key::Char('你'),
            mods: Mods::default(),
            text: Some("你好".to_string()),
            repeat: false,
            pressed: true,
        })
    );
    assert_eq!(ime_commit(""), None);
}

#[test]
fn wheel_becomes_rows() {
    let lines = |y: f32| MouseScrollDelta::LineDelta(0.0, y);
    let pixels = |y: f64| MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, y));
    let mut carry = 0.0;
    // Wheel up (positive y) goes into the scrollback: negative rows.
    assert_eq!(wheel_rows(&lines(1.0), 17.0, &mut carry), -3);
    assert_eq!(wheel_rows(&lines(-2.0), 17.0, &mut carry), 6);
    assert_eq!(
        wheel_rows(&MouseScrollDelta::LineDelta(5.0, 0.0), 17.0, &mut carry),
        0
    );
    assert_eq!(carry, 0.0);

    // Touchpad pixels: whole rows now, the rest is carried to the next event.
    assert_eq!(wheel_rows(&pixels(34.0), 17.0, &mut carry), -2);
    assert_eq!(wheel_rows(&pixels(10.0), 17.0, &mut carry), 0);
    assert_eq!(wheel_rows(&pixels(10.0), 17.0, &mut carry), -1);
    assert_eq!(wheel_rows(&pixels(-20.0), 17.0, &mut carry), 1);
    assert_eq!(wheel_rows(&pixels(-5.0), 17.0, &mut carry), 0);
    // Half-line wheel notches accumulate as well.
    let mut carry = 0.0;
    assert_eq!(wheel_rows(&lines(0.1), 17.0, &mut carry), 0);
    assert_eq!(wheel_rows(&lines(0.3), 17.0, &mut carry), -1);
    // A broken cell height or delta does not panic or scroll.
    let mut carry = 0.0;
    assert_eq!(wheel_rows(&pixels(50.0), 0.0, &mut carry), 0);
    assert_eq!(wheel_rows(&lines(f32::NAN), 17.0, &mut carry), 0);
    assert_eq!(wheel_rows(&lines(1.0), 17.0, &mut carry), -3);
}
