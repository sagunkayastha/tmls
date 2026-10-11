//! The UI as quads: two layers of `gfx::grid::Instance`s around the terminal's own —
//! `base` (sidebar, list, form) is drawn before the terminal, `top` (context menu) after it:
//!
//! `[base, scene.bg, scene.glyphs, scene.overlay, top]`
//!
//! so the `--photon` square (last in the scene's overlay) stays on top of the sidebar.
//!
//! Text is set on the cell grid of the atlas (one character per column, wide ones two), like
//! the terminal's. The renderer has no scissor, so nothing here relies on clipping: labels
//! are cut to the columns that fit (`fit`, with an ellipsis) and the field draws only whole
//! characters inside it (`TextField::view`).
//!
//! `UiLayers` caches the quads: they are rebuilt when `invalidate` was called or the atlas
//! started a new generation (zoom, scale change, atlas restart) — not on every frame, so a
//! key echo pays nothing for the sidebar. `compose` builds terminal overlays and UI together
//! and survives an atlas restart caused by either.

use s2_core::Frame;

use super::Ui;
use super::focus::Focus;
use super::layout::{CANCEL_LABEL, CREATE_LABEL, FORM_LABEL, Layout, Rect};
use super::menu::{BORDER, ITEMS};
use crate::gfx::atlas::{Atlas, CellMetrics, FontStyle};
use crate::gfx::grid::{Instance, Overlays, Scene, clusters, push_text, solid};

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

/// `text` cut to `cols` columns: `(character, column)` of what is drawn. Text that does not
/// fit ends in "…". Wide characters take two columns and are never cut; marks stay with
/// their base; control characters are skipped.
pub fn fit(text: &str, cols: usize) -> Vec<(String, usize)> {
    let all = clusters(text);
    let total: usize = all.iter().map(|(_, width)| width).sum();
    let mut out = Vec::new();
    let mut used = 0;
    if total <= cols {
        for (cluster, width) in all {
            out.push((cluster, used));
            used += width;
        }
        return out;
    }
    if cols == 0 {
        return out;
    }
    for (cluster, width) in all {
        if used + width > cols - 1 {
            break;
        }
        out.push((cluster, used));
        used += width;
    }
    out.push(("…".to_string(), used));
    out
}

/// Collects quads; `text` and `rect` are the only two things the UI draws.
struct Painter<'a> {
    out: &'a mut Vec<Instance>,
    atlas: &'a mut Atlas,
    m: CellMetrics,
}

impl Painter<'_> {
    fn rect(&mut self, r: Rect, colour: [u8; 4]) {
        if r.w > 0.0 && r.h > 0.0 {
            self.out.push(solid([r.x, r.y], [r.w, r.h], colour));
        }
    }

    /// A frame of thickness `t` just inside `r`.
    fn outline(&mut self, r: Rect, t: f32, colour: [u8; 4]) {
        let t = t.min(r.w / 2.0).min(r.h / 2.0);
        let side_h = r.h - 2.0 * t;
        self.rect(Rect { h: t, ..r }, colour);
        self.rect(
            Rect {
                y: r.bottom() - t,
                h: t,
                ..r
            },
            colour,
        );
        self.rect(
            Rect {
                y: r.y + t,
                w: t,
                h: side_h,
                ..r
            },
            colour,
        );
        self.rect(
            Rect {
                x: r.right() - t,
                y: r.y + t,
                w: t,
                h: side_h,
            },
            colour,
        );
    }

    /// `text` from `(x, y)` (top-left of its first cell), cut to `cols` columns.
    fn label(&mut self, text: &str, x: f32, y: f32, cols: usize, colour: [u8; 4]) {
        for (cluster, col) in fit(text, cols) {
            self.cell(&cluster, x + col as f32 * self.m.width, y, colour);
        }
    }

    fn cell(&mut self, text: &str, x: f32, y: f32, colour: [u8; 4]) {
        let style = FontStyle::default();
        push_text(self.out, self.atlas, text, style, x, y, &self.m, colour);
    }

    /// A label centred in `r` (on whole cells), cut to what fits.
    fn centred(&mut self, text: &str, r: Rect, colour: [u8; 4]) {
        let cols = (r.w / self.m.width).floor() as usize;
        let used = fit(text, cols).last().map_or(0, |(_, col)| col + 1);
        let x = r.x + ((r.w - used as f32 * self.m.width) / 2.0).floor().max(0.0);
        let y = r.y + ((r.h - self.m.height) / 2.0).floor().max(0.0);
        if r.h >= self.m.height {
            self.label(text, x, y, cols, colour);
        }
    }
}

