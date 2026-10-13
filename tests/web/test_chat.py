import asyncio
import base64
import hashlib
import json
import os
import secrets
from unittest.mock import AsyncMock

from tmls import approve, hosts, image_paste, prompts
from tmls.web import chat, server

SID = "0a1b2c3d-0000-4000-8000-000000000001"
PNG = b"\x89PNG fake"


def sess(name, **kw):
    return hosts.Session("box", name, 1, False, 0, 100, **kw)


def claude(name="work", session_id=SID, **kw):
    return sess(name, **{"claude": "idle", "pane": "%7", "session_id": session_id, "title": "Fix parser", **kw})


def line(kind, content, uuid, **more):
    return json.dumps({"type": kind, "uuid": uuid, "timestamp": "2026-10-13T08:15:00Z",
                       "message": {"role": kind, "content": content}, **more}) + "\n"


def say(n):
    return line("assistant", [{"type": "text", "text": f"reply {n}"}], f"a{n}")


def record_file(home, session_id=SID):
    path = home / ".claude" / "projects" / "-some-folder" / f"{session_id}.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


async def make(aiohttp_client, monkeypatch, tmp_path, listing, login=True):
    """The real app with a faked host "box": its listing is `listing["box"]`, and its record is
    followed on this machine under a HOME of its own (the real script, no ssh)."""
    async def list_host(host):
        return listing["box"]

    async def no_background(host):
        return []

    async def run(argv, stdin=None):
        return 0, "screen\n"

    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(hosts, "background", no_background)
    monkeypatch.setattr(prompts, "_run", run)
    monkeypatch.setattr(approve, "current", AsyncMock(return_value=None))
    monkeypatch.setattr(approve, "current_choices", AsyncMock(return_value=None))
    monkeypatch.setattr(chat, "WATCH", 0.05)
    monkeypatch.setattr(chat, "RETRY", 0.05)
    monkeypatch.setenv("HOME", str(tmp_path))
    salt = secrets.token_hex(16)
    hashed = hashlib.scrypt(b"pw", salt=bytes.fromhex(salt), n=2**14, r=8, p=1).hex()
    (tmp_path / "auth.json").write_text(json.dumps({"username": "me", "salt": salt, "hash": hashed, "secret": "s" * 64}))
    app = server.make_app(tmp_path / "auth.json", ["box"])
    app["poll_interval"] = 0.05
    app["chat_argv"] = lambda host, session_id: chat.follow_argv(hosts.LOCAL, session_id)
    client = await aiohttp_client(app)
    if login:
        await client.post("/login", data={"username": "me", "password": "pw"})
    return client


async def get(ws):
    return await asyncio.wait_for(ws.receive_json(), 5)


async def closed(ws):
    msg = await asyncio.wait_for(ws.receive(), 5)
    assert msg.type.name in ("CLOSE", "CLOSED", "CLOSING"), msg
    return ws.close_code


async def test_chat_needs_login_and_this_sites_origin(aiohttp_client, monkeypatch, tmp_path):
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude()])}, login=False)
    assert await closed(await client.ws_connect("/api/chat?key=box/work")) == 4401
    assert (await client.get("/api/chat/blob?key=box/work&id=x")).status == 401
    for path, body in (("/api/chat/send", {"key": "box/work", "text": "hi"}),
                       ("/api/chat/key", {"key": "box/work", "name": "Escape"})):
        assert (await client.post(path, json=body)).status == 401
    assert (await client.post("/api/chat/upload?key=box/work&name=a.png", data=PNG)).status == 401
    await client.post("/login", data={"username": "me", "password": "pw"})
    evil = {"Origin": "https://evil.example"}
    assert await closed(await client.ws_connect("/api/chat?key=box/work", headers=evil)) == 4403
    send = AsyncMock(return_value=None)
    monkeypatch.setattr(prompts, "send", send)
    assert (await client.post("/api/chat/send", json={"key": "box/work", "text": "hi"}, headers=evil)).status == 403
    assert (await client.post("/api/chat/key", json={"key": "box/work", "name": "Escape"}, headers=evil)).status == 403
    assert (await client.post("/api/chat/upload?key=box/work&name=a.png", data=PNG, headers=evil)).status == 403
    send.assert_not_awaited()


