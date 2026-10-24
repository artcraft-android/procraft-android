//! 3D models: the meshes 3D painting paints on. Read from their files (OBJ, with its MTL), checked
//! as 3D painting needs them (faces with texture coordinates, not too big), and described the way
//! the renderer draws them: corners with a position, a normal and a texture coordinate, triangles,
//! and the parts each material covers (a texture set each).

use std::collections::HashMap;

/// The most triangles a model may have.
pub const MAX_TRIANGLES: usize = 2_000_000;

/// A model's corner as drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Texture coordinates, v up (as the files keep them).
    pub uv: [f32; 2],
}

/// The triangles one material covers: a run of [`Mesh::triangles`].
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// The material's name (empty for faces before any material).
    pub material: String,
    pub first: usize,
    pub count: usize,
}

/// One of the model's meshes (an object or group in an OBJ, a mesh in USD) within a part: its
/// name and its run of [`Mesh::triangles`], inside the part's. A part painted on is shared by
/// every submesh in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Submesh {
    pub name: String,
    pub part: usize,
    pub first: usize,
    pub count: usize,
}

/// What a material file says of one material: its colours and the images it maps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Material {
    pub name: String,
    /// The diffuse colour (Kd).
    pub color: Option<[f32; 3]>,
    /// The images for each channel, by the names the file gives them.
    pub color_map: Option<String>,
    pub roughness_map: Option<String>,
    pub metallic_map: Option<String>,
    pub normal_map: Option<String>,
    pub occlusion_map: Option<String>,
    pub roughness: Option<f32>,
    pub metallic: Option<f32>,
    /// Which of a packed image's components the roughness and metalness are (0 red to 3 alpha;
    /// none, its grey).
    pub roughness_component: Option<usize>,
    pub metallic_component: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[u32; 3]>,
    pub parts: Vec<Part>,
    /// The meshes in each part, in order: each part's triangles are its submeshes' runs, one after
    /// another.
    pub submeshes: Vec<Submesh>,
    pub materials: Vec<Material>,
    /// The material file the model names, if any.
    pub material_file: Option<String>,
}

/// Why a file can't be painted on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// Points or lines only, or nothing.
    NoFaces,
    /// Faces without texture coordinates: there is nothing to paint into.
    NoTextureCoordinates,
    /// More triangles than [`MAX_TRIANGLES`].
    TooLarge,
    /// A line the reader can't make sense of (1-based).
    Malformed(usize),
    /// Not a model file this reader knows, or a damaged one.
    Unreadable,
}

/// A part's roughness and metalness where its material gives none.
pub const PLAIN_ROUGHNESS: f32 = 1.0;
pub const PLAIN_METALLIC: f32 = 0.0;

impl Mesh {
    /// Part `part`'s own surface, under whatever is painted on it: its material's colour (else
    /// white), roughness and metalness (else plain), each 0 to 1.
    pub fn surface(&self, part: usize) -> ([f32; 3], f32, f32) {
        let m = self.parts.get(part).and_then(|p| self.materials.iter().find(|m| m.name == p.material));
        let color = m.and_then(|m| m.color).map_or([1.0; 3], |c| c.map(|x| x.clamp(0.0, 1.0)));
        let roughness = m.and_then(|m| m.roughness).unwrap_or(PLAIN_ROUGHNESS).clamp(0.0, 1.0);
        let metallic = m.and_then(|m| m.metallic).unwrap_or(PLAIN_METALLIC).clamp(0.0, 1.0);
        (color, roughness, metallic)
    }

    /// The smallest box holding the model: its least and most corners.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut it = self.vertices.iter().map(|v| v.position);
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        }))
    }

    /// How much of part `index`'s texture its triangles cover more than once: the share of the
    /// covered texels, sampled on a `side` square grid. Painting there would show in two places.
    pub fn uv_overlap(&self, index: usize, side: usize) -> f32 {
        let Some(part) = self.parts.get(index) else { return 0.0 };
        let side = side.max(8);
        let mut cover = vec![0u8; side * side];
        let tris = self.triangles.get(part.first..part.first + part.count).unwrap_or(&[]);
        for t in tris {
            let uv = t.map(|i| self.vertices.get(i as usize).map_or([0.0, 0.0], |v| v.uv));
            raster_triangle(uv, side, |k| {
                if let Some(c) = cover.get_mut(k) {
                    *c = c.saturating_add(1);
                }
            });
        }
        let covered = cover.iter().filter(|&&c| c > 0).count();
        let twice = cover.iter().filter(|&&c| c > 1).count();
        if covered == 0 { 0.0 } else { twice as f32 / covered as f32 }
    }
}

