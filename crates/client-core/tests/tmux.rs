//! The tmux command builders, as text: exact argv for this machine and for a remote host, and
//! what a shell makes of the remote command strings. No tmux and no ssh runs here: where a
//! command string is given to `sh`, `tmux`, `cd`, `git`, `ssh` and `claude` are shell functions
//! that print their arguments.

use std::process::Command;
use std::time::{Duration, Instant};

use client_core::listing::SessionKey;
use client_core::tmux::{
    MAX_NAME_CHARS, NewSession, Start, attach_argv, attach_text, derived_name_argv, kill_argv, new_session_argv,
    rename_argv, run, running_commands_argv, valid_session_name,
};

/// This machine's name in every test; `ph` is a remote host.
const LOCAL: &str = "archbox";

/// Names a shell would mangle if any builder forgot to quote.
const HOSTILE: [&str; 7] = ["a b", "it's", "x\"y", "$HOME", "a;rm", "日本", "-lead"];

/// The words ssh puts before the command for everything but attach.
const SSH: [&str; 6] = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "ph"];

/// In place of the real programs: each prints its arguments, one per line. `has-session` says
/// "no such session", so a new-session script goes on.
const STUBS: &str = r#"p() { printf '%s\n' "$@"; }
tmux() { p "$@"; [ "$1" != has-session ]; }
cd() { p cd "$@"; }
ssh() { p "$@"; }
claude() { p "$@"; }
git() { return 1; }
"#;

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey { host: host.to_string(), name: name.to_string() }
}

