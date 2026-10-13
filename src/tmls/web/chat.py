"""A Claude session as a chat: follow its record on the host, and send it text, keys and files.

One `tail -F` child per open chat, run on the session's host like every other host command
(hosts.run_argv). Which record to follow comes from the poll loop (events.py: app["claudes"]),
so an open chat costs its host nothing beyond that one child.
"""
import asyncio
import re
import secrets
import shlex
import sys
import traceback
from collections import OrderedDict
from datetime import datetime
from pathlib import PurePosixPath

from aiohttp import WSMsgType, web

from tmls import approve, hosts, image_paste, prompts
from tmls.web import record

WINDOW = 16 << 20       # bytes of the record's end read for the history: records grow to 100 MB
LINE_MAX = 32 << 20     # one line holds a whole base64 screenshot
BLOB_BYTES = 64 << 20   # images kept per chat; the oldest go first
UPLOAD_MAX = 32 << 20
KEYS = ("Escape", "Enter")
FIRST_POLL = 10         # seconds a request waits for the first listing after a server start
HEAD_TIMEOUT = 10       # seconds for the host to say how long the record is
QUIET = 2               # seconds without a line that end the history early (the record shrank)
WATCH = 1               # seconds between looks for a new record or a gone session
RETRY = 15              # seconds between looks for a record Claude has not started yet
QUICK_LOOKS = 20        # the first looks come every 2 s: a new session's first message shows at once
MISSING, LOST, CHANGED = "record not found", "host unreachable", "changed"
_ID = re.compile(r"[A-Za-z0-9-]+")
SHELLS = ("bash", "zsh", "sh", "fish", "dash", "ksh")
UNSAFE = re.compile("[\x00-\x08\x0b-\x1f\x7f\x9b]")  # C0 but \n and \t; ESC [ 201 ~ would end the paste
MARK = "\n@@tmls-command@@\n"


class Blobs:
    """The images of one chat's record, by id, for /api/chat/blob. Shared by the chat's open sockets."""

    def __init__(self, limit=BLOB_BYTES):
        self.limit, self.size, self.users, self.items = limit, 0, 0, OrderedDict()

    def put(self, blob_id, mime, data):
        if len(data) > self.limit:
            return
        old = self.items.pop(blob_id, None)  # a reopened record brings the same ones again
        self.size += len(data) - (len(old[1]) if old else 0)
        self.items[blob_id] = (mime, data)
        while self.size > self.limit:
            _, (_, dropped) = self.items.popitem(last=False)
            self.size -= len(dropped)

    def get(self, blob_id):
        return self.items.get(blob_id)


def follow_argv(host, session_id):
    """argv printing the record's size, then its last WINDOW bytes, then every new line as it is
    written; or "none" when Claude has not written one. The size tells where the history ends."""
    name = shlex.quote(f"{session_id}.jsonl")
    script = (f'for t in "$HOME"/.claude/projects/*/{name}; do [ -f "$t" ] || continue; '
              f's=$(wc -c < "$t"); echo "$s"; '
              f'exec tail -c +$((s > {WINDOW} ? s - {WINDOW} + 1 : 1)) -F "$t"; done; echo none')
    return hosts.run_argv(host, script)


def _fail(status, error):
    return web.json_response({"ok": False, "error": error}, status=status)


def _split(app, key):
    """(host, name) from a key, or None. Only configured hosts, as for approve: the host goes to ssh."""
    if not isinstance(key, str):
        return None
    host, _, name = key.partition("/")
    if not name or "\0" in name or not hosts.allowed(host, app["hosts"]):
        return None
    return host, name


async def _polled(app):
    """Right after a start nothing is listed yet: wait for the first listing rather than say 404."""
    try:
        await asyncio.wait_for(app["polled"].wait(), FIRST_POLL)
    except asyncio.TimeoutError:
        pass


async def _row(app, key, claude=False):
    """(host, name, row) of a listed session, or the error response. claude: it must run one."""
    target = _split(app, key)
    if target is None:
        return _fail(400, "invalid request")
    await _polled(app)
    row = app["state"].rows.get(key)
    if row is None:
        return _fail(404, "unknown session")
    if not row["online"]:
        return _fail(502, "host unreachable")
    if claude and not row["claude"]:
        return _fail(409, f"no Claude in {target[1]}")
    return (*target, row)


