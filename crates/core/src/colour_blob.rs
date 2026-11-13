//! The colour dragged out to be dropped (ColorDrop), as it is drawn. Pulled from the colour
//! button it stretches out of an end left over the button, its neck thinning until it lets go;
//! then it follows the pointer as a ball carried on a spring, wobbling with how fast it went.
//! Pulled from a swatch it grows out of nothing. The apps draw the outline worked out here each
//! frame; where the colour lands is the pointer's, not the drawn ball's.

use std::f64::consts::PI;
use std::sync::{Arc, Mutex, MutexGuard};

/// The interface's text size the sizes below are seen at: a finger's dot of 60 points shows 55.5
/// across, as measured.
pub const TEXT_SIZE: f64 = 0.925;
/// The dot across, before the text size: a finger's and a pen's.
const FINGER: f64 = 60.0;
const PEN: f64 = 20.0;
/// The end left over the colour button, across, before the text size.
const BUTTON: f64 = 28.0;
/// Over a bar the dot shrinks with its distance from the colour button: whole within this many
/// points of it, gone this many points further.
const BAR_WHOLE: f64 = 40.0;
const BAR_GONE: f64 = 80.0;
/// How far the drop reaches before it lets go: the button's end across, and this much of both
/// ends across.
const REACH: f64 = 0.33;
/// Past twice its reach, the pointer this much further on has the drop come out at its quickest.
const SPEED_UP: f64 = 250.0;
/// The spring's steps a second: a frame steps it as many of them as the frame took.
const TICKS: f64 = 120.0;
/// The most steps one frame takes (a stalled frame only slows the spring).
const MOST_TICKS: f64 = 4.0;
/// A quarter circle's handles, as a share of its radius.
const ROUND: f64 = 0.552_284_749_831;
/// Lines a curve of the outline is drawn as.
const CURVE_STEPS: usize = 8;
/// Where the joined drop is thinner than this (points across), it has no rim.
const RIM_LEAST: f64 = 0.7;

/// The outline to fill this frame.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct BlobShape {
    /// Closed outlines, filled together (non-zero): x, y, x, y, ... in points.
    pub outlines: Vec<BlobOutline>,
    /// The same area as triangles, three x, y pairs each, for a painter that fills only those.
    pub triangles: Vec<f32>,
    /// The outlines' edges where there is something either side of them to fill, as lines (a
    /// closed one ends where it began), for such a painter to smooth the triangles' edges by:
    /// where the neck has thinned to a line there is no edge.
    pub rims: Vec<BlobOutline>,
}

/// One outline of a [`BlobShape`].
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct BlobOutline {
    pub points: Vec<f32>,
}

/// What a frame draws, from the spring: both ends, how far the drop has let go, and the wobble.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    origin: [f64; 2],
    origin_d: f64,
    end: [f64; 2],
    end_d: f64,
    apart: f64,
    /// How far the ball squashes (0 not at all) and where in its wobble it is (half turns).
    wobble: f64,
    turn: f64,
}

#[derive(Clone, Debug)]
struct Spring {
    /// Where it is pulled from and that end across; where the pointer is and the dot across.
    origin: [f64; 2],
    origin_d: f64,
    to: [f64; 2],
    to_d: f64,
    /// The dot across before the bars shrink it.
    whole_d: f64,
    reduce_motion: bool,
    /// The drawn end carried towards the pointer, and how far it goes a step.
    end: [f64; 2],
    speed: [f64; 2],
    /// How far out the drop is pulled (0 at the start, 1 its reach), lagging the pointer.
    pulled: f64,
    /// How far it has let go (0 joined, 1 apart).
    apart: f64,
    /// How much it is wobbling (0..1), and how far through its wobble (half turns).
    wobble: f64,
    turn: f64,
    /// When the spring was last stepped (ms).
    at_ms: f64,
}

impl Spring {
    fn new(x: f64, y: f64, origin_d: f64, to_d: f64, reduce_motion: bool, ms: f64) -> Spring {
        Spring {
            origin: [x, y],
            origin_d,
            to: [x, y],
            to_d,
            whole_d: to_d,
            reduce_motion,
            end: [x, y],
            speed: [0.0; 2],
            pulled: 0.0,
            apart: 0.0,
            wobble: 0.0,
            turn: 0.0,
            at_ms: ms,
        }
    }

