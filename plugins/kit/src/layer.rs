//! Kinds of layer of the plugin's own (its manifest's `layer_kinds`, interface 3): a vector layer,
//! say. Such a layer sits in the stack as any other. The app paints each stroke on it with its own
//! brush engine, as on any layer, then records the stroke as an [`Item`] and hands it to the plugin,
//! which keeps the layer's items in data of its own: the app holds that data, saves it beside the
//! layer's pixels, and undoes and redoes it with them. When the plugin changes items already drawn
//! (taking some away, say), it asks the app to draw the layer again from its items. See
//! `docs/plugins.md`. [`layers!`](crate::layers!) exports the calls.
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
    /// 32-bit float: the brush's width at full pressure, canvas pixels.
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
    /// The symmetry it was drawn with, if any.
    pub const SYMMETRY: u16 = 16;
    /// u32: in a layer's items to draw, which of its brushes the item has (in place of [`BRUSH`]).
    pub const BRUSH_REF: u16 = 17;
    /// The first id of a plugin's own fields.
    pub const OWN: u16 = 0x8000;
}

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

/// A stroke recorded on a layer, as the app handed it over: its record, and what a plugin most
/// often wants of it read out.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    record: Vec<u8>,
}

fn f32_at(bytes: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
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

    fn field(&self, id: u16) -> Option<&[u8]> {
        fields(&self.record).find(|(f, _)| *f == id).map(|(_, bytes)| bytes)
    }

    pub fn tool(&self) -> Tool {
        match self.field(field::TOOL).and_then(|b| b.first()) {
            Some(1) => Tool::Mask,
            Some(2) => Tool::Smudge,
            _ => Tool::Paint,
        }
    }

    /// The brush's width at full pressure, canvas pixels.
    pub fn width(&self) -> f32 {
        self.field(field::WIDTH).and_then(|b| f32_at(b, 0)).filter(|w| w.is_finite()).unwrap_or(0.0).max(0.0)
    }

    /// Where its samples are, canvas pixels, in order.
    pub fn points(&self) -> Vec<(f32, f32)> {
        let Some(samples) = self.field(field::SAMPLES) else { return Vec::new() };
        samples
            .chunks_exact(32)
            .filter_map(|s| Some((f32_at(s, 0)?, f32_at(s, 4)?)))
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .collect()
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

/// A layer's items to draw, as the app takes them: its brushes, each once, and its items in order,
/// each naming its brush.
#[derive(Default)]
pub struct Drawn(Record, u32);

impl Drawn {
    /// A brush (as an item's [`Item::brush`] gave it): its place, for items to name.
    pub fn brush(&mut self, bytes: &[u8]) -> u32 {
        self.0.field(1, bytes);
        self.1 += 1;
        self.1 - 1
    }

    /// An item, its record without its brush (see [`Item::without_brush`]), drawn with brush
    /// `brush`.
    pub fn item(&mut self, record: &[u8], brush: u32) {
        let mut item = Record::default();
        for (id, bytes) in fields(record).filter(|(id, _)| *id != field::BRUSH && *id != field::BRUSH_REF) {
            item.field(id, bytes);
        }
        item.field(field::BRUSH_REF, &brush.to_le_bytes());
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
    /// Its items to draw, in order.
    fn draw(&self, list: &mut Drawn);
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
    fn a_layer_s_items_to_draw_name_their_brushes() {
        let item = a_stroke(0, &[(1.0, 2.0)]);
        let mut list = Drawn::default();
        let b = list.brush(item.brush().unwrap());
        list.item(&item.without_brush(), b);
        let bytes = list.bytes();
        let read: Vec<(u16, &[u8])> = fields(&bytes).collect();
        assert_eq!(read[0], (1, &b"brush"[..]));
        assert_eq!(read[1].0, 2);
        let named = fields(read[1].1).find(|(id, _)| *id == field::BRUSH_REF).unwrap().1;
        assert_eq!(named, 0u32.to_le_bytes());
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
            for item in &self.0 {
                let b = list.brush(item.brush().unwrap_or_default());
                list.item(&item.without_brush(), b);
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
    }
}
