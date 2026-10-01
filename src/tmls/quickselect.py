"""Visible terminal targets for keyboard hint selection."""

import re
import string
from dataclasses import dataclass

from tmls.term import link_at

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
        x = 0
        while x < len(row):
            found = link_at(rows, x, y)
            if found:
                start = x
                while x < len(row) and link_at(rows, x, y) == found:
                    occupied.add(x)
                    x += 1
                kind, target, line = found
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
