//! Glyph atlas (CPU side): shaping and font fallback by `cosmic-text`, rasterisation by `swash`,
//! shelf packing into one RGBA image that `render::Renderer` uploads.
//!
//! API for the front-end:
//! - `Atlas::new(FontSpec, scale)` loads the system fonts once (slow: do it once per process).
//!   `set_scale` (HiDPI / monitor change) and `set_font_size` (zoom) re-rasterise: they drop every
//!   cached glyph and start a new `generation()`.
//! - `metrics()` is the cell box in physical pixels; `CellMetrics::grid_for(w, h)` gives the grid
//!   that fits a surface.
//! - `shape(text, style)` turns one cell's text into positioned glyphs (with fallback fonts);
//!   `key(glyph, subpx)` + `glyph(key)` return the glyph's rectangle in the atlas, rasterising on
//!   a miss. `grid::Scene` does all of this; a front-end normally calls only the lines above.
//! - `image()` / `take_dirty()` are what the renderer uploads.
//!
//! Cache key: (font face, weight, font size, glyph id, subpixel x bin, scale) — `GlyphKey`.
//! Rectangles are in atlas pixels, so the image may grow downwards without invalidating them.
//! When it cannot grow any more it is emptied and `generation()` changes: every rectangle handed
//! out before is then void, and `grid::Scene` rebuilds its rows when it sees the new generation.
//!
//! All sizes here are physical pixels unless a name says otherwise.

use std::collections::HashMap;
use std::rc::Rc;

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Format, Vector};

pub use cosmic_text::fontdb;

/// Atlas width; fixed so that growing never moves a glyph.
const ATLAS_WIDTH: u32 = 1024;
const ATLAS_START_HEIGHT: u32 = 256;
const ATLAS_MAX_HEIGHT: u32 = 4096;
/// Empty pixels between neighbours in the atlas.
const PADDING: u32 = 1;
/// The shape cache is emptied when it reaches this many strings (a flood of unique graphemes).
const SHAPE_CACHE_LIMIT: usize = 16_384;
/// Tried in order when `FontSpec::family` is `None` or not installed; then any monospaced face.
const DEFAULT_FAMILIES: [&str; 6] = [
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Adwaita Mono",
    "Cascadia Mono",
    "Courier New",
];

/// Horizontal subpixel positions a glyph is rasterised at (quarter pixels).
pub const SUBPIXEL_BINS: u8 = 4;

/// Which font, and how large in logical pixels (multiplied by the scale factor for rasterising).
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub family: Option<String>,
    pub size: f32,
}

impl Default for FontSpec {
    fn default() -> Self {
        FontSpec {
            family: None,
            size: 14.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontStyle {
    pub bold: bool,
    pub italic: bool,
}

/// The cell box. `width` and `height` are whole pixels; the y values are measured from the top
/// of the cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
    /// Top of the underline stroke.
    pub underline_y: f32,
    /// Top of the strike-through stroke.
    pub strike_y: f32,
    /// Thickness of both strokes.
    pub stroke: f32,
}

impl CellMetrics {
    /// Columns and rows that fit into a surface of this size; never less than 1 x 1.
    pub fn grid_for(&self, width_px: u32, height_px: u32) -> (u16, u16) {
        let fit = |px: u32, cell: f32| {
            ((px as f32 / cell.max(1.0)).floor() as u32).clamp(1, u16::MAX as u32) as u16
        };
        (fit(width_px, self.width), fit(height_px, self.height))
    }
}

/// Everything a rasterised glyph depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font: fontdb::ID,
    pub weight: u16,
    /// Logical font size, `f32::to_bits`.
    pub size_bits: u32,
    pub glyph: u16,
    /// Subpixel x bin, `0..SUBPIXEL_BINS`.
    pub subpx: u8,
    /// Scale factor, `f32::to_bits`.
    pub scale_bits: u32,
}

/// A glyph's rectangle in the atlas image plus its bearings: draw it with its left edge at
/// `pen_x + left` and its top edge at `baseline_y - top`. `w == 0` means "nothing to draw".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasGlyph {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub left: i16,
    pub top: i16,
    /// RGBA bitmap (colour emoji) rather than a coverage mask.
    pub color: bool,
}

/// One glyph of a shaped cell text. `x` is the pen position from the cell's left edge, `y` the
/// shift above the baseline; both in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub font: fontdb::ID,
    pub weight: u16,
    pub glyph: u16,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AtlasStats {
    /// `glyph()` calls answered from the cache.
    pub hits: u64,
    /// `glyph()` calls that rasterised.
    pub misses: u64,
    /// Glyphs cached now.
    pub glyphs: usize,
}

