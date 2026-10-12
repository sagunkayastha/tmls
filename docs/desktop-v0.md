# tmls-rs desktop v0

`tmls-rs` is one native window (winit + wgpu) for your tmux sessions. The list on the left
shows the sessions of this machine and of your ssh hosts. Each session you attach opens in a
tab. It is a port of the sidebar and tabs of the Python app `tmls`.

This page describes the code as it is on branch `feat/desktop-v0`. Where something is drawn
but does nothing yet, it says "not built yet".

## What v0 does

- Lists tmux sessions per host, under a header for each host. The list is read again every 5 s.
  A host that does not answer shows as `· offline`; the other hosts are not held up.
- Shows a status mark per session: `●` running, `?` waiting, `◆` done, `✕` error, `○` idle.
- Attaches a session in a tab (`tmux attach` here, `ssh HOST tmux attach` for a remote host).
  Opening a session that has a tab brings that tab to the front.
- Keeps a tab when its session ends or ssh drops. The tab says
  `session ended — Enter to reattach`; the other tabs keep working.
- Closing a tab detaches. The tmux session lives on.
- Pins sessions (`★`). Pinned sessions sort first within their host.
- Makes a new session from a form, renames a session, and kills one after a confirm.
- Opens a session in a new terminal window, and copies its attach command.
- Alerts: notes every session that turns `?`, `◆` or `✕`, counts the new ones on the bell
  (`!3`), lists them, and sends a desktop notification for `?` and `◆`. See "Alerts".

## Not in v0

`ag`, `ct`, the file viewer, image paste, quick select, the
splits menu, kitty (non-tmux) sessions, web, Android, Windows.

## Build

On archbox the GUI crates build in Docker:

```
tooling/docker/build.sh build --release -p desktop
```

The binary is `target-docker/release/tmls-rs`.

## Run

```
target-docker/release/tmls-rs [HOST…] [--bench DIR]
```

- `HOST…` — ssh hosts to list instead of those in `~/.config/tmls/hosts`. This machine is
  always listed when it has tmux.
- `--bench DIR` — writes a latency log to `DIR/latency.jsonl`. Only the first tab opened is
  logged.
- `--foreground` (or `--fg`) — stay attached to the terminal (see below).

`tmls-rs` returns the shell at once, like a desktop app: it starts itself again as its own
process in a new session (no controlling terminal, stdin from `/dev/null`, same working
directory) and exits; closing the terminal leaves the window up. Nothing is printed. Its
stdout and stderr (errors, adapter and focus lines) go to `$XDG_CACHE_HOME/tmls-rs/tmls-rs.log`,
else `~/.cache/tmls-rs/tmls-rs.log`, emptied at each launch, so it holds the last run only.

It stays in the foreground instead, with its output on the terminal, when given
`--foreground`/`--fg`, with `--bench`, with `TMLS_RS_FOREGROUND=1` in the environment, for the
`tmls-rs keys` subcommand, when the arguments are wrong (the usage is printed), or when the log
file cannot be opened (the reason is printed). `tmls-rs HOST…` detaches like `tmls-rs`.

Hosts are reached with your own ssh setup (keys, `~/.ssh/config`). Listing never asks for a
password: a host that needs one shows as offline. Attaching runs ssh inside the tab, so a
prompt from ssh shows there.

