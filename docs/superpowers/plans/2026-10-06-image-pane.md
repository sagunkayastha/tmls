# Image side pane in the tmls TUI (PC first, web later)

Written 2026-10-06 by the BAR session (archbox) for the `tmls-image` session. User approved the idea;
**this plan is the brief — build it, then show the user before committing.**

## Goal

Clicking an image path in a tmls terminal opens the image in a side pane next to the terminal — like
the existing text `FileViewer` — instead of only handing it to Gwenview. Real pixels via kitty's
graphics protocol (tmls on the PC runs in kitty). Scope now: **PC TUI only**. The web version is a
separate later plan (see the end).

```
┌ tmls ──────────┬──────────────────────────┬──────────────────────────┐
│ sessions       │ terminal                 │ archbox:/data/…/bar.png  │
│                │  …/data/…/bar.png ← click│ [Open in app] [Save] [×] │
│                │                          │ ┌──────────────────────┐ │
│                │                          │ │  image, fit to pane  │ │
│                │                          │ └──────────────────────┘ │
└────────────────┴──────────────────────────┴──────────────────────────┘
```

## Starting point (already in the working tree, NOT committed yet)

The BAR session added bare-path links (uncommitted on `main` in `~/Others/tmls`; same files copied
into the PC's editable checkout `pcl:~/Others/P_projects/tmls`). **Keep these changes; build on them.**

- `term.py`: `BARE_PATH` — `/abs/path` and `~/path` without `:line` are links (`link_at` → `("file", path, None)`).
- `viewer.py`: `OUTSIDE` (image/pdf/media/archive/office extensions), `opens_outside(path, is_dir)`,
  `outside_url(host, path)` (local path, or `sftp://<machine><quoted path>`), `locate(host, session, path)`
  → `(abs path, is_dir)` via the session's machine (`hosts.run_argv`).
- `app.py` `on_terminal_link_clicked`: bare path or outside-type → `locate`; outside types and folders →
  `open_url(outside_url(...))` (xdg-open; KDE apps read sftp in place, verified PC→archbox, key auth).
  Text → existing `FileViewer` (`event.line or 1`).
- `quickselect.py`: bare paths from `link_at` keep line 1.
- Tests added in `tests/test_term.py`, `tests/test_viewer.py`, `tests/test_app.py` (all pass).
- Pre-existing failures on clean `main`, not ours: `test_ag::test_resolve_prefers_claude_names...`,
  `test_manage::test_window_and_pane_actions_keep_the_active_folder`, two `test_term::test_right_click_paste*`.

## Design

- **Which files:** raster images only — `png jpg jpeg gif webp bmp tif tiff ico avif` (not svg/heic at
  first; fall back to "Open in app"). PDFs, video, folders keep going to the desktop app (no change).
- **New widget `ImageViewer`** in `viewer.py` (or `image_viewer.py`), mounted where `FileViewer` mounts
  (`#workspace`, same CSS slot: `width: 50%; border-left`). Only one side pane at a time: opening either
  viewer removes the other. Esc / × close it and refocus the terminal (mirror `FileViewer.Closed`).
- **Header:** `host:path`, buttons **Open in app** (`open_url(outside_url(host, path))`),
  **Save** (copy to `~/Downloads/<name>` on the PC, notify the path), **×**.
- **Rendering:** add dependency `textual-image` (renders via kitty TGP / sixel, falls back to
  half-blocks). Check its current version/API first and pin a lower bound in `pyproject.toml`.
  Fit image to pane, keep aspect ratio; redraw on resize.
- **Loading bytes:** new `async load_image(host, session, path)` in `viewer.py`: local → read file;
  remote → `cat` over `hosts.run_argv` (quote like `load_file`; never interpolate raw screen text).
  Size cap ~25 MB (`ViewerError("image too large")`); decode with Pillow (comes with textual-image —
  verify) and reject non-images with `ViewerError("not an image")`. Off the UI thread (await subprocess).
- **App wiring** (`on_terminal_link_clicked`): after `locate`, if `is_image(path)` → load bytes → mount
  `ImageViewer`; on `ViewerError` → notify, and fall back to `open_url(outside_url(...))`. Other outside
  types unchanged. Quick-select (Alt+Shift+S, Shift+letter) on an image follows the same path.
- **No new network service.** Bytes come over the ssh connection tmls already uses.

## Tasks (TDD: write each test first, watch it fail, then implement)

1. `is_image(path)` + tests (case-insensitive extensions; svg/pdf → False).
2. `load_image` + tests: local read; remote via a fake `ssh` on PATH (copy the pattern in
   `tests/test_viewer.py::test_remote_file_with_spaces_and_semicolon_uses_ssh_safely`, incl. an
   injection-looking filename); size cap; non-image bytes rejected.
3. `ImageViewer` widget + test with `app.run_test`: mounts in `#workspace`, title shows host:path,
   Esc closes and focuses terminal, × closes, only one side pane (opening text viewer replaces it and vice versa).
4. App wiring + tests (monkeypatch `viewer.locate`/`viewer.load_image`/`open_url` like the existing
   `test_bare_image_path_opens_on_its_machine_over_sftp`; **update that test**: images now open in the
   pane, PDFs still go to `open_url`). Fallback test: `load_image` raises → `open_url` called + notify.
5. Save button test (writes into a tmp "Downloads" dir via monkeypatched home).
6. Full suite: only the 4 pre-existing failures may remain.

## Verify on the PC (manual, with the user)

- Deploy without committing: copy changed `src/` files to `pcl:~/Others/P_projects/tmls/` (editable uv
  tool install). New dependency: install it into the tool env on the PC
  (`uv tool install --editable ~/Others/P_projects/tmls --reinstall` or equivalent) — check the running
  tmls imports it before telling the user.
- The user restarts tmls (Quit button) — do not kill their running tmls yourself.
- Test list for the user: `/data/scratch/streamtest/mock/offscreen-stream.png` (pane),
  `/data/scratch/streamtest/mock/bar_pill2_left.png` (wide strip — check fit),
  `/data/scratch/cnt/alex.pdf` (still Okular), `/data/scratch/streamtest/mock` (Dolphin),
  `/data/scratch/streamtest/SETUPS.md:81` (text viewer), a missing `.png` (notify).
- Watch for kitty-image flicker when switching sessions/tabs or resizing; note it to the user.

## Rules

- Commit/push only after the user has tried it and says so (then one commit for the bare-path links +
  one for the image pane; no Claude attribution lines; author sgkayast <sgn.kayastha@gmail.com>).
- Do not delete or move files without the user's OK. Temp files go under `/data/scratch/…`, never `/tmp`.
- Never test widgets against the live Plasma desktop; tmls tests run headless (`app.run_test`).

## Later (separate plan): web

Browser can show images/PDFs natively: authenticated route on the tmls web server that streams a file
from the session's machine + bare-path link provider in xterm.js + side pane (`<img>` / PDF viewer;
full-screen sheet on phones, swipe down to close). Same buttons. Not part of this task.
