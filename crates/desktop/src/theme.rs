//! Colours and sizes. What tmls defines is taken from tmls (the source line is cited); the
//! rest goes with the terminal's dark default (`s2_wgpu::gfx::grid::Theme`).
//!
//! "tmls" is `src/tmls/app.py` on its branch `feat/native-attach` unless another file is named.
//! Where its CSS names a variable of Textual's instead of a colour (`$accent`, `$boost`, …) the
//! value is the one Textual's default dark theme gives it, computed with the Textual that tmls
//! runs on (`textual-dark`: background #121212, foreground #e0e0e0, primary #0178d4,
//! primary-darken-2 #0053aa, accent #fea62b, success #4ebf71, boost = white at 4 %).
//!
//! Sizes are logical pixels or cells; `draw::layout` multiplies the pixels by the window's
//! scale factor. Colours are sRGB bytes. One font for everything: the atlas rasterises at a
//! single size, so the sidebar and the tab bar are set in the terminal's font and zoom with it.

use client_core::{HostColor, Status, Tool};
use s2_wgpu::gfx::atlas::FontSpec;
use s2_wgpu::gfx::grid;

/// Width of the sidebar in cells (tmls `app.py`: `#left { width: 28; … }`).
pub const SIDEBAR_COLS: usize = 28;

/// The widest a tab gets, in cells; tabs shrink evenly when the bar is full.
pub const TAB_MAX_COLS: usize = 24;

/// The mark of a session's close button in the tab bar.
pub const CLOSE_MARK: &str = "×";

/// Text that is `dim` in tmls (the idle mark, a row's second line, a low context %): the
/// terminal's faint attribute, here the foreground at half strength over the background.
pub const DIM_RGB: [u8; 3] = [0x79, 0x79, 0x79];
/// The bar at the left of the list's cursor row (tmls line 439: `border-left: outer $accent`).
pub const CURSOR_RGB: [u8; 3] = [0xfe, 0xa6, 0x2b];
/// The name of the session whose tab is in front (tmls line 392: `SessionRow.current`).
pub const CURRENT_RGB: [u8; 3] = [0xff, 0x8c, 0x00];
/// The `+` of a host's header and the New and Open buttons (tmls line 387: `AddSession`,
/// `$success`; line 533: `variant="success"`).
pub const ADD_RGB: [u8; 3] = [0x4e, 0xbf, 0x71];
/// The Copy button (tmls line 534: `variant="primary"`, `$primary`).
pub const COPY_RGB: [u8; 3] = [0x01, 0x78, 0xd4];
/// What is wrong or ends something: a form's error line, the Kill button (tmls `$error`;
/// the colour of the error mark).
pub const DANGER_RGB: [u8; 3] = [0xe0, 0x6c, 0x75];
/// The mark after a pinned session's name and its colour (tmls line 64: `PIN`).
pub const PIN: &str = "★";
pub const PIN_RGB: [u8; 3] = [0xe5, 0xc0, 0x7b];
/// Before the number of other clients attached to a session (tmls line 144).
pub const ATTACHED_MARK: &str = "◉";
/// After a session's name when it has news the user has not looked at (`Seen::unread`), and
/// its colour: one dot, no animation (D7).
pub const UNREAD_MARK: &str = "•";
pub const UNREAD_RGB: [u8; 3] = [0x56, 0xb6, 0xc2];
/// After a session's name while messages wait for it (`3✉`), and its colour.
pub const QUEUED_MARK: &str = "✉";
pub const QUEUED_RGB: [u8; 3] = [0xc6, 0x78, 0xdd];
/// After a host's name when it does not answer (tmls line 893).
pub const OFFLINE: &str = "· offline";
/// The legend's words, in tmls's order (lines 92–93); tmls's `failed` is `Status::Error`.
pub const LEGEND: [(Status, &str); 5] = [
    (Status::Running, "running"),
    (Status::Waiting, "waiting"),
    (Status::Unseen, "done"),
    (Status::Error, "error"),
    (Status::Idle, "idle"),
];

/// The narrowest the list can be dragged to: its legend and buttons are made for 20 columns
/// up (tmls line 90).
pub const SIDEBAR_MIN_COLS: usize = 20;

