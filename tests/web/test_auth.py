import hashlib
import json
import secrets

from tmls.web import auth, server


def write_creds(path, user="me", password="pw"):
    salt = secrets.token_hex(16)
    hashed = hashlib.scrypt(password.encode(), salt=bytes.fromhex(salt), n=2**14, r=8, p=1).hex()
    path.write_text(json.dumps({"username": user, "salt": salt, "hash": hashed, "secret": "s" * 64}))


def test_cookie_round_trip_and_expiry():
    cookie = auth.make_cookie("k", "me", now=1000)
    assert auth.verify_cookie("k", cookie, now=1001) == "me"
    assert auth.verify_cookie("k", cookie, now=1000 + auth.SESSION_TTL + 1) is None
    assert auth.verify_cookie("other", cookie, now=1001) is None


def test_garbage_cookie_is_none():
    for value in ["", "x", "a.b", "!!!.sig", "Zm9v.sig"]:
        assert auth.verify_cookie("k", value) is None


def test_login_against_sketchpad_credentials(tmp_path):
    path = tmp_path / "auth.json"
    assert auth.load(path) is None
    write_creds(path)
    assert auth.check_login(auth.load(path), "me", "pw")
    assert not auth.check_login(auth.load(path), "me", "bad")


async def test_page_redirects_to_login_then_logs_in(aiohttp_client, tmp_path):
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    response = await client.get("/", allow_redirects=False)
    assert response.status == 302 and response.headers["Location"] == "/login"
    response = await client.post("/login", data={"username": "me", "password": "bad"}, allow_redirects=False)
    assert response.status == 302 and response.headers["Location"] == "/login?error=1"
    response = await client.post("/login", data={"username": "me", "password": "pw"}, allow_redirects=False)
    assert response.status == 302 and auth.COOKIE in response.cookies
    assert (await client.get("/", allow_redirects=False)).status == 200


async def test_missing_credentials_reject_login(aiohttp_client, tmp_path):
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    response = await client.post("/login", data={"username": "me", "password": "pw"})
    assert response.status == 401 and "no credentials" in await response.text()


async def test_websocket_without_login_closes_4401(aiohttp_client, tmp_path):
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    ws = await client.ws_connect("/api/events")
    await ws.receive()
    assert ws.close_code == 4401


async def test_cross_origin_websocket_rejected(aiohttp_client, tmp_path):
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    await client.post("/login", data={"username": "me", "password": "pw"})
    ws = await client.ws_connect("/api/events", headers={"Origin": "https://evil.example"})
    await ws.receive()
    assert ws.close_code == 4403


async def test_cross_origin_post_rejected(aiohttp_client, tmp_path):
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    await client.post("/login", data={"username": "me", "password": "pw"})
    response = await client.post("/api/seen", json={"key": "x/y"}, headers={"Origin": "https://evil.example"})
    assert response.status == 403


async def test_logged_in_websocket_gets_rows(aiohttp_client, tmp_path):
    write_creds(tmp_path / "auth.json")
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    await client.post("/login", data={"username": "me", "password": "pw"})
    ws = await client.ws_connect("/api/events")
    assert (await ws.receive_json())["t"] == "rows"
