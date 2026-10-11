//! The frame the emulator thread publishes and the latest-wins slot the front-end reads.

use std::sync::atomic::{AtomicBool, Ordering};
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
    /// When the last PTY bytes in this frame were read. A frame published without new output
    /// (the first one, a resize, a viewport scroll) carries the time it was built instead.
    pub t_output_ns: u128,
    /// Ids of the keys this frame is the first to show output for: keys whose PTY write began at
    /// or before `t_output_ns`. Always empty for a frame without new output.
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
    exited: AtomicBool,
    mouse_reporting: AtomicBool,
    log: Option<Log>,
}

/// Latest-wins single slot. Frames may drop; PTY bytes never.
#[derive(Clone)]
pub struct FrameSlot {
    inner: Arc<SlotInner>,
}

impl FrameSlot {
    pub(crate) fn new(log: Option<Log>) -> FrameSlot {
        let inner = SlotInner { frame: Mutex::new(None), waker: Mutex::new(None), exited: AtomicBool::new(false), mouse_reporting: AtomicBool::new(false), log };
        FrameSlot { inner: Arc::new(inner) }
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

    fn wake(&self) {
        if let Some(waker) = self.inner.waker.lock().unwrap().as_ref() {
            waker();
        }
    }

    /// The child closed the PTY: `exited` turns true, then the waker fires.
    pub(crate) fn mark_exited(&self) {
        self.inner.exited.store(true, Ordering::SeqCst);
        self.wake();
    }

    pub(crate) fn exited(&self) -> bool {
        self.inner.exited.load(Ordering::SeqCst)
    }

    /// Stored before the frame that follows a mode change is published.
    pub(crate) fn set_mouse_reporting(&self, on: bool) {
        self.inner.mouse_reporting.store(on, Ordering::SeqCst);
    }

    /// The program has a mouse tracking mode set (9, 1000, 1002 or 1003), as of the newest frame
    /// published — taken or not. An atomic read: the front-end asks on every mouse event.
    pub fn mouse_reporting(&self) -> bool {
        self.inner.mouse_reporting.load(Ordering::SeqCst)
    }

    pub fn take(&self) -> Option<Frame> {
        let frame = self.inner.frame.lock().unwrap().take()?;
        if let Some(log) = &self.inner.log {
            log.frame_taken(frame.seq, frame.t_output_ns, &frame.keys);
        }
        Some(frame)
    }

    /// Installs the wake callback, replacing any earlier one. It is called after every publish
    /// and when the child exits — and once from here, before this returns, if a frame is already
    /// waiting or the child has already exited, so a waker installed late misses nothing.
    /// It may run on any thread and may run spuriously; it must not call `set_waker`.
    pub fn set_waker(&self, f: Box<dyn Fn() + Send + Sync>) {
        *self.inner.waker.lock().unwrap() = Some(f);
        // The waker is stored before the checks, and `publish` / `mark_exited` store their state
        // before they read the waker: whichever runs second sees the other's write.
        let pending = self.inner.frame.lock().unwrap().is_some();
        if pending || self.exited() {
            self.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn frame(seq: u64, rows: u16, dirty: &[u16], keys: &[u64]) -> Frame {
        let mut dirty_rows = vec![false; rows as usize];
        for &y in dirty {
            dirty_rows[y as usize] = true;
        }
        Frame {
            cols: 2,
            rows,
            cells: vec![Cell::default(); 2 * rows as usize],
            cursor: None,
            dirty_rows,
            seq,
            t_output_ns: seq as u128,
            keys: keys.to_vec(),
        }
    }

    fn counting_waker(slot: &FrameSlot) -> Arc<AtomicUsize> {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        slot.set_waker(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        calls
    }

    #[test]
    fn slot_keeps_only_the_latest_frame() {
        let slot = FrameSlot::new(None);
        assert!(slot.take().is_none());
        for seq in 1..=3 {
            slot.publish(frame(seq, 4, &[], &[]));
        }
        assert_eq!(slot.take().map(|f| f.seq), Some(3));
        assert!(slot.take().is_none(), "one slot, not a queue");
    }

    #[test]
    fn slot_carries_keys_and_dirty_rows_of_a_replaced_frame() {
        let slot = FrameSlot::new(None);
        slot.publish(frame(1, 4, &[0], &[7]));
        slot.publish(frame(2, 4, &[2], &[]));
        slot.publish(frame(3, 4, &[], &[8, 9]));
        let merged = slot.take().unwrap();
        assert_eq!(merged.seq, 3);
        assert_eq!(merged.keys, [7, 8, 9], "keys of replaced frames come first, in order");
        assert_eq!(merged.dirty_rows, [true, false, true, false]);

        // Nothing carries over from a frame that was taken.
        slot.publish(frame(4, 4, &[3], &[10]));
        let next = slot.take().unwrap();
        assert_eq!((next.keys, next.dirty_rows), (vec![10], vec![false, false, false, true]));

        // A replaced frame of another size makes every row dirty.
        slot.publish(frame(5, 4, &[1], &[11]));
        slot.publish(frame(6, 6, &[0], &[]));
        let resized = slot.take().unwrap();
        assert_eq!((resized.keys, resized.dirty_rows), (vec![11], vec![true; 6]));
    }

    #[test]
    fn waker_fires_on_publish_and_exit() {
        let slot = FrameSlot::new(None);
        let calls = counting_waker(&slot);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "nothing pending yet");
        slot.publish(frame(1, 1, &[], &[]));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!slot.exited());
        slot.mark_exited();
        assert!(slot.exited());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn waker_set_late_fires_at_once() {
        let pending = FrameSlot::new(None);
        pending.publish(frame(1, 1, &[], &[]));
        assert_eq!(counting_waker(&pending).load(Ordering::SeqCst), 1, "a frame was waiting");

        let exited = FrameSlot::new(None);
        exited.mark_exited();
        assert_eq!(counting_waker(&exited).load(Ordering::SeqCst), 1, "the child had exited");

        let idle = FrameSlot::new(None);
        assert_eq!(counting_waker(&idle).load(Ordering::SeqCst), 0);
    }
}
