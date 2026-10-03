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
