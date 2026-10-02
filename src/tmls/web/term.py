"""Bridge one browser WebSocket to one tmux attach client in a pty."""
import asyncio
import errno
import fcntl
import json
import os
import pty
import signal
import struct
import termios

from aiohttp import WSMsgType


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


async def bridge(ws, argv, cols=80, rows=24, ptys=None):
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        try:
            os.execvp(argv[0], argv)
        except OSError:
            os._exit(127)
    if ptys is not None:
        ptys.add(pid)
    loop = asyncio.get_running_loop()
    os.set_blocking(fd, False)
    set_size(fd, cols, rows)
    output = asyncio.Queue()
    closed = False
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
        nonlocal closed
        try:
            chunk = os.read(fd, 65536)
        except OSError as error:
            if error.errno not in (errno.EIO, errno.EBADF):
                chunk = b""
            else:
                chunk = b""
        if chunk:
            output.put_nowait(chunk)
        elif not closed:
            closed = True
            loop.remove_reader(fd)
            output.put_nowait(None)

    async def send_output():
        while True:
            chunk = await output.get()
            if chunk is None:
                await ws.send_json({"t": "exit"})
                await ws.close()
                return
            await ws.send_bytes(chunk)

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
            except (ValueError, TypeError, KeyError, BlockingIOError, OSError):
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
                try:
                    await task
                except asyncio.CancelledError:
                    pass
        os.close(fd)
        await reap(pid)
        if ptys is not None:
            ptys.discard(pid)