    /// How far the drop reaches before it lets go, and that at least a point.
    fn reach(&self) -> (f64, f64) {
        let reach = self.origin_d + REACH * (self.origin_d + self.to_d);
        (reach, reach.max(1.0))
    }

    /// The pointer's distance from the start, and the way to it (none at the start).
    fn span(&self) -> (f64, [f64; 2]) {
        let (dx, dy) = (self.to[0] - self.origin[0], self.to[1] - self.origin[1]);
        let distance = dx.hypot(dy);
        let way = if distance > 0.0 { [dx / distance, dy / distance] } else { [0.0; 2] };
        (distance, way)
    }

    /// How far out the pointer has pulled the drop: its distance over the reach, at most 1.
    fn wanted(&self) -> f64 {
        let (distance, _) = self.span();
        let (reach, _) = self.reach();
        (if reach >= 1.0 { distance / reach } else { distance }).min(1.0)
    }

    /// Steps the spring on by `n` of its steps.
    fn step(&mut self, n: f64) {
        let (distance, _) = self.span();
        let (_, room) = self.reach();
        let wanted = self.wanted();
        let halfway = (distance / (2.0 * room)).min(1.0);
        let beyond = ((distance - 2.0 * room) / SPEED_UP).clamp(0.0, 1.0);
        // going out it quickens the further out it is and the further the pointer is past its
        // reach; coming back, the nearer the pointer
        let (near, far) = if wanted > self.pulled { (halfway, beyond) } else { (1.0 - halfway, 1.0 - beyond) };
        let p = self.pulled;
        let rate = ((p * p * near).max(p * far) * 0.2).max(0.02) * n;
        self.pulled = wanted * rate + p * (1.0 - rate);
        // nearly all the way out it lets go, a tenth a step at most; back under 0.8 it joins again
        if self.pulled > 0.9 {
            self.apart = (self.apart + (n * (1.0 - self.apart).max(0.0)).min(0.1)).clamp(0.0, 1.0);
        } else if self.pulled < 0.8 {
            self.apart = (self.apart * (n * self.apart - 0.01).max(0.5)).clamp(0.0, 1.0);
        }
        // the drawn end is carried after the pointer, and wobbles the more the further behind
        let gap = [self.to[0] - self.end[0], self.to[1] - self.end[1]];
        let behind = (gap[0].hypot(gap[1]) / 20.0).min(1.0);
        self.wobble = self.wobble.max(behind) * 0.995 + behind * 0.005;
        self.turn += 2.0 * n / TICKS * self.wobble;
        let pull = n * 0.08;
        for i in 0..2 {
            let (g, s) = (gap.get(i).copied().unwrap_or(0.0), self.speed.get(i).copied().unwrap_or(0.0));
            let s = (g * pull + s * (1.0 - pull)) * 0.9;
            if let (Some(speed), Some(end)) = (self.speed.get_mut(i), self.end.get_mut(i)) {
                *speed = s;
                *end += s;
            }
        }
        if self.reduce_motion {
            self.turn = 1.0;
            self.apart = 1.0;
        }
    }

    /// What is drawn now.
    fn pose(&self) -> Pose {
        if self.reduce_motion {
            return Pose {
                origin: self.origin,
                origin_d: self.origin_d,
                end: self.to,
                end_d: self.to_d,
                apart: 1.0,
                wobble: 0.0,
                turn: 1.0,
            };
        }
        let (_, way) = self.span();
        let (_, room) = self.reach();
        // the button's end leans a little after the drop, most when half pulled out
        let lean = 0.1 * (PI * self.pulled).sin() * self.wanted() * room;
        let origin = [self.origin[0] + way[0] * lean, self.origin[1] + way[1] * lean];
        // the drawn end comes out from the start as the drop is pulled
        let out = self.pulled.powf(1.5);
        let end = [self.origin[0] * (1.0 - out) + self.end[0] * out, self.origin[1] * (1.0 - out) + self.end[1] * out];
        let wobble = if self.to_d > 0.0 { 20.0 / self.to_d * self.wobble * 0.15 } else { 0.0 };
        Pose { origin, origin_d: self.origin_d, end, end_d: self.to_d, apart: self.apart, wobble, turn: self.turn }
    }
}

