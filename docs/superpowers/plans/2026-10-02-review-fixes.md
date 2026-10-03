# Review fixes + web "new session" — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the defects found in the 2026-10-02 review of tmls (TUI core + web), and add "new tmux session" to the web UI (`+` on a host header → modal, option C).

**Architecture:** Four workstreams on disjoint files, each on its own branch in its own git worktree, run in parallel by separate implementer subagents, then reviewed and merged by the controller: **A** `src/tmls/app.py` (TUI app), **B** `src/tmls/hosts.py`, `prompts.py`, `approve.py`, `local.py`, `tests/test_hosts.py` etc. (TUI core), **C** `src/tmls/web/*` (web fixes), **D** web "new session" (runs after C is merged, since it edits the same web files; also splits the Textual form out of `create.py`). One small post-merge task **E** wires B's new `pane` parameter into A's and C's call sites.

**Tech Stack:** Python 3.12, Textual 8, pyte, aiohttp, xterm.js 5.5 (CDN), pytest (+pytest-aiohttp), Playwright e2e (`tests/web/e2e_web.py`, run with system python; server half via `uv run`).

**Spec:** the review findings in this conversation (W1–W10 web, T1–T11 TUI) and `docs/superpowers/specs/2026-10-02-web-tmls-design.md` for the web app. Findings are restated in full inside each task, so this plan is self-contained.

## Global Constraints

- Run tests with `env -u NO_COLOR uv run pytest -q` from the worktree root; the whole suite (207 tests before this plan) must pass before every commit. Browser e2e: `python3 tests/web/e2e_web.py` (needs Chrome; 15 checks before this plan).
- Surgical changes in the existing style: short comments only for non-obvious reasons, no new abstractions, no new config options except where a task says so. Don't refactor neighbouring code.
- The repo is public: no IPs, real hostnames beyond `archbox`/`ubu`-style examples, tokens, or personal paths in committed files.
- Session names reach tmux as argv or `shlex.quote`d; never f-string a name into a shell command unquoted.
- Each workstream touches **only its own files** (listed per task). If you believe you must touch another workstream's file, stop and report `NEEDS_CONTEXT` instead.
- Commit per task with a clear message; never push; never merge.
- Test-first: write the failing test, run it to see it fail, implement, run it to see it pass, run the whole suite, commit.

## Review Focus

1. A session name with spaces, `'`, `"` or `$` must list, attach, rename, Ask and Copy correctly (A: Task A3 test; B: Task B5 test).
2. A host that hangs for 8 s must not stop the other hosts' rows from updating (A: Task A2 test).
3. A permission prompt containing `[`, `]`, `[/]` or `[b]` must display verbatim (A: Task A4 test).
4. With the browser's live feed down, the user must be able to tell the rows are stale (C: Task C3 e2e check).
5. Creating a session whose name already exists, or in a missing folder, must show the server's reason inside the modal and leave the modal open (D: Task D2 server test + D3 e2e check).

---

## Workstream A — TUI app (`src/tmls/app.py`, `tests/test_app.py`)

Branch `fix/tui-app`. Owns only `src/tmls/app.py` and `tests/test_app.py`.

### Task A1: Ask during a permission prompt must queue, not type into the dialog

**Finding T1:** `send_prompt` (`app.py` ≈ line 550) queues only when the mark is `running`. When the session shows `?` (mark `waiting`), `prompts.send` pastes the text and presses Enter, which picks the highlighted dialog option (usually "1. Yes").

**Files:**
- Modify: `src/tmls/app.py` (`send_prompt`)
- Test: `tests/test_app.py`

- [ ] **Step 1: Write the failing test.** Find the existing Ask/queue tests in `tests/test_app.py` (search for `Queued for`). Copy the pattern of the test that sends while `running` and make the session's mark `waiting` instead (a fake session with `claude="waiting"`, `waiting="permission prompt"`). Assert that `prompts.send` was **not** called, the message is in `app.queue[slug(host, name)]`, and the notification says `Queued for <name>`.
- [ ] **Step 2: Run it:** `env -u NO_COLOR uv run pytest tests/test_app.py -q -k <your test name>` → FAIL (send was called).
- [ ] **Step 3: Implement:** in `send_prompt`, change `if self.marks.get(key) == "running":` to `if self.marks.get(key) in ("running", "waiting"):`. Delivery already triggers on the transition to `done`/`idle` in `_draw_rows`, and a `?` → `done` transition satisfies `old not in {"done", "idle"}`.
- [ ] **Step 4: Run the test → PASS; run the whole suite → all pass.**
- [ ] **Step 5: Update README:** in the **Ask** bullet, change "While the session is working (●), Ask queues the message instead." to "While the session is working (●) or waiting on you (?), Ask queues the message instead." (README is shared, but only this task edits it in workstream A; keep the edit to that sentence.)
- [ ] **Step 6: Commit:** `fix(app): Ask queues while a session waits on a permission prompt instead of typing into the dialog`

### Task A2: A slow host must not freeze the whole session list

**Finding T2:** `refresh_sessions` runs `_refresh` with `exclusive=True` every 5 s; `_refresh` `gather`s every host; `hosts.list_host` allows up to 8 s per host. A host slower than 5 s means every refresh is cancelled before `self._results` is set: nothing updates and the slow host never shows offline.

