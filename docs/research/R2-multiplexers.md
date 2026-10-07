# R2 — Multiplexers, session daemons, remote-terminal transports (2026-10-07)

Scope: PLAN.md §3 open questions 3 (transport), 4 (persistence), 5 (latency tricks), 6 (reuse vs write hostd), plus the tmux-bridge question.
Evidence level: GitHub pages fetched 2026-10-07 for shpool, Zellij releases, WezTerm, Eternal Terminal, mosh, zmx (search only). The rest is from prior knowledge and is marked "(memory)"; verify before relying on exact dates or versions.

## 1. Comparison table

| Project | Lang / license | Who owns PTY | Who emulates | Wire content | Survives daemon restart | Reconnect / roaming | Multi-client sizes | Windows | Reusable pieces |
|---|---|---|---|---|---|---|---|---|---|
| tmux (+ `-CC`) | C / ISC | server | server (full grid per pane) | normal: server redraws as escape bytes for a client's TTY; `-CC`: `%output` text lines carrying raw pane bytes (octal-escaped) plus `%layout-change` etc. | No (server death kills panes) | none itself (rides on ssh) | `window-size latest/smallest/largest/manual`; smaller clients see dot-fill | WSL only | control-mode protocol (the only stable machine API) |
| Zellij | Rust / MIT | server (one per session) | server (own grid, per-client rendering) | client sends keys; server sends rendered screen frames (ANSI) to a thin client; web client uses WebSocket carrying the same | No, but "session resurrection" re-runs commands from a serialized layout (not live processes) | rides on ssh; web client over HTTPS/WS | v0.45 (Aug 2026): per-client tab sizing | No native (WSL) | WASM plugin API; `zellij-tile`; not reusable as a library |
| WezTerm mux | Rust / MIT | mux-server | mux-server holds `wezterm-term` per pane; GUI client also emulates | codec = PDU over unix socket / TLS TCP / ssh stdio; pane output is sent as raw bytes plus a rendered-line sync protocol (lazy line fetch) (memory) | No (mux server is a plain process) | no roaming protocol; reconnect re-syncs panes | per-client windows; pane size = one size, clients resize it | Yes (mux server and GUI run on Windows) | `wezterm-term`, `termwiz`, `portable-pty`, `mux`, `codec` crates (workspace crates, some on crates.io) |
| shpool | Rust / Apache-2.0 | daemon (one thread set per session) | daemon keeps `shpool_vt100` screen only for re-draw on reattach | raw PTY bytes to the client's own terminal; on attach a redraw of the stored screen | No; supports systemd unit and auto-daemonize | rides on ssh (`shpool attach` via ssh) | one client per session at a time; steals on re-attach, size = attached client | No (unix sockets) | `shpool_vt100` fork; small, readable codebase |
| abduco | C / ISC | server per session (one process) | none | raw bytes via unix socket | process per session, survives anything except host reboot | ssh | all clients share the PTY size | No | none |
| dtach | C / GPL-2 | master process per session | none | raw bytes | same as abduco | ssh | same | No | none |
| zmx | Zig / MIT (memory) | daemon, one process per session | libghostty-vt snapshot for re-attach (memory) | raw bytes + restore snapshot | per-session process (memory) | ssh | multiple clients on one session | No (macOS/Linux) | idea: the shpool model with libghostty-vt; 1.4k stars, active in 2026 |
| mosh | C++ / GPL-3 | `mosh-server` | server emulates (Terminal::Complete); client also emulates for prediction | SSP: UDP, AES-OCB, sends **screen-state diffs** (state-sync of the framebuffer), numbered states, roaming | server process lives until killed; no daemon to restart | best-in-class: UDP, IP roaming, sleep/wake | one size (the client's) | No native (Cygwin ports) | prediction algorithm only (GPL, so reimplement) |
| Eternal Terminal | C++ / Apache-2.0 | `etterminal` per user | none | raw bytes over reliable TCP with its own session layer, replays unacked bytes after reconnect | per-session process | reconnect + resume after IP change, no UDP; auth via ssh bootstrap | one PTY size | WSL only | none (design idea: ack/replay buffer) |
| sshx | Rust / MIT | client binary on the host | browser (xterm.js) | E2E-encrypted bytes through a relay server over gRPC/WS | no | relay re-sync | per-user cursors, shared size | Linux/mac, Windows (memory) | CRDT-free; encryption scheme (Argon2 key in URL fragment) |
| upterm | Go / Apache-2.0 | host's pty over ssh | none | ssh tunnel via a relay | no | no | shared | partial | none |
| tmate | C (tmux fork) / ISC | tmux server | tmux server | tmux protocol through ssh relay | no | no | tmux sizing | no | none |

## 2. Per-project notes

### tmux and control mode
- https://github.com/tmux/tmux (maintained, `3.5`/`3.6` line; ISC). https://github.com/tmux/tmux/wiki/Control-Mode
- Server owns PTYs and the emulated grid for every pane. Normal clients get a redrawn screen translated to their terminal's capabilities.
- `tmux -CC` / `-C`: client talks a line protocol over stdio. Output arrives as `%output %<pane> <bytes>` (non-printables octal-escaped, or `%extended-output` with pause/latency info when `pause-after` flow control is on). Notifications: `%window-add`, `%layout-change`, `%session-changed`, `%client-session-changed`, `%exit`. Commands are sent as normal tmux commands, replies are wrapped in `%begin/%end/%error`. Flow control: `refresh-client -f pause-after=N` and `refresh-client -A %pane:on/off/continue`.
- iTerm2 uses it as a native-UI mirror: each tmux window becomes a native tab/split, iTerm2's own emulator renders each pane from the raw `%output` bytes, scrollback is rebuilt via `capture-pane -pe`. So: raw bytes to a client-side emulator, which is exactly tmls-rs's planned model. Known pain points iTerm2 solved: initial state sync (`capture-pane` with escapes, then `%output` resumes; must pause output during the capture to avoid duplicates), resizing (client announces size with `refresh-client -C WxH`, tmux then lays out for the control client), no real alt-screen/scrollback history sharing across panes at attach.
- Control-mode clients count as clients for `window-size`; a control client that never sends a size can force `smallest` weirdness, so set `refresh-client -C` or `window-size` explicitly.

### Zellij
- https://github.com/zellij-org/zellij ; v0.45.1 released 2026-08-28, v0.45.0 2026-08-20 (nested sessions, Kitty graphics, OSC 133, per-client tab sizing, OSC9/99/777 notifications). MIT. Very active.
- Architecture: one server process per session, thin clients over unix socket (protobuf). Server runs its own VTE-based emulator per pane and composes frames; plugins are WASM (wasmtime) with a pane/tab/session API.
- Web client (added in the 0.43 line; page said 0.43.0): `zellij web`, built-in login tokens (also read-only tokens), enforced HTTPS, browser attach, mobile/PWA UI, `zellij attach https://host/session`. Closest existing thing to "daemon + native + web clients", but it is a full multiplexer with its own UI model (panes, tabs, status bars drawn server-side); tmls-rs wants the client to own layout and drawing.
- Not a library; the render path is server-side composited, so client-side GPU rendering would fight it. Persistence: no live-process survival across server restart, only layout resurrection.

### WezTerm mux
- https://github.com/wezterm/wezterm ; MIT; 29k stars; 1.5k open issues. Release cadence slowed a lot since 2024 (nightly builds are the practical release channel; latest tagged stable is 20240203) (memory). Treat as maintenance-mode, one-maintainer risk.
- Docs: https://wezterm.org/multiplexing.html. Domains: unix domain (local mux-server on a socket, `wezterm-mux-server`), TLS domain (server listens TCP with TLS, client certs bootstrapped via ssh), SSH domain (either "wezterm" mode, which tunnels to a remote mux-server via ssh stdio, or plain ssh with native terminals only).
- Architecture: mux-server owns PTYs and a `wezterm-term` Terminal per pane. Client GUI has local proxy panes: it fetches lines it needs lazily (render-line and dimension PDUs) and gets push notifications ("pane output") then pulls changed lines; so wire is neither pure bytes nor pure frames, it is line-level state sync with sequence numbers. This is why reconnect is cheap but also why it is complex and has been a known source of lag/scroll bugs (issues around remote scrollback, mux latency).
- Not persistent across server restart. No roaming. Auth: TLS client cert or ssh.
- Reusable: `wezterm-term` and `termwiz` are the strongest Rust emulator candidates (R1 covers), `portable-pty` is already in PLAN. The `mux` and `codec` crates are tightly coupled to the GUI (Lua config, `window` etc.), so reuse = painful fork.

### shpool
- https://github.com/shell-pool/shpool ; Apache-2.0; Rust; Google-originated (shell-pool org); 428 commits, active; debian changelog in repo.
- Closest conceptual match to hostd's session-keeping half. Daemon (systemd user unit or autodaemonize) owns PTYs, one thread group per session. It keeps an in-memory `shpool_vt100` render of each session so reattach redraws the screen, but sends raw bytes to the user's real terminal (native scrollback). Attach is via unix socket; remote use is `ssh host shpool attach name`. Single attached client per session (new attach kicks or errors out the old one).
- No Windows. No client-side emulation, no structured protocol for a GUI: you would be reading its internals, not a public API. Persistence: no, daemon restart kills sessions (known limitation).
- Takeaways to copy: per-session small vt100 for redraw, `SHPOOL__*` session env hooks, prompt-prefix detection, systemd socket activation.

### abduco / dtach
- https://github.com/martanne/abduco (ISC; low activity, stable), https://github.com/crigler/dtach (GPL-2; very old, stable).
- One tiny process per session holds the PTY master, listens on a unix socket; clients attach and get raw bytes. No emulation, so no screen restore: on attach they send a redraw request (SIGWINCH / Ctrl-L). Survives everything but reboot, including "upgrades" of any front-end tool because the holder is trivial and rarely changes.
- This is the model for the "holder process" persistence answer in §3b. Lacks the thing tmls-rs needs (instant exact screen), which must come from a second component that also sees the bytes.

### zmx and 2025-2026 persisters
- https://github.com/neurosnap/zmx ; Zig; ~1.4k stars; macOS/Linux; "session persistence without windows/tabs/splits"; attach/detach, multiple clients, restores prior state and output on reattach; per-session processes (memory). Uses libghostty's VT parser to build the restore snapshot (memory; verify). Active (commits through 2026 on the pgs.sh mirror).
- Confirms the market direction: shpool-style persistence, no multiplexer UI, snapshot via a real emulator. Same shape as the planned hostd, minus the structured protocol.
- Others to keep an eye on (memory, unverified): `dvtm`+`abduco` combos, `sesh` (tmux session manager, not a persister), `mprocs`.

### mosh
- https://github.com/mobile-shell/mosh ; GPL-3.0; 14.5k stars; 1471 commits; released 1.4.0 in 2022, development slow, mostly maintenance; stable since. Paper: https://mosh.org/mosh-paper.pdf .
- SSP (State Synchronization Protocol): UDP datagrams, AES-128-OCB, per-direction numbered states. Client sends user-input *diffs* (keystroke stream as state); server sends **diffs of the screen framebuffer** relative to a state the client acked, rate-limited to adapt to network (frame-rate throttling, so floods like `cat bigfile` do not queue). Roaming comes free (any authenticated packet from a new source address moves the endpoint). Bootstrap via ssh that starts `mosh-server` and returns a key.
- Prediction: client keeps its own copy of the emulator; typed characters are speculatively echoed (underlined when unconfident) after a confidence epoch scheme; incorrect predictions are repaired when the server state arrives. Tunable (`--predict=adaptive|always|never|experimental`). Adaptive mode only predicts when RTT exceeds ~30 ms (SRTT trigger around 30 ms, flag threshold ~80 ms for underlining) (memory).
- Weaknesses: no scrollback (emulator is screen-only, which is why users pair it with tmux), no native scroll, limited OSC/image/kitty protocol support because the diff format only carries what mosh's emulator understands, no multi-client, no Windows, GPL so no code reuse.
- Lesson for tmls-rs: do not adopt diffs-only; keep raw bytes, and take from mosh: (1) server-side flood throttling, (2) datagram roaming, (3) predictive echo as an optional layer.

### Eternal Terminal
- https://github.com/MisterTea/EternalTerminal ; Apache-2.0; 3.9k stars; 1144 commits; active enough. 
- ssh bootstraps auth, then client talks to `etserver` (TCP 2022) which spawns `etterminal`. Own encrypted, reliable stream (libsodium) with sequence numbers and a backup buffer; after disconnect, client reconnects and both sides replay unacked bytes. So raw byte pass-through with true scrollback, native terminal rendering, TCP not UDP, no prediction.
- Windows: WSL only. Persistence: per-session process; etserver restart drops connections but terminals live in etterminal (memory).
- Lesson: the cheap way to get "survive network drop" is an application-level ack/replay window over a normal reliable transport; no custom UDP needed.

### sshx, upterm, tmate (collaboration relays)
- sshx: https://github.com/ekzhang/sshx , MIT, Rust. Host binary spawns shells, streams bytes through a relay server (gRPC/WebSocket); browser UI with xterm.js, infinite canvas, E2E encryption with key in the URL fragment. Relevant for web-client bytes model and the encryption trick; relay architecture not needed (Tailscale).
- upterm: https://github.com/owenthereder/upterm (actual repo `owenthereder/upterm`, memory; verify) Go, Apache-2.0; ssh reverse tunnel to a relay.
- tmate: https://github.com/tmate-io/tmate , tmux fork with ssh relay, ISC.
- None offer anything to reuse; included for the "share a session" feature only.

## 3. Answers

### (a) Own hostd or build on an existing one?
Write own hostd, borrow pieces.
- WezTerm mux: the only one with a Windows story and Rust emulator, but its mux client protocol is tied to wezterm's GUI, development is slow, the line-pull sync is the type of layer that is hard to make fast and is where your stated pain (lag, missed keys) would reappear, and persistence is not there. Reuse its crates (`wezterm-term` or `termwiz`, `portable-pty`), not its mux.
- Zellij: server-composited UI model conflicts with a client-owned GPU UI; plugin system is attractive but wrong layer; no library.
- shpool: right shape (daemon + vt100 per session, Apache-2.0, Rust), but no structured protocol, one client at a time, no Windows, no snapshot of scrollback, and its internals are not published as a crate API. Best use: read the code, possibly depend on / fork its `shpool_vt100` if the R1 emulator choice does not cover re-attach snapshots. Forking whole shpool means rewriting its protocol layer anyway, which is most of what hostd is.
- Core of hostd is small (PTY spawn, a byte ring + emulator per session, a framed protocol, fan-out to N clients). The hard, differentiating parts (status detection, Claude integration, protocol, tmux bridge) are yours regardless. Writing it costs ~2-3 weeks to a usable core; adapting costs similar and carries someone else's model.

### (b) Persistence model
Recommended: **hybrid, holder process per session + a stateless-ish hostd**.
- `tmls-hold` (tiny Rust binary, dtach-class): owns PTY master and child, listens on a per-session unix socket (`$XDG_RUNTIME_DIR/tmls-rs/<id>.sock`), keeps a bounded raw-byte ring (for replay) and appends a sequence number. Almost never changes, so it can outlive hostd upgrades. Under systemd put holders in a separate unit/cgroup (`KillMode=process` on hostd, or holders started via `systemd-run --user --scope`/`Delegate`) so restarting hostd does not kill them.
- `hostd` connects to each holder socket on start (re-adopts), owns the emulator per session (rebuilt by replaying the ring + a periodic holder-side snapshot, or holder runs the vt100 as shpool does so the snapshot also survives), clients, protocol, status detection.
- Cost: an extra IPC hop (unix socket, microseconds) and a second protocol (versioned, kept minimal). Benefit: upgrade/restart/crash of hostd never kills a Claude session, which is the main practical failure with 3-day agent runs. In-daemon only is simpler and is what tmux/shpool/Zellij do, but they then all lose sessions on upgrade. For Windows later: ConPTY holder as a per-session process works equally (named pipe instead of unix socket), which fits the holder model better than a service.
- Minimum viable: start in-daemon with the holder boundary designed in (session = trait, PTY side behind a socket-shaped interface), add the holder before Phase 2 daily-driver.

### (c) Transport and prediction
- **QUIC (quinn) for desktop, WebSocket for web, same framed protocol on both** (length-prefixed messages; QUIC stream per session plus one control stream). Reasons: no head-of-line blocking between sessions (a flood in one tab cannot stall keystrokes in another, which is a real tmux-over-ssh problem since all share one TCP), 0-RTT/connection migration for laptop sleep and network switches (Tailscale already handles IP change under the hood, so migration is a bonus), TLS 1.3 with pinned per-device keys (rustls) fits the auth plan. Tailscale caveat: UDP over DERP relays works but adds latency; direct path is typical on the LAN. Keep a TCP/WebSocket fallback for networks that block UDP; since you need the WS path for the browser anyway, the fallback is free.
- ssh transport: skip. Its only wins (auth, existing keys) are replaced by Tailscale identity + device keys, and the per-key ssh cost is the thing being removed. Optionally allow `ssh -L`-style bootstrap for first pairing.
- Resume semantics: do the ET trick on top of any transport: sequence-numbered output stream per session; client reconnects with "last seq seen"; hostd replays from the ring or sends a fresh snapshot if the gap exceeds the ring. This covers laptop sleep without UDP-level SSP cleverness.
- Latency: on LAN/Tailscale RTT is ~1-30 ms, below mosh's adaptive threshold (mosh itself turns prediction off below ~30 ms). The dominating latency here is the layer stack and render pipeline, not the wire. **Skip prediction in v1.** Reconsider only for the web/phone client over cellular (RTT 60-200 ms), as an optional adaptive local-echo mode, and only for plain printable characters at a shell prompt (Claude Code's TUI redraws make speculation unreliable). Do copy mosh's *flow control*: coalesce output per frame interval (e.g. 4-8 ms) and drop intermediate frames of a flood by snapshotting, so Ctrl-C / typing is never queued behind megabytes of output. Prioritise input messages over output on the client's send path.
- Do not go mosh-style diff-only on the wire: loses scrollback, Kitty graphics, OSC passthrough, and can't feed an xterm.js client.

