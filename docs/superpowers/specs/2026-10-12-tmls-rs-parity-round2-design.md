# tmls-rs: parity round 2 — Design

Date 2026-10-12. Follows `2026-10-12-tmls-rs-attention-loop-design.md` (round 1, F1–F8). Branch
`feat/parity-round2`, from `feat/attention-loop` once round 1's Task 10 is committed. Unattended; the user asked for
"the plan for all remaining" after an Opus comparison of Python tmls (`feat/native-attach`, run daily on the PC) and
tmls-rs. The goal: after this round the user stops launching the Python `tmls` sidebar; `ag`, `ct`, `tmls hooks`,
`tmls-web` stay as they are (not replaced here).

## Intent

Everything a daily tmls user still misses in tmls-rs, ranked by how much they would notice (comparison of 2026-10-12):

| # | Feature | tmls reference | Size |
|---|---|---|---|
| G1 | Row menu: new window, rename window, split side-by-side / top-bottom, kill pane (with confirm, warns on last pane) | `manage.py`, `tmux_ops.py` | hours |
| G2 | Game-mode awareness: `game-mode status` polled every 30 s on the local machine; while `on`, listing period 300 s for remote hosts (local stays 5 s), push tails unchanged, no notify-send sounds; host header shows `· game` dimly | branch `app.py:549-551,654-679` | hours |
| G3 | Quick select `Alt+Shift+S`: labels `a`–`z` drawn over every URL, path and git hash visible in the front tab; a letter copies the target to the clipboard, Shift+letter opens it (same routing as Ctrl+click: panel / native app / browser); Esc cancels; more than 26 targets → the first 26 from the bottom | `quickselect.py`, branch `kitty/hints.py` | 1 day |
| G4 | Image paste `Alt+Shift+V`: the clipboard's image (`wl-paste --list-types` has `image/png` or `image/jpeg`) is written to `~/.cache/tmls-rs/paste/<utc-stamp>.<ext>` locally, copied to the session's host under `~/.cache/tmls/images/` (scp over `ssh_base`; local host: moved, not copied), and the remote path is typed into the session as bracketed-paste text (`tmux::send_text_script`, without the trailing Enter). Status line says `pasted image → PATH` or why not (`clipboard has no image`, `copy to HOST failed: …`) | `image_paste.py` | 1 day |
| G5 | Background tmux servers: a collapsed section `background ▸ · N` per host listing sessions of other tmux servers (`tmux -L NAME` sockets under `$TMUX_TMPDIR`/`/tmp/tmux-UID/`); `b` in the list toggles it; rows attach with `tmux -L NAME attach`; host form `HOST#NAME` in `SessionKey.host`; rescanned every 6th poll | `app.py:23,148-158` | ½–1 day |
| G6 | Sign-in row: a host whose listing says `sign-in required` shows `⚠ sign in` instead of `· offline`; Enter / click on it opens a terminal (`terminal_argv`) running `ssh HOST` so the user can answer the prompt; the next poll clears it | branch `app.py:257` | hours |
| G7 | Alert sound and in-app switches: `notifications.json` keys `desktop`, `sound`, `silence_focused` are read AND written; the alerts list gets three toggles at its top (Space flips the one under the cursor); sound = `canberra-gtk-play -i message` if present on PATH, else silently off; defaults as tmls (desktop on, sound off, silence on) | `notifications.py`, `app.py:607-651` | hours |
| G8 | "Recent" prompts in the Ask form: the last 10 user lines from the session's Claude transcript, under the saved prompts, loaded off-thread when the form opens | `prompts.recent_argv`, `parse_recent` | hours |

Not in this round (standing decisions): spinner/pulse (D7 of round 1: no animation), web/Android, native sketch board,
`ag`/`ct` ports.

## Architecture

