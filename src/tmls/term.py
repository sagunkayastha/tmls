"""A terminal inside a Textual widget: runs a command on a pty and renders it with pyte."""
import asyncio
import base64
import fcntl
import os
import pty
import re
import shutil
import signal
import struct
import termios
from functools import lru_cache

import pyte
from rich.segment import Segment
from rich.style import Style
from textual import events
from textual.message import Message
from textual.strip import Strip
from textual.widget import Widget

KEYS = {
    "up": "\x1b[A", "down": "\x1b[B", "right": "\x1b[C", "left": "\x1b[D",
    "home": "\x1b[H", "end": "\x1b[F", "pageup": "\x1b[5~", "pagedown": "\x1b[6~",
    "insert": "\x1b[2~", "delete": "\x1b[3~", "enter": "\r", "tab": "\t",
    "shift+tab": "\x1b[Z", "backspace": "\x7f", "escape": "\x1b",
    "f1": "\x1bOP", "f2": "\x1bOQ", "f3": "\x1bOR", "f4": "\x1bOS", "f5": "\x1b[15~",
    "f6": "\x1b[17~", "f7": "\x1b[18~", "f8": "\x1b[19~", "f9": "\x1b[20~",
    "f10": "\x1b[21~", "f11": "\x1b[23~", "f12": "\x1b[24~",
}
ARROWS = {"up": "A", "down": "B", "right": "C", "left": "D"}
MODIFIERS = {"shift": 2, "alt": 3, "ctrl": 5, "ctrl+shift": 6}
# pyte keeps DEC private modes shifted left by 5
MOUSE_CLICKS, MOUSE_DRAGS, MOUSE_ANY, MOUSE_SGR = (m << 5 for m in (1000, 1002, 1003, 1006))
BRACKETED_PASTE = 2004 << 5  # the child wants pastes wrapped so it doesn't run them as typed keys
BUTTONS = {1: 0, 2: 1, 3: 2}  # Textual left/middle/right -> xterm button codes
OSC52 = re.compile(rb"\x1b\]52;[^;]*;([A-Za-z0-9+/=]*)(?:\x07|\x1b\\)")
URL = re.compile(r"""https?://[^\s<>"'`]+""")
FILE_LINE = re.compile(
    r"(?P<path>[A-Za-z0-9._~/+-]*[A-Za-z0-9_~-]\.[A-Za-z0-9]+|"
    r"[A-Za-z0-9._~+-]*/[A-Za-z0-9._~/+-]+):(?P<line>\d+)(?::\d+)?"
)


def link_at(rows, x, y):
    """Return (kind, target, line) for the visible link under a screen cell."""
    if not 0 <= y < len(rows) or not 0 <= x < len(rows[y]):
        return None
    row = rows[y]
    if y + 1 < len(rows) and row and row[-1] != " " and rows[y + 1][:1].strip():
        row += rows[y + 1]
    urls = list(URL.finditer(row))
    for match in urls:
        value = match.group()
        while value and value[-1] in ".,;:!?)]}'\"":
            if value[-1] == ")" and value.count("(") >= value.count(")"):
                break
            value = value[:-1]
        if match.start() <= x < match.start() + len(value):
            return "url", value, None
    for match in FILE_LINE.finditer(row):
        if any(match.start() < url.end() and match.end() > url.start() for url in urls):
            continue
        if match.start() <= x < match.end():
            return "file", match.group("path"), int(match.group("line"))
    return None


def trim_copy(text):
    return "\n".join(line.rstrip(" \t") for line in text.split("\n"))


def key_to_bytes(key, character):
    if key in KEYS:
        return KEYS[key].encode()
    mods, _, base = key.rpartition("+")
    if base in ARROWS and mods in MODIFIERS:
        return f"\x1b[1;{MODIFIERS[mods]}{ARROWS[base]}".encode()
    if mods == "alt" and len(base) == 1:
        return b"\x1b" + base.encode()
    if mods == "ctrl" and len(base) == 1 and base.isalpha():
        return bytes([ord(base) & 0x1F])
    return character.encode() if character else b""


