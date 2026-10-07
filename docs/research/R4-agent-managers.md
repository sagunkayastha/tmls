# R4 - Agent / session managers (research, 2026-10-07)

Scope: tools that manage many AI coding-agent sessions, and how they track Claude Code state.
Method: web search + GitHub/docs fetches, plus checks on this machine (Claude Code 2.1.292, archbox).
Items marked (unverified) came from search snippets or memory, not a primary page.

## 1. Comparison table

| Tool | Tech / platform | Session host | State detection | Notifications | Multi-machine | Licence | Activity |
|---|---|---|---|---|---|---|---|
| Claude Squad | Go TUI, Linux/mac | tmux + git worktree per session | Screen scraping: SHA256 of `capture-pane`; greps prompt strings ("No, and tell Claude what to do differently"); auto-Enter on trust prompts; `--autoyes` | none to speak of | no | AGPL-3.0 | 8.6k stars, 222 commits, active |
| Crystal (stravu) | Electron | own PTY + worktrees | parses Claude output/JSON stream | desktop | no | MIT | DEAD: final v0.3.5 2026-02-26, successor Nimbalyst |
| Conductor (conductor.build) | native Mac app, Apple Silicon only | worktrees; local, or "Conductor Cloud" microVMs | agent-integrated (closed) | in-app | cloud VMs only | closed source, free | active, funded |
| vibe-kanban (BloopAI) | Rust (axum/SQLx) + React; npx or Tauri | worktrees under `.vibe-kanban-workspaces` | drives agents via their structured protocols | in-app | SSH-tunnel remote deploy | Apache-2.0 | 28.3k stars, but **sunsetting** |
| opcode (ex-Claudia, winfunc) | Tauri 2 (Rust + React), SQLite | spawns `claude` as child process (not a PTY terminal); reads `~/.claude/projects` | parses stream-json / transcript JSONL | minimal | no | AGPL | 22.4k stars; feature set = checkpoints/timeline, usage analytics, custom agents, MCP registry |
| Claude Code built-in "agent view" (`claude agents`, `--bg`) | Claude Code itself | supervisor daemon hosts each bg session as its own claude process | first-party: `~/.claude/jobs/<id>/state.json`, `claude agents --json` | `Notification` hook types `agent_needs_input` / `agent_completed` + terminal notif | local only | proprietary | current, the reference |
| Claude Code Remote Control / teleport / cloud | Claude Code + claude.ai + mobile apps | local session reachable from phone/browser via outbound-only HTTPS; teleport moves a cloud session to local | first-party | mobile push | via Anthropic relay | proprietary | current |
| Codex app / CLI (OpenAI) | Mac (Feb 2026) + Windows (Mar 2026) app; Rust CLI | worktrees, automations queue; `codex app-server` JSON-RPC over stdio | structured events `thread/*`, `turn/*`, `item/*`, `requestApproval` server requests | in-app | cloud tasks | CLI Apache-2.0 (unverified); app closed | very active |
| Warp (agent mode, Oz) | Rust GPU terminal, closed-ish | PTY per tab; Oz = cloud agents | agent-notification integration for Claude/Codex/Oz; attention badge on tabs; Agent Management Panel | in-app + desktop | Oz cloud runs | proprietary (unverified for 2026) | active |
| Zed agent panel | Rust, gpui | ACP external agents (Claude, Codex, Gemini); "terminal threads" host any CLI as a thread in a sidebar; parallel threads | ACP protocol events (structured) | in-app | Zed remote dev (SSH) | GPL/AGPL/Apache mix | very active |
| Cursor cloud/background agents | Electron IDE + iOS app (2026-06-29) | cloud Ubuntu VMs; separate "remote agents" on own devbox | closed | mobile + web | cloud / devbox | proprietary | very active |
| cmux (manaflow-ai) | native macOS Swift/AppKit on libghostty | Ghostty panes; SSH workspaces attach remote tmux | OSC 9/99/777 + `cmux notify` + agent hooks (Claude, Codex, OpenCode, Pi) | blue ring on pane, tab lights, notification panel | SSH + remote tmux, scp drop | GPL-3.0 client, BSL server | 27.7k stars, hot |
| agent-deck (asheshgoplani) | Go + Bubble Tea, SQLite | tmux (isolated socket), worktrees, Docker sandbox | Claude via **hooks**; others via screen parse / Codex notify | Telegram/Slack/ntfy via "conductor", web push | remote SSH hosts; web UI | MIT | ~1k stars, active |
| amux (mixpeek) | single Rust binary, axum + SQLite + web SPA | tmux panes | watchdog (auto-compact, restart, replay last msg) | web/phone, mac Messages | self-hosted, Tailscale | MIT + Commons Clause | ~520 stars, active |
| tmux-ccm / tmux-claude-session-manager / tmux-claude-status-tabs / tmux-agent-status | tmux plugins (shell/Python) | tmux sessions | **hooks first**, fallback transcript-JSONL heartbeat + footer pattern match + process tree | tmux bar / popup | tmux only | MIT | small (16 stars for ccm) but design is instructive |
| sshx | Rust server+CLI, Svelte web | process under a PTY, streamed via gRPC mesh | n/a | n/a | any browser, E2E crypto (Argon2/AES), Mosh-style predictive echo | MIT | 7.7k stars; **no self-host** |
| upterm / tmate | Go / C (tmux fork) | reverse SSH tunnel to relay | n/a | n/a | yes | MIT-ish / tmate marked discontinued | low relevance |

