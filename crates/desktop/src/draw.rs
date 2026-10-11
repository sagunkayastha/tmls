//! Where everything in the window is, and everything but the terminal's cells as quads.
//!
//! ```text
//! +-- sidebar (28 cols) --+s+-- tab bar (1 row) ---------------------+
//! | LAPTOP              + |e| ▌● name  × | ▌? name  × |              |
//! | ● C name ★            |p+----------------------------------------+
//! |     title         58% |a|  terminal: the grid of the tab in front|
//! | ○   name          ◉2  |r|                                        |
//! |                       |a|                                        |
//! | ● running  ? waiting  |t|                                        |
//! | ◆ done     ✕ error    |o+----------------------------------------+
//! | ?  !                 ✕ |r| status line (1 row)                    |
//! +-----------------------+-+----------------------------------------+
//! ```
//!
//! `layout` gives the rectangles in physical pixels, all on whole pixels (atlas texels are
//! copied 1:1), `Layout::hit` what is under a pixel. `Layout::terminal` is the origin of the
//! tabs' scenes and `Layout::grid` the size the sessions are resized to.
//!
//! **The sidebar** is tmls's (its pane mode): the session list on the cell grid, one line per
//! row and two for a session with a summary, then a legend of the marks and two rows of buttons
//! at the foot. Which rows are in view and which row a line belongs to is the model's
//! (`sidebar::Sidebar::view`, `row_at`); what a row says is `session_spans` and
//! `header_spans`, text on the list's columns that the tests read without a font.
//!
//! The quads are two layers of `gfx::grid::Instance`s around the terminal's own:
//! `[chrome.base, scene.bg, scene.glyphs, scene.overlay, chrome.top]` (`layer_order`) — `base`
//! is the sidebar, the tab bar and the status line, `top` the notice over the terminal of a tab
//! whose session ended and, over everything, the open form or menu (`forms::Panel`, placed by
//! `Layout::panel_rect`: a form in the middle of the window, a menu at the pointer). `Chrome` caches both: they are rebuilt when `invalidate` was called or
//! the atlas started a new generation, not on every frame, so a key echo pays nothing for them.
//! A rebuild sets the text of the rows in view only.
//!
//! Text is set on the atlas's cell grid and cut to the columns that fit (the renderer has no
//! scissor).

use std::borrow::Cow;

use client_core::{SessionInfo, SessionKey, Tool};
use s2_wgpu::gfx::atlas::{Atlas, CellMetrics, FontStyle};
use s2_wgpu::gfx::grid::{self, Instance, Scene, clusters, push_text, solid};
use s2_wgpu::ui::draw::fit;
pub use s2_wgpu::ui::layout::Rect;

use crate::forms::{Cell, Look, Panel, Stop, Target};
use crate::sidebar::{Row, Sidebar, summary};
use crate::tabs::Front;
use crate::theme::{
    ADD_RGB, ATTACHED_MARK, CLOSE_MARK, COPY_RGB, CURRENT_RGB, CURSOR_RGB, DANGER_RGB, LEGEND,
    OFFLINE, PIN, PIN_RGB, TAB_MAX_COLS, Theme, context_rgb, host_rgb, mark_glyph, mark_rgb,
    tool_letter,
};

/// What a tab whose session ended shows over its last screen.
pub const ENDED_NOTICE: &str = "session ended — Enter to reattach";

/// Cells of a tab that are not its name: the host bar's cell, the mark, the `×` and the spaces
/// between them — `▌● name × `.
const TAB_FIXED_COLS: usize = 6;
/// A tab narrower than this has no `×` (and no place to click to close it).
const TAB_CLOSE_MIN_COLS: usize = 4;
/// Cells at a tab's right end that close it when clicked: the `×` and one cell either side.
const TAB_CLOSE_COLS: usize = 3;
/// Cells at the right end of a host's header that are its `+` when clicked: the `+` and one
/// cell either side.
const ADD_COLS: usize = 3;
/// Lines of the legend, as in tmls: the marks and the ★ two a line, then the agents' letters
/// on two lines.
const LEGEND_LINES: usize = (LEGEND.len() + 1).div_ceil(2) + TOOL_LINES;
/// The legend's lines for the agents' letters: `C Claude  O opencode`, `X Codex`.
const TOOL_LINES: usize = 2;
/// The agents of the legend, in tmls's order.
const LEGEND_TOOLS: [(Tool, &str); 3] = [
    (Tool::Claude, "Claude"),
    (Tool::Opencode, "opencode"),
    (Tool::Codex, "Codex"),
];
/// Columns from one legend entry to the next: the mark, a space, the word padded to nine
/// (tmls line 96).
const LEGEND_ENTRY_COLS: usize = 11;
/// The legend and the buttons are left out when fewer lines than this would be the list's.
const MIN_LIST_LINES: usize = 4;

