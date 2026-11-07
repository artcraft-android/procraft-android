//! The artwork: canvas size, background and layers. Pixels live with the renderer, keyed by
//! [`LayerId`]; this is the structure around them, cheap to clone (undo keeps copies).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u64);

/// How a layer combines with what is below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Darken,
    /// The darker of what is below and the layer laid over it normally, alpha included: it shows
    /// nothing where there is nothing below.
    Shade,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    Add,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Greater,
}

impl BlendMode {
    /// Every mode, in the order the layer panel lists them (grouped: darken, normal, lighten,
    /// contrast, difference, colour).
    pub const ALL: [BlendMode; 28] = [
        BlendMode::Multiply,
        BlendMode::Darken,
        BlendMode::Shade,
        BlendMode::ColorBurn,
        BlendMode::LinearBurn,
        BlendMode::DarkerColor,
        BlendMode::Normal,
        BlendMode::Lighten,
        BlendMode::Screen,
        BlendMode::ColorDodge,
        BlendMode::Add,
        BlendMode::LighterColor,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::HardMix,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Subtract,
        BlendMode::Divide,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
        BlendMode::Greater,
    ];

    /// The highest of the modes' numbers.
    pub const MAX_ID: u32 = 30;

    /// The renderer's number for the mode (stable: shaders switch on it).
    pub fn id(self) -> u32 {
        match self {
            BlendMode::Normal => 0,
            BlendMode::Multiply => 1,
            BlendMode::Darken => 2,
            BlendMode::ColorBurn => 3,
            BlendMode::LinearBurn => 4,
            BlendMode::DarkerColor => 5,
            BlendMode::Lighten => 6,
            BlendMode::Screen => 7,
            BlendMode::ColorDodge => 8,
            BlendMode::Add => 9,
            BlendMode::LighterColor => 10,
            BlendMode::Overlay => 11,
            BlendMode::SoftLight => 12,
            BlendMode::HardLight => 13,
            BlendMode::VividLight => 14,
            BlendMode::LinearLight => 15,
            BlendMode::PinLight => 16,
            BlendMode::HardMix => 17,
            BlendMode::Difference => 18,
            BlendMode::Exclusion => 19,
            BlendMode::Subtract => 20,
            BlendMode::Divide => 21,
            BlendMode::Hue => 22,
            BlendMode::Saturation => 23,
            BlendMode::Color => 24,
            BlendMode::Luminosity => 25,
            BlendMode::Shade => 26,
            BlendMode::Greater => 30,
        }
    }

    pub fn from_id(id: u32) -> Option<BlendMode> {
        BlendMode::ALL.iter().copied().find(|m| m.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Darken => "Darken",
            BlendMode::Shade => "Shade",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::LinearBurn => "Linear Burn",
            BlendMode::DarkerColor => "Darker Color",
            BlendMode::Lighten => "Lighten",
            BlendMode::Screen => "Screen",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::Add => "Add",
            BlendMode::LighterColor => "Lighter Color",
            BlendMode::Overlay => "Overlay",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::HardLight => "Hard Light",
            BlendMode::VividLight => "Vivid Light",
            BlendMode::LinearLight => "Linear Light",
            BlendMode::PinLight => "Pin Light",
            BlendMode::HardMix => "Hard Mix",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Subtract => "Subtract",
            BlendMode::Divide => "Divide",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
            BlendMode::Greater => "Greater",
        }
    }
}

/// How much a mask's pixel (RGBA, as painted) shows its layer, as a grey byte: its brightness, as
/// the canvas composites it (a colour painted on a mask counts by how light it is).
pub fn mask_grey([r, g, b, _]: [u8; 4]) -> u8 {
    let lit = 299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b);
    u8::try_from((lit + 500) / 1000).unwrap_or(u8::MAX)
}

/// What a layer in the stack is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LayerKind {
    /// Pixels to paint on.
    #[default]
    Paint,
    /// A group: the layers just below it whose parent it is, composited together first.
    Group,
    /// A mask: just above the layer it belongs to (its parent), showing that layer as much as it
    /// is light.
    Mask,
}

/// How lines of text line up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Text a layer holds, still editable: the app draws it into the layer's pixels whenever it
/// changes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LayerText {
    pub text: String,
    /// The typeface's family and style names.
    pub font: String,
    pub style: String,
    /// Letter height, canvas pixels.
    pub size: f32,
    /// Extra space at the caret between two letters, and between all letters, as a fraction of
    /// the size (-1..1).
    pub kerning: f32,
    pub tracking: f32,
    /// Extra space between lines, and the letters raised, as a fraction of the size (-1..1).
    pub leading: f32,
    pub baseline: f32,
    pub opacity: f32,
    pub align: TextAlign,
    pub underline: bool,
    pub outline: bool,
    pub vertical: bool,
    pub all_caps: bool,
    /// 0..1 in the document's profile.
    pub color: [f32; 3],
    /// The text box's top left corner, canvas pixels, and how wide it wraps (0: no wrapping).
    pub origin: [f32; 2],
    pub width: f32,
}

impl Default for LayerText {
    fn default() -> Self {
        LayerText {
            text: String::new(),
            font: String::new(),
            style: String::new(),
            size: 64.0,
            kerning: 0.0,
            tracking: 0.0,
            leading: 0.0,
            baseline: 0.0,
            opacity: 1.0,
            align: TextAlign::Left,
            underline: false,
            outline: false,
            vertical: false,
            all_caps: false,
            color: [0.0; 3],
            origin: [0.0; 2],
            width: 0.0,
        }
    }
}

impl LayerText {
    /// The same text with every number in range.
    pub fn sanitized(mut self) -> LayerText {
        let unit = |v: f32| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        self.size = if self.size.is_finite() { self.size.clamp(1.0, 4096.0) } else { 64.0 };
        self.kerning = unit(self.kerning);
        self.tracking = unit(self.tracking);
        self.leading = unit(self.leading);
        self.baseline = unit(self.baseline);
        self.opacity = if self.opacity.is_finite() { self.opacity.clamp(0.0, 1.0) } else { 1.0 };
        self.color = self.color.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 });
        self.origin = self.origin.map(|c| if c.is_finite() { c.clamp(-1.0e6, 1.0e6) } else { 0.0 });
        self.width = if self.width.is_finite() { self.width.clamp(0.0, 1.0e6) } else { 0.0 };
        self.text = self.text.chars().take(100_000).collect();
        self
    }

    /// Its look as new text takes it on: the typeface and style, the alignment and whether it
    /// runs down; nothing else (new text is sized by the screen and coloured by the brush).
    pub fn look(&self) -> LayerText {
        LayerText {
            font: self.font.clone(),
            style: self.style.clone(),
            align: self.align,
            vertical: self.vertical,
            ..LayerText::default()
        }
    }

    /// A layer name from the text: its first line, shortened.
    pub fn name(&self) -> Option<String> {
        let line = self.text.lines().map(str::trim).find(|l| !l.is_empty())?;
        Some(line.chars().take(64).collect())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    /// Painting only changes pixels that already have alpha.
    pub alpha_lock: bool,
    /// Shown only where the nearest unclipped layer below has alpha.
    pub clipped: bool,
    /// No painting or changes.
    pub locked: bool,
    /// What colour fills and automatic selections look at instead of the current layer (one
    /// layer at most).
    #[serde(default)]
    pub reference: bool,
    #[serde(default)]
    pub kind: LayerKind,
    /// A mask's layer, or the group a layer is in.
    #[serde(default)]
    pub parent: Option<LayerId>,
    /// A group shown closed in the layers panel.
    #[serde(default)]
    pub collapsed: bool,
    /// A text layer's text, until its pixels are changed some other way.
    #[serde(default)]
    pub text: Option<LayerText>,
    /// Left out of the gallery's thumbnail, the file's preview and the time-lapse's frames (put
    /// in by a private insert or paste).
    #[serde(default)]
    pub private: bool,
    /// Strokes on it follow the drawing guide (drawing assist).
    #[serde(default)]
    pub assisted: bool,
}

impl Layer {
    /// A layer of `kind` as new ones are: shown, opaque, normal, nothing else set.
    pub(crate) fn plain(id: LayerId, kind: LayerKind, name: String, parent: Option<LayerId>) -> Layer {
        Layer {
            id,
            name,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            alpha_lock: false,
            clipped: false,
            locked: false,
            reference: false,
            kind,
            parent,
            collapsed: false,
            text: None,
            private: false,
            assisted: false,
        }
    }

    /// A group as groups are made (normal, fully opaque): its members blend with what is below
    /// it as they would ungrouped. A group given another mode or opacity (by Layer Opacity or a
    /// file) is composited on its own first, then blended.
    pub fn passes_through(&self) -> bool {
        self.kind == LayerKind::Group && self.blend == BlendMode::Normal && self.opacity >= 1.0
    }
}

/// Why a change to a layer would go unseen (see [`Document::unseen`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unseen {
    /// Its own box is off.
    Itself,
    /// A mask whose layer is hidden.
    MaskLayer,
    /// A group it is in is hidden.
    Group,
    /// A group with something hidden in it (a layer, a group or a mask, at any depth).
    Inside,
    /// Left out while another layer is shown alone, and so is every layer picked with it.
    Solo,
}

/// Largest canvas edge, pixels.
pub const MAX_CANVAS_EDGE: u32 = 16384;

/// The largest edge a new canvas or a crop may have here: [`MAX_CANVAS_EDGE`], or the GPU's
/// largest texture where that is less (set as its device is made).
static EDGE_MOST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(MAX_CANVAS_EDGE);

/// The largest edge a new canvas or a crop may have on this device, pixels (a canvas opened from
/// a file is as large as it is, and a GPU that can't hold it refuses it).
pub fn canvas_edge_most() -> u32 {
    EDGE_MOST.load(std::sync::atomic::Ordering::Relaxed)
}

/// The GPU's textures are at most `side` pixels a side: new canvases and crops keep within it.
pub fn set_gpu_texture_most(side: u32) {
    EDGE_MOST.store(side.clamp(1, MAX_CANVAS_EDGE), std::sync::atomic::Ordering::Relaxed);
}

/// Memory the layers' pixels may take unless the device says otherwise, bytes.
pub const LAYER_BUDGET: u64 = 2 * 1024 * 1024 * 1024;

/// Memory the layers' pixels may take on this device, bytes: it sets how many layers a canvas
/// holds.
static BUDGET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(LAYER_BUDGET);

/// How many layers with pixels a canvas `width` × `height` holds on this device.
pub fn layer_cap_for(width: u32, height: u32) -> usize {
    let bytes = u64::from(width) * u64::from(height) * 4;
    let budget = BUDGET.load(std::sync::atomic::Ordering::Relaxed);
    usize::try_from(budget / bytes.max(1)).unwrap_or(usize::MAX).max(1)
}

/// The memory layers may take on this device, bytes.
pub fn layer_budget() -> u64 {
    BUDGET.load(std::sync::atomic::Ordering::Relaxed)
}

