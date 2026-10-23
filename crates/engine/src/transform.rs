//! Transforming a layer's content: the box drawn around it, and how its handles, the toolbar's
//! actions and undo change where the content goes.
//!
//! The content's place is the quad its source rectangle's corners are carried to; the box is
//! usually that quad, but turning the box alone (the node below it) leaves the content where it is
//! and fits the box around it at the new angle. Every change to the box carries the content with
//! it through the map from the old box to the new one.
//!
//! A warp lays a 4 × 4 mesh (`warp.rs`) over the content; once there is one it places the content,
//! and the box is its corners.

use crate::geom::{Affine, Homography, Vec2};
use crate::warp::{self, Mesh};

/// How the box's handles change the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformMode {
    /// Corners and edges stretch the box along its own axes.
    Freeform,
    /// Corners and edges scale the box, keeping its proportions.
    Uniform,
    /// Corners move on their own; edges move both their corners.
    Distort,
    /// The content is bent through a mesh: its points, or the surface between them, are dragged.
    Warp,
}

/// What a drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// A corner node: 0 top left, 1 top right, 2 bottom right, 3 bottom left.
    Corner(u8),
    /// The node in the middle of an edge: 0 top, 1 right, 2 bottom, 3 left.
    Edge(u8),
    /// The node above the box: turns the content about the box's centre.
    Rotate,
    /// The node below the box: turns the box alone, which then fits the content again.
    TurnBox,
    /// Anywhere else: moves the content (in a warp, drags the mesh's surface under the touch).
    Move,
    /// One of a warp's 16 control points, row by row.
    Mesh(u8),
}

/// Four corners: top left, top right, bottom right, bottom left.
pub type Quad = [Vec2; 4];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    content: Quad,
    frame: Quad,
    warp: Option<Mesh>,
}

/// An open transform.
#[derive(Clone, Debug)]
pub struct Transform {
    source: Quad,
    /// The convex hull of the content's pixels, in source coordinates.
    hull: Vec<Vec2>,
    now: Placement,
    history: Vec<Placement>,
    /// The handle held, where the drag started and the placement then.
    drag: Option<(Handle, Vec2, Placement)>,
    /// Lines a moved box snaps onto besides the canvas's own (canvas pixels): other layers'
    /// edges and middles, across (x) and down (y).
    snap_lines: (Vec<f32>, Vec<f32>),
}

/// What a drag is pulled towards.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Guides {
    /// Moves keep to 15° directions once past `lock_after`, and turns to 15° steps.
    pub magnetics: bool,
    /// How far (canvas pixels) a move goes before its direction locks.
    pub lock_after: f32,
    /// A moved box's edges and centre this near (canvas pixels) the canvas's edges or centre go
    /// onto them; none for no snapping.
    pub snap: Option<f32>,
    /// The canvas's size.
    pub canvas: Vec2,
}

/// Smallest scale a handle can bring the box to.
const MIN_SCALE: f32 = 0.01;

/// The step magnetics keeps directions and turns to.
const MAGNETIC_STEP: f32 = std::f32::consts::PI / 12.0;

impl Transform {
    /// A transform of the content inside the rectangle (`x`, `y`, `width`, `height`, canvas
    /// pixels) whose pixels' convex hull is `hull`.
    pub fn new(x: f32, y: f32, width: f32, height: f32, hull: Vec<Vec2>) -> Transform {
        let source =
            [Vec2::new(x, y), Vec2::new(x + width, y), Vec2::new(x + width, y + height), Vec2::new(x, y + height)];
        let now = Placement { content: source, frame: source, warp: None };
        Transform { source, hull, now, history: Vec::new(), drag: None, snap_lines: (Vec::new(), Vec::new()) }
    }

    /// Other things a moved box snaps onto: their bounds (`x`, `y`, `width`, `height`, canvas
    /// pixels), by their edges and middles.
    pub fn set_snap_targets(&mut self, bounds: &[[f32; 4]]) {
        let ok = |v: &f32| v.is_finite();
        self.snap_lines = (
            bounds.iter().flat_map(|&[x, _, w, _]| [x, x + w / 2.0, x + w]).filter(ok).collect(),
            bounds.iter().flat_map(|&[_, y, _, h]| [y, y + h / 2.0, y + h]).filter(ok).collect(),
        );
    }

    /// The source rectangle's corners.
    pub fn source(&self) -> Quad {
        self.source
    }

    /// Where the source rectangle's corners are now.
    pub fn content(&self) -> Quad {
        self.now.content
    }

