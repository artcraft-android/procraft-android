"""The tablet's screen captures hold Display P3 values; this turns them into sRGB JPEGs for the web.

usage: python -I to_srgb.py <out dir> <width> <png ...>
"""
import os
import sys

import numpy as np
from PIL import Image

P3_TO_SRGB = np.array([[1.2249, -0.2247, 0.0], [-0.0420, 1.0419, 0.0], [-0.0197, -0.0786, 1.0979]])


def decode(v):
    return np.where(v <= 0.04045, v / 12.92, ((v + 0.055) / 1.055) ** 2.4)


def encode(v):
    v = np.clip(v, 0, 1)
    return np.where(v <= 0.0031308, v * 12.92, 1.055 * v ** (1 / 2.4) - 0.055)


out_dir, width = sys.argv[1], int(sys.argv[2])
os.makedirs(out_dir, exist_ok=True)
for path in sys.argv[3:]:
    im = Image.open(path).convert("RGB")
    a = np.asarray(im, dtype=np.float64) / 255
    lin = decode(a) @ P3_TO_SRGB.T
    rgb = Image.fromarray((encode(lin) * 255 + 0.5).astype(np.uint8))
    if rgb.width != width:
        rgb = rgb.resize((width, round(rgb.height * width / rgb.width)), Image.LANCZOS)
    name = os.path.splitext(os.path.basename(path))[0] + ".jpg"
    rgb.save(os.path.join(out_dir, name), quality=88, optimize=True, progressive=True)
    print(name, os.path.getsize(os.path.join(out_dir, name)) // 1024, "KB")