/// A button at the foot of the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// A new session on the selected session's host.
    New,
    /// Attach the selected session.
    Open,
    /// Copy the command that attaches the selected session.
    Copy,
    /// Opens the sketchpad hub in the browser; drawn only when one is configured.
    Sketch,
    /// The keys.
    Help,
    /// The bell: the list of alerts. Its label is followed by how many are new.
    Alerts,
    /// Quit (tmls's red ✕). The sessions keep running.
    Quit,
}

/// The bell. tmls draws 🔔, which the terminal's font need not have; this is in every font.
pub const BELL: &str = "!";

/// The buttons' two rows, each from left to right with the labels two columns apart: the
/// app's own, then what acts on a session. Quit is apart, at its row's right end.
pub const BUTTONS: [&[(Button, &str)]; 2] = [
    &[
        (Button::Help, "?"),
        (Button::Alerts, BELL),
        (Button::Quit, "✕"),
    ],
    &[
        (Button::New, "New"),
        (Button::Open, "Open"),
        (Button::Copy, "Copy"),
        (Button::Sketch, "Sketch"),
    ],
];

/// Each button with its label, its row, the column the label starts at and how many columns
/// are the button's: its label's, and for the bell two more for the count (at most
/// `MAX_ALERTS`).
fn button_cells(
    list_cols: usize,
) -> impl Iterator<Item = (Button, &'static str, usize, usize, usize)> {
    BUTTONS.iter().enumerate().flat_map(move |(row, buttons)| {
        buttons.iter().scan(1, move |col, (button, label)| {
            let cols = label.chars().count() + if *button == Button::Alerts { 2 } else { 0 };
            let start = match button {
                Button::Quit => list_cols.saturating_sub(2),
                _ => *col,
            };
            *col += cols + 2;
            Some((*button, *label, row, start, cols))
        })
    })
}

