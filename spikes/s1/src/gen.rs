//! Adversarial VT byte streams for the snapshot/restore fidelity harness. Each stream targets one
//! area of terminal state (attributes, wide chars, alt screen, regions, modes, OSC, …). All
//! streams are deterministic (no RNG) and at least 1 KiB. `wrap_pending` assumes 120 columns.

use std::fmt::Write as _;

const ESC: &str = "\x1b";
const CSI: &str = "\x1b[";
const ST: &str = "\x1b\\";

/// All named adversarial streams, in a fixed order.
pub fn adversarial() -> Vec<(String, Vec<u8>)> {
    let v: Vec<(&str, String)> = vec![
        ("sgr_matrix", sgr_matrix()),
        ("utf8_wide_combining", utf8_wide_combining()),
        ("alt_screen_cycle", alt_screen_cycle()),
        ("scroll_region", scroll_region()),
        ("scrollback_200", scrollback_200()),
        ("modes_mix", modes_mix()),
        ("kitty_kbd", kitty_kbd()),
        ("osc_title_pwd_hyperlink", osc_title_pwd_hyperlink()),
        ("tabs_and_cursor_save", tabs_and_cursor_save()),
        ("erase_insert_delete", erase_insert_delete()),
        ("wrap_pending", wrap_pending()),
        ("dcs_apc_noise", dcs_apc_noise()),
        ("resize_marker", resize_marker()),
    ];
    v.into_iter().map(|(n, s)| (n.to_string(), s.into_bytes())).collect()
}

/// Every SGR attribute on/off, 16 / 256 / truecolor fg+bg, underline styles and underline colour.
/// Ends with a non-default pen (bold + 256 fg + truecolor bg) so the cursor style is exercised.
fn sgr_matrix() -> String {
    let mut s = String::new();
    for (on, off) in [(1, 22), (2, 22), (3, 23), (4, 24), (5, 25), (7, 27), (8, 28), (9, 29), (53, 55)] {
        let _ = write!(s, "{CSI}{on}mattr{on}{CSI}{off}m ");
    }
    s.push_str("\r\n");
    for u in 1..=5 {
        let _ = write!(s, "{CSI}4:{u}mul{u}{CSI}58;2;{};{};{}mcol{CSI}59m{CSI}24m ", u * 40, 255 - u * 40, 128);
    }
    s.push_str("\r\n");
    for c in (30..=37).chain(90..=97) {
        let _ = write!(s, "{CSI}{c}m{c}{CSI}{}m#", c + 10);
    }
    let _ = write!(s, "{CSI}39;49m\r\n");
    for n in 0..256 {
        let _ = write!(s, "{CSI}38;5;{n}m{CSI}48;5;{}m{:02x}", 255 - n, n);
        if n % 32 == 31 { s.push_str("\x1b[0m\r\n"); }
    }
    for i in 0..48u32 {
        let (r, g, b) = ((i * 5) % 256, (255 - i * 5) % 256, (i * 37) % 256);
        let _ = write!(s, "{CSI}38;2;{r};{g};{b}m{CSI}48;2;{b};{r};{g}m*");
    }
    let _ = write!(s, "{CSI}0m\r\n{CSI}1;3;4;7;38;5;201m combo {CSI}0m\r\n");
    let _ = write!(s, "{CSI}1;38;5;208;48;2;10;20;30m");
    s
}

/// CJK, Hangul, emoji ZWJ sequences, flags, skin tones, VS16 and combining marks, including a
/// wide char placed in the last column (forces a spacer/wrap).
fn utf8_wide_combining() -> String {
    let samples = [
        "日本語テキスト", "中文字符", "한국어", "👨\u{200d}👩\u{200d}👧\u{200d}👦", "🇯🇵🇺🇸",
        "👍🏽", "❤\u{fe0f}", "e\u{301}", "a\u{308}\u{323}", "Z\u{335}\u{30c}\u{317}",
        "ｆｕｌｌｗｉｄｔｈ", "🧑\u{200d}💻", "ﷺ", "\u{1f600}",
    ];
    let mut s = String::new();
    for row in 0..20 {
        let _ = write!(s, "{row:02}:");
        for k in 0..samples.len() {
            s.push_str(samples[(row + k) % samples.len()]);
            s.push(' ');
        }
        s.push_str("\r\n");
    }
    // Wide char straddling the right margin at both 80 and 120 columns.
    for col in [80u32, 120] {
        let _ = write!(s, "{CSI}{col}G日x\r\n");
        let _ = write!(s, "{CSI}{}G語\r\n", col - 1);
    }
    s
}