async def test_history_then_new_items_patches_and_blobs(aiohttp_client, monkeypatch, tmp_path):
    path = record_file(tmp_path)
    image = {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": base64.b64encode(PNG).decode()}}
    path.write_text(line("user", "fix it", "u1")
                    + line("assistant", [{"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/a/app.rs"}}], "a1")
                    + line("user", [{"type": "tool_result", "tool_use_id": "t1", "content": "fn main"}], "u2")
                    + json.dumps({"type": "custom-title", "customTitle": "x"}) + "\n")
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude()])})
    ws = await client.ws_connect("/api/chat?key=box/work")
    assert await get(ws) == {"t": "open", "key": "box/work", "session": SID, "title": "Fix parser"}
    first = await get(ws)
    assert first["t"] == "items" and first["reset"] is True
    assert [(i["kind"], i["id"]) for i in first["items"]] == [("user", "u1"), ("tool", "t1")]
    assert first["items"][1]["summary"] == "Read app.rs" and first["items"][1]["output"] == "fn main"
    assert len(client.app["chat_children"]) == 1
    with path.open("a") as f:  # Claude goes on: a reply, a call, and later that call's result
        f.write(say(2) + line("assistant", [{"type": "tool_use", "id": "t2", "name": "Bash", "input": {"command": "shot"}}], "a3"))
    assert await get(ws) == {"t": "items", "reset": False, "items": [
        {"id": "a2", "ts": 1791879300, "kind": "assistant", "text": "reply 2"}]}
    call = await get(ws)
    assert call["reset"] is False and call["items"][0]["id"] == "t2" and call["items"][0]["output"] is None
    with path.open("a") as f:
        f.write(line("user", [{"type": "tool_result", "tool_use_id": "t2", "content": [image]}], "u3")
                + line("user", [{"type": "tool_result", "tool_use_id": "never-sent", "content": "x"}], "u4") + say(4))
    assert await get(ws) == {"t": "patch", "id": "t2", "output": "", "error": False,
                             "images": [{"id": "t2-0", "mime": "image/png"}]}
    assert (await get(ws))["items"][0]["id"] == "a4"  # the result of a call the app never got is not sent
    got = await client.get("/api/chat/blob?key=box/work&id=t2-0")
    assert got.status == 200 and got.content_type == "image/png" and await got.read() == PNG
    assert got.headers["X-Content-Type-Options"] == "nosniff"
    assert (await client.get("/api/chat/blob?key=box/work&id=nope")).status == 404
    assert (await client.get("/api/chat/blob?key=box/other&id=t2-0")).status == 404
    assert (await client.get("/api/chat/blob?key=-oProxy/x&id=t2-0")).status == 400
    pid = next(iter(client.app["chat_children"]))
    await ws.close()
    for _ in range(100):  # the socket closed: its child is killed, and its images are let go
        if not client.app["chat_children"] and not client.app["chat_blobs"]:
            break
        await asyncio.sleep(0.02)
    assert not client.app["chat_children"] and not client.app["chat_blobs"]
    try:
        os.kill(pid, 0)
        assert False, "the tail is still running"
    except ProcessLookupError:
        pass


async def test_history_is_the_newest_300_of_the_records_end(aiohttp_client, monkeypatch, tmp_path):
    path = record_file(tmp_path)
    path.write_text("".join(say(n) for n in range(400)))
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude()])})
    ws = await client.ws_connect("/api/chat?key=box/work")
    assert (await get(ws))["t"] == "open"
    first = await get(ws)
    assert [i["id"] for i in first["items"]] == [f"a{n}" for n in range(100, 400)]
    await ws.close()
    # a long record: only its end is read, and the line the cut falls in is dropped
    monkeypatch.setattr(chat, "WINDOW", len(say(399)) * 10 + 5)
    ws = await client.ws_connect("/api/chat?key=box/work")
    assert (await get(ws))["t"] == "open"
    assert [i["id"] for i in (await get(ws))["items"]] == [f"a{n}" for n in range(390, 400)]
    with path.open("a") as f:
        f.write(say(400))
    assert (await get(ws))["items"][0]["id"] == "a400"