/// What the bell shows: `!`, or `!3` with three alerts not looked at yet.
pub fn bell_label(unread: usize) -> String {
    match unread {
        0 => BELL.to_string(),
        unread => format!("{BELL}{unread}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: (f32, f32),
    /// Cell width and height the layout was made for.
    pub cell: (f32, f32),
    pub sidebar: Rect,
    /// The line between the sidebar and the rest.
    pub separator: Rect,
    /// Pixels right of the separator that are still its handle for dragging: the padding
    /// left of the terminal's text, six logical pixels at most.
    pub grab: f32,
    /// The session list: `list_lines` lines of a cell's height.
    pub list: Rect,
    pub list_lines: usize,
    /// Columns of the sidebar. A row's text leaves one free at either end.
    pub list_cols: usize,
    /// The legend of the marks and, under it, the rows of buttons; both empty when the window
    /// is too low for them.
    pub legend: Rect,
    pub buttons: Rect,
    pub tab_bar: Rect,
    /// The grid's area; its corner is the grid's origin. Half a cell right of the separator,
    /// so the text does not touch it.
    pub terminal: Rect,
    pub status: Rect,
    /// Columns and rows of the terminal.
    pub grid: (u16, u16),
    /// Width of the host colour bar at a tab's left edge.
    pub host_bar: f32,
}

/// What is under a window pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The session list: the line, counted from the first one drawn (it may be empty), and
    /// the column.
    List {
        line: usize,
        col: usize,
    },
    /// A button at the foot of the sidebar.
    Button(Button),
    /// The sidebar, where nothing is to click.
    Sidebar,
    /// The line right of the list and a few pixels right of it: dragging it makes the list
    /// wider or narrower.
    Separator,
    /// A tab, anywhere but its `×`.
    Tab(usize),
    /// The `×` of a tab.
    TabClose(usize),
    /// The tab bar right of the last tab.
    TabBar,
    Terminal,
    Status,
    Outside,
}

/// Lays the window out. `window` is the surface size in physical pixels, `m` the cell box of
/// the atlas at the current scale and font size.
pub fn layout(window: (u32, u32), scale: f32, m: &CellMetrics, theme: &Theme) -> Layout {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let px = |logical: f32| (logical * scale).round().max(0.0);
    let (win_w, win_h) = (window.0 as f32, window.1 as f32);
    let (cell_w, cell_h) = (m.width.max(1.0), m.height.max(1.0));
    let inset = px(theme.inset);
    let line = px(theme.border).max(1.0);

    // The sidebar never takes more than half the window.
    let side_w = (theme.sidebar_cols as f32 * cell_w).min((win_w / 2.0).floor());
    let sep_w = line.min((win_w - side_w).max(0.0));
    let right_x = side_w + sep_w;
    let right_w = (win_w - right_x).max(0.0);

    // The tab bar and the status line never take more than a third of the height each.
    let bar_h = (cell_h + 2.0 * inset).min((win_h / 3.0).floor());
    // (tmls's terminal is a kitty window with kitty's own padding; half a cell here.)
    let pad = (cell_w / 2.0).round().min((right_w - cell_w).max(0.0));
    let terminal = Rect {
        x: right_x + pad,
        y: bar_h,
        w: right_w - pad,
        h: (win_h - 2.0 * bar_h).max(0.0),
    };
    // The foot of the sidebar: a line, the legend, the buttons. The buttons' last row is
    // level with the terminal's last row, not with the window's bottom edge: a desktop panel
    // that slides in from that edge would cover them. The space under them stays empty.
    let term_rows = (terminal.h / cell_h).floor().max(BUTTONS.len() as f32);
    let buttons_y = terminal.y + (term_rows - BUTTONS.len() as f32) * cell_h;
    let legend_y = buttons_y - LEGEND_LINES as f32 * cell_h;
    let foot = legend_y - line - inset >= MIN_LIST_LINES as f32 * cell_h;
    let across = Rect {
        x: 0.0,
        y: win_h,
        w: side_w,
        h: 0.0,
    };
    let (legend, buttons) = match foot {
        true => (
            Rect {
                y: legend_y,
                h: LEGEND_LINES as f32 * cell_h,
                ..across
            },
            Rect {
                y: buttons_y,
                h: BUTTONS.len() as f32 * cell_h,
                ..across
            },
        ),
        false => (across, across),
    };
    let list_bottom = if foot { legend_y - line } else { win_h };
    let list_lines = ((list_bottom - inset).max(0.0) / cell_h).floor() as usize;
    Layout {
        window: (win_w, win_h),
        cell: (cell_w, cell_h),
        sidebar: Rect {
            x: 0.0,
            y: 0.0,
            w: side_w,
            h: win_h,
        },
        // Not into the terminal's cells: the padding left of them at most.
        grab: px(6.0).min(pad),
        separator: Rect {
            x: side_w,
            y: 0.0,
            w: sep_w,
            h: win_h,
        },
        list: Rect {
            y: inset,
            h: list_lines as f32 * cell_h,
            ..across
        },
        list_lines,
        list_cols: (side_w / cell_w).floor() as usize,
        legend,
        buttons,
        tab_bar: Rect {
            x: right_x,
            y: 0.0,
            w: right_w,
            h: bar_h,
        },
        terminal,
        status: Rect {
            x: right_x,
            y: terminal.bottom(),
            w: right_w,
            h: bar_h,
        },
        grid: m.grid_for(terminal.w as u32, terminal.h as u32),
        host_bar: px(theme.host_bar).clamp(1.0, cell_w),
    }
}

impl Layout {
    /// Width of a tab in cells when `tabs` are open: they share the bar evenly, up to
    /// `TAB_MAX_COLS` each.
    pub fn tab_cols(&self, tabs: usize) -> usize {
        let bar_cols = (self.tab_bar.w / self.cell.0).floor() as usize;
        (bar_cols / tabs.max(1)).min(TAB_MAX_COLS)
    }

    /// The rectangle of tab `index` of `tabs`.
    pub fn tab_rect(&self, index: usize, tabs: usize) -> Rect {
        let w = self.tab_cols(tabs) as f32 * self.cell.0;
        Rect {
            x: self.tab_bar.x + index as f32 * w,
            w,
            ..self.tab_bar
        }
    }

    /// The rectangle of `lines` lines of the session list from line `line`.
    pub fn list_lines_rect(&self, line: usize, lines: usize) -> Rect {
        Rect {
            y: self.list.y + line as f32 * self.cell.1,
            h: lines as f32 * self.cell.1,
            ..self.list
        }
    }

    /// Where a panel of `cols` x `lines` cells goes: in the middle of the window, or with its
    /// corner at the pixel `at` — left of or above it where the window ends too soon. Never
    /// past the window's top-left corner.
    pub fn panel_rect(&self, cols: usize, lines: usize, at: Option<(u32, u32)>) -> Rect {
        let (w, h) = (cols as f32 * self.cell.0, lines as f32 * self.cell.1);
        let place = |at: Option<f32>, size: f32, limit: f32| {
            let at = match at {
                Some(at) if at + size > limit => at - size,
                Some(at) => at,
                None => (limit - size) / 2.0,
            };
            at.floor().max(0.0)
        };
        Rect {
            x: place(at.map(|at| at.0 as f32), w, self.window.0),
            y: place(at.map(|at| at.1 as f32), h, self.window.1),
            w,
            h,
        }
    }

    /// A window pixel in cells from the corner of the panel at `rect` (`Form::click`).
    pub fn panel_cell(&self, rect: Rect, x: f32, y: f32) -> (f32, f32) {
        ((x - rect.x) / self.cell.0, (y - rect.y) / self.cell.1)
    }

    /// Whether a click at this column of a host's header is on its `+`.
    pub fn on_add(&self, col: usize) -> bool {
        col + ADD_COLS >= self.list_cols
    }

    /// What is under a pixel while `tabs` tabs are open.
    pub fn hit(&self, x: f32, y: f32, tabs: usize) -> Hit {
        let window = Rect {
            x: 0.0,
            y: 0.0,
            w: self.window.0,
            h: self.window.1,
        };
        if !window.contains(x, y) {
            return Hit::Outside;
        }
        if x >= self.separator.x && x < self.separator.x + self.separator.w + self.grab {
            return Hit::Separator;
        }
        if x < self.tab_bar.x {
            let col = (x / self.cell.0).floor() as usize;
            if self.list.contains(x, y) {
                let line = ((y - self.list.y) / self.cell.1).floor() as usize;
                return Hit::List { line, col };
            }
            let row = ((y - self.buttons.y) / self.cell.1).floor() as usize;
            let button = button_cells(self.list_cols)
                .find(|(_, _, at, start, cols)| *at == row && (*start..start + cols).contains(&col))
                .filter(|_| self.buttons.contains(x, y));
            return button.map_or(Hit::Sidebar, |(button, ..)| Hit::Button(button));
        }
        // The terminal's rows, the padding at its left included.
        if y >= self.terminal.y && y < self.terminal.bottom() {
            return Hit::Terminal;
        }
        if !self.tab_bar.contains(x, y) {
            return Hit::Status;
        }
        let cols = self.tab_cols(tabs);
        let col = ((x - self.tab_bar.x) / self.cell.0).floor() as usize;
        if cols == 0 || col / cols >= tabs {
            return Hit::TabBar;
        }
        let index = col / cols;
        if cols >= TAB_CLOSE_MIN_COLS && col % cols >= cols - TAB_CLOSE_COLS {
            Hit::TabClose(index)
        } else {
            Hit::Tab(index)
        }
    }
}

/// One tab of the tab bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabView {
    /// The tab's session; its name is the tab's.
    pub key: SessionKey,
    /// The session's status mark and its colour; `None` while the listing does not know the
    /// session.
    pub mark: Option<(char, [u8; 3])>,
    /// The host's accent colour.
    pub host: [u8; 3],
    /// The session ended: the name is dimmed.
    pub ended: bool,
}

