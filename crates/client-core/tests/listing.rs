//! The listing parser and the status rules, against captured and synthesised listing outputs
//! (`tests/data/*.txt`). Cases ported from tmls's `tests/test_hosts.py` keep its inputs.

use client_core::listing::{
    HostSnapshot, LISTING_SCRIPT, SessionInfo, SessionKey, Status, Tool, background_sessions, listing_command,
    parse_background, parse_listing, split_host, valid_server_name,
};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn parse(name: &str) -> HostSnapshot {
    parse_listing("nas", &fixture(name))
}

fn session<'a>(snap: &'a HostSnapshot, name: &str) -> &'a SessionInfo {
    snap.sessions
        .iter()
        .find(|s| s.key.name == name)
        .unwrap_or_else(|| panic!("no session {name:?} in {:?}", names(snap)))
}

fn names(snap: &HostSnapshot) -> Vec<&str> {
    snap.sessions.iter().map(|s| s.key.name.as_str()).collect()
}

#[test]
fn parses_a_real_archbox_listing() {
    // tests/data/archbox.txt: the tmux half of the listing as archbox printed it on 2026-10-11
    // (`tmux ls` then showed Projects, Raid, Rnners, none attached); its Claude half is empty.
    let snap = parse_listing("archbox", &fixture("archbox.txt"));
    assert!(snap.online);
    assert_eq!(snap.error, None);
    assert_eq!(snap.host, "archbox");
    let expected = [("Projects", Status::Running, 1), ("Raid", Status::Idle, 33), ("Rnners", Status::Idle, 33)];
    assert_eq!(snap.sessions.len(), expected.len());
    for (s, (name, status, age)) in snap.sessions.iter().zip(expected) {
        assert_eq!(s.key, SessionKey { host: "archbox".into(), name: name.into() });
        assert_eq!((s.status, s.last_output_age_s), (status, Some(age)), "{name}");
        assert_eq!((s.tool, s.attached), (Tool::Claude, 0), "{name}");
        assert_eq!((&s.reason, &s.title, s.context_pct), (&None, &None, None), "{name}");
    }
}

#[test]
fn status_rules_match_tmls() {
    // busy Claude is running, however long ago its pane last printed
    let snap = parse("claude-busy.txt");
    let s = session(&snap, "work");
    assert_eq!((s.status, &s.reason, s.last_output_age_s), (Status::Running, &None, Some(600)));
    assert_eq!((s.tool, s.attached, s.context_pct), (Tool::Claude, 1, Some(58)));

    // a permission prompt: waiting on you, with what for
    let snap = parse("claude-waiting.txt");
    let s = session(&snap, "ask");
    assert_eq!((s.status, s.reason.as_deref()), (Status::Waiting, Some("permission prompt")));

    // Claude's newest reply is an API error; a new turn (busy) clears the mark
    let snap = parse("claude-api-error.txt");
    assert_eq!(session(&snap, "broke").status, Status::Error);
    assert_eq!(session(&snap, "broke").reason, None);
    assert_eq!(session(&snap, "retry").status, Status::Running);

    // no Claude: quiet for more than 30 s is idle (30 s exactly is too)
    let snap = parse("shell-quiet.txt");
    assert_eq!((session(&snap, "build").status, session(&snap, "build").last_output_age_s), (Status::Idle, Some(31)));
    assert_eq!(session(&snap, "edge").status, Status::Idle);
    assert_eq!(session(&snap, "build").tool, Tool::Other);

    // no Claude: printed less than 30 s ago is running; the newest window's output counts
    let snap = parse("shell-printing.txt");
    assert_eq!(names(&snap), ["build"]);
    assert_eq!((session(&snap, "build").status, session(&snap, "build").last_output_age_s), (Status::Running, Some(29)));
}

