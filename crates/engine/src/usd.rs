//! Models in USD's binary form (a `.usdc` crate file, or a `.usdz` package holding one): the
//! meshes of the stage, placed by their transforms, with their texture coordinates and the
//! materials bound to their faces. Read from the file format as published, without USD's own
//! library: no layer composition (references, payloads, variants), only what one file holds.
//!
//! A crate file starts with its magic, version and the table of contents' place. Its sections
//! hold the tokens (compressed text), the fields (a name and a value's place each), the field
//! sets, the paths (a compressed tree) and the specs (each a path, its field set and its kind).
//! Values are 64-bit "reps": a type, flags (array, inlined, compressed) and either the value
//! itself or where it is in the file.

use std::collections::HashMap;

use crate::applz4::raw_block;
use crate::mesh::{Gathered, MAX_TRIANGLES, Material, Mesh, MeshError, Part};
use crate::zip::{ZipReader, ZipWriter};

/// Reads a model from a `.usdz` package: its first layer (binary or text).
pub fn read_usdz(bytes: &[u8]) -> Result<Mesh, MeshError> {
    let zip = ZipReader::new(bytes).ok_or(MeshError::Unreadable)?;
    let first = zip
        .names()
        .find(|n| n.ends_with(".usdc") || n.ends_with(".usd") || n.ends_with(".usda"))
        .ok_or(MeshError::Unreadable)?;
    let layer = zip.read(first).ok_or(MeshError::Unreadable)?;
    read_usd(&layer)
}

/// Reads a model from a USD layer, binary (a crate file) or text.
pub fn read_usd(bytes: &[u8]) -> Result<Mesh, MeshError> {
    if bytes.starts_with(b"PXR-USDC") { read_usdc(bytes) } else { read_usda(&String::from_utf8_lossy(bytes)) }
}

/// Reads a model from a crate file (`.usdc`, or a binary `.usd`).
pub fn read_usdc(bytes: &[u8]) -> Result<Mesh, MeshError> {
    let file = Crate::read(bytes).ok_or(MeshError::Unreadable)?;
    Stage::of_crate(&file).model()
}

/// Writes the model as a USDZ package: a text layer (`model.usda`) with the mesh, a subset of its
/// faces for each part bound to a preview-surface material, and each part's images beside it
/// (`textures`, by part: its colour, roughness and metalness, each a file name and its PNG).
pub fn write_usdz(mesh: &Mesh, textures: &[[(String, Vec<u8>); 3]]) -> Vec<u8> {
    let layer = write_usda(mesh, textures.iter().map(|t| t.each_ref().map(|(name, _)| name.as_str())));
    let mut zip = ZipWriter::new();
    zip.add_aligned("model.usda", layer.as_bytes(), 64);
    for (name, png) in textures.iter().flatten() {
        zip.add_aligned(&format!("textures/{name}"), png, 64);
    }
    zip.finish()
}

/// A name USD takes for a prim: letters, digits and `_`, not starting with a digit.
fn identifier(name: &str, otherwise: &str) -> String {
    let mut out: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if out.is_empty() {
        out = otherwise.to_owned();
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// The text layer of [`write_usdz`]: the mesh's corners as its points (each with its normal and
/// texture coordinate), its triangles, and per part a subset and a material reading `textures`
/// (its colour, roughness and metalness images).
fn write_usda<'a>(mesh: &Mesh, textures: impl Iterator<Item = [&'a str; 3]>) -> String {
    let mut names: Vec<String> = Vec::new();
    for (i, part) in mesh.parts.iter().enumerate() {
        let mut n = identifier(&part.material, "Material");
        if names.contains(&n) {
            n = format!("{n}_{}", i + 1);
        }
        names.push(n);
    }
    let list = |items: Vec<String>| items.join(", ");
    let mut lines: Vec<(usize, String)> = Vec::new();
    let mut line = |depth: usize, text: String| lines.push((depth, text));
    line(0, "#usda 1.0".into());
    line(0, "(".into());
    line(1, "defaultPrim = \"Model\"".into());
    line(1, "metersPerUnit = 1".into());
    line(1, "upAxis = \"Y\"".into());
    line(0, ")".into());
    line(0, String::new());
    line(0, "def Xform \"Model\"".into());
    line(0, "{".into());
    line(1, "def Mesh \"Mesh\" (".into());
    line(2, "prepend apiSchemas = [\"MaterialBindingAPI\"]".into());
    line(1, ")".into());
    line(1, "{".into());
    line(2, "uniform bool doubleSided = 1".into());
    line(2, format!("int[] faceVertexCounts = [{}]", list(vec!["3".to_owned(); mesh.triangles.len()])));
    let corners = mesh.triangles.iter().flatten().map(u32::to_string).collect();
    line(2, format!("int[] faceVertexIndices = [{}]", list(corners)));
    let points = mesh.vertices.iter().map(|v| format!("({}, {}, {})", v.position[0], v.position[1], v.position[2]));
    line(2, format!("point3f[] points = [{}]", list(points.collect())));
    let normals = mesh.vertices.iter().map(|v| format!("({}, {}, {})", v.normal[0], v.normal[1], v.normal[2]));
    line(2, format!("normal3f[] normals = [{}] (", list(normals.collect())));
    line(3, "interpolation = \"vertex\"".into());
    line(2, ")".into());
    let uvs = mesh.vertices.iter().map(|v| format!("({}, {})", v.uv[0], v.uv[1]));
    line(2, format!("texCoord2f[] primvars:st = [{}] (", list(uvs.collect())));
    line(3, "interpolation = \"vertex\"".into());
    line(2, ")".into());
    line(2, "uniform token subdivisionScheme = \"none\"".into());
    // every face in exactly one subset
    line(2, "uniform token subsetFamily:materialBind:familyType = \"partition\"".into());
    for (part, name) in mesh.parts.iter().zip(&names) {
        let faces = (part.first..part.first + part.count).map(|f| f.to_string()).collect();
        line(0, String::new());
        line(2, format!("def GeomSubset \"{name}\" ("));
        line(3, "prepend apiSchemas = [\"MaterialBindingAPI\"]".into());
        line(2, ")".into());
        line(2, "{".into());
        line(3, "uniform token elementType = \"face\"".into());
        line(3, "uniform token familyName = \"materialBind\"".into());
        line(3, format!("int[] indices = [{}]", list(faces)));
        line(3, format!("rel material:binding = </Model/Materials/{name}>"));
        line(2, "}".into());
    }
    line(1, "}".into());
    line(0, String::new());
    line(1, "def Scope \"Materials\"".into());
    line(1, "{".into());
    for (name, [color, roughness, metallic]) in names.iter().zip(textures) {
        let m = format!("/Model/Materials/{name}");
        line(2, format!("def Material \"{name}\""));
        line(2, "{".into());
        line(3, format!("token outputs:surface.connect = <{m}/Surface.outputs:surface>"));
        line(0, String::new());
        line(3, "def Shader \"Surface\"".into());
        line(3, "{".into());
        line(4, "uniform token info:id = \"UsdPreviewSurface\"".into());
        line(4, format!("color3f inputs:diffuseColor.connect = <{m}/Texture.outputs:rgb>"));
        line(4, format!("float inputs:roughness.connect = <{m}/Roughness.outputs:r>"));
        line(4, format!("float inputs:metallic.connect = <{m}/Metallic.outputs:r>"));
        line(4, "token outputs:surface".into());
        line(3, "}".into());
        // the colour as it is, the greys as numbers (their red)
        for (shader, file, output) in [
            ("Texture", color, "float3 outputs:rgb"),
            ("Roughness", roughness, "float outputs:r"),
            ("Metallic", metallic, "float outputs:r"),
        ] {
            line(0, String::new());
            line(3, format!("def Shader \"{shader}\""));
            line(3, "{".into());
            line(4, "uniform token info:id = \"UsdUVTexture\"".into());
            line(4, format!("asset inputs:file = @textures/{file}@"));
            if shader != "Texture" {
                line(4, "token inputs:sourceColorSpace = \"raw\"".into());
            }
            line(4, format!("float2 inputs:st.connect = <{m}/Coordinates.outputs:result>"));
            line(4, output.into());
            line(3, "}".into());
        }
        line(0, String::new());
        line(3, "def Shader \"Coordinates\"".into());
        line(3, "{".into());
        line(4, "uniform token info:id = \"UsdPrimvarReader_float2\"".into());
        line(4, "string inputs:varname = \"st\"".into());
        line(4, "float2 outputs:result".into());
        line(3, "}".into());
        line(2, "}".into());
    }
    line(1, "}".into());
    line(0, "}".into());
    let mut out = String::new();
    for (depth, text) in lines {
        if !text.is_empty() {
            out.push_str(&"    ".repeat(depth));
            out.push_str(&text);
        }
        out.push('\n');
    }
    out
}

