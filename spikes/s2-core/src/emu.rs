//! The emulator thread. libghostty-vt objects are not `Send`, so the terminal, its render
//! state and the key encoder are created on this thread and never leave it.

use std::io::Read;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use libghostty_vt::render::{CellIterator, CursorVisualStyle, Dirty, RenderState, RowIterator};
use libghostty_vt::screen::CellWide;
use libghostty_vt::style::Underline;
use libghostty_vt::terminal::{Mode, Options, Point, PointCoordinate, ScrollViewport, Terminal};

use crate::frame::{Cell, CellFlags, Cursor, CursorShape, Frame, FrameSlot, Wide};
use crate::keys::{self, KeyEvent, Mods, MouseButton, MouseKind};
use crate::latency::Log;
use crate::now_ns;
use crate::osc52;
use crate::pty::{Pty, PtyReader, PtyWriter, Wake};

const SCROLLBACK: usize = 10_000;
/// The PTY reader waits while this many bytes are queued, so a flood cannot build a backlog
/// that later output (a key's echo) has to wait behind.
const MAX_QUEUED: usize = 256 * 1024;
/// PTY output is parsed this many bytes at a time, checking for shutdown in between.
const PARSE_PIECE: usize = 4096;

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
    /// Scrolls the viewport by this many rows: negative towards older output (wheel up),
    /// positive towards the live screen. Clamped at both ends of the scrollback; the alternate
    /// screen has none. A frame follows. Output does not move a scrolled viewport back; a key or
    /// paste that sends bytes does, and so does `ScrollToBottom`.
    Scroll(i32),
    /// Returns the viewport to the live screen. A frame follows.
    ScrollToBottom,
    /// One mouse event at a 0-based viewport cell, for the program: written to the PTY as the
    /// report the terminal's mouse modes ask for (tracking 1000 / 1002 / 1003, format SGR 1006
    /// or X10), and not at all when the program did not ask for it — see
    /// `keys::MouseEncoder::encode`. `button`: the one pressed or released; for `Motion` the one
    /// held (`None`: none). The wheel is `Press` of `WheelUp` / `WheelDown`. Unlike a key it
    /// does not move a scrolled viewport, gets no key id and no latency line.
    /// Whether to send it at all is the front-end's choice: `Emu::mouse_reporting`.
    Mouse { kind: MouseKind, button: Option<MouseButton>, col: u16, row: u16, mods: Mods },
    Shutdown,
}

pub struct Emu {
    pub input_tx: Sender<InputMsg>,
    pub frames: FrameSlot,
    thread: Option<JoinHandle<()>>,
    reader: Option<(JoinHandle<()>, Arc<Backlog>, Arc<Wake>)>,
}

impl Emu {
    pub fn start(pty: Pty, cols: u16, rows: u16, log: Option<Log>) -> anyhow::Result<Emu> {
        let (input_tx, rx) = channel();
        let frames = FrameSlot::new(log.clone());
        let backlog = Arc::new(Backlog::default());
        let reader = pty.stoppable_reader()?;
        let (ready_tx, ready_rx) = channel();
        let (slot, counted) = (frames.clone(), backlog.clone());
        let thread = std::thread::Builder::new().name("s2-emu".into()).spawn(move || {
            match State::new(pty, cols, rows, log, slot, counted) {
                Ok(mut state) => {
                    let _ = ready_tx.send(Ok(()));
                    state.run(rx);
                }
                Err(e) => drop(ready_tx.send(Err(e))),
            }
        })?;
        ready_rx.recv()??;
        // From here `Drop` stops and joins whatever is running, also when the next spawn fails.
        let mut emu = Emu { input_tx, frames, thread: Some(thread), reader: None };
        let stop = reader.stopper();
        let (tx, counted, slot) = (emu.input_tx.clone(), backlog.clone(), emu.frames.clone());
        let read_thread = std::thread::Builder::new()
            .name("s2-pty-read".into())
            .spawn(move || read_pty(reader, &tx, &counted, &slot))?;
        emu.reader = Some((read_thread, backlog, stop));
        Ok(emu)
    }

    /// True once the PTY reached end of file (the child exited). The waker fires when it does.
    pub fn exited(&self) -> bool {
        self.frames.exited()
    }

    /// The program asked for mouse reports (any tracking mode), as of the newest frame
    /// published. Never blocks: see `FrameSlot::mouse_reporting`.
    pub fn mouse_reporting(&self) -> bool {
        self.frames.mouse_reporting()
    }
}

