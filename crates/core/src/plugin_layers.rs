//! Layers of kinds plugins add (interface 3): a vector layer, say. Such a layer sits in the stack
//! as any other, and the app paints, shows, blends, undoes and saves its pixels as any layer's. A
//! stroke on it is painted live as on any layer, by the app's own brush engine, and then recorded as
//! an item (its samples, its brush, colour, size and opacity, and its tool) and handed to the plugin
//! that adds the kind, which keeps the layer's items in its data, in its own format. The app holds
//! that data in the document (see [`procraft_engine::document::PluginLayer`]), saves it beside the
//! layer's pixels in `layers/<id>.layerdata`, and ties each change of it to the canvas step that changed
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
//! A plugin that says which items a selection takes and changes them (`procraft_layer_select` and
//! `procraft_layer_edit`) has them moved, restyled and deleted as items: the app hands it the
//! selection, it answers with the items' ids, the app asks for changes by id (fields in place of an
//! item's own, or the item gone), and the layer is laid again as one step. An item's transform
//! ([`field::TRANSFORM`]) moves its samples where it is laid, its brush scaled with it; a stroke drawn
//! inside a selection keeps the selection ([`field::CLIP`]) and is laid within it again.
//!
//! Each layer's items are known where they reached as they were last laid ([`Laid`]): an edit then
//! lays again only where what it changed reaches, each item there laid within it, rather than the
//! whole layer.
//!
//! The brush an item names is kept in it as the brush library keeps a brush: the brush it came from
//! among those the app comes with, and every setting changed from there as it was when the stroke
//! was drawn, its own pictures in it (without its preview, maker, notches and reset point). So the
//! item draws the same whatever happens to the brush later, edited or deleted. Its name goes with
//! it, for finding it again by.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use procraft_engine::brush::Brush;
use procraft_engine::curve::Curve;
use procraft_engine::document::LayerId;
use procraft_engine::geom::{Affine, IRect, Vec2};
use procraft_engine::guide::{Guide, GuideKind, SymmetryKind};
use procraft_engine::input::Sample;
use procraft_engine::selection::Mask;
use procraft_engine::stroke::{InputSettings, StrokeBuilder};

use crate::plugin_run::LayerAdded;
use crate::{CanvasSession, PointerKind, Sliders, State, ToolKind};

/// The most a layer's data may be, and the most its items to draw may take, in bytes.
pub(crate) const MOST_DATA: usize = 64 << 20;
const MOST_DRAWN: usize = 96 << 20;
/// How many of the layers' changes are kept, by the step that made each: more than the canvas
/// keeps steps.
const STEPS_KEPT: usize = 256;
/// The most samples an item read back may hold, and the most pixels its selection may cover.
const MOST_SAMPLES: usize = 1 << 20;
const MOST_CLIP: usize = 1 << 28;
/// The most items a selection may take.
const MOST_TAKEN: usize = 1 << 20;
/// The most a file a plugin makes of an artwork's layers may be, in bytes.
const MOST_EXPORT: usize = 256 << 20;
/// The most of a layer's pixels handed to its plugin for a colour dropped on it, a side: a larger
/// canvas goes shrunk by whole steps.
const MOST_DROP_SIDE: u32 = 2048;
/// What an item's dual brush's randomness is seeded with, beside its own (as a stroke's is).
const SECOND_SEED: u32 = 0x5eed_0002;
/// The bit of a batch's count saying the pen lifted after it.
const LIFTED: u32 = 1 << 31;
/// The most of a layer laid again in part, as a share of it: more is laid again whole.
const MOST_IN_PART: f64 = 0.6;

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
    /// u32: in a layer's items to draw, the plugin's id for the item, which a selection and an edit
    /// name it by.
    pub(crate) const ID: u16 = 21;
    /// Six 32-bit floats `a b c d e f`: where the item is laid, its samples' (x, y) taken to
    /// (a·x + c·y + e, b·x + d·y + f), its brush scaled and its pen's lean turned with them.
    pub(crate) const TRANSFORM: u16 = 22;
    /// 32-bit float: the smoothing it is laid with in place of its brush's own (its streamline).
    pub(crate) const SMOOTHING: u16 = 23;
    /// The selection it was drawn within, if any: its box (x, y, width, height as u32), then its
    /// coverage in runs (a u32 count, then the byte), row by row.
    pub(crate) const CLIP: u16 = 24;
    /// A region it fills, in place of samples: u32 how many outlines, then each a u32 count of its
    /// points and them, x and y as 32-bit floats (canvas pixels, before its transform), filled
    /// even-odd in its colour at its opacity.
    pub(crate) const OUTLINE: u16 = 25;
    /// The shape it was snapped to (QuickShape): laid exactly along its samples (nothing smooths
    /// them), a dual brush's first alone; u8 its kind (line, polyline, arc, curve, ellipse, circle,
    /// triangle, equilateral triangle, quadrilateral, rectangle, square), then its nodes, x and y as
    /// 32-bit floats (canvas pixels, before its transform).
    pub(crate) const SHAPE: u16 = 26;
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

    /// It where `t` takes the stroke it copies: its centre moved and its axes turned with it.
    fn moved(self, t: Affine) -> Symmetry {
        let centre = t.apply(Vec2::new(self.centre[0], self.centre[1]));
        Symmetry { centre: [centre.x, centre.y], angle: self.angle + t.rotation(), ..self }
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
    /// The plugin's id for it, as its items to draw say it.
    pub(crate) id: Option<u32>,
    /// Where it is laid, if moved since it was drawn.
    pub(crate) transform: Option<Affine>,
    /// The smoothing it is laid with in place of its brush's own.
    pub(crate) smoothing: Option<f32>,
    /// The selection it was drawn within.
    pub(crate) clip: Option<Arc<Clip>>,
    /// The region it fills, if it is a fill (it then has no samples, and needs no brush).
    pub(crate) region: Option<Arc<Region>>,
    /// The shape it was snapped to, if it was: its kind and nodes.
    pub(crate) shape: Option<Arc<(u8, Vec<Vec2>)>>,
}

/// A region an item fills: its outlines (canvas pixels, before the item's transform), filled
/// even-odd.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Region {
    pub(crate) outlines: Vec<Vec<Vec2>>,
}

