"""Messages for a session: saved prompts, what you typed to its Claude before, and sending."""
import asyncio
import json
import secrets
import shlex

from tmls import hosts

PROMPTS = hosts.CONFIG.parent / "prompts"  # one saved prompt per line
DEFAULT = ["What's the progress?"]
RECENT = 10


def saved(path=PROMPTS):
    return hosts.read_config(path) or list(DEFAULT)


def _run_argv(host, script):
    if host == hosts.LOCAL:
        return ["sh", "-c", script]
    return ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host, script]


def recent_argv(host, name):
    """User lines from the transcript of the Claude running in tmux session `name`."""
    tmux = shlex.quote(f'"tmux":"{name}:')
    script = ('for f in "$HOME"/.claude/sessions/*.json; do p=${f##*/}; kill -0 "${p%.json}" 2>/dev/null '
              f'&& grep -qF {tmux} "$f" || continue; '
              'i=$(sed -n \'s/.*"sessionId":"\\([^"]*\\)".*/\\1/p\' "$f"); '
              'for t in "$HOME"/.claude/projects/*/"$i".jsonl; do [ -f "$t" ] && tail -c 2000000 "$t" '
              '| grep \'"type":"user"\' | grep -v \'"tool_use_id"\' | tail -n 200; done; done; true')
    return _run_argv(host, script)


def parse_recent(out):
    """Messages you typed (not tool results or commands), newest first, each once."""
    seen = []
    for line in reversed(out.splitlines()):
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        content = entry.get("message", {}).get("content")
        if entry.get("isMeta") or not isinstance(content, str) or content.startswith("<"):
            continue
        if content not in seen:
            seen.append(content)
    return seen[:RECENT]


async def _run(argv, stdin=None):
    """(returncode, output) or (None, why)."""
    try:
        proc = await asyncio.create_subprocess_exec(
            *argv, stdin=asyncio.subprocess.PIPE if stdin is not None else asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.STDOUT)
        out, _ = await asyncio.wait_for(proc.communicate(stdin), timeout=10)
    except asyncio.TimeoutError:
        proc.kill()  # a hung ssh would otherwise be left behind on every call
        await proc.wait()
        return None, "couldn't reach the host"
    except OSError:
        return None, "couldn't reach the host"
    return proc.returncode, out.decode(errors="replace")


async def recent(host, name):
    code, out = await _run(recent_argv(host, name))
    return parse_recent(out) if code == 0 else []


async def send(host, name, text, pane=None):
    """Type `text` into the session and press Enter. None when sent, else what went wrong.

    Goes through tmux's paste buffer (as a bracketed paste when the program asks for one), so
    the tab needn't be open and a multi-line message arrives whole instead of line by line.
    `pane` (Claude's own, "%7") is the target when given, else the window's active pane."""
    t = shlex.quote(pane or f"={name}:")
    buf = f"tmls-{secrets.token_hex(4)}"  # one per send: two sends in the same tick can't swap texts
    script = (f"tmux load-buffer -b {buf} - && "
              f"{{ tmux paste-buffer -p -d -b {buf} -t {t} || {{ tmux delete-buffer -b {buf}; false; }}; }} "
              f"&& sleep 0.2 && tmux send-keys -t {t} Enter")
    code, out = await _run(_run_argv(host, script), stdin=text.encode())
    return None if code == 0 else (out.strip() or f"couldn't send to {name}")
