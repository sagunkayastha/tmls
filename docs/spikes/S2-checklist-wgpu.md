# S2 checklist — `s2-wgpu` interaction slice (Track A, A3)

What this file is: (1) the checks only a person at the PC can do, as empty items to fill in; (2) the key
bindings; (3) what the hand-rolled wgpu UI cost to build; (4) what was not built.

Binary: `target-docker/release/s2-wgpu`, built with `tooling/docker/build.sh build --release -p s2-wgpu`.
Nothing below has been run on a real display, a real GPU, a real keyboard, a pointer or an input method:
archbox is headless. What was run there is in the task report
(`.superpowers/sdd/2026-10-08-s2-track-a-core-wgpu/task-4-report.md`, section "Smoke").

## 1. User checklist (at the PC) — to be filled in

Record for the session: date ________ · binary git hash (`meta` line of any bench log) ________ ·
monitor(s) and refresh rate (`kscreen-doctor -o`) ________ · scaling ________ · compositor
(`kwin_wayland --version`) ________ · NVIDIA driver (`nvidia-smi`) ________ · `game-mode status` ________

Start: `~/Others/tmls-rs/target-docker/release/s2-wgpu` (add `--cmd bash` for a plain shell).
If it does not start for a missing shared library: paste `ldd target-docker/release/s2-wgpu` here, install nothing.

### 1.1 fcitx5 pinyin in the text field
Ctrl+Tab twice (terminal → list → field), switch fcitx5 to pinyin, type `nihao`, pick 你好, press Enter.

- [ ] While composing, the pinyin letters are shown in the field, underlined, at the caret: ________
- [ ] The candidate window sits at the field's caret (not at the terminal cursor, not at 0,0): ________
- [ ] After choosing, the field holds 你好 exactly once, no stray letters: ________
- [ ] Enter adds an item named 你好 at the end of the list: ________
- [ ] Composing with the field already full of text: the composition stays visible (the text scrolls): ________
- [ ] Nothing of this was typed into the shell (look at the terminal): ________

### 1.2 fcitx5 pinyin in the terminal (Review Focus 2)
Click the terminal (or Ctrl+Tab until its left edge is blue). With `cat` running, compose and commit 你好.

- [ ] Preedit shown inline at the cursor, underlined: ________
- [ ] `cat` echoes 你好 once, no stray bytes: ________
- [ ] After visiting the list (Ctrl+Tab) and coming back, pinyin still composes in the terminal
      (the app switches the IME off while the list has the focus): ________
- [ ] With the list focused and pinyin active, `j` / `k` move the selection (no composition starts): ________

### 1.3 Select a wide-character line in the terminal and copy
Run `printf 'ab你好cd   \n'`. Drag over that line with the left button, then right-click → Copy
(or Ctrl+Shift+C). Paste somewhere else (e.g. `wl-paste | od -c`).

- [ ] The highlight never covers half of a wide character: ________
- [ ] Pasted text is `ab你好cd` — matches the grid, trailing spaces trimmed: ________
- [ ] Right-click opens the menu at the pointer; hover highlights a row; a click outside closes it
      without changing the selection: ________
- [ ] Menu → Paste types the clipboard into the shell; Menu → Clear redraws an empty screen (it sends
      Ctrl+L, see "Not built"): ________

### 1.4 Paste into the field
With `ab你好cd` on the clipboard: click the field, Ctrl+V.

- [ ] The field shows `ab你好cd`; the caret is after it: ________
- [ ] Shift+Left ×3 selects `好cd` (the highlight covers the whole wide character); typing `x` replaces
      it: ________
- [ ] Ctrl+A, Ctrl+C, then paste in another program: the field's text: ________
- [ ] Multi-line clipboard text arrives as one line (line ends become spaces): ________

