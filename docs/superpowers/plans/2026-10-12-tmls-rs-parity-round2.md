# tmls-rs: parity round 2 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything a daily tmls user still misses in tmls-rs, so the Python sidebar can stop being launched: window and pane actions in the row menu, game-mode awareness, a sign-in row, alert switches with sound, recent prompts in the Ask form, other tmux servers in a collapsed section, quick select and image paste.

**Architecture:** As in round 1: everything that runs on a host is an argv built in `client-core` (`tmux.rs` gains the window, pane, socket, sign-in and paste builders; three new pure modules `gamemode.rs`, `prompts.rs`, `imagepaste.rs`), every remote command runs on a thread and comes back as a `UserEvent`, new modals are `forms::Form` variants or a `Modal` of their own (`select::Select`). One new thread: a 30 s ticker (`UserEvent::Tick`) that checks game mode and rescans the hosts' other tmux servers; the poller gains `slow_down` so game mode can stretch the remote hosts' period without restarting it. Quick select lives in `desktop` beside `links.rs` so its URL and path rules are the ones Ctrl+click uses.

**Tech Stack:** Rust 2024, winit 0.30, wgpu 30, `s2-core` / `s2-wgpu` (terminal, `TextField`, `Frame`), `arboard` 3.6, `serde_json`, POSIX sh + tmux + OpenSSH on the hosts; `wl-paste`, `canberra-gtk-play`, `game-mode` as optional programs on PATH; headless e2e in Docker (sway, lavapipe, wtype, grim, `vptr.py`, `wl-clipboard`). No new crate.

**Spec:** `docs/superpowers/specs/2026-10-12-tmls-rs-parity-round2-design.md` (features G1–G8, decisions R1–R8). Read it with this plan; where this plan departs from it, the task says so and why ("Departs from the spec").

**Starting point:** round 1 (`docs/superpowers/plans/2026-10-12-tmls-rs-attention-loop.md`, Tasks 1–10) is complete and committed on `feat/attention-loop`. Before Task 1 the main agent runs `git checkout -b feat/parity-round2 feat/attention-loop` and notes `BASE=$(git rev-parse feat/attention-loop)` in its ledger (Task 9 measures against it). All work is committed on `feat/parity-round2`. Nothing is merged or pushed.

**Names this plan takes from round 1** (they do not exist at `1245e9c`; they are produced by round 1's Tasks 5–8 — read those tasks' Interfaces if a name below is unclear): `forms::Form::Approve`, `Form::Ask { key, pane, field, saved, pick, queued, focus, error }`, `Form::ask(key, pane, saved, queued)`, `FormResult::Ask`, `Request::Answer`, `forms::menu_items`, `RowMenu::with`, `RowMenu::items`, `forms::first_line`, `forms::is_claude`, `forms::saved_prompts`, `MAX_SAVED_SHOWN`, `Sidebar::info` (Task 5); `SessionInfo.last_reply`, `Poller::force_next`, `poll::period_from`, `State.resend`, `draw::header_spans_with`, `draw::POLL_ONLY`, `View.polled`, the e2e's second run (Task 6); `tmux::send_text_script`, `send_text_argv`, `buffer_name`, `Running::ask_form`, `Running.prompts_file`, `Running.queue` (Task 7); `tabs::rekey` (Task 8); e2e helpers `fake_claude`, `top_of_list`, `FAKE_PIDS`, `list_picture` (Tasks 5, 6). Line numbers below are at HEAD `260db5a` (round 1 Tasks 1–4); round 1's later tasks move them, so find each place by the function name given.

## Who does the work (the user's instruction for this round: "work over night", unattended)

- **One main agent, model `fable` (this session):** owns the plan from Task 1 to Task 10, keeps the ledger, dispatches, makes the rulings, runs every verification itself (tests, clippy, fmt, e2e, looks at the screenshots) and writes `PROGRESS.md`. It writes no feature code.
- **Opus subagents (`model: opus`), dispatched by the main agent:** one implementer per task, **never two at once** (Tasks 2–8 all touch `crates/desktop/src/app.rs` and most touch `draw.rs`/`forms.rs`); one task reviewer after each task, given `git diff <task start>..HEAD` as a file under `/data/scratch/tmls-rs-v0/round2/review/` and the task's text, not the main agent's conclusions; one whole-branch reviewer in Task 9.
- **The PC:** at the end only, and only after `ssh -o ConnectTimeout=5 pcl 'bash -lic "game-mode status"'` — run as its own command, never chained — prints `off`. Then, as a separate command: `ssh pcl 'cd ~/Others/tmls-rs && git fetch && git checkout --detach <verified commit> && tooling/docker/build.sh build --release -p desktop'`. If game mode is `on`, do not build; say so in the report.
- **Never:** merge, push, delete or move anything (worktrees and scratch included), allocate compute, inject input into the user's KDE session, or message anyone. Stop and report for any of these.
- **Last task:** the hand-off sheet `docs/NIGHT-TEST-2.md` (Task 10) — what the user tries in the morning.

## Global Constraints

- Scratch, logs and e2e output under `/data/scratch/tmls-rs-v0/round2/`, never `/tmp`. `spikes/s1/build-env.sh` resets `TMPDIR`; use explicit paths for logs.
- Git: name `sgkayast`, email `sgn.kayastha@gmail.com` (the repo's config already says so), **no Claude attribution** in commits (no `Co-Authored-By`, no "Generated with"). Commit on `feat/parity-round2` only. No push, no merge.
- Tests never touch the user's live tmux (private server, `tmlsrs-test-` prefix, or a shell `tmux()` stub) and never inject input into the KDE session.
- `client-core` is not rustfmt-clean: do not reformat it wholesale; format only the lines you write in its style. `cargo fmt -p desktop --check` must stay clean.
- Commands:
  - desktop tests: `tooling/docker/build.sh test -p desktop` (one file: `… -p desktop --test NAME`)
  - client-core tests: `source spikes/s1/build-env.sh && cargo test -p client-core` (one file: `--test NAME`)
  - lints: `source spikes/s1/build-env.sh && cargo clippy -p desktop -p client-core --all-targets -- -D warnings`
  - fmt: `source spikes/s1/build-env.sh && cargo fmt -p desktop --check`
  - e2e: `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/<task>-e2e tooling/smoke/run.sh` (a fresh directory each run); it ends with `== 0 of N checks failed`.
- Every image mentioned to the user (screenshots included) is given by its full absolute path.
- The app never writes `~/.claude/settings.json`, never installs hooks, never writes `~/.cache/tmls/` (D1). The only file under `~/.config/tmls` it writes stays `pins`.
  - **Amended by this round's spec (the spec is the authority):** G7 writes `~/.config/tmls/notifications.json` too (only when the user flips a switch, atomically, the three keys tmls writes); G4 (R3) writes image files into `~/.cache/tmls/images/` on the session's host and nothing else under `~/.cache/tmls/`. Its own staging folder is `~/.cache/tmls-rs/paste/`.

## Review Focus

1. A screen with more than 26 targets, a git hash inside a URL or a path, a wide (CJK) character before a target, or nothing selectable at all: at most 26 labels, the bottom-most target is `a`, a hash that is part of a link is no target of its own, each label sits on its target's first cell, and an empty screen opens no overlay — pinned in Task 7 by `targets_keep_the_26_nearest_the_bottom_and_skip_hashes_in_links` (`crates/desktop/tests/quickselect.rs`).
2. Image paste when the clipboard has only text, nothing, no `wl-paste` on PATH, or the copy to the remote host fails: the status line says which (`clipboard has no image`, `wl-paste is not installed: no image paste`, `copy to HOST failed: …`), nothing is typed into the session, and a staged file that did not reach the host is kept — pinned in Task 8 by `a_failed_copy_types_nothing_and_says_why` (`crates/client-core/tests/imagepaste.rs`).
3. A session or window name with spaces, quotes, `$HOME`, `;` or a leading `-`, sent to a remote host for new window, rename window, split or kill pane: tmux receives each word exactly as typed and no shell runs any of it — pinned in Task 1 by `window_commands_quote_every_word_for_a_remote_host` (`crates/client-core/tests/tmux.rs`).
4. A tmux socket directory with sockets named `a b`, `x;y`, a plain file among them, or no directory at all: those servers are skipped, no socket name that is not `[A-Za-z0-9_.-]+` ever reaches a `tmux -L`, and a missing directory lists nothing — pinned in Task 6 by `server_names_that_are_not_names_are_skipped` (`crates/client-core/tests/tmux.rs`).
5. Flipping an alert switch when `~/.config/tmls` is read-only, or turning Sound on without `canberra-gtk-play`: the switch stays as it was and the status line says it could not be saved, the old file is untouched; with no player the switch flips and says there will be no sound — pinned in Task 4 by `a_switch_that_cannot_be_saved_stays_and_says_why` (`crates/desktop/tests/alerts.rs`).

Each line's test is written in the task that owns the code, in that task's first step.

---

## File map

| File | Tasks | Responsibility after this round |
|---|---|---|
| `crates/client-core/src/tmux.rs` | 1, 3, 6, 8 | `Split`, `new_window_argv`, `rename_window_argv`, `split_argv`, `kill_pane_argv`, `pane_count_argv`, `pane_command_argv`, `valid_window_name`; `sign_in_argv`; `SERVER_SOCKETS_SCRIPT`, `server_sockets_argv`, `attach_argv`/`attach_text` with `-L`; `paste_text_script`, `paste_text_argv` |
| `crates/client-core/src/listing.rs` | 3, 6 | `HostSnapshot::needs_sign_in`; `split_host`, `valid_server_name`, `parse_background` |
| `crates/client-core/src/poll.rs` | 2 | `Poller::slow_down` (a period per host) |
| `crates/client-core/src/gamemode.rs` (new) | 2 | `game-mode status`: argv, parse, the 300 s period |
| `crates/client-core/src/prompts.rs` (new) | 5 | `recent_argv`, `parse_recent` |
| `crates/client-core/src/imagepaste.rs` (new) | 8 | `pick_type`, `stamp_name`, `store_argv`, `paste` |
| `crates/desktop/src/forms.rs` | 1, 3, 5 | window/pane menu items, `Form::RenameWindow`, `Form::ConfirmKillPane`, `PaneInfo`, `pane_info`; `sign_in`; `Form::Ask.recent`, `set_recent`, `recent_prompts` |
| `crates/desktop/src/alerts.rs` | 4 | `Settings.sound`, `Settings::save`, `switch`, `emit`, `sound_argv`, the three switches in the panel |
| `crates/desktop/src/sidebar.rs` | 6 | `Row::Background`, `Action::ToggleBackground`, `apply_background`, `toggle_background` |
| `crates/desktop/src/links.rs` | 7 | `row_links`, `frame_rows` |
| `crates/desktop/src/quickselect.rs` (new) | 7 | `targets`, `label`, `Kind`, `Target` |
| `crates/desktop/src/select.rs` (new) | 7 | the overlay's state: `Select`, `Label`, `Picked` |
| `crates/desktop/src/keys.rs` | 6, 7, 8 | `ListKey::Background`, `Global::QuickSelect`, `Global::PasteImage`, `HELP` rows |
| `crates/desktop/src/draw.rs`, `theme.rs` | 2, 3, 6, 7 | `HeaderLook`, `header_spans_look`, `GAME_MODE`, `SIGN_IN`, `background_spans`, label cells; `View.game_hosts`, `.sign_in`, `.labels` |
| `crates/desktop/src/app.rs` | 1–8 | wiring only: ticker, game mode, sign-in click, switches, recent, background, overlay, paste |
| `tooling/smoke/e2e.sh`, `run.sh` | 1–8 | one e2e block per feature; a third app run (remote host, game mode, background server) |

---

### Task 1: G1 — window and pane actions in the row menu

**Files:**
- Modify: `crates/client-core/src/tmux.rs` (add `Split`, `session_tmux`, `new_window_argv`, `rename_window_argv`, `split_argv`, `kill_pane_argv`, `pane_count_argv`, `pane_command_argv`, `valid_window_name` after `running_commands_argv`, `:259-269`)
- Modify: `crates/desktop/src/forms.rs:60-67` (`Request`), `:122-137` (`Form`), `:574-827` (`impl Form`), `:829-950` (`MenuItem`, `MENU`, `RowMenu::panel`), `:964-997` (`request_argv`, `perform`); add `PaneInfo`, `LAST_PANE`, `pane_info`
- Modify: `crates/desktop/src/keys.rs:361-366` (`list_keeps_keys`)
- Modify: `crates/desktop/src/app.rs:122-145` (`UserEvent`), `:197-226` (`user_event`), `:1072-1111` (`menu_result`), `:1161-1225` (`done`); add `run_request`
- Modify: `tooling/smoke/e2e.sh` (top variables, `cleanup`, block before `# --- quit`)
- Test: `crates/client-core/tests/tmux.rs`, `crates/desktop/tests/forms.rs`

**Interfaces:**
- Consumes: `forms::first_line`, `menu_items`, `RowMenu::with` / `items` (from round 1 Task 5); `forms::Runner`, `run_tmux`, `SHELLS` (exist); e2e `top_of_list` (from round 1 Task 5).
- Produces:
  - `client_core::tmux::Split { SideBySide, TopBottom }` (`Clone, Copy, Debug, PartialEq, Eq`)
  - `tmux::new_window_argv(key: &SessionKey, local_name: &str) -> Vec<String>`, `rename_window_argv(key, new_name: &str, local_name) -> Vec<String>`, `split_argv(key, split: Split, local_name) -> Vec<String>`, `kill_pane_argv(key, local_name)`, `pane_count_argv(key, local_name)`, `pane_command_argv(key, local_name)`, `valid_window_name(name: &str) -> Result<(), String>`
  - `forms::Request::{NewWindow(SessionKey), RenameWindow(SessionKey, String), Split(SessionKey, Split), KillPane(SessionKey, bool)}` (the `bool`: it is the session's last pane)
  - `forms::PaneInfo { pub command: Option<String>, pub last: bool }`; `forms::pane_info(key, local_name, run: Runner) -> Result<PaneInfo, String>`; `forms::LAST_PANE: &str`
  - `Form::RenameWindow { key, field: TextField, focus: Stop, error: Option<String> }`, `Form::rename_window(key) -> Form`; `Form::ConfirmKillPane { key, pane: Option<Result<PaneInfo, String>>, focus: Stop }`, `Form::confirm_kill_pane(key) -> Form`, `Form::set_pane_info(&mut self, session: &SessionKey, info: Result<PaneInfo, String>)`
  - `MenuItem::{NewWindow, RenameWindow, SplitSideBySide, SplitTopBottom, KillPane}`; `MENU: [MenuItem; 9]`
  - app: `UserEvent::PaneInfo(SessionKey, Result<forms::PaneInfo, String>)`; `Running::run_request(&mut self, request: Request)`
- Departs from the spec: none in behaviour. The new entries go after the existing four so every menu position round 1 and its e2e count on stays where it was. New window and the splits run at once (tmls does the same); rename window and kill pane open a form. tmux's `-h` is "side by side", `-v` "top / bottom" (tmls `split`).

- [ ] **Step 1: Failing client-core tests.** Append to `crates/client-core/tests/tmux.rs` (add to the `use client_core::tmux::{…}` list: `Split, kill_pane_argv, new_window_argv, pane_command_argv, pane_count_argv, rename_window_argv, split_argv, valid_window_name`):

```rust
#[test]
fn window_and_pane_commands_target_the_sessions_current_window() {
    let k = key(LOCAL, "w");
    assert_eq!(new_window_argv(&k, LOCAL), v(&["tmux", "new-window", "-t", "=w:", "-c", "#{pane_current_path}"]));
    assert_eq!(rename_window_argv(&k, "logs", LOCAL), v(&["tmux", "rename-window", "-t", "=w:", "--", "logs"]));
    assert_eq!(
        split_argv(&k, Split::SideBySide, LOCAL),
        v(&["tmux", "split-window", "-h", "-t", "=w:", "-c", "#{pane_current_path}"])
    );
    assert_eq!(
        split_argv(&k, Split::TopBottom, LOCAL),
        v(&["tmux", "split-window", "-v", "-t", "=w:", "-c", "#{pane_current_path}"])
    );
    assert_eq!(kill_pane_argv(&k, LOCAL), v(&["tmux", "kill-pane", "-t", "=w:"]));
    assert_eq!(pane_count_argv(&k, LOCAL), v(&["tmux", "list-panes", "-s", "-t", "=w:", "-F", "#{pane_id}"]));
    assert_eq!(
        pane_command_argv(&k, LOCAL),
        v(&["tmux", "display-message", "-p", "-t", "=w:", "#{pane_current_command}"])
    );
}

#[test]
fn window_commands_quote_every_word_for_a_remote_host() {
    for name in HOSTILE {
        let k = key("ph", name);
        let target = format!("={name}:");
        let cases: [(Vec<String>, Vec<&str>); 6] = [
            (new_window_argv(&k, LOCAL), vec!["new-window", "-t", &target, "-c", "#{pane_current_path}"]),
            (split_argv(&k, Split::SideBySide, LOCAL), vec!["split-window", "-h", "-t", &target, "-c", "#{pane_current_path}"]),
            (split_argv(&k, Split::TopBottom, LOCAL), vec!["split-window", "-v", "-t", &target, "-c", "#{pane_current_path}"]),
            (kill_pane_argv(&k, LOCAL), vec!["kill-pane", "-t", &target]),
            (pane_count_argv(&k, LOCAL), vec!["list-panes", "-s", "-t", &target, "-F", "#{pane_id}"]),
            (pane_command_argv(&k, LOCAL), vec!["display-message", "-p", "-t", &target, "#{pane_current_command}"]),
        ];
        for (argv, want) in cases {
            assert_eq!(words(&remote(&argv, &SSH)), v(&want), "{name}");
        }
        for title in HOSTILE {
            assert_eq!(
                words(&remote(&rename_window_argv(&k, title, LOCAL), &SSH)),
                v(&["rename-window", "-t", &target, "--", title]),
                "{name} / {title}"
            );
        }
    }
}

#[test]
fn a_window_needs_a_name_tmux_keeps() {
    assert_eq!(valid_window_name("logs"), Ok(()));
    assert_eq!(valid_window_name("a b: c.d #1"), Ok(()), "a window may have what a session may not");
    assert_eq!(valid_window_name(""), Err("the window needs a name".to_string()));
    assert_eq!(valid_window_name("a\tb"), Err("names can't contain control characters".to_string()));
    assert_eq!(
        valid_window_name(&"x".repeat(MAX_NAME_CHARS + 1)),
        Err(format!("names can't be longer than {MAX_NAME_CHARS} characters"))
    );
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test tmux`
Expected: compile errors `cannot find function new_window_argv` (and the others).

- [ ] **Step 3: Implement in `tmux.rs`**, after `running_commands_argv`:

```rust
/// Which way `split_argv` splits the session's current pane (tmls `tmux_ops.split`: tmux's
/// `-h` puts the new pane beside it, `-v` under it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    SideBySide,
    TopBottom,
}

/// `tmux ARGS` here, or over ssh with every word single-quoted for the host's shell.
fn session_tmux(key: &SessionKey, args: &[&str], local_name: &str) -> Vec<String> {
    if key.host == local_name {
        let mut argv = strings(&["tmux"]);
        argv.extend(strings(args));
        argv
    } else {
        let words: Vec<String> = args.iter().map(|word| quote(word)).collect();
        ssh(&key.host, format!("tmux {}", words.join(" ")))
    }
}

/// A new window in the session, in the current pane's folder (tmls `new_window`).
pub fn new_window_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["new-window", "-t", &target, "-c", "#{pane_current_path}"], local_name)
}

/// Renames the session's current window. Check `new_name` with [`valid_window_name`] first.
pub fn rename_window_argv(key: &SessionKey, new_name: &str, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["rename-window", "-t", &target, "--", new_name], local_name)
}

/// Splits the session's current pane, the new one in the same folder (tmls `split`).
pub fn split_argv(key: &SessionKey, split: Split, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    let way = match split {
        Split::SideBySide => "-h",
        Split::TopBottom => "-v",
    };
    session_tmux(key, &["split-window", way, "-t", &target, "-c", "#{pane_current_path}"], local_name)
}

/// Ends the session's active pane; the last one ends the session (tmls `kill_pane`).
pub fn kill_pane_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["kill-pane", "-t", &target], local_name)
}

/// Prints one line per pane of the whole session: one line means the active pane is its last.
pub fn pane_count_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["list-panes", "-s", "-t", &target, "-F", "#{pane_id}"], local_name)
}

/// Prints what the session's active pane runs (tmls `pane_info`).
pub fn pane_command_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["display-message", "-p", "-t", &target, "#{pane_current_command}"], local_name)
}

/// Whether tmux can name a window `name`; the error is the reason, for the user. A window may
/// have `:` `.` `#` in its name; it needs one, without control characters.
pub fn valid_window_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the window needs a name".to_string());
    }
    if name.chars().any(char::is_control) {
        return Err("names can't contain control characters".to_string());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("names can't be longer than {MAX_NAME_CHARS} characters"));
    }
    Ok(())
}
```

- [ ] **Step 4: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass.

- [ ] **Step 5: Failing desktop tests.** In `crates/desktop/tests/forms.rs` add `Split, kill_pane_argv, new_window_argv, pane_command_argv, rename_window_argv, split_argv` to the `client_core::tmux::{…}` import and `PaneInfo` to the `desktop::forms::{…}` import. Replace `the_row_menu_offers_rename_kill_open_copy` with:

```rust
#[test]
fn the_row_menu_offers_rename_kill_open_copy_then_windows_and_panes() {
    assert_eq!(
        MENU,
        [
            MenuItem::Rename,
            MenuItem::Kill,
            MenuItem::OpenTerminal,
            MenuItem::CopyAttach,
            MenuItem::NewWindow,
            MenuItem::RenameWindow,
            MenuItem::SplitSideBySide,
            MenuItem::SplitTopBottom,
            MenuItem::KillPane,
        ]
    );
    let session = key("ph", "tmlsrs-test-menu");
    let mut menu = RowMenu::new(session.clone());
    let panel = menu.panel();
    let said = text(&panel);
    for label in [
        "Rename",
        "Kill…",
        "Open in new terminal",
        "Copy attach command",
        "New window",
        "Rename window…",
        "Split side by side",
        "Split top / bottom",
        "Kill pane…",
    ] {
        assert!(said.contains(label), "{label} in {said}");
    }
    // Enter picks the row the arrows are on; the first is Rename, never Kill.
    assert_eq!(
        menu.key(&named(Named::Enter)),
        MenuResult::Pick(MenuItem::Rename)
    );
    assert_eq!(menu.key(&named(Named::Down)), MenuResult::Pending);
    assert_eq!(menu.key(&named(Named::Down)), MenuResult::Pending);
    assert_eq!(
        menu.key(&named(Named::Enter)),
        MenuResult::Pick(MenuItem::OpenTerminal)
    );
    assert_eq!(menu.key(&named(Named::Up)), MenuResult::Pending);
    assert_eq!(
        menu.key(&named(Named::Enter)),
        MenuResult::Pick(MenuItem::Kill)
    );
    // Any other key closes it.
    assert_eq!(
        menu.key(&event(Key::Char('x'), Mods::default())),
        MenuResult::Closed
    );
    let (x, y) = at(&panel, "Copy attach command");
    assert!(menu.hover(x, y));
    assert_eq!(menu.click(x, y), MenuResult::Pick(MenuItem::CopyAttach));
    let (x, y) = at(&panel, "Split side by side");
    assert_eq!(menu.click(x, y), MenuResult::Pick(MenuItem::SplitSideBySide));
    // Kill pane is red, as Kill is.
    let kill_pane = menu
        .panel()
        .cells
        .into_iter()
        .find(|cell| cell.text == " Kill pane…")
        .expect("a Kill pane row");
    assert_eq!(
        kill_pane.look,
        Look::Item {
            hovered: false,
            danger: true
        }
    );
    assert_eq!(
        menu.click(-1.0, 0.5),
        MenuResult::Closed,
        "a click beside it closes"
    );
}

#[test]
fn rename_window_needs_a_name() {
    let k = key("ph", "w");
    let mut form = Form::rename_window(k.clone());
    assert_eq!(form.focus(), Stop::Name);
    assert_eq!(form.key(&named(Named::Enter)), FormResult::Pending);
    assert_eq!(form.error(), Some("the window needs a name"));
    type_text(&mut form, "logs");
    assert_eq!(form.error(), None, "typing clears it");
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Submit(Request::RenameWindow(k.clone(), "logs".to_string()))
    );
    let said = text(&form.panel());
    assert!(
        said.contains("ph · w") && said.contains("New name of the current window"),
        "{said}"
    );
    assert_eq!(
        Form::rename_window(k).key(&named(Named::Escape)),
        FormResult::Cancelled
    );
}

