//! Selections: shapes drawn on the window become a selection on the canvas, which painting keeps
//! inside, and which can be inverted, feathered, saved, filled or copied to a layer of its own.

use procraft_engine::geom::Vec2;
use procraft_engine::selection::{ColourFlood, Combine, Mask, feather_radius};

use crate::{CanvasSession, State};

/// How a new shape joins the selection there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionJoin {
    Add,
    Remove,
}

impl From<SelectionJoin> for Combine {
    fn from(j: SelectionJoin) -> Combine {
        match j {
            SelectionJoin::Add => Combine::Add,
            SelectionJoin::Remove => Combine::Remove,
        }
    }
}

/// How the selection shows: bands over what is not selected, or what is selected tinted (while
/// selecting by colour) or covered (while its threshold is being dragged).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionDisplay {
    Stripes,
    Tint,
    Fill,
}

/// Points around an ellipse drawn as a polygon.
const ELLIPSE_POINTS: usize = 128;

impl State {
    /// Shows the selection on the canvas (or none when nothing is selected).
    fn show_selection(&mut self) {
        let mask = self.selection.as_ref().filter(|m| !m.is_empty());
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.set_selection(gpu, mask.map(|m| m.data.as_slice()));
        }
        if mask.is_none() {
            self.selection = None;
        }
    }

    /// The selection as it is now becomes one undo step, called `name`, that puts `before` back.
    pub(crate) fn record_selection(&mut self, before: Option<Mask>, name: &str) {
        self.finish_stroke();
        self.settle();
        if self.selection.as_ref().is_some_and(Mask::is_empty) {
            self.selection = None;
        }
        if self.selection == before {
            return;
        }
        let coverage = self.selection.as_ref().map(|m| m.data.as_slice());
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.name_next_step(name);
            canvas.change_selection(gpu, &mut self.doc, coverage, before.map(|m| m.data));
            self.revision += 1;
        }
    }

    /// Joins `shape` to the selection; `record` makes it an undo step (otherwise it is only shown,
    /// as while a colour threshold is dragged).
    fn join(&mut self, shape: Mask, how: SelectionJoin, record: bool) {
        self.feather_base = None;
        let before = self.selection.clone();
        match (self.selection.as_mut(), how) {
            (Some(m), _) => m.combine(&shape, how.into()),
            (None, SelectionJoin::Add) => self.selection = Some(shape),
            (None, SelectionJoin::Remove) => {}
        }
        if record {
            let name = match (how, before.is_some()) {
                (SelectionJoin::Remove, _) => "subtract from selection",
                (SelectionJoin::Add, true) => "add to selection",
                (SelectionJoin::Add, false) => "draw selection",
            };
            self.record_selection(before, name);
        } else {
            self.show_selection();
        }
    }

    /// Takes the selection back from the canvas (after an undo or redo).
    pub(crate) fn sync_selection(&mut self) {
        let (w, h) = (self.doc.width(), self.doc.height());
        self.selection =
            self.canvas.as_ref().and_then(|c| c.selection()).map(|c| Mask { width: w, height: h, data: c.to_vec() });
        self.feather_base = None;
    }

    /// The canvas pixel under window point (`x`, `y`), if there is one: through the view, or on a
    /// shown model the texel of the texture set painted on.
    pub(crate) fn window_texel(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        if self.scene_shown() {
            return self.scene_texel(x, y);
        }
        let p = self.view?.screen_to_canvas(Vec2::new(x, y))?;
        let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
        (p.x >= 0.0 && p.y >= 0.0 && p.x < w && p.y < h).then_some((p.x as u32, p.y as u32))
    }

    /// Window points to canvas points.
    fn to_canvas_points(&self, window: &[Vec2]) -> Option<Vec<Vec2>> {
        let view = self.view?;
        window.iter().map(|&p| view.screen_to_canvas(p).filter(|c| c.is_finite())).collect()
    }

    /// The current layer's pixels, after any queued work.
    pub(crate) fn current_pixels(&mut self) -> Option<Vec<u8>> {
        self.finish_stroke();
        self.settle();
        self.flush();
        let current = self.doc.current();
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return None };
        canvas.read_layer(gpu, current).ok()
    }
}

