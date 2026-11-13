//! Transform: the current layer's content moved, scaled, rotated or distorted as a whole through
//! the box around it, shown live and kept as one undo step.

use procraft_engine::document::{Document, Layer, LayerId, LayerKind};
use procraft_engine::geom::{Affine, Homography, IRect, Vec2};
use procraft_engine::transform::{Guides, Handle, Transform, TransformMode, convex_hull};
use procraft_engine::warp;
use procraft_gpu::Resample;

use crate::{CanvasSession, State};

/// How transformed pixels are resampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Interpolation {
    Nearest,
    Bilinear,
    Bicubic,
}

impl From<Interpolation> for Resample {
    fn from(i: Interpolation) -> Resample {
        match i {
            Interpolation::Nearest => Resample::Nearest,
            Interpolation::Bilinear => Resample::Bilinear,
            Interpolation::Bicubic => Resample::Bicubic,
        }
    }
}

/// How the box's handles change the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransformKind {
    Freeform,
    Uniform,
    Distort,
    Warp,
}

impl From<TransformKind> for TransformMode {
    fn from(k: TransformKind) -> TransformMode {
        match k {
            TransformKind::Freeform => TransformMode::Freeform,
            TransformKind::Uniform => TransformMode::Uniform,
            TransformKind::Distort => TransformMode::Distort,
            TransformKind::Warp => TransformMode::Warp,
        }
    }
}

/// What a drag on the box holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransformHandle {
    /// 0 top left, 1 top right, 2 bottom right, 3 bottom left.
    Corner {
        index: u8,
    },
    /// The middle of an edge: 0 top, 1 right, 2 bottom, 3 left.
    Edge {
        index: u8,
    },
    /// The node above the box.
    Rotate,
    /// The node below the box.
    TurnBox,
    Move,
    /// One of a warp's 16 control points, row by row.
    Mesh {
        index: u8,
    },
}

impl From<TransformHandle> for Handle {
    fn from(h: TransformHandle) -> Handle {
        match h {
            TransformHandle::Corner { index } => Handle::Corner(index),
            TransformHandle::Edge { index } => Handle::Edge(index),
            TransformHandle::Rotate => Handle::Rotate,
            TransformHandle::TurnBox => Handle::TurnBox,
            TransformHandle::Move => Handle::Move,
            TransformHandle::Mesh { index } => Handle::Mesh(index),
        }
    }
}

/// The transform toolbar's one-tap changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransformAction {
    FlipHorizontal,
    FlipVertical,
    Rotate45,
    FitToCanvas,
    Reset,
    /// Takes back the last change made in this transform.
    Undo,
}

/// What pulls a moved or turned box: magnetics (15° steps), and snapping onto the canvas's
/// edges and centre within `distance` points while the drag is no faster than `velocity` points
/// a frame.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct GuideSettings {
    pub magnetics: bool,
    pub snapping: bool,
    pub distance: f32,
    pub velocity: f32,
}

impl Default for GuideSettings {
    fn default() -> GuideSettings {
        GuideSettings { magnetics: false, snapping: false, distance: 3.0, velocity: 10.0 }
    }
}

/// How far (points) a move goes before magnetics locks its direction.
const LOCK_AFTER_PT: f32 = 2.0;

/// The box to draw: its corners (top left, top right, bottom right, bottom left) as eight numbers
/// in canvas pixels, and whether there is a change to take back. With a warp, also its 16 control
/// points (32 numbers, row by row) and its lines at the thirds (eight lines of
/// [`GRID_SAMPLES`] + 1 points each: four across, then four down); empty otherwise.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TransformBox {
    pub corners: Vec<f32>,
    pub can_undo: bool,
    pub mesh: Vec<f32>,
    pub grid: Vec<f32>,
    /// The box's width and height (canvas pixels) and its turn from upright (degrees,
    /// clockwise), as numeric entry shows them.
    pub width: f32,
    pub height: f32,
    pub angle: f32,
    /// The lines the box held in a move has snapped onto, as guides across the canvas: segments
    /// (x0, y0, x1, y1 in turn, canvas pixels); none while it isn't snapped.
    pub snap_lines: Vec<f32>,
}

/// How finely a warp's lines are sampled for drawing, and its triangles for rendering.
const GRID_SAMPLES: usize = 24;
const MESH_STEPS: usize = 48;

