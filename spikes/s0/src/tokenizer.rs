//! Splits the raw byte stream at `#dddddd` markers. Keeps a partial tail (an unfinished marker
//! or escape) between reads.
use crate::event::expected_payload;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival { pub seq: u32, pub intact: bool, pub got: Vec<u8> }

#[derive(Default)]
pub struct Tokenizer { buf: Vec<u8> }

impl Tokenizer {
    pub fn new() -> Self { Self::default() }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Arrival> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            // Find a complete marker: '#' followed by six ASCII digits.
            let Some(h) = self.buf.iter().position(|&b| b == b'#') else { break };
            if self.buf.len() < h + 7 { break; }
            let digits = &self.buf[h + 1..h + 7];
            if !digits.iter().all(u8::is_ascii_digit) {
                // Not a marker: a '#' inside payload (excluded by the alphabet) — skip it.
                self.buf.drain(..=h);
                continue;
            }
            let seq: u32 = std::str::from_utf8(digits).unwrap().parse().unwrap();
            let got = self.buf[..h].to_vec();
            self.buf.drain(..h + 7);
            let intact = got == expected_payload(seq);
            out.push(Arrival { seq, intact, got });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{encode, expected_payload};
    fn whole(seq: u32) -> Vec<u8> { encode(seq).concat() }
    #[test]
    fn single_event_intact() {
        let mut t = Tokenizer::new();
        let a = t.feed(&whole(0));
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].seq, a[0].intact), (0, true));
    }
    #[test]
    fn alt_split_across_reads() {
        let mut t = Tokenizer::new();
        let w = whole(2); // ESC + letter + marker
        assert!(t.feed(&w[..1]).is_empty());
        let a = t.feed(&w[1..]);
        assert_eq!((a[0].seq, a[0].intact), (2, true));
    }
    #[test]
    fn marker_split_mid_digits() {
        let mut t = Tokenizer::new();
        let w = whole(13);
        let cut = w.len() - 3;
        assert!(t.feed(&w[..cut]).is_empty());
        let a = t.feed(&w[cut..]);
        assert_eq!((a[0].seq, a[0].intact), (13, true));
    }
    #[test]
    fn corrupt_payload_is_reported_with_bytes() {
        let mut t = Tokenizer::new();
        let mut w = whole(2);
        w.remove(0); // ESC lost: Alt+x became x
        let a = t.feed(&w);
        assert_eq!(a[0].seq, 2);
        assert!(!a[0].intact);
        assert_eq!(a[0].got, expected_payload(2)[1..].to_vec());
    }
    #[test]
    fn many_events_one_read() {
        let mut t = Tokenizer::new();
        let mut buf = Vec::new();
        for s in 0..64 { buf.extend(whole(s)); }
        let a = t.feed(&buf);
        assert_eq!(a.iter().map(|x| x.seq).collect::<Vec<_>>(), (0..64).collect::<Vec<_>>());
        assert!(a.iter().all(|x| x.intact));
    }
}
