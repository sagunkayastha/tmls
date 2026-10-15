"""Keeping uploads and pruning the upload cache. The prune and keep scripts are run for real, with
`sh` on this machine, under a HOME made inside pytest's tmp_path: never against a real home."""
import os
import subprocess
import time

import pytest

from tmls import hosts, image_paste
from tmls.web import server, uploads

from .test_chat import PNG, claude, make, sess

DAY = 86400


def touch(path, days_old, data=b"x"):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    stamp = time.time() - days_old * DAY
    os.utime(path, (stamp, stamp))
    return path


def link(path, target, days_old):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.symlink_to(target)
    stamp = time.time() - days_old * DAY
    os.utime(path, (stamp, stamp), follow_symlinks=False)
    return path


def run(script, home):
    env = {"PATH": os.environ["PATH"], "HOME": str(home)}
    return subprocess.run(["sh", "-c", script], env=env, capture_output=True, text=True, timeout=30)


def tree(root):
    """Every path under root (links not followed), relative."""
    found = set()
    for dirpath, dirnames, filenames in os.walk(root):
        for name in dirnames + filenames:
            found.add(os.path.relpath(os.path.join(dirpath, name), root))
    return found


# ---- names ----

def test_safe_name_keeps_the_name_readable_and_harmless():
    assert uploads.safe_name("RAM photo.JPG") == "RAM_photo.JPG"
    assert uploads.safe_name("../../etc/passwd") == "passwd"
    assert uploads.safe_name("C:\\Users\\me\\report final.pdf") == "report_final.pdf"
    assert uploads.safe_name(".bashrc") == "bashrc"
    assert uploads.safe_name("x.$(reboot)") == "x._reboot"
    assert uploads.safe_name("a'b\"c`d;e|f&g\n.txt") == "a_b_c_d_e_f_g.txt"
    assert uploads.safe_name("Bérénice 写真.png") == "Bérénice_写真.png"
    assert uploads.safe_name("") == "file" and uploads.safe_name(None) == "file" and uploads.safe_name("...") == "file"
    long = uploads.safe_name("a" * 300 + ".png")
    assert long == "a" * uploads.STEM_MAX + ".png"


def test_original_name_comes_back_out_of_the_cached_one():
    assert uploads.original_name("20261015-021500-0a1b2c3d-RAM_photo.jpg") == "RAM_photo.jpg"
    assert uploads.original_name("20261015-021500-0a1b2c3d.png") == "20261015-021500-0a1b2c3d.png"  # before names were kept
    assert uploads.original_name("20261012-101010-123456.png") == "20261012-101010-123456.png"     # image_paste's


def test_only_paths_into_the_upload_cache_are_taken():
    ok = "/home/u/.cache/tmls/images/20261015-021500-0a1b2c3d-a.png"
    assert uploads.cached_name(ok) == "20261015-021500-0a1b2c3d-a.png"
    assert uploads.cached_name("~/.cache/tmls/images/x.png") == "x.png"
    for bad in ("/home/u/.cache/tmls/images/../../.ssh/id_ed25519", "/home/u/.cache/tmls/images/.hidden",
                "/home/u/.cache/tmls/x.png", "/home/u/Documents/tmls/2026-10/a.png", "/etc/passwd",
                "/home/u/.cache/tmls/images/a b.png", "/home/u/.cache/tmls/images/$(reboot)",
                "/home/u/.cache/tmls/images/", "/home/u/.cache/tmls/images/a\0.png", None, 3, ["x"]):
        assert uploads.cached_name(bad) is None, bad


# ---- prune ----

def test_prune_script_refuses_bad_ages():
    for bad in (0, -1, uploads.MAX_DAYS + 1, True, "14", 14.0, None):
        with pytest.raises(ValueError):
            uploads.prune_script(bad)


def test_prune_script_shape():
    script = uploads.prune_script(14)
    assert script.count("find ") == 1 and 'find "$d" -mindepth 1 -maxdepth 1 -xdev -type f -mmin +20160 -print -delete' in script
    for word in ("rm ", "..", "Documents", "-exec", "xargs", "*", "cd "):
        assert word not in script, word
    find = script[script.index("find "):]
    for word in (" -L", " -H", "-follow", "-P"):
        assert word not in find, word


def make_home(tmp_path):
    home = tmp_path / "home"
    images = home / ".cache" / "tmls" / "images"
    touch(images / "old.png", 20)
    touch(images / "fifteen.pdf", 15)
    touch(images / ".hidden-old", 30)
    touch(images / "odd name\nwith newline", 30)
    touch(images / "-delete", 30)
    touch(images / "new.png", 1)
    touch(images / "thirteen.png", 13)
    touch(images / "sub" / "deep-old.png", 40)
    os.utime(images / "sub", (time.time() - 40 * DAY,) * 2)
    victim = touch(tmp_path / "outside" / "victim.txt", 40)
    link(images / "link-to-outside", victim, 40)
    link(images / "link-to-dir", tmp_path / "outside", 40)
    touch(home / "Documents" / "tmls" / "2026-01" / "kept.png", 300)
    touch(home / ".cache" / "tmls" / "beside.png", 300)
    touch(home / ".cache" / "other-app" / "x.png", 300)
    return home, images


