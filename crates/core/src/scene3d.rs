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
use procraft_engine::mesh::{MAX_TRIANGLES, Mesh, MeshError, PartImages, SurfaceHit, read_obj};
use procraft_engine::stroke::StrokeBuilder;
use procraft_engine::studio::MAX_GRAIN_SCALE;
use procraft_engine::usd::{read_usd, read_usdc, read_usdz};
use procraft_engine::view::View;
use procraft_gpu::wgpu::util::DeviceExt as _;
use procraft_gpu::{
    Canvas, Gpu, Light, Lighting, MOST_LIGHTS, MeshView, Orbit, SharedStroke, ShownSelection, StrokeStyle, StrokeTool,
    wgpu,
};

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

impl CanvasSession {
    /// Moves the picture being placed as `change` says.
    fn change_picture(&self, change: impl FnOnce(&mut PicturePlace)) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(placing) = st.scene.as_mut().and_then(|s| s.placing.as_mut()) {
            change(&mut placing.place);
            self.shared.frame(st);
        }
    }

    /// `flags` of the model's look, one for each of `count`'s (false where none was set).
    fn look_flags(&self, flags: impl Fn(&MeshLook) -> &Vec<bool>, count: impl Fn(&Mesh) -> usize) -> Vec<bool> {
        let st = self.shared.lock();
        let Some(scene) = st.scene.as_ref() else { return Vec::new() };
        let set = flags(&scene.look);
        (0..count(&scene.mesh)).map(|i| set.get(i).copied().unwrap_or(false)).collect()
    }

    /// Changes the model's look as an undo step named `name` in the canvas painted on.
    fn change_look(&self, name: &str, change: impl FnOnce(&mut MeshLook, &Mesh)) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(scene) = st.scene.as_mut() else { return };
        // a picture being placed holds the canvas's stroke open
        if scene.placing.is_some() {
            return;
        }
        let before = scene.look.clone();
        change(&mut scene.look, &scene.mesh);
        if scene.look == before {
            return;
        }
        let after = scene.look.clone();
        let (part, channel) = (scene.current, scene.channel);
        if st.scene_shown() {
            st.scene_stroke_cancel();
        } else {
            st.finish_stroke();
        }
        st.settle();
        if let Some(c) = st.canvas.as_mut() {
            c.record_outside_change(name);
        }
        st.flush();
        let step = st.canvas.as_ref().and_then(Canvas::undo_step);
        if let (Some(step), Some(scene)) = (step, st.scene.as_mut()) {
            if scene.look_steps.len() >= MOST_JOINT_STEPS {
                scene.look_steps.remove(0);
            }
            scene.look_steps.push(LookStep { part, channel, step, before, after });
        }
        st.revision += 1;
        self.shared.frame(st);
    }
}

/// A texture set waiting while another is painted on: its document, its canvas once the model
/// has been drawn, and until then the pixels it was opened with.
pub(crate) struct TextureSet {
    doc: Document,
    canvas: Option<Canvas>,
    pending: Pixels,
    /// A layer to fill with one colour (premultiplied RGBA8) when its canvas is made, unless
    /// pixels wait for it: a new set's Base Layer, with what the model's material gives.
    fill: Option<(LayerId, [u8; 4])>,
}

impl TextureSet {
    fn empty(side: u32) -> TextureSet {
        TextureSet { doc: empty_texture(side), canvas: None, pending: Vec::new(), fill: None }
    }

    /// A new set with `structure`'s layers, its Base Layer (if it still has it) filled with
    /// `rgba` once it is drawn.
    fn filled(side: u32, structure: &Document, rgba: [u8; 4]) -> TextureSet {
        let mut set = TextureSet::empty(side);
        follow_layers(&mut set, structure);
        let base = base_layer();
        set.fill = set.doc.layer(base).is_some().then_some((base, rgba));
        set
    }

    /// The pixels waiting for its canvas, its fill made into pixels (taken: it is given once).
    fn take_pending(&mut self) -> Pixels {
        let mut out = std::mem::take(&mut self.pending);
        if let Some((id, rgba)) = self.fill.take()
            && self.doc.layer(id).is_some()
            && !out.iter().any(|(l, _)| *l == id)
        {
            out.insert(0, (id, solid(self.doc.width(), rgba)));
        }
        out
    }

    /// The pixels waiting for its canvas with its fill, kept waiting.
    fn pending_with_fill(&self) -> Pixels {
        let mut out = self.pending.clone();
        if let Some((id, rgba)) = self.fill
            && self.doc.layer(id).is_some()
            && !out.iter().any(|(l, _)| *l == id)
        {
            out.insert(0, (id, solid(self.doc.width(), rgba)));
        }
        out
    }
}

/// Whether a stroke laying down `amount` (0..1) leaves a texel of `value` as it is: the same
/// value, and one a byte holds exactly (so the texel, the material's own value, is the same).
fn same_texel(amount: f32, value: f32) -> bool {
    let byte = value * 255.0;
    (amount - value).abs() < 1e-6 && (byte - byte.round()).abs() < 1e-3
}

/// The layer every texture set starts with: its Base Layer.
fn base_layer() -> LayerId {
    empty_texture(1).layers().first().map_or(LayerId(1), |l| l.id)
}

/// A `side` square of one colour (premultiplied RGBA8).
fn solid(side: u32, rgba: [u8; 4]) -> Vec<u8> {
    rgba.repeat((side as usize) * (side as usize))
}

/// A colour, roughness or metalness (0..1) as a Base Layer is filled with it: opaque.
fn opaque(rgb: [f32; 3]) -> [u8; 4] {
    let [r, g, b] = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    [r, g, b, 255]
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

/// A feature of a model that painting on it may not go well with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ModelWarning {
    /// Texels several surfaces share: paint on one shows on the others.
    OverlappingUvs,
    /// Texture coordinates outside the texture: paint shows on every repeat.
    RepeatingTextures,
}

/// How much of a texture set may be covered twice before it's worth a warning.
const OVERLAP_WARNED: f32 = 0.01;

/// What a newly opened model warns of.
fn warnings_of(mesh: &Mesh) -> Vec<ModelWarning> {
    let mut out = Vec::new();
    if (0..mesh.parts.len()).any(|i| mesh.uv_overlap(i, 256) > OVERLAP_WARNED) {
        out.push(ModelWarning::OverlappingUvs);
    }
    if mesh.repeats_textures() {
        out.push(ModelWarning::RepeatingTextures);
    }
    out
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
    model_name_among(zip.names())
}

/// The model a zip holding `names` holds (see [`zipped_model_name`]).
pub(crate) fn model_name_among<'n>(names: impl Iterator<Item = &'n str>) -> Option<String> {
    let files: Vec<&str> = names
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

/// Whether a zip holding `names` is a model's artwork: its folder, as
/// [`CanvasSession::export_model_artwork`] writes it.
pub(crate) fn is_model_artwork<'n>(mut names: impl Iterator<Item = &'n str>) -> bool {
    let (mut artwork, mut model) = (false, false);
    names.any(|n| {
        artwork |= n == "artwork.json";
        model |= !n.contains('/') && n.starts_with("model.") && is_model_name(n);
        artwork && model
    })
}

/// Every file under `dir` into `zip`, named by its path from `root`.
fn zip_folder(zip: &mut procraft_engine::zip::ZipWriter, root: &Path, dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            zip_folder(zip, root, &path)?;
        } else if let Ok(name) = path.strip_prefix(root) {
            let name: Vec<String> = name.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            zip.add(&name.join("/"), &fs::read(&path)?);
        }
    }
    Ok(())
}

/// Unpacks a model's artwork (see [`CanvasSession::export_model_artwork`]) from file `path` into
/// the empty folder `dir`: false, with nothing kept, if it isn't one. Nothing goes outside `dir`.
#[uniffi::export]
pub fn unpack_model_artwork(path: String, dir: String) -> bool {
    let Ok(bytes) = fs::read(&path) else { return false };
    let Some(zip) = procraft_engine::zip::ZipReader::new(&bytes) else { return false };
    if !is_model_artwork(zip.names()) {
        return false;
    }
    let root = Path::new(&dir);
    let inside = |name: &str| {
        let parts = || name.split('/');
        !name.contains('\\') && !name.contains(':') && parts().all(|p| !p.is_empty() && p != "." && p != "..")
    };
    let wrote = zip.names().filter(|n| !n.ends_with('/') && inside(n)).try_for_each(|name| {
        let bytes = zip.read(name).ok_or(())?;
        let to = root.join(name);
        to.parent().map_or(Ok(()), fs::create_dir_all).map_err(|_| ())?;
        fs::write(to, bytes).map_err(|_| ())
    });
    if wrote.is_err() {
        let _ = fs::remove_dir_all(root);
    }
    wrote.is_ok()
}

/// The model a zip of one holds (its path in the zip), as the gallery's Import opens it; none if
/// it holds none or is an artwork, a layered package or brushes.
#[uniffi::export]
pub fn zip_model_name(bytes: Vec<u8>) -> Option<String> {
    zipped_model_name(&procraft_engine::zip::ZipReader::new(&bytes)?)
}

/// Sets flag `index` of `flags`, `count` of them.
fn set_flag(flags: &mut Vec<bool>, count: usize, index: u32, value: bool) {
    flags.resize(count, false);
    if let Some(f) = flags.get_mut(index as usize) {
        *f = value;
    }
}

/// The places of the flags set.
fn places(flags: &[bool]) -> Vec<usize> {
    flags.iter().enumerate().filter(|(_, f)| **f).map(|(i, _)| i).collect()
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
    /// The texture shows flat only for a tool that works on it, back to the model when it ends.
    flat_for_tool: bool,
    /// A picture being placed on the model.
    placing: Option<PlacedPicture>,
    /// What a model opened from its file warns of, until it has been said.
    warnings: Vec<ModelWarning>,
    /// What fills and adjustments reach while meshes are picked: the meshes it was made for, and
    /// where they lie in the texture.
    reach: Option<(Vec<usize>, procraft_engine::selection::Mask)>,
    /// The selection as it was before an adjustment was kept to the meshes picked.
    held_selection: Option<Option<procraft_engine::selection::Mask>>,
    /// How the Reference window sees the model, from when it first has.
    reference_orbit: Option<Orbit>,
    /// The window as the eyedropper last saw it.
    seen: Option<SeenFrame>,
    /// What of the model is hidden, locked and picked.
    look: MeshLook,
    /// The hide, lock and pick changes, each an undo step in the canvas it was made on.
    look_steps: Vec<LookStep>,
    /// Each part's roughness and metalness documents, once made; the place of the channel painted
    /// is empty (it is the session's own).
    channels: Vec<[Option<TextureSet>; 2]>,
    /// The channel painted on the current part: 0 its colour, 1 roughness, 2 metalness. While it
    /// isn't the colour, the part's colour document waits in [`Scene::sets`].
    channel: usize,
    /// A channel's tile is chosen under the current layer (shown blue, painting going to that
    /// channel), not the layer's own row.
    tile_chosen: bool,
    /// The tiles chosen (and rows chosen again), each an undo step in the channels it was made
    /// between.
    tile_steps: Vec<TileStep>,
    /// Each part's channel maps as last given to the view (as [`Scene::filtered`]).
    channel_shown: Vec<[Option<(u64, bool)>; 2]>,
    /// The images picked with an OBJ, until its materials' maps have been taken from them.
    picked: Vec<ModelFile>,
    /// Each part's normal and occlusion maps from the model's file (RGBA8, the texture size
    /// square), shown as they are; and whether the view has them as they are now.
    surface_maps: Vec<[Option<Arc<Vec<u8>>>; 2]>,
    surface_shown: bool,
    /// The stroke under way changes what's under it (a smudge, wet paint): the layer as the camera
    /// saw it as it began, on the screen's canvas, and as it was.
    laying: Option<wgpu::TextureView>,
    /// A stroke is being carried onto the model each frame; `ending`: once more, then it ends.
    pub(crate) stroking: bool,
    ending: bool,
    /// What the stroke on the current part's colour lays down in its roughness and metalness as
    /// well (a stroke on the colour paints all three), while it goes on.
    material_paint: Option<[MaterialPaint; 2]>,
    /// The brush's roughness and metalness sources as textures, with their keys.
    material_sources: [Option<(u64, wgpu::TextureView)>; 2],
    /// Strokes that painted the colour, roughness and metalness together: the step each made in
    /// each one's canvas, so undo and redo take them together.
    joint_steps: Vec<JointStep>,
    /// The brush's grain as a texture, with its key (its own image's, or its built-in one's with
    /// the top bit set), for a stroke's grain read along the model's axes.
    grain_source: Option<(u64, wgpu::TextureView)>,
    /// The stroke textures the texture sets' canvases draw their strokes in by turns, made when
    /// first wanted: the colours', and the roughness and metalness's (a stroke on a colour draws
    /// in one of each at once, one channel after the other).
    strokes: [Option<SharedStroke>; 2],
}

/// What a stroke on a part's colour lays down in its roughness or metalness: the brush's amount
/// (0..1), one tile of its source every so many canvas pixels, and the source's key (none: the
/// amount all over).
#[derive(Clone, Copy, Debug, PartialEq)]
struct MaterialPaint {
    amount: f32,
    tile: f32,
    source: Option<u64>,
    /// The source is read about the camera, as the brush's grain is, not fixed to the model.
    follows_camera: bool,
}

/// A stroke that painted part `part`'s colour, roughness and metalness: the step it made in each
/// (as [`Canvas::undo_step`] numbers them), if it made one there.
#[derive(Clone, Copy, Debug, PartialEq)]
struct JointStep {
    part: usize,
    steps: [Option<u64>; 3],
}

/// The most strokes remembered as painting every channel (older ones are long out of the undo
/// history).
const MOST_JOINT_STEPS: usize = 512;

/// A tile chosen under part `part`'s current layer, or its row chosen again: the channel painted
/// and whether a tile was chosen, before and after, and the step it left in the canvas of each
/// channel it was made between (as [`Canvas::undo_step`] numbers them), for undo and redo to
/// follow.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TileStep {
    part: usize,
    from: (usize, bool),
    to: (usize, bool),
    steps: [Option<u64>; 3],
}

/// What of a model is hidden or locked, and which of its meshes strokes reach.
#[derive(Clone, Debug, Default, PartialEq)]
struct MeshLook {
    /// By submesh: hidden ones aren't drawn or painted, locked ones are drawn but take no paint.
    hidden: Vec<bool>,
    locked: Vec<bool>,
    /// By texture set, the same for all its meshes.
    sets_hidden: Vec<bool>,
    sets_locked: Vec<bool>,
    /// The meshes picked, in the texture set painted on: strokes reach only them. None picked:
    /// the whole set.
    active: Vec<usize>,
}

/// A change to [`MeshLook`] kept in `part`'s `channel` canvas as `step`, with the look before and
/// after it, for undo and redo to follow.
#[derive(Clone, Debug, PartialEq)]
struct LookStep {
    part: usize,
    channel: usize,
    step: u64,
    before: MeshLook,
    after: MeshLook,
}

/// A picture being placed on a model, shown as it lands, a stroke on its own new layer until it
/// is kept.
struct PlacedPicture {
    view: wgpu::TextureView,
    size: (u32, u32),
    place: PicturePlace,
    first: PicturePlace,
    /// Where it was last carried onto the model, and how the model was seen then.
    carried: Option<(PicturePlace, Orbit)>,
}

/// Where a picture lies on the window: its middle, how far it reaches from there along its own
/// sides (one negative: it is flipped), and its turn (radians, clockwise on the window).
#[derive(Clone, Copy, Debug, PartialEq)]
struct PicturePlace {
    middle: [f32; 2],
    half: [f32; 2],
    turn: f32,
}

impl PicturePlace {
    /// A `size` picture first placed on a `window`: in its middle, its long side this share of
    /// the window's shorter side.
    const FIRST_SHARE: f32 = 0.7;

    fn first(size: (u32, u32), window: (u32, u32)) -> PicturePlace {
        let long = size.0.max(size.1).max(1) as f32;
        let reach = window.0.min(window.1) as f32 * Self::FIRST_SHARE / 2.0;
        PicturePlace {
            middle: [window.0 as f32 / 2.0, window.1 as f32 / 2.0],
            half: [size.0 as f32 / long * reach, size.1 as f32 / long * reach],
            turn: 0.0,
        }
    }

    /// Its four corners on the window, clockwise from its top left.
    fn corners(&self) -> [[f32; 2]; 4] {
        let (s, c) = self.turn.sin_cos();
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(u, v)| {
            let (x, y) = (u * self.half[0], v * self.half[1]);
            [self.middle[0] + x * c - y * s, self.middle[1] + x * s + y * c]
        })
    }

    /// Where window point `p` lies across it: -1 to 1 along each of its sides.
    fn across(&self, p: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.turn.sin_cos();
        let (x, y) = (p[0] - self.middle[0], p[1] - self.middle[1]);
        [(x * c + y * s) / self.half[0], (y * c - x * s) / self.half[1]]
    }

    fn holds(&self, p: [f32; 2]) -> bool {
        self.across(p).iter().all(|t| t.abs() <= 1.0)
    }

    /// A `size` picture's pixels from the window's, x then y (a·x + b·y + c).
    fn to_picture(self, size: (u32, u32)) -> [[f32; 3]; 2] {
        let (s, c) = self.turn.sin_cos();
        let row = |extent: u32, half: f32, a: f32, b: f32| {
            let k = extent as f32 / (2.0 * half);
            [a * k, b * k, extent as f32 / 2.0 - (a * self.middle[0] + b * self.middle[1]) * k]
        };
        [row(size.0, self.half[0], c, s), row(size.1, self.half[1], -s, c)]
    }
}