async def test_a_new_record_in_the_same_session_starts_over(aiohttp_client, monkeypatch, tmp_path):
    other = "0a1b2c3d-0000-4000-8000-000000000002"
    record_file(tmp_path).write_text(say(1))
    record_file(tmp_path, other).write_text(say(7) + say(8))
    listing = {"box": (True, [claude()])}
    client = await make(aiohttp_client, monkeypatch, tmp_path, listing)
    ws = await client.ws_connect("/api/chat?key=box/work")
    assert (await get(ws))["session"] == SID
    assert [i["id"] for i in (await get(ws))["items"]] == ["a1"]
    listing["box"] = (True, [claude(session_id=other)])  # /clear
    assert await get(ws) == {"t": "open", "key": "box/work", "session": other, "title": "Fix parser"}
    again = await get(ws)
    assert again["reset"] is True and [i["id"] for i in again["items"]] == ["a7", "a8"]
    listing["box"] = (True, [])  # the tmux session is killed
    assert await get(ws) == {"t": "none", "reason": "session ended"}
    assert await closed(ws) == 1000


async def test_a_claude_that_has_written_nothing_yet_is_an_empty_chat(aiohttp_client, monkeypatch, tmp_path):
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude()])})
    ws = await client.ws_connect("/api/chat?key=box/work")
    assert (await get(ws))["t"] == "open"
    assert await get(ws) == {"t": "items", "reset": True, "items": []}
    record_file(tmp_path).write_text(line("user", "hello", "u1"))  # the first message starts the record
    assert (await get(ws))["t"] == "open"
    first = await get(ws)
    assert first["reset"] is True and first["items"][0]["text"] == "hello"


async def test_no_chat_without_a_claude_a_session_or_a_host(aiohttp_client, monkeypatch, tmp_path):
    listing = {"box": (True, [claude(), sess("shell"), claude("odd", session_id="$(touch x)")])}
    client = await make(aiohttp_client, monkeypatch, tmp_path, listing)
    for key, reason in (("box/shell", "no Claude in this session"), ("box/gone", "no such session"),
                        ("box/odd", "no Claude in this session")):
        ws = await client.ws_connect("/api/chat", params={"key": key})
        assert await get(ws) == {"t": "none", "reason": reason}
        assert await closed(ws) == 1000
    for key in ("-oProxyCommand=bad/work", "elsewhere/work", "box", "box/", ""):
        assert await closed(await client.ws_connect("/api/chat", params={"key": key})) == 4404
    assert await closed(await client.ws_connect("/api/chat")) == 4404
    client.app["chat_argv"] = lambda host, session_id: ["sh", "-c", "exit 255"]  # ssh could not connect
    ws = await client.ws_connect("/api/chat?key=box/work")
    assert await get(ws) == {"t": "none", "reason": "host unreachable"}
    listing["box"] = (False, [])
    await asyncio.sleep(0.2)
    ws = await client.ws_connect("/api/chat?key=box/work")
    assert await get(ws) == {"t": "none", "reason": "host unreachable"}
    assert not client.app["chat_children"]


