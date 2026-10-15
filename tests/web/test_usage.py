import asyncio
import json
import time

from aiohttp import web

from tmls import hosts, prompts
from tmls.web import events, usage

NOW = 1_800_000_000


def stdin(five=12, five_at=NOW + 3600, week=51, week_at=NOW + 86400, **extra):
    """A statusline stdin like Claude Code's, with the paths and session details it carries."""
    return json.dumps({
        "session_id": "secret-session", "transcript_path": "/home/me/.claude/projects/x/secret.jsonl",
        "cwd": "/home/me/private", "model": {"id": "claude-opus-5-5"}, "cost": {"total_cost_usd": 3.2},
        "rate_limits": {"five_hour": {"used_percentage": five, "resets_at": five_at},
                        "seven_day": {"used_percentage": week, "resets_at": week_at}, "extra": {"x": 1}},
        **extra,
    })


def output(body, host_now=NOW, mtime=NOW - 30, banner="Welcome to box\n"):
    return f"{banner}{usage.MARK}\n{host_now}\n{mtime}\n{body}"


def test_parse_reads_only_the_two_windows():
    age, windows = usage.parse(output(stdin()), now=NOW)
    assert age == 30
    assert windows == {"five_hour": {"pct": 12, "resets_at": NOW + 3600},
                       "seven_day": {"pct": 51, "resets_at": NOW + 86400}}


def test_nothing_else_of_the_file_leaks_into_the_wire():
    found = usage.parse(output(stdin()), now=NOW)
    sent = json.dumps(usage.pick([found], now=NOW))
    for secret in ("secret", "/home/me", "cost", "claude-opus", "extra", "session"):
        assert secret not in sent
    assert set(json.loads(sent)) == {"five_hour", "seven_day", "at"}
    assert set(json.loads(sent)["five_hour"]) == {"pct", "resets_at"}


def test_a_missing_file_is_nothing():
    assert usage.parse(f"{usage.MARK}\n{NOW}\n", now=NOW) is None
    assert usage.parse("", now=NOW) is None  # ssh failed before the script ran
    assert usage.pick([None, None], now=NOW) is None


def test_malformed_files_are_nothing():
    for body in ("{not json", "[]", "null", json.dumps({"rate_limits": "x"}), json.dumps({"cwd": "/x"}),
                 json.dumps({"rate_limits": {"five_hour": {"used_percentage": "12"}}}),
                 json.dumps({"rate_limits": {"five_hour": {"used_percentage": True}}}),
                 json.dumps({"rate_limits": {"five_hour": {"used_percentage": float("nan")}}})):
        assert usage.parse(output(body), now=NOW) is None, body
    assert usage.parse(f"{usage.MARK}\nnot a clock\n{NOW}\n{stdin()}", now=NOW) is None


def test_one_window_alone_still_shows_and_the_other_is_null():
    body = json.dumps({"rate_limits": {"seven_day": {"used_percentage": 40.6, "resets_at": NOW + 10}}})
    _, windows = usage.parse(output(body), now=NOW)
    assert windows == {"five_hour": None, "seven_day": {"pct": 41, "resets_at": NOW + 10}}


def test_a_reset_time_already_past_is_dropped_and_percent_is_clamped():
    _, windows = usage.parse(output(stdin(five=130, five_at=NOW - 5, week=-3, week_at="soon")), now=NOW)
    assert windows == {"five_hour": {"pct": 100, "resets_at": None}, "seven_day": {"pct": 0, "resets_at": None}}


def test_the_newest_file_wins_whatever_the_hosts_clocks_say():
    # box's clock runs an hour fast; its file is 10 s old. pc's file is 300 s old.
    box = usage.parse(output(stdin(five=20), host_now=NOW + 3600, mtime=NOW + 3590), now=NOW)
    pc = usage.parse(output(stdin(five=5), host_now=NOW, mtime=NOW - 300), now=NOW)
    picked = usage.pick([pc, None, box], now=NOW)
    assert picked["five_hour"]["pct"] == 20 and picked["at"] == NOW - 10


def test_a_file_from_the_future_counts_as_just_written():
    found = usage.parse(output(stdin(), host_now=NOW, mtime=NOW + 50), now=NOW)
    assert found[0] == 0


async def test_the_feed_sends_usage_on_connect_and_when_it_changes(aiohttp_client, monkeypatch):
    now = int(time.time())
    files = {"box": output(stdin(five=12, five_at=now + 3600, week_at=now + 9000), host_now=now, mtime=now - 5)}

    async def run(argv, stdin=None):
        script = argv[-1]
        if usage.MARK in script:
            host = argv[-2] if argv[0] == "ssh" else "box"
            return 0, files.get(host, f"{usage.MARK}\n0\n")
        return 0, ""

    async def list_host(host, screens=False):
        return True, []

    async def no_background(host):
        return []
    monkeypatch.setattr(prompts, "_run", run)
    monkeypatch.setattr(hosts, "list_host", list_host)
    monkeypatch.setattr(hosts, "background", no_background)
    app = web.Application()
    app["hosts"], app["poll_interval"], app["usage_interval"] = ["box", "pc"], 0.05, 0.05
    events.setup(app)
    client = await aiohttp_client(app)
    await asyncio.sleep(0.2)
    ws = await client.ws_connect("/api/events")
    pct = lambda m: (m["usage"] or {}).get("five_hour", {}).get("pct")
    await _until(ws, lambda m: pct(m) == 12)
    files["box"] = files["box"].replace('"used_percentage": 12', '"used_percentage": 77')
    await _until(ws, lambda m: pct(m) == 77)
    del files["box"]
    await _until(ws, lambda m: m["usage"] is None)
    await ws.close()


async def _until(ws, wanted):
    """The next usage message that is `wanted`; fails after 5 s."""
    async def look():
        while True:
            msg = await ws.receive_json()
            if msg["t"] == "usage" and wanted(msg):
                return msg
    return await asyncio.wait_for(look(), 5)


async def test_background_servers_are_not_asked(monkeypatch):
    asked = []

    async def run(argv, stdin=None):
        asked.append(argv)
        return 0, ""
    monkeypatch.setattr(prompts, "_run", run)
    app = {"hosts": ["box", "box#web"], "usage": {}}
    assert not await usage.poll_once(app)
    assert len(asked) == 1 and "box" in asked[0]
