//! Editing the brush library: brushes duplicated, deleted and made new, sets made, renamed and
//! deleted, and the whole library kept between sessions as each brush's origin and changes.

use std::collections::BTreeMap;

use procraft_engine::brush::{
    Author, Brush, FullGrain, GrainFile, Notches, PressureCurves, ResetPoint, ShapeFile, Source, StockImage,
    StockImages,
};
use procraft_engine::brushfile::{self, Texture};
use procraft_engine::curve::Curve;
use procraft_engine::geom::Vec2;
use procraft_engine::library::{BrushSet, SHELF};

use crate::{CanvasSession, State};

/// A set in the library as it is now: the shipped sets as edited, and the user's own.
#[derive(Clone)]
pub(crate) struct LibrarySet {
    pub(crate) name: String,
    pub(crate) brushes: Vec<Brush>,
    /// Where each brush came from: a shipped (set, index), or none for a new brush.
    pub(crate) origins: Vec<Option<(usize, usize)>>,
    /// Which of the set icons it shows.
    pub(crate) icon: u32,
}

/// How many set icons there are: the 42 to choose from, then the six shipped sets' own.
pub const SET_ICONS: u32 = 48;

/// The brush icon, which a new set starts with.
const BRUSH_ICON: u32 = 5;

/// The icon a set of imported brushes starts with.
pub(crate) const IMPORTED_ICON: u32 = BRUSH_ICON;

/// The set icons' numbering as kept. Sets kept under an earlier one have their icon worked out
/// again (their shipped icon, else the brush).
const ICON_NUMBERING: u32 = 2;

/// The icon a shipped set starts with, by its name (the brush icon for the rest).
fn shipped_icon(name: &str) -> u32 {
    match name {
        "Sketching" => 42,
        "Inking" => 43,
        "Painting" => 44,
        "Airbrushing" => 45,
        "Charcoals" => 46,
        "Textures" => 47,
        // later sets show glyphs from the shared choice
        "Drawing" => 1,
        "Watercolour" => 13,
        "Calligraphy" => 27,
        "Spray Paint" => 35,
        "Nature" => 38,
        "Effects" => 32,
        "Pixel" => 9,
        "Elements" => 18,
        "Industrial" => 21,
        "Organic" => 16,
        "Abstract" => 11,
        "Touchups" => 40,
        "Retro" => 15,
        "Vintage" => 37,
        "Luminance" => 34,
        _ => BRUSH_ICON,
    }
}

impl LibrarySet {
    /// The library as it ships, its sets in shelf order.
    pub(crate) fn shipped(sets: &[BrushSet]) -> Vec<LibrarySet> {
        SHELF
            .iter()
            .filter_map(|&s| sets.get(s).map(|set| (s, set)))
            .map(|(s, set)| LibrarySet {
                name: set.name.to_owned(),
                brushes: set.brushes.clone(),
                origins: (0..set.brushes.len()).map(|i| Some((s, i))).collect(),
                icon: shipped_icon(set.name),
            })
            .collect()
    }
}

impl CanvasSession {
    fn shipped_counts(shipped: &[BrushSet]) -> Vec<u32> {
        shipped.iter().map(|set| set.brushes.len() as u32).collect()
    }
}

/// Where the brush that started as shipped `origin` is in `library`, else the first place.
pub(crate) fn place_of(library: &[LibrarySet], origin: (usize, usize)) -> (usize, usize) {
    library
        .iter()
        .enumerate()
        .find_map(|(s, set)| set.origins.iter().position(|o| *o == Some(origin)).map(|i| (s, i)))
        .unwrap_or((0, 0))
}

/// What a new brush starts as.
fn blank_brush() -> Brush {
    Brush { name: "Untitled Brush".to_owned(), ..Brush::round_pen() }
}

/// What a brush file gave a grain, as kept: its image's width and, if it has one at more than the
/// brush holds, that image's id and side.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedGrainFile {
    width: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    full: Option<(u64, u32)>,
}

/// A brush as kept: where it came from, its name, the controls that differ from there and, if it
/// differs, its pressure curve.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedBrush {
    #[serde(default)]
    from: Option<(usize, usize)>,
    name: String,
    #[serde(default)]
    settings: BTreeMap<String, f32>,
    #[serde(default)]
    curve: Vec<[f32; 2]>,
    /// The pressure's own curves for the size and the opacity, if they differ.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    size_curve: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    opacity_curve: Vec<[f32; 2]>,
    /// Its own pressure curves for other controls, by their keys in brush files, if they differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pressure_curves: Option<BTreeMap<String, Vec<[f32; 2]>>>,
    /// The brush's own shape and grain images, as text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shape_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grain_source: Option<String>,
    /// What its file gave the grain beyond that, the image kept in a file of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grain_file: Option<SavedGrainFile>,
    /// A shape at more than that holds, as its file gave it: the id and side of the image, kept in
    /// a file of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shape_full: Option<(u64, u32)>,
    /// The shipped shape and grain images it uses, where they differ from its origin's (-1: none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shape_image: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grain_image: Option<i64>,
    /// The preview its file brought, while it is as it came (PNG, as text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thumbnail: Option<String>,
    /// The values saved on the side sliders while using it.
    #[serde(default, skip_serializing_if = "SavedNotches::is_empty")]
    notches: SavedNotches,
    /// Where its sliders stand for painting, smudging and erasing, as its file brought them.
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    sliders: Vec<Option<[f32; 2]>>,
    /// Who made it, if its file said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    author: Option<SavedAuthor>,
    /// What resetting it puts back, and when that was set aside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reset: Option<SavedReset>,
    /// The stock shape and grain images its file named (and whether inverted): drawn with ours
    /// standing in, found again by name when it is loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shape_stock: Option<(String, bool)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grain_stock: Option<(String, bool)>,
    /// What it paints a model's metalness and roughness through: its own images (as text), the
    /// shipped grains where they differ from its origin's (-1: none), and the stock images its
    /// file named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    metallic_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    roughness_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    metallic_image: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    roughness_image: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    metallic_stock: Option<(String, bool)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    roughness_stock: Option<(String, bool)>,
    /// A dual brush's second, kept whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary: Option<Box<SavedBrush>>,
}

/// A brush's maker as kept: their name, and their picture and signature (PNG, as text).
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedAuthor {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    picture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

/// A reset point as kept: the brush set aside (from the same origin) and when.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedReset {
    made: i64,
    brush: Box<SavedBrush>,
}

/// A brush's slider notches as kept: sizes and opacities for painting, smudging and erasing.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SavedNotches {
    #[serde(default)]
    sizes: [Vec<f32>; 3],
    #[serde(default)]
    opacities: [Vec<f32>; 3],
}

impl SavedNotches {
    fn is_empty(&self) -> bool {
        self.sizes.iter().chain(&self.opacities).all(Vec::is_empty)
    }

    fn of(n: &Notches) -> SavedNotches {
        SavedNotches { sizes: n.sizes.clone(), opacities: n.opacities.clone() }
    }

    /// As notches again: only slider positions, and no more than a slider takes.
    fn notches(&self) -> Notches {
        let clean = |l: &Vec<f32>| -> Vec<f32> {
            l.iter().copied().filter(|v| (0.0..=1.0).contains(v)).take(Notches::MOST).collect()
        };
        Notches { sizes: self.sizes.each_ref().map(clean), opacities: self.opacities.each_ref().map(clean) }
    }
}

/// How a kept brush's images are put: whole (as base64); only by what tells each apart from any
/// other, to see whether the library changed without writing them out; or by the name of a file of
/// its own beside the library, the picture noted in the list to be kept there.
#[derive(Clone, Copy)]
enum Images<'a> {
    Whole,
    Told,
    Filed(&'a std::cell::RefCell<Vec<(String, crate::brush_pictures::Held)>>),
}

