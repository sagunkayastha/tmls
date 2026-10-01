import asyncio
import os

from textual.app import App

import pyte

from textual import events

from tmls.term import BRACKETED_PASTE, VT, Clipboard, Terminal, color, key_to_bytes, mouse_bytes


def test_special_keys():
    assert key_to_bytes("up", None) == b"\x1b[A"
    assert key_to_bytes("enter", "\r") == b"\r"
    assert key_to_bytes("backspace", "\x08") == b"\x7f"
    assert key_to_bytes("shift+tab", None) == b"\x1b[Z"
    assert key_to_bytes("ctrl+left", None) == b"\x1b[1;5D"


def test_characters_and_control_chars():
    assert key_to_bytes("a", "a") == b"a"
    assert key_to_bytes("ctrl+c", "\x03") == b"\x03"
    assert key_to_bytes("ctrl+c", None) == b"\x03"  # Textual may omit the character
    assert key_to_bytes("ctrl+a", None) == b"\x01"
    assert key_to_bytes("alt+b", None) == b"\x1bb"
    assert key_to_bytes("é", "é") == "é".encode()


def test_color_names():
    assert color("default") is None
    assert color("brown") == "yellow"
    assert color("brightred") == "bright_red"
    assert color("brightbrown") == "bright_yellow"
    assert color("ff00d7") == "#ff00d7"


class Host(App):
    def __init__(self, argv):
        super().__init__()
        self.argv = argv

    def compose(self):
        yield Terminal(self.argv, id="t")


async def wait_for(pilot, cond, timeout=5):
    for _ in range(int(timeout / 0.05)):
        await pilot.pause(0.05)
        if cond():
            return True
    return False


def screen_text(term):
    return "\n".join(term.vt.display)


