//! 3D painting in the session: a model open in place of a flat canvas. Each texture set (the part
//! of the model one material covers) is a document of its own; the current one is the session's
//! own document and canvas, so layers, undo and adjustments work on it as on any canvas, and the
//! others wait here. Strokes are drawn on the screen and carried onto the model every frame.
//!
//! Saved, a model's artwork folder also holds its file as opened (`model.obj` with `model.mtl`,
//! `model.usdz` or `model.usdc`), and the
//! waiting texture sets' layers in `sets/<part>/<id>.png`; `artwork.json` holds their documents.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use procraft_engine::document::{Document, LayerId, LayerKind};
use procraft_engine::geom::{IRect, Vec2};
use procraft_engine::input::Sample;
use procraft_engine::mesh::{MAX_TRIANGLES, Mesh, MeshError, read_obj};
use procraft_engine::stroke::StrokeBuilder;
use procraft_engine::usd::{read_usd, read_usdc, read_usdz};
use procraft_engine::view::View;
use procraft_gpu::{Canvas, Lighting, MeshView, Orbit, StrokeTool, wgpu};

use crate::project::{THUMBNAIL_SIDE, file_error, pixels_hash, read_png, write_atomically, write_png};
use crate::{CanvasSession, MobileError, PenSample, PointerKind, State, ToolKind};

/// How far a one-finger drag turns the model: this many radians across the window's shorter
/// side (a small drag's share of a trackball's turn).
const TURN_PER_SIDE: f32 = 2.93;
/// The nearest and farthest the camera comes, in the framing's units (the model's largest side
/// counted as 4).
const NEAREST: f32 = 0.05;
const FARTHEST: f32 = 30.0;
/// The texture sets' side when none is asked for, and the most there can be.
const DEFAULT_TEXTURE_SIZE: u32 = 2048;
const MOST_TEXTURE_SIZE: u32 = 4096;

/// Layers' pixels (premultiplied RGBA8), by layer.
pub(crate) type Pixels = Vec<(LayerId, Vec<u8>)>;

/// A texture set waiting while another is painted on: its document, its canvas once the model
/// has been drawn, and until then the pixels it was opened with.
pub(crate) struct TextureSet {
    doc: Document,
    canvas: Option<Canvas>,
    pending: Pixels,
}

impl TextureSet {
    fn empty(side: u32) -> TextureSet {
        TextureSet { doc: empty_texture(side), canvas: None, pending: Vec::new() }
    }
}

/// A new texture set's document: `side` square, nothing painted (the part's base colour shows).
fn empty_texture(side: u32) -> Document {
    let mut doc = Document::new(side, side);
    doc.background_visible = false;
    doc
}

/// The model's files as they were opened, kept to be saved with the artwork: the model, its
/// kind (its file name's extension) and an OBJ's material file.
pub(crate) struct ModelSource {
    bytes: Vec<u8>,
    extension: String,
    mtl: Option<String>,
}

/// The kinds of model file that open: OBJ (with its MTL), USDZ, and USD binary or text.
fn read_model(bytes: &[u8], extension: &str, mtl: Option<&str>) -> Result<Mesh, MeshError> {
    match extension {
        "obj" => read_obj(&String::from_utf8_lossy(bytes), mtl),
        "usdz" => read_usdz(bytes),
        "usdc" => read_usdc(bytes),
        "usda" | "usd" => read_usd(bytes),
        _ => Err(MeshError::Unreadable),
    }
}

/// A model file's kind from its name, lower case.
fn extension_of(name: &str) -> String {
    name.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase()
}

pub(crate) struct Scene {
    mesh: Mesh,
    source: Arc<ModelSource>,
    view: Option<MeshView>,
    /// How the model was last seen, until it is drawn again.
    orbit: Option<Orbit>,
    /// The texture sets by part; the current one's place is empty (it is the session's own).
    sets: Vec<Option<TextureSet>>,
    current: usize,
    /// Each texture set's side (pixels).
    texture_size: u32,
    /// The stroke as drawn on the screen: a canvas the window's size, and what it covers.
    screen: Option<(Document, Canvas)>,
    bounds: IRect,
    /// Each set's picture as last shown on the model: its canvas's composite count, and whether
    /// it was the session's canvas then (its levels are made again when either changes).
    filtered: Vec<Option<(u64, bool)>>,
    /// Strokes reach every surface under them, not only the nearest.
    paint_through: bool,
    /// An environment to light the model by, waiting for the next frame to be set up.
    environment: Option<procraft_engine::environment::Equirect>,
    /// The environment's name, kept with the model (none: the app's default).
    environment_name: Option<String>,
    /// A stroke is being carried onto the model each frame; `ending`: once more, then it ends.
    pub(crate) stroking: bool,
    ending: bool,
}

impl Scene {
    /// `mesh` with a texture set `texture_size` square for each of its parts, set `current` the
    /// session's own; the others are `waiting` where given, else new.
    fn new(
        mesh: Mesh,
        source: ModelSource,
        texture_size: u32,
        current: usize,
        mut waiting: Vec<Option<TextureSet>>,
    ) -> Scene {
        let current = current.min(mesh.parts.len().saturating_sub(1));
        let sets = (0..mesh.parts.len())
            .map(|i| {
                (i != current).then(|| {
                    waiting.get_mut(i).and_then(Option::take).unwrap_or_else(|| TextureSet::empty(texture_size))
                })
            })
            .collect();
        Scene {
            mesh,
            source: Arc::new(source),
            view: None,
            orbit: None,
            sets,
            current,
            texture_size,
            screen: None,
            bounds: IRect::EMPTY,
            filtered: Vec::new(),
            paint_through: false,
            environment: None,
            environment_name: None,
            stroking: false,
            ending: false,
        }
    }

    /// Names of the texture sets: their materials', else numbered.
    pub(crate) fn set_names(&self) -> Vec<String> {
        let parts = self.mesh.parts.iter().enumerate();
        parts
            .map(|(i, p)| if p.material.is_empty() { format!("Texture Set {}", i + 1) } else { p.material.clone() })
            .collect()
    }

