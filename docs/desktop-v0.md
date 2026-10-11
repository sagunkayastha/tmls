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

## Not in v0

`ag`, `ct`, approvals, Ask / queue, prompts, the file viewer, image paste, quick select, the
splits menu, kitty (non-tmux) sessions, web, Android, Windows.

Separate tasks of the plan, which may land after this page was written: new / rename / kill
of a session, mouse reporting to the program in tmux, clickable links, alerts. As of this
page none of them is wired: see "not built yet" below.

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
| `agent-presets.json` | agent commands for a new session | read; not used yet (the new-session form is not built yet) |
| `pins` | pinned sessions, in tmls's format | read and **written** |

`pins` is the only file v0 writes. Nothing else in that directory is created or changed.

## Keys

The keyboard focus is either on the terminal of the front tab or on the session list. At
start it is on the list. Opening or switching a tab moves it to the terminal.

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
| Esc | focus back to the terminal |

In the terminal:

| Key | Does |
| --- | --- |
| Enter on an ended tab | reattach |
| Ctrl+Shift+C | copy the selection |
| Ctrl+Shift+V | paste |
| every other key | goes to the session unchanged (Tab, Esc, Ctrl+C, Alt+letters included) |

Keys typed while the focus is on the list reach no session.

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
| `Open` button | attach the session under the cursor |
| `Copy` button | copy the `tmux attach` command of that session |

Not built yet:

- the `+` on a host's header and the `New` button (new session);
- right-click on a session (its menu: rename, kill); it only moves the cursor;
- the `?` button (key help);
- rename and kill of a session;
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
- `SMOKE_TIMEOUT=SECONDS` — limit for the whole run (default 300).

The unit and PTY tests need no window: `tooling/docker/build.sh test -p desktop`.
