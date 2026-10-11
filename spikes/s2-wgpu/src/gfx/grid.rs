//! Quad building: an `s2_core::Frame` becomes three lists of `Instance`s for `render::Renderer`.
//!
//! API for the front-end:
//! - `Scene::update_cells(&frame, &mut atlas, &theme)` — call it once for **every** frame taken
//!   from the `FrameSlot`, in order. It rebuilds only the rows the frame marks dirty (a frame
//!   skipped here would lose its dirty rows), or all rows when the grid size, the atlas
//!   generation (scale / font size / atlas restart), the theme or the origin changed.
//!   Returns how many rows it rebuilt.
//! - `Scene::update_overlays(&frame, &mut atlas, &theme, &overlays)` — cursor, IME preedit,
//!   selection and the `--photon` square. Cheap; call it before each render, also when only an
//!   overlay changed (same frame as before). If an overlay glyph restarts a full atlas, the
//!   cell quads are rebuilt in the same call; nothing for the caller to check.
//! - `Scene::update(..)` does both.
//! - `Scene::layers()` → `[background, glyphs, overlay]` in draw order, for
//!   `Renderer::prepare`. A front-end can append layers of its own (sidebar, menus).
//! - `Scene::set_origin(x, y)` moves the grid inside the window (snapped to whole pixels);
//!   `Scene::cell_at(x, y)` maps a pixel back to `(col, row)` for the mouse.
//!
//! Positions and sizes are physical pixels, y down, origin at the window's top-left corner.
//! Layout rules: a `Wide::Lead` cell followed by its `Wide::Spacer` is one quad two columns
//! wide; cells with the default background emit no background quad (the clear colour shows) and
//! neighbours with the same background merge into one quad; blank cells emit no glyph.

use unicode_width::UnicodeWidthChar;

use s2_core::{Cell, CursorShape, Frame, Wide};

use super::atlas::{Atlas, CellMetrics, FontStyle, SUBPIXEL_BINS};

/// One instanced quad. `kind` selects how the fragment shader colours it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    /// Top-left corner, pixels.
    pub pos: [f32; 2],
    pub size: [f32; 2],
    /// Atlas rectangle `[x, y, w, h]` in atlas pixels; unused for `KIND_SOLID`.
    pub uv: [f32; 4],
    /// sRGB, straight alpha.
    pub color: [u8; 4],
    pub kind: u32,
}

/// A filled rectangle: cell background, cursor, underline, selection, photon square.
pub const KIND_SOLID: u32 = 0;
/// A glyph from a coverage mask, tinted with `color`.
pub const KIND_MASK: u32 = 1;
/// A glyph with its own colours (emoji); `color` is ignored.
pub const KIND_COLOR: u32 = 2;

/// Edge of the `--photon` square, pixels.
pub const PHOTON_PX: f32 = 64.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Default foreground (cells with `fg: None`).
    pub fg: [u8; 3],
    /// Default background; also the colour to clear the surface with.
    pub bg: [u8; 3],
    pub cursor: [u8; 3],
    /// Drawn over the text, so keep it translucent.
    pub selection: [u8; 4],
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            fg: [0xd8, 0xd8, 0xd8],
            bg: [0x10, 0x10, 0x14],
            cursor: [0xe0, 0xc0, 0x40],
            selection: [0x40, 0x70, 0xc0, 0x80],
        }
    }
}

/// IME composition text, drawn underlined starting at the cursor cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
}

/// Columns `col_start..col_end` of one row (end exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellSpan {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
}

/// Everything drawn over the cells besides the cursor (which comes from the frame).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overlays {
    pub preedit: Option<Preedit>,
    /// `--photon`: `Some(true)` a white square, `Some(false)` a black one, `None` no square.
    pub photon: Option<bool>,
    pub selection: Vec<CellSpan>,
}

#[derive(Default)]
struct Row {
    bg: Vec<Instance>,
    glyphs: Vec<Instance>,
}

/// What the cached rows were built against; any difference means "rebuild every row".
#[derive(Clone, Copy, PartialEq)]
struct Built {
    cols: u16,
    rows: u16,
    generation: u64,
    metrics: CellMetrics,
    theme: Theme,
    origin: [f32; 2],
}

#[derive(Default)]
pub struct Scene {
    origin: [f32; 2],
    built: Option<Built>,
    rows: Vec<Row>,
    bg: Vec<Instance>,
    glyphs: Vec<Instance>,
    overlay: Vec<Instance>,
}

impl Scene {
    pub fn new() -> Scene {
        Scene::default()
    }

