# tmls-rs desktop v0 — overnight build plan (2026-10-11)

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development to run this plan task by
> task, **unattended** (the user is asleep; see "Unattended rules"). Steps use checkbox (`- [ ]`) syntax.
> Task headings are `### Task N:` so the skill's `task-brief` script can extract them.

**Goal:** by morning, a native GPU window called `tmls-rs` that looks and works like the user's tmls sidebar +
terminal: real tmux sessions from every host, status marks, click to attach in a tab, new / rename / kill.

**Architecture:** a thin native client over **ssh + tmux** — no worker, no hostd, no own transport. Two new crates:
`crates/client-core` (no UI: hosts, listing, status, tmux commands, config) and `crates/desktop` (window, sidebar,
tabs; binary `tmls-rs`). The terminal in each tab is an `s2_core::Emu` running `tmux attach` (over `ssh -t` for
remote hosts); drawing, input, selection and latency marks come from the `s2-wgpu` library built on 2026-10-10/11.

**Tech stack:** Rust 2024, winit 0.30.13, wgpu 30.0.1, cosmic-text 0.19, libghostty-vt 0.2.2 (Zig 0.15.2), arboard
3.6.1 — all already in `Cargo.lock`. New dependencies need a reason in the task report.

**Spec:** `docs/PLAN.md` §2 (requirements), §3 row "Build or reuse?" (*"first measure simpler options: direct
ssh+tmux in a native terminal"* — this plan is that option), §3 row "tmux bridge", §8 (parity checklist).
**Behaviour reference = the user's Python tmls, branch `feat/native-attach`, work tree `/data/scratch/tmls-b1/wt-na`**
(read-only; never check that branch out in `~/Others/tmls`). Its README is `/home/sgkayast/Others/tmls/README.md`,
its screenshot `/home/sgkayast/Others/tmls/docs/screenshots/tmls.png`.

## Decisions taken for the user (veto any of these before restarting)

1. **"Finish building tmls overnight" = desktop v0 over ssh+tmux**, not PLAN phases 1a–7. The worker/hostd/transport
   architecture (PLAN §4–§5) and spikes S3–S7 cannot be built and gated in one night. v0 keeps tmux as the session
   holder, exactly as tmls does today. Cost if wrong: v0's `client-core` listing/ops are reusable; the tab→`Emu`
   wiring is replaced when workers exist.
2. **Toolkit = wgpu**, on the evidence of 2026-10-11 (key→present p50 0.56 ms on the PC; sidebar, field, menu, zoom
   built and reviewed). The gpui comparison (Track B, the 16-run matrix) is **deferred, not cancelled**. Cost if
   wrong: `crates/desktop` UI code is redone in gpui; `client-core` and `s2-core` are not affected.
3. **Scope tiers.** Must (Tasks 1–5) = usable daily: list, marks, attach in tabs, keys. Should (Tasks 6–7) =
   new/rename/kill, mouse to tmux, links. Stretch (Task 8) = alerts. Tasks 9–10 (tests, PC run, hand-off) always
   run. **Not in v0:** `ag`, `ct`, approvals, Ask/queue, prompts, file viewer, image paste, quick select, splits
   menu, kitty (non-tmux) sessions, web, Android, Windows.
4. **The A3 checklist** (`docs/spikes/S2-checklist-wgpu.md`): the machine-doable items run on the PC in Task 9
   (bench with and without sidebar, `hz`, CJK + emoji screenshot). Items that need a person (fcitx5, monitor move,
   changing the system scale) stay for the user. System display settings are never changed.

## Global Constraints

- Repo `/home/sgkayast/Others/tmls-rs`. Work on branch **`feat/desktop-v0`** from `main`. Do not push anywhere
  (the PC fetches from archbox). Merge to `main` only in Task 10, `--no-ff`, never squash.
- Commit author `sgkayast <sgn.kayastha@gmail.com>` (`git -c user.name=sgkayast -c user.email=sgn.kayastha@gmail.com
  commit`). **No AI attribution, no `Co-Authored-By`.** Stage by explicit path, never `git add -A`.
- **No deletes or moves** of anything that exists now (files, directories, branches, Docker images, tmux sessions that
  the test did not create). New scratch goes under `/data/scratch/tmls-rs-v0/`; `/data` is 89 % full — keep it under
  2 GB. **Never `/tmp`**: `export TMPDIR=/data/scratch/tmls-rs-v0/tmp`.
- **No host package installs** on archbox or the PC. Docker images are fine. Build env on archbox:
  `source spikes/s1/build-env.sh`. GUI crates build on archbox only inside `tooling/docker/build.sh`.
- Do not edit `spikes/s2-gpui/**`, `spikes/s2-bench/**`, `tooling/docker/Dockerfile.build`, or anything in
  `~/Others/tmls` and `/data/scratch/tmls-b1/`. `spikes/s2-core/**` and `spikes/s2-wgpu/**` may change **additively**
  (new items, no signature changes); a contract change is recorded in
  `docs/superpowers/plans/2026-10-08-s2-split-overview.md` in the same commit.
- **Never touch the user's live sessions.** Tests create tmux sessions only with the prefix **`tmlsrs-test-`** and
  kill only those. Never `tmux kill-server`, never `kill-session` on another name, never send keys to another
  session. On the PC never restart kitty or the user's running `tmls`, never toggle `game-mode`.
- Config is **read** from `~/.config/tmls/` (same files tmls uses: `hosts`, `name`, `pins`, `host-colors.json`,
  `agent-presets.json`). v0 **writes only `pins`**, in tmls's format. No other file there is created or changed.
- Latency invariants from S2 hold for the terminal in every tab: key time stamp first in `window_event`;
  `Marks::begin_redraw()` first in `redraw`; every taken frame through `Scene::update_cells`; keys typed while
  focus is not on a terminal reach neither a PTY nor a latency log.
- Tests must not hang: every wait has a timeout that fails the test. Test output pristine (no warnings).

## Unattended rules (the user is asleep)

- No questions to the user. Decide, write a `Ruling:` line in the SDD ledger, continue. Stop only for: an
  irreversible/destructive operation, a security-sensitive action, or a plan so broken every path is a guess.
- **Time budget.** Record the start time in the ledger. Per task: at most 2 fix rounds (not 5) — after that, park
  what is left with a ruling and move on. **At 06:30 local time stop starting implementation tasks** and go to
  Task 9 → 10 with whatever is done. Tasks 9 and 10 must finish; an unfinished feature task is fine.
- If a permission prompt or classifier denial blocks a command: do not work around it; record it, use the fallback
  the task names, continue.
- PC (`ssh pcl`): the user said on 2026-10-11 "you PC, it will be on overnight" — PC use is approved for this plan.
  Docker only, inside `~/Others/tmls-rs` (the clone) on the PC. Check `ssh pcl 'bash -lic "game-mode status"'` first;
  `on` → skip the PC. If `ssh pcl` commands are denied → skip the PC, say so in the hand-off.
- Models: implementers and task reviewers `opus`; scoped re-reviews of small diffs `sonnet`; lookups `haiku`.
  Never `fable`. One implementer at a time.
- End: Telegram ping (`tg`, format in `~/.claude/docs/telegram.md`) when Task 10 is done or when stopped.

## Review Focus

Conditions no single task's happy path covers, most likely first — each is pinned by a named test in its task:

1. **A host that is offline or slow** (ssh hangs, `BatchMode` refuses): the window must stay responsive and the host
   show as *offline*; one dead host must not delay the others. → Task 1 `slow_host_does_not_block_the_others`.
2. **Session names with spaces, quotes, `:` `.` `=` `$` and non-ASCII** (the user has a session called
   `Agent Thing`): every tmux/ssh command must quote them; attach must target exactly that session
   (`-t '=NAME'`). → Task 2 `attach_and_ops_quote_hostile_names`, Task 9 end-to-end with `tmlsrs-test-a b'c`.
3. **The attached session ends or ssh drops** while its tab is open: the tab says so and Enter reattaches; the app
   does not exit, other tabs keep working. → Task 3 `a_tab_whose_child_exits_stays_and_can_reattach`.
4. **The list changes under the user** (session renamed or killed elsewhere, host goes offline) while a row is
   selected or a tab is open on it: selection follows the session by identity, not by row index; the tab is marked
   ended. → Task 4 `selection_follows_the_session_when_rows_move`.
5. **Keys while focus is in the sidebar or a form**: nothing reaches any session. Typing a session name into the
   new-session form must not type into the terminal behind it. → Task 5 `sidebar_and_form_keys_reach_no_pty`.

---

## File structure

```
Cargo.toml                         add members crates/client-core, crates/desktop; default-members += client-core
crates/client-core/                no UI, no winit/wgpu; tested on archbox with plain cargo
  src/lib.rs        re-exports
  src/config.rs     ~/.config/tmls readers (hosts, name, pins, host-colors, agent-presets); pins writer
  src/listing.rs    the per-host listing script + parser → HostSnapshot / SessionInfo / Status
  src/poll.rs       one thread per host, 5 s period, timeout; snapshots over a channel
  src/tmux.rs       command builders: attach, new-session, rename, kill, copy text of the attach command
  src/seen.rs       "unseen completion" (◆) bookkeeping per session identity
  tests/            listing fixtures ported from tmls tests; a real local tmux test (prefix tmlsrs-test-)
crates/desktop/                    binary `tmls-rs`
  src/main.rs       CLI (hosts on the command line, --bench DIR), starts the app
  src/app.rs        winit ApplicationHandler: wiring only
  src/tabs.rs       tab model: open/switch/close/reattach; one s2_core::Emu per tab
  src/sidebar.rs    sidebar model: host sections, rows, selection by identity, scroll, pins
  src/draw.rs       sidebar, tab bar, status line, forms → gfx instances (uses s2_wgpu::gfx)
  src/keys.rs       global bindings and routing (terminal / sidebar / form)
  src/forms.rs      new-session form, rename field, kill confirm
  src/theme.rs      colours and sizes taken from tmls
  tests/            pure-logic tests + PTY tests; no window
docs/desktop-v0.md  what it is, how to build and run, key bindings, what is not built
docs/MORNING-TEST.md  the user's short test list for the morning (Task 10)
```

Reused as libraries, not copied: `s2_core::{Emu, InputMsg, KeyEvent, Frame, latency}`,
`s2_wgpu::{gfx, input, select, marks, surface, meta}`. Read their module headers before use; the contract is
`docs/superpowers/plans/2026-10-08-s2-split-overview.md` ("Contract notes from A1" and "from A2").

---

### Task 0: Branch, workspace, crates that build

**Files:** Modify `Cargo.toml`; create `crates/client-core/{Cargo.toml,src/lib.rs}`, `crates/desktop/{Cargo.toml,src/main.rs}`; commit `PROGRESS.md`.

- [ ] **Step 1:** On `main`, commit the pending `PROGRESS.md` alone (`docs: PROGRESS — Track A merged, cleanup`).
      Create `feat/desktop-v0` from it. Create `/data/scratch/tmls-rs-v0/tmp`.
- [ ] **Step 2:** Add both crates as workspace members; `default-members` gains `crates/client-core` only.
      `client-core` depends on `anyhow`, `serde`, `serde_json` (already in the lock file). `desktop` depends on
      `client-core`, `s2-core`, `s2-wgpu` by path. `main.rs` prints the version and exits.
- [ ] **Step 3:** `source spikes/s1/build-env.sh && cargo test -p client-core` → 0 tests, exit 0.
      `tooling/docker/build.sh build --release -p desktop` → builds (if the image is missing on archbox, build it
      per `tooling/docker/README.md`). `cargo test -p s2-core -p s2-wgpu` still 155 passed / 1 ignored.
- [ ] **Step 4:** Commit `desktop-v0: workspace members client-core and desktop`.

The controller does Task 0 itself (mechanical, no review seat needed) and records the BASE commit in the ledger.

---

### Task 1: `client-core` — hosts, listing, status marks, polling

**Reference (port the behaviour, do not transliterate):** `/data/scratch/tmls-b1/wt-na/src/tmls/hosts.py` (the
listing shell script, its parser, status rules, tool letters, Claude name + context %), `local.py` (local machine
name), tests `/data/scratch/tmls-b1/wt-na/tests/test_hosts.py`. README section "Shows what each session is doing".

**Files:** Create `crates/client-core/src/{config.rs,listing.rs,poll.rs}`, `tests/listing.rs`, `tests/poll.rs`,
`tests/data/*.txt` (listing outputs captured from archbox and synthesised edge cases).

**Interfaces — Produces:**

```rust
// config.rs
pub struct Config { pub local_name: String, pub hosts: Vec<String>, pub host_colors: BTreeMap<String, HostColor>,
                    pub presets: Vec<(String, Vec<String>)>, pub pins: BTreeSet<SessionKey> }
pub enum HostColor { Blue, Teal, Violet, Green, Amber, Rose }
impl Config { pub fn load(dir: &Path, cli_hosts: &[String]) -> Config;   // missing files = defaults, never an error
              pub fn save_pins(dir: &Path, pins: &BTreeSet<SessionKey>) -> anyhow::Result<()>; }
pub fn default_color(host: &str) -> HostColor;       // stable across restarts; same rule as tmls host_colors.py

// listing.rs
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey { pub host: String, pub name: String }     // identity; host "" is never used, local = local_name
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Running, Waiting, Unseen, Error, Idle }        // ● ? ◆ ✕ ○   (Unseen is set by seen.rs, not here)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool { Claude, Opencode, Codex, Other }                 // letters C O X, none
#[derive(Clone, Debug, PartialEq)]
pub struct SessionInfo { pub key: SessionKey, pub status: Status, pub reason: Option<String>, pub tool: Tool,
                         pub title: Option<String>, pub context_pct: Option<u8>, pub attached: u32,
                         pub last_output_age_s: Option<u64> }
#[derive(Clone, Debug, PartialEq)]
pub struct HostSnapshot { pub host: String, pub online: bool, pub error: Option<String>, pub sessions: Vec<SessionInfo> }
pub const LISTING_SCRIPT: &str;                                  // POSIX sh; same data tmls collects
pub fn parse_listing(host: &str, output: &str) -> HostSnapshot;  // total: never panics on any input
pub fn listing_command(host: &str, local_name: &str) -> std::process::Command;
      // local: `sh -s`; remote: `ssh -o BatchMode=yes -o ConnectTimeout=5 HOST sh -s`; script on stdin

// poll.rs
pub struct Poller { /* … */ }
impl Poller { pub fn start(cfg: &Config, period: Duration, timeout: Duration,
                           on_snapshot: Box<dyn Fn(HostSnapshot) + Send>) -> Poller;
              pub fn refresh_now(&self);  /* Drop stops and joins every thread, kills running children */ }
```

- [ ] **Step 1: Failing tests** (`tests/listing.rs`, fixtures in `tests/data/`): `parses_a_real_archbox_listing`
      (capture one with the script; expected sessions written by hand from `tmux ls`), `status_rules_match_tmls`
      (one fixture per mark: busy Claude → Running; permission prompt → Waiting with reason; API error → Error;
      quiet > 30 s non-Claude → Idle; printed < 30 s ago → Running), `claude_title_and_context_percent`
      (200k vs 1M rule from the README), `tool_letters`, `no_tmux_server_is_online_with_no_sessions`,
      `garbage_and_truncated_output_never_panics` (random bytes, cut mid-line, empty), `names_with_spaces_colons_and_unicode`.
      Port the cases of `test_hosts.py` that apply; say in the report which were not ported and why.
- [ ] **Step 2:** `cargo test -p client-core` → fails to compile / FAIL.
- [ ] **Step 3: Implement** `config.rs` + `listing.rs` → green. Tests for config: missing dir, comments and blank
      lines in `hosts`, `name` override, `pins` round trip, bad JSON in `host-colors.json` = defaults.
- [ ] **Step 4: Failing tests** (`tests/poll.rs`, using a fake host command injected for tests — make the command
      factory a parameter): `snapshots_arrive_every_period`, **`slow_host_does_not_block_the_others`** (one fake
      host sleeps 60 s: the others deliver within one period, the slow one is reported offline after `timeout`, its
      child is killed), `drop_stops_threads_and_children` (no thread named `tmls-poll-*` and no child left).
- [ ] **Step 5: Implement** `poll.rs` → green. One thread per host; a snapshot is delivered only when it differs
      from the previous one or the host's online state changed.
- [ ] **Step 6:** `cargo clippy -p client-core --all-targets` clean; run the listing once for real against archbox
      and the PC (`ssh pcl`), paste both snapshots (names only) into the report.
- [ ] **Step 7:** Commits `client-core: config readers`, `client-core: host listing and status marks`,
      `client-core: per-host polling with timeout`.

---

### Task 2: `client-core` — tmux commands and seen-state

**Reference:** `/data/scratch/tmls-b1/wt-na/src/tmls/{tmux_ops.py,create.py,manage.py}` and their tests.

**Files:** Create `crates/client-core/src/{tmux.rs,seen.rs}`, `tests/tmux.rs`, `tests/tmux_real.rs`, `tests/seen.rs`.

**Interfaces — Consumes:** `SessionKey`, `Config` (Task 1). **Produces:**

```rust
// tmux.rs — every function returns argv, never a shell string, except attach_text (for the clipboard)
pub fn attach_argv(key: &SessionKey, local_name: &str) -> Vec<String>;
      // local: ["tmux","-u","attach","-t","=NAME"]; remote: ["ssh","-t",HOST,"tmux -u attach -t '=NAME'"] (quoted)
pub fn attach_text(key: &SessionKey, local_name: &str) -> String;       // what tmls's Copy button copies
pub struct NewSession { pub host: String, pub folder: String, pub name: String, pub start: Start,
                        pub size: Option<(u16, u16)> }
pub enum Start { Shell, Claude, Preset(Vec<String>) }
pub fn new_session_argv(req: &NewSession, local_name: &str) -> Vec<String>;
pub fn rename_argv(key: &SessionKey, new_name: &str, local_name: &str) -> Vec<String>;
pub fn kill_argv(key: &SessionKey, local_name: &str) -> Vec<String>;
pub fn derived_name_argv(host: &str, folder: &str, local_name: &str) -> Vec<String>;  // git repo name, else folder name
pub fn valid_session_name(name: &str) -> Result<(), String>;            // tmux forbids '.' and ':'; empty; too long
pub fn run(argv: &[String], timeout: Duration) -> anyhow::Result<String>;  // stdout; stderr in the error

// seen.rs
pub struct Seen { /* … */ }
impl Seen { pub fn apply(&mut self, snap: &mut HostSnapshot);  // Idle that finished since last look → Unseen (◆)
            pub fn opened(&mut self, key: &SessionKey);         // opening its tab clears ◆
            pub fn forget_missing(&mut self, snap: &HostSnapshot); }
```

- [ ] **Step 1: Failing tests** `tests/tmux.rs`: exact argv for local and remote of every builder;
      **`attach_and_ops_quote_hostile_names`** — names `a b`, `it's`, `x"y`, `$HOME`, `a;rm`, `日本`, and a leading
      `-`: the remote command string, parsed by `sh -c 'printf "%s\n" …'`, yields the name byte for byte;
      `valid_session_name_rejects_what_tmux_rejects`.
- [ ] **Step 2: Failing tests** `tests/tmux_real.rs` (local tmux on archbox, every session name starts with
      `tmlsrs-test-`, each test kills only what it created, in a drop guard): `create_list_rename_kill_round_trip`
      including the name `tmlsrs-test-a b'c`; `attach_argv_attaches` (spawn it on a PTY via `s2_core::pty`, see the
      shell prompt, detach with the tmux prefix `d`); `new_session_starts_in_the_folder`.
- [ ] **Step 3: Failing tests** `tests/seen.rs`: Running → Idle while not opened becomes Unseen; `opened` clears
      it; Waiting and Error are never overwritten; a session that disappears is forgotten; a renamed session is a
      new identity (documented limit).
- [ ] **Step 4: Implement** → green; clippy clean.
- [ ] **Step 5:** Commits `client-core: tmux command builders`, `client-core: unseen-completion tracking`.

---

### Task 3: `desktop` — window with tabs, each an attached terminal

**Reference in this repo:** `spikes/s2-wgpu/src/{app.rs,session.rs,marks.rs,surface.rs,input.rs,select.rs}` — the
single-terminal app. `session.rs`'s `Session` is the unit to hold once per tab. README "Attach in a tab".

**Files:** Create `crates/desktop/src/{main.rs,app.rs,tabs.rs,keys.rs,theme.rs,draw.rs}`, `tests/tabs.rs`, `tests/keys.rs`.

**Interfaces — Consumes:** `tmux::attach_argv`, `SessionKey` (Tasks 1–2); `s2_wgpu::session::Session`, `gfx::Scene`.
**Produces:**

```rust
// tabs.rs
pub struct Tabs { /* … */ }
pub enum TabState { Live, Ended { exit: Option<i32> } }
impl Tabs {
    pub fn open(&mut self, key: &SessionKey, grid: (u16, u16)) -> anyhow::Result<usize>; // existing tab → just select it
    pub fn select(&mut self, index: usize);  pub fn next(&mut self);  pub fn prev(&mut self);
    pub fn close(&mut self, index: usize);                 // detaches: drops the Emu; the tmux session lives on
    pub fn reattach(&mut self, index: usize) -> anyhow::Result<()>;
    pub fn active(&self) -> Option<usize>;  pub fn len(&self) -> usize;
    pub fn key(&self, index: usize) -> &SessionKey;  pub fn state(&self, index: usize) -> TabState;
    pub fn resize_all(&mut self, grid: (u16, u16));        // only the active tab is resized eagerly; others on select
}
// keys.rs
pub enum Focus { Terminal, Sidebar, Form }
pub enum Global { NextTab, PrevTab, FocusSidebar, FocusTerminal, CloseTab, NextWaiting, Quit, ZoomIn, ZoomOut, Reattach }
pub fn global(ev: &KeyEvent, focus: Focus) -> Option<Global>;
      // Alt+Shift+Right/Left tabs · Alt+Shift+Up sidebar · Esc (in sidebar) terminal · Alt+Shift+N next waiting ·
      // Ctrl+Q quit · Ctrl+= / Ctrl+− zoom · Enter on an ended tab reattach. Everything else with Terminal focus
      // goes to the active tab's Session unchanged (Tab, Ctrl+C, Alt+letters included).
```

- [ ] **Step 1: Failing tests** `tests/tabs.rs` (real PTYs, child = `sh -c` scripts, no tmux needed): `open_twice_selects_the_existing_tab`;
      `typing_goes_only_to_the_active_tab` (two tabs running `cat > file`; bytes land in one file);
      **`a_tab_whose_child_exits_stays_and_can_reattach`** (child `true`: state `Ended`, tab count unchanged,
      `reattach` gives a Live tab; other tab unaffected); `close_drops_the_emu_and_keeps_the_others`
      (no `s2-pty-*` thread of the closed tab remains); `inactive_tabs_keep_parsing_output` (switch back → screen
      current); `resize_reaches_the_active_pty` (`stty size`).
- [ ] **Step 2: Failing tests** `tests/keys.rs`: every binding above per focus; `terminal_focus_passes_everything_else`
      (table of 30 keys incl. Tab, Esc, Ctrl+C, Alt+f, Ctrl+Tab, F-keys → `None`).
- [ ] **Step 3: Implement.** Layout: `[sidebar 28 cols][tab bar 1 row / terminal / status line 1 row]`. Tab bar:
      status mark + name, host colour bar, `×`; mouse click selects, `×` closes. An ended tab shows
      `session ended — Enter to reattach` centred. `--bench DIR` writes one latency log for the **active** tab
      (same format and marks as `s2-wgpu`, `meta.app = "tmls-rs"`). The sidebar in this task is a plain list of
      `SessionKey`s from the poller (click → `Tabs::open`); Task 4 replaces its model and drawing.
- [ ] **Step 4:** `cargo test -p desktop` in Docker (`tooling/docker/build.sh test -p desktop`; if font-dependent
      tests cannot run in the image, run the pure tests on archbox with `cargo test -p desktop --test tabs --test keys`
      and say which ran where). Clippy clean. Docker release build + `ldd` check.
- [ ] **Step 5:** Commits `desktop: tab model over s2-core sessions`, `desktop: window, tab bar, global keys`.

---

### Task 4: `desktop` — the tmls sidebar

**Reference:** `/data/scratch/tmls-b1/wt-na/src/tmls/app.py` (pane-mode sidebar: row layout, sections, legend,
button bar, colours in its CSS), `host_colors.py`, `keys.py`; the redesign notes in
`/home/sgkayast/Others/tmls/PROGRESS.md` entry "2026-10-09 (eve)". The screenshot shows the older look — where they
differ, the code on `feat/native-attach` wins.

**Files:** Create `crates/desktop/src/sidebar.rs`; modify `draw.rs`, `theme.rs`, `app.rs`, `keys.rs`; `tests/sidebar.rs`, `tests/sidebar_draw.rs`.

**Interfaces — Consumes:** `HostSnapshot`, `SessionInfo`, `Status`, `Tool`, `Seen`, `Config` (pins, colours).
**Produces:**

```rust
pub enum Row { Host { host: String, online: bool, color: HostColor }, Session(SessionInfo), Gap }
pub struct Sidebar { /* … */ }
impl Sidebar {
    pub fn apply(&mut self, snap: HostSnapshot);                 // replaces that host's rows; keeps selection by key
    pub fn rows(&self) -> &[Row];  pub fn selected(&self) -> Option<&SessionKey>;
    pub fn move_by(&mut self, delta: i32);  pub fn select_key(&mut self, key: &SessionKey);
    pub fn click(&mut self, row: usize) -> Option<Action>;  pub fn activate(&self) -> Option<Action>;
    pub fn toggle_pin(&mut self) -> bool;  pub fn next_waiting(&self, after: Option<&SessionKey>) -> Option<SessionKey>;
    pub fn scroll(&mut self, rows: i32);  pub fn view(&self, height: usize) -> std::ops::Range<usize>;
}
pub enum Action { Attach(SessionKey), NewSessionOn(String), Menu(SessionKey) }
```

Row drawing (28 columns): `mark  tool-letter  name ★` then, for a Claude session with a title, a dim second line
`title … 58%` (context % orange from 70, red from 90). `◉2` when other clients are attached. Host header: full-width
line in the host colour, name, `+` at the right end, *offline* dimmed. Bottom: legend (`● running  ? waiting
◆ done  ✕ error  ○ idle`) and a one-row button bar (`New  Open  Copy  ?`). The active tab's session row is
highlighted. Colours and mark glyphs from the tmls CSS, as `theme.rs` constants with the source line cited.

- [ ] **Step 1: Failing tests** `tests/sidebar.rs`: `hosts_in_config_order_local_first`; `offline_host_keeps_its_header_and_drops_sessions`;
      **`selection_follows_the_session_when_rows_move`** (a session inserted above, a rename elsewhere, the
      selected one killed → selection moves to the nearest row of the same host, never to a `Host`/`Gap` row);
      `move_by_skips_headers_and_clamps`; `pins_sort_and_persist`; `next_waiting_order` (? before ◆, wraps);
      `view_keeps_the_selection_visible`; `two_line_rows_count_two` (hit-testing by pixel row).
- [ ] **Step 2: Failing tests** `tests/sidebar_draw.rs` (instances, not pixels, plus one offscreen render saved to
      `/data/scratch/tmls-rs-v0/sidebar.png`): mark colour per status; name truncated with `…` at 28 columns incl.
      wide characters; context colour thresholds; header colour = host colour; selected and active-tab rows differ.
- [ ] **Step 3: Implement**; wire `Poller` → `Seen::apply` → `Sidebar::apply` → redraw. Sidebar keys (focus
      Sidebar): `j`/`k`/arrows, Enter attach, `p` pin, `n` next waiting, Esc back to the terminal. Mouse: click
      attach, middle-click pin, wheel scroll, right-click → `Action::Menu`, click `+` → `Action::NewSessionOn`.
      Opening a tab calls `Seen::opened`. Window title `session · host`.
- [ ] **Step 4:** tests green (same Docker/archbox split as Task 3), clippy clean. Look at
      `/data/scratch/tmls-rs-v0/sidebar.png` next to `/home/sgkayast/Others/tmls/docs/screenshots/tmls.png` and
      describe the differences in the report.
- [ ] **Step 5:** Commits `desktop: sidebar model`, `desktop: tmls-style sidebar drawing and keys`.

---

### Task 5: End-to-end harness and the first full run (gate for "Must")

**Files:** Create `tooling/smoke/Dockerfile.smoke`, `tooling/smoke/run.sh`, `tooling/smoke/e2e.sh`,
`crates/desktop/tests/routing.rs`; `docs/desktop-v0.md`.

- [ ] **Step 1: Failing test** `tests/routing.rs`: **`sidebar_and_form_keys_reach_no_pty`** — with two live tabs
      (`cat > file`), 40 key events with focus Sidebar and (once Task 6 exists, extended there) Form: both files
      stay empty and no latency `key` line is written; with focus Terminal the same keys land in the active file.
- [ ] **Step 2:** `Dockerfile.smoke`: `FROM tmls-rs-build:latest` + sway (or weston) headless, mesa lavapipe,
      `wtype`, `grim` (screenshots), tmux, openssh-client, one monospace font, one CJK font, one colour-emoji font.
      `run.sh` builds the image and the release binary. (The earlier smoke setup was deleted on 2026-10-11; this
      one lives in the repo.)
- [ ] **Step 3:** `e2e.sh`, inside the container, with its own tmux server on a private socket dir and `HOME` set
      to a scratch dir (so the user's tmux and config are never seen): create `tmlsrs-test-one` and
      `tmlsrs-test-a b'c`; write a `hosts`-less config; start `tmls-rs --bench …`; assert via `grim` screenshots +
      the latency log + tmux itself: both sessions listed; click/Enter attaches; typed `echo hi-<nonce>` appears in
      `tmux capture-pane`; second tab opens; Alt+Shift+Left/Right switch; `tmux kill-session` of one → its tab says
      ended; `exit` path clean; every terminal key attributed in the log; no panic on stderr.
- [ ] **Step 4:** Run it; save screenshots to `/data/scratch/tmls-rs-v0/e2e/`; **look at every screenshot** and
      describe what is on it in the report (blurry, clipped or overlapping text is a failure to fix or to list).
- [ ] **Step 5:** `docs/desktop-v0.md`: what v0 is and is not, build, run, config files read, key bindings table.
- [ ] **Step 6:** Commits `smoke: headless end-to-end harness for tmls-rs`, `desktop: key routing test`, `docs: desktop v0`.

**After Task 5 the controller checks the clock.** Past 06:30 → Task 9.

---

### Task 6: New session, rename, kill, Open, Copy  *(Should)*

**Reference:** `/data/scratch/tmls-b1/wt-na/src/tmls/{create_form.py,create.py,manage.py}`; README "New session",
"Rename or kill", "Open", "Copy". Reuse `s2_wgpu::ui::{textfield, menu, focus}` for the field and menu logic.

**Files:** Create `crates/desktop/src/forms.rs`; modify `draw.rs`, `app.rs`, `keys.rs`; `tests/forms.rs`; extend `tests/routing.rs`.

**Interfaces — Produces:**

```rust
pub enum Form { New(NewForm), Rename { key: SessionKey, field: TextField }, ConfirmKill { key: SessionKey, running: Vec<String> } }
pub struct NewForm { pub host: usize, pub folder: TextField, pub name: TextField, pub start: usize, pub name_edited: bool }
pub enum FormResult { Pending, Cancelled, Submit(Request) }
pub enum Request { New(NewSession), Rename(SessionKey, String), Kill(SessionKey) }
impl Form { pub fn key(&mut self, ev: &KeyEvent) -> FormResult;  pub fn click(&mut self, x: f32, y: f32) -> FormResult; }
```

- [ ] **Step 1: Failing tests** `tests/forms.rs`: Tab order host → folder → name → start → Create → Cancel; name
      follows the folder (git-derived via `derived_name_argv`, injected) until edited, then stays; invalid names
      (`a.b`, `a:b`, empty) block Create with a message; Start lists Shell, `claude`, then presets, default = first
      preset else `claude`; rename pre-fills the old name; kill needs an explicit confirm and shows running
      commands; Esc cancels every form.
- [ ] **Step 2: Implement.** Requests run on a worker thread (`tmux::run`, 10 s timeout); the result line shows in
      the status line; success → `Poller::refresh_now` and, for New, open its tab; Rename reopens the tab under the
      new name; Kill closes the tab. Row menu (`⋯` / right-click): Rename, Kill, Open in new terminal
      (`$TERMINAL`, else `kitty`, else `x-terminal-emulator`, with the attach argv), Copy attach command.
      New sessions are created at the view's size.
- [ ] **Step 3:** Extend `e2e.sh`: create `tmlsrs-test-new` through the form with `wtype`, rename it, kill it;
      assert with `tmux ls`. Extend `sidebar_and_form_keys_reach_no_pty` to Form focus.
- [ ] **Step 4:** Commits `desktop: new-session form`, `desktop: rename, kill, open, copy`.

---

### Task 7: Mouse to tmux, selection, links  *(Should)*

**Reference:** README "Mouse in the terminal", "Links in the terminal"; `/data/scratch/tmls-b1/wt-na/src/tmls/term.py`
(mouse mapping, Ctrl+C copy-or-interrupt, trailing-space trim).

**Files:** Modify `spikes/s2-core/src/{emu.rs,keys.rs}` (additive: `InputMsg::Mouse`), the contract overview,
`crates/desktop/src/{app.rs,tabs.rs}`; create `crates/desktop/src/links.rs`; tests in both crates.

- [ ] **Step 1: Failing tests (core)**: with mouse reporting on (`printf '\e[?1000h\e[?1006h'`), `InputMsg::Mouse`
      press/release/wheel/drag produce the SGR bytes the program reads back (`cat -v`); with reporting off they
      produce nothing. Use libghostty-vt's mouse encoder if the crate exposes one; otherwise encode SGR 1006 and
      X10 by hand and say so.
- [ ] **Step 2: Implement** `InputMsg::Mouse { kind, button, col, row, mods }`; contract note in the same commit.
- [ ] **Step 3: Failing tests (desktop)**: wheel with reporting on → mouse events to the PTY (tmux scrolls its
      history); reporting off → local viewport scroll as today; plain drag with reporting on goes to the program,
      **Ctrl+drag or Shift+drag always selects locally** and copies on release; Ctrl+C copies when a selection
      exists, else sends `\x03`; copied text loses trailing spaces per line; `links::at(row_text, col)` finds URLs
      (and `localhost` stays `localhost`), Ctrl+click opens with `xdg-open` (injected in tests).
- [ ] **Step 4: Implement**; extend `e2e.sh` with a wheel/scroll check if the headless seat gets a pointer
      (sway `seat * pointer` via `wlrctl`/`ydotool` is not available there → say it was not exercised).
- [ ] **Step 5:** Commits `s2-core: mouse reports to the PTY`, `desktop: mouse to tmux, selection rules, links`.

---

### Task 8: Alerts  *(Stretch)*

**Reference:** README "🔔 Alerts" (without the Yes/No approval buttons — approvals are out of v0);
`/data/scratch/tmls-b1/wt-na/src/tmls/notifications.py`.

- [ ] Model `alerts.rs`: a session turning `?`, `◆` or `✕` adds an entry (newest first, max 50); bell with unread
      count in the button bar; panel lists them, click/Enter jumps to the session; "silence focused" skips the
      active tab. Desktop notification via `notify-send` for `?` and `◆` (command injected in tests; if the binary
      is missing, no error). Settings are **read** from `~/.config/tmls/notifications.json`, never written.
- [ ] Tests: transitions that alert / do not alert; unread count; jump; silence-focused; missing `notify-send`.
- [ ] Commit `desktop: alerts list and desktop notifications`.

---

### Task 9: Test everything a machine can test

The controller runs this itself where it is commands and reading results; fixes go to one implementer dispatch.

- [ ] **Step 1 (archbox):** `cargo test -p client-core -p s2-core -p s2-wgpu` and the `desktop` tests; clippy on
      all four; `tooling/smoke/e2e.sh` three times in a row (flaky = a finding). Totals in the ledger.
- [ ] **Step 2 (archbox, real data, read-only):** run `tmls-rs` in the smoke container **with the real host list**
      (mount `~/.config/tmls` and `~/.ssh` read-only, archbox reached over ssh as in tmls's Docker setup; if ssh
      from the container is not possible, run only the listing: `cargo run -p client-core --example list`).
      Compare the session names and marks with `tmls`'s own listing (`ag ls` on archbox). Attach to **no** real
      session from the test; screenshot the sidebar only.
- [ ] **Step 3 (PC, only if allowed — see Unattended rules):**
      `ssh pcl 'cd ~/Others/tmls-rs && git fetch && git checkout --detach origin/feat/desktop-v0 && tooling/docker/build.sh build --release -p desktop -p s2-wgpu && ldd target-docker/release/tmls-rs | grep "not found"'`.
      (`feat/desktop-v0` is visible to the PC because its `origin` is the archbox repo.)
- [ ] **Step 4 (PC desktop, guarded):** only if `loginctl` shows the session unlocked. Environment for GUI commands
      over ssh: `XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-0 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus`.
      a. Start `s2-wgpu --bench ~/s2-logs/night-ui --cmd bash` and, separately, with `S2_WGPU_NO_UI=1`
         (`night-noui`). **Input rule:** `ydotool` types into whatever window has focus, which may be the user's
         Claude session. Inject only after the app's own stderr shows it has keyboard focus (add a
         `s2-wgpu: focus true|false` stderr line if it has none), type only lowercase letters and Enter, in bursts
         of ≤ 20 keys, re-checking focus before each burst; if focus is never reported within 5 s or is lost,
         **inject nothing**, close the app, record "not run". ~300 keys per run, then `exit`.
      b. Same for `tmls-rs --bench ~/s2-logs/night-tmlsrs` attached to a `tmlsrs-test-` session created on the PC
         for the test (and killed after).
      c. `printf 'ab你好cd\n😀 ok\n'` in the app, screenshot with `spectacle -b -n -f -o FILE`; copy the PNG and the
         logs to `/data/scratch/tmls-rs-v0/pc/`; look at the PNG (CJK placement, emoji colour, crispness at 1.15).
      d. Report per run: `meta` (`hz` should be 165 with `hz_source current_monitor`), keys sent/attributed,
         key→present p50/p95/p99, submit→present p50 with and without the sidebar.
      e. Close every window the test opened; kill only `tmlsrs-test-*` sessions; leave the user's windows alone.
- [ ] **Step 5:** Fill the machine-doable items of `docs/spikes/S2-checklist-wgpu.md` (1.0 hash and `ldd`, the
      emoji line of 1.3, the `hz` items and the 115 % row of the bench table) with the measured values, marked
      "(measured unattended, 2026-10-11)". Leave every item that needs a person empty. Commit `docs: S2 checklist — machine-run items`.

---

### Task 10: Final review, merge, hand-off

- [ ] **Step 1:** Whole-branch review (`opus`) of `main..feat/desktop-v0` with the ledger's deferred minors and
      rulings; **one** fix dispatch; one scoped re-review; park the rest with rulings.
- [ ] **Step 2:** Tests green at the final head → merge `feat/desktop-v0` → `main` (`--no-ff`). If the final review
      has an open Critical, do **not** merge; say so in the hand-off.
- [ ] **Step 3:** Write `docs/MORNING-TEST.md` for the user — plain steps, 15 minutes: build line for the PC, start
      line (`~/Others/tmls-rs/target-docker/release/tmls-rs`), then: sessions of both hosts listed with the right
      marks? click → attaches? type, Tab completes, Ctrl+C? Alt+Shift+arrows? new / rename / kill on a throw-away
      session? wheel scroll, drag copy? fcitx5 once in the terminal? Each with a blank to fill and "if it fails,
      write what you did". List what was built, what was not, and every `Ruling:` from the ledger with its cost.
- [ ] **Step 4:** `PROGRESS.md` entry (progress-log skill; newest first) — commit it with the merge this time.
- [ ] **Step 5:** Telegram ping: status, the path of `docs/MORNING-TEST.md`, the three most important caveats.
- [ ] **Step 6:** Leave the SDD workspace and `/data/scratch/tmls-rs-v0/` in place (deleting needs the user's yes).

## Done when

- `tmls-rs` on archbox's headless container passes `e2e.sh` three times in a row.
- `cargo test` green for `client-core`, `desktop`, `s2-core`, `s2-wgpu`; clippy clean.
- Tasks 1–5 merged at minimum; whatever of 6–8 was finished and reviewed.
- `docs/MORNING-TEST.md`, `docs/desktop-v0.md`, the `PROGRESS.md` entry and the Telegram ping exist.

## Self-review of this plan (2026-10-11)

- PLAN §8 parity: sessions per host + offline ✔ T1/T4; marks, Claude title, context % ✔ T1/T4; tabs +
  Alt+Shift keys + list keys ✔ T3/T4; Open / Copy ✔ T6; mouse wheel/drag/Ctrl+C/trim ✔ T7; paste ✔ (s2-wgpu);
  links (URL only) ✔ T7; new / rename / kill ✔ T6; notifications ✔ T8 (no approvals). **Not covered, on purpose**
  (Decision 3): kitty sessions, `file:line` → viewer, quick select, image paste, splits menu, `ag`, `ct`, Ask,
  approvals, prompts, web, Android, config migration.
- Types used across tasks are defined once: `SessionKey`, `Status`, `Tool`, `SessionInfo`, `HostSnapshot` (T1);
  `NewSession`, `Start` (T2); `Tabs`, `Focus`, `Global` (T3); `Sidebar`, `Row`, `Action` (T4); `Form`, `Request` (T6).
- This plan gives interfaces, test names with their behaviour, and references to the Python source — not full
  implementation code per step. That is deliberate (the reference implementation exists and tonight's tasks ran
  well from briefs of this shape), and it means implementers must be `opus`, not a cheaper tier.
