//! Glyph atlas: cache keys, scale invalidation, packing, fallback fonts.

use s2_wgpu::gfx::atlas::{Atlas, AtlasGlyph, FontSpec, FontStyle, SUBPIXEL_BINS};

const REGULAR: FontStyle = FontStyle {
    bold: false,
    italic: false,
};

fn atlas(scale: f32) -> Atlas {
    Atlas::new(FontSpec::default(), scale)
}

/// Sum of the alpha channel inside a glyph's atlas rectangle.
fn ink(atlas: &Atlas, g: &AtlasGlyph) -> u64 {
    let image = atlas.image();
    let mut sum = 0u64;
    for y in g.y as u32..(g.y + g.h) as u32 {
        for x in g.x as u32..(g.x + g.w) as u32 {
            sum += image.rgba[((y * image.width + x) * 4 + 3) as usize] as u64;
        }
    }
    sum
}

#[test]
fn atlas_cache_and_scale_invalidate() {
    let mut atlas = atlas(1.0);
    assert!(
        atlas.primary_font().is_some(),
        "this test needs at least one system font"
    );
    let shaped = atlas.shape("A", REGULAR);
    assert_eq!(shaped.len(), 1);
    let key = atlas.key(&shaped[0], 0);

    // Miss, then hit: the second lookup rasterises nothing and returns the same slot.
    let first = atlas.glyph(key).expect("'A' rasterises");
    assert!(first.w > 0 && first.h > 0);
    let after_miss = atlas.stats();
    assert_eq!(
        (after_miss.hits, after_miss.misses, after_miss.glyphs),
        (0, 1, 1)
    );
    let again = atlas.glyph(key).unwrap();
    assert_eq!(again, first);
    let after_hit = atlas.stats();
    assert_eq!(
        (after_hit.hits, after_hit.misses, after_hit.glyphs),
        (1, 1, 1)
    );

    // Every part of the key separates entries: subpixel x, glyph, style (another face).
    let shifted = atlas.key(&shaped[0], 1);
    assert_ne!(shifted, key);
    atlas.glyph(shifted).unwrap();
    assert_eq!(atlas.stats().misses, 2);
    let other = atlas.shape("B", REGULAR);
    assert_ne!(atlas.key(&other[0], 0), key);

    // The same scale again is not a change.
    let generation = atlas.generation();
    let metrics = atlas.metrics();
    assert!(!atlas.set_scale(1.0));
    assert_eq!(atlas.generation(), generation);
    assert_eq!(atlas.stats().glyphs, 2);

    // A new scale drops every cached glyph, bumps the generation and scales the cell.
    assert!(atlas.set_scale(2.0));
    assert_eq!(atlas.scale(), 2.0);
    assert_ne!(atlas.generation(), generation);
    assert_eq!(atlas.stats().glyphs, 0);
    let doubled = atlas.metrics();
    assert!(
        (doubled.width - 2.0 * metrics.width).abs() <= 1.0,
        "{doubled:?} vs {metrics:?}"
    );
    assert!(
        (doubled.height - 2.0 * metrics.height).abs() <= 2.0,
        "{doubled:?} vs {metrics:?}"
    );

    // The old key no longer names a cached glyph; the new one rasterises at the new size.
    let shaped = atlas.shape("A", REGULAR);
    let rescaled_key = atlas.key(&shaped[0], 0);
    assert_ne!(rescaled_key, key, "scale is part of the key");
    let misses = atlas.stats().misses;
    let big = atlas.glyph(rescaled_key).unwrap();
    assert_eq!(
        atlas.stats().misses,
        misses + 1,
        "re-rasterised after the scale change"
    );
    assert!(big.h > first.h && big.w > first.w, "{big:?} vs {first:?}");
}

#[test]
fn atlas_image_holds_the_rasterised_glyph() {
    let mut atlas = atlas(1.0);
    assert!(atlas.take_dirty().is_none(), "nothing packed yet");
    let shaped = atlas.shape("W", REGULAR);
    let key = atlas.key(&shaped[0], 0);
    let glyph = atlas.glyph(key).unwrap();
    let image = atlas.image();
    assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
    assert!(
        (glyph.x + glyph.w) as u32 <= image.width && (glyph.y + glyph.h) as u32 <= image.height
    );
    assert!(
        ink(&atlas, &glyph) > 0,
        "the glyph's rectangle has coverage"
    );
    assert!(!glyph.color, "an outline glyph is a mask");

    // The rows that changed are reported once, and cover the glyph.
    let (y0, y1) = atlas.take_dirty().expect("packing dirtied the image");
    assert!(y0 <= glyph.y as u32 && y1 >= (glyph.y + glyph.h) as u32);
    assert!(atlas.take_dirty().is_none());
    atlas.glyph(key).unwrap();
    assert!(atlas.take_dirty().is_none(), "a cache hit uploads nothing");

    // Two glyphs never share pixels.
    let other = atlas.shape("M", REGULAR);
    let other_key = atlas.key(&other[0], 0);
    let m = atlas.glyph(other_key).unwrap();
    let apart_x = m.x >= glyph.x + glyph.w || glyph.x >= m.x + m.w;
    let apart_y = m.y >= glyph.y + glyph.h || glyph.y >= m.y + m.h;
    assert!(apart_x || apart_y, "{m:?} overlaps {glyph:?}");
}

