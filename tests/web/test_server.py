import hashlib
import json
import secrets
import subprocess
import sys

from tmls.web import server


def write_creds(path):
    salt = secrets.token_hex(16)
    hashed = hashlib.scrypt(b"pw", salt=bytes.fromhex(salt), n=2**14, r=8, p=1).hex()
    path.write_text(json.dumps({"username": "me", "salt": salt, "hash": hashed, "secret": "s" * 64}))


def test_create_imports_without_textual():
    # the web server imports create; Textual belongs to the TUI only
    check = "import sys; import tmls.create; assert 'textual' not in sys.modules"
    assert subprocess.run([sys.executable, "-c", check]).returncode == 0


async def test_health_needs_no_login(aiohttp_client, tmp_path):
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    resp = await client.get("/healthz")
    assert resp.status == 200 and await resp.text() == "ok"


async def fail_from(client, address, times=server.MAX_FAILS):
    for _ in range(times):
        await client.post("/login", data={"username": "me", "password": "bad"},
                          headers={"X-Forwarded-For": address}, allow_redirects=False)


async def locked(client, *lines):
    """One X-Forwarded-For header line per argument."""
    response = await client.post("/login", data={"username": "me", "password": "bad"},
                                 headers=[("X-Forwarded-For", line) for line in lines], allow_redirects=False)
    return response.headers["Location"] == "/login?error=locked"


async def test_lockout_is_per_client_behind_a_trusted_proxy(aiohttp_client, tmp_path, monkeypatch):
    monkeypatch.setattr(server, "FAIL_DELAY", 0)
    write_creds(tmp_path / "auth.json")
    app = server.make_app(tmp_path / "auth.json", [])
    app["trusted_proxies"] = {"127.0.0.1"}
    client = await aiohttp_client(app)
    await fail_from(client, "10.0.0.9")
    assert await locked(client, "10.0.0.9")
    assert not await locked(client, "10.0.0.10")
    # a proxy that appends puts the address it saw last; what the client sent before it is untrusted
    assert await locked(client, "10.0.0.10, 10.0.0.9")
    assert await locked(client, "10.0.0.10", "10.0.0.9")  # ... also when the proxy adds its own line


async def test_trust_proxy_takes_a_network(aiohttp_client, tmp_path, monkeypatch):
    """The sidecar's address on its docker network changes; its subnet doesn't."""
    monkeypatch.setattr(server, "FAIL_DELAY", 0)
    write_creds(tmp_path / "auth.json")
    app = server.make_app(tmp_path / "auth.json", [])
    app["trusted_proxies"] = {"127.0.0.0/8"}  # the test client connects from 127.0.0.1
    client = await aiohttp_client(app)
    await fail_from(client, "10.0.0.9")
    assert await locked(client, "10.0.0.9")
    assert not await locked(client, "10.0.0.10")
    other = server.make_app(tmp_path / "auth.json", [])
    other["trusted_proxies"] = {"10.0.0.0/8", "not an address"}  # 127.0.0.1 isn't in it: header ignored
    client = await aiohttp_client(other)
    await fail_from(client, "10.0.0.9")
    assert await locked(client, "10.0.0.10")  # every request counts as 127.0.0.1, whatever the header says


def test_trust_proxy_argument_rejects_garbage():
    import pytest
    with pytest.raises(SystemExit):
        server.parse_args(["--bind", "127.0.0.1", "--trust-proxy", "evil"])
    assert server.parse_args(["--bind", "127.0.0.1", "--trust-proxy", "172.31.77.0/24",
                              "--trust-proxy", "10.0.0.1"]).trust_proxy == ["172.31.77.0/24", "10.0.0.1/32"]


async def test_forwarded_for_is_ignored_without_a_trusted_proxy(aiohttp_client, tmp_path, monkeypatch):
    monkeypatch.setattr(server, "FAIL_DELAY", 0)
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    await fail_from(client, "10.0.0.9")
    assert await locked(client, "10.0.0.10")  # a made-up header doesn't dodge the lockout


async def create_client(aiohttp_client, tmp_path, monkeypatch, result=None):
    made = []

    async def fake_create(host, name, folder, start):
        made.append((host, name, folder, start))
        return result

    async def fake_suggest(host, folder):
        return "repo-" + folder.rsplit("/", 1)[-1]
    monkeypatch.setattr(server.create, "create", fake_create)
    monkeypatch.setattr(server.create, "suggest_name", fake_suggest)
    monkeypatch.setattr(server.create, "load_presets", lambda: {"Opus plan": ("claude", "--model", "opus")})
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", ["box"]))
    await client.post("/login", data={"username": "me", "password": "pw"})
    return client, made


