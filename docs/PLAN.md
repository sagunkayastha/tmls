# tmls-rs — plan (draft 2, 2026-10-07)

Working name. A native, GPU-drawn terminal + session system in Rust that replaces tmls + tmux + ssh-per-key.
Status: **research done (docs/research/R1–R6). Next: user reviews this plan → Phase 0 spikes → implementation plan.**
Nothing built yet.

## 1. Why

tmls is laggy at times and misses keypresses (PC and laptop). Every key and every screen update passes through 2–3
terminal emulators, two of them in Python, and two ssh hops:

```
laptop terminal ─ssh→ tmls (Python/Textual) ─ pyte (Python VT) ─ ssh -t archbox ─ tmux client ─ tmux server ─ claude
```

Measured 2026-10-07: tmls's Terminal widget alone passes every key (396/396 quiet, 288/288 under a 100 Hz full-screen
flood), so the stack, not one widget, is the problem. tmls-rs removes layers: **one connection, one emulator per
screen, no ssh/tmux/Python per key.**

## 2. Requirements (user decisions)

| Topic | Decision |
|---|---|
| Priorities | **Best performance and visuals first.** Native desktop first, web second. |
| Clients | Linux PC (CachyOS, KDE Wayland, RTX 5070 Ti) + Windows laptop: **native desktop app, own GPU window**. Later: browser/iPad, then Android. |
| Session hosts | archbox, PC (laanta-chy). Windows laptop later (keep possible). |
| Migration | **Side by side with tmux**: new sessions in tmls-rs; existing tmux sessions listed and attachable; tmls stays until parity. |
| Scope | **Full parity with tmls** (§7) + the user's 31 "want" picks (R6). Built in phases. |
| Auth | Tailscale (personal tailnet) + per-device keys; no passwords on the wire (user note in R6). |

## 3. Decisions from research

| Question | Decision | Why / source |
|---|---|---|
| Build or reuse? | **Build** hostd and the desktop client; **reuse** libraries. No existing tool does multi-host + own PTY daemon + native client + hook status (R4 c). WezTerm: no release since 2024-02; Zellij/WezTerm mux not embeddable; Ghostty: no Windows GUI; Zed/Warp: GPL/AGPL (R1, R2). | R1 §9, R2 a, R4 c |
| Emulator core | **`libghostty-vt`** (Rust crate 0.2.2, 2026-09-28, MIT/Apache) in hostd and desktop, behind our own `Emu` trait. Has snapshot encode/restore, dirty-region render state, key/mouse/paste encoders, kitty keyboard + graphics, Windows + WASM. **Fallback: `alacritty_terminal` 0.26.** Needs Zig 0.16 at build time; pre-1.0 API → pin crate + ghostty commit. | R1 §1–6 |
| Desktop GUI | **`winit` 0.30 + `wgpu` 30 + own renderer and small UI layer** (sidebar, tabs, forms, toasts). Text: `cosmic-text` 0.19 + `swash`, own glyph atlas, instanced cell quads from damaged rows; box/block glyphs as geometry; `glyphon` only for UI chrome. **Runner-up: `gpui`** (crates.io copy stale since 2025-10; needs a pinned Zed git dep). Decided by spike S2. | R3 §1–6, R1 §7–8 |
| Session persistence | **One small holder process per session** (dtach/shpool style: PTY + byte ring with sequence numbers), in its own systemd cgroup; hostd re-adopts holders on restart, so hostd upgrades/crashes never kill sessions. | R2 b |
| Wire | **Raw bytes + sequence numbers** (not screen diffs: those lose scrollback/graphics). On attach: snapshot from hostd's emulator, then live bytes; on reconnect: replay from last seq (Eternal Terminal style). Output coalescing so input never queues behind a flood. | R2 c |
| Transport | **QUIC (`quinn` 0.11)** desktop↔hostd over Tailscale; **WebSocket** for web; one framed `proto`. No ssh. | R2 c |
| Typing prediction | **Not in v1** (LAN/Tailscale RTT well under where mosh prediction helps). Revisit for phone over cellular. | R2 c |
| tmux bridge | v1: attach through a PTY (`tmux attach` in a holder). Later: **control mode** (`tmux -C`, `%output`, `capture-pane -peC`, `refresh-client -C`). | R2 d |
| Claude status | **Hooks first** (`PermissionRequest`, `Stop`, `StopFailure`, `Notification`… as `http` hooks posting to hostd), reconciled with `~/.claude/sessions/<pid>.json` and `claude agents --json`; **context %** via a small status-line shim; screen parsing only as last resort. | R4 a |
| Web client (phase 5) | **xterm.js 6 + WebGL addon**; ghostty-web as runner-up (same API). Own input layer; Android keeps tmls's native invisible input (`TerminalKeys`). No Rust-to-WASM emulator for now. | R5 |
| Licences | Our repo MIT/Apache. Ideas only (no code) from GPL/AGPL projects: Zed terminal, COSMIC Term, Warp, Claude Squad, opcode, cmux. | R1, R3, R4 |

