//! QuickShape: a stroke drawn and held snaps to the shape it looks like, redrawn with the same
//! brush along the shape's outline. It stays open while it is edited (its kind changed, its
//! handles dragged) and is kept as drawn with the next thing done.

use std::f32::consts::{PI, TAU};

use procraft_engine::brush::Brush;
use procraft_engine::document::LayerId;
use procraft_engine::geom::Vec2;
use procraft_engine::input::Sample;
use procraft_engine::quickshape::{Shape, ShapeKind, fit_ellipse, recognise};
use procraft_engine::stroke::StrokeBuilder;
use procraft_gpu::{Canvas, StrokeStyle};

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
    /// As it first snapped; as it was before each drag, the latest last; and what undo took back
    /// of those, for redo.
    first: Shape,
    earlier: Vec<Shape>,
    later: Vec<Shape>,
    choices: Vec<ShapeKind>,
    /// The stroke as drawn (canvas points), which other kinds are fitted to.
    drawn: Vec<Vec2>,
    /// What the pen did along it.
    along: StrokeAlong,
    /// While the pen that drew it is still down.
    held: Option<Held>,
    /// The stroke it was made of, for undo to go back to.
    stroke: Option<DrawnStroke>,
}

impl ShapeEdit {
    fn new(
        shape: Shape,
        choices: Vec<ShapeKind>,
        drawn: Vec<Vec2>,
        along: StrokeAlong,
        held: Option<Held>,
        stroke: Option<DrawnStroke>,
    ) -> ShapeEdit {
        let first = shape.clone();
        ShapeEdit { shape, first, earlier: Vec::new(), later: Vec::new(), choices, drawn, along, held, stroke }
    }

    /// A drag undone can be done again.
    pub(crate) fn can_redo(&self) -> bool {
        !self.later.is_empty()
    }
}

/// A stroke made a shape, as it can be laid again: where, its builder as it began and its input.
/// (A dual brush's isn't kept: its shape is its first alone.)
pub(crate) struct DrawnStroke {
    layer: LayerId,
    style: StrokeStyle,
    builder: StrokeBuilder,
    samples: Vec<Sample>,
}

/// A shape kept, as it first snapped, or the stroke it gave way to: the canvas's undo steps once
/// that was laid tell if it still stands.
pub(crate) struct KeptShape {
    first: Shape,
    choices: Vec<ShapeKind>,
    drawn: Vec<Vec2>,
    along: StrokeAlong,
    stroke: DrawnStroke,
    steps: u64,
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
    /// What it drew (canvas points) and how fast it went.
    pub(crate) drawn: Vec<Vec2>,
    pub(crate) pace: f32,
    /// The canvas's undo steps once it was kept: anything done since changes them.
    pub(crate) steps: u64,
    /// Its builder as it began (none for a dual brush), and its input.
    pub(crate) fresh: Option<StrokeBuilder>,
    pub(crate) samples: Vec<Sample>,
}

impl LastStroke {
    /// The stroke as it can be laid again, given its input.
    fn laid_again(&self, samples: Vec<Sample>) -> Option<DrawnStroke> {
        let builder = self.fresh.clone()?;
        Some(DrawnStroke { layer: self.layer, style: self.style, builder, samples })
    }
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
/// A step of a shape's stroke goes no slower or faster than this many times its usual pace: a
/// crawl of a few pixels is no reason for a shape to stop there.
const PACE_MOST_OFF: f32 = 8.0;

impl State {
    /// Notes a stroke's input, for QuickShape.
    pub(crate) fn remember_input(&mut self, samples: &[Sample]) {
        for s in samples.iter().filter(|s| s.is_finite()) {
            if self.drawn.last().is_some_and(|last| last.distance(s.pos) >= PACE_MOVED) {
                self.drawn_moving += (s.time_ms - self.drawn_at).clamp(0.0, PACE_MOST_MS);
            }
            self.drawn_at = s.time_ms;
            self.drawn.push(s.pos);
        }
        self.drawn_samples.extend_from_slice(samples);
    }

