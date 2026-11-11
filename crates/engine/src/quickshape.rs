//! Shapes recognised in a stroke drawn and held: a line, a polyline or an arc for open strokes;
//! an ellipse or circle, a triangle, a quadrilateral, a rectangle or square for closed ones. Each
//! comes with the nodes it is edited by and the outline the brush redraws it along.

use std::f32::consts::{PI, TAU};

use crate::geom::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Line,
    Polyline,
    Arc,
    /// A chain of cubics through the stroke's bends (a plugin fits it, on the core's curve hook).
    Curve,
    Ellipse,
    Circle,
    Triangle,
    EquilateralTriangle,
    Quadrilateral,
    Rectangle,
    Square,
}

/// How a curve's anchor sits: its controls mirrored either side of it, free of each other, or
/// turned along their own spans as a corner. Smooth and broken are the same shape, so which one an
/// anchor is has to be remembered rather than read off it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Smooth,
    Broken,
    Corner,
}

/// A recognised shape: its kind and the nodes that define and edit it.
///
/// Nodes, by kind: a line's two ends; a polyline's corners; an arc's start, middle and end, then
/// the centre of the ellipse it is a stretch of and the ends of two conjugate half-axes of that; an
/// ellipse's or circle's centre and the four ends of its axes; a polygon's corners; a curve's
/// anchors, each but the last followed by the two controls of the span it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub kind: ShapeKind,
    pub nodes: Vec<Vec2>,
}

/// How straight a stroke must be (its widest gap from the chord, as a share of the chord) to be a
/// line.
const STRAIGHT: f32 = 0.06;
/// A stroke whose ends are this close (a share of its length) is closed.
const CLOSED: f32 = 0.18;
/// Simplifying a stroke into corners keeps points this far off (a share of its size).
const CORNER_TOLERANCE: f32 = 0.08;
/// Points sampled around a curved outline.
const OUTLINE_STEPS: usize = 128;
/// A held finger turns an ellipse into a circle when its axes' ratio is within this of 1, and a
/// rectangle into a square within this.
const ROUND_ENOUGH: f32 = 0.5;
const SQUARE_ENOUGH: f32 = 0.25;
/// The steps a held finger keeps a shape's turn to: 15 degrees.
pub const TURN_STEP: f32 = std::f32::consts::PI / 12.0;

impl Shape {
    /// The shape's outline, from where the stroke began, as the brush should redraw it.
    pub fn outline(&self) -> Vec<Vec2> {
        match self.kind {
            ShapeKind::Line | ShapeKind::Polyline => self.nodes.clone(),
            ShapeKind::Arc => arc_points(&self.nodes),
            ShapeKind::Curve => curve_points(&self.nodes),
            ShapeKind::Ellipse | ShapeKind::Circle => ellipse_points(&self.nodes),
            _ => {
                let mut pts = self.nodes.clone();
                if let Some(&first) = pts.first() {
                    pts.push(first);
                }
                pts
            }
        }
    }

    /// The handles shown while editing: the nodes, and for polygons the middle of each side.
    pub fn handles(&self) -> Vec<Vec2> {
        match self.kind {
            ShapeKind::Ellipse | ShapeKind::Circle => self.nodes.iter().skip(1).copied().collect(),
            ShapeKind::Arc => self.nodes.iter().take(3).copied().collect(),
            ShapeKind::Triangle
            | ShapeKind::EquilateralTriangle
            | ShapeKind::Quadrilateral
            | ShapeKind::Rectangle
            | ShapeKind::Square => {
                let n = self.nodes.len();
                let mut out = self.nodes.clone();
                for i in 0..n {
                    if let (Some(&a), Some(&b)) = (self.nodes.get(i), self.nodes.get((i + 1) % n)) {
                        out.push(a.lerp(b, 0.5));
                    }
                }
                out
            }
            _ => self.nodes.clone(),
        }
    }

    /// Moves handle `index` (as [`Shape::handles`] lists them) to `to`, keeping the shape its kind.
    pub fn move_handle(&mut self, index: usize, to: Vec2) {
        if !to.is_finite() {
            return;
        }
        match self.kind {
            ShapeKind::Ellipse | ShapeKind::Circle => self.move_axis(index, to),
            ShapeKind::Arc => self.move_arc_node(index, to),
            ShapeKind::Curve => self.move_curve_handle(index, to, true),
            ShapeKind::Rectangle | ShapeKind::Square => self.move_rect_handle(index, to),
            ShapeKind::Triangle | ShapeKind::EquilateralTriangle | ShapeKind::Quadrilateral => {
                let n = self.nodes.len();
                if index < n {
                    if let Some(p) = self.nodes.get_mut(index) {
                        *p = to;
                    }
                } else {
                    // a side's middle moves the whole side
                    let side = index - n;
                    let (Some(&a), Some(&b)) = (self.nodes.get(side), self.nodes.get((side + 1) % n)) else { return };
                    let by = to - a.lerp(b, 0.5);
                    for k in [side, (side + 1) % n] {
                        if let Some(p) = self.nodes.get_mut(k) {
                            *p = *p + by;
                        }
                    }
                }
                if self.kind == ShapeKind::EquilateralTriangle {
                    self.kind = ShapeKind::Triangle;
                }
            }
            _ => {
                if let Some(p) = self.nodes.get_mut(index) {
                    *p = to;
                }
            }
        }
    }

    /// An arc's end takes the whole arc with it, turned and sized about the other end; its middle
    /// bends it between its ends, which stay.
    fn move_arc_node(&mut self, index: usize, to: Vec2) {
        let get = |i: usize| self.nodes.get(i).copied().unwrap_or(Vec2::ZERO);
        let (start, middle, end) = (get(0), get(1), get(2));
        if index != 1 {
            let (moving, other) = if index == 0 { (start, end) } else { (end, start) };
            if to.distance(other) >= 1.0 {
                *self = self.moved_with(other, moving, to);
            }
            return;
        }
        // every point goes by as much as it stands off the line between the ends, which leaves an
        // ellipse an ellipse; a middle on that line would flatten it for good
        let across = Vec2::new(start.y - end.y, end.x - start.x).normalized_or(Vec2::ZERO);
        let (was, now) = ((middle - start).dot(across), (to - start).dot(across));
        if was.abs() < 1e-3 || now.abs() < 1.0 {
            return;
        }
        let by = to - middle;
        for p in &mut self.nodes {
            *p = *p + by * ((*p - start).dot(across) / was);
        }
    }

    /// Moves one of the curve's nodes, a control taking the one the other side of its anchor with it
    /// (mirrored, so the curve stays smooth through there) unless `mirror` is off. An anchor moves
    /// alone.
    fn move_curve_handle(&mut self, index: usize, to: Vec2, mirror: bool) {
        let Some(slot) = self.nodes.get_mut(index) else { return };
        *slot = to;
        if index.is_multiple_of(3) || !mirror {
            return;
        }
        // a control starts the span after its anchor at 3i+1, or ends the one before it at 3i-1
        let (anchor, pair) =
            if index % 3 == 1 { (index - 1, index.checked_sub(2)) } else { (index + 1, Some(index + 2)) };
        let Some(about) = self.nodes.get(anchor).copied() else { return };
        if let Some(other) = pair.and_then(|p| self.nodes.get_mut(p)) {
            *other = about * 2.0 - to;
        }
    }

    /// As [`Shape::move_handle`], but a curve's controls stay free of each other: for an anchor set
    /// broken.
    pub fn move_handle_free(&mut self, index: usize, to: Vec2) {
        if !to.is_finite() {
            return;
        }
        if self.kind == ShapeKind::Curve {
            self.move_curve_handle(index, to, false);
        } else {
            self.move_handle(index, to);
        }
    }

    /// Puts the curve's anchor at node `at` into one of its states, its controls following:
    /// broken leaves them where they are; a corner turns each along its own span, a third of the
    /// way to the next anchor; smooth sets them mirrored either side again, along the way through.
    pub fn set_anchor(&mut self, at: usize, to: Anchor) {
        if self.kind != ShapeKind::Curve || !at.is_multiple_of(3) {
            return;
        }
        let Some(&here) = self.nodes.get(at) else { return };
        let before = at.checked_sub(3).and_then(|i| self.nodes.get(i)).copied().unwrap_or(here);
        let after = self.nodes.get(at + 3).copied().unwrap_or(here);
        let (back, ahead) = match to {
            Anchor::Broken => return,
            Anchor::Corner => (
                here + (before - here).normalized_or(Vec2::ZERO) * (here.distance(before) / 3.0),
                here + (after - here).normalized_or(Vec2::ZERO) * (here.distance(after) / 3.0),
            ),
            Anchor::Smooth => {
                let was_back = at.checked_sub(1).and_then(|i| self.nodes.get(i)).copied().unwrap_or(here);
                let was_ahead = self.nodes.get(at + 1).copied().unwrap_or(here);
                let mut way = ((was_ahead - here).normalized_or(Vec2::ZERO)
                    + (here - was_back).normalized_or(Vec2::ZERO))
                .normalized_or(Vec2::ZERO);
                let mut reach = (here.distance(was_ahead) + here.distance(was_back)) * 0.5;
                if way.length() <= 0.5 || reach <= 0.5 {
                    // folded onto the anchor: along the anchors either side instead
                    way = (after - before).normalized_or(Vec2::ZERO);
                    reach = here.distance(after).max(here.distance(before)) / 3.0;
                }
                if way.length() <= 0.5 || reach <= 0.5 {
                    return;
                }
                (here - way * reach, here + way * reach)
            }
        };
        if let Some(slot) = at.checked_sub(1).and_then(|i| self.nodes.get_mut(i)) {
            *slot = back;
        }
        if let Some(slot) = self.nodes.get_mut(at + 1) {
            *slot = ahead;
        }
    }

