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
use procraft_engine::mesh::{MAX_TRIANGLES, Mesh, MeshError, PartImages, read_obj};
use procraft_engine::stroke::StrokeBuilder;
use procraft_engine::usd::{read_usd, read_usdc, read_usdz};
use procraft_engine::view::View;
use procraft_gpu::{Canvas, Light, Lighting, MOST_LIGHTS, MeshView, Orbit, StrokeTool, wgpu};

use crate::project::{THUMBNAIL_SIDE, file_error, pixels_hash, read_png, write_atomically, write_png};
use crate::{CanvasSession, MobileError, PenSample, PointerKind, State, ToolKind};

/// How far a one-finger drag turns the model: this many radians across the window's shorter
/// side, wherever the finger is and however fast it goes (measured: 0.0122 rad a point on a 744
/// point side).
const TURN_PER_SIDE: f32 = 9.08;
/// How far a drag across the Lighting Studio takes its camera round: radians across the shorter
/// side.
const STUDIO_TURN_PER_SIDE: f32 = 2.93;
/// The nearest and farthest the camera comes, in the framing's units (the model's largest side
/// counted as 4).
const NEAREST: f32 = 0.05;
const FARTHEST: f32 = 30.0;
/// The brightest the model's light can be taken.
const MOST_EXPOSURE: f32 = 5.0;
/// Where a light added in the Lighting Studio stands: just above and behind the model's middle
/// (framing units).
const NEW_LIGHT: [f32; 3] = [0.8, 2.6, -0.6];
/// How far a duplicated light stands from the one it copies (framing units).
const DUPLICATE_OFFSET: [f32; 3] = [1.0, 0.0, 0.0];
/// How far round a drag across the studio carries its camera (as the model is turned), and the
/// lowest and highest it looks from (radians up from level).
const STUDIO_LOWEST: f32 = 0.05;
const STUDIO_HIGHEST: f32 = 1.5;
/// A light is touched within this many pixels of its cube.
const LIGHT_SLACK: f32 = 24.0;
/// The hue bar's colours (sRGB), evenly spaced from its start to its end.
const HUE_STOPS: [[f32; 3]; 8] = [
    [1.0, 0.0, 0.0],
    [1.0, 110.0 / 255.0, 0.0],
    [1.0, 220.0 / 255.0, 0.0],
    [53.0 / 255.0, 1.0, 0.0],
    [0.0, 1.0, 236.0 / 255.0],
    [0.0, 52.0 / 255.0, 1.0],
    [170.0 / 255.0, 0.0, 1.0],
    [1.0, 0.0, 0.0],
];
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

/// A new texture set's document: `side` square, its one layer the Base Layer, nothing painted (the
/// part's base colour shows).
fn empty_texture(side: u32) -> Document {
    let mut doc = Document::new(side, side);
    doc.background_visible = false;
    if let Some(id) = doc.layers().first().map(|l| l.id) {
        doc.rename(id, "Base Layer");
    }
    doc
}

/// The model's files as they were opened, kept to be saved with the artwork: the model, its
/// kind (its file name's extension) and an OBJ's material file.
pub(crate) struct ModelSource {
    bytes: Vec<u8>,
    extension: String,
    mtl: Option<String>,
}

/// A file picked along with a model: an OBJ's material file, or an image its materials map.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ModelFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// What a model takes from the files picked with it: an OBJ the material file it names (or the
/// only one), and the rest as the images its materials may map. A USDZ packs its own.
fn companions(model: &[u8], extension: &str, files: Vec<ModelFile>) -> (Option<String>, Vec<ModelFile>) {
    if extension == "usdz" {
        return (None, Vec::new());
    }
    if extension != "obj" {
        return (None, files);
    }
    let (libraries, images): (Vec<ModelFile>, Vec<ModelFile>) =
        files.into_iter().partition(|f| extension_of(&f.name) == "mtl");
    let named = String::from_utf8_lossy(model).lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("mtllib")?;
        rest.starts_with(char::is_whitespace).then(|| rest.trim().to_owned())
    });
    let only = if libraries.len() == 1 { libraries.first() } else { None };
    let chosen = named.and_then(|n| companion(&libraries, &n)).or(only);
    (chosen.map(|f| String::from_utf8_lossy(&f.bytes).into_owned()), images)
}

/// The file among `files` that `path` (as a model's file gives it, relative to the model) names:
/// the one at that path, else the one with its name, ignoring case either way.
fn companion<'a>(files: &'a [ModelFile], path: &str) -> Option<&'a ModelFile> {
    let wanted = tidy_path(path);
    files.iter().find(|f| tidy_path(&f.name) == wanted).or_else(|| files.iter().find(|f| same_file(&f.name, path)))
}

/// A relative path as compared: forward slashes, no `./` parts, lower case.
fn tidy_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/").to_lowercase();
    path.split('/').filter(|p| !p.is_empty() && *p != ".").collect::<Vec<_>>().join("/")
}

/// Whether two paths name the same file: their last parts, ignoring case.
fn same_file(a: &str, b: &str) -> bool {
    let last = |p: &str| p.trim().rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
    last(a) == last(b)
}

/// A model zipped up with the files it needs, as a folder of them would be: an OBJ with its
/// material file and images, a USD with its images.
struct Zipped {
    /// The model's path in the zip.
    name: String,
    model: Vec<u8>,
    /// Every other file in the zip, named by its path from the model's folder.
    files: Vec<ModelFile>,
}

/// Whether a file's name is one of a model's kinds that open.
fn is_model_name(name: &str) -> bool {
    matches!(extension_of(name).as_str(), "obj" | "usdz" | "usdc" | "usda" | "usd")
}

/// The model in zip `bytes` (the first of those nearest its top), if it holds one and isn't a
/// package of another kind (an artwork or brushes). Folders, and the hidden files another system
/// adds when it zips (`__MACOSX/`, `._` files), are passed over.
fn zipped_model_name(zip: &procraft_engine::zip::ZipReader<'_>) -> Option<String> {
    let files: Vec<&str> = zip
        .names()
        .filter(|n| {
            let last = n.rsplit('/').next().unwrap_or(n);
            !n.ends_with('/') && !n.starts_with("__MACOSX/") && !last.starts_with("._") && !last.is_empty()
        })
        .collect();
    let package = files.iter().any(|n| {
        let last = n.rsplit('/').next().unwrap_or(n);
        matches!(last, "Document.archive" | "artwork.json" | "Brush.archive" | "brushset.plist")
    });
    if package {
        return None;
    }
    let depth = |n: &str| n.matches('/').count();
    let models = files.iter().filter(|n| is_model_name(n));
    let nearest = models.clone().map(|n| depth(n)).min()?;
    models.into_iter().find(|n| depth(n) == nearest).map(|n| (*n).to_owned())
}

/// The model in a zip and the other files beside it (see [`zipped_model_name`]).
fn unzip_model(bytes: &[u8]) -> Option<Zipped> {
    let zip = procraft_engine::zip::ZipReader::new(bytes)?;
    let name = zipped_model_name(&zip)?;
    let model = zip.read(&name)?;
    let folder = name.rsplit_once('/').map_or("", |(f, _)| f);
    let mut files = Vec::new();
    for entry in zip.names() {
        let last = entry.rsplit('/').next().unwrap_or(entry);
        if entry == name || entry.ends_with('/') || entry.starts_with("__MACOSX/") || last.starts_with("._") {
            continue;
        }
        let Some(bytes) = zip.read(entry) else { continue };
        let relative =
            if folder.is_empty() { Some(entry) } else { entry.strip_prefix(folder).and_then(|r| r.strip_prefix('/')) };
        files.push(ModelFile { name: relative.unwrap_or(entry).to_owned(), bytes });
    }
    Some(Zipped { name, model, files })
}

