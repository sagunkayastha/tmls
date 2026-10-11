//! The UI as quads: labels stay inside the sidebar, the field shows what is being composed,
//! focus and theme tokens are visible, cached layers follow the atlas.

mod common;

use common::{cursor, text_frame};
use s2_core::CursorShape;
use s2_wgpu::gfx::atlas::{Atlas, FontSpec, FontStyle};
use s2_wgpu::gfx::grid::{Instance, KIND_SOLID, Overlays, Preedit, Scene};
use s2_wgpu::gfx::render::{Gpu, OFFSCREEN_FORMAT, Renderer, render_offscreen};
use s2_wgpu::ui::Ui;
use s2_wgpu::ui::draw::{UiLayers, compose, fit, layer_order};
use s2_wgpu::ui::focus::Focus;
use s2_wgpu::ui::layout::{Layout, Rect, layout};
use s2_wgpu::ui::list::List;

fn atlas(scale: f32) -> Atlas {
    let atlas = Atlas::new(FontSpec::default(), scale);
    assert!(
        atlas.primary_font().is_some(),
        "these tests need at least one system font"
    );
    atlas
}

fn rig(scale: f32, window: (u32, u32)) -> (Ui, Atlas, Layout) {
    let ui = Ui::new();
    let atlas = atlas(scale);
    let layout = layout(window, scale, &atlas.metrics(), &ui.theme);
    (ui, atlas, layout)
}

fn built(ui: &mut Ui, layout: &Layout, atlas: &mut Atlas) -> UiLayers {
    let mut layers = UiLayers::new();
    assert!(layers.update(ui, layout, atlas), "the first update builds");
    layers
}