// ---- reading bytes -----------------------------------------------------------------------------

fn u8_at(d: &[u8], at: usize) -> Option<u8> {
    d.get(at).copied()
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

fn f32_at(d: &[u8], at: usize) -> Option<f32> {
    u32_at(d, at).map(f32::from_bits)
}

fn f64_at(d: &[u8], at: usize) -> Option<f64> {
    u64_at(d, at).map(f64::from_bits)
}

fn usize_at(d: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u64_at(d, at)?).ok()
}

/// The crate's own framing round LZ4: a chunk count (0: one block), then each chunk's size and
/// block.
fn decompress(src: &[u8], limit: usize) -> Option<Vec<u8>> {
    let chunks = *src.first()?;
    if chunks == 0 {
        return raw_block(src.get(1..)?, limit);
    }
    let mut out = Vec::new();
    let mut i = 1;
    for _ in 0..chunks {
        let size = usize::try_from(u32_at(src, i)?).ok()?;
        i += 4;
        out.extend(raw_block(src.get(i..i.checked_add(size)?)?, limit.checked_sub(out.len())?)?);
        i += size;
    }
    Some(out)
}

/// `n` integers coded as running sums of deltas: the most common delta, two bits a delta saying
/// whether it is that or how wide it is written, then the written ones.
fn decode_ints(buf: &[u8], n: usize, wide: bool) -> Option<Vec<i64>> {
    // two bits each at the least
    if n > buf.len().saturating_mul(4) {
        return None;
    }
    let (common, mut at) = if wide { (u64_at(buf, 0)? as i64, 8) } else { (i64::from(u32_at(buf, 0)? as i32), 4) };
    let codes = at;
    at += (n * 2).div_ceil(8);
    let mut out = Vec::with_capacity(n);
    let mut sum: i64 = 0;
    for k in 0..n {
        let code = (u8_at(buf, codes + k / 4)? >> ((k % 4) * 2)) & 3;
        let delta = match (code, wide) {
            (0, _) => common,
            (1, false) => i64::from(*buf.get(at)? as i8),
            (2, false) => i64::from(i16::from_le_bytes(buf.get(at..at + 2)?.try_into().ok()?)),
            (3, false) => i64::from(u32_at(buf, at)? as i32),
            (1, true) => i64::from(i16::from_le_bytes(buf.get(at..at + 2)?.try_into().ok()?)),
            (2, true) => i64::from(u32_at(buf, at)? as i32),
            _ => u64_at(buf, at)? as i64,
        };
        at += match (code, wide) {
            (0, _) => 0,
            (1, false) => 1,
            (2, false) | (1, true) => 2,
            (3, false) | (2, true) => 4,
            _ => 8,
        };
        sum = sum.wrapping_add(delta);
        out.push(sum);
    }
    Some(out)
}

/// Compressed integers at `at`: their compressed size, then the compressed bytes. Returns them
/// and where they end.
fn compressed_ints(d: &[u8], at: usize, n: usize, wide: bool) -> Option<(Vec<i64>, usize)> {
    let size = usize_at(d, at)?;
    let start = at.checked_add(8)?;
    let end = start.checked_add(size)?;
    let width = if wide { 8 } else { 4 };
    let limit = width + n.div_ceil(4) + n.checked_mul(width)?;
    let raw = decompress(d.get(start..end)?, limit)?;
    Some((decode_ints(&raw, n, wide)?, end))
}

// ---- the crate file --------------------------------------------------------------------------

/// A value's type, as the crate numbers them (those read here).
mod kind {
    pub const INT: u8 = 3;
    pub const UINT: u8 = 4;
    pub const HALF: u8 = 7;
    pub const FLOAT: u8 = 8;
    pub const DOUBLE: u8 = 9;
    pub const TOKEN: u8 = 11;
    pub const ASSET: u8 = 12;
    pub const MATRIX4D: u8 = 15;
    pub const QUATD: u8 = 16;
    pub const QUATF: u8 = 17;
    pub const VEC2D: u8 = 19;
    pub const VEC2F: u8 = 20;
    pub const VEC2H: u8 = 21;
    pub const VEC3D: u8 = 23;
    pub const VEC3F: u8 = 24;
    pub const VEC3H: u8 = 25;
    pub const PATH_LIST_OP: u8 = 34;
    pub const TOKEN_VECTOR: u8 = 41;
}

/// A value's place and kind.
#[derive(Clone, Copy, Debug)]
struct Rep(u64);

impl Rep {
    fn is_array(self) -> bool {
        self.0 >> 63 & 1 == 1
    }
    fn is_inlined(self) -> bool {
        self.0 >> 62 & 1 == 1
    }
    fn is_compressed(self) -> bool {
        self.0 >> 61 & 1 == 1
    }
    fn kind(self) -> u8 {
        (self.0 >> 48) as u8
    }
    fn payload(self) -> u64 {
        self.0 & ((1 << 48) - 1)
    }
    fn offset(self) -> Option<usize> {
        usize::try_from(self.payload()).ok()
    }
}

struct Crate<'a> {
    data: &'a [u8],
    /// Whether array sizes are 64-bit (version 0.7 on).
    wide_counts: bool,
    tokens: Vec<String>,
    /// Each field: its name (a token) and value.
    fields: Vec<(usize, Rep)>,
    /// Field indices, each set ended by `u32::MAX`.
    field_sets: Vec<i64>,
    /// Each path: its parent's index, its last element (a token) and whether it names a property.
    paths: Vec<Option<(Option<usize>, usize, bool)>>,
    /// Each spec: its path and the start of its field set.
    specs: Vec<(usize, usize)>,
}