    /// The same shape as another kind it can be shown as (an ellipse as a circle, a quadrilateral
    /// as a rectangle, ...); kinds it cannot become come back unchanged.
    pub fn as_kind(&self, kind: ShapeKind, stroke: &[Vec2]) -> Shape {
        if kind == self.kind {
            return self.clone();
        }
        match kind {
            ShapeKind::Circle => {
                let (c, a, b) = self.ellipse_frame();
                let r = (a.length() + b.length()) * 0.5;
                // along the ellipse's axes, so the outline still starts where the stroke began
                axes_shape(
                    ShapeKind::Circle,
                    c,
                    a.normalized_or(Vec2::new(1.0, 0.0)) * r,
                    b.normalized_or(Vec2::new(0.0, 1.0)) * r,
                )
            }
            ShapeKind::Ellipse => fit_ellipse(stroke),
            ShapeKind::Square | ShapeKind::Rectangle => {
                let rect = fit_rect(stroke);
                if kind == ShapeKind::Square { squared(&rect) } else { rect }
            }
            ShapeKind::Quadrilateral => polygon(stroke, 4).unwrap_or_else(|| fit_rect(stroke)),
            ShapeKind::Triangle => polygon(stroke, 3).unwrap_or_else(|| self.clone()),
            ShapeKind::EquilateralTriangle => equilateral(&polygon(stroke, 3).unwrap_or_else(|| self.clone())),
            ShapeKind::Line => line_of(stroke),
            ShapeKind::Arc => fit_arc(stroke).unwrap_or_else(|| self.clone()),
            // fitted by a plugin, never here
            ShapeKind::Curve => self.clone(),
            ShapeKind::Polyline => {
                Shape { kind: ShapeKind::Polyline, nodes: simplify(stroke, CORNER_TOLERANCE * size(stroke)) }
            }
        }
    }

    /// How far `p` is from the shape's outline.
    pub fn edge_distance(&self, p: Vec2) -> f32 {
        self.outline()
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(distance_to_segment(p, *a, *b)),
                _ => None,
            })
            .fold(f32::INFINITY, f32::min)
    }

    /// Whether a drag on its outline turns and resizes it while it is edited; lines and
    /// polylines are only moved.
    pub fn turns_from_edge(&self) -> bool {
        !matches!(self.kind, ShapeKind::Line | ShapeKind::Polyline)
    }

    /// The shape moved by `by`.
    pub fn translated(&self, by: Vec2) -> Shape {
        Shape { kind: self.kind, nodes: self.nodes.iter().map(|&p| p + by).collect() }
    }

    /// What a shape still held turns and grows about: an open shape's start, a closed shape's
    /// centre.
    pub fn pivot(&self) -> Vec2 {
        match self.kind {
            ShapeKind::Line | ShapeKind::Polyline | ShapeKind::Arc | ShapeKind::Curve => {
                self.nodes.first().copied().unwrap_or(Vec2::ZERO)
            }
            ShapeKind::Ellipse | ShapeKind::Circle => self.ellipse_frame().0,
            _ => centroid(&self.nodes),
        }
    }

    /// The shape turned and scaled about `pivot` so that the point `from` lands on `to`, as the
    /// pen still down after a snap turns and resizes it.
    pub fn moved_with(&self, pivot: Vec2, from: Vec2, to: Vec2) -> Shape {
        let (f, t) = (from - pivot, to - pivot);
        let len2 = f.dot(f);
        if !t.is_finite() || len2 < 1.0 {
            return self.clone();
        }
        // t / f as complex numbers: the turn and the scale in one
        let (re, im) = (t.dot(f) / len2, f.cross(t) / len2);
        let nodes = self
            .nodes
            .iter()
            .map(|&p| {
                let d = p - pivot;
                pivot + Vec2::new(d.x * re - d.y * im, d.x * im + d.y * re)
            })
            .collect();
        Shape { kind: self.kind, nodes }
    }

    /// The kind a finger down makes of a shape still held, from the shape as it snapped (the
    /// `stroke` it was drawn from): an ellipse whose axes are within half of each other a circle,
    /// a triangle equilateral, a quadrilateral a rectangle, and a rectangle whose sides are within
    /// a quarter of each other a square. Other kinds stay as they are; the finger also keeps the
    /// turn to whole steps (see [`Shape::turned_to_step`]).
    pub fn snapped(&self, stroke: &[Vec2]) -> Shape {
        match self.kind {
            ShapeKind::Ellipse => {
                let (_, a, b) = self.ellipse_frame();
                if b.length() > 0.0 && (1.0 - a.length() / b.length()).abs() < ROUND_ENOUGH {
                    self.as_kind(ShapeKind::Circle, stroke)
                } else {
                    self.clone()
                }
            }
            ShapeKind::Triangle => equilateral(self),
            ShapeKind::Quadrilateral | ShapeKind::Rectangle => {
                let rect = self.as_kind(ShapeKind::Rectangle, if stroke.is_empty() { &self.nodes } else { stroke });
                let get = |i: usize| rect.nodes.get(i).copied().unwrap_or(Vec2::ZERO);
                let (w, h) = (get(0).distance(get(1)), get(0).distance(get(3)));
                if h > 0.0 && (1.0 - w / h).abs() < SQUARE_ENOUGH { squared(&rect) } else { rect }
            }
            _ => self.clone(),
        }
    }

    /// The point whose direction from the pivot a finger keeps to whole steps: an open shape's
    /// end, an ellipse's first axis end, a polygon's first corner.
    fn reference(&self) -> Vec2 {
        match self.kind {
            ShapeKind::Line | ShapeKind::Polyline | ShapeKind::Curve => self.nodes.last(),
            ShapeKind::Arc => self.nodes.get(2),
            ShapeKind::Ellipse | ShapeKind::Circle => self.nodes.get(1),
            _ => self.nodes.first(),
        }
        .copied()
        .unwrap_or(Vec2::ZERO)
    }

    /// The shape turned about its pivot to the nearest whole step of [`TURN_STEP`]: a line's
    /// angle, an ellipse's axes, a polygon's first corner seen from its centre.
    pub fn turned_to_step(&self) -> Shape {
        let pivot = self.pivot();
        let from = self.reference();
        let d = from - pivot;
        if d.length() < 1.0 {
            return self.clone();
        }
        let angle = (d.y.atan2(d.x) / TURN_STEP).round() * TURN_STEP;
        self.moved_with(pivot, from, pivot + Vec2::new(angle.cos(), angle.sin()) * d.length())
    }

    /// An ellipse's centre and its two half-axes.
    fn ellipse_frame(&self) -> (Vec2, Vec2, Vec2) {
        let get = |i: usize| self.nodes.get(i).copied().unwrap_or(Vec2::ZERO);
        let c = get(0);
        (c, get(1) - c, get(2) - c)
    }

    fn move_axis(&mut self, index: usize, to: Vec2) {
        let (c, a, b) = self.ellipse_frame();
        let along = to - c;
        if self.kind == ShapeKind::Circle {
            let r = along.length();
            *self = axes_shape(ShapeKind::Circle, c, Vec2::new(r, 0.0), Vec2::new(0.0, r));
            return;
        }
        // handles 0 and 2 are the first axis's ends, 1 and 3 the second's: the dragged one turns
        // the ellipse and sets its length, the other keeps its length at a right angle
        let (first, second) = if index.is_multiple_of(2) {
            let len = along.length().max(1.0);
            let sign = if index == 2 { -1.0 } else { 1.0 };
            let u = along * (sign / len);
            (u * len, Vec2::new(-u.y, u.x) * b.length())
        } else {
            let len = along.length().max(1.0);
            let sign = if index == 3 { -1.0 } else { 1.0 };
            let v = along * (sign / len);
            (Vec2::new(v.y, -v.x) * a.length(), v * len)
        };
        *self = axes_shape(ShapeKind::Ellipse, c, first, second);
    }

    fn move_rect_handle(&mut self, index: usize, to: Vec2) {
        let get = |i: usize| self.nodes.get(i).copied().unwrap_or(Vec2::ZERO);
        let (p0, p1, p3) = (get(0), get(1), get(3));
        let u = (p1 - p0).normalized_or(Vec2::new(1.0, 0.0));
        let v = (p3 - p0).normalized_or(Vec2::new(0.0, 1.0));
        // the rectangle in its own frame: corner 0 at the origin, sides along u and v
        let (mut x0, mut y0, mut x1, mut y1) = (0.0f32, 0.0f32, (p1 - p0).dot(u), (p3 - p0).dot(v));
        let local = Vec2::new((to - p0).dot(u), (to - p0).dot(v));
        match index {
            0 => (x0, y0) = (local.x, local.y),
            1 => (x1, y0) = (local.x, local.y),
            2 => (x1, y1) = (local.x, local.y),
            3 => (x0, y1) = (local.x, local.y),
            4 => y0 = local.y,
            5 => x1 = local.x,
            6 => y1 = local.y,
            _ => x0 = local.x,
        }
        if self.kind == ShapeKind::Square {
            // a square keeps its sides equal: the larger move wins
            let side = (x1 - x0).abs().max((y1 - y0).abs());
            if index == 0 || index == 3 || index == 7 {
                x0 = x1 - side * (x1 - x0).signum();
            } else {
                x1 = x0 + side * (x1 - x0).signum();
            }
            if index == 0 || index == 1 || index == 4 {
                y0 = y1 - side * (y1 - y0).signum();
            } else {
                y1 = y0 + side * (y1 - y0).signum();
            }
        }
        let at = |x: f32, y: f32| p0 + u * x + v * y;
        self.nodes = vec![at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
    }
}