/// A shape made in a frame of its own (x along the drop from its start), then placed.
struct Placed {
    at: [f64; 2],
    cos: f64,
    sin: f64,
}

impl Placed {
    fn put(&self, p: [f64; 2]) -> [f32; 2] {
        [
            (self.at[0] + p[0] * self.cos - p[1] * self.sin) as f32,
            (self.at[1] + p[0] * self.sin + p[1] * self.cos) as f32,
        ]
    }
}

/// The points along the cubic from `a` (left out) by handles `b` and `c` to `d`.
fn curve(out: &mut Vec<[f64; 2]>, a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) {
    for k in 1..=CURVE_STEPS {
        let t = k as f64 / CURVE_STEPS as f64;
        let u = 1.0 - t;
        let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push([w0 * a[0] + w1 * b[0] + w2 * c[0] + w3 * d[0], w0 * a[1] + w1 * b[1] + w2 * c[1] + w3 * d[1]]);
    }
}

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

/// A circle of radius `r` round (0, 0), from its top, a quarter at a time.
fn circle(r: f64) -> Vec<[f64; 2]> {
    let k = r * ROUND;
    let mut out = vec![[0.0, -r]];
    curve(&mut out, [0.0, -r], [k, -r], [r, -k], [r, 0.0]);
    curve(&mut out, [r, 0.0], [r, k], [k, r], [0.0, r]);
    curve(&mut out, [0.0, r], [-k, r], [-r, k], [-r, 0.0]);
    curve(&mut out, [-r, 0.0], [-r, -k], [-k, -r], [0.0, -r]);
    out.pop();
    out
}

/// Adds the closed outline `points` (placed by `frame`) to `shape`, filled as a fan from `middle`
/// (it is convex round it).
fn add_round(shape: &mut BlobShape, frame: &Placed, points: &[[f64; 2]], middle: [f64; 2]) {
    let placed: Vec<[f32; 2]> = points.iter().map(|&p| frame.put(p)).collect();
    let centre = frame.put(middle);
    for (i, a) in placed.iter().enumerate() {
        let b = placed.get((i + 1) % placed.len()).unwrap_or(a);
        shape.triangles.extend_from_slice(&[centre[0], centre[1], a[0], a[1], b[0], b[1]]);
    }
    shape.outlines.push(BlobOutline { points: placed.iter().flatten().copied().collect() });
    let closed = placed.iter().chain(placed.first()).flatten().copied().collect();
    shape.rims.push(BlobOutline { points: closed });
}