#[test]
fn claude_status_is_matched_by_tmux_session() {
    // test_hosts.py::test_parse_claude_status_by_tmux_session, plus "shell" (a monitor still runs)
    let out = "1000\nwork:1:0:990\nnotes:1:0:400\nplain:1:0:10\nmon:1:0:10\nredraw:1:0:999\n---\n\
               {\"status\":\"idle\",\"statusUpdatedAt\":500000,\"tmux\":\"notes:@2.%2\"}\n\
               {\"status\":\"busy\",\"statusUpdatedAt\":900000,\"tmux\":\"work:@1.%1\"}\n\
               {\"status\":\"idle\",\"statusUpdatedAt\":950000,\"tmux\":\"work:@3.%3\"}\n\
               {\"status\":\"shell\",\"statusUpdatedAt\":1,\"tmux\":\"mon:@4.%4\"}\n\
               {\"status\":\"idle\",\"statusUpdatedAt\":1,\"tmux\":\"redraw:@5.%5\"}\n\
               {\"status\":\"busy\",\"statusUpdatedAt\":1,\"tmux\":null}\n";
    let snap = parse_listing("nas", out);
    let got: Vec<_> = snap.sessions.iter().map(|s| (s.key.name.as_str(), s.status, s.tool)).collect();
    assert_eq!(
        got,
        [
            ("work", Status::Running, Tool::Claude), // two Claudes in one session: the running one wins
            ("notes", Status::Idle, Tool::Claude),
            ("plain", Status::Idle, Tool::Other),
            ("mon", Status::Running, Tool::Claude),
            ("redraw", Status::Idle, Tool::Claude), // an idle Claude's status line redraws: not work
        ]
    );
}

#[test]
fn waiting_beats_running_and_failed() {
    // test_hosts.py::test_parse_waiting_reason_and_failed_reply
    let out = "1000\nask:1:0:990\nbroke:1:0:990\nboth:1:0:990\n---\n\
               {\"status\":\"waiting\",\"waitingFor\":\"permission prompt\",\"statusUpdatedAt\":900000,\"tmux\":\"ask:@1.%1\"}\n\
               {\"status\":\"idle\",\"statusUpdatedAt\":900000,\"tmux\":\"broke:@2.%2\"}\nfailed\n\
               {\"status\":\"idle\",\"statusUpdatedAt\":950000,\"tmux\":\"both:@3.%3\"}\nfailed\n\
               {\"status\":\"waiting\",\"waitingFor\":\"input needed\",\"statusUpdatedAt\":940000,\"tmux\":\"both:@4.%4\"}\n";
    let snap = parse_listing("nas", out);
    let got: Vec<_> = snap.sessions.iter().map(|s| (s.key.name.as_str(), s.status, s.reason.as_deref())).collect();
    assert_eq!(
        got,
        [
            ("ask", Status::Waiting, Some("permission prompt")),
            ("broke", Status::Error, None),
            ("both", Status::Waiting, Some("input needed")),
        ]
    );
}

#[test]
fn claude_title_and_context_percent() {
    let snap = parse("claude-title-context.txt");
    let got: Vec<_> =
        snap.sessions.iter().map(|s| (s.key.name.as_str(), s.title.as_deref(), s.context_pct)).collect();
    assert_eq!(
        got,
        [
            ("Season-36", Some("NERSC_Training"), Some(58)), // Claude 5: 584 676 of 1M
            ("old", Some("Old model"), Some(75)),            // older models: 150 000 of 200k
            ("full", None, Some(100)),                       // never above 100
            ("fresh", None, None),                           // no reply yet: no model, no percent
            ("plain", None, None),                           // not Claude
        ]
    );
}

#[test]
fn context_limit_is_1m_from_claude_5_and_200k_before() {
    // test_hosts.py::test_context_limit_by_model
    let pct = |model: &str, used: u64| {
        let out = format!(
            "1000\nx:1:0:990\n---\n{{\"status\":\"idle\",\"tmux\":\"x:@1.%1\"}}\nusage \"model\":\"{model}\" \"input_tokens\":{used}\n"
        );
        parse_listing("h", &out).sessions[0].context_pct
    };
    assert_eq!(pct("claude-sonnet-5-5", 900_000), Some(90));
    assert_eq!(pct("claude-haiku-4-5-20251001", 150_000), Some(75));
    assert_eq!(pct("claude-opus-10", 500_000), Some(50));
    assert_eq!(pct("some-other-model", 100_000), Some(50));
    assert_eq!(pct("claude-opus-5-5", u64::MAX), Some(100));
}

