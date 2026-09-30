import pytest
from textual.widgets import ContentSwitcher, Tabs

from tmls import app as tmls_app
from tmls import hosts
from tmls.term import Terminal


@pytest.fixture
def fake_hosts(monkeypatch):
    state = {"online": {"box": True, "down": False}}

    async def list_host(host):
        if not state["online"][host]:
            return False, []
        return True, [hosts.Session(host, "alpha", 1, False, 0), hosts.Session(host, "beta", 2, True, 0)]

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
    return [t.label.plain for t in app.query_one(Tabs).query("Tab")]


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
        assert app.query_one(Tabs).active_tab.label.plain == "alpha ×"


async def test_tabs_switch_and_do_not_duplicate(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query("#s-box-alpha"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        tabs = app.query_one(Tabs)
        assert tab_names(app) == ["alpha ×", "beta ×"]
        assert tabs.active_tab.label.plain == "beta ×"
        await open_session(app, pilot, "alpha")
        assert tab_names(app) == ["alpha ×", "beta ×"]
        assert tabs.active_tab.label.plain == "alpha ×"
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
        assert app.query_one(Tabs).active_tab.label.plain == "alpha ×"


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