    /// `edit`'s shape was just kept: undo can give back the stroke it was made of.
    pub(crate) fn note_kept_shape(&mut self, edit: Option<ShapeEdit>) {
        let Some(ShapeEdit { first, choices, drawn, along, stroke: Some(stroke), .. }) = edit else { return };
        // its step is made now, so that a later change shows against it
        self.flush();
        let Some(canvas) = self.canvas.as_ref() else { return };
        let steps = canvas.steps_made();
        self.kept_shape = Some(KeptShape { first, choices, drawn, along, stroke, steps });
    }

    /// Whether `kept` is still the last thing done, with nothing undone after it.
    fn still_stands(&self, kept: &KeptShape) -> bool {
        self.canvas.as_ref().is_some_and(|c| c.steps_made() == kept.steps && !c.can_redo())
    }

    /// There is a shape for redo to put back in place of the stroke it gave way to.
    pub(crate) fn shape_to_redo(&self) -> bool {
        self.unkept_shape.as_ref().is_some_and(|kept| self.still_stands(kept))
    }

    /// The shape last kept, if it is the last thing done, gives way to the stroke it was made of.
    fn back_to_stroke(&mut self) -> bool {
        self.flush();
        let Some(mut kept) = self.kept_shape.take().filter(|kept| self.still_stands(kept)) else { return false };
        let guide = self.doc.stroke_guide();
        let Some(canvas) = self.canvas.as_mut() else { return false };
        canvas.undo();
        canvas.begin_stroke(kept.stroke.layer, kept.stroke.style);
        let mut builder = kept.stroke.builder.clone();
        crate::draw_samples(canvas, &mut builder, None, &kept.stroke.samples, &guide);
        crate::close_stroke(canvas, builder, None, &guide);
        self.flush();
        kept.steps = self.canvas.as_ref().map_or(0, Canvas::steps_made);
        self.unkept_shape = Some(kept);
        true
    }

    /// The stroke a shape gave way to, if it is the last thing done, is that shape again, as it
    /// first snapped.
    fn back_to_shape(&mut self) -> bool {
        self.flush();
        if !self.shape_to_redo() {
            return false;
        }
        let (Some(kept), Some(canvas)) = (self.unkept_shape.take(), self.canvas.as_mut()) else { return false };
        canvas.undo();
        canvas.begin_stroke(kept.stroke.layer, kept.stroke.style);
        self.stroke = Some(kept.stroke.builder.clone());
        self.second = None;
        self.shape = Some(ShapeEdit::new(kept.first, kept.choices, kept.drawn, kept.along, None, Some(kept.stroke)));
        self.draw_shape();
        self.finish_stroke();
        true
    }

