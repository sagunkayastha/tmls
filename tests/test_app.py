import re

import pytest
from textual.widgets import ContentSwitcher, Input, Tabs, TextArea

from tmls import app as tmls_app
from tmls import host_colors, hosts, local, manage, notifications
from tmls import viewer
from tmls.term import Terminal


@pytest.fixture(autouse=True)
def no_local_claude(monkeypatch):
    # the machine running the tests has its own Claude sessions; keep them out
    async def none():
        return []
    monkeypatch.setattr(local, "list_sessions", none)


@pytest.fixture(autouse=True)
def no_real_notifications(monkeypatch, tmp_path):
    monkeypatch.setattr(notifications, "CONFIG", tmp_path / "notifications.json")
    async def none(*args):
        pass
    monkeypatch.setattr(notifications, "emit", none)


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
    tab = app.query_one(f"#tab-{tmls_app.slug('box', name)}")
    await pilot.click(tab, offset=(tab.size.width - 2, 0))
    await pilot.pause(0.2)


async def open_session(app, pilot, name):
    await pilot.click(f"#s-{tmls_app.slug('box', name)}")
    await pilot.pause(0.2)


def test_slug_distinct_for_names_that_sanitize_alike():
    assert tmls_app.slug("box", "my work") != tmls_app.slug("box", "my_work")
    assert tmls_app.slug("a", "b-c") != tmls_app.slug("a-b", "c")


def test_slug_is_a_valid_textual_id_and_stable():
    s = tmls_app.slug("archbox", "Season 36 (v2)")
    assert re.fullmatch(r"[A-Za-z_][A-Za-z0-9_-]*", s)
    assert s == tmls_app.slug("archbox", "Season 36 (v2)")


async def test_sessions_whose_names_sanitize_alike_both_get_rows(fake_hosts, monkeypatch):
    from tmls import prompts
    odd = "it's \"$HOME\""
    names = {"my work", "my_work", odd}
    sent, renamed = [], []

    async def list_host(host):
        return True, [hosts.Session(host, n, 1, False, 0, 3600) for n in sorted(names)]  # idle: Ask sends

    async def send(host, name, text):
        sent.append((host, name, text))

    async def rename(host, old, new):
        renamed.append((host, old, new))
        names.discard(old)
        names.add(new)
    monkeypatch.setattr(hosts, "hosts", lambda remotes: ["box"])
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(prompts, "send", send)
    monkeypatch.setattr(manage, "rename", rename)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: len(app.query(tmls_app.SessionRow)) == 3)
        assert {r.session.name for r in app.query(tmls_app.SessionRow)} == names
        await open_session(app, pilot, "my work")
        await open_session(app, pilot, "my_work")
        await open_session(app, pilot, odd)
        assert tab_names(app) == ["my work ×", "my_work ×", f"{odd} ×"]
        await app.send_prompt("hi")
        assert sent == [("box", odd, "hi")]
        await pilot.click("#copy")
        assert app.clipboard == f"ssh -t box tmux attach -t {odd}"  # the exact name reaches attach_command
        row = app.query_one(f"#s-{tmls_app.slug('box', 'my work')}")
        await pilot.click(row, offset=(tmls_app.ROW_WIDTH - 4, 0))
        assert isinstance(app.screen, manage.SessionActions)
        app.screen.query_one("#new-name", Input).value = "my work 2"
        await pilot.click("#rename-session")
        assert await wait_for(pilot, lambda: tmls_app.slug("box", "my work 2") in app.open_sessions)
        assert renamed == [("box", "my work", "my work 2")]
        assert tmls_app.slug("box", "my_work") in app.open_sessions


