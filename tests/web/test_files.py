"""Pictures named by path in a reply (/api/chat/file). The read script runs for real with `sh` on
this machine, under a HOME made inside pytest's tmp_path: never against a real home."""
import os

import pytest

from tmls import hosts, image_paste
from tmls.web import files

from .test_chat import claude, make

PNG = b"\x89PNG\r\n\x1a\n" + b"\0" * 32
JPEG = b"\xff\xd8\xff\xe0" + b"\0" * 32
GIF = b"GIF89a" + b"\0" * 32
WEBP = b"RIFF\x10\0\0\0WEBPVP8 " + b"\0" * 32


def put(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return path


async def client_for(aiohttp_client, monkeypatch, tmp_path, login=True):
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude()])}, login=login)
    monkeypatch.setattr(hosts, "run_argv", lambda host, script: ["sh", "-c", script])  # "box" is this machine
    return client


def get(client, path, key="box/work", **headers):
    return client.get("/api/chat/file", params={"key": key, "path": path}, headers=headers)


def test_sniff_knows_the_four_types_and_nothing_else():
    assert files.sniff(PNG) == "image/png" and files.sniff(JPEG) == "image/jpeg"
    assert files.sniff(GIF) == "image/gif" and files.sniff(WEBP) == "image/webp"
    for other in (b"", b"<svg xmlns=", b"%PDF-1.7", b"RIFF\0\0\0\0WAVE", b"just text"):
        assert files.sniff(other) is None


def test_check_path_refuses_what_can_never_be_a_picture():
    assert files.check_path("/data/x/plot.png") == "/data/x/plot.png"
    assert files.check_path("~/shots/a b.JPG") == "\"$HOME\"/'shots/a b.JPG'"
    for bad in ("plot.png", "./plot.png", "shots/a.png", "~user/a.png", "~/", "/etc/passwd", "/x/a.svg",
                "/x/a.png.txt", "/x/a\n.png", "/x/a\0.png", "", None, 3, "~/.png/"):
        assert files.check_path(bad) is None, bad