#[test]
fn the_kill_pane_confirm_waits_for_the_host_and_warns_on_the_last_pane() {
    let k = key("ph", "w");
    let mut form = Form::confirm_kill_pane(k.clone());
    assert_eq!(form.focus(), Stop::Cancel);
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Cancelled,
        "Enter on Cancel kills nothing"
    );
    let mut form = Form::confirm_kill_pane(k.clone());
    tab(&mut form, 1);
    assert_eq!(form.focus(), Stop::Ok);
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Pending,
        "not before the host has said"
    );
    assert!(text(&form.panel()).contains("asking the host"));
    form.set_pane_info(
        &key("ph", "other"),
        Ok(PaneInfo {
            command: None,
            last: true,
        }),
    );
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Pending,
        "an answer about another session is not this one's"
    );
    form.set_pane_info(
        &k,
        Ok(PaneInfo {
            command: Some("vim".to_string()),
            last: false,
        }),
    );
    let said = text(&form.panel());
    assert!(
        said.contains("Running: vim") && !said.contains("last pane"),
        "{said}"
    );
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Submit(Request::KillPane(k.clone(), false))
    );
    form.set_pane_info(
        &k,
        Ok(PaneInfo {
            command: None,
            last: true,
        }),
    );
    assert!(text(&form.panel()).contains(forms::LAST_PANE));
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Submit(Request::KillPane(k.clone(), true))
    );
    form.set_pane_info(&k, Err("couldn't ask ph about its pane: timed out".to_string()));
    assert!(text(&form.panel()).contains("timed out"));
    assert_eq!(form.key(&named(Named::Enter)), FormResult::Pending);
}

#[test]
fn pane_info_reads_the_command_and_the_count() {
    let k = key("ph", "w");
    let answer = |command: &'static str, panes: &'static str| {
        move |argv: &[String]| -> anyhow::Result<String> {
            let asks_command = argv == tmux::pane_command_argv(&key("ph", "w"), LOCAL).as_slice();
            Ok(if asks_command { command } else { panes }.to_string())
        }
    };
    assert_eq!(
        forms::pane_info(&k, LOCAL, &answer("vim\n", "%1\n%2\n")),
        Ok(PaneInfo {
            command: Some("vim".to_string()),
            last: false
        })
    );
    assert_eq!(
        forms::pane_info(&k, LOCAL, &answer("bash\n", "%1\n")),
        Ok(PaneInfo {
            command: None,
            last: true
        })
    );
    let down = |_: &[String]| -> anyhow::Result<String> {
        anyhow::bail!("ssh: connect to host ph: timed out")
    };
    assert_eq!(
        forms::pane_info(&k, LOCAL, &down),
        Err("couldn't ask ph about its pane: ssh: connect to host ph: timed out".to_string())
    );
}

#[test]
fn window_and_pane_requests_run_their_commands_and_say_what_happened() {
    let k = key("ph", "tmlsrs-test-a b'c");
    let cases = [
        (
            Request::NewWindow(k.clone()),
            new_window_argv(&k, LOCAL),
            "opened a new window in tmlsrs-test-a b'c on ph",
        ),
        (
            Request::RenameWindow(k.clone(), "logs".to_string()),
            rename_window_argv(&k, "logs", LOCAL),
            "renamed the window of tmlsrs-test-a b'c to logs on ph",
        ),
        (
            Request::Split(k.clone(), Split::SideBySide),
            split_argv(&k, Split::SideBySide, LOCAL),
            "split tmlsrs-test-a b'c side by side on ph",
        ),
        (
            Request::Split(k.clone(), Split::TopBottom),
            split_argv(&k, Split::TopBottom, LOCAL),
            "split tmlsrs-test-a b'c top / bottom on ph",
        ),
        (
            Request::KillPane(k.clone(), false),
            kill_pane_argv(&k, LOCAL),
            "killed the active pane of tmlsrs-test-a b'c on ph",
        ),
        (
            Request::KillPane(k.clone(), true),
            kill_pane_argv(&k, LOCAL),
            "killed the last pane of tmlsrs-test-a b'c on ph: the session ended",
        ),
    ];
    for (request, argv, said) in cases {
        assert_eq!(forms::request_argv(&request, LOCAL), argv);
        let ok = |_: &[String]| -> anyhow::Result<String> { Ok(String::new()) };
        assert_eq!(forms::perform(&request, LOCAL, &ok), Ok(said.to_string()));
        let failing =
            |_: &[String]| -> anyhow::Result<String> { anyhow::bail!("can't find window: 3\n") };
        let error = forms::perform(&request, LOCAL, &failing).unwrap_err();
        assert!(error.ends_with(": can't find window: 3"), "{error:?}");
    }
}
```

In `menu_items_offer_approve_only_while_claude_asks` (round 1 Task 5, extended by round 1 Task 7 with `Send`), replace the literal list it expects for a session that asks by a list built from `MENU`:

```rust
    let mut want = vec![MenuItem::Approve];
    want.extend(MENU);
    want.push(MenuItem::Send);
    assert_eq!(forms::menu_items(Some(&asks)), want);
```

- [ ] **Step 6: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test forms`
Expected: compile errors (`Form::rename_window`, `PaneInfo`, `Request::NewWindow`, `MenuItem::NewWindow`).

- [ ] **Step 7: Implement in `forms.rs`.** Imports: `use client_core::tmux::{self, NewSession, Split, Start};`. Module comment, menu paragraph: "Rename, Kill…, Open in new terminal, Copy attach command, then New window, Rename window…, Split side by side, Split top / bottom, Kill pane… (tmls `manage.py`'s Window and pane part)."

`Request` gains:

```rust
    /// A new window in the session, in the current pane's folder.
    NewWindow(SessionKey),
    /// The session's current window gets this name.
    RenameWindow(SessionKey, String),
    Split(SessionKey, Split),
    /// The session's active pane ends; `true`: it is the last one, and the session ends with it.
    KillPane(SessionKey, bool),
```

After `Look`'s definitions, the pane facts the kill-pane confirm waits for:

```rust
/// What the kill-pane confirm says: what runs in the active pane (shells are not worth saying)
/// and whether it is the session's last pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneInfo {
    pub command: Option<String>,
    pub last: bool,
}

/// What the kill-pane confirm says when the pane is the session's last (tmls `manage.py`).
pub const LAST_PANE: &str = "This is the last pane: the session ends.";
```

`Form` gains:

```rust
    /// A new name for the session's current window: an empty field (tmls "Window name").
    RenameWindow {
        key: SessionKey,
        field: TextField,
        focus: Stop,
        error: Option<String>,
    },
    /// The question before the active pane is killed; `pane` is `None` until the host has
    /// said what runs in it and whether it is the last.
    ConfirmKillPane {
        key: SessionKey,
        pane: Option<Result<PaneInfo, String>>,
        focus: Stop,
    },
```

In `impl Form`, after `confirm_kill`:

```rust
    /// The field for the current window's new name, empty.
    pub fn rename_window(key: SessionKey) -> Form {
        Form::RenameWindow {
            key,
            field: TextField::new(),
            focus: Stop::Name,
            error: None,
        }
    }

    /// The question before the active pane is killed. The focus is on Cancel; Kill pane does
    /// nothing until `set_pane_info` brought the host's answer.
    pub fn confirm_kill_pane(key: SessionKey) -> Form {
        Form::ConfirmKillPane {
            key,
            pane: None,
            focus: Stop::Cancel,
        }
    }

    /// The host's answer about `session`'s active pane, for the kill-pane confirm of that
    /// session.
    pub fn set_pane_info(&mut self, session: &SessionKey, info: Result<PaneInfo, String>) {
        if let Form::ConfirmKillPane { key, pane, .. } = self
            && key == session
        {
            *pane = Some(info);
        }
    }
```

The other methods (each arm added to the existing `match`, beside the arms round 1 added):
- `focus()`: add `| Form::RenameWindow { focus, .. } | Form::ConfirmKillPane { focus, .. }` to the arm that returns `*focus`.
- `error()`: `Form::RenameWindow { error, .. } => error.as_deref(),` and `Form::ConfirmKillPane { .. } => None,`.
- `stops()`: `Form::RenameWindow { .. } => &RENAME_STOPS,` and `Form::ConfirmKillPane { .. } => &KILL_STOPS,`.
- `focus_on()`:

```rust
            Form::RenameWindow { focus, field, .. } => {
                *focus = stop;
                if stop == Stop::Name {
                    field.select_all();
                }
            }
            Form::ConfirmKillPane { focus, .. } => *focus = stop,
```

- `submit()`:

```rust
            Form::RenameWindow {
                key, field, error, ..
            } => {
                let name = field.text().trim().to_string();
                if let Err(why) = tmux::valid_window_name(&name) {
                    *error = Some(why);
                    return FormResult::Pending;
                }
                FormResult::Submit(Request::RenameWindow(key.clone(), name))
            }
            Form::ConfirmKillPane {
                key,
                pane: Some(Ok(info)),
                ..
            } => FormResult::Submit(Request::KillPane(key.clone(), info.last)),
            Form::ConfirmKillPane { .. } => FormResult::Pending,
```

- `key()`: the Rename editing arm becomes an or-pattern, and the two new forms join the button arm:

```rust
            Form::Rename {
                field,
                focus: Stop::Name,
                error,
                ..
            }
            | Form::RenameWindow {
                field,
                focus: Stop::Name,
                error,
                ..
            } => {
                let before = field.text().to_string();
                if edit(field, ev) && field.text() != before {
                    *error = None;
                }
            }
```

and `Form::Rename { focus, .. } | Form::ConfirmKill { focus, .. }` becomes `Form::Rename { focus, .. } | Form::ConfirmKill { focus, .. } | Form::RenameWindow { focus, .. } | Form::ConfirmKillPane { focus, .. }`.
- `paste()`: the `Form::Rename { field, focus: Stop::Name, error, .. }` arm becomes the same or-pattern with `Form::RenameWindow { field, focus: Stop::Name, error, .. }`.
- `click()`: `(Target::Stop(Stop::Name), Form::Rename { field, focus, .. } | Form::RenameWindow { field, focus, .. }) => { *focus = Stop::Name; field.click(col); }` replaces the Rename-only arm.
- `panel()`:

```rust
            Form::RenameWindow {
                key,
                field,
                focus,
                error,
            } => {
                let mut p = Panel::new(ACTION_COLS);
                let title = format!("{} · {}", key.host, key.name);
                p.put(0, PAD, inner, &title, Look::Title);
                p.put(2, PAD, inner, "New name of the current window", Look::Label);
                p.field(3, PAD, inner, field, Stop::Name, *focus);
                if let Some(error) = error {
                    p.put(4, PAD, inner, error, Look::Error);
                }
                p.buttons(5, "Rename", false, *focus);
                p.lines = 7;
                p
            }
            Form::ConfirmKillPane { key, pane, focus } => {
                let mut p = Panel::new(ACTION_COLS);
                let title = format!("{} · {}", key.host, key.name);
                p.put(0, PAD, inner, &title, Look::Title);
                let question = format!("Kill the active pane of {}?", key.name);
                p.put(2, PAD, inner, &question, Look::Text);
                match pane {
                    None => {
                        p.put(3, PAD, inner, "asking the host what runs in it…", Look::Dim);
                    }
                    Some(Err(why)) => {
                        p.put(3, PAD, inner, why, Look::Error);
                    }
                    Some(Ok(info)) => {
                        if let Some(command) = &info.command {
                            p.put(3, PAD, inner, &format!("Running: {command}"), Look::Error);
                        }
                        if info.last {
                            p.put(4, PAD, inner, LAST_PANE, Look::Error);
                        }
                    }
                }
                p.buttons(6, "Kill pane", true, *focus);
                p.lines = 8;
                p
            }
```

Menu: `MenuItem` gains `NewWindow, RenameWindow, SplitSideBySide, SplitTopBottom, KillPane` with labels `"New window"`, `"Rename window…"`, `"Split side by side"`, `"Split top / bottom"`, `"Kill pane…"`; `MENU` becomes `[MenuItem; 9]` in the order of the test above; in `RowMenu::panel` `danger: *item == MenuItem::Kill` becomes `danger: matches!(*item, MenuItem::Kill | MenuItem::KillPane)`.

`request_argv` gains:

```rust
        Request::NewWindow(key) => tmux::new_window_argv(key, local_name),
        Request::RenameWindow(key, name) => tmux::rename_window_argv(key, name, local_name),
        Request::Split(key, split) => tmux::split_argv(key, *split, local_name),
        Request::KillPane(key, _) => tmux::kill_pane_argv(key, local_name),
```

`perform`'s `(done, failed)` match gains:

```rust
        Request::NewWindow(key) => (
            format!("opened a new window in {} on {}", key.name, key.host),
            format!("couldn't open a window in {} on {}", key.name, key.host),
        ),
        Request::RenameWindow(key, name) => (
            format!("renamed the window of {} to {name} on {}", key.name, key.host),
            format!("couldn't rename the window of {} on {}", key.name, key.host),
        ),
        Request::Split(key, split) => {
            let way = match split {
                Split::SideBySide => "side by side",
                Split::TopBottom => "top / bottom",
            };
            (
                format!("split {} {way} on {}", key.name, key.host),
                format!("couldn't split {} on {}", key.name, key.host),
            )
        }
        Request::KillPane(key, last) => (
            match last {
                true => format!("killed the last pane of {} on {}: the session ended", key.name, key.host),
                false => format!("killed the active pane of {} on {}", key.name, key.host),
            },
            format!("couldn't kill the pane of {} on {}", key.name, key.host),
        ),
```

After `running_commands`:

```rust
/// What runs in the session's active pane and whether it is the session's last (tmls
/// `pane_info`), for the kill-pane confirm; why not, when the host did not say.
pub fn pane_info(key: &SessionKey, local_name: &str, run: Runner) -> Result<PaneInfo, String> {
    let ask = |argv: Vec<String>| {
        run(&argv).map_err(|e| format!("couldn't ask {} about its pane: {}", key.host, first_line(&e)))
    };
    let command = ask(tmux::pane_command_argv(key, local_name))?;
    let panes = ask(tmux::pane_count_argv(key, local_name))?;
    let command = command.trim();
    Ok(PaneInfo {
        command: (!command.is_empty() && !SHELLS.contains(&command)).then(|| command.to_string()),
        last: panes.lines().filter(|line| !line.trim().is_empty()).count() <= 1,
    })
}
```

`keys.rs` `list_keeps_keys`: the second arm becomes `Request::Rename(..) | Request::Kill(_) | Request::NewWindow(_) | Request::RenameWindow(..) | Request::Split(..) | Request::KillPane(..) => keys_were == Focus::Sidebar,` (round 1's `Request::New(_) | Request::Answer { .. } => false` arm stays).

- [ ] **Step 8: Run the desktop unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test forms --test keys`
Expected: pass (if the test build needs the app to compile, after Step 9).

- [ ] **Step 9: Wire `app.rs`.** Imports: `use client_core::tmux::Split;`. `UserEvent` gains `/// What runs in a session's active pane, for the kill-pane confirm, or why not.\n PaneInfo(SessionKey, Result<forms::PaneInfo, String>),`; `user_event` routes it like `Running`:

```rust
            UserEvent::PaneInfo(key, info) => {
                if let Some((Modal::Form(form), _)) = &mut running.modal {
                    form.set_pane_info(&key, info);
                    running.invalidate();
                }
            }
```

New method after `lookup_name`:

```rust
    /// Runs a request's command on a thread of its own; the answer is `UserEvent::Done`.
    fn run_request(&mut self, request: Request) {
        let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
        std::thread::spawn(move || {
            let result = forms::perform(&request, &local, &forms::run_tmux);
            let _ = proxy.send_event(UserEvent::Done(request, result));
        });
    }
```

`menu_result` gains (before `MenuResult::Pick(MenuItem::OpenTerminal)`):

```rust
            MenuResult::Pick(MenuItem::NewWindow) => {
                self.close_modal();
                self.run_request(Request::NewWindow(key));
            }
            MenuResult::Pick(MenuItem::SplitSideBySide) => {
                self.close_modal();
                self.run_request(Request::Split(key, Split::SideBySide));
            }
            MenuResult::Pick(MenuItem::SplitTopBottom) => {
                self.close_modal();
                self.run_request(Request::Split(key, Split::TopBottom));
            }
            MenuResult::Pick(MenuItem::RenameWindow) => {
                self.modal = Some((Modal::Form(Form::rename_window(key)), None));
            }
            MenuResult::Pick(MenuItem::KillPane) => {
                // Asked at once; what runs in the pane, and whether it is the last, follow.
                self.modal = Some((Modal::Form(Form::confirm_kill_pane(key.clone())), None));
                let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
                std::thread::spawn(move || {
                    let info = forms::pane_info(&key, &local, &forms::run_tmux);
                    let _ = proxy.send_event(UserEvent::PaneInfo(key, info));
                });
            }
```

`done`'s `match request` gains:

```rust
            Request::KillPane(key, true) => {
                // The session ended with its last pane, as after a kill.
                if let Some(index) = self.tabs.index_of(&key) {
                    self.close_tab(index);
                }
            }
            Request::NewWindow(_) | Request::RenameWindow(..) | Request::Split(..) | Request::KillPane(_, false) => {}
```

(`form_result` needs nothing: `RenameWindow` and `KillPane` come as `FormResult::Submit` and run through `forms::perform` like the others.) Module comment, Request path: "(new, rename, kill, window and pane changes)".

- [ ] **Step 10: e2e block.** Top of `e2e.sh`, after round 1's variables: `WIN='tmlsrs-test-00win'`; `cleanup()`: `tmux kill-session -t "=$WIN" 2>/dev/null`. Before `# --- quit` (after round 1's blocks):

```bash
# --- the row menu's window and pane entries (G1) -------------------------------------------------
# $WIN sorts first ('00' < '0a'), so it is the list's first session. Its menu: Rename, Kill…,
# Open in new terminal, Copy attach command, New window, Rename window…, Split side by side,
# Split top / bottom, Kill pane….
panes_are() { [ "$(tmux list-panes -s -t "=$1" -F '#{pane_id}' 2>/dev/null | wc -l)" -eq "$2" ]; }
windows_are() { [ "$(tmux list-windows -t "=$1" -F '#{window_id}' 2>/dev/null | wc -l)" -eq "$2" ]; }
tmux new-session -d -s "$WIN" -x 100 -y 30
sleep 6
key_focus_list
top_of_list
keys -k m
check "screenshot 41-window-menu" shot 41-window-menu
keys -k j -k j -k j -k j -k j -k j
key_enter
if wait_for 8 panes_are "$WIN" 2; then
  pass "Split side by side from the menu: the session has two panes"
else
  fail "Split side by side from the menu: the session has two panes" "panes: $(tmux list-panes -s -t "=$WIN" 2>&1 | tr '\n' '|')"
fi
keys -k m
keys -k j -k j -k j -k j
key_enter
if wait_for 8 windows_are "$WIN" 2; then
  pass "New window from the menu: the session has two windows"
else
  fail "New window from the menu: the session has two windows" "windows: $(tmux list-windows -t "=$WIN" 2>&1 | tr '\n' '|')"
fi
# Kill pane… on the new window's only pane: the confirm, Tab to Kill pane, Enter.
keys -k m
keys -k j -k j -k j -k j -k j -k j -k j -k j
key_enter
sleep 1
check "screenshot 42-kill-pane (the question, what runs)" shot 42-kill-pane
keys -k Tab
key_enter
if wait_for 8 windows_are "$WIN" 1 && panes_are "$WIN" 2; then
  pass "Kill pane… then Kill pane ends the active pane only (one window, two panes left)"
else
  fail "Kill pane… then Kill pane ends the active pane only" "windows: $(tmux list-windows -t "=$WIN" 2>&1 | wc -l), panes: $(tmux list-panes -s -t "=$WIN" 2>&1 | wc -l)"
fi
tmux kill-session -t "=$WIN"
```

- [ ] **Step 11: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; `source spikes/s1/build-env.sh && cargo test -p client-core`; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/t1-e2e tooling/smoke/run.sh`
Expected: all pass, `== 0 of N checks failed`. Look at `/data/scratch/tmls-rs-v0/round2/t1-e2e/41-window-menu.png` (nine entries, Kill… and Kill pane… red) and `/data/scratch/tmls-rs-v0/round2/t1-e2e/42-kill-pane.png`.

- [ ] **Step 12: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): new window, rename window, split and kill pane from a session's menu"
```

---

### Task 2: G2 — game mode slows the remote hosts

