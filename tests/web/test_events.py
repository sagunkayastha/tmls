from tmls.web import server
import asyncio

from aiohttp import web

from tmls import approve, hosts, prompts
from tmls.web import events


def sess(name, **kw):
    return hosts.Session("box", name, 1, False, 0, 100, **kw)


async def no_background(host):
    return []


async def make(aiohttp_client, monkeypatch, listing):
    async def list_host(host):
        result = listing["box"]
        if isinstance(result, Exception):
            raise result
        return result

    async def run(argv, stdin=None):
        return 0, "line1\nlast line\n"

    async def current(host, name, pane=None):
        return listing.get("shown")

    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(hosts, "background", no_background)
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
    monkeypatch.setattr(hosts, "background", no_background)
    monkeypatch.setattr(prompts, "_run", run)
    app = web.Application()
    app["hosts"] = ["box"]
    events.setup(app)
    socket = Socket()
    app["sockets"].add(socket)
    await events.poll_once(app)
    await events.poll_once(app)
    assert [m["t"] for m in socket.sent] == ["hosts", "rows"]  # hosts once: nothing changed on the 2nd tick


async def test_hosts_are_sent_even_without_sessions_so_each_gets_a_plus(aiohttp_client, monkeypatch):
    # an online host with no tmux sessions has no rows; the page still needs its header and +
    client = await make(aiohttp_client, monkeypatch, {"box": (True, [])})
    ws = await client.ws_connect("/api/events")
    msg = await next_kind(ws, "hosts")
    assert msg["hosts"] == [{"host": "box", "label": "box", "online": True, "background": False}]
    await ws.close()
    ws = await client.ws_connect("/api/events")  # a new tab gets them at once
    first = await asyncio.wait_for(ws.receive_json(), 3)
    assert first == {"t": "hosts", "hosts": [{"host": "box", "label": "box", "online": True, "background": False}]}
    await ws.close()


async def test_seen_rejects_a_body_that_is_not_an_object(aiohttp_client, monkeypatch):
    client = await make(aiohttp_client, monkeypatch, {"box": (True, [sess("a")])})
    for body in ([], "x", {"key": 5}):
        response = await client.post("/api/seen", json=body)
        assert response.status == 400 and await response.json() == {"ok": False, "error": "invalid request"}
    response = await client.post("/api/seen", data="not json", headers={"Content-Type": "application/json"})
    assert response.status == 400


async def test_ssh_commands_per_host_are_capped(monkeypatch, tmp_path):
    """sshd's MaxSessions (10) caps the channels on one ControlMaster connection, so a host's
    captures must not all run at once."""
    async def list_host(host):
        return True, [hosts.Session(host, f"s{i}", 1, False, i, 1000) for i in range(20)]
    running = {"now": 0, "most": 0}

    async def run(argv, stdin=None):
        running["now"] += 1
        running["most"] = max(running["most"], running["now"])
        await asyncio.sleep(0.01)
        running["now"] -= 1
        return 0, "line\n"
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(hosts, "background", no_background)
    monkeypatch.setattr(prompts, "_run", run)
    app = server.make_app(tmp_path / "auth.json", ["box"])
    await events.poll_once(app)
    assert 0 < running["most"] <= events.SLOTS


async def test_background_servers_are_listed_flagged_only_with_sessions(monkeypatch):
    # other tmux servers on a host ("box#web") join the poll; one with no sessions (a stale socket) is left out
    async def list_host(host):
        return {"box": (True, [sess("a")]), "box#web": (True, [hosts.Session("box#web", "pm", 1, False, 0, 100)]),
                "box#old": (True, [])}[host]

    async def background(host):
        return ["box#web", "box#old"]

    async def run(argv, stdin=None):
        return 0, "x\n"

    class Socket:
        def __init__(self):
            self.sent = []

        async def send_json(self, message):
            self.sent.append(message)

    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(hosts, "background", background)
    monkeypatch.setattr(prompts, "_run", run)
    app = web.Application()
    app["hosts"] = ["box"]
    events.setup(app)
    socket = Socket()
    app["sockets"].add(socket)
    await events.poll_once(app)
    sent = {m["t"]: m for m in socket.sent}
    assert sent["hosts"]["hosts"] == [{"host": "box", "label": "box", "online": True, "background": False},
                                      {"host": "box#web", "label": "box · web", "online": True, "background": True}]
    assert {r["key"]: r["background"] for r in sent["rows"]["set"]} == {"box/a": False, "box#web/pm": True}


async def test_each_page_gets_the_version_to_reload_on_a_redeploy(aiohttp_client, monkeypatch):
    client = await make(aiohttp_client, monkeypatch, {"box": (True, [])})
    client.server.app["version"] = "v1"
    ws = await client.ws_connect("/api/events")
    assert (await next_kind(ws, "version"))["v"] == "v1"
    await ws.close()