async def test_send_pastes_into_claudes_own_pane(aiohttp_client, monkeypatch, tmp_path):
    listing = {"box": (True, [claude(), sess("shell"), claude("split", pane=None)])}
    client = await make(aiohttp_client, monkeypatch, tmp_path, listing)
    send = AsyncMock(return_value=None)
    monkeypatch.setattr(prompts, "send", send)
    text = "two lines\n$(rm -rf ~); `x` 'q'"
    ok = await client.post("/api/chat/send", json={"key": "box/work", "text": text})
    assert ok.status == 200 and await ok.json() == {"ok": True}
    send.assert_awaited_once_with("box", "work", text, pane="%7")
    assert (await client.post("/api/chat/send", json={"key": "box/split", "text": "hi"})).status == 200
    send.assert_awaited_with("box", "split", "hi", pane=None)  # no pane known: the session's active one
    calls = send.await_count
    for body, status in (({"key": "box/work", "text": ""}, 400), ({"key": "box/work", "text": " \n"}, 400),
                         ({"key": "box/work"}, 400), ({"key": "box/work", "text": 5}, 400), ({"text": "hi"}, 400),
                         ({"key": "-oProxyCommand=bad/work", "text": "hi"}, 400), ({"key": "nas/work", "text": "hi"}, 400),
                         ({"key": ["box/work"], "text": "hi"}, 400), ({"key": "box/gone", "text": "hi"}, 404),
                         ({"key": "box/shell", "text": "hi"}, 409)):
        answer = await client.post("/api/chat/send", json=body)
        assert answer.status == status and (await answer.json())["ok"] is False, body
    assert (await client.post("/api/chat/send", data="not json")).status == 400
    assert (await client.post("/api/chat/send", json=["a list"])).status == 400
    assert send.await_count == calls
    send.return_value = "can't find pane: %7"
    gone = await client.post("/api/chat/send", json={"key": "box/work", "text": "hi"})
    assert gone.status == 409 and await gone.json() == {"ok": False, "error": "can't find pane: %7"}
    send.return_value = "couldn't reach the host"
    assert (await client.post("/api/chat/send", json={"key": "box/work", "text": "hi"})).status == 502
    listing["box"] = (False, [])
    await asyncio.sleep(0.2)
    assert (await client.post("/api/chat/send", json={"key": "box/work", "text": "hi"})).status == 502


async def test_send_hands_the_text_to_tmux_on_stdin(monkeypatch):
    seen = []

    async def run(argv, stdin=None):
        seen.append((argv, stdin))
        return 0, ""

    monkeypatch.setattr(prompts, "_run", run)
    text = "it's $(date)\nline two"
    assert await prompts.send("box", "work", text, pane="%7") is None
    (argv, stdin), = seen
    assert stdin == text.encode() and "date" not in argv[-1] and "load-buffer" in argv[-1]
    assert "paste-buffer -p -d" in argv[-1] and "send-keys -t %7 Enter" in argv[-1]


async def test_key_sends_only_the_named_keys(aiohttp_client, monkeypatch, tmp_path):
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude(), claude("my session", pane=None)])})
    seen = []

    async def run(argv, stdin=None):
        seen.append(argv[-1])
        return (None, "couldn't reach the host") if "Down" in argv[-1] else (1 if "Up" in argv[-1] else 0, "no pane\n")

    monkeypatch.setattr(prompts, "_run", run)
    for name in chat.KEYS:
        if name not in ("Up", "Down"):
            answer = await client.post("/api/chat/key", json={"key": "box/work", "name": name})
            assert answer.status == 200 and await answer.json() == {"ok": True}
            assert seen[-1] == f"tmux send-keys -t %7 {name}"
    assert (await client.post("/api/chat/key", json={"key": "box/my session", "name": "Escape"})).status == 200
    assert seen[-1] == "tmux send-keys -t '=my session:' Escape"
    failed = await client.post("/api/chat/key", json={"key": "box/work", "name": "Up"})
    assert failed.status == 409 and (await failed.json())["error"] == "no pane"
    assert (await client.post("/api/chat/key", json={"key": "box/work", "name": "Down"})).status == 502
    seen.clear()
    for name in ("C-c", "5", "Enter; rm -rf ~", "escape", "", None, 1, ["Enter"], "Enter Enter"):
        assert (await client.post("/api/chat/key", json={"key": "box/work", "name": name})).status == 400, name
    assert (await client.post("/api/chat/key", json={"key": "elsewhere/work", "name": "Enter"})).status == 400
    assert (await client.post("/api/chat/key", json={"key": "box/gone", "name": "Enter"})).status == 404
    assert not [s for s in seen if "send-keys" in s]