/// The shapes a held stroke (canvas points, in drawing order) could be, best first; empty if it is
/// too short to be anything.
/// `stroke` as a shape is fitted to, without a hook at its end: over its last stretch, from the
/// last point farther than `reach` from its end, the line should only come closer to that end;
/// where it first turns away again (a flick as the pen lifts), the rest is left off.
pub fn without_end_hook(stroke: &[Vec2], reach: f32) -> &[Vec2] {
    let n = stroke.len();
    let Some(&end) = stroke.last() else { return stroke };
    if n < 4 || !reach.is_finite() {
        return stroke;
    }
    let away = |i: usize| stroke.get(i).map_or(0.0, |p| p.distance(end));
    // the last point out of reach (at least the third)
    let from = (3..n).rev().find(|&i| away(i) > reach).unwrap_or(2);
    let mut before = away(from);
    for (i, p) in stroke.iter().enumerate().skip(from + 1) {
        let now = p.distance(end);
        if now > before {
            return stroke.get(..i).unwrap_or(stroke);
        }
        before = now;
    }
    stroke
}

/// How near its end a stroke's hook is looked for ([`without_end_hook`]): a quarter of the
/// stroke's box across, and no more than `most` (canvas pixels).
pub fn end_hook_reach(stroke: &[Vec2], most: f32) -> f32 {
    let (lo, hi) = bounds(stroke);
    (lo.distance(hi) / 4.0).min(most)
}

pub fn recognise(stroke: &[Vec2]) -> Vec<Shape> {
    let pts: Vec<Vec2> = stroke.iter().copied().filter(|p| p.is_finite()).collect();
    let length = path_length(&pts);
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return Vec::new() };
    if pts.len() < 2 || length < 4.0 {
        return Vec::new();
    }
    let chord = first.distance(last);
    let widest = pts.iter().map(|&p| distance_to_segment(p, first, last)).fold(0.0f32, f32::max);
    if widest <= STRAIGHT * chord.max(1.0) {
        return vec![line_of(&pts)];
    }
    if chord > CLOSED * length {
        // open: corners make a polyline; an even bend an arc
        let polyline = Shape { kind: ShapeKind::Polyline, nodes: simplify(&pts, CORNER_TOLERANCE * size(&pts)) };
        let arc = fit_arc(&pts);
        let arc_error = arc.as_ref().map_or(f32::MAX, |a| outline_error(a, &pts));
        // a sharp corner counts only where the corners fit clearly better than an even bend: a
        // hand's wobble along an arc can turn as sharply between a few points, but is no corner
        let cornered = || sharp_corner(&pts, &polyline.nodes) && arc_error > 2.0 * outline_error(&polyline, &pts);
        if polyline.nodes.len() >= 3 && (arc_error > 0.04 || cornered()) {
            return vec![polyline, line_of(&pts)];
        }
        return match arc {
            Some(a) => vec![a, line_of(&pts)],
            None => vec![polyline],
        };
    }
    // closed: an ellipse against the polygon the corners make
    let ellipse = fit_ellipse(&pts);
    let ellipse_error = outline_error(&ellipse, &pts);
    let corners = closed_corners(&pts);
    let polygon_shape = match corners.len() {
        3 => Some(Shape { kind: ShapeKind::Triangle, nodes: corners }),
        4 => Some(Shape { kind: ShapeKind::Quadrilateral, nodes: corners }),
        _ => None,
    };
    let (c, a, b) = ellipse.ellipse_frame();
    let round = a.length().min(b.length()) / a.length().max(b.length()).max(1e-3);
    let mut out = Vec::new();
    match polygon_shape {
        Some(p) if outline_error(&p, &pts) < ellipse_error * 0.8 => {
            if p.kind == ShapeKind::Triangle {
                out.push(p.clone());
                out.push(equilateral(&p));
            } else {
                out.push(p);
                out.push(fit_rect(&pts));
                out.push(ellipse);
            }
        }
        Some(p) => {
            out.push(ellipse);
            if p.kind == ShapeKind::Quadrilateral {
                out.push(p);
                out.push(fit_rect(&pts));
            } else {
                out.push(p);
            }
        }
        None => {
            out.push(ellipse);
            if round > 0.75 {
                let r = (a.length() + b.length()) * 0.5;
                out.push(axes_shape(ShapeKind::Circle, c, Vec2::new(r, 0.0), Vec2::new(0.0, r)));
            }
        }
    }
    out
}

fn path_length(pts: &[Vec2]) -> f32 {
    pts.windows(2)
        .map(|w| match w {
            [a, b] => a.distance(*b),
            _ => 0.0,
        })
        .sum()
}

fn size(pts: &[Vec2]) -> f32 {
    let (lo, hi) = bounds(pts);
    (hi - lo).length().max(1.0)
}

fn bounds(pts: &[Vec2]) -> (Vec2, Vec2) {
    pts.iter().fold((Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN)), |(lo, hi), p| {
        (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)))
    })
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 <= 1e-9 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn line_of(pts: &[Vec2]) -> Shape {
    let first = pts.first().copied().unwrap_or(Vec2::ZERO);
    let last = pts.last().copied().unwrap_or(first);
    Shape { kind: ShapeKind::Line, nodes: vec![first, last] }
}

/// The corners that keep the path within `tolerance` (Ramer–Douglas–Peucker), ends included.
fn simplify(pts: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    fn keep(pts: &[Vec2], tolerance: f32, out: &mut Vec<Vec2>) {
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { return };
        let far = pts
            .iter()
            .enumerate()
            .map(|(i, &p)| (i, distance_to_segment(p, a, b)))
            .fold((0, 0.0f32), |best, x| if x.1 > best.1 { x } else { best });
        if far.1 > tolerance && far.0 > 0 && far.0 + 1 < pts.len() {
            keep(pts.get(..=far.0).unwrap_or(&[]), tolerance, out);
            out.pop();
            keep(pts.get(far.0..).unwrap_or(&[]), tolerance, out);
        } else {
            out.push(a);
            out.push(b);
        }
    }
    let mut out = Vec::new();
    keep(pts, tolerance, &mut out);
    out.dedup();
    out
}

/// A closed stroke's corners: simplified from the point farthest from its start, so the start
/// is not taken for a corner when it is not one.
fn closed_corners(pts: &[Vec2]) -> Vec<Vec2> {
    let tolerance = CORNER_TOLERANCE * size(pts);
    let Some(&start) = pts.first() else { return Vec::new() };
    let far = pts.iter().enumerate().fold((0, 0.0f32), |best, (i, p)| {
        let d = p.distance(start);
        if d > best.1 { (i, d) } else { best }
    });
    // walk once round from the far point, back to it
    let mut ring: Vec<Vec2> = pts.get(far.0..).unwrap_or(&[]).to_vec();
    ring.extend(pts.get(..=far.0).unwrap_or(&[]));
    let mut corners = simplify(&ring, tolerance);
    // the far point opens and closes the ring; drop it at the end, and anything nearly straight
    corners.pop();
    let n = corners.len();
    let kept: Vec<Vec2> = (0..n)
        .filter_map(|i| {
            let prev = corners.get((i + n - 1) % n)?;
            let p = corners.get(i)?;
            let next = corners.get((i + 1) % n)?;
            let turn = (*p - *prev).normalized_or(Vec2::ZERO).dot((*next - *p).normalized_or(Vec2::ZERO));
            (turn < 0.9).then_some(*p)
        })
        .collect();
    // in drawing order from the corner nearest the start
    let first = kept.iter().enumerate().fold((0, f32::MAX), |best, (i, p)| {
        let d = p.distance(start);
        if d < best.1 { (i, d) } else { best }
    });
    let mut ordered = kept.get(first.0..).unwrap_or(&[]).to_vec();
    ordered.extend(kept.get(..first.0).unwrap_or(&[]));
    ordered
}

