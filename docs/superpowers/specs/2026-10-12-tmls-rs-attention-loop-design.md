# tmls-rs: attention loop and reliability — Design

Date 2026-10-12. Branch `feat/attention-loop` from `main` at `d235ee3`. Unattended overnight run; the user said
"work over night" after approving the scope below (Tier 1 + Tier 2 of the gap list, 2026-10-11 session).

## Intent

tmls is the user's productivity tool: one window that shows every Claude / Codex / opencode session on every host,
says which one needs them, and lets them act. tmls-rs (native, wgpu, ssh + tmux) replaces the Python tmls, which the
user still runs daily on the PC because it does more. This round closes the gap that matters daily:

1. **See it sooner** — status changes reach the sidebar in well under a second (today: a 5 s poll).
2. **Act without switching** — answer a permission prompt, or send / queue a message, from the list.
3. **Trust it** — copy/paste works and says why when it does not; a sleeping link does not freeze a tab.

Out of scope (next round): quick select, image paste, list filter, splits in the row menu, markdown panel,
thumbnails, game-mode polling, sound, background tmux servers, native sketch board.

What the user said vs. what I assumed: the user picked the tiers ("Okay") and asked for unattended work. Every
design decision below is mine; each is listed in "Decisions" at the end so they can be vetoed in the morning.

## Features

### F1 — `?` help panel

The `?` foot button (today a no-op at `app.rs:1731`) and `?` pressed in the list open a read-only panel listing
the keys and mouse actions, grouped: Everywhere / List / Terminal / Panel / Forms. One source of truth: a
`keys::HELP: &[(&str, &[(&str, &str)])]` table (group, [(chord, what)]) rendered into a `forms::Panel`; the same
table is printed by `tmls-rs keys` (new CLI flag) so the docs cannot drift. Esc, `?`, or a click outside closes it.

### F2 — copy / paste that works and talks

Reported broken on the PC (KDE Wayland). The clipboard is `arboard` without its `wayland-data-control` feature, so
on Wayland it goes through X11 — on a Wayland-only session that fails silently (errors only reach stderr).

- Enable `arboard` feature `wayland-data-control`.
- Fallback when arboard errors: `wl-copy` / `wl-paste --no-newline` as child processes (if present on `PATH`).
- Every outcome reaches the status line through `App::say`: "copied N chars", "nothing selected", "pasted N chars",
  "clipboard empty", "copy failed: <error>", "paste failed: <error>".
- Ctrl+C with a selection keeps its current meaning (copy, no interrupt).
- Test: a unit test for the status texts; the e2e types text into a tab, Shift+drags over it, presses Ctrl+Shift+C,
  then Ctrl+Shift+V into a second tab and checks the bytes arrived (`wl-copy` is in the smoke image; arboard's
  Wayland backend is exercised under headless sway).

### F3 — ssh keepalive and ControlMaster

All ssh argv built in `client-core::tmux` (`ssh`, `attach_argv`, the listing command, the new push tail) get:

```
-o ServerAliveInterval=15 -o ServerAliveCountMax=3
-o ControlMaster=auto -o ControlPersist=10m -o ControlPath=<runtime>/tmls-rs/cm-%C
```

`<runtime>` = `$XDG_RUNTIME_DIR` else `~/.cache/tmls-rs`; the directory is created at start with mode 0700. A dead
link is detected by the master within 45 s; the attached tab then shows the existing "ended — Enter reattaches"
state instead of staying frozen. One builder `tmux::ssh_base(host) -> Vec<String>` owns the options so no call site
drifts. `TMLS_RS_NO_CONTROLMASTER=1` turns ControlMaster off (keepalive stays) for debugging.

### F4 — pane id, unread mark