    /// The box shown around the content.
    pub fn frame(&self) -> Quad {
        self.now.frame
    }

    /// The warp's mesh, once there is one.
    pub fn warp(&self) -> Option<Mesh> {
        self.now.warp
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Lays a warp's mesh evenly over the content, unless there is one already (not a step of its
    /// own: nothing moves).
    pub fn start_warp(&mut self) {
        if self.now.warp.is_none() {
            self.now.warp = Some(warp::over_quad(self.now.content));
            self.now.frame = self.now.content;
        }
    }

    /// A drag of `handle` starts at canvas point `at`.
    pub fn grab(&mut self, handle: Handle, at: Vec2) {
        self.drag = Some((handle, at, self.now));
    }

    /// The drag reached canvas point `to`, in `mode`, pulled by `guides`. Returns whether
    /// anything moved.
    pub fn drag(&mut self, mode: TransformMode, to: Vec2, guides: &Guides) -> bool {
        let Some((handle, from, start)) = self.drag else { return false };
        if !to.is_finite() {
            return false;
        }
        let next = self.dragged(handle, mode, from, to, start, guides).unwrap_or(start);
        let moved = next != self.now;
        self.now = next;
        moved
    }

    /// The drag ends; a change is kept as an undo step.
    pub fn release(&mut self) {
        if let Some((_, _, start)) = self.drag.take()
            && start != self.now
        {
            self.history.push(start);
        }
    }

    /// The drag is abandoned (a second finger came down): the box goes back.
    pub fn cancel_drag(&mut self) {
        if let Some((_, _, start)) = self.drag.take() {
            self.now = start;
        }
    }

    /// Mirrors the content across the box's upright axis.
    pub fn flip_horizontal(&mut self) {
        let [a, b, c, d] = self.now.frame;
        self.change(|p| {
            let content = carry(p, [b, a, d, c])?;
            // a mesh mirrors by taking its columns in the other order
            let warp =
                p.warp.map(|m| std::array::from_fn(|k| m.get(k / 4 * 4 + 3 - k % 4).copied().unwrap_or_default()));
            Some(Placement { content, frame: p.frame, warp })
        });
    }

    /// Mirrors the content across the box's level axis.
    pub fn flip_vertical(&mut self) {
        let [a, b, c, d] = self.now.frame;
        self.change(|p| {
            let content = carry(p, [d, c, b, a])?;
            let warp =
                p.warp.map(|m| std::array::from_fn(|k| m.get((3 - k / 4) * 4 + k % 4).copied().unwrap_or_default()));
            Some(Placement { content, frame: p.frame, warp })
        });
    }

    /// Turns the box and content by `radians` (clockwise on screen) about the box's centre.
    pub fn rotate(&mut self, radians: f32) {
        let c = centre(self.now.frame);
        let m = Affine::translate(Vec2::ZERO - c).then(Affine::rotate(radians)).then(Affine::translate(c));
        self.change(|p| Some(map_placement(p, m)));
    }

    /// The box's width and height (canvas pixels), along its top and left edges.
    pub fn size(&self) -> (f32, f32) {
        let [a, b, _, d] = self.now.frame;
        (a.distance(b), a.distance(d))
    }

    /// How far the box is turned from upright (radians, clockwise on screen).
    pub fn angle(&self) -> f32 {
        let [a, b, ..] = self.now.frame;
        angle(b - a)
    }

    /// Makes the box `width` × `height` (canvas pixels) about its centre, turned as it is: a
    /// size typed in. A negative size mirrors the content that way.
    pub fn set_size(&mut self, width: f32, height: f32) {
        let (w, h) = self.size();
        if !(width.is_finite() && height.is_finite()) || width.abs() < 1.0 || height.abs() < 1.0 || w <= 0.0 || h <= 0.0
        {
            return;
        }
        let c = centre(self.now.frame);
        let turn = self.angle();
        let stretch = Affine { a: width.abs() / w, d: height.abs() / h, ..Affine::IDENTITY };
        let m = Affine::translate(Vec2::ZERO - c)
            .then(Affine::rotate(-turn))
            .then(stretch)
            .then(Affine::rotate(turn))
            .then(Affine::translate(c));
        self.change(|p| Some(map_placement(p, m)));
        if width < 0.0 {
            self.flip_horizontal();
        }
        if height < 0.0 {
            self.flip_vertical();
        }
    }

    /// Turns the box to `radians` from upright (clockwise on screen) about its centre: an angle
    /// typed in.
    pub fn set_angle(&mut self, radians: f32) {
        if radians.is_finite() {
            self.rotate(radians - self.angle());
        }
    }

    /// Scales the box, upright and keeping its proportions, to fit a `width` × `height` canvas,
    /// centred.
    pub fn fit(&mut self, width: f32, height: f32) {
        let [a, b, _, d] = self.now.frame;
        let (w, h) = (a.distance(b), a.distance(d));
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let k = (width / w).min(height / h);
        let (w, h) = (w * k, h * k);
        let (x, y) = ((width - w) / 2.0, (height - h) / 2.0);
        let to = [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)];
        self.change(|p| carry_all(p, to));
    }

