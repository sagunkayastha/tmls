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

## Not in v0

`ag`, `ct`, approvals, Ask / queue, prompts, the file viewer, image paste, quick select, the
splits menu, kitty (non-tmux) sessions, web, Android, Windows.

Separate tasks of the plan, which may land after this page was written: mouse reporting to
the program in tmux, clickable links, alerts. As of this page none of them is wired: see
"not built yet" below.

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

Hosts are reached with your own ssh setup (keys, `~/.ssh/config`). Listing never asks for a
password: a host that needs one shows as offline. Attaching runs ssh inside the tab, so a
prompt from ssh shows there.

## Config files

All in `~/.config/tmls/`, the same files `tmls` uses. Every one is optional.

| File | Use | Access |
| --- | --- | --- |
| `hosts` | ssh hosts to list, one per line; `#` starts a comment | read |
| `name` | what this machine is called in the list (default: its hostname) | read |
| `host-colors.json` | accent colour per host | read |
| `agent-presets.json` | agent commands for a new session: the form's Start choices after `shell` and `claude` | read |
| `pins` | pinned sessions, in tmls's format | read and **written** |

`pins` is the only file v0 writes. Nothing else in that directory is created or changed.

## Keys

The keyboard focus is on the terminal of the front tab, on the session list, or in an open
form or menu. At start it is on the list. Opening or switching a tab moves it to the terminal.

Everywhere:

| Key | Does |
| --- | --- |
| Ctrl+Q | quit (sessions keep running) |
| Ctrl+= or Ctrl++ | larger font |
| Ctrl+- | smaller font |
| Alt+Shift+Right / Left | next / previous tab |
| Alt+Shift+Up | focus to the session list |
| Alt+Shift+N | open the next session that waits for you |
| Alt+Shift+W | close the front tab (the session lives on) |

In the session list:

| Key | Does |
| --- | --- |
| j / Down, k / Up | move the cursor |
| Enter | attach the session under the cursor |
| p | pin or unpin it |
| n | open the next session that waits for you |
| c | new-session form, for the host of the session under the cursor |
| m | menu of the session under the cursor: Rename, Kill…, Open in new terminal, Copy attach command |
| Esc | focus back to the terminal |

In a form (new session, rename, kill confirm):

| Key | Does |
| --- | --- |
| Tab / Shift+Tab | next / previous field or button |
| Left / Right / Up / Down | on Host or Start: choose; on a button: the other button; in a field: move the caret |
| Enter | Create / Rename, from any field; on Cancel: cancel |
| Esc | cancel |
| Ctrl+A | select the field's text |
| Ctrl+Shift+V | paste into the field |

In the menu: Up / Down or k / j move, Enter picks, any other key closes it.

The Alt+Shift keys do nothing while a form or menu is open. Ctrl+Q and zoom still work.

In the terminal:

| Key | Does |
| --- | --- |
| Enter on an ended tab | reattach |
| Ctrl+Shift+C | copy the selection |
| Ctrl+Shift+V | paste |
| every other key | goes to the session unchanged (Tab, Esc, Ctrl+C, Alt+letters included) |

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

The status line says what each of these did, or why it failed, until the next key. Each
command has 10 s to finish.

## Mouse

| Action | Does |
| --- | --- |
| click a session in the list | attach it |
| middle-click a session | pin or unpin it |
| click a tab | bring it to the front |
| click the `×` of a tab | close the tab (the session lives on) |
| drag in the terminal | select text |
| wheel over the terminal | scroll back |
| wheel over the list | scroll the list |
| right-click a session | its menu (Rename, Kill…, Open in new terminal, Copy attach command) |
| `+` on a host's header | new-session form for that host |
| `New` button | new-session form for the host of the session under the cursor |
| `Open` button | attach the session under the cursor |
| `Copy` button | copy the `tmux attach` command of that session |
| click in a form | focus a field, choose a Start, press a button; a click on Host shows the next host |
| click beside an open menu | close it |

Not built yet:

- the `⋯` at the end of a session row that tmls has (use right-click or `m`);
- window and pane actions of tmls's menu (new window, split, kill pane);
- a renamed session keeps its pin only if you pin it again;
- the `?` button (key help);
- mouse clicks and wheel passed to the program inside tmux;
- clickable links;
- alerts.

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

- `SMOKE_OUT=DIR` — another output directory.
- `SMOKE_NO_BUILD=1` — use the binary as it is.
- `SMOKE_TIMEOUT=SECONDS` — limit for the whole run (default 480).

The unit and PTY tests need no window: `tooling/docker/build.sh test -p desktop`.
