//! Quad building: `Frame` → instances.

mod common;

use common::{cell, cursor, frame, lead, spacer, text_frame};
use s2_core::{Cell, CellFlags, CursorShape};
use s2_wgpu::gfx::atlas::{Atlas, FontSpec, FontStyle};
use s2_wgpu::gfx::grid::{
    CellSpan, Instance, KIND_SOLID, Overlays, PHOTON_PX, Preedit, Scene, Theme,
};

const RED: (u8, u8, u8) = (200, 0, 0);

fn atlas() -> Atlas {
    Atlas::new(FontSpec::default(), 1.0)
}

fn rgba(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

fn solids(layer: &[Instance], color: [u8; 4]) -> Vec<Instance> {
    layer
        .iter()
        .copied()
        .filter(|i| i.kind == KIND_SOLID && i.color == color)
        .collect()
}

fn textured(layer: &[Instance]) -> Vec<Instance> {
    layer
        .iter()
        .copied()
        .filter(|i| i.kind != KIND_SOLID)
        .collect()
}

fn with_bg(mut c: Cell, bg: (u8, u8, u8)) -> Cell {
    c.bg = Some(bg);
    c
}

#[test]
fn cell_quads_for_wide_char_span_two_columns() {
    let mut atlas = atlas();
    let m = atlas.metrics();
    let theme = Theme::default();
    let mut scene = Scene::new();

    // a | 好 (lead with a red background + its spacer) | a
    // The spacer carries no colour of its own here: the lead alone must cover both columns.
    let f = frame(vec![vec![
        cell("a"),
        with_bg(lead("好"), RED),
        spacer(),
        cell("a"),
    ]]);
    assert_eq!(scene.update_cells(&f, &mut atlas, &theme), 1);

    // One background quad for the wide cell: it starts at column 1 and is two columns wide.
    // The spacer adds no quad of its own.
    let bg = scene.bg();
    assert_eq!(bg.len(), 1, "{bg:?}");
    assert_eq!(bg[0].pos, [m.width, 0.0]);
    assert_eq!(bg[0].size, [2.0 * m.width, m.height]);
    assert_eq!(bg[0].color, [RED.0, RED.1, RED.2, 255]);
    // A spacer that repeats the lead's background changes nothing.
    let same = frame(vec![vec![
        cell("a"),
        with_bg(lead("好"), RED),
        with_bg(spacer(), RED),
        cell("a"),
    ]]);
    let mut other = Scene::new();
    other.update_cells(&same, &mut atlas, &theme);
    assert_eq!(other.bg(), bg);
    assert_eq!(other.glyphs(), scene.glyphs());

    // The narrow cell after the wide one is drawn at column 3, not column 2: the two 'a' glyphs
    // are the same atlas slot, exactly three columns apart.
    let glyphs = textured(scene.glyphs());
    let a: Vec<&Instance> = glyphs.iter().filter(|g| g.uv == glyphs[0].uv).collect();
    assert_eq!(a.len(), 2, "{glyphs:?}");
    assert_eq!(a[1].pos[0] - a[0].pos[0], 3.0 * m.width);
    assert_eq!(a[1].pos[1], a[0].pos[1]);
    // Whatever the wide character's glyph is (a CJK font may be missing here: then it is the
    // font's missing-glyph box or nothing), it starts inside the lead's two columns.
    assert!(glyphs.len() <= 3);
    for g in glyphs.iter().filter(|g| g.uv != glyphs[0].uv) {
        assert!(
            g.pos[0] >= m.width - 2.0 && g.pos[0] < 3.0 * m.width,
            "{g:?}"
        );
    }

    // A block cursor on the lead covers both columns — and so does one on the spacer, which is
    // where the emulator can report it.
    for col in [1, 2] {
        let mut f = f.clone();
        f.cursor = cursor(col, 0, CursorShape::Block);
        scene.update(&f, &mut atlas, &theme, &Overlays::default());
        let block = solids(scene.overlay(), rgba(theme.cursor));
        assert_eq!(block.len(), 1, "cursor at column {col}");
        assert_eq!(
            (block[0].pos, block[0].size),
            ([m.width, 0.0], [2.0 * m.width, m.height]),
            "cursor at column {col}"
        );
    }
}

#[test]
fn wide_lead_in_the_last_column_and_stray_spacers_do_not_panic() {
    let mut atlas = atlas();
    let m = atlas.metrics();
    let mut scene = Scene::new();
    // A spacer with no lead before it, and a lead with no room for its spacer.
    let mut f = frame(vec![vec![
        with_bg(spacer(), RED),
        cell("x"),
        with_bg(lead("好"), RED),
    ]]);
    f.cursor = cursor(0, 0, CursorShape::Block);
    scene.update(&f, &mut atlas, &Theme::default(), &Overlays::default());
    for quad in scene.bg() {
        assert!(
            quad.pos[0] + quad.size[0] <= 3.0 * m.width,
            "clipped to the row: {quad:?}"
        );
    }
    assert_eq!(scene.bg().len(), 2);
}

#[test]
fn backgrounds_merge_into_runs_and_default_background_draws_nothing() {
    let mut atlas = atlas();
    let m = atlas.metrics();
    let theme = Theme::default();
    let mut scene = Scene::new();
    let green = (0, 200, 0);
    let f = frame(vec![
        vec![
            cell("a"),
            with_bg(cell("b"), RED),
            with_bg(cell(""), RED),
            with_bg(cell("c"), green),
            cell(""),
        ],
        vec![
            Cell::default(),
            Cell::default(),
            Cell::default(),
            Cell::default(),
            with_bg(Cell::default(), RED),
        ],
    ]);
    scene.update_cells(&f, &mut atlas, &theme);
    let bg = scene.bg();
    assert_eq!(bg.len(), 3, "{bg:?}");
    assert_eq!(
        (bg[0].pos, bg[0].size, bg[0].color),
        ([m.width, 0.0], [2.0 * m.width, m.height], [200, 0, 0, 255])
    );
    assert_eq!(
        (bg[1].pos, bg[1].size, bg[1].color),
        ([3.0 * m.width, 0.0], [m.width, m.height], [0, 200, 0, 255])
    );
    assert_eq!(
        (bg[2].pos, bg[2].size),
        ([4.0 * m.width, m.height], [m.width, m.height])
    );
    assert_eq!(
        textured(scene.glyphs()).len(),
        3,
        "blank cells have no glyph"
    );
    assert!(
        textured(scene.glyphs())
            .iter()
            .all(|g| g.color == rgba(theme.fg))
    );
}

#[test]
fn cell_flags_change_colours_and_add_lines() {
    let mut atlas = atlas();
    let m = atlas.metrics();
    let theme = Theme::default();
    let mut scene = Scene::new();
    let flagged = |flags: CellFlags| Cell { flags, ..cell("x") };
    let f = frame(vec![vec![
        flagged(CellFlags {
            inverse: true,
            ..CellFlags::default()
        }),
        flagged(CellFlags {
            faint: true,
            ..CellFlags::default()
        }),
        flagged(CellFlags {
            underline: true,
            ..CellFlags::default()
        }),
        flagged(CellFlags {
            strike: true,
            ..CellFlags::default()
        }),
        Cell {
            fg: Some(RED),
            ..cell("x")
        },
    ]]);
    scene.update_cells(&f, &mut atlas, &theme);

    // Inverse: the default colours swap, so the cell gets a background quad in the foreground colour.
    let bg = scene.bg();
    assert_eq!(bg.len(), 1);
    assert_eq!((bg[0].pos, bg[0].color), ([0.0, 0.0], rgba(theme.fg)));
    let glyphs = textured(scene.glyphs());
    assert_eq!(glyphs.len(), 5);
    assert_eq!(glyphs[0].color, rgba(theme.bg));
    // Faint: between foreground and background.
    assert!(
        glyphs[1].color != rgba(theme.fg) && glyphs[1].color != rgba(theme.bg),
        "{:?}",
        glyphs[1].color
    );
    assert_eq!(glyphs[4].color, [200, 0, 0, 255]);

    // Underline and strike are solid quads one cell wide in the glyph layer, in the text colour.
    let lines = solids(scene.glyphs(), rgba(theme.fg));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
        (lines[0].pos, lines[0].size),
        ([2.0 * m.width, m.underline_y], [m.width, m.stroke])
    );
    assert_eq!(
        (lines[1].pos, lines[1].size),
        ([3.0 * m.width, m.strike_y], [m.width, m.stroke])
    );
    assert!(m.strike_y < m.baseline && m.underline_y >= m.baseline);

    // On a wide cell the underline runs under both columns.
    let wide = Cell {
        flags: CellFlags {
            underline: true,
            ..CellFlags::default()
        },
        ..lead("好")
    };
    scene.update_cells(&frame(vec![vec![wide, spacer()]]), &mut atlas, &theme);
    let lines = solids(scene.glyphs(), rgba(theme.fg));
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].size, [2.0 * m.width, m.stroke]);
}

