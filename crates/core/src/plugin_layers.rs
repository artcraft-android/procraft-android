//! Layers of kinds plugins add (interface 3): a vector layer, say. Such a layer sits in the stack
//! as any other, and the app paints, shows, blends, undoes and saves its pixels as any layer's. A
//! stroke on it is painted live as on any layer, by the app's own brush engine, and then recorded as
//! an item (its samples, its brush, colour, size and opacity, and its tool) and handed to the plugin
//! that adds the kind, which keeps the layer's items in its data, in its own format. The app holds
//! that data in the document (see [`procraft_engine::document::PluginLayer`]), saves it beside the
//! layer's pixels in `layers/<id>.vector`, and ties each change of it to the canvas step that changed
//! the pixels: undo and redo put back both together, between other layers' steps. When the plugin
//! changes items already drawn (the eraser taking whole strokes away, say), the app clears the layer
//! and lays its items again through the brush engine, in canvas pixels, as one step.
//!
//! Its plugin off or gone, or its data unreadable, the layer shows its pixels and takes no strokes,
//! and its data is kept as it was.
//!
//! An item, as the app hands it to a plugin and takes it back, is a record: fields one after another,
//! each a 16-bit id, a 32-bit length and that many bytes, all little-endian, in any order. A reader
//! passes over fields it doesn't know, so later versions add fields without breaking older ones; ids
//! from [`field::OWN`] up are the plugin's own, kept by the app as they come. The fields are in
//! [`field`]. A layer's items to draw are a record too: its brushes ([`drawn::BRUSH`], each the bytes
//! of one) and its items ([`drawn::ITEM`]), in order, each naming its brush by its place among them
//! ([`field::BRUSH_REF`]) or holding it ([`field::BRUSH`]).
//!
//! The brush an item names is kept in it as the brush library keeps a brush: the brush it came from
//! among those the app comes with, and every setting changed from there as it was when the stroke
//! was drawn, its own pictures in it (without its preview, maker, notches and reset point). So the
//! item draws the same whatever happens to the brush later, edited or deleted. Its name goes with
//! it, for finding it again by.

use std::sync::Arc;

use procraft_engine::brush::Brush;
use procraft_engine::curve::Curve;
use procraft_engine::document::LayerId;
use procraft_engine::geom::Vec2;
use procraft_engine::guide::{Guide, GuideKind, SymmetryKind};
use procraft_engine::input::Sample;
use procraft_engine::stroke::{InputSettings, StrokeBuilder};

use crate::plugin_run::LayerAdded;
use crate::{CanvasSession, PointerKind, Sliders, State, ToolKind};

/// The most a layer's data may be, and the most its items to draw may take, in bytes.
pub(crate) const MOST_DATA: usize = 64 << 20;
const MOST_DRAWN: usize = 96 << 20;
/// How many of the layers' changes are kept, by the step that made each: more than the canvas
/// keeps steps.
const STEPS_KEPT: usize = 256;
/// The most samples an item read back may hold.
const MOST_SAMPLES: usize = 1 << 20;
/// What an item's dual brush's randomness is seeded with, beside its own (as a stroke's is).
const SECOND_SEED: u32 = 0x5eed_0002;
/// The bit of a batch's count saying the pen lifted after it.
const LIFTED: u32 = 1 << 31;

