"""The browser's session rows: built from host listings, diffed, and turned into alerts."""
from dataclasses import dataclass, field

from tmls import hosts

ALERTS = {"waiting", "done", "failed"}


def key(host, name):
    return f"{host}/{name}"


@dataclass
class State:
    seen: dict = field(default_factory=dict)     # key -> host time it was last looked at
    started: dict = field(default_factory=dict)  # host -> host time we started watching
    marks: dict = field(default_factory=dict)    # key -> mark from the last build
    rows: dict = field(default_factory=dict)     # key -> row from the last build


def second_line(s, mark, screen):
    """Why it waits; else Claude's conversation title (a Claude pane's last line is its footer);
    else the last non-empty line on screen."""
    if mark == "waiting" and s.waiting:
        return s.waiting
    if s.claude:
        return s.title or s.name
    lines = [line.strip() for line in (screen or "").splitlines() if line.strip()]
    return lines[-1] if lines else ""


def build(state, found, extras):
    """found: [(host, online, sessions)]; extras: key -> {"line": screen text, "shown": prompt lines}.
    An offline host keeps its last rows, marked offline."""
    out = []
    for host, online, sessions in found:
        if not online:
            out.extend({**r, "online": False} for r in state.rows.values() if r["host"] == host)
            continue
        if sessions:
            state.started.setdefault(host, sessions[0].now - hosts.QUIET)
        for s in sessions:
            k = key(host, s.name)
            mark = hosts.status(s, state.seen.get(k, 0), state.started[host])
            extra = extras.get(k, {})
            out.append({"key": k, "host": host, "label": hosts.label(host), "name": s.name, "mark": mark,
                        "line": second_line(s, mark, extra.get("line")), "waiting": s.waiting,
                        "shown": extra.get("shown") if mark == "waiting" else None, "pane": s.pane,
                        "online": True})
    state.marks = {r["key"]: r["mark"] for r in out}
    state.rows = {r["key"]: r for r in out}
    return out


def diff(old, new):
    keys = {r["key"] for r in new}
    return {"set": [r for r in new if old.get(r["key"]) != r], "gone": [k for k in old if k not in keys]}


def alerts(old_marks, new):
    return [{"key": r["key"], "name": r["name"], "mark": r["mark"]} for r in new
            if r["mark"] in ALERTS and old_marks.get(r["key"]) not in (None, r["mark"])]
