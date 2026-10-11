//! Where everything is, in physical pixels: the sidebar with its list and form, and the
//! terminal beside it. Recomputed when the window size, the scale factor or the font size
//! (zoom) changes.
//!
//! Every rectangle is on whole pixels at any scale factor — atlas texels are copied 1:1, so
//! text between pixels would be resampled. `terminal` gives `Scene::set_origin` its offset and
//! `grid` the size for `InputMsg::Resize`.
//!
//! ```text
//! +-- sidebar ---------+s+-- terminal -------------
//! | list row           |e|
//! | list row           |p|
//! | …                  |a|
//! |--- form_rule ------|r|
//! | form_label         |a|
//! | [ field          ] |t|
//! | [Create] [Cancel]  |o|
//! +--------------------+r+-------------------------
//! ```

use super::theme::UiTheme;
use crate::gfx::atlas::CellMetrics;

/// Label of the form and of its buttons (the buttons are sized for them).
pub const FORM_LABEL: &str = "New session";
pub const CREATE_LABEL: &str = "Create";
pub const CANCEL_LABEL: &str = "Cancel";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// Left and top edges are inside, right and bottom are not.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: (f32, f32),
    /// Cell width and height the layout was made for.
    pub cell: (f32, f32),
    pub sidebar: Rect,
    /// The line between sidebar and terminal.
    pub separator: Rect,
    /// The rows of the list; `visible_rows` of height `row_h` fit.
    pub list: Rect,
    pub row_h: f32,
    pub visible_rows: usize,
    /// Columns a list label may take.
    pub label_cols: usize,
    /// The line above the form.
    pub form_rule: Rect,
    pub form_label: Rect,
    pub field: Rect,
    /// The text area inside the field: `field_cols` whole cells.
    pub field_text: Rect,
    pub field_cols: usize,
    pub create: Rect,
    pub cancel: Rect,
    /// Everything right of the separator; its corner is the grid's origin.
    pub terminal: Rect,
    /// Columns and rows of the terminal.
    pub grid: (u16, u16),
}

/// What is under a window pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A row of the list, counted from the first visible one (there may be no item in it).
    ListRow(usize),
    /// The field; `col` is the caret position nearest to the pixel, in visible columns.
    Field {
        col: usize,
    },
    Create,
    Cancel,
    /// The sidebar or the separator, where nothing is.
    Sidebar,
    Terminal,
    Outside,
}

/// Lays the window out. `window` is the surface size in physical pixels, `m` the cell box of
/// the atlas at the current scale and font size.
pub fn layout(window: (u32, u32), scale: f32, m: &CellMetrics, theme: &UiTheme) -> Layout {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let px = |logical: f32| (logical * scale).round().max(0.0);
    let (win_w, win_h) = (window.0 as f32, window.1 as f32);
    let (cell_w, cell_h) = (m.width.max(1.0), m.height.max(1.0));
    let pad = px(theme.padding);
    let inset = px(theme.inset);
    let line = px(theme.border).max(1.0);

    // The sidebar never takes more than half the window.
    let side_w = px(theme.sidebar_width).min((win_w / 2.0).floor());
    let sep_w = line.min((win_w - side_w).max(0.0));
    let term_x = side_w + sep_w;
    let terminal = Rect {
        x: term_x,
        y: 0.0,
        w: (win_w - term_x).max(0.0),
        h: win_h,
    };

    // The form, from the bottom up; then the list gets what is left above it.
    let inner_w = (side_w - 2.0 * pad).max(0.0);
    let boxed_h = cell_h + 2.0 * inset;
    let button_y = (win_h - pad - boxed_h).max(0.0);
    let field_y = (button_y - pad - boxed_h).max(0.0);
    let label_y = (field_y - inset - cell_h).max(0.0);
    let rule_y = (label_y - pad - line).max(0.0);
    let button_w = |label: &str| {
        let wanted = (label.chars().count() + 2) as f32 * cell_w;
        wanted.min(((inner_w - pad) / 2.0).floor().max(0.0))
    };
    let create = Rect {
        x: pad.min(side_w),
        y: button_y,
        w: button_w(CREATE_LABEL),
        h: boxed_h.min(win_h - button_y),
    };
    let cancel = Rect {
        x: (create.right() + pad).min(side_w),
        w: button_w(CANCEL_LABEL),
        ..create
    };
    let field = Rect {
        x: pad.min(side_w),
        y: field_y,
        w: inner_w,
        h: boxed_h.min((button_y - field_y).max(0.0)),
    };
    let field_cols = ((field.w - 2.0 * inset) / cell_w).floor().max(0.0) as usize;
    let field_text = Rect {
        x: field.x + inset.min(field.w),
        y: field.y + inset.min(field.h),
        w: field_cols as f32 * cell_w,
        h: cell_h.min((field.h - inset).max(0.0)),
    };
    let form_label = Rect {
        x: pad.min(side_w),
        y: label_y,
        w: inner_w,
        h: cell_h.min((field_y - label_y).max(0.0)),
    };
    let form_rule = Rect {
        x: 0.0,
        y: rule_y,
        w: side_w,
        h: line.min((label_y - rule_y).max(0.0)),
    };
    let row_h = cell_h + inset;
    let list_y = pad.min(rule_y);
    let list = Rect {
        x: 0.0,
        y: list_y,
        w: side_w,
        h: (rule_y - pad - list_y).max(0.0),
    };

    Layout {
        window: (win_w, win_h),
        cell: (cell_w, cell_h),
        sidebar: Rect {
            x: 0.0,
            y: 0.0,
            w: side_w,
            h: win_h,
        },
        separator: Rect {
            x: side_w,
            y: 0.0,
            w: sep_w,
            h: win_h,
        },
        list,
        row_h,
        visible_rows: (list.h / row_h).floor() as usize,
        label_cols: (inner_w / cell_w).floor() as usize,
        form_rule,
        form_label,
        field,
        field_text,
        field_cols,
        create,
        cancel,
        terminal,
        grid: m.grid_for(terminal.w as u32, window.1),
    }
}

impl Layout {
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let window = Rect {
            x: 0.0,
            y: 0.0,
            w: self.window.0,
            h: self.window.1,
        };
        if !window.contains(x, y) {
            return Hit::Outside;
        }
        if x >= self.terminal.x {
            return Hit::Terminal;
        }
        if self.create.contains(x, y) {
            return Hit::Create;
        }
        if self.cancel.contains(x, y) {
            return Hit::Cancel;
        }
        if self.field.contains(x, y) {
            return Hit::Field {
                col: self.field_col(x),
            };
        }
        if self.list.contains(x, y) && self.sidebar.contains(x, y) {
            let row = ((y - self.list.y) / self.row_h).floor() as usize;
            if row < self.visible_rows {
                return Hit::ListRow(row);
            }
        }
        Hit::Sidebar
    }

    /// The caret position (in visible columns, `0..=field_cols`) nearest to pixel column `x`,
    /// which may be outside the field (a drag).
    pub fn field_col(&self, x: f32) -> usize {
        let col = ((x - self.field_text.x) / self.cell.0).round();
        // `as` maps NaN and negatives to 0.
        (col.max(0.0) as usize).min(self.field_cols)
    }

    /// The rectangle of a visible list row.
    pub fn list_row(&self, row: usize) -> Rect {
        Rect {
            x: self.list.x,
            y: self.list.y + row as f32 * self.row_h,
            w: self.list.w,
            h: self.row_h,
        }
    }
}
