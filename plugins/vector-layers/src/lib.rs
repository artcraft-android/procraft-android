//! Vector layers: a kind of layer whose strokes stay strokes. The app paints each stroke on such a
//! layer with its own brush engine, as on any layer, so every brush works; this plugin keeps each
//! stroke as the app recorded it (its samples, its brush, colour, size and opacity) and hands them
//! back for the app to draw again when they change. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The eraser, as the card's Eraser says:
//! - Mask (as it starts) keeps each erasing stroke as a mask that hides what is under it, so taking
//!   the mask away later brings the strokes back.
//! - Whole takes away every stroke the eraser touches, whole.
//! - Cut takes away the parts of the strokes the eraser passes over: what is left of each becomes
//!   strokes of its own, its samples kept as they were.
//!
//! A selection takes the strokes wholly inside it, or with Touching strokes on, those it touches
//! too; masks with them while Masks is on. The app moves, restyles or deletes what it takes.
//!
//! Its data: `PCVL`, then a record (see [`kit::layer`]): the version, each brush once (an item names
//! its brush by its place among them), the next id to give, and each item: its brush, the box it
//! reaches, its id, and its record as the app wrote it, its brush left out. Fields it doesn't know are
//! kept as they came.

use kit::layer::{Added, Area, Change, Drawn, Item, Layer, Record, Sample, Tool, fields};

/// What its data starts with, and the version of what follows (1 had no ids).
const MAGIC: &[u8; 4] = b"PCVL";
const VERSION: u32 = 2;

/// The fields of its data.
const DATA_VERSION: u16 = 1;
const DATA_BRUSH: u16 = 2;
const DATA_ITEM: u16 = 3;
const DATA_NEXT_ID: u16 = 4;
/// The fields of an item in its data.
const ITEM_BRUSH: u16 = 1;
const ITEM_BOX: u16 = 2;
const ITEM_RECORD: u16 = 3;
const ITEM_ID: u16 = 4;

/// The most items a layer keeps.
const MOST_ITEMS: usize = 200_000;
/// How far a cut's ends are found along a stroke: halved this many times.
const CUT_STEPS: usize = 12;

/// Its controls: what the eraser does (0 Mask, 1 Whole, 2 Cut), whether a selection takes the
/// strokes it touches as well as those inside it, and whether it takes masks.
static CONTROLS: kit::Controls<3> = kit::Controls::new([0.0, 0.0, 1.0]);
kit::controls!(CONTROLS);

/// What the eraser does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Eraser {
    Mask,
    Whole,
    Cut,
}

fn eraser() -> Eraser {
    match CONTROLS.get(0).round() as i32 {
        1 => Eraser::Whole,
        2 => Eraser::Cut,
        _ => Eraser::Mask,
    }
}

/// The box a stroke reaches: left, top, right, bottom, canvas pixels.
type Reach = [f32; 4];