fn v(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// The command string of a remote argv, after checking the words before it.
fn remote(argv: &[String], prefix: &[&str]) -> String {
    let (command, before) = argv.split_last().expect("an argv");
    assert_eq!(before, v(prefix), "the words before the remote command");
    command.clone()
}

/// What a shell makes of `command`: the arguments the stubbed programs got, one per line.
fn words(command: &str) -> Vec<String> {
    let out = Command::new("sh")
        .args(["-c", &format!("{STUBS}{command}")])
        .env("HOME", "/home/u")
        .current_dir("/")
        .output()
        .expect("running sh");
    assert!(out.stderr.is_empty(), "sh complained about {command:?}: {}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{command:?} failed: {}", String::from_utf8_lossy(&out.stdout));
    String::from_utf8(out.stdout).expect("utf-8").lines().map(str::to_string).collect()
}

fn new_session(host: &str, folder: &str, name: &str, start: Start, size: Option<(u16, u16)>) -> NewSession {
    NewSession { host: host.to_string(), folder: folder.to_string(), name: name.to_string(), start, size }
}

#[test]
fn attach_argv_local_and_remote() {
    assert_eq!(attach_argv(&key(LOCAL, "work"), LOCAL), v(&["tmux", "-u", "attach", "-t", "=work"]));
    assert_eq!(
        attach_argv(&key(LOCAL, "Agent Thing"), LOCAL),
        v(&["tmux", "-u", "attach", "-t", "=Agent Thing"]),
        "an argv needs no quoting"
    );
    assert_eq!(attach_argv(&key("ph", "work"), LOCAL), v(&["ssh", "-t", "ph", "tmux -u attach -t '=work'"]));
    assert_eq!(
        attach_argv(&key("ph", "Agent Thing"), LOCAL),
        v(&["ssh", "-t", "ph", "tmux -u attach -t '=Agent Thing'"])
    );
}

#[test]
fn attach_text_is_what_tmls_copies() {
    // the values of tmls's hosts.attach_command (shlex.join of the attach argv)
    assert_eq!(attach_text(&key(LOCAL, "work"), LOCAL), "tmux -u attach -t =work");
    assert_eq!(attach_text(&key(LOCAL, "Agent Thing"), LOCAL), "tmux -u attach -t '=Agent Thing'");
    assert_eq!(attach_text(&key("ph", "work"), LOCAL), "ssh -t ph 'tmux -u attach -t =work'");
    assert_eq!(
        attach_text(&key("ph", "Agent Thing"), LOCAL),
        r#"ssh -t ph 'tmux -u attach -t '"'"'=Agent Thing'"'"''"#
    );
    assert_eq!(attach_text(&key(LOCAL, "it's"), LOCAL), r#"tmux -u attach -t '=it'"'"'s'"#);
}

#[test]
fn rename_argv_local_and_remote() {
    assert_eq!(
        rename_argv(&key(LOCAL, "old one"), "new one", LOCAL),
        v(&["tmux", "rename-session", "-t", "=old one", "--", "new one"])
    );
    let argv = rename_argv(&key("ph", "old one"), "new one", LOCAL);
    assert_eq!(remote(&argv, &SSH), "tmux rename-session -t '=old one' -- 'new one'");
}

#[test]
fn kill_argv_local_and_remote() {
    assert_eq!(kill_argv(&key(LOCAL, "work"), LOCAL), v(&["tmux", "kill-session", "-t", "=work"]));
    assert_eq!(remote(&kill_argv(&key("ph", "work"), LOCAL), &SSH), "tmux kill-session -t '=work'");
}

#[test]
fn new_session_argv_local_and_remote() {
    let shell = "cd -- \"$HOME\"/'src/app' 2>/dev/null || { echo 'no such folder: ~/src/app'; exit 3; }; \
                 tmux has-session -t '=app' 2>/dev/null && { echo 'a session named app already exists'; exit 3; }; \
                 tmux new-session -d -s 'app' -c \"$PWD\"";
    let local = new_session_argv(&new_session(LOCAL, "~/src/app", "app", Start::Shell, None), LOCAL);
    assert_eq!(local, v(&["sh", "-c", shell]));
    let on_ph = new_session_argv(&new_session("ph", "~/src/app", "app", Start::Shell, None), LOCAL);
    assert_eq!(remote(&on_ph, &SSH), shell);

    let sized = new_session_argv(&new_session(LOCAL, "~/src/app", "app", Start::Shell, Some((132, 43))), LOCAL);
    assert_eq!(sized, v(&["sh", "-c", &format!("{shell} -x 132 -y 43")]));

    let claude = new_session_argv(&new_session("ph", "~/src/app", "app", Start::Claude, Some((132, 43))), LOCAL);
    assert_eq!(
        remote(&claude, &SSH),
        format!("{shell} -x 132 -y 43; tmux send-keys -t '=app:' 'claude --name app' Enter")
    );
}

#[test]
fn new_session_folders() {
    let cd = |folder: &str| {
        let argv = new_session_argv(&new_session(LOCAL, folder, "s", Start::Shell, None), LOCAL);
        words(&argv[2])[..3].to_vec()
    };
    assert_eq!(cd("~"), v(&["cd", "--", "/home/u"]));
    assert_eq!(cd(""), v(&["cd", "--", "/home/u"]), "no folder is the home folder");
    assert_eq!(cd("~/a b"), v(&["cd", "--", "/home/u/a b"]));
    assert_eq!(cd("/srv/it's"), v(&["cd", "--", "/srv/it's"]));
    assert_eq!(cd("~x/$HOME"), v(&["cd", "--", "~x/$HOME"]), "only a leading ~/ is the home folder");
    assert_eq!(cd("-p"), v(&["cd", "--", "-p"]));
}

#[test]
fn new_session_start_commands() {
    let typed = |start: Start| {
        let argv = new_session_argv(&new_session(LOCAL, "~", "my app", start, None), LOCAL);
        let said = words(&argv[2]);
        let at = said.iter().position(|w| w == "send-keys")?;
        assert_eq!(said[at + 1..at + 3], v(&["-t", "=my app:"]));
        assert_eq!(said[at + 4..], v(&["Enter"]));
        Some(said[at + 3].clone())
    };
    let preset = |argv: &[&str]| Start::Preset(v(argv));
    assert_eq!(typed(Start::Shell), None, "a shell session gets no keys");
    assert_eq!(typed(preset(&[])), None, "an empty preset is a shell");
    assert_eq!(typed(Start::Claude).as_deref(), Some("claude --name 'my app'"));
    assert_eq!(
        typed(preset(&["claude", "--model", "opus"])).as_deref(),
        Some("claude --model opus --name 'my app'")
    );
    assert_eq!(
        typed(preset(&["/opt/bin/claude"])).as_deref(),
        Some("/opt/bin/claude --name 'my app'"),
        "claude by path is claude"
    );
    for own in [&["claude", "--name", "x"][..], &["claude", "-n", "x"], &["claude", "--name=x"]] {
        assert_eq!(typed(preset(own)), Some(own.join(" ")), "a preset that names itself keeps its name");
    }
    assert_eq!(typed(preset(&["codex", "--full auto"])).as_deref(), Some("codex '--full auto'"));
}

#[test]
fn derived_name_argv_local_and_remote() {
    let script = "if t=$(git -C \"$HOME\"/'src/app' rev-parse --show-toplevel 2>/dev/null); then n=${t##*/}; \
                  [ -n \"$n\" ] || n=home; else n='app'; fi; printf '%s\\n' \"$n\"";
    assert_eq!(derived_name_argv(LOCAL, "~/src/app", LOCAL), v(&["sh", "-c", script]));
    assert_eq!(remote(&derived_name_argv("ph", "~/src/app", LOCAL), &SSH), script);
}

#[test]
fn derived_name_is_the_repository_else_the_folder() {
    let scratch = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tmux-derived-{}", std::process::id()));
    let repo = scratch.join("my repo");
    let inside = repo.join("src/deep");
    std::fs::create_dir_all(&inside).unwrap();
    let git = Command::new("git").args(["init", "-q"]).current_dir(&repo).output().expect("running git");
    assert!(git.status.success(), "git init: {}", String::from_utf8_lossy(&git.stderr));

    let name = |folder: &str| {
        run(&derived_name_argv(LOCAL, folder, LOCAL), Duration::from_secs(10)).expect("the name command")
    };
    assert_eq!(name(inside.to_str().unwrap()), "my repo\n", "in a repository: the repository's name");
    assert_eq!(name("/tmlsrs-no-such-folder/it's here/"), "it's here\n", "elsewhere: the folder's name");
    assert_eq!(name("/"), "home\n");
}

#[test]
fn derived_name_of_home_is_home() {
    // with git stubbed out ("not a repository"), whatever the folder is
    let name = |folder: &str| words(&derived_name_argv(LOCAL, folder, LOCAL)[2]);
    assert_eq!(name("~"), v(&["home"]));
    assert_eq!(name("~/"), v(&["home"]));
    assert_eq!(name(""), v(&["home"]));
    assert_eq!(name("~/src/app/"), v(&["app"]));
    assert_eq!(name("relative"), v(&["relative"]));
}

#[test]
fn attach_and_ops_quote_hostile_names() {
    for name in HOSTILE {
        let exact = format!("={name}");
        let other = format!("{name} 2");

        let attach = remote(&attach_argv(&key("ph", name), LOCAL), &["ssh", "-t", "ph"]);
        assert_eq!(words(&attach), v(&["-u", "attach", "-t", &exact]), "attach {name:?}");

        // the copied text, pasted into a shell: ssh gets the command, the host's shell the name
        let pasted = words(&attach_text(&key("ph", name), LOCAL));
        assert_eq!(pasted[..2], v(&["-t", "ph"]), "copied text for {name:?}");
        assert_eq!(pasted.len(), 3, "copied text for {name:?}: {pasted:?}");
        assert_eq!(words(&pasted[2]), v(&["-u", "attach", "-t", &exact]), "copied text for {name:?}");
        assert_eq!(
            words(&attach_text(&key(LOCAL, name), LOCAL)),
            v(&["-u", "attach", "-t", &exact]),
            "local copied text for {name:?}"
        );

        let kill = remote(&kill_argv(&key("ph", name), LOCAL), &SSH);
        assert_eq!(words(&kill), v(&["kill-session", "-t", &exact]), "kill {name:?}");

        let running = remote(&running_commands_argv(&key("ph", name), LOCAL), &SSH);
        assert_eq!(
            words(&running),
            v(&["list-panes", "-s", "-t", &exact, "-F", "#{pane_current_command}"]),
            "running commands of {name:?}"
        );
        assert_eq!(
            running_commands_argv(&key(LOCAL, name), LOCAL),
            v(&["tmux", "list-panes", "-s", "-t", &exact, "-F", "#{pane_current_command}"])
        );

        let rename = remote(&rename_argv(&key("ph", name), &other, LOCAL), &SSH);
        assert_eq!(words(&rename), v(&["rename-session", "-t", &exact, "--", &other]), "rename {name:?}");

        // the name as the folder too
        let folder = format!("/srv/{name}");
        let home_folder = format!("~/{name}");
        for (host, argv_at) in [("ph", SSH.len()), (LOCAL, 2)] {
            let create = new_session_argv(&new_session(host, &folder, name, Start::Claude, None), LOCAL);
            let said = words(&create[argv_at]);
            assert_eq!(said[..3], v(&["cd", "--", &folder]), "new session in {folder:?} on {host}");
            assert_eq!(said[3..6], v(&["has-session", "-t", &exact]), "new session {name:?} on {host}");
            assert_eq!(said[6..12], v(&["new-session", "-d", "-s", name, "-c", "/"]), "new session {name:?}");
            assert_eq!(said[12..15], v(&["send-keys", "-t", &format!("{exact}:")]), "keys for {name:?} on {host}");
            // the typed line, read by the session's shell
            assert_eq!(words(&said[15]), v(&["--name", name]), "claude's name for {name:?} on {host}");
            assert_eq!(said[16..], v(&["Enter"]));

            let derived = derived_name_argv(host, &home_folder, LOCAL);
            assert_eq!(words(&derived[argv_at]), v(&[name]), "derived name of {home_folder:?} on {host}");
        }
    }
}

#[test]
fn new_session_messages_quote_hostile_names() {
    // the folder does not exist: the script says so and stops, whatever the folder is called
    for name in HOSTILE {
        let folder = format!("/tmlsrs-no-such-folder/{name}");
        let argv = new_session_argv(&new_session(LOCAL, &folder, name, Start::Shell, None), LOCAL);
        let script = argv[2].replace("tmux ", "false "); // nothing after the cd may reach tmux
        let error = run(&v(&["sh", "-c", &script]), Duration::from_secs(10)).expect_err("no such folder");
        assert_eq!(error.to_string(), format!("no such folder: {folder}"));
    }
}

#[test]
fn valid_session_name_rejects_what_tmux_rejects() {
    for good in ["work", "Agent Thing", "a b'c", "x\"y", "$HOME", "a;rm", "日本", "a=b", "tmlsrs-test-a b'c"] {
        assert_eq!(valid_session_name(good), Ok(()), "{good:?}");
    }
    assert_eq!(valid_session_name(""), Err("the session needs a name".to_string()));
    for bad in ["a:b", "a.b", ".", ":"] {
        assert_eq!(valid_session_name(bad), Err("names can't contain : or .".to_string()), "{bad:?}");
    }
    assert_eq!(valid_session_name("#{pane_id}"), Err("names can't contain #".to_string()));
    assert_eq!(valid_session_name("-t"), Err("names can't start with -".to_string()));
    for bad in ["a\nb", "a\0b", "tab\there", "\u{1b}[31m"] {
        assert_eq!(valid_session_name(bad), Err("names can't contain control characters".to_string()), "{bad:?}");
    }
    assert_eq!(valid_session_name(&"日".repeat(MAX_NAME_CHARS)), Ok(()), "the limit counts characters");
    assert_eq!(
        valid_session_name(&"x".repeat(MAX_NAME_CHARS + 1)),
        Err(format!("names can't be longer than {MAX_NAME_CHARS} characters"))
    );
}

#[test]
fn run_returns_stdout_and_puts_stderr_in_the_error() {
    let sh = |script: &str| run(&v(&["sh", "-c", script]), Duration::from_secs(10));
    assert_eq!(sh("echo out; echo err >&2").unwrap(), "out\n");
    assert_eq!(sh("echo out; echo ' why it failed ' >&2; exit 1").unwrap_err().to_string(), "why it failed");
    assert_eq!(sh("echo only on stdout; exit 3").unwrap_err().to_string(), "only on stdout");
    assert!(sh("exit 7").unwrap_err().to_string().contains("sh failed"), "a silent failure names the program");
    assert_eq!(sh("cat").unwrap(), "", "the command's stdin is empty");
    assert!(run(&[], Duration::from_secs(1)).is_err());
    let missing = run(&v(&["/tmlsrs-no-such-program"]), Duration::from_secs(1)).unwrap_err();
    assert!(missing.to_string().contains("couldn't start /tmlsrs-no-such-program"), "{missing}");
}

#[test]
fn run_keeps_output_when_a_pipe_stays_open() {
    // the background sleep inherits stdout and stderr and holds them after the shell is gone,
    // as a persisting ssh connection master does
    let sh = |script: &str| {
        let started = Instant::now();
        let result = run(&v(&["sh", "-c", script]), Duration::from_secs(10));
        assert!(started.elapsed() < Duration::from_secs(2), "returned after {:?}", started.elapsed());
        result
    };
    assert_eq!(sh("echo out; echo err >&2; sleep 3 &").unwrap(), "out\n");
    assert_eq!(sh("echo out; echo err >&2; sleep 3 & exit 1").unwrap_err().to_string(), "err");
}

#[test]
fn run_kills_a_command_that_takes_too_long() {
    let marker = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tmux-run-{}", std::process::id()));
    let started = Instant::now();
    // the sleep is a child of the shell: killing only the shell would leave it to write the marker
    let script = format!("(sleep 1; echo late > '{}') & wait", marker.display());
    let error = run(&v(&["sh", "-c", &script]), Duration::from_millis(200)).unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(started.elapsed() < Duration::from_millis(900), "returned after {:?}", started.elapsed());
    std::thread::sleep(Duration::from_millis(1300));
    assert!(!marker.exists(), "what the command started outlived the timeout");
}
