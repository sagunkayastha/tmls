//! The UI logic that needs neither a window nor a font: focus ring, key routing, the text
//! field, the list, the context menu, zoom steps, layout, and the `Ui` state as a whole.

use s2_wgpu::gfx::atlas::CellMetrics;
use s2_wgpu::input::{Action, key_action};
use s2_wgpu::ui::focus::{Dir, FieldKey, Focus, ListKey, MenuKey, Route, route};
use s2_wgpu::ui::layout::{Hit, Layout, Rect, layout};
use s2_wgpu::ui::list::List;
use s2_wgpu::ui::menu::{ITEMS, Menu, MenuItem, Outcome, clear_key};
use s2_wgpu::ui::textfield::TextField;
use s2_wgpu::ui::theme::{UiTheme, ZOOM_MAX, ZOOM_MIN, ZoomDir, zoom_step};
use s2_wgpu::ui::{Effect, Ui};
use winit::event::Ime;
use winit::keyboard::{Key as WKey, ModifiersState as Mods, NamedKey};

const NONE: Mods = Mods::empty();
const CTRL: Mods = Mods::CONTROL;
const SHIFT: Mods = Mods::SHIFT;

fn ch(c: &str) -> WKey {
    WKey::Character(c.into())
}

fn named(key: NamedKey) -> WKey {
    WKey::Named(key)
}

/// A press without repeat.
fn press(focus: Focus, menu: bool, key: &WKey, text: Option<&str>, mods: Mods) -> Route {
    route(focus, menu, key, text, mods, true, false)
}

fn metrics(width: f32, height: f32) -> CellMetrics {
    CellMetrics {
        width,
        height,
        baseline: (height * 0.8).round(),
        underline_y: (height * 0.9).round(),
        strike_y: (height * 0.5).round(),
        stroke: 1.0,
    }
}

/// 1280x800 at scale 1 with a 10x20 cell.
fn plain_layout() -> Layout {
    layout((1280, 800), 1.0, &metrics(10.0, 20.0), &UiTheme::default())
}