impl Region {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&u32::try_from(self.outlines.len()).unwrap_or(0).to_le_bytes());
        for outline in &self.outlines {
            out.extend_from_slice(&u32::try_from(outline.len()).unwrap_or(0).to_le_bytes());
            for p in outline {
                out.extend_from_slice(&p.x.to_le_bytes());
                out.extend_from_slice(&p.y.to_le_bytes());
            }
        }
        out
    }

    /// The region its field holds, if it holds up: whole outlines of points on the canvas, not
    /// too many of them.
    fn read(bytes: &[u8]) -> Option<Region> {
        let count = u32_of(bytes)? as usize;
        let mut at = 4;
        let mut outlines = Vec::new();
        let mut points = 0;
        for _ in 0..count {
            let n = u32_of(bytes.get(at..)?)? as usize;
            at += 4;
            points += n;
            if points > MOST_SAMPLES {
                return None;
            }
            let outline: Vec<Vec2> = floats_of(bytes.get(at..at + n * 8)?)
                .collect::<Vec<_>>()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[x, y]| Vec2::new(x, y))
                .collect();
            if outline.len() != n || !outline.iter().all(|p| p.is_finite()) {
                return None;
            }
            at += n * 8;
            if n >= 3 {
                outlines.push(outline);
            }
        }
        (!outlines.is_empty()).then_some(Region { outlines })
    }

    /// The pixels it covers through `to` on a `width` × `height` canvas: the box (one pixel to
    /// spare for its smoothed edge) and how much of each pixel there, rows top to bottom; none if
    /// it covers none.
    fn coverage(&self, to: Affine, width: u32, height: u32) -> Option<(IRect, Vec<u8>)> {
        let moved: Vec<Vec<Vec2>> = self.outlines.iter().map(|o| o.iter().map(|p| to.apply(*p)).collect()).collect();
        let all = moved.iter().flatten();
        let (mut l, mut t, mut r, mut b) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in all {
            (l, t, r, b) = (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y));
        }
        let rect = IRect::new(
            (l - 1.0).floor() as i32,
            (t - 1.0).floor() as i32,
            (r + 1.0).ceil() as i32,
            (b + 1.0).ceil() as i32,
        )
        .intersect(IRect::from_size(width, height));
        if rect.is_empty() {
            return None;
        }
        let mut pixmap = tiny_skia::Pixmap::new(rect.width(), rect.height())?;
        let mut path = tiny_skia::PathBuilder::new();
        let (ox, oy) = (rect.x0 as f32, rect.y0 as f32);
        for outline in &moved {
            let mut points = outline.iter();
            let Some(first) = points.next() else { continue };
            path.move_to(first.x - ox, first.y - oy);
            for p in points {
                path.line_to(p.x - ox, p.y - oy);
            }
            path.close();
        }
        let path = path.finish()?;
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(255, 255, 255, 255);
        paint.anti_alias = true;
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::EvenOdd, tiny_skia::Transform::identity(), None);
        let coverage: Vec<u8> = pixmap.data().as_chunks::<4>().0.iter().map(|px| px[3]).collect();
        Some((rect, coverage))
    }
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
        self.written(true)
    }

    /// It as a record, its brush in it or not.
    fn written(&self, brush: bool) -> Vec<u8> {
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
        if brush {
            r.field(field::BRUSH, &self.brush);
        }
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
        if let Some(t) = self.transform {
            r.floats(field::TRANSFORM, &affine_floats(t));
        }
        if let Some(smoothing) = self.smoothing {
            r.floats(field::SMOOTHING, &[smoothing]);
        }
        if let Some(clip) = &self.clip {
            r.field(field::CLIP, &clip.bytes());
        }
        if let Some(region) = &self.region {
            r.field(field::OUTLINE, &region.bytes());
        }
        if let Some(shape) = &self.shape {
            let mut bytes = vec![shape.0];
            for p in &shape.1 {
                bytes.extend_from_slice(&p.x.to_le_bytes());
                bytes.extend_from_slice(&p.y.to_le_bytes());
            }
            r.field(field::SHAPE, &bytes);
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
            id: None,
            transform: None,
            smoothing: None,
            clip: None,
            region: None,
            shape: None,
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
                field::ID => item.id = u32_of(bytes),
                field::TRANSFORM => {
                    item.transform = floats::<6>(bytes)
                        .map(|[a, b, c, d, e, f]| Affine { a, b, c, d, e, f })
                        .filter(|t| t.inverse().is_some());
                }
                field::SMOOTHING => item.smoothing = floats::<1>(bytes).map(|[v]| v.clamp(0.0, 1.0)),
                field::CLIP => item.clip = Clip::read(bytes).map(Arc::new),
                field::OUTLINE => item.region = Region::read(bytes).map(Arc::new),
                field::SHAPE => {
                    item.shape = bytes.first().map(|kind| {
                        let nodes = floats_of(bytes.get(1..).unwrap_or_default()).collect::<Vec<_>>();
                        let nodes = nodes.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y));
                        Arc::new((*kind, nodes.filter(|p| p.is_finite()).take(MOST_SAMPLES).collect()))
                    });
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
        // a fill needs no brush
        item.brush = match brush {
            Some(brush) => brush,
            None if item.region.is_some() => Arc::new(Vec::new()),
            None => return None,
        };
        // batches that don't add up to the samples are no use: laid as if they came at once
        let counted: u64 = item.batches.iter().map(|b| u64::from(b & !LIFTED)).sum();
        if counted != item.samples.len() as u64 {
            item.batches.clear();
        }
        (!item.samples.is_empty() || item.region.is_some()).then_some(item)
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

/// What each of `items` is, as far as laying it goes: two items laid alike have the same, items laid
/// differently not (their ids aside).
pub(crate) fn item_keys(items: &[Item]) -> Vec<u64> {
    // each brush once (the items read together share them)
    let mut brushes: Vec<(*const Vec<u8>, u64)> = Vec::new();
    items
        .iter()
        .map(|item| {
            let at = Arc::as_ptr(&item.brush);
            let brush = match brushes.iter().find(|(p, _)| *p == at) {
                Some(&(_, h)) => h,
                None => {
                    let mut h = DefaultHasher::new();
                    item.brush.hash(&mut h);
                    let h = h.finish();
                    brushes.push((at, h));
                    h
                }
            };
            let mut h = DefaultHasher::new();
            brush.hash(&mut h);
            item.written(false).hash(&mut h);
            h.finish()
        })
        .collect()
}

/// Where an item laid on a plugin's layer reached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LaidItem {
    /// What it is (see [`item_keys`]).
    key: u64,
    /// The pixels its stamps reach.
    bounds: IRect,
    /// It changed the layer as it went (smudging, painting wet): what was under it counts.
    in_place: bool,
}

/// A plugin's layer's pixels as its items were last laid: each item where it reached, in order,
/// and the layer's pixels' version then on the canvas then.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Laid {
    canvas: u64,
    version: u64,
    items: Arc<Vec<LaidItem>>,
}

/// What laying a layer's items again in part takes: the parts (apart from each other) to lay
/// again, and each item where it reaches and whether it is laid in place (those changed found
/// by measuring them).
struct InPart {
    parts: Vec<IRect>,
    items: Vec<LaidItem>,
    measured: Vec<bool>,
}

/// Which of `new` (keys) are new or changed since `old`, and where those of `old` that went (or
/// changed) reached; none if any is there twice, or those kept come in another order.
fn changed_items(old: &[LaidItem], new: &[u64]) -> Option<(Vec<bool>, Vec<IRect>)> {
    let mut olds = std::collections::HashSet::with_capacity(old.len());
    if !old.iter().all(|item| olds.insert(item.key)) {
        return None;
    }
    let mut news = std::collections::HashSet::with_capacity(new.len());
    if !new.iter().all(|key| news.insert(*key)) {
        return None;
    }
    let kept_old = old.iter().map(|i| i.key).filter(|k| news.contains(k));
    if !kept_old.eq(new.iter().copied().filter(|k| olds.contains(k))) {
        return None;
    }
    let added = new.iter().map(|k| !olds.contains(k)).collect();
    let gone = old.iter().filter(|o| !news.contains(&o.key)).map(|o| o.bounds).collect();
    Some((added, gone))
}

/// `rects` joined where they overlap, until none of them do.
fn apart(rects: Vec<IRect>) -> Vec<IRect> {
    let mut out: Vec<IRect> = Vec::new();
    for rect in rects.into_iter().filter(|r| !r.is_empty()) {
        let mut rect = rect;
        while let Some(at) = out.iter().position(|o| !o.intersect(rect).is_empty()) {
            rect = rect.union(out.swap_remove(at));
        }
        out.push(rect);
    }
    out
}

fn area(r: IRect) -> f64 {
    f64::from(r.width()) * f64::from(r.height())
}

/// An affine's six numbers, as an item's transform keeps them.
fn affine_floats(t: Affine) -> [f32; 6] {
    [t.a, t.b, t.c, t.d, t.e, t.f]
}

/// A selection a stroke was drawn within: the box it reaches, and how much it takes of each pixel
/// there (0 none, 255 all), rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Clip {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

impl Clip {
    /// The part of `mask` that takes anything; none if it takes nothing.
    pub(crate) fn of(mask: &Mask) -> Option<Clip> {
        let [x, y, width, height] = mask.bounds()?;
        let mut coverage = Vec::with_capacity(width as usize * height as usize);
        for row in y..y + height {
            let at = (row * mask.width + x) as usize;
            coverage.extend_from_slice(mask.data.get(at..at + width as usize)?);
        }
        Some(Clip { x, y, width, height, coverage })
    }