/// The atlas pixels: `width * height * 4` bytes, RGBA. A mask glyph is stored as white with
/// its coverage in alpha.
pub struct AtlasImage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

pub struct Atlas {
    fonts: FontSystem,
    scaler: ScaleContext,
    buffer: Buffer,
    /// Resolved family name; `None` when no usable font is installed.
    family: Option<String>,
    primary: Option<fontdb::ID>,
    size: f32,
    scale: f32,
    metrics: CellMetrics,
    generation: u64,

    glyphs: HashMap<GlyphKey, Option<AtlasGlyph>>,
    /// Indexed by `style_index`.
    shapes: [HashMap<String, Rc<[ShapedGlyph]>>; 4],
    hits: u64,
    misses: u64,

    pixels: Vec<u8>,
    height: u32,
    /// Shelf packer: the open shelf's top, its height, and the next free x on it.
    shelf_y: u32,
    shelf_h: u32,
    shelf_x: u32,
    dirty: Option<(u32, u32)>,
}

fn style_index(style: FontStyle) -> usize {
    style.bold as usize | (style.italic as usize) << 1
}

impl Atlas {
    /// Loads the system fonts (through fontconfig's directories) and picks the grid font.
    pub fn new(spec: FontSpec, scale: f32) -> Atlas {
        Atlas::with_font_system(FontSystem::new(), spec, scale)
    }