def mouse_bytes(button, x, y, press, drag=False, shift=False, meta=False, ctrl=False):
    """SGR mouse report (xterm mode 1006) for 0-based cell x, y."""
    code = button + 32 * drag + 4 * shift + 8 * meta + 16 * ctrl
    return f"\x1b[<{code};{x + 1};{y + 1}{'M' if press else 'm'}".encode()


class Clipboard:
    """Picks OSC 52 clipboard writes (tmux copy with set-clipboard) out of the child's output,
    which pyte would drop. Keeps an unfinished sequence until its terminator arrives."""

    def __init__(self):
        self.pending = b""

    def feed(self, data):
        buf = self.pending + data
        texts = [base64.b64decode(m.group(1)).decode(errors="replace") for m in OSC52.finditer(buf)]
        start = buf.rfind(b"\x1b]52;")
        unfinished = start != -1 and not OSC52.match(buf, start)
        self.pending = buf[start:][-(1 << 20):] if unfinished else b""
        return texts


def color(name):
    if name == "default":
        return None
    if len(name) == 6 and all(c in "0123456789abcdef" for c in name):
        return "#" + name
    bright = name.startswith("bright")
    base = name[6:] if bright else name
    base = "yellow" if base == "brown" else base
    return f"bright_{base}" if bright else base


@lru_cache(maxsize=4096)
def style(fg, bg, bold, italics, underscore, reverse):
    return Style(color=color(fg), bgcolor=color(bg), bold=bold, italic=italics,
                 underline=underscore, reverse=reverse)


class VT(pyte.Screen):
    """pyte screen whose replies to status queries go back to the child."""

    def __init__(self, columns, lines, reply):
        super().__init__(columns, lines)
        self.reply = reply

    def write_process_input(self, data):
        self.reply(data)

    def report_device_attributes(self, *args, **kwargs):
        # pyte can't tell CSI c from CSI > c and answers both as primary DA; tmux then leaks
        # the unparsed reply ("6c") into the pane as input. Nothing needs an answer, so stay silent.
        pass

    def report_device_status(self, mode, **kwargs):
        # pyte 0.8.2 passes private=True for CSI ? n queries and then crashes; answer the normal form.
        if not kwargs.get("private"):
            super().report_device_status(mode)