/// The polygon with `n` corners the stroke makes, if it makes one.
fn polygon(pts: &[Vec2], n: usize) -> Option<Shape> {
    let corners = closed_corners(pts);
    let kind = if n == 3 { ShapeKind::Triangle } else { ShapeKind::Quadrilateral };
    (corners.len() == n).then_some(Shape { kind, nodes: corners })
}

/// The ellipse with the stroke's centre and spread: for points evenly round an ellipse, the
/// spread along each axis is half the axis squared. Its first axis points to where the stroke
/// began.
pub fn fit_ellipse(pts: &[Vec2]) -> Shape {
    let (c, weight) = pts.windows(2).fold((Vec2::ZERO, 0.0f32), |(sum, w), pair| match pair {
        [a, b] => {
            let l = a.distance(*b);
            (sum + a.lerp(*b, 0.5) * l, w + l)
        }
        _ => (sum, w),
    });
    let c = if weight > 0.0 { c * (1.0 / weight) } else { pts.first().copied().unwrap_or(Vec2::ZERO) };
    let (mut xx, mut xy, mut yy) = (0.0f32, 0.0f32, 0.0f32);
    for pair in pts.windows(2) {
        if let [a, b] = pair {
            let l = a.distance(*b);
            let d = a.lerp(*b, 0.5) - c;
            xx += d.x * d.x * l;
            xy += d.x * d.y * l;
            yy += d.y * d.y * l;
        }
    }
    let w = weight.max(1e-6);
    let (xx, xy, yy) = (xx / w, xy / w, yy / w);
    // the spread's principal axes
    let mean = (xx + yy) * 0.5;
    let gap = (((xx - yy) * 0.5).powi(2) + xy * xy).sqrt();
    let (l1, l2) = (mean + gap, (mean - gap).max(0.0));
    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    let u = Vec2::new(angle.cos(), angle.sin());
    let v = Vec2::new(-u.y, u.x);
    // the first axis points to where the stroke began, so the outline starts there
    let start = pts.first().copied().unwrap_or(c) - c;
    let (a, b) = ((2.0 * l1).sqrt(), (2.0 * l2).sqrt());
    let (ua, vb) = if start.dot(u).abs() / a.max(1e-3) >= start.dot(v).abs() / b.max(1e-3) {
        (u * a * start.dot(u).signum(), v * b)
    } else {
        (v * b * start.dot(v).signum(), u * a * -1.0)
    };
    axes_shape(ShapeKind::Ellipse, c, ua, vb)
}

fn axes_shape(kind: ShapeKind, c: Vec2, a: Vec2, b: Vec2) -> Shape {
    Shape { kind, nodes: vec![c, c + a, c + b, c - a, c - b] }
}

fn ellipse_points(nodes: &[Vec2]) -> Vec<Vec2> {
    let get = |i: usize| nodes.get(i).copied().unwrap_or(Vec2::ZERO);
    let (c, a, b) = (get(0), get(1) - get(0), get(2) - get(0));
    (0..=OUTLINE_STEPS)
        .map(|i| {
            let t = i as f32 / OUTLINE_STEPS as f32 * TAU;
            c + a * t.cos() + b * t.sin()
        })
        .collect()
}