/// The ids of an item's fields.
pub(crate) mod field {
    /// u8: 0 paint, 1 mask (drawn as the eraser, hiding what is under it), 2 smudge.
    pub(crate) const TOOL: u16 = 1;
    /// Each sample: x, y (canvas pixels), pressure (0..1), tilt, azimuth and roll (radians) as
    /// 32-bit floats, then the milliseconds since the stroke's first sample as a 64-bit float.
    pub(crate) const SAMPLES: u16 = 2;
    /// Three 32-bit floats: the colour, sRGB 0..1.
    pub(crate) const COLOUR: u16 = 3;
    /// Three 32-bit floats: the secondary colour, for secondary colour dynamics.
    pub(crate) const SECONDARY: u16 = 4;
    /// 32-bit float: the size slider, 0..1.
    pub(crate) const SIZE: u16 = 5;
    /// 32-bit float: the opacity slider, 0..1.
    pub(crate) const OPACITY: u16 = 6;
    /// 32-bit float: the brush's width at full pressure, canvas pixels (for hit tests).
    pub(crate) const WIDTH: u16 = 7;
    /// The brush, whole, as the app writes it (see the module's notes).
    pub(crate) const BRUSH: u16 = 8;
    /// UTF-8: the brush's name.
    pub(crate) const BRUSH_NAME: u16 = 9;
    /// u32: what the stroke's randomness was seeded with.
    pub(crate) const SEED: u16 = 10;
    /// u8: 0 a finger, 1 a pen, 2 a pen's eraser end, 3 a mouse.
    pub(crate) const POINTER: u16 = 11;
    /// 32-bit float: what the brush's size was multiplied by (a brush sized to the screen).
    pub(crate) const SCALE: u16 = 12;
    /// Two 32-bit floats: the view's turn (radians) and how many window points a canvas pixel
    /// took, as the stroke was drawn.
    pub(crate) const VIEW: u16 = 13;
    /// u8: the layer's alpha was locked.
    pub(crate) const ALPHA_LOCK: u16 = 14;
    /// The app's own smoothing and pressure: stabilisation, motion filtering, motion expression,
    /// pressure smoothing and speed distance as 32-bit floats, u8 tip attached, then the pressure
    /// curve's points (x and y, 32-bit floats).
    pub(crate) const INPUT: u16 = 15;
    /// The symmetry it was drawn with, if any: u8 kind (vertical, horizontal, quadrant, radial),
    /// u8 turned only, the centre (two 32-bit floats), the turn (32-bit float), u32 parts.
    pub(crate) const SYMMETRY: u16 = 16;
    /// u32: in a layer's items to draw, which of its brushes the item has.
    pub(crate) const BRUSH_REF: u16 = 17;
    /// u32 each: how the samples came, in turn, so they're laid again in the same steps (a wet brush
    /// mixes as they come): each how many, the top bit set on the last if the pen lifted after it.
    pub(crate) const BATCHES: u16 = 18;
    /// 64-bit float: when the first sample came, milliseconds on the input's clock (wet paint lays
    /// what came in each display frame together, by that clock).
    pub(crate) const STARTED: u16 = 19;
    /// 32-bit floats: the brush's sizes at the size slider's least and most, canvas pixels, then its
    /// dual brush's if it has one (the brush's own words keep them as slider places, a hair off).
    pub(crate) const SIZES: u16 = 20;
    /// The first id of a plugin's own fields.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const OWN: u16 = 0x8000;
}

/// The ids of the fields of a layer's items to draw.
pub(crate) mod drawn {
    /// A brush's bytes, as [`super::field::BRUSH`] holds them; numbered from 0 in order.
    pub(crate) const BRUSH: u16 = 1;
    /// An item.
    pub(crate) const ITEM: u16 = 2;
}

/// A record being written: its fields one after another.
#[derive(Default)]
pub(crate) struct Record(Vec<u8>);

impl Record {
    /// A field `id` holding `bytes` (left out if they're more than a field holds).
    pub(crate) fn field(&mut self, id: u16, bytes: &[u8]) -> &mut Self {
        if let Ok(len) = u32::try_from(bytes.len()) {
            self.0.extend_from_slice(&id.to_le_bytes());
            self.0.extend_from_slice(&len.to_le_bytes());
            self.0.extend_from_slice(bytes);
        }
        self
    }

    fn floats(&mut self, id: u16, values: &[f32]) -> &mut Self {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.field(id, &bytes)
    }

    pub(crate) fn bytes(self) -> Vec<u8> {
        self.0
    }
}

/// The fields of the record `bytes`, in order (id and bytes); none from one cut short on.
pub(crate) fn fields(bytes: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut rest = bytes;
    std::iter::from_fn(move || {
        let (head, after) = rest.split_at_checked(6)?;
        let id = u16::from_le_bytes([*head.first()?, *head.get(1)?]);
        let len = u32::from_le_bytes([*head.get(2)?, *head.get(3)?, *head.get(4)?, *head.get(5)?]) as usize;
        let (value, next) = after.split_at_checked(len)?;
        rest = next;
        Some((id, value))
    })
}

/// The 32-bit floats `bytes` holds, as many as are whole.
fn floats_of(bytes: &[u8]) -> impl Iterator<Item = f32> + '_ {
    bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b))
}

/// The first `N` 32-bit floats of `bytes`, if it holds them and each is a number.
fn floats<const N: usize>(bytes: &[u8]) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    let mut read = floats_of(bytes);
    for slot in &mut out {
        *slot = read.next().filter(|v| v.is_finite())?;
    }
    Some(out)
}

fn u32_of(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(*bytes.first_chunk::<4>()?))
}

/// The symmetry a stroke was drawn with: its copies made again as it lays again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Symmetry {
    kind: SymmetryKind,
    rotational: bool,
    centre: [f32; 2],
    angle: f32,
    parts: u32,
}

impl Symmetry {
    /// The symmetry `guide` copies a stroke with, if it copies one.
    fn of(guide: &Guide) -> Option<Symmetry> {
        (guide.on && guide.assisted && guide.kind == GuideKind::Symmetry).then(|| Symmetry {
            kind: guide.symmetry,
            rotational: guide.rotational,
            centre: guide.centre,
            angle: guide.angle,
            parts: guide.radial_parts(),
        })
    }

    /// A guide making the same copies.
    fn guide(self) -> Guide {
        Guide {
            on: true,
            kind: GuideKind::Symmetry,
            centre: self.centre,
            angle: self.angle,
            symmetry: self.kind,
            rotational: self.rotational,
            assisted: true,
            radial: Some(self.parts),
            ..Guide::default()
        }
    }