def _pane(row):
    """Claude's own pane ("%7") from the row, else None: the session's active pane."""
    pane = row.get("pane")
    return pane if isinstance(pane, str) and re.fullmatch(r"%[0-9]+", pane) else None


async def _body(request):
    try:
        data = await request.json()
    except ValueError:
        return {}
    return data if isinstance(data, dict) else {}


def _sent(error):
    if error:  # the host said no (session or pane gone), or said nothing at all
        return _fail(502 if error == "couldn't reach the host" else 409, error)
    return web.json_response({"ok": True})


async def _guard(request, data, found, escape=False):
    """None when Claude's pane takes input, else the 409: the session moved to another
    conversation, a permission dialog is up (escape: Escape is still fine), or the pane is a shell.
    One host round trip: the screen and the pane's foreground command."""
    host, name, row = found
    session, claude = data.get("session"), request.app["claudes"].get(data.get("key"))
    if isinstance(session, str) and claude is not None and claude.session_id != session:
        return _fail(409, f"{name} is on another conversation now")
    target = shlex.quote(_pane(row) or f"={name}:")
    script = (f"tmux capture-pane -p -t {target} && printf '{MARK}' && "
              f"tmux display-message -p -t {target} '#{{pane_current_command}}'")
    code, out = await prompts._run(prompts._run_argv(host, script))
    if code is None:
        return _fail(502, out)
    if code != 0:
        return _fail(409, out.strip() or f"couldn't read {name}")
    screen, _, command = out.partition(MARK)
    if command.strip() in SHELLS:
        return _fail(409, f"Claude is not running in {name} any more")
    if not escape and approve.request(screen) is not None:
        return _fail(409, f"{name} is asking for permission: answer that first")


async def send(request):
    """Paste text into Claude's pane and press Enter. prompts.send hands the text to tmux on
    stdin (load-buffer, bracketed paste), so no shell ever reads it."""
    data = await _body(request)
    text = data.get("text")
    if not isinstance(text, str):
        return _fail(400, "invalid request")
    text = UNSAFE.sub("", text.replace("\r\n", "\n").replace("\r", "\n"))
    if not text.strip():
        return _fail(400, "invalid request")
    found = await _row(request.app, data.get("key"), claude=True)
    if isinstance(found, web.Response):
        return found
    host, name, row = found
    refused = await _guard(request, data, found)
    if refused is not None:
        return refused
    request.app["chat_sent"][data["key"]] = asyncio.get_running_loop().time()  # an empty chat looks again soon
    return _sent(await prompts.send(host, name, text, pane=_pane(row)))


async def press(request):
    """One key for Claude's pane: Escape interrupts, the rest answer its menus. Only KEYS."""
    data = await _body(request)
    key_name = data.get("name")
    if not isinstance(key_name, str) or key_name not in KEYS:
        return _fail(400, "invalid request")
    found = await _row(request.app, data.get("key"), claude=True)
    if isinstance(found, web.Response):
        return found
    host, name, row = found
    refused = await _guard(request, data, found, escape=key_name == "Escape")
    if refused is not None:
        return refused
    target = shlex.quote(_pane(row) or f"={name}:")
    code, out = await prompts._run(prompts._run_argv(host, f"tmux send-keys -t {target} {key_name}"))
    return _sent(None if code == 0 else out.strip() or f"couldn't send to {name}")


async def store(host, data, name):
    """Save bytes under ~/.cache/tmls/images/ on the host (stdin of the remote command, like
    image_paste.store) and return the absolute path there. The name is ours; only the extension
    of the given one is kept, and only a plain one."""
    suffix = PurePosixPath(name).suffix.lower()
    suffix = suffix if re.fullmatch(r"\.[a-z0-9]{1,10}", suffix) else ""
    filename = f"{datetime.now():%Y%m%d-%H%M%S}-{secrets.token_hex(4)}{suffix}"
    script = f'd="$HOME/.cache/tmls/images"; mkdir -p "$d" && cat > "$d/{filename}" && echo "$d/{filename}"'
    out = await image_paste._run(hosts.run_argv(host, script), stdin=data, timeout=60)
    lines = out.decode(errors="replace").split()
    if not lines or not lines[-1].startswith("/"):
        raise image_paste.ImageError("the host did not say where it put the file")
    return lines[-1]


