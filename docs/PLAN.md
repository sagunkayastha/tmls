# tmls-rs — plan (draft 3, 2026-10-07)

Working name. A native, GPU-drawn terminal + session system in Rust to replace tmls + tmux.

> **PAUSED 2026-10-09.** The research and code review of 6 similar tools concluded that tmls should keep its
> Python sidebar and let a native terminal draw the session (kitty on the PC, tmux pane host on the laptop).
> Plan: `tmls/docs/design/native-attach-pc.md`. This repo's S0 key-echo harness is reused for those gates.
> Restart only if the native-attach approach fails a gate. S2 split plans (917bfce) are on hold.
>
> **RESUMED 2026-10-10** on the user's decision, alongside the native-attach daily-use trial (not because a gate failed).
> S2 continues from the split plans: Task 0 (workspace + stub crates) is on `main`; Track A starts on `spike/s2-wgpu`.
Status: **draft 3 after Codex (gpt-6-astra) review. Phase 0: S0 done; S1 partially passed (Linux Formatter path); S1b corpus gate closed on Linux (3901/3901 identical), with simulated resize handoff 480/480; S2 (PC) next after review and merge. Windows and Claude recording pending. No product implementation yet.**
History: draft 1 (requirements), draft 2 (research R1–R6), draft 3 (review: `docs/reviews/2026-10-07-codex-astra.md`).

## 1. Why — and what we know vs assume

**Observed:** tmls feels laggy at times and misses keypresses (PC and laptop).
**Measured (2026-10-07):** tmls's Terminal widget, fed keys directly, passed every key (396/396 quiet, 288/288 under a
100 Hz full-screen flood). That test **skipped** the outer terminal, ssh, and Textual's own input parser, so
end-to-end loss is **not yet located**.
**Suspects in tmls (from code, unproven):** the terminal widget redraws on a 30 Hz timer (up to ~33 ms added);
`_read()` parses up to 64 KiB through pyte on the same event loop that handles keys; `key_to_bytes()` returns
nothing for unmapped keys; PTY writes don't handle short writes. tmls keeps **one persistent ssh per tab** (not per key).

Current path when used from the laptop:
```
laptop terminal ─ssh→ tmls (Textual parser → pyte) ─ ssh -t archbox ─ tmux client ─ tmux server ─ claude
```

**Hypothesis:** fewer nested terminal translations + native input + off-loop parsing removes most lag. **S0 tests this
first**, and may also give tmls a quick fix while tmls-rs is built.

**Measured (S0, 2026-10-07/08; scripted PTY input, receiver-observed — details `docs/spikes/S0-results.md`):**
0 missing / 0 duplicate of 10,000 events on every 10k row (local, ssh, tmux, tmls; quiet 50/15 Hz and 500 kB/s
flood; alphabet without Ctrl+P). tmls is the only source of loss/corruption found: Ctrl+P opens Textual's command
palette and swallows every later key (141/200 missing from event 59 on, the `^P` included; dry run, no
palette-disabled control yet); at 15/50 Hz 1249–1250/1250 Alt+letters arrive as `^`+letter (Alt+f as Ctrl+Right
at any rate) and are delayed (one-way p50 45.6 ms at 50 Hz, 140.1 ms at 15 Hz), with the following key delayed too —
*supported hypothesis*: Textual holds the pending ESC sequence and the next key's bytes merge into it (rate and
next-key interval co-vary in these runs, and Ctrl+letter always follows Alt+letter). tmls return-output timing
(ack p50 37.2 ms) is far above tmux's (0.8 ms); under flood only tmls's receiver-side timing degrades (tmls flood
ack percentiles come from a 10–19% subsample of events). On the measured loopback interactive ssh path, OpenSSH
`ObscureKeystrokeTiming` adds a uniform 0–~21 ms (off → one-way p50 0.4 ms); the pcl ssh rows are consistent with
the same spread (cross-host one-way carries clock skew; LAN on/off pair not run). Re-running P1/P2/P5 from a
kitty window on the PC (P6) gave the same picture (0 missing / 0 dup; tmls again 1250 corrupt Alt+letters), and
2000 real keyboard events (K1: `ydotool` uinput → KDE Wayland → kitty → PTY, plain letters only) arrived 2000/2000
with no loss or corruption. Not measured: modifier/Alt keys via a real keyboard, IME, laptop, key-to-photon.

