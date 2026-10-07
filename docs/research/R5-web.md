# R5 — Web terminal front-ends (research for phases 5/6)

Researched 2026-10-07. Sources: npm registry and GitHub API (versions, dates, stars, licences), GitHub issues,
project READMEs and source (ghostty-web `lib/`), web search. Items marked **[unverified]** come from memory or
secondary sources and were not confirmed against primary material. Nothing here was benchmarked by us; the public
data contains **no trustworthy cross-library throughput or frame-time numbers** (ghostty-web ships a
`bench/versus.ts` micro-benchmark of parse speed under happy-dom, not a browser render benchmark).
A real benchmark on our own traffic is a phase-5 task (see §5).

## 1. Comparison table

| Project | Rendering | Emulator | Perf (public evidence) | Correctness | Mobile / IME | Size | Licence | Activity (2026-10-07) |
|---|---|---|---|---|---|---|---|---|
| **xterm.js 6.0** (`@xterm/xterm`) | DOM (default), **WebGL2 addon** (texture atlas). Canvas addon **removed in 6.0** | JS, in-browser | WebGL addon is the fast path; used by VS Code, sshx, ttyd, Zellij. 6.x added sync output (mode 2026). No hard numbers found | Very broad: OSC 8, OSC 52 (6.0 clipboard addon + OSC52), Sixel/iTerm images via `addon-image`, ligatures addon, Unicode 11 addon, sync output. **Kitty keyboard protocol: not in 6.0**; landed on master for 7.0 (#5687 closed in 7.0.0 milestone) with many open bugfix issues (#5882, #5819, #5823). Kitty *graphics* protocol: no **[unverified]** | Weakest area. Hidden `<textarea>` + diffing: open bugs #6045 (keyCode 229 rollover dup/drop, 6.0.0), #6012 (textarea keeps IME text), #6078 (re-emits whole buffer on 229, Chrome/WebKit), #6112 (IME single-char dropped when kitty kbd enabled), #5438 (Android physical keyboard `jk` -> `kjj`), #4173 (Android delete re-triggers composition), #3600 (Chrome+Gboard erratic), #3727 (iOS touch select/copy broken), #5377 (no real touch handling), #5343 (image addon breaks Chrome/Android) | core ~5.9 MB unpacked npm (≈ 300-400 KB min+gz core **[unverified]**), webgl addon 2.4 MB unpacked | MIT | 21.3k stars, v6.0.0 2025-12-22, 6.1 betas, pushed 2026-09-13, 293 open issues. ~6.8M npm downloads/week |
| **ghostty-web** (coder) | **Canvas 2D** (`getContext('2d')`, `lib/renderer.ts`); no WebGL/WebGPU | **libghostty-vt compiled to WASM** (patched Ghostty, ~400 KB wasm) | "60 FPS" claim only; no published numbers. Canvas2D full-repaint issue at fractional DPR (#198, 2026-09-27) | Ghostty VT core: grapheme clusters, XTPUSHSGR, OSC 8 (own link provider), uses Ghostty **KeyEncoder** (kitty keyboard capable). Images: not seen in `lib/`. Title only parses OSC 0/1/2 naively. OSC 52 not seen in source **[unverified]** | Has `beforeinput` + composition handlers, `touchend` caret hack, **but** file header still says IME "to be added", open #119 (Korean IME broken), #145/#148 (paste/Ctrl+V), #187 (no screen-reader text). No mobile/touch story documented, no iOS/Android issues filed (= little mobile use, not = works) | npm unpacked 2.2 MB; wasm ~400 KB | MIT | 2.9k stars, **npm 0.4.0 2025-12-09**, last push 2026-07-02, 64 open issues; 563k npm dl/week. Slow cadence; maintainers expect to swap in an official Ghostty WASM dist |
| **wterm** (Vercel; `@wterm/*`) | **DOM** rows with dirty-row tracking + rAF; native selection/find/a11y | Own ~12 KB Zig->WASM core, optional `@wterm/ghostty` (libghostty, ~400 KB) | Claims fine for htop-like load; no numbers | Core is minimal; full VT only with ghostty core. Kitty/OSC 52: undocumented | Not documented (DOM selection helps iOS native selection/copy, a real plus) | 12 KB / 400 KB | Apache-2.0 (npm `@wterm/dom` 0.5.4) | 0.5.x, published 2026-09-29. Young |
| **restty** | **WebGPU** (WebGL2 fallback), text-shaper; canvas + hidden IME input | libghostty-vt WASM | Unmeasured | Kitty image protocol "can fail in edge cases" (own README) | Hidden IME input design; unproven on mobile | npm 9.4 MB unpacked | MIT | 0.3.0, 2026-09-05. Very young |
| **Rio / riotermjs** (`librio` -> WASM) | Canvas **or** DOM, per instance | Rio's Rust core (`teletypewriter`/crosswords, from alacritty lineage) compiled to WASM | Unmeasured | Unknown | Unknown | n/a (fetch failed, 503) | MIT (Rio) | Rio itself active (7.6k stars, push 2026-10-04); riotermjs is a side project **[unverified maturity]** |
| **beamterm** (+ ratzilla) | **WebGL2 instanced** (one draw call per terminal), font atlas; also native OpenGL 3.3 via glow. Renderer only | None (you bring one) | Claims <1 ms render | n/a (cells in, pixels out) | None (no input layer) | small Rust/WASM | MIT | v1.0.0 2026-03-30, 201 stars, one maintainer. **Only the Rust-native "cell grid -> WebGL2" renderer found** |
| **justerm-core** (crate) | None | Rust VT -> grid + scrollback + **damage**, builds on `vte`; ships `justerm-wasm-decode` for a wire format | n/a | Partial, "design-complete, implementation ongoing" | n/a | n/a | MIT/Apache-2.0 | 0.7.0, early stage. Interesting as a *design* (server emulator, damage wire format, wasm decoder), not a dependency |
| **wezterm-term / alacritty_terminal in WASM** | n/a | Rust crates | No public working browser port found. alacritty_terminal is I/O-light (pty is cfg-gated) so wasm32 *should* build **[unverified]**; wezterm-term pulls termwiz/config deps, heavier **[unverified]** | Best-in-class VT (wezterm: kitty kbd + graphics, iTerm images, sixel, OSC 8/52) | Would need our own input layer | would be ours | MIT / Apache-2.0 | Both maintained; **nobody ships this** -> we would be first |
| **hterm** (Chromium) | DOM (+ Blink Shell ships it in a WKWebView) | JS | OK, dated | OSC 52, OSC 8 (partial), no images | Good on iOS via Blink's own native input layer **[unverified]** | small | BSD-3 | **Archived** (repo archived, last push 2022-06). npm package 2015 |
| **ttyd** | Serves xterm.js 6? (its `html/package.json`: **xterm 5.5 + webgl 0.18 + canvas 0.7 + image + unicode11 + clipboard + zmodem.js**), preact UI | JS (xterm.js) | libwebsockets C server, solid | = xterm.js 5.5 incl. Sixel; has ZMODEM | = xterm.js issues; no custom mobile layer | n/a | MIT | 12.5k stars, push 2026-08-12 |
| **gotty** (yudai) | hterm (originally) / xterm.js in forks | JS | n/a | n/a | n/a | n/a | MIT | 19.6k stars, **unmaintained** (push 2024-08); use fork `sorenisanerd/gotty` **[unverified]** |
| **wetty** | xterm.js (`@xterm/xterm ^6.0.0`, webgl 0.19, image 0.9, fit, web-links) | JS | Node + node-pty | = xterm.js 6 | = xterm.js issues | n/a | MIT | v4.0.0 2026-10-05; 5.5k stars; active |
| **sshx web client** | xterm.js via vendored fork **`sshx-xterm` 5.2.1** + addon-webgl 0.16 + image + web-links; Svelte 3 / SvelteKit 1, Rust server | JS (xterm.js fork) | Infinite-canvas of many terminals; mesh server does raw bytes + seq numbers | = xterm.js 5.2 | Desktop-first, not mobile-tuned | n/a | MIT | 7.7k stars, push 2026-09-21. Note: they had to **fork** xterm.js |
| **Zellij web client** (0.43+) | **xterm.js** over **two WebSockets** (terminal data + control); axum server, tokens, HTTPS, read-only tokens; PWA, "mobile interface with touch controls, viewport resizing" | Zellij server renders the *whole multiplexed screen* to VT bytes; browser xterm.js is a dumb display | Fine | = xterm.js; Zellij maintains its own mouse integration | Docs claim mobile UI; no data on IME quality | n/a | MIT | Very active (35.7k stars, push 2026-10-06). **Closest architectural analogue to tmls-rs** (Rust daemon + server-side VT state + xterm.js client) |
| **VS Code terminal** | xterm.js (WebGL "GPU acceleration" with DOM fallback) | JS xterm.js + VS Code shell-integration OSC 633 | The reference deployment: xterm.js perf is what VS Code ships | Same | VS Code web on iPad/Android is known to be poor for terminal IME (xterm.js issues above) | n/a | MIT | Drives xterm.js development (6.0 imported VS Code's `base/` scrollbar) |

## 2. Per-project notes

### xterm.js — https://github.com/xtermjs/xterm.js
- 6.0.0 (2025-12-22): canvas renderer addon deleted -> only **DOM** and **WebGL2**. Added DEC 2026 sync output, OSC 52, shadow-DOM WebGL, progress addon, ligature detail, VS Code scrollbar (changes viewport/scroll behaviour — retest touch scrolling), removed `windowsMode`/`fastScrollModifier` and alt+arrow->ctrl+arrow mapping. https://newreleases.io/project/npm/@xterm/xterm/release/6.0.0
- Addons we would use: webgl, fit, web-links (OSC 8 is handled in core via `linkHandler`), image (Sixel/iTerm2 inline), unicode11 (or `unicode-graphemes` in newer betas **[unverified]**), clipboard, search, serialize (could serialise a snapshot).
- Input architecture is the root of the mobile pain: a hidden textarea receives key/IME events and the library *diffs* its value (`_handleAnyTextareaChanges` with `setTimeout(0)`). Every race around that diff shows up as the "doubled text" bug. 2026 issues (#6045, #6012, #6078, #6112) show it is still not solved on desktop either; fix PRs are being proposed by users, not landed. The kitty-keyboard work (7.0) will add a second IME interaction surface (#6112).
- Next: 6.1 betas on npm (`6.1.0-beta.304`), 7.0 milestone (kitty keyboard).
- Security/maintenance: huge adoption, MIT, Microsoft/VS Code backed -> lowest abandonment risk.

### ghostty-web — https://github.com/coder/ghostty-web
- Drop-in xterm.js API (`import {Terminal} from 'ghostty-web'`, `onData`, `write`, `open`). WASM = libghostty-vt with a small patch (`patches/ghostty-wasm-api.patch`); build needs Zig+Bun.
- **Rendering is Canvas 2D with per-frame work**, not GPU; issue #198 (repaint every frame at fractional DPR) and #189 (a throw in `render()` kills the loop) show it is immature. On iPad/Android DPRs (2.625, 2.0) are often fractional -> check.
- Strengths: exact same VT semantics as native Ghostty (grapheme clusters, kitty keyboard encoder, XTPUSHSGR), tiny, MIT. Weaknesses: no images, no addons ecosystem, v0.4.0 since Dec 2025, IME/Korean broken, no a11y, no selection handles on touch, `getChars`/`translateToString` truncate graphemes (#200).
- Maintained mainly for Coder's `mux`; official Ghostty WASM distribution expected to replace the patch.
- Own wasm code paths: input-handler uses Ghostty `KeyEncoder` — so kitty keyboard comes "for free", unlike xterm.js 6.

### libghostty-vt — https://mitchellh.com/writing/libghostty-is-coming
Zero-dependency VT parser + state library with Linux/macOS/Windows/WASM targets; consumers include ghostty-web, wterm/ghostty, restty, an Obsidian plugin. It is the only mature *non-JS* emulator with a WASM story today. It is Zig (C ABI), so for a Rust workspace it is a C dependency on the desktop side (this is R1's question, not R5's).

### wterm (Vercel) — https://www.npmjs.com/package/@wterm/dom ; https://betterstack.com/community/guides/scaling-nodejs/w-term-dom/
DOM renderer: one `div.term-row` per line, dirty-row diff in rAF. Gives native text selection/find/screen reader, which is exactly what iOS/Android lack in canvas terminals. Concerns: DOM at 120x50 with fast full-screen redraws; unproven on Claude Code-style heavy TUIs; v0.5.

### restty — https://upd.dev/wiedymi/restty
Only candidate with **WebGPU** + libghostty-vt. 0.3.0, immature; own README admits kitty image edge-case failures. Worth watching, not building on.

### Rust -> WASM options
- **Rio's librio -> riotermjs** (https://upd.dev/raphamorim/riotermjs): proof that a Rust alacritty-lineage core runs in WASM with canvas/DOM renderers. Could not read the page (503) — re-check maturity before leaning on it.
- **beamterm** (https://github.com/junkdog/beamterm, docs https://docs.rs/beamterm-renderer): WebGL2 renderer taking a cell grid; same crate can render natively with OpenGL 3.3, so one renderer could serve both desktop (if glow-based) and web. 201 stars, single maintainer, v1.0.0 (Mar 2026). Renderer only: no input, selection, links, IME.
- **justerm-core** (https://docs.rs/crate/justerm-core/0.7.0): architecture is the same shape we want (daemon emulator -> damage wire format -> wasm decoder in browser) — read its design records, don't depend on it yet.
- **alacritty_terminal in wasm32**: no public port found. Plausible (the crate's pty code is behind cfg and the grid is plain Rust) but we would own panics-on-wasm, size and perf tuning **[unverified]**.
- **wezterm-term/termwiz**: heavier dependency graph (config, wezterm-input-types, etc.) **[unverified]**; better protocol coverage than alacritty_terminal.

### ttyd — https://github.com/tsl0922/ttyd ; gotty — https://github.com/yudai/gotty ; wetty — https://github.com/butlerx/wetty
All three are "PTY -> WebSocket -> xterm.js" (gotty originally hterm). Nothing to learn on emulation; ttyd's UI is a good minimal reference (preact + xterm 5.5 + webgl/canvas/image/unicode11/clipboard + zmodem). They do not solve mobile input; ttyd issues for Android Gboard are the same xterm.js ones. gotty is stale; wetty and ttyd alive.

### hterm — https://github.com/chromium/hterm
Archived 2022. Skip. Notable only because Blink Shell (iOS) built a native input layer *around* a WebView-hosted hterm — the same pattern as tmls's native Android input view, i.e. independent confirmation that serious mobile terminals do not trust the WebView's IME.

### sshx — https://github.com/ekzhang/sshx
Rust server + Svelte client, **vendored xterm.js fork `sshx-xterm` 5.2.1**, webgl + image + web-links addons. Many terminals on one canvas; transport is sequence-numbered raw byte chunks with E2E crypto. Lesson: even a polished product forked xterm.js; do not expect to track upstream unpatched.

### Zellij web client — https://zellij.dev/documentation/web-client ; https://heise.de/-10549868
Rust server streams the multiplexer's rendered screen to xterm.js over a terminal WebSocket plus a control WebSocket (resize, config, session switch). Auth by hashed tokens, HTTPS required off-localhost, PWA, read-only tokens, URL-per-session. Mouse integration customised. This is the design tmls-rs phase 5 is essentially replicating — worth reading its `web_client` code (server crate) before specifying the WS protocol.

### VS Code — https://code.visualstudio.com/docs/terminal/advanced
xterm.js with WebGL ("terminal.integrated.gpuAcceleration": auto/on/canvas/off) and DOM fallback. Because the largest xterm.js consumer ships WebGL+fallback, the WebGL addon (context loss handling!) is the proven production path.

## 3. Answers

### (a) Recommended web renderer for phases 5/6; runner-up
**Recommended: xterm.js 6.x + `@xterm/addon-webgl` (DOM fallback), with our own input layer.**
- Only option with a full addon set (images, links, clipboard, search, serialize, unicode), huge deployment (VS Code, ttyd, wetty, Zellij), MIT, active, Sixel/OSC 8/OSC 52 now.
- It fits the PLAN's draft decision, with the explicit proviso that **we do not rely on its hidden-textarea IME path on touch devices**: on Android we already have the native input view; on iPad Safari use hardware keyboards through xterm's normal path and a custom soft-keyboard path (below).
- Kitty keyboard protocol: not in 6.0. Decide whether Claude Code/Neovim need it on the web; if so, do key encoding ourselves (we intercept keys anyway, see c) or take 7.0 betas. Do **not** advertise kitty keyboard flags to web clients until it works, otherwise #6112-type regressions appear (hostd emulator must answer `CSI ? u` per client).
- Pin versions; expect to carry a small patch set (sshx forked at 5.2.1).

**Runner-up: ghostty-web** (same xterm API, so a swap is cheap if we code against the xterm API subset: `open/write/onData/onResize/resize/loadAddon`). Revisit when (1) a release newer than 0.4.0 exists, (2) it has a GPU or at least damage-based renderer, (3) IME/Korean and fractional-DPR issues are closed, (4) images exist. Its advantage is VT parity with a Ghostty-based desktop core, which only matters if R1 picks libghostty.

**Third path worth a spike if xterm.js perf or input proves painful: thin renderer.** hostd already owns the authoritative emulator and sends snapshots (PLAN §3). For the web, hostd could instead send *cell-grid damage* (justerm-style), and the browser needs no emulator at all — just a renderer (beamterm WebGL2, or ours) plus our own input layer. This removes byte-stream-versus-emulator drift and the xterm.js emulator cost, at the price of losing selection/links/search/images from addons (we would implement them on the grid) and extra daemon-side work per web client. It also makes phase 5 depend on the proto grid format, which phases 1-4 do not need. Recommend deferring; note it in the spec as an open option.

### (b) Is sharing the Rust emulator core desktop <-> web via WASM worth it vs xterm.js?
**Not for phase 5; keep it as a later, reversible option.**
- Evidence for feasibility is thin: no maintained project runs alacritty_terminal or wezterm-term in a browser (only Rio's side project and libghostty-vt, which is Zig). ghostty-web is the only *working* WASM-emulator web terminal and it still lacks images, IME, GPU rendering.
- Benefits of sharing are real but narrower than they look: because hostd holds the authoritative emulator and sends a snapshot on attach, client-side divergence only matters between snapshot points. An xterm.js-vs-Rust mismatch shows up as small rendering differences for odd sequences, not as lost state. The Rust core's value is **one codebase for protocol correctness** (kitty kbd, grapheme width, OSC handling) and identical scrollback/selection semantics on desktop and web.
- Costs: you must write the web renderer + input + selection + links + IME + a11y yourself on top of the WASM grid (beamterm covers only drawing), i.e. re-create the xterm.js surface that took years; WASM size/GC friendliness; and the mobile input problem is *not* reduced at all (it is the DOM/IME layer, not the emulator).
- Decision rule: if R1 selects **libghostty-vt**, ghostty-web already gives "same core on web" at near-zero cost -> adopt it as a drop-in alternative behind the same xterm-API interface. If R1 selects **alacritty_terminal/wezterm-term**, do not pursue WASM until after Android (phase 6); at that point run a 2-day spike: build the crate for wasm32, feed it a recorded Claude Code session, measure parse MB/s and wasm size, render with beamterm. Only proceed if it clears xterm.js WebGL on the throughput test in §5 and we have appetite to own the input layer.
- Design implication for now: keep `proto` byte-stream based and let the web client choose its emulator; reserve a capability bit for a future "grid damage" stream.

### (c) Concrete known fixes for Android / iOS soft keyboard input in web terminals

What the public record shows (xterm.js issues cited above): the failure is the browser/IME contract, not the terminal — Chrome Android + Gboard does not reliably fire `keydown`/`keypress`, reports `keyCode 229`, and wraps Enter/Backspace/space in composition events (#3600, quoting CodeMirror's `domobserver.ts` as the reference solution); interrupted compositions duplicate text (#4173, #6045); `onData` timing differs between iOS and Android WebViews (#5108).

Fixes that work (ranked, ours first):
1. **Native invisible input view in the Android app (tmls's existing fix).** Own `onCreateInputConnection` with `TYPE_NULL` or `TYPE_TEXT_VARIATION_VISIBLE_PASSWORD | TYPE_TEXT_FLAG_NO_SUGGESTIONS`, `IME_FLAG_NO_EXTRACT_UI | IME_FLAG_NO_FULLSCREEN`, implement `commitText`, `deleteSurroundingText`, `sendKeyEvent`, and forward committed text through JS bridge into the terminal (`term.input()` exists since xterm 5.4, or straight to the WebSocket). Never let the WebView's textarea receive IME events. This is the Termux model and Blink's model on iOS **[unverified for exact flags]**. Port tmls's `TerminalKeys` unchanged — it is proven.
2. **If a browser-only (Safari/Chrome tab) path must work: replace xterm's textarea handling** instead of patching around it:
   - Use `beforeinput`/`input` `inputType` (`insertText`, `insertCompositionText`, `deleteContentBackward`, `insertLineBreak`) as the source of truth; ignore `keyCode 229` keydowns; commit on `compositionend` only.
   - Keep a **shadow value** of the textarea and compute a delta (common prefix/suffix trim) on every `input`; never emit the whole value (cause of #6078). Flush synchronously inside the event, not `setTimeout(0)` (cause of #6045).
   - After `compositionend`, clear the textarea **one tick later** (proposed in #4173/#6012); skip clearing while composing, and test Korean/Japanese since #4173 reports the clear breaks Hangul.
   - Attributes on the helper element: `autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false"`, `inputmode="text"` (or `none` + own on-screen keys), and the "password"-style input trick to disable Gboard suggestions/predictive text **[unverified for current Gboard]**. Disabling predictive text removes most composition spans, so many words arrive as plain `insertText`.
   - Handle Enter/Backspace from `beforeinput` since Gboard may not emit keydown for them.
   - Reference implementation to copy: CodeMirror 6 `domobserver.ts` (handles Android Chrome keyboard deletion/enter heuristics).
3. **Sticky modifier / special-key toolbar** (Esc, Tab, Ctrl, Alt, arrows) as DOM buttons — soft keyboards lack them; Blink and Termux do the same. Do not depend on the soft keyboard for modifiers; set Ctrl latch and apply it to the next committed character yourself.
4. **iOS Safari specifics**
   - `focus()` on the input must run inside a user gesture; keep one persistent input element rather than recreating it, or the keyboard dismisses.
   - Track `window.visualViewport` (resize/scroll) to lift the terminal above the keyboard; `100dvh`/`interactive-widget=resizes-content`; refit the terminal after keyboard open/close.
   - iOS touch selection/copy is broken in xterm.js (#3727) and touch support is nominal (#5377): implement our own long-press selection overlay (select by cells, copy via `navigator.clipboard.writeText` triggered from a button tap) and own touch scrolling (translate drags to scrollback or to wheel/arrow sequences in alt screen). A DOM renderer (wterm-style) gets native iOS selection handles for free but costs render speed.
   - Hardware keyboard on iPad works through normal keydown; Cmd shortcuts are intercepted by Safari — document which are unavailable.
   - Disable iOS double-tap zoom and rubber-banding (`touch-action: none` on the terminal, `overscroll-behavior: none`).
5. **Android WebView app extras**: `WebView.setOnKeyListener`/`dispatchKeyEvent` for physical keyboards to avoid xterm's physical-keyboard duplicate (#5438: `j`+`k` pressed together on Android gives `kjj`; fix by routing physical key events natively and not through the DOM at all).
6. **Do not enable kitty keyboard flags for mobile clients** (xterm.js #6112; also Gboard cannot produce key release events).

## 4. Risks / caveats
- Public benchmark data does not exist for any of these in realistic TUI-heavy workloads; the recommendation is based on ecosystem maturity, not measured speed.
- xterm.js and ghostty-web both have **active, unresolved** input-layer bugs in 2026, including on desktop; we should budget our own input layer regardless of renderer.
- Version facts (npm/GitHub API, checked 2026-10-07): @xterm/xterm 6.0.0 (2025-12-22), addon-webgl 0.19.0, addon-image 0.9.0, canvas addon last 0.7.0 (2024-04), ghostty-web 0.4.0 (2025-12-09), @wterm/dom 0.5.4 (2026-09-29), restty 0.3.0 (2026-09-05), wetty 4.0.0 (2026-10-05), beamterm 1.0.0 (2026-03-30).
- Not verified: Zellij web client mobile quality, Rio's riotermjs maturity, exact bundle min+gz sizes, ghostty-web OSC 52/image support.

## 5. Suggested phase-5 spike before locking the spec (half a day each)
1. xterm.js 6 + WebGL against hostd WS with a recorded Claude Code session replay at 10x speed: measure bytes/s sustained, long-task count (`PerformanceObserver`), p95 frame time on laptop Chrome and on the iPad.
2. Same replay on ghostty-web (Canvas2D) for a head-to-head.
3. Android: reproduce the doubled-text bug in a plain xterm.js page on the target phone to confirm the native input view remains required (yes expected).
4. Read Zellij's `web_client` server code for protocol ideas (control channel, resize, reconnect).

## 6. URLs
- https://github.com/xtermjs/xterm.js (issues #6045 #6012 #6078 #6112 #5438 #4173 #3600 #5108 #3727 #5377 #5343 #5882)
- https://newreleases.io/project/npm/@xterm/xterm/release/6.0.0
- https://github.com/coder/ghostty-web (+ issues #119 #145 #187 #198 #189 #200)
- https://mitchellh.com/writing/libghostty-is-coming
- https://betterstack.com/community/guides/scaling-nodejs/w-term-dom/ ; https://www.npmjs.com/package/@wterm/dom
- https://upd.dev/wiedymi/restty ; https://upd.dev/raphamorim/riotermjs
- https://github.com/junkdog/beamterm ; https://docs.rs/ratzilla ; https://docs.rs/crate/justerm-core/0.7.0
- https://github.com/ekzhang/sshx ; https://github.com/tsl0922/ttyd ; https://github.com/butlerx/wetty ; https://github.com/yudai/gotty ; https://github.com/chromium/hterm
- https://zellij.dev/documentation/web-client ; https://heise.de/-10549868
- https://users.rust-lang.org/t/in-browser-chrome-wasm32-terminal-emulators/103717