#[test]
fn atlas_bold_and_italic_use_other_faces_or_keys() {
    let mut atlas = atlas(1.0);
    let regular = atlas.shape("a", REGULAR);
    let bold = atlas.shape(
        "a",
        FontStyle {
            bold: true,
            italic: false,
        },
    );
    assert_eq!((regular.len(), bold.len()), (1, 1));
    assert_ne!(
        atlas.key(&regular[0], 0),
        atlas.key(&bold[0], 0),
        "bold must not reuse the regular glyph"
    );
}

#[test]
fn fallback_font_lookup() {
    let mut atlas = atlas(1.0);
    let primary = atlas.primary_font().expect("this test needs system fonts");
    let latin = atlas.shape("A", REGULAR);
    assert_eq!(latin[0].font, primary, "ASCII comes from the primary font");
    assert_ne!(latin[0].glyph, 0);

    // Characters a monospace Latin font usually lacks. At least one must be found in another
    // installed font (archbox: DejaVu Sans / DejaVu Math); a machine with a single font fails here.
    let candidates = [
        "\u{283F}",
        "\u{2A0C}",
        "\u{0E01}",
        "\u{05D0}",
        "\u{4E2D}",
        "\u{1D11E}",
        "\u{2603}",
    ];
    let mut found = None;
    for text in candidates {
        let shaped = atlas.shape(text, REGULAR);
        if let Some(g) = shaped.first().filter(|g| g.font != primary && g.glyph != 0) {
            found = Some((text, *g));
            break;
        }
    }
    let (text, shaped) = found.expect("no candidate character was found in a fallback font");
    let key = atlas.key(&shaped, 0);
    assert_eq!(key.font, shaped.font);
    let glyph = atlas
        .glyph(key)
        .unwrap_or_else(|| panic!("fallback glyph for {text:?} rasterises"));
    assert!(glyph.w > 0 && ink(&atlas, &glyph) > 0);

    // A character no font has still shapes to something drawable or to nothing — never a panic.
    let _ = atlas.shape("\u{10FFFD}", REGULAR);
    let _ = atlas.shape("", REGULAR);
}

#[test]
fn atlas_full_starts_a_new_generation() {
    // 200 px glyphs: the printable ASCII set in four subpixel positions and two styles does not
    // fit, so the atlas must start over at least once — and keep serving valid slots.
    let mut atlas = Atlas::new(
        FontSpec {
            family: None,
            size: 200.0,
        },
        1.0,
    );
    let start = atlas.generation();
    let mut last = None;
    for style in [
        REGULAR,
        FontStyle {
            bold: true,
            italic: false,
        },
    ] {
        for subpx in 0..SUBPIXEL_BINS {
            for ch in '!'..='~' {
                let shaped = atlas.shape(&ch.to_string(), style);
                let key = atlas.key(&shaped[0], subpx);
                let glyph = atlas
                    .glyph(key)
                    .unwrap_or_else(|| panic!("{ch:?} rasterises"));
                let image = atlas.image();
                assert!(
                    (glyph.x + glyph.w) as u32 <= image.width
                        && (glyph.y + glyph.h) as u32 <= image.height
                );
                last = Some(glyph);
            }
        }
    }
    assert!(
        atlas.generation() > start,
        "the atlas filled up and restarted"
    );
    assert!(
        ink(&atlas, &last.unwrap()) > 0,
        "the newest glyph is in the image"
    );
    assert!(
        atlas.stats().glyphs < 2 * 4 * 94,
        "the restart dropped the old glyphs"
    );
}

#[test]
fn metrics_give_the_grid_size() {
    let atlas = atlas(1.0);
    let m = atlas.metrics();
    assert!(m.width >= 1.0 && m.height > m.width, "{m:?}");
    assert!(m.baseline > 0.0 && m.baseline < m.height);
    assert!(
        m.underline_y > m.baseline - 1.0 && m.underline_y + m.stroke <= m.height,
        "{m:?}"
    );
    assert_eq!(m.width.fract(), 0.0, "cells sit on whole pixels");
    assert_eq!(m.height.fract(), 0.0);
    let (cols, rows) = m.grid_for((m.width * 80.0) as u32 + 3, (m.height * 24.0) as u32 + 3);
    assert_eq!((cols, rows), (80, 24));
    assert_eq!(m.grid_for(0, 0), (1, 1), "never a zero-sized grid");
}