**Hypothesis (revised after S0):** for scripted input, key loss/garbling is tmls's (Textual key parsing and
bindings), not ssh/tmux/PTY; lag comes from tmls (tens of ms for Alt/Ctrl keys — the hold-and-merge mechanism is a
supported hypothesis, not isolated — and worse under output load) plus a 0–~21 ms ssh keystroke-obfuscation spread
(measured on loopback). A client with its own input encoding removes the tmls defects; whether native
input + off-loop parsing removes *most* lag is unresolved until key-to-present (S2) and real-keyboard paths are
measured. Quick tmls fixes: `ENABLE_COMMAND_PALETTE = False`; Alt-key handling (results doc, "tmls defects").

**What tmls-rs will not fix by itself:** OS/compositor stalls, keyboard/IME bugs, app-side input handling, host CPU
starvation, network delay. Transport retransmission delays bytes; it doesn't drop single keys.

## 2. Requirements (user decisions)

| Topic | Decision |
|---|---|
| Priorities | **Best performance and visuals first.** Native desktop first, web second. |
| Clients | Linux PC (CachyOS, KDE Wayland, RTX 5070 Ti) + Windows laptop: **native executable with its own GPU window** (not platform widgets). Later: browser/iPad, then Android. |
| Session hosts | archbox, PC (laanta-chy). Windows laptop later (keep possible). |
| Migration | **Side by side with tmux**: tmux sessions listed + attachable; tmls stays until parity. |
| Scope | **Full parity with tmls** (§8) + the user's "want" picks (R6). Tracked as a release program, phase by phase. |
| Auth | Tailscale (personal tailnet) + per-device key; owner-only local sockets. |
| Scale | **Personal tool, one user.** Deliberately out of scope for now: device enrollment UI, code-signed installer, crash-report service, screen-reader support, exactly-once input across every crash mode. |

## 3. Decisions (research R1–R6, revised by review)

| Question | Decision | Status |
|---|---|---|
| Build or reuse? | Build session worker, hostd, desktop client; reuse libraries. **But first measure simpler options** (S0, S4): direct ssh+tmux in a native terminal, native client over a plain persistent connection. | Provisional until S0/S4 |
| Object model | **Session = one PTY** (one program). Tabs/splits/layouts live in the **client** (per device). Stable `session_id` + `incarnation`; name is a mutable label. tmux sessions appear as bridged sessions. | Decided |
| Who owns terminal state | **Session worker** (one process per session) owns PTY, the **authoritative emulator**, scrollback, replay log, input dedup. hostd = discovery, auth, routing, status, files; restartable without touching sessions. | Decided (replaces "holder + emulator in hostd") |
| Emulator core | **libghostty-vt** (crate 0.2.2, MIT/Apache; Zig 0.15.2 build — not 0.16, S1), behind an `emu` crate. **Provisional**: snapshot format has no cross-version guarantee; pinned build + S1 required. `alacritty_terminal` is a **separate design branch** (no snapshot/encoders/graphics), not a drop-in. | Provisional. S1 and S1b measured on Linux: worker carry reaches 3901/3901 identical expanded-corpus splits under the S1 comparison; simulated resized handoff reaches 480/480, while production attach remains provisional; see docs/spikes/S1-results.md and docs/spikes/S1b-results.md. |
| Desktop GUI | `winit` 0.30 + `wgpu` 30 + own renderer/UI layer vs `gpui` vs custom grid inside an existing toolkit — **all kept open until S2**, judged on latency **and** the cost of forms, IME, selection, menus. Text: `cosmic-text` + `swash`, own atlas. | Gate S2 |
| Transport | Compare **QUIC (`quinn`)** vs **persistent TLS/TCP** in S4 on direct and DERP-relayed Tailscale paths. WebSocket for web. **0-RTT off** for input and mutations. | Gate S4 |
| Typing prediction | Not in v1; revisit after measuring real RTTs (S4), esp. phone/cellular. | Deferred |
| tmux bridge | v1: `tmux attach` inside a session worker (shows tmux's rendered stream, no per-pane history — documented limit). Later: control mode (`tmux -C`, `refresh-client -A '%pane:continue'`, decode `capture-pane -peC` octal escapes). | Decided |
| Claude status | Hooks (`http` → hostd) + `~/.claude/sessions/<pid>.json` + `claude agents --json`; context % via status-line shim; explicit **unknown/stale** state; unseen-completion tracked by hostd per client. | Gate S5 |
| Approvals | Own design (§5.5), not "status + type y". | Gate S5 |
| Web (phase 5) | xterm.js 6 + WebGL; ghostty-web runner-up. Needs a **portable attach format** (§5.2) because xterm.js can't load a Ghostty snapshot. Android keeps tmls's native input view. | Decided (format in §5.2) |
| Licences | Our code MIT/Apache; ideas only from GPL/AGPL projects (Zed terminal, COSMIC Term, Warp, Claude Squad, opcode, cmux). | Decided |

## 4. Architecture

```
┌─ desktop (Rust, GPU) ───────────────┐            ┌─ archbox ─────────────────────────────────────┐
│ sidebar · tabs/splits (per device)  │            │ hostd (systemd --user): auth · routing ·      │
│ grid renderer · forms · viewer      │◄─ QUIC or ─►│   status/hooks · files · discovery · tmux     │
│ local emulator per tab              │  TLS/TCP   │                 │ owner-only unix sockets      │
└─────────────────────────────────────┘ (S4 picks) │  ┌──────────────┴───────────────┐             │
┌─ web (phase 5: TS + xterm.js) ──────┐            │  session worker (1 per PTY, own cgroup)       │
│ browser · iPad · Android WebView    │◄── WSS ───►│  PTY · authoritative emulator · scrollback ·   │
└─────────────────────────────────────┘            │  replay log · input dedup · reply authority    │
                                                    └───────────────────────────────────────────────┘
```

| Crate | Job |
|---|---|
| `proto` | Versioned frames: input(seq), output(offset), resize(epoch), snapshot boundary, session list/status, requests with ids. |
| `emu` | Emulator wrapper (libghostty-vt), snapshot + portable VT reconstruction. Only crate touching the emulator. |
| `worker` | Per-session process: PTY, emulator on its own thread, scrollback, replay log, input dedup, reply authority, IPC to hostd. |
| `hostd` | Supervises/adopts workers, network server, auth, status (hooks), files, tmux bridge, `ag` inbox. |
| `client-core` | Connections, reconnect, session list, per-tab emulator, controller lease. No UI. |
| `desktop` | Window, renderer, UI layer, features. |
| `cli` | `ag`, `ct`, `tmls-rs attach` (via portable format), `tmls-rs doctor`. |

## 5. Contracts (must hold before phase 1 code is frozen)

### 5.1 Session invariants
- One canonical **size** and **capability profile** per session (`TERM`, terminfo, Unicode width policy, keyboard
  protocol, graphics). Clients never change those per-viewer.
- **One reply authority:** only the worker's emulator answers terminal queries (DSR, cursor position, keyboard/graphics
  queries). Client emulators never write to the PTY.
