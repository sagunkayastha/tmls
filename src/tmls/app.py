"""tmls: tmux sessions from every host on the left, the chosen one live on the right."""
import argparse
import asyncio
import hashlib
import os
import re
import shutil
import subprocess
import time
from dataclasses import replace

from rich.text import Text
from textual.app import App
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Button, ContentSwitcher, Input, Static, Tab, Tabs

from tmls import approve, create, host_colors, hosts, local, manage, notifications, prompts, viewer
from tmls.term import Terminal
from tmls.viewer import FileViewer

REFRESH_SECONDS = 5


def slug(host, name):
    """A widget-id-safe key that stays unique: the readable part can collide ("my work" and
    "my_work"), so a short hash of the exact host and name is appended."""
    digest = hashlib.sha1(f"{host}\0{name}".encode()).hexdigest()[:8]
    return re.sub(r"[^A-Za-z0-9_-]", "_", f"{host}-{name}") + "-" + digest


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
    def __init__(self, session, mark, queued=0):
        count = f"{queued}✉ " if queued else ""
        left = Text(session.name)
        left.truncate(ROW_WIDTH - (5 if session.host != hosts.KITTY else 2) - len(count),
                      overflow="ellipsis", pad=True)
        text = left + (" ⋯ " if session.host != hosts.KITTY else " ") + count + MARKS[mark]
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

    async def on_click(self, event):
        if self.session.host != hosts.KITTY and event.y == 0 and event.x >= ROW_WIDTH - 4:
            self.app.push_screen(manage.SessionActions(self.session))
        else:
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
        reason = session.waiting if mark == "waiting" and session.waiting else ALERT_TEXT[mark]
        text += Text(reason, style="dim")
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


class Approval(Vertical):
    """A permission prompt in the alerts panel, answered without opening the session."""

    def __init__(self, session, shown):
        super().__init__()
        self.session, self.shown = session, shown

    def compose(self):
        yield Static(Text("? ", style="bold #e5c07b") + f"{self.session.name} · permission prompt")
        yield Static("\n".join(self.shown[:8]), classes="request")
        with Horizontal(classes="answers"):
            yield Button("Yes", classes="yes", variant="success")
            yield Button("No", classes="no", variant="error")

    async def on_button_pressed(self, event):
        event.stop()
        yes = event.button.has_class("yes")
        error = await approve.answer(self.session.host, self.session.name, self.shown, yes)
        self.app.notify(error or f"{'Approved' if yes else 'Denied'} in {self.session.name}.",
                        severity="error" if error else "information")
        await self.remove()