    /// Redraws the open stroke along the shape's outline.
    fn draw_shape(&mut self) {
        let (Some(edit), Some(old)) = (self.shape.as_ref(), self.stroke.as_ref()) else { return };
        // the outline is the path: nothing smooths it, or round its corners
        let mut builder = StrokeBuilder::with_seed(old.brush().clone(), old.diameter(), old.pointer(), 0x5eed_1234)
            .scaled(self.brush_scale())
            .exact_path();
        // an ellipse goes where the stroke went, each of its points taken onto it: a stroke that
        // didn't close stays open there, as drawn, rather than meeting itself in a seam
        let outline = match edit.shape.kind {
            ShapeKind::Ellipse | ShapeKind::Circle => {
                along_ellipse(&edit.shape.nodes, &edit.drawn).unwrap_or_else(|| edit.shape.outline())
            }
            _ => edit.shape.outline(),
        };
        let samples = resample(&outline, &edit.along);
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
        last.pace = self.pace();
        last.steps = canvas.steps_made();
        last.samples = std::mem::take(&mut self.drawn_samples);
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

/// The pen at a point of a stroke.
#[derive(Clone, Copy)]
struct PenThere {
    /// How far through the stroke it is, 0..1.
    way: f32,
    pressure: f32,
    tilt: f32,
    azimuth: f32,
    roll: f32,
    /// How fast it was going (canvas pixels a millisecond).
    pace: f32,
}

impl PenThere {
    /// Part of the way from this to `next`; its angles the short way round.
    fn towards(self, next: PenThere, part: f32) -> PenThere {
        let mix = |a: f32, b: f32| a + (b - a) * part;
        let turn = |a: f32, b: f32| a + ((b - a + PI).rem_euclid(TAU) - PI) * part;
        PenThere {
            way: mix(self.way, next.way),
            pressure: mix(self.pressure, next.pressure),
            tilt: mix(self.tilt, next.tilt),
            azimuth: turn(self.azimuth, next.azimuth),
            roll: turn(self.roll, next.roll),
            pace: mix(self.pace, next.pace),
        }
    }

    /// As a stroke's sample at `pos`, `time_ms` into it.
    fn sample(self, pos: Vec2, time_ms: f64) -> Sample {
        let PenThere { pressure, tilt, azimuth, roll, .. } = self;
        Sample { pos, pressure, tilt, azimuth, roll, time_ms }
    }
}

/// What the pen did along a stroke, for the shape it is redrawn as to do the same: a shape keeps
/// the stroke's pressure and lean where it had them, and the pace its tapers go by.
#[derive(Clone)]
pub(crate) struct StrokeAlong {
    points: Vec<PenThere>,
}

impl StrokeAlong {
    /// From the stroke's input; `usual` how fast it went overall.
    fn of(samples: &[Sample], usual: f32) -> StrokeAlong {
        let usual = if usual.is_finite() && usual > 1e-3 { usual } else { SAMPLE_PACE };
        let mut points: Vec<PenThere> = Vec::with_capacity(samples.len());
        let (mut gone, mut pace, mut last) = (0.0f32, usual, None::<Sample>);
        for s in samples.iter().filter(|s| s.is_finite()) {
            if let Some(before) = last {
                let step = before.pos.distance(s.pos);
                gone += step;
                // a pause, or the hold that snaps it, is no part of how fast it went
                let took = (s.time_ms - before.time_ms).clamp(0.0, PACE_MOST_MS);
                if step >= PACE_MOVED && took > 0.0 {
                    pace = (step / took as f32).clamp(usual / PACE_MOST_OFF, usual * PACE_MOST_OFF);
                }
            }
            let (pressure, roll) = (s.pressure.clamp(0.0, 1.0), if s.roll.is_finite() { s.roll } else { 0.0 });
            points.push(PenThere { way: gone, pressure, tilt: s.tilt, azimuth: s.azimuth, roll, pace });
            last = Some(*s);
        }
        if gone > 0.0 {
            for point in &mut points {
                point.way /= gone;
            }
        }
        StrokeAlong { points }
    }

    /// The pen `way` through the stroke (0..1), looking on from point `from`, as the way only
    /// grows; none for a stroke with no input.
    fn at(&self, way: f32, from: &mut usize) -> Option<PenThere> {
        while self.points.get(*from + 1).is_some_and(|next| next.way < way) {
            *from += 1;
        }
        let here = *self.points.get(*from)?;
        let Some(&next) = self.points.get(*from + 1) else { return Some(here) };
        let span = next.way - here.way;
        Some(here.towards(next, if span > 0.0 { ((way - here.way) / span).clamp(0.0, 1.0) } else { 0.0 }))
    }
}

/// Points [`SAMPLE_STEP`] apart along `outline`, as stroke samples with the pen as it was at that
/// share of the way along its stroke, taking as long as it did there.
fn resample(outline: &[Vec2], along: &StrokeAlong) -> Vec<Sample> {
    let Some(&first) = outline.first() else { return Vec::new() };
    let whole: f32 = outline.windows(2).filter_map(|w| Some(w.first()?.distance(*w.get(1)?))).sum();
    let plain = PenThere { way: 0.0, pressure: 1.0, tilt: 0.0, azimuth: 0.0, roll: 0.0, pace: SAMPLE_PACE };
    let mut from = 0;
    let mut out = vec![along.at(0.0, &mut from).unwrap_or(plain).sample(first, 0.0)];
    let (mut t, mut gone) = (0.0f64, 0.0f32);
    for w in outline.windows(2) {
        let [a, b] = w else { continue };
        let step = a.distance(*b);
        let n = (step / SAMPLE_STEP).ceil().max(1.0) as usize;
        for k in 1..=n {
            let part = k as f32 / n as f32;
            let pen = along.at((gone + step * part) / whole.max(1e-6), &mut from).unwrap_or(plain);
            t += f64::from(step / n as f32 / pen.pace.max(1e-3));
            out.push(pen.sample(a.lerp(*b, part), t));
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
        // the line the stabilisers and smoothing are drawing, not the movement it came from: a
        // shape should follow the line being painted rather than the hand's shake
        let mut path = st.drawn.clone();
        if let Some(old) = st.stroke.as_ref() {
            let mut smoothed = old.path_points();
            // the stamps stop a spacing short of either end, so the stroke's own ends go on: a
            // stroke that met itself still does
            if smoothed.len() >= 2
                && let (Some(&start), Some(&end)) = (st.drawn.first(), st.drawn.last())
            {
                smoothed.insert(0, start);
                smoothed.push(end);
            }
            if smoothed.len() >= 2 {
                path = smoothed;
            }
        }
        let found = recognise(&path);
        let first = found.first()?.clone();
        let choices = found.iter().map(|s| s.kind).collect();
        let held = st.drawn.last().map(|&at| Held { snapped: first.clone(), from: at, at, finger: false });
        let (drawn, along) = (st.drawn.clone(), StrokeAlong::of(&st.drawn_samples, st.pace()));
        let stroke = st.began.take().and_then(|began| began.laid_again(st.drawn_samples.clone()));
        st.shape = Some(ShapeEdit::new(first, choices, drawn, along, held, stroke));
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
        let stroke = last.laid_again(last.samples.clone());
        let along = StrokeAlong::of(&last.samples, last.pace);
        st.stroke =
            Some(StrokeBuilder::with_seed(last.brush, last.diameter, last.pointer.into(), 0x5eed_1234).scaled(scale));
        st.second = None;
        st.shape = Some(ShapeEdit::new(first, choices, last.drawn, along, None, stroke));
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
        let before = edit.shape.clone();
        // a kind already shown keeps its edits; another is fitted to the stroke as drawn
        edit.shape = edit.shape.as_kind(kind.into(), &edit.drawn);
        if edit.shape.kind == current && ShapeKind::from(kind) != current {
            return st.shape_info();
        }
        edit.earlier.push(before);
        edit.later.clear();
        st.draw_shape();
        self.shared.frame(st);
        st.shape_info()
    }

    /// A drag of the snapped shape begins: undo takes it back as a step of its own.
    pub(crate) fn quickshape_step(&self) {
        if let Some(edit) = self.shared.lock().shape.as_mut() {
            edit.earlier.push(edit.shape.clone());
            edit.later.clear();
        }
    }

    /// Undo where a shape is the last thing done: one still edited goes back a drag; with none
    /// left, or once kept, it gives way to the stroke it was made of, which the next undo takes.
    /// True if that was the step.
    pub(crate) fn quickshape_undo(&self) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(edit) = st.shape.as_mut() {
            if edit.held.is_none()
                && let Some(before) = edit.earlier.pop()
            {
                edit.later.push(std::mem::replace(&mut edit.shape, before));
                st.draw_shape();
                st.revision += 1;
                self.shared.frame(st);
                return true;
            }
            st.finish_stroke();
        }
        let back = st.back_to_stroke();
        if back {
            st.revision += 1;
            self.shared.frame(st);
        }
        drop(guard);
        self.set_shape_editing(false);
        back
    }

    /// Redo where a shape was undone: a drag still open to it, or the shape back in place of its
    /// stroke (as it snapped: its drags went with it). A shape open has nothing else to redo.
    /// True if there is no more to it.
    pub(crate) fn quickshape_redo(&self) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(edit) = st.shape.as_mut() {
            if let Some(after) = edit.later.pop() {
                edit.earlier.push(std::mem::replace(&mut edit.shape, after));
                st.draw_shape();
                st.revision += 1;
                self.shared.frame(st);
            }
            return true;
        }
        let back = st.back_to_shape();
        if back {
            st.revision += 1;
            self.shared.frame(st);
        }
        back
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
            st.stroke = Some(StrokeBuilder::new(brush, 8.0, Pointer::Stylus));
            st.remember_input(samples);
        }
        assert!(s.quickshape_hold().is_some());
        let st = s.shared.lock();
        st.stroke.as_ref().unwrap().stamps()
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
        let held: Vec<Sample> = (1..80)
            .map(|i| {
                let wobble = Vec2::new(0.2 * (i % 2) as f32, 0.0);
                Sample { pos: end.pos + wobble, time_ms: end.time_ms + f64::from(i) * 8.0, ..end }
            })
            .collect();
        st.remember_input(&held);
        // about 1270 pixels round in a second
        let pace = st.pace();
        assert!((pace - 1.27).abs() < 0.05, "{pace}");
        // and the outline takes as long, the hold in its input or not
        let whole: Vec<Sample> = drawn.iter().chain(&held).copied().collect();
        let outline = resample(&positions(&drawn), &StrokeAlong::of(&whole, pace));
        let took = outline[outline.len() - 1].time_ms;
        assert!((took - 1000.0).abs() < 30.0, "{took}");
    }