/// The window as last drawn for the eyedropper, and what it was drawn from.
struct SeenFrame {
    size: (u32, u32),
    orbit: Orbit,
    revision: u64,
    pixels: Vec<u8>,
}

/// The `side` square of RGBA8 `pixels` (a window `size`) centred on `at`: what lies off the window
/// clear.
fn window_round(pixels: &[u8], size: (u32, u32), at: (f32, f32), side: u32) -> Vec<u8> {
    let (w, h) = (size.0 as i64, size.1 as i64);
    let (x0, y0) = (at.0 as i64 - i64::from(side) / 2, at.1 as i64 - i64::from(side) / 2);
    let mut out = vec![0u8; side as usize * side as usize * 4];
    for (row, line) in out.chunks_exact_mut(side as usize * 4).enumerate() {
        let y = y0 + row as i64;
        if !(0..h).contains(&y) {
            continue;
        }
        for (col, px) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = x0 + col as i64;
            if !(0..w).contains(&x) {
                continue;
            }
            let at = ((y * w + x) * 4) as usize;
            if let Some(from) = pixels.get(at..at + 4) {
                px.copy_from_slice(from);
            }
        }
    }
    out
}

/// How long a model takes to go round in the gallery's Preview.
const MODEL_PREVIEW_FRAMES: u32 = 72;
const MODEL_PREVIEW_FPS: u32 = 18;

/// What undo and redo call a channel's content replaced.
const MAP_REPLACED: &str = "replace material map content";

/// `image` (premultiplied RGBA8) stretched over a `side` square, read between its pixels. None if
/// it holds nothing.
fn stretched(image: &crate::layers::ThumbnailImage, side: u32) -> Option<Vec<u8>> {
    let (w, h) = (image.width as usize, image.height as usize);
    if w == 0 || h == 0 || image.rgba.len() != w * h * 4 {
        return None;
    }
    let at = |x: usize, y: usize, c: usize| f32::from(image.rgba.get((y * w + x) * 4 + c).copied().unwrap_or(0));
    let mut out = Vec::with_capacity(side as usize * side as usize * 4);
    for y in 0..side {
        let fy = ((y as f32 + 0.5) * h as f32 / side as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (fy as usize, fy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..side {
            let fx = ((x as f32 + 0.5) * w as f32 / side as f32 - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (fx as usize, fx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
                let low = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
                out.push((top * (1.0 - ty) + low * ty).round() as u8);
            }
        }
    }
    Some(out)
}

/// How far past the meshes picked fills and adjustments reach, texels: as far as strokes do.
const REACH_GROWN: usize = 2;

/// What undo and redo call a mesh or texture set hidden, shown, locked or unlocked, or meshes
/// picked.
const MESHES_SELECTED: &str = "select meshes";

/// Which part a texel of a `side` square texture holds texture coordinates `uv`: the coordinates
/// wrapped into the texture, rows running down from v = 1.
fn texel_of(uv: [f32; 2], side: u32) -> (u32, u32) {
    let wrap = |t: f32| t - t.floor();
    let along = |t: f32| ((t * side as f32) as u32).min(side.saturating_sub(1));
    (along(wrap(uv[0])), along(1.0 - wrap(uv[1])))
}

/// What undo and redo call a tile chosen, and the layer's row chosen again.
const TILE_CHOSEN: &str = "select material layer";
const ROW_CHOSEN: &str = "deselect material layer";

/// The step undone (true) or redone by work on `canvas`, from its next undo and redo steps
/// `before` it.
fn stepped(before: (Option<u64>, Option<u64>), canvas: &Canvas) -> Option<(u64, bool)> {
    match (before, (canvas.undo_step(), canvas.redo_step())) {
        ((Some(s), _), (u, Some(r))) if r == s && u != Some(s) => Some((s, true)),
        ((_, Some(s)), (Some(u), r)) if u == s && r != Some(s) => Some((s, false)),
        _ => None,
    }
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
            flat_for_tool: false,
            placing: None,
            warnings: Vec::new(),
            reach: None,
            held_selection: None,
            reference_orbit: None,
            seen: None,
            look: MeshLook::default(),
            look_steps: Vec::new(),
            channels: Vec::new(),
            channel: 0,
            tile_chosen: false,
            tile_steps: Vec::new(),
            channel_shown: Vec::new(),
            picked: Vec::new(),
            surface_maps: Vec::new(),
            surface_shown: false,
            laying: None,
            stroking: false,
            ending: false,
            material_paint: None,
            material_sources: [None, None],
            joint_steps: Vec::new(),
            grain_source: None,
            strokes: [None, None],
        }
    }

    /// The stroke texture the canvases of the texture sets' colours (`material` false), or of
    /// their roughness and metalness (true), draw in by turns.
    fn stroke(&mut self, gpu: &Gpu, material: bool) -> SharedStroke {
        let side = self.texture_size;
        let [colour, materials] = &mut self.strokes;
        let slot = if material { materials } else { colour };
        slot.get_or_insert_with(|| SharedStroke::new(gpu, side, side)).clone()
    }

    /// Names of the texture sets: their materials', else numbered.
    pub(crate) fn set_names(&self) -> Vec<String> {
        let parts = self.mesh.parts.iter().enumerate();
        parts
            .map(|(i, p)| if p.material.is_empty() { format!("Texture Set {}", i + 1) } else { p.material.clone() })
            .collect()
    }

    /// A picture of the model through `orbit`, `side` square, on the surround, with its environment
    /// behind it if one shows. None until it has been drawn.
    fn picture_through(&mut self, gpu: &Gpu, orbit: Orbit, side: u32) -> Option<crate::layers::ThumbnailImage> {
        let s = crate::SURROUND;
        let clear = [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0];
        let environment = self.lit && self.environment_shown;
        let rgba = self.view.as_mut()?.picture_from(gpu, orbit, side, side, clear, environment);
        match rgba {
            Ok(rgba) => Some(crate::layers::ThumbnailImage { width: side, height: side, rgba }),
            Err(e) => {
                log::warn!("a picture of the model: {e}");
                None
            }
        }
    }

    /// How the model is seen now.
    fn orbit(&self) -> Option<Orbit> {
        self.view.as_ref().map(|v| v.orbit).or(self.orbit)
    }

    /// Submesh `index`'s texture set.
    fn set_of(&self, index: usize) -> Option<usize> {
        self.mesh.submeshes.get(index).map(|m| m.part)
    }

    /// Whether submesh `index` is hidden, itself or with its texture set.
    fn mesh_hidden(&self, index: usize) -> bool {
        let set = self.set_of(index).and_then(|p| self.look.sets_hidden.get(p)).copied();
        self.look.hidden.get(index).copied().unwrap_or(false) || set.unwrap_or(false)
    }

    /// Whether submesh `index` is locked, itself or with its texture set.
    fn mesh_locked(&self, index: usize) -> bool {
        let set = self.set_of(index).and_then(|p| self.look.sets_locked.get(p)).copied();
        self.look.locked.get(index).copied().unwrap_or(false) || set.unwrap_or(false)
    }

    /// The meshes a stroke is meant for: those picked, else the texture set's.
    fn meant_meshes(&self) -> Vec<usize> {
        if !self.look.active.is_empty() {
            return self.look.active.clone();
        }
        (0..self.mesh.submeshes.len()).filter(|&i| self.set_of(i) == Some(self.current)).collect()
    }

    /// Whether submesh `index` takes strokes now.
    fn mesh_painted(&self, index: usize) -> bool {
        let meant = self.look.active.is_empty() || self.look.active.contains(&index);
        meant && !self.mesh_hidden(index) && !self.mesh_locked(index)
    }

    /// Draws `layer` of the current part as the camera sees it (a window `size`) onto the screen's
    /// canvas, and gives that picture back. None before the model has been drawn.
    fn see_layer(&mut self, gpu: &Gpu, layer: &wgpu::TextureView, size: (u32, u32)) -> Option<wgpu::TextureView> {
        self.tell_view_what_takes_paint();
        let part = self.current;
        let (doc, screen) = self.screen.as_mut()?;
        let view = self.view.as_mut()?;
        screen.render(gpu, doc, None);
        let seen = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer as seen"),
            size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target = seen.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("layer as seen") });
        view.draw_seen(gpu, &mut encoder, part, layer, &target, size.0, size.1);
        gpu.queue.submit([encoder.finish()]);
        screen.copy_to_layer(gpu, doc.current(), &seen).then_some(target)
    }

    /// Gives the view the meshes that show.
    fn tell_view_what_shows(&mut self) {
        let hidden: Vec<bool> = (0..self.mesh.submeshes.len()).map(|i| self.mesh_hidden(i)).collect();
        if let Some(view) = self.view.as_mut() {
            for (i, hide) in hidden.into_iter().enumerate() {
                view.set_hidden(i, hide);
            }
        }
    }

    /// Gives the view the meshes strokes reach.
    fn tell_view_what_takes_paint(&mut self) {
        let painted: Vec<bool> = (0..self.mesh.submeshes.len()).map(|i| self.mesh_painted(i)).collect();
        if let Some(view) = self.view.as_mut() {
            for (i, takes) in painted.into_iter().enumerate() {
                view.set_painted(i, takes);
            }
        }
    }

    /// Where fills and adjustments stay on the current texture set while some of its meshes are
    /// picked, or locked or hidden; none while they reach all of it.
    fn reach(&mut self) -> Option<procraft_engine::selection::Mask> {
        let all: Vec<usize> =
            (0..self.mesh.submeshes.len()).filter(|&i| self.set_of(i) == Some(self.current)).collect();
        let mut picked: Vec<usize> = all.iter().copied().filter(|&i| self.mesh_painted(i)).collect();
        picked.sort_unstable();
        if picked.is_empty() || picked.len() >= all.len() {
            return None;
        }
        if self.reach.as_ref().is_none_or(|(made, _)| *made != picked) {
            let side = self.texture_size;
            let data = self.mesh.submesh_coverage(&picked, side as usize, REACH_GROWN);
            self.reach = Some((picked, procraft_engine::selection::Mask { width: side, height: side, data }));
        }
        self.reach.as_ref().map(|(_, m)| m.clone())
    }

    /// Why strokes can't land: every mesh they're meant for is hidden, or hidden or locked.
    fn why_blocked(&self) -> Option<crate::layers::Blocked> {
        let meant = self.meant_meshes();
        if self.flat || meant.is_empty() {
            return None;
        }
        if meant.iter().all(|&i| self.mesh_hidden(i)) {
            return Some(crate::layers::Blocked::MeshHidden);
        }
        meant.iter().all(|&i| self.mesh_hidden(i) || self.mesh_locked(i)).then_some(crate::layers::Blocked::MeshLocked)
    }

    /// The surface under window point (`x`, `y`) of a `width` × `height` window, among what shows.
    fn surface_at(&self, x: f32, y: f32, (width, height): (u32, u32)) -> Option<SurfaceHit> {
        let (from, along) = self.orbit()?.sight(x, y, width, height);
        self.mesh.first_surface(from, along, |i| self.mesh_hidden(i))
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
        let warnings = warnings_of(&mesh);
        let mut scene = Scene::new(mesh, ModelSource { bytes: file, extension, mtl }, side, 0, Vec::new());
        scene.picked = picked;
        scene.warnings = warnings;
        // each texture set's Base Layer the material's colour, where the model brings no image
        let brings = scene.brings_colour();
        let colour =
            |part: usize| (!brings.get(part).copied().unwrap_or(false)).then(|| opaque(scene.mesh.surface(part).0));
        let own = colour(0).map(|rgba| vec![(base_layer(), solid(side, rgba))]).unwrap_or_default();
        for part in 1..scene.sets.len() {
            let fill = colour(part).map(|rgba| (base_layer(), rgba));
            if let Some(set) = scene.sets.get_mut(part).and_then(Option::as_mut) {
                set.fill = fill;
            }
        }
        let session = CanvasSession::with_document(empty_texture(side), name, own);
        session.shared.lock().scene = Some(scene);
        Ok(session)
    }

    /// The model painted on, if this is one.
    pub fn model_info(&self) -> Option<ModelInfo> {
        self.shared.lock().scene.as_ref().map(ModelInfo::of)
    }

    /// Each texture set's layers, in order, whichever is painted on; none unless a model is open.
    pub fn texture_set_stats(&self) -> Vec<TextureSetStats> {
        let st = self.shared.lock();
        let Some(scene) = st.scene.as_ref() else { return Vec::new() };
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let row = |(name, doc): (String, &Document)| TextureSetStats {
            name,
            used: count(doc.pixel_layer_ids().count()),
            maximum: count(doc.layer_cap()),
        };
        scene.set_names().into_iter().zip(st.colour_docs()).map(row).collect()
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

    /// Starts placing `image` (RGBA8, `straight` or premultiplied) on the shown model, on a new layer: it shows as it
    /// lands, taken from the camera, until [`CanvasSession::model_picture_end`]. False if it can't
    /// be (no model shown, no room for a layer, nothing to place).
    pub fn model_picture_begin(&self, image: crate::layers::ThumbnailImage, straight: bool) -> bool {
        let image = if straight { crate::clipboard::premultiplied(image) } else { image };
        let (w, h) = (image.width, image.height);
        if w == 0 || h == 0 || image.rgba.len() != w as usize * h as usize * 4 {
            return false;
        }
        if !self.shared.lock().scene_shown() || self.add_layer_quietly() == 0 {
            return false;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let layer = st.doc.current();
        let size = st.window_size;
        let (Some(gpu), Some(scene), Some(canvas)) = (st.gpu.as_ref(), st.scene.as_mut(), st.canvas.as_mut()) else {
            return false;
        };
        let texture = gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("picture placed on the model"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &image.rgba,
        );
        let first = PicturePlace::first((w, h), size);
        scene.placing = Some(PlacedPicture {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            size: (w, h),
            place: first,
            first,
            carried: None,
        });
        canvas.begin_drawn_stroke(layer, StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
        self.shared.frame(st);
        true
    }

    /// The corners of the picture being placed on the window (x, y pairs clockwise from its top
    /// left), while one is: for the box round it.
    pub fn model_picture_box(&self) -> Option<Vec<f32>> {
        let st = self.shared.lock();
        let placing = st.scene.as_ref()?.placing.as_ref()?;
        Some(placing.place.corners().into_iter().flatten().collect())
    }

    /// Moves the picture being placed by (`dx`, `dy`) window pixels, scales it by `scale` and turns
    /// it by `turn` radians about its middle.
    pub fn model_picture_move(&self, dx: f32, dy: f32, scale: f32, turn: f32) {
        self.change_picture(|p| {
            p.middle = [p.middle[0] + dx, p.middle[1] + dy];
            if scale.is_finite() && scale > 0.0 {
                p.half = p.half.map(|h| h * scale);
            }
            p.turn += turn;
        });
    }

    /// Flips the picture being placed across (`horizontal`) or up and down.
    pub fn model_picture_flip(&self, horizontal: bool) {
        self.change_picture(|p| {
            let side = usize::from(!horizontal);
            if let Some(h) = p.half.get_mut(side) {
                *h = -*h;
            }
        });
    }

    /// Puts the picture being placed back where it first lay.
    pub fn model_picture_reset(&self) {
        let first = self.shared.lock().scene.as_ref().and_then(|s| s.placing.as_ref()).map(|p| p.first);
        if let Some(first) = first {
            self.change_picture(|p| *p = first);
        }
    }

    /// Ends placing a picture: it stays on its layer (`keep`), or goes with the layer.
    pub fn model_picture_end(&self, keep: bool) {
        let layer = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let Some(scene) = st.scene.as_mut() else { return };
            if scene.placing.take().is_none() {
                return;
            }
            let (Some(gpu), Some(canvas)) = (st.gpu.as_ref(), st.canvas.as_mut()) else { return };
            if keep {
                canvas.end_stroke();
                canvas.render(gpu, &mut st.doc, None);
            } else {
                canvas.cancel_stroke();
            }
            st.revision += 1;
            self.shared.frame(st);
            st.doc.current()
        };
        if !keep {
            self.delete_layer(layer.0);
        }
    }

    /// The model as the Reference window shows it, `side` square: through a camera of its own, at
    /// first where the model is seen from, its textures as they are now (shown flat or not).
    pub fn reference_model_picture(&self, side: u32) -> Option<crate::layers::ThumbnailImage> {
        let side = side.clamp(8, 2048);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.scene.as_ref()?;
        st.scene_frame(None, wgpu::TextureFormat::Rgba8Unorm, st.window_size, None);
        let (Some(gpu), Some(scene)) = (st.gpu.as_ref(), st.scene.as_mut()) else { return None };
        let orbit = scene.reference_orbit.or_else(|| scene.orbit())?;
        scene.reference_orbit = Some(orbit);
        scene.picture_through(gpu, orbit, side)
    }

    /// A finger dragged by (`dx`, `dy`) pixels across the Reference window, `side` pixels across,
    /// turns its model.
    pub fn reference_model_turn(&self, dx: f32, dy: f32, side: f32) {
        let mut st = self.shared.lock();
        let Some(scene) = st.scene.as_mut() else { return };
        let Some(mut orbit) = scene.reference_orbit.or_else(|| scene.orbit()) else { return };
        let length = (dx * dx + dy * dy).sqrt();
        orbit.turn(dx, dy, length / side.max(1.0) * TURN_PER_SIDE);
        scene.reference_orbit = Some(orbit);
    }

    /// How many frames a model turns through in the gallery's Preview, and at how many a second.
    pub fn model_preview_frames(&self) -> u32 {
        MODEL_PREVIEW_FRAMES
    }

    pub fn model_preview_fps(&self) -> u32 {
        MODEL_PREVIEW_FPS
    }

    /// Frame `index` of a model turning in the gallery's Preview, `side` square, opened without a
    /// window ([`CanvasSession::draw_without_window`]).
    pub fn model_preview_picture(&self, index: u32, side: u32) -> Option<crate::layers::ThumbnailImage> {
        let side = side.clamp(8, 2048);
        {
            let mut st = self.shared.lock();
            if index == 0 || st.scene.as_ref().is_none_or(|s| s.view.is_none()) {
                st.window_size = (side, side);
                st.scene_frame(None, wgpu::TextureFormat::Rgba8Unorm, (side, side), None);
            }
        }
        let seconds = MODEL_PREVIEW_FRAMES as f32 / MODEL_PREVIEW_FPS as f32;
        let turntable = Turntable { swing: false, seconds, zoom: 0.0, ease: 0.0, environment: false };
        self.model_turntable_picture(index, MODEL_PREVIEW_FRAMES, turntable, side, side, false)
    }

    /// Replaces layer `id`'s content in `channel` (0 its colour, 1 roughness, 2 metalness) with
    /// `image` (straight RGBA8) stretched over the texture, as an undo step; that channel is
    /// painted from now on. False if it can't be.
    pub fn replace_material_map(&self, id: u64, channel: u32, image: crate::layers::ThumbnailImage) -> bool {
        let layer = LayerId(id);
        {
            let st = self.shared.lock();
            if channel > 2 || st.scene.is_none() || st.doc.layer(layer).is_none_or(|l| st.doc.is_locked(l.id)) {
                return false;
            }
        }
        let side = self.shared.lock().doc.width();
        let Some(pixels) = stretched(&crate::clipboard::premultiplied(image), side) else { return false };
        self.choose_channel_tile(channel);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.settle();
        let Some(canvas) = st.canvas.as_mut() else { return false };
        canvas.name_next_step(MAP_REPLACED);
        canvas.write_layer(layer, pixels);
        st.flush();
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    /// Whether a selection drawn on a model reaches every surface under it.
    pub fn selection_x_ray(&self) -> bool {
        self.shared.lock().x_ray
    }

    /// Sets whether a selection drawn on a model reaches every surface under it (X-Ray).
    pub fn set_selection_x_ray(&self, on: bool) {
        self.shared.lock().x_ray = on;
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

    /// Whether a channel's tile is chosen under the current layer (Color, Roughness or Metallic,
    /// shown blue), not the layer's own row.
    pub fn channel_tile_chosen(&self) -> bool {
        self.shared.lock().scene.as_ref().is_some_and(|s| s.tile_chosen)
    }

    /// Chooses `channel`'s tile under the current layer (0 Color, 1 Roughness, 2 Metallic), its
    /// channel painted on from now on: an undo step, which chooses again what was before.
    pub fn choose_channel_tile(&self, channel: u32) {
        let mut st = self.shared.lock();
        st.choose_tile(Some(channel as usize));
        self.shared.frame(&mut st);
    }

    /// Chooses the current layer's own row again (its colour painted, no tile chosen), as an undo
    /// step.
    pub fn leave_channel_tiles(&self) {
        let mut st = self.shared.lock();
        st.choose_tile(None);
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
        let stroke = scene.stroke(gpu, channel != 0);
        let set = scene.slot(scene.current, channel)?.as_mut()?;
        set_canvas(gpu, set, &stroke).ok()?;
        let TextureSet { canvas: Some(canvas), doc, .. } = set else { return None };
        let t = canvas.layer_thumbnail(gpu, doc, LayerId(id), max_side).ok()?;
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

    /// The images the model brings ([`CanvasSession::model_texture_files`]) decoded (PNG or
    /// JPEG) and scaled to the texture sets' side into them: colour as it is, roughness,
    /// metalness and occlusion from the component they name, as grey. An image that can't be read
    /// is left out, the rest still come in. How many came in.
    pub fn load_model_textures(&self) -> u32 {
        let Some(side) = self.model_info().map(|m| m.texture_size) else { return 0 };
        let mut loaded = 0;
        for f in self.model_texture_files() {
            let Ok(decoded) = image::load_from_memory(&f.bytes) else { continue };
            let picture = decoded.to_rgba8();
            let mut picture = if picture.dimensions() == (side, side) {
                picture
            } else {
                image::imageops::resize(&picture, side, side, image::imageops::FilterType::Triangle)
            };
            for px in picture.pixels_mut() {
                let [r, g, b, a] = px.0;
                px.0 = match usize::try_from(f.component).ok().and_then(|c| px.0.get(c).copied()) {
                    // one component, as grey
                    Some(v) => [v, v, v, 255],
                    // premultiplied
                    None => {
                        let k = u16::from(a);
                        let mul = |c: u8| ((u16::from(c) * k + 127) / 255) as u8;
                        [mul(r), mul(g), mul(b), a]
                    }
                };
            }
            if self.set_model_texture(f.texture_set, f.channel, picture.into_raw()) {
                loaded += 1;
            }
        }
        loaded
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
                None => {
                    st.pending_pixels.retain(|(id, _)| *id != base);
                    st.pending_pixels.push((base, crate::project::Opened::Pixels(rgba)));
                }
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
                None => {
                    target.pending.retain(|(id, _)| *id != base);
                    target.pending.push((base, rgba));
                }
            }
        }
        self.shared.frame(st);
        true
    }

    /// Which of the model's submeshes are hidden, in order (themselves, not by their texture set).
    pub fn submeshes_hidden(&self) -> Vec<bool> {
        self.look_flags(|l| &l.hidden, |m| m.submeshes.len())
    }

    /// Hides submesh `index` (not drawn or painted), or shows it again: an undo step.
    pub fn set_submesh_hidden(&self, index: u32, hidden: bool) {
        let name = if hidden { "Hide Submesh" } else { "Show Submesh" };
        self.change_look(name, |look, mesh| set_flag(&mut look.hidden, mesh.submeshes.len(), index, hidden));
    }

    /// Which of the model's submeshes are locked (themselves, not by their texture set).
    pub fn submeshes_locked(&self) -> Vec<bool> {
        self.look_flags(|l| &l.locked, |m| m.submeshes.len())
    }

    /// Locks submesh `index` (drawn, but taking no paint), or unlocks it: an undo step.
    pub fn set_submesh_locked(&self, index: u32, locked: bool) {
        let name = if locked { "Lock mesh" } else { "Unlock mesh" };
        self.change_look(name, |look, mesh| set_flag(&mut look.locked, mesh.submeshes.len(), index, locked));
    }

    /// Which texture sets are hidden.
    pub fn texture_sets_hidden(&self) -> Vec<bool> {
        self.look_flags(|l| &l.sets_hidden, |m| m.parts.len())
    }

    /// Hides texture set `index` with all its meshes, or shows it again: an undo step.
    pub fn set_texture_set_hidden(&self, index: u32, hidden: bool) {
        let name = if hidden { "Hide Texture Set" } else { "Show Texture Set" };
        self.change_look(name, |look, mesh| set_flag(&mut look.sets_hidden, mesh.parts.len(), index, hidden));
    }

    /// Which texture sets are locked.
    pub fn texture_sets_locked(&self) -> Vec<bool> {
        self.look_flags(|l| &l.sets_locked, |m| m.parts.len())
    }

    /// Locks texture set `index` with all its meshes, or unlocks it: an undo step.
    pub fn set_texture_set_locked(&self, index: u32, locked: bool) {
        let name = if locked { "Lock Texture Set" } else { "Unlock Texture Set" };
        self.change_look(name, |look, mesh| set_flag(&mut look.sets_locked, mesh.parts.len(), index, locked));
    }

    /// The meshes picked, which alone take strokes (none: the whole texture set does).
    pub fn active_submeshes(&self) -> Vec<u32> {
        let st = self.shared.lock();
        st.scene.as_ref().map_or_else(Vec::new, |s| s.look.active.iter().map(|&i| i as u32).collect())
    }

    /// Picks mesh `index` alone, its texture set the one painted on.
    pub fn pick_submesh(&self, index: u32) {
        let index = index as usize;
        let set = {
            let st = self.shared.lock();
            let Some(scene) = st.scene.as_ref() else { return };
            let Some(set) = scene.set_of(index) else { return };
            if scene.look.active == [index] && scene.current == set {
                return;
            }
            set
        };
        self.choose_texture_set(set as u32);
        self.change_look(MESHES_SELECTED, |look, _| look.active = vec![index]);
    }

    /// Adds mesh `index` to those picked, or takes it away (the last one stays). False, with
    /// nothing changed, for a mesh of a texture set other than the one painted on: meshes picked
    /// together share a texture set.
    pub fn pick_more_submeshes(&self, index: u32) -> bool {
        let index = index as usize;
        {
            let st = self.shared.lock();
            let Some(scene) = st.scene.as_ref() else { return false };
            if scene.set_of(index) != Some(scene.current) {
                return false;
            }
        }
        self.change_look(MESHES_SELECTED, |look, _| {
            if !look.active.contains(&index) {
                look.active.push(index);
            } else if look.active.len() > 1 {
                look.active.retain(|&i| i != index);
            }
        });
        true
    }

    /// The mesh shown under window point (`x`, `y`), if there is one.
    pub fn submesh_at(&self, x: f32, y: f32) -> Option<u32> {
        let st = self.shared.lock();
        let hit = st.scene.as_ref()?.surface_at(x, y, st.window_size)?;
        hit.submesh.map(|i| i as u32)
    }

    /// Picks the mesh under window point (`x`, `y`), its texture set the one painted on, and says
    /// what is painted on now. None off the model.
    pub fn pick_submesh_at(&self, x: f32, y: f32) -> Option<String> {
        let index = self.submesh_at(x, y)?;
        self.pick_submesh(index);
        let st = self.shared.lock();
        let mesh = st.scene.as_ref()?.mesh.submeshes.get(index as usize)?.name.clone();
        let layer = st.doc.layer(st.doc.current())?.name.clone();
        Some(format!("{layer} on {mesh}"))
    }

    /// Shows and unlocks the meshes strokes are meant for, and their texture set: what the
    /// dialog over a stroke that couldn't land offers.
    pub fn free_meant_meshes(&self) {
        let meant = {
            let st = self.shared.lock();
            let Some(scene) = st.scene.as_ref() else { return };
            scene.meant_meshes()
        };
        self.change_look("Show Submesh", |look, mesh| {
            look.hidden.resize(mesh.submeshes.len(), false);
            look.locked.resize(mesh.submeshes.len(), false);
            for i in meant {
                if let Some(h) = look.hidden.get_mut(i) {
                    *h = false;
                }
                if let Some(l) = look.locked.get_mut(i) {
                    *l = false;
                }
                let set = mesh.submeshes.get(i).map(|m| m.part).unwrap_or(0);
                if let Some(h) = look.sets_hidden.get_mut(set) {
                    *h = false;
                }
                if let Some(l) = look.sets_locked.get_mut(set) {
                    *l = false;
                }
            }
        });
    }

    /// The name of the first of the meshes strokes are meant for, for a dialog about them.
    pub fn meant_mesh_name(&self) -> Option<String> {
        let st = self.shared.lock();
        let scene = st.scene.as_ref()?;
        let first = *scene.meant_meshes().first()?;
        Some(scene.mesh.submeshes.get(first)?.name.clone())
    }

    /// What a model newly opened from its file warns of, once.
    pub fn take_model_warnings(&self) -> Vec<ModelWarning> {
        let mut st = self.shared.lock();
        st.scene.as_mut().map_or_else(Vec::new, |s| std::mem::take(&mut s.warnings))
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

    /// The artwork, saved to `dir` first, as one file at `path` that comes back whole in another
    /// copy of the app: its folder zipped, the model, every texture set's layers and how it was
    /// seen.
    pub fn export_model_artwork(&self, dir: String, path: String) -> Result<(), MobileError> {
        self.save(dir.clone())?;
        let root = Path::new(&dir);
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip_folder(&mut zip, root, root).map_err(|e| file_error("saving", e))?;
        let bytes = zip.finish();
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("saving", e)))
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

    /// The model as the window shows it, on the canvas's surround, as a JPEG at `path`.
    pub fn export_model_jpeg(&self, path: String) -> Result<(), MobileError> {
        let (w, h, rgba) = self.model_picture(false)?;
        let rgb = crate::project::onto_white(&crate::project::straight_alpha(&rgba));
        let bytes = procraft_engine::jpeg::encode(&rgb, w, h, crate::project::JPEG_QUALITY);
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("rendering the model", e)))
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
        // the frames made take it to 0.95, the file written to 1
        self.exported(0.0);
        for i in 0..count {
            let t = self
                .model_turntable_picture(i, count, turntable, width, height, false)
                .ok_or_else(|| file_error("saving", "the model is not shown yet"))?;
            gif.get_or_insert_with(|| procraft_engine::formats::GifWriter::new(t.width, t.height)).frame(
                &t.rgba,
                delay.max(1),
                true,
            );
            self.exported(0.95 * (i + 1) as f32 / count as f32);
        }
        let bytes = gif.ok_or_else(|| file_error("saving", "there are no frames"))?.finish();
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
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
        self.exported(0.0);
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
                self.exported(0.95 * (i + 1) as f32 / count as f32);
            }
            writer.finish().map_err(|e| file_error("saving", e))
        })?;
        self.exported(1.0);
        Ok(())
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
    zip.read(&packed_name(zip, path)?)
}

