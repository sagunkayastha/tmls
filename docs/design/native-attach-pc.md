# tmls native-attach — PC edition (plan)

Status: **draft for approval**, 2026-10-09. The laptop edition (tmux pane host) comes after the PC edition
is the daily driver. tmls-rs (Rust rewrite) is paused; see "Why not tmls-rs" at the end.

## Goal

tmls becomes the **sidebar and brain**. **kitty draws the session**, natively on the GPU. Priorities in order:

1. Performance.
2. Visual quality.
3. Native feel.

It must be close to free while gaming.

PC facts:
- laanta-chy: CachyOS, KDE Wayland, RTX 5070 Ti, 16 cores, 30 GB RAM.
- kitty 0.49.2, with `allow_remote_control socket-only`, `listen_on unix:${XDG_RUNTIME_DIR}/kitty-{kitty_pid}`, and the `splits` layout enabled.
- tmux 3.7 on the PC and on archbox.
- tmls already runs inside kitty (`KITTY_LISTEN_ON` is set).

## Where we start (measured 2026-10-08)

The lag comes from the embedded Python terminal (pyte + Textual), not from the sidebar or any one feature. It costs the same in the first commit, and it grows with every open tab, because hidden tabs still parse their output.

After today's fixes (`3dd21c3` in-place rows, `1294a7d` dirty-line redraw), a busy Claude tab costs about 5.5% of a core, 14 kB/s of screen output, and p99 stalls around 6 ms. Very heavy output still costs about 48% of a core.

Benchmark harness: `/data/scratch/tmls-bisect/` (`bench_app.py`, `run_one.py`, `flood.py`, `flood_claude.py`).

## Design

```
┌─ kitty OS window (splits layout) ──────────────────────────────────────────┐
│ kitty window 1: tmls sidebar  │ kitty window 2: tmls-view                  │
│ (Textual, no terminal inside) │   exec-style child: ssh -t HOST tmux attach│
│ ─ archbox ─                   │   ← kitty draws this directly (GPU)        │
│ ● tmls          ◀── click ────┼── same host: tmux switch-client (instant)  │
│ ? pace     58%                │── other host: tell tmls-view → new ssh     │
│ ─ laanta-chy ─                │   over a warm ControlMaster connection     │
│ ◆ fan-tune                    │                                            │
│ [Ask] [Image] [Hints]         │                                            │
└───────────────────────────────┴────────────────────────────────────────────┘
```

- **Launch.** `tmls` checks for `KITTY_LISTEN_ON`.
  - If it's set, it switches the tab to the `splits` layout and opens the view pane:
    `kitten @ launch --location=vsplit --bias 80 --keep-focus --var tmls=view --cwd=current tmls-view`
  - It then sets the sidebar to a fixed width, about 30 columns, with `kitten @ resize-window`.
  - If kitty isn't there, today's tab mode runs unchanged (fallback).
- **tmls-view is never in the data path.**
  - It is a tiny supervisor. It starts `ssh -t host tmux -u attach -t =name` as a child on the **same pty** (inherited stdin/stdout) and waits.
  - Bytes go straight from ssh to kitty, so its cost is about zero.
  - It listens on `$XDG_RUNTIME_DIR/tmls/view.sock` for one message: `attach HOST NAME`.
- **Same-host switch.** `tmux switch-client -c <tty> -t =NAME` over the host's ControlMaster connection. The client is found by its `TMLS_VIEWER` tag, which `hosts.CLIENTS` already reads.
- **Cross-host switch.** tmls sends `attach HOST NAME` to tmls-view. tmls-view ends its ssh child and starts the new one immediately. A ControlMaster connection kept warm per host makes the new ssh start in milliseconds.
- **Focus.**
  - Clicking a row switches the session and moves focus to the view (`kitten @ focus-window --match var:tmls=view`).
  - One key goes back to the sidebar, via a kitty mapping that tmls installs (below).
- **One view, many sessions.** The view is one pane that switches sessions, which replaces tmls tabs. An optional second view (a split, or another kitty tab) can come later if wanted.

### Native replacements for features that lived inside the Python terminal

