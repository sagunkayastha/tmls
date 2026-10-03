"""The Android app's update files: /app/latest.json and /app/tmls.apk, behind the login."""
import json

from tmls.web import server

from .test_approve import client_logged_in


async def test_app_files_need_the_login(aiohttp_client, tmp_path):
    client = await aiohttp_client(server.make_app(tmp_path / "auth.json", []))
    for path in ("/app/latest.json", "/app/tmls.apk", "/app/update"):
        resp = await client.get(path, allow_redirects=False)
        assert resp.status == 302 and resp.headers["Location"] == "/login", path


async def test_nothing_published_is_404(aiohttp_client, monkeypatch, tmp_path):
    monkeypatch.setattr(server, "APK_DIR", tmp_path / "apk")
    client = await client_logged_in(aiohttp_client, tmp_path)
    for path in ("/app/latest.json", "/app/tmls.apk"):
        resp = await client.get(path)
        assert resp.status == 404, path


async def test_manifest_and_apk_are_served_as_published(aiohttp_client, monkeypatch, tmp_path):
    apk_dir = tmp_path / "apk"
    apk_dir.mkdir()
    monkeypatch.setattr(server, "APK_DIR", apk_dir)
    client = await client_logged_in(aiohttp_client, tmp_path)
    manifest = {"versionCode": 7, "versionName": "1.261002.7", "size": 3, "sha256": "ab" * 32}
    (apk_dir / "latest.json").write_text(json.dumps(manifest))
    (apk_dir / "tmls.apk").write_bytes(b"PK\x03")
    resp = await client.get("/app/latest.json")
    assert resp.status == 200 and await resp.json() == manifest
    assert resp.headers["Cache-Control"] == "no-store"  # the updater must see each new build
    resp = await client.get("/app/tmls.apk")
    assert resp.status == 200 and await resp.read() == b"PK\x03"
    assert resp.headers["Content-Type"] == "application/vnd.android.package-archive"
    assert 'filename="tmls.apk"' in resp.headers["Content-Disposition"]


async def test_update_link_lands_a_browser_on_the_page(aiohttp_client, tmp_path):
    # the app intercepts /app/update itself; anything else following it just gets the page
    client = await client_logged_in(aiohttp_client, tmp_path)
    resp = await client.get("/app/update", allow_redirects=False)
    assert resp.status == 303 and resp.headers["Location"] == "/"


def test_apk_dir_argument():
    assert str(server.parse_args(["--bind", "127.0.0.1", "--apk-dir", "/x/apk"]).apk_dir) == "/x/apk"


async def test_config_says_when_an_app_is_published(aiohttp_client, monkeypatch, tmp_path):
    # the page offers "Get the Android app" only when there is one to get
    apk_dir = tmp_path / "apk"
    apk_dir.mkdir()
    monkeypatch.setattr(server, "APK_DIR", apk_dir)
    client = await client_logged_in(aiohttp_client, tmp_path)
    assert (await (await client.get("/api/config")).json())["app"] is False
    (apk_dir / "tmls.apk").write_bytes(b"PK")
    assert (await (await client.get("/api/config")).json())["app"] is True