    /// It as its field holds it: the box, then the coverage in runs.
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [self.x, self.y, self.width, self.height] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let mut runs = self.coverage.chunk_by(|a, b| a == b);
        for run in runs.by_ref() {
            out.extend_from_slice(&u32::try_from(run.len()).unwrap_or(u32::MAX).to_le_bytes());
            out.push(run.first().copied().unwrap_or(0));
        }
        out
    }

    /// The selection its field holds, if it holds up: its runs filling its box exactly.
    fn read(bytes: &[u8]) -> Option<Clip> {
        let n = |at: usize| u32_of(bytes.get(at..)?);
        let (x, y, width, height) = (n(0)?, n(4)?, n(8)?, n(12)?);
        let area = (width as usize).checked_mul(height as usize).filter(|a| *a <= MOST_CLIP && *a > 0)?;
        let mut coverage = Vec::with_capacity(area);
        for run in bytes.get(16..)?.chunks(5) {
            let [a, b, c, d, value] = *run else { return None };
            let count = u32::from_le_bytes([a, b, c, d]) as usize;
            if coverage.len() + count > area {
                return None;
            }
            coverage.resize(coverage.len() + count, value);
        }
        (coverage.len() == area).then_some(Clip { x, y, width, height, coverage })
    }

    /// It as a selection of a `width` × `height` canvas each of whose pixels `back` takes to where
    /// it was drawn (nearest).
    fn mask_onto(&self, width: u32, height: u32, back: Affine) -> Mask {
        let mut mask = Mask::empty(width, height);
        let (x0, y0) = (self.x as f32, self.y as f32);
        for (y, row) in mask.data.chunks_exact_mut(width.max(1) as usize).enumerate() {
            for (x, v) in row.iter_mut().enumerate() {
                let p = back.apply(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
                let (cx, cy) = ((p.x - x0).floor(), (p.y - y0).floor());
                if cx >= 0.0 && cy >= 0.0 && cx < self.width as f32 && cy < self.height as f32 {
                    let at = cy as usize * self.width as usize + cx as usize;
                    *v = self.coverage.get(at).copied().unwrap_or(0);
                }
            }
        }
        mask
    }

    /// It as a selection of a `width` × `height` canvas, moved by `transform` (where its stroke is
    /// laid now).
    fn mask(&self, width: u32, height: u32, transform: Option<Affine>) -> Mask {
        let mut mask = Mask::empty(width, height);
        for row in 0..self.height.min(height.saturating_sub(self.y)) {
            let from = (row * self.width) as usize;
            let to = ((self.y + row) * width + self.x) as usize;
            let across = (self.width.min(width.saturating_sub(self.x))) as usize;
            if let (Some(src), Some(dst)) = (self.coverage.get(from..from + across), mask.data.get_mut(to..to + across))
            {
                dst.copy_from_slice(src);
            }
        }
        match transform.and_then(Affine::inverse) {
            Some(back) => mask.moved(|p| Some(back.apply(p))),
            None => mask,
        }
    }
}

/// A change asked of one of a layer's items: which, by its id, and whether it goes, or the fields
/// it takes in place of its own (a record).
pub(crate) struct Change {
    pub(crate) id: u32,
    pub(crate) delete: bool,
    pub(crate) fields: Vec<u8>,
}

/// `changes` as a plugin takes them: a record of changes, each a record of the id, whether it goes,
/// and the fields it takes.
fn changes_record(changes: &[Change]) -> Vec<u8> {
    let mut r = Record::default();
    for change in changes {
        let mut one = Record::default();
        one.field(1, &change.id.to_le_bytes()).field(2, &[u8::from(change.delete)]).field(3, &change.fields);
        r.field(1, &one.bytes());
    }
    r.bytes()
}

/// `mask` as a plugin takes a selection: its box (x, y, width, height) and its coverage there; none
/// if it takes nothing.
fn area_record(mask: &Mask) -> Option<Vec<u8>> {
    let clip = Clip::of(mask)?;
    let mut r = Record::default();
    r.field(1, &clip.x.to_le_bytes()).field(2, &clip.y.to_le_bytes());
    r.field(3, &clip.width.to_le_bytes()).field(4, &clip.height.to_le_bytes()).field(5, &clip.coverage);
    Some(r.bytes())
}

