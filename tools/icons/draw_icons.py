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


def ring(cx, cy, r, w):
    """A circle's outline, w thick."""
    return Point(cx, cy).buffer(r + w / 2, quad_segs=16).difference(Point(cx, cy).buffer(r - w / 2, quad_segs=16))


def frame(x0, y0, x1, y1, w, r):
    """A rounded rectangle's outline, w thick, its outer corners rounded by r."""
    outer = soft(box(x0, y0, x1, y1), r)
    return outer.difference(soft(box(x0 + w, y0 + w, x1 - w, y1 - w), max(0.2, r - w)))


def plus():
    """Add: a plus."""
    return unary_union([capsule((12, 4), (12, 20), 1.3), capsule((4, 12), (20, 12), 1.3)])


def canvas_tab():
    """Canvas: a canvas's frame with a stroke across it."""
    return unary_union([frame(3, 4, 21, 20, 2.2, 2.0), capsule((7.5, 15.5), (16.5, 8.5), 1.3)])


def share_tab():
    """Share: a tray with an arrow rising out of it."""
    tray = LineString([(4, 12.5), (4, 20), (20, 20), (20, 12.5)]).buffer(1.2, join_style='round')
    shaft = capsule((12, 4.5), (12, 15), 1.2)
    head = soft(Polygon([(12, 2.4), (17, 7.6), (7, 7.6)]), 0.4)
    return unary_union([tray, shaft, head])


def video_tab():
    """Video: a camera, its body and its lens."""
    body = soft(box(2.5, 6.5, 15.5, 17.5), 2.0)
    lens = soft(Polygon([(16.8, 12), (21.5, 8.2), (21.5, 15.8)]), 0.6)
    return unary_union([body, lens])


def prefs_tab():
    """Preferences: a switch, turned on."""
    track = capsule((7.5, 12), (16.5, 12), 5.2).difference(capsule((7.5, 12), (16.5, 12), 3.2))
    return unary_union([track, Point(16.5, 12).buffer(2.4, quad_segs=16)])


def help_tab():
    """Help: a question mark in a ring."""
    hook = LineString([(12 + 3.0 * math.cos(math.radians(a)), 9.4 + 3.0 * math.sin(math.radians(a)))
                       for a in range(200, 361, 8)] + [(15.0, 9.4), (12.2, 12.4), (12.0, 14.0)])
    return unary_union([ring(12, 12, 9.0, 2.0), hook.buffer(1.15, join_style='round'), Point(12, 17.4).buffer(1.35, quad_segs=10)])


def dashes(points, closed, dash, gap_len, r):
    """Short capsules along a line through `points`: a dashed outline."""
    line = LineString(points + ([points[0]] if closed else []))
    out, t = [], 0.0
    while t < line.length:
        a, b = line.interpolate(t), line.interpolate(min(t + dash, line.length))
        out.append(capsule((a.x, a.y), (b.x, b.y), r))
        t += dash + gap_len
    return unary_union(out)


def lasso():
    """Freehand: a lasso's loop with its tail."""
    loop = rotate(Point(12.5, 9.5).buffer(1.0, quad_segs=24), 0)
    from shapely.affinity import scale
    ellipse = scale(Point(12.5, 9.5).buffer(1.0, quad_segs=24), 8.0, 5.2)
    loop = rotate(ellipse.buffer(1.1).difference(ellipse.buffer(-1.1)), -12, origin=(12.5, 9.5))
    tail = LineString([(7.6, 13.6), (7.0, 16.2), (8.6, 18.0), (6.6, 20.6)]).buffer(1.1, join_style='round')
    return unary_union([loop, tail, Point(7.8, 13.8).buffer(1.6, quad_segs=10)])


def rectangle_tool():
    """Rectangle: a dashed rectangle."""
    return dashes([(3.5, 5.5), (20.5, 5.5), (20.5, 18.5), (3.5, 18.5)], True, 2.4, 2.4, 1.0)


def ellipse_tool():
    """Ellipse: a dashed ellipse."""
    pts = [(12 + 8.5 * math.cos(a / 48 * 2 * math.pi), 12 + 6.5 * math.sin(a / 48 * 2 * math.pi)) for a in range(48)]
    return dashes(pts, True, 2.4, 2.4, 1.0)


def sign_in_disc(minus):
    """Add (or Subtract): a plus (a minus) cut out of a disc."""
    disc = Point(12, 12).buffer(9.0, quad_segs=24)
    mark = capsule((7.2, 12), (16.8, 12), 1.2)
    if not minus:
        mark = mark.union(capsule((12, 7.2), (12, 16.8), 1.2))
    return disc.difference(mark)


def invert():
    """Invert: a disc, one half solid, the other its outline."""
    outline = ring(12, 12, 8.0, 2.0)
    half = Point(12, 12).buffer(8.0, quad_segs=24).intersection(box(12, 0, 24, 24))
    return unary_union([outline, half])


def copy_paste():
    """Copy and paste: two sheets, the front one solid."""
    back = frame(3.5, 3.5, 15.5, 15.5, 2.0, 2.0)
    front = soft(box(8.5, 8.5, 20.5, 20.5), 2.0)
    return unary_union([back.difference(box(7.3, 7.3, 21.7, 21.7)), front])


def feather():
    """Feather: a feather, its vane split by the shaft."""
    vane = Polygon([(19.5, 4.5)] + [(19.5 - 12 * t + 3.2 * math.sin(t * math.pi) * SQ2, 4.5 + 12 * t + 3.2 * math.sin(t * math.pi) * SQ2)
                                    for t in [i / 20 for i in range(1, 21)]]
                   + [(19.5 - 12 * t - 3.2 * math.sin(t * math.pi) * SQ2, 4.5 + 12 * t - 3.2 * math.sin(t * math.pi) * SQ2)
                      for t in [i / 20 for i in range(19, 0, -1)]])
    shaft = LineString([(19.0, 5.0), (5.5, 18.5)])
    vane = soft(vane, 0.3).difference(shaft.buffer(0.55))
    return unary_union([vane, capsule((7.8, 16.2), (4.0, 20.0), 1.0)])


def save_load():
    """Save and load: a tray with an arrow coming down into it."""
    tray = LineString([(4, 12.5), (4, 20), (20, 20), (20, 12.5)]).buffer(1.2, join_style='round')
    shaft = capsule((12, 3.5), (12, 12), 1.2)
    head = soft(Polygon([(12, 16.6), (7, 11.4), (17, 11.4)]), 0.4)
    return unary_union([tray, shaft, head])