    fn bytes(self) -> Vec<u8> {
        let kind = match self.kind {
            SymmetryKind::Vertical => 0_u8,
            SymmetryKind::Horizontal => 1,
            SymmetryKind::Quadrant => 2,
            SymmetryKind::Radial => 3,
        };
        let mut out = vec![kind, u8::from(self.rotational)];
        for v in [self.centre[0], self.centre[1], self.angle] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.parts.to_le_bytes());
        out
    }

    fn read(bytes: &[u8]) -> Option<Symmetry> {
        let kind = match bytes.first()? {
            0 => SymmetryKind::Vertical,
            1 => SymmetryKind::Horizontal,
            2 => SymmetryKind::Quadrant,
            3 => SymmetryKind::Radial,
            _ => return None,
        };
        let rotational = *bytes.get(1)? != 0;
        let [x, y, angle] = floats::<3>(bytes.get(2..)?)?;
        let parts = u32_of(bytes.get(14..)?)?.clamp(2, 64);
        Some(Symmetry { kind, rotational, centre: [x, y], angle, parts })
    }
}

/// A stroke recorded on a plugin's layer: all it takes to lay it again as it was laid.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub(crate) tool: ToolKind,
    /// Canvas pixels; times in milliseconds since the first.
    pub(crate) samples: Vec<Sample>,
    /// When the first sample came, milliseconds on the input's clock.
    pub(crate) started: f64,
    /// How many samples came each time, the last marked as the pen lifting ([`LIFTED`]).
    pub(crate) batches: Vec<u32>,
    pub(crate) colour: [f32; 3],
    pub(crate) secondary: [f32; 3],
    pub(crate) size: f32,
    pub(crate) opacity: f32,
    pub(crate) width: f32,
    pub(crate) brush: Arc<Vec<u8>>,
    /// The brush's least and most sizes exactly, then its dual brush's.
    pub(crate) sizes: Vec<f32>,
    pub(crate) brush_name: String,
    pub(crate) seed: u32,
    pub(crate) pointer: PointerKind,
    pub(crate) scale: f32,
    pub(crate) rotation: f32,
    pub(crate) seen_at: f32,
    pub(crate) alpha_lock: bool,
    pub(crate) input: InputSettings,
    pub(crate) symmetry: Option<Symmetry>,
}

fn tool_number(tool: ToolKind) -> u8 {
    match tool {
        ToolKind::Paint => 0,
        ToolKind::Erase => 1,
        ToolKind::Smudge => 2,
    }
}

fn pointer_number(pointer: PointerKind) -> u8 {
    match pointer {
        PointerKind::Finger => 0,
        PointerKind::Stylus => 1,
        PointerKind::StylusEraser => 2,
        PointerKind::Mouse => 3,
    }
}

impl Item {
    /// It as a record, its brush in it.
    pub(crate) fn record(&self) -> Vec<u8> {
        let mut r = Record::default();
        r.field(field::TOOL, &[tool_number(self.tool)]);
        let mut samples = Vec::with_capacity(self.samples.len() * 32);
        for s in &self.samples {
            for v in [s.pos.x, s.pos.y, s.pressure, s.tilt, s.azimuth, s.roll] {
                samples.extend_from_slice(&v.to_le_bytes());
            }
            samples.extend_from_slice(&s.time_ms.to_le_bytes());
        }
        r.field(field::SAMPLES, &samples);
        r.floats(field::COLOUR, &self.colour);
        r.floats(field::SECONDARY, &self.secondary);
        r.floats(field::SIZE, &[self.size]);
        r.floats(field::OPACITY, &[self.opacity]);
        r.floats(field::WIDTH, &[self.width]);
        r.field(field::BRUSH, &self.brush);
        r.floats(field::SIZES, &self.sizes);
        r.field(field::BRUSH_NAME, self.brush_name.as_bytes());
        r.field(field::SEED, &self.seed.to_le_bytes());
        r.field(field::POINTER, &[pointer_number(self.pointer)]);
        r.floats(field::SCALE, &[self.scale]);
        r.floats(field::VIEW, &[self.rotation, self.seen_at]);
        r.field(field::ALPHA_LOCK, &[u8::from(self.alpha_lock)]);
        let i = &self.input;
        let mut input = Vec::new();
        for v in [i.stabilization, i.motion_filtering, i.motion_expression, i.pressure_smoothing, i.speed_distance] {
            input.extend_from_slice(&v.to_le_bytes());
        }
        input.push(u8::from(i.tip_attached));
        for p in i.pressure_curve.points() {
            input.extend_from_slice(&p.x.to_le_bytes());
            input.extend_from_slice(&p.y.to_le_bytes());
        }
        r.field(field::INPUT, &input);
        if let Some(symmetry) = self.symmetry {
            r.field(field::SYMMETRY, &symmetry.bytes());
        }
        let batches: Vec<u8> = self.batches.iter().flat_map(|b| b.to_le_bytes()).collect();
        r.field(field::BATCHES, &batches);
        r.field(field::STARTED, &self.started.to_le_bytes());
        r.bytes()
    }

