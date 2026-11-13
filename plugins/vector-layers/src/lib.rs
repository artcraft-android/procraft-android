//! Vector layers: a kind of layer whose strokes stay strokes. The app paints each stroke on such a
//! layer with its own brush engine, as on any layer, so every brush works; this plugin keeps each
//! stroke as the app recorded it (its samples, its brush, colour, size and opacity) and hands them
//! back for the app to draw again when they change. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The eraser, as the card's Eraser says: Mask (as it starts) keeps each erasing stroke as a mask
//! that hides what is under it, so taking the mask away later brings the strokes back; Whole takes
//! away every stroke the eraser touches, whole, and has the layer drawn again.
//!
//! Its data: `PCVL`, then a record (see [`kit::layer`]): the version, each brush once (an item names
//! its brush by its place among them), and each item: its brush, the box its samples reach, and its
//! record as the app wrote it, its brush left out. Fields it doesn't know are kept as they came.

use kit::layer::{Added, Drawn, Item, Layer, Record, Tool, fields};

/// What its data starts with, and the version of what follows.
const MAGIC: &[u8; 4] = b"PCVL";
const VERSION: u32 = 1;

/// The fields of its data.
const DATA_VERSION: u16 = 1;
const DATA_BRUSH: u16 = 2;
const DATA_ITEM: u16 = 3;
/// The fields of an item in its data.
const ITEM_BRUSH: u16 = 1;
const ITEM_BOX: u16 = 2;
const ITEM_RECORD: u16 = 3;

/// The most items a layer keeps.
const MOST_ITEMS: usize = 200_000;

/// Its one control: what the eraser does (0 Mask, 1 Whole).
static CONTROLS: kit::Controls<1> = kit::Controls::new([0.0]);
kit::controls!(CONTROLS);

/// The box a stroke reaches: left, top, right, bottom, canvas pixels.
type Reach = [f32; 4];

/// A stroke kept: its brush (by place), the box its samples reach, its record without its brush.
#[derive(Clone, Debug, PartialEq)]
struct Kept {
    brush: u32,
    reach: Reach,
    record: Vec<u8>,
    tool: Tool,
}

impl Kept {
    fn item(&self) -> Item {
        Item::from_record(self.record.clone())
    }
}

/// A vector layer: its brushes, each once, and its strokes in order.
#[derive(Debug, Default, PartialEq)]
struct Vectors {
    brushes: Vec<Vec<u8>>,
    items: Vec<Kept>,
    /// Fields of its data a later version wrote, kept as they came.
    later: Vec<(u16, Vec<u8>)>,
}

/// The box `points` reach, none if there are none.
fn reach_of(points: &[(f32, f32)]) -> Option<Reach> {
    let &(x, y) = points.first()?;
    Some(points.iter().fold([x, y, x, y], |[l, t, r, b], &(x, y)| [l.min(x), t.min(y), r.max(x), b.max(y)]))
}

