"""Paints "Dusk" in ProCraft on the tablet at a person's pace, for the README's demo: menus held
open long enough to read, colours picked on the Classic square, strokes that ease in and out
with a wobble and real pressure, pauses between moves. Each span of time is logged as
"menu", "draw" or (between them) idle, so the video can play menus at full speed, drawing
faster, and cut the idle time down.

usage: python -I paint_dusk2.py <segments.json> [stage ...]
"""
import colorsys
import contextlib
import json
import math
import os
import random
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import drive as d  # noqa: E402
import human  # noqa: E402

random.seed(7)
LOG = []
T0 = time.time()


@contextlib.contextmanager
def seg(kind, label=""):
    t = time.time() - T0
    yield
    LOG.append({"kind": kind, "start": round(t, 3), "end": round(time.time() - T0, 3), "label": label})


def hold(s):
    time.sleep(s)


# --- the interface, at a person's pace -------------------------------------------------------

CLASSIC_HUE = (1830, 2488, 944)           # the hue strip: x from, x to, y
CLASSIC_FIELD = (1824, 2494, 250, 900)    # saturation across, brightness down
# the hue strip, measured: (x, the hue a tap there picks); its hues aren't spread evenly
HUE_AT = [(1836, 0.4), (1900, 15.0), (1980, 40.8), (2060, 94.2), (2140, 124.2), (2220, 180.8), (2300, 226.2),
          (2380, 273.1), (2440, 310.0), (2480, 348.5)]


def hue_x(hue):
    pts = HUE_AT + [(2492, 360.0)]
    for (xa, ha), (xb, hb) in zip(pts, pts[1:]):
        if ha <= hue <= hb:
            return xa + (xb - xa) * (hue - ha) / (hb - ha)
    return pts[0][0]


_classic_chosen = False
_size = None


def pick(hexcode, label=""):
    """Opens the colour panel, picks the colour on the Classic square (hue strip, then the
    square), lets it be seen, closes the panel."""
    global _classic_chosen
    r, g, b = (int(hexcode[i:i + 2], 16) / 255 for i in (0, 2, 4))
    h, s, v = colorsys.rgb_to_hsv(r, g, b)
    with seg("menu", f"colour {label or hexcode}"):
        if not d.panel_open():
            d.tap(*d.COLOUR, wait=1.0)
        if not _classic_chosen:
            at = d.bounds_of("Classic")
            if at:
                d.tap(at[0], at[1] - 34, wait=1.0)
            _classic_chosen = True
        d.tap(hue_x(h * 360), CLASSIC_HUE[2], wait=0.8)
        fx0, fx1, fy0, fy1 = CLASSIC_FIELD
        d.tap(fx0 + s * (fx1 - fx0), fy0 + (1 - v) * (fy1 - fy0), wait=1.4)
        d.tap(*d.COLOUR, wait=0.6)


def brush(set_name, index, label=""):
    global _size
    # each brush keeps its own size: the slider's place is unknown again
    _size = None
    with seg("menu", f"brush {label}"):
        if not d.library_open():
            d.tap(*d.PAINT, wait=1.2)
            if not d.library_open():
                d.tap(*d.PAINT, wait=1.2)
        hold(0.6)
        d.tap(1620, d.SETS[set_name], wait=1.3)
        d.tap(2000, 300 + 176 * index, wait=1.4)
        tree = d.ui()
        if 'text="Brush Studio"' in tree:
            at = d.bounds_of("Cancel", tree)
            d.tap(*(at or (2333, 84)), wait=0.8)
            tree = d.ui()
        if 'text="Brush library"' in tree:
            d.tap(*d.PAINT, wait=0.6)


def size(frac):
    """Drags the size slider's thumb from where it is to `frac` of the track, as a hand would."""
    global _size
    x, top, bottom = d.SIZE_TRACK
    ty = bottom - frac * (bottom - top)
    with seg("menu", f"size {frac}"):
        if _size is None:
            d.swipe(x, top + 10, x, bottom + 300, 300, wait=0.2)
            d.swipe(x, bottom, x, ty, 700, wait=0.6)
        else:
            d.swipe(x, bottom - _size * (bottom - top), x, ty, 700, wait=0.6)
    _size = frac