## 4. Architecture

```
┌─ desktop (Rust: winit + wgpu) ───────┐            ┌─ archbox ───────────────────────────────────┐
│ sidebar: hosts → sessions + marks    │            │ hostd (systemd --user)                      │
│ tabs · terminal grid (libghostty-vt) │◄── QUIC ──►│  ├─ holder: claude  (PTY + ring) ◄─ hooks   │
│ viewer · forms · toasts · palette    │  Tailscale │  ├─ holder: shell   (PTY + ring)            │
└──────────────────────────────────────┘            │  ├─ emulator per session (snapshots)        │
┌─ web (phase 5: TS + xterm.js) ───────┐            │  ├─ status: hooks + sessions/*.json         │
│ browser · iPad · Android WebView     │◄── WSS ───►│  └─ tmux bridge (existing sessions)         │
└──────────────────────────────────────┘            └─────────────────────────────────────────────┘
                                                      PC: hostd   ·   laptop: hostd (later, ConPTY)
```

Rust workspace:

| Crate | Job |
|---|---|
| `proto` | Framed, versioned messages: input, output(seq), resize, snapshot, session list/status, create/rename/kill, file read, ag send/read. |
| `emu` | `Emu` trait + libghostty-vt implementation (+ alacritty_terminal fallback behind a feature). Only crate that touches the emulator. |
| `holder` | Tiny per-session process: owns PTY, ring buffer, seq numbers; survives hostd restarts. |
| `hostd` | Supervises holders, one emulator per session, QUIC + WebSocket server, auth, status (hooks endpoint, Claude files), tmux bridge, files, `ag` inbox. |
| `client-core` | Connections + reconnect/replay, multi-host session list, per-tab emulator. No UI; unit-testable. |
| `desktop` | Window, renderer, UI layer, all user features. |
| `cli` | `ag`, `ct`, `tmls-rs attach NAME` (fallback inside any terminal), `tmls-rs doctor`. |

## 5. Phase 0 — spikes before committing (≈1–2 weeks total)

| Spike | Question | Pass |
|---|---|---|
| S1 Emulator | libghostty-vt vs alacritty_terminal: replay recorded Claude sessions, compare grids; snapshot round-trip; build on Linux + Windows MSVC (Zig 0.16). | libghostty-vt builds on both and matches; else alacritty_terminal. |
| S2 Renderer | Same 200×60 grid + sidebar in raw wgpu vs gpui; measure key-to-present latency on KDE Wayland/5070 Ti and the Windows laptop; check fractional scaling, emoji, ligatures, IME (fcitx5/ibus, Windows TSF). | Raw wgpu unless gpui is within ~1 frame and builds cleanly on Windows. |
| S3 Holder | Holder process survives `systemctl --user restart hostd`; hostd re-adopts it; no output lost (seq check). | Zero lost bytes across 20 restarts. |
| S4 Transport | quinn over Tailscale PC↔archbox and laptop↔archbox: key RTT, reconnect after Wi-Fi drop, replay correctness. | Reconnect < 1 s, replay exact. |
| S5 Claude status | Real permission prompt → which hook fires and when; `sessions/*.json` `status`/`waitingFor` values; status-line shim gives context %. | Each tmls mark derivable without screen parsing. |

## 6. Phases (each ends with the user using it for real)

