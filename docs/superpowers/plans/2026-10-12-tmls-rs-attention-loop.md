# tmls-rs: attention loop and reliability — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One window that says sooner which session needs the user and lets them act on it from the list — keys help, copy/paste that works and talks, ssh that survives a sleeping link, an unread mark, approvals, pushed status with the last reply, and send/queue a message — plus two small fixes.

**Architecture:** Everything that runs on a host stays an argv built in `client-core` (`tmux.rs` gains one ssh builder, `ssh_base`, used by every ssh); two new `client-core` modules hold pure logic (`approve.rs`: the permission dialog off a pane capture; `push.rs`: one `tail -F` of tmls's `events.jsonl` per host, its parser and the tmls mapping onto a `HostSnapshot`). The desktop app keeps its rule "remote work on a thread, answer as a `UserEvent`"; new state (`Queue`, the latest listing per host, the push readers) lives in `Running`; new modals are `forms::Form` variants or a `forms::Panel`. Poll stays the truth: a pushed event changes the stored listing and goes down the same path as a poll (`Running::snapshot`), and `Poller::force_next` makes sure the next real listing replaces it.

**Tech Stack:** Rust 2024, winit 0.30, wgpu 30, `s2-core` / `s2-wgpu` (terminal, `TextField`), `arboard` 3.6 (`wayland-data-control`), `serde_json`, POSIX sh + tmux + OpenSSH on the hosts; headless e2e in Docker (sway, lavapipe, wtype, grim, `vptr.py`, now `wl-clipboard`). No new crate.

**Spec:** `docs/superpowers/specs/2026-10-12-tmls-rs-attention-loop-design.md` (features F1–F8, decisions D1–D10). Read it with this plan; where this plan departs from it, the task says so and why ("Departs from the spec").

**Starting point:** branch `feat/attention-loop` at `2007d49` (spec commits on top of `main` `d235ee3`). All work is committed on this branch. Nothing is merged or pushed.

## Who does the work (the user's instruction for this round: "work over night", unattended)

- **One main agent, model `fable` (this session):** owns the plan from Task 1 to Task 10, keeps the ledger, dispatches, makes the rulings, runs every verification itself (tests, clippy, fmt, e2e, looks at the screenshots) and writes `PROGRESS.md`. It writes no feature code.
- **Opus subagents (`model: opus`), dispatched by the main agent:** one implementer per task, **never two at once** (Tasks 2–8 all touch `crates/desktop/src/app.rs` and most touch `draw.rs`/`forms.rs`); one task reviewer after each task, given `git diff <task start>..HEAD` as a file under `/data/scratch/tmls-rs-v0/night2/review/` and the task's text, not the main agent's conclusions; one whole-branch reviewer in Task 9.
- **The PC:** at the end only, and only after `ssh -o ConnectTimeout=5 pcl 'bash -lic "game-mode status"'` — run as its own command, never chained — prints `off`. Then, as a separate command: `ssh pcl 'cd ~/Others/tmls-rs && git fetch && git checkout --detach <verified commit> && tooling/docker/build.sh build --release -p desktop'`. If game mode is `on`, do not build; say so in the report.
- **Never:** merge, push, delete or move anything (worktrees and scratch included), allocate compute, inject input into the user's KDE session, or message anyone. Stop and report for any of these.
- **Last task:** the hand-off sheet `docs/NIGHT-TEST.md` (Task 10) — what the user tries in the morning.

## Global Constraints

- Scratch, logs and e2e output under `/data/scratch/tmls-rs-v0/night2/`, never `/tmp`. `spikes/s1/build-env.sh` resets `TMPDIR`; use explicit paths for logs.
- Git: name `sgkayast`, email `sgn.kayastha@gmail.com` (the repo's config already says so), **no Claude attribution** in commits (no `Co-Authored-By`, no "Generated with"). Commit on `feat/attention-loop` only. No push, no merge.
- Tests never touch the user's live tmux (private server, `tmlsrs-test-` prefix, or a shell `tmux()` stub) and never inject input into the KDE session.
- `client-core` is not rustfmt-clean: do not reformat it wholesale; format only the lines you write in its style. `cargo fmt -p desktop --check` must stay clean.
- Commands:
  - desktop tests: `tooling/docker/build.sh test -p desktop` (one file: `… -p desktop --test NAME`)
  - client-core tests: `source spikes/s1/build-env.sh && cargo test -p client-core` (one file: `--test NAME`)
  - lints: `source spikes/s1/build-env.sh && cargo clippy -p desktop -p client-core --all-targets -- -D warnings`
  - fmt: `source spikes/s1/build-env.sh && cargo fmt -p desktop --check`
  - e2e: `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/<task>-e2e tooling/smoke/run.sh` (a fresh directory each run); it ends with `== 0 of N checks failed`.
- Every image mentioned to the user (screenshots included) is given by its full absolute path.
- The app never writes `~/.claude/settings.json`, never installs hooks, never writes `~/.cache/tmls/` (D1). The only file under `~/.config/tmls` it writes stays `pins`.

## Review Focus

1. A message with quotes, `$(…)`, backticks, backslashes, tabs, newlines and non-ASCII, 4 KB long: tmux receives it byte for byte on stdin and nothing in it is run by any shell — pinned in Task 7 by `send_text_gives_tmux_the_message_on_stdin_byte_for_byte` (`crates/client-core/tests/tmux.rs`).
2. The dialog changed or vanished between the capture and Yes/No (answered in the terminal meanwhile, a second prompt came up): no key is sent and the status line says `NAME isn't asking that any more` — pinned in Task 5 by `answer_sends_the_key_only_while_the_same_dialog_is_up` (`crates/desktop/tests/forms.rs`).
3. `events.jsonl` with junk (empty, not JSON, a cut-off line, no `t`, unknown `ev`, a 100 KB message), an event for a session nobody lists, or one older than what the listing already saw: ignored, a session's state never moves backwards, nothing panics — pinned in Task 6 by `junk_lines_are_ignored` and `older_or_unknown_events_change_nothing` (`crates/client-core/tests/push.rs`).
4. A busy session that flaps Running↔Waiting, fails (Error), drops out of the listing (host offline), or has its turn end again while a queued send is still in flight: no message is lost and none is sent twice — pinned in Task 7 by `flapping_failing_or_unlisted_sends_nothing` and `a_message_in_flight_is_not_sent_twice` (`crates/desktop/tests/queue.rs`).
5. The ssh socket directory already exists with mode 0755, or `$XDG_RUNTIME_DIR` is so long that `cm-%C` would overflow a Unix socket path: it is made 0700, or there is no ControlMaster at all (plain ssh) — never an ssh that fails to bind — pinned in Task 1 by `the_socket_directory_is_the_runtime_dir_else_the_cache` and `once_switched_on_every_ssh_shares_the_connection` (`crates/client-core/tests/ssh_control.rs`).

Each line's test is written in the task that owns the code, in that task's first step.

---

## File map

| File | Tasks | Responsibility after this round |
|---|---|---|
| `crates/client-core/src/tmux.rs` | 1, 2, 4, 5, 7 | every argv: `ssh_base` + keepalive + ControlMaster; `valid_pane`, `pane_target`, `capture_argv`, `send_keys_argv`, `send_text_script`/`send_text_argv`/`buffer_name`; `run_with_stdin` |
| `crates/client-core/src/listing.rs` | 1, 2, 6 | `listing_command` through `ssh_base`; `SessionInfo.pane`, `.activity`, `.last_reply` |
| `crates/client-core/src/seen.rs` | 2 | ◆ plus the unread map (`looked`, `unread`) |
| `crates/client-core/src/poll.rs` | 2, 6 | activity not a reason to deliver; `force_next`; `period_from` (`TMLS_RS_POLL_S`) |
| `crates/client-core/src/approve.rs` (new) | 5 | the permission dialog read off a capture |
| `crates/client-core/src/push.rs` (new) | 6 | `events.jsonl` reader, parser, tmls mapping, replies carried across listings |
| `crates/desktop/src/keys.rs` | 3, 5, 7 | `HELP`, `help_text`, `help_panel`, `ListKey::{Help, Approve, Send}`, `Global::Ask` |
| `crates/desktop/src/clip.rs` (new) | 4 | the clipboard: arboard, `wl-copy`/`wl-paste` fallback, the status-line texts |
| `crates/desktop/src/forms.rs` | 4, 5, 7, 8 | `Form::Approve`, `Form::Ask`, `Request::Answer`, menu items, the remote glue (`approval`, `answer`, `send_text`), `new_session_again` |
| `crates/desktop/src/queue.rs` (new) | 7 | the in-memory queue's state machine |
| `crates/desktop/src/sidebar.rs` | 2, 5, 6, 8 | `next_unread`, `info`, `summary` with the last reply, `renamed_outside` |
| `crates/desktop/src/draw.rs`, `theme.rs` | 2, 6, 7 | `•`, `N✉`, `· poll`, legend entries |
| `crates/desktop/src/alerts.rs` | 5 | `y` on an alert |
| `crates/desktop/src/tabs.rs` | 8 | `rekey` |
| `crates/desktop/src/app.rs` | 1–8 | wiring only |
| `crates/desktop/src/main.rs` | 3 | `tmls-rs keys` |
| `tooling/smoke/e2e.sh`, `Dockerfile.smoke`, `run.sh` | 3–8 | one e2e block per feature; `wl-clipboard`; longer timeout |

---

### Task 1: F3 — ssh keepalive and one shared connection per host

**Files:**
- Modify: `crates/client-core/src/tmux.rs:13-18` (imports), `:83-88` (`ssh`), `:95-105` (`attach_argv`); add `ssh_base`, `control_dir`, `control_options`, `control_wanted`, `use_control_master` after `strings` (`:79-81`)
- Modify: `crates/client-core/src/listing.rs:158-171` (`listing_command`)
- Modify: `crates/desktop/src/app.rs:521-542` (`Running::start`, before `Poller::start`)
- Test: `crates/client-core/tests/ssh_control.rs` (new), `crates/client-core/tests/tmux.rs:21-22,74-78,215`, `crates/client-core/tests/files.rs:20,44`, `crates/client-core/tests/listing.rs:316-323`

**Interfaces:**
- Consumes: nothing new.
- Produces (client-core `tmux`):
  - `pub fn ssh_base(host: &str, flags: &[&str]) -> Vec<String>` — `ssh`, `-o ServerAliveInterval=15 -o ServerAliveCountMax=3`, the ControlMaster options once switched on, `flags`, `host`. Every ssh argv in the crate starts here.
  - `pub fn control_dir(runtime: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf>`
  - `pub fn control_options(dir: &Path) -> Vec<String>`
  - `pub fn control_wanted(value: Option<&OsStr>) -> bool` (value of `TMLS_RS_NO_CONTROLMASTER`)
  - `pub fn use_control_master(dir: &Path) -> std::io::Result<()>`
- Departs from the spec: `ssh_base(host)` becomes `ssh_base(host, flags)` — attach needs `-t` and no `BatchMode` (`tmux.rs:95-105`), every other command `BatchMode` and `ConnectTimeout`; the builder takes them so the keepalive and ControlMaster words exist once. The socket directory is `$XDG_RUNTIME_DIR/tmls-rs`, else `~/.cache/tmls-rs` (the spec's "`<runtime>/tmls-rs`" with runtime `~/.cache/tmls-rs` would be `~/.cache/tmls-rs/tmls-rs`). The switch is a process-wide `OnceLock` set once at start, so test binaries that never call `use_control_master` get deterministic argv (keepalive only). The Copy button's text (`attach_text`) stays plain ssh, as in tmls.

- [ ] **Step 1: Write the failing tests.** New file `crates/client-core/tests/ssh_control.rs` (its own test binary: the switch is process-wide):

```rust
//! The shared ssh connection: where its socket goes, and that every ssh tmls-rs starts uses it
//! once `use_control_master` ran. A test binary of its own: that switch is for the whole process.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use client_core::listing::{SessionKey, listing_command};
use client_core::tmux::{
    attach_argv, control_dir, control_options, control_wanted, kill_argv, ssh_base, use_control_master,
};

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey { host: host.to_string(), name: name.to_string() }
}

#[test]
fn the_socket_directory_is_the_runtime_dir_else_the_cache() {
    let os = |s: &'static str| Some(OsStr::new(s));
    assert_eq!(control_dir(os("/run/user/1000"), os("/home/u")), Some(PathBuf::from("/run/user/1000/tmls-rs")));
    assert_eq!(control_dir(None, os("/home/u")), Some(PathBuf::from("/home/u/.cache/tmls-rs")));
    assert_eq!(control_dir(os(""), os("/home/u")), Some(PathBuf::from("/home/u/.cache/tmls-rs")));
    assert_eq!(control_dir(None, None), None);
    // `/cm-` and the 40 characters of %C would not fit a Unix socket's path: no shared connection.
    let long = format!("/{}", "d".repeat(70));
    assert_eq!(control_dir(Some(OsStr::new(&long)), None), None);
}

#[test]
fn the_switch_is_off_only_for_1() {
    assert!(control_wanted(None));
    assert!(control_wanted(Some(OsStr::new(""))));
    assert!(control_wanted(Some(OsStr::new("0"))));
    assert!(!control_wanted(Some(OsStr::new("1"))));
}

#[test]
fn control_options_name_the_socket_in_the_directory() {
    assert_eq!(
        control_options(Path::new("/run/user/1000/tmls-rs")),
        ["-o", "ControlMaster=auto", "-o", "ControlPersist=10m", "-o", "ControlPath=/run/user/1000/tmls-rs/cm-%C"]
    );
}

#[test]
fn once_switched_on_every_ssh_shares_the_connection() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("ssh-control-{}", std::process::id()))
        .join("tmls-rs");
    std::fs::create_dir_all(&dir).unwrap();
    // An existing directory with loose permissions is made private.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    use_control_master(&dir).expect("the socket directory");
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);

    let path = format!("ControlPath={}/cm-%C", dir.display());
    let base = ssh_base("ph", &["-t"]);
    assert_eq!(base[..5], ["ssh", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3"]);
    assert!(base.contains(&path), "{base:?}");
    assert_eq!(base[base.len() - 2..], ["-t", "ph"]);
    for argv in [attach_argv(&key("ph", "w"), "here"), kill_argv(&key("ph", "w"), "here")] {
        assert!(argv.contains(&path), "{argv:?}");
    }
    assert!(listing_command("ph", "here").get_args().any(|arg| arg == OsStr::new(&path)));
    // This machine's commands are no ssh at all.
    assert_eq!(attach_argv(&key("here", "w"), "here")[0], "tmux");
}
```

In `crates/client-core/tests/tmux.rs` replace the `SSH` constant (line 21-22) and add `ATTACH`:

```rust
/// The words ssh puts before the command for everything but attach (no `use_control_master`
/// in this test binary: keepalive only).
const SSH: [&str; 10] = [
    "ssh", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "ph",
];
/// The same for attach: a terminal, and ssh may ask (no BatchMode).
const ATTACH: [&str; 7] = ["ssh", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-t", "ph"];
```

and at lines 74-78 build the expectation from it:

```rust
    let mut work = v(&ATTACH);
    work.push("tmux -u attach -t '=work'".to_string());
    assert_eq!(attach_argv(&key("ph", "work"), LOCAL), work);
    let mut agent = v(&ATTACH);
    agent.push("tmux -u attach -t '=Agent Thing'".to_string());
    assert_eq!(attach_argv(&key("ph", "Agent Thing"), LOCAL), agent);
```

and at line 215: `let attach = remote(&attach_argv(&key("ph", name), LOCAL), &ATTACH);`.

In `crates/client-core/tests/files.rs:20` use the same 10-word `SSH` constant, and at line 44 compare `&argv[..SSH.len()]` instead of `&argv[..6]`.

In `crates/client-core/tests/listing.rs:316-323`:

```rust
    assert_eq!(
        argv(listing_command("nas", "laptop")),
        [
            "ssh", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "-o", "BatchMode=yes", "-o",
            "ConnectTimeout=5", "nas", "sh", "-s",
        ]
    );
```

- [ ] **Step 2: Run to see them fail.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test ssh_control --test tmux --test files --test listing`
Expected: compile errors `cannot find function ssh_base` / `control_dir` …; once those exist, assertion failures on the old argv.

- [ ] **Step 3: Implement in `tmux.rs`.** Imports (lines 13-18) become:

```rust
use std::ffi::OsStr;
use std::io::{ErrorKind, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};
```

After `strings` add:

```rust
/// What every ssh tmls-rs starts says first: a probe every 15 s, given up after three
/// unanswered, so a link that died (a laptop asleep, Wi-Fi gone) ends the command — and an
/// attached tab shows "ended" — within 45 s instead of hanging.
const KEEPALIVE: [&str; 4] = ["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3"];

/// The ControlMaster options, once `use_control_master` ran; never set in a process that does
/// not call it (the tests), whose ssh commands then have the keepalive only.
static CONTROL: OnceLock<Vec<String>> = OnceLock::new();

/// Where the shared connections' sockets go: `$XDG_RUNTIME_DIR/tmls-rs`, else
/// `~/.cache/tmls-rs`. `None` when neither is known, or when the socket's path would not fit:
/// `cm-` and the 40 characters of `%C` after the directory must stay well under a Unix
/// socket's 108 bytes (ssh adds a suffix while it binds).
pub fn control_dir(runtime: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let dir = match (runtime.filter(|r| !r.is_empty()), home.filter(|h| !h.is_empty())) {
        (Some(runtime), _) => Path::new(runtime).join("tmls-rs"),
        (None, Some(home)) => Path::new(home).join(".cache/tmls-rs"),
        (None, None) => return None,
    };
    (dir.as_os_str().len() + "/cm-".len() + 40 < 100).then_some(dir)
}

/// One master connection per host, kept 10 minutes after its last user (tmls does the same).
pub fn control_options(dir: &Path) -> Vec<String> {
    vec![
        "-o".to_string(),
        "ControlMaster=auto".to_string(),
        "-o".to_string(),
        "ControlPersist=10m".to_string(),
        "-o".to_string(),
        format!("ControlPath={}/cm-%C", dir.display()),
    ]
}

/// Whether to share connections, from `TMLS_RS_NO_CONTROLMASTER`: off only for `1`.
pub fn control_wanted(value: Option<&OsStr>) -> bool {
    value != Some(OsStr::new("1"))
}

/// Makes `dir` (mode 0700, also when it existed with another mode) and from then on every ssh
/// this process starts shares one connection per host through it. Call once, at start; a
/// second call changes nothing.
pub fn use_control_master(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let _ = CONTROL.set(control_options(dir));
    Ok(())
}

/// The start of every ssh argv: `ssh`, the keepalive, the shared connection once switched on,
/// `flags`, then `host`. The caller adds the remote command.
pub fn ssh_base(host: &str, flags: &[&str]) -> Vec<String> {
    let mut argv = vec!["ssh".to_string()];
    argv.extend(KEEPALIVE.iter().map(|word| word.to_string()));
    if let Some(control) = CONTROL.get() {
        argv.extend(control.iter().cloned());
    }
    argv.extend(flags.iter().map(|word| word.to_string()));
    argv.push(host.to_string());
    argv
}
```

`ssh` (line 84-88) becomes:

```rust
/// `command` run by `host`'s shell without ever prompting (as the listing does).
pub(crate) fn ssh(host: &str, command: String) -> Vec<String> {
    let mut argv = ssh_base(host, &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5"]);
    argv.push(command);
    argv
}
```

`attach_argv`'s remote branch (line 103):

```rust
    } else {
        let mut argv = ssh_base(&key.host, &["-t"]);
        argv.push(format!("tmux -u attach -t {}", quote(&target)));
        argv
    }
```

- [ ] **Step 4: `listing_command`** (`listing.rs:166-170`):

```rust
    } else {
        let argv = crate::tmux::ssh_base(host, &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5"]);
        let mut cmd = std::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]).args(["sh", "-s"]);
        cmd
    }
```

Update its doc comment: "…over ssh (`tmux::ssh_base`: keepalive, the shared connection), which must not prompt…".

- [ ] **Step 5: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass (including the four new ones).

- [ ] **Step 6: Switch it on at start.** In `crates/desktop/src/app.rs` `Running::start`, right before `let poller = Poller::start(` (line 535):

```rust
        // One ssh connection per host, kept 10 minutes; TMLS_RS_NO_CONTROLMASTER=1 turns it off
        // (the keepalive stays). Before the poller: its first listing is the first ssh.
        if client_core::tmux::control_wanted(std::env::var_os("TMLS_RS_NO_CONTROLMASTER").as_deref()) {
            let dir = client_core::tmux::control_dir(
                std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
                std::env::var_os("HOME").as_deref(),
            );
            match dir.map(|dir| client_core::tmux::use_control_master(&dir).map(|()| dir)) {
                Some(Ok(dir)) => crate::errln(format_args!("tmls-rs: ssh shares one connection per host ({})", dir.display())),
                Some(Err(e)) => crate::errln(format_args!("tmls-rs: ssh without shared connections: {e}")),
                None => crate::errln(format_args!("tmls-rs: ssh without shared connections: no place for the sockets")),
            }
        }
```

(`cargo fmt -p desktop` will wrap the long lines; run it.)

- [ ] **Step 7: Desktop tests, lints, fmt.**
Run: `tooling/docker/build.sh test -p desktop` then `source spikes/s1/build-env.sh && cargo clippy -p desktop -p client-core --all-targets -- -D warnings && cargo fmt -p desktop --check`
Expected: all pass, no warnings, fmt clean. (`tests/viewer.rs:199` only checks `["ssh", "-o"]` and still holds.)

- [ ] **Step 8: Commit.**

```bash
git add crates/client-core/src/tmux.rs crates/client-core/src/listing.rs crates/client-core/tests/ssh_control.rs \
  crates/client-core/tests/tmux.rs crates/client-core/tests/files.rs crates/client-core/tests/listing.rs crates/desktop/src/app.rs
git commit -m "feat(client-core): a dead ssh link ends within 45 s instead of freezing a tab; one shared connection per host"
```

---

### Task 2: F4 — pane id, unread mark `•`

**Files:**
- Modify: `crates/client-core/src/listing.rs:94-115` (`SessionInfo`), `:174-216` (`Claude`, `from_json`), `:385-396` and `:414-425` (the two `SessionInfo` literals)
- Modify: `crates/client-core/src/tmux.rs` (add `valid_pane`)
- Modify: `crates/client-core/src/seen.rs` (whole file is 76 lines)
- Modify: `crates/client-core/src/poll.rs:155-161` (`without_ages`)
- Modify: `crates/desktop/src/theme.rs:44-58`, `crates/desktop/src/draw.rs:83,102-115,805-830,884-962,1310-1335,1354-1375`, `crates/desktop/src/sidebar.rs:466-487`, `crates/desktop/src/app.rs:247-252,739-772,803-851,1634-1639,1814-1830`
- Test: `crates/client-core/tests/listing.rs`, `crates/client-core/tests/seen.rs`, `crates/client-core/tests/poll.rs`, `crates/desktop/tests/sidebar.rs`, `crates/desktop/tests/sidebar_draw.rs:104-117,633-650`, `crates/desktop/tests/layout.rs:48-56,185-200`, `crates/desktop/tests/side_draw.rs:213,394,687`; every test `SessionInfo { … }` literal that lists all fields (the compiler names them: E0063) gains `pane: None, activity: None,`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces:
  - `SessionInfo.pane: Option<String>` (`%N`, validated), `SessionInfo.activity: Option<u64>` (host clock, s)
  - `client_core::tmux::valid_pane(pane: &str) -> bool`
  - `Seen::looked(&mut self, key: &SessionKey)`, `Seen::unread(&self, key: &SessionKey) -> bool`
  - `Sidebar::next_unread(&self, after: Option<&SessionKey>, unread: &dyn Fn(&SessionKey) -> bool) -> Option<SessionKey>`
  - `draw::RowLook.unread: bool`, `draw::View.unread_rows: Vec<SessionKey>`
  - `theme::UNREAD_MARK: &str = "•"`, `theme::UNREAD_RGB: [u8; 3]`
- Departs from the spec: "unread = `window_activity` changed" would make every Claude session unread every minute — Claude repaints its status line each minute (`listing.rs:377`: "its status line redraws every minute: output isn't work"). For a Claude session `activity` is its status file's `statusUpdatedAt` (what tmls's `hosts.status` uses as `claude_since`); for any other session its newest window output. The poller does not deliver a listing that differs only in `activity` (as with the ages): a session's activity reaches the app with its next status change (a shell printing turns Running; Claude's status file changes with its status), so the poll's delivery rate and the idle frame count stay as they are. The legend grows from 5 to 6 lines (7 entries): the layout tests' numbers move by one line.

- [ ] **Step 1: Failing client-core tests.** Append to `crates/client-core/tests/listing.rs`:

```rust
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
```

Append to `crates/client-core/tests/seen.rs` (and add `pane: None, activity: None,` to the literal in `snap`):

```rust
/// `host`'s sessions, idle, each with the time of its newest output.
fn active(host: &str, sessions: &[(&str, u64)]) -> HostSnapshot {
    let idle: Vec<(&str, Status)> = sessions.iter().map(|(name, _)| (*name, Status::Idle)).collect();
    let mut snapshot = snap(host, &idle);
    for (info, (_, at)) in snapshot.sessions.iter_mut().zip(sessions) {
        info.activity = Some(*at);
    }
    snapshot
}

#[test]
fn output_since_the_last_look_is_unread() {
    let mut seen = Seen::new();
    let w = key("nas", "w");
    seen.apply(&mut active("nas", &[("w", 100)]));
    assert!(!seen.unread(&w), "a session's first listing is never unread");
    seen.apply(&mut active("nas", &[("w", 100)]));
    assert!(!seen.unread(&w));
    seen.apply(&mut active("nas", &[("w", 130)]));
    assert!(seen.unread(&w), "new output, not looked at");
    seen.looked(&w);
    assert!(!seen.unread(&w));
    seen.apply(&mut active("nas", &[("w", 160)]));
    assert!(seen.unread(&w));
    seen.forget_missing(&active("nas", &[]));
    assert!(!seen.unread(&w), "gone");
    seen.apply(&mut active("nas", &[("w", 200)]));
    assert!(!seen.unread(&w), "listed again: a first listing");
}

#[test]
fn outside_tmux_or_without_activity_is_never_unread() {
    let mut seen = Seen::new();
    seen.apply(&mut active(KITTY, &[("notes", 1)]));
    seen.apply(&mut active(KITTY, &[("notes", 50)]));
    assert!(!seen.unread(&key(KITTY, "notes")));
    seen.apply(&mut snap("nas", &[("w", Status::Idle)]));
    seen.apply(&mut snap("nas", &[("w", Status::Running)]));
    assert!(!seen.unread(&key("nas", "w")));
    assert!(!seen.unread(&key("nas", "never-listed")));
    seen.looked(&key("nas", "never-listed")); // nothing to remember, no panic
}
```

Append to `crates/client-core/tests/poll.rs`:

```rust
#[test]
fn a_listing_that_differs_only_in_activity_is_not_delivered() {
    let _one = alone();
    // The window's output time changes at every listing; its status (running) does not.
    let (poller, rx) = start(&["a"], Duration::from_millis(100), Duration::from_secs(5), |_| {
        sh("cat >/dev/null; n=$(od -An -N2 -tu2 /dev/urandom | tr -d ' '); echo 100; echo \"w:1:0:$((100 + n))\"")
    });
    next(&rx, Duration::from_secs(5));
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err(), "new output alone is no news for the poller");
    drop(poller);
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test listing --test seen --test poll`
Expected: compile errors (`no field pane`, `no method looked`), then the poll test fails (deliveries every 100 ms).

- [ ] **Step 3: Implement in client-core.**

`tmux.rs`, after `valid_session_name`:

```rust
/// Whether `pane` is a tmux pane id (`%` and digits): the only form of a pane that goes into a
/// command.
pub fn valid_pane(pane: &str) -> bool {
    pane.strip_prefix('%').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}
```

`listing.rs` `SessionInfo`, after `session_id`:

```rust
    /// Claude's own tmux pane (`%7`, from its status file's `tmux`); `None` for a session that
    /// is not Claude's. Approvals and messages go there: after a split the window's active pane
    /// may be a shell.
    pub pane: Option<String>,
    /// When the session last had news, by the host's clock (s): for a Claude session the time
    /// of its last status change, for any other its newest window output; `None` outside tmux.
    /// What "unread" compares (`seen`).
    pub activity: Option<u64>,
```

`struct Claude` gains `pane: Option<String>,` and `from_json` sets it, next to `session:`:

```rust
            // "SESSION:@WINDOW.%PANE": the pane is what follows the last dot, if it is an id
            pane: pane.rsplit_once('.').map(|(_, id)| id).filter(|id| crate::tmux::valid_pane(id)).map(str::to_string),
```

The tmux sessions' literal (line 385-396) gains:

```rust
                pane: claude.and_then(|c| c.pane.clone()),
                // Claude repaints its status line every minute: its news is its status time
                activity: Some(match claude.filter(|c| c.status.is_some()) {
                    Some(c) => c.since.max(0) as u64,
                    None => w.activity,
                }),
```

The outside-tmux literal (line 414-425) gains `pane: None, activity: None,`.

`poll.rs` `without_ages`:

```rust
/// The snapshot with every `last_output_age_s` and `activity` blanked: what "the same as
/// before" compares. Ages grow at every listing; activity moves with output, which reaches the
/// app with the session's next status change (a shell that prints turns running).
fn without_ages(snapshot: &HostSnapshot) -> HostSnapshot {
    let mut snapshot = snapshot.clone();
    for session in &mut snapshot.sessions {
        session.last_output_age_s = None;
        session.activity = None;
    }
    snapshot
}
```

`seen.rs`: module comment gains a paragraph "**Unread (`•`)**: a session whose `activity` is newer than when the user last looked at it (`looked`: its tab in front while the window has the focus). A session's first listing sets what it was looked at; sessions outside tmux are never unread." The struct and the new code:

```rust
#[derive(Debug, Default)]
pub struct Seen {
    /// What the listing said of each session at its host's last snapshot.
    last: HashMap<SessionKey, Status>,
    /// Sessions that finished and have not been opened since.
    unseen: HashSet<SessionKey>,
    /// Each session's `activity` at its last listing.
    activity: HashMap<SessionKey, u64>,
    /// The `activity` each session had when the user last looked at it.
    looked_at: HashMap<SessionKey, u64>,
}
```

In `apply`, first thing inside the `for session in &mut snap.sessions` loop:

```rust
            if let (Some(at), false) = (session.activity, session.key.host == KITTY) {
                self.activity.insert(session.key.clone(), at);
                // a first listing: what is there now was there before anyone could look
                self.looked_at.entry(session.key.clone()).or_insert(at);
            }
```

New methods after `opened`:

```rust
    /// The user is looking at the session (its tab is in front and the window has the focus):
    /// what it has shown so far is read.
    pub fn looked(&mut self, key: &SessionKey) {
        if let Some(at) = self.activity.get(key) {
            self.looked_at.insert(key.clone(), *at);
        }
    }

    /// The session has news the user has not looked at: the `•` after its name.
    pub fn unread(&self, key: &SessionKey) -> bool {
        matches!((self.activity.get(key), self.looked_at.get(key)), (Some(now), Some(looked)) if now > looked)
    }
```

In `forget`, after `self.unseen.retain(listed);`:

```rust
        self.activity.retain(|key, _| listed(key));
        self.looked_at.retain(|key, _| listed(key));
```

- [ ] **Step 4: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass.

- [ ] **Step 5: Failing desktop tests.** First make every test `View { … }` literal end in `..View::default()` instead of naming the trailing fields (`tests/sidebar_draw.rs:104-117`, `tests/side_draw.rs:213,394,687`), so later tasks' fields need no edits; add `pane: None, activity: None,` to every full `SessionInfo` literal. Then append to `crates/desktop/tests/sidebar_draw.rs`:

```rust
#[test]
fn unread_shows_a_dot_after_the_name_before_the_star() {
    let info = session(LOCAL, "work", Status::Idle);
    let look = RowLook {
        unread: true,
        pinned: true,
        ..RowLook::default()
    };
    let said = spans(&info, look);
    let (name, dot, star) = (find(&said, "work"), find(&said, "•"), find(&said, "★"));
    assert_eq!(dot.col, name.col + 4 + 1);
    assert_eq!(star.col, dot.col + 2);
    assert_eq!(dot.colour, theme::UNREAD_RGB);
    assert_within(&said);
    // Without it nothing moves: the ★ is right after the name.
    let pinned = spans(
        &info,
        RowLook {
            pinned: true,
            ..RowLook::default()
        },
    );
    assert_eq!(find(&pinned, "★").col, name.col + 4 + 1);
    assert!(!pinned.iter().any(|span| span.text == "•"));
}
```

In `legend_and_buttons_sit_level_with_the_terminals_last_row` change `assert_eq!(l.legend.h, 5.0 * l.cell.1);` to `6.0`, and after the tool-letter loop add:

```rust
    assert_eq!(glyphs(&quads, l.legend, theme::UNREAD_RGB).len(), 1, "• unread");
```

In `crates/desktop/tests/layout.rs` (worked out from `layout_with` with one more legend line; do not change layout code to fit old numbers):
- lines 48-56: comment "five lines of legend" → "six lines of legend"; `l.list_lines` `25` → `24`; `l.list.h` `500.0` → `480.0`; `(l.legend.y, l.legend.h)` `(546.0, 100.0)` → `(526.0, 120.0)`.
- `a_low_window_has_no_legend_and_no_buttons` (185-200) becomes:

```rust
#[test]
fn a_low_window_has_no_legend_and_no_buttons() {
    // Four lines for the list, or the foot goes. At 312 the terminal has thirteen rows, the
    // buttons start at its last but one (26 + 11 * 20 = 246), the legend at 126: the list has
    // the 99 pixels from 26 to the legend's line.
    let l = at(1281, 312);
    assert_eq!((l.legend.h, l.buttons.h, l.list_lines), (120.0, 40.0, 4));
    assert_eq!((l.list.y, l.legend.y, l.buttons.y), (26.0, 126.0, 246.0));
    let l = at(1281, 311);
    assert_eq!((l.legend.h, l.buttons.h), (0.0, 0.0));
    assert_eq!(
        l.list_lines, 14,
        "the list has the whole height under the bar's band"
    );
    assert_eq!(l.hit(15.0, 250.0, 0), Hit::List { line: 11, col: 1 });
    assert_eq!(l.hit(15.0, 306.0, 0), Hit::Sidebar);
}
```

Append to `crates/desktop/tests/sidebar.rs`:

```rust
#[test]
fn next_unread_wraps_and_never_returns_where_it_starts() {
    let mut s = sidebar(&[LOCAL]);
    s.apply(snapshot(
        LOCAL,
        vec![
            session(LOCAL, "a", Status::Idle),
            session(LOCAL, "b", Status::Idle),
            session(LOCAL, "c", Status::Idle),
        ],
    ));
    let unread = |k: &SessionKey| k.name == "a" || k.name == "c";
    assert_eq!(s.next_unread(None, &unread), Some(key(LOCAL, "a")));
    assert_eq!(s.next_unread(Some(&key(LOCAL, "a")), &unread), Some(key(LOCAL, "c")));
    assert_eq!(s.next_unread(Some(&key(LOCAL, "c")), &unread), Some(key(LOCAL, "a")), "wraps round");
    let only_a = |k: &SessionKey| k.name == "a";
    assert_eq!(s.next_unread(Some(&key(LOCAL, "a")), &only_a), None, "not the one it starts from");
    assert_eq!(sidebar(&[LOCAL]).next_unread(None, &unread), None, "an empty list");
}
```

- [ ] **Step 6: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test sidebar_draw --test layout --test sidebar`
Expected: compile errors (`no field unread`, `UNREAD_RGB`, `next_unread`), then the legend/layout numbers fail.

- [ ] **Step 7: Implement in the desktop.**

`theme.rs`, after `ATTACHED_MARK`:

```rust
/// After a session's name when it has news the user has not looked at (`Seen::unread`), and
/// its colour: one dot, no animation (D7).
pub const UNREAD_MARK: &str = "•";
pub const UNREAD_RGB: [u8; 3] = [0x56, 0xb6, 0xc2];
```

`draw.rs`: import `UNREAD_MARK, UNREAD_RGB` (line 83). Replace the legend constants (102-104) and the marks chain in `foot` (1354-1360):

```rust
/// The legend's entries after the status marks: what follows a name.
const LEGEND_MORE: [(&str, [u8; 3], &str); 2] =
    [(PIN, PIN_RGB, "pinned"), (UNREAD_MARK, UNREAD_RGB, "unread")];
/// The status marks and `LEGEND_MORE`, two a line.
const LEGEND_MARKS: usize = LEGEND.len() + LEGEND_MORE.len();
/// Lines of the legend, as in tmls: the marks two a line, then the agents' letters on two lines.
const LEGEND_LINES: usize = LEGEND_MARKS.div_ceil(2) + TOOL_LINES;
```

```rust
    // The marks, then what follows a name, then the agents' letters from a line of their own on.
    let marks = LEGEND
        .iter()
        .map(|(status, word)| (mark_glyph(*status), mark_rgb(*status), *word))
        .chain(LEGEND_MORE.iter().copied());
    let tools_from = LEGEND_MARKS.div_ceil(2) * 2;
```

`RowLook` (884-892) gains:

```rust
    /// The session has output the user has not looked at: `•` after the name.
    pub unread: bool,
```

In `session_spans` replace the block from `let pin_cols = …` to the `★` push (lines ~940-952) by:

```rust
    // After the name, in this order: `•` (unread), `★` (pinned); each with a space before it.
    let after: Vec<(&'static str, [u8; 3])> = [
        look.unread.then_some((UNREAD_MARK, UNREAD_RGB)),
        look.pinned.then_some((PIN, PIN_RGB)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let after_cols: usize = after.iter().map(|(text, _)| columns(text) + 1).sum();
    let name_cols = right.saturating_sub(name_col + after_cols);
    let name = Span::new(0, name_col, name_cols, info.key.name.as_str());
    let mut col = name_col + columns(&info.key.name).min(name_cols) + 1;
    out.push(match look.active {
        true => name.colour(CURRENT_RGB).bold(),
        false => name.colour(theme.sidebar_fg),
    });
    for (text, colour) in after {
        let cols = columns(text);
        if col + cols <= right {
            out.push(Span::whole(0, col, text).colour(colour).bold());
        }
        col += cols + 1;
    }
```

`View` (805-830) gains:

```rust
    /// Sessions with output the user has not looked at (`Seen::unread`).
    pub unread_rows: Vec<SessionKey>,
```

and in `sidebar()` the `RowLook` (1328-1332) gains `unread: view.unread_rows.contains(key),`.

`sidebar.rs`, after `next_waiting`:

```rust
    /// The next session after `after` with news the user has not looked at (`unread`), wrapping
    /// round: where Alt+Shift+N goes when nothing waits (tmls visits its "done" ones too).
    /// Never `after` itself, never a session outside tmux.
    pub fn next_unread(
        &self,
        after: Option<&SessionKey>,
        unread: &dyn Fn(&SessionKey) -> bool,
    ) -> Option<SessionKey> {
        let count = self.rows.len();
        let from = after
            .and_then(|key| self.position(key))
            .map_or(0, |at| at + 1);
        (0..count)
            .filter_map(|step| match &self.rows[(from + step) % count] {
                Row::Session(info) => Some(&info.key),
                _ => None,
            })
            .find(|key| Some(*key) != after && key.host != KITTY && unread(key))
            .cloned()
    }
```

`app.rs`:
- `snapshot()` (after `self.sidebar.deliver(&mut self.seen, snapshot, front);`, line 748):

```rust
        // The front tab, in a window that has the focus, is being read: nothing in it is news.
        if self.window_focused
            && let Some(front) = front
        {
            self.seen.looked(front);
        }
```

- `tab_changed()` (inside `Some(key) =>`, after `self.sidebar.opened(&mut self.seen, key);`): `if self.window_focused { self.seen.looked(key); }`
- `WindowEvent::Focused` (line 250, after `running.window_focused = focused;`): `if focused { running.looked_front(); }` with the new method:

```rust
    /// The window got the focus: what the front tab's session shows is read now.
    fn looked_front(&mut self) {
        if let Some(index) = self.tabs.active() {
            let key = self.tabs.key(index).clone();
            self.seen.looked(&key);
            self.invalidate();
        }
    }
```

- `build_view()`: in the `View { … }` literal add

```rust
            unread_rows: self
                .sidebar
                .rows()
                .iter()
                .filter_map(|row| match row {
                    sidebar::Row::Session(info) if self.seen.unread(&info.key) => {
                        Some(info.key.clone())
                    }
                    _ => None,
                })
                .collect(),
```

- `next_waiting()` (1634-1639):

```rust
    /// Alt+Shift+N or `n`: the next session that waits or finished unwatched; when none does,
    /// the next with news not looked at (`•`).
    fn next_waiting(&mut self) {
        let front = self.tabs.active().map(|index| self.tabs.key(index));
        let seen = &self.seen;
        let next = self
            .sidebar
            .next_waiting(front)
            .or_else(|| self.sidebar.next_unread(front, &|key| seen.unread(key)));
        if let Some(key) = next {
            self.open(&key);
        }
    }
```

Update the module comment of `draw.rs` (row picture, line 6) to show `● C name • ★`.

- [ ] **Step 8: All tests, lints, fmt.**
Run: `tooling/docker/build.sh test -p desktop`, `source spikes/s1/build-env.sh && cargo test -p client-core && cargo clippy -p desktop -p client-core --all-targets -- -D warnings && cargo fmt -p desktop --check`
Expected: all pass, clean.

- [ ] **Step 9: e2e (no new checks; the legend moved).**
Run: `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/t2-e2e tooling/smoke/run.sh`
Expected: `== 0 of N checks failed`. Look at `/data/scratch/tmls-rs-v0/night2/t2-e2e/01-start.png`: the legend has six lines, `• unread` among them.

- [ ] **Step 10: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests
git commit -m "feat(desktop): sessions with output you have not read get a • and Alt+Shift+N visits them"
```

---

### Task 3: F1 — `?` help panel and `tmls-rs keys`

**Files:**
- Modify: `crates/desktop/src/keys.rs:1-35` (module comment), `:233-266` (`ListKey`, `list_key`); add `HELP`, `help_text`, `help_panel` at the end
- Modify: `crates/desktop/src/app.rs:146-153` (`Modal`), `:803-808` (`build_view`), `:951-968` (`form_key`), `:1538-1576` (`modal_click`), `:1641-1664` (`list_key`), `:1707-1733` (`foot_button`)
- Modify: `crates/desktop/src/main.rs:8-10,36-57`
- Modify: `tooling/smoke/e2e.sh` (block before `# --- quit`)
- Test: `crates/desktop/tests/keys.rs`

**Interfaces:**
- Consumes: `forms::Panel`, `forms::Look` (exist; `Panel::new`, `Panel::put` are `pub(crate)`).
- Produces:
  - `keys::HELP: &[(&str, &[(&str, &str)])]` — (group, [(chord, what)]); the groups are `Everywhere`, `List`, `Terminal`, `Panel`, `Forms`, in that order. Later tasks add rows to it.
  - `keys::help_text() -> String`, `keys::help_panel() -> forms::Panel`
  - `keys::ListKey::Help` (`?` in the list, Shift allowed)
  - app: `Modal::Help(forms::Panel)`; `Running::open_help()`

- [ ] **Step 1: Failing tests.** In `crates/desktop/tests/keys.rs` add `HELP, help_panel, help_text` to the `desktop::keys` import and append:

```rust
#[test]
fn question_mark_in_the_list_opens_the_help() {
    assert_eq!(list_key(&ch('?', SHIFT)), Some(ListKey::Help));
    assert_eq!(list_key(&ch('?', NONE)), Some(ListKey::Help));
    assert_eq!(list_key(&repeat(&ch('?', SHIFT))), None);
    assert_eq!(list_key(&ch('?', CTRL)), None);
    assert_eq!(list_key(&ch('?', ALT_SHIFT)), None);
}

/// The chords of a group, split into their words (`j / k  ↑ / ↓` → j, k, ↑, ↓).
fn chords(group: &str) -> Vec<&'static str> {
    let rows = HELP.iter().find(|(name, _)| *name == group).expect(group).1;
    rows.iter()
        .flat_map(|(chord, _)| chord.split([' ', '/']))
        .filter(|word| !word.is_empty())
        .collect()
}

#[test]
fn the_help_names_every_key_of_the_list_and_every_alt_shift_letter() {
    let list = chords("List");
    for c in ['j', 'k', 'p', 'n', 'c', 'm', 'a', '?'] {
        let mods = if c == '?' { SHIFT } else { NONE };
        assert!(list_key(&ch(c, mods)).is_some(), "{c} is a key of the list");
        assert!(list.contains(&c.to_string().as_str()), "{c} is in the help");
    }
    let everywhere: Vec<&str> = HELP[0].1.iter().map(|(chord, _)| *chord).collect();
    assert_eq!(HELP[0].0, "Everywhere");
    for c in ['n', 'w', 'x', 'f'] {
        assert!(global(&ch(c, ALT_SHIFT), Focus::Terminal).is_some(), "Alt+Shift+{c} is bound");
        let chord = format!("Alt+Shift+{}", c.to_ascii_uppercase());
        assert!(everywhere.contains(&chord.as_str()), "{chord} is in the help");
    }
}

#[test]
fn the_help_panel_shows_every_row_and_fits_a_small_window() {
    let panel = help_panel();
    let texts: Vec<&str> = panel.cells.iter().map(|cell| cell.text.as_str()).collect();
    for (group, rows) in HELP {
        assert!(texts.contains(group), "{group}");
        for (chord, what) in *rows {
            assert!(texts.contains(chord), "{chord} uncut");
            assert!(texts.contains(what), "{what} uncut");
        }
    }
    assert!(panel.lines <= 40, "{} lines", panel.lines);
    assert!(panel.cols <= 110, "{} columns", panel.cols);
}

#[test]
fn help_text_is_the_same_table() {
    let text = help_text();
    for (group, rows) in HELP {
        assert!(text.contains(&format!("{group}\n")));
        for (chord, what) in *rows {
            assert!(text.lines().any(|line| line.trim_start().starts_with(chord) && line.ends_with(what)), "{chord}");
        }
    }
}
```

- [ ] **Step 2: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test keys`
Expected: compile errors (`HELP`, `ListKey::Help`).

- [ ] **Step 3: Implement in `keys.rs`.** `ListKey` gains:

```rust
    /// The keys and mouse actions (`?`).
    Help,
```

`list_key` (251-266) becomes:

```rust
pub fn list_key(ev: &KeyEvent) -> Option<ListKey> {
    if !ev.pressed {
        return None;
    }
    // `?` is Shift+/ on most layouts: Shift may be down.
    let plain = !ev.mods.ctrl && !ev.mods.alt && !ev.mods.logo;
    if ev.key == Key::Char('?') && plain && !ev.repeat {
        return Some(ListKey::Help);
    }
    if ev.mods != NONE {
        return None;
    }
    match ev.key {
        Key::Named(Named::Up) | Key::Char('k') => Some(ListKey::Up),
        Key::Named(Named::Down) | Key::Char('j') => Some(ListKey::Down),
        Key::Named(Named::Enter) if !ev.repeat => Some(ListKey::Open),
        Key::Char('p') if !ev.repeat => Some(ListKey::Pin),
        Key::Char('n') if !ev.repeat => Some(ListKey::NextWaiting),
        Key::Char('c') if !ev.repeat => Some(ListKey::New),
        Key::Char('m') if !ev.repeat => Some(ListKey::Menu),
        Key::Char('a') if !ev.repeat => Some(ListKey::Alerts),
        _ => None,
    }
}
```

At the end of `keys.rs`:

```rust
/// Every key and mouse action, grouped: the help panel (`?`) and `tmls-rs keys` print this
/// one table, so neither can drift from the other. Each description fits 31 columns.
pub const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Everywhere",
        &[
            ("Ctrl+Q", "quit; the sessions keep running"),
            ("Ctrl+= / Ctrl+−", "font size"),
            ("Alt+Shift+→ / ←", "next / previous pinned or tab"),
            ("Alt+Shift+↑", "the keys to the session list"),
            ("Alt+Shift+N", "next that waits, else unread"),
            ("Alt+Shift+W", "close the tab (session lives)"),
            ("Alt+Shift+F", "images this session made"),
            ("Alt+Shift+X", "close the file panel"),
        ],
    ),
    (
        "List",
        &[
            ("j / k  ↑ / ↓", "move"),
            ("Enter", "attach"),
            ("Esc", "keys back to the terminal"),
            ("p", "pin or unpin (★)"),
            ("n", "next session that waits"),
            ("c", "new session on this host"),
            ("m", "the session's menu"),
            ("a", "alerts"),
            ("?", "this help"),
            ("click", "attach; on + a new session"),
            ("right-click", "the session's menu"),
            ("middle-click", "pin or unpin"),
        ],
    ),
    (
        "Terminal",
        &[
            ("Ctrl+Shift+C", "copy the selection"),
            ("Ctrl+Shift+V", "paste"),
            ("Ctrl+C", "copies when text is selected"),
            ("drag", "select (Shift+drag in tmux)"),
            ("Ctrl+click", "open a link or a file"),
            ("Shift+wheel", "scroll here, not in tmux"),
            ("Enter", "reattach a tab that ended"),
        ],
    ),
    (
        "Panel",
        &[
            ("j / k", "move in the image list"),
            ("Enter", "open the picture"),
            ("Backspace / Esc", "back"),
            ("drag the divider", "the panel's width"),
        ],
    ),
    (
        "Forms",
        &[
            ("Tab / Shift+Tab", "next / previous field"),
            ("Enter", "the focused button"),
            ("Esc", "cancel"),
            ("↑ / ↓", "choose"),
            ("Ctrl+Shift+V", "paste into the field"),
        ],
    ),
];

