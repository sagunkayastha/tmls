# web tmls — design

Date: 2026-10-02. Status: approved in conversation, awaiting spec review.

## Goal

A browser version of tmls for working **at the desk on a laptop**, all day. What it adds over the
terminal UI is layout: one big typeable session, the other sessions readable at a glance next to it,
and the sketchpad board one click away. It runs alongside the terminal tmls; it does not replace it.

Success: the user can do a normal day's work in it (type in a session, watch the others, approve
prompts, sketch to a session) from home over the LAN and from outside over their personal tailnet.

## Screen (v1)

```
┌─ tmls ─────────────────────────────────────────────── A- A+ 14px ─ 🔔2 ─┐
│ [Terminal] [Sketch]   archbox · Season-36                     ● working │
├──────────────────────────────────────────────────────┬──────────────────┤
│ $ claude                                              │ archbox          │
│ > fix the parser so it handles \t                     │▌● Season-36      │
│ ● Reading parser.py…                                  │  fix the parser… │
│                                                       │ ? New_Vision     │
│                                                       │  Allow Edit app.py│
│   (xterm.js: the whole tmux session; type here)       │  [Yes] [No]      │
│                                                       │ ○ season-tool    │
│                                                       │  idle 12m        │
└──────────────────────────────────────────────────────┴──────────────────┘
```

- **Main pane**: the selected session at full size (xterm.js). Clicking a row switches it.
- **Rows** (right column, grouped by host): status mark (● working, ? waiting, ◆ done, ✕ failed,
  ○ idle), name, and a second line: the waiting reason for `?`, otherwise the session's last
  non-empty screen line. `?` rows have **Yes / No** buttons.
- **Font zoom**: A− / A+ and Ctrl +/− change the terminal font size only (not the browser zoom),
  saved per device in localStorage.
- **[Sketch] tab**: the sketchpad board replaces the terminal in the main pane, targeted at the
  selected session (sketchpad page in an iframe with `?target=<host>/<name>`).
- **🔔**: alert inbox (`?`, `◆`, `✕` transitions); clicking an alert selects that session.
- **States**: a dropped terminal dims with "reconnecting…"; an offline host greys its rows.

Out of scope for v1 (already in the terminal tmls; add later): create/rename/kill sessions,
window/pane actions, message queue, saved prompts / Ask panel, tile view, phone layout.

## Architecture

```
browser                                  tmls web (archbox, aiohttp, one process)
┌───────────────┐   GET / (login)      ┌──────────────────────────────────────────┐
│ index.html    │◀────────────────────│ static/ (xterm.js from CDN, no build)    │
│ rows, 🔔      │◀── ws /api/events ──│ poll loop, 2 s: hosts.list_host(each)    │
│               │                     │  + hosts.status() → marks, last line     │
│ [Yes]/[No]    │── POST /api/approve▶│ approve.answer(host, name, shown, yes)   │
│ xterm.js      │◀═ ws /api/term ════▶│ pty.fork → hosts.attach_argv(host, name) │
│ [Sketch]      │── iframe ──────────▶ sketchpad page ?target=host/name          │
└───────────────┘                     └──────────────────────────────────────────┘
```

- New command **`tmls web`** (entry point in `pyproject.toml`), code in `src/tmls/web/`:
  `server.py` (aiohttp app, routes, poll loop), `auth.py` (login), `static/` (`index.html`,
  `app.js`, `style.css`). New dependency: `aiohttp`.
- **Reused as-is** from tmls: `hosts.list_host`, `hosts.status`, `hosts.attach_argv`,
  `hosts.read_config` (host list from `~/.config/tmls/hosts`), `approve.current` / `approve.answer`.
  No Textual import in the web server.
- **Poll loop**: one per server, shared by all browser tabs. Every 2 s it lists every host, computes
  marks with `hosts.status`, and for sessions whose mark or activity changed captures the last screen
  line (`tmux capture-pane`). It pushes only changed rows over `/api/events` (JSON list of
  `{host, name, mark, line, waiting}`); a new connection gets the full list first. Alerts are derived
  from mark transitions in the same loop.