    /// Moves the content one canvas pixel towards `toward`, along whichever axis it is further
    /// along from the box's centre.
    pub fn nudge(&mut self, toward: Vec2) {
        let d = toward - centre(self.now.frame);
        if !d.is_finite() || d == Vec2::ZERO {
            return;
        }
        let step = if d.x.abs() >= d.y.abs() { Vec2::new(d.x.signum(), 0.0) } else { Vec2::new(0.0, d.y.signum()) };
        self.change(|p| Some(map_placement(p, Affine::translate(step))));
    }

    /// Puts the content back where it started.
    pub fn reset(&mut self) {
        let source = self.source;
        self.change(|_| Some(Placement { content: source, frame: source, warp: None }));
    }

    /// Takes back the last change. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        self.drag = None;
        match self.history.pop() {
            Some(p) => {
                self.now = p;
                true
            }
            None => false,
        }
    }

    /// Applies `f` to the placement, kept as an undo step when it changes anything.
    fn change(&mut self, f: impl FnOnce(&Placement) -> Option<Placement>) {
        self.drag = None;
        let finite =
            |n: &Placement| n.content.iter().chain(&n.frame).chain(n.warp.iter().flatten()).all(|v| v.is_finite());
        if let Some(next) = f(&self.now).filter(finite)
            && next != self.now
        {
            self.history.push(self.now);
            self.now = next;
        }
    }

    fn dragged(
        &self,
        handle: Handle,
        mode: TransformMode,
        from: Vec2,
        to: Vec2,
        start: Placement,
        guides: &Guides,
    ) -> Option<Placement> {
        let f = start.frame;
        let delta = to - from;
        if mode == TransformMode::Warp
            && let Some(mesh) = start.warp
        {
            return Some(warped(mesh, handle, from, delta));
        }
        let distort = mode == TransformMode::Distort;
        let next = match handle {
            Handle::Move => {
                let mut delta = delta;
                if guides.magnetics && delta.length() > guides.lock_after {
                    // the direction keeps to the 15° step at or below it
                    let step = (angle(delta) / MAGNETIC_STEP).floor() * MAGNETIC_STEP;
                    let (sin, cos) = step.sin_cos();
                    let along = delta.x * cos + delta.y * sin;
                    delta = Vec2::new(cos, sin) * along;
                }
                if let Some(reach) = guides.snap {
                    delta = delta + snap_offset(f.map(|p| p + delta), guides.canvas, &self.snap_lines, reach);
                }
                map_placement(&start, Affine::translate(delta))
            }
            Handle::Rotate => {
                let c = centre(f);
                let mut turn = angle(to - c) - angle(from - c);
                if guides.magnetics {
                    let [a, b, ..] = f;
                    let was = angle(b - a);
                    turn = ((was + turn) / MAGNETIC_STEP).round() * MAGNETIC_STEP - was;
                }
                map_placement(
                    &start,
                    Affine::translate(Vec2::ZERO - c).then(Affine::rotate(turn)).then(Affine::translate(c)),
                )
            }
            Handle::TurnBox => {
                let c = centre(f);
                let turn = angle(to - c) - angle(from - c);
                let [a, b, ..] = f;
                let frame = self.fit_box(&start, angle(b - a) + turn)?;
                Placement { frame, ..start }
            }
            Handle::Mesh(_) => start,
            Handle::Corner(i) => {
                let i = usize::from(i % 4);
                let (corner, anchor) = (at(f, i), at(f, i + 2));
                if distort {
                    let mut frame = f;
                    set(&mut frame, i, corner + delta);
                    return carry_all(&start, frame);
                }
                let target = corner + delta;
                let m = if mode == TransformMode::Uniform {
                    scale_about(anchor, projected_scale(target, corner, anchor))
                } else {
                    // the target in the box's own axes, against the corner's
                    let basis = basis(f)?.inverse()?;
                    let (t, c) = (basis.apply(target - anchor), basis.apply(corner - anchor));
                    let sx = if c.x.abs() > 1e-6 { (t.x / c.x).max(MIN_SCALE) } else { 1.0 };
                    let sy = if c.y.abs() > 1e-6 { (t.y / c.y).max(MIN_SCALE) } else { 1.0 };
                    axes_scale_about(f, anchor, sx, sy)?
                };
                map_placement(&start, m)
            }
            Handle::Edge(k) => {
                let k = usize::from(k % 4);
                if distort {
                    let mut frame = f;
                    set(&mut frame, k, at(f, k) + delta);
                    set(&mut frame, k + 1, at(f, k + 1) + delta);
                    return carry_all(&start, frame);
                }
                let mid = at(f, k).lerp(at(f, k + 1), 0.5);
                let anchor = at(f, k + 2).lerp(at(f, k + 3), 0.5);
                let s = projected_scale(mid + delta, mid, anchor);
                let m = if mode == TransformMode::Uniform {
                    scale_about(anchor, s)
                } else if k % 2 == 0 {
                    axes_scale_about(f, anchor, 1.0, s)?
                } else {
                    axes_scale_about(f, anchor, s, 1.0)?
                };
                map_placement(&start, m)
            }
        };
        Some(next)
    }

    /// The box at `radians` that just holds the content placed as in `p`.
    fn fit_box(&self, p: &Placement, radians: f32) -> Option<Quad> {
        let to_content = Homography::quad_to_quad(self.source, p.content)?;
        let [s0, s1, _, s3] = self.source;
        let (w, h) = (s1.x - s0.x, s3.y - s0.y);
        let place = |q: Vec2| match p.warp {
            Some(m) if w > 0.0 && h > 0.0 => Some(warp::point(&m, (q.x - s0.x) / w, (q.y - s0.y) / h)),
            _ => to_content.apply(q),
        };
        let points: Vec<Vec2> = if self.hull.is_empty() {
            p.warp.map_or(p.content.to_vec(), |m| m.to_vec())
        } else {
            self.hull.iter().filter_map(|&q| place(q)).collect()
        };
        let (sin, cos) = radians.sin_cos();
        let (u, v) = (Vec2::new(cos, sin), Vec2::new(-sin, cos));
        let dot = |a: Vec2, b: Vec2| a.x * b.x + a.y * b.y;
        let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for q in &points {
            let (pu, pv) = (dot(*q, u), dot(*q, v));
            u0 = u0.min(pu);
            u1 = u1.max(pu);
            v0 = v0.min(pv);
            v1 = v1.max(pv);
        }
        (u0 <= u1 && v0 <= v1).then(|| [u * u0 + v * v0, u * u1 + v * v0, u * u1 + v * v1, u * u0 + v * v1])
    }
}