/// The distance from `p` to the segment from `a` to `b`.
fn to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// Which side of the line through `a` and `b` `p` is on (0 on it).
fn side(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// The distance between the segments `a0`–`a1` and `b0`–`b1`: 0 where they cross.
fn between(a0: (f32, f32), a1: (f32, f32), b0: (f32, f32), b1: (f32, f32)) -> f32 {
    let crosses = side(a0, a1, b0) * side(a0, a1, b1) < 0.0 && side(b0, b1, a0) * side(b0, b1, a1) < 0.0;
    if crosses {
        return 0.0;
    }
    to_segment(a0, b0, b1).min(to_segment(a1, b0, b1)).min(to_segment(b0, a0, a1)).min(to_segment(b1, a0, a1))
}

/// The segments of a path: each point to the next, or a lone point to itself.
fn segments(points: &[(f32, f32)]) -> Vec<Segment> {
    match points {
        [] => Vec::new(),
        [p] => vec![(*p, *p)],
        _ => points.windows(2).map(|w| (w[0], w[1])).collect(),
    }
}

/// A segment: from one point to another.
type Segment = ((f32, f32), (f32, f32));

/// Whether the path `a` comes within `reach` of the path `b`.
fn touches(a: &[(f32, f32)], b: &[(f32, f32)], reach: f32) -> bool {
    // segments whose boxes are further apart than that are passed over at once
    let near = |p: &Segment, q: &Segment| {
        let box_of = |(s, e): &Segment| [s.0.min(e.0), s.1.min(e.1), s.0.max(e.0), s.1.max(e.1)];
        let ([l0, t0, r0, b0], [l1, t1, r1, b1]) = (box_of(p), box_of(q));
        l0 - reach <= r1 && l1 - reach <= r0 && t0 - reach <= b1 && t1 - reach <= b0
    };
    let (sa, sb) = (segments(a), segments(b));
    sa.iter().any(|p| sb.iter().any(|q| near(p, q) && between(p.0, p.1, q.0, q.1) <= reach))
}

impl Vectors {
    /// Brush `bytes`' place among its brushes, kept there if it is new.
    fn brush(&mut self, bytes: &[u8]) -> u32 {
        match self.brushes.iter().position(|b| b == bytes) {
            Some(at) => at as u32,
            None => {
                self.brushes.push(bytes.to_vec());
                (self.brushes.len() - 1) as u32
            }
        }
    }

    /// Takes away every paint or smudge stroke the eraser `item` touches, whole: true if any went.
    fn erase_whole(&mut self, item: &Item) -> bool {
        let path = item.points();
        let Some(eraser) = reach_of(&path) else { return false };
        let width = item.width();
        let before = self.items.len();
        self.items.retain(|kept| {
            if kept.tool == Tool::Mask {
                return true;
            }
            let stroke = kept.item();
            let reach = (width + stroke.width()) / 2.0;
            let [l, t, r, b] = kept.reach;
            let apart =
                l - reach > eraser[2] || eraser[0] - reach > r || t - reach > eraser[3] || eraser[1] - reach > b;
            apart || !touches(&path, &stroke.points(), reach)
        });
        self.items.len() != before
    }

    /// Its brushes still named by an item, each item naming its place among them.
    fn compacted(&self) -> (Vec<&[u8]>, Vec<u32>) {
        let mut kept: Vec<u32> = Vec::new();
        let mut places = Vec::with_capacity(self.items.len());
        for item in &self.items {
            let at = match kept.iter().position(|b| *b == item.brush) {
                Some(at) => at,
                None => {
                    kept.push(item.brush);
                    kept.len() - 1
                }
            };
            places.push(at as u32);
        }
        let brushes = kept.iter().filter_map(|b| self.brushes.get(*b as usize).map(Vec::as_slice)).collect();
        (brushes, places)
    }
}

impl Layer for Vectors {
    fn new(_kind: usize) -> Self {
        Vectors::default()
    }

    fn load(_kind: usize, data: &[u8]) -> Option<Self> {
        let rest = data.strip_prefix(MAGIC)?;
        let mut layer = Vectors::default();
        for (id, bytes) in fields(rest) {
            match id {
                DATA_VERSION => {
                    // a version this one can't read
                    if bytes.get(..4).map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))? > VERSION {
                        return None;
                    }
                }
                DATA_BRUSH => layer.brushes.push(bytes.to_vec()),
                DATA_ITEM if layer.items.len() < MOST_ITEMS => {
                    let mut kept = Kept { brush: 0, reach: [0.0; 4], record: Vec::new(), tool: Tool::Paint };
                    for (field, value) in fields(bytes) {
                        match field {
                            ITEM_BRUSH if value.len() >= 4 => {
                                kept.brush = u32::from_le_bytes([value[0], value[1], value[2], value[3]]);
                            }
                            ITEM_BOX if value.len() >= 16 => {
                                for (i, slot) in kept.reach.iter_mut().enumerate() {
                                    let at = i * 4;
                                    *slot =
                                        f32::from_le_bytes([value[at], value[at + 1], value[at + 2], value[at + 3]]);
                                }
                            }
                            ITEM_RECORD => kept.record = value.to_vec(),
                            _ => {}
                        }
                    }
                    kept.tool = kept.item().tool();
                    layer.items.push(kept);
                }
                DATA_ITEM => {}
                _ => layer.later.push((id, bytes.to_vec())),
            }
        }
        Some(layer)
    }

    fn save(&self) -> Vec<u8> {
        let (brushes, places) = self.compacted();
        let mut r = Record::default();
        r.field(DATA_VERSION, &VERSION.to_le_bytes());
        for brush in brushes {
            r.field(DATA_BRUSH, brush);
        }
        for (item, place) in self.items.iter().zip(places) {
            let mut kept = Record::default();
            let reach: Vec<u8> = item.reach.iter().flat_map(|v| v.to_le_bytes()).collect();
            kept.field(ITEM_BRUSH, &place.to_le_bytes()).field(ITEM_BOX, &reach).field(ITEM_RECORD, &item.record);
            r.field(DATA_ITEM, &kept.bytes());
        }
        for (id, bytes) in &self.later {
            r.field(*id, bytes);
        }
        [MAGIC.as_slice(), &r.bytes()].concat()
    }

    fn add(&mut self, item: Item) -> Added {
        let tool = item.tool();
        if tool == Tool::Mask && CONTROLS.get(0) >= 0.5 {
            // Whole: what it touched goes, and the layer is drawn again without it (or as it was)
            self.erase_whole(&item);
            return Added::DrawAgain;
        }
        if self.items.len() >= MOST_ITEMS {
            return Added::DrawAgain;
        }
        let Some(reach) = reach_of(&item.points()) else { return Added::DrawAgain };
        let brush = self.brush(item.brush().unwrap_or_default());
        self.items.push(Kept { brush, reach, record: item.without_brush(), tool });
        Added::Kept
    }

    fn draw(&self, list: &mut Drawn) {
        let (brushes, places) = self.compacted();
        for brush in brushes {
            list.brush(brush);
        }
        for (item, place) in self.items.iter().zip(places) {
            list.item(&item.record, place);
        }
    }
}