    /// How the model is seen now.
    fn orbit(&self) -> Option<Orbit> {
        self.view.as_ref().map(|v| v.orbit).or(self.orbit)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Opens a model to paint on: `file` the model file named `file_name` (OBJ, USDZ or USDC, by
    /// its extension), `mtl` an OBJ's material file if there is one. Each texture set is
    /// `texture_size` pixels square (0: the default).
    #[uniffi::constructor]
    pub fn open_model(
        file: Vec<u8>,
        file_name: String,
        mtl: Option<String>,
        name: String,
        texture_size: u32,
    ) -> Result<Arc<CanvasSession>, MobileError> {
        let extension = extension_of(&file_name);
        let mesh = read_model(&file, &extension, mtl.as_deref())
            .map_err(|e| file_error("opening the model", mesh_error_text(&e)))?;
        let side = if texture_size == 0 { DEFAULT_TEXTURE_SIZE } else { texture_size.clamp(64, MOST_TEXTURE_SIZE) };
        let scene = Scene::new(mesh, ModelSource { bytes: file, extension, mtl }, side, 0, Vec::new());
        let session = CanvasSession::with_document(empty_texture(side), name, Vec::new());
        session.shared.lock().scene = Some(scene);
        Ok(session)
    }

    /// The model painted on, if this is one.
    pub fn model_info(&self) -> Option<ModelInfo> {
        self.shared.lock().scene.as_ref().map(ModelInfo::of)
    }

    /// The texture set painted on (the layers show its layers).
    pub fn texture_set(&self) -> u32 {
        self.shared.lock().scene.as_ref().map_or(0, |s| u32::try_from(s.current).unwrap_or(0))
    }

    /// Paints on texture set `index` from now on.
    pub fn choose_texture_set(&self, index: u32) {
        let mut st = self.shared.lock();
        st.choose_texture_set(index as usize);
        self.shared.frame(&mut st);
    }

    /// Whether strokes paint through the model: every surface under them, not only the
    /// nearest.
    pub fn paint_through(&self) -> bool {
        self.shared.lock().scene.as_ref().is_some_and(|s| s.paint_through)
    }

    /// Lights the model by the environment `name` whose panorama is `hdr` (a Radiance `.hdr`)
    /// and shows it behind the model; false if it can't be read.
    pub fn set_model_environment(&self, name: String, hdr: Vec<u8>) -> bool {
        let Some(panorama) = procraft_engine::environment::read_hdr(&hdr) else { return false };
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return false };
        scene.environment = Some(panorama);
        scene.environment_name = Some(name);
        self.shared.frame(&mut st);
        true
    }

    /// The name of the environment the model was last lit by, if one was set.
    pub fn model_environment(&self) -> Option<String> {
        self.shared.lock().scene.as_ref().and_then(|s| s.environment_name.clone())
    }

    pub fn set_paint_through(&self, on: bool) {
        if let Some(scene) = self.shared.lock().scene.as_mut() {
            scene.paint_through = on;
        }
    }

    /// One finger dragged by (`dx`, `dy`) window pixels across a model: it turns.
    pub fn turn_model(&self, dx: f32, dy: f32) {
        let mut st = self.shared.lock();
        st.scene_turn(dx, dy);
        self.shared.frame(&mut st);
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Writes the model as OBJ into a zip at `path`: the model, its material file, and each
    /// texture set's picture as a PNG (over the part's base colour, white), named after the
    /// artwork.
    pub fn export_model_obj(&self, path: String) -> Result<(), MobileError> {
        let files = self.model_files()?;
        let base = &files.base;
        let images: Vec<(String, String)> =
            files.names.iter().map(|n| (n.clone(), format!("{base}_{n}.png"))).collect();
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add(
            &format!("{base}.obj"),
            procraft_engine::mesh::write_obj(&files.mesh, &format!("{base}.mtl"), &files.names).as_bytes(),
        );
        zip.add(&format!("{base}.mtl"), procraft_engine::mesh::write_mtl(&images).as_bytes());
        for ((_, file), png) in images.iter().zip(&files.pngs) {
            zip.add(file, png);
        }
        let bytes = zip.finish();
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("exporting the model", e)))
    }

    /// Writes the model as a USDZ package at `path`: the mesh with a preview-surface material
    /// for each texture set, reading its picture (as for [`CanvasSession::export_model_obj`]).
    pub fn export_model_usdz(&self, path: String) -> Result<(), MobileError> {
        let files = self.model_files()?;
        let textures: Vec<(String, Vec<u8>)> = files.names.iter().map(|n| format!("{n}.png")).zip(files.pngs).collect();
        let bytes = procraft_engine::usd::write_usdz(&files.mesh, &textures);
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("exporting the model", e)))
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Each texture set's picture as a PNG in folder `dir` (made if missing; as for
    /// [`CanvasSession::export_model_obj`]); returns the files' paths.
    pub fn export_model_textures(&self, dir: String) -> Result<Vec<String>, MobileError> {
        let files = self.model_files()?;
        let dir = Path::new(&dir);
        fs::create_dir_all(dir).map_err(|e| file_error("making the folder", e))?;
        let mut out = Vec::new();
        for (name, png) in files.names.iter().zip(&files.pngs) {
            let path = dir.join(format!("{}_{name}.png", files.base));
            write_atomically(&path, |p| fs::write(p, png).map_err(|e| file_error("exporting the textures", e)))?;
            out.push(path.to_string_lossy().into_owned());
        }
        Ok(out)
    }

    /// The model as the window shows it, at the window's size, as a PNG at `path`: on the
    /// canvas's surround, or with `transparent`, on nothing.
    pub fn export_model_render(&self, path: String, transparent: bool) -> Result<(), MobileError> {
        let (w, h, rgba) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let (w, h) = st.window_size;
            let (Some(gpu), Some(view)) = (st.gpu.as_ref(), st.scene.as_mut().and_then(|s| s.view.as_mut())) else {
                return Err(file_error("rendering the model", "the model is not shown yet"));
            };
            let s = crate::SURROUND;
            let clear = if transparent { [0.0; 4] } else { [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0] };
            let rgba = view.picture(gpu, w, h, clear).map_err(|e| file_error("rendering the model", e))?;
            (w, h, rgba)
        };
        let png = crate::project::rgba_png(w, h, &rgba);
        write_atomically(Path::new(&path), |p| fs::write(p, &png).map_err(|e| file_error("rendering the model", e)))
    }
}

