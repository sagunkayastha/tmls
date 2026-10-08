import asyncio
import os
from types import SimpleNamespace
from unittest.mock import PropertyMock, patch

from textual.app import App

import pyte

from textual import events
from textual.geometry import Region, Size

from tmls import term as term_module
from tmls.term import BRACKETED_PASTE, VT, Clipboard, Terminal, color, key_to_bytes, mouse_bytes


def redraw_probe():
    term = Terminal(["true"])
    calls = []
    term.refresh = lambda region=None, **kwargs: calls.append(region)
    term.vt.dirty.clear()
    term._dirty = False
    return term, calls


def test_flush_refreshes_only_changed_line():
    term, calls = redraw_probe()
    term._cursor_row = 3
    term.stream.feed(b"\x1b[4;1Hchanged")
    term._dirty = True
    with patch.object(Terminal, "size", new_callable=PropertyMock, return_value=Size(80, 24)):
        term._flush()
    assert calls == [Region(0, 3, 80, 1)]
    assert not term.vt.dirty


def test_flush_refreshes_old_and_new_cursor_rows():
    term, calls = redraw_probe()
    term._cursor_row = 0
    term.stream.feed(b"\x1b[4;1H")
    term._dirty = True
    with patch.object(Terminal, "size", new_callable=PropertyMock, return_value=Size(80, 24)):
        term._flush()
    assert calls == [Region(0, 0, 80, 1), Region(0, 3, 80, 1)]


def test_pyte_marks_scroll_index_and_erase_dirty():
    term, _ = redraw_probe()
    for output in (b"\x1b[24;1H\n", b"\x1b[1;1H\x1bM", b"\x1b[2J"):
        term.vt.dirty.clear()
        term.stream.feed(output)
        assert term.vt.dirty == set(range(term.vt.lines))


def test_alternate_screen_switch_forces_full_refresh():
    term, calls = redraw_probe()
    with patch.object(Terminal, "size", new_callable=PropertyMock, return_value=Size(80, 24)):
        for sequence in (b"\x1b[?1049h", b"\x1b[?1049l", b"\x1b[?47h", b"\x1b[?47l"):
            term.stream.feed(sequence)
            term._dirty = True
            term._flush()
    assert calls == [None] * 4


async def test_selection_and_quick_select_changes_refresh_all_rows():
    term, calls = redraw_probe()
    term._selection = ((0, 0), (0, 0))
    term._selecting = True
    term.on_mouse_move(SimpleNamespace(x=5, y=2, stop=lambda: None))
    assert calls == [None]
    calls.clear()
    term.quick_hits = [SimpleNamespace(label="a")]
    await term._on_key(events.Key("escape", None))
    assert term.quick_hits == []
    assert calls == [None]


def test_resize_and_focus_changes_refresh_all_rows():
    term, calls = redraw_probe()
    term._spawn = lambda rows, cols: None
    term.on_resize(SimpleNamespace(size=Size(90, 30)))
    term.on_focus()
    term.on_blur()
    assert calls == [None, None, None]
    assert (term.vt.columns, term.vt.lines) == (90, 30)


def test_special_keys():
    assert key_to_bytes("up", None) == b"\x1b[A"
    assert key_to_bytes("enter", "\r") == b"\r"
    assert key_to_bytes("backspace", "\x08") == b"\x7f"
    assert key_to_bytes("shift+backspace", None) == b"\x7f"  # kitty sends it as its own key
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
    def __init__(self, argv, host="local"):
        super().__init__()
        self.argv = argv
        self.host = host
        self.links = []

    def compose(self):
        yield Terminal(self.argv, host=self.host, id="t")

    def on_terminal_link_clicked(self, event):
        self.links.append((event.kind, event.target, event.line))


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


def test_link_at_url_keeps_balanced_parens_and_trims_sentence_punctuation():
    row = "See (https://example.com/a(b))., next"
    assert term_module.link_at([row], row.index("example"), 0) == ("url", "https://example.com/a(b)", None)
    assert term_module.link_at([row], row.index("next"), 0) is None