/// The smallest rectangle (x, y, width, height) holding every pixel with any alpha in
/// premultiplied RGBA8 `pixels` (`width` wide), and the convex hull of those pixels' corners;
/// none when all are clear.
pub(crate) fn content_shape(pixels: &[u8], width: u32) -> Option<([f32; 4], Vec<Vec2>)> {
    let row_len = width as usize * 4;
    if row_len == 0 {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    // each row's outermost pixels are all the hull can need
    let mut ends: Vec<Vec2> = Vec::new();
    for (y, row) in pixels.chunks_exact(row_len).enumerate() {
        let (px, _) = row.as_chunks::<4>();
        let Some(first) = px.iter().position(|&[.., a]| a != 0) else { continue };
        let last = px.iter().rposition(|&[.., a]| a != 0).unwrap_or(first) + 1;
        x0 = x0.min(first);
        x1 = x1.max(last);
        y0 = y0.min(y);
        y1 = y + 1;
        let (yt, yb) = (y as f32, (y + 1) as f32);
        ends.extend([(first, yt), (first, yb), (last, yt), (last, yb)].map(|(x, y)| Vec2::new(x as f32, y)));
    }
    (x0 < x1).then(|| ([x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32], convex_hull(&ends)))
}

/// Whether `l` shows: it and every group it is in visible.
fn shown(doc: &Document, l: &Layer) -> bool {
    l.visible && l.parent.and_then(|p| doc.layer(p)).is_none_or(|p| shown(doc, p))
}

/// How many other layers a moved box snaps onto at most (each is read back once, at the start).
const MAX_SNAP_LAYERS: usize = 16;

/// The bounds (left, top, width, height) of the layers shown besides `targets`, nearest the top
/// first, for a moved box to snap onto: found on the GPU, every one of them read.
fn snap_bounds(
    doc: &Document,
    canvas: &procraft_gpu::Canvas,
    gpu: &procraft_gpu::Gpu,
    targets: &[LayerId],
) -> Vec<[f32; 4]> {
    let others: Vec<_> = doc
        .layers()
        .iter()
        .rev()
        .filter(|l| !targets.contains(&l.id) && l.kind == LayerKind::Paint && shown(doc, l))
        .take(MAX_SNAP_LAYERS)
        .map(|l| l.id)
        .collect();
    canvas
        .painted_boxes(gpu, &others)
        .into_iter()
        .flatten()
        .map(|r| [r.x0 as f32, r.y0 as f32, r.width() as f32, r.height() as f32])
        .collect()
}

/// The affine map taking corners `from` to corners `to` (top left, top right, bottom right, bottom
/// left): exact where they are a parallelogram moved as one, else what the top left, top right and
/// bottom left corners say.
fn corners_affine(from: [Vec2; 4], to: [Vec2; 4]) -> Option<Affine> {
    let ([s0, s1, _, s3], [c0, c1, _, c3]) = (from, to);
    let (u, v, u2, v2) = (s1 - s0, s3 - s0, c1 - c0, c3 - c0);
    let det = u.x * v.y - u.y * v.x;
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    // the linear part takes u to u2 and v to v2
    let (ia, ib, ic, id) = (v.y / det, -u.y / det, -v.x / det, u.x / det);
    let a = u2.x * ia + v2.x * ib;
    let b = u2.y * ia + v2.y * ib;
    let c = u2.x * ic + v2.x * id;
    let d = u2.y * ic + v2.y * id;
    let t = Affine { a, b, c, d, e: c0.x - (a * s0.x + c * s0.y), f: c0.y - (b * s0.x + d * s0.y) };
    t.is_finite().then_some(t)
}

impl State {
    /// Ends an open transform: `commit` keeps it as one undo step, otherwise the layer is put back.
    /// A plugin's layer's items are moved as items, by where the box's corners went, and the layer
    /// laid again exactly.
    pub(crate) fn end_transform(&mut self, commit: bool) {
        self.snap_pending = None;
        if let Some((layer, ids, laid)) = self.transform_items.take()
            && let Some(t) = self.transform.take()
        {
            let moved = corners_affine(t.source(), t.content()).filter(|_| commit);
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.end_transform(false);
            }
            // its pixels back as they were: as its items were laid, if they were then
            self.flush();
            if laid {
                self.trust_laid(layer);
            }
            if let Some(moved) = moved {
                let name = match self.transform_mode {
                    procraft_engine::transform::TransformMode::Uniform => "uniform transform",
                    _ => "non-uniform transform",
                };
                self.move_taken_items(layer, Some(&ids), moved, name);
            }
            return;
        }
        let within = if commit && self.transform.is_some() { self.changed_since_begin() } else { None };
        if self.transform.take().is_some()
            && let Some(canvas) = self.canvas.as_mut()
        {
            if commit {
                canvas.name_next_step(match self.transform_mode {
                    procraft_engine::transform::TransformMode::Freeform => "non-uniform transform",
                    procraft_engine::transform::TransformMode::Uniform => "uniform transform",
                    procraft_engine::transform::TransformMode::Distort => "distort transform",
                    procraft_engine::transform::TransformMode::Warp => "warp transform",
                });
            }
            match within {
                Some(within) => canvas.end_transform_within(commit, within),
                None => canvas.end_transform(commit),
            }
        }
    }

    /// Where the open transform or adjustment changed its layers, all it drew done first: the box
    /// around the pixels changed (a pixel if none were, still a step to undo); none if that can't
    /// be told.
    pub(crate) fn changed_since_begin(&mut self) -> Option<IRect> {
        self.flush();
        let (Some(canvas), Some(gpu)) = (self.canvas.as_ref(), self.gpu.as_ref()) else { return None };
        canvas.changed_box(gpu).map(|b| b.unwrap_or(IRect::new(0, 0, 1, 1)))
    }

    /// Redraws the layer with the content where the transform has it now.
    fn show_transform(&mut self) {
        let (Some(t), Some(canvas)) = (self.transform.as_ref(), self.canvas.as_mut()) else { return };
        if let Some(mesh) = t.warp() {
            canvas.set_transform_mesh(&warp::triangles(&mesh, t.source(), MESH_STEPS), self.interpolation.into());
        } else if let Some(to_source) = Homography::quad_to_quad(t.content(), t.source()) {
            // each layer pixel looks up where it came from
            canvas.set_transform(to_source.to_f32(), self.interpolation.into());
        }
    }

    pub(crate) fn transform_box(&self) -> Option<TransformBox> {
        let t = self.transform.as_ref()?;
        let flat = |points: &mut dyn Iterator<Item = &Vec2>| points.flat_map(|p| [p.x, p.y]).collect::<Vec<f32>>();
        let mesh = t.warp();
        Some(TransformBox {
            corners: flat(&mut t.frame().iter()),
            can_undo: t.can_undo(),
            mesh: mesh.map(|m| flat(&mut m.iter())).unwrap_or_default(),
            grid: mesh.map(|m| flat(&mut warp::grid_lines(&m, GRID_SAMPLES).iter().flatten())).unwrap_or_default(),
            width: t.size().0,
            height: t.size().1,
            angle: t.angle().to_degrees(),
            snap_lines: {
                let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
                let [x, y] = t.snapped();
                x.map(|x| [x, 0.0, x, h]).into_iter().chain(y.map(|y| [0.0, y, w, y])).flatten().collect()
            },
        })
    }

    /// What pulls the drag now that it reached window point `to`.
    fn guides(&mut self, to: Vec2) -> Guides {
        let now = std::time::Instant::now();
        let zoom = self.view.map_or(1.0, |v| v.zoom()).max(1e-6);
        let points = |px: f32| px / self.display_scale.max(1e-6);
        // the speed in points per 60 Hz frame since the last step
        let speed = self.last_drag.map_or(0.0, |(at, when)| {
            let ms = now.duration_since(when).as_secs_f32() * 1000.0;
            points((to.x - at.x).abs() + (to.y - at.y).abs()) * 16.7 / ms.max(1.0)
        });
        self.last_drag = Some((to, now));
        let g = self.transform_guides;
        Guides {
            magnetics: g.magnetics,
            lock_after: LOCK_AFTER_PT * self.display_scale / zoom,
            snap: (g.snapping && speed <= g.velocity).then(|| g.distance * self.display_scale / zoom),
            canvas: Vec2::new(self.doc.width() as f32, self.doc.height() as f32),
        }
    }

    /// Window pixels to canvas pixels.
    pub(crate) fn window_to_canvas(&self, x: f32, y: f32) -> Option<Vec2> {
        self.view?.screen_to_canvas(Vec2::new(x, y)).filter(|p| p.is_finite())
    }
}

