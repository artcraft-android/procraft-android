"""Makes scripted pen strokes and taps read as a person's: strokes that start and end slowly, a
slight wobble, pressure that rises and falls, and pauses between moves.
"""
import math
import random

rng = random.Random(11)


def smooth_noise(n, amp, scale=18):
    """n values of a smooth wobble, about +-amp."""
    knots = [rng.uniform(-1, 1) for _ in range(n // scale + 3)]
    out = []
    for i in range(n):
        t = i / scale
        k = int(t)
        f = t - k
        f = f * f * (3 - 2 * f)
        out.append(amp * (knots[k] * (1 - f) + knots[k + 1] * f))
    return out


def resample(path, step):
    """Points along a polyline `path` [(x, y, p)], about `step` px apart."""
    out = [path[0]]
    carry = 0.0
    for (x0, y0, p0), (x1, y1, p1) in zip(path, path[1:]):
        seg = math.hypot(x1 - x0, y1 - y0)
        if seg == 0:
            continue
        d = step - carry
        while d <= seg:
            t = d / seg
            out.append((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, p0 + (p1 - p0) * t))
            d += step
        carry = seg - (d - step)
    if out[-1][:2] != path[-1][:2]:
        out.append(path[-1])
    return out


def stroke(path, seconds, wobble=1.6, taper_in=0.12, taper_out=0.18, step=10):
    """A person's version of `path`: (points, per-point delays in ms). The speed eases in and out
    (slow at the ends), the line wobbles a little across, and the pressure rises at the start
    and falls at the end."""
    pts = resample(path, step)
    n = len(pts)
    if n < 2:
        return pts, [16] * n
    wob = smooth_noise(n, wobble)
    out = []
    for i, (x, y, p) in enumerate(pts):
        a, b = pts[max(0, i - 1)], pts[min(n - 1, i + 1)]
        dx, dy = b[0] - a[0], b[1] - a[1]
        ln = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / ln, dx / ln
        t = i / (n - 1)
        rise = min(1.0, t / taper_in) if taper_in else 1.0
        fall = min(1.0, (1 - t) / taper_out) if taper_out else 1.0
        shape = (0.35 + 0.65 * math.sin(math.pi / 2 * rise)) * (0.3 + 0.7 * math.sin(math.pi / 2 * fall))
        out.append((x + nx * wob[i], y + ny * wob[i], max(0.04, min(1.0, p * shape))))
    # time per point: an eased speed, slower at both ends
    weights = [1.0 / (0.35 + math.sin(math.pi * (i + 0.5) / n)) for i in range(n)]
    total = sum(weights)
    dts = [max(4, int(seconds * 1000 * w / total)) for w in weights]
    return out, dts


def pause(lo=0.25, hi=0.7):
    return rng.uniform(lo, hi)
