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
    /// The threshold last poured, in thousandths, and where the layer was last poured into (x, y,
    /// width, height): outside it the layer is as it was.
    shown: i32,
    shown_region: Option<[u32; 4]>,
    /// What it last poured, as the flood reached (within the selection).
    shown_mask: Option<Mask>,
    /// It keeps out of gaps up to this many pixels wide (0: none).
    gap: u32,
}

impl Pour {
    /// The colour poured at `threshold`, as the rectangle of the layer it changes now joined with
    /// the one it changed last (put back as it was where it no longer pours): that rectangle (x, y,
    /// width, height) and its pixels; none if neither changes anything. With `same_flood` (only
    /// the threshold changed since it was shown) only the rectangle where what it pours changed.
    /// Remembered as shown.
    fn pour_region(&mut self, width: u32, threshold: f32, same_flood: bool) -> Option<([u32; 4], Vec<u8>)> {
        let mask = self.flood.as_ref().map(|flood| {
            let mut mask = flood.mask(threshold);
            if self.gap > 0 {
                mask = fill::close_gaps(&mask, flood.seed(), self.gap);
            }
            // poured into what is see-through, it goes on under the line it stopped at
            if flood.colour[3] == 0 {
                fill::under_edges(&mut mask, self.elsewhere.as_deref().unwrap_or(&self.before));
            }
            if let Some(sel) = &self.selection {
                for (m, &s) in mask.data.iter_mut().zip(&sel.data) {
                    *m = ((u16::from(*m) * u16::from(s) + 127) / 255) as u8;
                }
            }
            mask
        });
        let now = mask.as_ref().and_then(Mask::bounds);
        // the same colour poured from the same seed colour: the pixels change only where the
        // flood reaches differently (a threshold dragged over a large fill changes its edges)
        let changed = match (&mask, &self.shown_mask) {
            (Some(m), Some(old)) if same_flood && m.data.len() == old.data.len() => Some(differ(m, old)),
            _ => None,
        };
        let region = match (changed, now, self.shown_region) {
            (Some(Some(r)), ..) => r,
            (Some(None), ..) => {
                self.shown_region = now;
                self.shown_mask = mask;
                return None;
            }
            (None, Some(a), Some(b)) => join(a, b),
            (None, a, b) => a.or(b)?,
        };
        self.shown_region = now;
        let [x, y, w, h] = region.map(|v| v as usize);
        let stride = width as usize;
        // the layer as it was there, and what of the flood falls there (a row at a time)
        let cut = |all: &[u8], px: usize| -> Vec<u8> {
            let mut out = Vec::with_capacity(w * h * px);
            for r in y..y + h {
                out.extend_from_slice(all.get((r * stride + x) * px..(r * stride + x + w) * px).unwrap_or(&[]));
            }
            out
        };
        let mut pixels = cut(&self.before, 4);
        if let (Some(mask), Some(flood)) = (&mask, &self.flood) {
            let data = cut(&mask.data, 1);
            let part = Mask { width: region[2], height: region[3], data };
            if self.elsewhere.is_some() {
                fill::pour_over(&mut pixels, &part, self.colour, self.alpha_lock);
            } else {
                fill::pour(&mut pixels, &part, flood.colour, self.colour, self.alpha_lock);
            }
        }
        self.shown_mask = mask;
        Some((region, pixels))
    }

    /// The area measured again from canvas point `seed`, on the reference layer or the layer as
    /// it was.
    fn measure(&mut self, width: u32, height: u32, seed: (u32, u32)) {
        let sample = self.elsewhere.as_deref().unwrap_or(&self.before);
        self.flood = Flood::new(width, height, sample, seed);
    }

    /// The point moved to `seed`: true if the pour stays as it is, the new point being of the
    /// same colour inside what it poured (the area joined to it then the same), so only the seed
    /// moves; otherwise measured again from it, and false.
    fn moved(&mut self, width: u32, height: u32, seed: (u32, u32)) -> bool {
        let sample = self.elsewhere.as_deref().unwrap_or(&self.before);
        let i = seed.1 as usize * width as usize + seed.0 as usize;
        let colour: Option<[u8; 4]> = sample.get(i * 4..i * 4 + 4).and_then(|c| c.try_into().ok());
        let inside = self.shown_mask.as_ref().and_then(|m| m.data.get(i)).is_some_and(|&v| v > 0);
        if let Some(flood) = self.flood.as_mut()
            && inside
            && colour == Some(flood.colour)
            && flood.reseed(seed)
        {
            return true;
        }
        self.measure(width, height, seed);
        false
    }

