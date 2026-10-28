"""Draws ProCraft's own interface icons from plain geometry (docs/icons/STYLE_GUIDE.md) and puts them
in place: an SVG master per icon (assets/icons/procraft/<name>.svg, a 24-unit grid), and its path
data, scaled into its slot (assets/icons/slots.tsv), in Icons.kt instead of the Phosphor icon.
Needs shapely. Run from anywhere:

    python tools/icons/draw_icons.py
"""

import math
import os
import re

from shapely.affinity import rotate, translate
from shapely.geometry import LineString, Point, Polygon, box
from shapely.ops import unary_union

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MASTERS = os.path.join(ROOT, 'assets', 'icons', 'procraft')
SLOTS = os.path.join(ROOT, 'assets', 'icons', 'slots.tsv')
ICONS = os.path.join(ROOT, 'apps', 'procraft-android', 'android', 'app', 'src', 'main', 'java', 'io', 'github',
                     'artcraftandroid', 'procraft', 'ui', 'Icons.kt')

GAP = 1.2  # a knockout's width
SQ2 = math.sqrt(0.5)


def capsule(a, b, r):
    """A line from a to b, r thick each side, with round ends."""
    return LineString([a, b]).buffer(r, quad_segs=16)


def soft(shape, r=0.4):
    """`shape` with its corners rounded by r."""
    return shape.buffer(-r, quad_segs=8).buffer(r, quad_segs=8)


def gap_across(at, direction, length):
    """A knockout across a part at `at`, at right angles to `direction`."""
    nx, ny = -direction[1], direction[0]
    a = (at[0] - nx * length / 2, at[1] - ny * length / 2)
    b = (at[0] + nx * length / 2, at[1] + ny * length / 2)
    return LineString([a, b]).buffer(GAP / 2, cap_style='flat')


def star(cx, cy, r, inner=0.3):
    """A four-pointed sparkle."""
    points = []
    for k in range(8):
        a = math.pi / 4 * k - math.pi / 2
        rr = r if k % 2 == 0 else r * inner
        points.append((cx + rr * math.cos(a), cy + rr * math.sin(a)))
    return Polygon(points).buffer(0.15, quad_segs=6)


def along_diagonal(shape, tip):
    """A drawing made along +x (its writing end at 0) turned to run up and to the right from `tip`."""
    return translate(rotate(shape, -45, origin=(0, 0)), tip[0], tip[1])


def wrench():
    """Actions: an open-end wrench with a hole in its handle."""
    head = (15.5, 8.5)
    shape = unary_union([Point(head).buffer(5.0, quad_segs=16), capsule(head, (5.5, 18.5), 1.9)])
    jaw = capsule(head, (head[0] + SQ2 * 8, head[1] - SQ2 * 8), 1.7)
    hole = Point(7.4, 16.6).buffer(0.85, quad_segs=12)
    return soft(shape.difference(jaw).difference(hole), 0.35)


def wand():
    """Adjustments: a wand with a banded tip and three sparkles."""
    shaft = capsule((4.5, 19.5), (15.0, 9.0), 1.5).difference(gap_across((12.6, 11.4), (SQ2, -SQ2), 6))
    return unary_union([shaft, star(18.6, 5.2, 3.3), star(11.2, 4.0, 2.0), star(20.2, 11.8, 1.8)])


def selection():
    """Selection: a selection's four corner marks around a solid square."""
    marks = []
    for (cx, cy), (dx, dy) in (((3.5, 3.5), (1, 1)), ((20.5, 3.5), (-1, 1)), ((3.5, 20.5), (1, -1)), ((20.5, 20.5), (-1, -1))):
        marks.append(capsule((cx, cy), (cx + 5 * dx, cy), 1.1))
        marks.append(capsule((cx, cy), (cx, cy + 5 * dy), 1.1))
    inside = soft(box(8.6, 8.6, 15.4, 15.4), 1.0)
    return unary_union(marks + [inside])


def arrow():
    """Transform: a solid arrow pointing up and to the right."""
    shaft = capsule((0, 0), (9.0, 0), 1.3)
    head = soft(Polygon([(7.0, -5.0), (15.6, 0), (7.0, 5.0)]), 0.6)
    shape = unary_union([shaft, head])
    return along_diagonal(translate(shape, -7.8, 0), (12.0, 12.0))


def paintbrush():
    """Paint: a round brush, its bristles down to the left."""
    bristles = unary_union([Point(5.6, 0).buffer(3.1, quad_segs=16), Point(0, 0).buffer(0.2)]).convex_hull
    ferrule = soft(box(9.6, -2.3, 12.4, 2.3), 0.4)
    handle = unary_union([Polygon([(13.2, -1.5), (21.6, -1.0), (21.6, 1.0), (13.2, 1.5)]),
                          Point(21.6, 0).buffer(1.0, quad_segs=12)])
    return along_diagonal(unary_union([soft(bristles, 0.3), ferrule, handle]), (4.0, 20.0))


def fingertip():
    """Smudge: a fingertip pressing on a smear."""
    finger = capsule((14.6, 2.6), (10.0, 12.4), 3.4)
    crease = gap_across((12.9, 6.2), (-0.42, 0.91), 8)
    finger = finger.difference(crease)
    wave = LineString([(3.8 + 16.4 * t, 19.4 - 1.4 * math.sin(t * 2 * math.pi)) for t in [i / 40 for i in range(41)]])
    return unary_union([finger, wave.buffer(1.3, quad_segs=10)])


