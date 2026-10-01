import subprocess

import pytest

from tmls import hosts, manage


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
    assert "sleep" in await manage.running_commands(hosts.LOCAL, "alpha")
    assert await manage.running_commands(hosts.LOCAL, "alphabet") == []
    assert await manage.kill(hosts.LOCAL, "alpha") is None
    names = subprocess.check_output(["tmux", "list-sessions", "-F", "#{session_name}"], text=True).splitlines()
    assert names == ["alphabet"]