/// What `Chrome` draws besides the session list (that is the `Sidebar`'s), as the app last
/// put it together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub sidebar_focused: bool,
    pub tabs: Vec<TabView>,
    pub active: Option<usize>,
    pub status: String,
    /// Shown centred over the terminal (`ENDED_NOTICE`, or that no session is open).
    pub notice: Option<String>,
    /// The open form or menu, and the window pixel a menu was opened at.
    pub panel: Option<(Panel, Option<(u32, u32)>)>,
    /// A sketchpad hub is configured: the Sketch button is drawn.
    pub sketch: bool,
    /// Alerts not looked at yet, for the bell.
    pub unread: usize,
}

impl View {
    /// The session of the tab in front.
    fn front(&self) -> Option<&SessionKey> {
        Some(&self.tabs.get(self.active?)?.key)
    }
}

/// A piece of a row's text: where it starts on the list's columns, how many columns it may
/// take (longer text ends in "…"), and how it looks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span<'a> {
    /// 0, or 1 for a session's second line.
    pub line: usize,
    pub col: usize,
    pub cols: usize,
    pub text: Cow<'a, str>,
    pub colour: [u8; 3],
    pub bold: bool,
}

impl<'a> Span<'a> {
    fn new(line: usize, col: usize, cols: usize, text: impl Into<Cow<'a, str>>) -> Span<'a> {
        Span {
            line,
            col,
            cols,
            text: text.into(),
            colour: [0; 3],
            bold: false,
        }
    }

    /// One that takes the columns of its text.
    fn whole(line: usize, col: usize, text: impl Into<Cow<'a, str>>) -> Span<'a> {
        let text = text.into();
        Span::new(line, col, columns(&text), text)
    }

    fn colour(self, colour: [u8; 3]) -> Span<'a> {
        Span { colour, ..self }
    }

    fn bold(self) -> Span<'a> {
        Span { bold: true, ..self }
    }

    /// The text as drawn: cut to the span's columns.
    pub fn shown(&self) -> String {
        fit(&self.text, self.cols)
            .into_iter()
            .map(|(cluster, _)| cluster)
            .collect()
    }
}

/// Columns a text takes on the cell grid.
fn columns(text: &str) -> usize {
    clusters(text).iter().map(|(_, width)| width).sum()
}

/// What of a session's row does not come from its listing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowLook {
    pub pinned: bool,
    /// tmux clients attached besides this window's own tab.
    pub others: u32,
    /// Its tab is the one in front.
    pub active: bool,
}