    /// The item `record` holds, its brush there or among `brushes` (a layer's items to draw); none
    /// if it has no samples or no brush. A field missing or not a number is as a stroke starts.
    pub(crate) fn read(record: &[u8], brushes: &[Arc<Vec<u8>>]) -> Option<Item> {
        let mut item = Item {
            tool: ToolKind::Paint,
            samples: Vec::new(),
            started: 0.0,
            batches: Vec::new(),
            colour: [0.0; 3],
            secondary: [1.0; 3],
            size: 0.3,
            opacity: 1.0,
            width: 0.0,
            brush: Arc::new(Vec::new()),
            sizes: Vec::new(),
            brush_name: String::new(),
            seed: 0,
            pointer: PointerKind::Stylus,
            scale: 1.0,
            rotation: 0.0,
            seen_at: 1.0,
            alpha_lock: false,
            input: InputSettings::default(),
            symmetry: None,
        };
        let mut brush = None;
        for (id, bytes) in fields(record) {
            match id {
                field::TOOL => {
                    item.tool = match bytes.first() {
                        Some(1) => ToolKind::Erase,
                        Some(2) => ToolKind::Smudge,
                        _ => ToolKind::Paint,
                    }
                }
                field::SAMPLES => item.samples = samples_of(bytes),
                field::COLOUR => item.colour = floats::<3>(bytes).unwrap_or(item.colour).map(|c| c.clamp(0.0, 1.0)),
                field::SECONDARY => {
                    item.secondary = floats::<3>(bytes).unwrap_or(item.secondary).map(|c| c.clamp(0.0, 1.0));
                }
                field::SIZE => item.size = floats::<1>(bytes).map_or(item.size, |[v]| v.clamp(0.0, 1.0)),
                field::OPACITY => item.opacity = floats::<1>(bytes).map_or(item.opacity, |[v]| v.clamp(0.0, 1.0)),
                field::WIDTH => item.width = floats::<1>(bytes).map_or(item.width, |[v]| v.max(0.0)),
                field::BRUSH => brush = Some(Arc::new(bytes.to_vec())),
                field::BRUSH_REF => {
                    if let Some(found) = u32_of(bytes).and_then(|i| brushes.get(i as usize)) {
                        brush = Some(Arc::clone(found));
                    }
                }
                field::SIZES => item.sizes = floats_of(bytes).take(4).filter(|v| v.is_finite() && *v >= 0.0).collect(),
                field::BRUSH_NAME => item.brush_name = String::from_utf8_lossy(bytes).chars().take(256).collect(),
                field::SEED => item.seed = u32_of(bytes).unwrap_or(0),
                field::POINTER => {
                    item.pointer = match bytes.first() {
                        Some(0) => PointerKind::Finger,
                        Some(2) => PointerKind::StylusEraser,
                        Some(3) => PointerKind::Mouse,
                        _ => PointerKind::Stylus,
                    }
                }
                field::SCALE => item.scale = floats::<1>(bytes).map_or(1.0, |[v]| v.clamp(0.01, 100.0)),
                field::VIEW => {
                    if let Some([rotation, seen_at]) = floats::<2>(bytes) {
                        (item.rotation, item.seen_at) = (rotation, seen_at.clamp(1e-4, 1e4));
                    }
                }
                field::ALPHA_LOCK => item.alpha_lock = bytes.first().is_some_and(|b| *b != 0),
                field::INPUT => item.input = input_of(bytes).unwrap_or_default(),
                field::SYMMETRY => item.symmetry = Symmetry::read(bytes),
                field::STARTED => {
                    item.started = bytes
                        .first_chunk::<8>()
                        .map(|b| f64::from_le_bytes(*b))
                        .filter(|t| t.is_finite())
                        .unwrap_or(0.0);
                }
                field::BATCHES => {
                    item.batches = bytes.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
                }
                _ => {}
            }
        }
        item.brush = brush?;
        // batches that don't add up to the samples are no use: laid as if they came at once
        let counted: u64 = item.batches.iter().map(|b| u64::from(b & !LIFTED)).sum();
        if counted != item.samples.len() as u64 {
            item.batches.clear();
        }
        (!item.samples.is_empty()).then_some(item)
    }

    /// Its tool's step name, as undo calls a stroke of it.
    fn step_name(&self) -> &'static str {
        match self.tool {
            ToolKind::Erase => "Erase stroke",
            ToolKind::Smudge => "Smudge stroke",
            ToolKind::Paint => "Paint stroke",
        }
    }
}

