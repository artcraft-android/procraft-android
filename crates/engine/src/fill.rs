//! Filling by colour (dropping a colour on the canvas): a flood from a point over the pixels whose
//! colour is near its own, and the colour poured into it so that soft edges stay soft.

use crate::selection::Mask;

/// How far past the threshold (as a colour distance, 0..1) the fill fades out.
pub const FADE: f32 = 0.09;

/// The pixels' colour distances from a seed pixel, kept to flood again at another threshold.
pub struct Flood {
    width: u32,
    height: u32,
    seed: (u32, u32),
    /// Each pixel's distance from the seed's colour: the largest difference of their
    /// premultiplied channels, 0..255.
    distance: Vec<u8>,
    /// The seed pixel's colour (premultiplied RGBA8).
    pub colour: [u8; 4],
}

impl Flood {
    /// `pixels` (premultiplied RGBA8, `width` x `height`) measured from the pixel at `seed`; None
    /// when the seed is off the picture.
    pub fn new(width: u32, height: u32, pixels: &[u8], seed: (u32, u32)) -> Option<Flood> {
        let count = width as usize * height as usize;
        if seed.0 >= width || seed.1 >= height || pixels.len() < count * 4 {
            return None;
        }
        let i = (seed.1 as usize * width as usize + seed.0 as usize) * 4;
        let colour: [u8; 4] = pixels.get(i..i + 4)?.try_into().ok()?;
        let distance = pixels.as_chunks::<4>().0.iter().take(count).map(|p| distance(p, &colour)).collect();
        Some(Flood { width, height, seed, distance, colour })
    }

    /// The pixel the fill is measured from, now `seed`, for a seed of the same colour (the
    /// distances stay as they are). False, and left as it was, if `seed` is off the picture.
    pub fn reseed(&mut self, seed: (u32, u32)) -> bool {
        let on = seed.0 < self.width && seed.1 < self.height;
        if on {
            self.seed = seed;
        }
        on
    }

    /// How much each pixel takes the fill at `threshold` (0..1): the pixels joined to the seed
    /// through pixels within reach, fully where the distance is within the threshold, fading out
    /// over [`FADE`] past it.
    pub fn mask(&self, threshold: f32) -> Mask {
        let t = threshold.clamp(0.0, 1.0) * 255.0;
        let fade = FADE * 255.0;
        let weights: [u8; 256] = std::array::from_fn(|d| {
            let d = d as f32;
            if d <= t { 255 } else { (((t + fade - d) / fade).clamp(0.0, 1.0) * 255.0).round() as u8 }
        });
        // the farthest distance the fill reaches (the weights only fall with distance)
        let cut = weights.iter().rposition(|&v| v > 0).unwrap_or(0) as u8;
        let mut m = Mask::empty(self.width, self.height);
        let (w, h) = (self.width as usize, self.height as usize);
        // a pixel not yet filled (filled ones are at least 1) that the fill can reach
        let open = |(m, d): (&u8, &u8)| *m == 0 && *d <= cut;
        fn before(s: &[u8], n: usize) -> &[u8] {
            s.get(..n).unwrap_or_default()
        }
        fn after(s: &[u8], n: usize) -> &[u8] {
            s.get(n..).unwrap_or_default()
        }
        // scanlines: each span filled whole, then the runs above and below it seeded; each row
        // looked along as a slice
        let mut stack = vec![(self.seed.0 as usize, self.seed.1 as usize)];
        while let Some((x, y)) = stack.pop() {
            let (Some(line), Some(far)) = (m.data.get_mut(y * w..(y + 1) * w), self.distance.get(y * w..(y + 1) * w))
            else {
                continue;
            };
            if !line.get(x).zip(far.get(x)).is_some_and(open) {
                continue;
            }
            let l = x - before(line, x).iter().rev().zip(before(far, x).iter().rev()).take_while(|&p| open(p)).count();
            let r = x + after(line, x + 1).iter().zip(after(far, x + 1)).take_while(|&p| open(p)).count();
            for (v, &d) in line.get_mut(l..=r).unwrap_or_default().iter_mut().zip(far.get(l..=r).unwrap_or_default()) {
                *v = weights.get(usize::from(d)).copied().unwrap_or(0);
            }
            for ny in [y.wrapping_sub(1), y + 1] {
                if ny >= h {
                    continue;
                }
                let under = m.data.get(ny * w + l..=ny * w + r).unwrap_or_default();
                let far = self.distance.get(ny * w + l..=ny * w + r).unwrap_or_default();
                let mut xi = 0;
                while let Some(start) = after(under, xi).iter().zip(after(far, xi)).position(open) {
                    stack.push((l + xi + start, ny));
                    xi += start;
                    xi += after(under, xi).iter().zip(after(far, xi)).take_while(|&p| open(p)).count();
                }
            }
        }
        m
    }
}

/// The largest difference of two premultiplied pixels' channels.
fn distance(a: &[u8; 4], b: &[u8; 4]) -> u8 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