/// How far a turntable's swing goes each way (radians).
const SWING: f32 = std::f32::consts::PI / 6.0;

#[uniffi::export]
impl CanvasSession {
    /// Frame `index` of `count` of the model on a turntable, as the window shows it and at
    /// `width` × `height`: turning once round, or with `swing`, swinging a twelfth of a turn
    /// each way and back. Opaque, each pixel's bytes blue, green, red, alpha (an ARGB integer
    /// each, little-endian), ready for a video frame; none if the model isn't shown yet.
    pub fn model_turntable_frame(
        &self,
        index: u32,
        count: u32,
        swing: bool,
        width: u32,
        height: u32,
    ) -> Option<crate::layers::ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (Some(gpu), Some(view)) = (st.gpu.as_ref(), st.scene.as_mut().and_then(|s| s.view.as_mut())) else {
            return None;
        };
        let phase = index as f32 / count.max(1) as f32 * std::f32::consts::TAU;
        let angle = if swing { SWING * phase.sin() } else { phase };
        let kept = view.orbit;
        view.orbit = kept.spun(angle);
        let s = crate::SURROUND;
        let clear = [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0];
        let picture = view.picture(gpu, width.max(1), height.max(1), clear);
        view.orbit = kept;
        match picture {
            Ok(mut rgba) => {
                for p in rgba.as_chunks_mut::<4>().0 {
                    *p = [p[2], p[1], p[0], 255];
                }
                Some(crate::layers::ThumbnailImage { width: width.max(1), height: height.max(1), rgba })
            }
            Err(e) => {
                log::warn!("turntable frame: {e}");
                None
            }
        }
    }
}

/// What a model's exports are made of: the model, its texture sets' names (safe as file names,
/// different from each other), their pictures as PNGs over white, and the artwork's name.
struct ModelFiles {
    mesh: Mesh,
    names: Vec<String>,
    pngs: Vec<Vec<u8>>,
    base: String,
}

impl CanvasSession {
    fn model_files(&self) -> Result<ModelFiles, MobileError> {
        let (mesh, set_names, pictures, base) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let pictures = st.texture_pictures()?;
            let Some(scene) = st.scene.as_ref() else { return Err(file_error("exporting the model", "no model")) };
            (scene.mesh.clone(), scene.set_names(), pictures, file_stem(&st.name, "Model"))
        };
        let mut names: Vec<String> = Vec::new();
        for name in set_names {
            let mut n = file_stem(&name, "Material");
            if names.contains(&n) {
                n = format!("{n}_{}", names.len() + 1);
            }
            names.push(n);
        }
        let pngs = pictures
            .into_iter()
            .map(|(side, mut px)| {
                // over white, opaque
                for p in px.as_chunks_mut::<4>().0 {
                    let clear = 255 - p[3];
                    *p = [p[0].saturating_add(clear), p[1].saturating_add(clear), p[2].saturating_add(clear), 255];
                }
                crate::project::rgba_png(side, side, &px)
            })
            .collect();
        Ok(ModelFiles { mesh, names, pngs, base })
    }
}

/// A name made safe as part of a file name: letters, digits, `-` and `_` kept, the rest `_`.
fn file_stem(name: &str, otherwise: &str) -> String {
    let kept: String =
        name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if kept.is_empty() { otherwise.to_owned() } else { kept }
}

impl State {
    /// Each texture set's picture (premultiplied RGBA8, its side), drawn now.
    fn texture_pictures(&mut self) -> Result<Vec<(u32, Vec<u8>)>, MobileError> {
        let failed = |e: String| file_error("reading the model's textures", e);
        let gpu = self.gpu.as_ref().ok_or_else(|| failed("the model is not shown yet".to_owned()))?;
        let Some(scene) = self.scene.as_mut() else { return Ok(Vec::new()) };
        let side = scene.texture_size;
        let mut out = Vec::new();
        for (i, set) in scene.sets.iter_mut().enumerate() {
            let px = if i == scene.current {
                let canvas = self.canvas.as_mut().ok_or_else(|| failed("the model is not shown yet".to_owned()))?;
                canvas.render(gpu, &mut self.doc, None);
                canvas.read_composite(gpu).map_err(|e| failed(e.to_string()))?
            } else {
                let Some(set) = set else { continue };
                if set.canvas.is_none() {
                    let mut c = Canvas::new(gpu, &set.doc).map_err(|e| failed(e.to_string()))?;
                    for (id, pixels) in std::mem::take(&mut set.pending) {
                        c.upload_layer(gpu, id, &pixels).map_err(|e| failed(e.to_string()))?;
                    }
                    set.canvas = Some(c);
                }
                let Some(canvas) = set.canvas.as_mut() else { continue };
                canvas.render(gpu, &mut set.doc, None);
                canvas.read_composite(gpu).map_err(|e| failed(e.to_string()))?
            };
            out.push((side, px));
        }
        Ok(out)
    }
}

/// Why a model can't be painted on, in words.
fn mesh_error_text(e: &MeshError) -> String {
    match e {
        MeshError::NoFaces => "it has no faces".to_owned(),
        MeshError::NoTextureCoordinates => "it has no texture coordinates".to_owned(),
        MeshError::TooLarge => format!("it has more than {MAX_TRIANGLES} triangles"),
        MeshError::Malformed(line) => format!("line {line} can't be read"),
        MeshError::Unreadable => "it isn't a model file that can be read".to_owned(),
    }
}

