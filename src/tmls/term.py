"""A terminal inside a Textual widget: runs a command on a pty and renders it with pyte."""
import asyncio
import base64
import fcntl
import os
import pty
import re
import signal
import struct
import termios
from functools import lru_cache

import pyte
from rich.segment import Segment
from rich.style import Style
from textual import events
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
BUTTONS = {1: 0, 2: 1, 3: 2}  # Textual left/middle/right -> xterm button codes
OSC52 = re.compile(rb"\x1b\]52;[^;]*;([A-Za-z0-9+/=]*)(?:\x07|\x1b\\)")


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
            self.app.copy_to_clipboard(text)
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
            s = style(ch.fg, ch.bg, ch.bold, ch.italics, ch.underscore, ch.reverse != (show_cursor and x == cur.x))
            if s is not run_style and run:
                segments.append(Segment("".join(run), run_style))
                run = []
            run_style = s
            run.append(ch.data or " ")
        if run:
            segments.append(Segment("".join(run), run_style))
        return Strip(segments)

    async def _on_key(self, event: events.Key):
        # Every key goes to the child, so tab/ctrl+c don't move focus or quit tmls.
        event.stop()
        event.prevent_default()
        if self.fd is not None and not self.exited:
            os.write(self.fd, key_to_bytes(event.key, event.character))

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
        if event.button in BUTTONS:
            self._held = BUTTONS[event.button]
            self.capture_mouse()  # keep getting the drag when it leaves the widget
            self._mouse(event, self._held, press=True)

    def on_mouse_move(self, event):
        modes = self.vt.mode
        if self._held is not None and modes & {MOUSE_DRAGS, MOUSE_ANY}:
            self._mouse(event, self._held, press=True, drag=True)

    def on_mouse_up(self, event):
        if self._held is not None:
            self._mouse(event, self._held, press=False)
            self._held = None
            self.release_mouse()

    def on_mouse_scroll_up(self, event):
        self._mouse(event, 64, press=True)

    def on_mouse_scroll_down(self, event):
        self._mouse(event, 65, press=True)

    def close(self):
        if self.pid is not None and not self.exited:
            os.kill(self.pid, signal.SIGHUP)

    def on_unmount(self):
        self.close()