**Files:**
- Create: `crates/client-core/src/gamemode.rs`, `crates/client-core/tests/gamemode.rs`
- Modify: `crates/client-core/src/lib.rs` (`pub mod gamemode;`)
- Modify: `crates/client-core/src/poll.rs:9-15` (imports), `:47-51` (`State`), `:82` (`start_with`'s `State`), `:98-102` (add `slow_down` after `refresh_now` / round 1's `force_next`), `:116-152` (`poll_host`'s wait)
- Modify: `crates/desktop/src/draw.rs` (after round 1's `header_spans_with`: `HeaderLook`, `GAME_MODE`, `header_spans_look`; `View`, `:811-830`; the header call in `sidebar()`, `:1284-1362`)
- Modify: `crates/desktop/src/app.rs` (constants `:100-111`, `UserEvent`, `user_event`, `Running` `:336-438`, `Running::start` `:541-640`, `snapshot` `:776-815`, `build_view` `:846-906`; new `tick`, `game_mode`)
- Modify: `tooling/smoke/e2e.sh` (`cleanup`, the third run after round 1's second run), `tooling/smoke/run.sh` (`SMOKE_TIMEOUT` default)
- Test: `crates/client-core/tests/gamemode.rs`, `crates/client-core/tests/poll.rs`, `crates/desktop/tests/sidebar_draw.rs`

**Interfaces:**
- Consumes: `Poller::force_next`, `State.resend` (from round 1 Task 6); `draw::header_spans_with`, `POLL_ONLY`, `View.polled` (from round 1 Task 6); `forms::on_path` (exists).
- Produces:
  - `client_core::gamemode::{PROGRAM: &str = "game-mode", REMOTE_PERIOD: Duration = 300 s, TIMEOUT: Duration = 2 s, status_argv() -> Vec<String>, parse(out: &str) -> bool}`
  - `Poller::slow_down(&self, hosts: &[String], period: Option<Duration>)`
  - `draw::HeaderLook { pub polled: bool, pub game: bool }` (`Clone, Copy, Debug, Default, PartialEq, Eq`; Task 3 adds `sign_in`), `draw::GAME_MODE: &str = "· game"`, `draw::header_spans_look(host: &str, online: bool, look: HeaderLook, cols: usize, theme: &Theme) -> Vec<Span<'static>>`; `View.game_hosts: Vec<String>`
  - app: `const TICK: Duration = 30 s`; `UserEvent::Tick`, `UserEvent::GameMode(bool)`; `Running.game: bool`, `Running.game_probe: bool`; `Running::tick(&mut self)`, `Running::game_mode(&mut self, on: bool)`
  - e2e: the third app run (`THIRD_TMUX`, `$OUT/bin3` with `ssh`, `game-mode`, `fake-term`; `SSH_LOG`, `TERM_LOG`) and its anchor line `# --- third run: more checks above this line ---` (Tasks 3 and 6 add checks above it)
- Departs from the spec: (a) "a missing `game-mode` is a status-line sentence once" — for game mode it is one stderr line at start (`tmls-rs: no game-mode on PATH: game mode stays off`): the user asked for nothing, and every machine but the PC would show that sentence at every start. (b) `· game` is drawn on the remote hosts' headers (those R1 slows), not on this machine's, and only while they answer (an offline header already says why). (c) "no notify-send sounds": while game mode is on no notification command runs at all (`notify-send`, and from Task 4 the sound); the alerts still go into the bell's list. (d) The 30 s check is a ticker thread (`UserEvent::Tick`) that Task 6 reuses for the background servers; the first tick is at start.

- [ ] **Step 1: Failing client-core tests.** `crates/client-core/tests/gamemode.rs`:

```rust
//! `game-mode status` (the PC's script): what is asked and what counts as on.

use std::time::Duration;

use client_core::gamemode::{PROGRAM, REMOTE_PERIOD, TIMEOUT, parse, status_argv};

#[test]
fn the_status_is_asked_and_only_on_is_on() {
    assert_eq!(PROGRAM, "game-mode");
    assert_eq!(status_argv(), ["game-mode", "status"]);
    for on in ["on", "on\n", " ON \n", "On"] {
        assert!(parse(on), "{on:?}");
    }
    for off in ["off\n", "", "\n", "on and off", "unknown", "1"] {
        assert!(!parse(off), "{off:?}");
    }
    assert_eq!(REMOTE_PERIOD, Duration::from_secs(300));
    assert_eq!(TIMEOUT, Duration::from_secs(2));
}
```

Append to `crates/client-core/tests/poll.rs`:

```rust
#[test]
fn slow_down_gives_those_hosts_their_own_period_and_back_at_once() {
    let _one = alone();
    let d = dir("slow");
    let count = {
        let d = d.clone();
        move |host: &str| std::fs::read_to_string(d.join(host)).map_or(0, |text| text.lines().count())
    };
    let logs = d.clone();
    let (poller, _rx) = start(&["here", "far"], Duration::from_millis(100), Duration::from_secs(5), move |host| {
        sh(format!("echo x >>'{}'; cat >/dev/null; echo 100", logs.join(host).display()))
    });
    std::thread::sleep(Duration::from_millis(350));
    poller.slow_down(&hosts(&["far"]), Some(Duration::from_secs(60)));
    let (here, far) = (count("here"), count("far"));
    std::thread::sleep(Duration::from_millis(700));
    assert!(count("here") >= here + 3, "the other host keeps its period: {here} -> {}", count("here"));
    // A listing that had started already may end; no other one starts within the minute.
    assert!(count("far") <= far + 1, "far waits its minute: {far} -> {}", count("far"));
    let far = count("far");
    poller.slow_down(&hosts(&["far"]), None);
    std::thread::sleep(Duration::from_millis(450));
    assert!(count("far") >= far + 2, "back to the poller's period at once: {far} -> {}", count("far"));
    drop(poller);
    assert_no_poll_threads();
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test gamemode --test poll`
Expected: compile errors (module `gamemode`, `slow_down`).

- [ ] **Step 3: Implement.** `crates/client-core/src/gamemode.rs`:

```rust
//! Game mode (the PC's `game-mode` script, spec G2): while `game-mode status` says `on`,
//! tmls-rs lists the remote hosts every 300 s instead of every 5 s (R1: their ssh is what
//! wakes the network) and runs no notification command. Asked every 30 s, on this machine
//! only; a machine without the script has no game mode.

use std::time::Duration;

/// The script's name on PATH.
pub const PROGRAM: &str = "game-mode";
/// The listing period of a remote host while game mode is on.
pub const REMOTE_PERIOD: Duration = Duration::from_secs(300);
/// How long `game-mode status` may take before it counts as off.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// `game-mode status`.
pub fn status_argv() -> Vec<String> {
    vec![PROGRAM.to_string(), "status".to_string()]
}

/// Whether what `game-mode status` printed (exit status 0) means on: `on`, nothing else.
pub fn parse(out: &str) -> bool {
    out.trim().eq_ignore_ascii_case("on")
}
```

`lib.rs`: `pub mod gamemode;` and a line in the crate comment ("game mode's status").

`poll.rs`: imports gain `use std::collections::HashMap;`. `State` gains:

```rust
    /// Hosts listed at a period of their own (`slow_down`), instead of the poller's.
    slow: HashMap<String, Duration>,
```

initialised `slow: HashMap::new()` in `start_with`. After `force_next` (round 1 Task 6):

```rust
    /// Lists `hosts` every `period` from now on instead of the poller's own period; `None`
    /// gives them their own back. Takes effect at once both ways: a thread waiting out the old
    /// period wakes and waits for the new one, counted from its last listing.
    pub fn slow_down(&self, hosts: &[String], period: Option<Duration>) {
        let mut state = self.shared.state();
        for host in hosts {
            match period {
                Some(period) => {
                    state.slow.insert(host.clone(), period);
                }
                None => {
                    state.slow.remove(host);
                }
            }
        }
        drop(state);
        self.shared.wake.notify_all();
    }
```

In `poll_host` the wait after a listing (from `let wake_at = Instant::now() + period;` to the end of the inner loop) becomes:

```rust
        // The period is read again at every wake: `slow_down` takes effect at once.
        let listed_at = Instant::now();
        let mut state = shared.state();
        loop {
            if state.stop {
                return;
            }
            let period = state.slow.get(host).copied().unwrap_or(period);
            let left = (listed_at + period).saturating_duration_since(Instant::now());
            if state.refresh != refresh || left.is_zero() {
                break;
            }
            state = shared.wake.wait_timeout(state, left).unwrap_or_else(PoisonError::into_inner).0;
        }
```

(`refresh` is the value round 1 Task 6 reads together with `resend` at the loop's head; nothing else in `poll_host` changes.)

- [ ] **Step 4: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass.

- [ ] **Step 5: Failing desktop test.** Append to `crates/desktop/tests/sidebar_draw.rs` (import `GAME_MODE, HeaderLook, POLL_ONLY, header_spans_look` from `desktop::draw`):

```rust
#[test]
fn a_host_header_says_poll_and_game_dimly_after_its_name() {
    let theme = Theme::default();
    let both = HeaderLook {
        polled: true,
        game: true,
    };
    let said = header_spans_look("nas", true, both, SIDEBAR_COLS, &theme);
    let (poll, game) = (find(&said, POLL_ONLY), find(&said, GAME_MODE));
    assert_eq!((poll.col, game.col), (5, 12));
    assert!(poll.colour == theme.dim_fg && game.colour == theme.dim_fg);
    assert!(said.iter().any(|span| span.text == "+"), "the + stays");
    assert_within(&said);
    // Game mode alone sits where `· poll` would.
    let game_only = HeaderLook {
        game: true,
        ..HeaderLook::default()
    };
    let said = header_spans_look("nas", true, game_only, SIDEBAR_COLS, &theme);
    assert_eq!(find(&said, GAME_MODE).col, 5);
    // An offline host says offline, not game.
    let said = header_spans_look("nas", false, game_only, SIDEBAR_COLS, &theme);
    assert!(said.iter().all(|span| span.text != GAME_MODE));
    assert_eq!(said, header_spans("nas", false, SIDEBAR_COLS, &theme));
    // A name that fills the row leaves no room: the tags go, the + stays.
    let long = header_spans_look(&"h".repeat(40), true, both, SIDEBAR_COLS, &theme);
    assert!(long.iter().all(|span| span.text != GAME_MODE && span.text != POLL_ONLY));
    assert!(long.iter().any(|span| span.text == "+"));
    assert_within(&long);
}
```

- [ ] **Step 6: Run it.**
Run: `tooling/docker/build.sh test -p desktop --test sidebar_draw`
Expected: compile errors (`HeaderLook`, `header_spans_look`, `GAME_MODE`).

- [ ] **Step 7: Implement in `draw.rs`.** After round 1's `header_spans_with`, and make `header_spans_with` call the new function:

```rust
/// After the name of a remote host while game mode lists it every 300 s.
pub const GAME_MODE: &str = "· game";

/// What a host's header says after its name, besides `+` or `· offline`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeaderLook {
    /// Its events.jsonl is not there: `· poll`.
    pub polled: bool,
    /// Game mode slows its listing: `· game`.
    pub game: bool,
}

/// `header_spans`, with the tags of `look` dimly after the name while the host answers, each
/// only if it fits before the `+`.
pub fn header_spans_look(
    host: &str,
    online: bool,
    look: HeaderLook,
    cols: usize,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans = header_spans(host, online, cols, theme);
    if !online {
        return spans;
    }
    let end = cols.saturating_sub(1);
    let room = end.saturating_sub(1 + 2); // the name's columns in `header_spans`
    let mut col = 1 + columns(&host.to_uppercase()).min(room) + 1;
    for (on, tag) in [(look.polled, POLL_ONLY), (look.game, GAME_MODE)] {
        if on && col + columns(tag) < end.saturating_sub(1) {
            spans.push(Span::whole(0, col, tag).colour(theme.dim_fg));
            col += columns(tag) + 1;
        }
    }
    spans
}
```

`header_spans_with`'s body becomes `header_spans_look(host, online, HeaderLook { polled, ..HeaderLook::default() }, cols, theme)` (its round-1 test still holds). `View` gains `/// Remote hosts game mode slows (\`· game\`).\n pub game_hosts: Vec<String>,`. In `sidebar()` the header call becomes:

```rust
                let look = HeaderLook {
                    polled: view.polled.contains(host),
                    game: view.game_hosts.contains(host),
                };
                for span in header_spans_look(host, *online, look, l.list_cols, theme) {
                    p.span((r.x, r.y), &span);
                }
```

- [ ] **Step 8: Run the desktop test.**
Run: `tooling/docker/build.sh test -p desktop --test sidebar_draw`
Expected: pass.

- [ ] **Step 9: Wire `app.rs`.** Imports: `use client_core::gamemode;`. After `LOOKUP_DELAY`:

```rust
/// How often game mode is asked about, and (Task 6) the hosts' other tmux servers looked for:
/// six listings of 5 s.
const TICK: Duration = Duration::from_secs(30);
```

`UserEvent` gains `/// The 30 s ticker: game mode and the other tmux servers are asked about again.\n Tick,` and `/// What \`game-mode status\` said.\n GameMode(bool),`; `user_event`: `UserEvent::Tick => running.tick(),` and `UserEvent::GameMode(on) => running.game_mode(on),`. `Running` gains:

```rust
    /// `game-mode status` said on: remote hosts are listed every 300 s, no notification runs.
    game: bool,
    /// `game-mode` is on PATH; without it there is no game mode.
    game_probe: bool,
```

In `Running::start`, after the `Poller::start(…)` statement:

```rust
        let game_probe = forms::on_path(gamemode::PROGRAM);
        if !game_probe {
            crate::errln(format_args!("tmls-rs: no game-mode on PATH: game mode stays off"));
        }
        // The ticker: one event now, then one every 30 s, until the event loop is gone.
        let tick_proxy = proxy.clone();
        std::thread::Builder::new()
            .name("tmls-tick".to_string())
            .spawn(move || {
                while tick_proxy.send_event(UserEvent::Tick).is_ok() {
                    std::thread::sleep(TICK);
                }
            })
            .context("the ticker thread")?;
```

and in the `Running { … }` literal `game: false, game_probe,`. New methods:

```rust
    /// Every 30 s: game mode is asked about on a thread (2 s at most; no answer is off).
    fn tick(&mut self) {
        if self.game_probe {
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                let on = client_core::tmux::run(&gamemode::status_argv(), gamemode::TIMEOUT)
                    .is_ok_and(|out| gamemode::parse(&out));
                let _ = proxy.send_event(UserEvent::GameMode(on));
            });
        }
    }

    /// Game mode went on or off: the remote hosts' period follows (R1), this machine keeps 5 s.
    fn game_mode(&mut self, on: bool) {
        if on == self.game {
            return;
        }
        self.game = on;
        let remote: Vec<String> = self
            .cfg
            .hosts
            .iter()
            .filter(|host| **host != self.cfg.local_name)
            .cloned()
            .collect();
        self.poller
            .slow_down(&remote, on.then_some(gamemode::REMOTE_PERIOD));
        crate::errln(format_args!(
            "tmls-rs: game mode {}",
            if on { "on: remote hosts listed every 300 s" } else { "off" }
        ));
        self.invalidate();
    }
```

In `snapshot`, `alerts::send(&notes, &mut forms::spawn_detached);` becomes:

```rust
        // In game mode no notification runs; the bell's list has the alerts all the same.
        if !self.game {
            alerts::send(&notes, &mut forms::spawn_detached);
        }
```

`build_view` gains:

```rust
            game_hosts: match self.game {
                true => self
                    .cfg
                    .hosts
                    .iter()
                    .filter(|host| **host != self.cfg.local_name)
                    .cloned()
                    .collect(),
                false => Vec::new(),
            },
```

Module comment: a paragraph "Ticker: a thread sends `UserEvent::Tick` at start and every 30 s; `tick` asks `game-mode status` on a thread → `UserEvent::GameMode` → `Poller::slow_down` for the remote hosts."

- [ ] **Step 10: e2e — the third run.** `run.sh`: `SMOKE_TIMEOUT` default `900` → `1200` (code and comment: the third run and the round-2 blocks add about two minutes). `e2e.sh` `cleanup()`: nothing new yet. After round 1 Task 6's second run (after its `no panic on the second run's stderr` if/else) and before `echo "--- files under the scratch HOME"`:

```bash
# --- a third run: a remote host, game mode (G2), sign-in (G6), other tmux servers (G5) ----------
# Its own tmux socket directory (no default server: this machine lists no session) and, first on
# PATH, stand-ins for ssh (every call logged, every sign-in refused), game-mode (says on) and the
# terminal (logs its arguments). TMLS_RS_FOREGROUND: without --bench the app would detach.
THIRD_TMUX="$SCRATCH/tmux3"
mkdir -p "$THIRD_TMUX" "$OUT/bin3" && chmod 700 "$THIRD_TMUX"
SSH_LOG="$OUT/ssh3.log"
TERM_LOG="$OUT/term3.log"
: >"$SSH_LOG"
: >"$TERM_LOG"
cat >"$OUT/bin3/ssh" <<EOF
#!/bin/sh
echo "\$*" >>"$SSH_LOG"
echo 'tmlsrs@signhost: Permission denied (publickey,keyboard-interactive).' >&2
exit 255
EOF
cat >"$OUT/bin3/game-mode" <<'EOF'
#!/bin/sh
[ "$1" = status ] && echo on
EOF
cat >"$OUT/bin3/fake-term" <<EOF
#!/bin/sh
{ printf '%s|' "\$@"; echo; } >>"$TERM_LOG"
EOF
chmod +x "$OUT/bin3/ssh" "$OUT/bin3/game-mode" "$OUT/bin3/fake-term"
ssh_calls() { wc -l <"$SSH_LOG"; }
PATH="$OUT/bin3:$PATH" TMUX_TMPDIR="$THIRD_TMUX" TERMINAL="$OUT/bin3/fake-term" TMLS_RS_FOREGROUND=1 \
  "$BIN" signhost 2>"$OUT/app3.stderr" &
APP_PID=$!
if wait_for 30 window_up; then
  pass "the third run's window opened (signhost, game mode on)"
else
  fail "the third run's window opened (signhost, game mode on)" "stderr: $(tail -3 "$OUT/app3.stderr" | tr '\n' ' ')"
fi
if wait_for 10 grep -qF 'tmls-rs: game mode on' "$OUT/app3.stderr"; then
  pass "game-mode status says on: the app is in game mode"
else
  fail "game-mode status says on: the app is in game mode" "stderr: $(tail -3 "$OUT/app3.stderr" | tr '\n' ' ')"
fi
sleep 2
CALLS=$(ssh_calls)
sleep 12
if [ "$(ssh_calls)" -eq "$CALLS" ]; then
  pass "in game mode the remote host is not listed again within 12 s (without it: every 5 s)"
else
  fail "in game mode the remote host is not listed again within 12 s" "ssh calls $CALLS -> $(ssh_calls): $(tail -3 "$SSH_LOG" | tr '\n' '|')"
fi
check "screenshot 43-third-run (this machine, signhost)" shot 43-third-run
# --- third run: more checks above this line ---
key_quit
wait_for 10 app_gone || kill "$APP_PID" 2>/dev/null
if grep -nE "panicked|stack backtrace" "$OUT/app3.stderr" >/dev/null; then
  fail "no panic on the third run's stderr" "$(grep -n 'panicked' -A3 "$OUT/app3.stderr" | head -6 | tr '\n' ' ')"
else
  pass "no panic on the third run's stderr"
fi
```

- [ ] **Step 11: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; `source spikes/s1/build-env.sh && cargo test -p client-core`; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/t2-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/round2/t2-e2e/43-third-run.png`: this machine's header, a gap, `SIGNHOST · offline`.

- [ ] **Step 12: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh tooling/smoke/run.sh
git commit -m "feat(desktop): in game mode remote hosts are listed every 5 minutes and nothing notifies"
```

---

### Task 3: G6 — the sign-in row

**Files:**
- Modify: `crates/client-core/src/listing.rs:125-133` (`impl HostSnapshot` after the struct)
- Modify: `crates/client-core/src/tmux.rs` (add `sign_in_argv` after `attach_text`, `:174-183`)
- Modify: `crates/desktop/src/forms.rs` (add `sign_in` after `open_terminal`, `:1043-1057`)
- Modify: `crates/desktop/src/draw.rs` (`HeaderLook.sign_in`, `SIGN_IN`, `header_spans_look`'s offline branch, `View.sign_in`, the header call)
- Modify: `crates/desktop/src/app.rs` (`Running.sign_in`, `snapshot`, `build_view`, `list_click` `:1767-1790`; new `sign_in_to`)
- Modify: `tooling/smoke/e2e.sh` (the third run, above its anchor)
- Test: `crates/client-core/tests/poll.rs`, `crates/client-core/tests/tmux.rs`, `crates/desktop/tests/forms.rs`, `crates/desktop/tests/sidebar_draw.rs`

**Interfaces:**
- Consumes: `poll::SIGN_IN_REQUIRED` (exists, `poll.rs:22`); `HeaderLook`, `header_spans_look`, the third run (Task 2); `forms::terminal_argv`, `spawn_detached`, `on_path` (exist).
- Produces: `HostSnapshot::needs_sign_in(&self) -> bool`; `tmux::sign_in_argv(host: &str) -> Vec<String>`; `forms::sign_in(host: &str, terminal: Option<&str>, on_path: &dyn Fn(&str) -> bool, spawn: &mut dyn FnMut(&[String]) -> std::io::Result<()>) -> Result<String, String>`; `HeaderLook.sign_in: bool`, `draw::SIGN_IN: &str = "⚠ sign in"`; `View.sign_in: Vec<String>`; app `Running.sign_in: BTreeSet<String>`, `Running::sign_in_to(&mut self, host: &str)`.
- Departs from the spec: (a) `HostSnapshot.sign_in: bool` is a method, `needs_sign_in()`, reading what the poller already writes (`error == SIGN_IN_REQUIRED`, `poll.rs:22,227-228`): a new field would have to be added to every `HostSnapshot` literal in both crates for no new information. (b) "Enter / click on it": click only. The list's cursor never stands on a header (`sidebar.rs:15-19`: "never to a header or a gap"), and an offline host has no session row; a keyboard way would need a new cursor rule — left for a later round. (c) The terminal runs `ssh -t HOST true` (tmls `SignInRow`): the user answers the prompts, the shared connection (round 1's ControlMaster) stays up, and the next listing (5 s, or 300 s in game mode) clears the row.

- [ ] **Step 1: Failing client-core tests.** Append to `crates/client-core/tests/poll.rs`:

```rust
#[test]
fn a_host_that_wants_a_sign_in_says_so() {
    let refused = "cat >/dev/null; echo 'u@ph: Permission denied (publickey).' >&2; exit 255";
    assert!(list_once("ph", sh(refused), Duration::from_secs(5)).needs_sign_in());
    let unreachable = "cat >/dev/null; echo 'ssh: connect to host ph port 22: No route to host' >&2; exit 255";
    assert!(!list_once("ph", sh(unreachable), Duration::from_secs(5)).needs_sign_in());
    let up = list_once("ph", sh("cat >/dev/null; echo 100"), Duration::from_secs(5));
    assert!(up.online && !up.needs_sign_in());
}
```

Append to `crates/client-core/tests/tmux.rs` (import `sign_in_argv`):

```rust
#[test]
fn sign_in_is_an_ssh_that_may_ask_and_then_ends() {
    let mut want = v(&ATTACH);
    want.push("true".to_string());
    assert_eq!(sign_in_argv("ph"), want);
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test poll --test tmux`
Expected: compile errors (`needs_sign_in`, `sign_in_argv`).

- [ ] **Step 3: Implement.** `listing.rs`, after `HostSnapshot`:

```rust
impl HostSnapshot {
    /// ssh wants a password, a key or a host-key answer (`poll::SIGN_IN_REQUIRED`): the user
    /// has to sign in by hand (spec G6).
    pub fn needs_sign_in(&self) -> bool {
        !self.online && self.error.as_deref() == Some(crate::poll::SIGN_IN_REQUIRED)
    }
}
```

`tmux.rs`, after `attach_text`:

```rust
/// What the sign-in row runs in a terminal (tmls `SignInRow`): ssh with a terminal and without
/// `BatchMode`, so it may ask for a password, a code or a host key, then `true`. Once in, the
/// shared connection stays up and the next listing finds the host.
pub fn sign_in_argv(host: &str) -> Vec<String> {
    let mut argv = ssh_base(host, &["-t"]);
    argv.push("true".to_string());
    argv
}
```

- [ ] **Step 4: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass.

- [ ] **Step 5: Failing desktop tests.** Append to `crates/desktop/tests/forms.rs`:

```rust
#[test]
fn sign_in_opens_the_users_terminal_with_ssh() {
    let mut started = Vec::new();
    let mut spawn = |argv: &[String]| -> std::io::Result<()> {
        started.push(argv.to_vec());
        Ok(())
    };
    let said = forms::sign_in("ph", Some("foot"), &|_| false, &mut spawn);
    assert_eq!(said, Ok("sign in to ph in foot; the list follows by itself".to_string()));
    let mut want = vec!["foot".to_string(), "-e".to_string()];
    want.extend(tmux::sign_in_argv("ph"));
    assert_eq!(started, [want]);
    let mut broken = |_: &[String]| -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    };
    let said = forms::sign_in("ph", Some("foot"), &|_| false, &mut broken);
    assert!(said.unwrap_err().starts_with("couldn't start foot: "));
}
```

Append to `crates/desktop/tests/sidebar_draw.rs` (import `SIGN_IN` from `desktop::draw`):

```rust
#[test]
fn a_host_that_wants_a_sign_in_says_so_instead_of_offline() {
    let theme = Theme::default();
    let look = HeaderLook {
        sign_in: true,
        ..HeaderLook::default()
    };
    let said = header_spans_look("nas", false, look, SIDEBAR_COLS, &theme);
    let sign = find(&said, SIGN_IN);
    assert_eq!((sign.col, sign.colour), (5, mark_rgb(Status::Waiting)));
    assert!(said.iter().all(|span| span.text != "· offline" && span.text != "+"));
    assert_within(&said);
    let long = header_spans_look(&"h".repeat(40), false, look, SIDEBAR_COLS, &theme);
    assert_within(&long);
    assert!(long.iter().any(|span| span.text == SIGN_IN));
    // A host that answers needs no sign-in, whatever the look says.
    let online = header_spans_look("nas", true, look, SIDEBAR_COLS, &theme);
    assert!(online.iter().all(|span| span.text != SIGN_IN));
}
```

In `a_host_header_says_poll_and_game_dimly_after_its_name` (Task 2) the `both` literal gains `..HeaderLook::default()`.

- [ ] **Step 6: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test forms --test sidebar_draw`
Expected: compile errors (`forms::sign_in`, `HeaderLook.sign_in`, `SIGN_IN`).

- [ ] **Step 7: Implement.** `forms.rs`, after `open_terminal`:

```rust
/// Opens a terminal running ssh to `host`, so the user can answer what it asks (R5), through
/// `spawn`; says so for the status line.
pub fn sign_in(
    host: &str,
    terminal: Option<&str>,
    on_path: &dyn Fn(&str) -> bool,
    spawn: &mut dyn FnMut(&[String]) -> std::io::Result<()>,
) -> Result<String, String> {
    let argv = terminal_argv(terminal, on_path, &tmux::sign_in_argv(host));
    match spawn(&argv) {
        Ok(()) => Ok(format!("sign in to {host} in {}; the list follows by itself", argv[0])),
        Err(e) => Err(format!("couldn't start {}: {e}", argv[0])),
    }
}
```

`draw.rs`: `HeaderLook` gains `/// ssh wants a sign-in: \`⚠ sign in\` instead of \`· offline\`.\n pub sign_in: bool,`; after `GAME_MODE`:

```rust
/// Instead of `· offline` on a host whose ssh wants a sign-in; a click on the header signs in.
pub const SIGN_IN: &str = "⚠ sign in";
```

`header_spans_look`'s `if !online { return spans; }` becomes:

```rust
    if !online {
        // `· offline` is the last span of an offline header; the sign-in takes its place.
        if look.sign_in
            && let Some(last) = spans.last_mut()
        {
            *last = Span::whole(0, last.col, SIGN_IN).colour(mark_rgb(client_core::Status::Waiting));
        }
        return spans;
    }
```

(import `mark_rgb` from `crate::theme` if `draw.rs` does not yet). `View` gains `/// Hosts whose ssh wants a sign-in (\`⚠ sign in\`).\n pub sign_in: Vec<String>,`; the header call's `HeaderLook` gains `sign_in: view.sign_in.contains(host),`.

- [ ] **Step 8: Run the desktop unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test forms --test sidebar_draw`
Expected: pass.

- [ ] **Step 9: Wire `app.rs`.** Imports: `use std::collections::BTreeSet;`. `Running` gains `/// Hosts whose last listing said ssh wants a sign-in.\n sign_in: BTreeSet<String>,` (`BTreeSet::new()`). First lines of `snapshot`:

```rust
        if snapshot.needs_sign_in() {
            self.sign_in.insert(snapshot.host.clone());
        } else {
            self.sign_in.remove(&snapshot.host);
        }
```

New method:

```rust
    /// A click on `⚠ sign in`: a terminal with ssh to the host (R5); the next listing clears the
    /// row once the user is in.
    fn sign_in_to(&mut self, host: &str) {
        let terminal = std::env::var("TERMINAL").ok();
        let said = forms::sign_in(
            host,
            terminal.as_deref(),
            &forms::on_path,
            &mut forms::spawn_detached,
        );
        self.say(said);
    }
```

`list_click`, right after `self.select_when_listed = None;`:

```rust
        if button == MouseButton::Left
            && let sidebar::Row::Host {
                host,
                online: false,
                ..
            } = &self.sidebar.rows()[row]
            && self.sign_in.contains(host)
        {
            let host = host.clone();
            self.sign_in_to(&host);
            self.invalidate();
            return;
        }
```

`build_view`: `sign_in: self.sign_in.iter().cloned().collect(),`.

- [ ] **Step 10: e2e, in the third run** (Task 2), right above `# --- third run: more checks above this line ---`:

```bash
# The list: line 0 this machine's header, line 1 the gap, line 2 SIGNHOST · ⚠ sign in (y 65).
if click 100 65 && wait_for 5 grep -qF -- "|-t|signhost|true|" "$TERM_LOG"; then
  pass "a click on ⚠ sign in opens a terminal running ssh -t signhost true"
else
  fail "a click on ⚠ sign in opens a terminal running ssh -t signhost true" "terminal log: $(tr '\n' ' ' <"$TERM_LOG")"
fi
check "the sign-in terminal is given ssh after -e" grep -qF -- "-e|ssh|" "$TERM_LOG"
```

(Rename the third run's screenshot check to `"screenshot 43-third-run (SIGNHOST ⚠ sign in)"`.)

- [ ] **Step 11: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; client-core tests; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/t3-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/round2/t3-e2e/43-third-run.png`: `SIGNHOST ⚠ sign in` in amber.

- [ ] **Step 12: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): a host that wants a sign-in says so, and a click opens ssh in a terminal"
```

---

### Task 4: G7 — alert switches and the sound

**Files:**
- Modify: `crates/desktop/src/alerts.rs:1-22` (module comment), `:39-75` (`Settings`), `:115-122` (add `emit`, `sound_argv`, `switch` after `send`), `:157-211` (`observe`'s gate), `:223-232` (`open`), `:234-241` (`PanelResult`), `:265-377` (`AlertsPanel`)
- Modify: `crates/desktop/src/forms.rs:167-173` (`Target::Switch`)
- Modify: `crates/desktop/src/keys.rs` (`HELP`, Forms group)
- Modify: `crates/desktop/src/app.rs` (`Running.config_dir`, `alerts_result` `:1051-1070`, `snapshot`'s notification)
- Modify: `tooling/smoke/e2e.sh` (block before `# --- quit`; `only_pins_written` and its check)
- Test: `crates/desktop/tests/alerts.rs`

**Interfaces:**
- Consumes: `forms::on_path`, `spawn_detached` (exist); `Running.game` (Task 2); round 1 Task 5's `PanelResult::Approve` and `y` key (kept).
- Produces:
  - `alerts::Settings { pub desktop: bool, pub sound: bool, pub silence_focused: bool }`; `Settings::save(&self, config_dir: &Path) -> std::io::Result<()>`
  - `alerts::SOUND_PLAYER: &str = "canberra-gtk-play"`, `alerts::sound_argv() -> Vec<String>`, `alerts::emit(notes: &[Notification], settings: Settings, player: bool, spawn: &mut dyn FnMut(&[String]) -> std::io::Result<()>)`, `alerts::switch(current: Settings, wanted: Settings, save: &mut dyn FnMut(&Settings) -> std::io::Result<()>, player: bool) -> (Settings, Result<String, String>)`
  - `Alerts::settings(&self) -> Settings`, `Alerts::set_settings(&mut self, settings: Settings)`; `AlertsPanel::set_settings(&mut self, settings: Settings)`; `alerts::SWITCHES: usize = 3`
  - `PanelResult::Settings(Settings)`; `forms::Target::Switch(usize)`
  - app: `Running.config_dir: PathBuf`
- Departs from the spec: (a) the sound is tmls's exact command, `canberra-gtk-play -i message-new-instant -d tmls` (`notifications.py:53`), not `-i message`: R6 says "what tmls uses", and both apps then sound alike. (b) Enter on a switch does nothing (Space flips it, as the spec says); Enter keeps meaning "jump" in this panel. (c) The panel opens with the cursor on the newest alert as before, and on the first switch when there is none; the switches are above the alerts, so Up from the newest alert reaches them.

- [ ] **Step 1: Failing tests.** In `crates/desktop/tests/alerts.rs` add imports `use std::os::unix::fs::PermissionsExt;`, `use std::fs::Permissions;` and `desktop::alerts::{self, SOUND_PLAYER, emit, sound_argv}`. Replace `settings_are_read_from_notifications_json_and_never_written`, `the_panel_lists_the_alerts_newest_first`, `keys_and_clicks_in_the_panel_jump_to_the_session` and `a_long_list_follows_the_cursor` with:

```rust
#[test]
fn settings_are_read_from_notifications_json_and_written_when_switched() {
    let on = Settings {
        desktop: true,
        sound: false,
        silence_focused: true,
    };
    assert_eq!(Settings::default(), on);
    // No file: the defaults, and loading makes none.
    let dir = scratch("settings");
    assert_eq!(Settings::load(&dir), on);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "loading creates nothing");
    // tmls's own file.
    let file = dir.join("notifications.json");
    std::fs::write(&file, "{\"desktop\": false, \"sound\": true, \"silence_focused\": false}\n").unwrap();
    let off = Settings {
        desktop: false,
        sound: true,
        silence_focused: false,
    };
    assert_eq!(Settings::load(&dir), off);
    // Damaged, not an object, a value that is no boolean, a key missing: that key's default.
    for (text, want) in [
        ("{\"desktop\": fal", on),
        ("", on),
        ("[false, false]", on),
        ("{\"desktop\": 0, \"sound\": \"yes\", \"silence_focused\": \"no\"}", on),
        ("{\"sound\": true}", Settings { sound: true, ..on }),
        ("{\"silence_focused\": false, \"desktop\": null}", Settings { silence_focused: false, ..on }),
    ] {
        assert_eq!(Settings::parse(text), want, "{text}");
    }
    // Written as tmls writes it, whole, with no temp file left beside it.
    let saved = Settings {
        desktop: true,
        sound: true,
        silence_focused: false,
    };
    saved.save(&dir).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{\"desktop\": true, \"sound\": true, \"silence_focused\": false}\n"
    );
    assert_eq!(Settings::load(&dir), saved);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    // A config directory that does not exist yet is made.
    let fresh = dir.join("fresh");
    on.save(&fresh).unwrap();
    assert_eq!(Settings::load(&fresh), on);
}

#[test]
fn a_switch_that_cannot_be_saved_stays_and_says_why() {
    let on = Settings::default();
    let wanted = Settings { sound: true, ..on };
    let mut failing = |_: &Settings| -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    };
    let (kept, said) = alerts::switch(on, wanted, &mut failing, true);
    assert_eq!(kept, on);
    assert!(said.unwrap_err().starts_with("couldn't save the alert switches: "));
    // A read-only config directory: the real save fails the same way, the old file stays.
    // (The test container runs as uid 1000: the mode is enforced.)
    let dir = scratch("readonly");
    let file = dir.join("notifications.json");
    std::fs::write(&file, "{\"desktop\": fal").unwrap();
    std::fs::set_permissions(&dir, Permissions::from_mode(0o500)).unwrap();
    let result = wanted.save(&dir);
    std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{\"desktop\": fal");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temp file left");
    // Saved: the new settings, and the status line says which switch.
    let mut ok = |_: &Settings| -> std::io::Result<()> { Ok(()) };
    assert_eq!(alerts::switch(on, wanted, &mut ok, true), (wanted, Ok("Sound ON".to_string())));
    assert_eq!(alerts::switch(wanted, on, &mut ok, true), (on, Ok("Sound OFF".to_string())));
    let quiet = Settings { desktop: false, ..on };
    assert_eq!(alerts::switch(on, quiet, &mut ok, false), (quiet, Ok("Desktop OFF".to_string())));
    // Sound with no player: on all the same, and it says there will be none.
    assert_eq!(
        alerts::switch(on, wanted, &mut ok, false),
        (wanted, Ok(format!("Sound ON, but {SOUND_PLAYER} is not installed: no sound")))
    );
}

#[test]
fn each_alert_runs_what_the_switches_ask() {
    let note = Notification {
        title: "tmls · nas".to_string(),
        body: "? s · waiting".to_string(),
    };
    assert_eq!(sound_argv(), ["canberra-gtk-play", "-i", "message-new-instant", "-d", "tmls"]);
    let run = |settings: Settings, player: bool| {
        let mut started: Vec<Vec<String>> = Vec::new();
        emit(&[note.clone()], settings, player, &mut |argv: &[String]| {
            started.push(argv.to_vec());
            Ok(())
        });
        started
    };
    let on = Settings::default();
    assert_eq!(run(on, true), [notify_argv(&note)]);
    let both = Settings { sound: true, ..on };
    assert_eq!(run(both, true), [notify_argv(&note), sound_argv()]);
    assert_eq!(run(both, false), [notify_argv(&note)], "no player: no sound, no error");
    let sound_only = Settings { desktop: false, sound: true, ..on };
    assert_eq!(run(sound_only, true), [sound_argv()]);
    // Sound alone still makes `observe` hand out the alerts' notes.
    let mut alerts = Alerts::new(sound_only);
    list(&mut alerts, &[("s", Status::Running)]);
    assert_eq!(list(&mut alerts, &[("s", Status::Waiting)]).len(), 1);
}

#[test]
fn the_panel_shows_the_switches_then_the_alerts_newest_first() {
    let mut alerts = with_alerts(3);
    let opened = alerts.entries()[0].at + Duration::from_secs(5 * 60 + 10);
    let panel = alerts.open(opened).panel();
    assert_eq!((panel.cols, panel.lines), (60, 7));
    assert_eq!(
        lines(&panel),
        [
            "Alerts · Enter jumps · Space switches · Esc closes",
            " Desktop ON",
            " Sound OFF",
            " Silence focused ON",
            "  5m  ? s2 · nas  waiting",
            "  5m  ✕ s1 · nas  error",
            "  5m  ? s0 · nas  waiting",
        ]
    );
    // The cursor is on the newest alert; the error is red.
    let looks: Vec<Look> = panel.cells[1..].iter().map(|cell| cell.look).collect();
    let item = |hovered, danger| Look::Item { hovered, danger };
    assert_eq!(
        looks,
        [
            item(false, false),
            item(false, false),
            item(false, false),
            item(true, false),
            item(false, true),
            item(false, false)
        ]
    );
    // No alert yet: the cursor is on Desktop; Enter does nothing there, Space flips it.
    let mut none = Alerts::new(Settings::default());
    let mut empty = none.open(Instant::now());
    assert_eq!(lines(&empty.panel())[4], "nothing yet");
    assert_eq!(empty.panel().lines, 5);
    assert_eq!(empty.key(&press(Key::Named(Named::Enter))), PanelResult::Pending);
    assert_eq!(
        empty.key(&press(Key::Char(' '))),
        PanelResult::Settings(Settings {
            desktop: false,
            ..Settings::default()
        })
    );
    assert_eq!(lines(&empty.panel())[1], " Desktop OFF");
    assert_eq!(empty.key(&press(Key::Named(Named::Escape))), PanelResult::Closed);
}

#[test]
fn keys_and_clicks_in_the_panel_jump_to_the_session_or_flip_a_switch() {
    let mut alerts = with_alerts(3);
    let mut panel = alerts.open(Instant::now());
    let name = |panel: &desktop::alerts::AlertsPanel| panel.selected().map(|alert| alert.key.name.clone());
    assert_eq!(name(&panel), Some("s2".to_string()));
    panel.key(&press(Key::Char('j')));
    assert_eq!(name(&panel), Some("s1".to_string()));
    panel.key(&press(Key::Named(Named::Down)));
    panel.key(&press(Key::Named(Named::Down)));
    assert_eq!(name(&panel), Some("s0".to_string()));
    panel.key(&press(Key::Char('k')));
    assert_eq!(name(&panel), Some("s1".to_string()));
    assert_eq!(panel.key(&press(Key::Char('x'))), PanelResult::Pending);
    assert_eq!(panel.key(&press(Key::Named(Named::Tab))), PanelResult::Pending);
    // Enter jumps, once per press; a release does nothing.
    let enter = press(Key::Named(Named::Enter));
    let held = KeyEvent { repeat: true, ..enter.clone() };
    let released = KeyEvent { pressed: false, ..enter.clone() };
    assert_eq!(panel.key(&held), PanelResult::Pending);
    assert_eq!(panel.key(&released), PanelResult::Pending);
    assert_eq!(panel.key(&enter), PanelResult::Jump(key(HOST, "s1")));
    // Up past the newest alert: the switches. Space flips the one under the cursor; the
    // panel shows it at once.
    panel.key(&press(Key::Char('k')));
    panel.key(&press(Key::Named(Named::Up)));
    assert_eq!(name(&panel), None);
    assert_eq!(panel.key(&enter), PanelResult::Pending, "Enter on a switch does nothing");
    let quiet = Settings { silence_focused: false, ..Settings::default() };
    assert_eq!(panel.key(&press(Key::Char(' '))), PanelResult::Settings(quiet));
    assert_eq!(lines(&panel.panel())[3], " Silence focused OFF");
    for _ in 0..4 {
        panel.key(&press(Key::Named(Named::Up)));
    }
    assert_eq!(
        panel.key(&press(Key::Char(' '))),
        PanelResult::Settings(Settings { desktop: false, ..quiet }),
        "the cursor stops at Desktop"
    );
    assert_eq!(panel.key(&press(Key::Named(Named::Escape))), PanelResult::Closed);
    // The pointer: line 0 the title, 1–3 the switches, 4–6 the alerts.
    assert!(panel.hover(5.0, 6.5));
    assert!(!panel.hover(6.0, 6.2));
    assert_eq!(name(&panel), Some("s0".to_string()));
    assert!(!panel.hover(5.0, 0.5), "the title");
    assert_eq!(panel.click(59.5, 4.0), PanelResult::Jump(key(HOST, "s2")));
    assert_eq!(
        panel.click(3.0, 2.5),
        PanelResult::Settings(Settings { desktop: false, sound: true, silence_focused: false })
    );
    assert_eq!(panel.click(3.0, 0.5), PanelResult::Pending);
    assert_eq!(panel.click(60.5, 4.0), PanelResult::Closed);
    assert_eq!(panel.click(3.0, 7.0), PanelResult::Closed);
    assert_eq!(panel.click(-0.5, 4.0), PanelResult::Closed);
}

#[test]
fn a_long_list_follows_the_cursor() {
    let mut alerts = with_alerts(MAX_ALERTS);
    let mut panel = alerts.open(Instant::now());
    assert_eq!(panel.panel().lines, 16, "the title, three switches and twelve alerts");
    assert!(lines(&panel.panel())[4].contains(" s49 "));
    for _ in 0..MAX_ALERTS + 3 {
        panel.key(&press(Key::Named(Named::Down)));
    }
    assert_eq!(panel.selected().unwrap().key, key(HOST, "s0"));
    let shown = panel.panel();
    assert_eq!(shown.lines, 16);
    assert!(lines(&shown)[4].contains(" s11 "));
    assert!(lines(&shown)[15].contains(" s0 "));
    assert_eq!(panel.click(2.0, 15.5), PanelResult::Jump(key(HOST, "s0")));
    assert_eq!(panel.click(2.0, 4.5), PanelResult::Jump(key(HOST, "s11")));
}
```

Every other `Settings { desktop: …, silence_focused: … }` literal in this file (the silence and desktop-off tests, `:346`, `:364`) already ends in `..Settings::default()`; leave them.

- [ ] **Step 2: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test alerts`
Expected: compile errors (`Settings.sound`, `save`, `switch`, `emit`, `sound_argv`, `PanelResult::Settings`).

- [ ] **Step 3: Implement in `alerts.rs`.** Module comment's **Settings** paragraph becomes: "**Settings** are tmls's `~/.config/tmls/notifications.json`: `desktop` (default on), `sound` (default off: `canberra-gtk-play`, when it is installed) and `silence_focused` (default on). A missing or damaged file, or a value that is not a boolean, is the default. The three switches head the alerts list; Space flips one, and the file is written then, whole, as tmls writes it (`switch`)." Drop the paragraph that says approvals are not built (round 1 Task 5 built them).

`Settings`:

```rust
/// tmls's alert switches (`notifications.py`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Desktop notifications through `notify-send`.
    pub desktop: bool,
    /// A sound through `canberra-gtk-play` with every alert that notifies.
    pub sound: bool,
    /// No alert for the session the user is looking at.
    pub silence_focused: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            desktop: true,
            sound: false,
            silence_focused: true,
        }
    }
}
```

`parse` gains `sound: switch("sound", defaults.sound),`; `load`'s doc: "Reads `notifications.json` in `config_dir`. Never fails, never writes." After `parse`:

```rust
    /// Writes `notifications.json` in `config_dir` as tmls does (`json.dumps(asdict(…))`):
    /// the text goes to a temp file that is renamed over the old one, so a reader sees the old
    /// switches or the new ones; a write that fails leaves the old file and no temp file.
    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let text = format!(
            "{{\"desktop\": {}, \"sound\": {}, \"silence_focused\": {}}}\n",
            self.desktop, self.sound, self.silence_focused
        );
        let temp = config_dir.join(format!(".notifications.json.tmp-{}", std::process::id()));
        let written = std::fs::write(&temp, text)
            .and_then(|()| std::fs::rename(&temp, config_dir.join("notifications.json")));
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        written
    }
```

After `send`:

```rust
/// The program that plays the alert sound (R6); without it there is none.
pub const SOUND_PLAYER: &str = "canberra-gtk-play";

/// The sound tmls plays with an alert (`notifications.emit`).
pub fn sound_argv() -> Vec<String> {
    [SOUND_PLAYER, "-i", "message-new-instant", "-d", "tmls"]
        .map(str::to_string)
        .to_vec()
}

/// What `settings` ask for each note: the notification when `desktop`, the sound when `sound`
/// and `player` (`canberra-gtk-play` is on PATH). Nothing that cannot start is an error.
pub fn emit(
    notes: &[Notification],
    settings: Settings,
    player: bool,
    spawn: &mut dyn FnMut(&[String]) -> std::io::Result<()>,
) {
    for note in notes {
        if settings.desktop {
            let _ = spawn(&notify_argv(note));
        }
        if settings.sound && player {
            let _ = spawn(&sound_argv());
        }
    }
}

/// How many switches head the alerts list: Desktop, Sound, Silence focused.
pub const SWITCHES: usize = 3;

/// The switches as the list shows them, in order.
fn switch_rows(settings: Settings) -> [(&'static str, bool); SWITCHES] {
    [
        ("Desktop", settings.desktop),
        ("Sound", settings.sound),
        ("Silence focused", settings.silence_focused),
    ]
}

/// `settings` with switch `index` flipped.
fn flipped(settings: Settings, index: usize) -> Settings {
    match index {
        0 => Settings { desktop: !settings.desktop, ..settings },
        1 => Settings { sound: !settings.sound, ..settings },
        _ => Settings { silence_focused: !settings.silence_focused, ..settings },
    }
}

/// From `current` to `wanted`: saved through `save` (the app: `Settings::save` in
/// `~/.config/tmls`). Returns the settings to use from now on and the status line. A switch
/// that cannot be saved stays as it was; Sound turned on without a `player` is on all the
/// same, and says there will be no sound.
pub fn switch(
    current: Settings,
    wanted: Settings,
    save: &mut dyn FnMut(&Settings) -> std::io::Result<()>,
    player: bool,
) -> (Settings, Result<String, String>) {
    if let Err(e) = save(&wanted) {
        return (current, Err(format!("couldn't save the alert switches: {e}")));
    }
    let changed: Vec<String> = switch_rows(current)
        .into_iter()
        .zip(switch_rows(wanted))
        .filter(|((_, was), (_, now))| was != now)
        .map(|(_, (name, now))| format!("{name} {}", if now { "ON" } else { "OFF" }))
        .collect();
    let mut said = changed.join(", ");
    if wanted.sound && !current.sound && !player {
        said = format!("{said}, but {SOUND_PLAYER} is not installed: no sound");
    }
    (wanted, Ok(said))
}
```

In `observe`, `if self.settings.desktop && status != Status::Error {` becomes `if (self.settings.desktop || self.settings.sound) && status != Status::Error {`. `impl Alerts` gains:

```rust
    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }
```

`open` gives the panel the settings and starts on the newest alert, else on the first switch:

```rust
    pub fn open(&mut self, now: Instant) -> AlertsPanel {
        self.unread = 0;
        AlertsPanel {
            entries: self.entries.clone(),
            settings: self.settings,
            now,
            cursor: if self.entries.is_empty() { 0 } else { SWITCHES },
        }
    }
```

`PanelResult` gains `/// A switch was flipped: these are the settings now.\n Settings(Settings),`. `AlertsPanel` gains the field `settings: Settings,`; its `cursor` doc: "The row the arrows or the pointer are on: `0..SWITCHES` the switches, then the alerts." Its methods:

```rust
    /// The alert the cursor is on; `None` on a switch.
    pub fn selected(&self) -> Option<&Alert> {
        self.entries.get(self.cursor.checked_sub(SWITCHES)?)
    }

    /// The switches it shows, after a flip that could not be saved was undone.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// Flips the switch under the cursor.
    fn flip(&mut self, index: usize) -> PanelResult {
        self.settings = flipped(self.settings, index);
        PanelResult::Settings(self.settings)
    }

    /// Up / Down or k / j move over the switches and the alerts, Enter jumps to an alert's
    /// session, Space flips a switch, Esc closes; nothing else does anything.
    pub fn key(&mut self, ev: &KeyEvent) -> PanelResult {
        if !ev.pressed {
            return PanelResult::Pending;
        }
        let last = SWITCHES + self.entries.len().saturating_sub(1);
        match ev.key {
            Key::Named(Named::Up) | Key::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            Key::Named(Named::Down) | Key::Char('j') => self.cursor = (self.cursor + 1).min(last),
            Key::Named(Named::Enter) if !ev.repeat => {
                return self.selected().map_or(PanelResult::Pending, |alert| {
                    PanelResult::Jump(alert.key.clone())
                });
            }
            Key::Char(' ') if !ev.repeat && self.cursor < SWITCHES => return self.flip(self.cursor),
            Key::Named(Named::Escape) => return PanelResult::Closed,
            _ => {}
        }
        PanelResult::Pending
    }

    /// The row under a position: a switch's index, or `SWITCHES` + an alert's.
    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        match self.panel().target_at(x, y) {
            Some((Target::Switch(index), _)) => Some(index),
            Some((Target::Item(index), _)) => Some(SWITCHES + index),
            _ => None,
        }
    }

    pub fn hover(&mut self, x: f32, y: f32) -> bool {
        let Some(row) = self.row_at(x, y) else {
            return false;
        };
        let changed = self.cursor != row;
        self.cursor = row;
        changed
    }

    /// A click: flips a switch, jumps to an alert's session, closes beside the panel.
    pub fn click(&mut self, x: f32, y: f32) -> PanelResult {
        match self.row_at(x, y) {
            Some(row) if row < SWITCHES => {
                self.cursor = row;
                self.flip(row)
            }
            Some(row) => PanelResult::Jump(self.entries[row - SWITCHES].key.clone()),
            None if self.panel().contains(x, y) => PanelResult::Pending,
            None => PanelResult::Closed,
        }
    }

    /// The first alert shown: the view follows the cursor.
    fn first(&self) -> usize {
        (self.cursor.saturating_sub(SWITCHES) + 1).saturating_sub(PANEL_ROWS)
    }

    /// The panel as text in cells: a title, the three switches, then one line per alert.
    pub fn panel(&self) -> Panel {
        let mut p = Panel::new(PANEL_COLS);
        let title = "Alerts · Enter jumps · Space switches · Esc closes";
        p.put(0, 1, PANEL_COLS - 2, title, Look::Title);
        for (index, (name, on)) in switch_rows(self.settings).into_iter().enumerate() {
            let look = Look::Item {
                hovered: index == self.cursor,
                danger: false,
            };
            let text = format!(" {name} {}", if on { "ON" } else { "OFF" });
            p.put(1 + index, 0, PANEL_COLS, &text, look).target = Some(Target::Switch(index));
        }
        let top = 1 + SWITCHES;
        if self.entries.is_empty() {
            p.put(top, 1, PANEL_COLS - 2, "nothing yet", Look::Dim);
        }
        let shown = self
            .entries
            .iter()
            .enumerate()
            .skip(self.first())
            .take(PANEL_ROWS);
        for (line, (index, alert)) in shown.enumerate() {
            let look = Look::Item {
                hovered: SWITCHES + index == self.cursor,
                danger: alert.status == Status::Error,
            };
            let text = format!(
                " {:>3}  {} {} · {}  {}",
                age(self.now.saturating_duration_since(alert.at)),
                alert.status.mark(),
                alert.key.name,
                alert.key.host,
                alert.detail(),
            );
            p.put(top + line, 0, PANEL_COLS, &text, look).target = Some(Target::Item(index));
        }
        p.lines = top + self.entries.len().clamp(1, PANEL_ROWS);
        p
    }
```

(Round 1 Task 5's `Key::Char('y')` arm in `key` stays, before the Escape arm; it uses `selected()`, which is `None` on a switch.)

`forms.rs` `Target` gains `/// A switch of the alerts list.\n Switch(usize),`. `keys.rs` `HELP`, Forms group, append `("Space", "a switch in the alerts"),`.

- [ ] **Step 4: Run the desktop tests.**
Run: `tooling/docker/build.sh test -p desktop --test alerts --test keys`
Expected: pass.

- [ ] **Step 5: Wire `app.rs`.** `Running` gains `/// \`~/.config/tmls\`: where \`notifications.json\` is written.\n config_dir: PathBuf,` (`config_dir.to_path_buf()` in `start`). In `snapshot`, the block Task 2 wrote becomes:

```rust
        // In game mode no notification runs; the bell's list has the alerts all the same.
        if !self.game && !notes.is_empty() {
            let settings = self.alerts.settings();
            let player = settings.sound && forms::on_path(alerts::SOUND_PLAYER);
            alerts::emit(&notes, settings, player, &mut forms::spawn_detached);
        }
```

`alerts_result` gains:

```rust
            PanelResult::Settings(wanted) => {
                let dir = self.config_dir.clone();
                let player = forms::on_path(alerts::SOUND_PLAYER);
                let (now, said) = alerts::switch(
                    self.alerts.settings(),
                    wanted,
                    &mut |settings| settings.save(&dir),
                    player,
                );
                self.alerts.set_settings(now);
                if let Some((Modal::Alerts(panel), _)) = &mut self.modal {
                    panel.set_settings(now);
                }
                self.say(said);
            }
```

- [ ] **Step 6: e2e block.** In `e2e.sh`, the quit section's helper and check change (the app now writes `notifications.json` too):

```bash
only_config_written() { [ "$(find "$HOME/.config/tmls" -mindepth 1 | sort | tr '\n' '|')" = "$HOME/.config/tmls/notifications.json|$PINS|" ]; }
```

(it replaces `only_pins_written`, defined next to `pins_are`), and `check "at exit ~/.config/tmls holds pins and nothing else" only_pins_written` becomes `check "at exit ~/.config/tmls holds pins and notifications.json and nothing else" only_config_written`. Before `# --- quit` (after Task 1's block):

```bash
# --- alert switches (G7) -------------------------------------------------------------------------
# The list opens on the newest alert (or on Desktop when there is none): up to Desktop, down to
# Sound, Space. canberra-gtk-play is not in the image: the status line says there is no sound.
NOTES="$HOME/.config/tmls/notifications.json"
notes_are() { [ "$(cat "$NOTES" 2>/dev/null)" = "$1" ]; }
key_focus_list
keys -k a
keys -k k -k k -k k -k k
keys -k j
keys -k space
if wait_for 3 notes_are '{"desktop": true, "sound": true, "silence_focused": true}'; then
  pass "Space on Sound writes notifications.json with sound true"
else
  fail "Space on Sound writes notifications.json with sound true" "file: $(cat "$NOTES" 2>&1)"
fi
check "screenshot 44-alert-switches (Sound ON; no player)" shot 44-alert-switches
keys -k space
check "Space again writes sound false" wait_for 3 notes_are '{"desktop": true, "sound": false, "silence_focused": true}'
keys -k Escape
```

- [ ] **Step 7: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; client-core tests; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/t4-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/round2/t4-e2e/44-alert-switches.png`: three switch rows, Sound ON, the status line `Sound ON, but canberra-gtk-play is not installed: no sound`.

- [ ] **Step 8: Commit.**

```bash
git add crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): the alerts list switches desktop, sound and silence, saved where tmls keeps them"
```

---

### Task 5: G8 — recent prompts in the Ask form

**Files:**
- Create: `crates/client-core/src/prompts.rs`, `crates/client-core/tests/prompts.rs`
- Modify: `crates/client-core/src/lib.rs` (`pub mod prompts;`)
- Modify: `crates/desktop/src/forms.rs` (round 1 Task 7's `Form::Ask`: field `recent`, `ask`, the `Up | Down` key arm, the panel arm; new `set_recent`, `recent_prompts`)
- Modify: `crates/desktop/src/app.rs` (`UserEvent::Recent`, `user_event`, round 1 Task 7's `ask_form`)
- Modify: `tooling/smoke/e2e.sh` (top variables, `cleanup`, block before `# --- quit`)
- Test: `crates/client-core/tests/prompts.rs`, `crates/desktop/tests/forms.rs`

**Interfaces:**
- Consumes: `Form::Ask`, `Form::ask`, `MAX_SAVED_SHOWN`, `FormResult::Ask`, `Running::ask_form`, `forms::is_claude` (from round 1 Task 7); `SessionInfo.session_id` (exists); `tmux::script_argv`, `tmux::quote` (crate-private, exist); e2e `fake_claude`, `top_of_list` (from round 1 Task 5).
- Produces: `client_core::prompts::{RECENT: usize = 10, recent_argv(key: &SessionKey, session_id: &str, local_name: &str) -> Vec<String>, parse_recent(out: &str) -> Vec<String>}`; `Form::Ask.recent: Option<Vec<String>>` (`None` while loading); `Form::set_recent(&mut self, session: &SessionKey, prompts: Vec<String>)`; `forms::recent_prompts(key, session_id: &str, local_name, run: Runner) -> Vec<String>`; app `UserEvent::Recent(SessionKey, Vec<String>)`.
- Departs from the spec: `recent_argv` takes the session's Claude id from the listing (`SessionInfo.session_id`, already checked by `files::valid_session_id`) instead of grepping every status file for the session's name as tmls does: same transcript, one command fewer, and nothing found by a name another session once had.

- [ ] **Step 1: Failing client-core tests.** `crates/client-core/tests/prompts.rs`:

```rust
//! Earlier messages for the Ask form: the user lines of a Claude transcript, newest first.

use std::path::PathBuf;
use std::process::Command;

use client_core::listing::SessionKey;
use client_core::prompts::{RECENT, parse_recent, recent_argv};

const SID: &str = "2b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey { host: host.to_string(), name: name.to_string() }
}

fn user(content: &str) -> String {
    serde_json::json!({"type": "user", "message": {"role": "user", "content": content}}).to_string()
}

#[test]
fn recent_is_what_the_user_typed_newest_first_each_once() {
    let lines = [
        user("first"),
        user("second"),
        serde_json::json!({"type": "user", "message": {"content": [{"tool_use_id": "t1", "type": "tool_result"}]}}).to_string(),
        serde_json::json!({"type": "user", "isMeta": true, "message": {"content": "meta"}}).to_string(),
        user("<command-name>/clear</command-name>"),
        "not json at all".to_string(),
        user("first"),
        user("third\nwith a second line"),
    ];
    assert_eq!(parse_recent(&lines.join("\n")), ["third\nwith a second line", "first", "second"]);
    let many: Vec<String> = (0..15).map(|n| user(&format!("m{n}"))).collect();
    let recent = parse_recent(&many.join("\n"));
    assert_eq!(recent.len(), RECENT);
    assert_eq!((recent[0].as_str(), recent[9].as_str()), ("m14", "m5"));
    assert!(parse_recent("").is_empty());
}

#[test]
fn recent_argv_reads_the_sessions_transcript_and_nothing_else() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("prompts-{}", std::process::id()));
    let projects = dir.join(".claude/projects/-work");
    std::fs::create_dir_all(&projects).unwrap();
    let transcript = [
        user("hello"),
        serde_json::json!({"type": "assistant", "message": {"content": "hi"}}).to_string(),
        serde_json::json!({"type": "user", "message": {"content": [{"tool_use_id": "t", "type": "tool_result"}]}}).to_string(),
        user("again"),
    ];
    std::fs::write(projects.join(format!("{SID}.jsonl")), transcript.join("\n") + "\n").unwrap();
    std::fs::write(projects.join("other.jsonl"), user("not this one") + "\n").unwrap();
    let argv = recent_argv(&key("here", "w"), SID, "here");
    assert_eq!(argv[..2], ["sh", "-c"]);
    let out = Command::new(&argv[0]).args(&argv[1..]).env("HOME", &dir).output().unwrap();
    assert!(out.status.success());
    assert_eq!(parse_recent(&String::from_utf8_lossy(&out.stdout)), ["again", "hello"]);
    // Another host: over ssh, the id quoted.
    let there = recent_argv(&key("ph", "w"), SID, "here");
    assert_eq!(there[0], "ssh");
    assert!(there.last().unwrap().contains(&format!("/'{SID}'.jsonl")), "{there:?}");
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test prompts`
Expected: compile errors (module `prompts`).

- [ ] **Step 3: Implement.** `crates/client-core/src/prompts.rs`:

```rust
//! Earlier messages for the Ask form (tmls `prompts.recent_argv`, `parse_recent`, R7): the
//! lines the user typed into a Claude session, read from its transcript, newest first.

use serde_json::Value;

use crate::listing::SessionKey;

/// How many earlier messages the form lists.
pub const RECENT: usize = 10;

/// Prints the user lines of the transcript of Claude session `session_id` on `key`'s host:
/// the last 200 lines of type `user` that are no tool result, from its last 2 MB. The id is
/// the listing's (`SessionInfo::session_id`, an id by `files::valid_session_id`), quoted all
/// the same.
pub fn recent_argv(key: &SessionKey, session_id: &str, local_name: &str) -> Vec<String> {
    let script = format!(
        "for t in \"$HOME\"/.claude/projects/*/{}.jsonl; do [ -f \"$t\" ] && tail -c 2000000 \"$t\" | grep '\"type\":\"user\"' | grep -v '\"tool_use_id\"' | tail -n 200; done; true",
        crate::tmux::quote(session_id)
    );
    crate::tmux::script_argv(&key.host, script, local_name)
}

/// The messages the user typed (not tool results, not commands, not meta lines), newest first,
/// each once, at most `RECENT`.
pub fn parse_recent(out: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for line in out.lines().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        if entry.get("isMeta").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(content) = entry.get("message").and_then(|m| m.get("content")).and_then(Value::as_str) else {
            continue;
        };
        if content.starts_with('<') || seen.iter().any(|known| known == content) {
            continue;
        }
        seen.push(content.to_string());
        if seen.len() == RECENT {
            break;
        }
    }
    seen
}
```

`lib.rs`: `pub mod prompts;` and a line in the crate comment.

- [ ] **Step 4: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass.

- [ ] **Step 5: Failing desktop test.** Append to `crates/desktop/tests/forms.rs`:

```rust
#[test]
fn the_ask_form_lists_recent_prompts_under_the_saved_ones() {
    let k = key(LOCAL, "w");
    let saved = vec!["What's the progress?".to_string()];
    let mut form = Form::ask(k.clone(), Some("%3".to_string()), saved.clone(), 0);
    assert!(text(&form.panel()).contains("Recent · loading…"));
    // An answer for another session changes nothing.
    form.set_recent(&key(LOCAL, "other"), vec!["no".to_string()]);
    assert!(text(&form.panel()).contains("Recent · loading…"));
    form.set_recent(&k, vec!["newest".to_string(), "older".to_string()]);
    let said = text(&form.panel());
    assert!(
        said.contains("Recent (↑ / ↓)") && said.contains("newest") && said.contains("older"),
        "{said}"
    );
    assert!(!said.contains("loading"));
    // ↓ walks the saved prompt, then the recent ones, then round; ↑ the other way.
    let field = |form: &Form| match form {
        Form::Ask { field, .. } => field.text().to_string(),
        _ => panic!("the Ask form"),
    };
    let down = named(Named::Down);
    for want in ["What's the progress?", "newest", "older", "What's the progress?"] {
        assert_eq!(form.key(&down), FormResult::Pending);
        assert_eq!(field(&form), want);
    }
    form.key(&named(Named::Up));
    assert_eq!(field(&form), "older");
    assert_eq!(
        form.key(&named(Named::Enter)),
        FormResult::Ask {
            key: k.clone(),
            pane: Some("%3".to_string()),
            text: "older".to_string(),
        }
    );
    // No earlier message: no Recent heading at all; a message of several lines is one row.
    let mut none = Form::ask(k.clone(), None, saved.clone(), 0);
    none.set_recent(&k, Vec::new());
    assert!(!text(&none.panel()).contains("Recent"));
    let mut lines = Form::ask(k.clone(), None, saved, 0);
    lines.set_recent(&k, vec!["one\ntwo".to_string()]);
    assert!(text(&lines.panel()).contains("one ⏎ two"));
}

#[test]
fn recent_prompts_come_from_the_transcript_or_are_none() {
    let k = key("ph", "w");
    let sid = "2b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";
    let asked = RefCell::new(Vec::new());
    let run = |argv: &[String]| -> anyhow::Result<String> {
        asked.borrow_mut().push(argv.to_vec());
        Ok("{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n".to_string())
    };
    assert_eq!(forms::recent_prompts(&k, sid, LOCAL, &run), ["hi"]);
    assert_eq!(*asked.borrow(), [client_core::prompts::recent_argv(&k, sid, LOCAL)]);
    let down = |_: &[String]| -> anyhow::Result<String> { anyhow::bail!("timed out") };
    assert!(forms::recent_prompts(&k, sid, LOCAL, &down).is_empty());
}
```

- [ ] **Step 6: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test forms`
Expected: compile errors (`set_recent`, `recent_prompts`).

- [ ] **Step 7: Implement in `forms.rs`.** Imports: `use client_core::prompts;`. `Form::Ask` gains, after `saved`:

```rust
        /// The user's earlier messages, newest first; `None` until the host has said (R7).
        recent: Option<Vec<String>>,
```

`Form::ask` sets `recent: None,`. After `set_pane_info`:

```rust
    /// The earlier messages of `session`, for the Ask form of that session.
    pub fn set_recent(&mut self, session: &SessionKey, prompts: Vec<String>) {
        if let Form::Ask { key, recent, .. } = self
            && key == session
        {
            *recent = Some(prompts);
        }
    }
```

In `key()`, round 1 Task 7's `Form::Ask { field, focus: Stop::Name, saved, pick, error, .. }` arm becomes:

```rust
            Form::Ask {
                field,
                focus: Stop::Name,
                saved,
                recent,
                pick,
                error,
                ..
            } => match ev.key {
                Key::Named(Named::Up | Named::Down) if !chord => {
                    // The saved prompts shown, then the earlier messages.
                    let choices: Vec<&String> = saved
                        .iter()
                        .take(MAX_SAVED_SHOWN)
                        .chain(recent.iter().flatten())
                        .collect();
                    if !choices.is_empty() {
                        let n = choices.len();
                        let forward = ev.key == Key::Named(Named::Down);
                        let next = match *pick {
                            None if forward => 0,
                            None => n - 1,
                            Some(at) => cycle(at.min(n - 1), n, forward),
                        };
                        *pick = Some(next);
                        set_text(field, choices[next]);
                        *error = None;
                    }
                }
                _ => {
                    let before = field.text().to_string();
                    if edit(field, ev) && field.text() != before {
                        *error = None;
                    }
                }
            },
```

In `panel()`'s `Form::Ask` arm add `recent,` to the bindings and replace `let buttons = 5 + shown + 1;` with:

```rust
                let mut line = 5 + shown;
                match recent {
                    None => {
                        p.put(line, PAD, inner, "Recent · loading…", Look::Dim);
                        line += 1;
                    }
                    Some(recent) if !recent.is_empty() => {
                        p.put(line, PAD, inner, "Recent (↑ / ↓)", Look::Label);
                        line += 1;
                        for (n, prompt) in recent.iter().enumerate() {
                            let look = match *pick == Some(shown + n) {
                                true => Look::Choice {
                                    focused: *focus == Stop::Name,
                                    on: true,
                                },
                                false => Look::Dim,
                            };
                            // One row each: a message of several lines shows them joined.
                            p.put(line, PAD, inner, &prompt.replace('\n', " ⏎ "), look);
                            line += 1;
                        }
                    }
                    Some(_) => {}
                }
                let buttons = line + 1;
```

After `saved_prompts`:

```rust
/// The user's earlier messages to the session's Claude, newest first (R7); none when the host
/// did not say.
pub fn recent_prompts(key: &SessionKey, session_id: &str, local_name: &str, run: Runner) -> Vec<String> {
    run(&prompts::recent_argv(key, session_id, local_name))
        .map(|out| prompts::parse_recent(&out))
        .unwrap_or_default()
}
```

- [ ] **Step 8: Run the desktop unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test forms`
Expected: pass.

- [ ] **Step 9: Wire `app.rs`.** `UserEvent` gains `/// The earlier messages of a session, for its Ask form.\n Recent(SessionKey, Vec<String>),`; `user_event`:

```rust
            UserEvent::Recent(key, prompts) => {
                if let Some((Modal::Form(form), _)) = &mut running.modal {
                    form.set_recent(&key, prompts);
                    running.invalidate();
                }
            }
```

Round 1 Task 7's `ask_form` becomes:

```rust
    fn ask_form(&mut self, key: SessionKey) {
        let Some(info) = self.sidebar.info(&key) else {
            return self.say(Err(format!("{} is not listed any more", key.name)));
        };
        if !forms::is_claude(info) {
            return self.say(Err(format!("{} is not a Claude session", key.name)));
        }
        let (pane, session_id) = (info.pane.clone(), info.session_id.clone());
        let saved = forms::saved_prompts(std::fs::read_to_string(&self.prompts_file).ok().as_deref());
        let queued = self.queue.count(&key);
        self.open_modal(Modal::Form(Form::ask(key.clone(), pane, saved, queued)), None);
        // The earlier messages come from the transcript, off this thread (R7).
        let Some(id) = session_id else {
            if let Some((Modal::Form(form), _)) = &mut self.modal {
                form.set_recent(&key, Vec::new());
            }
            return;
        };
        let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
        std::thread::spawn(move || {
            let recent = forms::recent_prompts(&key, &id, &local, &forms::run_tmux);
            let _ = proxy.send_event(UserEvent::Recent(key, recent));
        });
    }
```

- [ ] **Step 10: e2e block.** Top: `RECENT='tmlsrs-test-00recent'`, `RECENT_SID='4b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e'`; `cleanup()`: `tmux kill-session -t "=$RECENT" 2>/dev/null`. Before `# --- quit` (after Task 4's block):

```bash
# --- recent prompts in the Ask form (G8) ---------------------------------------------------------
# A Claude stand-in whose transcript has two typed messages, a tool result and a meta line. The
# form lists the saved prompt (the default), then the newest message: ↓ ↓ picks it.
tmux new-session -d -s "$RECENT" -x 100 -y 30
fake_claude "$RECENT" "$RECENT_SID" idle
mkdir -p "$HOME/.claude/projects/-work"
{
  printf '{"type":"user","message":{"role":"user","content":"echo tmls-recent-old-%s"}}\n' "$NONCE"
  printf '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}\n'
  printf '{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}\n'
  printf '{"type":"user","message":{"role":"user","content":"echo tmls-recent-%s"}}\n' "$NONCE"
} >"$HOME/.claude/projects/-work/$RECENT_SID.jsonl"
sleep 6
key_focus_list
top_of_list
keys s
sleep 1.5
check "screenshot 45-ask-recent (Saved, then Recent newest first)" shot 45-ask-recent
keys -k Down -k Down
key_enter
if wait_for 8 pane_has_line "$RECENT" "tmls-recent-$NONCE"; then
  pass "↓ ↓ in the Ask form picks the newest recent message and Enter sends it"
else
  fail "↓ ↓ in the Ask form picks the newest recent message and Enter sends it" "pane: $(pane_text "$RECENT" | grep -v '^$' | tail -3 | tr '\n' '|')"
fi
tmux kill-session -t "=$RECENT"
```

- [ ] **Step 11: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; client-core tests; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/t5-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/round2/t5-e2e/45-ask-recent.png`: `Saved (↑ / ↓)`, the default prompt, `Recent (↑ / ↓)`, the two messages newest first.

- [ ] **Step 12: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): the Ask form lists the last ten messages you typed to that Claude"
```

---

### Task 6: G5 — other tmux servers, in a collapsed section

**Files:**
- Modify: `crates/client-core/src/listing.rs` (after `listing_command`, `:170-181`: `BACKGROUND_SEP`, `split_host`, `valid_server_name`, `parse_background`)
- Modify: `crates/client-core/src/tmux.rs:160-183` (`attach_argv`, `attach_text`); add `SERVER_SOCKETS_SCRIPT`, `server_sockets_argv` after `sign_in_argv`
- Modify: `crates/desktop/src/sidebar.rs:42-52` (`Row`), `:71-80` (`Action`), `:98-149` (`Sidebar`, `new`), `:376-402` (`click`, `menu`), `:412-418` (`toggle_pin`); add `apply_background`, `toggle_background`, `background_open`, `place_background`, `background_rows`, `background_place`
- Modify: `crates/desktop/src/draw.rs` (add `background_spans` after `rule_spans`, `:1013-1027`; a `Row::Background` arm in `sidebar()`)
- Modify: `crates/desktop/src/keys.rs` (`ListKey::Background`, `list_key`, `HELP`)
- Modify: `crates/desktop/src/app.rs` (`UserEvent::Background`, `tick`, `snapshot`, `list_key` `:1726-1750`, `act` `:1753-1763`, `foot_button` `:1793-1819`)
- Modify: `tooling/smoke/e2e.sh` (`cleanup`; the third run: a `tmux -L` server before the app starts, checks above the anchor, the sign-in click moves a line down)
- Test: `crates/client-core/tests/listing.rs`, `crates/client-core/tests/tmux.rs`, `crates/desktop/tests/sidebar.rs`, `crates/desktop/tests/sidebar_draw.rs`, `crates/desktop/tests/keys.rs`

**Interfaces:**
- Consumes: the ticker and `Running.game` (Task 2); the third run (Tasks 2, 3); `SessionInfo.last_reply` (from round 1 Task 6); e2e `list_picture` (from round 1 Task 6).
- Produces:
  - `listing::{BACKGROUND_SEP: char = '#', split_host(host: &str) -> (&str, Option<&str>), valid_server_name(name: &str) -> bool, parse_background(host: &str, output: &str) -> Vec<HostSnapshot>}`
  - `tmux::{SERVER_SOCKETS_SCRIPT: &str, server_sockets_argv(host: &str, local_name: &str) -> Vec<String>}`; `attach_argv`/`attach_text` speak `-L NAME` for a host `HOST#NAME`
  - `sidebar::Row::Background { host: String, count: usize, open: bool }`, `sidebar::Action::ToggleBackground`; `Sidebar::apply_background(&mut self, host: &str, sessions: Vec<SessionInfo>) -> bool`, `Sidebar::toggle_background(&mut self)`, `Sidebar::background_open(&self) -> bool`
  - `draw::background_spans(count: usize, open: bool, cols: usize, theme: &Theme) -> Vec<Span<'static>>`; `keys::ListKey::Background`
  - app: `UserEvent::Background(String, Vec<SessionInfo>)`
- Departs from the spec: (a) "listing.rs parses the background section (`background NAME:` lines)": they come from a script of their own (`SERVER_SOCKETS_SCRIPT`), not from `LISTING_SCRIPT`, so the 5 s listing does not grow; `listing::parse_background` reads it. (b) "rescanned every 6th poll": every 30 s on the ticker (six default polls), remote hosts not while game mode is on (R1). (c) A background session's dim line says `tmux -L NAME` (its `title`), so two servers' sessions of the same name can be told apart. (d) The row reads `background ▸ · N` as the spec writes it (tmls: `▸ background · N`); open: `background ▾ · N`.

- [ ] **Step 1: Failing client-core tests.** Append to `crates/client-core/tests/listing.rs` (import `parse_background, split_host, valid_server_name` from `client_core::listing`):

```rust
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
```

Append to `crates/client-core/tests/tmux.rs` (import `server_sockets_argv`; `use std::path::PathBuf;` if not there):

```rust
#[test]
fn server_names_that_are_not_names_are_skipped() {
    use std::os::unix::net::UnixListener;
    let run = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("sockets-{}-{run}", std::process::id()));
    let sockets = dir.join("tmux-4242");
    std::fs::create_dir_all(&sockets).unwrap();
    let _listening: Vec<UnixListener> = ["default", "web", "a b", "x;y", "jobs.2"]
        .iter()
        .map(|name| UnixListener::bind(sockets.join(name)).unwrap())
        .collect();
    std::fs::write(sockets.join("plain-file"), "").unwrap();
    let argv = server_sockets_argv(LOCAL, LOCAL);
    assert_eq!(argv[..2], v(&["sh", "-c"]));
    let format = "#{session_name}:#{session_windows}:#{session_attached}:#{window_activity}";
    let stubs = "id() { echo 4242; }; date() { echo 1000; }; tmux() { echo \"T $*\"; }; ";
    let ran = Command::new("sh").args(["-c", &format!("{stubs}{}", argv[2])]).env("TMUX_TMPDIR", &dir).output().unwrap();
    assert!(ran.status.success());
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        format!("1000\nbackground jobs.2:\nT -L jobs.2 -u list-windows -a -F {format}\nbackground web:\nT -L web -u list-windows -a -F {format}\n")
    );
    // No socket directory at all: the clock, nothing else.
    let none = Command::new("sh").args(["-c", &format!("{stubs}{}", argv[2])]).env("TMUX_TMPDIR", dir.join("nowhere")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&none.stdout), "1000\n");
    // Another host: the same script over ssh.
    assert_eq!(remote(&server_sockets_argv("ph", LOCAL), &SSH), argv[2]);
}

#[test]
fn a_background_server_attaches_with_its_socket_name() {
    let here = key(&format!("{LOCAL}#web"), "site");
    assert_eq!(attach_argv(&here, LOCAL), v(&["tmux", "-u", "-L", "web", "attach", "-t", "=site"]));
    assert_eq!(attach_text(&here, LOCAL), "tmux -u -L web attach -t =site");
    let there = remote(&attach_argv(&key("ph#web", "it's"), LOCAL), &ATTACH);
    assert_eq!(there, "tmux -u -L 'web' attach -t '=it'\\''s'");
    assert_eq!(words(&there), v(&["-u", "-L", "web", "attach", "-t", "=it's"]));
    assert_eq!(attach_text(&key("ph#web", "site"), LOCAL), "ssh -t ph 'tmux -u -L web attach -t =site'");
    // The default server is unchanged.
    assert_eq!(attach_argv(&key(LOCAL, "w"), LOCAL), v(&["tmux", "-u", "attach", "-t", "=w"]));
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test listing --test tmux`
Expected: compile errors (`parse_background`, `split_host`, `valid_server_name`, `server_sockets_argv`).

- [ ] **Step 3: Implement.** `listing.rs`, after `listing_command`:

```rust
/// Between a machine and one of its other tmux servers in a host's name: `HOST#NAME` (tmls
/// `hosts.SEP`). Such a host is listed under its machine's `background` row.
pub const BACKGROUND_SEP: char = '#';

/// `(machine, Some(server))` for `HOST#NAME`, `(host, None)` for any other host.
pub fn split_host(host: &str) -> (&str, Option<&str>) {
    match host.split_once(BACKGROUND_SEP) {
        Some((machine, server)) if !server.is_empty() => (machine, Some(server)),
        _ => (host, None),
    }
}

/// Whether `name` is a tmux server name tmls-rs passes to `tmux -L`: tmls's `[\w.-]+`, in
/// ASCII. Anything else never goes into a command.
pub fn valid_server_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

/// Reads what `tmux::server_sockets_argv` printed on `host`: one snapshot per other tmux
/// server, its host `HOST#NAME`, its sessions with no Claude status (running while a window had
/// output in the last 30 s, else idle; the dim line says `tmux -L NAME`). A server whose name
/// is not `valid_server_name` is skipped with its lines; without a clock line there is nothing.
pub fn parse_background(host: &str, output: &str) -> Vec<HostSnapshot> {
    let lines: Vec<&str> = output.lines().collect();
    let first = lines.iter().position(|line| line.starts_with("background ")).unwrap_or(lines.len());
    let clock = lines[..first].iter().enumerate().rev().find_map(|(i, line)| {
        let digits = !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit());
        Some((i, line.parse::<u64>().ok().filter(|_| digits)?))
    });
    let Some((start, now)) = clock else { return Vec::new() };
    let mut out: Vec<HostSnapshot> = Vec::new();
    let mut current: Option<String> = None;
    for line in &lines[start + 1..] {
        if let Some(name) = line.strip_prefix("background ").and_then(|rest| rest.strip_suffix(':')) {
            current = valid_server_name(name).then(|| name.to_string());
            if let Some(name) = &current {
                out.push(HostSnapshot { host: format!("{host}{BACKGROUND_SEP}{name}"), online: true, error: None, sessions: Vec::new() });
            }
            continue;
        }
        let (Some(server), Some(window), Some(snap)) = (&current, window_line(line), out.last_mut()) else { continue };
        if let Some(known) = snap.sessions.iter_mut().find(|s| s.key.name == window.name) {
            // newest window output wins
            let age = now.saturating_sub(window.activity);
            if known.last_output_age_s.is_none_or(|known_age| age < known_age) {
                known.last_output_age_s = Some(age);
                known.activity = Some(window.activity);
                known.status = if age < QUIET_S { Status::Running } else { Status::Idle };
            }
            continue;
        }
        let age = now.saturating_sub(window.activity);
        snap.sessions.push(SessionInfo {
            key: SessionKey { host: snap.host.clone(), name: window.name.to_string() },
            status: if age < QUIET_S { Status::Running } else { Status::Idle },
            reason: None,
            tool: Tool::Other,
            title: Some(format!("tmux -L {server}")),
            context_pct: None,
            attached: window.attached,
            last_output_age_s: Some(age),
            pid: None,
            session_id: None,
            pane: None,
            activity: Some(window.activity),
            last_reply: None,
        });
    }
    out
}
```

`tmux.rs` `attach_argv` and `attach_text`:

```rust
/// Attaches to the session, for a PTY: `tmux -u attach -t =NAME` here, `ssh -t HOST "tmux -u
/// attach -t '=NAME'"` elsewhere; a session of another tmux server (`HOST#SERVER`) with
/// `-L SERVER`. `-u`: ssh without a login often has no UTF-8 locale, and tmux then draws `_`
/// and `lqqk`. ssh may prompt here (it has the terminal), so no `BatchMode`.
pub fn attach_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}", key.name);
    let (machine, server) = crate::listing::split_host(&key.host);
    if machine == local_name {
        let mut argv = strings(&["tmux", "-u"]);
        if let Some(server) = server {
            argv.extend(strings(&["-L", server]));
        }
        argv.extend(strings(&["attach", "-t", &target]));
        argv
    } else {
        let socket = server.map_or(String::new(), |server| format!(" -L {}", quote(server)));
        let mut argv = ssh_base(machine, &["-t"]);
        argv.push(format!("tmux -u{socket} attach -t {}", quote(&target)));
        argv
    }
}

/// The attach command as the user would type it, safe to paste into a shell: what tmls's Copy
/// button copies, byte for byte (quoted only where a shell needs it).
pub fn attach_text(key: &SessionKey, local_name: &str) -> String {
    let (machine, server) = crate::listing::split_host(&key.host);
    let mut tmux = strings(&["tmux", "-u"]);
    if let Some(server) = server {
        tmux.extend(strings(&["-L", server]));
    }
    tmux.extend(strings(&["attach", "-t", &format!("={}", key.name)]));
    if machine == local_name {
        shlex_join(&tmux)
    } else {
        shlex_join(&["ssh", "-t", machine, &shlex_join(&tmux)])
    }
}
```

After `sign_in_argv`:

```rust
/// Lists the sessions of every other tmux server of the user (tmls `hosts.SOCKETS`): the
/// host's clock, then for each socket in `${TMUX_TMPDIR:-/tmp}/tmux-UID` but `default` whose
/// name is `[A-Za-z0-9_.-]+`, a `background NAME:` line and that server's windows as the
/// listing prints them. Read by `listing::parse_background`.
pub const SERVER_SOCKETS_SCRIPT: &str = r#"date +%s
d="${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"
for s in "$d"/*; do
  [ -S "$s" ] || continue
  n=${s##*/}
  [ "$n" = default ] && continue
  case $n in *[!A-Za-z0-9_.-]*) continue ;; esac
  echo "background $n:"
  tmux -L "$n" -u list-windows -a -F '#{session_name}:#{session_windows}:#{session_attached}:#{window_activity}' 2>/dev/null
done
true"#;

/// [`SERVER_SOCKETS_SCRIPT`] on `host`.
pub fn server_sockets_argv(host: &str, local_name: &str) -> Vec<String> {
    script_argv(host, SERVER_SOCKETS_SCRIPT.to_string(), local_name)
}
```

- [ ] **Step 4: Run the client-core tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core`
Expected: all pass.

- [ ] **Step 5: Failing desktop tests.** In `crates/desktop/tests/sidebar.rs`, `shape` (`:104-116`) and `kinds` (`:764-770`) each gain the arm:

```rust
            Row::Background { host, count, open } => {
                format!("[{host} background {count}{}]", if *open { " open" } else { "" })
            }
```

and append:

```rust
fn bg(host: &str, server: &str, name: &str) -> SessionInfo {
    session(&format!("{host}#{server}"), name, Status::Idle)
}

#[test]
fn other_tmux_servers_are_a_collapsed_row_under_their_host() {
    let mut s = sidebar(&[LOCAL, "nas"]);
    s.apply(idle(LOCAL, &["work"]));
    s.apply(idle("nas", &["media"]));
    let two = || vec![bg("nas", "web", "site"), bg("nas", "jobs", "cron")];
    assert!(s.apply_background("nas", two()));
    assert_eq!(shape(&s), ["[laptop]", "work", "-", "[nas]", "media", "[nas background 2]"]);
    assert!(!s.apply_background("nas", two()), "the same again changes nothing");
    assert!(!s.background_open());
    s.toggle_background();
    assert_eq!(
        shape(&s),
        ["[laptop]", "work", "-", "[nas]", "media", "[nas background 2 open]", "site", "cron"]
    );
    // A click on the row toggles; its sessions attach, and have no menu and no pin (R4).
    let row = s
        .rows()
        .iter()
        .position(|row| matches!(row, Row::Background { .. }))
        .unwrap();
    assert_eq!(s.click(row), Some(Action::ToggleBackground));
    assert_eq!(s.click(row + 1), Some(Action::Attach(key("nas#web", "site"))));
    assert_eq!(s.menu(row + 1), None);
    assert!(!s.toggle_pin());
    // The host's own listing leaves the row after its sessions.
    s.apply(idle("nas", &["media", "backup"]));
    assert_eq!(
        shape(&s),
        ["[laptop]", "work", "-", "[nas]", "media", "backup", "[nas background 2 open]", "site", "cron"]
    );
    assert_eq!(selected(&s), at("nas#web", "site"));
    // Hidden with the cursor on one of them: the cursor goes to the nearest session.
    s.toggle_background();
    assert_eq!(selected(&s), at("nas", "backup"));
    // None left: no row.
    assert!(s.apply_background("nas", Vec::new()));
    assert_eq!(shape(&s), ["[laptop]", "work", "-", "[nas]", "media", "backup"]);
}

#[test]
fn this_machines_background_row_comes_after_its_sessions_outside_tmux() {
    let mut s = sidebar(&[LOCAL, "nas"]);
    s.apply(idle(LOCAL, &["work"]));
    assert!(s.apply_background(LOCAL, vec![bg(LOCAL, "web", "site")]));
    let mut with_outside = idle(LOCAL, &["work"]);
    with_outside.sessions.push(session(KITTY, "notes", Status::Idle));
    s.apply(with_outside);
    assert_eq!(
        kinds(&s),
        ["[laptop]", "work", "[kitty]", "notes", "[laptop background 1]", "-", "[nas]"]
    );
}
```

Append to `crates/desktop/tests/sidebar_draw.rs` (import `background_spans`):

```rust
#[test]
fn the_background_row_says_how_many_and_whether_it_is_open() {
    let theme = Theme::default();
    let closed = background_spans(3, false, SIDEBAR_COLS, &theme);
    assert_eq!((closed[0].text.as_ref(), closed[0].col, closed[0].colour), ("background ▸ · 3", 1, theme.dim_fg));
    assert_eq!(background_spans(3, true, SIDEBAR_COLS, &theme)[0].text, "background ▾ · 3");
    assert_within(&closed);
}
```

In `crates/desktop/tests/keys.rs`: in `the_help_names_every_key_of_the_list_and_every_alt_shift_letter` add `'b'` to the list's characters, and append:

```rust
#[test]
fn b_in_the_list_shows_or_hides_the_other_tmux_servers() {
    assert_eq!(list_key(&ch('b', NONE)), Some(ListKey::Background));
    assert_eq!(list_key(&repeat(&ch('b', NONE))), None);
    assert_eq!(list_key(&ch('B', SHIFT)), None);
}
```

- [ ] **Step 6: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test sidebar --test sidebar_draw --test keys`
Expected: compile errors (`Row::Background`, `apply_background`, `background_spans`, `ListKey::Background`).

- [ ] **Step 7: Implement.** `sidebar.rs`: imports `use std::collections::{BTreeMap, BTreeSet, HashMap};` and `use client_core::listing::split_host;`. Module comment gains a paragraph: "**Other tmux servers** (`tmux -L NAME`, spec G5): under each host's sessions (and under this machine's sessions outside tmux) a row `background ▸ · N` when the host has any; `b` or a click on it shows them all (`toggle_background`). Their sessions attach and do nothing else: no menu, no pin (R4)."

`Row` gains:

```rust
    /// The sessions of `host`'s other tmux servers: `count` of them, listed under it while
    /// `open`.
    Background {
        host: String,
        count: usize,
        open: bool,
    },
```

`Action` gains `/// Show or hide the other tmux servers' sessions.\n ToggleBackground,`. `Sidebar` gains:

```rust
    /// Each host's sessions on its other tmux servers, as the last scan found them.
    background: BTreeMap<String, Vec<SessionInfo>>,
    /// They are listed under their `background` row (`b`).
    background_open: bool,
```

(`BTreeMap::new()`, `false` in `new`). `click` gains the arm `Row::Background { .. } => Some(Action::ToggleBackground),` before `Row::Host { .. } | Row::Gap => None`. `menu`'s `if key.host == KITTY {` becomes `if key.host == KITTY || split_host(&key.host).1.is_some() {`; the same condition replaces `if key.host == KITTY {` in `toggle_pin` (with the comment "Nothing to attach, or a server tmls-rs only attaches (R4): in no ring to walk."). New methods, after `renamed`:

```rust
    /// `host`'s sessions on its other tmux servers (`listing::parse_background`, every server's
    /// together): what its `background` row counts and, while open, lists. None: no row.
    /// Returns whether the list changed (output ages alone change nothing). The cursor stays
    /// on its session; when that is hidden or gone it goes to the nearest one.
    pub fn apply_background(&mut self, host: &str, sessions: Vec<SessionInfo>) -> bool {
        let same = |a: &[SessionInfo], b: &[SessionInfo]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| (&x.key, x.status, x.attached) == (&y.key, y.status, y.attached))
        };
        let known = self.background.get(host).map(Vec::as_slice).unwrap_or_default();
        if same(known, &sessions) {
            if let Some(known) = self.background.get_mut(host) {
                *known = sessions;
            }
            return false;
        }
        if sessions.is_empty() {
            self.background.remove(host);
        } else {
            self.background.insert(host.to_string(), sessions);
        }
        self.place_background(host);
        true
    }

    /// `b`: shows or hides every host's other tmux servers' sessions.
    pub fn toggle_background(&mut self) {
        self.background_open = !self.background_open;
        let hosts: Vec<String> = self.background.keys().cloned().collect();
        for host in hosts {
            self.place_background(&host);
        }
    }

    /// The other tmux servers' sessions are listed.
    pub fn background_open(&self) -> bool {
        self.background_open
    }

    /// Puts `host`'s `background` row (and, while open, its sessions) where it belongs, or
    /// takes it away.
    fn place_background(&mut self, host: &str) {
        let selected = self.selected().cloned();
        let at = self.cursor;
        if let Some(old) = self.background_rows(host) {
            self.rows.drain(old);
        }
        let sessions = self.background.get(host).cloned().unwrap_or_default();
        if !sessions.is_empty()
            && let Some(place) = self.background_place(host)
        {
            let mut rows = vec![Row::Background {
                host: host.to_string(),
                count: sessions.len(),
                open: self.background_open,
            }];
            if self.background_open {
                rows.extend(sessions.into_iter().map(Row::Session));
            }
            self.rows.splice(place..place, rows);
        }
        self.cursor = match selected {
            Some(key) => self
                .position(&key)
                .or_else(|| at.and_then(|at| self.nearest_session(at))),
            None => at.and_then(|at| self.nearest_session(at)),
        };
    }

    /// `host`'s `background` row and the sessions under it.
    fn background_rows(&self, host: &str) -> Option<Range<usize>> {
        let start = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Background { host: h, .. } if h == host))?;
        let sessions = self.rows[start + 1..]
            .iter()
            .take_while(|row| row.key().is_some())
            .count();
        Some(start..start + 1 + sessions)
    }

    /// Where `host`'s `background` row goes: after its sessions, and for this machine after
    /// its sessions outside tmux.
    fn background_place(&self, host: &str) -> Option<usize> {
        let mut at = self.section(host)?.end;
        if host == self.local_name
            && let Some(outside) = self.section(KITTY)
            && outside.start == at
        {
            at = outside.end;
        }
        Some(at)
    }
```

(`HashMap` stays imported for `listed`; `BTreeMap` is new.) `draw.rs`, after `rule_spans`:

```rust
/// The row of a host's other tmux servers: `background ▸ · N`, `▾` while they are listed.
pub fn background_spans(count: usize, open: bool, cols: usize, theme: &Theme) -> Vec<Span<'static>> {
    let text = format!("background {} · {count}", if open { '▾' } else { '▸' });
    vec![Span::new(0, 1, cols.saturating_sub(2), text).colour(theme.dim_fg)]
}
```

and in `sidebar()`'s `match row` a new arm:

```rust
            Row::Background { count, open, .. } => {
                for span in background_spans(*count, *open, l.list_cols, theme) {
                    p.span((r.x, r.y), &span);
                }
            }
```

`keys.rs`: `ListKey` gains `/// Show or hide the other tmux servers' sessions.\n Background,`; `list_key`: `Key::Char('b') if !ev.repeat => Some(ListKey::Background),`; `HELP` List group after `("a", "alerts")`: `("b", "show / hide background tmux"),`; module comment's list line gains "b the other tmux servers' sessions".

- [ ] **Step 8: Run the desktop unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test sidebar --test sidebar_draw --test keys`
Expected: pass.

- [ ] **Step 9: Wire `app.rs`.** Imports: `use client_core::SessionInfo;` (if not there) and `use client_core::listing::{self as listing_rs, split_host};` — or call them by full path; the code below uses full paths. `UserEvent` gains `/// A host's sessions on its other tmux servers.\n Background(String, Vec<SessionInfo>),`; `user_event`:

```rust
            UserEvent::Background(host, sessions) => {
                if running.sidebar.apply_background(&host, sessions) {
                    running.invalidate();
                }
            }
```

`tick` (Task 2) gains, after the game-mode part:

```rust
        // The other tmux servers of every host that answers (R4: listed, attach only); not the
        // remote ones in game mode (R1).
        let online: Vec<String> = self
            .sidebar
            .rows()
            .iter()
            .filter_map(|row| match row {
                sidebar::Row::Host {
                    host, online: true, ..
                } if host != KITTY => Some(host.clone()),
                _ => None,
            })
            .collect();
        for host in online {
            if self.game && host != self.cfg.local_name {
                continue;
            }
            let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
            std::thread::spawn(move || {
                let argv = client_core::tmux::server_sockets_argv(&host, &local);
                let found = client_core::tmux::run(&argv, POLL_TIMEOUT)
                    .map(|out| client_core::listing::parse_background(&host, &out))
                    .unwrap_or_default();
                let sessions = found.into_iter().flat_map(|snap| snap.sessions).collect();
                let _ = proxy.send_event(UserEvent::Background(host, sessions));
            });
        }
```

`snapshot`, before `self.invalidate();` at its end: `if !online { self.sidebar.apply_background(&host, Vec::new()); }`. `list_key`: `ListKey::Background => self.sidebar.toggle_background(),`; `ListKey::Menu`'s filter becomes `.filter(|key| key.host != KITTY && client_core::listing::split_host(&key.host).1.is_none())`. `act`: `sidebar::Action::ToggleBackground => self.sidebar.toggle_background(),`. `foot_button`'s `Button::New` host mapping: `false => client_core::listing::split_host(&key.host).0.to_string(),` (a new session from a background row goes to its machine's default server). Module comment, Ticker paragraph: "… and `server_sockets_argv` for each online host (remote ones not in game mode) → `UserEvent::Background` → `Sidebar::apply_background`."

- [ ] **Step 10: e2e, in the third run.** `cleanup()`: `TMUX_TMPDIR="$SCRATCH/tmux3" tmux -L tmlsrs-bg kill-server 2>/dev/null`. Right after the `chmod +x "$OUT/bin3/…"` line:

```bash
BG='tmlsrs-test-bgjob'
TMUX_TMPDIR="$THIRD_TMUX" tmux -L tmlsrs-bg new-session -d -s "$BG" -x 100 -y 30
bg_attached() { TMUX_TMPDIR="$THIRD_TMUX" tmux -L tmlsrs-bg list-clients -F '#{session_name}' 2>/dev/null | grep -qxF -- "$BG"; }
```

Task 3's sign-in click moves a line down — line 1 is now `background ▸ · 1`: change its comment to `# The list: line 0 this machine's header, line 1 background ▸ · 1, line 2 the gap, line 3 SIGNHOST · ⚠ sign in (y 82).` and `click 100 65` to `click 100 82`. Above `# --- third run: more checks above this line ---`:

```bash
# This machine has a second tmux server (-L tmlsrs-bg) with one session: `background ▸ · 1`.
# b lists it (two lines: its name, `tmux -L tmlsrs-bg`); j puts the cursor on it, Enter attaches.
check "screenshot 46-background-closed" shot 46-background-closed
BG_CLOSED=$(list_picture)
bg_listed() { [ "$(list_picture)" != "$BG_CLOSED" ]; }
keys b
if [ "$BG_CLOSED" != none ] && wait_for 3 bg_listed; then
  pass "b shows the other tmux server's session under background"
else
  fail "b shows the other tmux server's session under background" "the list's pixels did not change"
fi
check "screenshot 47-background-open" shot 47-background-open
keys -k j
key_enter
if wait_for 8 bg_attached; then
  pass "Enter on a background session attaches with tmux -L tmlsrs-bg"
else
  fail "Enter on a background session attaches with tmux -L tmlsrs-bg" "stderr: $(tail -2 "$OUT/app3.stderr" | tr '\n' ' ')"
fi
```

- [ ] **Step 11: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; client-core tests; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/t6-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/round2/t6-e2e/46-background-closed.png` (`background ▸ · 1` under this machine's header) and `/data/scratch/tmls-rs-v0/round2/t6-e2e/47-background-open.png` (`▾`, `tmlsrs-test-bgjob`, dim `tmux -L tmlsrs-bg`).

- [ ] **Step 12: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): sessions of other tmux servers in a background section that b opens"
```

---

### Task 7: G3 — quick select

**Files:**
- Create: `crates/desktop/src/quickselect.rs`, `crates/desktop/src/select.rs`, `crates/desktop/tests/quickselect.rs`, `crates/desktop/tests/select.rs`
- Modify: `crates/desktop/src/lib.rs` (`pub mod quickselect; pub mod select;` and their lines in the crate comment)
- Modify: `crates/desktop/src/links.rs` (after `link_at`: `row_links`, `frame_rows`)
- Modify: `crates/desktop/src/keys.rs` (`Global::QuickSelect`, `binding`, `HELP`)
- Modify: `crates/desktop/src/theme.rs` (`LABEL_BG_RGB`, `LABEL_FG_RGB`), `crates/desktop/src/draw.rs` (`View.labels`, `select_labels`, `Chrome::update`)
- Modify: `crates/desktop/src/app.rs` (`Modal::Select`, `build_view`, `form_key`, `modal_click`, `global`; new `quick_select`, `picked`)
- Modify: `tooling/smoke/e2e.sh` (top variables, `cleanup`, an `xdg-open` stand-in, block before `# --- quit`)
- Test: `crates/desktop/tests/quickselect.rs`, `crates/desktop/tests/select.rs`, `crates/desktop/tests/keys.rs`

**Interfaces:**
- Consumes: `links::links` (private, `links.rs:~300`), `row_of` (private), `Link`, `FileLink`, `links::open` (exist); `Running::open_file`, `set_clipboard` (exist).
- Produces:
  - `links::row_links(row_text: &str) -> Vec<(usize, usize, Link)>`, `links::frame_rows(frame: &Frame) -> Vec<String>`
  - `quickselect::{MAX_TARGETS: usize = 26, Kind { Url, File, Hash }, Target { pub row: usize, pub col: usize, pub len: usize, pub kind: Kind, pub text: String, pub link: Option<Link> }, label(i: usize) -> char, targets(rows: &[String]) -> Vec<Target>}`
  - `select::{Label { pub row: usize, pub col: usize, pub letter: char }, Picked { Pending, Cancelled, Copy(String), Open(Link) }, Select}`; `Select::new(targets: Vec<Target>) -> Option<Select>`, `Select::labels(&self) -> Vec<Label>`, `Select::key(&mut self, ev: &KeyEvent) -> Picked`
  - `keys::Global::QuickSelect`; `theme::{LABEL_BG_RGB, LABEL_FG_RGB}`; `View.labels: Vec<select::Label>`
- Departs from the spec: (a) `quickselect.rs` is in `desktop`, not `client-core`: its URL and path rules must be Ctrl+click's (`links.rs`, a desktop module), and copying them into client-core would make two sets to keep alike. (b) `a` is the target nearest the bottom, then upwards (right to left within a row): R2's "nearest the bottom" read as the labels' order too. (c) A git hash has nothing to open: Shift and its letter copies it as well (tmls's kitty hints copy hashes). (d) Keys that are no label (another letter, a digit, Enter) leave the overlay open; Esc or a click closes it.

- [ ] **Step 1: Failing tests.** `crates/desktop/tests/quickselect.rs`:

```rust
//! Quick select's targets: every URL, path and git hash of the screen, the 26 nearest the
//! bottom, labelled from there (tmls `quickselect.py`, R2).

use desktop::links::{FileLink, Link};
use desktop::quickselect::{Kind, MAX_TARGETS, label, targets};

fn rows(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| line.to_string()).collect()
}

#[test]
fn a_url_a_path_and_a_hash_get_labels_from_the_bottom() {
    let screen = rows(&["see https://example.com/a?b=1, then", "edit src/main.rs:12 and commit 1a2b3c4d", ""]);
    let found = targets(&screen);
    let got: Vec<(usize, usize, usize, Kind, &str)> =
        found.iter().map(|t| (t.row, t.col, t.len, t.kind, t.text.as_str())).collect();
    assert_eq!(
        got,
        [
            (1, 31, 8, Kind::Hash, "1a2b3c4d"),
            (1, 5, 14, Kind::File, "src/main.rs:12"),
            (0, 4, 25, Kind::Url, "https://example.com/a?b=1"),
        ]
    );
    assert_eq!(found[0].link, None);
    assert_eq!(found[1].link, Some(Link::File(FileLink { path: "src/main.rs".to_string(), line: Some(12) })));
    assert_eq!(found[2].link, Some(Link::Url("https://example.com/a?b=1".to_string())));
}

#[test]
fn targets_keep_the_26_nearest_the_bottom_and_skip_hashes_in_links() {
    // 30 lines with a path each: the last 26, the bottom one first.
    let many: Vec<String> = (0..30).map(|n| format!("file{n}.txt")).collect();
    let found = targets(&many);
    assert_eq!(found.len(), MAX_TARGETS);
    assert_eq!((found[0].text.as_str(), found[0].row), ("file29.txt", 29));
    assert_eq!(found[25].text, "file4.txt");
    assert_eq!((label(0), label(25)), ('a', 'z'));
    // A hash inside a URL or a path is part of it, no target of its own.
    let screen = rows(&["https://github.com/o/r/commit/0123456789abcdef x/deadbeef1/y"]);
    let kinds: Vec<Kind> = targets(&screen).iter().map(|t| t.kind).collect();
    assert_eq!(kinds, [Kind::File, Kind::Url]);
    // A wide character is two cells: the frame gives its second as a space, so the column is
    // the character's index and the label lands on the target's first cell.
    assert_eq!(targets(&rows(&["日 本 ./a.sh"]))[0].col, 4);
    // Seven hex digits at least, forty at most, nothing hex either side.
    let hashes: Vec<String> = targets(&rows(&["abc1234 abc123 x1234567 ", &"f".repeat(41)])).iter().map(|t| t.text.clone()).collect();
    assert_eq!(hashes, ["abc1234"]);
    // Nothing to select.
    assert!(targets(&rows(&["", "   ", "plain words only"])).is_empty());
}
```

`crates/desktop/tests/select.rs`:

```rust
//! The quick-select overlay: a letter copies, Shift and the letter opens, Esc closes.

use desktop::links::{FileLink, Link};
use desktop::quickselect::targets;
use desktop::select::{Label, Picked, Select};
use s2_core::{Key, KeyEvent, Mods, Named};

fn press(key: Key, shift: bool) -> KeyEvent {
    KeyEvent {
        key,
        mods: Mods {
            shift,
            ..Mods::default()
        },
        text: None,
        repeat: false,
        pressed: true,
    }
}

#[test]
fn a_letter_copies_shift_and_the_letter_opens_esc_closes() {
    let rows = vec![
        "open https://example.com/x now".to_string(),
        "then src/lib.rs:7 and 1a2b3c4d".to_string(),
    ];
    let mut select = Select::new(targets(&rows)).expect("three targets");
    assert_eq!(
        select.labels(),
        [
            Label { row: 1, col: 22, letter: 'a' },
            Label { row: 1, col: 5, letter: 'b' },
            Label { row: 0, col: 5, letter: 'c' },
        ]
    );
    assert_eq!(select.key(&press(Key::Char('a'), false)), Picked::Copy("1a2b3c4d".to_string()));
    assert_eq!(
        select.key(&press(Key::Char('A'), true)),
        Picked::Copy("1a2b3c4d".to_string()),
        "a hash has nothing to open: Shift copies it too"
    );
    assert_eq!(select.key(&press(Key::Char('b'), false)), Picked::Copy("src/lib.rs:7".to_string()));
    assert_eq!(
        select.key(&press(Key::Char('B'), true)),
        Picked::Open(Link::File(FileLink { path: "src/lib.rs".to_string(), line: Some(7) }))
    );
    assert_eq!(
        select.key(&press(Key::Char('c'), true)),
        Picked::Open(Link::Url("https://example.com/x".to_string()))
    );
    // A letter with no target, a digit, Enter, a held key, a release: nothing.
    let held = KeyEvent { repeat: true, ..press(Key::Char('a'), false) };
    let released = KeyEvent { pressed: false, ..press(Key::Char('a'), false) };
    for ev in [press(Key::Char('d'), false), press(Key::Char('1'), false), press(Key::Named(Named::Enter), false), held, released] {
        assert_eq!(select.key(&ev), Picked::Pending);
    }
    assert_eq!(select.key(&press(Key::Named(Named::Escape), false)), Picked::Cancelled);
    assert_eq!(Select::new(Vec::new()), None, "nothing to label: no overlay");
}
```

In `crates/desktop/tests/keys.rs`: in `the_help_names_every_key_of_the_list_and_every_alt_shift_letter` add `'s'` to `['n', 'w', 'x', 'f']`, and append:

```rust
#[test]
fn alt_shift_s_is_quick_select_outside_forms() {
    assert_eq!(global(&ch('s', ALT_SHIFT), Focus::Terminal), Some(Global::QuickSelect));
    assert_eq!(global(&ch('S', ALT_SHIFT), Focus::Sidebar), Some(Global::QuickSelect));
    assert_eq!(global(&ch('s', ALT_SHIFT), Focus::Form), None);
    assert_eq!(global(&repeat(&ch('s', ALT_SHIFT)), Focus::Terminal), None);
}
```

- [ ] **Step 2: Run them.**
Run: `tooling/docker/build.sh test -p desktop --test quickselect --test select --test keys`
Expected: compile errors (modules `quickselect`, `select`; `Global::QuickSelect`).

- [ ] **Step 3: Implement.** `links.rs`, after `link_at`:

```rust
/// The links of one row of text, each with the columns it covers (end exclusive), in the
/// order they were found: what quick select labels.
pub fn row_links(row_text: &str) -> Vec<(usize, usize, Link)> {
    let chars: Vec<char> = row_text.chars().collect();
    links(&chars)
}

/// Every row of `frame` as text, one character per cell (the second cell of a wide character
/// is a space), so character N of a row is its column N.
pub fn frame_rows(frame: &Frame) -> Vec<String> {
    (0..frame.rows)
        .filter_map(|row| row_of(frame, (0, row)).map(|(text, _)| text))
        .collect()
}
```

`crates/desktop/src/quickselect.rs`:

```rust
//! Quick select's targets (tmls `quickselect.py`, spec G3): every URL and path Ctrl+click
//! would open (`links::row_links`) and every git hash (7 to 40 hex digits with no hex digit
//! either side, not inside a link) of the front tab's screen. At most 26, those nearest the
//! bottom (R2); the first returned is the bottom-most and gets `a`.

use crate::links::{self, Link};

/// The most targets labelled at once: `a` to `z`.
pub const MAX_TARGETS: usize = 26;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Url,
    File,
    Hash,
}

/// One labelled thing on the screen: where it starts (row, column), how many cells it covers,
/// what kind it is, its text as shown (what a letter copies) and what Shift and the letter
/// opens (`None` for a hash).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub row: usize,
    pub col: usize,
    pub len: usize,
    pub kind: Kind,
    pub text: String,
    pub link: Option<Link>,
}

/// The letter of the `i`th target.
pub fn label(i: usize) -> char {
    (b'a' + (i % MAX_TARGETS) as u8) as char
}

/// tmls's `HASH` in a row: 7 to 40 hex digits with no hex digit either side.
fn hashes(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_hexdigit() {
            i += 1;
            continue;
        }
        let end = i + chars[i..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
        if (7..=40).contains(&(end - i)) {
            out.push((i, end));
        }
        i = end;
    }
    out
}

/// The targets of `rows` (one string per screen row, one character per cell), the bottom-most
/// first, at most `MAX_TARGETS`.
pub fn targets(rows: &[String]) -> Vec<Target> {
    let mut all = Vec::new();
    for (row, text) in rows.iter().enumerate() {
        let chars: Vec<char> = text.chars().collect();
        let mut found: Vec<(usize, usize, Kind, Option<Link>)> = links::row_links(text)
            .into_iter()
            .map(|(start, end, link)| {
                let kind = match link {
                    Link::Url(_) => Kind::Url,
                    Link::File(_) => Kind::File,
                };
                (start, end, kind, Some(link))
            })
            .collect();
        for (start, end) in hashes(&chars) {
            if !found.iter().any(|(from, to, ..)| start < *to && end > *from) {
                found.push((start, end, Kind::Hash, None));
            }
        }
        found.sort_by_key(|(start, ..)| *start);
        for (start, end, kind, link) in found {
            let text = chars[start..end].iter().collect();
            all.push(Target { row, col: start, len: end - start, kind, text, link });
        }
    }
    all.reverse();
    all.truncate(MAX_TARGETS);
    all
}
```

`crates/desktop/src/select.rs`:

```rust
//! Quick select (Alt+Shift+S, spec G3): a letter over each target of the front tab's screen
//! (`quickselect::targets`). The letter copies the target; Shift and the letter opens it as a
//! Ctrl+click would (a URL in the browser, a file in the panel or the desktop's app); a hash
//! is copied either way. Esc or a click closes it. Drawn by `draw` from `View::labels`.

use s2_core::{Key, KeyEvent, Named};

use crate::links::Link;
use crate::quickselect::{Target, label};

/// A letter on the terminal grid: the first cell of its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label {
    pub row: usize,
    pub col: usize,
    pub letter: char,
}

/// What a key did to the overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picked {
    Pending,
    Cancelled,
    /// To the clipboard.
    Copy(String),
    /// Opened as a Ctrl+click opens it.
    Open(Link),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Select {
    targets: Vec<Target>,
}

impl Select {
    /// The overlay for these targets; `None` when there is none.
    pub fn new(targets: Vec<Target>) -> Option<Select> {
        (!targets.is_empty()).then_some(Select { targets })
    }

    /// The letters to draw.
    pub fn labels(&self) -> Vec<Label> {
        self.targets
            .iter()
            .enumerate()
            .map(|(i, target)| Label {
                row: target.row,
                col: target.col,
                letter: label(i),
            })
            .collect()
    }

    pub fn key(&mut self, ev: &KeyEvent) -> Picked {
        if !ev.pressed || ev.repeat {
            return Picked::Pending;
        }
        let plain = !ev.mods.ctrl && !ev.mods.alt && !ev.mods.logo;
        match ev.key {
            Key::Named(Named::Escape) => Picked::Cancelled,
            Key::Char(c) if c.is_ascii_alphabetic() && plain => {
                let index = (c.to_ascii_lowercase() as u8 - b'a') as usize;
                let Some(target) = self.targets.get(index) else {
                    return Picked::Pending;
                };
                let open = ev.mods.shift || c.is_ascii_uppercase();
                match (&target.link, open) {
                    (Some(link), true) => Picked::Open(link.clone()),
                    _ => Picked::Copy(target.text.clone()),
                }
            }
            _ => Picked::Pending,
        }
    }
}
```

`keys.rs`: `Global` gains `/// Letters over the front tab's links, paths and hashes.\n QuickSelect,`; `binding`'s Alt+Shift match: `Key::Char(c) if c.eq_ignore_ascii_case(&'s') => Some(Global::QuickSelect),`. `HELP` Everywhere after `Alt+Shift+X`: `("Alt+Shift+S", "label links; a letter copies"),`; Terminal after `("Ctrl+click", …)`: `("Shift+letter", "quick select: open it"),`. Module comment: "Alt+Shift+S — quick select: a letter over every link, path and hash; the letter copies, Shift and the letter opens".

`theme.rs`:

```rust
/// Quick select's letters: dark on amber, over the terminal.
pub const LABEL_BG_RGB: [u8; 3] = [0xe5, 0xc0, 0x7b];
pub const LABEL_FG_RGB: [u8; 3] = [0x1e, 0x1e, 0x1e];
```

`draw.rs`: import them and `crate::select::Label`; `View` gains `/// Quick select's letters, while it is open.\n pub labels: Vec<Label>,`; after `notice`:

```rust
/// Quick select's letters over the terminal: each on its target's first cell.
fn select_labels(p: &mut Painter, view: &View, l: &Layout) {
    let bold = FontStyle {
        bold: true,
        ..FontStyle::default()
    };
    for label in &view.labels {
        let x = l.terminal.x + label.col as f32 * p.m.width;
        let y = l.terminal.y + label.row as f32 * p.m.height;
        let cell = Rect {
            x,
            y,
            w: p.m.width,
            h: p.m.height,
        };
        p.rect(cell, LABEL_BG_RGB);
        p.styled(&label.letter.to_string(), x, y, 1, LABEL_FG_RGB, bold);
    }
}
```

and in `Chrome::update` after `notice(&mut p, view, layout, theme);`: `select_labels(&mut p, view, layout);`.

- [ ] **Step 4: Run the desktop unit tests.**
Run: `tooling/docker/build.sh test -p desktop --test quickselect --test select --test keys --test links`
Expected: pass.

- [ ] **Step 5: Wire `app.rs`.** Imports: `use crate::links::Link;`, `use crate::quickselect;`, `use crate::select::{self, Picked};`. `Modal` gains `/// Quick select's letters over the terminal.\n Select(select::Select),`. `build_view`: the `panel` closure becomes `self.modal.as_mut().and_then(|(modal, at)| match modal { Modal::Menu(menu) => Some((menu.panel(), *at)), Modal::Form(form) => Some((form.panel(), *at)), Modal::Alerts(alerts) => Some((alerts.panel(), *at)), Modal::Help(panel) => Some((panel.clone(), *at)), Modal::Select(_) => None })`, and the `View` gains `labels: match &self.modal { Some((Modal::Select(select), _)) => select.labels(), _ => Vec::new() },`. `form_key`'s match gains:

```rust
            Some((Modal::Select(select), _)) => {
                let picked = select.key(ev);
                self.picked(picked);
            }
```

`modal_click`'s match gains `Modal::Select(_) => self.close_modal(),`. `global` gains `Global::QuickSelect => self.quick_select(),`. New methods:

```rust
    /// Alt+Shift+S: letters over the links, paths and hashes of the front tab's screen.
    fn quick_select(&mut self) {
        let Some(index) = self.tabs.active() else {
            return self.say(Ok("no tab: nothing to select".to_string()));
        };
        let rows = self
            .tabs
            .session(index)
            .frame()
            .map(links::frame_rows)
            .unwrap_or_default();
        match select::Select::new(quickselect::targets(&rows)) {
            Some(select) => self.open_modal(Modal::Select(select), None),
            None => self.say(Ok("nothing to select: no link, path or hash on the screen".to_string())),
        }
    }

    /// A key of quick select: copy, open (as a Ctrl+click), or close.
    fn picked(&mut self, picked: Picked) {
        match picked {
            Picked::Pending => {}
            Picked::Cancelled => self.close_modal(),
            Picked::Copy(text) => {
                self.close_modal();
                self.set_clipboard(text);
            }
            Picked::Open(Link::Url(url)) => {
                self.close_modal();
                let said = links::open(&url).map(|()| format!("opened {url}"));
                self.say(said);
            }
            Picked::Open(Link::File(file)) => {
                self.close_modal();
                self.open_file(file);
            }
        }
    }
```

- [ ] **Step 6: e2e block.** Top: `QS='tmlsrs-test-00qs'`; `cleanup()`: `tmux kill-session -t "=$QS" 2>/dev/null`. Next to the `notify-send` stand-in (before `export PATH=…`):

```bash
# A stand-in for xdg-open: one line per call, its arguments ending in "|".
cat >"$OUT/bin/xdg-open" <<EOF
#!/bin/sh
{ printf '%s|' "\$@"; echo; } >>"$OUT/xdg-open.log"
EOF
chmod +x "$OUT/bin/xdg-open"
: >"$OUT/xdg-open.log"
```

Before `# --- quit` (after Task 5's block):

```bash
# --- quick select (G3) ---------------------------------------------------------------------------
# $QS shows one URL and no prompt; it sorts first. Its tab in front: Alt+Shift+S puts `a` on it.
tmux new-session -d -s "$QS" -x 100 -y 30 "printf 'see https://example.com/qs-%s here\n' $NONCE; sleep 600"
sleep 6
key_focus_list
top_of_list
key_enter
check "the tab of $QS opens" wait_for 8 attached_to "$QS"
key_quick_select() { keys -M alt -M shift -k s -m shift -m alt; }
key_quick_select
check "screenshot 48-quick-select (a over the URL)" shot 48-quick-select
timeout 5 wl-copy "not-it-$NONCE"
keys a
if wait_for 5 clipboard_has "https://example.com/qs-$NONCE"; then
  pass "Alt+Shift+S then a copies the URL (wl-paste)"
else
  fail "Alt+Shift+S then a copies the URL (wl-paste)" "clipboard: $(clipboard | head -c 80)"
fi
key_quick_select
keys -M shift -k a -m shift
if wait_for 5 grep -qxF -- "https://example.com/qs-$NONCE|" "$OUT/xdg-open.log"; then
  pass "Alt+Shift+S then Shift+a opens the URL (xdg-open)"
else
  fail "Alt+Shift+S then Shift+a opens the URL (xdg-open)" "log: $(tr '\n' ' ' <"$OUT/xdg-open.log")"
fi
key_close_tab
tmux kill-session -t "=$QS"
```

- [ ] **Step 7: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; client-core tests; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/t7-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/round2/t7-e2e/48-quick-select.png`: an amber `a` on the `h` of `https`.

- [ ] **Step 8: Commit.**

```bash
git add crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): Alt+Shift+S puts a letter on every link, path and hash; the letter copies, Shift opens"
```

---

### Task 8: G4 — image paste

**Files:**
- Create: `crates/client-core/src/imagepaste.rs`, `crates/client-core/tests/imagepaste.rs`
- Modify: `crates/client-core/src/lib.rs` (`pub mod imagepaste;`)
- Modify: `crates/client-core/src/tmux.rs` (round 1 Task 7's `send_text_script` split into `paste_text_script` + Enter; add `paste_text_argv`)
- Modify: `crates/desktop/src/keys.rs` (`Global::PasteImage`, `binding`, `HELP`)
- Modify: `crates/desktop/src/app.rs` (`global`; new `paste_image`)
- Modify: `tooling/smoke/e2e.sh` (top variables, `cleanup`, block before `# --- quit`)
- Test: `crates/client-core/tests/imagepaste.rs`, `crates/client-core/tests/tmux.rs`, `crates/desktop/tests/keys.rs`

**Interfaces:**
- Consumes: `tmux::send_text_script`, `send_text_argv`, `buffer_name`, `pane_target`, `run_with_stdin` (from round 1 Tasks 4, 5, 7); `tmux::run_bytes`, `ssh`, `quote` (exist).
- Produces:
  - `tmux::paste_text_script(target: &str, buffer: &str) -> String`, `tmux::paste_text_argv(key: &SessionKey, pane: Option<&str>, buffer: &str, local_name: &str) -> Vec<String>`
  - `client_core::imagepaste::{HOST_DIR: &str = ".cache/tmls/images", STAGING: &str = ".cache/tmls-rs/paste", NO_IMAGE: &str = "clipboard has no image", TIMEOUT: Duration = 10 s, Reader<'a>, Feeder<'a>, Paste<'a> { key: &'a SessionKey, local_name: &'a str, staging: &'a Path, home: &'a Path, now: SystemTime }, pick_type(list: &str) -> Option<&'static str>, extension(mime: &str) -> &'static str, stamp_name(now: SystemTime, ext: &str) -> String, store_argv(host: &str, name: &str, local_name: &str) -> Vec<String>, paste(p: &Paste, read: Reader, feed: Feeder) -> Result<String, String>}`
  - `keys::Global::PasteImage`
- Departs from the spec: (a) the copy to another host is `ssh … 'mkdir -p "$HOME/.cache/tmls/images" && cat > …'` with the bytes on stdin (`store_argv`, through `ssh_base`), as tmls's `image_paste.store` does, not `scp_argv`: scp cannot make the folder, and this reuses round 1's shared connection and `run_with_stdin`. (b) The answer comes back as `UserEvent::Said`, not a new `ImagePasted`: the status line is all it changes. (c) The staged file is removed once it reached a remote host (on this machine it is moved), so `~/.cache/tmls-rs/paste/` holds only images that did not get there. (d) The path is pasted into the session's current pane (`=NAME:`), the one the user is looking at, not Claude's pane.

- [ ] **Step 1: Failing tests.** Append to `crates/client-core/tests/tmux.rs` (import `paste_text_argv, paste_text_script`):

```rust
#[test]
fn paste_text_is_send_text_without_the_enter() {
    let paste = paste_text_script("%7", "tmls-0123abcd");
    assert_eq!(
        paste,
        "tmux load-buffer -b 'tmls-0123abcd' - && { tmux paste-buffer -p -d -b 'tmls-0123abcd' -t '%7' || { tmux delete-buffer -b 'tmls-0123abcd'; false; }; }"
    );
    assert_eq!(send_text_script("%7", "tmls-0123abcd"), format!("{paste} && sleep 0.2 && tmux send-keys -t '%7' Enter"));
    let here = paste_text_argv(&key(LOCAL, "w"), None, "tmls-0123abcd", LOCAL);
    assert_eq!(here, v(&["sh", "-c", &paste_text_script("=w:", "tmls-0123abcd")]));
    let there = paste_text_argv(&key("ph", "a b"), None, "tmls-0123abcd", LOCAL);
    assert_eq!(remote(&there, &SSH), paste_text_script("=a b:", "tmls-0123abcd"));
}
```

`crates/client-core/tests/imagepaste.rs`:

```rust
//! Image paste: the clipboard's image to the session's host, its path typed into the session.
//! The clipboard and every command are stand-ins; files go under cargo's target tmp dir.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use client_core::imagepaste::{Paste, paste, pick_type, stamp_name, store_argv};
use client_core::listing::SessionKey;

const LOCAL: &str = "archbox";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot-really-a-picture";
const NAME: &str = "20261012-035502-000000.png";

fn scratch(test: &str) -> PathBuf {
    let run = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("imagepaste-{}-{run}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn key(host: &str) -> SessionKey {
    SessionKey { host: host.to_string(), name: "w".to_string() }
}

/// A clipboard that offers `types` and gives `bytes` for an image.
fn clipboard(types: &'static str, bytes: &'static [u8]) -> impl Fn(&[String]) -> anyhow::Result<Vec<u8>> {
    move |argv: &[String]| match argv.get(1).map(String::as_str) {
        Some("--list-types") => Ok(types.as_bytes().to_vec()),
        Some("--type") => Ok(bytes.to_vec()),
        _ => anyhow::bail!("unexpected {argv:?}"),
    }
}

#[test]
fn the_clipboard_type_and_the_file_name() {
    assert_eq!(pick_type("text/plain\nimage/jpeg\nimage/png\n"), Some("image/png"));
    assert_eq!(pick_type("image/jpeg\n"), Some("image/jpeg"));
    assert_eq!(pick_type("text/plain\nimage/webp\n"), None);
    assert_eq!(pick_type(""), None);
    let at = UNIX_EPOCH + Duration::from_secs(1_791_777_302) + Duration::from_micros(123_456);
    assert_eq!(stamp_name(at, "png"), "20261012-035502-123456.png");
    assert_eq!(stamp_name(UNIX_EPOCH, "jpg"), "19700101-000000-000000.jpg");
    assert_eq!(stamp_name(UNIX_EPOCH + Duration::from_secs(951_782_400), "png"), "20000229-000000-000000.png");
    assert_eq!(stamp_name(UNIX_EPOCH + Duration::from_secs(4_102_444_799), "png"), "20991231-235959-000000.png");
    let store = store_argv("ph", "a b.png", LOCAL);
    assert_eq!(store[0], "ssh");
    assert!(
        store.last().unwrap().ends_with("mkdir -p \"$HOME/.cache/tmls/images\" && cat > \"$HOME/.cache/tmls/images/\"'a b.png'"),
        "{store:?}"
    );
}

#[test]
fn on_this_machine_the_image_is_moved_into_tmls_images_and_its_path_typed() {
    let dir = scratch("local");
    let (home, staging) = (dir.join("home"), dir.join("home/.cache/tmls-rs/paste"));
    let k = key(LOCAL);
    let fed = RefCell::new(Vec::new());
    let feed = |argv: &[String], input: &[u8]| -> anyhow::Result<String> {
        fed.borrow_mut().push((argv.to_vec(), input.to_vec()));
        Ok(String::new())
    };
    let p = Paste { key: &k, local_name: LOCAL, staging: &staging, home: &home, now: UNIX_EPOCH + Duration::from_secs(1_791_777_302) };
    let want = home.join(".cache/tmls/images").join(NAME);
    assert_eq!(paste(&p, &clipboard("text/plain\nimage/png\n", PNG), &feed), Ok(format!("pasted image → {}", want.display())));
    assert_eq!(std::fs::read(&want).unwrap(), PNG);
    assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 0, "moved, not copied");
    let fed = fed.borrow();
    assert_eq!(fed.len(), 1, "only the typing");
    let (argv, input) = &fed[0];
    assert_eq!(argv[..2], ["sh", "-c"]);
    assert!(argv[2].contains("paste-buffer -p") && argv[2].contains("-t '=w:'") && !argv[2].contains("Enter"), "{}", argv[2]);
    assert_eq!(input.as_slice(), want.to_string_lossy().as_bytes());
}

#[test]
fn a_failed_copy_types_nothing_and_says_why() {
    let dir = scratch("remote");
    let (home, staging) = (dir.join("home"), dir.join("home/.cache/tmls-rs/paste"));
    let k = key("ph");
    let p = Paste { key: &k, local_name: LOCAL, staging: &staging, home: &home, now: UNIX_EPOCH + Duration::from_secs(1_791_777_302) };
    // To another host: ssh `cat >` with the bytes, then `~/…` typed; nothing left staged.
    let fed = RefCell::new(Vec::new());
    let feed = |argv: &[String], input: &[u8]| -> anyhow::Result<String> {
        fed.borrow_mut().push((argv.to_vec(), input.to_vec()));
        Ok(String::new())
    };
    let typed = format!("~/.cache/tmls/images/{NAME}");
    assert_eq!(paste(&p, &clipboard("image/png\n", PNG), &feed), Ok(format!("pasted image → {typed}")));
    assert_eq!(fed.borrow()[0], (store_argv("ph", NAME, LOCAL), PNG.to_vec()));
    assert_eq!(fed.borrow()[1].1, typed.as_bytes());
    assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 0);
    // The copy fails: it says so, types nothing, and keeps the staged file.
    let tried = RefCell::new(0);
    let refused = |_: &[String], _: &[u8]| -> anyhow::Result<String> {
        *tried.borrow_mut() += 1;
        anyhow::bail!("ssh: connect to host ph port 22: Connection refused\nmore")
    };
    assert_eq!(
        paste(&p, &clipboard("image/png\n", PNG), &refused),
        Err("copy to ph failed: ssh: connect to host ph port 22: Connection refused".to_string())
    );
    assert_eq!(*tried.borrow(), 1, "no typing after a failed copy");
    assert_eq!(std::fs::read(staging.join(NAME)).unwrap(), PNG);
    // No image: text only, nothing at all (wl-paste fails), or no wl-paste.
    let never = |_: &[String], _: &[u8]| -> anyhow::Result<String> { panic!("nothing is fed without an image") };
    assert_eq!(paste(&p, &clipboard("text/plain\nUTF8_STRING\n", b""), &never), Err("clipboard has no image".to_string()));
    let empty = |_: &[String]| -> anyhow::Result<Vec<u8>> { anyhow::bail!("Nothing is copied") };
    assert_eq!(paste(&p, &empty, &never), Err("clipboard has no image".to_string()));
    let missing = |_: &[String]| -> anyhow::Result<Vec<u8>> { anyhow::bail!("couldn't start wl-paste: No such file or directory (os error 2)") };
    assert_eq!(paste(&p, &missing, &never), Err("wl-paste is not installed: no image paste".to_string()));
    assert_eq!(paste(&p, &clipboard("image/png\n", b""), &never), Err("clipboard has no image".to_string()));
}
```

In `crates/desktop/tests/keys.rs`: add `'v'` to the Alt+Shift letters of the help test, and append:

```rust
#[test]
fn alt_shift_v_pastes_an_image_outside_forms() {
    assert_eq!(global(&ch('v', ALT_SHIFT), Focus::Terminal), Some(Global::PasteImage));
    assert_eq!(global(&ch('V', ALT_SHIFT), Focus::Sidebar), Some(Global::PasteImage));
    assert_eq!(global(&ch('v', ALT_SHIFT), Focus::Form), None);
    assert_eq!(global(&repeat(&ch('v', ALT_SHIFT)), Focus::Terminal), None);
}
```

- [ ] **Step 2: Run them.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core --test imagepaste --test tmux` and `tooling/docker/build.sh test -p desktop --test keys`
Expected: compile errors (`paste_text_script`, module `imagepaste`, `Global::PasteImage`).

- [ ] **Step 3: Implement.** `tmux.rs`: round 1 Task 7's `send_text_script` becomes two functions, and `paste_text_argv` follows `send_text_argv`:

```rust
/// Types the text on the script's stdin into `target` (tmls `prompts.send` without its Enter):
/// through a paste buffer of its own (`buffer`), as a bracketed paste when the program asks
/// for one; the buffer is deleted after the paste, or when the paste fails. The text never
/// appears in the script.
pub fn paste_text_script(target: &str, buffer: &str) -> String {
    let (t, b) = (quote(target), quote(buffer));
    format!(
        "tmux load-buffer -b {b} - && {{ tmux paste-buffer -p -d -b {b} -t {t} || {{ tmux delete-buffer -b {b}; false; }}; }}"
    )
}

/// [`paste_text_script`], then Enter: a message sent.
pub fn send_text_script(target: &str, buffer: &str) -> String {
    format!("{} && sleep 0.2 && tmux send-keys -t {} Enter", paste_text_script(target, buffer), quote(target))
}

/// [`paste_text_script`] for the pane (`pane_target`), here or on its host.
pub fn paste_text_argv(key: &SessionKey, pane: Option<&str>, buffer: &str, local_name: &str) -> Vec<String> {
    script_argv(&key.host, paste_text_script(&pane_target(key, pane), buffer), local_name)
}
```

`crates/client-core/src/imagepaste.rs`:

```rust
//! Image paste (Alt+Shift+V, tmls `image_paste.py`, spec G4): the clipboard's image (`wl-paste`)
//! is staged in `~/.cache/tmls-rs/paste/`, then moved (this machine) or copied (another host,
//! through ssh) into `~/.cache/tmls/images/` there (R3), and its path is typed into the session
//! as a bracketed paste, without Enter. The clipboard reader and every command are parameters.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::listing::SessionKey;
use crate::tmux;

/// Where the images go on the session's host, under its home (shared with tmls, R3).
pub const HOST_DIR: &str = ".cache/tmls/images";
/// Where an image waits here first, under this machine's home.
pub const STAGING: &str = ".cache/tmls-rs/paste";
/// The status line when there is no image to paste.
pub const NO_IMAGE: &str = "clipboard has no image";
/// How long reading the clipboard, the copy, or the typing may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// Runs a command and returns its bytes (`tmux::run_bytes` in the app).
pub type Reader<'a> = &'a dyn Fn(&[String]) -> anyhow::Result<Vec<u8>>;
/// Runs a command with bytes on its stdin (`tmux::run_with_stdin` in the app).
pub type Feeder<'a> = &'a dyn Fn(&[String], &[u8]) -> anyhow::Result<String>;

/// One paste: into which session, where this machine's home and staging folder are, and when.
pub struct Paste<'a> {
    pub key: &'a SessionKey,
    pub local_name: &'a str,
    pub staging: &'a Path,
    pub home: &'a Path,
    pub now: SystemTime,
}

/// The image type to take from `wl-paste --list-types`: PNG, else JPEG.
pub fn pick_type(list: &str) -> Option<&'static str> {
    ["image/png", "image/jpeg"].into_iter().find(|mime| list.lines().any(|line| line.trim() == *mime))
}