async def test_lists_sessions_under_host_headers(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        headers = [str(h.render()) for h in app.query(".host-label")]
        assert headers == ["box", "down · offline"]


async def test_host_header_and_tab_share_color_without_changing_status(fake_hosts, monkeypatch):
    monkeypatch.setattr(host_colors, "load", lambda: {"box": "green"})
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#host-{tmls_app.slug('box', '')}"))
        header = app.query_one(f"#host-{tmls_app.slug('box', '')}")
        assert str(header.render()) == "box"
        await open_session(app, pilot, "alpha")
        tab = app.query_one(f"#tab-{tmls_app.slug('box', 'alpha')}")
        assert header.styles.border_left == tab.styles.border_left
        assert name(tab) == "alpha ×"  # the status mark remains first


async def test_click_opens_session_in_a_tab(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        term = app.query_one(ContentSwitcher).visible_content
        assert isinstance(term, Terminal)
        assert await wait_for(pilot, lambda: "attached-to-alpha" in text(term))
        assert name(app.query_one(Tabs).active_tab) == "alpha ×"


async def test_row_menu_renames_an_open_session_and_reattaches(fake_hosts, monkeypatch):
    called = []
    async def rename(host, old, new):
        called.append((host, old, new))
    monkeypatch.setattr(manage, "rename", rename)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        row = app.query_one(f"#s-{tmls_app.slug('box', 'alpha')}")
        await pilot.click(row, offset=(tmls_app.ROW_WIDTH - 4, 0))
        assert isinstance(app.screen, manage.SessionActions)
        app.screen.query_one("#new-name", Input).value = "renamed"
        await pilot.click("#rename-session")
        assert await wait_for(pilot, lambda: tmls_app.slug("box", "renamed") in app.open_sessions)
        assert called == [("box", "alpha", "renamed")]
        assert tmls_app.slug("box", "alpha") not in app.open_sessions
        assert name(app.query_one(Tabs).active_tab) == "renamed ×"


async def test_kill_requires_confirmation_warns_about_process_and_closes_tab(fake_hosts, monkeypatch):
    killed = []
    async def commands(host, name):
        return ["claude"]
    async def kill(host, name):
        killed.append((host, name))
    monkeypatch.setattr(manage, "running_commands", commands)
    monkeypatch.setattr(manage, "kill", kill)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        row = app.query_one(f"#s-{tmls_app.slug('box', 'alpha')}")
        await pilot.click(row, offset=(tmls_app.ROW_WIDTH - 4, 0))
        await pilot.click("#kill-session")
        assert "claude" in str(app.screen.query_one("#kill-warning").render())
        assert killed == []
        await pilot.pause(.2)
        await pilot.click("#kill-session")
        assert await wait_for(pilot, lambda: tmls_app.slug("box", "alpha") not in app.open_sessions), (killed, app.screen)
        assert killed == [("box", "alpha")]


async def test_new_window_keeps_the_existing_terminal_attached(fake_hosts, monkeypatch):
    made = []
    async def new_window(host, name):
        made.append((host, name))
    monkeypatch.setattr(manage, "new_window", new_window)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        terminal = app.query_one(f"#term-{tmls_app.slug('box', 'alpha')}", Terminal)
        row = app.query_one(f"#s-{tmls_app.slug('box', 'alpha')}")
        await pilot.click(row, offset=(tmls_app.ROW_WIDTH - 4, 0))
        await pilot.click("#new-window")
        assert await wait_for(pilot, lambda: made == [("box", "alpha")])
        assert app.query_one(f"#term-{tmls_app.slug('box', 'alpha')}", Terminal) is terminal


@pytest.mark.parametrize("last", [True, False])
async def test_kill_pane_confirms_process_and_closes_only_the_last_tab(fake_hosts, monkeypatch, last):
    killed = []
    async def pane_info(host, name):
        return "claude", last
    async def kill_pane(host, name):
        killed.append((host, name))
    monkeypatch.setattr(manage, "pane_info", pane_info)
    monkeypatch.setattr(manage, "kill_pane", kill_pane)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        row = app.query_one(f"#s-{tmls_app.slug('box', 'alpha')}")
        await pilot.click(row, offset=(tmls_app.ROW_WIDTH - 4, 0))
        await pilot.click("#kill-pane")
        warning = str(app.screen.query_one("#pane-warning").render())
        assert "claude" in warning and ("this ends the session" in warning) == last
        assert killed == []
        await pilot.pause(.2)
        await pilot.click("#kill-pane")
        assert await wait_for(pilot, lambda: bool(killed))
        assert (tmls_app.slug("box", "alpha") in app.open_sessions) != last


async def test_kill_pane_refreshes_confirmation_if_the_target_changed(fake_hosts, monkeypatch):
    info = iter([("claude", False), ("sleep", True), ("sleep", True)])
    killed = []
    async def pane_info(host, name):
        return next(info)
    async def kill_pane(host, name):
        killed.append((host, name))
    monkeypatch.setattr(manage, "pane_info", pane_info)
    monkeypatch.setattr(manage, "kill_pane", kill_pane)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        row = app.query_one(f"#s-{tmls_app.slug('box', 'alpha')}")
        await pilot.click(row, offset=(tmls_app.ROW_WIDTH - 4, 0))
        await pilot.click("#kill-pane")
        await pilot.pause(.2)
        await pilot.click("#kill-pane")
        warning = str(app.screen.query_one("#pane-warning").render())
        assert "sleep" in warning and "this ends the session" in warning
        assert killed == []
        await pilot.pause(.2)
        await pilot.click("#kill-pane")
        assert await wait_for(pilot, lambda: killed == [("box", "alpha")])


async def test_url_link_opens_external_browser(fake_hosts, monkeypatch):
    opened = []
    monkeypatch.setattr(tmls_app, "open_url", opened.append)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        app.query_one(Terminal).post_message(Terminal.LinkClicked("url", "https://example.com", None))
        assert await wait_for(pilot, lambda: opened == ["https://example.com"])
        assert not app.query("#file-viewer")


async def test_quick_select_copies_a_hash_and_opens_an_external_url(fake_hosts, monkeypatch):
    opened = []
    copied = []
    monkeypatch.setattr(tmls_app, "open_url", opened.append)
    app = tmls_app.Tmls()
    monkeypatch.setattr(app, "copy_to_clipboard", copied.append)
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        term = app.query_one(Terminal)
        term.stream.feed(b"\r\nhttps://example.com\r\ncommit 85388e9\r\n")
        await pilot.press("alt+shift+s")
        assert [h.target for h in term.quick_hits] == ["https://example.com", "85388e9"]
        first = term.quick_hits[0]
        assert "".join(segment.text for segment in term.render_line(first.y))[:first.x + 1].endswith("a")
        await pilot.press("b")
        assert copied == ["85388e9"] and not term.quick_hits
        await pilot.press("alt+shift+s", "shift+a")
        assert opened == ["https://example.com"] and not term.quick_hits


async def test_file_link_opens_read_only_viewer_beside_terminal(fake_hosts, monkeypatch):
    async def load_file(host, session, path):
        return "/tmp/term.py", "\n".join(f"line {n}" for n in range(1, 101))
    monkeypatch.setattr(viewer, "load_file", load_file)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        term = app.query_one(Terminal)
        term.post_message(Terminal.LinkClicked("file", "src/term.py", 40))
        assert await wait_for(pilot, lambda: app.query("#file-viewer"))
        pane = app.query_one("#file-viewer")
        area = pane.query_one(TextArea)
        assert area.read_only
        assert await wait_for(pilot, lambda: area.cursor_location == (39, 0))
        assert "line 40" in area.text
        assert term.has_focus
        assert pane.size.width >= 35
        assert app.query_one("#terms").size.width >= 35


async def test_file_viewer_closes_with_button_or_escape_and_restores_focus(fake_hosts, monkeypatch):
    async def load_file(host, session, path):
        return "/tmp/term.py", "line 1\nline 2\n"
    monkeypatch.setattr(viewer, "load_file", load_file)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        term = app.query_one(Terminal)
        term.post_message(Terminal.LinkClicked("file", "term.py", 2))
        assert await wait_for(pilot, lambda: app.query("#file-viewer"))
        await pilot.click("#viewer-close")
        assert await wait_for(pilot, lambda: not app.query("#file-viewer"))
        assert term.has_focus
        term.post_message(Terminal.LinkClicked("file", "term.py", 2))
        assert await wait_for(pilot, lambda: app.query("#file-viewer"))
        app.query_one("#viewer-text", TextArea).focus()
        await pilot.press("escape")
        assert await wait_for(pilot, lambda: not app.query("#file-viewer"))
        assert term.has_focus


async def test_second_file_replaces_viewer_and_tab_switch_keeps_it(fake_hosts, monkeypatch):
    async def load_file(host, session, path):
        return f"/tmp/{path}", path
    monkeypatch.setattr(viewer, "load_file", load_file)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        term = app.query_one(Terminal)
        term.post_message(Terminal.LinkClicked("file", "first.py", 1))
        assert await wait_for(pilot, lambda: app.query("#file-viewer"))
        term.post_message(Terminal.LinkClicked("file", "second.py", 1))
        assert await wait_for(pilot, lambda: "second.py" in app.query_one(TextArea).text)
        assert len(app.query("#file-viewer")) == 1
        await open_session(app, pilot, "beta")
        assert app.query_one("#file-viewer")
        assert "second.py" in app.query_one(TextArea).text


async def test_file_read_error_notifies_without_opening_viewer(fake_hosts, monkeypatch):
    async def load_file(host, session, path):
        raise viewer.ViewerError("binary file")
    monkeypatch.setattr(viewer, "load_file", load_file)
    app = tmls_app.Tmls()
    notices = []
    monkeypatch.setattr(app, "notify", lambda message, **kwargs: notices.append(message))
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        app.query_one(Terminal).post_message(Terminal.LinkClicked("file", "bad.bin", 1))
        assert await wait_for(pilot, lambda: notices)
        assert "binary file" in notices[-1]
        assert not app.query("#file-viewer")


async def test_tabs_switch_and_do_not_duplicate(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
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
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        await pilot.click("#copy")
        assert app.clipboard == "ssh -t box tmux attach -t alpha"


async def test_open_launches_a_new_terminal_window(fake_hosts, monkeypatch):
    launched = []
    monkeypatch.setattr(tmls_app, "launch_window", lambda argv: launched.append(argv))
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
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
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        await pilot.click(f"#tab-{tmls_app.slug('box', 'alpha')}", offset=(1, 0))
        await pilot.pause(0.2)
        assert tab_names(app) == ["alpha ×", "beta ×"]
        assert name(app.query_one(Tabs).active_tab) == "alpha ×"


async def test_x_closes_shown_tab_and_moves_to_neighbour(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        term = app.query_one(f"#term-{tmls_app.slug('box', 'beta')}")
        await click_x(pilot, app, "beta")
        assert tab_names(app) == ["alpha ×"]
        assert not app.query(f"#term-{tmls_app.slug('box', 'beta')}")
        assert term.exited  # its tmux client was hung up
        assert app.query_one(ContentSwitcher).visible_content.id == f"term-{tmls_app.slug('box', 'alpha')}"
        assert tmls_app.slug("box", "beta") not in app.open_sessions
        assert not app.query_one(f"#s-{tmls_app.slug('box', 'beta')}").has_class("open")
        assert app.query_one(f"#s-{tmls_app.slug('box', 'alpha')}").has_class("current")


async def test_x_closes_a_tab_that_is_not_shown(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        await open_session(app, pilot, "beta")
        await click_x(pilot, app, "alpha")
        assert tab_names(app) == ["beta ×"]
        assert app.query_one(ContentSwitcher).visible_content.id == f"term-{tmls_app.slug('box', 'beta')}"
        assert not app.query(f"#term-{tmls_app.slug('box', 'alpha')}")


async def test_closing_last_tab_shows_the_hint_again(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        await click_x(pilot, app, "alpha")
        assert tab_names(app) == []
        assert app.query_one(ContentSwitcher).visible_content.id == "empty"
        assert app.current is None
        assert not app.query_one(f"#s-{tmls_app.slug('box', 'alpha')}").has_class("current")
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


async def test_a_host_slower_than_the_refresh_interval_does_not_cancel_every_refresh(monkeypatch):
    import asyncio
    slow = {"now": 0, "most": 0}

    async def list_host(host):
        if host == "slow":
            slow["now"] += 1
            slow["most"] = max(slow["most"], slow["now"])
            try:
                await asyncio.sleep(0.7)
            finally:
                slow["now"] -= 1
        return True, [hosts.Session(host, "alpha", 1, False, 0, 0)]
    monkeypatch.setattr(hosts, "hosts", lambda remotes: ["slow", "fast"])
    monkeypatch.setattr(hosts, "label", lambda h: h)
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(tmls_app, "REFRESH_SECONDS", 0.2)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: "fast" in [h for h, _, _ in app._results], timeout=3)
        assert app.query(tmls_app.SessionRow)
        await pilot.pause(1)
        assert slow["most"] == 1  # ticks while the slow host answers don't start a second poll


async def test_a_refresh_asked_for_during_a_poll_runs_right_after_it(monkeypatch):
    import asyncio
    state = {"name": "alpha", "calls": 0}

    async def list_host(host):
        name = state["name"]  # what the host says now; a rename after this isn't in this answer
        state["calls"] += 1
        await asyncio.sleep(0.3)
        return True, [hosts.Session(host, name, 1, False, 0, 3600)]
    monkeypatch.setattr(hosts, "hosts", lambda remotes: ["box"])
    monkeypatch.setattr(hosts, "label", lambda h: h)
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(tmls_app, "REFRESH_SECONDS", 30)  # only the asked-for refresh can show it
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: "alpha" in rows(app))
        before = state["calls"]
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: state["calls"] == before + 1)
        state["name"] = "renamed"
        app.refresh_sessions()  # as renamed_session does, while that poll is still out
        assert await wait_for(pilot, lambda: "renamed" in rows(app), timeout=2)


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
        await pilot.click(f"#s-{tmls_app.slug('box', 'beta')}")
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
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
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
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('kitty', 'notes')}"))
        assert "box" in [str(h.render()) for h in app.query(".host-label")]
        await pilot.click(f"#s-{tmls_app.slug('kitty', 'notes')}")
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
        assert app.query_one(f"#s-{tmls_app.slug('box', 'ask')}").tooltip == "permission prompt"
        assert app.query_one(f"#s-{tmls_app.slug('box', 'broke')}").tooltip is None


async def test_each_tab_shows_its_session_mark_in_front(clock_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        await pilot.click(f"#s-{tmls_app.slug('box', 'beta')}")
        await pilot.click(f"#s-{tmls_app.slug('box', 'alpha')}")
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


async def test_context_fill_shows_at_the_end_of_the_second_line(clock_hosts, monkeypatch):
    async def list_host(host):
        return True, [hosts.Session(host, "Season-36", 1, False, 3600, 3600, "idle", 0, title="NERSC_Training",
                                    model="claude-opus-5-5", context=754_000),
                      hosts.Session(host, "notes", 1, False, 3600, 3600, "idle", 0, title="notes",
                                    model="claude-opus-5-5", context=50_000)]
    monkeypatch.setattr(hosts, "list_host", list_host)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        lines = {n: r.split("\n") for n, r in rows(app).items()}
        assert lines["Season-36"][1].startswith("  NERSC_Train") and lines["Season-36"][1].endswith(" 75%")
        assert len(lines["Season-36"][1]) == tmls_app.ROW_WIDTH
        assert lines["notes"][1].strip() == "5%"  # same name as tmux: the line only holds the meter


async def test_keyboard_into_the_list_move_and_attach(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        assert isinstance(app.focused, Terminal)
        cursor = lambda: [r.session.name for r in app.query(".cursor")]
        await pilot.press("alt+shift+up")  # from the terminal, which takes every other key
        assert await wait_for(pilot, lambda: isinstance(app.focused, tmls_app.SessionList))
        assert cursor() == ["alpha"]  # starts on the tab being shown
        await pilot.press("j")
        assert cursor() == ["beta"]
        await pilot.press("j")  # stays on the last row
        await pilot.press("up")
        assert cursor() == ["alpha"]
        await pilot.press("down", "enter")
        assert await wait_for(pilot, lambda: name(app.query_one(Tabs).active_tab) == "beta ×")
        assert await wait_for(pilot, lambda: isinstance(app.focused, Terminal))


async def test_escape_leaves_the_list_without_changing_anything(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        await pilot.press("alt+shift+up", "j")
        assert await wait_for(pilot, lambda: isinstance(app.focused, tmls_app.SessionList))
        await pilot.press("escape")
        assert await wait_for(pilot, lambda: isinstance(app.focused, Terminal))
        assert tab_names(app) == ["alpha ×"]


async def test_enter_on_the_shown_session_goes_back_to_its_terminal(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        await pilot.press("alt+shift+up", "enter")
        assert await wait_for(pilot, lambda: isinstance(app.focused, Terminal))


async def test_alerts_collect_marks_that_need_you_and_jump_to_the_session(clock_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        bell = app.query_one("#alerts-button")
        assert str(bell.label) == "🔔"  # what's already there at startup isn't news
        clock_hosts["claude"]["beta"] = ("idle", 3650)  # finished
        clock_hosts["claude"]["alpha"] = ("busy", 3690)  # started: not an alert
        clock_hosts["now"] = 3700
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: str(bell.label) == "🔔1")
        await pilot.click("#alerts-button")
        lines = app.query(tmls_app.AlertLine)
        assert app.query_one("#alerts").display and [l.session.name for l in lines] == ["beta"]
        assert "◆ beta" in str(lines.first().render())
        assert str(bell.label) == "🔔"  # seen
        await pilot.click(lines.first())
        assert await wait_for(pilot, lambda: name(app.query_one(Tabs).active_tab) == "beta ×")
        assert not app.query_one("#alerts").display


async def test_alerts_panel_does_not_resize_the_terminal(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        term = app.query_one(Terminal)
        before = term.size
        await pilot.click("#alerts-button")
        await pilot.pause(0.2)
        assert app.query_one("#alerts").display and term.size == before  # floats over it


async def test_notification_switches_in_bell_panel_persist(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await pilot.click("#alerts-button")
        assert "Desktop ON" in str(app.query_one("#notify-desktop").label)
        assert "Sound OFF" in str(app.query_one("#notify-sound").label)
        assert "Silence focused ON" in str(app.query_one("#notify-focused").label)
        await pilot.click("#notify-desktop")
        await pilot.click("#notify-sound")
        await pilot.click("#notify-focused")
        assert not app.notifications.desktop
        assert app.notifications.sound
        assert not app.notifications.silence_focused
        assert notifications.load() == app.notifications


async def test_notification_switches_and_focused_suppression_hook_into_alerts(fake_hosts, monkeypatch):
    emitted = []
    async def emit(*args):
        emitted.append(args)
    monkeypatch.setattr(notifications, "emit", emit)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        alpha = next(s for _, _, ss in app._results for s in ss if s.name == "alpha")
        beta = next(s for _, _, ss in app._results for s in ss if s.name == "beta")
        app._alert(alpha, "done")
        assert app.alerts == [] and app.unread == 0
        app._alert(beta, "waiting")
        assert await wait_for(pilot, lambda: len(emitted) == 1)
        assert emitted[0][1:4] == ("box", "beta", "waiting")
        assert [mark for _, _, mark in app.alerts] == ["waiting"]
        app._alert(beta, "failed")
        assert [mark for _, _, mark in app.alerts] == ["failed", "waiting"]
        assert len(emitted) == 1  # ✕ stays in the inbox only
        app.notifications.silence_focused = False
        app._alert(alpha, "done")
        assert await wait_for(pilot, lambda: len(emitted) == 2)


async def test_a_redraw_during_the_local_lookup_does_not_crash(clock_hosts, monkeypatch):
    # a tab switch can redraw while _refresh is still waiting on the kitty lookup
    app = tmls_app.Tmls()

    async def interleaved():
        await app._render_rows()
        return []
    monkeypatch.setattr(local, "list_sessions", interleaved)
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))


async def test_plus_on_a_host_line_creates_a_session_there_and_opens_it(fake_hosts, monkeypatch):
    from tmls import create
    made = []

    async def fake(host, name, folder, start):
        made.append((host, name, folder, start))
    monkeypatch.setattr(create, "create", fake)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#add-{tmls_app.slug('box', '')}"))
        assert not app.query(f"#add-{tmls_app.slug('down', '')}")  # offline: nowhere to create
        await pilot.click(f"#add-{tmls_app.slug('box', '')}")
        assert await wait_for(pilot, lambda: isinstance(app.screen, create.NewSession))
        app.screen.query_one("#folder").value = "~/proj"
        await pilot.pause()
        await pilot.click("#create")
        assert await wait_for(pilot, lambda: tab_names(app) == ["proj ×"])
    assert made == [("box", "proj", "~/proj", "shell")]


async def test_ask_panel_sends_saved_recent_or_typed_messages_to_the_shown_session(clock_hosts, monkeypatch):
    from textual.widgets import Input
    from tmls import prompts
    sent = []

    async def send(host, name, text):
        sent.append((host, name, text))

    async def recent(host, name):
        return ["run the tests"]
    monkeypatch.setattr(prompts, "send", send)
    monkeypatch.setattr(prompts, "recent", recent)
    monkeypatch.setattr(prompts, "saved", lambda: ["What's the progress?"])
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await open_session(app, pilot, "alpha")
        await pilot.click("#ask-button")
        assert await wait_for(pilot, lambda: len(app.query(tmls_app.PromptLine)) == 2)
        assert app.query_one("#ask").border_title == "Send to alpha"
        await pilot.click(app.query(tmls_app.PromptLine).first())
        assert await wait_for(pilot, lambda: sent == [("box", "alpha", "What's the progress?")])
        assert not app.query_one("#ask").display
        await pilot.click("#ask-button")
        assert await wait_for(pilot, lambda: app.query(tmls_app.PromptLine))
        app.query_one("#ask-input", Input).value = "hello there"
        await pilot.press("enter")
        assert await wait_for(pilot, lambda: sent[-1] == ("box", "alpha", "hello there"))


async def test_ask_without_a_tab_just_hints(fake_hosts):
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.click("#ask-button")
        await pilot.pause(0.2)
        assert not app.query_one("#ask").display


async def test_working_ask_queues_typed_and_saved_messages_then_can_clear(clock_hosts, monkeypatch):
    from tmls import prompts
    sent = []

    async def send(host, name, text):
        sent.append((host, name, text))

    monkeypatch.setattr(prompts, "send", send)
    monkeypatch.setattr(prompts, "recent", lambda h, n: async_recent())
    monkeypatch.setattr(prompts, "saved", lambda: ["check progress"])

    async def async_recent():
        return []

    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'beta')}"))
        await open_session(app, pilot, "beta")
        await pilot.click("#ask-button")
        app.query_one("#ask-input", Input).value = "first"
        await pilot.press("enter")
        assert await wait_for(pilot, lambda: "1✉ ●" in rows(app)["beta"])
        assert sent == []
        await pilot.click("#ask-button")
        assert app.query_one("#ask").border_title == "Send to beta · 1 queued"
        await pilot.click(app.query(tmls_app.PromptLine).first())
        assert await wait_for(pilot, lambda: "2✉ ●" in rows(app)["beta"])
        assert sent == []
        await pilot.click("#ask-button")
        await pilot.click("#ask-clear")
        assert await wait_for(pilot, lambda: "✉" not in rows(app)["beta"])
        assert app.query_one("#ask").border_title == "Send to beta"
        assert sent == []


async def test_ask_queues_while_a_permission_prompt_waits_instead_of_typing_into_it(clock_hosts, monkeypatch):
    from tmls import prompts
    sent = []

    async def send(host, name, text):
        sent.append((host, name, text))

    async def list_host(host):
        return True, [hosts.Session(host, "ask", 1, False, 3600, 3600, "waiting", 3590, waiting="permission prompt")]
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(prompts, "send", send)
    app = tmls_app.Tmls()
    notices = []
    monkeypatch.setattr(app, "notify", lambda message, **kwargs: notices.append(message))
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        await open_session(app, pilot, "ask")
        assert rows(app)["ask"].endswith("?")
        await app.send_prompt("hello")
        assert sent == []  # Enter would pick the highlighted dialog option
        assert app.queue[tmls_app.slug("box", "ask")] == ["hello"]
        assert any(n.startswith("Queued for ask") for n in notices)


async def test_session_names_with_markup_brackets_can_be_asked_and_notified(clock_hosts, monkeypatch):
    from tmls import prompts
    sent = []
    odd = "x[/]y"

    async def send(host, name, text):
        sent.append((host, name, text))

    async def recent(host, name):
        return []

    async def list_host(host):
        return True, [hosts.Session(host, odd, 1, False, 0, 3600)]
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(prompts, "send", send)
    monkeypatch.setattr(prompts, "recent", recent)
    monkeypatch.setattr(prompts, "saved", lambda: [])
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        await open_session(app, pilot, odd)
        await pilot.click("#ask-button")
        assert await wait_for(pilot, lambda: app.query_one("#ask").display)
        await app.send_prompt("hi")  # notifies "Sent to x[/]y."
        await pilot.pause(0.3)  # the toast renders
        assert sent == [("box", odd, "hi")]
        await pilot.click("#copy")  # notifies "Copied: ... x[/]y"
        await pilot.pause(0.3)


async def test_queue_releases_one_message_per_done_or_idle_change(clock_hosts, monkeypatch):
    from tmls import prompts
    sent = []

    async def send(host, name, text):
        sent.append((host, name, text))

    monkeypatch.setattr(prompts, "send", send)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'beta')}"))
        await open_session(app, pilot, "beta")
        await app.send_prompt("one")
        await app.send_prompt("two")
        assert sent == []
        await app.open_session(next(s for _, _, ss in app._results for s in ss if s.name == "alpha"))
        assert await wait_for(pilot, lambda: app.current == tmls_app.slug("box", "alpha"))
        clock_hosts["claude"]["beta"] = ("waiting", 3650)
        clock_hosts["now"] = 3700
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("?"))
        assert sent == []
        clock_hosts["claude"]["beta"] = ("idle", 3710)
        clock_hosts["now"] = 3720
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: sent == [("box", "beta", "one")])
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("◆"))
        await pilot.click(f"#s-{tmls_app.slug('box', 'beta')}")  # ◆ → ○ from looking is still the same finished turn
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("○"))
        assert sent == [("box", "beta", "one")]
        app.refresh_sessions()
        await pilot.pause(0.2)
        assert sent == [("box", "beta", "one")]
        clock_hosts["claude"]["beta"] = ("busy", 3730)
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("●"))
        clock_hosts["claude"]["beta"] = ("idle", 3750)
        clock_hosts["now"] = 3800
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: sent == [("box", "beta", "one"), ("box", "beta", "two")])