/// The outline of `pose`.
fn outline(pose: &Pose) -> BlobShape {
    let mut shape = BlobShape::default();
    let (r0, r1) = (pose.origin_d / 2.0, pose.end_d / 2.0);
    let (dx, dy) = (pose.end[0] - pose.origin[0], pose.end[1] - pose.origin[1]);
    let length = dx.hypot(dy);
    let angle = dy.atan2(dx);
    let frame = Placed { at: pose.origin, cos: angle.cos(), sin: angle.sin() };
    let reach = pose.origin_d + REACH * (pose.origin_d + pose.end_d);
    let out = (if reach >= 1.0 { length / reach } else { length }).min(1.0);
    // the neck thins as the drop comes out, and over the last quarter both ends round off
    let rounded = pose.apart + (1.0 - pose.apart) * ((out - 0.75) / 0.25).clamp(0.0, 1.0);
    let grown = pose.apart + (1.0 - pose.apart) * (out / 0.75).min(1.0);
    let h = (rounded + grown) / 2.0;
    // the far end grows from the start's size to the dot's
    let re = r0 * (1.0 - h) + r1 * h;
    if pose.apart >= 1.0 {
        // apart: the button's end, and the ball squashed and turned as it wobbles
        if r0 > 0.0 {
            add_round(&mut shape, &frame, &circle(r0), [0.0, 0.0]);
        }
        if re > 0.0 {
            let a = PI * pose.turn;
            let (sx, sy) =
                (1.0 - pose.wobble + pose.wobble * (4.0 * a).cos(), 1.0 - pose.wobble + pose.wobble * (3.0 * a).sin());
            let (c, s) = (a.cos(), a.sin());
            let ball: Vec<[f64; 2]> = circle(re)
                .into_iter()
                .map(|[x, y]| {
                    let (x, y) = (x * sx, y * sy);
                    [length + x * c - y * s, x * s + y * c]
                })
                .collect();
            add_round(&mut shape, &frame, &ball, [length, 0.0]);
        }
        return shape;
    }
    // joined: one half of the outline, the other its mirror across the drop
    let neck = (1.0 - grown.powi(3)) * (r0 + re) / 2.0;
    let (flare0, flare1) = (1.5 * grown * r0, 1.5 * grown * re);
    let (k0, ke) = (r0 * ROUND, re * ROUND);
    let m = rounded;
    let a = [mix(flare0, r0, m), (1.0 - m) * neck];
    let b = [mix(length - flare1, length - re, m), (1.0 - m) * neck];
    let mut half = vec![[-r0, 0.0]];
    curve(&mut half, [-r0, 0.0], [-r0, k0], [-k0, r0], [0.0, r0]);
    curve(&mut half, [0.0, r0], [mix(flare0 / 2.0, k0, m), r0], [mix(0.75 * flare0, r0, m), mix(neck, k0, m)], a);
    half.push(b);
    curve(
        &mut half,
        b,
        [mix(length - 0.75 * flare1, length - re, m), mix(neck, ke, m)],
        [mix(length - flare1 / 2.0, length - ke, m), re],
        [length, re],
    );
    curve(&mut half, [length, re], [length + ke, re], [length + re, ke], [length + re, 0.0]);
    let placed: Vec<([f32; 2], [f32; 2])> =
        half.iter().map(|&[x, y]| (frame.put([x, y]), frame.put([x, -y]))).collect();
    // filled as a strip between the half and its mirror: right where the halves cross over
    // themselves too (a drop pulled back over its start)
    for pair in placed.windows(2) {
        if let [(a, a2), (b, b2)] = pair {
            shape.triangles.extend_from_slice(&[a[0], a[1], b[0], b[1], b2[0], b2[1]]);
            shape.triangles.extend_from_slice(&[a[0], a[1], b2[0], b2[1], a2[0], a2[1]]);
        }
    }
    let mut points: Vec<f32> = placed.iter().flat_map(|(p, _)| *p).collect();
    points.extend(placed.iter().rev().skip(1).take(placed.len().saturating_sub(2)).flat_map(|(_, q)| *q));
    shape.outlines.push(BlobOutline { points });
    // the rims: round the outline, broken where the drop is too thin to have one (its two tips,
    // on the drop's line, always kept)
    let last = half.len().saturating_sub(1);
    let thick = |i: usize, y: f64| i == 0 || i == last || 2.0 * y >= RIM_LEAST;
    let mut ring: Vec<([f32; 2], bool)> =
        half.iter().zip(&placed).enumerate().map(|(i, (&[_, y], &(p, _)))| (p, thick(i, y))).collect();
    let back = half.iter().zip(&placed).enumerate().rev().skip(1).take(last.saturating_sub(1));
    ring.extend(back.map(|(i, (&[_, y], &(_, q)))| (q, thick(i, y))));
    shape.rims = rims(&ring);
    shape
}

/// The runs of `ring` (a closed outline) whose points are marked, as lines; all of it marked, the
/// whole of it closed.
fn rims(ring: &[([f32; 2], bool)]) -> Vec<BlobOutline> {
    let Some(gap) = ring.iter().position(|&(_, keep)| !keep) else {
        let points = ring.iter().chain(ring.first()).flat_map(|(p, _)| *p).collect();
        return vec![BlobOutline { points }];
    };
    let mut out = Vec::new();
    let mut run: Vec<f32> = Vec::new();
    // from a gap round to it again, so no run is cut in two
    for i in 1..=ring.len() {
        match ring.get((gap + i) % ring.len()) {
            Some(&(p, true)) => run.extend_from_slice(&p),
            _ => {
                if run.len() >= 4 {
                    out.push(BlobOutline { points: std::mem::take(&mut run) });
                }
                run.clear();
            }
        }
    }
    out
}

