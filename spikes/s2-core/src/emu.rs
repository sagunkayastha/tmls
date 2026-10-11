//! The emulator thread. libghostty-vt objects are not `Send`, so the terminal, its render
//! state and the key encoder are created on this thread and never leave it.

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use libghostty_vt::render::{CellIterator, CursorVisualStyle, Dirty, RenderState, RowIterator};
use libghostty_vt::screen::CellWide;
use libghostty_vt::style::Underline;
use libghostty_vt::terminal::{Mode, Options, Point, PointCoordinate, Terminal};

use crate::frame::{Cell, CellFlags, Cursor, CursorShape, Frame, FrameSlot, Wide};
use crate::keys::{self, KeyEvent};
use crate::latency::Log;
use crate::now_ns;
use crate::pty::{Pty, PtyWriter};

const SCROLLBACK: usize = 10_000;
/// The PTY reader waits while this many bytes are queued, so a flood cannot build a backlog
/// that later output (a key's echo) has to wait behind.
const MAX_QUEUED: usize = 256 * 1024;

#[derive(Debug)]
pub enum InputMsg {
    /// PTY output and `t_output_ns`, taken right after `read()` returned.
    Bytes(Vec<u8>, u128),
    Resize(u16, u16),
    /// Key event and its key id (the front-end has already logged `key`).
    Key(KeyEvent, u64),
    Paste(String),
    /// Window focus. `false` drops queued key repeats; both report to the PTY under mode 1004.
    Focus(bool),
    Shutdown,
}

pub struct Emu {
    pub input_tx: Sender<InputMsg>,
    pub frames: FrameSlot,
    exited: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Emu {
    pub fn start(pty: Pty, cols: u16, rows: u16, log: Option<Log>) -> anyhow::Result<Emu> {
        let (input_tx, rx) = channel();
        let frames = FrameSlot::new(log.clone());
        let exited = Arc::new(AtomicBool::new(false));
        let queued = Arc::new(AtomicUsize::new(0));
        spawn_reader(pty.reader()?, input_tx.clone(), queued.clone(), exited.clone(), frames.clone())?;
        let (ready_tx, ready_rx) = channel();
        let slot = frames.clone();
        let thread = std::thread::Builder::new().name("s2-emu".into()).spawn(move || {
            match State::new(pty, cols, rows, log, slot, queued) {
                Ok(mut state) => {
                    let _ = ready_tx.send(Ok(()));
                    state.run(rx);
                }
                Err(e) => drop(ready_tx.send(Err(e))),
            }
        })?;
        ready_rx.recv()??;
        Ok(Emu { input_tx, frames, exited, thread: Some(thread) })
    }

    /// True once the PTY reached end of file (the child exited). The waker fires when it does.
    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::Relaxed)
    }
}

impl Drop for Emu {
    fn drop(&mut self) {
        let _ = self.input_tx.send(InputMsg::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    tx: Sender<InputMsg>,
    queued: Arc<AtomicUsize>,
    exited: Arc<AtomicBool>,
    frames: FrameSlot,
) -> std::io::Result<()> {
    std::thread::Builder::new().name("s2-pty-read".into()).spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            while queued.load(Ordering::Relaxed) > MAX_QUEUED {
                std::thread::sleep(Duration::from_micros(200));
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let t_output_ns = now_ns();
                    queued.fetch_add(n, Ordering::Relaxed);
                    if tx.send(InputMsg::Bytes(buf[..n].to_vec(), t_output_ns)).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        exited.store(true, Ordering::Relaxed);
        frames.wake();
    })?;
    Ok(())
}

/// Removes key repeats that must not reach the PTY: those queued ahead of a focus loss in
/// this batch, and any that arrive until a fresh press. `repeat_ok` carries across batches.
fn drop_stale_repeats(batch: Vec<InputMsg>, repeat_ok: &mut bool) -> Vec<InputMsg> {
    let focus_lost_at = batch.iter().rposition(|m| matches!(m, InputMsg::Focus(false)));
    let mut kept = Vec::with_capacity(batch.len());
    for (i, msg) in batch.into_iter().enumerate() {
        match &msg {
            InputMsg::Key(ev, _) if ev.pressed && ev.repeat => {
                if !*repeat_ok || focus_lost_at.is_some_and(|at| i < at) {
                    continue;
                }
            }
            InputMsg::Key(ev, _) if ev.pressed => *repeat_ok = true,
            InputMsg::Focus(false) => *repeat_ok = false,
            _ => {}
        }
        kept.push(msg);
    }
    kept
}

struct State {
    term: Terminal<'static, 'static>,
    render: RenderState<'static>,
    row_iter: RowIterator<'static>,
    cell_iter: CellIterator<'static>,
    encoder: keys::Encoder,
    pty: Pty,
    writer: PtyWriter,
    log: Option<Log>,
    frames: FrameSlot,
    queued: Arc<AtomicUsize>,
    /// The viewport as last read; rows the render state reports clean are reused.
    cells: Vec<Cell>,
    size: (u16, u16),
    seq: u64,
    /// Written keys not yet attributed to a frame: (key id, when its PTY write began).
    unseen_keys: Vec<(u64, u128)>,
    repeat_ok: bool,
}

impl State {
    fn new(
        pty: Pty,
        cols: u16,
        rows: u16,
        log: Option<Log>,
        frames: FrameSlot,
        queued: Arc<AtomicUsize>,
    ) -> anyhow::Result<State> {
        let mut term = Terminal::new(Options { cols, rows, max_scrollback: SCROLLBACK })?;
        let writer = pty.writer();
        // Replies to queries (CPR, DA, DECRQM, …) take the same writer as keys: one input path.
        let replies = writer.clone();
        term.on_pty_write(move |_, data| drop(replies.write_all(data)))?;
        Ok(State {
            term,
            render: RenderState::new()?,
            row_iter: RowIterator::new()?,
            cell_iter: CellIterator::new()?,
            encoder: keys::Encoder::new()?,
            pty,
            writer,
            log,
            frames,
            queued,
            cells: Vec::new(),
            size: (0, 0),
            seq: 0,
            unseen_keys: Vec::new(),
            repeat_ok: true,
        })
    }