/// A model kept in `artwork.json`: its texture sets but the current one (the artwork's own
/// document), and how it was last seen.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct SavedModel {
    texture_size: u32,
    current: usize,
    /// The other texture sets' documents, by part (none in the current one's place).
    sets: Vec<Option<Document>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orbit: Option<SavedOrbit>,
    /// The model file's kind: it is `model.<this>`.
    #[serde(default = "obj")]
    extension: String,
    #[serde(default)]
    paint_through: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    environment: Option<String>,
}

fn obj() -> String {
    "obj".to_owned()
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
struct SavedOrbit {
    target: [f32; 3],
    /// Older saves: the turn about the upright axis and the tilt (radians).
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    pitch: f32,
    /// The camera's turn (a quaternion: i, j, k, real).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotation: Option<[f32; 4]>,
    distance: f32,
    fov_y: f32,
}

/// What a save of a model writes besides the artwork's own, read under the lock.
pub(crate) struct ModelSnapshot {
    pub(crate) saved: SavedModel,
    source: Arc<ModelSource>,
    /// The waiting texture sets' layers, by part.
    layers: Vec<(usize, Pixels)>,
    /// The model as last shown (premultiplied RGBA8, its size), for the gallery.
    pub(crate) picture: Option<(u32, u32, Vec<u8>)>,
}

/// What the last saves of a model wrote: a hash of each waiting texture set's layers.
pub(crate) type SavedSets = HashMap<(usize, LayerId), u64>;

impl State {
    /// The model's part of a save, if this is a model.
    pub(crate) fn model_snapshot(&mut self) -> Result<Option<ModelSnapshot>, MobileError> {
        let (Some(scene), gpu) = (self.scene.as_mut(), self.gpu.as_ref()) else { return Ok(None) };
        let mut docs = Vec::new();
        let mut layers = Vec::new();
        for (i, set) in scene.sets.iter_mut().enumerate() {
            let Some(set) = set else {
                docs.push(None);
                continue;
            };
            let pixels = match (set.canvas.as_mut(), gpu) {
                (Some(canvas), Some(gpu)) => {
                    canvas.render(gpu, &mut set.doc, None);
                    let mut pixels = Vec::new();
                    for layer in set.doc.layers() {
                        let rgba = canvas.read_layer(gpu, layer.id).map_err(|e| file_error("reading a layer", e))?;
                        pixels.push((layer.id, rgba));
                    }
                    pixels
                }
                // not drawn yet: what was opened is still what there is
                _ => set.pending.clone(),
            };
            docs.push(Some(set.doc.clone()));
            layers.push((i, pixels));
        }
        let orbit = scene.orbit().map(|o| SavedOrbit {
            target: o.target,
            yaw: 0.0,
            pitch: 0.0,
            rotation: Some(o.rotation),
            distance: o.distance,
            fov_y: o.fov,
        });
        // the model as last seen, square as the gallery shows a texture set, on the surround
        let side = THUMBNAIL_SIDE;
        let s = crate::SURROUND;
        let clear = [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0];
        let picture = match (scene.view.as_mut(), gpu) {
            (Some(view), Some(gpu)) => view.picture(gpu, side, side, clear).ok().map(|px| (side, side, px)),
            _ => None,
        };
        Ok(Some(ModelSnapshot {
            saved: SavedModel {
                texture_size: scene.texture_size,
                current: scene.current,
                sets: docs,
                orbit,
                extension: scene.source.extension.clone(),
                paint_through: scene.paint_through,
                environment: scene.environment_name.clone(),
            },
            source: scene.source.clone(),
            layers,
            picture,
        }))
    }
}

/// Writes a model's files into its artwork folder `dir`: the model (once), and the waiting texture
/// sets' layers that changed since the last save there.
pub(crate) fn write_model(saved: &mut SavedSets, dir: &Path, model: &ModelSnapshot) -> Result<(), MobileError> {
    let file = dir.join(format!("model.{}", model.source.extension));
    if !file.exists() {
        write_atomically(&file, |p| fs::write(p, &model.source.bytes).map_err(|e| file_error("saving the model", e)))?;
    }
    let mtl = dir.join("model.mtl");
    if let Some(text) = model.source.mtl.as_ref()
        && !mtl.exists()
    {
        write_atomically(&mtl, |p| fs::write(p, text).map_err(|e| file_error("saving the model", e)))?;
    }
    let side = model.saved.texture_size;
    for (part, pixels) in &model.layers {
        let folder = dir.join("sets").join(part.to_string());
        fs::create_dir_all(&folder).map_err(|e| file_error("making the folder", e))?;
        for (id, rgba) in pixels {
            let hash = pixels_hash(rgba);
            let path = folder.join(format!("{}.png", id.0));
            if saved.get(&(*part, *id)) == Some(&hash) && path.exists() {
                continue;
            }
            write_atomically(&path, |p| write_png(p, side, side, rgba, &[]))?;
            saved.insert((*part, *id), hash);
        }
        // layer files the set no longer has
        if let Ok(entries) = fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !pixels.iter().any(|(id, _)| path == folder.join(format!("{}.png", id.0))) {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
    saved.retain(|(part, id), _| model.layers.iter().any(|(p, px)| p == part && px.iter().any(|(i, _)| i == id)));
    Ok(())
}

/// The model saved in the artwork folder `dir`, as `saved` describes it.
pub(crate) fn open_saved_model(dir: &Path, saved: &SavedModel) -> Result<Scene, MobileError> {
    let extension = extension_of(&format!("model.{}", saved.extension));
    let bytes = fs::read(dir.join(format!("model.{extension}"))).map_err(|e| file_error("opening the model", e))?;
    let mtl = fs::read_to_string(dir.join("model.mtl")).ok();
    let mesh = read_model(&bytes, &extension, mtl.as_deref())
        .map_err(|e| file_error("opening the model", mesh_error_text(&e)))?;
    let side = saved.texture_size.clamp(64, MOST_TEXTURE_SIZE);
    let waiting = saved
        .sets
        .iter()
        .enumerate()
        .map(|(part, doc)| {
            let doc = doc.as_ref()?.clone().repaired();
            if (doc.width(), doc.height()) != (side, side) {
                return None;
            }
            let folder = dir.join("sets").join(part.to_string());
            let mut pending = Vec::new();
            for layer in doc.layers() {
                let path = folder.join(format!("{}.png", layer.id.0));
                // a layer with no file is empty
                if !path.exists() {
                    continue;
                }
                match read_png(&path, side, side) {
                    Ok(rgba) => pending.push((layer.id, rgba)),
                    Err(e) => log::error!("texture set {part} layer {:?}: {e}", layer.id),
                }
            }
            Some(TextureSet { doc, canvas: None, pending })
        })
        .collect();
    let mut scene = Scene::new(mesh, ModelSource { bytes, extension, mtl }, side, saved.current, waiting);
    scene.paint_through = saved.paint_through;
    scene.environment_name = saved.environment.clone();
    scene.orbit = saved.orbit.map(|o| Orbit {
        target: o.target,
        rotation: o.rotation.unwrap_or_else(|| Orbit::rotation_of(o.yaw, o.pitch)),
        distance: o.distance,
        fov: o.fov_y,
    });
    Ok(scene)
}

impl State {
    /// Makes texture set `index` the current one: the session's document and canvas.
    pub(crate) fn choose_texture_set(&mut self, index: usize) {
        if self.scene.as_ref().is_none_or(|s| index == s.current || index >= s.sets.len()) {
            return;
        }
        self.scene_stroke_cancel();
        self.settle();
        let Some(scene) = self.scene.as_mut() else { return };
        let Some(set) = scene.sets.get_mut(index).and_then(Option::take) else { return };
        let old = TextureSet {
            doc: std::mem::replace(&mut self.doc, set.doc),
            canvas: std::mem::replace(&mut self.canvas, set.canvas),
            pending: std::mem::replace(&mut self.pending_pixels, set.pending),
        };
        if let Some(slot) = scene.sets.get_mut(scene.current) {
            *slot = Some(old);
        }
        scene.current = index;
        self.ensure_canvas();
    }

    /// A stroke begins on the model: built in window pixels, drawn on the screen's canvas and
    /// carried onto the current texture set's layer.
    pub(crate) fn scene_stroke_begin(&mut self, pointer: PointerKind, samples: &[PenSample]) {
        let current = self.doc.current();
        if self
            .doc
            .layer(current)
            .is_none_or(|l| l.locked || !l.visible || l.kind == LayerKind::Group || l.text.is_some())
        {
            return;
        }
        let tool = if pointer == PointerKind::StylusEraser { ToolKind::Erase } else { self.tool };
        if !matches!(tool.stroke_tool(), StrokeTool::Paint | StrokeTool::Erase) {
            return;
        }
        let (brush, mut style, diameter) = self.stroke_brush(tool, current);
        // the brush shows as big as on the texture fitted to the window
        let size = self.window_size;
        let diameter = diameter * View::fit((self.doc.width(), self.doc.height()), size).zoom();
        self.strokes = self.strokes.wrapping_add(1);
        self.info.strokes = self.info.strokes.saturating_add(1);
        let seed = self.strokes.wrapping_mul(0x9e37_79b9);
        let mut builder = StrokeBuilder::with_seed(brush.clone(), diameter, pointer.into(), seed)
            .with_input(self.input.clone())
            .seen_at(1.0);
        let stamps = builder.add(&window_samples(samples));
        let (Some(gpu), Some(scene)) = (self.gpu.as_ref(), self.scene.as_mut()) else { return };
        // the screen's canvas, the window's size
        let stale = scene.screen.as_ref().is_none_or(|(d, _)| (d.width(), d.height()) != size);
        if stale {
            let doc = Document::new(size.0.max(1), size.1.max(1));
            match Canvas::new(gpu, &doc) {
                Ok(c) => scene.screen = Some((doc, c)),
                Err(e) => return log::error!("3D stroke canvas: {e}"),
            }
        }
        let Some((screen_doc, screen)) = scene.screen.as_mut() else { return };
        // the screen's canvas holds its own copies of the brush's shape and grain
        let canvas_style = style;
        screen.use_sources(gpu, &mut style, &brush);
        screen.begin_stroke(screen_doc.current(), style);
        screen.add_stamps(&stamps);
        scene.bounds = bounds_of(&stamps);
        scene.stroking = true;
        scene.ending = false;
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.begin_drawn_stroke(current, canvas_style);
        }
        self.stroke = Some(builder);
    }

    pub(crate) fn scene_stroke_add(&mut self, samples: &[PenSample], last: bool) {
        let Some(builder) = self.stroke.as_mut() else { return };
        let mut stamps = builder.add(&window_samples(samples));
        if last && let Some(mut b) = self.stroke.take() {
            stamps.extend(b.finish());
        }
        let Some(scene) = self.scene.as_mut() else { return };
        scene.bounds = scene.bounds.union(bounds_of(&stamps));
        if let Some((_, screen)) = scene.screen.as_mut() {
            screen.add_stamps(&stamps);
        }
        if last {
            // carried on at once and kept, so undo sees it as soon as the pen lifts
            scene.ending = true;
            self.carry_stroke();
        }
    }

    pub(crate) fn scene_stroke_cancel(&mut self) {
        self.stroke = None;
        let Some(scene) = self.scene.as_mut() else { return };
        scene.stroking = false;
        scene.ending = false;
        if let Some((_, screen)) = scene.screen.as_mut() {
            screen.cancel_stroke();
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.cancel_stroke();
        }
    }

    /// One finger dragged by (`dx`, `dy`) window pixels: the model turns under it.
    pub(crate) fn scene_turn(&mut self, dx: f32, dy: f32) {
        let side = self.window_size.0.min(self.window_size.1).max(1) as f32;
        let Some(view) = self.scene.as_mut().and_then(|s| s.view.as_mut()) else { return };
        let o = &mut view.orbit;
        let length = (dx * dx + dy * dy).sqrt();
        o.turn(dx, dy, length / side * TURN_PER_SIDE);
    }

    /// Two fingers: pinching brings the model nearer, moving them moves it across the screen.
    pub(crate) fn scene_gesture(&mut self, pan: Vec2, scale: f32) {
        let side = self.window_size.0.min(self.window_size.1).max(1) as f32;
        let Some(scene) = self.scene.as_mut() else { return };
        let unit = Orbit::unit(&scene.mesh);
        let Some(view) = scene.view.as_mut() else { return };
        let o = &mut view.orbit;
        if scale.is_finite() && scale > 0.0 {
            o.distance = (o.distance / scale).clamp(NEAREST * unit, FARTHEST * unit);
        }
        // the target moves in the screen's plane, as far as the fingers did at its distance
        let per_pixel = 2.0 * o.distance * (o.fov / 2.0).tan() / side;
        let (right, up) = (o.right(), o.up());
        for k in 0..3 {
            if let (Some(t), Some(r), Some(u)) = (o.target.get_mut(k), right.get(k), up.get(k)) {
                *t += (-pan.x * r + pan.y * u) * per_pixel;
            }
        }
    }

    /// Carries the stroke drawn on the screen so far onto the current texture set; once the pen
    /// has lifted, the stroke ends there. Not before the model has been drawn once.
    fn carry_stroke(&mut self) {
        let size = self.window_size;
        let (Some(gpu), Some(scene)) = (self.gpu.as_ref(), self.scene.as_mut()) else { return };
        if !scene.stroking {
            return;
        }
        let (Some((screen_doc, screen)), Some(view), Some(canvas)) =
            (scene.screen.as_mut(), scene.view.as_mut(), self.canvas.as_mut())
        else {
            return;
        };
        screen.render(gpu, screen_doc, None);
        view.paint_through = scene.paint_through;
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("3D stroke") });
        let reach = view.project_stroke(
            gpu,
            &mut encoder,
            scene.current,
            screen.stroke_view(),
            scene.bounds,
            size.0,
            size.1,
            canvas.stroke_target(),
        );
        gpu.queue.submit([encoder.finish()]);
        canvas.stroke_drawn(reach);
        if scene.ending {
            canvas.end_stroke();
            screen.cancel_stroke();
            screen.render(gpu, screen_doc, None);
            scene.stroking = false;
            scene.ending = false;
        }
    }

    /// Draws a frame of the model into `target` (the window's size): the stroke carried onto it,
    /// the current texture set composited, the model drawn with each set showing.
    #[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
    pub(crate) fn render_scene(
        &mut self,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        convert: Option<[[f32; 3]; 3]>,
    ) {
        {
            let (Some(gpu), Some(scene)) = (self.gpu.as_ref(), self.scene.as_mut()) else { return };
            if scene.view.is_none() {
                let mut view = MeshView::new(gpu, &scene.mesh, scene.texture_size, format);
                if let Some(orbit) = scene.orbit.take() {
                    view.orbit = orbit;
                }
                // the surround behind the model until an environment comes
                view.show_environment(gpu, false);
                scene.view = Some(view);
            }
            // the waiting texture sets' canvases, with the pixels they opened with
            for set in scene.sets.iter_mut().flatten() {
                if set.canvas.is_none() {
                    set.canvas = Canvas::new(gpu, &set.doc).ok();
                    if let Some(c) = set.canvas.as_mut() {
                        for (id, pixels) in std::mem::take(&mut set.pending) {
                            if let Err(e) = c.upload_layer(gpu, id, &pixels) {
                                log::error!("opening a texture set's layer: {e}");
                            }
                        }
                        c.render(gpu, &mut set.doc, None);
                    }
                }
            }
        }
        self.carry_stroke();
        let (Some(gpu), Some(scene)) = (self.gpu.as_ref(), self.scene.as_mut()) else { return };
        // a new environment: prefiltered now, shown behind the model
        if let (Some(view), Some(panorama)) = (scene.view.as_mut(), scene.environment.take()) {
            view.set_lighting(gpu, Lighting::new(gpu, &panorama));
            view.show_environment(gpu, true);
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.render(gpu, &mut self.doc, None);
        }
        let Some(view) = scene.view.as_mut() else { return };
        view.set_convert(convert);
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("3D view") });
        // each set's texture, its smaller levels made again when its picture has changed
        scene.filtered.resize(scene.sets.len(), None);
        for (i, (set, filtered)) in scene.sets.iter().zip(scene.filtered.iter_mut()).enumerate() {
            let canvas =
                if i == scene.current { self.canvas.as_ref() } else { set.as_ref().and_then(|s| s.canvas.as_ref()) };
            if let Some(c) = canvas
                && *filtered != Some((c.composites(), i == scene.current))
            {
                view.set_albedo_filtered(gpu, &mut encoder, i, c.composite_view());
                *filtered = Some((c.composites(), i == scene.current));
            }
        }
        let s = crate::SURROUND;
        view.render(
            gpu,
            &mut encoder,
            target,
            size.0,
            size.1,
            [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0],
        );
        gpu.queue.submit([encoder.finish()]);
    }
}

