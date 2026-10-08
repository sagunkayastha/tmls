#!/usr/bin/env python3
"""Replay a VT byte file into pyte and print the visible screen.

Usage: pyte_restore.py <snapshot.vt> <cols> <rows>

stdout: exactly <rows> lines, `screen.display` (one line per row, not trimmed).
stderr: one JSON object: cursor_x, cursor_y (pyte `screen.cursor`), `ignored` = number of
        events pyte routed to `Screen.debug` (sequences it does not implement), and
        `ignored_top` = the most common of those, as escaped strings.

Run with the tmls venv (pyte 0.8.2): ~/Others/tmls/.venv/bin/python (read-only use).
"""
import collections
import inspect
import json
import sys

import pyte


class CountingScreen(pyte.Screen):
    """pyte.Screen that counts what the parser dispatches to `debug` (unsupported sequences)."""

    def __init__(self, cols, rows):
        super().__init__(cols, rows)
        self.ignored = collections.Counter()
        # pyte 0.8.2 passes `private=True` for every `CSI ? ...` sequence, but most handlers do not
        # accept it and the parser generator then dies with TypeError (e.g. `CSI ? 6 n`,
        # `CSI ? 4 m`). Wrap those handlers so such a sequence is counted as ignored instead.
        for name in set(pyte.Stream.csi.values()):
            fn = getattr(self, name, None)
            if fn is None:
                continue
            params = inspect.signature(fn).parameters
            if "private" in params or any(p.kind == p.VAR_KEYWORD for p in params.values()):
                continue
            setattr(self, name, self._tolerant(name, fn))

    def _tolerant(self, name, fn):
        def wrapper(*args, **kwargs):
            if kwargs.pop("private", False):
                self.ignored["CSI ? %s (%s)" % (";".join(map(str, args)), name)] += 1
                return None
            return fn(*args, **kwargs)
        return wrapper

    def debug(self, *args, **kwargs):  # pyte calls this for unknown ESC / CSI finals
        key = " ".join(repr(a) for a in args) or "(no args)"
        if kwargs:
            key += " " + repr(kwargs)
        self.ignored[key[:80]] += 1


def main():
    path, cols, rows = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    with open(path, "rb") as f:
        data = f.read()
    screen = CountingScreen(cols, rows)
    stream = pyte.ByteStream(screen)
    stream.feed(data)
    out = sys.stdout.buffer  # explicit UTF-8, independent of the locale
    for line in screen.display:
        out.write(line.encode("utf-8", "replace") + b"\n")
    out.flush()
    info = {
        "cursor_x": screen.cursor.x,
        "cursor_y": screen.cursor.y,
        "ignored": sum(screen.ignored.values()),
        "ignored_top": [[k, n] for k, n in screen.ignored.most_common(8)],
    }
    sys.stderr.write(json.dumps(info) + "\n")


if __name__ == "__main__":
    main()
