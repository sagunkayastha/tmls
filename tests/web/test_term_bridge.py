import asyncio
import hashlib
import json
import os
import secrets

from aiohttp import WSMsgType
import pytest

from tmls import hosts
from tmls.web import server, term

pytestmark = pytest.mark.filterwarnings("ignore:This process.*multi-threaded.*forkpty:DeprecationWarning")


@pytest.fixture
async def client(aiohttp_client, monkeypatch, tmp_path):
    salt = secrets.token_hex(16)
    hashed = hashlib.scrypt(b"pw", salt=bytes.fromhex(salt), n=2**14, r=8, p=1).hex()
    (tmp_path / "auth.json").write_text(json.dumps({"username": "me", "salt": salt,
                                                    "hash": hashed, "secret": "s" * 64}))

    async def offline(_host):
        return False, []

    monkeypatch.setattr(hosts, "list_host", offline)
    app = server.make_app(tmp_path / "auth.json", ["box"])
    app["poll_interval"] = 100
    app["term_queued"] = []
    attach = {"fn": None}
    app["attach_argv"] = lambda host, name: attach["fn"](host, name)
    browser = await aiohttp_client(app)
    await browser.post("/login", data={"username": "me", "password": "pw"})
    return browser, app, attach


async def output_until(ws, wanted, timeout=5):
    data = bytearray()
    async with asyncio.timeout(timeout):
        while wanted not in data:
            msg = await ws.receive()
            if msg.type == WSMsgType.BINARY:
                data.extend(msg.data)
            elif msg.type == WSMsgType.TEXT and '"exit"' in msg.data:
                break
    return bytes(data)


async def test_typing_reaches_pty_and_odd_name_is_one_argument(client):
    browser, app, attach = client
    name = 'my "odd" name;x'
    attach["fn"] = lambda _host, n: ["sh", "-c", "echo attached-$0; exec cat", n]
    ws = await browser.ws_connect("/api/term?host=box&name=my+%22odd%22+name%3Bx")
    assert b'attached-my "odd" name;x' in await output_until(ws, b'attached-my "odd" name;x')
    await ws.send_json({"t": "in", "d": "hello\n"})
    assert b"hello" in await output_until(ws, b"hello")
    await ws.close()


async def test_resize_reaches_child(client):
    browser, app, attach = client
    attach["fn"] = lambda _host, _name: ["sh", "-c", "sleep 0.2; stty size"]
    ws = await browser.ws_connect("/api/term?host=box&name=alpha")
    await ws.send_json({"t": "size", "cols": 100, "rows": 30})
    assert b"30 100" in await output_until(ws, b"30 100")
    await ws.close()


async def test_closing_socket_reaps_child(client):
    browser, app, attach = client
    attach["fn"] = lambda _host, _name: ["sh", "-c", "exec cat"]
    ws = await browser.ws_connect("/api/term?host=box&name=alpha")
    async with asyncio.timeout(2):
        while not app["ptys"]:
            await asyncio.sleep(.01)
    pid = next(iter(app["ptys"]))
    await ws.close()
    async with asyncio.timeout(3):
        while pid in app["ptys"]:
            await asyncio.sleep(.01)
    with pytest.raises(ProcessLookupError):
        os.kill(pid, 0)


async def test_child_exit_and_unknown_host(client):
    browser, app, attach = client
    attach["fn"] = lambda _host, _name: ["sh", "-c", "echo bye"]
    denied = await browser.ws_connect("/api/term?host=nope&name=alpha")
    await denied.receive()
    assert denied.close_code == 4404 and not app["ptys"]
    ws = await browser.ws_connect("/api/term?host=box&name=alpha")
    assert b"bye" in await output_until(ws, b"bye")
    msg = await ws.receive(timeout=2)
    assert msg.type == WSMsgType.TEXT and json.loads(msg.data) == {"t": "exit"}


async def test_term_rejects_unlogged_and_cross_origin_before_pty(client):
    browser, app, attach = client
    attach["fn"] = lambda _host, _name: ["sh", "-c", "exec cat"]
    denied = await browser.ws_connect("/api/term?host=box&name=alpha",
                                      headers={"Origin": "https://evil.example"})
    await denied.receive()
    assert denied.close_code == 4403 and not app["ptys"]
    browser.session.cookie_jar.clear()
    denied = await browser.ws_connect("/api/term?host=box&name=alpha")
    await denied.receive()
    assert denied.close_code == 4401 and not app["ptys"]


async def test_big_output_burst(client):
    browser, app, attach = client
    attach["fn"] = lambda _host, _name: ["sh", "-c", "head -c 2000000 /dev/zero | tr '\\0' x"]
    ws = await browser.ws_connect("/api/term?host=box&name=alpha")
    size = 0
    async with asyncio.timeout(10):
        while True:
            msg = await ws.receive()
            if msg.type == WSMsgType.BINARY:
                size += len(msg.data)
            elif msg.type == WSMsgType.TEXT and json.loads(msg.data) == {"t": "exit"}:
                break
    assert size == 2_000_000


async def test_big_paste_is_not_dropped(client):
    browser, app, attach = client
    attach["fn"] = lambda _host, _name: ["sh", "-c", "stty -icanon -echo; echo ready; head -c 200000 | wc -c"]
    ws = await browser.ws_connect("/api/term?host=box&name=alpha")
    await output_until(ws, b"ready")
    await ws.send_json({"t": "in", "d": "x" * 200000})
    assert b"200000" in await output_until(ws, b"200000", timeout=10)
    await ws.close()


async def test_stalled_reader_pauses_the_pty(client):
    """A browser that stops reading must not make the server buffer output without limit."""
    browser, app, attach = client
    attach["fn"] = lambda _host, _name: ["sh", "-c", "exec yes"]
    ws = await browser.ws_connect("/api/term?host=box&name=alpha", max_msg_size=0)
    await asyncio.sleep(1.5)  # read nothing meanwhile
    assert max(app["term_queued"]) <= 2 * term.HIGH_WATER
    await ws.close()


async def test_nul_in_name_is_refused(client):
    browser, app, attach = client
    attach["fn"] = lambda _host, n: ["sh", "-c", "echo $0", n]
    ws = await browser.ws_connect("/api/term?host=box&name=a%00b")
    await ws.receive()
    assert ws.close_code == 4404 and not app["ptys"]
