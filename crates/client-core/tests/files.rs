//! The builders that find and read a file on a session's host, as text and against real files.
//! No tmux and no ssh runs here: the remote command string is given to `sh` on this machine,
//! with `tmux` a shell function that prints the directory the test made.

use std::path::{Path, PathBuf};
use std::time::Duration;

use client_core::files::{
    Kind, Located, MADE_MAX, Made, NoTranscript, local_time, locate_argv, made_argv, parse_located, parse_made,
    read_argv, valid_session_id,
};
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

// --- the images a Claude session made -------------------------------------------------------------

/// A session id as Claude Code writes them.
const ID: &str = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";

fn made(path: &str, modified_s: u64, bytes: u64) -> Made {
    Made { path: path.to_string(), modified_s, bytes }
}

#[test]
fn parse_made_keeps_the_order_and_skips_what_is_no_line_of_the_list() {
    let out = "1760000300\t12\t/w/plots/c.png\n1760000200\t3456\t/w/b.jpg\n1760000100\t0\t/tmp/a.webp\n";
    let three = vec![
        made("/w/plots/c.png", 1_760_000_300, 12),
        made("/w/b.jpg", 1_760_000_200, 3456),
        made("/tmp/a.webp", 1_760_000_100, 0),
    ];
    assert_eq!(parse_made(out), Ok(three.clone()));
    // A tab missing, a number that is none, a path that is not absolute, a login script's line.
    let noisy = "Welcome to nas\n1760000300\t12\t/w/plots/c.png\n1760000250 77 /w/no-tabs.png\n\
                 1760000240\t/w/one-tab.png\nyesterday\t5\t/w/x.png\n1760000230\tbig\t/w/x.png\n\
                 1760000220\t5\trelative.png\n1760000200\t3456\t/w/b.jpg\n\n1760000100\t0\t/tmp/a.webp";
    assert_eq!(parse_made(noisy), Ok(three));
    assert_eq!(parse_made(""), Ok(Vec::new()));
    assert_eq!(parse_made("no-transcript\n"), Err(NoTranscript));
    assert_eq!(parse_made("Welcome to nas\nno-transcript\n"), Err(NoTranscript));
}

#[test]
fn parse_made_takes_no_more_than_the_cap() {
    let out: String = (0..3 * MADE_MAX).map(|n| format!("{}\t1\t/w/{n}.png\n", 2_000_000_000 - n)).collect();
    let files = parse_made(&out).unwrap();
    assert_eq!(files.len(), MADE_MAX);
    assert_eq!((files[0].path.as_str(), files[MADE_MAX - 1].path.as_str()), ("/w/0.png", "/w/499.png"));
}

#[test]
fn made_argv_is_a_script_here_and_one_ssh_argument_elsewhere() {
    let script = "f=$(ls -t \"$HOME\"/.claude/projects/*/0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e.jsonl 2>/dev/null \
                  | head -n 1)\n\
                  [ -n \"$f\" ] || { echo \"no-transcript\"; exit 0; }\n\
                  grep -aoE '/[A-Za-z0-9_.~+@%=,/-]+\\.(png|jpe?g|gif|webp|bmp)' \"$f\" | sort -u |\n\
                  while IFS= read -r p; do\n  [ -f \"$p\" ] && stat -c '%Y\t%s\t%n' -- \"$p\"\n\
                  done | sort -rn | head -n 500\n";
    assert_eq!(made_argv(LOCAL, ID, None, LOCAL), v(&["sh", "-c", script]));
    let mut remote = v(&SSH);
    remote.push(script.to_string());
    assert_eq!(made_argv("ph", ID, None, LOCAL), remote);
    // The session's directory is not part of the command, whatever it is called.
    assert_eq!(made_argv("ph", ID, Some("/w/$(touch canary)/it's"), LOCAL), remote);
    assert!(script.contains(&format!("head -n {MADE_MAX}")));
}

#[test]
fn only_a_session_id_reaches_the_script() {
    assert!(valid_session_id(ID));
    assert!(valid_session_id("00000000-0000-0000-0000-000000000000"));
    let hostile = [
        "$(reboot)",
        "",
        "x",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e0",
        "0B5C1D2E-3F4A-4B6C-8D7E-9F0A1B2C3D4E",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d;e",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d e",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d'e",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d*e",
        "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d\ne",
        "../../../../../../../../etc/passwd.x",
        "$(touch /nonexistent/tmlsrs-canary)x",
    ];
    for id in hostile {
        assert!(!valid_session_id(id), "{id:?}");
        // Refused: the command that is built names no transcript and has nothing of the id.
        for host in [LOCAL, "ph"] {
            let argv = made_argv(host, id, None, LOCAL);
            let script = argv.last().unwrap();
            assert_eq!(script, "echo no-transcript", "{id:?} on {host}");
            assert_eq!(parse_made(&run(&v(&["sh", "-c", script]), TIMEOUT).unwrap()), Err(NoTranscript));
        }
    }
}

