"""tmls in a browser: one big terminal, status rows, approve, sketch."""
import argparse
import asyncio
import hashlib
import ipaddress
import re
import time
from pathlib import Path
from urllib.parse import urlparse

from aiohttp import web

from tmls import approve, create, hosts, tmux_ops
from tmls.web import auth, events, term

STATIC = Path(__file__).parent / "static"
PUBLIC = ("/healthz", "/login")
FAIL_DELAY = 1         # seconds after a wrong password
MAX_FAILS = 5          # wrong passwords from one address before a lockout
LOCKOUT = 300          # seconds


def logged_in(request):
    creds = auth.load(request.app["auth_file"])
    if not creds or not creds.get("secret"):
        return False
    return bool(auth.verify_cookie(creds["secret"], request.cookies.get(auth.COOKIE, "")))


async def _refuse_ws(request, code):
    ws = web.WebSocketResponse()
    await ws.prepare(request)
    await ws.close(code=code)
    return ws


@web.middleware
async def require_login(request, handler):
    """Every route but PUBLIC needs the cookie. WebSockets and POSTs also need this site's own
    Origin, so another page open in the browser can't open a terminal or approve a prompt."""
    websocket = request.headers.get("Upgrade", "").lower() == "websocket"
    if request.path in PUBLIC:
        return await handler(request)
    if not logged_in(request):
        if websocket:
            return await _refuse_ws(request, 4401)
        if request.path.startswith("/api/"):  # fetch would follow a redirect and look successful
            return web.json_response({"ok": False, "error": "login"}, status=401)
        raise web.HTTPFound("/login")
    if (websocket or request.method != "GET") and not auth.same_origin(request):
        if websocket:
            return await _refuse_ws(request, 4403)
        raise web.HTTPForbidden(text="cross-origin request")
    return await handler(request)


async def healthz(request):
    return web.Response(text="ok")


def _sketch_cookie_domain(request):
    """Where sketchpad's cookie must go for its frame to see it: "" for this very host (cookies
    ignore ports, so http://lan:8794 and http://lan:8790 share them), the parent domain for a
    sibling name (tmls.example.com and sketchpad.example.com), None if sketchpad lives elsewhere."""
    here = request.host.rsplit(":", 1)[0] if request.host.count(":") == 1 else request.host
    for url in request.app.get("sketchpad", []):
        there = urlparse(url).hostname
        if there == here:
            return ""
        if there and "." in here and here.split(".", 1)[1] == there.split(".", 1)[1] \
                and "." in here.split(".", 1)[1]:
            return here.split(".", 1)[1]
    return None


async def page(request):
    # no-store: after logout, Back or a revisit must ask the server (and get the login page)
    response = web.FileResponse(STATIC / "index.html", headers={"Cache-Control": "no-store"})
    domain = _sketch_cookie_domain(request)
    creds = auth.load(request.app["auth_file"]) if domain is not None else None
    user = creds and auth.verify_cookie(creds["secret"], request.cookies.get(auth.COOKIE, ""))
    if user:  # logged in here = logged in to sketchpad (same credentials): no second login in Sketch
        response.set_cookie(auth.SKETCHPAD_COOKIE, auth.sketchpad_cookie(creds["secret"], user),
                            max_age=auth.SESSION_TTL, httponly=True, samesite="Lax", path="/",
                            domain=domain or None, secure=_https(request))
    return response


async def login_page(request):
    return web.FileResponse(STATIC / "login.html")


def _trusted(remote, proxies):
    """Is this peer one of the --trust-proxy addresses or networks?"""
    try:
        address = ipaddress.ip_address(remote)
    except ValueError:  # no address: a unix socket, or a test transport
        return False
    for proxy in proxies:
        try:
            if address in ipaddress.ip_network(proxy, strict=False):
                return True
        except ValueError:
            continue
    return False


def client_address(request):
    """Who is logging in. Behind a --trust-proxy address that is the proxy's X-Forwarded-For entry:
    the last one, since a proxy appends what it saw and anything before that came from the client."""
    forwarded = ", ".join(request.headers.getall("X-Forwarded-For", [])).split(",")[-1].strip()
    if forwarded and _trusted(request.remote, request.app["trusted_proxies"]):
        return forwarded
    return request.remote


async def login(request):
    """The password is a shell login: wrong ones cost a second, and five in a row from one
    address lock it out for five minutes. scrypt runs off the event loop so terminals keep going."""
    creds = auth.load(request.app["auth_file"])
    if not creds:
        return web.Response(status=401, text="no credentials: set a sketchpad password first")
    fails, now, client = request.app["fails"], time.monotonic(), client_address(request)
    count, since = fails.get(client, (0, now))
    if now - since > LOCKOUT:
        count, since = 0, now
    if count >= MAX_FAILS:
        raise web.HTTPFound("/login?error=locked")
    data = await request.post()
    user, password = str(data.get("username", "")), str(data.get("password", ""))
    if not await asyncio.to_thread(auth.check_login, creds, user, password):
        fails[client] = (count + 1, since)
        await asyncio.sleep(FAIL_DELAY)
        raise web.HTTPFound("/login?error=1")  # the form shows the message
    fails.pop(client, None)
    response = web.HTTPFound("/")
    response.set_cookie(auth.COOKIE, auth.make_cookie(creds["secret"], user),
                        max_age=auth.SESSION_TTL, httponly=True, samesite="Lax", path="/",
                        secure=_https(request))
    raise response


