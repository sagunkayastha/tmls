import pytest
from textual.widgets import ContentSwitcher, Tabs

from tmls import app as tmls_app
from tmls import hosts, local
from tmls.term import Terminal


@pytest.fixture(autouse=True)
def no_local_claude(monkeypatch):
    # the machine running the tests has its own Claude sessions; keep them out
    async def none():
        return []
    monkeypatch.setattr(local, "list_sessions", none)


@pytest.fixture
def fake_hosts(monkeypatch):
    state = {"online": {"box": True, "down": False}}

    async def list_host(host):
        if not state["online"][host]:
            return False, []
        return True, [hosts.Session(host, "alpha", 1, False, 0, 0, None, 0), hosts.Session(host, "beta", 2, True, 0, 0, None, 0)]

    monkeypatch.setattr(hosts, "hosts", lambda remotes: ["box", "down"])
    monkeypatch.setattr(hosts, "label", lambda h: h)
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(hosts, "attach_argv", lambda h, n: ["sh", "-c", f"echo attached-to-{n}; sleep 5"])
    monkeypatch.setattr(hosts, "attach_command", lambda h, n: f"ssh -t {h} tmux attach -t {n}")
    return state


async def wait_for(pilot, cond, timeout=5):
    for _ in range(int(timeout / 0.05)):
        await pilot.pause(0.05)
        if cond():
            return True
    return False


def text(term):
    return "\n".join(term.vt.display)


def tab_names(app):
    return [name(t) for t in app.query_one(Tabs).query("Tab")]


def name(tab):
    return tab.label.plain[2:]  # drop the status mark in front


async def click_x(pilot, app, name):
    tab = app.query_one(f"#tab-box-{name}")
    await pilot.click(tab, offset=(tab.size.width - 2, 0))
    await pilot.pause(0.2)


async def open_session(app, pilot, name):
    await pilot.click(f"#s-box-{name}")
    await pilot.pause(0.2)