async def upload(request):
    """A file for Claude to read: stored on the session's host, the app then names the path in its
    message. Too large a body is still read to its end (and dropped), so the client gets the 413
    instead of a broken connection."""
    found = await _row(request.app, request.query.get("key"))
    name = request.query.get("name")
    if not isinstance(found, web.Response) and (not name or "\0" in name):
        found = _fail(400, "invalid request")
    data, large = bytearray(), (request.content_length or 0) > UPLOAD_MAX
    async for chunk in request.content.iter_chunked(1 << 16):
        large = large or len(data) + len(chunk) > UPLOAD_MAX
        if not large and not isinstance(found, web.Response):
            data += chunk
    if isinstance(found, web.Response):
        return found
    if large:
        return _fail(413, "file too large (32 MiB at most)")
    if not data:
        return _fail(400, "empty file")
    try:
        path = await store(found[0], bytes(data), name)
    except image_paste.ImageError as error:
        return _fail(502, str(error) or "host unreachable")
    return web.json_response({"ok": True, "path": path})


async def blob(request):
    key, blob_id = request.query.get("key"), request.query.get("id")
    if _split(request.app, key) is None or not blob_id:
        return _fail(400, "invalid request")
    blobs = request.app["chat_blobs"].get(key)
    found = blobs.get(blob_id) if blobs else None
    if found is None:
        return _fail(404, "unknown image")
    # nosniff: these bytes come out of a record, and are served from this site's origin
    return web.Response(body=found[1], content_type=found[0],
                        headers={"X-Content-Type-Options": "nosniff", "Cache-Control": "private, max-age=3600"})


async def _line(out):
    """The next line, None at the end. A line beyond LINE_MAX is dropped: what follows it does not parse."""
    try:
        return await out.readline() or None
    except ValueError:
        return b"\n"


def _take(line, blobs):
    try:
        events, images = record.read(line)
    except Exception:  # one bad line is skipped, not fatal
        traceback.print_exc(file=sys.stderr)
        return []
    for image in images:
        blobs.put(*image)
    return events


async def _pump(ws, key, claude, proc, blobs):
    """The child's output as messages: `open`, the history as one `items`, then every new line.
    Returns why it stopped: MISSING (no record yet) or LOST (the child ended)."""
    out, head = proc.stdout, b""
    try:
        for _ in range(50):  # a login script may print first
            raw = await asyncio.wait_for(out.readline(), HEAD_TIMEOUT)
            if not raw:  # the end of the output: a blank line is only a login script's
                head = b""
                break
            head = raw.strip()
            if head.isdigit() or head == b"none":
                break
    except (asyncio.TimeoutError, ValueError):
        return LOST
    if head == b"none":
        return MISSING
    if not head.isdigit():
        return LOST
    await ws.send_json({"t": "open", "key": key, "session": claude.session_id, "title": claude.title})
    size, events, count = int(head), [], 0
    left, partial = min(size, WINDOW), size > WINDOW  # partial: the tail starts inside a line
    while left > 0:
        try:
            line = await asyncio.wait_for(_line(out), QUIET)
        except asyncio.TimeoutError:
            break
        if line is None:
            return LOST
        left -= len(line)
        if partial:
            partial = False
            continue
        events += _take(line, blobs)
        count += 1
        if count % 200 == 0:  # megabytes are already buffered: let the other sockets run
            await asyncio.sleep(0)
    items = record.fold(events)
    known = {item["id"] for item in items if item["kind"] == "tool"}  # tool items the client has
    del events
    await ws.send_json({"t": "items", "reset": True, "items": items})
    while True:
        line = await _line(out)
        if line is None:
            return LOST
        events = _take(line, blobs)
        items = [e for e in events if "kind" in e]
        if items:
            known.update(item["id"] for item in items if item["kind"] == "tool")
            await ws.send_json({"t": "items", "reset": False, "items": items})
        for patch in events:
            if "kind" not in patch and patch["id"] in known:
                await ws.send_json(patch)


async def _watch(app, key, session_id):
    """Returns CHANGED when the session's Claude writes another record (/clear, resume, a new
    Claude), or why the chat is over. A Claude that exited leaves the chat open on what it wrote."""
    while True:
        await asyncio.sleep(WATCH)
        claude = app["claudes"].get(key)
        if claude is not None and claude.session_id != session_id and _ID.fullmatch(claude.session_id or ""):
            return CHANGED
        if key not in app["state"].rows:
            return "session ended"