/// The model a zip of one holds (its path in the zip), as the gallery's Import opens it; none if
/// it holds none or is an artwork, a layered package or brushes.
#[uniffi::export]
pub fn zip_model_name(bytes: Vec<u8>) -> Option<String> {
    zipped_model_name(&procraft_engine::zip::ZipReader::new(&bytes)?)
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
    /// The current texture set is shown flat, as a canvas, in place of the model.
    flat: bool,
    /// An environment to light the model by, waiting for the next frame to be set up.
    environment: Option<procraft_engine::environment::Equirect>,
    /// The environment's name, kept with the model (none: the app's default).
    environment_name: Option<String>,
    /// How bright the light is taken (1 as it is; 0 to 5).
    exposure: f32,
    /// The environment shows behind the model (else the canvas's surround); and whether one has
    /// been set up yet.
    environment_shown: bool,
    lit: bool,
    /// The lights it is lit by, as the interface sets them.
    lights: Vec<ModelLight>,
    /// The Lighting Studio is showing, seen through its own camera (kept while the model is open).
    studio_open: bool,
    studio: Option<Orbit>,
    /// Which of its submeshes are hidden.
    hidden: Vec<bool>,
    /// Each part's roughness and metalness documents, once made; the place of the channel painted
    /// is empty (it is the session's own).
    channels: Vec<[Option<TextureSet>; 2]>,
    /// The channel painted on the current part: 0 its colour, 1 roughness, 2 metalness. While it
    /// isn't the colour, the part's colour document waits in [`Scene::sets`].
    channel: usize,
    /// Each part's channel maps as last given to the view (as [`Scene::filtered`]).
    channel_shown: Vec<[Option<(u64, bool)>; 2]>,
    /// The images picked with an OBJ, until its materials' maps have been taken from them.
    picked: Vec<ModelFile>,
    /// Each part's normal and occlusion maps from the model's file (RGBA8, the texture size
    /// square), shown as they are; and whether the view has them as they are now.
    surface_maps: Vec<[Option<Arc<Vec<u8>>>; 2]>,
    surface_shown: bool,
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
            flat: false,
            environment: None,
            environment_name: None,
            exposure: 1.0,
            environment_shown: true,
            lit: false,
            lights: Light::DEFAULTS.iter().map(ModelLight::from_gpu).collect(),
            studio_open: false,
            studio: None,
            hidden: Vec::new(),
            channels: Vec::new(),
            channel: 0,
            channel_shown: Vec::new(),
            picked: Vec::new(),
            surface_maps: Vec::new(),
            surface_shown: false,
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
    /// its extension), with the `files` picked along with it (an OBJ's material file and the
    /// images it maps). A zip (`.zip`) brings the model and those files in one: as a folder
    /// holding them, its model the one nearest its top. Each texture set is `texture_size` pixels
    /// square (0: the default).
    #[uniffi::constructor]
    pub fn open_model(
        file: Vec<u8>,
        file_name: String,
        files: Vec<ModelFile>,
        name: String,
        texture_size: u32,
    ) -> Result<Arc<CanvasSession>, MobileError> {
        let (file, file_name, files) = if extension_of(&file_name) == "zip" {
            let zipped = unzip_model(&file).ok_or_else(|| file_error("opening the model", "no model in the zip"))?;
            let mut all = zipped.files;
            all.extend(files);
            (zipped.model, zipped.name, all)
        } else {
            (file, file_name, files)
        };
        let extension = extension_of(&file_name);
        let (mtl, picked) = companions(&file, &extension, files);
        let mesh = read_model(&file, &extension, mtl.as_deref())
            .map_err(|e| file_error("opening the model", mesh_error_text(&e)))?;
        let side = if texture_size == 0 { DEFAULT_TEXTURE_SIZE } else { texture_size.clamp(64, MOST_TEXTURE_SIZE) };
        let mut scene = Scene::new(mesh, ModelSource { bytes: file, extension, mtl }, side, 0, Vec::new());
        scene.picked = picked;
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

    /// How bright the model's light is taken: 1 as it is, 0 to [`MOST_EXPOSURE`].
    pub fn model_exposure(&self) -> f32 {
        self.shared.lock().scene.as_ref().map_or(1.0, |s| s.exposure)
    }

    pub fn set_model_exposure(&self, exposure: f32) {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return };
        if exposure.is_finite() {
            scene.exposure = exposure.clamp(0.0, MOST_EXPOSURE);
        }
        self.shared.frame(&mut st);
    }

    /// The lights the model is lit by.
    pub fn model_lights(&self) -> Vec<ModelLight> {
        self.shared.lock().scene.as_ref().map_or_else(Vec::new, |s| s.lights.clone())
    }

    /// Lights the model by `lights` (the first [`MOST_LIGHTS`]).
    pub fn set_model_lights(&self, lights: Vec<ModelLight>) {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return };
        scene.lights = lights.into_iter().take(MOST_LIGHTS).map(ModelLight::sane).collect();
        self.shared.frame(&mut st);
    }

    /// Changes light `index` to `light` (its place, colour and intensity).
    pub fn set_model_light(&self, index: u32, light: ModelLight) {
        let mut st = self.shared.lock();
        let Some(slot) = st.scene.as_mut().and_then(|s| s.lights.get_mut(index as usize)) else { return };
        *slot = light.sane();
        self.shared.frame(&mut st);
    }

    /// Adds a white light above and behind the model; false when it has as many as it can take.
    pub fn add_model_light(&self) -> bool {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return false };
        if scene.lights.len() >= MOST_LIGHTS {
            return false;
        }
        let [x, y, z] = NEW_LIGHT;
        scene.lights.push(ModelLight { x, y, z, hue: 0.0, saturation: 0.0, intensity: DEFAULT_INTENSITY });
        self.shared.frame(&mut st);
        true
    }

    /// Adds a copy of light `index` beside it; false when there are as many as can be.
    pub fn duplicate_model_light(&self, index: u32) -> bool {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return false };
        let Some(light) = scene.lights.get(index as usize).copied() else { return false };
        if scene.lights.len() >= MOST_LIGHTS {
            return false;
        }
        let [dx, dy, dz] = DUPLICATE_OFFSET;
        scene.lights.push(ModelLight { x: light.x + dx, y: light.y + dy, z: light.z + dz, ..light });
        self.shared.frame(&mut st);
        true
    }

    pub fn delete_model_light(&self, index: u32) {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return };
        if (index as usize) < scene.lights.len() {
            scene.lights.remove(index as usize);
        }
        self.shared.frame(&mut st);
    }

    /// The channel painted on: 0 colour, 1 roughness, 2 metalness.
    pub fn model_channel(&self) -> u32 {
        self.shared.lock().scene.as_ref().map_or(0, |s| u32::try_from(s.channel).unwrap_or(0))
    }

    /// Paints on `channel` of the current texture set from now on (0 colour, 1 roughness, 2
    /// metalness): its layers are the colour's layers, each with its own pixels.
    pub fn set_model_channel(&self, channel: u32) {
        let mut st = self.shared.lock();
        st.choose_channel(channel as usize);
        self.shared.frame(&mut st);
    }

    /// Layer `id`'s picture in `channel` of the current texture set (as layer thumbnails are),
    /// for the channel tiles; none until it is drawn.
    pub fn channel_layer_thumbnail(
        &self,
        channel: u32,
        id: u64,
        max_side: u32,
    ) -> Option<crate::layers::ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let channel = channel as usize;
        let current = st.scene.as_ref().map(|s| s.channel)?;
        if channel == current {
            drop(guard);
            return self.layer_thumbnail(id, max_side);
        }
        let gpu = st.gpu.as_ref()?;
        let scene = st.scene.as_mut()?;
        let set = scene.slot(scene.current, channel)?.as_mut()?;
        if set.canvas.is_none() {
            let mut c = Canvas::new(gpu, &set.doc).ok()?;
            for (lid, pixels) in std::mem::take(&mut set.pending) {
                let _ = c.upload_layer(gpu, lid, &pixels);
            }
            set.canvas = Some(c);
        }
        let canvas = set.canvas.as_mut()?;
        let t = canvas.layer_thumbnail(gpu, &mut set.doc, LayerId(id), max_side).ok()?;
        Some(crate::layers::ThumbnailImage { width: t.width, height: t.height, rgba: t.rgba })
    }

    /// The images the model brings for its texture sets (a USDZ's packed images, or those
    /// picked or zipped with an OBJ or USD that its materials map): to be decoded and given back
    /// with [`CanvasSession::set_model_texture`]. Its normal and occlusion maps come as channels
    /// 3 and 4. Picked images are given once.
    pub fn model_texture_files(&self) -> Vec<ModelTextureFile> {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return Vec::new() };
        let picked = std::mem::take(&mut scene.picked);
        let zip = (scene.source.extension == "usdz")
            .then(|| procraft_engine::zip::ZipReader::new(&scene.source.bytes))
            .flatten();
        let found = |path: &str| match &zip {
            Some(zip) => packed_file(zip, path),
            None => companion(&picked, path).map(|f| f.bytes.clone()),
        };
        let mut out = Vec::new();
        for (set, part) in scene.mesh.parts.iter().enumerate() {
            let Some(m) = scene.mesh.materials.iter().find(|m| m.name == part.material) else { continue };
            let maps = [
                (0, m.color_map.as_deref(), None),
                (1, m.roughness_map.as_deref(), m.roughness_component),
                (2, m.metallic_map.as_deref(), m.metallic_component),
                (3, m.normal_map.as_deref(), None),
                (4, m.occlusion_map.as_deref(), m.occlusion_component),
            ];
            for (channel, map, component) in maps {
                let Some(bytes) = map.and_then(found) else { continue };
                out.push(ModelTextureFile {
                    texture_set: u32::try_from(set).unwrap_or(0),
                    channel,
                    component: component.and_then(|c| i32::try_from(c).ok()).unwrap_or(-1),
                    bytes,
                });
            }
        }
        out
    }

    /// Puts `rgba` (premultiplied, the texture size square) into texture set `texture_set`'s
    /// Base Layer in `channel` (0 colour, 1 roughness, 2 metalness), with no undo step; or, as
    /// channel 3 or 4, makes it the set's normal or occlusion map. False if it doesn't fit.
    pub fn set_model_texture(&self, texture_set: u32, channel: u32, rgba: Vec<u8>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (set, channel) = (texture_set as usize, channel as usize);
        let Some(scene) = st.scene.as_mut() else { return false };
        let side = scene.texture_size;
        if channel > 4 || set >= scene.mesh.parts.len() || rgba.len() != (side as usize) * (side as usize) * 4 {
            return false;
        }
        // the maps shown as they are
        if channel >= 3 {
            scene.surface_maps.resize(scene.mesh.parts.len(), [None, None]);
            if let Some(slot) = scene.surface_maps.get_mut(set).and_then(|m| m.get_mut(channel - 3)) {
                *slot = Some(Arc::new(rgba));
            }
            scene.surface_shown = false;
            self.shared.frame(st);
            return true;
        }
        // the colour's layers say which is the Base Layer
        let colour_is_session = set == scene.current && scene.channel == 0;
        let structure = if colour_is_session {
            st.doc.clone()
        } else {
            match scene.sets.get(set).and_then(Option::as_ref) {
                Some(s) => s.doc.clone(),
                None => return false,
            }
        };
        let Some(base) = structure.layers().first().map(|l| l.id) else { return false };
        if set == scene.current && channel == scene.channel {
            match st.canvas.as_mut() {
                Some(canvas) => canvas.put_layer(base, rgba),
                None => st.pending_pixels.push((base, crate::project::Opened::Pixels(rgba))),
            }
        } else {
            let Some(slot) = scene.slot(set, channel) else { return false };
            let target = slot.get_or_insert_with(|| {
                let mut made = TextureSet::empty(side);
                follow_layers(&mut made, &structure);
                made
            });
            match target.canvas.as_mut() {
                Some(canvas) => canvas.put_layer(base, rgba),
                None => target.pending.push((base, rgba)),
            }
        }
        self.shared.frame(st);
        true
    }

    /// Which of the model's submeshes are hidden, in order.
    pub fn submeshes_hidden(&self) -> Vec<bool> {
        let st = self.shared.lock();
        st.scene.as_ref().map_or_else(Vec::new, |s| {
            (0..s.mesh.submeshes.len()).map(|i| s.hidden.get(i).copied().unwrap_or(false)).collect()
        })
    }

    /// Hides submesh `index` (not drawn or painted), or shows it again.
    pub fn set_submesh_hidden(&self, index: u32, hidden: bool) {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return };
        let count = scene.mesh.submeshes.len();
        scene.hidden.resize(count, false);
        if let Some(h) = scene.hidden.get_mut(index as usize) {
            *h = hidden;
        }
        self.shared.frame(&mut st);
    }

    /// Submesh `index` on its own, seen as the model is now, filling a `side` square on nothing:
    /// its row's picture. None until the model has been drawn.
    pub fn submesh_thumbnail(&self, index: u32, side: u32) -> Option<crate::layers::ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let side = side.clamp(8, 1024);
        let (Some(gpu), Some(view)) = (st.gpu.as_ref(), st.scene.as_mut().and_then(|s| s.view.as_mut())) else {
            return None;
        };
        match view.submesh_picture(gpu, index as usize, side) {
            Ok(rgba) => Some(crate::layers::ThumbnailImage { width: side, height: side, rgba }),
            Err(e) => {
                log::warn!("submesh picture: {e}");
                None
            }
        }
    }

    /// Shows the Lighting Studio (the model on its floor among its lights) or the model again.
    pub fn set_lighting_studio(&self, open: bool) {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return };
        scene.studio_open = open;
        self.shared.frame(&mut st);
    }

    /// A drag of (`dx`, `dy`) window pixels across the studio: its camera goes round the model
    /// (across) and up or down (along), the model upright.
    pub fn studio_turn(&self, dx: f32, dy: f32) {
        let mut st = self.shared.lock();
        let side = st.window_size.0.min(st.window_size.1).max(1) as f32;
        let Some(scene) = st.scene.as_mut() else { return };
        let Some(view) = scene.view.as_ref() else { return };
        let orbit = scene.studio.get_or_insert_with(|| view.studio_orbit());
        let (yaw, pitch) = yaw_pitch(orbit);
        let yaw = yaw - dx / side * STUDIO_TURN_PER_SIDE;
        let pitch = (pitch + dy / side * STUDIO_TURN_PER_SIDE).clamp(STUDIO_LOWEST, STUDIO_HIGHEST);
        orbit.rotation = Orbit::rotation_of(yaw, pitch);
        self.shared.frame(&mut st);
    }

    /// Two fingers pinching the studio by `scale`: its camera nearer or farther.
    pub fn studio_zoom(&self, scale: f32) {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return };
        let unit = Orbit::unit(&scene.mesh);
        let Some(view) = scene.view.as_ref() else { return };
        let orbit = scene.studio.get_or_insert_with(|| view.studio_orbit());
        if scale.is_finite() && scale > 0.0 {
            orbit.distance = (orbit.distance / scale).clamp(NEAREST * unit * 20.0, FARTHEST * unit * 2.0);
        }
        self.shared.frame(&mut st);
    }

    /// The light whose cube is under window point (`x`, `y`) in the studio, the nearest if more.
    pub fn model_light_at(&self, x: f32, y: f32) -> Option<u32> {
        let st = self.shared.lock();
        let (w, h) = st.window_size;
        let scene = st.scene.as_ref()?;
        let view = scene.view.as_ref()?;
        let orbit = scene.studio.unwrap_or_else(|| view.studio_orbit());
        let gpu_view = scene.lights_view(view);
        (0..gpu_view.lights.len())
            .filter_map(|i| {
                let [lx, ly, reach] = gpu_view.light_on_screen(&orbit, i, w, h)?;
                let d = ((lx - x).powi(2) + (ly - y).powi(2)).sqrt();
                (d <= reach + LIGHT_SLACK).then_some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .and_then(|(i, _)| u32::try_from(i).ok())
    }

    /// Light `index` in the studio: its middle on the window and how far its cube reaches (pixels).
    pub fn model_light_on_screen(&self, index: u32) -> Option<Vec<f32>> {
        let st = self.shared.lock();
        let (w, h) = st.window_size;
        let scene = st.scene.as_ref()?;
        let view = scene.view.as_ref()?;
        let orbit = scene.studio.unwrap_or_else(|| view.studio_orbit());
        scene.lights_view(view).light_on_screen(&orbit, index as usize, w, h).map(|p| p.to_vec())
    }

    /// Drags light `index` by (`dx`, `dy`) window pixels across the studio, at its depth.
    pub fn move_model_light(&self, index: u32, dx: f32, dy: f32) {
        let mut st = self.shared.lock();
        let (w, h) = st.window_size;
        let Some(scene) = st.scene.as_mut() else { return };
        let Some(view) = scene.view.as_ref() else { return };
        let orbit = scene.studio.unwrap_or_else(|| view.studio_orbit());
        let mut moved = scene.lights_view(view);
        moved.move_light(&orbit, index as usize, dx, dy, w, h);
        if let (Some(light), Some(m)) = (scene.lights.get_mut(index as usize), moved.lights.get(index as usize)) {
            [light.x, light.y, light.z] = m.place;
        }
        self.shared.frame(&mut st);
    }

    /// Whether the environment shows behind the model (else the canvas's surround).
    pub fn model_environment_shown(&self) -> bool {
        self.shared.lock().scene.as_ref().is_none_or(|s| s.environment_shown)
    }

    pub fn set_model_environment_shown(&self, shown: bool) {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return };
        scene.environment_shown = shown;
        self.shared.frame(&mut st);
    }

    pub fn set_paint_through(&self, on: bool) {
        if let Some(scene) = self.shared.lock().scene.as_mut() {
            scene.paint_through = on;
        }
    }

    /// Whether the texture set painted on is shown flat (its 2D texture) in place of the model.
    pub fn model_flat(&self) -> bool {
        self.shared.lock().scene.as_ref().is_some_and(|s| s.flat)
    }

    /// Shows the texture set painted on flat, a canvas drawn and painted on as any other, or the
    /// model again.
    pub fn set_model_flat(&self, on: bool) {
        let mut st = self.shared.lock();
        let Some(flat) = st.scene.as_ref().map(|s| s.flat) else { return };
        if flat == on {
            return;
        }
        // the stroke under way ends where it was drawn
        if flat {
            st.finish_stroke();
        } else {
            st.scene_stroke_cancel();
        }
        if let Some(scene) = st.scene.as_mut() {
            scene.flat = on;
        }
        if on {
            st.view = Some(st.fitted_view());
        }
        self.shared.frame(&mut st);
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
        let materials: Vec<(&str, &PartImages)> = files.names.iter().map(String::as_str).zip(&files.images).collect();
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add(
            &format!("{base}.obj"),
            procraft_engine::mesh::write_obj(&files.mesh, &format!("{base}.mtl"), &files.names).as_bytes(),
        );
        zip.add(&format!("{base}.mtl"), procraft_engine::mesh::write_mtl(&materials).as_bytes());
        for (file, png) in files.images.iter().flat_map(PartImages::files) {
            zip.add(file, png);
        }
        let bytes = zip.finish();
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("exporting the model", e)))
    }

    /// Writes the model as a glTF binary at `path`, for a viewer that shows it in the room: as
    /// for [`CanvasSession::export_model_usdz`], each set's roughness and metalness in one image
    /// (green and blue).
    pub fn export_model_glb(&self, path: String) -> Result<(), MobileError> {
        let ModelPixels { mesh, names, pixels, maps, side, .. } = self.model_pixels()?;
        let png = |px: &[u8]| crate::project::rgba_png(side, side, px);
        let encoded: Vec<[Vec<u8>; 4]> = pixels
            .iter()
            .enumerate()
            .map(|(part, [colour, roughness, metallic])| {
                let mut both = roughness.clone();
                for (p, m) in both.as_chunks_mut::<4>().0.iter_mut().zip(metallic.as_chunks::<4>().0) {
                    *p = [255, p[1], m[2], 255];
                }
                let [normal, occlusion] = maps.get(part).cloned().unwrap_or_default();
                let map = |m: Option<Arc<Vec<u8>>>| m.map_or_else(Vec::new, |px| png(&px));
                [png(colour), png(&both), map(normal), map(occlusion)]
            })
            .collect();
        let parts: Vec<procraft_engine::gltf::GlbPart<'_>> = names
            .iter()
            .zip(&encoded)
            .map(|(name, [colour, both, normal, occlusion])| procraft_engine::gltf::GlbPart {
                name,
                color: colour,
                metallic_roughness: both,
                normal: (!normal.is_empty()).then_some(normal.as_slice()),
                occlusion: (!occlusion.is_empty()).then_some(occlusion.as_slice()),
            })
            .collect();
        let bytes = procraft_engine::gltf::write_glb(&mesh, &parts);
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("exporting the model", e)))
    }

    /// Writes the model as a USDZ package at `path`: the mesh with a preview-surface material
    /// for each texture set, reading its images (as for [`CanvasSession::export_model_obj`]).
    pub fn export_model_usdz(&self, path: String) -> Result<(), MobileError> {
        let files = self.model_files()?;
        let bytes = procraft_engine::usd::write_usdz(&files.mesh, &files.images);
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("exporting the model", e)))
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Each texture set's colour, roughness and metalness (and its normal and occlusion maps, if
    /// it has them) as PNGs in folder `dir` (made if missing; as for
    /// [`CanvasSession::export_model_obj`]); returns the files' paths.
    pub fn export_model_textures(&self, dir: String) -> Result<Vec<String>, MobileError> {
        let files = self.model_files()?;
        let dir = Path::new(&dir);
        fs::create_dir_all(dir).map_err(|e| file_error("making the folder", e))?;
        let mut out = Vec::new();
        for (name, images) in files.names.iter().zip(&files.images) {
            let [colour, roughness, metallic] = images.maps.each_ref().map(|(_, png)| png);
            let named = [("Color", colour), ("Roughness", roughness), ("Metallic", metallic)]
                .into_iter()
                .chain(images.normal.iter().map(|(_, png)| ("Normal", png)))
                .chain(images.occlusion.iter().map(|(_, png)| ("AmbientOcclusion", png)));
            for (channel, png) in named {
                let path = dir.join(format!("{name}-{channel}.png"));
                write_atomically(&path, |p| fs::write(p, png).map_err(|e| file_error("exporting the textures", e)))?;
                out.push(path.to_string_lossy().into_owned());
            }
        }
        Ok(out)
    }

    /// The model as the window shows it, at the window's size, as a PNG at `path`: on the
    /// canvas's surround, or with `transparent`, on nothing.
    pub fn export_model_render(&self, path: String, transparent: bool) -> Result<(), MobileError> {
        let (w, h, rgba) = self.model_picture(transparent)?;
        let png = crate::project::rgba_png(w, h, &rgba);
        write_atomically(Path::new(&path), |p| fs::write(p, &png).map_err(|e| file_error("rendering the model", e)))
    }

    /// The model as the window shows it, on the canvas's surround, as a TIFF at `path`.
    pub fn export_model_tiff(&self, path: String) -> Result<(), MobileError> {
        let (w, h, rgba) = self.model_picture(false)?;
        let bytes = procraft_engine::formats::tiff(w, h, 72.0, &[], &rgba);
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("rendering the model", e)))
    }
}