fn rgba(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

fn glyphs(layer: &[Instance]) -> Vec<Instance> {
    layer
        .iter()
        .filter(|i| i.kind != KIND_SOLID)
        .copied()
        .collect()
}

fn solids(layer: &[Instance], color: [u8; 4]) -> Vec<Instance> {
    layer
        .iter()
        .filter(|i| i.kind == KIND_SOLID && i.color == color)
        .copied()
        .collect()
}

fn within(i: &Instance, r: Rect) -> bool {
    i.pos[0] >= r.x
        && i.pos[1] >= r.y
        && i.pos[0] + i.size[0] <= r.right()
        && i.pos[1] + i.size[1] <= r.bottom()
}

#[test]
fn fit_truncates_by_columns_with_an_ellipsis() {
    let cols = |text: &str, n: usize| -> Vec<(String, usize)> { fit(text, n) };
    let s = |v: &[(&str, usize)]| -> Vec<(String, usize)> {
        v.iter().map(|(t, c)| (t.to_string(), *c)).collect()
    };
    assert_eq!(cols("abc", 5), s(&[("a", 0), ("b", 1), ("c", 2)]));
    assert_eq!(
        cols("abc", 3),
        s(&[("a", 0), ("b", 1), ("c", 2)]),
        "fits exactly"
    );
    assert_eq!(
        cols("abcdef", 4),
        s(&[("a", 0), ("b", 1), ("c", 2), ("…", 3)])
    );
    // Wide characters take two columns and are never cut in half.
    assert_eq!(cols("你好", 4), s(&[("你", 0), ("好", 2)]));
    assert_eq!(cols("你好吗", 4), s(&[("你", 0), ("…", 2)]));
    assert_eq!(cols("a你好", 4), s(&[("a", 0), ("你", 1), ("…", 3)]));
    assert_eq!(
        cols("a你好", 3),
        s(&[("a", 0), ("…", 1)]),
        "no room for half"
    );
    // Combining marks stay with their base; control characters are not drawn.
    assert_eq!(
        cols("e\u{301}x\ty", 5),
        s(&[("e\u{301}", 0), ("x", 1), ("y", 2)])
    );
    assert_eq!(cols("abc", 1), s(&[("…", 0)]));
    assert_eq!(cols("abc", 0), s(&[]));
    assert_eq!(cols("", 5), s(&[]));
}

#[test]
fn sidebar_labels_never_leave_the_sidebar() {
    // Glyphs are not clipped by the renderer, so a label has to be cut to what fits — at an
    // odd scale, with names far too long, wide characters included.
    for scale in [1.0, 1.25, 1.75] {
        let (mut ui, mut atlas, layout) = rig(scale, (1400, 900));
        let mut names: Vec<String> = (0..40)
            .map(|i| format!("session-{i}-with-a-very-long-name-that-cannot-possibly-fit-{i}"))
            .collect();
        names[1] = "宽字符会话名称非常非常非常非常非常非常非常非常长".to_string();
        names[2] = "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW".to_string();
        ui.list = List::new(names);
        ui.field
            .insert(&"field text that is much longer than the field ".repeat(4));
        ui.set_focus(Focus::Field);
        let layers = built(&mut ui, &layout, &mut atlas);
        let drawn = glyphs(layers.base());
        assert!(drawn.len() > 100, "labels were drawn: {}", drawn.len());
        for glyph in &drawn {
            assert!(
                within(glyph, layout.sidebar),
                "scale {scale}: {glyph:?} outside {:?}",
                layout.sidebar
            );
        }
        // Nothing of the base layer reaches into the terminal at all.
        for quad in layers.base() {
            assert!(
                quad.pos[0] + quad.size[0] <= layout.terminal.x && quad.pos[0] >= 0.0,
                "scale {scale}: {quad:?}"
            );
            assert_eq!(quad.pos, quad.pos.map(f32::round), "whole pixels: {quad:?}");
        }
        assert!(layers.top().is_empty(), "no menu");
        // Rows below the form are not drawn: every list glyph is above the rule.
        let list_glyphs = drawn
            .iter()
            .filter(|g| g.pos[1] < layout.form_rule.y)
            .count();
        assert!(list_glyphs > 0);
        assert!(
            drawn
                .iter()
                .all(|g| g.pos[1] + g.size[1] <= layout.form_rule.y
                    || g.pos[1] >= layout.form_rule.y)
        );
    }
}

#[test]
fn the_field_shows_text_selection_caret_and_composition() {
    let (mut ui, mut atlas, layout) = rig(1.25, (1400, 900));
    let m = atlas.metrics();
    let theme = ui.theme.clone();
    let field_glyphs = |layers: &UiLayers| -> Vec<Instance> {
        glyphs(layers.base())
            .into_iter()
            .filter(|g| within(g, layout.field))
            .collect()
    };
    let carets = |layers: &UiLayers| solids(layers.base(), rgba(theme.caret));

    // Unfocused and empty: no caret, no text.
    let layers = built(&mut ui, &layout, &mut atlas);
    assert!(field_glyphs(&layers).is_empty());
    assert!(carets(&layers).is_empty());

    // Focused: a caret at the start of the text area, full cell height.
    ui.set_focus(Focus::Field);
    let layers = built(&mut ui, &layout, &mut atlas);
    let caret = carets(&layers);
    assert_eq!(caret.len(), 1);
    assert_eq!(caret[0].pos, [layout.field_text.x, layout.field_text.y]);
    assert_eq!(caret[0].size[1], m.height);
    assert!(caret[0].size[0] >= 1.0 && caret[0].size[0] < m.width);

    // Text: one glyph per character, the caret after it.
    ui.field.insert("abc");
    let layers = built(&mut ui, &layout, &mut atlas);
    assert_eq!(field_glyphs(&layers).len(), 3);
    assert_eq!(
        carets(&layers)[0].pos[0],
        layout.field_text.x + 3.0 * m.width
    );

    // A selection: one translucent rectangle over the selected columns.
    ui.field.left(true);
    ui.field.left(true);
    let layers = built(&mut ui, &layout, &mut atlas);
    let selection = solids(layers.base(), theme.field_selection);
    assert_eq!(selection.len(), 1);
    assert_eq!(
        (selection[0].pos, selection[0].size),
        (
            [layout.field_text.x + m.width, layout.field_text.y],
            [2.0 * m.width, m.height]
        )
    );
    ui.field.end(false);

    // Composing: the preedit is drawn at the caret with an underline, the caret after it.
    ui.field.set_preedit("xy");
    let layers = built(&mut ui, &layout, &mut atlas);
    assert_eq!(field_glyphs(&layers).len(), 5);
    let underline: Vec<Instance> = solids(layers.base(), rgba(theme.field_fg))
        .into_iter()
        .filter(|q| q.size[1] == m.stroke)
        .collect();
    assert_eq!(underline.len(), 1);
    assert_eq!(underline[0].pos[0], layout.field_text.x + 3.0 * m.width);
    assert_eq!(underline[0].size[0], 2.0 * m.width);
    assert!(within(&underline[0], layout.field));
    assert_eq!(
        carets(&layers)[0].pos[0],
        layout.field_text.x + 5.0 * m.width
    );

    // A full field: the composition is still drawn (the terminal would drop it), everything
    // stays inside the field.
    ui.field.set_preedit("");
    ui.field.insert(&"m".repeat(layout.field_cols + 7));
    ui.field.set_preedit("composing");
    let layers = built(&mut ui, &layout, &mut atlas);
    let underline: Vec<Instance> = solids(layers.base(), rgba(theme.field_fg))
        .into_iter()
        .filter(|q| q.size[1] == m.stroke)
        .collect();
    assert_eq!(underline.len(), 1);
    assert_eq!(underline[0].size[0], 9.0 * m.width, "all nine columns");
    assert_eq!(
        underline[0].pos[0] + underline[0].size[0],
        layout.field_text.right(),
        "its end is at the right edge"
    );
    assert_eq!(field_glyphs(&layers).len(), layout.field_cols);
    let caret = carets(&layers);
    assert!(within(&caret[0], layout.field), "{:?}", caret[0]);
    for glyph in glyphs(layers.base()) {
        assert!(within(&glyph, layout.sidebar));
    }
}

#[test]
fn focus_selection_and_theme_tokens_are_visible() {
    let (mut ui, mut atlas, layout) = rig(1.0, (1280, 800));
    let theme = ui.theme.clone();
    let ring = |ui: &mut Ui, atlas: &mut Atlas| {
        let layers = built(ui, &layout, atlas);
        solids(layers.base(), rgba(theme.focus_ring))
    };
    // Each focus stop shows the ring somewhere else.
    let on_terminal = ring(&mut ui, &mut atlas);
    assert!(!on_terminal.is_empty());
    assert!(
        on_terminal
            .iter()
            .all(|q| q.pos[0] >= layout.sidebar.right())
    );
    ui.set_focus(Focus::List);
    let on_list = ring(&mut ui, &mut atlas);
    assert!(!on_list.is_empty());
    assert!(on_list.iter().all(|q| within(q, layout.sidebar)));
    assert!(on_list.iter().all(|q| q.pos[1] < layout.form_rule.y));
    ui.set_focus(Focus::Field);
    let on_field = ring(&mut ui, &mut atlas);
    assert!(!on_field.is_empty());
    assert!(on_field.iter().all(|q| q.pos[1] >= layout.field.y - 2.0));
    assert_ne!(on_list, on_field);

    // The selected row has its own background, on the row that is selected.
    ui.list.click_row(3);
    let layers = built(&mut ui, &layout, &mut atlas);
    let row = solids(layers.base(), rgba(theme.item_selected_bg));
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].pos[1], layout.list_row(3).y);
    // Scrolled out of view (a window too short for all rows): not drawn.
    let short = s2_wgpu::ui::layout::layout((1280, 300), 1.0, &atlas.metrics(), &ui.theme);
    assert!(short.visible_rows > 3 && short.visible_rows + 10 < ui.list.items().len());
    let shown = built(&mut ui, &short, &mut atlas);
    assert_eq!(solids(shown.base(), rgba(theme.item_selected_bg)).len(), 1);
    assert!(ui.list.scroll(10, short.visible_rows));
    let hidden = built(&mut ui, &short, &mut atlas);
    assert!(solids(hidden.base(), rgba(theme.item_selected_bg)).is_empty());
    ui.list.scroll(-10, short.visible_rows);
    let layers = built(&mut ui, &layout, &mut atlas);

    // Colours come from the tokens: change one, and the quads change with it.
    let sidebar = solids(layers.base(), rgba(theme.sidebar_bg));
    assert_eq!(sidebar[0].pos, [0.0, 0.0]);
    assert_eq!(sidebar[0].size, [layout.sidebar.w, layout.sidebar.h]);
    assert_eq!(
        layers.base()[0],
        sidebar[0],
        "drawn first, under everything"
    );
    ui.theme.sidebar_bg = [1, 2, 3];
    ui.theme.sidebar_fg = [200, 100, 50];
    let layers = built(&mut ui, &layout, &mut atlas);
    assert!(solids(layers.base(), rgba(theme.sidebar_bg)).is_empty());
    assert_eq!(solids(layers.base(), [1, 2, 3, 255]).len(), 1);
    assert!(
        glyphs(layers.base())
            .iter()
            .any(|g| g.color == [200, 100, 50, 255])
    );
}

