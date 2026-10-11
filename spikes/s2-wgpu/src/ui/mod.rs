//! The non-terminal UI of the interaction slice: a sidebar with a list of sessions and a
//! "New session" form (one text field, Create / Cancel), a context menu on the terminal,
//! keyboard focus that moves between list, field and terminal, theme tokens and font zoom.
//!
//! - `theme`     — tokens (colours, sizes, font) and the zoom steps.
//! - `focus`     — the focus ring and `route`: which key goes where.
//! - `layout`    — rectangles in physical pixels, hit-testing, the terminal's origin and grid.
//! - `list`      — the sidebar list.
//! - `textfield` — the one-line editor: caret, selection, IME preedit, scrolling.
//! - `menu`      — the context menu's geometry and hit-testing.
//! - `draw`      — all of it as quads for `gfx::render`.
//! - here        — `Ui`, the state as a whole, and `key_event` / `ime_event`, the two
//!   functions the window calls for every key and IME event.
//!
//! Nothing in here needs a window or a GPU. `Ui` never touches the clipboard, the session or
//! the atlas itself: it answers with an `Effect` and `app` does that part.

pub mod draw;
pub mod focus;
pub mod layout;
pub mod list;
pub mod menu;
pub mod textfield;
pub mod theme;

use winit::event::Ime;
use winit::keyboard::{Key as WinitKey, ModifiersState};

use crate::input::Action;
use crate::session::Session;
use focus::{FieldKey, Focus, ListKey, Route};
use layout::{Hit, Layout};
use list::List;
use menu::{Menu, MenuItem, Outcome};
use textfield::TextField;
use theme::{UiTheme, ZoomDir};

/// What the app has to do after an event was given to the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// The UI looks different now.
    Redraw,
    /// Field copy / cut: put this text on the clipboard (and redraw).
    SetClipboard(String),
    /// Field paste: read the clipboard and call `Ui::paste`.
    PasteField,
    /// Copy the terminal's selection (Ctrl+Shift+C, or the menu).
    TermCopy,
    /// Paste the clipboard into the terminal (Ctrl+Shift+V, or the menu).
    TermPaste,
    /// The menu's Clear: send `menu::clear_key()` to the terminal.
    TermClear,
    /// A left press on the terminal: a selection may start there.
    TermPress,
    /// Change the font size by one step.
    Zoom(ZoomDir),
}

pub struct Ui {
    pub theme: UiTheme,
    pub focus: Focus,
    pub list: List,
    /// The form's one field: the new session's name.
    pub field: TextField,
    /// The context menu, while it is open.
    pub menu: Option<Menu>,
    /// The left button went down in the field and is still held.
    field_drag: bool,
}

impl Default for Ui {
    fn default() -> Self {
        Ui::new()
    }
}

impl Ui {
    /// The terminal has the focus at start: what is typed first goes to the shell.
    pub fn new() -> Ui {
        Ui {
            theme: UiTheme::default(),
            focus: Focus::Terminal,
            list: List::sample(),
            field: TextField::new(),
            menu: None,
            field_drag: false,
        }
    }

    /// Moves the keyboard focus. A composition in the field ends when the field loses it.
    /// Returns whether the focus changed.
    pub fn set_focus(&mut self, focus: Focus) -> bool {
        if focus == self.focus {
            return false;
        }
        if self.focus == Focus::Field {
            self.field.set_preedit("");
            self.field_drag = false;
        }
        self.focus = focus;
        true
    }

    fn redraw_if(changed: bool) -> Effect {
        if changed {
            Effect::Redraw
        } else {
            Effect::None
        }
    }

    /// A key that `focus::route` did not give to the terminal.
    pub fn key(&mut self, route: Route, layout: &Layout) -> Effect {
        match route {
            Route::Terminal(_) | Route::Ignore => Effect::None,
            Route::Focus(dir) => Self::redraw_if(self.set_focus(self.focus.step(dir))),
            Route::Zoom(dir) => Effect::Zoom(dir),
            Route::List(ListKey::Activate) => Self::redraw_if(self.set_focus(Focus::Terminal)),
            Route::List(key) => Self::redraw_if(self.list.key(key, layout.visible_rows)),
            Route::Field(key) => self.field_key(key, layout),
            Route::Menu(key) => {
                let Some(menu) = &mut self.menu else {
                    return Effect::None;
                };
                match menu.key(key) {
                    Outcome::Stay => Effect::Redraw,
                    Outcome::Close => {
                        self.menu = None;
                        Effect::Redraw
                    }
                    Outcome::Pick(item) => self.pick(item),
                }
            }
        }
    }

