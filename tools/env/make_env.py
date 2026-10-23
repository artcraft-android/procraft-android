"""The 3D view's environments: CC0 panoramas from Poly Haven, halved to 512 x 256, scaled so the
brightest light they cast on a surface (their irradiance, the cosine-weighted mean about a normal)
is 1, and written as Radiance .hdr files (flat RGBE) into the app's assets, with their sources in
README.md beside them.

Run from the repo root: python tools/env/make_env.py (needs numpy and the network).
"""
import io
import os
import sys
import urllib.request

import numpy as np

OUT = os.path.join("apps", "procraft-android", "android", "app", "src", "main", "assets", "environments")
WIDTH = 512

# preset -> (Poly Haven id, its title, its authors)
PRESETS = [
    ("studio", "studio_small_09", "Studio Small 09", "Sergej Majboroda"),
    ("sunrise", "spruit_sunrise", "Spruit Sunrise", "Greg Zaal"),
    ("beach", "spiaggia_di_mondello", "Spiaggia di Mondello", "Andreas Mischok"),
    ("coast", "white_cliff_top", "White Cliff Top", "Greg Zaal, Rico Cilliers"),
    ("mountain", "alps_field", "Alps Field", "Andreas Mischok"),
    ("city", "canary_wharf", "Canary Wharf", "Andreas Mischok"),
    ("industrial", "empty_warehouse_01", "Empty Warehouse 01", "Sergej Majboroda"),
    ("auditorium", "dancing_hall", "Dancing Hall", "Sergej Majboroda"),
    ("daytime", "meadow_2", "Meadow 2", "Sergej Majboroda"),
    ("nightlife", "shanghai_bund", "Shanghai Bund", "Greg Zaal"),
    ("portside", "small_harbor_01", "Small Harbor 01", "Greg Zaal"),
]


def read_hdr(data):
    f = io.BytesIO(data)
    if not f.readline().startswith(b"#?"):
        raise ValueError("not a Radiance file")
    while f.readline().strip():
        pass
    _, h, _, w = f.readline().split()
    h, w = int(h), int(w)
    out = np.zeros((h, w, 4), np.uint8)
    for y in range(h):
        head = f.read(4)
        if head[0] == 2 and head[1] == 2:
            for c in range(4):
                x = 0
                while x < w:
                    n = f.read(1)[0]
                    if n > 128:
                        out[y, x:x + n - 128, c] = f.read(1)[0]
                        x += n - 128
                    else:
                        out[y, x:x + n, c] = np.frombuffer(f.read(n), np.uint8)
                        x += n
        else:
            rest = f.read(w * 4 - 4)
            out[y] = np.frombuffer(head + rest, np.uint8).reshape(w, 4)
    e = out[..., 3].astype(np.int32)
    scale = np.where(e > 0, np.ldexp(1.0, e - 136), 0.0)
    return out[..., :3].astype(np.float64) * scale[..., None]


def write_hdr(rgb):
    h, w, _ = rgb.shape
    m = rgb.max(axis=2)
    mant, exp = np.frexp(m)
    scale = np.where(m > 1e-32, mant * 256.0 / np.maximum(m, 1e-32), 0.0)
    out = np.zeros((h, w, 4), np.uint8)
    out[..., :3] = np.clip(rgb * scale[..., None], 0, 255).astype(np.uint8)
    out[..., 3] = np.where(m > 1e-32, exp + 128, 0).astype(np.uint8)
    head = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y %d +X %d\n" % (h, w)
    return head + out.tobytes()


def brightest_irradiance(rgb):
    """The most irradiance (mean of RGB) about any of 2000 normals, from the panorama's spherical
    harmonics (bands 0 to 2), as the app computes it."""
    h, w, _ = rgb.shape
    th = (np.arange(h) + 0.5) / h * np.pi
    ph = ((np.arange(w) + 0.5) / w - 0.5) * 2 * np.pi
    t, p = np.meshgrid(th, ph, indexing="ij")
    x, y, z = np.sin(t) * np.sin(p), np.cos(t), -np.sin(t) * np.cos(p)
    weight = np.sin(t) * (2 * np.pi / w) * (np.pi / h)

    def basis(x, y, z):
        return [0.282095 + 0 * x, 0.488603 * y, 0.488603 * z, 0.488603 * x, 1.092548 * x * y, 1.092548 * y * z,
                0.315392 * (3 * z * z - 1), 1.092548 * x * z, 0.546274 * (x * x - y * y)]

    bands = [1, 2 / 3, 2 / 3, 2 / 3, 0.25, 0.25, 0.25, 0.25, 0.25]
    luma = rgb.mean(axis=2)
    coefficients = [(luma * b * weight).sum() * band for b, band in zip(basis(x, y, z), bands)]
    normals = np.random.default_rng(1).normal(size=(2000, 3))
    normals /= np.linalg.norm(normals, axis=1)[:, None]
    nx, ny, nz = normals.T
    return max(sum(c * b for c, b in zip(coefficients, basis(nx, ny, nz))))


def halve(rgb):
    h, w, _ = rgb.shape
    return rgb.reshape(h // 2, 2, w // 2, 2, 3).mean(axis=(1, 3))


def main():
    os.makedirs(OUT, exist_ok=True)
    lines = [
        "# Environments",
        "",
        "Panoramas of light for the 3D view, halved to 512 x 256, scaled so the most light they cast",
        "on a surface is 1, and stored as Radiance `.hdr` files by `tools/env/make_env.py`. Each is",
        "from Poly Haven (https://polyhaven.com), released under CC0 (public domain).",
        "",
        "| File | Panorama | Authors |",
        "|---|---|---|",
    ]
    for preset, ident, title, authors in PRESETS:
        url = "https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/1k/%s_1k.hdr" % ident
        data = urllib.request.urlopen(url, timeout=120).read()
        rgb = read_hdr(data)
        while rgb.shape[1] > WIDTH:
            rgb = halve(rgb)
        rgb = rgb / brightest_irradiance(rgb)
        with open(os.path.join(OUT, preset + ".hdr"), "wb") as f:
            f.write(write_hdr(rgb))
        lines.append("| %s.hdr | [%s](https://polyhaven.com/a/%s) | %s |" % (preset, title, ident, authors))
        print(preset, rgb.shape, file=sys.stderr)
    with open(os.path.join(OUT, "README.md"), "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
