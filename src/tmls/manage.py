"""Rename and kill tmux sessions, with a small confirmation dialog."""

import asyncio
import shlex

from textual.containers import Horizontal, Vertical
from textual.screen import ModalScreen
from textual.widgets import Button, Input, Static

from tmls import create, hosts

SHELLS = {"sh", "bash", "dash", "zsh", "fish", "ksh", "csh", "tcsh"}


class ManageError(Exception):
    pass


async def _run(host, command):
    argv = command if host == hosts.LOCAL else [
        "ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, shlex.join(command)]
    try:
        proc = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        out, err = await asyncio.wait_for(proc.communicate(), timeout=8)
    except asyncio.TimeoutError as error:
        proc.kill()
        await proc.wait()
        raise ManageError(f"{hosts.label(host)} timed out") from error
    except OSError as error:
        raise ManageError(str(error)) from error
    if proc.returncode:
        raise ManageError((err or out).decode(errors="replace").strip() or "tmux command failed")
    return out.decode(errors="replace")


async def rename(host, old, new):
    error = create.check_name(new)
    if error:
        return error
    try:
        await _run(host, ["tmux", "rename-session", "-t", "=" + old, new])
    except ManageError as error:
        return str(error)
    return None


async def running_commands(host, name):
    output = await _run(host, ["tmux", "list-panes", "-s", "-t", "=" + name,
                               "-F", "#{pane_current_command}"])
    return sorted({command for command in output.splitlines() if command and command not in SHELLS})


async def kill(host, name):
    try:
        await _run(host, ["tmux", "kill-session", "-t", "=" + name])
    except ManageError as error:
        return str(error)
    return None


class SessionActions(ModalScreen):
    CSS = """
    SessionActions { align: center middle; }
    #session-actions { width: 56; height: auto; border: round $accent;
                       background: $panel; padding: 1 2; }
    #session-actions Input { width: 1fr; }
    #session-actions Horizontal { height: auto; align-horizontal: right; }
    #session-actions Button { margin-left: 1; }
    #action-error { color: $error; height: auto; }
    #kill-warning { height: auto; }
    """

    def __init__(self, session):
        super().__init__()
        self.session = session
        self.confirming = False

    def compose(self):
        with Vertical(id="session-actions") as box:
            box.border_title = f"{self.session.host} · {self.session.name}"
            yield Static("New name")
            yield Input(self.session.name, id="new-name")
            yield Static("", id="action-error")
            yield Static("", id="kill-warning")
            with Horizontal():
                yield Button("Cancel", id="cancel-actions")
                yield Button("Rename", id="rename-session", variant="primary")
                yield Button("Kill…", id="kill-session", variant="error")

    async def on_button_pressed(self, event):
        event.stop()
        if event.button.id == "cancel-actions":
            self.dismiss(None)
        elif event.button.id == "rename-session":
            new = self.query_one("#new-name", Input).value.strip()
            error = await rename(self.session.host, self.session.name, new)
            if error:
                self.query_one("#action-error", Static).update(error)
                return
            await self.app.renamed_session(self.session, new)
            self.dismiss(None)
        elif event.button.id == "kill-session":
            if not self.confirming:
                try:
                    commands = await running_commands(self.session.host, self.session.name)
                except ManageError as error:
                    self.query_one("#action-error", Static).update(str(error))
                    return
                warning = f"Kill {self.session.name}? This ends its tmux session."
                if commands:
                    warning += "\nRunning: " + ", ".join(commands)
                self.query_one("#kill-warning", Static).update(warning)
                event.button.label = "Confirm Kill"
                self.confirming = True
                return
            error = await kill(self.session.host, self.session.name)
            if error:
                self.query_one("#action-error", Static).update(error)
                return
            await self.app.killed_session(self.session)
            self.dismiss(None)

    def key_escape(self):
        self.dismiss(None)
