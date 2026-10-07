//! Navigation script for TUI paths: a comma list of `wait:ms`, `text:s`, `key:name`, `expect:s:ms`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step { Wait(u64), Text(String), Key(String), ExpectText(String, u64) }

/// Parse a nav spec; unknown steps, bad numbers and unknown key names are errors.
pub fn parse(spec: &str) -> anyhow::Result<Vec<Step>> {
    use anyhow::Context;
    spec.split(',').filter(|s| !s.is_empty()).map(|s| {
        let mut it = s.splitn(3, ':');
        Ok(match (it.next(), it.next(), it.next()) {
            (Some("wait"), Some(ms), _) => Step::Wait(ms.parse().with_context(|| format!("nav {s:?}: wait ms"))?),
            (Some("text"), Some(t), _) => Step::Text(t.to_string()),
            (Some("key"), Some(k), _) => {
                anyhow::ensure!(try_key_bytes(k).is_some(), "nav {s:?}: unknown key {k:?}");
                Step::Key(k.to_string())
            }
            (Some("expect"), Some(t), Some(ms)) =>
                Step::ExpectText(t.to_string(), ms.parse().with_context(|| format!("nav {s:?}: expect ms"))?),
            _ => anyhow::bail!("bad nav step {s:?}"),
        })
    }).collect()
}

/// Bytes for a key name. Panics on unknown names; `parse` has already rejected those.
pub fn key_bytes(k: &str) -> &'static [u8] {
    try_key_bytes(k).unwrap_or_else(|| panic!("unknown key {k}"))
}

pub fn try_key_bytes(k: &str) -> Option<&'static [u8]> {
    Some(match k {
        "enter" => b"\r", "esc" => b"\x1b", "tab" => b"\t",
        "up" => b"\x1b[A", "down" => b"\x1b[B",
        "alt+shift+up" => b"\x1b[1;4A", "alt+shift+down" => b"\x1b[1;4B",
        "alt+shift+left" => b"\x1b[1;4D", "alt+shift+right" => b"\x1b[1;4C",
        "j" => b"j", "k" => b"k", "b" => b"b", "q" => b"q",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_spec() {
        let s = parse("wait:300,expect:s0-recv:5000,key:j,key:enter,text:hi").unwrap();
        assert_eq!(s, vec![Step::Wait(300), Step::ExpectText("s0-recv".into(), 5000), Step::Key("j".into()),
                           Step::Key("enter".into()), Step::Text("hi".into())]);
    }
    #[test]
    fn key_bytes_table() { // named so it does not shadow super::key_bytes
        assert_eq!(key_bytes("enter"), b"\r");
        assert_eq!(key_bytes("alt+shift+up"), b"\x1b[1;4A");
        assert_eq!(key_bytes("j"), b"j");
    }
    #[test]
    fn bad_specs_are_errors() {
        assert!(parse("key:nope").is_err());
        assert!(parse("wait:abc").is_err());
        assert!(parse("frob:1").is_err());
        assert!(parse("expect:x").is_err());
    }
}
