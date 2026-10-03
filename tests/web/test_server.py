import hashlib
import json
import secrets

from tmls.web import auth, server


def write_creds(path):
    salt = secrets.token_hex(16)
    hashed = hashlib.scrypt(b"pw", salt=bytes.fromhex(salt), n=2**14, r=8, p=1).hex()
    path.write_text(json.dumps({"username": "me", "salt": salt, "hash": hashed, "secret": "s" * 64}))


async def test_health_needs_no_login(aiohttp_client, tmp_path):
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    resp = await client.get("/healthz")
    assert resp.status == 200 and await resp.text() == "ok"


async def test_lockout_sends_the_form_back_with_a_reason(aiohttp_client, tmp_path, monkeypatch):
    monkeypatch.setattr(server, "FAIL_DELAY", 0)
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    for _ in range(server.MAX_FAILS):
        await client.post("/login", data={"username": "me", "password": "bad"}, allow_redirects=False)
    response = await client.post("/login", data={"username": "me", "password": "bad"}, allow_redirects=False)
    assert response.status == 302 and response.headers["Location"] == "/login?error=locked"
    assert auth.COOKIE not in response.cookies


async def fail_from(client, address, times=server.MAX_FAILS):
    for _ in range(times):
        await client.post("/login", data={"username": "me", "password": "bad"},
                          headers={"X-Forwarded-For": address}, allow_redirects=False)


async def locked(client, address):
    response = await client.post("/login", data={"username": "me", "password": "bad"},
                                 headers={"X-Forwarded-For": address}, allow_redirects=False)
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


async def test_forwarded_for_is_ignored_without_a_trusted_proxy(aiohttp_client, tmp_path, monkeypatch):
    monkeypatch.setattr(server, "FAIL_DELAY", 0)
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    await fail_from(client, "10.0.0.9")
    assert await locked(client, "10.0.0.10")  # a made-up header doesn't dodge the lockout