/// The samples an item's field holds; none past one that isn't a place on the canvas.
fn samples_of(bytes: &[u8]) -> Vec<Sample> {
    let mut out = Vec::new();
    for chunk in bytes.as_chunks::<32>().0.iter().take(MOST_SAMPLES) {
        let Some([x, y, pressure, tilt, azimuth, roll]) = floats::<6>(chunk) else { break };
        let Some(time) = chunk.last_chunk::<8>().map(|t| f64::from_le_bytes(*t)).filter(|t| t.is_finite()) else {
            break;
        };
        out.push(Sample {
            pos: Vec2::new(x, y),
            pressure: pressure.clamp(0.0, 1.0),
            tilt,
            azimuth,
            roll,
            time_ms: time,
        });
    }
    out
}

/// The app's smoothing and pressure an item's field holds.
fn input_of(bytes: &[u8]) -> Option<InputSettings> {
    let [stabilization, motion_filtering, motion_expression, pressure_smoothing, speed_distance] = floats::<5>(bytes)?;
    let tip_attached = *bytes.get(20)? != 0;
    let points: Vec<Vec2> = floats_of(bytes.get(21..)?)
        .collect::<Vec<_>>()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[x, y]| Vec2::new(x, y))
        .collect();
    let unit = |v: f32| v.clamp(0.0, 1.0);
    Some(InputSettings {
        stabilization: unit(stabilization),
        motion_filtering: unit(motion_filtering),
        motion_expression: unit(motion_expression),
        pressure_smoothing: unit(pressure_smoothing),
        pressure_curve: if points.len() >= 2 { Curve::new(&points) } else { Curve::linear() },
        speed_distance: speed_distance.clamp(0.25, 1.0),
        tip_attached,
    })
}

/// A layer's items to draw, as a plugin gives them: in order, each with its brush. Those that
/// don't hold up are left out.
pub(crate) fn read_drawn(bytes: &[u8]) -> Vec<Item> {
    let mut brushes = Vec::new();
    let mut items = Vec::new();
    for (id, value) in fields(bytes) {
        match id {
            drawn::BRUSH => brushes.push(Arc::new(value.to_vec())),
            drawn::ITEM => items.extend(Item::read(value, &brushes)),
            _ => {}
        }
    }
    items
}

/// A stroke on a plugin's layer being drawn, recorded as it goes: the layer, its kind, and the
/// item so far.
pub(crate) struct Recording {
    layer: LayerId,
    kind: String,
    item: Item,
    /// When its first sample came (milliseconds on the input's clock).
    started: Option<f64>,
}

impl Recording {
    /// More of its samples (canvas pixels, as the stroke takes them), as they came.
    pub(crate) fn add(&mut self, samples: &[Sample]) {
        let before = self.item.samples.len();
        for s in samples.iter().filter(|s| s.is_finite()) {
            let started = *self.started.get_or_insert(s.time_ms);
            self.item.started = started;
            self.item.samples.push(Sample { time_ms: s.time_ms - started, ..*s });
        }
        self.item.batches.push(u32::try_from(self.item.samples.len() - before).unwrap_or(0));
    }

    /// The last of its samples, `samples`, as the pen lifts.
    pub(crate) fn lift(&mut self, samples: &[Sample]) {
        self.add(samples);
        if let Some(last) = self.item.batches.last_mut() {
            *last |= LIFTED;
        }
    }
}

/// A change to a plugin's layer's data, made by a canvas step: put back as the step is undone, and
/// again as it's redone.
pub(crate) struct LayerStep {
    made: u64,
    layer: LayerId,
    before: Option<Arc<Vec<u8>>>,
    after: Option<Arc<Vec<u8>>>,
}

impl State {
    /// The kind of plugin layer `layer` is (`<id>.<key>`), if it is one.
    pub(crate) fn plugin_layer_kind(&self, layer: LayerId) -> Option<&str> {
        self.doc.layer(layer).and_then(|l| l.plugin.as_ref()).map(|p| p.kind.as_str())
    }

    /// Whether `layer` is a plugin's layer that takes no strokes now: its plugin is off or gone, or
    /// its data couldn't be read.
    pub(crate) fn plugin_layer_off(&self, layer: LayerId) -> bool {
        let Some(kind) = self.plugin_layer_kind(layer) else { return false };
        self.layer_data_unread.contains(&layer)
            || !self.hooks.as_ref().is_some_and(|hooks| hooks.layer_kind_label(kind).is_some())
    }