async def test_failed_transition_keeps_queue_until_a_later_turn(clock_hosts, monkeypatch):
    from tmls import prompts
    sent = []
    state = {"status": "busy", "failed": False, "now": 3600}

    async def list_host(host):
        return True, [hosts.Session(host, "beta", 1, False, state["now"], state["now"],
                                    state["status"], state["now"] - 10, failed=state["failed"])]

    async def send(host, name, text):
        sent.append((host, name, text))

    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(prompts, "send", send)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'beta')}"))
        await open_session(app, pilot, "beta")
        await app.send_prompt("retry after failure")
        state.update(status="idle", failed=True, now=3700)
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("✕"))
        assert app.queue == {tmls_app.slug("box", "beta"): ["retry after failure"]} and sent == []
        state.update(status="busy", failed=False, now=3720)
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: rows(app)["beta"].endswith("●"))
        state.update(status="idle", now=3740)
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: sent == [("box", "beta", "retry after failure")])


async def test_open_ask_title_tracks_delivery_count(clock_hosts, monkeypatch):
    from tmls import prompts

    async def send(host, name, text):
        return None

    async def recent(host, name):
        return []

    monkeypatch.setattr(prompts, "send", send)
    monkeypatch.setattr(prompts, "recent", recent)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'beta')}"))
        await open_session(app, pilot, "beta")
        await app.send_prompt("one")
        await app.send_prompt("two")
        await pilot.click("#ask-button")
        assert await wait_for(pilot, lambda: app.query_one("#ask").border_title ==
                              "Send to beta · 2 queued")
        clock_hosts["claude"]["beta"] = ("idle", 3650)
        clock_hosts["now"] = 3700
        app.refresh_sessions()
        assert await wait_for(pilot, lambda: app.queue.get(tmls_app.slug("box", "beta")) == ["two"])
        assert app.query_one("#ask").border_title == "Send to beta · 1 queued"


