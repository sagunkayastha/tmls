# Night of 2026-10-12: what to try (tmls-rs attention loop)

Verified commit: `745ba5b`. **The PC build of it is NOT done** — the PC was unreachable (asleep/off, LAN and
Tailscale) at the end of the night; say "build it" and I build + restart you. The PC's current build is `0c5cb73`
(Tasks 1–6 + detach + icon) — approvals and push status work there, but send/queue (`s`), the F8 fixes and the
final fix wave (the critical "queued text into a bash shell" guard) are only in `745ba5b`. Do not use `s` on the
`0c5cb73` build.
Start it as usual with `tmls-rs`. Each line: ok / not ok / a note. Screenshots of the headless
run are under `/data/scratch/tmls-rs-v0/night2/e2e-3/`.

## What changed since last night
- `?` opens a keys panel (also the `?` button, and `tmls-rs keys` on the command line).
- Copy and paste say what they did in the status line; `wl-copy`/`wl-paste` is the fallback.
- ssh: keepalive (15 s x 3) and one shared connection per host (ControlMaster).
- `•` marks unread sessions; Alt+Shift+N visits them when nothing waits.
- Approve a permission prompt from the list, the alerts or the row menu (`y`).
- Status is pushed from tmls's hooks (`events.jsonl`): `?` within a fraction of a second, and the
  last reply shows under the row.
- Send a message to a Claude session (`s`, Alt+Shift+A, menu); queued while it works (`N✉`).
- A failed new session reopens the form; a tab follows a session renamed outside the app.
- `tmls-rs` returns the shell at once (logs to `~/.cache/tmls-rs/tmls-rs.log`); it has an icon.

## Start and icon
- [ ] Start `tmls-rs` from kitty: the prompt comes back at once and the window stays; closing kitty leaves the window up.
- [ ] After `tooling/desktop/install.sh` on the PC, the task bar and the launcher show the icon (Rem). If it is stale: log out and in once.

## Look
- [ ] `?` in the list (or the `?` button): the keys panel; Esc closes it. Anything missing or wrong?
- [ ] `tmls-rs keys` in a terminal prints the same table.
- [ ] A session with output you have not looked at has a `•` after its name; it goes when you look at its tab. The legend says `• unread`.
- [ ] With nothing waiting, Alt+Shift+N (or `n`) visits an unread session.
- [ ] A host without `~/.cache/tmls/events.jsonl` says `· poll` beside its name.

## Copy and paste (reported broken on KDE; never run in a real window)
The headless sway could not reproduce the failure, and the clipboard library may report success
while the clipboard stays unchanged. Check every copy against `wl-paste` in a terminal.
- [ ] Shift+drag over text in a tab: the status line says `copied N chars`; `wl-paste` shows that text.
- [ ] Click without selecting, then Ctrl+Shift+C: `nothing selected`.
- [ ] Ctrl+Shift+C on a selection, then paste in another app: the text arrives.
- [ ] Ctrl+Shift+V in a tab: `pasted N chars`, the text arrives. With an empty clipboard: `clipboard empty`.
- [ ] Ctrl+Shift+V in a form field: the text arrives and the status line reports it.
- [ ] If any step fails: the status line's words (`copy failed: …` / `paste failed: …`) and the last lines of `~/.cache/tmls-rs/tmls-rs.log`.

## Act from the list
- [ ] A Claude asking permission: `y` on its row (or Approve… in its right-click menu, or `y` in the alerts) shows the dialog; Enter approves (Yes), `n` declines (No), Esc does nothing.
- [ ] Answer a prompt in the terminal first, then press Enter in the form: the status line says the session isn't asking that any more, and nothing is typed.
- [ ] Open the approval form, then type into the session before it appears: the late form is dropped (`dropped — keys moved on`).
- [ ] `s` on an idle Claude row: send `What's the progress?`; it arrives as typed.
- [ ] `s` on a working or waiting Claude row: the row shows `1✉`, the status line says `queued for NAME (1 waiting)`, the legend says `✉ queued`; the message arrives when the turn ends, one per turn.
- [ ] A message that cannot be delivered stays queued (`…; 1 queued message(s) kept`); Clear queue (in the menu) empties it.
- [ ] Alt+Shift+A in a Claude tab, and Send message… in the right-click menu, open the same form.
- [ ] A line you add to `~/.config/tmls/prompts` shows in the form (Up / Down).

## Faster status (needs `tmls hooks install` done on the host)
- [ ] A permission prompt shows `?` and notifies within a second, not five.
- [ ] `notify-send` on a pushed permission prompt: the desktop notification appears (never run in a real window).
- [ ] When a turn ends, the row's dim line shows the first line of the reply.

## Links that sleep
- [ ] Suspend a remote host (or pull its network) with its tab open: within ~45 s (15 s x 3) the tab says ended; Enter reattaches when it is back. Never run for real.
- [ ] Opening a remote tab feels quicker than before (shared ssh connection; sockets under `$XDG_RUNTIME_DIR/tmls-rs/cm-*`). With `TMLS_RS_NO_CONTROLMASTER=1` there are none.

## Small fixes
- [ ] New session in a folder that does not exist: the form comes back with what you typed and the reason, once.
- [ ] Rename a Claude session from a shell (`tmux rename-session`): its tab follows after Claude's next status change (only when it is unambiguous).

## Decisions to veto (spec D1–D10)
Hooks stay tmls's; push is `tail -F`; poll is the truth; Yes=`1`, No=Esc; saved prompts only; queue in memory; `•` without animation; ControlMaster on; keys `y` `s` `?` Alt+Shift+A; last reply only with push.