/// Calls `hit` with the index of each texel (row-major, `side` square, v up: row 0 at v = 1)
/// whose centre a texture-space triangle covers.
fn raster_triangle(uv: [[f32; 2]; 3], side: usize, mut hit: impl FnMut(usize)) {
    let n = side as f32;
    let p = uv.map(|[u, v]| [u * n, (1.0 - v) * n]);
    let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
    if area.abs() < 1e-9 {
        return;
    }
    let (x0, x1) = (p.iter().map(|q| q[0]).fold(f32::MAX, f32::min), p.iter().map(|q| q[0]).fold(f32::MIN, f32::max));
    let (y0, y1) = (p.iter().map(|q| q[1]).fold(f32::MAX, f32::min), p.iter().map(|q| q[1]).fold(f32::MIN, f32::max));
    let clampi = |v: f32| (v.max(0.0) as usize).min(side);
    for y in clampi(y0.floor())..clampi(y1.ceil()) {
        for x in clampi(x0.floor())..clampi(x1.ceil()) {
            let c = [x as f32 + 0.5, y as f32 + 0.5];
            let edge = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            let (w0, w1, w2) = (edge(p[1], p[2]), edge(p[2], p[0]), edge(p[0], p[1]));
            // inside, either winding; edges shared by neighbours count for one side only
            let inside = if area > 0.0 { w0 > 0.0 && w1 > 0.0 && w2 > 0.0 } else { w0 < 0.0 && w1 < 0.0 && w2 < 0.0 };
            if inside {
                hit(y * side + x);
            }
        }
    }
}

/// Reads an OBJ model (and the materials of `mtl`, its material file, if given). Polygons are
/// fanned into triangles; corners sharing a position, texture coordinate and normal are kept
/// once; normals the file lacks are made smooth from the faces around each position.
pub fn read_obj(obj: &str, mtl: Option<&str>) -> Result<Mesh, MeshError> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    // corners: (position, uv, normal) indices, 0-based; usize::MAX for none
    let mut corners: Vec<[usize; 3]> = Vec::new();
    let mut faces: Vec<[usize; 3]> = Vec::new(); // into `corners`
    let mut parts: Vec<Part> = Vec::new();
    // each run's object (or group), and the one faces go in now
    let mut objects: Vec<String> = Vec::new();
    let mut object = String::new();
    let mut material = String::new();
    let mut material_file = None;
    let mut any_face_without_uv = false;
    let mut known: HashMap<[usize; 3], usize> = HashMap::new();

    for (n, line) in obj.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let bad = || MeshError::Malformed(n + 1);
        let floats = |words: std::str::SplitWhitespace<'_>| -> Result<Vec<f32>, MeshError> {
            words.map(|w| w.parse::<f32>().map_err(|_| bad())).collect()
        };
        match key {
            "v" => {
                let f = floats(words)?;
                let (Some(&x), Some(&y), Some(&z)) = (f.first(), f.get(1), f.get(2)) else { return Err(bad()) };
                positions.push([x, y, z]);
            }
            "vt" => {
                let f = floats(words)?;
                let Some(&u) = f.first() else { return Err(bad()) };
                uvs.push([u, f.get(1).copied().unwrap_or(0.0)]);
            }
            "vn" => {
                let f = floats(words)?;
                let (Some(&x), Some(&y), Some(&z)) = (f.first(), f.get(1), f.get(2)) else { return Err(bad()) };
                normals.push([x, y, z]);
            }
            "f" => {
                let mut poly = Vec::new();
                for w in words {
                    let mut fields = w.split('/');
                    let index = |field: Option<&str>, count: usize| -> Result<usize, MeshError> {
                        match field {
                            None | Some("") => Ok(usize::MAX),
                            Some(s) => {
                                let i: i64 = s.parse().map_err(|_| bad())?;
                                // 1-based, or counting back from the latest
                                let at = if i > 0 { i - 1 } else { count as i64 + i };
                                if at < 0 || at as usize >= count { Err(bad()) } else { Ok(at as usize) }
                            }
                        }
                    };
                    let p = index(fields.next(), positions.len())?;
                    if p == usize::MAX {
                        return Err(bad());
                    }
                    let t = index(fields.next(), uvs.len())?;
                    let nrm = index(fields.next(), normals.len())?;
                    if t == usize::MAX {
                        any_face_without_uv = true;
                    }
                    let key = [p, t, nrm];
                    let c = *known.entry(key).or_insert_with(|| {
                        corners.push(key);
                        corners.len() - 1
                    });
                    poly.push(c);
                }
                if poly.len() < 3 {
                    continue;
                }
                if parts.is_empty() {
                    parts.push(Part { material: material.clone(), first: faces.len(), count: 0 });
                    objects.push(object.clone());
                }
                for k in 1..poly.len() - 1 {
                    if let (Some(&a), Some(&b), Some(&c)) = (poly.first(), poly.get(k), poly.get(k + 1)) {
                        faces.push([a, b, c]);
                    }
                }
                if faces.len() > MAX_TRIANGLES {
                    return Err(MeshError::TooLarge);
                }
            }
            "usemtl" => {
                material = words.collect::<Vec<_>>().join(" ");
                // a new run, or back into a material already used (kept apart: parts are runs)
                parts.push(Part { material: material.clone(), first: faces.len(), count: 0 });
                objects.push(object.clone());
            }
            "o" | "g" => {
                object = words.collect::<Vec<_>>().join(" ");
                parts.push(Part { material: material.clone(), first: faces.len(), count: 0 });
                objects.push(object.clone());
            }
            "mtllib" => material_file = Some(words.collect::<Vec<_>>().join(" ")),
            _ => {}
        }
    }
    if any_face_without_uv {
        return Err(MeshError::NoTextureCoordinates);
    }
    let materials = mtl.map(read_mtl).unwrap_or_default();
    let gathered = Gathered { positions, uvs, normals, corners, faces, parts, objects };
    gathered.into_mesh(materials, material_file)
}