/// Primary text, switch to alt (1049h), alt text, back to primary (1049l), more primary — cycled
/// three times, also using 47 and 1047. Ends on the primary screen.
fn alt_screen_cycle() -> String {
    let mut s = String::new();
    for cycle in 0..3 {
        for i in 0..6 {
            let _ = write!(s, "primary cycle {cycle} line {i} {CSI}3{}mcolour{CSI}0m\r\n", i % 8);
        }
        let enter = ["?1049h", "?1047h", "?47h"][cycle];
        let leave = ["?1049l", "?1047l", "?47l"][cycle];
        let _ = write!(s, "{CSI}{enter}{CSI}2J{CSI}H");
        for i in 0..8 {
            let _ = write!(s, "{CSI}{};{}HALT {cycle} row {i} {CSI}7minverse{CSI}27m", i + 2, 5 + i * 3);
        }
        let _ = write!(s, "{CSI}{leave}");
        let _ = write!(s, "back on primary after cycle {cycle}\r\n");
    }
    s
}

/// DECSTBM with scrolling (LF, RI, SU, SD) inside the region, DECLRMM + DECSLRM with text, then
/// a region left set at the end.
fn scroll_region() -> String {
    let mut s = String::new();
    for i in 0..24 {
        let _ = write!(s, "{CSI}{};1Hbase row {i:02} ................", i + 1);
    }
    let _ = write!(s, "{CSI}5;15r{CSI}15;1H");
    for i in 0..20 {
        let _ = write!(s, "\nscroll-in {i:02}\r");
    }
    let _ = write!(s, "{CSI}5;1H");
    for i in 0..5 {
        let _ = write!(s, "{ESC}Mreverse {i}\r");
    }
    let _ = write!(s, "{CSI}3S{CSI}2T{CSI}10;1Hafter SU/SD");
    let _ = write!(s, "{CSI}?69h{CSI}10;50s{CSI}6;10H");
    for i in 0..15 {
        let _ = write!(s, "lr-margin text {i:02} wraps within the margins ");
    }
    let _ = write!(s, "{CSI}?69l{CSI}r{CSI}24;1H");
    let _ = write!(s, "{CSI}3;20r{CSI}12;4Hregion 3..20 left set");
    s
}

/// 300 numbered lines (on a 24-row terminal 276 go to scrollback).
fn scrollback_200() -> String {
    let mut s = String::new();
    for i in 0..300 {
        let _ = write!(s, "scrollback line {i:03} {CSI}3{}m{}{CSI}0m\r\n", i % 8, "=".repeat(i % 40));
    }
    s
}

/// Toggle DECCKM(1), DECAWM(7), DECTCEM(25), bracketed paste(2004), mouse 1000/1002/1006,
/// focus(1004), sync output(2026). Ends with a mixed set: 1, 2004, 1002, 1006, 1004 on; 7 and
/// 25 off; 1000, 2026 off.
fn modes_mix() -> String {
    let modes = [1, 7, 25, 2004, 1000, 1002, 1006, 1004, 2026];
    let mut s = String::new();
    for round in 0..4 {
        for m in modes {
            let _ = write!(s, "{CSI}?{m}hon {m} r{round} {CSI}?{m}loff {m}\r\n");
        }
    }
    for m in [1, 2004, 1002, 1006, 1004] {
        let _ = write!(s, "{CSI}?{m}h");
    }
    let _ = write!(s, "{CSI}?7l{CSI}?25l{CSI}?1000l{CSI}?2026l");
    let _ = write!(s, "final mode set: 1 2004 1002 1006 1004 on; 7 25 off\r\n");
    s
}

/// Kitty keyboard protocol: push (`CSI > 1 u`), set (`CSI = 5;1 u`), modify, pop, query. Ends
/// with flags pushed and set to 5.
fn kitty_kbd() -> String {
    let mut s = String::new();
    for i in 0..16 {
        let _ = write!(s, "{CSI}>1ukitty push {i}\r\n{CSI}=5;1uset 5\r\n{CSI}=2;2uor 2\r\n");
        let _ = write!(s, "{CSI}=1;3uandnot 1\r\n{CSI}<1upop\r\n");
    }
    let _ = write!(s, "{CSI}>1u{CSI}=5;1ufinal flags 5 pushed\r\n");
    s
}