    fn field_key(&mut self, key: FieldKey, layout: &Layout) -> Effect {
        let field = &mut self.field;
        match key {
            FieldKey::Insert(text) => field.insert(&text),
            FieldKey::Backspace => return Self::redraw_if(field.backspace()),
            FieldKey::Delete => return Self::redraw_if(field.delete()),
            FieldKey::Left { select } => field.left(select),
            FieldKey::Right { select } => field.right(select),
            FieldKey::Home { select } => field.home(select),
            FieldKey::End { select } => field.end(select),
            FieldKey::SelectAll => field.select_all(),
            FieldKey::Copy => {
                return field
                    .selected_text()
                    .map_or(Effect::None, Effect::SetClipboard);
            }
            FieldKey::Cut => return field.cut().map_or(Effect::None, Effect::SetClipboard),
            FieldKey::Paste => return Effect::PasteField,
            FieldKey::Submit => return self.create(layout),
            FieldKey::Cancel => return self.cancel(),
        }
        Effect::Redraw
    }

    /// Create: the field's text becomes a new list item (selected, in view); the list gets
    /// the focus so the new item is where the keyboard is. An empty name creates nothing.
    fn create(&mut self, layout: &Layout) -> Effect {
        let name = self.field.text().trim().to_string();
        if name.is_empty() {
            return Effect::None;
        }
        self.list.push(name, layout.visible_rows);
        self.cancel()
    }

    /// Cancel: the text is thrown away and the focus returns to the list.
    fn cancel(&mut self) -> Effect {
        self.field.clear();
        self.set_focus(Focus::List);
        Effect::Redraw
    }

    /// A menu item was chosen: the menu closes and the app does the rest.
    fn pick(&mut self, item: MenuItem) -> Effect {
        self.menu = None;
        match item {
            MenuItem::Copy => Effect::TermCopy,
            MenuItem::Paste => Effect::TermPaste,
            MenuItem::Clear => Effect::TermClear,
        }
    }

    /// The left button went down at a window pixel.
    pub fn press(&mut self, x: f32, y: f32, layout: &Layout) -> Effect {
        if let Some(menu) = self.menu.take() {
            // A click outside only closes the menu; it is not also a click on what is under.
            return menu
                .hit(x, y)
                .map_or(Effect::Redraw, |item| self.pick(item));
        }
        match layout.hit(x, y) {
            Hit::ListRow(row) => {
                let focused = self.set_focus(Focus::List);
                Self::redraw_if(self.list.click_row(row) || focused)
            }
            Hit::Field { col } => {
                self.set_focus(Focus::Field);
                self.field.click(col);
                self.field_drag = true;
                Effect::Redraw
            }
            Hit::Create => self.create(layout),
            Hit::Cancel => self.cancel(),
            Hit::Terminal => {
                self.set_focus(Focus::Terminal);
                Effect::TermPress
            }
            Hit::Sidebar | Hit::Outside => Effect::None,
        }
    }

    /// The left button went up.
    pub fn release(&mut self) {
        self.field_drag = false;
    }

    /// The pointer moved: menu highlight, or a drag that selects in the field.
    pub fn pointer(&mut self, x: f32, y: f32, layout: &Layout) -> Effect {
        if let Some(menu) = &mut self.menu {
            return Self::redraw_if(menu.hover(x, y));
        }
        if self.field_drag {
            return Self::redraw_if(self.field.drag(layout.field_col(x)));
        }
        Effect::None
    }

    /// The right button went down: the context menu opens on the terminal (which gets the
    /// focus); anywhere else an open menu closes.
    pub fn context(&mut self, x: f32, y: f32, layout: &Layout) -> Effect {
        if layout.hit(x, y) == Hit::Terminal {
            self.menu = Some(Menu::open((x, y), layout.window, layout.cell));
            self.set_focus(Focus::Terminal);
            return Effect::Redraw;
        }
        Self::redraw_if(self.menu.take().is_some())
    }

