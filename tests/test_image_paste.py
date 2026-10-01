import asyncio
from pathlib import Path

import pytest

from tmls import image_paste


def test_drop_accepts_one_existing_image_path_or_file_uri(tmp_path):
    image = tmp_path / "my photo.JPEG"
    image.write_bytes(b"jpeg bytes")
    assert image_paste.dropped_image(str(image)) == image
    assert image_paste.dropped_image(image.as_uri()) == image
    assert image_paste.dropped_image(str(image) + "\n" + str(image)) is None
    assert image_paste.dropped_image(str(tmp_path / "missing.png")) is None
    text = tmp_path / "notes.txt"
    text.write_text("hello")
    assert image_paste.dropped_image(str(text)) is None


async def test_local_image_is_saved_as_png_and_returns_a_path(tmp_path, monkeypatch):
    monkeypatch.setattr(image_paste, "CACHE", tmp_path / "images")
    path = await image_paste.store("local", b"\x89PNG\r\n\x1a\ncontents", ".png")
    saved = Path(path)
    assert saved.parent == tmp_path / "images"
    assert saved.suffix == ".png"
    assert saved.read_bytes() == b"\x89PNG\r\n\x1a\ncontents"


async def test_remote_image_uses_one_ssh_call_and_keeps_bytes_off_command_line(monkeypatch):
    calls = []

    class Process:
        returncode = 0

        async def communicate(self, data):
            calls.append(data)
            return b"", b""

    async def spawn(*argv, **kwargs):
        calls.append((argv, kwargs))
        return Process()

    monkeypatch.setattr(asyncio, "create_subprocess_exec", spawn)
    path = await image_paste.store("archbox", b"\x89PNG\x00image", ".png")
    assert path.startswith("~/.cache/tmls/images/") and path.endswith(".png")
    assert len(calls) == 2
    argv, kwargs = calls[0]
    assert argv[:5] == ("ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5")
    assert argv[5] == "archbox"
    assert "mkdir -p" in argv[6] and "cat >" in argv[6]
    assert b"\x89PNG\x00image" not in argv[6].encode()
    assert calls[1] == b"\x89PNG\x00image"
    assert kwargs["stdin"] == asyncio.subprocess.PIPE


async def test_failed_remote_copy_raises_instead_of_typing_a_path(monkeypatch):
    class Process:
        returncode = 1

        async def communicate(self, data):
            return b"", b"permission denied"

    async def spawn(*argv, **kwargs):
        return Process()

    monkeypatch.setattr(asyncio, "create_subprocess_exec", spawn)
    with pytest.raises(image_paste.ImageError, match="permission denied"):
        await image_paste.store("archbox", b"PNG", ".png")


async def test_wayland_clipboard_reads_png_bytes_after_type_probe(monkeypatch):
    calls = []

    class Process:
        returncode = 0

        def __init__(self, output):
            self.output = output

        async def communicate(self, data=None):
            return self.output, b""

    async def spawn(*argv, **kwargs):
        calls.append(argv)
        return Process(b"text/plain\nimage/png\n" if "--list-types" in argv else b"\x89PNG\r\n\x1a\nimage")

    monkeypatch.setenv("WAYLAND_DISPLAY", "wayland-0")
    monkeypatch.setattr(image_paste.shutil, "which", lambda name: "/usr/bin/" + name)
    monkeypatch.setattr(asyncio, "create_subprocess_exec", spawn)
    assert await image_paste.clipboard_image() == (b"\x89PNG\r\n\x1a\nimage", ".png")
    assert calls == [("wl-paste", "--list-types"), ("wl-paste", "--type", "image/png")]


async def test_x11_clipboard_keeps_jpeg_bytes_and_extension(monkeypatch):
    jpeg = b"\xff\xd8\xffjpeg bytes"
    outputs = [b"TARGETS\nimage/jpeg\n", jpeg]

    class Process:
        returncode = 0

        async def communicate(self, data=None):
            return outputs.pop(0), b""

    async def spawn(*argv, **kwargs):
        return Process()

    monkeypatch.delenv("WAYLAND_DISPLAY", raising=False)
    monkeypatch.setenv("DISPLAY", ":0")
    monkeypatch.setattr(image_paste.shutil, "which", lambda name: "/usr/bin/" + name)
    monkeypatch.setattr(asyncio, "create_subprocess_exec", spawn)
    assert await image_paste.clipboard_image() == (jpeg, ".jpg")


async def test_bmp_clipboard_is_left_for_text_fallback(monkeypatch):
    class Process:
        returncode = 0

        async def communicate(self, data=None):
            return b"image/bmp\n", b""

    async def spawn(*argv, **kwargs):
        return Process()

    monkeypatch.setenv("WAYLAND_DISPLAY", "wayland-0")
    monkeypatch.setattr(image_paste.shutil, "which", lambda name: "/usr/bin/" + name)
    monkeypatch.setattr(asyncio, "create_subprocess_exec", spawn)
    assert await image_paste.clipboard_image() is None