    fn run(&mut self, rx: Receiver<InputMsg>) {
        self.publish(now_ns());
        while let Ok(first) = rx.recv() {
            let mut batch = vec![first];
            batch.extend(rx.try_iter());
            // Input is handled before this batch's output so a flood never delays a key.
            let mut output = Vec::new();
            let mut resized = false;
            for msg in drop_stale_repeats(batch, &mut self.repeat_ok) {
                match msg {
                    InputMsg::Bytes(bytes, t_output_ns) => output.push((bytes, t_output_ns)),
                    InputMsg::Key(ev, id) => self.key(&ev, id),
                    InputMsg::Paste(text) => self.paste(&text),
                    InputMsg::Focus(focused) => self.focus(focused),
                    InputMsg::Resize(cols, rows) => resized |= self.resize(cols, rows),
                    InputMsg::Shutdown => return,
                }
            }
            let mut t_output_ns = resized.then(now_ns);
            for (bytes, t) in output {
                self.term.vt_write(&bytes);
                let _ = self.queued.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |q| {
                    Some(q.saturating_sub(bytes.len()))
                });
                t_output_ns = Some(t);
            }
            if let Some(t) = t_output_ns {
                self.publish(t);
            }
        }
    }

    fn send(&mut self, bytes: &[u8]) -> bool {
        !bytes.is_empty() && self.writer.write_all(bytes).is_ok()
    }

    fn key(&mut self, ev: &KeyEvent, id: u64) {
        let Ok(bytes) = self.encoder.encode(ev, &self.term) else { return };
        // The echo can be read (and stamped) by the reader thread before `write_all` returns
        // here, so attribution compares against the time the write began.
        let t_write_began_ns = now_ns();
        if self.send(&bytes) {
            if let Some(log) = &self.log {
                log.write(id, now_ns());
            }
            self.unseen_keys.push((id, t_write_began_ns));
        }
    }

    fn paste(&mut self, text: &str) {
        let bracketed = self.term.mode(Mode::BRACKETED_PASTE).unwrap_or(false);
        if let Ok(bytes) = keys::paste_bytes(text, bracketed) {
            self.send(&bytes);
        }
    }

    fn focus(&mut self, focused: bool) {
        if self.term.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
            self.send(if focused { b"\x1b[I" } else { b"\x1b[O" });
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) -> bool {
        // Cell pixel size only feeds image protocols and size reports; S2 draws neither.
        self.term.resize(cols, rows, 8, 16).is_ok() && self.pty.resize(cols, rows).is_ok()
    }

    fn publish(&mut self, t_output_ns: u128) {
        let (cursor, dirty_rows) = match self.read_viewport() {
            Ok(read) => read,
            Err(e) => return eprintln!("s2-core: reading the render state failed: {e:#}"),
        };
        // A key belongs to the first frame whose output was read at or after its write began.
        let mut keys = Vec::new();
        self.unseen_keys.retain(|&(id, t_write_began_ns)| {
            let seen = t_write_began_ns <= t_output_ns;
            if seen {
                keys.push(id);
            }
            !seen
        });
        self.seq += 1;
        self.frames.publish(Frame {
            cols: self.size.0,
            rows: self.size.1,
            cells: self.cells.clone(),
            cursor,
            dirty_rows,
            seq: self.seq,
            t_output_ns,
            keys,
        });
    }

