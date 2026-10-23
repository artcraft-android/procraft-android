# Test models

Small USD models for the reader's tests, made by `tools/usd/make_usd.py` with Pixar's `usd-core`
Python package (`pip install usd-core`, then `python make_usd.py <out dir>`). They are ours: the
geometry is a cube and a grid written by the script, no outside content.

- `cube.usdz`: a cube with face-varying texture coordinates and two materials bound to
  `GeomSubset`s (Body: four faces, Lid: two), packaged as USDZ.
- `moved.usdc`: the cube with indexed texture coordinates, scaled by 2 inside an Xform moved
  10 along x, on a Z-up stage.
- `grid.usdc`: a 20 by 20 grid of quads with vertex texture coordinates (large enough for its
  integer arrays to be compressed).
- `cube.usda`, `moved.usda`, `grid.usda`: the same layers in USD's text form (exported by the
  script with `Sdf.Layer.Export`).
