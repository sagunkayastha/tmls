//! A single-process model of the attach handoff: snapshot at an output offset,
//! buffered bytes after that offset, and ordered resize epochs.

use std::cell::Cell;
use std::rc::Rc;

use libghostty_vt::terminal::{Options, Terminal};
use serde::Serialize;

use crate::carry::{self, Tracker};
use crate::experiment;
use crate::grid;
use crate::snap::SnapOpts;
use crate::splitter::{self, CutState};

#[derive(Clone, Debug)]
pub struct Handoff {
    pub bytes: Vec<u8>,
    pub offset: u64,
    pub epoch: u32,
    pub cols: u16,
    pub rows: u16,
    pub scrollback: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ResizeEvent {
    pub offset: u64,
    pub epoch: u32,
    pub cols: u16,
    pub rows: u16,
}

pub struct Worker {
    pub term: Terminal<'static, 'static>,
    pub tracker: Tracker,
    pub log: Vec<u8>,
    pub offset: u64,
    pub epoch: u32,
    pub cols: u16,
    pub rows: u16,
    pub scrollback: usize,
    replies: Rc<Cell<usize>>,
}

impl Worker {
    pub fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        let mut term = Terminal::new(Options { cols, rows, max_scrollback: scrollback }).expect("worker terminal");
        let replies = Rc::new(Cell::new(0));
        let count = replies.clone();
        term.on_pty_write(move |_, _| count.set(count.get() + 1)).expect("reply callback");
        Self { term, tracker: Tracker::default(), log: Vec::new(), offset: 0, epoch: 0, cols, rows, scrollback, replies }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.tracker.feed(&mut self.term, bytes);
        self.log.extend_from_slice(bytes);
        self.offset += bytes.len() as u64;
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(cols, rows, 8, 16).expect("worker resize");
        self.cols = cols;
        self.rows = rows;
        self.epoch += 1;
    }

    pub fn snapshot(&mut self) -> Handoff {
        assert_eq!(splitter::cut_states(&self.log).last(), Some(&CutState::Ground), "handoff requires a VT boundary");
        Handoff {
            bytes: carry::snapshot_full(&mut self.term, &self.tracker, SnapOpts { palette: false }),
            offset: self.offset, epoch: self.epoch, cols: self.cols, rows: self.rows,
            scrollback: self.scrollback,
        }
    }

    pub fn reply_attempts(&self) -> usize { self.replies.get() }
}

pub struct Client {
    pub term: Terminal<'static, 'static>,
    pub offset: u64,
    pub epoch: u32,
    replies: Rc<Cell<usize>>,
    suppressed_side_effects: Rc<Cell<usize>>,
    pending: Vec<u8>,
}

impl Client {
    pub fn attach(h: &Handoff) -> Self {
        let mut term = Terminal::new(Options { cols: h.cols, rows: h.rows, max_scrollback: h.scrollback }).expect("client terminal");
        let replies = Rc::new(Cell::new(0));
        let count = replies.clone();
        term.on_pty_write(move |_, _| count.set(count.get() + 1)).expect("reply callback");
        let suppressed_side_effects = Rc::new(Cell::new(0));
        let bell = suppressed_side_effects.clone();
        term.on_bell(move |_| bell.set(bell.get() + 1)).expect("bell callback");
        let title = suppressed_side_effects.clone();
        term.on_title_changed(move |_| title.set(title.get() + 1)).expect("title callback");
        let clipboard = suppressed_side_effects.clone();
        term.on_clipboard_write(move |_, _| { clipboard.set(clipboard.get() + 1); Ok(()) }).expect("clipboard callback");
        term.vt_write(&h.bytes);
        Self { term, offset: h.offset, epoch: h.epoch, replies, suppressed_side_effects, pending: Vec::new() }
    }

    pub fn apply(&mut self, bytes_after_offset: &[u8], resizes_after_epoch: &[ResizeEvent]) {
        let start = self.offset;
        let end = start + bytes_after_offset.len() as u64;
        let mut cursor = 0usize;
        for event in resizes_after_epoch {
            assert!(event.offset >= start + cursor as u64 && event.offset <= end);
            assert_eq!(event.epoch, self.epoch + 1);
            let next = (event.offset - start) as usize;
            self.apply_bytes(&bytes_after_offset[cursor..next]);
            self.term.resize(event.cols, event.rows, 8, 16).expect("client resize");
            self.epoch = event.epoch;
            cursor = next;
        }
        self.apply_bytes(&bytes_after_offset[cursor..]);
        self.offset = end;
    }

    fn apply_bytes(&mut self, bytes: &[u8]) {
        // Queries change no compared terminal state. Removing complete query
        // units keeps this replica from invoking its reply callback. A query
        // can cross delivery chunks, so retain the incomplete VT suffix.
        self.pending.extend_from_slice(bytes);
        let states = splitter::cut_states(&self.pending);
        let end = (1..=self.pending.len())
            .rev()
            .find(|&p| states[p] == CutState::Ground)
            .unwrap_or(0);
        if end == 0 { return; }
        let ready: Vec<u8> = self.pending.drain(..end).collect();
        let bounds = splitter::boundaries(&ready);
        for w in bounds.windows(2) {
            let unit = &ready[w[0]..w[1]];
            if !is_reply_query(unit) { self.term.vt_write(unit); }
        }
    }

    pub fn reply_attempts(&self) -> usize { self.replies.get() }
    pub fn suppressed_side_effects(&self) -> usize { self.suppressed_side_effects.get() }
}

fn is_reply_query(unit: &[u8]) -> bool {
    unit == b"\x1b[6n" || unit == b"\x1b[5n"
        || unit == b"\x1b[c" || unit == b"\x1b[0c"
        || (unit.starts_with(b"\x1b[?") && unit.ends_with(b"$p"))
        || unit == b"\x1b[>q"
}

