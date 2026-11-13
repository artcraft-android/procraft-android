//! A layer's own actions from its options: selecting what is on it, copying it, filling it with the
//! colour, inverting it, and making it the reference layer.

use procraft_engine::document::LayerId;
use procraft_engine::geom::IRect;
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

    /// The canvas as composited (premultiplied RGBA8, rows top to bottom): every layer as it is
    /// seen, everything queued drawn first.
    pub(crate) fn composite_pixels(&mut self) -> Option<Vec<u8>> {
        self.finish_stroke();
        self.settle();
        self.flush();
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return None };
        canvas.read_composite_without(gpu, &mut self.doc, &[]).ok()
    }

    /// A layer's pixels inside `rect` (rows top to bottom), everything queued drawn first.
    pub(crate) fn layer_part(&mut self, id: LayerId, rect: IRect) -> Option<Vec<u8>> {
        self.finish_stroke();
        self.settle();
        self.flush();
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return None };
        canvas.read_layer_part(gpu, id, rect).ok()
    }

    /// Where layer `id` is painted (as the GPU finds it, everything queued drawn first); none if
    /// nowhere, the whole canvas if the GPU can't tell.
    pub(crate) fn painted_box(&mut self, id: LayerId) -> Option<IRect> {
        self.finish_stroke();
        self.settle();
        self.flush();
        let whole = IRect::from_size(self.doc.width(), self.doc.height());
        let (Some(canvas), Some(gpu)) = (self.canvas.as_ref(), self.gpu.as_ref()) else { return None };
        match canvas.painted_boxes_or_why(gpu, &[id]).pop() {
            Some(Ok(painted)) => painted,
            Some(Err(_)) | None => Some(whole),
        }
    }

    /// Replaces an unlocked layer's pixels inside `rect` with `part` (its rows), as one undo step.
    pub(crate) fn rewrite_layer(&mut self, id: LayerId, rect: IRect, part: Vec<u8>) {
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.write_layer_part(id, rect, part);
        }
        self.revision += 1;
    }
}

/// What `mask` takes inside `rect` (on it), row by row.
pub(crate) fn mask_part(mask: &Mask, rect: IRect) -> Vec<u8> {
    let w = mask.width as usize;
    let (x0, x1) = (rect.x0.max(0) as usize, rect.x1.max(0) as usize);
    let mut out = Vec::with_capacity(rect.width() as usize * rect.height() as usize);
    for y in rect.y0.max(0) as usize..rect.y1.max(0) as usize {
        out.extend_from_slice(mask.data.get(y * w + x0..y * w + x1).unwrap_or_default());
    }
    out
}

/// The rectangle around what `mask` takes; none when it takes nothing.
pub(crate) fn mask_rect(mask: &Mask) -> Option<IRect> {
    let [x, y, w, h] = mask.bounds()?.map(|v| i32::try_from(v).unwrap_or(i32::MAX));
    Some(IRect::new(x, y, x.saturating_add(w), y.saturating_add(h)))
}

/// The rectangle around the pixels of a picture `width` wide (RGBA) that aren't clear; none when
/// all are (as the GPU's [`procraft_gpu::Canvas::painted_boxes`] finds a layer's).
pub(crate) fn painted_rect(pixels: &[u8], width: u32) -> Option<IRect> {
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
    let n = |v: usize| i32::try_from(v).unwrap_or(i32::MAX);
    (x0 < x1).then(|| IRect::new(n(x0), n(y0), n(x1), n(y1)))
}

#[uniffi::export]
impl CanvasSession {
    /// Selects what is on layer `id` (by its coverage), as one undo step.
    pub fn select_layer_contents(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (w, h) = (st.doc.width(), st.doc.height());
        let mut mask = Mask::empty(w, h);
        // only where it is painted is read back (nothing selected if nowhere)
        if let Some(rect) = st.painted_box(LayerId(id)) {
            let Some(pixels) = st.layer_part(LayerId(id), rect) else { return false };
            let (x0, across) = (rect.x0.max(0) as usize, rect.width() as usize);
            for (row, y) in pixels.chunks_exact(across * 4).zip(rect.y0.max(0) as usize..) {
                let start = y * w as usize + x0;
                if let Some(line) = mask.data.get_mut(start..start + across) {
                    for (m, px) in line.iter_mut().zip(row.as_chunks::<4>().0) {
                        *m = px[3];
                    }
                }
            }
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
        if st.doc.is_locked(id) || layer.text.is_some() {
            return false;
        }
        let [r, g, b] = st.color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round());
        let mask = st.reach_selection();
        // only what is selected changes, and with the alpha locked only what is painted: only
        // that is read back and filled
        let (w, h) = (st.doc.width(), st.doc.height());
        let selected = mask.as_ref().map_or(Some(IRect::from_size(w, h)), mask_rect);
        let painted = if layer.alpha_lock { st.painted_box(id) } else { Some(IRect::from_size(w, h)) };
        let Some(rect) = selected.zip(painted).map(|(a, b)| a.intersect(b)).filter(|r| !r.is_empty()) else {
            return false;
        };
        // nothing selected and the alpha not locked: the colour everywhere, on the GPU (nothing
        // read back or sent)
        if mask.is_none() && !layer.alpha_lock {
            let Some(canvas) = st.canvas.as_mut() else { return false };
            canvas.name_next_step("fill layer");
            canvas.fill_layer(id, [r, g, b].map(|c| c as u8));
            st.revision += 1;
            self.shared.frame(st);
            return true;
        }
        let Some(mut pixels) = st.layer_part(id, rect) else { return false };
        let taken = mask.as_ref().map(|m| mask_part(m, rect));
        for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let k = taken.as_ref().map_or(1.0, |m| f32::from(m.get(i).copied().unwrap_or(0)) / 255.0);
            // the colour over what is there, as much as the selection takes; alpha-locked, only
            // as much as is there
            let a = if layer.alpha_lock { f32::from(px[3]) / 255.0 } else { 1.0 };
            let fill = [r * a, g * a, b * a, a * 255.0];
            for (c, f) in px.iter_mut().zip(fill) {
                *c = (f32::from(*c) + (f - f32::from(*c)) * k).round().clamp(0.0, 255.0) as u8;
            }
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("fill layer");
        }
        st.rewrite_layer(id, rect, pixels);
        self.shared.frame(st);
        true
    }

    /// Inverts layer `id`'s colours (its coverage kept), as one undo step.
    pub fn invert_layer(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let id = LayerId(id);
        if st.doc.is_locked(id) || st.doc.layer(id).is_none_or(|l| l.text.is_some()) {
            return false;
        }
        // only what is painted changes, inverted on the GPU (nothing read back)
        let Some(rect) = st.painted_box(id) else { return false };
        let Some(canvas) = st.canvas.as_mut() else { return false };
        canvas.name_next_step("invert");
        canvas.invert_layer(id, rect);
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    /// Makes layer `id` the reference layer, or (not `on`) leaves none: one step, named as
    /// measured ("Undo reference layer on").
    pub fn set_layer_reference(&self, id: u64, on: bool) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let before = st.doc.clone();
        if !st.doc.set_reference(LayerId(id), on) {
            return false;
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step(if on { "reference layer on" } else { "reference layer off" });
            canvas.record_document(before);
        }
        st.revision += 1;
        true
    }
}