/// Input as window pixels (3D strokes are built on the screen).
fn window_samples(samples: &[PenSample]) -> Vec<Sample> {
    samples
        .iter()
        .map(|s| Sample {
            pos: Vec2::new(s.x, s.y),
            pressure: s.pressure,
            tilt: s.tilt,
            azimuth: s.orientation,
            time_ms: s.time_ms,
        })
        .collect()
}

/// The pixels stamps can touch.
fn bounds_of(stamps: &[procraft_engine::stroke::Stamp]) -> IRect {
    stamps.iter().fold(IRect::EMPTY, |r, s| r.union(IRect::around(s.center, s.radius * std::f32::consts::SQRT_2)))
}

/// A model and its texture sets, as the interface sees them.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ModelInfo {
    pub texture_sets: Vec<String>,
    pub triangles: u32,
    /// How much of each set's texture its triangles cover twice (0..1).
    pub overlaps: Vec<f32>,
}

impl ModelInfo {
    pub(crate) fn of(scene: &Scene) -> ModelInfo {
        ModelInfo {
            texture_sets: scene.set_names(),
            triangles: u32::try_from(scene.mesh.triangles.len()).unwrap_or(u32::MAX),
            overlaps: (0..scene.mesh.parts.len()).map(|i| scene.mesh.uv_overlap(i, 256)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use procraft_gpu::{Gpu, wgpu};

    use crate::{CanvasSession, PenSample, PointerKind};

    /// A cube whose every face covers the whole texture.
    const CUBE: &str = "v -1 -1 -1\nv 1 -1 -1\nv 1 1 -1\nv -1 1 -1\nv -1 -1 1\nv 1 -1 1\nv 1 1 1\nv -1 1 1\n\
                        vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n\
                        f 1/1 4/4 3/3 2/2\nf 5/1 6/2 7/3 8/4\nf 4/1 8/2 7/3 3/4\n\
                        f 1/1 2/2 6/3 5/4\nf 2/1 3/2 7/3 6/4\nf 1/1 5/2 8/3 4/4\n";

    fn sample(x: f32, y: f32, t: f64) -> PenSample {
        PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: t }
    }

    /// Draws a frame of the model into a `side` square texture.
    fn frame(s: &CanvasSession, side: u32) {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = st.gpu.as_ref().unwrap().device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test frame"),
            size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        st.render_scene(&view, format, (side, side), None);
        assert_eq!(st.gpu.as_ref().unwrap().errors(), 0, "GPU validation errors");
    }

    /// How many of the current texture set's pixels have paint.
    fn painted(s: &CanvasSession) -> usize {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        canvas.read_composite(gpu).unwrap().chunks(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn a_stroke_across_a_model_paints_its_texture() {
        let gpu = match Gpu::headless() {
            Ok(g) => g,
            Err(e) => return eprintln!("skipped: {e}"),
        };
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), None, "Cube".into(), 256).unwrap();
        let info = s.model_info().unwrap();
        assert_eq!((info.texture_sets.len(), info.triangles), (1, 12));
        {
            let mut st = s.shared.lock();
            st.window_size = (200, 200);
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        // lit by an environment, shown behind it (set up on the next frame)
        let sky = procraft_engine::environment::Equirect::uniform([0.8, 0.9, 1.0]);
        assert!(s.set_model_environment("sky".into(), procraft_engine::environment::write_hdr(&sky)));
        assert!(!s.set_model_environment("bad".into(), b"not a picture".to_vec()));
        assert_eq!(s.model_environment().as_deref(), Some("sky"));
        frame(&s, 200);
        assert_eq!(painted(&s), 0);
        // across the middle of the window, where the cube faces the camera
        s.stroke_begin(PointerKind::Stylus, vec![sample(70.0, 100.0, 0.0)]);
        frame(&s, 200);
        s.stroke_add((1..16).map(|i| sample(70.0 + i as f32 * 4.0, 100.0, f64::from(i) * 8.0)).collect());
        frame(&s, 200);
        s.stroke_end(vec![sample(134.0, 100.0, 140.0)]);
        // kept as the pen lifts, before the next frame
        assert!(s.info().can_undo);
        frame(&s, 200);
        let kept = painted(&s);
        assert!(kept > 100, "{kept} pixels painted");
        assert!(s.info().can_undo);
        s.undo();
        assert_eq!(painted(&s), 0);

        // turning the model and painting again reaches another face
        s.turn_model(100.0, 0.0);
        s.stroke_begin(PointerKind::Stylus, vec![sample(100.0, 70.0, 0.0)]);
        s.stroke_end(vec![sample(100.0, 130.0, 100.0)]);
        frame(&s, 200);
        assert!(painted(&s) > 100);
    }

    #[test]
    fn two_fingers_bring_the_model_nearer_and_move_it() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), None, "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let orbit = |s: &CanvasSession| s.shared.lock().scene.as_ref().and_then(|sc| sc.orbit()).unwrap();
        let before = orbit(&s);
        // spread to twice as far apart: half as far away
        s.view_gesture(100.0, 100.0, 0.0, 0.0, 2.0, 0.0);
        let nearer = orbit(&s);
        assert!((nearer.distance - before.distance / 2.0).abs() < 1e-4);
        // moved right together: the model goes right, so the camera's target goes left of it
        s.view_gesture(100.0, 100.0, 50.0, 0.0, 1.0, 0.0);
        let moved = orbit(&s);
        assert!(moved.target[0] < nearer.target[0] - 1e-3, "{:?}", moved.target);
        assert!((moved.target[1] - nearer.target[1]).abs() < 1e-4);
        // the cube's middle is now right of the window's
        let (gpu_ok, middle_x) = {
            let st = s.shared.lock();
            let o = st.scene.as_ref().and_then(|sc| sc.orbit()).unwrap();
            let m = o.view_proj(1.0);
            let clip_x = m[3][0];
            let w = m[3][3];
            (st.gpu.is_some(), clip_x / w)
        };
        assert!(gpu_ok && middle_x > 0.0, "{middle_x}");
    }

    #[test]
    fn a_model_exports_as_obj_with_its_textures() {
        let two = CUBE
            .replacen(
                "f 1/1",
                "usemtl front
f 1/1",
                1,
            )
            .replacen(
                "f 1/1 2/2",
                "usemtl side wall
f 1/1 2/2",
                1,
            );
        let s = CanvasSession::open_model(two.into_bytes(), "two.obj".into(), None, "Two cubes".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        dab(&s);
        let path = std::env::temp_dir().join(format!("procraft-obj-{}.zip", std::process::id()));
        s.export_model_obj(path.to_string_lossy().into_owned()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let zip = procraft_engine::zip::ZipReader::new(&bytes).unwrap();
        let mut names: Vec<&str> = zip.names().collect();
        names.sort_unstable();
        assert_eq!(names, ["Two_cubes.mtl", "Two_cubes.obj", "Two_cubes_front.png", "Two_cubes_side_wall.png"]);
        let obj = String::from_utf8(zip.read("Two_cubes.obj").unwrap()).unwrap();
        let mtl = String::from_utf8(zip.read("Two_cubes.mtl").unwrap()).unwrap();
        let mesh = procraft_engine::mesh::read_obj(&obj, Some(&mtl)).unwrap();
        assert_eq!(mesh.triangles.len(), 12);
        assert_eq!(mesh.materials[1].color_map.as_deref(), Some("Two_cubes_side_wall.png"));
        // the painted set's picture has the stroke on white
        let png = zip.read("Two_cubes_front.png").unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut px = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut px).unwrap();
        assert!(px.chunks(4).all(|p| p[3] == 255));
        assert!(px.chunks(4).any(|p| p[0] < 128) && px.chunks(4).any(|p| p[0] == 255));
        let _ = std::fs::remove_file(&path);
        // the pictures on their own, and the model as shown
        let dir = std::env::temp_dir().join(format!("procraft-textures-{}", std::process::id()));
        let pngs = s.export_model_textures(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!(pngs.len(), 2);
        assert!(pngs.iter().all(|p| std::path::Path::new(p).exists()));
        let _ = std::fs::remove_dir_all(&dir);
        let render = std::env::temp_dir().join(format!("procraft-render-{}.png", std::process::id()));
        s.export_model_render(render.to_string_lossy().into_owned(), true).unwrap();
        let mut reader =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&render).unwrap())).read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (200, 200));
        let mut px = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut px).unwrap();
        // the model in the middle, nothing at the corner
        assert_eq!(px[3], 0);
        assert_eq!(px[(100 * 200 + 100) * 4 + 3], 255);
        let _ = std::fs::remove_file(&render);
        // a turntable's frames: the model turned, half way round the back
        let first = s.model_turntable_frame(0, 4, false, 100, 100).unwrap();
        let half = s.model_turntable_frame(2, 4, false, 100, 100).unwrap();
        assert_eq!((first.width, first.height, first.rgba.len()), (100, 100, 100 * 100 * 4));
        assert!(first.rgba.chunks(4).all(|p| p[3] == 255));
        assert_ne!(first.rgba, half.rgba);
        let again = s.model_turntable_frame(4, 4, false, 100, 100).unwrap();
        assert_eq!(first.rgba, again.rgba);
        // as a USDZ: the layer and both pictures
        s.export_model_usdz(path.to_string_lossy().into_owned()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let zip = procraft_engine::zip::ZipReader::new(&bytes).unwrap();
        assert_eq!(zip.names().collect::<Vec<_>>(), ["model.usda", "textures/front.png", "textures/side_wall.png"]);
        let _ = std::fs::remove_file(&path);
    }

    /// Gives `s` a headless GPU and a 200 px square window; false without a GPU.
    fn shown(s: &CanvasSession) -> bool {
        let gpu = match Gpu::headless() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipped: {e}");
                return false;
            }
        };
        let mut st = s.shared.lock();
        st.window_size = (200, 200);
        st.gpu = Some(gpu);
        st.ensure_canvas();
        true
    }

    /// A dab in the middle of the window.
    fn dab(s: &CanvasSession) {
        s.stroke_begin(PointerKind::Stylus, vec![sample(90.0, 100.0, 0.0)]);
        s.stroke_end(vec![sample(110.0, 100.0, 50.0)]);
        frame(s, 200);
    }

    #[test]
    fn a_model_saves_with_each_texture_set_and_how_it_was_seen() {
        // the cube's front, top and back in one material, its bottom and sides in another
        let two = CUBE.replacen("f 1/1", "usemtl front\nf 1/1", 1).replacen("f 1/1 2/2", "usemtl sides\nf 1/1 2/2", 1);
        let s = CanvasSession::open_model(two.into_bytes(), "two.OBJ".into(), None, "Two".into(), 128).unwrap();
        assert_eq!(s.model_info().unwrap().texture_sets, ["front", "sides"]);
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        dab(&s);
        let front = painted(&s);
        assert!(front > 20, "{front}");
        // the right side turned to face the camera, painted in its own set
        s.choose_texture_set(1);
        assert_eq!(s.texture_set(), 1);
        assert_eq!(painted(&s), 0);
        s.turn_model(-100.0, 0.0);
        frame(&s, 200);
        dab(&s);
        let side = painted(&s);
        assert!(side > 20, "{side}");

        let dir = std::env::temp_dir().join(format!("procraft-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!s.paint_through());
        s.set_paint_through(true);
        let sky = procraft_engine::environment::write_hdr(&procraft_engine::environment::Equirect::uniform([0.5; 3]));
        assert!(s.set_model_environment("coast".into(), sky));
        s.save(dir.to_string_lossy().into_owned()).unwrap();
        for file in ["model.obj", "thumbnail.png", "artwork.json", "sets/0"] {
            assert!(dir.join(file).exists(), "{file}");
        }
        let again = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!(again.model_info().unwrap().texture_sets, ["front", "sides"]);
        assert_eq!(again.texture_set(), 1);
        assert!(shown(&again));
        assert_eq!(painted(&again), side);
        assert!(again.paint_through());
        assert_eq!(again.model_environment().as_deref(), Some("coast"));
        // seen as it was left: the side still faces the camera
        let turn = |s: &CanvasSession| s.shared.lock().scene.as_ref().and_then(|sc| sc.orbit()).unwrap().rotation;
        assert!(turn(&again).iter().zip(turn(&s)).all(|(a, b)| (a - b).abs() < 1e-6));
        frame(&again, 200);
        // and painted on again, elsewhere
        again.stroke_begin(PointerKind::Stylus, vec![sample(80.0, 80.0, 0.0)]);
        again.stroke_end(vec![sample(120.0, 80.0, 50.0)]);
        frame(&again, 200);
        let more = painted(&again);
        assert!(more > side, "{more} after {side}");
        again.choose_texture_set(0);
        assert_eq!(painted(&again), front);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