/// Columns of a chord and of its description in the help panel.
const HELP_CHORD_COLS: usize = 20;
const HELP_WHAT_COLS: usize = 32;
/// The groups of the left column; the rest go right.
const HELP_LEFT_GROUPS: usize = 2;

/// `HELP` as plain text, for `tmls-rs keys`.
pub fn help_text() -> String {
    let mut out = String::new();
    for (group, rows) in HELP {
        out.push_str(group);
        out.push('\n');
        for (chord, what) in *rows {
            out.push_str(&format!("  {chord:<w$}{what}\n", w = HELP_CHORD_COLS));
        }
        out.push('\n');
    }
    out
}

/// `HELP` as a panel: a title line, then the groups in two columns.
pub fn help_panel() -> crate::forms::Panel {
    use crate::forms::{Look, Panel};
    let column = HELP_CHORD_COLS + HELP_WHAT_COLS;
    let cols = 2 + column + 2 + column + 2;
    let mut p = Panel::new(cols);
    p.put(0, 2, cols - 4, "Keys and mouse · Esc or ? closes", Look::Title);
    let mut bottom = 0;
    for (index, groups) in [&HELP[..HELP_LEFT_GROUPS], &HELP[HELP_LEFT_GROUPS..]].into_iter().enumerate() {
        let x = 2 + index * (column + 2);
        let mut line = 2;
        for (group, rows) in groups {
            p.put(line, x, column, group, Look::Label);
            line += 1;
            for (chord, what) in *rows {
                p.put(line, x, HELP_CHORD_COLS - 1, chord, Look::Text);
                p.put(line, x + HELP_CHORD_COLS, HELP_WHAT_COLS, what, Look::Dim);
                line += 1;
            }
            line += 1;
        }
        bottom = bottom.max(line);
    }
    p.lines = bottom;
    p
}
```

Add to the module comment's bindings list: "- `?` in the list or the `?` button — the keys and mouse actions (`HELP`); Esc, `?` or a click beside it closes it".

- [ ] **Step 4: Run the keys tests.**
Run: `tooling/docker/build.sh test -p desktop --test keys`
Expected: pass.

- [ ] **Step 5: Wire the panel in `app.rs`.** `Modal` gains `Help(forms::Panel),`. `build_view`: `Modal::Help(panel) => (panel.clone(), *at),`. New method:

```rust
    /// `?` in the list or the `?` button: the keys and mouse actions over the window.
    fn open_help(&mut self) {
        self.open_modal(Modal::Help(keys::help_panel()), None);
    }
