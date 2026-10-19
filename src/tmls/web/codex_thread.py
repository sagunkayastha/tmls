"""Which Codex conversation runs in a tmux session, and where its record is.

Codex names no tmux pane anywhere, and since its app-server daemon one process writes every TUI's
record, so an open file alone doesn't tell. One host call prints the facts and pick() decides:
- the session's panes (pane title: "<thread name> | <folder>", current folder), the user's
  processes (to find the Codex TUIs under each pane),
- the records and thread locks Codex processes hold open (the threads that are live now),
- for each live thread: its record's path and first line (folder, which client started it), and
  its name from Codex's state database when sqlite3 is there.
A TUI holding a record itself (no daemon) settles it; else the thread named in the pane title,
else the thread `codex resume <id>` opened, else the one live thread started in the pane's folder.
"""
import json
import re
import shlex

from tmls import hosts, prompts
from tmls.web.record import epoch

UUID = r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
RECORD = re.compile(rf"^/.*/rollout-[^/]*-({UUID})\.jsonl$")
RESUME = re.compile(rf"\sresume\s+({UUID})(\s|$)")
LOCK = re.compile(rf"/thread-writer-locks/({UUID})\.lock$")
META = 3000  # bytes of a record's first line: the folder and client come before the long instructions

SCRIPT = r"""
printf 'now\t%s\n' "$(date +%s)"
tmux -u list-panes -s -t {target} -F 'pane	#{{pane_id}}	#{{pane_pid}}	#{{pane_current_path}}	#{{pane_title}}' 2>/dev/null </dev/null
u=$(id -u)
ps -u "$u" -o pid=,ppid=,etimes=,args= 2>/dev/null | awk '{{p=$1; q=$2; e=$3; $1=$2=$3=""; sub(/^ +/, ""); print "proc\t" p "\t" q "\t" e "\t" substr($0, 1, 200)}}'
fds=$(for p in $( (pgrep -u "$u" codex; pgrep -u "$u" -f '/codex( |$)') 2>/dev/null | sort -u); do
  printf 'cwd\t%s\t%s\n' "$p" "$(readlink /proc/$p/cwd 2>/dev/null)"
  for f in /proc/$p/fd/*; do l=$(readlink "$f" 2>/dev/null) || continue
    case $l in */rollout-*.jsonl|*/thread-writer-locks/*.lock) printf 'fd\t%s\t%s\n' "$p" "$l";; esac
  done
done)
printf '%s\n' "$fds"
ids=$(printf '%s\n' "$fds" | grep -o -E '{uuid}\.(lock|jsonl)$' | sed 's/\.[a-z]*$//' | sort -u)
for i in $ids; do
  f=$(ls "$HOME"/.codex/sessions/*/*/*/rollout-*-"$i".jsonl 2>/dev/null | head -n 1)
  [ -n "$f" ] || continue
  printf 'thread\t%s\t%s\t' "$i" "$f"; head -c {meta} "$f" | head -n 1 | tr '\t\r' '  '; echo
done
db=$(for f in "$HOME"/.codex/state_*.sqlite; do [ -f "$f" ] || continue; n=${{f##*state_}}; echo "${{n%.sqlite}} $f"; done | sort -n | tail -n 1 | cut -d' ' -f2-)
if [ -n "$ids" ] && [ -n "$db" ] && command -v sqlite3 >/dev/null; then
  q=$(printf "'%s'," $ids); sqlite3 -readonly -separator '	' "$db" "select 'name', id, name from threads where id in (${{q%,}}) and name is not null" 2>/dev/null
fi
true
"""
SUBCOMMANDS = {"exec", "e", "app-server", "mcp", "mcp-server", "login", "logout", "apply", "a", "cloud", "sandbox"}
SLACK = 5  # seconds: a thread a TUI started is not older than the TUI


def find_argv(host, name):
    return hosts.run_argv(host, SCRIPT.format(target=shlex.quote(f"={name}:"), uuid=UUID, meta=META))


def _field(line, key):
    m = re.search(rf'"{key}":\s*"((?:[^"\\]|\\.)*)"', line)
    if not m:
        return None
    try:
        return json.loads(f'"{m.group(1)}"')
    except ValueError:
        return None


def is_tui(args):
    """A Codex TUI's command line (not its app-server daemon, `codex exec` or another subcommand)."""
    words = args.split()
    for i, word in enumerate(words[:2]):
        if re.search(r"(^|/)codex$", word):
            return len(words) == i + 1 or words[i + 1] not in SUBCOMMANDS
    return False