- Output carries a **byte offset**; resizes carry an **epoch** ordered in the same stream.
- **Controller lease:** one client controls size (last to type/resize takes it, with takeover + expiry); others
  letterbox/pan. Input from any attached client is allowed, tagged by writer.

### 5.2 Attach / reconnect
- Attach = snapshot taken **at offset S**, then output from S. **Snapshot = Formatter VT reconstruction** (libghostty's
  `Format::Vt` escape sequences rebuilding screen, modes, cursor, scrollback; S1 measured its gaps). It is taken at a
  **VT sequence boundary with tail carry** (bytes after the boundary go out as output from S), at the **session's
  size**: the client restores at that size and applies resizes after (resize epoch).
- **Worker state carry (measured in S1b on Linux; corpus gate closed):** beside Formatter VT, the worker
  carries title, pwd, kitty keyboard push stack, pending wrap, DECSC saved cursor + pen, hyperlinks, and a full
  primary-screen snapshot taken at alt-screen entry. ECH overlays restore erased-cell identity/background in the
  **viewport only**. History rows retain the Formatter's empty-cell/background loss; a taller resize can bring them
  into view. The carry path probes unwrapped Formatter VT, then uses per-row unwrapping on flagged rows when flags
  differ; wide-spacer rows retain wrapped emission. The unchanged S1 comparison is identical on 3901/3901 splits;
  row flags agree on 3883/3901 as a separate diagnostic. The prior path used two or three temporary restores;
  this implementation uses three on the primary attempt and up to five when it retries, including at each
  alternate-screen entry inside `Tracker::feed`. Matrix snapshot-plus-restore CPU time rose from
  3.75 s in the earlier round-2 run to 4.50 s after the prior fallback and 5.74 s in this final run. That cost
  matters to the requirement to encode without holding the emulator lock. See docs/spikes/S1b-results.md.
