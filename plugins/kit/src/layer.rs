//! Kinds of layer of the plugin's own (its manifest's `layer_kinds`, interface 3): a vector layer,
//! say. Such a layer sits in the stack as any other. The app paints each stroke on it with its own
//! brush engine, as on any layer, then records the stroke as an [`Item`] and hands it to the plugin,
//! which keeps the layer's items in data of its own: the app holds that data, saves it beside the
//! layer's pixels, and undoes and redoes it with them. When the plugin changes items already drawn
//! (taking some away, say), it asks the app to draw the layer again from its items. The app can also
//! ask which items a selection takes ([`Layer::select`]) and change or delete them ([`Layer::edit`]),
//! hand it a colour dropped on the layer ([`Layer::fill`]), and ask it for a file of an artwork's
//! layers ([`Layer::export`]).
//! See `docs/plugins.md`. [`layers!`](crate::layers!) exports the calls.
//!
//! ```ignore
//! kit::layers!(MyLayer);
//! impl kit::layer::Layer for MyLayer { ... }
//! ```
//!
//! An item is a record: fields one after another, each a 16-bit id, a 32-bit length and that many
//! bytes, little-endian. [`fields`] reads one and [`Record`] writes one; [`field`] has the ids the
//! app writes. A plugin keeps an item's record as it came, so fields of a later app stay with it;
//! ids from [`field::OWN`] up are the plugin's own.

use std::sync::{Mutex, MutexGuard};

/// The ids of an item's fields, as the app writes them.
pub mod field {
    /// u8: 0 paint, 1 mask (drawn as the eraser, hiding what is under it), 2 smudge.
    pub const TOOL: u16 = 1;
    /// Each sample 32 bytes: x, y (canvas pixels), pressure, tilt, azimuth and roll as 32-bit
    /// floats, then the milliseconds since the stroke's first sample as a 64-bit float.
    pub const SAMPLES: u16 = 2;
    /// Three 32-bit floats: the colour, sRGB 0..1.
    pub const COLOUR: u16 = 3;
    /// Three 32-bit floats: the secondary colour.
    pub const SECONDARY: u16 = 4;
    /// 32-bit float: the size slider, 0..1.
    pub const SIZE: u16 = 5;
    /// 32-bit float: the opacity slider, 0..1.
    pub const OPACITY: u16 = 6;
    /// 32-bit float: the brush's width at full pressure, canvas pixels (before the item's
    /// transform).
    pub const WIDTH: u16 = 7;
    /// The brush as it was, whole, in the app's own words: kept as it is.
    pub const BRUSH: u16 = 8;
    /// UTF-8: the brush's name.
    pub const BRUSH_NAME: u16 = 9;
    /// u32: what the stroke's randomness was seeded with.
    pub const SEED: u16 = 10;
    /// u8: 0 a finger, 1 a pen, 2 a pen's eraser end, 3 a mouse.
    pub const POINTER: u16 = 11;
    /// 32-bit float: what the brush's size was multiplied by.
    pub const SCALE: u16 = 12;
    /// Two 32-bit floats: the view's turn and how big a canvas pixel showed.
    pub const VIEW: u16 = 13;
    /// u8: the layer's alpha was locked.
    pub const ALPHA_LOCK: u16 = 14;
    /// The app's own smoothing and pressure.
    pub const INPUT: u16 = 15;
    /// The symmetry it was drawn with, if any: u8 kind (vertical, horizontal, quadrant, radial),
    /// u8 turned only, the centre (two 32-bit floats), the turn (32-bit float), u32 parts.
    pub const SYMMETRY: u16 = 16;
    /// u32: in a layer's items to draw, which of its brushes the item has (in place of [`BRUSH`]).
    pub const BRUSH_REF: u16 = 17;
    /// u32 each: how many samples came each time, the top bit set on the last if the pen lifted
    /// after it.
    pub const BATCHES: u16 = 18;
    /// 64-bit float: when the first sample came, milliseconds on the input's clock.
    pub const STARTED: u16 = 19;
    /// 32-bit floats: the brush's exact sizes.
    pub const SIZES: u16 = 20;
    /// u32: in a layer's items to draw, the plugin's id for the item, which a selection and an edit
    /// name it by.
    pub const ID: u16 = 21;
    /// Six 32-bit floats `a b c d e f`: where the item is drawn, its samples' (x, y) taken to
    /// (a·x + c·y + e, b·x + d·y + f), canvas pixels; none for where it was drawn.
    pub const TRANSFORM: u16 = 22;
    /// 32-bit float: the smoothing it is laid with in place of its brush's own, 0..1.
    pub const SMOOTHING: u16 = 23;
    /// The selection it was drawn within, if any: its box (x, y, width, height as u32), then its
    /// coverage in runs (a u32 count, then the byte), row by row.
    pub const CLIP: u16 = 24;
    /// A region it fills, in place of samples: u32 how many outlines, then each a u32 count of its
    /// points and them, x and y as 32-bit floats (canvas pixels, before its transform), filled
    /// even-odd in its colour at its opacity. Such an item needs no brush.
    pub const OUTLINE: u16 = 25;
    /// The shape it was snapped to (QuickShape): u8 its kind (0 line, 1 polyline, 2 arc, 3 curve,
    /// 4 ellipse, 5 circle, 6 triangle, 7 equilateral triangle, 8 quadrilateral, 9 rectangle,
    /// 10 square), then its nodes, x and y as 32-bit floats (canvas pixels, before its transform).
    /// Its samples are laid exactly along the shape.
    pub const SHAPE: u16 = 26;
    /// The first id of a plugin's own fields.
    pub const OWN: u16 = 0x8000;
}