    /// A stroke of `tool` with `pointer`, seeded `seed`, begins on `layer` with `samples` (canvas
    /// pixels): recorded as it goes if the layer is a plugin's.
    pub(crate) fn begin_recording(
        &mut self,
        layer: LayerId,
        tool: ToolKind,
        pointer: PointerKind,
        seed: u32,
        samples: &[Sample],
    ) {
        self.recording = None;
        let Some(kind) = self.plugin_layer_kind(layer).map(str::to_owned) else { return };
        let raw = self.brush(tool).clone();
        let sliders = self.sliders(tool);
        let scale = self.brush_scale();
        let item = Item {
            tool,
            samples: Vec::new(),
            colour: self.color,
            secondary: self.secondary,
            size: sliders.size,
            opacity: sliders.opacity,
            width: raw.diameter(sliders.size) * scale,
            brush: Arc::new(self.brush_snapshot(tool)),
            sizes: [Some(&raw), raw.secondary.as_deref()]
                .into_iter()
                .flatten()
                .flat_map(|b| [b.min_size, b.max_size])
                .collect(),
            brush_name: raw.name.clone(),
            seed,
            pointer,
            scale,
            rotation: self.view.map_or(0.0, |v| v.rotation()),
            seen_at: self.view.map_or(1.0, |v| v.zoom()) / self.display_scale,
            alpha_lock: self.doc.layer(layer).is_some_and(|l| l.alpha_lock),
            input: self.input.clone(),
            symmetry: Symmetry::of(&self.doc.stroke_guide()),
            batches: Vec::new(),
            started: 0.0,
        };
        let mut recording = Recording { layer, kind, item, started: None };
        recording.add(samples);
        self.recording = Some(recording);
    }

    /// Ends the open stroke (`builder`, and its dual brush's `second`), recorded on a plugin's layer
    /// as `recording`: handed to the plugin as an item, and laid as drawn if it keeps it so; the
    /// layer drawn again from its items if the plugin says to; not laid at all if it can't keep it.
    /// The layer's data from then on goes with the step made.
    pub(crate) fn end_recorded(&mut self, recording: Recording, builder: StrokeBuilder, second: Option<StrokeBuilder>) {
        let Recording { layer, kind, item, .. } = recording;
        let guide = self.doc.stroke_guide().into_owned();
        let before = self.doc.layer(layer).and_then(|l| l.plugin.as_ref()).and_then(|p| p.data.clone());
        let record = item.record();
        let answer = self.hooks.as_ref().and_then(|hooks| {
            hooks.on_layer(&kind, |running, index| {
                if !running.layer_load(index, before.as_deref().map_or(&[][..], Vec::as_slice))? {
                    return Ok(None);
                }
                let added = running.layer_add(&record)?;
                let after = running.layer_save(MOST_DATA)?;
                let drawn = match added {
                    LayerAdded::Kept => None,
                    LayerAdded::DrawAgain => Some(running.layer_draw(MOST_DRAWN)?),
                };
                Ok(Some((after, drawn)))
            })
        });
        let Some(canvas) = self.canvas.as_mut() else { return };
        let steps = canvas.steps_made();
        let after = match answer {
            Some(Some((after, None))) => {
                crate::close_stroke(canvas, builder, second, &guide);
                after
            }
            Some(Some((after, Some(list)))) => {
                canvas.cancel_stroke();
                // nothing changed (or a layer with nothing yet still has nothing): the layer stays
                // as it was, and no step is made
                let items = read_drawn(&list);
                if before.as_deref() == Some(&after) || (before.is_none() && items.is_empty()) {
                    return;
                }
                self.lay_items_again(layer, &items, item.step_name());
                after
            }
            answer => {
                // not kept (the plugin stopped, or couldn't read the layer's data): not laid either, so
                // the layer stays as its items draw it
                canvas.cancel_stroke();
                if answer.is_some() {
                    log::warn!("a plugin couldn't read its layer's data: the layer takes no more strokes");
                    self.layer_data_unread.insert(layer);
                }
                return;
            }
        };
        self.flush();
        let made = self.canvas.as_ref().filter(|c| c.steps_made() > steps).and_then(procraft_gpu::Canvas::undo_step);
        let after = Some(Arc::new(after));
        self.doc.set_plugin_data(layer, after.clone());
        if let Some(made) = made {
            self.layer_steps.push(LayerStep { made, layer, before, after });
            let over = self.layer_steps.len().saturating_sub(STEPS_KEPT);
            self.layer_steps.drain(..over);
        }
        self.revision += 1;
    }

