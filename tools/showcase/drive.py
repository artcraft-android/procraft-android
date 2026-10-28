"""Drives ProCraft on a tablet (landscape, 2560 x 1600) for the showcase: adb taps for the
interface, and pen strokes with pressure through a virtual HID pen (penhid.py, Android's `hid`).
The positions are measured on a Lenovo Legion Tab; set ANDROID_SERIAL to pick the device.
"""
import math
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from penhid import Script  # noqa: E402

ADB = os.environ.get("ADB", "adb")
W, H = 2560, 1600

# the digitizer's axes on the landscape screen: its Y runs left to right, its X bottom to top
def screen_to_pen(x, y):
    return 1.0 - y / H, x / W


def adb(*args, check=True):
    return subprocess.run([ADB, *args], check=check, capture_output=True, text=True).stdout


def sh(cmd):
    return adb("shell", cmd)


def tap(x, y, wait=0.35):
    sh(f"input tap {int(x)} {int(y)}")
    time.sleep(wait)


def taps(points, wait=0.35):
    sh("; ".join(f"input tap {int(x)} {int(y)}" for x, y in points))
    time.sleep(wait)


def swipe(x1, y1, x2, y2, ms=400, wait=0.4):
    sh(f"input swipe {int(x1)} {int(y1)} {int(x2)} {int(y2)} {int(ms)}")
    time.sleep(wait)


def drag(x1, y1, x2, y2, ms=900, wait=0.6):
    """A finger drag that holds still at the start first (for drag-and-drop)."""
    sh(f"input draganddrop {int(x1)} {int(y1)} {int(x2)} {int(y2)} {int(ms)}")
    time.sleep(wait)


def colour_drop(x, y, wait=1.2):
    """ColorDrop: the colour dragged out of the colour button and let go at (x, y). A plain swipe:
    holding first would take the previous colour, and starting nearer the top edge pulls down the
    system bars."""
    swipe(2432, 74, x, y, 900, wait=wait)


class Pen:
    """Collects strokes in screen pixels, then plays them in one `hid` run."""

    def __init__(self):
        self.s = Script(map_xy=screen_to_pen)

    def stroke(self, pts, step_ms=8):
        self.s.stroke(pts, step_ms)
        return self

    def pause(self, ms):
        self.s.delay(ms)
        return self

    def play(self, name="pen"):
        local = os.path.join(HERE, f"{name}.json")
        self.s.write(local)
        adb("push", local, f"/data/local/tmp/{name}.json")
        sh(f"hid /data/local/tmp/{name}.json")
        time.sleep(0.3)


# the interface, in screen pixels
PAINT, SMUDGE, ERASE, LAYERS, COLOUR = (1876, 44), (2016, 44), (2156, 44), (2296, 44), (2432, 44)
ACTIONS, ADJUST, SELECT, TRANSFORM = (268, 44), (384, 44), (500, 44), (616, 44)
SETS = {"Sketching": 290, "Inking": 414, "Drawing": 538, "Painting": 662, "Watercolour": 786,
        "Calligraphy": 910, "Airbrushing": 1034, "Spray Paint": 1158, "Charcoals": 1282, "Textures": 1406,
        "Abstract": 1530}
HEX_FIELD = (2420, 912)
HEX_KEYS = {"D": (1355, 642), "E": (1517, 642), "F": (1679, 642), "A": (1355, 750), "B": (1517, 750),
            "C": (1679, 750), "7": (1355, 858), "8": (1517, 858), "9": (1679, 858), "4": (1355, 966),
            "5": (1517, 966), "6": (1679, 966), "1": (1355, 1074), "2": (1517, 1074), "3": (1679, 1074),
            "0": (1436, 1182), "del": (1679, 1182)}
PANEL_BLANK = (2160, 1000)
SIZE_TRACK = (50, 340, 660)       # x, top (largest), bottom (smallest)
OPACITY_TRACK = (50, 830, 1140)


def ui():
    """The screen's interface tree, as uiautomator dumps it."""
    for _ in range(3):
        out = sh("uiautomator dump /sdcard/u.xml >/dev/null 2>&1; cat /sdcard/u.xml")
        if "<hierarchy" in out:
            return out
        time.sleep(0.4)
    return ""


def bounds_of(text, tree=None):
    """The centre of the first node whose text is `text`, or None."""
    import re
    m = re.search(r'text="' + re.escape(text) + r'"[^>]*?bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', tree or ui())
    if not m:
        return None
    x0, y0, x1, y1 = map(int, m.groups())
    return (x0 + x1) // 2, (y0 + y1) // 2


def panel_open():
    return 'text="Colors"' in ui()


def colour(hexcode, wait=0.25):
    """Sets the colour by hex in the colour panel's Value tab, checking each step, and closes it."""
    want = "#" + hexcode.upper().lstrip("#")
    for _ in range(3):
        if not panel_open():
            tap(*COLOUR, wait=0.8)
        tap(*HEX_FIELD, wait=0.6)
        keys = [HEX_KEYS["del"]] * 6 + [HEX_KEYS[c] for c in want[1:]]
        taps(keys, wait=0.3)
        tap(*PANEL_BLANK, wait=0.5)
        if f'text="{want}"' in ui():
            break
    if panel_open():
        tap(*COLOUR, wait=wait)


def library_open():
    return 'text="Brush library"' in ui()


def brush(set_name, index, wait=0.3):
    """Picks a brush from the library: its set, then its row (0 first)."""
    if not library_open():
        tap(*PAINT, wait=0.8)
        if not library_open():
            tap(*PAINT, wait=0.8)
    tap(1620, SETS[set_name], wait=0.9)
    tap(2000, 300 + 176 * index, wait=0.8)
    tree = ui()
    if 'text="Brush Studio"' in tree:
        # the brush was already chosen, and a second tap opens its studio: leave it unchanged
        at = bounds_of("Cancel", tree)
        tap(*(at or (2333, 84)), wait=0.8)
        tree = ui()
    if 'text="Brush library"' in tree:
        tap(*PAINT, wait=wait)


def size(frac, wait=0.3):
    """The brush size slider: all the way down, then up to `frac` of its track."""
    x, top, bottom = SIZE_TRACK
    swipe(x, top + 10, x, bottom + 300, 300, wait=0.2)
    swipe(x, bottom, x, bottom - frac * (bottom - top), 500, wait=wait)


def opacity(frac, wait=0.3):
    """The brush opacity slider: all the way down, then up to `frac` of its track."""
    x, top, bottom = OPACITY_TRACK
    swipe(x, top + 10, x, bottom + 300, 300, wait=0.2)
    swipe(x, bottom, x, bottom - frac * (bottom - top) - (40 if frac >= 1 else 0), 500, wait=wait)


def new_layer(wait=0.5):
    tap(*LAYERS, wait=0.6)
    tap(2460, 176, wait=0.5)
    tap(*LAYERS, wait=wait)


def wave(x0, x1, y, amp, waves, pressure, n=90, phase=0.0, p_amp=0.12):
    pts = []
    for i in range(n + 1):
        t = i / n
        x = x0 + (x1 - x0) * t
        yy = y + amp * math.sin(2 * math.pi * waves * t + phase)
        p = pressure + p_amp * math.sin(math.pi * t)
        pts.append((x, yy, min(1.0, p)))
    return pts
