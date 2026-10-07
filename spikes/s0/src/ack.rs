//! Finds `A<6 digits>` acks and `S0READY` / `S0DONE` markers in PTY output after stripping escapes.
//! State machines so sequences split across reads are handled.

/// Escape stripper: drops CSI (`ESC [ … final`), string sequences OSC/DCS/APC/PM/SOS
/// (`ESC ] P _ ^ X … BEL|ST`), nF escapes (`ESC 0x20-0x2f* final`, e.g. `ESC ( B` from sgr0) and
/// bare `ESC x`. Keeps state across `feed` calls.
#[derive(Default)]
pub struct Stripper { esc: Esc }
#[derive(Default, Clone, Copy, PartialEq)]
enum Esc { #[default] None, Esc, Csi, Nf, Str, StrEsc }

/// State after `ESC b`.
fn after_esc(b: u8) -> Esc {
    match b {
        b'[' => Esc::Csi,
        b']' | b'P' | b'_' | b'^' | b'X' => Esc::Str,
        0x20..=0x2f => Esc::Nf,
        0x1b => Esc::Esc,
        _ => Esc::None,
    }
}

impl Stripper {
    pub fn new() -> Self { Self::default() }
    /// Append the plain (non-escape) bytes of `bytes` to `out`.
    pub fn feed_into(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        for &b in bytes {
            self.esc = match self.esc {
                Esc::None => if b == 0x1b { Esc::Esc } else { out.push(b); Esc::None },
                Esc::Esc => after_esc(b),
                Esc::Csi => if (0x40..=0x7e).contains(&b) { Esc::None } else { Esc::Csi },
                Esc::Nf => if (0x20..=0x2f).contains(&b) { Esc::Nf } else { Esc::None },
                Esc::Str => match b { 0x07 => Esc::None, 0x1b => Esc::StrEsc, _ => Esc::Str },
                // ESC \ is ST; any other ESC x ends the string and starts a new escape.
                Esc::StrEsc => if b == b'\\' { Esc::None } else { after_esc(b) },
            };
        }
    }
}

/// Longest fixed pattern we must find across a chunk boundary (`A000000` and `S0READY` are 7 bytes).
const TAIL: usize = 7;

/// The readiness text the receiver prints: `S0READY`, or `S0READY:<token>` with `--ready-token`.
pub fn ready_text(token: Option<&str>) -> String {
    match token { Some(t) => format!("S0READY:{t}"), None => "S0READY".into() }
}

/// Finds acks and the ready/done markers. With a ready token, only the exact `S0READY:<token>`
/// text counts as ready; a plain `S0READY` (or another token) is ignored.
pub struct AckScanner {
    tail: Vec<u8>, strip: Stripper, ready_pat: Vec<u8>,
    pub saw_done: bool,
    /// Sticky: the ready text has been seen at least once.
    pub saw_ready: bool,
    /// The ready text ended inside the bytes of the most recent `feed` call (a match lying wholly
    /// in the carried-over tail does not count again). Lets the caller time each sighting.
    pub ready_in_last_feed: bool,
}

impl Default for AckScanner { fn default() -> Self { Self::with_ready_token(None) } }

impl AckScanner {
    pub fn new() -> Self { Self::default() }
    pub fn with_ready_token(token: Option<&str>) -> Self {
        Self { tail: Vec::new(), strip: Stripper::new(), ready_pat: ready_text(token).into_bytes(),
               saw_done: false, saw_ready: false, ready_in_last_feed: false }
    }
    /// Feed raw PTY bytes; returns the ack seqs completed by this chunk, each reported once.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u32> {
        let mut plain = std::mem::take(&mut self.tail);
        let carried = plain.len();
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
        let pl = self.ready_pat.len();
        // Only matches that end in this call's new bytes (end > carried).
        self.ready_in_last_feed = plain.windows(pl).enumerate()
            .any(|(k, w)| w == self.ready_pat.as_slice() && k + pl > carried);
        if self.ready_in_last_feed { self.saw_ready = true; }
        // Keep only unconsumed bytes (never re-scan a matched ack), enough to catch any pattern
        // split across reads; the ready pattern can be longer than TAIL with a token.
        let keep = TAIL.max(pl - 1);
        let start = last_match_end.max(plain.len().saturating_sub(keep));
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
    fn nf_escape_sgr0_does_not_leak() {
        let mut s = AckScanner::new();
        assert_eq!(s.feed(b"\x1b(B\x1b[mA000007\r\n"), vec![7]);
        // split inside the nF escape
        assert!(s.feed(b"\x1b(").is_empty());
        assert_eq!(s.feed(b"BA000008\r\n"), vec![8]);
    }
    #[test]
    fn dcs_apc_pm_sos_contents_ignored() {
        let mut s = AckScanner::new();
        assert_eq!(s.feed(b"\x1bPtmux;A000001\x1b\\A000002\r\n"), vec![2]);
        assert_eq!(s.feed(b"\x1b_A000003\x07\x1b^A000004\x1b\\\x1bXA000005\x1b\\A000006"), vec![6]);
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
    #[test]
    fn ready_token_wrong_ignored_right_accepted() {
        let mut s = AckScanner::with_ready_token(Some("c0ffee12"));
        s.feed(b"S0READY\r\n"); assert!(!s.saw_ready, "plain S0READY must not count with a token");
        s.feed(b"S0READY:deadbeef\r\n"); assert!(!s.saw_ready, "wrong token");
        s.feed(b"S0READY:c0ffee1\r\n"); assert!(!s.saw_ready, "truncated token");
        s.feed(b"x S0READY:c0ffee12\r\n"); assert!(s.saw_ready && s.ready_in_last_feed);
        s.feed(b"A000001\r\n"); assert!(!s.ready_in_last_feed, "tail must not re-report the match");
    }
    #[test]
    fn ready_token_split_across_chunks_with_escapes() {
        let mut s = AckScanner::with_ready_token(Some("0123456789abcdef"));
        for c in [&b"\x1b[3;1HS0RE"[..], b"ADY:0123", b"\x1b[1m45678", b"9abc", b"de"] {
            s.feed(c); assert!(!s.saw_ready);
        }
        s.feed(b"f |"); assert!(s.saw_ready && s.ready_in_last_feed);
        s.feed(b"\r\n"); assert!(!s.ready_in_last_feed);
    }
    #[test]
    fn no_token_plain_ready_still_works() {
        let mut s = AckScanner::new();
        s.feed(b"S0READY\r\n"); assert!(s.saw_ready && s.ready_in_last_feed);
        s.feed(b"zz"); assert!(!s.ready_in_last_feed);
    }
}
