//! `~/.config/tmls` readers and the pins writer, each test in a directory of its own.

use std::collections::BTreeSet;
use std::path::PathBuf;

use client_core::config::{Config, HostColor, default_color, tmls_slug};
use client_core::listing::SessionKey;

/// A fresh directory under cargo's target tmp dir (never /tmp); removed by `done`.
fn dir(test: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("config-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn done(dir: PathBuf) {
    std::fs::remove_dir_all(dir).unwrap();
}

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey { host: host.into(), name: name.into() }
}

fn remotes(cfg: &Config) -> Vec<&str> {
    cfg.hosts.iter().map(String::as_str).filter(|h| *h != cfg.local_name).collect()
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_string()
}

#[test]
fn missing_dir_gives_defaults() {
    let missing = dir("missing").join("not-there");
    let cfg = Config::load(&missing, &[]);
    assert_eq!(cfg.local_name, hostname());
    assert!(remotes(&cfg).is_empty());
    assert!(cfg.hosts.len() <= 1, "at most this machine: {:?}", cfg.hosts);
    assert!(cfg.host_colors.is_empty() && cfg.presets.is_empty() && cfg.pins.is_empty());
    assert!(!missing.exists(), "loading creates nothing");
    done(missing.parent().unwrap().to_path_buf());
}

#[test]
fn hosts_file_skips_comments_blank_lines_and_this_machine() {
    let d = dir("hosts");
    let me = hostname();
    std::fs::write(d.join("hosts"), format!("# my boxes\nnas\n\nlaptop  # sometimes off\n{me}\n-oProxyCommand=x\nnas\n")).unwrap();
    let cfg = Config::load(&d, &[]);
    assert_eq!(remotes(&cfg), ["nas", "laptop"]);
    // this machine comes first when it has tmux, under its own name
    assert!(cfg.hosts.iter().skip(1).all(|h| *h != cfg.local_name));

    // hosts on the command line replace the file's
    let cfg = Config::load(&d, &["pcl".to_string(), me, "-bad".to_string(), " ".to_string()]);
    assert_eq!(remotes(&cfg), ["pcl"]);
    done(d);
}

#[test]
fn name_file_overrides_the_hostname() {
    let d = dir("name");
    std::fs::write(d.join("name"), "laptop\n").unwrap();
    std::fs::write(d.join("hosts"), "nas\nlaptop\n").unwrap();
    let cfg = Config::load(&d, &[]);
    assert_eq!(cfg.local_name, "laptop");
    assert_eq!(remotes(&cfg), ["nas"], "a remote can't share this machine's name");

    std::fs::write(d.join("name"), "  \n").unwrap();
    assert_eq!(Config::load(&d, &[]).local_name, hostname(), "an empty name file is no override");
    done(d);
}

#[test]
fn pins_round_trip() {
    let d = dir("pins").join("made-on-save");
    std::fs::write(d.parent().unwrap().join("x"), "").unwrap();
    let me = Config::load(&d, &[]).local_name;
    let pins: BTreeSet<SessionKey> = [
        key(&me, "Agent Thing"),
        key("laanta-chy", "café: x"),
        key("user@nas", "50% / done"),
        key("nas", "rs:a/b"),
    ]
    .into();
    Config::save_pins(&d, &pins).unwrap();
    assert_eq!(Config::load(&d, &[]).pins, pins);

    // tmls's format: one whitespace-free token per line, sorted; its own slug for every pin
    // (this machine is "local" there), so tmls shows the same sessions pinned
    let text = std::fs::read_to_string(d.join("pins")).unwrap();
    let tokens: Vec<&str> = text.lines().collect();
    assert_eq!(tokens, text.split_whitespace().collect::<Vec<_>>());
    assert!(tokens.is_sorted());
    assert!(tokens.contains(&"local-Agent_Thing-0ffb10f9"), "{tokens:?}");
    assert!(tokens.contains(&"laanta-chy-caf___x-404715a3"), "{tokens:?}");
    assert_eq!(tokens.len(), 8);

    // unpinning removes both of a pin's lines
    let fewer: BTreeSet<SessionKey> = [key("laanta-chy", "café: x")].into();
    Config::save_pins(&d, &fewer).unwrap();
    assert_eq!(Config::load(&d, &[]).pins, fewer);
    assert_eq!(std::fs::read_to_string(d.join("pins")).unwrap().lines().count(), 2);
    Config::save_pins(&d, &BTreeSet::new()).unwrap();
    assert_eq!(std::fs::read_to_string(d.join("pins")).unwrap(), "");
    done(d.parent().unwrap().to_path_buf());
}

