"""Find tmux sessions on this machine and on remote hosts reached over ssh."""
import asyncio
import json
import re
import shlex
import shutil
import socket
from dataclasses import dataclass
from pathlib import Path

LOCAL = "local"
KITTY = "kitty"  # this machine's Claude sessions outside tmux (see local.py)
CONFIG = Path.home() / ".config" / "tmls" / "hosts"
SKETCHPAD = CONFIG.parent / "sketchpad"  # optional: the sketchpad hub's URL on one line
# tmux prints tabs in -F output as "_"; ":" is safe because tmux bans it in session names.
# window_activity is the last output; session_activity only moves on keypresses.
FORMAT = "#{session_name}:#{session_windows}:#{session_attached}:#{window_activity}"
QUIET = 30  # seconds without output before a non-Claude session counts as finished
# Claude Code keeps ~/.claude/sessions/<pid>.json (status, statusUpdatedAt, tmux pane) per running
# session; its status line redraws every minute, so output alone can't tell busy from idle.
# Only live pids' .json files: stale ones outlive crashes, and the .key files next to them are secrets.
RUNNING = {"busy", "shell"}  # "shell": the turn is over but a monitor or background shell still runs
# After each session's file, from the newest replies in its transcript: "failed" when the newest is
# an API error, and "usage" with the model and token counts of the newest real one (context fill).
# [^\\] skips fields quoted inside a message, where JSON escapes the quotes (doubled for ugrep).
# The final `true`: a false test last would fail the whole listing.
CLAUDE = ('echo ---; for f in "$HOME"/.claude/sessions/*.json; do p=${f##*/}; '
          'kill -0 "${p%.json}" 2>/dev/null || continue; cat "$f"; echo; '
          'i=$(sed -n \'s/.*"sessionId":"\\([^"]*\\)".*/\\1/p\' "$f"); '
          'for t in "$HOME"/.claude/projects/*/"$i".jsonl; do [ -f "$t" ] || continue; '
          'a=$(tail -c 300000 "$t" | grep \'[^\\\\]"type":"assistant"\'); '
          'printf "%s\\n" "$a" | tail -n 1 | grep -q \'[^\\\\]"isApiErrorMessage":true\' && echo failed; '
          'echo usage $(printf "%s\\n" "$a" | grep -v \'[^\\\\]"isApiErrorMessage":true\' | tail -n 1 '
          '| grep -o \'"model":"[^"]*"\\|"[a-z_]*input_tokens":[0-9]*\' | head -n 4); done; done; true')


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
    kitty: object = None       # local.Window for KITTY sessions: click focuses it instead of attaching
    waiting: str | None = None  # what Claude waits on you for ("permission prompt", "input needed", ...)
    failed: bool = False        # Claude's newest reply is an API error
    title: str | None = None    # Claude's name for the conversation (/rename, or one it made up)
    model: str | None = None    # model of Claude's newest reply
    context: int = 0            # tokens in context at Claude's newest reply


def parse(host, out):
    """First line is the host's `date +%s`, then one line per window (newest output wins), then
    after "---" Claude Code's session files. Lines a login script printed are skipped; ValueError
    when there's no clock line."""
    tmux, _, claude = out.partition("\n---\n")
    lines = tmux.splitlines()
    start = next((i for i, line in enumerate(lines) if line.isdigit()), None)
    if start is None:
        raise ValueError("no clock line in the listing")
    now, lines = lines[start], lines[start + 1:]
    sessions = {}
    for line in lines:
        try:
            name, windows, attached, activity = line.rsplit(":", 3)
            int(windows), int(activity)
        except ValueError:
            continue
        if name not in sessions or int(activity) > sessions[name].activity:
            sessions[name] = Session(host, name, int(windows), attached != "0", int(activity), int(now))
    files = []
    for line in claude.splitlines():
        if line == "failed" and files:
            files[-1]["failed"] = True
            continue
        if line.startswith("usage") and files:
            files[-1].update(usage(line))
            continue
        try:
            files.append(json.loads(line))
        except ValueError:
            continue
    for c in files:
        s = sessions.get((c.get("tmux") or "").split(":")[0])
        if s is None:
            continue
        status, since = c.get("status"), c.get("statusUpdatedAt", 0) // 1000
        # two Claudes in one session: waiting on you wins, then running, then the latest change
        if s.claude is None or rank(status, since) > rank(s.claude, s.claude_since):
            s.claude, s.claude_since = status, since
            s.waiting, s.failed, s.title = c.get("waitingFor"), bool(c.get("failed")), c.get("name")
            s.model, s.context = c.get("model"), c.get("context", 0)
    return list(sessions.values())


def usage(line):
    """{"model", "context"} from a `usage "model":"…" "input_tokens":N …` line."""
    fields = dict(re.findall(r'"(\w+)":"?([^"\s]*)"?', line))
    if "model" not in fields:
        return {}
    tokens = ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens")
    return {"model": fields["model"], "context": sum(int(fields.get(k) or 0) for k in tokens)}


def context_pct(s):
    """How full Claude's context is, in percent, or None without a reply. Claude Code doesn't save
    the window; Claude 5 models run with 1M, older ones with 200k (unless opted in to 1M)."""
    if not s.model:
        return None
    m = re.match(r"claude-[a-z]+-(\d+)", s.model)
    limit = 1_000_000 if m and int(m.group(1)) >= 5 else 200_000
    return min(100, s.context * 100 // limit)


def rank(status, since):
    return status == "waiting", status in RUNNING, since


def list_argv(host):
    script = f"date +%s; tmux list-windows -a -F {shlex.quote(FORMAT)} && {{ {CLAUDE}; }}"
    if host == LOCAL:
        return ["sh", "-c", script]
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, script]


def status(s, seen, started):
    """"waiting" (on you), "running", "failed", "done" (finished after tmls started and after
    you last looked), or "idle". Looking doesn't clear waiting or failed: they still need you."""
    if s.claude:
        if s.claude == "waiting":
            return "waiting"
        if s.claude in RUNNING:
            return "running"
        if s.failed:
            return "failed"
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
    if host == KITTY:
        return f"{socket.gethostname()} · kitty"
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
        try:
            return True, parse(host, out.decode())
        except ValueError:  # not a listing (e.g. a login script's output only): treat as unreachable
            return False, []
    return b"no server running" in err, []