/// The dot across for a pen or a finger at the interface's text size.
fn dot_across(pen: bool) -> f64 {
    (if pen { PEN } else { FINGER }) * TEXT_SIZE
}

/// A colour being dragged out, as it is drawn: made as the drag starts, told where the pointer
/// goes, asked for its outline each frame.
#[derive(uniffi::Object)]
pub struct ColourBlob {
    spring: Mutex<Spring>,
}

#[uniffi::export]
impl ColourBlob {
    /// The colour pulled out from (`x`, `y`) (points) at `ms`: out of the colour button
    /// (`from_button`: its centre, an end left over it while the drop is joined) or out of a
    /// swatch; by a pen or a finger. With `reduce_motion` it neither stretches nor wobbles: the
    /// dot is under the pointer.
    #[uniffi::constructor]
    pub fn new(x: f32, y: f32, from_button: bool, pen: bool, reduce_motion: bool, ms: f64) -> Arc<ColourBlob> {
        let origin_d = if from_button { BUTTON * TEXT_SIZE } else { 0.0 };
        let spring = Spring::new(f64::from(x), f64::from(y), origin_d, dot_across(pen), reduce_motion, ms);
        Arc::new(ColourBlob { spring: Mutex::new(spring) })
    }

    /// The pointer is at (`x`, `y`) (points); `over_bar` when that is over one of the bars, where
    /// the dot shrinks the further it is from the colour button's centre (`button_x`,
    /// `button_y`).
    pub fn point_to(&self, x: f32, y: f32, over_bar: bool, button_x: f32, button_y: f32) {
        let mut s = self.lock();
        let (x, y) = (f64::from(x), f64::from(y));
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        s.to = [x, y];
        let shrunk = if over_bar {
            let away = (x - f64::from(button_x)).hypot(y - f64::from(button_y));
            ((away - BAR_WHOLE) / BAR_GONE).clamp(0.0, 1.0)
        } else {
            0.0
        };
        s.to_d = if shrunk.is_finite() { (1.0 - shrunk) * s.whole_d } else { s.whole_d };
    }

    /// The outline to draw at `ms`, the spring stepped on to then.
    pub fn shape(&self, ms: f64) -> BlobShape {
        let mut s = self.lock();
        let ticks = ((ms - s.at_ms) / 1000.0 * TICKS).clamp(0.0, MOST_TICKS);
        if ms.is_finite() {
            s.at_ms = s.at_ms.max(ms);
        }
        if ticks > 0.0 {
            s.step(ticks);
        }
        outline(&s.pose())
    }
}

impl ColourBlob {
    fn lock(&self) -> MutexGuard<'_, Spring> {
        self.spring.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A colour pulled out of the colour button at (100, 20) by a finger.
    fn from_button() -> Spring {
        Spring::new(100.0, 20.0, BUTTON * TEXT_SIZE, FINGER * TEXT_SIZE, false, 0.0)
    }

    /// The outline's farthest reach from `p` along the way from the start to the drawn end.
    fn bounds(shape: &BlobShape) -> [f32; 4] {
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for o in &shape.outlines {
            for p in o.points.chunks(2) {
                b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
            }
        }
        b
    }

    #[test]
    fn the_dot_is_sixty_points_by_a_finger_and_twenty_by_a_pen_at_the_text_size() {
        assert!((dot_across(false) - 55.5).abs() < 1e-9);
        assert!((dot_across(true) - 18.5).abs() < 1e-9);
        let blob = ColourBlob::new(100.0, 20.0, true, false, false, 0.0);
        assert!((blob.lock().origin_d - 25.9).abs() < 1e-9, "the button's end is 28 points");
        assert_eq!(ColourBlob::new(0.0, 0.0, false, true, false, 0.0).lock().origin_d, 0.0);
    }