def pick(out):
    """{"pane", "thread", "path", "title"} of the Codex conversation in the listed session; thread,
    path and title are None while a Codex runs there with no conversation yet (none is written before
    the first message), or when it can't be told which live thread is this pane's. None when no Codex
    runs in it."""
    now, panes, procs, cwds, fds, threads, names = None, [], {}, {}, [], {}, {}
    for line in out.splitlines():
        part = line.split("\t")
        if part[0] == "now" and len(part) == 2 and part[1].isdigit():
            now = int(part[1])
        elif part[0] == "pane" and len(part) >= 5:
            panes.append({"id": part[1], "pid": part[2], "cwd": part[3], "title": "\t".join(part[4:])})
        elif part[0] == "proc" and len(part) >= 5 and part[3].isdigit():
            procs[part[1]] = (part[2], int(part[3]), "\t".join(part[4:]))
        elif part[0] == "cwd" and len(part) == 3:
            cwds[part[1]] = part[2]
        elif part[0] == "fd" and len(part) == 3:
            fds.append((part[1], part[2]))
        elif part[0] == "thread" and len(part) >= 4 and RECORD.match(part[2]):
            meta = "\t".join(part[3:])
            threads[part[1]] = {"path": part[2], "cwd": _field(meta, "cwd"), "originator": _field(meta, "originator"),
                                "ts": epoch(_field(meta, "timestamp"))}
        elif part[0] == "name" and len(part) >= 3:
            names[part[1]] = "\t".join(part[2:])
    children = {}
    for pid, (ppid, _, _) in procs.items():
        children.setdefault(ppid, []).append(pid)
    tui = {pid for pid, (_, _, args) in procs.items() if is_tui(args)}
    tops = {pid for pid in tui if procs[pid][0] not in tui}  # one per Codex: its wrapper, not the binary under it

    def under(root):
        found, todo = [], [root]
        while todo:
            pid = todo.pop()
            if pid in tui:
                found.append(pid)
            todo += children.get(pid, [])
        return found

    # threads of Codex's own TUI only: not `codex exec` runs or other clients of the daemon
    live = {i: t for i, t in threads.items() if t["originator"] == "codex-tui"}

    def found(pane, thread_id):
        t = threads.get(thread_id) if thread_id else None
        return {"pane": pane["id"], "thread": thread_id if t else None, "path": t["path"] if t else None,
                "title": names.get(thread_id) if t else None}

    with_codex = [(p, under(p["pid"])) for p in panes]
    with_codex = [(p, pids) for p, pids in with_codex if pids]
    for pane, pids in with_codex:  # a TUI that writes its own record
        for pid, target in fds:
            m = RECORD.match(target)
            if pid in pids and m and m.group(1) in threads:
                return found(pane, m.group(1))
    for pane, _ in with_codex:
        # the thread named in the pane's title: "<name> | <folder>" after a busy spinner, or with a
        # state in front ("[ . ] Action Required | <name> | <folder>")
        parts = [re.sub(r"^\W+\s", "", part) for part in pane["title"].split(" | ")]
        named = [i for i in live if names.get(i) and names[i] in parts]
        if len(named) == 1:
            return found(pane, named[0])
    for pane, pids in with_codex:  # `codex resume <id>`: stale after /new, so only after the title
        resumed = {m.group(1) for p in pids for m in [RESUME.search(procs[p][2])] if m}
        here = [i for i in resumed if i in live]
        if len(here) == 1:
            return found(pane, here[0])
    for pane, pids in with_codex:
        # the one live thread in this Codex's folder, started after it; only when no other Codex
        # on the host works in that folder (its threads would look the same)
        mine = [p for p in pids if p in tops]
        folder = next((cwds[p] for p in mine if cwds.get(p)), pane["cwd"])
        others = [p for p in tops - set(mine) if cwds.get(p, folder) == folder]
        if len(mine) != 1 or others or now is None:
            continue
        started = now - procs[mine[0]][1]
        here = [i for i, t in live.items() if t["cwd"] == folder and t["ts"] >= started - SLACK]
        if len(here) == 1:
            return found(pane, here[0])
    return found(with_codex[0][0], None) if with_codex else None


async def find(host, name):
    """pick() for a session, or a str saying why the host could not be asked."""
    code, out = await prompts._run(find_argv(host, name))
    if code != 0:  # the script ends in `true`: anything else is ssh failing
        return out.strip() if code is None else "couldn't reach the host"
    return pick(out)
