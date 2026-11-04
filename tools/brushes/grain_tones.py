"""Writes the engine's tables of stock grain and shape tones (crates/engine/src/stock_tones.rs): for
each grain image another app's brush files name instead of carrying it, the values at nine shares
of it (none, 5%, 10%, a quarter, half, three quarters, 90%, 95%, all); for each shape image, at
every sixteenth, and how much ink each of 64 bands of its rows holds, top to bottom (as shares of
its fullest band). Measured as the engine reads an image (its luminance times its alpha, averaged
down to 256 square). Each is keyed by a hash of its file's name, so the tables hold measurements
only: neither the images nor their names.

usage: python -I tools/brushes/grain_tones.py <that app's bundle folder>
"""
import os
import plistlib
import sys
import zipfile

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "crates/engine/src/stock_tones.rs")
SHARES = (0.0, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 1.0)
SHAPE_SHARES = tuple(i / 16 for i in range(17))
BANDS = 64
SIDE = 256


def fnv1a(text):
    """FNV-1a, 64 bits, of the name in lower case (as the engine hashes it)."""
    h = 0xCBF29CE484222325
    for b in text.lower().encode("utf-8"):
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def named_images(bundle, key):
    """The images the bundle's brush files name under `key`, each once."""
    names = set()
    for folder, _, files in os.walk(bundle):
        for f in files:
            if not f.endswith(".brush"):
                continue
            try:
                with zipfile.ZipFile(os.path.join(folder, f)) as z:
                    archive = next(n for n in z.namelist() if n.endswith("Brush.archive"))
                    p = plistlib.loads(z.read(archive))
            except (StopIteration, zipfile.BadZipFile, plistlib.InvalidFileException, OSError):
                continue
            objects = p.get("$objects", [])
            top = next((o for o in objects if isinstance(o, dict) and "plotSpacing" in o), None)
            if top is None:
                continue
            v = top.get(key)
            if isinstance(v, plistlib.UID):
                v = objects[v.data]
            if isinstance(v, str) and v and v != "$null":
                names.add(v)
    return names


def images_by_name(bundle):
    """Every image in the bundle, by its file's name in lower case."""
    found = {}
    for folder, _, files in os.walk(bundle):
        for f in files:
            if f.lower().endswith((".png", ".jpg", ".jpeg")):
                found.setdefault(f.lower(), os.path.join(folder, f))
    return found


def values(path):
    """The image's values as the engine reads it: luminance times alpha, 256 square."""
    im = Image.open(path)
    if im.mode in ("LA", "RGBA", "PA") or "transparency" in im.info:
        rgba = im.convert("RGBA")
        lum = rgba.convert("L")
        alpha = rgba.getchannel("A")
        im = Image.frombytes("L", rgba.size, bytes(l * a // 255 for l, a in zip(lum.tobytes(), alpha.tobytes())))
    else:
        im = im.convert("L")
    # each of the engine's pixels the average of the image's it covers
    return im.resize((SIDE, SIDE), Image.Resampling.BOX)


def tones(path, shares):
    """The image's values at `shares`."""
    px = sorted(values(path).tobytes())
    return [px[round(s * (len(px) - 1))] for s in shares]


def rows(path):
    """How much ink each band of the image's rows holds, top to bottom, as shares of the fullest
    band (0..255)."""
    px = values(path).tobytes()
    per = SIDE // BANDS
    sums = [sum(px[y * SIDE:(y + per) * SIDE]) for y in range(0, SIDE, per)]
    top = max(sums) or 1
    return [round(255 * v / top) for v in sums]


def table(bundle, images, key, measure):
    """One table: each image the brush files name under `key`, its hash and `measure` of it."""
    out = []
    for name in sorted(named_images(bundle, key)):
        base = name.replace("\\", "/").split("/")[-1].lower()
        path = images.get(base)
        if path is None:
            continue
        out.append((fnv1a(base), measure(path)))
    out.sort()
    return out


def body(rows):
    return "\n".join(f"    (0x{h:016x}, [{', '.join(str(v) for v in t)}])," for h, t in rows)


def main():
    bundle = sys.argv[1]
    images = images_by_name(bundle)
    grains = table(bundle, images, "bundledGrainPath", lambda p: tones(p, SHARES))
    shapes = table(bundle, images, "bundledShapePath", lambda p: tones(p, SHAPE_SHARES))
    bands = table(bundle, images, "bundledShapePath", rows)
    header = """//! Written by tools/brushes/grain_tones.py (don't edit by hand): the tones of the stock grains
//! and shapes other apps' brush files name instead of carrying (see [`stock_grain_tones`] and
//! [`stock_shape_tones`]), each the values at [`TONE_SHARES`] or [`SHAPE_TONE_SHARES`] of it, and
//! how much each band of a shape's rows holds (see [`stock_shape_rows`]), keyed by a hash of its
//! file's name.

#![cfg_attr(rustfmt, rustfmt_skip)]

"""
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + f"pub(crate) const STOCK_GRAIN_TONES: &[(u64, [u8; 9])] = &[\n{body(grains)}\n];\n")
        f.write(f"\npub(crate) const STOCK_SHAPE_TONES: &[(u64, [u8; 17])] = &[\n{body(shapes)}\n];\n")
        f.write(f"\npub(crate) const STOCK_SHAPE_ROWS: &[(u64, [u8; {BANDS}])] = &[\n{body(bands)}\n];\n")
    print(f"{len(grains)} grains, {len(shapes)} shapes measured")


if __name__ == "__main__":
    sys.exit(main())