impl<'a> Crate<'a> {
    fn read(data: &'a [u8]) -> Option<Crate<'a>> {
        if data.get(..8)? != b"PXR-USDC" {
            return None;
        }
        let (major, minor) = (u8_at(data, 8)?, u8_at(data, 9)?);
        // version 0.4 on: compressed sections
        if major != 0 || minor < 4 {
            return None;
        }
        let toc = usize_at(data, 16)?;
        let count = usize_at(data, toc)?;
        let mut sections: HashMap<String, usize> = HashMap::new();
        for k in 0..count.min(64) {
            let at = toc + 8 + k * 32;
            let name = data.get(at..at + 16)?.split(|&b| b == 0).next()?;
            sections.insert(String::from_utf8_lossy(name).into_owned(), usize_at(data, at + 16)?);
        }
        let section = |name: &str| sections.get(name).copied();

        let at = section("TOKENS")?;
        let (count, size, packed) = (usize_at(data, at)?, usize_at(data, at + 8)?, usize_at(data, at + 16)?);
        let text = decompress(data.get(at + 24..(at + 24).checked_add(packed)?)?, size)?;
        let tokens: Vec<String> =
            text.split(|&b| b == 0).take(count).map(|t| String::from_utf8_lossy(t).into_owned()).collect();

        let at = section("FIELDS")?;
        let count = usize_at(data, at)?;
        let (names, end) = compressed_ints(data, at + 8, count, false)?;
        let packed = usize_at(data, end)?;
        let raw = decompress(data.get(end + 8..(end + 8).checked_add(packed)?)?, count.checked_mul(8)?)?;
        let fields = names
            .iter()
            .enumerate()
            .map(|(k, &name)| Some((usize::try_from(name).ok()?, Rep(u64_at(&raw, k * 8)?))))
            .collect::<Option<Vec<_>>>()?;

        let at = section("FIELDSETS")?;
        let (field_sets, _) = compressed_ints(data, at + 8, usize_at(data, at)?, false)?;

        let at = section("PATHS")?;
        let (count, encoded) = (usize_at(data, at)?, usize_at(data, at + 8)?);
        if count > data.len() || encoded > data.len() {
            return None;
        }
        let (indices, end) = compressed_ints(data, at + 16, encoded, false)?;
        let (elements, end) = compressed_ints(data, end, encoded, false)?;
        let (jumps, _) = compressed_ints(data, end, encoded, false)?;
        let mut paths = vec![None; count];
        build_paths(&indices, &elements, &jumps, &mut paths)?;

        let at = section("SPECS")?;
        let count = usize_at(data, at)?;
        let (spec_paths, end) = compressed_ints(data, at + 8, count, false)?;
        let (spec_sets, _) = compressed_ints(data, end, count, false)?;
        let specs = spec_paths
            .iter()
            .zip(&spec_sets)
            .map(|(&p, &s)| Some((usize::try_from(p).ok()?, usize::try_from(s).ok()?)))
            .collect::<Option<Vec<_>>>()?;

        Some(Crate { data, wide_counts: minor >= 7, tokens, fields, field_sets, paths, specs })
    }

    fn token(&self, index: u64) -> Option<&str> {
        self.tokens.get(usize::try_from(index).ok()?).map(String::as_str)
    }

    /// Path `index` written out ("/World/Cube", "/World/Cube.points").
    fn path(&self, index: usize) -> Option<String> {
        let mut names = Vec::new();
        let mut at = Some(index);
        let mut depth = 0;
        while let Some(i) = at {
            let (parent, element, property) = (*self.paths.get(i)?)?;
            if parent.is_some() {
                names.push((self.tokens.get(element)?.as_str(), property));
            }
            at = parent;
            depth += 1;
            if depth > 4096 {
                return None;
            }
        }
        let mut out = String::new();
        for (name, property) in names.iter().rev() {
            out.push(if *property { '.' } else { '/' });
            out.push_str(name);
        }
        Some(if out.is_empty() { "/".to_owned() } else { out })
    }

    /// Where an array's elements start and how many there are.
    fn array(&self, rep: Rep) -> Option<(usize, usize)> {
        let at = rep.offset()?;
        if self.wide_counts {
            Some((at + 8, usize_at(self.data, at)?))
        } else {
            Some((at + 4, usize::try_from(u32_at(self.data, at)?).ok()?))
        }
    }

    fn ints(&self, rep: Rep) -> Option<Vec<i64>> {
        if !rep.is_array() || !matches!(rep.kind(), kind::INT | kind::UINT) {
            return None;
        }
        let (at, n) = self.array(rep)?;
        if rep.is_compressed() {
            return Some(compressed_ints(self.data, at, n, false)?.0);
        }
        (0..n).map(|k| u32_at(self.data, at + k * 4).map(|v| i64::from(v as i32))).collect()
    }

    /// An array of `width`-component vectors of floats, doubles or halves.
    fn vectors<const N: usize>(&self, rep: Rep) -> Option<Vec<[f32; N]>> {
        if !rep.is_array() {
            return None;
        }
        let size = match (rep.kind(), N) {
            (kind::VEC2F, 2) | (kind::VEC3F, 3) => 4,
            (kind::VEC2D, 2) | (kind::VEC3D, 3) => 8,
            (kind::VEC2H, 2) | (kind::VEC3H, 3) => 2,
            _ => return None,
        };
        let (at, n) = self.array(rep)?;
        let mut out = Vec::with_capacity(n.min(self.data.len()));
        for k in 0..n {
            let mut v = [0.0f32; N];
            for (c, slot) in v.iter_mut().enumerate() {
                let p = at + (k * N + c) * size;
                *slot = match size {
                    4 => f32_at(self.data, p)?,
                    8 => f64_at(self.data, p)? as f32,
                    _ => half(u16::from_le_bytes(self.data.get(p..p + 2)?.try_into().ok()?)),
                };
            }
            out.push(v);
        }
        Some(out)
    }

    fn token_value(&self, rep: Rep) -> Option<&str> {
        (rep.kind() == kind::TOKEN && rep.is_inlined() && !rep.is_array()).then(|| self.token(rep.payload()))?
    }

    /// A list of tokens (a token vector, or an array of tokens).
    fn token_list(&self, rep: Rep) -> Option<Vec<String>> {
        let (at, n) = match (rep.kind(), rep.is_array()) {
            (kind::TOKEN_VECTOR, false) => (rep.offset()? + 8, usize_at(self.data, rep.offset()?)?),
            (kind::TOKEN, true) => self.array(rep)?,
            _ => return None,
        };
        (0..n).map(|k| self.token(u64::from(u32_at(self.data, at + k * 4)?)).map(str::to_owned)).collect()
    }

    /// The first path a path list op names (explicit, prepended, appended or added).
    fn first_path(&self, rep: Rep) -> Option<String> {
        if rep.kind() != kind::PATH_LIST_OP {
            return None;
        }
        let at = rep.offset()?;
        let header = u8_at(self.data, at)?;
        let mut p = at + 1;
        // explicit, added, deleted, ordered, prepended, appended: each a count and path indices
        for bit in [2u8, 4, 8, 16, 32, 64] {
            if header & bit == 0 {
                continue;
            }
            let n = usize_at(self.data, p)?;
            p += 8;
            if n > 0 && bit != 8 {
                return self.path(usize::try_from(u32_at(self.data, p)?).ok()?);
            }
            p += n * 4;
        }
        None
    }

    /// A number (inlined or not), as a double.
    fn number(&self, rep: Rep) -> Option<f64> {
        if rep.is_array() {
            return None;
        }
        let pay = rep.payload();
        match (rep.kind(), rep.is_inlined()) {
            (kind::FLOAT, true) => Some(f64::from(f32::from_bits(pay as u32))),
            // a double written as a float when that loses nothing
            (kind::DOUBLE, true) => Some(f64::from(f32::from_bits(pay as u32))),
            (kind::HALF, true) => Some(f64::from(half(pay as u16))),
            (kind::INT, true) => Some(f64::from(pay as u32 as i32)),
            (kind::FLOAT, false) => f32_at(self.data, rep.offset()?).map(f64::from),
            (kind::DOUBLE, false) => f64_at(self.data, rep.offset()?),
            _ => None,
        }
    }

    /// A three-component vector (inlined ones are small whole numbers).
    fn vector3(&self, rep: Rep) -> Option<[f64; 3]> {
        if rep.is_array() {
            return None;
        }
        if rep.is_inlined() {
            let b = rep.payload().to_le_bytes();
            return Some([0, 1, 2].map(|k| f64::from(b.get(k).copied().unwrap_or(0) as i8)));
        }
        let at = rep.offset()?;
        match rep.kind() {
            kind::VEC3D => Some([f64_at(self.data, at)?, f64_at(self.data, at + 8)?, f64_at(self.data, at + 16)?]),
            kind::VEC3F => Some([0, 1, 2].map(|k| f32_at(self.data, at + k * 4).map_or(0.0, f64::from))),
            kind::VEC3H => Some([0, 1, 2].map(|k| {
                let p = at + k * 2;
                self.data
                    .get(p..p + 2)
                    .and_then(|s| s.try_into().ok())
                    .map_or(0.0, |b| f64::from(half(u16::from_le_bytes(b))))
            })),
            _ => None,
        }
    }

    /// A 4×4 matrix, rows as USD writes them (points as rows, the translation in the last row).
    fn matrix(&self, rep: Rep) -> Option<[[f64; 4]; 4]> {
        if rep.kind() != kind::MATRIX4D || rep.is_array() {
            return None;
        }
        if rep.is_inlined() {
            // a diagonal of small whole numbers
            let b = rep.payload().to_le_bytes();
            let mut m = [[0.0; 4]; 4];
            for (k, row) in m.iter_mut().enumerate() {
                if let (Some(v), Some(d)) = (row.get_mut(k), b.get(k)) {
                    *v = f64::from(*d as i8);
                }
            }
            return Some(m);
        }
        let at = rep.offset()?;
        let mut m = [[0.0; 4]; 4];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = f64_at(self.data, at + (r * 4 + c) * 8)?;
            }
        }
        Some(m)
    }

    /// A rotation as a quaternion (i, j, k, real).
    fn quaternion(&self, rep: Rep) -> Option<[f64; 4]> {
        if rep.is_array() || rep.is_inlined() {
            return None;
        }
        let at = rep.offset()?;
        match rep.kind() {
            kind::QUATF => Some([0, 1, 2, 3].map(|k| f32_at(self.data, at + k * 4).map_or(0.0, f64::from))),
            kind::QUATD => Some([0, 1, 2, 3].map(|k| f64_at(self.data, at + k * 8).unwrap_or(0.0))),
            _ => None,
        }
    }
}