/// Pours `colour` (RGB, 0..1) into `pixels` (premultiplied RGBA8) as much as `mask` takes it,
/// where the flood was measured on these same pixels from a seed of colour `seed`. What there is
/// of the seed's colour becomes the new colour, each channel scaled so that black and white stay
/// as they are (the dark edge of a line on white keeps its darkness); where the seed is
/// see-through the colour goes behind what is there, so a line's soft edge stays over it.
/// `alpha_lock` keeps each pixel's coverage.
pub fn pour(pixels: &mut [u8], mask: &Mask, seed: [u8; 4], colour: [f32; 3], alpha_lock: bool) {
    let [sr, sg, sb, sa] = seed.map(f32::from);
    let seed_a = sa / 255.0;
    let seed_rgb = [sr, sg, sb].map(|c| if sa > 0.0 { c / sa } else { 0.0 });
    let colour = colour.map(|c| c.clamp(0.0, 1.0));
    pour_with(pixels, mask, alpha_lock, |[r, g, b, a]| {
        let mut out = [0.0; 4];
        for (((o, p), s), f) in out.iter_mut().zip([r, g, b]).zip(seed_rgb).zip(colour) {
            // behind: the colour under what is there
            let behind = p + (1.0 - a) * f;
            // recoloured: the seed's colour taken to the new one, coverage kept
            let recoloured = if a > 0.0 { recolour(p / a, s, f) * a } else { 0.0 };
            *o = behind + (recoloured - behind) * seed_a;
        }
        // opaque behind, as covered as it was recoloured
        let [r, g, b, _] = out;
        [r, g, b, 1.0 + (a - 1.0) * seed_a]
    });
}

/// Pours `colour` (RGB, 0..1) over `pixels` (premultiplied RGBA8) as much as `mask` takes it: for a
/// flood measured on another layer. `alpha_lock` keeps each pixel's coverage.
pub fn pour_over(pixels: &mut [u8], mask: &Mask, colour: [f32; 3], alpha_lock: bool) {
    let [r, g, b] = colour.map(|c| c.clamp(0.0, 1.0));
    pour_with(pixels, mask, alpha_lock, |_| [r, g, b, 1.0]);
}

/// Each pixel `mask` takes moved toward what `full` (from the pixel, premultiplied 0..1) makes of
/// it, as far as the mask goes.
fn pour_with(pixels: &mut [u8], mask: &Mask, alpha_lock: bool, full: impl Fn([f32; 4]) -> [f32; 4]) {
    // a filled area is mostly runs of the same pixel taking the same amount: made once each run
    let mut last: Option<([u8; 4], u8, [u8; 4])> = None;
    for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&mask.data) {
        if m == 0 {
            continue;
        }
        if let Some((was, k, made)) = last
            && was == *px
            && k == m
        {
            *px = made;
            continue;
        }
        let input = *px;
        let k = f32::from(m) / 255.0;
        let p = px.map(|v| f32::from(v) / 255.0);
        let mut out = p;
        for ((o, t), q) in out.iter_mut().zip(full(p)).zip(p) {
            *o = q + (t - q) * k;
        }
        let [mut r, mut g, mut b, mut a] = out;
        if alpha_lock {
            let was = p[3];
            let scale = if a > 0.0 { was / a } else { 0.0 };
            (r, g, b, a) = (r * scale, g * scale, b * scale, was);
        }
        let a = a.clamp(0.0, 1.0);
        *px = [r.clamp(0.0, a), g.clamp(0.0, a), b.clamp(0.0, a), a].map(|v| (v * 255.0).round() as u8);
        last = Some((input, m, *px));
    }
}

