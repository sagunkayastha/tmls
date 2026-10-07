//! Finds `A<6 digits>` acks and `S0READY` / `S0DONE` markers in PTY output after stripping escapes.
//! State machines so sequences split across reads are handled.

/// Escape stripper: drops CSI (`ESC [ … final`), OSC (`ESC ] … BEL|ST`) and bare `ESC x`.
/// Keeps state across `feed` calls.
#[derive(Default)]
pub struct Stripper { esc: Esc }
#[derive(Default, Clone, Copy, PartialEq)]
enum Esc { #[default] None, Esc, Csi, Osc, OscEsc }

impl Stripper {
    pub fn new() -> Self { Self::default() }
    /// Append the plain (non-escape) bytes of `bytes` to `out`.
    pub fn feed_into(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        for &b in bytes {
            self.esc = match self.esc {
                Esc::None => if b == 0x1b { Esc::Esc } else { out.push(b); Esc::None },
                Esc::Esc => match b { b'[' => Esc::Csi, b']' => Esc::Osc, _ => Esc::None },
                Esc::Csi => if (0x40..=0x7e).contains(&b) { Esc::None } else { Esc::Csi },
                Esc::Osc => match b { 0x07 => Esc::None, 0x1b => Esc::OscEsc, _ => Esc::Osc },
                Esc::OscEsc => Esc::None, // ESC \ (ST)
            };
        }
    }
}

/// Longest pattern we must find across a chunk boundary (`A000000` and `S0READY` are 7 bytes).
const TAIL: usize = 7;

#[derive(Default)]
pub struct AckScanner { tail: Vec<u8>, strip: Stripper, pub saw_done: bool, pub saw_ready: bool }

impl AckScanner {
    pub fn new() -> Self { Self::default() }
    /// Feed raw PTY bytes; returns the ack seqs completed by this chunk, each reported once.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u32> {
        let mut plain = std::mem::take(&mut self.tail);
        self.strip.feed_into(bytes, &mut plain);
        let mut out = Vec::new();
        let mut i = 0;
        let mut last_match_end = 0;
        while i + 7 <= plain.len() {
            if plain[i] == b'A' && plain[i + 1..i + 7].iter().all(u8::is_ascii_digit) {
                out.push(std::str::from_utf8(&plain[i + 1..i + 7]).unwrap().parse().unwrap());
                i += 7;
                last_match_end = i;
            } else { i += 1; }
        }
        if plain.windows(6).any(|w| w == b"S0DONE") { self.saw_done = true; }
        if plain.windows(7).any(|w| w == b"S0READY") { self.saw_ready = true; }
        // Keep only unconsumed bytes (never re-scan a matched ack), at most TAIL of them.
        let start = last_match_end.max(plain.len().saturating_sub(TAIL));
        self.tail = plain[start..].to_vec();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ack_across_chunks_with_escapes() {
        let mut s = AckScanner::new();
        assert!(s.feed(b"\x1b[2;5HA0000").is_empty());
        assert_eq!(s.feed(b"\x1b[0m42\r\n"), vec![42]);
    }
    #[test]
    fn ack_inside_osc_is_ignored_text_outside_counts() {
        let mut s = AckScanner::new();
        assert_eq!(s.feed(b"\x1b]0;A000001\x07A000002\r\n"), vec![2]);
    }
    #[test]
    fn done_marker() {
        let mut s = AckScanner::new();
        s.feed(b"xx S0DO"); s.feed(b"NE\r\n");
        assert!(s.saw_done);
    }
    #[test]
    fn no_double_report_when_ack_ends_chunk() {
        let mut s = AckScanner::new();
        assert_eq!(s.feed(b"A000001\r"), vec![1]);
        assert_eq!(s.feed(b"\n"), Vec::<u32>::new());
    }
    #[test]
    fn no_double_report_when_ack_ends_exactly_at_chunk_end() {
        let mut s = AckScanner::new();
        assert_eq!(s.feed(b"A000001"), vec![1]);
        assert_eq!(s.feed(b"A000002"), vec![2]);
        assert_eq!(s.feed(b"\r\n"), Vec::<u32>::new());
    }
    #[test]
    fn ready_marker_split_and_in_a_row() {
        let mut s = AckScanner::new();
        s.feed(b"\x1b[3;1H| s0 S0RE"); assert!(!s.saw_ready);
        s.feed(b"\x1b[1mADY |"); assert!(s.saw_ready);
    }
}
