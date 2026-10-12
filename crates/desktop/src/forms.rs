//! The new-session form, the rename field, the kill confirm, the approval and a session's
//! menu: what their keys and clicks do, what they look like in cells, and the commands they
//! end in. No window and no drawing here; `draw` paints a `Panel`, `app` runs a `Request` on a
//! worker thread.
//!
//! **Forms** (tmls `create_form.py`, `manage.py`) are modal: while one is open every key is
//! its own (`keys::Route::Form`) and reaches no session.
//!
//! - *New session*: Host, Folder, Name, Start, then Create and Cancel — the Tab order. The
//!   host and the start are chosen with the arrows. The name follows the folder until it is
//!   edited: at once the folder's own name, then — when the app has asked the host (`lookup`,
//!   `derived_name`) — the name of the git repository the folder is in. Start lists `shell`,
//!   `claude`, then the presets of `agent-presets.json`; the first preset is the default, else
//!   `claude`. Enter creates from every stop but Cancel. A name tmux cannot use blocks Create
//!   and says why.
//! - *Rename*: one field with the old name, selected, so typing replaces it.
//! - *Kill*: asks first, and says what runs in the session. The focus starts on Cancel:
//!   Enter there kills nothing. Kill takes Tab (or an arrow) and then Enter, or a click on
//!   the Kill button. A held Enter never confirms.
//! - *Approve*: Claude's permission dialog as read off the session's pane, then Yes, No and
//!   Cancel; the focus starts on Yes. Enter answers the focused button, `n` answers No. The
//!   answer (`1` or Escape) is sent only if a fresh capture still shows the same dialog.
//! - *Ask* (send a message): a field, tmls's saved prompts under it (↑ / ↓ puts one in the
//!   field), Send and Cancel, and Clear queue while messages wait. Send goes now, or into the
//!   queue while the session is busy — the app decides (`queue`).
//!
//! Esc cancels every form. Tab into a field selects its text (as Textual's inputs do).
//!
//! **The menu** of a session row (`RowMenu`): Approve… while Claude asks for permission, then
//! Rename, Kill…, Open in new terminal, Copy attach command, and Send message… for a Claude
//! session (`menu_items`). Arrows or j / k
//! move, Enter picks, any other key or a click beside it closes it.
//!
//! **Panels.** `Form::panel` and `RowMenu::panel` give the text on a grid of cells, each piece
//! cut to its columns; `click` takes a position in those cells (fractions allowed), relative
//! to the panel's top-left corner.
//!
//! **Commands.** `perform` runs a request's tmux command and returns the line for the status
//! line; the runner, the terminal's spawner and the clipboard are parameters, so tests start
//! nothing.

use std::process::{Command, Stdio};
use std::time::Duration;

use client_core::tmux::{self, NewSession, Start};
use client_core::{KITTY, SessionInfo, SessionKey, Status, approve};
use s2_core::{Key, KeyEvent, Named};
use s2_wgpu::gfx::grid::clusters;
use s2_wgpu::ui::draw::fit;
use s2_wgpu::ui::focus::MenuKey;
use s2_wgpu::ui::textfield::TextField;

/// How long a request's command may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Programs that are not worth a warning before a kill (tmls `tmux_ops.py`, `SHELLS`).
const SHELLS: [&str; 8] = ["sh", "bash", "dash", "zsh", "fish", "ksh", "csh", "tcsh"];

/// Width of the new-session form and of the rename and kill forms, in cells (tmls: 60, 56).
const NEW_COLS: usize = 60;
const ACTION_COLS: usize = 56;
/// Width of a session's menu: the longest label and its padding.
const MENU_COLS: usize = 24;
/// Columns left free at either side of a form.
const PAD: usize = 2;
/// Where a form's values start: after the padding and the labels.
const VALUE_COL: usize = 10;

/// What a form asks the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// `size` is left open: the app fills in the terminal view's.
    New(NewSession),
    Rename(SessionKey, String),
    Kill(SessionKey),
    /// Yes (`1`) or No (Escape) to the dialog `shown`, if the pane still shows it.
    Answer {
        key: SessionKey,
        pane: Option<String>,
        shown: Vec<String>,
        yes: bool,
    },
}

/// What a key or a click did to a form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormResult {
    /// The form stays open.
    Pending,
    Cancelled,
    Submit(Request),
    /// Send `text` to the session now, or queue it while it is busy (the app decides).
    Ask {
        key: SessionKey,
        pane: Option<String>,
        text: String,
    },
    /// Drop the session's queued messages.
    ClearQueue(SessionKey),
}

/// A place the keyboard focus can be inside a form. `Name` is the rename form's field too;
/// `Ok` is Create, Rename or Kill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Host,
    Folder,
    Name,
    Start,
    Ok,
    Cancel,
    /// The approval's No button.
    No,
    /// The Ask form's Clear queue button.
    Clear,
}