#[uniffi::export]
impl CanvasSession {
    /// A rectangle (or with `ellipse`, the ellipse in it) between window points (`x0`, `y0`) and
    /// (`x1`, `y1`) joins the selection. Drawn on the window, so it turns with the view.
    pub fn select_shape(&self, ellipse: bool, x0: f32, y0: f32, x1: f32, y1: f32, join: SelectionJoin) {
        let (a, b) = (Vec2::new(x0, y0), Vec2::new(x1, y1));
        let window: Vec<Vec2> = if ellipse {
            let (c, r) = (a.lerp(b, 0.5), (b - a) * 0.5);
            (0..ELLIPSE_POINTS)
                .map(|i| {
                    let t = i as f32 / ELLIPSE_POINTS as f32 * std::f32::consts::TAU;
                    Vec2::new(c.x + r.x * t.cos(), c.y + r.y * t.sin())
                })
                .collect()
        } else {
            vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]
        };
        self.select_window_polygon(&window, join);
    }

    /// The polygon through window points `points` (x, y pairs) joins the selection.
    pub fn select_polygon(&self, points: Vec<f32>, join: SelectionJoin) {
        let window: Vec<Vec2> = points.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
        self.select_window_polygon(&window, join);
    }

    /// Selects by colour: the pixels of the current layer joined to window point (`x`, `y`) whose
    /// colour is within `threshold` (0..1) of its colour join the selection. While `live` (the
    /// threshold being dragged) each call replaces the last one's shape; the last call is not live.
    pub fn select_by_colour(&self, x: f32, y: f32, threshold: f32, join: SelectionJoin, live: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(seed) = st.window_texel(x, y) else { return };
        let (w, h) = (st.doc.width(), st.doc.height());
        // the layer is read and measured from the seed once for the whole drag
        let (pixels, flood) = match st.colour_source.take() {
            Some((px, flood)) if flood.seed() == seed => (px, Some(flood)),
            Some((px, _)) => (px, None),
            None => {
                let Some(px) = st.current_pixels() else { return };
                st.selection_before = Some(st.selection.clone());
                (px, None)
            }
        };
        let Some(flood) = flood.or_else(|| {
            let flood = ColourFlood::new(w, h, &pixels, seed)?;
            if live {
                flood.work_out_reach();
            }
            Some(flood)
        }) else {
            return;
        };
        let shape = flood.mask(threshold);
        let before = st.selection_before.clone().flatten();
        st.selection = before.clone();
        if live {
            st.join(shape, join, false);
            st.colour_source = Some((pixels, flood));
        } else {
            // the drag ends: one step from before it
            st.join(shape, join, false);
            st.selection_before = None;
            st.record_selection(before, "Automatic Selection");
        }
        self.shared.frame(st);
    }

    /// Selection mask visibility, 0..1 (a quarter by default).
    pub fn set_selection_visibility(&self, visibility: f32) {
        let mut st = self.shared.lock();
        st.selection_visibility = if visibility.is_finite() { visibility.clamp(0.0, 1.0) } else { 0.25 };
        self.shared.frame(&mut st);
    }

    /// Brush scaling: relative to the screen (true) or to the canvas.
    pub fn set_brush_scaling(&self, on_screen: bool) {
        self.shared.lock().brush_on_screen = on_screen;
    }

    pub fn set_selection_display(&self, display: SelectionDisplay) {
        let mut st = self.shared.lock();
        st.selection_display = display;
        self.shared.frame(&mut st);
    }

    /// Selects what was not selected, and the other way round.
    pub fn select_invert(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (w, h) = (st.doc.width(), st.doc.height());
        let before = st.selection.clone();
        let mut m = st.selection.take().unwrap_or_else(|| Mask::empty(w, h));
        st.feather_base = None;
        m.invert();
        st.selection = Some(m);
        st.record_selection(before, "invert");
        self.shared.frame(st);
    }

    /// Softens the selection's edges by `amount` (0..1, the feather popover's): each call feathers
    /// the selection as it was before the first, until the selection is changed some other way.
    /// A big selection takes a while, so the canvas is let go of meanwhile (a host may call this
    /// off its main thread, one call at a time); a selection changed some other way meanwhile
    /// stays as it was changed.
    pub fn select_feather(&self, amount: f32) {
        let (base, mut room) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            // the selection as it was before the first, kept once
            if st.feather_base.is_none() {
                st.feather_base = st.selection.clone().map(|m| (m, Vec::new()));
            }
            let Some((base, room)) = st.feather_base.as_mut() else { return };
            (base.clone(), std::mem::take(room))
        };
        let mut m = base.clone();
        m.feather_in(feather_radius(amount), &mut room);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some((_, kept)) = st.feather_base.as_mut().filter(|(now, _)| *now == base) else { return };
        *kept = room;
        st.selection = Some(m);
        st.show_selection();
        self.shared.frame(st);
    }

    /// The feather popover closed: its feathering becomes one undo step.
    pub fn select_feather_done(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some((base, _)) = st.feather_base.take() {
            st.record_selection(Some(base), "feather");
            self.shared.frame(st);
        }
    }

    /// Selects nothing (painting goes anywhere again).
    pub fn select_none(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let before = st.selection.take();
        st.feather_base = None;
        st.record_selection(before, "clear selection");
        self.shared.frame(st);
    }

    pub fn has_selection(&self) -> bool {
        self.shared.lock().selection.is_some()
    }

    /// Keeps the selection to load again later; returns how many are kept.
    pub fn save_selection(&self) -> u32 {
        let mut st = self.shared.lock();
        if let Some(m) = st.selection.clone() {
            st.saved_selections.push(m);
        }
        u32::try_from(st.saved_selections.len()).unwrap_or(u32::MAX)
    }

    /// Makes a kept selection the selection again.
    pub fn load_selection(&self, index: u32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(m) = st.saved_selections.get(index as usize).cloned() else { return false };
        let before = st.selection.replace(m);
        st.feather_base = None;
        st.record_selection(before, "load selection");
        self.shared.frame(st);
        true
    }

    pub fn delete_saved_selection(&self, index: u32) {
        let mut st = self.shared.lock();
        if (index as usize) < st.saved_selections.len() {
            st.saved_selections.remove(index as usize);
        }
    }

    /// A kept selection as a small picture fitting `max_width` × `max_height`: white where it
    /// selects, black where it does not.
    pub fn saved_selection_thumbnail(
        &self,
        index: u32,
        max_width: u32,
        max_height: u32,
    ) -> Option<crate::layers::ThumbnailImage> {
        let st = self.shared.lock();
        let m = st.saved_selections.get(index as usize)?;
        let (w, h) = (m.width.max(1) as f32, m.height.max(1) as f32);
        let scale = (max_width.max(1) as f32 / w).min(max_height.max(1) as f32 / h);
        let (tw, th) = (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1));
        let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
        for y in 0..th {
            for x in 0..tw {
                // the mean of the mask pixels this one covers, sampled on a small grid
                let mut sum = 0u32;
                for sy in 0..3 {
                    for sx in 0..3 {
                        let mx = ((x as f32 + (sx as f32 + 0.5) / 3.0) / scale) as u32;
                        let my = ((y as f32 + (sy as f32 + 0.5) / 3.0) / scale) as u32;
                        let i =
                            (my.min(m.height.saturating_sub(1)) * m.width + mx.min(m.width.saturating_sub(1))) as usize;
                        sum += u32::from(m.data.get(i).copied().unwrap_or(0));
                    }
                }
                let v = (sum / 9) as u8;
                rgba.extend([v, v, v, 255]);
            }
        }
        Some(crate::layers::ThumbnailImage { width: tw, height: th, rgba })
    }

    pub fn saved_selection_count(&self) -> u32 {
        u32::try_from(self.shared.lock().saved_selections.len()).unwrap_or(u32::MAX)
    }

    /// Fills the selection on the current layer with the paint colour, as one undo step.
    pub fn fill_selection(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let current = st.doc.current();
        if st.doc.layer(current).is_none() || st.doc.is_locked(current) {
            return;
        }
        let Some(mask) = st.selection.clone() else { return };
        // only what is selected changes
        let Some(rect) = crate::layer_ops::mask_rect(&mask) else { return };
        let Some(mut pixels) = st.layer_part(current, rect) else { return };
        let [r, g, b] = st.color.map(|c| c.clamp(0.0, 1.0));
        let alpha_lock = st.doc.layer(current).is_some_and(|l| l.alpha_lock);
        for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&crate::layer_ops::mask_part(&mask, rect)) {
            let [pr, pg, pb, pa] = px;
            if m == 0 {
                continue;
            }
            // the colour over what is there, as much as is selected (only where there is paint,
            // with the alpha locked)
            let k = f32::from(m) / 255.0;
            let a = f32::from(*pa) / 255.0;
            let (cover, out_a) = if alpha_lock { (k * a, a) } else { (k, k + a * (1.0 - k)) };
            for (dst, src) in [(&mut *pr, r), (&mut *pg, g), (&mut *pb, b)] {
                *dst = to_byte(src * cover + f32::from(*dst) / 255.0 * (1.0 - k));
            }
            *pa = to_byte(out_a);
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.write_layer_part(current, rect, pixels);
            st.flush();
            st.revision += 1;
            self.shared.frame(st);
        }
    }

    /// Copies what is selected on the current layer to a new layer above it, made current;
    /// returns its id.
    pub fn copy_selection_to_layer(&self) -> Option<u64> {
        // only what the selection's box holds is read back and copied
        let part = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let mask = st.selection.clone()?;
            match crate::layer_ops::mask_rect(&mask) {
                Some(rect) => {
                    let mut pixels = st.layer_part(st.doc.current(), rect)?;
                    for (px, &m) in
                        pixels.as_chunks_mut::<4>().0.iter_mut().zip(&crate::layer_ops::mask_part(&mask, rect))
                    {
                        let k = u16::from(m);
                        for c in px.iter_mut() {
                            *c = ((u16::from(*c) * k + 127) / 255) as u8;
                        }
                    }
                    Some((rect, pixels))
                }
                None => None,
            }
        };
        let id = self.add_layer_quietly();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let (Some(canvas), Some(gpu), Some((rect, pixels))) = (st.canvas.as_mut(), st.gpu.as_ref(), part) {
            let _ = canvas.upload_layer_part(gpu, procraft_engine::document::LayerId(id), rect, &pixels);
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(id)
    }
}

impl CanvasSession {
    fn select_window_polygon(&self, window: &[Vec2], join: SelectionJoin) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let shape = if st.scene_shown() {
            let Some(shape) = st.scene_mask(window) else { return };
            shape
        } else {
            let Some(points) = st.to_canvas_points(window) else { return };
            Mask::polygon(st.doc.width(), st.doc.height(), &points)
        };
        st.join(shape, join, true);
        self.shared.frame(st);
    }
}

fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}