async def test_lists_sessions_under_host_headers(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        headers = [str(h.render()) for h in app.query(".host-label")]
        assert headers == ["box", "down · offline"]


async def test_click_opens_session_in_a_tab(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        term = app.query_one(ContentSwitcher).visible_content
        assert isinstance(term, Terminal)
        assert await wait_for(pilot, lambda: "attached-to-alpha" in text(term))
        assert name(app.query_one(Tabs).active_tab) == "alpha ×"


async def test_tabs_switch_and_do_not_duplicate(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        tabs = app.query_one(Tabs)
        assert tab_names(app) == ["alpha ×", "beta ×"]
        assert name(tabs.active_tab) == "beta ×"
        await open_session(app, pilot, "alpha")
        assert tab_names(app) == ["alpha ×", "beta ×"]
        assert name(tabs.active_tab) == "alpha ×"
        assert await wait_for(pilot, lambda: "attached-to-alpha" in text(app.query_one(ContentSwitcher).visible_content))


async def test_copy_puts_attach_command_on_clipboard(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        await pilot.click("#copy")
        assert app.clipboard == "ssh -t box tmux attach -t alpha"


async def test_open_launches_a_new_terminal_window(fake_hosts, monkeypatch):
    launched = []
    monkeypatch.setattr(tmls_app, "launch_window", lambda argv: launched.append(argv))
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "beta")
        await pilot.click("#open")
        assert launched == [["sh", "-c", "echo attached-to-beta; sleep 5"]]


async def test_buttons_without_a_session_just_hint(fake_hosts, monkeypatch):
    launched = []
    monkeypatch.setattr(tmls_app, "launch_window", lambda argv: launched.append(argv))
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.click("#open")
        await pilot.click("#copy")
        assert launched == [] and app.clipboard == ""


async def test_clicking_tab_body_switches_without_closing(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        await pilot.click("#tab-box-alpha", offset=(1, 0))
        await pilot.pause(0.2)
        assert tab_names(app) == ["alpha ×", "beta ×"]
        assert name(app.query_one(Tabs).active_tab) == "alpha ×"


async def test_x_closes_shown_tab_and_moves_to_neighbour(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        term = app.query_one("#term-box-beta")
        await click_x(pilot, app, "beta")
        assert tab_names(app) == ["alpha ×"]
        assert not app.query("#term-box-beta")
        assert term.exited  # its tmux client was hung up
        assert app.query_one(ContentSwitcher).visible_content.id == "term-box-alpha"
        assert "box-beta" not in app.open_sessions
        assert not app.query_one("#s-box-beta").has_class("open")
        assert app.query_one("#s-box-alpha").has_class("current")


async def test_x_closes_a_tab_that_is_not_shown(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        await click_x(pilot, app, "alpha")
        assert tab_names(app) == ["beta ×"]
        assert app.query_one(ContentSwitcher).visible_content.id == "term-box-beta"
        assert not app.query("#term-box-alpha")


async def test_closing_last_tab_shows_the_hint_again(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        await click_x(pilot, app, "alpha")
        assert tab_names(app) == []
        assert app.query_one(ContentSwitcher).visible_content.id == "empty"
        assert app.current is None
        assert not app.query_one("#s-box-alpha").has_class("current")
        await open_session(app, pilot, "alpha")  # can be reopened afterwards
        assert tab_names(app) == ["alpha ×"]


async def test_quit_button_exits_tmls(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.click("#quit")
        await pilot.pause(0.2)
        assert app._exit


async def test_no_hosts_explains_why(monkeypatch):
    # no local tmux and no hosts file used to leave the list silently blank
    monkeypatch.setattr(hosts, "hosts", lambda remotes: [])
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(".host-label"))
        msg = str(app.query_one(".host-label").render())
        assert "no hosts" in msg and str(hosts.CONFIG) in msg


@pytest.fixture
def clock_hosts(monkeypatch):
    # name -> Claude status and when it last changed (host clock)
    state = {"now": 3600, "claude": {"alpha": ("idle", 0), "beta": ("busy", 3590)}}

    async def list_host(host):
        return True, [hosts.Session(host, n, 1, False, state["now"], state["now"], c, t)
                      for n, (c, t) in state["claude"].items()]

    monkeypatch.setattr(hosts, "hosts", lambda remotes: ["box"])
    monkeypatch.setattr(hosts, "label", lambda h: h)
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(hosts, "attach_argv", lambda h, n: ["sh", "-c", "sleep 5"])
    return state


def rows(app):
    return {r.session.name: str(r.render()).rstrip() for r in app.query(tmls_app.SessionRow)}


async def test_marks_running_and_idle_in_name_order(clock_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        assert list(rows(app)) == ["alpha", "beta"]
        assert rows(app)["beta"].endswith("●") and rows(app)["alpha"].endswith("○")


async def test_needs_me_diamond_when_claude_stops_then_cleared_by_looking(clock_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        clock_hosts["claude"]["beta"] = ("idle", 3650)
        clock_hosts["now"] = 3700
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("◆"))
        await pilot.click("#s-box-beta")
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("○"))
        tab = app.query_one(Tabs).active_tab
        await pilot.click(tab, offset=(tab.size.width - 2, 0))  # close it
        clock_hosts["claude"]["beta"] = ("idle", 3750)  # another turn finished while not shown
        clock_hosts["now"] = 3800
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("◆"))


async def test_sketch_button_opens_the_configured_sketchpad(fake_hosts, monkeypatch):
    opened = []
    monkeypatch.setattr(tmls_app, "open_url", opened.append)
    app = tmls_app.Tmls(sketchpad="http://hub.lan:8790")
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.click("#sketch")
        assert opened == ["http://hub.lan:8790"]


async def test_no_sketch_button_without_a_sketchpad(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)):
        assert not app.query("#sketch")


async def test_mark_stays_on_the_name_line(clock_hosts):
    clock_hosts["claude"]["a-very-long-session-name-indeed"] = ("busy", 3590)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        await pilot.pause(0.1)
        assert [r.size.height for r in app.query(tmls_app.SessionRow)] == [1, 1, 1]


async def test_alt_shift_arrows_switch_tabs_even_from_the_terminal(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        active = lambda: name(app.query_one(Tabs).active_tab)
        assert active() == "beta ×" and isinstance(app.focused, Terminal)
        await pilot.press("alt+shift+left")
        assert await wait_for(pilot, lambda: active() == "alpha ×")
        await pilot.press("alt+shift+right")
        assert await wait_for(pilot, lambda: active() == "beta ×")
        await pilot.press("alt+shift+right")  # wraps around
        assert await wait_for(pilot, lambda: active() == "alpha ×")


async def test_kitty_sessions_listed_and_click_focuses_their_window(fake_hosts, monkeypatch):
    focused = []
    win = local.Window("sock", 4, False)
    async def sessions():
        return [hosts.Session(hosts.KITTY, "notes", 1, False, 0, 3600, "idle", 3590, win)]

    async def focus(w):
        focused.append(w)
    monkeypatch.setattr(local, "list_sessions", sessions)
    monkeypatch.setattr(local, "focus", focus)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query("#s-kitty-notes"))
        assert "box" in [str(h.render()) for h in app.query(".host-label")]
        await pilot.click("#s-kitty-notes")
        await pilot.pause(0.2)
        assert focused == [win]
        assert not app.open_sessions  # no tab: it lives in its own kitty window


async def test_waiting_mark_with_reason_on_hover_and_failed_mark(clock_hosts, monkeypatch):
    async def list_host(host):
        return True, [hosts.Session(host, "ask", 1, False, 3600, 3600, "waiting", 3590, waiting="permission prompt"),
                      hosts.Session(host, "broke", 1, False, 3600, 3600, "idle", 3590, failed=True)]
    monkeypatch.setattr(hosts, "list_host", list_host)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        assert rows(app)["ask"].endswith("?") and rows(app)["broke"].endswith("✕")
        assert app.query_one("#s-box-ask").tooltip == "permission prompt"
        assert app.query_one("#s-box-broke").tooltip is None


async def test_each_tab_shows_its_session_mark_in_front(clock_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        await pilot.click("#s-box-beta")
        await pilot.click("#s-box-alpha")
        labels = lambda: [t.label.plain for t in app.query_one(Tabs).query("Tab")]
        assert await wait_for(pilot, lambda: labels() == ["● beta ×", "○ alpha ×"])
        clock_hosts["claude"]["beta"] = ("idle", 3650)  # finished while alpha is shown
        clock_hosts["now"] = 3700
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: labels() == ["◆ beta ×", "○ alpha ×"])


async def test_claude_rows_show_the_conversation_name_on_a_dim_second_line(clock_hosts, monkeypatch):
    async def list_host(host):
        return True, [hosts.Session(host, "Season-36", 1, False, 3600, 3600, "idle", 0, title="NERSC_Training"),
                      hosts.Session(host, "notes", 1, False, 3600, 3600, "idle", 0, title="notes"),
                      hosts.Session(host, "plain", 1, False, 0, 3600)]
    monkeypatch.setattr(hosts, "list_host", list_host)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        lines = {n: r.split("\n") for n, r in rows(app).items()}
        assert lines["Season-36"][0].endswith("○") and lines["Season-36"][1].strip() == "NERSC_Training"
        assert len(lines["notes"]) == 1 and len(lines["plain"]) == 1  # same name, or no Claude: one line