`tooling/desktop/install.sh [/abs/path/to/tmls-rs]` puts the icon and `tmls-rs.desktop` under `~/.local/share` (`uninstall.sh` removes them).
Once installed, the icon (Rem's face) and the name show in the task bar and the launcher: Wayland takes the icon from the desktop entry via the window's `app_id` `tmls-rs`; the embedded icon is for X11.

## Config files

All in `~/.config/tmls/`, the same files `tmls` uses. Every one is optional.

| File | Use | Access |
| --- | --- | --- |
| `hosts` | ssh hosts to list, one per line; `#` starts a comment | read |
| `name` | what this machine is called in the list (default: its hostname) | read |
| `host-colors.json` | accent colour per host | read |
| `agent-presets.json` | agent commands for a new session: the form's Start choices after `shell` and `claude` | read |
| `notifications.json` | the alert switches `desktop` and `silence_focused` (both on by default) | read |
| `pins` | pinned sessions, in tmls's format | read and **written** |
| `sketchpad` | the sketchpad hub's address, first line | read |
| `prompts` | saved messages for the Send form, one per line; `#` starts a comment (default: `What's the progress?`) | read |

`pins` is the only file v0 writes. Nothing else in that directory is created or changed.

Also read, never written: `~/.cache/tmls/events.jsonl` on each host (written by `tmls hooks install`; without it the host's header says `· poll`). Environment: `TMLS_RS_POLL_S` (listing period in seconds, default 5), `TMLS_RS_NO_CONTROLMASTER=1` (no shared ssh connection; the keepalive stays). ssh sockets: `$XDG_RUNTIME_DIR/tmls-rs/cm-*`, else `~/.cache/tmls-rs/cm-*`.

## Keys

The keyboard focus is on the terminal of the front tab, on the session list, or in an open
form or menu. At start it is on the list. Opening or switching a tab moves it to the terminal.

The full table is in the app (`?` in the list, or the `?` button) and on the command line:

```text
Everywhere
  Ctrl+Q              quit; the sessions keep running
  Ctrl+= / Ctrl+−     font size
  Alt+Shift+→ / ←     next / previous tab (Ctrl+Alt+→ / ← too)
  Alt+Shift+↑         the keys to the session list
  Alt+Shift+N         next that waits, else unread
  Alt+Shift+A         send a message to this tab
  Alt+Shift+W         close the tab (session lives)
  Alt+Shift+F         images this session made
  Alt+Shift+X         close the file panel

List
  j / k  ↑ / ↓        move
  Enter               attach
  Esc                 keys back to the terminal
  p                   pin or unpin (★)
  n                   next that waits, else unread
  c                   new session on this host
  m                   the session's menu
  a                   alerts
  ?                   this help
  y                   approve a permission prompt
  s                   send or queue a message
  click               attach; on + a new session
  right-click         the session's menu
  middle-click        pin or unpin

Terminal
  Ctrl+Shift+C        copy the selection
  Ctrl+Shift+V        paste
  Ctrl+C              copies when text is selected
  drag                select (Shift+drag in tmux)
  Ctrl+click          open a link or a file
  Shift+wheel         scroll here, not in tmux
  Enter               reattach a tab that ended

Panel
  j / k               move in the image list
  Enter               open the picture
  Backspace / Esc     back
  drag the divider    the panel's width

Forms
  Tab / Shift+Tab     next / previous field
  Enter               the focused button
  Esc                 cancel
  ↑ / ↓               choose; a saved message
  n                   No, in an approval
  Ctrl+Shift+V        paste into the field
```

**Approvals.** On a session that waits for a permission (`?`), `y` in the list, `y` in the
list of alerts, or Menu → Approve… opens a form with the dialog as the session shows it. Enter
answers Yes (sends `1`), `n` answers No (sends Esc), Esc cancels and sends nothing. The dialog
is read again just before the key is sent: if it changed, nothing is sent and the status line
says the session isn't asking that any more. A form that arrives late is dropped if you typed
into the session meanwhile.

**Messages.** `s` on a Claude session in the list, Alt+Shift+A in its tab, or Menu → Send
message… opens a form with your saved prompts (`~/.config/tmls/prompts`). A message to a session
that is running or waiting is queued (`N✉` on the row) and sent one per turn end; the queue is
kept in memory and lost at quit. If sending fails the message stays queued.

**Unread.** A session with output you have not looked at has `•` after its name. Alt+Shift+N
(and `n` in the list) visits the sessions that wait; when none waits it visits the unread ones.

Keys typed while the focus is on the list, in a form or in a menu reach no session.


## New session, rename, kill

**New session.** `c` in the list, the `+` on a host's header, or the `New` button. The form
has Host, Folder, Name and Start. Start lists `shell`, `claude`, then your presets; the first
preset is the default, else `claude`. The name follows the folder (the git repository's name
when the folder is in one) until you edit it. A name tmux cannot use (empty, or with `:`, `.`
or `#`) blocks Create and the form says why. The session is made at the size of the terminal
view and its tab opens.

**Rename.** Menu → Rename. The field holds the old name, selected. If the session has a tab,
the tab is closed and opened again under the new name.

**Kill.** Menu → Kill…. A confirm names the session and what runs in it. The focus starts on
Cancel, so Enter kills nothing. To kill: Tab, then Enter, or click `Kill`. The session's tab
is closed.

**Open in new terminal.** Menu → Open in new terminal. Runs `$TERMINAL -e <attach command>`;
without `$TERMINAL`, kitty if it is installed, else `x-terminal-emulator`.

**Copy attach command.** Menu → Copy attach command, or the `Copy` button.

After a rename or a kill started from the list (`m` with the keys in the list), the keys
stay in the list: the tab that is reopened or comes to the front does not take them. Esc
gives them to the terminal. Started while the terminal had the keys (a right-click on a
row), they go to the tab in front as before.

The status line says what each of these did, or why it failed, until the next key. Each
command has 10 s to finish.

## Alerts

When a session's mark changes to `?` (waiting), `◆` (done while you were not looking) or `✕`
(error), the app notes it. The bell at the foot of the list, right of `?`, shows how many are
new: `!` with none, `!3` with three. It is `!` and not tmls's 🔔 because the terminal's font
need not have that glyph.

`a` in the list or a click on the bell opens the list of alerts: newest first, the last 50,
each with its age, mark, session, host and what happened. Opening it sets the count back to
zero. Enter or a click goes to the session (opens its tab or brings it to the front) and
closes the list. If that session was renamed or has ended since, or its host is offline now,
the status line says so.

For `?` and `◆` the app also runs `notify-send -- "tmls · HOST" "◆ NAME · done"` (for `?`:
what the session waits for, else `waiting`). Without `notify-send` on this machine there is
no notification and no error.

What does not alert:

- anything at the first listing after the start, and a session that is new in the list (as
  in tmls: unknown before is not news);
- the session of the tab in front while this window has the keyboard focus ("silence
  focused"). When the window is not focused, that session does alert, also when it finishes.

A host that does not answer for a while loses no alert: when it is back, every session whose
mark changed meanwhile alerts once, as the list then shows it.

Switches, read once at start from `~/.config/tmls/notifications.json` (the file tmls's alert
panel saves): `"desktop": false` turns the notifications off, `"silence_focused": false`
alerts for the tab you are looking at too. The app never writes that file and has no switches
of its own. tmls's `sound` switch is ignored: the app plays no sound.

Approvals (tmls's Yes / No on a Claude permission prompt) are done from the list or the alerts
with `y`; see "Keys". Nothing else in the alerts sends a key to a session.

## Claude outside tmux

This machine's Claude Code sessions that run in no tmux pane (started straight in a terminal
window) are listed under a rule `── not in tmux ──`, right below this machine's tmux sessions,
with their status mark and what they wait for. The section is there only while there is such a
session. They cannot be attached, pinned, renamed or killed from here and have no menu; Enter
or a click asks kitty to bring their window to the front (kitty needs `allow_remote_control`
and `listen_on` — the same as tmls). In another terminal the status line says the window was
not found. Other hosts' sessions outside tmux are not shown, as in tmls. They raise no alert.

## Mouse

| Action | Does |
| --- | --- |
| click a session in the list | attach it |
| click a session under `── not in tmux ──` | bring its kitty window to the front (it cannot be attached) |
| middle-click a session | pin or unpin it |
| click a tab | bring it to the front |
| click the `×` of a tab | close the tab (the session lives on) |
| click the red `✕` at the foot of the list | quit (sessions keep running) |
| drag the line right of the list | make the list wider or narrower (20 columns to half the window; not kept across a restart) |
| click `Sketch` at the foot of the list | open the sketchpad hub in the browser; the button is there only when `~/.config/tmls/sketchpad` has the hub's address |
| wheel over the list | scroll the list |
| right-click a session | its menu (Rename, Kill…, Open in new terminal, Copy attach command) |
| the `?` button | the keys and mouse actions |
| right-click a session that asks | its menu starts with Approve… |
| right-click a Claude session | its menu ends with Send message… |
| `+` on a host's header | new-session form for that host |
| `New` button | new-session form for the host of the session under the cursor |
| `Open` button | attach the session under the cursor |
| `Copy` button | copy the `tmux attach` command of that session |
| click in a form | focus a field, choose a Start, press a button; a click on Host shows the next host |
| click beside an open menu | close it |
| the bell (`!`, or `!3` with three new alerts) | the list of alerts |
| click an alert | go to its session; a click beside the list closes it |

### Mouse in the terminal

Every copy and paste says in the status line what it did (`copied N chars`, `nothing selected`, `pasted N chars`, `clipboard empty`, or why it failed).

A program in the session can ask for the mouse; tmux does with `set -g mouse on`. What the
mouse does over the terminal depends on that:

| Action | tmux has `mouse on` | no program asked for the mouse |
| --- | --- | --- |
| wheel | goes to tmux: it scrolls its own history (copy mode) | scrolls this window's scrollback |
| click, drag, right-click, middle-click | go to tmux: select a pane, drag a border, tmux's own selection and menu | a left drag selects text here and copies it on release |
| **Ctrl+drag** or **Shift+drag** | selects text here and copies it on release | the same |
| **Ctrl+click** on a link | opens it | the same |
| Shift+wheel | scrolls this window's scrollback | the same |

With Ctrl or Shift held, the left button (click and drag) is not sent to the program: it
selects or opens a link here. That is about the left button only: Ctrl+wheel, and pointer
motion with no button held, are still reported to a program that asked for them. Copied text
has no trailing spaces at the ends of its lines. A selection stays highlighted until the text under it changes, a
click, or Ctrl+C (which copies it again and ends it). On a tab whose session ended the
mouse always selects here.

### Links

Ctrl+click on an `http://` or `https://` URL opens it with `xdg-open`. The URL is taken as it
is shown: `localhost` stays `localhost`, also for a session on another host. Text marked as
a link by the program (OSC 8) opens its target, if that is http or https. Nothing else is
opened (no `file:`, no paths). A URL that wraps onto the next row is cut at the edge. The
status line says what was opened, or why not.

Not built yet:

- the `⋯` at the end of a session row that tmls has (use right-click or `m`);
- window and pane actions of tmls's menu (new window, split, kill pane);
- file paths and `file:line` as links, and tmls's quick select (Alt+Shift+S);
- right-click paste in the terminal;
- in the alerts: sound, and switches in the app.

## Smoke test

A headless end-to-end run, in a container. It needs Docker and nothing else.

```
tooling/smoke/run.sh
```

It builds the image `tmls-rs-smoke` and the release binary, then runs `tooling/smoke/e2e.sh`
in the container: a headless sway, software Vulkan, its own tmux server and a scratch `HOME`.
Your tmux sessions and your `~/.config/tmls` are never seen. Keys are sent with `wtype`,
clicks with `tooling/smoke/vptr.py`, pictures are taken with `grim`.

It prints one `PASS` or `FAIL` line per check and exits 1 if any failed. Screenshots, the
app's stderr and the latency log go to `/data/scratch/tmls-rs-v0/e2e/`.

No real notification is sent: the run puts a stand-in `notify-send` first on `PATH` (in
`bin/` of the output directory) that writes its arguments to `notify.log` there.

- `SMOKE_OUT=DIR` — another output directory.
- `SMOKE_NO_BUILD=1` — use the binary as it is.
- `SMOKE_TIMEOUT=SECONDS` — limit for the whole run (default 480).

The unit and PTY tests need no window: `tooling/docker/build.sh test -p desktop`.
