"""Writes the Mac app's icon (an .icns) from the 1024 px icon: the tile shrunk onto a Mac icon's
grid (824 of 1024, centred, as the system's own icons sit) at every size the Finder and the Dock
look up, each kept as a PNG inside. Needs Python 3 alone (no Pillow, no Mac), so it runs on any
build machine:

    python3 packaging/macos/icns.py assets/app-icon/procraft-1024.png procraft.icns

Run by package.sh. Shrinking is by area (each pixel the average of what it covers, alpha
premultiplied), halving first while the picture is at least twice the tile.
"""

import struct
import sys
import zlib
from array import array

# The tile's share of a Mac icon's square.
GRID = 824 / 1024

# Each icon in the file: its type and its edge (pixels); the @2x ones share a size with another.
TYPES = [
    (b'icp4', 16),
    (b'icp5', 32),
    (b'icp6', 64),
    (b'ic07', 128),
    (b'ic08', 256),
    (b'ic09', 512),
    (b'ic10', 1024),
    (b'ic11', 32),
    (b'ic12', 64),
    (b'ic13', 256),
    (b'ic14', 512),
]

PNG_SIGNATURE = b'\x89PNG\r\n\x1a\n'


class Picture:
    """Premultiplied RGBA, 0..1, `w` x `h`, rows top first."""

    def __init__(self, w, h, px):
        self.w, self.h, self.px = w, h, px


def read_png(path):
    """An 8-bit RGB or RGBA PNG, not interlaced, as a premultiplied picture."""
    with open(path, 'rb') as f:
        data = f.read()
    if data[:8] != PNG_SIGNATURE:
        raise ValueError(f'{path} is not a PNG')
    at, idat, header = 8, [], None
    while at + 8 <= len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat.append(body)
        elif kind == b'IEND':
            break
        at += 12 + length
    if header is None:
        raise ValueError(f'{path} has no header')
    w, h, depth, colour, _, _, interlace = header
    if depth != 8 or colour not in (2, 6) or interlace:
        raise ValueError(f'{path}: only 8-bit RGB or RGBA, not interlaced, is read')
    bpp = 4 if colour == 6 else 3
    raw = zlib.decompress(b''.join(idat))
    stride = w * bpp
    rows, prev, at = [], bytearray(stride), 0
    for _ in range(h):
        kind, line = raw[at], bytearray(raw[at + 1:at + 1 + stride])
        at += 1 + stride
        unfilter(kind, line, prev, bpp)
        rows.append(line)
        prev = line
    px = array('d', bytes(0))
    for line in rows:
        for x in range(0, stride, bpp):
            a = line[x + 3] / 255 if bpp == 4 else 1.0
            px.extend((line[x] / 255 * a, line[x + 1] / 255 * a, line[x + 2] / 255 * a, a))
    return Picture(w, h, px)


def unfilter(kind, line, prev, bpp):
    """PNG's filters undone in place on `line` (the row above is `prev`)."""
    n = len(line)
    if kind == 0:
        return
    if kind == 1:
        for i in range(bpp, n):
            line[i] = (line[i] + line[i - bpp]) & 0xFF
    elif kind == 2:
        for i in range(n):
            line[i] = (line[i] + prev[i]) & 0xFF
    elif kind == 3:
        for i in range(n):
            left = line[i - bpp] if i >= bpp else 0
            line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
    elif kind == 4:
        for i in range(n):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            p = a + b - c
            pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
            line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 0xFF
    else:
        raise ValueError(f'unknown PNG filter {kind}')


def halve(pic):
    """Each 2 x 2 block averaged."""
    w, h, src = pic.w // 2, pic.h // 2, pic.px
    out = array('d', bytes(8 * w * h * 4))
    row = pic.w * 4
    for y in range(h):
        top, bottom, o = 2 * y * row, (2 * y + 1) * row, y * w * 4
        for x in range(w):
            i, j = top + 8 * x, bottom + 8 * x
            for c in range(4):
                out[o + 4 * x + c] = (src[i + c] + src[i + 4 + c] + src[j + c] + src[j + 4 + c]) / 4
    return Picture(w, h, out)


