//! One terminal session: the emulator thread, key ids and `key` lines, and the state drawn
//! over the cells (preedit, selection, photon square). Everything the app does except the
//! window and the GPU, so it runs in tests against a real PTY.
//!
//! Latency contract as used here:
//! - `key(t_key_ns, ev)` writes the `key` line (presses and repeats) and then sends
//!   `InputMsg::Key` — the core writes the `write` line.
//! - `take_frames` takes every frame from the slot; the core's log remembers the keys of each
//!   taken frame until a `frame` line with that seq or a later one is written (`marks`), so a
//!   frame that is taken but not presented loses no key.
//! - The photon square flips once per presented frame that carries at least one key press
//!   (`Frame::keys`; press or repeat, never a release), i.e. in the same rendered frame as
//!   the key's echo. See `Photon`.

use s2_core::latency::Log;
use s2_core::{Emu, Frame, InputMsg, KeyEvent, Pty};

use crate::gfx::atlas::Atlas;
use crate::gfx::grid::{Overlays, Preedit, Scene, Theme};
use crate::input;
use crate::select::{CellPos, Selection};

/// The `--photon` square. Contract rule: **one flip per presented frame that carries at least
/// one key press** (press or repeat; releases never count). So the square changes colour on
/// every frame that shows a key's echo, however many keys that frame carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Photon {
    /// White (`true`) or black.
    on: bool,
    /// Ids of the presses and repeats sent to the core that no frame has carried yet. A
    /// release takes an id too and can be on a frame (kitty protocol), but is never in here.
    presses: Vec<u64>,
    /// The square has flipped for frames taken since the last present. Frames taken before a
    /// present that did not happen (surface not ready) are shown by the next one, together
    /// with whatever comes in between: that is one presented frame, so one flip.
    flipped: bool,
}

impl Photon {
    pub fn on(&self) -> bool {
        self.on
    }

    /// A press or repeat with this key id was sent to the core.
    pub fn press(&mut self, id: u64) {
        self.presses.push(id);
    }

    /// A frame carrying these key ids was taken; it is on screen after the next present.
    pub fn frame(&mut self, keys: &[u64]) {
        let Some(&newest) = keys.iter().max() else {
            return;
        };
        let press = keys.iter().any(|id| self.presses.contains(id));
        // The core hands keys to frames in the order they were written, so nothing at or
        // below the newest id on this frame is still to come (a press that sent no bytes is
        // on no frame at all).
        self.presses.retain(|&id| id > newest);
        if press && !self.flipped {
            self.on = !self.on;
            self.flipped = true;
        }
    }

    /// The frames taken so far have been presented.
    pub fn presented(&mut self) {
        self.flipped = false;
    }
}

pub struct Session {
    emu: Emu,
    log: Option<Log>,
    next_key: u64,
    /// The newest frame taken; what is on screen after the next render.
    frame: Option<Frame>,
    grid: (u16, u16),
    preedit: Option<String>,
    selection: Option<Selection>,
    /// The mouse button is down and the pointer extends `selection`.
    dragging: bool,
    /// `--photon`: the square, `None` without the flag.
    photon: Option<Photon>,
    /// When `begin_exit` sent its marker.
    exit_asked_ns: Option<u128>,
}

impl Session {
    /// Starts the emulator on `pty`. `photon`: draw the `--photon` square (black at first).
    pub fn start(
        pty: Pty,
        cols: u16,
        rows: u16,
        log: Option<Log>,
        photon: bool,
    ) -> anyhow::Result<Session> {
        Ok(Session {
            emu: Emu::start(pty, cols, rows, log.clone())?,
            log,
            next_key: 1,
            frame: None,
            grid: (cols, rows),
            preedit: None,
            selection: None,
            dragging: false,
            photon: photon.then(Photon::default),
            exit_asked_ns: None,
        })
    }

