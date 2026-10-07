import shlex
import subprocess

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


def test_parse_waiting_reason_and_failed_reply():
    # "failed" after a session file: its transcript's last reply is an API error
    out = ("1000\nask:1:0:990\nbroke:1:0:990\nboth:1:0:990\n---\n"
           '{"status":"waiting","waitingFor":"permission prompt","statusUpdatedAt":900000,"tmux":"ask:@1.%1"}\n'
           '{"status":"idle","statusUpdatedAt":900000,"tmux":"broke:@2.%2"}\nfailed\n'
           '{"status":"idle","statusUpdatedAt":950000,"tmux":"both:@3.%3"}\nfailed\n'
           '{"status":"waiting","waitingFor":"input needed","statusUpdatedAt":940000,"tmux":"both:@4.%4"}\n')
    got = {s.name: (s.claude, s.waiting, s.failed) for s in hosts.parse("nas", out)}
    assert got == {"ask": ("waiting", "permission prompt", False), "broke": ("idle", None, True),
                   "both": ("waiting", "input needed", False)}  # waiting on you beats the rest


def test_parse_claude_conversation_title():
    out = ("1000\nSeason-36:1:0:990\nplain:1:0:10\n---\n"
           '{"name":"NERSC_Training","status":"idle","statusUpdatedAt":900000,"tmux":"Season-36:@1.%1"}\n')
    assert {s.name: s.title for s in hosts.parse("nas", out)} == {"Season-36": "NERSC_Training", "plain": None}


def test_parse_context_use_from_the_newest_reply():
    # "usage" after a session file: model and token counts of its newest real reply
    out = ("1000\nwork:1:0:990\n---\n"
           '{"status":"idle","statusUpdatedAt":900000,"tmux":"work:@1.%1"}\n'
           'usage "model":"claude-opus-5-5" "input_tokens":2 "cache_creation_input_tokens":3000 '
           '"cache_read_input_tokens":581674\n')
    s, = hosts.parse("nas", out)
    assert (s.model, s.context) == ("claude-opus-5-5", 584676)
    assert hosts.context_pct(s) == 58


def test_context_limit_by_model():
    pct = lambda model, used: hosts.context_pct(hosts.Session("h", "x", 1, False, 0, 0, model=model, context=used))
    assert pct("claude-sonnet-5-5", 900_000) == 90       # Claude 5 models run with 1M
    assert pct("claude-haiku-4-5-20251001", 150_000) == 75  # older ones 200k
    assert pct(None, 0) is None                             # no reply yet, or not Claude


def test_list_argv_reads_host_clock_windows_and_claude():
    script = hosts.list_argv("nas")[-1]
    assert hosts.list_argv(hosts.LOCAL)[:2] == ["sh", "-c"]
    assert "date +%s" in script and "list-windows -a" in script and ".claude/sessions/" in script
    assert ".key" not in script
    assert '"isApiErrorMessage":true' in script and ".claude/projects/" in script and "echo usage" in script


def S(activity=0, now=100, claude=None, since=0, failed=False):
    return hosts.Session("nas", "x", 1, False, activity, now, claude, since, failed=failed)


def test_status_waiting_and_failed_stay_until_they_change():
    assert hosts.status(S(claude="waiting", since=50), seen=90, started=0) == "waiting"  # looking doesn't clear it
    assert hosts.status(S(claude="idle", since=50, failed=True), seen=90, started=0) == "failed"
    assert hosts.status(S(claude="busy", failed=True), seen=0, started=0) == "running"  # retrying


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
    assert hosts.attach_argv(hosts.LOCAL, "work") == ["tmux", "-u", "attach", "-t", "=work"]


def test_remote_commands_quote_names():
    argv = hosts.attach_argv("nas", "my notes")
    assert argv == ["ssh", "-t", "nas", "tmux -u attach -t '=my notes'"]
    assert hosts.list_argv("nas")[:4] == ["ssh", "-o", "BatchMode=yes", "-o"]


def test_copy_command_is_shell_ready():
    assert hosts.attach_command("nas", "work") == "ssh -t nas 'tmux -u attach -t =work'"


def test_attach_command_is_shell_safe():
    cmd = hosts.attach_command("nas", 'x"; touch /tmp/pwned; "')
    assert shlex.split(cmd) == hosts.attach_argv("nas", 'x"; touch /tmp/pwned; "')
    assert "$" not in hosts.attach_command("nas", "cost$5").replace("'=cost$5'", "")


def test_pasted_copy_command_reaches_ssh_unchanged(tmp_path):
    # a real shell, with ssh replaced by a function that prints its arguments
    for name in ['x"; touch pwned; "', "cost$5", "it's `w` $(x)", "my notes"]:
        cmd = hosts.attach_command("nas", name)
        out = subprocess.run(["sh", "-c", "ssh() { printf '%s\\n' \"$@\"; }; " + cmd],
                             capture_output=True, text=True, cwd=tmp_path).stdout
        assert out.splitlines() == hosts.attach_argv("nas", name)[1:]
    assert not (tmp_path / "pwned").exists()


def test_attach_targets_exact_name():
    assert hosts.attach_argv(hosts.LOCAL, "work")[-1] == "=work"


def test_listing_forces_utf8():
    assert "tmux -u list-windows" in hosts.list_argv(hosts.LOCAL)[-1]


