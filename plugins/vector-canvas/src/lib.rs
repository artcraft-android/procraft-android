//! Vector canvas: a kind of artwork whose strokes are kept as curves, not pixels. Each stroke is
//! the points the pen passed through, with their pressure, in the brush's colour, opacity and
//! width; it is drawn as an outline that swells and thins with the pressure, round at both ends.
//! Undo, redo and the artwork's file are the strokes as the plugin saves them. It's a plugin, not
//! part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The eraser cuts strokes where it passes, or, with the card's switch on, takes away each stroke
//! it touches whole. The smudge tool leaves it as it is.

use kit::canvas::{Artwork, Brush, DrawList, Phase, Tool};

/// What its file starts with, and the most strokes and points one may hold.
const MAGIC: &[u8; 6] = b"PCVEC1";
const MOST_STROKES: usize = 100_000;
const MOST_POINTS: usize = 1 << 20;
/// How far the pen goes before another point is kept, canvas pixels.
const STEP: f32 = 0.75;
/// The thinnest a stroke gets at no pressure, as a share of its width.
const THINNEST: f32 = 0.15;
/// How far a point may be from the line through its neighbours before it is kept, canvas pixels,
/// once a stroke ends; and how much its pressure may differ.
const KEEP_OFF: f32 = 0.35;
const KEEP_PRESSURE: f32 = 0.05;

#[derive(Clone, Debug, PartialEq)]
struct Stroke {
    /// 0xRRGGBBAA, the opacity in its alpha.
    colour: u32,
    width: f32,
    /// Each point: x, y and the pressure there.
    points: Vec<[f32; 3]>,
}

impl Stroke {
    /// Its width at pressure `p`.
    fn width_at(&self, p: f32) -> f32 {
        self.width * (THINNEST + (1.0 - THINNEST) * p.clamp(0.0, 1.0))
    }

    /// Drawn: a disc where it is one point, else its outline (each point's sides, out along one side
    /// and back along the other) with a disc at each end.
    fn draw(&self, list: &mut DrawList) {
        let Some(&[x0, y0, p0]) = self.points.first() else { return };
        disc(list, self.colour, x0, y0, self.width_at(p0) / 2.0);
        if self.points.len() < 2 {
            return;
        }
        let (mut left, mut right) = (Vec::with_capacity(self.points.len()), Vec::with_capacity(self.points.len()));
        for (i, &[x, y, p]) in self.points.iter().enumerate() {
            // the way it goes here, from the point before to the one after
            let before = self.points[i.saturating_sub(1)];
            let after = self.points[(i + 1).min(self.points.len() - 1)];
            let (dx, dy) = (after[0] - before[0], after[1] - before[1]);
            let length = dx.hypot(dy).max(1e-6);
            let half = self.width_at(p) / 2.0;
            let (nx, ny) = (-dy / length * half, dx / length * half);
            left.push((x + nx, y + ny));
            right.push((x - nx, y - ny));
        }
        list.path(self.colour, 0, 0.0, false);
        let (fx, fy) = left[0];
        list.move_to(fx, fy);
        // each side a smooth curve through its points, joined across the ends by straight lines
        curve_through(list, &left);
        let back: Vec<(f32, f32)> = right.iter().rev().copied().collect();
        list.line_to(back[0].0, back[0].1);
        curve_through(list, &back);
        list.close();
        let &[x1, y1, p1] = self.points.last().unwrap_or(&[x0, y0, p0]);
        disc(list, self.colour, x1, y1, self.width_at(p1) / 2.0);
    }
}

/// A smooth curve through `points` from the first (where the path is), each span a cubic curve
/// whose handles follow the points either side of it.
fn curve_through(list: &mut DrawList, points: &[(f32, f32)]) {
    for i in 0..points.len().saturating_sub(1) {
        let at = |j: usize| points[j.min(points.len() - 1)];
        let (p0, p1, p2, p3) = (at(i.saturating_sub(1)), at(i), at(i + 1), at(i + 2));
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        list.cubic_to(c1.0, c1.1, c2.0, c2.1, p2.0, p2.1);
    }
}