    /// Refreshes `self.cells` from the render state (dirty rows only) and returns the cursor
    /// and which rows changed.
    fn read_viewport(&mut self) -> anyhow::Result<(Option<Cursor>, Vec<bool>)> {
        let State { term, render, row_iter, cell_iter, cells, size, .. } = self;
        let snapshot = render.update(term)?;
        let (cols, rows) = (snapshot.cols()?, snapshot.rows()?);
        let full = snapshot.dirty()? == Dirty::Full || *size != (cols, rows);
        if *size != (cols, rows) {
            cells.clear();
            cells.resize(cols as usize * rows as usize, Cell::default());
            *size = (cols, rows);
        }
        let (cols, rows) = (cols as usize, rows as usize);
        let mut dirty_rows = vec![full; rows];
        let mut linked = Vec::new();
        {
            let mut row_iteration = row_iter.update(&snapshot)?;
            let mut y = 0;
            while let Some(row) = row_iteration.next() {
                if y >= rows {
                    break;
                }
                if full || row.dirty()? {
                    dirty_rows[y] = true;
                    let mut x = 0;
                    {
                        let mut cell_iteration = cell_iter.update(row)?;
                        while let Some(c) = cell_iteration.next() {
                            if x >= cols {
                                break;
                            }
                            let (style, raw) = (c.style()?, c.raw_cell()?);
                            let cell = &mut cells[y * cols + x];
                            cell.text.clear();
                            if c.graphemes_len()? > 0 {
                                c.graphemes_utf8(&mut cell.text)?;
                            }
                            cell.fg = c.fg_color()?.map(|c| (c.r, c.g, c.b));
                            cell.bg = c.bg_color()?.map(|c| (c.r, c.g, c.b));
                            cell.flags = CellFlags {
                                bold: style.bold,
                                italic: style.italic,
                                underline: style.underline != Underline::None,
                                inverse: style.inverse,
                                strike: style.strikethrough,
                                faint: style.faint,
                            };
                            cell.wide = match raw.wide()? {
                                CellWide::Wide => Wide::Lead,
                                CellWide::SpacerTail => Wide::Spacer,
                                CellWide::Narrow | CellWide::SpacerHead => Wide::Narrow,
                            };
                            cell.hyperlink = None;
                            if raw.has_hyperlink()? {
                                linked.push(y * cols + x);
                            }
                            x += 1;
                        }
                    }
                    cells[y * cols + x..(y + 1) * cols].fill(Cell::default());
                    row.set_dirty(false)?;
                }
                y += 1;
            }
        }
        let cursor = match snapshot.cursor_viewport()? {
            None => None,
            Some(at) => Some(Cursor {
                col: at.x,
                row: at.y,
                shape: match snapshot.cursor_visual_style()? {
                    CursorVisualStyle::Bar => CursorShape::Bar,
                    CursorVisualStyle::Underline => CursorShape::Underline,
                    _ => CursorShape::Block,
                },
                visible: snapshot.cursor_visible()?,
            }),
        };
        snapshot.set_dirty(Dirty::Clean)?;
        // Hyperlink URIs are not in the render state; resolve them per linked cell.
        let mut uri = vec![0u8; 1024];
        for i in linked {
            let at = PointCoordinate { x: (i % cols) as u16, y: (i / cols) as u32 };
            let grid_ref = term.grid_ref(Point::Viewport(at))?;
            let len = loop {
                match grid_ref.hyperlink_uri(&mut uri) {
                    Ok(len) => break len,
                    Err(libghostty_vt::error::Error::OutOfSpace { required }) => uri.resize(required, 0),
                    Err(e) => return Err(e.into()),
                }
            };
            if len > 0 {
                cells[i].hyperlink = Some(String::from_utf8_lossy(&uri[..len]).into_owned());
            }
        }
        Ok((cursor, dirty_rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Key, Mods};

    fn key(repeat: bool) -> InputMsg {
        let ev = KeyEvent { key: Key::Char('a'), mods: Mods::default(), text: Some("a".into()), repeat, pressed: true };
        InputMsg::Key(ev, 0)
    }

    fn repeats(batch: &[InputMsg]) -> usize {
        batch.iter().filter(|m| matches!(m, InputMsg::Key(ev, _) if ev.repeat)).count()
    }

    #[test]
    fn keys_no_repeat_after_focus_loss() {
        let mut repeat_ok = true;
        // Repeats queued ahead of the focus loss, and the one after it, are dropped; the press stays.
        let batch = vec![key(false), key(true), key(true), InputMsg::Focus(false), key(true)];
        let kept = drop_stale_repeats(batch, &mut repeat_ok);
        assert_eq!(repeats(&kept), 0);
        assert_eq!(kept.len(), 2, "the press and the focus marker remain: {kept:?}");

        // Still unfocused in a later batch, and after focus returns: no repeat without a new press.
        assert_eq!(repeats(&drop_stale_repeats(vec![key(true)], &mut repeat_ok)), 0);
        assert_eq!(repeats(&drop_stale_repeats(vec![InputMsg::Focus(true), key(true)], &mut repeat_ok)), 0);

        // A fresh press makes repeats flow again.
        assert_eq!(repeats(&drop_stale_repeats(vec![key(false), key(true), key(true)], &mut repeat_ok)), 2);
    }
}
