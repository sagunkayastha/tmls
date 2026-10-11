//! Latency marks of the render side: `t_submit_ns`, `t_present_ns` and `t_work_done_ns` of the
//! `frame` line (contract: "Latency log").
//!
//! - `t_submit_ns`  — taken immediately before `queue.submit` is called.
//! - `t_present_ns` — taken when `present()` has returned; `present_kind` is `"present_return"`.
//! - `t_work_done_ns` — when the `queue.on_submitted_work_done` callback of that submission ran.
//!   wgpu runs the callback from inside `device.poll`, which the app calls on its own thread, so
//!   this time is late by up to the app's polling interval.
//!
//! - `t_redraw_ns` — taken at the entry of the redraw that presented the frame; on every
//!   `frame` line. From then until `present()` returns the main thread takes no events, so a
//!   key arriving in that window gets its `t_key_ns` late.
//!
//! None of these is the time light left the screen.
//!
//! The GPU calls sit behind `PresentPath`, so the order of marks and calls is tested with a
//! fake. One `frame` line is written per presented frame, in frame order, once its work-done
//! time is known — or without that field if the callback has not run within
//! `WORK_DONE_WAIT_NS`, or by the time the app exits.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// `present_kind` of every line this front-end writes.
pub const PRESENT_KIND: &str = "present_return";
/// How long a `frame` line waits for its work-done time.
pub const WORK_DONE_WAIT_NS: u128 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLine {
    /// `Frame::seq` of the frame that was drawn.
    pub seq: u64,
    /// When the redraw that presented this frame began (`Marks::begin_redraw`).
    pub t_redraw_ns: u128,
    pub t_submit_ns: u128,
    pub t_present_ns: u128,
    pub t_work_done_ns: Option<u128>,
}

/// Where finished lines go: the latency log, or a `Vec` in tests.
pub trait FrameSink {
    fn frame(&mut self, line: FrameLine);
}

impl FrameSink for s2_core::latency::Log {
    fn frame(&mut self, line: FrameLine) {
        self.frame_with_marks(
            line.seq,
            line.t_submit_ns,
            line.t_present_ns,
            PRESENT_KIND,
            line.t_work_done_ns,
            Some(line.t_redraw_ns),
        );
    }
}

impl FrameSink for Vec<FrameLine> {
    fn frame(&mut self, line: FrameLine) {
        self.push(line);
    }
}

/// Set once by the work-done callback of one submission (on whatever thread runs it).
#[derive(Clone, Debug, Default)]
pub struct WorkDone(Arc<AtomicU64>);

impl WorkDone {
    /// Records the time; only the first call counts.
    pub fn mark(&self, t_ns: u128) {
        let t = (t_ns.min(u64::MAX as u128) as u64).max(1);
        let _ = self
            .0
            .compare_exchange(0, t, Ordering::SeqCst, Ordering::SeqCst);
    }

    pub fn get(&self) -> Option<u128> {
        match self.0.load(Ordering::SeqCst) {
            0 => None,
            t => Some(t as u128),
        }
    }
}

/// The two GPU calls of a frame.
pub trait PresentPath {
    /// `queue.submit` of the frame's commands; then registers `done.mark(now)` as the
    /// submission's work-done callback.
    fn submit(&mut self, done: WorkDone);
    /// `present()` of the frame's surface texture.
    fn present(&mut self);
}

struct Pending {
    line: FrameLine,
    done: WorkDone,
}

pub struct Marks<S: FrameSink> {
    sink: S,
    clock: Box<dyn Fn() -> u128>,
    /// Presented frames whose line is not written yet, oldest first.
    pending: VecDeque<Pending>,
    /// Entry time of the redraw in progress.
    redraw_ns: Option<u128>,
}

impl<S: FrameSink> Marks<S> {
    pub fn new(sink: S) -> Marks<S> {
        Marks::with_clock(sink, Box::new(s2_core::now_ns))
    }

    /// With another clock than `s2_core::now_ns` (tests).
    pub fn with_clock(sink: S, clock: Box<dyn Fn() -> u128>) -> Marks<S> {
        Marks {
            sink,
            clock,
            pending: VecDeque::new(),
            redraw_ns: None,
        }
    }

    /// Call as the first statement of a redraw. From here until `present()` returns the main
    /// thread takes no events, so a key that arrives in that window gets its `t_key_ns` late.
    /// If the redraw ends without a present, the next call replaces the stamp.
    pub fn begin_redraw(&mut self) {
        self.redraw_ns = Some((self.clock)());
    }

    /// Submits and presents the frame `seq` through `path`, taking the marks around the calls.
    /// The line is written by a later `settle` (or `finish`).
    pub fn present_frame(&mut self, seq: u64, path: &mut impl PresentPath) {
        let done = WorkDone::default();
        let t_submit_ns = (self.clock)();
        path.submit(done.clone());
        path.present();
        let t_present_ns = (self.clock)();
        let line = FrameLine {
            seq,
            t_redraw_ns: self.redraw_ns.take().unwrap_or(t_submit_ns),
            t_submit_ns,
            t_present_ns,
            t_work_done_ns: None,
        };
        self.pending.push_back(Pending { line, done });
    }

    /// Writes the lines that are ready, oldest first, and returns how many still wait. A line
    /// never passes an older one: `Log::frame(seq)` gives a line the keys of every frame taken
    /// up to `seq`, so a newer line written first would take an older frame's keys and pair
    /// them with its own, later, times.
    pub fn settle(&mut self) -> usize {
        while let Some(front) = self.pending.front() {
            let t_work_done_ns = front.done.get();
            let waited = (self.clock)().saturating_sub(front.line.t_present_ns);
            if t_work_done_ns.is_none() && waited < WORK_DONE_WAIT_NS {
                break;
            }
            let mut line = front.line;
            line.t_work_done_ns = t_work_done_ns;
            self.sink.frame(line);
            self.pending.pop_front();
        }
        self.pending.len()
    }

    /// Writes every pending line now, with the work-done time where it is known (app exit).
    pub fn finish(&mut self) {
        for pending in std::mem::take(&mut self.pending) {
            let mut line = pending.line;
            line.t_work_done_ns = pending.done.get();
            self.sink.frame(line);
        }
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }
}
