//! Navigation script for TUI paths: a comma list of `wait:ms`, `text:s`, `key:name`, `expect:s:ms`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step { Wait(u64), Text(String), Key(String), ExpectText(String, u64) }

pub fn parse(spec: &str) -> Vec<Step> {
    spec.split(',').filter(|s| !s.is_empty()).map(|s| {
        let mut it = s.splitn(3, ':');
        match (it.next(), it.next(), it.next()) {
            (Some("wait"), Some(ms), _) => Step::Wait(ms.parse().expect("wait ms")),
            (Some("text"), Some(t), _) => Step::Text(t.to_string()),
            (Some("key"), Some(k), _) => Step::Key(k.to_string()),
            (Some("expect"), Some(t), Some(ms)) => Step::ExpectText(t.to_string(), ms.parse().expect("expect ms")),
            _ => panic!("bad nav step {s:?}"),
        }
    }).collect()
}

pub fn key_bytes(k: &str) -> &'static [u8] {
    match k {
        "enter" => b"\r", "esc" => b"\x1b", "tab" => b"\t",
        "up" => b"\x1b[A", "down" => b"\x1b[B",
        "alt+shift+up" => b"\x1b[1;4A", "alt+shift+down" => b"\x1b[1;4B",
        "alt+shift+left" => b"\x1b[1;4D", "alt+shift+right" => b"\x1b[1;4C",
        "j" => b"j", "k" => b"k", "b" => b"b", "q" => b"q",
        other => panic!("unknown key {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_spec() {
        let s = parse("wait:300,expect:s0-recv:5000,key:j,key:enter,text:hi");
        assert_eq!(s, vec![Step::Wait(300), Step::ExpectText("s0-recv".into(), 5000), Step::Key("j".into()),
                           Step::Key("enter".into()), Step::Text("hi".into())]);
    }
    #[test]
    fn key_bytes_table() { // named so it does not shadow super::key_bytes
        assert_eq!(key_bytes("enter"), b"\r");
        assert_eq!(key_bytes("alt+shift+up"), b"\x1b[1;4A");
        assert_eq!(key_bytes("j"), b"j");
    }
}
