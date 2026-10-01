import json
import socket
import threading

from tmls import ag, hosts

CLAUDES = {
    "archbox": [{"pid": 1, "name": "bloom-be", "status": "idle", "tmux": "ph_forecast:@1.%1",
                 "messagingSocketPath": "/run/user/1000/cc-socks/1.sock"},
                {"pid": 2, "name": "Cron job Azure", "status": "busy", "tmux": None,
                 "messagingSocketPath": "/run/user/1000/cc-socks/2.sock"}],
    hosts.LOCAL: [{"pid": 3, "name": "tmls", "status": "busy", "messagingSocketPath": "/s/3.sock"}],
}
TMUX = {"archbox": ["ph_forecast", "Season-36"], hosts.LOCAL: []}


def test_resolve_prefers_claude_names_then_tmux_sessions_then_codex():
    assert ag.resolve("bloom-be", CLAUDES, TMUX) == ("claude", "archbox", CLAUDES["archbox"][0])
    assert ag.resolve("Season-36", CLAUDES, TMUX) == ("tmux", "archbox", "Season-36")
    assert ag.resolve("tmls-codex", CLAUDES, TMUX) == ("codex", None, "tmls-codex")
    assert ag.resolve("archbox:Season-36", CLAUDES, TMUX) == ("tmux", "archbox", "Season-36")


def test_same_claude_name_on_two_hosts_needs_the_host():
    both = {**CLAUDES, "nas": [dict(CLAUDES[hosts.LOCAL][0])]}
    try:
        ag.resolve("tmls", both, TMUX)
    except ag.Ambiguous as e:
        assert "local:tmls" in str(e) and "nas:tmls" in str(e)
    else:
        raise AssertionError("expected Ambiguous")


def test_message_line_attests_only_what_it_is_told():
    line = json.loads(ag.message_line("hi <there>", sender="tmls", mode="bypass"))
    assert line["type"] == "user"
    assert line["message"]["content"] == ('<cross-session-message from-name="tmls" from-mode="bypass">\n'
                                          'hi <there>\n</cross-session-message>')
    plain = json.loads(ag.message_line("hi", sender=None, mode=None))
    assert plain["message"]["content"] == "hi"  # unattested: the receiver may hold it for approval


def test_socket_send_writes_one_json_line(tmp_path):
    path = str(tmp_path / "s.sock")
    got = []
    srv = socket.socket(socket.AF_UNIX)
    srv.bind(path)
    srv.listen(1)

    def accept():
        conn, _ = srv.accept()
        got.append(conn.makefile().readline())
        conn.close()
    t = threading.Thread(target=accept)
    t.start()
    import subprocess
    subprocess.run(ag.socket_argv(hosts.LOCAL, path), input=b'{"type":"user"}\n', check=True)
    t.join(5)
    assert got == ['{"type":"user"}\n']


def test_remote_commands_quote_their_arguments():
    argv = ag.socket_argv("archbox", "/run/x y.sock")
    assert argv[:3] == ["ssh", "-o", "BatchMode=yes"] and "'/run/x y.sock'" in argv[-1]
    assert ag.codex_argv("archbox", "my thread", "a; b")[-1].endswith("--thread 'my thread' --message 'a; b'")
    assert ag.read_argv(hosts.LOCAL, "Season-36", 40)[-1] == "tmux capture-pane -p -t =Season-36: -S -40"


def test_survey_lists_claude_even_without_a_tmux_server():
    script = ag.survey_argv(hosts.LOCAL)[-1]
    assert "list-windows" in script and "2>/dev/null;" in script and "&&" not in script.split("---")[0]


def test_this_machine_by_its_hostname(monkeypatch):
    monkeypatch.setattr(hosts.socket, "gethostname", lambda: "ubu")
    assert ag.resolve("ubu:tmls", CLAUDES, TMUX)[:2] == ("claude", hosts.LOCAL)
