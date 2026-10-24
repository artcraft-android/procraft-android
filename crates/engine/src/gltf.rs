//! Models as glTF 2.0 binary files (`.glb`), for viewers that show a model in the room: one node
//! holding the model's mesh, a primitive per part, each with a metallic-roughness material
//! reading the part's images (PNGs packed in the file).

use serde_json::{Value, json};

use crate::mesh::Mesh;

/// A part's images for [`write_glb`], each a PNG: its colour, its roughness and metalness (in
/// green and blue, as glTF reads them), and its normal and occlusion (red) maps if it has them.
pub struct GlbPart<'a> {
    pub name: &'a str,
    pub color: &'a [u8],
    pub metallic_roughness: &'a [u8],
    pub normal: Option<&'a [u8]>,
    pub occlusion: Option<&'a [u8]>,
}

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

/// The file's binary chunk as it is built: its bytes and the views into them.
#[derive(Default)]
struct Bin {
    bytes: Vec<u8>,
    views: Vec<Value>,
}

impl Bin {
    /// `data` as a new view (on a 4-byte boundary), with what it holds for the GPU if anything;
    /// returns the view's index.
    fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        let mut view = json!({ "buffer": 0, "byteOffset": self.bytes.len(), "byteLength": data.len() });
        if let (Some(t), Some(v)) = (target, view.as_object_mut()) {
            v.insert("target".into(), json!(t));
        }
        self.bytes.extend_from_slice(data);
        self.views.push(view);
        self.views.len() - 1
    }
}

