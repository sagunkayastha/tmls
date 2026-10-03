import subprocess

import pytest

from tmls import approve, hosts

DIALOG = """\
● Creating empty marker file
  ⎿  $ touch /tmp/x
────────────────────────────────────────────────────────────
 Bash command
 Tip: auto mode handles these prompts for you — choose "switch to auto mode" below
 Create empty marker file
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 touch /tmp/x
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and always allow access to /tmp from this project
   4. No
 Esc to cancel · Tab to amend
"""


def test_request_is_the_dialog_without_tips_or_choices():
    assert approve.request(DIALOG) == ["Bash command", "Create empty marker file", "touch /tmp/x"]


def test_capture_targets_claudes_pane_when_given():
    assert approve._capture_script("w", "%7") == "tmux capture-pane -p -t %7"
    assert approve._capture_script("w") == "tmux capture-pane -p -t =w:"


async def test_answer_reads_and_answers_claudes_pane(monkeypatch):
    got = []

    async def fake_run(argv, stdin=None):
        got.append(argv[-1])
        return 0, DIALOG
    monkeypatch.setattr(approve.prompts, "_run", fake_run)
    shown = approve.request(DIALOG)
    assert await approve.current(hosts.LOCAL, "w", pane="%7") == shown
    assert await approve.answer(hosts.LOCAL, "w", shown, True, pane="%7") is None
    assert got == ["tmux capture-pane -p -t %7"] * 2 + ["tmux send-keys -t %7 1"]


def test_no_request_when_no_permission_dialog_is_up():
    assert approve.request("❯ ls\n● done\n") is None
    assert approve.request(DIALOG.replace("Do you want to proceed?", "Which one?")) is None


@pytest.fixture
def private_tmux(tmp_path, monkeypatch):
    monkeypatch.setenv("TMUX_TMPDIR", str(tmp_path))
    monkeypatch.delenv("TMUX", raising=False)
    yield tmp_path
    subprocess.run(["tmux", "kill-server"], capture_output=True)


async def test_answer_sends_the_key_only_while_the_same_request_is_up(private_tmux, tmp_path):
    # a stand-in for Claude: prints the dialog, then shows the key it got
    (tmp_path / "dialog.txt").write_text(DIALOG)
    script = f"cat {tmp_path}/dialog.txt; stty raw -echo; dd bs=1 count=1 2>/dev/null | od -An -c; sleep 30"
    subprocess.run(["tmux", "new-session", "-d", "-s", "w", "-x", "100", "-y", "40", script], check=True)
    import asyncio
    for _ in range(50):
        if await approve.current(hosts.LOCAL, "w"):
            break
        await asyncio.sleep(0.1)
    shown = await approve.current(hosts.LOCAL, "w")
    assert shown == ["Bash command", "Create empty marker file", "touch /tmp/x"]
    assert await approve.answer(hosts.LOCAL, "w", ["something else"], True)  # changed: refuse
    assert await approve.answer(hosts.LOCAL, "w", shown, True) is None
    await asyncio.sleep(0.3)
    out = subprocess.run(["tmux", "capture-pane", "-p", "-t", "=w:"], capture_output=True, text=True).stdout
    assert out.rstrip().endswith("1")
