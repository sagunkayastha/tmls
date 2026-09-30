"""Find tmux sessions on this machine and on remote hosts reached over ssh."""
import asyncio
import json
import shlex
import shutil
import socket
from dataclasses import dataclass
from pathlib import Path

LOCAL = "local"
CONFIG = Path.home() / ".config" / "tmls" / "hosts"
# tmux prints tabs in -F output as "_"; ":" is safe because tmux bans it in session names.
# window_activity is the last output; session_activity only moves on keypresses.
FORMAT = "#{session_name}:#{session_windows}:#{session_attached}:#{window_activity}"
QUIET = 30  # seconds without output before a non-Claude session counts as finished
# Claude Code keeps ~/.claude/sessions/<pid>.json (status, statusUpdatedAt, tmux pane) per running
# session; its status line redraws every minute, so output alone can't tell busy from idle.
# Only live pids' .json files: stale ones outlive crashes, and the .key files next to them are secrets.
RUNNING = {"busy", "shell"}  # "shell": the turn is over but a monitor or background shell still runs
CLAUDE = ('echo ---; for f in "$HOME"/.claude/sessions/*.json; do p=${f##*/}; '
          'kill -0 "${p%.json}" 2>/dev/null && cat "$f" && echo; done')


@dataclass
class Session:
    host: str
    name: str
    windows: int
    attached: bool
    activity: int  # newest window output, host clock
    now: int       # host clock when listed; compare only against the same host's times
    claude: str | None = None  # Claude Code's status in this session ("busy", "idle", "shell")
    claude_since: int = 0      # when that status began, host clock


def parse(host, out):
    """First line is the host's `date +%s`, then one line per window (newest output wins), then
    after "---" Claude Code's session files."""
    tmux, _, claude = out.partition("\n---\n")
    now, *lines = tmux.splitlines()
    sessions = {}
    for line in lines:
        name, windows, attached, activity = line.rsplit(":", 3)
        if name not in sessions or int(activity) > sessions[name].activity:
            sessions[name] = Session(host, name, int(windows), attached != "0", int(activity), int(now))
    for line in claude.splitlines():
        try:
            c = json.loads(line)
        except ValueError:
            continue
        s = sessions.get((c.get("tmux") or "").split(":")[0])
        if s is None:
            continue
        status, since = c.get("status"), c.get("statusUpdatedAt", 0) // 1000
        # two Claudes in one session: running wins, then the latest change
        if s.claude is None or (status in RUNNING, since) > (s.claude in RUNNING, s.claude_since):
            s.claude, s.claude_since = status, since
    return list(sessions.values())


def list_argv(host):
    script = f"date +%s; tmux list-windows -a -F {shlex.quote(FORMAT)} && {{ {CLAUDE}; }}"
    if host == LOCAL:
        return ["sh", "-c", script]
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, script]


def status(s, seen, started):
    """"running", "done" (finished after tmls started and after you last looked), or "idle"."""
    if s.claude:
        if s.claude in RUNNING:
            return "running"
        finished = s.claude_since
    else:
        if s.now - s.activity < QUIET:
            return "running"
        finished = s.activity
    return "done" if finished > max(seen, started) else "idle"


def attach_argv(host, name):
    # -u: non-interactive ssh often has no UTF-8 locale, and tmux then draws "_" and "lqqk"
    tmux = ["tmux", "-u", "attach", "-t", name]
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