**Files:**
- Modify: `src/tmls/app.py` (`refresh_sessions`, `_refresh`)
- Test: `tests/test_app.py`

- [ ] **Step 1: Write the failing test.** Using the `fake_hosts` fixture pattern in `tests/test_app.py` (look for how `hosts.list_host` is monkeypatched), make `list_host` for host `slow` sleep 0.7 s and for host `fast` return immediately. Set `app_module.REFRESH_SECONDS = 0.2` (monkeypatch) and run the app with a pilot for ~1.5 s. Assert `app._results` contains a `fast` entry (rows for `fast` were drawn) — before the fix, every refresh is cancelled and `_results` stays `[]`.
- [ ] **Step 2: Run it → FAIL.**
- [ ] **Step 3: Implement.** Replace the exclusive worker with a "skip if still running" guard:

```python
    def refresh_sessions(self):
        if self._refreshing:
            return  # a slow host is still answering; cancelling would throw away every host's result
        self._refreshing = True
        self.run_worker(self._refresh(), group="refresh")

    async def _refresh(self):
        try:
            ... existing body ...
        finally:
            self._refreshing = False
```

Initialise `self._refreshing = False` in `__init__` (next to `self._results`). Keep `group="refresh"`; drop `exclusive=True`.
- [ ] **Step 4: Run the test → PASS; whole suite → pass.**
- [ ] **Step 5: Commit:** `fix(app): a host slower than the refresh interval no longer cancels every refresh`

### Task A3: Session row IDs must be unique for distinct sessions

**Finding T3:** `slug(host, name)` (`app.py:24`) rewrites every char outside `[A-Za-z0-9_-]` to `_` and joins with `-`, so `my work`/`my_work`, or host `a`+session `b-c` vs host `a-b`+session `c`, collide → duplicate widget ID → `MountError` → the app exits. `seen`, `marks`, `queue`, `cursor` and tab ids are keyed the same way.

**Files:**
- Modify: `src/tmls/app.py` (`slug`)
- Test: `tests/test_app.py`

- [ ] **Step 1: Write the failing tests** (plain unit tests, no pilot):

```python
def test_slug_distinct_for_names_that_sanitize_alike():
    assert app_module.slug("box", "my work") != app_module.slug("box", "my_work")
    assert app_module.slug("a", "b-c") != app_module.slug("a-b", "c")

def test_slug_is_a_valid_textual_id_and_stable():
    s = app_module.slug("archbox", "Season 36 (v2)")
    assert re.fullmatch(r"[A-Za-z_][A-Za-z0-9_-]*", s)
    assert s == app_module.slug("archbox", "Season 36 (v2)")
```

Add a pilot test: two sessions `my work` and `my_work` on one fake host mount without error and both rows exist.
- [ ] **Step 2: Run → FAIL (first assertion).**
- [ ] **Step 3: Implement:**

```python
def slug(host, name):
    """A widget-id-safe key that stays unique: the readable part can collide ("my work" and
    "my_work"), so a short hash of the exact host and name is appended."""
    digest = hashlib.sha1(f"{host}\0{name}".encode()).hexdigest()[:8]
    return re.sub(r"[^A-Za-z0-9_-]", "_", f"{host}-{name}") + "-" + digest
```

Add `import hashlib`. Check every use of `slug(...)`: `f"add-{slug(host, '')}".rstrip("-")` and `f"host-{slug(h, '')}".rstrip("-")` still produce valid ids (the hash never ends in `-`); `tab.id.removeprefix("tab-")` keys still match `self.marks`. Grep tests for hard-coded ids like `#s-box-alpha` and update them to call `slug()`.
- [ ] **Step 4: Run tests → PASS; whole suite → pass.**
- [ ] **Step 5: Commit:** `fix(app): session widget ids are unique for names that sanitize alike`

### Task A4: Permission-prompt text is not Textual markup

**Finding T4:** `Approval.compose` (`app.py` ≈ 161) yields `Static("\n".join(self.shown[:8]), classes="request")`; text captured from the pane is parsed as markup, so `[/]` raises `MarkupError` when 🔔 opens, and `[id]` silently disappears.

**Files:**
- Modify: `src/tmls/app.py` (`Approval.compose`)
- Test: `tests/test_app.py`

- [ ] **Step 1: Write the failing test:** mount `Approval(session, ["Bash command", "sed 's/[/]/_/g' f", "src/[id]/page.tsx"])` in a small `App` (see how other widgets are mounted in existing tests) and assert the `.request` Static's `renderable`/text contains `[/]` and `[id]` verbatim and no exception is raised.
- [ ] **Step 2: Run → FAIL (MarkupError).**
- [ ] **Step 3: Implement:** `yield Static("\n".join(self.shown[:8]), classes="request", markup=False)`. Check `AlertLine` and the Ask panel's Recent/Saved lines for the same pattern (text from sessions or transcripts shown via `Static(...)` without `markup=False`) and fix those too.
- [ ] **Step 4: Run → PASS; whole suite → pass.**
- [ ] **Step 5: Commit:** `fix(app): text captured from sessions is shown verbatim, not parsed as markup`

### Task A5: Approve panel must skip kitty pseudo-host sessions

**Finding T13:** `_show_approvals` (`app.py` ≈ 511) includes `hosts.KITTY` sessions, so `approve.current("kitty", name)` runs `ssh kitty …`.