def test_prune_deletes_only_old_files_directly_in_the_cache(tmp_path):
    home, images = make_home(tmp_path)
    before = tree(tmp_path)
    done = run(uploads.prune_script(14), home)
    assert done.returncode == 0, done.stderr
    gone = before - tree(tmp_path)
    assert gone == {f"home/.cache/tmls/images/{n}" for n in
                    ("old.png", "fifteen.pdf", ".hidden-old", "odd name\nwith newline", "-delete")}
    # what stays: young files, the sub folder and what is in it, links and their targets, kept files
    for kept in ("new.png", "thirteen.png", "sub/deep-old.png", "link-to-outside", "link-to-dir"):
        assert os.path.lexists(images / kept), kept
    assert (tmp_path / "outside" / "victim.txt").read_bytes() == b"x"
    assert (home / "Documents" / "tmls" / "2026-01" / "kept.png").exists()
    assert (home / ".cache" / "tmls" / "beside.png").exists() and (home / ".cache" / "other-app" / "x.png").exists()
    assert len([line for line in done.stdout.splitlines() if line.startswith("/")]) == 5  # one line each (the newline name: 2)


def test_prune_age_is_a_setting(tmp_path):
    home, images = make_home(tmp_path)
    assert run(uploads.prune_script(30), home).returncode == 0
    assert not (images / ".hidden-old").exists() and not (images / "-delete").exists()
    assert (images / "old.png").exists() and (images / "fifteen.pdf").exists()


@pytest.mark.parametrize("linked", [".cache/tmls/images", ".cache/tmls", ".cache"])
def test_prune_refuses_when_the_folder_or_a_parent_is_a_link(tmp_path, linked):
    home = tmp_path / "home"
    elsewhere = tmp_path / "elsewhere"
    touch(elsewhere / "precious.txt", 400)
    touch(elsewhere / "images" / "precious.txt", 400)
    touch(elsewhere / "tmls" / "images" / "precious.txt", 400)
    link(home / linked, {".cache/tmls/images": elsewhere, ".cache/tmls": elsewhere / "tmls",
                         ".cache": elsewhere}[linked] if linked != ".cache" else elsewhere, 400)
    if linked == ".cache":
        touch(elsewhere / "tmls" / "images" / "old.png", 400)
    before = tree(tmp_path)
    done = run(uploads.prune_script(14), home)
    assert done.returncode == 3 and "symlink" in done.stderr
    assert tree(tmp_path) == before


def test_prune_without_a_home_does_nothing(tmp_path):
    for home in ("", "/"):
        done = subprocess.run(["sh", "-c", uploads.prune_script(14)], env={"PATH": os.environ["PATH"], "HOME": home},
                              capture_output=True, text=True, timeout=30)
        assert done.returncode == 3 and done.stdout == ""
    assert run(uploads.prune_script(14), tmp_path / "no-such-home").returncode == 0


def test_prune_is_off_unless_asked(aiohttp_client, tmp_path):
    assert server.make_app(tmp_path / "auth.json", [])["prune_days"] is None
    args = server.parse_args(["--bind", "127.0.0.1"])
    assert args.prune_uploads is False and args.upload_days == 14
    args = server.parse_args(["--bind", "127.0.0.1", "--prune-uploads", "--upload-days", "30"])
    assert args.prune_uploads is True and args.upload_days == 30
    for bad in ("0", "-3", "x", "1.5", str(uploads.MAX_DAYS + 1)):
        with pytest.raises(SystemExit):
            server.parse_args(["--bind", "127.0.0.1", "--upload-days", bad])