/// A stroke kept: its id, its brush (by place), the box it reaches, its record without its brush.
#[derive(Clone, Debug, PartialEq)]
struct Kept {
    id: u32,
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

/// The box `points` reach, `around` further each way; none if there are none.
fn reach_of(points: impl IntoIterator<Item = (f32, f32)>, around: f32) -> Option<Reach> {
    let mut points = points.into_iter();
    let (x, y) = points.next()?;
    let [l, t, r, b] = points.fold([x, y, x, y], |[l, t, r, b], (x, y)| [l.min(x), t.min(y), r.max(x), b.max(y)]);
    Some([l - around, t - around, r + around, b + around])
}

/// Whether two boxes overlap.
fn overlap(a: &Reach, b: &Reach) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
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

/// A segment: from one point to another.
type Segment = ((f32, f32), (f32, f32));

/// The segments of a path: each point to the next, or a lone point to itself.
fn segments(points: &[(f32, f32)]) -> Vec<Segment> {
    match points {
        [] => Vec::new(),
        [p] => vec![(*p, *p)],
        _ => points.windows(2).map(|w| (w[0], w[1])).collect(),
    }
}

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

/// How far `p` is from the path `path`.
fn distance(p: (f32, f32), path: &[(f32, f32)]) -> f32 {
    segments(path).iter().map(|(a, b)| to_segment(p, *a, *b)).fold(f32::INFINITY, f32::min)
}

/// What is left of `stroke` once the eraser's `path` (drawn `width` wide) has passed over it: none if
/// it doesn't touch it, else each piece left (perhaps none), its samples as they were, cut where the
/// eraser's edge crosses it. A sample goes where the eraser reaches it or any copy of it its symmetry
/// makes, so its copies go with it.
fn cut(stroke: &Item, path: &[(f32, f32)], width: f32) -> Option<Vec<Item>> {
    let samples = stroke.samples();
    let paths = stroke.paths();
    let reach = (width + stroke.width()) / 2.0;
    // how far inside the eraser's reach sample `i` (or a point between samples) is: above 0 inside
    let inside = |at: &dyn Fn(&[(f32, f32)]) -> (f32, f32)| {
        paths.iter().map(|copy| reach - distance(at(copy), path)).fold(f32::NEG_INFINITY, f32::max)
    };
    let gone: Vec<bool> = (0..samples.len()).map(|i| inside(&|copy| copy[i]) > 0.0).collect();
    if !gone.contains(&true) {
        return None;
    }
    // where between samples `i` and `i + 1` the eraser's edge is (0 at `i`, 1 at `i + 1`)
    let edge = |i: usize| {
        let at = |t: f32| inside(&|copy: &[(f32, f32)]| lerp(copy[i], copy[i + 1], t));
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        let lo_gone = gone[i];
        for _ in 0..CUT_STEPS {
            let mid = (lo + hi) / 2.0;
            if (at(mid) > 0.0) == lo_gone {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    };
    let mut pieces = Vec::new();
    let mut piece: Vec<Sample> = Vec::new();
    for i in 0..samples.len() {
        if !gone[i] {
            if piece.is_empty() && i > 0 && gone[i - 1] {
                piece.push(samples[i - 1].toward(&samples[i], edge(i - 1)));
            }
            piece.push(samples[i]);
        }
        let ends = i + 1 == samples.len() || gone[i + 1];
        if !gone[i] && ends {
            if i + 1 < samples.len() {
                piece.push(samples[i].toward(&samples[i + 1], edge(i)));
            }
            if piece.len() >= 2 {
                pieces.push(stroke.with_samples(&piece));
            }
            piece.clear();
        }
    }
    Some(pieces)
}

fn lerp(a: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

/// A vector layer: its brushes, each once, its strokes in order, and the next id to give.
#[derive(Debug, Default, PartialEq)]
struct Vectors {
    brushes: Vec<Vec<u8>>,
    items: Vec<Kept>,
    next_id: u32,
    /// Fields of its data a later version wrote, kept as they came.
    later: Vec<(u16, Vec<u8>)>,
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

    /// `item` kept as a new stroke with brush `brush` (none: its own): none if it has no samples.
    fn kept(&mut self, item: &Item, brush: Option<u32>) -> Option<Kept> {
        let reach = reach_of(item.paths().into_iter().flatten(), item.width() / 2.0)?;
        let brush = brush.unwrap_or_else(|| self.brush(item.brush().unwrap_or_default()));
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        Some(Kept { id, brush, reach, record: item.without_brush(), tool: item.tool() })
    }

    /// The paint and smudge strokes the eraser `item` touches, by place: those whose box comes
    /// within its reach first.
    fn touched(&self, item: &Item) -> Vec<usize> {
        let path = item.points();
        let width = item.width();
        let Some(eraser) = reach_of(path.iter().copied(), width / 2.0) else { return Vec::new() };
        self.items
            .iter()
            .enumerate()
            .filter(|(_, kept)| kept.tool != Tool::Mask && overlap(&kept.reach, &eraser))
            .filter(|(_, kept)| {
                let stroke = kept.item();
                let reach = (width + stroke.width()) / 2.0;
                stroke.paths().iter().any(|copy| touches(&path, copy, reach))
            })
            .map(|(at, _)| at)
            .collect()
    }

    /// Takes away the parts of the paint and smudge strokes the eraser `item` passes over, each
    /// one's pieces left in its place: true if any changed.
    fn erase_cut(&mut self, item: &Item) -> bool {
        let (path, width) = (item.points(), item.width());
        let mut changed = false;
        for at in self.touched(item).into_iter().rev() {
            let Some(kept) = self.items.get(at).cloned() else { continue };
            let Some(pieces) = cut(&kept.item(), &path, width) else { continue };
            let pieces: Vec<Kept> = pieces.iter().filter_map(|piece| self.kept(piece, Some(kept.brush))).collect();
            self.items.splice(at..=at, pieces);
            changed = true;
        }
        changed
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

fn u32_of(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

impl Layer for Vectors {
    fn new(_kind: usize) -> Self {
        Vectors::default()
    }

    fn load(_kind: usize, data: &[u8]) -> Option<Self> {
        let rest = data.strip_prefix(MAGIC)?;
        let mut layer = Vectors::default();
        let mut next_id = None;
        for (id, bytes) in fields(rest) {
            match id {
                // a version this one can't read
                DATA_VERSION if u32_of(bytes)? > VERSION => return None,
                DATA_VERSION => {}
                DATA_BRUSH => layer.brushes.push(bytes.to_vec()),
                DATA_NEXT_ID => next_id = u32_of(bytes),
                DATA_ITEM if layer.items.len() < MOST_ITEMS => {
                    let mut kept =
                        Kept { id: u32::MAX, brush: 0, reach: [0.0; 4], record: Vec::new(), tool: Tool::Paint };
                    for (field, value) in fields(bytes) {
                        match field {
                            ITEM_BRUSH => kept.brush = u32_of(value).unwrap_or(0),
                            ITEM_ID => kept.id = u32_of(value).unwrap_or(u32::MAX),
                            ITEM_BOX if value.len() >= 16 => {
                                for (i, slot) in kept.reach.iter_mut().enumerate() {
                                    *slot = f32::from_le_bytes(value[i * 4..i * 4 + 4].try_into().ok()?);
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
        // ids for the strokes of data from before them, and none given twice
        let most = layer.items.iter().map(|k| k.id).filter(|id| *id != u32::MAX).max().map_or(0, |m| m + 1);
        layer.next_id = next_id.unwrap_or(0).max(most);
        let mut seen = std::collections::HashSet::new();
        for i in 0..layer.items.len() {
            if layer.items[i].id == u32::MAX || !seen.insert(layer.items[i].id) {
                layer.items[i].id = layer.next_id;
                seen.insert(layer.next_id);
                layer.next_id = layer.next_id.wrapping_add(1);
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
        r.field(DATA_NEXT_ID, &self.next_id.to_le_bytes());
        for (item, place) in self.items.iter().zip(places) {
            let mut kept = Record::default();
            let reach: Vec<u8> = item.reach.iter().flat_map(|v| v.to_le_bytes()).collect();
            kept.field(ITEM_BRUSH, &place.to_le_bytes()).field(ITEM_BOX, &reach);
            kept.field(ITEM_ID, &item.id.to_le_bytes()).field(ITEM_RECORD, &item.record);
            r.field(DATA_ITEM, &kept.bytes());
        }
        for (id, bytes) in &self.later {
            r.field(*id, bytes);
        }
        [MAGIC.as_slice(), &r.bytes()].concat()
    }

    fn add(&mut self, item: Item) -> Added {
        if item.tool() == Tool::Mask {
            match eraser() {
                Eraser::Mask => {}
                // what it touched goes, whole or the parts it passed over, and the layer is drawn
                // again without them (or as it was)
                Eraser::Whole => {
                    for at in self.touched(&item).into_iter().rev() {
                        self.items.remove(at);
                    }
                    return Added::DrawAgain;
                }
                Eraser::Cut => {
                    self.erase_cut(&item);
                    return Added::DrawAgain;
                }
            }
        }
        if self.items.len() >= MOST_ITEMS {
            return Added::DrawAgain;
        }
        match self.kept(&item, None) {
            Some(kept) => {
                self.items.push(kept);
                Added::Kept
            }
            None => Added::DrawAgain,
        }
    }

    fn draw(&self, list: &mut Drawn) {
        let (brushes, places) = self.compacted();
        for brush in brushes {
            list.brush(brush);
        }
        for (item, place) in self.items.iter().zip(places) {
            list.item(item.id, &item.record, place);
        }
    }

    fn select(&self, area: &Area) -> Vec<u32> {
        let (touching, masks) = (CONTROLS.on(1), CONTROLS.on(2));
        let taken = |p: &(f32, f32)| area.at(*p) >= 128;
        self.items
            .iter()
            .filter(|kept| masks || kept.tool != Tool::Mask)
            .filter(|kept| {
                let points = kept.item().points();
                !points.is_empty() && if touching { points.iter().any(taken) } else { points.iter().all(taken) }
            })
            .map(|kept| kept.id)
            .collect()
    }

    fn edit(&mut self, changes: &[Change]) -> bool {
        let mut changed = false;
        for change in changes {
            let Some(at) = self.items.iter().position(|k| k.id == change.id) else { continue };
            if change.delete {
                self.items.remove(at);
                changed = true;
                continue;
            }
            let kept = &self.items[at];
            // a brush given is kept once among the others
            let mut item = change.applied(&kept.item());
            let brush = match item.brush().map(<[u8]>::to_vec) {
                Some(bytes) => {
                    item = Item::from_record(item.without_brush());
                    self.brush(&bytes)
                }
                None => kept.brush,
            };
            let Some(reach) = reach_of(item.paths().into_iter().flatten(), item.width() / 2.0) else { continue };
            let kept = &mut self.items[at];
            kept.record = item.record().to_vec();
            (kept.brush, kept.reach, kept.tool) = (brush, reach, item.tool());
            changed = true;
        }
        changed
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

    fn area(x: u32, y: u32, width: u32, height: u32) -> Area {
        Area { x, y, width, height, coverage: vec![255; (width * height) as usize] }
    }

    #[test]
    fn strokes_are_kept_and_saved_with_each_brush_once_and_ids_of_their_own() {
        let mut layer = Vectors::new(0);
        assert_eq!(layer.add(stroke(0, 10.0, b"pen", &line(20.0))), Added::Kept);
        assert_eq!(layer.add(stroke(0, 10.0, b"pen", &line(60.0))), Added::Kept);
        assert_eq!(layer.add(stroke(2, 20.0, b"blender", &[(50.0, 0.0), (50.0, 80.0)])), Added::Kept, "a smudge");
        assert_eq!(layer.brushes.len(), 2);
        assert_eq!(layer.items.iter().map(|k| k.id).collect::<Vec<_>>(), [0, 1, 2]);
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

    /// (The one test that sets the controls: tests run side by side.)
    #[test]
    fn the_eraser_hides_as_a_mask_takes_strokes_whole_or_cuts_them_and_selections_take_strokes() {
        let mut layer = Vectors::new(0);
        layer.add(stroke(0, 10.0, b"pen", &line(20.0)));
        layer.add(stroke(0, 10.0, b"pen", &line(60.0)));
        layer.add(stroke(0, 4.0, b"marker", &[(200.0, 200.0)]));
        // Mask: kept as a stroke of its own, nothing taken away
        CONTROLS.told(0, 0.0);
        assert_eq!(layer.add(stroke(1, 20.0, b"eraser", &[(50.0, 0.0), (50.0, 80.0)])), Added::Kept);
        assert_eq!((layer.items.len(), layer.items[3].tool), (4, Tool::Mask));
        // a selection takes what is wholly inside it, masks too while Masks is on
        CONTROLS.told(1, 0.0);
        CONTROLS.told(2, 1.0);
        assert_eq!(layer.select(&area(0, 0, 130, 30)), [0]);
        assert_eq!(layer.select(&area(0, 0, 130, 100)), [0, 1, 3]);
        CONTROLS.told(2, 0.0);
        assert_eq!(layer.select(&area(0, 0, 130, 100)), [0, 1]);
        // touching strokes too, with Touching strokes on
        CONTROLS.told(1, 1.0);
        assert_eq!(layer.select(&area(0, 0, 40, 30)), [0]);
        CONTROLS.told(1, 0.0);
        assert!(layer.select(&area(0, 0, 40, 30)).is_empty());
        CONTROLS.told(2, 1.0);
        layer.items.pop();
        // Whole: down across the first stroke only, its width reaching it from beside
        CONTROLS.told(0, 1.0);
        assert_eq!(layer.add(stroke(1, 10.0, b"eraser", &[(50.0, 0.0), (50.0, 12.0)])), Added::DrawAgain);
        assert_eq!(layer.items.len(), 2);
        assert_eq!(layer.items[0].item().points(), line(60.0), "the second is left");
        // a brush no stroke has any more isn't saved
        assert_eq!(Vectors::load(0, &layer.save()).unwrap().brushes.len(), 2);
        // touching nothing, nothing goes: still drawn again, as it was
        assert_eq!(layer.add(stroke(1, 2.0, b"eraser", &[(300.0, 300.0)])), Added::DrawAgain);
        assert_eq!(layer.items.len(), 2);
        // Cut: across the middle of the stroke left, two pieces in its place, ending where the
        // eraser's edge (and the stroke's own width) begins
        CONTROLS.told(0, 2.0);
        assert_eq!(layer.add(stroke(1, 10.0, b"eraser", &[(55.0, 40.0), (55.0, 80.0)])), Added::DrawAgain);
        assert_eq!(layer.items.len(), 3);
        let left = layer.items[0].item().points();
        let right = layer.items[1].item().points();
        assert_eq!((left[0], right.last().copied()), ((10.0, 60.0), Some((110.0, 60.0))));
        assert!((left.last().unwrap().0 - 45.0).abs() < 0.1, "{left:?}");
        assert!((right[0].0 - 65.0).abs() < 0.1, "{right:?}");
        assert_eq!(layer.items[1].item().samples()[0].time, 0.0, "each piece's time from its own start");
        assert_ne!(layer.items[0].id, layer.items[1].id);
        // an end cut off leaves the rest; a stroke wholly under it goes
        assert_eq!(layer.add(stroke(1, 10.0, b"eraser", &[(10.0, 50.0), (10.0, 70.0)])), Added::DrawAgain);
        assert!((layer.items[0].item().points()[0].0 - 20.0).abs() < 0.1);
        assert_eq!(layer.add(stroke(1, 30.0, b"eraser", &[(200.0, 190.0), (200.0, 210.0)])), Added::DrawAgain);
        assert_eq!(layer.items.len(), 2);
        CONTROLS.told(0, 0.0);
    }

    #[test]
    fn an_edit_changes_or_deletes_strokes_by_their_ids() {
        let mut layer = Vectors::new(0);
        layer.add(stroke(0, 10.0, b"pen", &line(20.0)));
        layer.add(stroke(0, 10.0, b"pen", &line(60.0)));
        let moved: Vec<u8> = [1.0f32, 0.0, 0.0, 1.0, 0.0, 100.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let changes = [
            Change { id: 0, delete: false, fields: vec![(field::TRANSFORM, moved), (field::BRUSH, b"ink".to_vec())] },
            Change { id: 1, delete: true, fields: Vec::new() },
            Change { id: 9, delete: true, fields: Vec::new() },
        ];
        assert!(layer.edit(&changes));
        assert_eq!(layer.items.len(), 1);
        let kept = &layer.items[0];
        assert_eq!(kept.item().points()[0], (10.0, 120.0), "moved");
        assert_eq!(kept.reach[1], 115.0, "its box with it");
        assert_eq!(layer.brushes[kept.brush as usize], b"ink");
        assert!(kept.item().brush().is_none(), "kept once, apart");
        assert!(!layer.edit(&[Change { id: 1, delete: true, fields: Vec::new() }]), "gone already");
    }

    #[test]
    fn data_from_before_ids_is_given_them_and_a_later_version_s_fields_are_kept() {
        let mut layer = Vectors::new(0);
        layer.add(stroke(0, 10.0, b"pen", &line(20.0)));
        layer.add(stroke(0, 10.0, b"pen", &line(40.0)));
        let mut data = layer.save();
        let mut more = Record::default();
        more.field(77, b"later");
        data.extend(more.bytes());
        let back = Vectors::load(0, &data).unwrap();
        assert_eq!(back.later, [(77, b"later".to_vec())]);
        assert!(back.save().ends_with(b"later"));
        // the first version: no ids, nor the next to give
        let mut first = Record::default();
        first.field(DATA_VERSION, &1u32.to_le_bytes()).field(DATA_BRUSH, b"pen");
        for kept in &layer.items {
            let mut item = Record::default();
            item.field(ITEM_BRUSH, &0u32.to_le_bytes()).field(ITEM_RECORD, &kept.record);
            first.field(DATA_ITEM, &item.bytes());
        }
        let old = Vectors::load(0, &[MAGIC.as_slice(), &first.bytes()].concat()).unwrap();
        assert_eq!((old.items[0].id, old.items[1].id, old.next_id), (0, 1, 2));
        let mut newer = Record::default();
        newer.field(DATA_VERSION, &3u32.to_le_bytes());
        assert!(Vectors::load(0, &[MAGIC.as_slice(), &newer.bytes()].concat()).is_none());
    }
}