    /// Called (on any thread, possibly spuriously) when a frame is waiting or the child exited.
    pub fn set_waker(&self, f: Box<dyn Fn() + Send + Sync>) {
        self.emu.frames.set_waker(f);
    }

    /// The child closed the PTY. Its last output may not be in a frame yet: see `begin_exit`.
    pub fn exited(&self) -> bool {
        self.emu.exited()
    }

    /// Call when `exited()` is first seen. `exited()` is set by the PTY reader thread at end
    /// of file, when the emulator thread may still have the child's last output queued, so the
    /// last frame can arrive after it. This sends a marker through the same queue (a
    /// `ScrollToBottom`, which the emulator always answers with a frame); keep calling
    /// `take_frames` until `output_drained`. Returns `true` on the first call.
    pub fn begin_exit(&mut self) -> bool {
        if self.exit_asked_ns.is_some() {
            return false;
        }
        self.exit_asked_ns = Some(s2_core::now_ns());
        self.send(InputMsg::ScrollToBottom);
        true
    }

    /// After `begin_exit`: the newest frame taken shows everything the child wrote.
    ///
    /// How it is known: a frame built for a batch of input without PTY output carries its build
    /// time, so `t_output_ns >= ` the time the marker was sent means the emulator got to the
    /// marker and found no output left. A frame with an earlier time may or may not include the
    /// last output; `take_frames` then sends the marker again, and since every byte of output
    /// was queued before the first marker, the batch that answers a marker sent after such a
    /// frame was taken has no output in it.
    pub fn output_drained(&self) -> bool {
        match (self.exit_asked_ns, &self.frame) {
            (Some(asked), Some(frame)) => frame.t_output_ns >= asked,
            _ => false,
        }
    }

    fn send(&self, msg: InputMsg) {
        // The emulator thread is gone only while the app shuts down.
        let _ = self.emu.input_tx.send(msg);
    }

    /// One key event, `t_key_ns` taken at `window_event` entry. Returns its key id. Every
    /// event gets an id; a `key` line is written for presses and repeats only (a release sends
    /// nothing in the legacy protocol and is not a latency sample).
    pub fn key(&mut self, t_key_ns: u128, ev: KeyEvent) -> u64 {
        let id = self.next_key;
        self.next_key += 1;
        if ev.pressed {
            if let Some(log) = &self.log {
                log.key(id, t_key_ns);
            }
            if let Some(photon) = &mut self.photon {
                photon.press(id);
            }
        }
        self.send(InputMsg::Key(ev, id));
        id
    }

    /// `Ime::Commit`: ends the composition and sends the text as one key event.
    pub fn ime_commit(&mut self, t_key_ns: u128, text: &str) -> Option<u64> {
        self.preedit = None;
        input::ime_commit(text).map(|ev| self.key(t_key_ns, ev))
    }

    /// `Ime::Preedit`: the text being composed (empty = none). Returns whether it changed.
    pub fn set_preedit(&mut self, text: &str) -> bool {
        let new = (!text.is_empty()).then(|| text.to_string());
        let changed = new != self.preedit;
        self.preedit = new;
        changed
    }

    pub fn paste(&mut self, text: &str) {
        if !text.is_empty() {
            self.send(InputMsg::Paste(text.to_string()));
        }
    }

    /// Window focus changed. `false` is the contract's focus-lost marker: the core drops key
    /// repeats until the next fresh press.
    pub fn focus(&mut self, focused: bool) {
        self.send(InputMsg::Focus(focused));
    }

    /// Moves the viewport (`input::wheel_rows`). Returning to the bottom on a key press is the
    /// core's job. The selection is in viewport cells, so it ends here.
    pub fn scroll(&mut self, rows: i32) {
        if rows != 0 {
            self.clear_selection();
            self.send(InputMsg::Scroll(rows));
        }
    }