const NEW_STOPS: [Stop; 6] = [
    Stop::Host,
    Stop::Folder,
    Stop::Name,
    Stop::Start,
    Stop::Ok,
    Stop::Cancel,
];
const RENAME_STOPS: [Stop; 3] = [Stop::Name, Stop::Ok, Stop::Cancel];
const KILL_STOPS: [Stop; 2] = [Stop::Cancel, Stop::Ok];
const APPROVE_STOPS: [Stop; 3] = [Stop::Ok, Stop::No, Stop::Cancel];
/// Width of the approval form, in cells: room for a command line.
const APPROVE_COLS: usize = 76;
const ASK_STOPS: [Stop; 3] = [Stop::Name, Stop::Ok, Stop::Cancel];
const ASK_STOPS_CLEAR: [Stop; 4] = [Stop::Name, Stop::Ok, Stop::Clear, Stop::Cancel];
const ASK_COLS: usize = 60;
/// The saved prompts the Ask form lists.
const MAX_SAVED_SHOWN: usize = 8;
/// The saved prompt when `~/.config/tmls/prompts` has none (tmls `prompts.DEFAULT`).
pub const DEFAULT_PROMPT: &str = "What's the progress?";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewForm {
    /// Index into the hosts the form was opened with.
    pub host: usize,
    pub folder: TextField,
    pub name: TextField,
    /// Index into `starts`.
    pub start: usize,
    /// The name is the user's: it no longer follows the folder.
    pub name_edited: bool,
    pub focus: Stop,
    /// Why Create did nothing.
    pub error: Option<String>,
    hosts: Vec<String>,
    starts: Vec<(String, Start)>,
    /// The name the form itself last put in the field.
    auto_name: String,
    /// The host and folder a derived name was last asked for.
    looked_up: (usize, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Form {
    New(NewForm),
    Rename {
        key: SessionKey,
        field: TextField,
        focus: Stop,
        error: Option<String>,
    },
    ConfirmKill {
        key: SessionKey,
        /// What runs in the session besides shells; empty until the host has said.
        running: Vec<String>,
        focus: Stop,
    },
    /// Claude asks for permission: its dialog, Yes / No / Cancel (tmls `approve.py`).
    Approve {
        key: SessionKey,
        pane: Option<String>,
        dialog: Vec<String>,
        focus: Stop,
    },
    /// A message for a Claude session: a field, the saved prompts, how many already wait.
    Ask {
        key: SessionKey,
        pane: Option<String>,
        field: TextField,
        saved: Vec<String>,
        /// The saved prompt ↑ / ↓ last put into the field.
        pick: Option<usize>,
        queued: usize,
        focus: Stop,
        error: Option<String>,
    },
}

/// How a piece of a panel looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Title,
    Label,
    Text,
    Dim,
    Error,
    /// A text field; `focused` shows its caret.
    Field {
        focused: bool,
    },
    /// The host, or one of the starts; `on` marks the chosen one.
    Choice {
        focused: bool,
        on: bool,
    },
    Button {
        focused: bool,
        danger: bool,
    },
    /// A row of the menu.
    Item {
        hovered: bool,
        danger: bool,
    },
}

/// What a click on a piece of a panel means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Stop(Stop),
    Start(usize),
    Item(usize),
}

/// A piece of a panel: `text`, already cut to `cols` columns, from column `col` of `line`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub line: usize,
    pub col: usize,
    pub cols: usize,
    pub text: String,
    pub look: Look,
    /// A field's caret: before this column of the cell.
    pub caret: Option<usize>,
    /// A field's selected columns, end exclusive.
    pub selection: Option<(usize, usize)>,
    pub target: Option<Target>,
}

/// A form or a menu as text on a grid of `cols` x `lines` cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub cols: usize,
    pub lines: usize,
    pub cells: Vec<Cell>,
}

impl Panel {
    pub(crate) fn new(cols: usize) -> Panel {
        Panel {
            cols,
            lines: 0,
            cells: Vec::new(),
        }
    }

    /// Adds `text`, cut to `cols` columns.
    pub(crate) fn put(
        &mut self,
        line: usize,
        col: usize,
        cols: usize,
        text: &str,
        look: Look,
    ) -> &mut Cell {
        let text: String = fit(text, cols)
            .into_iter()
            .map(|(cluster, _)| cluster)
            .collect();
        self.cells.push(Cell {
            line,
            col,
            cols,
            text,
            look,
            caret: None,
            selection: None,
            target: None,
        });
        self.cells.last_mut().expect("just pushed")
    }

    /// Adds `text` on as many columns as it takes, up to `max`.
    fn tight(&mut self, line: usize, col: usize, max: usize, text: &str, look: Look) -> &mut Cell {
        let cols: usize = clusters(text).iter().map(|(_, width)| width).sum();
        self.put(line, col, cols.clamp(1, max), text, look)
    }

    /// Adds a text field `cols` columns wide.
    fn field(
        &mut self,
        line: usize,
        col: usize,
        cols: usize,
        field: &mut TextField,
        stop: Stop,
        focus: Stop,
    ) {
        let view = field.view(cols.saturating_sub(1));
        let mut text = String::new();
        let mut at = 0;
        for (cluster, cell_col, width) in &view.cells {
            text.extend(std::iter::repeat_n(' ', cell_col.saturating_sub(at)));
            text.push_str(cluster);
            at = cell_col + width;
        }
        let focused = stop == focus;
        let cell = self.put(line, col, cols, &text, Look::Field { focused });
        cell.caret = focused.then_some(view.caret_col);
        cell.selection = view.selection.filter(|_| focused);
        cell.target = Some(Target::Stop(stop));
    }

    /// Adds the buttons at the right end of `line`: `ok`, and Cancel left of it.
    fn buttons(&mut self, line: usize, ok: &str, danger: bool, focus: Stop) {
        let ok = format!(" {ok} ");
        let cancel = " Cancel ";
        let ok_col = self.cols - PAD - ok.len();
        let cancel_col = ok_col - 1 - cancel.len();
        let look = |stop: Stop, danger: bool| Look::Button {
            focused: focus == stop,
            danger,
        };
        self.put(
            line,
            cancel_col,
            cancel.len(),
            cancel,
            look(Stop::Cancel, false),
        )
        .target = Some(Target::Stop(Stop::Cancel));
        self.put(line, ok_col, ok.len(), &ok, look(Stop::Ok, danger))
            .target = Some(Target::Stop(Stop::Ok));
    }

    /// The piece under a position in cells that a click means something on.
    pub(crate) fn target_at(&self, x: f32, y: f32) -> Option<(Target, usize)> {
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let (col, line) = (x.floor() as usize, y.floor() as usize);
        self.cells
            .iter()
            .filter(|cell| cell.line == line && (cell.col..cell.col + cell.cols).contains(&col))
            .find_map(|cell| Some((cell.target?, col - cell.col)))
    }

    /// Whether a position in cells is on the panel.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= 0.0 && y >= 0.0 && x < self.cols as f32 && y < self.lines as f32
    }
}

/// The stop after (`forward`) or before `focus` in `stops`, round the ends.
fn step(stops: &[Stop], focus: Stop, forward: bool) -> Stop {
    let n = stops.len();
    let at = stops.iter().position(|stop| *stop == focus).unwrap_or(0);
    stops[if forward { at + 1 } else { at + n - 1 } % n]
}

/// The index after or before `at` of `n`, round the ends.
fn cycle(at: usize, n: usize, forward: bool) -> usize {
    if n == 0 {
        return 0;
    }
    if forward {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    }
}