async def test_create_starts_a_shell_or_a_preset_on_a_configured_host(aiohttp_client, tmp_path, monkeypatch):
    client, made = await create_client(aiohttp_client, tmp_path, monkeypatch)
    body = {"host": "box", "folder": "~/x", "name": "work", "start": "shell"}
    response = await client.post("/api/create", json=body)
    assert response.status == 200 and await response.json() == {"ok": True}
    await client.post("/api/create", json={**body, "start": "Opus plan"})
    assert made == [("box", "work", "~/x", "shell"), ("box", "work", "~/x", ("claude", "--model", "opus"))]
    assert (await (await client.get("/api/config")).json())["presets"] == ["Opus plan"]


async def test_create_refuses_bad_input_before_running_anything(aiohttp_client, tmp_path, monkeypatch):
    client, made = await create_client(aiohttp_client, tmp_path, monkeypatch)
    body = {"host": "box", "folder": "~/x", "name": "work", "start": "shell"}
    unknown = await client.post("/api/create", json={**body, "host": "-oProxyCommand=bad"})
    assert unknown.status == 400
    colon = await client.post("/api/create", json={**body, "name": "a:b"})
    assert colon.status == 400 and await colon.json() == {"ok": False, "error": "names can't contain : or ."}
    assert (await client.post("/api/create", json={**body, "start": "rm -rf"})).status == 400
    assert (await client.post("/api/create", json={"host": "box"})).status == 400
    assert made == []


async def test_create_reports_what_went_wrong_on_the_host(aiohttp_client, tmp_path, monkeypatch):
    client, _ = await create_client(aiohttp_client, tmp_path, monkeypatch,
                                    result="a session named work already exists")
    response = await client.post("/api/create", json={"host": "box", "folder": "~", "name": "work",
                                                      "start": "claude"})
    assert response.status == 409
    assert await response.json() == {"ok": False, "error": "a session named work already exists"}


async def test_suggest_name_asks_the_host(aiohttp_client, tmp_path, monkeypatch):
    client, _ = await create_client(aiohttp_client, tmp_path, monkeypatch)
    response = await client.post("/api/suggest-name", json={"host": "box", "folder": "~/code/thing"})
    assert await response.json() == {"ok": True, "name": "repo-thing"}
    assert (await client.post("/api/suggest-name", json={"host": "nope", "folder": "~"})).status == 400


async def manage_client(aiohttp_client, tmp_path, monkeypatch, result=None, running=()):
    done = []

    async def fake_rename(host, old, new):
        done.append(("rename", host, old, new))
        return result

    async def fake_kill(host, name):
        done.append(("kill", host, name))
        return result

    async def fake_running(host, name):
        return list(running)
    monkeypatch.setattr(server.tmux_ops, "rename", fake_rename)
    monkeypatch.setattr(server.tmux_ops, "kill", fake_kill)
    monkeypatch.setattr(server.tmux_ops, "running_commands", fake_running)
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", ["box"]))
    await client.post("/login", data={"username": "me", "password": "pw"})
    return client, done


def test_server_imports_without_textual():
    check = "import sys; import tmls.web.server; assert 'textual' not in sys.modules"
    assert subprocess.run([sys.executable, "-c", check]).returncode == 0


async def test_rename_renames_on_a_configured_host_and_keeps_seen(aiohttp_client, tmp_path, monkeypatch):
    client, done = await manage_client(aiohttp_client, tmp_path, monkeypatch)
    client.server.app["state"].seen["box/old"] = 123
    response = await client.post("/api/rename", json={"host": "box", "name": "old", "new": " fresh "})
    assert response.status == 200 and await response.json() == {"ok": True}
    assert done == [("rename", "box", "old", "fresh")]
    # copied, not moved: a poll already in flight still finds the old key seen (no false alert)
    assert client.server.app["state"].seen == {"box/old": 123, "box/fresh": 123}