/// Builds the path tree: each entry a path's index, its last element (negative: a property) and
/// a jump (-2 no child or sibling after, -1 a child next and no sibling, 0 a sibling next, more:
/// a child next and a sibling that far on).
fn build_paths(
    indices: &[i64],
    elements: &[i64],
    jumps: &[i64],
    paths: &mut [Option<(Option<usize>, usize, bool)>],
) -> Option<()> {
    // (entry, parent path) still to visit
    let mut todo: Vec<(usize, Option<usize>)> = vec![(0, None)];
    let mut steps = 0usize;
    while let Some((mut entry, mut parent)) = todo.pop() {
        loop {
            steps += 1;
            if steps > indices.len() * 2 + 2 {
                return None;
            }
            let index = usize::try_from(*indices.get(entry)?).ok()?;
            let element = *elements.get(entry)?;
            *paths.get_mut(index)? = Some((parent, usize::try_from(element.unsigned_abs()).ok()?, element < 0));
            let jump = *jumps.get(entry)?;
            let child = jump > 0 || jump == -1;
            let sibling = jump >= 0;
            if child {
                if sibling {
                    todo.push((entry.checked_add(usize::try_from(jump).ok()?)?, parent));
                }
                parent = Some(index);
                entry += 1;
            } else if sibling {
                entry += 1;
            } else {
                break;
            }
        }
    }
    Some(())
}

/// A half-precision float.
fn half(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

// ---- the stage -------------------------------------------------------------------------------

/// A 4×4 matrix for column vectors (points transformed as `m · p`), rows first.
type Matrix = [[f64; 4]; 4];

const IDENTITY: Matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = (0..4)
                .map(|k| {
                    a.get(r).and_then(|x| x.get(k)).unwrap_or(&0.0) * b.get(k).and_then(|x| x.get(c)).unwrap_or(&0.0)
                })
                .sum();
        }
    }
    out
}

fn apply(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
    let v = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2]), 1.0];
    let row = |r: usize| m.get(r).map_or(0.0, |x| x.iter().zip(&v).map(|(a, b)| a * b).sum::<f64>());
    let w = row(3);
    let w = if w.abs() > 1e-12 { w } else { 1.0 };
    [(row(0) / w) as f32, (row(1) / w) as f32, (row(2) / w) as f32]
}

fn translate(t: [f64; 3]) -> Matrix {
    [[1.0, 0.0, 0.0, t[0]], [0.0, 1.0, 0.0, t[1]], [0.0, 0.0, 1.0, t[2]], [0.0, 0.0, 0.0, 1.0]]
}

fn scale(s: [f64; 3]) -> Matrix {
    [[s[0], 0.0, 0.0, 0.0], [0.0, s[1], 0.0, 0.0], [0.0, 0.0, s[2], 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// A turn of `degrees` about axis 0 (x), 1 (y) or 2 (z).
fn rotate(axis: usize, degrees: f64) -> Matrix {
    let (s, c) = degrees.to_radians().sin_cos();
    match axis {
        0 => [[1.0, 0.0, 0.0, 0.0], [0.0, c, -s, 0.0], [0.0, s, c, 0.0], [0.0, 0.0, 0.0, 1.0]],
        1 => [[c, 0.0, s, 0.0], [0.0, 1.0, 0.0, 0.0], [-s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]],
        _ => [[c, -s, 0.0, 0.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
    }
}

fn quaternion(q: [f64; 4]) -> Matrix {
    let [x, y, z, w] = q;
    let n = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = if n > 1e-12 { (x / n, y / n, z / n, w / n) } else { (0.0, 0.0, 0.0, 1.0) };
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), 0.0],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), 0.0],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y), 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn transpose(m: &Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = m.get(c).and_then(|x| x.get(r)).copied().unwrap_or(0.0);
        }
    }
    out
}

/// The inverse of an affine matrix (its last row 0 0 0 1), or none.
fn inverse(m: &Matrix) -> Option<Matrix> {
    let a = |r: usize, c: usize| m.get(r).and_then(|x| x.get(c)).copied().unwrap_or(0.0);
    let (m00, m01, m02, m10, m11, m12, m20, m21, m22) =
        (a(0, 0), a(0, 1), a(0, 2), a(1, 0), a(1, 1), a(1, 2), a(2, 0), a(2, 1), a(2, 2));
    let det = m00 * (m11 * m22 - m12 * m21) - m01 * (m10 * m22 - m12 * m20) + m02 * (m10 * m21 - m11 * m20);
    if det.abs() < 1e-15 || !det.is_finite() {
        return None;
    }
    let r = [
        [(m11 * m22 - m12 * m21) / det, (m02 * m21 - m01 * m22) / det, (m01 * m12 - m02 * m11) / det],
        [(m12 * m20 - m10 * m22) / det, (m00 * m22 - m02 * m20) / det, (m02 * m10 - m00 * m12) / det],
        [(m10 * m21 - m11 * m20) / det, (m01 * m20 - m00 * m21) / det, (m00 * m11 - m01 * m10) / det],
    ];
    let t = [a(0, 3), a(1, 3), a(2, 3)];
    let moved = r.map(|row| -(row[0] * t[0] + row[1] * t[1] + row[2] * t[2]));
    Some([
        [r[0][0], r[0][1], r[0][2], moved[0]],
        [r[1][0], r[1][1], r[1][2], moved[1]],
        [r[2][0], r[2][1], r[2][2], moved[2]],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// A value as the stage reads it, whichever kind of file it came from.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Token(String),
    Tokens(Vec<String>),
    Ints(Vec<i64>),
    Vec2s(Vec<[f32; 2]>),
    Vec3s(Vec<[f32; 3]>),
    Number(f64),
    Vector3([f64; 3]),
    Matrix([[f64; 4]; 4]),
    /// i, j, k, real.
    Quaternion([f64; 4]),
    Path(String),
}

impl Value {
    fn token(&self) -> Option<&str> {
        if let Value::Token(t) = self { Some(t) } else { None }
    }
    fn tokens(self) -> Option<Vec<String>> {
        if let Value::Tokens(t) = self { Some(t) } else { None }
    }
    fn ints(self) -> Option<Vec<i64>> {
        if let Value::Ints(v) = self { Some(v) } else { None }
    }
    fn vec2s(self) -> Option<Vec<[f32; 2]>> {
        if let Value::Vec2s(v) = self { Some(v) } else { None }
    }
    fn vec3s(self) -> Option<Vec<[f32; 3]>> {
        if let Value::Vec3s(v) = self { Some(v) } else { None }
    }
    fn number(&self) -> Option<f64> {
        if let Value::Number(n) = self { Some(*n) } else { None }
    }
    fn vector3(&self) -> Option<[f64; 3]> {
        if let Value::Vector3(v) = self { Some(*v) } else { None }
    }
    fn matrix(&self) -> Option<[[f64; 4]; 4]> {
        if let Value::Matrix(m) = self { Some(*m) } else { None }
    }
    fn quaternion(&self) -> Option<[f64; 4]> {
        if let Value::Quaternion(q) = self { Some(*q) } else { None }
    }
    fn path(self) -> Option<String> {
        if let Value::Path(p) = self { Some(p) } else { None }
    }
}

impl Crate<'_> {
    /// A value as the stage takes it (the kinds read here).
    fn value(&self, rep: Rep) -> Option<Value> {
        match (rep.kind(), rep.is_array()) {
            (kind::TOKEN, false) => self.token_value(rep).map(|t| Value::Token(t.to_owned())),
            // an asset's path is kept as a token
            (kind::ASSET, false) if rep.is_inlined() => self.token(rep.payload()).map(|t| Value::Token(t.to_owned())),
            (kind::TOKEN, true) | (kind::TOKEN_VECTOR, _) => self.token_list(rep).map(Value::Tokens),
            (kind::INT | kind::UINT, true) => self.ints(rep).map(Value::Ints),
            (kind::VEC2F | kind::VEC2D | kind::VEC2H, true) => self.vectors::<2>(rep).map(Value::Vec2s),
            (kind::VEC3F | kind::VEC3D | kind::VEC3H, true) => self.vectors::<3>(rep).map(Value::Vec3s),
            (kind::VEC3F | kind::VEC3D | kind::VEC3H, false) => self.vector3(rep).map(Value::Vector3),
            (kind::MATRIX4D, false) => self.matrix(rep).map(Value::Matrix),
            (kind::QUATF | kind::QUATD, false) => self.quaternion(rep).map(Value::Quaternion),
            (kind::PATH_LIST_OP, _) => self.first_path(rep).map(Value::Path),
            (_, false) => self.number(rep).map(Value::Number),
            _ => None,
        }
    }
}

/// Where the stage's fields come from: a crate file's specs (decoded as they are asked for), or
/// a text layer's (decoded as it was read).
enum Fields<'a> {
    Crate(&'a Crate<'a>, HashMap<String, HashMap<&'a str, Rep>>),
    Text(HashMap<String, HashMap<String, Value>>),
}

/// The specs of one layer by path, each with its fields by name.
struct Stage<'a> {
    fields: Fields<'a>,
}