def new_layer():
    with seg("menu", "new layer"):
        d.tap(*d.LAYERS, wait=1.2)
        d.tap(2460, 176, wait=1.5)
        d.tap(*d.LAYERS, wait=0.5)


def colour_drop(x, y):
    with seg("menu", "drop colour"):
        d.swipe(2432, 74, x, y, 1300, wait=1.6)


# --- strokes -----------------------------------------------------------------------------------

def draw(strokes, label=""):
    """strokes: [(path, seconds)] or [(path, seconds, options for human.stroke)]; played as one pen
    session with a person's pauses between."""
    pen = d.Pen()
    for i, stroke in enumerate(strokes):
        path, seconds = stroke[0], stroke[1]
        pts, dts = human.stroke(path, seconds, **(stroke[2] if len(stroke) > 2 else {}))
        pen.stroke(pts, dts=dts, hover_ms=int(human.rng.uniform(120, 260)))
        if i < len(strokes) - 1:
            pen.pause(int(human.pause() * 1000))
    local = os.path.join(d.HERE, "pen.json")
    pen.s.write(local)
    d.adb("push", local, "/data/local/tmp/pen.json")
    with seg("draw", label):
        d.sh("hid /data/local/tmp/pen.json")


def hill(x, base, a1, f1, p1, a2, f2, p2):
    return base + a1 * math.sin(f1 * x / 2560 * 2 * math.pi + p1) + a2 * math.sin(f2 * x / 2560 * 2 * math.pi + p2)


def line(a, b, step=24, p=0.85):
    n = max(2, int(math.hypot(b[0] - a[0], b[1] - a[1]) / step))
    return [(a[0] + (b[0] - a[0]) * i / n, a[1] + (b[1] - a[1]) * i / n, p) for i in range(1, n + 1)]


# an outline that a fill will need closed: no wobble, no thinning at its ends
SEALED = dict(wobble=0.0, taper_in=0.0, taper_out=0.0)
FAR = dict(base=1150, a1=70, f1=1.3, p1=0.6, a2=28, f2=3.7, p2=1.9)
NEAR = dict(base=1330, a1=55, f1=0.9, p1=2.4, a2=22, f2=4.3, p2=0.4)
SUN = (1760, 1010, 118)


# --- the painting ------------------------------------------------------------------------------

def new_canvas():
    with seg("menu", "new canvas"):
        hold(0.8)
        at = None
        for _ in range(3):
            if 'text="New canvas"' not in d.ui():
                d.tap(2476, 65, wait=1.6)
            at = d.bounds_of("Screen Size")
            if at:
                break
        hold(0.6)
        d.tap(*at, wait=2.4)


def sky():
    brush("Airbrushing", 0, "Soft Airbrush")
    size(1.0)
    with seg("menu", "opacity"):
        d.opacity(1.0)
    pick("1E1B4B", "indigo")
    colour_drop(1280, 700)
    for hexcode, ys in (("4A2A7E", (560, 680)), ("B33F7C", (880,)), ("F07F4F", (1100, 1180)), ("FFC46B", (1320,))):
        pick(hexcode)
        strokes = []
        for k, y in enumerate(ys):
            x0, x1 = (140, 2552) if 820 <= y <= 1040 else (2552, 8)
            strokes.append((d.wave(x0, x1, y, 26, 1.2 + 0.3 * k, 0.8, n=60, phase=k * 1.7), 1.7))
        draw(strokes, f"sky {hexcode}")


def sun():
    cx, cy, r = SUN
    new_layer()
    size(0.6)
    pick("FFD9A0", "glow")
    rings = []
    for k, rr in enumerate((r + 50, r + 100, r + 150)):
        rings.append(([(cx + rr * math.cos(t), cy + rr * math.sin(t), 0.45 - 0.1 * k)
                       for t in [i / 60 * 2 * math.pi for i in range(61)]], 1.5))
    draw(rings, "glow")
    new_layer()
    pick("FFF1D2", "sun")
    brush("Inking", 0, "Round Pen")
    size(0.25)
    pts = [(cx + r * math.cos(t) * (1 + 0.03 * math.sin(3 * t)), cy + r * math.sin(t) * (1 + 0.03 * math.cos(2 * t)), 0.7)
           for t in [i / 60 * 2 * math.pi * 1.1 - math.pi / 2 for i in range(61)]]
    path, dts = human.stroke(pts, 1.5, **SEALED)
    # held still at the end, so the circle snaps round
    pen = d.Pen()
    pen.stroke(path + [path[-1]] * 70, dts=dts + [12] * 70, hover_ms=200)
    local = os.path.join(d.HERE, "pen.json")
    pen.s.write(local)
    d.adb("push", local, "/data/local/tmp/pen.json")
    with seg("draw", "sun circle"):
        d.sh("hid /data/local/tmp/pen.json")
        hold(1.0)
    colour_drop(cx, cy)


