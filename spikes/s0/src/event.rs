//! The numbered key alphabet. Every event is payload bytes followed by a plain-ASCII marker
//! `#dddddd` that survives any terminal re-rendering. Kind is a pure function of seq.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum Kind { Letter, Digit, AltLetter, CtrlLetter, ModArrow, FKey, Repeat, Paste }

pub const KINDS: [Kind; 8] = [Kind::Letter, Kind::Digit, Kind::AltLetter, Kind::CtrlLetter,
                              Kind::ModArrow, Kind::FKey, Kind::Repeat, Kind::Paste];

/// Letters safe everywhere (no ctrl-c/d/z/s/q/b collisions once masked to 0x1f).
const LETTERS: &[u8] = b"aefghijklmnoprtuvwxy";
/// Ctrl+letter subset: avoid ^C ^D ^Z ^S ^Q ^B (tmux prefix) ^M ^J ^H ^I ^[ ^G? keep ^G out too (bell).
const CTRL_LETTERS: &[u8] = b"aefklnopruvwxy";

pub fn kind_of(seq: u32) -> Kind { KINDS[(seq % 8) as usize] }

pub fn marker(seq: u32) -> Vec<u8> { format!("#{seq:06}").into_bytes() }

fn letter(seq: u32) -> u8 { LETTERS[(seq / 8 % LETTERS.len() as u32) as usize] }

pub fn expected_payload(seq: u32) -> Vec<u8> {
    let n = seq / 8;
    match kind_of(seq) {
        Kind::Letter => vec![letter(seq)],
        Kind::Digit => vec![b'0' + (n % 10) as u8],
        Kind::AltLetter => vec![0x1b, letter(seq)],
        Kind::CtrlLetter => vec![CTRL_LETTERS[(n % CTRL_LETTERS.len() as u32) as usize] & 0x1f],
        Kind::ModArrow => {
            let m = [b'2', b'3', b'5', b'6'][(n % 4) as usize]; // shift, alt, ctrl, ctrl+shift
            let d = [b'A', b'B', b'C', b'D'][(n / 4 % 4) as usize];
            vec![0x1b, b'[', b'1', b';', m, d]
        }
        Kind::FKey => {
            let code: &[u8] = [&b"15~"[..], b"17~", b"18~", b"19~", b"20~", b"21~"][(n % 6) as usize];
            let mut v = vec![0x1b, b'['];
            v.extend_from_slice(code);
            v
        }
        Kind::Repeat => vec![letter(seq); 5],
        Kind::Paste => {
            let mut v = b"\x1b[200~".to_vec();
            v.extend_from_slice(format!("p{n:04}").as_bytes());
            v.extend_from_slice(b"\x1b[201~");
            v
        }
    }
}

/// The write sequence for seq. Repeat = five single-byte writes (key repeat), then marker.
pub fn encode(seq: u32) -> Vec<Vec<u8>> {
    let payload = expected_payload(seq);
    let mut writes = match kind_of(seq) {
        Kind::Repeat => payload.iter().map(|b| vec![*b]).collect::<Vec<_>>(),
        _ => vec![payload],
    };
    writes.push(marker(seq));
    writes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marker_is_hash_plus_six_digits() {
        assert_eq!(marker(42), b"#000042");
        assert_eq!(marker(999_999), b"#999999");
    }
    #[test]
    fn kinds_cycle_every_eight() {
        assert_eq!(kind_of(0), Kind::Letter);
        assert_eq!(kind_of(8), Kind::Letter);
        assert_eq!(kind_of(2), Kind::AltLetter);
        assert_eq!(kind_of(7), Kind::Paste);
    }
    #[test]
    fn payload_never_contains_forbidden_controls() {
        for seq in 0..10_000u32 {
            for b in expected_payload(seq) {
                assert!(![0x03, 0x04, 0x1a, 0x13, 0x11, 0x02, b'\r', b'\n', b'#'].contains(&b), "seq {seq} byte {b:#x}");
            }
        }
    }
    #[test]
    fn encode_concat_equals_payload_plus_marker() {
        for seq in [0u32, 5, 6, 7, 13, 9_999] {
            let joined: Vec<u8> = encode(seq).concat();
            let mut want = expected_payload(seq);
            want.extend(marker(seq));
            assert_eq!(joined, want);
        }
    }
    #[test]
    fn repeat_burst_is_five_writes_plus_marker() {
        assert_eq!(encode(6).len(), 6);
    }
}
