//! Local PTY (portable-pty). `PtyWriter` is the only path that writes to it (PLAN §5.1).

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: PtyWriter,
}

struct WriterInner {
    out: Box<dyn Write + Send>,
    /// Copy of everything written, kept only after `start_recording` (tests, diagnostics).
    recorded: Option<Vec<u8>>,
}

/// Clone + Send; keys, pastes and emulator replies all go through this one writer.
#[derive(Clone)]
pub struct PtyWriter {
    inner: Arc<Mutex<WriterInner>>,
}

impl PtyWriter {
    /// Writes all of `bytes` (short writes are retried) and flushes.
    pub fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.out.write_all(bytes)?;
        inner.out.flush()?;
        if let Some(rec) = inner.recorded.as_mut() {
            rec.extend_from_slice(bytes);
        }
        Ok(())
    }

    pub fn start_recording(&self) {
        self.inner.lock().unwrap().recorded.get_or_insert_with(Vec::new);
    }

    pub fn recorded(&self) -> Vec<u8> {
        self.inner.lock().unwrap().recorded.clone().unwrap_or_default()
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

impl Pty {
    pub fn spawn(cmd: &[String], cols: u16, rows: u16) -> anyhow::Result<Pty> {
        let (program, args) = cmd.split_first().context("empty command")?;
        let pair = native_pty_system().openpty(size(cols, rows))?;
        let mut builder = CommandBuilder::new(program);
        builder.args(args);
        builder.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(builder)?;
        drop(pair.slave);
        let out = pair.master.take_writer()?;
        let writer = PtyWriter { inner: Arc::new(Mutex::new(WriterInner { out, recorded: None })) };
        Ok(Pty { master: pair.master, child, writer })
    }

    pub fn writer(&self) -> PtyWriter {
        self.writer.clone()
    }

    pub fn reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        self.master.try_clone_reader()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.resize(size(cols, rows))
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
