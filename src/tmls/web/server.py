"""tmls in a browser: one big terminal, status rows, approve, sketch."""
import argparse
from pathlib import Path

from aiohttp import web

from tmls import hosts
from tmls.web import auth, events

STATIC = Path(__file__).parent / "static"
PUBLIC = ("/healthz", "/login")


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
        raise web.HTTPFound("/login")
    if (websocket or request.method != "GET") and not auth.same_origin(request):
        if websocket:
            return await _refuse_ws(request, 4403)
        raise web.HTTPForbidden(text="cross-origin request")
    return await handler(request)


async def healthz(request):
    return web.Response(text="ok")


async def page(request):
    return web.FileResponse(STATIC / "index.html")


async def login_page(request):
    return web.FileResponse(STATIC / "login.html")


async def login(request):
    creds = auth.load(request.app["auth_file"])
    if not creds:
        return web.Response(status=401, text="no credentials: set a sketchpad password first")
    data = await request.post()
    user, password = str(data.get("username", "")), str(data.get("password", ""))
    if not auth.check_login(creds, user, password):
        return web.Response(status=401, text="wrong username or password")
    response = web.HTTPFound("/")
    response.set_cookie(auth.COOKIE, auth.make_cookie(creds["secret"], user),
                        max_age=auth.SESSION_TTL, httponly=True, samesite="Lax", path="/",
                        secure=request.secure or request.headers.get("X-Forwarded-Proto") == "https")
    raise response


async def logout(request):
    response = web.HTTPFound("/login")
    response.del_cookie(auth.COOKIE, path="/")
    raise response


def make_app(auth_file, hosts_list):
    app = web.Application(middlewares=[require_login])
    app["auth_file"], app["hosts"] = auth_file, hosts_list
    app.router.add_get("/healthz", healthz)
    app.router.add_get("/", page)
    app.router.add_get("/login", login_page)
    app.router.add_post("/login", login)
    app.router.add_post("/logout", logout)
    events.setup(app)
    return app


def main(argv=None):
    parser = argparse.ArgumentParser(prog="tmls-web")
    parser.add_argument("--bind", action="append", required=True, help="address to listen on (repeatable)")
    parser.add_argument("--port", type=int, default=8794)
    args = parser.parse_args(argv)
    web.run_app(make_app(auth.AUTH_FILE, hosts.hosts(hosts.read_config())), host=args.bind, port=args.port)