```

`form_key` starts with:

```rust
        if let Some((Modal::Help(_), _)) = &self.modal {
            let closes = matches!(
                ev.key,
                s2_core::Key::Named(s2_core::Named::Escape) | s2_core::Key::Char('?')
            );
            if ev.pressed && closes {
                self.close_modal();
            }
            self.invalidate();
            return;
        }
```

and its `None => {}` arm becomes `Some((Modal::Help(_), _)) | None => {}`. `modal_click` gains:

```rust
            Modal::Help(panel) => {
                let rect = self.layout.panel_rect(panel.cols, panel.lines, *at);
                let (cx, cy) = self.layout.panel_cell(rect, x, y);
                if !panel.contains(cx, cy) {
                    self.close_modal();
                }
            }
```

`list_key` gains `ListKey::Help => self.open_help(),`. `foot_button`: the arm `(Button::Help, _) | (Button::Open | Button::Copy, None) => {}` becomes `(Button::Help, _) => self.open_help(),` and `(Button::Open | Button::Copy, None) => {}`.

- [ ] **Step 6: `tmls-rs keys`** in `main.rs`. `USAGE`:

```rust
const USAGE: &str = "usage: tmls-rs [--bench <dir>] [HOST…]\n       tmls-rs keys\n\
  HOST…          ssh hosts to list instead of those in ~/.config/tmls/hosts\n\
  --bench <dir>  write a latency log for the first tab opened to <dir>/latency.jsonl\n\
  keys           print the keys and mouse actions, and exit";