    /// A line of ten steps of 100 pixels, each sample as `pen` makes it from its number.
    fn straight(pen: impl Fn(usize, Sample) -> Sample) -> Vec<Sample> {
        (0..=10).map(|i| pen(i, Sample::at(Vec2::new(100.0 * i as f32, 0.0), 1.0, i as f64 * 20.0))).collect()
    }

    #[test]
    fn a_shape_carries_the_pressure_and_the_lean_its_stroke_had_along_it() {
        // pressed harder and harder, and leaning round past straight up
        let stroke = straight(|i, s| Sample {
            pressure: 0.2 + 0.08 * i as f32,
            tilt: 0.1 * i as f32,
            azimuth: (6.0 + 0.1 * i as f32).rem_euclid(TAU),
            ..s
        });
        let shape: Vec<Vec2> = (0..=20).map(|i| Vec2::new(i as f32 * 5.0, 50.0)).collect();
        let samples = resample(&shape, &StrokeAlong::of(&stroke, 5.0));
        let (first, last) = (samples[0], samples[samples.len() - 1]);
        assert!((first.pressure - 0.2).abs() < 0.01 && (last.pressure - 1.0).abs() < 0.01, "{first:?} {last:?}");
        assert!((first.tilt - 0.0).abs() < 0.01 && (last.tilt - 1.0).abs() < 0.01, "{first:?} {last:?}");
        // the lean goes on round the short way, not back through all the others
        let way = samples
            .windows(2)
            .map(|w| (w[1].azimuth - w[0].azimuth).abs().min(TAU - (w[1].azimuth - w[0].azimuth).abs()));
        assert!(way.fold(0.0f32, f32::max) < 0.2);
    }