async def test_upload_goes_to_the_hosts_cache_through_stdin(aiohttp_client, monkeypatch, tmp_path):
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude(), sess("shell")])})
    seen = []

    async def run(argv, stdin=None, timeout=10):
        seen.append((argv, stdin))
        if stdin == b"fail":
            raise image_paste.ImageError("disk full")
        return b"a login banner\n/home/u/.cache/tmls/images/NAME\n"

    monkeypatch.setattr(image_paste, "_run", run)
    ok = await client.post("/api/chat/upload", params={"key": "box/work", "name": "My Photo.PNG"}, data=PNG)
    assert ok.status == 200 and await ok.json() == {"ok": True, "path": "/home/u/.cache/tmls/images/NAME"}
    argv, stdin = seen[-1]
    assert argv[:1] == ["ssh"] and argv[-2] == "box" and stdin == PNG
    assert 'cat > "$d/' in argv[-1] and '.png"' in argv[-1] and "My Photo" not in argv[-1]
    # a plain session takes files too; the extension is kept only when it is a plain one
    for name in ("notes.tar.gz", "x.$(reboot)", "noext", "a.p;g", ".bashrc"):
        assert (await client.post("/api/chat/upload", params={"key": "box/shell", "name": name}, data=b"x")).status == 200
    scripts = [argv[-1] for argv, _ in seen[1:]]
    assert '.gz"' in scripts[0] and all("reboot" not in s and ";g" not in s and "bashrc" not in s for s in scripts)
    calls = len(seen)
    for params, status in (({"key": "box/work"}, 400), ({"name": "a.png"}, 400), ({"key": "elsewhere/x", "name": "a.png"}, 400),
                           ({"key": "box/gone", "name": "a.png"}, 404)):
        assert (await client.post("/api/chat/upload", params=params, data=PNG)).status == status, params
    assert (await client.post("/api/chat/upload", params={"key": "box/work", "name": "a.png"}, data=b"")).status == 400
    large = await client.post("/api/chat/upload", params={"key": "box/work", "name": "a.png"},
                              data=bytes(chat.UPLOAD_MAX + 1))
    assert large.status == 413 and (await large.json())["ok"] is False
    assert len(seen) == calls
    failed = await client.post("/api/chat/upload", params={"key": "box/work", "name": "a.png"}, data=b"fail")
    assert failed.status == 502 and await failed.json() == {"ok": False, "error": "disk full"}


async def test_upload_really_stores_the_bytes(monkeypatch, tmp_path):
    monkeypatch.setenv("HOME", str(tmp_path))
    path = await chat.store(hosts.LOCAL, PNG, "shot.jpeg")
    assert path.startswith(str(tmp_path / ".cache" / "tmls" / "images")) and path.endswith(".jpeg")
    assert open(path, "rb").read() == PNG
    assert await chat.store(hosts.LOCAL, PNG, "shot.jpeg") != path


def test_blobs_drop_the_oldest_when_full():
    blobs = chat.Blobs(limit=10)
    blobs.put("a", "image/png", b"1234")
    blobs.put("b", "image/png", b"1234")
    blobs.put("a", "image/png", b"1234")  # seen again: not counted twice, and now the newest
    assert blobs.size == 8
    blobs.put("c", "image/png", b"1234")
    assert blobs.get("b") is None and blobs.get("a") == ("image/png", b"1234") and blobs.size == 8
    blobs.put("huge", "image/png", bytes(11))
    assert blobs.get("huge") is None and blobs.get("c")


def test_follow_argv_quotes_and_runs_on_the_sessions_host():
    argv = chat.follow_argv("nas", SID)
    assert argv[0] == "ssh" and argv[-2] == "nas" and f"{SID}.jsonl" in argv[-1] and "tail -c +" in argv[-1]
    assert chat.follow_argv(hosts.LOCAL, SID)[:2] == ["sh", "-c"]
