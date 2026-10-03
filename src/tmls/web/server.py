"""tmls in a browser: one big terminal, status rows, approve, sketch."""
import argparse
import asyncio
import time
from pathlib import Path

from aiohttp import web

from tmls import approve, hosts
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


async def page(request):
    # no-store: after logout, Back or a revisit must ask the server (and get the login page)
    return web.FileResponse(STATIC / "index.html", headers={"Cache-Control": "no-store"})


async def login_page(request):
    return web.FileResponse(STATIC / "login.html")


async def login(request):
    """The password is a shell login: wrong ones cost a second, and five in a row from one
    address lock it out for five minutes. scrypt runs off the event loop so terminals keep going."""
    creds = auth.load(request.app["auth_file"])
    if not creds:
        return web.Response(status=401, text="no credentials: set a sketchpad password first")
    fails, now = request.app["fails"], time.monotonic()
    count, since = fails.get(request.remote, (0, now))
    if now - since > LOCKOUT:
        count, since = 0, now
    if count >= MAX_FAILS:
        raise web.HTTPFound("/login?error=locked")
    data = await request.post()
    user, password = str(data.get("username", "")), str(data.get("password", ""))
    if not await asyncio.to_thread(auth.check_login, creds, user, password):
        fails[request.remote] = (count + 1, since)
        await asyncio.sleep(FAIL_DELAY)
        raise web.HTTPFound("/login?error=1")  # the form shows the message
    fails.pop(request.remote, None)
    response = web.HTTPFound("/")
    response.set_cookie(auth.COOKIE, auth.make_cookie(creds["secret"], user),
                        max_age=auth.SESSION_TTL, httponly=True, samesite="Lax", path="/",
                        secure=request.secure or request.headers.get("X-Forwarded-Proto") == "https")
    raise response


async def logout(request):
    response = web.HTTPFound("/login")
    response.del_cookie(auth.COOKIE, path="/")
    raise response


async def approve_prompt(request):
    """Yes/No on a waiting row. approve.answer re-reads the prompt and sends nothing if it changed.
    Only configured hosts: a host string goes to ssh, so "-oProxyCommand=..." must never get there."""
    try:
        data = await request.json()
        host, name, shown, yes = (data[field] for field in ("host", "name", "shown", "yes"))
        if (not isinstance(host, str) or not isinstance(name, str) or not name
                or not isinstance(shown, list) or not all(isinstance(line, str) for line in shown)
                or not isinstance(yes, bool) or host not in (*request.app["hosts"], hosts.LOCAL)):
            raise ValueError
    except (ValueError, KeyError, TypeError):
        return web.json_response({"ok": False, "error": "invalid approval request"}, status=400)
    error = await approve.answer(host, name, shown, yes)
    if error:
        return web.json_response({"ok": False, "error": error}, status=409)
    return web.json_response({"ok": True})


async def config(request):
    """Sketchpad's addresses (~/.config/tmls/sketchpad, one per line: home first, then away);
    the page uses the one whose scheme matches its own, so https never frames http."""
    return web.json_response({"sketchpad": request.app.get("sketchpad", [])})


def make_app(auth_file, hosts_list):
    app = web.Application(middlewares=[require_login])
    app["auth_file"], app["hosts"] = auth_file, hosts_list
    app["fails"] = {}
    app.router.add_get("/healthz", healthz)
    app.router.add_get("/", page)
    app.router.add_get("/login", login_page)
    app.router.add_post("/login", login)
    app.router.add_post("/logout", logout)
    app.router.add_post("/api/approve", approve_prompt)
    term.setup(app)
    app.router.add_get("/api/config", config)
    app.router.add_static("/static", STATIC)
    events.setup(app)
    return app


def main(argv=None):
    parser = argparse.ArgumentParser(prog="tmls-web")
    parser.add_argument("--bind", action="append", required=True, help="address to listen on (repeatable)")
    parser.add_argument("--port", type=int, default=8794)
    args = parser.parse_args(argv)
    app = make_app(auth.AUTH_FILE, hosts.hosts(hosts.read_config()))
    app["sketchpad"] = hosts.read_config(hosts.SKETCHPAD)
    web.run_app(app, host=args.bind, port=args.port)
