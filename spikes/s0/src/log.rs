//! JSONL records shared by the sender and receiver sides.
use serde::{Deserialize, Serialize};
use std::io::Write;

pub fn now_ns() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecvRecord { pub seq: u32, pub intact: bool, pub got_hex: String, pub wall_ns: u128, pub idx: u64 }

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SendRecord {
    pub seq: u32, pub kind: crate::event::Kind,
    pub write_start_ns: u128, pub write_end_ns: u128, pub ack_ns: Option<u128>,
}

pub struct Jsonl(std::io::BufWriter<std::fs::File>);
impl Jsonl {
    pub fn create(path: &std::path::Path) -> anyhow::Result<Self> {
        if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
        Ok(Self(std::io::BufWriter::new(std::fs::File::create(path)?)))
    }
    pub fn write<T: Serialize>(&mut self, rec: &T) -> anyhow::Result<()> {
        serde_json::to_writer(&mut self.0, rec)?; self.0.write_all(b"\n")?; Ok(())
    }
    pub fn flush(&mut self) -> anyhow::Result<()> { Ok(self.0.flush()?) }
}

pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
