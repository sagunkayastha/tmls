from tmls import hosts
from tmls.web import rows


def sess(name, activity=0, now=100, **kw):
    return hosts.Session("box", name, 1, False, activity, now, **kw)


def test_build_marks_running_and_waiting_with_shown():
    state = rows.State()
    found = [("box", True, [sess("a", claude="busy", claude_since=90),
                            sess("b", claude="waiting", claude_since=90, waiting="permission prompt")])]
    out = {r["key"]: r for r in rows.build(state, found, {"box/b": {"line": None, "shown": ["Bash", "rm x"]}})}
    assert out["box/a"]["mark"] == "running"
    assert out["box/b"]["mark"] == "waiting" and out["box/b"]["shown"] == ["Bash", "rm x"]
    assert out["box/b"]["line"] == "permission prompt"
    assert state.started["box"] == 100 - hosts.QUIET


def test_offline_host_keeps_rows_greyed():
    state = rows.State()
    rows.build(state, [("box", True, [sess("a")])], {})
    out = rows.build(state, [("box", False, [])], {})
    assert [(r["key"], r["online"]) for r in out] == [("box/a", False)]


def test_diff_reports_changed_and_gone():
    old = {"box/a": {"key": "box/a", "mark": "idle"}, "box/b": {"key": "box/b", "mark": "idle"}}
    new = [{"key": "box/a", "mark": "idle"}, {"key": "box/c", "mark": "running"}]
    assert rows.diff(old, new) == {"set": [{"key": "box/c", "mark": "running"}], "gone": ["box/b"]}


def test_alerts_only_on_known_transitions():
    new = [{"key": "box/a", "name": "a", "mark": "done"}, {"key": "box/b", "name": "b", "mark": "done"},
           {"key": "box/c", "name": "c", "mark": "done"}]
    old = {"box/a": "running", "box/c": "done"}
    assert rows.alerts(old, new) == [{"key": "box/a", "name": "a", "mark": "done"}]


def test_second_line():
    assert rows.second_line(sess("a", claude="waiting", waiting="input needed"), "waiting", "x") == "input needed"
    assert rows.second_line(sess("a", claude="idle", title="Fix parser"), "idle", "footer") == "Fix parser"
    assert rows.second_line(sess("a"), "idle", "a\nb\n\n  \n") == "b"
    assert rows.second_line(sess("a"), "idle", None) == ""


def test_seen_turns_done_into_idle():
    state = rows.State()
    found = [("box", True, [sess("a", claude="idle", claude_since=95)])]
    assert rows.build(state, found, {})[0]["mark"] == "done"
    state.seen["box/a"] = 100
    assert rows.build(state, found, {})[0]["mark"] == "idle"


def test_odd_names_survive():
    name = 'my "odd" name;x'
    out = rows.build(rows.State(), [("box", True, [sess(name)])], {})
    assert out[0]["name"] == name and out[0]["key"] == f"box/{name}"


def test_rows_carry_a_display_label(monkeypatch):
    monkeypatch.setattr(hosts, "label", lambda h: "archbox" if h == hosts.LOCAL else h)
    local = hosts.Session(hosts.LOCAL, "a", 1, False, 0, 100)
    out = rows.build(rows.State(), [(hosts.LOCAL, True, [local])], {})
    assert out[0]["host"] == hosts.LOCAL and out[0]["label"] == "archbox"


def test_rows_carry_claudes_pane():
    out = rows.build(rows.State(), [("box", True, [sess("a", pane="%7"), sess("b")])], {})
    assert [r["pane"] for r in out] == ["%7", None]


def test_background_rows_are_flagged_and_never_alert():
    state = rows.State()
    s = hosts.Session("box#web", "pm", 1, False, 0, 100)
    rows.build(state, [("box#web", True, [s])], {})
    s2 = hosts.Session("box#web", "pm", 1, False, 0, 1000)  # finished: would be "done"
    new = rows.build(state, [("box#web", True, [s2])], {})
    assert new[0]["background"] and new[0]["label"] == "box · web"
    assert rows.alerts({"box#web/pm": "running"}, new) == []