def weights(n, m):
    """For shrinking `n` pixels to `m`: each output pixel's (first input, weights), by area."""
    scale, out = n / m, []
    for o in range(m):
        start, end = o * scale, (o + 1) * scale
        first, taps = int(start), []
        i = first
        while i < end and i < n:
            taps.append((min(end, i + 1) - max(start, i)) / scale)
            i += 1
        out.append((first, taps))
    return out


def shrink(pic, w, h):
    """`pic` shrunk to `w` x `h` by area: across, then down."""
    px, across = pic.px, array('d', bytes(8 * w * pic.h * 4))
    columns = weights(pic.w, w)
    for y in range(pic.h):
        src, o = y * pic.w * 4, y * w * 4
        for x, (first, taps) in enumerate(columns):
            r = g = b = a = 0.0
            i = src + 4 * first
            for t in taps:
                r += px[i] * t
                g += px[i + 1] * t
                b += px[i + 2] * t
                a += px[i + 3] * t
                i += 4
            j = o + 4 * x
            across[j], across[j + 1], across[j + 2], across[j + 3] = r, g, b, a
    out, row = array('d', bytes(8 * w * h * 4)), w * 4
    for y, (first, taps) in enumerate(weights(pic.h, h)):
        line = [0.0] * row
        for k, t in enumerate(taps):
            src = (first + k) * row
            line = [v + s * t for v, s in zip(line, across[src:src + row])]
        out[y * row:(y + 1) * row] = array('d', line)
    return Picture(w, h, out)


def on_grid(halves, edge):
    """The icon at `edge` pixels: the tile shrunk to the grid's share, centred on clear.
    `halves` is the icon and each half of the one before, as far as made (more are added)."""
    tile = max(1, round(edge * GRID))
    level = 0
    while halves[level].w >= 2 * tile and halves[level].w % 2 == 0:
        if level + 1 == len(halves):
            halves.append(halve(halves[level]))
        level += 1
    pic = halves[level]
    if pic.w != tile:
        pic = shrink(pic, tile, tile)
    margin = (edge - tile) // 2
    out = array('d', bytes(8 * edge * edge * 4))
    for y in range(tile):
        o = ((margin + y) * edge + margin) * 4
        out[o:o + tile * 4] = pic.px[y * tile * 4:(y + 1) * tile * 4]
    return Picture(edge, edge, out)


def png(pic):
    """`pic` as an 8-bit RGBA PNG (straight alpha)."""
    raw = bytearray()
    for y in range(pic.h):
        raw.append(0)
        for x in range(pic.w):
            i = (y * pic.w + x) * 4
            a = pic.px[i + 3]
            if a <= 0:
                raw += b'\0\0\0\0'
                continue
            raw += bytes(min(255, round(v / a * 255)) for v in pic.px[i:i + 3])
            raw.append(min(255, round(a * 255)))

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))

    header = struct.pack('>IIBBBBB', pic.w, pic.h, 8, 6, 0, 0, 0)
    return PNG_SIGNATURE + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(bytes(raw), 9)) + chunk(b'IEND', b'')


def icns(source):
    """The .icns file's bytes: each type's PNG, those of one size drawn once."""
    drawn, entries, halves = {}, b'', [source]
    for kind, edge in TYPES:
        if edge not in drawn:
            drawn[edge] = png(on_grid(halves, edge))
        body = drawn[edge]
        entries += kind + struct.pack('>I', 8 + len(body)) + body
    return b'icns' + struct.pack('>I', 8 + len(entries)) + entries


def main():
    if len(sys.argv) != 3:
        sys.exit('usage: icns.py <icon.png (square)> <out.icns>')
    source = read_png(sys.argv[1])
    if source.w != source.h:
        sys.exit(f'{sys.argv[1]} is not square')
    data = icns(source)
    with open(sys.argv[2], 'wb') as f:
        f.write(data)
    print(f'wrote {sys.argv[2]} ({len(data)} bytes, {len(TYPES)} icons)')


if __name__ == '__main__':
    main()
