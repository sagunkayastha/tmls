import asyncio
import subprocess

import pytest
from textual.app import App
from textual.content import Content

from tmls import hosts, manage


async def wait_tmux(argv, expected):
    for _ in range(40):
        if subprocess.check_output(argv, text=True).strip() == expected:
            return True
        await asyncio.sleep(.05)
    return False


@pytest.fixture
def private_tmux(tmp_path, monkeypatch):
    monkeypatch.setenv("TMUX_TMPDIR", str(tmp_path))
    monkeypatch.delenv("TMUX", raising=False)
    yield
    subprocess.run(["tmux", "kill-server"], capture_output=True)


async def test_rename_validates_and_uses_exact_tmux_target(private_tmux):
    subprocess.run(["tmux", "new-session", "-d", "-s", "alpha"], check=True)
    subprocess.run(["tmux", "new-session", "-d", "-s", "alphabet"], check=True)
    assert "names can't" in await manage.rename(hosts.LOCAL, "alpha", "bad.name")
    assert await manage.rename(hosts.LOCAL, "alpha", "renamed") is None
    names = subprocess.check_output(["tmux", "list-sessions", "-F", "#{session_name}"], text=True).splitlines()
    assert set(names) == {"renamed", "alphabet"}


async def test_kill_only_exact_session_and_detect_running_command(private_tmux):
    subprocess.run(["tmux", "new-session", "-d", "-s", "alpha", "sleep", "120"], check=True)
    subprocess.run(["tmux", "new-session", "-d", "-s", "alphabet"], check=True)
    # until exec, a new pane reports its fork's name (sh, tmux) instead of its command
    assert await wait_tmux(["tmux", "display", "-p", "-t", "=alpha:", "#{pane_current_command}"], "sleep")
    assert await wait_tmux(["tmux", "display", "-p", "-t", "=alphabet:", "#{pane_current_command}"],
                           subprocess.check_output(["tmux", "show", "-gv", "default-shell"], text=True).strip().rsplit("/", 1)[-1])
    assert "sleep" in await manage.running_commands(hosts.LOCAL, "alpha")
    assert await manage.running_commands(hosts.LOCAL, "alphabet") == []
    assert await manage.kill(hosts.LOCAL, "alpha") is None
    names = subprocess.check_output(["tmux", "list-sessions", "-F", "#{session_name}"], text=True).splitlines()
    assert names == ["alphabet"]


async def test_window_and_pane_actions_keep_the_active_folder(private_tmux, tmp_path):
    folder = tmp_path / "project"
    folder.mkdir()
    subprocess.run(["tmux", "new-session", "-d", "-s", "alpha", "-c", str(folder)], check=True)
    assert await wait_tmux(["tmux", "display", "-p", "-t", "=alpha:", "#{pane_current_path}"], str(folder))
    assert await manage.new_window(hosts.LOCAL, "alpha") is None
    assert await wait_tmux(["tmux", "display", "-p", "-t", "=alpha:", "#{pane_current_path}"], str(folder))
    windows = subprocess.check_output(["tmux", "list-windows", "-t", "=alpha", "-F", "#{window_name}"], text=True)
    assert len(windows.splitlines()) == 2
    assert await manage.rename_window(hosts.LOCAL, "alpha", "work.py") is None
    active = subprocess.check_output(["tmux", "display", "-p", "-t", "=alpha:",
                                      "#{window_name} #{pane_current_path}"], text=True).strip()
    assert active == f"work.py {folder}"
    assert await manage.split(hosts.LOCAL, "alpha", "h") is None
    assert await wait_tmux(["tmux", "display", "-p", "-t", "=alpha:", "#{pane_current_path}"], str(folder))
    assert await manage.split(hosts.LOCAL, "alpha", "v") is None
    assert await wait_tmux(["tmux", "display", "-p", "-t", "=alpha:", "#{pane_current_path}"], str(folder))
    paths = subprocess.check_output(["tmux", "list-panes", "-t", "=alpha:",
                                     "-F", "#{pane_current_path}"], text=True).splitlines()
    assert paths == [str(folder)] * 3


async def test_kill_pane_info_warns_about_process_and_last_pane(private_tmux):
    subprocess.run(["tmux", "new-session", "-d", "-s", "alpha", "sleep", "120"], check=True)
    subprocess.run(["tmux", "new-session", "-d", "-s", "alphabet"], check=True)
    assert await wait_tmux(["tmux", "display", "-p", "-t", "=alpha:", "#{pane_current_command}"], "sleep")
    assert await manage.pane_info(hosts.LOCAL, "alpha") == ("sleep", True)
    assert await manage.kill_pane(hosts.LOCAL, "alpha") is None
    assert subprocess.run(["tmux", "has-session", "-t", "=alpha"], capture_output=True).returncode != 0
    assert subprocess.run(["tmux", "has-session", "-t", "=alphabet"], capture_output=True).returncode == 0