/// The file extension of `pick_type`'s answer.
pub fn extension(mime: &str) -> &'static str {
    if mime == "image/jpeg" { "jpg" } else { "png" }
}

/// Days since 1970-01-01 as (year, month, day) in the proleptic Gregorian calendar.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The image's file name: `YYYYmmdd-HHMMSS-micros.EXT` in UTC (tmls's `%Y%m%d-%H%M%S-%f`).
pub fn stamp_name(now: SystemTime, ext: &str) -> String {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (year, month, day) = civil((secs / 86_400) as i64);
    let rest = secs % 86_400;
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}-{:06}.{ext}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        since.subsec_micros()
    )
}

/// Writes its stdin to `~/.cache/tmls/images/NAME` on `host` (tmls `image_paste.store`).
pub fn store_argv(host: &str, name: &str, _local_name: &str) -> Vec<String> {
    tmux::ssh(
        host,
        format!("mkdir -p \"$HOME/{HOST_DIR}\" && cat > \"$HOME/{HOST_DIR}/\"{}", tmux::quote(name)),
    )
}

fn first_line(e: &anyhow::Error) -> String {
    format!("{e:#}").lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("it failed").to_string()
}

/// A rename, else a copy and a remove (another file system).
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to).or_else(|_| {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)
    })
}