impl Drop for Emu {
    /// Stops and joins both threads. Neither can be stuck: the emulator thread never blocks on
    /// the PTY, and the reader waits only in `poll` or on the backlog, both of which are woken.
    fn drop(&mut self) {
        if let Some((_, backlog, stop)) = &self.reader {
            backlog.stop();
            stop.signal();
        }
        let _ = self.input_tx.send(InputMsg::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Some((thread, ..)) = self.reader.take() {
            let _ = thread.join();
        }
    }
}

/// PTY bytes read but not yet fed to the terminal. The reader waits while more than
/// `MAX_QUEUED` are queued, so a flood cannot build a backlog that later output (a key's echo)
/// has to wait behind.
#[derive(Default)]
struct Backlog {
    state: Mutex<BacklogState>,
    changed: Condvar,
}

#[derive(Default)]
struct BacklogState {
    queued: usize,
    stopped: bool,
}

impl Backlog {
    /// Waits for room. False when the reader must stop instead.
    fn wait_for_room(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        while state.queued > MAX_QUEUED && !state.stopped {
            state = self.changed.wait(state).unwrap();
        }
        !state.stopped
    }

    fn add(&self, n: usize) {
        self.state.lock().unwrap().queued += n;
    }

    fn remove(&self, n: usize) {
        let mut state = self.state.lock().unwrap();
        let was_full = state.queued > MAX_QUEUED;
        state.queued = state.queued.saturating_sub(n);
        if was_full && state.queued <= MAX_QUEUED {
            self.changed.notify_all();
        }
    }

    fn stop(&self) {
        self.state.lock().unwrap().stopped = true;
        self.changed.notify_all();
    }

