"""tmls: tmux sessions from every host on the left, the chosen one live on the right."""
import argparse
import asyncio
import os
import re
import shutil
import subprocess
import time

from rich.text import Text
from textual.app import App
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Button, ContentSwitcher, Input, Static, Tab, Tabs

from tmls import create, hosts, local, prompts
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


def open_url(url):
    subprocess.Popen(["xdg-open", url], start_new_session=True, stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


ROW_WIDTH = 24  # #left width 28, minus its right border (1) and SessionRow padding (2 + 1)
MARKS = {"running": Text("●", style="bold #4ebf71"), "waiting": Text("?", style="bold #e5c07b"),
         "done": Text("◆", style="bold #ff8c00"), "failed": Text("✕", style="bold #e06c75"),
         "idle": Text("○", style="dim")}


def fill_style(pct):
    return "bold #e06c75" if pct >= 90 else "bold #ff8c00" if pct >= 70 else "dim"


class SessionRow(Static):
    def __init__(self, session, mark):
        left = Text(session.name)
        left.truncate(ROW_WIDTH - 2, overflow="ellipsis", pad=True)
        text = left + " " + MARKS[mark]
        title = session.title if session.title != session.name else None
        pct = hosts.context_pct(session)
        if title or pct is not None:
            second = Text(f"  {title or ''}", style="dim")
            second.truncate(ROW_WIDTH - 5, overflow="ellipsis", pad=True)
            if pct is not None:
                second += Text(f"{pct:>4}%", style=fill_style(pct))
            text += Text("\n") + second
        super().__init__(text, id=f"s-{slug(session.host, session.name)}")
        self.session = session
        if mark == "waiting":
            self.tooltip = session.waiting  # the row has no room for why

    async def on_click(self):
        await self.app.open_session(self.session)


class SessionList(VerticalScroll):
    """The sessions on the left; Alt+Shift+Up focuses it from the terminal."""
    BINDINGS = [
        Binding("j,down", "move(1)", "Next", show=False),
        Binding("k,up", "move(-1)", "Previous", show=False),
        Binding("enter", "attach", "Attach", show=False),
        Binding("escape", "leave", "Back to the terminal", show=False),
    ]

    def action_move(self, step):
        rows = list(self.query(SessionRow))
        keys = [slug(r.session.host, r.session.name) for r in rows]
        if not rows:
            return
        i = keys.index(self.app.cursor) + step if self.app.cursor in keys else 0
        self.app.cursor = keys[max(0, min(i, len(keys) - 1))]
        self.app._mark_rows()
        self.query_one(".cursor").scroll_visible()

    async def action_attach(self):
        for row in self.query(".cursor"):
            await self.app.open_session(row.session)
            if row.session.host != hosts.KITTY:
                self.app.focus_terminal()  # already the shown tab: no tab switch to move focus

    def action_leave(self):
        self.app.focus_terminal()


ALERTS = {"waiting", "done", "failed"}  # marks that need you
ALERT_TEXT = {"waiting": "waiting", "done": "done", "failed": "API error"}
KEEP_ALERTS = 50


class AlertLine(Static):
    """One line in the alerts panel; a click jumps to its session."""

    def __init__(self, when, session, mark):
        text = Text(f"{when}  ") + MARKS[mark] + f" {session.name}  "
        text += Text(session.waiting if mark == "waiting" and session.waiting else ALERT_TEXT[mark], style="dim")
        super().__init__(text)
        self.session = session

    async def on_click(self):
        self.app.query_one("#alerts").display = False
        await self.app.open_session(self.session)


class AddSession(Static):
    """The + on a host line: a new tmux session there."""

    def __init__(self, host):
        super().__init__("+", id=f"add-{slug(host, '')}".rstrip("-"))
        self.host = host

    def on_click(self):
        names = [h for h, online, _ in self.app._results if online and h != hosts.KITTY]
        self.app.push_screen(create.NewSession(names, self.host), self.app.created)


class PromptLine(Static):
    """A saved or earlier message in the Ask panel; a click sends it."""

    def __init__(self, text):
        first = text.splitlines()[0] if text.strip() else text
        super().__init__(Text(f"  {first}" + (" …" if "\n" in text.strip() else ""), no_wrap=True,
                              overflow="ellipsis"))
        self.text = text

    async def on_click(self):
        await self.app.send_prompt(self.text)


class CloseTab(Tab):
    """A tab whose last cell is an × that closes it."""

    class Closed(Tab.TabMessage):
        pass

    def __init__(self, name, mark, **kw):
        self.session_name = name
        super().__init__(self.text(mark), **kw)

    def text(self, mark):
        return MARKS[mark] + f" {self.session_name} ×"

    def show_mark(self, mark):
        self.label = self.text(mark)

    def _on_click(self, event):
        if event.x >= self.size.width - 2:  # the × (or the padding after it)
            event.prevent_default()  # otherwise Tab's own handler also activates it
            self.post_message(self.Closed(self))


class Tmls(App):
    TITLE = "tmls"
    # priority: the terminal forwards every other key to the session. kitty owns ctrl+shift+arrows.
    BINDINGS = [
        Binding("alt+shift+left", "switch_tab(-1)", "Previous tab", priority=True),
        Binding("alt+shift+right", "switch_tab(1)", "Next tab", priority=True),
        Binding("alt+shift+up", "focus_list", "Sessions list", priority=True),
    ]
    CSS = """
    #left { width: 28; border-right: solid $primary-darken-2; }
    #title { padding: 0 1; text-style: bold; }
    .host-label { padding: 1 1 0 1; color: $text-muted; text-style: bold; width: 1fr; }
    .host-header { height: auto; }
    AddSession { padding: 1 2 0 0; width: auto; color: $success; text-style: bold; }
    AddSession:hover { color: $text; }
    SessionRow { padding: 0 1 0 2; }
    SessionRow:hover { background: $boost; }
    SessionRow.open { text-style: bold; }
    SessionRow.current { color: #ff8c00; background: $boost; }
    SessionList:focus SessionRow.cursor { border-left: outer $accent; padding-left: 1; }
    #bar { height: 3; }
    #terms { height: 1fr; }
    #bar Tabs { width: 1fr; }
    #bar Button { min-width: 8; margin-left: 1; }
    #quit { margin-left: 3; }
    #empty { padding: 2 4; color: $text-muted; }
    #alerts { overlay: screen; position: absolute; offset: 30 3; width: 60; height: auto; max-height: 14;
              background: $panel; border: round $accent; border-title-align: left; display: none; }
    AlertLine:hover { background: $boost; }
    #ask { overlay: screen; position: absolute; offset: 20 3; width: 70; height: auto; max-height: 20;
           background: $panel; border: round $accent; border-title-align: left; display: none; }
    #ask .heading { color: $text-muted; text-style: bold; padding-top: 1; }
    PromptLine:hover { background: $boost; }
    """

    def __init__(self, remotes=(), sketchpad=None):
        super().__init__()
        self.remotes = list(remotes)
        self.sketchpad = sketchpad  # sketchpad hub URL; no Sketch button without one
        self.open_sessions = {}  # slug -> Session
        self.current = None      # slug of the tab being shown
        self._listing = None
        self._results = []       # [(host, online, sessions)] from the last poll
        self.started = {}        # host -> host clock at first poll, minus QUIET: older output isn't news
        self.seen = {}           # slug -> host clock when its tab was last on screen
        self._render_lock = asyncio.Lock()  # refresh and clicks both redraw the list
        self.marks = {}          # slug -> last mark shown, for the tabs
        self.cursor = None       # slug of the row the keyboard is on in the list
        self.alerts = []         # [(HH:MM, Session, mark)], newest first
        self.unread = 0

    def compose(self):
        with Horizontal():
            with Vertical(id="left"):
                yield Static("tmux", id="title")
                yield SessionList(id="sessions")
            with Vertical(id="right"):
                with Horizontal(id="bar"):
                    yield Tabs(id="tabs")
                    yield Button("🔔", id="alerts-button")
                    yield Button("Ask", id="ask-button")
                    yield Button("Open", id="open", variant="success")
                    yield Button("Copy", id="copy", variant="primary")
                    if self.sketchpad:
                        yield Button("Sketch", id="sketch", variant="warning")
                    yield Button("Quit", id="quit", variant="error")
                yield VerticalScroll(id="alerts")  # floats over the terminal, so it never resizes it
                yield VerticalScroll(id="ask")
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
        found = [(h, online, ss) for h, (online, ss) in zip(names, results)]
        kitty = await local.list_sessions()
        if kitty:
            found.append((hosts.KITTY, True, kitty))
        for h, _, ss in found:
            if ss:
                self.started.setdefault(h, ss[0].now - hosts.QUIET)
        self._results = found  # only now: a redraw while awaiting must not see hosts without `started`
        await self._render_rows(bool(self._results))

    def _mark(self, s):
        key = slug(s.host, s.name)
        if key == self.current or (s.kitty and s.kitty.focused):
            self.seen[key] = s.now  # on screen, so seen up to now
        return hosts.status(s, self.seen.get(key, 0), self.started[s.host])

    async def _render_rows(self, any_hosts=True):
        async with self._render_lock:  # interleaved redraws would mount the same row twice
            await self._draw_rows(any_hosts)

    async def _draw_rows(self, any_hosts):
        rows = [(h, online, [(s, self._mark(s)) for s in ss]) for h, online, ss in self._results]
        listing = [(h, online, [(s.name, m, s.waiting, s.title, hosts.context_pct(s)) for s, m in sm]) for h, online, sm in rows]
        marks = {slug(s.host, s.name): m for _, _, sm in rows for s, m in sm}
        for _, _, sm in rows:
            for s, m in sm:
                old = self.marks.get(slug(s.host, s.name))
                if m in ALERTS and old is not None and old != m:  # unknown before: not news
                    self._alert(s, m)
        self.marks = marks
        for tab in self.query(CloseTab):
            tab.show_mark(self.marks.get(tab.id.removeprefix("tab-"), "idle"))
        if listing == self._listing:
            return  # rebuilding would flicker and lose the scroll position
        self._listing = listing
        box = self.query_one("#sessions")
        await box.remove_children()
        widgets = []
        if not any_hosts:
            widgets.append(Static(f"no hosts: tmux isn't installed here and {hosts.CONFIG} "
                                  "lists none", classes="host-label"))
        for h, online, sm in rows:
            label = hosts.label(h) if online else f"{hosts.label(h)} · offline"
            header = [Static(label, classes="host-label")]
            if online and h != hosts.KITTY:
                header.append(AddSession(h))
            widgets.append(Horizontal(*header, classes="host-header"))
            widgets.extend(SessionRow(s, m) for s, m in sm)
        await box.mount_all(widgets)
        self._mark_rows()

    async def open_session(self, session):
        key = slug(session.host, session.name)
        if session.host == hosts.KITTY:  # no tmux to attach: jump to its own window
            if session.kitty:
                await local.focus(session.kitty)
                self.seen[key] = session.now
                # not inline: this runs in the clicked row's handler, and the redraw removes that row
                self.run_worker(self._render_rows(), group="render")
            else:
                self.notify(f"{session.name} isn't in a kitty window tmls can reach.")
            return
        if key not in self.open_sessions:
            self.open_sessions[key] = session
            # add_content keeps it hidden until its tab is active, so the terminals
            # never share the space (a squeezed pyte screen drops its top rows)
            await self.query_one(ContentSwitcher).add_content(
                Terminal(hosts.attach_argv(session.host, session.name), id=f"term-{key}"))
            tab = CloseTab(session.name, self.marks.get(key, "idle"), id=f"tab-{key}")
            await self.query_one(Tabs).add_tab(tab)
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
        self.run_worker(self._render_rows(), group="render")  # clears its ◆

    def action_switch_tab(self, step):
        tabs = self.query_one(Tabs)
        if step > 0:
            tabs.action_next_tab()
        else:
            tabs.action_previous_tab()

    def _mark_rows(self):
        for row in self.query(SessionRow):
            key = slug(row.session.host, row.session.name)
            row.set_class(key in self.open_sessions, "open")
            row.set_class(key == self.current, "current")
            row.set_class(key == self.cursor, "cursor")

    def created(self, result):
        if result:  # (host, name): the session exists now
            host, name = result
            self.run_worker(self.open_session(hosts.Session(host, name, 1, False, 0, 0)))
            self.refresh_sessions()

    def _alert(self, session, mark):
        self.alerts.insert(0, (time.strftime("%H:%M"), session, mark))
        del self.alerts[KEEP_ALERTS:]
        self.unread += 1
        self._show_unread()

    def _show_unread(self):
        self.query_one("#alerts-button").label = f"🔔{self.unread or ''}"

    async def toggle_alerts(self):
        panel = self.query_one("#alerts")
        panel.display = not panel.display
        if panel.display:
            panel.border_title = "Alerts · click one to jump"
            await panel.remove_children()
            await panel.mount_all([AlertLine(*a) for a in self.alerts] or [Static(" nothing yet")])
            self.unread = 0
            self._show_unread()

    async def toggle_ask(self):
        panel = self.query_one("#ask")
        session = self.open_sessions.get(self.current)
        if panel.display or session is None:
            panel.display = False
            if session is None:
                self.notify("Open a session's tab first.")
            return
        panel.border_title = f"Send to {session.name}"
        panel.display = True
        await panel.remove_children()
        await panel.mount_all([Input(placeholder="type a message, Enter sends", id="ask-input"),
                               Static("Saved", classes="heading"),
                               *[PromptLine(p) for p in prompts.saved()],
                               Static("Recent", classes="heading")])
        self.query_one("#ask-input").focus()
        earlier = await prompts.recent(session.host, session.name)
        if panel.display:
            await panel.mount_all([PromptLine(p) for p in earlier] or [Static("  none yet")])

    async def on_input_submitted(self, event):
        if event.input.id == "ask-input" and event.value.strip():
            await self.send_prompt(event.value)

    async def send_prompt(self, text):
        session = self.open_sessions.get(self.current)
        self.query_one("#ask").display = False
        self.focus_terminal()
        if session is None:
            return
        error = await prompts.send(session.host, session.name, text)
        self.notify(error or f"Sent to {session.name}.", severity="error" if error else "information")

    def action_focus_list(self):
        self.cursor = self.current
        box = self.query_one(SessionList)
        box.focus()
        box.action_move(0)  # onto the shown session, else the first

    def focus_terminal(self):
        if self.current:
            self.query_one(ContentSwitcher).visible_content.focus()

    def on_button_pressed(self, event):
        if event.button.id == "quit":
            self.exit()
            return
        if event.button.id == "ask-button":
            self.run_worker(self.toggle_ask())
            return
        if event.button.id == "alerts-button":
            self.run_worker(self.toggle_alerts())
            return
        if event.button.id == "sketch":
            open_url(self.sketchpad)
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
    sketchpad = hosts.read_config(hosts.SKETCHPAD)
    Tmls(args.host or hosts.read_config(), sketchpad[0] if sketchpad else None).run()
