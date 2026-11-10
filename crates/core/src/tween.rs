//! In-betweens (a plugin's): a frame drawn between two of Animation Assist's frames, both drawings
//! slid toward each other's place and size as far as the in-between falls, and faded from one to
//! the other. Made here from the two frames' pixels and written as one undo step.

use procraft_engine::animation;
use procraft_engine::document::{LayerId, LayerKind};
use procraft_engine::geom::IRect;

use crate::CanvasSession;

/// How a plugin has in-betweens drawn: whether the drawings slide between their places, and how
/// near the next frame the in-between falls (0 the frame, 1 the next).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TweenSettings {
    pub(crate) slides: bool,
    pub(crate) at: f32,
}

/// What counts as painted, of 255.
const PAINTED: u8 = 8;

/// Where `rgba` (`w` wide, premultiplied RGBA8) is painted, as left, top, right and bottom edges in
/// pixels; none if nowhere.
fn painted(rgba: &[u8], w: usize) -> Option<[f32; 4]> {
    let mut edges: Option<(usize, usize, usize, usize)> = None;
    for (i, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
        if pixel.get(3).is_some_and(|&a| a > PAINTED) {
            let (x, y) = (i % w.max(1), i / w.max(1));
            let (l, t, r, b) = edges.get_or_insert((x, y, x + 1, y + 1));
            (*l, *t, *r, *b) = ((*l).min(x), (*t).min(y), (*r).max(x + 1), (*b).max(y + 1));
        }
    }
    edges.map(|(l, t, r, b)| [l as f32, t as f32, r as f32, b as f32])
}

/// `rgba` (`w` × `h`) at (`x`, `y`), pixels' centres at halves, between its four nearest; clear
/// outside it.
fn sample(rgba: &[u8], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let at = |px: f32, py: f32| -> [f32; 4] {
        if px < 0.0 || py < 0.0 || px >= w as f32 || py >= h as f32 {
            return [0.0; 4];
        }
        let i = (py as usize * w + px as usize) * 4;
        rgba.get(i..i + 4).and_then(|p| <[u8; 4]>::try_from(p).ok()).map_or([0.0; 4], |p| p.map(f32::from))
    };
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
    let mut out = [0.0; 4];
    for (k, o) in out.iter_mut().enumerate() {
        let get = |v: [f32; 4]| v.get(k).copied().unwrap_or(0.0);
        let top = get(a) + (get(b) - get(a)) * tx;
        let bottom = get(c) + (get(d) - get(c)) * tx;
        *o = top + (bottom - top) * ty;
    }
    out
}

/// A drawing's way to the in-between: a pixel there comes from `from` + (it − `to`) ÷ `scale`.
#[derive(Clone, Copy)]
struct Way {
    from: (f32, f32),
    to: (f32, f32),
    scale: (f32, f32),
}

impl Way {
    const STILL: Way = Way { from: (0.0, 0.0), to: (0.0, 0.0), scale: (1.0, 1.0) };