class Host(App):
    def __init__(self, session):
        super().__init__()
        self.session = session

    def on_mount(self):
        self.push_screen(manage.SessionActions(self.session))


async def test_actions_dialog_shows_bracketed_names_and_errors_verbatim(monkeypatch):
    async def rename(host, old, new):
        return "tmux: bad [/] name"

    async def running(host, name):
        return ["vim[/]"]

    async def pane_info(host, name):
        return "top[/]", False
    monkeypatch.setattr(manage, "rename", rename)
    monkeypatch.setattr(manage, "running_commands", running)
    monkeypatch.setattr(manage, "pane_info", pane_info)
    app = Host(hosts.Session("box", "a[/]b", 1, False, 0, 0))
    async with app.run_test(size=(100, 40)) as pilot:
        box = app.screen.query_one("#session-actions")
        assert Content.from_markup(box.border_title).plain == "box · a[/]b"  # the getter gives markup
        await pilot.click("#rename-session")
        await pilot.pause(.1)
        assert "tmux: bad [/] name" in str(app.screen.query_one("#action-error").render())
        await pilot.click("#kill-session")
        await pilot.pause(.1)
        warning = str(app.screen.query_one("#kill-warning").render())
        assert "Kill a[/]b?" in warning and "Running: vim[/]" in warning
        await pilot.click("#kill-pane")
        await pilot.pause(.1)
        assert "Running: top[/]" in str(app.screen.query_one("#pane-warning").render())


def test_claude_in_matches_pane_and_old_or_new_session_name():
    files = "\n".join([
        '{"tmux": "other:@1.%3", "name": "another tmux server, same pane id"}',
        'not json',
        '{"tmux": "oldname:@0.%3", "name": "mine", "status": "idle"}',
    ])
    assert manage.claude_in(f"%3\n%4\n---\n{files}", {"oldname", "newname"}) == {
        "tmux": "oldname:@0.%3", "name": "mine", "status": "idle", "pane": "%3"}
    assert manage.claude_in(f"%5\n---\n{files}", {"oldname", "newname"}) is None
    assert manage.claude_in(f"%3\n---\n{files}", {"third"}) is None


@pytest.fixture
def claude_home(tmp_path, monkeypatch, private_tmux):
    """A fake ~/.claude/sessions with one live 'Claude' (this test's own pid) in session alpha."""
    import json
    import os
    monkeypatch.setenv("HOME", str(tmp_path))
    (tmp_path / ".claude" / "sessions").mkdir(parents=True)
    subprocess.run(["tmux", "new-session", "-d", "-s", "alpha", "-x", "120", "-y", "20"], check=True)
    pane = subprocess.check_output(["tmux", "list-panes", "-t", "=alpha", "-F", "#{pane_id}"], text=True).strip()

    def write(status, name="old title"):
        (tmp_path / ".claude" / "sessions" / f"{os.getpid()}.json").write_text(json.dumps(
            {"tmux": f"alpha:@0.{pane}", "name": name, "status": status}))
    return write


def screen():
    return subprocess.check_output(["tmux", "capture-pane", "-p", "-t", "=alpha:"], text=True)


async def test_rename_claude_types_rename_when_idle(claude_home):
    claude_home("idle")
    assert await manage.rename_claude(hosts.LOCAL, "alpha", "alpha", "new title") is None
    for _ in range(40):
        if "/rename new title" in screen():
            break
        await asyncio.sleep(.05)
    assert "/rename new title" in screen()


@pytest.mark.parametrize("status,why", [("busy", "working"), ("waiting", "waiting on you")])
async def test_rename_claude_leaves_a_busy_or_waiting_claude_alone(claude_home, status, why):
    claude_home(status)
    note = await manage.rename_claude(hosts.LOCAL, "alpha", "alpha", "new title")
    assert why in note and "/rename new title" in note
    await asyncio.sleep(.3)
    assert "/rename" not in screen()


async def test_rename_claude_skips_sessions_without_claude_or_with_the_name(claude_home):
    claude_home("idle", name="new title")
    assert await manage.rename_claude(hosts.LOCAL, "alpha", "alpha", "new title") is None
    subprocess.run(["tmux", "new-session", "-d", "-s", "plain"], check=True)
    assert await manage.rename_claude(hosts.LOCAL, "plain", "plain", "x") is None
    await asyncio.sleep(.3)
    assert "/rename" not in screen()