fn centre(rect: Rect) -> (f32, f32) {
    (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

// ---------------------------------------------------------------------------------------------
// Focus ring and key routing
// ---------------------------------------------------------------------------------------------

#[test]
fn focus_ring_goes_list_field_terminal_and_back() {
    assert_eq!(Focus::List.step(Dir::Next), Focus::Field);
    assert_eq!(Focus::Field.step(Dir::Next), Focus::Terminal);
    assert_eq!(Focus::Terminal.step(Dir::Next), Focus::List);
    assert_eq!(Focus::List.step(Dir::Prev), Focus::Terminal);
    assert_eq!(Focus::Terminal.step(Dir::Prev), Focus::Field);
    assert_eq!(Focus::Field.step(Dir::Prev), Focus::List);
    for focus in [Focus::List, Focus::Field, Focus::Terminal] {
        assert_eq!(focus.step(Dir::Next).step(Dir::Prev), focus);
        assert_eq!(focus.step(Dir::Next).step(Dir::Next).step(Dir::Next), focus);
    }
}

/// Keys that are not app shortcuts, as `(key, text, mods)`.
fn ordinary_keys() -> Vec<(WKey, Option<&'static str>, Mods)> {
    vec![
        (ch("a"), Some("a"), NONE),
        (ch("A"), Some("A"), SHIFT),
        (ch("j"), Some("j"), NONE),
        (ch("k"), Some("k"), NONE),
        (ch("你"), Some("你"), NONE),
        (ch("c"), None, CTRL),
        (ch("a"), None, CTRL),
        (ch("v"), None, CTRL),
        (ch("x"), Some("x"), Mods::ALT),
        (ch("l"), None, CTRL),
        (named(NamedKey::Space), Some(" "), NONE),
        (named(NamedKey::Enter), Some("\r"), NONE),
        (named(NamedKey::Escape), None, NONE),
        (named(NamedKey::Backspace), None, NONE),
        (named(NamedKey::Delete), None, NONE),
        (named(NamedKey::ArrowUp), None, NONE),
        (named(NamedKey::ArrowDown), None, NONE),
        (named(NamedKey::ArrowLeft), None, SHIFT),
        (named(NamedKey::Home), None, NONE),
        (named(NamedKey::PageDown), None, NONE),
        (named(NamedKey::F5), None, NONE),
        (named(NamedKey::Tab), Some("\t"), NONE),
        (named(NamedKey::Tab), None, SHIFT),
        (named(NamedKey::Shift), None, SHIFT),
        (ch("c"), None, CTRL | SHIFT),
        (ch("V"), None, CTRL | SHIFT),
    ]
}

#[test]
fn with_the_terminal_focused_keys_take_the_old_path_unchanged() {
    for (key, text, mods) in ordinary_keys() {
        for (pressed, repeat) in [(true, false), (true, true), (false, false)] {
            let old = key_action(&key, text, mods, pressed, repeat);
            assert_eq!(
                route(Focus::Terminal, false, &key, text, mods, pressed, repeat),
                Route::Terminal(old),
                "{key:?} {mods:?} pressed={pressed} repeat={repeat}"
            );
        }
    }
    // Tab and Shift+Tab belong to the program in the terminal (completion, reverse completion).
    let tab = named(NamedKey::Tab);
    match press(Focus::Terminal, false, &tab, Some("\t"), NONE) {
        Route::Terminal(Action::Send(ev)) => assert!(!ev.mods.shift),
        other => panic!("Tab went to {other:?}"),
    }
    match press(Focus::Terminal, false, &tab, None, SHIFT) {
        Route::Terminal(Action::Send(ev)) => assert!(ev.mods.shift),
        other => panic!("Shift+Tab went to {other:?}"),
    }
}

#[test]
fn nothing_is_a_terminal_key_while_the_focus_is_elsewhere_or_the_menu_is_open() {
    let mut keys = ordinary_keys();
    keys.push((named(NamedKey::Tab), None, CTRL));
    keys.push((ch("="), None, CTRL));
    keys.push((ch("-"), None, CTRL));
    for (key, text, mods) in keys {
        for (pressed, repeat) in [(true, false), (true, true), (false, false)] {
            for (focus, menu) in [
                (Focus::List, false),
                (Focus::Field, false),
                (Focus::List, true),
                (Focus::Field, true),
                (Focus::Terminal, true),
            ] {
                let to = route(focus, menu, &key, text, mods, pressed, repeat);
                assert!(
                    !matches!(to, Route::Terminal(_)),
                    "{key:?} {mods:?} with focus {focus:?}, menu {menu}: {to:?}"
                );
            }
        }
    }
}

#[test]
fn traversal_keys() {
    let tab = named(NamedKey::Tab);
    // Ctrl+Tab / Ctrl+Shift+Tab: from anywhere, the terminal included.
    for focus in [Focus::List, Focus::Field, Focus::Terminal] {
        assert_eq!(
            press(focus, false, &tab, None, CTRL),
            Route::Focus(Dir::Next)
        );
        assert_eq!(
            press(focus, false, &tab, None, CTRL | SHIFT),
            Route::Focus(Dir::Prev)
        );
        // The release of a shortcut goes nowhere (not to the terminal either).
        assert_eq!(
            route(focus, false, &tab, None, CTRL, false, false),
            Route::Ignore
        );
    }
    // Plain Tab / Shift+Tab: only outside the terminal.
    for focus in [Focus::List, Focus::Field] {
        assert_eq!(
            press(focus, false, &tab, Some("\t"), NONE),
            Route::Focus(Dir::Next)
        );
        assert_eq!(
            press(focus, false, &tab, None, SHIFT),
            Route::Focus(Dir::Prev)
        );
        assert_eq!(
            route(focus, false, &tab, None, NONE, false, false),
            Route::Ignore
        );
    }
}

#[test]
fn zoom_keys_work_everywhere() {
    for focus in [Focus::List, Focus::Field, Focus::Terminal] {
        for (key, mods, dir) in [
            ("=", CTRL, ZoomDir::In),
            ("+", CTRL | SHIFT, ZoomDir::In),
            ("-", CTRL, ZoomDir::Out),
            ("_", CTRL | SHIFT, ZoomDir::Out),
        ] {
            assert_eq!(
                press(focus, false, &ch(key), None, mods),
                Route::Zoom(dir),
                "{key} with focus {focus:?}"
            );
            // Holding the key zooms on; the release does nothing.
            assert_eq!(
                route(focus, false, &ch(key), None, mods, true, true),
                Route::Zoom(dir)
            );
            assert_eq!(
                route(focus, false, &ch(key), None, mods, false, false),
                Route::Ignore
            );
        }
        // Without Ctrl, or with Alt as well, these are ordinary keys.
        assert!(!matches!(
            press(focus, false, &ch("="), Some("="), NONE),
            Route::Zoom(_)
        ));
        assert!(!matches!(
            press(focus, false, &ch("-"), None, CTRL | Mods::ALT),
            Route::Zoom(_)
        ));
    }
}

#[test]
fn list_field_and_menu_keys() {
    let list = |key: &WKey, text, mods| press(Focus::List, false, key, text, mods);
    assert_eq!(
        list(&named(NamedKey::ArrowUp), None, NONE),
        Route::List(ListKey::Up)
    );
    assert_eq!(list(&ch("k"), Some("k"), NONE), Route::List(ListKey::Up));
    assert_eq!(
        list(&named(NamedKey::ArrowDown), None, NONE),
        Route::List(ListKey::Down)
    );
    assert_eq!(list(&ch("j"), Some("j"), NONE), Route::List(ListKey::Down));
    assert_eq!(
        list(&named(NamedKey::PageUp), None, NONE),
        Route::List(ListKey::PageUp)
    );
    assert_eq!(
        list(&named(NamedKey::PageDown), None, NONE),
        Route::List(ListKey::PageDown)
    );
    assert_eq!(
        list(&named(NamedKey::Home), None, NONE),
        Route::List(ListKey::Home)
    );
    assert_eq!(
        list(&named(NamedKey::End), None, NONE),
        Route::List(ListKey::End)
    );
    assert_eq!(
        list(&named(NamedKey::Enter), Some("\r"), NONE),
        Route::List(ListKey::Activate)
    );
    assert_eq!(list(&ch("x"), Some("x"), NONE), Route::Ignore);
    assert_eq!(list(&ch("j"), None, CTRL), Route::Ignore);
    assert_eq!(
        route(Focus::List, false, &ch("j"), None, NONE, false, false),
        Route::Ignore,
        "a release"
    );

    let field = |key: &WKey, text, mods| press(Focus::Field, false, key, text, mods);
    let insert = |s: &str| Route::Field(FieldKey::Insert(s.to_string()));
    assert_eq!(field(&ch("a"), Some("a"), NONE), insert("a"));
    assert_eq!(field(&ch("A"), Some("A"), SHIFT), insert("A"));
    assert_eq!(field(&ch("j"), Some("j"), NONE), insert("j"));
    assert_eq!(field(&ch("好"), Some("好"), NONE), insert("好"));
    assert_eq!(field(&ch("é"), None, NONE), insert("é"), "no text: the key");
    assert_eq!(field(&named(NamedKey::Space), Some(" "), NONE), insert(" "));
    assert_eq!(field(&named(NamedKey::Space), None, NONE), insert(" "));
    assert_eq!(
        field(&named(NamedKey::Backspace), Some("\u{8}"), NONE),
        Route::Field(FieldKey::Backspace)
    );
    assert_eq!(
        field(&named(NamedKey::Delete), None, NONE),
        Route::Field(FieldKey::Delete)
    );
    for (select, mods) in [(false, NONE), (true, SHIFT)] {
        assert_eq!(
            field(&named(NamedKey::ArrowLeft), None, mods),
            Route::Field(FieldKey::Left { select })
        );
        assert_eq!(
            field(&named(NamedKey::ArrowRight), None, mods),
            Route::Field(FieldKey::Right { select })
        );
        assert_eq!(
            field(&named(NamedKey::Home), None, mods),
            Route::Field(FieldKey::Home { select })
        );
        assert_eq!(
            field(&named(NamedKey::End), None, mods),
            Route::Field(FieldKey::End { select })
        );
    }
    assert_eq!(
        field(&ch("a"), None, CTRL),
        Route::Field(FieldKey::SelectAll)
    );
    assert_eq!(field(&ch("c"), None, CTRL), Route::Field(FieldKey::Copy));
    assert_eq!(field(&ch("x"), None, CTRL), Route::Field(FieldKey::Cut));
    assert_eq!(field(&ch("v"), None, CTRL), Route::Field(FieldKey::Paste));
    assert_eq!(
        field(&ch("V"), None, CTRL | SHIFT),
        Route::Field(FieldKey::Paste)
    );
    assert_eq!(
        field(&named(NamedKey::Enter), Some("\r"), NONE),
        Route::Field(FieldKey::Submit)
    );
    assert_eq!(
        field(&named(NamedKey::Escape), None, NONE),
        Route::Field(FieldKey::Cancel)
    );
    assert_eq!(field(&ch("q"), None, CTRL), Route::Ignore);
    assert_eq!(field(&ch("x"), Some("x"), Mods::ALT), Route::Ignore);
    assert_eq!(field(&named(NamedKey::F5), None, NONE), Route::Ignore);
    assert_eq!(
        route(Focus::Field, false, &ch("a"), None, NONE, false, false),
        Route::Ignore,
        "a release"
    );

    // An open menu takes every key, whatever has the focus.
    for focus in [Focus::List, Focus::Field, Focus::Terminal] {
        let menu = |key: &WKey, text, mods| press(focus, true, key, text, mods);
        assert_eq!(
            menu(&named(NamedKey::ArrowUp), None, NONE),
            Route::Menu(MenuKey::Up)
        );
        assert_eq!(
            menu(&named(NamedKey::ArrowDown), None, NONE),
            Route::Menu(MenuKey::Down)
        );
        assert_eq!(
            menu(&named(NamedKey::Enter), Some("\r"), NONE),
            Route::Menu(MenuKey::Activate)
        );
        assert_eq!(
            menu(&named(NamedKey::Escape), None, NONE),
            Route::Menu(MenuKey::Close)
        );
        assert_eq!(menu(&ch("a"), Some("a"), NONE), Route::Menu(MenuKey::Close));
        assert_eq!(
            menu(&named(NamedKey::Tab), None, CTRL),
            Route::Menu(MenuKey::Close)
        );
        // A modifier going down on its way to a chord does not close it; nor does a release.
        assert_eq!(menu(&named(NamedKey::Shift), None, SHIFT), Route::Ignore);
        assert_eq!(
            route(focus, true, &ch("a"), None, NONE, false, false),
            Route::Ignore
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Text field
// ---------------------------------------------------------------------------------------------

fn field_with(text: &str) -> TextField {
    let mut field = TextField::new();
    field.insert(text);
    field
}

#[test]
fn text_field_insert_caret_and_delete() {
    let mut f = TextField::new();
    assert_eq!((f.text(), f.caret()), ("", 0));
    f.insert("ac");
    assert_eq!((f.text(), f.caret()), ("ac", 2));
    f.left(false);
    f.insert("b");
    assert_eq!((f.text(), f.caret()), ("abc", 2));

    // Moves stop at the ends.
    f.home(false);
    assert_eq!(f.caret(), 0);
    f.left(false);
    assert_eq!(f.caret(), 0);
    assert!(!f.backspace(), "nothing before the caret");
    assert!(f.delete());
    assert_eq!((f.text(), f.caret()), ("bc", 0));
    f.end(false);
    assert_eq!(f.caret(), 2);
    f.right(false);
    assert_eq!(f.caret(), 2);
    assert!(!f.delete(), "nothing after the caret");
    assert!(f.backspace());
    assert_eq!((f.text(), f.caret()), ("b", 1));

    // Control characters never get in.
    f.insert("x\u{7}\ny\u{1b}");
    assert_eq!(f.text(), "bxy");
    f.clear();
    assert_eq!((f.text(), f.caret(), f.selection()), ("", 0, None));
}

#[test]
fn text_field_moves_and_deletes_whole_characters() {
    // 'a', a wide character (3 bytes), 'e' + combining acute (1 + 2 bytes), 'z'.
    let mut f = field_with("a你e\u{301}z");
    assert_eq!(f.caret(), 8);
    f.left(false);
    assert_eq!(f.caret(), 7, "before z");
    f.left(false);
    assert_eq!(f.caret(), 4, "before e + accent, not between them");
    f.left(false);
    assert_eq!(f.caret(), 1, "before the wide character, not inside it");
    f.right(false);
    assert_eq!(f.caret(), 4);
    f.right(false);
    assert_eq!(f.caret(), 7);

    assert!(f.backspace());
    assert_eq!(
        (f.text(), f.caret()),
        ("a你z", 4),
        "base and accent together"
    );
    assert!(f.backspace());
    assert_eq!((f.text(), f.caret()), ("az", 1), "the whole wide character");
    f.insert("好");
    f.left(false);
    assert_eq!(f.caret(), 1);
    assert!(f.delete());
    assert_eq!((f.text(), f.caret()), ("az", 1));
}

#[test]
fn text_field_selection() {
    let mut f = field_with("ab你cd");
    assert_eq!(f.selection(), None);
    assert_eq!(f.selected_text(), None);

    // Shift+Left twice from the end, then once more over the wide character.
    f.left(true);
    f.left(true);
    assert_eq!(f.selection(), Some(5..7));
    assert_eq!(f.selected_text().as_deref(), Some("cd"));
    f.left(true);
    assert_eq!(f.selection(), Some(2..7));
    assert_eq!(f.selected_text().as_deref(), Some("你cd"));
    // Back the other way shrinks it; meeting the anchor leaves nothing selected.
    f.right(true);
    assert_eq!(f.selection(), Some(5..7));
    f.right(true);
    f.right(true);
    assert_eq!(f.selection(), None);

    // A move without Shift drops the selection and puts the caret at that end of it.
    f.home(false);
    f.right(true);
    f.right(true);
    assert_eq!(f.selection(), Some(0..2));
    f.right(false);
    assert_eq!((f.caret(), f.selection()), (2, None));
    f.right(true);
    assert_eq!(f.selection(), Some(2..5));
    f.left(false);
    assert_eq!((f.caret(), f.selection()), (2, None));

    // Typing replaces the selection; so do Backspace and Delete, without taking a neighbour.
    f.end(true);
    assert_eq!(f.selected_text().as_deref(), Some("你cd"));
    f.insert("X");
    assert_eq!((f.text(), f.caret(), f.selection()), ("abX", 3, None));
    f.home(true);
    assert!(f.backspace());
    assert_eq!((f.text(), f.caret()), ("", 0));
    f.insert("hello");
    f.home(false);
    f.right(true);
    assert!(f.delete());
    assert_eq!((f.text(), f.caret()), ("ello", 0));

    f.select_all();
    assert_eq!(f.selection(), Some(0..4));
    assert_eq!(f.cut().as_deref(), Some("ello"));
    assert_eq!((f.text(), f.caret(), f.selection()), ("", 0, None));
    assert_eq!(f.cut(), None);
    f.select_all();
    assert_eq!(f.selection(), None, "nothing to select");
}

#[test]
fn text_field_paste() {
    let mut f = field_with("ab");
    f.left(false);
    f.paste("x你y");
    assert_eq!((f.text(), f.caret()), ("ax你yb", 6));
    // One line only: line ends and tabs become spaces, other control characters go.
    f.clear();
    f.paste("one\r\ntwo\tthree\u{7}\n");
    assert_eq!(f.text(), "one two three ");
    // A paste replaces the selection.
    f.select_all();
    f.paste("new");
    assert_eq!((f.text(), f.caret(), f.selection()), ("new", 3, None));
    f.paste("");
    assert_eq!(f.text(), "new");
}

#[test]
fn text_field_ime_preedit_and_commit() {
    let mut f = field_with("ab");
    f.left(false);
    assert!(f.set_preedit("ni"));
    assert!(!f.set_preedit("ni"), "unchanged");
    assert_eq!(f.preedit(), "ni");
    assert_eq!(
        (f.text(), f.caret()),
        ("ab", 1),
        "composing changes no text"
    );

    // Drawn at the caret, underlined, the caret after it, the rest of the text pushed right.
    let view = f.view(20);
    let drawn: Vec<(&str, usize, usize)> = view
        .cells
        .iter()
        .map(|(s, col, width)| (s.as_str(), *col, *width))
        .collect();
    assert_eq!(drawn, [("a", 0, 1), ("n", 1, 1), ("i", 2, 1), ("b", 3, 1)]);
    assert_eq!(view.preedit, Some((1, 3)));
    assert_eq!(view.caret_col, 3);

    // The composition grows into wide characters, then is committed: inserted once.
    assert!(f.set_preedit("你好"));
    let view = f.view(20);
    assert_eq!(view.preedit, Some((1, 5)));
    assert_eq!(view.caret_col, 5);
    assert_eq!(view.cells[1], ("你".to_string(), 1, 2));
    assert_eq!(view.cells[3], ("b".to_string(), 5, 1));
    f.commit("你好");
    assert_eq!((f.text(), f.caret(), f.preedit()), ("a你好b", 7, ""));
    let view = f.view(20);
    assert_eq!(view.preedit, None);
    assert_eq!(view.caret_col, 5);

    // An abandoned composition leaves nothing behind.
    f.set_preedit("x");
    assert!(f.set_preedit(""));
    assert_eq!(f.view(20).preedit, None);
    assert_eq!(f.text(), "a你好b");

    // A commit replaces the selection.
    f.select_all();
    f.set_preedit("zhong");
    f.commit("中");
    assert_eq!((f.text(), f.caret(), f.selection()), ("中", 3, None));
}

#[test]
fn text_field_view_columns_selection_and_scrolling() {
    let mut f = field_with("ab你cd");
    let view = f.view(10);
    let drawn: Vec<(&str, usize, usize)> = view
        .cells
        .iter()
        .map(|(s, col, width)| (s.as_str(), *col, *width))
        .collect();
    assert_eq!(
        drawn,
        [
            ("a", 0, 1),
            ("b", 1, 1),
            ("你", 2, 2),
            ("c", 4, 1),
            ("d", 5, 1)
        ]
    );
    assert_eq!(
        (view.caret_col, view.selection, view.preedit),
        (6, None, None)
    );
    f.left(true);
    f.left(true);
    f.left(true);
    assert_eq!(f.view(10).selection, Some((2, 6)), "columns, not bytes");
    assert_eq!(f.view(10).caret_col, 2);

    // Longer than the field: the view follows the caret.
    let mut f = field_with("0123456789abcdef");
    let view = f.view(10);
    assert_eq!(view.caret_col, 10, "at the right edge");
    assert_eq!(view.cells.first().unwrap(), &("6".to_string(), 0, 1));
    assert_eq!(view.cells.last().unwrap(), &("f".to_string(), 9, 1));
    assert_eq!(view.cells.len(), 10);
    // Moving left inside the visible part does not scroll; passing its left edge does.
    for _ in 0..10 {
        f.left(false);
    }
    let view = f.view(10);
    assert_eq!(view.caret_col, 0);
    assert_eq!(view.cells.first().unwrap(), &("6".to_string(), 0, 1));
    f.left(false);
    let view = f.view(10);
    assert_eq!(view.caret_col, 0);
    assert_eq!(view.cells.first().unwrap(), &("5".to_string(), 0, 1));
    // A selection that runs out of view is clipped to the field.
    f.end(true);
    assert_eq!(f.view(10).selection, Some((0, 10)));
    // Deleting at the end pulls the text back in rather than leaving a gap.
    f.end(false);
    for _ in 0..8 {
        f.backspace();
    }
    let view = f.view(10);
    assert_eq!(view.cells.len(), 8);
    assert_eq!(view.cells.first().unwrap(), &("0".to_string(), 0, 1));
    assert_eq!(view.caret_col, 8);

    // A wide character cut by the left edge is not drawn (never half of it, never outside).
    // Columns: a 0, the wide character 1..3, b 3, … j 11; ten columns show 2..12.
    let mut f = field_with("a你bcdefghij");
    let view = f.view(10);
    assert_eq!(view.cells.first().unwrap(), &("b".to_string(), 1, 1));
    assert_eq!(view.caret_col, 10);
    assert!(view.cells.iter().all(|(_, col, width)| col + width <= 10));
    // And one cut by the right edge: the caret at the start, nine columns show 0..9 of
    // "abcdefgh" + a wide character at 8..10.
    let mut f = field_with("abcdefgh你");
    f.home(false);
    let view = f.view(9);
    assert_eq!(view.cells.len(), 8);
    assert!(view.cells.iter().all(|(_, col, width)| col + width <= 9));
    assert_eq!(f.view(0), Default::default(), "no room at all");
}

#[test]
fn composing_text_stays_visible_when_the_field_is_full() {
    // The terminal's preedit is dropped at the right edge; the field scrolls instead.
    let mut f = field_with("0123456789abcdef");
    f.set_preedit("你好");
    let view = f.view(10);
    assert_eq!(view.preedit, Some((6, 10)));
    assert_eq!(view.caret_col, 10);
    assert_eq!(view.cells.first().unwrap(), &("a".to_string(), 0, 1));
    assert_eq!(
        &view.cells[view.cells.len() - 2..],
        [("你".to_string(), 6, 2), ("好".to_string(), 8, 2)]
    );

    // Longer than the whole field: its end (where the next character appears) is shown.
    let mut f = field_with("a");
    f.set_preedit("你好你好你好");
    let view = f.view(10);
    assert_eq!(view.caret_col, 10);
    assert_eq!(view.preedit, Some((0, 10)));
    assert_eq!(view.cells.len(), 5);
    assert!(view.cells.iter().all(|(_, col, width)| col + width <= 10));
    assert_eq!(view.cells.last().unwrap(), &("好".to_string(), 8, 2));

    // Composing in the middle of a long text, caret scrolled to the left edge: the whole
    // composition is shown when it fits.
    let mut f = field_with("0123456789abcdef");
    f.view(10);
    for _ in 0..10 {
        f.left(false);
    }
    f.set_preedit("xyz");
    let view = f.view(10);
    assert_eq!(view.preedit, Some((0, 3)));
    assert_eq!(view.caret_col, 3);
}

#[test]
fn text_field_mouse_click_and_drag() {
    let mut f = field_with("ab你cd");
    f.view(10);
    f.click(1);
    assert_eq!((f.caret(), f.selection()), (1, None));
    // Column 3 is the middle of the wide character (columns 2..4): the nearer edge, never inside.
    f.click(3);
    assert!(f.caret() == 2 || f.caret() == 5);
    f.click(4);
    assert_eq!(f.caret(), 5);
    f.click(99);
    assert_eq!(f.caret(), 7, "past the end");
    // Press at column 1, drag to column 5: "b你c".
    f.click(1);
    assert!(f.drag(5));
    assert!(!f.drag(5), "same place");
    assert_eq!(f.selected_text().as_deref(), Some("b你c"));
    // A click elsewhere drops it.
    f.click(0);
    assert_eq!((f.caret(), f.selection()), (0, None));

    // Scrolled: columns are those of the visible part.
    let mut f = field_with("0123456789abcdef");
    f.view(10);
    f.click(0);
    assert_eq!(f.caret(), 6);
}

// ---------------------------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------------------------

fn numbered(n: usize) -> List {
    List::new((0..n).map(|i| format!("item {i}")).collect())
}

#[test]
fn the_sample_list_has_at_least_thirty_items() {
    let list = List::sample();
    assert!(list.items().len() >= 30);
    assert_eq!((list.selected(), list.top()), (0, 0));
    let mut names = list.items().to_vec();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), list.items().len(), "names are distinct");
}

#[test]
fn list_keyboard_selection_scrolls_it_into_view() {
    let mut list = numbered(30);
    assert!(!list.key(ListKey::Up, 10), "already at the top");
    assert!(list.key(ListKey::Down, 10));
    assert_eq!((list.selected(), list.top()), (1, 0));
    for _ in 0..9 {
        list.key(ListKey::Down, 10);
    }
    assert_eq!(
        (list.selected(), list.top()),
        (10, 1),
        "one row past the view"
    );
    assert!(list.key(ListKey::PageDown, 10));
    assert_eq!((list.selected(), list.top()), (20, 11));
    list.key(ListKey::PageDown, 10);
    assert_eq!((list.selected(), list.top()), (29, 20), "stops at the last");
    assert!(!list.key(ListKey::Down, 10));
    assert!(list.key(ListKey::Up, 10));
    assert_eq!((list.selected(), list.top()), (28, 20), "still in view");
    assert!(list.key(ListKey::PageUp, 10));
    assert_eq!((list.selected(), list.top()), (18, 18));
    assert!(list.key(ListKey::Home, 10));
    assert_eq!((list.selected(), list.top()), (0, 0));
    assert!(list.key(ListKey::End, 10));
    assert_eq!((list.selected(), list.top()), (29, 20));
    assert!(!list.key(ListKey::Activate, 10), "changes nothing here");

    // No rows visible, or more rows than items: no panic, selection still moves.
    assert!(list.key(ListKey::Up, 0));
    assert_eq!(list.selected(), 28);
    list.key(ListKey::Home, 100);
    assert_eq!((list.selected(), list.top()), (0, 0));
    let mut empty = List::new(Vec::new());
    assert!(!empty.key(ListKey::Down, 10));
    assert!(!empty.key(ListKey::End, 10));
    assert!(!empty.click_row(0));
}

#[test]
fn list_mouse_selection_wheel_and_new_items() {
    let mut list = numbered(30);
    assert!(list.click_row(3));
    assert_eq!(list.selected(), 3);
    // The wheel moves the view, not the selection, and stops at both ends.
    assert!(list.scroll(5, 10));
    assert_eq!((list.selected(), list.top()), (3, 5));
    assert!(list.click_row(0), "rows are those on screen");
    assert_eq!(list.selected(), 5);
    assert!(list.scroll(100, 10));
    assert_eq!(list.top(), 20);
    assert!(!list.scroll(1, 10));
    assert!(list.click_row(9));
    assert_eq!(list.selected(), 29);
    assert!(list.scroll(-100, 10));
    assert_eq!(list.top(), 0);
    assert!(!list.scroll(-1, 10));

    // A click below the last item selects nothing.
    let mut short = numbered(3);
    assert!(!short.click_row(3));
    assert_eq!(short.selected(), 0);
    assert!(!short.scroll(1, 10), "everything is in view");

    // A new item goes to the end, is selected and in view.
    list.push("new".to_string(), 10);
    assert_eq!(list.items().len(), 31);
    assert_eq!(list.items()[30], "new");
    assert_eq!((list.selected(), list.top()), (30, 21));

    // Fewer rows after a resize: the selection is brought back into view.
    assert!(list.fit(4));
    assert_eq!(list.top(), 27);
    assert!(!list.fit(4));
}

// ---------------------------------------------------------------------------------------------
// Context menu
// ---------------------------------------------------------------------------------------------

#[test]
fn menu_hit_testing() {
    // Cell 10x20: rows of 30 px, 9 columns wide, a 1 px border all round → 92x92.
    let menu = Menu::open((500.0, 300.0), (1280.0, 800.0), (10.0, 20.0));
    assert_eq!(
        menu.rect(),
        Rect {
            x: 500.0,
            y: 300.0,
            w: 92.0,
            h: 92.0
        }
    );
    assert_eq!(ITEMS, [MenuItem::Copy, MenuItem::Paste, MenuItem::Clear]);
    assert_eq!(ITEMS.map(MenuItem::label), ["Copy", "Paste", "Clear"]);
    for (index, item) in ITEMS.into_iter().enumerate() {
        let rect = menu.item_rect(index);
        assert_eq!((rect.x, rect.w, rect.h), (501.0, 90.0, 30.0));
        assert_eq!(rect.y, 301.0 + 30.0 * index as f32);
        // Corners and centre of the row; the pixel past its end belongs to the next.
        assert_eq!(menu.hit(rect.x, rect.y), Some(item));
        assert_eq!(menu.hit(rect.x + 89.5, rect.y + 29.5), Some(item));
        assert_eq!(menu.hit(rect.x + 45.0, rect.y + 15.0), Some(item));
        assert_ne!(menu.hit(rect.x + 45.0, rect.y + 30.0), Some(item));
    }
    // The border and everything outside hit nothing.
    for (x, y) in [
        (500.5, 320.0),
        (591.5, 320.0),
        (540.0, 300.5),
        (540.0, 391.5),
        (499.0, 320.0),
        (592.0, 320.0),
        (540.0, 299.0),
        (540.0, 392.0),
        (-1.0, -1.0),
        (f32::NAN, 320.0),
    ] {
        assert_eq!(menu.hit(x, y), None, "({x}, {y})");
    }
}

#[test]
fn menu_stays_inside_the_window() {
    let window = (1280.0, 800.0);
    let cell = (10.0, 20.0);
    // Too close to the right / bottom edge: it opens to the other side of the pointer.
    let menu = Menu::open((1250.0, 790.0), window, cell);
    assert_eq!((menu.rect().x, menu.rect().y), (1158.0, 698.0));
    let menu = Menu::open((1250.0, 100.0), window, cell);
    assert_eq!((menu.rect().x, menu.rect().y), (1158.0, 100.0));
    // A fractional pointer position: whole pixels.
    let menu = Menu::open((300.4, 200.6), window, cell);
    assert_eq!((menu.rect().x, menu.rect().y), (300.0, 201.0));
    // A window smaller than the menu: pinned to the top-left corner.
    let menu = Menu::open((40.0, 40.0), (50.0, 50.0), cell);
    assert_eq!((menu.rect().x, menu.rect().y), (0.0, 0.0));
    // A fractional-scale cell: still whole pixels.
    let menu = Menu::open((10.0, 10.0), window, (11.0, 23.0));
    let rect = menu.rect();
    for v in [rect.x, rect.y, rect.w, rect.h, menu.item_rect(1).y] {
        assert_eq!(v, v.round());
    }
}

#[test]
fn menu_hover_and_keys() {
    let mut menu = Menu::open((500.0, 300.0), (1280.0, 800.0), (10.0, 20.0));
    assert_eq!(menu.hovered(), None);
    assert_eq!(menu.key(MenuKey::Activate), Outcome::Stay, "nothing chosen");
    assert!(menu.hover(540.0, 340.0));
    assert_eq!(menu.hovered(), Some(1));
    assert!(!menu.hover(541.0, 341.0), "same row");
    assert!(menu.hover(10.0, 10.0));
    assert_eq!(menu.hovered(), None);

    assert_eq!(menu.key(MenuKey::Down), Outcome::Stay);
    assert_eq!(menu.hovered(), Some(0));
    menu.key(MenuKey::Down);
    menu.key(MenuKey::Down);
    assert_eq!(menu.hovered(), Some(2));
    menu.key(MenuKey::Down);
    assert_eq!(menu.hovered(), Some(0), "wraps");
    menu.key(MenuKey::Up);
    assert_eq!(menu.hovered(), Some(2));
    assert_eq!(menu.key(MenuKey::Activate), Outcome::Pick(MenuItem::Clear));
    assert_eq!(menu.key(MenuKey::Close), Outcome::Close);

    let mut menu = Menu::open((500.0, 300.0), (1280.0, 800.0), (10.0, 20.0));
    menu.key(MenuKey::Up);
    assert_eq!(menu.hovered(), Some(2), "Up from nothing is the last row");
}

#[test]
fn clear_is_the_form_feed_key() {
    // "Clear" asks the program to redraw on an empty screen, as Ctrl+L on the keyboard does.
    let key = clear_key();
    let typed = key_action(&ch("l"), None, CTRL, true, false);
    assert_eq!(Action::Send(key), typed);
}

// ---------------------------------------------------------------------------------------------
// Zoom, theme, layout
// ---------------------------------------------------------------------------------------------

#[test]
fn zoom_steps() {
    assert_eq!(zoom_step(14.0, ZoomDir::In), 15.0);
    assert_eq!(zoom_step(14.0, ZoomDir::Out), 13.0);
    // In then out is where it started.
    let mut size = 14.0;
    for _ in 0..5 {
        size = zoom_step(size, ZoomDir::In);
    }
    for _ in 0..5 {
        size = zoom_step(size, ZoomDir::Out);
    }
    assert_eq!(size, 14.0);
    // Stops at both ends.
    for _ in 0..100 {
        size = zoom_step(size, ZoomDir::In);
    }
    assert_eq!(size, ZOOM_MAX);
    for _ in 0..100 {
        size = zoom_step(size, ZoomDir::Out);
    }
    assert_eq!(size, ZOOM_MIN);
    const { assert!(ZOOM_MIN >= 4.0 && ZOOM_MAX <= 96.0 && ZOOM_MIN < 14.0 && ZOOM_MAX > 14.0) };
    // An odd starting size lands on the next whole size; nonsense becomes the default step.
    assert_eq!(zoom_step(14.5, ZoomDir::In), 15.0);
    assert_eq!(zoom_step(14.5, ZoomDir::Out), 14.0);
    assert_eq!(zoom_step(f32::NAN, ZoomDir::In), 15.0);
    assert_eq!(zoom_step(-3.0, ZoomDir::Out), ZOOM_MIN);
    assert_eq!(zoom_step(1e9, ZoomDir::In), ZOOM_MAX);
}

#[test]
fn theme_tokens() {
    let theme = UiTheme::default();
    assert_eq!(theme.sidebar_width, 240.0, "logical pixels");
    assert_eq!(theme.font.size, 14.0);
    assert_eq!(theme.terminal, s2_wgpu::gfx::grid::Theme::default());
    // The states a user has to tell apart have different colours.
    assert_ne!(theme.sidebar_bg, theme.terminal.bg);
    assert_ne!(theme.item_selected_bg, theme.sidebar_bg);
    assert_ne!(theme.focus_ring, theme.separator);
    assert_ne!(theme.field_bg, theme.sidebar_bg);
    assert_ne!(theme.menu_hover_bg, theme.menu_bg);
    assert!(theme.field_selection[3] < 255, "drawn over the text");
}

fn rects(l: &Layout) -> Vec<(&'static str, Rect)> {
    vec![
        ("sidebar", l.sidebar),
        ("separator", l.separator),
        ("list", l.list),
        ("form_rule", l.form_rule),
        ("form_label", l.form_label),
        ("field", l.field),
        ("field_text", l.field_text),
        ("create", l.create),
        ("cancel", l.cancel),
        ("terminal", l.terminal),
    ]
}

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

#[test]
fn layout_at_a_fractional_scale() {
    // 125 %: a 240 px sidebar is 300 physical pixels; an 8 px padding 10.
    let m = metrics(10.0, 21.0);
    let l = layout((1600, 1000), 1.25, &m, &UiTheme::default());
    assert_eq!(l.window, (1600.0, 1000.0));
    assert_eq!(l.cell, (10.0, 21.0));
    assert_eq!(
        l.sidebar,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 1000.0
        }
    );
    assert_eq!((l.separator.x, l.separator.w), (300.0, 1.0));
    // The terminal starts right of the separator and gets every cell that fits in the rest.
    assert_eq!(
        l.terminal,
        Rect {
            x: 301.0,
            y: 0.0,
            w: 1299.0,
            h: 1000.0
        }
    );
    assert_eq!(l.grid, (129, 47));
    assert_eq!(l.grid, m.grid_for(1299, 1000));
    assert_eq!(l.visible_rows, 33);
    assert_eq!(l.label_cols, 28);
    assert_eq!(l.field_cols, 27);
    assert_eq!(l.field_text.w, 270.0);

    // Glyphs are copied texel for texel: nothing may sit between pixels.
    for (name, r) in rects(&l) {
        for v in [r.x, r.y, r.w, r.h] {
            assert_eq!(v, v.round(), "{name}: {r:?}");
        }
    }
    assert_eq!(l.row_h, l.row_h.round());
    assert_eq!(l.list_row(7).y, l.list.y + 7.0 * l.row_h);

    // Everything of the sidebar is inside it; the pieces do not overlap.
    for (name, r) in rects(&l) {
        if !["sidebar", "separator", "terminal"].contains(&name) {
            assert!(inside(r, l.sidebar), "{name}: {r:?}");
        }
    }
    assert!(inside(l.field_text, l.field));
    assert!(l.field_text.h == 21.0 && l.form_label.h == 21.0);
    assert!(l.create.h >= 21.0 && l.cancel.h >= 21.0);
    let pieces = [
        l.list,
        l.form_rule,
        l.form_label,
        l.field,
        l.create,
        l.cancel,
    ];
    for (i, a) in pieces.iter().enumerate() {
        for b in &pieces[i + 1..] {
            assert!(!overlap(*a, *b), "{a:?} / {b:?}");
        }
    }
    assert!(l.list.bottom() <= l.form_rule.y && l.form_label.bottom() <= l.field.y);
    assert!(l.field.bottom() <= l.create.y && l.create.y == l.cancel.y);
    assert!(l.visible_rows as f32 * l.row_h <= l.list.h);
    assert!(l.label_cols as f32 * 10.0 <= l.sidebar.w);
    assert!(l.row_h >= 21.0);
}

#[test]
fn layout_follows_scale_zoom_and_window_size() {
    let theme = UiTheme::default();
    // Scale 1: 240 px. Scale 1.5: 360. Scale 1.1: 264.
    for (scale, width) in [(1.0, 240.0), (1.5, 360.0), (1.1, 264.0), (2.0, 480.0)] {
        let l = layout((1920, 1080), scale, &metrics(9.0, 19.0), &theme);
        assert_eq!(l.sidebar.w, width, "scale {scale}");
        assert_eq!(l.terminal.x, l.separator.right());
        assert_eq!(l.terminal.right(), 1920.0);
        assert!(l.separator.w >= 1.0);
    }
    // Scales where logical sizes do not land on whole pixels (8 px × 1.1 = 8.8): every
    // rectangle is on whole pixels all the same.
    for scale in [1.1, 1.3, 1.75, 2.25] {
        let l = layout((1917, 1083), scale, &metrics(11.0, 23.0), &theme);
        for (name, r) in rects(&l) {
            for v in [r.x, r.y, r.w, r.h] {
                assert_eq!(v, v.round(), "scale {scale} {name}: {r:?}");
            }
        }
        assert_eq!(l.row_h, l.row_h.round());
        assert!(inside(l.field_text, l.field) && inside(l.field, l.sidebar));
    }
    // A bigger font (zoom): fewer cells and rows, same sidebar.
    let small = layout((1280, 800), 1.0, &metrics(8.0, 17.0), &theme);
    let large = layout((1280, 800), 1.0, &metrics(16.0, 34.0), &theme);
    assert_eq!(small.sidebar, large.sidebar);
    assert_eq!(small.terminal, large.terminal);
    assert!(large.grid.0 < small.grid.0 && large.grid.1 < small.grid.1);
    assert!(large.visible_rows < small.visible_rows);
    assert!(large.field_cols < small.field_cols);
    assert!(inside(large.create, large.sidebar) && inside(large.cancel, large.sidebar));
    assert!(!overlap(large.create, large.cancel));

    // A narrow window: the sidebar takes at most half, the grid never drops below 1x1.
    let l = layout((300, 200), 1.0, &metrics(10.0, 20.0), &theme);
    assert_eq!(l.sidebar.w, 150.0);
    assert!(l.grid.0 >= 1 && l.grid.1 >= 1);
    // Degenerate sizes do not panic or produce negative sizes.
    for window in [(1, 1), (0, 0), (5, 2000), (4000, 3)] {
        let l = layout(window, 1.25, &metrics(10.0, 21.0), &theme);
        for (name, r) in rects(&l) {
            assert!(r.w >= 0.0 && r.h >= 0.0, "{window:?} {name}: {r:?}");
        }
        assert!(l.grid.0 >= 1 && l.grid.1 >= 1);
    }
}

#[test]
fn layout_hit_testing() {
    let l = plain_layout();
    assert!(
        Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0
        }
        .contains(1.0, 2.0)
    );
    assert!(
        !Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0
        }
        .contains(4.0, 2.0)
    );
    assert!(
        !Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0
        }
        .contains(1.0, 6.0)
    );
    assert!(
        !Rect {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0
        }
        .contains(f32::NAN, 3.0)
    );

    for row in [0, 1, l.visible_rows - 1] {
        let (x, y) = centre(l.list_row(row));
        assert_eq!(l.hit(x, y), Hit::ListRow(row));
    }
    assert_eq!(l.hit(0.0, l.list.y), Hit::ListRow(0));
    assert_eq!(l.hit(0.0, l.list.y - 1.0), Hit::Sidebar);
    let (x, y) = centre(l.create);
    assert_eq!(l.hit(x, y), Hit::Create);
    let (x, y) = centre(l.cancel);
    assert_eq!(l.hit(x, y), Hit::Cancel);
    let (x, y) = centre(l.form_label);
    assert_eq!(l.hit(x, y), Hit::Sidebar);
    assert_eq!(l.hit(l.separator.x, 100.0), Hit::Sidebar);
    assert_eq!(l.hit(l.terminal.x, 0.0), Hit::Terminal);
    assert_eq!(l.hit(1279.0, 799.0), Hit::Terminal);
    for (x, y) in [(-1.0, 5.0), (5.0, -1.0), (1280.0, 5.0), (5.0, 800.0)] {
        assert_eq!(l.hit(x, y), Hit::Outside);
    }

    // In the field the column is the caret position nearest to the pointer.
    let t = l.field_text;
    assert_eq!(l.hit(t.x + 1.0, t.y + 5.0), Hit::Field { col: 0 });
    assert_eq!(l.hit(t.x + 6.0, t.y + 5.0), Hit::Field { col: 1 });
    assert_eq!(l.hit(t.x + 34.0, t.y + 5.0), Hit::Field { col: 3 });
    assert_eq!(
        l.hit(l.field.x, l.field.y),
        Hit::Field { col: 0 },
        "the padding"
    );
    assert_eq!(
        l.hit(l.field.right() - 1.0, l.field.y),
        Hit::Field { col: l.field_cols }
    );
}

