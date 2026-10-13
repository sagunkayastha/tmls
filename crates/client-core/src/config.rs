//! Readers for the files tmls keeps in `~/.config/tmls` (`hosts`, `name`, `pins`,
//! `host-colors.json`, `agent-presets.json`) and the one writer v0 has: `pins`.
//!
//! Reading never fails: a missing or damaged file is that file's default.
//!
//! **Pins.** The `pins` file is tmls's own: one *slug* per pinned session ([`tmls_slug`], a
//! lossy key ending in a hash, so host and name cannot be read back from it). `Config::pins`
//! holds those slugs as they are; a session pinned in tmls is pinned here and the other way round.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use anyhow::Context;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde_json::Value;

use crate::listing::SessionKey;

#[derive(Clone, Debug)]
pub struct Config {
    /// What this machine is called: the `name` file, else its hostname. Its sessions' host.
    pub local_name: String,
    /// Hosts to list, in order: this machine (when it has tmux), then the remote ssh hosts.
    pub hosts: Vec<String>,
    /// The user's accent colour per host; a host without one gets [`default_color`].
    pub host_colors: BTreeMap<String, HostColor>,
    /// Agent presets for a new session, `(name, argv)`, in the file's order.
    pub presets: Vec<(String, Vec<String>)>,
    /// The pinned sessions' tmls slugs ([`tmls_slug`]): every whitespace-separated token of
    /// the `pins` file, as tmls loads it. Ask with [`Config::is_pinned`].
    pub pins: BTreeSet<String>,
}

/// A host's accent colour, by its name in tmls's palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostColor {
    Blue,
    Teal,
    Violet,
    Green,
    Amber,
    Rose,
}

const PALETTE: [(&str, HostColor); 6] = [
    ("blue", HostColor::Blue),
    ("teal", HostColor::Teal),
    ("violet", HostColor::Violet),
    ("green", HostColor::Green),
    ("amber", HostColor::Amber),
    ("rose", HostColor::Rose),
];

/// The colour of a host the user gave none: stable across restarts and the same one tmls picks
/// (`host_colors.py`: the first four bytes of sha256(host), big-endian, modulo the palette).
pub fn default_color(host: &str) -> HostColor {
    let digest = sha256(host.as_bytes());
    let index = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) as usize % PALETTE.len();
    PALETTE[index].1
}

impl Config {
    /// Reads `dir` (`~/.config/tmls`). `cli_hosts`, when not empty, replace the `hosts` file.
    pub fn load(dir: &Path, cli_hosts: &[String]) -> Config {
        let read = |file: &str| std::fs::read_to_string(dir.join(file)).unwrap_or_default();
        let local_name = local_name(dir);
        let machine = hostname();
        let named: Vec<String> = if cli_hosts.is_empty() {
            // one ssh host per line; blank lines and # comments are skipped
            read("hosts").lines().map(|line| line.split('#').next().unwrap_or("").trim().to_string()).collect()
        } else {
            cli_hosts.iter().map(|host| host.trim().to_string()).collect()
        };
        let mut hosts: Vec<String> = Vec::new();
        if has_tmux() {
            hosts.push(local_name.clone());
        }
        for host in named {
            // "-…" would reach ssh as an option; this machine is listed without ssh, once
            let usable = !host.is_empty() && !host.starts_with('-') && host != machine && host != local_name;
            if usable && !hosts.contains(&host) {
                hosts.push(host);
            }
        }

        let host_colors = json_object(&read("host-colors.json"))
            .into_iter()
            .filter_map(|(host, color)| {
                let color = PALETTE.iter().find(|(name, _)| color.as_str() == Some(name))?.1;
                Some((host, color)).filter(|(host, _)| !host.is_empty())
            })
            .collect();
        let presets = json_object(&read("agent-presets.json"))
            .into_iter()
            .filter_map(|(name, argv)| {
                let argv: Vec<String> = argv
                    .as_array()?
                    .iter()
                    .map(|arg| arg.as_str().filter(|a| !a.is_empty()).map(str::to_string))
                    .collect::<Option<_>>()?;
                Some((name, argv)).filter(|(name, argv)| !name.is_empty() && !argv.is_empty())
            })
            .collect();
        let pins = read("pins").split_whitespace().map(str::to_string).collect();
        Config { local_name, hosts, host_colors, presets, pins }
    }

    /// Whether the session is pinned, here or in tmls.
    pub fn is_pinned(&self, key: &SessionKey) -> bool {
        self.pins.contains(&tmls_slug(key, &self.local_name))
    }

    /// The sketchpad hub's address: the first line of `dir/sketchpad` that is not blank or a
    /// `#` comment (tmls `read_config`). `None` without the file: then there is no Sketch.
    pub fn sketchpad(dir: &Path) -> Option<String> {
        let text = std::fs::read_to_string(dir.join("sketchpad")).ok()?;
        text.lines()
            .map(|line| line.split('#').next().unwrap_or("").trim())
            .find(|line| !line.is_empty() && !line.starts_with('-'))
            .map(str::to_string)
    }

