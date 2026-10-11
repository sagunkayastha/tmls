//! Finding the kitty window of a process: `kitty @ ls` and `ps` output, no kitty run.

use desktop::kitty::{parents, window_of, windows};

const LS: &str = r#"[
  {"id": 1, "is_focused": true, "tabs": [
    {"id": 1, "windows": [
      {"id": 7, "pid": 100, "foreground_processes": [{"pid": 150, "cmdline": ["claude"]}]},
      {"id": 8, "pid": 200, "foreground_processes": []}
    ]},
    {"id": 2, "windows": [{"id": 9, "pid": 300}]}
  ]},
  {"id": 2, "tabs": [{"id": 3, "windows": [{"id": 12, "pid": 400, "foreground_processes": [{"pid": 400}]}]}]}
]"#;

#[test]
fn every_window_process_is_found() {
    let w = windows(LS);
    let mut pids: Vec<(u32, u64)> = w.into_iter().collect();
    pids.sort();
    assert_eq!(pids, [(100, 7), (150, 7), (200, 8), (300, 9), (400, 12)]);
    assert!(windows("").is_empty());
    assert!(windows("not json").is_empty());
    assert!(windows(r#"{"id": 1}"#).is_empty());
    assert!(
        windows(r#"[{"tabs": [{"windows": [{"pid": 5}]}]}]"#).is_empty(),
        "no id"
    );
}

#[test]
fn the_window_is_the_nearest_ancestors() {
    let p = parents(
        "  100     1\n  150   100\n  151   150\n  900   151\n  500     1\n  501   500\nbad line\n\n",
    );
    assert_eq!(p.len(), 6);
    let w = windows(LS);
    assert_eq!(
        window_of(150, &p, &w),
        Some(7),
        "a window's own foreground process"
    );
    assert_eq!(window_of(900, &p, &w), Some(7), "a grandchild of one");
    assert_eq!(window_of(100, &p, &w), Some(7));
    assert_eq!(window_of(501, &p, &w), None, "runs in another terminal");
    assert_eq!(window_of(77, &p, &w), None, "not in the process table");
    // A damaged table with a loop ends.
    let looped = parents("10 11\n11 10\n");
    assert_eq!(window_of(10, &looped, &w), None);
}
