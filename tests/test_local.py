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
