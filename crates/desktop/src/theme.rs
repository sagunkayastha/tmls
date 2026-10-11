//! Colours and sizes. What tmls defines is taken from tmls (the source line is cited); the
//! rest goes with the terminal's dark default (`s2_wgpu::gfx::grid::Theme`).
//!
//! Sizes are logical pixels or cells; `draw::layout` multiplies the pixels by the window's
//! scale factor. Colours are sRGB bytes. One font for everything: the atlas rasterises at a
//! single size, so the sidebar and the tab bar are set in the terminal's font and zoom with it.

use client_core::{HostColor, Status};
use s2_wgpu::gfx::atlas::FontSpec;
use s2_wgpu::gfx::grid;

/// Width of the sidebar in cells (tmls `app.py`: `#left { width: 28; … }`).
pub const SIDEBAR_COLS: usize = 28;

/// The widest a tab gets, in cells; tabs shrink evenly when the bar is full.
pub const TAB_MAX_COLS: usize = 24;

/// The mark of a session's close button in the tab bar.
pub const CLOSE_MARK: &str = "×";

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The terminal's own colours; `terminal.bg` clears the window.
    pub terminal: grid::Theme,
    /// Family and logical size at zoom level zero.
    pub font: FontSpec,
    pub sidebar_bg: [u8; 3],
    pub sidebar_fg: [u8; 3],
    /// Text that matters less: an inactive tab's name, the status line.
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
            terminal,
            font: FontSpec::default(),
            sidebar_bg: [0x1a, 0x1b, 0x22],
            sidebar_fg: [0xc8, 0xc8, 0xd0],
            muted_fg: [0x80, 0x84, 0x94],
            separator: [0x34, 0x36, 0x42],
            focus_ring: [0x5a, 0x9c, 0xf0],
            row_selected_bg: [0x2c, 0x3a, 0x5c],
            row_selected_fg: [0xf0, 0xf0, 0xf4],
            bar_bg: [0x1a, 0x1b, 0x22],
            tab_active_bg: terminal.bg,
            tab_active_fg: [0xf0, 0xf0, 0xf4],
            notice_bg: [0x2a, 0x2c, 0x38],
            notice_fg: [0xf0, 0xf0, 0xf4],
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

/// The colour of a status mark (tmls `app.py`, `MARKS`, lines 47–49; idle is tmls's "dim").
pub fn mark_rgb(status: Status) -> [u8; 3] {
    match status {
        Status::Running => [0x4e, 0xbf, 0x71],
        Status::Waiting => [0xe5, 0xc0, 0x7b],
        Status::Unseen => [0xff, 0x8c, 0x00],
        Status::Error => [0xe0, 0x6c, 0x75],
        Status::Idle => [0x80, 0x84, 0x94],
    }
}