/// The convex hull of `points`, without repeats.
pub fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower = half_hull(pts.iter());
    let mut upper = half_hull(pts.iter().rev());
    // each half ends where the other starts
    lower.pop();
    upper.pop();
    lower.append(&mut upper);
    lower
}

/// One side of the hull of points sorted along x (monotone chain).
fn half_hull<'a>(points: impl Iterator<Item = &'a Vec2>) -> Vec<Vec2> {
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut chain: Vec<Vec2> = Vec::new();
    for &p in points {
        while let [.., a, b] = *chain.as_slice() {
            if cross(a, b, p) > 0.0 {
                break;
            }
            chain.pop();
        }
        chain.push(p);
    }
    chain
}

/// How far to shift `frame` so that the nearest of its left, middle and right (and top, middle and
/// bottom) lands on a line, the canvas's edges and middle or one of `others` (across, down),
/// when one is within `reach`.
fn snap_offset(frame: Quad, canvas: Vec2, others: &(Vec<f32>, Vec<f32>), reach: f32) -> Vec2 {
    let axis = |values: [f32; 4], size: f32, others: &[f32]| {
        let (lo, hi) = values.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let features = [lo, (lo + hi) / 2.0, hi];
        let lines = [0.0, size / 2.0, size];
        let mut best: Option<f32> = None;
        for f in features {
            for &l in lines.iter().chain(others) {
                let d = l - f;
                if d.abs() <= reach && best.is_none_or(|b| d.abs() < b.abs()) {
                    best = Some(d);
                }
            }
        }
        best.unwrap_or(0.0)
    };
    Vec2::new(axis(frame.map(|p| p.x), canvas.x, &others.0), axis(frame.map(|p| p.y), canvas.y, &others.1))
}