    /// Like `new`, with a caller-built font database.
    pub fn with_font_system(mut fonts: FontSystem, spec: FontSpec, scale: f32) -> Atlas {
        let (family, primary) = resolve_family(&mut fonts, spec.family.as_deref());
        let size = sane(spec.size, 14.0);
        let scale = sane(scale, 1.0);
        let mut atlas = Atlas {
            buffer: Buffer::new_empty(Metrics::new(size * scale, size * scale * 1.2)),
            fonts,
            scaler: ScaleContext::new(),
            family,
            primary,
            size,
            scale,
            metrics: CellMetrics {
                width: 1.0,
                height: 1.0,
                baseline: 1.0,
                underline_y: 0.0,
                strike_y: 0.0,
                stroke: 1.0,
            },
            generation: 0,
            glyphs: HashMap::new(),
            shapes: Default::default(),
            hits: 0,
            misses: 0,
            pixels: vec![0; (ATLAS_WIDTH * ATLAS_START_HEIGHT * 4) as usize],
            height: ATLAS_START_HEIGHT,
            shelf_y: 0,
            shelf_h: 0,
            shelf_x: 0,
            dirty: None,
        };
        atlas.remeasure();
        atlas
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Logical font size.
    pub fn font_size(&self) -> f32 {
        self.size
    }

    /// Sets the scale factor. A change drops every cached glyph (they are re-rasterised at the
    /// new size on demand), updates `metrics()` and starts a new generation. Returns whether
    /// anything changed.
    pub fn set_scale(&mut self, scale: f32) -> bool {
        let scale = sane(scale, 1.0);
        if scale == self.scale {
            return false;
        }
        self.scale = scale;
        self.remeasure();
        self.invalidate();
        true
    }

    /// Sets the logical font size (zoom); same effect on the cache as `set_scale`.
    pub fn set_font_size(&mut self, size: f32) -> bool {
        let size = sane(size, 14.0);
        if size == self.size {
            return false;
        }
        self.size = size;
        self.remeasure();
        self.invalidate();
        true
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    /// Changes whenever rectangles handed out earlier stop being valid: after a scale or size
    /// change, and when the atlas filled up and was emptied.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The regular face of the grid font; `None` if no font could be loaded at all.
    pub fn primary_font(&self) -> Option<fontdb::ID> {
        self.primary
    }

    /// Shapes one cell's text (a grapheme cluster, or a short string). Characters the grid font
    /// lacks come from whichever installed font has them. Results are cached per (text, style).
    pub fn shape(&mut self, text: &str, style: FontStyle) -> Rc<[ShapedGlyph]> {
        let index = style_index(style);
        if let Some(shaped) = self.shapes[index].get(text) {
            return shaped.clone();
        }
        let shaped: Rc<[ShapedGlyph]> = self.shape_uncached(text, style).into();
        if self.shapes[index].len() >= SHAPE_CACHE_LIMIT {
            self.shapes[index].clear();
        }
        self.shapes[index].insert(text.to_string(), shaped.clone());
        shaped
    }

    fn shape_uncached(&mut self, text: &str, style: FontStyle) -> Vec<ShapedGlyph> {
        let Some(family) = self.family.as_deref() else {
            return Vec::new();
        };
        if text.is_empty() {
            return Vec::new();
        }
        let attrs = Attrs::new()
            .family(Family::Name(family))
            .weight(if style.bold {
                Weight::BOLD
            } else {
                Weight::NORMAL
            })
            .style(if style.italic {
                Style::Italic
            } else {
                Style::Normal
            });
        self.buffer.set_text(text, &attrs, Shaping::Advanced, None);
        self.buffer.shape_until_scroll(&mut self.fonts, false);
        let mut out = Vec::new();
        // One line of input; if the text held a line break only its first line is drawn.
        if let Some(run) = self.buffer.layout_runs().next() {
            for g in run.glyphs {
                out.push(ShapedGlyph {
                    font: g.font_id,
                    weight: g.font_weight.0,
                    glyph: g.glyph_id,
                    x: g.x + g.font_size * g.x_offset,
                    y: g.font_size * g.y_offset,
                });
            }
        }
        out
    }

    /// The cache key of a shaped glyph at the current size and scale.
    pub fn key(&self, glyph: &ShapedGlyph, subpx: u8) -> GlyphKey {
        GlyphKey {
            font: glyph.font,
            weight: glyph.weight,
            size_bits: self.size.to_bits(),
            glyph: glyph.glyph,
            subpx: subpx % SUBPIXEL_BINS,
            scale_bits: self.scale.to_bits(),
        }
    }

    /// The glyph's place in the atlas, rasterising and packing it on a miss. `None`: the glyph
    /// has no bitmap (a space), its font is gone, or it is larger than the atlas.
    ///
    /// A miss may empty a full atlas: compare `generation()` before and after a batch of calls.
    pub fn glyph(&mut self, key: GlyphKey) -> Option<AtlasGlyph> {
        if let Some(cached) = self.glyphs.get(&key) {
            self.hits += 1;
            return *cached;
        }
        self.misses += 1;
        let slot = self.rasterise(key);
        // `rasterise` may have emptied the atlas (and the map); the new glyph goes in after that.
        self.glyphs.insert(key, slot);
        slot
    }

    fn rasterise(&mut self, key: GlyphKey) -> Option<AtlasGlyph> {
        let font = self.fonts.get_font(key.font, Weight(key.weight))?;
        let font_ref = font.as_swash();
        let px = f32::from_bits(key.size_bits) * f32::from_bits(key.scale_bits);
        let mut builder = self.scaler.builder(font_ref).size(px).hint(true);
        // A variable font is one face for every weight: ask for the weight that was shaped.
        let wght = swash::Tag::from_be_bytes(*b"wght");
        if let Some(axis) = font_ref.variations().find_by_tag(wght) {
            let weight = f32::from(key.weight).clamp(axis.min_value(), axis.max_value());
            builder = builder
                .normalized_coords(font_ref.variations().normalized_coords([(wght, weight)]));
        }
        let mut scaler = builder.build();
        let offset = Vector::new(key.subpx as f32 / SUBPIXEL_BINS as f32, 0.0);
        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .offset(offset)
        .render(&mut scaler, key.glyph)?;
        let (w, h) = (image.placement.width, image.placement.height);
        if w == 0 || h == 0 {
            return None;
        }
        let color = match image.content {
            Content::Mask => false,
            Content::Color => true,
            // Not requested (`Format::Alpha`); would need a three-channel blend.
            Content::SubpixelMask => return None,
        };
        let bytes_per_pixel = if color { 4 } else { 1 };
        if image.data.len() < (w * h) as usize * bytes_per_pixel {
            return None;
        }
        let (x, y) = self.allocate(w, h)?;
        for row in 0..h {
            let dst = ((y + row) * ATLAS_WIDTH + x) as usize * 4;
            let dst = &mut self.pixels[dst..dst + w as usize * 4];
            let src = (row * w) as usize * bytes_per_pixel;
            if color {
                dst.copy_from_slice(&image.data[src..src + w as usize * 4]);
            } else {
                for (out, coverage) in dst
                    .chunks_exact_mut(4)
                    .zip(&image.data[src..src + w as usize])
                {
                    out.copy_from_slice(&[255, 255, 255, *coverage]);
                }
            }
        }
        self.mark_dirty(y, y + h);
        let clamp_i16 = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        Some(AtlasGlyph {
            x: x as u16,
            y: y as u16,
            w: w as u16,
            h: h as u16,
            left: clamp_i16(image.placement.left),
            top: clamp_i16(image.placement.top),
            color,
        })
    }

    /// Shelf packing: left to right on the open shelf, a new shelf below when the row is full,
    /// a taller image when the shelves reach the bottom, and an empty atlas (new generation)
    /// when the image is at its maximum height.
    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + PADDING > ATLAS_WIDTH || h + PADDING > ATLAS_MAX_HEIGHT {
            return None;
        }
        if self.shelf_x + w + PADDING > ATLAS_WIDTH {
            self.shelf_y += self.shelf_h;
            self.shelf_h = 0;
            self.shelf_x = 0;
        }
        if self.shelf_y + h + PADDING > ATLAS_MAX_HEIGHT {
            self.invalidate();
        }
        let needed = self.shelf_y + h + PADDING;
        if needed > self.height {
            while self.height < needed {
                self.height *= 2;
            }
            self.pixels
                .resize((ATLAS_WIDTH * self.height * 4) as usize, 0);
        }
        let at = (self.shelf_x, self.shelf_y);
        self.shelf_x += w + PADDING;
        self.shelf_h = self.shelf_h.max(h + PADDING);
        Some(at)
    }

    fn mark_dirty(&mut self, y0: u32, y1: u32) {
        self.dirty = Some(match self.dirty {
            Some((a, b)) => (a.min(y0), b.max(y1)),
            None => (y0, y1),
        });
    }

    /// Forgets every glyph and shape and empties the image (its height is kept).
    fn invalidate(&mut self) {
        self.generation += 1;
        self.glyphs.clear();
        for shapes in &mut self.shapes {
            shapes.clear();
        }
        self.pixels.fill(0);
        (self.shelf_x, self.shelf_y, self.shelf_h) = (0, 0, 0);
        self.dirty = Some((0, self.height));
    }

    pub fn stats(&self) -> AtlasStats {
        AtlasStats {
            hits: self.hits,
            misses: self.misses,
            glyphs: self.glyphs.len(),
        }
    }

    pub fn image(&self) -> AtlasImage<'_> {
        AtlasImage {
            width: ATLAS_WIDTH,
            height: self.height,
            rgba: &self.pixels,
        }
    }

