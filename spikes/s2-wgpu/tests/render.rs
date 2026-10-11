//! Shader validation (no GPU) and the offscreen render path (needs an adapter).

mod common;

use common::{cell, cursor, frame, lead, spacer, text_frame};
use s2_core::{Cell, CellFlags, CursorShape};
use s2_wgpu::gfx::SHADER_WGSL;
use s2_wgpu::gfx::atlas::{Atlas, FontSpec, FontStyle};
use s2_wgpu::gfx::grid::{CellSpan, Overlays, Preedit, Scene, Theme};
use s2_wgpu::gfx::render::{Gpu, OFFSCREEN_FORMAT, Renderer, adapter_report, render_offscreen};

#[test]
fn shader_validates() {
    let module = naga::front::wgsl::parse_str(SHADER_WGSL)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(SHADER_WGSL)));
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(SHADER_WGSL)));

    let stage = |name: &str| {
        module
            .entry_points
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.stage)
    };
    assert_eq!(stage("vs_main"), Some(naga::ShaderStage::Vertex));
    assert_eq!(stage("fs_main"), Some(naga::ShaderStage::Fragment));

    // The instance attributes the pipeline feeds: pos, size, uv, colour, kind at locations 0..=4.
    let vs = module
        .entry_points
        .iter()
        .find(|e| e.name == "vs_main")
        .unwrap();
    let mut locations = Vec::new();
    for arg in &vs.function.arguments {
        match (&arg.binding, &module.types[arg.ty].inner) {
            (Some(naga::Binding::Location { location, .. }), _) => locations.push(*location),
            (None, naga::TypeInner::Struct { members, .. }) => {
                for member in members {
                    if let Some(naga::Binding::Location { location, .. }) = member.binding {
                        locations.push(location);
                    }
                }
            }
            _ => {}
        }
    }
    locations.sort();
    assert_eq!(locations, [0, 1, 2, 3, 4]);
    // One uniform block and the atlas texture, both in group 0.
    let bound: Vec<(u32, u32)> = module
        .global_variables
        .iter()
        .filter_map(|(_, v)| v.binding.as_ref().map(|b| (b.group, b.binding)))
        .collect();
    assert_eq!(bound, [(0, 0), (0, 1)]);
}

/// Not a pass/fail check: prints what wgpu can see here, for the task report.
#[test]
fn gpu_adapter_probe() {
    for line in adapter_report() {
        println!("adapter: {line}");
    }
    match Gpu::headless() {
        Ok(gpu) => println!("headless: {:?}", gpu.adapter.get_info()),
        Err(e) => println!("headless: none ({e:#})"),
    }
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn close(a: [u8; 4], b: [u8; 3]) -> bool {
    a[..3].iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2) && a[3] == 255
}