/// OSC 0 / 2 titles, OSC 7 pwd, OSC 8 hyperlinks open/close (with and without id), OSC 52
/// clipboard. Alternates BEL and ST terminators. Ends with a hyperlink open.
fn osc_title_pwd_hyperlink() -> String {
    let mut s = String::new();
    for i in 0..10 {
        let _ = write!(s, "{ESC}]0;title {i}\x07{ESC}]2;window {i}{ST}");
        let _ = write!(s, "{ESC}]7;file://host/home/user/dir{i}{ST}");
        let _ = write!(s, "pre {ESC}]8;id=l{i};https://example.com/p/{i}{ST}link {i}{ESC}]8;;{ST} post ");
        let _ = write!(s, "{ESC}]8;;https://example.org/{i}\x07anon{ESC}]8;;\x07\r\n");
        let _ = write!(s, "{ESC}]52;c;aGVsbG8gd29ybGQ={ST}");
    }
    let _ = write!(s, "{ESC}]0;final title{ST}{ESC}]7;file://host/final/pwd{ST}");
    let _ = write!(s, "{ESC}]8;id=open;https://example.com/still-open{ST}open link text");
    s
}

/// HTS / TBC tab stop edits, HT movement, DECSC/DECRC (with SGR + origin state), CUP/CUF/CUB/
/// CUU/CUD/CHA/VPA. Ends with custom tab stops and a saved cursor different from the live one.
fn tabs_and_cursor_save() -> String {
    let mut s = String::new();
    let _ = write!(s, "{CSI}3g");
    for col in [3, 7, 13, 21, 34, 55, 89, 100] {
        let _ = write!(s, "{CSI}{col}G{ESC}H");
    }
    let _ = write!(s, "\r");
    for i in 0..10 {
        let _ = write!(s, "a\tb\tc\td\te\tf\t{i}\r\n");
    }
    let _ = write!(s, "{CSI}21G{CSI}0g\r\ttab after clearing 21\r\n");
    for i in 0..15 {
        let _ = write!(s, "{CSI}{};{}H{CSI}1;3{}m{ESC}7saved {i}", i + 1, i * 4 + 1, i % 8);
        let _ = write!(s, "{CSI}0m{CSI}5C{CSI}2Dmv{CSI}1A{CSI}2Bdown{CSI}40Gcha{CSI}20dvpa");
        let _ = write!(s, "{ESC}8restored {i}{CSI}0m");
    }
    let _ = write!(s, "{CSI}10;20H{CSI}4;35m{ESC}7{CSI}0m{CSI}22;60Hlive cursor elsewhere");
    s
}

/// Fill screen then ED 0/1/2, EL 0/1/2, ICH, DCH, IL, DL, ECH, with a coloured background
/// (erase uses the current bg).
fn erase_insert_delete() -> String {
    let mut s = String::new();
    let fill = |s: &mut String, tag: &str| {
        for r in 1..=24 {
            let _ = write!(s, "{CSI}{r};1H{tag}{r:02}abcdefghijklmnopqrstuvwxyz0123456789");
        }
    };
    fill(&mut s, "F");
    let _ = write!(s, "{CSI}44m{CSI}5;10H{CSI}0K{CSI}6;10H{CSI}1K{CSI}7;10H{CSI}2K");
    let _ = write!(s, "{CSI}8;5H{CSI}5@INS{CSI}9;5H{CSI}3P{CSI}10;5H{CSI}6X");
    let _ = write!(s, "{CSI}12;1H{CSI}2L{CSI}16;1H{CSI}3M{CSI}0m");
    let _ = write!(s, "{CSI}20;20H{CSI}0J");
    fill(&mut s, "G");
    let _ = write!(s, "{CSI}41m{CSI}4;30H{CSI}1J{CSI}0m");
    fill(&mut s, "H");
    let _ = write!(s, "{CSI}42m{CSI}2J{CSI}0m{CSI}H");
    fill(&mut s, "I");
    let _ = write!(s, "{CSI}43m{CSI}3;3H{CSI}4L{CSI}3;3H{CSI}10@{CSI}0m{CSI}20;1H{CSI}2M");
    s
}

