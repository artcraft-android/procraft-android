"""Paints "Dusk" in ProCraft on the tablet, start to finish, the way a person would: a new canvas,
a sky filled and airbrushed, a sun drawn with QuickShape and filled with ColorDrop, two ranges of
hills filled with ColorDrop, pines, stars with Bloom, a look at the layers, back to the gallery.

usage: python -I paint_dusk.py [stage ...]   (stages: new sky sun hills trees stars bloom tour gallery)
"""
import math
import os
import random
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import drive as d  # noqa: E402

random.seed(7)


def hill(x, base, a1, f1, p1, a2, f2, p2):
    return base + a1 * math.sin(f1 * x / 2560 * 2 * math.pi + p1) + a2 * math.sin(f2 * x / 2560 * 2 * math.pi + p2)


FAR = dict(base=1150, a1=70, f1=1.3, p1=0.6, a2=28, f2=3.7, p2=1.9)
NEAR = dict(base=1330, a1=55, f1=0.9, p1=2.4, a2=22, f2=4.3, p2=0.4)
SUN = (1760, 1010, 118)


def new_canvas():
    d.tap(2476, 65, wait=1.0)
    at = d.bounds_of("Screen Size")
    d.tap(*at, wait=2.5)


def sky():
    d.brush("Airbrushing", 0)
    d.size(1.0)
    d.opacity(1.0)
    d.colour("1E1B4B", wait=1.0)
    d.colour_drop(1280, 700)
    for hexcode, ys in (("4A2A7E", (560, 660)), ("B33F7C", (860, 950)), ("F07F4F", (1090, 1160)),
                        ("FFC46B", (1270, 1340))):
        d.colour(hexcode, wait=0.5)
        pen = d.Pen()
        for k, y in enumerate(ys):
            # start clear of the sidebar (left) and the system's edge handle (right, 820-1040)
            x0, x1 = (140, 2552) if 820 <= y <= 1040 else (2552, 8)
            pen.stroke(d.wave(x0, x1, y, 26, 1.2 + 0.3 * k, 0.78, n=110, phase=k * 1.7), step_ms=7)
        pen.play("sky")


def line(a, b, step=26, p=0.85):
    n = max(2, int(math.hypot(b[0] - a[0], b[1] - a[1]) / step))
    return [(a[0] + (b[0] - a[0]) * i / n, a[1] + (b[1] - a[1]) * i / n, p) for i in range(1, n + 1)]


def sun():
    cx, cy, r = SUN
    # a glow on its own layer, soft, round the sun's place
    d.new_layer()
    d.size(0.6)
    d.colour("FFD9A0", wait=0.5)
    pen = d.Pen()
    for k, rr in enumerate((r + 50, r + 95, r + 150)):
        pen.stroke([(cx + rr * math.cos(t), cy + rr * math.sin(t), 0.42 - 0.1 * k)
                    for t in [i / 90 * 2 * math.pi for i in range(91)]], step_ms=8)
    pen.play("glow")
    d.new_layer()
    d.colour("FFF1D2", wait=0.5)
    # the disc: a circle held at its end so QuickShape makes it round, then filled by ColorDrop
    d.brush("Inking", 0)
    d.size(0.25)
    pts = [(cx + r * math.cos(t) * (1 + 0.03 * math.sin(3 * t)), cy + r * math.sin(t) * (1 + 0.03 * math.cos(2 * t)), 0.7)
           for t in [i / 90 * 2 * math.pi * 1.02 - math.pi / 2 for i in range(91)]]
    pts += [pts[-1]] * 110
    d.Pen().stroke(pts, step_ms=8).play("circle")
    time.sleep(0.8)
    d.colour_drop(cx, cy)


def hills():
    for hexcode, shape, drop_y in (("7B3F8E", FAR, 1530), ("3B2056", NEAR, 1575)):
        d.new_layer()
        d.colour(hexcode, wait=1.0)
        # a closed outline just inside the canvas: the hill's crest right to left, then down,
        # along the bottom and back up, so ColorDrop has a sealed shape to fill
        crest = [(x, hill(x, **shape), 0.85) for x in range(2552, 7, -22)] + [(8, hill(8, **shape), 0.85)]
        pts = crest + line((8, crest[-1][1]), (8, 1590)) + line((8, 1590), (2552, 1590)) + line((2552, 1590), crest[0][:2])
        d.Pen().stroke(pts, step_ms=7).play("hill")
        time.sleep(0.4)
        d.colour_drop(1280, drop_y)


def pine(pen, x, base, h):
    # a trunk, then branches zigzagging out wider as they come down
    pen.stroke([(x, base + 6, 0.9), (x, base - h, 0.5)], step_ms=30)
    pts = []
    rows = max(8, int(h / 9))
    for i in range(rows + 1):
        t = i / rows
        y = base - h + 8 + t * (h - 18)
        w = 4 + t * h * 0.30
        side = -1 if i % 2 == 0 else 1
        pts.append((x + side * w, y, 0.45 + 0.4 * t))
    pen.stroke(pts, step_ms=22)


def trees():
    d.new_layer()
    d.colour("150B22", wait=0.5)
    d.size(0.55)
    pen = d.Pen()
    for x, h in ((210, 210), (300, 160), (380, 250), (470, 180), (1090, 150), (1160, 205), (1240, 130),
                 (2050, 230), (2140, 170), (2230, 260), (2330, 190), (2420, 150)):
        pine(pen, x, hill(x, **NEAR) + 30, h)
    pen.play("trees")
    # birds near the sun, with a fine brush
    d.size(0.10)
    pen = d.Pen()
    for bx, by, s in ((1500, 760, 26), (1560, 720, 20), (1620, 790, 16)):
        pen.stroke([(bx - s, by - s * 0.4, 0.4), (bx - s * 0.5, by - s * 0.6, 0.5), (bx, by, 0.6),
                    (bx + s * 0.5, by - s * 0.6, 0.5), (bx + s, by - s * 0.4, 0.4)], step_ms=25)
    pen.play("birds")


def stars():
    d.new_layer()
    d.brush("Airbrushing", 1)
    d.size(0.14)
    d.colour("FFFFFF", wait=0.5)
    pen = d.Pen()
    for _ in range(46):
        x = random.uniform(90, 2500)
        y = random.uniform(120, 560) if random.random() < 0.8 else random.uniform(560, 760)
        if (x - SUN[0]) ** 2 + (y - SUN[1]) ** 2 < 330 ** 2:
            continue
        p = random.uniform(0.35, 1.0)
        pen.stroke([(x, y, p)] * 4, step_ms=8)
    pen.play("stars")


def bloom():
    d.tap(*d.ADJUST, wait=0.8)
    d.tap(300, 1122, wait=1.2)
    d.swipe(700, 800, 1900, 800, 1600, wait=1.2)
    d.tap(*d.ADJUST, wait=1.0)


def tour():
    d.tap(*d.LAYERS, wait=2.6)
    d.tap(*d.LAYERS, wait=0.8)


def gallery():
    d.tap(136, 42, wait=2.5)


STAGES = {"new": new_canvas, "sky": sky, "sun": sun, "hills": hills, "trees": trees, "stars": stars,
          "bloom": bloom, "tour": tour, "gallery": gallery}

if __name__ == "__main__":
    for name in (sys.argv[1:] or list(STAGES)):
        t = time.time()
        STAGES[name]()
        print(f"{name}: {time.time() - t:.1f} s", flush=True)
