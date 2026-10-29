//! Transform: the current layer's content moved, scaled, rotated or distorted as a whole through
//! the box around it, shown live and kept as one undo step.

use procraft_engine::document::{Document, Layer, LayerId, LayerKind};
use procraft_engine::geom::{Affine, Homography, Vec2};
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

/// The box around the pixels that aren't clear (`x`, `y`, `width`, `height`), if any.
fn content_bounds(pixels: &[u8], width: u32) -> Option<[f32; 4]> {
    let row_len = width as usize * 4;
    if row_len == 0 {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (y, row) in pixels.chunks_exact(row_len).enumerate() {
        let (px, _) = row.as_chunks::<4>();
        let Some(first) = px.iter().position(|&[.., a]| a != 0) else { continue };
        let last = px.iter().rposition(|&[.., a]| a != 0).unwrap_or(first) + 1;
        x0 = x0.min(first);
        x1 = x1.max(last);
        y0 = y0.min(y);
        y1 = y + 1;
    }
    (x0 < x1).then(|| [x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32])
}

/// Whether `l` shows: it and every group it is in visible.
fn shown(doc: &Document, l: &Layer) -> bool {
    l.visible && l.parent.and_then(|p| doc.layer(p)).is_none_or(|p| shown(doc, p))
}

/// How many other layers a moved box snaps onto at most (each is read back once, at the start).
const MAX_SNAP_LAYERS: usize = 16;

impl State {
    /// Ends an open transform: `commit` keeps it as one undo step, otherwise the layer is put back.
    pub(crate) fn end_transform(&mut self, commit: bool) {
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
            canvas.end_transform(commit);
        }
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

    fn transform_box(&self) -> Option<TransformBox> {
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
    fn window_to_canvas(&self, x: f32, y: f32) -> Option<Vec2> {
        self.view?.screen_to_canvas(Vec2::new(x, y)).filter(|p| p.is_finite())
    }
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
        let movable = |l: &Layer| !l.locked && l.visible && l.text.is_none();
        // with layers picked, every paint layer chosen (those in a chosen group too) moves
        let chosen = st.doc.chosen();
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
            return None;
        }
        st.flush();
        let width = st.doc.width();
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
        // what all of them hold together
        let mut pixels: Vec<u8> = Vec::new();
        for id in &targets {
            let layer = canvas.read_layer(gpu, *id).ok()?;
            if pixels.is_empty() {
                pixels = layer;
            } else {
                for (p, q) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(layer.as_chunks::<4>().0) {
                    p[3] = p[3].max(q[3]);
                }
            }
        }
        // with a selection, only what it takes moves
        if let Some(mask) = st.selection.as_ref() {
            for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&mask.data) {
                px[3] = ((u16::from(px[3]) * u16::from(m) + 127) / 255) as u8;
            }
        }
        let ([x, y, w, h], hull) = content_shape(&pixels, width)?;
        // the other layers shown, nearest the top first, are snapped onto by their bounds
        let others: Vec<_> = st
            .doc
            .layers()
            .iter()
            .rev()
            .filter(|l| !targets.contains(&l.id) && l.kind == LayerKind::Paint && shown(&st.doc, l))
            .take(MAX_SNAP_LAYERS)
            .map(|l| l.id)
            .collect();
        let bounds: Vec<[f32; 4]> =
            others.into_iter().filter_map(|id| content_bounds(&canvas.read_layer(gpu, id).ok()?, width)).collect();
        canvas.begin_transform_layers(&targets);
        let mut t = Transform::new(x, y, w, h, hull);
        t.set_snap_targets(&bounds);
        st.transform = Some(t);
        self.shared.frame(st);
        st.transform_box()
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
        self.shared.lock().transform_guides = guides;
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