#[test]
fn the_menu_is_drawn_on_top_and_only_while_open() {
    let (mut ui, mut atlas, layout) = rig(1.25, (1400, 900));
    let theme = ui.theme.clone();
    let layers = built(&mut ui, &layout, &mut atlas);
    assert!(layers.top().is_empty());
    let base = layers.base().to_vec();

    ui.context(layout.terminal.x + 100.0, 200.0, &layout);
    let menu = ui.menu.expect("open");
    let layers = built(&mut ui, &layout, &mut atlas);
    assert_eq!(layers.base(), base.as_slice(), "the sidebar is as before");
    let top = layers.top();
    // Border first, then the panel inside it, then the labels: 14 letters.
    assert_eq!(top[0].color, rgba(theme.menu_border));
    assert_eq!(
        (top[0].pos, top[0].size),
        (
            [menu.rect().x, menu.rect().y],
            [menu.rect().w, menu.rect().h]
        )
    );
    assert_eq!(top[1].color, rgba(theme.menu_bg));
    assert_eq!(glyphs(top).len(), "CopyPasteClear".len());
    for quad in top {
        assert!(within(quad, menu.rect()), "{quad:?}");
    }
    assert!(solids(top, rgba(theme.menu_hover_bg)).is_empty());

    // The hovered row is highlighted.
    let row = menu.item_rect(2);
    ui.pointer(row.x + 3.0, row.y + 3.0, &layout);
    let layers = built(&mut ui, &layout, &mut atlas);
    let hover = solids(layers.top(), rgba(theme.menu_hover_bg));
    assert_eq!(hover.len(), 1);
    assert_eq!(
        (hover[0].pos, hover[0].size),
        ([row.x, row.y], [row.w, row.h])
    );
}