// ---------------------------------------------------------------------------------------------
// The UI state as a whole
// ---------------------------------------------------------------------------------------------

fn type_into(ui: &mut Ui, l: &Layout, text: &str) {
    for c in text.chars() {
        let s = c.to_string();
        let to = press(ui.focus, ui.menu.is_some(), &ch(&s), Some(&s), NONE);
        ui.key(to, l);
    }
}

fn key(ui: &mut Ui, l: &Layout, key: NamedKey, mods: Mods) -> Effect {
    let to = press(ui.focus, ui.menu.is_some(), &named(key), None, mods);
    ui.key(to, l)
}

#[test]
fn the_app_starts_with_the_terminal_focused() {
    // A bench run types at once: those keys must be terminal keys.
    let ui = Ui::new();
    assert_eq!(ui.focus, Focus::Terminal);
    assert!(ui.menu.is_none());
    assert!(ui.list.items().len() >= 30);
    assert_eq!(ui.field.text(), "");
}

#[test]
fn traversal_walks_the_ring_and_list_keys_move_the_selection() {
    let l = plain_layout();
    let mut ui = Ui::new();
    assert_eq!(key(&mut ui, &l, NamedKey::Tab, CTRL), Effect::Redraw);
    assert_eq!(ui.focus, Focus::List);
    assert_eq!(key(&mut ui, &l, NamedKey::ArrowDown, NONE), Effect::Redraw);
    type_into(&mut ui, &l, "jjk");
    assert_eq!(ui.list.selected(), 2);
    assert_eq!(key(&mut ui, &l, NamedKey::Home, NONE), Effect::Redraw);
    assert_eq!(key(&mut ui, &l, NamedKey::Home, NONE), Effect::None);
    assert_eq!(key(&mut ui, &l, NamedKey::Tab, NONE), Effect::Redraw);
    assert_eq!(ui.focus, Focus::Field);
    assert_eq!(key(&mut ui, &l, NamedKey::Tab, NONE), Effect::Redraw);
    assert_eq!(ui.focus, Focus::Terminal);
    // Plain Tab now belongs to the terminal: the UI does nothing with it.
    let to = press(ui.focus, false, &named(NamedKey::Tab), Some("\t"), NONE);
    assert!(matches!(to, Route::Terminal(Action::Send(_))));
    assert_eq!(ui.key(to, &l), Effect::None);
    assert_eq!(ui.focus, Focus::Terminal);
    key(&mut ui, &l, NamedKey::Tab, CTRL | SHIFT);
    assert_eq!(ui.focus, Focus::Field);
    key(&mut ui, &l, NamedKey::Tab, SHIFT);
    assert_eq!(ui.focus, Focus::List);
    // Enter on an item goes to the terminal.
    assert_eq!(key(&mut ui, &l, NamedKey::Enter, NONE), Effect::Redraw);
    assert_eq!(ui.focus, Focus::Terminal);
    // Zoom is passed on to the app.
    let to = press(ui.focus, false, &ch("="), None, CTRL);
    assert_eq!(ui.key(to, &l), Effect::Zoom(ZoomDir::In));
}