/// A session's row on `cols` columns (tmls `pane_row_text`): `mark  tool-letter  name ★`,
/// then at the right end `◉N` when other clients are attached; under it, when there is a
/// summary, that in dim. How full Claude's context is ends the summary's line, or the name's
/// when there is no summary. The ★, the count and the percentage always fit; the name and the
/// summary are cut.
pub fn session_spans<'a>(
    info: &'a SessionInfo,
    look: RowLook,
    cols: usize,
    theme: &Theme,
) -> Vec<Span<'a>> {
    // One column of padding at either end (tmls line 437); the name starts under the summary.
    let (first, name_col) = (1, 5);
    let end = cols.saturating_sub(1);
    let mut out = Vec::with_capacity(7);
    out.push(
        Span::whole(0, first, mark_glyph(info.status))
            .colour(mark_rgb(info.status))
            .bold(),
    );
    if let Some((letter, colour)) = tool_letter(info.tool) {
        out.push(Span::whole(0, first + 2, letter).colour(colour).bold());
    }
    let summary = summary(info);
    let percent = info.context_pct.map(|pct| {
        let text = format!("{pct}%");
        let col = end.saturating_sub(text.len());
        let line = usize::from(summary.is_some());
        Span::whole(line, col, text).colour(context_rgb(pct, theme))
    });

    // The name's line, from the right: the percentage if it is on this line, the count.
    let mut right = end;
    if let Some(percent) = percent.as_ref().filter(|percent| percent.line == 0) {
        right = percent.col.saturating_sub(1);
    }
    let others = (look.others > 0).then(|| {
        let text = format!("{ATTACHED_MARK}{}", look.others);
        let col = right.saturating_sub(columns(&text));
        right = col.saturating_sub(1);
        Span::whole(0, col, text).colour(theme.muted_fg)
    });
    let pin_cols = if look.pinned { 2 } else { 0 };
    let name_cols = right.saturating_sub(name_col + pin_cols);
    let name = Span::new(0, name_col, name_cols, info.key.name.as_str());
    let pin_col = name_col + columns(&info.key.name).min(name_cols) + 1;
    out.push(match look.active {
        true => name.colour(CURRENT_RGB).bold(),
        false => name.colour(theme.sidebar_fg),
    });
    if look.pinned && pin_col < right {
        out.push(Span::whole(0, pin_col, PIN).colour(PIN_RGB).bold());
    }
    out.extend(others);

    if let Some((text, waiting)) = summary {
        // tmls keeps five columns for the percentage: four and a space before it.
        let taken = if percent.is_some() { 5 } else { 0 };
        let colour = match waiting {
            true => mark_rgb(client_core::Status::Waiting),
            false => theme.dim_fg,
        };
        let cols = end.saturating_sub(name_col + taken);
        out.push(Span::new(1, name_col, cols, text).colour(colour));
    }
    out.extend(percent);
    out
}

/// A host's header on `cols` columns: the name in capitals as tmls writes it (line 895), then
/// `+` at the right end, or `· offline` after the name of a host that does not answer, dimmed.
pub fn header_spans(host: &str, online: bool, cols: usize, theme: &Theme) -> Vec<Span<'static>> {
    let end = cols.saturating_sub(1);
    let name = host.to_uppercase();
    if online {
        let name_cols = end.saturating_sub(1 + 2);
        return vec![
            Span::new(0, 1, name_cols, name)
                .colour(theme.muted_fg)
                .bold(),
            Span::whole(0, end.saturating_sub(1), "+")
                .colour(ADD_RGB)
                .bold(),
        ];
    }
    let name_cols = end.saturating_sub(1 + columns(OFFLINE) + 1);
    let offline_col = 1 + columns(&name).min(name_cols) + 1;
    vec![
        Span::new(0, 1, name_cols, name).colour(theme.dim_fg).bold(),
        Span::whole(0, offline_col, OFFLINE).colour(theme.dim_fg),
    ]
}

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

/// Collects quads: rectangles and text on the cell grid.
struct Painter<'a> {
    out: &'a mut Vec<Instance>,
    atlas: &'a mut Atlas,
    m: CellMetrics,
}

impl Painter<'_> {
    fn rect(&mut self, r: Rect, colour: [u8; 3]) {
        if r.w > 0.0 && r.h > 0.0 {
            self.out.push(solid([r.x, r.y], [r.w, r.h], opaque(colour)));
        }
    }

    /// `text` from `(x, y)` (top-left of its first cell), cut to `cols` columns.
    fn label(&mut self, text: &str, x: f32, y: f32, cols: usize, colour: [u8; 3]) {
        self.styled(text, x, y, cols, colour, FontStyle::default());
    }

    fn styled(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        cols: usize,
        colour: [u8; 3],
        style: FontStyle,
    ) {
        for (cluster, col) in fit(text, cols) {
            let x = x + col as f32 * self.m.width;
            push_text(
                self.out,
                self.atlas,
                &cluster,
                style,
                x,
                y,
                &self.m,
                opaque(colour),
            );
        }
    }

    /// A span of a row whose first line's first column is at `origin`.
    fn span(&mut self, origin: (f32, f32), span: &Span) {
        let style = FontStyle {
            bold: span.bold,
            ..FontStyle::default()
        };
        let x = origin.0 + span.col as f32 * self.m.width;
        let y = origin.1 + span.line as f32 * self.m.height;
        self.styled(&span.text, x, y, span.cols, span.colour, style);
    }

    /// The y that centres one line of text in `r`.
    fn text_y(&self, r: Rect) -> f32 {
        r.y + ((r.h - self.m.height) / 2.0).floor().max(0.0)
    }
}