#[test]
fn pins_made_in_tmls_survive_a_save() {
    let d = dir("pins-tmls");
    // two sessions pinned in tmls (bare slugs: host and name can't be read back from them)
    std::fs::write(d.join("pins"), "local-work-b12e6407\nnas-build-0123abcd\n").unwrap();
    let cfg = Config::load(&d, &[]);
    assert!(cfg.pins.is_empty());
    assert_eq!(tmls_slug(&key(&cfg.local_name, "work"), &cfg.local_name), "local-work-b12e6407");

    let pins: BTreeSet<SessionKey> = [key(&cfg.local_name, "work"), key("nas", "other")].into();
    Config::save_pins(&d, &pins).unwrap();
    let text = std::fs::read_to_string(d.join("pins")).unwrap();
    assert_eq!(text.matches("local-work-b12e6407\n").count(), 1);
    assert!(text.contains("nas-build-0123abcd\n"), "a tmls pin this app can't name is kept: {text}");
    assert_eq!(Config::load(&d, &[]).pins, pins);

    // a damaged line of ours is kept as it is, not read as a pin
    std::fs::write(d.join("pins"), "rs:%zz/x\nrs:nohost\n").unwrap();
    assert!(Config::load(&d, &[]).pins.is_empty());
    Config::save_pins(&d, &BTreeSet::new()).unwrap();
    assert_eq!(std::fs::read_to_string(d.join("pins")).unwrap(), "rs:%zz/x\nrs:nohost\n");
    done(d);
}

#[test]
fn host_colors_bad_json_is_defaults() {
    let d = dir("colors");
    for bad in ["{not json", "[1, 2]", "\"blue\"", ""] {
        std::fs::write(d.join("host-colors.json"), bad).unwrap();
        assert!(Config::load(&d, &[]).host_colors.is_empty(), "{bad:?}");
    }
    std::fs::write(
        d.join("host-colors.json"),
        r#"{"nas": "teal", "laptop": "rose", "odd": "pink", "num": 3, "": "blue"}"#,
    )
    .unwrap();
    let colors = Config::load(&d, &[]).host_colors;
    assert_eq!(colors.into_iter().collect::<Vec<_>>(), [("laptop".to_string(), HostColor::Rose), ("nas".to_string(), HostColor::Teal)]);
    done(d);
}

#[test]
fn default_color_is_tmls_rule() {
    // host_colors.py: sha256(host)[:4] big-endian, modulo the palette (values from Python)
    let expected = [
        ("archbox", HostColor::Rose),
        ("laanta-chy", HostColor::Green),
        ("pcl", HostColor::Green),
        ("ph", HostColor::Amber),
        ("nas", HostColor::Teal),
        ("café", HostColor::Amber),
        ("", HostColor::Violet),
    ];
    for (host, color) in expected {
        assert_eq!(default_color(host), color, "{host}");
    }
}

#[test]
fn presets_keep_file_order_and_skip_bad_entries() {
    let d = dir("presets");
    std::fs::write(
        d.join("agent-presets.json"),
        r#"{"xdsp": ["codex", "--dangerously-bypass-approvals-and-sandbox"], "cdsp": ["claude", "--dangerously-skip-permissions"],
            "empty": [], "blank-arg": ["a", ""], "not-a-list": "claude", "": ["x"], "nums": [1]}"#,
    )
    .unwrap();
    let presets = Config::load(&d, &[]).presets;
    assert_eq!(
        presets,
        [
            ("xdsp".to_string(), vec!["codex".to_string(), "--dangerously-bypass-approvals-and-sandbox".to_string()]),
            ("cdsp".to_string(), vec!["claude".to_string(), "--dangerously-skip-permissions".to_string()]),
        ]
    );
    std::fs::write(d.join("agent-presets.json"), "{oops").unwrap();
    assert!(Config::load(&d, &[]).presets.is_empty());
    done(d);
}
