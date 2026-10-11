//! Theme tokens: every colour, size and font the UI draws with, in one place, and the zoom
//! steps of the font size.
//!
//! Sizes are logical pixels; `layout` multiplies them by the window's scale factor and rounds
//! to whole physical pixels. Colours are sRGB bytes, as everywhere in `gfx`.
//!
//! One font for everything: the atlas rasterises at a single size, so sidebar text is set in
//! the terminal's font and zooms with it (see `docs/spikes/S2-checklist-wgpu.md`).

use crate::gfx::atlas::FontSpec;
use crate::gfx::grid::Theme;

#[derive(Clone, Debug, PartialEq)]
pub struct UiTheme {
    /// The terminal's own colours (`gfx::grid`); `terminal.bg` clears the window.
    pub terminal: Theme,
    /// Family and logical size at zoom level zero.
    pub font: FontSpec,
    pub sidebar_bg: [u8; 3],
    pub sidebar_fg: [u8; 3],
    /// Captions: the form's label.
    pub muted_fg: [u8; 3],
    /// Lines between areas and the outline of a widget without focus.
    pub separator: [u8; 3],
    pub item_selected_bg: [u8; 3],
    pub item_selected_fg: [u8; 3],
    /// Outline of the widget with the keyboard focus (for the terminal: its left edge).
    pub focus_ring: [u8; 3],
    pub field_bg: [u8; 3],
    pub field_fg: [u8; 3],
    /// Drawn over the text, so translucent.
    pub field_selection: [u8; 4],
    pub caret: [u8; 3],
    pub button_bg: [u8; 3],
    pub button_fg: [u8; 3],
    pub menu_bg: [u8; 3],
    pub menu_fg: [u8; 3],
    pub menu_border: [u8; 3],
    pub menu_hover_bg: [u8; 3],
    /// Width of the sidebar.
    pub sidebar_width: f32,
    /// Space between the sidebar's edge and its content, and between its parts.
    pub padding: f32,
    /// Space between a widget's edge and its text.
    pub inset: f32,
    /// Thickness of lines and outlines.
    pub border: f32,
}

impl Default for UiTheme {
    /// Dark, to go with `gfx::grid::Theme::default()`.
    fn default() -> Self {
        UiTheme {
            terminal: Theme::default(),
            font: FontSpec::default(),
            sidebar_bg: [0x1a, 0x1b, 0x22],
            sidebar_fg: [0xc8, 0xc8, 0xd0],
            muted_fg: [0x80, 0x84, 0x94],
            separator: [0x34, 0x36, 0x42],
            item_selected_bg: [0x2c, 0x3a, 0x5c],
            item_selected_fg: [0xf0, 0xf0, 0xf4],
            focus_ring: [0x5a, 0x9c, 0xf0],
            field_bg: [0x0c, 0x0c, 0x10],
            field_fg: [0xe4, 0xe4, 0xe8],
            field_selection: [0x40, 0x70, 0xc0, 0x90],
            caret: [0xe0, 0xc0, 0x40],
            button_bg: [0x2a, 0x2c, 0x38],
            button_fg: [0xd4, 0xd4, 0xdc],
            menu_bg: [0x24, 0x25, 0x2e],
            menu_fg: [0xdc, 0xdc, 0xe2],
            menu_border: [0x4a, 0x4c, 0x5a],
            menu_hover_bg: [0x35, 0x48, 0x74],
            sidebar_width: 240.0,
            padding: 8.0,
            inset: 4.0,
            border: 1.0,
        }
    }
}

/// Smallest and largest logical font size zoom reaches, and the size of one step.
pub const ZOOM_MIN: f32 = 6.0;
pub const ZOOM_MAX: f32 = 48.0;
pub const ZOOM_STEP: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomDir {
    /// Ctrl+= : larger.
    In,
    /// Ctrl+− : smaller.
    Out,
}

/// The logical font size one zoom step from `size`: the next whole multiple of `ZOOM_STEP` in
/// that direction, within `ZOOM_MIN..=ZOOM_MAX`. Whole steps, so in-then-out returns exactly.
pub fn zoom_step(size: f32, dir: ZoomDir) -> f32 {
    let size = if size.is_finite() {
        size
    } else {
        FontSpec::default().size
    };
    let steps = size / ZOOM_STEP;
    let next = match dir {
        ZoomDir::In => steps.floor() + 1.0,
        ZoomDir::Out => steps.ceil() - 1.0,
    };
    (next * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX)
}