#[test]
fn only_dirty_rows_are_rebuilt() {
    let mut atlas = atlas();
    let theme = Theme::default();
    let mut scene = Scene::new();
    let first = text_frame(&["aaa", "bbb", "ccc"], 4);
    assert_eq!(
        scene.update_cells(&first, &mut atlas, &theme),
        3,
        "a new scene builds every row"
    );
    assert_eq!(scene.glyphs().len(), 9);
    let row0: Vec<Instance> = scene.glyphs()[..3].to_vec();

    // Row 1 changes and says so; row 2 changes too but is NOT marked dirty.
    let mut second = text_frame(&["aaa", "x", ""], 4);
    second.dirty_rows = vec![false, true, false];
    assert_eq!(scene.update_cells(&second, &mut atlas, &theme), 1);
    assert_eq!(
        scene.glyphs().len(),
        3 + 1 + 3,
        "row 1 rebuilt, row 2 still shows its old quads"
    );
    assert_eq!(&scene.glyphs()[..3], &row0[..]);

    // Nothing dirty: nothing rebuilt.
    second.dirty_rows = vec![false; 3];
    assert_eq!(scene.update_cells(&second, &mut atlas, &theme), 0);
    assert_eq!(scene.glyphs().len(), 7);

    second.dirty_rows = vec![false, false, true];
    assert_eq!(scene.update_cells(&second, &mut atlas, &theme), 1);
    assert_eq!(scene.glyphs().len(), 4);
}

