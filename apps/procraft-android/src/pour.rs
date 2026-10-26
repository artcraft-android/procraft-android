//! Dropping a colour on the canvas: the colour poured into the area joined to a point whose colour
//! is near the point's own, measured on the reference layer if there is one. While the finger stays
//! down the threshold can change and the pour is redone from the layer as it was; it ends as one
//! undo step.

use procraft_engine::document::LayerId;
use procraft_engine::fill::{self, Flood};
use procraft_engine::geom::Vec2;
use procraft_engine::selection::Mask;

use crate::CanvasSession;

/// A pour under way.
pub(crate) struct Pour {
    layer: LayerId,
    /// The layer's pixels as the pour began.
    before: Vec<u8>,
    flood: Flood,
    /// Measured on another layer (the reference), so poured over this one.
    elsewhere: bool,
    colour: [f32; 3],
    selection: Option<Mask>,
    alpha_lock: bool,
    /// The threshold last poured, in thousandths.
    shown: i32,
}

impl Pour {
    /// The layer's pixels with the colour poured at `threshold`.
    fn poured(&self, threshold: f32) -> Vec<u8> {
        let mut mask = self.flood.mask(threshold);
        if let Some(sel) = &self.selection {
            for (m, &s) in mask.data.iter_mut().zip(&sel.data) {
                *m = ((u16::from(*m) * u16::from(s) + 127) / 255) as u8;
            }
        }
        let mut pixels = self.before.clone();
        if self.elsewhere {
            fill::pour_over(&mut pixels, &mask, self.colour, self.alpha_lock);
        } else {
            fill::pour(&mut pixels, &mask, self.flood.colour, self.colour, self.alpha_lock);
        }
        pixels
    }
}

fn thousandths(threshold: f32) -> i32 {
    (threshold.clamp(0.0, 1.0) * 1000.0).round() as i32
}

#[uniffi::export]
impl CanvasSession {
    /// Pours colour (`r`, `g`, `b`, 0..1) at window point (`x`, `y`) with `threshold` (0..1) on
    /// the current layer, within the selection if there is one. False if nothing was poured (off
    /// the canvas, or the layer locked or text). The pour stays open for
    /// [`CanvasSession::pour_threshold`] until [`CanvasSession::pour_end`].
    pub fn pour_begin(&self, x: f32, y: f32, r: f32, g: f32, b: f32, threshold: f32) -> bool {
        self.pour_end(true);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(p) = st.view.and_then(|v| v.screen_to_canvas(Vec2::new(x, y))) else { return false };
        let (w, h) = (st.doc.width(), st.doc.height());
        if !(p.x >= 0.0 && p.y >= 0.0 && p.x < w as f32 && p.y < h as f32) {
            return false;
        }
        let current = st.doc.current();
        let Some(layer) = st.doc.layer(current).cloned() else { return false };
        if layer.locked || layer.text.is_some() {
            return false;
        }
        let Some(before) = st.current_pixels() else { return false };
        let seed = (p.x as u32, p.y as u32);
        // measured on the reference layer when another layer is it
        let reference = st.doc.reference().filter(|&r| r != current);
        let flood = match reference {
            Some(r) => st.layer_pixels(r).and_then(|px| Flood::new(w, h, &px, seed)),
            None => Flood::new(w, h, &before, seed),
        };
        let Some(flood) = flood else { return false };
        let colour = [r, g, b].map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 });
        let pour = Pour {
            layer: current,
            before,
            flood,
            elsewhere: reference.is_some(),
            colour,
            selection: st.selection.clone(),
            alpha_lock: layer.alpha_lock,
            shown: thousandths(threshold),
        };
        let pixels = pour.poured(threshold);
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.hold_layer(current);
            canvas.put_layer(current, pixels);
        }
        st.pour = Some(pour);
        self.shared.frame(st);
        true
    }

    /// The open pour redone at `threshold` (0..1).
    pub fn pour_threshold(&self, threshold: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(pour) = st.pour.as_mut() else { return };
        let t = thousandths(threshold);
        if t == pour.shown {
            return;
        }
        pour.shown = t;
        let (layer, pixels) = (pour.layer, pour.poured(threshold));
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.put_layer(layer, pixels);
        }
        self.shared.frame(st);
    }

    /// Ends the open pour: `keep` makes it one undo step; otherwise the layer goes back to how it
    /// was.
    pub fn pour_end(&self, keep: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.pour.take().is_none() {
            return;
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.release_layer(keep, None);
        }
        st.revision += 1;
        self.shared.frame(st);
    }
}
