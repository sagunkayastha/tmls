"""tmls in a browser: one big terminal, status rows, approve, sketch."""
import argparse
from pathlib import Path

from aiohttp import web

from tmls import hosts

STATIC = Path(__file__).parent / "static"


async def healthz(request):
    return web.Response(text="ok")


def make_app(auth_file, hosts_list):
    app = web.Application()
    app["auth_file"], app["hosts"] = auth_file, hosts_list
    app.router.add_get("/healthz", healthz)
    return app


def main(argv=None):
    parser = argparse.ArgumentParser(prog="tmls-web")
    parser.add_argument("--bind", action="append", required=True, help="address to listen on (repeatable)")
    parser.add_argument("--port", type=int, default=8794)
    args = parser.parse_args(argv)
    auth_file = Path.home() / ".config/sketchpad/auth.json"
    web.run_app(make_app(auth_file, hosts.hosts(hosts.read_config())), host=args.bind, port=args.port)