/// The rectangle along the stroke's corners (or its spread), oriented with its sides.
fn fit_rect(pts: &[Vec2]) -> Shape {
    let corners = closed_corners(pts);
    // the sides' mean direction (folded into a quarter turn) sets the rectangle's turn
    let angle = if corners.len() >= 3 {
        let n = corners.len();
        let (s, c) = (0..n).fold((0.0f32, 0.0f32), |(s, c), i| {
            let (Some(a), Some(b)) = (corners.get(i), corners.get((i + 1) % n)) else { return (s, c) };
            let d = *b - *a;
            let t = d.y.atan2(d.x) * 4.0;
            let w = d.length();
            (s + t.sin() * w, c + t.cos() * w)
        });
        s.atan2(c) / 4.0
    } else {
        0.0
    };
    let u = Vec2::new(angle.cos(), angle.sin());
    let v = Vec2::new(-u.y, u.x);
    let source = if corners.len() >= 3 { corners } else { pts.to_vec() };
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &source {
        let (x, y) = (p.dot(u), p.dot(v));
        (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
    }
    let at = |x: f32, y: f32| u * x + v * y;
    let nodes = vec![at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
    let start = pts.first().copied().unwrap_or(Vec2::ZERO);
    Shape { kind: ShapeKind::Rectangle, nodes: from_nearest(nodes, start) }
}

/// The square with a quadrilateral's centre and mean side, along its first side and turning the
/// same way.
fn squared(quad: &Shape) -> Shape {
    let get = |i: usize| quad.nodes.get(i).copied().unwrap_or(Vec2::ZERO);
    let (p0, p1, p2, p3) = (get(0), get(1), get(2), get(3));
    let side = (p0.distance(p1) + p1.distance(p2) + p2.distance(p3) + p3.distance(p0)) * 0.25;
    let c = centroid(&[p0, p1, p2, p3]);
    let turn = if (p1 - p0).cross(p2 - p1) >= 0.0 { 1.0 } else { -1.0 };
    let u = (p1 - p0).normalized_or(Vec2::new(1.0, 0.0)) * (side * 0.5);
    let v = Vec2::new(-u.y, u.x) * turn;
    Shape { kind: ShapeKind::Square, nodes: vec![c - u - v, c + u - v, c + u + v, c - u + v] }
}

fn centroid(pts: &[Vec2]) -> Vec2 {
    pts.iter().fold(Vec2::ZERO, |a, &p| a + p) * (1.0 / pts.len().max(1) as f32)
}

fn equilateral(t: &Shape) -> Shape {
    let c = centroid(&t.nodes);
    let first = t.nodes.first().copied().unwrap_or(c) - c;
    let r = t.nodes.iter().map(|p| p.distance(c)).sum::<f32>() / t.nodes.len().max(1) as f32;
    let a0 = first.y.atan2(first.x);
    // the same turning sense as the stroke
    let turn = match (t.nodes.get(1), t.nodes.get(2)) {
        (Some(&b), Some(&d)) => {
            let a = t.nodes.first().copied().unwrap_or(c);
            if (b - a).cross(d - a) >= 0.0 { 1.0 } else { -1.0 }
        }
        _ => 1.0,
    };
    let nodes = (0..3).map(|k| {
        let a = a0 + turn * k as f32 * TAU / 3.0;
        c + Vec2::new(a.cos(), a.sin()) * r
    });
    Shape { kind: ShapeKind::EquilateralTriangle, nodes: nodes.collect() }
}

fn from_nearest(nodes: Vec<Vec2>, start: Vec2) -> Vec<Vec2> {
    let first = nodes.iter().enumerate().fold((0, f32::MAX), |best, (i, p)| {
        let d = p.distance(start);
        if d < best.1 { (i, d) } else { best }
    });
    let mut out = nodes.get(first.0..).unwrap_or(&[]).to_vec();
    out.extend(nodes.get(..first.0).unwrap_or(&[]));
    out
}

/// An ellipse: its centre and two conjugate half-axes, which trace it as `a cos t + b sin t`.
#[derive(Clone, Copy)]
struct Oval {
    centre: Vec2,
    a: Vec2,
    b: Vec2,
}

impl Oval {
    /// How far `d` goes along each half-axis.
    fn parts(self, d: Vec2) -> (f32, f32) {
        let whole = self.a.cross(self.b);
        (d.cross(self.b) / whole, self.a.cross(d) / whole)
    }

    fn at(self, t: f32) -> Vec2 {
        self.centre + self.a * t.cos() + self.b * t.sin()
    }

    /// Where the ray from the centre at `angle` meets it.
    fn at_angle(self, angle: f32) -> Vec2 {
        let way = Vec2::new(angle.cos(), angle.sin());
        let (s, t) = self.parts(way);
        self.centre + way * (1.0 / s.hypot(t))
    }
}

/// The stretch of the ellipse fitted to the stroke that the stroke swept: from the least to the
/// most of the way round the centre it got, the way it went, its middle halfway between.
fn fit_arc(pts: &[Vec2]) -> Option<Shape> {
    let oval = fit_oval(pts)?;
    let angle = |p: Vec2| (p - oval.centre).y.atan2((p - oval.centre).x);
    let first = angle(*pts.first()?);
    let (mut was, mut gone, mut least, mut most) = (first, 0.0f32, 0.0f32, 0.0f32);
    for &p in pts {
        let now = angle(p);
        gone += (now - was + PI).rem_euclid(TAU) - PI;
        least = least.min(gone);
        most = most.max(gone);
        was = now;
    }
    if most - least < 1e-4 {
        return None;
    }
    let (from, to) = if gone < 0.0 { (first + most, first + least) } else { (first + least, first + most) };
    let nodes = vec![
        oval.at_angle(from),
        oval.at_angle((from + to) * 0.5),
        oval.at_angle(to),
        oval.centre,
        oval.centre + oval.a,
        oval.centre + oval.b,
    ];
    nodes.iter().all(|p| p.is_finite()).then_some(Shape { kind: ShapeKind::Arc, nodes })
}

/// `shapes` with the curve through `stroke`'s bends first, where the first is a polyline (as the
/// QuickShape curves plugin offers one).
#[cfg(test)]
pub fn with_curve(mut shapes: Vec<Shape>, stroke: &[Vec2]) -> Vec<Shape> {
    if shapes.first().is_some_and(|s| s.kind == ShapeKind::Polyline) {
        let pts: Vec<Vec2> = stroke.iter().copied().filter(|p| p.is_finite()).collect();
        shapes.insert(0, fit_curve(&pts));
    }
    shapes
}

/// The curve through the stroke's bends: a chain of cubics meeting at its corners, each anchor's
/// controls leaning the way the stroke came and went, so the joins are smooth. (A plugin fits
/// curves; this one is for the tests of reshaping them.)
#[cfg(test)]
fn fit_curve(pts: &[Vec2]) -> Shape {
    let mut anchors = simplify(pts, CORNER_TOLERANCE * size(pts));
    if anchors.len() < 2 {
        let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else {
            return Shape { kind: ShapeKind::Curve, nodes: Vec::new() };
        };
        anchors = vec![first, last];
    }
    let n = anchors.len();
    let mut nodes = Vec::with_capacity(n * 3);
    for i in 0..n {
        let Some(&here) = anchors.get(i) else { continue };
        let before = i.checked_sub(1).and_then(|k| anchors.get(k)).copied().unwrap_or(here);
        let after = if i + 1 == n { here } else { anchors.get(i + 1).copied().unwrap_or(here) };
        nodes.push(here);
        if i + 1 < n {
            let beyond = anchors.get(i + 2).copied().unwrap_or(after);
            nodes.push(here + (after - before) * (1.0 / 6.0));
            nodes.push(after - (beyond - here) * (1.0 / 6.0));
        }
    }
    Shape { kind: ShapeKind::Curve, nodes }
}

/// The curve's chain of cubics, sampled span by span.
fn curve_points(nodes: &[Vec2]) -> Vec<Vec2> {
    let mut out = Vec::new();
    for span in 0..nodes.len().saturating_sub(1) / 3 {
        let base = span * 3;
        let (Some(&a), Some(&b), Some(&c), Some(&d)) =
            (nodes.get(base), nodes.get(base + 1), nodes.get(base + 2), nodes.get(base + 3))
        else {
            continue;
        };
        for k in 0..OUTLINE_STEPS {
            out.push(cubic(a, b, c, d, k as f32 / OUTLINE_STEPS as f32));
        }
    }
    if let Some(&end) = nodes.last() {
        out.push(end);
    }
    out
}

/// The point `t` of the way along the cubic from `a` to `d` through the controls `b` and `c`.
fn cubic(a: Vec2, b: Vec2, c: Vec2, d: Vec2, t: f32) -> Vec2 {
    let keep = 1.0 - t;
    a * (keep * keep * keep) + b * (3.0 * keep * keep * t) + c * (3.0 * keep * t * t) + d * (t * t * t)
}

fn arc_points(nodes: &[Vec2]) -> Vec<Vec2> {
    let get = |i: usize| nodes.get(i).copied().unwrap_or(Vec2::ZERO);
    let (start, middle, end, centre) = (get(0), get(1), get(2), get(3));
    let oval = Oval { centre, a: get(4) - centre, b: get(5) - centre };
    if oval.a.cross(oval.b).abs() < 1e-6 {
        return vec![start, end];
    }
    let place = |p: Vec2| {
        let (s, t) = oval.parts(p - centre);
        t.atan2(s)
    };
    let (from, by, to) = (place(start), place(middle), place(end));
    // the sweep from start to end that passes the middle
    let norm = |t: f32| t.rem_euclid(TAU);
    let ccw = norm(by - from) <= norm(to - from);
    let sweep = if ccw { norm(to - from) } else { -norm(from - to) };
    (0..=OUTLINE_STEPS).map(|i| oval.at(from + sweep * i as f32 / OUTLINE_STEPS as f32)).collect()
}

type Rows = [[f64; 3]; 3];

fn dot3([a, b, c]: [f64; 3], [x, y, z]: [f64; 3]) -> f64 {
    a * x + b * y + c * z
}

fn cross3([a, b, c]: [f64; 3], [x, y, z]: [f64; 3]) -> [f64; 3] {
    [b * z - c * y, c * x - a * z, a * y - b * x]
}

fn transposed([[a, b, c], [d, e, f], [g, h, i]]: Rows) -> Rows {
    [[a, d, g], [b, e, h], [c, f, i]]
}

fn product(m: Rows, n: Rows) -> Rows {
    let columns = transposed(n);
    m.map(|row| columns.map(|column| dot3(row, column)))
}

fn inverse([r, s, t]: Rows) -> Option<Rows> {
    let det = dot3(r, cross3(s, t));
    if det.abs() < 1e-12 {
        return None;
    }
    Some(transposed([cross3(s, t), cross3(t, r), cross3(r, s)].map(|row| row.map(|x| x / det))))
}

/// The eigenvalues of `m`, whose are all real.
fn eigenvalues(m: Rows) -> [f64; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let trace = a + e + i;
    let minors = a * e - b * d + a * i - c * g + e * i - f * h;
    let det = dot3([a, b, c], cross3([d, e, f], [g, h, i]));
    // the characteristic cubic as t^3 + p t + q, at t = the eigenvalue less a third of the trace
    let p = minors - trace * trace / 3.0;
    let q = -2.0 * trace.powi(3) / 27.0 + trace * minors / 3.0 - det;
    if p >= 0.0 {
        return [trace / 3.0; 3];
    }
    let r = 2.0 * (-p / 3.0).sqrt();
    let turn = (3.0 * q / (p * r)).clamp(-1.0, 1.0).acos() / 3.0;
    [0.0, 1.0, 2.0].map(|k: f64| trace / 3.0 + r * (turn - k * std::f64::consts::TAU / 3.0).cos())
}

/// The direction `m` only scales by `value`: square to all the rows of `m` less that.
fn eigenvector(m: Rows, value: f64) -> [f64; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let (r, s, t) = ([a - value, b, c], [d, e - value, f], [g, h, i - value]);
    [cross3(r, s), cross3(s, t), cross3(t, r)]
        .into_iter()
        .fold([0.0; 3], |best, v| if dot3(v, v) > dot3(best, best) { v } else { best })
}

/// The ellipse nearest `pts`: the least squares of a conic, held to being an ellipse (the direct
/// fit of Fitzgibbon, Pilu and Fisher, in Halir and Flusser's steadier form). It works in the
/// stroke's box, so a thin stroke fits as well as a round one, and holds for part of an ellipse.
fn fit_oval(pts: &[Vec2]) -> Option<Oval> {
    let (lo, hi) = bounds(pts);
    let (hx, hy) = (f64::from(hi.x - lo.x) / 2.0, f64::from(hi.y - lo.y) / 2.0);
    if pts.len() < 5 || hx.min(hy) < 0.001 {
        return None;
    }
    let count = pts.len() as f64;
    let mx = pts.iter().map(|p| f64::from(p.x)).sum::<f64>() / count;
    let my = pts.iter().map(|p| f64::from(p.y)).sum::<f64>() / count;
    // the sums of each point's second-order terms and first-order terms, against each other
    let add = |sum: &mut Rows, u: [f64; 3], v: [f64; 3]| {
        for (row, a) in sum.iter_mut().zip(u) {
            for (cell, b) in row.iter_mut().zip(v) {
                *cell += a * b;
            }
        }
    };
    let (mut high, mut both, mut low) = ([[0.0; 3]; 3], [[0.0; 3]; 3], [[0.0; 3]; 3]);
    for p in pts {
        let (x, y) = ((f64::from(p.x) - mx) / hx, (f64::from(p.y) - my) / hy);
        let (second, first) = ([x * x, x * y, y * y], [x, y, 1.0]);
        add(&mut high, second, second);
        add(&mut both, second, first);
        add(&mut low, first, first);
    }
    // the conic's first-order part follows from its second-order part, which leaves three terms
    let to_low = product(inverse(low)?, transposed(both)).map(|row| row.map(|x| -x));
    let mut left = high;
    for (row, taken) in left.iter_mut().zip(product(both, to_low)) {
        for (cell, x) in row.iter_mut().zip(taken) {
            *cell += x;
        }
    }
    let [r, s, t] = left;
    let m = [t.map(|x| x / 2.0), s.map(|x| -x), r.map(|x| x / 2.0)];
    // of its three solutions, the one ellipse
    let roundness = |[a, b, c]: [f64; 3]| (4.0 * a * c - b * b) / (a * a + b * b + c * c);
    let conic = eigenvalues(m)
        .map(|value| eigenvector(m, value))
        .into_iter()
        .filter(|&v| roundness(v).is_finite())
        .max_by(|&u, &v| roundness(u).total_cmp(&roundness(v)))
        .filter(|&v| roundness(v) > 0.0)?;
    let [a, b, c] = conic;
    let [d, e, f] = to_low.map(|row| dot3(row, conic));
    // its centre and axes in the box; stretched back out of that, the axes stay conjugate
    let det = 4.0 * a * c - b * b;
    let (cx, cy) = ((b * e - 2.0 * c * d) / det, (b * d - 2.0 * a * e) / det);
    let level = -(f + (d * cx + e * cy) / 2.0);
    let (sin, cos) = (0.5 * b.atan2(a - c)).sin_cos();
    let along = a * cos * cos + b * sin * cos + c * sin * sin;
    let across = a * sin * sin - b * sin * cos + c * cos * cos;
    let (ra, rb) = ((level / along).sqrt(), (level / across).sqrt());
    let out = |x: f64, y: f64| Vec2::new((hx * x) as f32, (hy * y) as f32);
    let oval = Oval {
        centre: Vec2::new((mx + hx * cx) as f32, (my + hy * cy) as f32),
        a: out(cos * ra, sin * ra),
        b: out(-sin * rb, cos * rb),
    };
    (oval.centre.is_finite() && oval.a.is_finite() && oval.b.is_finite()).then_some(oval)
}

/// Whether the stroke turns sharply at any of the corners it simplifies to: its direction just
/// before a corner and just after differ by more than a corner's worth.
fn sharp_corner(pts: &[Vec2], corners: &[Vec2]) -> bool {
    let reach = path_length(pts) * 0.05;
    corners.iter().skip(1).take(corners.len().saturating_sub(2)).any(|&c| {
        let Some(at) = pts.iter().position(|&p| p == c) else { return false };
        let walk = |forward: bool| {
            let mut gone = 0.0;
            let mut i = at;
            loop {
                let next = if forward { i + 1 } else { i.wrapping_sub(1) };
                let (Some(&a), Some(&b)) = (pts.get(i), pts.get(next)) else { return pts.get(i).copied() };
                gone += a.distance(b);
                i = next;
                if gone >= reach {
                    return Some(b);
                }
            }
        };
        let (Some(before), Some(after)) = (walk(false), walk(true)) else { return false };
        let turn = (c - before).normalized_or(Vec2::ZERO).dot((after - c).normalized_or(Vec2::ZERO));
        // more than about forty degrees
        turn < 0.77
    })
}

/// How far the stroke strays from a shape's outline, on average, as a share of the stroke's size.
fn outline_error(shape: &Shape, pts: &[Vec2]) -> f32 {
    let outline = shape.outline();
    let near = |p: Vec2| {
        outline
            .windows(2)
            .map(|w| match w {
                [a, b] => distance_to_segment(p, *a, *b),
                _ => f32::MAX,
            })
            .fold(f32::MAX, f32::min)
    };
    pts.iter().map(|&p| near(p)).sum::<f32>() / pts.len().max(1) as f32 / size(pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stroke_bending_twice_can_be_a_curve_through_its_bends() {
        // across, back and across again
        let corners = [Vec2::new(0.0, 0.0), Vec2::new(200.0, 10.0), Vec2::new(10.0, 90.0), Vec2::new(200.0, 160.0)];
        let stroke: Vec<Vec2> =
            corners.windows(2).flat_map(|w| (0..25).map(move |i| w[0].lerp(w[1], i as f32 / 25.0))).collect();
        let found = recognise(&stroke);
        assert_eq!(found[0].kind, ShapeKind::Polyline);
        let with = with_curve(found.clone(), &stroke);
        assert_eq!(with[0].kind, ShapeKind::Curve);
        assert_eq!(with[1..], found[..], "the polyline and the rest still there");
        let curve = &with[0];
        // anchors at 0, 3, 6 ...: each span's two controls between them
        assert_eq!(curve.nodes.len() % 3, 1, "{:?}", curve.nodes);
        let outline = curve.outline();
        assert_eq!(outline.first(), curve.nodes.first());
        assert_eq!(outline.last(), curve.nodes.last());
        // a control dragged takes its pair the other way; set broken it goes alone
        let mut moved = curve.clone();
        moved.move_handle(2, Vec2::new(50.0, 50.0));
        let (anchor, pair) = (moved.nodes[3], moved.nodes[4]);
        assert!((anchor * 2.0 - Vec2::new(50.0, 50.0)).distance(pair) < 1e-3);
        let mut free = curve.clone();
        free.move_handle_free(2, Vec2::new(50.0, 50.0));
        assert_eq!(free.nodes[4], curve.nodes[4]);
        // a corner: its controls along its own spans
        let mut corner = curve.clone();
        corner.set_anchor(3, Anchor::Corner);
        let toward =
            |a: Vec2, b: Vec2, c: Vec2| (b - a).normalized_or(Vec2::ZERO).distance((c - a).normalized_or(Vec2::ZERO));
        assert!(toward(corner.nodes[3], corner.nodes[2], corner.nodes[0]) < 1e-3);
        assert!(toward(corner.nodes[3], corner.nodes[4], corner.nodes[6]) < 1e-3);
        // a stroke that is no polyline is left as it was
        let line = vec![Vec2::ZERO, Vec2::new(100.0, 1.0)];
        assert_eq!(with_curve(recognise(&line), &line), recognise(&line));
    }

    fn wobbly_circle(r: f32, squash: f32, n: usize) -> Vec<Vec2> {
        (0..=n)
            .map(|i| {
                let t = i as f32 / n as f32 * TAU;
                Vec2::new(200.0 + r * t.cos() + 3.0 * (5.0 * t).sin(), 200.0 + r * squash * t.sin())
            })
            .collect()
    }

    fn polygon_path(corners: &[Vec2], per_side: usize) -> Vec<Vec2> {
        let mut out = Vec::new();
        for i in 0..corners.len() {
            let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
            for k in 0..per_side {
                out.push(a.lerp(b, k as f32 / per_side as f32) + Vec2::new(0.0, if k % 2 == 0 { 1.0 } else { -1.0 }));
            }
        }
        out.push(corners[0]);
        out
    }

    #[test]
    fn a_nearly_straight_stroke_is_a_line() {
        let pts: Vec<Vec2> =
            (0..40).map(|i| Vec2::new(i as f32 * 8.0, 100.0 + if i % 2 == 0 { 2.0 } else { -2.0 })).collect();
        let found = recognise(&pts);
        assert_eq!(found[0].kind, ShapeKind::Line);
        assert_eq!(found[0].nodes, vec![pts[0], pts[39]]);
    }

    #[test]
    fn closed_round_strokes_are_ellipses_and_circles() {
        let found = recognise(&wobbly_circle(100.0, 0.6, 80));
        assert_eq!(found[0].kind, ShapeKind::Ellipse);
        let (c, a, b) = found[0].ellipse_frame();
        assert!(c.distance(Vec2::new(200.0, 200.0)) < 5.0, "{c:?}");
        assert!((a.length().max(b.length()) - 100.0).abs() < 8.0 && (a.length().min(b.length()) - 60.0).abs() < 8.0);
        let round = recognise(&wobbly_circle(100.0, 1.0, 80));
        assert!(round.iter().any(|s| s.kind == ShapeKind::Circle));
        // the outline starts where the stroke did
        assert!(found[0].outline()[0].distance(Vec2::new(303.0, 200.0)) < 12.0);
    }

    #[test]
    fn closed_cornered_strokes_are_polygons() {
        let rect = [Vec2::new(100.0, 100.0), Vec2::new(400.0, 105.0), Vec2::new(395.0, 300.0), Vec2::new(105.0, 295.0)];
        let found = recognise(&polygon_path(&rect, 20));
        assert_eq!(found[0].kind, ShapeKind::Quadrilateral, "{found:?}");
        assert!(found.iter().any(|s| s.kind == ShapeKind::Rectangle));
        let tri = [Vec2::new(200.0, 50.0), Vec2::new(350.0, 300.0), Vec2::new(50.0, 300.0)];
        let found = recognise(&polygon_path(&tri, 25));
        assert_eq!(found[0].kind, ShapeKind::Triangle, "{found:?}");
        assert_eq!(found[1].kind, ShapeKind::EquilateralTriangle);
    }

    /// A bow far flatter than a circle.
    fn flat_bow() -> Vec<Vec2> {
        (0..=60)
            .map(|i| {
                let t = 0.4 + i as f32 / 60.0 * 2.3;
                Vec2::new(500.0 + 400.0 * t.cos(), 300.0 + 90.0 * t.sin())
            })
            .collect()
    }

    #[test]
    fn an_arc_is_the_stretch_of_an_ellipse_the_stroke_swept() {
        let bow = flat_bow();
        let arc = &recognise(&bow)[0];
        assert_eq!(arc.kind, ShapeKind::Arc);
        let stray = bow.iter().map(|&p| arc.edge_distance(p)).fold(0.0f32, f32::max);
        assert!(stray < 1.0, "strays {stray}");
        assert!(arc.nodes[0].distance(bow[0]) < 1.0 && arc.nodes[2].distance(bow[60]) < 1.0, "{arc:?}");
        assert_eq!(arc.handles().len(), 3);
        // drawn the other way, it starts from the other end
        let back: Vec<Vec2> = bow.iter().rev().copied().collect();
        let arc = &recognise(&back)[0];
        assert!(arc.nodes[0].distance(bow[60]) < 1.0 && arc.nodes[2].distance(bow[0]) < 1.0, "{arc:?}");
        // turned, and with a hand's wobble, it still follows
        let turned: Vec<Vec2> = bow
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let wobble = 2.0 * (((i * 37 % 11) as f32 / 5.0) - 1.0);
                Vec2::new(0.6 * p.x - 0.8 * p.y, 0.8 * p.x + 0.6 * p.y + wobble)
            })
            .collect();
        let arc = &recognise(&turned)[0];
        assert_eq!(arc.kind, ShapeKind::Arc);
        let stray = turned.iter().map(|&p| arc.edge_distance(p)).fold(0.0f32, f32::max);
        assert!(stray < 6.0, "strays {stray}");
    }

    #[test]
    fn a_hook_drawn_by_hand_is_a_curve_not_a_polyline() {
        // a hand's hook: over the top, down the right side and away, far from a circle's bend
        let knots = [
            (270.0, 595.0),
            (285.0, 550.0),
            (330.0, 535.0),
            (380.0, 545.0),
            (400.0, 610.0),
            (385.0, 700.0),
            (340.0, 770.0),
            (275.0, 820.0),
        ];
        let at = |i: usize| {
            let (x, y) = knots[i.min(knots.len() - 1)];
            Vec2::new(x, y)
        };
        // through the knots, as a hand goes
        let hook: Vec<Vec2> = (0..knots.len() - 1)
            .flat_map(|i| {
                let (a, b, c, d) = (at(i.saturating_sub(1)), at(i), at(i + 1), at(i + 2));
                (0..12).map(move |k| {
                    let t = k as f32 / 12.0;
                    let (t2, t3) = (t * t, t * t * t);
                    let spline = |p: f32, q: f32, r: f32, s: f32| {
                        0.5 * (2.0 * q
                            + (r - p) * t
                            + (2.0 * p - 5.0 * q + 4.0 * r - s) * t2
                            + (3.0 * q - p - 3.0 * r + s) * t3)
                    };
                    Vec2::new(spline(a.x, b.x, c.x, d.x), spline(a.y, b.y, c.y, d.y))
                })
            })
            .collect();
        let shape = &recognise(&hook)[0];
        assert_eq!(shape.kind, ShapeKind::Arc);
    }

    #[test]
    fn an_arc_goes_with_its_ends_and_bends_by_its_middle() {
        let arc = recognise(&flat_bow())[0].clone();
        let (start, middle, end) = (arc.nodes[0], arc.nodes[1], arc.nodes[2]);
        let to = start + Vec2::new(0.0, 500.0);
        let mut moved = arc.clone();
        moved.move_handle(2, to);
        assert!(moved.nodes[0].distance(start) < 1e-2 && moved.nodes[2].distance(to) < 1e-2, "{moved:?}");
        let scale = 500.0 / start.distance(end);
        for (p, q) in arc.outline().iter().zip(&moved.outline()) {
            assert!((p.distance(start) * scale - q.distance(start)).abs() < 0.1);
        }
        let mut bent = arc.clone();
        bent.move_handle(1, middle + (middle - start.lerp(end, 0.5)) * 0.5);
        assert!(bent.nodes[0].distance(start) < 1e-2 && bent.nodes[2].distance(end) < 1e-2, "{bent:?}");
        let near = bent.outline().iter().map(|p| p.distance(bent.nodes[1])).fold(f32::MAX, f32::min);
        assert!(near < 1.0, "the curve passes {near} from its middle");
        // not flattened for good
        let before = bent.clone();
        bent.move_handle(1, start.lerp(end, 0.5));
        assert_eq!(bent, before);
    }

    #[test]
    fn a_line_chosen_goes_back_to_the_arc_or_polyline_it_was_offered_beside() {
        let arc: Vec<Vec2> = (0..=40)
            .map(|i| {
                let t = i as f32 / 40.0 * 2.0;
                Vec2::new(200.0 + 150.0 * t.cos(), 200.0 + 150.0 * t.sin())
            })
            .collect();
        let bend: Vec<Vec2> = (0..=40)
            .map(|i| {
                let t = i as f32;
                if i <= 20 { Vec2::new(10.0 * t, 0.0) } else { Vec2::new(200.0, 10.0 * (t - 20.0)) }
            })
            .collect();
        for (stroke, kind) in [(&arc, ShapeKind::Arc), (&bend, ShapeKind::Polyline)] {
            let found = recognise(stroke);
            assert_eq!(found.iter().map(|s| s.kind).collect::<Vec<_>>(), [kind, ShapeKind::Line]);
            let line = found[0].as_kind(ShapeKind::Line, stroke);
            assert_eq!(line.kind, ShapeKind::Line);
            let back = line.as_kind(kind, stroke);
            assert_eq!(back.kind, kind);
            assert_eq!(back.nodes, found[0].nodes);
        }
    }

    #[test]
    fn a_hook_at_the_end_is_left_off_and_a_clean_end_kept() {
        // a straight line left to right that runs on past where it ends, then comes back to it
        // as the pen lifts
        let mut line: Vec<Vec2> = (0..=50).map(|i| Vec2::new(4.0 * i as f32, 100.0)).collect();
        let clean = line.len();
        line.extend([Vec2::new(204.0, 98.0), Vec2::new(200.0, 95.0), Vec2::new(194.0, 94.0)]);
        let reach = end_hook_reach(&line, 30.0);
        assert_eq!(reach, 30.0);
        let kept = without_end_hook(&line, reach);
        assert!(kept.len() <= clean + 1 && kept.len() > clean - 10, "{} of {}", kept.len(), line.len());
        assert_eq!(recognise(kept)[0].kind, ShapeKind::Line);
        // a line that comes straight in to its end keeps all of it
        let straight: Vec<Vec2> = (0..=50).map(|i| Vec2::new(4.0 * i as f32, 100.0)).collect();
        assert_eq!(without_end_hook(&straight, 30.0).len(), straight.len());
        // a small stroke looks for a hook only near its end
        assert_eq!(end_hook_reach(&straight[..6], 30.0), 5.0);
    }

    #[test]
    fn open_strokes_are_arcs_or_polylines() {
        let arc: Vec<Vec2> = (0..=40)
            .map(|i| {
                let t = i as f32 / 40.0 * 2.0;
                Vec2::new(200.0 + 150.0 * t.cos(), 200.0 + 150.0 * t.sin())
            })
            .collect();
        assert_eq!(recognise(&arc)[0].kind, ShapeKind::Arc);
        // drawn by an unsteady hand (each point up to 9 off the bend): still an arc
        let wobbly: Vec<Vec2> = (0..=60)
            .map(|i| {
                let t = i as f32 / 60.0 * 2.1;
                let r = 300.0 + 9.0 * (((i * 37 % 11) as f32 / 5.0) - 1.0);
                Vec2::new(500.0 + r * t.cos(), 400.0 + r * t.sin())
            })
            .collect();
        assert_eq!(recognise(&wobbly)[0].kind, ShapeKind::Arc);
        // a corner of 50 degrees, wobbling the same: a polyline
        let bent = [Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(200.0 + 200.0 * 0.643, 200.0 * 0.766)];
        let mut path = Vec::new();
        for w in bent.windows(2) {
            for k in 0..30 {
                let p = w[0].lerp(w[1], k as f32 / 30.0);
                path.push(Vec2::new(p.x, p.y + 4.0 * (((k * 37 % 11) as f32 / 5.0) - 1.0)));
            }
        }
        path.push(bent[2]);
        assert_eq!(recognise(&path)[0].kind, ShapeKind::Polyline);
        let zig = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0)];
        let mut path = Vec::new();
        for w in zig.windows(2) {
            for k in 0..15 {
                path.push(w[0].lerp(w[1], k as f32 / 15.0));
            }
        }
        path.push(zig[3]);
        let found = recognise(&path);
        assert_eq!(found[0].kind, ShapeKind::Polyline);
        assert_eq!(found[0].nodes.len(), 4);
    }

    #[test]
    fn handles_edit_their_shapes() {
        let mut rect = Shape {
            kind: ShapeKind::Rectangle,
            nodes: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 50.0), Vec2::new(0.0, 50.0)],
        };
        assert_eq!(rect.handles().len(), 8);
        rect.move_handle(2, Vec2::new(120.0, 80.0));
        assert_eq!(rect.nodes[2], Vec2::new(120.0, 80.0));
        assert_eq!(rect.nodes[0], Vec2::new(0.0, 0.0));
        rect.move_handle(4, Vec2::new(60.0, -10.0));
        assert_eq!(rect.nodes[0].y, -10.0);
        let mut circle = axes_shape(ShapeKind::Circle, Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0));
        circle.move_handle(1, Vec2::new(0.0, 30.0));
        assert!((circle.nodes[1].distance(circle.nodes[0]) - 30.0).abs() < 1e-3);
        let mut ellipse = axes_shape(ShapeKind::Ellipse, Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(0.0, 20.0));
        ellipse.move_handle(0, Vec2::new(0.0, 50.0));
        let (_, a, b) = ellipse.ellipse_frame();
        assert!((a.length() - 50.0).abs() < 1e-3 && (b.length() - 20.0).abs() < 1e-3 && a.dot(b).abs() < 1e-3);
        let mut line = Shape { kind: ShapeKind::Line, nodes: vec![Vec2::ZERO, Vec2::new(10.0, 0.0)] };
        line.move_handle(1, Vec2::new(5.0, 5.0));
        assert_eq!(line.nodes[1], Vec2::new(5.0, 5.0));
        line.move_handle(9, Vec2::new(5.0, 5.0));
        line.move_handle(0, Vec2::new(f32::NAN, 0.0));
        assert_eq!(line.nodes[0], Vec2::ZERO);
    }

    #[test]
    fn a_held_shape_turns_and_grows_with_the_pen() {
        let line = Shape { kind: ShapeKind::Line, nodes: vec![Vec2::ZERO, Vec2::new(100.0, 0.0)] };
        assert_eq!(line.pivot(), Vec2::ZERO);
        let moved = line.moved_with(line.pivot(), Vec2::new(100.0, 0.0), Vec2::new(0.0, 200.0));
        assert!(
            moved.nodes[0].distance(Vec2::ZERO) < 1e-3 && moved.nodes[1].distance(Vec2::new(0.0, 200.0)) < 1e-3,
            "{moved:?}"
        );
        let ellipse = axes_shape(ShapeKind::Ellipse, Vec2::new(50.0, 50.0), Vec2::new(40.0, 0.0), Vec2::new(0.0, 20.0));
        assert_eq!(ellipse.pivot(), Vec2::new(50.0, 50.0));
        let moved = ellipse.moved_with(ellipse.pivot(), Vec2::new(90.0, 50.0), Vec2::new(50.0, 130.0));
        let (c, a, b) = moved.ellipse_frame();
        assert!(c.distance(Vec2::new(50.0, 50.0)) < 1e-3);
        assert!(a.distance(Vec2::new(0.0, 80.0)) < 1e-3 && b.distance(Vec2::new(-40.0, 0.0)) < 1e-3, "{a:?} {b:?}");
        let tri =
            Shape { kind: ShapeKind::Triangle, nodes: vec![Vec2::ZERO, Vec2::new(90.0, 0.0), Vec2::new(0.0, 90.0)] };
        assert_eq!(tri.pivot(), Vec2::new(30.0, 30.0));
        // a pen on the pivot itself leaves the shape alone
        assert_eq!(tri.moved_with(tri.pivot(), tri.pivot(), Vec2::new(5.0, 5.0)), tri);
    }

    #[test]
    fn shapes_are_moved_and_found_by_their_outline() {
        let rect = Shape {
            kind: ShapeKind::Rectangle,
            nodes: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 50.0), Vec2::new(0.0, 50.0)],
        };
        assert!((rect.edge_distance(Vec2::new(50.0, 10.0)) - 10.0).abs() < 1e-3);
        // the closing side counts too
        assert!((rect.edge_distance(Vec2::new(-4.0, 25.0)) - 4.0).abs() < 1e-3);
        let moved = rect.translated(Vec2::new(5.0, -5.0));
        assert_eq!(moved.nodes[2], Vec2::new(105.0, 45.0));
        assert!(rect.turns_from_edge());
        assert!(!Shape { kind: ShapeKind::Line, nodes: vec![Vec2::ZERO, Vec2::new(1.0, 0.0)] }.turns_from_edge());
    }

    #[test]
    fn a_finger_snaps_a_held_shape() {
        // near round: a circle of the mean radius, still starting where the ellipse did
        let ellipse = axes_shape(ShapeKind::Ellipse, Vec2::ZERO, Vec2::new(0.0, 40.0), Vec2::new(-30.0, 0.0));
        let circle = ellipse.snapped(&[]);
        assert_eq!(circle.kind, ShapeKind::Circle);
        assert!(circle.nodes[1].distance(Vec2::new(0.0, 35.0)) < 1e-3, "{circle:?}");
        // too long and thin (axes 3 to 1) stays an ellipse
        let long = axes_shape(ShapeKind::Ellipse, Vec2::ZERO, Vec2::new(60.0, 0.0), Vec2::new(0.0, 20.0));
        assert_eq!(long.snapped(&[]).kind, ShapeKind::Ellipse);
        // a nearly square quadrilateral a square; a long one a rectangle
        let square_ish = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 5.0), Vec2::new(95.0, 95.0), Vec2::new(-5.0, 90.0)];
        let quad = Shape { kind: ShapeKind::Quadrilateral, nodes: square_ish.to_vec() };
        let square = quad.snapped(&polygon_path(&square_ish, 20));
        assert_eq!(square.kind, ShapeKind::Square);
        let sides: Vec<f32> = (0..4).map(|i| square.nodes[i].distance(square.nodes[(i + 1) % 4])).collect();
        assert!(sides.iter().all(|s| (s - sides[0]).abs() < 1e-3), "{sides:?}");
        assert!((square.nodes[1] - square.nodes[0]).dot(square.nodes[3] - square.nodes[0]).abs() < 1e-2);
        let long = [Vec2::new(0.0, 0.0), Vec2::new(200.0, 5.0), Vec2::new(195.0, 95.0), Vec2::new(-5.0, 90.0)];
        let quad = Shape { kind: ShapeKind::Quadrilateral, nodes: long.to_vec() };
        assert_eq!(quad.snapped(&polygon_path(&long, 20)).kind, ShapeKind::Rectangle);
        let tri =
            Shape { kind: ShapeKind::Triangle, nodes: vec![Vec2::ZERO, Vec2::new(90.0, 0.0), Vec2::new(0.0, 90.0)] };
        let eq = tri.snapped(&[]);
        assert_eq!(eq.kind, ShapeKind::EquilateralTriangle);
        assert!((eq.nodes[0].distance(eq.nodes[1]) - eq.nodes[1].distance(eq.nodes[2])).abs() < 1e-2);
        let arc = Shape { kind: ShapeKind::Arc, nodes: vec![Vec2::ZERO, Vec2::new(50.0, 20.0), Vec2::new(100.0, 0.0)] };
        assert_eq!(arc.snapped(&[]), arc);
    }

    #[test]
    fn a_finger_keeps_the_turn_to_whole_steps() {
        // a line about its start, its length kept
        let line = Shape { kind: ShapeKind::Line, nodes: vec![Vec2::new(10.0, 10.0), Vec2::new(110.0, 30.0)] };
        let stepped = line.turned_to_step();
        let d = stepped.nodes[1] - stepped.nodes[0];
        assert_eq!(stepped.nodes[0], Vec2::new(10.0, 10.0));
        assert!((d.y.atan2(d.x).to_degrees() - 15.0).abs() < 1e-3, "{d:?}");
        assert!((d.length() - line.nodes[0].distance(line.nodes[1])).abs() < 1e-3);
        // an ellipse about its centre: its first axis on a step
        let ellipse = axes_shape(ShapeKind::Ellipse, Vec2::new(5.0, 5.0), Vec2::new(40.0, 4.0), Vec2::new(-1.0, 10.0));
        let (c, a, _) = ellipse.turned_to_step().ellipse_frame();
        assert!(c.distance(Vec2::new(5.0, 5.0)) < 1e-3);
        assert!(a.y.abs() < 1e-3 && (a.x - Vec2::new(40.0, 4.0).length()).abs() < 1e-3, "{a:?}");
        // already on a step: unchanged
        let flat = Shape { kind: ShapeKind::Line, nodes: vec![Vec2::ZERO, Vec2::new(50.0, 0.0)] };
        assert_eq!(flat.turned_to_step(), flat);
    }

    #[test]
    fn shapes_change_kind() {
        let pts = wobbly_circle(100.0, 0.6, 80);
        let e = recognise(&pts).remove(0);
        let c = e.as_kind(ShapeKind::Circle, &pts);
        assert_eq!(c.kind, ShapeKind::Circle);
        let s = e.as_kind(ShapeKind::Square, &pts);
        assert!((s.nodes[0].distance(s.nodes[1]) - s.nodes[0].distance(s.nodes[3])).abs() < 1e-3);
        assert!(recognise(&[Vec2::ZERO]).is_empty());
    }
}
