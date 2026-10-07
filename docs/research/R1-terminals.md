# R1 - Rust terminals and embeddable emulator libraries

Research date: 2026-10-07. Feeds PLAN.md section 3 open questions 1 (emulator crate), 2 (GUI stack, text rendering part) and 7 (Windows).
Method: GitHub API (licence, pushed_at, releases), crates.io API, repo READMEs/headers/Cargo.toml read directly, plus a few web searches.
Items marked **(unverified)** come from memory or a single secondary source and were not confirmed against the repo in this session.

## 1. Comparison table

| Project | Lang | Licence | Last release / activity (2026-10-07) | Embeddable as lib? | Graphics (sixel / kitty gfx) | Kitty kbd | Sync output 2026 | OSC 8 / 52 | Windows | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| **alacritty_terminal** (Alacritty) | Rust | Apache-2.0 | crate 0.26.0 (2026-04-06); repo pushed 2026-10-05; app v0.17.0 (2026-04-06) | Yes, crates.io, 1.87M dl, used by Zed/Warp-derived code | **No / No** (sixel issue #910 open for years) | Yes (TermMode flags; opt-in config) | Yes | Yes / Yes | Yes (ConPTY via `tty::windows`) | Pure Rust, small, stable, 0.x semver breaks each release. No key encoder, no snapshot. |
| **wezterm-term / termwiz** (WezTerm) | Rust | MIT | **last release 20240203** (nightlies only); repo still gets commits (2026-10-05, community PRs; Wez's own last commit 2026-08-12). termwiz crates.io 0.23.3 (2025-03), repo says 0.24.0 | termwiz yes (crates.io). **wezterm-term is NOT on crates.io**: git dependency only | Sixel (preliminary), iTerm2 images; kitty graphics partial **(unverified)** | Yes (`enable_kitty_keyboard`) | Yes | Yes / Yes | Yes (first-class) | Richest Rust core, but in-tree, undocumented as a library, release-less for 2.7 years. |
| **Rio** (`rio-vt`, `sugarloaf`) | Rust | MIT | v0.5.28 (2026-09-17); very active | `rio-vt` 0.5.x "embeddable terminal core" (headless quickstart in README) + `librio` C ABI; new, API immature | Sixel / iTerm2 / kitty gfx via `rio-graphics` **(unverified depth)** | **(unverified)** | **(unverified)** | **(unverified)** | Yes (wgpu path on Windows) | Core is derived from alacritty's crosswords with own parser and SIMD paths. Young as a library. |
| **Ghostty / libghostty-vt** | Zig, C ABI | MIT | No tagged libghostty; Ghostty v1.3.1 app; repo pushed 2026-10-07. Rust crate `libghostty-vt` 0.2.2 (2026-09-28), MIT OR Apache-2.0, 154k dl | **Yes**: C API with Terminal, RenderState, key/mouse/paste encoders, formatter, search, **snapshot encode/restore**, kitty graphics, WASM. API "not yet stable" | Kitty graphics yes (API present); sixel **(unverified)** | Yes (key encoder) | Yes | Yes / Yes | Library yes (CI present in Rust bindings, ghostty README says Windows+WASM). GUI app: macOS+Linux only | Best-regarded VT correctness. Needs Zig 0.16 at build time. |
| **Zed terminal** (`crates/terminal`, `terminal_view`) | Rust | **GPL-3.0** (terminal crates); gpui is Apache-2.0 | Zed v1.22.0 (2026-09-30) | No (GPL, app-internal) but is the best worked example | via alacritty_terminal: none | via alacritty | yes | yes | Yes (gpui_windows, DirectX 11 + DirectWrite) | Wraps `alacritty_terminal` + renders with gpui. Read for ideas, do not copy code. |
| **Warp** | Rust | **AGPL-3.0** (client); UI framework `warpui*` MIT | v0.2026.06.03 (2026-06-03); repo pushed 2026-10-07 | No (app-level) | partial **(unverified)** | **(unverified)** | **(unverified)** | **(unverified)** | Yes | Terminal model derived from Alacritty (`LICENSE-ALACRITTY` in `warp_terminal/model`), forked `vte`, wgpu 30. |
| **kitty** | C + Python + Go | GPL-3.0 | v0.49.2 (2026-10-01) | No | Own graphics protocol (reference impl) / no sixel | Reference impl (author's protocol) | Yes | Yes / Yes | **No** | Reference for graphics + keyboard protocol and the published perf numbers. |
| **foot** | C | MIT | v1.28.0 (2026-09-02) | No | Sixel yes | Yes **(unverified)** | Yes | Yes / Yes | No (Wayland only) | CPU-rendered (pixman), famously snappy. Useful as latency baseline and Wayland reference. |
| **vt100** crate | Rust | MIT | 0.16.2 (2025-07-12), 13M dl | Yes (screen-state parser, `contents_formatted()` diffing) | No | No | No | Partial | n/a (no PTY) | Aimed at tmux/screen-like programs, small. Low activity (122 stars). |
| **avt** (asciinema) | Rust | Apache-2.0 | 0.18.0 (2026-05-05); pushed 2026-08-14 | Yes, parsing + virtual buffers only | No | No | **(unverified)** | **(unverified)** | n/a | Deliberately "not 100% any variant". Good for recording/diffing, too minimal as primary core. |
| **Windows Terminal** (reference) | C++ | MIT | v1.25 (2026-10-02) | ConPTY source of truth | sixel yes | yes | yes | yes | Yes | Relevant only as ConPTY behaviour reference. |
| **Contour** (reference) | C++ | Apache-2.0 | v0.7.0 (2026-08-17) | Has libterminal | sixel, own | yes | yes (invented the sync-output spec) | yes | Yes | Mentioned for completeness. |

## 2. Per-project notes

### 2.1 alacritty_terminal (Alacritty)
- What: the grid, scrollback, VT state machine (on `vte` 0.15) and PTY/event loop that Alacritty uses, published as a library. https://github.com/alacritty/alacritty , https://crates.io/crates/alacritty_terminal
- Maintained: 0.26.0 on 2026-04-06, active changelog (bug fixes only post 0.26; `0.26.1-dev`). Apache-2.0. Semver 0.x: breaking changes every few releases (changelog marks them in bold, e.g. 0.26 changed `ChildEvent::Exited` payload). Source: https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/CHANGELOG.md
- API: `Term<T: EventListener>`, `Processor` (parser), `Grid`, selection, regex search, vi mode, hints. Docs.rs coverage is thin; real documentation is "read Zed/Alacritty". Includes a PTY layer (`tty`) with ConPTY on Windows (deps `windows-sys`, `miow`, `piper`) but you can ignore it and drive `Term` directly.
- VT correctness: escapes man page lists modes 2004 (bracketed paste), 2026 (synchronized output), 1004, 1006, 1049, plus OSC 8 and OSC 52. Kitty keyboard protocol is implemented in `Term` (`TermMode::KITTY_KEYBOARD_PROTOCOL`, off unless `kitty_keyboard` config set; the **key-to-bytes encoder lives in the Alacritty binary, not the crate**, so we must write/port it). **No sixel, no kitty graphics, no iTerm2 images**: the maintainers have refused/ignored image support for years (issue #910 still open). That matters for the PLAN's image viewer / `ct` only if users want in-terminal images from TUIs.
- Pitfalls: no snapshot/serialize of the whole terminal (we would have to walk the grid and re-emit VT or invent a format); no render-state diffing API (damage tracking exists: `Term::damage()`); changelog shows memory-safety fixes in 2026 (unbounded zero-width cell memory, `Row::new` unsoundness), i.e. still being hardened.
- GPU rendering in the app: OpenGL ES 2/3 (glutin), glyph atlas via `crossfont` (FreeType/fontconfig, CoreText, DirectWrite), no shaping or ligatures. Latency: third-party measurement of an old version at 57.9 ms (M1 Mac, camera method, 2022) vs kitty 37.5 ms. https://dev.to/lkhrs/measuring-terminal-latency-26m7 ; throughput 54 MB/s avg vs kitty 134 MB/s on Linux (alacritty 0.13.1, Ryzen 7 PRO 5850U). https://sw.kovidgoyal.net/kitty/performance/ . Alacritty tops the VTE/termbench throughput charts in Warp's own comparison (note: vendor page, no input latency). https://docs.warp.dev/terminal/comparisons/performance

### 2.2 WezTerm: wezterm-term, termwiz, mux
- What: GPU terminal by Wez Furlong with a rich in-tree stack: `term` (wezterm-term, the emulator), `termwiz` (escape parsing, input decoding, surface, widgets), `mux`, `pty` (portable-pty), `wezterm-ssh`, `wezterm-mux-server`. https://github.com/wezterm/wezterm
- Licence: MIT. Maintenance: **no tagged release since 20240203-110809-5046fc22**. Repo is under `wezterm/wezterm` now; main still receives commits (spelling fixes and community PRs on 2026-09/10; maintainer's own last commit 2026-08-12). Nightly builds are the practical release channel. Risk: bus factor 1 plus drive-by maintenance.
- Embeddability: `termwiz` is on crates.io (0.23.3, 2025-03; repo is at 0.24.0). `wezterm-term` is **not on crates.io**, so you depend on it via git rev and it drags in `wezterm-*` helper crates (wezterm-cell, wezterm-surface, wezterm-escape-parser...). Tattoy and a few others do exactly this. Documentation: essentially none beyond source.
- VT correctness: docs list sixel ("preliminary and incomplete"), iTerm2 image protocol, kitty keyboard (opt-in), OSC 8, OSC 52, synchronized output (DECSET 2026). https://wezterm.org/escape-sequences.html ; kitty graphics: documented as not listed, treat as partial.
- Rendering: wgpu/OpenGL front end, HarfBuzz + FreeType (and DirectWrite/CoreText discovery), **ligatures, font fallback, emoji** supported. Latency measured 45.8 ms vs 37.5 kitty on M1 (2022 camera method).
- Mux: the mux server is a hostd-like precedent (panes/tabs/windows in a daemon, clients attach via unix socket/TLS/ssh). Relevant to R2/question 6, but WezTerm's mux sends terminal *lines/diffs* from server-side emulator, not a raw byte stream. See also the reported pain points (multiplexing + scrollback sync); not verified in this batch.
- Pitfall: if you take `wezterm-term` you inherit its Lua/config-shaped type system (`TerminalConfiguration` trait) and its `Line`/`Cell` representation, with `Arc`-heavy cluster data; fine, but heavier than alacritty's grid.

### 2.3 Rio and rio-vt / sugarloaf
- What: wgpu-based terminal, MIT, v0.5.28 on 2026-09-17, MSRV 1.96.1 (nightly-ish cadence, active). https://github.com/raphamorim/rio
- New relevant fact: workspace now contains **`rio-vt`** ("Embeddable terminal core extracted from Rio: VT state machine, grid, PTY driver, selection, search", no GPU/font deps, has a headless quickstart with `Crosswords` + `Processor::advance`) and **`librio`** (C ABI of the same), **`librio-wasm`**, plus **`sugarloaf`** (renderer, wgpu only on Windows/WASM; native Metal/Vulkan elsewhere) and `rio-graphics`, `rio-fonts`, `rio-unicode`. https://github.com/raphamorim/rio/blob/main/rio-vt/README.md
- Maintained: the `rio-vt` path has 94 commits (all recent). It is a rename/extraction of the former `rio-backend` (crates.io `rio-backend` 0.5.28, 56k dl), originally an alacritty_terminal fork. So it inherits Alacritty's design with own parser and graphics (sixel, iTerm2).
- Verdict: interesting alternative but **too young as a library** (single maintainer, API churn, no third-party adopters seen). Watch, do not bet on it. sugarloaf is not a good dependency for us (custom, tied to Rio's needs, own glyph pipeline).

### 2.4 Ghostty / libghostty-vt (+ Rust bindings)
- What: Mitchell Hashimoto's terminal (Zig, MIT). Core is being split into reusable libraries; first one is **libghostty-vt**: parsing, terminal state, scrollback, reflow, key/mouse/focus/paste encoding, formatter (plain/VT/HTML), search, selection, tracked grid refs, kitty graphics, OSC/SGR parsers, **terminal snapshot encode/restore (CRC-protected binary format)**, **render-state** (dirty-region incremental updates for custom renderers), WASM helpers. https://github.com/ghostty-org/ghostty/tree/main/include/ghostty/vt (headers read: `render.h`, `snapshot.h`, `vt.h`)
- Status: header banner says "incomplete, work-in-progress API... not stable". README says "extremely stable (proven in Ghostty GUI)... API signatures still in flux", no tagged libghostty version, "compatible with macOS, Linux, Windows, and WebAssembly". https://github.com/ghostty-org/ghostty
- Rust: `libghostty-vt` + `libghostty-vt-sys` 0.2.2 (2026-09-28, MIT OR Apache-2.0), repo https://github.com/Uzaaft/libghostty-rs (392 stars, pushed 2026-10-06; community, not Mitchell's). Safe wrappers present for Terminal, RenderState, RowIterator/CellIterator, key/mouse encoders, paste, osc/sgr, **snapshot.rs**, **kitty graphics** (checked in `src/`). Build: vendored via `build.rs`, **requires Zig 0.16.x on PATH**, fetches a pinned ghostty commit (22d1317...), static lib on Windows (`ghostty-vt-static.lib`), Windows CI workflow exists. Has a Rust Ghostling port (macroquad) as an example.
- Ecosystem evidence (https://github.com/Uzaaft/awesome-libghostty): `zmx` (session persistence using libghostty-vt for state restore: the exact daemon pattern we want, MIT, 2.2k stars), `forgetty` (daemon-architected GTK4 terminal), `phantty` and `mightty` (Windows renderers on libghostty-vt), `restty` (web: libghostty-vt WASM + WebGPU/WebGL2, MIT), `coder/boo` (GNU-screen-style multiplexer), `go-libghostty` (Mitchell's own Go binding), RoyalTerminal (.NET/Avalonia). This is a real adoption wave, much of it for multiplexers and agent tooling.
- VT correctness: widely considered the strictest of the modern emulators (follows xterm/DEC behaviour, grapheme clusters mode 2027, kitty keyboard + graphics, sync output, OSC 8/52/133). I did not run vttest myself.
- Ghostty GUI rendering: Metal (macOS) and OpenGL (Linux), FreeType + HarfBuzz on Linux, CoreText on macOS, glyph atlas, ligatures, multi-threaded (IO/render/UI threads). No native Windows app upstream (community forks exist). Published input-latency numbers: none found (author has argued latency is dominated by frame pacing; **(unverified)**).
- Pitfalls: (1) Zig toolchain in the Rust build, pinned commit, slower CI; (2) unsafe FFI boundary, C allocations; (3) API churn until 1.0 (pin the commit and wrap in our own trait); (4) smaller Rust community than alacritty_terminal; (5) snapshot format is versioned but only meant to be restored by libghostty itself (same lib both sides, fine for us).

### 2.5 Zed's terminal
- `crates/terminal` (GPL-3.0-or-later) depends on `alacritty_terminal` + `vte`, uses its own `Terminal` wrapper that owns the `Term`, handles events, hyperlinks (regex via alacritty hints), path-like targets (`terminal_path_like_target.rs`), search, IME, on gpui. `crates/terminal_view` has `terminal_element.rs` (3k lines). https://github.com/zed-industries/zed/tree/main/crates/terminal_view/src
- Rendering technique (read from source): `layout_grid` walks visible cells, **merges adjacent cells with the same style into `BatchedTextRun`s** (one `shape_line` per run, not per cell), **merges background cells into `BackgroundRegion`/`LayoutRect` quads** (fixpoint merge), special-cases block-element glyphs as rects, then paints quads then shaped lines, plus separate cursor and IME overlay. Zed's text system caches shaped lines and glyphs in an atlas.
- gpui: Apache-2.0, `gpui` on crates.io is stale (0.2.2, 2025-10-22), so use a git dependency. Linux backend is now `gpui_wgpu` (wgpu + **cosmic-text 0.19 + swash**), Windows is `gpui_windows` (Direct3D 11 + DirectWrite + DirectComposition), plus macOS and a `gpui_web` crate. https://github.com/zed-industries/zed/tree/main/crates/gpui_wgpu . This is the strongest existing evidence that cosmic-text+swash+wgpu is production-grade for code/terminal text on Wayland.
- Takeaway: Zed's terminal is "alacritty_terminal + a very good batching renderer". Copy the *architecture*, not the code (GPL).

### 2.6 Warp
- Client open-sourced (AGPL-3.0; `warpui` UI crates MIT), v0.2026.06.03, repo pushed daily (agent-driven). https://github.com/warpdotdev/warp
- Terminal model derives from Alacritty's (LICENSE-ALACRITTY inside `crates/warp_terminal/src/model`), forked `vte`, wgpu 30, forked `font-kit`. Block-based UI model (command boundaries via shell hooks) is the notable differentiator. Their performance page benchmarks throughput only and explicitly does **not** measure input latency. https://docs.warp.dev/terminal/comparisons/performance
- Unusable as a dependency (AGPL), useful for session/agent UX ideas only (R4).

### 2.7 kitty (reference)
- C/Python/Go, GPL-3.0, v0.49.2 (2026-10-01), OpenGL, own text pipeline, **authoritative implementation of the kitty keyboard and graphics protocols**; the best-published perf page (throughput table, typometer-based latency claim: "far and away the best latency" among tested Linux terminals). https://sw.kovidgoyal.net/kitty/performance/ . No Windows. Not embeddable. Use its protocol docs and test suite as the compat target.

### 2.8 foot (reference)
- C, MIT, v1.28.0 (2026-09-02), Wayland only, CPU renderer (pixman), sixel, server/client mode, very low latency in the community folklore. https://codeberg.org/dnkl/foot . Proof that **GPU is not required for low latency**; what matters is avoiding extra frames of buffering and rendering only damaged regions. Latency numbers not independently verified here.

### 2.9 vt100 and avt crates
- `vt100` 0.16.2 (MIT): screen parser with `Screen::contents_formatted()` / `contents_diff()`, ideal for tmux-like multiplexing logic and tests, but no graphics, no kitty keyboard, 2025-07 last release. https://github.com/doy/vt100-rust
- `avt` 0.18.0 (Apache-2.0, asciinema): parser + virtual buffers; explicit non-goals: input, rendering, perfect fidelity. https://github.com/asciinema/avt
- Both are fine for tests/recording, not for a daily-driver terminal core.

### 2.10 Newer 2025-2026 projects worth knowing
- **zmx** (session attach/detach built on libghostty-vt, no windows/tabs/splits) https://github.com/neurosnap/zmx , **restty** (WASM + WebGPU web terminal on libghostty-vt) https://github.com/wiedymi/restty , **forgetty** (daemon-architected GTK4 terminal on Ghostty VT), **phantty** / **mightty** (Windows renderers on libghostty-vt), **coder/boo** (screen-style mux on libghostty), **it-shell3** (CJK-focused mux). Index: https://github.com/Uzaaft/awesome-libghostty
- **gpui + gpui_wgpu** and **Rio's librio-wasm** as signs that Rust GPU text stacks are consolidating on wgpu + cosmic-text/swash.
- None of these is a ready-made "GPU terminal + session manager with Claude-state awareness" for Linux+Windows.

## 3. GPU text rendering: how the good ones do it, and which crates

Common recipe (Alacritty, kitty, WezTerm, Ghostty, Zed all do variants of it):
1. Fixed cell grid: cell metrics from the primary font (advance, ascent, descent), computed once per font/DPI change.
2. **Glyph atlas** (R8 coverage for text, RGBA for colour emoji), glyphs rasterised lazily on CPU (FreeType/swash/CoreText/DirectWrite) and uploaded into atlas pages; LRU or page-reset on full.
3. Per frame, upload only **damaged rows** as instance data: background quads (merged runs), glyph quads (position, atlas rect, fg colour, flags), then decoration quads (underline, cursor, selection), via 2-3 instanced draw calls. Ghostty and Alacritty keep per-row dirty tracking; Zed rebuilds per frame but batches (section 2.5).
4. **Shaping**: kitty/WezTerm/Ghostty shape *runs* of cells (HarfBuzz) so ligatures and complex scripts work, and fall back per-cluster to other fonts (fontconfig/CoreText/DirectWrite discovery). Alacritty does no shaping (one glyph per cell: simple, fastest, no ligatures).
5. Special-case box-drawing/block/powerline glyphs as geometry (Alacritty 0.17 added built-in block elements U+1FB82..8B; Zed's `push_block_element_region`; kitty and Ghostty draw them procedurally) so lines join without seams at any scale.
6. Frame pacing: render on damage, present on vsync, skip the render thread while idle; use synchronized-output (2026) to avoid presenting half-updated frames from agents that redraw a whole screen.

Crate options (versions from crates.io, 2026-10-07):

| Crate | Version / date | Licence | Role | Verdict |
|---|---|---|---|---|
| `wgpu` | 30.0.1 (2026-08-22) | MIT/Apache | Vulkan/DX12/Metal/GL + WebGPU/WASM | Use. Same API native and in the browser. Zed, Warp, Rio, glyphon all on 30. |
| `winit` | (not surveyed here; R3) | Apache-2.0 | Window/IME/DPI | R3 decides. |
| `cosmic-text` | 0.19.0 (2026-04-22) | MIT/Apache | Font discovery (fontdb), shaping (rustybuzz/HarfRust), bidi, fallback, layout, rasterisation via swash | Use for font fallback+shaping; production in COSMIC, Zed Linux. Layout engine is general, so for the cell grid use only its shaper/fallback and place glyphs on the grid yourself. |
| `swash` | 0.2.10 (2026-07-17) | MIT/Apache | Glyph scaling/rasterisation (hinting, subpixel, colour/emoji, variable fonts), shaping | Use via cosmic-text. |
| `glyphon` | 0.12.0 (2026-07-09) | MIT/Apache/Zlib | wgpu text renderer = cosmic-text + atlas + wgpu pipeline | Good starting point, but its "text areas" model is per-run rich text, not a cell grid. Fine for UI chrome (tabs, session list); for the terminal grid write a dedicated instanced-quad cell renderer (about 600-1000 lines) using cosmic-text/swash for rasterisation + our own atlas. |
| `crossfont` | 0.9.0 (2025-06-09) | Apache-2.0 | Alacritty's rasteriser (FreeType, CoreText, DirectWrite) | Skip: no shaping, no ligatures, effectively Alacritty-only. |
| `parley` (linebender) | pushed 2026-10-06 | Apache/MIT | Newer layout/shaping on harfrust/fontique | Watch (R3). |
| `font-kit` | (forked by Zed and Warp) | MIT/Apache | System font enumeration | Optional; cosmic-text's fontdb covers us. |
| `beamterm` and similar | not verifiable (repo not found) | - | Terminal-specific atlas renderers | No evidence; do not rely. |

Ligatures: needed? Claude Code's UI uses plain text plus box drawing; ligatures are a nice-to-have. cosmic-text gives them for free if we shape per style run. Colour emoji + nerd-font symbols: need font fallback; cosmic-text handles it, test early on both OSes (Windows via DirectWrite fonts through fontdb is workable but less polished than DirectWrite itself, a known risk, see Windows below).

Windows: wgpu picks DX12 or Vulkan; both fine. IME and DPI belong to the window layer (R3). Font rendering will look slightly different from ClearType unless we add a gamma/contrast tweak; Zed solves this with DirectWrite on Windows, we would use swash greyscale AA (no subpixel). Acceptable but state it as a visual trade-off.

## 4. Recommendations

### (a) Which emulator core to embed

**Primary: `libghostty-vt` via `libghostty-vt` (Rust crate), in both `hostd` and the desktop client, wrapped behind our own small `Emu` trait.**

Why:
- **One core, three places.** Daemon, desktop client and (phase 5) browser client can all run the *same* engine (native + WASM via the same C library, proven by restty). That removes the main correctness hazard in PLAN section 3: daemon and client emulators disagreeing about the screen.
- **Snapshot encode/restore is a first-class API** with versioned, checksummed format and incremental scrollback restore (READY then older pages). That is exactly "on attach the daemon sends screen + scrollback then live bytes", and zmx is a working precedent of this pattern.
- **Render-state API** (dirty-region incremental, two-phase update so the IO thread is blocked only briefly) is designed for a custom GPU renderer, which is what we are building.
- Key/mouse/focus/paste **encoders included**, so kitty keyboard, bracketed paste and mouse modes encode correctly without us re-implementing them (alacritty_terminal leaves that to the app).
- Best-in-class VT behaviour (kitty keyboard + kitty graphics + sync output + OSC 8/52 + grapheme clustering), plus built-in search, selection and a formatter (plain text for `ag read`, HTML for the viewer).
- Windows and WASM are stated targets; MIT/Apache licences; very active.

Costs to accept (and mitigate):
1. Zig 0.16 required by `build.rs`: pin it in CI and the dev shell; consider building the static lib once and caching it. Windows MSVC build must be proven in the first spike.
2. Pre-1.0 API and community-maintained Rust wrapper: pin both crate version and ghostty commit; confine all use to one crate (`emu`) so a swap is a day of work.
3. FFI: `Terminal` is not `Sync`; keep each session's terminal on one thread (daemon: per-session task; client: renderer holds a lock only during `begin_update`).

**Fallback (and test oracle): `alacritty_terminal`.** Pure Rust, Apache-2.0, boring, stable, proven by Zed/Warp. Use it if the Windows/Zig build proves painful or libghostty-vt breaks. Trade-offs: no graphics, no snapshot (we'd serialise the grid ourselves), no key encoder, 0.x semver churn.

Not chosen: `wezterm-term` (git-only, no release since 2024-02, undocumented as a library, though MIT and feature-rich), `rio-vt` (too new), `vt100`/`avt` (too minimal; keep `vt100` or `avt` for cheap tests and tmux-control-mode glue).

**Gate before committing (1-2 day spike, do it first in Phase 1):** build `emu` against both libghostty-vt and alacritty_terminal; replay recorded Claude Code sessions (full-screen redraw floods, box-drawing UI, resize, alt screen, 2026 sync) and compare grids cell-by-cell; test snapshot/restore round trip; measure CPU per MB; build on Windows MSVC and Linux. Pick on evidence.

### (b) GPU text rendering

- **`wgpu` + `cosmic-text` (shaping, fallback) + `swash` (rasterise)** with our own R8/RGBA glyph atlas and an **instanced cell-quad renderer** fed from libghostty-vt's render state (damaged rows only). Use `glyphon` for the non-grid UI (tabs, session list, viewer chrome) or whatever R3 picks for the toolkit. This is the same stack Zed's Linux backend ships.
- Batch like Zed: merge same-style runs, merge background rects, draw box/block/powerline glyphs procedurally, draw the cursor/selection/underlines as quads.
- Present on damage + vsync, honour mode 2026, avoid extra swapchain frames (set frame latency 1, `PresentMode::Mailbox`/`Fifo` choice to be measured).
- Skip `crossfont`.
- Measure with typometer (X11/XWayland) plus a camera/photodiode check for Wayland, and an in-app timestamp from key event to present. Published terminal latency data is old (2022, macOS, camera) and unrepresentative of Wayland/Windows, so **our own numbers are the only ones that count** (PLAN section 7).

### (c) Do we need to build our own desktop terminal? (honest answer)

Can anything be extended instead?
- **WezTerm**: closest feature match (Rust, MIT, mux, Windows, GPU, ligatures, Lua, ssh domains). But: no release since 2024-02, a very large codebase with its own GUI toolkit and config model, mux protocol is its own thing, and every tmls requirement (Claude status marks, session list across hosts, inbox/`ag`, approvals, file viewer, image paste, QUIC reconnect) would be a fork that must track a half-maintained upstream. Doable as a Lua plugin for *some* things, not for hostd-owned sessions with client roaming. Realistically a fork we maintain alone.
- **Ghostty**: excellent but no Windows GUI, Zig codebase (our dev language is Rust), config-driven not extensible (no plugin API), GTK on Linux (KDE Wayland ok). Extending = forking Zig. No.
- **Zed**: terminal is GPL, tied into the editor. No.
- **Warp**: AGPL, whole product. No.
- **Rio**: MIT Rust, wgpu, small codebase, Windows. Plausible fork base but one-maintainer, rapid churn, and its UI is single-purpose. A fork buys little over embedding libghostty-vt directly.
- **kitty/foot**: not Windows / not Rust.

Conclusion: no existing terminal gives us the one-hop daemon-owned-session model plus a session manager UI plus Windows + Linux Wayland in Rust. The **terminal widget itself is not the hard or novel part** with `libghostty-vt` (render-state API) + wgpu + cosmic-text: it is roughly the grid renderer, selection/IME/mouse glue and font handling. So building our own desktop client is justified, **provided we keep it small and buy the emulator** (libghostty-vt), and don't write a VT parser.

What could make building our own *unnecessary for the daemon half*: reuse `zmx` (MIT, libghostty-vt, session persistence, Linux/macOS) or WezTerm mux as the hostd. That is R2's question; note zmx lacks Windows, windows/tabs/splits and a structured protocol, so likely a reference rather than a base.

Cheap insurance if the custom client turns out too costly: because hostd owns the sessions and libghostty-vt snapshots exist, a **thin attach client inside any existing terminal** (`tmls-rs attach NAME`, already in PLAN) keeps all sessions usable from Ghostty/WezTerm/Windows Terminal while the GPU client matures.

## 5. Source URLs (all accessed 2026-10-07)
- https://github.com/alacritty/alacritty , .../blob/master/alacritty_terminal/CHANGELOG.md , .../extra/man/alacritty-escapes.7.scd , https://crates.io/crates/alacritty_terminal
- https://github.com/wezterm/wezterm , https://wezterm.org/escape-sequences.html , https://crates.io/crates/termwiz
- https://github.com/raphamorim/rio , https://github.com/raphamorim/rio/blob/main/rio-vt/README.md
- https://github.com/ghostty-org/ghostty (include/ghostty/vt/*.h), https://github.com/Uzaaft/libghostty-rs , https://crates.io/crates/libghostty-vt , https://github.com/Uzaaft/awesome-libghostty , https://github.com/neurosnap/zmx , https://github.com/wiedymi/restty
- https://github.com/zed-industries/zed/tree/main/crates/terminal , .../terminal_view/src/terminal_element.rs , .../crates/gpui_wgpu
- https://github.com/warpdotdev/warp , https://docs.warp.dev/terminal/comparisons/performance
- https://sw.kovidgoyal.net/kitty/performance/ , https://dev.to/lkhrs/measuring-terminal-latency-26m7 , https://codeberg.org/dnkl/foot
- https://github.com/doy/vt100-rust , https://github.com/asciinema/avt
- https://github.com/grovesNL/glyphon , https://github.com/pop-os/cosmic-text , https://github.com/dfrg/swash , https://github.com/linebender/parley

## 6. Gaps / not verified
- No vttest or esctest runs were done here; correctness statements rest on docs/headers and community reputation. The spike in 4(a) should run vttest + kitty's keyboard tests.
- Rio's and Warp's kitty keyboard / sync-output / OSC support not checked.
- Ghostty latency numbers: none found; kitty page's "best latency" claim is third-party-sourced and old.
- Whether libghostty-vt's Windows MSVC build works from Cargo today is claimed (CI file exists, static lib naming) but not run.
- Sixel in libghostty-vt: not confirmed (kitty graphics is).
