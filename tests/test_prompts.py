import json
import subprocess
import time

import pytest

from tmls import hosts, prompts


def test_saved_prompts_default_and_config(tmp_path):
    assert prompts.saved(tmp_path / "missing") == ["What's the progress?"]
    f = tmp_path / "prompts"
    f.write_text("# mine\nWhat's the progress?\n\nRun the tests\n")
    assert prompts.saved(f) == ["What's the progress?", "Run the tests"]


def test_recent_reads_typed_messages_newest_first():
    dumps = lambda d: json.dumps(d, separators=(",", ":"))
    out = "\n".join([
        dumps({"type": "user", "message": {"role": "user", "content": "first"}}),
        dumps({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "x"}]}}),
        dumps({"type": "user", "message": {"content": "second\nline two"}}),
        dumps({"type": "user", "isMeta": True, "message": {"content": "<command-name>/clear</command-name>"}}),
        dumps({"type": "user", "message": {"content": "first"}}),  # repeated: once, at its newest
        "garbage",
    ])
    assert prompts.parse_recent(out) == ["first", "second\nline two"]


def test_recent_script_finds_the_claude_in_that_tmux_session():
    script = prompts.recent_argv("nas", "my notes")[-1]
    assert '"tmux":"my notes:' in script and ".claude/projects/" in script


@pytest.fixture
def private_tmux(tmp_path, monkeypatch):
    monkeypatch.setenv("TMUX_TMPDIR", str(tmp_path))
    monkeypatch.delenv("TMUX", raising=False)
    yield tmp_path
    subprocess.run(["tmux", "kill-server"], capture_output=True)


async def test_send_pastes_into_the_session_and_presses_enter(private_tmux):
    subprocess.run(["tmux", "new-session", "-d", "-s", "work", "-x", "80", "-y", "10", "cat"], check=True)
    assert await prompts.send(hosts.LOCAL, "work", "hello; $(rm -rf x)") is None
    time.sleep(0.3)
    screen = subprocess.run(["tmux", "capture-pane", "-p", "-t", "=work:"], capture_output=True, text=True).stdout
    assert screen.splitlines()[:2] == ["hello; $(rm -rf x)", "hello; $(rm -rf x)"]  # echo, then cat after Enter


async def test_send_to_a_missing_session_says_so(private_tmux):
    assert await prompts.send(hosts.LOCAL, "nope", "hi")
