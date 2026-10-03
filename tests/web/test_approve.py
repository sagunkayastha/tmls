import hashlib
import json
import secrets
from unittest.mock import AsyncMock

from tmls.web import server


async def client_logged_in(aiohttp_client, tmp_path):
    salt = secrets.token_hex(16)
    hashed = hashlib.scrypt(b"pw", salt=bytes.fromhex(salt), n=2**14, r=8, p=1).hex()
    (tmp_path / "auth.json").write_text(json.dumps({"username": "me", "salt": salt,
                                                    "hash": hashed, "secret": "s" * 64}))
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    await client.post("/login", data={"username": "me", "password": "pw"})
    return client


async def test_approve_happy_path_and_changed_prompt(aiohttp_client, monkeypatch, tmp_path):
    client = await client_logged_in(aiohttp_client, tmp_path)
    answer = AsyncMock(side_effect=[None, "Season-36 isn't asking that any more"])
    monkeypatch.setattr(server.approve, "answer", answer)
    body = {"host": "local", "name": "Season-36", "shown": ["Bash", "rm x"], "yes": True}
    first = await client.post("/api/approve", json=body)
    assert first.status == 200 and await first.json() == {"ok": True}
    second = await client.post("/api/approve", json=body)
    assert second.status == 409 and "isn't asking" in (await second.json())["error"]
    assert answer.call_count == 2
    answer.assert_any_await("local", "Season-36", ["Bash", "rm x"], True)


async def test_approve_missing_fields_and_cross_origin(aiohttp_client, monkeypatch, tmp_path):
    client = await client_logged_in(aiohttp_client, tmp_path)
    answer = AsyncMock()
    monkeypatch.setattr(server.approve, "answer", answer)
    missing = await client.post("/api/approve", json={"host": "local", "name": "x", "yes": False})
    assert missing.status == 400
    wrong = await client.post("/api/approve", json={"host": "local", "name": "x",
                                                     "shown": ["Bash"], "yes": False},
                              headers={"Origin": "https://evil.example"})
    assert wrong.status == 403
    unknown = await client.post("/api/approve", json={"host": "-oProxyCommand=bad", "name": "x",
                                                       "shown": ["Bash"], "yes": False})
    assert unknown.status == 400
    answer.assert_not_awaited()