impl CanvasSession {
    /// Whether two fingers at these window points take what the box holds: any of them on it,
    /// though never a warp's, whose fingers move the canvas.
    pub(crate) fn fingers_on_box(&self, fingers: impl Iterator<Item = (f32, f32)>) -> bool {
        let st = self.shared.lock();
        let Some(t) = st.transform.as_ref().filter(|_| st.transform_mode != TransformMode::Warp) else {
            return false;
        };
        let frame = t.frame();
        fingers.filter_map(|(x, y)| st.window_to_canvas(x, y)).any(|p| on_quad(frame, p))
    }

    /// Two fingers at these window points take what the box holds.
    pub(crate) fn transform_pinch_grab(&self, a: (f32, f32)) {
        let mut st = self.shared.lock();
        let Some(at) = st.window_to_canvas(a.0, a.1) else { return };
        if let Some(t) = st.transform.as_mut() {
            t.grab(Handle::Move, at);
        }
    }

    /// The fingers that took what the box holds at the window points `from` reached those at
    /// `to`: it goes with them. Returns the box.
    pub(crate) fn transform_pinch(&self, from: [(f32, f32); 2], to: [(f32, f32); 2]) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let canvas = |st: &State, [a, b]: [(f32, f32); 2]| {
            Some([st.window_to_canvas(a.0, a.1)?, st.window_to_canvas(b.0, b.1)?])
        };
        let (from, to) = (canvas(st, from)?, canvas(st, to)?);
        if st.transform.as_mut()?.pinch(from, to) {
            st.show_transform();
            self.shared.frame(st);
        }
        st.transform_box()
    }
}