#[test]
fn half_written_status_file_keeps_its_failed_and_usage_lines() {
    // B's file was caught mid-write; the "failed" and "usage" after it are B's, not A's
    let out = "1000\nA:1:0:990\nB:1:0:990\n---\n\
               {\"status\":\"idle\",\"statusUpdatedAt\":900000,\"tmux\":\"A:@1.%1\"}\n\
               {\"status\":\"idle\",\"statusUpdatedAt\":9\n\
               failed\n\
               usage \"model\":\"claude-haiku-4-5\" \"input_tokens\":190000\n";
    let snap = parse_listing("nas", out);
    let a = session(&snap, "A");
    assert_eq!((a.status, a.context_pct), (Status::Idle, None));
    assert_eq!((session(&snap, "B").status, session(&snap, "B").tool), (Status::Running, Tool::Other));
}

#[test]
fn blank_line_after_a_status_file_keeps_its_failed_line() {
    let out = "1000\nA:1:0:990\n---\n{\"status\":\"idle\",\"statusUpdatedAt\":900000,\"tmux\":\"A:@1.%1\"}\n\nfailed\n";
    assert_eq!(parse_listing("nas", out).sessions[0].status, Status::Error);
}

#[test]
fn tool_letters() {
    let snap = parse("tools.txt");
    let got: Vec<_> = snap.sessions.iter().map(|s| (s.key.name.as_str(), s.tool)).collect();
    assert_eq!(
        got,
        [
            ("work", Tool::Opencode),
            ("both", Tool::Codex),      // several agents in one session: claude, then codex, then opencode
            ("sh", Tool::Other),
            ("tool x", Tool::Other),    // a session really named "tool x" is still a session
            ("viaфайл", Tool::Claude), // Claude's session file says so, whatever the pane's command is called
        ]
    );
    assert_eq!(Tool::Claude.letter(), Some('C'));
    assert_eq!(Tool::Opencode.letter(), Some('O'));
    assert_eq!(Tool::Codex.letter(), Some('X'));
    assert_eq!(Tool::Other.letter(), None);
    let marks = [Status::Running, Status::Waiting, Status::Unseen, Status::Error, Status::Idle].map(Status::mark);
    assert_eq!(marks, ['●', '?', '◆', '✕', '○']);
}

#[test]
fn no_tmux_server_is_online_with_no_sessions() {
    for name in ["no-server-old.txt", "no-server-3.7.txt"] {
        let snap = parse(name);
        assert_eq!(snap, HostSnapshot { host: "nas".into(), online: true, error: None, sessions: vec![] }, "{name}");
    }
    // tmux is not installed there: nothing to list, and the snapshot says why
    let snap = parse("no-tmux.txt");
    assert!(!snap.online && snap.sessions.is_empty());
    assert_eq!(snap.error.as_deref(), Some("sh: 1: tmux: not found"));
}

#[test]
fn login_banner_lines_are_skipped() {
    // test_hosts.py::test_parse_skips_lines_that_are_not_windows: a banner's number isn't the clock
    let snap = parse("banner.txt");
    assert_eq!(names(&snap), ["work"]);
    assert_eq!(snap.sessions[0].last_output_age_s, Some(10));
}

#[test]
fn output_that_is_not_a_listing_is_offline() {
    // test_hosts.py::test_list_host_offline_when_output_is_garbage
    for out in ["", "garbage\n", "Welcome\nwork:1:0:990\n", "---\n", "\n---\n{}\n"] {
        let snap = parse_listing("box", out);
        assert!(!snap.online && snap.sessions.is_empty() && snap.error.is_some(), "{out:?}");
        assert_eq!(snap.host, "box");
    }
}

/// xorshift: the same "random" bytes on every run.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

