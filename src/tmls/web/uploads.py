"""Files sent from the app: kept for good, or cleaned out of the upload cache after a while.

Uploads land in ~/.cache/tmls/images/ of the session's host (chat.upload). Two things here:

- Keep (`POST /api/chat/keep`): copy one of them to ~/Documents/tmls/YYYY-MM/ on that host under
  its original name (a suffix instead of overwriting anything), and say where. The app does it
  before sending for attachments marked Keep, so Claude is given the kept path, and on a file
  already sent.
- Prune: delete files directly inside ~/.cache/tmls/images/ older than N days (14 by default), on
  every configured machine over the usual ssh path, every few hours. Off unless tmls-web is
  started with --prune-uploads. The command never follows a symlink, never descends, and never
  names any other directory (see prune_script and its tests).
"""
import asyncio
import re
import shlex
import sys
from datetime import datetime
from pathlib import PurePosixPath

from aiohttp import web

from tmls import hosts, image_paste

CACHE_DIR = ".cache/tmls/images"   # under $HOME on each host
KEEP_DIR = "Documents/tmls"        # under $HOME on each host; a YYYY-MM folder per month
DEFAULT_DAYS = 14
MAX_DAYS = 3650
PRUNE_FIRST = 120                  # seconds after start before the first prune
PRUNE_EVERY = 6 * 3600             # seconds between prunes
PRUNE_TIMEOUT = 60
KEEP_TIMEOUT = 60
STEM_MAX = 80
# What chat.store names a file: stamp, random part, then the original name made safe (newer ones).
_STORED = re.compile(r"\d{8}-\d{6}-[0-9a-f]{8}-(?P<orig>.+)")
_CACHED = re.compile(r"\w[\w.-]{0,240}")


def safe_name(name, fallback="file"):
    """A file name that is safe to hand around (shell, Claude, other machines) but still looks
    like the original: letters and digits of any script, `.`, `_` and `-`; anything else becomes
    `_`. No folders, no leading dot, at most STEM_MAX characters before a short extension."""
    base = re.split(r"[\\/]", str(name or ""))[-1]
    clean = re.sub(r"[^\w.-]+", "_", base, flags=re.UNICODE).strip("._-")
    clean = re.sub(r"_{2,}", "_", clean)
    if not clean:
        return fallback
    path = PurePosixPath(clean)
    suffix = path.suffix if re.fullmatch(r"\.[A-Za-z0-9]{1,10}", path.suffix) else ""
    stem = clean[: len(clean) - len(suffix)] if suffix else clean
    stem = stem[:STEM_MAX].rstrip("._-") or fallback
    return stem + suffix


def original_name(cached):
    """The original name kept inside a cached file's name, else the cached name itself."""
    found = _STORED.fullmatch(cached)
    return safe_name(found.group("orig") if found else cached)


def cached_name(path):
    """The file name of an upload path the app names, or None when it is not one of ours."""
    if not isinstance(path, str) or "\0" in path or "\n" in path:
        return None
    pure = PurePosixPath(path)
    if pure.parent.parts[-3:] != (".cache", "tmls", "images") or not _CACHED.fullmatch(pure.name):
        return None
    return pure.name


def keep_script(cached, name, month):
    """sh: copy $HOME/.cache/tmls/images/CACHED to $HOME/Documents/tmls/MONTH/NAME and print the
    path. An existing file there is never overwritten: NAME-1, NAME-2... instead, unless it already
    holds the same bytes (kept before), which is then the answer."""
    if not _CACHED.fullmatch(cached) or not re.fullmatch(r"\d{4}-\d{2}", month):
        raise ValueError("bad name")
    name = safe_name(name)
    suffix = PurePosixPath(name).suffix
    stem = name[: len(name) - len(suffix)] if suffix else name
    q = shlex.quote
    return (
        f'src="$HOME/{CACHE_DIR}/"{q(cached)}; '
        '[ -f "$src" ] && [ ! -L "$src" ] || { echo "that upload is not on this host any more" >&2; exit 3; }; '
        f'd="$HOME/{KEEP_DIR}/"{q(month)}; mkdir -p "$d" || exit 1; '
        f'stem={q(stem)}; ext={q(suffix)}; t="$stem$ext"; i=1; '
        'while [ -e "$d/$t" ] || [ -L "$d/$t" ]; do '
        'if [ -f "$d/$t" ] && [ ! -L "$d/$t" ] && cmp -s "$src" "$d/$t"; then echo "$d/$t"; exit 0; fi; '
        't="$stem-$i$ext"; i=$((i+1)); [ "$i" -le 999 ] || { echo "too many files named $stem$ext" >&2; exit 4; }; '
        'done; '
        '(set -C; cat -- "$src" > "$d/$t") && echo "$d/$t"'
    )