def _https(request):
    return request.secure or request.headers.get("X-Forwarded-Proto") == "https"


async def logout(request):
    response = web.HTTPFound("/login")
    response.del_cookie(auth.COOKIE, path="/")
    domain = _sketch_cookie_domain(request)
    if domain is not None:
        response.del_cookie(auth.SKETCHPAD_COOKIE, path="/", domain=domain or None)
    raise response


async def approve_prompt(request):
    """Yes/No on a waiting row. approve.answer re-reads the prompt and sends nothing if it changed.
    Only configured hosts: a host string goes to ssh, so "-oProxyCommand=..." must never get there."""
    try:
        data = await request.json()
        host, name, shown, yes = (data[field] for field in ("host", "name", "shown", "yes"))
        pane = data.get("pane")  # Claude's own tmux pane ("%7"); absent for sessions without one
        if (not (pane is None or isinstance(pane, str) and re.fullmatch(r"%[0-9]+", pane))
                or not isinstance(host, str) or not isinstance(name, str) or not name
                or not isinstance(shown, list) or not all(isinstance(line, str) for line in shown)
                or not isinstance(yes, bool) or host not in (*request.app["hosts"], hosts.LOCAL)):
            raise ValueError
    except (ValueError, KeyError, TypeError):
        return web.json_response({"ok": False, "error": "invalid approval request"}, status=400)
    error = await approve.answer(host, name, shown, yes, pane=pane)
    if error:
        return web.json_response({"ok": False, "error": error}, status=409)
    return web.json_response({"ok": True})


async def create_session(request):
    """New tmux session from the + on a host header. Only configured hosts, as for approve."""
    try:
        data = await request.json()
        host, folder, name, start = (data[f] for f in ("host", "folder", "name", "start"))
        if not all(isinstance(v, str) for v in (host, folder, name, start)) or host not in (*request.app["hosts"], hosts.LOCAL):
            raise ValueError
    except (ValueError, KeyError, TypeError):
        return web.json_response({"ok": False, "error": "invalid request"}, status=400)
    presets = create.load_presets()
    if start not in ("shell", "claude"):
        if start not in presets:
            return web.json_response({"ok": False, "error": "unknown start option"}, status=400)
        start = presets[start]
    error = create.check_name(name.strip())
    if error:
        return web.json_response({"ok": False, "error": error}, status=400)
    error = await create.create(host, name.strip(), folder.strip() or "~", start)
    if error:
        return web.json_response({"ok": False, "error": error}, status=409)
    return web.json_response({"ok": True})


def _session(data, hosts_list):
    """host and name of an existing session, from a JSON body. Only configured hosts, as for approve."""
    host, name = data["host"], data["name"]
    if (not isinstance(host, str) or not isinstance(name, str) or not name or "\0" in name
            or not hosts.allowed(host, hosts_list)):
        raise ValueError
    return host, name


async def rename_session(request):
    """Rename from a row's ⋯. The row's seen time goes with the new name, so it doesn't turn unread,
    and every open page hears of it at once: the one showing the session follows the new name."""
    try:
        data = await request.json()
        host, name = _session(data, request.app["hosts"])
        new = data["new"]
        if not isinstance(new, str):
            raise ValueError
    except (ValueError, KeyError, TypeError):
        return web.json_response({"ok": False, "error": "invalid request"}, status=400)
    new = new.strip()
    error = create.check_name(new)
    if error:
        return web.json_response({"ok": False, "error": error}, status=400)
    error = await tmux_ops.rename(host, name, new)
    if error:
        return web.json_response({"ok": False, "error": error}, status=409)
    old_key, new_key = f"{host}/{name}", f"{host}/{new}"
    seen = request.app["state"].seen
    if old_key in seen:  # copied, not moved: a poll in flight may still list the old name
        seen[new_key] = seen[old_key]
    await events.broadcast(request.app, {"t": "renamed", "old": old_key, "new": new_key, "name": new})
    return web.json_response({"ok": True})


async def kill_session(request):
    """Kill from a row's ⋯, in two steps like the TUI: without confirm it only says what is still
    running in the session, so the page can ask; with confirm it kills."""
    try:
        data = await request.json()
        host, name = _session(data, request.app["hosts"])
        confirm = data.get("confirm", False)
        if not isinstance(confirm, bool):
            raise ValueError
    except (ValueError, KeyError, TypeError):
        return web.json_response({"ok": False, "error": "invalid request"}, status=400)
    if not confirm:
        try:
            running = await tmux_ops.running_commands(host, name)
        except tmux_ops.ManageError as error:
            return web.json_response({"ok": False, "error": str(error)}, status=409)
        return web.json_response({"ok": True, "confirm": True, "running": running})
    error = await tmux_ops.kill(host, name)
    if error:
        return web.json_response({"ok": False, "error": error}, status=409)
    await events.broadcast(request.app, {"t": "killed", "key": f"{host}/{name}"})
    return web.json_response({"ok": True})


