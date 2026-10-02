"""Manage tmux sessions, windows and panes from one dialog."""

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


async def _change(host, command):
    try:
        await _run(host, command)
    except ManageError as error:
        return str(error)
    return None


async def new_window(host, name):
    return await _change(host, ["tmux", "new-window", "-t", "=" + name + ":",
                                "-c", "#{pane_current_path}"])


async def rename_window(host, name, new_name):
    if not new_name.strip():
        return "the window needs a name"
    return await _change(host, ["tmux", "rename-window", "-t", "=" + name + ":", new_name])


async def split(host, name, direction):
    if direction not in {"h", "v"}:
        raise ValueError(direction)
    return await _change(host, ["tmux", "split-window", "-" + direction,
                                "-t", "=" + name + ":", "-c", "#{pane_current_path}"])


async def pane_info(host, name):
    target = "=" + name + ":"
    command = (await _run(host, ["tmux", "display-message", "-p", "-t", target,
                                 "#{pane_current_command}"])).strip()
    panes = (await _run(host, ["tmux", "list-panes", "-s", "-t", target,
                               "-F", "#{pane_id}"])).splitlines()
    return (command if command not in SHELLS else None), len(panes) == 1


async def kill_pane(host, name):
    return await _change(host, ["tmux", "kill-pane", "-t", "=" + name + ":"])


class SessionActions(ModalScreen):
    CSS = """
    SessionActions { align: center middle; }
    #session-actions { width: 56; height: auto; border: round $accent;
                       background: $panel; padding: 1 2; }
    #session-actions Input { width: 1fr; }
    #session-actions Horizontal { height: auto; align-horizontal: right; }
    #session-actions Button { margin-left: 1; }
    #action-error { color: $error; height: auto; }
    #kill-warning, #pane-warning { height: auto; }
    #window-heading { padding-top: 1; text-style: bold; }
    """

    def __init__(self, session):
        super().__init__()
        self.session = session
        self.confirming = False
        self.confirming_pane = False
        self.pane_last = False
        self.pane_confirmation = None

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
            yield Static("Window and pane", id="window-heading")
            yield Input(placeholder="Window name", id="window-name")
            with Horizontal():
                yield Button("New window", id="new-window")
                yield Button("Rename window", id="rename-window")
            with Horizontal():
                yield Button("Split side by side", id="split-horizontal")
                yield Button("Split top/bottom", id="split-vertical")
            yield Static("", id="pane-warning")
            with Horizontal():
                yield Button("Kill pane…", id="kill-pane", variant="error")

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
        elif event.button.id in {"new-window", "rename-window", "split-horizontal", "split-vertical"}:
            action = event.button.id
            if action == "new-window":
                error = await new_window(self.session.host, self.session.name)
            elif action == "rename-window":
                window_name = self.query_one("#window-name", Input).value.strip()
                error = await rename_window(self.session.host, self.session.name, window_name)
            else:
                direction = "h" if action == "split-horizontal" else "v"
                error = await split(self.session.host, self.session.name, direction)
            if error:
                self.query_one("#action-error", Static).update(error)
                return
            self.app.refresh_sessions()
            self.dismiss(None)
        elif event.button.id == "kill-pane":
            try:
                current = await pane_info(self.session.host, self.session.name)
            except ManageError as error:
                self.query_one("#action-error", Static).update(str(error))
                return
            if not self.confirming_pane or current != self.pane_confirmation:
                command, self.pane_last = current
                warning = "Kill active pane?"
                if command:
                    warning += f"\nRunning: {command}"
                if self.pane_last:
                    warning += "\nThis is the last pane; this ends the session."
                self.query_one("#pane-warning", Static).update(warning)
                event.button.label = "Confirm Kill pane"
                self.confirming_pane = True
                self.pane_confirmation = current
                return
            error = await kill_pane(self.session.host, self.session.name)
            if error:
                self.query_one("#action-error", Static).update(error)
                return
            if self.pane_last:
                await self.app.killed_session(self.session)
            else:
                self.app.refresh_sessions()
            self.dismiss(None)

    async def on_input_submitted(self, event):
        event.stop()
        target = {"new-name": "#rename-session", "window-name": "#rename-window"}[event.input.id]
        self.query_one(target, Button).press()

    def key_escape(self):
        self.dismiss(None)