/// The cached quads of everything but the terminal.
#[derive(Default)]
pub struct Chrome {
    base: Vec<Instance>,
    top: Vec<Instance>,
    /// Atlas generation the quads were built in.
    built: Option<u64>,
    dirty: bool,
}

impl Chrome {
    pub fn new() -> Chrome {
        Chrome::default()
    }

    /// The view or the layout changed: rebuild at the next `update`.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Sidebar, tab bar and status line: drawn before the terminal.
    pub fn base(&self) -> &[Instance] {
        &self.base
    }

    /// The notice over the terminal: drawn after it.
    pub fn top(&self) -> &[Instance] {
        &self.top
    }

    /// Rebuilds the quads if needed; returns whether it did.
    pub fn update(
        &mut self,
        view: &View,
        list: &Sidebar,
        layout: &Layout,
        theme: &Theme,
        atlas: &mut Atlas,
    ) -> bool {
        if !self.dirty && self.built == Some(atlas.generation()) {
            return false;
        }
        // A second pass only if the atlas filled up and restarted during the first: quads
        // built before the restart then point at glyphs that are gone.
        for _pass in 0..2 {
            let generation = atlas.generation();
            self.base.clear();
            self.top.clear();
            let m = atlas.metrics();
            let mut p = Painter {
                out: &mut self.base,
                atlas,
                m,
            };
            sidebar(&mut p, view, list, layout, theme);
            tab_bar(&mut p, view, layout, theme);
            status_line(&mut p, view, layout, theme);
            p.out = &mut self.top;
            notice(&mut p, view, layout, theme);
            panel(&mut p, view, layout, theme);
            if atlas.generation() == generation {
                break;
            }
        }
        self.built = Some(atlas.generation());
        self.dirty = false;
        true
    }
}

fn sidebar(p: &mut Painter, view: &View, list: &Sidebar, l: &Layout, theme: &Theme) {
    p.rect(l.sidebar, theme.sidebar_bg);
    // The line beside the sidebar shows which side has the keyboard focus.
    let beside = if view.sidebar_focused {
        theme.focus_ring
    } else {
        theme.sidebar_rule
    };
    p.rect(l.separator, beside);
    let line_px = l.separator.w.max(1.0);
    let front = view.front();
    let selected = list.selected();
    let mut line = 0;
    for row in &list.rows()[list.view(l.list_lines)] {
        let r = l.list_lines_rect(line, row.lines());
        line += row.lines();
        match row {
            Row::Gap => {}
            Row::Host {
                host,
                online,
                color,
            } => {
                // tmls: a full-width line in the host's colour above each machine (line 908).
                p.rect(
                    Rect {
                        h: (2.0 * line_px).min(r.h),
                        ..r
                    },
                    host_rgb(*color),
                );
                for span in header_spans(host, *online, l.list_cols, theme) {
                    p.span((r.x, r.y), &span);
                }
            }
            Row::Session(info) => {
                let key = &info.key;
                let active = front == Some(key);
                if active {
                    p.rect(r, theme.row_active_bg);
                }
                if selected == Some(key) {
                    // The cursor's bar takes the padding's column: the text keeps its place.
                    let bar = Rect {
                        w: (p.m.width / 3.0).round().max(1.0).min(r.w),
                        ..r
                    };
                    p.rect(
                        bar,
                        match view.sidebar_focused {
                            true => CURSOR_RGB,
                            false => theme.cursor_idle,
                        },
                    );
                }
                // This window's own live tab is one of the attached clients.
                let own = view.tabs.iter().any(|tab| !tab.ended && tab.key == *key);
                let look = RowLook {
                    pinned: list.is_pinned(key),
                    others: info.attached.saturating_sub(u32::from(own)),
                    active,
                };
                for span in session_spans(info, look, l.list_cols, theme) {
                    p.span((r.x, r.y), &span);
                }
            }
        }
    }
    foot(p, view, l, theme);
}