async def test_serves_a_picture_by_absolute_and_home_path(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    put(tmp_path / "shots" / "s19.png", PNG)
    put(tmp_path / "shots" / "photo.jpeg", JPEG)
    one = await get(client, str(tmp_path / "shots" / "s19.png"))
    assert one.status == 200 and one.content_type == "image/png" and await one.read() == PNG
    assert one.headers["X-Content-Type-Options"] == "nosniff" and one.headers["ETag"]
    two = await get(client, "~/shots/photo.jpeg")
    assert two.status == 200 and two.content_type == "image/jpeg" and await two.read() == JPEG


async def test_the_type_comes_from_the_bytes_not_the_name(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    put(tmp_path / "really-a-gif.png", GIF)
    found = await get(client, str(tmp_path / "really-a-gif.png"))
    assert found.status == 200 and found.content_type == "image/gif"


async def test_refuses_relative_paths_and_other_extensions(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    put(tmp_path / "notes.txt", PNG)
    put(tmp_path / "pic.png", PNG)
    for bad in (str(tmp_path / "notes.txt"), "pic.png", "./pic.png", "../pic.png", str(tmp_path / "pic.svg")):
        response = await get(client, bad)
        assert response.status == 400, bad
        assert response.content_type == "application/json"


async def test_refuses_bytes_that_are_not_a_picture(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    put(tmp_path / "fake.png", b"#!/bin/sh\necho not a picture\n")
    put(tmp_path / "empty.jpg", b"")
    put(tmp_path / "svg.webp", b"<svg xmlns='http://www.w3.org/2000/svg'/>")
    for name in ("fake.png", "empty.jpg", "svg.webp"):
        response = await get(client, str(tmp_path / name))
        assert response.status == 404, name
        assert b"echo" not in await response.read()


async def test_refuses_a_file_over_the_cap(aiohttp_client, monkeypatch, tmp_path):
    monkeypatch.setattr(files, "MAX_BYTES", 1000)
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    put(tmp_path / "big.png", PNG + b"\0" * 2000)
    put(tmp_path / "fits.png", PNG + b"\0" * 900)
    assert (await get(client, str(tmp_path / "big.png"))).status == 413
    assert (await get(client, str(tmp_path / "fits.png"))).status == 200


async def test_symlinks_must_lead_to_a_picture(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    secret = put(tmp_path / "secret.txt", b"password=hunter2")
    disguised = put(tmp_path / "disguised.txt", PNG)  # picture bytes, but not a picture's name
    real = put(tmp_path / "real.png", PNG)
    (tmp_path / "to-secret.png").symlink_to(secret)
    (tmp_path / "to-disguised.png").symlink_to(disguised)
    (tmp_path / "to-real.png").symlink_to(real)
    (tmp_path / "dangling.png").symlink_to(tmp_path / "nowhere.png")
    for name in ("to-secret.png", "to-disguised.png", "dangling.png"):
        response = await get(client, str(tmp_path / name))
        assert response.status == 404, name
        assert b"hunter2" not in await response.read()
    assert (await get(client, str(tmp_path / "to-real.png"))).status == 200


async def test_no_folders_no_listing_no_devices(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    put(tmp_path / "dir.png" / "inside.png", PNG)
    (tmp_path / "pipe.png").parent.mkdir(exist_ok=True)
    os.mkfifo(tmp_path / "pipe.png")
    for name in ("dir.png", "pipe.png", "missing.png"):
        response = await get(client, str(tmp_path / name))
        assert response.status == 404, name
        assert b"inside" not in await response.read()
    assert (await get(client, "/dev/null.png")).status == 404


async def test_cached_by_mtime_and_revalidated(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    pic = put(tmp_path / "plot.png", PNG)
    os.utime(pic, (1_700_000_000, 1_700_000_000))
    first = await get(client, str(pic))
    etag = first.headers["ETag"]
    seen = []
    real_run = image_paste._run

    async def counting(argv, stdin=None, timeout=10):
        out = await real_run(argv, stdin, timeout)
        seen.append(out)
        return out

    monkeypatch.setattr(image_paste, "_run", counting)
    again = await get(client, str(pic))
    assert again.status == 200 and await again.read() == PNG and again.headers["ETag"] == etag
    assert seen[-1].rstrip().endswith(b"same")  # the host did not send the bytes again
    assert (await get(client, str(pic), **{"If-None-Match": etag})).status == 304
    pic.write_bytes(GIF)
    os.utime(pic, (1_700_000_100, 1_700_000_100))
    changed = await get(client, str(pic), **{"If-None-Match": etag})
    assert changed.status == 200 and await changed.read() == GIF and changed.headers["ETag"] != etag
    pic.unlink()
    assert (await get(client, str(pic))).status == 404
    assert ("box", str(pic)) not in client.app["file_cache"].items


async def test_needs_login_and_a_listed_session(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path, login=False)
    put(tmp_path / "a.png", PNG)
    assert (await get(client, str(tmp_path / "a.png"))).status == 401
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)
    assert (await get(client, str(tmp_path / "a.png"), key="box/gone")).status == 404
    assert (await get(client, str(tmp_path / "a.png"), key="elsewhere/x")).status == 400


async def test_unreachable_host_is_a_502(aiohttp_client, monkeypatch, tmp_path):
    client = await client_for(aiohttp_client, monkeypatch, tmp_path)

    async def down(argv, stdin=None, timeout=10):
        raise image_paste.ImageError("ssh: connect to host box: No route to host")

    monkeypatch.setattr(image_paste, "_run", down)
    assert (await get(client, "/data/x.png")).status == 502


def test_cache_drops_the_oldest_beyond_its_size():
    cache = files.Cache(limit=10)
    cache.put("a", "1 1", "image/png", b"12345")
    cache.put("b", "1 1", "image/png", b"12345")
    cache.get("a")
    cache.put("c", "1 1", "image/png", b"123")
    assert set(cache.items) == {"a", "c"} and cache.size == 8
    cache.put("huge", "1 1", "image/png", b"x" * 11)
    assert "huge" not in cache.items


@pytest.mark.parametrize("out", [b"", b"garbage", b"ok 1 2\n/x.png\nweird\n", b"ok x y\n/x.png\ndata\n"])
def test_parse_refuses_nonsense(out):
    with pytest.raises(ValueError):
        files.parse(out)
