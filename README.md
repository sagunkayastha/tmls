# tmls

tmux sessions from every machine in one terminal UI. Pick a session on the left and it
attaches in a tab on the right, or opens in a new terminal window, or you copy the attach
command.

![tmls with three hosts and two attached tabs](docs/screenshots/tmls.png)

## What it does

- **Lists sessions per host**: this machine (if it has tmux) plus any ssh hosts you name.
  The list refreshes every 5 seconds. A host that doesn't answer shows as *offline*.
- **Also lists Claude Code sessions on this machine that aren't in tmux**, under
  `<hostname> · kitty`, with the same status marks. There's no tmux to attach to, so a
  click focuses the [kitty](https://sw.kovidgoyal.net/kitty/) window they run in. This
  needs kitty remote control in `kitty.conf`:

  ```
  allow_remote_control socket-only
  listen_on unix:${XDG_RUNTIME_DIR}/kitty-{kitty_pid}
  ```
- **Shows what each session is doing**, with one mark after its name:

  | Mark | Meaning |
  |------|---------|
  | green `●` | running |
  | yellow `?` | Claude is waiting on you (a permission prompt or a question). Hover for which. It stays until you answer. |
  | orange `◆` | finished since you last looked: it needs you. Opening its tab clears it. |
  | red `✕` | Claude's last reply was an API error. Clears when the next turn starts. |
  | dim `○` | idle, and you've seen it |

  For a session running [Claude Code](https://docs.anthropic.com/en/docs/claude-code),
  tmls reads Claude's own status file (`~/.claude/sessions/<pid>.json`): busy, or a
  monitor or background shell still going, is running. Any other session counts as
  running while it has printed something in the last 30 seconds. Open tabs show the
  same mark in front of the name. A Claude session whose conversation has its own name
  (`/rename`, or one Claude made up) shows it on a dim second line, ending in how full its
  context is (`58%`; orange from 70%, red from 90%). Claude Code doesn't save its context
  limit, so tmls assumes 1M for Claude 5 models and 200k for older ones.
- **Attach in a tab**: click a session and it runs `tmux attach` (over `ssh -t` for
  remote hosts) in an embedded terminal. Every key goes to the session, including
  Ctrl+C and Tab. Open several and switch with the tab bar or **Alt+Shift+Left/Right**.
  **Alt+Shift+Up** moves to the session list: j/k or arrows move, Enter attaches, Esc goes
  back to the terminal.
  `×` on a tab detaches it; the session keeps running.
- **Mouse in the terminal**: the wheel scrolls back through tmux's history, and a drag
  selects and copies to your clipboard (needs `set -g mouse on` in tmux). Shift+drag
  still gives your terminal's own selection. Ctrl+drag selects and copies text on
  release; Ctrl+C copies the selection again, or interrupts when nothing is selected.
  Copied text loses trailing spaces on each line. Pasting
  (kitty's Ctrl+Shift+V, or right-click for the system clipboard) goes to the session, as a
  bracketed paste when it asks for one.
- **`ag`** (for agents): `ag ls` lists sessions on every host, `ag read NAME` prints the
  end of a session's screen, and `ag send NAME "text" [--from ME --mode MODE]` messages a
  Claude session by name (its own inbox socket, over ssh for remote hosts), a Codex thread by
  its `/rename` name (`codex queue`), or any other tmux session (paste + Enter).
  `HOST:NAME` picks the host. `--mode` states the sender's own permission mode; a Claude
  that skips permission prompts holds messages from senders that don't say they do too.
- **Links in the terminal**: Ctrl+click a URL to open it in your normal browser, or a
  `file:line` path to read that file beside the terminal. A URL printed by a remote host
  with `localhost` still opens on this laptop's `localhost`.
- **Quick select**: Alt+Shift+S labels URLs, file paths and commit hashes visible in the
  terminal. Press a label letter to copy it, Shift+letter to open a URL or file, or Esc
  to cancel. The labels go away after the choice.
- **`ct`**: run instead of `claude` to start Claude inside a tmux session named after the
  folder (or rejoin it), so tmls can open it in a tab. Arguments go to `claude`; inside tmux
  it's plain `claude`.
- **New session**: `+` on a host line opens a small form (host, folder, name, start
  a shell, `claude`, or a named agent command). The session is created with `tmux new-session` in that folder and
  opens in a tab. Name follows the git repository name when Folder is inside one (or
  the folder name otherwise); editing Name yourself keeps your choice. Optional named
  agent commands appear below `claude` in Start. Put them
  in `~/.config/tmls/agent-presets.json` as argv lists, for example
  `{"Opus plan": ["claude", "--model", "opus", "--permission-mode", "plan"]}`.
  The first of them is the default Start; with none, `claude` is.
  Also in the browser: `+` on a host header.
- **Rename or kill**: click `⋯` at the end of a tmux session row. Rename changes the
  session name and reopens its tab under the new name. Kill always asks you to confirm,
  shows running commands in the warning, and closes the session's tab. The same menu can
  create or rename the active window, split its pane side by side or top/bottom, and kill
  the active pane with confirmation. New windows and panes start in that pane's folder;
  the open tab follows tmux without reconnecting.
- **Host colors**: a thin colored bar on each host header and its tabs helps distinguish
  hosts. Colors are stable across restarts. To override one, put a named color in
  `~/.config/tmls/host-colors.json`, for example `{"archbox": "green"}`. Available names:
  `blue`, `teal`, `violet`, `green`, `amber`, `rose`.
- **Ask**: sends a message to the shown session: type one, pick a saved prompt (one per
  line in `~/.config/tmls/prompts`; default "What's the progress?"), or resend one of the
  last ten you typed to its Claude. While the session is working (●) or waiting on
  you (?), Ask queues the message instead. The row and Ask title show the count;
  **Clear queue** removes messages still waiting. One message goes through tmux's paste
  buffer after each finished turn (◆ or ○). A permission prompt (?) or API error (✕)
  keeps the queue. Queues last only until tmls exits.
- **Image paste**: right-click an image clipboard in a session to save it under
  `~/.cache/tmls/images/` on that session's host and paste its path without Enter.
  Dropping one local `.png`, `.jpg`, `.jpeg`, `.gif`, or `.webp` file into a remote tab
  copies it there and pastes the remote path. Local dropped paths stay local. Text
  paste still works as before. This uses `wl-paste` on Wayland or `xclip` on X11.
- **🔔 Alerts**: every time a session turns `?`, `◆` or `✕`, tmls notes it. The bell
  shows how many are new; click it for the list (newest first) and click a line to jump to
  that session. Claude sessions sitting at a permission prompt are listed on top with the
  request and **Yes** / **No**, which send `1` or Esc. tmls checks that the same request is
  still on screen first. The panel has separate **Desktop**, **Sound**, and **Silence focused**
  switches, saved in `~/.config/tmls/notifications.json`. Desktop alerts use `notify-send`
  for `?` and `◆`; Sound uses `canberra-gtk-play` (off by default). Silence focused is on
  by default and skips alerts for the tab you are viewing.
- **Open** puts the same attach command in a new terminal window (`$TERMINAL`, else
  kitty, else `x-terminal-emulator`).
- **Copy** puts the attach command on the clipboard, for example
  `ssh -t nas 'tmux -u attach -t =work'`, so you can paste it anywhere.
- **Sketch** (optional) opens your [sketchpad](https://github.com/sagunkayastha/sketchpad)
  in the browser. See [Sketchpad button](#sketchpad-button).
- **Quit** (or Ctrl+Q) closes tmls and hangs up its tmux clients. Sessions keep running.

## Install

Needs Python 3.12+ and [uv](https://docs.astral.sh/uv/). tmux is needed on the machines
whose sessions you want to see, not necessarily on the one running tmls.

```sh
git clone https://github.com/sagunkayastha/tmls
cd tmls
uv tool install .
```

## Use

```sh
tmls                 # this machine plus the hosts in ~/.config/tmls/hosts
tmls nas laptop      # this machine plus these ssh hosts
```

The config file has one ssh host per line; blank lines and `#` comments are ignored:

```
# ~/.config/tmls/hosts
nas
laptop   # off most of the time
```

Hosts are whatever `ssh <host>` accepts, so put aliases and keys in `~/.ssh/config`.
Listing uses `ssh -o BatchMode=yes`, so a host that needs a password shows as offline.

`tmls-web --bind ADDR [--port 8794] [--trust-proxy ADDR]` serves the browser version;
`--trust-proxy` names a reverse proxy whose `X-Forwarded-For` is believed for the login lockout.
`--bind` and `--trust-proxy` can be repeated, and `--trust-proxy` also takes a network
(`172.31.77.0/24`). On a phone the session list slides in from `☰`, the terminal takes the
whole width, and a bar under it supplies Esc, Tab, Ctrl (arms the next key) and the arrows.

## Docker

`tmls-web` can run as a container next to your other stacks. There is no tmux inside it: every
host, including the machine running the container, is reached over ssh. `deploy/` has a
`docker-compose.yml` (tmls-web plus a Tailscale sidecar for use away from home),
`tailscale-serve.json` and `ssh_config.example`.

1. On this machine, set the sketchpad password (`~/.config/sketchpad/auth.json`) and write
   `~/.config/tmls/hosts` first, naming **this machine by its hostname** too: from inside the
   container it is no longer "local". A missing file would come up as a root-owned folder.
2. Copy the three files to a folder of their own, say `~/stacks/tmls`, and in it run
   `mkdir -p ssh state && touch state/known_hosts && chmod 700 ssh`, and put the **absolute**
   path of this checkout in `.env` as `TMLS_SRC=`.
3. `ssh/`: a key every listed host accepts (`cp ~/.ssh/id_ed25519{,.pub} ssh/`, and add the `.pub`
   to this machine's own `~/.ssh/authorized_keys`, ideally with `from="172.31.77.0/24"` in front
   so it only works from the container), plus `ssh/config` made from `ssh_config.example`: one
   `Host` block per line of `~/.config/tmls/hosts`, your login as `User`, and this machine as
   `host.docker.internal`. The container runs as uid 1000; if your uid differs, put `UID=` in `.env`.
   This machine's sshd must accept connections from Docker's bridge addresses.
4. Put the LAN address under `ports:`, stop anything else on that port (an old
   `tmls-web.service`), and run `docker compose up -d --build` as yourself, not with sudo. Log in
   on port 8794 as before; `docker exec tmls-web ssh -v <host> true` shows why a host is offline.
5. Away from home: `docker compose logs tmls-tailscale` prints a login URL once; open it while
   signed in to your tailnet, then use `https://tmls.<tailnet>.ts.net`.

After a `git pull`, `docker compose up -d --build` again. `--trust-proxy` in the compose file names
the sidecar's fixed address, so the login lockout counts per real client.

Already have a reverse proxy on your tailnet (say Caddy in another stack, with your own domain)?
Drop the sidecar, attach `tmls-web` to that stack's network (`networks: {proxy: {name: <stack>_default,
external: true}}`) and point the proxy at `tmls-web:8794`. If that proxy can't see the real client
address, leave `--trust-proxy` out.

## Android app

`android/` is a small app around tmls-web: the same page, full screen, with its own login that
lasts, and an in-app update button. It feels like a native app, not a page in a browser:
dark from the first frame, the key bar rides on the keyboard as it slides, Back closes the drawer
or goes from a session to the list, and the terminal draws with WebGL.

- **Build** (Docker only, nothing installed on the host): `android/make.sh` builds, tests and lints a
  release APK signed with a key it creates once in `~/.config/tmls-android/` (**back that folder
  up**: a new key means uninstalling the app to update it), then publishes `tmls.apk` and
  `latest.json` to `$TMLS_APK_DIR` (default `~/stacks/tmls/apk`). `android/make.sh debug` builds a
  debuggable `dev.sagun.tmls.debug`; `android/make.sh profile` the same package built like a release,
  for measuring smoothness. `android/test.sh` runs the unit tests and lint.
- **Serve updates:** run tmls-web with `--apk-dir DIR` (in the compose file: mount `./apk` read-only
  and pass `--apk-dir /home/tmls/apk`). It serves `/app/latest.json` and `/app/tmls.apk`, behind the
  login.
- **Install:** `android/install.sh` (adb over USB; `DEVICE=` picks the phone, `APK=` the file), or
  tap **Get the Android app** at the bottom of the session list in an Android browser (logged in).
  To skip the address screen, put your addresses in `~/.config/tmls-android/servers` before
  building (home on the first line, e.g. `http://192.168.1.10:8794`, away on the second, e.g.
  `https://tmls.example.com`): `make.sh` builds them in. Without it, first run asks for them. The
  app uses whichever answers, then shows the site's login once.
- **Update:** each new `make.sh` publishes a higher version. The app checks on start and every 6
  hours, and ⟳ in the header checks now; a banner offers it, the download is checked against
  `latest.json`'s SHA-256, and Android asks you to confirm.

## Sketchpad button

[sketchpad](https://github.com/sagunkayastha/sketchpad) is a drawing board that sends
sketches straight into a running Claude Code session. tmls can show a **Sketch** button
that opens it in your browser. The button only appears once you tell tmls where your
sketchpad hub is:

1. Set up the hub by following sketchpad's
   [quick start](https://github.com/sagunkayastha/sketchpad#quick-start-one-machine).
   It serves the board on port 8790.
2. Put the hub's URL on one line in `~/.config/tmls/sketchpad`:

   ```sh
   echo "http://192.168.1.10:8790" > ~/.config/tmls/sketchpad   # your hub's address
   ```

3. Restart tmls. The button opens that URL with `xdg-open`.

## How it works

`hosts.py` runs one shell script per host, locally or over ssh: the host's clock,
`tmux list-windows -a -F ...`, and the Claude Code status files of live processes. Times
are compared on each host's own clock. `term.py` is
the embedded terminal: a pty (`pty.fork`, so signals and resizes work) rendered through
[pyte](https://github.com/selectel/pyte) into a [Textual](https://textual.textualize.io/)
widget. `app.py` is the layout and the session/tab bookkeeping.

## Limitations

- Keyboard focus stays in the terminal once a tab is open; use the mouse for the list.
- Attaching resizes the tmux window to tmls's pane, which other attached clients see
  (that's tmux's `window-size` behaviour).
- Tested on Linux only.
- In the browser, the mouse wheel only scrolls tmux history when the session has `set -g mouse on`;
  otherwise it sends arrow keys like a plain terminal.
- In the browser, a host with no sessions has no `+` yet; create its first session from the terminal UI
  or `tmux new-session`.

## Development

```sh
uv run pytest        # 247 tests: parsing, status marks, key and mouse mapping, a real pty, and Textual pilot tests with fake hosts
```

## License

MIT, see [LICENSE](LICENSE).