    fn stopped(&self) -> bool {
        self.state.lock().unwrap().stopped
    }
}

/// The reader thread: forwards PTY output until the child closes the PTY, the emulator thread
/// is gone, or `Emu::drop` stops it.
fn read_pty(mut reader: PtyReader, tx: &Sender<InputMsg>, backlog: &Backlog, frames: &FrameSlot) {
    let mut buf = vec![0u8; 64 * 1024];
    while backlog.wait_for_room() {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let t_output_ns = now_ns();
                backlog.add(n);
                if tx.send(InputMsg::Bytes(buf[..n].to_vec(), t_output_ns)).is_err() {
                    return;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    // A stopped reader also reads 0 bytes; only the child closing the PTY is an exit.
    if !backlog.stopped() {
        frames.mark_exited();
    }
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
    mouse_encoder: keys::MouseEncoder,
    pty: Pty,
    writer: PtyWriter,
    log: Option<Log>,
    frames: FrameSlot,
    backlog: Arc<Backlog>,
    /// The viewport as last read; rows the render state reports clean are reused.
    cells: Vec<Cell>,
    size: (u16, u16),
    seq: u64,
    /// Keys the PTY has accepted and no frame carries yet: (key id, when its PTY write began).
    /// Filled by the write's completion, which runs on the writer thread for a queued write.
    unseen_keys: Arc<Mutex<Vec<(u64, u128)>>>,
    repeat_ok: bool,
    /// The viewport may be away from the live screen (a `Scroll` since the last return).
    scrolled: bool,
    /// This batch changed what the viewport shows without PTY output (resize, scroll).
    redraw: bool,
    /// Reads the PTY output beside the terminal, for clipboard writes (OSC 52).
    osc52: osc52::Scanner,
}

impl State {
    fn new(
        pty: Pty,
        cols: u16,
        rows: u16,
        log: Option<Log>,
        frames: FrameSlot,
        backlog: Arc<Backlog>,
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
            mouse_encoder: keys::MouseEncoder::new()?,
            pty,
            writer,
            log,
            frames,
            backlog,
            cells: Vec::new(),
            size: (0, 0),
            seq: 0,
            unseen_keys: Arc::default(),
            repeat_ok: true,
            scrolled: false,
            redraw: false,
            osc52: osc52::Scanner::default(),
        })
    }

    fn run(&mut self, rx: Receiver<InputMsg>) {
        self.publish(now_ns(), false);
        while let Ok(first) = rx.recv() {
            let mut batch = vec![first];
            batch.extend(rx.try_iter());
            // Input is handled before this batch's output so a flood never delays a key.
            let mut output = Vec::new();
            self.redraw = false;
            for msg in drop_stale_repeats(batch, &mut self.repeat_ok) {
                match msg {
                    InputMsg::Bytes(bytes, t_output_ns) => output.push((bytes, t_output_ns)),
                    InputMsg::Key(ev, id) => self.key(&ev, id),
                    InputMsg::Paste(text) => self.paste(&text),
                    InputMsg::Focus(focused) => self.focus(focused),
                    InputMsg::Resize(cols, rows) => self.redraw |= self.resize(cols, rows),
                    InputMsg::Scroll(rows) => self.scroll(ScrollViewport::Delta(rows as isize)),
                    InputMsg::ScrollToBottom => self.scroll(ScrollViewport::Bottom),
                    InputMsg::Mouse { kind, button, col, row, mods } => self.mouse(kind, button, (col, row), mods),
                    InputMsg::Shutdown => return,
                }
            }
            let mut t_output_ns = None;
            for (bytes, t) in output {
                // In pieces, so `Emu::drop` never waits for a whole backlog to be parsed (a
                // debug build of libghostty-vt takes seconds for the 256 KiB).
                for piece in bytes.chunks(PARSE_PIECE) {
                    if self.backlog.stopped() {
                        return;
                    }
                    self.term.vt_write(piece);
                    let mut copied = Vec::new();
                    self.osc52.feed(piece, &mut copied);
                    for text in copied {
                        self.frames.push_copied(text);
                    }
                }
                self.backlog.remove(bytes.len());
                t_output_ns = Some(t);
            }
            match t_output_ns {
                Some(t) => self.publish(t, true),
                // Nothing was read from the PTY, so this frame cannot show any key's echo.
                None if self.redraw => self.publish(now_ns(), false),
                None => {}
            }
        }
    }

    /// Queues `bytes` for the PTY; the emulator thread never waits for the child to read.
    fn send(&mut self, bytes: Vec<u8>, written: Option<crate::pty::Written>) {
        if bytes.is_empty() {
            return;
        }
        // Typing returns to the live screen, as terminals do.
        if self.scrolled {
            self.scroll(ScrollViewport::Bottom);
        }
        let _ = self.writer.write_then(bytes, written);
    }

    fn key(&mut self, ev: &KeyEvent, id: u64) {
        let Ok(bytes) = self.encoder.encode(ev, &self.term) else { return };
        let (log, unseen_keys) = (self.log.clone(), self.unseen_keys.clone());
        // The echo can be read (and stamped) by the reader thread before the write returns, so
        // attribution compares against the time the write began.
        let written = move |t_write_began_ns, t_write_ns| {
            if let Some(log) = &log {
                log.write(id, t_write_ns);
            }
            unseen_keys.lock().unwrap().push((id, t_write_began_ns));
        };
        self.send(bytes, Some(Box::new(written)));
    }

    fn paste(&mut self, text: &str) {
        let bracketed = self.term.mode(Mode::BRACKETED_PASTE).unwrap_or(false);
        if let Ok(bytes) = keys::paste_bytes(text, bracketed) {
            self.send(bytes, None);
        }
    }

    fn mouse(&mut self, kind: MouseKind, button: Option<MouseButton>, at: (u16, u16), mods: Mods) {
        let grid = (self.term.cols().unwrap_or(0), self.term.rows().unwrap_or(0));
        if let Ok(bytes) = self.mouse_encoder.encode(kind, button, at, mods, grid, &self.term)
            && !bytes.is_empty()
        {
            let _ = self.writer.write_then(bytes, None);
        }
    }

    fn focus(&mut self, focused: bool) {
        if self.term.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
            let report: &[u8] = if focused { b"\x1b[I" } else { b"\x1b[O" };
            let _ = self.writer.write_all(report);
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) -> bool {
        // Cell pixel size only feeds image protocols and size reports; S2 draws neither.
        self.term.resize(cols, rows, 8, 16).is_ok() && self.pty.resize(cols, rows).is_ok()
    }

    fn scroll(&mut self, to: ScrollViewport) {
        self.term.scroll_viewport(to);
        self.scrolled = to != ScrollViewport::Bottom;
        self.redraw = true;
    }

    /// Publishes the viewport. `has_output` says PTY output read at `t_output_ns` is in it; only
    /// such a frame can show a key's echo, so only such a frame takes keys.
    fn publish(&mut self, t_output_ns: u128, has_output: bool) {
        let (cursor, dirty_rows) = match self.read_viewport() {
            Ok(read) => read,
            Err(e) => return eprintln!("s2-core: reading the render state failed: {e:#}"),
        };
        // A key belongs to the first frame whose output was read at or after its write began.
        let mut keys = Vec::new();
        if has_output {
            self.unseen_keys.lock().unwrap().retain(|&(id, t_write_began_ns)| {
                let seen = t_write_began_ns <= t_output_ns;
                if seen {
                    keys.push(id);
                }
                !seen
            });
        }
        self.seq += 1;
        self.frames.set_mouse_reporting(self.term.is_mouse_tracking().unwrap_or(false));
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