def eraser():
    """Erase: an eraser block with a band, on a line."""
    block = soft(box(0, -3.8, 14.5, 3.8), 1.2).difference(box(4.6, -5, 4.6 + GAP, 5))
    block = translate(rotate(translate(block, -7.25, 0), -45, origin=(0, 0)), 12.0, 10.4)
    line = capsule((3.5, 20.8), (20.5, 20.8), 1.0)
    return unary_union([block, line])


def layers():
    """Layers: two stacked sheets, the top one solid."""
    top = soft(Polygon([(12, 2.6), (21.4, 8.4), (12, 14.2), (2.6, 8.4)]), 1.0)
    under = LineString([(2.8, 14.2), (12, 19.9), (21.2, 14.2)]).buffer(1.2, quad_segs=10)
    return unary_union([top, under])


def turning_arrow(mirror):
    """Undo (or, mirrored, Redo): a curved arrow turning back."""
    cx, cy, r = 13.0, 13.0, 6.4
    angles = [math.radians(-150 + 260 * i / 64) for i in range(65)]
    arc = LineString([(cx + r * math.cos(a), cy + r * math.sin(a)) for a in angles]).buffer(1.25, cap_style='flat')
    arc = arc.union(Point(cx + r * math.cos(angles[-1]), cy + r * math.sin(angles[-1])).buffer(1.25, quad_segs=10))
    a = angles[0]
    sx, sy = cx + r * math.cos(a), cy + r * math.sin(a)
    back = (math.sin(a), -math.cos(a))
    normal = (math.cos(a), math.sin(a))
    tip = (sx + back[0] * 3.6, sy + back[1] * 3.6)
    head = soft(Polygon([tip, (sx + normal[0] * 3.6, sy + normal[1] * 3.6), (sx - normal[0] * 3.6, sy - normal[1] * 3.6)]), 0.45)
    shape = unary_union([arc, head])
    if mirror:
        from shapely.affinity import scale
        shape = scale(shape, -1, 1, origin=(12, 12))
    return shape


DRAWINGS = {
    'actions': wrench, 'adjustments': wand, 'selection': selection, 'transform': arrow,
    'brush': paintbrush, 'smudge': fingertip, 'erase': eraser, 'layers': layers,
    'undo': lambda: turning_arrow(False), 'redo': lambda: turning_arrow(True),
}


def fmt(v):
    s = f'{v:.2f}'.rstrip('0').rstrip('.')
    return '0' if s in ('-0', '') else s


def path_data(shape, fit=lambda x, y: (x, y)):
    """Path data (even-odd) for every outline and hole of `shape`, through `fit`."""
    polygons = getattr(shape, 'geoms', [shape])
    d = []
    for poly in polygons:
        for ring in [poly.exterior, *poly.interiors]:
            pts = [fit(x, y) for x, y in ring.coords[:-1]]
            d.append('M' + 'L'.join(f'{fmt(x)} {fmt(y)}' for x, y in pts) + 'Z')
    return ''.join(d)


def slots():
    out = {}
    for line in open(SLOTS, encoding='utf-8'):
        if line.startswith('#') or line.startswith('name'):
            continue
        name, w, h, x0, y0, x1, y1 = line.rstrip('\n').split('\t')
        out[name] = (float(w), float(h), (float(x0), float(y0), float(x1), float(y1)))
    return out


def main():
    os.makedirs(MASTERS, exist_ok=True)
    places = slots()
    src = open(ICONS, encoding='utf-8', newline='').read()
    for name, draw in DRAWINGS.items():
        shape = draw().simplify(0.02)
        with open(os.path.join(MASTERS, f'{name}.svg'), 'w', encoding='utf-8', newline='\n') as f:
            f.write('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">\n'
                    f'  <!-- ProCraft {name} icon: original, drawn by tools/icons/draw_icons.py. MIT OR Apache-2.0. -->\n'
                    f'  <path fill="currentColor" fill-rule="evenodd" d="{path_data(shape)}"/>\n</svg>\n')
        w, h, (x0, y0, x1, y1) = places[name]
        bx0, by0, bx1, by1 = shape.bounds
        s = min((x1 - x0) / (bx1 - bx0), (y1 - y0) / (by1 - by0))
        ox = (x0 + x1) / 2 - s * (bx0 + bx1) / 2
        oy = (y0 + y1) / 2 - s * (by0 + by1) / 2
        d = path_data(shape, lambda x, y: (ox + s * x, oy + s * y))
        frame = f'{fmt(w)}f' if w == h else f'{fmt(w)}f, {fmt(h)}f'
        call = re.compile(r'(?:phosphor|ours)\("' + re.escape(name) + r'", [^\n]*?\)(?= \})')
        src, n = call.subn(f'ours("{name}", {frame}, "{d}")', src)
        if n != 1:
            raise SystemExit(f'{name}: {n} places in Icons.kt')
    open(ICONS, 'w', encoding='utf-8', newline='').write(src)
    print(f'{len(DRAWINGS)} icons drawn')


if __name__ == '__main__':
    main()