#[test]
fn the_form_creates_a_session_or_cancels() {
    let l = plain_layout();
    let mut ui = Ui::new();
    let before = ui.list.items().len();
    ui.set_focus(Focus::Field);

    // Nothing typed: Create does nothing.
    assert_eq!(key(&mut ui, &l, NamedKey::Enter, NONE), Effect::None);
    assert_eq!((ui.list.items().len(), ui.focus), (before, Focus::Field));
    type_into(&mut ui, &l, "   ");
    key(&mut ui, &l, NamedKey::Enter, NONE);
    assert_eq!(ui.list.items().len(), before, "blanks are not a name");
    ui.field.clear();

    // Typed text, edited, then Enter: a new item with that text, selected and in view.
    type_into(&mut ui, &l, "buildx");
    assert_eq!(key(&mut ui, &l, NamedKey::Backspace, NONE), Effect::Redraw);
    type_into(&mut ui, &l, " 你好");
    assert_eq!(ui.field.text(), "build 你好");
    assert_eq!(key(&mut ui, &l, NamedKey::Enter, NONE), Effect::Redraw);
    assert_eq!(ui.list.items().len(), before + 1);
    assert_eq!(ui.list.items()[before], "build 你好");
    assert_eq!(ui.list.selected(), before);
    assert!(
        ui.list.top() + l.visible_rows > before,
        "scrolled into view"
    );
    assert_eq!(ui.field.text(), "");
    assert_eq!(ui.focus, Focus::List);

    // The Create button does the same; surrounding blanks are dropped.
    ui.set_focus(Focus::Field);
    type_into(&mut ui, &l, " logs ");
    let (x, y) = centre(l.create);
    assert_eq!(ui.press(x, y, &l), Effect::Redraw);
    assert_eq!(ui.list.items().last().unwrap(), "logs");
    assert_eq!(ui.list.items().len(), before + 2);

    // Escape and the Cancel button throw the text away.
    ui.set_focus(Focus::Field);
    type_into(&mut ui, &l, "oops");
    assert_eq!(key(&mut ui, &l, NamedKey::Escape, NONE), Effect::Redraw);
    assert_eq!((ui.field.text(), ui.focus), ("", Focus::List));
    ui.set_focus(Focus::Field);
    type_into(&mut ui, &l, "oops");
    let (x, y) = centre(l.cancel);
    assert_eq!(ui.press(x, y, &l), Effect::Redraw);
    assert_eq!((ui.field.text(), ui.focus), ("", Focus::List));
    assert_eq!(ui.list.items().len(), before + 2);
}

