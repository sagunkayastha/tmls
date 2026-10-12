# tmls-rs — morning test (about 15 minutes)

Written 2026-10-11 after the overnight build. `tmls-rs` is the new native window: your tmux sessions of
every host in a tmls-style sidebar, each attached session in a tab. Everything below is done on the PC.
Fill the blanks. If a step fails, write what you did and what you saw.

Full description and all key bindings: `docs/desktop-v0.md`.

## Before you start — three things to know

1. **Start it from a terminal you keep open**, and use a throw-away session for new / rename / kill
   (step 6 makes one). Kill and rename act on real tmux sessions.
2. **tmls and tmls-rs share `~/.config/tmls/pins`.** A pin you set in tmls-rs is written with the pin set
   tmls-rs read at its start; pins changed in tmls after that are dropped by the next `p` in tmls-rs.
   With both apps open you also get every desktop notification twice.
3. **Never run for real, only in the headless test or in unit tests:** mouse click and drag inside the
   terminal, Ctrl+click on a link, Ctrl+C copy, "Open in new terminal", the clipboard, real
   `notify-send`, fcitx5. They may be broken. Typing, tabs, the list, forms and the wheel did run.

## 1. Build and start

```sh
cd ~/Others/tmls-rs && git fetch && git checkout --detach origin/main
git rev-parse --short HEAD
tooling/docker/build.sh build --release -p desktop
ldd target-docker/release/tmls-rs | grep "not found"     # must print nothing
~/Others/tmls-rs/target-docker/release/tmls-rs
```

- Hash printed: ________
- `ldd` printed nothing: ________
- A window opens; the list on the left has the keyboard: ________

## 2. The list

- Both hosts are there, each under its own header, with the same sessions as tmls shows: ________
- Marks are right (`●` running, `?` waiting, `◆` done, `✕` error, `○` idle): ________
- Claude sessions show a `C`, a title line under the name and a context %: ________
- Your pins (`★`) are the same as in tmls: ________
- Does it look like tmls? What is off (colours, spacing, font size, anything): ________

## 3. Attach and type

Click a session (or `j` / `k`, then Enter).

- It attaches in a tab; the tab shows the session's name: ________
- Typing appears at once; Tab completes; Ctrl+C interrupts (`sleep 100`, Ctrl+C): ________
- How does typing feel next to kitty + tmls? ________
- `printf 'ab你好cd\n😀 ok\n'` — Chinese placed right: ________ · emoji in colour: ________
  (Overnight on the PC the emoji was a one-colour outline: `/data/scratch/tmls-rs-v0/pc/s2-wgpu-ui-crop.png`
  on archbox.)

## 4. Tabs

Attach a second session.

- Alt+Shift+Right / Left switch tabs; the list cursor follows the front tab: ________
- Alt+Shift+Up puts the keys in the list; Esc puts them back in the terminal: ________
- Alt+Shift+W (or the `×`) closes a tab; the session is still in the list: ________
- Alt+Shift+N opens the next session that waits for you (`?`), if there is one: ________

## 5. Mouse in the terminal (never run for real)

With tmux mouse mode on, in an attached tab:

- Wheel scrolls tmux's history: ________
- A click selects a tmux pane (make a split first): ________
- Shift+drag (or Ctrl+drag) selects text locally; on release it is on the clipboard: ________
- With text selected, Ctrl+C copies and does not interrupt: ________
- Ctrl+click on a URL (`echo https://example.com`) opens the browser: ________

## 6. New, rename, kill — on a throw-away session

- Click `+` on the PC's header (or `c` in the list). Folder `~`, name `throwaway`, Start `shell`, Create:
  a session `throwaway` appears and its tab opens: ________
