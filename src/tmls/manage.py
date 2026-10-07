"""Manage tmux sessions, windows and panes from one dialog."""

from rich.text import Text
from textual.containers import Horizontal, Vertical
from textual.screen import ModalScreen
from textual.widgets import Button, Input, Static

from tmls.tmux_ops import (  # noqa: F401  re-exported: the dialog and its tests use them from here
    SHELLS, ManageError, _run, rename, rename_claude, claude_in, running_commands, kill, _change, new_window, rename_window, split, pane_info, kill_pane)


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
            box.border_title = Text(f"{self.session.host} · {self.session.name}")
            yield Static("New name")
            yield Input(self.session.name, id="new-name")
            yield Static("", id="action-error", markup=False)
            yield Static("", id="kill-warning", markup=False)
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
            yield Static("", id="pane-warning", markup=False)
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
            note = await rename_claude(self.session.host, self.session.name, new, new)
            await self.app.renamed_session(self.session, new)
            if note:
                self.app.notify(note, severity="warning", markup=False)
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