/// Pastes the clipboard's image into the session; the status line, or why not. Nothing is
/// typed unless the image is on the session's host; a staged image that did not get there is
/// kept.
pub fn paste(p: &Paste, read: Reader, feed: Feeder) -> Result<String, String> {
    let types = read(&["wl-paste".to_string(), "--list-types".to_string()]).map_err(|e| {
        match first_line(&e).starts_with("couldn't start") {
            true => "wl-paste is not installed: no image paste".to_string(),
            false => NO_IMAGE.to_string(),
        }
    })?;
    let mime = pick_type(&String::from_utf8_lossy(&types)).ok_or_else(|| NO_IMAGE.to_string())?;
    let bytes = read(&["wl-paste".to_string(), "--type".to_string(), mime.to_string()])
        .map_err(|e| format!("couldn't read the clipboard's image: {}", first_line(&e)))?;
    if bytes.is_empty() {
        return Err(NO_IMAGE.to_string());
    }
    let name = stamp_name(p.now, extension(mime));
    let staged = p.staging.join(&name);
    std::fs::create_dir_all(p.staging)
        .and_then(|()| std::fs::write(&staged, &bytes))
        .map_err(|e| format!("couldn't write {}: {e}", staged.display()))?;
    let path = if p.key.host == p.local_name {
        let dir = p.home.join(HOST_DIR);
        let dest = dir.join(&name);
        std::fs::create_dir_all(&dir)
            .and_then(|()| move_file(&staged, &dest))
            .map_err(|e| format!("couldn't move the image to {}: {e}", dest.display()))?;
        dest.to_string_lossy().into_owned()
    } else {
        feed(&store_argv(&p.key.host, &name, p.local_name), &bytes)
            .map_err(|e| format!("copy to {} failed: {}", p.key.host, first_line(&e)))?;
        let _ = std::fs::remove_file(&staged);
        format!("~/{HOST_DIR}/{name}")
    };
    feed(&tmux::paste_text_argv(p.key, None, &tmux::buffer_name(), p.local_name), path.as_bytes())
        .map_err(|e| format!("couldn't type the path into {}: {}", p.key.name, first_line(&e)))?;
    Ok(format!("pasted image → {path}"))
}
```

`lib.rs`: `pub mod imagepaste;` and a crate-comment line. (`tmux::ssh` and `tmux::quote` are `pub(crate)`: reachable here.)

`keys.rs`: `Global` gains `/// The clipboard's image to the front tab's host, its path typed.\n PasteImage,`; `binding`: `Key::Char(c) if c.eq_ignore_ascii_case(&'v') => Some(Global::PasteImage),`; `HELP` Everywhere after `Alt+Shift+S`: `("Alt+Shift+V", "paste the clipboard's image"),`; module comment: "Alt+Shift+V — the clipboard's image to the front tab's host, its path typed in".

