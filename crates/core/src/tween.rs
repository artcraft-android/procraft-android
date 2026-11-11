//! In-betweens (a plugin's): Animation Assist's In-between hands two frames' pixels to the plugin
//! on the `inbetween` hook and puts the frame it draws just after the first, as one undo step. How
//! the frame between is drawn is all the plugin's.

use procraft_engine::animation;
use procraft_engine::document::{LayerId, LayerKind};
use procraft_engine::geom::IRect;

use crate::CanvasSession;
use crate::plugin_run::MOST_BETWEEN_MEMORY;

#[uniffi::export]
impl CanvasSession {
    /// Animation Assist's In-between (a plugin's): a frame drawn between frame `id` and the next by
    /// the plugin on the `inbetween` hook, put just after `id` and made current, as one undo step:
    /// a long call, not for the main thread. None without such a plugin, with no next frame, if
    /// either frame is more than one layer, nothing is drawn on either, the part they're drawn in
    /// is more than the plugin may hold, the plugin draws nothing, or there is no room for a layer.
    pub fn add_inbetween(&self, id: u64) -> Option<u64> {
        let hooks = self.shared.lock().hooks.clone()?;
        if !hooks.tweens() {
            return None;
        }
        let this = LayerId(id);
        // the two frames read under the lock, drawn by the plugin without it
        let (next, within, from_this, from_next) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let frames = animation::frames(&st.doc);
            let next = frames.iter().position(|f| *f == this).and_then(|i| frames.get(i + 1)).copied()?;
            let one_layer = |l: LayerId| st.doc.layer(l).is_some_and(|l| l.kind == LayerKind::Paint);
            if !one_layer(this) || !one_layer(next) || !st.doc.has_room(1) {
                return None;
            }
            let boxes: Vec<IRect> = [st.painted_box(this), st.painted_box(next)].into_iter().flatten().collect();
            let within = boxes.into_iter().reduce(IRect::union)?;
            // the two frames and the one between, with room to spare, in what the plugin may hold
            let bytes = within.width() as usize * within.height() as usize * 4;
            if bytes.saturating_mul(3) > MOST_BETWEEN_MEMORY - (16 << 20) {
                return None;
            }
            (next, within, st.layer_part(this, within)?, st.layer_part(next, within)?)
        };
        let (w, h) = (within.width() as usize, within.height() as usize);
        let part = hooks.inbetween(&from_this, &from_next, w, h)?;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        // the frames as they were read (an undo or a change meanwhile: nothing made)
        let frames = animation::frames(&st.doc);
        let still = frames.iter().position(|f| *f == this).and_then(|i| frames.get(i + 1)) == Some(&next);
        if !still || !st.doc.has_room(1) {
            return None;
        }
        let before = st.doc.clone();
        st.doc.set_current(this);
        let made = st.doc.add_layer();
        let n = animation::frames(&st.doc).len();
        st.doc.rename(made, &format!("Frame {n}"));
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("in-between");
            canvas.write_layer_parts_and_document(vec![(made, within, part)], before);
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(made.0)
    }
}
