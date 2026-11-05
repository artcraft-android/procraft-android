//! QuickShape: a stroke drawn and held snaps to the shape it looks like, redrawn with the same
//! brush along the shape's outline. It stays open while it is edited (its kind changed, its
//! handles dragged) and is kept as drawn with the next thing done.

use procraft_engine::brush::Brush;
use procraft_engine::document::LayerId;
use procraft_engine::geom::Vec2;
use procraft_engine::input::Sample;
use procraft_engine::quickshape::{Shape, ShapeKind, fit_ellipse, recognise};
use procraft_engine::stroke::StrokeBuilder;
use procraft_gpu::StrokeStyle;

use crate::{CanvasSession, PointerKind, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum QuickShapeKind {
    Line,
    Polyline,
    Arc,
    Ellipse,
    Circle,
    Triangle,
    EquilateralTriangle,
    Quadrilateral,
    Rectangle,
    Square,
}

impl From<ShapeKind> for QuickShapeKind {
    fn from(k: ShapeKind) -> QuickShapeKind {
        match k {
            ShapeKind::Line => QuickShapeKind::Line,
            ShapeKind::Polyline => QuickShapeKind::Polyline,
            ShapeKind::Arc => QuickShapeKind::Arc,
            ShapeKind::Ellipse => QuickShapeKind::Ellipse,
            ShapeKind::Circle => QuickShapeKind::Circle,
            ShapeKind::Triangle => QuickShapeKind::Triangle,
            ShapeKind::EquilateralTriangle => QuickShapeKind::EquilateralTriangle,
            ShapeKind::Quadrilateral => QuickShapeKind::Quadrilateral,
            ShapeKind::Rectangle => QuickShapeKind::Rectangle,
            ShapeKind::Square => QuickShapeKind::Square,
        }
    }
}

impl From<QuickShapeKind> for ShapeKind {
    fn from(k: QuickShapeKind) -> ShapeKind {
        match k {
            QuickShapeKind::Line => ShapeKind::Line,
            QuickShapeKind::Polyline => ShapeKind::Polyline,
            QuickShapeKind::Arc => ShapeKind::Arc,
            QuickShapeKind::Ellipse => ShapeKind::Ellipse,
            QuickShapeKind::Circle => ShapeKind::Circle,
            QuickShapeKind::Triangle => ShapeKind::Triangle,
            QuickShapeKind::EquilateralTriangle => ShapeKind::EquilateralTriangle,
            QuickShapeKind::Quadrilateral => ShapeKind::Quadrilateral,
            QuickShapeKind::Rectangle => ShapeKind::Rectangle,
            QuickShapeKind::Square => ShapeKind::Square,
        }
    }
}

/// The shape a stroke snapped to: its kind, the kinds it can be shown as instead (its own first),
/// and its editing handles (window points, x and y in turn).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct QuickShapeInfo {
    pub kind: QuickShapeKind,
    pub choices: Vec<QuickShapeKind>,
    pub handles: Vec<f32>,
}

/// A snapped stroke being shown and edited.
pub(crate) struct ShapeEdit {
    shape: Shape,
    choices: Vec<ShapeKind>,
    /// The stroke as drawn (canvas points), which other kinds are fitted to, and the pressure the
    /// pen had at each of those points.
    drawn: Vec<Vec2>,
    pressures: Vec<f32>,
    /// When each of those points came (milliseconds): the tapers are measured in time, so a shape
    /// redrawn without it loses them.
    times: Vec<f64>,
    /// How fast it was drawn (canvas pixels a millisecond).
    pace: f32,
    /// While the pen that drew it is still down.
    held: Option<Held>,
}

/// A snapped shape whose pen is still down: shown as worked out afresh from the shape as it
/// snapped, at each move.
struct Held {
    snapped: Shape,
    /// Where the pen was when it snapped, and where it is (canvas points): the shape turns and
    /// grows about its pivot as the one goes to the other.
    from: Vec2,
    at: Vec2,
    /// A finger is down: the kind as [`Shape::snapped`] makes it, the turn in whole steps.
    finger: bool,
}

