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
    # tmux 3.7 prints tabs in -F output as "_", so fields are ":"-separated (":" is banned in session names)
    out = "work:2:1:1790720000\nmy notes:1:0:1790710000\n"
    got = hosts.parse("nas", out)
    assert [(s.host, s.name, s.windows, s.attached) for s in got] == [
        ("nas", "work", 2, True),
        ("nas", "my notes", 1, False),
    ]


def test_local_commands_skip_ssh():
    assert hosts.attach_argv(hosts.LOCAL, "work") == ["tmux", "attach", "-t", "work"]
    assert hosts.list_argv(hosts.LOCAL)[:2] == ["tmux", "ls"]


def test_remote_commands_quote_names():
    argv = hosts.attach_argv("nas", "my notes")
    assert argv == ["ssh", "-t", "nas", "tmux attach -t 'my notes'"]
    assert hosts.list_argv("nas")[:4] == ["ssh", "-o", "BatchMode=yes", "-o"]


def test_copy_command_is_shell_ready():
    assert hosts.attach_command("nas", "my notes") == "ssh -t nas \"tmux attach -t 'my notes'\""
