#!/usr/bin/env python3
"""rows.py — measure text rows in a `grim -t ppm` dump or an 8-bit PNG (stdlib only).

    rows.py FILE.ppm|FILE.png X0 X1 [--bg R,G,B] [--tol N] [--y0 Y] [--y1 Y]

For the column band X0..X1 (pixels, X1 exclusive) prints one line per inked run of rows:
`first_y last_y height` — a row is inked when any pixel in the band differs from the background
by more than `tol` in any channel. The background is the band's most common colour unless `--bg`
is given. Use it on the list, terminal and panel columns of a screenshot to see where rows start.
"""
import sys
from collections import Counter


def read_ppm(path):
    with open(path, "rb") as f:
        data = f.read()
    fields = []
    pos = 0
    while len(fields) < 4:
        while data[pos : pos + 1].isspace():
            pos += 1
        if data[pos : pos + 1] == b"#":
            while data[pos : pos + 1] not in (b"\n", b""):
                pos += 1
            continue
        start = pos
        while not data[pos : pos + 1].isspace():
            pos += 1
        fields.append(data[start:pos])
    pos += 1  # one whitespace byte after maxval
    if fields[0] != b"P6":
        sys.exit(f"{path}: not a binary PPM (P6)")
    w, h, maxval = int(fields[1]), int(fields[2]), int(fields[3])
    if maxval != 255:
        sys.exit(f"{path}: maxval {maxval} unsupported")
    return w, h, data[pos : pos + w * h * 3]


def read_png(path):
    """8-bit RGB/RGBA/grey non-interlaced PNG → (w, h, rgb bytes). Stdlib only."""
    import struct
    import zlib

    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit(f"{path}: not a PNG")
    pos, idat, w = 8, [], None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or interlace != 0 or ctype not in (0, 2, 4, 6):
                sys.exit(f"{path}: PNG depth {depth} type {ctype} interlace {interlace} unsupported")
            ch = {0: 1, 2: 3, 4: 2, 6: 4}[ctype]
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    raw = zlib.decompress(b"".join(idat))
    stride = w * ch
    out = bytearray(w * h * 3)
    prev = bytearray(stride)
    at = 0
    for y in range(h):
        ftype = raw[at]
        line = bytearray(raw[at + 1 : at + 1 + stride])
        at += 1 + stride
        if ftype == 1:
            for i in range(ch, stride):
                line[i] = (line[i] + line[i - ch]) & 255
        elif ftype == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif ftype == 3:
            for i in range(stride):
                left = line[i - ch] if i >= ch else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 255
        elif ftype == 4:
            for i in range(stride):
                a = line[i - ch] if i >= ch else 0
                b = prev[i]
                c = prev[i - ch] if i >= ch else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 255
        row = y * w * 3
        if ch >= 3:
            for x in range(w):
                out[row + 3 * x : row + 3 * x + 3] = line[ch * x : ch * x + 3]
        else:
            for x in range(w):
                g = line[ch * x]
                out[row + 3 * x : row + 3 * x + 3] = bytes((g, g, g))
        prev = line
    return w, h, bytes(out)


def read_image(path):
    return read_png(path) if path.lower().endswith(".png") else read_ppm(path)


def main(argv):
    if len(argv) < 4:
        sys.exit(__doc__)
    path, x0, x1 = argv[1], int(argv[2]), int(argv[3])
    bg, tol, y0, y1 = None, 24, 0, None
    i = 4
    while i < len(argv):
        if argv[i] == "--bg":
            bg = tuple(int(c) for c in argv[i + 1].split(","))
            i += 2
        elif argv[i] == "--tol":
            tol = int(argv[i + 1])
            i += 2
        elif argv[i] == "--y0":
            y0 = int(argv[i + 1])
            i += 2
        elif argv[i] == "--y1":
            y1 = int(argv[i + 1])
            i += 2
        else:
            sys.exit(f"unknown argument {argv[i]}")
    w, h, px = read_image(path)
    x1 = min(x1, w)
    y1 = h if y1 is None else min(y1, h)
    if bg is None:
        counts = Counter()
        for y in range(y0, y1):
            row = (y * w + x0) * 3
            for x in range(x1 - x0):
                counts[px[row + 3 * x : row + 3 * x + 3]] += 1
        bg = tuple(counts.most_common(1)[0][0])
    print(f"# {path}: {w}x{h}, band x={x0}..{x1}, bg={bg}, tol={tol}")
    inked = []
    for y in range(y0, y1):
        row = (y * w + x0) * 3
        hit = False
        for x in range(x1 - x0):
            r, g, b = px[row + 3 * x], px[row + 3 * x + 1], px[row + 3 * x + 2]
            if abs(r - bg[0]) > tol or abs(g - bg[1]) > tol or abs(b - bg[2]) > tol:
                hit = True
                break
        inked.append(hit)
    start = None
    for y, hit in enumerate(inked, start=y0):
        if hit and start is None:
            start = y
        elif not hit and start is not None:
            print(f"{start} {y - 1} {y - start}")
            start = None
    if start is not None:
        print(f"{start} {y1 - 1} {y1 - start}")


if __name__ == "__main__":
    main(sys.argv)