/// A channel `p` (0..1) with the seed's `s` taken to `f`: darker than the seed scaled toward
/// black, lighter toward white.
fn recolour(p: f32, s: f32, f: f32) -> f32 {
    if p <= s { if s > 0.0 { p / s * f } else { f } } else { f + (p - s) / (1.0 - s) * (1.0 - f) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORANGE: [f32; 3] = [1.0, 136.0 / 255.0, 72.0 / 255.0];

    /// A row of pixels, each (grey, alpha) as premultiplied black or grey over nothing.
    fn row(px: &[[u8; 4]]) -> Vec<u8> {
        px.iter().flatten().copied().collect()
    }

    /// How long a pour's area over most of a 4K layer takes at a few thresholds (redone a frame at
    /// a time while one slides): run on a device with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn pour_cost() {
        let (w, h) = (3840u32, 2160u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for (i, p) in px.chunks_mut(4).enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            let v = (100 + x * 60 / w + y * 40 / h) as u8;
            let blob = (x as i32 - 1900).pow(2) + (y as i32 - 1000).pow(2) < 400 * 400;
            p.copy_from_slice(&if blob { [30, 30, 40, 255] } else { [v, v, v, 255] });
        }
        let start = std::time::Instant::now();
        let Some(flood) = Flood::new(w, h, &px, (100, 100)) else { return };
        println!("measured: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
        for threshold in [0.05f32, 0.2, 0.5] {
            let mut each = Vec::new();
            for _ in 0..5 {
                let start = std::time::Instant::now();
                std::hint::black_box(flood.mask(threshold));
                each.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            each.sort_by(f64::total_cmp);
            println!("pour {threshold}: {:.1} ms (fastest {:.1})", each[2], each[0]);
        }
    }

    #[test]
    fn the_flood_stops_at_a_line_and_fades_past_the_threshold() {
        // empty, a soft edge (alpha 0.31), the line's near-solid edge (0.91), the line, empty
        let px = row(&[[0, 0, 0, 0], [0, 0, 0, 79], [0, 0, 0, 232], [0, 0, 0, 255], [0, 0, 0, 0]]);
        let f = Flood::new(5, 1, &px, (0, 0)).unwrap();
        let m = f.mask(0.867);
        assert_eq!(m.data[0], 255);
        assert_eq!(m.data[1], 255);
        // 0.91 is 0.043 past the threshold: about half way through the fade
        assert!((110..=140).contains(&m.data[2]), "{}", m.data[2]);
        assert_eq!(m.data[3], 0);
        // beyond the line it is not joined
        assert_eq!(m.data[4], 0);
        // at the most everything is joined
        assert!(f.mask(1.0).data.iter().all(|&v| v == 255));
        assert!(Flood::new(5, 1, &px, (5, 0)).is_none());
    }

    #[test]
    fn the_flood_goes_round_corners() {
        // a wall with a gap at the bottom: the fill gets round it
        #[rustfmt::skip]
        let wall = [
            0, 1, 0,
            0, 1, 0,
            0, 0, 0,
        ];
        let px: Vec<u8> = wall.iter().flat_map(|&w| if w == 1 { [0, 0, 0, 255] } else { [0, 0, 0, 0] }).collect();
        let m = Flood::new(3, 3, &px, (0, 0)).unwrap().mask(0.5);
        assert_eq!(m.data, vec![255, 0, 255, 255, 0, 255, 255, 255, 255]);
    }

    #[test]
    fn colour_goes_behind_a_soft_edge() {
        // as measured: a line's edge at alpha 0.31 over white turns into the line over the fill
        let mut px = row(&[[0, 0, 0, 0], [0, 0, 0, 79]]);
        let mask = Mask { width: 2, height: 1, data: vec![255, 255] };
        pour(&mut px, &mask, [0, 0, 0, 0], ORANGE, false);
        assert_eq!(&px[0..4], &[255, 136, 72, 255]);
        // 255 * 0.69 and so on, opaque
        assert_eq!(&px[4..8], &[176, 94, 50, 255]);
    }

    #[test]
    fn a_coloured_area_is_recoloured_keeping_its_shading() {
        // white with a grey edge: the grey takes the new colour, as dark as it was
        let mut px = row(&[[255, 255, 255, 255], [128, 128, 128, 255], [0, 0, 0, 255]]);
        let mask = Mask { width: 3, height: 1, data: vec![255, 255, 255] };
        pour(&mut px, &mask, [255, 255, 255, 255], ORANGE, false);
        assert_eq!(&px[0..4], &[255, 136, 72, 255]);
        assert_eq!(&px[4..8], &[128, 68, 36, 255]);
        assert_eq!(&px[8..12], &[0, 0, 0, 255]);
        // on black, lighter greys stay lighter
        let mut px = row(&[[0, 0, 0, 255], [255, 255, 255, 255]]);
        pour(&mut px, &Mask { width: 2, height: 1, data: vec![255, 255] }, [0, 0, 0, 255], ORANGE, false);
        assert_eq!(&px[0..4], &[255, 136, 72, 255]);
        assert_eq!(&px[4..8], &[255, 255, 255, 255]);
    }

    #[test]
    fn the_mask_weighs_the_pour_and_alpha_lock_keeps_coverage() {
        let mut px = row(&[[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 128]]);
        let mask = Mask { width: 3, height: 1, data: vec![0, 128, 255] };
        let mut locked = px.clone();
        pour(&mut px, &mask, [0, 0, 0, 0], ORANGE, false);
        assert_eq!(&px[0..4], &[0, 0, 0, 0]);
        assert_eq!(px[7], 128);
        // locked: nothing where there was nothing; the half covered pixel stays half covered
        pour(&mut locked, &mask, [0, 0, 0, 0], ORANGE, true);
        assert_eq!(&locked[4..8], &[0, 0, 0, 0]);
        assert_eq!(locked[11], 128);
        assert!(locked[8] > 0);
        // over: the colour as the mask takes it
        let mut over = row(&[[0, 0, 0, 0], [0, 0, 0, 255]]);
        pour_over(&mut over, &Mask { width: 2, height: 1, data: vec![255, 255] }, ORANGE, false);
        assert_eq!(over, row(&[[255, 136, 72, 255], [255, 136, 72, 255]]));
    }
}
