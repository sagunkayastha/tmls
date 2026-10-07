# R3 — GUI stacks for a GPU terminal + session manager (2026-10-07)

Scope: PLAN.md open questions 2 (GUI stack) and 7 (one codebase, Linux + Windows). Targets: KDE Plasma Wayland + NVIDIA RTX 5070 Ti, and Windows 11.
Method: web search + fetched pages, Oct 2026. Items marked **[unverified]** come from my background knowledge, not a page I could open; re-check before committing. Several GitHub pages could not be fetched (403), so version numbers are the ones I could confirm.

## 1. Recommendation (short)

**(a) Primary: raw `winit` + `wgpu` with our own renderer, `cosmic-text` (or swash+harfrust) for shaping/fallback, and a small purpose-built UI layer for chrome (sidebar, tabs, forms, toasts). Runner-up: `gpui` via the `gpui-wgpu`/`gpui-ce` community forks or Zed's git tree (decision gate below).**

Why raw wgpu wins for *this* project:
- Priorities are latency and visuals. The terminal grid is ~90% of the pixels and 100% of the latency budget. Every retained/declarative toolkit adds a layer between a PTY byte and a presented frame; with our own loop we control exactly when we redraw, which present mode we use, and how many frames are queued.
- The UI chrome we need is small and boring (list, tabs, 3-4 forms, toast, image/text viewer). That is a few weeks of work on a custom widget layer, not a year, and it stays in our style (one theme, one font stack, same text engine as the grid).
- Proven by shipping terminals: Alacritty (winit+OpenGL), Rio (winit+wgpu "Sugarloaf"), WezTerm (own window layer + wgpu/OpenGL), COSMIC Term (iced's wgpu + glyphon custom widget). Nobody ships a good terminal by treating the grid as an ordinary widget.
- Both platforms use the same code path (wgpu: Vulkan on Linux, DX12/Vulkan on Windows), and winit gives Wayland, Win32, IME events.
- Cost we must accept: we own IME composition UI, accessibility (none initially), text input fields in forms, focus handling, HiDPI. Mitigation: use `egui` only for forms if hand-rolling inputs hurts (see 2.7 — egui can be embedded as an overlay on our own wgpu surface), or `accesskit` later.

**Runner-up: gpui.** Best-looking, fastest-feeling ready-made framework with a real terminal (Zed's), 75+ widgets (gpui-component), Windows now shipping in Zed (DX11/DirectWrite), Linux on wgpu since ~Mar 2026. Reasons it is not first: crates.io `gpui` is stale (0.2.2, Oct 2025, listed macOS+Linux only), so real use means a git dependency on Zed's monorepo or a community fork (`gpui-ce`, `gpui-wgpu`, `gpui-pre`, `gpui-box`) with breaking changes between versions; Zed's own team says out-of-scope features (e.g. tray) are rejected upstream; fcitx/ibus and Windows IME maturity is "being refined" in Zed itself. If the user wants "most polished UI soonest" and accepts upstream-churn risk, gpui is the right pick instead.

**Decision gate (cheap, do in week 1):** build a 1-day spike of the same screen (grid with 200x60 cells, scrolling flood, a sidebar) in (1) winit+wgpu and (2) gpui; measure key-to-photon with a high-speed camera/`typometer`-style timestamping on the PC under Wayland+NVIDIA. If gpui matches raw within ~1 frame and Windows builds cleanly, take gpui for development speed; otherwise raw wgpu.

Not recommended: iced (best runner-up among non-gpui, but see 2.3), egui (as the main shell), Slint, Makepad, Xilem, Dioxus native, Tauri.

## 2. Comparison table

| | winit+wgpu (own UI) | gpui | iced | egui/eframe | Slint | Makepad | Xilem/Vello | Dioxus native (Blitz) | Tauri |
|---|---|---|---|---|---|---|---|---|---|
| Render backend | wgpu (Vulkan/DX12/Metal/GL) | Linux: wgpu (since Mar 2026, replaced blade); Windows: DirectX 11 + DirectWrite (Zed); mac: Metal. Forks swap in wgpu everywhere | wgpu or tiny-skia (CPU) | wgpu or glow (GL) | Skia / FemtoVG (GL, now wgpu option) / software | Own GL/Metal/D3D11 shader renderer | Vello (compute, wgpu); Vello Hybrid/CPU variants | wgpu via Vello (Blitz) | OS webview (WebView2 / WebKitGTK) |
| Mode | Whatever we build | Hybrid immediate/retained | Retained (Elm), reactive redraw since 0.14 | Immediate | Retained/declarative (.slint) | Retained, GPU-shader-centric | Retained (Masonry) | Retained DOM/CSS | Web |
| Text shaping | cosmic-text (rustybuzz/harfrust) or own | cosmic-text (Linux), DirectWrite (Win), CoreText (mac) | cosmic-text | epaint/ab_glyph + ecolor; limited complex shaping | Parley/swash path in 1.16; FemtoVG+SW use swash | Own, basic | Parley + HarfRust (0.4) | Parley (Blitz) | Browser engine |
| Font fallback / emoji | cosmic-text via fontdb: fallback yes, colour emoji needs own atlas work | Good (platform text on Win/mac); Linux via cosmic-text | cosmic-text: fallback ok, colour emoji partial | Weak: no system fallback by default, no colour emoji (egui) | Decent | Weak | Improving (Parley) | Improving | Best |
| Ligatures | Yes (we choose) | Yes (editor) | cosmic-text yes | No/partial | Yes | Limited | Via Parley | Via Parley | Yes |
| AA | greyscale or own subpixel | greyscale on Linux; DirectWrite on Win (ClearType-ish) | greyscale | greyscale | Skia subpixel positioning (1.16) | greyscale | greyscale | greyscale | OS |
| Wayland fractional scale | via winit (wp-fractional-scale; fixes in 0.31 beta); we must render at physical px | Native Wayland (no winit); fractional ok | via winit | via winit | via winit | weak (X11/XWayland historically) | via winit | via winit | WebKitGTK via GTK |
| IME | winit Ime events (preedit/commit; 0.31 adds DeleteSurrounding); we draw preedit; fcitx5/ibus via text-input-v3 | Implemented own; Wayland text-input + Windows IMM; Zed Windows IME still being refined | 0.14 added input-method support | 0.35 improved IME; Linux/Win ok-ish | Winit-based; ok, simple fields | Poor on Linux **[unverified]** | Early, text-input-only **[unverified]** | Early | Good (browser) |
| Windows 11 | Good | Zed ships (Oct 2025); crates.io gpui doc says mac/Linux only | Good | Good | Good | OK | OK, immature | Alpha | Good |
| Accessibility | none unless accesskit added | Minimal in gpui; gpui-component claims AccessKit **[unverified detail]** | Minimal (COSMIC working on it) | AccessKit built in | Good on Win/mac, partial Linux | None | AccessKit built in (Masonry) | Via AccessKit (Blitz) | Browser a11y |
| Custom grid widget | Natural (it is the app) | Easy: custom Element; Zed did it | Medium: Canvas or custom shader widget via `iced_wgpu` Primitive (COSMIC Term does it) | Medium: paint callback or Painter; per-cell painter is slow, wgpu PaintCallback fine | Hard: need Skia/wgpu texture import hacks | Easy-ish (shader) but own ecosystem | Medium: Canvas widget just added | Hard (DOM) | Wrong tool: xterm.js in webview |
| Ecosystem | Tiny (we write) | Growing: gpui-component (16k stars, 60-75+ widgets, Apache-2.0), awesome-gpui | Large for Rust, COSMIC widgets (libcosmic) | Largest immediate-mode ecosystem | Good, commercial-backed | Small | Small, pre-1.0 | Small | Huge (web) |
| Licence | MIT/Apache deps | Apache-2.0 | MIT | MIT/Apache | GPLv3 / royalty-free / commercial | MIT/Apache | Apache-2.0 | MIT/Apache | MIT/Apache |
| Activity 2026 | Active (wgpu, winit 0.30.13 Mar 2026, 0.31 in beta) | Very active (Zed) but crates.io release stale | Active: 0.14 Dec 2025 | Very active: 0.36.1 Aug 2026 | Active: 1.16 Apr 2026 | Moderate | Active, pre-1.0 (0.4 Oct 2025) | Alpha (Blitz 0.3.0-alpha.4 May 2026) | Active |

## 3. Per-toolkit notes

### 3.1 Raw winit + wgpu (own UI)
- How the references do it:
  - **Alacritty** — winit + glutin/OpenGL, glyph atlas, deliberately no UI chrome. Best latency reputation on Linux via typometer; simple. https://github.com/alacritty/alacritty
  - **Rio** — winit + wgpu, own "Sugarloaf" renderer (usable as a library, also WASM-ready). Latest release found: v0.5.28, 2026-09-17, with Windows MSI (x86_64+ARM64), macOS, Linux/Wayland packages; 0.3 branch was a big rewrite for performance. https://github.com/raphamorim/rio , https://rioterm.com/changelog
  - **WezTerm** — own window abstraction (not winit), wgpu/OpenGL front ends, termwiz/wezterm-term emulator, built-in mux. 29k stars, 1.5k open issues; maintained as a spare-time project by one person (risk for depending on its crates). https://github.com/wezterm/wezterm
  - **Warp** — own "WarpUI" framework on GPU; open-sourced April 2026 (UI crates MIT, rest AGPLv3), claims 144+ fps, 1.9 ms average redraw. Worth reading for how a block-based terminal UI composes. https://github.com/warpdotdev/Warp
  - **Ghostty** — Zig; native GTK4/AppKit chrome + OpenGL/Metal renderer, `libghostty-vt` (Zig/C, Windows+WASM capable; Rust bindings exist: https://docs.rs/libghostty-vt ; API not stable, no tagged release). No native Windows GUI yet.
- Text: `cosmic-text` (shaping with rustybuzz/harfrust, font fallback via fontdb, swash rasteriser) + `glyphon` for wgpu atlas. For a monospace grid we can bypass layout: shape runs only for non-ASCII/ligature spans, cache glyphs in an atlas, draw instanced quads per cell run. Colour emoji needs a second RGBA atlas. Subpixel (LCD) AA is possible in our own fragment shader but must be disabled over translucent/transformed content; most modern terminals ship greyscale on HiDPI.
- Latency: present mode is ours. Use `PresentMode::Mailbox`/`Immediate` where supported or `Fifo` with `desired_maximum_frame_latency = 1` and redraw-on-demand (only when PTY bytes, input, or animation). Avoid redrawing the full UI per keypress: damage-limited UI layer + grid pass. NVIDIA+Wayland: Vulkan FIFO/Mailbox via wp-presentation; known driver quirks (see risks). Kitty's guidance is relevant: `repaint_delay`/`input_delay` tuning and disabling vsync raise energy use but cut latency. https://sw.kovidgoyal.net/kitty/performance/
- IME: winit `WindowEvent::Ime` (Enabled/Preedit/Commit/Disabled/0.31: DeleteSurrounding) + `set_ime_cursor_area` to place the candidate window. We must draw preedit text in the grid ourselves. Works with fcitx5/ibus through text-input-v3 on Wayland; Windows via IMM32/TSF through winit. Alacritty and Rio are the worked examples. winit 0.30.13 (2026-03-02) stable; 0.31.0-beta.3 (2025-09-04) adds Wayland popups, fractional-scale fixes, ImeHints. https://github.com/rust-windowing/winit/releases
- Wayland: client-side decorations are our job (sctk-adwaita in winit, or draw our own title bar — we have a tab bar anyway). KDE supports SSD via xdg-decoration; fall back to CSD only elsewhere.
- HiDPI: take scale from winit, render glyphs at physical pixel size, snap cell metrics to integer pixels to avoid seams on fractional scales (1.25/1.5).
- Accessibility: none by default; `accesskit_winit` can be added for the chrome (not the grid).
- UI chrome options on top: (i) hand-rolled (rects + text + hit-test; ~2-3k lines for sidebar/tabs/menus/toast); (ii) embed `egui` for forms/menus rendered into our wgpu pass (`egui-wgpu`, `egui-winit`); (iii) `taffy` for flex/grid layout; (iv) `vello`/`lyon` for rounded shapes and shadows.
- Effort/risk: highest dev cost of the options but lowest runtime risk; no framework upgrades churn except wgpu/winit majors.

### 3.2 gpui (Zed)
- State: Zed ships on Linux (wgpu renderer since ~Mar 2026; PR "gpui: Remove blade, reimplement linux renderer with wgpu (#46758)", motivated partly by NVIDIA freezes on Smithay-based Wayland compositors and by third-party GPUI app pain with blade; ~20% CPU reduction in review) and Windows (stable Oct 15 2025; DirectX 11 + DirectWrite). https://zed.dev/blog/zed-for-windows-is-here , https://git.secluded.site/zed/commit/af8ea0d6c26192c45f44f473c0d4a7d6f72ed018
- Standalone usability: crates.io `gpui` 0.2.2 (Oct 22 2025; Apache-2.0; owned by Zed devs; docs list macOS+Linux only; pre-1.0, breaking changes between releases). https://lib.rs/crates/gpui . Realistic use in 2026 = git dependency on `zed-industries/zed` crates/gpui pinned to a SHA, or community distributions: `gpui-ce` (community edition, active through Jul 2026; Zed devs themselves point people there for out-of-scope features such as tray), `gpui-wgpu` (wgpu on all platforms incl. Windows and mac), `gpui-pre`, `gpui-box`. https://docs.rs/crate/gpui-ce/0.3.2 , https://deepwiki.com/mdeand/gpui-wgpu , https://docs.rs/crate/gpui-box/0.1.0
- Docs: sparse. Best sources: gpui `examples/`, README (https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md), Zed's own source (the working reference), `awesome-gpui` (https://upd.dev/zed-industries/awesome-gpui), gpui-component docs.
- gpui-component (Longbridge): 16.4k stars, 75+ components (virtual list/table, dock/tab layouts, forms, menus, code editor, markdown), Windows/Linux/macOS/WASM, Apache-2.0, in production in Longbridge Pro. Directly covers sidebar/tabs/forms/menus. https://github.com/longbridge/gpui-component
- Text/latency: Zed advertises 120 fps; retained+immediate hybrid with a frame scheduler tied to the display; input handled in the platform layer then dispatched through actions. Text on Linux via cosmic-text (greyscale AA, ligatures, fallback good enough for an editor), DirectWrite on Windows.
- IME: implemented per platform (Wayland text-input, X11 XIM, Win32 IME); Zed asked for Windows IME/keyboard-layout feedback at launch, so treat as "works, edges".
- Terminal embedding: Zed's terminal is exactly this: `alacritty_terminal` VT core + a custom gpui `Element` that paints a batched grid (cell runs with same style merged into one text run + background rects). Apps built on gpui with terminals: Zed, Termy, Codux ("native AI coding terminal in Rust+GPUI, desktop/mobile/headless"), Arbor (git worktrees+terminals+diffs). Closest prior art to tmls-rs (agent session UI). https://www.mintlify.com/lassejlv/termy/features/gpu-acceleration
- Lessons from Zed's terminal: VT parsing/semantics (not drawing) dominates cost under floods; they batch cell runs and run the emulator off the UI thread; they track alacritty_terminal closely (PRs on rendering perf). Terminal in Zed gets the same GPU path as the editor, so smoothness is shared.
- Accessibility: minimal in core gpui.
- Risks: API churn, git dep size (48-115 MB of deps), upstream scope rejections, Windows via DX11 differs from Linux (wgpu), so two renderers unless using the wgpu fork.

### 3.3 iced (+ COSMIC)
- iced 0.14 released 2025-12-07: reactive rendering, time-travel debugging, headless testing, input-method support, concurrent image decoding, hot reloading, smart scrollbars, new widgets. https://www.phoronix.com/news/Iced-0.14-Rust-GUI-LIbrary , https://github.com/iced-rs/iced/releases
- Backends: wgpu (default) or tiny-skia; text via cosmic-text (shaping, fallback, ligatures, bidi) - same stack System76 maintains. Wayland/Windows via winit fork. MIT.
- COSMIC desktop is built on iced (libcosmic) = the largest real-world iced deployment; widgets, theming, a11y work happens there.
- Terminal prior art: **COSMIC Term** (GPL-3.0; alacritty_terminal + custom cosmic-text renderer, wgpu via glyphon with softbuffer+tiny-skia CPU fallback, bidi+ligatures; 594 stars, 257 open issues) https://github.com/pop-os/cosmic-term ; **iced_term** widget v0.8.0, alacritty backend, tested on mac/Linux/Windows, "does not provide full terminal features", unstable API, last push Apr 2026 https://github.com/Harzu/iced_term .
- Latency: retained Elm loop; 0.14 reactive rendering reduces idle redraws, but update->view->layout->draw per message is heavier than a custom loop; fine for a terminal if grid is a single custom primitive.
- IME: added in 0.14 (young). API breaks every release (no 1.0). Learned from COSMIC: keep the terminal as a bespoke widget outside the generic text pipeline.
- Verdict: best "framework" runner-up if gpui churn is unacceptable; weaker latency control and younger IME than raw winit.

### 3.4 egui / eframe
- Immediate mode, wgpu or glow, winit. Latest found: 0.36.1 (2026-08-07); 0.35 improved IME; 0.36 improved mobile keyboard/IME on web. AccessKit built in. https://newreleases.io/project/github/emilk/egui/release/0.35.0 , https://newreleases.io/project/github/emilk/egui/release/0.36.0
- Text: simple shaping, limited fallback/emoji, no real ligatures: poor fit for visual-quality priority.
- Latency: open issue "High input latency with wgpu backend"; Wayland `request_repaint` issues when minimized. Redraws the whole UI each frame it runs (fine, but repaint scheduling is a known pain).
- Terminal: `egui_term` (alacritty backend, mac/Linux tested, basic features) https://rust-digger.code-maven.com/crates/egui_term . Fine for tools, not for best visuals.
- Use only as an overlay for forms/debug panel inside option 3.1.

### 3.5 Slint
- 1.16.0 (2026-04-16): FemtoVG and software renderers use swash for glyph rasterisation; Skia got subpixel glyph positioning; LinuxKMS supports wgpu rendering. Winit backend with Wayland+X11. https://slint.dev/blog/slint-1.16-released , https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/
- Licence: GPLv3 / royalty-free (with attribution) / commercial; fine for personal use.
- Good UI quality and a11y; embedding a custom GPU grid requires texture-import/"rendering notifier" hacks and a bespoke wgpu pass; no known serious terminal built on it. Declarative DSL adds a second language.
- Verdict: good for forms-heavy apps, wrong for a latency-critical grid.

### 3.6 Makepad
- Own shader-based UI (GPU-first, live-design DSL), very fast rendering, runs mac/Win/Linux/web/mobile. Small ecosystem, thin docs, weaker Linux/Wayland+IME story **[unverified]**; I could not find 2026 release info. Not a safe base for a daily-driver terminal.

### 3.7 Xilem / Masonry / Vello (Linebender)
- Xilem+Masonry 0.4 (Oct 2025) switched shaping to HarfRust; Dec 2025 report: new Canvas widget, flex/grid gaps, accessibility docs, Parley ported to ICU4X, Vello Hybrid fixes. Vello is compute-shader based (needs capable GPU; fine on RTX 5070 Ti), Vello CPU/Hybrid for lower tiers. AccessKit native. Apache-2.0. https://linebender.org/blog/tmil-24/
- Pre-1.0, widget set small, IME not clearly described; no terminal known. Good vector quality, but cell-grid text is not Vello's sweet spot (glyph atlas beats path rendering for a grid). Watch, do not build on.

### 3.8 Dioxus native / Blitz
- Dioxus 0.7 includes Dioxus Native (wgpu HTML/CSS renderer, no webview); Blitz 0.3.0-alpha.4 (2026-05-17); described as pre-alpha with bugs and missing features; Nico Burns now full-time on it. https://dioxuslabs.com/blog/release-070 , https://lib.rs/crates/blitz-html
- Not ready; terminal embedding would be a custom canvas anyway.

### 3.9 Tauri (comparison only)
- WebView2 (Windows) / WebKitGTK (Linux) + xterm.js (WebGL renderer). Excellent text/emoji/IME for free and trivially shares code with the planned phase-5 web client. But: an extra IPC hop and JS in the keystroke path, WebKitGTK on NVIDIA+Wayland has a history of compositing/vsync glitches **[unverified for 2026]**, frame pacing outside our control, DOM-based chrome. Fails "best latency". Keep xterm.js for phase 5 (R5); do not use Tauri for the desktop app.

## 4. Terminal apps per stack and lessons

| Stack | Apps | Lessons |
|---|---|---|
| raw winit/OpenGL or wgpu | Alacritty, Rio, WezTerm | Glyph atlas + instanced quads; damage tracking; keep emulator on a separate thread; set frame latency to 1; kitty-style input/repaint delay tuning; WezTerm shows one-maintainer risk, and that built-in mux+GUI in one process is what we are trying to avoid |
| gpui | Zed terminal, Termy, Codux, Arbor | Custom Element painting merged text runs; alacritty_terminal as core; platform text system gives good emoji/fallback on Windows; the framework tracks Zed's needs, not ours |
| iced | COSMIC Term, iced_term | Custom wgpu/glyphon grid widget with CPU fallback; GPL-3 (cannot copy code into non-GPL without care); iced API churn breaks widget crates |
| egui | egui_term | Prototype-grade; per-cell painter slow |
| own framework | Warp (WarpUI) | Fully custom stack can hit 144 fps; UI crates MIT-licensed (check if reusable, https://github.com/warpdotdev/Warp) |
| Zig/native | Ghostty | Native chrome per platform + shared core is the opposite trade-off (best native feel, no code sharing for UI) |

## 5. Risks (ranked)

1. **Wayland + NVIDIA frame pacing/stalls** (any wgpu/Vulkan stack): Zed's blade→wgpu move was driven by NVIDIA freezes on Smithay compositors; KWin should be OK but must be tested on the actual machine (driver 5xx/Blackwell, explicit sync). Mitigation: week-1 spike on the 5070 Ti; keep a GL/software fallback toggle.
2. **IME on Linux** (fcitx5/ibus via text-input-v3, KDE quirks, candidate window placement) and Windows TSF; user may not need CJK but dead keys/compose do matter. winit has it; custom UI means we draw preedit.
3. **Text quality**: colour emoji, ligatures, fallback chain, fractional-scale pixel snapping; subpixel AA decisions. Budget time; consider `swash`+`harfrust` directly if cosmic-text overhead shows in profiles.
4. **gpui churn/forks**: pinned SHA, forks diverge, crates.io stale; Windows is DX11 in upstream vs wgpu in forks.
5. **Custom UI debt**: text inputs (selection, undo, IME) in forms are the costliest widget to hand-roll; use egui overlay or gpui-component text input for these if needed.
6. **Windows specifics**: DPI per monitor, ConPTY quirks (host side), flicker on resize (wgpu swapchain recreation), DX12 vs Vulkan choice; test on the laptop early.
7. **Accessibility**: effectively absent in the recommended path; acceptable for a personal tool, add `accesskit` later for chrome.
8. **Licences**: COSMIC Term GPL-3, Warp rest AGPL-3, Slint GPL option; do not copy code from those into a differently licensed repo; gpui/iced/egui/wgpu/winit are permissive.

## 6. Suggested next steps
1. Week-1 spike per decision gate: same 200x60 grid + sidebar, winit+wgpu vs gpui(-ce/-wgpu); log key-to-present timing and flood smoothness on KDE Wayland/NVIDIA, then on the Windows laptop.
2. Use `alacritty_terminal` or `libghostty-vt` behind a trait so the GUI choice and the emulator choice (R1) stay independent; the renderer only needs a cell-grid snapshot interface.
3. Decide chrome strategy (hand-rolled vs egui overlay vs gpui-component) after the spike.

## 7. Sources
- winit releases: https://github.com/rust-windowing/winit/releases
- gpui on lib.rs: https://lib.rs/crates/gpui ; docs: https://docs.rs/crate/gpui/0.2.2 ; README: https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md
- gpui Linux wgpu PR: https://git.secluded.site/zed/commit/af8ea0d6c26192c45f44f473c0d4a7d6f72ed018
- Zed for Windows: https://zed.dev/blog/zed-for-windows-is-here ; Zed on Linux: https://zed.dev/blog/zed-on-linux
- gpui forks: https://docs.rs/crate/gpui-ce/0.3.2 , https://deepwiki.com/mdeand/gpui-wgpu , https://docs.rs/crate/gpui-pre/0.3.3 , https://docs.rs/crate/gpui-box/0.1.0 , https://upd.dev/zed-industries/awesome-gpui
- gpui-component: https://github.com/longbridge/gpui-component
- iced 0.14: https://www.phoronix.com/news/Iced-0.14-Rust-GUI-LIbrary ; iced_term: https://github.com/Harzu/iced_term ; cosmic-term: https://github.com/pop-os/cosmic-term
- egui releases: https://newreleases.io/project/github/emilk/egui/release/0.36.0 ; egui_term: https://rust-digger.code-maven.com/crates/egui_term
- Slint 1.16: https://slint.dev/blog/slint-1.16-released ; backends: https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/
- Linebender Dec 2025: https://linebender.org/blog/tmil-24/ ; Sept 2025: https://linebender.org/blog/tmil-21/
- Dioxus 0.7 / Blitz: https://dioxuslabs.com/blog/release-070 , https://lib.rs/crates/blitz-html
- Rio: https://github.com/raphamorim/rio/releases ; WezTerm: https://github.com/wezterm/wezterm ; Warp: https://github.com/warpdotdev/Warp ; libghostty-vt: https://docs.rs/libghostty-vt , https://mitchellh.com/writing/libghostty-is-coming
- Latency references: https://sw.kovidgoyal.net/kitty/performance/ , https://dev.to/lkhrs/measuring-terminal-latency-26m7