### 1.5 Mouse and focus
- [ ] Click a list row: it is selected and the list has the focus outline: ________
- [ ] Wheel over the sidebar scrolls the list; over the terminal it scrolls the scrollback: ________
- [ ] Click in the field puts the caret at the click; a drag selects: ________
- [ ] In the field, press and hold Tab for two seconds: the focus goes to the terminal and no tabs /
      completion output appear there (the held key's repeats are dropped): ________
- [ ] Create / Cancel buttons work with the mouse: ________
- [ ] With the terminal focused, Tab completes in the shell (`ech` + Tab → `echo `); it does not move
      the focus: ________

### 1.6 Move the window to another monitor (if there is one)
- [ ] Monitors: ________ (Hz / scaling of each). Not applicable: [ ]
- [ ] After the move, glyphs are sharp (re-rasterised at the new scale; the terminal prints
      `s2-wgpu: scale factor …` on stderr): ________
- [ ] The grid size follows (`stty size` before / after): ________ / ________
- [ ] No latency cliff after the move (bench numbers below, same load): ________

### 1.7 Scaling 125 % — glyph crispness and latency
System Settings → Display → scale 125 %, then start the app (and once more: change the scale while it runs).

- [ ] stderr shows `scale 1.25` at start (or `scale factor 1.25` on a change), not 1 or 2: ________
      (If it shows 2: the compositor is giving integer scale + downscaling — record that, it is the blurry case.)
- [ ] Terminal glyphs are crisp (compare with Konsole at the same scale): ________
- [ ] Sidebar, field and menu text is crisp and sits on whole pixels (no smeared edges): ________
- [ ] Sidebar is about 240 logical px wide (300 physical at 125 %): ________
- [ ] Ctrl+= ×3 then Ctrl+− ×3: glyphs crisp at every step, grid size back to the start
      (`stty size`): ________
- [ ] Latency at 125 % vs 100 % (commands below): ________

Bench log commands. **Run every measurement twice: with the UI and with `S2_WGPU_NO_UI=1`** (the same
window without sidebar, form and menu — the terminal alone, as before the UI was built). The difference
is what the hand-rolled UI costs per frame on this GPU; on software rendering (headless, lavapipe) it was
about +3 ms submit → present, which says nothing about a real GPU. Do each at 100 % and at 125 %; type
~200 keys into the shell, then `exit`:

```sh
B=~/Others/tmls-rs/target-docker/release/s2-wgpu
                  "$B" --bench ~/s2-logs/wgpu-scale100-quiet-ui   --photon --present fifo --load-label quiet --cmd bash 2> ~/s2-logs/wgpu-scale100-quiet-ui.stderr
S2_WGPU_NO_UI=1   "$B" --bench ~/s2-logs/wgpu-scale100-quiet-noui --photon --present fifo --load-label quiet --cmd bash 2> ~/s2-logs/wgpu-scale100-quiet-noui.stderr
                  "$B" --bench ~/s2-logs/wgpu-scale125-quiet-ui   --photon --present fifo --load-label quiet --cmd bash 2> ~/s2-logs/wgpu-scale125-quiet-ui.stderr
S2_WGPU_NO_UI=1   "$B" --bench ~/s2-logs/wgpu-scale125-quiet-noui --photon --present fifo --load-label quiet --cmd bash 2> ~/s2-logs/wgpu-scale125-quiet-noui.stderr
```

(Create `~/s2-logs` first. Which variant a log came from is in its `meta` line: `"ui":true|false`.
`grep '"ev":"meta"' ~/s2-logs/<run>/latency.jsonl` shows it; it is not always the first line.)

- [ ] **`hz` in `meta` is the monitor's real refresh rate, not `0.0`**: ________
      It was `0.0` on the first PC runs: the rate was asked for before the window was on a monitor. The
      app now waits for it (up to 1 s after the first frame) and the `meta` line says where it came from:
      `"hz_source":"current_monitor"` = the monitor the window is on (what is wanted); `"only_monitor"` =
      one monitor is connected and it was used; `"unknown"` with `"hz":0.0` = still not found — please
      report it. Paste the `meta` line of one run: ________
- [ ] `"ui":false` in `meta` on the `S2_WGPU_NO_UI=1` runs and no sidebar on screen; `"ui":true` on the
      others: ________

Numbers: Track B's `spikes/s2-bench/analyze.py <dir>/latency.jsonl` once it is merged. Until then this
prints key → present (the time `present()` returned — not photons) from one log:

```sh
python3 - ~/s2-logs/wgpu-scale125-quiet/latency.jsonl <<'EOF'
import json, sys, math
ev = [json.loads(l) for l in open(sys.argv[1])]
print([e for e in ev if e['ev'] == 'meta'])
key = {e['id']: e['t_key_ns'] for e in ev if e['ev'] == 'key'}
d = sorted((f['t_present_ns'] - key[k]) / 1e6 for f in ev if f['ev'] == 'frame' for k in f['keys'] if k in key)
p = lambda q: d[max(1, math.ceil(q / 100 * len(d))) - 1]
print(f"keys {len(key)} attributed {len(d)} key->present ms p50 {p(50):.2f} p95 {p(95):.2f} p99 {p(99):.2f} max {d[-1]:.2f}")
EOF
```

| Run | scale in `meta` | hz in `meta` | hz_source in `meta` | keys | p50 ms | p95 ms | p99 ms | max ms |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 100 % quiet, UI | | | | | | | | |
| 100 % quiet, `S2_WGPU_NO_UI=1` | | | | | | | | |
| 125 % quiet, UI | | | | | | | | |
| 125 % quiet, `S2_WGPU_NO_UI=1` | | | | | | | | |
| other monitor (if any), UI | | | | | | | | |

Note for reading the logs: only keys typed while the terminal has the focus are in them. Keys typed into
the list, the field or an open menu, and Ctrl+Tab / Ctrl+= / Ctrl+−, get no `key` line (they are never
echoed in a terminal frame). A redraw caused by the UI alone (focus change, typing in the field) writes a
`frame` line with the old `seq`, `t_output_ns: 0` and no keys, like any re-present did before.

## 2. Key bindings

| Keys | Where | Does |
|---|---|---|
| Ctrl+Tab / Ctrl+Shift+Tab | everywhere | focus to the next / previous stop: list → field → terminal → list |
| Tab / Shift+Tab | list, field | the same. **In the terminal Tab and Shift+Tab go to the program** (completion) |
| Ctrl+= (or Ctrl++) / Ctrl+− | everywhere | font size one logical px up / down (6 … 48), terminal and UI together |
| ↑ ↓ or k j, PgUp PgDn, Home End | list | move the selection (the list scrolls with it) |
| Enter | list | go to the terminal |
| printable keys, IME | field | insert at the caret (replacing the selection) |
| ← → Home End, with Shift | field | move the caret / extend the selection (by whole characters) |
| Backspace / Delete | field | delete the selection or one character |
| Ctrl+A / Ctrl+C / Ctrl+X / Ctrl+V (also Ctrl+Shift+V) | field | select all / copy / cut / paste |
| Enter / Escape | field | Create (adds the name to the list) / Cancel (clears the field); focus goes to the list |
| Ctrl+Shift+C / Ctrl+Shift+V | terminal | copy the selection / paste (as before) |
| ↑ ↓ Enter, Escape or any other key | open menu | choose / close |
| everything else | terminal | goes to the program, as before |

Mouse: left click focuses what it hits (list row, field, terminal); drag selects (terminal cells, field
text); wheel scrolls the list or the scrollback, whichever is under the pointer; right click on the
terminal opens Copy / Paste / Clear.

Keys the terminal no longer receives: Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+=, Ctrl++, Ctrl+−. (Ctrl+_ — `0x1f`,
"undo" in readline and Emacs — still goes to the program.)

A key that is held while it hands the keys to the terminal (Tab in the field, Enter on a list item, the
key that closes the menu) types nothing into the terminal: its auto-repeats are dropped until the next
fresh key press.

With `S2_WGPU_NO_UI=1` none of the bindings above exist except the terminal's own (Ctrl+Shift+C / V):
every key goes to the program.

Where the focus is, is shown by a blue outline (list, field) or a blue line at the terminal's left edge.

## 3. Implementation cost

### 3.1 Lines of code (`wc -l`, comments and blank lines included; `tokei` is not installed)

A3 only — the non-terminal UI:

| File | Lines | What |
|---|---|---|
| `spikes/s2-wgpu/src/ui/mod.rs` | 487 | `Ui` state, effects, focus decisions, `key_event` / `ime_event` |
| `spikes/s2-wgpu/src/ui/draw.rs` | 344 | everything as quads, label truncation, cached layers |
| `spikes/s2-wgpu/src/ui/textfield.rs` | 335 | one-line editor: caret, selection, preedit, scrolling |
| `spikes/s2-wgpu/src/ui/layout.rs` | 293 | rectangles, hit-testing, terminal origin + grid |
| `spikes/s2-wgpu/src/ui/focus.rs` | 237 | focus ring, key routing |
| `spikes/s2-wgpu/src/ui/menu.rs` | 143 | context menu geometry + hit-testing |
| `spikes/s2-wgpu/src/ui/list.rs` | 114 | list selection + scrolling |
| `spikes/s2-wgpu/src/ui/theme.rs` | 108 | tokens, zoom steps |
| **`src/ui/` total** | **2061** | (1900 as first built; +161 in review fix round 1) |
| `spikes/s2-wgpu/src/app.rs` | 710 → 904 | wiring (includes the refresh-rate query for `meta`, which is not UI) |
| `spikes/s2-wgpu/src/gfx/grid.rs` | +8 / −5 | three helpers made `pub` |
| tests: `tests/ui.rs` 1672, `tests/ui_draw.rs` 678, `tests/session.rs` +396, `tests/atlas.rs` +66 | 2812 | |

For scale — the whole crate after A3 and its fix round (`wc -l`): `src/` 5781 (`ui/` 2061, `gfx/` 1746,
`app.rs` 904, the other modules 1070), `tests/` 6027.

What the 2061 lines buy: one list, one single-line text field, two buttons, one three-item menu, three
focus stops. No widget is reusable beyond this window: each is laid out by hand in `layout.rs` and drawn
by hand in `draw.rs`.

### 3.2 Hours

- Hours spent (A3): ________ (to be filled in by the user; not recorded by the implementer)
- Hours spent (A2, grid + input + latency marks): ________

### 3.3 Upstream limitations worked around

No upstream bug was hit that needed a patch or a fork. What had to be worked around are missing pieces.
References are to the crate version in `Cargo.lock` and its API docs; **no upstream issue numbers were
looked up or verified** — where one would be needed for a decision, that is still to do.

| # | Crate (version) | Limitation | What the code does instead | Reference |
|---|---|---|---|---|
| 1 | winit 0.30.13 | `winit::event::KeyEvent` has a private field, so a test cannot build one | every key function takes the event's parts (`logical_key`, `text`, modifiers, pressed, repeat): `input::key_action`, `ui::focus::route`, `ui::key_event` | <https://docs.rs/winit/0.30.13/winit/event/struct.KeyEvent.html> |
| 2 | winit 0.30.13 | named keys carry `text` (`"\r"`, `"\t"`, …) and Ctrl chords may carry printable text | `text` is ignored for named keys and with Ctrl held (`input.rs`); the field refuses text with control characters (`ui/focus.rs`) | same page, field `text` |
| 3 | winit 0.30.13 | IME: one `set_ime_allowed` / `set_ime_cursor_area` per window, no notion of widgets | the app moves the cursor area to the focused widget's caret after each frame (`Ui::ime_anchor`) and switches the IME off while the list has the focus (`app.rs` `sync_focus`). `Ime::Preedit`'s cursor range is ignored (caret drawn after the preedit) | <https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.set_ime_cursor_area> |
| 4 | winit 0.30.13 | no text-input, focus, menu or layout layer at all — by design | all of `src/ui/` | — |
| 5 | wgpu 30.0.1 | `Surface` present returns no presentation time | `present_kind: "present_return"` plus `t_work_done_ns` from `Queue::on_submitted_work_done`, which only fires inside `Device::poll` (polled every 500 µs on the main thread) | <https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.on_submitted_work_done> |
| 6 | wgpu 30.0.1 (GLES backend) | no non-zero first instance on GLES-class devices (A2 design note; not re-checked here) | one instance buffer, one draw call from instance 0; UI layers are concatenated with the terminal's (`ui::draw::layer_order`) | <https://docs.rs/wgpu/30.0.1/wgpu/struct.DownlevelFlags.html> |
| 7 | wgpu 30.0.1 | scissor is per draw call, and there is one draw call | no clipping anywhere: labels are cut to whole columns with "…" (`ui::draw::fit`), the field draws only characters fully inside it (`TextField::view`). A glyph wider than its cell (fallback font) can still overhang by a few pixels | <https://docs.rs/wgpu/30.0.1/wgpu/struct.RenderPass.html#method.set_scissor_rect> |
| 8 | cosmic-text 0.19.0 / swash 0.2.10 | used for shaping and rasterising single cells only; its `Buffer` / `Editor` layout was not used because text must sit on the terminal's whole-pixel cell grid | the UI sets text one character per column like the terminal; no proportional font, no kerning, no bidi in the UI | <https://docs.rs/cosmic-text/0.19.0/cosmic_text/> |
| 9 | (own `gfx::atlas`) | one atlas = one font size | UI text uses the terminal's font size and zooms with it; a second size needs a second atlas texture or a size in the glyph call | `spikes/s2-wgpu/src/gfx/atlas.rs` |
| 10 | arboard 3.6.1 | Wayland clipboard needs the `wayland-data-control` feature (a wlroots / KDE protocol) | feature enabled; not tried on a compositor without that protocol (GNOME) | <https://docs.rs/arboard/3.6.1/arboard/> |

Not an upstream problem but seen in the headless runs: sway 1.7 reports integer scale 2 for an output
scale of 1.5, so a fractional scale factor has never reached this app through a real compositor — layout
and atlas at 1.25 are tested offscreen only. Item 1.7 of the checklist is the first real check.

## 4. Not built

- No sessions behind the list: items are static sample names; Create appends a name and nothing else;
  Enter on an item only moves the focus to the terminal.
- "Clear" sends Ctrl+L to the program (`ui::menu::clear_key`). `s2-core` has no message that clears the
  emulator's screen or scrollback, and adding one was outside this task. In a program that ignores Ctrl+L
  nothing happens.
- No clipping (see 3.3 #7), no scrollbar on the list, no tooltip, no disabled state (Copy is offered
  without a selection and then does nothing), no hover highlight except in the menu.
- Text field: single line; no undo; no word-wise movement (Ctrl+←/→), no double-click word selection,
  no caret blink; the caret cannot be moved with the mouse while composing (a click in the field is
  ignored until the composition ends); no right-to-left text.
- IME: the preedit's own cursor / segment highlighting is not drawn; no IME in the list.
- Buttons are not focus stops (the ring is list ↔ field ↔ terminal, as specified): keyboard users
  press Enter / Escape in the field.
- Zoom: no Ctrl+0 reset, no persistence; UI and terminal zoom together (3.3 #9).
- Themes: one dark set of tokens (`UiTheme::default()`); no light theme, no loading from a file, no
  runtime switch. The terminal's hollow cursor when it is not focused is still not drawn.
- Accessibility (screen reader tree, high contrast), touch, drag and drop: nothing.
- Windows (TSF) and macOS: not built, not tried. X11: compiled in, not tried.
- Not exercised in any window so far: pointer (clicks, drags, wheel, the context menu), any input
  method, a real GPU, vsync, fractional scale from a compositor, a second monitor.