/// `points` with those the line through the rest passes close enough to left out (the ends kept),
/// a point kept too where its pressure differs.
fn thinned(points: &[[f32; 3]]) -> Vec<[f32; 3]> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut spans = vec![(0, points.len() - 1)];
    while let Some((a, b)) = spans.pop() {
        let ([ax, ay, ap], [bx, by, bp]) = (points[a], points[b]);
        let (dx, dy) = (bx - ax, by - ay);
        let length = dx.hypot(dy).max(1e-6);
        let mut far = (0.0, None);
        for (i, &[x, y, p]) in points.iter().enumerate().take(b).skip(a + 1) {
            let off = ((x - ax) * dy - (y - ay) * dx).abs() / length;
            let t = (i - a) as f32 / (b - a) as f32;
            let pressure_off = (p - (ap + (bp - ap) * t)).abs() / KEEP_PRESSURE * KEEP_OFF;
            let worst = off.max(pressure_off);
            if worst > far.0 {
                far = (worst, Some(i));
            }
        }
        if let (worst, Some(i)) = far
            && worst > KEEP_OFF
        {
            keep[i] = true;
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// A filled disc of radius `r` at (`x`, `y`): four quarter arcs as cubic curves.
fn disc(list: &mut DrawList, colour: u32, x: f32, y: f32, r: f32) {
    if r <= 0.0 {
        return;
    }
    // how far a quarter arc's handles reach
    let k = 0.552_284_8 * r;
    list.path(colour, 0, 0.0, false).move_to(x + r, y);
    list.cubic_to(x + r, y + k, x + k, y + r, x, y + r);
    list.cubic_to(x - k, y + r, x - r, y + k, x - r, y);
    list.cubic_to(x - r, y - k, x - k, y - r, x, y - r);
    list.cubic_to(x + k, y - r, x + r, y - k, x + r, y);
    list.close();
}

/// Its one switch: the eraser takes whole strokes (else it cuts them).
static CONTROLS: kit::Controls<1> = kit::Controls::new([0.0]);
kit::controls!(CONTROLS);

/// The distance from (x, y) to the segment from `a` to `b`, and how far along it the nearest point
/// is (0 to 1).
fn distance_to(x: f32, y: f32, a: [f32; 3], b: [f32; 3]) -> (f32, f32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length2 = dx * dx + dy * dy;
    let t = if length2 > 0.0 { (((x - a[0]) * dx + (y - a[1]) * dy) / length2).clamp(0.0, 1.0) } else { 0.0 };
    ((x - (a[0] + dx * t)).hypot(y - (a[1] + dy * t)), t)
}

/// `points` with more between them, `step` apart at the most, the pressure going smoothly.
fn densified(points: &[[f32; 3]], step: f32) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(points.len());
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil().clamp(1.0, 10_000.0) as usize;
        for i in 0..n {
            let t = i as f32 / n as f32;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
        }
    }
    out.extend(points.last().copied());
    out
}

impl Stroke {
    /// Whether the eraser at (`x`, `y`), `r` across its middle, touches it.
    fn touched_by(&self, x: f32, y: f32, r: f32) -> bool {
        match self.points.as_slice() {
            [] => false,
            [p] => (x - p[0]).hypot(y - p[1]) <= r + self.width_at(p[2]) / 2.0,
            points => points.windows(2).any(|pair| {
                let (d, t) = distance_to(x, y, pair[0], pair[1]);
                d <= r + self.width_at(pair[0][2] + (pair[1][2] - pair[0][2]) * t) / 2.0
            }),
        }
    }

    /// What is left of it with the eraser at (`x`, `y`), `r` across, cut out: the runs of it the
    /// eraser doesn't reach, each a stroke of its own.
    fn cut(&self, x: f32, y: f32, r: f32) -> Vec<Stroke> {
        let dense = densified(&self.points, 1.0);
        let mut runs: Vec<Vec<[f32; 3]>> = vec![Vec::new()];
        for p in dense {
            let reached = (x - p[0]).hypot(y - p[1]) <= r + self.width_at(p[2]) / 2.0;
            match (reached, runs.last_mut()) {
                (false, Some(run)) => run.push(p),
                (true, Some(run)) if !run.is_empty() => runs.push(Vec::new()),
                _ => {}
            }
        }
        let piece = |points: Vec<[f32; 3]>| Stroke { colour: self.colour, width: self.width, points: thinned(&points) };
        runs.into_iter().filter(|run| !run.is_empty()).map(piece).collect()
    }
}

struct Vector {
    strokes: Vec<Stroke>,
    /// The stroke being drawn, if the pen is down painting.
    open: Option<Stroke>,
    /// The pen is down erasing.
    erasing: bool,
    brush: Brush,
}