    #[test]
    fn at_the_start_the_drop_is_the_button_s_end() {
        let s = from_button();
        let shape = outline(&s.pose());
        assert_eq!(shape.outlines.len(), 1);
        let b = bounds(&shape);
        // a disc 25.9 across round the button's centre
        assert!((b[2] - b[0] - 25.9).abs() < 0.2 && (b[3] - b[1] - 25.9).abs() < 0.2, "{b:?}");
        assert!(((b[0] + b[2]) / 2.0 - 100.0).abs() < 0.1 && ((b[1] + b[3]) / 2.0 - 20.0).abs() < 0.1);
    }

    #[test]
    fn its_reach_is_the_button_s_end_and_a_third_of_both_ends() {
        let s = from_button();
        let (reach, _) = s.reach();
        assert!((reach - (25.9 + 0.33 * (25.9 + 55.5))).abs() < 1e-9);
    }

    #[test]
    fn pulled_out_it_stretches_then_lets_go_and_the_ball_follows_behind() {
        let mut s = from_button();
        // the finger moves 200 points down at once and stays there; a 120 Hz step at a time
        s.to = [100.0, 220.0];
        let mut joined_steps = 0;
        let mut narrowest = f64::MAX;
        for _ in 0..240 {
            s.step(1.0);
            let pose = s.pose();
            if pose.apart < 1.0 {
                joined_steps += 1;
                // the joined drop is one outline, thinner at its neck than either end
                let shape = outline(&pose);
                assert_eq!(shape.outlines.len(), 1);
                narrowest = narrowest.min(pose.apart);
            }
        }
        assert!(joined_steps > 5, "it stays joined a moment ({joined_steps} steps)");
        assert_eq!(s.apart, 1.0, "then it has let go");
        let shape = outline(&s.pose());
        assert_eq!(shape.outlines.len(), 2, "two discs: the button's end and the ball");
        // the ball, settled, is the dot across under the finger
        let pose = s.pose();
        assert!((pose.end[1] - 220.0).abs() < 1.0, "{:?}", pose.end);
        assert!((pose.end_d - 55.5).abs() < 1e-9);
        assert!(narrowest <= 1.0);
    }

    #[test]
    fn the_ball_overshoots_and_comes_back_a_spring_s_way() {
        let mut s = from_button();
        s.to = [100.0, 320.0];
        let mut furthest: f64 = 0.0;
        for _ in 0..600 {
            s.step(1.0);
            furthest = furthest.max(s.end[1]);
        }
        assert!(furthest > 320.5, "carried past the finger ({furthest})");
        assert!((s.end[1] - 320.0).abs() < 0.5, "and back under it");
    }

    /// How wide `shape`'s first outline is across the drop (x, the drop going down) at each
    /// height from `y0` to `y1`, in `n` steps.
    fn widths(shape: &BlobShape, y0: f32, y1: f32, n: usize) -> Vec<f32> {
        let pts: Vec<[f32; 2]> = shape.outlines[0].points.chunks(2).map(|p| [p[0], p[1]]).collect();
        (0..=n)
            .map(|k| {
                let y = y0 + (y1 - y0) * k as f32 / n as f32;
                let mut xs = Vec::new();
                for (i, a) in pts.iter().enumerate() {
                    let b = pts[(i + 1) % pts.len()];
                    if (a[1] - y) * (b[1] - y) <= 0.0 && a[1] != b[1] {
                        xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                    }
                }
                (xs.iter().copied().fold(f32::MIN, f32::max) - xs.iter().copied().fold(f32::MAX, f32::min)).max(0.0)
            })
            .collect()
    }

    #[test]
    fn nearly_at_its_reach_its_neck_is_thin_and_its_far_end_grown() {
        let mut s = from_button();
        // held just inside its reach (0.89 of it): joined for good, pulled most of the way
        s.to = [100.0, 20.0 + 47.0];
        for _ in 0..400 {
            s.step(1.0);
        }
        let pose = s.pose();
        assert_eq!(pose.apart, 0.0, "still joined");
        let w = widths(&outline(&pose), pose.origin[1] as f32, pose.end[1] as f32, 24);
        let neck = w.iter().copied().fold(f32::MAX, f32::min);
        let far = w.last().copied().unwrap_or(0.0);
        assert!(neck < 25.9 / 2.0, "a thin neck: {w:?}");
        // the far end half way from the button's end to the dot
        assert!((far - (25.9 + 55.5) / 2.0).abs() < 2.0, "{w:?}");
    }

