# tmls

tmux sessions from every machine in one terminal UI. Pick a session on the left and it
attaches in a tab on the right, or opens in a new terminal window, or you copy the attach
command.

![tmls with three hosts and two attached tabs](docs/screenshots/tmls.png)

## What it does

- **Lists sessions per host**: this machine (if it has tmux) plus any ssh hosts you name.
  The list refreshes every 5 seconds. A host that doesn't answer shows as *offline*.
  A `●` marks a session that is attached somewhere else too.
- **Attach in a tab**: click a session and it runs `tmux attach` (over `ssh -t` for
  remote hosts) in an embedded terminal. Every key goes to the session, including
  Ctrl+C and Tab. Open several and switch with the tab bar. `×` on a tab detaches it;
  the session keeps running.
- **Open** puts the same attach command in a new terminal window (`$TERMINAL`, else
  kitty, else `x-terminal-emulator`).
- **Copy** puts the attach command on the clipboard, for example
  `ssh -t nas "tmux attach -t work"`, so you can paste it anywhere.
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

## How it works

`hosts.py` runs `tmux ls -F ...` locally or over ssh and parses the result. `term.py` is
the embedded terminal: a pty (`pty.fork`, so signals and resizes work) rendered through
[pyte](https://github.com/selectel/pyte) into a [Textual](https://textual.textualize.io/)
widget. `app.py` is the layout and the session/tab bookkeeping.

## Limitations

- No mouse or scrollback inside the embedded terminal; use tmux's own scroll mode.
- Keyboard focus stays in the terminal once a tab is open; use the mouse for the list.
- Attaching resizes the tmux window to tmls's pane, which other attached clients see
  (that's tmux's `window-size` behaviour).
- Tested on Linux only.

## Development

```sh
uv run pytest        # 27 tests: parsing, key mapping, a real pty, and Textual pilot tests with fake hosts
```

## License

MIT, see [LICENSE](LICENSE).
