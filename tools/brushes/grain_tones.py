"""Writes the engine's tables of stock grain and shape tones (crates/engine/src/stock_tones.rs): for
each grain image another app's brush files name instead of carrying it, the values at nine shares
of it (none, 5%, 10%, a quarter, half, three quarters, 90%, 95%, all); for each shape image, at
every sixteenth, and each of 64 bands of its rows' mean value, top to bottom, and of its columns,
left to right (means, so that a brush that inverts the image can have them inverted too). Measured as the engine reads an image (its luminance times its alpha, averaged
down to 256 square). And each grain's feature size: how far it shifts before it looks only half
like itself (its autocorrelation at half), as a share of its side, at four levels of detail (the
image at 1024, 512, 256 and 128 a side, no finer than its own), so that our stand-in can be tiled
to match whatever size its tile is drawn. Each is keyed by a hash of its file's name, so the tables hold
measurements only: neither the images nor their names.

usage: python -I tools/brushes/grain_tones.py <that app's bundle folder>   (needs Pillow and numpy)
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


def values(path, side=SIDE):
    """The image's values as the engine reads it: luminance times alpha, `side` square (None: at
    its own size)."""
    im = Image.open(path)
    if im.mode in ("LA", "RGBA", "PA") or "transparency" in im.info:
        rgba = im.convert("RGBA")
        lum = rgba.convert("L")
        alpha = rgba.getchannel("A")
        im = Image.frombytes("L", rgba.size, bytes(l * a // 255 for l, a in zip(lum.tobytes(), alpha.tobytes())))
    else:
        im = im.convert("L")
    if side is None:
        return im
    # each of the engine's pixels the average of the image's it covers
    return im.resize((side, side), Image.Resampling.BOX)


def tones(path, shares):
    """The image's values at `shares`."""
    px = sorted(values(path).tobytes())
    return [px[round(s * (len(px) - 1))] for s in shares]


def rows(path):
    """Each band of the image's rows' mean value, top to bottom (0..255): the ink it holds, or the
    paper it leaves for a brush that inverts it."""
    return band_means(values(path))


def cols(path):
    """The same of its columns, left to right."""
    return band_means(values(path).transpose(Image.Transpose.TRANSPOSE))


def band_means(im):
    px = im.tobytes()
    per = SIDE // BANDS
    return [round(sum(px[y * SIDE:(y + per) * SIDE]) / (per * SIDE)) for y in range(0, SIDE, per)]


FEATURE_SIDES = (1024, 512, 256, 128)


def feature(path):
    """How far the image shifts before it matches itself only half, at each of FEATURE_SIDES (the
    image averaged down to that side; at its own size where it is smaller), in 65536ths."""
    im = values(path, None)
    own = min(im.size)
    return [feature_at(im if side >= own else im.resize((side, side), Image.Resampling.BOX)) for side in FEATURE_SIDES]


def feature_at(im):
    """How far `im` shifts before it matches itself only half (its autocorrelation, wrapping
    round, falls to 0.5), across and down averaged, as a share of its side, in 65536ths (at least 1).
    """
    import numpy as np

    g = np.asarray(im, dtype=np.float64)
    g = g - g.mean()
    if g.std() < 1e-6:
        return 65535
    f = np.fft.fft2(g)
    ac = np.real(np.fft.ifft2(f * np.conj(f)))
    ac /= ac[0, 0]

    def half(line):
        for k in range(1, len(line) // 2):
            if line[k] <= 0.5:
                a, b = line[k - 1], line[k]
                return ((k - 1) + (a - 0.5) / max(a - b, 1e-9)) / len(line)
        return 0.5

    share = (half(ac[0, :]) + half(ac[:, 0])) / 2
    return max(1, min(65535, round(share * 65536)))


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
    columns = table(bundle, images, "bundledShapePath", cols)
    features = table(bundle, images, "bundledGrainPath", feature)
    header = """//! Written by tools/brushes/grain_tones.py (don't edit by hand): the tones of the stock grains
//! and shapes other apps' brush files name instead of carrying (see [`stock_grain_tones`] and
//! [`stock_shape_tones`]), each the values at [`TONE_SHARES`] or [`SHAPE_TONE_SHARES`] of it, and
//! each band of a shape's rows' and columns' mean value (see [`stock_shape_rows`] and
//! [`stock_shape_cols`]), and each grain's
//! feature size (see [`stock_grain_feature`]), keyed by a hash of its file's name.

#![cfg_attr(rustfmt, rustfmt_skip)]

"""
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + f"pub(crate) const STOCK_GRAIN_TONES: &[(u64, [u8; 9])] = &[\n{body(grains)}\n];\n")
        f.write(f"\npub(crate) const STOCK_SHAPE_TONES: &[(u64, [u8; 17])] = &[\n{body(shapes)}\n];\n")
        f.write(f"\npub(crate) const STOCK_SHAPE_ROWS: &[(u64, [u8; {BANDS}])] = &[\n{body(bands)}\n];\n")
        f.write(f"\npub(crate) const STOCK_SHAPE_COLS: &[(u64, [u8; {BANDS}])] = &[\n{body(columns)}\n];\n")
        f.write("\npub(crate) const STOCK_GRAIN_FEATURES: &[(u64, [u16; 4])] = &[\n"
                + "\n".join(f"    (0x{h:016x}, [{', '.join(str(v) for v in t)}])," for h, t in features) + "\n];\n")
    print(f"{len(grains)} grains, {len(shapes)} shapes, {len(features)} grains' features measured")


if __name__ == "__main__":
    sys.exit(main())
