"""Move pasted images to the host of the selected tmux session."""
import asyncio
import io
import os
import shutil
from datetime import datetime
from pathlib import Path
from urllib.parse import unquote, urlparse

from PIL import Image

from tmls import hosts

CACHE = Path.home() / ".cache" / "tmls" / "images"
IMAGE_TYPES = ("image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp")
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
    """PNG bytes when the system clipboard offers an image, else None."""
    wayland = bool(os.environ.get("WAYLAND_DISPLAY") and shutil.which("wl-paste"))
    x11 = bool(os.environ.get("DISPLAY") and shutil.which("xclip"))
    if not wayland and not x11:
        raise ImageError("install wl-paste or xclip to paste images")
    probe = ["wl-paste", "--list-types"] if wayland else ["xclip", "-selection", "clipboard", "-t", "TARGETS", "-o"]
    try:
        offered = (await _run(probe, timeout=2)).decode(errors="replace").splitlines()
    except ImageError as error:
        if "Nothing is copied" in str(error) or "target TARGETS not available" in str(error):
            return None
        raise
    mime = next((kind for kind in IMAGE_TYPES if kind in offered), None)
    if mime is None:
        return None
    command = ["wl-paste", "--type", mime] if wayland else [
        "xclip", "-selection", "clipboard", "-t", mime, "-o"]
    data = await _run(command, timeout=3)
    if mime == "image/png":
        return data
    try:
        with Image.open(io.BytesIO(data)) as image:
            output = io.BytesIO()
            image.convert("RGBA").save(output, format="PNG")
            return output.getvalue()
    except (OSError, ValueError) as error:
        raise ImageError(f"couldn't decode clipboard image: {error}") from error


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
