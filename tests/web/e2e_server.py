"""tmls web with fake hosts, for tests/web/e2e_web.py. Run: uv run python tests/web/e2e_server.py DIR PORT

alpha is a plain tmux session, beta is Claude waiting on a permission prompt. Terminals run
`cat` instead of tmux. approve.answer calls are appended to DIR/approved.jsonl; the prompt
beta shows can be changed by writing DIR/prompt.json.
"""
import hashlib
import json
import sys
from pathlib import Path

from aiohttp import web

from tmls import approve, hosts, prompts
from tmls.web import server

folder, port = Path(sys.argv[1]), int(sys.argv[2])
salt = "00" * 16
(folder / "auth.json").write_text(json.dumps({
    "username": "tester", "salt": salt, "secret": "e2e" * 20,
    "hash": hashlib.scrypt(b"pw", salt=bytes.fromhex(salt), n=2**14, r=8, p=1).hex()}))


async def list_host(host):
    return True, [hosts.Session("box", "alpha", 1, False, 0, 1000),
                  hosts.Session("box", "beta", 1, False, 0, 1000, claude="waiting", claude_since=990,
                                waiting="permission prompt", title="beta")]


async def run(argv, stdin=None):
    return 0, "$ make\nbuild ok\n"


def prompt():
    path = folder / "prompt.json"
    return json.loads(path.read_text()) if path.exists() else ["Bash", "rm x"]


async def current(host, name):
    return prompt() if name == "beta" else None


async def answer(host, name, shown, yes):
    with open(folder / "approved.jsonl", "a") as f:
        f.write(json.dumps([host, name, shown, yes]) + "\n")
    return None if shown == prompt() else f"{name} isn't asking that any more"


hosts.list_host, prompts._run, approve.current, approve.answer = list_host, run, current, answer
app = server.make_app(folder / "auth.json", ["box"])
app["poll_interval"] = 0.3
app["attach_argv"] = lambda host, name: ["sh", "-c", 'echo "attached-$0"; exec cat', name]
app["sketchpad"] = [f"http://127.0.0.1:{port}/healthz"]
web.run_app(app, host="127.0.0.1", port=port, print=None)
