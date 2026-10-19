import asyncio
import json
from unittest.mock import AsyncMock

from tmls import hosts, prompts
from tmls.web import chat, codex_record, codex_thread

from .test_chat import closed, get, make, sess

T1 = "01a147fe-72d4-70f1-b8a4-685fe0aa2450"
T2 = "01a147fe-72d4-70f1-b8a4-685fe0aa2451"


def done(item, ts="2026-10-17T03:53:49.505Z"):
    return json.dumps({"timestamp": ts, "type": "event_msg", "payload": {"type": "item_completed", "item": item}}) + "\n"


def user(i, text):
    return done({"type": "UserMessage", "id": i, "content": [{"type": "text", "text": text, "text_elements": []}]})


def reply(i, text):
    return done({"type": "AgentMessage", "id": i, "content": [{"type": "Text", "text": text}], "phase": "final_answer"})


def ran(i, command, output="", code=0):
    return done({"type": "CommandExecution", "id": i, "command": ["/bin/bash", "-lc", command],
                 "aggregated_output": output, "exit_code": code, "status": "completed"})


def items(lines):
    out = []
    for line in lines:
        out += codex_record.read(line)[0]
    return out


def test_record_items_from_completed_turn_items():
    got = items([
        json.dumps({"type": "session_meta", "payload": {"id": T1}}) + "\n",
        json.dumps({"type": "response_item", "payload": {"type": "message", "role": "user",
                                                         "content": [{"type": "input_text", "text": "# AGENTS.md"}]}}) + "\n",
        user("u1", "what is this"),
        done({"type": "Reasoning", "id": "r1", "summary": []}),
        reply("m1", "Looking."),
        ran("c1", "ls -la /tmp", "a\nb", 0),
        ran("c2", "false", "", 1),
        done({"type": "FileChange", "id": "f1", "changes": {"/a/app.py": {"type": "update"}}, "status": "completed",
              "stdout": "Success.", "stderr": ""}),
        done({"type": "Extension", "kind": "web.search", "id": "w1", "query": "codex rollout",
              "results": [{"title": "Docs", "url": "https://x.example"}]}),
        done({"type": "ContextCompaction", "id": "k1"}),
        json.dumps({"timestamp": "2026-10-17T03:54:00Z", "type": "event_msg",
                    "payload": {"type": "turn_aborted", "turn_id": "t9", "reason": "interrupted"}}) + "\n",
        "{broken",
    ])
    assert [(i["kind"], i["id"]) for i in got] == [
        ("user", "u1"), ("assistant", "m1"), ("tool", "c1"), ("tool", "c2"), ("tool", "f1"), ("tool", "w1"),
        ("note", "k1"), ("note", "aborted-t9")]
    assert got[0] == {"id": "u1", "ts": 1792209229, "kind": "user", "text": "what is this", "images": []}
    assert got[1]["text"] == "Looking."
    assert got[2]["summary"] == "Ran ls -la /tmp" and got[2]["input"] == "ls -la /tmp" and got[2]["output"] == "a\nb"
    assert got[2]["error"] is False and got[3]["error"] is True
    assert got[4]["summary"] == "Edited app.py" and got[4]["input"] == "update: /a/app.py"
    assert got[5]["summary"] == 'Searched "codex rollout"' and "https://x.example" in got[5]["output"]
    assert got[6]["text"] == "Context compacted" and got[7]["text"] == "Interrupted"


NOW = 1792209300  # 2026-10-17T03:55:00Z


def listing(panes, procs, fds=(), threads=(), names=(), cwds=()):
    out = [f"now\t{NOW}"] + [f"pane\t{p}" for p in panes] + [f"proc\t{p}" for p in procs]
    out += [f"cwd\t{pid}\t{folder}" for pid, folder in cwds] + [f"fd\t{f}" for f in fds]
    for i, cwd, originator, *ts in threads:
        meta = json.dumps({"timestamp": ts[0] if ts else "2026-10-17T03:54:00Z", "type": "session_meta",
                           "payload": {"id": i, "cwd": cwd, "originator": originator}}, separators=(",", ":"))
        out.append(f"thread\t{i}\t{path(i)}\t{meta[:200]}")
    out += [f"name\t{i}\t{n}" for i, n in names]
    return "\n".join(out) + "\n"


