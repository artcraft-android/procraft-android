"""Draws ProCraft's app icon, a paintbrush in a warm-to-cool blend on black, and writes every size
the app and the repository use. Needs Pillow. Run from anywhere:

    python tools/app-icon/make_icon.py

Writes:
- assets/app-icon/procraft-1024.png, procraft-512.png: the icon on its rounded tile.
- the Android adaptive icon's layers (108 dp, the tile its middle 72 dp): the brush and its glow
  (drawable-*dpi/ic_launcher_foreground.png), the brush alone in white for themed icons
  (drawable-*dpi/ic_launcher_monochrome.png), and a large copy for the About screen
  (drawable-nodpi/procraft_mark.png).
"""

import math
import os

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RES = os.path.join(ROOT, 'apps', 'procraft-android', 'android', 'app', 'src', 'main', 'res')

S = 2048  # the 512-unit tile, drawn at 4x
INK = (11, 11, 12)
PAPER = (239, 233, 220)
METAL = (196, 198, 206)
METAL_LINE = (120, 122, 130)
# the blend: amber, orange, coral pink, violet, blue
STOPS = [(0.0, (255, 196, 61)), (0.28, (240, 122, 46)), (0.52, (236, 72, 120)), (0.76, (139, 92, 246)),
         (1.0, (59, 130, 246))]
ANGLE = 225  # the brush drawn along x, then turned so it writes down to the left


def blend(t):
    t = min(1.0, max(0.0, t))
    for (a, ca), (b, cb) in zip(STOPS, STOPS[1:]):
        if t <= b:
            u = (t - a) / (b - a)
            return tuple(round(ca[i] + (cb[i] - ca[i]) * u) for i in range(3))
    return STOPS[-1][1]


def gradient_along(x0, x1):
    """An S x S picture whose colour runs along x, from the blend's start at x0 to its end at x1."""
    row = Image.new('RGB', (S, 1))
    for x in range(S):
        row.putpixel((x, 0), blend((x - x0) / (x1 - x0)))
    return row.resize((S, S), Image.NEAREST)


def brush():
    """The paintbrush along x: a tapered handle in the blend, a metal ferrule, paper bristles whose
    tip is dipped in the blend."""
    layer = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    cy = S / 2
    x_end, x_ferrule, x_bristle, x_tip = 230, 1080, 1300, 1820
    handle = [(x_end + 60, cy - 70), (x_ferrule - 260, cy - 120), (x_ferrule, cy - 100),
              (x_ferrule, cy + 100), (x_ferrule - 260, cy + 120), (x_end + 60, cy + 70)]
    m = Image.new('L', (S, S), 0)
    md = ImageDraw.Draw(m)
    md.polygon(handle, fill=255)
    md.ellipse((x_end, cy - 70, x_end + 120, cy + 70), fill=255)
    layer.paste(gradient_along(x_end, x_ferrule), (0, 0), m)
    shine = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(shine).polygon([(x_end + 60, cy - 70), (x_ferrule - 260, cy - 120), (x_ferrule, cy - 100),
                                   (x_ferrule, cy - 60), (x_end + 60, cy - 40)], fill=(255, 255, 255, 52))
    layer.alpha_composite(shine)
    d = ImageDraw.Draw(layer)
    d.polygon([(x_ferrule, cy - 104), (x_bristle, cy - 128), (x_bristle, cy + 128), (x_ferrule, cy + 104)],
              fill=METAL + (255,))
    d.line((x_ferrule + 70, cy - 112, x_ferrule + 70, cy + 112), fill=METAL_LINE + (255,), width=14)
    bristles = [(x_bristle, cy - 128)]
    for i in range(1, 41):
        t = i / 40
        x = x_bristle + t * (x_tip - x_bristle)
        y = cy - 128 * (1 - t ** 1.6) * (1 + 0.25 * math.sin(t * math.pi))
        bristles.append((x, y))
    bristles += [(x, 2 * cy - y) for x, y in reversed(bristles)]
    d.polygon(bristles, fill=PAPER + (255,))
    dip = Image.new('L', (S, S), 0)
    ImageDraw.Draw(dip).polygon(bristles, fill=255)
    cut = Image.new('L', (S, S), 0)
    ImageDraw.Draw(cut).rectangle((x_bristle + 0.55 * (x_tip - x_bristle), 0, S, S), fill=255)
    layer.paste(gradient_along(x_bristle + 0.4 * (x_tip - x_bristle), x_tip + 200), (0, 0), ImageChops.multiply(dip, cut))
    return layer.rotate(ANGLE, resample=Image.BICUBIC, center=(S / 2, S / 2))


def glow(turned):
    """A soft glow of the blend around the turned brush."""
    alpha = turned.split()[3].filter(ImageFilter.GaussianBlur(60)).point(lambda a: a * 0.35)
    colour = gradient_along(S * 0.15, S * 0.85).rotate(ANGLE, center=(S / 2, S / 2))
    out = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    out.paste(colour, (0, 0), alpha)
    return out


def tile_mask():
    m = Image.new('L', (S, S), 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, S - 1, S - 1), radius=112 * 4, fill=255)
    return m


def adaptive(picture):
    """`picture` (the tile's contents) placed in the middle 72 of the adaptive icon's 108."""
    side = round(S * 108 / 72)
    out = Image.new('RGBA', (side, side), (0, 0, 0, 0))
    out.alpha_composite(picture, ((side - S) // 2, (side - S) // 2))
    return out


def save(image, path, size):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    image.resize((size, size), Image.LANCZOS).save(path, optimize=True)


def main():
    turned = brush()
    picture = glow(turned)
    picture.alpha_composite(turned)

    tile = Image.new('RGBA', (S, S), INK + (255,))
    tile.alpha_composite(picture)
    tile.putalpha(tile_mask())
    for size in (1024, 512):
        save(tile, os.path.join(ROOT, 'assets', 'app-icon', f'procraft-{size}.png'), size)

    foreground = adaptive(picture)
    mono = Image.new('RGBA', turned.size, (255, 255, 255, 0))
    mono.putalpha(turned.split()[3])
    monochrome = adaptive(mono)
    for bucket, scale in (('mdpi', 1), ('hdpi', 1.5), ('xhdpi', 2), ('xxhdpi', 3), ('xxxhdpi', 4)):
        size = round(108 * scale)
        save(foreground, os.path.join(RES, f'drawable-{bucket}', 'ic_launcher_foreground.png'), size)
        save(monochrome, os.path.join(RES, f'drawable-{bucket}', 'ic_launcher_monochrome.png'), size)
    # the About screen shows it 450 dp across: sharp up to about 3 pixels a dp
    save(foreground, os.path.join(RES, 'drawable-nodpi', 'procraft_mark.png'), 1350)
    print('written')


if __name__ == '__main__':
    main()