#[test]
fn field_clipboard_keys_and_ime() {
    let l = plain_layout();
    let mut ui = Ui::new();
    ui.set_focus(Focus::Field);
    type_into(&mut ui, &l, "abc");
    let ctrl = |ui: &mut Ui, c: &str| {
        let to = press(ui.focus, false, &ch(c), None, CTRL);
        ui.key(to, &l)
    };
    assert_eq!(ctrl(&mut ui, "c"), Effect::None, "nothing selected");
    assert_eq!(ctrl(&mut ui, "a"), Effect::Redraw);
    assert_eq!(ctrl(&mut ui, "c"), Effect::SetClipboard("abc".to_string()));
    assert_eq!(ui.field.text(), "abc");
    assert_eq!(ctrl(&mut ui, "x"), Effect::SetClipboard("abc".to_string()));
    assert_eq!(ui.field.text(), "");
    // Paste: the app reads the clipboard and hands the text back.
    assert_eq!(ctrl(&mut ui, "v"), Effect::PasteField);
    assert_eq!(ui.paste("wide 你好\n"), Effect::Redraw);
    assert_eq!(ui.field.text(), "wide 你好 ");

    // IME: preedit, then one commit.
    assert_eq!(
        ui.ime(&Ime::Preedit("ni".to_string(), Some((2, 2)))),
        Effect::Redraw
    );
    assert_eq!(ui.field.preedit(), "ni");
    assert_eq!(ui.ime(&Ime::Preedit(String::new(), None)), Effect::Redraw);
    assert_eq!(ui.ime(&Ime::Commit("你".to_string())), Effect::Redraw);
    assert_eq!((ui.field.text(), ui.field.preedit()), ("wide 你好 你", ""));
    assert_eq!(ui.ime(&Ime::Enabled), Effect::None);

    // Leaving the field ends a composition; elsewhere the IME and a paste do nothing.
    ui.ime(&Ime::Preedit("x".to_string(), None));
    assert!(ui.set_focus(Focus::List));
    assert!(!ui.set_focus(Focus::List));
    assert_eq!(ui.field.preedit(), "");
    assert_eq!(ui.ime(&Ime::Commit("z".to_string())), Effect::None);
    assert_eq!(ui.paste("z"), Effect::None);
    assert_eq!(ui.field.text(), "wide 你好 你");
}