class Terminal(Widget, can_focus=True):
    DEFAULT_CSS = "Terminal { height: 1fr; }"
    ALLOW_SELECT = False  # drags go to the child (tmux selects and copies); shift+drag is kitty's

    class LinkClicked(Message):
        def __init__(self, kind, target, line):
            super().__init__()
            self.kind, self.target, self.line = kind, target, line

    def __init__(self, argv, **kwargs):
        super().__init__(**kwargs)
        self.argv = argv
        self.vt = VT(80, 24, self._reply)
        self.stream = pyte.ByteStream(self.vt)
        self.pid = None
        self.fd = None
        self.exited = False
        self._dirty = False
        self.clipboard = Clipboard()
        self._held = None  # xterm code of the button held down, for drags
        self._selection = None
        self._selecting = False
        self.quick_hits = []

    def on_mount(self):
        self.set_interval(1 / 30, self._flush)

    def on_resize(self, event):
        cols, rows = max(event.size.width, 2), max(event.size.height, 2)
        self.vt.resize(rows, cols)
        if self.pid is None:
            self._spawn(rows, cols)
        elif not self.exited:
            fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

    def _spawn(self, rows, cols):
        env = {k: v for k, v in os.environ.items() if k != "TMUX"}  # allow attaching from inside tmux
        env["TERM"] = "xterm-256color"
        # pty.fork makes the pty the child's controlling terminal, so ^C and resize signals work.
        pid, fd = pty.fork()
        if pid == 0:
            try:
                os.execvpe(self.argv[0], self.argv, env)
            finally:
                os._exit(127)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.pid, self.fd = pid, fd
        asyncio.get_running_loop().add_reader(fd, self._read)

    def _reply(self, data):
        if self.fd is not None and not self.exited:
            os.write(self.fd, data.encode())

    def _read(self):
        try:
            data = os.read(self.fd, 65536)
        except OSError:
            data = b""
        if not data:
            self._finish()
            return
        for text in self.clipboard.feed(data):
            self.app.copy_to_clipboard(trim_copy(text))
        self.stream.feed(data)
        self._dirty = True

    def _finish(self):
        asyncio.get_running_loop().remove_reader(self.fd)
        os.close(self.fd)
        os.waitpid(self.pid, 0)
        self.exited = True
        self.stream.feed(b"\r\n\x1b[2m[session closed]\x1b[0m")
        self._dirty = True

    def _flush(self):
        if self._dirty:
            self._dirty = False
            self.refresh()

    def render_line(self, y):
        if y >= self.vt.lines:
            return Strip.blank(self.size.width)
        row = self.vt.buffer[y]
        cur = self.vt.cursor
        show_cursor = self.has_focus and not cur.hidden and y == cur.y and not self.exited
        segments, run, run_style = [], [], None
        for x in range(self.vt.columns):
            ch = row[x]
            selected = self._selection is not None and min(self._selection) <= (y, x) < max(self._selection)
            s = style(ch.fg, ch.bg, ch.bold, ch.italics, ch.underscore,
                      ch.reverse ^ (show_cursor and x == cur.x) ^ selected)
            label = next((hit.label for hit in self.quick_hits if (hit.y, hit.x) == (y, x)), None)
            if label:
                s = Style(color="black", bgcolor="#e5c07b", bold=True)
            if s is not run_style and run:
                segments.append(Segment("".join(run), run_style))
                run = []
            run_style = s
            run.append(label or ch.data or " ")
        if run:
            segments.append(Segment("".join(run), run_style))
        return Strip(segments)

    async def _on_key(self, event: events.Key):
        # Every key goes to the child, so tab/ctrl+c don't move focus or quit tmls.
        event.stop()
        event.prevent_default()
        if event.key in {"alt+shift+s", "shift+alt+s"}:
            from tmls.quickselect import candidates
            self.quick_hits = [] if self.quick_hits else candidates(self.vt.display)
            self.refresh()
            if self.quick_hits:
                self.app.notify("a–z copies · Shift+a–z opens · Esc cancels")
            else:
                self.app.notify("No URL, file or hash on screen.")
            return
        if self.quick_hits:
            key = (event.character or event.key.rsplit("+", 1)[-1]).lower()
            chosen = next((hit for hit in self.quick_hits if hit.label == key), None)
            self.quick_hits = []
            self.refresh()
            if chosen:
                if (event.key.startswith("shift+") or (event.character or "").isupper()) and chosen.kind != "hash":
                    self.post_message(self.LinkClicked(chosen.kind, chosen.target, chosen.line))
                else:
                    self.app.copy_to_clipboard(chosen.copy_text)
            return
        if event.key == "ctrl+c" and self._copy_selection():
            self._selection = None
            self.refresh()
            return
        if self.fd is not None and not self.exited:
            os.write(self.fd, key_to_bytes(event.key, event.character))

    def _copy_selection(self):
        if self._selection is None:
            return False
        start, end = sorted(self._selection)
        if start == end:
            return False
        lines = []
        for y in range(start[0], end[0] + 1):
            left = start[1] if y == start[0] else 0
            right = end[1] if y == end[0] else self.vt.columns
            lines.append("".join(self.vt.buffer[y][x].data or " " for x in range(left, right)))
        self.app.copy_to_clipboard(trim_copy("\n".join(lines)))
        return True

    def on_paste(self, event: events.Paste):
        # kitty's ctrl+shift+v arrives as one Paste event, not as keys
        event.stop()
        self._paste(event.text)

    def _paste(self, text):
        if text and self.fd is not None and not self.exited:
            if BRACKETED_PASTE in self.vt.mode:
                text = f"\x1b[200~{text}\x1b[201~"
            os.write(self.fd, text.encode())

    @property
    def mouse_mode(self):
        """The child asked for mouse reports, in SGR form (tmux does with `mouse on`)."""
        modes = self.vt.mode
        return MOUSE_SGR in modes and bool(modes & {MOUSE_CLICKS, MOUSE_DRAGS, MOUSE_ANY})

    def _mouse(self, event, button, press, drag=False):
        event.stop()
        if self.mouse_mode and self.fd is not None and not self.exited:
            os.write(self.fd, mouse_bytes(button, event.x, event.y, press, drag,
                                          event.shift, event.meta, event.ctrl))

    def on_mouse_down(self, event):
        if event.button == 3:
            event.stop()
            asyncio.create_task(self._paste_clipboard())
            return
        if event.button == 1 and event.ctrl:
            point = (max(0, min(event.y, self.vt.lines - 1)),
                     max(0, min(event.x, self.vt.columns)))
            self._selection = (point, point)
            self._selecting = True
            self.capture_mouse()
            event.stop()
            self.refresh()
            return
        if event.button in BUTTONS:
            self._selection = None
            self.refresh()
            self._held = BUTTONS[event.button]
            self.capture_mouse()  # keep getting the drag when it leaves the widget
            self._mouse(event, self._held, press=True)

    def on_mouse_move(self, event):
        if self._selecting:
            self._selection = (self._selection[0],
                               (max(0, min(event.y, self.vt.lines - 1)),
                                max(0, min(event.x, self.vt.columns))))
            event.stop()
            self.refresh()
            return
        modes = self.vt.mode
        if self._held is not None and modes & {MOUSE_DRAGS, MOUSE_ANY}:
            self._mouse(event, self._held, press=True, drag=True)

    def on_mouse_up(self, event):
        if self._selecting:
            self._selection = (self._selection[0],
                               (max(0, min(event.y, self.vt.lines - 1)),
                                max(0, min(event.x, self.vt.columns))))
            self._selecting = False
            self.release_mouse()
            event.stop()
            self.refresh()
            if self._selection[0] == self._selection[1]:
                self._selection = None
                found = link_at(self.vt.display, event.x, event.y)
                if found:
                    self.post_message(self.LinkClicked(*found))
                return
            self._copy_selection()
            return
        if self._held is not None:
            self._mouse(event, self._held, press=False)
            self._held = None
            self.release_mouse()

    def on_mouse_scroll_up(self, event):
        self._mouse(event, 64, press=True)

    def on_mouse_scroll_down(self, event):
        self._mouse(event, 65, press=True)

    async def _paste_clipboard(self):
        command = (["wl-paste", "--no-newline"] if shutil.which("wl-paste") else
                   ["xclip", "-selection", "clipboard", "-o"] if shutil.which("xclip") else None)
        text, process = None, None
        if command:
            try:
                process = await asyncio.create_subprocess_exec(*command, stdout=asyncio.subprocess.PIPE,
                                                               stderr=asyncio.subprocess.DEVNULL)
                output, _ = await asyncio.wait_for(process.communicate(), timeout=2)
                if process.returncode == 0:
                    text = output.decode(errors="replace")
            except (OSError, asyncio.TimeoutError):
                if process is not None and process.returncode is None:
                    process.kill()
        self._paste(self.app.clipboard if text is None else text)

    def close(self):
        if self.pid is not None and not self.exited:
            os.kill(self.pid, signal.SIGHUP)

    def on_unmount(self):
        self.close()