    /// The wheel turned by `rows` (`input::wheel_rows`) with the pointer at a window pixel.
    /// `None`: not over the sidebar, the terminal scrolls.
    pub fn wheel(&mut self, x: f32, y: f32, rows: i32, layout: &Layout) -> Option<Effect> {
        match layout.hit(x, y) {
            Hit::Terminal | Hit::Outside => None,
            _ => Some(Self::redraw_if(self.list.scroll(rows, layout.visible_rows))),
        }
    }

    /// Clipboard text for the field (after `Effect::PasteField`).
    pub fn paste(&mut self, text: &str) -> Effect {
        if self.focus != Focus::Field {
            return Effect::None;
        }
        self.field.paste(text);
        Effect::Redraw
    }

    /// Top-left corner (window pixels) of the cell the input method should put its candidate
    /// window at: the terminal's cursor cell `(col, row)`, or the field's caret (after the
    /// composition), depending on the focus. `None`: no text is being entered.
    pub fn ime_anchor(
        &mut self,
        layout: &Layout,
        cursor: Option<(u16, u16)>,
    ) -> Option<(f32, f32)> {
        let (cell_w, cell_h) = layout.cell;
        match self.focus {
            Focus::Terminal => cursor.map(|(col, row)| {
                (
                    layout.terminal.x + col as f32 * cell_w,
                    layout.terminal.y + row as f32 * cell_h,
                )
            }),
            Focus::Field => {
                let col = self.field.view(layout.field_cols).caret_col;
                Some((
                    layout.field_text.x + col as f32 * cell_w,
                    layout.field_text.y,
                ))
            }
            Focus::List => None,
        }
    }

    /// An IME event while the terminal does not have the focus: the field composes, the list
    /// takes no text.
    pub fn ime(&mut self, ime: &Ime) -> Effect {
        if self.focus != Focus::Field {
            return Effect::None;
        }
        match ime {
            Ime::Preedit(text, _) => Self::redraw_if(self.field.set_preedit(text)),
            Ime::Commit(text) => {
                self.field.commit(text);
                Effect::Redraw
            }
            Ime::Disabled => Self::redraw_if(self.field.set_preedit("")),
            Ime::Enabled => Effect::None,
        }
    }
}

/// One key event from the window, whatever has the focus. This is the only place a key is
/// handed to the terminal: `focus::route` decides, and only `Route::Terminal` reaches
/// `Session::key` (which writes the latency `key` line). `t_key_ns` is the time taken at
/// `window_event` entry; the key parts are those of `input::key_action`.
#[allow(clippy::too_many_arguments)]
pub fn key_event(
    ui: &mut Ui,
    session: &mut Session,
    layout: &Layout,
    t_key_ns: u128,
    logical: &WinitKey,
    text: Option<&str>,
    mods: ModifiersState,
    pressed: bool,
    repeat: bool,
) -> Effect {
    let to = focus::route(
        ui.focus,
        ui.menu.is_some(),
        logical,
        text,
        mods,
        pressed,
        repeat,
    );
    match to {
        Route::Terminal(Action::Send(key)) => {
            session.key(t_key_ns, key);
            Effect::None
        }
        Route::Terminal(Action::Copy) => Effect::TermCopy,
        Route::Terminal(Action::Paste) => Effect::TermPaste,
        Route::Terminal(Action::Ignore) => Effect::None,
        other => ui.key(other, layout),
    }
}

/// One IME event from the window: the terminal's (preedit overlay; a commit is a key with a
/// latency line) while it has the focus, otherwise the UI's.
pub fn ime_event(ui: &mut Ui, session: &mut Session, t_key_ns: u128, ime: &Ime) -> Effect {
    if ui.focus != Focus::Terminal {
        return ui.ime(ime);
    }
    let changed = match ime {
        Ime::Preedit(text, _) => session.set_preedit(text),
        Ime::Commit(text) => {
            session.ime_commit(t_key_ns, text);
            true
        }
        Ime::Disabled => session.set_preedit(""),
        Ime::Enabled => false,
    };
    Ui::redraw_if(changed)
}
