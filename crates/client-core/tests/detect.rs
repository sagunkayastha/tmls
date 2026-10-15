//! herdr's agent-detection rules and the engine that reads them, against panes of codex 0.161.0
//! and opencode 1.18.33 captured on archbox on 2026-10-14 (`tests/data/agent-screens`: what
//! `tmux capture-pane -p` printed, and the pane's `#{pane_title}`), and against made-up screens
//! for the engine's corners.

use client_core::detect::{
    AgentState, Detection, ENGINE_VERSION, HERDR_COMMIT, INDEX, RULE_FILES, agent_of_command, detect, parse_rules,
    rules, screen_text,
};

fn capture(name: &str) -> (String, String) {
    let dir = format!("{}/tests/data/agent-screens", env!("CARGO_MANIFEST_DIR"));
    let read = |ext: &str| {
        let path = format!("{dir}/{name}.{ext}");
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    (read("screen"), read("title").trim_end_matches('\n').to_string())
}

/// The state and deciding rule of agent `id` on capture `name`.
fn on(id: &str, name: &str) -> (AgentState, Option<&'static str>) {
    let (screen, title) = capture(name);
    let lines: Vec<&str> = screen.lines().collect();
    let d = detect(id, &lines, &title).unwrap_or_else(|| panic!("no rules for {id}"));
    (d.state, d.rule)
}

/// Agent `id`'s state on a made-up screen and title.
fn state(id: &str, screen: &str, title: &str) -> AgentState {
    detect(id, &screen.lines().collect::<Vec<_>>(), title).unwrap().state
}

#[test]
fn every_vendored_rule_file_parses_and_is_listed_in_the_index() {
    for (id, text) in RULE_FILES {
        let parsed = parse_rules(text).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(parsed.id, *id);
        assert!(parsed.min_engine_version.is_some_and(|v| v <= ENGINE_VERSION), "{id}");
        assert!(rules(id).is_some(), "{id}");
        assert!(INDEX.contains(&format!("id = \"{id}\"")), "{id} is not in index.toml");
    }
    assert_eq!(INDEX.matches("[[agents]]").count(), RULE_FILES.len(), "index.toml lists a file that is not vendored");
    assert!(rules("mastracode").is_none(), "herdr has no screen rules for it");
    assert_eq!(HERDR_COMMIT.len(), 40);
    let notice = include_str!("../agent-detection/NOTICE");
    assert!(notice.contains(HERDR_COMMIT), "NOTICE and HERDR_COMMIT name the same commit");
}

#[test]
fn codex_captures() {
    // The title alone says it: a spinner while it works, "Action Required" while it asks.
    assert_eq!(on("codex", "codex-start"), (AgentState::Idle, Some("osc_title_idle")));
    assert_eq!(on("codex", "codex-working"), (AgentState::Working, Some("osc_title_working")));
    assert_eq!(on("codex", "codex-idle-after"), (AgentState::Idle, Some("osc_title_idle")));
    assert_eq!(on("codex", "codex-approval-blocked"), (AgentState::Blocked, Some("osc_title_blocked")));
    // The update picker at start-up, before codex has set a title: tmux shows the host's name.
    assert_eq!(on("codex", "codex-update-blocked"), (AgentState::Blocked, Some("startup_update")));
}

#[test]
fn codex_captures_read_by_their_screen_alone() {
    // A title from before (the shell's, or none) still leaves the screen rules to decide.
    let by_screen = |name: &str, title: &str| {
        let (screen, _) = capture(name);
        let d = detect("codex", &screen.lines().collect::<Vec<_>>(), title).unwrap();
        (d.state, d.rule)
    };
    for title in ["", "sgkayast@archbox:~/play"] {
        assert_eq!(by_screen("codex-working", title).0, AgentState::Working, "{title:?}");
        assert_eq!(by_screen("codex-working", title).1, Some("screen_working_fallback"));
        assert_eq!(by_screen("codex-approval-blocked", title), (AgentState::Blocked, Some("live_strong_blocker")));
        assert_eq!(by_screen("codex-idle-after", title).0, AgentState::Idle, "{title:?}");
        assert_eq!(by_screen("codex-start", title).0, AgentState::Idle, "{title:?}");
    }
}

#[test]
fn opencode_captures() {
    assert_eq!(on("opencode", "opencode-idle"), (AgentState::Idle, None));
    assert_eq!(on("opencode", "opencode-idle-after"), (AgentState::Idle, None));
    assert_eq!(on("opencode", "opencode-working"), (AgentState::Working, Some("progress_bar_working")));
    assert_eq!(on("opencode", "opencode-permission-blocked"), (AgentState::Blocked, Some("permission_required")));
}

#[test]
fn highest_priority_wins_and_a_tie_goes_to_the_first_rule() {
    let text = r#"
id = "toy"
[[rules]]
id = "first"
state = "working"
priority = 10
contains = ["busy"]
[[rules]]
id = "second"
state = "blocked"
priority = 10
contains = ["busy"]
[[rules]]
id = "louder"
state = "blocked"
priority = 20
contains = ["ask"]
"#;
    let toy: &'static _ = Box::leak(Box::new(parse_rules(text).unwrap()));
    assert_eq!(toy.detect("busy\n", "").rule, Some("first"));
    assert_eq!(toy.detect("BUSY, then ask\n", "").rule, Some("louder"), "contains ignores case");
    assert_eq!(toy.detect("nothing\n", ""), Detection { state: AgentState::Idle, rule: None, skip_state_update: false });
}

#[test]
fn nested_gates_regions_and_skip_state_update() {
    let text = r#"
id = "toy"
min_engine_version = 3
[[rules]]
id = "viewer"
state = "unknown"
priority = 50
skip_state_update = true
region = "bottom_non_empty_lines(1)"
contains = ["q to quit"]
[[rules]]
id = "top"
state = "blocked"
priority = 40
region = "top_non_empty_lines(1)"
regex = ['\ATrust\?']
[[rules]]
id = "nest"
state = "working"
priority = 30
any = [{ contains = ["spin"] }, { all = [{ line_regex = ['^\d+s$'] }] }]
not = [{ contains = ["stopped"] }]
[[rules]]
id = "title"
state = "working"
priority = 5
region = "osc_title"
contains = ["⠋"]
"#;
    let toy: &'static _ = Box::leak(Box::new(parse_rules(text).unwrap()));
    let d = toy.detect("history\nq to quit\n", "");
    assert_eq!((d.state, d.skip_state_update), (AgentState::Unknown, true));
    assert_eq!(toy.detect("q to quit\nmore\n", "").rule, None, "only the bottom line counts");
    assert_eq!(toy.detect("Trust?\nyes\n", "").rule, Some("top"));
    // herdr counts non-empty lines but keeps the blank ones above them in the region: \A sees those
    assert_eq!(toy.detect("\n\nTrust?\nyes\n", "").rule, None);
    assert_eq!(toy.detect("x\nTrust?\n", "").rule, None);
    assert_eq!(toy.detect("spin\n", "").rule, Some("nest"));
    assert_eq!(toy.detect("work\n12s\n", "").rule, Some("nest"));
    assert_eq!(toy.detect("spin\nstopped\n", "").rule, None);
    assert_eq!(toy.detect("idle\n", "⠋ codex").rule, Some("title"));
    assert_eq!(toy.detect("idle\n⠋\n", "").rule, None, "the title rule reads the title only");
}

#[test]
fn bad_rule_files_are_refused_as_herdr_refuses_them() {
    let bad = [
        ("id = \"x\"\n", "no rules"),
        ("id = \"x\"\nwho = 1\n[[rules]]\nid = \"r\"\ncontains = [\"a\"]\n", "unknown"),
        ("id = \"x\"\n[[rules]]\nid = \"r\"\ncolour = 1\ncontains = [\"a\"]\n", "unknown"),
        ("id = \"x\"\n[[rules]]\nid = \"r\"\nstate = \"idle\"\n", "matcher"),
        ("id = \"x\"\n[[rules]]\nid = \"r\"\nregion = \"middle\"\ncontains = [\"a\"]\n", "region"),
        ("id = \"x\"\n[[rules]]\nid = \"r\"\nregion = \"top_non_empty_lines(05)\"\ncontains = [\"a\"]\n", "region"),
        ("id = \"x\"\n[[rules]]\nid = \"r\"\nregex = ['(']\n", "regex"),
        ("id = \"x\"\n[[rules]]\nid = \"r\"\nstate = \"idle\"\nskip_state_update = true\ncontains = [\"a\"]\n", "skip_state_update"),
        ("id = \"x\"\n[[rules]]\nid = \"r\"\ncontains = [\"a\"]\nnot = [{}]\n", "matcher"),
        ("id = \"x\"\nmin_engine_version = 4\n[[rules]]\nid = \"r\"\ncontains = [\"a\"]\n", "engine"),
        ("id = \"x\"\nmin_engine_version = 2\n[[rules]]\nid = \"r\"\nregion = \"top_non_empty_lines(3)\"\ncontains = [\"a\"]\n", "top_non_empty_lines"),
        ("id = \"x\"\n[[rules]]\nid = \" \"\ncontains = [\"a\"]\n", "id"),
        ("not toml [", ""),
    ];
    for (text, why) in bad {
        let err = parse_rules(text).err().unwrap_or_else(|| panic!("accepted: {text}"));
        assert!(err.contains(why), "{text:?}: {err}");
    }
}

#[test]
fn a_capture_becomes_herdrs_detection_text() {
    assert_eq!(screen_text(&["a  ", "", "b", "   ", ""]), "a\n\nb\n");
    assert_eq!(screen_text(&["", "  "]), "");
    assert_eq!(screen_text(&[]), "");
}

#[test]
fn process_names_come_from_the_rule_files() {
    for (command, id) in [
        ("codex", "codex"),
        ("opencode", "opencode"),
        ("open-code", "opencode"),
        ("gemini", "gemini"),
        ("kiro-cli", "kiro"),
        ("cursor-agent", "cursor"),
        ("ghcs", "copilot"),
        ("claude", "claude"),
    ] {
        assert_eq!(agent_of_command(command), Some(id), "{command}");
    }
    for command in ["bash", "node", "vim", "herdr:opencode", "kimi code", ""] {
        assert_eq!(agent_of_command(command), None, "{command}");
    }
    // Rules for an agent not captured here (gemini runs under node), straight from its file.
    assert_eq!(state("gemini", "╭───\n│ Allow execution of: 'rm'?\n╰───\n", ""), AgentState::Blocked);
    assert_eq!(state("gemini", "⠼ Thinking (esc to cancel, 3s)\n", ""), AgentState::Working);
    assert_eq!(state("gemini", "> Type your message\n", ""), AgentState::Idle);
}
