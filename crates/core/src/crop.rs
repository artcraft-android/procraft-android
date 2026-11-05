//! Cropping and resizing the canvas. The canvas is made again at its new size with every layer
//! resampled; the canvas from before is kept, history and all, and undoing past the new canvas's
//! first step puts it back.

use procraft_engine::crop::{crop, resample};
use procraft_engine::view::View;
use procraft_gpu::{Canvas, Gpu};

use crate::{CanvasSession, State};

/// How long the canvas takes to shrink into place as crop begins.
const CROP_SHRINK_SECONDS: f32 = 0.3;

/// What the apps tell when the GPU's device is lost (see [`CanvasSession::take_failure`]).
const LOST_SAID: &str =
    "The graphics stopped working. Go back to the gallery and open the artwork again: it comes back as last saved.";

/// What the apps tell when a smudge stroke couldn't be begun.
const SMUDGE_SAID: &str = "There isn't enough graphics memory to smudge on this canvas.";

/// What a canvas holds besides its layers, in layers' worth: its stroke (two), composite and
/// caches (four).
const CANVAS_LAYERS: u64 = 7;

/// What the apps tell when a crop couldn't be made.
fn crop_failure_said(e: &procraft_gpu::GpuError) -> String {
    match e {
        procraft_gpu::GpuError::Lost => LOST_SAID,
        _ => "The canvas couldn't be cropped, most likely for lack of graphics memory. Nothing was changed.",
    }
    .to_owned()
}

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
    /// canvas is the rectangle's size. False if the canvas isn't shown yet, or if the GPU couldn't
    /// make the new canvas (out of memory, its device lost): the canvas is then left as it was,
    /// and [`CanvasSession::take_failure`] says why.
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
        let (sw, sh) = (st.doc.width(), st.doc.height());
        let c = crop([x, y, width, height], angle, resample_to.then_some((new_width, new_height)));
        let doc = st.doc.resized(c.width, c.height);
        let ids: Vec<_> = st.doc.pixel_layer_ids().collect();
        // the old canvas stays for undo, so this is all more: refused while there is still room,
        // not left to the system to find out
        let budget = old.resident_budget();
        let per = u64::from(c.width) * u64::from(c.height) * 4;
        let resident = ids.len().min(procraft_gpu::most_resident(budget, c.width, c.height)) as u64;
        if crate::device::short_of_memory((resident + CANVAS_LAYERS) * per) {
            log::error!("cropping: {} layers of {per} bytes won't fit in the memory free", resident);
            st.crop_edit.failure = Some(crop_failure_said(&procraft_gpu::GpuError::OutOfMemory(String::new())));
            return false;
        }
        // the new canvas made beside the old, which stays as it is unless every layer went across
        // as it should: a texture the GPU had no memory for would otherwise come over empty
        let made = gpu.checked(|| -> Result<Canvas, procraft_gpu::GpuError> {
            old.render(gpu, &mut st.doc, None);
            let mut canvas = Canvas::unfilled(gpu, &doc)?;
            canvas.set_resident_budget(budget);
            for id in ids {
                // copied (moved by whole pixels) or resampled on the GPU, nothing read back
                let m = c.to_old;
                let room = canvas.has_room();
                let on_gpu = room
                    && match c.whole_pixels() {
                        Some(offset) => canvas.copy_layer_from(gpu, old, id, offset),
                        None => canvas.resample_layer_from(gpu, old, id, [m.a, m.b, m.c, m.d, m.e, m.f]),
                    };
                if !on_gpu {
                    let pixels = resample(&old.read_layer(gpu, id)?, sw, sh, &c);
                    // past what fits, kept compressed until needed
                    if room {
                        canvas.upload_layer(gpu, id, &pixels)?;
                    } else {
                        canvas.park_pixels(id, &pixels)?;
                    }
                }
            }
            canvas.fill_rest(gpu, &doc);
            #[cfg(test)]
            tests::fail_here(gpu);
            Ok(canvas)
        });
        let canvas = match made.and_then(|made| made) {
            Ok(canvas) => canvas,
            Err(e) => {
                log::error!("cropping: {e}");
                st.crop_edit.failure = Some(crop_failure_said(&e));
                // memory is short: the buffers kept for reads go too
                gpu.let_go_of_kept_buffers();
                return false;
            }
        };
        let old_canvas = st.canvas.replace(canvas);
        let old_doc = std::mem::replace(&mut st.doc, doc);
        if let Some(mut old_canvas) = old_canvas {
            // the crop is a new step: what was undone before it can't be redone
            old_canvas.forget_redo();
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
    /// What the GPU couldn't do for the canvas, worded for the app to tell, once each: a crop or a
    /// smudge there was no memory for (the canvas is as it was), or its device lost (what it held
    /// is gone, and the last save is what there is). None if all is well.
    pub fn take_failure(&self) -> Option<String> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.gpu.as_ref().is_some_and(Gpu::is_lost) && !std::mem::replace(&mut st.crop_edit.lost_told, true) {
            st.crop_edit.failure = None;
            return Some(LOST_SAID.to_owned());
        }
        let refused = st.canvas.as_mut().is_some_and(Canvas::take_smudge_refused);
        st.crop_edit.failure.take().or_else(|| refused.then(|| SMUDGE_SAID.to_owned()))
    }

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

    /// Puts the canvas in the colour profile named `name`, as one undo step. As measured, nothing
    /// is turned: its colours (the layers', the background's, the text's) keep their numbers and
    /// take the new profile's look. False if no profile has that name or the canvas is in it.
    pub fn change_profile(&self, name: String) -> bool {
        let Some((to, icc)) = crate::colour::resolve(&name) else { return false };
        self.text_end(true);
        self.change(&format!("Set color profile \"{name}\""), |d| {
            (d.profile_name() != name).then(|| d.assign_profile(to, icc))
        })
        .is_some()
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

#[cfg(test)]
mod tests {
    use procraft_engine::view::View;
    use procraft_gpu::{Gpu, wgpu};

    use crate::{CanvasSession, PenSample, PointerKind};

    std::thread_local! {
        static FAIL_NEXT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Makes this thread's next crop fail on the GPU, as one whose new canvas's textures there
    /// was no memory for does.
    fn fail_next_crop() {
        FAIL_NEXT.with(|f| f.set(true));
    }

    /// Where a crop's work fails, if [`fail_next_crop`] said so: a texture the GPU won't make.
    pub(super) fn fail_here(gpu: &Gpu) {
        if FAIL_NEXT.with(|f| f.replace(false)) {
            let most = gpu.device.limits().max_texture_dimension_2d;
            drop(gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("too large"),
                size: wgpu::Extent3d { width: most + 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }));
        }
    }

    fn pen(x: f32, y: f32, t: f64) -> PenSample {
        PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: t, roll: 0.0 }
    }

    /// A canvas `side` square on a window its size, with a stroke on each of three layers.
    fn painted(side: u32) -> Option<std::sync::Arc<CanvasSession>> {
        let gpu = Gpu::headless().ok()?;
        let s = CanvasSession::new(side, side);
        {
            let mut st = s.shared.lock();
            st.window_size = (side, side);
            st.view = Some(View::fit((side, side), (side, side)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        s.set_brush_size(0.3);
        for k in 0..3 {
            if k > 0 {
                s.add_layer();
            }
            s.set_color(0.3 * k as f32, 0.2, 0.9 - 0.3 * k as f32);
            let y = side as f32 * (k + 1) as f32 / 4.0;
            s.stroke_begin(PointerKind::Finger, vec![pen(10.0, y, 0.0)]);
            s.stroke_add((1..12).map(|i| pen(10.0 + i as f32 * side as f32 / 13.0, y, i as f64 * 8.0)).collect());
            s.stroke_end(vec![]);
        }
        Some(s)
    }

    /// Each layer's pixels, and the canvas's size.
    fn contents(s: &CanvasSession) -> (Vec<Vec<u8>>, (u32, u32)) {
        let layers = s.layers().into_iter().map(|l| s.copy_layer_contents(l.id).map(|c| c.rgba).unwrap_or_default());
        (layers.collect(), (s.info().width, s.info().height))
    }

    #[test]
    fn a_crop_the_gpu_cant_make_leaves_the_canvas_as_it_was_and_says_so() {
        let Some(s) = painted(120) else { return };
        let dpi = s.info().dpi;
        let before = contents(&s);
        assert!(before.0.iter().all(|px| px.as_chunks::<4>().0.iter().any(|p| p[3] != 0)), "every layer painted");
        let dir = std::env::temp_dir().join(format!("procraft-crop-failed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        s.save(path.clone()).unwrap();
        // by whole pixels and resampled, through Done with a DPI typed in, and straight
        for resampled in [false, true] {
            fail_next_crop();
            s.crop_begin();
            s.crop_set_dpi(600.0);
            s.crop_set_resample(resampled);
            s.crop_set_size(Some(80), Some(70));
            assert!(!s.crop_end(true));
            assert!(s.take_failure().is_some_and(|why| why.contains("memory")));
            assert_eq!(s.take_failure(), None, "told once");
            assert_eq!(contents(&s), before);
            assert_eq!(s.info().dpi, dpi, "nor the DPI");
        }
        fail_next_crop();
        assert!(!s.crop_canvas(10.0, 10.0, 60.5, 50.0, 0.3, false, 0, 0));
        assert!(s.take_failure().is_some());
        assert_eq!(contents(&s), before);
        // no step was made
        assert_ne!(s.undo().as_deref(), Some("Crop canvas"));
        s.redo();
        // saved and opened again as it was
        s.save(path.clone()).unwrap();
        let opened = CanvasSession::open(path).unwrap();
        let Some(gpu) = s.shared.lock().gpu.clone() else { return };
        opened.use_gpu(gpu);
        {
            let mut st = opened.shared.lock();
            st.window_size = (120, 120);
            st.view = Some(View::fit((120, 120), (120, 120)));
            st.ensure_canvas();
        }
        assert_eq!(contents(&opened), before);
        // and a crop the GPU can make, made
        assert!(s.crop_canvas(10.0, 10.0, 60.0, 50.0, 0.0, false, 0, 0));
        assert_eq!(contents(&s).1, (60, 50));
        assert_eq!(s.take_failure(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_artwork_opened_on_a_lost_graphics_device_stays_as_it_was_saved() {
        let Some(s) = painted(80) else { return };
        let before = contents(&s);
        let dir = std::env::temp_dir().join(format!("procraft-open-lost-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        s.save(path.clone()).unwrap();
        let Ok(lost) = Gpu::headless() else { return };
        lost.device.destroy();
        let _ = lost.device.poll(wgpu::PollType::wait_indefinitely());
        // shown on it: no canvas of empty layers, and it says so
        let opened = CanvasSession::open(path.clone()).unwrap();
        opened.use_gpu(lost);
        {
            let mut st = opened.shared.lock();
            st.ensure_canvas();
            assert!(st.canvas.is_none());
            assert!(st.error.is_some());
        }
        // whatever a save does with it, what was saved stays
        let _ = opened.save(path.clone());
        let again = CanvasSession::open(path).unwrap();
        let Some(gpu) = s.shared.lock().gpu.clone() else { return };
        again.use_gpu(gpu);
        assert_eq!(contents(&again), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failure_elsewhere_doesnt_stop_the_canvas_being_made() {
        let Some(s) = painted(120) else { return };
        let dir = std::env::temp_dir().join(format!("procraft-open-after-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        s.save(path.clone()).unwrap();
        let opened = CanvasSession::open(path).unwrap();
        // a window's surface that couldn't be made, as the app came back
        opened.shared.lock().error = Some("no surface".to_owned());
        let Some(gpu) = s.shared.lock().gpu.clone() else { return };
        opened.use_gpu(gpu);
        let mut st = opened.shared.lock();
        st.ensure_canvas();
        assert!(st.canvas.is_some());
        drop(st);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crop_past_the_memory_free_is_refused_and_says_so() {
        let Some(s) = painted(120) else { return };
        let before = contents(&s);
        // nothing like enough left beside what the system keeps
        crate::device::MEMORY_FREE.with(|f| f.set(Some(100 << 20)));
        assert!(!s.crop_canvas(10.0, 10.0, 60.0, 50.0, 0.0, true, 4000, 4000));
        assert!(s.take_failure().is_some_and(|why| why.contains("memory")));
        assert_eq!(contents(&s), before);
        // a small canvas needs next to nothing, and is the way out of being short
        assert!(s.crop_canvas(10.0, 10.0, 60.0, 50.0, 0.0, false, 0, 0));
        crate::device::MEMORY_FREE.with(|f| f.set(None));
        assert_eq!(contents(&s).1, (60, 50));
        assert_eq!(s.take_failure(), None);
    }

    #[test]
    fn a_crop_past_the_budget_parks_what_doesnt_fit_and_loses_nothing() {
        let (Some(fits), Some(parks)) = (painted(120), painted(120)) else { return };
        let per = 80 * 70 * 4;
        // room for one layer of the new size: the other two go compressed
        for (x, angle) in [(10.0, 0.0), (10.5, 0.2)] {
            if let Some(c) = parks.shared.lock().canvas.as_mut() {
                c.set_resident_budget(per);
            }
            assert!(fits.crop_canvas(x, 10.0, 80.0, 70.0, angle, false, 0, 0));
            assert!(parks.crop_canvas(x, 10.0, 80.0, 70.0, angle, false, 0, 0));
            assert_eq!(parks.shared.lock().canvas.as_ref().map(procraft_gpu::Canvas::parked_layers), Some(2));
            assert_eq!(contents(&parks), contents(&fits));
            assert!(parks.undo().is_some());
            assert!(fits.undo().is_some());
        }
    }

    #[test]
    fn what_the_system_has_free_is_read_from_its_memory_report() {
        let report = "MemTotal:       11543576 kB\nMemFree:         1248464 kB\nMemAvailable:    6577012 kB\n";
        assert_eq!(crate::device::available_in(report), Some(6_577_012 * 1024));
        assert_eq!(crate::device::available_in("MemTotal: 1 kB\n"), None);
    }

    #[test]
    fn a_crop_on_a_lost_graphics_device_changes_nothing() {
        let Some(s) = painted(80) else { return };
        let gpu = s.shared.lock().gpu.clone().unwrap();
        gpu.device.destroy();
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        assert!(gpu.is_lost());
        // resampled (whose bindings wgpu panicked making on a lost device) and by whole pixels
        assert!(!s.crop_canvas(5.0, 5.0, 40.5, 30.0, 0.2, false, 0, 0));
        assert!(!s.crop_canvas(5.0, 5.0, 40.0, 30.0, 0.0, false, 0, 0));
        assert!(s.take_failure().is_some_and(|why| why.contains("open the artwork again")));
        assert_eq!((s.info().width, s.info().height), (80, 80));
    }
}
