//! Dropping a colour on the canvas: the colour poured into the area joined to a point whose colour
//! is near the point's own, measured on the reference layer if there is one. While the finger stays
//! down the threshold can change and the pour is redone from the layer as it was; it ends as one
//! undo step. Recolor keeps a pour open while its point moves and its colour changes.

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
    /// The area measured from the point; none while the point is off the canvas.
    flood: Option<Flood>,
    /// The reference layer's pixels when measured on it, so poured over this one.
    elsewhere: Option<Vec<u8>>,
    colour: [f32; 3],
    selection: Option<Mask>,
    alpha_lock: bool,
    /// The threshold last poured, in thousandths.
    shown: i32,
}

impl Pour {
    /// The layer's pixels with the colour poured at `threshold`.
    fn poured(&self, threshold: f32) -> Vec<u8> {
        let Some(flood) = &self.flood else { return self.before.clone() };
        let mut mask = flood.mask(threshold);
        if let Some(sel) = &self.selection {
            for (m, &s) in mask.data.iter_mut().zip(&sel.data) {
                *m = ((u16::from(*m) * u16::from(s) + 127) / 255) as u8;
            }
        }
        let mut pixels = self.before.clone();
        if self.elsewhere.is_some() {
            fill::pour_over(&mut pixels, &mask, self.colour, self.alpha_lock);
        } else {
            fill::pour(&mut pixels, &mask, flood.colour, self.colour, self.alpha_lock);
        }
        pixels
    }

    /// The area measured again from canvas point `seed`, on the reference layer or the layer as
    /// it was.
    fn measure(&mut self, width: u32, height: u32, seed: (u32, u32)) {
        let sample = self.elsewhere.as_deref().unwrap_or(&self.before);
        self.flood = Flood::new(width, height, sample, seed);
    }

    /// The pour as it now is, for the layer.
    fn shown(&self) -> Vec<u8> {
        self.poured(self.shown as f32 / 1000.0)
    }
}

fn thousandths(threshold: f32) -> i32 {
    (threshold.clamp(0.0, 1.0) * 1000.0).round() as i32
}

fn unit(c: f32) -> f32 {
    if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 }
}

/// The canvas pixel under window point (`x`, `y`), if it is on the canvas.
fn seed_at(st: &crate::State, x: f32, y: f32) -> Option<(u32, u32)> {
    let p = st.view.and_then(|v| v.screen_to_canvas(Vec2::new(x, y)))?;
    let (w, h) = (st.doc.width(), st.doc.height());
    (p.x >= 0.0 && p.y >= 0.0 && p.x < w as f32 && p.y < h as f32).then_some((p.x as u32, p.y as u32))
}

#[uniffi::export]
impl CanvasSession {
    /// Pours colour (`r`, `g`, `b`, 0..1) at window point (`x`, `y`) with `threshold` (0..1) on
    /// the current layer, within the selection if there is one. False if nothing was poured (off
    /// the canvas, or the layer locked or text). The pour stays open for
    /// [`CanvasSession::pour_threshold`] until [`CanvasSession::pour_end`].
    pub fn pour_begin(&self, x: f32, y: f32, r: f32, g: f32, b: f32, threshold: f32) -> bool {
        self.pour_end(true, None);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(seed) = seed_at(st, x, y) else { return false };
        let (w, h) = (st.doc.width(), st.doc.height());
        let current = st.doc.current();
        let Some(layer) = st.doc.layer(current).cloned() else { return false };
        if layer.locked || layer.text.is_some() {
            return false;
        }
        let Some(before) = st.current_pixels() else { return false };
        // measured on the reference layer when another layer is it
        let elsewhere = st.doc.reference().filter(|&r| r != current).and_then(|r| st.layer_pixels(r));
        let mut pour = Pour {
            layer: current,
            before,
            flood: None,
            elsewhere,
            colour: [r, g, b].map(unit),
            selection: st.selection.clone(),
            alpha_lock: layer.alpha_lock,
            shown: thousandths(threshold),
        };
        pour.measure(w, h, seed);
        if pour.flood.is_none() {
            return false;
        }
        let pixels = pour.poured(threshold);
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.hold_layer(current);
            canvas.put_layer(current, pixels);
        }
        st.pour = Some(pour);
        self.shared.frame(st);
        true
    }

    /// The open pour redone from window point (`x`, `y`) instead, from the layer as it was: off
    /// the canvas it pours nothing until it comes back. False if no pour is open.
    pub fn pour_move(&self, x: f32, y: f32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let seed = seed_at(st, x, y);
        let (w, h) = (st.doc.width(), st.doc.height());
        let Some(pour) = st.pour.as_mut() else { return false };
        match seed {
            Some(seed) => pour.measure(w, h, seed),
            None => pour.flood = None,
        }
        let (layer, pixels) = (pour.layer, pour.shown());
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.put_layer(layer, pixels);
        }
        self.shared.frame(st);
        true
    }

    /// The open pour redone with colour (`r`, `g`, `b`, 0..1).
    pub fn pour_colour(&self, r: f32, g: f32, b: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(pour) = st.pour.as_mut() else { return };
        let colour = [r, g, b].map(unit);
        if colour == pour.colour {
            return;
        }
        pour.colour = colour;
        let (layer, pixels) = (pour.layer, pour.shown());
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.put_layer(layer, pixels);
        }
        self.shared.frame(st);
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
        let (layer, pixels) = (pour.layer, pour.shown());
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.put_layer(layer, pixels);
        }
        self.shared.frame(st);
    }

    /// Ends the open pour: `keep` makes it one undo step, called `name` if given; otherwise (or
    /// with nothing poured) the layer goes back to how it was.
    pub fn pour_end(&self, keep: bool, name: Option<String>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(pour) = st.pour.take() else { return };
        let keep = keep && pour.flood.is_some();
        if let Some(canvas) = st.canvas.as_mut() {
            if let Some(name) = name.filter(|_| keep) {
                canvas.name_next_step(&name);
            }
            canvas.release_layer(keep, None);
        }
        st.revision += 1;
        self.shared.frame(st);
    }
}