    /// The pour as it now is, put on its layer in `canvas` where it changed (`same_flood`: only
    /// its threshold changed since it was last shown).
    fn show(&mut self, width: u32, canvas: Option<&mut procraft_gpu::Canvas>, same_flood: bool) {
        let threshold = self.shown as f32 / 1000.0;
        let Some(([x, y, w, h], pixels)) = self.pour_region(width, threshold, same_flood) else { return };
        if let Some(canvas) = canvas {
            canvas.put_layer_region(self.layer, x, y, w, h, pixels);
        }
    }
}

/// The smallest rectangle (x, y, width, height) holding every pixel where masks `a` and `b` (of
/// the same size) differ; none where they don't.
fn differ(a: &Mask, b: &Mask) -> Option<[u32; 4]> {
    let w = (a.width as usize).max(1);
    let (mut top, mut bottom, mut left, mut right) = (None, 0, usize::MAX, 0);
    for (y, (p, q)) in a.data.chunks(w).zip(b.data.chunks(w)).enumerate() {
        if p == q {
            continue;
        }
        let Some(first) = p.iter().zip(q).position(|(u, v)| u != v) else { continue };
        let last = p.iter().zip(q).rposition(|(u, v)| u != v).unwrap_or(first);
        top.get_or_insert(y);
        bottom = y;
        left = left.min(first);
        right = right.max(last);
    }
    let top = top?;
    let n = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
    Some([n(left), n(top), n(right - left + 1), n(bottom - top + 1)])
}

/// The smallest rectangle (x, y, width, height) holding both `a` and `b`.
fn join(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let [ax, ay, aw, ah] = a;
    let [bx, by, bw, bh] = b;
    let (x, y) = (ax.min(bx), ay.min(by));
    [x, y, (ax + aw).max(bx + bw) - x, (ay + ah).max(by + bh) - y]
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
    /// ColorDrop keeps out of gaps up to `pixels` wide (canvas pixels, at most
    /// [`fill::MOST_GAP`]) in the lines that bound it; 0 fills through any.
    pub fn set_fill_gap(&self, pixels: u32) {
        self.shared.lock().fill_gap = pixels.min(fill::MOST_GAP);
    }

    /// Whether window point (`x`, `y`) is on the artwork, where a dragged colour can come to rest.
    pub fn pour_lands(&self, x: f32, y: f32) -> bool {
        seed_at(&self.shared.lock(), x, y).is_some()
    }

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
        if st.doc.is_locked(current) || layer.text.is_some() {
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
            shown_region: None,
            shown_mask: None,
            gap: st.fill_gap,
        };
        pour.measure(w, h, seed);
        if pour.flood.is_none() {
            return false;
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.hold_layer(current);
        }
        pour.show(w, st.canvas.as_mut(), false);
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
        let same = match seed {
            Some(seed) => pour.moved(w, h, seed),
            None => {
                pour.flood = None;
                false
            }
        };
        // moved within what it filled, from the same colour: nothing changes
        if !same {
            pour.show(w, st.canvas.as_mut(), false);
            self.shared.frame(st);
        }
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
        let w = st.doc.width();
        pour.show(w, st.canvas.as_mut(), false);
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
        let w = st.doc.width();
        pour.show(w, st.canvas.as_mut(), true);
        self.shared.frame(st);
    }

    /// Ends the open pour: `keep` makes it one undo step, called `name` if given (else
    /// "ColorDrop"); otherwise (or with nothing poured) the layer goes back to how it was.
    pub fn pour_end(&self, keep: bool, name: Option<String>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(pour) = st.pour.take() else { return };
        let keep = keep && pour.flood.is_some();
        if let Some(canvas) = st.canvas.as_mut() {
            if keep {
                canvas.name_next_step(name.as_deref().unwrap_or("ColorDrop"));
            }
            canvas.release_layer(keep, None);
        }
        st.revision += 1;
        self.shared.frame(st);
    }
}