- `client-core`: `tmux.rs` gains argv builders `new_window_argv`, `rename_window_argv`, `split_argv(dir)`,
  `kill_pane_argv`, `pane_count_argv`, `server_sockets_argv`, `attach_argv` with a `-L` socket, `scp_argv(host, from, to)`;
  `listing.rs` parses the background section (`background NAME:` lines) and `sign-in required` becomes
  `HostSnapshot.sign_in: bool`; new `quickselect.rs` (pure: `targets(rows: &[String]) -> Vec<Target{row,col,len,kind,text}>`,
  `label(i) -> char`), new `gamemode.rs` (`status_argv`, `parse`), `prompts.rs` (`recent_argv`, `parse_recent`),
  `imagepaste.rs` (`pick_type(list) -> Option<&str>`, `stamp_name(now, ext)`). All pure functions unit-tested; nothing
  touches a real tmux or host in tests.
- `desktop`: `forms.rs` gains menu items (G1) with their confirm forms, `Form::Ask` gets a `recent` list (G8),
  `alerts.rs` gets the toggles (G7); new `select.rs` (overlay state for G3: labels + key handling, drawn as cells on top
  of the terminal grid only while active); `app.rs` wiring: game-mode timer (G2), `Alt+Shift+S/V`, sign-in click (G6),
  background toggle `b` (G5). `draw.rs`: label cells (G3), `background ▸ · N` rows (G5), `⚠ sign in` (G6), `· game` (G2).
- Threads/processes: one `game-mode status` child every 30 s (local only; skipped if the binary is not on PATH — then
  G2 is off); `wl-paste` and `scp` for G4 run off the UI thread (a `UserEvent::ImagePasted(Result)`); G8's transcript
  grep runs off-thread. Nothing new on the key-to-frame path; the G3 overlay adds cells only while it is open.
- Errors → `App::say(Err(..))`, never a panic; a missing `canberra-gtk-play`, `wl-paste`, `game-mode` or `scp` is a
  status-line sentence once, then the feature stays off.

## Testing

Per task: TDD unit tests on the pure functions and form/overlay state machines (desktop tests in Docker, client-core on
archbox), clippy `-D warnings`, `cargo fmt -p desktop --check`; one e2e block per feature in `tooling/smoke/e2e.sh`
(G1: split then `tmux list-panes` shows 2; G2: a fake `game-mode` script on PATH returning `on`, header shows `· game`;
G3: labels appear over a typed URL and `a` copies it (`wl-paste` checks); G4: `wl-copy -t image/png < fixture.png`,
Alt+Shift+V, the session's pane shows the typed path and the file exists; G5: a second `tmux -L bg` server with one
session appears under `background`; G6: a stub `ssh` on PATH that prints `Permission denied` → `⚠ sign in`; G7: Space on
the sound toggle writes `notifications.json`; G8: a fake transcript with user lines → the Ask form lists them). The suite
runs three times at the end; bench `BENCH_STATE=closed` before/after must not move. Hand-off sheet
`docs/NIGHT-TEST-2.md`.

## Decisions (veto any)

| # | Decision | Why |
|---|---|---|
| R1 | Game mode slows only remote hosts (300 s) and keeps the local 5 s poll | the local poll is cheap; the remote ssh is what woke the GPU box's network |
| R2 | Quick select: copy on letter, open on Shift+letter (tmls's rule); 26 targets max, nearest the bottom | one hand, no second key; the bottom is where the newest output is |
| R3 | Image paste files go to `~/.cache/tmls/images/` on the host, like tmls, so both apps share one place | the user's sessions already know that path |
| R4 | Background servers are read-only in v1: attach only — no new/rename/kill on them | tmls's rule too |
| R5 | Sign-in opens a terminal rather than a form in the app | ssh prompts (MFA, host key) need a real tty |
| R6 | Sound via `canberra-gtk-play` only (what tmls uses); absent → off | no new crate for audio |
| R7 | Recent prompts: last 10, loaded off-thread, listed under Saved | same as tmls |
| R8 | Task order: G1 → G2 → G6 → G7 → G8 → G5 → G3 → G4 (cheap and independent first; G3/G4 last, biggest) | a half-finished night still ships the small ones |