    /// Clears `layer` and lays `items` on it again in order, through the brush engine in canvas
    /// pixels (masks as the eraser), as one step called `name`, made now.
    fn lay_items_again(&mut self, layer: LayerId, items: &[Item], name: &str) {
        // what was queued before is made first; the selection is set aside meanwhile, so it
        // leaves out nothing of what was drawn
        self.flush();
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return };
        let selection = canvas.selection().map(<[u8]>::to_vec);
        if selection.is_some() {
            canvas.set_selection(gpu, None);
        }
        canvas.mark_steps();
        canvas.name_next_step(name);
        canvas.clear_layer(layer);
        // each brush made again once
        let mut brushes: Vec<(Arc<Vec<u8>>, Brush)> = Vec::new();
        for item in items {
            self.lay_item(layer, item, &mut brushes);
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.collapse_steps();
        }
        self.flush();
        if let (Some(canvas), Some(gpu), Some(selection)) = (self.canvas.as_mut(), self.gpu.as_ref(), selection) {
            canvas.set_selection(gpu, Some(&selection));
        }
    }

    /// Lays `item` on `layer` as it was laid, its brush made again from what it keeps (or taken
    /// from `brushes`, made already).
    fn lay_item(&mut self, layer: LayerId, item: &Item, brushes: &mut Vec<(Arc<Vec<u8>>, Brush)>) {
        let raw = match brushes.iter().find(|(kept, _)| **kept == *item.brush) {
            Some((_, brush)) => brush.clone(),
            None => {
                let brush = self.brush_from_snapshot(&item.brush).unwrap_or_else(|| self.fallback.clone());
                brushes.push((Arc::clone(&item.brush), brush.clone()));
                brush
            }
        };
        // its sizes exactly as they were
        let mut raw = raw;
        if let [least, most, rest @ ..] = item.sizes.as_slice() {
            (raw.min_size, raw.max_size) = (*least, *most);
            if let ([least, most], Some(second)) = (rest, raw.secondary.as_deref_mut()) {
                (second.min_size, second.max_size) = (*least, *most);
            }
        }
        let sliders = Sliders { size: item.size, opacity: item.opacity };
        let paint = crate::StrokePaint { colour: item.colour, secondary: item.secondary, alpha_lock: item.alpha_lock };
        let (brush, style, diameter) = self.stroke_brush_from(raw, sliders, item.tool, paint, false);
        let second = self.second_brush_from(item.tool, &brush, &style, sliders, paint);
        let make = |brush: Brush, diameter: f32, seed: u32| {
            StrokeBuilder::with_seed(brush, diameter, item.pointer.into(), seed)
                .scaled(item.scale)
                .with_input(item.input.clone())
                .on_view_turned(item.rotation)
                .seen_at(item.seen_at)
        };
        let mut builder = make(brush, diameter * item.scale, item.seed);
        let mut second_builder =
            second.as_ref().map(|(b, _, d)| make(b.clone(), d * item.scale, item.seed ^ SECOND_SEED));
        let guide = item.symmetry.map_or_else(Guide::default, Symmetry::guide);
        let Some(canvas) = self.canvas.as_mut() else { return };
        match second {
            Some((_, second, _)) => canvas.begin_dual_stroke(layer, style, second),
            None => canvas.begin_stroke(layer, style),
        }
        // in the steps they came in: drawn as they came, the last as the pen lifted
        let whole = [item.samples.len() as u32];
        let batches = if item.batches.is_empty() { &whole[..] } else { &item.batches };
        // on the input's clock again
        let timed: Vec<Sample> =
            item.samples.iter().map(|s| Sample { time_ms: item.started + s.time_ms, ..*s }).collect();
        let mut at = 0;
        for &batch in batches {
            let n = (batch & !LIFTED) as usize;
            let samples = timed.get(at..at + n).unwrap_or_default();
            at += n;
            if batch & LIFTED == 0 {
                crate::draw_samples(canvas, &mut builder, second_builder.as_mut(), samples, &guide);
            } else {
                crate::lift_samples(canvas, &mut builder, second_builder.as_mut(), samples, &guide);
            }
        }
        crate::lay_stroke(canvas, builder, second_builder, &guide);
    }

    /// Asks `layer`'s plugin for its items and lays them again (see [`State::lay_items_again`]):
    /// false if it isn't a plugin's layer taking strokes, or the plugin couldn't.
    pub(crate) fn draw_plugin_layer_again(&mut self, layer: LayerId) -> bool {
        if self.plugin_layer_off(layer) || self.doc.is_locked(layer) {
            return false;
        }
        let Some(kind) = self.plugin_layer_kind(layer).map(str::to_owned) else { return false };
        // a stroke still open on it is kept first
        self.finish_stroke();
        let data = self.doc.layer(layer).and_then(|l| l.plugin.as_ref()).and_then(|p| p.data.clone());
        let list = self.hooks.as_ref().and_then(|hooks| {
            hooks.on_layer(&kind, |running, index| {
                if !running.layer_load(index, data.as_deref().map_or(&[][..], Vec::as_slice))? {
                    return Ok(None);
                }
                running.layer_draw(MOST_DRAWN).map(Some)
            })
        });
        let Some(Some(list)) = list else { return false };
        self.lay_items_again(layer, &read_drawn(&list), "Redraw layer");
        self.revision += 1;
        true
    }

    /// Undo or redo moved the canvas from `before` (the steps undo and redo would take then): a
    /// plugin's layer whose data that step changed has it back as it was before the step, or after.
    pub(crate) fn follow_layer_step(&mut self, before: (Option<u64>, Option<u64>)) {
        let Some((step, undone)) = self.canvas.as_ref().and_then(|c| crate::scene3d::stepped(before, c)) else {
            return;
        };
        let Some(change) = self.layer_steps.iter().rev().find(|s| s.made == step) else { return };
        let data = if undone { change.before.clone() } else { change.after.clone() };
        let layer = change.layer;
        self.doc.set_plugin_data(layer, data);
    }
}