**Files:**
- Modify: `src/tmls/app.py` (`_show_approvals`)
- Test: `tests/test_app.py`

- [ ] **Step 1: Test:** with a fake kitty session marked `waiting`/`permission prompt` and `approve.current` monkeypatched to record its `host` argument, open the alerts panel; assert `approve.current` was not called with `hosts.KITTY`.
- [ ] **Step 2: FAIL. Step 3:** add `and s.host != hosts.KITTY` to the `asking` comprehension. **Step 4:** PASS + suite.
- [ ] **Step 5: Commit:** `fix(app): approve panel ignores kitty-only sessions (no tmux pane to read)`

---

## Workstream B — TUI core (`hosts.py`, `prompts.py`, `approve.py`, `local.py`, `ag.py`)

Branch `fix/tui-hosts`. Owns `src/tmls/hosts.py`, `src/tmls/prompts.py`, `src/tmls/approve.py`, `src/tmls/local.py`, `src/tmls/ag.py` and their tests (`tests/test_hosts.py`, `tests/test_prompts.py`, `tests/test_approve.py`, `tests/test_local.py`, `tests/test_ag.py`). README edits limited to the lines named below.

### Task B1: Unexpected host output marks the host offline instead of crashing

**Finding T5:** `hosts.parse` (`hosts.py:55-62`) does `now, *lines = …` and `line.rsplit(":", 3)` unguarded. A remote `.bashrc` that echoes to stdout makes `int()`/unpacking raise `ValueError`, which propagates out of `list_host` and kills the caller's refresh.

**Files:**
- Modify: `src/tmls/hosts.py` (`parse`, `list_host`)
- Test: `tests/test_hosts.py`

- [ ] **Step 1: Tests:**

```python
def test_parse_skips_lines_that_are_not_windows():
    out = "Welcome to the box\n1700000000\nwork:1:0:1699999990\nmotd line\n---\n"
    sessions = hosts.parse("box", out)
    assert [s.name for s in sessions] == ["work"]

async def test_list_host_offline_when_output_is_garbage(monkeypatch):
    # monkeypatch the subprocess the way existing list_host tests do, returning b"garbage\n"
    online, sessions = await hosts.list_host("box")
    assert (online, sessions) == (False, [])
```

(Read `list_host` at the bottom of `hosts.py` first; copy the monkeypatch style of the neighbouring tests.)
- [ ] **Step 2: FAIL. Step 3: Implement:** in `parse`, find the clock line as the first line that is all digits (skip anything before it); for window lines, `try: name, windows, attached, activity = line.rsplit(":", 3); int(windows); int(activity) except ValueError: continue`. In `list_host`, wrap `parse(...)` in `try/except ValueError: return False, []`.
- [ ] **Step 4: PASS + suite. Step 5: Commit:** `fix(hosts): stray output from a host no longer crashes the listing`

### Task B2: `failed`/`usage` lines attach only to the status file they follow

**Finding T6:** in `parse`, a `<pid>.json` caught mid-write fails `json.loads` and is skipped, but the `failed` and `usage` lines printed after it go to `files[-1]` — the previous session — giving it a false ✕, model and context %.

**Files:** Modify `src/tmls/hosts.py` (`parse`); Test `tests/test_hosts.py`.

- [ ] **Step 1: Test:** two status entries after `---`: a valid one for session A (`{"tmux":"A:@1.%1","status":"idle",...}`), then a truncated JSON line for B, then `failed`, then `usage "model":"claude-haiku-4-5" "input_tokens":190000`. Assert A has `failed is False`, `model is None`, `context == 0`.
- [ ] **Step 2: FAIL. Step 3: Implement:** keep a flag `skipping = False`; on a failed `json.loads` set `skipping = True` and `continue`; on success set `skipping = False`; for `failed`/`usage` lines, `if skipping or not files: continue`.
- [ ] **Step 4: PASS + suite. Step 5: Commit:** `fix(hosts): a half-written status file doesn't hand its ✕ and context to the previous session`

### Task B3: Messages and approvals target Claude's own pane