impl Vector {
    /// The eraser at (`x`, `y`): what it reaches cut out, or the strokes it touches taken away
    /// whole; whether anything went.
    fn erase(&mut self, x: f32, y: f32) -> bool {
        let r = (self.brush.size / 2.0).max(0.5);
        let whole = CONTROLS.on(0);
        let mut changed = false;
        let mut kept = Vec::with_capacity(self.strokes.len());
        for stroke in self.strokes.drain(..) {
            if !stroke.touched_by(x, y, r) {
                kept.push(stroke);
            } else {
                if !whole {
                    kept.extend(stroke.cut(x, y, r));
                }
                changed = true;
            }
        }
        kept.truncate(MOST_STROKES);
        self.strokes = kept;
        changed
    }
}

impl Artwork for Vector {
    fn new(_width: u32, _height: u32) -> Self {
        Vector {
            strokes: Vec::new(),
            open: None,
            erasing: false,
            brush: Brush { tool: Tool::Paint, colour: 0x000000FF, size: 10.0, opacity: 1.0 },
        }
    }

    fn brush(&mut self, brush: Brush) {
        self.brush = brush;
    }

    fn touch(&mut self, phase: Phase, x: f32, y: f32, pressure: f32) -> bool {
        // erasing: the eraser goes where the pen goes
        if phase == Phase::Down && self.brush.tool == Tool::Erase {
            self.erasing = true;
        }
        if self.erasing {
            let changed = phase != Phase::Off && self.erase(x, y);
            if matches!(phase, Phase::Up | Phase::Off) {
                self.erasing = false;
            }
            return changed;
        }
        match phase {
            Phase::Down if self.brush.tool == Tool::Paint => {
                let alpha = (self.brush.opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
                let colour = (self.brush.colour & 0xFFFF_FF00) | alpha;
                let width = self.brush.size.clamp(0.5, 2000.0);
                self.open = Some(Stroke { colour, width, points: vec![[x, y, pressure]] });
                true
            }
            Phase::Moved | Phase::Up => {
                let Some(open) = self.open.as_mut() else { return false };
                let far = open.points.last().is_none_or(|l| (x - l[0]).hypot(y - l[1]) >= STEP);
                if (far || phase == Phase::Up) && open.points.len() < MOST_POINTS {
                    open.points.push([x, y, pressure]);
                }
                if phase == Phase::Up
                    && let Some(mut done) = self.open.take()
                    && self.strokes.len() < MOST_STROKES
                {
                    // kept as few points as draw it as it was drawn
                    done.points = thinned(&done.points);
                    self.strokes.push(done);
                }
                true
            }
            Phase::Off => self.open.take().is_some(),
            // (the smudge tool leaves it as it is)
            Phase::Down => false,
        }
    }

    fn draw(&self, list: &mut DrawList) {
        for stroke in self.strokes.iter().chain(&self.open) {
            stroke.draw(list);
        }
    }

    fn save(&self) -> Vec<u8> {
        let mut data = MAGIC.to_vec();
        data.extend((self.strokes.len() as u32).to_le_bytes());
        for s in &self.strokes {
            data.extend(s.colour.to_le_bytes());
            data.extend(s.width.to_le_bytes());
            data.extend((s.points.len() as u32).to_le_bytes());
            for p in &s.points {
                for n in p {
                    data.extend(n.to_le_bytes());
                }
            }
        }
        data
    }

    fn load(width: u32, height: u32, data: &[u8]) -> Option<Self> {
        let mut at = 0;
        let mut take = |n: usize| {
            let part = data.get(at..at + n)?;
            at += n;
            Some(part)
        };
        if take(MAGIC.len())? != MAGIC {
            return None;
        }
        let word = |b: &[u8]| -> Option<u32> { Some(u32::from_le_bytes(b.try_into().ok()?)) };
        let count = word(take(4)?)? as usize;
        if count > MOST_STROKES {
            return None;
        }
        let mut artwork = Vector::new(width, height);
        for _ in 0..count {
            let colour = word(take(4)?)?;
            let width = f32::from_bits(word(take(4)?)?);
            let n = word(take(4)?)? as usize;
            if !width.is_finite() || n > MOST_POINTS {
                return None;
            }
            let mut points = Vec::with_capacity(n);
            for _ in 0..n {
                let mut p = [0.0; 3];
                for kept in &mut p {
                    *kept = f32::from_bits(word(take(4)?)?);
                }
                if !p.iter().all(|v| v.is_finite()) {
                    return None;
                }
                points.push(p);
            }
            artwork.strokes.push(Stroke { colour, width, points });
        }
        Some(artwork)
    }
}

kit::canvas!(Vector);

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(v: &Vector) -> Vec<u8> {
        let mut list = DrawList::default();
        v.draw(&mut list);
        list.bytes()
    }