/// A model's faces as a file gives them, before they are made a [`Mesh`]: positions, texture
/// coordinates and normals; corners, each an index into those three (`usize::MAX` for none);
/// triangles of corners; runs of triangles by material, and each run's object (its mesh's name).
pub(crate) struct Gathered {
    pub(crate) positions: Vec<[f32; 3]>,
    pub(crate) uvs: Vec<[f32; 2]>,
    pub(crate) normals: Vec<[f32; 3]>,
    pub(crate) corners: Vec<[usize; 3]>,
    pub(crate) faces: Vec<[usize; 3]>,
    pub(crate) parts: Vec<Part>,
    pub(crate) objects: Vec<String>,
}

impl Gathered {
    /// The model: triangles grouped by material in the order materials first appear, and within
    /// a material by object, normals the file lacks made smooth from the faces around each
    /// position.
    pub(crate) fn into_mesh(self, materials: Vec<Material>, material_file: Option<String>) -> Result<Mesh, MeshError> {
        let Gathered { positions, uvs, normals, corners, faces, parts, objects } = self;
        if faces.is_empty() {
            return Err(MeshError::NoFaces);
        }
        if faces.len() > MAX_TRIANGLES {
            return Err(MeshError::TooLarge);
        }
        if uvs.is_empty() || corners.iter().any(|c| c[1] == usize::MAX) {
            return Err(MeshError::NoTextureCoordinates);
        }
        // the runs' sizes and objects, empty ones dropped
        let mut counted: Vec<Part> = Vec::new();
        let mut counted_objects: Vec<&str> = Vec::new();
        for (i, p) in parts.iter().enumerate() {
            let end = parts.get(i + 1).map_or(faces.len(), |q| q.first);
            if end > p.first {
                counted.push(Part { material: p.material.clone(), first: p.first, count: end - p.first });
                counted_objects.push(objects.get(i).map_or("", String::as_str));
            }
        }
        // faces grouped by material, in the order materials first appear
        let mut order: Vec<String> = Vec::new();
        for p in &counted {
            if !order.contains(&p.material) {
                order.push(p.material.clone());
            }
        }
        let mut triangles_by_corner: Vec<[usize; 3]> = Vec::with_capacity(faces.len());
        let mut grouped: Vec<Part> = Vec::new();
        let mut submeshes: Vec<Submesh> = Vec::new();
        for (index, m) in order.iter().enumerate() {
            let first = triangles_by_corner.len();
            // the material's objects in the order they first appear in it, each one's runs together
            let runs: Vec<(&Part, &str)> =
                counted.iter().zip(counted_objects.iter().copied()).filter(|(p, _)| &p.material == m).collect();
            let mut names: Vec<&str> = Vec::new();
            for (_, o) in &runs {
                if !names.contains(o) {
                    names.push(o);
                }
            }
            for name in names {
                let start = triangles_by_corner.len();
                for (p, _) in runs.iter().filter(|(_, o)| *o == name) {
                    triangles_by_corner.extend(faces.get(p.first..p.first + p.count).unwrap_or(&[]));
                }
                let name = if !name.is_empty() {
                    name.to_owned()
                } else if !m.is_empty() {
                    m.clone()
                } else {
                    format!("Mesh {}", submeshes.len() + 1)
                };
                submeshes.push(Submesh { name, part: index, first: start, count: triangles_by_corner.len() - start });
            }
            grouped.push(Part { material: m.clone(), first, count: triangles_by_corner.len() - first });
        }

        // smooth normals for corners without one: each position's faces, weighted by their area
        let needs_normals = corners.iter().any(|c| c[2] == usize::MAX);
        let mut smooth = vec![[0.0f32; 3]; if needs_normals { positions.len() } else { 0 }];
        if needs_normals {
            for f in &triangles_by_corner {
                let p = f.map(|c| corners.get(c).and_then(|k| positions.get(k[0])).copied().unwrap_or([0.0; 3]));
                let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                for c in f {
                    if let Some(s) = corners.get(*c).and_then(|k| smooth.get_mut(k[0])) {
                        *s = [s[0] + n[0], s[1] + n[1], s[2] + n[2]];
                    }
                }
            }
        }
        let vertices = corners
            .iter()
            .map(|&[p, t, nrm]| Vertex {
                position: positions.get(p).copied().unwrap_or([0.0; 3]),
                uv: uvs.get(t).copied().unwrap_or([0.0; 2]),
                normal: normalized(
                    normals.get(nrm).copied().or_else(|| smooth.get(p).copied()).unwrap_or([0.0, 1.0, 0.0]),
                ),
            })
            .collect();
        let triangles = triangles_by_corner.iter().map(|f| f.map(|c| c as u32)).collect();
        Ok(Mesh { vertices, triangles, parts: grouped, submeshes, materials, material_file })
    }
}

