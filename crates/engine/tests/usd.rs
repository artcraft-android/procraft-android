//! Reading USD models (see `models/README.md` for how the files were made).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::mesh::Mesh;
use procraft_engine::usd::{read_usdc, read_usdz};

fn model(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/models").join(name)).unwrap()
}

fn uv_of(mesh: &Mesh, corner: u32) -> [f32; 2] {
    mesh.vertices[corner as usize].uv
}

#[test]
fn a_packaged_cube_keeps_its_faces_coordinates_and_materials() {
    let mesh = read_usdz(&model("cube.usdz")).unwrap();
    assert_eq!(mesh.triangles.len(), 12);
    let parts: Vec<(&str, usize)> = mesh.parts.iter().map(|p| (p.material.as_str(), p.count)).collect();
    assert_eq!(parts, [("Body", 8), ("Lid", 4)]);
    // one mesh, in two materials: a submesh in each, named as the mesh
    let subs: Vec<(&str, usize)> = mesh.submeshes.iter().map(|m| (m.name.as_str(), m.part)).collect();
    assert_eq!(subs.iter().map(|s| s.1).collect::<Vec<_>>(), [0, 1]);
    assert!(subs.iter().all(|s| s.0 == subs[0].0 && !s.0.is_empty()), "{subs:?}");
    let (lo, hi) = mesh.bounds().unwrap();
    assert_eq!((lo, hi), ([-1.0; 3], [1.0; 3]));
    // each face covers the whole texture: its corners are the square's four
    let mut corners: Vec<[f32; 2]> = mesh.triangles[..2].iter().flatten().map(|&c| uv_of(&mesh, c)).collect();
    corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
    corners.dedup();
    assert_eq!(corners, [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0], [1.0, 1.0]]);
    // normals made from the faces, pointing out
    for v in &mesh.vertices {
        let d = v.position[0] * v.normal[0] + v.position[1] * v.normal[1] + v.position[2] * v.normal[2];
        assert!(d > 0.0, "{v:?}");
    }
}

#[test]
fn transforms_and_a_z_up_stage_place_the_mesh() {
    let mesh = read_usdc(&model("moved.usdc")).unwrap();
    assert_eq!(mesh.triangles.len(), 12);
    // scaled by 2, moved 10 along x, then z up turned to y up
    let (lo, hi) = mesh.bounds().unwrap();
    for (got, want) in lo.iter().chain(&hi).zip([8.0, -2.0, -2.0, 12.0, 2.0, 2.0]) {
        assert!((got - want).abs() < 1e-5, "{lo:?} {hi:?}");
    }
    // indexed coordinates: still the square's corners
    assert!(mesh.vertices.iter().all(|v| v.uv[0] == 0.0 || v.uv[0] == 1.0));
    assert_eq!(mesh.parts.len(), 1);
}

#[test]
fn compressed_arrays_and_vertex_coordinates_read() {
    let mesh = read_usdc(&model("grid.usdc")).unwrap();
    assert_eq!(mesh.triangles.len(), 800);
    assert_eq!(mesh.vertices.len(), 441);
    // vertex coordinates follow the points: u is x, v is y
    for v in &mesh.vertices {
        assert!((v.uv[0] - v.position[0]).abs() < 1e-6 && (v.uv[1] - v.position[1]).abs() < 1e-6);
    }
}

#[test]
fn damaged_files_are_refused_without_panicking() {
    let bytes = model("grid.usdc");
    for cut in [8, 100, 1000, bytes.len() / 2] {
        assert!(read_usdc(&bytes[..cut]).is_err());
    }
    for cut in bytes.len() - 64..bytes.len() {
        let _ = read_usdc(&bytes[..cut]);
    }
    let mut flipped = bytes.clone();
    for k in (90..flipped.len()).step_by(37) {
        flipped[k] ^= 0x5a;
    }
    let _ = read_usdc(&flipped);
}