/// A stroke as it began (and, once kept, what it drew), so that QuickShape can redraw it as a
/// shape after it has ended.
pub(crate) struct LastStroke {
    pub(crate) layer: LayerId,
    pub(crate) style: StrokeStyle,
    pub(crate) brush: Brush,
    pub(crate) diameter: f32,
    pub(crate) pointer: PointerKind,
    /// What it drew (canvas points), the pressure it had at each of them, and how fast it went.
    pub(crate) drawn: Vec<Vec2>,
    pub(crate) pressures: Vec<f32>,
    pub(crate) times: Vec<f64>,
    pub(crate) pace: f32,
    /// The canvas's undo steps once it was kept: anything done since changes them.
    pub(crate) steps: u64,
}

/// The outline is redrawn with samples this far apart (canvas pixels), at the pace its stroke was
/// drawn: this if that isn't known (canvas pixels a millisecond).
const SAMPLE_STEP: f32 = 2.0;
pub(crate) const SAMPLE_PACE: f32 = 1.0;
/// A stroke counts as moving between two of its samples this far apart (canvas pixels), and a
/// step of it takes no longer than this (milliseconds): a pause, or the hold that snaps it, is no
/// part of how fast it was drawn.
const PACE_MOVED: f32 = 0.5;
const PACE_MOST_MS: f64 = 50.0;

impl State {
    /// Notes a stroke's input, for QuickShape.
    pub(crate) fn remember_input(&mut self, samples: &[Sample]) {
        for s in samples.iter().filter(|s| s.is_finite()) {
            if self.drawn.last().is_some_and(|last| last.distance(s.pos) >= PACE_MOVED) {
                self.drawn_moving += (s.time_ms - self.drawn_at).clamp(0.0, PACE_MOST_MS);
            }
            self.drawn_at = s.time_ms;
            self.drawn.push(s.pos);
            self.drawn_pressures.push(s.pressure.clamp(0.0, 1.0));
            self.drawn_times.push(s.time_ms);
        }
    }

    /// Redraws the open stroke along the shape's outline.
    fn draw_shape(&mut self) {
        let (Some(edit), Some(old)) = (self.shape.as_ref(), self.stroke.as_ref()) else { return };
        // the outline is the path: nothing smooths it, or round its corners
        // the start taper the stroke showed is the one the shape is drawn with: working it out again
        // from a redrawn path puts it somewhere else along it
        let start = old.start_taper();
        let mut builder = StrokeBuilder::with_seed(old.brush().clone(), old.diameter(), old.pointer(), 0x5eed_1234)
            .scaled(self.brush_scale())
            .exact_path()
            .with_start_span((start > 0.0).then_some(start));
        // an ellipse goes where the stroke went, each of its points taken onto it: a stroke that
        // didn't close stays open there, as drawn, rather than meeting itself in a seam
        let outline = match edit.shape.kind {
            ShapeKind::Ellipse | ShapeKind::Circle => {
                along_ellipse(&edit.shape.nodes, &edit.drawn).unwrap_or_else(|| edit.shape.outline())
            }
            _ => edit.shape.outline(),
        };
        let along = StrokeAlong::of(&edit.drawn, &edit.pressures, &edit.times, edit.pace);
        let samples = resample(&outline, &along);
        builder.add(&samples);
        let stamps = builder.finish();
        if let Some(canvas) = self.canvas.as_mut() {
            // nothing drawn ahead of the pen before it snapped stays over the shape
            canvas.set_prediction(&[]);
            canvas.set_tip(&[]);
            // made a shape, a dual brush's stroke goes on as its first alone
            canvas.drop_second();
            canvas.replace_stamps(&stamps);
        }
        self.stroke = Some(builder);
        self.second = None;
    }

    /// The pen still down after a snap moved to `to` (canvas point): the shape turns and grows
    /// with it, about its start (open shapes) or centre (closed ones). Whether it changed.
    pub(crate) fn follow_shape(&mut self, to: Vec2) -> bool {
        let Some(held) = self.shape.as_mut().and_then(|e| e.held.as_mut()) else { return false };
        held.at = to;
        self.reshape_held()
    }