/// The narrowest the file panel can be dragged to, and the fewest columns it leaves the
/// terminal: a window that cannot give both this many has no panel.
pub const PANEL_MIN_COLS: usize = 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Columns of the session list: `SIDEBAR_COLS` until its right edge is dragged. Never
    /// more than half the window (`draw::layout`).
    pub sidebar_cols: usize,
    /// The terminal's own colours; `terminal.bg` clears the window.
    pub terminal: grid::Theme,
    /// Family and logical size at zoom level zero.
    pub font: FontSpec,
    /// Textual's `$background` and `$foreground`: what tmls's sidebar is drawn on and in.
    pub sidebar_bg: [u8; 3],
    pub sidebar_fg: [u8; 3],
    /// See `DIM_RGB`.
    pub dim_fg: [u8; 3],
    /// The line beside the list and the one above its legend (tmls lines 383 and 447:
    /// `solid $primary-darken-2`).
    pub sidebar_rule: [u8; 3],
    /// Behind the row of the session whose tab is in front (tmls line 392: `$boost`).
    pub row_active_bg: [u8; 3],
    /// The cursor's bar while the list does not have the keys. (tmls shows none then.)
    pub cursor_idle: [u8; 3],
    /// Text that matters less: a host's name, the legend, an inactive tab's name, the status
    /// line (tmls `$text-muted`: the foreground at 60 %).
    pub muted_fg: [u8; 3],
    /// Lines between areas.
    pub separator: [u8; 3],
    /// The line beside whatever has the keyboard focus, and the list's cursor row.
    pub focus_ring: [u8; 3],
    pub row_selected_bg: [u8; 3],
    pub row_selected_fg: [u8; 3],
    pub bar_bg: [u8; 3],
    /// The tab in front; the terminal's background, so tab and terminal read as one.
    pub tab_active_bg: [u8; 3],
    pub tab_active_fg: [u8; 3],
    /// Background of the "session ended" notice over the terminal.
    pub notice_bg: [u8; 3],
    pub notice_fg: [u8; 3],
    /// Behind a form or a menu (tmls `$panel`), behind a text field in it, and behind a button
    /// that does not have the focus.
    pub panel_bg: [u8; 3],
    pub field_bg: [u8; 3],
    pub button_bg: [u8; 3],
    /// Space above and below the text of a row, the tab bar and the status line.
    pub inset: f32,
    /// Thickness of lines.
    pub border: f32,
    /// Width of the host colour bar at a tab's left edge.
    pub host_bar: f32,
}

impl Default for Theme {
    fn default() -> Self {
        let terminal = grid::Theme::default();
        Theme {
            sidebar_cols: SIDEBAR_COLS,
            terminal,
            font: FontSpec::default(),
            sidebar_bg: [0x12, 0x12, 0x12],
            sidebar_fg: [0xe0, 0xe0, 0xe0],
            dim_fg: DIM_RGB,
            sidebar_rule: [0x00, 0x53, 0xaa],
            row_active_bg: [0x1b, 0x1b, 0x1b],
            cursor_idle: [0x4a, 0x4a, 0x4a],
            muted_fg: [0x8e, 0x8e, 0x8e],
            separator: [0x34, 0x36, 0x42],
            focus_ring: [0x5a, 0x9c, 0xf0],
            row_selected_bg: [0x2c, 0x3a, 0x5c],
            row_selected_fg: [0xf0, 0xf0, 0xf4],
            bar_bg: [0x1a, 0x1b, 0x22],
            tab_active_bg: terminal.bg,
            tab_active_fg: [0xf0, 0xf0, 0xf4],
            notice_bg: [0x2a, 0x2c, 0x38],
            notice_fg: [0xf0, 0xf0, 0xf4],
            panel_bg: [0x24, 0x26, 0x30],
            field_bg: [0x12, 0x12, 0x12],
            button_bg: [0x34, 0x36, 0x42],
            inset: 3.0,
            border: 1.0,
            host_bar: 3.0,
        }
    }
}

/// A host's accent colour (tmls `host_colors.py`, `PALETTE`, lines 11–16).
pub fn host_rgb(color: HostColor) -> [u8; 3] {
    match color {
        HostColor::Blue => [0x44, 0x68, 0xb0],
        HostColor::Teal => [0x0b, 0x7d, 0x91],
        HostColor::Violet => [0x6f, 0x50, 0xbc],
        HostColor::Green => [0x2e, 0x7d, 0x5b],
        HostColor::Amber => [0x9c, 0x5c, 0x20],
        HostColor::Rose => [0xa6, 0x4f, 0x75],
    }
}

/// The colour of a status mark (tmls `MARKS`, lines 51–53; idle is `dim`).
pub fn mark_rgb(status: Status) -> [u8; 3] {
    match status {
        Status::Running => [0x4e, 0xbf, 0x71],
        Status::Waiting => [0xe5, 0xc0, 0x7b],
        Status::Unseen => [0xff, 0x8c, 0x00],
        Status::Error => [0xe0, 0x6c, 0x75],
        Status::Idle => DIM_RGB,
    }
}

/// A status mark as text (tmls `MARKS`, lines 51–53): `Status::mark`, for drawing.
pub fn mark_glyph(status: Status) -> &'static str {
    match status {
        Status::Running => "●",
        Status::Waiting => "?",
        Status::Unseen => "◆",
        Status::Error => "✕",
        Status::Idle => "○",
    }
}

/// The letter of the agent CLI a session runs and its colour (tmls `TOOL_LETTER`, lines
/// 66–67); none for a shell.
pub fn tool_letter(tool: Tool) -> Option<(&'static str, [u8; 3])> {
    match tool {
        Tool::Claude => Some(("C", [0xd9, 0x77, 0x57])),
        Tool::Opencode => Some(("O", [0x61, 0xaf, 0xef])),
        Tool::Codex => Some(("X", [0x98, 0xc3, 0x79])),
        // another agent herdr knows: the first letter of its id, grey
        Tool::Agent(_) => {
            const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
            let i = LETTERS.find(tool.letter()?)?;
            Some((&LETTERS[i..i + 1], [0xab, 0xb2, 0xbf]))
        }
        Tool::Other => None,
    }
}

/// The colour of "how full Claude's context is" (tmls `fill_style`, lines 58–59): red from
/// 90 %, orange from 70 %, dim below.
pub fn context_rgb(pct: u8, theme: &Theme) -> [u8; 3] {
    match pct {
        90.. => [0xe0, 0x6c, 0x75],
        70.. => [0xff, 0x8c, 0x00],
        _ => theme.dim_fg,
    }
}