#[test]
fn mouse_selects_items_places_the_caret_and_moves_the_focus() {
    let l = plain_layout();
    let mut ui = Ui::new();

    // A list row: selected, list focused.
    let (x, y) = centre(l.list_row(4));
    assert_eq!(ui.press(x, y, &l), Effect::Redraw);
    ui.release();
    assert_eq!((ui.focus, ui.list.selected()), (Focus::List, 4));
    // The wheel over the sidebar scrolls the list; over the terminal it is not ours.
    assert_eq!(ui.wheel(x, y, 3, &l), Some(Effect::Redraw));
    assert_eq!(ui.list.top(), 3);
    assert_eq!(ui.wheel(x, y, -100, &l), Some(Effect::Redraw));
    assert_eq!(ui.wheel(x, y, -1, &l), Some(Effect::None));
    assert_eq!(ui.wheel(l.terminal.x + 50.0, 50.0, 3, &l), None);
    assert_eq!(ui.list.top(), 0);

    // The field: focused, caret where the click was; a drag selects.
    ui.field.insert("hello world");
    let t = l.field_text;
    assert_eq!(ui.press(t.x + 20.0, t.y + 5.0, &l), Effect::Redraw);
    assert_eq!((ui.focus, ui.field.caret()), (Focus::Field, 2));
    assert_eq!(ui.pointer(t.x + 50.0, t.y + 5.0, &l), Effect::Redraw);
    assert_eq!(ui.field.selected_text().as_deref(), Some("llo"));
    // The drag goes on outside the field, and ends with the button.
    assert_eq!(ui.pointer(-50.0, 0.0, &l), Effect::Redraw);
    assert_eq!(ui.field.selected_text().as_deref(), Some("he"));
    ui.release();
    assert_eq!(ui.pointer(t.x + 80.0, t.y + 5.0, &l), Effect::None);
    assert_eq!(ui.field.selected_text().as_deref(), Some("he"));

    // The terminal: focused, and the press is the terminal's (a selection starts there).
    assert_eq!(ui.press(l.terminal.x + 100.0, 100.0, &l), Effect::TermPress);
    assert_eq!(ui.focus, Focus::Terminal);
    // Empty parts of the sidebar do nothing.
    let (x, y) = centre(l.form_label);
    assert_eq!(ui.press(x, y, &l), Effect::None);
    assert_eq!(ui.focus, Focus::Terminal);
}

