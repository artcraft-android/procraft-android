"""Writes the engine's table of stock grain tones (crates/engine/src/stock_tones.rs): for each grain
image another app's brush files name instead of carrying it, the values at nine shares of it
(none, 5%, 10%, a quarter, half, three quarters, 90%, 95%, all), measured as the engine reads an
image (its luminance times its alpha, averaged down to 256 square). Each is keyed by a hash of
its file's name, so the table holds measurements only: neither the images nor their names.

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
SIDE = 256


def fnv1a(text):
    """FNV-1a, 64 bits, of the name in lower case (as the engine hashes it)."""
    h = 0xCBF29CE484222325
    for b in text.lower().encode("utf-8"):
        h = ((h ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def named_grains(bundle):
    """The grain images the bundle's brush files name, each once."""
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
            v = top.get("bundledGrainPath")
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


def tones(path):
    """The image's values at the shares, as the engine reads it."""
    im = Image.open(path)
    if im.mode in ("LA", "RGBA", "PA") or "transparency" in im.info:
        rgba = im.convert("RGBA")
        lum = rgba.convert("L")
        alpha = rgba.getchannel("A")
        values = Image.frombytes("L", rgba.size, bytes(l * a // 255 for l, a in zip(lum.tobytes(), alpha.tobytes())))
    else:
        values = im.convert("L")
    # each of the engine's pixels the average of the image's it covers
    values = values.resize((SIDE, SIDE), Image.Resampling.BOX)
    px = sorted(values.tobytes())
    return [px[round(s * (len(px) - 1))] for s in SHARES]


def main():
    bundle = sys.argv[1]
    images = images_by_name(bundle)
    rows = []
    for name in sorted(named_grains(bundle)):
        base = name.replace("\\", "/").split("/")[-1].lower()
        path = images.get(base)
        if path is None:
            continue
        t = tones(path)
        rows.append((fnv1a(base), t))
    rows.sort()
    body = "\n".join(f"    (0x{h:016x}, [{', '.join(str(v) for v in t)}])," for h, t in rows)
    header = """//! Written by tools/brushes/grain_tones.py (don't edit by hand): the tones of the stock grains
//! other apps' brush files name instead of carrying (see [`stock_grain_tones`]), each the values
//! at [`TONE_SHARES`] of it, keyed by a hash of its file's name.

#![cfg_attr(rustfmt, rustfmt_skip)]

"""
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + f"pub(crate) const STOCK_GRAIN_TONES: &[(u64, [u8; 9])] = &[\n{body}\n];\n")
    print(f"{len(rows)} grains measured")


if __name__ == "__main__":
    sys.exit(main())