# a pane's shell, codex's node wrapper (started 10 min ago) and its binary, then the daemon
TUI = ["100\t1\t900\tbash", "101\t100\t600\tnode /h/.local/bin/codex --yolo", "102\t101\t600\t/h/vendor/codex --yolo"]
DAEMON = ["300\t102\t600\t/h/.codex/packages/codex app-server daemon",
          "301\t300\t600\t/h/.codex/packages/codex app-server --listen unix://"]
LOCKS = [f"301\t/h/.codex/thread-writer-locks/{T1}.lock", f"301\t/h/.codex/thread-writer-locks/{T2}.lock"]
# another tmux session's Codex: shell 200, wrapper 201
OTHER = ["200\t1\t900\tbash", "201\t200\t300\tnode /h/.local/bin/codex", "202\t201\t300\t/h/vendor/codex"]


def path(i):
    return f"/h/.codex/sessions/2026/10/16/rollout-2026-10-16T21-53-40-{i}.jsonl"


def test_pick_the_thread_named_in_the_pane_title_even_behind_a_spinner():
    out = listing(["%8\t100\t/h\t⠙ Inspect page | h"], TUI + DAEMON, LOCKS,
                  [(T1, "/h", "codex-tui"), (T2, "/h", "codex-tui")], [(T1, "Inspect page"), (T2, "Other")])
    assert codex_thread.pick(out) == {"pane": "%8", "thread": T1, "path": path(T1), "title": "Inspect page"}


def test_pick_the_one_live_thread_in_the_folder_started_after_this_codex():
    out = listing(["%8\t100\t/h/proj\tcodex"], TUI + DAEMON, LOCKS,
                  [(T1, "/h", "codex-tui"), (T2, "/h/proj", "codex-tui")], cwds=[("101", "/h/proj")])
    assert codex_thread.pick(out)["thread"] == T2
    # a `codex exec` run or another client of the daemon is not this TUI's
    for originator in ("codex_exec", "Claude Code"):
        out = listing(["%8\t100\t/h/proj\tcodex"], TUI + DAEMON, LOCKS,
                      [(T1, "/h/proj", originator), (T2, "/h/proj", "codex-tui")], cwds=[("101", "/h/proj")])
        assert codex_thread.pick(out)["thread"] == T2
    # a thread older than this Codex is someone else's
    out = listing(["%8\t100\t/h/proj\tcodex"], TUI + DAEMON, LOCKS,
                  [(T2, "/h/proj", "codex-tui", "2026-10-17T03:30:00Z")], cwds=[("101", "/h/proj")])
    assert codex_thread.pick(out)["thread"] is None


def test_pick_no_folder_guess_while_another_codex_works_in_that_folder():
    """Session A's fresh Codex must not show session B's conversation in the same folder."""
    out = listing(["%8\t100\t/h/proj\tcodex"], TUI + OTHER + DAEMON, [f"301\t/h/.codex/thread-writer-locks/{T2}.lock"],
                  [(T2, "/h/proj", "codex-tui")], cwds=[("101", "/h/proj"), ("201", "/h/proj")])
    assert codex_thread.pick(out) == {"pane": "%8", "thread": None, "path": None, "title": None}
    elsewhere = listing(["%8\t100\t/h/proj\tcodex"], TUI + OTHER + DAEMON, [f"301\t/h/.codex/thread-writer-locks/{T2}.lock"],
                        [(T2, "/h/proj", "codex-tui")], cwds=[("101", "/h/proj"), ("201", "/h/x")])
    assert codex_thread.pick(elsewhere)["thread"] == T2


def test_pick_a_tui_writing_its_own_record_wins():
    out = listing(["%8\t100\t/h\tcodex"], TUI, [f"102\t{path(T2)}", f"999\t{path(T1)}"],
                  [(T1, "/h", "codex-tui"), (T2, "/h", "codex-tui")])
    assert codex_thread.pick(out)["thread"] == T2


