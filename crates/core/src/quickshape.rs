//! QuickShape: a stroke drawn and held snaps to the shape it looks like, redrawn with the same
//! brush along the shape's outline. It stays open while it is edited (its kind changed, its
//! handles dragged) and is kept as drawn with the next thing done.

use procraft_engine::brush::Brush;
use procraft_engine::document::LayerId;
use procraft_engine::geom::Vec2;
use procraft_engine::input::Sample;
use procraft_engine::quickshape::{Shape, ShapeKind, recognise};
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
    /// The stroke as drawn (canvas points), which other kinds are fitted to.
    drawn: Vec<Vec2>,
    pressure: f32,
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
    /// What it drew (canvas points) and its mean pressure.
    pub(crate) drawn: Vec<Vec2>,
    pub(crate) pressure: f32,
    /// The canvas's undo steps once it was kept: anything done since changes them.
    pub(crate) steps: u64,
}

/// The outline is redrawn with samples this far apart (canvas pixels) and this many milliseconds
/// apart.
const SAMPLE_STEP: f32 = 2.0;
const SAMPLE_MS: f64 = 2.0;

impl State {
    /// Notes a stroke's input, for QuickShape.
    pub(crate) fn remember_input(&mut self, samples: &[Sample]) {
        for s in samples.iter().filter(|s| s.is_finite()) {
            self.drawn.push(s.pos);
            self.drawn_pressure += s.pressure.clamp(0.0, 1.0);
        }
    }

    /// Redraws the open stroke along the shape's outline.
    fn draw_shape(&mut self) {
        let (Some(edit), Some(old)) = (self.shape.as_ref(), self.stroke.as_ref()) else { return };
        let mut brush = old.brush().clone();
        // the outline is already smooth: nothing should round its corners
        brush.streamline = 0.0;
        brush.set_setting("stabilization", 0.0);
        let mut builder = StrokeBuilder::with_seed(brush, old.diameter(), old.pointer(), 0x5eed_1234);
        let samples = resample(&edit.shape.outline(), edit.pressure);
        builder.add(&samples);
        let stamps = builder.finish();
        if let Some(canvas) = self.canvas.as_mut() {
            // nothing drawn ahead of the pen before it snapped stays over the shape
            canvas.set_prediction(&[]);
            canvas.set_tip(&[]);
            canvas.replace_stamps(&stamps);
        }
        self.stroke = Some(builder);
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
        last.pressure = self.mean_pressure();
        last.steps = canvas.steps_made();
        self.last_stroke = Some(last);
    }

    fn mean_pressure(&self) -> f32 {
        let p = if self.drawn.is_empty() { 1.0 } else { self.drawn_pressure / self.drawn.len() as f32 };
        p.max(0.05)
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

/// Points `step` apart along `outline`, as stroke samples at `pressure`.
fn resample(outline: &[Vec2], pressure: f32) -> Vec<Sample> {
    let mut out = Vec::new();
    let mut t = 0.0f64;
    let mut push = |p: Vec2| {
        out.push(Sample::at(p, pressure, t));
        t += SAMPLE_MS;
    };
    let Some(&first) = outline.first() else { return Vec::new() };
    push(first);
    for w in outline.windows(2) {
        let [a, b] = w else { continue };
        let n = (a.distance(*b) / SAMPLE_STEP).ceil().max(1.0) as usize;
        for k in 1..=n {
            push(a.lerp(*b, k as f32 / n as f32));
        }
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
        st.shape =
            Some(ShapeEdit { shape: first, choices, drawn: st.drawn.clone(), pressure: st.mean_pressure(), held });
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
        st.stroke = Some(StrokeBuilder::with_seed(last.brush, last.diameter, last.pointer.into(), 0x5eed_1234));
        st.shape = Some(ShapeEdit { shape: first, choices, drawn: last.drawn, pressure: last.pressure, held: None });
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