/// The model as an OBJ file: each corner's position, texture coordinate and normal, then each
/// part's triangles under its material (`names`, by part), naming `mtl` as its material file.
pub fn write_obj(mesh: &Mesh, mtl: &str, names: &[String]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "mtllib {mtl}");
    for v in &mesh.vertices {
        let [x, y, z] = v.position;
        let _ = writeln!(out, "v {x} {y} {z}");
    }
    for v in &mesh.vertices {
        let [u, w] = v.uv;
        let _ = writeln!(out, "vt {u} {w}");
    }
    for v in &mesh.vertices {
        let [x, y, z] = v.normal;
        let _ = writeln!(out, "vn {x} {y} {z}");
    }
    for (i, part) in mesh.parts.iter().enumerate() {
        let _ = writeln!(out, "usemtl {}", names.get(i).map_or("default", String::as_str));
        for t in mesh.triangles.get(part.first..part.first + part.count).unwrap_or(&[]) {
            let [a, b, c] = t.map(|k| u64::from(k) + 1);
            let _ = writeln!(out, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
        }
    }
    out
}

/// A material file for [`write_obj`]: each material (its name, and the images beside it for its
/// colour, roughness and metalness) white and physically shaded, its properties indented.
pub fn write_mtl(materials: &[(String, [String; 3])]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (name, [color, roughness, metallic]) in materials {
        let _ = write!(
            out,
            "newmtl {name}
\tmap_Kd {color}
\tKd 1 1 1
\tKa 0 0 0
\tKs 0
\tao 0
\tsubsurface 0
\tmap_metallic {metallic}
\tspecularTint 0
\tmap_roughness {roughness}
\tanisotropicRotation 0
\tsheen 0.05
\tsheenTint 0
\tclearCoat 0
\tclearCoatGloss 0

"
        );
    }
    out
}

