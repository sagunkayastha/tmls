# tmls-rs — real-window review sheet (about 15 minutes)

Written 2026-10-11 for the review of branch `feat/viewer-panel` (plan
`docs/superpowers/plans/2026-10-11-tmls-rs-review-and-right-panel.md`, Task 5). Nothing below has been
run in a real window by the agents; all of it is unit- and headless-tested only. Done on the PC, in the
order a person meets things. Mark each line `ok` / `not ok` / `note`, and for `not ok` write what you
did and what you saw — every `not ok` becomes a row of `docs/reviews/2026-10-review.md` in your words.

Start it from a terminal you keep open (`~/Others/tmls-rs/target-docker/release/tmls-rs 2>tmls-rs.err`)
so stderr lines can be read afterwards. Use a throw-away session for rename / kill.

Keys: Alt+Shift+Right / Left pinned sessions (or tabs); Alt+Shift+Up list; Esc back; Alt+Shift+W close
tab; Alt+Shift+X close panel; Alt+Shift+F the panel's image list; Ctrl+= / Ctrl+− font size; Ctrl+Q quit.

## 1. Start
- [ ] The window opens, drawn at once, no stderr line but `tmls-rs: adapter …`: ____
- [ ] The list has the keyboard; the window's size and position are sane on the KDE desktop: ____
- [ ] Font size and scale look like tmls in kitty (write the scale KDE reports, e.g. 1.15): ____

## 2. The list
- [ ] Both hosts, same sessions and marks as tmls: ____
- [ ] Alt+Shift+Left / Right walks the pinned (★) sessions in order and wraps: ____
- [ ] Drag the list's right edge: it follows the pointer, stops at its minimum, the terminal reflows: ____
- [ ] Restart the app: the list's width is what you dragged (`~/.config/tmls-rs/ui`): ____
- [ ] This machine's Claude sessions outside tmux show a `not in tmux` row: ____
- [ ] Enter on a `not in tmux` row brings its kitty window to the front: ____

## 3. Tabs
- [ ] Attach two sessions; the tab shows the name; the list cursor follows the front tab: ____
- [ ] Click a tab, click its `×`; Alt+Shift+W; Enter on an ended tab reattaches: ____
- [ ] More tabs than fit: nothing drawn over the `Files` label at the bar's right end: ____

## 4. Foot buttons
- [ ] `New` opens the form for the host under the cursor; Esc closes it: ____
- [ ] `Open` opens a kitty window on the cursor's session: ____
- [ ] `Copy` says it copied; paste the attach command somewhere: ____
- [ ] `Sketch` opens the sketchpad (only if a hub is configured): ____
- [ ] `✕` closes the front tab; `?` and `!` do what the label says or say they do nothing yet: ____
- [ ] With KDE's auto-hide taskbar: the buttons' row is fully visible and clickable: ____

## 5. Mouse in the terminal
- [ ] Click selects a tmux pane (make a split first with `Ctrl+b %`): ____
- [ ] Drag with tmux mouse on resizes the split; the wheel scrolls history: ____
- [ ] Wheel in `less` / `vim` moves their view, not tmux's history: ____

## 6. Copy and paste
- [ ] Shift+drag selects text locally (highlight visible); on release it is on the clipboard: ____
- [ ] Ctrl+C with a selection copies and does not interrupt; without one it interrupts: ____
- [ ] Ctrl+Shift+C copies the selection; Ctrl+Shift+V pastes into the terminal: ____
- [ ] Paste into a form field (Ctrl+Shift+V in `New`'s name field): ____
- [ ] Any line on the launching terminal's stderr during 6 (copy the line here): ____

## 7. Links
- [ ] `echo https://example.com` then Ctrl+click: the browser opens it: ____
- [ ] Ctrl+click on a `.pdf` or `.md` path: the native app opens it, the status line says so: ____
- [ ] Ctrl+click on a path with no file behind it: the status line says so, nothing opens: ____

## 8. Text panel
- [ ] Ctrl+click on a `.txt` / `.rs` / `.py` path opens it beside the terminal with line numbers: ____
- [ ] The pane got narrower and tmux reflowed; the wheel over the panel scrolls it: ____
- [ ] `Open in app` opens it in the native editor; `×` or Alt+Shift+X closes the panel: ____
- [ ] A file over 2 MB: the status line says so, no panel: ____

## 9. Image panel
- [ ] Ctrl+click on a `.png` path (a plot): it shows, fitted, aspect kept, header has `W×H · N %`: ____
- [ ] Click the image: 100 %; wheel scrolls it, Shift+wheel sideways; click again: fit: ____
- [ ] `.jpg`, `.gif`, `.webp`, `.bmp` open; `.svg` goes to the native app: ____
- [ ] A huge image (over 25 MB, or 20000×20000): the status line says why, no freeze: ____
- [ ] A text file named `.png`: "not an image" in the status line: ____
- [ ] Ctrl+= / Ctrl+− with an image open: the image keeps its aspect, rows stay aligned: ____
- [ ] Resize the window with the panel open: the image refits, no stretching: ____

## 10. Divider
- [ ] Drag the divider: the cursor is a column-resize cursor, the terminal reflows live: ____
- [ ] Drag it to the far left and far right: both sides keep 20 columns: ____
- [ ] Restart the app, open a panel: the width is what you dragged: ____
- [ ] Make the window narrow (below 40 columns right of the list): the panel does not open: ____

## 11. Made-here list
- [ ] Alt+Shift+F (or the `Files` label) in a Claude tab: the list of the session's images, newest first: ____
- [ ] Each row: time (or date), file name, folder cut at its start: ____
- [ ] Click a row: the image opens; `‹` / Backspace / Esc returns to the list: ____
- [ ] `Refresh` reloads; make a new plot in the session: the list updates when the turn ends: ____
- [ ] A tab with no Claude session: `no Claude session in this tab`: ____
- [ ] A Claude session that made no image yet: `no images from this session yet`: ____
- [ ] Alt+Shift+F again closes the panel: ____

## 12. A session on archbox — the last four again
- [ ] Text panel on an archbox file (time to open): ____
- [ ] Image panel on an archbox plot (time to open): ____
- [ ] Divider drag with an archbox tab in front: ____
- [ ] Made-here list of an archbox Claude session (time to load): ____
- [ ] Pull the network (or sleep the laptop link) while an image loads: the window keeps drawing and
      typing; the status line reports the timeout within 60 s: ____

## 13. Alerts and notify-send
- [ ] A session goes `done`: the bell counts, one desktop notification (two if tmls is open): ____
- [ ] `a` in the list or a click on the bell lists the alerts; Enter jumps to the session: ____

## 14. fcitx5
- [ ] Preedit at the cursor in the terminal; `cat` echoes 你好 once: ____
- [ ] Emoji in colour or in outline (write which): ____

## 15. Rename and kill — on a throw-away session
- [ ] `m` → Rename: the new name is in the list and on the tab: ____
- [ ] `m` → Kill…: Enter alone does not kill; Tab → Enter kills; the keys stay in the list: ____

## 16. Typing feel
- [ ] Type a line in a busy shell beside kitty + tmls: same, better, worse (one word): ____
- [ ] The same with the image panel open: ____
- [ ] The same with the made-here list open: ____

## Anything else
- [ ] What bothered you that is not on this sheet: ____