    #[test]
    fn brought_back_it_joins_again_at_120_hz_but_stays_apart_stepped_two_at_a_time() {
        // apart, then the finger back over the button
        let parted = |n: f64| {
            let mut s = from_button();
            s.to = [100.0, 220.0];
            for _ in 0..(240.0 / n) as usize {
                s.step(n);
            }
            assert_eq!(s.apart, 1.0);
            s.to = [100.0, 20.0];
            for _ in 0..(480.0 / n) as usize {
                s.step(n);
            }
            s.apart
        };
        assert!(parted(1.0) < 0.01, "at 120 frames a second it joins again");
        // at 60 frames a second (two steps a frame) the rule never takes it back under: it stays
        // apart until the next drag
        assert_eq!(parted(2.0), 1.0);
    }

    #[test]
    fn reduce_motion_has_the_dot_under_the_pointer_apart_and_still() {
        let blob = ColourBlob::new(100.0, 20.0, true, false, true, 0.0);
        blob.point_to(300.0, 400.0, false, 100.0, 20.0);
        let shape = blob.shape(8.0);
        let s = blob.lock();
        let pose = s.pose();
        assert_eq!((pose.end, pose.apart, pose.wobble), ([300.0, 400.0], 1.0, 0.0));
        assert_eq!(shape.outlines.len(), 2);
        // the ball is a circle the dot across round the pointer
        let ball = &shape.outlines[1].points;
        for p in ball.chunks(2) {
            let r = (p[0] - 300.0).hypot(p[1] - 400.0);
            assert!((r - 27.75).abs() < 0.3, "{r}");
        }
    }

    #[test]
    fn over_a_bar_the_dot_shrinks_away_from_the_colour_button() {
        let blob = ColourBlob::new(100.0, 20.0, true, false, false, 0.0);
        let across = |x: f32, over: bool| {
            blob.point_to(x, 20.0, over, 100.0, 20.0);
            blob.lock().to_d
        };
        assert!((across(130.0, true) - 55.5).abs() < 1e-9, "whole within 40 points");
        assert!((across(180.0, true) - 55.5 / 2.0).abs() < 1e-9, "half at 80");
        assert_eq!(across(260.0, true), 0.0, "gone past 120");
        assert!((across(260.0, false) - 55.5).abs() < 1e-9, "off the bars it is whole");
    }

    #[test]
    fn a_frame_steps_the_spring_by_the_time_it_took() {
        let a = ColourBlob::new(0.0, 0.0, true, false, false, 0.0);
        let b = ColourBlob::new(0.0, 0.0, true, false, false, 0.0);
        a.point_to(0.0, 200.0, false, 0.0, 0.0);
        b.point_to(0.0, 200.0, false, 0.0, 0.0);
        // two frames of 1/120 s, or one of 1/60 s stepped twice as far
        a.shape(1000.0 / 120.0);
        a.shape(2000.0 / 120.0);
        b.shape(2000.0 / 120.0);
        let (pa, pb) = (a.lock().pulled, b.lock().pulled);
        assert!(pa > 0.0 && pb > 0.0);
        assert!((pa - pb).abs() < pa * 0.1, "{pa} {pb}");
        // the same moment again steps nothing
        let before = b.lock().pulled;
        b.shape(2000.0 / 120.0);
        assert_eq!(b.lock().pulled, before);
    }

    #[test]
    fn a_swatch_s_drop_grows_out_of_nothing() {
        let mut s = Spring::new(50.0, 50.0, 0.0, FINGER * TEXT_SIZE, false, 0.0);
        let shape = outline(&s.pose());
        let b = bounds(&shape);
        assert!(b[2] - b[0] < 0.01, "nothing at the start: {b:?}");
        s.to = [50.0, 250.0];
        for _ in 0..240 {
            s.step(1.0);
        }
        let shape = outline(&s.pose());
        assert_eq!(shape.outlines.len(), 1, "apart, just the ball");
        let b = bounds(&shape);
        assert!((b[3] - b[1] - 55.5).abs() < 4.0, "{b:?}");
    }

