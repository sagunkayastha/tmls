"""Claude's account-wide 5-hour and weekly usage, for the app's drawer.

Claude Code hands its status line `rate_limits` on stdin, and the user's status line tool
(claude-statusbar) keeps the last stdin in ~/.cache/claude-statusbar/last_stdin.json. The quota is
the account's, so any host's copy will do: the newest one (by file age on its own clock) wins.
That file also holds paths and session details; only the two windows' percent and reset time
ever leave this module.

Wire (on /api/events, once on connect and again when it changes):
  {"t":"usage","usage":{"five_hour":W,"seven_day":W,"at":<epoch s the file was written>}}
  W = {"pct":<int 0..100>,"resets_at":<epoch s, or null once past>} or null; "usage" is null
  when no host has the file.
"""
import asyncio
import json
import math
import time

from tmls import hosts, prompts

EVERY = 60  # seconds between looks
MARK = "==usage=="
# The host's clock, the file's mtime on it (GNU stat, else BSD's), then the file. Nothing when it's missing.
SCRIPT = (f'f="$HOME/.cache/claude-statusbar/last_stdin.json"; echo {MARK}; date +%s; '
          '[ -f "$f" ] && { stat -c %Y "$f" 2>/dev/null || stat -f %m "$f"; head -c 262144 "$f"; }; true')
WINDOWS = ("five_hour", "seven_day")


def window(raw, now):
    """{"pct", "resets_at"} from one rate_limits window, or None. A reset time already past is null."""
    if not isinstance(raw, dict):
        return None
    pct = raw.get("used_percentage")
    if isinstance(pct, bool) or not isinstance(pct, (int, float)) or not math.isfinite(pct):
        return None
    resets = raw.get("resets_at")
    resets = int(resets) if isinstance(resets, (int, float)) and not isinstance(resets, bool) and math.isfinite(resets) and resets > now else None
    return {"pct": max(0, min(100, round(pct))), "resets_at": resets}


def parse(out, now=None):
    """(age in seconds, {"five_hour", "seven_day"}) from SCRIPT's output, or None (no file, not JSON,
    no rate limits). `now`: this machine's clock, for judging reset times."""
    now = time.time() if now is None else now
    _, found, rest = out.partition(MARK + "\n")
    if not found:
        return None
    lines = rest.split("\n", 2)
    if len(lines) < 3:
        return None
    try:
        host_now, mtime = int(lines[0]), int(lines[1])
        limits = json.loads(lines[2]).get("rate_limits")
    except (ValueError, AttributeError):
        return None
    if not isinstance(limits, dict):
        return None
    windows = {key: window(limits.get(key), now) for key in WINDOWS}
    if not any(windows.values()):
        return None
    return max(0, host_now - mtime), windows


def pick(found, now=None):
    """The wire's `usage` from each host's parse() (None where nothing came): the newest, with `at`
    = when its file was written, on this machine's clock. None when no host had one."""
    now = time.time() if now is None else now
    found = [f for f in found if f]
    if not found:
        return None
    age, windows = min(found, key=lambda f: f[0])
    return {**windows, "at": int(now - age)}


async def _read(host):
    try:
        code, out = await prompts._run(hosts.run_argv(host, SCRIPT))
    except Exception:  # one bad host must not stop the rest
        return None
    return parse(out) if code == 0 else None


async def poll_once(app):
    names = [h for h in app["hosts"] if not hosts.is_background(h)]
    usage = pick(await asyncio.gather(*(_read(h) for h in names))) or {}
    if usage == app["usage"]:
        return False
    app["usage"].clear()  # in place: aiohttp frowns on replacing state once running
    app["usage"].update(usage)
    return True


def wire(app):
    """The feed's usage message."""
    return {"t": "usage", "usage": app["usage"] or None}