#[test]
fn offscreen_render_reads_back_pixels() {
    let gpu = Gpu::headless().expect("this test needs a wgpu adapter (a software one will do)");
    let (width, height) = (200u32, 100u32);
    let theme = Theme::default();
    let mut atlas = Atlas::new(FontSpec::default(), 1.0);
    let m = atlas.metrics();
    let mut scene = Scene::new();
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);

    // Row 0: "HI", then a red cell at column 4. Row 1: a block cursor at column 6.
    let mut frame = text_frame(&["HI", ""], 8);
    frame.cells[4].bg = Some((200, 0, 0));
    frame.cursor = cursor(6, 1, CursorShape::Block);
    scene.update(&frame, &mut atlas, &theme, &Overlays::default());
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut atlas,
        &scene.layers(),
        (width, height),
    );
    let rgba = render_offscreen(&gpu, &renderer, width, height, theme.bg).unwrap();
    assert_eq!(rgba.len(), (width * height * 4) as usize);

    let centre = |col: f32, row: f32| {
        (
            ((col + 0.5) * m.width) as u32,
            ((row + 0.5) * m.height) as u32,
        )
    };
    let at = |(x, y): (u32, u32)| pixel(&rgba, width, x, y);
    assert!(
        close(at(centre(4.0, 0.0)), [200, 0, 0]),
        "red cell: {:?}",
        at(centre(4.0, 0.0))
    );
    assert!(
        close(at(centre(6.0, 1.0)), theme.cursor),
        "cursor: {:?}",
        at(centre(6.0, 1.0))
    );
    assert!(
        close(at(centre(3.0, 1.0)), theme.bg),
        "empty cell: {:?}",
        at(centre(3.0, 1.0))
    );
    assert!(
        close(at((width - 1, height - 1)), theme.bg),
        "outside the grid is the clear colour"
    );
    // Just outside the red cell on every side it is background again.
    assert!(close(at(((4.0 * m.width) as u32 - 1, 2)), theme.bg));
    assert!(close(at(((5.0 * m.width) as u32, 2)), theme.bg));
    assert!(close(
        at(((4.5 * m.width) as u32, m.height as u32)),
        theme.bg
    ));

    // The two letters put foreground-coloured ink into their cells, and only there.
    let ink = |col: u32, row: u32| {
        let mut strongest = 0u8;
        for y in (row as f32 * m.height) as u32..((row + 1) as f32 * m.height) as u32 {
            for x in (col as f32 * m.width) as u32..((col + 1) as f32 * m.width) as u32 {
                let p = pixel(&rgba, width, x, y);
                strongest = strongest.max(p[0].abs_diff(theme.bg[0]));
            }
        }
        strongest
    };
    let full = theme.fg[0] - theme.bg[0];
    assert!(
        ink(0, 0) > full - 20,
        "'H' is drawn at full strength: {}",
        ink(0, 0)
    );
    assert!(ink(1, 0) > full - 20, "'I' is drawn: {}", ink(1, 0));
    assert_eq!(ink(2, 0), 0, "column 2 is blank");
    // A letter is a shape from the atlas, not a filled box: most of its cell stays background.
    let inked = |col: u32| {
        let (x0, x1) = (
            (col as f32 * m.width) as u32,
            ((col + 1) as f32 * m.width) as u32,
        );
        let cell: Vec<bool> = (0..m.height as u32)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .map(|(x, y)| pixel(&rgba, width, x, y)[0] != theme.bg[0])
            .collect();
        cell.iter().filter(|on| **on).count() as f32 / cell.len() as f32
    };
    assert!(
        inked(0) > 0.05 && inked(0) < 0.5,
        "'H' covers {} of its cell",
        inked(0)
    );
    assert!(
        inked(1) > 0.03 && inked(1) < inked(0),
        "'I' has less ink than 'H': {} vs {}",
        inked(1),
        inked(0)
    );
    assert_eq!(ink(0, 1), 0, "row 1 is blank");

    // Photon square on: 64 x 64 white pixels in the corner, drawn over the text.
    let overlays = Overlays {
        photon: Some(true),
        ..Overlays::default()
    };
    scene.update(&frame, &mut atlas, &theme, &overlays);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut atlas,
        &scene.layers(),
        (width, height),
    );
    let rgba = render_offscreen(&gpu, &renderer, width, height, theme.bg).unwrap();
    for (x, y) in [(0, 0), (5, 5), (63, 0), (0, 63), (63, 63)] {
        assert_eq!(
            pixel(&rgba, width, x, y),
            [255, 255, 255, 255],
            "photon at ({x},{y})"
        );
    }
    for (x, y) in [(64, 0), (0, 64), (64, 64)] {
        assert!(
            close(pixel(&rgba, width, x, y), theme.bg),
            "outside the photon square at ({x},{y})"
        );
    }
}

