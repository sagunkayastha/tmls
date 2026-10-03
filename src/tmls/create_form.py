"""The TUI's New session form. Kept apart from create so the web server never imports Textual."""
from functools import partial

from textual.containers import Horizontal, Vertical
from textual.screen import ModalScreen
from textual.widgets import Button, Input, Label, RadioButton, RadioSet, Select, Static

from tmls import create, hosts


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
        self._auto_name = create.default_name("~")
        self.presets = create.load_presets()
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
                self._auto_name = name.value = create.default_name(event.value.strip())
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
        result = await create.suggest_name(host, folder)
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
        error = create.check_name(name) or await create.create(host, name, folder, start)
        if error:
            self.query_one("#error", Static).update(error)
        else:
            self.dismiss((host, name))

    def key_escape(self):
        self.dismiss(None)
