//! The file panel's picture on the GPU: drawn into a texture and read back. Needs a wgpu
//! adapter (a software one will do); without one the GPU test says so and passes.

use desktop::draw::Rect;
use desktop::picture::{PictureRenderer, scissor};
use desktop::viewer::{Picture, place};
use s2_wgpu::gfx::render::Gpu;

const SIZE: u32 = 64;
/// What the target is cleared to: no colour of the picture.
const CLEAR: [u8; 4] = [20, 40, 60, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

const WHOLE: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: SIZE as f32,
    h: SIZE as f32,
};

/// Red, green / blue, white.
fn four() -> Picture {
    Picture {
        width: 2,
        height: 2,
        rgba: [RED, GREEN, BLUE, WHITE].concat(),
    }
}

/// An adapter, or none with the reason printed: the machine that builds in Docker has none.
fn gpu(test: &str) -> Option<Gpu> {
    match Gpu::headless() {
        Ok(gpu) => {
            println!("{test}: on {:?}", gpu.adapter.get_info().name);
            Some(gpu)
        }
        Err(e) => {
            println!("{test}: SKIPPED, no wgpu adapter here ({e:#})");
            None
        }
    }
}

/// A `SIZE` x `SIZE` target of `format` cleared to `CLEAR` (as bytes of the target), then
/// what `draw` adds; its pixels, four bytes each, top row first.
fn rendered(
    gpu: &Gpu,
    format: wgpu::TextureFormat,
    draw: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
) -> Vec<u8> {
    let extent = wgpu::Extent3d {
        width: SIZE,
        height: SIZE,
        depth_or_array_layers: 1,
    };
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("picture test target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    // SIZE * 4 is a multiple of 256: the rows need no padding.
    assert_eq!(SIZE * 4 % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("picture test readback"),
        size: (SIZE * SIZE * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    // A clear value is in the target's own space: linear for an sRGB target.
    let channel = |c: u8| {
        let c = c as f64 / 255.0;
        match (format.is_srgb(), c <= 0.04045) {
            (false, _) => c,
            (true, true) => c / 12.92,
            (true, false) => ((c + 0.055) / 1.055).powf(2.4),
        }
    };
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("picture test clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: channel(CLEAR[0]),
                    g: channel(CLEAR[1]),
                    b: channel(CLEAR[2]),
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    draw(&mut encoder, &view);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: None,
            },
        },
        extent,
    );
    gpu.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("waiting for the frame");
    rx.recv()
        .expect("the map callback ran")
        .expect("mapping the readback buffer");
    let pixels = readback
        .slice(..)
        .get_mapped_range()
        .expect("reading the readback buffer")
        .to_vec();
    readback.unmap();
    pixels
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIZE + x) * 4) as usize;
    pixels[i..i + 4].try_into().unwrap()
}

/// The picture drawn with these rectangles on a target of `format`.
fn drawn(
    gpu: &Gpu,
    format: wgpu::TextureFormat,
    picture: Option<&Picture>,
    dest: Rect,
    source: Rect,
    clip: Rect,
) -> Vec<u8> {
    let mut renderer = PictureRenderer::new(&gpu.device, format);
    assert_eq!(
        renderer.set(&gpu.device, &gpu.queue, picture),
        picture.is_some()
    );
    rendered(gpu, format, |encoder, view| {
        renderer.draw(encoder, view, (SIZE, SIZE), dest, source, clip);
    })
}