impl Images<'_> {
    /// Bytes kept with a brush (a preview, a picture).
    fn bytes(self, bytes: &std::sync::Arc<[u8]>) -> String {
        match self {
            Images::Whole => procraft_engine::base64::encode(bytes),
            Images::Told => format!("{:016x}:{}", crate::project::pixels_hash(bytes), bytes.len()),
            Images::Filed(list) => {
                let name = crate::brush_pictures::bytes_name(bytes);
                let held = crate::brush_pictures::Held::Bytes(std::sync::Arc::clone(bytes));
                list.borrow_mut().push((name.clone(), held));
                format!("{}{name}", crate::brush_pictures::MARK)
            }
        }
    }

    /// A shape, grain or material image of the brush's own.
    fn source(self, source: &Source) -> String {
        match self {
            Images::Whole => source.to_text(),
            Images::Told => format!("{:016x}", source.id),
            Images::Filed(list) => {
                let name = crate::brush_pictures::pixels_name(source.id);
                let held = crate::brush_pictures::Held::Pixels(std::sync::Arc::clone(&source.pixels));
                list.borrow_mut().push((name.clone(), held));
                format!("{}{name}", crate::brush_pictures::MARK)
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedSet {
    name: String,
    brushes: Vec<SavedBrush>,
    /// The set's icon; kept libraries from before have none.
    #[serde(default)]
    icon: Option<u32>,
    /// Which numbering [`Self::icon`] is in (none before the first change of it).
    #[serde(default)]
    icons: u32,
}

/// The sets kept in `json`, as many of them and of their brushes as can be read: in a damaged
/// file a set or a brush that can't be is left out, not the whole library. None if it isn't a
/// list.
fn read_saved_sets(json: &str) -> Option<Vec<SavedSet>> {
    let serde_json::Value::Array(sets) = serde_json::from_str(json).ok()? else { return None };
    let sets = sets.into_iter().filter_map(|set| {
        let serde_json::Value::Object(mut fields) = set else { return None };
        let brushes = match fields.insert("brushes".to_owned(), serde_json::Value::Array(Vec::new())) {
            Some(serde_json::Value::Array(brushes)) => brushes,
            _ => Vec::new(),
        };
        let mut set: SavedSet = serde_json::from_value(serde_json::Value::Object(fields)).ok()?;
        set.brushes = brushes.into_iter().filter_map(read_brush).collect();
        Some(set)
    });
    Some(sets.collect())
}

/// A brush as the library file keeps it; if some of it can't be read, it keeps what can (its
/// settings one by one, then each of its fields), so long as its name can be.
fn read_brush(entry: serde_json::Value) -> Option<SavedBrush> {
    if let Ok(brush) = serde_json::from_value(entry.clone()) {
        return Some(brush);
    }
    let serde_json::Value::Object(mut fields) = entry else { return None };
    if let Some(serde_json::Value::Object(settings)) = fields.get_mut("settings") {
        settings.retain(|_, v| v.is_number());
    }
    let name = fields.get("name")?.as_str()?.to_owned();
    let mut brush = serde_json::json!({ "name": name });
    for (key, value) in fields {
        let mut tried = brush.clone();
        if let Some(o) = tried.as_object_mut() {
            o.insert(key, value);
        }
        if serde_json::from_value::<SavedBrush>(tried.clone()).is_ok() {
            brush = tried;
        }
    }
    serde_json::from_value(brush).ok()
}

impl SavedSet {
    /// Its icon, in today's numbering.
    fn icon_now(&self) -> u32 {
        match self.icon {
            Some(icon) if self.icons == ICON_NUMBERING => icon.min(SET_ICONS - 1),
            _ => shipped_icon(&self.name),
        }
    }
}

/// The brushes in use, as kept between sessions (by place in the library).
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedChoices {
    /// The paint, erase and smudge tools' brushes.
    tools: [(usize, usize); 3],
    /// Each set's last chosen brush, the latest choice last.
    #[serde(default)]
    sets: Vec<(usize, usize)>,
    /// The side sliders' size and opacity per brush and tool (0 paint, 1 erase, 2 smudge).
    #[serde(default)]
    sliders: Vec<(u8, usize, usize, f32, f32)>,
    /// The Recent set: the brushes last painted with, the latest last, and those pinned there.
    #[serde(default)]
    recents: Vec<(usize, usize)>,
    #[serde(default)]
    pinned: Vec<(usize, usize)>,
    /// The brushes QuickMenu buttons are set to, by their keys.
    #[serde(default)]
    quick: Vec<(u64, usize, usize)>,
}

/// How many brushes QuickMenu buttons can be set to at once (the oldest given up past it).
const QUICK_BRUSHES: usize = 64;

/// How many brushes the Recent set keeps besides the pinned ones (as the reference: the oldest
/// gives way to a ninth).
pub(crate) const RECENT_BRUSHES: usize = 8;

/// The tools in the order [`SavedChoices`] keeps them.
const TOOLS: [crate::ToolKind; 3] = [crate::ToolKind::Paint, crate::ToolKind::Erase, crate::ToolKind::Smudge];

/// How many brushes each shipped set had when the library was first kept: a kept library has had
/// the chance to hold those, and what shipped after them joins it once.
const FIRST_SHIPPED: [u32; 6] = [3, 3, 3, 2, 2, 2];
/// The files a library is kept in, in the app's folder for it: the library, the brushes' choices
/// and sliders, and how many brushes of each shipped set it has seen.
const LIBRARY_FILE: &str = "library.json";

/// Held while the library or its choices are read out and written, so that writes from two threads
/// (a window's, one keeping it in the background) land in the order they read it.
static LIBRARY_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn library_writes() -> std::sync::MutexGuard<'static, ()> {
    LIBRARY_WRITES.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The library as last kept or read whole in this run: the next artwork takes it from here while
/// its file is as it was then, not read again (tens of megabytes with many brushes brought in, which
/// took seconds to read on a tablet). One for the app: it keeps one library.
static LAST_LIBRARY: std::sync::Mutex<Option<LastLibrary>> = std::sync::Mutex::new(None);

struct LastLibrary {
    path: std::path::PathBuf,
    /// The file's size and time once kept or read: anything else writing it changes them.
    stamp: (u64, std::time::SystemTime),
    /// What it held (see [`Images::Told`]).
    told: u64,
    library: Vec<LibrarySet>,
}

/// Reads the library kept in folder `dir` ahead of the first artwork (while the gallery shows), so
/// that it opens without reading it (see [`CanvasSession::open_brushes`]); nothing if it is held
/// already or there is none. Slow with many brushes brought in: call it off the main thread.
#[uniffi::export]
pub fn prepare_brushes(dir: String) {
    let library = std::path::Path::new(&dir).join(LIBRARY_FILE);
    if holds(&LAST_LIBRARY, &library) || !library.is_file() {
        return;
    }
    let started = std::time::Instant::now();
    if CanvasSession::new(1, 1).load_brush_library_file(library.to_string_lossy().into_owned()) {
        log::info!("the brushes read ahead in {} ms", started.elapsed().as_millis());
    }
}

/// Puts back the library kept in `json` (see [`CanvasSession::load_brush_library`]), the pictures it
/// names in files read from `folder` (beside it) first, not while the canvas waits.
fn load_library(session: &CanvasSession, json: &str, folder: Option<&std::path::Path>) -> bool {
    let Some(saved) = read_saved_sets(json) else { return false };
    if saved.is_empty() {
        return false;
    }
    let pictures = folder.map(|folder| crate::brush_pictures::read(folder, json)).unwrap_or_default();
    let mut st = session.shared.lock();
    let library: Vec<LibrarySet> = saved
        .into_iter()
        .map(|set| {
            let icon = set.icon_now();
            let origins = set.brushes.iter().map(|b| b.from).collect();
            let brushes = set.brushes.into_iter().map(|b| st.restored_with(b, &pictures)).collect();
            LibrarySet { name: set.name, brushes, origins, icon }
        })
        .collect();
    st.library = library;
    st.sliders.clear();
    st.move_brush_refs(Some);
    true
}

/// Whether `slot` holds the library of the file at `path` as the file is now.
fn holds(slot: &std::sync::Mutex<Option<LastLibrary>>, path: &std::path::Path) -> bool {
    let held = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    held.as_ref().is_some_and(|last| last.path == path && stamp(path) == Some(last.stamp))
}

/// `file`'s size and time, if it is there.
fn stamp(file: &std::path::Path) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(file).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// Holds `library` in `slot` as the file at `path` holds it now.
fn remember(slot: &std::sync::Mutex<Option<LastLibrary>>, path: &std::path::Path, told: u64, library: Vec<LibrarySet>) {
    let held = stamp(path).map(|stamp| LastLibrary { path: path.to_path_buf(), stamp, told, library });
    *slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = held;
}

/// The library held in `slot` for the file at `path`, and what it holds, while the file is as it
/// was when it was held.
fn remembered(slot: &std::sync::Mutex<Option<LastLibrary>>, path: &std::path::Path) -> Option<(Vec<LibrarySet>, u64)> {
    if !holds(slot, path) {
        return None;
    }
    let held = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    held.as_ref().filter(|last| last.path == path).map(|last| (last.library.clone(), last.told))
}
const CHOICES_FILE: &str = "choices.json";
const SEEN_FILE: &str = "seen.txt";

/// Puts what an earlier version kept elsewhere into folder `dir` for [`CanvasSession::open_brushes`]:
/// the shipped brushes it had seen (`seen`, numbers joined by commas) and the brushes in use
/// (`choices`, as [`CanvasSession::saved_brush_choices`] gives), each only where the folder has none.
/// False if one couldn't be written.
#[uniffi::export]
pub fn adopt_kept_brushes(dir: String, seen: Option<String>, choices: Option<String>) -> bool {
    let dir = std::path::PathBuf::from(dir);
    let _ = std::fs::create_dir_all(&dir);
    [(SEEN_FILE, seen), (CHOICES_FILE, choices)].into_iter().all(|(name, text)| {
        let path = dir.join(name);
        match text {
            Some(text) if !path.exists() => std::fs::write(&path, text).is_ok(),
            _ => true,
        }
    })
}

/// One brush as a file to share.
#[derive(serde::Serialize, serde::Deserialize)]
struct BrushFile {
    procraft_brush: u32,
    brush: SavedBrush,
}

/// A set as a file to share.
#[derive(serde::Serialize, serde::Deserialize)]
struct SetFile {
    procraft_brush_set: u32,
    set: SavedSet,
}

/// Whether `text` is one of our brush or set files from before (what
/// [`CanvasSession::import_brush_file`] reads).
pub(crate) fn is_brush_text(text: &str) -> bool {
    serde_json::from_str::<SetFile>(text).is_ok() || serde_json::from_str::<BrushFile>(text).is_ok()
}

impl State {
    /// What brush `origin` started as.
    pub(crate) fn origin_brush(&self, origin: Option<(usize, usize)>) -> Brush {
        origin
            .and_then(|(s, i)| self.shipped.get(s).and_then(|set| set.brushes.get(i)))
            .cloned()
            .unwrap_or_else(blank_brush)
    }

    /// Brush `i` of `set` as kept: its origin and its changes from there.
    fn saved(&self, set: &LibrarySet, i: usize) -> Option<SavedBrush> {
        self.saved_as(set, i, Images::Whole)
    }

    /// [`Self::saved`] with its images put as `images` says.
    fn saved_as(&self, set: &LibrarySet, i: usize, images: Images<'_>) -> Option<SavedBrush> {
        let brush = set.brushes.get(i)?;
        Some(self.saved_brush(brush, set.origins.get(i).copied().flatten(), images))
    }

    /// The library's sets as kept, their images put as `images` says.
    fn saved_sets(&self, images: Images<'_>) -> Vec<SavedSet> {
        self.library
            .iter()
            .map(|set| SavedSet {
                name: set.name.clone(),
                brushes: (0..set.brushes.len()).filter_map(|i| self.saved_as(set, i, images)).collect(),
                icon: Some(set.icon),
                icons: ICON_NUMBERING,
            })
            .collect()
    }

    /// What the library holds, told apart from any other: the library as kept, with each image
    /// by what tells it apart in place of the image (quick, where the whole library written out
    /// is tens of megabytes with many brushes brought in).
    fn library_told(&self) -> u64 {
        crate::project::pixels_hash(&serde_json::to_vec(&self.saved_sets(Images::Told)).unwrap_or_default())
    }

    /// `brush` as kept, as changed from what it started as (`from`), its images put as `images`
    /// says.
    fn saved_brush(&self, brush: &Brush, from: Option<(usize, usize)>, images: Images<'_>) -> SavedBrush {
        let base = self.origin_brush(from);
        let kept = |c: &Curve, b: &Curve| -> Vec<[f32; 2]> {
            if c == b { Vec::new() } else { c.points().iter().map(|p| [p.x, p.y]).collect() }
        };
        let image = |mine: Option<u32>, theirs: Option<u32>| (mine != theirs).then(|| mine.map_or(-1, i64::from));
        let text = |png: &Option<std::sync::Arc<[u8]>>| png.as_ref().map(|bytes| images.bytes(bytes));
        let source = |s: &Source| images.source(s);
        SavedBrush {
            from,
            name: brush.name.clone(),
            settings: brush.changes_from(&base).into_iter().collect(),
            curve: kept(&brush.pressure_curve, &base.pressure_curve),
            size_curve: kept(&brush.size_curve, &base.size_curve),
            opacity_curve: kept(&brush.opacity_curve, &base.opacity_curve),
            pressure_curves: (brush.pressure_curves != base.pressure_curves).then(|| {
                let points = |c: &Curve| c.points().iter().map(|p| [p.x, p.y]).collect();
                brush
                    .pressure_curves
                    .iter()
                    .map(|(k, c)| (brushfile::pressure_curve_key(k).to_owned(), points(c)))
                    .collect()
            }),
            // a stock shape's is made again from its name when the brush comes back
            shape_source: brush.shape.source.as_ref().filter(|_| brush.stock.shape.is_none()).map(source),
            // a stock grain's is made again from its name when the brush comes back
            grain_source: brush.grain.source.as_ref().filter(|_| brush.stock.grain.is_none()).map(source),
            grain_file: brush
                .grain_file_now()
                .map(|f| SavedGrainFile { width: f.width, full: f.full.as_ref().map(|g| (g.id, g.side)) }),
            shape_full: brush.shape_file_now().map(|f| (f.full.id, f.full.side)),
            shape_image: image(brush.shape.image, base.shape.image),
            grain_image: image(brush.grain.image, base.grain.image),
            thumbnail: brush.thumbnail.as_ref().map(|bytes| images.bytes(bytes)),
            notches: SavedNotches::of(&brush.notches),
            sliders: if brush.sliders.iter().any(Option::is_some) { brush.sliders.to_vec() } else { Vec::new() },
            author: brush.author.as_ref().map(|a| SavedAuthor {
                name: a.name.clone(),
                picture: text(&a.picture),
                signature: text(&a.signature),
            }),
            reset: brush
                .reset_point
                .as_ref()
                .map(|r| SavedReset { made: r.made, brush: Box::new(self.saved_brush(&r.brush, from, images)) }),
            shape_stock: brush.stock.shape.as_ref().map(|s| (s.name.clone(), s.inverted)),
            grain_stock: brush.stock.grain.as_ref().map(|s| (s.name.clone(), s.inverted)),
            metallic_source: brush.metallic.source.as_ref().map(source),
            roughness_source: brush.roughness.source.as_ref().map(source),
            metallic_image: image(brush.metallic.image, base.metallic.image),
            roughness_image: image(brush.roughness.image, base.roughness.image),
            metallic_stock: brush.stock.metallic.as_ref().map(|s| (s.name.clone(), s.inverted)),
            roughness_stock: brush.stock.roughness.as_ref().map(|s| (s.name.clone(), s.inverted)),
            secondary: brush.secondary.as_ref().map(|s| Box::new(self.saved_brush(s, None, images))),
        }
    }

    /// A kept brush made again.
    fn restored(&self, b: SavedBrush) -> Brush {
        self.restored_with(b, &std::collections::HashMap::new())
    }

    /// A kept brush made again, the pictures it names in files taken from `pictures` (see
    /// [`crate::brush_pictures::read`]).
    fn restored_with(
        &self,
        b: SavedBrush,
        pictures: &std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>,
    ) -> Brush {
        let picture = |text: &str| crate::brush_pictures::bytes_of(text, pictures);
        let mut brush = self.origin_brush(b.from);
        // where its origin's size controls sit (a size kept there isn't written down)
        let origin_sizes = [("max_size", brush.setting("max_size")), ("min_size", brush.setting("min_size"))];
        if !b.name.trim().is_empty() {
            brush.name = b.name.chars().take(256).collect();
        }
        // the images first: a brush's own shape sets the unit its sizes are kept in
        for (kept, image) in [
            (b.shape_image, &mut brush.shape.image),
            (b.grain_image, &mut brush.grain.image),
            (b.metallic_image, &mut brush.metallic.image),
            (b.roughness_image, &mut brush.roughness.image),
        ] {
            if let Some(k) = kept {
                *image = u32::try_from(k).ok();
            }
        }
        if let Some(text) = &b.shape_source {
            brush.shape.source = crate::brush_pictures::source_of(text, pictures);
        }
        if let Some(text) = &b.grain_source {
            brush.grain.source = crate::brush_pictures::source_of(text, pictures);
        }
        brush.grain_file = b.grain_file.as_ref().zip(brush.grain.source.as_ref()).map(|(f, source)| GrainFile {
            source: source.id,
            width: f.width,
            full: f.full.map(|(id, side)| FullGrain { id, side, pixels: None }),
        });
        brush.shape_file = b
            .shape_full
            .zip(brush.shape.source.as_ref())
            .map(|((id, side), source)| ShapeFile { source: source.id, full: FullGrain { id, side, pixels: None } });
        for (text, material) in [(&b.metallic_source, &mut brush.metallic), (&b.roughness_source, &mut brush.roughness)]
        {
            if let Some(text) = text {
                material.source = crate::brush_pictures::source_of(text, pictures);
            }
        }
        // the stock images its file named, stood in for by our nearest as they are now
        let stock = |kept: &Option<(String, bool)>| {
            kept.as_ref().map(|(name, inverted)| StockImage { name: name.clone(), inverted: *inverted })
        };
        brush.stock = StockImages {
            shape: stock(&b.shape_stock),
            grain: stock(&b.grain_stock),
            metallic: stock(&b.metallic_stock),
            roughness: stock(&b.roughness_stock),
        };
        let png = |text: &Option<String>| text.as_deref().and_then(picture).map(Into::into);
        brush.thumbnail = png(&b.thumbnail);
        if let Some(s) = &brush.stock.shape {
            let texture = Texture::Stock { name: s.name.clone(), inverted: s.inverted };
            (brush.shape.image, brush.shape.source, _) = crate::brush_files::shape_for(&texture);
        }
        if let Some(s) = &brush.stock.grain {
            let texture = Texture::Stock { name: s.name.clone(), inverted: s.inverted };
            (brush.grain.image, brush.grain.source) =
                crate::brush_files::grain_for(&texture, brush.thumbnail.as_deref());
        }
        for (stock, material) in
            [(&brush.stock.metallic, &mut brush.metallic), (&brush.stock.roughness, &mut brush.roughness)]
        {
            if let Some(s) = stock {
                let texture = Texture::Stock { name: s.name.clone(), inverted: s.inverted };
                material.image = crate::brush_files::material_for(&texture).0;
            }
        }
        brush.set_settings(b.settings.iter().map(|(k, v)| (k.as_str(), *v)));
        // a size left out sits where its origin's did: with a shape of its own (another unit) or
        // another spacing (another most), that place is other pixels than its origin's
        for (key, place) in origin_sizes {
            if !b.settings.contains_key(key) {
                brush.set_setting(key, place);
            }
        }
        for (points, curve) in [
            (&b.curve, &mut brush.pressure_curve),
            (&b.size_curve, &mut brush.size_curve),
            (&b.opacity_curve, &mut brush.opacity_curve),
        ] {
            if points.len() >= 2 {
                let pts: Vec<Vec2> = points.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
                *curve = Curve::new(&pts);
            }
        }
        if let Some(curves) = &b.pressure_curves {
            brush.pressure_curves = PressureCurves::default();
            for (key, points) in curves {
                if let Some(kind) = brushfile::pressure_curve_of(key)
                    && points.len() >= 2
                {
                    let pts: Vec<Vec2> = points.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
                    brush.pressure_curves.set(kind, Some(Curve::new(&pts)));
                }
            }
        }
        brush.notches = b.notches.notches();
        for (kept, slider) in brush.sliders.iter_mut().zip(&b.sliders) {
            *kept = slider.filter(|s| s.iter().all(|v| (0.0..=1.0).contains(v)));
        }
        brush.author = b.author.as_ref().map(|a| Author {
            name: a.name.clone(),
            picture: png(&a.picture),
            signature: png(&a.signature),
        });
        brush.reset_point = b.reset.map(|r| ResetPoint::of(&self.restored_with(*r.brush, pictures), r.made));
        brush.secondary = b.secondary.map(|s| Box::new(self.restored_with(*s, pictures)));
        brush
    }

    /// Whether the library has a brush at `(set, index)`.
    fn holds(&self, (set, index): (usize, usize)) -> bool {
        self.library.get(set).is_some_and(|s| index < s.brushes.len())
    }

    /// The brush last chosen in `set`, if one has been.
    pub(crate) fn set_choice(&self, set: usize) -> Option<usize> {
        self.set_choices.iter().find(|c| c.0 == set).map(|c| c.1)
    }

    /// The brush files' grains anything here can still read: the library's, what ships, the brush
    /// as the studio found it and the last stroke's. Recent, the pinned, the tools' and the
    /// QuickMenu's brushes are places in the library, not brushes of their own, and brushes brought
    /// in have their grain kept in the same breath as they join it.
    pub(crate) fn grains_held(&self) -> std::collections::HashSet<u64> {
        let library = self.library.iter().flat_map(|set| &set.brushes);
        let shipped = self.shipped.iter().flat_map(|set| &set.brushes);
        let studio = self.studio.iter().map(|(_, brush)| brush);
        let last = self.last_stroke.iter().map(|stroke| &stroke.brush);
        library.chain(shipped).chain(studio).chain(last).flat_map(Brush::full_grains).map(|g| g.id).collect()
    }

    /// Notes `place` as the brush last painted with, in the Recent set (a pinned one stays where
    /// it is): to the end if it is there already, else past the others, the oldest giving way.
    pub(crate) fn note_recent(&mut self, place: (usize, usize)) {
        if self.pinned_brushes.contains(&place) {
            return;
        }
        self.recent_brushes.retain(|&p| p != place);
        if self.recent_brushes.len() >= crate::brush_library::RECENT_BRUSHES {
            self.recent_brushes.remove(0);
        }
        self.recent_brushes.push(place);
    }

    /// Notes `place` as its set's last chosen brush.
    pub(crate) fn note_choice(&mut self, place: (usize, usize)) {
        self.set_choices.retain(|c| c.0 != place.0);
        self.set_choices.push(place);
    }

    /// The first brush in the library, for a tool whose brush is gone.
    fn first_brush(&self) -> (usize, usize) {
        (self.library.iter().position(|s| !s.brushes.is_empty()).unwrap_or(0), 0)
    }

    /// Every place that names a brush by its position: the tools' brushes and the kept sliders.
    pub(crate) fn move_brush_refs(&mut self, f: impl Fn((usize, usize)) -> Option<(usize, usize)>) {
        let first = self.first_brush();
        for r in [&mut self.paint_brush, &mut self.erase_brush, &mut self.smudge_brush] {
            *r = f(*r).unwrap_or(first);
        }
        self.sliders = std::mem::take(&mut self.sliders)
            .into_iter()
            .filter_map(|((tool, s, i), v)| f((s, i)).map(|(s, i)| ((tool, s, i), v)))
            .collect();
        // the Recent set's brushes follow theirs, those gone leaving it
        for list in [&mut self.recent_brushes, &mut self.pinned_brushes] {
            let mut moved: Vec<(usize, usize)> = Vec::new();
            for place in std::mem::take(list).into_iter().filter_map(&f) {
                if !moved.contains(&place) {
                    moved.push(place);
                }
            }
            *list = moved;
        }
        let library = &self.library;
        let holds = |&(s, i): &(usize, usize)| library.get(s).is_some_and(|set| i < set.brushes.len());
        self.recent_brushes.retain(holds);
        self.pinned_brushes.retain(holds);
        // two brushes made one can leave a place in both lists: pinned, it shows once, there
        let pinned = &self.pinned_brushes;
        self.recent_brushes.retain(|place| !pinned.contains(place));
        // a QuickMenu button's brush follows it, a brush gone leaving its button with none
        self.quick_brushes = std::mem::take(&mut self.quick_brushes)
            .into_iter()
            .filter_map(|(key, place)| f(place).filter(|p| holds(p)).map(|p| (key, p)))
            .collect();
        // a set's choice follows its brush: a set it leaves has none, and a set it joins keeps the
        // later choice
        for place in std::mem::take(&mut self.set_choices).into_iter().filter_map(&f) {
            if self.holds(place) {
                self.note_choice(place);
            }
        }
        // a tool left on a set with no brushes takes the first brush there is
        let first = self.first_brush();
        let library = &self.library;
        for r in [&mut self.paint_brush, &mut self.erase_brush, &mut self.smudge_brush] {
            if library.get(r.0).is_none_or(|s| r.1 >= s.brushes.len()) {
                *r = first;
            }
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The Recent set: the pinned brushes (as they were pinned), then those last painted with,
    /// the latest first.
    pub fn recent_brushes(&self) -> Vec<crate::brushes::RecentBrush> {
        let st = self.shared.lock();
        let named = |(s, i): (usize, usize), pinned: bool| {
            let b = st.library.get(s)?.brushes.get(i)?;
            Some(crate::brushes::RecentBrush {
                set: u32::try_from(s).ok()?,
                index: u32::try_from(i).ok()?,
                name: b.name.clone(),
                pinned,
            })
        };
        let pinned = st.pinned_brushes.iter().filter_map(|&p| named(p, true));
        let recent = st.recent_brushes.iter().rev().filter_map(|&p| named(p, false));
        pinned.chain(recent).collect()
    }

    /// Pins a brush of the Recent set there (out of the brushes that give way), or unpins it
    /// (back among them as the latest). False if it isn't a brush there.
    pub fn pin_recent_brush(&self, set: u32, index: u32, pinned: bool) -> bool {
        let place = (set as usize, index as usize);
        let mut st = self.shared.lock();
        if !st.holds(place) || pinned == st.pinned_brushes.contains(&place) {
            return false;
        }
        if pinned {
            st.recent_brushes.retain(|&p| p != place);
            st.pinned_brushes.push(place);
        } else {
            st.pinned_brushes.retain(|&p| p != place);
            st.note_recent(place);
        }
        true
    }

    /// Takes a brush out of the Recent set, pinned or not. False if it wasn't there.
    pub fn clear_recent_brush(&self, set: u32, index: u32) -> bool {
        let place = (set as usize, index as usize);
        let mut st = self.shared.lock();
        let before = st.recent_brushes.len() + st.pinned_brushes.len();
        st.recent_brushes.retain(|&p| p != place);
        st.pinned_brushes.retain(|&p| p != place);
        st.recent_brushes.len() + st.pinned_brushes.len() < before
    }

    /// The key a QuickMenu button keeps for the brush at `set`, `index` (the same for the same
    /// brush), which then follows the brush about the library; 0 if there is no brush there.
    pub fn quick_brush_key(&self, set: u32, index: u32) -> u64 {
        let place = (set as usize, index as usize);
        let mut st = self.shared.lock();
        if !st.holds(place) {
            return 0;
        }
        if let Some(&(key, _)) = st.quick_brushes.iter().find(|q| q.1 == place) {
            return key;
        }
        let key = st.quick_brushes.iter().map(|q| q.0).max().unwrap_or(0) + 1;
        if st.quick_brushes.len() >= QUICK_BRUSHES {
            st.quick_brushes.remove(0);
        }
        st.quick_brushes.push((key, place));
        key
    }

    /// The brush a QuickMenu button's key stands for, where it is now; none if it is gone.
    pub fn quick_brush(&self, key: u64) -> Option<crate::brushes::BrushInfo> {
        let st = self.shared.lock();
        let &(_, (s, i)) = st.quick_brushes.iter().find(|q| q.0 == key)?;
        let brush = st.library.get(s)?.brushes.get(i)?;
        Some(crate::brushes::BrushInfo {
            set: u32::try_from(s).ok()?,
            index: u32::try_from(i).ok()?,
            name: brush.name.clone(),
        })
    }

    /// Empties the Recent set, the pinned brushes too.
    pub fn clear_recent_brushes(&self) {
        let mut st = self.shared.lock();
        st.recent_brushes.clear();
        st.pinned_brushes.clear();
    }

    /// A copy of a brush just above it in its set; returns the copy's index.
    pub fn duplicate_brush(&self, set: u32, index: u32) -> Option<u32> {
        let mut st = self.shared.lock();
        let (s, i) = (set as usize, index as usize);
        let entry = st.library.get_mut(s)?;
        let brush = entry.brushes.get(i)?.clone();
        let origin = entry.origins.get(i).copied().flatten();
        entry.brushes.insert(i, brush);
        entry.origins.insert(i, origin);
        st.move_brush_refs(|(rs, ri)| Some(if rs == s && ri >= i { (rs, ri + 1) } else { (rs, ri) }));
        Some(index)
    }

    /// Moves a brush to place `to_index` of set `to_set` (counted without it there); a tool that
    /// used it keeps it, wherever it goes.
    pub fn move_brush(&self, set: u32, index: u32, to_set: u32, to_index: u32) -> bool {
        let mut st = self.shared.lock();
        let (s, i, ts) = (set as usize, index as usize, to_set as usize);
        if ts >= st.library.len() {
            return false;
        }
        let Some(entry) = st.library.get_mut(s) else { return false };
        if i >= entry.brushes.len() {
            return false;
        }
        let brush = entry.brushes.remove(i);
        let origin = if i < entry.origins.len() { entry.origins.remove(i) } else { None };
        let Some(target) = st.library.get_mut(ts) else { return false };
        let ti = (to_index as usize).min(target.brushes.len());
        target.brushes.insert(ti, brush);
        if ti <= target.origins.len() {
            target.origins.insert(ti, origin);
        } else {
            target.origins.push(origin);
        }
        st.move_brush_refs(|(rs, ri)| {
            if (rs, ri) == (s, i) {
                return Some((ts, ti));
            }
            // out of its old place, then into the new one
            let ri = if rs == s && ri > i { ri - 1 } else { ri };
            Some((rs, if rs == ts && ri >= ti { ri + 1 } else { ri }))
        });
        true
    }

    /// Removes a brush; a tool that used it takes the one now in its place.
    pub fn delete_brush(&self, set: u32, index: u32) -> bool {
        let mut st = self.shared.lock();
        let (s, i) = (set as usize, index as usize);
        let Some(entry) = st.library.get_mut(s) else { return false };
        if i >= entry.brushes.len() {
            return false;
        }
        entry.brushes.remove(i);
        if i < entry.origins.len() {
            entry.origins.remove(i);
        }
        let left = entry.brushes.len();
        // a QuickMenu button set to it is left with none (a tool takes the brush in its place), and
        // it leaves the Recent set
        st.quick_brushes.retain(|q| q.1 != (s, i));
        st.recent_brushes.retain(|&p| p != (s, i));
        st.pinned_brushes.retain(|&p| p != (s, i));
        st.move_brush_refs(|(rs, ri)| match (rs == s, ri.cmp(&i)) {
            (true, std::cmp::Ordering::Greater) => Some((rs, ri - 1)),
            // the brush below moves up into its place, or the one above when it was last
            (true, std::cmp::Ordering::Equal) => (left > 0).then(|| (rs, ri.min(left - 1))),
            _ => Some((rs, ri)),
        });
        true
    }

    /// Brush `second` of `set` becomes brush `first`'s second (a dual brush: two strokes along
    /// the same path): it leaves the set, `first` keeping its place, and a tool that used either
    /// uses the dual brush. False if they are one brush or either is a dual brush already.
    pub fn combine_brushes(&self, set: u32, first: u32, second: u32) -> bool {
        let mut st = self.shared.lock();
        let (s, a, b) = (set as usize, first as usize, second as usize);
        let Some(entry) = st.library.get_mut(s) else { return false };
        let dual = |i: usize| entry.brushes.get(i).is_none_or(|brush| brush.secondary.is_some());
        if a == b || dual(a) || dual(b) {
            return false;
        }
        let joining = entry.brushes.remove(b);
        if b < entry.origins.len() {
            entry.origins.remove(b);
        }
        // where the first is once the second has left
        let at = if a > b { a - 1 } else { a };
        let Some(dual) = entry.brushes.get_mut(at) else { return false };
        dual.secondary = Some(Box::new(joining));
        st.move_brush_refs(|(rs, ri)| {
            if rs != s {
                return Some((rs, ri));
            }
            if ri == b {
                return Some((rs, at));
            }
            Some((rs, if ri > b { ri - 1 } else { ri }))
        });
        true
    }

    /// Dual brush `index` of `set` comes apart: its second goes back into the set just below it.
    /// False if it isn't a dual brush.
    pub fn uncombine_brush(&self, set: u32, index: u32) -> bool {
        let mut st = self.shared.lock();
        let (s, i) = (set as usize, index as usize);
        let Some(entry) = st.library.get_mut(s) else { return false };
        let Some(second) = entry.brushes.get_mut(i).and_then(|brush| brush.secondary.take()) else { return false };
        entry.brushes.insert(i + 1, *second);
        if i < entry.origins.len() {
            entry.origins.insert(i + 1, None);
        }
        st.move_brush_refs(|(rs, ri)| Some(if rs == s && ri > i { (rs, ri + 1) } else { (rs, ri) }));
        true
    }

    /// Dual brush `index` of `set` swaps its two brushes: the second leads (its own settings, and
    /// the colour) and the first follows. False if it isn't a dual brush.
    pub fn swap_dual_brush(&self, set: u32, index: u32) -> bool {
        let mut st = self.shared.lock();
        let Some(brush) = st.library.get_mut(set as usize).and_then(|e| e.brushes.get_mut(index as usize)) else {
            return false;
        };
        let Some(mut second) = brush.secondary.take() else { return false };
        std::mem::swap(brush, &mut *second);
        brush.secondary = Some(second);
        true
    }

    /// Whether brush `index` of `set` is a dual brush.
    pub fn brush_is_dual(&self, set: u32, index: u32) -> bool {
        let st = self.shared.lock();
        st.library.get(set as usize).and_then(|e| e.brushes.get(index as usize)).is_some_and(|b| b.secondary.is_some())
    }

    /// A new plain round brush at the top of `set`, chosen for the current tool; returns its
    /// index.
    pub fn new_brush(&self, set: u32) -> Option<u32> {
        let mut st = self.shared.lock();
        let s = set as usize;
        let entry = st.library.get_mut(s)?;
        entry.brushes.insert(0, blank_brush());
        entry.origins.insert(0, None);
        st.move_brush_refs(|(rs, ri)| Some(if rs == s { (rs, ri + 1) } else { (rs, ri) }));
        let before = st.brush_ref(st.tool);
        st.note_choice(before);
        match st.tool {
            crate::ToolKind::Paint => st.paint_brush = (s, 0),
            crate::ToolKind::Erase => st.erase_brush = (s, 0),
            crate::ToolKind::Smudge => st.smudge_brush = (s, 0),
        }
        st.note_choice((s, 0));
        Some(0)
    }

    /// A new empty set at the top of the library; returns its index.
    pub fn new_brush_set(&self) -> u32 {
        let mut st = self.shared.lock();
        st.library.insert(
            0,
            LibrarySet { name: "Untitled Set".to_owned(), brushes: Vec::new(), origins: Vec::new(), icon: BRUSH_ICON },
        );
        st.move_brush_refs(|(s, i)| Some((s + 1, i)));
        0
    }

    pub fn rename_brush_set(&self, set: u32, name: String) -> bool {
        let name = name.trim();
        let mut st = self.shared.lock();
        match st.library.get_mut(set as usize) {
            Some(entry) if !name.is_empty() => {
                entry.name = name.chars().take(256).collect();
                true
            }
            _ => false,
        }
    }

    /// Removes a set and its brushes (not the last set).
    pub fn delete_brush_set(&self, set: u32) -> bool {
        let mut st = self.shared.lock();
        let s = set as usize;
        if s >= st.library.len() || st.library.len() <= 1 {
            return false;
        }
        st.library.remove(s);
        st.move_brush_refs(|(rs, ri)| match rs.cmp(&s) {
            std::cmp::Ordering::Less => Some((rs, ri)),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some((rs - 1, ri)),
        });
        true
    }

    /// Moves a set to place `to` (counted without it there), its brushes with it; a tool that used
    /// one of them keeps it.
    pub fn move_brush_set(&self, set: u32, to: u32) -> bool {
        let mut st = self.shared.lock();
        let (s, len) = (set as usize, st.library.len());
        if s >= len {
            return false;
        }
        let t = (to as usize).min(len - 1);
        let entry = st.library.remove(s);
        st.library.insert(t, entry);
        st.move_brush_refs(|(rs, ri)| {
            if rs == s {
                return Some((t, ri));
            }
            // out of its old place, then into the new one
            let rs = if rs > s { rs - 1 } else { rs };
            Some((if rs >= t { rs + 1 } else { rs }, ri))
        });
        true
    }

    /// The library as kept between sessions (JSON).
    pub fn saved_brush_library(&self) -> String {
        serde_json::to_string(&self.shared.lock().saved_sets(Images::Whole)).unwrap_or_default()
    }

    /// Keeps the library in the file at `path` (its folder made if missing), replaced whole, the
    /// pictures of its brushes in files of their own beside it (see [`crate::brush_pictures`]), so
    /// that it stays small. Not written again while it holds what was last kept there (or read
    /// from there): a brush chosen, or a slider moved, changes only the choices kept beside it.
    /// False if it couldn't be written.
    pub fn save_brush_library_file(&self, path: String) -> bool {
        let _writing = library_writes();
        let path = std::path::PathBuf::from(path);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
            self.shared.lock().grain_store.use_folder(dir);
        }
        let pictures = std::cell::RefCell::new(Vec::new());
        let (told, json, library) = {
            let st = self.shared.lock();
            let told = st.library_told();
            if path.is_file() && st.library_kept.as_ref().is_some_and(|(at, was)| *at == path && *was == told) {
                return true;
            }
            let json = serde_json::to_string(&st.saved_sets(Images::Filed(&pictures))).unwrap_or_default();
            (told, json, st.library.clone())
        };
        // the pictures first: the library names none that isn't kept
        let folder = path.parent().unwrap_or(std::path::Path::new(".")).join(crate::brush_pictures::FOLDER);
        if !crate::brush_pictures::keep(&folder, &pictures.into_inner()) {
            return false;
        }
        let written = crate::project::write_atomically(&path, |p| {
            std::fs::write(p, json.as_bytes()).map_err(|e| crate::project::file_error("keeping the brushes", e))
        });
        match &written {
            Ok(()) => {
                remember(&LAST_LIBRARY, &path, told, library);
                self.shared.lock().library_kept = Some((path, told));
            }
            Err(e) => log::error!("{e}"),
        }
        written.is_ok()
    }

    /// Puts back a library kept with [`Self::save_brush_library_file`]; false, changing nothing,
    /// if there is none or it can't be read.
    pub fn load_brush_library_file(&self, path: String) -> bool {
        let path = std::path::PathBuf::from(path);
        if let Some(dir) = path.parent() {
            self.shared.lock().grain_store.use_folder(dir);
        }
        // as last kept or read in this run, the file unchanged since
        if let Some((library, told)) = remembered(&LAST_LIBRARY, &path) {
            let mut st = self.shared.lock();
            st.library = library;
            st.sliders.clear();
            st.move_brush_refs(Some);
            st.library_kept = Some((path, told));
            return true;
        }
        let folder = path.parent().map(|dir| dir.join(crate::brush_pictures::FOLDER));
        let loaded = std::fs::read_to_string(&path).is_ok_and(|json| load_library(self, &json, folder.as_deref()));
        if loaded {
            // the file holds this (or more, of what couldn't be read back): kept again once it changes
            let (told, library) = {
                let mut st = self.shared.lock();
                let told = st.library_told();
                st.library_kept = Some((path.clone(), told));
                (told, st.library.clone())
            };
            remember(&LAST_LIBRARY, &path, told, library);
        }
        loaded
    }

    /// Puts back a library kept with [`Self::saved_brush_library`]; false, changing nothing, if it
    /// can't be read.
    pub fn load_brush_library(&self, json: String) -> bool {
        load_library(self, &json, None)
    }

    /// The library as the app kept it in folder `dir` (see [`Self::keep_brushes`]), what shipped
    /// since joining it once, with the tools' brushes, the sets' choices and the brushes' sliders
    /// as they were left; with none kept, the library as it ships. True if a kept one was put back.
    pub fn open_brushes(&self, dir: String) -> bool {
        let dir = std::path::PathBuf::from(dir);
        let library = dir.join(LIBRARY_FILE).to_string_lossy().into_owned();
        let loaded = self.load_brush_library_file(library.clone());
        let seen_file = dir.join(SEEN_FILE);
        let kept_seen = std::fs::read_to_string(&seen_file).ok();
        let seen: Vec<u32> = kept_seen
            .as_deref()
            .map_or_else(|| FIRST_SHIPPED.to_vec(), |t| t.split(',').filter_map(|v| v.trim().parse().ok()).collect());
        let shipped = if loaded { self.add_shipped_brushes(seen.clone()) } else { self.shipped_brush_counts() };
        if shipped != seen || kept_seen.is_none() {
            // kept with them, or the next start would put the library back without them
            if loaded {
                self.save_brush_library_file(library);
            }
            let text = shipped.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            let _ = std::fs::create_dir_all(&dir);
            // whole or not at all: cut short, the next start would add shipped brushes again
            let kept = crate::project::write_atomically(&seen_file, |p| {
                std::fs::write(p, &text).map_err(|e| crate::project::file_error("keeping the brushes seen", e))
            });
            if let Err(e) = kept {
                log::error!("{e}");
            }
        }
        if loaded && let Ok(choices) = std::fs::read_to_string(dir.join(CHOICES_FILE)) {
            self.load_brush_choices(choices);
        }
        if loaded {
            // only with the library back can it be told which grains' and pictures' files nothing
            // uses any more
            let st = self.shared.lock();
            st.grain_store.forget_all_but(&st.grains_held(), &dir.join(LIBRARY_FILE));
            drop(st);
            // (under the library's lock: one being kept meanwhile names its pictures first)
            let _writing = library_writes();
            crate::brush_pictures::forget_unnamed(&dir.join(crate::brush_pictures::FOLDER), &dir.join(LIBRARY_FILE));
        }
        loaded
    }

    /// Keeps the library, the tools' brushes, the sets' choices and the brushes' sliders in folder
    /// `dir`, for [`Self::open_brushes`]. False if they couldn't all be written.
    pub fn keep_brushes(&self, dir: String) -> bool {
        let library =
            self.save_brush_library_file(std::path::Path::new(&dir).join(LIBRARY_FILE).to_string_lossy().into_owned());
        self.keep_brush_choices(dir) && library
    }

    /// Keeps only the tools' brushes, the sets' choices and the brushes' sliders in folder `dir`
    /// (as [`Self::keep_brushes`] does with the library): for a brush chosen, the library as it
    /// was. False if they couldn't be written.
    pub fn keep_brush_choices(&self, dir: String) -> bool {
        let _writing = library_writes();
        let dir = std::path::PathBuf::from(dir);
        let choices = self.saved_brush_choices();
        let _ = std::fs::create_dir_all(&dir);
        let written = crate::project::write_atomically(&dir.join(CHOICES_FILE), |p| {
            std::fs::write(p, choices.as_bytes()).map_err(|e| crate::project::file_error("keeping the brushes", e))
        });
        if let Err(e) = &written {
            log::error!("{e}");
        }
        written.is_ok()
    }

    /// The tools' brushes, the sets' last choices and each brush's sliders, as kept between
    /// sessions (JSON); kept with the library as it is now.
    pub fn saved_brush_choices(&self) -> String {
        let st = self.shared.lock();
        let tool_index = |tool| TOOLS.iter().position(|&t| t == tool).unwrap_or(0) as u8;
        let mut sliders: Vec<_> =
            st.sliders.iter().map(|(&(tool, s, i), v)| (tool_index(tool), s, i, v.size, v.opacity)).collect();
        sliders.sort_by_key(|&(t, s, i, ..)| (t, s, i));
        let choices = SavedChoices {
            tools: TOOLS.map(|tool| st.brush_ref(tool)),
            sets: st.set_choices.clone(),
            sliders,
            recents: st.recent_brushes.clone(),
            pinned: st.pinned_brushes.clone(),
            quick: st.quick_brushes.iter().map(|&(key, (s, i))| (key, s, i)).collect(),
        };
        serde_json::to_string(&choices).unwrap_or_default()
    }

    /// Puts back what [`Self::saved_brush_choices`] kept, after the library it was kept with;
    /// places the library no longer has are left as they are. False if it can't be read.
    pub fn load_brush_choices(&self, json: String) -> bool {
        let Ok(saved) = serde_json::from_str::<serde_json::Value>(&json) else { return false };
        // entry by entry: in a damaged file one that can't be read is left out, not the rest
        fn each<T: serde::de::DeserializeOwned>(saved: &serde_json::Value, key: &str) -> Vec<Option<T>> {
            let list = saved.get(key).and_then(serde_json::Value::as_array).map(Vec::as_slice).unwrap_or_default();
            list.iter().map(|v| serde_json::from_value(v.clone()).ok()).collect()
        }
        let tools: Vec<Option<(usize, usize)>> = each(&saved, "tools");
        let sets: Vec<(usize, usize)> = each(&saved, "sets").into_iter().flatten().collect();
        let sliders: Vec<(u8, usize, usize, f32, f32)> = each(&saved, "sliders").into_iter().flatten().collect();
        let recents: Vec<(usize, usize)> = each(&saved, "recents").into_iter().flatten().collect();
        let pinned: Vec<(usize, usize)> = each(&saved, "pinned").into_iter().flatten().collect();
        let quick: Vec<(u64, usize, usize)> = each(&saved, "quick").into_iter().flatten().collect();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        for (tool, place) in TOOLS.into_iter().zip(tools) {
            let Some(place) = place.filter(|&p| st.holds(p)) else { continue };
            match tool {
                crate::ToolKind::Paint => st.paint_brush = place,
                crate::ToolKind::Erase => st.erase_brush = place,
                crate::ToolKind::Smudge => st.smudge_brush = place,
            }
        }
        st.set_choices.clear();
        for place in sets {
            if st.holds(place) {
                st.note_choice(place);
            }
        }
        st.pinned_brushes.clear();
        for place in pinned {
            if st.holds(place) && !st.pinned_brushes.contains(&place) {
                st.pinned_brushes.push(place);
            }
        }
        st.recent_brushes.clear();
        for place in recents {
            if st.holds(place) {
                st.note_recent(place);
            }
        }
        st.quick_brushes.clear();
        for (key, s, i) in quick {
            if key != 0 && st.holds((s, i)) && !st.quick_brushes.iter().any(|q| q.0 == key) {
                st.quick_brushes.push((key, (s, i)));
            }
        }
        for (tool, s, i, size, opacity) in sliders {
            let Some(&tool) = TOOLS.get(usize::from(tool)) else { continue };
            if st.holds((s, i)) && size.is_finite() && opacity.is_finite() {
                let v = crate::Sliders { size: size.clamp(0.0, 1.0), opacity: opacity.clamp(0.0, 1.0) };
                st.sliders.insert((tool, s, i), v);
            }
        }
        true
    }

    /// Adds to the library what shipped since it was kept. `seen` is how many brushes of each
    /// shipped set it has had the chance to hold (sets past its end hadn't shipped). A set's new
    /// brushes go at the end of the set holding its others (not at all if that set is gone); new
    /// sets go at the end, in shelf order. Nothing the library holds already is added again.
    /// Returns how many brushes each set ships with now.
    pub fn add_shipped_brushes(&self, seen: Vec<u32>) -> Vec<u32> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let holds = |library: &[LibrarySet], origin: (usize, usize)| {
            library.iter().any(|set| set.origins.contains(&Some(origin)))
        };
        // new brushes of sets it had
        for (s, set) in st.shipped.iter().enumerate() {
            let Some(&had) = seen.get(s) else { continue };
            let home = st.library.iter().position(|l| l.origins.iter().flatten().any(|&(from, _)| from == s));
            let Some(home) = home else { continue };
            for (i, brush) in set.brushes.iter().enumerate().skip(had as usize) {
                if holds(&st.library, (s, i)) {
                    continue;
                }
                if let Some(entry) = st.library.get_mut(home) {
                    entry.brushes.push(brush.clone());
                    entry.origins.push(Some((s, i)));
                }
            }
        }
        // sets it never had
        let new: Vec<LibrarySet> = LibrarySet::shipped(&st.shipped)
            .into_iter()
            .filter(|set| {
                set.origins.first().copied().flatten().is_some_and(|(s, _)| {
                    s >= seen.len()
                        && !st.library.iter().any(|l| l.origins.iter().flatten().any(|&(from, _)| from == s))
                })
            })
            .collect();
        st.library.extend(new);
        Self::shipped_counts(&st.shipped)
    }

    /// The brushes the app comes with that were deleted, back: each into the set its fellows are
    /// in (the one of the shipped set's name first), where it ships among them; a set whose
    /// brushes are all gone comes back whole at the end. Brushes changed, moved or kept are left as
    /// they are. How many came back.
    pub fn restore_shipped_brushes(&self) -> u32 {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let mut back = 0u32;
        for set in LibrarySet::shipped(&st.shipped) {
            let Some(s) = set.origins.first().copied().flatten().map(|(s, _)| s) else { continue };
            let fellows = |l: &LibrarySet| l.origins.iter().flatten().any(|&(from, _)| from == s);
            let home = st
                .library
                .iter()
                .position(|l| l.name == set.name && fellows(l))
                .or_else(|| st.library.iter().position(fellows));
            let Some(home) = home else {
                back += set.brushes.len() as u32;
                st.library.push(set);
                continue;
            };
            for (brush, origin) in set.brushes.into_iter().zip(set.origins) {
                let Some(origin) = origin else { continue };
                if st.library.iter().any(|l| l.origins.contains(&Some(origin))) {
                    continue;
                }
                let Some(entry) = st.library.get_mut(home) else { break };
                // after the last of its fellows that ships before it
                let at = entry
                    .origins
                    .iter()
                    .rposition(|o| o.is_some_and(|(from, i)| from == s && i < origin.1))
                    .map_or(0, |p| p + 1)
                    .min(entry.brushes.len());
                entry.brushes.insert(at, brush);
                if at <= entry.origins.len() {
                    entry.origins.insert(at, Some(origin));
                }
                back += 1;
                st.move_brush_refs(|(rs, ri)| Some(if rs == home && ri >= at { (rs, ri + 1) } else { (rs, ri) }));
            }
        }
        back
    }

    /// How many brushes each set ships with.
    pub fn shipped_brush_counts(&self) -> Vec<u32> {
        Self::shipped_counts(&self.shared.lock().shipped)
    }

    /// One brush as a file to share (JSON).
    pub fn brush_file(&self, set: u32, index: u32) -> Option<String> {
        let st = self.shared.lock();
        let brush = st.saved(st.library.get(set as usize)?, index as usize)?;
        serde_json::to_string(&BrushFile { procraft_brush: 1, brush }).ok()
    }

    /// A set as a file to share (JSON).
    pub fn brush_set_file(&self, set: u32) -> Option<String> {
        let st = self.shared.lock();
        let entry = st.library.get(set as usize)?;
        let brushes = (0..entry.brushes.len()).filter_map(|i| st.saved(entry, i)).collect();
        serde_json::to_string(&SetFile {
            procraft_brush_set: 1,
            set: SavedSet { name: entry.name.clone(), brushes, icon: Some(entry.icon), icons: ICON_NUMBERING },
        })
        .ok()
    }

    /// The set's icon (0 to [`SET_ICONS`] − 1).
    pub fn set_brush_set_icon(&self, set: u32, icon: u32) -> bool {
        let mut st = self.shared.lock();
        let Some(entry) = st.library.get_mut(set as usize) else { return false };
        entry.icon = icon.min(SET_ICONS - 1);
        true
    }

    /// A copy of a set (its brushes as they are) just above it; returns the copy's index.
    pub fn duplicate_brush_set(&self, set: u32) -> Option<u32> {
        let mut st = self.shared.lock();
        let s = set as usize;
        let entry = st.library.get(s)?;
        let copy = LibrarySet {
            name: entry.name.clone(),
            brushes: entry.brushes.clone(),
            origins: entry.origins.clone(),
            icon: entry.icon,
        };
        st.library.insert(s, copy);
        st.move_brush_refs(|(rs, ri)| Some(if rs >= s { (rs + 1, ri) } else { (rs, ri) }));
        Some(set)
    }

    /// A shared file of a brush (put at the top of `set`) or of a set (put at the top of the
    /// library); returns the set it went into, or none if the file can't be read.
    pub fn import_brush_file(&self, set: u32, file: String) -> Option<u32> {
        if let Ok(saved) = serde_json::from_str::<SetFile>(&file) {
            let mut st = self.shared.lock();
            let icon = saved.set.icon_now();
            let origins = saved.set.brushes.iter().map(|b| b.from).collect();
            let brushes = saved.set.brushes.into_iter().map(|b| st.restored(b)).collect();
            let name = if saved.set.name.trim().is_empty() { "Untitled Set".to_owned() } else { saved.set.name };
            st.library.insert(0, LibrarySet { name, brushes, origins, icon });
            st.move_brush_refs(|(rs, ri)| Some((rs + 1, ri)));
            return Some(0);
        }
        let saved = serde_json::from_str::<BrushFile>(&file).ok()?;
        let mut st = self.shared.lock();
        let s = set as usize;
        let from = saved.brush.from;
        let brush = st.restored(saved.brush);
        let entry = st.library.get_mut(s)?;
        entry.brushes.insert(0, brush);
        entry.origins.insert(0, from);
        st.move_brush_refs(|(rs, ri)| Some(if rs == s { (rs, ri + 1) } else { (rs, ri) }));
        Some(set)
    }
}

#[cfg(test)]
mod keeping {
    use crate::CanvasSession;

    #[test]
    fn brushes_moved_deleted_and_combined_at_random_leave_every_place_sound() {
        use crate::ToolKind;
        // a few thousand steps of what the library's lists and swipes do, at random (the same
        // each run), the library kept and read back now and then: nothing panics, and every place
        // the app is told of holds a brush
        let dir = std::env::temp_dir().join(format!(
            "procraft-brushes-random-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        ));
        let path = dir.to_string_lossy().into_owned();
        let mut s = CanvasSession::new(32, 32);
        // (what shipped noted, as the apps start)
        assert!(!s.open_brushes(path.clone()));
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = |most: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % u64::from(most.max(1))) as u32
        };
        for step in 0..3000 {
            let sets = s.brush_library();
            let total: usize = sets.iter().map(|s| s.brushes.len()).sum();
            // mostly a set with brushes in it, now and then any place, there or not
            let full: Vec<u32> = (0..sets.len() as u32).filter(|&i| !sets[i as usize].brushes.is_empty()).collect();
            let set = if full.is_empty() || next(5) == 0 {
                next(sets.len() as u32 + 1)
            } else {
                full[next(full.len() as u32) as usize]
            };
            let count = sets.get(set as usize).map_or(0, |s| s.brushes.len() as u32);
            let (index, other) = (next(count + 1), next(count + 1));
            let to_set = next(sets.len() as u32 + 1);
            match next(16) {
                0..=2 => {
                    s.move_brush(set, index, to_set, other);
                }
                3 | 4 if total > 30 => {
                    s.delete_brush(set, index);
                }
                5..=7 if total < 400 => {
                    s.duplicate_brush(set, index);
                }
                8 => {
                    s.combine_brushes(set, index, other);
                }
                9 => {
                    s.uncombine_brush(set, index);
                }
                10 => {
                    s.select_brush(set, index);
                }
                11 => {
                    s.pin_recent_brush(set, index, next(2) == 0);
                    let key = s.quick_brush_key(set, index);
                    let _ = s.quick_brush(key);
                }
                12 if sets.len() < 16 => {
                    if next(2) == 0 {
                        s.new_brush_set();
                    } else if total < 400 {
                        s.duplicate_brush_set(set);
                    }
                }
                // mostly an empty set: a set of brushes deleted now and then
                13 if sets.len() > 6 => {
                    let empty = (0..sets.len() as u32).find(|&i| sets[i as usize].brushes.is_empty());
                    match empty {
                        Some(empty) => {
                            s.delete_brush_set(empty);
                        }
                        None if next(8) == 0 => {
                            s.delete_brush_set(set);
                        }
                        None => {}
                    }
                }
                14 => {
                    s.move_brush_set(set, to_set);
                }
                _ => s.set_tool([ToolKind::Paint, ToolKind::Smudge, ToolKind::Erase][next(3) as usize]),
            }
            // what the app is told points at brushes there are
            let sets = s.brush_library();
            let holds =
                |set: u32, index: u32| sets.get(set as usize).is_some_and(|s| (index as usize) < s.brushes.len());
            let current = s.current_brush();
            assert!(sets.iter().all(|s| s.brushes.is_empty()) || holds(current.set, current.index), "step {step}");
            // each once: the Recent set's list is keyed by place, and a place twice stops it
            let recent = s.recent_brushes();
            let mut places = std::collections::HashSet::new();
            for r in &recent {
                assert!(holds(r.set, r.index), "step {step}: recent {}-{}", r.set, r.index);
                assert!(places.insert((r.set, r.index)), "step {step}: recent {}-{} twice", r.set, r.index);
            }
            for (i, set) in sets.iter().enumerate() {
                if let Some(chosen) = set.chosen {
                    assert!(holds(i as u32, chosen), "step {step}: set {i} chose {chosen}");
                }
            }
            if step % 500 == 499 {
                assert!(s.keep_brushes(path.clone()), "step {step}");
                let before: Vec<usize> = sets.iter().map(|s| s.brushes.len()).collect();
                s = CanvasSession::new(32, 32);
                assert!(s.open_brushes(path.clone()), "step {step}");
                let after: Vec<usize> = s.brush_library().iter().map(|s| s.brushes.len()).collect();
                assert_eq!(before, after, "step {step}: read back as kept");
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_brushes_the_app_comes_with_come_back_where_they_ship_once_deleted() {
        let s = CanvasSession::new(32, 32);
        let before = s.brush_library();
        let names = |s: &CanvasSession, set: usize| -> Vec<String> {
            s.brush_library()
                .get(set)
                .map(|set| set.brushes.iter().map(|b| b.name.clone()).collect())
                .unwrap_or_default()
        };
        let first = names(&s, 0);
        assert!(first.len() >= 3);
        // nothing gone: nothing comes back
        assert_eq!(s.restore_shipped_brushes(), 0);
        // the second brush of the first set, and the whole last set, deleted; a brush kept changed
        assert!(s.delete_brush(0, 1));
        let last = before.len() as u32 - 1;
        let last_name = before[last as usize].name.clone();
        assert!(s.delete_brush_set(last));
        assert!(s.rename_brush_set(0, "Mine".into()));
        let back = s.restore_shipped_brushes();
        assert_eq!(back as usize, 1 + before[last as usize].brushes.len());
        // back where it ships, in its set (renamed, it is still where its fellows are)
        assert_eq!(names(&s, 0), first);
        let library = s.brush_library();
        assert_eq!(library.len(), before.len());
        assert_eq!(library.last().map(|set| set.name.clone()), Some(last_name));
        assert_eq!(library[0].name, "Mine");
        assert_eq!(s.restore_shipped_brushes(), 0, "all there now");
    }

    #[test]
    fn a_librarys_pictures_are_kept_once_in_files_beside_it_and_come_back() {
        use procraft_engine::brush::{ResetPoint, Source};
        use procraft_engine::textures::TEXTURE_SIZE;
        let dir = std::env::temp_dir().join(format!(
            "procraft-brushes-pictures-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        ));
        let path = dir.to_string_lossy().into_owned();
        let (library, folder) = (dir.join(super::LIBRARY_FILE), dir.join(crate::brush_pictures::FOLDER));
        let s = CanvasSession::new(32, 32);
        assert!(s.duplicate_brush(0, 0).is_some());
        let shape = Source::new((0..TEXTURE_SIZE * TEXTURE_SIZE).map(|i| (i % 251) as u8).collect()).unwrap();
        let thumbnail: std::sync::Arc<[u8]> = vec![7u8; 5000].into();
        {
            let mut st = s.shared.lock();
            let b = &mut st.library[0].brushes[0];
            b.name = "Own shape".into();
            b.shape.source = Some(shape.clone());
            b.thumbnail = Some(std::sync::Arc::clone(&thumbnail));
            // a reset point of it as it is: the same pictures
            b.reset_point = Some(ResetPoint::of(&b.clone(), 1));
        }
        assert!(s.keep_brushes(path.clone()));
        let text = std::fs::read_to_string(&library).unwrap();
        assert!(text.contains(crate::brush_pictures::MARK));
        assert!(!text.contains(&shape.to_text()), "the pixels kept apart");
        assert!(text.len() < 200_000, "{}", text.len());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2, "each picture once");

        // back in another session, from the files (the file changed since it was held in memory)
        std::fs::write(&library, format!("{text} ")).unwrap();
        let next = CanvasSession::new(32, 32);
        assert!(next.open_brushes(path.clone()));
        {
            let st = next.shared.lock();
            let b = st.library[0].brushes.iter().find(|b| b.name == "Own shape").unwrap();
            assert_eq!(b.shape.source.as_ref().map(|s| s.id), Some(shape.id));
            assert_eq!(b.thumbnail.as_deref(), Some(&thumbnail[..]));
            let reset = b.reset_point.as_ref().map(|r| r.brush.shape.source.as_ref().map(|s| s.id));
            assert_eq!(reset, Some(Some(shape.id)));
        }
        // a library from before, its pictures inside it: read as it is
        let other = CanvasSession::new(32, 32);
        assert!(other.load_brush_library(s.saved_brush_library()));
        assert!(other.shared.lock().library[0].brushes.iter().any(|b| b.shape.source.as_ref() == Some(&shape)));

        // a picture nothing names any more goes once it is old; those named stay
        let stray = folder.join("0000000000000001.picture");
        std::fs::write(&stray, [1]).unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 24 * 60 * 60);
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            std::fs::File::options().write(true).open(entry.path()).unwrap().set_modified(long_ago).unwrap();
        }
        assert!(CanvasSession::new(32, 32).open_brushes(path));
        assert!(!stray.exists());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_library_kept_in_a_folder_comes_back_with_its_choices() {
        let dir = std::env::temp_dir().join(format!(
            "procraft-brushes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        ));
        let path = dir.to_string_lossy().into_owned();
        let first = CanvasSession::new(32, 32);
        assert!(!first.open_brushes(path.clone()), "nothing kept yet");
        assert!(dir.join("seen.txt").exists(), "what shipped is noted from the start");
        let set = first.new_brush_set();
        assert!(first.rename_brush_set(set, "Mine".into()));
        first.set_brush_size(0.37);
        assert!(first.keep_brushes(path.clone()));

        let next = CanvasSession::new(32, 32);
        assert!(next.open_brushes(path));
        assert!(next.brush_library().iter().any(|s| s.name == "Mine"));
        assert!((next.info().size - 0.37).abs() < 1e-4, "{}", next.info().size);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_library_is_written_again_only_once_it_changes() {
        let dir = std::env::temp_dir().join(format!(
            "procraft-brushes-again-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        ));
        let path = dir.to_string_lossy().into_owned();
        let (library, choices) = (dir.join(super::LIBRARY_FILE), dir.join(super::CHOICES_FILE));
        // a mark after the library's text (a space, still JSON): gone once it is written again
        let mark = || {
            let mut text = std::fs::read(&library).unwrap();
            text.push(b' ');
            std::fs::write(&library, text).unwrap();
        };
        let marked = || std::fs::read(&library).unwrap().ends_with(b" ");
        let s = CanvasSession::new(32, 32);
        assert!(s.keep_brushes(path.clone()));
        mark();
        // a brush chosen and a slider moved: only the choices change
        let before = std::fs::read_to_string(&choices).unwrap();
        assert!(s.select_brush(0, 1));
        s.set_brush_size(0.42);
        assert!(s.keep_brushes(path.clone()));
        assert!(marked(), "the library written again with nothing in it changed");
        assert_ne!(std::fs::read_to_string(&choices).unwrap(), before);
        // a brush added: written
        assert!(s.duplicate_brush(0, 0).is_some());
        assert!(s.keep_brushes(path.clone()));
        assert!(!marked());
        // read back by another session, which keeps it as it was: not written
        let next = CanvasSession::new(32, 32);
        assert!(next.open_brushes(path.clone()));
        mark();
        assert!(next.keep_brushes(path.clone()));
        assert!(marked());
        // gone from the folder: written again
        std::fs::remove_file(&library).unwrap();
        assert!(next.keep_brushes(path.clone()));
        assert!(library.is_file());
        // kept elsewhere: written there
        let other = dir.join("other");
        assert!(next.keep_brushes(other.to_string_lossy().into_owned()));
        assert!(other.join(super::LIBRARY_FILE).is_file());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_chosen_to_import_is_told_apart_by_its_name_and_contents() {
        let s = CanvasSession::new(32, 32);
        let file = s.brush_as_file(0, 0).unwrap_or_default();
        let sets = s.brush_library().len();
        assert_eq!(s.import_any_brushes(file.clone(), Some("Mine.brush".into()), 0), Some(0));
        assert_eq!(s.brush_library().len(), sets + 1, "into a set of imported brushes");
        assert_eq!(s.import_any_brushes(file, None, 0), Some(0), "no name: told by its contents");
        assert_eq!(s.import_any_brushes(vec![1, 2, 3], Some("junk.abr".into()), 0), None);
        assert_eq!(s.import_any_brushes(vec![1, 2, 3], None, 0), None);
    }

    #[test]
    fn the_next_artwork_has_the_library_as_kept_and_a_file_changed_elsewhere_is_read() {
        let dir = std::env::temp_dir().join(format!(
            "procraft-brushes-held-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        ));
        let path = dir.to_string_lossy().into_owned();
        let first = CanvasSession::new(32, 32);
        first.open_brushes(path.clone());
        let set = first.new_brush_set();
        assert!(first.rename_brush_set(set, "Mine".into()));
        let full = first.brush_library().iter().position(|s| s.brushes.len() > 2).unwrap() as u32;
        assert!(first.select_brush(full, 1));
        first.set_brush_size(0.37);
        assert!(first.duplicate_brush(full, 2).is_some());
        assert!(first.keep_brushes(path.clone()));
        let (library, choices) = (first.saved_brush_library(), first.saved_brush_choices());

        let next = CanvasSession::new(32, 32);
        assert!(next.open_brushes(path.clone()));
        assert_eq!(next.saved_brush_library(), library);
        assert_eq!(next.saved_brush_choices(), choices);
        assert!((next.info().size - 0.37).abs() < 1e-4, "{}", next.info().size);

        // the file written by something else: read from it
        let other = CanvasSession::new(32, 32);
        let elsewhere = other.saved_brush_library();
        std::fs::write(dir.join(super::LIBRARY_FILE), &elsewhere).unwrap();
        let after = CanvasSession::new(32, 32);
        assert!(after.open_brushes(path));
        assert_eq!(after.saved_brush_library(), elsewhere);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn brushes_read_ahead_open_as_kept() {
        let dir = std::env::temp_dir().join(format!(
            "procraft-brushes-ahead-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        ));
        let path = dir.to_string_lossy().into_owned();
        // nothing kept yet: nothing read
        super::prepare_brushes(path.clone());
        let first = CanvasSession::new(32, 32);
        let set = first.new_brush_set();
        assert!(first.rename_brush_set(set, "Ahead".into()));
        assert!(first.keep_brushes(path.clone()));
        // held for another folder meanwhile, then read ahead for this one
        assert!(CanvasSession::new(32, 32).keep_brushes(dir.join("other").to_string_lossy().into_owned()));
        super::prepare_brushes(path.clone());
        let next = CanvasSession::new(32, 32);
        assert!(next.open_brushes(path));
        assert_eq!(next.saved_brush_library(), first.saved_brush_library());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_seconds_own_shape_keeps_its_sizes_through_a_restart() {
        use procraft_engine::brush::{Brush, Source};
        use procraft_engine::textures::TEXTURE_SIZE;
        let s = CanvasSession::new(32, 32);
        let round = Brush::round_pen();
        // a second with a shape of its own, its sizes where the round pen's controls sit (so
        // left out when kept, as its origin's are): other pixels than the round pen's
        let mut second = Brush::round_pen();
        second.shape.image = None;
        second.shape.source = Source::new(vec![255; TEXTURE_SIZE * TEXTURE_SIZE]);
        second.set_setting("max_size", round.setting("max_size"));
        second.set_setting("min_size", round.setting("min_size"));
        assert!((second.min_size - round.min_size).abs() > 0.1, "{} {}", second.min_size, round.min_size);
        s.shared.lock().library[0].brushes[0].secondary = Some(Box::new(second.clone()));
        let t = CanvasSession::new(32, 32);
        assert!(t.load_brush_library(s.saved_brush_library()));
        let back = t.shared.lock().library[0].brushes[0].secondary.clone().unwrap();
        assert!((back.max_size - second.max_size).abs() < 1e-3, "{} {}", back.max_size, second.max_size);
        assert!((back.min_size - second.min_size).abs() < 1e-4, "{} {}", back.min_size, second.min_size);
    }

    #[test]
    fn a_held_library_is_used_only_while_its_file_is_as_it_was() {
        let dir = std::env::temp_dir().join(format!(
            "procraft-brushes-stamp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(super::LIBRARY_FILE);
        std::fs::write(&file, b"0123456789").unwrap();
        let slot = std::sync::Mutex::new(None);
        let s = CanvasSession::new(32, 32);
        let library = s.shared.lock().library.clone();
        let count = library.len();
        super::remember(&slot, &file, 7, library);
        assert_eq!(super::remembered(&slot, &file).map(|(l, told)| (l.len(), told)), Some((count, 7)));
        assert!(super::remembered(&slot, &dir.join("other.json")).is_none(), "another file");
        // written again at the same size, its time put back: still as it was
        let time = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::fs::write(&file, b"9876543210").unwrap();
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(time).unwrap();
        assert!(super::remembered(&slot, &file).is_some());
        // another size, or another time: read again
        std::fs::write(&file, b"01234567890").unwrap();
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(time).unwrap();
        assert!(super::remembered(&slot, &file).is_none());
        std::fs::write(&file, b"0123456789").unwrap();
        let later = time + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(later).unwrap();
        assert!(super::remembered(&slot, &file).is_none());
        std::fs::remove_file(&file).unwrap();
        assert!(super::remembered(&slot, &file).is_none(), "gone");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod recent {
    use crate::CanvasSession;

    /// The Recent set as places: its list keys its rows by them, and a place twice stops the app.
    fn places(s: &CanvasSession) -> Vec<(u32, u32)> {
        s.recent_brushes().iter().map(|r| (r.set, r.index)).collect()
    }

    #[test]
    fn a_brush_pinned_and_one_combined_into_it_show_once_in_the_recent_set() {
        let s = CanvasSession::new(32, 32);
        s.shared.lock().note_recent((1, 0));
        s.shared.lock().note_recent((1, 1));
        assert!(s.pin_recent_brush(1, 0, true));
        assert!(s.combine_brushes(1, 0, 1));
        assert_eq!(places(&s), [(1, 0)]);
    }

    #[test]
    fn a_brush_deleted_leaves_the_recent_set_and_the_one_under_it_stays_once() {
        let s = CanvasSession::new(32, 32);
        s.shared.lock().note_recent((1, 0));
        s.shared.lock().note_recent((1, 1));
        s.shared.lock().note_recent((1, 3));
        assert!(s.pin_recent_brush(1, 1, true));
        let kept = s.brush_library()[1].brushes[1].name.clone();
        assert!(s.delete_brush(1, 0));
        assert_eq!(places(&s), [(1, 0), (1, 2)], "the pinned one up a place, then the other");
        assert_eq!(s.recent_brushes()[0].name, kept);
    }

    #[test]
    fn a_set_moved_any_way_leaves_every_place_on_its_own_brush() {
        let s = CanvasSession::new(32, 32);
        let sets = s.brush_library().len() as u32;
        assert!(sets >= 6);
        // each kind of place on a brush of its own, named as it was
        let names = |s: &CanvasSession, places: &[(u32, u32)]| -> Vec<String> {
            let library = s.brush_library();
            places.iter().map(|&(set, i)| library[set as usize].brushes[i as usize].name.clone()).collect()
        };
        let mut seen = vec![(1, 0), (2, 1), (3, 0)];
        let was = names(&s, &seen);
        s.shared.lock().note_recent((1, 0));
        s.shared.lock().note_recent((2, 1));
        s.shared.lock().note_recent((3, 0));
        assert!(s.pin_recent_brush(3, 0, true));
        assert!(s.select_brush(2, 1));
        let key = s.quick_brush_key(1, 0);
        let mut state = 0x54u32;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n
        };
        for _ in 0..2000 {
            // past the end too: a row where a set no longer is
            let (from, to) = (next(sets), next(sets + 3));
            assert!(s.move_brush_set(from, to));
            let at = to.min(sets - 1);
            for place in &mut seen {
                place.0 = match place.0 {
                    p if p == from => at,
                    p if p > from && p <= at => p - 1,
                    p if p >= at && p < from => p + 1,
                    p => p,
                };
            }
            assert_eq!(names(&s, &seen), was);
            let mut found = places(&s);
            found.sort_unstable();
            let mut wanted = seen.clone();
            wanted.sort_unstable();
            assert_eq!(found, wanted, "the Recent set followed, each brush once");
            let c = s.current_brush();
            assert_eq!((c.set, c.index), seen[1]);
            assert_eq!(s.quick_brush(key).map(|b| (b.set, b.index)), Some(seen[0]));
        }
        // kept and read back after all that, each still where the choices say
        let t = CanvasSession::new(32, 32);
        assert!(t.load_brush_library(s.saved_brush_library()));
        assert!(t.load_brush_choices(s.saved_brush_choices()));
        assert_eq!(names(&t, &seen), was);
        assert_eq!(t.recent_brushes().len(), 3);
    }
}