    /// Top-left corner of the grid in the window, rounded to whole pixels: atlas texels are
    /// copied 1:1, so a glyph quad between pixels would be resampled. Takes effect at the next
    /// `update_cells`.
    pub fn set_origin(&mut self, x: f32, y: f32) {
        let snap = |v: f32| if v.is_finite() { v.round() } else { 0.0 };
        self.origin = [snap(x), snap(y)];
    }

    /// `update_cells` followed by `update_overlays`; returns the number of rows rebuilt.
    pub fn update(
        &mut self,
        frame: &Frame,
        atlas: &mut Atlas,
        theme: &Theme,
        overlays: &Overlays,
    ) -> usize {
        let rebuilt = self.update_cells(frame, atlas, theme);
        if self.update_overlays(frame, atlas, theme, overlays) {
            frame.rows as usize
        } else {
            rebuilt
        }
    }

    /// Rebuilds the background and glyph quads of the rows that need it; returns how many.
    pub fn update_cells(&mut self, frame: &Frame, atlas: &mut Atlas, theme: &Theme) -> usize {
        let (cols, rows) = (frame.cols as usize, frame.rows as usize);
        let mut rebuilt = 0;
        // A second pass only if the atlas filled up and restarted during the first: the quads
        // built before the restart then point at glyphs that are gone.
        for _pass in 0..2 {
            let target = Built {
                cols: frame.cols,
                rows: frame.rows,
                generation: atlas.generation(),
                metrics: atlas.metrics(),
                theme: *theme,
                origin: self.origin,
            };
            let all = self.built != Some(target);
            if all {
                self.rows.resize_with(rows, Row::default);
            }
            rebuilt = 0;
            for y in 0..rows {
                if !(all || frame.dirty_rows.get(y).copied().unwrap_or(true)) {
                    continue;
                }
                let cells = frame.cells.get(y * cols..(y + 1) * cols).unwrap_or(&[]);
                let top = self.origin[1] + y as f32 * target.metrics.height;
                build_row(
                    &mut self.rows[y],
                    cells,
                    self.origin[0],
                    top,
                    &target.metrics,
                    theme,
                    atlas,
                );
                rebuilt += 1;
            }
            self.built = Some(target);
            if atlas.generation() == target.generation {
                break;
            }
        }
        if rebuilt > 0 {
            self.bg.clear();
            self.glyphs.clear();
            for row in &self.rows {
                self.bg.extend_from_slice(&row.bg);
                self.glyphs.extend_from_slice(&row.glyphs);
            }
        }
        rebuilt
    }

    /// Rebuilds the overlay layer: selection, preedit, cursor, photon square — in that order,
    /// so the photon square is the last thing drawn. `frame` is the one last given to
    /// `update_cells`.
    ///
    /// An overlay glyph (new preedit text) can fill the atlas and restart it, which voids the
    /// cell quads too. That is handled here: the cells are rebuilt from `frame` and the overlays
    /// built again, and the call returns `true` (information only — nothing is left to do).
    pub fn update_overlays(
        &mut self,
        frame: &Frame,
        atlas: &mut Atlas,
        theme: &Theme,
        overlays: &Overlays,
    ) -> bool {
        self.build_overlays(frame, atlas, theme, overlays);
        let stale = self
            .built
            .is_some_and(|built| built.generation != atlas.generation());
        if stale {
            self.update_cells(frame, atlas, theme);
            self.build_overlays(frame, atlas, theme, overlays);
        }
        stale
    }