/// Which way the pen leant (`azimuth`, radians clockwise from up) once `t` has moved it.
fn turned_azimuth(t: Affine, azimuth: f32) -> f32 {
    let (x, y) = (azimuth.sin(), -azimuth.cos());
    let (tx, ty) = (t.a * x + t.c * y, t.b * x + t.d * y);
    if tx == 0.0 && ty == 0.0 { azimuth } else { tx.atan2(-ty) }
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
    /// The layer's pixels were as its items were last laid as it began.
    laid: bool,
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

    /// It made a shape (QuickShape) of kind `kind` (as a plugin is told it) with `nodes`, drawn
    /// along `samples`: kept as that, laid along them again exactly as the shape's stroke was drawn
    /// (all at once, its seed, nothing smoothing them, a dual brush's first alone).
    pub(crate) fn made_shape(&mut self, samples: &[Sample], kind: u8, nodes: &[Vec2]) {
        let item = &mut self.item;
        item.samples = samples.iter().filter(|s| s.is_finite()).copied().collect();
        item.batches = vec![u32::try_from(item.samples.len()).unwrap_or(0) | LIFTED];
        item.started = 0.0;
        item.seed = crate::quickshape::SHAPE_SEED;
        item.input = InputSettings::default();
        (item.rotation, item.seen_at) = (0.0, 1.0);
        item.shape = Some(Arc::new((kind, nodes.to_vec())));
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
    /// Where its items were laid before the step and after it, when known.
    laid: Option<LaidPair>,
}

/// Where a layer's items were laid before a step and after it.
type LaidPair = (Arc<Vec<LaidItem>>, Arc<Vec<LaidItem>>);

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
            id: None,
            transform: None,
            smoothing: None,
            // drawn within the selection: laid within it again
            clip: self.selection.as_ref().and_then(Clip::of).map(Arc::new),
            region: None,
            shape: None,
        };
        let laid = self.laid_current(layer);
        let mut recording = Recording { layer, kind, item, started: None, laid };
        recording.add(samples);
        self.recording = Some(recording);
    }

    /// Ends the open stroke (`builder`, and its dual brush's `second`), recorded on a plugin's layer
    /// as `recording`: handed to the plugin as an item, and laid as drawn if it keeps it so; the
    /// layer drawn again from its items if the plugin says to; not laid at all if it can't keep it.
    /// The layer's data from then on goes with the step made.
    pub(crate) fn end_recorded(&mut self, recording: Recording, builder: StrokeBuilder, second: Option<StrokeBuilder>) {
        let Recording { layer, kind, item, laid, .. } = recording;
        // (the stroke shown as it went changed the layer's pixels' version, not what they hold)
        let laid_before = self.laid.get(&layer).filter(|_| laid).map(|l| Arc::clone(&l.items));
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
                self.flush();
                // laid where it is laid now, after the rest
                let ended = self.canvas.as_ref().and_then(procraft_gpu::Canvas::last_stroke);
                if let (Some(mut items), Some((bounds, in_place))) = (laid_before.clone(), ended) {
                    let key = Item::read(&record, &[]).and_then(|i| item_keys(&[i]).first().copied()).unwrap_or(0);
                    Arc::make_mut(&mut items).push(LaidItem { key, bounds, in_place });
                    self.set_laid(layer, items);
                }
                after
            }
            Some(Some((after, Some(list)))) => {
                canvas.cancel_stroke();
                // as it was before the stroke: as its items were laid, if it was then
                self.flush();
                if laid {
                    self.trust_laid(layer);
                }
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
            let laid = laid_before.zip(self.laid_items(layer));
            self.note_layer_step(made, layer, before, after, laid);
        }
        self.revision += 1;
    }

    /// Where `layer`'s items reached as they were last laid, if its pixels are still as they laid
    /// them (nothing queued changes them either).
    pub(crate) fn laid_current(&self, layer: LayerId) -> bool {
        self.laid_items(layer).is_some() && self.canvas.as_ref().is_some_and(|canvas| !canvas.writes_queued(layer))
    }

    /// Where `layer`'s items reached as they were last laid, if its pixels are as they laid them as
    /// of the last render.
    fn laid_items(&self, layer: LayerId) -> Option<Arc<Vec<LaidItem>>> {
        let canvas = self.canvas.as_ref()?;
        let laid = self.laid.get(&layer)?;
        (laid.canvas == canvas.id() && laid.version == canvas.layer_version(layer)).then(|| Arc::clone(&laid.items))
    }

    /// `layer`'s pixels, as of the last render, are as they were when [`State::laid_current`] said
    /// they were as its items laid them (they were changed and put back since: a stroke taken back,
    /// a transform that moves items).
    pub(crate) fn trust_laid(&mut self, layer: LayerId) {
        if let (Some(canvas), Some(laid)) = (self.canvas.as_ref(), self.laid.get_mut(&layer))
            && laid.canvas == canvas.id()
        {
            laid.version = canvas.layer_version(layer);
        }
    }

    /// `layer`'s pixels, as of the last render, are as `items` laid them.
    fn set_laid(&mut self, layer: LayerId, items: Arc<Vec<LaidItem>>) {
        match self.canvas.as_ref() {
            Some(canvas) => {
                let laid = Laid { canvas: canvas.id(), version: canvas.layer_version(layer), items };
                self.laid.insert(layer, laid);
            }
            None => {
                self.laid.remove(&layer);
            }
        }
    }

    /// Lays `items` on `layer` again in order, through the brush engine in canvas pixels (masks as
    /// the eraser), as one step called `name`, made now: where its pixels are as its items were last
    /// laid, only where what changed reaches (cleared there first, each item there laid within it);
    /// else the whole layer, cleared first.
    fn lay_items_again(&mut self, layer: LayerId, items: &[Item], name: &str) {
        // what was queued before is made first; the selection is set aside meanwhile, so it
        // leaves out nothing of what was drawn
        self.flush();
        let keys = item_keys(items);
        let mut brushes: Vec<(Arc<Vec<u8>>, Brush)> = Vec::new();
        let in_part = self.in_part(layer, items, &keys, &mut brushes);
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else {
            self.laid.remove(&layer);
            return;
        };
        let selection = canvas.selection().map(<[u8]>::to_vec);
        if selection.is_some() {
            canvas.set_selection(gpu, None);
        }
        // one step, holding one copy of the layer as it was (none for each item)
        canvas.name_next_step(name);
        canvas.hold_layer_quietly(layer);
        let laid = in_part.and_then(|in_part| self.lay_in_part(layer, items, in_part, &mut brushes));
        let laid = match laid {
            Some(laid) => {
                self.lays[1] += 1;
                laid
            }
            None => {
                self.lays[0] += 1;
                if let Some(canvas) = self.canvas.as_mut() {
                    canvas.clear_layer(layer);
                }
                let mut laid = Vec::with_capacity(items.len());
                for (item, key) in items.iter().zip(&keys) {
                    let (bounds, in_place) = self.lay_clipped(layer, item, &mut brushes);
                    laid.push(LaidItem { key: *key, bounds, in_place });
                }
                laid
            }
        };
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.release_layer(true, None);
        }
        self.flush();
        if let (Some(canvas), Some(gpu), Some(selection)) = (self.canvas.as_mut(), self.gpu.as_ref(), selection) {
            canvas.set_selection(gpu, Some(&selection));
        }
        self.set_laid(layer, Arc::new(laid));
    }

    /// What laying `items` (their keys `keys`) on `layer` again in part takes, if its pixels are as
    /// its items were last laid and what changed reaches little enough of it: the items new or
    /// changed measured (not drawn). None to lay it all again.
    fn in_part(
        &mut self,
        layer: LayerId,
        items: &[Item],
        keys: &[u64],
        brushes: &mut Vec<(Arc<Vec<u8>>, Brush)>,
    ) -> Option<InPart> {
        let old = self.laid_items(layer)?;
        let (added, gone) = changed_items(&old, keys)?;
        let full = IRect::from_size(self.doc.width(), self.doc.height());
        let whole = area(full);
        // most of them changed, or what went reaches much of it: not worth measuring the rest
        let gone_area: f64 = apart(gone.iter().map(|r| r.intersect(full)).collect()).into_iter().map(area).sum();
        if added.iter().filter(|a| **a).count() * 2 > items.len().max(1) || gone_area > whole * MOST_IN_PART {
            return None;
        }
        // where those kept reach, and where those new or changed would
        let by_key: std::collections::HashMap<u64, LaidItem> = old.iter().map(|o| (o.key, *o)).collect();
        self.canvas.as_mut()?.measure(true);
        let mut laid = Some(Vec::with_capacity(items.len()));
        for ((item, &key), &new) in items.iter().zip(keys).zip(&added) {
            let one = if new {
                let (bounds, in_place) = self.lay_item(layer, item, brushes);
                Some(LaidItem { key, bounds, in_place })
            } else {
                by_key.get(&key).copied()
            };
            match (one, laid.as_mut()) {
                (Some(one), Some(laid)) => laid.push(one),
                _ => {
                    laid = None;
                    break;
                }
            }
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.measure(false);
        }
        let laid = laid?;
        let changed = laid.iter().zip(&added).filter(|(_, new)| **new).map(|(l, _)| l.bounds);
        let parts = apart(gone.into_iter().chain(changed).map(|r| r.intersect(full)).collect());
        if parts.iter().map(|r| area(*r)).sum::<f64>() > whole * MOST_IN_PART {
            return None;
        }
        // one laid in place there takes what is under it as it was: all laid again
        let touched = |item: &LaidItem| parts.iter().any(|p| !p.intersect(item.bounds).is_empty());
        if laid.iter().any(|item| item.in_place && touched(item)) {
            return None;
        }
        Some(InPart { parts, items: laid, measured: added })
    }

    /// Lays `items` again on `layer` as `in_part` says: each part cleared, then the items reaching
    /// it laid within it in order. Where each item reached; none if one new or changed reached
    /// beyond where it was measured to (the layer is then laid again whole).
    fn lay_in_part(
        &mut self,
        layer: LayerId,
        items: &[Item],
        in_part: InPart,
        brushes: &mut Vec<(Arc<Vec<u8>>, Brush)>,
    ) -> Option<Vec<LaidItem>> {
        let InPart { parts, items: laid, measured } = in_part;
        let mut fits = true;
        'parts: for part in &parts {
            let (Ok(x), Ok(y)) = (u32::try_from(part.x0), u32::try_from(part.y0)) else { continue };
            let (w, h) = (part.width(), part.height());
            let canvas = self.canvas.as_mut()?;
            canvas.put_layer_region(layer, x, y, w, h, vec![0; w as usize * h as usize * 4]);
            canvas.lay_within(Some(*part));
            for (item, (one, &new)) in items.iter().zip(laid.iter().zip(&measured)) {
                if part.intersect(one.bounds).is_empty() {
                    continue;
                }
                let (bounds, _) = self.lay_clipped(layer, item, brushes);
                if new && bounds.union(one.bounds) != one.bounds {
                    fits = false;
                    break 'parts;
                }
            }
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.lay_within(None);
            }
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.lay_within(None);
        }
        fits.then_some(laid)
    }

    /// Lays `item` on `layer` as [`State::lay_item`] does, within the selection it was drawn in
    /// (as the selection is read when the stroke is drawn, it is laid then).
    pub(crate) fn lay_clipped(
        &mut self,
        layer: LayerId,
        item: &Item,
        brushes: &mut Vec<(Arc<Vec<u8>>, Brush)>,
    ) -> (IRect, bool) {
        let (width, height) = (self.doc.width(), self.doc.height());
        let clip = item.clip.as_ref().and_then(|c| match self.lay_to {
            // laid on the sharp picture's canvas: its pixels back to where the stroke was drawn
            Some(to) => {
                let back = item.transform.unwrap_or(Affine::IDENTITY).then(to).inverse()?;
                Some(c.mask_onto(width, height, back))
            }
            None => Some(c.mask(width, height, item.transform)),
        });
        if let Some(clip) = &clip {
            self.flush();
            if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
                canvas.set_selection(gpu, Some(&clip.data));
            }
        }
        let laid = self.lay_item(layer, item, brushes);
        if clip.is_some() {
            self.flush();
            if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
                canvas.set_selection(gpu, None);
            }
        }
        laid
    }

    /// Lays `item` on `layer` as it was laid, its brush made again from what it keeps (or taken
    /// from `brushes`, made already): where it reached, and whether it changed the layer as it went
    /// (smudging, painting wet).
    fn lay_item(&mut self, layer: LayerId, item: &Item, brushes: &mut Vec<(Arc<Vec<u8>>, Brush)>) -> (IRect, bool) {
        if let Some(region) = &item.region {
            return self.lay_region(layer, item, region);
        }
        let raw = match brushes.iter().find(|(kept, _)| **kept == *item.brush) {
            Some((_, brush)) => brush.clone(),
            None => {
                let brush = self.brush_from_snapshot(&item.brush).unwrap_or_else(|| self.fallback.clone());
                brushes.push((Arc::clone(&item.brush), brush.clone()));
                brush
            }
        };
        // its sizes exactly as they were, and the smoothing it is given
        let mut raw = raw;
        if let [least, most, rest @ ..] = item.sizes.as_slice() {
            (raw.min_size, raw.max_size) = (*least, *most);
            if let ([least, most], Some(second)) = (rest, raw.secondary.as_deref_mut()) {
                (second.min_size, second.max_size) = (*least, *most);
            }
        }
        if let Some(smoothing) = item.smoothing {
            raw.streamline = smoothing;
        }
        // where it is laid: its samples moved, its brush scaled and its symmetry moved with them
        let t = item.transform.unwrap_or(Affine::IDENTITY);
        let scale = item.scale * t.scale_factor();
        let sliders = Sliders { size: item.size, opacity: item.opacity };
        let paint = crate::StrokePaint { colour: item.colour, secondary: item.secondary, alpha_lock: item.alpha_lock };
        let (brush, style, diameter) = self.stroke_brush_from(raw, sliders, item.tool, paint, false);
        // a shape: along its samples exactly, its first alone
        let exact = item.shape.is_some();
        let second = self.second_brush_from(item.tool, &brush, &style, sliders, paint).filter(|_| !exact);
        let make = |brush: Brush, diameter: f32, seed: u32| {
            let builder = StrokeBuilder::with_seed(brush, diameter, item.pointer.into(), seed)
                .scaled(scale)
                .with_input(item.input.clone())
                .on_view_turned(item.rotation)
                .seen_at(item.seen_at);
            if exact { builder.exact_path() } else { builder }
        };
        let mut builder = make(brush, diameter * scale, item.seed);
        let mut second_builder = second.as_ref().map(|(b, _, d)| make(b.clone(), d * scale, item.seed ^ SECOND_SEED));
        let guide = item.symmetry.map_or_else(Guide::default, |s| s.moved(t).guide());
        let Some(canvas) = self.canvas.as_mut() else { return (IRect::EMPTY, false) };
        match second {
            Some((_, second, _)) => canvas.begin_dual_stroke(layer, style, second),
            None => canvas.begin_stroke(layer, style),
        }
        // in the steps they came in: drawn as they came, the last as the pen lifted
        let whole = [item.samples.len() as u32];
        let batches = if item.batches.is_empty() { &whole[..] } else { &item.batches };
        // on the input's clock again, where it is laid
        let moved = item.transform.is_some();
        let timed: Vec<Sample> = item
            .samples
            .iter()
            .map(|s| Sample {
                pos: if moved { t.apply(s.pos) } else { s.pos },
                azimuth: if moved { turned_azimuth(t, s.azimuth) } else { s.azimuth },
                time_ms: item.started + s.time_ms,
                ..*s
            })
            .collect();
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
        canvas.last_stroke().unwrap_or((IRect::EMPTY, false))
    }

    /// Lays fill `item` (filling `region`) on `layer`: its colour at its opacity over what is there,
    /// where it is laid (on the sharp picture's canvas, there). Where it reaches; never in place.
    fn lay_region(&mut self, layer: LayerId, item: &Item, region: &Region) -> (IRect, bool) {
        let t = item.transform.unwrap_or(Affine::IDENTITY);
        let to = self.lay_to.map_or(t, |to| t.then(to));
        let (width, height) = (self.doc.width(), self.doc.height());
        let measuring = self.canvas.as_ref().is_some_and(procraft_gpu::Canvas::measuring);
        let Some((rect, coverage)) = region.coverage(to, width, height) else { return (IRect::EMPTY, false) };
        if !measuring && let Some(canvas) = self.canvas.as_mut() {
            let [r, g, b] = item.colour.map(|c| c.clamp(0.0, 1.0));
            let opacity = item.opacity.clamp(0.0, 1.0);
            let mut pixels = Vec::with_capacity(coverage.len() * 4);
            for &c in &coverage {
                let a = f32::from(c) / 255.0 * opacity;
                let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
                pixels.extend_from_slice(&[byte(r * a), byte(g * a), byte(b * a), byte(a)]);
            }
            canvas.lay_over(layer, rect, pixels);
        }
        (rect, false)
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
        // all of it, whatever is known of where its items were laid (and that put back with the
        // step: its data stays as it is)
        self.flush();
        let laid_before = self.laid_items(layer);
        self.laid.remove(&layer);
        let steps = self.canvas.as_ref().map_or(0, procraft_gpu::Canvas::steps_made);
        self.lay_items_again(layer, &read_drawn(&list), "Redraw layer");
        let made = self.canvas.as_ref().filter(|c| c.steps_made() > steps).and_then(procraft_gpu::Canvas::undo_step);
        if let Some(made) = made {
            let laid = laid_before.zip(self.laid_items(layer));
            self.note_layer_step(made, layer, data.clone(), data, laid);
        }
        self.revision += 1;
        true
    }

    /// The changes that give `items` the paint tool's colour (`colour`), brush (`brush`), size and
    /// opacity as they are now; masks keep theirs.
    fn restyled(&self, items: &[Item], colour: bool, brush: bool, size: bool, opacity: bool) -> Vec<Change> {
        let tool = ToolKind::Paint;
        let sliders = self.sliders(tool);
        let current = self.brush(tool).clone();
        let snapshot = brush.then(|| self.brush_snapshot(tool));
        let sizes: Vec<f32> = [Some(&current), current.secondary.as_deref()]
            .into_iter()
            .flatten()
            .flat_map(|b| [b.min_size, b.max_size])
            .collect();
        let mut out = Vec::new();
        for item in items.iter().filter(|i| i.tool != ToolKind::Erase) {
            let Some(id) = item.id else { continue };
            let mut r = Record::default();
            if colour {
                r.floats(field::COLOUR, &self.color);
            }
            if opacity {
                r.floats(field::OPACITY, &[sliders.opacity]);
            }
            if let Some(snapshot) = &snapshot {
                r.field(field::BRUSH, snapshot).field(field::BRUSH_NAME, current.name.as_bytes());
                r.floats(field::SIZES, &sizes);
            }
            let slider = if size { sliders.size } else { item.size };
            if size {
                r.floats(field::SIZE, &[slider]);
            }
            if brush || size {
                // its width for hit tests, with its brush as it is now
                let drawn = if brush { Some(current.clone()) } else { self.brush_from_snapshot(&item.brush) };
                if let Some(mut drawn) = drawn {
                    if let ([least, most, ..], false) = (item.sizes.as_slice(), brush) {
                        (drawn.min_size, drawn.max_size) = (*least, *most);
                    }
                    r.floats(field::WIDTH, &[drawn.diameter(slider) * item.scale]);
                }
            }
            out.push(Change { id, delete: false, fields: r.bytes() });
        }
        out
    }

    /// Undo or redo moved the canvas from `before` (the steps undo and redo would take then): a
    /// plugin's layer whose data that step changed has it back as it was before the step, or after.
    pub(crate) fn follow_layer_step(&mut self, before: (Option<u64>, Option<u64>)) {
        let Some((step, undone)) = self.canvas.as_ref().and_then(|c| crate::scene3d::stepped(before, c)) else {
            return;
        };
        // (one step may change several layers' data: flipping the canvas)
        type Back = (LayerId, Option<Arc<Vec<u8>>>, Option<Arc<Vec<LaidItem>>>);
        let changes: Vec<Back> = self
            .layer_steps
            .iter()
            .filter(|s| s.made == step)
            .map(|s| {
                let laid = s.laid.as_ref().map(|(before, after)| Arc::clone(if undone { before } else { after }));
                (s.layer, if undone { s.before.clone() } else { s.after.clone() }, laid)
            })
            .collect();
        for (layer, data, laid) in changes {
            self.doc.set_plugin_data(layer, data);
            // its pixels put back as they were laid then
            match laid {
                Some(laid) => self.set_laid(layer, laid),
                None => {
                    self.laid.remove(&layer);
                }
            }
        }
    }

    /// Keeps a change of `layer`'s data, from `before` to `after`, with canvas step `made`.
    fn note_layer_step(
        &mut self,
        made: u64,
        layer: LayerId,
        before: Option<Arc<Vec<u8>>>,
        after: Option<Arc<Vec<u8>>>,
        laid: Option<LaidPair>,
    ) {
        self.layer_steps.push(LayerStep { made, layer, before, after, laid });
        let over = self.layer_steps.len().saturating_sub(STEPS_KEPT);
        self.layer_steps.drain(..over);
    }

    /// `layer`'s kind and data, if it is a plugin's layer whose plugin can take changes to its items
    /// now (on, its data read). Whether the layer is locked is for the caller.
    fn editable_layer(&self, layer: LayerId) -> Option<(String, Option<Arc<Vec<u8>>>)> {
        if self.plugin_layer_off(layer) {
            return None;
        }
        let plugin = self.doc.layer(layer)?.plugin.as_ref()?;
        Some((plugin.kind.clone(), plugin.data.clone()))
    }

    /// `work` done by `layer`'s plugin on its data, handed to it first, if the plugin changes items
    /// (see [`State::editable_layer`]): none if it can't, stopped, or couldn't read the data.
    fn on_layer_data<T>(
        &self,
        layer: LayerId,
        work: impl FnOnce(&mut crate::plugin_run::Running) -> Result<T, crate::plugin_run::Stop>,
    ) -> Option<T> {
        let (kind, data) = self.editable_layer(layer)?;
        let done = self.hooks.as_ref()?.on_layer(&kind, |running, index| {
            if !running.layer_edits() || !running.layer_load(index, data.as_deref().map_or(&[][..], Vec::as_slice))? {
                return Ok(None);
            }
            work(running).map(Some)
        });
        done.flatten()
    }

    /// The items `layer` draws now, in order, each with its id; none if its plugin can't say.
    pub(crate) fn layer_items(&self, layer: LayerId) -> Option<Vec<Item>> {
        let list = self.on_layer_data(layer, |running| running.layer_draw(MOST_DRAWN))?;
        Some(read_drawn(&list))
    }

    /// Those of `items` (`layer`'s, in order) that may reach `rect`: by where each reached as last
    /// laid when that is known, else by its samples and its brush's width (and every one with a
    /// symmetry's copies).
    pub(crate) fn items_reaching(&self, layer: LayerId, items: Vec<Item>, rect: IRect) -> Vec<Item> {
        let laid = self.laid_items(layer).filter(|laid| laid.len() == items.len());
        let keys = laid.as_ref().map(|_| item_keys(&items));
        items
            .into_iter()
            .enumerate()
            .filter(|(i, item)| {
                let known = laid.as_ref().zip(keys.as_ref()).and_then(|(laid, keys)| {
                    let (one, key) = (laid.get(*i)?, keys.get(*i)?);
                    (one.key == *key).then_some(one.bounds)
                });
                let bounds = match known {
                    Some(bounds) => bounds,
                    None if item.symmetry.is_some() => return true,
                    None => {
                        let t = item.transform.unwrap_or(Affine::IDENTITY);
                        let margin = item.width * t.scale_factor() * 2.0 + 16.0;
                        items_reach(std::slice::from_ref(item)).map_or(IRect::EMPTY, |[l, t, r, b]| {
                            IRect::new(
                                (l - margin).floor() as i32,
                                (t - margin).floor() as i32,
                                (r + margin).ceil() as i32,
                                (b + margin).ceil() as i32,
                            )
                        })
                    }
                };
                !bounds.intersect(rect).is_empty()
            })
            .map(|(_, item)| item)
            .collect()
    }

    /// The items of `layer` the selection takes (every one with nothing selected), in order.
    pub(crate) fn taken_items(&self, layer: LayerId) -> Option<Vec<Item>> {
        let items = self.layer_items(layer)?;
        let Some(mask) = self.selection.as_ref() else { return Some(items) };
        let Some(area) = area_record(mask) else { return Some(Vec::new()) };
        let ids = self.on_layer_data(layer, |running| running.layer_select(&area, MOST_TAKEN))?;
        Some(items.into_iter().filter(|item| item.id.is_some_and(|id| ids.contains(&id))).collect())
    }

    /// `changes` made to `layer`'s items by its plugin, and the layer laid again, as one step called
    /// `name`: false if nothing changed or it couldn't.
    pub(crate) fn change_items(&mut self, layer: LayerId, changes: &[Change], name: &str) -> bool {
        if changes.is_empty() {
            return false;
        }
        let record = changes_record(changes);
        self.items_changed_by(layer, name, |running| running.layer_edit(&record))
    }

    /// `layer`'s items changed by its plugin doing `work` (true if it changed any), and the layer
    /// laid again, as one step called `name`: false if nothing changed or it couldn't.
    fn items_changed_by(
        &mut self,
        layer: LayerId,
        name: &str,
        work: impl FnOnce(&mut crate::plugin_run::Running) -> Result<bool, crate::plugin_run::Stop>,
    ) -> bool {
        if self.doc.is_locked(layer) {
            return false;
        }
        let before = self.doc.layer(layer).and_then(|l| l.plugin.as_ref()).and_then(|p| p.data.clone());
        let answer = self.on_layer_data(layer, |running| {
            if !work(running)? {
                return Ok(None);
            }
            Ok(Some((running.layer_save(MOST_DATA)?, running.layer_draw(MOST_DRAWN)?)))
        });
        let Some(Some((after, list))) = answer else { return false };
        let steps = self.canvas.as_ref().map_or(0, procraft_gpu::Canvas::steps_made);
        self.flush();
        let laid_before = self.laid_items(layer);
        self.lay_items_again(layer, &read_drawn(&list), name);
        let made = self.canvas.as_ref().filter(|c| c.steps_made() > steps).and_then(procraft_gpu::Canvas::undo_step);
        let after = Some(Arc::new(after));
        self.doc.set_plugin_data(layer, after.clone());
        if let Some(made) = made {
            let laid = laid_before.zip(self.laid_items(layer));
            self.note_layer_step(made, layer, before, after, laid);
        }
        self.revision += 1;
        true
    }

    /// Whether `layer` is a plugin's layer whose plugin takes a colour dropped on it (on, its data
    /// read).
    pub(crate) fn plugin_layer_fills(&self, layer: LayerId) -> bool {
        self.editable_layer(layer).is_some_and(|(kind, _)| {
            self.hooks.as_ref().and_then(|hooks| hooks.on_layer(&kind, |running, _| Ok(running.layer_fills())))
                == Some(true)
        })
    }

    /// Colour `colour` (sRGB 0..1) dropped on `layer` at canvas point `at` with `threshold` (0..1):
    /// handed to its plugin with the pixels that bound a fill there (as ColorDrop measures them:
    /// every layer seen while a plugin says so, the reference layer when another layer is it, else
    /// the layer's own), and the layer laid again with what it made of it, as one step. False if
    /// nothing changed.
    pub(crate) fn fill_plugin_layer(&mut self, layer: LayerId, at: Vec2, colour: [f32; 3], threshold: f32) -> bool {
        let (width, height) = (self.doc.width(), self.doc.height());
        let pixels = if self.hooks.as_ref().is_some_and(|hooks| hooks.fill_all_layers()) {
            self.composite_pixels()
        } else {
            match self.doc.reference().filter(|&r| r != layer) {
                Some(reference) => self.layer_pixels(reference),
                None => self.layer_pixels(layer),
            }
        };
        let Some(pixels) = pixels else { return false };
        // shrunk by whole steps to a size a plugin takes, each pixel the most of those it stands for
        let step = width.max(height).div_ceil(MOST_DROP_SIDE).max(1);
        let (w, h) = (width.div_ceil(step), height.div_ceil(step));
        let mut coverage = vec![0_u8; w as usize * h as usize];
        for (y, row) in pixels.as_chunks::<4>().0.chunks(width as usize).enumerate() {
            let out = (y as u32 / step) as usize * w as usize;
            for (x, px) in row.iter().enumerate() {
                if let Some(c) = coverage.get_mut(out + (x as u32 / step) as usize) {
                    *c = (*c).max(px[3]);
                }
            }
        }
        let mut r = Record::default();
        r.floats(1, &[at.x]).floats(2, &[at.y]).floats(3, &colour.map(|c| c.clamp(0.0, 1.0)));
        r.floats(4, &[threshold.clamp(0.0, 1.0)]).floats(5, &[step as f32]);
        let mut area = Record::default();
        area.field(1, &0u32.to_le_bytes()).field(2, &0u32.to_le_bytes());
        area.field(3, &w.to_le_bytes()).field(4, &h.to_le_bytes()).field(5, &coverage);
        r.field(6, &area.bytes());
        let record = r.bytes();
        self.items_changed_by(layer, "ColorDrop", |running| running.layer_fill(&record))
    }

    /// A tap of the selection tool at canvas point `at` on `layer`, `reach` canvas pixels about it:
    /// the item there joined to the selection as `how` says (as a selection of it alone would take
    /// it), or, with none there, the selection let go (if `clears`). False if it isn't a plugin's
    /// layer whose plugin says what is at a point, or nothing is there and it doesn't clear (the
    /// tap does as it does elsewhere).
    pub(crate) fn tap_items(
        &mut self,
        layer: LayerId,
        at: Vec2,
        reach: f32,
        how: crate::SelectionJoin,
        clears: bool,
    ) -> bool {
        if self.doc.is_locked(layer) {
            return false;
        }
        let mut r = Record::default();
        r.floats(1, &[at.x]).floats(2, &[at.y]).floats(3, &[reach.max(0.0)]);
        let request = r.bytes();
        let hit = self.on_layer_data(layer, |running| {
            if !running.layer_hits() {
                return Ok(None);
            }
            running.layer_hit(&request).map(Some)
        });
        let Some(Some(hit)) = hit else { return false };
        match hit.and_then(|id| self.layer_items(layer)?.into_iter().find(|i| i.id == Some(id))) {
            Some(item) => {
                let (width, height) = (self.doc.width(), self.doc.height());
                match item_mask(&item, width, height) {
                    Some(mask) => self.join(mask, how, true),
                    None => return false,
                }
            }
            None if clears => {
                let before = self.selection.take();
                self.record_selection(before, "clear selection");
            }
            None => return false,
        }
        true
    }

    /// Deletes the items of `layer` the selection takes (every one with nothing selected), as one
    /// step: false if none went.
    pub(crate) fn delete_taken_items(&mut self, layer: LayerId) -> bool {
        let Some(items) = self.taken_items(layer) else { return false };
        let changes: Vec<Change> =
            items.iter().filter_map(|i| Some(Change { id: i.id?, delete: true, fields: Vec::new() })).collect();
        self.change_items(layer, &changes, "Delete strokes")
    }

    /// `layer`'s data with each of its items moved by `t` after where it is (its pixels left as they
    /// are, moved by the caller): none if its plugin can't change its items, nothing within if it
    /// has none to move.
    pub(crate) fn moved_layer_data(&self, layer: LayerId, t: Affine) -> Option<Option<Arc<Vec<u8>>>> {
        let changes: Vec<Change> = self.layer_items(layer)?.iter().filter_map(|item| moved_change(item, t)).collect();
        if changes.is_empty() {
            return Some(None);
        }
        let record = changes_record(&changes);
        let after = self.on_layer_data(layer, |running| {
            if !running.layer_edit(&record)? {
                return Ok(None);
            }
            running.layer_save(MOST_DATA).map(Some)
        })?;
        Some(after.map(Arc::new))
    }

    /// Each plugin's layer's data with its items moved by `t` (the canvas cropped or flipped, its
    /// pixels with it): those whose plugin can't change their items become ordinary layers in
    /// `doc`, the others' data from then on given in it. The layers moved, and their data before and
    /// after.
    pub(crate) fn move_all_items(&self, t: Affine, doc: &mut procraft_engine::document::Document) -> Vec<MovedData> {
        let plugin_layers: Vec<LayerId> =
            self.doc.layers().iter().filter(|l| l.plugin.is_some()).map(|l| l.id).collect();
        let mut moved = Vec::new();
        for id in plugin_layers {
            match self.moved_layer_data(id, t) {
                Some(Some(after)) => {
                    let before = self.doc.layer(id).and_then(|l| l.plugin.as_ref()).and_then(|p| p.data.clone());
                    doc.set_plugin_data(id, Some(Arc::clone(&after)));
                    moved.push((id, before, Some(after)));
                }
                Some(None) => {}
                None => {
                    doc.rasterize(id);
                }
            }
        }
        moved
    }

    /// The layers' data moved by a step (see [`State::move_all_items`]), kept with step `made`.
    pub(crate) fn note_moved_data(&mut self, made: u64, moved: Vec<MovedData>) {
        for (layer, before, after) in moved {
            self.note_layer_step(made, layer, before, after, None);
        }
    }

    /// Moves the items the selection takes on `layer` (every one with nothing selected) by `t`, and
    /// lays the layer again, as one step called `name`: false if there were none, or it couldn't.
    pub(crate) fn move_taken_items(&mut self, layer: LayerId, ids: Option<&[u32]>, t: Affine, name: &str) -> bool {
        let Some(items) = (match ids {
            Some(ids) => self
                .layer_items(layer)
                .map(|all| all.into_iter().filter(|i| i.id.is_some_and(|id| ids.contains(&id))).collect::<Vec<_>>()),
            None => self.taken_items(layer),
        }) else {
            return false;
        };
        let changes: Vec<Change> = items.iter().filter_map(|item| moved_change(item, t)).collect();
        self.change_items(layer, &changes, name)
    }
}

