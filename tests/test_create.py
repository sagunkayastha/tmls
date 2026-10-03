import os
import subprocess
import json

import pytest
from textual.app import App
from textual.widgets import Input

from tmls import create, create_form, hosts


def test_script_expands_home_and_quotes_everything_else():
    script = create.script("my notes", "~/a b;rm -rf x", "shell")
    assert "cd -- \"$HOME\"/'a b;rm -rf x'" in script
    assert "-s 'my notes'" in script and "send-keys" not in script
    assert 'cd -- "$HOME"' in create.script("n", "~", "shell")
    assert "cd -- /srv/x" in create.script("n", "/srv/x", "shell")


def test_claude_starts_inside_the_login_shell_of_the_new_session():
    # a bare `tmux new ... claude` gets a non-login shell without ~/.local/bin on PATH
    assert "send-keys -t =n: claude Enter" in create.script("n", "~", "claude")


def test_named_agent_presets_are_argv_and_shell_quoted(tmp_path, monkeypatch):
    config = tmp_path / "agent-presets.json"
    monkeypatch.setattr(create, "PRESETS", config)
    config.write_text(json.dumps({"Opus plan": ["claude", "--model", "opus", "--permission-mode", "plan"],
                                  "bad": "claude --dangerously-skip-permissions"}))
    assert create.load_presets() == {"Opus plan": ("claude", "--model", "opus", "--permission-mode", "plan")}
    command = create.script("n", "~", ("claude", "--model", "opus", "; touch /tmp/INJECTED"))
    assert "send-keys -t =n: 'claude --model opus '\"'\"'; touch /tmp/INJECTED'\"'\"'' Enter" in command
    assert create.load_presets() == {"Opus plan": ("claude", "--model", "opus", "--permission-mode", "plan")}


def test_names():
    assert create.default_name("~/Pollensense/2026/bloom_26/") == "bloom_26"
    assert create.default_name("~") == "home"
    assert create.check_name("bloom_26") is None
    assert create.check_name("") and create.check_name("a:b") and create.check_name("a.b")


async def test_auto_name_uses_git_root_for_nested_local_folder(tmp_path):
    repo = tmp_path / "ph_forecast"
    nested = repo / "src" / "models"
    nested.mkdir(parents=True)
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    assert await create.suggest_name(hosts.LOCAL, str(nested)) == "ph_forecast"
    other = tmp_path / "scratch"
    other.mkdir()
    assert await create.suggest_name(hosts.LOCAL, str(other)) == "scratch"


def test_argv_runs_locally_or_over_ssh():
    assert create.argv(hosts.LOCAL, "n", "~", "shell")[:2] == ["sh", "-c"]
    assert create.argv("nas", "n", "~", "shell")[:4] == ["ssh", "-o", "BatchMode=yes", "-o"]


@pytest.fixture
def private_tmux(tmp_path, monkeypatch):
    # a tmux server of our own: never touches the user's sessions
    monkeypatch.setenv("TMUX_TMPDIR", str(tmp_path))
    monkeypatch.delenv("TMUX", raising=False)
    yield tmp_path
    subprocess.run(["tmux", "kill-server"], capture_output=True)


async def test_creates_a_real_session_in_the_folder(private_tmux):
    (private_tmux / "proj").mkdir()
    assert await create.create(hosts.LOCAL, "proj", str(private_tmux / "proj"), "shell") is None
    path = subprocess.run(["tmux", "display", "-p", "-t", "=proj:", "#{pane_current_path}"],
                          capture_output=True, text=True).stdout.strip()
    assert os.path.realpath(path) == os.path.realpath(private_tmux / "proj")


async def test_errors_create_nothing(private_tmux):
    assert "no such folder" in await create.create(hosts.LOCAL, "x", str(private_tmux / "missing"), "shell")
    assert await create.create(hosts.LOCAL, "x", str(private_tmux), "shell") is None
    assert "already" in await create.create(hosts.LOCAL, "x", str(private_tmux), "shell")
    out = subprocess.run(["tmux", "ls", "-F", "#{session_name}"], capture_output=True, text=True).stdout
    assert out.split() == ["x"]


class Host(App):
    def __init__(self, **kw):
        super().__init__()
        self.kw, self.result = kw, "unset"

    def on_mount(self):
        self.push_screen(create_form.NewSession(**self.kw), lambda r: setattr(self, "result", r))


async def test_form_creates_with_folder_name_by_default(monkeypatch):
    made = []

    async def fake(host, name, folder, start):
        made.append((host, name, folder, start))
    monkeypatch.setattr(create, "create", fake)
    app = Host(hosts=["local", "archbox"], host="archbox")
    async with app.run_test(size=(100, 30)) as pilot:
        app.screen.query_one("#folder", Input).value = "~/Pollensense/2026/bloom_26"
        await pilot.pause()
        assert app.screen.query_one("#name", Input).value == "bloom_26"
        await pilot.click("#claude")
        await pilot.click("#create")
        await pilot.pause(0.2)
    assert made == [("archbox", "bloom_26", "~/Pollensense/2026/bloom_26", "claude")]
    assert app.result == ("archbox", "bloom_26")


async def test_form_offers_named_agent_presets(tmp_path, monkeypatch):
    monkeypatch.setattr(create, "PRESETS", tmp_path / "presets.json")
    create.PRESETS.write_text(json.dumps({"Opus plan": ["claude", "--model", "opus", "--permission-mode", "plan"]}))
    made = []
    async def fake(host, name, folder, start):
        made.append((host, name, folder, start))
    monkeypatch.setattr(create, "create", fake)
    app = Host(hosts=["archbox"], host="archbox")
    async with app.run_test(size=(100, 30)) as pilot:
        await pilot.click("#preset-0")
        await pilot.click("#create")
        await pilot.pause(.2)
    assert made == [("archbox", "home", "~", ("claude", "--model", "opus", "--permission-mode", "plan"))]


async def test_form_follows_repo_name_until_name_is_edited(monkeypatch):
    async def suggest(host, folder):
        return "ph_forecast" if folder.endswith("/src") else create.default_name(folder)
    monkeypatch.setattr(create, "suggest_name", suggest)
    app = Host(hosts=["archbox"], host="archbox")
    async with app.run_test(size=(100, 30)) as pilot:
        form = app.screen
        form.query_one("#folder", Input).value = "~/ph_forecast/src"
        for _ in range(30):
            await pilot.pause(.05)
            if form.query_one("#name", Input).value == "ph_forecast":
                break
        assert form.query_one("#name", Input).value == "ph_forecast"
        form.query_one("#name", Input).value = "my-agent"
        form.query_one("#folder", Input).value = "~/other/src"
        await pilot.pause(.5)
        assert form.query_one("#name", Input).value == "my-agent"


async def test_form_shows_errors_and_stays_open(monkeypatch):
    async def fake(host, name, folder, start):
        return "no such folder: ~/nope"
    monkeypatch.setattr(create, "create", fake)
    app = Host(hosts=["archbox"], host="archbox")
    async with app.run_test(size=(100, 30)) as pilot:
        app.screen.query_one("#folder", Input).value = "~/nope"
        await pilot.click("#create")
        await pilot.pause(0.2)
        assert isinstance(app.screen, create_form.NewSession)
        assert "no such folder" in str(app.screen.query_one("#error").render())
        await pilot.click("#cancel")
        await pilot.pause(0.2)
    assert app.result is None