async def test_failed_queue_keeps_messages_and_rename_moves_it(clock_hosts, monkeypatch):
    from tmls import prompts
    sent = []

    async def send(host, name, text):
        sent.append((host, name, text))

    monkeypatch.setattr(prompts, "send", send)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'beta')}"))
        await open_session(app, pilot, "beta")
        await app.send_prompt("keep me")
        old = app.open_sessions[tmls_app.slug("box", "beta")]
        await app.renamed_session(old, "gamma")
        assert app.queue == {tmls_app.slug("box", "gamma"): ["keep me"]}
        await app.killed_session(hosts.Session("box", "gamma", 1, False, 0, 0))
        assert app.queue == {}
        assert sent == []
        await pilot.pause(0.2)


async def test_waiting_permission_prompts_can_be_answered_from_the_alerts_panel(clock_hosts, monkeypatch):
    from tmls import approve
    answered = []

    async def list_host(host):
        return True, [hosts.Session(host, "ask", 1, False, 3600, 3600, "waiting", 3590, waiting="permission prompt"),
                      hosts.Session(host, "quiz", 1, False, 3600, 3600, "waiting", 3590, waiting="input needed")]

    async def current(host, name):
        return ["Bash command", "touch /tmp/x"]

    async def answer(host, name, shown, yes):
        answered.append((host, name, shown, yes))
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(approve, "current", current)
    monkeypatch.setattr(approve, "answer", answer)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 40)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        await pilot.click("#alerts-button")
        assert await wait_for(pilot, lambda: len(app.query(tmls_app.Approval)) == 1)  # not "input needed"
        box = app.query_one(tmls_app.Approval)
        assert "touch /tmp/x" in str(box.query_one(".request").render())
        await pilot.click(box.query_one(".yes"))
        assert await wait_for(pilot, lambda: answered == [("box", "ask", ["Bash command", "touch /tmp/x"], True)])
        assert await wait_for(pilot, lambda: not app.query(tmls_app.Approval))


