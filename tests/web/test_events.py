import asyncio

from aiohttp import web

from tmls import approve, hosts, prompts
from tmls.web import events


def sess(name, **kw):
    return hosts.Session("box", name, 1, False, 0, 100, **kw)


async def make(aiohttp_client, monkeypatch, listing):
    async def list_host(host):
        result = listing["box"]
        if isinstance(result, Exception):
            raise result
        return result

    async def run(argv, stdin=None):
        return 0, "line1\nlast line\n"

    async def current(host, name):
        return listing.get("shown")

    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(prompts, "_run", run)
    monkeypatch.setattr(approve, "current", current)
    app = web.Application()
    app["hosts"], app["poll_interval"] = ["box"], 0.05
    events.setup(app)
    return await aiohttp_client(app)


async def next_kind(ws, kind):
    while True:
        msg = await asyncio.wait_for(ws.receive_json(), 3)
        if msg["t"] == kind:
            return msg


async def test_first_message_is_every_row(aiohttp_client, monkeypatch):
    client = await make(aiohttp_client, monkeypatch, {"box": (True, [sess("a"), sess("b")])})
    await asyncio.sleep(0.2)
    ws = await client.ws_connect("/api/events")
    first = await next_kind(ws, "rows")
    assert sorted(r["name"] for r in first["set"]) == ["a", "b"] and first["gone"] == [] and first["full"]
    assert {r["line"] for r in first["set"]} == {"last line"}


async def test_waiting_row_pushes_shown_and_alert(aiohttp_client, monkeypatch):
    listing = {"box": (True, [sess("a", claude="busy", claude_since=90)])}
    client = await make(aiohttp_client, monkeypatch, listing)
    ws = await client.ws_connect("/api/events")
    await next_kind(ws, "rows")
    listing["shown"] = ["Bash", "rm x"]
    listing["box"] = (True, [sess("a", claude="waiting", claude_since=95, waiting="permission prompt")])
    changed = await next_kind(ws, "rows")
    while not changed["set"]:
        changed = await next_kind(ws, "rows")
    assert changed["set"][0]["mark"] == "waiting" and changed["set"][0]["shown"] == ["Bash", "rm x"]
    alert = await next_kind(ws, "alerts")
    assert alert["items"] == [{"key": "box/a", "name": "a", "mark": "waiting"}]


async def test_poll_survives_a_listing_error(aiohttp_client, monkeypatch):
    listing = {"box": OSError("ssh died")}
    client = await make(aiohttp_client, monkeypatch, listing)
    await asyncio.sleep(0.15)
    listing["box"] = (True, [sess("a")])
    ws = await client.ws_connect("/api/events")
    msg = await next_kind(ws, "rows")
    while not msg["set"]:
        msg = await next_kind(ws, "rows")
    assert msg["set"][0]["name"] == "a"


async def test_seen_clears_done(aiohttp_client, monkeypatch):
    client = await make(aiohttp_client, monkeypatch,
                        {"box": (True, [sess("a", claude="idle", claude_since=95)])})
    ws = await client.ws_connect("/api/events")
    first = await next_kind(ws, "rows")
    while not first["set"]:
        first = await next_kind(ws, "rows")
    assert first["set"][0]["mark"] == "done"
    assert (await client.post("/api/seen", json={"key": "box/a"})).status == 200
    msg = await next_kind(ws, "rows")
    while not msg["set"]:
        msg = await next_kind(ws, "rows")
    assert msg["set"][0]["mark"] == "idle"


async def test_unchanged_tick_sends_nothing(monkeypatch):
    async def list_host(host):
        return True, [sess("a"), sess("b")]

    async def run(argv, stdin=None):
        return 0, "last line\n"

    class Socket:
        def __init__(self):
            self.sent = []

        async def send_json(self, message):
            self.sent.append(message)

    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(prompts, "_run", run)
    app = web.Application()
    app["hosts"] = ["box"]
    events.setup(app)
    socket = Socket()
    app["sockets"].add(socket)
    await events.poll_once(app)
    await events.poll_once(app)
    assert [m["t"] for m in socket.sent] == ["rows"]


async def test_seen_rejects_a_body_that_is_not_an_object(aiohttp_client, monkeypatch):
    client = await make(aiohttp_client, monkeypatch, {"box": (True, [sess("a")])})
    for body in ([], "x", {"key": 5}):
        response = await client.post("/api/seen", json=body)
        assert response.status == 400 and await response.json() == {"ok": False, "error": "invalid request"}
    response = await client.post("/api/seen", data="not json", headers={"Content-Type": "application/json"})
    assert response.status == 400