impl CanvasSession {
    /// The model as the window shows it, at the window's size: its width, height and pixels
    /// (premultiplied RGBA8), on the canvas's surround or, `transparent`, on nothing.
    fn model_picture(&self, transparent: bool) -> Result<(u32, u32, Vec<u8>), MobileError> {
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
        Ok((w, h, rgba))
    }
}

/// How far a turntable's swing goes each way (radians).
const SWING: f32 = std::f32::consts::PI / 6.0;
/// The turntable's camera: this far above level (radians) and back (framing units), the model
/// about four fifths of the picture's height; at its farthest it is this much further back for
/// each share of Zoom distance (the model then about 1.78 times smaller at 100 %).
const TURNTABLE_PITCH: f32 = 0.1;
const TURNTABLE_DISTANCE: f32 = 6.5;
const TURNTABLE_PULL: f32 = 0.65;

/// How a model's turntable goes: once round (or swinging each way and back) every `seconds`,
/// the camera pulled back by up to `zoom` (0 to 1) half way through, the turn eased in and out at
/// the loop by `ease` (0 to 1), the environment behind it or not.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Turntable {
    pub swing: bool,
    pub seconds: f32,
    pub zoom: f32,
    pub ease: f32,
    pub environment: bool,
}

impl Turntable {
    /// The camera at `phase` (0 to 1) through the loop.
    fn orbit(&self, view: &MeshView, phase: f32) -> Orbit {
        let tau = std::f32::consts::TAU;
        let p = phase.rem_euclid(1.0);
        let ease = self.ease.clamp(0.0, 1.0);
        // slowed to a stop at the loop's ends by the ease
        let eased = p - ease * (tau * p).sin() / tau;
        let yaw = if self.swing { SWING * (tau * eased).sin() } else { tau * eased };
        let pull = (1.0 - (tau * p).cos()) / 2.0;
        let distance = TURNTABLE_DISTANCE * (1.0 + TURNTABLE_PULL * self.zoom.clamp(0.0, 1.0) * pull);
        view.upright_orbit(yaw, TURNTABLE_PITCH, distance)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Frame `index` of `count` of the model's turntable as `turntable` sets it, at `width` ×
    /// `height`: RGBA8, or with `bgra` opaque blue, green, red, alpha (an ARGB integer each,
    /// little-endian, ready for a video frame). None if the model isn't shown yet.
    pub fn model_turntable_picture(
        &self,
        index: u32,
        count: u32,
        turntable: Turntable,
        width: u32,
        height: u32,
        bgra: bool,
    ) -> Option<crate::layers::ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (w, h) = (width.max(1), height.max(1));
        let (Some(gpu), Some(view)) = (st.gpu.as_ref(), st.scene.as_mut().and_then(|s| s.view.as_mut())) else {
            return None;
        };
        let orbit = turntable.orbit(view, index as f32 / count.max(1) as f32);
        let s = crate::SURROUND;
        let clear = [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0];
        match view.picture_from(gpu, orbit, w, h, clear, turntable.environment) {
            Ok(mut rgba) => {
                if bgra {
                    for p in rgba.as_chunks_mut::<4>().0 {
                        *p = [p[2], p[1], p[0], 255];
                    }
                }
                Some(crate::layers::ThumbnailImage { width: w, height: h, rgba })
            }
            Err(e) => {
                log::warn!("turntable picture: {e}");
                None
            }
        }
    }

