## Verdict

**Do not approve draft 2 for implementation; approve a revised measurement and architecture spike phase.** Removing Textual/pyte from the interactive path is a plausible improvement, but the evidence does not establish the cause of missed keys, and the proposed persistence/replay design cannot yet deliver its stated guarantees. The largest risks are terminal-state consistency, recovery during output floods, and the cost of building a complete desktop terminal—not Rust or QUIC selection.

## Critical issues — must fix before implementation

1. **The root-cause conclusion exceeds the evidence.**  
   **References:** [PLAN §1](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:7); R2 §3(c); predecessor [term.py](/home/sgkayast/Others/tmls/src/tmls/term.py:181), [hosts.py::attach_argv](/home/sgkayast/Others/tmls/src/tmls/hosts.py:209).

   The predecessor starts one persistent SSH process per attached remote tab. “ssh-per-key” suggests connection setup on every key, which the implementation does not do. Two SSH hops occur only when the user also reaches tmls through SSH.

   There are concrete latency candidates: the widget refreshes at **30 Hz**, adding up to roughly 33 ms of refresh scheduling delay; `_read()` synchronously parses up to 64 KiB through pyte on Textual’s event loop; input writes also run there. `key_to_bytes()` returns empty bytes for unsupported events without a character, and writes do not check for short completion.

   None proves the reported loss. The 396/288-key results need a linked harness, injection point, exact workload, and receiving endpoint. Injecting Textual events would bypass outer-terminal encoding, SSH delivery, and Textual’s input parser.

   Measure these paths separately: native terminal→local receiver; native terminal→SSH→receiver; SSH+tmux; local tmls; tmls reached through SSH. Record input at the receiver, separately from visible echo. Include Esc/Alt ambiguity, modified keys, repeats, paste, focus changes, and composition.

   Native input removes the outer terminal/Textual parsing path for desktop users. It does **not** fix OS shortcuts, keyboard/IME bugs, application input handling, host CPU starvation, network delay, or compositor stalls. Reliable transport retransmission normally delays bytes rather than silently dropping individual keys.

2. **A surviving holder does not imply recoverable terminal state.**  
   **References:** [PLAN §§3–4](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:38); [R2 §3(b)](/home/sgkayast/Others/tmls-rs/docs/research/R2-multiplexers.md:89).

   Suppose hostd crashes, the holder continues draining output, and its bounded ring wraps. Restarted hostd has neither the old emulator nor the beginning of the stream. The remaining suffix cannot reconstruct modes, cursor, alternate screen, scrollback, or graphics.

   R2 mentions “a periodic holder-side snapshot” or putting an emulator in the holder; PLAN commits to neither. A checkpoint alone is insufficient if the ring later discards bytes following it.

   Choose explicitly:

   - Put the authoritative emulator in each holder/session worker.
   - Preserve a checkpoint plus every subsequent state-changing event, with a defined storage/backpressure limit.
   - Weaken the guarantee to process survival with potentially incomplete display recovery.

   My preference is **one persistent session worker owning PTY, authoritative emulator, and recovery state**, with hostd handling discovery/auth/routing. That enlarges the holder, but makes the recovery guarantee coherent.