async def test_no_prune_task_without_the_setting(aiohttp_client, tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(uploads, "prune_host", lambda *a: calls.append(a))
    monkeypatch.setattr(uploads, "PRUNE_FIRST", 0)
    app = server.make_app(tmp_path / "auth.json", ["box"])
    await aiohttp_client(app)
    assert "pruner" not in app and not calls


async def test_prune_runs_once_per_machine_when_on(aiohttp_client, tmp_path, monkeypatch):
    seen = []

    async def fake(argv, stdin=None, timeout=10):
        seen.append(argv)
        return b"/h/.cache/tmls/images/a\n/h/.cache/tmls/images/b\n"

    monkeypatch.setattr(image_paste, "_run", fake)
    monkeypatch.setattr(uploads, "PRUNE_FIRST", 0)
    monkeypatch.setattr(uploads, "PRUNE_EVERY", 3600)
    monkeypatch.setattr(hosts, "list_host", lambda h: _offline())
    app = server.make_app(tmp_path / "auth.json", ["box", "box#web", "nas"])
    app["prune_days"] = 20
    await aiohttp_client(app)
    for _ in range(50):
        if len(seen) >= 2:
            break
        await __import__("asyncio").sleep(0.02)
    assert sorted(argv[-2] for argv in seen) == ["box", "nas"]
    assert all(argv[-1] == uploads.prune_script(20) for argv in seen)
    assert await uploads.prune_host("box", 20) == 2


async def _offline():
    return False, []


def test_machines_once_each():
    assert uploads.machines(["local", "box", "box#web", "nas", "local#x", hosts.KITTY]) == ["local", "box", "nas"]


# ---- keep ----

def test_keep_copies_under_the_month_without_overwriting(tmp_path):
    home = tmp_path / "home"
    images = home / ".cache" / "tmls" / "images"
    touch(images / "20261015-021500-0a1b2c3d-RAM.jpg", 0, b"ram")
    touch(images / "20261015-021501-0a1b2c3e-RAM.jpg", 0, b"other ram")
    first = run(uploads.keep_script("20261015-021500-0a1b2c3d-RAM.jpg", "RAM.jpg", "2026-10"), home)
    assert first.returncode == 0, first.stderr
    month = home / "Documents" / "tmls" / "2026-10"
    assert first.stdout.strip() == str(month / "RAM.jpg") and (month / "RAM.jpg").read_bytes() == b"ram"
    # the same bytes again: the copy already there; other bytes under the same name: a suffix
    again = run(uploads.keep_script("20261015-021500-0a1b2c3d-RAM.jpg", "RAM.jpg", "2026-10"), home)
    assert again.stdout.strip() == str(month / "RAM.jpg")
    other = run(uploads.keep_script("20261015-021501-0a1b2c3e-RAM.jpg", "RAM.jpg", "2026-10"), home)
    assert other.stdout.strip() == str(month / "RAM-1.jpg") and (month / "RAM-1.jpg").read_bytes() == b"other ram"
    assert (month / "RAM.jpg").read_bytes() == b"ram"
    # the upload itself stays where it was (the prune takes it later)
    assert (images / "20261015-021500-0a1b2c3d-RAM.jpg").exists()


def test_keep_never_writes_through_a_link_or_reads_one(tmp_path):
    home = tmp_path / "home"
    images = home / ".cache" / "tmls" / "images"
    month = home / "Documents" / "tmls" / "2026-10"
    touch(images / "u.txt", 0, b"new")
    target = touch(tmp_path / "outside" / "target.txt", 0, b"keep me")
    link(month / "doc.txt", target, 0)
    done = run(uploads.keep_script("u.txt", "doc.txt", "2026-10"), home)
    assert done.stdout.strip() == str(month / "doc-1.txt") and target.read_bytes() == b"keep me"
    link(images / "sneaky.txt", target, 0)
    refused = run(uploads.keep_script("sneaky.txt", "x.txt", "2026-10"), home)
    assert refused.returncode == 3 and not (month / "x.txt").exists()


def test_keep_script_refuses_bad_input():
    for cached, month in (("../x", "2026-10"), (".x", "2026-10"), ("a b", "2026-10"), ("a.png", "2026-1"), ("a.png", "../..")):
        with pytest.raises(ValueError):
            uploads.keep_script(cached, "a.png", month)
    script = uploads.keep_script("a.png", "x'; rm -rf ~; '.png", "2026-10")
    assert "rm -rf" not in script.replace("rm_-rf", "")


async def test_keep_over_http_end_to_end(aiohttp_client, monkeypatch, tmp_path):
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude(), sess("shell")])})
    monkeypatch.setattr(hosts, "run_argv", lambda host, script: ["sh", "-c", script])  # "box" is this machine, HOME=tmp_path
    up = await client.post("/api/chat/upload", params={"key": "box/work", "name": "RAM stick.JPG"}, data=PNG)
    path = (await up.json())["path"]
    assert path.startswith(str(tmp_path / ".cache" / "tmls" / "images")) and path.endswith("-RAM_stick.jpg")
    kept = await client.post("/api/chat/keep", json={"key": "box/work", "path": path})
    body = await kept.json()
    assert kept.status == 200 and body["ok"] and body["path"].endswith("/RAM_stick.jpg")
    assert body["path"].startswith(str(tmp_path / "Documents" / "tmls") + "/")
    assert open(body["path"], "rb").read() == PNG
    named = await client.post("/api/chat/keep", json={"key": "box/work", "path": path, "name": "my ram.jpg"})
    assert (await named.json())["path"].endswith("/my_ram.jpg")
    for bad in ({"key": "box/work", "path": "/etc/passwd"}, {"key": "box/work"}, {"path": path},
                {"key": "box/work", "path": path, "name": 3}, {"key": "elsewhere/x", "path": path}, [path]):
        assert (await client.post("/api/chat/keep", json=bad)).status == 400, bad
    assert (await client.post("/api/chat/keep", json={"key": "box/gone", "path": path})).status == 404
    gone = await client.post("/api/chat/keep", json={"key": "box/work", "path": path.replace("RAM", "XX")})
    assert gone.status == 502 and "not on this host" in (await gone.json())["error"]


async def test_keep_needs_login(aiohttp_client, monkeypatch, tmp_path):
    client = await make(aiohttp_client, monkeypatch, tmp_path, {"box": (True, [claude()])}, login=False)
    assert (await client.post("/api/chat/keep", json={"key": "box/work", "path": "/h/.cache/tmls/images/a"})).status == 401