/// Replaces a field's text; the caret ends after it.
fn set_text(field: &mut TextField, text: &str) {
    field.clear();
    field.insert(text);
}

/// A key in a text field. Returns whether the field took it.
fn edit(field: &mut TextField, ev: &KeyEvent) -> bool {
    let mods = ev.mods;
    if mods.alt || mods.logo {
        return false;
    }
    if mods.ctrl {
        if ev.key == Key::Char('a') || ev.key == Key::Char('A') {
            field.select_all();
            return true;
        }
        return false;
    }
    match ev.key {
        Key::Char(c) => {
            let typed = ev.text.clone().unwrap_or_else(|| c.to_string());
            field.insert(&typed);
        }
        Key::Named(Named::Backspace) => {
            field.backspace();
        }
        Key::Named(Named::Delete) => {
            field.delete();
        }
        Key::Named(Named::Left) => field.left(mods.shift),
        Key::Named(Named::Right) => field.right(mods.shift),
        Key::Named(Named::Home) => field.home(mods.shift),
        Key::Named(Named::End) => field.end(mods.shift),
        _ => return false,
    }
    true
}

/// Whether an arrow key goes forwards (down, right) or backwards; `None` for any other key.
fn arrow(ev: &KeyEvent) -> Option<bool> {
    match ev.key {
        Key::Named(Named::Down | Named::Right) => Some(true),
        Key::Named(Named::Up | Named::Left) => Some(false),
        _ => None,
    }
}

impl NewForm {
    fn new(hosts: &[String], host: &str, presets: &[(String, Vec<String>)]) -> NewForm {
        let mut starts = vec![
            ("shell".to_string(), Start::Shell),
            ("claude".to_string(), Start::Claude),
        ];
        starts.extend(
            presets
                .iter()
                .map(|(name, argv)| (name.clone(), Start::Preset(argv.clone()))),
        );
        let hosts = match hosts.is_empty() {
            true => vec![host.to_string()],
            false => hosts.to_vec(),
        };
        let host = hosts.iter().position(|h| h == host).unwrap_or(0);
        let (mut folder, mut name) = (TextField::new(), TextField::new());
        let auto_name = tmux::folder_name("~").to_string();
        set_text(&mut folder, "~");
        set_text(&mut name, &auto_name);
        NewForm {
            host,
            folder,
            name,
            // tmls: the first preset is what is usually started; without presets, claude.
            start: if presets.is_empty() { 1 } else { 2 },
            name_edited: false,
            focus: Stop::Host,
            error: None,
            hosts,
            starts,
            auto_name,
            looked_up: (host, "~".to_string()),
        }
    }

    /// What can be started, in the form's order, with the labels shown.
    pub fn starts(&self) -> &[(String, Start)] {
        &self.starts
    }

    /// The host the session is to be made on.
    pub fn host_name(&self) -> &str {
        &self.hosts[self.host]
    }

    /// The folder as it will be used: trimmed, `~` when empty.
    fn folder_text(&self) -> String {
        match self.folder.text().trim() {
            "" => "~".to_string(),
            folder => folder.to_string(),
        }
    }

    /// The host and folder whose git-derived name is to be looked up now: `Some` once after
    /// each change of either, while the name still follows the folder.
    pub fn lookup(&mut self) -> Option<(String, String)> {
        let now = (self.host, self.folder_text());
        if self.name_edited || now == self.looked_up {
            return None;
        }
        self.looked_up = now;
        Some((self.host_name().to_string(), self.looked_up.1.clone()))
    }

    /// The answer to a `lookup`: becomes the name if the form still shows that host and
    /// folder and the name is still its own.
    pub fn derived(&mut self, host: &str, folder: &str, name: &str) {
        let current = self.host_name() == host && self.folder_text() == folder;
        if current && !self.name_edited && !name.is_empty() {
            self.auto_name = name.to_string();
            set_text(&mut self.name, name);
        }
    }

    /// After the folder's text changed: the name follows unless it was edited.
    fn folder_changed(&mut self) {
        self.error = None;
        if !self.name_edited {
            self.auto_name = tmux::folder_name(self.folder.text().trim()).to_string();
            set_text(&mut self.name, &self.auto_name);
        }
    }

    /// After the name's text changed.
    fn name_changed(&mut self) {
        self.error = None;
        self.name_edited = self.name.text() != self.auto_name;
    }

    fn focus_on(&mut self, stop: Stop) {
        self.focus = stop;
        match stop {
            Stop::Folder => self.folder.select_all(),
            Stop::Name => self.name.select_all(),
            _ => {}
        }
    }

    fn submit(&mut self) -> FormResult {
        let name = self.name.text().trim().to_string();
        if let Err(why) = tmux::valid_session_name(&name) {
            self.error = Some(why);
            return FormResult::Pending;
        }
        FormResult::Submit(Request::New(NewSession {
            host: self.host_name().to_string(),
            folder: self.folder_text(),
            name,
            start: self.starts[self.start].1.clone(),
            size: None,
        }))
    }

    /// A key that is neither Esc, Tab nor Enter.
    fn other_key(&mut self, ev: &KeyEvent) {
        match self.focus {
            Stop::Host => {
                if let Some(forward) = arrow(ev) {
                    self.host = cycle(self.host, self.hosts.len(), forward);
                }
            }
            Stop::Start => {
                if let Some(forward) = arrow(ev) {
                    self.start = cycle(self.start, self.starts.len(), forward);
                }
            }
            Stop::Folder => {
                let before = self.folder.text().to_string();
                if edit(&mut self.folder, ev) && self.folder.text() != before {
                    self.folder_changed();
                }
            }
            Stop::Name => {
                let before = self.name.text().to_string();
                if edit(&mut self.name, ev) && self.name.text() != before {
                    self.name_changed();
                }
            }
            Stop::Ok | Stop::Cancel => {
                if arrow(ev).is_some() {
                    self.focus = other_button(self.focus);
                }
            }
            Stop::No | Stop::Clear => {}
        }
    }