def test_link_at_file_line_with_column_and_no_false_port_or_time():
    row = "src/tmls/term.py:214:5  10:30  host:22"
    assert term_module.link_at([row], row.index("term.py") + 2, 0) == ("file", "src/tmls/term.py", 214)
    assert term_module.link_at([row], row.index("10:30") + 1, 0) is None
    assert term_module.link_at([row], row.index("host:22") + 1, 0) is None


def test_link_at_wrapped_url_and_ignores_file_shape_inside_url():
    rows = ["go https://example.com/src/term.py:2", "14 now"]
    assert term_module.link_at(rows, rows[0].index("example"), 0) == (
        "url", "https://example.com/src/term.py:214", None)
    assert term_module.link_at(["https://example.com/file.py:9"], 22, 0)[0] == "url"


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


async def test_ctrl_click_url_posts_link_without_copying():
    app = Host(["sh", "-c", "printf 'https://example.com'; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: "https://example.com" in screen_text(term))
        await pilot.click(term, offset=(10, 0), control=True)
        assert app.links == [("url", "https://example.com", None)]
        assert app.clipboard == ""


async def test_ctrl_drag_over_url_remains_a_selection():
    app = Host(["sh", "-c", "printf 'https://example.com'; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: "https://example.com" in screen_text(term))
        await pilot.mouse_down(term, offset=(0, 0), control=True)
        await pilot.mouse_up(term, offset=(5, 0), control=True)
        assert app.links == []
        assert app.clipboard == "https"


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


async def test_drop_of_local_image_into_remote_terminal_copies_and_pastes_remote_path(tmp_path, monkeypatch):
    from tmls import image_paste
    image = tmp_path / "my photo.jpg"
    image.write_bytes(b"jpeg bytes")
    copied = []

    async def store(host, data, suffix):
        copied.append((host, data, suffix))
        return "~/.cache/tmls/images/remote.jpg"

    monkeypatch.setattr(image_paste, "store", store)
    app = Host(["sleep", "5"], host="archbox")
    async with app.run_test(size=(80, 10)) as pilot:
        term = app.query_one(Terminal)
        pasted = []
        monkeypatch.setattr(term, "_paste", pasted.append)
        term.post_message(events.Paste(image.as_uri()))
        assert await wait_for(pilot, lambda: pasted)
        assert copied == [("archbox", b"jpeg bytes", ".jpg")]
        assert pasted == ["~/.cache/tmls/images/remote.jpg"]


async def test_image_clipboard_pastes_path_without_enter(tmp_path, monkeypatch):
    from tmls import image_paste
    calls = []

    async def clipboard_image():
        return b"\x89PNG\r\n\x1a\nimage", ".png"

    async def store(host, data, suffix):
        calls.append((host, data, suffix))
        return "~/.cache/tmls/images/capture.png"

    monkeypatch.setattr(image_paste, "clipboard_image", clipboard_image)
    monkeypatch.setattr(image_paste, "store", store)
    app = Host(["sleep", "5"], host="archbox")
    async with app.run_test(size=(80, 10)) as pilot:
        term = app.query_one(Terminal)
        pasted = []
        monkeypatch.setattr(term, "_paste", pasted.append)
        await pilot.click(term, offset=(2, 0), button=3)
        assert await wait_for(pilot, lambda: pasted)
        assert calls == [("archbox", b"\x89PNG\r\n\x1a\nimage", ".png")]
        assert pasted == ["~/.cache/tmls/images/capture.png"]


async def test_failed_image_copy_toasts_and_types_nothing(monkeypatch):
    from tmls import image_paste

    async def clipboard_image():
        return b"PNG", ".png"

    async def store(host, data, suffix):
        raise image_paste.ImageError("ssh failed")

    monkeypatch.setattr(image_paste, "clipboard_image", clipboard_image)
    monkeypatch.setattr(image_paste, "store", store)
    app = Host(["sleep", "5"], host="archbox")
    async with app.run_test(size=(80, 10)) as pilot:
        term = app.query_one(Terminal)
        pasted = []
        monkeypatch.setattr(term, "_paste", pasted.append)
        await pilot.click(term, offset=(2, 0), button=3)
        await pilot.pause(0.2)
        assert pasted == []


async def test_x11_text_clipboard_uses_xclip_even_if_wl_paste_is_installed(tmp_path, monkeypatch):
    for name, output in (("wl-paste", "WRONG"), ("xclip", "RIGHT")):
        command = tmp_path / name
        command.write_text(f"#!/bin/sh\nprintf {output}\n")
        command.chmod(0o755)
    monkeypatch.setenv("PATH", f"{tmp_path}:{os.environ['PATH']}")
    monkeypatch.delenv("WAYLAND_DISPLAY", raising=False)
    monkeypatch.setenv("DISPLAY", ":0")
    app = Host(["sh", "-c", "stty raw -echo; dd bs=1 count=5 2>/dev/null; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: term.pid is not None)
        await pilot.click(term, offset=(2, 0), button=3)
        assert await wait_for(pilot, lambda: "RIGHT" in screen_text(term))


async def test_text_paste_event_keeps_its_text_when_clipboard_also_has_an_image(monkeypatch):
    from tmls import image_paste

    async def clipboard_image():
        return b"PNG"

    monkeypatch.setattr(image_paste, "clipboard_image", clipboard_image)
    app = Host(["sh", "-c", "stty raw -echo; dd bs=1 count=2 2>/dev/null; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        assert await wait_for(pilot, lambda: term.pid is not None)
        term.post_message(events.Paste("ok"))
        assert await wait_for(pilot, lambda: "ok" in screen_text(term))


async def test_right_click_without_clipboard_tools_uses_textual_clipboard(monkeypatch):
    monkeypatch.setattr(term_module.shutil, "which", lambda name: None)
    app = Host(["sh", "-c", "stty raw -echo; dd bs=1 count=8 2>/dev/null; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        app.copy_to_clipboard("FALLBACK")
        assert await wait_for(pilot, lambda: term.pid is not None)
        await pilot.click(term, offset=(2, 0), button=3)
        assert await wait_for(pilot, lambda: "FALLBACK" in screen_text(term))


async def test_right_click_with_failing_wl_paste_uses_textual_clipboard(tmp_path, monkeypatch):
    paste = tmp_path / "wl-paste"
    paste.write_text("#!/bin/sh\nexit 1\n")
    paste.chmod(0o755)
    monkeypatch.setenv("PATH", f"{tmp_path}:{os.environ['PATH']}")
    monkeypatch.setenv("WAYLAND_DISPLAY", "wayland-0")
    app = Host(["sh", "-c", "stty raw -echo; dd bs=1 count=8 2>/dev/null; sleep 5"])
    async with app.run_test(size=(60, 10)) as pilot:
        term = app.query_one(Terminal)
        app.copy_to_clipboard("FALLBACK")
        assert await wait_for(pilot, lambda: term.pid is not None)
        await pilot.click(term, offset=(2, 0), button=3)
        assert await wait_for(pilot, lambda: "FALLBACK" in screen_text(term))


def test_link_at_bare_absolute_and_home_paths_without_a_line():
    row = "Saved `/data/scratch/mock/bar left.png` and /data/scratch/mock/bar.png. See ~/notes/a.md, ok"
    at = lambda text: term_module.link_at([row], row.index(text) + 2, 0)
    assert at("/data/scratch/mock/bar.png") == ("file", "/data/scratch/mock/bar.png", None)
    assert at("~/notes") == ("file", "~/notes/a.md", None)
    # backticks and sentence punctuation are not part of the path
    assert at("/data/scratch/mock/bar left") == ("file", "/data/scratch/mock/bar", None)


def test_link_at_bare_paths_skip_words_with_slashes_and_lone_slash():
    row = "and/or 1/2 / x"
    for word in ("and/or", "1/2", "/ x"):
        assert term_module.link_at([row], row.index(word) + 1, 0) is None


def test_link_at_prefers_file_line_and_urls_over_bare_paths():
    row = "/home/u/src/term.py:214 https://example.com/data/x.png"
    assert term_module.link_at([row], 3, 0) == ("file", "/home/u/src/term.py", 214)
    assert term_module.link_at([row], row.index("data"), 0)[0] == "url"