/// The entry [`packed_file`] reads for `path`, if there is one.
fn packed_name(zip: &procraft_engine::zip::ZipReader<'_>, path: &str) -> Option<String> {
    let path = path.trim_start_matches("./");
    if zip.names().any(|n| n == path) {
        return Some(path.to_owned());
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    zip.names().find(|n| n.rsplit('/').next() == Some(name)).map(str::to_owned)
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
                    let stroke = scene.stroke(gpu, channel != 0);
                    match scene.slot(part, channel).and_then(Option::as_mut) {
                        Some(set) => Some(waiting_picture(gpu, set, &stroke).map_err(failed)?),
                        None => None,
                    }
                };
            }
            out.push(pictures);
        }
        Ok(out)
    }
}

/// A waiting texture set's picture, drawn now (its canvas made first, drawing in `stroke`, if
/// the model hasn't been drawn since it was opened).
fn waiting_picture(gpu: &Gpu, set: &mut TextureSet, stroke: &SharedStroke) -> Result<Vec<u8>, String> {
    set_canvas(gpu, set, stroke)?;
    let TextureSet { canvas: Some(canvas), doc, .. } = set else { return Err("no canvas".to_owned()) };
    canvas.render_finished(gpu, doc);
    canvas.read_composite(gpu).map_err(|e| e.to_string())
}

/// Makes `set`'s canvas if it has none (with the pixels it waits with): one that takes the
/// strokes carried onto the model, drawing them in `stroke`, and goes without brush images.
fn set_canvas(gpu: &Gpu, set: &mut TextureSet, stroke: &SharedStroke) -> Result<(), String> {
    if set.canvas.is_none() {
        let mut c = Canvas::for_drawn_strokes(gpu, &set.doc, stroke).map_err(|e| e.to_string())?;
        for (id, pixels) in set.take_pending() {
            if let Err(e) = c.upload_layer(gpu, id, &pixels) {
                log::error!("opening a texture set's layer: {e}");
            }
        }
        set.canvas = Some(c);
    }
    Ok(())
}

/// A texture set's canvas left waiting while another is painted: its work done, it keeps no
/// pictures to composite around and lets go of its brush images.
fn to_waiting(gpu: &Gpu, set: &mut TextureSet) {
    if let Some(canvas) = set.canvas.as_mut() {
        canvas.render(gpu, &mut set.doc, None);
        canvas.set_resident_budget(0);
        canvas.let_go_of_brush_images(gpu);
    }
}

/// A texture set's canvas becoming the session's: paged as the session's canvases are, its brush
/// images made now so the first stroke doesn't wait for them.
fn to_session(gpu: &Gpu, canvas: &mut Canvas) {
    canvas.set_resident_budget(crate::layers::resident_budget());
    canvas.prepare_brush_images(gpu);
}

