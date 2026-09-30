"""Claude Code sessions on this machine that aren't in tmux, and the kitty windows they run in.

They can't attach in a tab (there's no tmux), so tmls lists them with their status and a click
focuses their kitty window. Needs kitty's `allow_remote_control` and `listen_on unix:...` sockets.
"""
import asyncio
import glob
import json
import os
import time
from dataclasses import dataclass
from pathlib import Path

from tmls import hosts

SESSIONS = Path.home() / ".claude" / "sessions"


@dataclass(frozen=True)
class Window:
    socket: str
    id: int
    focused: bool  # the window you're looking at right now


def parse_kitty_ls(text, sock):
    """{pid: Window} for each window's shell and its foreground processes."""
    out = {}
    for os_window in json.loads(text):
        for tab in os_window["tabs"]:
            for w in tab["windows"]:
                win = Window(sock, w["id"], bool(os_window.get("is_focused") and w.get("is_focused")))
                for pid in [w["pid"], *(p["pid"] for p in w.get("foreground_processes", []))]:
                    out[pid] = win
    return out


def find_window(pid, parents, windows):
    """Nearest ancestor of pid that is a kitty window's process."""
    while pid and pid > 1:
        if pid in windows:
            return windows[pid]
        pid = parents.get(pid)
    return None


def to_sessions(files, windows, parents, now):
    """Interactive Claude sessions outside tmux, from their ~/.claude/sessions/<pid>.json."""
    out, names = [], set()
    for f in files:
        if f.get("tmux") or f.get("kind") != "interactive":
            continue
        name = f.get("name") or os.path.basename(f.get("cwd", "")) or str(f["pid"])
        if name in names:
            name = f"{name} ({f['pid']})"
        names.add(name)
        out.append(hosts.Session(hosts.KITTY, name, 1, False, 0, now, f.get("status"),
                                 f.get("statusUpdatedAt", 0) // 1000, find_window(f["pid"], parents, windows)))
    return out


def _alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        pass
    return True


def _files():
    # only <pid>.json of live processes: stale files outlive crashes; the .key files are secrets
    out = []
    for path in sorted(SESSIONS.glob("*.json")):
        try:
            f = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        if isinstance(f.get("pid"), int) and _alive(f["pid"]):
            out.append(f)
    return out


async def _run(*argv):
    """stdout, or None on failure. Async, not threads: tmls forks ptys, and forking a
    multi-threaded process can deadlock the child."""
    try:
        proc = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
        out, _ = await asyncio.wait_for(proc.communicate(), timeout=2)
    except (OSError, asyncio.TimeoutError):
        return None
    return out.decode() if proc.returncode == 0 else None


async def _windows():
    runtime = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
    out = {}
    for sock in glob.glob(f"{runtime}/kitty-*"):
        text = await _run("kitty", "@", "--to", f"unix:{sock}", "ls")
        if text:
            out.update(parse_kitty_ls(text, sock))
    return out


async def _parents():
    text = await _run("ps", "-eo", "pid=,ppid=") or ""
    return {int(pid): int(ppid) for pid, ppid in (line.split() for line in text.splitlines())}


async def list_sessions():
    files = _files()
    if not files:
        return []
    return to_sessions(files, await _windows(), await _parents(), int(time.time()))


async def focus(window):
    await _run("kitty", "@", "--to", f"unix:{window.socket}", "focus-window", "--match", f"id:{window.id}")
