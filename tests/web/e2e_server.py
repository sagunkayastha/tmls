"""tmls web with fake hosts, for tests/web/e2e_web.py. Run: uv run python tests/web/e2e_server.py DIR PORT

alpha is a plain tmux session, beta is Claude waiting on a permission prompt. Terminals run
`cat` instead of tmux. approve.answer calls are appended to DIR/approved.jsonl; the prompt
beta shows can be changed by writing DIR/prompt.json. Created sessions join box's list, and only they can be renamed or killed.
"""
import dataclasses
import hashlib
import json
import sys
from pathlib import Path

from aiohttp import web

from tmls import approve, create, hosts, prompts, tmux_ops
from tmls.web import server

folder, port = Path(sys.argv[1]), int(sys.argv[2])
salt = "00" * 16
(folder / "auth.json").write_text(json.dumps({
    "username": "tester", "salt": salt, "secret": "e2e" * 20,
    "hash": hashlib.scrypt(b"pw", salt=bytes.fromhex(salt), n=2**14, r=8, p=1).hex()}))


created = []


async def list_host(host):
    if host == "spare":  # online, no tmux sessions yet
        return True, []
    return True, [hosts.Session("box", "alpha", 1, False, 0, 1000),
                  hosts.Session("box", "beta", 1, False, 0, 1000, claude="waiting", claude_since=990,
                                waiting="permission prompt", title="beta"), *created]


async def create_session(host, name, folder, start):
    if name in ("alpha", "beta", *(s.name for s in created)):
        return f"a session named {name} already exists"
    created.append(hosts.Session("box", name, 1, False, 0, 1000))
    return None


async def rename_session(host, old, new):
    for i, s in enumerate(created):
        if s.name == old:
            created[i] = dataclasses.replace(s, name=new)
            return None
    return f"can't find session: {old}"


async def running_commands(host, name):
    return ["claude"] if name == "beta" else []


async def kill_session(host, name):
    if not any(s.name == name for s in created):
        return f"can't find session: {name}"
    created[:] = [s for s in created if s.name != name]
    return None


async def suggest_name(host, folder):
    return "repo-" + folder.rsplit("/", 1)[-1]


async def run(argv, stdin=None):
    return 0, "$ make\nbuild ok\n"


def prompt():
    path = folder / "prompt.json"
    return json.loads(path.read_text()) if path.exists() else ["Bash", "rm x"]


async def current(host, name, pane=None):
    return prompt() if name == "beta" else None


async def answer(host, name, shown, yes, pane=None):
    with open(folder / "approved.jsonl", "a") as f:
        f.write(json.dumps([host, name, shown, yes]) + "\n")
    return None if shown == prompt() else f"{name} isn't asking that any more"


hosts.list_host, prompts._run, approve.current, approve.answer = list_host, run, current, answer
create.create, create.suggest_name = create_session, suggest_name
tmux_ops.rename, tmux_ops.kill, tmux_ops.running_commands = rename_session, kill_session, running_commands
create.load_presets = lambda: {"Opus plan": ("claude", "--model", "opus")}
app = server.make_app(folder / "auth.json", ["box", "spare"])
app["poll_interval"] = 0.3
(folder / "apk").mkdir(exist_ok=True)
(folder / "apk" / "tmls.apk").write_bytes(b"PK")  # a published app: Android browsers get the link
app["apk_dir"] = folder / "apk"
app["attach_argv"] = lambda host, name: ["sh", "-c", 'echo "attached-$0"; exec cat', name]
app["sketchpad"] = [f"http://127.0.0.1:{port}/healthz"]
web.run_app(app, host="127.0.0.1", port=port, print=None)