/// The bit of a batch's count saying the pen lifted after it.
pub const LIFTED: u32 = 1 << 31;

/// The fields of the record `bytes`, in order (id and bytes); none from one cut short on.
pub fn fields(bytes: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut rest = bytes;
    std::iter::from_fn(move || {
        if rest.len() < 6 {
            return None;
        }
        let id = u16::from_le_bytes([rest[0], rest[1]]);
        let len = u32::from_le_bytes([rest[2], rest[3], rest[4], rest[5]]) as usize;
        let value = rest.get(6..6usize.checked_add(len)?)?;
        rest = &rest[6 + len..];
        Some((id, value))
    })
}

/// A record being written: its fields one after another.
#[derive(Default)]
pub struct Record(Vec<u8>);

impl Record {
    /// A field `id` holding `bytes`.
    pub fn field(&mut self, id: u16, bytes: &[u8]) -> &mut Self {
        self.0.extend_from_slice(&id.to_le_bytes());
        self.0.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        self.0.extend_from_slice(bytes);
        self
    }

    pub fn bytes(self) -> Vec<u8> {
        self.0
    }
}

/// What a stroke was drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Paint,
    /// The eraser, recorded as a mask: drawn as it erased, hiding what is under it.
    Mask,
    Smudge,
}

/// One of an item's samples, as the app recorded it: where (canvas pixels, before the item's
/// transform), how hard, how the pen leant and turned, and when (milliseconds since the first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt: f32,
    pub azimuth: f32,
    pub roll: f32,
    pub time: f64,
}

impl Sample {
    /// The sample `t` of the way from this one to `to`, each of its channels between theirs.
    pub fn toward(&self, to: &Sample, t: f32) -> Sample {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Sample {
            x: mix(self.x, to.x),
            y: mix(self.y, to.y),
            pressure: mix(self.pressure, to.pressure),
            tilt: mix(self.tilt, to.tilt),
            azimuth: mix(self.azimuth, to.azimuth),
            roll: mix(self.roll, to.roll),
            time: self.time + (to.time - self.time) * f64::from(t),
        }
    }
}

/// A transform: (x, y) goes to (a·x + c·y + e, b·x + d·y + f).
pub type Affine = [f32; 6];

/// The transform that moves nothing.
pub const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Where `t` takes (`x`, `y`).
pub fn apply(t: &Affine, (x, y): (f32, f32)) -> (f32, f32) {
    (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5])
}

/// `outer` after `inner`: what doing `inner` and then `outer` does.
pub fn then(inner: &Affine, outer: &Affine) -> Affine {
    let [a, b, c, d, e, f] = *outer;
    let [p, q, r, s, u, v] = *inner;
    [a * p + c * q, b * p + d * q, a * r + c * s, b * r + d * s, a * u + c * v + e, b * u + d * v + f]
}