| Phase | Delivers | Done when |
|---|---|---|
| 0 Spikes + spec | §5 spikes, final spec, implementation plan | User approves |
| 1 Core | proto, emu, holder, hostd (Linux), client-core, desktop: session list (archbox + PC), tabs, typing, resize, reconnect, snapshot attach; tmux sessions listed + attachable | Zero dropped keys in burst test; lower key-to-screen latency than tmls and plain ssh+tmux |
| 2 Daily driver | Status marks + context %, attention queue, new/rename/kill/windows/splits, scrollback + search, copy/paste (bracketed, smart Ctrl+C), mouse, links, quick select, notifications (easy off/on, sound toggle, suppress when focused), font zoom, session colours/icons, prompt history | User works a full day in it instead of tmls |
| 3 Agents | `ag ls/read/send` (+ Claude's own inbox, Codex threads), safe send / queue message, approve/deny from the list, `ct`, agent presets, saved prompts, auto-name | Existing agent workflows run unchanged |
| 4 Extras | File viewer (+ images), image paste/drop, url-screenshot, Sketch; worktree-per-session (maybe) | Matches tmls |
| 5 Web | Browser/iPad client on hostd (xterm.js), PWA | iPad replaces tmls web |
| 6 Android | WebView app with native input (port TerminalKeys), push | Phone replaces tmls app |
| 7 Laptop host | hostd + holder on Windows (ConPTY) | Laptop sessions listed/attached |

## 7. Parity checklist (tmls 2026-10-07) + "want" picks

tmls features: sessions per host + offline marking; background tmux servers folded; status marks ● ? ◆ ✕ ○ with hover
reason, Claude name + context %; tabs + Alt+Shift+arrows + list keys; mouse (wheel scrollback, drag copy, shift-drag,
ctrl-drag, Ctrl+C copy-or-interrupt, trailing-space trim); paste (system, bracketed, image → host file path); links
(URL with localhost remap, `file:line`, bare paths → viewer); quick select (Alt+Shift+S); new-session form (host,
folder, git-derived name, shell/claude/presets); rename (renames Claude too); kill with confirm; windows/splits;
`ag`, `ct`; notifications; approvals; prompts; file viewer (+ image pane); web (login, background toggle,
reload/version, Sketch, Send-to); Android (native input, copy/paste, keyboard control, links, camera in Sketch).

User's "want" picks (R6): agent-orchestration-api, agent-presets, auto-name, clickable-links, context-meter,
conversation-title, copy-options, create-session, desktop-notify, font-zoom, image-paste-drop, keyboard-sidebar,
kill-session, needs-input-detection, notification-inbox, notify-sound, notify-suppress-focused, prompt-history,
queue-message, quick-select, remote-approve, rename-session, saved-prompts, scroll-preserve, session-colors-icons,
smart-ctrl-c-paste, status-states-richer, tab-status-badge, tmux-wrap-local, url-screenshot, window-pane-actions.
"Maybe" picks are listed in R6 and decided per phase.

## 8. Success measures

- **Zero dropped keys**: automated burst test (thousands of keys, quiet and under full-screen flood), LAN + Tailscale.
- **Key-to-screen latency** measured (typometer-style) and lower than tmls and plain `ssh + tmux attach`.
- Redraw at monitor refresh with a busy Claude session; no frame drops on scroll.
- Session survives client crash, network drop, laptop sleep, **and hostd restart**; reattach shows exact screen instantly.

## 9. Risks

- libghostty-vt is pre-1.0, community Rust wrapper, Zig toolchain → pinned + behind `emu` trait; alacritty fallback.
- Wayland + NVIDIA frame pacing (Zed hit this) → S2 measures on the real PC.
- IME/preedit drawn by us (fcitx5/ibus, Windows TSF) → S2 checks early.
- Claude Code internals (`sessions/*.json`) undocumented → version-gated parser; hooks are primary.
- Scope: full parity is large → strict phases; tmls stays until parity.

## 10. Who does what (proposal)

Per the user's standing rule (Claude plans/specs/verifies; DeepSeek implements): Claude writes specs, spikes that
need the real machines (S2, S4, S5), reviews and merges; implementation tasks with clear specs go to DeepSeek
(opencode) or Codex. To confirm with the user.
