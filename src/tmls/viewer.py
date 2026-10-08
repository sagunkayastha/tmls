"""Read files named by terminal output for the in-app file viewer."""

import asyncio
import io
import posixpath
import shlex
import warnings
import urllib.parse
from pathlib import Path

from PIL import Image as PILImage
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widgets import Button, Static, TextArea
# Probes the terminal (kitty graphics / sixel / cell size) on import, which only works before Textual
# owns stdin: tmls.app imports this module at startup.
from textual_image.widget import Image as ImageWidget

from tmls import hosts

MAX_BYTES = 2_000_000
# Shown by the desktop's own apps instead of the text viewer (images, PDFs, media, archives, office).
OUTSIDE = {"png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "tif", "tiff", "ico", "avif", "heic",
           "pdf", "mp4", "mkv", "webm", "mov", "avi", "mp3", "wav", "ogg", "flac", "m4a",
           "zip", "tar", "gz", "tgz", "xz", "7z", "docx", "xlsx", "pptx", "odt", "ods", "odp"}
# Of those, shown in the image pane (svg/heic are not raster images Pillow reads; they stay outside).
IMAGES = {"png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "ico", "avif"}
MAX_IMAGE_BYTES = 25_000_000
MAX_IMAGE_PIXELS = 50_000_000  # decoding more stalls tmls and eats RAM; "Open in app" still works
MAX_SIDE = 2560  # the pane never shows more than this; a smaller picture redraws fast on resize


class ViewerError(Exception):
    pass


async def _run(argv, timeout=5):
    try:
        process = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=timeout)
    except asyncio.TimeoutError as error:
        process.kill()
        await process.wait()
        raise ViewerError("timed out") from error
    except OSError as error:
        raise ViewerError(str(error)) from error
    if process.returncode:
        raise ViewerError(stderr.decode(errors="replace").strip() or "command failed")
    return stdout


async def load_file(host, session, path):
    cwd = ""
    if not path.startswith(("/", "~/")):
        query = ["tmux", "display", "-p", "-t", f"={session}:", "#{pane_current_path}"]
        argv = query if host == hosts.LOCAL else hosts.run_argv(host, shlex.join(query))
        cwd = (await _run(argv)).decode(errors="replace").strip()
    machine = hosts.split(host)[0]  # a background tmux server's files are its machine's
    if machine == hosts.LOCAL:
        file = Path(path).expanduser()
        if not file.is_absolute():
            file = Path(cwd) / file
        try:
            with file.open("rb") as source:
                data = source.read(MAX_BYTES + 1)
        except OSError as error:
            raise ViewerError(str(error)) from error
        resolved = str(file)
    else:
        resolved = path if path.startswith("~/") else (
            path if path.startswith("/") else posixpath.join(cwd, path))
        # Quote screen text as one argument; keep only the fixed $HOME prefix expandable.
        operand = '"$HOME"/' + shlex.quote(path[2:]) if path.startswith("~/") else shlex.quote(resolved)
        command = f"head -c {MAX_BYTES + 1} -- {operand}"
        data = await _run(["ssh", "-o", "BatchMode=yes", machine, command])
    if len(data) > MAX_BYTES:
        raise ViewerError("file too large")
    if b"\0" in data:
        raise ViewerError("binary file")
    return resolved, data.decode(errors="replace")


def opens_outside(path, is_dir):
    """Folders and non-text files go to the desktop's apps; text stays in the viewer."""
    return is_dir or posixpath.splitext(path)[1][1:].lower() in OUTSIDE


def is_image(path):
    return posixpath.splitext(path)[1][1:].lower() in IMAGES


async def load_image(host, session, path):
    """(bytes, decoded picture) of an image on the session's machine (path as `locate` resolved it)."""
    machine = hosts.split(host)[0]
    if machine in (hosts.LOCAL, hosts.KITTY):
        try:
            with Path(path).expanduser().open("rb") as source:
                data = source.read(MAX_IMAGE_BYTES + 1)
        except OSError as error:
            raise ViewerError(str(error)) from error
    else:
        operand = '"$HOME"/' + shlex.quote(path[2:]) if path.startswith("~/") else shlex.quote(path)
        # A 25 MB image over a slow link takes longer than a text file's 5 s.
        data = await _run(hosts.run_argv(host, f"head -c {MAX_IMAGE_BYTES + 1} -- {operand}"), timeout=60)
    if len(data) > MAX_IMAGE_BYTES:
        raise ViewerError("image too large")
    return data, await asyncio.to_thread(_decode, data)


def _decode(data):
    """A fully decoded, pane-sized copy: decoding here (not in the widget's render) catches truncated
    files before they reach the screen, and keeps the slow part off the UI thread."""
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", PILImage.DecompressionBombWarning)
            image = PILImage.open(io.BytesIO(data))
            if image.width * image.height > MAX_IMAGE_PIXELS:
                raise ViewerError("image too large")
            image.draft("RGB", (MAX_SIDE, MAX_SIDE))  # JPEG: decode at a reduced scale
            image.load()
            image.thumbnail((MAX_SIDE, MAX_SIDE))
            return image
    except ViewerError:
        raise
    except PILImage.DecompressionBombError as error:
        raise ViewerError("image too large") from error
    except Exception as error:  # Pillow raises many types for bad data
        raise ViewerError("not an image") from error