#[test]
fn size_scale_theme_and_origin_changes_rebuild_everything() {
    let mut atlas = atlas();
    let theme = Theme::default();
    let mut scene = Scene::new();
    let clean = |lines: &[&str], cols| {
        let mut f = text_frame(lines, cols);
        f.dirty_rows.fill(false);
        f
    };
    scene.update_cells(&text_frame(&["ab", "cd"], 3), &mut atlas, &theme);

    // Another grid size, no row marked dirty.
    assert_eq!(
        scene.update_cells(&clean(&["ab", "cd", "ef"], 5), &mut atlas, &theme),
        3
    );
    assert_eq!(scene.glyphs().len(), 6);

    // Scale change: the atlas is a new generation, every quad is rebuilt at the new cell size.
    let before = scene.glyphs()[1];
    let m1 = atlas.metrics();
    atlas.set_scale(2.0);
    let m2 = atlas.metrics();
    assert_eq!(
        scene.update_cells(&clean(&["ab", "cd", "ef"], 5), &mut atlas, &theme),
        3
    );
    let after = scene.glyphs()[1];
    assert!(after.size[1] > before.size[1], "{after:?} vs {before:?}");
    assert!(after.pos[0] >= m2.width - 2.0 && before.pos[0] < m2.width && m2.width > m1.width);

    // Theme change.
    let light = Theme {
        fg: [0, 0, 0],
        bg: [255, 255, 255],
        ..theme
    };
    assert_eq!(
        scene.update_cells(&clean(&["ab", "cd", "ef"], 5), &mut atlas, &light),
        3
    );
    assert!(scene.glyphs().iter().all(|g| g.color == [0, 0, 0, 255]));

    // Origin change (the sidebar of the next task moves the grid).
    let at_zero = scene.glyphs()[0];
    scene.set_origin(240.0, 10.0);
    assert_eq!(
        scene.update_cells(&clean(&["ab", "cd", "ef"], 5), &mut atlas, &light),
        3
    );
    let moved = scene.glyphs()[0];
    assert_eq!(
        [moved.pos[0] - at_zero.pos[0], moved.pos[1] - at_zero.pos[1]],
        [240.0, 10.0]
    );
    assert_eq!(
        scene.cell_at(240.0 + m2.width * 1.5, 10.0 + m2.height * 2.5),
        Some((1, 2))
    );
    assert_eq!(scene.cell_at(239.0, 10.0), None);
    assert_eq!(
        scene.cell_at(240.0 + m2.width * 5.0, 10.0),
        None,
        "right of the last column"
    );
}

