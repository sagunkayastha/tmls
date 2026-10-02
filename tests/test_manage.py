import asyncio
import subprocess

import pytest

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