/// Sets the memory layers may take on this device (256 MiB to 64 GiB).
pub fn set_layer_budget(bytes: u64) {
    BUDGET.store(bytes.clamp(256 << 20, 64 << 30), std::sync::atomic::Ordering::Relaxed);
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Document {
    width: u32,
    height: u32,
    /// Background colour, 0..1 in the document's profile.
    pub background: [f32; 3],
    pub background_visible: bool,
    /// Bottom layer first.
    #[serde(deserialize_with = "readable_layers")]
    layers: Vec<Layer>,
    current: LayerId,
    /// Layers picked to act on together with the current one (in the layers panel); not saved.
    #[serde(skip)]
    picked: Vec<LayerId>,
    next_id: u64,
    /// The drawing guide over the canvas.
    #[serde(default, deserialize_with = "or_default")]
    pub guide: crate::guide::Guide,
    /// Dots per inch, for the canvas's printed size and in files that carry one.
    #[serde(default = "default_dpi")]
    pub dpi: f32,
    /// Animation Assist's settings while it is on.
    #[serde(default, deserialize_with = "or_default")]
    pub animation: Option<crate::animation::Animation>,
    /// The colour profile its pixels and colours are in.
    #[serde(default, deserialize_with = "or_default")]
    pub profile: crate::profile::Profile,
    /// A profile of its own: an RGB one its pixels and colours are in, or a CMYK one its colours
    /// print to (its pixels then in Display P3, shown as the press would print them). None for the
    /// built-in profiles.
    #[serde(default, deserialize_with = "crate::cms::lenient", skip_serializing_if = "Option::is_none")]
    pub icc: Option<crate::cms::CanvasIcc>,
    /// The layer shown alone, the others left out of the picture (solo visibility).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solo: Option<LayerId>,
    /// The look of the text last edited (see [`LayerText::look`]), which new text starts with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_style: Option<LayerText>,
}

/// The layers a file lists, as many as can be read: in a damaged file a layer with fields that
/// can't be read keeps those that can ([`readable_layer`]), and one whose id can't be is left out,
/// rather than the artwork failing to open (the repair mends what that leaves).
fn readable_layers<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Layer>, D::Error> {
    let entries: Vec<serde_json::Value> = serde::Deserialize::deserialize(d)?;
    Ok(entries.into_iter().filter_map(readable_layer).collect())
}

/// A layer as a file lists it; if some of its fields can't be read, it keeps those that can and
/// the rest as a new layer has them, so long as its id (which ties it to its pixels) can be.
fn readable_layer(entry: serde_json::Value) -> Option<Layer> {
    if let Ok(layer) = serde_json::from_value::<Layer>(entry.clone()) {
        return Some(layer);
    }
    let serde_json::Value::Object(fields) = entry else { return None };
    let id: LayerId = serde_json::from_value(fields.get("id")?.clone()).ok()?;
    let mut layer = serde_json::to_value(Layer::plain(id, LayerKind::Paint, "Layer".to_owned(), None)).ok()?;
    for (key, value) in fields {
        let mut tried = layer.clone();
        if let Some(o) = tried.as_object_mut() {
            o.insert(key, value);
        }
        if serde_json::from_value::<Layer>(tried.clone()).is_ok() {
            layer = tried;
        }
    }
    serde_json::from_value(layer).ok()
}

/// A setting as a file keeps it, or its default if it can't be read (the artwork opens without
/// it).
pub fn or_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let v: serde_json::Value = serde::Deserialize::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// A document's profile settings apart from it.
struct ProfileSettings {
    profile: crate::profile::Profile,
    icc: Option<crate::cms::CanvasIcc>,
}

impl ProfileSettings {
    fn rgb_profile(&self) -> std::sync::Arc<crate::cms::IccProfile> {
        match &self.icc {
            Some(i) if i.0.space == crate::cms::Space::Rgb => i.0.clone(),
            // a CMYK canvas's pixels are in Display P3
            Some(_) => crate::cms::builtin(crate::profile::Profile::DisplayP3),
            None => crate::cms::builtin(self.profile),
        }
    }
}

fn default_dpi() -> f32 {
    crate::formats::DPI as f32
}

impl Document {
    /// A canvas of `width` × `height` pixels (each clamped to 1..=[`MAX_CANVAS_EDGE`]) with a white
    /// background and one empty layer.
    pub fn new(width: u32, height: u32) -> Document {
        let mut doc = Document {
            width: width.clamp(1, MAX_CANVAS_EDGE),
            height: height.clamp(1, MAX_CANVAS_EDGE),
            background: [1.0, 1.0, 1.0],
            background_visible: true,
            layers: Vec::new(),
            current: LayerId(0),
            picked: Vec::new(),
            next_id: 1,
            guide: crate::guide::Guide::new(width.clamp(1, MAX_CANVAS_EDGE), height.clamp(1, MAX_CANVAS_EDGE)),
            dpi: default_dpi(),
            animation: None,
            profile: crate::profile::Profile::Srgb,
            icc: None,
            solo: None,
            text_style: None,
        };
        doc.add_layer();
        doc
    }

    /// A canvas of `width` × `height` pixels holding `layers` (bottom first), `current` chosen if
    /// it is one of them; what doesn't fit the rules (masks away from their layers, parents that
    /// aren't groups...) is put right as [`Document::repaired`] does. With no layers it has one.
    pub fn from_layers(width: u32, height: u32, layers: Vec<Layer>, current: LayerId) -> Document {
        let mut doc = Document::new(width, height);
        if layers.is_empty() {
            return doc;
        }
        doc.layers = layers;
        let mut doc = doc.repaired();
        let first = doc.layers.iter().find(|l| l.kind == LayerKind::Paint).map(|l| l.id);
        doc.current = if doc.layer(current).is_some_and(|l| l.kind == LayerKind::Paint) {
            current
        } else {
            first.unwrap_or(current)
        };
        doc
    }

    /// Takes on `other`'s layer numbering where it has gone further, so that no id it handed out
    /// is handed out again here (this document put back in its place by undo, or made to follow
    /// another's layers): a canvas may still hold pixels for that id.
    pub fn keep_ids_from(&mut self, other: &Document) {
        self.next_id = self.next_id.max(other.next_id);
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    /// How many layers with pixels (paint layers and masks) a canvas this size holds.
    pub fn layer_cap(&self) -> usize {
        layer_cap_for(self.width, self.height)
    }

    /// Whether `more` more layers with pixels fit under [`Document::layer_cap`].
    pub fn has_room(&self, more: usize) -> bool {
        self.pixel_layer_ids().count() + more <= self.layer_cap()
    }

    /// The same document on a canvas of `width` × `height` (each clamped to
    /// 1..=[`MAX_CANVAS_EDGE`]); the drawing guide is centred on it again.
    pub fn resized(&self, width: u32, height: u32) -> Document {
        let mut d = self.clone();
        d.width = width.clamp(1, MAX_CANVAS_EDGE);
        d.height = height.clamp(1, MAX_CANVAS_EDGE);
        d.guide.centre = [d.width as f32 / 2.0, d.height as f32 / 2.0];
        d.guide.vanishing.clear();
        d
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Puts the document in built-in profile `profile` with profile `icc` of its own (see
    /// [`Document::icc`]), nothing turned: its colours keep their numbers in it.
    pub fn assign_profile(&mut self, profile: crate::profile::Profile, icc: Option<crate::cms::CanvasIcc>) {
        self.profile = profile;
        self.icc = icc;
    }

    /// Puts the document in profile `to`, its background and text colours turned into it so they
    /// look the same (the layers' pixels are the renderer's).
    pub fn change_profile(&mut self, to: crate::profile::Profile) {
        self.change_to(to, None);
    }

    /// Puts the document in built-in profile `profile` with profile `icc` of its own (see
    /// [`Document::icc`]), its background and text colours turned into the RGB profile its pixels
    /// are then in.
    pub fn change_to(&mut self, profile: crate::profile::Profile, icc: Option<crate::cms::CanvasIcc>) {
        // between built-in profiles: their matrices, exactly
        if self.icc.is_none() && icc.is_none() {
            let from = self.profile;
            self.background = from.convert(self.background, profile);
            for text in self.layers.iter_mut().filter_map(|l| l.text.as_mut()) {
                text.color = from.convert(text.color, profile);
            }
            self.profile = profile;
            return;
        }
        let from = self.rgb_profile();
        let mut next = self.clone_settings();
        next.profile = profile;
        next.icc = icc;
        let to = next.rgb_profile();
        let turn = |c: [f32; 3]| -> [f32; 3] {
            let v = from.convert(&c.map(f64::from), &to);
            [0, 1, 2].map(|k| v.get(k).copied().unwrap_or(0.0) as f32)
        };
        if !std::sync::Arc::ptr_eq(&from, &to) && from != to {
            self.background = turn(self.background);
            for text in self.layers.iter_mut().filter_map(|l| l.text.as_mut()) {
                text.color = turn(text.color);
            }
        }
        self.profile = next.profile;
        self.icc = next.icc;
    }

    /// Just the profile settings, to ask what profile they make.
    fn clone_settings(&self) -> ProfileSettings {
        ProfileSettings { profile: self.profile, icc: self.icc.clone() }
    }

    /// The profile its colours print to, if it is a CMYK canvas.
    pub fn press(&self) -> Option<std::sync::Arc<crate::cms::IccProfile>> {
        self.icc.as_ref().filter(|i| i.0.space == crate::cms::Space::Cmyk).map(|i| i.0.clone())
    }

    /// The RGB profile its pixels and colours are in: its own, else the built-in one.
    pub fn rgb_profile(&self) -> std::sync::Arc<crate::cms::IccProfile> {
        self.clone_settings().rgb_profile()
    }

    /// Its profile's name, as lists show it.
    pub fn profile_name(&self) -> String {
        self.icc.as_ref().map_or_else(|| self.profile.name().to_owned(), |i| i.0.name.clone())
    }

    /// The profile a picture of it carries: the RGB one its colours are in (a CMYK canvas's
    /// pictures show the press's colours in Display P3).
    pub fn picture_icc(&self) -> Vec<u8> {
        match &self.icc {
            Some(i) if i.0.space == crate::cms::Space::Rgb => i.0.bytes.to_vec(),
            Some(_) => crate::icc::icc(crate::profile::Profile::DisplayP3),
            None => crate::icc::icc(self.profile),
        }
    }

    /// The layers whose pixels are colours (paint layers, not masks), bottom first.
    pub fn colour_layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.layers.iter().filter(|l| l.kind == LayerKind::Paint).map(|l| l.id)
    }

    /// Bottom layer first.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Makes `id` the reference layer (or, not `on`, no layer); false if there is no such layer.
    pub fn set_reference(&mut self, id: LayerId, on: bool) -> bool {
        if self.layer(id).is_none() {
            return false;
        }
        for l in &mut self.layers {
            l.reference = on && l.id == id;
        }
        true
    }

    /// Whether layer `id` is left out of the picture while another is shown alone (solo): all
    /// but that layer with its mask, what is in it (a group shown alone shows its layers as they
    /// are set) and the groups it is in. A mask shown alone shows its layer with it.
    pub fn soloed_out(&self, id: LayerId) -> bool {
        let Some(solo) = self.solo.filter(|s| self.layer(*s).is_some()) else { return false };
        let solo = self.mask_owner(solo).unwrap_or(solo);
        if id == solo || self.mask_of(solo) == Some(id) || self.is_inside(id, solo) {
            return false;
        }
        !(self.layer(id).is_some_and(|l| l.kind == LayerKind::Group) && self.is_inside(solo, id))
    }

    /// Whether what is drawn on layer `id` is seen: it can show ([`Document::shows`]) and isn't
    /// left out while another layer is shown alone.
    pub fn in_picture(&self, id: LayerId) -> bool {
        self.shows(id) && !self.soloed_out(id)
    }

    /// Why a change to layer `id` (a stroke, a transform, an adjustment...) would go unseen, if it
    /// would, as such a change is told: its box is off; a mask on a hidden layer; in a hidden
    /// group; a group with something hidden in it; or left out while another layer is shown
    /// alone, and so is every layer picked with it.
    pub fn unseen(&self, id: LayerId) -> Option<Unseen> {
        let l = self.layer(id)?;
        if !l.visible {
            return Some(Unseen::Itself);
        }
        if self.mask_owner(id).is_some_and(|o| self.layer(o).is_none_or(|o| !o.visible)) {
            return Some(Unseen::MaskLayer);
        }
        if !self.shows(id) {
            return Some(Unseen::Group);
        }
        if l.kind == LayerKind::Group && self.layers.iter().any(|o| !o.visible && self.is_inside(o.id, id)) {
            return Some(Unseen::Inside);
        }
        (self.soloed_out(id) && self.picked().iter().all(|p| self.soloed_out(*p))).then_some(Unseen::Solo)
    }

    /// The layer mask `id` belongs to, if it is a mask.
    fn mask_owner(&self, id: LayerId) -> Option<LayerId> {
        let l = self.layer(id).filter(|l| l.kind == LayerKind::Mask)?;
        l.parent.filter(|p| self.mask_of(*p) == Some(id))
    }

    /// Whether layer `id` can show: it is visible, and so is every group it is in (and a mask's
    /// layer).
    pub fn shows(&self, id: LayerId) -> bool {
        if self.layer(id).is_none_or(|l| !l.visible) {
            return false;
        }
        if let Some(owner) = self.mask_owner(id)
            && self.layer(owner).is_none_or(|l| !l.visible)
        {
            return false;
        }
        let mut at = self.group_of(id);
        for _ in 0..self.layers.len() {
            let Some(g) = at else { return true };
            if self.layer(g).is_none_or(|l| !l.visible) {
                return false;
            }
            at = self.group_of(g);
        }
        true
    }

    /// The reference layer, if one is set.
    pub fn reference(&self) -> Option<LayerId> {
        self.layers.iter().find(|l| l.reference).map(|l| l.id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn current(&self) -> LayerId {
        self.current
    }

    pub fn set_current(&mut self, id: LayerId) -> bool {
        let found = self.layer(id).is_some();
        if found {
            self.current = id;
        }
        found
    }

    /// The layers picked besides the current one, bottom first.
    pub fn picked(&self) -> Vec<LayerId> {
        self.layers.iter().filter(|l| l.id != self.current && self.picked.contains(&l.id)).map(|l| l.id).collect()
    }

    pub fn is_picked(&self, id: LayerId) -> bool {
        id != self.current && self.picked.contains(&id) && self.layer(id).is_some()
    }

    /// Picks layer `id` to act on with the current one, or unpicks it; false for the current
    /// layer or one that isn't there.
    pub fn toggle_pick(&mut self, id: LayerId) -> bool {
        if id == self.current || self.layer(id).is_none() {
            return false;
        }
        match self.picked.iter().position(|p| *p == id) {
            Some(i) => {
                self.picked.remove(i);
            }
            None => self.picked.push(id),
        }
        true
    }

    /// Picks every layer, groups and masks too; false if all already were.
    pub fn pick_all(&mut self) -> bool {
        let all: Vec<LayerId> = self.layers.iter().map(|l| l.id).filter(|id| *id != self.current).collect();
        if all.iter().all(|id| self.picked.contains(id)) {
            return false;
        }
        self.picked = all;
        true
    }

    /// Chooses layers `a` and `b` and every one between them in the list (two rows pinched
    /// together in the panel): the lowest made current, the rest picked. False if either isn't
    /// there.
    pub fn choose_between(&mut self, a: LayerId, b: LayerId) -> bool {
        let (Some(i), Some(j)) = (self.index_of(a), self.index_of(b)) else { return false };
        let Some(span) = self.layers.get(i.min(j)..=i.max(j)) else { return false };
        let ids: Vec<LayerId> = span.iter().map(|l| l.id).collect();
        let Some(&lowest) = ids.first() else { return false };
        self.current = lowest;
        self.picked = ids.into_iter().skip(1).collect();
        true
    }

    /// Unpicks everything; false if nothing was picked.
    pub fn clear_picks(&mut self) -> bool {
        let any = !self.picked().is_empty();
        self.picked.clear();
        any
    }

    /// The current layer and the picked ones, bottom first.
    pub fn chosen(&self) -> Vec<LayerId> {
        self.layers.iter().filter(|l| l.id == self.current || self.picked.contains(&l.id)).map(|l| l.id).collect()
    }

    /// What is chosen as whole items, bottom first: a mask stands for its layer, and what is in a
    /// chosen group goes with it.
    fn chosen_items(&self) -> Vec<LayerId> {
        let mut items: Vec<LayerId> = Vec::new();
        for id in self.chosen() {
            let Some(l) = self.layer(id) else { continue };
            let item = if l.kind == LayerKind::Mask { l.parent } else { Some(id) };
            if let Some(item) = item.filter(|i| !items.contains(i)) {
                items.push(item);
            }
        }
        let groups: Vec<LayerId> =
            items.iter().copied().filter(|g| self.layer(*g).is_some_and(|l| l.kind == LayerKind::Group)).collect();
        items.retain(|id| !groups.iter().any(|g| self.is_inside(*id, *g)));
        items.sort_by_key(|id| self.index_of(*id).unwrap_or(usize::MAX));
        items
    }

    /// Puts what is chosen in a new group where the top of it was (in its group), keeping its
    /// order; the picks are cleared. Returns the group.
    pub fn group_chosen(&mut self) -> Option<LayerId> {
        let items = self.chosen_items();
        let top = *items.last()?;
        let parent = self.group_of(top);
        let top_end = self.unit(top)?.end;
        // lifted out top down, so the ranges below stay where they were
        let mut moved: Vec<Layer> = Vec::new();
        for id in items.iter().rev() {
            let unit = self.unit(*id)?;
            let mut part: Vec<Layer> = self.layers.drain(unit).collect();
            part.append(&mut moved);
            moved = part;
        }
        let group = self.blank(LayerKind::Group, "New group".to_owned(), parent);
        let gid = group.id;
        for l in &mut moved {
            if items.contains(&l.id) {
                l.parent = Some(gid);
            }
        }
        let at = top_end.saturating_sub(moved.len()).min(self.layers.len());
        self.layers.splice(at..at, moved.into_iter().chain(std::iter::once(group)));
        self.picked.clear();
        self.fix_clips();
        Some(gid)
    }

    /// Copies of what is chosen (each with its mask, or everything in it), together just above
    /// the top of it and in its group, in order; the copies become the current layer and the
    /// picks. Returns each (original, copy) pair of layers whose pixels the renderer copies.
    pub fn duplicate_chosen(&mut self) -> Option<Vec<(LayerId, LayerId)>> {
        let items = self.chosen_items();
        let top = *items.last()?;
        let parent = self.group_of(top);
        let at = self.unit(top)?.end;
        let mut copies: Vec<Layer> = Vec::new();
        let mut pairs = Vec::new();
        let mut made = std::collections::HashMap::new();
        for id in &items {
            let unit = self.unit(*id)?;
            let originals: Vec<Layer> = self.layers.get(unit)?.to_vec();
            for l in &originals {
                let copy = self.new_id();
                made.insert(l.id, copy);
            }
            let renamed = |old: LayerId| made.get(&old).copied().unwrap_or(old);
            for l in &originals {
                let mut c = l.clone();
                c.id = renamed(l.id);
                c.reference = false;
                c.parent = if l.id == *id { parent } else { l.parent.map(renamed) };
                if l.kind != LayerKind::Group {
                    pairs.push((l.id, c.id));
                }
                copies.push(c);
            }
        }
        self.layers.splice(at..at, copies);
        self.current = made.get(&self.current).copied().unwrap_or(self.current);
        self.picked = self.picked.iter().filter_map(|p| made.get(p).copied()).collect();
        self.fix_clips();
        Some(pairs)
    }

    /// Moves what is chosen (each with what goes with it, in its order) together to position `to`
    /// of the stack without them (0 = bottom), joining the group they land in as
    /// [`Document::move_layer`] does.
    pub fn move_chosen(&mut self, to: usize) -> bool {
        let items = self.chosen_items();
        if items.is_empty() {
            return false;
        }
        let mut moved: Vec<Layer> = Vec::new();
        for id in items.iter().rev() {
            let Some(unit) = self.unit(*id) else { return false };
            let mut part: Vec<Layer> = self.layers.drain(unit).collect();
            part.append(&mut moved);
            moved = part;
        }
        let mut to = to.min(self.layers.len());
        // never between a layer and its mask
        if self.layers.get(to).is_some_and(|l| l.kind == LayerKind::Mask) {
            to += 1;
        }
        let parent = self.layers.get(to).and_then(|above| match above.kind {
            LayerKind::Group => Some(above.id),
            _ => self.group_of(above.id),
        });
        for l in &mut moved {
            if items.contains(&l.id) {
                l.parent = parent;
            }
        }
        self.layers.splice(to..to, moved);
        self.fix_clips();
        true
    }

    /// Whether what is chosen (and what is in a chosen group) holds a locked layer.
    pub fn chosen_locked(&self) -> bool {
        let chosen = self.chosen();
        self.layers.iter().any(|l| {
            (chosen.contains(&l.id) || chosen.iter().any(|c| self.is_inside(l.id, *c))) && self.is_locked(l.id)
        })
    }

    /// Removes what is chosen as [`Document::delete_layer`] does (the last paint layer stays); the
    /// picks are cleared. As measured, nothing goes while any of it is locked. False if nothing
    /// went.
    pub fn delete_chosen(&mut self) -> bool {
        if self.chosen_locked() {
            return false;
        }
        let mut any = false;
        for id in self.chosen().into_iter().rev() {
            if self.layer(id).is_some() {
                any |= self.delete_layer(id);
            }
        }
        self.picked.clear();
        any
    }

    /// Merging what is chosen: its bottom paint layer, and what is chosen above that (layers and
    /// groups), bottom first, for the renderer to draw into it, each layer's mask (the bottom
    /// one's too) applied to it first. None unless there is something to merge or if any of it
    /// is locked.
    pub fn merge_chosen_plan(&self) -> Option<(LayerId, Vec<LayerId>)> {
        let items = self.chosen_items();
        if items.iter().any(|id| self.is_locked(*id)) {
            return None;
        }
        let at = items.iter().position(|id| self.layer(*id).is_some_and(|l| l.kind == LayerKind::Paint))?;
        let lower = *items.get(at)?;
        let uppers: Vec<LayerId> = items
            .get(at + 1..)?
            .iter()
            .copied()
            .filter(|id| self.layer(*id).is_some_and(|l| matches!(l.kind, LayerKind::Group | LayerKind::Paint)))
            .collect();
        (!uppers.is_empty()).then_some((lower, uppers))
    }

    /// Removes `uppers` (each with everything in it) once they were drawn into `lower`, which
    /// becomes current; the picks are cleared.
    pub fn finish_merge_chosen(&mut self, lower: LayerId, uppers: &[LayerId]) -> bool {
        if self.layer(lower).is_none() {
            return false;
        }
        // their masks go with them; the bottom one's was applied to it
        let mask = self.mask_of(lower);
        for id in uppers.iter().rev() {
            if let Some(unit) = self.unit(*id) {
                self.layers.drain(unit);
            }
        }
        self.layers.retain(|l| Some(l.id) != mask);
        // merged, it shows at full: its opacity went into its pixels (a hidden one's were let go)
        if let Some(l) = self.layer_mut(lower) {
            (l.visible, l.opacity) = (true, 1.0);
        }
        self.current = lower;
        self.picked.clear();
        self.rasterize(lower);
        self.fix_clips();
        true
    }

    fn new_id(&mut self) -> LayerId {
        let id = LayerId(self.next_id);
        self.next_id += 1;
        id
    }

    /// A blank layer of `kind` named `name`, with a new id.
    fn blank(&mut self, kind: LayerKind, name: String, parent: Option<LayerId>) -> Layer {
        Layer::plain(self.new_id(), kind, name, parent)
    }

    /// Adds a text layer above the current one (in its group), named after its text, and makes
    /// it current.
    pub fn add_text_layer(&mut self, text: LayerText) -> LayerId {
        let id = self.add_layer();
        if let Some(l) = self.layer_mut(id) {
            let text = text.sanitized();
            if let Some(name) = text.name() {
                l.name = name;
            }
            l.text = Some(text);
        }
        id
    }

    /// Changes a text layer's text (renaming it after the text); false if it isn't one.
    pub fn set_text(&mut self, id: LayerId, text: LayerText) -> bool {
        let Some(l) = self.layer_mut(id).filter(|l| l.text.is_some()) else { return false };
        let text = text.sanitized();
        if let Some(name) = text.name() {
            l.name = name;
        }
        l.text = Some(text);
        true
    }

    /// Makes a text layer an ordinary one, its pixels kept; false if it wasn't one.
    pub fn rasterize(&mut self, id: LayerId) -> bool {
        self.layer_mut(id).and_then(|l| l.text.take()).is_some()
    }

    /// Adds an empty layer above the current one (in its group) and makes it current. It is named
    /// after the number of entries the stack will have, so a deleted layer's number comes back.
    pub fn add_layer(&mut self) -> LayerId {
        let name = format!("Layer {}", self.layers.len() + 1);
        // above the current layer's mask, or above the current group, in the same group
        let (at, parent) = match self.layer(self.current) {
            Some(c) => {
                let owner = if c.kind == LayerKind::Mask { c.parent.unwrap_or(c.id) } else { c.id };
                let top = self.mask_of(owner).unwrap_or(owner);
                (self.index_of(top).map_or(self.layers.len(), |i| i + 1), self.group_of(owner))
            }
            None => (self.layers.len(), None),
        };
        let layer = self.blank(LayerKind::Paint, name, parent);
        let id = layer.id;
        self.layers.insert(at.min(self.layers.len()), layer);
        self.current = id;
        id
    }

    /// The mask of layer `id`, if it has one.
    pub fn mask_of(&self, id: LayerId) -> Option<LayerId> {
        let at = self.index_of(id)?;
        self.layers.get(at + 1).filter(|l| l.kind == LayerKind::Mask && l.parent == Some(id)).map(|l| l.id)
    }

    /// The group directly holding `id` (for a mask, its layer's group), if any.
    pub fn group_of(&self, id: LayerId) -> Option<LayerId> {
        let l = self.layer(id)?;
        let l = match l.kind {
            LayerKind::Mask => self.layer(l.parent?).filter(|o| o.kind == LayerKind::Paint)?,
            _ => l,
        };
        l.parent.filter(|p| *p != l.id && self.layer(*p).is_some_and(|g| g.kind == LayerKind::Group))
    }

    /// Whether `id` takes no change: it is locked, or a group it is inside is.
    pub fn is_locked(&self, id: LayerId) -> bool {
        self.layer(id).is_some_and(|l| l.locked) || !self.locked_groups(id).is_empty()
    }

    /// The locked groups `id` is inside, innermost first.
    pub fn locked_groups(&self, id: LayerId) -> Vec<LayerId> {
        let mut groups = Vec::new();
        let mut at = self.group_of(id);
        for _ in 0..self.layers.len() {
            let Some(g) = at else { break };
            if self.layer(g).is_some_and(|l| l.locked) {
                groups.push(g);
            }
            at = self.group_of(g);
        }
        groups
    }

    /// How many groups `id` is inside.
    pub fn depth(&self, id: LayerId) -> usize {
        let mut depth = 0;
        let mut at = self.group_of(id);
        while let Some(g) = at {
            depth += 1;
            if depth >= self.layers.len() {
                break;
            }
            at = self.group_of(g);
        }
        depth
    }

    /// The deepest any layer sits in groups.
    pub fn max_depth(&self) -> usize {
        self.layers.iter().map(|l| self.depth(l.id)).max().unwrap_or(0)
    }

    /// Whether `id` is somewhere inside group `group`.
    pub fn is_inside(&self, id: LayerId, group: LayerId) -> bool {
        let mut at = self.group_of(id);
        for _ in 0..self.layers.len() {
            match at {
                Some(g) if g == group => return true,
                Some(g) => at = self.group_of(g),
                None => return false,
            }
        }
        false
    }

    /// The layers and groups directly in `group` (None: the top level), bottom first, without
    /// masks.
    pub fn members(&self, group: Option<LayerId>) -> Vec<LayerId> {
        self.layers.iter().filter(|l| l.kind != LayerKind::Mask && self.group_of(l.id) == group).map(|l| l.id).collect()
    }

    /// Everything inside group `id`, at any depth, masks included, bottom first.
    pub fn contents(&self, id: LayerId) -> Vec<LayerId> {
        self.layers.iter().filter(|l| self.is_inside(l.id, id)).map(|l| l.id).collect()
    }

    /// Where a layer sits in the stack with what goes with it: a paint layer with its mask, a
    /// group with everything in it (just below its entry).
    fn unit(&self, id: LayerId) -> Option<std::ops::Range<usize>> {
        let at = self.index_of(id)?;
        let l = self.layers.get(at)?;
        match l.kind {
            LayerKind::Paint => Some(at..at + 1 + usize::from(self.mask_of(id).is_some())),
            LayerKind::Mask => {
                let owner = self.layer(l.parent?).filter(|o| o.kind == LayerKind::Paint)?.id;
                self.unit(owner)
            }
            LayerKind::Group => {
                let mut start = at;
                while start > 0 && self.layers.get(start - 1).is_some_and(|b| self.is_inside(b.id, id)) {
                    start -= 1;
                }
                Some(start..at + 1)
            }
        }
    }

    /// The layer or group just below `id` in the same group, if any.
    pub fn below(&self, id: LayerId) -> Option<LayerId> {
        let start = self.unit(id)?.start;
        let b = self.layers.get(start.checked_sub(1)?)?;
        let b = if b.kind == LayerKind::Mask { b.parent? } else { b.id };
        (self.group_of(b) == self.group_of(id)).then_some(b)
    }

    /// Paint layers in the stack.
    fn paint_count(&self) -> usize {
        self.layers.iter().filter(|l| l.kind == LayerKind::Paint).count()
    }

    /// Adds a mask to paint layer `id` (just above it) and makes it current; the renderer fills it
    /// white. None if the layer can't take one or has one.
    pub fn add_mask(&mut self, id: LayerId) -> Option<LayerId> {
        let at = self.index_of(id)?;
        if self.layer(id)?.kind != LayerKind::Paint || self.mask_of(id).is_some() {
            return None;
        }
        let mask = self.blank(LayerKind::Mask, "Layer Mask".to_owned(), Some(id));
        let new = mask.id;
        self.layers.insert(at + 1, mask);
        self.current = new;
        Some(new)
    }

    /// Groups layer or group `id` with what is just below it in its group: both go in a new group
    /// there, or, when that is a group, `id` goes in it at its top. Returns the group; None if
    /// nothing is below.
    pub fn combine_down(&mut self, id: LayerId) -> Option<LayerId> {
        if self.layer(id)?.kind == LayerKind::Mask {
            return None;
        }
        let below = self.below(id)?;
        let unit = self.unit(id)?;
        if self.layer(below)?.kind == LayerKind::Group {
            // the group's entry moves up over the layer, which joins it at its top
            let at = self.index_of(below)?;
            let entry = self.layers.remove(at);
            self.layers.insert(unit.end - 1, entry);
            self.layer_mut(id)?.parent = Some(below);
            return Some(below);
        }
        let group = self.blank(LayerKind::Group, "New group".to_owned(), self.group_of(id));
        let new = group.id;
        self.layers.insert(unit.end, group);
        self.layer_mut(id)?.parent = Some(new);
        let lower = self.layer_mut(below)?;
        lower.parent = Some(new);
        // now at the bottom of the group, it has nothing to clip to
        lower.clipped = false;
        Some(new)
    }

    /// Takes the layers in group `id` out one level and removes the group; its top layer becomes
    /// current.
    pub fn ungroup(&mut self, id: LayerId) -> bool {
        if self.layer(id).is_none_or(|l| l.kind != LayerKind::Group) {
            return false;
        }
        let up = self.group_of(id);
        let members = self.members(Some(id));
        for m in &members {
            if let Some(l) = self.layer_mut(*m) {
                l.parent = up;
            }
        }
        let Some(at) = self.index_of(id) else { return false };
        self.layers.remove(at);
        if self.current == id {
            self.current = members.last().copied().unwrap_or(id);
        }
        self.settle_current(at);
        self.fix_clips();
        true
    }

    /// Replaces group `id` and everything in it with one paint layer where it was, named after its
    /// bottom layer and with the group's opacity, blend and visibility, made current. The renderer
    /// draws the group into it.
    pub fn flatten(&mut self, id: LayerId) -> Option<LayerId> {
        let group = self.layer(id).filter(|l| l.kind == LayerKind::Group)?.clone();
        let unit = self.unit(id)?;
        let name = self
            .layers
            .get(unit.clone())?
            .iter()
            .find(|l| l.kind == LayerKind::Paint)
            .map_or_else(|| group.name.clone(), |l| l.name.clone());
        let mut layer = self.blank(LayerKind::Paint, name, self.group_of(id));
        layer.visible = group.visible;
        layer.opacity = group.opacity;
        layer.blend = group.blend;
        let new = layer.id;
        let start = unit.start;
        self.layers.drain(unit);
        self.layers.insert(start, layer);
        self.current = new;
        self.fix_clips();
        Some(new)
    }

    pub fn set_collapsed(&mut self, id: LayerId, collapsed: bool) -> bool {
        self.layer_mut(id).filter(|l| l.kind == LayerKind::Group).map(|l| l.collapsed = collapsed).is_some()
    }

    /// A copy of `id` just above it (same name and settings), made current. The renderer copies
    /// the pixels.
    pub fn duplicate_layer(&mut self, id: LayerId) -> Option<LayerId> {
        self.duplicate(id).map(|(copy, _)| copy)
    }

    /// A copy of layer or group `id` (with its mask, or everything in it) just above it, made
    /// current; returns the copy and each (original, copy) pair of layers whose pixels the
    /// renderer copies.
    pub fn duplicate(&mut self, id: LayerId) -> Option<(LayerId, Vec<(LayerId, LayerId)>)> {
        if self.layer(id)?.kind == LayerKind::Mask {
            return None;
        }
        let unit = self.unit(id)?;
        let originals: Vec<Layer> = self.layers.get(unit.clone())?.to_vec();
        let mut new_ids = std::collections::HashMap::new();
        for l in &originals {
            let copy = self.new_id();
            new_ids.insert(l.id, copy);
        }
        let renamed = |old: LayerId| new_ids.get(&old).copied().unwrap_or(old);
        let copies: Vec<Layer> = originals
            .iter()
            .map(|l| {
                let mut c = l.clone();
                c.id = renamed(l.id);
                c.reference = false;
                if l.id != id {
                    c.parent = l.parent.map(renamed);
                }
                c
            })
            .collect();
        let pairs = originals.iter().filter(|l| l.kind != LayerKind::Group).map(|l| (l.id, renamed(l.id))).collect();
        let copy = renamed(id);
        self.layers.splice(unit.end..unit.end, copies);
        self.current = copy;
        Some((copy, pairs))
    }

    /// Removes a layer, with its mask, or a group with everything in it; the last paint layer
    /// can't be removed. The layer below (or above) becomes current.
    pub fn delete_layer(&mut self, id: LayerId) -> bool {
        let Some(layer) = self.layer(id) else { return false };
        let mut gone: Vec<LayerId> = match layer.kind {
            LayerKind::Group => self.contents(id),
            LayerKind::Paint => self.mask_of(id).into_iter().collect(),
            LayerKind::Mask => Vec::new(),
        };
        gone.push(id);
        let painted = self.layers.iter().filter(|l| l.kind == LayerKind::Paint && gone.contains(&l.id)).count();
        if self.paint_count() <= painted {
            return false;
        }
        let Some(at) = self.index_of(id) else { return false };
        let below = gone.iter().filter_map(|g| self.index_of(*g)).filter(|&i| i < at).count();
        self.layers.retain(|l| !gone.contains(&l.id));
        self.settle_current(at - below);
        self.fix_clips();
        true
    }

    /// When the current layer is gone, the one now below `at` (or the bottom one) becomes current.
    fn settle_current(&mut self, at: usize) {
        if self.layer(self.current).is_some() {
            return;
        }
        let next = at.saturating_sub(1).min(self.layers.len().saturating_sub(1));
        if let Some(l) = self.layers.get(next) {
            self.current = l.id;
        }
    }

    /// Unclips layers that no longer have a paint layer below them in their group.
    fn fix_clips(&mut self) {
        let stray: Vec<LayerId> =
            self.layers.iter().filter(|l| l.clipped && !self.can_clip(l.id)).map(|l| l.id).collect();
        for id in stray {
            if let Some(l) = self.layer_mut(id) {
                l.clipped = false;
            }
        }
    }

    /// Moves a paint layer (with its mask) or a group (with everything in it) to position `to`
    /// (0 = bottom, counted without them), clamped to the stack. It joins the group it lands in,
    /// or leaves its group.
    pub fn move_layer(&mut self, id: LayerId, to: usize) -> bool {
        if self.layer(id).is_none_or(|l| l.kind == LayerKind::Mask) {
            return false;
        }
        let Some(range) = self.unit(id) else { return false };
        let unit: Vec<Layer> = self.layers.drain(range).collect();
        let mut to = to.min(self.layers.len());
        // never between a layer and its mask
        if self.layers.get(to).is_some_and(|l| l.kind == LayerKind::Mask) {
            to += 1;
        }
        // landing below a group's entry, or among its layers, it joins that group
        let parent = self.layers.get(to).and_then(|above| match above.kind {
            LayerKind::Group => Some(above.id),
            _ => self.group_of(above.id),
        });
        for (k, mut l) in unit.into_iter().enumerate() {
            if l.id == id {
                l.parent = parent;
            }
            self.layers.insert(to + k, l);
        }
        self.fix_clips();
        true
    }

    /// What `id` merges into: for a paint layer, the paint layer just below it in its group; for a
    /// group, what its top layer merges into (see [`Document::group_merge_pair`]). A layer's mask
    /// and the one below's are each applied to its layer first.
    pub fn merge_target(&self, id: LayerId) -> Option<LayerId> {
        match self.layer(id)?.kind {
            LayerKind::Paint => self.below(id).filter(|b| self.layer(*b).is_some_and(|l| l.kind == LayerKind::Paint)),
            LayerKind::Group => self.group_merge_pair(id).map(|(_, into)| into),
            LayerKind::Mask => None,
        }
    }

    /// A group's Merge Down: its top paint layer (in a group in it, if that is on top) and the
    /// paint layer just under that in the stack, in the group or not. The group stays, emptied or
    /// not. None for an empty group, or with no paint layer under.
    pub fn group_merge_pair(&self, group: LayerId) -> Option<(LayerId, LayerId)> {
        let mut at = self.index_of(group)?;
        let top = loop {
            at = at.checked_sub(1)?;
            let l = self.layers.get(at)?;
            match l.kind {
                LayerKind::Paint => break l.id,
                LayerKind::Mask => break l.parent?,
                LayerKind::Group => {}
            }
        };
        if !self.is_inside(top, group) {
            return None;
        }
        let start = self.unit(top)?.start;
        let under = self.layers.get(start.checked_sub(1)?)?;
        let under = if under.kind == LayerKind::Mask { self.layer(under.parent?)? } else { under };
        (under.kind == LayerKind::Paint).then_some((top, under.id))
    }

    /// Removes `id` after it was merged down; the layer below becomes current. For a group, its
    /// top layer was merged (see [`Document::group_merge_pair`]): that goes, and the group stays
    /// current.
    pub fn finish_merge_down(&mut self, id: LayerId) -> bool {
        if self.layer(id).is_some_and(|l| l.kind == LayerKind::Group) {
            let Some((top, into)) = self.group_merge_pair(id) else { return false };
            return self.finish_merge_into(top, into, id);
        }
        let Some(below) = self.merge_target(id) else { return false };
        self.finish_merge_into(id, below, below)
    }

    /// Removes paint layer `upper` merged into `lower`, then makes `current` current.
    fn finish_merge_into(&mut self, upper: LayerId, lower: LayerId, current: LayerId) -> bool {
        let Some(unit) = self.unit(upper) else { return false };
        // its mask goes with it; the one below's was applied to that layer
        let mask = self.mask_of(lower);
        self.layers.drain(unit);
        self.layers.retain(|l| Some(l.id) != mask);
        // merged, it shows at full: its opacity went into its pixels (a hidden one's were let go)
        if let Some(l) = self.layer_mut(lower) {
            (l.visible, l.opacity) = (true, 1.0);
        }
        self.current = current;
        // merged into, a text layer's pixels are no longer just its text
        self.rasterize(lower);
        true
    }

    /// Whether paint layer `id` can be clipped: a paint layer is just below it in its group.
    pub fn can_clip(&self, id: LayerId) -> bool {
        self.layer(id).is_some_and(|l| l.kind == LayerKind::Paint)
            && self.below(id).and_then(|b| self.layer(b)).is_some_and(|b| b.kind == LayerKind::Paint)
    }

    /// The layer `id` is clipped to: the nearest unclipped paint layer below it in its group.
    pub fn clip_base(&self, id: LayerId) -> Option<LayerId> {
        if !self.layer(id)?.clipped {
            return None;
        }
        let mut at = self.below(id);
        for _ in 0..self.layers.len() {
            let l = self.layer(at?)?;
            if l.kind != LayerKind::Paint {
                return None;
            }
            if !l.clipped {
                return Some(l.id);
            }
            at = self.below(l.id);
        }
        None
    }

    pub fn set_visible(&mut self, id: LayerId, visible: bool) -> bool {
        self.layer_mut(id).map(|l| l.visible = visible).is_some()
    }

    /// Drawing assist on layer `id`: strokes on it follow the drawing guide.
    pub fn set_assisted(&mut self, id: LayerId, on: bool) -> bool {
        self.layer_mut(id).map(|l| l.assisted = on).is_some()
    }

    /// Whether strokes on `id` follow the drawing guide: its Drawing Assist, or for a mask its
    /// layer's (as measured, a mask shows "Assisted" with its layer).
    pub fn is_assisted(&self, id: LayerId) -> bool {
        self.layer(id).is_some_and(|l| {
            l.assisted
                || (l.kind == LayerKind::Mask && l.parent.and_then(|p| self.layer(p)).is_some_and(|o| o.assisted))
        })
    }

    /// The drawing guide as strokes on the current layer follow it: assisted when that layer
    /// has drawing assist.
    pub fn stroke_guide(&self) -> std::borrow::Cow<'_, crate::guide::Guide> {
        let assisted = self.is_assisted(self.current);
        if self.guide.assisted == assisted {
            std::borrow::Cow::Borrowed(&self.guide)
        } else {
            std::borrow::Cow::Owned(crate::guide::Guide { assisted, ..self.guide.clone() })
        }
    }

    pub fn set_private(&mut self, id: LayerId, private: bool) -> bool {
        self.layer_mut(id).map(|l| l.private = private).is_some()
    }

    /// The layers the gallery's thumbnail and the time-lapse leave out (see [`Layer::private`]).
    pub fn private_layers(&self) -> Vec<LayerId> {
        self.layers.iter().filter(|l| l.private).map(|l| l.id).collect()
    }

    pub fn set_opacity(&mut self, id: LayerId, opacity: f32) -> bool {
        let o = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { return false };
        self.layer_mut(id).map(|l| l.opacity = o).is_some()
    }

    pub fn set_blend(&mut self, id: LayerId, blend: BlendMode) -> bool {
        self.layer_mut(id).map(|l| l.blend = blend).is_some()
    }

    pub fn set_alpha_lock(&mut self, id: LayerId, on: bool) -> bool {
        self.layer_mut(id).map(|l| l.alpha_lock = on).is_some()
    }

    /// Locks `id`, or unlocks it: its own lock and those of the groups it is in (its row shows
    /// theirs, and its Unlock takes them off).
    pub fn set_locked(&mut self, id: LayerId, on: bool) -> bool {
        if !on {
            for g in self.locked_groups(id) {
                if let Some(l) = self.layer_mut(g) {
                    l.locked = false;
                }
            }
        }
        self.layer_mut(id).map(|l| l.locked = on).is_some()
    }

    /// Clipping needs a paint layer below in the same group.
    pub fn set_clipped(&mut self, id: LayerId, on: bool) -> bool {
        if on && !self.can_clip(id) {
            return false;
        }
        self.layer_mut(id).map(|l| l.clipped = on).is_some()
    }

    pub fn rename(&mut self, id: LayerId, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.layer_mut(id).map(|l| l.name = name.chars().take(256).collect()).is_some()
    }

    /// A document read from a file, made consistent: sizes in range, at least one layer, a current
    /// layer that exists, settings in range, ids that won't collide.
    pub fn repaired(mut self) -> Document {
        self.width = self.width.clamp(1, MAX_CANVAS_EDGE);
        self.height = self.height.clamp(1, MAX_CANVAS_EDGE);
        self.background = self.background.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 1.0 });
        self.dpi = if self.dpi.is_finite() { self.dpi.clamp(1.0, 9999.0) } else { default_dpi() };
        self.animation = self.animation.map(crate::animation::Animation::repaired);
        let mut seen = std::collections::HashSet::new();
        self.layers.retain(|l| seen.insert(l.id));
        for l in &mut self.layers {
            l.opacity = if l.opacity.is_finite() { l.opacity.clamp(0.0, 1.0) } else { 1.0 };
            if l.name.trim().is_empty() {
                l.name = "Layer".to_owned();
            }
        }
        let top = self.layers.iter().map(|l| l.id.0).max().unwrap_or(0);
        self.next_id = self.next_id.max(top + 1);
        // masks only just above their layers; parents only groups that exist
        let ids: Vec<LayerId> = self.layers.iter().map(|l| l.id).collect();
        let kinds: Vec<(LayerId, LayerKind)> = self.layers.iter().map(|l| (l.id, l.kind)).collect();
        let kind_of = |id: LayerId| kinds.iter().find(|(i, _)| *i == id).map(|(_, k)| *k);
        let mut keep = Vec::with_capacity(self.layers.len());
        for (i, l) in self.layers.iter().enumerate() {
            let ok = match l.kind {
                LayerKind::Mask => {
                    let below = i.checked_sub(1).and_then(|b| ids.get(b)).copied();
                    l.parent.is_some() && l.parent == below && below.and_then(kind_of) == Some(LayerKind::Paint)
                }
                _ => true,
            };
            keep.push(ok);
        }
        let mut k = keep.iter();
        self.layers.retain(|_| k.next().copied().unwrap_or(true));
        for l in &mut self.layers {
            if l.kind != LayerKind::Mask
                && (l.parent == Some(l.id) || l.parent.and_then(kind_of) != Some(LayerKind::Group))
            {
                l.parent = None;
            }
        }
        // groups inside themselves are taken out
        for id in ids {
            let mut seen = std::collections::HashSet::new();
            let mut at = Some(id);
            while let Some(a) = at {
                if !seen.insert(a) {
                    if let Some(l) = self.layer_mut(a) {
                        l.parent = None;
                    }
                    break;
                }
                at = self.group_of(a);
            }
        }
        self.tidy();
        if self.paint_count() == 0 {
            self.add_layer();
        }
        self.fix_clips();
        if self.layer(self.current).is_none() {
            self.current = self.layers.last().map_or(LayerId(0), |l| l.id);
        }
        // a layer shown alone that can't show (hidden, or in a hidden group) leaves only the
        // background, the panel's boxes still ticked: such an artwork opens with every layer shown
        if self.solo.is_some_and(|s| !self.shows(s)) {
            self.solo = None;
        }
        // drawing assist was once the guide's, not each layer's: it goes to the current layer
        if self.guide.assisted && !self.layers.iter().any(|l| l.assisted) {
            let current = self.current;
            if let Some(l) = self.layer_mut(current) {
                l.assisted = true;
            }
        }
        let (current, ids): (LayerId, Vec<LayerId>) = (self.current, self.layers.iter().map(|l| l.id).collect());
        self.picked.retain(|p| *p != current && ids.contains(p));
        self
    }

    /// Orders the stack so each group's contents sit just below it and each mask just above its
    /// layer, keeping the order otherwise.
    fn tidy(&mut self) {
        let mut order = Vec::with_capacity(self.layers.len());
        self.emit(None, &mut order, 0);
        let mut old = std::mem::take(&mut self.layers);
        for id in order {
            if let Some(i) = old.iter().position(|l| l.id == id) {
                self.layers.push(old.swap_remove(i));
            }
        }
    }

    /// The ids of `group`'s members in stack order, each group's contents before it.
    fn emit(&self, group: Option<LayerId>, order: &mut Vec<LayerId>, depth: usize) {
        if depth > self.layers.len() {
            return;
        }
        for m in self.members(group) {
            if self.layer(m).is_some_and(|l| l.kind == LayerKind::Group) {
                self.emit(Some(m), order, depth + 1);
                order.push(m);
            } else {
                order.push(m);
                if let Some(mask) = self.layers.iter().find(|l| l.kind == LayerKind::Mask && l.parent == Some(m)) {
                    order.push(mask.id);
                }
            }
        }
    }

    /// Every layer id, for checking what the renderer must keep.
    pub fn layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.layers.iter().map(|l| l.id)
    }

    /// The layers with pixels of their own (not groups).
    pub fn pixel_layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.layers.iter().filter(|l| l.kind != LayerKind::Group).map(|l| l.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mask_pixel_counts_by_its_brightness_and_greys_stay_as_they_are() {
        for g in 0..=255u8 {
            assert_eq!(mask_grey([g, g, g, 255]), g);
        }
        assert_eq!(mask_grey([255, 0, 0, 255]), 76);
        assert_eq!(mask_grey([0, 255, 0, 255]), 150);
        assert_eq!(mask_grey([0, 0, 255, 255]), 29);
    }

    #[test]
    fn new_document_has_one_current_layer() {
        let d = Document::new(0, 99_999);
        assert_eq!((d.width(), d.height()), (1, MAX_CANVAS_EDGE));
        assert_eq!(d.layers().len(), 1);
        assert_eq!(d.layers()[0].name, "Layer 1");
        assert_eq!(d.current(), d.layers()[0].id);
    }

    #[test]
    fn layers_go_above_the_current_one() {
        let mut d = Document::new(10, 10);
        let first = d.current();
        let second = d.add_layer();
        assert!(d.set_current(first));
        let third = d.add_layer();
        let order: Vec<LayerId> = d.layers().iter().map(|l| l.id).collect();
        assert_eq!(order, vec![first, third, second]);
        assert!(!d.set_current(LayerId(999)));
        assert_eq!(d.current(), third);
    }

    #[test]
    fn duplicate_delete_move() {
        let mut d = Document::new(10, 10);
        let a = d.current();
        d.set_opacity(a, 0.5);
        let b = d.duplicate_layer(a).unwrap();
        assert_eq!(d.layer(b).unwrap().opacity, 0.5);
        assert_eq!(d.layer(b).unwrap().name, d.layer(a).unwrap().name);
        assert_eq!(d.current(), b);
        assert!(d.move_layer(b, 0));
        assert_eq!(d.layers()[0].id, b);
        assert!(d.delete_layer(b));
        assert_eq!(d.current(), a);
        assert!(!d.delete_layer(a), "the last layer stays");
    }

    #[test]
    fn clipping_and_merge() {
        let mut d = Document::new(10, 10);
        let base = d.current();
        let mid = d.add_layer();
        let top = d.add_layer();
        assert!(!d.set_clipped(base, true));
        assert!(d.set_clipped(mid, true) && d.set_clipped(top, true));
        assert_eq!(d.clip_base(top), Some(base));
        assert_eq!(d.clip_base(base), None);
        assert_eq!(d.merge_target(top), Some(mid));
        assert_eq!(d.merge_target(base), None);
        assert!(d.finish_merge_down(top));
        assert_eq!(d.current(), mid);
        // moving a clipped layer to the bottom unclips it
        assert!(d.move_layer(mid, 0));
        assert!(!d.layers()[0].clipped);
    }

    #[test]
    fn masks_and_groups() {
        let mut d = Document::new(10, 10);
        let a = d.current();
        let b = d.add_layer();
        // a mask sits just above its layer and becomes current
        let m = d.add_mask(b).unwrap();
        assert_eq!(d.mask_of(b), Some(m));
        assert_eq!(d.current(), m);
        assert!(d.add_mask(b).is_none() && d.add_mask(m).is_none());
        // a new layer goes above the mask, not between
        let c = d.add_layer();
        assert_eq!(d.index_of(c), Some(3));
        assert_eq!(d.layer(c).unwrap().name, "Layer 4", "the mask counts in the number");
        // combine down: c and b (with its mask) in a group above them
        let g = d.combine_down(c).unwrap();
        assert_eq!(d.index_of(g), Some(4));
        assert_eq!(d.contents(g), vec![b, m, c]);
        assert_eq!(d.members(Some(g)), vec![b, c]);
        assert_eq!(d.group_of(m), Some(g));
        // merging needs neighbours in the same group, masked or not; clipping a paint layer below
        assert_eq!(d.merge_target(c), Some(b));
        assert!(d.can_clip(c) && !d.can_clip(b));
        // moved out to the bottom it leaves the group, and its mask comes with it
        assert!(d.move_layer(b, 0));
        assert_eq!(ids(&d), vec![b, m, a, c, g]);
        assert_eq!(d.group_of(b), None);
        assert_eq!(d.contents(g), vec![c]);
        // moved back under the group's entry it joins it
        assert!(d.move_layer(b, 2));
        assert_eq!(d.group_of(b), Some(g));
        // deleting a layer takes its mask; deleting the group takes everything in it
        assert!(d.delete_layer(g));
        assert_eq!(d.layers().len(), 1);
        assert_eq!(d.layers()[0].id, a);
        assert!(!d.delete_layer(a), "the last paint layer stays");
    }

    #[test]
    fn the_layer_cap_follows_the_canvas_size() {
        assert_eq!(Document::new(2048, 2048).layer_cap(), 128);
        assert_eq!(Document::new(16384, 16384).layer_cap(), 2);
        let d = Document::new(16384, 16384);
        assert!(d.has_room(1) && !d.has_room(2));
    }

    #[test]
    fn text_layers_keep_their_text_until_rasterized() {
        let mut d = Document::new(100, 100);
        let base = d.current();
        let text = LayerText {
            text: "\n  Hello there\nsecond".to_owned(),
            size: f32::NAN,
            opacity: 3.0,
            ..LayerText::default()
        };
        let t = d.add_text_layer(text);
        assert_eq!(d.current(), t);
        let layer = d.layer(t).unwrap();
        assert_eq!(layer.name, "Hello there");
        let kept = layer.text.clone().unwrap();
        assert_eq!((kept.size, kept.opacity), (64.0, 1.0));
        assert!(!d.set_text(base, kept.clone()), "an ordinary layer has no text");
        assert!(d.set_text(t, LayerText { text: "Bye".to_owned(), ..kept }));
        assert_eq!(d.layer(t).unwrap().name, "Bye");
        // saved and read back
        let back: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back.layer(t).unwrap().text, d.layer(t).unwrap().text);
        // a duplicate is a text layer too; merging another layer into it rasterizes it
        let copy = d.duplicate_layer(t).unwrap();
        assert!(d.layer(copy).unwrap().text.is_some());
        assert!(d.finish_merge_down(copy));
        assert!(d.layer(t).unwrap().text.is_none());
        assert!(!d.rasterize(t));
    }

    fn ids(d: &Document) -> Vec<LayerId> {
        d.layers().iter().map(|l| l.id).collect()
    }

    #[test]
    fn a_masked_layer_merges_down_and_both_masks_go() {
        let mut d = Document::new(10, 10);
        let a = d.current();
        let a_mask = d.add_mask(a).unwrap();
        assert!(d.set_current(a));
        let b = d.add_layer();
        let b_mask = d.add_mask(b).unwrap();
        assert_eq!(ids(&d), vec![a, a_mask, b, b_mask]);
        assert_eq!(d.merge_target(b), Some(a));
        assert!(d.finish_merge_down(b));
        assert_eq!((ids(&d), d.current()), (vec![a], a));
        // a group's Merge Down takes its top layer into the one under it, masked or not; the
        // group stays, emptied, and current
        let c = d.add_layer();
        let g = d.combine_down(c).unwrap();
        assert_eq!(ids(&d), vec![a, c, g]);
        let a_mask = d.add_mask(a).unwrap();
        assert_eq!((d.group_merge_pair(g), d.merge_target(g)), (Some((c, a)), Some(a)));
        assert!(d.finish_merge_down(g));
        assert_eq!((ids(&d), d.current()), (vec![a, g], g));
        assert!(d.layer(a_mask).is_none());
    }

    #[test]
    fn a_group_s_merge_down_merges_its_top_layer() {
        let mut d = Document::new(10, 10);
        let a = d.current();
        let b = d.add_layer();
        let c = d.add_layer();
        // b and c in a group above a: c goes into b, in the group
        let g = d.combine_down(c).unwrap();
        assert_eq!(ids(&d), vec![a, b, c, g]);
        assert_eq!(d.group_merge_pair(g), Some((c, b)));
        assert!(d.finish_merge_down(g));
        assert_eq!((ids(&d), d.current()), (vec![a, b, g], g));
        // its last layer goes into a, outside it; then empty, it has nothing to merge
        assert_eq!(d.group_merge_pair(g), Some((b, a)));
        assert!(d.finish_merge_down(g));
        assert_eq!(ids(&d), vec![a, g]);
        assert_eq!(d.merge_target(g), None);
    }

    #[test]
    fn groups_nest_ungroup_and_flatten() {
        let mut d = Document::new(10, 10);
        let a = d.current();
        let b = d.add_layer();
        let g = d.combine_down(b).unwrap();
        assert_eq!(ids(&d), vec![a, b, g]);
        // a layer above the group, combined down, joins it at its top
        assert!(d.set_current(g));
        let c = d.add_layer();
        assert_eq!(d.layer(c).unwrap().name, "Layer 4", "the group counts in the number");
        assert_eq!(d.group_of(c), None);
        assert_eq!(d.combine_down(c), Some(g));
        assert_eq!(ids(&d), vec![a, b, c, g]);
        // inside, combining makes a group in the group
        let inner = d.combine_down(c).unwrap();
        assert_eq!(ids(&d), vec![a, b, c, inner, g]);
        assert_eq!((d.depth(c), d.depth(inner), d.depth(a), d.max_depth()), (2, 1, 1, 2));
        assert_eq!(d.members(Some(g)), vec![a, inner]);
        assert_eq!((d.below(inner), d.merge_target(inner)), (Some(a), Some(b)));
        assert_eq!((d.below(b), d.merge_target(b)), (None, None));
        // a group moves with everything in it
        assert!(d.move_layer(inner, 0));
        assert_eq!(ids(&d), vec![b, c, inner, a, g]);
        assert_eq!(d.group_of(inner), Some(g));
        // ungrouping lifts the members one level; the top one becomes current
        assert!(d.set_current(g));
        assert!(d.ungroup(g));
        assert_eq!(ids(&d), vec![b, c, inner, a]);
        assert_eq!((d.current(), d.group_of(inner), d.depth(c)), (a, None, 1));
        // flattening leaves one layer where the group was, named after its bottom layer
        let flat = d.flatten(inner).unwrap();
        assert_eq!(ids(&d), vec![flat, a]);
        assert_eq!(d.layer(flat).unwrap().name, "Layer 2");
        assert_eq!(d.current(), flat);
        assert!(d.flatten(flat).is_none());
        // a group duplicates with everything in it; a layer with its mask
        let g = d.combine_down(a).unwrap();
        let m = d.add_mask(a).unwrap();
        let (copy, pairs) = d.duplicate(g).unwrap();
        assert_eq!(d.members(Some(copy)).len(), 2);
        assert_eq!(pairs.len(), 3);
        assert!(pairs.contains(&(m, d.mask_of(d.members(Some(copy))[1]).unwrap())));
        assert_eq!(d.index_of(copy), Some(d.layers().len() - 1));
    }

    #[test]
    fn repair_orders_groups_and_breaks_loops() {
        let mut d = Document::new(10, 10);
        let a = d.current();
        let b = d.add_layer();
        let g = d.combine_down(b).unwrap();
        // contents put back under their group, in their order
        let mut scrambled = d.clone();
        scrambled.layers.reverse();
        assert_eq!(ids(&scrambled.repaired()), vec![b, a, g]);
        // two groups inside each other
        assert!(d.set_current(g));
        let c = d.add_layer();
        let e = d.add_layer();
        let h = d.combine_down(e).unwrap();
        assert_eq!(ids(&d), vec![a, b, g, c, e, h]);
        let mut looped = d.clone();
        for l in &mut looped.layers {
            if l.id == g {
                l.parent = Some(h);
            }
            if l.id == h {
                l.parent = Some(g);
            }
        }
        let fixed = looped.repaired();
        assert_eq!(fixed.layers().len(), 6);
        assert!(fixed.max_depth() <= 2);
    }

    #[test]
    fn a_group_shown_alone_shows_what_is_in_it_and_one_that_cant_show_opens_without_solo() {
        let mut d = Document::new(10, 10);
        let below = d.current();
        let a = d.add_layer();
        let b = d.add_layer();
        let m = d.add_mask(b).unwrap();
        assert!(d.set_current(a));
        assert!(d.toggle_pick(b));
        let g = d.group_chosen().unwrap();
        assert!(d.set_current(g));
        let above = d.add_layer();
        d.solo = Some(g);
        for id in [g, a, b, m] {
            assert!(!d.soloed_out(id), "{id:?} is in the group shown alone");
        }
        assert!(d.soloed_out(below) && d.soloed_out(above));
        // nested: the outer group's layers show, the group around it too
        assert!(d.set_current(g));
        assert!(d.toggle_pick(below));
        let outer = d.group_chosen().unwrap();
        assert!(!d.soloed_out(outer) && !d.soloed_out(a) && d.soloed_out(below));
        d.solo = Some(outer);
        assert!(!d.soloed_out(below) && !d.soloed_out(a) && d.soloed_out(above));
        // saved shown alone and showing: kept
        d.solo = Some(g);
        assert_eq!(d.clone().repaired().solo, Some(g));
        // saved shown alone but hidden, or in a hidden group: every layer shows again on opening
        let mut hidden = d.clone();
        hidden.set_visible(g, false);
        assert_eq!(hidden.repaired().solo, None);
        let mut inside = d.clone();
        inside.solo = Some(a);
        assert_eq!(inside.clone().repaired().solo, Some(a));
        inside.set_visible(outer, false);
        let fixed = inside.repaired();
        assert_eq!(fixed.solo, None);
        assert_eq!(fixed.layers().len(), d.layers().len());
    }

    #[test]
    fn what_is_drawn_on_a_layer_is_seen_only_if_nothing_hides_it_or_leaves_it_out() {
        let mut d = Document::new(10, 10);
        let below = d.current();
        let a = d.add_layer();
        let m = d.add_mask(a).unwrap();
        assert!(d.set_current(a));
        let g = d.group_chosen().unwrap();
        for id in [below, a, m, g] {
            assert!(d.in_picture(id), "{id:?}");
        }
        // a group it is in hidden, or for a mask its layer
        d.set_visible(g, false);
        assert!(!d.in_picture(a) && !d.in_picture(m) && d.layer(a).unwrap().visible);
        d.set_visible(g, true);
        d.set_visible(a, false);
        assert!(!d.in_picture(m) && d.layer(m).unwrap().visible);
        d.set_visible(a, true);
        // another layer shown alone
        d.solo = Some(below);
        assert!(d.in_picture(below) && !d.in_picture(a) && !d.in_picture(m));
        // a mask shown alone shows its layer with it (not only the background)
        d.solo = Some(m);
        assert!(d.in_picture(a) && d.in_picture(m) && d.in_picture(g) && !d.in_picture(below));
        // one shown alone on a hidden layer can't show: it opens with every layer shown
        d.set_visible(a, false);
        assert_eq!(d.clone().repaired().solo, None);
    }

    #[test]
    fn a_change_goes_unseen_on_a_hidden_layer_in_a_hidden_group_or_a_group_holding_one() {
        let mut d = Document::new(10, 10);
        let below = d.current();
        let a = d.add_layer();
        let m = d.add_mask(a).unwrap();
        assert!(d.set_current(a));
        let inner = d.group_chosen().unwrap();
        assert!(d.set_current(inner));
        let outer = d.group_chosen().unwrap();
        for id in [below, a, m, inner, outer] {
            assert_eq!(d.unseen(id), None, "{id:?}");
        }
        // hidden itself; a mask on a hidden layer; anything in a hidden group, at any depth
        d.set_visible(a, false);
        assert_eq!(d.unseen(a), Some(Unseen::Itself));
        assert_eq!(d.unseen(m), Some(Unseen::MaskLayer));
        // a group with something hidden in it, at any depth, a mask too
        assert_eq!(d.unseen(inner), Some(Unseen::Inside));
        assert_eq!(d.unseen(outer), Some(Unseen::Inside));
        d.set_visible(a, true);
        d.set_visible(m, false);
        assert_eq!(d.unseen(outer), Some(Unseen::Inside));
        assert_eq!(d.unseen(a), None, "a hidden mask leaves its layer seen");
        d.set_visible(m, true);
        d.set_visible(outer, false);
        assert_eq!(d.unseen(outer), Some(Unseen::Itself));
        for id in [a, m, inner] {
            assert_eq!(d.unseen(id), Some(Unseen::Group), "{id:?}");
        }
        d.set_visible(outer, true);
        // left out while another is shown alone, unless a layer picked with it shows
        d.solo = Some(below);
        assert_eq!(d.unseen(a), Some(Unseen::Solo));
        assert_eq!(d.unseen(below), None);
        assert!(d.set_current(a));
        assert!(d.toggle_pick(below));
        assert_eq!(d.unseen(a), None);
        d.solo = Some(outer);
        assert!(d.set_current(below));
        assert!(d.toggle_pick(a));
        assert_eq!(d.unseen(below), None);
        d.clear_picks();
        assert_eq!(d.unseen(below), Some(Unseen::Solo));
    }

    #[test]
    fn repair_drops_stray_masks_and_parents() {
        let mut d = Document::new(10, 10);
        let a = d.current();
        let m = d.add_mask(a).unwrap();
        let mut broken = d.clone();
        broken.layers.swap(0, 1);
        let fixed = broken.repaired();
        assert!(fixed.layer(m).is_none());
        let mut orphan = d.clone();
        orphan.layers[0].parent = Some(LayerId(999));
        assert_eq!(orphan.repaired().layers()[0].parent, None);
    }

    #[test]
    fn saves_and_repairs() {
        let mut d = Document::new(300, 200);
        let a = d.current();
        d.add_layer();
        d.set_blend(a, BlendMode::Multiply);
        let json = serde_json::to_string(&d).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.clone().repaired(), d);
        // a broken file is made usable
        let broken = json.replace("\"current\":2", "\"current\":77").replace("\"width\":300", "\"width\":0");
        let fixed = serde_json::from_str::<Document>(&broken).unwrap().repaired();
        assert_eq!(fixed.width(), 1);
        assert!(fixed.layer(fixed.current()).is_some());
        let id = fixed.clone().add_layer();
        assert!(fixed.layer(id).is_none(), "new ids don't collide");
    }

    /// What holds in a document the app works on: ids unique; each group's parent a group in it
    /// and each mask's its paint layer just below; no group inside itself; each group's contents
    /// just below it; a paint layer; the current layer in it.
    fn assert_sound(d: &Document, why: &str) {
        let layers = d.layers();
        let ids: Vec<LayerId> = layers.iter().map(|l| l.id).collect();
        let unique: std::collections::HashSet<LayerId> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "{why}: ids repeat");
        for (i, l) in layers.iter().enumerate() {
            assert_ne!(l.parent, Some(l.id), "{why}: {:?} its own parent", l.id);
            if l.kind == LayerKind::Mask {
                let below = i.checked_sub(1).and_then(|b| layers.get(b));
                assert!(
                    below.is_some_and(|b| Some(b.id) == l.parent && b.kind == LayerKind::Paint),
                    "{why}: mask {:?} not over its layer",
                    l.id
                );
            } else if let Some(p) = l.parent {
                assert_eq!(d.layer(p).map(|g| g.kind), Some(LayerKind::Group), "{why}: {:?}'s parent", l.id);
            }
            let mut at = d.group_of(l.id);
            for _ in 0..=layers.len() {
                at = at.and_then(|g| d.group_of(g));
            }
            assert_eq!(at, None, "{why}: {:?} inside a loop of groups", l.id);
            if l.kind == LayerKind::Group {
                let inside = d.contents(l.id);
                assert!(
                    inside.iter().rev().zip((0..i).rev()).all(|(id, j)| layers.get(j).map(|x| x.id) == Some(*id)),
                    "{why}: group {:?}'s contents not just below it",
                    l.id
                );
            }
        }
        assert!(layers.iter().any(|l| l.kind == LayerKind::Paint), "{why}: no paint layer");
        assert!(d.layer(d.current()).is_some(), "{why}: the current layer");
    }

    #[test]
    fn a_document_damaged_anyhow_repairs_into_one_that_holds_together() {
        for seed in 0u64..2000 {
            let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let mut next = |n: usize| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state % n.max(1) as u64) as usize
            };
            // a document worked on: layers, masks, groups, moves, copies and deletions
            let mut d = Document::new(64, 64);
            let work = |d: &mut Document, n: usize, next: &mut dyn FnMut(usize) -> usize| {
                for _ in 0..n {
                    let ids: Vec<LayerId> = d.layers().iter().map(|l| l.id).collect();
                    let any = ids.get(next(ids.len())).copied().unwrap_or(LayerId(0));
                    match next(9) {
                        0 | 1 => _ = d.add_layer(),
                        2 => _ = d.add_mask(any),
                        3 => _ = d.combine_down(any),
                        4 => _ = d.set_current(any),
                        5 => _ = d.duplicate_layer(any),
                        6 => _ = d.delete_layer(any),
                        7 => _ = d.move_layer(any, next(ids.len() + 1)),
                        _ => _ = d.ungroup(any),
                    }
                }
            };
            work(&mut d, 4 + next(20), &mut next);
            assert_sound(&d, &format!("seed {seed} worked on"));
            // its file damaged: parents, kinds, ids and the current layer changed, entries dropped,
            // repeated and swapped, groups put inside each other, fields and settings made unreadable
            let mut v = serde_json::to_value(&d).unwrap();
            let top = d.layers().iter().map(|l| l.id.0).max().unwrap_or(0) + 3;
            for _ in 0..1 + next(4) {
                let Some(layers) = v["layers"].as_array_mut() else { break };
                let n = layers.len();
                let i = next(n);
                match next(10) {
                    // a field of a layer, or a setting, no longer readable
                    8 => {
                        if let Some(fields) = layers[i].as_object_mut() {
                            let key = fields.keys().nth(next(fields.len())).cloned().unwrap_or_default();
                            fields.insert(key, "?".into());
                        }
                    }
                    9 => v[["guide", "animation", "profile"][next(3)]] = 7.into(),
                    // two groups inside each other
                    7 => {
                        let j = next(n);
                        let (a, b) = (layers[i]["id"].clone(), layers[j]["id"].clone());
                        layers[i]["kind"] = "Group".into();
                        layers[j]["kind"] = "Group".into();
                        layers[i]["parent"] = b;
                        layers[j]["parent"] = a;
                    }
                    0 => {
                        layers[i]["parent"] =
                            if next(3) == 0 { serde_json::Value::Null } else { (next(top as usize) as u64).into() }
                    }
                    1 => layers[i]["kind"] = ["Paint", "Group", "Mask"][next(3)].into(),
                    2 => layers[i]["id"] = (next(top as usize) as u64).into(),
                    3 if n > 1 => _ = layers.remove(i),
                    4 => {
                        let copy = layers[i].clone();
                        layers.insert(next(n + 1), copy);
                    }
                    5 => layers.swap(i, next(n)),
                    _ => v["current"] = (next(top as usize) as u64).into(),
                }
            }
            let Ok(damaged) = serde_json::from_value::<Document>(v) else { continue };
            let why = format!("seed {seed} repaired");
            // every paint layer and group kept (masks not over their layer may go; of entries
            // sharing an id, the first stands)
            let mut first = std::collections::HashSet::new();
            let kept: Vec<LayerId> = damaged
                .layers()
                .iter()
                .filter(|l| first.insert(l.id) && l.kind != LayerKind::Mask)
                .map(|l| l.id)
                .collect();
            let fixed = damaged.repaired();
            assert_sound(&fixed, &why);
            for id in kept {
                assert!(fixed.layer(id).is_some(), "{why}: {id:?} lost");
            }
            assert_eq!(fixed.clone().repaired(), fixed, "{why}: a second repair changes it");
            // and worked on after
            let mut after = fixed;
            work(&mut after, 10, &mut next);
            assert_sound(&after, &format!("seed {seed} worked on after its repair"));
        }
    }

    #[test]
    fn a_file_with_a_layer_or_setting_it_cant_read_opens_with_the_rest() {
        let mut d = Document::new(40, 30);
        let a = d.current();
        let b = d.add_layer();
        let c = d.add_layer();
        d.guide.assisted = true;
        let mut v = serde_json::to_value(&d).unwrap();
        // the middle layer's opacity and the guide no longer what they should be
        v["layers"][1]["opacity"] = "half".into();
        v["guide"] = 7.into();
        let back = serde_json::from_value::<Document>(v).unwrap().repaired();
        // the middle layer kept with what of it can be read, its opacity as a new layer's
        assert_eq!(ids(&back), vec![a, b, c]);
        assert_eq!(back.layer(b).map(|l| (l.name.as_str(), l.opacity)), d.layer(b).map(|l| (l.name.as_str(), 1.0)));
        assert_eq!(back.guide, crate::guide::Guide::default());
        assert_eq!((back.width(), back.height()), (40, 30));
    }

    #[test]
    fn blend_ids_round_trip_and_settings_validate() {
        for m in BlendMode::ALL {
            assert_eq!(BlendMode::from_id(m.id()), Some(m));
        }
        assert_eq!(BlendMode::ALL.iter().map(|m| m.id()).max(), Some(BlendMode::MAX_ID));
        let mut d = Document::new(10, 10);
        let a = d.current();
        assert!(!d.set_opacity(a, f32::NAN));
        assert!(d.set_opacity(a, 3.0));
        assert_eq!(d.layer(a).unwrap().opacity, 1.0);
        assert!(!d.rename(a, "   "));
        assert!(d.rename(a, " Sketch "));
        assert_eq!(d.layer(a).unwrap().name, "Sketch");
    }

    /// A stack of `n` paint layers named 1..n, bottom first; the top one current.
    fn stack(n: usize) -> (Document, Vec<LayerId>) {
        let mut d = Document::new(10, 10);
        let mut ids = vec![d.current()];
        for _ in 1..n {
            ids.push(d.add_layer());
        }
        for (i, id) in ids.iter().enumerate() {
            d.rename(*id, &(i + 1).to_string());
        }
        (d, ids)
    }

    fn names(d: &Document) -> Vec<String> {
        d.layers().iter().map(|l| l.name.clone()).collect()
    }

    #[test]
    fn picks_toggle_and_never_hold_the_current_layer() {
        let (mut d, ids) = stack(4);
        let [a, b, c, top]: [LayerId; 4] = ids.try_into().unwrap();
        assert!(!d.toggle_pick(top));
        assert!(d.toggle_pick(a) && d.toggle_pick(c));
        assert_eq!(d.picked(), [a, c]);
        assert_eq!(d.chosen(), [a, c, top]);
        assert!(d.toggle_pick(c));
        assert_eq!(d.picked(), [a]);
        assert!(d.pick_all());
        assert_eq!(d.picked(), [a, b, c]);
        assert!(!d.pick_all());
        // a picked layer made current is no longer a pick
        d.set_current(b);
        assert_eq!(d.picked(), [a, c]);
        assert!(d.clear_picks() && !d.clear_picks());
    }

    #[test]
    fn picked_layers_group_where_the_top_one_was() {
        let (mut d, ids) = stack(5);
        let [one, _, three, _, five]: [LayerId; 5] = ids.try_into().unwrap();
        d.set_current(three);
        d.toggle_pick(one);
        d.toggle_pick(five);
        let g = d.group_chosen().unwrap();
        assert_eq!(names(&d), ["2", "4", "1", "3", "5", "New group"]);
        assert!([one, three, five].iter().all(|l| d.group_of(*l) == Some(g)));
        assert_eq!(d.current(), three);
        assert!(d.picked().is_empty());
    }

    #[test]
    fn picked_layers_duplicate_together_above_the_top_one() {
        let (mut d, ids) = stack(4);
        let [one, _, three, _]: [LayerId; 4] = ids.try_into().unwrap();
        d.set_current(three);
        d.toggle_pick(one);
        let pairs = d.duplicate_chosen().unwrap();
        assert_eq!(names(&d), ["1", "2", "3", "1", "3", "4"]);
        assert_eq!(pairs.len(), 2);
        // the copies are what is chosen now
        let copies: Vec<LayerId> = pairs.iter().map(|(_, c)| *c).collect();
        assert_eq!(d.chosen(), copies);
        assert_eq!(d.layer(d.current()).unwrap().name, "3");
    }

    #[test]
    fn picked_layers_move_together() {
        let (mut d, ids) = stack(5);
        let [one, _, three, _, _]: [LayerId; 5] = ids.try_into().unwrap();
        d.set_current(three);
        d.toggle_pick(one);
        // to the top of what is left: 2, 4, 5
        assert!(d.move_chosen(3));
        assert_eq!(names(&d), ["2", "4", "5", "1", "3"]);
        assert!(d.move_chosen(0));
        assert_eq!(names(&d), ["1", "3", "2", "4", "5"]);
    }

    #[test]
    fn picked_layers_delete_and_merge_into_the_bottom_one() {
        let (mut d, ids) = stack(4);
        let [one, two, three, four]: [LayerId; 4] = ids.try_into().unwrap();
        d.set_current(three);
        d.toggle_pick(one);
        assert_eq!(d.merge_chosen_plan(), Some((one, vec![three])));
        assert!(d.finish_merge_chosen(one, &[three]));
        assert_eq!(names(&d), ["1", "2", "4"]);
        assert_eq!(d.current(), one);
        d.toggle_pick(two);
        d.set_locked(four, true);
        d.toggle_pick(four);
        // a locked layer stops a merge, and a delete: nothing goes
        assert_eq!(d.merge_chosen_plan(), None);
        assert!(d.chosen_locked());
        assert!(!d.delete_chosen());
        assert_eq!(names(&d), ["1", "2", "4"]);
        // a locked layer in a chosen group stops it too
        d.set_locked(four, false);
        let group = d.group_chosen().unwrap();
        d.set_current(group);
        d.set_locked(two, true);
        assert!(d.chosen_locked() && !d.delete_chosen());
        d.set_locked(two, false);
        assert!(!d.chosen_locked());
    }

    #[test]
    fn picked_layers_merge_with_their_masks() {
        let (mut d, made) = stack(3);
        let [one, two, three]: [LayerId; 3] = made.try_into().unwrap();
        let one_mask = d.add_mask(one).unwrap();
        let three_mask = d.add_mask(three).unwrap();
        d.set_current(three);
        d.toggle_pick(one);
        assert_eq!(d.merge_chosen_plan(), Some((one, vec![three])));
        assert!(d.finish_merge_chosen(one, &[three]));
        assert_eq!(ids(&d), vec![one, two]);
        assert!(d.layer(one_mask).is_none() && d.layer(three_mask).is_none());
    }
}