    /// Works the held shape out again from the shape as it snapped. Whether it changed.
    fn reshape_held(&mut self) -> bool {
        let Some(edit) = self.shape.as_mut() else { return false };
        let Some(held) = edit.held.as_ref() else { return false };
        let base = if held.finger { held.snapped.snapped(&edit.drawn) } else { held.snapped.clone() };
        let mut shape = base.moved_with(base.pivot(), held.from, held.at);
        if held.finger {
            shape = shape.turned_to_step();
        }
        if shape == edit.shape {
            return false;
        }
        edit.shape = shape;
        self.draw_shape();
        true
    }

    /// The pen that drew the snapped shape lifted: it stays where it is, open to be edited.
    pub(crate) fn let_go_shape(&mut self) {
        if let Some(edit) = self.shape.as_mut() {
            edit.held = None;
        }
    }

    /// The stroke just kept is remembered, for QuickShape on the last stroke.
    pub(crate) fn remember_last_stroke(&mut self) {
        let Some(mut last) = self.began.take() else { return };
        // its step is made now, so that a later change shows against it
        self.flush();
        let Some(canvas) = self.canvas.as_ref() else { return };
        last.drawn = self.drawn.clone();
        last.pressures = self.drawn_pressures.clone();
        last.times = self.drawn_times.clone();
        last.pace = self.pace();
        last.steps = canvas.steps_made();
        self.last_stroke = Some(last);
    }

    /// How fast the stroke went while it moved (canvas pixels a millisecond).
    fn pace(&self) -> f32 {
        let way: f32 = self.drawn.windows(2).filter_map(|w| Some(w.first()?.distance(*w.get(1)?))).sum();
        if self.drawn_moving > 0.0 && way > 0.0 { way / self.drawn_moving as f32 } else { SAMPLE_PACE }
    }

    fn shape_info(&self) -> Option<QuickShapeInfo> {
        let edit = self.shape.as_ref()?;
        let view = self.view?;
        let to_screen = view.to_screen();
        Some(QuickShapeInfo {
            kind: edit.shape.kind.into(),
            choices: edit.choices.iter().map(|&k| k.into()).collect(),
            handles: edit
                .shape
                .handles()
                .into_iter()
                .flat_map(|p| {
                    let w = to_screen.apply(p);
                    [w.x, w.y]
                })
                .collect(),
        })
    }
}

/// The way along the ellipse `nodes` (its centre, then the ends of its two axes) that the stroke
/// `drawn` went: each of its points taken onto the ellipse at the same turn about the centre in the
/// ellipse's own frame, joined along the ellipse. A stroke that stopped short stays open, and one
/// that ran on past its start lies over it, its end clear of the start's taper. The turns are those
/// about the ellipse the stroke fitted, so a shape moved, turned or sized since goes as it did.
/// None for a flat ellipse or no stroke.
fn along_ellipse(nodes: &[Vec2], drawn: &[Vec2]) -> Option<Vec<Vec2>> {
    let frame = |n: &[Vec2]| Some((*n.first()?, *n.get(1)? - *n.first()?, *n.get(2)? - *n.first()?));
    let (c, a, b) = frame(nodes)?;
    let (fc, fa, fb) = frame(&fit_ellipse(drawn).nodes)?;
    let det = fa.x * fb.y - fa.y * fb.x;
    if det.abs() < 1e-6 || a.x * b.y - a.y * b.x == 0.0 || drawn.len() < 2 {
        return None;
    }
    // the turn of a point in the fitted ellipse's frame: (cos, sin) are its coordinates along the
    // axes
    let turn = |p: Vec2| {
        let d = p - fc;
        let (u, v) = ((d.x * fb.y - d.y * fb.x) / det, (fa.x * d.y - fa.y * d.x) / det);
        v.atan2(u)
    };
    let on = |t: f32| c + a * t.cos() + b * t.sin();
    use std::f32::consts::{PI, TAU};
    let step = TAU / ELLIPSE_STEPS;
    let mut t = turn(*drawn.first()?);
    let mut out = vec![on(t)];
    for p in drawn.iter().skip(1).filter(|p| p.is_finite()) {
        // the shorter way round
        let d = (turn(*p) - t + PI).rem_euclid(TAU) - PI;
        let n = (d.abs() / step).ceil().max(1.0) as usize;
        for k in 1..=n {
            out.push(on(t + d * k as f32 / n as f32));
        }
        t += d;
    }
    Some(out)
}