    #[test]
    fn a_shape_keeps_the_pace_of_its_stroke_but_no_pause_in_it() {
        // slowly at first, quickly at the end
        let times = |i: usize| if i <= 5 { i as f64 * 40.0 } else { 200.0 + (i - 5) as f64 * 10.0 };
        let stroke = straight(|i, s| Sample { time_ms: times(i), ..s });
        let shape: Vec<Vec2> = (0..=500).map(|i| Vec2::new(i as f32 * 2.0, 0.0)).collect();
        let samples = resample(&shape, &StrokeAlong::of(&stroke, 4.0));
        let half = samples[samples.len() / 2].time_ms;
        let end = samples[samples.len() - 1].time_ms;
        assert!(half > 3.0 * (end - half), "{half} of {end}");
        // a long pause and a crawl on from it: no step of the shape stops there
        let stroke = straight(|i, s| {
            let pos = if i == 6 { Vec2::new(500.6, 0.0) } else { s.pos };
            Sample { pos, time_ms: if i >= 6 { s.time_ms + 900.0 } else { s.time_ms }, ..s }
        });
        let samples = resample(&shape, &StrokeAlong::of(&stroke, 5.0));
        let longest = samples.windows(2).map(|w| w[1].time_ms - w[0].time_ms).fold(0.0f64, f64::max);
        assert!(longest <= 2.0 / (5.0 / f64::from(PACE_MOST_OFF)) + 0.1, "{longest}");
    }
}
