//! The context menu of the terminal: Copy / Paste / Clear. Geometry and hit-testing in
//! physical pixels; opening it and acting on a choice is `Ui`'s and the app's part.

use s2_core::{Key, KeyEvent, Mods};

use super::focus::MenuKey;
use super::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    /// Copies the terminal's selection (as Ctrl+Shift+C).
    Copy,
    /// Pastes the clipboard into the terminal (as Ctrl+Shift+V).
    Paste,
    /// Sends `clear_key()` to the program.
    Clear,
}

pub const ITEMS: [MenuItem; 3] = [MenuItem::Copy, MenuItem::Paste, MenuItem::Clear];

impl MenuItem {
    pub fn label(self) -> &'static str {
        match self {
            MenuItem::Copy => "Copy",
            MenuItem::Paste => "Paste",
            MenuItem::Clear => "Clear",
        }
    }
}

/// Columns of a row: the longest label and two columns of space on either side.
const ROW_COLS: f32 = 9.0;
/// The border around the rows, pixels.
pub const BORDER: f32 = 1.0;

/// What a key did to the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    Pick(MenuItem),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Menu {
    /// Border included.
    rect: Rect,
    row_h: f32,
    /// The row under the pointer or chosen with the arrow keys.
    hovered: Option<usize>,
}

impl Menu {
    /// A menu with its top-left corner at the pointer `at` — or, where the window ends too
    /// soon, to the left of / above the pointer; never outside the window's top-left corner.
    /// `cell` is the cell box (text size).
    pub fn open(at: (f32, f32), window: (f32, f32), cell: (f32, f32)) -> Menu {
        let row_h = cell.1.round() + 2.0 * (cell.1 * 0.25).round();
        let w = (ROW_COLS * cell.0).round() + 2.0 * BORDER;
        let h = ITEMS.len() as f32 * row_h + 2.0 * BORDER;
        let place = |at: f32, size: f32, limit: f32| {
            let at = if at.is_finite() { at.round() } else { 0.0 };
            let at = if at + size > limit { at - size } else { at };
            at.max(0.0)
        };
        Menu {
            rect: Rect {
                x: place(at.0, w, window.0),
                y: place(at.1, h, window.1),
                w,
                h,
            },
            row_h,
            hovered: None,
        }
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// The rectangle of row `index`, inside the border.
    pub fn item_rect(&self, index: usize) -> Rect {
        Rect {
            x: self.rect.x + BORDER,
            y: self.rect.y + BORDER + index as f32 * self.row_h,
            w: self.rect.w - 2.0 * BORDER,
            h: self.row_h,
        }
    }

    pub fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    fn index_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..ITEMS.len()).find(|&index| self.item_rect(index).contains(x, y))
    }

    /// The item under a window pixel; `None` on the border and outside.
    pub fn hit(&self, x: f32, y: f32) -> Option<MenuItem> {
        self.index_at(x, y).map(|index| ITEMS[index])
    }

    /// The pointer moved. Returns whether the highlighted row changed.
    pub fn hover(&mut self, x: f32, y: f32) -> bool {
        let hovered = self.index_at(x, y);
        let changed = hovered != self.hovered;
        self.hovered = hovered;
        changed
    }

    pub fn key(&mut self, key: MenuKey) -> Outcome {
        let n = ITEMS.len();
        match key {
            MenuKey::Down => self.hovered = Some(self.hovered.map_or(0, |i| (i + 1) % n)),
            MenuKey::Up => self.hovered = Some(self.hovered.map_or(n - 1, |i| (i + n - 1) % n)),
            MenuKey::Activate => {
                return self
                    .hovered
                    .map_or(Outcome::Stay, |i| Outcome::Pick(ITEMS[i]));
            }
            MenuKey::Close => return Outcome::Close,
        }
        Outcome::Stay
    }
}

/// What "Clear" sends: Ctrl+L (form feed), the key a shell or a full-screen program redraws
/// on. `s2-core` has no message that clears the emulator's screen or scrollback, so the menu
/// item is the keyboard's Ctrl+L, sent through the same key path (id, `key` and `write` lines).
pub fn clear_key() -> KeyEvent {
    KeyEvent {
        key: Key::Char('l'),
        mods: Mods {
            ctrl: true,
            ..Mods::default()
        },
        text: None,
        repeat: false,
        pressed: true,
    }
}
