//! Frames from the real emulator thread (`s2-core`) through the quad builder: the wide-cell and
//! dirty-row conventions here have to match what the core actually publishes.

use std::time::{Duration, Instant};

use s2_core::{Emu, Frame, InputMsg, Pty, Wide, now_ns};
use s2_wgpu::gfx::atlas::{Atlas, FontSpec};
use s2_wgpu::gfx::grid::{KIND_SOLID, Overlays, Scene, Theme};

const WAIT: Duration = Duration::from_secs(20);

/// Feeds every frame the emulator publishes to the scene, as a front-end does, until `done`
/// accepts one. Returns that frame and the rows rebuilt for it.
fn pump(
    emu: &Emu,
    scene: &mut Scene,
    atlas: &mut Atlas,
    theme: &Theme,
    done: impl Fn(&Frame) -> bool,
) -> (Frame, usize) {
    let start = Instant::now();
    loop {
        if let Some(frame) = emu.frames.take() {
            let rebuilt = scene.update(&frame, atlas, theme, &Overlays::default());
            if done(&frame) {
                return (frame, rebuilt);
            }
        }
        assert!(start.elapsed() < WAIT, "no matching frame within {WAIT:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn emulator_frames_become_quads() {
    let (cols, rows) = (20u16, 6u16);
    let pty = Pty::spawn(&["cat".to_string()], cols, rows).expect("spawn cat");
    let emu = Emu::start(pty, cols, rows, None).unwrap();
    let theme = Theme::default();
    let mut atlas = Atlas::new(FontSpec::default(), 1.0);
    let m = atlas.metrics();
    let mut scene = Scene::new();

    // A wide character on a red background, then a narrow one on the default background.
    emu.input_tx
        .send(InputMsg::Bytes(
            "\x1b[41m你\x1b[0ma".as_bytes().to_vec(),
            now_ns(),
        ))
        .unwrap();
    let (frame, _) = pump(&emu, &mut scene, &mut atlas, &theme, |f| {
        f.row_text(0) == "你a"
    });
    assert_eq!(
        (frame.cells[0].wide, frame.cells[1].wide),
        (Wide::Lead, Wide::Spacer)
    );

    // Whatever colour the spacer carries, the red area is exactly columns 0..2.
    let (r, g, b) = frame.cells[0]
        .bg
        .expect("SGR 41 gives the lead a background");
    let red: Vec<_> = scene
        .bg()
        .iter()
        .filter(|q| q.color == [r, g, b, 255])
        .collect();
    assert!(!red.is_empty());
    let left = red.iter().map(|q| q.pos[0]).fold(f32::MAX, f32::min);
    let right = red.iter().map(|q| q.pos[0] + q.size[0]).fold(0.0, f32::max);
    assert_eq!((left, right), (0.0, 2.0 * m.width), "{:?}", scene.bg());
    assert!(red.iter().all(|q| q.pos[1] == 0.0 && q.size[1] == m.height));
    // The cursor sits after the 'a', in column 3 of row 0.
    let cursor = frame.cursor.expect("the live screen has a cursor");
    assert_eq!((cursor.col, cursor.row), (3, 0));
    if cursor.visible {
        let quad = scene
            .overlay()
            .iter()
            .find(|q| q.kind == KIND_SOLID)
            .expect("a cursor quad");
        assert_eq!(quad.pos, [3.0 * m.width, 0.0]);
    }
    let glyphs_before = scene.glyphs().len();
    assert!(glyphs_before >= 1, "at least the 'a'");

    // More output on the next row: the emulator marks only what changed, so the scene rebuilds
    // a part of the grid, not all of it, and row 0 keeps its quads.
    emu.input_tx
        .send(InputMsg::Bytes(b"\r\nxyz".to_vec(), now_ns()))
        .unwrap();
    let (frame, rebuilt) = pump(&emu, &mut scene, &mut atlas, &theme, |f| {
        f.row_text(1) == "xyz"
    });
    assert!(
        rebuilt >= 1 && rebuilt < rows as usize,
        "rebuilt {rebuilt} of {rows} rows; dirty: {:?}",
        frame.dirty_rows
    );
    assert_eq!(scene.glyphs().len(), glyphs_before + 3);

    // A resize publishes a frame of another size: everything is rebuilt, nothing panics.
    emu.input_tx.send(InputMsg::Resize(10, 3)).unwrap();
    let (frame, rebuilt) = pump(&emu, &mut scene, &mut atlas, &theme, |f| {
        f.cols == 10 && f.rows == 3
    });
    assert_eq!(rebuilt, 3);
    assert_eq!(frame.cells.len(), 30);
}