/// Steps once round an ellipse followed from one of the stroke's points to the next.
const ELLIPSE_STEPS: f32 = 128.0;

/// What the pen was doing along a stroke, so the shape it is redrawn as carries the same: the
/// pressure it pressed with and how fast it was moving, at any fraction of the way through. Both
/// matter - the pressure shades the stroke, and the tapers are measured against the pace.
struct StrokeAlong {
    /// Where each sample sits along the stroke, as a share of its whole length.
    at: Vec<f32>,
    pressures: Vec<f32>,
    /// How fast the pen was moving there, canvas pixels a millisecond.
    paces: Vec<f32>,
}

impl StrokeAlong {
    fn of(drawn: &[Vec2], pressures: &[f32], times: &[f64], fallback: f32) -> StrokeAlong {
        let fallback = if fallback.is_finite() && fallback > 1e-3 { fallback } else { SAMPLE_PACE };
        let mut at = Vec::with_capacity(drawn.len());
        let mut paces = Vec::with_capacity(drawn.len());
        at.push(0.0);
        paces.push(fallback);
        let mut total = 0.0f32;
        for (i, w) in drawn.windows(2).enumerate() {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
            let step = a.distance(b);
            total += step;
            at.push(total);
            // how long that step took, where the stroke kept the time it was made at
            let took = times.get(i + 1).copied().zip(times.get(i).copied()).map_or(0.0, |(now, was)| now - was);
            let pace = if step > 0.0 && took > 0.0 { step / took as f32 } else { fallback };
            paces.push(pace.clamp(1e-3, 100.0));
        }
        if total > 0.0 {
            for share in &mut at {
                *share /= total;
            }
        }
        StrokeAlong { at, pressures: pressures.to_vec(), paces }
    }

    /// `values` read `way` through the stroke, between the samples either side of it.
    fn read(&self, values: &[f32], way: f32, when_none: f32) -> f32 {
        let Some(&first) = values.first() else { return when_none };
        let at = way.clamp(0.0, 1.0);
        let mut i = 0;
        while i + 2 < self.at.len() && self.at.get(i + 1).is_some_and(|&next| next < at) {
            i += 1;
        }
        let (Some(&lo), Some(&hi)) = (self.at.get(i), self.at.get(i + 1)) else { return first };
        let span = hi - lo;
        let into = if span > 0.0 { ((at - lo) / span).clamp(0.0, 1.0) } else { 0.0 };
        let a = values.get(i).copied().unwrap_or(first);
        let b = values.get(i + 1).copied().unwrap_or(a);
        a + (b - a) * into
    }

    fn pressure_at(&self, way: f32) -> f32 {
        self.read(&self.pressures, way, 1.0)
    }

    fn pace_at(&self, way: f32) -> f32 {
        self.read(&self.paces, way, SAMPLE_PACE)
    }
}