#[test]
fn cached_layers_are_rebuilt_when_told_or_when_the_atlas_changes() {
    let (mut ui, mut atlas, layout) = rig(1.0, (1280, 800));
    let mut layers = built(&mut ui, &layout, &mut atlas);
    let first = layers.base().to_vec();
    let stats = atlas.stats();
    // Nothing changed: no work, not even a cache lookup (this runs on every key echo).
    assert!(!layers.update(&mut ui, &layout, &mut atlas));
    assert_eq!(atlas.stats(), stats);

    // The state changed and the layers were told.
    ui.set_focus(Focus::List);
    assert!(!layers.update(&mut ui, &layout, &mut atlas), "not told yet");
    layers.invalidate();
    assert!(layers.update(&mut ui, &layout, &mut atlas));
    assert_ne!(layers.base(), first.as_slice());
    assert!(!layers.update(&mut ui, &layout, &mut atlas));

    // Zoom (or a new scale factor): every atlas rectangle is void, so the layers rebuild on
    // their own, and match a fresh build at the new size.
    let before = layers.base().to_vec();
    assert!(atlas.set_font_size(20.0));
    let relaid = s2_wgpu::ui::layout::layout((1280, 800), 1.0, &atlas.metrics(), &ui.theme);
    assert!(layers.update(&mut ui, &relaid, &mut atlas));
    assert_ne!(layers.base(), before.as_slice());
    let fresh = built(&mut ui, &relaid, &mut atlas);
    assert_eq!(layers.base(), fresh.base());
    let tallest = |layer: &[Instance]| {
        glyphs(layer)
            .iter()
            .map(|g| g.size[1])
            .fold(0.0f32, f32::max)
    };
    assert!(tallest(layers.base()) > tallest(&before), "bigger glyphs");
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

/// Rasterises glyphs nothing here draws (subpixel bins 1..) until the atlas starts over or
/// `stop_after` are in; returns how many fitted.
fn fill_big_atlas(atlas: &mut Atlas, stop_after: usize) -> usize {
    let start = atlas.generation();
    let mut done = 0;
    for bin in 1..4u8 {
        for bold in [false, true] {
            for ch in '!'..='~' {
                let style = FontStyle {
                    bold,
                    italic: false,
                };
                let shaped = atlas.shape(&ch.to_string(), style);
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
fn an_atlas_restart_while_composing_the_picture_leaves_no_stale_quads() {
    // Terminal cells are built, then the UI needs glyphs the atlas has no room for: it
    // restarts, which voids the terminal's quads too. `compose` must end with both the scene
    // and the UI layers equal to a fresh build.
    let capacity = fill_big_atlas(&mut big_atlas(), usize::MAX);
    assert!(capacity > 40);
    let mut atlas = big_atlas();
    fill_big_atlas(&mut atlas, capacity - 8);
    let mut ui = Ui::new();
    // Few glyphs, so that one generation holds the whole picture.
    ui.list = List::new(vec!["QRS".to_string(), "TUV".to_string()]);
    // At this font size a cell is over 100 px wide: a sidebar wide enough for the labels.
    ui.theme.sidebar_width = 2000.0;
    let layout = layout((4000, 3000), 1.0, &atlas.metrics(), &ui.theme);
    assert!(layout.visible_rows >= 2 && layout.label_cols >= 3);
    let mut frame = text_frame(&["abc"], 20);
    frame.cursor = cursor(4, 0, CursorShape::Bar);
    let overlays = Overlays {
        preedit: Some(Preedit {
            text: "xyz".to_string(),
        }),
        ..Overlays::default()
    };

    let mut scene = Scene::new();
    scene.set_origin(layout.terminal.x, layout.terminal.y);
    let generation = atlas.generation();
    scene.update_cells(&frame, &mut atlas, &ui.theme.terminal);
    assert_eq!(atlas.generation(), generation, "the cells still fit");
    let cells_before = scene.glyphs().to_vec();
    assert_eq!(cells_before.len(), 3);

    let mut layers = UiLayers::new();
    compose(
        &mut scene,
        &frame,
        &mut atlas,
        &overlays,
        &mut ui,
        &layout,
        &mut layers,
    );
    assert_ne!(atlas.generation(), generation, "the UI restarted the atlas");

    let generation = atlas.generation();
    let mut fresh_scene = Scene::new();
    fresh_scene.set_origin(layout.terminal.x, layout.terminal.y);
    fresh_scene.update(&frame, &mut atlas, &ui.theme.terminal, &overlays);
    let fresh = built(&mut ui, &layout, &mut atlas);
    assert_eq!(atlas.generation(), generation, "all cache hits");
    assert_ne!(
        scene.glyphs(),
        cells_before.as_slice(),
        "the cells were rebuilt"
    );
    assert_eq!(scene.glyphs(), fresh_scene.glyphs());
    assert_eq!(scene.overlay(), fresh_scene.overlay());
    assert_eq!(layers.base(), fresh.base());
    assert!(!glyphs(layers.base()).is_empty());

    // A second call with nothing changed does nothing to the UI layers.
    let stats = atlas.stats();
    compose(
        &mut scene,
        &frame,
        &mut atlas,
        &overlays,
        &mut ui,
        &layout,
        &mut layers,
    );
    assert_eq!(atlas.generation(), generation);
    assert_eq!(layers.base(), fresh.base());
    assert!(atlas.stats().misses == stats.misses, "nothing rasterised");
}

fn pixel(rgba: &[u8], width: u32, x: f32, y: f32) -> [u8; 4] {
    let i = ((y as u32 * width + x as u32) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn close(a: [u8; 4], b: [u8; 3]) -> bool {
    a[..3].iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2) && a[3] == 255
}

/// The whole window through the GPU at 125 %: sidebar, terminal beside it, menu on top, and
/// the photon square over the sidebar. Set `S2_WGPU_DUMP_UI=/some/file.ppm` to keep the
/// picture and look at it.
#[test]
fn the_window_through_the_gpu_has_the_layers_in_order() {
    let gpu = Gpu::headless().expect("this test needs a wgpu adapter (a software one will do)");
    let (width, height) = (1000u32, 600u32);
    let (mut ui, mut atlas, layout) = rig(1.25, (width, height));
    let theme = ui.theme.clone();
    let m = atlas.metrics();
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);

    let mut frame = text_frame(&["$ echo hello", "hello", "$"], layout.grid.0 as usize);
    frame.cells[1].bg = Some((0, 90, 0));
    frame.cursor = cursor(2, 2, CursorShape::Block);
    let mut scene = Scene::new();
    scene.set_origin(layout.terminal.x, layout.terminal.y);
    scene.update_cells(&frame, &mut atlas, &theme.terminal);
    let overlays = Overlays {
        photon: Some(true),
        ..Overlays::default()
    };

    ui.list.click_row(2);
    ui.field.insert("name 你好");
    // (Opening the menu moves the focus to the terminal; put it back on the field to see
    // the field's caret and composition in the same picture.)
    ui.context(layout.terminal.x + 150.0, 120.0, &layout);
    ui.set_focus(Focus::Field);
    ui.field.set_preedit("zhong");
    let menu = ui.menu.expect("open");
    ui.pointer(
        menu.item_rect(1).x + 2.0,
        menu.item_rect(1).y + 2.0,
        &layout,
    );
    let mut layers = UiLayers::new();
    compose(
        &mut scene,
        &frame,
        &mut atlas,
        &overlays,
        &mut ui,
        &layout,
        &mut layers,
    );
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut atlas,
        &layer_order(&scene, &layers),
        (width, height),
    );
    let rgba = render_offscreen(&gpu, &renderer, width, height, theme.terminal.bg).unwrap();
    if let Ok(path) = std::env::var("S2_WGPU_DUMP_UI") {
        let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
        ppm.extend(rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
        std::fs::write(&path, ppm).unwrap();
        println!("wrote {path}");
    }
    let at = |x: f32, y: f32| pixel(&rgba, width, x, y);

    // The photon square is over the sidebar, whole and white.
    for (x, y) in [
        (0.0, 0.0),
        (32.0, 32.0),
        (63.0, 63.0),
        (63.0, 0.0),
        (0.0, 63.0),
    ] {
        assert_eq!(at(x, y), [255, 255, 255, 255], "photon at ({x},{y})");
    }
    // Sidebar: its background right of the square and in the gap under the list; the
    // selected row; the separator; the field's background and focus outline.
    assert!(close(at(layout.sidebar.w - 2.0, 2.0), theme.sidebar_bg));
    assert!(close(at(2.0, layout.form_rule.y - 3.0), theme.sidebar_bg));
    let row = layout.list_row(2);
    assert!(close(
        at(row.right() - 3.0, row.y + 2.0),
        theme.item_selected_bg
    ));
    assert!(close(at(layout.separator.x, 300.0), theme.separator));
    assert!(close(at(layout.field.x, layout.field.y), theme.focus_ring));
    assert!(close(
        at(layout.field.right() - 4.0, layout.field.y + 4.0),
        theme.field_bg
    ));
    // The caret after "name " (5) + two wide characters (4) + the composition (5).
    let caret_x = layout.field_text.x + 14.0 * m.width;
    assert!(close(at(caret_x, layout.field_text.y + 1.0), theme.caret));
    // Text is there: some pixel of the field's text area is neither background nor caret.
    let t = layout.field_text;
    let inked = (0..(9.0 * m.width) as u32).any(|dx| {
        (0..m.height as u32).any(|dy| !close(at(t.x + dx as f32, t.y + dy as f32), theme.field_bg))
    });
    assert!(inked, "the field's text was drawn");

    // Terminal: starts right of the separator; the coloured cell (column 1) is where the
    // grid origin puts it; the clear colour elsewhere.
    let cell_x = layout.terminal.x + m.width;
    assert!(close(at(cell_x + 1.0, 1.0), [0, 90, 0]));
    assert!(close(
        at(layout.terminal.x + 1.0, 4.0 * m.height),
        theme.terminal.bg
    ));
    assert!(close(
        at(
            layout.terminal.x + 2.0 * m.width + 1.0,
            2.0 * m.height + 1.0
        ),
        theme.terminal.cursor
    ));
    assert!(close(
        at(width as f32 - 1.0, height as f32 - 1.0),
        theme.terminal.bg
    ));

    // Menu: over the terminal, border, panel and the hovered row.
    let r = menu.rect();
    assert!(close(at(r.x, r.y), theme.menu_border));
    assert!(close(at(r.x + 3.0, r.y + 3.0), theme.menu_bg));
    let hovered = menu.item_rect(1);
    assert!(close(
        at(hovered.x + 3.0, hovered.y + 3.0),
        theme.menu_hover_bg
    ));
    assert!(
        close(at(r.right(), r.y + 3.0), theme.terminal.bg),
        "ends there"
    );
}

#[test]
fn without_the_ui_no_quads_are_added() {
    let mut ui = Ui::with_ui(false);
    let mut atlas = atlas(1.0);
    let layout = ui.layout((1280, 800), 1.0, &atlas.metrics());
    let stats = atlas.stats();
    let mut layers = UiLayers::new();
    layers.update(&mut ui, &layout, &mut atlas);
    assert!(layers.base().is_empty() && layers.top().is_empty());
    assert_eq!(atlas.stats(), stats, "no glyph was looked up");

    // The draw order then is the terminal's three layers, as before the UI.
    let frame = text_frame(&["abc"], 20);
    let mut scene = Scene::new();
    scene.set_origin(layout.terminal.x, layout.terminal.y);
    scene.update_cells(&frame, &mut atlas, &ui.theme.terminal);
    compose(
        &mut scene,
        &frame,
        &mut atlas,
        &Overlays::default(),
        &mut ui,
        &layout,
        &mut layers,
    );
    let order = layer_order(&scene, &layers);
    let all: Vec<Instance> = order.iter().flat_map(|l| l.iter().copied()).collect();
    let plain: Vec<Instance> = scene
        .layers()
        .iter()
        .flat_map(|l| l.iter().copied())
        .collect();
    assert_eq!(all, plain);
    assert_eq!(glyphs(&all).len(), 3);
    assert_eq!(glyphs(&all)[0].pos[0].floor(), glyphs(&all)[0].pos[0]);
    assert!(
        glyphs(&all)[0].pos[0] < atlas.metrics().width,
        "the grid starts at x = 0"
    );
}