    fn source(self, x: f32, y: f32) -> (f32, f32) {
        (self.from.0 + (x - self.to.0) / self.scale.0, self.from.1 + (y - self.to.1) / self.scale.1)
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The in-between of `a` and `b` (each `w` × `h`, premultiplied RGBA8) at `t` (0 is `a`, 1 is
/// `b`): with `slides`, each drawing moved and sized from its own box toward the other's as far as
/// `t` says (when both have one); then the two faded together.
pub(crate) fn inbetween(a: &[u8], b: &[u8], w: usize, h: usize, t: f32, slides: bool) -> Vec<u8> {
    let t = t.clamp(0.0, 1.0);
    let boxes = if slides { painted(a, w).zip(painted(b, w)) } else { None };
    let (way_a, way_b) = match boxes {
        Some(([al, at, ar, ab], [bl, bt, br, bb])) => {
            let (ca, cb) = (((al + ar) / 2.0, (at + ab) / 2.0), ((bl + br) / 2.0, (bt + bb) / 2.0));
            let (sa, sb) = (((ar - al).max(1.0), (ab - at).max(1.0)), ((br - bl).max(1.0), (bb - bt).max(1.0)));
            let centre = (lerp(ca.0, cb.0, t), lerp(ca.1, cb.1, t));
            let scale_a = (lerp(1.0, sb.0 / sa.0, t), lerp(1.0, sb.1 / sa.1, t));
            let scale_b = (lerp(sa.0 / sb.0, 1.0, t), lerp(sa.1 / sb.1, 1.0, t));
            (Way { from: ca, to: centre, scale: scale_a }, Way { from: cb, to: centre, scale: scale_b })
        }
        None => (Way::STILL, Way::STILL),
    };
    let mut out = vec![0u8; w * h * 4];
    for (i, pixel) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((i % w.max(1)) as f32 + 0.5, (i / w.max(1)) as f32 + 0.5);
        let (ax, ay) = way_a.source(x, y);
        let (bx, by) = way_b.source(x, y);
        let (from_a, from_b) = (sample(a, w, h, ax, ay), sample(b, w, h, bx, by));
        for ((o, u), v) in pixel.iter_mut().zip(from_a).zip(from_b) {
            *o = lerp(u, v, t).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[uniffi::export]
impl CanvasSession {
    /// Animation Assist's In-between (a plugin's): a frame drawn between frame `id` and the next, as
    /// the plugin sets it, put just after `id` and made current, as one undo step. None without such
    /// a plugin, with no next frame, if either frame is more than one layer or nothing is drawn on
    /// either, or with no room for a layer.
    pub fn add_inbetween(&self, id: u64) -> Option<u64> {
        let hooks = self.shared.lock().hooks.clone();
        let settings = hooks.and_then(|hooks| hooks.tween())?;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let this = LayerId(id);
        let frames = animation::frames(&st.doc);
        let next = frames.iter().position(|f| *f == this).and_then(|i| frames.get(i + 1)).copied()?;
        let one_layer = |l: LayerId| st.doc.layer(l).is_some_and(|l| l.kind == LayerKind::Paint);
        if !one_layer(this) || !one_layer(next) || !st.doc.has_room(1) {
            return None;
        }
        let boxes: Vec<IRect> = [st.painted_box(this), st.painted_box(next)].into_iter().flatten().collect();
        let within = boxes.into_iter().reduce(IRect::union)?;
        let (from_this, from_next) = (st.layer_part(this, within)?, st.layer_part(next, within)?);
        let (w, h) = (within.width() as usize, within.height() as usize);
        let part = inbetween(&from_this, &from_next, w, h, settings.at, settings.slides);
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

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 64;

    /// A white square 8 pixels across, its top left at (`x`, `y`), `side` across.
    fn square(x: usize, y: usize, side: usize) -> Vec<u8> {
        let mut out = vec![0u8; W * W * 4];
        for row in y..y + side {
            for col in x..x + side {
                out[(row * W + col) * 4..(row * W + col) * 4 + 4].copy_from_slice(&[255; 4]);
            }
        }
        out
    }

    fn alpha(rgba: &[u8], x: usize, y: usize) -> u8 {
        rgba[(y * W + x) * 4 + 3]
    }

    #[test]
    fn a_drawing_slides_halfway_to_where_it_goes_next() {
        let (a, b) = (square(4, 24, 8), square(44, 24, 8));
        let mid = inbetween(&a, &b, W, W, 0.5, true);
        // the square whole in the middle, nothing left where it was or is going
        assert_eq!(alpha(&mid, 25, 28), 255);
        assert_eq!(alpha(&mid, 31, 28), 255);
        assert_eq!(alpha(&mid, 8, 28), 0);
        assert_eq!(alpha(&mid, 48, 28), 0);
        // nearer the next frame, nearer its place
        let late = inbetween(&a, &b, W, W, 0.75, true);
        assert_eq!(alpha(&late, 38, 28), 255);
        assert_eq!(alpha(&late, 28, 28), 0);
    }

    #[test]
    fn a_drawing_grows_toward_its_next_size_and_fades_when_it_does_not_slide() {
        let (small, big) = (square(28, 28, 8), square(20, 20, 24));
        let mid = inbetween(&small, &big, W, W, 0.5, true);
        // halfway between 8 and 24 across, about the same middle
        assert_eq!(alpha(&mid, 25, 32), 255);
        assert_eq!(alpha(&mid, 21, 32), 0);
        // not sliding: each where it is, faded half
        let faded = inbetween(&square(4, 24, 8), &square(44, 24, 8), W, W, 0.5, false);
        assert_eq!(alpha(&faded, 8, 28), 128);
        assert_eq!(alpha(&faded, 48, 28), 128);
        assert_eq!(alpha(&faded, 28, 28), 0);
        // nothing drawn on one: the other fades
        let one = inbetween(&square(4, 24, 8), &vec![0; W * W * 4], W, W, 0.5, true);
        assert_eq!(alpha(&one, 8, 28), 128);
    }
}