/// A layer whose data a step changed: the layer, its data before and its data after.
pub(crate) type MovedData = (LayerId, Option<Arc<Vec<u8>>>, Option<Arc<Vec<u8>>>);

/// The change that moves `item` by `t` after where it is: its transform from then on.
fn moved_change(item: &Item, t: Affine) -> Option<Change> {
    let id = item.id?;
    let now = item.transform.unwrap_or(Affine::IDENTITY).then(t);
    let mut fields = Record::default();
    fields.floats(field::TRANSFORM, &affine_floats(now));
    Some(Change { id, delete: false, fields: fields.bytes() })
}

/// A selection of the pixels `item` covers on a `width` × `height` canvas, near enough: its paths
/// as wide as its brush, or its region; none if it covers none.
fn item_mask(item: &Item, width: u32, height: u32) -> Option<Mask> {
    let [l, t, r, b] = items_reach(std::slice::from_ref(item))?;
    let rect = IRect::new(
        (l - 1.0).floor() as i32,
        (t - 1.0).floor() as i32,
        (r + 1.0).ceil() as i32,
        (b + 1.0).ceil() as i32,
    )
    .intersect(IRect::from_size(width, height));
    if rect.is_empty() {
        return None;
    }
    let to = item.transform.unwrap_or(Affine::IDENTITY);
    let (ox, oy) = (rect.x0 as f32, rect.y0 as f32);
    let mut path = tiny_skia::PathBuilder::new();
    let filled = match &item.region {
        Some(region) => {
            for outline in &region.outlines {
                for (i, p) in outline.iter().map(|p| to.apply(*p)).enumerate() {
                    if i == 0 { path.move_to(p.x - ox, p.y - oy) } else { path.line_to(p.x - ox, p.y - oy) }
                }
                path.close();
            }
            true
        }
        None => {
            // the stroke and each copy its symmetry makes
            let own: Vec<Vec2> = item.samples.iter().map(|s| s.pos).collect();
            let mut paths = vec![own.clone()];
            if let Some(symmetry) = item.symmetry {
                let guide = symmetry.guide();
                let copies: Vec<Vec<Vec2>> =
                    own.iter().map(|p| guide.copies(*p).into_iter().map(|c| c.0).collect()).collect();
                for k in 0..copies.first().map_or(0, Vec::len) {
                    paths.push(copies.iter().filter_map(|c| c.get(k).copied()).collect());
                }
            }
            for copy in &paths {
                for (i, p) in copy.iter().map(|p| to.apply(*p)).enumerate() {
                    if i == 0 { path.move_to(p.x - ox, p.y - oy) } else { path.line_to(p.x - ox, p.y - oy) }
                }
                if let [only] = copy.as_slice() {
                    let p = to.apply(*only);
                    path.line_to(p.x - ox + 0.01, p.y - oy);
                }
            }
            false
        }
    };
    let path = path.finish()?;
    let mut pixmap = tiny_skia::Pixmap::new(rect.width(), rect.height())?;
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    if filled {
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::EvenOdd, tiny_skia::Transform::identity(), None);
    } else {
        let stroke = tiny_skia::Stroke {
            width: (item.width * to.scale_factor()).max(1.0),
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..tiny_skia::Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
    }
    let mut mask = Mask::empty(width, height);
    for (row, line) in pixmap.data().as_chunks::<4>().0.chunks(rect.width() as usize).enumerate() {
        let at = (rect.y0 as usize + row) * width as usize + rect.x0 as usize;
        if let Some(out) = mask.data.get_mut(at..at + line.len()) {
            for (o, px) in out.iter_mut().zip(line) {
                *o = px[3];
            }
        }
    }
    Some(mask)
}