    fn build_overlays(
        &mut self,
        frame: &Frame,
        atlas: &mut Atlas,
        theme: &Theme,
        overlays: &Overlays,
    ) {
        let m = atlas.metrics();
        let (cols, rows) = (frame.cols as usize, frame.rows as usize);
        let origin = self.origin;
        let cell_x = |col: usize| origin[0] + col as f32 * m.width;
        let cell_y = |row: usize| origin[1] + row as f32 * m.height;
        let out = &mut self.overlay;
        out.clear();

        for span in &overlays.selection {
            let (start, end) = (span.col_start as usize, (span.col_end as usize).min(cols));
            if (span.row as usize) < rows && start < end {
                let size = [(end - start) as f32 * m.width, m.height];
                out.push(solid(
                    [cell_x(start), cell_y(span.row as usize)],
                    size,
                    theme.selection,
                ));
            }
        }

        // The cursor is `None` while the viewport is scrolled away from it, and may be stale
        // (outside the grid) right after a resize: then there is no cursor and no preedit.
        let cursor = frame
            .cursor
            .filter(|c| (c.col as usize) < cols && (c.row as usize) < rows);
        if let Some(cursor) = cursor {
            let row = cursor.row as usize;
            let mut col = cursor.col as usize;
            let mut composing = false;

            if let Some(preedit) = overlays.preedit.as_ref().filter(|p| !p.text.is_empty()) {
                composing = true;
                // Clusters that do not fit before the right edge are dropped.
                let mut used = 0;
                let mut text = Vec::new();
                for (cluster, width) in clusters(&preedit.text) {
                    if col + used + width > cols {
                        break;
                    }
                    text.push((cluster, used));
                    used += width;
                }
                let (x, y) = (cell_x(col), cell_y(row));
                let width = used as f32 * m.width;
                if used > 0 {
                    out.push(solid([x, y], [width, m.height], opaque(theme.bg)));
                    for (cluster, at) in &text {
                        push_text(
                            out,
                            atlas,
                            cluster,
                            FontStyle::default(),
                            x + *at as f32 * m.width,
                            y,
                            &m,
                            opaque(theme.fg),
                        );
                    }
                    out.push(solid(
                        [x, y + m.underline_y],
                        [width, m.stroke],
                        opaque(theme.fg),
                    ));
                }
                col = (col + used).min(cols - 1);
            }

            if cursor.visible {
                // On a wide character the cursor covers both columns. The emulator may report
                // either half, so a cursor on the spacer moves back to the lead.
                let at = |c: usize| frame.cells.get(row * cols + c);
                let wide = |c: usize| at(c).map(|cell| cell.wide);
                let mut span = 1;
                if !composing {
                    if wide(col) == Some(Wide::Spacer)
                        && col > 0
                        && wide(col - 1) == Some(Wide::Lead)
                    {
                        col -= 1;
                        span = 2;
                    } else if wide(col) == Some(Wide::Lead)
                        && col + 1 < cols
                        && wide(col + 1) == Some(Wide::Spacer)
                    {
                        span = 2;
                    }
                }
                let (x, y) = (cell_x(col), cell_y(row));
                let colour = opaque(theme.cursor);
                match cursor.shape {
                    CursorShape::Block => {
                        out.push(solid([x, y], [span as f32 * m.width, m.height], colour));
                        if let Some(cell) = at(col).filter(|cell| !composing && !is_blank(cell)) {
                            let style = FontStyle {
                                bold: cell.flags.bold,
                                italic: cell.flags.italic,
                            };
                            push_text(out, atlas, &cell.text, style, x, y, &m, opaque(theme.bg));
                        }
                    }
                    CursorShape::Bar => {
                        let thickness = (m.width / 8.0).round().max(1.0);
                        out.push(solid([x, y], [thickness, m.height], colour));
                    }
                    CursorShape::Underline => {
                        let thickness = (m.height / 8.0).round().max(1.0);
                        out.push(solid(
                            [x, y + m.height - thickness],
                            [span as f32 * m.width, thickness],
                            colour,
                        ));
                    }
                }
            }
        }

        if let Some(on) = overlays.photon {
            let colour = if on {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            };
            out.push(solid([0.0, 0.0], [PHOTON_PX, PHOTON_PX], colour));
        }
    }

    /// Cell background quads, row by row.
    pub fn bg(&self) -> &[Instance] {
        &self.bg
    }

    /// Glyph quads plus underline / strike-through lines, row by row.
    pub fn glyphs(&self) -> &[Instance] {
        &self.glyphs
    }

    pub fn overlay(&self) -> &[Instance] {
        &self.overlay
    }

    /// `[bg, glyphs, overlay]`: the order to draw them in.
    pub fn layers(&self) -> [&[Instance]; 3] {
        [&self.bg, &self.glyphs, &self.overlay]
    }

    /// The cell under a window pixel, as `(col, row)`; `None` outside the grid or before the
    /// first `update_cells`.
    pub fn cell_at(&self, x: f32, y: f32) -> Option<(u16, u16)> {
        let built = self.built.as_ref()?;
        let col = ((x - built.origin[0]) / built.metrics.width).floor();
        let row = ((y - built.origin[1]) / built.metrics.height).floor();
        let inside = col >= 0.0 && row >= 0.0 && col < built.cols as f32 && row < built.rows as f32;
        inside.then_some((col as u16, row as u16))
    }
}

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

fn solid(pos: [f32; 2], size: [f32; 2], color: [u8; 4]) -> Instance {
    Instance {
        pos,
        size,
        uv: [0.0; 4],
        color,
        kind: KIND_SOLID,
    }
}