#[uniffi::export]
impl CanvasSession {
    /// A new layer of a plugin's kind `kind` (`<id>.<key>`) above the current one, named after the
    /// kind and made current, as the Layers panel's + adds one: 0 if no plugin on adds that kind, or
    /// the canvas has no room for another.
    pub fn add_plugin_layer(&self, kind: String) -> u64 {
        let label = self.shared.lock().hooks.as_ref().and_then(|hooks| hooks.layer_kind_label(&kind));
        let Some(label) = label else { return 0 };
        let id = self
            .change("new layer", |d| {
                d.has_room(1).then(|| {
                    let name = format!("{label} {}", d.layers().len() + 1);
                    d.add_plugin_layer(kind, &name).0
                })
            })
            .unwrap_or(0);
        if id != 0 {
            self.tell_plugins(crate::plugin_run::Hook::LayerAdded);
        }
        id
    }

    /// Layer `id`'s kind (`<id>.<key>`), if a plugin adds layers of it.
    pub fn plugin_layer_kind(&self, id: u64) -> Option<String> {
        self.shared.lock().plugin_layer_kind(LayerId(id)).map(str::to_owned)
    }

    /// Clears plugin layer `id` and lays its items again, as its plugin has them, as one step: what
    /// a plugin's change to items already drawn takes. False if it isn't a plugin's layer taking
    /// strokes (its plugin off, say), or the plugin couldn't.
    pub fn draw_plugin_layer_again(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let done = st.draw_plugin_layer_again(LayerId(id));
        if done {
            self.shared.frame(st);
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn an_item() -> Item {
        Item {
            tool: ToolKind::Erase,
            samples: vec![
                Sample { pos: Vec2::new(1.0, 2.0), pressure: 0.5, tilt: 0.1, azimuth: 0.2, roll: 0.3, time_ms: 0.0 },
                Sample { pos: Vec2::new(3.0, 4.0), pressure: 1.0, tilt: 0.0, azimuth: 0.0, roll: 0.0, time_ms: 8.25 },
            ],
            batches: vec![1, 1 | LIFTED],
            started: 12_345.5,
            colour: [0.25, 0.5, 1.0],
            secondary: [1.0, 1.0, 0.0],
            size: 0.4,
            opacity: 0.75,
            width: 12.5,
            brush: Arc::new(b"{\"name\":\"Round\"}".to_vec()),
            sizes: vec![1.0, 40.0],
            brush_name: "Round".to_owned(),
            seed: 77,
            pointer: PointerKind::Finger,
            scale: 2.0,
            rotation: 0.5,
            seen_at: 0.25,
            alpha_lock: true,
            input: InputSettings {
                stabilization: 0.2,
                pressure_curve: Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.5, 0.7), Vec2::new(1.0, 1.0)]),
                tip_attached: false,
                ..InputSettings::default()
            },
            symmetry: Some(Symmetry {
                kind: SymmetryKind::Radial,
                rotational: true,
                centre: [10.0, 20.0],
                angle: 0.1,
                parts: 6,
            }),
        }
    }

    #[test]
    fn an_item_is_read_back_as_written_and_fields_it_doesn_t_know_are_passed_over() {
        let item = an_item();
        let mut record = item.record();
        assert_eq!(Item::read(&record, &[]), Some(item.clone()));
        // a later version's field, and a plugin's own, change nothing
        let mut more = Record::default();
        more.field(400, b"later").field(field::OWN + 3, &[9, 9]);
        record.extend(more.bytes());
        assert_eq!(Item::read(&record, &[]), Some(item.clone()));
        // cut short, what came whole is still read; without samples or a brush, it's no item
        let mut no_brush = Record::default();
        no_brush.field(field::SAMPLES, &[0; 32]);
        assert_eq!(Item::read(&no_brush.bytes(), &[]), None);
        let mut no_samples = Record::default();
        no_samples.field(field::BRUSH, b"{}");
        assert_eq!(Item::read(&no_samples.bytes(), &[]), None);
    }

    #[test]
    fn a_layer_s_items_to_draw_name_their_brushes_by_place() {
        let item = an_item();
        let brush = Arc::clone(&item.brush);
        // the item's own brush field left out, its brush named by its place
        let mut by_ref = Record::default();
        for (id, bytes) in fields(&item.record()).filter(|(id, _)| *id != field::BRUSH) {
            by_ref.field(id, bytes);
        }
        by_ref.field(field::BRUSH_REF, &0_u32.to_le_bytes());
        let mut list = Record::default();
        list.field(drawn::BRUSH, &brush).field(drawn::ITEM, &by_ref.bytes()).field(drawn::ITEM, &item.record());
        // and one naming a brush there isn't is left out
        let mut lost = Record::default();
        lost.field(field::SAMPLES, &[0; 32]).field(field::BRUSH_REF, &5_u32.to_le_bytes());
        list.field(drawn::ITEM, &lost.bytes());
        assert_eq!(read_drawn(&list.bytes()), vec![item.clone(), item]);
    }
}