- In the list, on `throwaway`: `m` (or right-click) → Rename → `throwaway2`: ________
- `m` → Kill…: the confirm names `throwaway2`; **Enter alone must not kill** (focus is on Cancel): ________
- `m` → Kill… → Tab → Enter: the session and its tab are gone; **the keys stay in the list**: ________
- `m` on any session → Copy attach command; paste it somewhere: ________
- `m` → Open in new terminal: a kitty window attached to that session: ________
- There is no `⋯` on the rows — the menu is right-click or `m`. Missed? ________

## 7. A session ends, a host goes away

- In a tab on a throw-away session type `exit`: the tab stays and says
  `session ended — Enter to reattach`; the other tabs still work: ________
- If you can, cut one host off for half a minute (e.g. a wrong name in `tmls-rs HOST`): its header says
  `· offline`, the window stays responsive, the other host keeps updating: ________

## 8. Alerts

Let a Claude session finish while another tab is in front.

- The bell at the foot of the list counts (`!1`): ________
- One desktop notification `tmls · HOST` / `◆ NAME · done` (two if tmls is also open): ________
- `a` in the list (or a click on the bell) lists the alerts; Enter jumps to the session: ________

## 9. fcitx5, once

In an attached tab with `cat` running, compose and commit 你好.

- Preedit at the cursor, `cat` echoes 你好 once: ________

## 10. Latency numbers (2 minutes, optional)

Nobody typed into the app on the PC overnight (see "Decisions" below), so there are no numbers yet.

```sh
mkdir -p ~/s2-logs
~/Others/tmls-rs/target-docker/release/tmls-rs --bench ~/s2-logs/tmlsrs-115 2> ~/s2-logs/tmlsrs-115.stderr
```

Attach **one** session first (only the first tab opened is logged), type about 200 letters and Enters in
it, then Ctrl+Q. Paste the analysis block from `docs/spikes/S2-checklist-wgpu.md` section 1.7 with its
first line changed to `for run in tmlsrs-115; do`.

- key → present p50 / p95 / p99: ________ (s2-wgpu alone on 2026-10-11: p50 0.56 ms)

---

## What was built

| Area | State |
|---|---|
| Sessions per host, offline hosts, status marks, Claude title and context % | built, run against your real hosts (listing only) |
| Tabs: attach, switch, close, reattach after the session ends | built, run headless |
| tmls-style sidebar: headers, pins, legend, buttons, keys | built, run headless and seen on the PC |
| New session form, rename, kill with confirm, row menu, Copy, Open in new terminal | built; Copy and Open never run for real |
| Mouse to tmux, local selection with Shift / Ctrl, Ctrl+C copy, links | built; only the wheel ran in a window |
| Alerts: bell count, list, desktop notification | built; real `notify-send` never ran |

Tests at the merged head: see the top entry of `PROGRESS.md` (unit and PTY tests for the four crates,
clippy, and the headless end-to-end run three times in a row).

## What was not built

`ag`, `ct`, approvals (Yes / No on a Claude prompt), Ask / queue, prompts, the file viewer, `file:line`
links, image paste, quick select, the splits menu, kitty (non-tmux) sessions, web, Android, Windows,
config migration. The worker / hostd / own-transport architecture of `docs/PLAN.md` §4–§5 is not started:
v0 is a thin client over ssh + tmux. The gpui comparison (Track B) is deferred.

Known gaps in what was built:

- A tab on a host whose network silently dropped looks alive but frozen (no ssh keepalive). Close it and
  attach again.
- A session renamed outside the app keeps its old tab label; reattach then fails. Close the tab.
- A failed "new session" closes the form; what you typed is lost. The status line says why it failed.
- After a rename the session's tab moves to the end of the tab bar.
- Forms take no fcitx5 input (paste works).
- `$TERMINAL` with arguments (e.g. `kitty -1`) does not work for "Open in new terminal".
- With more tabs than fit, the tab bar cannot be clicked (the keys still work).
- Emoji may be drawn without colour on the PC.
- About 300 lines of window code are copied from the `s2-wgpu` spike instead of shared.

## Decisions I took for you overnight — veto any of them