fn f32_at(bytes: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn f64_at(bytes: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// A stroke recorded on a layer, as the app handed it over: its record, and what a plugin most
/// often wants of it read out.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    record: Vec<u8>,
}

impl Item {
    /// The item `record` holds.
    pub fn from_record(record: Vec<u8>) -> Item {
        Item { record }
    }

    /// Its record, as the app wrote it.
    pub fn record(&self) -> &[u8] {
        &self.record
    }

    /// Its field `id`, if it has one.
    pub fn field(&self, id: u16) -> Option<&[u8]> {
        fields(&self.record).find(|(f, _)| *f == id).map(|(_, bytes)| bytes)
    }

    /// It with field `id` holding `bytes` (in place of what it held), or without it (none).
    pub fn with_field(&self, id: u16, bytes: Option<&[u8]>) -> Item {
        let mut out = Record::default();
        for (f, value) in fields(&self.record).filter(|(f, _)| *f != id) {
            out.field(f, value);
        }
        if let Some(bytes) = bytes {
            out.field(id, bytes);
        }
        Item { record: out.bytes() }
    }

    pub fn tool(&self) -> Tool {
        match self.field(field::TOOL).and_then(|b| b.first()) {
            Some(1) => Tool::Mask,
            Some(2) => Tool::Smudge,
            _ => Tool::Paint,
        }
    }

    /// Where it is drawn ([`field::TRANSFORM`]): where it was drawn if it holds none, or none that
    /// holds up.
    pub fn transform(&self) -> Affine {
        let Some(bytes) = self.field(field::TRANSFORM) else { return IDENTITY };
        let mut t = IDENTITY;
        for (i, slot) in t.iter_mut().enumerate() {
            match f32_at(bytes, i * 4).filter(|v| v.is_finite()) {
                Some(v) => *slot = v,
                None => return IDENTITY,
            }
        }
        t
    }

    /// How much its transform scales it.
    pub fn scale(&self) -> f32 {
        let t = self.transform();
        (t[0] * t[3] - t[1] * t[2]).abs().sqrt()
    }

    /// The brush's width at full pressure where it is drawn, canvas pixels.
    pub fn width(&self) -> f32 {
        let width = self.field(field::WIDTH).and_then(|b| f32_at(b, 0)).filter(|w| w.is_finite()).unwrap_or(0.0);
        width.max(0.0) * self.scale()
    }

    /// Its samples as recorded (before its transform).
    pub fn samples(&self) -> Vec<Sample> {
        let Some(bytes) = self.field(field::SAMPLES) else { return Vec::new() };
        bytes
            .chunks_exact(32)
            .map_while(|s| {
                let sample = Sample {
                    x: f32_at(s, 0)?,
                    y: f32_at(s, 4)?,
                    pressure: f32_at(s, 8)?,
                    tilt: f32_at(s, 12)?,
                    azimuth: f32_at(s, 16)?,
                    roll: f32_at(s, 20)?,
                    time: f64_at(s, 24)?,
                };
                (sample.x.is_finite() && sample.y.is_finite()).then_some(sample)
            })
            .collect()
    }

    /// Where its samples are drawn, canvas pixels, in order (a fill's: its outlines' points).
    pub fn points(&self) -> Vec<(f32, f32)> {
        let t = self.transform();
        if self.is_fill() {
            return self.outlines().into_iter().flatten().map(|p| apply(&t, p)).collect();
        }
        self.samples().iter().map(|s| apply(&t, (s.x, s.y))).collect()
    }

    /// Whether it fills a region ([`field::OUTLINE`]) rather than being a stroke.
    pub fn is_fill(&self) -> bool {
        self.field(field::OUTLINE).is_some()
    }

    /// The outlines of the region it fills, before its transform (none for a stroke).
    pub fn outlines(&self) -> Vec<Vec<(f32, f32)>> {
        let Some(bytes) = self.field(field::OUTLINE) else { return Vec::new() };
        let count = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        let mut out = Vec::new();
        let mut at = 4;
        for _ in 0..count(0).unwrap_or(0) {
            let Some(n) = count(at) else { break };
            at += 4;
            let mut outline = Vec::with_capacity(n.min(1 << 20));
            for i in 0..n {
                match (f32_at(bytes, at + i * 8), f32_at(bytes, at + i * 8 + 4)) {
                    (Some(x), Some(y)) => outline.push((x, y)),
                    _ => return out,
                }
            }
            at += n * 8;
            out.push(outline);
        }
        out
    }

    /// A fill of the region `outlines` make (even-odd, canvas pixels) in `colour` (sRGB 0..1) at
    /// `opacity`.
    pub fn fill(outlines: &[Vec<(f32, f32)>], colour: [f32; 3], opacity: f32) -> Item {
        let mut region = (outlines.len() as u32).to_le_bytes().to_vec();
        for outline in outlines {
            region.extend((outline.len() as u32).to_le_bytes());
            for &(x, y) in outline {
                region.extend(x.to_le_bytes());
                region.extend(y.to_le_bytes());
            }
        }
        let colour: Vec<u8> = colour.iter().flat_map(|c| c.to_le_bytes()).collect();
        let mut r = Record::default();
        r.field(field::TOOL, &[0]).field(field::OUTLINE, &region).field(field::COLOUR, &colour);
        r.field(field::OPACITY, &opacity.to_le_bytes()).field(field::WIDTH, &0f32.to_le_bytes());
        Item { record: r.bytes() }
    }

    /// Where its samples are drawn, and where the symmetry it was drawn with draws them again:
    /// each path, its own first (a fill's: its outlines, each closed).
    pub fn paths(&self) -> Vec<Vec<(f32, f32)>> {
        let t = self.transform();
        if self.is_fill() {
            let closed = |mut o: Vec<(f32, f32)>| {
                o.extend(o.first().copied());
                o.into_iter().map(|p| apply(&t, p)).collect()
            };
            return self.outlines().into_iter().map(closed).collect();
        }
        let own: Vec<(f32, f32)> = self.samples().iter().map(|s| (s.x, s.y)).collect();
        let mut out = vec![own.clone()];
        if let Some(symmetry) = self.field(field::SYMMETRY).and_then(Symmetry::read) {
            for k in 0..symmetry.copies() {
                out.push(own.iter().map(|&p| symmetry.copy(k, p)).collect());
            }
        }
        out.into_iter().map(|path| path.into_iter().map(|p| apply(&t, p)).collect()).collect()
    }

    /// It with `samples` in place of its own, laid in one go as the pen lifts (its time from the
    /// first of them).
    pub fn with_samples(&self, samples: &[Sample]) -> Item {
        let start = samples.first().map_or(0.0, |s| s.time);
        let mut bytes = Vec::with_capacity(samples.len() * 32);
        for s in samples {
            for v in [s.x, s.y, s.pressure, s.tilt, s.azimuth, s.roll] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            bytes.extend_from_slice(&(s.time - start).to_le_bytes());
        }
        let started = self.field(field::STARTED).and_then(|b| f64_at(b, 0)).unwrap_or(0.0) + start;
        let batches = (samples.len() as u32 | LIFTED).to_le_bytes();
        self.with_field(field::SAMPLES, Some(&bytes))
            .with_field(field::BATCHES, Some(&batches))
            .with_field(field::STARTED, Some(&started.to_le_bytes()))
    }

    /// The shape it was snapped to, if it was: its kind (see [`field::SHAPE`]) and its nodes where
    /// they are drawn (its transform applied).
    pub fn shape(&self) -> Option<(u8, Vec<(f32, f32)>)> {
        let bytes = self.field(field::SHAPE)?;
        let t = self.transform();
        let nodes = (0..(bytes.len().saturating_sub(1)) / 8)
            .filter_map(|i| Some(apply(&t, (f32_at(bytes, 1 + i * 8)?, f32_at(bytes, 5 + i * 8)?))))
            .collect();
        Some((*bytes.first()?, nodes))
    }

    /// Its colour (sRGB 0..1), black if it says none.
    pub fn colour(&self) -> [f32; 3] {
        let c = self.field(field::COLOUR);
        [0, 1, 2].map(|i| c.and_then(|c| f32_at(c, i * 4)).filter(|v| v.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0))
    }

    /// Its opacity (0..1), full if it says none.
    pub fn opacity(&self) -> f32 {
        self.field(field::OPACITY).and_then(|o| f32_at(o, 0)).filter(|v| v.is_finite()).unwrap_or(1.0).clamp(0.0, 1.0)
    }

    /// The brush it was drawn with, as the app wrote it.
    pub fn brush(&self) -> Option<&[u8]> {
        self.field(field::BRUSH)
    }

    /// Its record without its brush (to keep each brush once).
    pub fn without_brush(&self) -> Vec<u8> {
        let mut out = Record::default();
        for (id, bytes) in fields(&self.record).filter(|(id, _)| *id != field::BRUSH && *id != field::BRUSH_REF) {
            out.field(id, bytes);
        }
        out.bytes()
    }
}

/// The symmetry an item was drawn with ([`field::SYMMETRY`]): its copies made as the app makes them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Symmetry {
    kind: u8,
    rotational: bool,
    centre: (f32, f32),
    angle: f32,
    parts: u32,
}