### (d) Bridging existing tmux sessions
Use **control mode (`tmux -C`, with `-CC` semantics not needed outside iTerm)**, one control client per tmux server/session, run inside hostd.
- Why: gives structured per-pane raw bytes (`%output`), layout and window events, size negotiation (`refresh-client -C`), pause/flow control, and commands (new-window, kill, rename) with a stable, documented interface. Each pane maps to a tmls-rs "session" whose bytes feed the same client-side emulator; initial screen from `capture-pane -peC -S -` (escapes included) taken while output is paused, then resume with `%continue`.
- Pty attach (run `tmux attach` in a PTY and treat it as a normal session) is the zero-effort fallback: fully robust, but you get tmux's redraw escape stream for a single terminal, tmux status bar and its own window chrome stay, panes are not individually addressable, and you keep the extra emulation layer. Good as a Phase 1 stopgap ("attach any tmux session as a plain terminal"), and as the always-works fallback when the control bridge misbehaves.
- Control-mode pitfalls to design for: octal-escaped output decoding, scrollback only from capture-pane (not from the stream), sizes (the control client counts for `window-size`; set `refresh-client -C` to the active tmls-rs size and restore on detach, or use `window-size latest`), `%output` for panes in unattached windows, flow control to avoid tmux buffering GBs when the GUI is slow, and tmux version differences (`pause-after` needs 3.2+).
- Suggested order: Phase 1 pty-attach for tmux, Phase 2 control-mode bridge, retiring pty-attach once the bridge is solid.

## 4. Net recommendation table for PLAN §3

| Q | Recommendation |
|---|---|
| 3 Transport | QUIC (quinn) desktop + WebSocket web, one framed protocol; ET-style seq/ack replay for resume; no ssh. |
| 4 Persistence | Per-session holder process (dtach-class, with seq ring) + hostd that re-adopts them; start in-daemon behind the same trait if time-boxed. |
| 5 Prediction | Not in v1. Adopt mosh's flow-control/throttling, not its prediction. Revisit for WAN/mobile web only. |
| 6 Reuse | Own hostd. Reuse `portable-pty`, emulator crate from R1 (`wezterm-term`/`termwiz` or libghostty-vt), and ideas from shpool (vt100 per session) and ET (replay). |
| tmux bridge | Pty attach first, control mode as the real bridge. |

## 5. Gaps / to verify
- Exact current WezTerm release state (stable tag vs nightly) and whether `wezterm-term` on crates.io is current.
- zmx internals (libghostty-vt use, license) and shpool's protocol docs; read source before copying design.
- Mosh 1.4.0 date and prediction thresholds; Zellij web-client exact version (page showed "0.43.0").
- Holder-process survival under systemd (`KillMode`, cgroup) needs a 10-line experiment on archbox before the design is frozen.