#[test]
fn cursor_overlay_quad() {
    let mut atlas = atlas();
    let m = atlas.metrics();
    let theme = Theme::default();
    let mut scene = Scene::new();
    let mut f = text_frame(&["abc", "def"], 4);
    let none = Overlays::default();
    let cursor_colour = rgba(theme.cursor);

    scene.update(&f, &mut atlas, &theme, &none);
    assert!(scene.overlay().is_empty(), "no cursor, no overlay");

    // Block: the whole cell, and the glyph under it again in the background colour.
    f.cursor = cursor(1, 1, CursorShape::Block);
    scene.update_overlays(&f, &mut atlas, &theme, &none);
    let block = solids(scene.overlay(), cursor_colour);
    assert_eq!(block.len(), 1);
    assert_eq!(
        (block[0].pos, block[0].size),
        ([m.width, m.height], [m.width, m.height])
    );
    let under = textured(scene.overlay());
    assert_eq!(under.len(), 1, "the 'e' under the block");
    assert_eq!(under[0].color, rgba(theme.bg));
    assert_eq!(
        scene.overlay().last().unwrap().kind,
        under[0].kind,
        "the glyph is drawn after the block"
    );

    // Bar: a thin full-height strip at the cell's left edge.
    f.cursor = cursor(2, 0, CursorShape::Bar);
    scene.update_overlays(&f, &mut atlas, &theme, &none);
    let bar = solids(scene.overlay(), cursor_colour);
    assert_eq!(scene.overlay().len(), 1);
    assert_eq!(bar[0].pos, [2.0 * m.width, 0.0]);
    assert!(
        bar[0].size[0] >= 1.0 && bar[0].size[0] <= m.width / 3.0 && bar[0].size[1] == m.height,
        "{bar:?}"
    );

    // Underline: a thin full-width strip at the cell's bottom edge.
    f.cursor = cursor(0, 1, CursorShape::Underline);
    scene.update_overlays(&f, &mut atlas, &theme, &none);
    let line = solids(scene.overlay(), cursor_colour);
    assert_eq!(scene.overlay().len(), 1);
    assert_eq!(line[0].size[0], m.width);
    assert!(line[0].size[1] >= 1.0 && line[0].size[1] <= m.height / 3.0);
    assert_eq!(line[0].pos[1] + line[0].size[1], 2.0 * m.height);

    // Hidden, or outside the grid (a stale cursor after a resize): nothing, and no panic.
    for (col, row, visible) in [
        (1, 1, false),
        (4, 0, true),
        (0, 2, true),
        (u16::MAX, u16::MAX, true),
    ] {
        f.cursor = Some(s2_core::Cursor {
            col,
            row,
            shape: CursorShape::Block,
            visible,
        });
        scene.update_overlays(&f, &mut atlas, &theme, &none);
        assert!(
            scene.overlay().is_empty(),
            "cursor ({col},{row}) visible={visible}"
        );
    }
}