| Feature today | PC edition |
|---|---|
| Terminal drawing, scrollback, selection, links | kitty, natively. Mouse wheel goes to tmux, as now |
| Quick select (Alt+Shift+S) | **kitty hints kitten** (native) for URLs, paths, line numbers and hashes, with an action handler `tmls open-link` |
| Click a path → image / file / folder (`53ae25f`) | `tmls open-link HOST PATH[:LINE]`. Images are fetched to a cache and shown in a **kitty overlay with `kitten icat`** (native graphics, GPU). Text opens in an overlay running `ssh -t HOST less +LINE` (or `$EDITOR`). Folders and other files open in the system app, as now |
| Image paste (right-click) | A kitty key (and a sidebar button) runs `tmls paste-image`: `wl-paste` → scp → `tmux send-keys` with the path. Same code as `image_paste.py` |
| Tabs with status marks | Sidebar rows carry the marks. The current session is highlighted |
| Ask, alerts, create/rename/kill, host colors, Sketch, viewers line | Unchanged, in the sidebar |

tmls installs these kitty mappings into a separate file, `~/.config/kitty/tmls.conf`, plus one `include` line added to `kitty.conf` (shown to the user before writing; one-command uninstall):
- sidebar focus;
- hints;
- image paste.

### Status push (replaces the 5 s full poll)

From the research and code review (`docs/research/` once added):

- **Claude hooks** (decision 1: merged into `~/.claude/settings.json`; tagged; backup first; `tmls hooks uninstall` removes them). They append one JSON line per event to `~/.cache/tmls/events.jsonl` on each host.

  | Hook | Sets |
  |---|---|
  | `PermissionRequest` | waiting (fires immediately; Notification is about 6 s late) |
  | `Stop` | done, keeping running while `background_tasks` is non-empty; the last reply text is used for the row subtitle |
  | `StopFailure` | failed, with the error type |
  | `UserPromptSubmit` / `PreToolUse` | running |
  | `SessionStart` / `SessionEnd` | add / remove |

- **Status line wrapper** (decision 2). `tmls statusline` reads Claude's status JSON, writes `context_window.used_percentage` and `model.id` to the events file, then runs `cs render` with the same input. The bar looks identical, and context % becomes exact.
- **Per-host agent.** One persistent `ssh HOST python3 -` per host, with the agent sent over stdin, so nothing is installed on the host.
  - It tails `events.jsonl` from an offset.
  - It watches tmux for session changes (hooks or `refresh-client -B` subscriptions).
  - It coalesces events for 50 ms and pushes only changes.
  - Heartbeat, plus a full snapshot every 60 s as a backstop.
  - The old poll stays as the fallback when the agent can't start.