/// Points [`SAMPLE_STEP`] apart along `outline`, as stroke samples carrying the pressure the pen had
/// at that point of the way round and taking as long as it did there.
fn resample(outline: &[Vec2], along: &StrokeAlong) -> Vec<Sample> {
    let Some(&first) = outline.first() else { return Vec::new() };
    let total: f32 = outline.windows(2).filter_map(|w| Some(w.first()?.distance(*w.get(1)?))).sum();
    let mut out = vec![Sample::at(first, along.pressure_at(0.0), 0.0)];
    let (mut t, mut gone) = (0.0f64, 0.0f32);
    for w in outline.windows(2) {
        let [a, b] = w else { continue };
        let step = a.distance(*b);
        let n = (step / SAMPLE_STEP).ceil().max(1.0) as usize;
        for k in 1..=n {
            let into = k as f32 / n as f32;
            let way = (gone + step * into) / total.max(1e-6);
            t += f64::from(step / n as f32 / along.pace_at(way).max(1e-3));
            out.push(Sample::at(a.lerp(*b, into), along.pressure_at(way), t));
        }
        gone += step;
    }
    out
}

/// The outline (x and y in turn) of the shape a path of points (x and y in turn) looks like, if
/// any: a freehand selection held still snaps to it.
#[uniffi::export]
pub fn quickshape_outline(points: Vec<f32>) -> Option<Vec<f32>> {
    let path: Vec<Vec2> = points.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
    let shape = recognise(&path).into_iter().next()?;
    Some(shape.outline().into_iter().flat_map(|p| [p.x, p.y]).collect())
}

