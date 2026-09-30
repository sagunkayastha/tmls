"""Find tmux sessions on this machine and on remote hosts reached over ssh."""
import asyncio
import shlex
import shutil
import socket
from dataclasses import dataclass
from pathlib import Path

LOCAL = "local"
CONFIG = Path.home() / ".config" / "tmls" / "hosts"
# tmux prints tabs in -F output as "_"; ":" is safe because tmux bans it in session names.
FORMAT = "#{session_name}:#{session_windows}:#{session_attached}:#{session_activity}"


@dataclass
class Session:
    host: str
    name: str
    windows: int
    attached: bool
    activity: int


def parse(host, out):
    sessions = []
    for line in out.splitlines():
        name, windows, attached, activity = line.rsplit(":", 3)
        sessions.append(Session(host, name, int(windows), attached != "0", int(activity)))
    return sessions


def list_argv(host):
    tmux = ["tmux", "ls", "-F", FORMAT]
    if host == LOCAL:
        return tmux
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, shlex.join(tmux)]


def attach_argv(host, name):
    tmux = ["tmux", "attach", "-t", name]
    return tmux if host == LOCAL else ["ssh", "-t", host, shlex.join(tmux)]


def attach_command(host, name):
    """The attach command as a user would type it (for Copy)."""
    argv = attach_argv(host, name)
    return shlex.join(argv) if host == LOCAL else f'ssh -t {host} "{argv[-1]}"'


def read_config(path=CONFIG):
    """Remote ssh hosts, one per line; blank lines and # comments are skipped. Missing file = none."""
    try:
        lines = path.read_text().splitlines()
    except FileNotFoundError:
        return []
    return [h for h in (line.split("#", 1)[0].strip() for line in lines) if h]


def hosts(remotes):
    """Hosts to show: this machine (if it has tmux) plus remotes that aren't this machine."""
    me = socket.gethostname()
    return ([LOCAL] if shutil.which("tmux") else []) + [h for h in remotes if h != me]


def label(host):
    return socket.gethostname() if host == LOCAL else host


async def list_host(host):
    """Returns (online, sessions). A reachable host with no tmux server is online with no sessions."""
    try:
        proc = await asyncio.create_subprocess_exec(
            *list_argv(host), stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        out, err = await asyncio.wait_for(proc.communicate(), timeout=8)
    except (OSError, asyncio.TimeoutError):
        return False, []
    if proc.returncode == 0:
        return True, parse(host, out.decode())
    return b"no server running" in err, []
