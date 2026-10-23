//! Editing the brush library: brushes duplicated, deleted and made new, sets made, renamed and
//! deleted, and the whole library kept between sessions as each brush's origin and changes.

use std::collections::BTreeMap;

use procraft_engine::brush::{Author, Brush, Notches, ResetPoint, Source, StockImage, StockImages};
use procraft_engine::brushfile::Texture;
use procraft_engine::curve::Curve;
use procraft_engine::geom::Vec2;
use procraft_engine::library::{BrushSet, SHELF};

use crate::{CanvasSession, State};

/// A set in the library as it is now: the shipped sets as edited, and the user's own.
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
    /// The brush's own shape and grain images, as text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shape_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grain_source: Option<String>,
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
}

/// The tools in the order [`SavedChoices`] keeps them.
const TOOLS: [crate::ToolKind; 3] = [crate::ToolKind::Paint, crate::ToolKind::Erase, crate::ToolKind::Smudge];

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
        let brush = set.brushes.get(i)?;
        Some(self.saved_brush(brush, set.origins.get(i).copied().flatten()))
    }

    /// `brush` as kept, as changed from what it started as (`from`).
    fn saved_brush(&self, brush: &Brush, from: Option<(usize, usize)>) -> SavedBrush {
        let base = self.origin_brush(from);
        let kept = |c: &Curve, b: &Curve| -> Vec<[f32; 2]> {
            if c == b { Vec::new() } else { c.points().iter().map(|p| [p.x, p.y]).collect() }
        };
        let image = |mine: Option<u32>, theirs: Option<u32>| (mine != theirs).then(|| mine.map_or(-1, i64::from));
        let text = |png: &Option<std::sync::Arc<[u8]>>| png.as_deref().map(procraft_engine::base64::encode);
        SavedBrush {
            from,
            name: brush.name.clone(),
            settings: brush.changes_from(&base).into_iter().collect(),
            curve: kept(&brush.pressure_curve, &base.pressure_curve),
            size_curve: kept(&brush.size_curve, &base.size_curve),
            opacity_curve: kept(&brush.opacity_curve, &base.opacity_curve),
            shape_source: brush.shape.source.as_ref().map(Source::to_text),
            grain_source: brush.grain.source.as_ref().map(Source::to_text),
            shape_image: image(brush.shape.image, base.shape.image),
            grain_image: image(brush.grain.image, base.grain.image),
            thumbnail: brush.thumbnail.as_deref().map(procraft_engine::base64::encode),
            notches: SavedNotches::of(&brush.notches),
            author: brush.author.as_ref().map(|a| SavedAuthor {
                name: a.name.clone(),
                picture: text(&a.picture),
                signature: text(&a.signature),
            }),
            reset: brush
                .reset_point
                .as_ref()
                .map(|r| SavedReset { made: r.made, brush: Box::new(self.saved_brush(&r.brush, from)) }),
            shape_stock: brush.stock.shape.as_ref().map(|s| (s.name.clone(), s.inverted)),
            grain_stock: brush.stock.grain.as_ref().map(|s| (s.name.clone(), s.inverted)),
        }
    }

    /// A kept brush made again.
    fn restored(&self, b: SavedBrush) -> Brush {
        let mut brush = self.origin_brush(b.from);
        if !b.name.trim().is_empty() {
            brush.name = b.name.chars().take(256).collect();
        }
        // the images first: a brush's own shape sets the unit its sizes are kept in
        for (kept, image) in [(b.shape_image, &mut brush.shape.image), (b.grain_image, &mut brush.grain.image)] {
            if let Some(k) = kept {
                *image = u32::try_from(k).ok();
            }
        }
        if let Some(text) = &b.shape_source {
            brush.shape.source = Source::from_text(text);
        }
        if let Some(text) = &b.grain_source {
            brush.grain.source = Source::from_text(text);
        }
        // the stock images its file named, stood in for by our nearest as they are now
        let stock = |kept: &Option<(String, bool)>| {
            kept.as_ref().map(|(name, inverted)| StockImage { name: name.clone(), inverted: *inverted })
        };
        brush.stock = StockImages { shape: stock(&b.shape_stock), grain: stock(&b.grain_stock) };
        let png = |text: &Option<String>| text.as_deref().and_then(procraft_engine::base64::decode).map(Into::into);
        brush.thumbnail = png(&b.thumbnail);
        if let Some(s) = &brush.stock.shape {
            let texture = Texture::Stock { name: s.name.clone(), inverted: s.inverted };
            brush.shape.image = crate::brush_files::shape_for(&texture).0;
        }
        if let Some(s) = &brush.stock.grain {
            let texture = Texture::Stock { name: s.name.clone(), inverted: s.inverted };
            brush.grain.image = crate::brush_files::grain_for(&texture, brush.thumbnail.as_deref()).0;
        }
        brush.set_settings(b.settings.iter().map(|(k, v)| (k.as_str(), *v)));
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
        brush.notches = b.notches.notches();
        brush.author = b.author.as_ref().map(|a| Author {
            name: a.name.clone(),
            picture: png(&a.picture),
            signature: png(&a.signature),
        });
        brush.reset_point = b.reset.map(|r| ResetPoint::of(&self.restored(*r.brush), r.made));
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
        st.move_brush_refs(|(rs, ri)| match (rs == s, ri.cmp(&i)) {
            (true, std::cmp::Ordering::Greater) => Some((rs, ri - 1)),
            // the brush below moves up into its place, or the one above when it was last
            (true, std::cmp::Ordering::Equal) => (left > 0).then(|| (rs, ri.min(left - 1))),
            _ => Some((rs, ri)),
        });
        true
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

    /// The library as kept between sessions (JSON).
    pub fn saved_brush_library(&self) -> String {
        let st = self.shared.lock();
        let sets: Vec<SavedSet> = st
            .library
            .iter()
            .map(|set| SavedSet {
                name: set.name.clone(),
                brushes: (0..set.brushes.len()).filter_map(|i| st.saved(set, i)).collect(),
                icon: Some(set.icon),
                icons: ICON_NUMBERING,
            })
            .collect();
        serde_json::to_string(&sets).unwrap_or_default()
    }

    /// Puts back a library kept with [`Self::saved_brush_library`]; false, changing nothing, if it
    /// can't be read.
    pub fn load_brush_library(&self, json: String) -> bool {
        let Ok(saved) = serde_json::from_str::<Vec<SavedSet>>(&json) else { return false };
        if saved.is_empty() {
            return false;
        }
        let mut st = self.shared.lock();
        let library: Vec<LibrarySet> = saved
            .into_iter()
            .map(|set| {
                let icon = set.icon_now();
                let origins = set.brushes.iter().map(|b| b.from).collect();
                let brushes = set.brushes.into_iter().map(|b| st.restored(b)).collect();
                LibrarySet { name: set.name, brushes, origins, icon }
            })
            .collect();
        st.library = library;
        st.sliders.clear();
        st.move_brush_refs(Some);
        true
    }

    /// The tools' brushes, the sets' last choices and each brush's sliders, as kept between
    /// sessions (JSON); kept with the library as it is now.
    pub fn saved_brush_choices(&self) -> String {
        let st = self.shared.lock();
        let tool_index = |tool| TOOLS.iter().position(|&t| t == tool).unwrap_or(0) as u8;
        let mut sliders: Vec<_> =
            st.sliders.iter().map(|(&(tool, s, i), v)| (tool_index(tool), s, i, v.size, v.opacity)).collect();
        sliders.sort_by_key(|&(t, s, i, ..)| (t, s, i));
        let choices =
            SavedChoices { tools: TOOLS.map(|tool| st.brush_ref(tool)), sets: st.set_choices.clone(), sliders };
        serde_json::to_string(&choices).unwrap_or_default()
    }

    /// Puts back what [`Self::saved_brush_choices`] kept, after the library it was kept with;
    /// places the library no longer has are left as they are. False if it can't be read.
    pub fn load_brush_choices(&self, json: String) -> bool {
        let Ok(saved) = serde_json::from_str::<SavedChoices>(&json) else { return false };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        for (tool, place) in TOOLS.into_iter().zip(saved.tools) {
            if !st.holds(place) {
                continue;
            }
            match tool {
                crate::ToolKind::Paint => st.paint_brush = place,
                crate::ToolKind::Erase => st.erase_brush = place,
                crate::ToolKind::Smudge => st.smudge_brush = place,
            }
        }
        st.set_choices.clear();
        for place in saved.sets {
            if st.holds(place) {
                st.note_choice(place);
            }
        }
        for (tool, s, i, size, opacity) in saved.sliders {
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