#[uniffi::export]
impl CanvasSession {
    /// The stroke being drawn has been held still: it snaps to the shape it looks like, if any.
    pub fn quickshape_hold(&self) -> Option<QuickShapeInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.stroke.is_none() || st.shape.is_some() {
            return None;
        }
        let found = recognise(&st.drawn);
        let first = found.first()?.clone();
        let choices = found.iter().map(|s| s.kind).collect();
        let held = st.drawn.last().map(|&at| Held { snapped: first.clone(), from: at, at, finger: false });
        let (drawn, pressures, times, pace) =
            (st.drawn.clone(), st.drawn_pressures.clone(), st.drawn_times.clone(), st.pace());
        st.shape = Some(ShapeEdit { shape: first, choices, drawn, pressures, times, pace, held });
        st.began = None;
        st.draw_shape();
        self.shared.frame(st);
        st.shape_info()
    }

    /// The last stroke, if nothing has been done since it was kept, snaps to the shape it looks
    /// like: it comes off its layer and is redrawn along the shape, open to be edited as a held
    /// stroke is.
    pub fn quickshape_last(&self) -> Option<QuickShapeInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.stroke.is_some() || st.shape.is_some() {
            return None;
        }
        st.flush();
        let last = st.last_stroke.take()?;
        let canvas = st.canvas.as_mut()?;
        if canvas.steps_made() != last.steps || canvas.can_redo() {
            return None;
        }
        let found = recognise(&last.drawn);
        let first = found.first()?.clone();
        let choices = found.iter().map(|s| s.kind).collect();
        canvas.undo();
        canvas.begin_stroke(last.layer, last.style);
        let scale = st.brush_scale();
        st.stroke =
            Some(StrokeBuilder::with_seed(last.brush, last.diameter, last.pointer.into(), 0x5eed_1234).scaled(scale));
        st.second = None;
        let (drawn, pressures, times, pace) = (last.drawn, last.pressures, last.times, last.pace);
        st.shape = Some(ShapeEdit { shape: first, choices, drawn, pressures, times, pace, held: None });
        st.draw_shape();
        self.shared.frame(st);
        st.shape_info()
    }

    /// The snapped shape, while there is one.
    pub fn quickshape_info(&self) -> Option<QuickShapeInfo> {
        self.shared.lock().shape_info()
    }

    /// Shows the snapped shape as `kind` instead.
    pub fn quickshape_choose(&self, kind: QuickShapeKind) -> Option<QuickShapeInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let edit = st.shape.as_mut()?;
        // a kind chosen is kept: the pen, if still down, no longer moves it
        edit.held = None;
        let current = edit.shape.kind;
        // a kind already shown keeps its edits; another is fitted to the stroke as drawn
        edit.shape = edit.shape.as_kind(kind.into(), &edit.drawn);
        if edit.shape.kind == current && ShapeKind::from(kind) != current {
            return st.shape_info();
        }
        st.draw_shape();
        self.shared.frame(st);
        st.shape_info()
    }

    /// Drags handle `index` of the snapped shape to window point (`x`, `y`).
    pub fn quickshape_move_handle(&self, index: u32, x: f32, y: f32) -> Option<QuickShapeInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let to = st.view?.screen_to_canvas(Vec2::new(x, y))?;
        let edit = st.shape.as_mut()?;
        edit.held = None;
        edit.shape.move_handle(index as usize, to);
        st.draw_shape();
        self.shared.frame(st);
        st.shape_info()
    }

    /// A finger put down (`down`) or lifted while the pen still holds the snapped shape: while
    /// it is down the shape is shown as [`Shape::snapped`] makes it, its turn in whole steps;
    /// lifted, as before. Whatever shows when the pen lifts is kept.
    pub fn quickshape_finger(&self, down: bool) -> Option<QuickShapeInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let changed = match st.shape.as_mut()?.held.as_mut() {
            Some(held) if held.finger != down => {
                held.finger = down;
                true
            }
            _ => false,
        };
        if changed && st.reshape_held() {
            self.shared.frame(st);
        }
        st.shape_info()
    }

    /// Whether window point (`x`, `y`) is within `reach` (window pixels) of the snapped shape's
    /// outline, where a drag turns and resizes it while it is edited.
    pub fn quickshape_near_edge(&self, x: f32, y: f32, reach: f32) -> bool {
        let st = self.shared.lock();
        let (Some(edit), Some(view)) = (st.shape.as_ref(), st.view) else { return false };
        let Some(p) = view.screen_to_canvas(Vec2::new(x, y)) else { return false };
        edit.shape.turns_from_edge() && edit.shape.edge_distance(p) * view.zoom() <= reach
    }

    /// A drag on the edited shape's outline, from window point (`x`, `y`) (`begin`) or on to it:
    /// the shape turns and resizes about its pivot as a held pen turns it.
    pub fn quickshape_pin(&self, x: f32, y: f32, begin: bool) -> Option<QuickShapeInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let at = st.view?.screen_to_canvas(Vec2::new(x, y))?;
        let edit = st.shape.as_mut()?;
        if begin {
            edit.held = Some(Held { snapped: edit.shape.clone(), from: at, at, finger: false });
        } else if st.follow_shape(at) {
            self.shared.frame(st);
        }
        st.shape_info()
    }

    /// The drag on the outline (or the pen) let go: the shape stays as it shows.
    pub fn quickshape_unpin(&self) -> Option<QuickShapeInfo> {
        let mut guard = self.shared.lock();
        guard.let_go_shape();
        guard.shape_info()
    }

    /// The edited shape dragged from window point (`fx`, `fy`) to (`tx`, `ty`): it moves along.
    pub fn quickshape_move(&self, fx: f32, fy: f32, tx: f32, ty: f32) -> Option<QuickShapeInfo> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let view = st.view?;
        let by = view.screen_to_canvas(Vec2::new(tx, ty))? - view.screen_to_canvas(Vec2::new(fx, fy))?;
        let edit = st.shape.as_mut()?;
        edit.held = None;
        edit.shape = edit.shape.translated(by);
        st.draw_shape();
        self.shared.frame(st);
        st.shape_info()
    }

    /// Keeps the snapped shape as drawn.
    pub fn quickshape_done(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.shape.is_some() {
            st.finish_stroke();
            self.shared.frame(st);
        }
    }
}

#[cfg(test)]
mod tests {
    use procraft_engine::input::Pointer;

    use super::*;

    /// A hand's circle about (500, 400) from its right: `round` times round, a little off true.
    fn circle(round: f32, step_ms: f64) -> Vec<Sample> {
        let n = (round * 120.0) as usize;
        (0..=n)
            .map(|i| {
                let t = i as f32 / 120.0 * std::f32::consts::TAU;
                let wobble = 1.0 + 0.01 * (t * 3.0).sin();
                let at = Vec2::new(500.0 + 200.0 * wobble * t.cos(), 400.0 + 200.0 * wobble * t.sin());
                Sample::at(at, 0.5, f64::from(i as u32) * step_ms)
            })
            .collect()
    }

