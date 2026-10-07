"""Bridge one browser WebSocket to one tmux attach client in a pty."""
import asyncio
import fcntl
import json
import os
import pty
import re
import signal
import struct
import termios

from aiohttp import WSMsgType, web

from tmls import hosts

PHONE = r"Android|iPhone|Mobile"  # app.js's ME uses the same test, to mark "(you)"
HIGH_WATER = 1 << 20  # queued output that pauses the pty until the browser catches up


async def terminal(request):
    """One pty running `tmux attach` per open terminal (login and Origin: see server.require_login).
    The heartbeat notices a browser that vanished without closing (laptop asleep, Wi-Fi gone)."""
    ws = web.WebSocketResponse(heartbeat=30)
    await ws.prepare(request)
    host, name = request.query.get("host"), request.query.get("name")
    if not isinstance(host, str) or not hosts.allowed(host, request.app["hosts"]) or not name or "\0" in name:
        await ws.close(code=4404)
    else:
        viewer = "phone" if re.search(PHONE, request.headers.get("User-Agent", "")) else "web"
        await bridge(ws, request.app["attach_argv"](host, name, viewer), ptys=request.app["ptys"],
                     peaks=request.app.get("term_queued"))  # tests watch buffering
    return ws


def setup(app):
    app["attach_argv"], app["ptys"] = hosts.attach_argv, set()
    app.router.add_get("/api/term", terminal)


def set_size(fd, cols, rows):
    if 1 <= cols <= 500 and 1 <= rows <= 300:
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


async def reap(pid):
    try:
        os.kill(pid, signal.SIGHUP)
    except ProcessLookupError:
        pass
    loop = asyncio.get_running_loop()
    deadline = loop.time() + 2
    while loop.time() < deadline:
        try:
            waited, _ = os.waitpid(pid, os.WNOHANG)
        except ChildProcessError:
            return
        if waited:
            return
        await asyncio.sleep(.02)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    try:
        await asyncio.to_thread(os.waitpid, pid, 0)
    except ChildProcessError:
        pass


async def bridge(ws, argv, cols=80, rows=24, ptys=None, peaks=None):
    pid, fd = pty.fork()
    if pid == 0:
        # Whatever happens, the child never returns into the server's code.
        try:
            os.environ["TERM"] = "xterm-256color"
            os.execvp(argv[0], argv)
        finally:
            os._exit(127)
    if ptys is not None:
        ptys.add(pid)
    loop = asyncio.get_running_loop()
    os.set_blocking(fd, False)
    set_size(fd, cols, rows)
    output = asyncio.Queue()
    closed = paused = False
    queued = peak = 0  # bytes waiting for the browser
    pending = bytearray()  # typed or pasted input the pty hasn't taken yet

    def flush():
        try:
            while pending:
                del pending[:os.write(fd, pending)]
        except BlockingIOError:  # pty buffer full: finish when it drains
            loop.add_writer(fd, flush)
            return
        except OSError:
            pending.clear()
        loop.remove_writer(fd)

    def read_ready():
        nonlocal closed, paused, queued, peak
        try:
            chunk = os.read(fd, 65536)
        except BlockingIOError:  # spurious wakeup: nothing to read yet
            return
        except OSError:  # EIO: the child closed its side
            chunk = b""
        if chunk:
            output.put_nowait(chunk)
            queued += len(chunk)
            if peaks is not None and queued > peak:
                peak = queued
                peaks.append(peak)
            if queued > HIGH_WATER:  # a slow or stalled browser: let tmux wait instead of us buffering
                paused = True
                loop.remove_reader(fd)
        elif not closed:
            closed = True
            loop.remove_reader(fd)
            output.put_nowait(None)

    async def send_output():
        nonlocal paused, queued
        while True:
            chunk = await output.get()
            if chunk is None:
                await ws.send_json({"t": "exit"})
                await ws.close()
                return
            await ws.send_bytes(chunk)
            queued -= len(chunk)
            if paused and queued < HIGH_WATER // 2:
                paused = False
                loop.add_reader(fd, read_ready)

    async def read_input():
        async for msg in ws:
            if msg.type != WSMsgType.TEXT:
                continue
            try:
                data = json.loads(msg.data)
                if data.get("t") == "in" and isinstance(data.get("d"), str):
                    pending.extend(data["d"].encode())
                    flush()
                elif data.get("t") == "size":
                    set_size(fd, int(data["cols"]), int(data["rows"]))
            except (ValueError, TypeError, KeyError, AttributeError, OSError):  # junk frames are ignored
                continue

    sender = receiver = None
    try:
        loop.add_reader(fd, read_ready)
        sender = asyncio.create_task(send_output())
        receiver = asyncio.create_task(read_input())
        await asyncio.wait((sender, receiver), return_when=asyncio.FIRST_COMPLETED)
    finally:
        loop.remove_reader(fd)
        loop.remove_writer(fd)
        for task in (sender, receiver):
            if task and not task.done():
                task.cancel()
            if task:
                try:
                    await task
                except asyncio.CancelledError:
                    pass
                except Exception as error:  # the browser went away mid-send, or a bug: still clean up
                    print(f"tmls-web terminal: {error!r}")
        os.close(fd)
        await reap(pid)
        if ptys is not None:
            ptys.discard(pid)