kit::layers!(Vectors);

#[cfg(test)]
mod tests {
    use super::*;
    use kit::layer::field;

    fn stroke(tool: u8, width: f32, brush: &[u8], points: &[(f32, f32)]) -> Item {
        let mut samples = Vec::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            for v in [x, y, 1.0, 0.0, 0.0, 0.0] {
                samples.extend_from_slice(&f32::to_le_bytes(v));
            }
            samples.extend_from_slice(&(i as f64 * 8.0).to_le_bytes());
        }
        let mut r = Record::default();
        r.field(field::TOOL, &[tool]).field(field::SAMPLES, &samples).field(field::WIDTH, &width.to_le_bytes());
        r.field(field::BRUSH, brush);
        Item::from_record(r.bytes())
    }

    fn line(y: f32) -> Vec<(f32, f32)> {
        (0..=10).map(|i| (10.0 + i as f32 * 10.0, y)).collect()
    }

    #[test]
    fn strokes_are_kept_and_saved_with_each_brush_once() {
        let mut layer = Vectors::new(0);
        assert_eq!(layer.add(stroke(0, 10.0, b"pen", &line(20.0))), Added::Kept);
        assert_eq!(layer.add(stroke(0, 10.0, b"pen", &line(60.0))), Added::Kept);
        assert_eq!(layer.add(stroke(2, 20.0, b"blender", &[(50.0, 0.0), (50.0, 80.0)])), Added::Kept, "a smudge");
        assert_eq!(layer.brushes.len(), 2);
        let data = layer.save();
        let back = Vectors::load(0, &data).unwrap();
        assert_eq!(back, layer);
        assert!(Vectors::load(0, b"nonsense").is_none());
        // what it hands back to draw: in order, each naming its brush
        let mut list = Drawn::default();
        back.draw(&mut list);
        let list = list.bytes();
        let read: Vec<(u16, &[u8])> = fields(&list).collect();
        assert_eq!(read.iter().filter(|(id, _)| *id == 1).count(), 2, "two brushes");
        assert_eq!(read.iter().filter(|(id, _)| *id == 2).count(), 3, "three items");
    }

    /// (The one test that sets the control: tests run side by side.)
    #[test]
    fn the_eraser_hides_as_a_mask_or_takes_away_each_stroke_it_touches_whole() {
        let mut layer = Vectors::new(0);
        layer.add(stroke(0, 10.0, b"pen", &line(20.0)));
        layer.add(stroke(0, 10.0, b"pen", &line(60.0)));
        layer.add(stroke(0, 4.0, b"marker", &[(200.0, 200.0)]));
        // Mask: kept as a stroke of its own, nothing taken away
        CONTROLS.told(0, 0.0);
        assert_eq!(layer.add(stroke(1, 20.0, b"eraser", &[(50.0, 0.0), (50.0, 80.0)])), Added::Kept);
        assert_eq!((layer.items.len(), layer.items[3].tool), (4, Tool::Mask));
        layer.items.pop();
        // Whole: down across the first stroke only, its width reaching it from beside
        assert_eq!(layer.add(stroke(1, 10.0, b"eraser", &[(50.0, 0.0), (50.0, 12.0)])), Added::DrawAgain);
        assert_eq!(layer.items.len(), 2);
        assert_eq!(layer.items[0].item().points(), line(60.0), "the second is left");
        // a brush no stroke has any more isn't saved
        layer.items.retain(|k| k.brush != 1);
        assert_eq!(Vectors::load(0, &layer.save()).unwrap().brushes, [b"pen".to_vec()]);
        // touching nothing, nothing goes: still drawn again, as it was
        assert_eq!(layer.add(stroke(1, 2.0, b"eraser", &[(300.0, 300.0)])), Added::DrawAgain);
        assert_eq!(layer.items.len(), 1);
        CONTROLS.told(0, 0.0);
    }

    #[test]
    fn a_later_version_s_fields_are_kept_and_a_later_version_is_refused() {
        let mut layer = Vectors::new(0);
        layer.add(stroke(0, 10.0, b"pen", &line(20.0)));
        let mut data = layer.save();
        let mut more = Record::default();
        more.field(77, b"later");
        data.extend(more.bytes());
        let back = Vectors::load(0, &data).unwrap();
        assert_eq!(back.later, [(77, b"later".to_vec())]);
        assert!(back.save().ends_with(b"later"));
        let mut newer = Record::default();
        newer.field(DATA_VERSION, &2u32.to_le_bytes());
        assert!(Vectors::load(0, &[MAGIC.as_slice(), &newer.bytes()].concat()).is_none());
    }
}
