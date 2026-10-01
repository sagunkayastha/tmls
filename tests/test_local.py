import json

from tmls import hosts, local

KITTY_LS = json.dumps([{"is_focused": True, "tabs": [{"windows": [
    {"id": 1, "pid": 100, "is_focused": True, "foreground_processes": [{"pid": 101}]},
    {"id": 4, "pid": 200, "is_focused": False, "foreground_processes": [{"pid": 201}, {"pid": 202}]},
]}]}])


def test_parse_kitty_ls_maps_shell_and_foreground_pids():
    got = local.parse_kitty_ls(KITTY_LS, "/run/kitty-9")
    assert got[100] == got[101] == local.Window("/run/kitty-9", 1, True)
    assert got[202] == local.Window("/run/kitty-9", 4, False)


def test_find_window_walks_up_the_process_tree():
    windows = local.parse_kitty_ls(KITTY_LS, "s")
    parents = {500: 400, 400: 200, 200: 1}
    assert local.find_window(500, parents, windows).id == 4
    assert local.find_window(999, parents, windows) is None


def test_to_sessions_keeps_interactive_claude_outside_tmux():
    files = [
        {"pid": 500, "name": "tmls", "kind": "interactive", "status": "busy", "statusUpdatedAt": 90000, "tmux": None},
        {"pid": 600, "name": "tmls", "kind": "interactive", "status": "idle", "statusUpdatedAt": 80000},
        {"pid": 700, "name": "in-tmux", "kind": "interactive", "status": "idle", "tmux": "work:@1.%1"},
        {"pid": 800, "name": "cron", "kind": "bg", "status": "busy"},
        {"pid": 900, "cwd": "/home/me/proj", "kind": "interactive", "status": "idle", "statusUpdatedAt": 1000},
    ]
    windows = local.parse_kitty_ls(KITTY_LS, "s")
    got = local.to_sessions(files, windows, {500: 200, 600: 101}, now=100)
    assert [(s.host, s.name, s.claude, s.claude_since, s.now) for s in got] == [
        (hosts.KITTY, "tmls", "busy", 90, 100),
        (hosts.KITTY, "tmls (600)", "idle", 80, 100),  # same name twice: pid tells them apart
        (hosts.KITTY, "proj", "idle", 1, 100),         # unnamed: folder name
    ]
    assert [s.kitty.id if s.kitty else None for s in got] == [4, 1, None]


def test_to_sessions_carries_waiting_reason_and_failed():
    files = [{"pid": 1, "name": "a", "kind": "interactive", "status": "waiting", "waitingFor": "input needed"},
             {"pid": 2, "name": "b", "kind": "interactive", "status": "idle", "failed": True,
              "model": "claude-opus-5-5", "context": 5000}]
    got = local.to_sessions(files, {}, {}, now=100)
    assert [(s.waiting, s.failed, s.model, s.context) for s in got] == [
        ("input needed", False, None, 0), (None, True, "claude-opus-5-5", 5000)]


def test_last_reply_reads_failure_and_context_from_the_newest_assistant_lines(tmp_path):
    t = tmp_path / "s.jsonl"
    dumps = lambda d: json.dumps(d, separators=(",", ":"))  # compact, like Claude's transcripts
    usage = {"input_tokens": 2, "cache_creation_input_tokens": 3000, "cache_read_input_tokens": 581674}
    ok = dumps({"type": "assistant", "message": {"model": "claude-opus-5-5", "usage": usage,
                                                 "content": 'said "isApiErrorMessage":true'}})
    err = dumps({"type": "assistant", "isApiErrorMessage": True, "message": {"model": "<synthetic>"}})
    user = dumps({"type": "user", "message": {"content": "retry"}})
    t.write_text(f"{err}\n{ok}\n{user}\n")
    # an old error, and a reply that only quotes the field
    assert local.last_reply(t) == {"failed": False, "model": "claude-opus-5-5", "context": 584676}
    t.write_text(f"{ok}\n{err}\n{user}\n")
    assert local.last_reply(t) == {"failed": True, "model": "claude-opus-5-5", "context": 584676}
    assert local.last_reply(tmp_path / "missing.jsonl") == {}
