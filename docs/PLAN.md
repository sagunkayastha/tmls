# tmls-rs — plan (draft 1, 2026-10-07)

Working name. A native, GPU-drawn terminal + session system in Rust that replaces tmls + tmux + ssh-per-key.
Status: **draft — research phase next, then the spec is finalised, then implementation.** Nothing built yet.

## 1. Why

tmls today is laggy at times and misses keypresses (PC and laptop). Every key and every screen update passes
through 2–3 terminal emulators, two of them in Python, and two ssh hops:

```
laptop terminal ─ssh→ tmls (Python/Textual) ─ pyte (Python VT) ─ ssh -t archbox ─ tmux client ─ tmux server ─ claude
```

Measured 2026-10-07: tmls's own Terminal widget passes every key (396/396 quiet, 288/288 under a 100 Hz full-screen
flood), so the widget is not the bug; the stack is. A rewrite only helps if it **removes layers**, not just
changes language.

## 2. What the user asked for (decisions so far)

| Topic | Decision |
|---|---|
| Priorities | **Best performance and visuals first.** Compatibility: native desktop first, web second. |
| Clients | Linux PC (CachyOS) and Windows laptop: **native desktop app, own GPU window** (like kitty/WezTerm). Later: browser/iPad, Android. |
| Session hosts | archbox, PC (laanta-chy). Laptop (Windows) later — keep it possible (ConPTY), don't plan it now. |
| Migration | **Side by side with tmux**: new daemon for new sessions; the app also lists/attaches existing tmux sessions. tmls stays in use until parity. |
| Scope | **Full parity with tmls** (everything in §6). Built in phases; tmls retires only at parity. |
| Language | Rust for daemon, protocol, desktop client, CLI tools. Web client: TypeScript (phase 2). |

## 3. Architecture (draft — research may change parts)

```
┌─ Desktop app (Rust, GPU) ────────┐        ┌─ archbox ─────────────────────────────┐
│ winit window + wgpu renderer     │        │ hostd (Rust, systemd user service)    │
│ terminal emulator per tab        │◄──────►│  ├─ session: claude  (PTY + emulator) │
│ session list · tabs · viewer     │  QUIC  │  ├─ session: shell   (PTY + emulator) │
└──────────────────────────────────┘  over  │  ├─ status: Claude files, activity    │
┌─ Web client (phase 2) ───────────┐ Tail-  │  └─ tmux bridge (control mode)        │
│ TypeScript + xterm.js (WebGL)    │ scale  └───────────────────────────────────────┘
│ browser · iPad · Android WebView │◄──WSS─►  PC: hostd   ·   laptop: hostd (later)
└──────────────────────────────────┘
```

Rust workspace, five crates:

| Crate | Job |
|---|---|
| `proto` | Messages + wire format shared by all: keys, output, resize, session list, status, file reads. Versioned. |
| `hostd` | Daemon per host. Owns PTYs (`portable-pty`; ConPTY later), keeps sessions alive with no client, one emulator per session (instant exact screen + scrollback on attach/reconnect), Claude status detection, create/rename/kill, `ag` inbox, file reads for the viewer, tmux bridge. |
| `client-core` | Connections, reconnect, multi-host session list, per-tab emulator. No UI; unit-testable. |
| `desktop` | Window, GPU text rendering, tabs, session list, mouse, links, quick select, file viewer, image paste, notifications. |
| `cli` | `ag`, `ct`, `tmls-rs attach NAME` (fallback that runs inside any terminal). |

Key choices (to confirm in research):
- **One hop**: client ↔ hostd directly (QUIC via `quinn` for desktop; WebSocket for web). No ssh/tmux/Python per key.
- **Emulation**: daemon holds the authoritative screen state; client runs its own emulator per tab, fed the same
  byte stream; on attach the daemon sends a snapshot (screen + scrollback) then live bytes.
- **Auth**: Tailscale network + per-device key (mTLS / pinned keys). No passwords on the wire.
- **Sizes**: a session has one PTY size = most recently active client (tmux `latest` behaviour); others letterbox.

Open design questions (answer during research/spec):
1. Emulator crate: `alacritty_terminal` vs `wezterm-term`/`termwiz` vs `libghostty` (Zig, C ABI) vs `vt100`.
2. GUI stack: raw `winit`+`wgpu`+own text (glyphon/cosmic-text/swash) vs `gpui` (Zed) vs `iced` vs `egui` vs `slint`.
3. Transport: QUIC (`quinn`) vs WebSocket-only vs ssh-subsystem; reconnect + roaming (mosh/Eternal Terminal ideas).
4. Persistence: sessions survive a hostd restart? (Separate per-session holder process, like shpool/dtach, vs in-daemon.)
5. Latency tricks: mosh-style local echo/prediction — worth it on LAN/Tailscale?
6. Build on an existing multiplexer (WezTerm mux, Zellij, shpool) instead of writing hostd?
7. Windows: one desktop codebase for Linux + Windows (font rendering, IME, DPI).