/// Reads a material file: each material's colour, roughness and metalness, and the images it
/// maps to its colour, roughness, metallic, normal and occlusion channels.
pub fn read_mtl(mtl: &str) -> Vec<Material> {
    let mut out: Vec<Material> = Vec::new();
    for line in mtl.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let rest: Vec<&str> = words.collect();
        if key == "newmtl" {
            out.push(Material { name: rest.join(" "), ..Material::default() });
            continue;
        }
        let Some(m) = out.last_mut() else { continue };
        // a map's file is its last word (options such as -bm 1 come before it)
        let file = || rest.last().map(|s| (*s).to_owned());
        let number = || rest.first().and_then(|s| s.parse::<f32>().ok());
        match key {
            "Kd" => {
                let f: Vec<f32> = rest.iter().filter_map(|s| s.parse().ok()).collect();
                if let (Some(&r), Some(&g), Some(&b)) = (f.first(), f.get(1), f.get(2)) {
                    m.color = Some([r, g, b]);
                }
            }
            "Pr" => m.roughness = number(),
            "Pm" => m.metallic = number(),
            "map_Kd" => m.color_map = file(),
            "map_Pr" | "map_roughness" => m.roughness_map = file(),
            "map_Pm" | "map_metallic" => m.metallic_map = file(),
            "map_Bump" | "map_bump" | "bump" | "norm" | "map_Kn" | "map_tangentSpaceNormal" => m.normal_map = file(),
            "map_Ka" | "map_ao" | "map_AO" => m.occlusion_map = file(),
            _ => {}
        }
    }
    out
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-12 && l.is_finite() { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 1.0, 0.0] }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube, each face wound outwards, all using one quarter of the texture.
    const CUBE: &str = "mtllib cube.mtl