    /// The grid size the window has room for. Returns whether a resize was sent.
    pub fn resize(&mut self, cols: u16, rows: u16) -> bool {
        if (cols, rows) == self.grid {
            return false;
        }
        self.grid = (cols, rows);
        self.clear_selection();
        self.send(InputMsg::Resize(cols, rows));
        true
    }

    pub fn grid(&self) -> (u16, u16) {
        self.grid
    }

    /// Takes every waiting frame and feeds each to the scene (a taken frame that skipped
    /// `update_cells` would lose its dirty rows). Returns whether there was a new frame.
    pub fn take_frames(&mut self, scene: &mut Scene, atlas: &mut Atlas, theme: &Theme) -> bool {
        let mut any = false;
        while let Some(frame) = self.emu.frames.take() {
            scene.update_cells(&frame, atlas, theme);
            if let Some(photon) = &mut self.photon {
                photon.frame(&frame.keys);
            }
            if self.selection_is_stale(&frame) {
                self.clear_selection();
            }
            self.frame = Some(frame);
            any = true;
        }
        if any && self.exit_asked_ns.is_some() && !self.output_drained() {
            self.send(InputMsg::ScrollToBottom);
        }
        any
    }

    /// The selection highlights cells of the frame it was made on: it ends when the grid
    /// changes size or the content of one of its rows changes. (A dirty row is only a row that
    /// may have changed — the cursor leaving it marks it too — so the cells are compared.)
    fn selection_is_stale(&self, new: &Frame) -> bool {
        let Some((first, last)) = self.selection.as_ref().and_then(Selection::rows) else {
            return false;
        };
        let Some(old) = &self.frame else {
            return true;
        };
        if (old.cols, old.rows) != (new.cols, new.rows) {
            return true;
        }
        let cols = new.cols as usize;
        (first..=last).any(|row| {
            let cells = row as usize * cols..(row as usize + 1) * cols;
            new.dirty_rows.get(row as usize).copied().unwrap_or(true)
                && old.cells.get(cells.clone()) != new.cells.get(cells)
        })
    }

    /// Call after each present: the frames taken so far are on screen (`Photon::presented`).
    pub fn presented(&mut self) {
        if let Some(photon) = &mut self.photon {
            photon.presented();
        }
    }

    /// The newest frame taken.
    pub fn frame(&self) -> Option<&Frame> {
        self.frame.as_ref()
    }

    /// What `Scene::update_overlays` draws over the cells.
    pub fn overlays(&self) -> Overlays {
        let selection = match (&self.selection, &self.frame) {
            (Some(selection), Some(frame)) => selection.spans(frame),
            _ => Vec::new(),
        };
        Overlays {
            preedit: self.preedit.clone().map(|text| Preedit { text }),
            photon: self.photon.as_ref().map(Photon::on),
            selection,
        }
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.dragging = false;
    }

    /// Left button down on a cell (`Scene::cell_at`; `None` = outside the grid). Any earlier
    /// selection ends. Returns whether something has to be redrawn.
    pub fn select_start(&mut self, at: Option<CellPos>) -> bool {
        let had = self.selection.is_some_and(|s| !s.is_empty());
        self.selection = at.map(Selection::start);
        self.dragging = at.is_some();
        had
    }

    /// Pointer moved (`Scene::nearest_cell`). Returns whether the selection changed.
    pub fn select_drag(&mut self, at: CellPos) -> bool {
        match &mut self.selection {
            Some(selection) if self.dragging => selection.drag_to(at),
            _ => false,
        }
    }

    /// Left button up.
    pub fn select_end(&mut self) {
        self.dragging = false;
    }

    pub fn selecting(&self) -> bool {
        self.dragging
    }

    /// The text Ctrl+Shift+C copies; `None` when nothing (or only blanks) is selected.
    pub fn selected_text(&self) -> Option<String> {
        let text = self.selection?.text(self.frame.as_ref()?);
        (!text.is_empty()).then_some(text)
    }
}
