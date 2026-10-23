"""Test models for the USD reader, written with usd-core (run isolated: python -I make_usd.py OUT_DIR)."""
import os
import sys
import zipfile

from pxr import Gf, Sdf, Usd, UsdGeom, UsdShade, Vt

out = sys.argv[1]
os.makedirs(out, exist_ok=True)

# a unit cube: 8 points, 6 quads, face-varying UVs (each face the whole square), two materials
POINTS = [(-1, -1, -1), (1, -1, -1), (1, 1, -1), (-1, 1, -1), (-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1)]
FACES = [[0, 3, 2, 1], [4, 5, 6, 7], [3, 7, 6, 2], [0, 1, 5, 4], [1, 2, 6, 5], [0, 4, 7, 3]]
SQUARE = [(0, 0), (1, 0), (1, 1), (0, 1)]


def cube(stage, path, indexed=False, two_materials=True):
    mesh = UsdGeom.Mesh.Define(stage, path)
    mesh.CreatePointsAttr([Gf.Vec3f(*p) for p in POINTS])
    mesh.CreateFaceVertexCountsAttr([4] * 6)
    mesh.CreateFaceVertexIndicesAttr([i for f in FACES for i in f])
    pv = UsdGeom.PrimvarsAPI(mesh).CreatePrimvar("st", Sdf.ValueTypeNames.TexCoord2fArray, UsdGeom.Tokens.faceVarying)
    if indexed:
        pv.Set([Gf.Vec2f(*uv) for uv in SQUARE])
        pv.SetIndices(Vt.IntArray([0, 1, 2, 3] * 6))
    else:
        pv.Set([Gf.Vec2f(*uv) for _ in FACES for uv in SQUARE])
    if two_materials:
        for name, faces in (("Body", [0, 1, 4, 5]), ("Lid", [2, 3])):
            mat = UsdShade.Material.Define(stage, "/Looks/" + name)
            subset = UsdGeom.Subset.Define(stage, path + "/" + name.lower())
            subset.CreateElementTypeAttr("face")
            subset.CreateFamilyNameAttr("materialBind")
            subset.CreateIndicesAttr(faces)
            UsdShade.MaterialBindingAPI.Apply(subset.GetPrim()).Bind(mat)
    return mesh


def write(name, build, up_z=False):
    stage = Usd.Stage.CreateNew(os.path.join(out, name + ".usdc"))
    if up_z:
        UsdGeom.SetStageUpAxis(stage, UsdGeom.Tokens.z)
    build(stage)
    stage.GetRootLayer().Save()


write("cube", lambda s: cube(s, "/Cube"))


def moved(stage):
    xf = UsdGeom.Xform.Define(stage, "/World")
    xf.AddTranslateOp().Set(Gf.Vec3d(10, 0, 0))
    m = cube(stage, "/World/Cube", indexed=True, two_materials=False)
    m.AddScaleOp().Set(Gf.Vec3f(2, 2, 2))


write("moved", moved, up_z=True)

# a bigger mesh so its arrays are compressed (a grid of quads with vertex-interpolated UVs)
def grid(stage):
    n = 20
    mesh = UsdGeom.Mesh.Define(stage, "/Grid")
    mesh.CreatePointsAttr([Gf.Vec3f(x / n, y / n, 0) for y in range(n + 1) for x in range(n + 1)])
    mesh.CreateFaceVertexCountsAttr([4] * (n * n))
    idx = []
    for y in range(n):
        for x in range(n):
            a = y * (n + 1) + x
            idx += [a, a + 1, a + n + 2, a + n + 1]
    mesh.CreateFaceVertexIndicesAttr(idx)
    pv = UsdGeom.PrimvarsAPI(mesh).CreatePrimvar("st", Sdf.ValueTypeNames.TexCoord2fArray, UsdGeom.Tokens.vertex)
    pv.Set([Gf.Vec2f(x / n, y / n) for y in range(n + 1) for x in range(n + 1)])


write("grid", grid)

# text copies of the binary layers
for name in ("cube", "moved", "grid"):
    Sdf.Layer.FindOrOpen(os.path.join(out, name + ".usdc")).Export(os.path.join(out, name + ".usda"))

# the cube packaged as a usdz (stored, aligned to 64 bytes as the format asks)
from pxr import UsdUtils  # noqa: E402

UsdUtils.CreateNewUsdzPackage(os.path.join(out, "cube.usdc"), os.path.join(out, "cube.usdz"))
print(sorted(os.listdir(out)))