#[derive(Default)]
pub struct UiLayers {
    base: Vec<Instance>,
    top: Vec<Instance>,
    /// Atlas generation the quads were built in.
    built: Option<u64>,
    dirty: bool,
}

impl UiLayers {
    pub fn new() -> UiLayers {
        UiLayers::default()
    }

    /// The UI state or the layout changed: rebuild at the next `update`.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Sidebar, list and form: drawn before the terminal.
    pub fn base(&self) -> &[Instance] {
        &self.base
    }

    /// The context menu: drawn after the terminal.
    pub fn top(&self) -> &[Instance] {
        &self.top
    }

    /// Rebuilds the quads if needed; returns whether it did. `ui` is `&mut` only for the
    /// field's scroll position (`TextField::view`).
    pub fn update(&mut self, ui: &mut Ui, layout: &Layout, atlas: &mut Atlas) -> bool {
        if !self.dirty && self.built == Some(atlas.generation()) {
            return false;
        }
        // A second pass only if the atlas filled up and restarted during the first: quads
        // built before the restart then point at glyphs that are gone.
        for _pass in 0..2 {
            let generation = atlas.generation();
            self.base.clear();
            self.top.clear();
            build_base(&mut self.base, ui, layout, atlas);
            build_top(&mut self.top, ui, atlas);
            if atlas.generation() == generation {
                break;
            }
        }
        self.built = Some(atlas.generation());
        self.dirty = false;
        true
    }
}

fn build_base(out: &mut Vec<Instance>, ui: &mut Ui, l: &Layout, atlas: &mut Atlas) {
    let theme = ui.theme.clone();
    let m = atlas.metrics();
    let line = l.separator.w.max(1.0);
    let ring = opaque(theme.focus_ring);
    let mut p = Painter { out, atlas, m };

    // Always the first quad: everything else of the sidebar is on top of it.
    p.rect(l.sidebar, opaque(theme.sidebar_bg));
    // The terminal shows its focus on the line beside it.
    let beside = if ui.focus == Focus::Terminal {
        ring
    } else {
        opaque(theme.separator)
    };
    p.rect(l.separator, beside);

    // List: the rows that fit, labels cut to the sidebar.
    let text_x = l.form_label.x;
    let text_dy = ((l.row_h - m.height) / 2.0).floor().max(0.0);
    for row in 0..l.visible_rows {
        let index = ui.list.top() + row;
        let Some(name) = ui.list.items().get(index) else {
            break;
        };
        let r = l.list_row(row);
        let mut colour = opaque(theme.sidebar_fg);
        if index == ui.list.selected() {
            p.rect(r, opaque(theme.item_selected_bg));
            colour = opaque(theme.item_selected_fg);
        }
        p.label(name, text_x, r.y + text_dy, l.label_cols, colour);
    }
    if ui.focus == Focus::List {
        p.outline(l.list, 2.0 * line, ring);
    }

    // Form: caption, field, buttons.
    p.rect(l.form_rule, opaque(theme.separator));
    if l.form_label.h >= m.height {
        let (x, y) = (l.form_label.x, l.form_label.y);
        p.label(FORM_LABEL, x, y, l.label_cols, opaque(theme.muted_fg));
    }
    p.rect(l.field, opaque(theme.field_bg));
    let focused = ui.focus == Focus::Field;
    if focused {
        p.outline(l.field, 2.0 * line, ring);
    } else {
        p.outline(l.field, line, opaque(theme.separator));
    }
    if l.field_text.h >= m.height {
        let t = l.field_text;
        let view = ui.field.view(l.field_cols);
        let span = |(start, end): (usize, usize)| Rect {
            x: t.x + start as f32 * m.width,
            y: t.y,
            w: (end - start) as f32 * m.width,
            h: m.height,
        };
        for (text, col, _) in &view.cells {
            p.cell(
                text,
                t.x + *col as f32 * m.width,
                t.y,
                opaque(theme.field_fg),
            );
        }
        if let Some(columns) = view.selection {
            p.rect(span(columns), theme.field_selection);
        }
        if let Some(columns) = view.preedit {
            let under = span(columns);
            let underline = Rect {
                y: t.y + m.underline_y,
                h: m.stroke,
                ..under
            };
            p.rect(underline, opaque(theme.field_fg));
        }
        if focused {
            let caret = Rect {
                x: t.x + view.caret_col as f32 * m.width,
                y: t.y,
                w: (m.width / 8.0).round().max(1.0),
                h: m.height,
            };
            p.rect(caret, opaque(theme.caret));
        }
    }
    for (r, label) in [(l.create, CREATE_LABEL), (l.cancel, CANCEL_LABEL)] {
        p.rect(r, opaque(theme.button_bg));
        p.centred(label, r, opaque(theme.button_fg));
    }
}

