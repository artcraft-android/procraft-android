"""Writes the engine's tables of stock grain and shape tones (crates/engine/src/stock_tones.rs): for
each grain image another app's brush files name instead of carrying it (every brush image its bundle
holds: those its own brush files name, their seconds' too, and the rest by what their names say they
are, as older brushes and other people's still name them), the values at nine shares
of it (none, 5%, 10%, a quarter, half, three quarters, 90%, 95%, all); for each shape image, at
every sixteenth, and each of 64 bands of its rows' mean value, top to bottom, and of its columns,
left to right (means, so that a brush that inverts the image can have them inverted too). Measured as the engine reads an image (its luminance times its alpha, averaged
down to 256 square). And each grain's feature size: how far it shifts before it looks only half
like itself (its autocorrelation at half), as a share of its side, at four levels of detail (the
image at 1024, 512, 256 and 128 a side, no finer than its own), so that our stand-in can be tiled
to match whatever size its tile is drawn; and each shape's texture's feature size, its outline
taken away, so that our stand-in has marks as coarse. Each is keyed by a hash of its file's name, so the tables hold
measurements only: neither the images nor their names. And each grain's width and the strength of
each octave of our stand-in noise that makes it vary as much as the image does at each size, so
that a stand-in for a paper or a wash has its fibres, veins and blotches each as strong.

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
    """The images the bundle's brush files name under `key`, each once: a dual brush's second's as
    well as its first's."""
    names = set()
    for folder, _, files in os.walk(bundle):
        for f in files:
            if not f.endswith(".brush"):
                continue
            try:
                with zipfile.ZipFile(os.path.join(folder, f)) as z:
                    archives = [plistlib.loads(z.read(n)) for n in z.namelist() if n.endswith("Brush.archive")]
            except (zipfile.BadZipFile, plistlib.InvalidFileException, OSError):
                continue
            for p in archives:
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


def unnamed_images(bundle, images):
    """The brush images in the bundle no brush file of it names (older brushes and other people's
    still do), by file name: those named as a shape, those named as a grain, and those whose names
    say neither (measured both ways)."""
    named = {base_name(n) for key in ("bundledShapePath", "bundledGrainPath") for n in named_images(bundle, key)}
    shapes, grains = set(), set()
    for name, path in images.items():
        if name in named or "/brushes/images/" not in path.replace("\\", "/").lower():
            continue
        stem = name.rsplit(".", 1)[0]
        if not stem.endswith("_grain"):
            shapes.add(name)
        if not stem.endswith("_shape"):
            grains.add(name)
    return shapes, grains


def base_name(name):
    """An image's file name in lower case, from the path a brush file names it by."""
    return name.replace("\\", "/").split("/")[-1].lower()


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


def shape_feature(path):
    """How coarse a shape's texture is: the feature size (as [`feature_at`] measures it, at 256 a
    side) of the shape less its broad outline (the shape blurred over a sixteenth of its side), in
    65536ths; 65535 for one with no texture of its own (a solid or smooth shape)."""
    import numpy as np
    from PIL import ImageFilter

    im = values(path)
    broad = im.filter(ImageFilter.GaussianBlur(SIDE / 16))
    g = np.asarray(im, dtype=np.float64) - np.asarray(broad, dtype=np.float64)
    # where the shape has hardly any texture of its own, it says nothing of one
    if g.std() < 4.0:
        return [65535]
    return [feature_at(Image.fromarray(np.clip(g + 128.0, 0, 255).astype(np.uint8)))]


OCTAVES = 8
OCTAVE_GRID = 1024
OCTAVE_SIDE = 256


def blur_wrapped(a, sigma):
    """`a` softened by a Gaussian of `sigma` pixels, wrapping round (a grain tiles)."""
    import numpy as np

    if sigma <= 0:
        return a
    f = np.fft.fftfreq(a.shape[0])
    g = np.exp(-2 * (np.pi * sigma) ** 2 * (f[:, None] ** 2 + f[None, :] ** 2))
    return np.real(np.fft.ifft2(np.fft.fft2(a) * g))


def octave_bands(a):
    """How much `a` varies at each of OCTAVES sizes, finest first: the spread of what is left
    between softenings of 0, 0.5, 1, 2 ... pixels."""
    import numpy as np

    sigmas = [0.0] + [2 ** k / 2 for k in range(OCTAVES)]
    return np.array([(blur_wrapped(a, sigmas[k]) - blur_wrapped(a, sigmas[k + 1])).std() for k in range(OCTAVES)])


def value_noise(n, cell, rng):
    """Value noise as the engine draws an octave of a stand-in grain: a random value at each
    corner of cells `cell` pixels wide, eased between them (smoothstep), wrapping round."""
    import numpy as np

    p = n // cell
    lattice = rng.random((p, p))
    x = (np.arange(n) + 0.5) / cell - 0.5
    i0 = np.floor(x).astype(int)
    t = x - i0
    f = t * t * (3 - 2 * t)
    i0 %= p
    i1 = (i0 + 1) % p
    top = lattice[i0][:, i0] * (1 - f)[None, :] + lattice[i0][:, i1] * f[None, :]
    bottom = lattice[i1][:, i0] * (1 - f)[None, :] + lattice[i1][:, i1] * f[None, :]
    return top * (1 - f)[:, None] + bottom * f[:, None]