    /// The model's turntable as a looping GIF at `path`: `fps` frames a second for its
    /// `turntable.seconds`, `width` × `height`.
    pub fn export_model_gif(
        &self,
        path: String,
        turntable: Turntable,
        width: u32,
        height: u32,
        fps: f32,
    ) -> Result<(), MobileError> {
        let count = turntable_frames(turntable, fps);
        let delay = (100.0 / fps.clamp(1.0, 100.0)).round() as u16;
        let mut gif: Option<procraft_engine::formats::GifWriter> = None;
        for i in 0..count {
            let t = self
                .model_turntable_picture(i, count, turntable, width, height, false)
                .ok_or_else(|| file_error("saving", "the model is not shown yet"))?;
            gif.get_or_insert_with(|| procraft_engine::formats::GifWriter::new(t.width, t.height)).frame(
                &t.rgba,
                delay.max(1),
                true,
            );
        }
        let bytes = gif.ok_or_else(|| file_error("saving", "there are no frames"))?.finish();
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))
    }

    /// The model's turntable as a looping animated PNG at `path` (as for GIF).
    pub fn export_model_apng(
        &self,
        path: String,
        turntable: Turntable,
        width: u32,
        height: u32,
        fps: f32,
    ) -> Result<(), MobileError> {
        let count = turntable_frames(turntable, fps);
        let delay = (1000.0 / fps.clamp(1.0, 1000.0)).round().clamp(1.0, 65535.0) as u16;
        let (w, h) = (width.max(1), height.max(1));
        write_atomically(Path::new(&path), |f| {
            let out = fs::File::create(f).map_err(|e| file_error("saving", e))?;
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(out), w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            encoder.set_animated(count, 0).map_err(|e| file_error("saving", e))?;
            encoder.set_frame_delay(delay, 1000).map_err(|e| file_error("saving", e))?;
            let mut writer = encoder.write_header().map_err(|e| file_error("saving", e))?;
            for i in 0..count {
                let t = self
                    .model_turntable_picture(i, count, turntable, w, h, false)
                    .ok_or_else(|| file_error("saving", "the model is not shown yet"))?;
                writer.write_image_data(&t.rgba).map_err(|e| file_error("saving", e))?;
            }
            writer.finish().map_err(|e| file_error("saving", e))
        })
    }
}

/// How many frames a turntable takes at `fps`: its seconds' worth, at least one.
fn turntable_frames(turntable: Turntable, fps: f32) -> u32 {
    (turntable.seconds.clamp(0.1, 60.0) * fps.clamp(1.0, 120.0)).round().max(1.0) as u32
}

/// What a model's exports are made of: the model, its texture sets' names (safe as file names,
/// different from each other), each set's images as PNGs (its colour, roughness and metalness
/// over the part's own, as the model shows them, and its normal and occlusion maps) named as
/// models name them, and the artwork's name.
struct ModelFiles {
    mesh: Mesh,
    names: Vec<String>,
    images: Vec<PartImages>,
    base: String,
}

/// The images as a model's exports name them (`<set>-<channel>.png`).
const MODEL_CHANNEL_FILES: [&str; 3] = ["color", "roughness", "metallic"];
const MODEL_NORMAL_FILE: &str = "normal";
const MODEL_OCCLUSION_FILE: &str = "ambient_occlusion";

impl CanvasSession {
    /// The model's texture sets' pixels as exports take them: each set's colour, roughness and
    /// metalness over the part's own (RGBA8, the texture size square), with its maps and the
    /// sets' names safe as file names.
    fn model_pixels(&self) -> Result<ModelPixels, MobileError> {
        let (mesh, set_names, pictures, maps, side, base) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let pictures = st.channel_pictures()?;
            let Some(scene) = st.scene.as_ref() else { return Err(file_error("exporting the model", "no model")) };
            let maps = scene.surface_maps.clone();
            (scene.mesh.clone(), scene.set_names(), pictures, maps, scene.texture_size, file_stem(&st.name, "Model"))
        };
        let mut names: Vec<String> = Vec::new();
        for name in set_names {
            let mut n = file_stem(&name, "Material");
            if names.contains(&n) {
                n = format!("{n}_{}", names.len() + 1);
            }
            names.push(n);
        }
        let pixels = pictures
            .into_iter()
            .enumerate()
            .map(|(part, [color, roughness, metallic])| {
                let (under, r, m) = mesh.surface(part);
                [
                    flattened(side, color, under, false),
                    flattened(side, roughness, [r; 3], true),
                    flattened(side, metallic, [m; 3], true),
                ]
            })
            .collect();
        Ok(ModelPixels { mesh, names, pixels, maps, side, base })
    }

    fn model_files(&self) -> Result<ModelFiles, MobileError> {
        let ModelPixels { mesh, names, pixels, maps, side, base } = self.model_pixels()?;
        let png = |px: &[u8]| crate::project::rgba_png(side, side, px);
        let images = pixels
            .iter()
            .zip(&names)
            .enumerate()
            .map(|(part, ([cp, rp, mp], n))| {
                let [c, r, m] = MODEL_CHANNEL_FILES.map(|k| format!("{n}-{k}.png"));
                let [normal, occlusion] = maps.get(part).cloned().unwrap_or_default();
                let map = |rgba: Option<Arc<Vec<u8>>>, k: &str| rgba.map(|px| (format!("{n}-{k}.png"), png(&px)));
                PartImages {
                    maps: [(c, png(cp)), (r, png(rp)), (m, png(mp))],
                    normal: map(normal, MODEL_NORMAL_FILE),
                    occlusion: map(occlusion, MODEL_OCCLUSION_FILE),
                }
            })
            .collect();
        Ok(ModelFiles { mesh, names, images, base })
    }
}

/// What [`CanvasSession::model_pixels`] gives.
struct ModelPixels {
    mesh: Mesh,
    names: Vec<String>,
    pixels: Vec<[Vec<u8>; 3]>,
    maps: Vec<[Option<Arc<Vec<u8>>>; 2]>,
    side: u32,
    base: String,
}

/// A channel's picture (premultiplied RGBA8, `side` square; none if never painted) over what the
/// part shows under it (`under`, 0 to 1), opaque: as it is, or as the grey the model reads from
/// it.
fn flattened(side: u32, picture: Option<Vec<u8>>, under: [f32; 3], grey: bool) -> Vec<u8> {
    let under = under.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u16);
    let mut px = picture.unwrap_or_else(|| vec![0; (side as usize) * (side as usize) * 4]);
    let [ur, ug, ub] = under;
    for p in px.as_chunks_mut::<4>().0 {
        let [r, g, b, a] = *p;
        let clear = 255 - u16::from(a);
        let over = |c: u8, u: u16| (u16::from(c) + (u * clear + 127) / 255).min(255) as u8;
        let [r, g, b] = [over(r, ur), over(g, ug), over(b, ub)];
        *p = if grey {
            // the model's own weights for a painted grey
            let v = (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)).round() as u8;
            [v, v, v, 255]
        } else {
            [r, g, b, 255]
        };
    }
    px
}