#[test]
fn context_menu_opens_on_the_terminal_only_and_its_items_act_once() {
    let l = plain_layout();
    let mut ui = Ui::new();
    ui.set_focus(Focus::List);

    // Right click on the sidebar: nothing.
    let (x, y) = centre(l.list_row(2));
    assert_eq!(ui.context(x, y, &l), Effect::None);
    assert!(ui.menu.is_none());

    // On the terminal: opens at the pointer, terminal focused.
    let at = (l.terminal.x + 200.0, 300.0);
    assert_eq!(ui.context(at.0, at.1, &l), Effect::Redraw);
    let menu = ui.menu.expect("menu open");
    assert_eq!((menu.rect().x, menu.rect().y), at);
    assert_eq!(ui.focus, Focus::Terminal);

    // Hover highlights; a click on an item closes the menu and reports the item.
    let (x, y) = centre(menu.item_rect(1));
    assert_eq!(ui.pointer(x, y, &l), Effect::Redraw);
    assert_eq!(ui.pointer(x + 1.0, y, &l), Effect::None);
    assert_eq!(ui.menu.unwrap().hovered(), Some(1));
    assert_eq!(ui.press(x, y, &l), Effect::TermPaste);
    assert!(ui.menu.is_none());

    for (index, effect) in [
        (0, Effect::TermCopy),
        (1, Effect::TermPaste),
        (2, Effect::TermClear),
    ] {
        ui.context(at.0, at.1, &l);
        let (x, y) = centre(ui.menu.unwrap().item_rect(index));
        assert_eq!(ui.press(x, y, &l), effect);
        assert!(ui.menu.is_none());
    }

    // A click outside closes it and does nothing else: no selection starts under it, no list
    // item is selected through it.
    ui.context(at.0, at.1, &l);
    assert_eq!(ui.press(l.terminal.x + 5.0, 5.0, &l), Effect::Redraw);
    assert!(ui.menu.is_none());
    ui.context(at.0, at.1, &l);
    let selected = ui.list.selected();
    let (x, y) = centre(l.list_row(6));
    assert_eq!(ui.press(x, y, &l), Effect::Redraw);
    assert_eq!((ui.list.selected(), ui.focus), (selected, Focus::Terminal));

    // Keyboard: Down, Down, Enter picks Paste; Escape or any other key closes.
    ui.context(at.0, at.1, &l);
    assert_eq!(key(&mut ui, &l, NamedKey::ArrowDown, NONE), Effect::Redraw);
    key(&mut ui, &l, NamedKey::ArrowDown, NONE);
    assert_eq!(key(&mut ui, &l, NamedKey::Enter, NONE), Effect::TermPaste);
    assert!(ui.menu.is_none());
    ui.context(at.0, at.1, &l);
    assert_eq!(key(&mut ui, &l, NamedKey::Escape, NONE), Effect::Redraw);
    assert!(ui.menu.is_none());
    ui.context(at.0, at.1, &l);
    type_into(&mut ui, &l, "x");
    assert!(ui.menu.is_none());

    // A second right click moves the menu; one on the sidebar closes it.
    ui.context(at.0, at.1, &l);
    ui.context(at.0 + 50.0, at.1, &l);
    assert_eq!(ui.menu.unwrap().rect().x, at.0 + 50.0);
    let (x, y) = centre(l.list_row(2));
    assert_eq!(ui.context(x, y, &l), Effect::Redraw);
    assert!(ui.menu.is_none());
}

#[test]
fn the_ime_candidate_window_follows_the_focus() {
    let l = plain_layout();
    let mut ui = Ui::new();
    // Terminal: the cursor cell, offset by the sidebar. No cursor (scrolled away): nowhere.
    assert_eq!(
        ui.ime_anchor(&l, Some((3, 2))),
        Some((l.terminal.x + 30.0, 40.0))
    );
    assert_eq!(ui.ime_anchor(&l, None), None);
    // Field: the caret, after what is being composed; the terminal's cursor does not matter.
    ui.set_focus(Focus::Field);
    ui.field.insert("ab");
    assert_eq!(
        ui.ime_anchor(&l, Some((3, 2))),
        Some((l.field_text.x + 20.0, l.field_text.y))
    );
    ui.field.set_preedit("你");
    assert_eq!(
        ui.ime_anchor(&l, None),
        Some((l.field_text.x + 40.0, l.field_text.y))
    );
    // List: no text input.
    ui.set_focus(Focus::List);
    assert_eq!(ui.ime_anchor(&l, Some((3, 2))), None);
}