impl<'a> Stage<'a> {
    fn of_crate(file: &'a Crate<'a>) -> Stage<'a> {
        let mut specs = HashMap::new();
        for &(path, set) in &file.specs {
            let Some(name) = file.path(path) else { continue };
            let mut fields = HashMap::new();
            let mut at = set;
            while let Some(&f) = file.field_sets.get(at) {
                if f < 0 || f == i64::from(u32::MAX) {
                    break;
                }
                if let Some(&(token, rep)) = usize::try_from(f).ok().and_then(|f| file.fields.get(f))
                    && let Some(field) = file.tokens.get(token)
                {
                    fields.insert(field.as_str(), rep);
                }
                at += 1;
            }
            specs.insert(name, fields);
        }
        Stage { fields: Fields::Crate(file, specs) }
    }

    fn field(&self, path: &str, name: &str) -> Option<Value> {
        match &self.fields {
            Fields::Crate(file, specs) => specs.get(path)?.get(name).and_then(|&r| file.value(r)),
            Fields::Text(specs) => specs.get(path)?.get(name).cloned(),
        }
    }

    /// Every spec's path.
    fn paths(&self) -> Vec<&str> {
        match &self.fields {
            Fields::Crate(_, specs) => specs.keys().map(String::as_str).collect(),
            Fields::Text(specs) => specs.keys().map(String::as_str).collect(),
        }
    }

    fn token(&self, path: &str, name: &str) -> Option<String> {
        self.field(path, name).and_then(|v| v.token().map(str::to_owned))
    }

    fn type_name(&self, path: &str) -> Option<String> {
        self.token(path, "typeName")
    }

    fn children(&self, path: &str) -> Vec<String> {
        let names = self.field(path, "primChildren").and_then(Value::tokens).unwrap_or_default();
        names.into_iter().map(|n| if path == "/" { format!("/{n}") } else { format!("{path}/{n}") }).collect()
    }

    /// An attribute's value as written (its default, not its samples over time).
    fn value(&self, prim: &str, attribute: &str) -> Option<Value> {
        self.field(&format!("{prim}.{attribute}"), "default")
    }

    /// A prim's own transform, from its transform operations in order.
    fn local(&self, prim: &str) -> Matrix {
        let order = self.value(prim, "xformOpOrder").and_then(Value::tokens).unwrap_or_default();
        let mut m = IDENTITY;
        for op in &order {
            if op == "!resetXformStack!" {
                continue;
            }
            let (invert, name) = match op.strip_prefix("!invert!") {
                Some(rest) => (true, rest),
                None => (false, op.as_str()),
            };
            let Some(v) = self.value(prim, name) else { continue };
            let kind = name.strip_prefix("xformOp:").unwrap_or(name).split(':').next().unwrap_or("");
            let step = match kind {
                "translate" => v.vector3().map(translate),
                "scale" => v.vector3().map(scale),
                "rotateX" | "rotateY" | "rotateZ" => {
                    let axis = match kind {
                        "rotateX" => 0,
                        "rotateY" => 1,
                        _ => 2,
                    };
                    v.number().map(|d| rotate(axis, d))
                }
                "rotateXYZ" | "rotateXZY" | "rotateYXZ" | "rotateYZX" | "rotateZXY" | "rotateZYX" => {
                    v.vector3().map(|angles| {
                        // the first letter's turn happens first, so it is rightmost
                        kind.bytes().skip(6).fold(IDENTITY, |acc, axis| {
                            let a = usize::from(axis.saturating_sub(b'X'));
                            mul(&rotate(a, angles.get(a).copied().unwrap_or(0.0)), &acc)
                        })
                    })
                }
                "orient" => v.quaternion().map(quaternion),
                "transform" => v.matrix().map(|m| transpose(&m)),
                _ => None,
            };
            if let Some(step) = step {
                let step = if invert { inverse(&step).unwrap_or(IDENTITY) } else { step };
                m = mul(&m, &step);
            }
        }
        m
    }

    /// Calls `visit` with every prim under `path` and where it is in the world.
    fn walk(&self, path: &str, parent: Matrix, visit: &mut dyn FnMut(&str, &Matrix)) {
        let mut todo = vec![(path.to_owned(), parent, 0usize)];
        while let Some((prim, parent, depth)) = todo.pop() {
            if depth > 256 {
                continue;
            }
            let world = if prim == "/" { parent } else { mul(&parent, &self.local(&prim)) };
            visit(&prim, &world);
            for child in self.children(&prim).into_iter().rev() {
                todo.push((child, world, depth + 1));
            }
        }
    }

    /// The path of the material a prim's `material:binding` points at.
    fn material(&self, prim: &str) -> Option<String> {
        self.field(&format!("{prim}.material:binding"), "targetPaths").and_then(Value::path)
    }

    /// What an attribute is connected to (a prim's output), if anything.
    fn connection(&self, attribute: &str) -> Option<String> {
        self.field(attribute, "connectionPaths").and_then(Value::path)
    }

    /// The material at `path` as its preview surface describes it: its colour or colour image,
    /// and its roughness and metalness or their images (and the component of each image read).
    fn material_info(&self, path: &str) -> Material {
        let name = path.rsplit('/').next().unwrap_or(path).to_owned();
        let mut m = Material { name, ..Material::default() };
        let Some(surface) = self.connection(&format!("{path}.outputs:surface")) else { return m };
        let shader = prim_of(&surface);
        let input = |n: &str| format!("{shader}.inputs:{n}");
        match self.connection(&input("diffuseColor")).and_then(|t| self.texture_file(&t)) {
            Some((file, _)) => m.color_map = Some(file),
            None => {
                m.color =
                    self.value(shader, "inputs:diffuseColor").and_then(|v| v.vector3()).map(|c| c.map(|x| x as f32));
            }
        }
        match self.connection(&input("roughness")).and_then(|t| self.texture_file(&t)) {
            Some((file, component)) => (m.roughness_map, m.roughness_component) = (Some(file), component),
            None => m.roughness = self.value(shader, "inputs:roughness").and_then(|v| v.number()).map(|v| v as f32),
        }
        match self.connection(&input("metallic")).and_then(|t| self.texture_file(&t)) {
            Some((file, component)) => (m.metallic_map, m.metallic_component) = (Some(file), component),
            None => m.metallic = self.value(shader, "inputs:metallic").and_then(|v| v.number()).map(|v| v as f32),
        }
        m
    }

    /// The image a texture's output (`<texture>.outputs:<rgb, r, g, b or a>`) reads, and which
    /// component of it (none: all three).
    fn texture_file(&self, output: &str) -> Option<(String, Option<usize>)> {
        let file = self.value(prim_of(output), "inputs:file").and_then(|v| v.token().map(str::to_owned))?;
        let component = match output.rsplit(':').next() {
            Some("r") => Some(0),
            Some("g") => Some(1),
            Some("b") => Some(2),
            Some("a") => Some(3),
            _ => None,
        };
        Some((file, component))
    }

    /// The stage's meshes as one model, with the materials they are bound to.
    fn model(&self) -> Result<Mesh, MeshError> {
        let mut out = Gather::default();
        let z_up = self.token("/", "upAxis").as_deref() == Some("Z");
        self.walk("/", IDENTITY, &mut |path, world| self.add_mesh(path, world, z_up, &mut out));
        let materials = out.material_paths.iter().map(|p| self.material_info(p)).collect();
        out.into_gathered().into_mesh(materials, None)
    }

    /// Adds the mesh at `path` (if it is one) to `out`, placed by `world`.
    fn add_mesh(&self, path: &str, world: &Matrix, z_up: bool, out: &mut Gather) {
        if self.type_name(path).as_deref() != Some("Mesh") || out.faces.len() > MAX_TRIANGLES {
            return;
        }
        let Some(points) = self.value(path, "points").and_then(Value::vec3s) else { return };
        let Some(counts) = self.value(path, "faceVertexCounts").and_then(Value::ints) else { return };
        let Some(indices) = self.value(path, "faceVertexIndices").and_then(Value::ints) else { return };
        let uv = self.texture_coordinates(path);
        // each face's material: its subset's, else the mesh's own (by name; its path kept)
        let mut name_of = |p: Option<String>| -> String {
            let Some(p) = p else { return String::new() };
            if !out.material_paths.contains(&p) {
                out.material_paths.push(p.clone());
            }
            p.rsplit('/').next().unwrap_or(&p).to_owned()
        };
        let own = name_of(self.material(path));
        let mut face_material: Vec<Option<String>> = vec![None; counts.len()];
        for child in self.children(path) {
            if self.type_name(&child).as_deref() != Some("GeomSubset") {
                continue;
            }
            let family = self.value(&child, "familyName").and_then(|v| v.token().map(str::to_owned));
            if family.is_some_and(|fam| fam != "materialBind") {
                continue;
            }
            let Some(name) = self.material(&child).map(|p| name_of(Some(p))) else { continue };
            for face in self.value(&child, "indices").and_then(Value::ints).unwrap_or_default() {
                if let Some(slot) = usize::try_from(face).ok().and_then(|i| face_material.get_mut(i)) {
                    *slot = Some(name.clone());
                }
            }
        }
        // its faces are a submesh named as the mesh is
        out.object = path.rsplit('/').next().unwrap_or(path).to_owned();
        let base = out.positions.len();
        for p in points {
            let mut q = apply(world, p);
            if z_up {
                q = [q[0], q[2], -q[1]];
            }
            out.positions.push(q);
        }
        let mut corner = 0usize;
        for (face, &n) in counts.iter().enumerate() {
            let Ok(n) = usize::try_from(n) else { return };
            let mut poly = Vec::with_capacity(n.min(64));
            for k in 0..n {
                let Some(&point) = indices.get(corner + k) else { return };
                let Ok(point) = usize::try_from(point) else { return };
                let t = uv.as_ref().and_then(|uv| uv.at(point, corner + k, face));
                poly.push((base + point, t));
            }
            corner += n;
            let material = face_material.get(face).cloned().flatten().unwrap_or_else(|| own.clone());
            out.face(&poly, &material);
        }
    }

    /// The mesh's texture coordinates: `primvars:st`, else its first two-component primvar
    /// meant as texture coordinates.
    fn texture_coordinates(&self, mesh: &str) -> Option<Coordinates> {
        let prefix = format!("{mesh}.primvars:");
        let mut names: Vec<&str> = self
            .paths()
            .into_iter()
            .filter(|k| {
                k.strip_prefix(&prefix).is_some_and(|rest| !rest.contains(':'))
                    && self.type_name(k).is_some_and(|t| t.starts_with("texCoord2") || t.starts_with("float2"))
            })
            .collect();
        names.sort_by_key(|k| (!k.ends_with(":st"), *k));
        let name = names.first()?;
        let values = self.field(name, "default").and_then(Value::vec2s)?;
        let indices = self.field(&format!("{name}:indices"), "default").and_then(Value::ints);
        let interpolation = self.token(name, "interpolation").unwrap_or_else(|| "vertex".to_owned());
        Some(Coordinates { values, indices, interpolation })
    }
}

