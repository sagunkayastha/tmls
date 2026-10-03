"""ag: agents on every tmls host can list sessions, read their screens and message each other.

    ag ls
    ag read NAME [-n 40]
    ag send NAME TEXT [--from ME] [--mode MODE]

NAME is a Claude session name, a tmux session name, or a Codex thread name (`/rename` in Codex);
`host:NAME` picks the host. Messages go in through each agent's own channel: a Claude session's
inbox socket, `codex queue` for Codex, tmux paste for anything else. Delivery means submitted,
not read; ask for a reply when it matters.
"""
import argparse
import asyncio
import json
import re
import shlex
import subprocess
import sys

from tmls import hosts, prompts

# A Claude session that bypasses permission prompts holds messages whose sender doesn't say it
# does too. --mode states the sender's own mode; never claim one you aren't running in.
MODES = ("bypass", "default", "acceptEdits", "plan", "auto")


class Ambiguous(Exception):
    pass


def _argv(host, script):
    return prompts._run_argv(host, script)


def socket_argv(host, path):
    """Writes stdin to a Unix socket on the host (the inbox needs no token on Linux)."""
    code = ("import socket,sys; s=socket.socket(socket.AF_UNIX); s.settimeout(5); "
            "s.connect(sys.argv[1]); s.sendall(sys.stdin.buffer.read())")
    return _argv(host, shlex.join(["python3", "-c", code, path]))


def codex_argv(host, thread, text):
    # non-login ssh has no ~/.local/bin on PATH
    find = 'c=$(command -v codex || echo "$HOME/.local/bin/codex"); "$c" queue'
    return _argv(host, f"{find} --thread {shlex.quote(thread)} --message {shlex.quote(text)}")


def read_argv(host, name, lines, pane=None):
    # pane: Claude's own ("%7"); after a split the window's active pane may be a shell
    return _argv(host, f"tmux capture-pane -p -t {shlex.quote(pane or f'={name}:')} -S -{int(lines)}")


def message_line(text, sender, mode):
    content = text
    if sender and mode:
        content = (f'<cross-session-message from-name="{sender}" from-mode="{mode}">\n'
                   f"{text}\n</cross-session-message>")
    return json.dumps({"type": "user", "message": {"role": "user", "content": content}})


def resolve(target, claudes, tmux):
    """("claude", host, session file) | ("tmux", host, name) | ("codex", host or None, name)."""
    host, _, name = target.rpartition(":") if ":" in target else (None, "", target)
    if host == hosts.socket.gethostname():
        host = hosts.LOCAL
    places = [host] if host else list(claudes)
    found = [(h, c) for h in places for c in claudes.get(h, []) if c.get("name") == name]
    if len(found) > 1:
        raise Ambiguous(f"{name} is on several hosts: " + ", ".join(f"{h}:{name}" for h, _ in found))
    if found:
        return ("claude", *found[0])
    found = [h for h in places if name in tmux.get(h, [])]
    if len(found) > 1:
        raise Ambiguous(f"{name} is on several hosts: " + ", ".join(f"{h}:{name}" for h in found))
    if found:
        return ("tmux", found[0], name)
    return ("codex", host, name)


def survey_argv(host):
    # unlike tmls's listing, Claude sessions count even where no tmux server runs; -u as in hosts.list_argv
    return _argv(host, f"date +%s; tmux -u list-windows -a -F {shlex.quote(hosts.FORMAT)} 2>/dev/null; "
                       f"{{ {hosts.CLAUDE}; }}")


async def _survey(names):
    """{host: [Claude session files]}, {host: [tmux session names]} for the reachable hosts."""
    async def one(host):
        code, out = await prompts._run(survey_argv(host))
        if code != 0:
            return [], []
        tmux_part, _, claude_part = out.partition("\n---\n")
        sessions = sorted({line.rsplit(":", 3)[0] for line in tmux_part.splitlines()[1:]})
        files = []
        for line in claude_part.splitlines():
            try:
                files.append(json.loads(line))
            except ValueError:
                continue
        return [f for f in files if f.get("kind", "interactive") == "interactive"], sessions
    got = await asyncio.gather(*(one(h) for h in names))
    return ({h: c for h, (c, _) in zip(names, got)}, {h: t for h, (_, t) in zip(names, got)})