fn at(q: Quad, i: usize) -> Vec2 {
    q.get(i % 4).copied().unwrap_or_default()
}

fn set(q: &mut Quad, i: usize, v: Vec2) {
    if let Some(p) = q.get_mut(i % 4) {
        *p = v;
    }
}

fn centre(q: Quad) -> Vec2 {
    let [a, b, c, d] = q;
    (a + b + c + d) * 0.25
}

fn angle(v: Vec2) -> f32 {
    v.y.atan2(v.x)
}

/// The scale about `anchor` that brings `point` nearest `target` along the line through them.
fn projected_scale(target: Vec2, point: Vec2, anchor: Vec2) -> f32 {
    let (t, p) = (target - anchor, point - anchor);
    let len2 = p.x * p.x + p.y * p.y;
    if len2 <= 1e-12 {
        return 1.0;
    }
    ((t.x * p.x + t.y * p.y) / len2).max(MIN_SCALE)
}

fn scale_about(anchor: Vec2, s: f32) -> Affine {
    Affine::translate(Vec2::ZERO - anchor).then(Affine::scale(s)).then(Affine::translate(anchor))
}

/// The box's own axes (its top and left edges) as a linear map from unit axes.
fn basis(f: Quad) -> Option<Affine> {
    let [a, b, _, d] = f;
    let (e1, e2) = (b - a, d - a);
    let m = Affine { a: e1.x, b: e1.y, c: e2.x, d: e2.y, e: 0.0, f: 0.0 };
    (m.determinant().abs() > 1e-9).then_some(m)
}

/// Scaling by `sx` along the box's top edge and `sy` along its left edge, about `anchor`.
fn axes_scale_about(f: Quad, anchor: Vec2, sx: f32, sy: f32) -> Option<Affine> {
    let b = basis(f)?;
    let s = Affine { a: sx, d: sy, ..Affine::IDENTITY };
    Some(Affine::translate(Vec2::ZERO - anchor).then(b.inverse()?).then(s).then(b).then(Affine::translate(anchor)))
}

fn map_placement(p: &Placement, m: Affine) -> Placement {
    Placement {
        content: p.content.map(|v| m.apply(v)),
        frame: p.frame.map(|v| m.apply(v)),
        warp: p.warp.map(|w| w.map(|v| m.apply(v))),
    }
}

/// Everything carried along as the box goes from where it is to `frame`.
fn carry_all(p: &Placement, frame: Quad) -> Option<Placement> {
    let h = Homography::quad_to_quad(p.frame, frame)?;
    let content = carry(p, frame)?;
    let warp = match p.warp {
        Some(m) => {
            let mut out = m;
            for v in &mut out {
                *v = h.apply(*v)?;
            }
            Some(out)
        }
        None => None,
    };
    Some(Placement { content, frame, warp })
}

/// A drag in a warp: a control point (a corner node is its control point), or the surface under the
/// touch; off the mesh, the whole mesh moves. The box follows the mesh's corners.
fn warped(mesh: Mesh, handle: Handle, from: Vec2, delta: Vec2) -> Placement {
    let point = match handle {
        Handle::Mesh(k) => Some(usize::from(k % 16)),
        Handle::Corner(i) => [0, 3, 15, 12].get(usize::from(i % 4)).copied(),
        _ => None,
    };
    let next = match point {
        Some(k) => {
            let mut m = mesh;
            if let Some(p) = m.get_mut(k) {
                *p = *p + delta;
            }
            m
        }
        None => match warp::locate(&mesh, from) {
            Some((u, v)) => warp::drag_surface(&mesh, u, v, delta),
            None => mesh.map(|p| p + delta),
        },
    };
    let corners = [next[0], next[3], next[15], next[12]];
    Placement { content: corners, frame: corners, warp: Some(next) }
}

