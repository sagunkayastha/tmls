"""Read files named by terminal output for the in-app file viewer."""

import asyncio
import posixpath
import shlex
from pathlib import Path

from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.widgets import Button, Static, TextArea

from tmls import hosts

MAX_BYTES = 2_000_000


class ViewerError(Exception):
    pass


async def _run(argv):
    try:
        process = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        stdout, stderr = await asyncio.wait_for(process.communicate(), timeout=5)
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
        query = ["tmux", "display", "-p", "-t", session, "#{pane_current_path}"]
        argv = query if host == hosts.LOCAL else ["ssh", "-o", "BatchMode=yes", host, shlex.join(query)]
        cwd = (await _run(argv)).decode(errors="replace").strip()
    if host == hosts.LOCAL:
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
        data = await _run(["ssh", "-o", "BatchMode=yes", host, command])
    if len(data) > MAX_BYTES:
        raise ViewerError("file too large")
    if b"\0" in data:
        raise ViewerError("binary file")
    return resolved, data.decode(errors="replace")


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