OCTAVE_RESPONSE = []


def octave_response():
    """How much each octave of the engine's stand-in noise (one at a time, cells 1, 2, 4 ... pixels)
    varies at each size [`octave_bands`] measures: made once, from a fixed seed."""
    import numpy as np

    if not OCTAVE_RESPONSE:
        rng = np.random.default_rng(1)
        m = np.zeros((OCTAVES, OCTAVES))
        for k in range(OCTAVES):
            v = value_noise(OCTAVE_SIDE, 2 ** k, rng)
            m[:, k] = octave_bands(v - v.mean())
        OCTAVE_RESPONSE.append(m)
    return OCTAVE_RESPONSE[0]


def octaves(path):
    """A grain's width in pixels, and how strong each octave of the engine's stand-in noise must be
    (in 10000ths, finest first) for the noise to vary as much at each size as the image does, the
    image averaged to OCTAVE_GRID a side (so that a stand-in's texels are its pixels at that size):
    the spreads at each size add as variances, fitted without going below none."""
    import numpy as np

    im = values(path, None)
    width = im.size[0]
    a = np.asarray(im.resize((OCTAVE_GRID, OCTAVE_GRID), Image.Resampling.BOX), dtype=np.float64) / 255.0
    target = octave_bands(a) ** 2
    m = octave_response() ** 2
    x = np.full(OCTAVES, target.mean() / max(m.mean(), 1e-12))
    for _ in range(3000):
        x *= (m.T @ target) / np.maximum(m.T @ (m @ x), 1e-18)
    return [min(65535, width)] + [min(65535, round(float(np.sqrt(v)) * 10000)) for v in x]


def table(bundle, images, key, measure, unnamed):
    """One table: each image the brush files name under `key`, and those `unnamed` (file names),
    its hash and `measure` of it."""
    out = []
    for base in sorted({base_name(n) for n in named_images(bundle, key)} | unnamed):
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
    other_shapes, other_grains = unnamed_images(bundle, images)
    grains = table(bundle, images, "bundledGrainPath", lambda p: tones(p, SHARES), other_grains)
    shapes = table(bundle, images, "bundledShapePath", lambda p: tones(p, SHAPE_SHARES), other_shapes)
    bands = table(bundle, images, "bundledShapePath", rows, other_shapes)
    columns = table(bundle, images, "bundledShapePath", cols, other_shapes)
    features = table(bundle, images, "bundledGrainPath", feature, other_grains)
    shape_features = table(bundle, images, "bundledShapePath", shape_feature, other_shapes)
    grain_octaves = table(bundle, images, "bundledGrainPath", octaves, other_grains)
    header = """//! Written by tools/brushes/grain_tones.py (don't edit by hand): the tones of the stock grains
//! and shapes other apps' brush files name instead of carrying (see [`stock_grain_tones`] and
//! [`stock_shape_tones`]), each the values at [`TONE_SHARES`] or [`SHAPE_TONE_SHARES`] of it, and
//! each band of a shape's rows' and columns' mean value (see [`stock_shape_rows`] and
//! [`stock_shape_cols`]), each grain's feature size (see [`stock_grain_feature`]), how
//! coarse each shape's texture is (see [`stock_shape_feature`]) and each grain's width and the
//! octaves our noise stands in for it by (see [`stock_grain_octaves`]), keyed by a hash of its
//! file's name.

#![cfg_attr(rustfmt, rustfmt_skip)]

"""
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + f"pub(crate) const STOCK_GRAIN_TONES: &[(u64, [u8; 9])] = &[\n{body(grains)}\n];\n")
        f.write(f"\npub(crate) const STOCK_SHAPE_TONES: &[(u64, [u8; 17])] = &[\n{body(shapes)}\n];\n")
        f.write(f"\npub(crate) const STOCK_SHAPE_ROWS: &[(u64, [u8; {BANDS}])] = &[\n{body(bands)}\n];\n")
        f.write(f"\npub(crate) const STOCK_SHAPE_COLS: &[(u64, [u8; {BANDS}])] = &[\n{body(columns)}\n];\n")
        f.write("\npub(crate) const STOCK_GRAIN_FEATURES: &[(u64, [u16; 4])] = &[\n"
                + "\n".join(f"    (0x{h:016x}, [{', '.join(str(v) for v in t)}])," for h, t in features) + "\n];\n")
        f.write("\npub(crate) const STOCK_SHAPE_FEATURES: &[(u64, [u16; 1])] = &[\n"
                + "\n".join(f"    (0x{h:016x}, [{', '.join(str(v) for v in t)}])," for h, t in shape_features) + "\n];\n")
        f.write(f"\npub(crate) const STOCK_GRAIN_OCTAVES: &[(u64, [u16; {OCTAVES + 1}])] = &[\n{body(grain_octaves)}\n];\n")
    print(f"{len(grains)} grains, {len(shapes)} shapes, {len(features)} grains' features and {len(shape_features)} shapes' measured")


if __name__ == "__main__":
    sys.exit(main())