/// Whether `p` is on the box `q`: inside it (a box is convex until distorted past that, and then
/// this is near enough for a finger).
fn on_quad([a, b, c, d]: [Vec2; 4], p: Vec2) -> bool {
    let sides = [(a, b), (b, c), (c, d), (d, a)].map(|(from, to)| (to - from).cross(p - from));
    sides.iter().all(|&s| s >= 0.0) || sides.iter().all(|&s| s <= 0.0)
}

#[uniffi::export]
impl CanvasSession {
    /// Starts transforming the current layer and returns the box around what it holds; none when
    /// there is nothing to transform (the layer is empty, locked or hidden). A transform already
    /// open is kept first.
    ///
    /// Other changes to the artwork keep an open transform first; strokes are ignored while one is
    /// open.
    pub fn transform_begin(&self) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let current = st.doc.current();
        let movable = |l: &Layer| !st.doc.is_locked(l.id) && l.visible && !l.drawn_from_data();
        // with layers picked, every paint layer chosen (those in a chosen group too) moves
        let chosen = st.doc.chosen();
        // a plugin's layer alone moves its items as items: those the selection takes
        st.transform_items = None;
        let shown = st.doc.layer(current).is_some_and(|l| l.visible) && !st.doc.is_locked(current);
        if chosen.len() <= 1 && st.plugin_layer_kind(current).is_some() && shown {
            let ids: Vec<u32> = st.taken_items(current).unwrap_or_default().iter().filter_map(|i| i.id).collect();
            if ids.is_empty() {
                return None;
            }
            st.transform_items = Some((current, ids, false));
        }
        let movable = |l: &Layer| movable(l) || st.transform_items.as_ref().is_some_and(|(id, ..)| *id == l.id);
        let targets: Vec<LayerId> = if chosen.len() > 1 {
            st.doc
                .layers()
                .iter()
                .filter(|l| l.kind == LayerKind::Paint && movable(l))
                .filter(|l| chosen.iter().any(|c| *c == l.id || st.doc.is_inside(l.id, *c)))
                .map(|l| l.id)
                .collect()
        } else {
            st.doc.layer(current).filter(|l| movable(l)).map(|_| current).into_iter().collect()
        };
        if targets.is_empty() {
            st.transform_items = None;
            return None;
        }
        st.flush();
        if let Some((layer, ..)) = st.transform_items {
            let laid = st.laid_current(layer);
            if let Some(items) = st.transform_items.as_mut() {
                items.2 = laid;
            }
        }
        let (width, height) = (st.doc.width(), st.doc.height());
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
        // only where they are painted (and the selection takes) is read back; all of them where
        // the GPU can't tell
        let boxes = canvas.painted_boxes(gpu, &targets);
        let full = IRect::from_size(width, height);
        let mut area = boxes
            .iter()
            .copied()
            .collect::<Option<Vec<_>>>()
            .map_or(full, |b| b.into_iter().reduce(IRect::union).unwrap_or(IRect::EMPTY));
        if let Some(taken) = st.selection.as_ref().and_then(crate::layer_ops::mask_rect) {
            area = area.intersect(taken);
        }
        if area.is_empty() {
            return None;
        }
        // what all of them hold together there
        let mut pixels: Vec<u8> = Vec::new();
        for id in &targets {
            let layer = canvas.read_layer_part(gpu, *id, area).ok()?;
            if pixels.is_empty() {
                pixels = layer;
            } else {
                for (p, q) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(layer.as_chunks::<4>().0) {
                    p[3] = p[3].max(q[3]);
                }
            }
        }
        // with a selection, only what it takes moves
        let (left, top, across) = (area.x0.max(0) as usize, area.y0.max(0) as usize, area.width().max(1) as usize);
        if let Some(mask) = st.selection.as_ref() {
            for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let at = (top + i / across) * mask.width as usize + left + i % across;
                let m = mask.data.get(at).copied().unwrap_or(0);
                px[3] = ((u16::from(px[3]) * u16::from(m) + 127) / 255) as u8;
            }
        }
        let ([x, y, w, h], hull) = content_shape(&pixels, area.width())?;
        let corner = Vec2::new(area.x0 as f32, area.y0 as f32);
        let (x, y, hull) = (x + corner.x, y + corner.y, hull.into_iter().map(|p| p + corner).collect());
        // the other layers shown are snapped onto by their bounds: found now with snapping on,
        // else when it comes on (each layer read on the GPU, most transforms needn't)
        let bounds =
            if st.transform_guides.snapping { snap_bounds(&st.doc, canvas, gpu, &targets) } else { Vec::new() };
        st.snap_pending = (!st.transform_guides.snapping).then(|| targets.clone());
        // each layer's mask moves with it (the box is the layers' own)
        let moved: Vec<LayerId> = targets.iter().flat_map(|&t| std::iter::once(t).chain(st.doc.mask_of(t))).collect();
        canvas.begin_transform_layers(&moved);
        let mut t = Transform::new(x, y, w, h, hull);
        t.set_snap_targets(&bounds);
        st.transform = Some(t);
        st.scene_flat_for_tool();
        self.shared.frame(st);
        st.transform_box()
    }

    /// Why a transform didn't begin ([`CanvasSession::transform_begin`] gave none), as the message
    /// says it: a text layer must be rasterized first, a selection holds nothing of the layer, or
    /// the layer is empty.
    pub fn transform_refusal(&self) -> String {
        let st = self.shared.lock();
        let current = st.doc.layer(st.doc.current());
        if current.is_some_and(|l| l.text.is_some()) {
            "Rasterize the text to transform it"
        } else if current.is_some_and(|l| l.plugin.is_some()) {
            "Rasterize the layer to transform it"
        } else if st.selection.is_some() {
            "Nothing is selected on this layer"
        } else {
            "This layer is empty"
        }
        .to_owned()
    }

    /// A drag of `handle` starts at window point (`x`, `y`).
    pub fn transform_grab(&self, handle: TransformHandle, x: f32, y: f32) {
        let mut st = self.shared.lock();
        let Some(at) = st.window_to_canvas(x, y) else { return };
        st.last_drag = None;
        if let Some(t) = st.transform.as_mut() {
            t.grab(handle.into(), at);
        }
    }

    /// The drag reached window point (`x`, `y`): the content follows. Returns the box.
    pub fn transform_drag(&self, x: f32, y: f32) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let to = st.window_to_canvas(x, y)?;
        let mode = st.transform_mode;
        let guides = st.guides(Vec2::new(x, y));
        if st.transform.as_mut()?.drag(mode, to, &guides) {
            st.show_transform();
            self.shared.frame(st);
        }
        st.transform_box()
    }

    /// The drag ends: its change becomes a step the transform's undo takes back.
    pub fn transform_release(&self) -> Option<TransformBox> {
        let mut st = self.shared.lock();
        st.transform.as_mut()?.release();
        st.transform_box()
    }

    /// The drag is abandoned (a second finger came down): the content goes back.
    pub fn transform_cancel_drag(&self) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.transform.as_mut()?.cancel_drag();
        st.show_transform();
        self.shared.frame(st);
        st.transform_box()
    }

    /// One of the toolbar's changes. Returns the box.
    pub fn transform_apply(&self, action: TransformAction) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (width, height) = (st.doc.width() as f32, st.doc.height() as f32);
        let t = st.transform.as_mut()?;
        match action {
            TransformAction::FlipHorizontal => t.flip_horizontal(),
            TransformAction::FlipVertical => t.flip_vertical(),
            TransformAction::Rotate45 => t.rotate(std::f32::consts::FRAC_PI_4),
            TransformAction::FitToCanvas => t.fit(width, height),
            TransformAction::Reset => t.reset(),
            TransformAction::Undo => {
                t.undo();
            }
        }
        st.show_transform();
        self.shared.frame(st);
        st.transform_box()
    }

    /// A tap on the canvas away from the box's nodes, at window point (`x`, `y`): the content
    /// moves one canvas pixel towards it. Returns the box.
    pub fn transform_nudge(&self, x: f32, y: f32) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let at = st.window_to_canvas(x, y)?;
        st.transform.as_mut()?.nudge(at);
        st.show_transform();
        self.shared.frame(st);
        st.transform_box()
    }

    /// Makes the box `width` × `height` canvas pixels (numeric entry; negative mirrors). Returns
    /// the box.
    pub fn transform_set_size(&self, width: f32, height: f32) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.transform.as_mut()?.set_size(width, height);
        st.show_transform();
        self.shared.frame(st);
        st.transform_box()
    }

    /// Turns the box to `degrees` from upright, clockwise (numeric entry). Returns the box.
    pub fn transform_set_angle(&self, degrees: f32) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.transform.as_mut()?.set_angle(degrees.to_radians());
        st.show_transform();
        self.shared.frame(st);
        st.transform_box()
    }

    /// Magnetics and snapping from now on.
    pub fn transform_set_guides(&self, guides: GuideSettings) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.transform_guides = guides;
        // snapping on during a transform: what its box snaps onto found now
        if guides.snapping
            && st.transform.is_some()
            && let Some(targets) = st.snap_pending.take()
            && let (Some(canvas), Some(gpu)) = (st.canvas.as_ref(), st.gpu.as_ref())
        {
            let bounds = snap_bounds(&st.doc, canvas, gpu, &targets);
            if let Some(t) = st.transform.as_mut() {
                t.set_snap_targets(&bounds);
            }
        }
    }

    /// How the handles act from now on; a warp lays its mesh over the content. Returns the box.
    pub fn transform_set_kind(&self, kind: TransformKind) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.transform_mode = kind.into();
        if kind == TransformKind::Warp
            && let Some(t) = st.transform.as_mut()
        {
            t.start_warp();
            st.show_transform();
            self.shared.frame(st);
        }
        st.transform_box()
    }

    /// How the transformed pixels are resampled; redrawn at once.
    pub fn transform_set_interpolation(&self, interpolation: Interpolation) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.interpolation = interpolation;
        if st.transform.is_some() {
            st.show_transform();
            self.shared.frame(st);
        }
    }

    /// Ends the transform: `commit` keeps it as one undo step, otherwise the layer is put back.
    pub fn transform_end(&self, commit: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(t) = st.transform.as_ref() {
            // a selection moves with what it took
            let moved = match (commit, st.selection.as_ref()) {
                (true, Some(mask)) => match t.warp() {
                    Some(mesh) => Some(mask.warped(&warp::triangles(&mesh, t.source(), MESH_STEPS))),
                    None => Homography::quad_to_quad(t.content(), t.source()).map(|h| mask.moved(|p| h.apply(p))),
                },
                _ => None,
            };
            st.end_transform(commit);
            st.flush();
            if let Some(moved) = moved {
                let before = st.selection.replace(moved);
                st.record_selection(before, "move");
            }
            st.revision += 1;
            st.scene_back_from_tool();
            self.shared.frame(st);
        }
    }

    /// The view as six numbers `[a, b, c, d, e, f]`: canvas pixel (x, y) is shown at window pixel
    /// (a·x + c·y + e, b·x + d·y + f). The identity before the canvas is shown.
    pub fn view_matrix(&self) -> Vec<f32> {
        let st = self.shared.lock();
        let m = st.view.map_or(Affine::IDENTITY, |v| v.to_screen());
        vec![m.a, m.b, m.c, m.d, m.e, m.f]
    }

    /// The view the canvas opens with in a window of `width` × `height` pixels (as it was left,
    /// else fitted), as [`CanvasSession::view_matrix`] gives it; once shown, the view it has.
    pub fn opening_view_matrix(&self, width: u32, height: u32) -> Vec<f32> {
        let st = self.shared.lock();
        let window = (width.max(1), height.max(1));
        let view = st.view.unwrap_or_else(|| {
            st.saved_view.and_then(|v| v.view(window)).unwrap_or_else(|| {
                procraft_engine::view::View::fit_inside((st.doc.width(), st.doc.height()), window, st.fit_insets)
            })
        });
        let m = view.to_screen();
        vec![m.a, m.b, m.c, m.d, m.e, m.f]
    }
}

