"""New tmux session on a chosen host, in a chosen folder: the command behind the TUI form and the web modal."""
import asyncio
import json
import os
import shlex
from pathlib import Path

from tmls import hosts

PRESETS = Path.home() / ".config" / "tmls" / "agent-presets.json"
# Agents a new session can start besides a shell and Claude, without a preset (a preset of the
# same name wins).
AGENTS = {"codex": ("codex",), "opencode": ("opencode",)}
# A host's known folders, for the phone's New session: its home, every pane's folder, then the
# projects of Claude's newest prompts (history.jsonl, newest last; only its tail is read).
FOLDERS = ('echo "$HOME"; tmux list-panes -a -F \'#{pane_current_path}\' 2>/dev/null; echo ---; '
           'tail -n 2000 "$HOME"/.claude/history.jsonl 2>/dev/null | grep -o \'"project": *"[^"]*"\'; true')
MAX_FOLDERS = 15


def load_presets():
    try:
        data = json.loads(PRESETS.read_text())
    except (OSError, ValueError):
        return {}
    if not isinstance(data, dict):
        return {}
    return {name: tuple(argv) for name, argv in data.items()
            if isinstance(name, str) and name and isinstance(argv, list) and argv
            and all(isinstance(arg, str) and arg for arg in argv)}


def starts(presets):
    """What a new session can start, in the form's order: [(id, command shown or None)]."""
    out = [("shell", None), ("claude", "claude")]
    out += [(name, " ".join(argv)) for name, argv in AGENTS.items() if name not in presets]
    return out + [(name, " ".join(argv)) for name, argv in presets.items()]


def resolve_start(start, presets):
    """create()'s start for a form's choice: "shell", "claude" or an argv; None when unknown."""
    if start in ("shell", "claude"):
        return start
    return presets.get(start) or AGENTS.get(start)


def parse_folders(out):
    """(home, folders) from FOLDERS's output: Claude's newest projects first, then the panes'
    folders, each once, home itself left out, under home written with ~; at most MAX_FOLDERS."""
    head, _, history = out.partition("\n---\n")
    lines = head.splitlines()
    home = lines[0].strip() if lines else ""
    projects = []
    for line in reversed(history.splitlines()):
        try:
            projects.append(json.loads("{" + line.strip() + "}")["project"])
        except (ValueError, KeyError, TypeError):
            continue
    seen, folders = set(), []
    for path in [*projects, *(line.strip() for line in lines[1:])]:
        if not isinstance(path, str) or not path.startswith("/") or path.rstrip("/") == home.rstrip("/"):
            continue
        if home and path.startswith(home.rstrip("/") + "/"):
            path = "~/" + path[len(home.rstrip("/")) + 1:]
        if path not in seen:
            seen.add(path)
            folders.append(path)
    return home, folders[:MAX_FOLDERS]


async def folders(host):
    """(home, known folders) of host, or None when it can't be reached."""
    try:
        proc = await asyncio.create_subprocess_exec(
            *hosts.run_argv(host, FOLDERS), stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
        out, _ = await asyncio.wait_for(proc.communicate(), timeout=8)
    except asyncio.TimeoutError:
        proc.kill()
        await proc.wait()
        return None
    except OSError:
        return None
    return parse_folders(out.decode(errors="replace")) if proc.returncode == 0 else None


def default_name(folder):
    return os.path.basename(folder.rstrip("/")) if folder.strip("~/") else "home"


def check_name(name):
    """Why tmux can't use this name, or None. tmux rewrites ":" and "." in names, expands "#{...}"
    and runs "#(...)" in them, and reads a leading "-" as a flag."""
    if not name:
        return "the session needs a name"
    if ":" in name or "." in name:
        return "names can't contain : or ."
    if "#" in name or "\0" in name:
        return "names can't contain #"
    if name.startswith("-"):
        return "names can't start with -"
    return None


def _folder(folder):
    # ~ must stay unquoted for the host's shell to expand it; everything else is quoted
    if folder == "~" or folder.startswith("~/"):
        return '"$HOME"' + ("/" + shlex.quote(folder[2:]) if folder[2:] else "")
    return shlex.quote(folder)


async def suggest_name(host, folder):
    """Use the host's git root when the folder is in a repository."""
    command = f"git -C {_folder(folder)} rev-parse --show-toplevel 2>/dev/null"
    argv = ["sh", "-c", command] if host == hosts.LOCAL else [
        "ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=3", host, command]
    try:
        proc = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
        out, _ = await asyncio.wait_for(proc.communicate(), timeout=3)
    except asyncio.TimeoutError:
        proc.kill()
        await proc.wait()
        return default_name(folder)
    except OSError:
        return default_name(folder)
    return default_name(out.decode(errors="replace").strip()) if proc.returncode == 0 else default_name(folder)


def named(command, name):
    """A claude command also gets the session's name (--name), so Claude's own session name
    matches tmux's. Other agents, or a preset that sets a name itself, stay as they are."""
    command = list(command)
    if os.path.basename(command[0]) == "claude" and not any(
            arg in ("-n", "--name") or arg.startswith("--name=") for arg in command[1:]):
        command += ["--name", name]
    return command


def script(name, folder, start):
    say = lambda msg: f"{{ echo {shlex.quote(msg)}; exit 3; }}"
    s = (f"cd -- {_folder(folder)} 2>/dev/null || {say(f'no such folder: {folder}')}; "
         f"tmux has-session -t {shlex.quote('=' + name)} 2>/dev/null && "
         f"{say(f'a session named {name} already exists')}; "
         f'tmux new-session -d -s {shlex.quote(name)} -c "$PWD"')
    if start != "shell":
        # typed into the new session's own shell: a bare `tmux new … claude` runs without the
        # login PATH, where claude lives
        command = shlex.join(named(["claude"] if start == "claude" else start, name))
        s += f"; tmux send-keys -t {shlex.quote('=' + name + ':')} {shlex.quote(command)} Enter"
    return s


def argv(host, name, folder, start):
    s = script(name, folder, start)
    if host == hosts.LOCAL:
        return ["sh", "-c", s]
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, s]


async def create(host, name, folder, start):
    """None when the session exists now, else what went wrong."""
    try:
        proc = await asyncio.create_subprocess_exec(
            *argv(host, name, folder, start), stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        out, err = await asyncio.wait_for(proc.communicate(), timeout=10)
    except (OSError, asyncio.TimeoutError):
        return f"couldn't reach {hosts.label(host)}"
    if proc.returncode == 0:
        return None
    return (out or err).decode(errors="replace").strip() or f"failed on {hosts.label(host)}"