async def test_renders_child_output():
    app = Host(["sh", "-c", "printf 'hello \\033[31mred\\033[0m'; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: "hello red" in screen_text(term))
        assert term.vt.buffer[0][6].fg == "red"


async def test_keys_reach_the_child_including_tab_and_ctrl_c():
    # tty echo shows typed keys; ctrl+c must interrupt the child rather than quit tmls
    app = Host(["sh", "-c", "trap 'echo GOT-INT' INT; stty -echoctl; while true; do sleep 0.1; done"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        term.focus()
        await wait_for(pilot, lambda: term.pid is not None)
        await pilot.press("x", "tab", "y")
        # the echoed tab moves the cursor to column 8
        assert await wait_for(pilot, lambda: term.vt.display[0].startswith("x       y"))
        await pilot.press("ctrl+c")
        assert await wait_for(pilot, lambda: "GOT-INT" in screen_text(term))
        assert app.is_running


async def test_child_gets_the_widget_size():
    app = Host(["sh", "-c", "stty size; sleep 5"])
    async with app.run_test(size=(70, 12)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: "12 70" in screen_text(term))


async def test_exit_is_shown():
    app = Host(["sh", "-c", "exit 0"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: term.exited)


async def test_quitting_hangs_up_the_child():
    # closing tmls must detach its tmux clients, not leave them running
    app = Host(["sleep", "30"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        await wait_for(pilot, lambda: term.pid is not None)
        pid = term.pid
    for _ in range(40):
        try:
            if os.waitpid(pid, os.WNOHANG)[0]:
                break
        except ChildProcessError:
            break
        await asyncio.sleep(0.05)
    else:
        raise AssertionError("child still running")


def test_status_queries_are_answered_not_crashed():
    # tmux asks the terminal for its cursor position and features; pyte 0.8.2 crashes on the
    # private (CSI ?) form, and replies must go back to the child or it may wait for them.
    replies = []
    vt = VT(80, 24, replies.append)
    stream = pyte.ByteStream(vt)
    stream.feed(b"\x1b[?6n")
    stream.feed(b"\x1b[3;5H\x1b[6n")
    assert replies[-1] == "\x1b[3;5R"


def test_device_attribute_queries_get_no_reply():
    # pyte answers tmux's secondary DA query (CSI > c) as if it were primary; tmux can't parse
    # it and the leftover "6c" lands in the pane as typed input. Stay silent instead.
    replies = []
    stream = pyte.ByteStream(VT(80, 24, replies.append))
    stream.feed(b"\x1b[>c\x1b[c")
    assert replies == []


def test_terminal_keeps_child_reverse_video_without_a_selection():
    term = Terminal(["true"])
    term.stream.feed(b"\x1b[7mREVERSED\x1b[0m")
    assert next(iter(term.render_line(0))).style.reverse is True


def test_mouse_bytes_sgr():
    # 1-based cells; M = press/drag, m = release; drag adds 32, wheel is 64/65; shift 4, alt 8, ctrl 16
    assert mouse_bytes(0, 0, 0, press=True) == b"\x1b[<0;1;1M"
    assert mouse_bytes(0, 9, 4, press=False) == b"\x1b[<0;10;5m"
    assert mouse_bytes(0, 2, 3, press=True, drag=True) == b"\x1b[<32;3;4M"
    assert mouse_bytes(64, 5, 5, press=True) == b"\x1b[<64;6;6M"
    assert mouse_bytes(2, 0, 0, press=True, meta=True, ctrl=True) == b"\x1b[<26;1;1M"


def test_clipboard_catches_osc52_even_when_split():
    clip = Clipboard()
    assert clip.feed(b"hi \x1b]52;c;aGVs") == []
    assert clip.feed(b"bG8=\x07 and \x1b]52;;d29ybGQ=\x1b\\ done") == ["hello", "world"]
    assert clip.feed(b"plain output") == []


async def test_mouse_goes_to_child_only_when_it_asks():
    # the child turns on SGR mouse reporting, then shows the raw bytes it receives
    script = "printf '\\033[?1000h\\033[?1006h'; stty raw -echo; dd bs=1 count=10 2>/dev/null | od -An -c; sleep 5"
    app = Host(["sh", "-c", script])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: term.mouse_mode)
        await pilot.click(term, offset=(2, 1))
        assert await wait_for(pilot, lambda: "<   0   ;   3   ;   2   M" in screen_text(term))


async def test_no_mouse_mode_sends_nothing():
    app = Host(["sh", "-c", "stty raw -echo; dd bs=1 count=1 2>/dev/null | od -An -c; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        await pilot.pause(0.3)
        await pilot.click(term, offset=(2, 1))
        await pilot.pause(0.3)
        assert "<" not in screen_text(term)


async def test_child_copy_reaches_the_clipboard():
    app = Host(["sh", "-c", "printf '\\033]52;c;Y29waWVk\\007'; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        assert await wait_for(pilot, lambda: app.clipboard == "copied")


async def test_paste_reaches_the_child_bracketed_when_it_asks():
    # kitty's ctrl+shift+v arrives as a Paste event; tmux turns on bracketed paste (?2004h)
    script = "printf '\\033[?2004h'; stty raw -echo; dd bs=1 count=14 2>/dev/null | od -An -c; sleep 5"
    app = Host(["sh", "-c", script])
    async with app.run_test(size=(80, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: BRACKETED_PASTE in term.vt.mode)
        term.post_message(events.Paste("hi"))
        assert await wait_for(pilot, lambda: "033   [   2   0   0   ~   h   i 033   [   2   0   1   ~"
                              in " ".join(screen_text(term).split("\n")))


async def test_plain_paste_without_bracketed_mode():
    app = Host(["sh", "-c", "stty raw -echo; dd bs=1 count=2 2>/dev/null | od -An -c; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        await wait_for(pilot, lambda: term.pid is not None)
        term.post_message(events.Paste("ok"))
        assert await wait_for(pilot, lambda: "o   k" in screen_text(term))


async def test_ctrl_drag_then_ctrl_c_copies_terminal_text_without_interrupting():
    app = Host(["sh", "-c", "printf 'hello world'; trap 'echo GOT-INT' INT; while true; do sleep 0.1; done"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        term.focus()
        assert await wait_for(pilot, lambda: "hello world" in screen_text(term))
        await pilot.mouse_down(term, offset=(0, 0), control=True)
        await pilot.mouse_up(term, offset=(5, 0), control=True)
        await pilot.press("ctrl+c")
        assert app.clipboard == "hello"
        assert "GOT-INT" not in screen_text(term)


async def test_ctrl_drag_copies_on_release_and_trims_trailing_spaces():
    app = Host(["sh", "-c", "printf 'done  '; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: "done" in screen_text(term))
        await pilot.mouse_down(term, offset=(0, 0), control=True)
        await pilot.mouse_up(term, offset=(6, 0), control=True)
        assert app.clipboard == "done"


async def test_tmux_osc52_copy_trims_trailing_spaces_on_each_line():
    app = Host(["sh", "-c", "printf '\\033]52;c;ZG9uZSAgCm5leHQgIA==\\007'; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        assert await wait_for(pilot, lambda: app.clipboard == "done\nnext")


async def test_right_click_pastes_system_clipboard_into_child(tmp_path, monkeypatch):
    paste = tmp_path / "wl-paste"
    paste.write_text("#!/bin/sh\nprintf PASTED\n")
    paste.chmod(0o755)
    monkeypatch.setenv("PATH", f"{tmp_path}:{os.environ['PATH']}")
    app = Host(["sh", "-c", "stty raw -echo; dd bs=1 count=6 2>/dev/null; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: term.pid is not None)
        await pilot.click(term, offset=(2, 0), button=3)
        assert await wait_for(pilot, lambda: "PASTED" in screen_text(term))


async def test_right_click_paste_is_bracketed_when_the_child_asks(tmp_path, monkeypatch):
    # unbracketed, a multi-line paste into a shell or Claude runs line by line
    paste = tmp_path / "wl-paste"
    paste.write_text("#!/bin/sh\nprintf hi\n")
    paste.chmod(0o755)
    monkeypatch.setenv("PATH", f"{tmp_path}:{os.environ['PATH']}")
    script = "printf '\\033[?2004h'; stty raw -echo; dd bs=1 count=14 2>/dev/null | od -An -c; sleep 5"
    app = Host(["sh", "-c", script])
    async with app.run_test(size=(80, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: BRACKETED_PASTE in term.vt.mode)
        await pilot.click(term, offset=(2, 0), button=3)
        assert await wait_for(pilot, lambda: "033   [   2   0   0   ~   h   i 033   [   2   0   1   ~"
                              in " ".join(screen_text(term).split("\n")))
