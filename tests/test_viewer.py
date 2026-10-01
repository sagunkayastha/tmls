import os

import pytest

from tmls import hosts
from tmls import viewer


async def test_local_relative_file_uses_tmux_pane_cwd(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    (repo / "src").mkdir(parents=True)
    (repo / "src" / "term.py").write_text("first\nsecond\n")
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    tmux = bin_dir / "tmux"
    # answers only an exact target: "-t Season-36" would also match "Season-36b"
    tmux.write_text("#!/bin/sh\ncase \"$*\" in *'-t =Season-36: '*) printf '%s\\n' \"$TMLS_TEST_CWD\";; "
                    "*) exit 1;; esac\n")
    tmux.chmod(0o755)
    monkeypatch.setenv("PATH", f"{bin_dir}:{os.environ['PATH']}")
    monkeypatch.setenv("TMLS_TEST_CWD", str(repo))

    path, text = await viewer.load_file(hosts.LOCAL, "Season-36", "src/term.py")
    assert path == str(repo / "src" / "term.py")
    assert text == "first\nsecond\n"


async def test_remote_file_with_spaces_and_semicolon_uses_ssh_safely(tmp_path, monkeypatch):
    repo = tmp_path / "repo"
    repo.mkdir()
    name = "file name;touch INJECTED.py"
    (repo / name).write_text("remote text\n")
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    ssh = bin_dir / "ssh"
    ssh.write_text("#!/bin/sh\nwhile [ \"$1\" = -o ]; do shift 2; done\nshift\nTMLS_REMOTE=1 exec sh -c \"$1\"\n")
    ssh.chmod(0o755)
    tmux = bin_dir / "tmux"
    tmux.write_text("#!/bin/sh\n[ \"$TMLS_REMOTE\" = 1 ] || exit 22\nprintf '%s\\n' \"$TMLS_TEST_CWD\"\n")
    tmux.chmod(0o755)
    monkeypatch.setenv("PATH", f"{bin_dir}:{os.environ['PATH']}")
    monkeypatch.setenv("TMLS_TEST_CWD", str(repo))

    path, text = await viewer.load_file("archbox", "Season-36", name)
    assert path == str(repo / name)
    assert text == "remote text\n"
    assert not (repo / "INJECTED.py").exists()


async def test_absolute_local_file_opens_without_a_tmux_cwd_query(tmp_path):
    file = tmp_path / "term.py"
    file.write_text("line 1\n")
    assert await viewer.load_file(hosts.LOCAL, "no-such-session", str(file)) == (str(file), "line 1\n")


@pytest.mark.parametrize("content,reason", [
    pytest.param(b"text\0more", "binary file", id="binary"),
    pytest.param(b"x" * 2_000_001, "file too large", id="large"),
])
async def test_file_viewer_rejects_binary_and_oversized_files(tmp_path, content, reason):
    file = tmp_path / "file.dat"
    file.write_bytes(content)
    with pytest.raises(viewer.ViewerError, match=reason):
        await viewer.load_file(hosts.LOCAL, "no-such-session", str(file))


async def test_file_viewer_reports_missing_file(tmp_path):
    with pytest.raises(viewer.ViewerError, match="No such file"):
        await viewer.load_file(hosts.LOCAL, "no-such-session", str(tmp_path / "missing.py"))
