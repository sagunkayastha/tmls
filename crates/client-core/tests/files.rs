//! The builders that find and read a file on a session's host, as text and against real files.
//! No tmux and no ssh runs here: the remote command string is given to `sh` on this machine,
//! with `tmux` a shell function that prints the directory the test made.

use std::path::{Path, PathBuf};
use std::time::Duration;

use client_core::files::{Kind, Located, locate_argv, parse_located, read_argv};
use client_core::listing::SessionKey;
use client_core::tmux::{run, run_bytes};

/// This machine's name in every test; `ph` is a remote host.
const LOCAL: &str = "archbox";
const TIMEOUT: Duration = Duration::from_secs(10);

/// The words ssh puts before the command.
const SSH: [&str; 6] = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "ph"];

/// File names a shell would mangle, or take for an option, if a builder forgot to quote.
const HOSTILE: [&str; 8] =
    ["a b.txt", "it's.txt", "x\"y.txt", "$HOME.txt", "a;touch canary;.txt", "日本.txt", "-rf", "$(touch canary)"];

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey { host: host.to_string(), name: name.to_string() }
}

fn v(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// A fresh directory of this test's own, under cargo's target tmp dir.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the test's directory");
    dir
}

/// Runs the command a remote host's shell would get, here: `tmux` prints `cwd`, HOME is `home`.
fn as_remote(argv: &[String], cwd: &Path, home: &Path) -> anyhow::Result<String> {
    assert_eq!(&argv[..6], &v(&SSH)[..], "everything but the command is fixed");
    assert_eq!(argv.len(), 7, "the command is one argument");
    let stub = format!("tmux() {{ printf '%s\\n' '{}'; }}\nHOME='{}'\n", cwd.display(), home.display());
    run(&v(&["sh", "-c", &format!("{stub}{}", argv[6])]), TIMEOUT)
}

#[test]
fn locate_argv_is_a_script_here_and_one_ssh_argument_elsewhere() {
    let script = "p='/var/log/x.log'; [ -e \"$p\" ] || { echo 'not found' >&2; exit 1; }; \
                  if [ -d \"$p\" ]; then echo d; echo 0; elif [ -f \"$p\" ]; then echo f; wc -c <\"$p\" || exit 1; \
                  else echo o; echo 0; fi; printf %s \"$p\"";
    assert_eq!(locate_argv(&key(LOCAL, "work"), "/var/log/x.log", LOCAL), v(&["sh", "-c", script]));
    let mut remote = v(&SSH);
    remote.push(script.to_string());
    assert_eq!(locate_argv(&key("ph", "work"), "/var/log/x.log", LOCAL), remote);
}

#[test]
fn home_stays_outside_the_quotes_and_a_relative_path_asks_the_sessions_pane() {
    let script = |path: &str| locate_argv(&key("ph", "it's mine"), path, LOCAL).pop().unwrap();
    assert!(script("~/notes/a b.md").starts_with("p=\"$HOME\"/'notes/a b.md'; "));
    assert!(script("~").starts_with("p=\"$HOME\"; "));
    // `~name` is not expanded (tmls: only `~/`): it is a name in the pane's directory.
    for path in ["src/main.rs", "~bob/x", "-rf", "./a"] {
        let script = script(path);
        let asks = "d=$(tmux display-message -p -t '=it'\\''s mine:' '#{pane_current_path}') || exit 1; ";
        assert!(script.starts_with(asks), "{path}: {script}");
        assert!(script.contains(&format!("p=\"$d\"/'{path}'; ")), "{path}: {script}");
    }
}

#[test]
fn read_argv_takes_the_path_after_two_dashes() {
    assert_eq!(read_argv(LOCAL, "/tmp/-x", 2_000_001, LOCAL), v(&["head", "-c", "2000001", "--", "/tmp/-x"]));
    let mut remote = v(&SSH);
    remote.push("head -c 25000001 -- '/srv/it'\\''s here.png'".to_string());
    assert_eq!(read_argv("ph", "/srv/it's here.png", 25_000_001, LOCAL), remote);
}

#[test]
fn parse_located_reads_kind_size_and_path() {
    let file = |path: &str, size| Some(Located { path: path.to_string(), kind: Kind::File, size });
    assert_eq!(parse_located("f\n120\n/a/b.rs"), file("/a/b.rs", 120));
    // BSD's wc pads the number.
    assert_eq!(parse_located("f\n     120\n/a/b.rs"), file("/a/b.rs", 120));
    // The path is everything after the second line, as it is.
    assert_eq!(parse_located("f\n0\n/a/two\nlines "), file("/a/two\nlines ", 0));
    assert_eq!(parse_located("d\n0\n/a").map(|l| l.kind), Some(Kind::Dir));
    assert_eq!(parse_located("o\n0\n/dev/null").map(|l| l.kind), Some(Kind::Other));
    for bad in ["", "f", "f\n12", "x\n1\n/a", "f\nmany\n/a", "f\n1\nrelative", "f\n1\n", "f\n-1\n/a"] {
        assert_eq!(parse_located(bad), None, "{bad:?}");
    }
}

