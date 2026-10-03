"""Logged in to tmls means logged in to sketchpad too: same credentials file, so the page hands the
browser sketchpad's own session cookie (sp_session) and the Sketch tab needs no second login."""
import base64
import hashlib
import hmac
import time

from tmls.web import auth, server

from .test_auth import write_creds


def sketchpad_user(cookie, secret="s" * 64):
    """What sketchpad's verify_cookie would make of it (its format: no "tmls|" prefix)."""
    payload, _, sig = cookie.rpartition(".")
    assert hmac.compare_digest(sig, hmac.new(secret.encode(), payload.encode(), hashlib.sha256).hexdigest())
    expires, _, user = base64.urlsafe_b64decode(payload).decode().partition("|")
    assert int(expires) > time.time()
    return user


async def logged_in(aiohttp_client, tmp_path, sketchpad):
    write_creds(tmp_path / "auth.json")
    app = server.make_app(tmp_path / "auth.json", [])
    app["sketchpad"] = sketchpad
    client = await aiohttp_client(app)
    await client.post("/login", data={"username": "me", "password": "pw"}, allow_redirects=False)
    return client


async def test_the_page_hands_out_sketchpads_cookie(aiohttp_client, tmp_path):
    client = await logged_in(aiohttp_client, tmp_path, ["http://127.0.0.1:8790"])
    resp = await client.get("/")
    morsel = resp.cookies["sp_session"]
    assert sketchpad_user(morsel.value) == "me"
    assert morsel["httponly"] and morsel["path"] == "/"
    assert not morsel["domain"]  # same host as sketchpad (ports don't matter to cookies)


async def test_a_sibling_name_shares_the_parent_domain(aiohttp_client, tmp_path):
    client = await logged_in(aiohttp_client, tmp_path, ["http://127.0.0.1:8790", "https://sketchpad.example.com"])
    resp = await client.get("/", headers={"Host": "tmls.example.com"})
    assert resp.cookies["sp_session"]["domain"] == "example.com"


async def test_no_sketchpad_no_cookie_and_logout_clears_it(aiohttp_client, tmp_path):
    client = await logged_in(aiohttp_client, tmp_path, [])
    assert "sp_session" not in (await client.get("/")).cookies
    client = await logged_in(aiohttp_client, tmp_path, ["http://127.0.0.1:8790"])
    await client.get("/")
    resp = await client.post("/logout", allow_redirects=False)
    assert resp.cookies["sp_session"].value == ""


def test_a_sketchpad_cookie_still_does_not_open_tmls():
    # the reverse must stay closed: sketchpad's cookie is not a tmls login
    cookie = auth.sketchpad_cookie("s" * 64, "me")
    assert auth.verify_cookie("s" * 64, cookie) is None
