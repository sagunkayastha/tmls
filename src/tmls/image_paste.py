"""Move pasted images to the host of the selected tmux session."""
import asyncio
import os
import shutil
from datetime import datetime
from pathlib import Path
from urllib.parse import unquote, urlparse

from tmls import hosts

CACHE = Path.home() / ".cache" / "tmls" / "images"
IMAGE_TYPES = {"image/png": ".png", "image/jpeg": ".jpg", "image/webp": ".webp", "image/gif": ".gif"}
FILE_SUFFIXES = {".png", ".jpg", ".jpeg", ".gif", ".webp"}


class ImageError(Exception):
    """A clipboard read or image copy failed."""


def dropped_image(text):
    """A single existing image file dropped into kitty's paste stream, if any."""
    value = text.strip()
    if not value or "\n" in value or "\r" in value:
        return None
    if value.startswith("file://"):
        uri = urlparse(value)
        if uri.netloc not in {"", "localhost"}:
            return None
        value = unquote(uri.path)
    path = Path(value)
    return path if path.suffix.lower() in FILE_SUFFIXES and path.is_file() else None


async def _run(argv, stdin=None, timeout=10):
    try:
        process = await asyncio.create_subprocess_exec(
            *argv, stdin=asyncio.subprocess.PIPE if stdin is not None else asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        out, err = await asyncio.wait_for(process.communicate(stdin), timeout=timeout)
    except asyncio.TimeoutError:
        process.kill()
        await process.wait()
        raise ImageError("copy timed out") from None
    except OSError as error:
        raise ImageError(str(error)) from error
    if process.returncode:
        raise ImageError(err.decode(errors="replace").strip() or "clipboard/copy command failed")
    return out


async def clipboard_image():
    """(image bytes, matching suffix) when the clipboard offers an image, else None."""
    wayland = bool(os.environ.get("WAYLAND_DISPLAY") and shutil.which("wl-paste"))
    x11 = bool(os.environ.get("DISPLAY") and shutil.which("xclip"))
    if not wayland and not x11:
        return None
    probe = ["wl-paste", "--list-types"] if wayland else ["xclip", "-selection", "clipboard", "-t", "TARGETS", "-o"]
    try:
        offered = (await _run(probe, timeout=2)).decode(errors="replace").splitlines()
    except ImageError:
        return None
    mime = next((kind for kind in IMAGE_TYPES if kind in offered), None)
    if mime is None:
        return None
    command = ["wl-paste", "--type", mime] if wayland else [
        "xclip", "-selection", "clipboard", "-t", mime, "-o"]
    try:
        return await _run(command, timeout=3), IMAGE_TYPES[mime]
    except ImageError:
        return None


async def store(host, data, suffix=".png"):
    """Save bytes under the selected host's cache and return the path to paste."""
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    filename = stamp + suffix.lower()
    relative = f".cache/tmls/images/{filename}"
    if host in {hosts.LOCAL, hosts.KITTY}:
        path = CACHE / filename
        try:
            await asyncio.to_thread(path.parent.mkdir, parents=True, exist_ok=True)
            await asyncio.to_thread(path.write_bytes, data)
        except OSError as error:
            raise ImageError(str(error)) from error
        return str(path)
    command = ('mkdir -p "$HOME/.cache/tmls/images" && '
               f'cat > "$HOME/{relative}"')
    await _run(["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, command],
               stdin=data, timeout=10)
    return "~/" + relative