/// How far the rotate node sits above the box's top edge, and the box-turning node below its
/// bottom (points); how near a node a touch must land to take it, and a warp's control point.
const ROTATE_LIFT: f32 = 34.5;
const TURN_DROP: f32 = 35.5;
const GRAB_RADIUS: f32 = 37.5;
const MESH_GRAB_RADIUS: f32 = 15.0;

/// The transform box on screen, in window pixels (x, y pairs): its corners (top left, top right,
/// bottom right, bottom left), the middles of its edges (top, right, bottom, left), the rotate node
/// above it, the node below it that turns the box alone, and the box's up direction.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScreenBox {
    pub corners: Vec<f32>,
    pub edges: Vec<f32>,
    pub rotate: Vec<f32>,
    pub turn: Vec<f32>,
    pub up: Vec<f32>,
}

/// Canvas point (`x`, `y`) through the view `m` (`[a, b, c, d, e, f]`), in window pixels.
fn to_screen(x: f32, y: f32, m: &[f32]) -> Option<(f32, f32)> {
    let [a, b, c, d, e, f] = *m.get(..6)? else { return None };
    Some((a * x + c * y + e, b * x + d * y + f))
}

/// The warp's control points a touch can take: the corners, or all 16 with the advanced mesh.
#[uniffi::export]
pub fn shown_mesh_points(advanced: bool) -> Vec<u8> {
    if advanced { (0..16).collect() } else { vec![0, 3, 12, 15] }
}

