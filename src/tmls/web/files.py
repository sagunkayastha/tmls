"""Pictures a reply names by path, read from the session's host: `GET /api/chat/file?key=K&path=P`.

Claude often answers with "the plot is at /data/x/fig.png"; the app shows that file under the
paragraph. The file is read over the usual ssh path (hosts.run_argv) and only when all of this holds:

- P is absolute, or starts with `~/` (the host's $HOME), and has no NUL or newline in it;
- P ends in .png, .jpg, .jpeg, .gif or .webp, and so does the file it finally points to (a link
  named x.png that points at notes.txt is refused);
- it is a regular file (never a folder, so nothing is ever listed; no devices or pipes);
- it is at most MAX_BYTES long;
- its first bytes really are a PNG, JPEG, GIF or WebP; the Content-Type comes from those bytes.

Plain answers: 400 for a request that can never be served (relative path, other extension),
404 for no such picture (missing, a folder, not an image inside), 413 for too big, 502 when the
host does not answer. Bytes are kept here by host, path, size and mtime: a picture seen before
costs one small round trip (the host says the file did not change) and is not sent again.
"""
import re
import shlex
from collections import OrderedDict
from hashlib import sha256
from pathlib import PurePosixPath

from aiohttp import web

from tmls import hosts, image_paste

MAX_BYTES = 15 << 20
CACHE_BYTES = 96 << 20
TIMEOUT = 30
EXTENSIONS = (".png", ".jpg", ".jpeg", ".gif", ".webp")
_STAMP = re.compile(r"\d+ \d+")


def sniff(data):
    """The image type of these bytes by their magic number, else None."""
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        return "image/png"
    if data.startswith(b"\xff\xd8\xff"):
        return "image/jpeg"
    if data.startswith((b"GIF87a", b"GIF89a")):
        return "image/gif"
    if len(data) >= 12 and data[:4] == b"RIFF" and data[8:12] == b"WEBP":
        return "image/webp"
    return None


def is_image_name(path):
    return PurePosixPath(path).suffix.lower() in EXTENSIONS


def check_path(path):
    """The shell word for P on its host ("$HOME"/... for ~/...), or None when P is refused outright."""
    if not isinstance(path, str) or not path or "\0" in path or "\n" in path or "\r" in path:
        return None
    if not is_image_name(path):
        return None
    if path.startswith("~/"):
        rest = path[2:].lstrip("/")
        return '"$HOME"/' + shlex.quote(rest) if rest else None
    if path.startswith("/"):
        return shlex.quote(path)
    return None


def read_script(word, known=None, limit=MAX_BYTES):
    """sh: say what P is, then maybe its bytes. Lines first, then raw bytes:

    `missing`                       not a readable regular file (or link to one)
    `ok SIZE MTIME` + `REAL`        then one of `same` (KNOWN matched: nothing follows),
                                    `big` (over LIMIT), or `data` and the file's first LIMIT+1 bytes.
    REAL is where links finally lead, so its name can be checked too. `stat -L` and `[ -f ]`
    follow links; GNU stat first, BSD stat (macOS) as the fallback."""
    known_word = shlex.quote(known or "-")
    return (
        f'p={word}; '
        '[ -f "$p" ] && [ -r "$p" ] || { echo missing; exit 0; }; '
        's=$(stat -L -c "%s %Y" -- "$p" 2>/dev/null || stat -L -f "%z %m" -- "$p" 2>/dev/null) || { echo missing; exit 0; }; '
        'r=$(readlink -f -- "$p" 2>/dev/null || realpath -- "$p" 2>/dev/null || printf %s "$p"); '
        'echo "ok $s"; printf "%s\\n" "$r"; '
        f'[ "$s" = {known_word} ] && {{ echo same; exit 0; }}; '
        f'set -- $s; [ "$1" -le {int(limit)} ] || {{ echo big; exit 0; }}; '
        f'echo data; exec head -c {int(limit) + 1} -- "$p"'
    )


def parse(out):
    """(status, stamp, real, body) from read_script's output; status is missing/same/big/data."""
    if out.startswith(b"missing"):
        return "missing", None, None, b""
    head, real, rest = (out.split(b"\n", 2) + [b"", b""])[:3]
    stamp = head[3:].decode(errors="replace").strip() if head.startswith(b"ok ") else ""
    if not _STAMP.fullmatch(stamp):
        raise ValueError("unexpected answer")
    status, _, body = rest.partition(b"\n")
    status = status.decode(errors="replace")
    if status not in ("same", "big", "data"):
        raise ValueError("unexpected answer")
    return status, stamp, real.decode(errors="replace"), body


class Cache:
    """Picture bytes by (host, path), with the stamp ("SIZE MTIME") they were read at. Least recently used go first."""

    def __init__(self, limit=CACHE_BYTES):
        self.limit, self.size, self.items = limit, 0, OrderedDict()

    def get(self, key):
        found = self.items.get(key)
        if found is not None:
            self.items.move_to_end(key)
        return found

    def put(self, key, stamp, mime, data):
        self.drop(key)
        if len(data) > self.limit:
            return
        self.items[key] = (stamp, mime, data)
        self.size += len(data)
        while self.size > self.limit:
            _, (_, _, dropped) = self.items.popitem(last=False)
            self.size -= len(dropped)

    def drop(self, key):
        old = self.items.pop(key, None)
        if old is not None:
            self.size -= len(old[2])


def _fail(status, error):
    return web.json_response({"ok": False, "error": error}, status=status)


async def picture(request):
    from tmls.web import chat  # chat imports uploads, which this sits beside
    path, key = request.query.get("path"), request.query.get("key")
    word = check_path(path)
    if word is None:
        return _fail(400, "only an absolute (or ~/) path to a .png, .jpg, .jpeg, .gif or .webp file")
    found = await chat._row(request.app, key)
    if isinstance(found, web.Response):
        return found
    host = found[0]
    cache, cache_key = request.app["file_cache"], (host, path)
    cached = cache.get(cache_key)
    try:
        out = await image_paste._run(hosts.run_argv(host, read_script(word, cached and cached[0])), timeout=TIMEOUT)
        status, stamp, real, body = parse(out)
    except image_paste.ImageError as error:
        return _fail(502, str(error) or "host unreachable")
    except ValueError:
        return _fail(502, "the host gave an answer that makes no sense")
    if status == "missing":
        cache.drop(cache_key)
        return _fail(404, "not found")
    if not is_image_name(real):
        cache.drop(cache_key)
        return _fail(404, "not a picture")
    if status == "big" or len(body) > MAX_BYTES:
        cache.drop(cache_key)
        return _fail(413, f"too large ({MAX_BYTES >> 20} MB at most)")
    if status == "same" and cached is not None:
        mime, body = cached[1], cached[2]
    else:
        mime = sniff(body)
        if mime is None:
            cache.drop(cache_key)
            return _fail(404, "not a picture")
        cache.put(cache_key, stamp, mime, body)
    etag = '"' + sha256(f"{host}\0{path}\0{stamp}".encode()).hexdigest()[:24] + '"'
    headers = {"ETag": etag, "Cache-Control": "private, no-cache", "X-Content-Type-Options": "nosniff"}
    if request.headers.get("If-None-Match") == etag:
        return web.Response(status=304, headers=headers)
    return web.Response(body=body, content_type=mime, headers=headers)


def setup(app):
    app["file_cache"] = Cache()
    app.router.add_get("/api/chat/file", picture)