/// The content carried along as the box goes from where it is to `frame`.
fn carry(p: &Placement, frame: Quad) -> Option<Quad> {
    let h = Homography::quad_to_quad(p.frame, frame)?;
    let [a, b, c, d] = p.content;
    Some([h.apply(a)?, h.apply(b)?, h.apply(c)?, h.apply(d)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-3
    }

    fn quad_close(a: Quad, b: Quad) -> bool {
        a.iter().zip(&b).all(|(p, q)| close(*p, *q))
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Quad {
        [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)]
    }

    const NONE: Guides = Guides { magnetics: false, lock_after: 0.0, snap: None, canvas: Vec2::ZERO };

    fn box_100x200() -> Transform {
        Transform::new(100.0, 100.0, 100.0, 200.0, Vec::new())
    }

    #[test]
    fn moving_carries_box_and_content_and_undoes() {
        let mut t = box_100x200();
        t.grab(Handle::Move, Vec2::new(150.0, 150.0));
        assert!(t.drag(TransformMode::Uniform, Vec2::new(160.0, 130.0), &NONE));
        t.release();
        assert!(quad_close(t.content(), rect(110.0, 80.0, 100.0, 200.0)));
        assert!(quad_close(t.frame(), t.content()));
        assert!(t.can_undo());
        assert!(t.undo());
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0, 200.0)));
        assert!(!t.undo());
    }

    #[test]
    fn uniform_corners_scale_about_the_opposite_corner() {
        let mut t = box_100x200();
        t.grab(Handle::Corner(2), Vec2::new(200.0, 300.0));
        // along the diagonal: twice the size
        t.drag(TransformMode::Uniform, Vec2::new(300.0, 500.0), &NONE);
        assert!(quad_close(t.content(), rect(100.0, 100.0, 200.0, 400.0)));
        // off the diagonal: the nearest point on it
        t.drag(TransformMode::Uniform, Vec2::new(300.0, 300.0), &NONE);
        let s = (200.0 * 100.0 + 200.0 * 200.0) / (100.0 * 100.0 + 200.0 * 200.0);
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0 * s, 200.0 * s)));
    }

    #[test]
    fn uniform_edges_scale_about_the_opposite_edge() {
        let mut t = box_100x200();
        t.grab(Handle::Edge(1), Vec2::new(200.0, 200.0));
        t.drag(TransformMode::Uniform, Vec2::new(250.0, 260.0), &NONE);
        // the left edge's middle (100, 200) stays
        assert!(quad_close(t.content(), rect(100.0, 50.0, 150.0, 300.0)));
    }

    #[test]
    fn freeform_stretches_along_the_box() {
        let mut t = box_100x200();
        t.grab(Handle::Corner(2), Vec2::new(200.0, 300.0));
        t.drag(TransformMode::Freeform, Vec2::new(300.0, 320.0), &NONE);
        assert!(quad_close(t.content(), rect(100.0, 100.0, 200.0, 220.0)));
        t.release();
        t.grab(Handle::Edge(0), Vec2::new(200.0, 100.0));
        t.drag(TransformMode::Freeform, Vec2::new(500.0, 210.0), &NONE);
        assert!(quad_close(t.content(), rect(100.0, 210.0, 200.0, 110.0)));
        // and along a turned box's own axes
        let mut t = box_100x200();
        t.rotate(std::f32::consts::FRAC_PI_2);
        let [_, _, br, _] = t.frame();
        t.grab(Handle::Corner(2), br);
        t.drag(TransformMode::Freeform, br + Vec2::new(-50.0, 0.0), &NONE);
        let [a, b, _, d] = t.frame();
        assert!((a.distance(b) - 100.0).abs() < 1e-3 && (a.distance(d) - 250.0).abs() < 1e-3);
    }

    #[test]
    fn distort_moves_corners_and_edges_on_their_own() {
        let mut t = box_100x200();
        t.grab(Handle::Corner(1), Vec2::new(200.0, 100.0));
        t.drag(TransformMode::Distort, Vec2::new(230.0, 80.0), &NONE);
        let [a, b, c, d] = t.content();
        assert!(close(a, Vec2::new(100.0, 100.0)) && close(b, Vec2::new(230.0, 80.0)));
        assert!(close(c, Vec2::new(200.0, 300.0)) && close(d, Vec2::new(100.0, 300.0)));
        t.release();
        t.grab(Handle::Edge(3), Vec2::new(100.0, 200.0));
        t.drag(TransformMode::Distort, Vec2::new(90.0, 200.0), &NONE);
        let [a, _, _, d] = t.content();
        assert!(close(a, Vec2::new(90.0, 100.0)) && close(d, Vec2::new(90.0, 300.0)));
    }

    #[test]
    fn rotating_turns_about_the_centre_and_a_cancelled_drag_goes_back() {
        let mut t = box_100x200();
        t.grab(Handle::Rotate, Vec2::new(150.0, 50.0));
        // from straight above the centre (150, 200) to straight right of it: a quarter turn
        t.drag(TransformMode::Uniform, Vec2::new(400.0, 200.0), &NONE);
        let [a, ..] = t.content();
        assert!(close(a, Vec2::new(250.0, 150.0)), "{a:?}");
        t.cancel_drag();
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0, 200.0)));
        assert!(!t.can_undo());
    }

    #[test]
    fn turning_the_box_refits_it_and_leaves_the_content() {
        let hull = vec![Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0), Vec2::new(100.0, 300.0)];
        let mut t = Transform::new(100.0, 100.0, 100.0, 200.0, hull);
        // the node below the box, a quarter turn clockwise about the centre
        t.grab(Handle::TurnBox, Vec2::new(150.0, 350.0));
        t.drag(TransformMode::Uniform, Vec2::new(0.0, 200.0), &NONE);
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0, 200.0)));
        let [a, b, c, d] = t.frame();
        // a quarter turn: the box's top runs down the screen; it still just holds the triangle
        assert!(close(a, Vec2::new(200.0, 100.0)) && close(b, Vec2::new(200.0, 300.0)), "{a:?} {b:?}");
        assert!(close(c, Vec2::new(100.0, 300.0)) && close(d, Vec2::new(100.0, 100.0)), "{c:?} {d:?}");
    }

    #[test]
    fn flips_rotation_fit_and_reset() {
        let mut t = box_100x200();
        t.flip_horizontal();
        let [a, b, ..] = t.content();
        assert!(close(a, Vec2::new(200.0, 100.0)) && close(b, Vec2::new(100.0, 100.0)));
        assert!(quad_close(t.frame(), rect(100.0, 100.0, 100.0, 200.0)));
        t.flip_horizontal();
        t.flip_vertical();
        let [a, .., d] = t.content();
        assert!(close(a, Vec2::new(100.0, 300.0)) && close(d, Vec2::new(100.0, 100.0)));
        t.reset();
        t.rotate(std::f32::consts::FRAC_PI_4);
        assert!(close(centre(t.frame()), Vec2::new(150.0, 200.0)));
        let [a, b, ..] = t.frame();
        assert!((angle(b - a) - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
        t.fit(1000.0, 1000.0);
        assert!(quad_close(t.frame(), rect(250.0, 0.0, 500.0, 1000.0)));
        t.reset();
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0, 200.0)));
        // every action was a step
        let mut steps = 0;
        while t.undo() {
            steps += 1;
        }
        assert_eq!(steps, 7);
        // a reset with nothing changed is not a step
        t.reset();
        assert!(!t.can_undo());
    }

    #[test]
    fn magnetics_keep_moves_and_turns_to_15_degree_steps() {
        let magnetic = Guides { magnetics: true, lock_after: 4.0, ..NONE };
        let mut t = box_100x200();
        t.grab(Handle::Move, Vec2::new(150.0, 200.0));
        // a short wobble is free
        t.drag(TransformMode::Uniform, Vec2::new(152.0, 201.0), &magnetic);
        assert!(close(t.content()[0], Vec2::new(102.0, 101.0)));
        // 10° below level keeps to level (the step at or below it)
        let d = Vec2::new(100.0 * 10f32.to_radians().cos(), 100.0 * 10f32.to_radians().sin());
        t.drag(TransformMode::Uniform, Vec2::new(150.0, 200.0) + d, &magnetic);
        let [a, ..] = t.content();
        assert!((a.y - 100.0).abs() < 1e-3 && (a.x - (100.0 + d.x)).abs() < 1e-3, "{a:?}");
        t.cancel_drag();
        // a turn of 40° keeps to 45°
        let c = Vec2::new(150.0, 200.0);
        t.grab(Handle::Rotate, c + Vec2::new(0.0, -100.0));
        let turned = (-90f32 + 40.0).to_radians();
        t.drag(TransformMode::Uniform, c + Vec2::new(turned.cos(), turned.sin()) * 100.0, &magnetic);
        let [a, b, ..] = t.frame();
        assert!((angle(b - a).to_degrees() - 45.0).abs() < 1e-3);
    }

    #[test]
    fn snapping_pulls_a_moved_box_onto_the_canvas_lines() {
        let snap = Guides { snap: Some(10.0), canvas: Vec2::new(1000.0, 1000.0), ..NONE };
        let mut t = box_100x200();
        t.grab(Handle::Move, Vec2::new(150.0, 200.0));
        // the left edge 6 from the canvas's edge, the middle 4 from the canvas's middle: the nearer wins
        t.drag(TransformMode::Uniform, Vec2::new(56.0, 504.0), &snap);
        let [a, ..] = t.content();
        assert!(close(a, Vec2::new(0.0, 400.0)), "{a:?}");
        // out of reach: free
        t.drag(TransformMode::Uniform, Vec2::new(80.0, 470.0), &snap);
        assert!(close(t.content()[0], Vec2::new(30.0, 370.0)));
        // another layer's right edge at 333 takes the box's left edge, 3 off
        t.set_snap_targets(&[[233.0, 700.0, 100.0, 100.0]]);
        t.drag(TransformMode::Uniform, Vec2::new(380.0, 470.0), &snap);
        assert!(close(t.content()[0], Vec2::new(333.0, 370.0)), "{:?}", t.content()[0]);
    }

    #[test]
    fn a_nudge_moves_one_pixel_towards_the_tap() {
        let mut t = box_100x200();
        t.nudge(Vec2::new(500.0, 210.0));
        assert!(close(t.content()[0], Vec2::new(101.0, 100.0)));
        t.nudge(Vec2::new(151.0, 0.0));
        assert!(close(t.content()[0], Vec2::new(101.0, 99.0)));
        t.nudge(centre(t.frame()));
        assert!(t.undo() && t.undo() && !t.undo());
    }

    #[test]
    fn a_warp_bends_through_its_mesh_and_flips_and_resets() {
        let mut t = box_100x200();
        t.start_warp();
        let m = t.warp().unwrap();
        assert!(close(m[5], Vec2::new(100.0 + 100.0 / 3.0, 100.0 + 200.0 / 3.0)));
        // a corner node is its control point
        t.grab(Handle::Corner(2), Vec2::new(200.0, 300.0));
        t.drag(TransformMode::Warp, Vec2::new(230.0, 310.0), &NONE);
        t.release();
        assert!(close(t.warp().unwrap()[15], Vec2::new(230.0, 310.0)));
        assert!(close(t.frame()[2], Vec2::new(230.0, 310.0)));
        // the surface under the touch follows it
        let before = t.warp().unwrap();
        let at = warp::point(&before, 0.5, 0.5);
        t.grab(Handle::Move, at);
        t.drag(TransformMode::Warp, at + Vec2::new(20.0, 0.0), &NONE);
        assert!(close(warp::point(&t.warp().unwrap(), 0.5, 0.5), at + Vec2::new(20.0, 0.0)));
        // off the mesh: all of it moves
        t.grab(Handle::Move, Vec2::new(0.0, 0.0));
        t.drag(TransformMode::Warp, Vec2::new(5.0, 0.0), &NONE);
        assert!(close(t.warp().unwrap()[0], before[0] + Vec2::new(5.0, 0.0)));
        t.release();
        t.flip_horizontal();
        let flipped = t.warp().unwrap();
        assert!(close(flipped[0], before[3] + Vec2::new(5.0, 0.0)));
        t.rotate(std::f32::consts::FRAC_PI_2);
        t.reset();
        assert_eq!(t.warp(), None);
        assert!(t.undo() && t.warp().is_some());
    }

    #[test]
    fn hull_of_a_square_with_inner_points() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(0.0, 2.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 0.0),
        ];
        let hull = convex_hull(&pts);
        assert_eq!(hull.len(), 4, "{hull:?}");
        for corner in [Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(0.0, 2.0)] {
            assert!(hull.contains(&corner));
        }
        assert_eq!(convex_hull(&[Vec2::new(1.0, 1.0)]).len(), 1);
    }

    #[test]
    fn a_size_and_an_angle_typed_in() {
        let mut t = box_100x200();
        let c = centre(t.frame());
        t.set_size(50.0, 400.0);
        let (w, h) = t.size();
        assert!((w - 50.0).abs() < 1e-3 && (h - 400.0).abs() < 1e-3, "{w} {h}");
        assert!(close(centre(t.frame()), c));
        t.set_angle(30f32.to_radians());
        assert!((t.angle().to_degrees() - 30.0).abs() < 1e-3);
        // turned, a size keeps the turn
        t.set_size(100.0, 100.0);
        assert!((t.angle().to_degrees() - 30.0).abs() < 1e-3);
        let (w, h) = t.size();
        assert!((w - 100.0).abs() < 1e-2 && (h - 100.0).abs() < 1e-2);
        // nothing for sizes under a pixel or not numbers
        t.set_size(0.5, 10.0);
        t.set_size(f32::NAN, 10.0);
        assert!((t.size().0 - 100.0).abs() < 1e-2);
        assert!(t.undo() && t.undo() && t.undo() && !t.undo());
    }
}