## 2. Per-tool notes

### Claude Squad - https://github.com/smtg-ai/claude-squad
Go TUI; every instance = tmux session + git worktree. Status is pure screen scraping (`session/tmux/tmux.go`: hash of `capture-pane`, string match on the permission-dialog text, auto-Enter on trust prompts). Copying that is exactly what to avoid: brittle against Claude UI changes (confirmed by Claude's own UI changing in 2.1.x), but it shows the cheap fallback for agents with no hooks. AGPL: do not copy code, ideas only.

### Crystal / Nimbalyst - https://github.com/stravu/crystal
Electron, MIT, abandoned 2026-02; successor Nimbalyst (editor-centric). Nothing to adopt except "compare N attempts of one prompt side by side".

### Conductor - https://www.conductor.build
Mac-only, closed. Strong UX: per-workspace diff, checks, PR flow, merge+cleanup. Conductor Cloud adds microVMs. Not usable by us (Linux/Windows), but the workspace = branch + chat + terminal + diff + PR model is the benchmark for "review surface".

### vibe-kanban - https://github.com/BloopAI/vibe-kanban
Rust backend, Apache-2.0, 28k stars, now sunsetting (repo banner). Kanban over worktrees, inline diff comments, built-in preview with devtools. Lesson: kanban is popular but the project still died; keep tmls-rs session-centric.

### opcode - https://github.com/winfunc/opcode
Tauri/Rust, AGPL, 22k stars. Browses `~/.claude/projects`, checkpoints/timeline with branching and restore, usage analytics, custom agent runs in background, MCP registry. Worth copying: transcript browser, usage dashboard, checkpoint restore. Not a terminal (no PTY fidelity), so it is not a competitor for tmls.

### Claude Code built-in: agent view, `--bg`, daemon - https://code.claude.com/docs/en/agent-view
Biggest finding of this batch. Since ~2.1.2xx Claude Code ships a supervisor daemon (`claude daemon`), `claude --bg`, `claude agents` (TUI), `claude attach|logs|stop|respawn|rm`, and **a documented JSON API**: `claude agents --json [--all] [--cwd]`. Fields: `state` (working | blocked | done | failed | stopped), `status` (busy | waiting | idle), `waitingFor` ("permission prompt", "input needed", "sandbox request", "dialog open"), `sessionId`, `name`, `pid`, `kind` (interactive | background). Per-job `~/.claude/jobs/<id>/state.json` (state, detail, tempo, inFlight tasks/monitors, tokens, output.result, respawnFlags). Limit: local machine only; stops on shutdown. Verified on archbox (see section 3).

### Claude Code hooks - https://code.claude.com/docs/en/hooks
30+ events including `Notification` (types `permission_prompt` fires ~6 s after the dialog, `idle_prompt` ~60 s after finishing, `elicitation_*`, `agent_needs_input`, `agent_completed`, `quota_auto_resume_*`), `PermissionRequest` (carries tool_name, tool_input, suggestions; can allow/deny, so it replaces tmls `approve.py` cleanly), `Stop`, `StopFailure` (error types rate_limit, overloaded, authentication_failed, billing_error, max_output_tokens, ...), `SessionStart/End`, `SubagentStart/Stop`, `PreCompact/PostCompact`, `CwdChanged`, `WorktreeCreate/Remove`. Hook types: command, **http** (POST JSON), mcp_tool, prompt, agent. Common payload has `session_id`, `transcript_path`, `cwd`, `permission_mode`.

### Claude Code statusline - https://code.claude.com/docs/en/statusline
Script gets JSON on stdin: context-window `used_percentage`, `rate_limits.five_hour/seven_day.used_percentage` + `resets_at`, cost, model, session id, transcript path; `refreshInterval` option. This is the only first-party source of context %, so tmls-rs needs a statusline shim (or the existing `cs` bar's cache) to feed it.

### Remote Control / teleport - https://code.claude.com/docs/en/remote-control
Local session exposed to claude.ai/code and mobile app via outbound-only HTTPS (no inbound ports); survives sleep. `/teleport` pulls a cloud session to local. Server mode: one remote session per instance. Complementary, not a substitute: it routes via Anthropic, no multi-host dashboard, and not available for Codex or shells. Session JSON files here carry `bridgeSessionId`, `messagingSocketPath`, `peerProtocol` + `peerFeatures` (`notify_idle`, ...), i.e. Claude instances already have a local peer-messaging socket (`/run/user/1000/cc-socks/<pid>.sock`) - relevant to `ag send` (undocumented, treat as unstable).

### Agent SDK
Headless/streaming mode with structured messages; right tool for programmatic tasks (tmls-rs presets could spawn SDK sessions), but you lose the TUI the user wants to type into. Use the SDK for background automation, PTY for interactive work.

### Codex app / CLI - https://openai.com/index/introducing-the-codex-app/
Mac app Feb 2026, Windows Mar 2026. Parallel agents in worktrees, skills, "Automations" with a review queue. CLI exposes `codex app-server` (JSON-RPC 2.0 over stdio, `thread/started`, `turn/*`, `item/*`, `item/*/requestApproval`) plus a `notify` program hook and rollouts in `~/.codex/sessions/*.jsonl`. So Codex state is also obtainable without screen scraping.

### Warp - https://docs.warp.dev/agent-platform/capabilities/agent-notifications.md
Agent Management Panel, attention badges on tabs, auto-mark-read on focus, Oz cloud agents (Feb 2026). Best polished "which tab needs me" UX in a GPU terminal. Closed/hosted-account flavour; not self-hostable for our case.

### Zed - https://zed.dev/docs/ai/external-agents.md , https://zed.dev/blog/terminal-threads
ACP (Agent Client Protocol) for structured external agents; "terminal threads" put any CLI agent as a managed thread in a sidebar; parallel threads. Zed uses gpui, which is also an R3 candidate. Worth watching ACP as a future uniform agent protocol.

### Cursor - https://www.morphllm.com/cursor-background-agents
Cloud agents in VMs, iOS app (2026-06-29), "remote agents" on own devbox controlled from anywhere. Validates phone-as-monitor, which is tmls-rs phases 5-6.

### cmux - https://github.com/manaflow-ai/cmux
The closest "native terminal built around agents" today: 27.7k stars, Swift/AppKit on libghostty, vertical tabs, notification rings via OSC 9/99/777 and `cmux notify`, hooks for Claude/Codex/OpenCode/Pi, SSH workspaces onto remote tmux, scriptable socket/CLI, built-in browser pane. macOS only, GPL-3 (server parts BSL).

### agent-deck - https://github.com/asheshgoplani/agent-deck
Go/Bubble Tea, MIT, tmux-backed. Hooks for Claude, parsing for others; cost tracking via hooks; remote SSH hosts; MCP and skills managers per session; Docker sandbox; "conductor" supervisor session escalating to Telegram/Slack; web UI. Closest functional peer to tmls (TUI, multi-host), but tmux-hosted, so it inherits the latency stack tmls-rs wants to remove.

### amux - https://github.com/mixpeek/amux
Rust single binary, tmux panes, SQLite board, atomic task claiming, schedules/loops, watchdog self-heal (auto-compact, restart, replay last message), origin-stamped inter-agent messaging. Commons Clause licence.

### tmux plugins - https://github.com/yohasebe/tmux-ccm , https://github.com/LiveNL/tmux-claude-status-tabs , https://github.com/samleeney/tmux-agent-status
All converge on hooks -> state file/tmux option. tmux-ccm documents the best layered design (hooks primary; fallback transcript mtime heartbeat + dialog footer match + process tree) and one safety rule worth copying: `send` refuses to type into a session in PERMIT state.

### sshx / upterm / tmate - https://github.com/ekzhang/sshx
Share-a-terminal tools. sshx (Rust, MIT, 7.7k stars) has E2E crypto, infinite-canvas web UI and Mosh-style predictive echo; cannot be self-hosted. Useful ideas: share link with read-only/live viewer, predictive echo; not a session manager.

## 3. Answers

### (a) Most reliable way to detect Claude Code state in 2026

Answer: **Hooks as the primary event source, with the first-party session registry (`~/.claude/sessions/<pid>.json` + `claude agents --json`) as the state-of-record/reconciler; transcript JSONL only for enrichment; screen parsing only as a last-resort fallback (and for Codex/non-hook agents).**

Evidence:
1. Registry files are live and carry state. On archbox (v2.1.292) `~/.claude/sessions/<pid>.json` contains `pid, sessionId, cwd, startedAt, procStart, version, kind, entrypoint, tmux ("Queue:@0.%0"), name, nameSource, status ("idle"|"busy"), waitingFor, updatedAt, statusUpdatedAt, messagingSocketPath, bridgeSessionId`. Observed `idle` and `busy` values on 3 live sessions. `tmux` field maps session to tmux pane for free. `claude agents --json --all` returned interactive + background sessions together, with `state` (blocked/done/...) for background ones. Docs (agent-view page) define `status` = busy/waiting/idle and `waitingFor` = permission prompt / input needed / sandbox request / dialog open.
2. Pros of the registry: no setup per session, no hook install, atomic JSON, `procStart`+`pid` gives staleness detection (PID reuse), `statusUpdatedAt` gives "waiting since". Cons: schema of `sessions/*.json` is **not documented** (only the `claude agents --json` shape is); fields can change between versions (store `version`; pin tests per version); local filesystem only (hostd must read it on each host; fine since hostd runs there); I did not observe a `waiting` value on my three sessions, so confirm it empirically before relying on it (tmls-rs test: trigger a permission prompt and snapshot the file).
3. Hooks are documented and event-driven, with payloads that screens cannot give (tool name/input on `PermissionRequest`, error class on `StopFailure`, compaction, subagents, cwd changes). Cons: they must be installed in settings (user-level `~/.claude/settings.json` works for all sessions), `Notification:permission_prompt` lags ~6 s and `idle_prompt` 60 s (use `PermissionRequest` + `Stop`, not Notification, for low latency); project settings can override; sessions started before install are blind. Hook type `http` lets hostd receive events on a localhost port without spawning a shell per event (command hooks cost a process fork each; keep them tiny, or use `nc -U`/a static binary).
4. Everyone serious converged on hooks: agent-deck (Claude via hooks), tmux-ccm (hooks primary), tmux-claude-session-manager, status-tabs, cmux (agent hooks), Warp (agent integration). Screen-scrapers (Claude Squad) are the fragile tier; tmux-ccm itself keeps scraping only as fallback.
5. Transcript JSONL (`~/.claude/projects/<proj>/<session>.jsonl`, path given in every hook payload): good for mtime-as-heartbeat ("is it really working?"), last assistant message, token totals, tool-use history. Poor as a state source: it records completed events, not "a dialog is open right now" and can lag. Also huge (this machine: a 2 MB transcript scan offset in one job) so tail it, do not parse whole.
6. Context %: not in hooks or registry; only via statusline JSON (`context_window.used_percentage`), or by summing usage from the transcript tail. tmls-rs needs a statusline shim that writes `{session_id -> pct, rate limits}` to a file hostd watches.

Recommended tmls-rs design (layered, highest priority first): (1) hook events (`UserPromptSubmit`->running, `PermissionRequest`->waiting with reason, `Notification:elicitation_*`->needs-you, `Stop`->idle, `StopFailure`->error with class, `SessionEnd`->gone) pushed to hostd by HTTP/unix socket; (2) registry reconciliation every 1-2 s via inotify on `sessions/*.json` (fills sessions that missed hooks; detects crashed processes via pid+procStart); (3) statusline shim for context % and quota; (4) own-emulator screen heuristics (hostd owns the VT state, so scraping is cheap and exact: look for the dialog footer) as fallback and for Codex/shells; (5) Codex: `notify` hook or `app-server` events, else rollout JSONL. Since hostd owns the PTY it also knows precisely "last output at" and "user typed at", which tmux-based tools only approximate.

### (b) Top 10 features worth adding beyond tmls parity (ranked)

1. **Attention queue / inbox ordered by urgency** (Warp panel, cmux rings, Claude agent view, tmux-ccm). One view: needs-you first, then error, waiting, running, idle; time-in-state from `statusUpdatedAt`; keyboard "jump to next needing me". Highest value per effort, falls straight out of (a).
2. **Approve/deny from the list without attaching, with PermissionRequest detail** (tool name, command, diff) and "allow always" via `permission_suggestions`; plus remote approvals from phone. Replaces `approve.py`; Claude agent view does "peek + reply" similarly.
3. **Safe send** (tmux-ccm): `ag send` refuses or queues when target is in permission/dialog state; queue message until idle (Claude agent view queues replies while working).
4. **Git worktree-per-session with a review surface** (Claude Squad, Conductor, Codex app, Crystal, Superset): new-session form option "isolated worktree", per-session diff stat on the row, merge/PR/cleanup button. Claude Code now fires `WorktreeCreate/Remove` hooks and `claude rm` deletes worktree "if safe": reuse.
5. **Cost / quota / context dashboard** (opcode, agent-deck): per-session tokens and cost, 5 h and 7 d window with reset time and burn-rate forecast, warnings before rate-limit (`StopFailure:rate_limit`, `quota_auto_resume_*` events).
6. **Auto-resume and watchdog** (amux, Claude agent view): restart crashed claude with `--resume`, re-send last message, auto-resume after quota reset, auto-compact at threshold; stale-session detection via pid+procStart.
7. **Notification routing** (agent-deck, amux): desktop + phone push (ntfy/Telegram), per-session mute, quiet hours, "only when I am not looking at that tab" (Warp mark-read-on-focus), escalate if unanswered for N min.
8. **Session timeline/transcript browser and checkpoint restore** (opcode): search across transcripts, jump to last tool call, fork a session from a point (`--resume --fork-session`), compare N attempts of one prompt side by side (Crystal).
9. **Declarative groups/pinning + fork/templates** (agent-deck): group sessions by project, pin, one-click fork with same context, presets with permission mode/model/MCP set per agent.
10. **Share/spectate link** (sshx): read-only live view URL (via the web client) for showing a session to someone, with predictive echo for the remote case. Lower priority; also useful for iPad.

Near misses: kanban board (vibe-kanban died), per-session MCP/skills manager (agent-deck; nice but peripheral), Docker sandbox per session (agent-deck; later), cloud agents/Oz (out of scope), ACP adapter (watch, phase 7+).

### (c) Does anything already do what tmls-rs wants (multi-host, native, fast)?

Honest answer: **no single tool covers it.**
- cmux is the closest in spirit (native GPU terminal, agent notifications, SSH to remote tmux) but macOS-only, Swift, and its remote model is "SSH + tmux", the very stack tmls-rs is removing; GPL/BSL.
- agent-deck is the closest functionally (multi-host, Claude hooks, TUI) but is a Go TUI over tmux with ssh hops: same latency path as today, and it is a TUI inside some terminal, not a native window.
- Claude Code's own agent view/daemon is first-party and good, but local-only, Claude-only, TUI, and no remote dashboard; use it as a data source, not a product. Remote Control gives phone access via Anthropic's relay, single session per instance.
- Warp and Zed are native GPU apps with agent UX but are proprietary editors/terminals tied to their own accounts, and neither gives a multi-host daemon with persistent sessions for arbitrary Claude/Codex processes. Conductor is Mac only.
- amux is the closest to "daemon + web dashboard + multi-agent" (Rust), but tmux-hosted, Commons Clause, and oriented to autonomous task boards rather than a human driving terminals.
- Nothing combines: own PTY daemon + native low-latency client + QUIC multi-host + hook/registry-based status + agent messaging. That gap is the justification for tmls-rs. What is reusable: protocol ideas (Codex app-server JSON-RPC, ACP), detection recipes (tmux-ccm), UX (Warp/cmux attention model).

Risks to note for the spec: Claude Code's registry/job schema is undocumented and moving fast (supervisor daemon is new); version-gate parsers and keep the hooks path as the contract. AGPL tools (Claude Squad, opcode) are idea sources only.

## 4. Sources

- https://github.com/smtg-ai/claude-squad (and raw `session/tmux/tmux.go`)
- https://github.com/stravu/crystal
- https://www.conductor.build
- https://github.com/BloopAI/vibe-kanban
- https://github.com/winfunc/opcode
- https://code.claude.com/docs/en/agent-view , /hooks , /statusline , /remote-control
- https://openai.com/index/introducing-the-codex-app/
- https://docs.warp.dev/agent-platform/capabilities/agent-notifications.md
- https://zed.dev/docs/ai/external-agents.md , https://zed.dev/blog/terminal-threads
- https://www.morphllm.com/cursor-background-agents
- https://github.com/manaflow-ai/cmux
- https://github.com/asheshgoplani/agent-deck
- https://github.com/mixpeek/amux
- https://github.com/yohasebe/tmux-ccm , https://github.com/LiveNL/tmux-claude-status-tabs , https://github.com/samleeney/tmux-agent-status , https://github.com/jongwoo315/tmux-claude-session-manager
- https://github.com/ekzhang/sshx
- https://github.com/andyrewlee/awesome-agent-orchestrators (index of more: dmux, Superset, Emdash, Pragma, diri, Tortie; not individually evaluated)
- Local checks: `~/.claude/sessions/*.json`, `~/.claude/jobs/*/state.json`, `claude agents --json --all` on archbox