/// Why a model can't be painted on, in words.
fn mesh_error_text(e: &MeshError) -> String {
    match e {
        MeshError::NoFaces => "it has no faces".to_owned(),
        MeshError::PointCloud => "it is a point cloud, with no faces to paint".to_owned(),
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
    /// The submeshes locked, the texture sets hidden and locked, the meshes picked, by place.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    locked: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sets_hidden: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sets_locked: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    active: Vec<usize>,
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
            // the layers with pixels (a group has none of its own)
            for id in set.doc.pixel_layer_ids() {
                let rgba = canvas.read_layer(gpu, id).map_err(|e| file_error("reading a layer", e))?;
                pixels.push((id, rgba));
            }
            Ok(pixels)
        }
        _ => Ok(set.pending_with_fill()),
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
                hidden: places(&scene.look.hidden),
                locked: places(&scene.look.locked),
                sets_hidden: places(&scene.look.sets_hidden),
                sets_locked: places(&scene.look.sets_locked),
                active: scene.look.active.clone(),
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
            Some(TextureSet { doc, canvas: None, pending, fill: None })
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
                    *slot = Some(TextureSet { doc, canvas: None, pending, fill: None });
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
    let (meshes, sets) = (scene.mesh.submeshes.len(), scene.mesh.parts.len());
    let flags = |places: &[usize], count: usize| (0..count).map(|i| places.contains(&i)).collect();
    let in_set = |i: &usize| scene.set_of(*i) == Some(saved.current.min(sets.saturating_sub(1)));
    scene.look = MeshLook {
        hidden: flags(&saved.hidden, meshes),
        locked: flags(&saved.locked, meshes),
        sets_hidden: flags(&saved.sets_hidden, sets),
        sets_locked: flags(&saved.sets_locked, sets),
        active: saved.active.iter().copied().filter(in_set).collect(),
    };
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
    /// What part `part`'s `channel` (1 roughness, 2 metalness) starts as, where the model's
    /// material brings no image for it: the material's value, else the plain one.
    fn channel_value(&self, part: usize, channel: usize) -> f32 {
        let (_, roughness, metallic) = self.mesh.surface(part);
        if channel == 1 { roughness } else { metallic }
    }

    /// Whether the model brings an image for each part's colour: a USDZ's packed one, or one
    /// picked or zipped with it, that its material maps.
    fn brings_colour(&self) -> Vec<bool> {
        let zip = (self.source.extension == "usdz")
            .then(|| procraft_engine::zip::ZipReader::new(&self.source.bytes))
            .flatten();
        let found = |path: &str| match &zip {
            Some(zip) => packed_name(zip, path).is_some(),
            None => companion(&self.picked, path).is_some(),
        };
        let material = |p: &procraft_engine::mesh::Part| self.mesh.materials.iter().find(|m| m.name == p.material);
        self.mesh.parts.iter().map(|p| material(p).and_then(|m| m.color_map.as_deref()).is_some_and(found)).collect()
    }

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
    // no id handed out in any channel handed out again: a layer new here is clear, or holds what
    // this channel's own steps left on it (undone away and back on the other channel)
    doc.keep_ids_from(structure);
    doc.keep_ids_from(&set.doc);
    set.doc = doc;
    if !changed {
        return;
    }
    set.pending.retain(|(id, _)| had.contains(id) && structure.layer(*id).is_some());
    if let Some(canvas) = set.canvas.as_mut() {
        // its strokes' steps stay, to be undone with the colour's
        canvas.forget_document_steps();
        // composited again with the layers it has now
        canvas.invalidate();
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
        let mut old = TextureSet {
            doc: std::mem::replace(&mut self.doc, Document::new(1, 1)),
            canvas: self.canvas.take(),
            pending: crate::project::opened_pixels(std::mem::take(&mut self.pending_pixels), side, side),
            fill: None,
        };
        if let Some(gpu) = self.gpu.as_ref() {
            to_waiting(gpu, &mut old);
        }
        let structure = old.doc.clone();
        if let Some(slot) = scene.slot(part, from) {
            *slot = Some(old);
        }
        let value = scene.channel_value(part, channel);
        let mut new = scene
            .slot(part, channel)
            .and_then(Option::take)
            .unwrap_or_else(|| TextureSet::filled(side, &structure, opaque([value; 3])));
        follow_layers(&mut new, &structure);
        self.pending_pixels = crate::project::opened(new.take_pending());
        self.doc = new.doc;
        self.canvas = new.canvas;
        if let (Some(gpu), Some(canvas)) = (self.gpu.as_ref(), self.canvas.as_mut()) {
            to_session(gpu, canvas);
        }
        scene.channel = channel;
        self.ensure_canvas();
    }

    /// Chooses a tile (`Some` channel) under the current layer, or the layer's own row, as an
    /// undo step: one left in the canvas of the channel painted until now and one in the
    /// chosen channel's (one step when the channel stays the same).
    pub(crate) fn choose_tile(&mut self, tile: Option<usize>) {
        let Some(scene) = self.scene.as_ref() else { return };
        let to = match tile {
            Some(c) if c <= 2 => (c, true),
            Some(_) => return,
            None => (0, false),
        };
        let (from, part) = ((scene.channel, scene.tile_chosen), scene.current);
        if from == to {
            return;
        }
        let name = if to.1 { TILE_CHOSEN } else { ROW_CHOSEN };
        let mut steps = [None; 3];
        if from.0 == to.0 {
            if self.scene_shown() {
                self.scene_stroke_cancel();
            } else {
                self.finish_stroke();
            }
            self.settle();
        } else {
            self.choose_channel(to.0);
            // the step in the channel left, waiting now
            let set = self.scene.as_mut().and_then(|s| s.slot(part, from.0)).and_then(Option::as_mut);
            if let (Some(gpu), Some(set)) = (self.gpu.as_ref(), set)
                && let Some(c) = set.canvas.as_mut()
            {
                c.record_outside_change(name);
                c.render(gpu, &mut set.doc, None);
                if let Some(s) = steps.get_mut(from.0) {
                    *s = c.undo_step();
                }
            }
        }
        if let Some(c) = self.canvas.as_mut() {
            c.record_outside_change(name);
            self.flush();
            if let Some(s) = steps.get_mut(to.0) {
                *s = self.canvas.as_ref().and_then(Canvas::undo_step);
            }
        }
        let Some(scene) = self.scene.as_mut() else { return };
        scene.tile_chosen = to.1;
        if steps.iter().any(Option::is_some) {
            if scene.tile_steps.len() >= MOST_JOINT_STEPS {
                scene.tile_steps.remove(0);
            }
            scene.tile_steps.push(TileStep { part, from, to, steps });
        }
        self.revision += 1;
    }

    /// After an undo or redo (`before`: the session canvas's next undo and redo steps before
    /// it), a tile's choice undone or redone chooses again what was chosen before or after it:
    /// its channel painted, and its step in that channel's canvas taken with it.
    pub(crate) fn follow_tile_step(&mut self, before: (Option<u64>, Option<u64>)) {
        let Some((step, undone)) = self.canvas.as_ref().and_then(|c| stepped(before, c)) else { return };
        let Some(scene) = self.scene.as_ref() else { return };
        let (part, channel) = (scene.current, scene.channel);
        let found = scene.tile_steps.iter().rev().find(|t| t.part == part && t.steps.get(channel) == Some(&Some(step)));
        let Some(&TileStep { from, to, steps, .. }) = found else { return };
        let chosen = if undone { from } else { to };
        let follow = |c: &mut Canvas, want: Option<u64>| {
            let next = if undone { c.undo_step() } else { c.redo_step() };
            if want.is_some() && next == want {
                if undone {
                    c.undo();
                } else {
                    c.redo();
                }
            }
        };
        // the other channel it was made between: chosen again, or its step taken where it waits
        if let Some(other) = [from.0, to.0].into_iter().find(|&c| c != channel) {
            let want = steps.get(other).copied().flatten();
            if chosen.0 == other {
                self.choose_channel(other);
                if let Some(c) = self.canvas.as_mut() {
                    follow(c, want);
                    self.flush();
                }
            } else {
                let set = self.scene.as_mut().and_then(|s| s.slot(part, other)).and_then(Option::as_mut);
                if let (Some(gpu), Some(set)) = (self.gpu.as_ref(), set)
                    && let Some(c) = set.canvas.as_mut()
                {
                    follow(c, want);
                    c.render(gpu, &mut set.doc, None);
                }
            }
        }
        if let Some(scene) = self.scene.as_mut() {
            scene.tile_chosen = chosen.1;
        }
    }

    /// After an undo or redo (`before`: the session canvas's next undo and redo steps before it),
    /// a hide, lock or pick undone or redone puts the model's look back.
    pub(crate) fn follow_look_step(&mut self, before: (Option<u64>, Option<u64>)) {
        let Some((step, undone)) = self.canvas.as_ref().and_then(|c| stepped(before, c)) else { return };
        let Some(scene) = self.scene.as_mut() else { return };
        let (part, channel) = (scene.current, scene.channel);
        let found = scene.look_steps.iter().rev().find(|t| t.part == part && t.channel == channel && t.step == step);
        if let Some(found) = found {
            scene.look = if undone { found.before.clone() } else { found.after.clone() };
        }
    }

    /// The eyedropper on a shown model: the window as it shows round (`x`, `y`), and the colour
    /// painted there, in the channel being painted, on whichever texture set is under the point.
    pub(crate) fn scene_pick(&mut self, x: f32, y: f32, side: u32) -> Option<crate::pick::Pick> {
        let size = self.window_size;
        let s = crate::SURROUND;
        let clear = [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0];
        let revision = self.revision;
        let gpu = self.gpu.as_ref()?;
        let scene = self.scene.as_mut()?;
        let orbit = scene.orbit()?;
        let hit = scene.surface_at(x, y, size);
        // the window drawn once for all the moves of one pick
        let stale = scene.seen.as_ref().is_none_or(|f| f.size != size || f.orbit != orbit || f.revision != revision);
        if stale {
            let view = scene.view.as_mut()?;
            let pixels = view.picture(gpu, size.0, size.1, clear).ok()?;
            scene.seen = Some(SeenFrame { size, orbit, revision, pixels });
        }
        let pixels = scene.seen.as_ref().map(|f| window_round(&f.pixels, size, (x, y), side))?;
        let channel = scene.channel;
        let current = scene.current;
        let rgb = hit.and_then(|hit| {
            let (tx, ty) = texel_of(hit.uv, scene.texture_size);
            let canvas = if hit.part == current {
                self.canvas.as_mut()
            } else {
                scene.slot(hit.part, channel)?.as_mut().and_then(|s| s.canvas.as_mut())
            }?;
            let [r, g, b, _] = canvas.read_composite_pixel(gpu, tx, ty)?;
            Some(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
        });
        Some(crate::pick::Pick { rgb, side, pixels })
    }

    /// A tool that works on the texture, not through the camera (a transform, Liquify, Clone), is
    /// opening: a shown model gives way to its texture, flat, until the tool ends.
    pub(crate) fn scene_flat_for_tool(&mut self) {
        if !self.scene_shown() {
            return;
        }
        self.scene_stroke_cancel();
        if let Some(scene) = self.scene.as_mut() {
            scene.flat = true;
            scene.flat_for_tool = true;
        }
        self.view = Some(self.fitted_view());
    }

    /// The tool that had the texture shown flat has ended: the model shows again.
    pub(crate) fn scene_back_from_tool(&mut self) {
        if let Some(scene) = self.scene.as_mut().filter(|s| s.flat_for_tool) {
            scene.flat = false;
            scene.flat_for_tool = false;
        }
    }

    /// Whether a picture is being placed on the model, and whether window point `at` is on it.
    pub(crate) fn scene_picture_under(&self, at: (f32, f32)) -> Option<bool> {
        let placing = self.scene.as_ref()?.placing.as_ref()?;
        Some(placing.place.holds([at.0, at.1]))
    }

    /// Carries the picture being placed onto the model, if it has moved or the model has.
    fn carry_picture(&mut self) {
        let size = self.window_size;
        let (Some(gpu), Some(scene), Some(canvas)) = (self.gpu.as_ref(), self.scene.as_mut(), self.canvas.as_mut())
        else {
            return;
        };
        scene.tell_view_what_takes_paint();
        let (part, through) = (scene.current, scene.paint_through);
        let (Some(placing), Some(view)) = (scene.placing.as_mut(), scene.view.as_mut()) else { return };
        let now = (placing.place, view.orbit);
        if placing.carried == Some(now) {
            return;
        }
        placing.carried = Some(now);
        view.paint_through = through;
        view.within = canvas.selection_view().cloned();
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("picture on the model") });
        view.project_picture(
            gpu,
            &mut encoder,
            part,
            &placing.view,
            placing.place.to_picture(placing.size),
            size.0,
            size.1,
            canvas.stroke_target(),
        );
        gpu.queue.submit([encoder.finish()]);
        canvas.stroke_drawn(IRect::from_size(view.texture_size(), view.texture_size()));
    }

    /// The documents holding the artwork's layers, each once: the canvas's, or on a model its
    /// texture sets' colours in order, whichever is painted on.
    pub(crate) fn colour_docs(&self) -> Vec<&Document> {
        let Some(scene) = self.scene.as_ref() else { return vec![&self.doc] };
        let own = (scene.channel == 0).then_some(scene.current);
        let waiting = scene.sets.iter().map(|set| set.as_ref().map(|s| &s.doc));
        waiting.enumerate().filter_map(|(i, doc)| if Some(i) == own { Some(&self.doc) } else { doc }).collect()
    }

    /// The selection as fills take it: on a model, kept to the meshes picked.
    pub(crate) fn reach_selection(&mut self) -> Option<procraft_engine::selection::Mask> {
        let selection = self.selection.clone();
        let Some(reach) = self.scene.as_mut().and_then(Scene::reach) else { return selection };
        let Some(mut within) = selection else { return Some(reach) };
        for (c, r) in within.data.iter_mut().zip(&reach.data) {
            *c = ((u16::from(*c) * u16::from(*r) + 127) / 255) as u8;
        }
        Some(within)
    }

    /// An adjustment opens: on a model with meshes picked, it keeps to them, as it does to a
    /// selection, until [`State::scene_selection_back`].
    pub(crate) fn scene_selection_for_adjustment(&mut self) {
        if self.scene.as_mut().and_then(Scene::reach).is_none() {
            return;
        }
        let narrowed = self.reach_selection();
        let held = self.selection.clone();
        if let Some(scene) = self.scene.as_mut() {
            scene.held_selection = Some(held);
        }
        if let (Some(canvas), Some(gpu), Some(mask)) = (self.canvas.as_mut(), self.gpu.as_ref(), narrowed.as_ref()) {
            canvas.set_selection(gpu, Some(&mask.data));
        }
    }

    /// The adjustment has ended: the selection is what it was.
    pub(crate) fn scene_selection_back(&mut self) {
        let Some(held) = self.scene.as_mut().and_then(|s| s.held_selection.take()) else { return };
        self.flush();
        self.selection = held;
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.set_selection(gpu, self.selection.as_ref().map(|m| m.data.as_slice()));
        }
    }

    /// How the selection shows on the model from now on, if there is one.
    pub(crate) fn scene_selection_look(&mut self, look: Option<procraft_gpu::SelectionLook>) {
        let mask = self.canvas.as_ref().and_then(Canvas::selection_view).cloned();
        let Some(scene) = self.scene.as_mut() else { return };
        let part = scene.current;
        let Some(view) = scene.view.as_mut() else { return };
        view.selection = mask.zip(look).map(|(mask, look)| ShownSelection { mask, part, look });
    }

    /// The window polygon `window` carried onto the texture set painted on, as a selection: its
    /// surfaces under it nearest the camera, or with X-Ray all of them.
    pub(crate) fn scene_mask(&mut self, window: &[Vec2]) -> Option<procraft_engine::selection::Mask> {
        let size = self.window_size;
        let shape = procraft_engine::selection::Mask::polygon(size.0, size.1, window);
        let x_ray = self.x_ray;
        let gpu = self.gpu.as_ref()?;
        let scene = self.scene.as_mut()?;
        scene.tell_view_what_takes_paint();
        let (part, side) = (scene.current, scene.texture_size);
        let view = scene.view.as_mut()?;
        let through = std::mem::replace(&mut view.paint_through, x_ray);
        let data = view.project_coverage(gpu, part, &shape.data, size.0, size.1);
        view.paint_through = through;
        match data {
            Ok(data) => Some(procraft_engine::selection::Mask { width: side, height: side, data }),
            Err(e) => {
                log::error!("a selection on the model: {e}");
                None
            }
        }
    }

    /// Why a stroke on the model can't land, if it can't.
    pub(crate) fn scene_blocked(&self) -> Option<crate::layers::Blocked> {
        self.scene.as_ref().filter(|_| self.scene_shown()).and_then(Scene::why_blocked)
    }

    /// The texel of the texture set painted on under window point (`x`, `y`), on a shown model:
    /// none off the model, or on another texture set's surface.
    pub(crate) fn scene_texel(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        let scene = self.scene.as_ref()?;
        let hit = scene.surface_at(x, y, self.window_size)?;
        (hit.part == scene.current).then(|| texel_of(hit.uv, scene.texture_size))
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
        // opened on its colour, no tile chosen
        scene.tile_chosen = false;
        let Some(mut set) = scene.sets.get_mut(index).and_then(Option::take) else { return };
        let (w, h) = (self.doc.width(), self.doc.height());
        let pending = std::mem::replace(&mut self.pending_pixels, crate::project::opened(set.take_pending()));
        let mut old = TextureSet {
            doc: std::mem::replace(&mut self.doc, set.doc),
            canvas: std::mem::replace(&mut self.canvas, set.canvas),
            pending: crate::project::opened_pixels(pending, w, h),
            fill: None,
        };
        if let Some(gpu) = self.gpu.as_ref() {
            to_waiting(gpu, &mut old);
            if let Some(canvas) = self.canvas.as_mut() {
                to_session(gpu, canvas);
            }
        }
        if let Some(slot) = scene.sets.get_mut(scene.current) {
            *slot = Some(old);
        }
        scene.current = index;
        // meshes picked together share a texture set
        scene.look.active.clear();
        self.ensure_canvas();
    }

    /// A stroke begins on the model: built in window pixels, drawn on the screen's canvas and
    /// carried onto the current texture set's layer.
    pub(crate) fn scene_stroke_begin(&mut self, pointer: PointerKind, samples: &[PenSample]) {
        let current = self.doc.current();
        if self.doc.unseen(current).is_some()
            || self
                .doc
                .layer(current)
                .is_none_or(|l| self.doc.is_locked(l.id) || l.kind == LayerKind::Group || l.text.is_some())
        {
            return;
        }
        let tool = if pointer == PointerKind::StylusEraser { ToolKind::Erase } else { self.tool };
        if self.scene.as_ref().is_some_and(|s| s.why_blocked().is_some() || s.placing.is_some()) {
            return;
        }
        let (brush, mut style, diameter) = self.stroke_brush(tool, current);
        // the brush shows as big as on the texture fitted to the window
        let size = self.window_size;
        let zoom = View::fit((self.doc.width(), self.doc.height()), size).zoom();
        let diameter = diameter * zoom;
        self.strokes = self.strokes.wrapping_add(1);
        self.info.strokes = self.info.strokes.saturating_add(1);
        let seed = self.strokes.wrapping_mul(0x9e37_79b9);
        let mut builder = StrokeBuilder::with_seed(brush.clone(), diameter, pointer.into(), seed)
            .with_input(self.input.clone())
            .scaled(zoom)
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
        // a grain fixed to the canvas is read along the model's three axes (moving grain stays in
        // the stamps drawn on the screen), and shows in the colour's coverage alone
        let grain = stroke_grain(gpu, scene, &brush, style.grain_scale);
        let mut canvas_style = style;
        if grain.is_some() {
            canvas_style.grain = Some(0);
            canvas_style.grain_mode = procraft_engine::brush::GrainMode::Texturized;
        } else if canvas_style.grain_mode == procraft_engine::brush::GrainMode::Texturized {
            canvas_style.grain = None;
        }
        // a stroke changing what's under it works on the layer as the camera sees it, there
        let laid = style.in_place();
        scene.laying = None;
        if laid {
            let Some(canvas) = self.canvas.as_mut() else { return };
            canvas.render(gpu, &mut self.doc, None);
            let Some(layer) = canvas.layer_view(current) else { return };
            let Some(seen) = scene.see_layer(gpu, layer, size) else { return };
            scene.laying = Some(seen);
            canvas_style = StrokeStyle::new(StrokeTool::Laid, [0.0; 3], 1.0);
        }
        let Some((screen_doc, screen)) = scene.screen.as_mut() else { return };
        // the screen's canvas holds its own copies of the brush's shape and grain
        screen.use_sources(gpu, &mut style, &brush);
        screen.begin_stroke(screen_doc.current(), style);
        screen.add_stamps(&stamps);
        scene.bounds = bounds_of(&stamps);
        scene.stroking = true;
        scene.ending = false;
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.begin_drawn_stroke(current, canvas_style);
            if let Some(grain) = grain.as_ref().filter(|_| !laid) {
                canvas.set_stroke_grain(&gpu.device, grain);
            }
        }
        self.stroke = Some(builder);
        if !laid {
            self.begin_material_strokes(&brush, canvas_style, current);
        }
    }

    /// A stroke on a part's colour paints its roughness and metalness too, on the same layer and
    /// with the same coverage: the brush's amount of each through its source (an eraser takes
    /// all three away). A stroke on one of those channels paints only it.
    fn begin_material_strokes(&mut self, brush: &procraft_engine::brush::Brush, style: StrokeStyle, layer: LayerId) {
        let structure = self.doc.clone();
        let (Some(gpu), Some(scene)) = (self.gpu.as_ref(), self.scene.as_mut()) else { return };
        scene.material_paint = None;
        if scene.channel != 0 {
            return;
        }
        let (part, side) = (scene.current, scene.texture_size);
        let channels = [
            (brush.setting("roughness_amount"), brush.setting("roughness_scale"), &brush.roughness),
            (brush.setting("metallic_amount"), brush.setting("metallic_scale"), &brush.metallic),
        ];
        let follows_camera = brush.setting("grain_follows_camera") >= 0.5;
        let mut paint = [MaterialPaint { amount: 0.0, tile: 1.0, source: None, follows_camera }; 2];
        for (k, ((amount, scale, source), out)) in channels.into_iter().zip(&mut paint).enumerate() {
            let value = scene.channel_value(part, k + 1);
            let amount = amount.clamp(0.0, 1.0);
            let key = crate::studio::source_key(source);
            *out = MaterialPaint { amount, tile: (scale * MAX_GRAIN_SCALE).max(4.0), source: key, follows_camera };
            // a channel never painted is the material's value all over (its Base Layer filled with
            // it): a plain stroke of that very value onto that layer changes nothing, and the
            // channel waits to be made until one does
            let made = scene.slot(part, k + 1).is_some_and(|slot| slot.is_some());
            let into_base = layer == base_layer() && structure.layer(layer).is_some();
            if !made && into_base && key.is_none() && style.tool == StrokeTool::Paint && same_texel(amount, value) {
                continue;
            }
            // the channel's document, made now if it never was, with the colour's layers
            let stroke = scene.stroke(gpu, true);
            let Some(slot) = scene.slot(part, k + 1) else { return };
            let set = slot.get_or_insert_with(|| TextureSet::filled(side, &structure, opaque([value; 3])));
            follow_layers(set, &structure);
            if set_canvas(gpu, set, &stroke).is_err() {
                return;
            }
            let Some(canvas) = set.canvas.as_mut() else { return };
            // the colour's coverage, laid down plainly: no blend mode, grain or wet paint of its own
            let plain =
                StrokeStyle { color: [amount; 3], blend: 0, burnt_edges: 0.0, grain: None, wet_pull: None, ..style };
            canvas.begin_drawn_stroke(layer, plain);
            let cached = scene.material_sources.get(k).and_then(|c| c.as_ref()).map(|(key, _)| *key);
            if key.is_some() && cached != key {
                let made = crate::brush_files::material_pixels(source).and_then(|pixels| {
                    procraft_gpu::material_source(gpu, procraft_engine::textures::TEXTURE_SIZE as u32, &pixels)
                });
                if let Some(slot) = scene.material_sources.get_mut(k) {
                    *slot = key.zip(made);
                }
            }
        }
        scene.material_paint = Some(paint);
    }

    pub(crate) fn scene_stroke_add(&mut self, samples: &[PenSample], last: bool) {
        let Some(builder) = self.stroke.as_mut() else { return };
        let stamps = builder.add(&window_samples(samples));
        let drawn = builder.stamp_count();
        let all = if last { self.stroke.take().map(|mut b| b.finish()) } else { None };
        let Some(scene) = self.scene.as_mut() else { return };
        scene.bounds = scene.bounds.union(bounds_of(&stamps));
        if let Some((_, screen)) = scene.screen.as_mut() {
            screen.add_stamps(&stamps);
            if let Some(all) = all {
                scene.bounds = scene.bounds.union(bounds_of(&all));
                // a smudge has already changed what's under it: only the rest of it goes on
                if screen.stroke_tool() == Some(StrokeTool::Smudge) {
                    screen.add_stamps(all.get(drawn..).unwrap_or(&[]));
                } else {
                    screen.replace_stamps(&all);
                }
            }
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
        scene.laying = None;
        if let Some((_, screen)) = scene.screen.as_mut() {
            screen.cancel_stroke();
        }
        if scene.material_paint.take().is_some()
            && let Some(channels) = scene.channels.get_mut(scene.current)
        {
            for canvas in channels.iter_mut().flatten().filter_map(|s| s.canvas.as_mut()) {
                canvas.cancel_stroke();
            }
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
        scene.tell_view_what_takes_paint();
        let (Some((screen_doc, screen)), Some(view), Some(canvas)) =
            (scene.screen.as_mut(), scene.view.as_mut(), self.canvas.as_mut())
        else {
            return;
        };
        screen.render(gpu, screen_doc, None);
        view.paint_through = scene.paint_through;
        view.within = canvas.selection_view().cloned();
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("3D stroke") });
        let seen = screen.layer_view(screen_doc.current());
        let reach = match (scene.laying.as_ref(), seen) {
            (Some(was), Some(now)) => view.project_laid(
                gpu,
                &mut encoder,
                scene.current,
                now,
                was,
                scene.bounds,
                size.0,
                size.1,
                canvas.stroke_target(),
            ),
            _ => view.project_stroke(
                gpu,
                &mut encoder,
                scene.current,
                screen.stroke_view(),
                scene.bounds,
                size.0,
                size.1,
                canvas.stroke_target(),
            ),
        };
        gpu.queue.submit([encoder.finish()]);
        canvas.stroke_drawn(reach);
        // as the pen lifts, where the stroke really reached in the texture (the colour, roughness
        // and metalness the same): each canvas's undo step keeps only that, not all the texels
        // whose triangles lie under it on the screen (every one of them, where faces share a
        // texture)
        let within = if scene.ending { view.footprint(gpu, canvas.stroke_view(), reach) } else { None };
        // each canvas's step made as the pen lifts, to be undone and redone together
        let made = |c: &mut Canvas, doc: &mut Document| {
            let before = c.steps_made();
            match &within {
                Some(rects) => c.end_stroke_within(rects.clone()),
                None => c.end_stroke(),
            }
            c.render(gpu, doc, None);
            (c.steps_made() > before).then(|| c.undo_step()).flatten()
        };
        // the roughness and metalness take the colour's coverage as it is now, one after the
        // other through the stroke texture they share: each drawn there, then shown (or kept)
        let part = scene.current;
        let mut steps = [None, None, None];
        if let Some((paint, channels)) = scene.material_paint.zip(scene.channels.get_mut(part)) {
            for (k, ((look, set), step)) in
                paint.iter().zip(channels.iter_mut()).zip(steps.iter_mut().skip(1)).enumerate()
            {
                let Some(set) = set.as_mut() else { continue };
                let Some(target) = set.canvas.as_mut() else { continue };
                let source = scene
                    .material_sources
                    .get(k)
                    .and_then(Option::as_ref)
                    .filter(|(key, _)| Some(*key) == look.source)
                    .map(|(_, view)| view);
                let look = procraft_gpu::MaterialLook {
                    amount: look.amount,
                    source,
                    tile: look.tile,
                    follows_camera: look.follows_camera,
                };
                let mut encoder =
                    gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("3D material") });
                view.project_material(gpu, &mut encoder, part, k, canvas.stroke_view(), &look, target.stroke_target());
                gpu.queue.submit([encoder.finish()]);
                target.stroke_drawn(reach);
                if scene.ending {
                    *step = made(target, &mut set.doc);
                } else {
                    target.render(gpu, &mut set.doc, None);
                }
            }
        }
        if scene.ending {
            // the colour last: its stroke is what the others were drawn from
            steps[0] = made(canvas, &mut self.doc);
            if scene.material_paint.take().is_some() && steps.iter().flatten().count() > 1 {
                if scene.joint_steps.len() >= MOST_JOINT_STEPS {
                    scene.joint_steps.remove(0);
                }
                scene.joint_steps.push(JointStep { part, steps });
            }
            screen.cancel_stroke();
            screen.render(gpu, screen_doc, None);
            scene.stroking = false;
            scene.ending = false;
            scene.laying = None;
        }
    }

    /// After an undo or redo (`before`: the session canvas's next undo and redo steps before
    /// it), the rest of a stroke that painted every channel follows: the step it made in each
    /// other channel is undone or redone too, if that is that channel's next step.
    pub(crate) fn follow_joint_step(&mut self, before: (Option<u64>, Option<u64>)) {
        let Some((step, undone)) = self.canvas.as_ref().and_then(|c| stepped(before, c)) else { return };
        let (Some(gpu), Some(scene)) = (self.gpu.as_ref(), self.scene.as_mut()) else { return };
        let (part, channel) = (scene.current, scene.channel);
        let Some(joint) =
            scene.joint_steps.iter().find(|j| j.part == part && j.steps.get(channel) == Some(&Some(step))).copied()
        else {
            return;
        };
        for (k, want) in joint.steps.iter().enumerate() {
            let Some(want) = *want else { continue };
            if k == channel {
                continue;
            }
            let Some(set) = scene.slot(part, k).and_then(Option::as_mut) else { continue };
            let Some(c) = set.canvas.as_mut() else { continue };
            if undone && c.undo_step() == Some(want) {
                c.undo();
            } else if !undone && c.redo_step() == Some(want) {
                c.redo();
            } else {
                continue;
            }
            c.render(gpu, &mut set.doc, None);
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
        self.scene_frame(Some(target), format, size, convert);
    }

    /// Brings the model up to date with its textures and draws it into `target` if there is one.
    fn scene_frame(
        &mut self,
        target: Option<&wgpu::TextureView>,
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
            // the waiting texture sets' canvases (and channels), with the pixels they opened with,
            // drawing in the stroke textures of their kind
            let unmade = |s: &Option<TextureSet>| s.as_ref().is_some_and(|s| s.canvas.is_none());
            let colour = scene.sets.iter().any(unmade).then(|| scene.stroke(gpu, false));
            let material = scene.channels.iter().flatten().any(unmade).then(|| scene.stroke(gpu, true));
            let colours = scene.sets.iter_mut().flatten().map(|s| (s, &colour));
            let channels = scene.channels.iter_mut().flatten().flatten().map(|s| (s, &material));
            for (set, stroke) in colours.chain(channels) {
                if let (None, Some(stroke)) = (set.canvas.as_ref(), stroke) {
                    if let Err(e) = set_canvas(gpu, set, stroke) {
                        log::error!("a texture set's canvas: {e}");
                    }
                    if let TextureSet { canvas: Some(c), doc, .. } = set {
                        c.render(gpu, doc, None);
                    }
                }
            }
        }
        self.carry_stroke();
        self.carry_picture();
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
            // the session's canvas draws its strokes in the texture its kind shares, as the others
            let stroke = scene.stroke(gpu, scene.channel != 0);
            canvas.draw_strokes_in(gpu, &stroke);
        }
        scene.tell_view_what_shows();
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
        let s = crate::SURROUND;
        let clear = [f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), 1.0];
        let studio = scene.studio_open.then(|| *scene.studio.get_or_insert_with(|| view.studio_orbit()));
        let Some(target) = target else {
            gpu.queue.submit([encoder.finish()]);
            return;
        };
        match studio {
            Some(orbit) => view.render_studio(gpu, &mut encoder, target, size.0, size.1, clear, orbit),
            None => view.render(gpu, &mut encoder, target, size.0, size.1, clear),
        }
        gpu.queue.submit([encoder.finish()]);
        // frosted panels over the model: it drawn again, small, and blurred under each
        let glass = !self.backdrops.is_empty() || !self.backdrop_windows.is_empty();
        if let Some(canvas) = self.canvas.as_mut().filter(|_| glass) {
            let backdrop = procraft_gpu::Backdrop {
                regions: &self.backdrops,
                windows: &self.backdrop_windows,
                sigma: crate::BACKDROP_SIGMA_PT * self.display_scale,
            };
            canvas.frost_over(gpu, target, format, size, s, backdrop, &mut |encoder, small, (w, h)| match studio {
                Some(orbit) => view.render_studio(gpu, encoder, small, w, h, clear, orbit),
                None => view.render(gpu, encoder, small, w, h, clear),
            });
        }
    }
}