    fn positions(samples: &[Sample]) -> Vec<Vec2> {
        samples.iter().map(|s| s.pos).collect()
    }

    #[test]
    fn an_ellipse_moved_or_sized_goes_round_as_its_stroke_did() {
        let drawn = positions(&circle(1.0, 8.0));
        let shape = fit_ellipse(&drawn);
        let by = Vec2::new(900.0, -300.0);
        // moved clear of the stroke, and then its first axis drawn out
        let mut moved = shape.translated(by);
        let mut sized = moved.clone();
        sized.move_handle(0, moved.nodes[0] + (moved.nodes[1] - moved.nodes[0]) * 2.0);
        for shape in [&mut moved, &mut sized] {
            let way = along_ellipse(&shape.nodes, &drawn).unwrap();
            let (first, last) = (way[0], way[way.len() - 1]);
            assert!(first.distance(last) < 1.0, "left open from {first:?} to {last:?}");
            let centre = shape.nodes[0];
            // it reaches every side of the ellipse
            for side in [Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, -1.0)] {
                assert!(way.iter().any(|p| (*p - centre).dot(side) > 150.0), "{side:?} of {centre:?} not reached");
            }
        }
    }

    #[test]
    fn an_ellipse_goes_as_far_round_as_its_stroke_did() {
        let short = positions(&circle(0.9, 8.0));
        let shape = fit_ellipse(&short);
        let way = along_ellipse(&shape.nodes, &short).unwrap();
        assert!(way[0].distance(way[way.len() - 1]) > 100.0, "a stroke that stopped short stays open");
        // run on past its start it lies over it, ending as far on as the stroke did
        let over = positions(&circle(1.1, 8.0));
        let shape = fit_ellipse(&over);
        let way = along_ellipse(&shape.nodes, &over).unwrap();
        let (start, end) = (way[0], way[way.len() - 1]);
        let (want, got) = (over[0].distance(over[over.len() - 1]), start.distance(end));
        assert!((want - got).abs() < 25.0, "ended {got} from its start, as drawn {want}");
        assert!(way.iter().skip(way.len() / 2).any(|p| p.distance(start) < 4.0), "it never passed its start");
    }

    /// The session's stroke through `samples` with `brush`, held until it snapped: its stamps.
    fn snapped(brush: procraft_engine::brush::Brush, samples: &[Sample]) -> Vec<procraft_engine::stroke::Stamp> {
        let s = CanvasSession::new(1000, 800);
        {
            let mut st = s.shared.lock();
            st.view = Some(procraft_engine::view::View::fit((1000, 800), (1000, 800)));
            // the builder takes them as drawing does, so the tapers the shape keeps are the ones
            // this stroke had
            let mut builder = StrokeBuilder::new(brush, 8.0, Pointer::Stylus);
            builder.add(samples);
            st.stroke = Some(builder);
            st.remember_input(samples);
        }
        assert!(s.quickshape_hold().is_some());
        let st = s.shared.lock();
        st.stroke.as_ref().unwrap().stamps()
    }

    #[test]
    fn a_shape_keeps_the_start_taper_the_stroke_had() {
        let mut brush = procraft_engine::brush::Brush::round_pen();
        brush.pen_taper =
            procraft_engine::brush::Taper { start: 0.5, end: 0.5, size: 1.0, ..procraft_engine::brush::Taper::NONE };
        // a straight stroke drawn at a steady 8 ms a point
        let samples: Vec<Sample> =
            (0..=80).map(|i| Sample::at(Vec2::new(200.0 + 6.0 * i as f32, 400.0), 0.6, i as f64 * 8.0)).collect();
        let stamps = snapped(brush, &samples);
        let first = stamps.first().map_or(0.0, |s| s.radius);
        let middle = stamps.get(stamps.len() / 2).map_or(0.0, |s| s.radius);
        assert!(first < middle * 0.75, "start {first} against the middle {middle}");
    }

    #[test]
    fn a_shape_redrawn_is_not_smoothed_whatever_the_brush_does_to_a_stroke() {
        let mut brush = procraft_engine::brush::Brush::round_pen();
        brush.pen_taper = procraft_engine::brush::Taper::NONE;
        brush.set_setting("motion_filtering", 1.0);
        let stamps = snapped(brush, &circle(1.0, 4.0));
        let radius = |p: Vec2| p.distance(Vec2::new(500.0, 400.0));
        assert!(stamps.iter().all(|s| (radius(s.center) - 200.0).abs() < 6.0), "off the circle");
        let (first, last) = (stamps[0].center, stamps[stamps.len() - 1].center);
        assert!(first.distance(last) < 4.0, "{first:?} {last:?}");
    }

    #[test]
    fn a_shape_is_redrawn_as_fast_as_its_stroke_went_leaving_out_the_hold() {
        let s = CanvasSession::new(1000, 800);
        let mut st = s.shared.lock();
        // a second round, then the pen held still: wobbling on the spot, heard from every 8 ms
        let drawn = circle(1.0, 1000.0 / 120.0);
        st.remember_input(&drawn);
        let end = drawn[drawn.len() - 1];
        let held: Vec<Sample> = (1..80).map(|i| Sample { time_ms: end.time_ms + f64::from(i) * 8.0, ..end }).collect();
        st.remember_input(&held);
        // about 1270 pixels round in a second
        let pace = st.pace();
        assert!((pace - 1.27).abs() < 0.05, "{pace}");
        // and the outline takes as long
        let along = StrokeAlong { at: vec![0.0, 1.0], pressures: vec![1.0, 1.0], paces: vec![pace, pace] };
        let outline = resample(&positions(&drawn), &along);
        let took = outline[outline.len() - 1].time_ms;
        assert!((took - 1000.0).abs() < 30.0, "{took}");
    }

    #[test]
    fn a_shape_carries_the_pressure_the_stroke_had_along_it() {
        // a stroke drawn with the pen pressed harder and harder
        let drawn: Vec<Vec2> = (0..=10).map(|i| Vec2::new(100.0 * i as f32, 0.0)).collect();
        let pressures: Vec<f32> = (0..=10).map(|i| 0.2 + 0.8 * i as f32 / 10.0).collect();
        let along = StrokeAlong::of(&drawn, &pressures, &[], 1.0);
        assert!((along.pressure_at(0.0) - 0.2).abs() < 0.05, "{}", along.pressure_at(0.0));
        assert!((along.pressure_at(1.0) - 1.0).abs() < 0.05, "{}", along.pressure_at(1.0));
        let half = along.pressure_at(0.5);
        assert!((0.5..0.7).contains(&half), "{half}");
        // and a shape redrawn from it goes the same way, rather than flat
        let outline: Vec<Vec2> = (0..=20).map(|i| Vec2::new(i as f32 * 5.0, 0.0)).collect();
        let samples = resample(&outline, &along);
        let (first, last) = (samples.first().map_or(1.0, |s| s.pressure), samples.last().map_or(1.0, |s| s.pressure));
        assert!(last > first + 0.6, "{first} to {last}");
    }

    #[test]
    fn a_shape_keeps_the_pace_the_stroke_was_made_at() {
        // the same ground covered slowly at first and quickly at the end, as a hand does
        let drawn: Vec<Vec2> = (0..=10).map(|i| Vec2::new(100.0 * i as f32, 0.0)).collect();
        let times: Vec<f64> =
            (0..=10).map(|i| if i <= 5 { i as f64 * 20.0 } else { 100.0 + (i - 5) as f64 * 4.0 }).collect();
        let pressures = vec![0.5; 11];
        let along = StrokeAlong::of(&drawn, &pressures, &times, 1.0);
        let (slow, quick) = (along.pace_at(0.1), along.pace_at(0.9));
        assert!(quick > slow * 3.0, "moving {slow} then {quick}");
    }
}