- `SessionInfo` gains `pane: Option<String>` (the `%N` from the Claude status file's `tmux` field) — needed by F5/F7
  to target the right pane. Parsed in `listing.rs`; `None` for non-Claude sessions.
- **Unread**: a session whose output changed (listing `window_activity`) since the user last looked at it. "Looked"
  = its tab was front while the window had focus. `seen::Seen` tracks `looked_at: HashMap<SessionKey, u64>`
  (activity timestamp at last look) and exposes `unread(&key) -> bool`. The row draws `•` after the name (before
  `★`); legend gets `• unread`. Unread never fires for the front focused tab, for the first listing of a session,
  nor for `not in tmux` rows. Alt+Shift+N (next waiting) also visits unread sessions when nothing waits, as tmls does.

### F5 — approvals from the list

For a session Waiting with reason `permission prompt`:

- Entry points: row menu item **Approve…** (shown only then), key `y` on the cursor row in the list, key `y` on the
  selected alert in the alerts list.
- Flow (as tmls `approve.py`): run `tmux capture-pane -p -t <pane or =NAME:>` on the host; find the last line equal to
  `Do you want to proceed?`; the dialog body = lines from the last `──` line above it to the question, minus blank
  lines and lines starting with `╌` or `Tip:`. If not found: `say(Err("NAME isn't asking anything"))`.
- A `Form::Approve { key, dialog: Vec<String>, focus }` shows the dialog lines (tool, description, command — capped
  at 12 lines, middle elided) with buttons **Yes** / **No** / **Cancel**. Enter = Yes, `n` = No, Esc = Cancel.
- On Yes/No: re-capture; if the dialog differs from what was shown → `Err("NAME isn't asking that any more")`,
  nothing sent. Else `tmux send-keys -t T 1` (Yes) or `tmux send-keys -t T Escape` (No). Then `refresh_now()`.
- All remote work runs off the UI thread (`forms::perform` pattern, `REQUEST_TIMEOUT`).
- Tests: pure `approve::request(screen) -> Option<Vec<String>>` and `approve::same(a, b)` unit-tested on captured
  dialog texts (one real capture pasted as a fixture); the e2e runs a shell script in a `tmlsrs-test-` session that
  prints a fake dialog and `read`s one key, fakes the Claude status file as waiting/permission prompt, approves from
  the list, and checks the script received `1`.

### F6 — push status (events.jsonl) and last reply

The Python tmls branch `feat/native-attach` installs Claude hooks that append one JSON line per event to
`~/.cache/tmls/events.jsonl` on each host (`t, ev, sid, pane, cwd`, plus `msg`/`bg`/`err`/`tool`/`nt`). tmls-rs
**only reads** that file; it never installs or changes hooks (that stays `tmls hooks install`). If the file is
absent on a host the sidebar's host header shows `· poll` dimly and everything works as today.

- `client-core::push`: one long-lived process per online host: `tail -n0 -F ~/.cache/tmls/events.jsonl` (locally
  `tail`, remotely via `ssh_base`). Lines parsed to `PushEvent { t, sid, pane, kind }`,
  `kind ∈ {Busy, Waiting(reason), Stop{msg, bg}, Failed(msg), SessionStart, SessionEnd, Other}`. Non-JSON lines
  and unknown `ev` are ignored. The process is restarted 10 s after it exits; a host that goes offline stops it.
- Mapping (as tmls `push.apply`): `UserPromptSubmit`, `PreToolUse` → Running; `PermissionRequest`, or
  `Notification` with `nt == "permission_prompt"` → Waiting("permission prompt"); `Stop` → Idle (Running if `bg > 0`),
  `last_reply = msg`; `StopFailure` → Error(reason = err); `SessionStart`/`SessionEnd` → `Poller::refresh_now()`.
- Apply: the app keeps the latest `HostSnapshot` per host. An event matches a session by `sid == session_id`, else
  by `pane`. A matching event with `t` newer than the snapshot's listing time updates that `SessionInfo` in place,
  then the usual path runs (`seen.apply`, `alerts.observe`, queue delivery F7, redraw) — so notify-send and the `?`
  mark arrive in ~100–200 ms. The next poll snapshot replaces push state wholesale (poll stays the truth).
- `SessionInfo` gains `last_reply: Option<String>` (push only; first 120 chars, one line). The row's dim second line
  shows: waiting reason if Waiting, else the last reply if any, else Claude's title if it differs from the name.
- Tests: `push::parse_line` and the mapping are unit-tested on the event examples from `claude_hook.py`; the e2e
  appends lines to a fake `~/.cache/tmls/events.jsonl` and checks the mark and the dim line change without a poll
  (poll period is set long via `TMLS_RS_POLL_S=60` for that check), and that `notify.log` got the line.

### F7 — Ask: send or queue a message

- Entry: key `s` on the cursor row, row menu **Send message…**, Alt+Shift+A from the terminal (front tab's session).
  Claude sessions only (status file present); others get `Err("NAME is not a Claude session")`.
- `Form::Ask { key, field: TextField, saved: Vec<String>, pick: Option<usize>, queued: usize, focus, error }`: a
  one-line field, below it the saved prompts from `~/.config/tmls/prompts` (one per line; default
  `What's the progress?`), ↑/↓ pick one into the field, Enter sends, Esc cancels, button **Clear queue** when
  `queued > 0`. Title: `Send to NAME · N queued`.
- Send: `tmux load-buffer -b tmls-<hex8> - && { tmux paste-buffer -p -d -b B -t T || { tmux delete-buffer -b B; false; }; } && sleep 0.2 && tmux send-keys -t T Enter`,
  text on stdin. New `tmux::run_with_stdin(argv, stdin, timeout)`; `T` = pane else `=NAME:`.
- Queue: `queue: HashMap<SessionKey, VecDeque<String>>` in the app (memory only, as tmls). If the session is Running
  or Waiting the text is queued (`say("queued for NAME (N waiting)")`), else sent now. Delivery: when a session's
  status goes from Running/Waiting to Idle/Unseen (from poll or push), send the queue's head; the next one goes on
  the next such transition. Error → keep the queue, `say(Err)`. Rename moves the queue; kill drops it.
- Row shows `N✉` before the mark while queued. Legend gets `✉ queued`.
- Tests: `tmux::send_text_script` unit test (exact script, buffer name), queue state machine unit-tested on a status
  sequence; e2e sends "echo tmls-ask-ok" to an idle shell session and checks the output, then queues one while a
  fake Claude status says busy, flips the status file to idle, and checks delivery.

### F8 — two small losses fixed

- A failed "new session" reopens the form with the typed values and the error (today the text is lost).
- A session renamed outside the app: on a poll where a tab's name is gone and a session with the same `session_id`
  (Claude) appears under another name on the same host, the tab is retargeted (label, key, pins follow). No match →
  today's behaviour ("ended").

## Architecture

- `client-core` (no UI): `tmux::ssh_base`, `tmux::run_with_stdin`, `tmux::send_text_script`, `tmux::capture_argv`,
  `tmux::send_keys_argv`; new `push.rs` (tail process + parser + mapping) and `approve.rs` (pure text functions);
  `listing.rs` parses `pane`; `seen.rs` gets the unread map. All additive; existing tests unchanged.
- `desktop`: `forms.rs` gains `Form::Ask` and `Form::Approve` and two menu items; `app.rs` gains the queue, the push
  receivers (one `UserEvent::Push(host, PushEvent)` per line, coalesced per frame), the help panel, the clipboard
  messages, F8; `draw.rs` draws `•`, `N✉`, `· poll`, the legend entries; `keys.rs` gains `HELP`, `?`, `y`, `s`,
  Alt+Shift+A.
- Threads: push tails are one thread per host reading the child's stdout line by line and posting through the winit
  proxy, like the poller. Nothing new on the key-to-frame path; the panel/forms are drawn only while open.
- Errors: every remote failure ends in `say(Err(..))` (status line + stderr); nothing panics on a dead host.

## Testing and verification

- TDD per task: failing unit test first, then code. Desktop tests in Docker (`tooling/docker/build.sh test -p desktop`),
  `client-core` tests on archbox, clippy `-D warnings` on both, `cargo fmt -p desktop --check`.
- e2e: `tooling/smoke/e2e.sh` gains one block per feature (F2, F5, F6, F7, F8 rename), run as
  `SMOKE_OUT=/data/scratch/tmls-rs-v0/night2/e2e-N tooling/smoke/run.sh`; the whole suite must pass three times.
- Speed: `tooling/smoke/bench-run.sh` with `BENCH_STATE=closed` before and after; p95 key-to-frame and idle frame count
  must not move.
- Real window: nothing is injected into the user's KDE session. A hand-off sheet `docs/NIGHT-TEST.md` lists what to try.

## Decisions (veto any)

| # | Decision | Why |
|---|---|---|
| D1 | Push reads tmls's `events.jsonl`; hooks stay installed by `tmls hooks install` | tmls-rs must not edit `~/.claude/settings.json` on your hosts unattended |
| D2 | Push tail = `tail -F` over ssh, not a port of `agent.py` | 1 process, no Python on the host, rotation handled by `-F`; ctx % still comes from the poll |
| D3 | Poll stays the truth; push only moves state forward until the next snapshot | No second source of session identity; a lost event costs ≤ 5 s |
| D4 | Approvals: `1` = Yes, `Escape` = No, re-capture before sending (tmls's rule) | The number for "No" varies by prompt; Esc always declines |
| D5 | Ask has saved prompts, no "recent" list | Recent needs a transcript grep per open; cheap to add later |
| D6 | Queue is in memory, lost at quit | Same as tmls; a file brings stale sends after a restart |
| D7 | Unread = `•` after the name, no pulse / spinner | Zero cost when idle; animation is a redraw per tick |
| D8 | ControlMaster on by default, `TMLS_RS_NO_CONTROLMASTER=1` off | Faster commands and attach; tmls's branch runs it daily |
| D9 | Keys: `y` approve, `s` send, `?` help in the list; Alt+Shift+A ask from the terminal | Free in the list today; Alt+Shift chords are the app's own |
| D10 | Last reply shown only when push is on | Transcript parsing in POSIX sh is fragile; push has the text for free |