def paint_bucket():
    """Colour fill: a tipped bucket and a drop falling from it."""
    bucket = soft(Polygon([(4.5, 9.5), (13.5, 3.8), (19.0, 12.5), (10.0, 18.2)]), 1.2)
    rim = capsule((4.5, 9.5), (13.5, 3.8), 0.6)
    bucket = bucket.difference(LineString([(6.6, 11.2), (14.6, 6.1)]).buffer(0.55))
    drop = unary_union([Point(19.2, 18.6).buffer(2.1, quad_segs=12), Point(19.2, 14.4).buffer(0.2)]).convex_hull
    return unary_union([bucket, rim, drop])


def cross():
    """Clear: a cross."""
    return unary_union([capsule((5.5, 5.5), (18.5, 18.5), 1.3), capsule((18.5, 5.5), (5.5, 18.5), 1.3)])


def double_arrow(a, b, r=1.1, head=3.2):
    """A line from a to b with an arrowhead at each end."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    n = math.hypot(dx, dy)
    ux, uy = dx / n, dy / n
    px, py = -uy, ux
    parts = [capsule((a[0] + ux * head * 0.8, a[1] + uy * head * 0.8), (b[0] - ux * head * 0.8, b[1] - uy * head * 0.8), r)]
    for (tx, ty), s_ in ((a, 1), (b, -1)):
        bx, by = tx + s_ * ux * head * 1.3, ty + s_ * uy * head * 1.3
        parts.append(soft(Polygon([(tx, ty), (bx + px * head, by + py * head), (bx - px * head, by - py * head)]), 0.35))
    return unary_union(parts)


def freeform():
    """Freeform: a frame stretched across."""
    return unary_union([frame(2.5, 5, 21.5, 19, 1.8, 1.8), double_arrow((6.5, 12), (17.5, 12), 1.0, 2.6)])


def uniform():
    """Uniform: a square frame scaled corner to corner."""
    return unary_union([frame(3, 3, 21, 21, 1.8, 1.8), double_arrow((7.4, 16.6), (16.6, 7.4), 1.0, 2.6)])


def distort():
    """Distort: a skewed four-sided frame with a handle at each corner."""
    corners = [(5.0, 4.5), (19.5, 7.0), (18.0, 19.5), (3.5, 17.0)]
    edges = LineString(corners + [corners[0]]).buffer(0.9)
    return unary_union([edges] + [Point(c).buffer(2.3, quad_segs=12) for c in corners])


def warp():
    """Warp: a frame whose inner lines bend."""
    bends = []
    for x in (9.0, 15.0):
        bends.append(LineString([(x + 1.6 * math.sin(t * math.pi), 3.5 + 17 * t) for t in [i / 24 for i in range(25)]]).buffer(0.9))
    for y in (9.0, 15.0):
        bends.append(LineString([(3.5 + 17 * t, y - 1.6 * math.sin(t * math.pi)) for t in [i / 24 for i in range(25)]]).buffer(0.9))
    return unary_union([frame(2.5, 2.5, 21.5, 21.5, 1.8, 2.0)] + bends)


def magnet():
    """Snapping: a magnet, its poles cut off."""
    arc = [(12 - 5.6 * math.cos(math.pi * t), 12.8 + 5.6 * math.sin(math.pi * t)) for t in [i / 32 for i in range(33)]]
    body = LineString([(6.4, 3.0)] + arc + [(17.6, 3.0)]).buffer(1.9, cap_style='flat', join_style='round')
    return body.difference(box(0, 6.6, 24, 7.8))


def flip(vertical):
    """Flip: a solid triangle facing its hollow mirror image."""
    left = soft(Polygon([(10.6, 3.0), (10.6, 21.0), (2.0, 21.0)]), 0.6)
    right = soft(Polygon([(13.4, 3.0), (13.4, 21.0), (22.0, 21.0)]), 0.6)
    right = right.difference(right.buffer(-2.0, join_style='mitre'))
    shape = unary_union([left, right])
    return rotate(shape, 90, origin=(12, 12)) if vertical else shape


def arc_arrow(cx, cy, r, a0, a1, w=1.2, head=3.4):
    """An arc from angle a0 to a1 (degrees, clockwise on screen) with an arrowhead at a1."""
    angles = [math.radians(a0 + (a1 - a0) * i / 48) for i in range(49)]
    arc = LineString([(cx + r * math.cos(a), cy + r * math.sin(a)) for a in angles]).buffer(w, cap_style='flat')
    arc = arc.union(Point(cx + r * math.cos(angles[0]), cy + r * math.sin(angles[0])).buffer(w, quad_segs=10))
    a = angles[-1]
    ex, ey = cx + r * math.cos(a), cy + r * math.sin(a)
    ahead, out = (-math.sin(a), math.cos(a)), (math.cos(a), math.sin(a))
    tip = (ex + ahead[0] * head * 1.1, ey + ahead[1] * head * 1.1)
    return unary_union([arc, soft(Polygon([tip, (ex + out[0] * head, ey + out[1] * head), (ex - out[0] * head, ey - out[1] * head)]), 0.45)])


def rotate_step():
    """Rotate: a tilted square with an arrow turning over it."""
    square = rotate(soft(box(7.5, 10.5, 14.5, 17.5), 0.8), 45, origin=(11, 14))
    return unary_union([square, arc_arrow(11, 14, 8.5, -160, -38)])


def fit_to_canvas():
    """Fit to canvas: four broad arrows pointing out to the corners (the slot is small, so they're
    heavier than the guide's line weight)."""
    arrows = []
    for dx, dy in ((1, 1), (-1, 1), (1, -1), (-1, -1)):
        a = (12 + dx * 2.6, 12 + dy * 2.6)
        b = (12 + dx * 9.4, 12 + dy * 9.4)
        ux, uy = dx * SQ2, dy * SQ2
        px, py = -uy, ux
        head = soft(Polygon([b, (b[0] - ux * 5.6 + px * 4.0, b[1] - uy * 5.6 + py * 4.0), (b[0] - ux * 5.6 - px * 4.0, b[1] - uy * 5.6 - py * 4.0)]), 0.4)
        arrows += [capsule(a, (b[0] - ux * 4.0, b[1] - uy * 4.0), 1.6), head]
    return unary_union(arrows)


def interpolation():
    """Interpolation: square pixels turning into a smooth round one."""
    cells = [soft(box(3, 3, 11, 11), 0.6), soft(box(3, 13, 11, 21), 0.6), soft(box(13, 13, 21, 21), 0.6)]
    return unary_union(cells + [Point(17, 7).buffer(4.0, quad_segs=16)])


def reset_transform():
    """Reset: a turning arrow back, round a dot."""
    return unary_union([turning_arrow(False), Point(13.0, 13.0).buffer(1.6, quad_segs=12)])


def colour_wheel():
    """Hue, Saturation, Brightness: a colour wheel, six segments round a hole."""
    disc = Point(12, 12).buffer(9.0, quad_segs=24).difference(Point(12, 12).buffer(3.2, quad_segs=16))
    for a in (0, 60, 120):
        disc = disc.difference(rotate(box(0, 11.4, 24, 12.6), a, origin=(12, 12)))
    return disc


def scales():
    """Colour Balance: a balance, its beam on a post, a pan hanging from each end."""
    parts = [capsule((12, 6.5), (12, 18.8), 1.2), soft(box(7.0, 18.4, 17.0, 20.8), 0.8),
             capsule((5.5, 7.0), (18.5, 7.0), 1.1), Point(12, 5.0).buffer(1.8, quad_segs=12)]
    for x in (5.5, 18.5):
        parts.append(Point(x, 13.2).buffer(3.4, quad_segs=16).intersection(box(0, 13.2, 24, 24)))
        parts += [capsule((x, 7.0), (x - 2.9, 13.4), 0.8), capsule((x, 7.0), (x + 2.9, 13.4), 0.8)]
    return unary_union(parts)


def curve_graph():
    """Curves: a graph's two axes and an S-curve rising between them."""
    pts = [(8.0 + 12.0 * t, 16.6 - 12.0 * (3 * t * t - 2 * t * t * t)) for t in [i / 32 for i in range(33)]]
    return unary_union([capsule((4.0, 3.5), (4.0, 20.0), 1.1), capsule((4.0, 20.0), (20.5, 20.0), 1.1),
                        LineString(pts).buffer(1.2)])


def gradient_bars():
    """Gradient Map: a bar that breaks into narrower and narrower strips."""
    bar = soft(box(3.0, 5.0, 21.4, 19.0), 1.0)
    for x0, x1 in ((9.4, 10.6), (14.4, 15.8), (18.2, 19.8)):
        bar = bar.difference(box(x0, 0, x1, 24))
    return bar


def dot_halo():
    """Gaussian Blur: a dot with two rings of smaller dots round it."""
    parts = [Point(12, 12).buffer(3.4, quad_segs=16)]
    for i in range(8):
        a = math.radians(45 * i)
        parts.append(Point(12 + 6.6 * math.cos(a), 12 + 6.6 * math.sin(a)).buffer(1.3, quad_segs=10))
        b = a + math.radians(22.5)
        parts.append(Point(12 + 9.6 * math.cos(b), 12 + 9.6 * math.sin(b)).buffer(0.9, quad_segs=8))
    return unary_union(parts)


def speeding_disc():
    """Motion Blur: a disc with speed lines trailing behind it."""
    return unary_union([Point(15.4, 12).buffer(5.4, quad_segs=20), capsule((2.6, 12), (7.4, 12), 1.1),
                        capsule((4.6, 7.0), (9.4, 7.0), 1.1), capsule((4.6, 17.0), (9.4, 17.0), 1.1)])


def converging():
    """Perspective Blur: a tunnel of squares, smaller and smaller toward the middle."""
    return unary_union([frame(3, 3, 21, 21, 1.6, 1.6), frame(7.2, 7.2, 16.8, 16.8, 1.6, 1.0),
                        soft(box(10.6, 10.6, 13.4, 13.4), 0.4)])


def scattered_dots():
    """Noise: dots of a few sizes scattered over a square."""
    dots = [(5, 5, 1.5), (11, 4.5, 1.0), (17.5, 5.5, 1.8), (7.5, 10, 1.0), (13, 10, 1.8), (19, 11.5, 1.0),
            (4.5, 15.5, 1.8), (10, 15.5, 1.0), (15.5, 16, 1.2), (20, 18.5, 1.4), (6.5, 20, 1.0), (12.5, 20.5, 1.5)]
    return unary_union([Point(x, y).buffer(r, quad_segs=10) for x, y, r in dots])


def gem():
    """Sharpen: a cut gem, its facets knocked out."""
    crown = Polygon([(7.5, 4.5), (16.5, 4.5), (21.0, 9.6), (3.0, 9.6)])
    pavilion = Polygon([(3.0, 10.8), (21.0, 10.8), (12.0, 21.0)])
    stone = unary_union([soft(crown, 0.5), soft(pavilion, 0.5)])
    for x in (8.6, 15.4):
        stone = stone.difference(LineString([(12.0, 20.0), (x, 10.8)]).buffer(0.55))
    return stone


def sun():
    """Bloom: a bright disc caught in a four-point glint."""
    glint = star(12, 12, 10.5, 0.34).difference(Point(12, 12).buffer(5.6, quad_segs=20))
    return unary_union([glint, Point(12, 12).buffer(4.2, quad_segs=20)])


def glitch_bands():
    """Glitch: a block cut into three bands, each slipped sideways."""
    return unary_union([soft(box(3 + dx, y0, 21 + dx, y1), 0.8) for dx, y0, y1 in
                        ((-1.5, 4.0, 8.8), (2.0, 10.0, 14.6), (-0.5, 15.8, 20.0))])


def halftone():
    """Halftone: a grid of dots growing toward one corner."""
    dots = []
    for i, x in enumerate((4.5, 9.5, 14.5, 19.5)):
        for j, y in enumerate((4.5, 9.5, 14.5, 19.5)):
            dots.append(Point(x, y).buffer(0.8 + 0.2 * ((3 - i) + j), quad_segs=10))
    return unary_union(dots)


def fringed_lens():
    """Chromatic Aberration: a prism, one ray in and three spreading out."""
    prism = soft(Polygon([(10.0, 4.0), (16.5, 17.5), (3.5, 17.5)]), 0.6)
    rays = [capsule((1.8, 9.4), (5.0, 10.0), 1.0), capsule((14.6, 9.6), (21.6, 6.8), 1.0),
            capsule((15.8, 11.8), (22.0, 11.8), 1.0), capsule((16.9, 14.0), (21.6, 16.8), 1.0)]
    return unary_union([prism] + rays)


def spiral():
    """Liquify: a block melting, three drips running off it."""
    block = unary_union([soft(box(3.0, 3.0, 21.0, 12.0), 1.0), capsule((7.0, 11.0), (7.0, 17.0), 1.8),
                         capsule((13.6, 11.0), (13.6, 20.0), 1.8), capsule((18.6, 11.0), (18.6, 14.6), 1.5)])
    return block.buffer(1.0).buffer(-1.0)


def stamp():
    """Clone: a rubber stamp over the mark it leaves."""
    return unary_union([Point(12, 5.6).buffer(2.5, quad_segs=16), box(10.4, 7.0, 13.6, 10.4),
                        soft(box(3.0, 10.0, 21.0, 14.8), 1.0), capsule((3.2, 17.4), (20.8, 17.4), 1.0)])


def colour_disc():
    """Disc: a disc with a picker's ring on it."""
    return Point(12, 12).buffer(9.5, quad_segs=24).difference(ring(16.0, 8.0, 2.4, 1.2))


def classic_picker():
    """Classic: a square field with its picker, a slider under it."""
    field = soft(box(3.0, 3.0, 21.0, 15.4), 1.0).difference(ring(15.0, 8.2, 2.2, 1.1))
    bar = capsule((3.6, 19.8), (20.4, 19.8), 1.4).difference(Point(9.0, 19.8).buffer(3.5, quad_segs=16))
    return unary_union([field, bar, Point(9.0, 19.8).buffer(2.4, quad_segs=16)])


def harmony():
    """Harmony: a disc with two opposite points joined across it."""
    a, b = (7.4, 16.6), (16.6, 7.4)
    disc = Point(12, 12).buffer(9.5, quad_segs=24).difference(LineString([a, b]).buffer(0.6))
    return disc.difference(ring(a[0], a[1], 2.2, 1.2)).difference(ring(b[0], b[1], 2.2, 1.2))


def value_sliders():
    """Value: three upright sliders, set at different heights."""
    parts = []
    for x, y in ((4.5, 14.5), (12.0, 8.5), (19.5, 12.0)):
        parts.append(capsule((x, 6.0), (x, 18.0), 0.9).difference(Point(x, y).buffer(3.6, quad_segs=16)))
        parts.append(Point(x, y).buffer(2.5, quad_segs=16))
    return unary_union(parts)


def palette():
    """Palettes: a painter's palette, its thumb hole and paints cut out."""
    from shapely.affinity import scale
    board = rotate(scale(Point(12, 12).buffer(1.0, quad_segs=32), 10.0, 8.2), -12, origin=(12, 12))
    for x, y, r in ((15.6, 15.4, 2.1), (7.0, 10.0, 1.6), (11.4, 7.0, 1.6), (16.4, 8.2, 1.6), (7.8, 15.0, 1.6)):
        board = board.difference(Point(x, y).buffer(r, quad_segs=12))
    return board


def one_sheet():
    """Layer: a single sheet."""
    return soft(Polygon([(12, 4.5), (21.4, 10.3), (12, 16.1), (2.6, 10.3)]), 1.0)


def pencil():
    """Pencil: a pencil, its point down to the left."""
    cone = soft(Polygon([(0, 0), (4.6, -2.3), (4.6, 2.3)]), 0.3)
    body = soft(box(5.6, -2.3, 17.4, 2.3), 0.4)
    end = soft(box(18.4, -2.3, 21.4, 2.3), 0.9)
    return along_diagonal(unary_union([cone, body, end]), (3.6, 20.4))


def check_in_disc():
    """Apply: a tick cut out of a disc."""
    tick = unary_union([capsule((7.2, 12.4), (10.6, 15.8), 1.2), capsule((10.6, 15.8), (17.0, 8.6), 1.2)])
    return Point(12, 12).buffer(9.0, quad_segs=24).difference(tick)


def eye():
    """Preview: an eye, its outline and its pupil."""
    lens = Point(12, 16.44).buffer(10.94, quad_segs=32).intersection(Point(12, 7.56).buffer(10.94, quad_segs=32))
    return unary_union([lens.difference(lens.buffer(-1.8)), Point(12, 12).buffer(3.4, quad_segs=16)])


def cross_in_disc():
    """Cancel: a cross cut out of a disc."""
    x = unary_union([capsule((8.2, 8.2), (15.8, 15.8), 1.2), capsule((15.8, 8.2), (8.2, 15.8), 1.2)])
    return Point(12, 12).buffer(9.0, quad_segs=24).difference(x)


def knob():
    """Adjust: a knurled knob with its pointer."""
    disc = Point(12, 12).buffer(8.6, quad_segs=32)
    for k in range(18):
        a = math.radians(20 * k)
        disc = disc.difference(Point(12 + 8.9 * math.cos(a), 12 + 8.9 * math.sin(a)).buffer(0.9, quad_segs=8))
    a = math.radians(-60)
    return disc.difference(capsule((12, 12), (12 + 5.0 * math.cos(a), 12 + 5.0 * math.sin(a)), 0.7))


def dotted_path():
    """Stroke path: a stroke's dabs, a row of dots along a curve."""
    dots = []
    for k in range(6):
        a = math.radians(180 + 90 * k / 5)
        dots.append(Point(20 + 15 * math.cos(a), 20 + 15 * math.sin(a)).buffer(1.8, quad_segs=12))
    return unary_union(dots)


def smoothed():
    """Stabilisation: a shaky line whose shake dies away."""
    pts = [(2.8 + 2.3 * k, 12.0 + 6.4 * max(0.0, 1 - k / 6) * (-1) ** k) for k in range(9)]
    return LineString(pts).buffer(1.1, join_style='round')


def spindle():
    """Taper: a stroke that thins out at both ends."""
    n = 40
    top, bottom = [], []
    for i in range(n + 1):
        t = i / n
        x, y, w = 2.0 + 20.0 * t, 12.6 - 1.2 * math.sin(math.pi * t), 2.8 * math.sin(math.pi * t) ** 0.7
        top.append((x, y - w))
        bottom.append((x, y + w))
    return Polygon(top + bottom[::-1]).buffer(0.3)


def three_shapes():
    """Shape: a circle, a square and a triangle."""
    return unary_union([Point(16.5, 7.2).buffer(4.0, quad_segs=16), soft(box(2.6, 12.6, 10.8, 20.8), 0.8),
                        soft(Polygon([(17.1, 13.0), (21.6, 21.0), (12.6, 21.0)]), 0.6)])


def grain_tile():
    """Grain: a tile pitted with holes of a few sizes."""
    tile = soft(box(3, 3, 21, 21), 1.2)
    for x, y, r in ((6.4, 6.4, 1.3), (12.0, 5.8, 0.9), (17.4, 6.8, 1.3), (8.8, 10.6, 0.9), (14.0, 10.4, 1.4),
                    (18.4, 12.4, 0.9), (5.8, 14.6, 1.4), (10.8, 15.2, 1.0), (15.6, 16.4, 1.3), (7.6, 18.8, 0.9),
                    (12.8, 18.8, 0.9)):
        tile = tile.difference(Point(x, y).buffer(r, quad_segs=10))
    return tile


def roller():
    """Rendering: a paint roller."""
    frame_ = LineString([(17.0, 6.2), (20.6, 6.2), (20.6, 12.0), (12.0, 12.0), (12.0, 14.4)]).buffer(0.9, join_style='round')
    return unary_union([soft(box(3.0, 3.0, 17.0, 9.4), 1.2), frame_, soft(box(10.0, 14.2, 14.0, 21.2), 1.2)])


def drop(cx, cy, r):
    """A drop: a circle drawn up to a point."""
    return unary_union([Point(cx, cy).buffer(r, quad_segs=20), Point(cx, cy - 2.0 * r).buffer(0.2)]).convex_hull


def two_drops():
    """Wet mix: two drops, a small one beside a large one."""
    big = drop(14.5, 14.8, 6.0)
    return unary_union([big, drop(5.5, 10.8, 3.2).difference(big.buffer(1.2))])


def colour_circles():
    """Colour dynamics: three overlapping circles, their edges cut through."""
    discs = [Point(c).buffer(5.8, quad_segs=24) for c in ((12.0, 7.4), (7.6, 15.0), (16.4, 15.0))]
    shape = unary_union(discs)
    for d in discs:
        shape = shape.difference(d.exterior.buffer(0.5))
    return shape


def chevrons():
    """Dynamics: three chevrons, heavier as they go."""
    return unary_union([LineString([(tip - 3.5, 5.0), (tip, 12.0), (tip - 3.5, 19.0)]).buffer(r, join_style='round')
                        for tip, r in ((8.6, 0.9), (14.6, 1.2), (20.6, 1.5))])


def stylus():
    """Stylus: a pen-shaped stylus, its point down to the left."""
    cone = soft(Polygon([(0, 0), (4.4, -2.0), (4.4, 2.0)]), 0.3)
    body = unary_union([box(5.4, -2.0, 19.4, 2.0), Point(19.4, 0).buffer(2.0, quad_segs=12)])
    return along_diagonal(unary_union([cone, body]), (3.4, 20.6))


def gear():
    """Properties: a gear."""
    teeth = [rotate(soft(box(10.4, 2.6, 13.6, 12.0), 0.5), 45 * k, origin=(12, 12)) for k in range(8)]
    return unary_union([Point(12, 12).buffer(6.8, quad_segs=24)] + teeth).difference(Point(12, 12).buffer(2.6, quad_segs=16))


def material_ball():
    """Materials: a ball with a rim of light round its shaded side."""
    ball = Point(12, 12).buffer(9.5, quad_segs=32)
    wedge = Polygon([(12, 12)] + [(12 + 20 * math.cos(math.radians(a)), 12 + 20 * math.sin(math.radians(a))) for a in range(-10, 101, 10)])
    return ball.difference(ring(12, 12, 7.4, 1.2).intersection(wedge))


def name_tag():
    """About this brush: a name tag, its hole and two lines of writing."""
    tag = soft(Polygon([(2.6, 12.0), (8.0, 6.0), (21.4, 6.0), (21.4, 18.0), (8.0, 18.0)]), 1.0)
    tag = tag.difference(Point(8.2, 12.0).buffer(1.6, quad_segs=12))
    return tag.difference(capsule((11.8, 10.2), (18.2, 10.2), 0.6)).difference(capsule((11.8, 13.8), (16.2, 13.8), 0.6))


def ruler():
    """Dimensions: a ruler, its marks cut in from one edge."""
    bar = soft(box(2.0, 7.5, 22.0, 16.5), 1.0)
    for i in range(6):
        x = 4.6 + 2.96 * i
        bar = bar.difference(box(x - 0.55, 6.0, x + 0.55, 7.5 + (4.4 if i % 2 == 0 else 2.8)))
    return bar


def gamut():
    """Colour profile: a triangle of colours inside the ring of all colours."""
    return unary_union([ring(12, 12, 8.6, 1.8), soft(Polygon([(12.0, 6.2), (17.4, 15.6), (6.6, 15.6)]), 0.6)])


def pie():
    """Statistics: a pie chart with one slice pulled out."""
    c = (11.0, 13.0)
    wedge = Polygon([c] + [(c[0] + 8.5 * math.cos(math.radians(a)), c[1] + 8.5 * math.sin(math.radians(a))) for a in range(-90, 1, 5)])
    body = Point(c).buffer(8.5, quad_segs=32).difference(wedge.buffer(0.6))
    return unary_union([body, translate(wedge, 1.8, -1.8)])


def new_brush():
    """New brush: the Paint brush with a plus beside it."""
    from shapely.affinity import scale
    cross = unary_union([capsule((14.6, 17.6), (21.4, 17.6), 1.2), capsule((18.0, 14.2), (18.0, 21.0), 1.2)])
    brush = scale(paintbrush(), 0.8, 0.8, origin=(2, 22)).difference(cross.buffer(1.2))
    return unary_union([brush, cross])


def folder():
    """A folder, its tab up to the left."""
    back = soft(Polygon([(2.5, 4.6), (9.6, 4.6), (11.6, 6.8), (21.5, 6.8), (21.5, 19.6), (2.5, 19.6)]), 1.0)
    return back.difference(box(0, 9.0, 24, 10.2))


def new_set():
    """New set: a folder with a plus cut out of it."""
    return folder().difference(capsule((8.6, 14.8), (15.4, 14.8), 1.1)).difference(capsule((12.0, 11.4), (12.0, 18.2), 1.1))


def chevron(angle=0):
    """A chevron pointing right (turned by `angle`)."""
    return rotate(LineString([(9.0, 4.5), (16.0, 12.0), (9.0, 19.5)]).buffer(1.3, join_style='round'), angle, origin=(12, 12))


def tick():
    """Done: a tick."""
    return LineString([(4.5, 12.5), (9.5, 17.5), (19.5, 6.5)]).buffer(1.3, join_style='round')


def full_screen():
    """Full screen: a two-headed arrow from corner to corner."""
    return double_arrow((4.5, 19.5), (19.5, 4.5), 1.2, 3.4)


def rows():
    """Compact cards: three bars, one under another."""
    return unary_union([capsule((3.8, y), (20.2, y), 1.8) for y in (6.0, 12.0, 18.0)])


def stacked_cards():
    """Stacked cards: a card in front of another."""
    front = soft(box(2.5, 8.0, 17.5, 21.5), 1.2)
    back = soft(box(6.5, 3.5, 21.5, 17.0), 1.2).difference(front.buffer(1.2))
    return unary_union([front, back])


def bin_():
    """Clear the history: a bin, its lid and two slots."""
    lid = unary_union([capsule((4.0, 6.4), (20.0, 6.4), 1.1), frame(9.2, 2.6, 14.8, 6.4, 1.4, 0.8)])
    body = soft(Polygon([(5.4, 8.8), (18.6, 8.8), (17.4, 21.2), (6.6, 21.2)]), 1.0)
    body = body.difference(capsule((10.0, 11.6), (10.0, 18.2), 0.6)).difference(capsule((14.0, 11.6), (14.0, 18.2), 0.6))
    return unary_union([lid, body])


def keyboard():
    """Keyboard: a keyboard, its keys and space bar cut out."""
    board = soft(box(2.0, 5.5, 22.0, 18.5), 1.4)
    for y in (9.0, 12.2):
        for k in range(5):
            x = 5.5 + 3.25 * k
            board = board.difference(soft(box(x - 0.8, y - 0.8, x + 0.8, y + 0.8), 0.2))
    return board.difference(capsule((7.5, 15.4), (16.5, 15.4), 0.8))


def guided_line():
    """Assisted drawing: a straight line between two handles."""
    return unary_union([ring(5.0, 19.0, 2.6, 1.8), ring(19.0, 5.0, 2.6, 1.8), capsule((7.2, 16.8), (16.8, 7.2), 1.2)])


def eyedropper():
    """Eyedropper: a dropper, its tip down to the left."""
    tip = capsule((0.4, 0), (5.2, 0), 0.9)
    glass = soft(box(5.0, -1.9, 12.6, 1.9), 0.4)
    collar = soft(box(13.6, -3.0, 15.2, 3.0), 0.4)
    bulb = capsule((15.8, 0), (20.4, 0), 2.4)
    return along_diagonal(unary_union([tip, glass, collar, bulb]), (3.4, 20.6))


def radial_menu():
    """QuickMenu: six buttons round a centre."""
    dots = [Point(12, 12).buffer(2.4, quad_segs=12)]
    for k in range(6):
        a = math.radians(-90 + 60 * k)
        dots.append(Point(12 + 7.0 * math.cos(a), 12 + 7.0 * math.sin(a)).buffer(2.4, quad_segs=12))
    return unary_union(dots)


def hovering_pen():
    """Hover: a stylus held just above the glass."""
    body = unary_union([box(9.2, 4.6, 14.8, 13.6), Point(12.0, 4.6).buffer(2.8, quad_segs=12)]).difference(box(0, 6.6, 24, 7.6))
    cone = soft(Polygon([(9.2, 14.6), (14.8, 14.6), (12.0, 19.0)]), 0.3)
    return unary_union([body, cone, capsule((3.0, 21.6), (21.0, 21.6), 0.9)])


def scissors():
    """Cut: scissors, open, handles down."""
    parts = [ring(7.0, 18.0, 2.6, 1.8), ring(17.0, 18.0, 2.6, 1.8)]
    for (bx, by), (tx, ty) in (((8.4, 15.4), (17.2, 2.6)), ((15.6, 15.4), (6.8, 2.6))):
        ux, uy = tx - bx, ty - by
        n = math.hypot(ux, uy)
        px, py = -uy / n, ux / n
        parts.append(soft(Polygon([(bx + px * 1.5, by + py * 1.5), (tx + px * 0.4, ty + py * 0.4),
                                   (tx - px * 0.4, ty - py * 0.4), (bx - px * 1.5, by - py * 1.5)]), 0.3))
    return unary_union(parts).difference(Point(12.0, 10.2).buffer(0.8, quad_segs=8))


def duplicate():
    """Duplicate: two sheets, a plus cut out of the front one."""
    return copy_paste().difference(capsule((11.6, 14.5), (17.4, 14.5), 1.1)).difference(capsule((14.5, 11.6), (14.5, 17.4), 1.1))


def three_sheets():
    """Copy all: three sheets, one behind another."""
    s1, s2, s3 = (soft(box(8.5, 2.5, 21.5, 15.5), 1.4), soft(box(5.5, 5.5, 18.5, 18.5), 1.4),
                  soft(box(2.5, 8.5, 15.5, 21.5), 1.4))
    return unary_union([s1.difference(s2.buffer(1.2)), s2.difference(s3.buffer(1.2)), s3])


def lifted_sheet():
    """Cut and paste: a sheet with an arrow leaping off it."""
    return unary_union([soft(box(2.5, 12.0, 12.5, 21.5), 1.2), arc_arrow(15.0, 12.0, 7.0, -160, -18)])


def clipboard():
    """Paste: a clipboard, its clip and two lines of writing."""
    clip = soft(box(8.5, 2.5, 15.5, 6.6), 0.8)
    board = soft(box(4.0, 4.5, 20.0, 21.5), 1.4).difference(clip.buffer(1.2))
    board = board.difference(capsule((8.0, 11.8), (16.0, 11.8), 0.6)).difference(capsule((8.0, 15.8), (14.0, 15.8), 0.6))
    return unary_union([board, clip])


def aligned(how):
    """Text alignment: four lines of text, long and short, set left, centre, right or justified."""
    lines = []
    for k, y in enumerate((5.0, 9.67, 14.33, 19.0)):
        w = 18.0 if how == 'justify' or k % 2 == 0 else 11.0
        x0 = {'left': 3.0, 'center': 12.0 - w / 2, 'right': 21.0 - w, 'justify': 3.0}[how]
        lines.append(capsule((x0 + 1.2, y), (x0 + w - 1.2, y), 1.2))
    return unary_union(lines)


def underlined():
    """Underline: a block of text with a line under it."""
    return unary_union([soft(box(6.0, 3.0, 18.0, 14.2), 1.4), capsule((4.4, 19.4), (19.6, 19.4), 1.4)])


def outlined():
    """Outline: a shape with its outline drawn round it."""
    return unary_union([Point(12, 12).buffer(5.8, quad_segs=20), ring(12, 12, 9.2, 1.6)])


def vertical_text():
    """Vertical text: three columns of writing, the first running down to an arrowhead."""
    cols = [capsule((17.0, 3.0), (17.0, 14.6), 1.2), capsule((12.0, 3.0), (12.0, 21.0), 1.2),
            capsule((7.0, 3.0), (7.0, 16.0), 1.2)]
    head = soft(Polygon([(17.0, 21.4), (13.8, 16.6), (20.2, 16.6)]), 0.4)
    return unary_union(cols + [head])


def line_shape():
    """Line: a line between its two end points."""
    return unary_union([capsule((5.0, 19.0), (19.0, 5.0), 1.1), Point(5.0, 19.0).buffer(2.4, quad_segs=12),
                        Point(19.0, 5.0).buffer(2.4, quad_segs=12)])


def polyline_shape():
    """Polyline: three joined lines, a dot at each corner."""
    pts = [(3.5, 18.5), (9.0, 8.0), (15.0, 16.0), (20.5, 5.5)]
    return unary_union([LineString(pts).buffer(1.1, join_style='round')] + [Point(p).buffer(2.2, quad_segs=12) for p in pts])


def solid_ellipse():
    """Ellipse: a solid ellipse."""
    from shapely.affinity import scale
    return scale(Point(12, 12).buffer(1.0, quad_segs=32), 9.5, 6.0)


def solid_circle():
    """Circle: a solid circle."""
    return Point(12, 12).buffer(9.0, quad_segs=32)


def solid_triangle(equal):
    """Triangle: a solid triangle, wide (or with equal sides)."""
    pts = [(12.0, 4.2), (21.0, 19.8), (3.0, 19.8)] if equal else [(12.0, 6.0), (22.0, 18.5), (2.0, 18.5)]
    return soft(Polygon(pts), 1.0)


def solid_quad():
    """Quadrilateral: a solid trapezoid."""
    return soft(Polygon([(6.5, 5.5), (17.5, 5.5), (22.0, 18.5), (2.0, 18.5)]), 1.0)


def solid_rect(square):
    """Rectangle (or square): a solid one, its corners rounded."""
    return soft(box(3.0, 3.0, 21.0, 21.0) if square else box(2.0, 6.5, 22.0, 17.5), 1.4)


def grid_guide():
    """Grid: nine squares in three rows."""
    return unary_union([soft(box(x, y, x + 5.2, y + 5.2), 0.6) for x in (3.0, 9.4, 15.8) for y in (3.0, 9.4, 15.8)])


def iso_cube():
    """Isometric: a cube, its three faces apart."""
    h = 5.2
    top = soft(Polygon([(12, 2.6), (20.2, 7.3), (12, 12.0), (3.8, 7.3)]), 0.6)
    left = soft(Polygon([(3.2, 8.7), (11.3, 13.4), (11.3, 21.6), (3.2, 16.9)]), 0.6)
    right = soft(Polygon([(20.8, 8.7), (12.7, 13.4), (12.7, 21.6), (20.8, 16.9)]), 0.6)
    return unary_union([top, left, right])


def symmetry():
    """Symmetry: a wing and its mirror image across a dashed axis."""
    from shapely.affinity import scale
    upper = soft(Polygon([(9.4, 11.0), (9.4, 7.0), (4.6, 2.8), (2.2, 5.0), (3.0, 11.0)]), 1.0)
    lower = soft(Polygon([(9.4, 13.2), (9.4, 17.6), (6.2, 21.2), (3.0, 19.6), (3.8, 13.2)]), 1.0)
    wing = unary_union([upper, lower])
    return unary_union([wing, scale(wing, -1, 1, origin=(12, 12)), dashes([(12, 2.4), (12, 21.6)], False, 2.0, 1.6, 0.6)])


def wisps():
    """Clouds (noise): wisps of mist drifting across."""
    return unary_union([capsule((3.0, 7.0), (17.0, 7.0), 1.6), capsule((7.0, 12.0), (21.0, 12.0), 1.6),
                        capsule((3.0, 17.0), (13.6, 17.0), 1.6), Point(17.6, 17.0).buffer(1.6, quad_segs=12)])


def billows():
    """Billows: two rows of rounded humps."""
    rows_ = []
    for y, xs in ((8.6, (5.0, 12.0, 19.0)), (16.6, (5.0, 12.0, 19.0))):
        rows_.append(unary_union([Point(x, y).buffer(3.0, quad_segs=16) for x in xs] + [box(2.0, y, 22.0, y + 2.6)]))
    return unary_union(rows_)


def ridges():
    """Ridges: a range of sharp peaks."""
    return soft(Polygon([(2.0, 19.0), (8.0, 7.0), (11.5, 12.5), (15.5, 5.0), (22.0, 19.0)]), 0.8)


def halftone_full():
    """Full colour halftone: big and small dots in a checker."""
    return unary_union([Point(x, y).buffer(2.6 if (i + j) % 2 == 0 else 1.4, quad_segs=12)
                        for i, x in enumerate((4.5, 12.0, 19.5)) for j, y in enumerate((4.5, 12.0, 19.5))])


def halftone_screen():
    """Screen halftone: dots set out in a diamond, the screen's angle."""
    return unary_union([Point(12 + 4.6 * i, 12 + 4.6 * j).buffer(1.6, quad_segs=10)
                        for i in range(-2, 3) for j in range(-2, 3) if abs(i) + abs(j) <= 2])


def newspaper():
    """Newspaper halftone: a page with a picture and columns of text."""
    page = soft(box(3.0, 3.5, 21.0, 20.5), 1.2).difference(soft(box(5.6, 6.0, 11.2, 11.6), 0.4))
    for a, b, y in ((13.2, 18.4, 7.0), (13.2, 17.0, 10.4), (5.6, 18.4, 14.4), (5.6, 15.0, 17.6)):
        page = page.difference(capsule((a, y), (b, y), 0.6))
    return page


def outward_arrows():
    """Perspective (chromatic): four arrows pointing out from a centre."""
    parts = [Point(12, 12).buffer(2.2, quad_segs=12)]
    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        px, py = -dy, dx
        parts.append(capsule((12 + dx * 4.4, 12 + dy * 4.4), (12 + dx * 7.6, 12 + dy * 7.6), 1.0))
        tip, base = (12 + dx * 10.6, 12 + dy * 10.6), (12 + dx * 7.4, 12 + dy * 7.4)
        parts.append(soft(Polygon([tip, (base[0] + px * 2.6, base[1] + py * 2.6), (base[0] - px * 2.6, base[1] - py * 2.6)]), 0.3))
    return unary_union(parts)


def displaced():
    """Displace: a disc with its outline copy shifted off it."""
    front = Point(9.4, 13.6).buffer(6.4, quad_segs=24)
    return unary_union([front, ring(14.6, 10.4, 6.4, 1.6).difference(front.buffer(1.2))])


def blocky():
    """Artifact: a mosaic of blocks of all sizes."""
    blocks = [(2.5, 4.0, 9.0, 10.5), (10.2, 4.0, 14.0, 7.8), (15.2, 4.0, 21.5, 8.5), (10.2, 9.0, 14.0, 13.0),
              (15.2, 9.7, 19.0, 13.5), (2.5, 11.7, 7.0, 16.0), (8.2, 14.2, 16.0, 20.0), (17.2, 14.7, 21.5, 20.0),
              (2.5, 17.2, 7.0, 20.0)]
    return unary_union([soft(box(*b), 0.4) for b in blocks])


def square_wave():
    """Wave (glitch): a square wave."""
    return LineString([(2.5, 15.0), (6.5, 15.0), (6.5, 8.0), (11.0, 8.0), (11.0, 16.5), (14.5, 16.5), (14.5, 6.5),
                       (18.0, 6.5), (18.0, 13.0), (21.5, 13.0)]).buffer(1.2, join_style='round')


def static_screen():
    """Signal: a screen with broken lines across it."""
    parts = [frame(2.0, 4.0, 22.0, 20.0, 1.6, 1.6)]
    for y, cuts in ((8.4, ((5.6, 10.0), (12.2, 18.4))), (12.0, ((5.6, 7.0), (9.2, 18.4))), (15.6, ((5.6, 14.0), (16.2, 18.4)))):
        parts += [capsule((a, y), (b, y), 0.8) for a, b in cuts]
    return unary_union(parts)


def fork():
    """Diverge: a line that splits in two."""
    ends = [(12.0, 20.6), (5.6, 4.6), (18.4, 4.6)]
    return unary_union([capsule((12.0, 12.6), e, 1.2) for e in ends] + [Point(e).buffer(2.2, quad_segs=12) for e in ends])


def moving_grain():
    """Moving grain: two rows of grain and an arrow along under them."""
    dots = [Point(x, y).buffer(1.6, quad_segs=10) for x in (4.5, 9.5, 14.5, 19.5) for y in (5.0, 10.0)]
    arrow = [capsule((3.5, 17.5), (16.0, 17.5), 1.2), soft(Polygon([(21.5, 17.5), (16.4, 14.5), (16.4, 20.5)]), 0.4)]
    return unary_union(dots + arrow)


def painted_on():
    """Painting an adjustment on: the Paint brush with an adjustment ring."""
    from shapely.affinity import scale
    return unary_union([scale(paintbrush(), 0.86, 0.86, origin=(2, 22)), ring(6.0, 6.0, 3.0, 1.8)])


DRAWINGS = {
    'actions': wrench, 'adjustments': wand, 'selection': selection, 'transform': arrow,
    'brush': paintbrush, 'smudge': fingertip, 'erase': eraser, 'layers': layers,
    'undo': lambda: turning_arrow(False), 'redo': lambda: turning_arrow(True),
    'tabadd': plus, 'tabcanvas': canvas_tab, 'tabshare': share_tab, 'tabvideo': video_tab,
    'tabprefs': prefs_tab, 'tabhelp': help_tab,
    'automatic': wand, 'freehand': lasso, 'rectangleshape': rectangle_tool, 'ellipseshape': ellipse_tool,
    'selectadd': lambda: sign_in_disc(False), 'selectremove': lambda: sign_in_disc(True), 'invert': invert,
    'selectcopypaste': copy_paste, 'feather': feather, 'saveload': save_load, 'colorfill': paint_bucket,
    'clearselection': cross,
    'freeform': freeform, 'uniform': uniform, 'distort': distort, 'warp': warp, 'snapping': magnet,
    'fliphorizontal': lambda: flip(False), 'flipvertical': lambda: flip(True), 'rotate45': rotate_step,
    'fittocanvas': fit_to_canvas, 'interpolation': interpolation, 'transformreset': reset_transform,
    'adjusthsb': colour_wheel, 'adjustbalance': scales, 'adjustcurves': curve_graph, 'adjustgradient': gradient_bars,
    'adjustblur': dot_halo, 'adjustmotion': speeding_disc, 'adjustperspective': converging, 'adjustnoise': scattered_dots,
    'adjustsharpen': gem, 'adjustbloom': sun, 'adjustglitch': glitch_bands, 'adjusthalftone': halftone,
    'adjustchromatic': fringed_lens, 'adjustliquify': spiral, 'adjustclone': stamp,
    'modedisc': colour_disc, 'modeclassic': classic_picker, 'modeharmony': harmony, 'modevalue': value_sliders,
    'modepalettes': palette, 'modelayer': one_sheet, 'modepencil': pencil, 'apply': check_in_disc, 'preview': eye,
    'canceladjust': cross_in_disc, 'pieundo': lambda: turning_arrow(False), 'piereset': reset_transform,
    'adjustknob': knob,
    'studiostroke': dotted_path, 'studiostabilize': smoothed, 'studiotaper': spindle, 'studioshape': three_shapes,
    'studiograin': grain_tile, 'studiorendering': roller, 'studiowet': two_drops, 'studiocolor': colour_circles,
    'studiodynamics': chevrons, 'studiostylus': stylus, 'studioproperties': gear, 'studiomaterials': material_ball,
    'studiopreview': eye, 'studioabout': name_tag,
    'infoabout': name_tag, 'infodimensions': ruler, 'infolayers': layers, 'infoprofile': gamut, 'infovideo': video_tab,
    'infostatistics': pie, 'menunewbrush': new_brush, 'menunewset': new_set, 'menushare': share_tab,
    'menuchevron': chevron, 'disclose': lambda: chevron(90), 'donetick': tick, 'add': plus, 'fullscreen': full_screen,
    'cardcompact': rows, 'cardstack': stacked_cards, 'folder-fill': folder, 'sweep': bin_, 'keyboard': keyboard,
    'gesturegeneral': prefs_tab, 'gesturecopypaste': copy_paste, 'gestureassisted': guided_line,
    'gestureeyedropper': eyedropper, 'gesturequickshape': three_shapes, 'gesturequickmenu': radial_menu,
    'gesturefullscreen': full_screen, 'gestureclearlayer': bin_, 'gesturelayerselect': layers, 'gesturehover': hovering_pen,
    'copycut': scissors, 'copyduplicate': duplicate, 'copyone': copy_paste, 'copyall': three_sheets,
    'cutpaste': lifted_sheet, 'pastedown': clipboard,
    'textcut': scissors, 'textcopy': copy_paste, 'textpaste': clipboard, 'alignleft': lambda: aligned('left'),
    'aligncenter': lambda: aligned('center'), 'alignright': lambda: aligned('right'),
    'alignjustify': lambda: aligned('justify'), 'textunderline': underlined, 'textoutline': outlined,
    'textvertical': vertical_text, 'shapeline': line_shape, 'shapepolyline': polyline_shape,
    'shapeellipse': solid_ellipse, 'shapecircle': solid_circle, 'shapetriangle': lambda: solid_triangle(False),
    'shapeequilateral': lambda: solid_triangle(True), 'shapequad': solid_quad, 'shaperect': lambda: solid_rect(False),
    'shapesquare': lambda: solid_rect(True), 'guidegrid': grid_guide, 'guideisometric': iso_cube, 'guidesymmetry': symmetry,
    'noiseclouds': wisps, 'noisebillows': billows, 'noiseridges': ridges, 'halftonefull': halftone_full,
    'halftonescreen': halftone_screen, 'halftonenews': newspaper, 'chromaperspective': outward_arrows,
    'chromadisplace': displaced, 'glitchartifact': blocky, 'glitchwave': square_wave, 'glitchsignal': static_screen,
    'glitchdiverge': fork, 'grainmoving': moving_grain, 'graintexturized': grain_tile, 'brushpaintedon': painted_on,
    'swipe-share': share_tab, 'swipe-duplicate': duplicate,
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