/// A directory of this test's own for a home: under `/data/scratch/tmls-rs-v0/panel2/tmp` on the
/// machine these are written on (its `/tmp` is small), else under cargo's target tmp dir. Never
/// the user's home: the script reads `$HOME/.claude`. It is left where it is.
fn fake_home(name: &str) -> PathBuf {
    let scratch = Path::new("/data/scratch/tmls-rs-v0/panel2/tmp");
    let base = if scratch.is_dir() { scratch.to_path_buf() } else { PathBuf::from(env!("CARGO_TARGET_TMPDIR")) };
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = base.join(format!("made-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the test's home");
    dir
}

/// Runs the script of `argv` (built for this machine) with `home` as HOME.
fn with_home(argv: &[String], home: &Path) -> anyhow::Result<String> {
    assert_eq!(&argv[..2], &v(&["sh", "-c"])[..]);
    assert_eq!(argv.len(), 3);
    assert!(!home.to_str().unwrap().contains('\''));
    run(&v(&["sh", "-c", &format!("HOME='{}'\n{}", home.display(), argv[2])]), TIMEOUT)
}

fn set_modified(path: &Path, secs: u64) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
}

#[test]
fn the_script_lists_the_transcripts_images_that_exist_newest_first() {
    let home = fake_home("list");
    let work = home.join("work");
    let projects = home.join(".claude/projects");
    std::fs::create_dir_all(work.join("plots")).unwrap();
    std::fs::create_dir_all(projects.join("x")).unwrap();
    let (old, new, gone) = (work.join("old.png"), work.join("plots/new-1.JPG.jpeg"), work.join("gone.png"));
    let script_py = work.join("plot.py");
    std::fs::write(&old, b"12345").unwrap();
    std::fs::write(&new, b"123").unwrap();
    std::fs::write(&script_py, b"print()").unwrap();
    // A folder named like a picture is none.
    std::fs::create_dir_all(work.join("dir.png")).unwrap();
    set_modified(&old, 1_760_000_000);
    set_modified(&new, 1_760_000_500);
    let (old, new) = (old.to_str().unwrap(), new.to_str().unwrap());
    let transcript = format!(
        "{{\"type\":\"assistant\",\"message\":\"wrote {old} and {gone}\"}}\n\
         {{\"type\":\"user\",\"content\":\"run {py}, then look at \\\"{new}\\\" and {old} again; {dir}\"}}\n\
         {{\"note\":\"a relative plots/new-1.png and a path with a space /w/my plots/x.png are not found\"}}\n",
        gone = gone.display(),
        py = script_py.display(),
        dir = work.join("dir.png").display(),
    );
    std::fs::write(projects.join("x").join(format!("{ID}.jsonl")), &transcript).unwrap();
    // Another session's transcript is not read.
    let other = format!("{{\"x\":\"{}\"}}\n", script_py.with_extension("png").display());
    std::fs::write(script_py.with_extension("png"), b"1").unwrap();
    std::fs::write(projects.join("x/11111111-2222-3333-4444-555555555555.jsonl"), other).unwrap();

    let out = with_home(&made_argv(LOCAL, ID, None, LOCAL), &home).unwrap();
    assert_eq!(parse_made(&out), Ok(vec![made(new, 1_760_000_500, 3), made(old, 1_760_000_000, 5)]), "{out:?}");

    // No transcript of that id: said so, and no error.
    let none = with_home(&made_argv(LOCAL, "99999999-2222-3333-4444-555555555555", None, LOCAL), &home).unwrap();
    assert_eq!(parse_made(&none), Err(NoTranscript));
    // One that names no picture: an empty list.
    std::fs::write(projects.join("x/99999999-2222-3333-4444-555555555555.jsonl"), "{\"a\":\"/w/x.py\"}\n").unwrap();
    let empty = with_home(&made_argv(LOCAL, "99999999-2222-3333-4444-555555555555", None, LOCAL), &home).unwrap();
    assert_eq!(parse_made(&empty), Ok(Vec::new()));
}

#[test]
fn a_transcript_with_thousands_of_images_gives_the_newest_five_hundred() {
    let home = fake_home("many");
    let (work, projects) = (home.join("w"), home.join(".claude/projects/-w"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&projects).unwrap();
    let mut transcript = String::new();
    for n in 0..620u64 {
        let path = work.join(format!("{n}.png"));
        std::fs::write(&path, b"x").unwrap();
        set_modified(&path, 1_760_000_000 + n);
        transcript.push_str(&format!("{{\"made\":\"{}\"}}\n", path.display()));
    }
    // And two thousand that are gone.
    for n in 0..2000 {
        transcript.push_str(&format!("{{\"made\":\"{}/gone-{n}.png\"}}\n", work.display()));
    }
    std::fs::write(projects.join(format!("{ID}.jsonl")), transcript).unwrap();
    let files = parse_made(&with_home(&made_argv(LOCAL, ID, None, LOCAL), &home).unwrap()).unwrap();
    assert_eq!(files.len(), MADE_MAX);
    assert_eq!(files[0], made(work.join("619.png").to_str().unwrap(), 1_760_000_619, 1));
    assert_eq!(files[MADE_MAX - 1].modified_s, 1_760_000_120);
}

#[test]
fn local_time_is_what_date_says() {
    // Winter and summer, and a moment ago: whatever zone this machine is in.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    for secs in [0, 1_736_000_000, 1_752_000_000, 1_760_000_000, now] {
        let said = run(&v(&["date", "-d", &format!("@{secs}"), "+%Y %-m %-d %-H %-M"]), TIMEOUT).unwrap();
        let t = local_time(secs).expect("a time this machine can name");
        assert_eq!(format!("{} {} {} {} {}", t.year, t.month, t.day, t.hour, t.minute), said.trim(), "{secs}");
    }
    assert_eq!(local_time(u64::MAX), None);
}