    fn panel(&mut self) -> Panel {
        let mut p = Panel::new(NEW_COLS);
        let focus = self.focus;
        let value_cols = NEW_COLS - VALUE_COL - PAD;
        let label = |p: &mut Panel, line: usize, text: &str| {
            p.put(line, PAD, VALUE_COL - PAD - 1, text, Look::Label);
        };
        p.put(0, PAD, NEW_COLS - 2 * PAD, "New session", Look::Title);
        label(&mut p, 2, "Host");
        let host = format!("< {} >", self.host_name());
        let host_look = Look::Choice {
            focused: focus == Stop::Host,
            on: true,
        };
        p.tight(2, VALUE_COL, value_cols, &host, host_look).target = Some(Target::Stop(Stop::Host));
        label(&mut p, 3, "Folder");
        p.field(
            3,
            VALUE_COL,
            value_cols,
            &mut self.folder,
            Stop::Folder,
            focus,
        );
        label(&mut p, 4, "Name");
        p.field(4, VALUE_COL, value_cols, &mut self.name, Stop::Name, focus);
        label(&mut p, 5, "Start");
        for (index, (name, _)) in self.starts.iter().enumerate() {
            let on = index == self.start;
            let text = format!("{} {name}", if on { '●' } else { '○' });
            let look = Look::Choice {
                focused: on && focus == Stop::Start,
                on,
            };
            p.tight(5 + index, VALUE_COL, value_cols, &text, look)
                .target = Some(Target::Start(index));
        }
        let after = 5 + self.starts.len();
        if let Some(error) = &self.error {
            p.put(after, PAD, NEW_COLS - 2 * PAD, error, Look::Error);
        }
        p.buttons(after + 1, "Create", false, focus);
        p.lines = after + 3;
        p
    }
}

/// Cancel for Ok and Ok for Cancel.
fn other_button(focus: Stop) -> Stop {
    if focus == Stop::Ok {
        Stop::Cancel
    } else {
        Stop::Ok
    }
}

impl Form {
    /// The new-session form for `hosts`, on `host`, with the presets of `agent-presets.json`.
    pub fn new_session(hosts: &[String], host: &str, presets: &[(String, Vec<String>)]) -> Form {
        Form::New(NewForm::new(hosts, host, presets))
    }

    /// The rename field for a session: its name, selected.
    pub fn rename(key: SessionKey) -> Form {
        let mut field = TextField::new();
        set_text(&mut field, &key.name);
        field.select_all();
        Form::Rename {
            key,
            field,
            focus: Stop::Name,
            error: None,
        }
    }

    /// The question before a kill. The focus is on Cancel.
    pub fn confirm_kill(key: SessionKey) -> Form {
        Form::ConfirmKill {
            key,
            running: Vec::new(),
            focus: Stop::Cancel,
        }
    }

    /// The question of Claude's permission dialog, as captured. The focus is on Yes.
    pub fn approve(key: SessionKey, pane: Option<String>, dialog: Vec<String>) -> Form {
        Form::Approve {
            key,
            pane,
            dialog,
            focus: Stop::Ok,
        }
    }

    /// The message form for a session; `queued` messages wait for it already.
    pub fn ask(key: SessionKey, pane: Option<String>, saved: Vec<String>, queued: usize) -> Form {
        Form::Ask {
            key,
            pane,
            field: TextField::new(),
            saved,
            pick: None,
            queued,
            focus: Stop::Name,
            error: None,
        }
    }

    /// Where the keyboard focus is.
    pub fn focus(&self) -> Stop {
        match self {
            Form::New(new) => new.focus,
            Form::Rename { focus, .. }
            | Form::ConfirmKill { focus, .. }
            | Form::Approve { focus, .. }
            | Form::Ask { focus, .. } => *focus,
        }
    }

    /// Why the last Create or Rename did nothing.
    pub fn error(&self) -> Option<&str> {
        match self {
            Form::New(new) => new.error.as_deref(),
            Form::Rename { error, .. } => error.as_deref(),
            Form::Ask { error, .. } => error.as_deref(),
            Form::ConfirmKill { .. } | Form::Approve { .. } => None,
        }
    }

    /// What runs in `session`, for the kill confirm of that session.
    pub fn set_running(&mut self, session: &SessionKey, commands: Vec<String>) {
        if let Form::ConfirmKill { key, running, .. } = self
            && key == session
        {
            *running = commands;
        }
    }