- [ ] **Step 4: Run the tests.**
Run: `source spikes/s1/build-env.sh && cargo test -p client-core` and `tooling/docker/build.sh test -p desktop --test keys`
Expected: all pass (round 1's `send_text_script_pastes_through_a_named_buffer_then_presses_enter` included: the text is the same).

- [ ] **Step 5: Wire `app.rs`.** Imports: `use client_core::imagepaste;`. `global`: `Global::PasteImage => self.paste_image(),`. New method:

```rust
    /// Alt+Shift+V: the clipboard's image to the front tab's host, its path typed in, on a
    /// thread; the status line says how it went (`UserEvent::Said`).
    fn paste_image(&mut self) {
        let Some(index) = self.tabs.active() else {
            return self.say(Ok("no tab to paste an image into".to_string()));
        };
        let key = self.tabs.key(index).clone();
        if client_core::listing::split_host(&key.host).1.is_some() {
            return self.say(Err(format!(
                "{} is on a background tmux server: no image paste there",
                key.name
            )));
        }
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return self.say(Err("no HOME: nowhere to put the image".to_string()));
        };
        self.say(Ok(format!("pasting the clipboard's image into {} …", key.name)));
        let (proxy, local) = (self.proxy.clone(), self.cfg.local_name.clone());
        std::thread::spawn(move || {
            let staging = home.join(imagepaste::STAGING);
            let paste = imagepaste::Paste {
                key: &key,
                local_name: &local,
                staging: &staging,
                home: &home,
                now: std::time::SystemTime::now(),
            };
            let said = imagepaste::paste(
                &paste,
                &|argv| client_core::tmux::run_bytes(argv, imagepaste::TIMEOUT),
                &|argv, input| client_core::tmux::run_with_stdin(argv, input, imagepaste::TIMEOUT),
            );
            let _ = proxy.send_event(UserEvent::Said(said));
        });
    }
```

- [ ] **Step 6: e2e block.** Top: `PASTE='tmlsrs-test-00paste'`; `cleanup()`: `tmux kill-session -t "=$PASTE" 2>/dev/null`. Before `# --- quit` (after Task 7's block):

```bash
# --- image paste (G4) ----------------------------------------------------------------------------
# The first screenshot as the clipboard's PNG; $PASTE's tab in front; Alt+Shift+V types the path.
tmux new-session -d -s "$PASTE" -x 100 -y 30
sleep 6
key_focus_list
top_of_list
key_enter
check "the tab of $PASTE opens" wait_for 8 attached_to "$PASTE"
timeout 5 wl-copy -t image/png <"$OUT/01-start.png"
keys -M alt -M shift -k v -m shift -m alt
typed_image() { pane_text "$PASTE" | grep -qF -- "$HOME/.cache/tmls/images/"; }
if wait_for 8 typed_image; then
  pass "Alt+Shift+V types the image's path into the session"
else
  fail "Alt+Shift+V types the image's path into the session" "pane: $(pane_text "$PASTE" | grep -v '^$' | tail -2 | tr '\n' '|'); stderr: $(tail -2 "$OUT/app.stderr" | tr '\n' ' ')"
fi
IMG=$(pane_text "$PASTE" | grep -oE "$HOME/\.cache/tmls/images/[0-9]{8}-[0-9]{6}-[0-9]{6}\.png" | head -1)
check "the typed path is the clipboard's image, byte for byte" cmp -s "$OUT/01-start.png" "$IMG"
check "nothing is left in the staging folder" bash -c '[ -z "$(ls -A "$0" 2>/dev/null)" ]' "$HOME/.cache/tmls-rs/paste"
check "screenshot 49-image-paste (pasted image → PATH)" shot 49-image-paste
key_close_tab
tmux kill-session -t "=$PASTE"
```

- [ ] **Step 7: All tests, lints, fmt, e2e.**
Run: `tooling/docker/build.sh test -p desktop`; client-core tests; clippy; fmt; `SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/t8-e2e tooling/smoke/run.sh`
Expected: all pass. Look at `/data/scratch/tmls-rs-v0/round2/t8-e2e/49-image-paste.png`: the path at the prompt, the status line `pasted image → …`.

- [ ] **Step 8: Commit.**

```bash
git add crates/client-core crates/desktop/src crates/desktop/tests tooling/smoke/e2e.sh
git commit -m "feat(desktop): Alt+Shift+V puts the clipboard's image on the session's host and types its path"
```

---

### Task 9: Whole-branch review, speed before/after, e2e three times

**Files:**
- Create (scratch only): `/data/scratch/tmls-rs-v0/round2/review/branch.diff`, `/data/scratch/tmls-rs-v0/round2/bench/…`, `/data/scratch/tmls-rs-v0/round2/e2e-{1,2,3}/…`
- Modify: whatever a confirmed finding needs, one commit per finding, test first.

**Interfaces:** consumes the whole branch; produces the review's findings (in the main agent's ledger, then in `PROGRESS.md`) and the speed table.

- [ ] **Step 1: The diff for the reviewer.**
Run: `mkdir -p /data/scratch/tmls-rs-v0/round2/review && git diff "$BASE"..feat/parity-round2 > /data/scratch/tmls-rs-v0/round2/review/branch.diff && wc -l /data/scratch/tmls-rs-v0/round2/review/branch.diff`

- [ ] **Step 2: One fresh Opus reviewer** (`model: opus`) on that file, the spec and this plan, with this focus list and not the main agent's conclusions: the five Review Focus lines; quoting of every word that reaches a shell (window names, `SERVER_SOCKETS_SCRIPT`'s socket names, the session id in `recent_argv`, the image name in `store_argv`, `paste_text_script`, `attach_argv`'s `-L`); threads and processes that can pile up or outlive the window (the ticker, one `game-mode status` per tick with its 2 s limit, one socket scan per host per tick under `POLL_TIMEOUT` < `TICK`, the image paste thread, `canberra-gtk-play` per alert); `Poller::slow_down` against `refresh_now`/`force_next` and a thread mid-listing; game mode and alerts (nothing runs, the list still fills); the background rows against selection by identity, pins, renames and a host going offline; `notifications.json` writes (atomic, only on a flip); the staging folder; anything new on the key-to-frame path (quick select draws only while open; `Tick` and `Background` redraw only when something changed); panics on an empty frame, zero targets, a `game-mode` that prints nothing, a socket directory that is a file.

- [ ] **Step 3: Rule on each finding** (fix now / later / never), fix the "now" ones one commit each with a failing test first, run the full suite after each.

- [ ] **Step 4: Speed, before.** A detached worktree of the round's base in scratch (created, never removed without the user's yes):

