from tmls import hosts


def test_read_config_skips_blanks_and_comments(tmp_path):
    f = tmp_path / "hosts"
    f.write_text("# my boxes\nnas\n\nlaptop  # sometimes off\n")
    assert hosts.read_config(f) == ["nas", "laptop"]
    assert hosts.read_config(tmp_path / "missing") == []


def test_hosts_drops_this_machine(monkeypatch):
    monkeypatch.setattr(hosts.socket, "gethostname", lambda: "me")
    monkeypatch.setattr(hosts.shutil, "which", lambda _: None)
    assert hosts.hosts(["nas", "me"]) == ["nas"]


def test_parse_sessions():
    # first line is the host's clock; then one line per window (tmux 3.7 prints tabs in -F output as "_",
    # so fields are ":"-separated; ":" is banned in session names); newest window output wins.
    out = "1000\nwork:2:1:900\nwork:2:1:990\nmy notes:1:0:400\n"
    got = hosts.parse("nas", out)
    assert [(s.host, s.name, s.windows, s.activity, s.now, s.claude) for s in got] == [
        ("nas", "work", 2, 990, 1000, None),
        ("nas", "my notes", 1, 400, 1000, None),
    ]


def test_parse_claude_status_by_tmux_session():
    # after "---": Claude Code's own ~/.claude/sessions/<pid>.json of live pids, one per line
    out = ("1000\nwork:1:0:990\nnotes:1:0:400\nplain:1:0:10\n---\n"
           '{"status":"idle","statusUpdatedAt":500000,"tmux":"notes:@2.%2"}\n'
           '{"status":"busy","statusUpdatedAt":900000,"tmux":"work:@1.%1"}\n'
           '{"status":"idle","statusUpdatedAt":950000,"tmux":"work:@3.%3"}\n'
           '{"status":"idle","statusUpdatedAt":1,"tmux":null}\n')
    got = {s.name: (s.claude, s.claude_since) for s in hosts.parse("nas", out)}
    assert got == {"work": ("busy", 900), "notes": ("idle", 500), "plain": (None, 0)}


def test_list_argv_reads_host_clock_windows_and_claude():
    script = hosts.list_argv("nas")[-1]
    assert hosts.list_argv(hosts.LOCAL)[:2] == ["sh", "-c"]
    assert "date +%s" in script and "list-windows -a" in script and ".claude/sessions/" in script
    assert ".key" not in script


def S(activity=0, now=100, claude=None, since=0):
    return hosts.Session("nas", "x", 1, False, activity, now, claude, since)


def test_status_from_claude():
    assert hosts.status(S(claude="busy"), seen=0, started=0) == "running"
    assert hosts.status(S(claude="idle", since=50), seen=0, started=0) == "done"
    assert hosts.status(S(claude="shell", since=50), seen=0, started=0) == "running"  # monitor/bg shell still going
    assert hosts.status(S(claude="idle", since=50), seen=60, started=0) == "idle"   # seen after it finished
    assert hosts.status(S(claude="idle", since=50), seen=0, started=70) == "idle"   # finished before tmls
    assert hosts.status(S(activity=99, claude="idle", since=50), seen=60, started=0) == "idle"  # redraws aren't work


def test_status_from_output_without_claude():
    assert hosts.status(S(activity=95), seen=0, started=0) == "running"   # output 5s ago
    assert hosts.status(S(activity=50), seen=0, started=0) == "done"      # quiet 50s, never seen
    assert hosts.status(S(activity=50), seen=60, started=0) == "idle"
    assert hosts.status(S(activity=10), seen=0, started=20) == "idle"


def test_local_commands_skip_ssh():
    assert hosts.attach_argv(hosts.LOCAL, "work") == ["tmux", "-u", "attach", "-t", "work"]
    

def test_remote_commands_quote_names():
    argv = hosts.attach_argv("nas", "my notes")
    assert argv == ["ssh", "-t", "nas", "tmux -u attach -t 'my notes'"]
    assert hosts.list_argv("nas")[:4] == ["ssh", "-o", "BatchMode=yes", "-o"]


def test_copy_command_is_shell_ready():
    assert hosts.attach_command("nas", "my notes") == "ssh -t nas \"tmux -u attach -t 'my notes'\""