def test_parse_skips_lines_that_are_not_windows():
    out = "Welcome to the box\n42\n1700000000\nwork:1:0:1699999990\nmotd line\n---\n"
    sessions = hosts.parse("box", out)
    assert [s.name for s in sessions] == ["work"]
    assert sessions[0].now == 1700000000  # a banner's number isn't the clock


async def test_list_host_offline_when_output_is_garbage(monkeypatch):
    class Proc:
        returncode = 0

        async def communicate(self):
            return b"garbage\n", b""

    async def fake_exec(*argv, **kw):
        return Proc()
    monkeypatch.setattr(hosts.asyncio, "create_subprocess_exec", fake_exec)
    online, sessions = await hosts.list_host("box")
    assert (online, sessions) == (False, [])


async def test_list_host_online_without_a_tmux_server(monkeypatch):
    for err in (b"no server running on /tmp/tmux-1000/default\n",
                b"error connecting to /tmp/tmux-1000/default (No such file or directory)\n"):
        class Proc:
            returncode = 1

            async def communicate(self, err=err):
                return b"", err

        async def fake_exec(*argv, **kw):
            return Proc()
        monkeypatch.setattr(hosts.asyncio, "create_subprocess_exec", fake_exec)
        assert await hosts.list_host("box") == (True, [])


async def test_list_host_offline_when_ssh_fails(monkeypatch):
    class Proc:
        returncode = 255

        async def communicate(self):
            return b"", b"ssh: connect to host box port 22: Connection refused\n"

    async def fake_exec(*argv, **kw):
        return Proc()
    monkeypatch.setattr(hosts.asyncio, "create_subprocess_exec", fake_exec)
    assert await hosts.list_host("box") == (False, [])


def test_half_written_status_file_keeps_its_failed_and_usage_lines():
    # B's file was caught mid-write; the "failed" and "usage" after it are B's, not A's
    out = ("1000\nA:1:0:990\nB:1:0:990\n---\n"
           '{"status":"idle","statusUpdatedAt":900000,"tmux":"A:@1.%1"}\n'
           '{"status":"idle","statusUpdatedAt":9\n'
           'failed\n'
           'usage "model":"claude-haiku-4-5" "input_tokens":190000\n')
    a = {s.name: s for s in hosts.parse("nas", out)}["A"]
    assert (a.failed, a.model, a.context) == (False, None, 0)


def test_parse_records_claudes_own_pane():
    out = ("1000\nBudget:1:0:990\nnone:1:0:990\nodd:1:0:990\nplain:1:0:10\n---\n"
           '{"status":"idle","statusUpdatedAt":900000,"tmux":"Budget:@7.%7"}\n'
           '{"status":"idle","statusUpdatedAt":900000,"tmux":"none"}\n'
           '{"status":"idle","statusUpdatedAt":900000,"tmux":"odd:@1.1"}\n')
    got = {s.name: s.pane for s in hosts.parse("nas", out)}
    assert got == {"Budget": "%7", "none": None, "odd": None, "plain": None}


def test_blank_line_after_a_status_file_keeps_its_failed_line():
    out = '1000\nA:1:0:990\n---\n{"status":"idle","statusUpdatedAt":900000,"tmux":"A:@1.%1"}\n\nfailed\n'
    assert hosts.parse("nas", out)[0].failed is True


def test_background_server_hosts():
    # other tmux servers (`tmux -L web`) are hosts of their own, "MACHINE#SERVER"
    assert hosts.split("archbox#web") == ("archbox", "web")
    assert hosts.split("archbox") == ("archbox", None)
    assert hosts.is_background("local#web") and not hosts.is_background("user@nas")
    assert hosts.label("nas#web") == "nas · web"


def test_parse_sockets_skips_default_and_odd_names():
    out = "default\nweb\n\nmy server\nlab-2.x\n"
    assert hosts.parse_sockets("nas", out) == ["nas#web", "nas#lab-2.x"]


def test_background_attach_and_commands_name_the_server():
    assert hosts.attach_argv("local#web", "pm") == ["tmux", "-u", "-L", "web", "attach", "-t", "=pm"]
    assert hosts.attach_argv("nas#web", "pm") == ["ssh", "-t", "nas", "tmux -u -L web attach -t =pm"]
    argv = hosts.run_argv("nas#web", "tmux ls")
    assert argv[:-1] == ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "nas"]
    assert argv[-1] == 'tmux() { command tmux -L web "$@"; }; tmux ls'
    assert hosts.run_argv("local", "tmux ls") == ["sh", "-c", "tmux ls"]


def test_background_function_reaches_the_named_server(tmp_path):
    # the shell function really routes a script's `tmux` to the -L server (a stub tmux records its args)
    stub = tmp_path / "tmux"
    stub.write_text('#!/bin/sh\necho "$@"\n')
    stub.chmod(0o755)
    argv = hosts.run_argv("local#web", "tmux list-sessions")
    out = subprocess.run(argv, capture_output=True, text=True, env={"PATH": f"{tmp_path}:/usr/bin:/bin"}).stdout
    assert out.strip() == "-L web list-sessions"


def test_background_list_skips_claude_files():
    script = hosts.list_argv("local#web")[-1]
    assert "-L web" in script and ".claude/sessions" not in script


def test_allowed_hosts():
    assert hosts.allowed("nas", ["nas"]) and hosts.allowed("nas#web", ["nas"])
    assert hosts.allowed("local#web", []) and not hosts.allowed("other#web", ["nas"])
    assert not hosts.allowed("nas#we b", ["nas"]) and not hosts.allowed("nas#a;rm", ["nas"])