def test_pick_no_conversation_yet_or_no_codex():
    two_here = listing(["%8\t100\t/h\tcodex"], TUI + DAEMON, LOCKS, [(T1, "/h", "codex-tui"), (T2, "/h", "codex-tui")],
                       cwds=[("101", "/h")])
    assert codex_thread.pick(two_here) == {"pane": "%8", "thread": None, "path": None, "title": None}
    assert codex_thread.pick(listing(["%8\t100\t/h\tcodex"], TUI + DAEMON)) == {
        "pane": "%8", "thread": None, "path": None, "title": None}
    daemon_only = ["100\t1\t9\tbash", "300\t100\t9\t/h/codex app-server daemon", "301\t100\t9\tcodex exec hi"]
    assert codex_thread.pick(listing(["%9\t100\t/h\tbash"], daemon_only)) is None
    assert codex_thread.pick("") is None


def test_tui_command_lines():
    assert codex_thread.is_tui("node /h/.local/bin/codex --yolo")
    assert codex_thread.is_tui("codex")
    assert codex_thread.is_tui('codex "exec the tests"')  # a prompt that starts with a subcommand's name
    assert not codex_thread.is_tui("codex exec hi")
    assert not codex_thread.is_tui("/h/.codex/packages/codex app-server daemon")
    assert not codex_thread.is_tui("/h/codex-code-mode-host")


def test_pick_on_this_machines_real_output_parses(tmp_path):
    """The script runs under sh and prints what pick reads (no tmux session here: nothing found)."""
    import subprocess
    out = subprocess.run(codex_thread.find_argv(hosts.LOCAL, "tmls-no-such-session"), capture_output=True, text=True)
    assert out.returncode == 0 and out.stdout.startswith("now\t")
    assert codex_thread.pick(out.stdout) is None


def codex(name="cx", **kw):
    return sess(name, **{"agent": "codex", **kw})


async def codex_client(aiohttp_client, monkeypatch, tmp_path, found, rows=None):
    """make() with a Codex session "cx" whose find answers found[0] (changeable by the test)."""
    client = await make(aiohttp_client, monkeypatch, tmp_path, rows or {"box": (True, [codex()])})

    async def find(host, name):
        return found[0]

    monkeypatch.setattr(chat, "CODEX_WATCH", 0.05)
    client.app["codex_find"] = find
    client.app["codex_argv"] = lambda host, p: chat.follow_path_argv(hosts.LOCAL, p)
    return client


async def test_codex_chat_history_new_items_and_a_new_thread(aiohttp_client, monkeypatch, tmp_path):
    first, second = tmp_path / "r1.jsonl", tmp_path / "r2.jsonl"
    first.write_text(user("u1", "hi") + reply("m1", "hello") + ran("c1", "ls", "x"))
    second.write_text(reply("m9", "fresh"))
    found = [{"pane": "%8", "thread": T1, "path": str(first), "title": "Inspect page"}]
    client = await codex_client(aiohttp_client, monkeypatch, tmp_path, found)
    ws = await client.ws_connect("/api/chat?key=box/cx")
    assert await get(ws) == {"t": "open", "key": "box/cx", "session": T1, "title": "Inspect page"}
    history = await get(ws)
    assert history["reset"] is True and [i["id"] for i in history["items"]] == ["u1", "m1", "c1"]
    with first.open("a") as f:
        f.write(reply("m2", "more"))
    assert (await get(ws))["items"] == [{"id": "m2", "ts": 1792209229, "kind": "assistant", "text": "more"}]
    found[0] = {"pane": "%8", "thread": T2, "path": str(second), "title": None}  # /new in Codex
    assert await get(ws) == {"t": "open", "key": "box/cx", "session": T2, "title": None}
    assert [i["id"] for i in (await get(ws))["items"]] == ["m9"]
    await ws.close()