    #[test]
    fn a_neck_thinned_to_a_line_has_no_rim_along_it() {
        let mut s = from_button();
        s.to = [100.0, 20.0 + 47.0];
        for _ in 0..400 {
            s.step(1.0);
        }
        let shape = outline(&s.pose());
        // joined and its neck still has width: one rim round all of it, closed
        assert_eq!(shape.rims.len(), 1);
        let rim = &shape.rims[0].points;
        assert_eq!(rim[..2], rim[rim.len() - 2..]);
        // just before it lets go, the neck is a line: the rim breaks there, top and bottom
        let pose = Pose { apart: 0.0, ..s.pose() };
        let (r0, re) = (pose.origin_d / 2.0, pose.end_d / 2.0);
        let length = 0.33 * (pose.origin_d + pose.end_d) + pose.origin_d + 5.0;
        let pose = Pose { end: [pose.origin[0], pose.origin[1] + length], ..pose };
        let shape = outline(&pose);
        assert!(shape.rims.len() >= 2, "{}", shape.rims.len());
        // no rim point lies on the neck's line between the two ends
        for rim in &shape.rims {
            for p in rim.points.chunks(2) {
                let along = f64::from(p[1]) - pose.origin[1];
                let across = (f64::from(p[0]) - pose.origin[0]).abs();
                assert!(!(along > r0 + 1.0 && along < length - re - 1.0 && across < 0.3), "{p:?}");
            }
        }
        // apart, each disc's rim is closed
        let apart = outline(&Pose { apart: 1.0, ..pose });
        assert_eq!(apart.rims.len(), 2);
        for rim in &apart.rims {
            assert_eq!(rim.points[..2], rim.points[rim.points.len() - 2..]);
        }
    }

    #[test]
    fn its_triangles_cover_what_its_outline_fills() {
        // the joined drop half out, its ends crossing into each other, and apart
        for dist in [20.0, 47.0, 200.0] {
            let mut s = from_button();
            s.to = [130.0, 20.0 + dist];
            for _ in 0..400 {
                s.step(1.0);
            }
            let shape = outline(&s.pose());
            let inside_triangle = |x: f32, y: f32| {
                shape.triangles.chunks(6).any(|t| {
                    let d = |ax: f32, ay: f32, bx: f32, by: f32| (bx - ax) * (y - ay) - (by - ay) * (x - ax);
                    let (d1, d2, d3) =
                        (d(t[0], t[1], t[2], t[3]), d(t[2], t[3], t[4], t[5]), d(t[4], t[5], t[0], t[1]));
                    (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0)
                })
            };
            // the outlines' winding, non-zero inside
            let winding = |x: f32, y: f32| -> i32 {
                let mut w = 0;
                for o in &shape.outlines {
                    let pts: Vec<[f32; 2]> = o.points.chunks(2).map(|p| [p[0], p[1]]).collect();
                    for (i, a) in pts.iter().enumerate() {
                        let b = pts[(i + 1) % pts.len()];
                        let side = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
                        if a[1] <= y && b[1] > y && side > 0.0 {
                            w += 1;
                        } else if a[1] > y && b[1] <= y && side < 0.0 {
                            w -= 1;
                        }
                    }
                }
                w
            };
            let b = bounds(&shape);
            let (mut inside, mut differ) = (0, 0);
            let mut y = b[1] + 0.25;
            while y < b[3] {
                let mut x = b[0] + 0.25;
                while x < b[2] {
                    let filled = winding(x, y) != 0;
                    inside += i32::from(filled);
                    differ += i32::from(filled != inside_triangle(x, y));
                    x += 0.5;
                }
                y += 0.5;
            }
            assert!(inside > 0 && differ * 100 < inside, "{dist}: {differ} of {inside}");
        }
    }
}