def _hosts():
    return [hosts.LOCAL] + [h for h in hosts.read_config() if h not in (hosts.LOCAL, hosts.socket.gethostname())]


def _run(argv, stdin=None):
    r = subprocess.run(argv, input=stdin, capture_output=True, timeout=20)
    return r.returncode, (r.stdout + r.stderr).decode(errors="replace").strip()


def cmd_ls(args):
    claudes, tmux = asyncio.run(_survey(_hosts()))
    for host in claudes:
        print(hosts.label(host))
        by_tmux = {(c.get("tmux") or "").split(":")[0]: c for c in claudes[host]}
        for name in tmux[host]:
            c = by_tmux.get(name)
            print(f"  {name:<22} " + (f"claude {c.get('name')} · {c.get('status')}" if c else "tmux"))
        for c in claudes[host]:
            if not c.get("tmux"):
                print(f"  {c.get('name') or c['pid']:<22} claude · {c.get('status')} (no tmux)")
    print("Codex threads: by their /rename name (ag send NAME …)")
    return 0


def cmd_read(args):
    claudes, tmux = asyncio.run(_survey(_hosts()))
    try:
        kind, host, what = resolve(args.name, claudes, tmux)
    except Ambiguous as e:
        print(e, file=sys.stderr)
        return 2
    if kind == "claude":
        if not what.get("tmux"):
            print(f"{args.name} isn't in tmux; its screen can't be read", file=sys.stderr)
            return 1
        name = what["tmux"].split(":")[0]
        pane = re.search(r"%\d+$", what["tmux"])
    elif kind == "tmux":
        name, pane = what, None
    else:
        print(f"no session named {args.name}", file=sys.stderr)
        return 1
    code, out = _run(read_argv(host, name, args.n, pane and pane.group(0)))
    print(out)
    return code


def cmd_send(args):
    claudes, tmux = asyncio.run(_survey(_hosts()))
    try:
        kind, host, what = resolve(args.name, claudes, tmux)
    except Ambiguous as e:
        print(e, file=sys.stderr)
        return 2
    if kind == "claude":
        line = message_line(args.text, args.sender, args.mode) + "\n"
        code, out = _run(socket_argv(host, what["messagingSocketPath"]), stdin=line.encode())
    elif kind == "tmux":
        error = asyncio.run(prompts.send(host, what, args.text))
        code, out = (1, error) if error else (0, "")
    else:
        code, out = 1, f"no Claude, tmux or Codex session named {args.name}"
        for h in [host] if host else _hosts():
            code, out = _run(codex_argv(h, what, args.text))
            if code == 0:
                host = h
                break
    if code == 0:
        print(f"submitted to {kind} {args.name} on {hosts.label(host)}")
    else:
        print(out, file=sys.stderr)
    return 0 if code == 0 else 1


def main(argv=None):
    ap = argparse.ArgumentParser(prog="ag", description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("ls", help="sessions on every host")
    r = sub.add_parser("read", help="the last lines on a session's screen")
    r.add_argument("name")
    r.add_argument("-n", type=int, default=40)
    s = sub.add_parser("send", help="message a Claude, Codex or tmux session")
    s.add_argument("name")
    s.add_argument("text")
    s.add_argument("--from", dest="sender", help="your name, shown to a Claude receiver")
    s.add_argument("--mode", choices=MODES, help="your own permission mode (truthfully)")
    args = ap.parse_args(argv)
    return {"ls": cmd_ls, "read": cmd_read, "send": cmd_send}[args.cmd](args)


if __name__ == "__main__":
    sys.exit(main())