async def test_codex_with_no_conversation_yet_is_an_empty_chat(aiohttp_client, monkeypatch, tmp_path):
    record = tmp_path / "r1.jsonl"
    found = [{"pane": "%8", "thread": None, "path": None, "title": None}]
    client = await codex_client(aiohttp_client, monkeypatch, tmp_path, found)
    ws = await client.ws_connect("/api/chat?key=box/cx")
    assert await get(ws) == {"t": "open", "key": "box/cx", "session": None, "title": None}
    assert await get(ws) == {"t": "items", "reset": True, "items": []}
    record.write_text(user("u1", "first"))
    found[0] = {"pane": "%8", "thread": T1, "path": str(record), "title": None}
    assert (await get(ws))["session"] == T1
    assert [i["text"] for i in (await get(ws))["items"]] == ["first"]
    await ws.close()


async def test_no_codex_or_an_unreachable_host_ends_the_chat(aiohttp_client, monkeypatch, tmp_path):
    found = [None]
    client = await codex_client(aiohttp_client, monkeypatch, tmp_path, found)
    ws = await client.ws_connect("/api/chat?key=box/cx")
    assert await get(ws) == {"t": "none", "reason": "no Codex in this session"}
    assert await closed(ws) == 1000
    found[0] = "couldn't reach the host"
    ws = await client.ws_connect("/api/chat?key=box/cx")
    assert await get(ws) == {"t": "none", "reason": "host unreachable"}


async def test_send_and_keys_go_to_codexs_pane_found_now(aiohttp_client, monkeypatch, tmp_path):
    rows = {"box": (True, [codex(), codex("asks", agent_state="waiting"), sess("opencode", agent="opencode")])}
    found = [{"pane": "%8", "thread": T1, "path": None, "title": None}]
    client = await codex_client(aiohttp_client, monkeypatch, tmp_path, found, rows)
    send = AsyncMock(return_value=None)
    monkeypatch.setattr(prompts, "send", send)
    assert (await client.post("/api/chat/send", json={"key": "box/cx", "text": "hi", "session": T1})).status == 200
    send.assert_awaited_once_with("box", "cx", "hi", pane="%8")
    found[0] = {"pane": "%9", "thread": None, "path": None, "title": None}  # moved to a split, no record yet
    assert (await client.post("/api/chat/send", json={"key": "box/cx", "text": "hi"})).status == 200
    send.assert_awaited_with("box", "cx", "hi", pane="%9")
    found[0] = {"pane": "%8", "thread": T2, "path": None, "title": None}
    moved = await client.post("/api/chat/send", json={"key": "box/cx", "text": "hi", "session": T1})
    assert moved.status == 409 and "another conversation" in (await moved.json())["error"]
    asks = await client.post("/api/chat/send", json={"key": "box/asks", "text": "hi"})
    assert asks.status == 409 and "permission" in (await asks.json())["error"]
    assert (await client.post("/api/chat/send", json={"key": "box/opencode", "text": "hi"})).status == 409
    found[0] = None
    gone = await client.post("/api/chat/send", json={"key": "box/cx", "text": "hi"})
    assert gone.status == 409 and "not running" in (await gone.json())["error"]
    found[0] = "couldn't reach the host"
    assert (await client.post("/api/chat/send", json={"key": "box/cx", "text": "hi"})).status == 502
    assert send.await_count == 2
    found[0] = {"pane": "%8", "thread": T1, "path": None, "title": None}
    seen = []

    async def run(argv, stdin=None):
        seen.append(argv[-1])
        return 0, "screen\n@@tmls-command@@\nnode\n" if "capture-pane" in argv[-1] else ""

    monkeypatch.setattr(prompts, "_run", run)
    assert (await client.post("/api/chat/key", json={"key": "box/cx", "name": "Escape"})).status == 200
    assert seen[-1] == "tmux send-keys -t %8 Escape"
    assert (await client.post("/api/chat/key", json={"key": "box/asks", "name": "Escape"})).status == 200


async def test_rows_say_which_sessions_open_as_a_chat(aiohttp_client, monkeypatch, tmp_path):
    from tmls.web import rows
    state = rows.State()
    out = rows.build(state, [("box", True, [codex(), sess("oc", agent="opencode"), sess("sh"),
                                            sess("cl", claude="idle", agent="claude")])], {})
    assert {r["name"]: r["chat"] for r in out} == {"cx": True, "oc": False, "sh": False, "cl": True}