**Finding T7:** `prompts.send` and `approve.current/answer` target `=name:` (the window's *active* pane). After a split, the active pane may be a shell: a queued message is pasted + Enter'd into bash. Claude's status file records its pane (`"tmux":"Budget:@7.%7"`).

**Files:** Modify `src/tmls/hosts.py` (`Session`, `parse`), `src/tmls/prompts.py` (`send`, `recent_argv` unchanged), `src/tmls/approve.py` (`_capture_script`, `current`, `answer`), `src/tmls/ag.py` (callers of `prompts.send`/`approve`); Tests in `tests/test_hosts.py`, `tests/test_prompts.py`, `tests/test_approve.py`.

**Interfaces (Task E and workstream C rely on these exact signatures):**
- `hosts.Session.pane: str | None` — the `%N` pane id from the status file's `tmux` field, e.g. `"%7"`; `None` for non-Claude sessions.
- `prompts.send(host, name, text, pane=None)` — target is `pane` when given, else `=name:`.
- `approve.current(host, name, pane=None)` and `approve.answer(host, name, shown, yes, pane=None)` — same rule.

- [ ] **Step 1: Tests:** (a) `parse` sets `pane == "%7"` from `"tmux":"Budget:@7.%7"` and `None` when the field is missing/has no `%`. (b) `prompts.send("box", "work", "hi", pane="%7")` builds a script whose `-t` targets are `%7` (capture the argv via the existing monkeypatch of `prompts._run`); without `pane` the targets stay `=work:`. (c) same for `approve._capture_script(name, pane)` and `approve.answer(..., pane="%7")`.
- [ ] **Step 2: FAIL. Step 3: Implement:** in `parse`, `m = re.search(r"%\d+$", c.get("tmux") or ""); s.pane = m.group(0) if m else None` (set alongside `s.waiting, s.failed, s.title`). In `prompts.send`: `t = shlex.quote(pane or f"={name}:")`. In `approve`: `_capture_script(name, pane=None)` → `shlex.quote(pane or f"={name}:")`; thread `pane` through `current` and `answer`. `ag.py`: where it resolves a Claude session by name and calls `prompts.send`/`approve`, pass `session.pane` if a `Session` is at hand; otherwise leave the default.
- [ ] **Step 4: PASS + suite. Step 5: Commit:** `fix(prompts,approve): send keys to Claude's own pane, not the window's active pane`

### Task B4: One tmux paste buffer per send

**Finding T8:** `prompts.send` uses the shared buffer name `tmls-send` with `-d`; two deliveries in the same tick (two sessions finishing together, or `ag send` concurrently) can swap or lose text.

**Files:** Modify `src/tmls/prompts.py` (`send`); Test `tests/test_prompts.py`.

- [ ] **Step 1: Test:** call `send` twice (monkeypatched `_run` recording argv) and assert the two scripts use different `-b` names, both matching `tmls-[0-9a-f]+`.
- [ ] **Step 2: FAIL. Step 3:** `buf = f"tmls-{secrets.token_hex(4)}"` and use it for `load-buffer -b`, `paste-buffer -b` (`import secrets`).
- [ ] **Step 4: PASS + suite. Step 5: Commit:** `fix(prompts): each send uses its own tmux buffer so parallel sends can't swap messages`

### Task B5: Copy command is shell-safe; attach and listing are exact and UTF-8

**Findings T9/T10/T15:** `attach_command` (`hosts.py:144-147`) wraps a `shlex`-quoted remote command in double quotes: a name containing `"` breaks out and `$x` expands when pasted. `attach_argv` uses `-t name` (prefix match: a stale `work` row attaches `workshop`); every other tmux target uses `=`. `list_argv` lacks `-u`, so under a C locale `café` lists as `caf_` and never matches attach or status.

**Files:** Modify `src/tmls/hosts.py` (`attach_command`, `attach_argv`, `list_argv`); Test `tests/test_hosts.py` (update the existing attach_command test at ≈ line 120).

- [ ] **Step 1: Tests:**

```python
def test_attach_command_is_shell_safe():
    cmd = hosts.attach_command("nas", 'x"; touch /tmp/pwned; "')
    assert shlex.split(cmd) == hosts.attach_argv("nas", 'x"; touch /tmp/pwned; "')
    assert "$" not in hosts.attach_command("nas", "cost$5").replace("'cost$5'", "")

def test_attach_targets_exact_name():
    assert hosts.attach_argv(hosts.LOCAL, "work")[-1] == "=work"

def test_listing_forces_utf8():
    assert "tmux -u list-windows" in hosts.list_argv(hosts.LOCAL)[-1]
```

- [ ] **Step 2: FAIL. Step 3:** `attach_command` → `return shlex.join(argv)`; `attach_argv` → `["tmux", "-u", "attach", "-t", "=" + name]`; `list_argv` script → `tmux -u list-windows -a -F …`. Update the README **Copy** bullet example if its quoting changes (it shows `ssh -t nas "tmux attach -t work"`; make it match the new output, e.g. `ssh -t nas 'tmux -u attach -t =work'`).
- [ ] **Step 4: PASS + suite. Step 5: Commit:** `fix(hosts): shell-safe Copy command; exact-name attach; UTF-8 listing`

### Task B6: Dead code and stale comments

**Finding T11:** `local.py:116-122` is unreachable (after a `return`); the comment at `local.py:126` says tmls avoids threads but `image_paste.store` and `term._paste_input` use `asyncio.to_thread`; README says "131 tests" (there are 207); `ag read` leaks an `Ambiguous` traceback (`ag.py:129`).

**Files:** Modify `src/tmls/local.py`, `src/tmls/ag.py`, `README.md` (Development section only); Test `tests/test_ag.py`.

- [ ] **Step 1:** test that `ag read` with an ambiguous name prints a one-line error and exits non-zero (mirror the existing `Ambiguous` handling in `cmd_send`).
- [ ] **Step 2: FAIL. Step 3:** catch `Ambiguous` in `cmd_read` like `cmd_send` does; delete `local.py:116-122`; reword the `local.py:126` comment to what is true now; README: `uv run pytest        # 207 tests: …`.
- [ ] **Step 4: PASS + suite. Step 5: Commit:** `chore: drop unreachable code, fix stale comments, ag read handles ambiguous names`

---

## Workstream C — web fixes (`src/tmls/web/*`, `tests/web/*`)

Branch `fix/web`. Owns `src/tmls/web/**`, `tests/web/**`, and the README's web section (if one exists; otherwise add the one sentence named in C7 under **Limitations**).

### Task C1: No sidebar rebuild when nothing changed

**Finding W1:** `events.poll_once` (`events.py:45`) broadcasts `{"t":"rows", "set":[], "gone":[]}` every tick, and `app.js` (≈ line 212) calls `drawRows()` (`replaceChildren`) on every message — measured 6 rebuilds in 12 s with nothing changing. Effects: hover flicker, element refs die, and a click whose mousedown/mouseup straddle a rebuild is swallowed (observed once in testing).

**Files:** Modify `src/tmls/web/events.py` (`poll_once`), `src/tmls/web/static/app.js` (`drawRows`, `rowEl`, events `onmessage`); Tests `tests/web/test_events.py`, `tests/web/e2e_web.py`.

- [ ] **Step 1: Server test:** with a fake `list_host` that returns the same sessions twice and a recording fake socket in `app["sockets"]`, call `poll_once` twice; assert exactly one `rows` message was sent (the second tick's empty diff is skipped). Look at `tests/web/test_events.py` for the existing poll_once harness.
- [ ] **Step 2: FAIL. Step 3 (server):** in `poll_once`: `changed = rows.diff(old_rows, new)`; `if changed["set"] or changed["gone"]: await broadcast(app, {"t": "rows", **changed})`.
- [ ] **Step 4 (client):** make `drawRows()` reuse row elements whose content didn't change:

```js
function rowSig(row) { return JSON.stringify([row.mark, row.line, row.shown, row.online, row.key === current]); }

function drawRows() {
  const box = $("rows");
  const old = new Map([...box.querySelectorAll(".row")].map((el) => [el.dataset.key, el]));
  const children = [];
  const byHost = new Map();
  for (const row of rows.values()) {
    if (!byHost.has(row.host)) byHost.set(row.host, []);
    byHost.get(row.host).push(row);
  }
  for (const [host, list] of byHost) {
    const head = document.createElement("div");
    head.className = "host" + (list.every((r) => !r.online) ? " offline" : "");
    head.textContent = list[0].label || host;
    children.push(head);
    for (const row of list.sort((a, b) => a.name.localeCompare(b.name))) {
      const sig = rowSig(row), prev = old.get(row.key);
      if (prev && prev.dataset.sig === sig) { children.push(prev); continue; }  // same node: a click in flight still lands
      const el = rowEl(row);
      el.dataset.sig = sig;
      children.push(el);
    }
  }
  box.replaceChildren(...children);
}
```

`rowEl` must keep setting `el.dataset.key`. In `events.onmessage` keep calling `drawRows()` (it is now cheap and only replaces changed rows).
- [ ] **Step 5: e2e check** in `tests/web/e2e_web.py`: after rows appear, record `document.querySelector('.row[data-key="box/alpha"]')` identity via `page.evaluate` setting `window.__el = …`, wait 1.5 s (fake poll is 0.3 s), then assert `document.querySelector('.row[data-key="box/alpha"]') === window.__el`.
- [ ] **Step 6: Suite + e2e pass. Commit:** `fix(web): rows are only redrawn when they change; unchanged row elements are kept`

### Task C2: Zoom keeps the terminal focused

**Finding W2:** after clicking A+/A− the button keeps focus; typing goes nowhere until the terminal is clicked.

**Files:** `src/tmls/web/static/app.js` (`setFont`, zoom buttons); `tests/web/e2e_web.py`.

- [ ] **Step 1: e2e check:** after `page.click("#zoom-in")`, `page.keyboard.type("zoomtype")`, `Enter`; wait for `zoomtype` to echo back in `.xterm-rows`.
- [ ] **Step 2: FAIL. Step 3:** `$("zoom-in").onmousedown = $("zoom-out").onmousedown = (e) => e.preventDefault();` (buttons never take focus) and at the end of `setFont`: `if (current && $("sketch").hidden) term.focus();`.
- [ ] **Step 4: PASS. Commit:** `fix(web): zoom buttons don't steal focus from the terminal`

### Task C3: Lost live feed is visible; "connection lost" vs "session ended"

**Finding W3:** when the server goes away, rows stay frozen with no sign; the terminal overlay says "session ended" though the session is fine.

**Files:** `src/tmls/web/static/app.js` (`connectEvents`, terminal `onclose`), `src/tmls/web/static/style.css`, `src/tmls/web/static/index.html`; `tests/web/e2e_web.py`.

- [ ] **Step 1: e2e check:** after login and rows, `page.evaluate("events.close()")`; wait for `#rows.stale` and `#feed` visible with text containing `reconnecting`; then (server still up) wait for `#rows:not(.stale)`.
- [ ] **Step 2: FAIL. Step 3:** add `<div id="feed" hidden>live feed lost · reconnecting…</div>` under `<header>` in `index.html`; CSS `#feed { background: var(--panel); color: var(--wait); font-size: 12px; padding: 4px 10px; text-align: center; } #feed[hidden] { display: none; } #rows.stale { opacity: .5; }`. In `connectEvents`: `ws.onopen = () => { $("feed").hidden = true; $("rows").classList.remove("stale"); }`; in `onclose` (non-4401 path) before scheduling the retry: `$("feed").hidden = false; $("rows").classList.add("stale");`. In the terminal `onclose`: `if (retries >= 5) { overlay("connection lost", true); return; }` (keep `"session ended"` for the `exit` message).
- [ ] **Step 4: PASS. Commit:** `fix(web): show when the live feed is down; 'connection lost' when the server is unreachable`

### Task C4: `?` rows show the prompt lines readably

**Finding W4:** `rowEl` flattens `row.shown` with ` · ` into one ellipsised line and sets the tooltip before the override, so the tooltip shows "permission prompt" instead of the prompt.

**Files:** `src/tmls/web/static/app.js` (`rowEl`), `style.css`; `tests/web/e2e_web.py`.

- [ ] **Step 1: e2e check:** `.row[data-key="box/beta"] .prompt` has two child lines `Bash` and `rm x` (the fake prompt), and the row's `title` attribute contains `rm x`.
- [ ] **Step 2: FAIL. Step 3:** for waiting rows with `shown`, replace the single `.line` with `<div class="prompt">` containing one `<div>` per line (max 8), `white-space: normal; word-break: break-word; color: var(--dim); font-size: 12px`; set `el.title = row.shown.join("\n")`. Rows without `shown` keep the ellipsised `.line` with `title = row.line`.
- [ ] **Step 4: PASS. Commit:** `fix(web): permission prompts are shown line by line on their row`

### Task C5: Alert history stays; panel closes on Esc/outside click

**Finding W5:** opening the bell empties `unread`, so reopening shows "No new alerts"; nothing closes the panel but the bell.

**Files:** `src/tmls/web/static/app.js` (alerts section); `tests/web/e2e_web.py`.

- [ ] **Step 1: e2e check:** inject two alerts via `events.onmessage({data: JSON.stringify({t:'alerts', items:[…]})})`, click `#bell` → 2 `.alert` entries; click `#bell` to close; click again → still 2 entries, `#bell-count` empty; press Escape → `#alerts[hidden]`.
- [ ] **Step 2: FAIL. Step 3:** keep `let alerts = []` (newest last, cap 50) and `let unreadCount = 0`; `alerts` message: `alerts.push(...items)`, `alerts = alerts.slice(-50)`, `unreadCount += items.length`; bell click renders all of `alerts` newest first and sets `unreadCount = 0`; `window.addEventListener("keydown", e => { if (e.key === "Escape") $("alerts").hidden = true; })` and `document.addEventListener("click", e => { if (!$("alerts").hidden && !e.target.closest("#alerts, #bell")) $("alerts").hidden = true; })`.
- [ ] **Step 4: PASS. Commit:** `fix(web): alert list is kept after a look; Esc or a click outside closes it`

### Task C6: Login form keeps the username, styles the error, handles the lockout

**Finding W6:** a wrong password clears the username; the error is unstyled; the 1 s penalty gives no feedback and a double click counts twice; the 429 is a bare text page with no way back.

**Files:** `src/tmls/web/static/login.html`, `src/tmls/web/server.py` (`login`); `tests/web/test_server.py`, `tests/web/e2e_web.py`.

- [ ] **Step 1: Server test:** after 5 wrong form posts, the 6th **form post** is a `302` to `/login?error=locked` (not a 429 text page). Keep the existing throttle tests passing.
- [ ] **Step 2: FAIL. Step 3 (server):** replace the `429` response with `raise web.HTTPFound("/login?error=locked")`.
- [ ] **Step 4 (page):** in `login.html`: `#error { color: #e06c75; min-height: 1.2em }`; script: on submit, `sessionStorage.setItem("tmls-user", form.username.value)` and disable the button with text `Checking…`; on load, if `?error` present, restore the username from `sessionStorage` and show `Wrong username or password.` for `error=1` or `Too many wrong passwords; try again in a few minutes.` for `error=locked`. Never put the username in the URL.
- [ ] **Step 5: e2e:** update the wrong-password check to also assert the username field still reads `tester`. Suite + e2e pass.
- [ ] **Step 6: Commit:** `fix(web): login keeps the username, shows a styled error, and explains the lockout`

### Task C7: Lockout keyed by the real client behind a trusted proxy; small robustness fixes

**Findings W7/W10/W8:** the throttle keys on `request.remote`, which is the proxy's address behind the planned Tailscale sidecar. `handle_seen` 500s on non-object JSON. Reattach stacks old screen text (no `term.reset()`). `.host` is `text-transform: lowercase`. Wheel-scroll in a tmux session without `mouse on` sends arrow keys.

**Files:** `src/tmls/web/server.py` (`main`, `login`, new `client_address`), `src/tmls/web/events.py` (`handle_seen`), `src/tmls/web/static/app.js` (`reattach`), `style.css`, README (one sentence); tests in `tests/web/test_server.py`, `tests/web/test_events.py`.

- [ ] **Step 1: Tests:** (a) `client_address(request)` returns the first `X-Forwarded-For` entry only when `request.remote` is in `app["trusted_proxies"]`, else `request.remote` (use `aiohttp.test_utils.make_mocked_request` with `headers` and a mocked `transport` peername, or test through the login route by posting with `X-Forwarded-For: 10.0.0.9` from a client whose remote is `127.0.0.1` and `app["trusted_proxies"] = {"127.0.0.1"}`: five failures from `10.0.0.9` lock out `10.0.0.9` but not a request with `X-Forwarded-For: 10.0.0.10`). (b) `POST /api/seen` with body `[]` or `"x"` → 400 JSON `{"ok": false, "error": "invalid request"}`.
- [ ] **Step 2: FAIL. Step 3:** add `--trust-proxy ADDR` (repeatable) to `main`, stored in `app["trusted_proxies"]` (a set; default empty); `client_address(request)` as above; use it in `login` instead of `request.remote`. `handle_seen`: `try: data = await request.json(); k = data["key"] if isinstance(data, dict) and isinstance(data.get("key"), str) else None except ValueError: k = None; if k is None: return 400`. `$("reattach").onclick`: call `term.reset()` before `attach`. Remove `text-transform: lowercase` from `.host`. README: under **Limitations** (or the web section if one exists) add: "In the browser, the mouse wheel only scrolls tmux history when the session has `set -g mouse on`; otherwise it sends arrow keys like a plain terminal."
- [ ] **Step 4: PASS + suite + e2e. Commit:** `fix(web): lockout keyed by the client behind --trust-proxy; /api/seen validates; reattach resets the screen`

---

## Workstream D — web "new session" (after C is merged; branch `feat/web-create` from `main`)

Owns `src/tmls/web/**`, `tests/web/**`, plus the one-time split of `src/tmls/create.py` and the import line in `src/tmls/app.py` (Task D1 only).

### Task D1: Split the Textual form out of `create.py`

**Why:** the web server must not import Textual (design spec: "No Textual import in the web server"), but `create.py`'s pure functions (`load_presets`, `default_name`, `check_name`, `suggest_name`, `script`, `argv`, `create`) sit next to the `NewSession(ModalScreen)` class and `textual` imports.

**Files:** Modify `src/tmls/create.py` (remove the class and textual imports), Create `src/tmls/create_form.py` (the class, importing the functions from `tmls.create`), Modify `src/tmls/app.py` (only the reference to `create.NewSession` → `create_form.NewSession` and its import), `tests/test_create.py` (update imports for the form tests).

- [ ] **Step 1: Test:** `python -c "import sys; import tmls.create; assert 'textual' not in sys.modules"` as a pytest test in `tests/web/test_server.py` (run in a subprocess via `subprocess.run([sys.executable, "-c", …])`).
- [ ] **Step 2: FAIL. Step 3:** move `NewSession` verbatim into `create_form.py`; keep `from tmls.create import check_name, create, default_name, load_presets, suggest_name` there; update `app.py` and the tests. No behaviour change.
- [ ] **Step 4: PASS + suite. Commit:** `refactor(create): Textual form lives in create_form.py so the web server can import create`

### Task D2: `/api/create`, `/api/suggest-name`, presets in `/api/config`

**Files:** Modify `src/tmls/web/server.py`; Test `tests/web/test_server.py`.

**Interfaces (D3 relies on these):**
- `GET /api/config` → `{"sketchpad": [...], "presets": ["Opus plan", ...]}` (names from `create.load_presets()`).
- `POST /api/suggest-name {"host": str, "folder": str}` → `{"ok": true, "name": str}` (`create.suggest_name`), 400 on bad input.
- `POST /api/create {"host": str, "folder": str, "name": str, "start": "shell" | "claude" | <preset name>}` → `{"ok": true}`; `400 {"ok": false, "error": …}` for invalid input or `check_name` failure; `409 {"ok": false, "error": <create's message>}` when `create.create` returns an error (missing folder, name exists, host unreachable).

- [ ] **Step 1: Tests** (monkeypatch `create.create`, `create.suggest_name`, `create.load_presets` like the e2e server does for `approve`): happy path records `("box", "work", "~/x", "shell")`; a preset name resolves to its argv tuple; `host` not in `app["hosts"]` → 400; name with `:` → 400 with `check_name`'s message; `create.create` returning `"a session named work already exists"` → 409 with that error; `suggest-name` returns the fake's value.
- [ ] **Step 2: FAIL. Step 3: Implement** in `server.py`:

```python
async def create_session(request):
    try:
        data = await request.json()
        host, folder, name, start = (data[f] for f in ("host", "folder", "name", "start"))
        if not all(isinstance(v, str) for v in (host, folder, name, start)) or host not in (*request.app["hosts"], hosts.LOCAL):
            raise ValueError
    except (ValueError, KeyError, TypeError):
        return web.json_response({"ok": False, "error": "invalid request"}, status=400)
    presets = create.load_presets()
    if start not in ("shell", "claude"):
        if start not in presets:
            return web.json_response({"ok": False, "error": "unknown start option"}, status=400)
        start = presets[start]
    error = create.check_name(name.strip())
    if error:
        return web.json_response({"ok": False, "error": error}, status=400)
    error = await create.create(host, name.strip(), folder.strip() or "~", start)
    if error:
        return web.json_response({"ok": False, "error": error}, status=409)
    return web.json_response({"ok": True})
```

plus `suggest_name` route and `presets` in `config`. Register `app.router.add_post("/api/create", create_session)` and `add_post("/api/suggest-name", …)` in `make_app`. Remember the middleware already requires login + same Origin for POSTs.
- [ ] **Step 4: PASS + suite. Commit:** `feat(web): /api/create and /api/suggest-name`

### Task D3: `+` on host headers → New session modal

**Files:** Modify `src/tmls/web/static/index.html`, `app.js`, `style.css`, `tests/web/e2e_server.py` (fake `create.create` that appends to its session list; fake `suggest_name`), `tests/web/e2e_web.py`, README (**New session** bullet: add "Also in the browser: `+` on a host header.").

**UI (approved option C):**

```
┌─ Terminal  Sketch   archbox · Season-36 ● running        A− A+ 14px 🔔 ⎋ ─┐
│                                        ┌─ New session on archbox ────────┐ │
│                                        │ Folder  [~/Others/tmls        ] │ │
│                                        │ Name    [tmls                 ] │ │
│                                        │ Start   (•) shell  ( ) claude   │ │
│                                        │         ( ) Opus plan           │ │
│                                        │ a session named tmls already exists │
│                                        │                 [Cancel] [Create] │ │
│                                        └─────────────────────────────────┘ │
│  sidebar: "archbox  +"  ← the + on each online host header opens the modal │
└────────────────────────────────────────────────────────────────────────────┘
```

Behaviour: Name follows Folder (basename, like `create.default_name`) until the user edits Name; 250 ms after Folder/host changes, `POST /api/suggest-name` and, if Name is still the auto value, replace it with the git repo name. Enter submits, Esc cancels. Create → `POST /api/create`; on error show it in the modal and keep it open; on success close, then select the new session once its row arrives (poll `rows.has(key)` every 300 ms for up to 15 s, then `select(key)`).

- [ ] **Step 1: e2e server:** in `tests/web/e2e_server.py`, keep a module-level `created = []`; fake `create.create(host, name, folder, start)` returns `"a session named X already exists"` when `name` is already listed, else appends `hosts.Session("box", name, 1, False, 0, 1000)` to the sessions `list_host` returns; fake `create.suggest_name` returns `"repo-" + folder.rsplit("/", 1)[-1]`; fake `create.load_presets` returns `{"Opus plan": ("claude", "--model", "opus")}`.
- [ ] **Step 2: e2e checks** (add to `e2e_web.py`): clicking `.host .add` for `box` opens `#new:not([hidden])` with title `New session on box`; typing `~/code/thing` in `#new-folder` makes `#new-name` read `thing` and then (after the fetch) `repo-thing`; the Start radios list `shell`, `claude`, `Opus plan`; Create with name `alpha` shows `already exists` inside `#new-error` and the modal stays open; name `gamma` + Create closes the modal and `.row[data-key="box/gamma"].current` appears and the terminal shows `attached-gamma`; Escape closes the modal.
- [ ] **Step 3: FAIL. Step 4: Implement:** `index.html`: a `<div id="new" hidden>` overlay containing the form (ids `new-title`, `new-folder`, `new-name`, `new-start` (radios), `new-error`, `new-cancel`, `new-create`). `style.css`: centred panel like `#alerts` (`position: fixed; inset: 0; display: grid; place-items: center; background: rgba(21,25,31,.6)`; the panel `width: min(26rem, 90vw); background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 12px`; `#new-error { color: var(--fail); min-height: 1.2em }`). `app.js`: in `drawRows`, when a host group is online append `<button class="add" title="New session">+</button>` to the host header with `onclick = (e) => { e.stopPropagation(); openNew(host, list[0].label || host); }`; `openNew(host, label)` fills the title, resets fields (`~`, `home`, shell), renders preset radios from `presets` (loaded with `/api/config`), shows the overlay and focuses Folder; the auto-name logic as described; `submitNew()` posts and handles the result; Esc/Cancel hide it. Host headers keep their `sig`-style reuse out of scope: rebuilding headers each draw is fine (they hold only the `+`).
- [ ] **Step 5: PASS (suite + e2e). Commit:** `feat(web): new tmux session from a + on each host header`

---

## Task E — wire `pane` into the callers (after A, B, C are merged; branch `fix/wire-pane`)

**Files:** `src/tmls/app.py` (`send_prompt`, `_deliver_queued`, `_show_approvals`, the Approval Yes/No handler), `src/tmls/web/events.py` (`approve.current(host, s.name, pane=s.pane)`), `src/tmls/web/rows.py` (`"pane": s.pane` in each row), `src/tmls/web/server.py` (`approve_prompt` accepts optional `pane` string from the body and passes it), `src/tmls/web/static/app.js` (`answer()` sends `row.pane`); tests in `tests/test_app.py`, `tests/web/test_server.py`, `tests/web/test_rows.py`.

- [ ] **Step 1: Tests:** app: sending to a session whose `pane == "%7"` calls `prompts.send(host, name, text, pane="%7")` (record kwargs); web: a row carries `pane`; `/api/approve` with `"pane": "%7"` calls `approve.answer(..., pane="%7")`, a non-string `pane` → 400.
- [ ] **Step 2: FAIL. Step 3:** pass `session.pane`/`s.pane`/`row.pane` through at each call site; defaults keep the old `=name:` behaviour for non-Claude sessions.
- [ ] **Step 4: PASS + suite + e2e. Commit:** `fix: messages and approvals go to Claude's pane in the TUI and the web`

## Deferred (not in this plan, recorded for the ledger)

- W9: one `tmux capture-pane` per chatty plain session per 2 s tick (perf; consider a longer interval for non-Claude rows).
- T11 "Silence focused off never shows ◆ for the viewed tab"; T12 approve two-round-trip race; T14 create.py suggests `example.com` then rejects it; T16 status matching by pane survives renames (partly covered by B3's `%N` targeting); T17 timeouts in `hosts.list_host`/`local._run`/`create.create` don't kill the child; T19 pid reuse.
- W10 phone layout (18 rem fixed sidebar) — out of scope for v1 per the design spec.
