//! S2 shared core — Task 0 stub of the contract in
//! `docs/superpowers/plans/2026-10-08-s2-split-overview.md`. Track A replaces the bodies.
#![allow(unused_variables)]

use std::sync::mpsc::Sender;

// pty
pub struct Pty;
/// Clone + Send; the ONLY path that writes to the PTY.
#[derive(Clone)]
pub struct PtyWriter;

impl Pty {
    pub fn spawn(cmd: &[String], cols: u16, rows: u16) -> anyhow::Result<Pty> {
        todo!()
    }
    pub fn writer(&self) -> PtyWriter {
        todo!()
    }
    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        todo!()
    }
}

// emulator thread (libghostty-vt objects are not Send, so they live on this thread only)
pub struct Emu {
    pub input_tx: Sender<InputMsg>,
    pub frames: FrameSlot,
}

pub enum InputMsg {
    /// PTY output and `t_output_ns`, taken right after `read()` returned.
    Bytes(Vec<u8>, u128),
    Resize(u16, u16),
    /// Key event and its key id.
    Key(KeyEvent, u64),
    Paste(String),
    Shutdown,
}

impl Emu {
    pub fn start(pty: Pty, cols: u16, rows: u16, log: Option<latency::Log>) -> anyhow::Result<Emu> {
        todo!()
    }
}

/// Latest-wins single slot. Frames may drop; PTY bytes never.
pub struct FrameSlot;

impl FrameSlot {
    pub fn take(&self) -> Option<Frame> {
        todo!()
    }
    pub fn set_waker(&self, f: Box<dyn Fn() + Send + Sync>) {
        todo!()
    }
}

pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub cursor: Option<Cursor>,
    pub dirty_rows: Vec<bool>,
    pub seq: u64,
    pub t_output_ns: u128,
    pub keys: Vec<u64>,
}

pub struct Cell {
    pub text: String,
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub flags: CellFlags,
    pub wide: Wide,
    pub hyperlink: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CellFlags {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub strike: bool,
    pub faint: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wide {
    Narrow,
    Lead,
    Spacer,
}

pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub shape: CursorShape,
    pub visible: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

// keys (toolkit-neutral; front-ends map winit/gpui events into this)
pub struct KeyEvent {
    pub key: Key,
    pub mods: Mods,
    pub text: Option<String>,
    pub repeat: bool,
    pub pressed: bool,
}

pub enum Key {
    Char(char),
    Named(Named),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Named {
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

/// CLOCK_MONOTONIC; every timestamp in every log uses this.
pub fn now_ns() -> u128 {
    todo!()
}

pub mod latency {
    /// Append-only `<dir>/latency.jsonl`; schema in the split overview.
    pub struct Log;

    impl Log {
        pub fn frame(&self, seq: u64, t_submit_ns: u128, t_present_ns: u128, t_present_kind: &str) {
            todo!()
        }
    }
}