/// A stroke's grain fixed to the canvas read along the model's three axes into the current
/// part's texture space, one tile every `tile` canvas pixels, if the brush has one and the model
/// has been drawn: for [`Canvas::set_stroke_grain`].
fn stroke_grain(
    gpu: &Gpu,
    scene: &mut Scene,
    brush: &procraft_engine::brush::Brush,
    tile: f32,
) -> Option<wgpu::TextureView> {
    let g = &brush.grain;
    if g.mode != procraft_engine::brush::GrainMode::Texturized || g.depth <= 0.0 {
        return None;
    }
    let key = g.source.as_ref().map(|s| s.id).or_else(|| g.image.map(|i| (1 << 63) | u64::from(i)))?;
    if scene.grain_source.as_ref().map(|(k, _)| *k) != Some(key) {
        let pixels = crate::brush_files::grain_pixels(brush)?;
        let made = procraft_gpu::material_source(gpu, procraft_engine::textures::TEXTURE_SIZE as u32, &pixels);
        scene.grain_source = made.map(|v| (key, v));
    }
    let source = scene.grain_source.as_ref().map(|(_, v)| v)?;
    let look = procraft_gpu::GrainLook {
        source,
        tile: tile.max(1.0),
        follows_camera: brush.setting("grain_follows_camera") >= 0.5,
    };
    let view = scene.view.as_mut()?;
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("3D grain") });
    let grain = view.project_grain(gpu, &mut encoder, scene.current, &look);
    gpu.queue.submit([encoder.finish()]);
    grain
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
            roll: s.roll,
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
    /// The model's meshes, each in its texture set, in order.
    pub submeshes: Vec<SubmeshInfo>,
    /// Each texture set's side, pixels.
    pub texture_size: u32,
}

/// A texture set's share of a model's layers, as Canvas information lists it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextureSetStats {
    pub name: String,
    /// Its layers with pixels, and how many it can hold.
    pub used: u32,
    pub maximum: u32,
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