    fn stops(&self) -> &'static [Stop] {
        match self {
            Form::New(_) => &NEW_STOPS,
            Form::Rename { .. } => &RENAME_STOPS,
            Form::ConfirmKill { .. } => &KILL_STOPS,
            Form::Approve { .. } => &APPROVE_STOPS,
            Form::Ask { queued, .. } if *queued > 0 => &ASK_STOPS_CLEAR,
            Form::Ask { .. } => &ASK_STOPS,
        }
    }

    fn focus_on(&mut self, stop: Stop) {
        match self {
            Form::New(new) => new.focus_on(stop),
            Form::Rename { focus, field, .. } => {
                *focus = stop;
                if stop == Stop::Name {
                    field.select_all();
                }
            }
            Form::Ask { focus, field, .. } => {
                *focus = stop;
                if stop == Stop::Name {
                    field.select_all();
                }
            }
            Form::ConfirmKill { focus, .. } | Form::Approve { focus, .. } => *focus = stop,
        }
    }

    /// Create, Rename or Kill.
    fn submit(&mut self) -> FormResult {
        match self {
            Form::New(new) => new.submit(),
            Form::Rename {
                key, field, error, ..
            } => {
                let name = field.text().trim().to_string();
                if name == key.name {
                    return FormResult::Cancelled;
                }
                if let Err(why) = tmux::valid_session_name(&name) {
                    *error = Some(why);
                    return FormResult::Pending;
                }
                FormResult::Submit(Request::Rename(key.clone(), name))
            }
            Form::ConfirmKill { key, .. } => FormResult::Submit(Request::Kill(key.clone())),
            Form::Approve {
                key,
                pane,
                dialog,
                focus,
            } => FormResult::Submit(Request::Answer {
                key: key.clone(),
                pane: pane.clone(),
                shown: dialog.clone(),
                yes: *focus != Stop::No,
            }),
            Form::Ask {
                key,
                pane,
                field,
                error,
                ..
            } => {
                let text = field.text().trim().to_string();
                if text.is_empty() {
                    *error = Some("type a message first".to_string());
                    return FormResult::Pending;
                }
                FormResult::Ask {
                    key: key.clone(),
                    pane: pane.clone(),
                    text,
                }
            }
        }
    }

    /// Enter, or Space on a button: what the focused stop does.
    fn activate(&mut self) -> FormResult {
        match (self.focus(), &*self) {
            (Stop::Cancel, _) => FormResult::Cancelled,
            (Stop::Clear, Form::Ask { key, .. }) => FormResult::ClearQueue(key.clone()),
            _ => self.submit(),
        }
    }

    /// A key while the form is open.
    pub fn key(&mut self, ev: &KeyEvent) -> FormResult {
        if !ev.pressed {
            return FormResult::Pending;
        }
        let chord = ev.mods.ctrl || ev.mods.alt || ev.mods.logo;
        let on_button = matches!(
            self.focus(),
            Stop::Ok | Stop::Cancel | Stop::No | Stop::Clear
        );
        match ev.key {
            Key::Named(Named::Escape) => return FormResult::Cancelled,
            Key::Named(Named::Tab) if !chord => {
                let to = step(self.stops(), self.focus(), !ev.mods.shift);
                self.focus_on(to);
                return FormResult::Pending;
            }
            // A held key confirms nothing: it may be the Enter that opened the form.
            Key::Named(Named::Enter) | Key::Char(' ')
                if ev.repeat && (on_button || ev.key != Key::Char(' ')) =>
            {
                return FormResult::Pending;
            }
            Key::Named(Named::Enter) if !chord => return self.activate(),
            Key::Char(' ') if !chord && on_button => return self.activate(),
            _ => {}
        }
        let mut decline = false;
        match self {
            Form::New(new) => new.other_key(ev),
            Form::Rename {
                field,
                focus: Stop::Name,
                error,
                ..
            } => {
                let before = field.text().to_string();
                if edit(field, ev) && field.text() != before {
                    *error = None;
                }
            }
            Form::Approve { focus, .. } => {
                if ev.key == Key::Char('n') && !chord && !ev.repeat {
                    *focus = Stop::No;
                    decline = true;
                } else if let Some(forward) = arrow(ev) {
                    *focus = step(&APPROVE_STOPS, *focus, forward);
                }
            }
            Form::Ask {
                field,
                focus: Stop::Name,
                saved,
                pick,
                error,
                ..
            } => match ev.key {
                Key::Named(Named::Up | Named::Down) if !saved.is_empty() && !chord => {
                    let n = saved.len().min(MAX_SAVED_SHOWN);
                    let forward = ev.key == Key::Named(Named::Down);
                    let next = match *pick {
                        None if forward => 0,
                        None => n - 1,
                        Some(at) => cycle(at, n, forward),
                    };
                    *pick = Some(next);
                    set_text(field, &saved[next]);
                    *error = None;
                }
                _ => {
                    let before = field.text().to_string();
                    if edit(field, ev) && field.text() != before {
                        *error = None;
                    }
                }
            },
            Form::Ask { focus, queued, .. } => {
                if let Some(forward) = arrow(ev) {
                    let stops: &[Stop] = if *queued > 0 {
                        &ASK_STOPS_CLEAR
                    } else {
                        &ASK_STOPS
                    };
                    *focus = step(stops, *focus, forward);
                }
            }
            Form::Rename { focus, .. } | Form::ConfirmKill { focus, .. } => {
                if arrow(ev).is_some() {
                    *focus = other_button(*focus);
                }
            }
        }
        if decline {
            return self.submit();
        }
        FormResult::Pending
    }

    /// Clipboard text for the focused field; nothing elsewhere.
    pub fn paste(&mut self, text: &str) {
        match self {
            Form::New(new) if new.focus == Stop::Folder => {
                new.folder.paste(text);
                new.folder_changed();
            }
            Form::New(new) if new.focus == Stop::Name => {
                new.name.paste(text);
                new.name_changed();
            }
            Form::Rename {
                field,
                focus: Stop::Name,
                error,
                ..
            } => {
                field.paste(text);
                *error = None;
            }
            Form::Ask {
                field,
                focus: Stop::Name,
                error,
                ..
            } => {
                field.paste(text);
                *error = None;
            }
            _ => {}
        }
    }

    /// A click at `(x, y)` cells from the panel's top-left corner.
    pub fn click(&mut self, x: f32, y: f32) -> FormResult {
        let Some((target, col)) = self.panel().target_at(x, y) else {
            return FormResult::Pending;
        };
        match (target, &mut *self) {
            (Target::Stop(Stop::Cancel), _) => return FormResult::Cancelled,
            (Target::Stop(Stop::Ok), form) => {
                form.focus_on(Stop::Ok);
                return form.submit();
            }
            (Target::Stop(Stop::No), form) => {
                form.focus_on(Stop::No);
                return form.submit();
            }
            (Target::Stop(Stop::Host), Form::New(new)) => {
                new.focus = Stop::Host;
                new.host = cycle(new.host, new.hosts.len(), true);
            }
            (Target::Stop(Stop::Folder), Form::New(new)) => {
                new.focus = Stop::Folder;
                new.folder.click(col);
            }
            (Target::Stop(Stop::Name), Form::New(new)) => {
                new.focus = Stop::Name;
                new.name.click(col);
            }
            (Target::Stop(Stop::Name), Form::Rename { field, focus, .. }) => {
                *focus = Stop::Name;
                field.click(col);
            }
            (Target::Stop(Stop::Clear), Form::Ask { key, .. }) => {
                return FormResult::ClearQueue(key.clone());
            }
            (Target::Stop(Stop::Name), Form::Ask { field, focus, .. }) => {
                *focus = Stop::Name;
                field.click(col);
            }
            (Target::Start(index), Form::New(new)) => {
                new.focus = Stop::Start;
                new.start = index.min(new.starts.len() - 1);
            }
            _ => {}
        }
        FormResult::Pending
    }

    /// The form as text in cells.
    pub fn panel(&mut self) -> Panel {
        let inner = ACTION_COLS - 2 * PAD;
        match self {
            Form::New(new) => new.panel(),
            Form::Rename {
                key,
                field,
                focus,
                error,
            } => {
                let mut p = Panel::new(ACTION_COLS);
                let title = format!("{} · {}", key.host, key.name);
                p.put(0, PAD, inner, &title, Look::Title);
                p.put(2, PAD, inner, "New name", Look::Label);
                p.field(3, PAD, inner, field, Stop::Name, *focus);
                if let Some(error) = error {
                    p.put(4, PAD, inner, error, Look::Error);
                }
                p.buttons(5, "Rename", false, *focus);
                p.lines = 7;
                p
            }
            Form::ConfirmKill {
                key,
                running,
                focus,
            } => {
                let mut p = Panel::new(ACTION_COLS);
                let title = format!("{} · {}", key.host, key.name);
                p.put(0, PAD, inner, &title, Look::Title);
                p.put(2, PAD, inner, &format!("Kill {}?", key.name), Look::Text);
                p.put(3, PAD, inner, "This ends its tmux session.", Look::Text);
                if !running.is_empty() {
                    let running = format!("Running: {}", running.join(", "));
                    p.put(4, PAD, inner, &running, Look::Error);
                }
                p.buttons(6, "Kill", true, *focus);
                p.lines = 8;
                p
            }
            Form::Approve {
                key, dialog, focus, ..
            } => {
                let inner = APPROVE_COLS - 2 * PAD;
                let mut p = Panel::new(APPROVE_COLS);
                p.put(
                    0,
                    PAD,
                    inner,
                    &format!("{} · {} asks", key.host, key.name),
                    Look::Title,
                );
                let shown = approve::shown(dialog);
                for (n, line) in shown.iter().enumerate() {
                    let look = if n == 0 { Look::Label } else { Look::Text };
                    p.put(2 + n, PAD, inner, line, look);
                }
                let hint = 2 + shown.len() + 1;
                p.put(
                    hint,
                    PAD,
                    inner,
                    "Enter: Yes · n: No · Esc: Cancel",
                    Look::Dim,
                );
                let buttons = hint + 1;
                // Right to left: Cancel at the end, No, then Yes.
                let mut col = APPROVE_COLS - PAD;
                for (stop, label) in [
                    (Stop::Cancel, " Cancel "),
                    (Stop::No, " No "),
                    (Stop::Ok, " Yes "),
                ] {
                    col -= label.len();
                    let look = Look::Button {
                        focused: *focus == stop,
                        danger: false,
                    };
                    p.put(buttons, col, label.len(), label, look).target = Some(Target::Stop(stop));
                    col -= 1;
                }
                p.lines = buttons + 2;
                p
            }
            Form::Ask {
                key,
                field,
                saved,
                pick,
                queued,
                focus,
                error,
                ..
            } => {
                let inner = ASK_COLS - 2 * PAD;
                let mut p = Panel::new(ASK_COLS);
                let title = match *queued {
                    0 => format!("Send to {}", key.name),
                    n => format!("Send to {} · {n} queued", key.name),
                };
                p.put(0, PAD, inner, &title, Look::Title);
                p.field(2, PAD, inner, field, Stop::Name, *focus);
                if let Some(error) = error {
                    p.put(3, PAD, inner, error, Look::Error);
                }
                p.put(4, PAD, inner, "Saved (↑ / ↓)", Look::Label);
                let shown = saved.len().min(MAX_SAVED_SHOWN);
                for (n, prompt) in saved.iter().take(shown).enumerate() {
                    let look = match *pick == Some(n) {
                        true => Look::Choice {
                            focused: *focus == Stop::Name,
                            on: true,
                        },
                        false => Look::Dim,
                    };
                    p.put(5 + n, PAD, inner, prompt, look);
                }
                let buttons = 5 + shown + 1;
                if *queued > 0 {
                    let label = " Clear queue ";
                    let look = Look::Button {
                        focused: *focus == Stop::Clear,
                        danger: true,
                    };
                    p.put(buttons, PAD, label.len(), label, look).target =
                        Some(Target::Stop(Stop::Clear));
                }
                p.buttons(buttons, "Send", false, *focus);
                p.lines = buttons + 2;
                p
            }
        }
    }
}