impl Symmetry {
    fn read(bytes: &[u8]) -> Option<Symmetry> {
        let kind = *bytes.first().filter(|k| **k <= 3)?;
        let rotational = *bytes.get(1)? != 0;
        let (x, y, angle) = (f32_at(bytes, 2)?, f32_at(bytes, 6)?, f32_at(bytes, 10)?);
        let parts = u32::from_le_bytes(bytes.get(14..18)?.try_into().ok()?).clamp(2, 64);
        Some(Symmetry { kind, rotational, centre: (x, y), angle, parts })
    }

    /// How many copies it makes, the original not counted.
    fn copies(&self) -> usize {
        self.count() - 1
    }

    fn count(&self) -> usize {
        match self.kind {
            0 | 1 => 2,
            2 => 4,
            _ => self.parts as usize,
        }
    }

    /// Where copy `k` (from 0) of `p` is.
    fn copy(&self, k: usize, (x, y): (f32, f32)) -> (f32, f32) {
        let (cx, cy) = self.centre;
        let (dx, dy) = (x - cx, y - cy);
        let turn = |a: f32| (cx + dx * a.cos() - dy * a.sin(), cy + dx * a.sin() + dy * a.cos());
        let mirror = |a: f32| {
            let (s, c) = ((2.0 * a).sin(), (2.0 * a).cos());
            (cx + dx * c + dy * s, cy + dx * s - dy * c)
        };
        let axis = if self.kind == 0 { self.angle + std::f32::consts::FRAC_PI_2 } else { self.angle };
        let count = self.count();
        if !self.rotational && count <= 2 {
            return mirror(axis);
        }
        let k = k + 1;
        let a = std::f32::consts::TAU / count as f32 * k as f32;
        if self.rotational || k % 2 == 0 { turn(a) } else { mirror(axis + a / 2.0) }
    }
}

/// The part of the canvas a selection takes, as the app hands it to [`Layer::select`]: its box, and
/// how much of each pixel there it takes (0 none, 255 all).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
}

impl Area {
    /// What `bytes` (the app's selection record) holds.
    pub fn read(bytes: &[u8]) -> Area {
        let mut area = Area::default();
        for (id, value) in fields(bytes) {
            let n = || value.get(..4).map_or(0, |v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]));
            match id {
                1 => area.x = n(),
                2 => area.y = n(),
                3 => area.width = n(),
                4 => area.height = n(),
                5 => area.coverage = value.to_vec(),
                _ => {}
            }
        }
        area
    }

    /// How much it takes of the pixel at (`x`, `y`), canvas pixels.
    pub fn at(&self, (x, y): (f32, f32)) -> u8 {
        if !x.is_finite() || !y.is_finite() || x < self.x as f32 || y < self.y as f32 {
            return 0;
        }
        let (col, row) = ((x - self.x as f32) as usize, (y - self.y as f32) as usize);
        if col >= self.width as usize || row >= self.height as usize {
            return 0;
        }
        self.coverage.get(row * self.width as usize + col).copied().unwrap_or(0)
    }
}

/// A colour dropped on a layer ([`Layer::fill`]): where (canvas pixels), the colour (sRGB 0..1),
/// how far a fill may reach into what is drawn (0..1: the coverage it passes over is at most this),
/// and what bounds a fill there: how much of each pixel is drawn, in a picture each of whose pixels
/// is `scale` canvas pixels across (its box in its own pixels).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dropped {
    pub x: f32,
    pub y: f32,
    pub colour: [f32; 3],
    pub threshold: f32,
    pub scale: f32,
    pub drawn: Area,
}

impl Dropped {
    /// What `bytes` (the app's record of it) holds.
    pub fn read(bytes: &[u8]) -> Dropped {
        let mut out = Dropped { scale: 1.0, ..Dropped::default() };
        for (id, value) in fields(bytes) {
            let f = |i: usize| f32_at(value, i * 4).filter(|v| v.is_finite());
            match id {
                1 => out.x = f(0).unwrap_or(0.0),
                2 => out.y = f(0).unwrap_or(0.0),
                3 => out.colour = [f(0), f(1), f(2)].map(|c| c.unwrap_or(0.0).clamp(0.0, 1.0)),
                4 => out.threshold = f(0).unwrap_or(0.0).clamp(0.0, 1.0),
                5 => out.scale = f(0).filter(|s| *s >= 1.0).unwrap_or(1.0),
                6 => out.drawn = Area::read(value),
                _ => {}
            }
        }
        out
    }
}

/// An artwork's layers of a kind the plugin adds, handed to it for a file ([`Layer::export`]):
/// which of the kind's exports (by its place in the manifest), which kind, the canvas's size, and
/// the layers bottom up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Export {
    pub export: usize,
    pub kind: usize,
    pub width: u32,
    pub height: u32,
    pub layers: Vec<ExportedLayer>,
}