```

In `main`, after `remove_var("TMUX")`:

```rust
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["keys"] {
        use std::io::Write;
        // Not `print!`: a closed pipe (`tmls-rs keys | head`) must not panic.
        let _ = std::io::stdout().write_all(desktop::keys::help_text().as_bytes());
        return ExitCode::SUCCESS;
    }
    let args = match parse(args) {
```

- [ ] **Step 7: e2e block.** In `tooling/smoke/e2e.sh`, immediately before `# --- quit ---…`:

```bash
# --- the keys' help (F1) -------------------------------------------------------------------------
# "$ODD" is the only tab, in front, with the keys.
if "$BIN" keys | grep -qF 'Alt+Shift+N'; then
  pass "tmls-rs keys prints the key table"
else
  fail "tmls-rs keys prints the key table" "$("$BIN" keys 2>&1 | head -3 | tr '\n' ' ')"
fi
key_focus_list
NO_HELP=$(picture)
keys "?"
HELP_PICTURE=$(picture)
check "screenshot 34-help" shot 34-help
if [ "$HELP_PICTURE" != "$NO_HELP" ]; then
  pass "? in the list opens the help (the picture changed)"
else
  fail "? in the list opens the help (the picture changed)" "same picture"
fi
keys -k Escape
sleep 0.5
if [ "$(picture)" != "$HELP_PICTURE" ]; then
  pass "Esc closes the help"
else
  fail "Esc closes the help" "the picture is the help's"
fi
# Esc again: the keys go back to the terminal of "$ODD".
keys -k Escape
check_typed "after the help the keys go to \"$ODD\" again" "$ODD" afterhelp
```

- [ ] **Step 8: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; clippy and fmt as in Global Constraints; `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/t3-e2e tooling/smoke/run.sh`
Expected: all pass; look at `/data/scratch/tmls-rs-v0/night2/t3-e2e/34-help.png` (two columns, nothing cut).

- [ ] **Step 9: Commit.**

```bash
git add crates/desktop/src/keys.rs crates/desktop/src/app.rs crates/desktop/src/main.rs crates/desktop/tests/keys.rs tooling/smoke/e2e.sh
git commit -m "feat(desktop): ? shows the keys and mouse actions; tmls-rs keys prints them"
```

---

### Task 4: F2 — copy and paste that work and say what happened

**Files:**
- Create: `crates/desktop/src/clip.rs`
- Modify: `crates/desktop/src/lib.rs` (add `pub mod clip;` and its line in the module list)
- Modify: `crates/client-core/src/tmux.rs:13` (import `Write`), `:295-341` (`run`, `run_bytes` → `run_input`; add `run_with_stdin`)
- Modify: `crates/desktop/src/forms.rs:906-908` (add `InputRunner`)
- Modify: `crates/desktop/src/app.rs:383` (`clipboard` field), `:576-584` (init), `:866-884` (`key`: Copy/Paste), `:1507-1535` (`copy_attach`, `paste_into_form`), `:1892-1920`, `:1946`, `:1957-1967` (`clipboard`, `copy`, `set_clipboard`, `paste`)
- Modify: `tooling/smoke/Dockerfile.smoke:13` (add `wl-clipboard`), `tooling/smoke/e2e.sh` (block before `# --- quit`)
- Test: `crates/desktop/tests/clip.rs` (new), `crates/client-core/tests/tmux.rs`

**Interfaces:**
- Consumes: `forms::Runner`, `forms::on_path` (exist).
- Produces:
  - `client_core::tmux::run_with_stdin(argv: &[String], input: &[u8], timeout: Duration) -> anyhow::Result<String>` (Task 7 sends messages with it)
  - `forms::InputRunner<'a> = &'a dyn Fn(&[String], &[u8]) -> anyhow::Result<String>` (Task 7 uses it)
  - `clip::copy_with(text: Option<String>, set: impl FnOnce(&str) -> Result<(), String>) -> Result<String, String>`
  - `clip::paste_with(got: Result<String, String>, put: impl FnOnce(&str)) -> Result<String, String>`
  - `clip::wl_copy(text: &str, feed: InputRunner) -> Result<(), String>`, `clip::wl_paste(run: Runner) -> Result<String, String>`
  - `clip::Clipboard { new() -> Clipboard, set(&mut self, &str) -> Result<(), String>, get(&mut self) -> Result<String, String> }`
- Departs from the spec: `run_with_stdin` is the spec's F7 item; it lands here because the `wl-copy` fallback needs it first.

- [ ] **Step 1: Failing tests.** New `crates/desktop/tests/clip.rs`:

```rust
//! What a copy or a paste says in the status line, and the wl-clipboard fallback's commands. No
//! clipboard and no compositor here: the clipboard is a stand-in.

use std::cell::RefCell;

use desktop::clip::{copy_with, paste_with, wl_copy, wl_paste};

#[test]
fn copy_says_what_happened() {
    assert_eq!(copy_with(Some("héllo".into()), |_| Ok(())), Ok("copied 5 chars".to_string()));
    assert_eq!(copy_with(None, |_| panic!("nothing to set")), Ok("nothing selected".to_string()));
    assert_eq!(copy_with(Some(String::new()), |_| panic!("nothing to set")), Ok("nothing selected".to_string()));
    assert_eq!(
        copy_with(Some("x".into()), |_| Err("no seat".into())),
        Err("copy failed: no seat".to_string())
    );
}

#[test]
fn paste_says_what_happened() {
    let mut put = String::new();
    assert_eq!(paste_with(Ok("ab日".into()), |text| put.push_str(text)), Ok("pasted 3 chars".to_string()));
    assert_eq!(put, "ab日");
    assert_eq!(paste_with(Ok(String::new()), |_| panic!("nothing to paste")), Ok("clipboard empty".to_string()));
    assert_eq!(
        paste_with(Err("no seat".into()), |_| panic!("nothing to paste")),
        Err("paste failed: no seat".to_string())
    );
}

#[test]
fn wl_copy_feeds_the_text_on_stdin() {
    let got = RefCell::new(None);
    let feed = |argv: &[String], input: &[u8]| -> anyhow::Result<String> {
        *got.borrow_mut() = Some((argv.to_vec(), input.to_vec()));
        Ok(String::new())
    };
    assert_eq!(wl_copy("a'b\n", &feed), Ok(()));
    assert_eq!(got.into_inner(), Some((vec!["wl-copy".to_string()], b"a'b\n".to_vec())));
    let broken = |_: &[String], _: &[u8]| -> anyhow::Result<String> { anyhow::bail!("no wayland") };
    assert_eq!(wl_copy("x", &broken), Err("no wayland".to_string()));
}

#[test]
fn wl_paste_reads_without_a_newline_and_empty_is_no_error() {
    let text = |argv: &[String]| -> anyhow::Result<String> {
        assert_eq!(argv, ["wl-paste", "--no-newline"]);
        Ok("x".into())
    };
    assert_eq!(wl_paste(&text), Ok("x".to_string()));
    let empty = |_: &[String]| -> anyhow::Result<String> { anyhow::bail!("Nothing is copied") };
    assert_eq!(wl_paste(&empty), Ok(String::new()));
    let broken = |_: &[String]| -> anyhow::Result<String> { anyhow::bail!("Failed to connect to a Wayland server") };
    assert_eq!(wl_paste(&broken), Err("Failed to connect to a Wayland server".to_string()));
}
```

Append to `crates/client-core/tests/tmux.rs` (add `run_with_stdin` to the `client_core::tmux` import):

```rust
#[test]
fn run_with_stdin_gives_the_command_its_input() {
    let argv = v(&["sh", "-c", "cat; echo; echo done"]);
    assert_eq!(run_with_stdin(&argv, b"a'b $(x)\n", Duration::from_secs(5)).unwrap(), "a'b $(x)\n\ndone\n");
    let fails = run_with_stdin(&v(&["sh", "-c", "cat >/dev/null; echo nope >&2; exit 3"]), b"x", Duration::from_secs(5));
    assert_eq!(fails.unwrap_err().to_string(), "nope");
    // A megabyte does not block: the command reads while the input is written.
    let big = vec![b'x'; 1 << 20];
    assert_eq!(run_with_stdin(&v(&["wc", "-c"]), &big, Duration::from_secs(5)).unwrap().trim(), "1048576");
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test tmux` and `tooling/docker/build.sh test -p desktop --test clip`
Expected: compile errors (`run_with_stdin`, module `clip`).

- [ ] **Step 3: `run_with_stdin`** in `tmux.rs`. Import `std::io::{ErrorKind, Read, Write}`. Replace `run_bytes`'s body and add:

```rust
/// [`run`], with what the command printed as the bytes they are: for the contents of a file,
/// which need not be text.
pub fn run_bytes(argv: &[String], timeout: Duration) -> anyhow::Result<Vec<u8>> {
    run_input(argv, None, timeout)
}

/// [`run`] with `input` on the command's stdin (a message for tmux's paste buffer, a text for
/// `wl-copy`). The input is written on a thread of its own and its end closes stdin.
pub fn run_with_stdin(argv: &[String], input: &[u8], timeout: Duration) -> anyhow::Result<String> {
    run_input(argv, Some(input), timeout).map(|out| String::from_utf8_lossy(&out).into_owned())
}

fn run_input(argv: &[String], input: Option<&[u8]>, timeout: Duration) -> anyhow::Result<Vec<u8>> {
    let (program, args) = argv.split_first().context("empty command")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0) // its own, so a timeout kills what it started as well
        .spawn()
        .with_context(|| format!("couldn't start {program}"))?;
    if let (Some(input), Some(mut pipe)) = (input, child.stdin.take()) {
        let input = input.to_vec();
        // Not on this thread: a command that writes before it has read everything would wait
        // for us while we wait for it.
        std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
```

followed by the rest of the old `run_bytes` body unchanged (from `let (stdout, stderr) = (drain(…` to the final `bail!`). Update the doc comment of `run`: "The command reads nothing: its stdin is /dev/null (see [`run_with_stdin`])".

- [ ] **Step 4: `InputRunner`** in `forms.rs`, after `pub type Runner`:

```rust
/// Runs a command with bytes on its stdin and returns what it printed;
/// `tmux::run_with_stdin` in the app.
pub type InputRunner<'a> = &'a dyn Fn(&[String], &[u8]) -> anyhow::Result<String>;
```

- [ ] **Step 5: `clip.rs`.**

```rust
//! Copy and paste: `arboard` (Wayland data-control, X11) first; when it fails on Wayland,
//! `wl-copy` / `wl-paste` as child processes if they are installed. Every outcome is a line for
//! the status line (`copy_with`, `paste_with`): what was copied or pasted, that there was
//! nothing, or why it failed — nothing goes only to stderr any more.

use std::time::Duration;

use client_core::tmux;

use crate::forms::{self, InputRunner, Runner};

/// How long `wl-copy` / `wl-paste` may take.
const WL_TIMEOUT: Duration = Duration::from_secs(3);

/// The status line after a copy of `text` (`None` or empty: nothing was selected) through `set`.
pub fn copy_with(
    text: Option<String>,
    set: impl FnOnce(&str) -> Result<(), String>,
) -> Result<String, String> {
    let Some(text) = text.filter(|text| !text.is_empty()) else {
        return Ok("nothing selected".to_string());
    };
    match set(&text) {
        Ok(()) => Ok(format!("copied {} chars", text.chars().count())),
        Err(e) => Err(format!("copy failed: {e}")),
    }
}

/// The status line after a paste: `got` is what the clipboard gave, `put` takes it when there
/// is some.
pub fn paste_with(got: Result<String, String>, put: impl FnOnce(&str)) -> Result<String, String> {
    match got {
        Ok(text) if text.is_empty() => Ok("clipboard empty".to_string()),
        Ok(text) => {
            put(&text);
            Ok(format!("pasted {} chars", text.chars().count()))
        }
        Err(e) => Err(format!("paste failed: {e}")),
    }
}

/// `wl-copy`, the text on its stdin.
pub fn wl_copy(text: &str, feed: InputRunner) -> Result<(), String> {
    feed(&["wl-copy".to_string()], text.as_bytes())
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

/// `wl-paste --no-newline`; an empty clipboard is an empty text, not an error.
pub fn wl_paste(run: Runner) -> Result<String, String> {
    match run(&["wl-paste".to_string(), "--no-newline".to_string()]) {
        Ok(text) => Ok(text),
        Err(e) if format!("{e:#}").contains("Nothing is copied") => Ok(String::new()),
        Err(e) => Err(format!("{e:#}")),
    }
}

/// The window's clipboard.
pub struct Clipboard {
    arboard: Option<arboard::Clipboard>,
    /// Why arboard could not be had, once it was tried.
    broken: Option<String>,
    /// On Wayland with `wl-copy` and `wl-paste` on `PATH`: they are the fallback.
    wl: bool,
}

impl Default for Clipboard {
    fn default() -> Clipboard {
        Clipboard::new()
    }
}

impl Clipboard {
    pub fn new() -> Clipboard {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        Clipboard {
            arboard: None,
            broken: None,
            wl: wayland && forms::on_path("wl-copy") && forms::on_path("wl-paste"),
        }
    }

    fn arboard(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.arboard.is_none() && self.broken.is_none() {
            match arboard::Clipboard::new() {
                Ok(clipboard) => self.arboard = Some(clipboard),
                Err(e) => self.broken = Some(e.to_string()),
            }
        }
        match (&mut self.arboard, &self.broken) {
            (Some(clipboard), _) => Ok(clipboard),
            (None, Some(why)) => Err(why.clone()),
            (None, None) => Err("no clipboard".to_string()),
        }
    }

    /// Puts `text` on the clipboard; the error says what arboard and `wl-copy` said.
    pub fn set(&mut self, text: &str) -> Result<(), String> {
        let first = match self.arboard() {
            Ok(clipboard) => match clipboard.set_text(text.to_string()) {
                Ok(()) => return Ok(()),
                Err(e) => e.to_string(),
            },
            Err(e) => e,
        };
        if !self.wl {
            return Err(first);
        }
        let feed = |argv: &[String], input: &[u8]| tmux::run_with_stdin(argv, input, WL_TIMEOUT);
        wl_copy(text, &feed).map_err(|e| format!("{first}; wl-copy: {e}"))
    }

    /// The clipboard's text; empty when it has none.
    pub fn get(&mut self) -> Result<String, String> {
        let first = match self.arboard() {
            Ok(clipboard) => match clipboard.get_text() {
                Ok(text) => return Ok(text),
                Err(arboard::Error::ContentNotAvailable) => return Ok(String::new()),
                Err(e) => e.to_string(),
            },
            Err(e) => e,
        };
        if !self.wl {
            return Err(first);
        }
        let run = |argv: &[String]| tmux::run(argv, WL_TIMEOUT);
        wl_paste(&run).map_err(|e| format!("{first}; wl-paste: {e}"))
    }
}
```

`lib.rs`: `pub mod clip;` and the module list line "- `clip` — copy and paste: arboard, the wl-clipboard fallback, what the status line says."

- [ ] **Step 6: Run the unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test clip` and `source spikes/s1/build-env.sh && cargo test -p client-core --test tmux`
Expected: pass.

- [ ] **Step 7: Write the e2e block first, then wire.** `Dockerfile.smoke` line 13: `sway grim wtype wl-clipboard \`. In `e2e.sh` before `# --- quit`:

```bash
# --- copy and paste (F2) -------------------------------------------------------------------------
# "$ODD" is in front with the keys; tmux's mouse is off, so a drag selects here. wl-paste reads
# the compositor's clipboard on its own, apart from the app.
clipboard() { timeout 5 wl-paste --no-newline 2>/dev/null; }
clipboard_has() { clipboard | grep -qF -- "$1"; }
keys "clear; printf '  COPYME-%s\n' $NONCE"
key_enter
if wait_for 5 pane_has_line "$ODD" "  COPYME-$NONCE"; then
  pass "the line to copy is on the pane's first row"
else
  fail "the line to copy is on the pane's first row" "pane: $(pane_text "$ODD" | head -2 | tr '\n' '|')"
fi
# Shift held while the pointer drags along the first row (y 31), from its start.
timeout 10 wtype -M shift -s 3000 -m shift &
HOLD=$!
sleep 0.8
click drag 230 31 700
wait "$HOLD" 2>/dev/null
# Something else on the clipboard first: only Ctrl+Shift+C can put the line back.
timeout 5 wl-copy "not-it-$NONCE"
keys -M ctrl -M shift -k c -m shift -m ctrl
if wait_for 5 clipboard_has "COPYME-$NONCE"; then
  pass "Shift+drag, then Ctrl+Shift+C puts the selection on the clipboard (wl-paste)"
else
  fail "Shift+drag, then Ctrl+Shift+C puts the selection on the clipboard (wl-paste)" \
    "clipboard: $(clipboard | head -c 80); stderr: $(grep -E 'copy|clipboard' "$OUT/app.stderr" | tail -3 | tr '\n' ' ')"
fi
check "screenshot 35-copied (the status line says copied N chars)" shot 35-copied
keys "echo PASTED:"
keys -M ctrl -M shift -k v -m shift -m ctrl
key_enter
pasted() { pane_text "$ODD" | grep -qE "^PASTED: ?COPYME-$NONCE\$"; }
if wait_for 5 pasted; then
  pass "Ctrl+Shift+V pastes it into the tab"
else
  fail "Ctrl+Shift+V pastes it into the tab" "pane: $(pane_text "$ODD" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
check "no copy or paste failure on stderr" bash -c '! grep -qE "(copy|paste) failed" "$0"' "$OUT/app.stderr"
```

Run it against the **unchanged** app to see what fails and why:
Run: `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/t4-diagnose tooling/smoke/run.sh`
Expected: record which of the three F2 checks fail and the matching lines of `/data/scratch/tmls-rs-v0/night2/t4-diagnose/app.stderr` (`copy failed: …`, `no clipboard: …`, `paste failed: …`) in `/data/scratch/tmls-rs-v0/night2/t4-diagnose/CAUSE.txt`. Check the spec's candidates in order: (1) the key match — `s2_wgpu::input::key_action` (`spikes/s2-wgpu/src/input.rs:53-66`) needs mods exactly Ctrl+Shift and `C`/`c`; does the app get `Action::Copy` at all (add a temporary `crate::errln` in `key()` if stderr says nothing; remove it before commit)? (2) arboard's error text on `wlr-data-control`; (3) `selected_text()` empty after the drag. The cause found goes into the commit body.

- [ ] **Step 8: Wire `clip` into `app.rs`.** Field `clipboard: Option<arboard::Clipboard>` → `clip: clip::Clipboard` (init `clip: clip::Clipboard::new()`; import `crate::clip`). Delete the method `clipboard()` (1892-1900). Then:

```rust
    /// Ctrl+Shift+C: copies the front tab's selection, and says so.
    fn copy(&mut self) {
        let text = self
            .tabs
            .active()
            .and_then(|index| self.tabs.session(index).selected_text());
        let said = clip::copy_with(text, |text| self.clip.set(text));
        self.say(said);
    }

    /// A selection made with the mouse, or Ctrl+C on one: to the clipboard, and says so.
    fn set_clipboard(&mut self, text: String) {
        let said = clip::copy_with(Some(text), |text| self.clip.set(text));
        self.say(said);
    }

    /// Ctrl+Shift+V: pastes the clipboard into the front tab, if its session is live.
    fn paste(&mut self) {
        if self.tabs.live_mut().is_none() {
            return self.say(Ok("no live session to paste into".to_string()));
        }
        let got = self.clip.get();
        let said = clip::paste_with(got, |text| {
            if let Some(session) = self.tabs.live_mut() {
                session.paste(text);
            }
        });
        self.say(said);
    }
```

`copy_attach`:

```rust
        let local = self.cfg.local_name.clone();
        let said = forms::copy_attach(key, &local, &mut |text| self.clip.set(&text));
        self.say(said);
```

`paste_into_form`:

```rust
    fn paste_into_form(&mut self) {
        let text = match self.clip.get() {
            Ok(text) if !text.is_empty() => text,
            Ok(_) => return self.say(Ok("clipboard empty".to_string())),
            Err(e) => return self.say(Err(format!("paste failed: {e}"))),
        };
        if let Some((Modal::Form(form), _)) = &mut self.modal {
            form.paste(&text);
            self.lookup_name();
            self.invalidate();
        }
    }
```

In `key()` (866-884) the copy/paste routing becomes — the selection is the front tab's whatever has the keys, a paste goes to the terminal only:

```rust
        let ev = match action {
            Action::Send(ev) => ev,
            Action::Paste if self.focus == Focus::Form => {
                self.paste_into_form();
                return After::Stay;
            }
            Action::Copy if self.focus != Focus::Form => {
                self.copy();
                return After::Stay;
            }
            Action::Paste if self.focus == Focus::Terminal => {
                self.paste();
                return After::Stay;
            }
            Action::Paste => {
                self.say(Ok("paste goes to the terminal: click it first".to_string()));
                return After::Stay;
            }
            Action::Copy | Action::Ignore => return After::Stay,
        };
```

(The existing line 866-868 that leaves the panel on Copy/Paste stays above it.) Then add whatever the diagnosis of Step 7 showed is still missing (for example, a key match that missed `Action::Copy`); if the diagnosis pointed at `s2-wgpu`, fix it there in the same commit with a unit test in its own `tests/` next to the existing `key_action` tests.

- [ ] **Step 9: Tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; clippy, fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/t4-e2e tooling/smoke/run.sh`
Expected: all pass, including the four F2 checks. Look at `/data/scratch/tmls-rs-v0/night2/t4-e2e/35-copied.png`: the status line reads `copied N chars`.

- [ ] **Step 10: Commit** (the body names the cause from `CAUSE.txt`):

```bash
git add crates/client-core/src/tmux.rs crates/client-core/tests/tmux.rs crates/desktop/src/clip.rs crates/desktop/src/lib.rs \
  crates/desktop/src/forms.rs crates/desktop/src/app.rs crates/desktop/tests/clip.rs tooling/smoke/Dockerfile.smoke tooling/smoke/e2e.sh
git commit -m "fix(desktop): copy and paste fall back to wl-copy and say what happened" \
  -m "Cause on headless sway: <one line from /data/scratch/tmls-rs-v0/night2/t4-diagnose/CAUSE.txt>."
```

(The main agent fills the cause line from the file before committing; a commit is not made with the angle-bracket text in it.)

---

### Task 5: F5 — approvals from the list

**Files:**
- Create: `crates/client-core/src/approve.rs`, `crates/client-core/tests/approve.rs`, `crates/client-core/tests/data/approve-dialog.txt`
- Modify: `crates/client-core/src/lib.rs` (`pub mod approve;`), `crates/client-core/src/tmux.rs` (add `pane_target`, `capture_argv`, `send_keys_argv` after `running_commands_argv`)
- Modify: `crates/desktop/src/forms.rs:62-200` (`Request`, `Stop`, `Form`), `:575-800` (`impl Form`), `:831-950` (menu), `:908-930` (`request_argv`, `perform`)
- Modify: `crates/desktop/src/keys.rs` (`ListKey::Approve`, `list_key`, `list_keeps_keys`, `HELP` rows)
- Modify: `crates/desktop/src/alerts.rs:236-310` (`PanelResult::Approve`, `AlertsPanel::key`)
- Modify: `crates/desktop/src/sidebar.rs` (add `info`)
- Modify: `crates/desktop/src/app.rs:122-144` (`UserEvent`), `:194-223`, `:976-1036` (`alerts_result`, `menu_result`), `:1086-1150` (`done`), `:1641-1677` (`list_key`, `act`)
- Modify: `tooling/smoke/e2e.sh` (top variables, `cleanup`, helper `fake_claude`, block before `# --- quit`), `tooling/smoke/run.sh` (default `SMOKE_TIMEOUT` 480 → 900, its comment too)
- Test: `crates/client-core/tests/tmux.rs`, `crates/desktop/tests/forms.rs`, `crates/desktop/tests/keys.rs`, `crates/desktop/tests/alerts.rs`

**Interfaces:**
- Consumes: `SessionInfo.pane`, `tmux::valid_pane` (Task 2); `forms::Runner`, `forms::run_tmux`, `REQUEST_TIMEOUT` (exist).
- Produces:
  - `client_core::approve::{QUESTION: &str, YES_KEY: &str = "1", NO_KEY: &str = "Escape", MAX_LINES: usize = 12, request(screen: &str) -> Option<Vec<String>>, same(a: &[String], b: &[String]) -> bool, shown(dialog: &[String]) -> Vec<String>}`
  - `tmux::pane_target(key: &SessionKey, pane: Option<&str>) -> String`, `tmux::capture_argv(key: &SessionKey, pane: Option<&str>, local_name: &str) -> Vec<String>`, `tmux::send_keys_argv(key: &SessionKey, pane: Option<&str>, keys: &str, local_name: &str) -> Vec<String>`
  - `forms::Stop::No`; `forms::Form::Approve { key: SessionKey, pane: Option<String>, dialog: Vec<String>, focus: Stop }`; `Form::approve(key: SessionKey, pane: Option<String>, dialog: Vec<String>) -> Form`
  - `forms::Request::Answer { key: SessionKey, pane: Option<String>, shown: Vec<String>, yes: bool }`
  - `forms::PERMISSION_PROMPT: &str = "permission prompt"`, `forms::asks_permission(info: &SessionInfo) -> bool`, `forms::menu_items(info: Option<&SessionInfo>) -> Vec<MenuItem>`, `forms::approval(key, pane: Option<&str>, local_name, run: Runner) -> Result<Vec<String>, String>`, `forms::answer(key, pane: Option<&str>, shown: &[String], yes: bool, local_name, run: Runner) -> Result<String, String>`, `forms::first_line(e: &anyhow::Error) -> String`
  - `forms::MenuItem::Approve`; `forms::RowMenu::with(key: SessionKey, items: Vec<MenuItem>) -> RowMenu`, `RowMenu::items(&self) -> &[MenuItem]`
  - `keys::ListKey::Approve`; `alerts::PanelResult::Approve(SessionKey)`; `Sidebar::info(&self, key: &SessionKey) -> Option<&SessionInfo>`
  - app: `UserEvent::Approval(SessionKey, Option<String>, Result<Vec<String>, String>)`; `Running::approve(&mut self, key: &SessionKey)`; `Running::row_menu(&self, key: SessionKey) -> RowMenu`
  - e2e helper `fake_claude NAME SID STATUS [WAITING_FOR]` and array `FAKE_PIDS` (Tasks 6–8 use them)

- [ ] **Step 1: The fixture and failing client-core tests.** `crates/client-core/tests/data/approve-dialog.txt` — the capture tmls's `tests/test_approve.py` keeps (a real Claude prompt):

```text
● Creating empty marker file
  ⎿  $ touch /tmp/x
────────────────────────────────────────────────────────────
 Bash command
 Tip: auto mode handles these prompts for you — choose "switch to auto mode" below
 Create empty marker file
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 touch /tmp/x
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and always allow access to /tmp from this project
   4. No
 Esc to cancel · Tab to amend
```

`crates/client-core/tests/approve.rs`:

```rust
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
```

Append to `crates/client-core/tests/tmux.rs` (import `capture_argv, send_keys_argv`):

```rust
#[test]
fn capture_and_send_keys_target_claudes_pane_else_the_session() {
    let k = key(LOCAL, "w");
    assert_eq!(capture_argv(&k, Some("%7"), LOCAL), v(&["tmux", "capture-pane", "-p", "-t", "%7"]));
    assert_eq!(capture_argv(&k, None, LOCAL), v(&["tmux", "capture-pane", "-p", "-t", "=w:"]));
    // Not a pane id: it never goes into a command.
    assert_eq!(capture_argv(&k, Some("%7; reboot"), LOCAL), v(&["tmux", "capture-pane", "-p", "-t", "=w:"]));
    assert_eq!(send_keys_argv(&k, Some("%7"), "Escape", LOCAL), v(&["tmux", "send-keys", "-t", "%7", "Escape"]));
    let capture = remote(&capture_argv(&key("ph", "a b"), None, LOCAL), &SSH);
    assert_eq!(words(&capture), v(&["capture-pane", "-p", "-t", "=a b:"]));
    let send = remote(&send_keys_argv(&key("ph", "it's"), Some("%3"), "1", LOCAL), &SSH);
    assert_eq!(words(&send), v(&["send-keys", "-t", "%3", "1"]));
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test approve --test tmux`
Expected: compile errors (module `approve`, `capture_argv`).

- [ ] **Step 3: Implement.** `crates/client-core/src/approve.rs`:

```rust
//! Answering a Claude permission prompt from the list (tmls `approve.py`): the dialog is read
//! off the session's pane, shown, and answered with `1` (Yes) or Escape (No) — only if a fresh
//! capture still shows the same dialog. Pure text here; the commands are `tmux::capture_argv`
//! and `tmux::send_keys_argv`.

/// The line Claude's permission dialog asks.
pub const QUESTION: &str = "Do you want to proceed?";
/// Approves at once.
pub const YES_KEY: &str = "1";
/// Declines: the number for No differs from prompt to prompt, Escape does not.
pub const NO_KEY: &str = "Escape";
/// The most lines of a dialog the form shows.
pub const MAX_LINES: usize = 12;

/// The dialog's lines (tool, description, command) when a permission prompt is up in `screen`:
/// from the last rule (`──…`) above the last `QUESTION` down to it, trimmed, without blank
/// lines, dashed rules (`╌`) and tips. `None` when nothing asks.
pub fn request(screen: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = screen.lines().collect();
    let ask = lines.iter().rposition(|line| line.trim() == QUESTION)?;
    let top = lines[..ask]
        .iter()
        .rposition(|line| line.starts_with("──"))
        .map_or(0, |rule| rule + 1);
    let dialog = lines[top..ask]
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('╌') && !line.starts_with("Tip:"))
        .map(str::to_string)
        .collect();
    Some(dialog)
}

/// Whether two captures show the same dialog.
pub fn same(a: &[String], b: &[String]) -> bool {
    a == b
}

/// The dialog as the form shows it: at most `MAX_LINES`, the middle replaced by one line that
/// says how many are left out.
pub fn shown(dialog: &[String]) -> Vec<String> {
    if dialog.len() <= MAX_LINES {
        return dialog.to_vec();
    }
    let head = MAX_LINES / 2;
    let tail = MAX_LINES - head - 1;
    let mut out = dialog[..head].to_vec();
    out.push(format!("… {} more lines …", dialog.len() - head - tail));
    out.extend_from_slice(&dialog[dialog.len() - tail..]);
    out
}
```

`lib.rs`: `pub mod approve;` (and a line in the crate comment). `tmux.rs`, after `running_commands_argv`:

```rust
/// What a command for Claude's pane targets: the pane (`%7`) when the listing has a valid one,
/// else the session's current pane (`=NAME:`).
pub fn pane_target(key: &SessionKey, pane: Option<&str>) -> String {
    match pane.filter(|pane| valid_pane(pane)) {
        Some(pane) => pane.to_string(),
        None => format!("={}:", key.name),
    }
}

/// Prints what the pane shows now (tmls `approve._capture_script`).
pub fn capture_argv(key: &SessionKey, pane: Option<&str>, local_name: &str) -> Vec<String> {
    let target = pane_target(key, pane);
    if key.host == local_name {
        strings(&["tmux", "capture-pane", "-p", "-t", &target])
    } else {
        ssh(&key.host, format!("tmux capture-pane -p -t {}", quote(&target)))
    }
}

/// Presses `keys` (a tmux key name: `1`, `Escape`, `Enter`) in the pane.
pub fn send_keys_argv(key: &SessionKey, pane: Option<&str>, keys: &str, local_name: &str) -> Vec<String> {
    let target = pane_target(key, pane);
    if key.host == local_name {
        strings(&["tmux", "send-keys", "-t", &target, keys])
    } else {
        ssh(&key.host, format!("tmux send-keys -t {} {}", quote(&target), quote(keys)))
    }
}
```

- [ ] **Step 4: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass.

- [ ] **Step 5: Failing desktop tests.** In `crates/desktop/tests/forms.rs` (imports: add `client_core::{SessionInfo, Status, Tool}`; `desktop::forms::Stop` is imported already):

```rust
const DIALOG: &str = include_str!("../../client-core/tests/data/approve-dialog.txt");

/// A listed Claude session (`pane` from its status file).
fn info(name: &str, status: Status, reason: Option<&str>) -> SessionInfo {
    SessionInfo {
        key: key(LOCAL, name),
        status,
        reason: reason.map(str::to_string),
        tool: Tool::Claude,
        title: None,
        context_pct: None,
        attached: 0,
        last_output_age_s: None,
        pid: None,
        session_id: None,
        pane: Some("%3".to_string()),
        activity: None,
    }
}

#[test]
fn answer_sends_the_key_only_while_the_same_dialog_is_up() {
    let calls = RefCell::new(Vec::new());
    let screen = RefCell::new(DIALOG.to_string());
    let run = |argv: &[String]| -> anyhow::Result<String> {
        calls.borrow_mut().push(argv.to_vec());
        Ok(screen.borrow().clone())
    };
    let k = key(LOCAL, "w");
    let shown = forms::approval(&k, Some("%7"), LOCAL, &run).unwrap();
    assert_eq!(shown, ["Bash command", "Create empty marker file", "touch /tmp/x"]);
    assert_eq!(forms::answer(&k, Some("%7"), &shown, true, LOCAL, &run), Ok("approved w".to_string()));
    assert_eq!(calls.borrow().last(), Some(&tmux::send_keys_argv(&k, Some("%7"), "1", LOCAL)));
    assert_eq!(forms::answer(&k, Some("%7"), &shown, false, LOCAL, &run), Ok("declined w".to_string()));
    assert_eq!(calls.borrow().last(), Some(&tmux::send_keys_argv(&k, Some("%7"), "Escape", LOCAL)));
    // Claude moved on — another dialog, or none: nothing is sent.
    for now in [DIALOG.replace("touch /tmp/x", "rm -rf /tmp/y"), "❯ \n".to_string()] {
        *screen.borrow_mut() = now;
        calls.borrow_mut().clear();
        assert_eq!(
            forms::answer(&k, Some("%7"), &shown, true, LOCAL, &run),
            Err("w isn't asking that any more".to_string())
        );
        assert_eq!(*calls.borrow(), [tmux::capture_argv(&k, Some("%7"), LOCAL)], "only the capture ran");
    }
    // Nothing asks: the form never opens.
    assert_eq!(forms::approval(&k, None, LOCAL, &run), Err("w isn't asking anything".to_string()));
    let down = |_: &[String]| -> anyhow::Result<String> { anyhow::bail!("ssh: connect to host ph: timed out") };
    assert_eq!(
        forms::approval(&k, None, LOCAL, &down),
        Err("couldn't read w: ssh: connect to host ph: timed out".to_string())
    );
}

#[test]
fn the_approve_form_answers_yes_on_enter_no_on_n_and_cancels_on_esc() {
    let k = key(LOCAL, "w");
    let dialog: Vec<String> = ["Bash command", "touch /tmp/x"].map(str::to_string).to_vec();
    let answer = |yes: bool| FormResult::Submit(Request::Answer {
        key: k.clone(),
        pane: Some("%3".to_string()),
        shown: dialog.clone(),
        yes,
    });
    let form = || Form::approve(k.clone(), Some("%3".to_string()), dialog.clone());
    assert_eq!(form().focus(), Stop::Ok);
    assert_eq!(form().key(&named(Named::Enter)), answer(true));
    assert_eq!(form().key(&event(Key::Char('n'), Mods::default())), answer(false));
    assert_eq!(form().key(&named(Named::Escape)), FormResult::Cancelled);
    // A held Enter answers nothing: it may be the one that opened the form.
    let mut held = named(Named::Enter);
    held.repeat = true;
    assert_eq!(form().key(&held), FormResult::Pending);
    // Tab: Yes, No, Cancel.
    let mut f = form();
    f.key(&named(Named::Tab));
    assert_eq!(f.focus(), Stop::No);
    assert_eq!(f.key(&named(Named::Enter)), answer(false));
    // The panel shows the dialog and the three buttons.
    let texts: Vec<String> = form().panel().cells.iter().map(|cell| cell.text.clone()).collect();
    for want in ["here · w asks", "Bash command", "touch /tmp/x", " Yes ", " No ", " Cancel "] {
        assert!(texts.iter().any(|text| text == want), "{want}: {texts:?}");
    }
}

#[test]
fn menu_items_offer_approve_only_while_claude_asks() {
    assert_eq!(forms::menu_items(None), MENU.to_vec());
    assert_eq!(forms::menu_items(Some(&info("w", Status::Idle, None))), MENU.to_vec());
    assert_eq!(forms::menu_items(Some(&info("w", Status::Waiting, Some("input needed")))), MENU.to_vec());
    let asks = info("w", Status::Waiting, Some("permission prompt"));
    assert_eq!(
        forms::menu_items(Some(&asks)),
        [MenuItem::Approve, MenuItem::Rename, MenuItem::Kill, MenuItem::OpenTerminal, MenuItem::CopyAttach]
    );
    let mut menu = RowMenu::with(asks.key.clone(), forms::menu_items(Some(&asks)));
    assert!(menu.panel().cells.iter().any(|cell| cell.text == " Approve…"));
    assert_eq!(menu.key(&named(Named::Enter)), MenuResult::Pick(MenuItem::Approve));
    assert_eq!(RowMenu::new(key(LOCAL, "w")).items(), MENU);
}
```

In `crates/desktop/tests/keys.rs`: in `the_help_names_every_key_of_the_list_and_every_alt_shift_letter` add `'y'` to the list of characters, and append:

```rust
#[test]
fn y_in_the_list_approves() {
    assert_eq!(list_key(&ch('y', NONE)), Some(ListKey::Approve));
    assert_eq!(list_key(&repeat(&ch('y', NONE))), None);
    assert_eq!(list_key(&ch('Y', SHIFT)), None);
}
```

In `crates/desktop/tests/alerts.rs` (import `desktop::alerts::{PanelResult, Settings}` if not yet):

```rust
#[test]
fn y_on_an_alert_asks_to_approve_its_session() {
    let mut alerts = Alerts::new(Settings::default());
    let idle = session(HOST, "ask", Status::Idle);
    alerts.observe(HOST, true, [&idle], AWAY, Instant::now());
    let asking = SessionInfo {
        reason: Some("permission prompt".to_string()),
        ..session(HOST, "ask", Status::Waiting)
    };
    alerts.observe(HOST, true, [&asking], AWAY, Instant::now());
    let mut panel = alerts.open(Instant::now());
    assert_eq!(panel.key(&press(Key::Char('y'))), PanelResult::Approve(key(HOST, "ask")));
}
```

- [ ] **Step 6: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test forms --test keys --test alerts`
Expected: compile errors (`Request::Answer`, `Form::approve`, `menu_items`, `ListKey::Approve`, `PanelResult::Approve`).

- [ ] **Step 7: Implement in `forms.rs`.** Imports: `use client_core::{KITTY, SessionInfo, SessionKey, Status, approve};`. Then:

`Request` gains:

```rust
    /// Yes (`1`) or No (Escape) to the dialog `shown`, if the pane still shows it.
    Answer {
        key: SessionKey,
        pane: Option<String>,
        shown: Vec<String>,
        yes: bool,
    },
```

`Stop` gains `/// The approval's No button.\n No,`. Add `const APPROVE_STOPS: [Stop; 3] = [Stop::Ok, Stop::No, Stop::Cancel];` and `const APPROVE_COLS: usize = 76;`. `Form` gains:

```rust
    /// Claude asks for permission: its dialog, Yes / No / Cancel (tmls `approve.py`).
    Approve {
        key: SessionKey,
        pane: Option<String>,
        dialog: Vec<String>,
        focus: Stop,
    },
```

In `impl Form`:

```rust
    /// The question of Claude's permission dialog, as captured. The focus is on Yes.
    pub fn approve(key: SessionKey, pane: Option<String>, dialog: Vec<String>) -> Form {
        Form::Approve {
            key,
            pane,
            dialog,
            focus: Stop::Ok,
        }
    }
```

- `focus()`: `Form::Rename { focus, .. } | Form::ConfirmKill { focus, .. } | Form::Approve { focus, .. } => *focus`
- `error()`: `Form::ConfirmKill { .. } | Form::Approve { .. } => None`
- `stops()`: `Form::Approve { .. } => &APPROVE_STOPS`
- `focus_on()`: `Form::ConfirmKill { focus, .. } | Form::Approve { focus, .. } => *focus = stop`
- `submit()`:

```rust
            Form::Approve {
                key,
                pane,
                dialog,
                focus,
            } => FormResult::Submit(Request::Answer {
                key: key.clone(),
                pane: pane.clone(),
                shown: dialog.clone(),
                yes: *focus != Stop::No,
            }),
```

- `key()`: `let on_button = matches!(self.focus(), Stop::Ok | Stop::Cancel | Stop::No);`; the per-form match gains (before the `Form::Rename { focus, .. } | Form::ConfirmKill { focus, .. }` arm), with a flag so the borrow ends before `submit`:

```rust
            Form::Approve { focus, .. } => {
                if ev.key == Key::Char('n') && !chord && !ev.repeat {
                    *focus = Stop::No;
                    decline = true;
                } else if let Some(forward) = arrow(ev) {
                    *focus = step(&APPROVE_STOPS, *focus, forward);
                }
            }
```

with `let mut decline = false;` declared before that `match self {` and, after it, `if decline { return self.submit(); }`.
- `NewForm::other_key`: add the arm `Stop::No => {}`.
- `click()`: add `(Target::Stop(Stop::No), form) => { form.focus_on(Stop::No); return form.submit(); }`.
- `panel()`:

```rust
            Form::Approve {
                key, dialog, focus, ..
            } => {
                let inner = APPROVE_COLS - 2 * PAD;
                let mut p = Panel::new(APPROVE_COLS);
                p.put(0, PAD, inner, &format!("{} · {} asks", key.host, key.name), Look::Title);
                let shown = approve::shown(dialog);
                for (n, line) in shown.iter().enumerate() {
                    let look = if n == 0 { Look::Label } else { Look::Text };
                    p.put(2 + n, PAD, inner, line, look);
                }
                let hint = 2 + shown.len() + 1;
                p.put(hint, PAD, inner, "Enter: Yes · n: No · Esc: Cancel", Look::Dim);
                let buttons = hint + 1;
                // Right to left: Cancel at the end, No, then Yes.
                let mut col = APPROVE_COLS - PAD;
                for (stop, label) in [(Stop::Cancel, " Cancel "), (Stop::No, " No "), (Stop::Ok, " Yes ")] {
                    col -= label.len();
                    let look = Look::Button {
                        focused: *focus == stop,
                        danger: false,
                    };
                    p.put(buttons, col, label.len(), label, look).target = Some(Target::Stop(stop));
                    col -= 1;
                }
                p.lines = buttons + 2;
                p
            }
```

Menu: `MenuItem` gains `Approve` (label `"Approve…"`). `RowMenu` gets a field `items: Vec<MenuItem>`; `new(key)` sets `items: MENU.to_vec()`; add

```rust
    /// The menu with these entries (`menu_items`), its first one marked.
    pub fn with(key: SessionKey, items: Vec<MenuItem>) -> RowMenu {
        RowMenu {
            key,
            items,
            hovered: Some(0),
        }
    }

    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }
```

and replace every `MENU` inside `RowMenu::key`, `click` and `panel` by `self.items` (`let n = self.items.len();`, `MenuResult::Pick(self.items[i])`, `for (index, item) in self.items.iter().enumerate()`, `p.lines = self.items.len() + 1;`).

Free functions, after `running_commands`:

```rust
/// What Claude's status file says while it waits for a permission answer.
pub const PERMISSION_PROMPT: &str = "permission prompt";

/// The session waits on a permission prompt that can be answered from here (it runs in tmux).
pub fn asks_permission(info: &SessionInfo) -> bool {
    info.status == Status::Waiting
        && info.reason.as_deref() == Some(PERMISSION_PROMPT)
        && info.key.host != KITTY
}

/// The entries of a session's menu: `Approve…` first while it asks, then the usual ones.
pub fn menu_items(info: Option<&SessionInfo>) -> Vec<MenuItem> {
    let mut items = Vec::with_capacity(MENU.len() + 2);
    if info.is_some_and(asks_permission) {
        items.push(MenuItem::Approve);
    }
    items.extend(MENU);
    items
}

/// The first line of what a command said, for the status line.
pub fn first_line(e: &anyhow::Error) -> String {
    let all = format!("{e:#}");
    all.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("it failed")
        .to_string()
}

/// The permission dialog up in the session's pane now; why not, for the status line.
pub fn approval(
    key: &SessionKey,
    pane: Option<&str>,
    local_name: &str,
    run: Runner,
) -> Result<Vec<String>, String> {
    let screen = run(&tmux::capture_argv(key, pane, local_name))
        .map_err(|e| format!("couldn't read {}: {}", key.name, first_line(&e)))?;
    approve::request(&screen).ok_or_else(|| format!("{} isn't asking anything", key.name))
}

/// Yes or No to the dialog `shown` — only if a fresh capture still shows it (D4).
pub fn answer(
    key: &SessionKey,
    pane: Option<&str>,
    shown: &[String],
    yes: bool,
    local_name: &str,
    run: Runner,
) -> Result<String, String> {
    let screen = run(&tmux::capture_argv(key, pane, local_name))
        .map_err(|e| format!("couldn't read {}: {}", key.name, first_line(&e)))?;
    if !approve::request(&screen).is_some_and(|now| approve::same(&now, shown)) {
        return Err(format!("{} isn't asking that any more", key.name));
    }
    let keys = if yes { approve::YES_KEY } else { approve::NO_KEY };
    run(&tmux::send_keys_argv(key, pane, keys, local_name))
        .map_err(|e| format!("couldn't answer {}: {}", key.name, first_line(&e)))?;
    Ok(match yes {
        true => format!("approved {}", key.name),
        false => format!("declined {}", key.name),
    })
}
```

`request_argv` gains `Request::Answer { key, pane, yes, .. } => tmux::send_keys_argv(key, pane.as_deref(), if *yes { approve::YES_KEY } else { approve::NO_KEY }, local_name),`. `perform`'s first match arm becomes `Request::Answer { key, pane, shown, yes } => return answer(key, pane.as_deref(), shown, *yes, local_name, run),` and its error formatting uses `first_line(&e)`.

`keys.rs`: `ListKey` gains `/// Approve the permission prompt of the session under the cursor.\n Approve,`; `list_key` gains `Key::Char('y') if !ev.repeat => Some(ListKey::Approve),`; `list_keeps_keys` becomes `Request::New(_) | Request::Answer { .. } => false,`. `HELP`: in `List` after `("?", "this help")` add `("y", "approve a permission prompt"),`; in `Forms` after `("↑ / ↓", "choose")` add `("n", "No, in an approval"),`. Module comment: "y — answer the cursor's session's permission prompt (Yes / No / Cancel)".

`alerts.rs`: `PanelResult` gains `/// Answer this alert's session's permission prompt.\n Approve(SessionKey),`; `AlertsPanel::key` gains

```rust
            Key::Char('y') if !ev.repeat => {
                return self.selected().map_or(PanelResult::Pending, |alert| {
                    PanelResult::Approve(alert.key.clone())
                });
            }
```

`sidebar.rs`, after `status`:

```rust
    /// The session as the list shows it, marks included; `None` when it is not listed.
    pub fn info(&self, key: &SessionKey) -> Option<&SessionInfo> {
        self.rows.iter().find_map(|row| match row {
            Row::Session(info) if info.key == *key => Some(info),
            _ => None,
        })
    }
```

- [ ] **Step 8: Run the desktop unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test forms --test keys --test alerts`
Expected: pass (the app does not compile until Step 9; if the test run builds the lib only, they pass now; otherwise after Step 9).

- [ ] **Step 9: Wire `app.rs`.** `UserEvent` gains

```rust
    /// The permission dialog read off a session's pane (its key and pane), or why not.
    Approval(SessionKey, Option<String>, Result<Vec<String>, String>),
```

and `user_event` routes `UserEvent::Approval(key, pane, result) => running.approval(key, pane, result),`. New methods:

```rust
    /// `y` in the list or the alerts, or the menu's Approve…: the session's pane is read on a
    /// thread; the dialog comes back as `UserEvent::Approval`.
    fn approve(&mut self, key: &SessionKey) {
        let listed = self
            .sidebar
            .info(key)
            .map(|info| (forms::asks_permission(info), info.pane.clone()));
        let Some((asks, pane)) = listed else {
            return self.say(Err(format!("{} is not listed any more", key.name)));
        };
        if key.host == KITTY {
            return self.say(Err(format!("{} runs outside tmux: answer it in its window", key.name)));
        }
        if !asks {
            return self.say(Err(format!("{} isn't asking anything", key.name)));
        }
        let (proxy, local, key) = (self.proxy.clone(), self.cfg.local_name.clone(), key.clone());
        std::thread::spawn(move || {
            let result = forms::approval(&key, pane.as_deref(), &local, &forms::run_tmux);
            let _ = proxy.send_event(UserEvent::Approval(key, pane, result));
        });
    }

    /// The dialog came: the form opens, unless another form or menu was opened meanwhile.
    fn approval(&mut self, key: SessionKey, pane: Option<String>, result: Result<Vec<String>, String>) {
        match result {
            Err(why) => self.say(Err(why)),
            Ok(_) if self.modal.is_some() => {
                self.say(Ok(format!("{} asks: close this, then y again", key.name)))
            }
            Ok(dialog) => self.open_modal(Modal::Form(Form::approve(key, pane, dialog)), None),
        }
    }

    /// The menu of a session row, with the entries its state allows.
    fn row_menu(&self, key: SessionKey) -> RowMenu {
        let items = forms::menu_items(self.sidebar.info(&key));
        RowMenu::with(key, items)
    }
```

- `list_key`: `ListKey::Approve => { if let Some(key) = self.sidebar.selected().cloned() { self.approve(&key); } }`; `ListKey::Menu`'s `self.open_modal(Modal::Menu(RowMenu::new(key)), None)` → `let menu = self.row_menu(key); self.open_modal(Modal::Menu(menu), None);`; the same in `act`'s `Action::Menu` (`Some(at)`).
- `menu_result`: `MenuResult::Pick(MenuItem::Approve) => { self.close_modal(); self.approve(&key); }`.
- `alerts_result`: `PanelResult::Approve(key) => { self.close_modal(); self.approve(&key); }`.
- `done`: the `match request` gains `Request::Answer { .. } => {}` (the status line and `refresh_now` above it already ran).
- `form_result` needs nothing: `Request::Answer` runs through `forms::perform` on its thread like the others, with `REQUEST_TIMEOUT`.

- [ ] **Step 10: e2e block.** At the top of `e2e.sh`, after `BYE=…`:

```bash
APPROVE='tmlsrs-test-0approve'
APPROVE_SID='1b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'
# The stand-ins for Claude processes, by session id: their status files are named after them.
declare -A FAKE_PIDS=()
```

In `cleanup()`, before the sway kill:

```bash
  tmux kill-session -t "=$APPROVE" 2>/dev/null
  for pid in "${FAKE_PIDS[@]}"; do kill "$pid" 2>/dev/null; done
```

In `run.sh`: `SMOKE_TIMEOUT` default `480` → `900` (code and comment). Before `# --- quit`:

```bash
# fake_claude NAME SID STATUS [WAITING_FOR] — (re)writes the status file of a Claude with session
# id SID that runs in NAME's pane and is STATUS. The file is named after a sleep of this script's
# (the listing skips files of dead processes); statusUpdatedAt is now.
fake_claude() {
  local name=$1 sid=$2 status=$3 waiting=${4:-} pid ids
  pid=${FAKE_PIDS[$sid]:-}
  if [ -z "$pid" ]; then
    sleep 900 &
    pid=$!
    FAKE_PIDS[$sid]=$pid
  fi
  ids=$(tmux display -p -t "=$name:" '#{window_id}.#{pane_id}')
  mkdir -p "$HOME/.claude/sessions"
  printf '{"pid":%s,"kind":"interactive","sessionId":"%s","status":"%s","waitingFor":"%s","statusUpdatedAt":%s,"tmux":"%s:%s","cwd":"/work"}\n' \
    "$pid" "$sid" "$status" "$waiting" "$(date +%s%3N)" "$name" "$ids" >"$HOME/.claude/sessions/$pid.json"
}
# top_of_list  — the list's cursor to its first session (pins are empty here).
top_of_list() { keys -k k -k k -k k -k k -k k -k k -k k -k k; }

# --- approvals from the list (F5) ----------------------------------------------------------------
# A stand-in for Claude asking: the dialog of client-core's fixture, then one key read and shown.
# $APPROVE sorts first ('0' < 'a'), so it is the list's first session.
cp /work/crates/client-core/tests/data/approve-dialog.txt "$SCRATCH/dialog.txt"
cat >"$SCRATCH/dialog.sh" <<'EOF'
clear
cat "$1"
IFS= read -rsn1 k
printf 'GOT[%s]\n' "$k"
sleep 600
EOF
tmux new-session -d -s "$APPROVE" -x 100 -y 40 "bash '$SCRATCH/dialog.sh' '$SCRATCH/dialog.txt'"
fake_claude "$APPROVE" "$APPROVE_SID" waiting "permission prompt"
sleep 6
key_focus_list
top_of_list
keys y
check "screenshot 36-approve (the dialog, Yes / No / Cancel)" shot 36-approve
key_enter
if wait_for 8 pane_has_line "$APPROVE" "GOT[1]"; then
  pass "y on a row that asks, then Enter, presses 1 in its pane"
else
  fail "y on a row that asks, then Enter, presses 1 in its pane" \
    "pane: $(pane_text "$APPROVE" | grep -v '^$' | tail -3 | tr '\n' '|'); stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
```

- [ ] **Step 11: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; `source spikes/s1/build-env.sh && cargo test -p client-core`; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/t5-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/night2/t5-e2e/36-approve.png`: title `smoke · tmlsrs-test-0approve asks`, three dialog lines, the buttons.

- [ ] **Step 12: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh tooling/smoke/run.sh
git commit -m "feat(desktop): answer Claude's permission prompts from the list (y, the row menu, the alerts)"
```

---

### Task 6: F6 — pushed status (events.jsonl) and the last reply

**Files:**
- Create: `crates/client-core/src/push.rs`, `crates/client-core/tests/push.rs`
- Modify: `crates/client-core/src/lib.rs` (`pub mod push;`), `crates/client-core/src/listing.rs:94-115` and the two `SessionInfo` literals (`last_reply`)
- Modify: `crates/client-core/src/poll.rs:47-51` (`State`), `:95-102` (add `force_next`), `:116-152` (`poll_host`); add `DEFAULT_PERIOD`, `period_from`
- Modify: `crates/desktop/src/sidebar.rs:82-97` (`summary`), `crates/desktop/src/draw.rs:960-990` (`header_spans_with`, `POLL_ONLY`), `:805-830` (`View.polled`), `:1290-1300` (header call)
- Modify: `crates/desktop/src/app.rs:103-106` (`POLL_PERIOD`), `:122-144`, `:194-223`, `:330-432`, `:535-542`, `:549-612`, `:739-772`, `:803-851`
- Modify: `tooling/smoke/e2e.sh` (top variables, `cleanup`, block after the `no panic on stderr` check)
- Test: `crates/client-core/tests/push.rs`, `crates/client-core/tests/poll.rs`, `crates/desktop/tests/sidebar.rs`, `crates/desktop/tests/sidebar_draw.rs`; every full `SessionInfo` literal gains `last_reply: None,`

**Interfaces:**
- Consumes: `SessionInfo.pane`, `.activity` (Task 2); `tmux::valid_pane` (Task 2); `tmux::script_argv` (crate-private, exists).
- Produces:
  - `SessionInfo.last_reply: Option<String>`
  - `client_core::push::{RESTART_AFTER: Duration, REPLY_CHARS: usize = 120, Kind, PushEvent { t: f64, sid: Option<String>, pane: Option<String>, kind: Kind }, PushLine, Applied, parse_line(line: &str) -> Option<PushLine>, apply(snap: &mut HostSnapshot, event: &PushEvent) -> Applied, one_line(text: &str) -> Option<String>, carry_replies(snap: &mut HostSnapshot, replies: &mut HashMap<SessionKey, String>), tail_argv(host: &str, local_name: &str) -> Vec<String>, Tail::start(argv: Vec<String>, on_line: Box<dyn Fn(PushLine) + Send>) -> Tail, Tail::start_with(argv, restart: Duration, on_line) -> Tail}`
  - `enum Kind { Busy, Waiting(String), Stop { msg: Option<String>, bg: u32 }, Failed { err: String, msg: Option<String> }, SessionStart, SessionEnd, Other }`; `enum PushLine { Available(bool), Event(PushEvent) }`; `enum Applied { Unchanged, Changed, Relist }`
  - `Poller::force_next(&self)`; `poll::DEFAULT_PERIOD: Duration = 5 s`; `poll::period_from(value: Option<&str>) -> Duration`
  - `draw::POLL_ONLY: &str = "· poll"`, `draw::header_spans_with(host: &str, online: bool, polled: bool, cols: usize, theme: &Theme) -> Vec<Span<'static>>`, `View.polled: Vec<String>`
  - app: `UserEvent::Push(String, PushLine)`
- Departs from the spec: (a) "apply an event whose `t` is newer than the snapshot's listing time" — `HostSnapshot` has no listing time (`listing.rs:117-125`); the rule is tmls's `push.apply` per session: an event older than the session's `activity` (its status time) is dropped. (b) D3 "the next poll snapshot replaces push state": the poller delivers only listings that changed (`poll.rs:134`), so a pushed state could outlive a listing that says otherwise; after applying an event the app calls `Poller::force_next`, and the next listing is delivered whatever it says. (c) `StopFailure` keeps `msg` too (`Kind::Failed { err, msg }`), as tmls does for its last line. (d) The reader's first line says whether the file exists (`tmls-push on|off`): that is what `· poll` shows. (e) `last_reply` is carried across listings in the app (`carry_replies`): a listing never has it, and the dim line must not vanish at each poll.

- [ ] **Step 1: Failing client-core tests.** `crates/client-core/tests/push.rs`:

```rust
//! Status pushed by Claude's hooks through tmls's events.jsonl: the line parser, the mapping
//! onto a listing (tmls `push.apply`), the replies kept across listings, and the reader process.

use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use client_core::listing::{HostSnapshot, SessionInfo, SessionKey, Status, Tool};
use client_core::push::{
    Applied, Kind, PushEvent, PushLine, REPLY_CHARS, Tail, apply, carry_replies, one_line, parse_line, tail_argv,
};

const SID: &str = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";

fn key(name: &str) -> SessionKey {
    SessionKey { host: "nas".to_string(), name: name.to_string() }
}

/// A listed Claude session with session id `sid`, pane `pane`, status time `since`.
fn claude(name: &str, status: Status, sid: Option<&str>, pane: Option<&str>, since: u64) -> SessionInfo {
    SessionInfo {
        key: key(name),
        status,
        reason: None,
        tool: Tool::Claude,
        title: None,
        context_pct: None,
        attached: 0,
        last_output_age_s: Some(0),
        pid: None,
        session_id: sid.map(str::to_string),
        pane: pane.map(str::to_string),
        activity: Some(since),
        last_reply: None,
    }
}

fn listing(sessions: Vec<SessionInfo>) -> HostSnapshot {
    HostSnapshot { host: "nas".to_string(), online: true, error: None, sessions }
}

fn event(line: &str) -> PushEvent {
    match parse_line(line) {
        Some(PushLine::Event(event)) => event,
        other => panic!("{line}: {other:?}"),
    }
}

#[test]
fn hook_lines_parse_as_tmls_writes_them() {
    let e = event(&format!(r#"{{"t":1760200000.25,"ev":"PermissionRequest","sid":"{SID}","pane":"%7","cwd":"/w","tool":"Bash"}}"#));
    assert_eq!(e, PushEvent { t: 1760200000.25, sid: Some(SID.into()), pane: Some("%7".into()), kind: Kind::Waiting("permission prompt".into()) });
    let kind = |line: &str| event(line).kind;
    assert_eq!(kind(r#"{"t":1,"ev":"Notification","nt":"permission_prompt","msg":"Claude needs your permission"}"#), Kind::Waiting("permission prompt".into()));
    assert_eq!(kind(r#"{"t":1,"ev":"Notification","nt":"idle_prompt"}"#), Kind::Other);
    assert_eq!(kind(r#"{"t":1,"ev":"UserPromptSubmit","sid":"x"}"#), Kind::Busy);
    assert_eq!(kind(r#"{"t":1,"ev":"PreToolUse","tool":"Edit"}"#), Kind::Busy);
    assert_eq!(kind(r#"{"t":1,"ev":"Stop","msg":"All 12 tests pass.","bg":2}"#), Kind::Stop { msg: Some("All 12 tests pass.".into()), bg: 2 });
    assert_eq!(kind(r#"{"t":1,"ev":"Stop","msg":"","bg":0}"#), Kind::Stop { msg: None, bg: 0 });
    assert_eq!(kind(r#"{"t":1,"ev":"StopFailure","err":"rate_limit","msg":"429 Too Many Requests"}"#), Kind::Failed { err: "rate_limit".into(), msg: Some("429 Too Many Requests".into()) });
    assert_eq!(kind(r#"{"t":1,"ev":"SessionStart","src":"startup"}"#), Kind::SessionStart);
    assert_eq!(kind(r#"{"t":1,"ev":"SessionEnd","reason":"exit"}"#), Kind::SessionEnd);
    assert_eq!(kind(r#"{"t":1,"ev":"StatusLine","sid":"x","ctx":40}"#), Kind::Other);
    // A pane that is no pane id is dropped, the event kept.
    assert_eq!(event(r#"{"t":1,"ev":"Stop","pane":"%7;reboot"}"#).pane, None);
    assert_eq!(parse_line("tmls-push on"), Some(PushLine::Available(true)));
    assert_eq!(parse_line("tmls-push off"), Some(PushLine::Available(false)));
}

#[test]
fn junk_lines_are_ignored() {
    for junk in ["", "not json", r#"{"t":1,"ev":"Stop""#, "[1,2]", r#"{"ev":"Stop","sid":"x"}"#, r#"{"t":"soon","ev":"Stop"}"#, r#"{"t":1}"#, "tmls-push maybe"] {
        assert_eq!(parse_line(junk), None, "{junk:?}");
    }
    // A huge message parses; its reply is one line of REPLY_CHARS.
    let huge = format!(r#"{{"t":5,"ev":"Stop","sid":"{SID}","msg":"{}"}}"#, "x".repeat(100_000));
    let mut snap = listing(vec![claude("w", Status::Running, Some(SID), None, 1)]);
    assert_eq!(apply(&mut snap, &event(&huge)), Applied::Changed);
    assert_eq!(snap.sessions[0].last_reply.as_ref().map(|r| r.chars().count()), Some(REPLY_CHARS));
}

#[test]
fn events_move_a_session_as_tmls_does() {
    let mut snap = listing(vec![claude("w", Status::Idle, Some(SID), Some("%3"), 100)]);
    let at = |t: f64, kind: Kind| PushEvent { t, sid: Some(SID.into()), pane: None, kind };
    assert_eq!(apply(&mut snap, &at(101.0, Kind::Busy)), Applied::Changed);
    assert_eq!((snap.sessions[0].status, snap.sessions[0].activity), (Status::Running, Some(101)));
    assert_eq!(apply(&mut snap, &at(101.5, Kind::Busy)), Applied::Unchanged, "nothing new");
    assert_eq!(apply(&mut snap, &at(102.0, Kind::Waiting("permission prompt".into()))), Applied::Changed);
    assert_eq!((snap.sessions[0].status, snap.sessions[0].reason.as_deref()), (Status::Waiting, Some("permission prompt")));
    assert_eq!(apply(&mut snap, &at(103.0, Kind::Stop { msg: Some("Done.\nDetails below".into()), bg: 0 })), Applied::Changed);
    let s = &snap.sessions[0];
    assert_eq!((s.status, s.reason.as_deref(), s.last_reply.as_deref()), (Status::Idle, None, Some("Done.")));
    // A background task still runs: running, the reply kept.
    assert_eq!(apply(&mut snap, &at(104.0, Kind::Stop { msg: None, bg: 1 })), Applied::Changed);
    assert_eq!((snap.sessions[0].status, snap.sessions[0].last_reply.as_deref()), (Status::Running, Some("Done.")));
    assert_eq!(apply(&mut snap, &at(105.0, Kind::Failed { err: "rate_limit".into(), msg: None })), Applied::Changed);
    let s = &snap.sessions[0];
    assert_eq!((s.status, s.reason.as_deref(), s.last_reply.as_deref()), (Status::Error, Some("rate_limit"), Some("rate_limit")));
    // By pane when the session id is not listed.
    let mut snap = listing(vec![claude("p", Status::Idle, None, Some("%9"), 100)]);
    let by_pane = PushEvent { t: 200.0, sid: Some("other".into()), pane: Some("%9".into()), kind: Kind::Busy };
    assert_eq!(apply(&mut snap, &by_pane), Applied::Changed);
    assert_eq!(snap.sessions[0].status, Status::Running);
}

#[test]
fn older_or_unknown_events_change_nothing() {
    let mut snap = listing(vec![claude("w", Status::Running, Some(SID), Some("%3"), 500)]);
    let before = snap.clone();
    let older = PushEvent { t: 499.0, sid: Some(SID.into()), pane: None, kind: Kind::Stop { msg: Some("old".into()), bg: 0 } };
    assert_eq!(apply(&mut snap, &older), Applied::Unchanged, "the listing saw something newer");
    let stranger = PushEvent { t: 900.0, sid: Some("nobody".into()), pane: Some("%77".into()), kind: Kind::Busy };
    assert_eq!(apply(&mut snap, &stranger), Applied::Unchanged);
    let other = PushEvent { t: 900.0, sid: Some(SID.into()), pane: None, kind: Kind::Other };
    assert_eq!(apply(&mut snap, &other), Applied::Unchanged);
    let mut offline = HostSnapshot { online: false, sessions: Vec::new(), ..snap.clone() };
    assert_eq!(apply(&mut offline, &PushEvent { kind: Kind::Busy, ..stranger.clone() }), Applied::Unchanged);
    assert_eq!(snap, before);
}

#[test]
fn session_start_and_end_ask_for_a_listing() {
    let mut snap = listing(vec![]);
    for kind in [Kind::SessionStart, Kind::SessionEnd] {
        assert_eq!(apply(&mut snap, &PushEvent { t: 1.0, sid: None, pane: None, kind }), Applied::Relist);
    }
}

#[test]
fn the_reply_is_its_first_line_cut_to_120_chars() {
    assert_eq!(one_line("\n\n  Fixed it.  \nMore."), Some("Fixed it.".to_string()));
    assert_eq!(one_line("   \n"), None);
    assert_eq!(one_line(&"é".repeat(300)).map(|r| r.chars().count()), Some(REPLY_CHARS));
}

#[test]
fn replies_are_carried_across_listings_until_the_session_goes() {
    let mut replies = HashMap::new();
    let mut first = listing(vec![SessionInfo { last_reply: Some("done".into()), ..claude("w", Status::Idle, Some(SID), None, 1) }]);
    carry_replies(&mut first, &mut replies);
    assert_eq!(replies.get(&key("w")).map(String::as_str), Some("done"));
    let mut next = listing(vec![claude("w", Status::Idle, Some(SID), None, 1)]);
    carry_replies(&mut next, &mut replies);
    assert_eq!(next.sessions[0].last_reply.as_deref(), Some("done"));
    let mut offline = HostSnapshot { online: false, sessions: Vec::new(), ..listing(vec![]) };
    carry_replies(&mut offline, &mut replies);
    assert_eq!(replies.len(), 1, "a host away forgets nothing");
    carry_replies(&mut listing(vec![]), &mut replies);
    assert!(replies.is_empty(), "gone from an online listing");
}

#[test]
fn the_reader_only_reads_events_jsonl() {
    let argv = tail_argv("ph", "here");
    let script = argv.last().unwrap();
    assert!(script.contains("tail -n0 -F \"$f\""), "{script}");
    assert!(script.contains("$HOME/.cache/tmls/events.jsonl"), "{script}");
    assert!(!script.contains('>') || script.contains("2>/dev/null"), "writes nothing: {script}");
    assert_eq!(tail_argv("here", "here")[..2], ["sh", "-c"]);
}

#[test]
fn a_reader_reads_lines_restarts_after_its_end_and_stops_when_dropped() {
    let (tx, rx) = channel();
    let script = r#"echo tmls-push on; echo '{"t":5,"ev":"UserPromptSubmit","sid":null,"pane":"%1"}'; echo garbage; exit 0"#;
    let tail = Tail::start_with(
        vec!["sh".into(), "-c".into(), script.into()],
        Duration::from_millis(100),
        Box::new(move |line| {
            let _ = tx.send(line);
        }),
    );
    let got: Vec<PushLine> = (0..3).map(|_| rx.recv_timeout(Duration::from_secs(5)).expect("a line")).collect();
    assert_eq!(got[0], PushLine::Available(true));
    assert!(matches!(&got[1], PushLine::Event(e) if e.kind == Kind::Busy && e.pane.as_deref() == Some("%1")));
    assert_eq!(got[2], PushLine::Available(true), "started again after it ended; garbage skipped");
    drop(tail);
    // A reader that never ends is killed when dropped, at once.
    let tail = Tail::start_with(
        vec!["sh".into(), "-c".into(), "echo tmls-push off; exec sleep 30".into()],
        Duration::from_millis(100),
        Box::new(|_| {}),
    );
    std::thread::sleep(Duration::from_millis(300));
    let dropping = Instant::now();
    drop(tail);
    assert!(dropping.elapsed() < Duration::from_secs(2));
}
```

Append to `crates/client-core/tests/poll.rs` (import `period_from` from `client_core::poll`):

```rust
#[test]
fn force_next_delivers_the_next_listing_even_when_unchanged() {
    let _one = alone();
    let (poller, rx) = start(&["a"], Duration::from_millis(100), Duration::from_secs(5), |_| sh("cat >/dev/null; echo 100"));
    next(&rx, Duration::from_secs(5));
    assert!(rx.recv_timeout(Duration::from_millis(400)).is_err(), "an unchanged listing is not delivered");
    poller.force_next();
    next(&rx, Duration::from_secs(2));
    assert!(rx.recv_timeout(Duration::from_millis(400)).is_err(), "only the next one");
    drop(poller);
}

#[test]
fn the_period_comes_from_tmls_rs_poll_s() {
    assert_eq!(period_from(None), Duration::from_secs(5));
    assert_eq!(period_from(Some("60")), Duration::from_secs(60));
    assert_eq!(period_from(Some(" 2 ")), Duration::from_secs(2));
    for bad in ["", "0", "-3", "x", "3601", "1.5"] {
        assert_eq!(period_from(Some(bad)), Duration::from_secs(5), "{bad:?}");
    }
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test push --test poll`
Expected: compile errors (module `push`, `force_next`, `last_reply`).

- [ ] **Step 3: Implement in client-core.** `listing.rs` `SessionInfo`, after `activity`:

```rust
    /// The first line of Claude's last reply (at most 120 characters), from a pushed `Stop`
    /// event; a listing never has it (D10).
    pub last_reply: Option<String>,
```

and both literals gain `last_reply: None,`.

`poll.rs`: `State` gains `/// Counts force_next calls: a thread that sees it move delivers its next listing.\n resend: u64,` (initialised `resend: 0`). After `refresh_now`:

```rust
    /// The next listing of every host is delivered even if it is the same as the last one: a
    /// pushed event changed the app's copy, and the listing is the truth again (D3).
    pub fn force_next(&self) {
        self.shared.state().resend += 1;
    }
```

In `poll_host`: `let mut resent = 0;` before the loop; `let refresh = shared.state().refresh;` becomes `let (refresh, resend) = { let state = shared.state(); (state.refresh, state.resend) };`; the delivery test becomes

```rust
        if last.as_ref() != Some(&comparable) || resend != resent {
            (deliver.lock().unwrap_or_else(PoisonError::into_inner))(snapshot);
            last = Some(comparable);
            resent = resend;
        }
```

After the constants:

```rust
/// How often every host is listed unless `TMLS_RS_POLL_S` says otherwise.
pub const DEFAULT_PERIOD: Duration = Duration::from_secs(5);

/// The listing period from `TMLS_RS_POLL_S` (whole seconds, 1 to 3600); `DEFAULT_PERIOD` for
/// anything else.
pub fn period_from(value: Option<&str>) -> Duration {
    value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| (1..=3600).contains(s))
        .map_or(DEFAULT_PERIOD, Duration::from_secs)
}
```

`crates/client-core/src/push.rs`:

```rust
//! Status pushed by Claude's hooks (D1, D2). tmls (`tmls hooks install`, branch
//! `feat/native-attach`) makes every Claude on a host append one JSON line per hook event to
//! `~/.cache/tmls/events.jsonl` (`claude_hook.py`: `t, ev, sid, pane, cwd` and `msg`, `bg`,
//! `err`, `tool`, `nt`). tmls-rs only reads that file: one `tail -n0 -F` per online host (here
//! `sh -c`, elsewhere over `tmux::ssh`), restarted 10 s after it ends. Its first line says
//! whether the file is there (`· poll` in the list when it is not).
//!
//! An event moves the session it names (by Claude's session id, else by its pane) as tmls's
//! `push.apply` does; one older than what the listing already saw is dropped. The listing stays
//! the truth (D3): the app applies events to its copy of the last listing, and the next listing
//! replaces it.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::listing::{HostSnapshot, SessionKey, Status};
use crate::tmux::{script_argv, valid_pane};

/// How long after its reader ended a host's file is read again.
pub const RESTART_AFTER: Duration = Duration::from_secs(10);
/// The longest reply kept, in characters.
pub const REPLY_CHARS: usize = 120;

/// Says whether the file is there, then follows it from its end; rotation (tmls renames it at
/// 5 MB) is followed by `-F`. Reads only.
const TAIL_SCRIPT: &str = r#"f="$HOME/.cache/tmls/events.jsonl"; if [ -f "$f" ]; then echo tmls-push on; else echo tmls-push off; fi; exec tail -n0 -F "$f" 2>/dev/null"#;

/// What a hook event says about a session.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// `UserPromptSubmit`, `PreToolUse`: working.
    Busy,
    /// `PermissionRequest`, or `Notification` of type `permission_prompt`: waits, for this.
    Waiting(String),
    /// The turn ended; `bg` background tasks still run; `msg` is the reply.
    Stop { msg: Option<String>, bg: u32 },
    /// The turn ended in an API error.
    Failed { err: String, msg: Option<String> },
    SessionStart,
    SessionEnd,
    /// Anything else (`StatusLine`, other notifications): nothing to do.
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PushEvent {
    /// When, by the host's clock (s).
    pub t: f64,
    pub sid: Option<String>,
    /// A valid pane id, else `None`.
    pub pane: Option<String>,
    pub kind: Kind,
}

/// One line of a reader.
#[derive(Clone, Debug, PartialEq)]
pub enum PushLine {
    /// The host has `events.jsonl` (or not: the list says `· poll`).
    Available(bool),
    Event(PushEvent),
}

/// What an event did to a listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Unchanged,
    Changed,
    /// A Claude session started or ended: list the host again to learn its pane.
    Relist,
}

/// One line of the reader; `None` for anything that is not a line this module knows (not
/// JSON, no `t` or `ev`).
pub fn parse_line(line: &str) -> Option<PushLine> {
    match line.trim() {
        "tmls-push on" => return Some(PushLine::Available(true)),
        "tmls-push off" => return Some(PushLine::Available(false)),
        _ => {}
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let e = value.as_object()?;
    let text = |k: &str| e.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let t = e.get("t").and_then(Value::as_f64)?;
    let kind = match e.get("ev").and_then(Value::as_str)? {
        "UserPromptSubmit" | "PreToolUse" => Kind::Busy,
        "PermissionRequest" => Kind::Waiting("permission prompt".to_string()),
        "Notification" if text("nt").as_deref() == Some("permission_prompt") => {
            Kind::Waiting("permission prompt".to_string())
        }
        "Stop" => Kind::Stop {
            msg: text("msg"),
            bg: e.get("bg").and_then(Value::as_u64).map_or(0, |n| n.min(u64::from(u32::MAX)) as u32),
        },
        "StopFailure" => Kind::Failed { err: text("err").unwrap_or_else(|| "error".to_string()), msg: text("msg") },
        "SessionStart" => Kind::SessionStart,
        "SessionEnd" => Kind::SessionEnd,
        _ => Kind::Other,
    };
    let pane = text("pane").filter(|pane| valid_pane(pane));
    Some(PushLine::Event(PushEvent { t, sid: text("sid"), pane, kind }))
}

/// The first non-blank line of `text`, trimmed, cut to `REPLY_CHARS`.
pub fn one_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(REPLY_CHARS).collect())
}

/// Moves the session `event` names in `snap` (tmls `push.apply`).
pub fn apply(snap: &mut HostSnapshot, event: &PushEvent) -> Applied {
    if matches!(event.kind, Kind::SessionStart | Kind::SessionEnd) {
        return Applied::Relist;
    }
    if !snap.online {
        return Applied::Unchanged;
    }
    let by_sid = event
        .sid
        .as_ref()
        .and_then(|sid| snap.sessions.iter().position(|s| s.session_id.as_ref() == Some(sid)));
    let by_pane = || event.pane.as_ref().and_then(|pane| snap.sessions.iter().position(|s| s.pane.as_ref() == Some(pane)));
    let Some(index) = by_sid.or_else(by_pane) else {
        return Applied::Unchanged;
    };
    let session = &mut snap.sessions[index];
    let t = event.t.max(0.0) as u64;
    if session.activity.is_some_and(|since| t < since) {
        return Applied::Unchanged; // the listing already saw something newer
    }
    let (status, reason, reply) = match &event.kind {
        Kind::Busy => (Status::Running, None, None),
        Kind::Waiting(why) => (Status::Waiting, Some(why.clone()), None),
        Kind::Stop { msg, bg } => {
            let status = if *bg > 0 { Status::Running } else { Status::Idle };
            (status, None, msg.as_deref().and_then(one_line))
        }
        Kind::Failed { err, msg } => (Status::Error, Some(err.clone()), one_line(msg.as_deref().unwrap_or(err))),
        Kind::SessionStart | Kind::SessionEnd | Kind::Other => return Applied::Unchanged,
    };
    // the app's copy holds the listing's statuses, which are never `Unseen`; a copy that went
    // through `Seen` would say it for an idle session
    let listed = if session.status == Status::Unseen { Status::Idle } else { session.status };
    let new_reply = reply.is_some() && reply != session.last_reply;
    if listed == status && session.reason == reason && !new_reply {
        return Applied::Unchanged;
    }
    if listed != status {
        session.activity = Some(t);
    }
    session.status = status;
    session.reason = reason;
    if reply.is_some() {
        session.last_reply = reply;
    }
    Applied::Changed
}

/// Keeps the last replies across listings, which never have them: a reply in `snap` is
/// remembered, a remembered one is put back into `snap`; the host's sessions an online listing
/// no longer has are forgotten.
pub fn carry_replies(snap: &mut HostSnapshot, replies: &mut HashMap<SessionKey, String>) {
    if !snap.online {
        return;
    }
    replies.retain(|key, _| key.host != snap.host || snap.sessions.iter().any(|s| s.key == *key));
    for session in &mut snap.sessions {
        match &session.last_reply {
            Some(reply) => {
                replies.insert(session.key.clone(), reply.clone());
            }
            None => session.last_reply = replies.get(&session.key).cloned(),
        }
    }
}

/// The reader of `host`'s events.
pub fn tail_argv(host: &str, local_name: &str) -> Vec<String> {
    script_argv(host, TAIL_SCRIPT.to_string(), local_name)
}

/// One host's reader: a thread that runs `argv`, hands each line it knows to `on_line`, and
/// starts it again `restart` after it ended, until dropped.
pub struct Tail {
    stop: Arc<AtomicBool>,
    /// The running reader's pid (its process group's id); the lock is held while it is killed
    /// or reaped, so a pid is never used after its process was reaped.
    child: Arc<Mutex<Option<u32>>>,
    thread: Option<JoinHandle<()>>,
}

impl Tail {
    pub fn start(argv: Vec<String>, on_line: Box<dyn Fn(PushLine) + Send>) -> Tail {
        Tail::start_with(argv, RESTART_AFTER, on_line)
    }

    pub fn start_with(argv: Vec<String>, restart: Duration, on_line: Box<dyn Fn(PushLine) + Send>) -> Tail {
        let stop = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));
        let (thread_stop, thread_child) = (stop.clone(), child.clone());
        let thread = std::thread::Builder::new()
            .name("tmls-push".to_string())
            .spawn(move || follow(&argv, restart, &thread_stop, &thread_child, &*on_line))
            .expect("spawning a push reader");
        Tail { stop, child, thread: Some(thread) }
    }
}

fn kill_group(pid: u32) {
    // SAFETY: plain syscall on a process group this module started and has not reaped.
    unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
}

fn follow(argv: &[String], restart: Duration, stop: &AtomicBool, child: &Mutex<Option<u32>>, on_line: &dyn Fn(PushLine)) {
    let Some((program, args)) = argv.split_first() else { return };
    while !stop.load(Ordering::SeqCst) {
        let spawned = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();
        if let Ok(mut process) = spawned {
            *child.lock().unwrap_or_else(PoisonError::into_inner) = Some(process.id());
            if stop.load(Ordering::SeqCst) {
                kill_group(process.id()); // dropped while it started
            }
            if let Some(out) = process.stdout.take() {
                for line in BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if let Some(parsed) = parse_line(&line) {
                        on_line(parsed);
                    }
                }
            }
            let mut held = child.lock().unwrap_or_else(PoisonError::into_inner);
            *held = None;
            kill_group(process.id()); // ssh may have left something behind
            let _ = process.wait();
            drop(held);
        }
        let until = Instant::now() + restart;
        while !stop.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Tail {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(pid) = *self.child.lock().unwrap_or_else(PoisonError::into_inner) {
            kill_group(pid);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
```

`lib.rs`: `pub mod push;` plus a line in the crate comment.

- [ ] **Step 4: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass.

- [ ] **Step 5: Failing desktop tests.** Append to `crates/desktop/tests/sidebar.rs` (import `desktop::sidebar::summary`):

```rust
#[test]
fn the_dim_line_is_the_reason_else_the_last_reply_else_the_title() {
    let mut info = claude(LOCAL, "work", "a title");
    info.status = Status::Idle;
    assert_eq!(summary(&info), Some(("a title", false)));
    info.last_reply = Some("All 12 tests pass.".to_string());
    assert_eq!(summary(&info), Some(("All 12 tests pass.", false)));
    info.status = Status::Waiting;
    info.reason = Some("permission prompt".to_string());
    assert_eq!(summary(&info), Some(("permission prompt", true)));
}
```

Append to `crates/desktop/tests/sidebar_draw.rs` (import `header_spans_with`, `POLL_ONLY`):

```rust
#[test]
fn a_host_without_events_says_poll_dimly() {
    let theme = Theme::default();
    let said = header_spans_with("nas", true, true, SIDEBAR_COLS, &theme);
    let poll = find(&said, POLL_ONLY);
    assert_eq!((poll.col, poll.colour), (1 + 3 + 1, theme.dim_fg));
    assert!(said.iter().any(|span| span.text == "+"), "the + stays");
    assert_eq!(header_spans_with("nas", true, false, SIDEBAR_COLS, &theme), header_spans("nas", true, SIDEBAR_COLS, &theme));
    assert!(!header_spans_with("nas", false, true, SIDEBAR_COLS, &theme).iter().any(|span| span.text == POLL_ONLY), "offline says offline");
    assert_within(&header_spans_with(&"h".repeat(40), true, true, SIDEBAR_COLS, &theme));
}
```

- [ ] **Step 6: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test sidebar --test sidebar_draw`
Expected: compile errors (`header_spans_with`, `last_reply` in `summary`).

- [ ] **Step 7: Implement in the desktop.** `sidebar.rs` `summary` (update its doc: "…the reason while it waits, else Claude's last reply (pushed), else its title…"):

```rust
    if info.status == Status::Waiting
        && let Some(reason) = text(&info.reason)
    {
        return Some((reason, true));
    }
    if let Some(reply) = text(&info.last_reply) {
        return Some((reply, false));
    }
    text(&info.title)
        .filter(|title| *title != info.key.name)
        .map(|title| (title, false))
```

`draw.rs`, after `header_spans`:

```rust
/// After the name of a host whose `events.jsonl` is not there: status comes by polling only.
pub const POLL_ONLY: &str = "· poll";

/// `header_spans`, with `· poll` after the name when `polled` (and the host answers).
pub fn header_spans_with(
    host: &str,
    online: bool,
    polled: bool,
    cols: usize,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans = header_spans(host, online, cols, theme);
    if online && polled {
        let end = cols.saturating_sub(1);
        let room = end.saturating_sub(1 + 2); // the name's columns in `header_spans`
        let col = 1 + columns(&host.to_uppercase()).min(room) + 1;
        if col + columns(POLL_ONLY) < end.saturating_sub(1) {
            spans.push(Span::whole(0, col, POLL_ONLY).colour(theme.dim_fg));
        }
    }
    spans
}
```

`View` gains `/// Hosts whose events.jsonl is not there (\`· poll\`).\n pub polled: Vec<String>,`; in `sidebar()` the header call becomes `header_spans_with(host, *online, view.polled.contains(host), l.list_cols, theme)`.

`app.rs`:
- Delete the constant `POLL_PERIOD` (line 105) and its comment line about the period; `Poller::start(&cfg, POLL_PERIOD, …)` becomes `Poller::start(&cfg, client_core::poll::period_from(std::env::var("TMLS_RS_POLL_S").ok().as_deref()), POLL_TIMEOUT, …)`.
- Imports: `use std::collections::HashMap;`, `use client_core::push::{self, Applied, PushLine};`.
- `UserEvent` gains `/// A line of a host's events.jsonl reader.\n Push(String, PushLine),`; `user_event`: `UserEvent::Push(host, line) => running.pushed(host, line),`.
- `Running` gains (initialised with `HashMap::new()`):

```rust
    /// Each host's latest listing as the poller gave it (the listing's statuses, no ◆): what
    /// a pushed event changes.
    listings: HashMap<String, HostSnapshot>,
    /// The last reply of each Claude session, from pushed `Stop` events.
    replies: HashMap<SessionKey, String>,
    /// One events.jsonl reader per online host.
    tails: HashMap<String, push::Tail>,
    /// Whether a host has events.jsonl (`· poll` when not); unknown until its reader says.
    push_on: HashMap<String, bool>,
```

- `snapshot(&mut self, mut snapshot: HostSnapshot)` — first lines:

```rust
        push::carry_replies(&mut snapshot, &mut self.replies);
        self.listings.insert(snapshot.host.clone(), snapshot.clone());
        self.follow_push(&snapshot.host, snapshot.online);
```

- New methods:

```rust
    /// Starts the reader of `host`'s events.jsonl when the host answers; stops it when not.
    fn follow_push(&mut self, host: &str, online: bool) {
        if !online {
            self.tails.remove(host);
            self.push_on.remove(host);
            return;
        }
        if host == KITTY || self.tails.contains_key(host) {
            return;
        }
        let (proxy, name) = (self.proxy.clone(), host.to_string());
        let argv = push::tail_argv(host, &self.cfg.local_name);
        let tail = push::Tail::start(
            argv,
            Box::new(move |line| {
                let _ = proxy.send_event(UserEvent::Push(name.clone(), line));
            }),
        );
        self.tails.insert(host.to_string(), tail);
    }

    /// A line of a host's reader: whether it has events at all, or an event, which moves the
    /// app's copy of the host's last listing and goes the way a listing goes (marks, alerts,
    /// queue, redraw). Redraws coalesce: `invalidate` asks for one frame, however many events.
    fn pushed(&mut self, host: String, line: PushLine) {
        match line {
            PushLine::Available(on) => {
                self.push_on.insert(host, on);
                self.invalidate();
            }
            PushLine::Event(event) => {
                if self.push_on.insert(host.clone(), true) != Some(true) {
                    self.invalidate();
                }
                let Some(mut snapshot) = self.listings.get(&host).cloned() else {
                    return;
                };
                match push::apply(&mut snapshot, &event) {
                    Applied::Unchanged => {}
                    Applied::Relist => self.poller.refresh_now(),
                    Applied::Changed => {
                        // Until the next listing, which replaces this whatever it says.
                        self.poller.force_next();
                        self.snapshot(snapshot);
                    }
                }
            }
        }
    }
```

- `build_view`: `polled: self.push_on.iter().filter(|(_, on)| !**on).map(|(host, _)| host.clone()).collect(),`
- `shutdown`: first line `self.tails.clear();` (the readers are killed before the window goes).
- Update the module comment's "List path" paragraph: "A host's events.jsonl reader (`push::Tail`) delivers each hook event → `UserEvent::Push` → `push::apply` on the host's last listing → the same path."

- [ ] **Step 8: e2e block.** Top of `e2e.sh`: `PUSH='tmlsrs-test-2push'` and `PUSH_SID='3b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'`; `cleanup()` gains `tmux kill-session -t "=$PUSH" 2>/dev/null`. After the `no panic on stderr` if/else block and before `echo "--- files under the scratch HOME"`:

```bash
# --- push: events.jsonl moves the marks without a poll (F6) ----------------------------------------
# A second run of the app that lists every 60 s: what changes within seconds came by push.
PUSH_START_NOTIFIED=$(wc -l <"$NOTIFY_LOG")
tmux new-session -d -s "$PUSH" -x 100 -y 30
fake_claude "$PUSH" "$PUSH_SID" idle
EVENTS="$HOME/.cache/tmls/events.jsonl"
mkdir -p "$HOME/.cache/tmls" && : >"$EVENTS"
PUSH_PANE=$(tmux display -p -t "=$PUSH:" '#{pane_id}')
# push_event EV [MORE_JSON] — one line as tmls's hook writes it, timed now.
push_event() {
  printf '{"t":%s,"ev":"%s","sid":"%s","pane":"%s","cwd":"/work"%s}\n' \
    "$(date +%s.%N)" "$1" "$PUSH_SID" "$PUSH_PANE" "${2:-}" >>"$EVENTS"
}
# list_picture — a hash of the list's pixel columns only (0-223).
list_picture() {
  timeout 10 grim -t ppm - 2>>"$OUT/grim.log" | python3 -c '
import hashlib, sys
d = sys.stdin.buffer.read()
parts = d.split(b"\n", 3)
w, h = map(int, parts[1].split())
px = parts[3]
crop = hashlib.sha256()
for y in range(h):
    crop.update(px[3 * w * y:3 * (w * y + 224)])
print(crop.hexdigest()[:16])
' 2>/dev/null || echo none
}
TMLS_RS_POLL_S=60 "$BIN" 2>"$OUT/app2.stderr" &
APP_PID=$!
if wait_for 30 window_up; then pass "the second run's window opened (TMLS_RS_POLL_S=60)"; else fail "the second run's window opened (TMLS_RS_POLL_S=60)"; fi
sleep 3 # its first listing, at start; the next one is a minute away
push_event PermissionRequest ',"tool":"Bash"'
if wait_for 4 grep -qxF -- "--|tmls · $(hostname)|? $PUSH · permission prompt|" "$NOTIFY_LOG"; then
  pass "a PermissionRequest event alerts within seconds, with no poll ('? $PUSH · permission prompt')"
else
  fail "a PermissionRequest event alerts within seconds, with no poll" "log: $(tail -n +"$((PUSH_START_NOTIFIED + 1))" "$NOTIFY_LOG" | tr '\n' ' ')"
fi
check "screenshot 39-push-waiting" shot 39-push-waiting
push_event Stop ',"msg":"first reply","bg":0'
sleep 1.5
REPLY_BEFORE=$(list_picture)
push_event Stop ',"msg":"tmls-push-reply-two","bg":0'
list_changed() { [ "$(list_picture)" != "$REPLY_BEFORE" ]; }
if [ "$REPLY_BEFORE" != none ] && wait_for 3 list_changed; then
  pass "a Stop event's reply changes the row's dim line, with no poll"
else
  fail "a Stop event's reply changes the row's dim line, with no poll" "the list's pixels did not change"
fi
check "screenshot 40-push-reply (the dim line is the reply)" shot 40-push-reply
key_quit
wait_for 10 app_gone || kill "$APP_PID" 2>/dev/null
if grep -nE "panicked|stack backtrace" "$OUT/app2.stderr" >/dev/null; then
  fail "no panic on the second run's stderr" "$(grep -n 'panicked' -A3 "$OUT/app2.stderr" | head -6 | tr '\n' ' ')"
else
  pass "no panic on the second run's stderr"
fi
```

- [ ] **Step 9: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; client-core tests; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/t6-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/night2/t6-e2e/40-push-reply.png`: the row's second line reads `tmls-push-reply-two`.

- [ ] **Step 10: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): status from Claude's hooks reaches the list in a blink, and rows show the last reply"
```

---

### Task 7: F7 — send a message, or queue it until the turn ends

**Files:**
- Create: `crates/desktop/src/queue.rs`, `crates/desktop/tests/queue.rs`
- Modify: `crates/desktop/src/lib.rs` (`pub mod queue;`)
- Modify: `crates/client-core/src/tmux.rs` (add `send_text_script`, `send_text_argv`, `buffer_name` after `send_keys_argv`)
- Modify: `crates/desktop/src/forms.rs` (`Stop::Clear`, `Form::Ask`, `FormResult::{Ask, ClearQueue}`, `MenuItem::Send`, `menu_items`, `is_claude`, `saved_prompts`, `DEFAULT_PROMPT`, `send_text`, `run_tmux_with`)
- Modify: `crates/desktop/src/keys.rs` (`ListKey::Send`, `Global::Ask`, `binding`, `HELP`)
- Modify: `crates/desktop/src/theme.rs` (`QUEUED_MARK`, `QUEUED_RGB`), `crates/desktop/src/draw.rs` (`RowLook.queued`, `View.queued`, `LEGEND_MORE`, `session_spans`)
- Modify: `crates/desktop/src/app.rs` (`UserEvent::Sent`, `Running.queue`, `.prompts_file`, `snapshot`, `form_result`, `menu_result`, `list_key`, `global`, `done`, `build_view`, new `ask_form`, `deliver_queued`, `sent`)
- Modify: `tooling/smoke/e2e.sh` (top variables, `cleanup`, block before `# --- quit`)
- Test: `crates/client-core/tests/tmux.rs`, `crates/desktop/tests/forms.rs`, `crates/desktop/tests/keys.rs`, `crates/desktop/tests/sidebar_draw.rs`, `crates/desktop/tests/queue.rs`

**Interfaces:**
- Consumes: `tmux::run_with_stdin` (Task 4), `forms::InputRunner` (Task 4), `forms::first_line`, `menu_items`, `RowMenu::with`, `Sidebar::info` (Task 5), `SessionInfo.pane` (Task 2), `fake_claude`, `top_of_list` (Task 5 e2e).
- Produces:
  - `tmux::send_text_script(target: &str, buffer: &str) -> String`, `tmux::send_text_argv(key: &SessionKey, pane: Option<&str>, buffer: &str, local_name: &str) -> Vec<String>`, `tmux::buffer_name() -> String` (`tmls-` + 8 hex)
  - `queue::Queue` (`Default`) with `busy(Option<Status>) -> bool` (assoc.), `add(&mut, &SessionKey, String) -> usize`, `count(&, &SessionKey) -> usize`, `keys(&) -> impl Iterator<Item = &SessionKey>`, `due(&mut, &SessionKey, before: Option<Status>, after: Option<Status>) -> Option<String>`, `sent(&mut, &SessionKey, text: &str, ok: bool)`, `clear`, `renamed(&mut, old, new)`, `forget`
  - `forms::Stop::Clear`; `forms::Form::Ask { key, pane: Option<String>, field: TextField, saved: Vec<String>, pick: Option<usize>, queued: usize, focus: Stop, error: Option<String> }`; `Form::ask(key: SessionKey, pane: Option<String>, saved: Vec<String>, queued: usize) -> Form`; `FormResult::Ask { key: SessionKey, pane: Option<String>, text: String }`, `FormResult::ClearQueue(SessionKey)`; `MenuItem::Send`; `forms::is_claude(info: &SessionInfo) -> bool`; `forms::DEFAULT_PROMPT`, `forms::saved_prompts(text: Option<&str>) -> Vec<String>`; `forms::send_text(key, pane: Option<&str>, text: &str, local_name, feed: InputRunner) -> Result<String, String>`; `forms::run_tmux_with(argv: &[String], input: &[u8]) -> anyhow::Result<String>`
  - `keys::ListKey::Send`, `keys::Global::Ask`; `theme::QUEUED_MARK: &str = "✉"`, `theme::QUEUED_RGB`; `RowLook.queued: usize`; `View.queued: Vec<(SessionKey, usize)>`
  - app: `UserEvent::Sent(SessionKey, String, Result<String, String>)`

- [ ] **Step 1: Failing tests.** Append to `crates/client-core/tests/tmux.rs` (import `send_text_argv, send_text_script, buffer_name, run_with_stdin`; `use std::path::PathBuf;`):

```rust
#[test]
fn send_text_script_pastes_through_a_named_buffer_then_presses_enter() {
    assert_eq!(
        send_text_script("%7", "tmls-0123abcd"),
        "tmux load-buffer -b 'tmls-0123abcd' - && { tmux paste-buffer -p -d -b 'tmls-0123abcd' -t '%7' || { tmux delete-buffer -b 'tmls-0123abcd'; false; }; } && sleep 0.2 && tmux send-keys -t '%7' Enter"
    );
    let name = buffer_name();
    assert!(name.len() == 13 && name.starts_with("tmls-") && name[5..].bytes().all(|b| b.is_ascii_hexdigit()), "{name}");
    assert_ne!(buffer_name(), buffer_name(), "two sends in one tick get two buffers");
    let there = send_text_argv(&key("ph", "a b"), None, "tmls-0123abcd", LOCAL);
    assert_eq!(&there[..SSH.len()], &v(&SSH)[..]);
    assert!(there.last().unwrap().ends_with("tmux send-keys -t '=a b:' Enter"), "{there:?}");
    assert_eq!(send_text_argv(&key(LOCAL, "w"), Some("%7"), "tmls-0123abcd", LOCAL)[..2], v(&["sh", "-c"]));
}

#[test]
fn send_text_gives_tmux_the_message_on_stdin_byte_for_byte() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("send-text-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let got = dir.join("buffer");
    let message = "it's \"quoted\" $(touch nope) `touch nope2` \\n; 日本 é\n\tsecond line\n".repeat(64);
    // A tmux that keeps what load-buffer reads; paste-buffer and send-keys succeed.
    let stub = format!("tmux() {{ if [ \"$1\" = load-buffer ]; then cat >'{}'; fi; }}; ", got.display());
    let argv = v(&["sh", "-c", &format!("{stub}{}", send_text_script("%7", "tmls-0123abcd"))]);
    run_with_stdin(&argv, message.as_bytes(), Duration::from_secs(5)).expect("the script");
    assert_eq!(std::fs::read(&got).unwrap(), message.as_bytes());
    assert!(!std::path::Path::new("nope").exists() && !std::path::Path::new("nope2").exists(), "nothing in it ran");
}
```

New `crates/desktop/tests/queue.rs`:

```rust
//! The queue of messages for busy Claude sessions: one per turn end, in order, never twice.

use client_core::{SessionKey, Status};
use desktop::queue::Queue;

fn key(name: &str) -> SessionKey {
    SessionKey { host: "nas".to_string(), name: name.to_string() }
}

#[test]
fn one_message_per_turn_end_in_order() {
    let (mut q, k) = (Queue::default(), key("w"));
    assert_eq!(q.add(&k, "one".into()), 1);
    assert_eq!(q.add(&k, "two".into()), 2);
    assert_eq!(q.due(&k, Some(Status::Running), Some(Status::Running)), None);
    assert_eq!(q.due(&k, Some(Status::Running), Some(Status::Idle)), Some("one".to_string()));
    q.sent(&k, "one", true);
    assert_eq!(q.count(&k), 1);
    assert_eq!(q.due(&k, Some(Status::Idle), Some(Status::Idle)), None, "no turn ended");
    assert_eq!(q.due(&k, Some(Status::Waiting), Some(Status::Unseen)), Some("two".to_string()));
    q.sent(&k, "two", true);
    assert_eq!((q.count(&k), q.keys().count()), (0, 0));
}

#[test]
fn flapping_failing_or_unlisted_sends_nothing() {
    let (mut q, k) = (Queue::default(), key("w"));
    q.add(&k, "one".into());
    for (before, after) in [
        (Some(Status::Running), Some(Status::Waiting)),
        (Some(Status::Waiting), Some(Status::Running)),
        (Some(Status::Running), Some(Status::Error)),
        (Some(Status::Running), None), // the host went away
        (None, Some(Status::Idle)),    // first seen idle: no turn ended here
    ] {
        assert_eq!(q.due(&k, before, after), None, "{before:?} -> {after:?}");
    }
    assert_eq!(q.count(&k), 1, "kept");
}

#[test]
fn a_message_in_flight_is_not_sent_twice() {
    let (mut q, k) = (Queue::default(), key("w"));
    q.add(&k, "one".into());
    assert_eq!(q.due(&k, Some(Status::Running), Some(Status::Idle)), Some("one".to_string()));
    assert_eq!(q.due(&k, Some(Status::Running), Some(Status::Idle)), None, "still on its way");
    q.sent(&k, "one", false);
    assert_eq!(q.count(&k), 1, "a failed send keeps it");
    assert_eq!(q.due(&k, Some(Status::Running), Some(Status::Idle)), Some("one".to_string()), "tried at the next turn end");
}

#[test]
fn rename_moves_the_queue_kill_drops_it_clear_empties_it() {
    let (mut q, old, new) = (Queue::default(), key("old"), key("new"));
    q.add(&old, "a".into());
    q.renamed(&old, &new);
    assert_eq!((q.count(&old), q.count(&new)), (0, 1));
    q.forget(&new);
    assert_eq!(q.count(&new), 0);
    q.add(&new, "b".into());
    q.clear(&new);
    assert_eq!(q.keys().count(), 0);
}

#[test]
fn busy_is_running_or_waiting() {
    assert!(Queue::busy(Some(Status::Running)) && Queue::busy(Some(Status::Waiting)));
    for idle in [Some(Status::Idle), Some(Status::Unseen), Some(Status::Error), None] {
        assert!(!Queue::busy(idle), "{idle:?}");
    }
}
```

Append to `crates/desktop/tests/forms.rs`:

```rust
#[test]
fn ask_sends_the_typed_text_or_a_saved_prompt() {
    let k = key(LOCAL, "w");
    let mut form = Form::ask(k.clone(), Some("%3".into()), vec!["What's the progress?".into()], 0);
    assert_eq!(form.focus(), Stop::Name);
    assert_eq!(form.key(&named(Named::Enter)), FormResult::Pending);
    assert_eq!(form.error(), Some("type a message first"));
    for c in "hi there".chars() {
        form.key(&event(Key::Char(c), Mods::default()));
    }
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Ask { key: k.clone(), pane: Some("%3".into()), text: "hi there".into() }
    );
    let saved = vec!["a".to_string(), "b".to_string()];
    let mut down = Form::ask(k.clone(), None, saved.clone(), 0);
    down.key(&named(Named::Down));
    assert_eq!(down.key(&named(Named::Enter)), FormResult::Ask { key: k.clone(), pane: None, text: "a".into() });
    let mut up = Form::ask(k.clone(), None, saved, 0);
    up.key(&named(Named::Up));
    assert_eq!(up.key(&named(Named::Enter)), FormResult::Ask { key: k.clone(), pane: None, text: "b".into() });
    assert_eq!(Form::ask(k, None, vec![], 0).key(&named(Named::Escape)), FormResult::Cancelled);
}

#[test]
fn ask_shows_the_queue_and_can_clear_it() {
    let k = key(LOCAL, "w");
    let mut form = Form::ask(k.clone(), None, vec![], 2);
    let texts: Vec<String> = form.panel().cells.iter().map(|cell| cell.text.clone()).collect();
    assert!(texts.contains(&"Send to w · 2 queued".to_string()), "{texts:?}");
    assert!(texts.contains(&" Clear queue ".to_string()));
    form.key(&named(Named::Tab));
    assert_eq!(form.focus(), Stop::Ok);
    form.key(&named(Named::Tab));
    assert_eq!(form.focus(), Stop::Clear);
    assert_eq!(form.key(&named(Named::Enter)), FormResult::ClearQueue(k.clone()));
    let empty: Vec<String> = Form::ask(k, None, vec![], 0).panel().cells.iter().map(|cell| cell.text.clone()).collect();
    assert!(!empty.contains(&" Clear queue ".to_string()));
    assert!(empty.contains(&"Send to w".to_string()));
}

#[test]
fn saved_prompts_are_tmls_s_file_or_the_default() {
    assert_eq!(forms::saved_prompts(None), [forms::DEFAULT_PROMPT]);
    assert_eq!(forms::saved_prompts(Some("\n# only a comment\n")), [forms::DEFAULT_PROMPT]);
    assert_eq!(forms::saved_prompts(Some("# mine\nRun the tests\n\n  Commit  # now\n")), ["Run the tests", "Commit"]);
}

#[test]
fn send_text_feeds_the_message_and_says_so() {
    let fed = RefCell::new(Vec::new());
    let feed = |argv: &[String], input: &[u8]| -> anyhow::Result<String> {
        fed.borrow_mut().push((argv.to_vec(), input.to_vec()));
        Ok(String::new())
    };
    let k = key("ph", "w");
    assert_eq!(forms::send_text(&k, Some("%3"), "hi\nthere", LOCAL, &feed), Ok("sent to w".to_string()));
    let (argv, input) = fed.borrow()[0].clone();
    assert_eq!(input, b"hi\nthere");
    assert!(argv.last().unwrap().ends_with("-t '%3' Enter"), "{argv:?}");
    let failing = |_: &[String], _: &[u8]| -> anyhow::Result<String> { anyhow::bail!("no server running on /tmp/x") };
    assert_eq!(forms::send_text(&k, None, "x", LOCAL, &failing), Err("couldn't send to w: no server running on /tmp/x".to_string()));
}

#[test]
fn claude_sessions_get_send_in_their_menu() {
    let mut plain = info("w", Status::Idle, None);
    plain.pane = None;
    assert!(!forms::is_claude(&plain));
    assert_eq!(forms::menu_items(Some(&plain)), MENU.to_vec());
    let claude = info("w", Status::Idle, None);
    assert!(forms::is_claude(&claude));
    assert_eq!(forms::menu_items(Some(&claude)).last(), Some(&MenuItem::Send));
}
```

and in `menu_items_offer_approve_only_while_claude_asks` (Task 5) the three Claude cases now end with `MenuItem::Send` (the helper `info` has a pane): change `MENU.to_vec()` for the two `info(…)` cases to `[MENU.as_slice(), &[MenuItem::Send]].concat()` and the asking case to `[MenuItem::Approve, MenuItem::Rename, MenuItem::Kill, MenuItem::OpenTerminal, MenuItem::CopyAttach, MenuItem::Send]` (the `None` case stays `MENU`).

`crates/desktop/tests/keys.rs`: add `'s'` to the list keys of `the_help_names_every_key_of_the_list_and_every_alt_shift_letter` and `'a'` to its Alt+Shift letters; append

```rust
#[test]
fn s_in_the_list_and_alt_shift_a_send_a_message() {
    assert_eq!(list_key(&ch('s', NONE)), Some(ListKey::Send));
    assert_eq!(list_key(&repeat(&ch('s', NONE))), None);
    for focus in [Focus::Terminal, Focus::Sidebar] {
        assert_eq!(global(&ch('A', ALT_SHIFT), focus), Some(Global::Ask));
        assert_eq!(global(&ch('a', ALT_SHIFT), focus), Some(Global::Ask));
    }
    assert_eq!(global(&ch('a', ALT_SHIFT), Focus::Form), None);
}
```

`crates/desktop/tests/sidebar_draw.rs`:

```rust
#[test]
fn queued_messages_show_before_the_dot_and_the_star() {
    let info = session(LOCAL, "work", Status::Running);
    let look = RowLook {
        queued: 2,
        unread: true,
        pinned: true,
        ..RowLook::default()
    };
    let said = spans(&info, look);
    let (name, queued, dot, star) = (find(&said, "work"), find(&said, "2✉"), find(&said, "•"), find(&said, "★"));
    assert_eq!(queued.col, name.col + 4 + 1);
    assert_eq!((dot.col, star.col), (queued.col + 3, queued.col + 5));
    assert_eq!(queued.colour, theme::QUEUED_RGB);
    assert_within(&said);
}
```

and in the legend test add `assert_eq!(glyphs(&quads, l.legend, theme::QUEUED_RGB).len(), 1, "✉ queued");` (the legend stays six lines: eight entries).

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test tmux`; `tooling/docker/build.sh test -p desktop --test queue --test forms --test keys --test sidebar_draw`
Expected: compile errors for every new name.

- [ ] **Step 3: client-core.** `tmux.rs`, after `send_keys_argv` (import `std::sync::atomic::{AtomicU32, Ordering}` and `std::time::{SystemTime, UNIX_EPOCH}`):

```rust
/// Types the text on the script's stdin into `target` and presses Enter (tmls `prompts.send`):
/// through a paste buffer of its own (`buffer`), as a bracketed paste when the program asks for
/// one, so a message of several lines arrives whole; the buffer is deleted after the paste, or
/// when the paste fails. The text never appears in the script.
pub fn send_text_script(target: &str, buffer: &str) -> String {
    let (t, b) = (quote(target), quote(buffer));
    format!(
        "tmux load-buffer -b {b} - && {{ tmux paste-buffer -p -d -b {b} -t {t} || {{ tmux delete-buffer -b {b}; false; }}; }} && sleep 0.2 && tmux send-keys -t {t} Enter"
    )
}

/// [`send_text_script`] for Claude's pane of the session (`pane_target`), here or on its host.
pub fn send_text_argv(key: &SessionKey, pane: Option<&str>, buffer: &str, local_name: &str) -> Vec<String> {
    script_argv(&key.host, send_text_script(&pane_target(key, pane), buffer), local_name)
}

/// A paste buffer's name of its own (`tmls-` and 8 hex digits): two sends in the same moment
/// cannot swap their texts.
pub fn buffer_name() -> String {
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let n = COUNT.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    format!("tmls-{:08x}", nanos ^ std::process::id().rotate_left(16) ^ n.wrapping_mul(0x9e37_79b9))
}
```

- [ ] **Step 4: `queue.rs`.**

```rust
//! Messages for a Claude session that is busy (D6: in memory only, gone at quit, as in tmls):
//! kept here and sent one at a time, the oldest first, each when the session's turn ends —
//! its status goes from running or waiting to idle or done, by a listing or a pushed event
//! (tmls `app.py`: `queue`, `_deliver_queued`). A send that fails keeps its message for the
//! next turn end; one that is on its way is not started again.

use std::collections::{HashMap, HashSet, VecDeque};

use client_core::{SessionKey, Status};

#[derive(Debug, Default)]
pub struct Queue {
    pending: HashMap<SessionKey, VecDeque<String>>,
    /// Sessions whose first message is being sent now.
    sending: HashSet<SessionKey>,
}

impl Queue {
    /// Whether a message for a session in `status` waits in the queue rather than going now.
    pub fn busy(status: Option<Status>) -> bool {
        matches!(status, Some(Status::Running | Status::Waiting))
    }

    /// Queues `text`; returns how many wait for the session now.
    pub fn add(&mut self, key: &SessionKey, text: String) -> usize {
        let queue = self.pending.entry(key.clone()).or_default();
        queue.push_back(text);
        queue.len()
    }

    pub fn count(&self, key: &SessionKey) -> usize {
        self.pending.get(key).map_or(0, VecDeque::len)
    }

    /// The sessions with messages waiting.
    pub fn keys(&self) -> impl Iterator<Item = &SessionKey> {
        self.pending.keys()
    }

    /// After a listing or an event moved the session from `before` to `after`: the message to
    /// send now, if its turn just ended and none is on its way. The caller sends it and reports
    /// with `sent`.
    pub fn due(&mut self, key: &SessionKey, before: Option<Status>, after: Option<Status>) -> Option<String> {
        let ended = Queue::busy(before) && matches!(after, Some(Status::Idle | Status::Unseen));
        if !ended || self.sending.contains(key) {
            return None;
        }
        let head = self.pending.get(key)?.front()?.clone();
        self.sending.insert(key.clone());
        Some(head)
    }

    /// The send `due` started is over: when it worked, `text` leaves the queue.
    pub fn sent(&mut self, key: &SessionKey, text: &str, ok: bool) {
        self.sending.remove(key);
        if !ok {
            return;
        }
        if let Some(queue) = self.pending.get_mut(key)
            && queue.front().map(String::as_str) == Some(text)
        {
            queue.pop_front();
        }
        if self.pending.get(key).is_some_and(VecDeque::is_empty) {
            self.pending.remove(key);
        }
    }

    /// Clear queue, in the form.
    pub fn clear(&mut self, key: &SessionKey) {
        self.pending.remove(key);
    }

    /// The session was renamed: its messages follow it.
    pub fn renamed(&mut self, old: &SessionKey, new: &SessionKey) {
        if let Some(queue) = self.pending.remove(old) {
            self.pending.entry(new.clone()).or_default().extend(queue);
        }
        if self.sending.remove(old) {
            self.sending.insert(new.clone());
        }
    }

    /// The session was killed: its messages go with it.
    pub fn forget(&mut self, key: &SessionKey) {
        self.pending.remove(key);
        self.sending.remove(key);
    }
}
```

`lib.rs`: `pub mod queue;` and "- `queue` — messages waiting for a busy Claude session."

- [ ] **Step 5: `forms.rs`.** `Stop` gains `/// The Ask form's Clear queue button.\n Clear,`; consts:

```rust
const ASK_STOPS: [Stop; 3] = [Stop::Name, Stop::Ok, Stop::Cancel];
const ASK_STOPS_CLEAR: [Stop; 4] = [Stop::Name, Stop::Ok, Stop::Clear, Stop::Cancel];
const ASK_COLS: usize = 60;
/// The saved prompts the Ask form lists.
const MAX_SAVED_SHOWN: usize = 8;
/// The saved prompt when `~/.config/tmls/prompts` has none (tmls `prompts.DEFAULT`).
pub const DEFAULT_PROMPT: &str = "What's the progress?";
```

`FormResult` gains:

```rust
    /// Send `text` to the session now, or queue it while it is busy (the app decides).
    Ask {
        key: SessionKey,
        pane: Option<String>,
        text: String,
    },
    /// Drop the session's queued messages.
    ClearQueue(SessionKey),
```

`Form` gains:

```rust
    /// A message for a Claude session: a field, the saved prompts, how many already wait.
    Ask {
        key: SessionKey,
        pane: Option<String>,
        field: TextField,
        saved: Vec<String>,
        /// The saved prompt ↑ / ↓ last put into the field.
        pick: Option<usize>,
        queued: usize,
        focus: Stop,
        error: Option<String>,
    },
```

with

```rust
    /// The message form for a session; `queued` messages wait for it already.
    pub fn ask(key: SessionKey, pane: Option<String>, saved: Vec<String>, queued: usize) -> Form {
        Form::Ask {
            key,
            pane,
            field: TextField::new(),
            saved,
            pick: None,
            queued,
            focus: Stop::Name,
            error: None,
        }
    }
```

- `focus()`: add `| Form::Ask { focus, .. }`; `error()`: `Form::Ask { error, .. } => error.as_deref()`; `stops()`: `Form::Ask { queued, .. } if *queued > 0 => &ASK_STOPS_CLEAR, Form::Ask { .. } => &ASK_STOPS`;
- `focus_on()`: `Form::Ask { focus, field, .. } => { *focus = stop; if stop == Stop::Name { field.select_all(); } }`
- `submit()`:

```rust
            Form::Ask {
                key,
                pane,
                field,
                error,
                ..
            } => {
                let text = field.text().trim().to_string();
                if text.is_empty() {
                    *error = Some("type a message first".to_string());
                    return FormResult::Pending;
                }
                FormResult::Ask {
                    key: key.clone(),
                    pane: pane.clone(),
                    text,
                }
            }
```

- `activate()`:

```rust
    fn activate(&mut self) -> FormResult {
        match (self.focus(), &*self) {
            (Stop::Cancel, _) => FormResult::Cancelled,
            (Stop::Clear, Form::Ask { key, .. }) => FormResult::ClearQueue(key.clone()),
            _ => self.submit(),
        }
    }
```

- `key()`: `on_button` adds `Stop::Clear`; the per-form match gains, before the `Form::Rename { focus, .. } | Form::ConfirmKill { focus, .. }` arm:

```rust
            Form::Ask {
                field,
                focus: Stop::Name,
                saved,
                pick,
                error,
                ..
            } => match ev.key {
                Key::Named(Named::Up | Named::Down) if !saved.is_empty() && !chord => {
                    let n = saved.len().min(MAX_SAVED_SHOWN);
                    let forward = ev.key == Key::Named(Named::Down);
                    let next = match *pick {
                        None if forward => 0,
                        None => n - 1,
                        Some(at) => cycle(at, n, forward),
                    };
                    *pick = Some(next);
                    set_text(field, &saved[next]);
                    *error = None;
                }
                _ => {
                    let before = field.text().to_string();
                    if edit(field, ev) && field.text() != before {
                        *error = None;
                    }
                }
            },
            Form::Ask { focus, queued, .. } => {
                if let Some(forward) = arrow(ev) {
                    let stops: &[Stop] = if *queued > 0 { &ASK_STOPS_CLEAR } else { &ASK_STOPS };
                    *focus = step(stops, *focus, forward);
                }
            }
```

- `NewForm::other_key`: `Stop::No => {}` becomes `Stop::No | Stop::Clear => {}`.
- `paste()`: `Form::Ask { field, focus: Stop::Name, error, .. } => { field.paste(text); *error = None; }`.
- `click()`: `(Target::Stop(Stop::Clear), Form::Ask { key, .. }) => return FormResult::ClearQueue(key.clone()),` and `(Target::Stop(Stop::Name), Form::Ask { field, focus, .. }) => { *focus = Stop::Name; field.click(col); }`.
- `panel()`:

```rust
            Form::Ask {
                key,
                field,
                saved,
                pick,
                queued,
                focus,
                error,
                ..
            } => {
                let inner = ASK_COLS - 2 * PAD;
                let mut p = Panel::new(ASK_COLS);
                let title = match *queued {
                    0 => format!("Send to {}", key.name),
                    n => format!("Send to {} · {n} queued", key.name),
                };
                p.put(0, PAD, inner, &title, Look::Title);
                p.field(2, PAD, inner, field, Stop::Name, *focus);
                if let Some(error) = error {
                    p.put(3, PAD, inner, error, Look::Error);
                }
                p.put(4, PAD, inner, "Saved (↑ / ↓)", Look::Label);
                let shown = saved.len().min(MAX_SAVED_SHOWN);
                for (n, prompt) in saved.iter().take(shown).enumerate() {
                    let look = match *pick == Some(n) {
                        true => Look::Choice {
                            focused: *focus == Stop::Name,
                            on: true,
                        },
                        false => Look::Dim,
                    };
                    p.put(5 + n, PAD, inner, prompt, look);
                }
                let buttons = 5 + shown + 1;
                if *queued > 0 {
                    let label = " Clear queue ";
                    let look = Look::Button {
                        focused: *focus == Stop::Clear,
                        danger: true,
                    };
                    p.put(buttons, PAD, label.len(), label, look).target = Some(Target::Stop(Stop::Clear));
                }
                p.buttons(buttons, "Send", false, *focus);
                p.lines = buttons + 2;
                p
            }
```

Menu: `MenuItem::Send` (label `"Send message…"`); `menu_items` ends with `if info.is_some_and(is_claude) { items.push(MenuItem::Send); }`. Free functions:

```rust
/// A Claude session in tmux: its status file named it (session id or pane). Messages go only
/// to those.
pub fn is_claude(info: &SessionInfo) -> bool {
    info.key.host != KITTY && (info.session_id.is_some() || info.pane.is_some())
}

/// tmls's saved prompts (`~/.config/tmls/prompts`: one per line, `#` comments), or the default.
pub fn saved_prompts(text: Option<&str>) -> Vec<String> {
    let saved: Vec<String> = text
        .unwrap_or("")
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    if saved.is_empty() { vec![DEFAULT_PROMPT.to_string()] } else { saved }
}

/// The input runner the app uses: `tmux::run_with_stdin` with `REQUEST_TIMEOUT`.
pub fn run_tmux_with(argv: &[String], input: &[u8]) -> anyhow::Result<String> {
    tmux::run_with_stdin(argv, input, REQUEST_TIMEOUT)
}

/// Types `text` into Claude's pane of the session and presses Enter; says so.
pub fn send_text(
    key: &SessionKey,
    pane: Option<&str>,
    text: &str,
    local_name: &str,
    feed: InputRunner,
) -> Result<String, String> {
    let argv = tmux::send_text_argv(key, pane, &tmux::buffer_name(), local_name);
    feed(&argv, text.as_bytes())
        .map(|_| format!("sent to {}", key.name))
        .map_err(|e| format!("couldn't send to {}: {}", key.name, first_line(&e)))
}
```

- [ ] **Step 6: keys, theme, draw.** `keys.rs`: `ListKey::Send` (`/// Send or queue a message to the cursor's Claude session.`), `list_key`: `Key::Char('s') if !ev.repeat => Some(ListKey::Send),`; `Global::Ask` (`/// The message form for the front tab's Claude session.`); in `binding`'s Alt+Shift match: `Key::Char(c) if c.eq_ignore_ascii_case(&'a') => Some(Global::Ask),`. `HELP`: Everywhere after `Alt+Shift+N`: `("Alt+Shift+A", "send a message to this tab"),`; List after `("y", …)`: `("s", "send or queue a message"),`; Forms: `("↑ / ↓", "choose")` → `("↑ / ↓", "choose; a saved message")`. Module comment: "s — the message form; Alt+Shift+A — the same for the front tab".

`theme.rs`:

```rust
/// After a session's name while messages wait for it (`3✉`), and its colour.
pub const QUEUED_MARK: &str = "✉";
pub const QUEUED_RGB: [u8; 3] = [0xc6, 0x78, 0xdd];
```

`draw.rs`: import both; `LEGEND_MORE` becomes `[(&str, [u8; 3], &str); 3]` with `(QUEUED_MARK, QUEUED_RGB, "queued")` appended; `RowLook` gains `/// Messages waiting for the session: \`N✉\` after the name.\n pub queued: usize,`; `View` gains `/// Messages waiting, per session.\n pub queued: Vec<(SessionKey, usize)>,`; in `session_spans` the `after` list becomes `Vec<(String, [u8; 3])>`:

```rust
    let queued = (look.queued > 0).then(|| (format!("{}{QUEUED_MARK}", look.queued), QUEUED_RGB));
    let after: Vec<(String, [u8; 3])> = [
        queued,
        look.unread.then(|| (UNREAD_MARK.to_string(), UNREAD_RGB)),
        look.pinned.then(|| (PIN.to_string(), PIN_RGB)),
    ]
    .into_iter()
    .flatten()
    .collect();
```

(the loop pushes `Span::whole(0, col, text)` with the owned `String`; `columns(&text)`). In `sidebar()`'s `RowLook`: `queued: view.queued.iter().find(|(k, _)| k == key).map_or(0, |(_, n)| *n),`.

- [ ] **Step 7: Run the unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test queue --test forms --test keys --test sidebar_draw` and `source spikes/s1/build-env.sh && cargo test -p client-core --test tmux`
Expected: pass.

- [ ] **Step 8: Wire `app.rs`.** Imports: `crate::queue::Queue`. `UserEvent` gains `/// A queued message was sent (its session, its text), or not.\n Sent(SessionKey, String, Result<String, String>),`; route `UserEvent::Sent(key, text, result) => running.sent(key, text, result),`. `Running` gains `queue: Queue,` (`Queue::default()`) and `/// tmls's saved prompts, \`~/.config/tmls/prompts\`.\n prompts_file: PathBuf,` (`config_dir.join("prompts")`).

New methods:

```rust
    /// `s` in the list, the menu's Send message…, or Alt+Shift+A: the message form for a
    /// Claude session.
    fn ask_form(&mut self, key: SessionKey) {
        let Some(info) = self.sidebar.info(&key) else {
            return self.say(Err(format!("{} is not listed any more", key.name)));
        };
        if !forms::is_claude(info) {
            return self.say(Err(format!("{} is not a Claude session", key.name)));
        }
        let pane = info.pane.clone();
        let saved = forms::saved_prompts(std::fs::read_to_string(&self.prompts_file).ok().as_deref());
        let queued = self.queue.count(&key);
        self.open_modal(Modal::Form(Form::ask(key, pane, saved, queued)), None);
    }

    /// The head of a session's queue, sent on a thread; the answer is `UserEvent::Sent`.
    fn deliver_queued(&mut self, key: SessionKey, text: String) {
        let pane = self.sidebar.info(&key).and_then(|info| info.pane.clone());
        let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
        std::thread::spawn(move || {
            let result = forms::send_text(&key, pane.as_deref(), &text, &local, &forms::run_tmux_with);
            let _ = proxy.send_event(UserEvent::Sent(key, text, result));
        });
    }

    fn sent(&mut self, key: SessionKey, text: String, result: Result<String, String>) {
        self.queue.sent(&key, &text, result.is_ok());
        let left = self.queue.count(&key);
        self.say(match result {
            Ok(_) if left > 0 => Ok(format!("sent a queued message to {} ({left} waiting)", key.name)),
            Ok(_) => Ok(format!("sent the queued message to {}", key.name)),
            Err(e) => Err(format!("{e}; {left} queued message(s) kept")),
        });
    }
```

- `snapshot()`: before `self.sidebar.deliver(…)` (and before `front` is taken):

```rust
        let queued: Vec<(SessionKey, Option<Status>)> = self
            .queue
            .keys()
            .map(|key| (key.clone(), self.sidebar.status(key)))
            .collect();
```

  and after the `looked` block of Task 2:

```rust
        // A turn that ended sends the head of its session's queue.
        for (key, before) in queued {
            let after = self.sidebar.status(&key);
            if let Some(text) = self.queue.due(&key, before, after) {
                self.deliver_queued(key, text);
            }
        }
```

  (`front` borrows `self.tabs`; if the borrow checker objects to `deliver_queued(&mut self)` while `front` is alive, move this loop after the last use of `front`, before `self.invalidate()`.)
- `form_result`:

```rust
            FormResult::Ask { key, pane, text } => {
                self.close_modal();
                if Queue::busy(self.sidebar.status(&key)) {
                    let waiting = self.queue.add(&key, text);
                    self.say(Ok(format!("queued for {} ({waiting} waiting)", key.name)));
                } else {
                    let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
                    std::thread::spawn(move || {
                        let said = forms::send_text(&key, pane.as_deref(), &text, &local, &forms::run_tmux_with);
                        let _ = proxy.send_event(UserEvent::Said(said));
                    });
                }
            }
            FormResult::ClearQueue(key) => {
                self.close_modal();
                self.queue.clear(&key);
                self.say(Ok(format!("cleared the queue of {}", key.name)));
            }
```

- `menu_result`: `MenuResult::Pick(MenuItem::Send) => { self.close_modal(); self.ask_form(key); }`.
- `list_key`: `ListKey::Send => { if let Some(key) = self.sidebar.selected().cloned() { self.ask_form(key); } }`.
- `global`: `Global::Ask => { if let Some(index) = self.tabs.active() { let key = self.tabs.key(index).clone(); self.ask_form(key); } }`.
- `done`: `Request::Rename(old, name)` arm, after `self.sidebar.renamed(&old, &key);`: `self.queue.renamed(&old, &key);`; `Request::Kill(key)` arm first line: `self.queue.forget(&key);`.
- `build_view`: `queued: self.queue.keys().map(|key| (key.clone(), self.queue.count(key))).collect(),`.

- [ ] **Step 9: e2e block.** Top: `ASK='tmlsrs-test-1ask'`, `ASK_SID='2b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'`; `cleanup()`: `tmux kill-session -t "=$ASK" 2>/dev/null`. Before `# --- quit` (after the F5 block; the list has the keys):

```bash
# --- send or queue a message (F7) ----------------------------------------------------------------
# $ASK is a shell that a status file calls Claude's; it sorts second.
tmux new-session -d -s "$ASK" -x 100 -y 30
fake_claude "$ASK" "$ASK_SID" idle
sleep 6
top_of_list
keys -k j
keys s
check "screenshot 37-ask (Send to $ASK, the default saved prompt)" shot 37-ask
keys "echo tmls-ask-ok-$NONCE"
key_enter
if wait_for 8 pane_has_line "$ASK" "tmls-ask-ok-$NONCE"; then
  pass "s on an idle Claude row sends the message at once"
else
  fail "s on an idle Claude row sends the message at once" "pane: $(pane_text "$ASK" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
fake_claude "$ASK" "$ASK_SID" busy
sleep 6
keys s
keys "echo tmls-queued-$NONCE"
key_enter
sleep 2
check "a message for a busy session waits in the queue" \
  bash -c '! tmux capture-pane -p -t "=$0:" | grep -qxF -- "$1"' "$ASK" "tmls-queued-$NONCE"
check "screenshot 38-queued (1✉ on the row)" shot 38-queued
fake_claude "$ASK" "$ASK_SID" idle
if wait_for 12 pane_has_line "$ASK" "tmls-queued-$NONCE"; then
  pass "when its turn ends the queued message is sent"
else
  fail "when its turn ends the queued message is sent" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
```

- [ ] **Step 10: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; client-core tests; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/t7-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/night2/t7-e2e/37-ask.png` and `/data/scratch/tmls-rs-v0/night2/t7-e2e/38-queued.png` (`1✉` after the name).

- [ ] **Step 11: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): send a message to a Claude session from the list, or queue it until its turn ends"
```

---

### Task 8: F8 — a failed new session keeps what was typed; a tab follows a rename made elsewhere

**Files:**
- Modify: `crates/desktop/src/forms.rs` (`Form::new_session_again`, next to `new_session`)
- Modify: `crates/desktop/src/sidebar.rs` (free fn `renamed_outside` after `summary`)
- Modify: `crates/desktop/src/tabs.rs` (add `rekey` after `index_of`)
- Modify: `crates/desktop/src/app.rs` (`done`, `snapshot`, new `follow_rename`)
- Modify: `tooling/smoke/e2e.sh` (top variable, `cleanup`, block before `# --- quit`)
- Test: `crates/desktop/tests/forms.rs`, `crates/desktop/tests/sidebar.rs`, `crates/desktop/tests/tabs.rs`

**Interfaces:**
- Consumes: `Queue::renamed` (Task 7), `Sidebar::renamed`, `take_pin_error` (exist), `fake_claude`, `top_of_list` (Task 5).
- Produces: `Form::new_session_again(hosts: &[String], presets: &[(String, Vec<String>)], failed: &NewSession, error: String) -> Form`; `sidebar::renamed_outside(before: &[SessionInfo], snap: &HostSnapshot) -> Vec<(SessionKey, SessionKey)>`; `Tabs::rekey(&mut self, index: usize, key: SessionKey)`.
- Found while planning (not a departure, a limit): the listing pairs a status file with a session by the session name inside the file's `tmux` field (`listing.rs:203`, `:367`). A session renamed outside the app has no `session_id` until Claude writes its status file again (its next status change), so the tab follows at that listing, not at the rename itself. The e2e rewrites the status file to stand in for Claude.

- [ ] **Step 1: Failing tests.** `crates/desktop/tests/forms.rs`:

```rust
#[test]
fn a_failed_new_session_reopens_the_form_with_what_was_typed() {
    let failed = NewSession {
        host: "ph".to_string(),
        folder: "~/src/app".to_string(),
        name: "app-2".to_string(),
        start: Start::Preset(vec!["codex".to_string()]),
        size: Some((100, 30)),
    };
    let why = "couldn't create app-2 on ph: no such folder: ~/src/app".to_string();
    let mut form = Form::new_session_again(&hosts(), &presets(), &failed, why.clone());
    assert_eq!(form.error(), Some(why.as_str()));
    assert_eq!(form.focus(), Stop::Name);
    let Form::New(new) = &form else {
        panic!("a new-session form")
    };
    assert_eq!((new.host_name(), new.folder.text(), new.name.text()), ("ph", "~/src/app", "app-2"));
    assert_eq!(new.starts()[new.start].0, "codex");
    assert!(new.name_edited, "the name is the user's: no lookup replaces it");
    match form.key(&named(Named::Enter)) {
        FormResult::Submit(Request::New(again)) => assert_eq!(again, NewSession { size: None, ..failed }),
        other => panic!("{other:?}"),
    }
}
```

`crates/desktop/tests/sidebar.rs` (import `desktop::sidebar::renamed_outside`):

```rust
#[test]
fn renamed_outside_follows_claudes_session_id() {
    let id = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";
    let with = |name: &str, id: Option<&str>| SessionInfo {
        session_id: id.map(str::to_string),
        ..session("nas", name, Status::Idle)
    };
    let before = vec![with("old", Some(id)), with("plain", None)];
    let after = snapshot("nas", vec![with("new", Some(id)), with("plain2", None)]);
    assert_eq!(renamed_outside(&before, &after), [(key("nas", "old"), key("nas", "new"))]);
    // Still listed under its name: no rename. A host away says nothing. Another host's
    // listing is not this one's.
    let both = snapshot("nas", vec![with("old", Some(id)), with("new", Some(id))]);
    assert!(renamed_outside(&before, &both).is_empty());
    assert!(renamed_outside(&before, &offline("nas")).is_empty());
    assert!(renamed_outside(&before, &snapshot("ph", vec![with("new", Some(id))])).is_empty());
}
```

`crates/desktop/tests/tabs.rs`:

```rust
#[test]
fn rekey_gives_a_tab_a_new_key_and_keeps_its_session() {
    let _serial = serial();
    let mut tabs = tabs(&[("old", "sleep 30".to_string())]);
    let index = tabs.open(&key("old"), GRID).unwrap();
    tabs.rekey(index, key("new"));
    assert_eq!(tabs.key(index), &key("new"));
    assert_eq!(tabs.index_of(&key("old")), None);
    assert_eq!(tabs.index_of(&key("new")), Some(index));
    assert_eq!(tabs.state(index), TabState::Live);
}
```

- [ ] **Step 2: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test forms --test sidebar --test tabs`
Expected: compile errors (`new_session_again`, `renamed_outside`, `rekey`).

- [ ] **Step 3: Implement.** `forms.rs`, after `new_session`:

```rust
    /// The new-session form again after Create failed: the host, folder, name and start that
    /// were asked for, and why it failed; the focus on the name.
    pub fn new_session_again(
        hosts: &[String],
        presets: &[(String, Vec<String>)],
        failed: &NewSession,
        error: String,
    ) -> Form {
        let mut form = NewForm::new(hosts, &failed.host, presets);
        set_text(&mut form.folder, &failed.folder);
        set_text(&mut form.name, &failed.name);
        form.name_edited = true;
        form.looked_up = (form.host, failed.folder.clone());
        if let Some(index) = form.starts.iter().position(|(_, start)| *start == failed.start) {
            form.start = index;
        }
        form.focus_on(Stop::Name);
        form.error = Some(error);
        Form::New(form)
    }
```

`sidebar.rs`, after `summary`:

```rust
/// The sessions of `snap`'s host renamed outside the app since `before` (that host's rows as
/// the list had them): a name the listing no longer has whose Claude conversation
/// (`session_id`) it lists under another name. Pairs (old, new). Sessions without a Claude id
/// are not followed: nothing says which new name is theirs.
pub fn renamed_outside(before: &[SessionInfo], snap: &HostSnapshot) -> Vec<(SessionKey, SessionKey)> {
    if !snap.online {
        return Vec::new();
    }
    before
        .iter()
        .filter(|old| old.key.host == snap.host && !snap.sessions.iter().any(|s| s.key == old.key))
        .filter_map(|old| {
            let id = old.session_id.as_ref()?;
            let new = snap.sessions.iter().find(|s| {
                s.key.host == snap.host && s.session_id.as_ref() == Some(id) && s.key != old.key
            })?;
            Some((old.key.clone(), new.key.clone()))
        })
        .collect()
}
```

`tabs.rs`, after `index_of`:

```rust
    /// The tab's session was renamed outside the app: the tab goes on under `key`. Its attach
    /// command follows the session by itself and is not started again; a reattach uses `key`.
    pub fn rekey(&mut self, index: usize, key: SessionKey) {
        self.tabs[index].key = key;
    }
```

`app.rs`:
- `done()`: replace `let ok = result.is_ok(); self.say(result); if !ok { return; }` by

```rust
        if let (Request::New(new), Err(why)) = (&request, &result) {
            // What was typed is not lost: the form comes back with it and the reason.
            let form = Form::new_session_again(&self.cfg.hosts, &self.cfg.presets, new, why.clone());
            self.say(result);
            if self.modal.is_none() {
                self.open_modal(Modal::Form(form), None);
            }
            return;
        }
        let ok = result.is_ok();
        self.say(result);
        if !ok {
            return;
        }
```

- `snapshot()`: first, before `let front = …` (and before Task 6's/7's lines that need `front`), right after the `push::carry_replies`/`listings`/`follow_push` lines:

```rust
        // Sessions renamed outside the app: their tabs, pins and queues follow the new name.
        let before: Vec<SessionInfo> = self
            .sidebar
            .rows()
            .iter()
            .filter_map(|row| match row {
                sidebar::Row::Session(info) if info.key.host == snapshot.host => Some(info.clone()),
                _ => None,
            })
            .collect();
        for (old, new) in sidebar::renamed_outside(&before, &snapshot) {
            self.follow_rename(&old, &new);
        }
```

  (import `client_core::SessionInfo`), and the new method:

```rust
    /// A session with a tab was renamed elsewhere: the tab, its ★ and its queue take the new
    /// name; the window's title too when it is in front.
    fn follow_rename(&mut self, old: &SessionKey, new: &SessionKey) {
        let Some(index) = self.tabs.index_of(old) else {
            return;
        };
        if self.tabs.index_of(new).is_some() {
            return;
        }
        self.tabs.rekey(index, new.clone());
        self.sidebar.renamed(old, new);
        if let Err(e) = self.sidebar.take_pin_error() {
            crate::errln(format_args!("tmls-rs: {e:#}"));
        }
        self.queue.renamed(old, new);
        if self.tabs.active() == Some(index) {
            self.window.set_title(&format!("{} · {}", new.name, new.host));
        }
        self.say(Ok(format!("{} is now {} (renamed outside tmls-rs)", old.name, new.name)));
    }
```

- [ ] **Step 4: Run the unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test forms --test sidebar --test tabs`
Expected: pass.

- [ ] **Step 5: e2e block.** Top: `ASK2='tmlsrs-test-1asked'`; `cleanup()`: `tmux kill-session -t "=$ASK2" 2>/dev/null`. Before `# --- quit` (after F7; the list's cursor is on $ASK):

```bash
# --- a session renamed outside the app keeps its tab (F8) ---------------------------------------------
key_enter
if wait_for 8 attached_to "$ASK"; then pass "Enter attaches $ASK"; else fail "Enter attaches $ASK" "clients: $(attached | tr '\n' '|')"; fi
tmux rename-session -t "=$ASK" "$ASK2"
# Claude writes its status file again under the new name (here: the stand-in does).
fake_claude "$ASK2" "$ASK_SID" idle
titled() { sway_msg -t get_tree | grep -qF -- "$ASK2"; }
if wait_for 12 titled; then
  pass "a session renamed outside the app keeps its tab: the window is titled $ASK2"
else
  fail "a session renamed outside the app keeps its tab: the window is titled $ASK2" "stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
check "screenshot 41-renamed-outside" shot 41-renamed-outside
check_typed "the renamed session's tab still takes the keys" "$ASK2" renamedout
key_close_tab
check_typed "its tab closed: the keys go to \"$ODD\"" "$ODD" afterrename
```

- [ ] **Step 6: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/t8-e2e tooling/smoke/run.sh`
Expected: all pass; `/data/scratch/tmls-rs-v0/night2/t8-e2e/41-renamed-outside.png` shows the tab named `tmlsrs-test-1asked`.

- [ ] **Step 7: Commit.**

```bash
git add crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "fix(desktop): a failed new session keeps what you typed, and a tab follows a session renamed elsewhere"
```

---

### Task 9: Whole-branch review, speed before/after, e2e three times

**Files:**
- Create (scratch only): `/data/scratch/tmls-rs-v0/night2/review/branch.diff`, `/data/scratch/tmls-rs-v0/night2/bench/…`, `/data/scratch/tmls-rs-v0/night2/e2e-{1,2,3}/…`
- Modify: whatever a confirmed finding needs, one commit per finding, test first.

**Interfaces:** consumes the whole branch; produces the review's findings (in the main agent's ledger, then in `PROGRESS.md`) and the speed table.

- [ ] **Step 1: The diff for the reviewer.**
Run: `mkdir -p /data/scratch/tmls-rs-v0/night2/review && git diff main..feat/attention-loop > /data/scratch/tmls-rs-v0/night2/review/branch.diff && wc -l /data/scratch/tmls-rs-v0/night2/review/branch.diff`

- [ ] **Step 2: One fresh Opus reviewer** (`model: opus`) on that file, the spec and this plan, with this focus list and not the main agent's conclusions: the five Review Focus lines; quoting of every word that reaches a shell (`send_text_script`, `capture_argv`, `send_keys_argv`, `TAIL_SCRIPT`, pane ids); threads and processes that can outlive the window or pile up (`push::Tail` per host, restart loop, kill at drop, ssh ControlMaster holding pipes: `tmux::run`'s 1 s grace and `push::follow`); every timeout; the queue's state machine against renames and kills during a send; push applying to a listing that went offline; `force_next` and the poller's comparison; `Seen` unread vs ◆; anything new on the key-to-frame path; panics on empty lists, zero panes, a closed stdout (`tmls-rs keys | head`).

- [ ] **Step 3: Rule on each finding** (fix now / later / never), fix the "now" ones one commit each with a failing test first, run the full suite after each.

- [ ] **Step 4: Speed, before.** A detached worktree of `main` in scratch (created, never removed without the user's yes):

```bash
B=/data/scratch/tmls-rs-v0/night2
git worktree add --detach "$B/base" d235ee3
(cd "$B/base" && tooling/docker/build.sh build --release -p desktop)
for n in 1 2 3; do
  SMOKE_BIN="$B/base/target-docker/release/tmls-rs" SMOKE_OUT="$B/bench/base-closed-$n" BENCH_STATE=closed tooling/smoke/bench-run.sh
done
```

- [ ] **Step 5: Speed, after.**

```bash
tooling/docker/build.sh build --release -p desktop
for n in 1 2 3; do
  SMOKE_BIN=target-docker/release/tmls-rs SMOKE_OUT="$B/bench/head-closed-$n" BENCH_STATE=closed tooling/smoke/bench-run.sh
done
python3 -I tooling/smoke/bench-report.py "$B"/bench/base-closed-* "$B"/bench/head-closed-*
```

Expected: head's median and p95 key→frame within the base runs' spread; idle frames 0 in every run (as base). A miss is a finding: find what draws while idle (the push reader's `Available` line at start is one redraw, before the idle window; a redraw per poll would be a bug), fix, re-measure.

- [ ] **Step 6: e2e three times in a row**, each into a fresh directory:

```bash
for n in 1 2 3; do SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/e2e-$n tooling/smoke/run.sh || break; done
```

Expected: three times `== 0 of N checks failed`. A flaky check is a finding (fix the cause, not the wait), and the three runs start again.

- [ ] **Step 7:** Final `tooling/docker/build.sh test -p desktop`, `source spikes/s1/build-env.sh && cargo test -p client-core`, clippy, fmt — all clean. Note the verified commit hash for the PC build.

---

### Task 10: Docs, the hand-off sheet, PROGRESS

**Files:**
- Modify: `docs/desktop-v0.md` (sections `## Config files` at line 56, `## Keys` at line 72, `## Mouse` at line 205)
- Create: `docs/NIGHT-TEST.md`
- Modify: `PROGRESS.md` (the main agent writes this entry itself)

**Interfaces:** consumes everything above; produces documentation only.

- [ ] **Step 1: `docs/desktop-v0.md`.**
  - `## Config files`: add rows

    | `prompts` | saved messages for the Send form, one per line; `#` starts a comment (default: `What's the progress?`) | read |

    and under the table: "Also read, never written: `~/.cache/tmls/events.jsonl` on each host (written by `tmls hooks install`; without it the host's header says `· poll`). Environment: `TMLS_RS_POLL_S` (listing period in seconds, default 5), `TMLS_RS_NO_CONTROLMASTER=1` (no shared ssh connection; the keepalive stays). ssh sockets: `$XDG_RUNTIME_DIR/tmls-rs/cm-*`, else `~/.cache/tmls-rs/cm-*`."
  - `## Keys`: keep its first paragraph; replace the tables by "The full table is in the app (`?` in the list, or the `?` button) and on the command line:" followed by a ```text block holding the exact output of `target-docker/release/tmls-rs keys` at the verified commit (run it and paste; do not retype), then one paragraph each on approvals (`y`, Enter Yes, `n` No, Esc Cancel; nothing is sent if the dialog changed), messages (`s` / Alt+Shift+A; queued while running or waiting, sent one per turn end, lost at quit) and unread (`•`; Alt+Shift+N visits unread sessions when nothing waits).
  - `## Mouse`: rows `| the \`?\` button | the keys and mouse actions |`, `| right-click a session that asks | its menu starts with Approve… |`, `| right-click a Claude session | its menu ends with Send message… |`; under "Mouse in the terminal" one line: "Every copy and paste says in the status line what it did (`copied N chars`, `nothing selected`, `pasted N chars`, `clipboard empty`, or why it failed)."

- [ ] **Step 2: `docs/NIGHT-TEST.md`** — what the user tries in the morning, in the order they meet it, one box each:

```markdown
# Night of 2026-10-12: what to try (tmls-rs attention loop)

The PC has the build of `<verified commit>` (or: not built — game mode was on; say "build it").
Start it as usual with `tmls-rs`. Each line: ok / not ok / a note. Screenshots of the headless
run are under `/data/scratch/tmls-rs-v0/night2/e2e-3/`.

## Look
- [ ] `?` in the list (or the `?` button): the keys panel; Esc closes it. Anything missing or wrong?
- [ ] A session with output you have not looked at has a `•` after its name; it goes when you look at its tab.
- [ ] A host without `~/.cache/tmls/events.jsonl` says `· poll` beside its name.

## Copy and paste (reported broken on KDE)
- [ ] Shift+drag over text in a tab: the status line says `copied N chars`.
- [ ] Ctrl+Shift+C on a selection, then paste in another app: the text arrives.
- [ ] Ctrl+Shift+V in a tab: `pasted N chars`, the text arrives. If not: the status line's words, and the last lines on the terminal you started tmls-rs from.

## Act from the list
- [ ] A Claude asking permission: `y` on its row (or Approve… in its right-click menu, or `y` in the alerts) shows the dialog; Enter approves, `n` declines, Esc does nothing.
- [ ] Answer a prompt in the terminal first, then press Enter in the form: the status line says it isn't asking that any more, and nothing is typed.
- [ ] `s` on an idle Claude row: send `What's the progress?`; it arrives as typed.
- [ ] `s` on a working Claude row: the row shows `1✉`; the message arrives when the turn ends.
- [ ] Alt+Shift+A in a Claude tab opens the same form.

## Faster status (needs `tmls hooks install` done on the host)
- [ ] A permission prompt shows `?` and notifies within a second, not five.
- [ ] When a turn ends, the row's dim line shows the first line of the reply.

## Links that sleep
- [ ] Suspend a remote host (or pull its network) with its tab open: within ~45 s the tab says ended; Enter reattaches when it is back.
- [ ] Opening a remote tab feels quicker than before (shared ssh connection).

## Small fixes
- [ ] New session in a folder that does not exist: the form comes back with what you typed and the reason.
- [ ] Rename a Claude session from a shell (`tmux rename-session`): its tab follows after Claude's next status change.

## Decisions to veto (spec D1–D10)
Hooks stay tmls's; push is `tail -F`; poll is the truth; Yes=`1`, No=Esc; saved prompts only; queue in memory; `•` without animation; ControlMaster on; keys `y` `s` `?` Alt+Shift+A; last reply only with push.
```

(The main agent puts the verified commit hash where the sheet says `<verified commit>`, or the game-mode sentence, before committing.)

- [ ] **Step 3: PC build** (main agent, see "Who does the work"): game mode first, as its own command; build only if `off`.

- [ ] **Step 4: PROGRESS entry** (main agent): what was built per task with its commit, the e2e count and the three run directories, the speed table (base vs head), the review's findings and rulings, the clipboard's cause (Task 4), what is not verified in a real window, the scratch worktree `/data/scratch/tmls-rs-v0/night2/base` left in place (removal needs the user's yes), and NEXT = the user's NIGHT-TEST answers.

- [ ] **Step 5: Commit.**

```bash
git add docs/desktop-v0.md docs/NIGHT-TEST.md PROGRESS.md
git commit -m "docs: keys, config and the morning's test sheet for the attention loop"
```

---

## Self-review (done while writing; kept for the executor)

**Spec coverage.** F1 → T3 (panel, `?` key and button, Esc/`?`/click outside, `tmls-rs keys`, one table). F2 → T4 (diagnose first via the e2e, `wl-copy`/`wl-paste` fallback, six status texts, Ctrl+C unchanged in `keys::copy_instead`, `wl-clipboard` in the image, e2e with `wl-paste`). F3 → T1 (keepalive, ControlMaster, dir 0700, env switch, one builder). F4 → T2 (`pane`, unread map, `•` before `★`, legend, never front-focused / first listing / outside tmux, Alt+Shift+N). F5 → T5 (menu item only while asking, `y` in list and alerts, capture, dialog rules, 12-line cap, Yes/No/Cancel keys, re-capture, `1`/Escape, `refresh_now` via `done`, off the UI thread, fixture, e2e). F6 → T6 (read-only tail, restart 10 s, stop when offline, parser, mapping, sid then pane, `t` rule, `· poll`, `last_reply`, notify within seconds, `TMLS_RS_POLL_S`, e2e). F7 → T7 (entries, Claude-only, form, saved prompts, send script on stdin, queue rules, rename/kill, `N✉`, legend, e2e). F8 → T8 (form reopens, rename by `session_id`, e2e). Testing section → T9 (three e2e runs, bench before/after, no injection; hand-off sheet T10). D1 T6 (`tail_argv` test: reads only), D2 T6, D3 T6 (`force_next`), D4 T5, D5 T7 (`saved_prompts`, no recent), D6 T7, D7 T2, D8 T1, D9 T3/T5/T7, D10 T6 (`summary`).

**Placeholders.** The only angle-bracket texts left are `<verified commit>` and the clipboard cause line, which the main agent fills from files produced during the run (Task 4 Step 7, Task 9 Step 7) before the commit that carries them; the plan says so where each appears.

**Type consistency.** `ssh_base(host, flags)` T1 → used by `ssh`, attach, listing. `valid_pane` T2 → listing, `pane_target` T5, `push::parse_line` T6. `SessionInfo { pane, activity }` T2, `last_reply` T6 — every later test helper lists them. `forms::Runner` / `InputRunner` (T4) → `approval`/`answer` (T5), `send_text` (T7), `clip` (T4). `first_line` T5 → `send_text` T7. `menu_items`/`RowMenu::with` T5 → extended in T7 (test updated there). `Sidebar::info` T5 → `ask_form`, `deliver_queued` T7. `Queue::renamed` T7 → `follow_rename` T8. `View` literals end in `..View::default()` from T2 on, so `polled` (T6) and `queued` (T7) need no test edits. `RowLook` literals already use `..RowLook::default()`.