#[test]
fn a_written_package_reads_back() {
    let mesh = read_usdz(&model("cube.usdz")).unwrap();
    // the lid has a normal map and ambient occlusion too
    let textures: Vec<procraft_engine::mesh::PartImages> = mesh
        .parts
        .iter()
        .map(|p| {
            let file = |c: &str| (format!("{}-{c}.png", p.material), vec![0; 4]);
            let extra = p.material == "Lid";
            procraft_engine::mesh::PartImages {
                maps: ["color", "roughness", "metallic"].map(file),
                normal: extra.then(|| file("normal")),
                occlusion: extra.then(|| file("ambient_occlusion")),
            }
        })
        .collect();
    let again = read_usdz(&procraft_engine::usd::write_usdz(&mesh, &textures)).unwrap();
    assert_eq!(again.triangles.len(), mesh.triangles.len());
    let parts: Vec<(&str, usize)> = again.parts.iter().map(|p| (p.material.as_str(), p.count)).collect();
    assert_eq!(parts, [("Body", 8), ("Lid", 4)]);
    // each material's images, as written: the greys read from their red
    let maps: Vec<(&str, Option<&str>)> =
        again.materials.iter().map(|m| (m.name.as_str(), m.color_map.as_deref())).collect();
    assert_eq!(maps, [("Body", Some("textures/Body-color.png")), ("Lid", Some("textures/Lid-color.png"))]);
    let lid = &again.materials[1];
    assert_eq!(lid.roughness_map.as_deref(), Some("textures/Lid-roughness.png"));
    assert_eq!(lid.metallic_map.as_deref(), Some("textures/Lid-metallic.png"));
    assert_eq!((lid.roughness_component, lid.metallic_component), (Some(0), Some(0)));
    assert_eq!(lid.normal_map.as_deref(), Some("textures/Lid-normal.png"));
    assert_eq!(lid.occlusion_map.as_deref(), Some("textures/Lid-ambient_occlusion.png"));
    assert_eq!(lid.occlusion_component, Some(0));
    let body = &again.materials[0];
    assert_eq!((body.normal_map.as_deref(), body.occlusion_map.as_deref()), (None, None));
    assert_eq!(again.bounds(), mesh.bounds());
}

#[test]
fn text_layers_read() {
    let text = r#"#usda 1.0
(
    doc = """A "test" layer"""
    upAxis = "Z"
    customLayerData = { string creator = "us" }
)

def Xform "World" (
    kind = "component"
)
{
    float3 xformOp:translate = (0, 0, 5)
    quatf xformOp:orient = (1, 0, 0, 0)
    uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient"]

    def Mesh "Tri" (
        prepend apiSchemas = ["MaterialBindingAPI"]
    )
    {
        int[] faceVertexCounts = [3]
        int[] faceVertexIndices = [0, 1, 2]
        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]
        point3f[] points.timeSamples = { 0: [(0, 0, 0)], }
        texCoord2f[] primvars:uv = [(0, 0), (1, 0), (0, 1)] (
            interpolation = "vertex"
        )
        rel material:binding = </Looks/Paint>
        # a comment
        variantSet "look" = {
            "a" { float x = 1 }
        }
    }
}

def Scope "Looks"
{
    def Material "Paint"
    {
        token outputs:surface.connect = </Looks/Paint/S.outputs:surface>
    }
}
"#;
    let mesh = procraft_engine::usd::read_usda(text).unwrap();
    assert_eq!(mesh.triangles.len(), 1);
    assert_eq!(mesh.parts[0].material, "Paint");
    // moved 5 up z, then z up turned to y up
    let (lo, hi) = mesh.bounds().unwrap();
    assert_eq!((lo, hi), ([0.0, 5.0, -1.0], [1.0, 5.0, 0.0]));
    assert!(mesh.vertices.iter().any(|v| v.uv == [1.0, 0.0]));
    // nonsense is refused, without panicking
    for bad in [
        "def",
        "def Mesh \"A\" {",
        "#usda 1.0\n(\n",
        "def Mesh \"A\" { int[] faceVertexCounts = [3 }",
        "\"unterminated",
    ] {
        assert!(procraft_engine::usd::read_usda(bad).is_err(), "{bad}");
    }
}

#[test]
fn text_copies_read_as_their_binary_layers() {
    for (text, binary) in [("cube.usda", "cube.usdz"), ("moved.usda", "moved.usdc"), ("grid.usda", "grid.usdc")] {
        let a = procraft_engine::usd::read_usd(&model(text)).unwrap();
        let b = if binary.ends_with(".usdz") { read_usdz(&model(binary)) } else { read_usdc(&model(binary)) }.unwrap();
        assert_eq!(a, b, "{text}");
    }
}
