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