def hills():
    for hexcode, shape, drop_y, label in (("7B3F8E", FAR, 1530, "far hills"), ("3B2056", NEAR, 1575, "near hills")):
        new_layer()
        pick(hexcode, label)
        crest = [(x, hill(x, **shape), 0.85) for x in range(2552, 7, -22)] + [(8, hill(8, **shape), 0.85)]
        outline = crest + line((8, crest[-1][1]), (8, 1590)) + line((8, 1590), (2552, 1590)) + line((2552, 1590), crest[0][:2])
        draw([(outline + outline[1:6], 4.4, SEALED)], label)
        colour_drop(1280, drop_y)


def pine(x, base, h):
    rows = max(8, int(h / 9))
    zig = []
    for i in range(rows + 1):
        t = i / rows
        side = -1 if i % 2 == 0 else 1
        zig.append((x + side * (4 + t * h * 0.30), base - h + 8 + t * (h - 18), 0.5 + 0.4 * t))
    return [([(x, base + 6, 0.9), (x, base - h, 0.6)], 0.35), (zig, 0.9)]


def trees():
    new_layer()
    pick("150B22", "pines")
    size(0.55)
    for group in (((210, 210), (300, 160), (380, 250), (470, 180)), ((1090, 150), (1160, 205), (1240, 130)),
                  ((2050, 230), (2140, 170), (2230, 260), (2330, 190), (2420, 150))):
        strokes = []
        for x, h in group:
            strokes += pine(x, hill(x, **NEAR) + 30, h)
        draw(strokes, "pines")
    size(0.10)
    birds = []
    for bx, by, s in ((1500, 760, 26), (1560, 720, 20), (1620, 790, 16)):
        birds.append(([(bx - s, by - s * 0.4, 0.4), (bx - s * 0.5, by - s * 0.6, 0.5), (bx, by, 0.6),
                       (bx + s * 0.5, by - s * 0.6, 0.5), (bx + s, by - s * 0.4, 0.4)], 0.45))
    draw(birds, "birds")


def stars():
    new_layer()
    brush("Airbrushing", 1, "Hard Airbrush")
    size(0.14)
    pick("FFFFFF", "stars")
    dabs = []
    for _ in range(40):
        x = random.uniform(160, 2440)
        y = random.uniform(120, 560) if random.random() < 0.8 else random.uniform(560, 760)
        if (x - SUN[0]) ** 2 + (y - SUN[1]) ** 2 < 330 ** 2:
            continue
        p = random.uniform(0.35, 1.0)
        dabs.append(([(x, y, p), (x + 1, y + 1, p)], 0.06))
    draw(dabs, "stars")


def bloom():
    with seg("menu", "bloom"):
        d.tap(*d.ADJUST, wait=1.8)
        at = d.bounds_of("Bloom")
        d.tap(*at, wait=1.6)
        d.swipe(700, 800, 1900, 800, 2200, wait=2.0)
        d.tap(*d.ADJUST, wait=1.2)


def tour():
    with seg("menu", "layers"):
        d.tap(*d.LAYERS, wait=3.5)
        d.tap(*d.LAYERS, wait=1.5)


def gallery():
    with seg("menu", "gallery"):
        d.tap(136, 42, wait=3.5)


STAGES = {"new": new_canvas, "sky": sky, "sun": sun, "hills": hills, "trees": trees, "stars": stars,
          "bloom": bloom, "tour": tour, "gallery": gallery}

if __name__ == "__main__":
    out = sys.argv[1]
    try:
        for name in (sys.argv[2:] or list(STAGES)):
            t = time.time()
            STAGES[name]()
            print(f"{name}: {time.time() - t:.1f} s", flush=True)
    finally:
        with open(out, "w", encoding="utf-8") as f:
            json.dump({"t0": T0, "segments": LOG}, f, indent=1)