/// An entry of a session's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    /// Answer the session's permission prompt; listed only while it asks.
    Approve,
    Rename,
    Kill,
    OpenTerminal,
    CopyAttach,
    /// Send or queue a message; listed for Claude sessions in tmux.
    Send,
}

/// The menu from top to bottom.
pub const MENU: [MenuItem; 4] = [
    MenuItem::Rename,
    MenuItem::Kill,
    MenuItem::OpenTerminal,
    MenuItem::CopyAttach,
];

impl MenuItem {
    pub fn label(self) -> &'static str {
        match self {
            MenuItem::Approve => "Approve…",
            MenuItem::Rename => "Rename",
            // It asks first.
            MenuItem::Kill => "Kill…",
            MenuItem::OpenTerminal => "Open in new terminal",
            MenuItem::CopyAttach => "Copy attach command",
            MenuItem::Send => "Send message…",
        }
    }
}

/// What a key or a click did to the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuResult {
    Pending,
    Closed,
    Pick(MenuItem),
}

/// The menu of one session row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowMenu {
    pub key: SessionKey,
    /// The entries, top to bottom.
    items: Vec<MenuItem>,
    /// The row the arrows or the pointer are on.
    hovered: Option<usize>,
}

impl RowMenu {
    /// The menu for a session, with its first row (Rename) marked.
    pub fn new(key: SessionKey) -> RowMenu {
        RowMenu {
            key,
            items: MENU.to_vec(),
            hovered: Some(0),
        }
    }

    /// The menu with these entries (`menu_items`), its first one marked.
    pub fn with(key: SessionKey, items: Vec<MenuItem>) -> RowMenu {
        RowMenu {
            key,
            items,
            hovered: Some(0),
        }
    }

    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    pub fn key(&mut self, ev: &KeyEvent) -> MenuResult {
        if !ev.pressed {
            return MenuResult::Pending;
        }
        let key = match ev.key {
            Key::Named(Named::Up) | Key::Char('k') => MenuKey::Up,
            Key::Named(Named::Down) | Key::Char('j') => MenuKey::Down,
            Key::Named(Named::Enter) if ev.repeat => return MenuResult::Pending,
            Key::Named(Named::Enter) => MenuKey::Activate,
            _ => MenuKey::Close,
        };
        let n = self.items.len();
        match key {
            MenuKey::Down => self.hovered = Some(self.hovered.map_or(0, |i| (i + 1) % n)),
            MenuKey::Up => self.hovered = Some(self.hovered.map_or(n - 1, |i| (i + n - 1) % n)),
            MenuKey::Activate => {
                return self
                    .hovered
                    .map_or(MenuResult::Pending, |i| MenuResult::Pick(self.items[i]));
            }
            MenuKey::Close => return MenuResult::Closed,
        }
        MenuResult::Pending
    }

    fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        match self.panel().target_at(x, y) {
            Some((Target::Item(index), _)) => Some(index),
            _ => None,
        }
    }

    /// The pointer moved to `(x, y)` cells from the panel's corner. Returns whether the
    /// marked row changed. Off the rows the mark stays where it is.
    pub fn hover(&mut self, x: f32, y: f32) -> bool {
        let Some(index) = self.item_at(x, y) else {
            return false;
        };
        let changed = self.hovered != Some(index);
        self.hovered = Some(index);
        changed
    }

    /// A click at `(x, y)` cells from the panel's corner: picks a row, closes beside the menu.
    pub fn click(&mut self, x: f32, y: f32) -> MenuResult {
        match self.item_at(x, y) {
            Some(index) => MenuResult::Pick(self.items[index]),
            None if self.panel().contains(x, y) => MenuResult::Pending,
            None => MenuResult::Closed,
        }
    }

    /// The menu as text in cells: the session's name, then one row per entry.
    pub fn panel(&self) -> Panel {
        let mut p = Panel::new(MENU_COLS);
        p.put(0, 1, MENU_COLS - 2, &self.key.name, Look::Title);
        for (index, item) in self.items.iter().enumerate() {
            let look = Look::Item {
                hovered: self.hovered == Some(index),
                danger: *item == MenuItem::Kill,
            };
            let text = format!(" {}", item.label());
            p.put(index + 1, 0, MENU_COLS, &text, look).target = Some(Target::Item(index));
        }
        p.lines = self.items.len() + 1;
        p
    }
}

/// Runs a command and returns what it printed; `tmux::run` in the app.
pub type Runner<'a> = &'a dyn Fn(&[String]) -> anyhow::Result<String>;

/// Runs a command with bytes on its stdin and returns what it printed;
/// `tmux::run_with_stdin` in the app.
pub type InputRunner<'a> = &'a dyn Fn(&[String], &[u8]) -> anyhow::Result<String>;

/// The runner the app uses: `tmux::run` with `REQUEST_TIMEOUT`.
pub fn run_tmux(argv: &[String]) -> anyhow::Result<String> {
    tmux::run(argv, REQUEST_TIMEOUT)
}

/// The command that does what `request` asks.
pub fn request_argv(request: &Request, local_name: &str) -> Vec<String> {
    match request {
        Request::New(new) => tmux::new_session_argv(new, local_name),
        Request::Rename(key, name) => tmux::rename_argv(key, name, local_name),
        Request::Kill(key) => tmux::kill_argv(key, local_name),
        Request::Answer { key, pane, yes, .. } => tmux::send_keys_argv(
            key,
            pane.as_deref(),
            if *yes {
                approve::YES_KEY
            } else {
                approve::NO_KEY
            },
            local_name,
        ),
    }
}

/// Does what `request` asks and says, in one line for the status line, what happened.
pub fn perform(request: &Request, local_name: &str, run: Runner) -> Result<String, String> {
    let (done, failed) = match request {
        Request::Answer {
            key,
            pane,
            shown,
            yes,
        } => return answer(key, pane.as_deref(), shown, *yes, local_name, run),
        Request::New(new) => (
            format!("created {} on {}", new.name, new.host),
            format!("couldn't create {} on {}", new.name, new.host),
        ),
        Request::Rename(key, name) => (
            format!("renamed {} to {name} on {}", key.name, key.host),
            format!("couldn't rename {} on {}", key.name, key.host),
        ),
        Request::Kill(key) => (
            format!("killed {} on {}", key.name, key.host),
            format!("couldn't kill {} on {}", key.name, key.host),
        ),
    };
    match run(&request_argv(request, local_name)) {
        Ok(_) => Ok(done),
        Err(e) => Err(format!("{failed}: {}", first_line(&e))),
    }
}

/// The git-derived name for a new session in `folder` on `host`; `None` when the host did
/// not say.
pub fn derived_name(host: &str, folder: &str, local_name: &str, run: Runner) -> Option<String> {
    let out = run(&tmux::derived_name_argv(host, folder, local_name)).ok()?;
    let name = out.lines().next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// What runs in the session besides shells, sorted, each once (tmls `running_commands`).
/// Empty when the host did not say.
pub fn running_commands(key: &SessionKey, local_name: &str, run: Runner) -> Vec<String> {
    let out = run(&tmux::running_commands_argv(key, local_name)).unwrap_or_default();
    let mut commands: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|command| !command.is_empty() && !SHELLS.contains(command))
        .map(str::to_string)
        .collect();
    commands.sort();
    commands.dedup();
    commands
}

/// What Claude's status file says while it waits for a permission answer.
pub const PERMISSION_PROMPT: &str = "permission prompt";

/// The session waits on a permission prompt that can be answered from here (it runs in tmux).
pub fn asks_permission(info: &SessionInfo) -> bool {
    info.status == Status::Waiting
        && info.reason.as_deref() == Some(PERMISSION_PROMPT)
        && info.key.host != KITTY
}

/// The entries of a session's menu: `Approve…` first while it asks, then the usual ones.
pub fn menu_items(info: Option<&SessionInfo>) -> Vec<MenuItem> {
    let mut items = Vec::with_capacity(MENU.len() + 2);
    if info.is_some_and(asks_permission) {
        items.push(MenuItem::Approve);
    }
    items.extend(MENU);
    if info.is_some_and(is_claude) {
        items.push(MenuItem::Send);
    }
    items
}

/// A Claude session in tmux: its status file named it (session id or pane). Messages go only
/// to those.
pub fn is_claude(info: &SessionInfo) -> bool {
    info.key.host != KITTY && (info.session_id.is_some() || info.pane.is_some())
}