/// Every instance kind through the GPU at a fractional scale and with the grid moved off the
/// corner. Set `S2_WGPU_DUMP=/some/file.ppm` to keep the picture and look at it.
#[test]
fn offscreen_sample_scene_at_fractional_scale() {
    let gpu = Gpu::headless().expect("this test needs a wgpu adapter (a software one will do)");
    let (width, height) = (480u32, 160u32);
    let theme = Theme::default();
    let mut atlas = Atlas::new(FontSpec::default(), 1.25);
    let m = atlas.metrics();
    let mut scene = Scene::new();
    scene.set_origin(80.0, 8.0);
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);

    let styled = |text: &str,
                  flags: CellFlags,
                  fg: Option<(u8, u8, u8)>,
                  bg: Option<(u8, u8, u8)>|
     -> Vec<Cell> {
        text.chars()
            .map(|c| Cell {
                flags,
                fg,
                bg,
                ..cell(&c.to_string())
            })
            .collect()
    };
    let pad = |mut row: Vec<Cell>| {
        row.resize(24, Cell::default());
        row
    };
    let plain = CellFlags::default();
    let mut wide_row = styled("wide:", plain, None, None);
    wide_row.extend([
        Cell {
            bg: Some((0, 90, 0)),
            ..lead("\u{597d}")
        },
        spacer(),
    ]);
    wide_row.extend(styled("|ok", plain, None, None));
    let mut mixed = styled(
        "bold",
        CellFlags {
            bold: true,
            ..plain
        },
        None,
        None,
    );
    mixed.push(Cell::default());
    mixed.extend(styled(
        "ital",
        CellFlags {
            italic: true,
            ..plain
        },
        Some((90, 200, 250)),
        None,
    ));
    mixed.push(Cell::default());
    mixed.extend(styled(
        "undr",
        CellFlags {
            underline: true,
            ..plain
        },
        None,
        None,
    ));
    mixed.push(Cell::default());
    mixed.extend(styled(
        "strk",
        CellFlags {
            strike: true,
            ..plain
        },
        None,
        None,
    ));
    let mut frame = frame(vec![
        pad(styled("The quick brown fox", plain, None, None)),
        pad(mixed),
        pad(styled(
            "inverse",
            CellFlags {
                inverse: true,
                ..plain
            },
            None,
            None,
        )),
        pad(wide_row),
        pad(styled(
            "faint red",
            CellFlags {
                faint: true,
                ..plain
            },
            Some((250, 60, 60)),
            Some((40, 40, 60)),
        )),
        pad(styled("$ ", plain, None, None)),
    ]);
    frame.cursor = cursor(2, 5, CursorShape::Block);
    let overlays = Overlays {
        preedit: Some(Preedit { text: "ni".into() }),
        photon: Some(true),
        selection: vec![CellSpan {
            row: 0,
            col_start: 4,
            col_end: 9,
        }],
    };
    assert_eq!(scene.update(&frame, &mut atlas, &theme, &overlays), 6);
    let layers = scene.layers();
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut atlas,
        &layers,
        (width, height),
    );
    let rgba = render_offscreen(&gpu, &renderer, width, height, theme.bg).unwrap();

    // Left of the grid origin only the photon square is drawn.
    assert_eq!(pixel(&rgba, width, 32, 32), [255, 255, 255, 255]);
    assert!(close(pixel(&rgba, width, 70, 100), theme.bg));
    // The wide cell's background is two columns wide: both halves are green, the next cell is not.
    let cell_centre = |col: f32, row: f32| {
        (
            (80.0 + (col + 0.5) * m.width) as u32,
            (8.0 + (row + 0.2) * m.height) as u32,
        )
    };
    let green = |(x, y): (u32, u32)| {
        let p = pixel(&rgba, width, x, y);
        p[1] > 80 && p[0] < 30
    };
    assert!(
        green(cell_centre(5.0, 3.0)) && green(cell_centre(6.0, 3.0)),
        "both columns of the wide cell"
    );
    assert!(!green(cell_centre(7.0, 3.0)) && !green(cell_centre(4.0, 3.0)));
    // Inverse text: the cell is filled with the foreground colour.
    let p = pixel(&rgba, width, 80 + 1, 8 + 2 * m.height as u32 + 1);
    assert!(close(p, theme.fg), "inverse cell corner: {p:?}");
    // Selection tints the background of row 0, columns 4..9, and nothing else.
    let tinted =
        |col: f32| pixel(&rgba, width, cell_centre(col, 0.0).0, 8 + 1)[2] > theme.bg[2] + 40;
    assert!(tinted(4.0) && tinted(8.0) && !tinted(3.0) && !tinted(9.0));
    // The preedit's underline run: two columns from the cursor cell, in the text colour.
    let underline_y = 8 + (5.0 * m.height + m.underline_y) as u32;
    for col in [2.0, 3.0] {
        let p = pixel(&rgba, width, cell_centre(col, 5.0).0, underline_y);
        assert!(
            close(p, theme.fg),
            "preedit underline at column {col}: {p:?}"
        );
    }
    // The cursor block sits after the preedit.
    assert!(close(
        pixel(
            &rgba,
            width,
            cell_centre(4.0, 5.0).0,
            8 + (5.5 * m.height) as u32
        ),
        theme.cursor
    ));

    if let Ok(path) = std::env::var("S2_WGPU_DUMP") {
        let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
        ppm.extend(rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
        std::fs::write(&path, ppm).unwrap();
        println!("wrote {path}");
    }
}