#[test]
fn garbage_and_truncated_output_never_panics() {
    assert!(!parse_listing("h", "").online);
    for seed in 1..200u64 {
        let bytes = noise(seed, (seed as usize * 37) % 4096);
        let _ = parse_listing("h", &String::from_utf8_lossy(&bytes));
        // noise after a valid clock line reaches the window, tool and Claude branches
        let _ = parse_listing("h", &format!("1000\n{}\n---\n{}", String::from_utf8_lossy(&bytes), String::from_utf8_lossy(&bytes)));
    }
    // every fixture cut at every char boundary, mid-line and mid-JSON included
    for name in ["claude-title-context.txt", "names.txt", "tools.txt", "claude-api-error.txt", "no-server-3.7.txt"] {
        let text = fixture(name);
        for cut in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            let snap = parse_listing("h", &text[..cut]);
            assert!(snap.sessions.iter().all(|s| !s.key.name.is_empty() && s.key.host == "h"), "{name} cut at {cut}");
        }
    }
    // hostile values in every field
    let hostile = [
        "99999999999999999999999999\nwork:1:0:1\n",
        "1000\nwork:1:0:99999999999999999999999999\n",
        "1000\nwork:-1:-1:-1\n:::\n::::\n:1:0:5\ntool \ntool :\ntool :claude\ntmux-error \n",
        "5\nold:1:0:1000\n", // the window printed "after" the clock: age 0, not an underflow
        "1000\nw:1:0:990\n---\n[]\n42\nnull\n\"x\"\n{\"tmux\":7,\"status\":[],\"statusUpdatedAt\":\"x\"}\n\
         {\"tmux\":\"w:@1.%1\",\"status\":5,\"statusUpdatedAt\":1e400,\"name\":{},\"waitingFor\":3}\n\
         {\"tmux\":\"w:@1.%1\",\"status\":\"idle\",\"statusUpdatedAt\":-99999999999999999999}\n\
         usage \"model\":\"claude-opus-5\" \"input_tokens\":99999999999999999999999 \"cache_read_input_tokens\":18446744073709551615\n\
         usage\nusage \"model\nusage \"\":\"\"\nfailed\n",
        "1000\r\nwork:1:0:990\r\n---\r\n{\"status\":\"busy\",\"tmux\":\"work:@1.%1\"}\r\n",
        "1000\nw:1:0:990\n---",
    ];
    for out in hostile {
        let _ = parse_listing("h", out);
    }
    assert_eq!(parse_listing("h", hostile[3]).sessions[0].last_output_age_s, Some(0));
    assert_eq!(parse_listing("h", hostile[5]).sessions[0].status, Status::Running); // CRLF line ends
}

#[test]
fn names_with_spaces_colons_and_unicode() {
    let snap = parse("names.txt");
    assert_eq!(
        names(&snap),
        ["Agent Thing", "a:b:c", "café ☕ 日本語", "it's \"q\" $HOME `x` = . #", " lead and trail "]
    );
    let agent = session(&snap, "Agent Thing");
    assert_eq!(agent.key, SessionKey { host: "nas".into(), name: "Agent Thing".into() });
    assert_eq!((agent.status, agent.reason.as_deref(), agent.attached), (Status::Waiting, Some("input needed"), 2));
    assert_eq!(agent.tool, Tool::Claude);
    assert_eq!(session(&snap, "a:b:c").tool, Tool::Codex);
    let cafe = session(&snap, "café ☕ 日本語");
    assert_eq!((cafe.tool, cafe.title.as_deref()), (Tool::Claude, Some("カフェ")));
}

#[test]
fn listing_script_collects_what_tmls_collects() {
    // test_hosts.py::test_list_argv_reads_host_clock_windows_and_claude, ::test_listing_forces_utf8
    for part in [
        "date +%s",
        "tmux -u list-windows -a",
        "list-panes",
        ".claude/sessions/",
        ".claude/projects/",
        "\"isApiErrorMessage\":true",
        "echo usage",
        "#{pane_title}",
        "capture-pane -p",
    ] {
        assert!(LISTING_SCRIPT.contains(part), "the script lacks {part:?}");
    }
    assert!(!LISTING_SCRIPT.contains(".key"), "the .key files beside the session files are secrets");
}