#[derive(Clone, Debug, Serialize)]
pub struct HandoffResult {
    pub split: usize,
    pub resize: bool,
    pub identical: bool,
    pub class: Option<String>,
    pub first_mismatch: Option<String>,
    pub client_reply_attempts: usize,
    pub worker_reply_attempts: usize,
    pub client_suppressed_side_effects: usize,
}

/// Two deliveries of one fixed snapshot: plain tail and tail with an ordered
/// resize event. The worker keeps parsing while the snapshot is in flight.
pub fn simulate(bytes: &[u8], cols: u16, rows: u16, scrollback: usize, split: usize) -> Vec<HandoffResult> {
    [false, true].into_iter().map(|resize| simulate_one(bytes, cols, rows, scrollback, split, resize)).collect()
}

fn simulate_one(bytes: &[u8], cols: u16, rows: u16, scrollback: usize, split: usize, resize: bool) -> HandoffResult {
    assert!(split <= bytes.len());
    let mut worker = Worker::new(cols, rows, scrollback);
    worker.feed(&bytes[..split]);
    let snapshot_alt = worker.term.active_screen().expect("screen") == libghostty_vt::screen::Screen::Alternate;
    let h = worker.snapshot();
    let mut events = Vec::new();
    if resize {
        let bounds = splitter::boundaries(&bytes[split..]);
        let middle = bytes.len().saturating_sub(split) / 2;
        let relative = *bounds.iter().min_by_key(|&&p| (p.abs_diff(middle), p)).expect("boundary");
        let at = split + relative;
        worker.feed(&bytes[split..at]);
        worker.resize(100, 30);
        events.push(ResizeEvent { offset: at as u64, epoch: worker.epoch, cols: 100, rows: 30 });
        worker.feed(&bytes[at..]);
    } else {
        worker.feed(&bytes[split..]);
    }
    let mut client = Client::attach(&h);
    client.apply(&worker.log[h.offset as usize..], &events);
    let mut d = grid::capture(&mut worker.term).expect("worker capture");
    let mut c = grid::capture(&mut client.term).expect("client capture");
    grid::normalize(&mut d.0);
    grid::normalize(&mut c.0);
    let d_hist = experiment::history_text(&worker.term);
    let c_hist = experiment::history_text(&client.term);
    let diff = grid::diff(&d, &c);
    let identical = diff.is_identical() && d_hist == c_hist;
    let (class, first_mismatch) = if identical { (None, None) } else {
        let (class, detail) = experiment::classify_aligned(snapshot_alt, &bytes[split..], &diff, &d, &c, &d_hist, &c_hist);
        (Some(class), Some(detail))
    };
    HandoffResult {
        split, resize, identical, class, first_mismatch,
        client_reply_attempts: client.reply_attempts(),
        worker_reply_attempts: worker.reply_attempts(),
        client_suppressed_side_effects: client.suppressed_side_effects(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{experiment::history_text, grid};

    fn equal(a: &mut Terminal, b: &mut Terminal) -> bool {
        let mut aa = grid::capture(a).unwrap();
        let mut bb = grid::capture(b).unwrap();
        grid::normalize(&mut aa.0);
        grid::normalize(&mut bb.0);
        grid::diff(&aa, &bb).is_identical() && history_text(a) == history_text(b)
    }

    #[test]
    fn handoff_identical_quiet() {
        let mut worker = Worker::new(20, 4, 2000);
        worker.feed(b"primary\x1b[?1049halt");
        let h = worker.snapshot();
        worker.feed(b"\x1b[?1049lmore");
        let mut client = Client::attach(&h);
        client.apply(&worker.log[h.offset as usize..], &[]);
        assert!(equal(&mut worker.term, &mut client.term));
        assert_eq!(client.reply_attempts(), 0);
    }

    #[test]
    fn handoff_with_resize_epoch() {
        let mut worker = Worker::new(20, 4, 2000);
        worker.feed(b"one\r\ntwo\r\n");
        let h = worker.snapshot();
        worker.feed(b"three");
        let at = worker.offset;
        worker.resize(30, 6);
        worker.feed(b"\r\nfour");
        let mut client = Client::attach(&h);
        client.apply(&worker.log[h.offset as usize..], &[ResizeEvent { offset: at, epoch: worker.epoch, cols: 30, rows: 6 }]);
        assert_eq!(client.epoch, worker.epoch);
        assert!(equal(&mut worker.term, &mut client.term));
    }

    #[test]
    fn client_never_replies() {
        let mut worker = Worker::new(20, 4, 2000);
        worker.feed(b"ready");
        let h = worker.snapshot();
        worker.feed(b"\x1b[6n");
        assert_eq!(worker.reply_attempts(), 1);
        let mut client = Client::attach(&h);
        client.apply(&worker.log[h.offset as usize..], &[]);
        assert_eq!(client.reply_attempts(), 0);
        assert!(equal(&mut worker.term, &mut client.term));
    }

    #[test]
    fn client_query_split_across_deliveries() {
        let mut worker = Worker::new(20, 4, 2000);
        worker.feed(b"ready");
        let h = worker.snapshot();
        let mut client = Client::attach(&h);
        client.apply(b"\x1b[", &[]);
        client.apply(b"6n", &[]);
        assert_eq!(client.reply_attempts(), 0);
        assert!(equal(&mut worker.term, &mut client.term));
    }

    #[test]
    fn simulation_reports_both_variants() {
        let b = b"one\r\ntwo\r\nthree\x1b[6n";
        let results = simulate(b, 20, 5, 2000, 5);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.identical && r.client_reply_attempts == 0), "{results:?}");
        assert_eq!(results.iter().filter(|r| r.resize).count(), 1);
    }
}