/// With an sRGB-encoded target the shader converts to linear and the GPU encodes again: solid
/// colours read back as the bytes that went in.
#[test]
fn srgb_target_keeps_solid_colours() {
    let gpu = Gpu::headless().expect("this test needs a wgpu adapter (a software one will do)");
    let (width, height) = (64u32, 32u32);
    let theme = Theme::default();
    let mut atlas = Atlas::new(FontSpec::default(), 1.0);
    let m = atlas.metrics();
    let mut scene = Scene::new();
    let mut renderer = Renderer::new(&gpu.device, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut frame = text_frame(&[""], 4);
    frame.cells[1].bg = Some((200, 100, 30));
    scene.update(&frame, &mut atlas, &theme, &Overlays::default());
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut atlas,
        &scene.layers(),
        (width, height),
    );
    let rgba = render_offscreen(&gpu, &renderer, width, height, theme.bg).unwrap();
    let cell = pixel(&rgba, width, (1.5 * m.width) as u32, 4);
    assert!(close(cell, [200, 100, 30]), "coloured cell: {cell:?}");
    let clear = pixel(&rgba, width, width - 1, height - 1);
    assert!(close(clear, theme.bg), "clear colour: {clear:?}");
}

#[test]
fn atlas_never_outgrows_the_device_texture_limit() {
    // A device capped at 2048 px textures (GLES 3 class hardware) and an atlas that has grown
    // past that: `prepare` must not ask for a texture the device rejects.
    let gpu = Gpu::headless().expect("this test needs a wgpu adapter (a software one will do)");
    const LIMIT: u32 = 2048;
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("s2-wgpu capped"),
        required_limits: wgpu::Limits {
            max_texture_dimension_2d: LIMIT,
            ..wgpu::Limits::downlevel_defaults()
        },
        ..Default::default()
    };
    let (device, queue) = pollster::block_on(gpu.adapter.request_device(&descriptor)).unwrap();
    assert_eq!(device.limits().max_texture_dimension_2d, LIMIT);

    let mut atlas = Atlas::new(
        FontSpec {
            family: None,
            size: 200.0,
        },
        1.0,
    );
    let theme = Theme::default();
    let mut scene = Scene::new();
    let mut renderer = Renderer::new(&device, OFFSCREEN_FORMAT);
    // Enough distinct 200 px glyphs to need more than 2048 atlas rows.
    for bin in 0..4u8 {
        for ch in '!'..='~' {
            let shaped = atlas.shape(&ch.to_string(), FontStyle::default());
            let key = atlas.key(&shaped[0], bin);
            atlas.glyph(key);
        }
    }
    let frame = text_frame(&["ABCD"], 4);
    assert!(
        atlas.image().height > LIMIT,
        "the test needs an atlas taller than the limit, got {}",
        atlas.image().height
    );
    let m = atlas.metrics();
    let (width, height) = (m.width as u32 * 4, m.height as u32);
    for _ in 0..2 {
        scene.update(&frame, &mut atlas, &theme, &Overlays::default());
        renderer.prepare(
            &device,
            &queue,
            &mut atlas,
            &scene.layers(),
            (width, height),
        );
    }
    assert!(atlas.image().height <= LIMIT);
    // The frame drawn after the clamp shows real glyphs: "ABCD" leaves ink.
    let capped = Gpu {
        instance: gpu.instance,
        adapter: gpu.adapter,
        device,
        queue,
    };
    let rgba = render_offscreen(&capped, &renderer, width, height, theme.bg).unwrap();
    let ink = rgba.chunks_exact(4).filter(|p| p[0] > 150).count();
    assert!(ink > 500, "glyph pixels after the clamp: {ink}");
}
