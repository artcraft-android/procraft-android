//! A layer's own actions from its options: selecting what is on it, copying it, filling it with the
//! colour, inverting it, and making it the reference layer.

use procraft_engine::document::LayerId;
use procraft_engine::selection::Mask;

use crate::layers::ThumbnailImage;
use crate::{CanvasSession, State};

impl State {
    /// A layer's pixels, everything queued drawn first.
    pub(crate) fn layer_pixels(&mut self, id: LayerId) -> Option<Vec<u8>> {
        self.finish_stroke();
        self.settle();
        self.flush();
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return None };
        canvas.read_layer(gpu, id).ok()
    }

    /// Replaces an unlocked layer's pixels as one undo step.
    fn rewrite_layer(&mut self, id: LayerId, pixels: Vec<u8>) {
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.write_layer(id, pixels);
        }
        self.revision += 1;
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Selects what is on layer `id` (by its coverage), as one undo step.
    pub fn select_layer_contents(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(pixels) = st.layer_pixels(LayerId(id)) else { return false };
        let (w, h) = (st.doc.width(), st.doc.height());
        let mut mask = Mask::empty(w, h);
        for (m, px) in mask.data.iter_mut().zip(pixels.as_chunks::<4>().0) {
            *m = px[3];
        }
        let before = st.selection.take();
        st.selection = Some(mask);
        st.record_selection(before, "select layer contents");
        self.shared.frame(st);
        true
    }

    /// Layer `id`'s pixels, to paste later.
    pub fn copy_layer_contents(&self, id: u64) -> Option<ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let rgba = st.layer_pixels(LayerId(id))?;
        Some(ThumbnailImage { width: st.doc.width(), height: st.doc.height(), rgba })
    }

    /// Fills layer `id` with the colour, within the selection if there is one (only where it
    /// already has paint when alpha-locked), as one undo step.
    pub fn fill_layer(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let id = LayerId(id);
        let Some(layer) = st.doc.layer(id).cloned() else { return false };
        if layer.locked || layer.text.is_some() {
            return false;
        }
        let Some(mut pixels) = st.layer_pixels(id) else { return false };
        let [r, g, b] = st.color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round());
        let mask = st.selection.clone();
        for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let k = mask.as_ref().map_or(1.0, |m| f32::from(m.data.get(i).copied().unwrap_or(0)) / 255.0);
            // the colour over what is there, as much as the selection takes; alpha-locked, only
            // as much as is there
            let a = if layer.alpha_lock { f32::from(px[3]) / 255.0 } else { 1.0 };
            let fill = [r * a, g * a, b * a, a * 255.0];
            for (c, f) in px.iter_mut().zip(fill) {
                *c = (f32::from(*c) + (f - f32::from(*c)) * k).round().clamp(0.0, 255.0) as u8;
            }
        }
        st.rewrite_layer(id, pixels);
        self.shared.frame(st);
        true
    }

    /// Inverts layer `id`'s colours (its coverage kept), as one undo step.
    pub fn invert_layer(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let id = LayerId(id);
        if st.doc.layer(id).is_none_or(|l| l.locked || l.text.is_some()) {
            return false;
        }
        let Some(mut pixels) = st.layer_pixels(id) else { return false };
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = px[3];
            for c in px.iter_mut().take(3) {
                *c = a.saturating_sub(*c);
            }
        }
        st.rewrite_layer(id, pixels);
        self.shared.frame(st);
        true
    }

    /// Makes layer `id` the reference layer, or (not `on`) leaves none.
    pub fn set_layer_reference(&self, id: u64, on: bool) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let before = st.doc.clone();
        if !st.doc.set_reference(LayerId(id), on) {
            return false;
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.record_document(before);
        }
        st.revision += 1;
        true
    }
}