- **Handoff:** snapshot bytes + output offset S + resize epoch + replay subscription from S are one atomic unit;
  output events are retained until delivered; the worker encodes without holding the emulator lock (and never sends
  under it).
- **Client restore** runs with PTY replies and side effects disabled (the worker is the sole reply authority; bell,
  OSC 52, notifications in the snapshot are not re-fired).
- **S1b handoff evidence (simulation, not transport proof):** 480/480 plain-tail and 480/480 resized
  continuations are identical under the S1 comparison, with zero client PTY reply attempts after filtering the
  measured query forms. The former three `wrap_pending` resize text mismatches are gone. Inert side-effect handlers do not
  establish phase-aware suppression, and a real atomic replay subscription has not been tested. Production
  attach/reconnect remains provisional (docs/spikes/S1b-results.md).
- **Portable attach (web / CLI / other versions)** uses the same VT reconstruction; measured only at **text-level
  fidelity** (right-trimmed visible text of end-of-stream snapshots in pyte) — no colour/attribute, history or
  continuation guarantee until measured further. Graphics and some parser state are documented limits.
- **"Native snapshot"** (whole-terminal serialize/restore) does not exist at the pinned ghostty (no `snapshot.h`); it is
  a follow-up needing a newer ghostty + own bindings.
- Reconnect: client sends last **committed** offset; worker replays from there if still retained, else sends a fresh
  snapshot with an explicit **stream reset**.
- **Side effects** (bell, clipboard OSC 52, notifications) are delivered only live, never on replay.

### 5.3 Input
- Each client has a writer id; input frames have **sequence numbers**; the worker dedups and acks after the PTY write
  (handling short writes). Ack = written to PTY, not "processed by the app".
- On disconnect the client **shows** unsent input as pending and **never resends silently** after reconnect; the user
  confirms or drops it.
- Requests (create/rename/kill/send/approve) carry request ids; retries are idempotent by id.

### 5.4 Recovery and overload
| Event | Survives |
|---|---|
| Client crash / network drop | Everything; reattach via §5.2. |
| hostd crash/restart/upgrade | Sessions, screen, scrollback (worker holds them); hostd re-adopts via registry + fencing (session_id, incarnation, owner check). |
| Worker crash | The program dies with its PTY (documented). |
| Logout / reboot | Nothing, unless `loginctl enable-linger` (logout); reboot ends sessions (document; maybe "restart in folder" later). |

Limits (configurable): scrollback lines, replay log bytes per session, per-client send queue. **Slow client** → its
queue fills → it gets a stream reset + snapshot; it never blocks the worker, the controller, or other sessions.
Rendered frames may be dropped; terminal bytes to the authoritative emulator never are. Parsing runs on a dedicated
thread per worker.

### 5.5 Approvals
Request identity from the hook event; expiry; first decision wins (others see "already answered"); late/expired
answers rejected; hostd down → Claude's own prompt still works in the terminal. Network-permission prompts arrive via
`Notification`, not `PermissionRequest` — handled as "needs you" without one-click approve.

### 5.6 Compatibility and security
- Versioned boundaries: worker IPC, network proto, snapshot format. hostd supports older workers (they keep running
  after upgrades); incompatible snapshot → portable reconstruction.
- Auth: per-device key pinned on hostd + Tailscale identity check; owner-only unix sockets with peer-cred check;
  hooks authenticated with a per-host token; web: token login + Origin check.
- Terminal-output trust: OSC 52 clipboard writes need a setting (default: allow from own sessions); links/paths never
  auto-open; image decode size limits.

## 6. Phase 0 — spikes (feasibility, then re-estimate)