async def test_alerts_panel_does_not_read_permission_prompts_of_kitty_only_sessions(clock_hosts, monkeypatch):
    from tmls import approve
    asked = []
    win = local.Window("sock", 4, False)

    async def list_host(host):
        return True, [hosts.Session(host, "ask", 1, False, 3600, 3600, "waiting", 3590, waiting="permission prompt")]

    async def kitty():
        return [hosts.Session(hosts.KITTY, "notes", 1, False, 0, 3600, "waiting", 3590, win,
                              waiting="permission prompt")]

    async def current(host, name):
        asked.append(host)
        return ["Bash command", "touch /tmp/x"]
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(local, "list_sessions", kitty)
    monkeypatch.setattr(approve, "current", current)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 40)) as pilot:
        assert await wait_for(pilot, lambda: len(app.query(tmls_app.SessionRow)) == 2)
        assert rows(app)["notes"].endswith("?")
        await pilot.click("#alerts-button")
        assert await wait_for(pilot, lambda: len(app.query(tmls_app.Approval)) == 1)
        assert asked == ["box"]  # kitty has no tmux pane to read


async def test_permission_prompt_text_is_shown_verbatim_not_as_markup():
    from textual.app import App
    shown = ["Bash command", "sed 's/[/]/_/g' f", "src/[id]/page.tsx", "[b]not bold[/b]"]

    class Box(App):
        def compose(self):
            yield tmls_app.Approval(hosts.Session("box", "ask", 1, False, 0, 0), shown)
    app = Box()
    async with app.run_test(size=(80, 20)) as pilot:
        await pilot.pause()
        request = str(app.query_one(".request").render())
        assert "[/]" in request and "src/[id]/page.tsx" in request and "[b]not bold[/b]" in request