/// What each warning is called, as a list in a dialog shows it.
#[uniffi::export]
pub fn model_warning_name(warning: ModelWarning) -> String {
    match warning {
        ModelWarning::OverlappingUvs => "Overlapping UV Coordinates",
        ModelWarning::RepeatingTextures => "Repeating Textures",
    }
    .to_owned()
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
        PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: t, roll: 0.0 }
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

    /// What the current channel of the current texture set starts as (its Base Layer's fill).
    fn fresh(s: &CanvasSession) -> [u8; 4] {
        let st = s.shared.lock();
        let scene = st.scene.as_ref().unwrap();
        let (colour, roughness, metallic) = scene.mesh.surface(scene.current);
        super::opaque(match scene.channel {
            0 => colour,
            1 => [roughness; 3],
            _ => [metallic; 3],
        })
    }

    /// How many of the current texture set's pixels have been painted: changed from what the
    /// model's material gave them.
    fn painted(s: &CanvasSession) -> usize {
        let fresh = fresh(s);
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        canvas.read_composite(gpu).unwrap().chunks(4).filter(|p| *p != fresh).count()
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
    fn a_finger_on_the_model_turns_it_and_the_pen_paints() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let orbit = |s: &CanvasSession| s.shared.lock().scene.as_ref().and_then(|c| c.view.as_ref()).map(|v| v.orbit);
        let before = orbit(&s);
        assert!(before.is_some());
        s.touch_down(1, PointerKind::Finger, vec![sample(70.0, 100.0, 0.0)]);
        s.touch_move(1, vec![sample(130.0, 100.0, 16.0)]);
        let out = s.touch_up(1, vec![sample(130.0, 100.0, 32.0)]);
        assert!(!out.contains(&crate::TouchCommand::StrokeEnded), "{out:?}");
        assert_ne!(orbit(&s), before);
        frame(&s, 200);
        assert_eq!(painted(&s), 0);
        // the pen paints on it
        s.touch_down(2, PointerKind::Stylus, vec![sample(70.0, 100.0, 100.0)]);
        frame(&s, 200);
        s.touch_move(2, (1..16).map(|i| sample(70.0 + i as f32 * 4.0, 100.0, 100.0 + f64::from(i) * 8.0)).collect());
        frame(&s, 200);
        s.touch_up(2, vec![sample(134.0, 100.0, 240.0)]);
        frame(&s, 200);
        let kept = painted(&s);
        assert!(kept > 100);
        // a second finger hands both to the gestures: spread apart, they bring the model nearer
        let distance = |s: &CanvasSession| orbit(s).map_or(0.0, |o| o.distance);
        let far = distance(&s);
        s.touch_down(3, PointerKind::Finger, vec![sample(80.0, 100.0, 300.0)]);
        s.touch_move(3, vec![sample(90.0, 100.0, 308.0)]);
        s.touch_down(4, PointerKind::Finger, vec![sample(120.0, 100.0, 316.0)]);
        s.touch_move(3, vec![sample(60.0, 100.0, 332.0)]);
        s.touch_move(4, vec![sample(150.0, 100.0, 332.0)]);
        let mut out = s.touch_up(4, vec![sample(150.0, 100.0, 348.0)]);
        out.extend(s.touch_up(3, vec![sample(60.0, 100.0, 348.0)]));
        assert!(!out.contains(&crate::TouchCommand::Undo), "{out:?}");
        assert!(distance(&s) < far, "{} from {far}", distance(&s));
        frame(&s, 200);
        assert_eq!(painted(&s), kept);
    }

    /// Two squares facing the camera side by side, Left and Right, one material: Left on the left
    /// half of the texture, Right on the right.
    const TWO_MESHES: &str = "v -2 -1 0\nv -0.2 -1 0\nv -0.2 1 0\nv -2 1 0\n\
                              v 0.2 -1 0\nv 2 -1 0\nv 2 1 0\nv 0.2 1 0\n\
                              vt 0 0\nvt 0.5 0\nvt 0.5 1\nvt 0 1\nvt 0.5 0\nvt 1 0\nvt 1 1\nvt 0.5 1\n\
                              o Left\nf 1/1 2/2 3/3 4/4\no Right\nf 5/5 6/6 7/7 8/8\n";

    /// A pen stroke from (`x0`, `y`) to (`x1`, `y`), each step drawn.
    fn pen_across(s: &CanvasSession, x0: f32, x1: f32, y: f32, t: f64) {
        s.touch_down(7, PointerKind::Stylus, vec![sample(x0, y, t)]);
        frame(s, 200);
        let at = |i: u32| x0 + (x1 - x0) * i as f32 / 20.0;
        s.touch_move(7, (1..20).map(|i| sample(at(i), y, t + f64::from(i) * 8.0)).collect());
        frame(s, 200);
        s.touch_up(7, vec![sample(x1, y, t + 170.0)]);
        frame(s, 200);
    }

    /// How many texels of the current set's left and right halves are painted.
    fn painted_halves(s: &CanvasSession) -> (usize, usize) {
        let fresh = fresh(s);
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let side = st.doc.width() as usize;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        let pixels = canvas.read_composite(gpu).unwrap();
        let on = pixels.chunks(4).enumerate().filter(|(_, p)| *p != fresh).map(|(i, _)| i % side);
        on.fold((0, 0), |(l, r), x| if x < side / 2 { (l + 1, r) } else { (l, r + 1) })
    }

    #[test]
    fn a_stroke_reaches_the_mesh_picked_and_spares_a_locked_one() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        assert_eq!(s.model_info().unwrap().submeshes.len(), 2);
        // a mesh under each side of the window, nothing in the gap or round the edge
        assert_eq!((s.submesh_at(60.0, 100.0), s.submesh_at(140.0, 100.0)), (Some(0), Some(1)));
        assert_eq!((s.submesh_at(100.0, 100.0), s.submesh_at(100.0, 10.0)), (None, None));
        // the left picked: a stroke across both stays on it
        s.pick_submesh(0);
        assert_eq!(s.active_submeshes(), [0]);
        pen_across(&s, 40.0, 160.0, 100.0, 0.0);
        let (left, right) = painted_halves(&s);
        assert!(left > 50 && right == 0, "{left} {right}");
        // both picked, it reaches both
        assert!(s.pick_more_submeshes(1));
        pen_across(&s, 40.0, 160.0, 80.0, 500.0);
        let (more, right) = painted_halves(&s);
        assert!(more > left && right > 50, "{more} {right}");
        // the left locked: it stays as it is, the right goes on taking paint
        s.set_submesh_locked(0, true);
        assert_eq!(s.submeshes_locked(), [true, false]);
        pen_across(&s, 40.0, 160.0, 120.0, 1000.0);
        let (still, grown) = painted_halves(&s);
        assert!(still == more && grown > right, "{still} {grown}");
        // the locks and picks undo as steps of their own
        s.undo();
        assert!(s.submeshes_locked() == [true, false], "the stroke goes first");
        s.undo();
        assert_eq!(s.submeshes_locked(), [false, false]);
        s.redo();
        assert_eq!(s.submeshes_locked(), [true, false]);
        // every mesh meant locked or hidden, a stroke can't land and the session says why
        s.set_submesh_locked(1, true);
        assert_eq!(s.why_blocked(), Some(crate::Blocked::MeshLocked));
        s.set_submesh_hidden(0, true);
        s.set_submesh_hidden(1, true);
        assert_eq!(s.why_blocked(), Some(crate::Blocked::MeshHidden));
        assert_eq!(s.submesh_at(60.0, 100.0), None);
        s.set_submesh_hidden(0, false);
        s.set_submesh_hidden(1, false);
        s.set_submesh_locked(0, false);
        s.set_submesh_locked(1, false);
        assert_eq!(s.why_blocked(), None);
        // a texture set locked or hidden takes its meshes with it
        s.set_texture_set_locked(0, true);
        assert_eq!(s.why_blocked(), Some(crate::Blocked::MeshLocked));
        s.undo();
        assert_eq!(s.texture_sets_locked(), [false]);
        s.set_texture_set_hidden(0, true);
        assert_eq!(s.why_blocked(), Some(crate::Blocked::MeshHidden));
        assert_eq!(s.submesh_at(60.0, 100.0), None);
    }

    /// A pen stroke on the model with `tool`, from (`x0`, `y`) to (`x1`, `y`).
    fn tool_across(s: &CanvasSession, tool: crate::ToolKind, x0: f32, x1: f32, y: f32, t: f64) {
        s.shared.lock().tool = tool;
        pen_across(s, x0, x1, y, t);
    }

    /// The texels of the current set that aren't `like`, with the left half of the texture's
    /// colour most changed, red first.
    fn texels(s: &CanvasSession) -> Vec<[u8; 4]> {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        canvas.read_composite(gpu).unwrap().as_chunks::<4>().0.to_vec()
    }

    #[test]
    fn a_smudge_on_a_model_only_drags_paint_the_way_it_goes() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        s.set_color(1.0, 0.0, 0.0);
        pen_across(&s, 40.0, 80.0, 100.0, 0.0);
        let before = texels(&s);
        let side = 128usize;
        let band = (0..side).find(|y| before[y * side..(y + 1) * side].iter().any(|p| p[1] < 200)).unwrap();
        assert!(band > 30, "{band}");
        // down the model, across the band
        s.shared.lock().tool = crate::ToolKind::Smudge;
        let (x, top, bottom, t) = (60.0, 70.0, 130.0, 1000.0);
        s.touch_down(7, PointerKind::Stylus, vec![sample(x, top, t)]);
        frame(&s, 200);
        let at = |i: u32| top + (bottom - top) * i as f32 / 20.0;
        s.touch_move(7, (1..20).map(|i| sample(x, at(i), t + f64::from(i) * 8.0)).collect());
        frame(&s, 200);
        s.touch_up(7, vec![sample(x, bottom, t + 170.0)]);
        frame(&s, 200);
        let after = texels(&s);
        // what lies before the band stays bare, and what lies past it takes some on
        let upper = (band - 4) * side;
        assert_eq!(before[..upper], after[..upper]);
        let last_row = &after[(side - 8) * side..(side - 7) * side];
        assert!(last_row.iter().any(|p| p[1] < 250), "nothing was dragged past the band");
    }

    #[test]
    fn a_smudge_on_a_model_drags_what_is_painted_along_its_surface() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        s.set_color(1.0, 0.0, 0.0);
        // a red band on the left mesh, then a smudge across it, on and off it
        pen_across(&s, 55.0, 80.0, 100.0, 0.0);
        let before = texels(&s);
        let red = |t: &[[u8; 4]]| t.iter().filter(|p| p[0] > 200 && p[1] < 120).count();
        let pink = |t: &[[u8; 4]]| t.iter().filter(|p| p[1] < 245 && p[1] >= 40).count();
        tool_across(&s, crate::ToolKind::Smudge, 65.0, 88.0, 100.0, 1000.0);
        let after = texels(&s);
        assert_ne!(before, after);
        assert!(pink(&after) > pink(&before), "{} from {}", pink(&after), pink(&before));
        assert!(after.iter().all(|p| p[3] == 255), "the base layer stays opaque");
        // one undo step, whole
        s.undo();
        assert_eq!(texels(&s), before);
        s.redo();
        assert_eq!(texels(&s), after);
        assert!(red(&after) > 0);
        // the right mesh, and what the smudge didn't reach, are as they were
        let side = 128usize;
        let right = |t: &[[u8; 4]]| {
            t.iter().enumerate().filter(|(i, _)| i % side >= side / 2).map(|(_, p)| *p).collect::<Vec<_>>()
        };
        assert_eq!(right(&before), right(&after));
    }

    #[test]
    fn fills_and_adjustments_keep_to_the_mesh_picked() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let base = s.layers().first().unwrap().id;
        s.set_color(1.0, 0.0, 0.0);
        // with nothing picked a fill reaches the whole set
        assert!(s.fill_layer(base));
        frame(&s, 200);
        let (left, right) = painted_halves(&s);
        assert!(left > 500 && right > 500, "{left} {right}");
        s.undo();
        // the left mesh picked, the right stays as it was
        s.pick_submesh(0);
        assert!(s.fill_layer(base));
        frame(&s, 200);
        let (left, right) = painted_halves(&s);
        // (it reaches two texels past its edge, as a stroke does: two columns, here)
        assert!(left > 500 && right <= 2 * 64, "{left} {right}");
        s.undo();
        // an adjustment too, and it leaves no selection behind
        let paint = s.add_layer();
        assert!(s.fill_layer(paint));
        s.pick_submesh(1);
        frame(&s, 200);
        assert!(s.adjustment_start(crate::Adjustment::Hsb));
        s.adjustment_hsb(0.7, 0.5, 0.5);
        frame(&s, 200);
        assert!(s.adjustment_end(true));
        frame(&s, 200);
        assert!(!s.has_selection());
        let t = texels(&s);
        let (red, turned) =
            (t.iter().filter(|p| p[0] > 200 && p[2] < 40).count(), t.iter().filter(|p| p[2] > 100).count());
        assert!(red > 500 && turned > 500, "{red} {turned}");
        // a clear keeps to it as well
        s.clear_layer();
        frame(&s, 200);
        assert!(texels(&s).iter().filter(|p| p[0] > 200 && p[2] < 40).count() > 500);
    }

    #[test]
    fn a_channels_content_is_replaced_by_a_picture_as_an_undo_step() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let base = s.layers().first().unwrap().id;
        let grey = crate::layers::ThumbnailImage { width: 2, height: 2, rgba: [128u8, 128, 128, 255].repeat(4) };
        assert!(s.replace_material_map(base, 1, grey));
        assert_eq!((s.model_channel(), s.channel_tile_chosen()), (1, true));
        frame(&s, 200);
        assert!(texels(&s).iter().all(|p| p[0] == 128 && p[3] == 255));
        s.undo();
        assert!(texels(&s).iter().all(|p| p[0] != 128));
    }

    #[test]
    fn the_reference_window_has_a_camera_of_its_own_on_the_model() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let first = s.reference_model_picture(64).unwrap();
        let seen = s.shared.lock().scene.as_ref().unwrap().orbit();
        s.reference_model_turn(30.0, 0.0, 300.0);
        let turned = s.reference_model_picture(64).unwrap();
        assert_ne!(first.rgba, turned.rgba);
        assert_eq!(s.shared.lock().scene.as_ref().unwrap().orbit(), seen);
        // shown flat, it still shows what is painted
        s.set_model_flat(true);
        s.set_color(1.0, 0.0, 0.0);
        let base = s.layers().first().unwrap().id;
        assert!(s.fill_layer(base));
        let painted = s.reference_model_picture(64).unwrap();
        assert_ne!(painted.rgba, turned.rgba);
    }

    #[test]
    fn a_model_left_showing_itself_in_the_reference_window_opens_on_it_again() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        let mut window = s.reference_window();
        assert_eq!(window.shows, crate::ReferenceShows::Canvas);
        window.shown = true;
        window.shows = crate::ReferenceShows::Model;
        s.set_reference_window(window);
        assert_eq!(s.reference_window(), window);
        let dir = std::env::temp_dir().join(format!("procraft-model-reference-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        s.save(path.clone()).unwrap();
        assert_eq!(CanvasSession::open(path.clone()).unwrap().reference_window(), window);
        // and in the model's own file
        let shared = std::env::temp_dir().join(format!("procraft-model-reference-{}.procraft", std::process::id()));
        let unpacked = std::env::temp_dir().join(format!("procraft-model-reference-in-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&unpacked);
        s.export_model_artwork(path.clone(), shared.to_string_lossy().into_owned()).unwrap();
        assert!(crate::unpack_model_artwork(
            shared.to_string_lossy().into_owned(),
            unpacked.to_string_lossy().into_owned()
        ));
        let opened = CanvasSession::open(unpacked.to_string_lossy().into_owned()).unwrap();
        assert_eq!(opened.reference_window().shows, crate::ReferenceShows::Model);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&unpacked);
        let _ = std::fs::remove_file(&shared);
    }

    #[test]
    fn a_saved_model_turns_in_the_gallerys_preview_without_a_window() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let dir = std::env::temp_dir().join(format!("procraft-model-preview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        s.save(path.clone()).unwrap();
        assert!(crate::project::artwork_plays(path.clone()));
        let opened = CanvasSession::open(path).unwrap();
        assert!(opened.draw_without_window());
        let a = opened.model_preview_picture(0, 64).unwrap();
        let b = opened.model_preview_picture(opened.model_preview_frames() / 4, 64).unwrap();
        assert_eq!(a.rgba.len(), 64 * 64 * 4);
        assert_ne!(a.rgba, b.rgba);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_model_warns_once_of_what_painting_it_may_not_go_well_with() {
        use super::ModelWarning::{OverlappingUvs, RepeatingTextures};
        let open = |text: &str| {
            CanvasSession::open_model(text.as_bytes().to_vec(), "m.obj".into(), Vec::new(), "M".into(), 64).unwrap()
        };
        let s = open(TWO_MESHES);
        assert_eq!(s.take_model_warnings(), []);
        // every face of the cube covers the whole texture
        let s = open(CUBE);
        assert_eq!(s.take_model_warnings(), [OverlappingUvs]);
        assert_eq!(s.take_model_warnings(), []);
        // coordinates past the texture's edge
        let tiled = TWO_MESHES.replace("vt 1 1", "vt 2 1");
        let s = open(&tiled);
        assert!(s.take_model_warnings().contains(&RepeatingTextures));
    }

    #[test]
    fn a_models_artwork_goes_out_as_one_file_and_comes_back_whole() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        pen_across(&s, 40.0, 160.0, 100.0, 0.0);
        s.set_submesh_locked(1, true);
        let painted = painted_halves(&s);
        let work = std::env::temp_dir().join(format!("procraft-model-artwork-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let (dir, file, again) = (work.join("artwork"), work.join("Two.procraft"), work.join("again"));
        std::fs::create_dir_all(&work).unwrap();
        let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
        s.export_model_artwork(path(&dir), path(&file)).unwrap();
        assert_eq!(crate::zip_file_kind(path(&file)), crate::ZipKind::ModelArtwork);
        assert!(super::unpack_model_artwork(path(&file), path(&again)));
        let opened = CanvasSession::open(path(&again)).unwrap();
        assert!(shown(&opened));
        frame(&opened, 200);
        assert_eq!(opened.model_info().unwrap().submeshes.len(), 2);
        assert_eq!(opened.submeshes_locked(), [false, true]);
        assert_eq!(painted_halves(&opened), painted);
        // a file that isn't one is left alone, and nothing leaves the folder given
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add("artwork.json", b"{}");
        zip.add("model.obj", b"");
        zip.add("../outside.txt", b"x");
        std::fs::write(work.join("sly.procraft"), zip.finish()).unwrap();
        assert!(super::unpack_model_artwork(path(&work.join("sly.procraft")), path(&work.join("sly"))));
        assert!(!work.join("outside.txt").exists());
        assert!(!super::unpack_model_artwork(path(&work.join("artwork/artwork.json")), path(&work.join("no"))));
        let _ = std::fs::remove_dir_all(&work);
    }

    const TWO_SETS: &str = "v -2 -1 0\nv -0.2 -1 0\nv -0.2 1 0\nv -2 1 0\nv 0.2 -1 0\nv 2 -1 0\nv 2 1 0\nv 0.2 1 0\n\
                            vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n\
                            usemtl a\no A\nf 1/1 2/2 3/3 4/4\nusemtl b\no B\nf 5/1 6/2 7/3 8/4\n";

    #[test]
    fn canvas_information_counts_a_models_layers_by_texture_set() {
        let s = CanvasSession::open_model(TWO_SETS.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let most = s.layer_stats().maximum;
        let used = |s: &CanvasSession| s.texture_set_stats().iter().map(|t| t.used).collect::<Vec<_>>();
        let names = s.texture_set_stats().into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert_eq!(names, s.model_info().unwrap().texture_sets);
        assert!(s.texture_set_stats().iter().all(|t| t.maximum == most));
        assert_eq!((used(&s), s.layer_stats().used), (vec![1, 1], 2));
        // a layer on the first, counted where it is whichever set or channel is painted on
        assert_ne!(s.add_layer(), 0);
        assert_eq!((used(&s), s.layer_stats().used), (vec![2, 1], 3));
        s.set_model_channel(1);
        assert_eq!((used(&s), s.layer_stats().used), (vec![2, 1], 3));
        s.set_model_channel(0);
        s.choose_texture_set(1);
        assert_eq!((used(&s), s.layer_stats().used), (vec![2, 1], 3));
        assert_eq!(s.layer_stats().maximum, most);
    }

    #[test]
    fn meshes_of_another_texture_set_cant_be_picked_together() {
        let s = CanvasSession::open_model(TWO_SETS.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        assert_eq!(s.model_info().unwrap().texture_sets.len(), 2);
        s.pick_submesh(0);
        assert!(!s.pick_more_submeshes(1));
        assert_eq!(s.active_submeshes(), [0]);
        // picking it alone moves to its texture set
        s.pick_submesh(1);
        assert_eq!((s.active_submeshes(), s.texture_set()), (vec![1], 1));
    }

    #[test]
    fn a_picture_placed_on_a_model_lands_on_a_layer_of_its_own() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let layers = s.layers().len();
        let rgba = [255u8, 0, 0, 255].repeat(16 * 16);
        assert!(s.model_picture_begin(crate::layers::ThumbnailImage { width: 16, height: 16, rgba }, true));
        assert_eq!(s.layers().len(), layers + 1);
        // centred on the window, it starts as a square: over the gap and both meshes
        let corners = s.model_picture_box().unwrap();
        assert_eq!(corners.len(), 8);
        assert!((corners[0] - 30.0).abs() < 1.0 && (corners[2] - 170.0).abs() < 1.0, "{corners:?}");
        frame(&s, 200);
        let (left, right) = painted_halves(&s);
        assert!(left > 50 && right > 50, "{left} {right}");
        // moved over the left mesh it leaves the right one
        s.model_picture_move(-40.0, 0.0, 0.3, 0.0);
        frame(&s, 200);
        let (left, right) = painted_halves(&s);
        assert!(left > 10 && right == 0, "{left} {right}");
        // kept, it stays and the box goes; undo takes it back
        s.model_picture_end(true);
        assert_eq!(s.model_picture_box(), None);
        frame(&s, 200);
        assert_eq!(painted_halves(&s), (left, 0));
        s.undo();
        frame(&s, 200);
        assert_eq!(painted_halves(&s), (0, 0));
        // cancelled, the layer goes too
        s.redo();
        let rgba = [0u8, 255, 0, 255].repeat(4);
        assert!(s.model_picture_begin(crate::layers::ThumbnailImage { width: 2, height: 2, rgba }, true));
        frame(&s, 200);
        s.model_picture_end(false);
        assert_eq!(s.layers().len(), layers + 1);
        frame(&s, 200);
        assert_eq!(painted_halves(&s), (left, 0));
    }

    #[test]
    fn a_transform_on_a_model_shows_its_texture_flat_until_it_ends() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        pen_across(&s, 40.0, 160.0, 100.0, 0.0);
        assert!(!s.model_flat());
        assert!(s.transform_begin().is_some());
        assert!(s.model_flat());
        s.transform_end(false);
        assert!(!s.model_flat());
        // shown flat by the switch, it stays so
        s.set_model_flat(true);
        assert!(s.transform_begin().is_some());
        s.transform_end(false);
        assert!(s.model_flat());
    }

    #[test]
    fn a_placed_pictures_place_maps_the_window_onto_the_picture() {
        let place =
            super::PicturePlace { middle: [100.0, 50.0], half: [40.0, 20.0], turn: std::f32::consts::FRAC_PI_2 };
        // turned a quarter clockwise: its right side points down the window
        let [_, top_right, ..] = place.corners();
        assert!((top_right[0] - 120.0).abs() < 1e-3 && (top_right[1] - 90.0).abs() < 1e-3, "{top_right:?}");
        let [x, y] = place.to_picture((80, 40));
        let at = |p: [f32; 2]| (x[0] * p[0] + x[1] * p[1] + x[2], y[0] * p[0] + y[1] * p[1] + y[2]);
        let (px, py) = at(top_right);
        assert!((px - 80.0).abs() < 1e-3 && py.abs() < 1e-3, "{px} {py}");
        assert!(place.holds([100.0, 50.0]) && !place.holds([50.0, 50.0]));
        // flipped, the same corner reads the other side of the picture
        let flipped = super::PicturePlace { half: [-40.0, 20.0], ..place };
        let [x, _] = flipped.to_picture((80, 40));
        assert!((x[0] * 120.0 + x[1] * 90.0 + x[2]).abs() < 1e-3);
    }

    #[test]
    fn the_eyedropper_reads_what_is_painted_on_the_model_under_it() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        s.set_color(1.0, 0.0, 0.0);
        frame(&s, 200);
        pen_across(&s, 40.0, 160.0, 100.0, 0.0);
        let pick = s.pick(60.0, 100.0, 9).unwrap();
        let rgb = pick.rgb.unwrap();
        assert!(rgb >> 16 > 200 && rgb & 0xff < 60, "{rgb:06x}");
        assert_eq!(pick.pixels.len(), 9 * 9 * 4);
        // what shows there is the model, lit, in the picture round the point
        let middle = &pick.pixels[(4 * 9 + 4) * 4..][..4];
        assert!(middle[0] > middle[2], "{middle:?}");
        // off the model, no colour, but still a picture
        let pick = s.pick(100.0, 10.0, 9).unwrap();
        assert_eq!(pick.rgb, None);
        // where nothing is painted, the model's own white
        let top = (0..100).find(|&y| s.submesh_at(60.0, y as f32).is_some()).unwrap() as f32;
        assert_eq!(s.pick(60.0, top + 1.0, 1).unwrap().rgb, Some(0xffffff));
    }

    /// One square behind another, both filling the same place on the screen: the near one on the
    /// left half of the texture, the far one on the right.
    const FRONT_AND_BACK: &str = "v -2 -2 0\nv 2 -2 0\nv 2 2 0\nv -2 2 0\nv -2 -2 -1\nv 2 -2 -1\nv 2 2 -1\nv -2 2 -1\n\
                                  vt 0 0\nvt 0.5 0\nvt 0.5 1\nvt 0 1\nvt 0.5 0\nvt 1 0\nvt 1 1\nvt 0.5 1\n\
                                  o Near\nf 1/1 2/2 3/3 4/4\no Far\nf 5/5 6/6 7/7 8/8\n";

    /// How much of the selection lies on each half of the texture.
    fn selected_halves(s: &CanvasSession) -> (usize, usize) {
        let st = s.shared.lock();
        let Some(m) = st.selection.as_ref() else { return (0, 0) };
        let side = m.width as usize;
        let on = m.data.iter().enumerate().filter(|(_, c)| **c > 127).map(|(i, _)| i % side);
        on.fold((0, 0), |(l, r), x| if x < side / 2 { (l + 1, r) } else { (l, r + 1) })
    }

    #[test]
    fn a_selection_is_drawn_on_the_screen_and_x_ray_reaches_through_the_model() {
        use crate::SelectionJoin::Add;
        let s =
            CanvasSession::open_model(FRONT_AND_BACK.into(), "two.obj".into(), Vec::new(), "Two".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        // only the near surface takes it
        s.select_shape(false, 80.0, 80.0, 120.0, 120.0, Add);
        let (near, far) = selected_halves(&s);
        assert!(near > 100 && far == 0, "{near} {far}");
        // a stroke across the window stays inside it
        pen_across(&s, 20.0, 180.0, 100.0, 0.0);
        let (left, right) = painted_halves(&s);
        assert!(left > 10 && left <= near && right == 0, "{left} {right} of {near}");
        {
            let st = s.shared.lock();
            let selection = st.selection.as_ref().unwrap();
            let side = selection.width as usize;
            drop(st);
            let fresh = fresh(&s);
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render(gpu, &mut st.doc, None);
            let pixels = canvas.read_composite(gpu).unwrap();
            let selection = st.selection.as_ref().unwrap();
            let outside = pixels.chunks(4).zip(&selection.data).filter(|(p, c)| *p != fresh && **c == 0).count();
            assert_eq!(outside, 0, "{outside} texels painted outside, of {side} square");
        }
        // the selection shows on the model
        let plain = frame_pixels(&s, 200);
        {
            let mut guard = s.shared.lock();
            guard.scene_selection_look(Some(procraft_gpu::SelectionLook::Fill([1.0, 0.0, 0.0])));
        }
        let covered = frame_pixels(&s, 200);
        let at = |p: &[u8], x: usize, y: usize| p[(y * 200 + x) * 4..][..3].to_vec();
        assert_ne!(at(&plain, 100, 100), at(&covered, 100, 100));
        assert_eq!(at(&covered, 100, 100), [255, 0, 0]);
        assert_eq!(at(&plain, 60, 60), at(&covered, 60, 60));
        s.select_none();
        // with X-Ray, the surface behind as well
        s.set_selection_x_ray(true);
        s.select_shape(false, 80.0, 80.0, 120.0, 120.0, Add);
        let (near, far) = selected_halves(&s);
        assert!(near > 100 && far > 100, "{near} {far}");
        // tapped with Automatic, the texel under the touch is what the flood starts from
        s.select_none();
        s.select_by_colour(100.0, 100.0, 0.1, Add, false);
        let (near, far) = selected_halves(&s);
        assert!(near > 0 && far == 0, "{near} {far}");
    }

    #[test]
    fn a_finger_held_still_on_a_model_picks_and_set_to_paint_it_paints() {
        let s = CanvasSession::open_model(TWO_MESHES.into(), "two.obj".into(), Vec::new(), "Two".into(), 128).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        s.touch_down(1, PointerKind::Finger, vec![sample(60.0, 100.0, 0.0)]);
        let out = s.touch_tick(10_000.0);
        assert!(out.iter().any(|c| matches!(c, crate::TouchCommand::Pick { .. })), "{out:?}");
        s.touch_up(1, vec![sample(60.0, 100.0, 10_016.0)]);
        // set to paint, a finger paints and with the modify button held it turns the model
        s.set_touch_settings(crate::TouchSettings { model_finger_paint: true, ..Default::default() });
        s.touch_down(2, PointerKind::Finger, vec![sample(40.0, 100.0, 20_000.0)]);
        frame(&s, 200);
        s.touch_move(2, (1..20).map(|i| sample(40.0 + i as f32 * 6.0, 100.0, 20_000.0 + f64::from(i) * 8.0)).collect());
        frame(&s, 200);
        s.touch_up(2, vec![sample(160.0, 100.0, 20_200.0)]);
        frame(&s, 200);
        assert!(painted(&s) > 50);
        let orbit = |s: &CanvasSession| s.shared.lock().scene.as_ref().and_then(|c| c.view.as_ref()).map(|v| v.orbit);
        let kept = (painted(&s), orbit(&s));
        s.set_modify_held(true);
        s.touch_down(3, PointerKind::Finger, vec![sample(70.0, 100.0, 30_000.0)]);
        s.touch_move(3, vec![sample(130.0, 100.0, 30_016.0)]);
        s.touch_up(3, vec![sample(130.0, 100.0, 30_032.0)]);
        s.set_modify_held(false);
        frame(&s, 200);
        assert_eq!(painted(&s), kept.0);
        assert_ne!(orbit(&s), kept.1);
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
        // roughness: the same layers, the dab on the colour laid down there too
        assert_eq!(s.model_channel(), 0);
        s.set_model_channel(1);
        assert_eq!(s.model_channel(), 1);
        assert!(painted(&s).abs_diff(colour) * 10 < colour, "{} {colour}", painted(&s));
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
        // painted, the metalness painted with the brush's (none, as the plain one)
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

    /// The current channel's texels as painted: each one's grey (unpremultiplied) and alpha.
    fn channel_texels(s: &CanvasSession) -> Vec<(u8, u8)> {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        let px = canvas.read_composite(gpu).unwrap();
        px.chunks(4)
            .map(|p| (if p[3] == 0 { 0 } else { (u16::from(p[0]) * 255 / u16::from(p[3])) as u8 }, p[3]))
            .collect()
    }

    /// The grey most of the current channel's painted texels have (the middle of a dab).
    fn covered_grey(s: &CanvasSession) -> Option<u8> {
        let fresh = fresh(s)[0];
        let mut counts = [0usize; 256];
        for (v, a) in channel_texels(s) {
            if a == 255 && v != fresh {
                counts[usize::from(v)] += 1;
            }
        }
        let (grey, &n) = counts.iter().enumerate().max_by_key(|&(_, n)| *n)?;
        (n > 0).then_some(grey as u8)
    }

    /// Sets the paint brush's material controls as the studio does.
    fn materials(s: &CanvasSession, roughness: f32, metallic: f32) {
        s.studio_begin();
        s.set_studio_setting("roughness_amount".into(), roughness);
        s.set_studio_setting("metallic_amount".into(), metallic);
        s.studio_end(true);
    }

    #[test]
    fn a_stroke_on_the_colour_lays_down_the_brushs_roughness_and_metalness() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        materials(&s, 0.2, 0.8);
        frame(&s, 200);
        dab(&s);
        let colour = painted(&s);
        assert!(colour > 20, "{colour}");
        // each the brush's amount where the stroke covers, nothing elsewhere
        s.set_model_channel(1);
        assert_eq!(covered_grey(&s), Some(51));
        assert!(painted(&s).abs_diff(colour) * 10 < colour, "{} {colour}", painted(&s));
        s.set_model_channel(2);
        assert_eq!(covered_grey(&s), Some(204));
        // a stroke on a channel paints that channel alone, with the channel's own colour
        s.set_color(1.0, 1.0, 1.0);
        s.stroke_begin(PointerKind::Stylus, vec![sample(60.0, 60.0, 0.0)]);
        s.stroke_end(vec![sample(140.0, 60.0, 50.0)]);
        frame(&s, 200);
        let metal = painted(&s);
        assert!(metal > colour);
        s.set_model_channel(1);
        assert!(painted(&s).abs_diff(colour) * 10 < colour);
        s.set_model_channel(0);
        assert_eq!(painted(&s), colour);
    }

    #[test]
    fn the_brushs_material_source_shapes_what_it_lays_down() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        let size = crate::studio::source_size() as usize;
        s.studio_begin();
        // the metalness through a mid grey, the roughness through black
        assert!(s.set_studio_source(crate::studio::SourceKind::Metallic, vec![128; size * size]));
        assert!(s.set_studio_source(crate::studio::SourceKind::Roughness, vec![0; size * size]));
        s.set_studio_setting("metallic_amount".into(), 1.0);
        s.set_studio_setting("roughness_amount".into(), 1.0);
        s.studio_end(true);
        frame(&s, 200);
        dab(&s);
        s.set_model_channel(2);
        let grey = covered_grey(&s).unwrap();
        assert!(grey.abs_diff(128) <= 2, "{grey}");
        s.set_model_channel(1);
        assert_eq!(covered_grey(&s), Some(0));
    }

    #[test]
    fn a_grain_fixed_to_the_canvas_shows_in_the_colour_read_along_the_models_axes() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        // stripes down the grain image, eight to a tile, fully deep and fixed to the canvas
        let n = crate::studio::source_size() as usize;
        let stripes: Vec<u8> =
            (0..n * n).map(|i| if ((i % n) / (n / 8)).is_multiple_of(2) { 0 } else { 255 }).collect();
        s.studio_begin();
        assert!(s.set_studio_source(crate::studio::SourceKind::Grain, stripes));
        s.set_studio_setting("grain_behavior".into(), 1.0);
        s.set_studio_setting("grain_depth".into(), 1.0);
        s.set_studio_setting("grain_scale".into(), 0.5);
        s.set_studio_setting("roughness_amount".into(), 0.4);
        s.studio_end(true);
        frame(&s, 200);
        s.stroke_begin(PointerKind::Stylus, vec![sample(60.0, 100.0, 0.0)]);
        s.stroke_add((1..20).map(|i| sample(60.0 + i as f32 * 4.0, 100.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(140.0, 100.0, 170.0)]);
        frame(&s, 200);
        // the colour has gaps where the grain is dark; the roughness takes the whole stroke
        let colour = painted(&s);
        s.set_model_channel(1);
        let rough = painted(&s);
        s.set_model_channel(0);
        assert!(rough > 100, "{rough}");
        assert!(colour > 20 && colour * 10 < rough * 8, "colour {colour}, roughness {rough}");
        // undone together
        s.undo();
        assert_eq!(painted(&s), 0);
    }

    #[test]
    fn undo_and_redo_take_a_strokes_channels_together() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        materials(&s, 0.4, 0.6);
        frame(&s, 200);
        dab(&s);
        let colour = painted(&s);
        let all = |s: &CanvasSession| {
            let counts: Vec<usize> = (0..3)
                .map(|c| {
                    s.set_model_channel(c);
                    painted(s)
                })
                .collect();
            s.set_model_channel(0);
            counts
        };
        let painted_all = all(&s);
        assert!(painted_all.iter().all(|&n| n > 20), "{painted_all:?}");
        s.undo();
        assert_eq!(all(&s), [0, 0, 0]);
        s.redo();
        assert_eq!(all(&s), painted_all);
        // undone from the roughness, the colour and metalness go too
        s.set_model_channel(1);
        s.undo();
        assert_eq!(painted(&s), 0);
        assert_eq!(all(&s), [0, 0, 0]);
        // an eraser on the colour takes all three away
        s.redo();
        assert_eq!(all(&s), painted_all);
        let cover = |s: &CanvasSession| {
            let sums: Vec<u32> = (0..3)
                .map(|c| {
                    s.set_model_channel(c);
                    channel_texels(s).iter().map(|t| u32::from(t.1)).sum()
                })
                .collect();
            s.set_model_channel(0);
            sums
        };
        let before = cover(&s);
        s.stroke_begin(PointerKind::StylusEraser, vec![sample(80.0, 100.0, 0.0)]);
        s.stroke_end(vec![sample(120.0, 100.0, 50.0)]);
        frame(&s, 200);
        let after = cover(&s);
        assert!(after.iter().zip(&before).all(|(a, b)| a * 10 < b * 9), "{after:?} {before:?}");
        assert_eq!(colour, painted_all[0]);
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
        // decoded into the Base Layer, each channel's
        let mut reader = png::Decoder::new(std::io::Cursor::new(files[0].bytes.clone())).read_info().unwrap();
        let mut px = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut px).unwrap();
        assert_eq!(px.len(), 64 * 64 * 4);
        assert!(!again.set_model_texture(0, 0, vec![0; 8]));
        assert_eq!(again.load_model_textures(), 3);
        assert!(shown(&again));
        frame(&again, 200);
        // the exported picture (opaque, over white) in place of the material's colour
        let shown_px = {
            let mut guard = again.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render(gpu, &mut st.doc, None);
            canvas.read_composite(gpu).unwrap()
        };
        assert!(shown_px == px);
        assert!(painted(&again) > 20);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_models_base_layers_start_as_its_materials_colour_roughness_and_metalness() {
        let two = format!(
            "mtllib m.mtl
usemtl red
{}",
            CUBE.replacen(
                "f 4/1",
                "usemtl mapped
f 4/1",
                1
            )
        );
        let mtl = b"newmtl red
Kd 1 0 0
Pr 0.25
Pm 0.5
newmtl mapped
Kd 0 0 1
map_Kd blue.png
";
        let files = vec![
            ModelFile { name: "m.mtl".into(), bytes: mtl.to_vec() },
            ModelFile { name: "blue.png".into(), bytes: b"an image".to_vec() },
        ];
        let s = CanvasSession::open_model(two.into_bytes(), "two.obj".into(), files, "Two".into(), 16).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let layer = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let id = st.doc.layers().first().unwrap().id;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render(gpu, &mut st.doc, None);
            canvas.read_layer(gpu, id).unwrap()
        };
        // the Base Layer itself, opaque: the colour, and each channel's own value
        assert!(layer(&s).chunks(4).all(|p| p == [255, 0, 0, 255]));
        s.set_model_channel(1);
        assert!(layer(&s).chunks(4).all(|p| p == [64, 64, 64, 255]));
        s.set_model_channel(2);
        assert!(layer(&s).chunks(4).all(|p| p == [128, 128, 128, 255]));
        // a set whose material brings an image waits for it, its Base Layer clear
        s.set_model_channel(0);
        s.choose_texture_set(1);
        assert!(layer(&s).chunks(4).all(|p| p[3] == 0));
        // kept so when saved before it is drawn, and opened again so
        let dir = std::env::temp_dir().join(format!("procraft-fill-{}", std::process::id()));
        s.choose_texture_set(0);
        s.save(dir.to_string_lossy().into_owned()).unwrap();
        let again = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
        assert!(shown(&again));
        frame(&again, 200);
        assert!(layer(&again).chunks(4).all(|p| p == [255, 0, 0, 255]));
        let _ = std::fs::remove_dir_all(&dir);
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

    /// What the current part's roughness and metalness canvases hold on the GPU, bytes (a texture
    /// they share counted once), apart from their undo history; what their histories hold; and
    /// which of the two are made.
    fn channel_bytes(s: &CanvasSession) -> (u64, u64, [bool; 2]) {
        let st = s.shared.lock();
        let scene = st.scene.as_ref().unwrap();
        let pair = scene.channels.get(scene.current).unwrap();
        let mut all: Vec<wgpu::Texture> = Vec::new();
        let mut history = 0;
        for c in pair.iter().flatten().filter_map(|set| set.canvas.as_ref()) {
            history += c.history_bytes();
            for t in c.textures() {
                if !all.contains(&t) {
                    all.push(t);
                }
            }
        }
        (procraft_gpu::textures_bytes(&all) - history, history, [pair[0].is_some(), pair[1].is_some()])
    }

    #[test]
    fn a_models_roughness_and_metalness_hold_their_layers_and_composites_and_share_a_stroke() {
        let side = 512u64;
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 512).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        // the plain brush on a plain material: rough as the brush, as metal as the material
        // (none), so the metalness isn't made
        dab(&s);
        assert_eq!(channel_bytes(&s).2, [true, false]);
        // a stroke under way on both: each its layer and composite (RGBA8) and a few small
        // textures, one stroke texture (half floats) between them, no brush images
        materials(&s, 0.2, 0.8);
        s.stroke_begin(PointerKind::Stylus, vec![sample(90.0, 100.0, 0.0)]);
        frame(&s, 200);
        // the two stand-ins for brush images a texel a layer
        let layers = s.shared.lock().gpu.as_ref().map_or(1, |g| u64::from(g.array_layers_least()));
        let small = 1024 + 1 + 2 * layers;
        let (bytes, _, made) = channel_bytes(&s);
        assert_eq!(made, [true, true]);
        assert_eq!(bytes, 2 * (side * side * 8 + small) + side * side * 8);
        s.stroke_end(vec![sample(110.0, 100.0, 50.0)]);
        frame(&s, 200);
        assert_eq!(channel_bytes(&s).0, bytes);
        // painting the roughness itself gives it its brush images, and back on the colour it
        // lets them go
        s.set_model_channel(1);
        assert!(s.shared.lock().canvas.as_ref().unwrap().has_brush_images());
        s.set_model_channel(0);
        assert_eq!(channel_bytes(&s).0, bytes);
    }

    /// The undo history's bytes of the current part's colour, roughness and metalness.
    fn history_bytes(s: &CanvasSession) -> [u64; 3] {
        let st = s.shared.lock();
        let scene = st.scene.as_ref().unwrap();
        let pair = scene.channels.get(scene.current).unwrap();
        let of = |k: usize| pair[k].as_ref().and_then(|set| set.canvas.as_ref()).map_or(0, |c| c.history_bytes());
        [st.canvas.as_ref().unwrap().history_bytes(), of(0), of(1)]
    }

    #[test]
    fn a_strokes_undo_step_keeps_only_the_texels_it_reached() {
        // every face of the cube covers the whole texture: a dab on the front lies under all of
        // it on the screen, and once took a step of the whole texture in each channel
        let side = 512u32;
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), side).unwrap();
        if !shown(&s) {
            return;
        }
        materials(&s, 0.2, 0.8);
        frame(&s, 200);
        let fresh = fresh(&s);
        dab(&s);
        // the texels it changed, and the tiles (16 texels, a 32nd of the side) they lie in
        let changed: Vec<usize> = {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render(gpu, &mut st.doc, None);
            let px = canvas.read_composite(gpu).unwrap();
            px.chunks(4).enumerate().filter(|(_, p)| *p != fresh).map(|(i, _)| i).collect()
        };
        assert!(changed.len() > 100, "{}", changed.len());
        let tile = 16usize;
        let mut tiles: Vec<usize> = changed
            .iter()
            .map(|i| (i / side as usize / tile) * (side as usize / tile) + (i % side as usize) / tile)
            .collect();
        tiles.sort_unstable();
        tiles.dedup();
        let at_least = (tiles.len() * tile * tile * 4) as u64;
        // each channel's step the same tiles, a few more than those it visibly changed at most
        // (where the stroke is too faint to change a byte), far from the whole texture
        let [colour, rough, metal] = history_bytes(&s);
        assert_eq!((colour, metal), (rough, rough));
        assert!(colour >= at_least && colour <= at_least * 2, "{colour} for {at_least}");
        assert!(colour * 8 < u64::from(side * side * 4), "{colour}");
        // undone and redone as exactly as before
        let all = |s: &CanvasSession| {
            let counts: Vec<usize> = (0..3)
                .map(|c| {
                    s.set_model_channel(c);
                    painted(s)
                })
                .collect();
            s.set_model_channel(0);
            counts
        };
        let before = all(&s);
        s.undo();
        assert_eq!(all(&s), [0, 0, 0]);
        s.redo();
        assert_eq!(all(&s), before);
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
        let dir = std::env::temp_dir().join(format!("procraft-reopen-model-{}", std::process::id()));
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

    #[test]
    fn choosing_a_tile_is_an_undo_step_that_chooses_again_what_was_before() {
        let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
        if !shown(&s) {
            return;
        }
        frame(&s, 200);
        let chosen = |s: &CanvasSession| (s.model_channel(), s.channel_tile_chosen());
        assert_eq!(chosen(&s), (0, false));
        assert!(!s.info().can_undo);
        // the Roughness tile, a stroke on it, the Color tile, the layer's row again
        s.choose_channel_tile(1);
        assert_eq!(chosen(&s), (1, true));
        assert!(s.info().can_undo);
        dab(&s);
        let rough = painted(&s);
        assert!(rough > 20, "{rough}");
        s.choose_channel_tile(0);
        assert_eq!(chosen(&s), (0, true));
        s.leave_channel_tiles();
        assert_eq!(chosen(&s), (0, false));
        // undone one by one, each choice taken back to what was chosen before it
        assert_eq!(s.undo().as_deref(), Some(super::ROW_CHOSEN));
        assert_eq!(chosen(&s), (0, true));
        assert_eq!(s.undo().as_deref(), Some(super::TILE_CHOSEN));
        assert_eq!(chosen(&s), (1, true));
        assert_eq!(painted(&s), rough);
        s.undo();
        assert_eq!((chosen(&s), painted(&s)), ((1, true), 0));
        assert_eq!(s.undo().as_deref(), Some(super::TILE_CHOSEN));
        assert_eq!(chosen(&s), (0, false));
        assert!(!s.info().can_undo);
        // and redone the same way
        s.redo();
        assert_eq!((chosen(&s), painted(&s)), ((1, true), 0));
        s.redo();
        assert_eq!((chosen(&s), painted(&s)), ((1, true), rough));
        s.redo();
        assert_eq!(chosen(&s), (0, true));
        s.redo();
        assert_eq!(chosen(&s), (0, false));
        assert!(!s.info().can_redo);
        // a channel switched to for a moment (as a save does) leaves the steps in order
        s.choose_channel_tile(2);
        s.set_model_channel(0);
        s.set_model_channel(2);
        assert_eq!(s.undo().as_deref(), Some(super::TILE_CHOSEN));
        assert_eq!(chosen(&s), (0, false));
        s.redo();
        assert_eq!(chosen(&s), (2, true));
    }

    #[test]
    fn model_painting_undoes_redoes_and_saves_whole() {
        for seed in [2u32, 30] {
            let s = CanvasSession::open_model(CUBE.into(), "cube.obj".into(), Vec::new(), "Cube".into(), 64).unwrap();
            if !shown(&s) {
                return;
            }
            frame(&s, 200);
            let mut state = seed;
            let mut next = |n: u32| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) % n.max(1)
            };
            // every channel's texture as composited, the channel painted on kept
            let all = |s: &CanvasSession| {
                let kept = s.model_channel();
                let out: Vec<Vec<u8>> = (0..3)
                    .map(|c| {
                        s.set_model_channel(c);
                        let mut guard = s.shared.lock();
                        let st = &mut *guard;
                        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
                        canvas.render_finished(gpu, &mut st.doc);
                        assert_eq!(gpu.errors(), 0, "GPU validation errors");
                        canvas.read_composite(gpu).unwrap()
                    })
                    .collect();
                s.set_model_channel(kept);
                out
            };
            let can = |s: &CanvasSession, undo: bool| {
                let info = s.info();
                if undo { info.can_undo } else { info.can_redo }
            };
            // each channel keeps its own history (a stroke on the colour steps in all three):
            // every channel's undone (or redone) in turn until none has any left; how many
            let through = |s: &CanvasSession, undo: bool| {
                let mut count = 0;
                loop {
                    let before = count;
                    for c in 0..3 {
                        s.set_model_channel(c);
                        while can(s, undo) && count < 400 {
                            if undo {
                                s.undo();
                            } else {
                                s.redo();
                            }
                            count += 1;
                        }
                    }
                    if count == before {
                        break count;
                    }
                }
            };
            let start = all(&s);
            let mut done = Vec::new();
            let mut t = 0.0;
            for _ in 0..30 {
                let op = next(10);
                done.push(op);
                let (x, y) = (60.0 + next(80) as f32, 60.0 + next(80) as f32);
                let (dx, dy) = (next(60) as f32 - 30.0, next(60) as f32 - 30.0);
                t += 1000.0;
                match op {
                    0..=3 => {
                        s.set_tool(
                            [
                                crate::ToolKind::Paint,
                                crate::ToolKind::Paint,
                                crate::ToolKind::Erase,
                                crate::ToolKind::Smudge,
                            ][next(4) as usize],
                        );
                        s.set_brush_size(0.2 + next(60) as f32 / 100.0);
                        s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                        s.stroke_begin(PointerKind::Stylus, vec![sample(x, y, t)]);
                        frame(&s, 200);
                        s.stroke_add(
                            (1..8)
                                .map(|k| {
                                    sample(x + dx * k as f32 / 7.0, y + dy * k as f32 / 7.0, t + f64::from(k) * 8.0)
                                })
                                .collect(),
                        );
                        s.stroke_end(vec![]);
                    }
                    4 => {
                        // a finger turns it
                        s.touch_down(1, PointerKind::Finger, vec![sample(x, y, t)]);
                        s.touch_move(1, vec![sample(x + dx, y + dy, t + 16.0)]);
                        s.touch_up(1, vec![sample(x + dx, y + dy, t + 32.0)]);
                    }
                    5 => match next(3) {
                        0 => s.set_model_channel(next(3)),
                        1 => s.choose_channel_tile(next(3)),
                        _ => s.leave_channel_tiles(),
                    },
                    // from whichever channel is painted on (the others follow its layers)
                    6 => _ = s.add_layer(),
                    7 => {
                        let layers = s.layers();
                        if let Some(l) = layers.get(next(layers.len() as u32) as usize) {
                            _ = s.select_layer(l.id);
                        }
                    }
                    8 => materials(&s, next(100) as f32 / 100.0, next(100) as f32 / 100.0),
                    _ => {
                        if next(2) == 0 {
                            _ = s.undo();
                        } else {
                            _ = s.redo();
                        }
                    }
                }
                frame(&s, 200);
            }
            through(&s, false);
            let end = all(&s);
            let steps = through(&s, true);
            assert!(all(&s) == start, "seed {seed}: undone {steps} steps of {done:?}");
            let again = through(&s, false);
            assert!(all(&s) == end, "seed {seed}: redone {again} steps of {done:?}");
            // saved and opened again
            let dir = std::env::temp_dir().join(format!("procraft-model-work-{}-{seed}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            s.save(dir.to_string_lossy().into_owned()).unwrap();
            let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
            let _ = std::fs::remove_dir_all(&dir);
            assert!(shown(&opened));
            assert!(all(&opened) == end, "seed {seed}: reopened, after {done:?}");
        }
    }
}