async def _end(*tasks):
    for task in tasks:
        task.cancel()
    await asyncio.gather(*tasks, return_exceptions=True)


async def _serve(app, ws, key):
    """Follow the session's record until the chat is over; returns the reason for the `none`."""
    host, blobs = key.split("/", 1)[0], app["chat_blobs"][key]
    await _polled(app)
    row = app["state"].rows.get(key)
    if row is None:
        return "no such session"
    if not row["online"]:
        return LOST
    empty = None  # the session id an empty chat was shown for: Claude writes no record before the first message
    looks = 0
    while True:
        claude = app["claudes"].get(key)
        if claude is None or not _ID.fullmatch(claude.session_id or ""):
            return "no Claude in this session"
        try:
            proc = await asyncio.create_subprocess_exec(
                *app["chat_argv"](host, claude.session_id), stdin=asyncio.subprocess.DEVNULL,
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL, limit=LINE_MAX)
        except OSError:
            return LOST
        app["chat_children"].add(proc.pid)
        pump = asyncio.create_task(_pump(ws, key, claude, proc, blobs))
        watch = asyncio.create_task(_watch(app, key, claude.session_id))
        try:
            await asyncio.wait((pump, watch), return_when=asyncio.FIRST_COMPLETED)
            ended = (watch if watch.done() else pump).result()
        finally:
            await _end(pump, watch)
            try:
                proc.kill()
            except ProcessLookupError:
                pass
            await proc.wait()
            app["chat_children"].discard(proc.pid)
        if ended == MISSING:
            if empty != claude.session_id:
                empty = claude.session_id
                await ws.send_json({"t": "open", "key": key, "session": claude.session_id, "title": claude.title})
                await ws.send_json({"t": "items", "reset": True, "items": []})
            looks += 1
            typed = asyncio.get_running_loop().time() - app["chat_sent"].get(key, float("-inf")) < 60
            await asyncio.sleep(min(RETRY, 2) if typed or looks <= QUICK_LOOKS else RETRY)
        elif ended != CHANGED:
            return ended


async def _stream(app, ws, key):
    try:
        reason = await _serve(app, ws, key)
        await ws.send_json({"t": "none", "reason": reason})
        await ws.close()
    except (ConnectionError, RuntimeError):  # the app went away mid-send
        pass
    except Exception:  # never leave the socket open and silent
        traceback.print_exc(file=sys.stderr)
        try:
            await ws.send_json({"t": "none", "reason": "reader failed"})
            await ws.close()
        except (ConnectionError, RuntimeError):
            pass


async def chat(request):
    """The chat stream (login and Origin: see server.require_login). Server to client only: what
    the app sends is read just to notice that it left, which ends the child."""
    ws = web.WebSocketResponse(heartbeat=30)
    await ws.prepare(request)
    app, key = request.app, request.query.get("key")
    if _split(app, key) is None:
        await ws.close(code=4404)
        return ws
    blobs = app["chat_blobs"].setdefault(key, Blobs())
    blobs.users += 1
    app["chat_sockets"].add(ws)
    stream = asyncio.create_task(_stream(app, ws, key))
    try:
        async for msg in ws:
            if msg.type == WSMsgType.ERROR:
                break
    finally:
        # nothing awaited before the books are straight: aiohttp may cancel a handler whose client left
        stream.cancel()
        app["chat_sockets"].discard(ws)
        blobs.users -= 1
        if not blobs.users and app["chat_blobs"].get(key) is blobs:
            del app["chat_blobs"][key]
        await asyncio.gather(stream, return_exceptions=True)
    return ws


async def _stop(app):
    for ws in list(app["chat_sockets"]):  # each handler then kills its child
        await ws.close()


def setup(app):
    app["chat_argv"], app["chat_blobs"], app["chat_children"] = follow_argv, {}, set()
    app["chat_sockets"], app["chat_sent"] = set(), {}
    app.on_shutdown.append(_stop)
    app.router.add_get("/api/chat", chat)
    app.router.add_get("/api/chat/blob", blob)
    app.router.add_post("/api/chat/send", send)
    app.router.add_post("/api/chat/key", press)
    app.router.add_post("/api/chat/upload", upload)
