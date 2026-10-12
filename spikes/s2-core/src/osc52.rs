//! Clipboard writes from the program: OSC 52 (`ESC ] 52 ; Pc ; Pd BEL` or `… ESC \`, `Pd` the
//! text in base64). tmux sends one when a mouse selection ends (`set-clipboard external`), and
//! so do programs that copy for the user. libghostty-vt's terminal has no callback for it, so
//! the PTY output is scanned beside the parser: `Scanner` is fed the same bytes, in the same
//! order, and returns the texts written.
//!
//! Only a write is answered. A query (`Pd` = `?`) is ignored: the clipboard is never read out
//! to a program. An empty or malformed `Pd` writes nothing.

/// A longer `Pd` is dropped whole: the clipboard is not a file transfer.
pub const MAX_ENCODED: usize = 8 * 1024 * 1024;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const PREFIX: &[u8] = b"52;";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
    Ground,
    /// After ESC outside a string.
    Esc,
    /// In an OSC, its first bytes still match `52;`.
    Prefix(usize),
    /// In an OSC 52: the bytes go to `body`.
    Body,
    /// In another OSC, or one too long: until its end.
    Skip,
    /// After ESC inside an OSC (`keep`: it was an OSC 52 within the limit).
    StringEsc { keep: bool },
}

/// Finds OSC 52 writes in PTY output, across any split of the bytes.
#[derive(Debug)]
pub struct Scanner {
    at: At,
    /// `Pc ; Pd` of the OSC 52 being read.
    body: Vec<u8>,
}

impl Default for Scanner {
    fn default() -> Scanner {
        Scanner { at: At::Ground, body: Vec::new() }
    }
}

impl Scanner {
    /// Reads `bytes`; every clipboard text whose sequence ends in them is pushed to `out`.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<String>) {
        for &byte in bytes {
            self.at = match (self.at, byte) {
                (At::Ground, ESC) => At::Esc,
                (At::Ground, _) => At::Ground,
                (At::Esc, b']') => At::Prefix(0),
                (At::Esc, ESC) => At::Esc,
                (At::Esc, _) => At::Ground,
                // The end of the string: BEL, or ESC \ (seen in `StringEsc`).
                (At::Body, BEL) => self.end(true, out),
                (At::Prefix(_) | At::Skip, BEL) => self.end(false, out),
                (At::Prefix(_) | At::Body | At::Skip, CAN | SUB) => self.end(false, out),
                (At::Body, ESC) => At::StringEsc { keep: true },
                (At::Prefix(_) | At::Skip, ESC) => At::StringEsc { keep: false },
                (At::StringEsc { keep }, b'\\') => self.end(keep, out),
                // ESC and anything else ends the string unfinished; `]` starts the next one.
                (At::StringEsc { .. }, b']') => {
                    self.body.clear();
                    At::Prefix(0)
                }
                (At::StringEsc { .. }, ESC) => {
                    self.body.clear();
                    At::Esc
                }
                (At::StringEsc { .. }, _) => self.end(false, out),
                (At::Prefix(n), byte) if byte == PREFIX[n] => {
                    if n + 1 == PREFIX.len() {
                        At::Body
                    } else {
                        At::Prefix(n + 1)
                    }
                }
                (At::Prefix(_), _) => At::Skip,
                (At::Body, byte) => {
                    if self.body.len() < MAX_ENCODED {
                        self.body.push(byte);
                        At::Body
                    } else {
                        self.body.clear();
                        At::Skip
                    }
                }
                (At::Skip, _) => At::Skip,
            };
        }
    }

    fn end(&mut self, keep: bool, out: &mut Vec<String>) -> At {
        if keep && let Some(text) = written(&self.body) {
            out.push(text);
        }
        self.body.clear();
        At::Ground
    }
}

/// The text an OSC 52 body (`Pc ; Pd`) writes; `None` for a query, an empty or a malformed one.
fn written(body: &[u8]) -> Option<String> {
    let split = body.iter().position(|&b| b == b';')?;
    let encoded = &body[split + 1..];
    if encoded == b"?" {
        return None;
    }
    let bytes = base64(encoded)?;
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Standard base64, padded or not; `None` when a byte is not of the alphabet.
fn base64(encoded: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(encoded.len() / 4 * 3 + 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &byte in encoded {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(pieces: &[&[u8]]) -> Vec<String> {
        let (mut scanner, mut out) = (Scanner::default(), Vec::new());
        for piece in pieces {
            scanner.feed(piece, &mut out);
        }
        out
    }

    #[test]
    fn a_write_ended_by_bel_or_by_st() {
        // "hello" and "wörld"
        assert_eq!(scan(&[b"\x1b]52;c;aGVsbG8=\x07"]), ["hello"]);
        assert_eq!(scan(&[b"\x1b]52;;d8O2cmxk\x1b\\"]), ["wörld"]);
    }

    #[test]
    fn tmux_form_between_other_output() {
        let bytes = b"abc\x1b[1mdef\x1b]2;title\x07\x1b]52;;aGk=\x07tail\x1b]52;c;eW8\x07";
        assert_eq!(scan(&[bytes]), ["hi", "yo"]);
    }

    #[test]
    fn split_at_every_byte() {
        let bytes = b"x\x1b]52;c;aGVsbG8gd29ybGQ=\x1b\\y";
        for at in 0..=bytes.len() {
            let (a, b) = bytes.split_at(at);
            assert_eq!(scan(&[a, b]), ["hello world"], "split at {at}");
        }
        let single: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(scan(&single), ["hello world"]);
    }

    #[test]
    fn a_query_an_empty_and_a_malformed_write_give_nothing() {
        assert!(scan(&[b"\x1b]52;c;?\x07"]).is_empty());
        assert!(scan(&[b"\x1b]52;c;\x07"]).is_empty());
        assert!(scan(&[b"\x1b]52;c;not base64!\x07"]).is_empty());
        assert!(scan(&[b"\x1b]52\x07"]).is_empty());
        assert!(scan(&[b"\x1b]520;c;aGk=\x07"]).is_empty());
    }

    #[test]
    fn a_cancelled_or_broken_string_writes_nothing_and_the_next_one_does() {
        assert_eq!(scan(&[b"\x1b]52;c;aGVs\x18\x1b]52;c;aGk=\x07"]), ["hi"]);
        assert_eq!(scan(&[b"\x1b]52;c;aGVs\x1b[0m\x1b]52;c;aGk=\x07"]), ["hi"]);
        assert_eq!(scan(&[b"\x1b]52;c;aGVs\x1b]52;c;aGk=\x07"]), ["hi"]);
    }

    #[test]
    fn too_long_is_dropped_whole() {
        let mut bytes = b"\x1b]52;c;".to_vec();
        bytes.extend(std::iter::repeat_n(b'A', MAX_ENCODED + 8));
        bytes.extend_from_slice(b"\x07\x1b]52;c;aGk=\x07");
        assert_eq!(scan(&[&bytes]), ["hi"]);
    }
}
