"""Writes scripts for Android's `hid` shell command that register a virtual pen digitizer (tip,
barrel, eraser, in range, X, Y and 12-bit pressure) and play pen strokes on it, with real timing.

Coordinates are fractions of the digitizer's area (0..1 each way); pressure is 0..1.
"""
import json

DESCRIPTOR = [
    0x05, 0x0D,        # Usage Page (Digitizer)
    0x09, 0x02,        # Usage (Pen)
    0xA1, 0x01,        # Collection (Application)
    0x85, 0x01,        #   Report ID (1)
    0x09, 0x20,        #   Usage (Stylus)
    0xA1, 0x00,        #   Collection (Physical)
    0x09, 0x42,        #     Usage (Tip Switch)
    0x09, 0x44,        #     Usage (Barrel Switch)
    0x09, 0x45,        #     Usage (Eraser)
    0x09, 0x32,        #     Usage (In Range)
    0x15, 0x00,        #     Logical Minimum (0)
    0x25, 0x01,        #     Logical Maximum (1)
    0x75, 0x01,        #     Report Size (1)
    0x95, 0x04,        #     Report Count (4)
    0x81, 0x02,        #     Input (Data, Var, Abs)
    0x95, 0x04,        #     Report Count (4)
    0x81, 0x03,        #     Input (Const)
    0x05, 0x01,        #     Usage Page (Generic Desktop)
    0x09, 0x30,        #     Usage (X)
    0x15, 0x00,        #     Logical Minimum (0)
    0x26, 0xFF, 0x7F,  #     Logical Maximum (32767)
    0x35, 0x00,        #     Physical Minimum (0)
    0x46, 0x10, 0x27,  #     Physical Maximum (10000)
    0x55, 0x0E,        #     Unit Exponent (-2)
    0x65, 0x11,        #     Unit (cm)
    0x75, 0x10,        #     Report Size (16)
    0x95, 0x01,        #     Report Count (1)
    0x81, 0x02,        #     Input (Data, Var, Abs)
    0x09, 0x31,        #     Usage (Y)
    0x46, 0x10, 0x27,  #     Physical Maximum (10000)
    0x81, 0x02,        #     Input (Data, Var, Abs)
    0x05, 0x0D,        #     Usage Page (Digitizer)
    0x09, 0x30,        #     Usage (Tip Pressure)
    0x15, 0x00,        #     Logical Minimum (0)
    0x26, 0xFF, 0x0F,  #     Logical Maximum (4095)
    0x75, 0x10,        #     Report Size (16)
    0x95, 0x01,        #     Report Count (1)
    0x81, 0x02,        #     Input (Data, Var, Abs)
    0xC0,              #   End Collection
    0xC0,              # End Collection
]


class Script:
    def __init__(self, map_xy=lambda x, y: (x, y)):
        self.events = [{"id": 1, "command": "register", "name": "Showcase Pen", "vid": 0x1209, "pid": 0x0D0D,
                        "bus": "usb", "descriptor": DESCRIPTOR}]
        self.map_xy = map_xy
        self.delay(800)

    def delay(self, ms):
        self.events.append({"id": 1, "command": "delay", "duration": int(ms)})

    def report(self, tip, in_range, x, y, pressure):
        x, y = self.map_xy(x, y)
        xi = max(0, min(32767, int(round(x * 32767))))
        yi = max(0, min(32767, int(round(y * 32767))))
        pi = max(0, min(4095, int(round(pressure * 4095))))
        buttons = (1 if tip else 0) | (8 if in_range else 0)
        self.events.append({"id": 1, "command": "report",
                            "report": [1, buttons, xi & 255, xi >> 8, yi & 255, yi >> 8, pi & 255, pi >> 8]})

    def stroke(self, points, step_ms=8, dts=None, hover_ms=None):
        """points: (x, y, pressure) in 0..1; the pen comes into range, touches, moves and lifts.
        `dts` gives each point's own delay in ms (else `step_ms` each); `hover_ms` how long the
        pen hovers before touching."""
        dts = dts or [step_ms] * len(points)
        x0, y0, _ = points[0]
        self.report(False, True, x0, y0, 0)
        self.delay(hover_ms if hover_ms is not None else step_ms * 2)
        for (x, y, p), dt in zip(points, dts):
            self.report(True, True, x, y, max(p, 0.02))
            self.delay(dt)
        x1, y1, _ = points[-1]
        self.report(False, True, x1, y1, 0)
        self.delay(step_ms)
        self.report(False, False, x1, y1, 0)
        self.delay(step_ms * 3)

    def tap(self, x, y, hold_ms=60):
        self.stroke([(x, y, 0.6)] * max(1, hold_ms // 8))

    def write(self, path):
        self.delay(500)
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            for e in self.events:
                f.write(json.dumps(e, separators=(",", ":")) + "\n")