class CloseTab(Tab):
    """A tab whose last cell is an × that closes it."""

    class Closed(Tab.TabMessage):
        pass

    def __init__(self, name, mark, host_color, **kw):
        self.session_name = name
        super().__init__(self.text(mark), **kw)
        self.styles.border_left = ("solid", host_color)

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
    #workspace { height: 1fr; }
    #terms { width: 1fr; height: 1fr; }
    FileViewer { width: 50%; height: 1fr; border-left: solid $primary-darken-2; }
    #viewer-header { height: 3; }
    #viewer-title { width: 1fr; padding: 1 0 0 1; text-overflow: ellipsis; }
    #viewer-close { min-width: 3; width: 3; }
    #viewer-text { height: 1fr; }
    #bar Tabs { width: 1fr; }
    #bar Button { min-width: 8; margin-left: 1; }
    #quit { margin-left: 3; }
    #empty { padding: 2 4; color: $text-muted; }
    #alerts { overlay: screen; position: absolute; offset: 30 3; width: 60; height: auto; max-height: 14;
              background: $panel; border: round $accent; border-title-align: left; display: none; }
    AlertLine:hover { background: $boost; }
    Approval { height: auto; border-bottom: dashed $accent; padding-bottom: 1; }
    Approval .request { color: $text-muted; padding-left: 2; }
    Approval .answers { height: auto; padding-left: 2; }
    Approval Button { min-width: 6; margin-right: 1; }
    #notify-controls { height: auto; }
    #notify-controls Button { min-width: 14; margin-right: 1; }
    #notify-focused { min-width: 22; }
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
        self._refreshing = False  # a poll is in flight
        self.started = {}        # host -> host clock at first poll, minus QUIET: older output isn't news
        self.seen = {}           # slug -> host clock when its tab was last on screen
        self._render_lock = asyncio.Lock()  # refresh and clicks both redraw the list
        self.marks = {}          # slug -> last mark shown, for the tabs
        self.queue = {}          # slug -> messages waiting for the next completed turn
        self._sending = set()    # slugs with a queued send in progress
        self.cursor = None       # slug of the row the keyboard is on in the list
        self.alerts = []         # [(HH:MM, Session, mark)], newest first
        self.unread = 0
        self.notifications = notifications.load()
        self.host_colors = host_colors.load()

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
                with Horizontal(id="workspace"):
                    with ContentSwitcher(id="terms", initial="empty"):
                        yield Static("← pick a session", id="empty")

    def on_mount(self):
        self.refresh_sessions()
        self.set_interval(REFRESH_SECONDS, self.refresh_sessions)

    def refresh_sessions(self):
        if self._refreshing:
            return  # a slow host is still answering; cancelling would throw away every host's result
        self._refreshing = True
        self.run_worker(self._refresh(), group="refresh")

    async def _refresh(self):
        try:
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
        finally:
            self._refreshing = False

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
        listing = [(h, online, [(s.name, m, s.waiting, s.title, hosts.context_pct(s),
                                len(self.queue.get(slug(s.host, s.name), ()))) for s, m in sm])
                   for h, online, sm in rows]
        marks = {slug(s.host, s.name): m for _, _, sm in rows for s, m in sm}
        previous = self.marks
        for _, _, sm in rows:
            for s, m in sm:
                old = previous.get(slug(s.host, s.name))
                if m in ALERTS and old is not None and old != m:  # unknown before: not news
                    self._alert(s, m)
        self.marks = marks
        for _, _, sm in rows:
            for s, m in sm:
                key = slug(s.host, s.name)
                old = previous.get(key)
                if old is not None and old != m and self.queue.get(key):
                    if m in {"done", "idle"} and old not in {"done", "idle"} and key not in self._sending:
                        self._sending.add(key)
                        self.run_worker(self._deliver_queued(s), group=f"queue-{key}")
                    elif m == "failed":
                        self.notify(f"{s.name} failed; {len(self.queue[key])} queued message(s) kept.",
                                    severity="warning")
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
            title = Static(label, classes="host-label", id=f"host-{slug(h, '')}".rstrip("-"))
            title.styles.border_left = ("solid", self._host_color(h))
            header = [title]
            if online and h != hosts.KITTY:
                header.append(AddSession(h))
            widgets.append(Horizontal(*header, classes="host-header"))
            widgets.extend(SessionRow(s, m, len(self.queue.get(slug(s.host, s.name), ()))) for s, m in sm)
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
                Terminal(hosts.attach_argv(session.host, session.name), host=session.host,
                         id=f"term-{key}"))
            tab = CloseTab(session.name, self.marks.get(key, "idle"), self._host_color(session.host),
                           id=f"tab-{key}")
            await self.query_one(Tabs).add_tab(tab)
        self.query_one(Tabs).active = f"tab-{key}"

    def _host_color(self, host):
        return host_colors.color_for(host_colors.key_for(host), self.host_colors)

    async def on_close_tab_closed(self, event):
        key = event.tab.id.removeprefix("tab-")
        await self._close_tab(key)

    async def _close_tab(self, key):
        del self.open_sessions[key]
        await self.query_one(Tabs).remove_tab(f"tab-{key}")  # activates a neighbour, or clears
        await self.query_one(f"#term-{key}").remove()     # unmounting hangs up its client
        self._mark_rows()

    async def renamed_session(self, session, new_name):
        old = slug(session.host, session.name)
        new = slug(session.host, new_name)
        selected = self.current
        was_open = old in self.open_sessions
        if was_open:
            await self._close_tab(old)
            await self.open_session(replace(session, name=new_name))
            if selected != old and selected in self.open_sessions:
                self.query_one(Tabs).active = f"tab-{selected}"
        if old in self.seen:
            self.seen[new] = self.seen.pop(old)
        if old in self.queue:
            self.queue[new] = self.queue.pop(old)
        if self.cursor == old:
            self.cursor = new
        self.refresh_sessions()

    async def killed_session(self, session):
        key = slug(session.host, session.name)
        self.queue.pop(key, None)
        if key in self.open_sessions:
            await self._close_tab(key)
        if self.cursor == key:
            self.cursor = None
        self.refresh_sessions()

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
        key = slug(session.host, session.name)
        if self.notifications.silence_focused and (key == self.current or
                                                   (session.kitty and session.kitty.focused)):
            return
        self.alerts.insert(0, (time.strftime("%H:%M"), session, mark))
        del self.alerts[KEEP_ALERTS:]
        self.unread += 1
        self._show_unread()
        if mark in notifications.SYMBOL and (self.notifications.desktop or self.notifications.sound):
            self.run_worker(notifications.emit(self.notifications, hosts.label(session.host), session.name,
                                               mark, session.waiting), group="notifications")

    def _notification_controls(self):
        settings = self.notifications
        return Horizontal(
            Button(f"Desktop {'ON' if settings.desktop else 'OFF'}", id="notify-desktop"),
            Button(f"Sound {'ON' if settings.sound else 'OFF'}", id="notify-sound"),
            Button(f"Silence focused {'ON' if settings.silence_focused else 'OFF'}",
                   id="notify-focused"), id="notify-controls")

    def _toggle_notification(self, key, button):
        old = getattr(self.notifications, key)
        setattr(self.notifications, key, not old)
        try:
            notifications.save(self.notifications)
        except OSError as error:
            setattr(self.notifications, key, old)
            self.notify(f"Could not save notification switch: {error}", severity="error")
        labels = {"desktop": "Desktop", "sound": "Sound", "silence_focused": "Silence focused"}
        button.label = f"{labels[key]} {'ON' if getattr(self.notifications, key) else 'OFF'}"

    def _show_unread(self):
        self.query_one("#alerts-button").label = f"🔔{self.unread or ''}"

    async def toggle_alerts(self):
        panel = self.query_one("#alerts")
        panel.display = not panel.display
        if panel.display:
            panel.border_title = "Alerts · click one to jump"
            await panel.remove_children()
            await panel.mount(self._notification_controls())
            await panel.mount_all([AlertLine(*a) for a in self.alerts] or [Static(" nothing yet")])
            self.unread = 0
            self._show_unread()
            await self._show_approvals(panel)

    async def _show_approvals(self, panel):
        """Sessions at a permission prompt right now, with the live request and Yes/No."""
        asking = [s for _, _, ss in self._results for s in ss
                  if self.marks.get(slug(s.host, s.name)) == "waiting" and s.waiting == "permission prompt"]
        shown = await asyncio.gather(*(approve.current(s.host, s.name) for s in asking))
        boxes = [Approval(s, req) for s, req in zip(asking, shown) if req]
        if boxes and panel.display:
            await panel.mount_all(boxes, before=0)

    async def toggle_ask(self):
        panel = self.query_one("#ask")
        session = self.open_sessions.get(self.current)
        if panel.display or session is None:
            panel.display = False
            if session is None:
                self.notify("Open a session's tab first.")
            return
        self._ask_title(session)
        panel.display = True
        await panel.remove_children()
        await panel.mount_all([Input(placeholder="type a message, Enter sends", id="ask-input"),
                               Button("Clear queue", id="ask-clear"),
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
        key = slug(session.host, session.name)
        if self.marks.get(key) in ("running", "waiting"):
            pending = self.queue.setdefault(key, [])
            pending.append(text)
            self.notify(f"Queued for {session.name} ({len(pending)} waiting).")
            await self._render_rows()
            return
        error = await prompts.send(session.host, session.name, text)
        self.notify(error or f"Sent to {session.name}.", severity="error" if error else "information")

    def _ask_title(self, session):
        count = len(self.queue.get(slug(session.host, session.name), ()))
        self.query_one("#ask").border_title = (f"Send to {session.name} · {count} queued" if count
                                               else f"Send to {session.name}")

    async def _deliver_queued(self, session):
        key = slug(session.host, session.name)
        try:
            pending = self.queue.get(key)
            if not pending or self.marks.get(key) not in {"done", "idle"}:
                return
            message = pending[0]
            error = await prompts.send(session.host, session.name, message)
            if error:
                self.notify(f"Could not send queued message to {session.name}: {error}", severity="error")
                return
            if self.queue.get(key) is pending and pending and pending[0] == message:
                pending.pop(0)
                if not pending:
                    self.queue.pop(key)
                if self.query_one("#ask").display and self.current == key:
                    self._ask_title(session)
                await self._render_rows()
            self.notify(f"Sent queued message to {session.name}.")
        finally:
            self._sending.discard(key)

    def action_focus_list(self):
        self.cursor = self.current
        box = self.query_one(SessionList)
        box.focus()
        box.action_move(0)  # onto the shown session, else the first

    def focus_terminal(self):
        if self.current:
            self.query_one(ContentSwitcher).visible_content.focus()

    async def on_terminal_link_clicked(self, event):
        if event.kind == "url":
            open_url(event.target)
            return
        session = self.open_sessions.get(self.current)
        if session is None:
            return
        try:
            path, text = await viewer.load_file(session.host, session.name, event.target)
        except viewer.ViewerError as error:
            self.notify(f"{event.target}: {error}", severity="error")
            return
        for old in self.query(FileViewer):
            await old.remove()
        await self.query_one("#workspace").mount(FileViewer(session.host, path, event.line, text))

    async def on_file_viewer_closed(self):
        await self.query_one(FileViewer).remove()
        self.focus_terminal()

    def on_button_pressed(self, event):
        if event.button.id == "ask-clear":
            session = self.open_sessions.get(self.current)
            if session:
                self.queue.pop(slug(session.host, session.name), None)
                self._ask_title(session)
                self.run_worker(self._render_rows(), group="render")
            return
        setting = {"notify-desktop": "desktop", "notify-sound": "sound",
                   "notify-focused": "silence_focused"}.get(event.button.id)
        if setting:
            self._toggle_notification(setting, event.button)
            return
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