/// The model as a GLB: its mesh (texture coordinates turned to glTF's, which run down the image)
/// and per part a primitive with its material and images (`parts`, by part).
pub fn write_glb(mesh: &Mesh, parts: &[GlbPart<'_>]) -> Vec<u8> {
    let mut bin = Bin::default();
    let mut accessors: Vec<Value> = Vec::new();
    let floats = |values: &mut dyn Iterator<Item = f32>| values.flat_map(f32::to_le_bytes).collect::<Vec<u8>>();
    let count = mesh.vertices.len();
    // the corners' places (with their bounds, as glTF asks), normals and texture coordinates
    let (lo, hi) = mesh.bounds().unwrap_or(([0.0; 3], [0.0; 3]));
    let positions = bin.view(&floats(&mut mesh.vertices.iter().flat_map(|v| v.position)), Some(ARRAY_BUFFER));
    accessors.push(json!({
        "bufferView": positions, "componentType": FLOAT, "count": count, "type": "VEC3", "min": lo, "max": hi
    }));
    let normals = bin.view(&floats(&mut mesh.vertices.iter().flat_map(|v| v.normal)), Some(ARRAY_BUFFER));
    accessors.push(json!({ "bufferView": normals, "componentType": FLOAT, "count": count, "type": "VEC3" }));
    let uvs = bin.view(&floats(&mut mesh.vertices.iter().flat_map(|v| [v.uv[0], 1.0 - v.uv[1]])), Some(ARRAY_BUFFER));
    accessors.push(json!({ "bufferView": uvs, "componentType": FLOAT, "count": count, "type": "VEC2" }));

    let mut images: Vec<Value> = Vec::new();
    let mut textures: Vec<Value> = Vec::new();
    let mut texture = |bin: &mut Bin, png: &[u8]| {
        let view = bin.view(png, None);
        images.push(json!({ "bufferView": view, "mimeType": "image/png" }));
        textures.push(json!({ "sampler": 0, "source": images.len() - 1 }));
        json!({ "index": textures.len() - 1 })
    };
    let mut materials: Vec<Value> = Vec::new();
    let mut primitives: Vec<Value> = Vec::new();
    for (part, images_of) in mesh.parts.iter().zip(parts) {
        let triangles = mesh.triangles.get(part.first..part.first + part.count).unwrap_or(&[]);
        if triangles.is_empty() {
            continue;
        }
        let indices: Vec<u8> = triangles.iter().flatten().flat_map(|i| i.to_le_bytes()).collect();
        let view = bin.view(&indices, Some(ELEMENT_ARRAY_BUFFER));
        accessors.push(json!({
            "bufferView": view, "componentType": UNSIGNED_INT, "count": triangles.len() * 3, "type": "SCALAR"
        }));
        let mut material = json!({
            "name": images_of.name,
            "pbrMetallicRoughness": {
                "baseColorTexture": texture(&mut bin, images_of.color),
                "metallicRoughnessTexture": texture(&mut bin, images_of.metallic_roughness),
                "metallicFactor": 1.0,
                "roughnessFactor": 1.0,
            },
            "doubleSided": true,
        });
        if let Some(m) = material.as_object_mut() {
            if let Some(png) = images_of.normal {
                m.insert("normalTexture".into(), texture(&mut bin, png));
            }
            if let Some(png) = images_of.occlusion {
                m.insert("occlusionTexture".into(), texture(&mut bin, png));
            }
        }
        materials.push(material);
        primitives.push(json!({
            "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 },
            "indices": accessors.len() - 1,
            "material": materials.len() - 1,
        }));
    }
    while !bin.bytes.len().is_multiple_of(4) {
        bin.bytes.push(0);
    }
    let document = json!({
        "asset": { "version": "2.0", "generator": "ProCraft" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0, "name": "Model" }],
        "meshes": [{ "primitives": primitives }],
        "materials": materials,
        "textures": textures,
        "images": images,
        "samplers": [{ "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497 }],
        "accessors": accessors,
        "bufferViews": bin.views,
        "buffers": [{ "byteLength": bin.bytes.len() }],
    });
    let mut text = document.to_string().into_bytes();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    // the header, the JSON chunk, the binary chunk
    let total = 12 + 8 + text.len() + 8 + bin.bytes.len();
    let word = |n: usize| u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&word(total));
    out.extend_from_slice(&word(text.len()));
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&word(bin.bytes.len()));
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin.bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::read_obj;

    const TWO: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n\
                       usemtl front\nf 1/1 2/2 3/3\nusemtl back\nf 1/1 3/3 4/4\n";

    /// The file's JSON and binary chunks.
    fn chunks(glb: &[u8]) -> (Value, Vec<u8>) {
        let word = |at: usize| u32::from_le_bytes(glb[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(word(4), 2);
        assert_eq!(word(8), glb.len());
        let json_len = word(12);
        assert_eq!(&glb[16..20], b"JSON");
        assert_eq!(json_len % 4, 0);
        let doc: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let at = 20 + json_len;
        let bin_len = word(at);
        assert_eq!(&glb[at + 4..at + 8], b"BIN\0");
        assert_eq!(at + 8 + bin_len, glb.len());
        (doc, glb[at + 8..].to_vec())
    }

    #[test]
    fn a_model_writes_as_a_glb_with_a_primitive_and_material_per_part() {
        let mesh = read_obj(TWO, None).unwrap();
        let (color, mr, normal) = (vec![1u8; 5], vec![2u8; 6], vec![3u8; 7]);
        let parts = [
            GlbPart { name: "front", color: &color, metallic_roughness: &mr, normal: Some(&normal), occlusion: None },
            GlbPart { name: "back", color: &color, metallic_roughness: &mr, normal: None, occlusion: None },
        ];
        let (doc, bin) = chunks(&write_glb(&mesh, &parts));
        assert_eq!(doc["asset"]["version"], "2.0");
        assert_eq!(doc["buffers"][0]["byteLength"].as_u64().unwrap() as usize, bin.len());
        // every view inside the binary chunk, on a 4-byte boundary
        for v in doc["bufferViews"].as_array().unwrap() {
            let (o, l) = (v["byteOffset"].as_u64().unwrap() as usize, v["byteLength"].as_u64().unwrap() as usize);
            assert_eq!(o % 4, 0);
            assert!(o + l <= bin.len());
        }
        let prims = doc["meshes"][0]["primitives"].as_array().unwrap();
        assert_eq!(prims.len(), 2);
        let accessor = |i: &Value| &doc["accessors"][i.as_u64().unwrap() as usize];
        assert_eq!(accessor(&prims[0]["indices"])["count"], 3);
        let position = accessor(&prims[0]["attributes"]["POSITION"]);
        assert_eq!(position["count"].as_u64().unwrap() as usize, mesh.vertices.len());
        assert_eq!(position["max"], json!([1.0, 1.0, 0.0]));
        // the materials and their images: the front has a normal map, the back none
        let front = &doc["materials"][0];
        assert_eq!(front["name"], "front");
        assert!(front["normalTexture"]["index"].is_u64());
        assert!(doc["materials"][1].get("normalTexture").is_none());
        assert_eq!(doc["images"].as_array().unwrap().len(), 5);
        let image = |t: &Value| {
            let source = doc["textures"][t["index"].as_u64().unwrap() as usize]["source"].as_u64().unwrap() as usize;
            let view = &doc["bufferViews"][doc["images"][source]["bufferView"].as_u64().unwrap() as usize];
            let o = view["byteOffset"].as_u64().unwrap() as usize;
            bin[o..o + view["byteLength"].as_u64().unwrap() as usize].to_vec()
        };
        assert_eq!(image(&front["normalTexture"]), normal);
        assert_eq!(image(&front["pbrMetallicRoughness"]["metallicRoughnessTexture"]), mr);
        // texture coordinates running down the image: the first corner's v 0 is 1
        let uv = accessor(&prims[0]["attributes"]["TEXCOORD_0"]);
        let view = &doc["bufferViews"][uv["bufferView"].as_u64().unwrap() as usize];
        let o = view["byteOffset"].as_u64().unwrap() as usize;
        let v0 = f32::from_le_bytes(bin[o + 4..o + 8].try_into().unwrap());
        let first = mesh.vertices[0].uv[1];
        assert!((v0 - (1.0 - first)).abs() < 1e-6);
    }
}