/// tmls's saved prompts (`~/.config/tmls/prompts`: one per line, `#` comments), or the default.
pub fn saved_prompts(text: Option<&str>) -> Vec<String> {
    let saved: Vec<String> = text
        .unwrap_or("")
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if saved.is_empty() {
        vec![DEFAULT_PROMPT.to_string()]
    } else {
        saved
    }
}

/// The input runner the app uses: `tmux::run_with_stdin` with `REQUEST_TIMEOUT`.
pub fn run_tmux_with(argv: &[String], input: &[u8]) -> anyhow::Result<String> {
    tmux::run_with_stdin(argv, input, REQUEST_TIMEOUT)
}

/// Types `text` into Claude's pane of the session and presses Enter; says so.
pub fn send_text(
    key: &SessionKey,
    pane: Option<&str>,
    text: &str,
    local_name: &str,
    feed: InputRunner,
) -> Result<String, String> {
    let argv = tmux::send_text_argv(key, pane, &tmux::buffer_name(), local_name);
    feed(&argv, text.as_bytes())
        .map(|_| format!("sent to {}", key.name))
        .map_err(|e| format!("couldn't send to {}: {}", key.name, first_line(&e)))
}

/// The first line of what a command said, for the status line.
pub fn first_line(e: &anyhow::Error) -> String {
    let all = format!("{e:#}");
    all.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("it failed")
        .to_string()
}

/// The permission dialog up in the session's pane now; why not, for the status line.
pub fn approval(
    key: &SessionKey,
    pane: Option<&str>,
    local_name: &str,
    run: Runner,
) -> Result<Vec<String>, String> {
    let screen = run(&tmux::capture_argv(key, pane, local_name))
        .map_err(|e| format!("couldn't read {}: {}", key.name, first_line(&e)))?;
    approve::request(&screen).ok_or_else(|| format!("{} isn't asking anything", key.name))
}

/// Yes or No to the dialog `shown` — only if a fresh capture still shows it (D4).
pub fn answer(
    key: &SessionKey,
    pane: Option<&str>,
    shown: &[String],
    yes: bool,
    local_name: &str,
    run: Runner,
) -> Result<String, String> {
    let screen = run(&tmux::capture_argv(key, pane, local_name))
        .map_err(|e| format!("couldn't read {}: {}", key.name, first_line(&e)))?;
    if !approve::request(&screen).is_some_and(|now| approve::same(&now, shown)) {
        return Err(format!("{} isn't asking that any more", key.name));
    }
    let keys = if yes {
        approve::YES_KEY
    } else {
        approve::NO_KEY
    };
    run(&tmux::send_keys_argv(key, pane, keys, local_name))
        .map_err(|e| format!("couldn't answer {}: {}", key.name, first_line(&e)))?;
    Ok(match yes {
        true => format!("approved {}", key.name),
        false => format!("declined {}", key.name),
    })
}

/// What becomes of a permission dialog read for the approval form when it comes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalFate {
    /// The form opens.
    Open,
    /// A newer `y` asked again: this answer is dropped without a word, the newer one decides.
    Superseded,
    /// A key was pressed or the focus moved since the `y`: the form would take keys meant for
    /// something else — an Enter for the shell would answer Yes — so it does not open.
    MovedOn,
}

/// The fate of the dialog read for approval request `request` when `latest` is the newest
/// request, `keys_then` and `keys_now` count the key presses the app had handled at the `y` and
/// has now, and `focus_kept` says the keyboard focus is where it was after the `y`. The read
/// can take up to `REQUEST_TIMEOUT` on a slow host: the user may be typing elsewhere by then.
pub fn approval_fate(
    request: u64,
    latest: u64,
    keys_then: u64,
    keys_now: u64,
    focus_kept: bool,
) -> ApprovalFate {
    if request != latest {
        ApprovalFate::Superseded
    } else if keys_now != keys_then || !focus_kept {
        ApprovalFate::MovedOn
    } else {
        ApprovalFate::Open
    }
}

/// The command that opens `attach` in a new terminal window (tmls `launch_window`):
/// `$TERMINAL`, else kitty when it is installed, else `x-terminal-emulator`. kitty takes the
/// command as its arguments, the others after `-e`.
pub fn terminal_argv(
    terminal: Option<&str>,
    on_path: &dyn Fn(&str) -> bool,
    attach: &[String],
) -> Vec<String> {
    let terminal = match terminal.filter(|terminal| !terminal.is_empty()) {
        Some(terminal) => terminal,
        None if on_path("kitty") => "kitty",
        None => "x-terminal-emulator",
    };
    let mut argv = vec![terminal.to_string()];
    if terminal != "kitty" {
        argv.push("-e".to_string());
    }
    argv.extend(attach.iter().cloned());
    argv
}

/// Opens the session in a new terminal window through `spawn`, and says so for the status
/// line.
pub fn open_terminal(
    key: &SessionKey,
    local_name: &str,
    terminal: Option<&str>,
    on_path: &dyn Fn(&str) -> bool,
    spawn: &mut dyn FnMut(&[String]) -> std::io::Result<()>,
) -> Result<String, String> {
    let argv = terminal_argv(terminal, on_path, &tmux::attach_argv(key, local_name));
    match spawn(&argv) {
        Ok(()) => Ok(format!("opened {} in {}", key.name, argv[0])),
        Err(e) => Err(format!("couldn't start {}: {e}", argv[0])),
    }
}

/// Puts the session's attach command on the clipboard through `set`, and says so.
pub fn copy_attach(
    key: &SessionKey,
    local_name: &str,
    set: &mut dyn FnMut(String) -> Result<(), String>,
) -> Result<String, String> {
    match set(tmux::attach_text(key, local_name)) {
        Ok(()) => Ok(format!("copied the attach command of {}", key.name)),
        Err(e) => Err(format!("copy failed: {e}")),
    }
}

/// Whether `program` is an executable on `$PATH`.
pub fn on_path(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join(program))
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    })
}

/// Starts `argv` on its own: no terminal, no pipes, its own process group, so it outlives
/// this window. The app's spawner for `open_terminal`.
pub fn spawn_detached(argv: &[String]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    // Reaped when it ends, however long that takes.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