- **Terminal**: each `/api/term?host=&name=` WebSocket owns one pty running the same
  `tmux -u attach` command the TUI uses (local on archbox, `ssh -t <host> …` for others).
  Browser → server messages: `{"t":"in","d":"<keys>"}` and `{"t":"size","cols":N,"rows":M}`
  (applied with TIOCSWINSZ + SIGWINCH); server → browser: raw pty output as binary frames.
  Closing the socket hangs up the pty (the tmux session keeps running).
- **Approve**: `POST /api/approve {host, name, shown, yes}` → `approve.answer`, which re-reads the
  prompt and sends nothing if it changed; the reply says which.
- **Sketchpad change** (sketchpad repo, small): read `?target=host/name` on load and preselect it.

## Access and login

Follows the user's existing archbox pattern (`~/stacks/<app>/`):

- **Home**: `tmls web` binds the archbox **LAN address only**, port **8794**
  (`--bind <LAN-IP> --port 8794`). Never `0.0.0.0`: that would also bind `tailscale0`, and the
  archbox host's tailscale is the employer's tailnet.
- **Away**: `~/stacks/tmls/docker-compose.yml` runs a `tmls-tailscale` sidecar (copy of
  `sketchpad-tailscale`) that joins the user's **personal** tailnet as node `tmls`, serves HTTPS with
  a Tailscale cert, and proxies to the host server on the docker bridge (`172.17.0.1:8794`; the
  server binds that address too).
- **Login**: password page on both paths, verified against **sketchpad's credentials file**
  (`~/.config/sketchpad/auth.json`, scrypt) so one password covers both apps. A signed, HttpOnly
  session cookie lasts 30 days. Every route except the login page and its POST requires it,
  including both WebSockets.
- **WebSocket Origin check**: `/api/term` and `/api/events` reject an `Origin` that isn't this
  server's own host, so another website open in the browser can't open a terminal.
- **Sketch iframe**: sketchpad is on a different host name. If its login cookie isn't sent inside the
  iframe, the [Sketch] tab opens sketchpad in a new browser tab instead (decided by an early check).
- **Deploy**: `tmls-web.service` (systemd user unit on archbox) + the `~/stacks/tmls/` sidecar,
  modelled on sketchpad's `deploy/` files.

## Errors

- **Terminal drop** (ssh dies, session killed): pane dims "reconnecting…", retries every 2 s up to
  5 times, then "connection lost" with a [Reattach] button. A session that really ended says
  "session ended".
- **Host offline**: its rows grey out with "offline"; attaching shows the ssh error in the pane.
- **Approve refused** (prompt changed): toast "prompt changed, nothing sent"; the row refreshes.
- **Browser closed / tab closed**: the pty and its tmux client are hung up; sessions keep running.
- **Not logged in / cookie expired**: HTTP routes redirect to the login page; WebSockets close with
  code 4401 and the page reloads to the login page.

## Testing (test-first)

- **Unit**: cookie sign/verify and expiry, password check against a temp `auth.json`, Origin check,
  row diffing, terminal message parsing.
- **Server** (aiohttp test client): fake `hosts.list_host` (like `fake_hosts` in `tests/test_app.py`);
  login redirect and 4401; `/api/events` full list then diffs; `/api/approve` happy path and "prompt
  changed"; `/api/term` with a real pty running `sh -c 'echo attached; cat'` (echo round trip,
  resize).
- **Browser end-to-end** (Playwright, as sketchpad's `tests/e2e_ui.py`): log in, rows appear,
  clicking switches the pane, typing reaches the pty, A+ grows the font and survives reload, Yes
  calls approve, [Sketch] loads the iframe.
- **Real check on archbox**: a throwaway tmux session opened from the laptop over the LAN and over
  the tailnet, then deleted.