#[test]
fn preedit_selection_and_photon_overlays() {
    let mut atlas = atlas();
    let m = atlas.metrics();
    let theme = Theme::default();
    let mut scene = Scene::new();
    let mut f = text_frame(&["abcdefgh", ""], 8);
    f.cursor = cursor(1, 0, CursorShape::Bar);

    // Preedit "a好" at the cursor: three columns, backed by the background colour so the cells
    // under it do not show through, with one underline run below it.
    let mut overlays = Overlays {
        preedit: Some(Preedit {
            text: "a好".into()
        }),
        ..Overlays::default()
    };
    scene.update(&f, &mut atlas, &theme, &overlays);
    let backing = solids(scene.overlay(), rgba(theme.bg));
    assert_eq!(backing.len(), 1, "{:?}", scene.overlay());
    assert_eq!(
        (backing[0].pos, backing[0].size),
        ([m.width, 0.0], [3.0 * m.width, m.height])
    );
    let underline: Vec<Instance> = solids(scene.overlay(), rgba(theme.fg))
        .into_iter()
        .filter(|q| q.size[0] == 3.0 * m.width)
        .collect();
    assert_eq!(underline.len(), 1, "{:?}", scene.overlay());
    assert_eq!(
        (underline[0].pos, underline[0].size[1]),
        ([m.width, m.underline_y], m.stroke)
    );
    assert!(
        !textured(scene.overlay()).is_empty(),
        "the preedit text itself"
    );
    // The cursor moves to the end of the preedit.
    let bar = solids(scene.overlay(), rgba(theme.cursor));
    assert_eq!(bar.len(), 1);
    assert_eq!(bar[0].pos, [4.0 * m.width, 0.0]);

    // A preedit longer than the rest of the row is clipped at the last column.
    overlays.preedit = Some(Preedit {
        text: "好好好好好好".into(),
    });
    f.cursor = cursor(5, 0, CursorShape::Bar);
    scene.update_overlays(&f, &mut atlas, &theme, &overlays);
    for quad in scene.overlay() {
        assert!(quad.pos[0] < 8.0 * m.width, "{quad:?}");
    }
    let backing = solids(scene.overlay(), rgba(theme.bg));
    assert_eq!(
        backing[0].size[0],
        2.0 * m.width,
        "one wide character fits in the three columns left; the next does not"
    );

    // No cursor (scrolled back): the preedit has nowhere to go and is not drawn.
    f.cursor = None;
    scene.update_overlays(&f, &mut atlas, &theme, &overlays);
    assert!(scene.overlay().is_empty());

    // Selection spans: translucent quads, end column exclusive, clipped to the grid.
    let overlays = Overlays {
        selection: vec![
            CellSpan {
                row: 0,
                col_start: 2,
                col_end: 5,
            },
            CellSpan {
                row: 1,
                col_start: 6,
                col_end: 99,
            },
            CellSpan {
                row: 7,
                col_start: 0,
                col_end: 2,
            },
        ],
        ..Overlays::default()
    };
    scene.update_overlays(&f, &mut atlas, &theme, &overlays);
    let selected = solids(scene.overlay(), theme.selection);
    assert_eq!(selected.len(), 2);
    assert_eq!(
        (selected[0].pos, selected[0].size),
        ([2.0 * m.width, 0.0], [3.0 * m.width, m.height])
    );
    assert_eq!(
        (selected[1].pos, selected[1].size),
        ([6.0 * m.width, m.height], [2.0 * m.width, m.height])
    );

    // Photon square: 64 x 64 px in the window's top-left corner (not moved by the grid origin),
    // white or black, the last instance so nothing covers it.
    scene.set_origin(100.0, 50.0);
    for (on, colour) in [(true, [255, 255, 255, 255]), (false, [0, 0, 0, 255])] {
        let overlays = Overlays {
            photon: Some(on),
            ..overlays.clone()
        };
        scene.update(&f, &mut atlas, &theme, &overlays);
        let square = *scene.overlay().last().unwrap();
        assert_eq!(
            (square.kind, square.pos, square.size, square.color),
            (KIND_SOLID, [0.0, 0.0], [PHOTON_PX, PHOTON_PX], colour)
        );
        let layers = scene.layers();
        assert_eq!(layers[2].last(), Some(&square));
    }
    assert_eq!(PHOTON_PX, 64.0);
}

#[test]
fn layers_are_background_then_glyphs_then_overlay() {
    let mut atlas = atlas();
    let theme = Theme::default();
    let mut scene = Scene::new();
    let mut f = frame(vec![vec![with_bg(cell("a"), RED), cell("b")]]);
    f.cursor = cursor(1, 0, CursorShape::Underline);
    scene.update(&f, &mut atlas, &theme, &Overlays::default());
    let [bg, glyphs, overlay] = scene.layers();
    assert_eq!(
        (bg, glyphs, overlay),
        (scene.bg(), scene.glyphs(), scene.overlay())
    );
    assert_eq!((bg.len(), glyphs.len(), overlay.len()), (1, 2, 1));
    assert_eq!(
        std::mem::size_of::<Instance>(),
        40,
        "the vertex layout in render.rs assumes this"
    );
}

/// 200 px glyphs: the atlas is full after a couple of hundred.
fn big_atlas() -> Atlas {
    Atlas::new(
        FontSpec {
            family: None,
            size: 200.0,
        },
        1.0,
    )
}

/// Rasterises glyphs no test frame uses (subpixel bins 1..) until the atlas starts over or
/// `stop_after` are in; returns how many fitted.
fn fill_big_atlas(atlas: &mut Atlas, stop_after: usize) -> usize {
    let start = atlas.generation();
    let mut done = 0;
    for bin in 1..4u8 {
        for bold in [false, true] {
            for ch in '!'..='~' {
                let shaped = atlas.shape(
                    &ch.to_string(),
                    FontStyle {
                        bold,
                        italic: false,
                    },
                );
                let key = atlas.key(&shaped[0], bin);
                atlas.glyph(key);
                if atlas.generation() != start || done == stop_after {
                    return done;
                }
                done += 1;
            }
        }
    }
    panic!("the atlas never filled up");
}