async def test_rename_and_kill_refuse_bad_input_before_running_anything(aiohttp_client, tmp_path, monkeypatch):
    client, done = await manage_client(aiohttp_client, tmp_path, monkeypatch)
    assert (await client.post("/api/rename", json={"host": "-oProxyCommand=bad", "name": "a", "new": "b"})).status == 400
    colon = await client.post("/api/rename", json={"host": "box", "name": "a", "new": "a:b"})
    assert colon.status == 400 and await colon.json() == {"ok": False, "error": "names can't contain : or ."}
    assert (await client.post("/api/rename", json={"host": "box", "name": "", "new": "b"})).status == 400
    assert (await client.post("/api/kill", json={"host": "nope", "name": "a"})).status == 400
    assert (await client.post("/api/kill", json={"host": "box"})).status == 400
    assert done == []


async def test_kill_asks_first_with_what_is_still_running(aiohttp_client, tmp_path, monkeypatch):
    client, done = await manage_client(aiohttp_client, tmp_path, monkeypatch, running=["claude", "node"])
    check = await client.post("/api/kill", json={"host": "box", "name": "work"})
    assert await check.json() == {"ok": True, "confirm": True, "running": ["claude", "node"]}
    assert done == []
    killed = await client.post("/api/kill", json={"host": "box", "name": "work", "confirm": True})
    assert killed.status == 200 and await killed.json() == {"ok": True}
    assert done == [("kill", "box", "work")]


async def test_rename_and_kill_report_what_went_wrong_on_the_host(aiohttp_client, tmp_path, monkeypatch):
    client, _ = await manage_client(aiohttp_client, tmp_path, monkeypatch, result="can't find session: work")
    rename = await client.post("/api/rename", json={"host": "box", "name": "work", "new": "x"})
    assert rename.status == 409 and await rename.json() == {"ok": False, "error": "can't find session: work"}
    kill = await client.post("/api/kill", json={"host": "box", "name": "work", "confirm": True})
    assert kill.status == 409 and await kill.json() == {"ok": False, "error": "can't find session: work"}


async def test_static_files_are_revalidated_so_a_deploy_reaches_the_browser(aiohttp_client, tmp_path):
    # Last-Modified alone lets a browser reuse an old app.js for hours without asking
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    await client.post("/login", data={"username": "me", "password": "pw"})
    response = await client.get("/static/app.js")
    assert response.status == 200 and response.headers["Cache-Control"] == "no-cache"


class FakeSocket:
    def __init__(self):
        self.sent = []

    async def send_json(self, message):
        self.sent.append(message)

    async def close(self):
        pass


async def test_rename_and_kill_tell_every_open_page(aiohttp_client, tmp_path, monkeypatch):
    # other tabs and the phone follow a rename, or learn the session ended, without waiting on a poll
    client, _ = await manage_client(aiohttp_client, tmp_path, monkeypatch)
    page = FakeSocket()
    client.server.app["sockets"].add(page)
    await client.post("/api/rename", json={"host": "box", "name": "old", "new": "fresh"})
    await client.post("/api/kill", json={"host": "box", "name": "fresh"})  # only asks: nothing to say yet
    await client.post("/api/kill", json={"host": "box", "name": "fresh", "confirm": True})
    assert page.sent == [{"t": "renamed", "old": "box/old", "new": "box/fresh", "name": "fresh"},
                         {"t": "killed", "key": "box/fresh"}]


async def test_failed_rename_or_kill_tells_no_one(aiohttp_client, tmp_path, monkeypatch):
    client, _ = await manage_client(aiohttp_client, tmp_path, monkeypatch, result="can't find session: old")
    page = FakeSocket()
    client.server.app["sockets"].add(page)
    client.server.app["state"].seen["box/old"] = 5
    await client.post("/api/rename", json={"host": "box", "name": "old", "new": "fresh"})
    await client.post("/api/kill", json={"host": "box", "name": "old", "confirm": True})
    assert page.sent == [] and client.server.app["state"].seen == {"box/old": 5}


async def test_names_tmux_would_mangle_are_refused(aiohttp_client, tmp_path, monkeypatch):
    client, done = await manage_client(aiohttp_client, tmp_path, monkeypatch)
    for new in ("x#{session_id}", "#(id)", "-foo", "a\0b"):
        response = await client.post("/api/rename", json={"host": "box", "name": "old", "new": new})
        assert response.status == 400, new
    assert (await client.post("/api/kill", json={"host": "box", "name": "a\0b"})).status == 400
    assert done == []