o Cube
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 0 1
v 1 0 1
v 1 1 1
v 0 1 1
vt 0 0
vt 0.5 0
vt 0.5 0.5
vt 0 0.5
usemtl body
f 1/1 4/4 3/3 2/2
f 5/1 6/2 7/3 8/4
usemtl lid
f 4/1 8/2 7/3 3/4
f 1/1 2/2 6/3 5/4
usemtl body
f 2/1 3/2 7/3 6/4
f -8/1 -4/2 -1/3 -5/4
";

    #[test]
    fn a_written_model_reads_back_the_same() {
        let mesh = read_obj(CUBE, None).unwrap();
        let names: Vec<String> = mesh.parts.iter().map(|p| p.material.clone()).collect();
        let text = write_obj(&mesh, "cube.mtl", &names);
        let images = ["body-color.png".into(), "body-roughness.png".into(), "body-metallic.png".into()];
        let again = read_obj(&text, Some(&write_mtl(&[("body".into(), images)]))).unwrap();
        assert_eq!(again.triangles.len(), mesh.triangles.len());
        assert_eq!(again.parts, mesh.parts);
        assert_eq!(again.vertices.len(), mesh.vertices.len());
        // the same corners, triangle by triangle (numbered afresh as they are met)
        for (ta, tb) in again.triangles.iter().zip(&mesh.triangles) {
            for (&ca, &cb) in ta.iter().zip(tb) {
                let (a, b) = (again.vertices[ca as usize], mesh.vertices[cb as usize]);
                assert_eq!((a.position, a.uv), (b.position, b.uv));
                assert!(a.normal.iter().zip(&b.normal).all(|(x, y)| (x - y).abs() < 1e-6));
            }
        }
        assert_eq!(again.material_file.as_deref(), Some("cube.mtl"));
        let m = &again.materials[0];
        assert_eq!(m.color_map.as_deref(), Some("body-color.png"));
        assert_eq!(m.roughness_map.as_deref(), Some("body-roughness.png"));
        assert_eq!(m.metallic_map.as_deref(), Some("body-metallic.png"));
        assert_eq!(m.color, Some([1.0; 3]));
    }

    #[test]
    fn a_parts_surface_is_its_materials_else_plain() {
        let mut mesh = read_obj(CUBE, Some("newmtl body\nKd 1 0.5 2\nPr 0.25\n")).unwrap();
        assert_eq!(mesh.surface(0), ([1.0, 0.5, 1.0], 0.25, PLAIN_METALLIC));
        // a part without a material of its own, and one that isn't there
        assert_eq!(mesh.surface(1), ([1.0; 3], PLAIN_ROUGHNESS, PLAIN_METALLIC));
        assert_eq!(mesh.surface(9), ([1.0; 3], PLAIN_ROUGHNESS, PLAIN_METALLIC));
        mesh.materials.clear();
        assert_eq!(mesh.surface(0).0, [1.0; 3]);
    }

    #[test]
    fn a_cube_reads_as_triangles_by_material() {
        let mesh =
            read_obj(CUBE, Some("newmtl body\nKd 1 0 0\nmap_Kd -bm 1 body color.png\nnewmtl lid\nmap_Pr rough.png\n"))
                .unwrap();
        // six quads, fanned
        assert_eq!(mesh.triangles.len(), 12);
        // the two bodies' runs joined, first as they first came
        assert_eq!(
            mesh.parts,
            vec![
                Part { material: "body".into(), first: 0, count: 8 },
                Part { material: "lid".into(), first: 8, count: 4 },
            ]
        );
        assert_eq!(mesh.material_file.as_deref(), Some("cube.mtl"));
        assert_eq!(mesh.materials.len(), 2);
        assert_eq!(mesh.materials.first().unwrap().color, Some([1.0, 0.0, 0.0]));
        assert_eq!(mesh.materials.first().unwrap().color_map.as_deref(), Some("color.png"));
        assert_eq!(mesh.materials.get(1).unwrap().roughness_map.as_deref(), Some("rough.png"));
        assert_eq!(mesh.bounds(), Some(([0.0; 3], [1.0; 3])));
        // corners sharing position and texture coordinate kept once
        assert!(mesh.vertices.len() < 24, "{}", mesh.vertices.len());
        // smooth normals made: unit length, pointing out of the cube's middle
        for v in &mesh.vertices {
            let n = v.normal;
            assert!(((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() - 1.0).abs() < 1e-4);
            let out = [v.position[0] - 0.5, v.position[1] - 0.5, v.position[2] - 0.5];
            assert!(n[0] * out[0] + n[1] * out[1] + n[2] * out[2] > 0.0, "{v:?}");
        }
    }

    #[test]
    fn objects_are_submeshes_within_their_materials() {
        // a lid and its tab sharing one material, the can in another; more lid at the end
        let obj = "v 0 0 0
v 1 0 0
v 1 1 0
vt 0 0
vt 1 0
vt 1 1
o Lid
usemtl lid
f 1/1 2/2 3/3
o Tab
usemtl lid
f 1/1 2/2 3/3
f 1/1 2/2 3/3
o Can
usemtl can
f 1/1 2/2 3/3
o Lid
usemtl lid
f 1/1 2/2 3/3
";
        let mesh = read_obj(obj, None).unwrap();
        let parts: Vec<(&str, usize, usize)> =
            mesh.parts.iter().map(|p| (p.material.as_str(), p.first, p.count)).collect();
        assert_eq!(parts, [("lid", 0, 4), ("can", 4, 1)]);
        let subs: Vec<(&str, usize, usize, usize)> =
            mesh.submeshes.iter().map(|m| (m.name.as_str(), m.part, m.first, m.count)).collect();
        assert_eq!(subs, [("Lid", 0, 0, 2), ("Tab", 0, 2, 2), ("Can", 1, 4, 1)]);
        // without objects, each material is one, named after it
        let plain = read_obj(
            "v 0 0 0
v 1 0 0
v 1 1 0
vt 0 0
vt 1 0
vt 1 1
f 1/1 2/2 3/3
usemtl red
f 1/1 2/2 3/3
",
            None,
        )
        .unwrap();
        let names: Vec<&str> = plain.submeshes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Mesh 1", "red"]);
    }

    #[test]
    fn models_without_faces_or_texture_coordinates_are_refused() {
        assert_eq!(read_obj("v 0 0 0\nv 1 0 0\nv 0 1 0\n", None), Err(MeshError::NoFaces));
        assert_eq!(read_obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", None), Err(MeshError::NoTextureCoordinates));
        assert_eq!(read_obj("v 0 0 0\nf 1 2 3\n", None), Err(MeshError::Malformed(2)));
        assert_eq!(read_obj("v 0 zero 0\n", None), Err(MeshError::Malformed(1)));
    }

    #[test]
    fn overlapping_texture_coordinates_are_found() {
        // the cube's faces all use the same quarter of the texture: almost all of it twice over
        let mesh = read_obj(CUBE, None).unwrap();
        assert!(mesh.uv_overlap(0, 64) > 0.9, "{}", mesh.uv_overlap(0, 64));
        // two triangles side by side, sharing an edge: none
        let flat = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
        let mesh = read_obj(flat, None).unwrap();
        assert_eq!(mesh.uv_overlap(0, 64), 0.0);
    }

    #[test]
    fn file_normals_are_kept() {
        let tri = "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nvn 0 0 -2\nf 1/1/1 2/2/1 3/3/1\n";
        let mesh = read_obj(tri, None).unwrap();
        assert!(mesh.vertices.iter().all(|v| v.normal == [0.0, 0.0, -1.0]));
    }
}