/// The legend of the marks under a line, and the buttons.
fn foot(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    if l.legend.h <= 0.0 {
        return;
    }
    let line_px = l.separator.w.max(1.0);
    p.rect(
        Rect {
            y: l.legend.y - line_px,
            h: line_px,
            ..l.legend
        },
        theme.sidebar_rule,
    );
    let entry_cols = LEGEND_ENTRY_COLS.min(l.list_cols.saturating_sub(2) / 2);
    // The marks, then the ★, then the agents' letters from a line of their own on.
    let marks = LEGEND
        .iter()
        .map(|(status, word)| (mark_glyph(*status), mark_rgb(*status), *word))
        .chain([(PIN, PIN_RGB, "pinned")]);
    let tools_from = (LEGEND.len() + 1).div_ceil(2) * 2;
    let tools = LEGEND_TOOLS.iter().filter_map(|(tool, word)| {
        let (letter, colour) = tool_letter(*tool)?;
        Some((letter, colour, *word))
    });
    let entries = marks
        .enumerate()
        .chain(tools.enumerate().map(|(n, entry)| (tools_from + n, entry)));
    for (n, (mark, colour, word)) in entries {
        let col = 1 + (n % 2) * entry_cols;
        let origin = (l.legend.x, l.legend.y + (n / 2) as f32 * p.m.height);
        p.span(origin, &Span::whole(0, col, mark).colour(colour).bold());
        let word = Span::new(0, col + 2, entry_cols.saturating_sub(3), word);
        p.span(origin, &word.colour(theme.muted_fg));
    }
    for (button, label, row, col, cols) in button_cells(l.list_cols) {
        if button == Button::Sketch && !view.sketch {
            continue;
        }
        let colour = match button {
            Button::New | Button::Open => ADD_RGB,
            Button::Copy => COPY_RGB,
            // tmls's "warning" button.
            Button::Sketch => PIN_RGB,
            Button::Help => theme.muted_fg,
            // tmls's accent while something is new.
            Button::Alerts if view.unread > 0 => CURSOR_RGB,
            Button::Alerts => theme.muted_fg,
            Button::Quit => DANGER_RGB,
        };
        let fits = col + cols < l.list_cols;
        let label = match button {
            Button::Alerts => Cow::Owned(bell_label(view.unread)),
            _ => Cow::Borrowed(label),
        };
        let span = Span::whole(0, col, label).colour(colour).bold();
        if fits {
            p.span((l.buttons.x, l.buttons.y + row as f32 * p.m.height), &span);
        }
    }
}

fn tab_bar(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    p.rect(l.tab_bar, theme.bar_bg);
    let tabs = view.tabs.len();
    let cols = l.tab_cols(tabs);
    let cell_w = p.m.width;
    let line = l.separator.w.max(1.0);
    for (index, tab) in view.tabs.iter().enumerate() {
        let r = l.tab_rect(index, tabs);
        let active = view.active == Some(index);
        if active {
            p.rect(r, theme.tab_active_bg);
        }
        p.rect(
            Rect {
                w: l.host_bar.min(r.w),
                ..r
            },
            tab.host,
        );
        p.rect(
            Rect {
                x: r.right() - line,
                w: line.min(r.w),
                ..r
            },
            theme.separator,
        );
        let y = p.text_y(r);
        let at = |col: usize| r.x + col as f32 * cell_w;
        if let Some((mark, colour)) = tab.mark.filter(|_| cols >= 3) {
            p.label(mark.encode_utf8(&mut [0; 4]), at(1), y, 1, colour);
        }
        let name_colour = match (active, tab.ended) {
            (true, false) => theme.tab_active_fg,
            (false, false) => theme.sidebar_fg,
            (_, true) => theme.muted_fg,
        };
        p.label(
            &tab.key.name,
            at(3),
            y,
            cols.saturating_sub(TAB_FIXED_COLS),
            name_colour,
        );
        if cols >= TAB_CLOSE_MIN_COLS {
            p.label(CLOSE_MARK, at(cols - 2), y, 1, theme.muted_fg);
        }
    }
}

fn status_line(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    p.rect(l.status, theme.bar_bg);
    let cols = ((l.status.w / p.m.width).floor() as usize).saturating_sub(2);
    if l.status.h >= p.m.height {
        let y = p.text_y(l.status);
        p.label(
            &view.status,
            l.status.x + p.m.width,
            y,
            cols,
            theme.muted_fg,
        );
    }
}

/// The notice, centred in the terminal's area on a band of its own colour.
fn notice(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    let Some(text) = &view.notice else {
        return;
    };
    let t = l.terminal;
    let cols = (t.w / p.m.width).floor() as usize;
    let used = fit(text, cols.saturating_sub(4))
        .last()
        .map_or(0, |(_, col)| col + 1);
    if used == 0 || t.h < 3.0 * p.m.height {
        return;
    }
    let w = (used + 4) as f32 * p.m.width;
    let band = Rect {
        x: t.x + ((t.w - w) / 2.0).floor().max(0.0),
        y: t.y + ((t.h - 3.0 * p.m.height) / 2.0).floor(),
        w,
        h: 3.0 * p.m.height,
    };
    p.rect(band, theme.notice_bg);
    p.label(
        text,
        band.x + 2.0 * p.m.width,
        band.y + p.m.height,
        used,
        theme.notice_fg,
    );
}

/// The open form or menu: a bordered box over everything, its pieces on the cell grid.
fn panel(p: &mut Painter, view: &View, l: &Layout, theme: &Theme) {
    let Some((panel, at)) = &view.panel else {
        return;
    };
    let r = l.panel_rect(panel.cols, panel.lines, *at);
    let line = l.separator.w.max(1.0);
    // tmls: `border: round $accent; background: $panel`.
    p.rect(
        Rect {
            x: r.x - line,
            y: r.y - line,
            w: r.w + 2.0 * line,
            h: r.h + 2.0 * line,
        },
        CURSOR_RGB,
    );
    p.rect(r, theme.panel_bg);
    for cell in &panel.cells {
        panel_cell(p, cell, r, line, theme);
    }
}

