import os

import pytest

from tmls import hosts
from tmls import viewer


async def test_local_relative_file_uses_tmux_pane_cwd(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    (repo / "src").mkdir(parents=True)
    (repo / "src" / "term.py").write_text("first\nsecond\n")
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    tmux = bin_dir / "tmux"
    # answers only an exact target: "-t Season-36" would also match "Season-36b"
    tmux.write_text("#!/bin/sh\ncase \"$*\" in *'-t =Season-36: '*) printf '%s\\n' \"$TMLS_TEST_CWD\";; "
                    "*) exit 1;; esac\n")
    tmux.chmod(0o755)
    monkeypatch.setenv("PATH", f"{bin_dir}:{os.environ['PATH']}")
    monkeypatch.setenv("TMLS_TEST_CWD", str(repo))

    path, text = await viewer.load_file(hosts.LOCAL, "Season-36", "src/term.py")
    assert path == str(repo / "src" / "term.py")
    assert text == "first\nsecond\n"


async def test_remote_file_with_spaces_and_semicolon_uses_ssh_safely(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    repo.mkdir()
    name = "file name;touch INJECTED.py"
    (repo / name).write_text("remote text\n")
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    ssh = bin_dir / "ssh"
    ssh.write_text("#!/bin/sh\nwhile [ \"$1\" = -o ]; do shift 2; done\nshift\nTMLS_REMOTE=1 exec sh -c \"$1\"\n")
    ssh.chmod(0o755)
    tmux = bin_dir / "tmux"
    tmux.write_text("#!/bin/sh\n[ \"$TMLS_REMOTE\" = 1 ] || exit 22\nprintf '%s\\n' \"$TMLS_TEST_CWD\"\n")
    tmux.chmod(0o755)
    monkeypatch.setenv("PATH", f"{bin_dir}:{os.environ['PATH']}")
    monkeypatch.setenv("TMLS_TEST_CWD", str(repo))

    path, text = await viewer.load_file("archbox", "Season-36", name)
    assert path == str(repo / name)
    assert text == "remote text\n"
    assert not (repo / "INJECTED.py").exists()


async def test_absolute_local_file_opens_without_a_tmux_cwd_query(tmp_path):
    file = tmp_path / "term.py"
    file.write_text("line 1\n")
    assert await viewer.load_file(hosts.LOCAL, "no-such-session", str(file)) == (str(file), "line 1\n")


@pytest.mark.parametrize("content,reason", [
    pytest.param(b"text\0more", "binary file", id="binary"),
    pytest.param(b"x" * 2_000_001, "file too large", id="large"),
])
async def test_file_viewer_rejects_binary_and_oversized_files(tmp_path, content, reason):
    file = tmp_path / "file.dat"
    file.write_bytes(content)
    with pytest.raises(viewer.ViewerError, match=reason):
        await viewer.load_file(hosts.LOCAL, "no-such-session", str(file))


async def test_file_viewer_reports_missing_file(tmp_path):
    with pytest.raises(viewer.ViewerError, match="No such file"):
        await viewer.load_file(hosts.LOCAL, "no-such-session", str(tmp_path / "missing.py"))


@pytest.mark.parametrize("name, outside", [("shot.PNG", True), ("doc.pdf", True), ("clip.mp4", True),
                                           ("notes.md", False), ("term.py", False), ("Makefile", False)])
def test_opens_outside_by_file_type(name, outside):
    assert viewer.opens_outside(f"/data/{name}", is_dir=False) is outside
    assert viewer.opens_outside("/data/folder", is_dir=True) is True


def test_outside_url_is_local_path_or_sftp_on_the_session_machine():
    assert viewer.outside_url(hosts.LOCAL, "/data/a b.png") == "/data/a b.png"
    assert viewer.outside_url(hosts.KITTY, "/tmp/x.png") == "/tmp/x.png"
    assert viewer.outside_url("archbox", "/data/a b#1.png") == "sftp://archbox/data/a%20b%231.png"
    assert viewer.outside_url("archbox#web", "/data/x.png") == "sftp://archbox/data/x.png"


async def test_locate_local_resolves_home_and_reports_directories(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    (tmp_path / "pics").mkdir()
    (tmp_path / "pics" / "a.png").write_bytes(b"\x89PNG")
    assert await viewer.locate(hosts.LOCAL, "s", "~/pics/a.png") == (str(tmp_path / "pics" / "a.png"), False)
    assert await viewer.locate(hosts.LOCAL, "s", str(tmp_path / "pics")) == (str(tmp_path / "pics"), True)


async def test_locate_remote_asks_the_machine_safely(tmp_path, monkeypatch):
    folder = tmp_path / "dir;touch INJECTED"
    folder.mkdir()
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    ssh = bin_dir / "ssh"
    ssh.write_text("#!/bin/sh\nwhile [ \"$1\" = -o ]; do shift 2; done\nshift\nexec sh -c \"$1\"\n")
    ssh.chmod(0o755)
    monkeypatch.setenv("PATH", f"{bin_dir}:{os.environ['PATH']}")
    assert await viewer.locate("archbox", "s", str(folder)) == (str(folder), True)
    assert not (tmp_path / "INJECTED").exists()


async def test_locate_missing_file_raises(tmp_path):
    with pytest.raises(viewer.ViewerError):
        await viewer.locate(hosts.LOCAL, "s", str(tmp_path / "nope.png"))


def png_bytes(size=(4, 3)):
    import io
    from PIL import Image
    out = io.BytesIO()
    Image.new("RGB", size, "red").save(out, "PNG")
    return out.getvalue()


@pytest.mark.parametrize("name, image", [("a.png", True), ("B.JPG", True), ("c.jpeg", True), ("d.WebP", True),
                                         ("e.gif", True), ("f.avif", True), ("g.svg", False), ("h.pdf", False),
                                         ("i.heic", False), ("notes.md", False), ("png", False)])
def test_is_image_covers_raster_types_only(name, image):
    assert viewer.is_image(f"/data/{name}") is image


async def test_load_image_reads_a_local_file(tmp_path):
    file = tmp_path / "a.png"
    file.write_bytes(png_bytes())
    data, picture = await viewer.load_image(hosts.LOCAL, "s", str(file))
    assert data == png_bytes()
    assert picture.size == (4, 3)


def jpeg_bytes(size):
    import io
    from PIL import Image
    out = io.BytesIO()
    Image.effect_noise(size, 64).convert("RGB").save(out, "JPEG")
    return out.getvalue()


async def test_load_image_rejects_a_truncated_jpeg(tmp_path):
    file = tmp_path / "half.jpg"
    whole = jpeg_bytes((200, 200))
    file.write_bytes(whole[:len(whole) // 2])
    with pytest.raises(viewer.ViewerError, match="not an image"):
        await viewer.load_image(hosts.LOCAL, "s", str(file))


async def test_load_image_rejects_huge_pixel_counts(tmp_path, monkeypatch):
    monkeypatch.setattr(viewer, "MAX_IMAGE_PIXELS", 100)
    file = tmp_path / "wide.png"
    file.write_bytes(png_bytes((20, 20)))
    with pytest.raises(viewer.ViewerError, match="image too large"):
        await viewer.load_image(hosts.LOCAL, "s", str(file))


async def test_load_image_shrinks_big_pictures_off_the_ui_thread(tmp_path, monkeypatch):
    monkeypatch.setattr(viewer, "MAX_SIDE", 50)
    file = tmp_path / "big.jpg"
    file.write_bytes(jpeg_bytes((400, 100)))
    data, picture = await viewer.load_image(hosts.LOCAL, "s", str(file))
    assert data == file.read_bytes()
    assert picture.size == (50, 12) or picture.size == (50, 13)


async def test_load_image_remote_uses_ssh_safely(tmp_path, monkeypatch):
    file = tmp_path / "shot;touch INJECTED.png"
    file.write_bytes(png_bytes())
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    ssh = bin_dir / "ssh"
    ssh.write_text("#!/bin/sh\nwhile [ \"$1\" = -o ]; do shift 2; done\nshift\nexec sh -c \"$1\"\n")
    ssh.chmod(0o755)
    monkeypatch.setenv("PATH", f"{bin_dir}:{os.environ['PATH']}")
    assert (await viewer.load_image("archbox", "s", str(file)))[0] == png_bytes()
    assert not (tmp_path / "INJECTED.png").exists()


async def test_load_image_rejects_oversized_files(tmp_path, monkeypatch):
    monkeypatch.setattr(viewer, "MAX_IMAGE_BYTES", 100)
    file = tmp_path / "big.png"
    file.write_bytes(png_bytes((200, 200)) + b"\0" * 200)
    with pytest.raises(viewer.ViewerError, match="image too large"):
        await viewer.load_image(hosts.LOCAL, "s", str(file))


async def test_load_image_rejects_non_images(tmp_path):
    file = tmp_path / "fake.png"
    file.write_bytes(b"not a png at all")
    with pytest.raises(viewer.ViewerError, match="not an image"):
        await viewer.load_image(hosts.LOCAL, "s", str(file))


async def test_load_image_reports_missing_file(tmp_path):
    with pytest.raises(viewer.ViewerError):
        await viewer.load_image(hosts.LOCAL, "s", str(tmp_path / "missing.png"))