/// One layer handed for a file: its name, whether it shows, its opacity and its data (none for one
/// with nothing on it yet).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportedLayer {
    pub name: String,
    pub shown: bool,
    pub opacity: f32,
    pub data: Vec<u8>,
}

impl Export {
    /// What `bytes` (the app's record of it) holds.
    pub fn read(bytes: &[u8]) -> Export {
        let n = |v: &[u8]| v.get(..4).map_or(0, |v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]));
        let mut out = Export::default();
        for (id, value) in fields(bytes) {
            match id {
                1 => out.export = n(value) as usize,
                2 => out.kind = n(value) as usize,
                3 => out.width = n(value),
                4 => out.height = n(value),
                5 => {
                    let mut layer = ExportedLayer { opacity: 1.0, ..ExportedLayer::default() };
                    for (field, v) in fields(value) {
                        match field {
                            1 => layer.name = String::from_utf8_lossy(v).into_owned(),
                            2 => layer.shown = v.first().is_some_and(|b| *b != 0),
                            3 => layer.opacity = f32_at(v, 0).filter(|o| o.is_finite()).unwrap_or(1.0).clamp(0.0, 1.0),
                            4 => layer.data = v.to_vec(),
                            _ => {}
                        }
                    }
                    out.layers.push(layer);
                }
                _ => {}
            }
        }
        out
    }
}

/// A point the app asks about ([`Layer::hit`]): where (canvas pixels), and how far about it counts
/// as there.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    pub x: f32,
    pub y: f32,
    pub reach: f32,
}

impl Hit {
    /// What `bytes` (the app's record of it) holds.
    pub fn read(bytes: &[u8]) -> Hit {
        let mut out = Hit::default();
        for (id, value) in fields(bytes) {
            let v = f32_at(value, 0).filter(|v| v.is_finite()).unwrap_or(0.0);
            match id {
                1 => out.x = v,
                2 => out.y = v,
                3 => out.reach = v.max(0.0),
                _ => {}
            }
        }
        out
    }
}

/// A change the app asks of one item ([`Layer::edit`]): which, by its id, and whether it goes, or
/// the fields it takes in place of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    pub id: u32,
    pub delete: bool,
    pub fields: Vec<(u16, Vec<u8>)>,
}

impl Change {
    /// The changes `bytes` (the app's edit record) asks for.
    pub fn read_all(bytes: &[u8]) -> Vec<Change> {
        fields(bytes)
            .filter(|(id, _)| *id == 1)
            .map(|(_, change)| {
                let mut out = Change::default();
                for (id, value) in fields(change) {
                    match id {
                        1 => out.id = value.get(..4).map_or(0, |v| u32::from_le_bytes([v[0], v[1], v[2], v[3]])),
                        2 => out.delete = value.first().is_some_and(|b| *b != 0),
                        3 => out.fields = fields(value).map(|(f, v)| (f, v.to_vec())).collect(),
                        _ => {}
                    }
                }
                out
            })
            .collect()
    }

    /// `item` as it changes (unless it goes).
    pub fn applied(&self, item: &Item) -> Item {
        self.fields.iter().fold(item.clone(), |item, (id, value)| item.with_field(*id, Some(value)))
    }
}

/// A layer's items to draw, as the app takes them: its brushes, each once, and its items in order,
/// each naming its brush and saying its id.
#[derive(Default)]
pub struct Drawn(Record, u32);

impl Drawn {
    /// A brush (as an item's [`Item::brush`] gave it): its place, for items to name.
    pub fn brush(&mut self, bytes: &[u8]) -> u32 {
        self.0.field(1, bytes);
        self.1 += 1;
        self.1 - 1
    }

    /// Item `id`, its record without its brush (see [`Item::without_brush`]), drawn with brush
    /// `brush`.
    pub fn item(&mut self, id: u32, record: &[u8], brush: u32) {
        let mut item = Record::default();
        let own = |f: &u16| *f != field::BRUSH && *f != field::BRUSH_REF && *f != field::ID;
        for (f, bytes) in fields(record).filter(|(f, _)| own(f)) {
            item.field(f, bytes);
        }
        item.field(field::BRUSH_REF, &brush.to_le_bytes()).field(field::ID, &id.to_le_bytes());
        self.0.field(2, &item.bytes());
    }

    pub fn bytes(self) -> Vec<u8> {
        self.0.bytes()
    }
}

/// What a layer says of an item recorded on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Added {
    /// Kept: the stroke stays as the app painted it.
    Kept,
    /// Kept, and items drawn before changed: the app draws the layer again from its items.
    DrawAgain,
}

/// A kind of layer: what the plugin keeps for each, and what it does with each stroke recorded.
pub trait Layer: Sized + Send {
    /// A new, empty one of kind `kind` (its place in the manifest's `layer_kinds`).
    fn new(kind: usize) -> Self;
    /// The one `data` (what it saved) holds; none if it doesn't hold up.
    fn load(kind: usize, data: &[u8]) -> Option<Self>;
    fn save(&self) -> Vec<u8>;
    /// A stroke recorded on it.
    fn add(&mut self, item: Item) -> Added;
    /// Its items to draw, in order, each with its id.
    fn draw(&self, list: &mut Drawn);
    /// The ids of the items `area` takes (none, unless it says).
    fn select(&self, _area: &Area) -> Vec<u32> {
        Vec::new()
    }
    /// Makes `changes`: whether anything changed (nothing, unless it says).
    fn edit(&mut self, _changes: &[Change]) -> bool {
        false
    }
    /// A colour dropped on it: whether its items changed (nothing, unless it says).
    fn fill(&mut self, _dropped: &Dropped) -> bool {
        false
    }
    /// The file `export` asks for of an artwork's layers of its kinds (none, unless it says).
    fn export(_export: &Export) -> Option<Vec<u8>> {
        None
    }
    /// The id of the item at `hit`, topmost first (none, unless it says): a tap of the selection
    /// tool takes it.
    fn hit(&self, _hit: &Hit) -> Option<u32> {
        None
    }
}