/// One piece of a panel whose corner is at `r`.
fn panel_cell(p: &mut Painter, cell: &Cell, r: Rect, line: f32, theme: &Theme) {
    let (cw, ch) = (p.m.width, p.m.height);
    let at = Rect {
        x: r.x + cell.col as f32 * cw,
        y: r.y + cell.line as f32 * ch,
        w: cell.cols as f32 * cw,
        h: ch,
    };
    let plain = FontStyle::default();
    let bold = FontStyle {
        bold: true,
        ..plain
    };
    let (bg, fg, style) = match cell.look {
        Look::Title => (None, CURSOR_RGB, bold),
        Look::Label => (None, theme.muted_fg, plain),
        Look::Text => (None, theme.sidebar_fg, plain),
        Look::Dim => (None, theme.dim_fg, plain),
        Look::Error => (None, DANGER_RGB, plain),
        Look::Field { .. } => (Some(theme.field_bg), theme.sidebar_fg, plain),
        Look::Choice { focused: true, .. } => (Some(theme.row_selected_bg), CURSOR_RGB, bold),
        Look::Choice { on: true, .. } => (None, theme.sidebar_fg, plain),
        Look::Choice { .. } => (None, theme.muted_fg, plain),
        Look::Button { focused, danger } => {
            let cancel = cell.target == Some(Target::Stop(Stop::Cancel));
            let colour = match (cancel, danger) {
                (true, _) => theme.sidebar_fg,
                (false, true) => DANGER_RGB,
                (false, false) => ADD_RGB,
            };
            match focused {
                true => (Some(colour), theme.sidebar_bg, bold),
                false => (Some(theme.button_bg), colour, plain),
            }
        }
        Look::Item { hovered, danger } => {
            let fg = if danger { DANGER_RGB } else { theme.sidebar_fg };
            (hovered.then_some(theme.row_selected_bg), fg, plain)
        }
    };
    if let Some(bg) = bg {
        p.rect(at, bg);
    }
    if let Some((from, to)) = cell.selection {
        let from = from.min(cell.cols);
        let selected = Rect {
            x: at.x + from as f32 * cw,
            w: to.min(cell.cols).saturating_sub(from) as f32 * cw,
            ..at
        };
        p.rect(selected, theme.row_selected_bg);
    }
    if let Look::Field { focused: true } = cell.look {
        // The focused field is underlined in the accent colour.
        let under = Rect {
            y: at.bottom() - line,
            h: line,
            ..at
        };
        p.rect(under, CURSOR_RGB);
    }
    p.styled(&cell.text, at.x, at.y, cell.cols, fg, style);
    if let Some(col) = cell.caret {
        let caret = Rect {
            x: at.x + col.min(cell.cols) as f32 * cw,
            w: (2.0 * line).min(cw),
            ..at
        };
        p.rect(caret, theme.sidebar_fg);
    }
}

/// The draw order for `Renderer::prepare`: the chrome under the terminal's three layers, the
/// notice over them. `scene` is the scene of the tab in front, if there is one to draw.
pub fn layer_order<'a>(scene: Option<&'a Scene>, chrome: &'a Chrome) -> [&'a [Instance]; 5] {
    let [bg, glyphs, overlay] = scene.map_or([&[][..]; 3], Scene::layers);
    [chrome.base(), bg, glyphs, overlay, chrome.top()]
}

/// Everything drawn besides new cells, for one render: the overlays of the tab in front
/// (cursor, preedit, selection) and the chrome.
///
/// `refeed`: no new frame was taken for this render, so the front tab's newest frame goes
/// through `Scene::update_cells` again — that picks up a new origin, scale factor or font size.
///
/// The overlays and the chrome both draw glyphs, and either can fill the atlas and restart
/// it, which voids the other's quads and the cells'. `Scene::update_overlays` rebuilds the
/// cells when it finds them stale and `Chrome::update` rebuilds on a new generation, so the two
/// are repeated until the chrome was built in the atlas's current generation.
#[allow(clippy::too_many_arguments)]
pub fn compose(
    front: Option<Front<'_>>,
    refeed: bool,
    atlas: &mut Atlas,
    theme: &Theme,
    chrome: &mut Chrome,
    view: &View,
    list: &Sidebar,
    layout: &Layout,
) {
    let Some(front) = front else {
        chrome.update(view, list, layout, theme, atlas);
        return;
    };
    if refeed {
        front
            .scene
            .update_cells(front.frame, atlas, &theme.terminal);
    }
    let overlays: grid::Overlays = front.session.overlays();
    for _round in 0..3 {
        chrome.update(view, list, layout, theme, atlas);
        front
            .scene
            .update_overlays(front.frame, atlas, &theme.terminal, &overlays);
        if chrome.built == Some(atlas.generation()) {
            break;
        }
    }
}