#[test]
fn atlas_restart_during_an_update_leaves_no_stale_quads() {
    let capacity = fill_big_atlas(&mut big_atlas(), usize::MAX);
    assert!(capacity > 20);

    // Ten glyphs short of full; a 26-letter row then overflows it in the middle of the row.
    let mut atlas = big_atlas();
    fill_big_atlas(&mut atlas, capacity - 10);
    let generation = atlas.generation();
    let theme = Theme::default();
    let f = text_frame(&["abcdefghijklmnopqrstuvwxyz"], 26);
    let mut scene = Scene::new();
    assert_eq!(
        scene.update(&f, &mut atlas, &theme, &Overlays::default()),
        1
    );
    assert_ne!(
        atlas.generation(),
        generation,
        "the update restarted the atlas"
    );
    assert_eq!(scene.glyphs().len(), 26);

    // Every quad matches what a fresh scene builds from the atlas as it is now.
    let generation = atlas.generation();
    let mut fresh = Scene::new();
    fresh.update(&f, &mut atlas, &theme, &Overlays::default());
    assert_eq!(atlas.generation(), generation, "all cache hits");
    assert_eq!(scene.glyphs(), fresh.glyphs());
}

#[test]
fn origin_is_snapped_to_whole_pixels() {
    // A fractional origin would put every glyph quad between pixels: the atlas texels are copied
    // 1:1 (no filtering), so the glyphs would be resampled and lose their hinting.
    let mut atlas = atlas();
    let theme = Theme::default();
    let m = atlas.metrics();
    let mut f = text_frame(&["ab", "cd"], 4);
    f.cells[1].bg = Some(RED);
    f.cursor = cursor(1, 1, CursorShape::Block);
    let mut scene = Scene::new();
    scene.set_origin(80.4, 8.6);
    let overlays = Overlays {
        selection: vec![CellSpan {
            row: 0,
            col_start: 0,
            col_end: 2,
        }],
        ..Overlays::default()
    };
    scene.update(&f, &mut atlas, &theme, &overlays);
    assert!(!scene.glyphs().is_empty() && !scene.overlay().is_empty());
    for layer in scene.layers() {
        for quad in layer {
            assert_eq!(
                [quad.pos[0].fract(), quad.pos[1].fract()],
                [0.0, 0.0],
                "{quad:?}"
            );
        }
    }
    // The grid starts at the rounded origin, and hit-testing agrees with what is drawn.
    assert_eq!(
        solids(scene.bg(), rgba([200, 0, 0]))[0].pos,
        [80.0 + m.width, 9.0]
    );
    assert_eq!(scene.cell_at(80.0, 9.0), Some((0, 0)));
    assert_eq!(scene.cell_at(79.5, 9.0), None);
    assert_eq!(scene.cell_at(80.0, 8.5), None);
}

#[test]
fn overlay_glyphs_that_restart_the_atlas_leave_no_stale_cell_quads() {
    // The IME path: cells are already built, then `update_overlays` alone is called with new
    // preedit text whose glyphs fill the atlas and restart it.
    let capacity = fill_big_atlas(&mut big_atlas(), usize::MAX);
    let mut atlas = big_atlas();
    fill_big_atlas(&mut atlas, capacity - 10);
    let theme = Theme::default();
    let mut f = text_frame(&["abc"], 40);
    f.cursor = cursor(4, 0, CursorShape::Bar);
    let mut scene = Scene::new();
    let generation = atlas.generation();
    scene.update(&f, &mut atlas, &theme, &Overlays::default());
    assert_eq!(atlas.generation(), generation, "the cells still fit");
    let cells_before = scene.glyphs().to_vec();
    assert_eq!(cells_before.len(), 3);

    // Thirty glyphs the atlas has not seen (subpixel bin 0): more than the room that is left.
    let overlays = Overlays {
        preedit: Some(Preedit {
            text: ('A'..='Z').chain('0'..='3').collect(),
        }),
        ..Overlays::default()
    };
    scene.update_overlays(&f, &mut atlas, &theme, &overlays);
    assert_ne!(
        atlas.generation(),
        generation,
        "the preedit restarted the atlas"
    );

    // No caller-side generation check: the scene must already match a fresh build.
    let generation = atlas.generation();
    let mut fresh = Scene::new();
    fresh.update(&f, &mut atlas, &theme, &overlays);
    assert_eq!(atlas.generation(), generation, "all cache hits");
    assert_ne!(
        scene.glyphs(),
        cells_before.as_slice(),
        "the cells were rebuilt"
    );
    assert_eq!(scene.glyphs(), fresh.glyphs());
    assert_eq!(scene.overlay(), fresh.overlay());
}
