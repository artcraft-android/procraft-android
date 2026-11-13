//! Tweening: In-between in Animation Assist's frame options. The app hands over two frames (the
//! part either is drawn in) and takes back the frame between them (the plugin kit passes them).
//! Here each drawing slides and grows from its own place toward the other's, as far
//! as the in-between falls, and the two fade from one to the other. It's a plugin, not part of the
//! app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

/// The controls: whether the drawings slide (0 or 1), and how near the next frame the in-between
/// falls.
static CONTROLS: kit::Controls<2> = kit::Controls::new([1.0, 0.5]);
kit::controls!(CONTROLS);

kit::inbetween!(|this, next, width, height| {
    let near = CONTROLS.get(1).clamp(0.05, 0.95);
    Some(inbetween(this, next, width, height, near, CONTROLS.on(0)))
});

/// What counts as drawn, of 255.
const DRAWN: u8 = 8;

/// Where `rgba` (`width` wide, premultiplied RGBA8) is drawn: left, top, right and bottom edges in
/// pixels; none if nowhere.
fn drawn(rgba: &[u8], width: usize) -> Option<[f32; 4]> {
    let mut edges: Option<(usize, usize, usize, usize)> = None;
    for (i, pixel) in rgba.chunks_exact(4).enumerate() {
        if pixel.get(3).is_some_and(|&a| a > DRAWN) {
            let (x, y) = (i % width.max(1), i / width.max(1));
            let (l, t, r, b) = edges.get_or_insert((x, y, x + 1, y + 1));
            (*l, *t, *r, *b) = ((*l).min(x), (*t).min(y), (*r).max(x + 1), (*b).max(y + 1));
        }
    }
    edges.map(|(l, t, r, b)| [l as f32, t as f32, r as f32, b as f32])
}

/// `rgba` (`width` × `height`) at (`x`, `y`), pixels' centres at halves, between its four
/// nearest; clear outside it.
fn sample(rgba: &[u8], width: usize, height: usize, x: f32, y: f32) -> [f32; 4] {
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let at = |px: f32, py: f32| -> [f32; 4] {
        if px < 0.0 || py < 0.0 || px >= width as f32 || py >= height as f32 {
            return [0.0; 4];
        }
        let i = (py as usize * width + px as usize) * 4;
        rgba.get(i..i + 4).and_then(|p| <[u8; 4]>::try_from(p).ok()).map_or([0.0; 4], |p| p.map(f32::from))
    };
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
    let mut out = [0.0; 4];
    for (k, o) in out.iter_mut().enumerate() {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bottom = c[k] + (d[k] - c[k]) * tx;
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

/// The in-between of `a` and `b` (each `width` × `height`, premultiplied RGBA8) at `t` (0 is
/// `a`, 1 is `b`): with `slides`, each drawing moved and sized from its own box toward the other's
/// as far as `t` says (when both have one); then the two faded together.
fn inbetween(a: &[u8], b: &[u8], width: usize, height: usize, t: f32, slides: bool) -> Vec<u8> {
    let t = t.clamp(0.0, 1.0);
    let boxes = if slides { drawn(a, width).zip(drawn(b, width)) } else { None };
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
    let mut out = vec![0u8; width * height * 4];
    for (i, pixel) in out.chunks_exact_mut(4).enumerate() {
        let (x, y) = ((i % width.max(1)) as f32 + 0.5, (i / width.max(1)) as f32 + 0.5);
        let (ax, ay) = way_a.source(x, y);
        let (bx, by) = way_b.source(x, y);
        let (from_a, from_b) = (sample(a, width, height, ax, ay), sample(b, width, height, bx, by));
        for ((o, u), v) in pixel.iter_mut().zip(from_a).zip(from_b) {
            *o = lerp(u, v, t).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 64;

    /// A white square `side` across, its top left at (`x`, `y`).
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
        assert_eq!((alpha(&mid, 25, 28), alpha(&mid, 31, 28)), (255, 255));
        assert_eq!((alpha(&mid, 8, 28), alpha(&mid, 48, 28)), (0, 0));
        let late = inbetween(&a, &b, W, W, 0.75, true);
        assert_eq!((alpha(&late, 38, 28), alpha(&late, 28, 28)), (255, 0));
    }

    #[test]
    fn a_drawing_grows_toward_its_next_size_and_fades_when_it_does_not_slide() {
        let mid = inbetween(&square(28, 28, 8), &square(20, 20, 24), W, W, 0.5, true);
        assert_eq!((alpha(&mid, 25, 32), alpha(&mid, 21, 32)), (255, 0));
        let faded = inbetween(&square(4, 24, 8), &square(44, 24, 8), W, W, 0.5, false);
        assert_eq!((alpha(&faded, 8, 28), alpha(&faded, 48, 28), alpha(&faded, 28, 28)), (128, 128, 0));
        let one = inbetween(&square(4, 24, 8), &vec![0; W * W * 4], W, W, 0.5, true);
        assert_eq!(alpha(&one, 8, 28), 128);
    }

    #[test]
    fn the_calls_take_rows_in_and_give_the_frame_between_out() {
        let (a, b) = (square(4, 24, 8), square(44, 24, 8));
        // (the app writes and reads the row through the answer, a place in the module's memory; here
        // the job's row itself)
        procraft_between_begin(W as i32, W as i32);
        for (frame, pixels) in [&a, &b].into_iter().enumerate() {
            for y in 0..W {
                kit::BETWEEN.lock().unwrap().as_mut().unwrap().row.copy_from_slice(&pixels[y * W * 4..(y + 1) * W * 4]);
                procraft_between_row(frame as i32, y as i32);
            }
        }
        assert_eq!(procraft_between_run(), 1);
        procraft_between_out(28);
        let out = kit::BETWEEN.lock().unwrap().as_ref().unwrap().row.clone();
        assert_eq!((out[28 * 4 + 3], out[8 * 4 + 3]), (255, 0));
    }
}