    /// Writes `dir/pins` as tmls does: the whole set, one slug per line, sorted. Creates `dir`
    /// if needed.
    ///
    /// The file is tmls's too, so it is never cut short in place ([`replace_file`]).
    pub fn save_pins(dir: &Path, pins: &BTreeSet<String>) -> anyhow::Result<()> {
        let path = dir.join("pins");
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let text: String = pins.iter().map(|slug| format!("{slug}\n")).collect();
        replace_file(&path, &text).with_context(|| format!("writing {}", path.display()))
    }
}

/// Makes `text` the whole of the file at `path`, a file tmls reads and writes too: the text
/// goes to a temp file beside it (`.NAME.tmp-PID`), which is then renamed over it — a reader
/// sees the old text or the new one, and a write that fails leaves the old one and no temp
/// file. The new file is on disk before the rename and keeps the old one's mode; a `path` that
/// is a symlink keeps being one (its target is replaced). The directory has to be there.
pub fn replace_file(path: &Path, text: &str) -> std::io::Result<()> {
    // where the bytes really live: the file itself, or what the symlink points at
    let target = if path.is_symlink() { std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()) } else { path.to_path_buf() };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = target.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let written = (|| {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        if let Ok(old) = std::fs::metadata(&target) {
            file.set_permissions(old.permissions())?;
        }
        file.sync_all()?;
        std::fs::rename(&temp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// tmls's key for a session in its `pins` file (`app.py: slug`): a readable part that can
/// collide ("my work", "my_work") plus a short hash of the exact host and name. tmls calls this
/// machine "local".
pub fn tmls_slug(key: &SessionKey, local_name: &str) -> String {
    let host = if key.host == local_name { "local" } else { &key.host };
    let digest = sha1(format!("{host}\0{}", key.name).as_bytes());
    let readable: String = format!("{host}-{}", key.name)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let hash: String = digest[..4].iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{readable}-{hash}")
}

fn local_name(dir: &Path) -> String {
    let name = std::fs::read_to_string(dir.join("name")).unwrap_or_default().trim().to_string();
    if name.is_empty() { hostname() } else { name }
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most `len` bytes into the buffer it is given.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len() - 1) } == 0;
    let len = buf.iter().position(|&b| b == 0).unwrap_or(0);
    let name = String::from_utf8_lossy(&buf[..len]).into_owned();
    if ok && !name.is_empty() { name } else { "localhost".to_string() }
}

/// Whether this machine can have tmux sessions at all (tmls: `shutil.which("tmux")`).
fn has_tmux() -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("tmux").is_file()))
}

/// The entries of a JSON object in the file's order; nothing when the text is anything else.
fn json_object(text: &str) -> Vec<(String, Value)> {
    struct Entries;
    impl<'de> Visitor<'de> for Entries {
        type Value = Vec<(String, Value)>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a JSON object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = Vec::new();
            while let Some(entry) = map.next_entry()? {
                entries.push(entry);
            }
            Ok(entries)
        }
    }
    let mut json = serde_json::Deserializer::from_str(text);
    let entries = json.deserialize_map(Entries);
    match (entries, json.end()) {
        (Ok(entries), Ok(())) => entries,
        _ => Vec::new(),
    }
}

/// Splits a message into the 64-byte blocks SHA-1 and SHA-256 hash, padding included.
fn padded_blocks(message: &[u8]) -> Vec<[u8; 64]> {
    let mut data = message.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&(message.len() as u64 * 8).to_be_bytes());
    data.chunks_exact(64).map(|block| block.try_into().expect("64-byte chunk")).collect()
}

fn be_word(block: &[u8; 64], i: usize) -> u32 {
    u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]])
}

/// SHA-256 (FIPS 180-4), for [`default_color`]: tmls's rule, without a new dependency.
fn sha256(message: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    for block in padded_blocks(message) {
        let mut w = [0u32; 64];
        for i in 0..64 {
            w[i] = if i < 16 {
                be_word(&block, i)
            } else {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1)
            };
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (bytes, word) in out.chunks_exact_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// SHA-1, for [`tmls_slug`]: tmls's pin key, without a new dependency. Not used for security.
fn sha1(message: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
    for block in padded_blocks(message) {
        let mut w = [0u32; 80];
        for i in 0..80 {
            w[i] = if i < 16 { be_word(&block, i) } else { (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1) };
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..20 => ((b & c) | (!b & d), 0x5a827999),
                20..40 => (b ^ c ^ d, 0x6ed9eba1),
                40..60 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6u32),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*word);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *state = state.wrapping_add(value);
        }
    }
    let mut out = [0u8; 20];
    for (bytes, word) in out.chunks_exact_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hashes_match_python_hashlib() {
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha256(&[b'a'; 200])), "c2a908d98f5df987ade41b5fce213067efbcc21ef2240212a41e54b5e7c28ae5");
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha1(&[b'a'; 200])), "e61cfffe0d9195a525fc6cf06ca2d77119c24a40");
    }

    #[test]
    fn slug_is_tmls_slug() {
        let key = |host: &str, name: &str| SessionKey { host: host.into(), name: name.into() };
        assert_eq!(tmls_slug(&key("laptop", "Agent Thing"), "laptop"), "local-Agent_Thing-0ffb10f9");
        assert_eq!(tmls_slug(&key("laanta-chy", "café: x"), "laptop"), "laanta-chy-caf___x-404715a3");
    }
}