async def suggest_name(request):
    """The folder's git repo name on that host, else its basename."""
    try:
        data = await request.json()
        host, folder = data["host"], data["folder"]
        if not isinstance(host, str) or not isinstance(folder, str) or host not in (*request.app["hosts"], hosts.LOCAL):
            raise ValueError
    except (ValueError, KeyError, TypeError):
        return web.json_response({"ok": False, "error": "invalid request"}, status=400)
    return web.json_response({"ok": True, "name": await create.suggest_name(host, folder.strip() or "~")})


async def config(request):
    """Sketchpad's addresses (~/.config/tmls/sketchpad, one per line: home first, then away);
    the page uses the one whose scheme matches its own, so https never frames http.
    Presets are the New session form's named agent commands. app: an Android app is published."""
    return web.json_response({"sketchpad": request.app.get("sketchpad", []),
                              "presets": list(create.load_presets()),
                              "app": (request.app["apk_dir"] / "tmls.apk").is_file()})


APK_DIR = Path.home() / ".config" / "tmls" / "apk"
APK_TYPE = "application/vnd.android.package-archive"


async def app_manifest(request):
    """What the Android app's updater reads: versionCode, versionName, size, sha256, written by
    android/make.sh next to the APK. Never cached, so each new build is seen at once."""
    path = request.app["apk_dir"] / "latest.json"
    if not path.is_file():
        return web.json_response({"ok": False, "error": "no app published"}, status=404)
    return web.FileResponse(path, headers={"Cache-Control": "no-store", "Content-Type": "application/json"})


async def app_apk(request):
    path = request.app["apk_dir"] / "tmls.apk"
    if not path.is_file():
        return web.json_response({"ok": False, "error": "no app published"}, status=404)
    return web.FileResponse(path, headers={"Content-Type": APK_TYPE,
                                           "Content-Disposition": 'attachment; filename="tmls.apk"'})


async def app_update(request):
    # The app intercepts this link and checks for an update; a browser just lands on the page.
    raise web.HTTPSeeOther("/")


async def revalidate_static(request, response):
    """The page's own js and css: ask every time (a 304 when unchanged). With only Last-Modified a
    browser keeps using the old app.js for hours after a deploy, so new buttons don't show."""
    if request.path.startswith("/static/") and "Cache-Control" not in response.headers:
        response.headers["Cache-Control"] = "no-cache"


def static_version():
    """A hash of the page's files: it changes on a redeploy that changes the page, and an open page
    that sees a different one reloads itself (an iPad home-screen app has no reload button)."""
    digest = hashlib.sha256()
    for path in sorted(STATIC.rglob("*")):
        if path.is_file():
            digest.update(path.name.encode() + path.read_bytes())
    return digest.hexdigest()[:12]


def make_app(auth_file, hosts_list):
    app = web.Application(middlewares=[require_login])
    app["auth_file"], app["hosts"], app["version"] = auth_file, hosts_list, static_version()
    app["fails"], app["trusted_proxies"], app["apk_dir"] = {}, set(), APK_DIR
    app.router.add_get("/healthz", healthz)
    app.router.add_get("/", page)
    app.router.add_get("/login", login_page)
    app.router.add_post("/login", login)
    app.router.add_post("/logout", logout)
    app.router.add_post("/api/approve", approve_prompt)
    app.router.add_post("/api/create", create_session)
    app.router.add_post("/api/suggest-name", suggest_name)
    app.router.add_post("/api/rename", rename_session)
    app.router.add_post("/api/kill", kill_session)
    term.setup(app)
    app.router.add_get("/api/config", config)
    app.router.add_get("/app/latest.json", app_manifest)
    app.router.add_get("/app/tmls.apk", app_apk)
    app.router.add_get("/app/update", app_update)
    app.router.add_static("/static", STATIC)
    app.on_response_prepare.append(revalidate_static)
    events.setup(app)
    return app


def _network(value):
    try:
        return str(ipaddress.ip_network(value, strict=False))
    except ValueError:
        raise argparse.ArgumentTypeError(f"{value!r} is not an address or network")


def parse_args(argv=None):
    parser = argparse.ArgumentParser(prog="tmls-web")
    parser.add_argument("--bind", action="append", required=True, help="address to listen on (repeatable)")
    parser.add_argument("--port", type=int, default=8794)
    parser.add_argument("--trust-proxy", action="append", default=[], metavar="ADDR|CIDR", type=_network,
                        help="a reverse proxy (or its network) whose X-Forwarded-For names the client (repeatable)")
    parser.add_argument("--apk-dir", type=Path, default=APK_DIR, metavar="DIR",
                        help="where android/make.sh publishes tmls.apk and latest.json for the app's updater")
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    app = make_app(auth.AUTH_FILE, hosts.hosts(hosts.read_config()))
    app["trusted_proxies"], app["apk_dir"] = set(args.trust_proxy), args.apk_dir
    app["sketchpad"] = hosts.read_config(hosts.SKETCHPAD)
    web.run_app(app, host=args.bind, port=args.port)