| Spike | Question | Pass gate |
|---|---|---|
| **S0 Key loss** (first) | Where do keys go missing or late today? Numbered key stream (letters, Esc/Alt, modifiers, repeats, paste) into a receiver that logs arrivals with timestamps, through: native terminal→local; native terminal→ssh; ssh+tmux; local tmls; tmls over ssh (laptop). Quiet and under a defined output flood. | Each path: missing/duplicate count and p50/p95/p99 delivery time, 10,000 events per path. Cause localised; quick tmls fix identified if one exists. **Status: measured 2026-10-07 (scripted PTY paths + kitty-hosted + real-keyboard plain letters on the PC; see docs/spikes/S0-results.md) — gate closed for key loss: none found; presentation latency and modifier keys via real keyboard remain for S2.** |
| S1 Emulator | libghostty-vt pinned build on Linux + Windows MSVC; recorded Claude sessions; snapshot/restore at adversarial byte boundaries = uninterrupted run; portable VT reconstruction quality. | Identical results; differences explained. **Status: partially passed (Linux, Formatter path) — see docs/spikes/S1-results.md; Windows + Claude recording pending.** Identical under the S1 comparison on 78.6% of Aligned cases (81.5% of distinct snapshot points); each difference assigned a proposed cause; worker state carry → S1b. |
| **S1b Snapshot repair** | Worker state carry + handoff per §5.2; continuation tests pass for the 5 carried items (primary at alt entry, hyperlinks, DECSC, kitty stack, pending wrap); nested/prompt-first recordings identical under AlignedCarry. | Identical under the S1 comparison on all corpus streams at sequence boundaries, or each residual explained as a Formatter limit with an upstream issue filed. Gates freezing §5.2 attach/reconnect. **Status: corpus gate closed (2026-10-08, Linux) — pending full Claude review of `2178799..258cec1` (numbers re-verified independently at `258cec1`).** AlignedCarry 3901/3901; simulated handoff 480/480 plain and 480/480 resized. Row flags match 3883/3901; 18 diagnostic discrepancies remain as named residuals (6 `less`, 8 `less_prompt`, 4 `utf8_wide_combining`; outside the S1 comparison, relevant if attach reflows on resize). Production §5.2 remains provisional pending transport/atomic subscription and side-effect validation. See docs/spikes/S1b-results.md. |
| S2 Interaction slice | Grid + sidebar + one editable form + selection + clipboard + IME (fcitx5, Windows TSF), in raw wgpu vs gpui; 60 Hz + high refresh, fractional scaling, monitor move. | p50/p95/p99 key-to-present (and camera-checked photon sample), CPU, idle power, text quality; pick on latency **and** coverage. |
| S3 Worker survival | Kill hostd abruptly 100× incl. during create/adopt, long downtime under flood. | Programs survive; promised state restorable; no duplicate replies/input; memory within limits. |
| S4 Transport | QUIC vs persistent TLS/TCP; direct vs DERP; loss, low bandwidth, Wi-Fi drop, sleep, address change. | Reconnect p95 < 1 s from restored connectivity; replay + input dedup correct under flood. |
| S5 Claude status/approvals | Every mark incl. unseen-completion and stale; permission/tool/network/question dialogs; hook loss; hostd outage; two devices answering. | No wrong-session or expired approvals; fallback states documented. |
| S6 Mixed clients | Two clients different sizes; reconnect during resize; old+new binaries; minimal CLI attach. | One canonical state, one reply per query, clear errors on incompatibility. |
| S7 ConPTY (small) | Worker on Windows keeps ConPTY alive across hostd restart. | Proven or design adjusted before worker IPC is frozen. |

Time: S0 ≈ 1 day; the rest are feasibility prototypes. **Schedule is re-estimated after the gates.**

## 7. Phases (each ends with the user using it for real)

| Phase | Delivers | Done when |
|---|---|---|
| 0 | Spikes S0–S7, final spec, implementation plan | User approves |
| 1a Headless core | proto, emu, worker, hostd (one host), headless test client, auth, recovery, logs/metrics | S3/S6-style tests pass in CI |
| 1b Desktop slice | Desktop app: both hosts, session list, tabs, typing, resize, reconnect, basic copy/paste, scrollback, keyboard + IME, tmux sessions attachable | Benchmark contract (§9) met; user uses it daily alongside tmls |
| 2 Daily driver | Status marks + context %, attention queue, new/rename/kill, splits (client-side), scrollback search, mouse, links, quick select, notifications (easy off/on, sound, suppress focused), font zoom, session colours, prompt history | User works a full day without tmls |
| 3 Agents | `ag`, safe send/queue, approvals (§5.5), `ct`, presets, saved prompts, auto-name | Agent workflows unchanged |
| 4 Extras | File viewer (+ images), image paste/drop (upload protocol with size limits), url-screenshot, Sketch | Matches tmls |
| 5 Web | Browser/iPad (xterm.js, portable attach), PWA | iPad replaces tmls web |
| 6 Android | WebView + native input | Phone replaces tmls app |
| 7 Laptop host | Worker + hostd on Windows (ConPTY) | Laptop sessions listed/attached |

Cross-cutting from phase 1a: deterministic terminal corpus tests, protocol state-machine tests, random chunk
boundaries, frame/snapshot fuzzing, soak test; structured logs with correlation ids (no terminal contents by
default); config file (hosts, fonts, keys, presets) with schema version; reproducible builds (pinned Rust + Zig).

