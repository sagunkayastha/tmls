"""tmls: tmux sessions from every host on the left, the chosen one live on the right."""
import argparse
import asyncio
import os
import re
import shutil
import subprocess

from rich.text import Text
from textual.app import App
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Button, ContentSwitcher, Static, Tab, Tabs

from tmls import hosts
from tmls.term import Terminal

REFRESH_SECONDS = 5


def slug(host, name):
    return re.sub(r"[^A-Za-z0-9_-]", "_", f"{host}-{name}")


def launch_window(argv):
    """Open argv in a new terminal window: $TERMINAL, else kitty, else the Debian alternative."""
    term = os.environ.get("TERMINAL") or ("kitty" if shutil.which("kitty") else "x-terminal-emulator")
    cmd = [term, *argv] if term == "kitty" else [term, "-e", *argv]
    subprocess.Popen(cmd, start_new_session=True, stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


class SessionRow(Static):
    def __init__(self, session):
        label = Text(session.name)
        if session.attached:
            label.append(" ●", style="dim")  # attached somewhere else too
        super().__init__(label, id=f"s-{slug(session.host, session.name)}")
        self.session = session

    async def on_click(self):
        await self.app.open_session(self.session)


class CloseTab(Tab):
    """A tab whose last cell is an × that closes it."""

    class Closed(Tab.TabMessage):
        pass

    def __init__(self, name, **kw):
        super().__init__(f"{name} ×", **kw)

    def _on_click(self, event):
        if event.x >= self.size.width - 2:  # the × (or the padding after it)
            event.prevent_default()  # otherwise Tab's own handler also activates it
            self.post_message(self.Closed(self))


class Tmls(App):
    TITLE = "tmls"
    CSS = """
    #left { width: 28; border-right: solid $primary-darken-2; }
    #title { padding: 0 1; text-style: bold; }
    .host-label { padding: 1 1 0 1; color: $text-muted; text-style: bold; }
    SessionRow { padding: 0 1 0 2; }
    SessionRow:hover { background: $boost; }
    SessionRow.open { text-style: bold; }
    SessionRow.current { color: #ff8c00; background: $boost; }
    #bar { height: 3; }
    #terms { height: 1fr; }
    #bar Tabs { width: 1fr; }
    #bar Button { min-width: 8; margin-left: 1; }
    #quit { margin-left: 3; }
    #empty { padding: 2 4; color: $text-muted; }
    """

    def __init__(self, remotes=()):
        super().__init__()
        self.remotes = list(remotes)
        self.open_sessions = {}  # slug -> Session
        self.current = None      # slug of the tab being shown
        self._listing = None

    def compose(self):
        with Horizontal():
            with Vertical(id="left"):
                yield Static("tmux", id="title")
                yield VerticalScroll(id="sessions")
            with Vertical(id="right"):
                with Horizontal(id="bar"):
                    yield Tabs(id="tabs")
                    yield Button("Open", id="open", variant="success")
                    yield Button("Copy", id="copy", variant="primary")
                    yield Button("Quit", id="quit", variant="error")
                with ContentSwitcher(id="terms", initial="empty"):
                    yield Static("← pick a session", id="empty")

    def on_mount(self):
        self.refresh_sessions()
        self.set_interval(REFRESH_SECONDS, self.refresh_sessions)

    def refresh_sessions(self):
        self.run_worker(self._refresh(), exclusive=True, group="refresh")

    async def _refresh(self):
        names = hosts.hosts(self.remotes)
        results = await asyncio.gather(*(hosts.list_host(h) for h in names))
        listing = [(h, online, [(s.name, s.windows, s.attached) for s in ss])
                   for h, (online, ss) in zip(names, results)]
        if listing == self._listing:
            return  # rebuilding would flicker and lose the scroll position
        self._listing = listing
        box = self.query_one("#sessions")
        await box.remove_children()
        widgets = []
        for h, (online, ss) in zip(names, results):
            label = hosts.label(h) if online else f"{hosts.label(h)} · offline"
            widgets.append(Static(label, classes="host-label"))
            widgets.extend(SessionRow(s) for s in ss)
        await box.mount_all(widgets)
        self._mark_rows()

    async def open_session(self, session):
        key = slug(session.host, session.name)
        if key not in self.open_sessions:
            self.open_sessions[key] = session
            # add_content keeps it hidden until its tab is active, so the terminals
            # never share the space (a squeezed pyte screen drops its top rows)
            await self.query_one(ContentSwitcher).add_content(
                Terminal(hosts.attach_argv(session.host, session.name), id=f"term-{key}"))
            await self.query_one(Tabs).add_tab(CloseTab(session.name, id=f"tab-{key}"))
        self.query_one(Tabs).active = f"tab-{key}"

    async def on_close_tab_closed(self, event):
        key = event.tab.id.removeprefix("tab-")
        del self.open_sessions[key]
        await self.query_one(Tabs).remove_tab(event.tab)  # activates a neighbour, or clears
        await self.query_one(f"#term-{key}").remove()     # unmounting hangs up its client
        self._mark_rows()

    def on_tabs_cleared(self):
        self.current = None
        self.query_one(ContentSwitcher).current = "empty"
        self._mark_rows()

    def on_tabs_tab_activated(self, event):
        if event.tab is None:
            return
        self.current = event.tab.id.removeprefix("tab-")
        switcher = self.query_one(ContentSwitcher)
        switcher.current = f"term-{self.current}"
        switcher.visible_content.focus()
        self._mark_rows()

    def _mark_rows(self):
        for row in self.query(SessionRow):
            key = slug(row.session.host, row.session.name)
            row.set_class(key in self.open_sessions, "open")
            row.set_class(key == self.current, "current")

    def on_button_pressed(self, event):
        if event.button.id == "quit":
            self.exit()
            return
        session = self.open_sessions.get(self.current)
        if session is None:
            self.notify("Pick a session on the left first.")
            return
        if event.button.id == "open":
            launch_window(hosts.attach_argv(session.host, session.name))
            self.notify(f"Opened {session.name} in a new window.")
        elif event.button.id == "copy":
            cmd = hosts.attach_command(session.host, session.name)
            self.copy_to_clipboard(cmd)
            self.notify(f"Copied: {cmd}")


def main():
    ap = argparse.ArgumentParser(description="tmux sessions from every host in one TUI.")
    ap.add_argument("host", nargs="*", help=f"ssh host to include (default: lines of {hosts.CONFIG})")
    args = ap.parse_args()
    Tmls(args.host or hosts.read_config()).run()
