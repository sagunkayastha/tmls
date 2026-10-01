from tmls.quickselect import candidates


def test_hints_find_visible_urls_files_paths_and_hashes_in_screen_order():
    rows = [
        "https://example.com src/tmls/app.py:40",
        "README.md /tmp/output.log 85388e9",
        "https://example.com/85388e9",
    ]
    hits = candidates(rows)
    assert [(h.label, h.kind, h.target, h.line, h.y, h.x) for h in hits] == [
        ("a", "url", "https://example.com", None, 0, 0),
        ("b", "file", "src/tmls/app.py", 40, 0, 20),
        ("c", "file", "README.md", 1, 1, 0),
        ("d", "file", "/tmp/output.log", 1, 1, 10),
        ("e", "hash", "85388e9", None, 1, 26),
        ("f", "url", "https://example.com/85388e9", None, 2, 0),
    ]


def test_hints_stop_at_26_and_skip_non_links():
    rows = ["10:30 host:22", " ".join(f"file{n}.py" for n in range(30))]
    hits = candidates(rows)
    assert len(hits) == 26
    assert hits[0].target == "file0.py"
    assert hits[-1].label == "z" and hits[-1].target == "file25.py"


def test_copy_keeps_an_explicit_line_one_but_not_a_plain_path():
    hits = candidates(["README.md:1 README.md"])
    assert [hit.copy_text for hit in hits] == ["README.md:1", "README.md"]


def test_a_full_screen_without_targets_is_quick():
    # Alt+Shift+S runs this on the key press; scanning every cell of every row took ~300 ms
    import time
    rows = [("plain words without targets here " * 7)[:200]] * 60
    start = time.perf_counter()
    assert candidates(rows) == []
    assert time.perf_counter() - start < 0.05