/// The layer being worked on (the app hands one over for each change), and what the app reads from
/// or writes into the module.
pub struct Held<L> {
    pub layer: Mutex<Option<L>>,
    pub out: Mutex<Vec<u8>>,
    pub input: Mutex<Vec<u8>>,
}

impl<L> Held<L> {
    pub const fn new() -> Self {
        Held { layer: Mutex::new(None), out: Mutex::new(Vec::new()), input: Mutex::new(Vec::new()) }
    }
}

impl<L> Default for Held<L> {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<L: Layer> Held<L> {
    pub fn input_at(&self, len: i32) -> i32 {
        let mut input = lock(&self.input);
        *input = vec![0; len.max(0) as usize];
        input.as_mut_ptr() as usize as i32
    }

    pub fn out_at(&self) -> i32 {
        lock(&self.out).as_ptr() as usize as i32
    }

    fn put(&self, bytes: Vec<u8>) -> i32 {
        let len = bytes.len() as i32;
        *lock(&self.out) = bytes;
        len
    }

    pub fn load(&self, kind: i32, len: i32) -> i32 {
        let data = std::mem::take(&mut *lock(&self.input));
        let kind = kind.max(0) as usize;
        let layer = if len <= 0 { Some(L::new(kind)) } else { L::load(kind, &data) };
        let loaded = layer.is_some();
        *lock(&self.layer) = layer;
        if loaded { 0 } else { 1 }
    }

    pub fn add(&self, _len: i32) -> i32 {
        let record = std::mem::take(&mut *lock(&self.input));
        let mut held = lock(&self.layer);
        match held.as_mut().map(|layer| layer.add(Item::from_record(record))) {
            Some(Added::DrawAgain) => 1,
            _ => 0,
        }
    }

    pub fn save(&self) -> i32 {
        let data = lock(&self.layer).as_ref().map(Layer::save).unwrap_or_default();
        self.put(data)
    }

    pub fn draw(&self) -> i32 {
        let mut list = Drawn::default();
        if let Some(layer) = lock(&self.layer).as_ref() {
            layer.draw(&mut list);
        }
        self.put(list.bytes())
    }

    pub fn select(&self, _len: i32) -> i32 {
        let area = Area::read(&std::mem::take(&mut *lock(&self.input)));
        let ids = lock(&self.layer).as_ref().map(|layer| layer.select(&area)).unwrap_or_default();
        self.put(ids.iter().flat_map(|id| id.to_le_bytes()).collect())
    }

    pub fn edit(&self, _len: i32) -> i32 {
        let changes = Change::read_all(&std::mem::take(&mut *lock(&self.input)));
        i32::from(lock(&self.layer).as_mut().is_some_and(|layer| layer.edit(&changes)))
    }

    pub fn fill(&self, _len: i32) -> i32 {
        let dropped = Dropped::read(&std::mem::take(&mut *lock(&self.input)));
        i32::from(lock(&self.layer).as_mut().is_some_and(|layer| layer.fill(&dropped)))
    }

    pub fn hit(&self, _len: i32) -> i32 {
        let hit = Hit::read(&std::mem::take(&mut *lock(&self.input)));
        match lock(&self.layer).as_ref().and_then(|layer| layer.hit(&hit)) {
            Some(id) => self.put(id.to_le_bytes().to_vec()),
            None => 0,
        }
    }