/// A file packed in a USDZ by the path its layer gives (relative to the package; `./` and folders
/// allowed for), else by its name alone.
fn packed_file(zip: &procraft_engine::zip::ZipReader<'_>, path: &str) -> Option<Vec<u8>> {
    let path = path.trim_start_matches("./");
    if let Some(bytes) = zip.read(path) {
        return Some(bytes);
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let found = zip.names().find(|n| n.rsplit('/').next() == Some(name))?.to_owned();
    zip.read(&found)
}

/// A name made safe as part of a file name: letters, digits, `-` and `_` kept, the rest `_`.
fn file_stem(name: &str, otherwise: &str) -> String {
    let kept: String =
        name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if kept.is_empty() { otherwise.to_owned() } else { kept }
}

impl State {
    /// Each texture set's colour, roughness and metalness as painted (premultiplied RGBA8, the
    /// texture size square), drawn now: none for a channel never painted.
    fn channel_pictures(&mut self) -> Result<Vec<[Option<Vec<u8>>; 3]>, MobileError> {
        let failed = |e: String| file_error("reading the model's textures", e);
        let gpu = self.gpu.as_ref().ok_or_else(|| failed("the model is not shown yet".to_owned()))?;
        let Some(scene) = self.scene.as_mut() else { return Ok(Vec::new()) };
        let (current, painted) = (scene.current, scene.channel);
        let mut out = Vec::new();
        for part in 0..scene.mesh.parts.len() {
            let mut pictures: [Option<Vec<u8>>; 3] = [None, None, None];
            for (channel, picture) in pictures.iter_mut().enumerate() {
                // the channel painted on is the session's; the others wait in the scene
                *picture = if part == current && channel == painted {
                    let canvas = self.canvas.as_mut().ok_or_else(|| failed("the model is not shown yet".to_owned()))?;
                    canvas.render_finished(gpu, &mut self.doc);
                    Some(canvas.read_composite(gpu).map_err(|e| failed(e.to_string()))?)
                } else {
                    match scene.slot(part, channel).and_then(Option::as_mut) {
                        Some(set) => Some(waiting_picture(gpu, set).map_err(failed)?),
                        None => None,
                    }
                };
            }
            out.push(pictures);
        }
        Ok(out)
    }
}

/// A waiting texture set's picture, drawn now (its canvas made first if the model hasn't been
/// drawn since it was opened).
fn waiting_picture(gpu: &procraft_gpu::Gpu, set: &mut TextureSet) -> Result<Vec<u8>, String> {
    let canvas = match set.canvas.take() {
        Some(c) => c,
        None => {
            let mut c = Canvas::new(gpu, &set.doc).map_err(|e| e.to_string())?;
            for (id, pixels) in std::mem::take(&mut set.pending) {
                c.upload_layer(gpu, id, &pixels).map_err(|e| e.to_string())?;
            }
            c
        }
    };
    let canvas = set.canvas.insert(canvas);
    canvas.render_finished(gpu, &mut set.doc);
    canvas.read_composite(gpu).map_err(|e| e.to_string())
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exposure: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    environment_shown: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lights: Option<Vec<ModelLight>>,
    /// The submeshes hidden, by place.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hidden: Vec<usize>,
    /// Each part's roughness and metalness documents, by part, if made.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    channels: Vec<[Option<Document>; 2]>,
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
    /// The waiting texture sets' layers, by part, and the parts' channels' (part, channel 1 or 2).
    layers: Vec<(usize, Pixels)>,
    channel_layers: Vec<(usize, usize, Pixels)>,
    /// The parts' normal and occlusion maps (part, its file's name, RGBA8).
    maps: Vec<(usize, &'static str, Arc<Vec<u8>>)>,
    /// The model as last shown (premultiplied RGBA8, its size), for the gallery.
    pub(crate) picture: Option<(u32, u32, Vec<u8>)>,
}

/// What the last saves of a model wrote: a hash of each waiting texture set's layers, by part,
/// channel (0 its colour) and layer.
pub(crate) type SavedSets = HashMap<(usize, usize, LayerId), u64>;

/// A waiting texture set's layers' pixels: read from its canvas, or as opened if not drawn yet.
fn set_pixels(set: &mut TextureSet, gpu: Option<&procraft_gpu::Gpu>) -> Result<Pixels, MobileError> {
    match (set.canvas.as_mut(), gpu) {
        (Some(canvas), Some(gpu)) => {
            canvas.render(gpu, &mut set.doc, None);
            let mut pixels = Vec::new();
            for layer in set.doc.layers() {
                let rgba = canvas.read_layer(gpu, layer.id).map_err(|e| file_error("reading a layer", e))?;
                pixels.push((layer.id, rgba));
            }
            Ok(pixels)
        }
        _ => Ok(set.pending.clone()),
    }
}

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
            let pixels = set_pixels(set, gpu)?;
            docs.push(Some(set.doc.clone()));
            layers.push((i, pixels));
        }
        // the channels (saved with the colour painted on, so none is the session's own)
        let mut channels = Vec::new();
        let mut channel_layers = Vec::new();
        for (part, pair) in scene.channels.iter_mut().enumerate() {
            let mut docs_of = [None, None];
            for (k, set) in pair.iter_mut().enumerate() {
                let Some(set) = set else { continue };
                channel_layers.push((part, k + 1, set_pixels(set, gpu)?));
                if let Some(d) = docs_of.get_mut(k) {
                    *d = Some(set.doc.clone());
                }
            }
            channels.push(docs_of);
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
                exposure: (scene.exposure != 1.0).then_some(scene.exposure),
                environment_shown: (!scene.environment_shown).then_some(false),
                lights: Some(scene.lights.clone()),
                hidden: scene.hidden.iter().enumerate().filter(|(_, h)| **h).map(|(i, _)| i).collect(),
                channels,
            },
            source: scene.source.clone(),
            layers,
            channel_layers,
            maps: scene
                .surface_maps
                .iter()
                .enumerate()
                .flat_map(|(part, maps)| {
                    maps.iter().zip(MAP_FILES).filter_map(move |(m, name)| m.as_ref().map(|m| (part, name, m.clone())))
                })
                .collect(),
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
    // the maps, written once: they don't change
    for (part, name, rgba) in &model.maps {
        let path = dir.join("maps").join(format!("{part}-{name}.png"));
        if !path.exists() {
            fs::create_dir_all(dir.join("maps")).map_err(|e| file_error("making the folder", e))?;
            write_atomically(&path, |p| write_png(p, side, side, rgba, &[]))?;
        }
    }
    let colour = model.layers.iter().map(|(part, pixels)| (*part, 0, pixels));
    let channels = model.channel_layers.iter().map(|(part, k, pixels)| (*part, *k, pixels));
    for (part, channel, pixels) in colour.chain(channels) {
        let mut folder = dir.join("sets").join(part.to_string());
        if channel > 0 {
            folder = folder.join(format!("c{channel}"));
        }
        fs::create_dir_all(&folder).map_err(|e| file_error("making the folder", e))?;
        for (id, rgba) in pixels {
            let hash = pixels_hash(rgba);
            let path = folder.join(format!("{}.png", id.0));
            if saved.get(&(part, channel, *id)) == Some(&hash) && path.exists() {
                continue;
            }
            write_atomically(&path, |p| write_png(p, side, side, rgba, &[]))?;
            saved.insert((part, channel, *id), hash);
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
    saved.retain(|(part, channel, id), _| {
        let has = |px: &Pixels| px.iter().any(|(i, _)| i == id);
        if *channel == 0 {
            model.layers.iter().any(|(p, px)| p == part && has(px))
        } else {
            model.channel_layers.iter().any(|(p, k, px)| p == part && k == channel && has(px))
        }
    });
    Ok(())
}

/// The names of a part's normal and occlusion maps' files in the artwork's `maps` folder
/// (`<part>-<name>.png`).
const MAP_FILES: [&str; 2] = ["normal", "occlusion"];

/// The layers of `doc` saved in `folder` (each `<id>.png`, `side` square); a layer with no file is
/// empty.
fn read_set_layers(folder: &Path, doc: &Document, side: u32) -> Pixels {
    let mut pending = Vec::new();
    for layer in doc.layers() {
        let path = folder.join(format!("{}.png", layer.id.0));
        if !path.exists() {
            continue;
        }
        match read_png(&path, side, side) {
            Ok(rgba) => pending.push((layer.id, rgba)),
            Err(e) => log::error!("{} layer {:?}: {e}", folder.display(), layer.id),
        }
    }
    pending
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
            let pending = read_set_layers(&dir.join("sets").join(part.to_string()), &doc, side);
            Some(TextureSet { doc, canvas: None, pending })
        })
        .collect();
    let channels = saved
        .channels
        .iter()
        .enumerate()
        .map(|(part, pair)| {
            let mut out: [Option<TextureSet>; 2] = [None, None];
            for (k, doc) in pair.iter().enumerate() {
                let Some(doc) =
                    doc.as_ref().map(|d| d.clone().repaired()).filter(|d| (d.width(), d.height()) == (side, side))
                else {
                    continue;
                };
                let folder = dir.join("sets").join(part.to_string()).join(format!("c{}", k + 1));
                let pending = read_set_layers(&folder, &doc, side);
                if let Some(slot) = out.get_mut(k) {
                    *slot = Some(TextureSet { doc, canvas: None, pending });
                }
            }
            out
        })
        .collect();
    let mut scene = Scene::new(mesh, ModelSource { bytes, extension, mtl }, side, saved.current, waiting);
    scene.paint_through = saved.paint_through;
    scene.environment_name = saved.environment.clone();
    scene.exposure = saved.exposure.filter(|e| e.is_finite()).map_or(1.0, |e| e.clamp(0.0, MOST_EXPOSURE));
    scene.environment_shown = saved.environment_shown.unwrap_or(true);
    if let Some(lights) = &saved.lights {
        scene.lights = lights.iter().take(MOST_LIGHTS).map(|l| l.sane()).collect();
    }
    scene.hidden = (0..scene.mesh.submeshes.len()).map(|i| saved.hidden.contains(&i)).collect();
    scene.channels = channels;
    scene.surface_maps = (0..scene.mesh.parts.len())
        .map(|part| {
            MAP_FILES.map(|name| {
                let path = dir.join("maps").join(format!("{part}-{name}.png"));
                path.exists().then(|| read_png(&path, side, side).ok()).flatten().map(Arc::new)
            })
        })
        .collect();
    scene.orbit = saved.orbit.map(|o| Orbit {
        target: o.target,
        rotation: o.rotation.unwrap_or_else(|| Orbit::rotation_of(o.yaw, o.pitch)),
        distance: o.distance,
        fov: o.fov_y,
    });
    Ok(scene)
}

impl Scene {
    /// Where part `part`'s `channel` document waits (0 colour, 1 roughness, 2 metalness).
    fn slot(&mut self, part: usize, channel: usize) -> Option<&mut Option<TextureSet>> {
        if channel == 0 {
            return self.sets.get_mut(part);
        }
        let count = self.mesh.parts.len();
        if self.channels.len() < count {
            self.channels.resize_with(count, || [None, None]);
        }
        self.channels.get_mut(part).and_then(|c| c.get_mut(channel - 1))
    }
}

/// `set`'s document made to have `structure`'s layers (their ids, order and settings, with its
/// own pixels): layers new to it start empty, and its undo history goes if the layers changed.
fn follow_layers(set: &mut TextureSet, structure: &Document) {
    let ids = |d: &Document| d.layers().iter().map(|l| (l.id, l.kind, l.parent)).collect::<Vec<_>>();
    let changed = ids(&set.doc) != ids(structure);
    let had: Vec<LayerId> = set.doc.layers().iter().map(|l| l.id).collect();
    let side = set.doc.width();
    let mut doc = Document::from_layers(side, side, structure.layers().to_vec(), structure.current());
    doc.background_visible = false;
    set.doc = doc;
    if !changed {
        return;
    }
    set.pending.retain(|(id, _)| had.contains(id) && structure.layer(*id).is_some());
    if let Some(canvas) = set.canvas.as_mut() {
        for l in structure.layers().iter().filter(|l| !had.contains(&l.id)) {
            canvas.wipe_layer(l.id);
        }
        canvas.forget_history();
    }
}

impl State {
    /// Paints on `channel` of the current part from now on: its document and canvas become the
    /// session's, with the layers of the document painted on until now.
    pub(crate) fn choose_channel(&mut self, channel: usize) {
        if channel > 2 || self.scene.as_ref().is_none_or(|s| s.channel == channel) {
            return;
        }
        if self.scene_shown() {
            self.scene_stroke_cancel();
        } else {
            self.finish_stroke();
        }
        self.settle();
        let side = self.doc.width();
        let Some(scene) = self.scene.as_mut() else { return };
        let (part, from) = (scene.current, scene.channel);
        let old = TextureSet {
            doc: std::mem::replace(&mut self.doc, Document::new(1, 1)),
            canvas: self.canvas.take(),
            pending: crate::project::opened_pixels(std::mem::take(&mut self.pending_pixels), side, side),
        };
        let structure = old.doc.clone();
        if let Some(slot) = scene.slot(part, from) {
            *slot = Some(old);
        }
        let mut new = scene.slot(part, channel).and_then(Option::take).unwrap_or_else(|| TextureSet::empty(side));
        follow_layers(&mut new, &structure);
        self.doc = new.doc;
        self.canvas = new.canvas;
        self.pending_pixels = crate::project::opened(new.pending);
        scene.channel = channel;
        self.ensure_canvas();
    }

    /// Runs `f` with the colour of the current part as the session's document (as saves need),
    /// the channel painted on put back after.
    pub(crate) fn with_colour<T>(&mut self, f: impl FnOnce(&mut State) -> T) -> T {
        let channel = self.scene.as_ref().map_or(0, |s| s.channel);
        self.choose_channel(0);
        let out = f(self);
        self.choose_channel(channel);
        out
    }

    /// Whether a model is shown (and strokes and gestures go to it), not a flat canvas: a model
    /// open and not showing its 2D texture.
    pub(crate) fn scene_shown(&self) -> bool {
        self.scene.as_ref().is_some_and(|s| !s.flat)
    }

