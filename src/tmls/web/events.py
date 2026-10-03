"""One poll loop for every browser tab: list the hosts, build rows, push what changed."""
import asyncio
import shlex

from aiohttp import WSMsgType, web

from tmls import approve, hosts, prompts
from tmls.web import rows

# Parallel ssh commands per host. With ControlMaster they are channels on one connection, and
# sshd's MaxSessions (10 by default) refuses the rest with "administratively prohibited".
SLOTS = 6


async def _capped(app, host, coro):
    slot = app["slots"].setdefault(host, asyncio.Semaphore(SLOTS))
    async with slot:
        return await coro


async def _screen(host, name):
    code, out = await prompts._run(prompts._run_argv(host, f"tmux capture-pane -p -t {shlex.quote(f'={name}:')}"))
    return out if code == 0 else None


async def _list(host):
    try:
        return await hosts.list_host(host)
    except Exception:  # one bad host must not stop the loop
        return False, []


async def poll_once(app):
    state, names = app["state"], app["hosts"]
    found = [(h, *r) for h, r in zip(names, await asyncio.gather(*(_list(h) for h in names)))]
    lines, extras = app["lines"], {}
    screens, prompts_up = {}, {}  # fetched in parallel: one slow host mustn't hold up the rest
    for host, online, sessions in found:
        for s in sessions:
            app["now"][host] = s.now
            k = rows.key(host, s.name)
            if not s.claude and lines.get(k, (None,))[0] != s.activity:  # plain tmux: only on new output
                screens[k] = (s.activity, _capped(app, host, _screen(host, s.name)))
            if s.claude == "waiting":
                prompts_up[k] = _capped(app, host, approve.current(host, s.name, pane=s.pane))
    for (k, (activity, _)), text in zip(screens.items(), await asyncio.gather(*(c for _, c in screens.values()))):
        lines[k] = (activity, text)
    shown = dict(zip(prompts_up, await asyncio.gather(*prompts_up.values())))
    for host, online, sessions in found:
        for s in sessions:
            k = rows.key(host, s.name)
            extras[k] = {"line": lines.get(k, (None, None))[1], "shown": shown.get(k)}
    old_rows, old_marks = dict(state.rows), dict(state.marks)
    new = rows.build(state, found, extras)
    changed = rows.diff(old_rows, new)
    if changed["set"] or changed["gone"]:
        await broadcast(app, {"t": "rows", **changed})
    items = rows.alerts(old_marks, new)
    if items:
        await broadcast(app, {"t": "alerts", "items": items})


async def broadcast(app, message):
    for ws in list(app["sockets"]):
        try:
            await ws.send_json(message)
        except (ConnectionError, RuntimeError):
            app["sockets"].discard(ws)


async def poll(app):
    while True:
        try:
            await poll_once(app)
        except Exception as error:  # keep polling; the next tick may work
            print(f"tmls-web poll: {error!r}")
        await asyncio.sleep(app.get("poll_interval", 2))


async def handle_events(request):
    app = request.app
    ws = web.WebSocketResponse(heartbeat=30)
    await ws.prepare(request)
    snapshot = list(app["state"].rows.values())
    app["sockets"].add(ws)  # before the await below, so no poll tick can slip past this client
    try:
        await ws.send_json({"t": "rows", "full": True, "set": snapshot, "gone": []})
        async for msg in ws:
            if msg.type == WSMsgType.ERROR:
                break
    finally:
        app["sockets"].discard(ws)
    return ws


async def handle_seen(request):
    app = request.app
    try:
        data = await request.json()
        k = data["key"] if isinstance(data, dict) and isinstance(data.get("key"), str) else None
    except ValueError:
        k = None
    if k is None:
        return web.json_response({"ok": False, "error": "invalid request"}, status=400)
    host = k.split("/", 1)[0]
    if host not in app["now"]:
        return web.json_response({"ok": False, "error": "unknown session"}, status=404)
    app["state"].seen[k] = app["now"][host]
    return web.json_response({"ok": True})


async def _start(app):
    app["poller"] = asyncio.create_task(poll(app))


async def _stop(app):
    app["poller"].cancel()
    for ws in list(app["sockets"]):
        await ws.close()


def setup(app):
    app["state"], app["sockets"], app["lines"], app["now"], app["slots"] = rows.State(), set(), {}, {}, {}
    app.router.add_get("/api/events", handle_events)
    app.router.add_post("/api/seen", handle_seen)
    app.on_startup.append(_start)
    app.on_cleanup.append(_stop)
