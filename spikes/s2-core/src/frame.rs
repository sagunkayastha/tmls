//! The frame the emulator thread publishes and the latest-wins slot the front-end reads.

use std::sync::{Arc, Mutex};

use crate::latency::Log;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    /// Row-major, `cells.len() == cols * rows`.
    pub cells: Vec<Cell>,
    pub cursor: Option<Cursor>,
    pub dirty_rows: Vec<bool>,
    pub seq: u64,
    /// When the PTY bytes that produced this frame were read.
    pub t_output_ns: u128,
    /// Ids of the keys whose echo is in this frame.
    pub keys: Vec<u64>,
}

impl Frame {
    /// Text of row `y`, right-trimmed. Wide-char spacer cells have no text.
    pub fn row_text(&self, y: u16) -> String {
        let start = y as usize * self.cols as usize;
        let row = &self.cells[start..start + self.cols as usize];
        let mut text = String::new();
        for cell in row {
            match (cell.wide, cell.text.is_empty()) {
                (Wide::Spacer, _) => {}
                (_, true) => text.push(' '),
                (_, false) => text.push_str(&cell.text),
            }
        }
        text.trim_end().to_string()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub flags: CellFlags,
    pub wide: Wide,
    pub hyperlink: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellFlags {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub strike: bool,
    pub faint: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wide {
    #[default]
    Narrow,
    Lead,
    Spacer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub shape: CursorShape,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

type Waker = Box<dyn Fn() + Send + Sync>;

struct SlotInner {
    frame: Mutex<Option<Frame>>,
    waker: Mutex<Option<Waker>>,
    log: Option<Log>,
}

/// Latest-wins single slot. Frames may drop; PTY bytes never.
#[derive(Clone)]
pub struct FrameSlot {
    inner: Arc<SlotInner>,
}

impl FrameSlot {
    pub(crate) fn new(log: Option<Log>) -> FrameSlot {
        FrameSlot { inner: Arc::new(SlotInner { frame: Mutex::new(None), waker: Mutex::new(None), log }) }
    }

    /// Replaces an unread frame. The replaced frame's keys and dirty rows carry over, so a
    /// dropped frame loses neither a key's present time nor a redraw.
    pub(crate) fn publish(&self, mut frame: Frame) {
        {
            let mut slot = self.inner.frame.lock().unwrap();
            if let Some(old) = slot.take() {
                let mut keys = old.keys;
                keys.append(&mut frame.keys);
                frame.keys = keys;
                if old.cols == frame.cols && old.rows == frame.rows {
                    for (new, was) in frame.dirty_rows.iter_mut().zip(&old.dirty_rows) {
                        *new |= *was;
                    }
                } else {
                    frame.dirty_rows.fill(true);
                }
            }
            *slot = Some(frame);
        }
        self.wake();
    }

    pub(crate) fn wake(&self) {
        if let Some(waker) = self.inner.waker.lock().unwrap().as_ref() {
            waker();
        }
    }

    pub fn take(&self) -> Option<Frame> {
        let frame = self.inner.frame.lock().unwrap().take()?;
        if let Some(log) = &self.inner.log {
            log.frame_taken(frame.seq, frame.t_output_ns, &frame.keys);
        }
        Some(frame)
    }

    pub fn set_waker(&self, f: Box<dyn Fn() + Send + Sync>) {
        *self.inner.waker.lock().unwrap() = Some(f);
    }
}