## 4. Research (next step — before the spec is final)

Like tmls's 2026-09-30 feature survey (picker page https://claude.ai/artifact/BYzRPajmDwjFWppwCsqkZq), survey
existing tools, then decide build vs reuse per component. Output: `docs/research/*.md` + a summary table with a
recommendation per open question in §3.

| Batch | Look at | Questions |
|---|---|---|
| R1 Rust terminals & emulator libs | Alacritty, WezTerm, Rio, Ghostty/libghostty, Zed terminal, Warp, kitty (C, reference), foot | Which emulator core to embed; GPU text rendering approach; input latency numbers; Windows support |
| R2 Multiplexers & session daemons | tmux (control mode), Zellij, WezTerm mux, shpool, abduco/dtach, mosh, Eternal Terminal, zmx-like | Reuse a daemon vs own hostd; persistence model; reconnect/roaming; multi-client sizing |
| R3 GUI toolkits for a GPU terminal app | winit+wgpu, gpui, iced, egui, slint, Tauri (for comparison) | Text quality, latency, IME, DPI, Linux Wayland + Windows, maturity |
| R4 Agent/session managers | Claude Squad, Crystal, Conductor, vibe-kanban, opcode/Claudia, Codex desktop, Warp agents, sshx, upterm | Features to copy (status, notifications, worktrees), how they track Claude state |
| R5 Web terminal clients (phase 2) | xterm.js, ghostty-web/wasm, ttyd, gotty, wetty, sshx | Best web renderer; reuse of the Rust emulator via WASM; mobile/IME handling |
| R6 Old tmls survey | picker page picks (want/maybe) | Carry the user's previous picks into this app's parity list |

## 5. Phases (each ends with the user using it for real)

| Phase | Delivers | Done when |
|---|---|---|
| 0 Research + spec | §4 surveys, final spec, implementation plan | User approves spec and plan |
| 1 Core | `proto`, `hostd` (Linux), `client-core`, desktop app: session list (both hosts), tabs, typing, resize, reconnect | Typing never drops a key in a measured test; input-to-screen latency below tmls's on LAN and laptop-over-Tailscale |
| 2 Daily-driver parity | Status marks (running/waiting/needs-you/error, context %), new/rename/kill/windows/splits, scrollback, copy/paste (bracketed), mouse, links, quick select, notifications, tmux bridge | User works a full day in it instead of tmls |
| 3 Agent tools | `ag ls/read/send` + inboxes, `ct`, agent presets, approvals | Existing agent workflows run unchanged |
| 4 Extras | File viewer (images too), image paste, Sketch | Matches tmls |
| 5 Web client | Browser/iPad page on the same daemon | iPad replaces tmls web |
| 6 Android | WebView app with native input (port of tmls's TerminalKeys fixes) | Phone replaces tmls app |
| 7 Laptop host | `hostd` on Windows (ConPTY) | Laptop sessions listed/attached |

## 6. Parity checklist (from tmls README/PROGRESS, 2026-10-07)

- Sessions per host (local + remote), offline marking, refresh; background tmux servers folded
- Claude sessions outside tmux (kitty) — becomes moot if all sessions live in hostd; decide in spec
- Status marks: running ●, waiting ? (with hover reason), needs-you ◆, API error ✕, idle ○; Claude name + context %
- Tabs; Alt+Shift+arrows; list navigation (j/k, Enter, Esc); detach ×
- Mouse: wheel scrollback, drag select/copy, shift-drag native, ctrl-drag copy, Ctrl+C copy-or-interrupt, trailing-space trim
- Paste: system clipboard, bracketed paste, image paste (store file on host, paste path)
- Links: URL open (localhost remap), `file:line` and bare paths open in viewer
- Quick select (Alt+Shift+S) for URLs, paths, hashes
- New session form (host, folder, name from git repo, shell/claude/agent presets); rename (renames Claude too); kill with confirm; windows/splits
- `ag ls/read/send` (+ `--from/--mode`, Codex threads, inbox sockets), `ct`
- Notifications/alerts; approvals (`approve.py`); prompts
- File viewer (+ image pane, in progress in tmls)
- Web: login, background servers toggle, reload/version, Sketch, Send-to
- Android: native input, copy/paste, keyboard control, links, camera/photo in Sketch

## 7. Success measures

- **Zero dropped keys** in an automated burst test (thousands of keys, quiet and under full-screen flood), over LAN and Tailscale.
- **Input-to-screen latency** measured (typometer-style) and better than tmls and plain `ssh + tmux attach`.
- Smooth scroll and redraw at the monitor's refresh rate with a busy Claude session.
- Session survives client crash, network drop, laptop sleep; reattach shows the exact screen instantly.