def save_image(data, name, folder):
    """Write data into folder as name, or "name (1)…" when taken (never replaces a file); the path."""
    folder.mkdir(parents=True, exist_ok=True)
    stem, suffix = posixpath.splitext(name)
    for n in range(1000):
        target = folder / (name if n == 0 else f"{stem} ({n}){suffix}")
        try:
            with target.open("xb") as out:
                out.write(data)
        except FileExistsError:
            continue
        return target
    raise ViewerError("no free name in " + str(folder))


def outside_url(host, path):
    """What xdg-open gets: a local path, or sftp on the session's machine (KDE apps read it in place,
    nothing is copied to this PC first)."""
    machine = hosts.split(host)[0]
    if machine in (hosts.LOCAL, hosts.KITTY):
        return path
    return f"sftp://{machine}{urllib.parse.quote(path)}"


async def locate(host, session, path):
    """(absolute path, is_dir) of a clicked path on the session's machine; raises if it is missing."""
    machine = hosts.split(host)[0]
    if machine in (hosts.LOCAL, hosts.KITTY):
        file = Path(path).expanduser()
        if not file.is_absolute():
            cwd = (await _run(["tmux", "display", "-p", "-t", f"={session}:", "#{pane_current_path}"]))
            file = Path(cwd.decode(errors="replace").strip()) / file
        if not file.exists():
            raise ViewerError("not found")
        return str(file), file.is_dir()
    operand = '"$HOME"/' + shlex.quote(path[2:]) if path.startswith("~/") else shlex.quote(path)
    if not path.startswith(("/", "~/")):
        query = shlex.join(["tmux", "display", "-p", "-t", f"={session}:", "#{pane_current_path}"])
        operand = f'"$({query})"/{operand}'
    script = f"p={operand}; [ -e \"$p\" ] || exit 1; [ -d \"$p\" ] && echo d || echo f; printf %s \"$p\""
    try:
        out = await _run(hosts.run_argv(host, script))
    except ViewerError as error:
        raise ViewerError("not found") from error
    kind, _, resolved = out.decode(errors="replace").partition("\n")
    return resolved, kind == "d"


class FileViewer(Vertical):
    BINDINGS = [Binding("escape", "close", "Close file", show=False, priority=True)]

    class Closed(Message):
        pass

    def __init__(self, host, path, line, text):
        super().__init__(id="file-viewer")
        self.host, self.path, self.line, self.text = host, path, line, text

    def compose(self):
        with Horizontal(id="viewer-header"):
            yield Static(f"{self.host}:{self.path}:{self.line}", id="viewer-title")
            yield Button("×", id="viewer-close")
        yield TextArea(self.text, read_only=True, show_line_numbers=True, soft_wrap=False,
                       id="viewer-text")

    def on_mount(self):
        area = self.query_one(TextArea)
        ext = Path(self.path).suffix.lstrip(".")
        language = {"py": "python", "js": "javascript", "sh": "bash", "md": "markdown"}.get(ext, ext)
        if language in area.available_languages:
            area.language = language
        row = max(0, min(self.line - 1, len(self.text.splitlines()) - 1))
        area.move_cursor((row, 0))
        self.call_after_refresh(lambda: area.scroll_to(y=max(0, row - area.size.height // 3), animate=False))

    def action_close(self):
        self.post_message(self.Closed())

    def on_button_pressed(self, event):
        if event.button.id == "viewer-close":
            event.stop()
            self.action_close()


class ImageViewer(Vertical):
    BINDINGS = [Binding("escape", "close", "Close image", show=False, priority=True)]

    class Closed(Message):
        pass

    class OpenInApp(Message):
        def __init__(self, host, path):
            super().__init__()
            self.host, self.path = host, path

    def __init__(self, host, path, data, picture):
        super().__init__(id="image-viewer")
        self.host, self.path, self.data, self.picture = host, path, data, picture

    def compose(self):
        with Horizontal(id="image-header"):
            yield Static(f"{self.host}:{self.path}", id="image-title")
            yield Button("Open in app", id="image-open")
            yield Button("Save", id="image-save")
            yield Button("×", id="image-close")
        with Vertical(id="image-body"):
            self.shown = ImageWidget(self.picture, id="image-picture",
                                     on_error=lambda error: Static(f"can't show this image: {error}"))
            yield self.shown

    def on_unmount(self):
        self.shown.image = None  # also deletes the picture from kitty's memory

    def action_close(self):
        self.post_message(self.Closed())

    def on_button_pressed(self, event):
        event.stop()
        if event.button.id == "image-close":
            self.action_close()
        elif event.button.id == "image-open":
            self.post_message(self.OpenInApp(self.host, self.path))
        elif event.button.id == "image-save":
            try:
                saved = save_image(self.data, posixpath.basename(self.path), Path.home() / "Downloads")
            except (OSError, ViewerError) as error:
                self.notify(f"Save failed: {error}", severity="error", markup=False)
            else:
                self.notify(f"Saved {saved}", markup=False)
