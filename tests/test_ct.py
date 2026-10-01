import os
import subprocess

import pytest

from tmls import ct


def test_session_name_comes_from_the_folder():
    assert ct.session_name("/home/me/Others/tmls") == "tmls"
    assert ct.session_name("/home/me/v1.2:x") == "v1_2_x"  # tmux rewrites . and :


def test_inside_tmux_it_is_just_claude(monkeypatch):
    monkeypatch.setenv("TMUX", "/tmp/tmux-1000/default,1,0")
    assert ct.plan("/p", ["--resume"]) == [["claude", "--resume"]]


def test_outside_tmux_it_starts_or_rejoins_a_session(monkeypatch):
    monkeypatch.delenv("TMUX", raising=False)
    steps = ct.plan("/home/me/proj", ["-c"], exists=False)
    assert steps[0] == ["tmux", "new-session", "-d", "-s", "proj", "-c", "/home/me/proj"]
    # typed into the session's shell, which has the login PATH that claude needs
    assert steps[1] == ["tmux", "send-keys", "-t", "=proj:", "claude -c", "Enter"]
    assert steps[-1] == ["tmux", "attach", "-t", "=proj"]
    assert ct.plan("/home/me/proj", ["-c"], exists=True) == [["tmux", "attach", "-t", "=proj"]]


@pytest.fixture
def private_tmux(tmp_path, monkeypatch):
    monkeypatch.setenv("TMUX_TMPDIR", str(tmp_path))
    monkeypatch.delenv("TMUX", raising=False)
    yield tmp_path
    subprocess.run(["tmux", "kill-server"], capture_output=True)


def test_exists_checks_the_exact_name(private_tmux):
    subprocess.run(["tmux", "new-session", "-d", "-s", "proj-two"], check=True)
    assert ct.exists("proj-two") and not ct.exists("proj")  # no prefix match