```bash
B=/data/scratch/tmls-rs-v0/round2
git worktree add --detach "$B/base" "$BASE"
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

Expected: head's median and p95 key→frame within the base runs' spread; idle frames 0 in every run (as base). A miss is a finding: find what draws while idle (a `Tick` or a `Background` answer that invalidates without a change would be one), fix, re-measure.

- [ ] **Step 6: e2e three times in a row**, each into a fresh directory:

```bash
for n in 1 2 3; do SMOKE_OUT=/data/scratch/tmls-rs-v0/round2/e2e-$n tooling/smoke/run.sh || break; done
```

Expected: three times `== 0 of N checks failed`. A flaky check is a finding (fix the cause, not the wait), and the three runs start again.

- [ ] **Step 7:** Final `tooling/docker/build.sh test -p desktop`, `source spikes/s1/build-env.sh && cargo test -p client-core`, clippy, fmt — all clean. Note the verified commit hash for the PC build.

---

### Task 10: Docs, the hand-off sheet, PROGRESS

**Files:**
- Modify: `docs/desktop-v0.md` (sections `## Config files`, `## Keys`, `## New session, rename, kill`, `## Alerts`, `## Mouse` — find them by heading; round 1 moved them)
- Create: `docs/NIGHT-TEST-2.md`
- Modify: `PROGRESS.md` (the main agent writes this entry itself)