Each line: what I decided — why — what it costs if I was wrong.

Plan level (from the plan's own "Decisions taken for the user"):

1. "Finish tmls overnight" = desktop v0 over ssh + tmux, not PLAN phases 1a–7 — those cannot be built and
   gated in one night — the tab ↔ emulator wiring is replaced when workers exist; listing and tmux
   commands are reusable.
2. Toolkit wgpu, gpui comparison deferred — measured 0.56 ms key → present — `crates/desktop` UI code is
   redone if gpui wins.

During the night, in order:

3. An extra file `crates/client-core/examples/list.rs` (prints the listing) — two plan steps needed it —
   one small file.
4. Tests that use real tmux run on a private tmux server (own socket directory) and only with names
   starting `tmlsrs-test-` — your live server is never contacted by a test — none.
5. The plan file itself was committed on the branch — it was untracked — one doc file.
6. The archbox checkout was switched from `spike/s2-wgpu` to the work branch, no separate git worktree —
   the Docker build and the PC fetch use this checkout — your checkout is on a different branch than you
   left it (now `main`).
7. **Pins are stored as tmls's slugs only** — "write only `pins`, in tmls's format" — pins are shared
   both ways with tmls; a pin made in one shows in the other.
8. The poller ignores "seconds since last output" when it decides whether a host's list changed — else
   every host would redraw every 5 s — that age (not shown anywhere) can be stale.
9. Twice, two agents wrote in the checkout at the same time on different files — the 06:30 limit — none
   seen.
10. Task 5 was split so the first headless run of the window happened before the sidebar was built — the
    window had never run — none.
11. `--bench` logs only the first tab opened — key ids are per session — no latency numbers for later
    tabs.
12. The copied window code (see "Known gaps") was left — extracting it reshapes the latency-critical
    spike the night before your test, and the gpui decision may discard it — fixes in that code are made
    twice until it is shared.
13. "Exit path clean" in the plan was read as "the app exits cleanly"; a session ended by `exit` was added
    as its own check — the phrase was ambiguous — none.
14. No `⋯` on the rows; the menu is right-click or `m` — 28 columns are tight — you may miss the
    affordance.
15. Task 7's tests were written with the code, not before it, and no mutation check was asked for — the
    tests compare exact bytes read back by a real program — a test that cannot fail could go unnoticed.
16. The real-host sidebar screenshot inside the test container was skipped; only the listing ran against
    your hosts — the plan's variant mounts `~/.ssh` into a container — you see the real list in step 2
    instead.
17. **Alerts are kept across a host's offline spell** (tmls drops them) — otherwise the list shows `◆`
    the bell never counted — when a host comes back you may get one notification per changed session.
18. **No keys were typed into the app on the PC by a script** — a script types into whatever window has
    the focus, which could have been one of your running Claude sessions — no unattended latency numbers;
    step 10 above takes two minutes.
19. New list keys that tmls does not have: `c` (new session), `m` (row menu), `a` (alerts), and
    Alt+Shift+W (close tab) — the features needed a key — they are taken from the list / the terminal.
20. After the final review: keys stay in the list after a rename or kill started from the list (before
    the fix they jumped into the neighbouring tab's terminal a moment later) — typing list keys into an
    agent session is the worse failure — if you expected the focus to follow the new front tab, press
    Esc.

Things left on disk for you to remove or keep (nothing was deleted overnight):

- `crates/desktop/tests/tabs.rs.new` (untracked stray file), `tooling/smoke/__pycache__/` (ignored).
- `/data/scratch/tmls-rs-v0/` on archbox (screenshots, logs, test output).
- `~/offload/tmls-rs-v0/` on the PC (three short bench logs and two full-screen screenshots).
- Docker image `tmls-rs-smoke:latest` on archbox (3.45 GB, 0.32 GB of it new).
- `.superpowers/sdd/2026-10-11-tmls-rs-desktop-v0/` (the night's ledger, briefs, reports).