fn is_blank(cell: &Cell) -> bool {
    cell.text.chars().all(|c| c == ' ')
}

/// Splits preedit text into drawable clusters with their width in columns (1 or 2); zero-width
/// characters (combining marks, joiners) stay with the character before them.
fn clusters(text: &str) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for ch in text.chars().filter(|c| !c.is_control()) {
        match (ch.width().unwrap_or(0), out.last_mut()) {
            (0, Some(last)) => last.0.push(ch),
            (0, None) => {}
            (width, _) => out.push((ch.to_string(), width.min(2))),
        }
    }
    out
}

/// Appends the glyph quads of one cell's text; `(x, y)` is the cell's top-left corner.
#[allow(clippy::too_many_arguments)]
fn push_text(
    out: &mut Vec<Instance>,
    atlas: &mut Atlas,
    text: &str,
    style: FontStyle,
    x: f32,
    y: f32,
    m: &CellMetrics,
    color: [u8; 4],
) {
    let shaped = atlas.shape(text, style);
    for glyph in shaped.iter() {
        // Whole pixels go into the quad position, the remainder selects a subpixel raster.
        let whole = glyph.x.floor();
        let bin = (((glyph.x - whole) * SUBPIXEL_BINS as f32) as u8).min(SUBPIXEL_BINS - 1);
        let Some(slot) = atlas.glyph(atlas.key(glyph, bin)) else {
            continue;
        };
        out.push(Instance {
            pos: [
                x + whole + slot.left as f32,
                y + m.baseline - glyph.y.round() - slot.top as f32,
            ],
            size: [slot.w as f32, slot.h as f32],
            uv: [slot.x as f32, slot.y as f32, slot.w as f32, slot.h as f32],
            color,
            kind: if slot.color { KIND_COLOR } else { KIND_MASK },
        });
    }
}

/// Rebuilds one row's quads. `left` / `top`: the row's top-left corner in pixels.
fn build_row(
    row: &mut Row,
    cells: &[Cell],
    left: f32,
    top: f32,
    m: &CellMetrics,
    theme: &Theme,
    atlas: &mut Atlas,
) {
    row.bg.clear();
    row.glyphs.clear();
    // The open background run: (first column, end column, colour).
    let mut run: Option<(usize, usize, [u8; 3])> = None;
    let flush = |run: &mut Option<(usize, usize, [u8; 3])>, bg: &mut Vec<Instance>| {
        if let Some((start, end, colour)) = run.take() {
            bg.push(solid(
                [left + start as f32 * m.width, top],
                [(end - start) as f32 * m.width, m.height],
                opaque(colour),
            ));
        }
    };

    let mut col = 0;
    while col < cells.len() {
        let cell = &cells[col];
        // A lead takes its spacer's column too. A lead without a spacer after it (last column)
        // and a spacer without a lead before it are drawn as ordinary single cells.
        let span = if cell.wide == Wide::Lead
            && cells
                .get(col + 1)
                .is_some_and(|next| next.wide == Wide::Spacer)
        {
            2
        } else {
            1
        };
        let x = left + col as f32 * m.width;

        let mut fg = cell.fg.map_or(theme.fg, |(r, g, b)| [r, g, b]);
        let mut bg = cell.bg.map_or(theme.bg, |(r, g, b)| [r, g, b]);
        if cell.flags.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.faint {
            fg = [0, 1, 2].map(|i| ((fg[i] as u16 + bg[i] as u16) / 2) as u8);
        }

        if bg == theme.bg {
            flush(&mut run, &mut row.bg);
        } else {
            match &mut run {
                Some((_, end, colour)) if *end == col && *colour == bg => *end = col + span,
                _ => {
                    flush(&mut run, &mut row.bg);
                    run = Some((col, col + span, bg));
                }
            }
        }

        if !is_blank(cell) {
            let style = FontStyle {
                bold: cell.flags.bold,
                italic: cell.flags.italic,
            };
            push_text(
                &mut row.glyphs,
                atlas,
                &cell.text,
                style,
                x,
                top,
                m,
                opaque(fg),
            );
        }
        let width = span as f32 * m.width;
        if cell.flags.underline {
            row.glyphs.push(solid(
                [x, top + m.underline_y],
                [width, m.stroke],
                opaque(fg),
            ));
        }
        if cell.flags.strike {
            row.glyphs
                .push(solid([x, top + m.strike_y], [width, m.stroke], opaque(fg)));
        }
        col += span;
    }
    flush(&mut run, &mut row.bg);
}