/// Text that ends exactly at column 120 (pending-wrap state), followed by more text, CR LF,
/// CUP and SGR changes. Ends in pending-wrap state.
fn wrap_pending() -> String {
    let line = |c: char| std::iter::repeat_n(c, 120).collect::<String>();
    let mut s = String::new();
    let _ = write!(s, "{}more after wrap\r\n", line('a'));
    let _ = write!(s, "{}\r\nCRLF after full line\r\n", line('b'));
    let _ = write!(s, "{}{CSI}5;1Hcup after full line\r\n", line('c'));
    let _ = write!(s, "{}{CSI}31mx{CSI}0m\r\n", line('d'));
    let _ = write!(s, "{CSI}?7l{}nowrap overflow{CSI}?7h\r\n", line('e'));
    let _ = write!(s, "{}日\r\n", &line('f')[..119]);
    for c in ['h', 'i', 'j'] {
        let _ = write!(s, "{CSI}1m{}{CSI}0m{CSI}2Aup from pending wrap\r\n", line(c));
    }
    let _ = write!(s, "{CSI}10;1H{}", line('g'));
    s
}

/// DCS (XTGETTCAP, DECRQSS, tmux passthrough), APC (non-graphics), PM, SOS strings ended by ST
/// (plus a BEL inside an APC, which does not terminate it), interleaved with text. Text inside
/// strings (`HIDDEN…`) must not appear on screen; `t<i>` / `visible <i>` must.
fn dcs_apc_noise() -> String {
    let mut s = String::new();
    for i in 0..15 {
        let _ = write!(s, "t{i} {ESC}P+q544e{ST}{ESC}P$qm{ST}{ESC}Ptmux;HIDDEN{i}{ST}");
        let _ = write!(s, "{ESC}_zHIDDEN apc {i}{ST}{ESC}^HIDDEN pm {i}{ST}{ESC}XHIDDEN sos {i}{ST}");
        // BEL does not end APC in libghostty-vt (only OSC): "HIDDEN bel" stays inside until ST.
        let _ = write!(s, "{ESC}_zHIDDEN bel\x07HIDDEN still apc{ST}visible {i}\r\n");
    }
    s
}

/// Plain numbered text with lines of varied length (some longer than 80 columns), used to test
/// snapshot across a resize at the midpoint.
fn resize_marker() -> String {
    let mut s = String::new();
    for i in 0..40 {
        let _ = write!(s, "resize {i:02} {}\r\n", "word ".repeat(3 + (i * 7) % 25));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::screen::Screen;
    use libghostty_vt::terminal::{Options, Terminal};
    use std::collections::HashSet;

    #[test]
    fn adversarial_streams_nonempty_and_named() {
        let v = adversarial();
        assert!(v.len() >= 13, "got {} streams", v.len());
        let names: HashSet<_> = v.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(names.len(), v.len(), "duplicate names");
        for (n, b) in &v {
            assert!(b.len() >= 1024, "{n} is {} bytes", b.len());
        }
        assert_eq!(adversarial(), v, "not deterministic");
    }

    #[test]
    fn alt_screen_cycle_ends_on_primary() {
        let b = adversarial().into_iter().find(|(n, _)| n == "alt_screen_cycle").unwrap().1;
        let mut t = Terminal::new(Options { cols: 120, rows: 40, max_scrollback: 1000 }).unwrap();
        t.vt_write(&b[..b.len() / 2]);
        t.vt_write(&b[b.len() / 2..]);
        assert_eq!(t.active_screen().unwrap(), Screen::Primary);
    }

    #[test]
    fn dcs_apc_noise_hides_string_payloads() {
        let b = adversarial().into_iter().find(|(n, _)| n == "dcs_apc_noise").unwrap().1;
        let mut t = Terminal::new(Options { cols: 120, rows: 40, max_scrollback: 1000 }).unwrap();
        t.vt_write(&b);
        let (g, _) = crate::grid::capture(&mut t).unwrap();
        let text: String = g.cells.iter().map(|c| c.text.as_str()).collect();
        assert!(!text.contains("HIDDEN"), "string payload leaked");
        for i in 0..15 {
            assert!(text.contains(&format!("t{i} visible {i}")), "missing line {i}");
        }
    }
}