**Interfaces:** consumes everything above; produces documentation only.

- [ ] **Step 1: `docs/desktop-v0.md`.**
  - `## Config files`: the `notifications.json` row says "read; written when a switch in the alerts list is flipped (desktop, sound, silence_focused — tmls's three keys)". Under the table: "Written on hosts: `~/.cache/tmls/images/` (pasted images, Alt+Shift+V; shared with tmls). Here: `~/.cache/tmls-rs/paste/` (an image that did not reach its host stays there). Optional programs: `game-mode` (game mode), `canberra-gtk-play` (alert sound), `wl-paste` (image paste); each missing one turns its feature off."
  - `## Keys`: replace the ```text block with the exact output of `target-docker/release/tmls-rs keys` at the verified commit (run it and paste; do not retype), then one paragraph each on quick select (Alt+Shift+S; `a` is the target nearest the bottom; the letter copies, Shift opens as Ctrl+click; Esc), image paste (Alt+Shift+V; PNG or JPEG; `~/…` path typed without Enter) and background servers (`b`; attach only).
  - `## New session, rename, kill`: a paragraph on the menu's window and pane entries (New window and the splits at once; Rename window… and Kill pane… ask; the last pane ends the session and its tab).
  - `## Alerts`: the three switches at the top of the list (Space flips; Sound needs `canberra-gtk-play`); in game mode nothing notifies, the list still fills.
  - `## Mouse`: rows `| click ⚠ sign in on a host | a terminal with ssh to it |`, `| click background ▸ · N | show / hide the other tmux servers' sessions |`.

- [ ] **Step 2: `docs/NIGHT-TEST-2.md`** — what the user tries in the morning, in the order they meet it, one box each:

```markdown
# Night of 2026-10-12, second round: what to try (tmls-rs parity round 2)

The PC has the build of `<verified commit>` (or: not built — game mode was on; say "build it").
Start it as usual with `tmls-rs`. Each line: ok / not ok / a note. Screenshots of the headless
run are under `/data/scratch/tmls-rs-v0/round2/e2e-3/`.

## The row menu
- [ ] `m` on a session (or right-click): New window, Rename window…, Split side by side, Split top / bottom, Kill pane… under the old four.
- [ ] Split side by side, then Kill pane… on a session with one pane: the confirm says it is the last pane.

## Game mode (on the PC)
- [ ] `game-mode on`: within 30 s the remote hosts say `· game`; no notification pops up; the bell still counts.
- [ ] `game-mode off`: the tag goes within 30 s and remote rows move every 5 s again.

## Hosts
- [ ] A host whose ssh wants a password or a code says `⚠ sign in`; a click opens a terminal with ssh; after signing in the row comes back within a few seconds.
- [ ] A host with another tmux server (`tmux -L web …`) shows `background ▸ · N`; `b` lists them; Enter attaches.

## Alerts
- [ ] `a`: Desktop / Sound / Silence focused at the top; Space flips one; tmls (Python) shows the same switches after a restart.
- [ ] Sound ON: an alert plays a sound (needs canberra-gtk-play).

## Messages
- [ ] `s` on a Claude row: under Saved, Recent lists your last messages to it; ↑ / ↓ walks both.

## Quick select and image paste
- [ ] Alt+Shift+S in a tab: letters on links, paths and hashes; a letter copies, Shift+letter opens (URL in the browser, file in the panel).
- [ ] Copy a screenshot, Alt+Shift+V in a Claude tab on a remote host: `~/.cache/tmls/images/….png` is typed; Claude can read it.

## Decisions to veto (spec R1–R8)
Game mode slows remote only; quick select copies on a letter, opens on Shift, 26 from the bottom; images in `~/.cache/tmls/images/`; background servers attach only; sign-in in a terminal; sound only through canberra-gtk-play; recent = last 10; order G1 → G2 → G6 → G7 → G8 → G5 → G3 → G4.
```

(The main agent puts the verified commit hash where the sheet says `<verified commit>`, or the game-mode sentence, before committing.)

- [ ] **Step 3: PC build** (main agent, see "Who does the work"): game mode first, as its own command; build only if `off`.

- [ ] **Step 4: PROGRESS entry** (main agent): what was built per task with its commit, the e2e count and the three run directories, the speed table (base vs head), the review's findings and rulings, every "Departs from the spec" of this plan, what is not verified in a real window (sound, a real sign-in, a real remote image paste, game mode on the PC), the scratch worktree `/data/scratch/tmls-rs-v0/round2/base` left in place (removal needs the user's yes), and NEXT = the user's NIGHT-TEST-2 answers.

- [ ] **Step 5: Commit.**

```bash
git add docs/desktop-v0.md docs/NIGHT-TEST-2.md PROGRESS.md
git commit -m "docs: keys, config and the morning's test sheet for parity round 2"
```

---

## Self-review (done while writing; kept for the executor)

**Spec coverage.** G1 → T1 (new window, rename window, split both ways, kill pane with confirm and last-pane warning; argv builders `new_window_argv`, `rename_window_argv`, `split_argv`, `kill_pane_argv`, `pane_count_argv`; e2e split → two panes). G2 → T2 (`game-mode status` every 30 s locally, skipped without the program; remote period 300 s via `Poller::slow_down`, local 5 s; push untouched; no notification commands; `· game` dim; e2e fake `game-mode` + ssh call count). G6 → T3 (`sign-in required` → `⚠ sign in`; click opens `terminal_argv` with ssh; next poll clears; e2e stub ssh `Permission denied`). G7 → T4 (`notifications.json` read and written, three toggles at the list's top, Space flips, `canberra-gtk-play` if on PATH, defaults as tmls; e2e Space writes the file). G8 → T5 (last 10 user lines, off-thread, under Saved; e2e fake transcript). G5 → T6 (collapsed `background ▸ · N` per host, `b`, `tmux -L` attach, `HOST#NAME`, rescan on the 30 s tick; e2e second server). G3 → T7 (labels a–z over URLs, paths, hashes; letter copies, Shift opens via Ctrl+click's routing; Esc; 26 from the bottom; e2e `wl-paste` and `xdg-open` checks). G4 → T8 (`wl-paste --list-types` png/jpeg, `~/.cache/tmls-rs/paste/<utc-stamp>.<ext>`, moved locally, copied remotely, path typed as bracketed paste without Enter, the three status texts; e2e `wl-copy -t image/png`). Testing section → T9 (three e2e runs, bench before/after; hand-off sheet T10). R1 T2 (`game_mode` slows remote only; test `slow_down_gives_those_hosts_their_own_period…`). R2 T7 (`targets_keep_the_26_nearest_the_bottom…`, `a_letter_copies_shift_and_the_letter_opens…`). R3 T8 (`HOST_DIR`). R4 T6 (`menu`/`toggle_pin` refuse `HOST#NAME`; `paste_image` refuses). R5 T3 (`sign_in` in a terminal). R6 T4 (`SOUND_PLAYER`, `emit`'s `player`). R7 T5 (`RECENT`, thread in `ask_form`, listed under Saved). R8: task order T1–T8.

**Not pinned, or contradicting the code** (also in the tasks' "Departs from the spec"): Enter on the sign-in row (the cursor never stands on a header, `sidebar.rs:15-19`); `HostSnapshot.sign_in` as a field (a method reads `poll.rs:22`'s `SIGN_IN_REQUIRED`); `-i message` vs tmls's `-i message-new-instant -d tmls` (`notifications.py:53`); `scp_argv` (ssh `cat >`); the Global Constraint "writes only `pins`, never `~/.cache/tmls/`" vs G7/R3 (amended above; e2e `only_pins_written` changed in T4); a missing `game-mode` is a stderr line, not a status sentence. Found in round 1's plan while writing this one: its Task 6 Step 8 starts the second run as `TMLS_RS_POLL_S=60 "$BIN" …` without `--bench` or `TMLS_RS_FOREGROUND=1`; since `a31fe91` (fix/detach) that run detaches, `APP_PID` is a parent that exits at once and `app2.stderr` stays empty — the third run here sets `TMLS_RS_FOREGROUND=1`; if round 1's Task 6 kept the bug, Task 9's reviewer should get it as a finding.

**Placeholders.** The only angle-bracket text left is `<verified commit>` in the hand-off sheet and the PC build command, which the main agent fills from Task 9 Step 7 before the commit that carries it.

**Type consistency.** `Split` T1 → `Request::Split`, `split_argv`. `PaneInfo` T1 → `Form::ConfirmKillPane`, `set_pane_info`, `UserEvent::PaneInfo`. `HeaderLook` T2 (`polled`, `game`) → T3 adds `sign_in` (T2's test literal gains `..HeaderLook::default()` in T3). `header_spans_with` (round 1) delegates to `header_spans_look`. `TICK`/`tick` T2 → extended in T6. `Running.game` T2 → read in T4 (`emit`) and T6 (scans). `Settings { desktop, sound, silence_focused }` T4 everywhere after. `Target::Switch` T4 is the only new `forms::Target`. `Form::Ask.recent: Option<Vec<String>>` T5. `split_host` T6 → `attach_argv`/`attach_text` (T6), `Sidebar::menu`/`toggle_pin` (T6), app `ListKey::Menu`, `foot_button` (T6), `paste_image` (T8). `Row::Background` T6 → `shape`/`kinds` test arms, `draw::sidebar`. `links::row_links`/`frame_rows` T7 → `quickselect::targets`, `quick_select`. `paste_text_script`/`paste_text_argv` T8 → `send_text_script` (round 1) built on it, `imagepaste::paste`. `SessionInfo` literals in new code (`parse_background`) carry round 1's `last_reply: None`; new tests build `SessionInfo` only through the files' existing `session(…)` helpers, which round 1 already extended. `View` literals end in `..View::default()` (round 1), so `game_hosts`, `sign_in`, `labels` need no test edits.
