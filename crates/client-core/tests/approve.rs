//! Claude's permission dialog, read off a pane's capture (tmls `approve.py`).

use client_core::approve::{MAX_LINES, NO_KEY, QUESTION, YES_KEY, request, same, shown};

const DIALOG: &str = include_str!("data/approve-dialog.txt");

fn v(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| line.to_string()).collect()
}

#[test]
fn the_dialog_is_read_without_tips_rules_or_choices() {
    assert_eq!(request(DIALOG), Some(v(&["Bash command", "Create empty marker file", "touch /tmp/x"])));
}

#[test]
fn no_question_no_dialog() {
    assert_eq!(request("❯ ls\n● done\n"), None);
    assert_eq!(request(&DIALOG.replace(QUESTION, "Which one?")), None);
    assert_eq!(request(""), None);
}

#[test]
fn the_last_question_is_the_one_up() {
    let two = format!("{DIALOG}\n────────\n Edit file\n src/x.rs\n Do you want to proceed?\n ❯ 1. Yes\n");
    assert_eq!(request(&two), Some(v(&["Edit file", "src/x.rs"])));
}

#[test]
fn without_a_rule_the_dialog_is_everything_above_the_question() {
    assert_eq!(request(" Bash command\n\n echo hi\n Do you want to proceed?\n"), Some(v(&["Bash command", "echo hi"])));
}

#[test]
fn same_is_line_for_line() {
    let a = v(&["Bash command", "touch /tmp/x"]);
    assert!(same(&a, &a.clone()));
    assert!(!same(&a, &v(&["Bash command", "rm /tmp/x"])));
    assert!(!same(&a, &v(&["Bash command"])));
}

#[test]
fn a_long_dialog_is_cut_in_the_middle() {
    let long: Vec<String> = (0..30).map(|n| format!("line {n}")).collect();
    let cut = shown(&long);
    assert_eq!(cut.len(), MAX_LINES);
    assert_eq!((cut[0].as_str(), cut[5].as_str()), ("line 0", "line 5"));
    assert_eq!(cut[6], "… 19 more lines …");
    assert_eq!((cut[7].as_str(), cut[11].as_str()), ("line 25", "line 29"));
    let short = v(&["a", "b"]);
    assert_eq!(shown(&short), short);
}

#[test]
fn yes_is_1_and_no_is_escape() {
    // The number for No differs from prompt to prompt; Esc always declines (D4).
    assert_eq!((YES_KEY, NO_KEY), ("1", "Escape"));
}