#[test]
fn a_picture_fills_its_rectangle_with_its_own_colours_and_stays_inside_the_clip() {
    let Some(gpu) = gpu("picture") else {
        return;
    };
    let source = rect(0.0, 0.0, 2.0, 2.0);
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        // The whole target: a colour in each quarter, exactly the picture's.
        let pixels = drawn(&gpu, format, Some(&four()), WHOLE, source, WHOLE);
        assert_eq!(pixel(&pixels, 8, 8), RED, "{format:?}");
        assert_eq!(pixel(&pixels, 56, 8), GREEN, "{format:?}");
        assert_eq!(pixel(&pixels, 8, 56), BLUE, "{format:?}");
        assert_eq!(pixel(&pixels, 56, 56), WHITE, "{format:?}");

        // Clipped to the left half: the right half keeps what was there.
        let left = rect(0.0, 0.0, 32.0, 64.0);
        let pixels = drawn(&gpu, format, Some(&four()), WHOLE, source, left);
        assert_eq!(pixel(&pixels, 8, 8), RED, "{format:?}");
        assert_eq!(pixel(&pixels, 8, 56), BLUE, "{format:?}");
        // Up to the clip's last pixel (red going over to green there), and not one further.
        let [red, green, blue, alpha] = pixel(&pixels, 31, 8);
        assert!(
            red > 150 && green > 150 && blue == 0 && alpha == 255,
            "{format:?}"
        );
        assert_eq!(pixel(&pixels, 32, 8), CLEAR, "{format:?}");
        assert_eq!(pixel(&pixels, 56, 8), CLEAR, "{format:?}");
        assert_eq!(pixel(&pixels, 56, 56), CLEAR, "{format:?}");
    }
}

