"""New tmux session on a chosen host, in a chosen folder: the form and the command behind it."""
import asyncio
import json
import os
import shlex
from functools import partial
from pathlib import Path

from textual.containers import Horizontal, Vertical
from textual.screen import ModalScreen
from textual.widgets import Button, Input, Label, RadioButton, RadioSet, Select, Static

from tmls import hosts

PRESETS = Path.home() / ".config" / "tmls" / "agent-presets.json"


def load_presets():
    try:
        data = json.loads(PRESETS.read_text())
    except (OSError, ValueError):
        return {}
    if not isinstance(data, dict):
        return {}
    return {name: tuple(argv) for name, argv in data.items()
            if isinstance(name, str) and name and isinstance(argv, list) and argv
            and all(isinstance(arg, str) and arg for arg in argv)}


def default_name(folder):
    return os.path.basename(folder.rstrip("/")) if folder.strip("~/") else "home"


def check_name(name):
    """Why tmux can't use this name, or None. tmux rewrites ":" and "." in names."""
    if not name:
        return "the session needs a name"
    if ":" in name or "." in name:
        return "names can't contain : or ."
    return None


def _folder(folder):
    # ~ must stay unquoted for the host's shell to expand it; everything else is quoted
    if folder == "~" or folder.startswith("~/"):
        return '"$HOME"' + ("/" + shlex.quote(folder[2:]) if folder[2:] else "")
    return shlex.quote(folder)


async def suggest_name(host, folder):
    """Use the host's git root when the folder is in a repository."""
    command = f"git -C {_folder(folder)} rev-parse --show-toplevel 2>/dev/null"
    argv = ["sh", "-c", command] if host == hosts.LOCAL else [
        "ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=3", host, command]
    try:
        proc = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
        out, _ = await asyncio.wait_for(proc.communicate(), timeout=3)
    except asyncio.TimeoutError:
        proc.kill()
        await proc.wait()
        return default_name(folder)
    except OSError:
        return default_name(folder)
    return default_name(out.decode(errors="replace").strip()) if proc.returncode == 0 else default_name(folder)


def script(name, folder, start):
    say = lambda msg: f"{{ echo {shlex.quote(msg)}; exit 3; }}"
    s = (f"cd -- {_folder(folder)} 2>/dev/null || {say(f'no such folder: {folder}')}; "
         f"tmux has-session -t {shlex.quote('=' + name)} 2>/dev/null && "
         f"{say(f'a session named {name} already exists')}; "
         f'tmux new-session -d -s {shlex.quote(name)} -c "$PWD"')
    if start != "shell":
        # typed into the new session's own shell: a bare `tmux new … claude` runs without the
        # login PATH, where claude lives
        command = "claude" if start == "claude" else shlex.join(start)
        s += f"; tmux send-keys -t {shlex.quote('=' + name + ':')} {shlex.quote(command)} Enter"
    return s


def argv(host, name, folder, start):
    s = script(name, folder, start)
    if host == hosts.LOCAL:
        return ["sh", "-c", s]
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, s]


async def create(host, name, folder, start):
    """None when the session exists now, else what went wrong."""
    try:
        proc = await asyncio.create_subprocess_exec(
            *argv(host, name, folder, start), stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        out, err = await asyncio.wait_for(proc.communicate(), timeout=10)
    except (OSError, asyncio.TimeoutError):
        return f"couldn't reach {hosts.label(host)}"
    if proc.returncode == 0:
        return None
    return (out or err).decode(errors="replace").strip() or f"failed on {hosts.label(host)}"


class NewSession(ModalScreen):
    """Dismisses with (host, name) once the session exists, or None."""

    CSS = """
    NewSession { align: center middle; }
    #form { width: 60; height: auto; border: round $accent; background: $panel; padding: 0 1; }
    #form Horizontal { height: auto; }
    #form Label { width: 8; padding-top: 1; }
    #form Input, #form Select { width: 1fr; }
    #start { width: 1fr; border: none; }
    #error { color: $error; height: auto; }
    #buttons { align-horizontal: right; }
    #buttons Button { margin-left: 1; }
    """

    def __init__(self, hosts, host):
        super().__init__()
        self.hosts, self.host = hosts, host
        self._auto_name = default_name("~")
        self.presets = load_presets()
        self._name_timer = None

    def compose(self):
        with Vertical(id="form") as form:
            form.border_title = "New session"
            with Horizontal():
                yield Label("Host")
                yield Select([(hosts.label(h), h) for h in self.hosts], value=self.host,
                             allow_blank=False, id="host")
            with Horizontal():
                yield Label("Folder")
                yield Input("~", id="folder")
            with Horizontal():
                yield Label("Name")
                yield Input(self._auto_name, id="name")
            with Horizontal():
                yield Label("Start")
                with RadioSet(id="start"):
                    yield RadioButton("shell", value=True, id="shell")
                    yield RadioButton("claude", id="claude")
                    for i, name in enumerate(self.presets):
                        yield RadioButton(name, id=f"preset-{i}")
            yield Static("", id="error")
            with Horizontal(id="buttons"):
                yield Button("Cancel", id="cancel")
                yield Button("Create", id="create", variant="success")

    def on_input_changed(self, event):
        if event.input.id == "folder":
            name = self.query_one("#name", Input)
            if name.value == self._auto_name:  # still the default: follow the folder
                self._auto_name = name.value = default_name(event.value.strip())
            self._queue_name_lookup()

    def on_select_changed(self, event):
        if event.select.id == "host":
            self._queue_name_lookup()

    def _queue_name_lookup(self):
        folder = self.query_one("#folder", Input).value.strip() or "~"
        host = self.query_one("#host", Select).value
        if self._name_timer:
            self._name_timer.stop()
        self._name_timer = self.set_timer(
            .25, lambda: self.run_worker(partial(self._suggest_name, folder, host),
                                          group="auto-name", exclusive=True))

    async def _suggest_name(self, folder, host):
        result = await suggest_name(host, folder)
        now_folder = self.query_one("#folder", Input).value.strip() or "~"
        if now_folder != folder or self.query_one("#host", Select).value != host:
            return
        name = self.query_one("#name", Input)
        if name.value == self._auto_name:
            self._auto_name = name.value = result

    async def on_button_pressed(self, event):
        if event.button.id == "cancel":
            self.dismiss(None)
            return
        host = self.query_one("#host", Select).value
        folder = self.query_one("#folder", Input).value.strip() or "~"
        name = self.query_one("#name", Input).value.strip()
        start = "claude" if self.query_one("#claude", RadioButton).value else "shell"
        for i, command in enumerate(self.presets.values()):
            if self.query_one(f"#preset-{i}", RadioButton).value:
                start = command
                break
        error = check_name(name) or await create(host, name, folder, start)
        if error:
            self.query_one("#error", Static).update(error)
        else:
            self.dismiss((host, name))

    def key_escape(self):
        self.dismiss(None)