- `~/.claude/sessions/<pid>.json` is still read for the tmux pane mapping (hooks don't carry it).

### Sidebar UX (from cmux, opensessions and agent-deck)

- An unread ring on rows that changed while unseen, and a 0.6 s pulse when something new arrives.
- A spinner while working. The latest Claude message text as the row's second line.
- One key jumps to the newest unread or waiting session.
- A clicked row is highlighted only after the switch is confirmed.
- A host whose ssh needs re-auth shows one "Sign in to HOST" row.
- States from agent-deck:
  - "still running" while background tasks run after Stop;
  - usage-limit and login-expired detection.
- **Game mode.** When `game-mode status` is `on`: no spinner or pulse animation, and the backstop snapshot every 5 min. Push events still update the marks.

## Budgets and gates (PC)

All runs on laanta-chy, with you present for anything with a window. The PC-job rule applies: ask first.

| Gate | Pass |
|---|---|
| Sidebar idle | < 1% of one core (5 min average), < 150 MB RSS |
| Sidebar while the view streams flood | < 1% (the sidebar never sees session output) |
| tmls-view cost | ≈ 0% CPU; not in the data path (checked with `ls -l /proc/<ssh>/fd`: the same pty as kitty) |
| Key echo through the view | p50 within 1 ms, p99 within 3 ms of plain `kitty + ssh + tmux attach` (S0 harness / ydotool K1 method from tmls-rs) |
| Same-host switch | ≤ 150 ms to the new session's first frame |
| Cross-host switch | ≤ 600 ms with a warm ControlMaster |
| Status latency | waiting ≤ 1 s after the permission dialog; done/failed ≤ 1 s; context % equals the status line |
| Host cost of status | < 1% of one core per host at idle |
| Game mode | no animation frames while on; sidebar < 0.5% |
| Daily use | 2 h of real work with no freeze or focus loss; fallback tab mode still works outside kitty |
| Visual | kitty fonts/ligatures in the view; `icat` image overlay; hints labels; no flicker on switch |

## Phases and who does what

**Claude implements the hard parts** (process/pty control, switch paths, push agent): bugs there are subtle
and only show at the gates. **Codex implements well-bounded tasks** from a written brief that Claude writes;
Claude reviews every Codex diff, merges, and runs the gates with you. DeepSeek is not used on tmls.
Agent-time estimates:

| Phase | Claude | Codex | Estimate |
|---|---|---|---|
| **B1** | `tmls-view` supervisor (child on same pty, clean detach-then-kill, terminal-state reset, disconnected/retry state); same-host and cross-host switch incl. no-client fallback; focus handoff; launch + fixed-width sidebar (relative resize + re-apply on OS-window resize) | ControlMaster config/warmup per host; tab-mode fallback wiring | ~3–4 h + one PC test session |
| **B2** | review only | `tmls open-link` (icat overlay, less overlay, system app), hints kitten mapping, `tmls paste-image`, `tmls.conf` installer/uninstaller | ~2–3 h + checklist session |
| **C** | per-host agent (stdin-shipped, tail from offset, tmux subscriptions, 50 ms coalesce, heartbeat/snapshot, poll fallback) | hooks installer (append into existing matcher arrays, backup, uninstall), events file writer, `tmls statusline` wrapper | ~3–4 h + one session with live Claude sessions |
| **D** | review only | sidebar UX (unread, pulse, subtitle, jump key, confirmed switch, sign-in row), game mode | ~2 h |
| Later | Laptop edition (tmux pane host in WSL); web/Android get the push feed | | after the PC is the daily driver |

Each phase works on its own branch. Claude merges only after its gate passes. The tab mode stays until the PC edition has been the daily driver for a week.

## Risks and open points

- **Focus handoff between two kitty windows.** It must feel instant. If `focus-window` round-trips are slow, the switch path moves to one socket call.
- **Client identification on the remote tmux.** The `TMLS_VIEWER` tag in the client env is read via `/proc` (Linux hosts only, which matches today).
- **Image pane.** The current one, drawn with textual-image inside the sidebar, is replaced by the kitty overlay in the PC edition. The tab-mode fallback keeps the old one.
- **Hooks in settings.json.** Claude Code reloads settings live. The installer is idempotent, keeps a backup, and has an uninstall command.
- **Status line wrapper.** It must pass through exactly what `cs` expects. Gate: the bar shows identical output before and after.
- **Terminal state on cross-host switch.** Killing `ssh -t … tmux attach` skips tmux's exit sequences, leaving
  alt screen, mouse tracking, bracketed paste, kitty keyboard protocol or cursor shape set. tmls-view first
  detaches the client (`tmux detach-client` over ControlMaster) and waits for ssh to exit; kill is the fallback,
  followed by a targeted mode-reset write (no full `reset`, which flickers).
- **Sidebar width.** `kitten @ resize-window` is relative. tmls reads the current width from `kitten @ ls`, sends the
  difference, and re-applies it when the OS window resizes.
- **Lost view.** If ssh dies (network, host reboot), tmls-view shows "disconnected — Enter to retry" in the pane
  instead of an empty window.
- **No client on the target host yet** (first launch, client died): same-host switch falls back to the cross-host path.
- **Existing hooks.** `settings.json` already has SessionStart/UserPromptSubmit hooks (caveman, superpowers). The
  installer appends inside existing matcher arrays; gate: those hooks still fire after install.
- **`cs` under the wrapper.** `cs` carries local source patches and has daemon/fast modes; the identical-bar gate is
  run with both modes.
- **Key-echo gate** proves the design (data path is identical by construction); keep it short. Switch and focus
  latency are the gates that matter.

## Why not tmls-rs

It was built to own a fast terminal pane. kitty already gives the best one on the PC. Its S0 key-echo harness is reused for the gates above. Restart it only if kitty or the pane approach fails a gate.