/// What a selection's items reach: left, top, right and bottom, canvas pixels, their brushes'
/// widths round their samples; none for no items.
pub(crate) fn items_reach(items: &[Item]) -> Option<[f32; 4]> {
    let mut reach: Option<[f32; 4]> = None;
    for item in items {
        let t = item.transform.unwrap_or(Affine::IDENTITY);
        let half = item.width * t.scale_factor() / 2.0;
        let outline = item.region.iter().flat_map(|r| r.outlines.iter().flatten().map(|p| (*p, 0.0)));
        for (at, half) in item.samples.iter().map(|s| (s.pos, half)).chain(outline) {
            let p = t.apply(at);
            let [l, top, r, b] = reach.unwrap_or([p.x, p.y, p.x, p.y]);
            reach = Some([l.min(p.x - half), top.min(p.y - half), r.max(p.x + half), b.max(p.y + half)]);
        }
    }
    reach
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
            // empty, as no items lay it
            let mut st = self.shared.lock();
            st.flush();
            st.set_laid(LayerId(id), Arc::new(Vec::new()));
            drop(st);
            self.tell_plugins(crate::plugin_run::Hook::LayerAdded);
        }
        id
    }

    /// The files plugins on make of the artwork's layers of the kinds they add, as Share offers them:
    /// those of each kind the artwork has a layer of.
    pub fn plugin_exports(&self) -> Vec<crate::PluginExport> {
        let st = self.shared.lock();
        let Some(hooks) = st.hooks.as_ref() else { return Vec::new() };
        let mut kinds: Vec<&str> =
            st.doc.layers().iter().filter_map(|l| l.plugin.as_ref().map(|p| p.kind.as_str())).collect();
        kinds.dedup();
        let mut out: Vec<crate::PluginExport> = Vec::new();
        for kind in kinds {
            for export in hooks.layer_kind_exports(kind) {
                if !out.iter().any(|e| e.key == export.key) {
                    out.push(export);
                }
            }
        }
        out
    }

    /// The file plugin export `key` makes (see [`CanvasSession::plugin_exports`]) written to `path`,
    /// whole.
    pub fn export_plugin(&self, key: String, path: String) -> Result<(), crate::MobileError> {
        let reason = |why: &str| crate::MobileError::File { reason: why.to_owned() };
        let bytes = self.plugin_export_bytes(&key).ok_or_else(|| reason("the plugin couldn't make it"))?;
        crate::project::write_atomically(std::path::Path::new(&path), |p| {
            std::fs::write(p, &bytes).map_err(|e| reason(&format!("writing the file: {e}")))
        })
    }

    /// Layer `id`'s kind (`<id>.<key>`), if a plugin adds layers of it.
    pub fn plugin_layer_kind(&self, id: u64) -> Option<String> {
        self.shared.lock().plugin_layer_kind(LayerId(id)).map(str::to_owned)
    }

    /// The ids of the items the selection takes on the current layer (every one with nothing
    /// selected), if it is a plugin's layer whose plugin changes its items; none otherwise.
    pub fn taken_plugin_items(&self) -> Vec<u32> {
        let mut st = self.shared.lock();
        st.finish_stroke();
        let current = st.doc.current();
        st.taken_items(current).unwrap_or_default().iter().filter_map(|i| i.id).collect()
    }

    /// What those items reach: left, top, right and bottom, canvas pixels; empty for none.
    pub fn taken_plugin_items_reach(&self) -> Vec<f32> {
        let mut st = self.shared.lock();
        st.finish_stroke();
        let current = st.doc.current();
        st.taken_items(current).and_then(|items| items_reach(&items)).map(Vec::from).unwrap_or_default()
    }

    /// The box round the items a selection takes on the current layer, as the selection tool shows
    /// them taken: its corners (top left, top right, bottom right, bottom left), x then y, canvas
    /// pixels. Empty with nothing selected, on a layer whose plugin doesn't change its items, or with
    /// none taken.
    pub fn taken_plugin_items_box(&self) -> Vec<f32> {
        let mut st = self.shared.lock();
        st.finish_stroke();
        if st.selection.is_none() {
            return Vec::new();
        }
        let current = st.doc.current();
        let Some([l, t, r, b]) = st.taken_items(current).and_then(|items| items_reach(&items)) else {
            return Vec::new();
        };
        vec![l, t, r, t, r, b, l, b]
    }

    /// Moves the items a selection takes on the current layer, and the selection with them, `dx`
    /// across and `dy` down (canvas pixels), as one step: a small move, as an arrow key makes. False
    /// with nothing selected, or none taken.
    pub fn nudge_plugin_items(&self, dx: f32, dy: f32) -> bool {
        if !dx.is_finite() || !dy.is_finite() || (dx == 0.0 && dy == 0.0) {
            return false;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let current = st.doc.current();
        let Some(before) = st.selection.clone() else { return false };
        let t = Affine::translate(Vec2::new(dx, dy));
        let changes: Vec<Change> =
            st.taken_items(current).unwrap_or_default().iter().filter_map(|i| moved_change(i, t)).collect();
        if changes.is_empty() || st.doc.is_locked(current) {
            return false;
        }
        // the selection moved first, the items last: the step they make together is the items'
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.mark_steps();
        }
        let shift = Vec2::new(dx, dy);
        st.selection = Some(before.moved(|p| Some(p - shift)));
        st.record_selection(Some(before), "Move strokes");
        let done = st.change_items(current, &changes, "Move strokes");
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.collapse_steps();
        }
        st.flush();
        self.shared.frame(st);
        done
    }

    /// Deletes those items, as one step: their masks gone, what they hid shows again. False if none
    /// went.
    pub fn delete_plugin_items(&self) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let current = st.doc.current();
        let done = st.delete_taken_items(current);
        if done {
            self.shared.frame(st);
        }
        done
    }

    /// Moves those items by `transform` (six numbers as [`CanvasSession::view_matrix`] gives them, in
    /// canvas pixels), as one step laid exactly. False if none moved.
    pub fn transform_plugin_items(&self, transform: Vec<f32>) -> bool {
        let &[a, b, c, d, e, f] = transform.as_slice() else { return false };
        let t = Affine { a, b, c, d, e, f };
        if !t.is_finite() || t.inverse().is_none() {
            return false;
        }
        self.on_taken_items("Move strokes", |_, items| items.iter().filter_map(|i| moved_change(i, t)).collect())
    }

    /// Gives those items (masks aside) the paint tool's colour (`colour`), brush (`brush`), size
    /// (`size`) and opacity (`opacity`) as they are now, as one step. False if none changed.
    pub fn restyle_plugin_items(&self, colour: bool, brush: bool, size: bool, opacity: bool) -> bool {
        self.on_taken_items("Restyle strokes", |st, items| st.restyled(items, colour, brush, size, opacity))
    }

    /// Lays those items with `amount` of smoothing (0..1) in place of their brushes' own, as one
    /// step. False if none changed.
    pub fn smooth_plugin_items(&self, amount: f32) -> bool {
        if !amount.is_finite() {
            return false;
        }
        self.on_taken_items("Smooth strokes", |_, items| {
            let mut r = Record::default();
            r.floats(field::SMOOTHING, &[amount.clamp(0.0, 1.0)]);
            let fields = r.bytes();
            items.iter().filter_map(|i| Some(Change { id: i.id?, delete: false, fields: fields.clone() })).collect()
        })
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

impl CanvasSession {
    /// The file plugin export `key` (`<id>.<kind>.<key>`) makes of the artwork's layers of that kind,
    /// if a plugin on makes it: handed each of them bottom up (its name, whether it shows, its
    /// opacity and its data) and the canvas's size.
    fn plugin_export_bytes(&self, key: &str) -> Option<Vec<u8>> {
        let mut st = self.shared.lock();
        st.finish_stroke();
        let hooks = st.hooks.clone()?;
        let (kind, _) = key.rsplit_once('.')?;
        let index = hooks.layer_kind_exports(kind).iter().position(|e| e.key == key)?;
        let mut r = Record::default();
        r.field(1, &u32::try_from(index).ok()?.to_le_bytes());
        r.field(3, &st.doc.width().to_le_bytes()).field(4, &st.doc.height().to_le_bytes());
        for layer in st.doc.layers().iter().filter(|l| l.plugin.as_ref().is_some_and(|p| p.kind == kind)) {
            let mut one = Record::default();
            one.field(1, layer.name.as_bytes()).field(2, &[u8::from(st.doc.unseen(layer.id).is_none())]);
            one.floats(3, &[layer.opacity]);
            if let Some(data) = layer.plugin.as_ref().and_then(|p| p.data.as_deref()) {
                one.field(4, data);
            }
            r.field(5, &one.bytes());
        }
        let request = r.bytes();
        drop(st);
        hooks.on_layer(kind, |running, kind_index| {
            let mut request = request;
            let mut head = Record::default();
            head.field(2, &u32::try_from(kind_index).unwrap_or(0).to_le_bytes());
            request.extend(head.bytes());
            running.layer_export(&request, MOST_EXPORT)
        })?
    }

    /// The changes `changes` makes of the items the selection takes on the current layer (every one
    /// with nothing selected), made as one step called `name`: false if none changed.
    fn on_taken_items(&self, name: &str, changes: impl FnOnce(&State, &[Item]) -> Vec<Change>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let current = st.doc.current();
        let Some(items) = st.taken_items(current) else { return false };
        let changes = changes(st, &items);
        let done = st.change_items(current, &changes, name);
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
            id: None,
            transform: Some(Affine { a: 0.0, b: 2.0, c: -2.0, d: 0.0, e: 5.0, f: 6.0 }),
            smoothing: Some(0.25),
            clip: Some(Arc::new(Clip { x: 3, y: 4, width: 3, height: 2, coverage: vec![0, 0, 255, 255, 255, 9] })),
            region: None,
            shape: Some(Arc::new((4, vec![Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)]))),
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