#[test]
fn a_part_of_the_picture_goes_where_place_says_and_nothing_else_is_touched() {
    let Some(gpu) = gpu("picture") else {
        return;
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    // 4 x 2 texels in a body of 32 x 32 pixels at (16, 16): fitted, it is drawn as it is, in
    // the middle — one texel a pixel, every colour exact.
    let orange = [255, 136, 0, 255];
    let grey = [119, 119, 119, 255];
    let picture = Picture {
        width: 4,
        height: 2,
        rgba: [orange, grey, RED, GREEN, BLUE, WHITE, orange, grey].concat(),
    };
    let body = rect(16.0, 16.0, 32.0, 32.0);
    let (dest, source) = place((4, 2), body, false, (0.0, 0.0));
    assert_eq!(dest, rect(30.0, 31.0, 4.0, 2.0));
    let pixels = drawn(&gpu, format, Some(&picture), dest, source, body);
    let row = |y| [30, 31, 32, 33].map(|x| pixel(&pixels, x, y));
    assert_eq!(row(31), [orange, grey, RED, GREEN]);
    assert_eq!(row(32), [BLUE, WHITE, orange, grey]);
    for (x, y) in [(29, 31), (34, 31), (30, 30), (30, 33), (0, 0), (63, 63)] {
        assert_eq!(pixel(&pixels, x, y), CLEAR, "({x}, {y})");
    }

    // At its own size in a body of 2 x 2 pixels, scrolled one texel right: the part from the
    // second column.
    let small = rect(10.0, 10.0, 2.0, 2.0);
    let (dest, source) = place((4, 2), small, true, (1.0, 0.0));
    assert_eq!((dest, source), (small, rect(1.0, 0.0, 2.0, 2.0)));
    let pixels = drawn(&gpu, format, Some(&picture), dest, source, small);
    assert_eq!(pixel(&pixels, 10, 10), grey);
    assert_eq!(pixel(&pixels, 11, 10), RED);
    assert_eq!(pixel(&pixels, 10, 11), WHITE);
    assert_eq!(pixel(&pixels, 11, 11), orange);
    assert_eq!(pixel(&pixels, 12, 10), CLEAR);

    // Half transparent white over the clear colour: mixed, half of each.
    let glass = Picture {
        width: 1,
        height: 1,
        rgba: vec![255, 255, 255, 128],
    };
    let one = rect(0.0, 0.0, 1.0, 1.0);
    let pixels = drawn(&gpu, format, Some(&glass), WHOLE, one, WHOLE);
    let mixed = pixel(&pixels, 30, 30);
    for (got, under) in mixed.iter().zip(CLEAR).take(3) {
        let wanted = (255.0 * 128.0 / 255.0 + under as f32 * 127.0 / 255.0).round() as i32;
        assert!((*got as i32 - wanted).abs() <= 1, "{mixed:?}");
    }
}

#[test]
fn no_picture_or_nothing_to_see_draws_nothing() {
    let Some(gpu) = gpu("picture") else {
        return;
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let source = rect(0.0, 0.0, 2.0, 2.0);
    let untouched = |pixels: Vec<u8>| pixels.chunks_exact(4).all(|pixel| pixel == CLEAR);
    // None loaded.
    assert!(untouched(drawn(&gpu, format, None, WHOLE, source, WHOLE)));
    // A rectangle of no size, a clip of no size, a clip outside the target.
    let nothing = rect(5.0, 5.0, 0.0, 0.0);
    let outside = rect(64.0, 0.0, 30.0, 30.0);
    for (dest, clip) in [(nothing, WHOLE), (WHOLE, nothing), (WHOLE, outside)] {
        let pixels = drawn(&gpu, format, Some(&four()), dest, source, clip);
        assert!(untouched(pixels), "{dest:?} clipped to {clip:?}");
    }

    // Loaded, then taken away: nothing is drawn any more.
    let mut renderer = PictureRenderer::new(&gpu.device, format);
    assert!(renderer.set(&gpu.device, &gpu.queue, Some(&four())));
    assert_eq!(renderer.size(), Some((2, 2)));
    assert!(!renderer.set(&gpu.device, &gpu.queue, None));
    assert_eq!(renderer.size(), None);
    let pixels = rendered(&gpu, format, |encoder, view| {
        renderer.draw(encoder, view, (SIZE, SIZE), WHOLE, source, WHOLE);
    });
    assert!(untouched(pixels));
    // Pixels that are not the size they say: not uploaded, not a panic.
    let short = Picture {
        width: 2,
        height: 2,
        rgba: vec![0; 15],
    };
    assert!(!renderer.set(&gpu.device, &gpu.queue, Some(&short)));
    let none = Picture {
        width: 0,
        height: 0,
        rgba: Vec::new(),
    };
    assert!(!renderer.set(&gpu.device, &gpu.queue, Some(&none)));
    // Longer than the device's largest texture.
    let most = gpu.device.limits().max_texture_dimension_2d;
    let long = Picture {
        width: most + 1,
        height: 1,
        rgba: vec![0; (most as usize + 1) * 4],
    };
    assert!(!renderer.set(&gpu.device, &gpu.queue, Some(&long)));
}

#[test]
fn the_scissor_is_the_whole_pixels_of_the_clip_inside_the_target() {
    let target = (100, 50);
    assert_eq!(
        scissor(rect(10.0, 20.0, 30.0, 5.0), target),
        Some((10, 20, 30, 5))
    );
    // Past the target's edges: what is inside.
    assert_eq!(
        scissor(rect(-10.0, -10.0, 30.0, 30.0), target),
        Some((0, 0, 20, 20))
    );
    assert_eq!(
        scissor(rect(90.0, 40.0, 30.0, 30.0), target),
        Some((90, 40, 10, 10))
    );
    // Parts of pixels are left out.
    assert_eq!(
        scissor(rect(10.5, 20.5, 10.0, 10.0), target),
        Some((11, 21, 9, 9))
    );
    for none in [
        rect(100.0, 0.0, 10.0, 10.0),
        rect(0.0, 50.0, 10.0, 10.0),
        rect(-20.0, 0.0, 10.0, 10.0),
        rect(10.0, 10.0, 0.0, 10.0),
        rect(10.0, 10.0, 10.0, -3.0),
        rect(10.2, 10.0, 0.7, 10.0),
        rect(f32::NAN, 0.0, 10.0, 10.0),
        rect(0.0, 0.0, f32::NAN, 10.0),
    ] {
        assert_eq!(scissor(none, target), None, "{none:?}");
    }
    assert_eq!(scissor(rect(0.0, 0.0, 10.0, 10.0), (0, 0)), None);
}