fn build_top(out: &mut Vec<Instance>, ui: &Ui, atlas: &mut Atlas) {
    let Some(menu) = &ui.menu else {
        return;
    };
    let theme = &ui.theme;
    let m = atlas.metrics();
    let mut p = Painter { out, atlas, m };
    let r = menu.rect();
    p.rect(r, opaque(theme.menu_border));
    let inner = Rect {
        x: r.x + BORDER,
        y: r.y + BORDER,
        w: r.w - 2.0 * BORDER,
        h: r.h - 2.0 * BORDER,
    };
    p.rect(inner, opaque(theme.menu_bg));
    for (index, item) in ITEMS.into_iter().enumerate() {
        let row = menu.item_rect(index);
        if menu.hovered() == Some(index) {
            p.rect(row, opaque(theme.menu_hover_bg));
        }
        let x = row.x + 2.0 * m.width;
        let y = row.y + ((row.h - m.height) / 2.0).floor().max(0.0);
        let cols = ((row.right() - x) / m.width).floor().max(0.0) as usize;
        p.label(item.label(), x, y, cols, opaque(theme.menu_fg));
    }
}

/// The draw order for `Renderer::prepare`: the sidebar under the terminal's three layers (the
/// photon square is the last quad of the terminal's overlay and must stay on top of the
/// sidebar it overlaps), the menu over them.
pub fn layer_order<'a>(scene: &'a Scene, layers: &'a UiLayers) -> [&'a [Instance]; 5] {
    let [bg, glyphs, overlay] = scene.layers();
    [layers.base(), bg, glyphs, overlay, layers.top()]
}

/// Everything drawn besides the cells, for one render: the terminal's overlays (cursor,
/// preedit, selection, photon) and the UI layers. `frame` is the one last given to
/// `Scene::update_cells`.
///
/// Both draw glyphs, and either can fill the atlas and restart it, which voids the other's
/// quads and the cells'. `Scene::update_overlays` rebuilds the cells when it finds them
/// stale and `UiLayers::update` rebuilds on a new generation, so the two are repeated until
/// the UI layers were built in the atlas's current generation.
pub fn compose(
    scene: &mut Scene,
    frame: &Frame,
    atlas: &mut Atlas,
    overlays: &Overlays,
    ui: &mut Ui,
    layout: &Layout,
    layers: &mut UiLayers,
) {
    let theme = ui.theme.terminal;
    for _round in 0..3 {
        layers.update(ui, layout, atlas);
        scene.update_overlays(frame, atlas, &theme, overlays);
        if layers.built == Some(atlas.generation()) {
            break;
        }
    }
}