    /// Makes texture set `index` the current one: the session's document and canvas.
    pub(crate) fn choose_texture_set(&mut self, index: usize) {
        if self.scene.as_ref().is_none_or(|s| index == s.current || index >= s.sets.len()) {
            return;
        }
        self.choose_channel(0);
        self.scene_stroke_cancel();
        self.settle();
        let Some(scene) = self.scene.as_mut() else { return };
        let Some(set) = scene.sets.get_mut(index).and_then(Option::take) else { return };
        let (w, h) = (self.doc.width(), self.doc.height());
        let pending = std::mem::replace(&mut self.pending_pixels, crate::project::opened(set.pending));
        let old = TextureSet {
            doc: std::mem::replace(&mut self.doc, set.doc),
            canvas: std::mem::replace(&mut self.canvas, set.canvas),
            pending: crate::project::opened_pixels(pending, w, h),
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
                scene.surface_shown = false;
            }
            // the waiting texture sets' canvases (and channels), with the pixels they opened with
            for set in scene.sets.iter_mut().flatten().chain(scene.channels.iter_mut().flatten().flatten()) {
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
        let grid = self.surround_grid(Vec2::ZERO);
        let (Some(gpu), Some(scene)) = (self.gpu.as_ref(), self.scene.as_mut()) else { return };
        // a new environment: prefiltered now, shown behind the model unless that's turned off
        if let (Some(view), Some(panorama)) = (scene.view.as_mut(), scene.environment.take()) {
            view.set_lighting(gpu, Lighting::new(gpu, &panorama));
            scene.lit = true;
        }
        if let Some(view) = scene.view.as_mut() {
            view.show_environment(gpu, scene.lit && scene.environment_shown);
            view.set_exposure(scene.exposure);
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.render(gpu, &mut self.doc, None);
        }
        let Some(view) = scene.view.as_mut() else { return };
        view.set_convert(convert);
        view.set_grid(Some(grid));
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("3D view") });
        // each set's texture, its smaller levels made again when its picture has changed (the
        // current part's colour is the session's own unless a channel is painted on)
        scene.filtered.resize(scene.sets.len(), None);
        let painting_colour = scene.channel == 0;
        for (i, (set, filtered)) in scene.sets.iter().zip(scene.filtered.iter_mut()).enumerate() {
            let canvas = if i == scene.current && painting_colour {
                self.canvas.as_ref()
            } else {
                set.as_ref().and_then(|s| s.canvas.as_ref())
            };
            if let Some(c) = canvas
                && *filtered != Some((c.composites(), i == scene.current))
            {
                view.set_albedo_filtered(gpu, &mut encoder, i, c.composite_view());
                *filtered = Some((c.composites(), i == scene.current));
            }
        }
        // each set's roughness and metalness, as painted
        scene.channel_shown.resize(scene.mesh.parts.len(), [None, None]);
        for part in 0..scene.mesh.parts.len() {
            for k in 1..=2usize {
                let session_own = part == scene.current && scene.channel == k;
                let canvas = if session_own {
                    self.canvas.as_ref()
                } else {
                    scene
                        .channels
                        .get(part)
                        .and_then(|c| c.get(k - 1))
                        .and_then(|s| s.as_ref())
                        .and_then(|s| s.canvas.as_ref())
                };
                let key = canvas.map(|c| (c.composites(), session_own));
                let Some(shown) = scene.channel_shown.get_mut(part).and_then(|c| c.get_mut(k - 1)) else { continue };
                if *shown != key {
                    view.set_channel_view(gpu, part, k, canvas.map(Canvas::composite_view));
                    *shown = key;
                }
            }
        }
        // the model's own normal and occlusion maps, given again when they change
        if !scene.surface_shown {
            let side = scene.texture_size;
            for (part, maps) in scene.surface_maps.iter().enumerate() {
                for (map, kind) in
                    maps.iter().zip([procraft_gpu::SurfaceMap::Normal, procraft_gpu::SurfaceMap::Occlusion])
                {
                    view.set_surface_map(gpu, part, kind, map.as_deref().map(|m| (side, m.as_slice())));
                }
            }
            scene.surface_shown = true;
        }
        view.lights = scene.lights.iter().map(|l| l.to_gpu()).collect();
        for i in 0..scene.mesh.submeshes.len() {
            view.set_hidden(i, scene.hidden.get(i).copied().unwrap_or(false));
        }
        let s = crate::SURROUND;
        let clear = [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0];
        let studio = scene.studio_open.then(|| *scene.studio.get_or_insert_with(|| view.studio_orbit()));
        match studio {
            Some(orbit) => view.render_studio(gpu, &mut encoder, target, size.0, size.1, clear, orbit),
            None => view.render(gpu, &mut encoder, target, size.0, size.1, clear),
        }
        gpu.queue.submit([encoder.finish()]);
        // frosted panels over the model: it drawn again, small, and blurred under each
        if let Some(canvas) = self.canvas.as_mut().filter(|_| !self.backdrops.is_empty()) {
            let backdrop = procraft_gpu::Backdrop {
                regions: &self.backdrops,
                sigma: crate::BACKDROP_SIGMA_PT * self.display_scale,
            };
            canvas.frost_over(gpu, target, format, size, s, backdrop, &mut |encoder, small, (w, h)| match studio {
                Some(orbit) => view.render_studio(gpu, encoder, small, w, h, clear, orbit),
                None => view.render(gpu, encoder, small, w, h, clear),
            });
        }
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

/// A light on the model as the interface sets it: its place (in the framing's units from the
/// model's middle: x right, y up, z toward the front), its hue (a place along the hue bar, 0 to 1),
/// how much of that colour it has (0 white, 1 the hue's own), and its intensity (0 to 1).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record, serde::Serialize, serde::Deserialize)]
pub struct ModelLight {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub hue: f32,
    pub saturation: f32,
    pub intensity: f32,
}

/// A light added in the studio shines this strongly.
const DEFAULT_INTENSITY: f32 = 0.35;

impl ModelLight {
    fn from_gpu(l: &Light) -> ModelLight {
        let [x, y, z] = l.place;
        ModelLight { x, y, z, hue: 0.0, saturation: 0.0, intensity: l.intensity }
    }

    /// Kept within its ranges, any value that isn't a number reset.
    fn sane(self) -> ModelLight {
        let f = |v: f32, lo: f32, hi: f32, or: f32| if v.is_finite() { v.clamp(lo, hi) } else { or };
        ModelLight {
            x: f(self.x, -1e4, 1e4, 0.0),
            y: f(self.y, -1e4, 1e4, 0.0),
            z: f(self.z, -1e4, 1e4, 0.0),
            hue: f(self.hue, 0.0, 1.0, 0.0),
            saturation: f(self.saturation, 0.0, 1.0, 0.0),
            intensity: f(self.intensity, 0.0, 1.0, DEFAULT_INTENSITY),
        }
    }

    /// The light as the renderer takes it: its colour the hue bar's at its hue, mixed with white
    /// by its saturation, in linear light.
    fn to_gpu(self) -> Light {
        let srgb = hue_color(self.hue).map(|c| 1.0 + (c - 1.0) * self.saturation.clamp(0.0, 1.0));
        Light { place: [self.x, self.y, self.z], color: srgb.map(srgb_to_linear), intensity: self.intensity }
    }
}

/// The hue bar's colour (sRGB) at `t` along it (0 to 1).
fn hue_color(t: f32) -> [f32; 3] {
    let at = t.clamp(0.0, 1.0) * (HUE_STOPS.len() - 1) as f32;
    let i = (at.floor() as usize).min(HUE_STOPS.len() - 2);
    let f = at - i as f32;
    let (a, b) = (HUE_STOPS.get(i).copied().unwrap_or([1.0; 3]), HUE_STOPS.get(i + 1).copied().unwrap_or([1.0; 3]));
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// An orbit's turn about the upright and its tilt up (radians), its camera held level.
fn yaw_pitch(orbit: &Orbit) -> (f32, f32) {
    let back = {
        let e = orbit.eye();
        let t = orbit.target;
        let v = [e[0] - t[0], e[1] - t[1], e[2] - t[2]];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    (back[0].atan2(back[2]), back[1].clamp(-1.0, 1.0).asin())
}

impl Scene {
    /// A copy of the view's lights as the scene's lights set them (to project and move them).
    fn lights_view<'a>(&self, view: &'a MeshView) -> LightsView<'a> {
        LightsView { view, lights: self.lights.iter().map(|l| l.to_gpu()).collect() }
    }
}

/// The view's way of placing lights on the screen, for lights of the scene's own.
struct LightsView<'a> {
    view: &'a MeshView,
    lights: Vec<Light>,
}

impl LightsView<'_> {
    fn light_on_screen(&self, orbit: &Orbit, index: usize, width: u32, height: u32) -> Option<[f32; 3]> {
        let light = self.lights.get(index)?;
        self.view.place_on_screen(orbit, light.place, width, height)
    }

    fn move_light(&mut self, orbit: &Orbit, index: usize, dx: f32, dy: f32, width: u32, height: u32) {
        if let Some(l) = self.lights.get_mut(index) {
            l.place = self.view.moved_place(orbit, l.place, dx, dy, width, height);
        }
    }
}

/// A model and its texture sets, as the interface sees them.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ModelInfo {
    pub texture_sets: Vec<String>,
    pub triangles: u32,
    /// How much of each set's texture its triangles cover twice (0..1).
    pub overlaps: Vec<f32>,
    /// The model's meshes, each in its texture set, in order.
    pub submeshes: Vec<SubmeshInfo>,
    /// Each texture set's side, pixels.
    pub texture_size: u32,
}

/// An image a model's file brings for a texture set's channel (0 colour, 1 roughness, 2
/// metalness, 3 its normal map, 4 its occlusion): its file's bytes, and which of its components
/// to take (-1: as it is, or its grey).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ModelTextureFile {
    pub texture_set: u32,
    pub channel: u32,
    pub component: i32,
    pub bytes: Vec<u8>,
}

/// One of a model's meshes: its name and the texture set it is painted in.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SubmeshInfo {
    pub name: String,
    pub texture_set: u32,
}