    /// The image rows `y0..y1` changed since the last call (for a partial texture upload). If
    /// the image's height changed as well, the texture has to be recreated and filled whole.
    pub fn take_dirty(&mut self) -> Option<(u32, u32)> {
        self.dirty.take()
    }

    /// Recomputes the cell box from the grid font at `size * scale` pixels.
    fn remeasure(&mut self) {
        let px = self.size * self.scale;
        // Without any font: a plausible box, so the grid still has a geometry.
        let mut m = CellMetrics {
            width: (px * 0.6).round().max(1.0),
            height: (px * 1.2).ceil().max(1.0),
            baseline: (px * 0.95).round(),
            underline_y: 0.0,
            strike_y: 0.0,
            stroke: (px / 14.0).round().max(1.0),
        };
        let mut underline_offset = -m.stroke;
        let mut strike_offset = px * 0.3;
        if let Some(font) = self
            .primary
            .and_then(|id| self.fonts.get_font(id, Weight::NORMAL))
        {
            let font = font.as_swash();
            let fm = font.metrics(&[]).scale(px);
            let advance = font
                .glyph_metrics(&[])
                .scale(px)
                .advance_width(font.charmap().map('M'));
            let advance = if advance > 0.0 { advance } else { fm.max_width };
            if advance > 0.0 && fm.ascent > 0.0 {
                m.width = advance.round().max(1.0);
                m.height = (fm.ascent + fm.descent + fm.leading).ceil().max(1.0);
                m.baseline = (fm.ascent + fm.leading / 2.0).round();
                m.stroke = fm.stroke_size.round().max(1.0);
                underline_offset = fm.underline_offset;
                strike_offset = fm.strikeout_offset;
            }
        }
        // Offsets are "above the baseline is positive"; keep both strokes inside the cell.
        let lowest = (m.height - m.stroke).max(0.0);
        m.underline_y = (m.baseline - underline_offset)
            .round()
            .clamp(m.baseline.min(lowest), lowest);
        m.strike_y = (m.baseline - strike_offset)
            .round()
            .clamp(0.0, (m.baseline - 1.0).max(0.0));
        self.metrics = m;
        self.buffer.set_metrics(Metrics::new(px, m.height));
    }
}

fn sane(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value.min(1024.0)
    } else {
        fallback
    }
}

/// The family to shape with and its regular face: the requested one if installed, else the
/// first installed default, else any monospaced face, else any face at all.
fn resolve_family(
    fonts: &mut FontSystem,
    wanted: Option<&str>,
) -> (Option<String>, Option<fontdb::ID>) {
    let db = fonts.db();
    let regular = |name: &str| {
        db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        })
    };
    for name in wanted.into_iter().chain(DEFAULT_FAMILIES) {
        if let Some(id) = regular(name) {
            return (Some(name.to_string()), Some(id));
        }
    }
    let any = db
        .faces()
        .find(|f| f.monospaced)
        .or_else(|| db.faces().next());
    match any.and_then(|face| face.families.first().map(|(name, _)| name.clone())) {
        Some(name) => {
            let id = regular(&name);
            (Some(name), id)
        }
        None => (None, None),
    }
}