## 8. Parity checklist (tmls 2026-10-07) + picks

tmls features: sessions per host + offline marking; background tmux servers folded; kitty (non-tmux) Claude sessions
discovery/focus; status marks ● ? ◆ ✕ ○ with hover reason, Claude name + context %; tabs + Alt+Shift+arrows + list
keys; Open in new terminal / Copy attach command; mouse (wheel scrollback, drag copy, shift-drag, ctrl-drag, Ctrl+C
copy-or-interrupt, trailing-space trim); paste (system, bracketed, image → host file path); links (URL with localhost
remap, `file:line`, bare paths → viewer); quick select; new-session form (host, folder, git-derived name,
shell/claude/presets); rename (renames Claude too); kill with confirm; windows/splits; `ag`, `ct`; notifications;
approvals; prompts; file viewer (+ image pane); web (login, background toggle, reload/version, Sketch, Send-to);
Android (native input, copy/paste, keyboard control, links, camera in Sketch); config migration from tmls.

User's "want" picks (R6): agent-orchestration-api, agent-presets, auto-name, clickable-links, context-meter,
conversation-title, copy-options, create-session, desktop-notify, font-zoom, image-paste-drop, keyboard-sidebar,
kill-session, needs-input-detection, notification-inbox, notify-sound, notify-suppress-focused, prompt-history,
queue-message, quick-select, remote-approve, rename-session, saved-prompts, scroll-preserve, session-colors-icons,
smart-ctrl-c-paste, status-states-richer, tab-status-badge, tmux-wrap-local, url-screenshot, window-pane-actions.
Items in this plan that R6 marked "skip" (e.g. host-daemon) are **implementation necessities**, not features.

## 9. Benchmark contract (replaces "success measures")

Fixed setup per run: hardware (PC: RTX 5070 Ti, KDE Wayland, monitor refresh; laptop), build profile, workload
(quiet; Claude streaming; defined bytes/s flood), session/client count, history size, network path (LAN direct,
Tailscale direct, DERP). Report: input correctness (missing/duplicate), key-to-present p50/p95/p99, CPU, RSS, idle
power, reconnect/recovery time. Baselines measured the same way: tmls, and native terminal + ssh + tmux.
Target: **zero unexplained missing/duplicate keys** and a material latency improvement over tmls; beating plain
ssh+tmux is an optimisation goal, not a precondition.

## 10. Risks

- libghostty-vt pre-1.0, community wrapper, Zig toolchain, no snapshot compatibility guarantee → pinned, S1, portable reconstruction path.
- libghostty-vt build: needs **Zig 0.15.2** exactly (upstream README says 0.16, which fails) → pin it; provision the
  ghostty source checkout + zig package deps so builds are reproducible offline (otherwise `build.rs` clones).
- `Options::max_scrollback` is **bytes**, not lines → size the scrollback limit in bytes.
- ghostty a887df42 has **no `snapshot.h`** → Formatter VT reconstruction gaps govern attach fidelity (S1, S1b).
- Commit `57151cb` (spike/s1) does not build on its own (intermediate) → bisect from `ddf25e0`.
- Building a full desktop terminal UI (text, IME, selection, forms) is the largest cost → S2 measures coverage, not just a grid.
- Wayland + NVIDIA frame pacing → S2 on the real PC.
- Claude Code internals undocumented → hooks primary, version-gated parsers, unknown/stale state.
- Scope → strict phases; tmls stays until parity.

## 11. Research corrections (from review)

R1 "swap emulator in a day" — overstated (Alacritty lacks snapshot/encoders). R2 "screen diffs inherently lose
scrollback" — wrong in general. R2 "QUIC stops floods stalling typing; TCP fallback free" — overstated. R2/§3
Tailscale RTT assumed low — must be measured (direct vs DERP). R2 tmux `%continue` — it's
`refresh-client -A '%pane:continue'`; `capture-pane -C` output needs octal decoding. R3 "toolkits cost latency / Tauri
fails latency" — unmeasured. R1 renderer "600–1000 lines" — prototype estimate only. R5 "emulator mismatch only
cosmetic" — wrong (widths/modes change state). R5 hterm "archived" — mirror only. R4 sshx "cannot self-host" —
unsupported, not impossible. zmx — prior art, has documented upgrade/restore limits. Research docs keep their text;
this section and the plan supersede them.

## 12. Who does what

To be set by the user (pending).