3. **Snapshot plus raw bytes needs a consistency protocol, not just sequence numbers.**  
   **References:** [PLAN §3 “Wire,” §4 `proto`](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:39); R1 §4(a).

   Specify a session incarnation, output byte offset, ordered resize events, snapshot boundary, and subscription handoff. A client must restore state at boundary S, then apply exactly the subsequent events—without a gap, overlap, or resize at the wrong point.

   Snapshots must preserve unfinished UTF-8/escape sequences and hidden terminal state, not merely equal-looking cells. Ghostty’s API requires continuation tracking for unfinished parser state and prohibits concurrent terminal mutation during encoding. Do not hold the emulator lock while streaming a snapshot to a slow network client. [Ghostty snapshot API](https://raw.githubusercontent.com/ghostty-org/ghostty/main/include/ghostty/vt/snapshot.h)

   Define what “last seq seen” means: received, parsed, or committed to the retained emulator. A restarted client with no emulator cannot resume using an offset alone.

4. **There is no authority model for terminal replies, capabilities, or sizing.**  
   **References:** [PLAN §4](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:47); [R5 §3(a)](/home/sgkayast/Others/tmls-rs/docs/research/R5-web.md:79).

   Feeding identical output into daemon and client emulators can generate duplicate device-status, cursor-position, keyboard-protocol, and graphics replies. Historical replay can also repeat clipboard writes, bells, or notifications.

   Designate exactly one authority for replies to the PTY. Separate terminal-state reconstruction from external side effects. Define a stable session capability profile, including `TERM`, terminfo, Unicode width policy, keyboard protocols, and graphics.

   One PTY has one size. Desktop and phone cannot independently resize its live terminal state. Start with an explicit controller lease: the controller supplies canonical dimensions; viewers pan/letterbox that grid. Specify takeover, disconnect, lease expiry, simultaneous input, and ordering between resize, emulator mutation, and PTY output.

   R5’s suggestion that hostd answer keyboard capability queries “per client” is incompatible with a single shared application input stream unless an explicit translation model is added.

5. **Input delivery across reconnects is unspecified.**  
   **References:** [PLAN §4 `proto`/`client-core`, §8](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:65); R2 §3(c).

   Output replay does not prevent lost or duplicated input. If a connection dies after the holder writes Enter but before its acknowledgement arrives, blind retry may execute another command; refusing retry may lose input.

   Define writer identity, input sequence numbers, partial-write tracking, and acknowledgement semantics at the holder. Preserve deduplication across **hostd** restart. State the limit: PTY delivery is not proof that an application processed the input, and exactly-once execution cannot be promised across arbitrary holder failure.

   Give create/kill/rename/send/approve requests separate request IDs and retry rules. Define whether disconnected typing is rejected or visibly queued; do not silently send stale input after reconnection.

6. **The flood-control proposal conflicts with byte-stream fidelity.**  
   **References:** [PLAN §3 “Wire”](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:39); [R2 §3(c)](/home/sgkayast/Others/tmls-rs/docs/research/R2-multiplexers.md:96).

   Coalescing writes preserves bytes but does not remove backlog. Dropping rendered frames is safe; dropping arbitrary terminal bytes is not. Skipping to a snapshot needs an explicit stream reset and a policy for omitted history and side effects.

   Under sustained overload, you cannot simultaneously guarantee bounded memory, every byte retained, an unblocked producer, and arbitrarily slow clients. Choose the tradeoff.

   Define limits for PTY queues, replay rings, snapshots, scrollback, graphics, and each client. Slow viewers must not block the controller or other sessions. Isolate bulk file/history traffic. Schedule parsing fairly so a flood cannot monopolize the executor.

   QUIC helps with loss-related head-of-line blocking between streams, but streams still share connection resources and congestion control. Input transmission and visible echo latency are separate measurements.

7. **The browser and fallback CLI cannot consume the proposed native snapshot directly.**  
   **References:** [PLAN §§3–4, phase 5](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:44); [R5 §3(b)](/home/sgkayast/Others/tmls-rs/docs/research/R5-web.md:90).

   xterm.js cannot restore a Ghostty binary snapshot. Neither can an arbitrary external terminal receiving `tmls-rs attach`. ANSI reconstruction is another implementation, with limitations around parser state, modes, graphics, history, and terminal-specific behavior.

   Resolve this before freezing `proto`, although the browser UI can wait. Options include a shared emulator, a defined portable state representation, or explicitly limited VT reconstruction. R5’s assertion that cross-emulator divergence causes only cosmetic differences is incorrect: differing character widths and mode handling can change subsequent cursor placement and input interpretation.

8. **Authentication is a requirement label, not a design.**  
   **References:** [PLAN §§2–4](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:29); R2 §3(c); R6 “Skipped.”

   Define server authentication, device enrollment/revocation, key storage, and authorization to read sessions, type, kill, approve, and access files. Tailscale reachability does not itself assign those application permissions.

   Include owner-only holder sockets and peer validation, authenticated hook ingestion, and browser authentication/Origin validation. A browser cannot simply reuse an unspecified native per-device-key mechanism.

   Disable QUIC 0-RTT for mutations and terminal input. Quinn explicitly warns that early data is replayable; it also warns against sending client-authenticated secrets before client authentication completes. [Quinn early-data security](https://docs.rs/quinn/latest/quinn/struct.Connecting.html#security)

## Important issues

1. **Holder adoption and service lifetime need a state machine.**  
   **References:** [PLAN §§3–5](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:38); R2 §3(b).

   Cover a hostd crash during creation, holder creation before registry publication, duplicate hostd instances, stale sockets, child exit while hostd is absent, and two controllers attempting adoption. Use stable session IDs, incarnation IDs, ownership checks, and fencing.

   Separate systemd units are materially different from setting `KillMode=process` or `Delegate=`. Specify actual unit relationships and logout/linger behavior. Hostd restart survival does not imply survival of user-manager shutdown, reboot, holder OOM, or administrator termination.

2. **Upgrade compatibility belongs in the initial protocol.**  
   **References:** [PLAN §4 `proto`/`holder`, §9](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:65); R1 §2.4.

   Long-lived holders will run older binaries than hostd and clients. Version all three boundaries: holder IPC, network protocol, and snapshot/emulator format. Specify supported version skew, capability negotiation, incompatible attach behavior, and rollback.

   Ghostty’s current snapshot format explicitly lacks a binary compatibility guarantee—even while called version 1. Pinning dependencies does not solve interoperability between separately upgraded processes. [Ghostty snapshot format](https://raw.githubusercontent.com/ghostty-org/ghostty/main/src/terminal/snapshot/main.zig)

3. **libghostty-vt is a conditional choice; alacritty_terminal is not a cheap interchangeable fallback.**  
   **References:** [PLAN §3, S1](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:36); [R1 §4(a)](/home/sgkayast/Others/tmls-rs/docs/research/R1-terminals.md:115).

   The fallback lacks the very snapshot, encoding, and graphics facilities used to justify the architecture. An `Emu` trait localizes API usage; it does not remove semantic differences or manufacture missing functionality.

   Keep Ghostty provisional pending pinned-build and recovery tests. Treat Alacritty as a separate design branch with explicit feature reductions and snapshot work. Differential testing is useful, but Alacritty is not an oracle for features it does not implement.

   Also define CPU scheduling explicitly: one asynchronous task per emulator does not automatically isolate synchronous parsing from the runtime or pin an object to one thread.

4. **The GUI comparison underestimates the actual product.**  
   **References:** [PLAN §3, S2, phase 2](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:37); R3 §§1, 3.1, 5.

   A grid and sidebar benchmark excludes the expensive parts: editable fields, IME composition, selection, focus traversal, menus, accessibility, drag/drop, font fallback, search, and multi-monitor DPI changes.

   “Within one frame” is too loose without a refresh rate: one 60 Hz frame is 16.7 ms. Compare p50/p95/p99 input-to-photon latency, CPU, idle power, text quality, and behavior under flood. `present()` timing is not photon timing.

   Keep winit+wgpu and gpui open until this test. A custom terminal renderer inside an existing UI framework is a serious third option. Framework overhead should be measured, not inferred from the existence of a framework.

5. **Windows ConPTY is more than a named-pipe substitution.**  
   **References:** [PLAN phase 7](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:94); R2 §3(b).

   The session worker must retain the pseudoconsole and appropriate pipe handles independently of hostd. Address synchronous pipe servicing, shutdown ordering, handle inheritance, job-object lifetime, Ctrl+C behavior, and Windows login/session boundaries. Microsoft documents deadlock risks and termination of attached applications when the pseudoconsole closes. [Microsoft ConPTY lifecycle](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)

   Keep full Windows hosting late, but run a small ownership/restart feasibility test before freezing holder IPC. Building the emulator on Windows does not validate ConPTY.

6. **Claude status and approvals need separate designs.**  
   **References:** [PLAN §3 “Claude status,” S5, phases 2–3](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:43); R4 §§2, 3(a–b).

   A status event is not a durable approval handle. Define request identity, expiration, cancellation, competing local/remote decisions, hook timeout, hostd outage, and late responses.

   `PermissionRequest` does not cover sandbox network permission prompts; current documentation directs those to notifications. The hook payload also lacks `tool_use_id`. “Replaces approve.py cleanly” is therefore too strong. [Claude hook contract](https://code.claude.com/docs/en/hooks#permissionrequest)

   Preserve an **unknown/stale** status. “Finished but unseen” also requires attention/read state beyond Claude’s process status. Status updates alone cannot make typing into arbitrary TUIs race-free; safe send should prefer structured messaging or use a clearly limited fallback.

7. **Session/window/pane semantics are missing from the object model.**  
   **References:** [PLAN §§4, 6–7](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:61); R2 §3(d).

   Is a “session” one PTY, a tmux-like collection of panes, or a client workspace? Where do layouts live? Can two clients arrange the same PTYs differently? What survives rename and restart? This affects holder count, IDs, status aggregation, resize, `ct`, and `ag`.

   Define it before implementation; defer layout UI afterward. The tmux PTY bridge also exposes tmux’s rendered display stream, not independent pane history. Document that limitation and keep the bridge as a compatibility path until control mode reaches parity.

8. **Simpler alternatives were dismissed without comparative evidence.**  
   **References:** [PLAN §3 “Build or reuse?”](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:35); R1 §4(c); R2 §3(a,c); R4 §3(c).

   Benchmark these:

   - A session dashboard that launches/focuses existing native terminals running direct SSH+tmux. The predecessor already offers “Open.”
   - The new native client over persistent SSH or TLS/TCP before adding QUIC. This isolates GUI/input improvements from transport changes.
   - Existing per-session persistence, such as zmx, behind a small status/discovery service while evaluating replacement costs.
   - WezTerm configuration or a narrow integration prototype before assuming every requirement needs a fork.

   These may fail the final integrated-UI goal, but they establish the value of each custom subsystem. “No product combines our selected implementation choices” is not evidence that all those choices are necessary.

## Minor/nits

- **Correct the emulator count.** [PLAN §1](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:16) promises “one emulator per screen,” while §4 runs daemon and client emulators. The defensible claim is fewer nested terminal translations on the interactive path.
- **Use measurable language.** [PLAN §8](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:113): replace “instantly,” “no frame drops,” and unconditional superiority to plain SSH with workload-specific budgets. Keep beating SSH+tmux as an optimization objective.
- **Clarify “native.”** [PLAN §2](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:20): a native executable/window, platform-native widgets, and desktop integration are different requirements.
- **Reconcile contradictory decisions.** PLAN phase 0 supersedes R1’s “first in Phase 1” spike; PLAN requires holders in phase 1 while R2 permits postponement; R1 calls beamterm unverifiable while R5 identifies it. Mark research recommendations superseded rather than leaving competing instructions.
- **Reconcile feature picks.** [PLAN §7](/home/sgkayast/Others/tmls-rs/docs/PLAN.md:96) versus [R6 “Skipped”](/home/sgkayast/Others/tmls-rs/docs/research/R6-old-picks.md:81): several planned features were skipped in the survey. Distinguish parity requirements, new wants, implementation necessities, and deliberate changes of scope.

## Doubtful research claims

| Reference | Claim and correction |
|---|---|
| **R1 §4(a): snapshot format and “swap is a day of work”** | **Overstated.** Snapshot compatibility is not guaranteed, and switching to Alacritty requires architectural work. Upstream confirms the format limitation. [Ghostty format](https://raw.githubusercontent.com/ghostty-org/ghostty/main/src/terminal/snapshot/main.zig) |
| **R2 §3(c): screen diffs inherently lose scrollback/graphics** | **Incorrect as a general claim.** A state protocol can carry history and image resources. Mosh’s feature limits are not inherent to state synchronization. Raw bytes also fail to recover expired history without retained state. |
| **R2 §3(c): QUIC prevents a flood from stalling other typing; TCP fallback is “free”** | **Overstated.** Congestion, flow-control limits, scheduler contention, and shared application queues remain. A fallback adds reconnect, authentication, framing, and testing paths. Tailscale DERP also introduces an underlying TCP relay path. [Tailscale DERP architecture](https://github.com/tailscale/tailscale/blob/main/derp/README.md) |
| **PLAN §3 / R2 §3(c): Tailscale RTT is inherently low enough to dismiss latency concerns** | **Unsupported for the actual deployment.** Tailscale may use direct or relayed connectivity. Measure both; deferring prediction is reasonable without asserting a latency range. [Tailscale connection types](https://tailscale.com/docs/reference/connection-types) |
| **R2 §3(d): resume tmux with `%continue`; `capture-pane -peC` as restoration** | **Partly wrong.** `%continue` is a notification; the command is `refresh-client -A '%pane:continue'`. `-C` octal-escapes nonprintable bytes and needs decoding. Captured styled text is not complete emulator state. [Control-mode documentation](https://github.com/tmux/tmux/wiki/Control-Mode), [tmux manual](https://raw.githubusercontent.com/tmux/tmux/master/tmux.1) |
| **R3 §§1, 3.9: toolkit layers necessarily cost latency; Tauri “fails best latency”** | **Unmeasured conclusions.** Neither source establishes performance on the target hardware/workload. The native-desktop preference can independently exclude a WebView. |
| **R1 §§3, 4(c): ligatures are free; terminal widget is easy; 600–1000 renderer lines** | **Prototype estimates presented as product estimates.** Cell-constrained shaping, fallback, selection, emoji, damage invalidation, atlas eviction, and DPI changes require substantial correctness work. |
| **R5 §3(b): emulator mismatch is only cosmetic; Ghostty web is the only working WASM option** | **Incorrect/contradictory.** Width/mode divergence changes state. The same document identifies restty and wterm’s Ghostty integration. |
| **R5 §2 “hterm”: archived in 2022** | **Wrong basis for dismissal.** The cited GitHub repository is a mirror; the project identifies Chromium’s `libapps` tree as its main site. [hterm upstream](https://chromium.googlesource.com/apps/libapps/+/HEAD/hterm/README.md) |
| **R4 §2 “sshx”: cannot be self-hosted** | **Too categorical.** Upstream says self-hosted deployment is unsupported and requires substantial deployment work, not impossible. [sshx deployment documentation](https://raw.githubusercontent.com/ekzhang/sshx/main/README.md) |
| **R1 §2.4 / R2 §2 “zmx”: precedent proves straightforward upgrade-safe persistence** | **Incomplete evidence.** zmx documents IPC-version upgrade failures and terminal restoration limitations. It is relevant prior art, not proof of the proposed guarantee. [zmx known issues](https://github.com/neurosnap/zmx#known-issues) |

The Rust binding’s Zig 0.16 requirement and automatic pinned-source fetch are supported by its README. They remain packaging/build-reproducibility risks, rather than doubtful facts. [Binding build instructions](https://github.com/Uzaaft/libghostty-rs#building)

## Missing items

| Missing specification | Where it belongs |
|---|---|
| Deterministic terminal corpus; protocol state-machine tests; randomized chunk boundaries; resize/reconnect races; malformed-frame and snapshot fuzzing; soak tests | **PLAN §§5, 8** |
| Config paths/schema/migration; host discovery; shell, argv, environment and working-directory semantics; fonts, keybindings and presets | **PLAN §§4, 6–7** |
| Structured logs and correlation IDs; queue depths, parser time, replay gaps, frame timings, reconnect reasons and direct/relay connectivity; no terminal contents or secrets logged by default | **PLAN §§4, 8** |
| Protocol, holder IPC, snapshot and agent-adapter compatibility matrices; rollback and old-holder coexistence | **PLAN §§4, 9** |
| Reproducible Rust/Zig builds; Linux runtime dependencies and service installation; Windows installer/signing/update process; dependency license inventory | **PLAN §6** |
| Local crash diagnostics, symbol/build identification, opt-in report export and redaction | **PLAN §§6, 9** |
| IME/input ownership for both terminal and forms; dead keys, AltGr, compose, keyboard layouts, accessibility focus and screen-reader representation | **PLAN S2, phase 2** |
| Terminal-output trust policy: OSC clipboard access, links, file/image paths, resource limits and graphics decoding | **PLAN §§3–4** |
| Exact parity matrix, including external kitty-session discovery/focus, Open/Copy attach commands, configuration migration and agent CLI behavior | **PLAN §7; predecessor README “What it does”** |
| File transfer/upload protocol: §4 lists file reads, but image paste/drop requires writes, size limits, cancellation and ownership rules | **PLAN §4, phase 4** |

## Suggested changes to PLAN.md — concrete

1. **Replace §1’s conclusion with a hypothesis.**

   > Observed: tmls feels laggy and sometimes misses input. A widget-level test delivered all tested keys; end-to-end loss has not been localized. The current implementation has a 30 Hz refresh timer and synchronous parsing on its UI event loop. Phase 0 will distinguish input loss, delayed delivery, and delayed presentation before assigning causes.

2. **Add §4.1, “Session invariants.”**

   Require one canonical terminal size and capability profile; one reply authority; stable session/incarnation IDs; ordered output and resize events; snapshot boundaries; input acknowledgements/deduplication; and suppression of historical side effects.

3. **Add §4.2, “Recovery and overload contract.”**

   Pick the authoritative-state owner. Specify exactly what survives client crash, hostd crash, holder crash, logout, and reboot. Define retained history, memory/storage limits, slow-client policy, and behavior when replay is unavailable.

4. **Add §4.3, “Compatibility and security.”**

   Specify holder/network/snapshot version negotiation, device enrollment/revocation, browser auth, hook authentication, permissions, and non-idempotent retry rules.

5. **Replace the spike table with explicit gates.** The numbers below are proposed acceptance targets, not measurements.

   | Spike | Revised measurable gate |
   |---|---|
   | **S0 — missing: end-to-end input diagnosis** | Test at least 100,000 numbered input events per principal path, quiet and under a specified bytes/sec flood. Zero unexplained missing/duplicate events at the receiver; separate p50/p95/p99 delivery and presentation latency. Exercise real input/parser paths, not only widget injection. |
   | **S1 — emulator and recovery** | Pinned Linux and Windows builds; deterministic cases covering modes, alternate screen, queries, Unicode, graphics if required, and resize. Snapshot/restore at adversarial byte boundaries followed by identical suffixes must match uninterrupted execution. Explain differential disagreements. |
   | **S2 — complete interaction slice** | Grid, sidebar, editable form, selection, clipboard and IME on both target machines. Fixed 60 Hz/high-refresh tests, fractional DPI and monitor transitions. Set explicit millisecond/resource budgets; choose the framework using latency **and** implementation coverage. |
   | **S3 — persistence under faults** | At least 100 randomized hostd terminations, including abrupt death, creation/adoption races and downtime sufficient to wrap the ring many times. Child survives; promised state/history remains recoverable; no duplicate replies or input. Memory stays within declared limits. |
   | **S4 — transport under impairment** | Compare QUIC with persistent SSH/TLS over the same paths. Test direct/DERP, packet loss, constrained bandwidth, sleep and address changes. Define reconnect target from **restored connectivity**, e.g. p95 <1 s; verify replay and input deduplication under a simultaneous flood. |
   | **S5 — status and approval lifecycle** | Exercise every mark, including unseen completion and stale state; permission/tool/network/question dialogs; hook loss; hostd outage; competing decisions. Zero wrong-session or expired approvals. Document states requiring fallback. |
   | **S6 — missing: mixed-client compatibility** | Simultaneous clients with different viewport sizes; reconnect during resize; supported old/new binaries; minimal browser and CLI attach. Verify canonical state, one reply per query, explicit unsupported capabilities and controlled incompatibility errors. |

   Add a small ConPTY ownership/restart proof before finalizing the cross-platform holder interface.

6. **Split phase 1 and move basic usability earlier.**

   First deliver one host, one persistent session, a headless test client, authentication, recovery, and observability. Then deliver the desktop end-to-end slice and second host. Include basic copy/paste, scrollback, keyboard handling and IME before calling it usable for real work; keep rich search, approvals, and extras later.

7. **Make the schedule conditional.**

   “1–2 weeks” may produce feasibility prototypes; it is not a defensible allowance for all listed correctness, platform, and fault tests. R2’s “2–3 weeks to a usable core” similarly excludes much of the required behavior. Re-estimate after the gates, and keep full parity as a tracked release program rather than a feature-list promise.

8. **Replace §8 with a benchmark contract.**

   Name hardware, refresh rate, compositor/driver, build profile, workload, session/client count, output rate, history size and network path. Report percentiles, CPU/RSS, idle power, recovery time and input correctness. Require a material improvement over tmls; use direct SSH+tmux as a measured baseline rather than an unconditional pass/fail claim.