//! Cropping and resizing the canvas. The canvas is made again at its new size with every layer
//! resampled; the canvas from before is kept, history and all, and undoing past the new canvas's
//! first step puts it back.

use procraft_engine::crop::{crop, resample};
use procraft_engine::view::View;
use procraft_gpu::Canvas;

use crate::{CanvasSession, State};

/// How long the canvas takes to shrink into place as crop begins.
const CROP_SHRINK_SECONDS: f32 = 0.3;

impl State {
    /// Whether redoing would put an undone crop back: only while nothing new was done since.
    pub(crate) fn crop_redo_ready(&self) -> bool {
        match (&self.crop_redo, &self.canvas) {
            (Some((_, _, steps)), Some(c)) => !c.can_redo() && c.steps_made() == *steps,
            _ => false,
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Crops and resizes the canvas, as one undo step: keeps the rectangle `x, y, width, height`
    /// (canvas pixels; it may reach past the edges) turned by `angle` radians about its middle.
    /// With `resample`, what is kept is scaled to `new_width` × `new_height` pixels; otherwise the
    /// canvas is the rectangle's size. False if the canvas isn't shown yet.
    #[allow(clippy::too_many_arguments)]
    pub fn crop_canvas(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        angle: f32,
        resample_to: bool,
        new_width: u32,
        new_height: u32,
    ) -> bool {
        self.text_end(true);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let (Some(old), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return false };
        old.render(gpu, &mut st.doc, None);
        let (sw, sh) = (st.doc.width(), st.doc.height());
        let c = crop([x, y, width, height], angle, resample_to.then_some((new_width, new_height)));
        let doc = st.doc.resized(c.width, c.height);
        let mut layers = Vec::new();
        for id in st.doc.pixel_layer_ids() {
            match old.read_layer(gpu, id) {
                Ok(pixels) => layers.push((id, resample(&pixels, sw, sh, &c))),
                Err(e) => {
                    log::error!("cropping: {e}");
                    return false;
                }
            }
        }
        let mut canvas = match Canvas::new(gpu, &doc) {
            Ok(c) => c,
            Err(e) => {
                log::error!("cropping: {e}");
                return false;
            }
        };
        for (id, pixels) in &layers {
            let _ = canvas.upload_layer(gpu, *id, pixels);
        }
        let old_canvas = st.canvas.replace(canvas);
        let old_doc = std::mem::replace(&mut st.doc, doc);
        if let Some(old_canvas) = old_canvas {
            st.crop_undo = Some((old_canvas, old_doc));
        }
        st.crop_redo = None;
        st.selection = None;
        st.view = Some(st.fitted_view());
        st.reconfigure = true;
        st.revision += 1;
        self.shared.frame(st);
        true
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The canvas's colour profile, by name.
    pub fn profile(&self) -> String {
        self.shared.lock().doc.profile_name()
    }

    /// Puts a canvas just made in the colour profile named `name` (nothing on it is moved). False
    /// if no profile has that name.
    pub fn set_new_profile(&self, name: String) -> bool {
        let Some((profile, icc)) = crate::colour::resolve(&name) else { return false };
        let mut st = self.shared.lock();
        st.doc.profile = profile;
        st.doc.icc = icc;
        self.shared.frame(&mut st);
        true
    }

    /// Puts the canvas in the colour profile named `name`, as one undo step: every layer's colours
    /// and the background are turned into it, so they look the same where it can show them. False
    /// if no profile has that name, the canvas is in it already or isn't shown yet.
    pub fn change_profile(&self, name: String) -> bool {
        let Some((to, icc)) = crate::colour::resolve(&name) else { return false };
        self.text_end(true);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let from = st.doc.profile;
        if st.doc.profile_name() == name {
            return false;
        }
        // the pixels move between the RGB profiles they are in (a CMYK canvas's: Display P3)
        let mut next = procraft_engine::document::Document::new(1, 1);
        next.change_to(to, icc.clone());
        let (old_rgb, new_rgb) = (st.doc.rgb_profile(), next.rgb_profile());
        let table = (st.doc.icc.is_some() || icc.is_some())
            .then(|| (old_rgb != new_rgb).then(|| procraft_engine::cms::Lut::build(&old_rgb, &new_rgb, None, 33)))
            .flatten();
        st.finish_stroke();
        st.settle();
        st.flush();
        let ids: Vec<_> = st.doc.colour_layer_ids().collect();
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return false };
        let mut layers = Vec::new();
        for id in ids {
            match canvas.read_layer(gpu, id) {
                Ok(mut pixels) => {
                    match &table {
                        Some(t) => t.apply_rgba(&mut pixels, true),
                        None if icc.is_none() && st.doc.icc.is_none() => from.convert_pixels(&mut pixels, to),
                        None => {}
                    }
                    layers.push((id, pixels));
                }
                Err(e) => {
                    log::error!("changing the colour profile: {e}");
                    return false;
                }
            }
        }
        let before = st.doc.clone();
        st.doc.change_to(to, icc);
        canvas.name_next_step(&format!("Set color profile \"{name}\""));
        canvas.write_layers_and_document(layers, before);
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    /// The canvas's dots per inch (1 to 9999), as an undo step.
    pub fn set_dpi(&self, dpi: f32) -> bool {
        if !dpi.is_finite() {
            return false;
        }
        let dpi = dpi.clamp(1.0, 9999.0);
        self.change("DPI", |d| ((d.dpi - dpi).abs() > f32::EPSILON).then(|| d.dpi = dpi)).is_some()
    }

    /// Fits the whole canvas in the window, then shrinks it by `scale` (0.05 to 1) about the
    /// window's middle, as crop does so its frame's handles can be reached. The canvas shrinks
    /// into place over [`CROP_SHRINK_SECONDS`], slowing to a stop.
    pub fn fit_canvas_scaled(&self, scale: f32) {
        use procraft_engine::geom::{Affine, Vec2};
        let scale = if scale.is_finite() { scale.clamp(0.05, 1.0) } else { 1.0 };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (ww, wh) = st.window_size;
        let fit = View::fit((st.doc.width(), st.doc.height()), (ww, wh)).to_screen();
        let (cx, cy) = (ww as f32 / 2.0, wh as f32 / 2.0);
        let shrunk = fit
            .then(Affine::translate(Vec2::new(-cx, -cy)))
            .then(Affine::scale(scale))
            .then(Affine::translate(Vec2::new(cx, cy)));
        // shrinking into place, slowing to a stop
        let to = View::from_affine(shrunk);
        st.view_motion = st.view.map(|from| crate::ViewMotion {
            from,
            to,
            start: std::time::Instant::now(),
            curve: crate::ViewCurve::EaseOut(CROP_SHRINK_SECONDS),
        });
        if st.view_motion.is_none() {
            st.view = Some(to);
        }
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }
}

impl CanvasSession {
    /// Undo past the first step of a cropped canvas puts the canvas from before back; redo, while
    /// nothing new was done, crops again. False when there is no such step (the canvas's own
    /// history comes first).
    pub(crate) fn swap_crop(&self, undo: bool) -> bool {
        {
            // nothing to swap (or an adjustment open, whose undo is its own): leave it all be
            let st = self.shared.lock();
            if st.adjusting || (st.crop_undo.is_none() && st.crop_redo.is_none()) {
                return false;
            }
        }
        self.text_end(true);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        // queued work becomes steps first, so the counts are current
        st.settle();
        st.flush();
        if undo {
            if st.canvas.as_ref().is_none_or(Canvas::can_undo) {
                return false;
            }
            let Some((canvas, doc)) = st.crop_undo.take() else { return false };
            let cropped = st.canvas.replace(canvas);
            let cropped_doc = std::mem::replace(&mut st.doc, doc);
            let steps = st.canvas.as_ref().map_or(0, Canvas::steps_made);
            st.crop_redo = cropped.map(|c| (c, cropped_doc, steps));
        } else {
            if !st.crop_redo_ready() {
                return false;
            }
            let Some((canvas, doc, _)) = st.crop_redo.take() else { return false };
            let before = st.canvas.replace(canvas);
            let before_doc = std::mem::replace(&mut st.doc, doc);
            st.crop_undo = before.map(|c| (c, before_doc));
        }
        st.selection = None;
        st.view = Some(st.fitted_view());
        st.reconfigure = true;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        st.revision += 1;
        self.shared.frame(st);
        true
    }
}