async def test_desktop_notifications_name_the_host_as_the_sidebar_does(fake_hosts, monkeypatch):
    # internally this machine is "local"; the popup should say its hostname
    emitted = []
    async def emit(*args):
        emitted.append(args)
    monkeypatch.setattr(notifications, "emit", emit)
    monkeypatch.setattr(hosts, "label", lambda h: f"label-of-{h}")
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        app._alert(hosts.Session("box", "beta", 1, False, 0, 0), "done")
        assert await wait_for(pilot, lambda: emitted and emitted[0][1] == "label-of-box")


async def test_click_on_second_line_right_end_opens_the_session_not_the_menu(clock_hosts, monkeypatch):
    async def list_host(host):
        return True, [hosts.Session(host, "Season-36", 1, False, 3600, 3600, "idle", 0, title="NERSC_Training")]
    monkeypatch.setattr(hosts, "list_host", list_host)
    opened = []
    async def open_session(session):
        opened.append(session.name)
    app = tmls_app.Tmls()
    monkeypatch.setattr(app, "open_session", open_session)
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(tmls_app.SessionRow))
        row = app.query_one(tmls_app.SessionRow)
        await pilot.click(row, offset=(tmls_app.ROW_WIDTH - 4, 1))
        await pilot.pause()
        assert not isinstance(app.screen, manage.SessionActions)
        assert opened == ["Season-36"]


async def test_enter_in_the_name_field_renames(fake_hosts, monkeypatch):
    called = []
    async def rename(host, old, new):
        called.append((host, old, new))
    monkeypatch.setattr(manage, "rename", rename)
    app = tmls_app.Tmls()
    async with app.run_test(size=(120, 30)) as pilot:
        assert await wait_for(pilot, lambda: app.query(f"#s-{tmls_app.slug('box', 'alpha')}"))
        await pilot.click(f"#s-{tmls_app.slug('box', 'alpha')}", offset=(tmls_app.ROW_WIDTH - 4, 0))
        assert isinstance(app.screen, manage.SessionActions)
        app.screen.query_one("#new-name", Input).value = "renamed"
        app.screen.query_one("#new-name", Input).focus()
        await pilot.press("enter")
        assert await wait_for(pilot, lambda: called == [("box", "alpha", "renamed")])
        assert await wait_for(pilot, lambda: not isinstance(app.screen, manage.SessionActions))