#[test]
fn listing_command_is_sh_here_and_batch_ssh_elsewhere() {
    let argv = |cmd: std::process::Command| -> Vec<String> {
        std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(argv(listing_command("laptop", "laptop")), ["sh", "-s"]);
    assert_eq!(
        argv(listing_command("nas", "laptop")),
        [
            "ssh", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-o", "BatchMode=yes", "-o",
            "ConnectTimeout=5", "nas", "sh", "-s",
        ]
    );
}

/// A stub `tmux` on PATH: the script, run by a real `sh -s`, prints a listing the parser reads.
#[test]
fn script_output_parses_with_a_stub_tmux() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("listing-stub-{}", std::process::id()));
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let stub = dir.join("tmux");
    let run = |body: &str| -> (bool, HostSnapshot) {
        std::fs::write(&stub, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = listing_command("me", "me")
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
            .env("HOME", &home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(LISTING_SCRIPT.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        (out.status.success(), parse_listing("me", &String::from_utf8_lossy(&out.stdout)))
    };

    let (ok, snap) = run(
        "case \"$*\" in\n\
         *list-windows*) echo 'my notes:1:1:5'; echo 'work:2:0:6';;\n\
         *list-panes*) echo 'my notes:opencode:/dev/pts/998:%1'; echo 'work:vim:/dev/pts/999:%2';;\n\
         *display-message*%1*capture-pane*%1*) echo OpenCode; echo '  △ Permission required'; echo ---;;\n\
         *display-message*) echo \"unexpected $*\";;\n\
         esac",
    );
    assert!(ok && snap.online, "{snap:?}");
    let got: Vec<_> = snap.sessions.iter().map(|s| (s.key.name.as_str(), s.tool, s.attached, s.status)).collect();
    // opencode's pane asks for a permission; vim's pane is not captured at all
    assert_eq!(got, [("my notes", Tool::Opencode, 1, Status::Waiting), ("work", Tool::Other, 0, Status::Idle)]);

    let (ok, snap) = run("echo 'no server running on /tmp/tmux-1000/default' >&2; exit 1");
    assert!(ok, "the script itself succeeds when tmux has no server");
    assert_eq!(snap, HostSnapshot { host: "me".into(), online: true, error: None, sessions: vec![] });

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn claude_outside_tmux_is_listed_under_kitty() {
    // One Claude in tmux, then four status files with no tmux pane: named, unnamed (its folder
    // names it), the same name again (its process is added), and a helper that is not
    // interactive (left out). One without a pid cannot be found again: left out too.
    let output = concat!(
        "1000\nwork:1:0:990\ntool work:claude\n---\n",
        r#"{"pid":11,"kind":"interactive","status":"idle","statusUpdatedAt":900000,"name":"work","tmux":"work:@1.%1"}"#,
        "\n",
        r#"{"pid":21,"kind":"interactive","status":"waiting","waitingFor":"permission prompt","statusUpdatedAt":900000,"name":"notes","cwd":"/home/u/notes"}"#,
        "\nusage \"model\":\"claude-opus-5-5\" \"input_tokens\":250000\n",
        r#"{"pid":22,"kind":"interactive","status":"busy","statusUpdatedAt":900000,"cwd":"/home/u/code/thing/"}"#,
        "\n",
        r#"{"pid":23,"kind":"interactive","status":"idle","statusUpdatedAt":900000,"name":"notes","cwd":"/tmp"}"#,
        "\nfailed\n",
        r#"{"pid":24,"kind":"background","status":"busy","statusUpdatedAt":900000,"cwd":"/home/u"}"#,
        "\n",
        r#"{"kind":"interactive","status":"busy","statusUpdatedAt":900000,"cwd":"/home/u/nopid"}"#,
        "\n",
    );
    let snap = parse_listing("nas", output);
    assert_eq!(names(&snap), ["work", "notes", "thing", "notes (23)"]);
    let outside: Vec<&SessionInfo> = snap.sessions.iter().filter(|s| s.key.host == client_core::KITTY).collect();
    assert_eq!(outside.len(), 3, "the tmux session keeps its host");
    assert_eq!(session(&snap, "work").pid, None);
    let said: Vec<_> = outside.iter().map(|s| (s.status, s.pid, s.tool, s.context_pct)).collect();
    assert_eq!(
        said,
        [
            (Status::Waiting, Some(21), Tool::Claude, Some(25)),
            (Status::Running, Some(22), Tool::Claude, None),
            (Status::Error, Some(23), Tool::Claude, None),
        ]
    );
    assert_eq!(outside[0].reason.as_deref(), Some("permission prompt"));
    assert!(outside.iter().all(|s| s.title.is_none() && s.attached == 0 && s.last_output_age_s.is_none()));
}

/// A listing of one tmux session `work` whose Claude status file is `file`.
fn with_status_file(file: &str) -> HostSnapshot {
    parse_listing("nas", &format!("100\nwork:1:0:90\ntool work:claude\n---\n{file}\n"))
}

#[test]
fn a_status_files_session_id_is_kept_only_when_it_is_one() {
    let id = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";
    let snap = with_status_file(&format!(r#"{{"pid":7,"sessionId":"{id}","status":"idle","tmux":"work:@1.%1"}}"#));
    assert_eq!(session(&snap, "work").session_id.as_deref(), Some(id));
    assert_eq!(session(&snap, "work").tool, Tool::Claude);

    // Anything else is not spliced into a command later: it is not kept at all.
    for bad in [
        "$(reboot)",
        "",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4",   // 35
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e0", // 37
        "0B5C1D2E-3F4A-4B6C-8D7E-9F0A1B2C3D4E",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4*",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d/e",
        "0b5c1d2e 3f4a 4b6c 8d7e 9f0a1b2c3d4e",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4é",
    ] {
        let snap = with_status_file(&format!(r#"{{"pid":7,"sessionId":"{bad}","status":"idle","tmux":"work:@1.%1"}}"#));
        assert_eq!(session(&snap, "work").session_id, None, "{bad:?}");
        assert_eq!(session(&snap, "work").tool, Tool::Claude, "the file still counts: {bad:?}");
    }
    // Not a string, or not there.
    let snap = with_status_file(r#"{"pid":7,"sessionId":12,"status":"idle","tmux":"work:@1.%1"}"#);
    assert_eq!(session(&snap, "work").session_id, None);
    let snap = with_status_file(r#"{"pid":7,"status":"idle","tmux":"work:@1.%1"}"#);
    assert_eq!(session(&snap, "work").session_id, None);
    // A session that is not Claude has none.
    let snap = parse_listing("nas", "100\nwork:1:0:90\n---\n");
    assert_eq!(session(&snap, "work").session_id, None);
}

#[test]
fn a_claude_outside_tmux_keeps_its_session_id_too() {
    let id = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";
    let file = format!(r#"{{"pid":7,"kind":"interactive","sessionId":"{id}","status":"idle","cwd":"/w/notes"}}"#);
    let snap = parse_listing("nas", &format!("100\n---\n{file}\n"));
    assert_eq!(session(&snap, "notes").session_id.as_deref(), Some(id));
}

#[test]
fn a_claude_session_has_its_pane_and_its_status_time() {
    let snap = parse_listing(
        "nas",
        "100\nwork:1:0:90\nplain:1:0:42\n---\n{\"pid\":7,\"status\":\"idle\",\"statusUpdatedAt\":77000,\"tmux\":\"work:@1.%12\"}\n",
    );
    let work = session(&snap, "work");
    assert_eq!(work.pane.as_deref(), Some("%12"));
    // Claude's status time, not its window's: its status line repaints every minute.
    assert_eq!(work.activity, Some(77));
    let plain = session(&snap, "plain");
    assert_eq!((plain.pane.as_deref(), plain.activity), (None, Some(42)));
    // A pane that is not `%N` is not kept: it becomes part of a command later.
    for bad in ["work:@1.%1;x", "work:@1.%", "work", "work:@1.12"] {
        let snap = parse_listing("nas", &format!("100\nwork:1:0:90\n---\n{{\"status\":\"idle\",\"tmux\":\"{bad}\"}}\n"));
        assert_eq!(session(&snap, "work").pane, None, "{bad:?}");
    }
}

#[test]
fn claude_outside_tmux_has_no_pane_and_no_activity() {
    let pid = std::process::id();
    let snap = parse_listing(
        "me",
        &format!("100\n---\n{{\"pid\":{pid},\"kind\":\"interactive\",\"status\":\"idle\",\"statusUpdatedAt\":5000,\"name\":\"notes\"}}\n"),
    );
    let outside = &snap.sessions[0];
    assert_eq!((outside.pane.as_deref(), outside.activity), (None, None));
}

#[test]
fn other_tmux_servers_parse_into_one_snapshot_each() {
    let out = "Welcome to nas\n1000\nbackground web:\nsite:1:1:995\nsite:2:1:900\nbackground jobs:\ncron:1:0:100\n\
               background bad name:\nhidden:1:0:999\nbackground empty:\n";
    let snaps = parse_background("nas", out);
    let hosts: Vec<&str> = snaps.iter().map(|snap| snap.host.as_str()).collect();
    assert_eq!(hosts, ["nas#web", "nas#jobs", "nas#empty"]);
    assert!(snaps.iter().all(|snap| snap.online && snap.error.is_none()));
    let site = &snaps[0].sessions;
    assert_eq!(site.len(), 1, "one row per session, the newest window's output");
    let site = &site[0];
    assert_eq!(
        (site.key.host.as_str(), site.key.name.as_str(), site.status, site.attached, site.last_output_age_s),
        ("nas#web", "site", Status::Running, 1, Some(5))
    );
    assert_eq!((site.tool, site.title.as_deref(), site.session_id.as_deref()), (Tool::Other, Some("tmux -L web"), None));
    assert_eq!(snaps[1].sessions[0].status, Status::Idle);
    assert!(snaps[2].sessions.is_empty());
    assert!(parse_background("nas", "").is_empty());
    assert!(parse_background("nas", "background web:\nsite:1:0:5\n").is_empty(), "no clock, nothing");
}

#[test]
fn a_scan_for_other_servers_that_failed_is_no_answer() {
    let names = |ran: anyhow::Result<String>| -> Option<Vec<String>> {
        Some(background_sessions("nas", ran)?.into_iter().map(|s| format!("{}/{}", s.key.host, s.key.name)).collect())
    };
    let two = "1000\nbackground web:\nsite:1:1:995\nbackground jobs:\ncron:1:0:100\n";
    assert_eq!(names(Ok(two.to_string())), Some(vec!["nas#web/site".to_string(), "nas#jobs/cron".to_string()]));
    // The host answered and has none: its `background` row goes.
    assert_eq!(names(Ok("1000\n".to_string())), Some(Vec::new()));
    // An ssh hiccup or the timeout says nothing about its servers: the last answer stays.
    assert_eq!(names(Err(anyhow::anyhow!("ssh timed out after 10.0 s"))), None);
}

#[test]
fn a_background_host_is_machine_hash_server() {
    assert_eq!(split_host("nas#web"), ("nas", Some("web")));
    assert_eq!(split_host("nas"), ("nas", None));
    assert_eq!(split_host("nas#"), ("nas#", None));
    for good in ["web", "jobs.2", "a_b-c"] {
        assert!(valid_server_name(good), "{good}");
    }
    for bad in ["", "a b", "x;y", "$(id)", "日本", "a/b"] {
        assert!(!valid_server_name(bad), "{bad}");
    }
}

/// A listing of sessions whose panes run agents, each pane's capture (title, then screen) taken
/// from `tests/data/agent-screens`.
fn with_screens(panes: &[(&str, &str, &str)], claude: &str) -> HostSnapshot {
    let mut out = String::from("1000\n");
    let mut sessions: Vec<&str> = panes.iter().map(|(name, _, _)| *name).collect();
    sessions.dedup();
    for name in &sessions {
        out += &format!("{name}:1:0:10\n"); // printed 990 s ago: idle by its age
    }
    for (name, command, capture) in panes {
        out += &format!("tool {name}:{command}\n");
        if capture.is_empty() {
            continue;
        }
        let dir = format!("{}/tests/data/agent-screens", env!("CARGO_MANIFEST_DIR"));
        let title = std::fs::read_to_string(format!("{dir}/{capture}.title")).unwrap();
        let screen = std::fs::read_to_string(format!("{dir}/{capture}.screen")).unwrap();
        for line in title.lines().chain(screen.lines()) {
            out += &format!(">{line}\n");
        }
    }
    out += "---\n";
    out += claude;
    parse_listing("archbox", &out)
}

#[test]
fn agent_panes_get_their_status_from_their_screen() {
    let snap = with_screens(
        &[
            ("cx-idle", "codex", "codex-idle-after"),
            ("cx-work", "codex", "codex-working"),
            ("cx-ask", "codex", "codex-approval-blocked"),
            ("oc-idle", "opencode", "opencode-idle"),
            ("oc-work", "opencode", "opencode-working"),
            ("oc-ask", "opencode", "opencode-permission-blocked"),
            ("bare", "codex", ""), // no capture (the pane went away): its output age decides
        ],
        "",
    );
    let got: Vec<_> = snap.sessions.iter().map(|s| (s.key.name.as_str(), s.status, s.tool)).collect();
    assert_eq!(
        got,
        [
            ("cx-idle", Status::Idle, Tool::Codex),
            ("cx-work", Status::Running, Tool::Codex),
            ("cx-ask", Status::Waiting, Tool::Codex),
            ("oc-idle", Status::Idle, Tool::Opencode),
            ("oc-work", Status::Running, Tool::Opencode),
            ("oc-ask", Status::Waiting, Tool::Opencode),
            ("bare", Status::Idle, Tool::Codex),
        ]
    );
    assert!(snap.sessions.iter().all(|s| s.reason.is_none() && s.title.is_none()));
}

#[test]
fn several_agent_panes_in_a_session_waiting_wins_then_working() {
    let status = |panes: &[(&str, &str, &str)]| with_screens(panes, "").sessions[0].status;
    let idle = ("s", "opencode", "opencode-idle");
    let work = ("s", "codex", "codex-working");
    let ask = ("s", "opencode", "opencode-permission-blocked");
    assert_eq!(status(&[idle, work]), Status::Running);
    assert_eq!(status(&[work, ask, idle]), Status::Waiting);
    assert_eq!(status(&[idle, idle]), Status::Idle);
    assert_eq!(with_screens(&[idle, work], "").sessions[0].tool, Tool::Codex);
}

#[test]
fn claudes_status_file_beats_its_screen_which_is_only_a_fallback() {
    // Its status file says idle; a working codex in the same session does not change that.
    let file = r#"{"status":"idle","statusUpdatedAt":900000,"tmux":"s:@1.%1"}"#;
    let snap = with_screens(&[("s", "claude", ""), ("s", "codex", "codex-working")], file);
    assert_eq!((snap.sessions[0].status, snap.sessions[0].tool), (Status::Idle, Tool::Claude));
    // No status file names the pane (an old Claude Code): herdr's Claude rules read its screen.
    let mut out = String::from("1000\ns:1:0:10\ntool s:claude\n>✳ Claude Code\n");
    for line in ["╭───", "│ Do you want to proceed?", "│ ❯ 1. Yes", "│   2. No", "╰───"] {
        out += &format!(">{line}\n");
    }
    let snap = parse_listing("archbox", &(out + "---\n"));
    assert_eq!((snap.sessions[0].status, snap.sessions[0].tool), (Status::Waiting, Tool::Claude));
}

#[test]
fn screen_lines_are_never_windows_and_never_end_the_tmux_part() {
    // A screen showing "---", a line that looks like a window, and one like a tool line.
    let out = "1000\nw:1:0:990\ntool w:opencode\n>OpenCode\n>---\n>evil:1:1:999\n>tool w:claude\n\
               >  △ Permission required\n---\n";
    let snap = parse_listing("nas", out);
    assert_eq!(names(&snap), ["w"]);
    assert_eq!((snap.sessions[0].status, snap.sessions[0].tool), (Status::Waiting, Tool::Opencode));
    // `>` lines before any tool line are not screens (a banner, a session named so)
    let snap = parse_listing("nas", "1000\n>odd:1:0:990\n---\n");
    assert_eq!(names(&snap), [">odd"]);
    // `>` lines after a pane that is no agent's belong to nobody
    let snap = parse_listing("nas", "1000\nw:1:0:10\ntool w:vim\n>  △ Permission required\n---\n");
    assert_eq!(snap.sessions[0].status, Status::Idle);
}

#[test]
fn other_agents_herdr_knows_get_a_letter_and_a_status() {
    let out = "1000\ng:1:0:10\nk:1:0:10\ntool g:gemini\n>gemini\n>│ Allow execution of: 'rm'?\ntool k:kiro-cli\n---\n";
    let snap = parse_listing("nas", out);
    let got: Vec<_> = snap.sessions.iter().map(|s| (s.key.name.as_str(), s.status, s.tool, s.tool.letter())).collect();
    assert_eq!(
        got,
        [("g", Status::Waiting, Tool::Agent("gemini"), Some('G')), ("k", Status::Idle, Tool::Agent("kiro"), Some('K'))]
    );
}

#[test]
fn the_scripts_agent_names_are_the_rule_files_names() {
    let line = LISTING_SCRIPT.lines().find_map(|l| l.trim().strip_prefix("e='")).expect("the script names agents");
    let mut in_script: Vec<&str> = line.trim_end_matches('\'').split('|').collect();
    let mut in_files: Vec<String> = Vec::new();
    for (id, _) in client_core::detect::RULE_FILES {
        in_files.extend(client_core::detect::rules(id).unwrap().process_names().map(str::to_string));
    }
    in_script.sort_unstable();
    in_files.sort_unstable();
    assert_eq!(in_script, in_files);
}
