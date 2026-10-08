"""Visible terminal targets for keyboard hint selection."""

import re
import string
from dataclasses import dataclass

from tmls.term import FILE_LINE, URL, link_at

PATH = re.compile(
    r"(?<![\w./])(?:~?/|\.{1,2}/)[A-Za-z0-9._/+~-]+|"
    r"(?<![\w./])[A-Za-z0-9._+-]+(?:/[A-Za-z0-9._+-]+)+|"
    r"(?<![\w./])[A-Za-z0-9_+-]+\.[A-Za-z0-9]+"
)
HASH = re.compile(r"(?<![0-9A-Fa-f])[0-9A-Fa-f]{7,40}(?![0-9A-Fa-f])")


@dataclass(frozen=True)
class Hit:
    label: str
    kind: str
    target: str
    line: int | None
    y: int
    x: int
    copy_text: str


def candidates(rows):
    hits = []
    for y, row in enumerate(rows):
        targets = []
        occupied = set()
        # link_at rescans the row per cell: only worth it where a link can be (it may wrap onto the next row)
        nearby = row + (rows[y + 1] if y + 1 < len(rows) else "")
        x = 0 if URL.search(nearby) or FILE_LINE.search(nearby) else len(row)
        while x < len(row):
            found = link_at(rows, x, y)
            if found:
                start = x
                while x < len(row) and link_at(rows, x, y) == found:
                    occupied.add(x)
                    x += 1
                kind, target, line = found
                if kind == "file" and line is None:
                    line = 1  # a plain path opens at its top, same as PATH matches below
                targets.append((start, kind, target, line, row[start:x] if kind == "file" else target))
            else:
                x += 1
        for pattern, kind in ((PATH, "file"), (HASH, "hash")):
            for match in pattern.finditer(row):
                if any(x in occupied for x in range(*match.span())):
                    continue
                targets.append((match.start(), kind, match.group(),
                                1 if kind == "file" else None, match.group()))
                occupied.update(range(*match.span()))
        for x, kind, target, line, copy_text in sorted(targets):
            if len(hits) == 26:
                return hits
            hits.append(Hit(string.ascii_lowercase[len(hits)], kind, target, line, y, x, copy_text))
    return hits