// ---- the text form ---------------------------------------------------------------------------

/// A piece of a text layer.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Punct(char),
    Str(String),
    Num(f64),
    Word(String),
    Path(String),
    Asset(String),
}

/// Splits a text layer into its pieces; none if it can't be.
fn tokenize(text: &str) -> Option<Vec<Tok>> {
    let chars: Vec<char> = text.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(c) = at(i) {
        match c {
            c if c.is_whitespace() => i += 1,
            '#' => {
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
            }
            '(' | ')' | '[' | ']' | '{' | '}' | '=' | ',' | ':' | ';' => {
                out.push(Tok::Punct(c));
                i += 1;
            }
            '"' | '\'' => {
                let triple = at(i + 1) == Some(c) && at(i + 2) == Some(c);
                i += if triple { 3 } else { 1 };
                let mut s = String::new();
                loop {
                    let d = at(i)?;
                    if d == '\\' {
                        let e = at(i + 1)?;
                        s.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                        i += 2;
                        continue;
                    }
                    if d == c && (!triple || (at(i + 1) == Some(c) && at(i + 2) == Some(c))) {
                        i += if triple { 3 } else { 1 };
                        break;
                    }
                    s.push(d);
                    i += 1;
                }
                out.push(Tok::Str(s));
            }
            '<' => {
                let mut s = String::new();
                i += 1;
                while let Some(d) = at(i) {
                    i += 1;
                    if d == '>' {
                        break;
                    }
                    s.push(d);
                }
                out.push(Tok::Path(s));
            }
            '@' => {
                let triple = at(i + 1) == Some('@') && at(i + 2) == Some('@');
                i += if triple { 3 } else { 1 };
                let mut s = String::new();
                loop {
                    let d = at(i)?;
                    if d == '@' && (!triple || (at(i + 1) == Some('@') && at(i + 2) == Some('@'))) {
                        i += if triple { 3 } else { 1 };
                        break;
                    }
                    s.push(d);
                    i += 1;
                }
                out.push(Tok::Asset(s));
            }
            c if c.is_ascii_digit()
                || ((c == '-' || c == '+' || c == '.')
                    && at(i + 1).is_some_and(|d| d.is_ascii_digit() || d == '.')) =>
            {
                let start = i;
                i += 1;
                while let Some(d) = at(i) {
                    let exponent_sign = (d == '-' || d == '+') && matches!(at(i - 1), Some('e' | 'E'));
                    if d.is_ascii_digit() || d == '.' || d == 'e' || d == 'E' || exponent_sign {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let s: String = chars.get(start..i)?.iter().collect();
                out.push(Tok::Num(s.parse().ok()?));
            }
            c if c.is_alphabetic() || c == '_' || c == '!' || c == '-' => {
                let start = i;
                while let Some(d) = at(i) {
                    // names hold namespaces (primvars:st) and property paths (outputs:surface.connect)
                    let inside = d.is_alphanumeric()
                        || matches!(d, '_' | '!' | '.' | '-')
                        || (d == ':' && at(i + 1).is_some_and(|e| e.is_alphanumeric() || e == '_'));
                    if !inside {
                        break;
                    }
                    i += 1;
                }
                let word: String = chars.get(start..i)?.iter().collect();
                match word.as_str() {
                    "inf" => out.push(Tok::Num(f64::INFINITY)),
                    "-inf" => out.push(Tok::Num(f64::NEG_INFINITY)),
                    "nan" => out.push(Tok::Num(f64::NAN)),
                    _ => out.push(Tok::Word(word)),
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

/// A value as written, before its type says what it is.
#[derive(Clone, Debug, PartialEq)]
enum Raw {
    Num(f64),
    Text(String),
    Path(String),
    List(Vec<Raw>),
    /// A dictionary or time samples, not read.
    Skipped,
}

/// Reads a text layer's specs.
struct TextReader {
    toks: Vec<Tok>,
    at: usize,
    specs: HashMap<String, HashMap<String, Value>>,
}

impl TextReader {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn is(&self, c: char) -> bool {
        self.peek() == Some(&Tok::Punct(c))
    }

    fn expect(&mut self, c: char) -> Option<()> {
        (self.next()? == Tok::Punct(c)).then_some(())
    }

    fn word(&mut self) -> Option<String> {
        match self.next()? {
            Tok::Word(w) => Some(w),
            _ => None,
        }
    }

    /// Skips a balanced run from an opening `(`, `[` or `{` to its close.
    fn skip_group(&mut self) -> Option<()> {
        let mut depth = 0usize;
        loop {
            match self.next()? {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(());
                    }
                }
                _ => {}
            }
        }
    }

    fn raw(&mut self, depth: usize) -> Option<Raw> {
        if depth > 64 {
            return None;
        }
        match self.peek()? {
            Tok::Punct('[' | '(') => {
                let close = if self.is('[') { ']' } else { ')' };
                self.next();
                let mut items = Vec::new();
                while !self.is(close) {
                    items.push(self.raw(depth + 1)?);
                    if self.is(',') {
                        self.next();
                    }
                }
                self.next();
                Some(Raw::List(items))
            }
            Tok::Punct('{') => {
                self.skip_group()?;
                Some(Raw::Skipped)
            }
            _ => match self.next()? {
                Tok::Num(n) => Some(Raw::Num(n)),
                Tok::Str(s) | Tok::Word(s) | Tok::Asset(s) => Some(Raw::Text(s)),
                Tok::Path(p) => Some(Raw::Path(p)),
                Tok::Punct(_) => None,
            },
        }
    }

    /// `( key = value ... )` metadata: the entries read, others skipped.
    fn metadata(&mut self) -> Option<HashMap<String, Raw>> {
        let mut out = HashMap::new();
        self.expect('(')?;
        while !self.is(')') {
            match self.next()? {
                // a doc string
                Tok::Str(_) => {}
                Tok::Word(mut key) => {
                    if matches!(key.as_str(), "prepend" | "append" | "add" | "delete" | "reorder") {
                        key = self.word()?;
                    }
                    self.expect('=')?;
                    let v = self.raw(0)?;
                    out.insert(key, v);
                }
                _ => return None,
            }
            if self.is(';') {
                self.next();
            }
        }
        self.next();
        Some(out)
    }

    fn put(&mut self, path: &str, field: &str, value: Value) {
        self.specs.entry(path.to_owned()).or_default().insert(field.to_owned(), value);
    }

    fn layer(&mut self) -> Option<()> {
        if self.is('(') {
            let meta = self.metadata()?;
            if let Some(Raw::Text(up)) = meta.get("upAxis") {
                self.put("/", "upAxis", Value::Token(up.clone()));
            }
        }
        let mut children = Vec::new();
        while self.peek().is_some() {
            if let Some(name) = self.prim("/", 0)? {
                children.push(name);
            }
        }
        self.put("/", "primChildren", Value::Tokens(children));
        Some(())
    }

    /// A prim under `parent`: its name, or none for something else read past.
    fn prim(&mut self, parent: &str, depth: usize) -> Option<Option<String>> {
        if depth > 128 {
            return None;
        }
        let specifier = self.word()?;
        if !matches!(specifier.as_str(), "def" | "over" | "class") {
            return None;
        }
        let type_name = match self.peek()? {
            Tok::Word(_) => self.word(),
            _ => None,
        };
        let Tok::Str(name) = self.next()? else { return None };
        let path = if parent == "/" { format!("/{name}") } else { format!("{parent}/{name}") };
        if self.is('(') {
            self.metadata()?;
        }
        self.expect('{')?;
        let mut children = Vec::new();
        while !self.is('}') {
            match self.peek()? {
                Tok::Word(w) if matches!(w.as_str(), "def" | "over" | "class") => {
                    if let Some(child) = self.prim(&path, depth + 1)? {
                        children.push(child);
                    }
                }
                Tok::Word(w) if w == "variantSet" => {
                    // variants are not composed: skipped
                    self.next();
                    self.next();
                    self.expect('=')?;
                    self.skip_group()?;
                }
                Tok::Word(w) if w == "reorder" => {
                    self.next();
                    self.word()?;
                    self.expect('=')?;
                    self.raw(0)?;
                }
                _ => self.property(&path)?,
            }
        }
        self.next();
        if let Some(t) = type_name {
            self.put(&path, "typeName", Value::Token(t));
        }
        self.put(&path, "primChildren", Value::Tokens(children));
        Some(Some(name))
    }

    /// An attribute or relationship of the prim at `prim`.
    fn property(&mut self, prim: &str) -> Option<()> {
        let mut word = self.word()?;
        while matches!(
            word.as_str(),
            "custom" | "uniform" | "varying" | "config" | "prepend" | "append" | "add" | "delete"
        ) {
            word = self.word()?;
        }
        if word == "rel" {
            let name = self.word()?;
            let mut target = None;
            if self.is('=') {
                self.next();
                target = match self.raw(0)? {
                    Raw::Path(p) => Some(p),
                    Raw::List(items) => {
                        items.into_iter().find_map(|r| if let Raw::Path(p) = r { Some(p) } else { None })
                    }
                    _ => None,
                };
            }
            if self.is('(') {
                self.metadata()?;
            }
            if let Some(t) = target {
                let full = if t.starts_with('/') { t } else { format!("{prim}/{t}") };
                self.put(&format!("{prim}.{name}"), "targetPaths", Value::Path(full));
            }
            return Some(());
        }
        let mut type_name = word;
        if self.is('[') {
            self.next();
            self.expect(']')?;
            type_name.push_str("[]");
        }
        let name = self.word()?;
        let mut value = None;
        if self.is('=') {
            self.next();
            value = Some(self.raw(0)?);
        }
        let meta = if self.is('(') { self.metadata()? } else { HashMap::new() };
        // a connection is kept as what it connects to; samples over time are not read
        if let Some(base) = name.strip_suffix(".connect") {
            let target = match value {
                Some(Raw::Path(p)) => Some(p),
                Some(Raw::List(items)) => {
                    items.into_iter().find_map(|r| if let Raw::Path(p) = r { Some(p) } else { None })
                }
                _ => None,
            };
            if let Some(t) = target {
                let full = if t.starts_with('/') { t } else { format!("{prim}/{t}") };
                self.put(&format!("{prim}.{base}"), "connectionPaths", Value::Path(full));
            }
            return Some(());
        }
        if name.ends_with(".timeSamples") {
            return Some(());
        }
        let path = format!("{prim}.{name}");
        self.put(&path, "typeName", Value::Token(type_name.clone()));
        if let Some(Raw::Text(i)) = meta.get("interpolation") {
            self.put(&path, "interpolation", Value::Token(i.clone()));
        }
        if let Some(v) = value.and_then(|v| typed(&type_name, v)) {
            self.put(&path, "default", v);
        }
        Some(())
    }
}

/// The prim a property's path names (before its `.`).
fn prim_of(property: &str) -> &str {
    property.split('.').next().unwrap_or(property)
}

/// A written value as its declared type makes it (the types read here).
fn typed(type_name: &str, raw: Raw) -> Option<Value> {
    let numbers = |r: &Raw| -> Option<Vec<f64>> {
        match r {
            Raw::List(items) => items.iter().map(|i| if let Raw::Num(n) = i { Some(*n) } else { None }).collect(),
            _ => None,
        }
    };
    let base = type_name.trim_end_matches("[]");
    let array = type_name.ends_with("[]");
    let width = match base.chars().last() {
        Some('2') | Some('f') | Some('d') | Some('h') if base.contains('2') => 2,
        _ if base.contains('3') => 3,
        _ => 1,
    };
    match (base, array) {
        ("token" | "string" | "asset", false) => match raw {
            Raw::Text(t) => Some(Value::Token(t)),
            _ => None,
        },
        ("token" | "string", true) => match raw {
            Raw::List(items) => items
                .into_iter()
                .map(|i| if let Raw::Text(t) = i { Some(t) } else { None })
                .collect::<Option<Vec<_>>>()
                .map(Value::Tokens),
            _ => None,
        },
        ("int" | "uint" | "int64" | "uint64", true) => {
            numbers(&raw).map(|v| Value::Ints(v.into_iter().map(|n| n as i64).collect()))
        }
        ("matrix4d", false) => {
            let Raw::List(rows) = raw else { return None };
            let mut m = [[0.0; 4]; 4];
            for (row, r) in m.iter_mut().zip(&rows) {
                let values = numbers(r)?;
                for (v, n) in row.iter_mut().zip(values) {
                    *v = n;
                }
            }
            Some(Value::Matrix(m))
        }
        // written real first
        ("quatf" | "quatd" | "quath", false) => {
            let v = numbers(&raw)?;
            Some(Value::Quaternion([*v.get(1)?, *v.get(2)?, *v.get(3)?, *v.first()?]))
        }
        (_, true) if width == 2 => match raw {
            Raw::List(items) => items
                .iter()
                .map(|i| {
                    let v = numbers(i)?;
                    Some([*v.first()? as f32, *v.get(1)? as f32])
                })
                .collect::<Option<Vec<_>>>()
                .map(Value::Vec2s),
            _ => None,
        },
        (_, true) if width == 3 => match raw {
            Raw::List(items) => items
                .iter()
                .map(|i| {
                    let v = numbers(i)?;
                    Some([*v.first()? as f32, *v.get(1)? as f32, *v.get(2)? as f32])
                })
                .collect::<Option<Vec<_>>>()
                .map(Value::Vec3s),
            _ => None,
        },
        (_, false) if width == 3 => {
            let v = numbers(&raw)?;
            Some(Value::Vector3([*v.first()?, *v.get(1)?, *v.get(2)?]))
        }
        (_, false) => match raw {
            Raw::Num(n) => Some(Value::Number(n)),
            _ => None,
        },
        _ => None,
    }
}

/// Reads a model from USD's text form (`.usda`).
pub fn read_usda(text: &str) -> Result<Mesh, MeshError> {
    let toks = tokenize(text).ok_or(MeshError::Unreadable)?;
    let mut reader = TextReader { toks, at: 0, specs: HashMap::new() };
    reader.layer().ok_or(MeshError::Unreadable)?;
    Stage { fields: Fields::Text(reader.specs) }.model()
}

/// A mesh's texture coordinates, how they are spread over it, and their indices if any.
struct Coordinates {
    values: Vec<[f32; 2]>,
    indices: Option<Vec<i64>>,
    interpolation: String,
}

impl Coordinates {
    /// The coordinate for a corner: its point's, its own (face-varying) or its face's.
    fn at(&self, point: usize, corner: usize, face: usize) -> Option<[f32; 2]> {
        let element = match self.interpolation.as_str() {
            "faceVarying" => corner,
            "uniform" => face,
            "constant" => 0,
            _ => point,
        };
        let index = match &self.indices {
            Some(ix) => usize::try_from(*ix.get(element)?).ok()?,
            None => element,
        };
        self.values.get(index).copied()
    }
}

/// Faces gathered from the stage's meshes.
#[derive(Default)]
struct Gather {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    uv_index: HashMap<[u32; 2], usize>,
    corners: Vec<[usize; 3]>,
    known: HashMap<[usize; 2], usize>,
    faces: Vec<[usize; 3]>,
    parts: Vec<Part>,
    /// Each run's mesh, and the mesh whose faces come now.
    objects: Vec<String>,
    object: String,
    /// The materials faces are bound to, by path.
    material_paths: Vec<String>,
}

impl Gather {
    /// A polygon of (position, texture coordinate) corners, fanned into triangles.
    fn face(&mut self, poly: &[(usize, Option<[f32; 2]>)], material: &str) {
        if poly.len() < 3 {
            return;
        }
        let corners: Vec<usize> = poly
            .iter()
            .map(|&(p, uv)| {
                let t = uv.map_or(usize::MAX, |uv| {
                    let key = [uv[0].to_bits(), uv[1].to_bits()];
                    *self.uv_index.entry(key).or_insert_with(|| {
                        self.uvs.push(uv);
                        self.uvs.len() - 1
                    })
                });
                *self.known.entry([p, t]).or_insert_with(|| {
                    self.corners.push([p, t, usize::MAX]);
                    self.corners.len() - 1
                })
            })
            .collect();
        if self.parts.last().is_none_or(|p| p.material != material) || self.objects.last() != Some(&self.object) {
            self.parts.push(Part { material: material.to_owned(), first: self.faces.len(), count: 0 });
            self.objects.push(self.object.clone());
        }
        if let Some(&a) = corners.first() {
            for pair in corners.get(1..).unwrap_or(&[]).windows(2) {
                if let [b, c] = pair {
                    self.faces.push([a, *b, *c]);
                }
            }
        }
    }

    fn into_gathered(self) -> Gathered {
        Gathered {
            positions: self.positions,
            uvs: self.uvs,
            normals: Vec::new(),
            corners: self.corners,
            faces: self.faces,
            parts: self.parts,
            objects: self.objects,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz4_blocks_undo() {
        // "abcabcabcabc": three literals, then a match of nine from three back
        let block = [0x35, b'a', b'b', b'c', 3, 0];
        assert_eq!(raw_block(&block, 64).unwrap(), b"abcabcabcabc");
        assert!(raw_block(&block, 5).is_none());
        // a match from before the start
        assert!(raw_block(&[0x10, b'a', 9, 0], 64).is_none());
    }

    #[test]
    fn integers_decode_as_running_sums() {
        // common delta 1; codes: common, int8 (5), common, int16 (-300)
        let mut buf = 1i32.to_le_bytes().to_vec();
        buf.push(0b1000_0100);
        buf.push(5);
        buf.extend((-300i16).to_le_bytes());
        assert_eq!(decode_ints(&buf, 4, false).unwrap(), [1, 6, 7, -293]);
    }

    #[test]
    fn rotations_compose_first_letter_first() {
        let p = [1.0, 0.0, 0.0];
        // a quarter turn about z takes x to y; then about x, y to z
        let zx = mul(&rotate(0, 90.0), &rotate(2, 90.0));
        let q = apply(&zx, p);
        assert!((q[0]).abs() < 1e-5 && (q[1]).abs() < 1e-5 && (q[2] - 1.0).abs() < 1e-5, "{q:?}");
        let m = mul(&translate([1.0, 2.0, 3.0]), &scale([2.0, 2.0, 2.0]));
        let back = inverse(&m).unwrap();
        let r = apply(&back, apply(&m, [0.5, -1.0, 4.0]));
        assert!((r[0] - 0.5).abs() < 1e-5 && (r[1] + 1.0).abs() < 1e-5 && (r[2] - 4.0).abs() < 1e-5);
    }

    #[test]
    fn halves_read() {
        assert_eq!(half(0x3c00), 1.0);
        assert_eq!(half(0xc000), -2.0);
        assert_eq!(half(0x3800), 0.5);
    }

    #[test]
    fn a_written_package_holds_its_layer_and_images_aligned() {
        let mesh = Mesh {
            vertices: [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
                .iter()
                .map(|&[x, y]| crate::mesh::Vertex { position: [x, y, 0.0], normal: [0.0, 0.0, 1.0], uv: [x, y] })
                .collect(),
            triangles: vec![[0, 1, 2]],
            parts: vec![Part { material: "1 coat".into(), first: 0, count: 1 }],
            ..Mesh::default()
        };
        let images =
            [("coat.png".into(), vec![7; 5]), ("rough.png".into(), vec![8; 5]), ("metal.png".into(), vec![9; 5])];
        let bytes = write_usdz(&mesh, &[images]);
        let zip = ZipReader::new(&bytes).unwrap();
        let names = ["model.usda", "textures/coat.png", "textures/rough.png", "textures/metal.png"];
        assert_eq!(zip.names().collect::<Vec<_>>(), names);
        assert_eq!(zip.read("textures/coat.png").unwrap(), [7; 5]);
        assert_eq!(zip.read("textures/metal.png").unwrap(), [9; 5]);
        // each file's bytes start on a 64-byte boundary
        for name in names {
            let data = zip.read(name).unwrap();
            let at = bytes.windows(data.len()).position(|w| w == data.as_slice()).unwrap();
            assert_eq!(at % 64, 0, "{name}");
        }
        let layer = String::from_utf8(zip.read("model.usda").unwrap()).unwrap();
        // a material named as USD allows
        assert!(layer.contains("def GeomSubset \"_1_coat\""));
        assert!(layer.contains("rel material:binding = </Model/Materials/_1_coat>"));
        assert!(layer.contains("asset inputs:file = @textures/coat.png@"));
        assert!(layer.contains("float inputs:roughness.connect = </Model/Materials/_1_coat/Roughness.outputs:r>"));
        assert!(layer.contains("asset inputs:file = @textures/metal.png@"));
        assert!(layer.contains("int[] faceVertexIndices = [0, 1, 2]"));
    }

    #[test]
    fn other_files_are_refused() {
        assert_eq!(read_usdc(b"#usda 1.0\n"), Err(MeshError::Unreadable));
        assert_eq!(read_usdz(b"not a zip"), Err(MeshError::Unreadable));
    }
}