/// Where a box with `corners` (canvas pixels) shows through the view `view`; `density` is pixels a
/// point.
#[uniffi::export]
pub fn screen_box(corners: Vec<f32>, view: Vec<f32>, density: f32) -> Option<ScreenBox> {
    let pts: Vec<(f32, f32)> =
        corners.as_chunks::<2>().0.iter().take(4).map(|&[x, y]| to_screen(x, y, &view)).collect::<Option<_>>()?;
    let [c0, c1, c2, c3] = pts[..] else { return None };
    let corners = [c0, c1, c2, c3];
    let mid = |a: (f32, f32), b: (f32, f32)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let edges = [mid(c0, c1), mid(c1, c2), mid(c2, c3), mid(c3, c0)];
    let (top, bottom) = (edges[0], edges[2]);
    let upward = (top.0 - bottom.0, top.1 - bottom.1);
    let length = upward.0.hypot(upward.1);
    let up = if length > 1e-3 { (upward.0 / length, upward.1 / length) } else { (0.0, -1.0) };
    // the box's right, a quarter turn clockwise from up
    let right = (-up.1, up.0);
    let rotate = (top.0 + up.0 * ROTATE_LIFT * density, top.1 + up.1 * ROTATE_LIFT * density);
    let turn = (
        bottom.0 - up.0 * TURN_DROP * density + right.0 * density,
        bottom.1 - up.1 * TURN_DROP * density + right.1 * density,
    );
    let flat = |ps: &[(f32, f32)]| ps.iter().flat_map(|&(x, y)| [x, y]).collect();
    Some(ScreenBox {
        corners: flat(&corners),
        edges: flat(&edges),
        rotate: vec![rotate.0, rotate.1],
        turn: vec![turn.0, turn.1],
        up: vec![up.0, up.1],
    })
}