    #[test]
    fn a_stroke_is_kept_drawn_saved_and_loaded() {
        let mut v = Vector::new(100, 100);
        v.brush(Brush { tool: Tool::Paint, colour: 0x3366CCFF, size: 8.0, opacity: 0.5 });
        assert!(v.touch(Phase::Down, 10.0, 10.0, 0.5));
        for i in 1..20 {
            v.touch(Phase::Moved, 10.0 + i as f32 * 3.0, 10.0, 1.0);
        }
        assert!(v.touch(Phase::Up, 70.0, 10.0, 0.2));
        assert_eq!(v.strokes.len(), 1);
        assert_eq!(v.strokes[0].colour, 0x3366CC80, "its opacity in its alpha");
        assert!(!drawn(&v).is_empty());
        // through its file and back, the same
        let data = v.save();
        let back = Vector::load(100, 100, &data).unwrap();
        assert_eq!(back.strokes, v.strokes);
        // one that doesn't hold up is none
        assert!(Vector::load(100, 100, b"nope").is_none());
        assert!(Vector::load(100, 100, &data[..data.len() - 2]).is_none());
    }

    #[test]
    fn a_stroke_keeps_only_the_points_that_shape_it() {
        // a straight line at one pressure: its two ends
        let line: Vec<[f32; 3]> = (0..50).map(|i| [i as f32, 10.0, 1.0]).collect();
        assert_eq!(thinned(&line), vec![[0.0, 10.0, 1.0], [49.0, 10.0, 1.0]]);
        // a corner keeps its corner, a change of pressure its middle
        let corner: Vec<[f32; 3]> =
            (0..20).map(|i| [i as f32, 0.0, 1.0]).chain((1..20).map(|i| [19.0, i as f32, 1.0])).collect();
        assert!(thinned(&corner).contains(&[19.0, 0.0, 1.0]));
        let swelling: Vec<[f32; 3]> = (0..21).map(|i| [i as f32, 0.0, 1.0 - (i as f32 - 10.0).abs() / 10.0]).collect();
        assert!(thinned(&swelling).contains(&[10.0, 0.0, 1.0]));
    }

    /// A horizontal stroke from x 10 to 90 at y 50, 4 wide.
    fn line(v: &mut Vector) {
        v.brush(Brush { tool: Tool::Paint, colour: 0xFF, size: 4.0, opacity: 1.0 });
        v.touch(Phase::Down, 10.0, 50.0, 1.0);
        for i in 1..80 {
            v.touch(Phase::Moved, 10.0 + i as f32, 50.0, 1.0);
        }
        v.touch(Phase::Up, 90.0, 50.0, 1.0);
    }

    fn erase_at(v: &mut Vector, x: f32, y: f32) -> bool {
        v.brush(Brush { tool: Tool::Erase, colour: 0xFF, size: 10.0, opacity: 1.0 });
        let changed = v.touch(Phase::Down, x, y, 1.0);
        v.touch(Phase::Up, x, y, 1.0);
        changed
    }

    #[test]
    fn the_eraser_cuts_a_stroke_where_it_passes_or_takes_it_whole() {
        // cutting: the middle goes, the two ends stay, each a stroke
        let mut v = Vector::new(100, 100);
        line(&mut v);
        assert!(!erase_at(&mut v, 50.0, 80.0), "nowhere near it");
        assert!(erase_at(&mut v, 50.0, 50.0));
        assert_eq!(v.strokes.len(), 2);
        let first_end = v.strokes[0].points.last().unwrap()[0];
        let second_start = v.strokes[1].points[0][0];
        assert!(first_end < 46.0 && second_start > 54.0, "{first_end} {second_start}");
        // whole: the stroke touched goes, all of it
        CONTROLS.told(0, 1.0);
        let mut w = Vector::new(100, 100);
        line(&mut w);
        assert!(erase_at(&mut w, 50.0, 50.0));
        assert!(w.strokes.is_empty());
        CONTROLS.told(0, 0.0);
    }

    #[test]
    fn a_called_off_stroke_leaves_it_as_it_was() {
        let mut v = Vector::new(100, 100);
        v.brush(Brush { tool: Tool::Paint, colour: 0xFF, size: 8.0, opacity: 1.0 });
        v.touch(Phase::Down, 10.0, 10.0, 1.0);
        assert!(v.touch(Phase::Off, 0.0, 0.0, 0.0));
        assert!(v.strokes.is_empty() && v.open.is_none());
    }
}
