//! Dropping a colour on the canvas: the colour poured into the area joined to a point whose colour
//! is near the point's own, measured on the reference layer if there is one. While the finger stays
//! down the threshold can change and the pour is redone from the layer as it was; it ends as one
//! undo step. Recolor keeps a pour open while its point moves and its colour changes. A plugin on
//! the fill hook has its say as a pour is let go, on a thread of its own: the pour stays open, its
//! layer held, until it has.

use std::sync::{Arc, Condvar, Mutex};

use procraft_engine::document::LayerId;
use procraft_engine::fill::{self, Flood};
use procraft_engine::geom::{IRect, Vec2};
use procraft_engine::selection::Mask;

use crate::hooks::Hooks;
use crate::plugin_run::MOST_FILL;
use crate::reveal::{Reveal, Shake};
use crate::{CanvasSession, Shared, State};

/// What is said of a fill with more in it than can go through a plugin.
const TOO_BIG: &str = "This fill is too big for a plugin: it stays as it is.";

/// A pour a plugin is finishing: when it has, and whoever is waiting for that.
#[derive(Default)]
pub(crate) struct Finishing {
    done: Mutex<bool>,
    ended: Condvar,
}

impl Finishing {
    fn wait(&self) {
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        while !*done {
            done = self.ended.wait(done).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn end(&self) {
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.ended.notify_all();
    }
}

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
    /// The flood as a plugin left it, once one has.
    closed: Option<Mask>,
    /// Being finished by a plugin: nothing more changes it.
    finishing: Option<Arc<Finishing>>,
}

impl Pour {
    /// The colour poured at `threshold`, as the rectangle of the layer it changes now joined with
    /// the one it changed last (put back as it was where it no longer pours): that rectangle (x, y,
    /// width, height) and its pixels; none if neither changes anything. With `same_flood` (only
    /// the threshold changed since it was shown) only the rectangle where what it pours changed.
    /// Remembered as shown.
    fn pour_region(&mut self, width: u32, threshold: f32, same_flood: bool) -> Option<([u32; 4], Vec<u8>)> {
        let mask = self.flood.as_ref().map(|flood| {
            let mut mask = self.closed.clone().unwrap_or_else(|| flood.mask(threshold));
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

/// `mask`, a fill from `seed`, as the plugins on the fill hook leave it: its box goes through with
/// a pixel round it, which, in the picture, is a pixel of no fill, and past the picture's edge
/// nothing. None if they change nothing, or it is too big to go through. Takes a while: nothing
/// is held while it does.
pub(crate) fn closed_by(hooks: &Hooks, mut mask: Mask, seed: (u32, u32)) -> Option<Mask> {
    let [x, y, w, h] = mask.bounds()?;
    let (left, top) = (x.saturating_sub(1), y.saturating_sub(1));
    let (right, bottom) = ((x + w + 1).min(mask.width), (y + h + 1).min(mask.height));
    let (width, height) = ((right - left) as usize, (bottom - top) as usize);
    if (width * height) as u64 > MOST_FILL {
        hooks.say(TOO_BIG);
        return None;
    }
    let (stride, words) = (mask.width as usize, width.div_ceil(64));
    let mut rows = vec![0_u64; words * height];
    for (from, to) in mask.data.chunks_exact(stride).skip(top as usize).zip(rows.chunks_exact_mut(words)) {
        for (i, &v) in from.iter().skip(left as usize).take(width).enumerate() {
            if let Some(word) = to.get_mut(i / 64).filter(|_| v > 0) {
                *word |= 1 << (i % 64);
            }
        }
    }
    let from = (seed.0.checked_sub(left)? as usize, seed.1.checked_sub(top)? as usize);
    if !hooks.fill_rows(&mut rows, width, height, from) {
        return None;
    }
    for (to, left_of) in mask.data.chunks_exact_mut(stride).skip(top as usize).zip(rows.chunks_exact(words)) {
        for (i, v) in to.iter_mut().skip(left as usize).take(width).enumerate() {
            if left_of.get(i / 64).is_some_and(|word| word >> (i % 64) & 1 == 0) {
                *v = 0;
            }
        }
    }
    Some(mask)
}

/// Ends the open pour: `keep` makes it one undo step, called `name` if given (else "ColorDrop");
/// otherwise (or with nothing poured) the layer goes back to how it was.
fn end_pour(shared: &Shared, st: &mut State, keep: bool, name: Option<&str>) {
    let Some(pour) = st.pour.take() else { return };
    let keep = keep && pour.flood.is_some();
    if let Some(canvas) = st.canvas.as_mut() {
        if keep {
            canvas.name_next_step(name.unwrap_or("ColorDrop"));
        }
        canvas.release_layer(keep, None);
    }
    st.revision += 1;
    shared.frame(st);
}

impl State {
    /// A dropped colour's fill over `region` (x, y, width, height, canvas pixels), the colour
    /// landed on window point `point`, starts spreading in: the canvas as it was is kept to show
    /// over the fill but for a hole that grows. Not with Reduce Motion, nor on a model.
    fn start_reveal(&mut self, point: Vec2, region: [u32; 4]) {
        self.end_reveal();
        if self.reduce_motion || self.scene_shown() || self.sphere.is_some() {
            return;
        }
        let Some(view) = self.view else { return };
        let [x, y, w, h] = region.map(|v| i32::try_from(v).unwrap_or(i32::MAX));
        let rect = IRect::new(x, y, x.saturating_add(w), y.saturating_add(h));
        let mut shake = Shake::from_clock();
        let Some(reveal) = Reveal::new(point, rect, &view, self.window_size, self.display_scale, &mut shake) else {
            return;
        };
        // only what the window shows of it is veiled, at no more pixels than it is shown with
        let (ww, wh) = (self.window_size.0 as f32, self.window_size.1 as f32);
        let seen = [(0.0, 0.0), (ww, 0.0), (0.0, wh), (ww, wh)].map(|(x, y)| view.screen_to_canvas(Vec2::new(x, y)));
        let seen = seen.iter().flatten().fold(None, |b: Option<(Vec2, Vec2)>, &q| {
            let (lo, hi) = b.unwrap_or((q, q));
            Some((Vec2::new(lo.x.min(q.x), lo.y.min(q.y)), Vec2::new(hi.x.max(q.x), hi.y.max(q.y))))
        });
        let Some((lo, hi)) = seen.filter(|(lo, hi)| lo.is_finite() && hi.is_finite()) else { return };
        let seen = IRect::new(
            lo.x.floor() as i32 - 2,
            lo.y.floor() as i32 - 2,
            hi.x.ceil() as i32 + 2,
            hi.y.ceil() as i32 + 2,
        );
        let scale = view.to_screen().scale_factor();
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.veil_begin(gpu, rect.intersect(seen), scale);
            if canvas.veil_rect().is_some() {
                self.reveal = Some(reveal);
            }
        }
    }

    /// A fill spreading in shows all of itself now.
    pub(crate) fn end_reveal(&mut self) {
        if self.reveal.take().is_some() {
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.veil_end();
            }
            self.needs_frame = true;
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
    st.window_texel(x, y)
}

#[uniffi::export]
impl CanvasSession {
    /// Whether window point (`x`, `y`) is on the artwork, where a dragged colour can come to rest.
    pub fn pour_lands(&self, x: f32, y: f32) -> bool {
        seed_at(&self.shared.lock(), x, y).is_some()
    }

    /// Pours colour (`r`, `g`, `b`, 0..1) at window point (`x`, `y`) with `threshold` (0..1) on
    /// the current layer, within the selection if there is one. False if nothing was poured (off
    /// the canvas, or the layer locked or text). The pour stays open for
    /// [`CanvasSession::pour_threshold`] until [`CanvasSession::pour_end`].
    pub fn pour_begin(&self, x: f32, y: f32, r: f32, g: f32, b: f32, threshold: f32) -> bool {
        self.begin_pour(x, y, [r, g, b], threshold, false)
    }

    /// Pours as [`CanvasSession::pour_begin`] does a colour dragged out and let go, or come to
    /// rest, at window point (`x`, `y`): the fill is made at once, and shows spreading in from the
    /// point over a moment (at once with Reduce Motion).
    pub fn pour_drop(&self, x: f32, y: f32, r: f32, g: f32, b: f32, threshold: f32) -> bool {
        self.begin_pour(x, y, [r, g, b], threshold, true)
    }

    /// The open pour redone from window point (`x`, `y`) instead, from the layer as it was: off
    /// the canvas it pours nothing until it comes back. False if no pour is open.
    pub fn pour_move(&self, x: f32, y: f32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let seed = seed_at(st, x, y);
        let (w, h) = (st.doc.width(), st.doc.height());
        let Some(pour) = st.pour.as_mut().filter(|p| p.finishing.is_none()) else { return false };
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
        let Some(pour) = st.pour.as_mut().filter(|p| p.finishing.is_none()) else { return };
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
        let Some(pour) = st.pour.as_mut().filter(|p| p.finishing.is_none()) else { return };
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
    /// "ColorDrop"); otherwise (or with nothing poured) the layer goes back to how it was. Kept,
    /// with a plugin on the fill hook, it ends when that has had its say, on a thread of its own:
    /// [`CanvasSession::pour_finishing`] until then.
    pub fn pour_end(&self, keep: bool, name: Option<String>) {
        self.await_pour();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if keep && self.finish_pour(st, &name) {
            return;
        }
        end_pour(&self.shared, st, keep, name.as_deref());
    }

    /// Whether a fill let go is still being finished by a plugin: nothing should be started until
    /// it is. The pour's own calls wait for it.
    pub fn pour_finishing(&self) -> bool {
        self.shared.lock().pour.as_ref().is_some_and(|pour| pour.finishing.is_some())
    }
}

impl CanvasSession {
    /// Pours `colour` at window point (`x`, `y`) with `threshold` (see
    /// [`CanvasSession::pour_begin`]); `spread`: shown spreading in from the point.
    fn begin_pour(&self, x: f32, y: f32, colour: [f32; 3], threshold: f32, spread: bool) -> bool {
        self.pour_end(true, None);
        self.await_pour();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        // a fill still spreading in shows all of itself
        st.end_reveal();
        let [r, g, b] = colour;
        let Some(seed) = seed_at(st, x, y) else { return false };
        let (w, h) = (st.doc.width(), st.doc.height());
        let current = st.doc.current();
        let Some(layer) = st.doc.layer(current).cloned() else { return false };
        // a plugin's layer that takes a dropped colour makes of it what it will, as one step
        if layer.plugin.is_some() && !st.doc.is_locked(current) && st.plugin_layer_fills(current) {
            let at = Vec2::new(seed.0 as f32 + 0.5, seed.1 as f32 + 0.5);
            let done = st.fill_plugin_layer(current, at, [r, g, b], threshold);
            if done {
                self.shared.frame(st);
            }
            return done;
        }
        if st.doc.is_locked(current) || layer.drawn_from_data() {
            return false;
        }
        let Some(before) = st.current_pixels() else { return false };
        let within = st.reach_selection();
        // measured on every layer seen while a plugin says so, else on the reference layer when
        // another layer is it
        let elsewhere = if st.hooks.as_ref().is_some_and(|hooks| hooks.fill_all_layers()) {
            st.composite_pixels()
        } else {
            st.doc.reference().filter(|&r| r != current).and_then(|r| st.layer_pixels(r))
        };
        let mut pour = Pour {
            layer: current,
            before,
            flood: None,
            elsewhere,
            colour: [r, g, b].map(unit),
            selection: within,
            alpha_lock: layer.alpha_lock,
            shown: thousandths(threshold),
            shown_region: None,
            shown_mask: None,
            closed: None,
            finishing: None,
        };
        pour.measure(w, h, seed);
        if pour.flood.is_none() {
            return false;
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.hold_layer(current);
        }
        pour.show(w, st.canvas.as_mut(), false);
        let region = pour.shown_region;
        st.pour = Some(pour);
        if let Some(region) = region.filter(|_| spread) {
            st.start_reveal(Vec2::new(x, y), region);
        }
        self.shared.frame(st);
        true
    }

    /// Waits for a fill a plugin is finishing, if there is one.
    pub(crate) fn await_pour(&self) {
        let finishing = self.shared.lock().pour.as_ref().and_then(|pour| pour.finishing.clone());
        if let Some(finishing) = finishing {
            finishing.wait();
        }
    }

    /// Hands the open pour to a thread of its own, to end when the plugins on the fill hook have
    /// had their say; false if none wants to, and it is to end now.
    fn finish_pour(&self, st: &mut State, name: &Option<String>) -> bool {
        let Some(hooks) = st.hooks.clone().filter(|hooks| hooks.fill_mask()) else { return false };
        let Some(pour) = st.pour.as_mut() else { return false };
        let threshold = pour.shown as f32 / 1000.0;
        let Some(flood) = pour.flood.as_ref() else { return false };
        let (mask, seed) = (flood.mask(threshold), flood.seed());
        let finishing = Arc::new(Finishing::default());
        let (shared, ended, name) = (Arc::clone(&self.shared), Arc::clone(&finishing), name.clone());
        let work = move || {
            let closed = closed_by(&hooks, mask, seed);
            let mut guard = shared.lock();
            let st = &mut *guard;
            let width = st.doc.width();
            if let (Some(pour), Some(closed)) = (st.pour.as_mut(), closed) {
                pour.closed = Some(closed);
                pour.show(width, st.canvas.as_mut(), false);
            }
            end_pour(&shared, st, true, name.as_deref());
            drop(guard);
            ended.end();
        };
        if std::thread::Builder::new().name("fill".to_owned()).spawn(work).is_err() {
            return false;
        }
        pour.finishing = Some(finishing);
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A mask filled all over.
    pub(crate) fn mask_of_box(width: u32, height: u32) -> Mask {
        Mask { width, height, data: vec![255; width as usize * height as usize] }
    }

    #[test]
    fn a_fill_goes_through_a_row_of_bits_and_comes_back_by_where_it_was() {
        use crate::plugin_run::tests::{FILL_AT, fill_module};
        use crate::plugin_run::{Hook, Running};
        // takes out the fifth pixel of every row it is handed
        let out = "(i64.store (i32.const 65536) (i64.const 16))";
        let hooks = Hooks::default();
        let module = fill_module(FILL_AT, "(i32.const 1)", out);
        hooks.put_on("fill".to_owned(), Running::start(&module, &[]).unwrap(), &[Hook::FillMask]);
        // a block of 10 x 3 at (20, 5) in a mask of 100 x 50, with soft values
        let mut mask = Mask { width: 100, height: 50, data: vec![0; 100 * 50] };
        for y in 5..8 {
            mask.data[y * 100 + 20..y * 100 + 30].fill(90);
        }
        let closed = closed_by(&hooks, mask.clone(), (22, 6)).unwrap();
        // its box and a pixel round it start at (19, 4): the fifth pixel is at 23
        for y in 5..8 {
            assert_eq!(closed.data[y * 100 + 23], 0, "{y}");
            assert_eq!(closed.data[y * 100 + 22], 90, "what stays keeps its value");
            assert_eq!(closed.data[y * 100 + 24], 90);
        }
        assert_eq!(closed.data.iter().filter(|&&v| v > 0).count(), 27);
    }

    #[test]
    fn a_box_at_the_picture_s_edge_has_nothing_past_it() {
        use crate::plugin_run::tests::fill_module;
        use crate::plugin_run::{Hook, Running};
        // it must be told 3 wide and 2 tall (nothing left or above, a pixel right and below) and
        // the seed at (1, 0), or it crashes
        let begin = "(if (i32.or (i32.ne (local.get 0) (i32.const 3)) (i32.or (i32.ne (local.get 1) (i32.const 2))
                (i32.or (i32.ne (local.get 2) (i32.const 1)) (i32.ne (local.get 3) (i32.const 0)))))
                (then (unreachable)))
             (i32.const 65536)";
        let hooks = Hooks::default();
        let module = fill_module(begin, "(i32.const 0)", "");
        hooks.put_on("fill".to_owned(), Running::start(&module, &[]).unwrap(), &[Hook::FillMask]);
        let mut mask = Mask { width: 100, height: 50, data: vec![0; 100 * 50] };
        mask.data[0..2].fill(255);
        assert!(closed_by(&hooks, mask, (1, 0)).is_none());
        assert_eq!(hooks.take_said(), None);
    }
}