/// The handle a touch at window point (`x`, `y`) takes on the box `shown`: the nearest node within
/// reach, otherwise a move.
#[uniffi::export]
pub fn handle_at(shown: ScreenBox, x: f32, y: f32, density: f32) -> TransformHandle {
    let point = |v: &[f32], i: usize| Some((*v.get(2 * i)?, *v.get(2 * i + 1)?));
    let mut nodes =
        vec![(TransformHandle::Rotate, point(&shown.rotate, 0)), (TransformHandle::TurnBox, point(&shown.turn, 0))];
    for i in 0..4u8 {
        nodes.push((TransformHandle::Corner { index: i }, point(&shown.corners, usize::from(i))));
    }
    for i in 0..4u8 {
        nodes.push((TransformHandle::Edge { index: i }, point(&shown.edges, usize::from(i))));
    }
    let nearest = nodes
        .into_iter()
        .filter_map(|(h, at)| at.map(|(px, py)| (h, (px - x).hypot(py - y))))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    match nearest {
        Some((h, d)) if d <= GRAB_RADIUS * density => h,
        _ => TransformHandle::Move,
    }
}

/// In a warp with control points `mesh` (canvas pixels) seen through `view`: the shown control
/// point a touch at window point (`x`, `y`) takes, otherwise the surface under it (a move).
#[uniffi::export]
pub fn mesh_handle_at(mesh: Vec<f32>, view: Vec<f32>, x: f32, y: f32, density: f32, advanced: bool) -> TransformHandle {
    if mesh.len() < 32 {
        return TransformHandle::Move;
    }
    let nearest = shown_mesh_points(advanced)
        .into_iter()
        .filter_map(|i| {
            let k = usize::from(i);
            let (px, py) = to_screen(*mesh.get(2 * k)?, *mesh.get(2 * k + 1)?, &view)?;
            Some((i, (px - x).hypot(py - y)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    match nearest {
        Some((index, d)) if d <= MESH_GRAB_RADIUS * density => TransformHandle::Mesh { index },
        _ => TransformHandle::Move,
    }
}

#[cfg(test)]
mod handles {
    use super::*;

    #[test]
    fn a_transform_that_cannot_begin_says_why() {
        let s = CanvasSession::new(32, 32);
        assert_eq!(s.transform_refusal(), "This layer is empty");
        s.select_invert();
        assert_eq!(s.transform_refusal(), "Nothing is selected on this layer");
    }

    #[test]
    fn a_box_shows_its_nodes_and_a_touch_takes_the_nearest() {
        let view = vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let shown = screen_box(vec![0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0], view.clone(), 1.0).unwrap();
        assert_eq!(shown.edges, vec![50.0, 0.0, 100.0, 50.0, 50.0, 100.0, 0.0, 50.0]);
        assert_eq!(shown.rotate, vec![50.0, -34.5]);
        assert_eq!(shown.turn, vec![51.0, 135.5]);
        assert_eq!(handle_at(shown.clone(), 3.0, 4.0, 1.0), TransformHandle::Corner { index: 0 });
        assert_eq!(handle_at(shown.clone(), 50.0, -30.0, 1.0), TransformHandle::Rotate);
        assert_eq!(handle_at(shown, 50.0, 50.0, 1.0), TransformHandle::Move);
        // a warp's corner points, and its inner ones only with the advanced mesh
        let mesh: Vec<f32> = (0..16).flat_map(|i| [(i % 4) as f32 * 30.0, (i / 4) as f32 * 30.0]).collect();
        assert_eq!(
            mesh_handle_at(mesh.clone(), view.clone(), 1.0, 1.0, 1.0, false),
            TransformHandle::Mesh { index: 0 }
        );
        assert_eq!(mesh_handle_at(mesh.clone(), view.clone(), 31.0, 31.0, 1.0, false), TransformHandle::Move);
        assert_eq!(mesh_handle_at(mesh, view, 31.0, 31.0, 1.0, true), TransformHandle::Mesh { index: 5 });
    }
}
