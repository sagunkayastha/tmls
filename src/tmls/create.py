"""New tmux session on a chosen host, in a chosen folder: the command behind the TUI form and the web modal."""
import asyncio
import json
import os
import shlex
from pathlib import Path

from tmls import hosts

PRESETS = Path.home() / ".config" / "tmls" / "agent-presets.json"


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


def script(name, folder, start):
    say = lambda msg: f"{{ echo {shlex.quote(msg)}; exit 3; }}"
    s = (f"cd -- {_folder(folder)} 2>/dev/null || {say(f'no such folder: {folder}')}; "
         f"tmux has-session -t {shlex.quote('=' + name)} 2>/dev/null && "
         f"{say(f'a session named {name} already exists')}; "
         f'tmux new-session -d -s {shlex.quote(name)} -c "$PWD"')
    if start != "shell":
        # typed into the new session's own shell: a bare `tmux new … claude` runs without the
        # login PATH, where claude lives
        command = "claude" if start == "claude" else shlex.join(start)
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