    pub fn export(&self, _len: i32) -> i32 {
        let export = Export::read(&std::mem::take(&mut *lock(&self.input)));
        match L::export(&export) {
            Some(bytes) => self.put(bytes),
            None => -1,
        }
    }
}

/// Exports the calls a plugin's own kinds of layer are reached through, for `$layer` (a type that
/// is a [`layer::Layer`](crate::layer::Layer)).
#[macro_export]
macro_rules! layers {
    ($layer:ty) => {
        static LAYER: $crate::layer::Held<$layer> = $crate::layer::Held::new();

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_in(len: i32) -> i32 {
            LAYER.input_at(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_out() -> i32 {
            LAYER.out_at()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_load(kind: i32, len: i32) -> i32 {
            LAYER.load(kind, len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_add(len: i32) -> i32 {
            LAYER.add(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_save() -> i32 {
            LAYER.save()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_draw() -> i32 {
            LAYER.draw()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_select(len: i32) -> i32 {
            LAYER.select(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_edit(len: i32) -> i32 {
            LAYER.edit(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_fill(len: i32) -> i32 {
            LAYER.fill(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_export(len: i32) -> i32 {
            LAYER.export(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_layer_hit(len: i32) -> i32 {
            LAYER.hit(len)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_stroke(tool: u8, points: &[(f32, f32)]) -> Item {
        let mut samples = Vec::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            for v in [x, y, 1.0, 0.0, 0.0, 0.0] {
                samples.extend_from_slice(&f32::to_le_bytes(v));
            }
            samples.extend_from_slice(&(i as f64 * 8.0).to_le_bytes());
        }
        let mut r = Record::default();
        r.field(field::TOOL, &[tool]).field(field::SAMPLES, &samples).field(field::WIDTH, &6.5f32.to_le_bytes());
        r.field(field::BRUSH, b"brush").field(900, b"later");
        Item::from_record(r.bytes())
    }

    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn an_item_is_read_and_kept_as_it_came() {
        let item = a_stroke(1, &[(1.0, 2.0), (3.0, 4.0)]);
        assert_eq!(item.tool(), Tool::Mask);
        assert_eq!(item.width(), 6.5);
        assert_eq!(item.points(), [(1.0, 2.0), (3.0, 4.0)]);
        assert_eq!(item.brush(), Some(&b"brush"[..]));
        let bare = item.without_brush();
        assert!(fields(&bare).all(|(id, _)| id != field::BRUSH));
        assert!(fields(&bare).any(|(id, bytes)| id == 900 && bytes == b"later"), "a later field stays");
        // cut short, what came whole is still read
        assert_eq!(fields(&item.record()[..item.record().len() - 2]).count(), 4);
    }

    #[test]
    fn an_item_s_transform_moves_its_points_and_scales_its_width() {
        let item = a_stroke(0, &[(1.0, 2.0), (3.0, 4.0)]);
        // turned a quarter, doubled, moved
        let t = [0.0, 2.0, -2.0, 0.0, 10.0, 20.0];
        let moved = item.with_field(field::TRANSFORM, Some(&floats(&t)));
        assert_eq!(moved.points(), [(6.0, 22.0), (2.0, 26.0)]);
        assert_eq!(moved.width(), 13.0);
        assert_eq!(then(&IDENTITY, &t), t);
        assert_eq!(apply(&then(&t, &[1.0, 0.0, 0.0, 1.0, 1.0, 1.0]), (1.0, 2.0)), (7.0, 23.0));
        // a transform that doesn't hold up is none
        assert_eq!(item.with_field(field::TRANSFORM, Some(&[1, 2, 3])).transform(), IDENTITY);
        assert_eq!(item.with_field(field::TRANSFORM, None).points(), item.points());
    }

    #[test]
    fn an_item_with_samples_of_its_own_is_laid_in_one_go() {
        let item = a_stroke(0, &[(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)]);
        let samples = item.samples();
        assert_eq!(samples[1].time, 8.0);
        let half = samples[0].toward(&samples[1], 0.5);
        assert_eq!((half.x, half.y, half.time), (2.0, 3.0, 4.0));
        let piece = item.with_samples(&samples[1..]);
        assert_eq!(piece.points(), [(3.0, 4.0), (5.0, 6.0)]);
        assert_eq!(piece.samples()[0].time, 0.0, "its time from its first");
        assert_eq!(piece.field(field::STARTED), Some(&8.0f64.to_le_bytes()[..]));
        assert_eq!(piece.field(field::BATCHES), Some(&(2 | LIFTED).to_le_bytes()[..]));
        assert_eq!(piece.field(900), Some(&b"later"[..]), "the rest as it was");
    }

    #[test]
    fn a_symmetric_item_s_paths_are_its_own_and_its_copies() {
        let item = a_stroke(0, &[(10.0, 0.0)]);
        // mirrored left to right about x = 50 (vertical symmetry at no turn)
        let mut sym = vec![0u8, 0];
        sym.extend(floats(&[50.0, 50.0, 0.0]));
        sym.extend(2u32.to_le_bytes());
        let paths = item.with_field(field::SYMMETRY, Some(&sym)).paths();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], [(10.0, 0.0)]);
        assert!((paths[1][0].0 - 90.0).abs() < 1e-4 && paths[1][0].1.abs() < 1e-4, "{:?}", paths[1]);
        // radial in four, turned only: a quarter, a half and three quarters round
        let mut radial = vec![3u8, 1];
        radial.extend(floats(&[0.0, 0.0, 0.0]));
        radial.extend(4u32.to_le_bytes());
        let paths = item.with_field(field::SYMMETRY, Some(&radial)).paths();
        assert_eq!(paths.len(), 4);
        assert!((paths[2][0].0 + 10.0).abs() < 1e-4, "{:?}", paths[2]);
    }

    #[test]
    fn a_selection_s_area_and_an_edit_s_changes_are_read() {
        let mut area = Record::default();
        area.field(1, &5u32.to_le_bytes()).field(2, &6u32.to_le_bytes());
        area.field(3, &2u32.to_le_bytes()).field(4, &1u32.to_le_bytes()).field(5, &[0, 255]);
        let area = Area::read(&area.bytes());
        assert_eq!((area.at((6.5, 6.5)), area.at((5.5, 6.5)), area.at((8.0, 6.5))), (255, 0, 0));
        let mut fields_set = Record::default();
        fields_set.field(field::COLOUR, &floats(&[1.0, 0.0, 0.0]));
        let mut one = Record::default();
        one.field(1, &7u32.to_le_bytes()).field(3, &fields_set.bytes());
        let mut gone = Record::default();
        gone.field(1, &8u32.to_le_bytes()).field(2, &[1]);
        let mut edit = Record::default();
        edit.field(1, &one.bytes()).field(1, &gone.bytes());
        let changes = Change::read_all(&edit.bytes());
        assert_eq!(changes.len(), 2);
        assert_eq!((changes[0].id, changes[0].delete, changes[1].id, changes[1].delete), (7, false, 8, true));
        let recoloured = changes[0].applied(&a_stroke(0, &[(1.0, 1.0)]));
        assert_eq!(recoloured.field(field::COLOUR), Some(&floats(&[1.0, 0.0, 0.0])[..]));
    }

    #[test]
    fn a_layer_s_items_to_draw_name_their_brushes_and_ids() {
        let item = a_stroke(0, &[(1.0, 2.0)]);
        let mut list = Drawn::default();
        let b = list.brush(item.brush().unwrap());
        list.item(9, &item.without_brush(), b);
        let bytes = list.bytes();
        let read: Vec<(u16, &[u8])> = fields(&bytes).collect();
        assert_eq!(read[0], (1, &b"brush"[..]));
        assert_eq!(read[1].0, 2);
        let named = |id: u16| fields(read[1].1).find(|(f, _)| *f == id).unwrap().1.to_vec();
        assert_eq!(named(field::BRUSH_REF), 0u32.to_le_bytes());
        assert_eq!(named(field::ID), 9u32.to_le_bytes());
    }

    struct Strokes(Vec<Item>);

    impl Layer for Strokes {
        fn new(_: usize) -> Self {
            Strokes(Vec::new())
        }
        fn load(_: usize, data: &[u8]) -> Option<Self> {
            Some(Strokes(fields(data).map(|(_, b)| Item::from_record(b.to_vec())).collect()))
        }
        fn save(&self) -> Vec<u8> {
            let mut r = Record::default();
            for item in &self.0 {
                r.field(1, item.record());
            }
            r.bytes()
        }
        fn add(&mut self, item: Item) -> Added {
            self.0.push(item);
            Added::Kept
        }
        fn draw(&self, list: &mut Drawn) {
            for (i, item) in self.0.iter().enumerate() {
                let b = list.brush(item.brush().unwrap_or_default());
                list.item(i as u32, &item.without_brush(), b);
            }
        }
    }

    #[test]
    fn a_layer_held_loads_takes_items_and_saves() {
        let held: Held<Strokes> = Held::new();
        assert_eq!(held.load(0, 0), 0, "a new one");
        let item = a_stroke(0, &[(1.0, 1.0)]);
        held.input_at(item.record().len() as i32);
        *held.input.lock().unwrap() = item.record().to_vec();
        assert_eq!(held.add(0), 0);
        let len = held.save();
        let saved = held.out.lock().unwrap().clone();
        assert_eq!(saved.len(), len as usize);
        assert_eq!(held.load(0, 0), 0);
        *held.input.lock().unwrap() = saved;
        assert_eq!(held.load(0, len), 0);
        assert_eq!(held.layer.lock().unwrap().as_ref().unwrap().0, [item]);
        // one that selects, edits, fills and exports nothing says so
        assert_eq!((held.select(0), held.edit(0), held.fill(0), held.export(0), held.hit(0)), (0, 0, 0, -1, 0));
        let mut point = Record::default();
        point.field(1, &2.5f32.to_le_bytes()).field(2, &3.0f32.to_le_bytes()).field(3, &(-1.0f32).to_le_bytes());
        assert_eq!(Hit::read(&point.bytes()), Hit { x: 2.5, y: 3.0, reach: 0.0 });
        // an export asked for, as the app hands it over
        let mut one = Record::default();
        one.field(1, b"Ink").field(2, &[1]).field(3, &0.5f32.to_le_bytes()).field(4, b"data");
        let mut r = Record::default();
        r.field(1, &1u32.to_le_bytes()).field(2, &0u32.to_le_bytes()).field(3, &640u32.to_le_bytes());
        r.field(4, &480u32.to_le_bytes()).field(5, &one.bytes()).field(5, &Record::default().bytes());
        let export = Export::read(&r.bytes());
        assert_eq!((export.export, export.width, export.height, export.layers.len()), (1, 640, 480, 2));
        let ink = &export.layers[0];
        assert_eq!((ink.name.as_str(), ink.shown, ink.opacity, &ink.data[..]), ("Ink", true, 0.5, &b"data"[..]));
        assert!(!export.layers[1].shown && export.layers[1].data.is_empty());
    }

    #[test]
    fn a_fill_keeps_its_outlines_and_is_hit_by_them() {
        let square = vec![(1.0, 1.0), (5.0, 1.0), (5.0, 5.0), (1.0, 5.0)];
        let fill = Item::fill(&[square.clone()], [1.0, 0.0, 0.0], 1.0);
        assert!(fill.is_fill() && fill.samples().is_empty());
        assert_eq!(fill.outlines(), [square.clone()]);
        assert_eq!(fill.points(), square);
        let moved = fill.with_field(field::TRANSFORM, Some(&floats(&[1.0, 0.0, 0.0, 1.0, 10.0, 0.0])));
        assert_eq!(moved.paths()[0].first(), Some(&(11.0, 1.0)));
        assert_eq!(moved.paths()[0].len(), 5, "closed");
        // a colour dropped, as the app hands it over
        let mut area = Record::default();
        area.field(1, &0u32.to_le_bytes()).field(2, &0u32.to_le_bytes());
        area.field(3, &2u32.to_le_bytes()).field(4, &1u32.to_le_bytes()).field(5, &[0, 200]);
        let mut r = Record::default();
        r.field(1, &3.5f32.to_le_bytes()).field(2, &4.0f32.to_le_bytes()).field(3, &floats(&[0.0, 1.0, 2.0]));
        r.field(4, &0.25f32.to_le_bytes()).field(5, &2.0f32.to_le_bytes()).field(6, &area.bytes());
        let dropped = Dropped::read(&r.bytes());
        assert_eq!(
            (dropped.x, dropped.y, dropped.colour, dropped.threshold, dropped.scale),
            (3.5, 4.0, [0.0, 1.0, 1.0], 0.25, 2.0)
        );
        assert_eq!(dropped.drawn.at((1.5, 0.5)), 200);
    }
}