def prune_script(days):
    """sh: delete regular files directly inside $HOME/.cache/tmls/images that were last changed
    more than DAYS days ago, and print each one deleted.

    Limits, each on purpose:
    - nothing happens when $HOME is unset or /, or when ~/.cache, ~/.cache/tmls or the folder
      itself is a symlink (it could point anywhere);
    - find without -L/-H/-follow never follows a link; `-type f` skips links, folders and
      everything else; -mindepth 1 -maxdepth 1: only the entries directly inside, never the
      folder itself, never deeper; -xdev for good measure;
    - -delete unlinks the entry itself (a link would be removed, never its target).
    """
    if isinstance(days, bool) or not isinstance(days, int) or not 1 <= days <= MAX_DAYS:
        raise ValueError(f"days must be a whole number from 1 to {MAX_DAYS}")
    return (
        '[ -n "$HOME" ] && [ "$HOME" != / ] || exit 3; '
        f'c="$HOME/.cache"; d="$HOME/{CACHE_DIR}"; '
        'for p in "$c" "$c/tmls" "$d"; do '
        'if [ -L "$p" ]; then echo "tmls: $p is a symlink: not pruning" >&2; exit 3; fi; done; '
        '[ -d "$d" ] || exit 0; '
        f'find "$d" -mindepth 1 -maxdepth 1 -xdev -type f -mmin +{days * 1440} -print -delete'
    )


IMAGE_EXT = {"image/png": ".png", "image/jpeg": ".jpg", "image/webp": ".webp", "image/gif": ".gif"}


async def keep(request):
    """`{"key": K, "path": P, "name": N?}` -> `{"ok": true, "path": KEPT}`. P is an upload path
    (in ~/.cache/tmls/images/ of the session's host); N the name to keep it under, else the
    original name inside the cached one.

    Or `{"key": K, "blob": ID, "name": N?}`: a picture of the chat (/api/chat/blob), as Claude's
    record holds it (a photo sent is in the record, not its path). Its bytes go to the host's
    upload cache like an upload, then are kept from there; N defaults to "image"."""
    from tmls.web import chat  # chat imports this module for the names
    try:
        data = await request.json()
    except ValueError:
        data = None
    if not isinstance(data, dict):
        return chat._fail(400, "invalid request")
    name, blob_id = data.get("name"), data.get("blob")
    if name is not None and (not isinstance(name, str) or "\0" in name):
        return chat._fail(400, "invalid request")
    if blob_id is not None:
        if not isinstance(blob_id, str) or not blob_id or "path" in data:
            return chat._fail(400, "invalid request")
        cached = None
    else:
        cached = cached_name(data.get("path"))
        if cached is None:
            return chat._fail(400, "invalid request")
    found = await chat._row(request.app, data.get("key"))
    if isinstance(found, web.Response):
        return found
    if blob_id is not None:
        blobs = request.app["chat_blobs"].get(data["key"])
        picture = blobs.get(blob_id) if blobs else None
        if picture is None:
            return chat._fail(404, "that picture is not loaded any more: open the chat again")
        mime, body = picture
        name = safe_name(name or "image")
        if not PurePosixPath(name).suffix and mime in IMAGE_EXT:
            name += IMAGE_EXT[mime]
        try:
            stored = await chat.store(found[0], body, name)
        except image_paste.ImageError as error:
            return chat._fail(502, str(error) or "host unreachable")
        cached = PurePosixPath(stored).name
    script = keep_script(cached, name or original_name(cached), f"{datetime.now():%Y-%m}")
    try:
        out = await image_paste._run(hosts.run_argv(found[0], script), timeout=KEEP_TIMEOUT)
    except image_paste.ImageError as error:
        return chat._fail(502, str(error) or "host unreachable")
    lines = out.decode(errors="replace").splitlines()
    if not lines or not lines[-1].startswith("/"):
        return chat._fail(502, "the host did not say where it put the file")
    return web.json_response({"ok": True, "path": lines[-1]})


def machines(host_list):
    """Each machine once: a background tmux server ("box#web") shares its machine's files."""
    return list(dict.fromkeys(hosts.split(h)[0] for h in host_list if h != hosts.KITTY))


async def prune_host(machine, days):
    """Prune one machine; returns how many files went, or None when it could not be done."""
    try:
        out = await image_paste._run(hosts.run_argv(machine, prune_script(days)), timeout=PRUNE_TIMEOUT)
    except image_paste.ImageError as error:
        print(f"tmls-web prune {machine}: {error}", file=sys.stderr)
        return None
    gone = [line for line in out.decode(errors="replace").splitlines() if line.startswith("/")]
    if gone:
        print(f"tmls-web prune {machine}: {len(gone)} upload(s) older than {days} days deleted", file=sys.stderr)
    return len(gone)


async def prune_loop(app):
    await asyncio.sleep(PRUNE_FIRST)
    while True:
        days = app["prune_days"]
        await asyncio.gather(*(prune_host(m, days) for m in machines(app["hosts"])))
        await asyncio.sleep(PRUNE_EVERY)


async def _start(app):
    if app["prune_days"] is not None:
        app["pruner"] = asyncio.create_task(prune_loop(app))


async def _stop(app):
    task = app.get("pruner")
    if task is not None:
        task.cancel()
        await asyncio.gather(task, return_exceptions=True)


def setup(app):
    # Days after which unkept uploads go, or None: pruning is off (the default; --prune-uploads).
    app["prune_days"] = None
    app.router.add_post("/api/chat/keep", keep)
    app.on_startup.append(_start)
    app.on_cleanup.append(_stop)