impl ModelInfo {
    pub(crate) fn of(scene: &Scene) -> ModelInfo {
        ModelInfo {
            texture_sets: scene.set_names(),
            triangles: u32::try_from(scene.mesh.triangles.len()).unwrap_or(u32::MAX),
            overlaps: (0..scene.mesh.parts.len()).map(|i| scene.mesh.uv_overlap(i, 256)).collect(),
            submeshes: scene
                .mesh
                .submeshes
                .iter()
                .map(|m| SubmeshInfo { name: m.name.clone(), texture_set: u32::try_from(m.part).unwrap_or(0) })
                .collect(),
            texture_size: scene.texture_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use procraft_gpu::{Gpu, wgpu};

    use super::ModelFile;
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

    /// A frame of `s`'s model, `side` square, read back (RGBA8).
    fn frame_pixels(s: &CanvasSession, side: u32) -> Vec<u8> {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        st.render_scene(&view, format, (side, side), None);
        let gpu = st.gpu.as_ref().unwrap();
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        procraft_gpu::read_texture(gpu, &target, side, side).unwrap()
    }

    #[test]
    fn panels_over_a_model_blur_it_under_them() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let plain = frame_pixels(&s, 200);
        // a panel over the left half
        s.shared.lock().backdrops = vec![[0.0, 0.0, 100.0, 200.0, 0.0, 1.0]];
        let frosted = frame_pixels(&s, 200);
        let at = |p: &[u8], x: usize, y: usize| p[(y * 200 + x) * 4..][..4].to_vec();
        // the model's edge softened under it, and nothing changed beside it
        let under = (0..200).flat_map(|y| (0..100).map(move |x| (x, y)));
        let changed = under.filter(|&(x, y)| at(&plain, x, y) != at(&frosted, x, y)).count();
        assert!(changed > 200, "{changed}");
        assert!((0..200).all(|y| (100..200).all(|x| at(&plain, x, y) == at(&frosted, x, y))));
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
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 256).unwrap();
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
    fn the_2d_texture_shows_and_paints_flat() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        assert!(!s.model_flat());
        s.set_model_flat(true);
        assert!(s.model_flat() && !s.shared.lock().scene_shown());
        // the texture fills the window: a stroke across it paints there as on any canvas
        s.stroke_begin(PointerKind::Stylus, vec![sample(20.0, 20.0, 0.0)]);
        s.stroke_add((1..20).map(|i| sample(20.0 + i as f32 * 8.0, 20.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(180.0, 20.0, 170.0)]);
        let flat = painted(&s);
        assert!(flat > 100, "{flat}");
        // the model again: the turn still works and the paint stays
        s.set_model_flat(false);
        assert!(!s.model_flat() && s.shared.lock().scene_shown());
        s.turn_model(50.0, 0.0);
        frame(&s, 200);
        assert_eq!(painted(&s), flat);
    }

    #[test]
    fn the_studio_adds_moves_and_keeps_lights() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        // two white lights to begin with; four at most
        assert_eq!(s.model_lights().len(), 2);
        assert!(s.model_lights().iter().all(|l| l.saturation == 0.0));
        assert!(s.add_model_light());
        assert!(s.duplicate_model_light(0));
        assert!(!s.add_model_light() && !s.duplicate_model_light(1));
        assert_eq!(s.model_lights().len(), 4);
        let copy = s.model_lights()[3];
        assert_eq!((copy.y, copy.intensity), (s.model_lights()[0].y, s.model_lights()[0].intensity));
        s.delete_model_light(3);
        s.delete_model_light(9);
        assert_eq!(s.model_lights().len(), 3);
        let red = super::ModelLight { hue: 0.0, saturation: 1.0, intensity: 2.0, ..s.model_lights()[2] };
        s.set_model_light(2, red);
        assert_eq!(s.model_lights()[2].intensity, 1.0);
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        s.set_lighting_studio(true);
        frame(&s, 200);
        // the new light, picked where its cube shows, dragged across the screen
        let at = s.model_light_on_screen(2).unwrap();
        assert_eq!(s.model_light_at(at[0] + 2.0, at[1] - 2.0), Some(2));
        assert_eq!(s.model_light_at(at[0] + 150.0, at[1] + 150.0), None);
        s.move_model_light(2, 20.0, 0.0);
        let moved = s.model_light_on_screen(2).unwrap();
        assert!((moved[0] - at[0] - 20.0).abs() < 0.5 && (moved[1] - at[1]).abs() < 0.5, "{at:?} -> {moved:?}");
        // the camera goes round and nearer; the light goes with the view
        s.studio_turn(40.0, 0.0);
        s.studio_zoom(1.5);
        frame(&s, 200);
        assert_ne!(s.model_light_on_screen(2).unwrap(), moved);
        s.set_lighting_studio(false);
        frame(&s, 200);
        // its one mesh, hidden and kept so
        let info = s.model_info().unwrap();
        assert_eq!(info.submeshes, [super::SubmeshInfo { name: "Mesh 1".into(), texture_set: 0 }]);
        assert!(s.submesh_thumbnail(0, 32).unwrap().rgba.chunks(4).any(|p| p[3] == 255));
        s.set_submesh_hidden(0, true);
        assert_eq!(s.submeshes_hidden(), [true]);
        frame(&s, 200);
        // kept with the model
        let dir = std::env::temp_dir().join(format!("procraft-lights-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        s.save(dir.to_string_lossy().into_owned()).unwrap();
        let again = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!(again.model_lights(), s.model_lights());
        assert_eq!(again.submeshes_hidden(), [true]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_turntable_pulls_back_half_way_and_exports_as_gif_and_png() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let turntable = super::Turntable { swing: false, seconds: 1.0, zoom: 1.0, ease: 1.0, environment: false };
        let covered = |t: super::Turntable, i: u32| {
            let p = s.model_turntable_picture(i, 8, t, 100, 100, false).unwrap();
            let s = crate::SURROUND.map(|c| (c * 255.0).round() as i32);
            p.rgba.chunks(4).filter(|px| (0..3).any(|k| (i32::from(px[k]) - s[k]).abs() > 12)).count()
        };
        let (near, far) = (covered(turntable, 0), covered(turntable, 4));
        assert!(far * 2 < near, "pulled back: {near} -> {far}");
        let still = super::Turntable { zoom: 0.0, ..turntable };
        let (a, b) = (covered(still, 0), covered(still, 4));
        assert!(a.abs_diff(b) * 10 < a, "no pull: {a} vs {b}");
        let dir = std::env::temp_dir();
        let gif = dir.join(format!("procraft-turn-{}.gif", std::process::id()));
        s.export_model_gif(gif.to_string_lossy().into_owned(), turntable, 40, 30, 8.0).unwrap();
        assert!(std::fs::read(&gif).unwrap().starts_with(b"GIF89a"));
        let png = dir.join(format!("procraft-turn-{}.png", std::process::id()));
        s.export_model_apng(png.to_string_lossy().into_owned(), turntable, 40, 30, 8.0).unwrap();
        assert!(std::fs::read(&png).unwrap().get(1..4) == Some(b"PNG".as_slice()));
        let _ = (std::fs::remove_file(&gif), std::fs::remove_file(&png));
    }

    #[test]
    fn roughness_and_metalness_are_painted_in_their_own_layers() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        dab(&s);
        let colour = painted(&s);
        assert!(colour > 20, "{colour}");
        // roughness: the same layers, nothing painted in them yet
        assert_eq!(s.model_channel(), 0);
        s.set_model_channel(1);
        assert_eq!(s.model_channel(), 1);
        assert_eq!(painted(&s), 0);
        s.stroke_begin(PointerKind::Stylus, vec![sample(70.0, 100.0, 0.0)]);
        s.stroke_end(vec![sample(130.0, 100.0, 50.0)]);
        frame(&s, 200);
        let rough = painted(&s);
        assert!(rough > colour, "{rough}");
        // a layer added here is a layer of the colour too
        let added = s.add_layer();
        assert!(added != 0);
        s.set_model_channel(0);
        assert_eq!(painted(&s), colour);
        assert!(s.layers().iter().any(|l| l.id == added));
        // exports take every channel whichever is painted on: the colour and roughness as
        // painted, the metalness (nothing painted) the plain one
        s.set_model_channel(2);
        assert_eq!(painted(&s), 0);
        let dir = std::env::temp_dir().join(format!("procraft-channels-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pngs = s.export_model_textures(dir.join("t").to_string_lossy().into_owned()).unwrap();
        let names: Vec<String> =
            pngs.iter().filter_map(|p| std::path::Path::new(p).file_name()?.to_str().map(str::to_owned)).collect();
        let set = super::file_stem(&s.model_info().unwrap().texture_sets[0], "Material");
        assert_eq!(names, ["Color", "Roughness", "Metallic"].map(|c| format!("{set}-{c}.png")));
        let read = |p: &str| {
            let mut reader =
                png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).unwrap())).read_info().unwrap();
            let mut px = vec![0; reader.output_buffer_size().unwrap()];
            reader.next_frame(&mut px).unwrap();
            px.chunks(4).map(|p| (p[0], p[3])).collect::<Vec<_>>()
        };
        let plain = (255, 255);
        assert!(read(&pngs[0]).iter().any(|&p| p != plain));
        assert!(read(&pngs[1]).iter().any(|&p| p != plain));
        assert!(read(&pngs[2]).iter().all(|&p| p == (0, 255)));
        // kept with the model, saved while painting metalness
        s.set_model_channel(1);
        s.save(dir.join("a").to_string_lossy().into_owned()).unwrap();
        assert_eq!(s.model_channel(), 1);
        assert_eq!(painted(&s), rough);
        assert!(dir.join("a").join("sets").join("0").join("c1").exists());
        let again = CanvasSession::open(dir.join("a").to_string_lossy().into_owned()).unwrap();
        assert!(shown(&again));
        assert_eq!(again.model_channel(), 0);
        assert_eq!(painted(&again), colour);
        again.set_model_channel(1);
        assert_eq!(painted(&again), rough);
        frame(&again, 200);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_obj_takes_its_material_file_and_images_from_the_files_picked_with_it() {
        let obj = format!("mtllib Cube Parts.mtl\nusemtl body\n{CUBE}");
        let file = |name: &str, bytes: &[u8]| ModelFile { name: name.into(), bytes: bytes.to_vec() };
        let mtl = b"newmtl body\nKd 1 0 0\nmap_Kd -bm 1 textures/Body.png\nmap_Pr rough.png\n";
        let files = vec![
            file("other.mtl", b"newmtl body\nKd 0 0 1\n"),
            file("cube parts.MTL", mtl),
            file("body.png", b"colour"),
            file("rough.png", b"rough"),
            file("unused.png", b"unused"),
        ];
        let s = CanvasSession::open_model(obj.clone().into(), "cube.obj".into(), files, "Cube".into(), 64).unwrap();
        // the one it names, whatever the case; its maps found by their names alone
        let found: Vec<(u32, Vec<u8>)> = s.model_texture_files().into_iter().map(|f| (f.channel, f.bytes)).collect();
        assert_eq!(found, [(0, b"colour".to_vec()), (1, b"rough".to_vec())]);
        // given once
        assert!(s.model_texture_files().is_empty());
        // without the one it names, the only one picked
        let only = vec![file("whatever.mtl", mtl), file("rough.png", b"rough")];
        let s = CanvasSession::open_model(obj.into(), "cube.obj".into(), only, "Cube".into(), 64).unwrap();
        assert_eq!(s.model_texture_files().len(), 1);
    }

    /// A zip of `files` (name, contents), in order.
    fn zipped(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = procraft_engine::zip::ZipWriter::new();
        for (name, bytes) in files {
            zip.add(name, bytes);
        }
        zip.finish()
    }

    #[test]
    fn a_zipped_obj_comes_in_with_its_material_file_and_images() {
        let obj = format!("mtllib cube.mtl\nusemtl body\n{CUBE}");
        let mtl =
            b"newmtl body\nmap_Kd textures/Body.png\nmap_roughness ./rough.png\nmap_tangentSpaceNormal flat.png\n";
        let zip = zipped(&[
            ("Cube Folder/", b""),
            ("__MACOSX/Cube Folder/._cube.obj", b"junk"),
            ("Cube Folder/cube.obj", obj.as_bytes()),
            ("Cube Folder/cube.mtl", mtl),
            ("Cube Folder/other/body.png", b"not this one"),
            ("Cube Folder/textures/body.png", b"colour"),
            ("Cube Folder/rough.png", b"rough"),
            ("Cube Folder/flat.png", b"normal"),
        ]);
        assert_eq!(super::zip_model_name(zip.clone()).as_deref(), Some("Cube Folder/cube.obj"));
        let s = CanvasSession::open_model(zip, "Cube Folder.zip".into(), Vec::new(), "Cube".into(), 64).unwrap();
        let info = s.model_info().unwrap();
        assert_eq!((info.texture_sets.as_slice(), info.triangles), (["body".to_owned()].as_slice(), 12));
        // each map found by its path from the model, ignoring case; the normal map as channel 3
        let found: Vec<(u32, Vec<u8>)> = s.model_texture_files().into_iter().map(|f| (f.channel, f.bytes)).collect();
        assert_eq!(found, [(0, b"colour".to_vec()), (1, b"rough".to_vec()), (3, b"normal".to_vec())]);
        // kept as the model it holds
        let source = s.shared.lock().scene.as_ref().map(|sc| (sc.source.extension.clone(), sc.source.mtl.is_some()));
        assert_eq!(source, Some(("obj".to_owned(), true)));
    }

    #[test]
    fn a_zips_model_is_the_one_nearest_its_top() {
        let deep = zipped(&[("parts/inner.obj", CUBE.as_bytes()), ("outer.OBJ", CUBE.as_bytes())]);
        assert_eq!(super::zip_model_name(deep).as_deref(), Some("outer.OBJ"));
        let usd = zipped(&[("readme.txt", b"hi"), ("scene/model.usdz", b"x")]);
        assert_eq!(super::zip_model_name(usd).as_deref(), Some("scene/model.usdz"));
        // nothing to paint, or another kind of package: not a model
        assert_eq!(super::zip_model_name(zipped(&[("picture.png", b"p")])), None);
        assert_eq!(super::zip_model_name(zipped(&[("Document.archive", b"d"), ("mesh.obj", CUBE.as_bytes())])), None);
        assert_eq!(super::zip_model_name(zipped(&[("artwork.json", b"{}"), ("model.obj", CUBE.as_bytes())])), None);
        assert_eq!(super::zip_model_name(b"not a zip".to_vec()), None);
        let none = CanvasSession::open_model(zipped(&[("a.png", b"p")]), "a.zip".into(), Vec::new(), "A".into(), 64);
        assert!(none.is_err());
    }

    #[test]
    fn a_models_own_obj_zip_opens_again_textured() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        dab(&s);
        let path = std::env::temp_dir().join(format!("procraft-obj-zip-{}.zip", std::process::id()));
        s.export_model_obj(path.to_string_lossy().into_owned()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let again = CanvasSession::open_model(bytes, "Cube.zip".into(), Vec::new(), "Again".into(), 64).unwrap();
        // its colour, roughness and metalness, as its material file maps them
        let channels: Vec<u32> = again.model_texture_files().iter().map(|f| f.channel).collect();
        assert_eq!(channels, [0, 1, 2]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_packaged_models_textures_come_in_as_its_base_layers() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        assert_eq!(s.layers().first().map(|l| l.name.clone()).as_deref(), Some("Base Layer"));
        assert!(s.model_texture_files().is_empty());
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        dab(&s);
        let path = std::env::temp_dir().join(format!("procraft-in-{}.usdz", std::process::id()));
        s.export_model_usdz(path.to_string_lossy().into_owned()).unwrap();
        // opened again, its textures come back as their images' files: the colour, and the
        // roughness and metalness from their red
        let bytes = std::fs::read(&path).unwrap();
        let again = CanvasSession::open_model(bytes, "cube.usdz".into(), Vec::new(), "Again".into(), 64).unwrap();
        let files = again.model_texture_files();
        let kinds: Vec<(u32, u32, i32)> = files.iter().map(|f| (f.texture_set, f.channel, f.component)).collect();
        assert_eq!(kinds, [(0, 0, -1), (0, 1, 0), (0, 2, 0)]);
        // decoded (as the interface does) into the Base Layer
        let mut reader = png::Decoder::new(std::io::Cursor::new(files[0].bytes.clone())).read_info().unwrap();
        let mut px = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut px).unwrap();
        assert_eq!(px.len(), 64 * 64 * 4);
        assert!(!again.set_model_texture(0, 0, vec![0; 8]));
        assert!(again.set_model_texture(0, 0, px));
        assert!(shown(&again));
        frame(&again, 200);
        // the exported picture is opaque (over white): every texel painted
        assert_eq!(painted(&again), 64 * 64);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn two_fingers_bring_the_model_nearer_and_move_it() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
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
        let s =
            CanvasSession::open_model(two.into_bytes(), "two.obj".into(), Vec::new(), "Two cubes".into(), 64).unwrap();
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
        assert_eq!(
            names,
            [
                "Two_cubes.mtl",
                "Two_cubes.obj",
                "front-color.png",
                "front-metallic.png",
                "front-roughness.png",
                "side_wall-color.png",
                "side_wall-metallic.png",
                "side_wall-roughness.png"
            ]
        );
        let obj = String::from_utf8(zip.read("Two_cubes.obj").unwrap()).unwrap();
        let mtl = String::from_utf8(zip.read("Two_cubes.mtl").unwrap()).unwrap();
        let mesh = procraft_engine::mesh::read_obj(&obj, Some(&mtl)).unwrap();
        assert_eq!(mesh.triangles.len(), 12);
        assert_eq!(mesh.materials[1].color_map.as_deref(), Some("side_wall-color.png"));
        assert_eq!(mesh.materials[1].roughness_map.as_deref(), Some("side_wall-roughness.png"));
        assert_eq!(mesh.materials[1].metallic_map.as_deref(), Some("side_wall-metallic.png"));
        // the painted set's picture has the stroke on white
        let png = zip.read("front-color.png").unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut px = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut px).unwrap();
        assert!(px.chunks(4).all(|p| p[3] == 255));
        assert!(px.chunks(4).any(|p| p[0] < 128) && px.chunks(4).any(|p| p[0] == 255));
        let _ = std::fs::remove_file(&path);
        // the pictures on their own, and the model as shown
        let dir = std::env::temp_dir().join(format!("procraft-textures-{}", std::process::id()));
        let pngs = s.export_model_textures(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!(pngs.len(), 6);
        assert!(pngs.iter().all(|p| std::path::Path::new(p).exists()));
        assert!(pngs.iter().any(|p| p.ends_with("side_wall-Roughness.png")));
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
        let tiff = std::env::temp_dir().join(format!("procraft-render-{}.tif", std::process::id()));
        s.export_model_tiff(tiff.to_string_lossy().into_owned()).unwrap();
        assert!(
            std::fs::read(&tiff).unwrap().starts_with(b"II*\0") || std::fs::read(&tiff).unwrap().starts_with(b"MM\0*")
        );
        let _ = std::fs::remove_file(&tiff);
        // a turntable's frames: the model turned, half way round the back, and round again
        let turntable = super::Turntable { swing: false, seconds: 1.0, zoom: 0.0, ease: 0.0, environment: false };
        let first = s.model_turntable_picture(0, 4, turntable, 100, 100, true).unwrap();
        let half = s.model_turntable_picture(2, 4, turntable, 100, 100, true).unwrap();
        assert_eq!((first.width, first.height, first.rgba.len()), (100, 100, 100 * 100 * 4));
        assert!(first.rgba.chunks(4).all(|p| p[3] == 255));
        assert_ne!(first.rgba, half.rgba);
        let again = s.model_turntable_picture(4, 4, turntable, 100, 100, true).unwrap();
        assert_eq!(first.rgba, again.rgba);
        // as a USDZ: the layer and both sets' images
        s.export_model_usdz(path.to_string_lossy().into_owned()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let zip = procraft_engine::zip::ZipReader::new(&bytes).unwrap();
        assert_eq!(
            zip.names().collect::<Vec<_>>(),
            [
                "model.usda",
                "textures/front-color.png",
                "textures/front-roughness.png",
                "textures/front-metallic.png",
                "textures/side_wall-color.png",
                "textures/side_wall-roughness.png",
                "textures/side_wall-metallic.png"
            ]
        );
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
    fn a_models_normal_and_occlusion_maps_are_kept_and_exported() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        // channels 3 and 4: the normal and occlusion maps, the texture size square
        assert!(!s.set_model_texture(0, 3, vec![0; 8]));
        assert!(!s.set_model_texture(0, 5, [128, 128, 255, 255].repeat(64 * 64)));
        assert!(s.set_model_texture(0, 3, [128, 128, 255, 255].repeat(64 * 64)));
        assert!(s.set_model_texture(0, 4, [200, 200, 200, 255].repeat(64 * 64)));
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let set = super::file_stem(&s.model_info().unwrap().texture_sets[0], "Material");
        let dir = std::env::temp_dir().join(format!("procraft-maps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // in the OBJ's zip, named in its material file
        let obj = dir.join("cube.zip");
        s.export_model_obj(obj.to_string_lossy().into_owned()).unwrap();
        let bytes = std::fs::read(&obj).unwrap();
        let zip = procraft_engine::zip::ZipReader::new(&bytes).unwrap();
        assert!(zip.read(&format!("{set}-normal.png")).is_some());
        assert!(zip.read(&format!("{set}-ambient_occlusion.png")).is_some());
        let mtl = String::from_utf8(zip.read("Cube.mtl").unwrap()).unwrap();
        assert!(mtl.contains(&format!("map_tangentSpaceNormal {set}-normal.png")), "{mtl}");
        assert!(mtl.contains(&format!("map_ao {set}-ambient_occlusion.png")), "{mtl}");
        // shared as textures
        let pngs = s.export_model_textures(dir.join("t").to_string_lossy().into_owned()).unwrap();
        assert_eq!(pngs.len(), 5);
        assert!(pngs.iter().any(|p| p.ends_with(&format!("{set}-AmbientOcclusion.png"))));
        // kept with the artwork, and back when it opens
        s.save(dir.join("a").to_string_lossy().into_owned()).unwrap();
        assert!(dir.join("a").join("maps").join("0-normal.png").exists());
        let again = CanvasSession::open(dir.join("a").to_string_lossy().into_owned()).unwrap();
        let kept = again.shared.lock().scene.as_ref().unwrap().surface_maps.clone();
        assert_eq!(kept[0][0].as_deref().map(|m| m[..4].to_vec()), Some(vec![128, 128, 255, 255]));
        assert_eq!(kept[0][1].as_deref().map(|m| m[..4].to_vec()), Some(vec![200, 200, 200, 255]));
        // a USDZ carries them, and brings them in again
        let usdz = dir.join("cube.usdz");
        s.export_model_usdz(usdz.to_string_lossy().into_owned()).unwrap();
        let back =
            CanvasSession::open_model(std::fs::read(&usdz).unwrap(), "cube.usdz".into(), Vec::new(), "Back".into(), 64)
                .unwrap();
        let channels: Vec<u32> = back.model_texture_files().iter().map(|f| f.channel).collect();
        assert_eq!(channels, [0, 1, 2, 3, 4]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_model_exports_as_a_glb_for_viewers() {
        let two = CUBE.replacen("f 1/1", "usemtl front\nf 1/1", 1).replacen("f 1/1 2/2", "usemtl sides\nf 1/1 2/2", 1);
        let s = CanvasSession::open_model(two.into_bytes(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let path = std::env::temp_dir().join(format!("procraft-glb-{}.glb", std::process::id()));
        s.export_model_glb(path.to_string_lossy().into_owned()).unwrap();
        let glb = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(&glb[0..4], b"glTF");
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let bin = &glb[20 + json_len + 8..];
        assert_eq!(doc["materials"].as_array().unwrap().len(), 2);
        assert_eq!(doc["materials"][0]["name"], "front");
        // the roughness (1, as nothing says otherwise) in green and the metalness (0) in blue
        let texture =
            doc["materials"][0]["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"].as_u64().unwrap();
        let source = doc["textures"][texture as usize]["source"].as_u64().unwrap() as usize;
        let view = &doc["bufferViews"][doc["images"][source]["bufferView"].as_u64().unwrap() as usize];
        let offset = view["byteOffset"].as_u64().unwrap() as usize;
        let png = &bin[offset..offset + view["byteLength"].as_u64().unwrap() as usize];
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut px = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut px).unwrap();
        assert!(px.chunks(4).all(|p| p[1] == 255 && p[2] == 0), "{:?}", &px[..4]);
    }

    #[test]
    fn a_saved_model_exports_once_opened_again() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        dab(&s);
        let dir = std::env::temp_dir().join(format!("procraft-reopen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        s.save(dir.join("a").to_string_lossy().into_owned()).unwrap();
        let again = CanvasSession::open(dir.join("a").to_string_lossy().into_owned()).unwrap();
        assert!(shown(&again));
        frame(&again, 200);
        let zip = dir.join("cube.zip").to_string_lossy().into_owned();
        again.export_model_obj(zip).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_model_saves_with_each_texture_set_and_how_it_was_seen() {
        // the cube's front, top and back in one material, its bottom and sides in another
        let two = CUBE.replacen("f 1/1", "usemtl front\nf 1/1", 1).replacen("f 1/1 2/2", "usemtl sides\nf 1/1 2/2", 1);
        let s = CanvasSession::open_model(two.into_bytes(), "two.OBJ".into(), Vec::new(), "Two".into(), 128).unwrap();
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
        assert_eq!(s.model_exposure(), 1.0);
        s.set_model_exposure(9.0);
        assert_eq!(s.model_exposure(), 5.0);
        s.set_model_exposure(0.4);
        assert!(s.model_environment_shown());
        s.set_model_environment_shown(false);
        frame(&s, 200);
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
        assert_eq!(again.model_exposure(), 0.4);
        assert!(!again.model_environment_shown());
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
