//! VT sequence boundaries: byte offsets where a stream can be cut without splitting an escape
//! sequence or a UTF-8 character. The escape state machine mirrors libghostty-vt's parser
//! (`src/terminal/parse_table.zig` at the pinned commit): CSI incl. intermediates; OSC ending at
//! BEL or `ESC \`; DCS/APC/PM/SOS ending **only** at `ESC \` (BEL is payload there); nF escapes
//! `ESC 0x20-0x2f* final`; single-char `ESC x`; ESC anywhere aborts the current sequence and
//! starts a new one; CAN/SUB anywhere return to ground. Plus UTF-8 continuation tracking.
//! 8-bit C1 controls are not modelled (UTF-8 streams).

#[derive(Default, Clone, Copy, PartialEq)]
enum Esc { #[default] None, Esc, Csi, Nf, Osc, OscEsc, Str, StrEsc }

/// State after `ESC b`.
fn after_esc(b: u8) -> Esc {
    match b {
        b'[' => Esc::Csi,
        b']' => Esc::Osc,
        b'P' | b'_' | b'^' | b'X' => Esc::Str,
        0x20..=0x2f => Esc::Nf,
        0x1b => Esc::Esc,
        _ => Esc::None,
    }
}

/// Number of continuation bytes a UTF-8 lead byte announces (0 for ASCII / invalid leads).
fn utf8_need(b: u8) -> u8 {
    match b { 0xc2..=0xdf => 1, 0xe0..=0xef => 2, 0xf0..=0xf4 => 3, _ => 0 }
}

/// Offsets `p` (0..=len) such that `bytes[..p]` ends between complete units. `0` and `len` are
/// always included (a truncated tail still counts as the stream end).
pub fn boundaries(bytes: &[u8]) -> Vec<usize> {
    let mut out = vec![0];
    let mut esc = Esc::None;
    let mut need = 0u8; // pending UTF-8 continuation bytes
    for (i, &b) in bytes.iter().enumerate() {
        if esc == Esc::None && need > 0 {
            if (0x80..=0xbf).contains(&b) {
                need -= 1;
            } else {
                // Malformed: abandon the char and treat `b` as a fresh byte.
                need = 0;
                if b == 0x1b { esc = Esc::Esc } else { need = utf8_need(b) }
            }
        } else {
            esc = match esc {
                Esc::None => if b == 0x1b { Esc::Esc } else { need = utf8_need(b); Esc::None },
                // CAN / SUB abort any sequence back to ground.
                _ if b == 0x18 || b == 0x1a => Esc::None,
                Esc::Esc => after_esc(b),
                // ESC anywhere aborts and starts a new escape.
                Esc::Csi | Esc::Nf if b == 0x1b => Esc::Esc,
                Esc::Csi => if (0x40..=0x7e).contains(&b) { Esc::None } else { Esc::Csi },
                Esc::Nf => if (0x20..=0x2f).contains(&b) { Esc::Nf } else { Esc::None },
                // Only OSC accepts BEL as a terminator.
                Esc::Osc => match b { 0x07 => Esc::None, 0x1b => Esc::OscEsc, _ => Esc::Osc },
                Esc::Str => if b == 0x1b { Esc::StrEsc } else { Esc::Str },
                // ESC \ is ST; any other ESC x ends the string and starts a new escape.
                Esc::OscEsc | Esc::StrEsc => if b == b'\\' { Esc::None } else { after_esc(b) },
            };
        }
        let p = i + 1;
        if esc == Esc::None && need == 0 && p < bytes.len() { out.push(p); }
    }
    if !bytes.is_empty() { out.push(bytes.len()); }
    out
}

/// Largest boundary `<= p` (`bounds` sorted, as returned by [`boundaries`]; 0 if none).
pub fn last_boundary_at_or_before(bounds: &[usize], p: usize) -> usize {
    match bounds.binary_search(&p) {
        Ok(i) => bounds[i],
        Err(0) => 0,
        Err(i) => bounds[i - 1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_exclude_inside_csi_and_utf8() {
        let b = b"a\x1b[31mb\xe6\x97\xa5c";
        let v = boundaries(b);
        assert!(v.contains(&1)); assert!(!v.contains(&2)); assert!(!v.contains(&3)); assert!(v.contains(&6)); // after 'm'
        assert!(v.contains(&7)); assert!(!v.contains(&8)); assert!(!v.contains(&9)); assert!(v.contains(&10)); assert!(v.contains(&11));
    }

    #[test]
    fn osc_until_st_or_bel() {
        let v = boundaries(b"\x1b]0;ti\x07x\x1b]8;;u\x1b\\y");
        assert!(v.contains(&8)); assert!(!v.contains(&4)); assert!(v.contains(&16));
    }

    #[test]
    fn exact_sets() {
        assert_eq!(boundaries(b""), vec![0]);
        assert_eq!(boundaries(b"ab"), vec![0, 1, 2]);
        // nF escape ESC ( B, then a 4-byte emoji, then a DCS with ST.
        let b = b"\x1b(Bx\xf0\x9f\x98\x80\x1bPq\x1b\\z";
        assert_eq!(boundaries(b), vec![0, 3, 4, 8, 13, 14]);
        // Unterminated CSI at the end: only 0 and len.
        assert_eq!(boundaries(b"\x1b[1;2"), vec![0, 5]);
    }

    #[test]
    fn bel_inside_apc_is_not_a_boundary() {
        // ESC _ z a BEL b ESC \ c : BEL does not end APC in libghostty-vt, only ST does.
        let b = b"\x1b_za\x07b\x1b\\c";
        let v = boundaries(b);
        assert!(!v.contains(&5), "BEL inside APC is not a boundary: {v:?}");
        assert!(!v.contains(&6), "still inside APC after BEL: {v:?}");
        assert_eq!(v, vec![0, 8, 9]);
        // Same for DCS / PM / SOS.
        for intro in [b'P', b'^', b'X'] {
            let b = [0x1b, intro, b'a', 0x07, b'b', 0x1b, b'\\', b'c'];
            assert_eq!(boundaries(&b), vec![0, 7, 8], "intro {}", intro as char);
        }
        // OSC still ends at BEL.
        assert_eq!(boundaries(b"\x1b]0;t\x07c"), vec![0, 6, 7]);
    }

    #[test]
    fn last_boundary_lookup() {
        let v = vec![0, 3, 4, 8, 13, 14];
        assert_eq!(last_boundary_at_or_before(&v, 0), 0);
        assert_eq!(last_boundary_at_or_before(&v, 7), 4);
        assert_eq!(last_boundary_at_or_before(&v, 8), 8);
        assert_eq!(last_boundary_at_or_before(&v, 99), 14);
    }
}