#[test]
fn the_local_script_finds_files_folders_and_nothing() {
    let dir = scratch("local");
    std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    let at = |name: &str| dir.join(name).to_str().unwrap().to_string();
    let locate = |path: &str| run(&locate_argv(&key(LOCAL, "work"), path, LOCAL), TIMEOUT);

    let found = parse_located(&locate(&at("a.txt")).unwrap()).unwrap();
    assert_eq!(found, Located { path: at("a.txt"), kind: Kind::File, size: 6 });
    assert_eq!(parse_located(&locate(&at("sub")).unwrap()).unwrap().kind, Kind::Dir);
    assert_eq!(parse_located(&locate("/dev/null").unwrap()).unwrap().kind, Kind::Other);
    let missing = locate(&at("nope.txt")).expect_err("nothing is there");
    assert_eq!(format!("{missing:#}"), "not found");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_names_are_one_word_of_the_remote_command() {
    let dir = scratch("hostile");
    let (cwd, home) = (dir.join("cwd"), dir.join("home"));
    for (n, name) in HOSTILE.iter().enumerate() {
        for base in [&cwd, &home, &dir.join("abs")] {
            std::fs::create_dir_all(base).unwrap();
            std::fs::write(base.join(name), "x".repeat(n + 1)).unwrap();
        }
    }
    let session = key("ph", "tmlsrs-test-a b'c");
    for (n, name) in HOSTILE.iter().enumerate() {
        let size = n as u64 + 1;
        // Relative to the pane's directory, under the home, and absolute.
        let cases = [
            (name.to_string(), cwd.join(name)),
            (format!("~/{name}"), home.join(name)),
            (dir.join("abs").join(name).to_str().unwrap().to_string(), dir.join("abs").join(name)),
        ];
        for (asked, expected) in cases {
            let out = as_remote(&locate_argv(&session, &asked, LOCAL), &cwd, &home)
                .unwrap_or_else(|e| panic!("{asked:?}: {e:#}"));
            let located = parse_located(&out).unwrap_or_else(|| panic!("{asked:?}: {out:?}"));
            let expected = expected.to_str().unwrap().to_string();
            assert_eq!(located, Located { path: expected.clone(), kind: Kind::File, size }, "{asked:?}");
            // The path it gave reads the file, here and through the remote command.
            let local = run_bytes(&read_argv(LOCAL, &expected, 100, LOCAL), TIMEOUT).unwrap();
            assert_eq!(local.len() as u64, size, "{asked:?}");
            let remote = as_remote(&read_argv("ph", &expected, 100, LOCAL), &cwd, &home).unwrap();
            assert_eq!(remote.len() as u64, size, "{asked:?}");
        }
    }
    // Nothing in a name ran: no `canary` anywhere, and this process's directory has none either.
    for base in [&cwd, &home, &dir.join("abs"), &dir, &std::env::current_dir().unwrap()] {
        assert!(!base.join("canary").exists(), "a name was run as a command in {base:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_stops_at_the_cap_and_keeps_bytes_that_are_not_text() {
    let dir = scratch("read");
    let path = dir.join("-blob");
    let bytes: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    std::fs::write(&path, &bytes).unwrap();
    let path = path.to_str().unwrap();
    assert_eq!(run_bytes(&read_argv(LOCAL, path, 10_000, LOCAL), TIMEOUT).unwrap(), bytes);
    assert_eq!(run_bytes(&read_argv(LOCAL, path, 1001, LOCAL), TIMEOUT).unwrap(), &bytes[..1001]);
    let missing = run_bytes(&read_argv(LOCAL, "/nonexistent/tmlsrs-test", 10, LOCAL), TIMEOUT);
    assert!(missing.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unreadable_file_and_a_pane_without_a_directory_are_errors() {
    let dir = scratch("errors");
    let (cwd, home) = (dir.join("cwd"), dir.join("home"));
    std::fs::create_dir_all(&cwd).unwrap();
    // tmux answers nothing: the path is not looked for at the root.
    let argv = locate_argv(&key("ph", "work"), "etc/passwd", LOCAL);
    let stub = "tmux() { :; }\n";
    let error = run(&v(&["sh", "-c", &format!("{stub}{}", argv[6])]), TIMEOUT).expect_err("no directory");
    assert_eq!(format!("{error:#}"), "the session has no current directory");
    // tmux fails (no such session): so does the command.
    let stub = "tmux() { echo \"can't find session\" >&2; return 1; }\n";
    let error = run(&v(&["sh", "-c", &format!("{stub}{}", argv[6])]), TIMEOUT).expect_err("no session");
    assert_eq!(format!("{error:#}"), "can't find session");
    assert_eq!(format!("{:#}", as_remote(&argv, &cwd, &home).expect_err("nothing there")), "not found");
    let _ = std::fs::remove_dir_all(&dir);
}
