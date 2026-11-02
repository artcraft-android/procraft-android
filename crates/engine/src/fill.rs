//! Filling by colour (dropping a colour on the canvas): a flood from a point over the pixels whose
//! colour is near its own, and the colour poured into it so that soft edges stay soft.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::selection::Mask;

/// How far past the threshold (as a colour distance, 0..1) the fill fades out.
pub const FADE: f32 = 0.09;

/// A flood is first worked out on its own thread, quickest when it fills little; past this share
/// of the picture (one in so many pixels) it is worked out again on every core, quicker then.
pub(crate) const FLOOD_HERE_MOST: usize = 8;

/// The most pixels of `pixels` a flood fills on its own thread before going over to every core
/// ([`FLOOD_HERE_MOST`]). Never on Android: on the Legion every core was slower than one (a 4K
/// flood 23-25 ms against 19, the screen off), though on a computer 2.5 times quicker.
pub(crate) fn flood_here_most(pixels: usize) -> usize {
    if cfg!(target_os = "android") { usize::MAX } else { pixels / FLOOD_HERE_MOST }
}

/// The most pixels a flood works each pixel's reach out for (8K x 4K): past it, the memory the
/// lists may take outweighs the speed, and each fill is a flood.
pub(crate) const REACH_MOST: u64 = 8192 * 4096;

/// Whether working out the reach of `pixels` pixels, `bytes` each at most while it runs (its
/// lists included), is worth its memory here: within [`REACH_MOST`], and within an eighth of the
/// device's layer budget (half its memory), so a tablet with little doesn't run short for it.
pub(crate) fn reach_fits(pixels: u64, bytes: u64) -> bool {
    reach_fits_within(pixels, bytes, crate::document::layer_budget())
}

/// [`reach_fits`] with a layer budget of `budget` bytes.
fn reach_fits_within(pixels: u64, bytes: u64, budget: u64) -> bool {
    pixels <= REACH_MOST && pixels.saturating_mul(bytes) <= budget / 8
}

/// The pixels' colour distances from a seed pixel, kept to flood again at another threshold.
pub struct Flood {
    width: u32,
    height: u32,
    seed: (u32, u32),
    /// Each pixel's distance from the seed's colour: the largest difference of their
    /// premultiplied channels, 0..255.
    distance: Arc<Vec<u8>>,
    /// The seed pixel's colour (premultiplied RGBA8).
    pub colour: [u8; 4],
    /// Each pixel's reach, worked out on a thread of its own once a second mask is asked for (a
    /// threshold dragged): the least threshold (as a distance) at which the fill gets to it,
    /// over every way there from the seed it was worked out from. With it a fill is one pass
    /// over the pixels; until then, a flood.
    reach: Arc<OnceLock<Vec<u8>>>,
    /// A mask has been asked for, and the reach is being worked out: a flood measured for one
    /// fill (a tap, a point moved on to another colour) never starts it.
    asked: AtomicBool,
    reaching: AtomicBool,
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
        let distance = Arc::new(pixels.as_chunks::<4>().0.iter().take(count).map(|p| distance(p, &colour)).collect());
        Some(Flood {
            width,
            height,
            seed,
            distance,
            colour,
            reach: Arc::new(OnceLock::new()),
            asked: AtomicBool::new(false),
            reaching: AtomicBool::new(false),
        })
    }

    /// The pixel the fill is measured from, now `seed`, for a seed of the same colour (the
    /// distances stay as they are, and so does the reach: see [`Flood::mask`]). False, and left
    /// as it was, if `seed` is off the picture.
    pub fn reseed(&mut self, seed: (u32, u32)) -> bool {
        let on = seed.0 < self.width && seed.1 < self.height;
        if on {
            self.seed = seed;
        }
        on
    }

    /// The pixel the fill is measured from.
    pub fn seed(&self) -> (u32, u32) {
        self.seed
    }

    /// How much each pixel takes the fill at `threshold` (0..1): the pixels joined to the seed
    /// through pixels within reach, fully where the distance is within the threshold, fading out
    /// over [`FADE`] past it.
    pub fn mask(&self, threshold: f32) -> Mask {
        // The reach may be from an earlier seed of this colour: a fill from there that gets to
        // this seed is the area joined to both, the same from either, so it holds for that.
        let seed = self.seed.1 as usize * self.width as usize + self.seed.0 as usize;
        let cut = farthest(threshold);
        match self.reach.get().filter(|reach| reach.get(seed).is_some_and(|&r| r <= cut)) {
            Some(reach) => self.by_reach(reach, threshold),
            None => {
                if self.asked.swap(true, Ordering::Relaxed) && !self.reaching.swap(true, Ordering::Relaxed) {
                    self.work_out_reach();
                }
                self.by_scanlines(threshold, flood_here_most(self.distance.len()))
                    .unwrap_or_else(|| self.by_runs(threshold))
            }
        }
    }

    /// [`Flood::mask`] by each pixel's reach: one pass.
    fn by_reach(&self, reach: &[u8], threshold: f32) -> Mask {
        let weights = weights(threshold);
        let cut = farthest(threshold);
        let mut m = Mask::empty(self.width, self.height);
        for ((v, &r), &d) in m.data.iter_mut().zip(reach).zip(self.distance.iter()) {
            if r <= cut {
                *v = weights.get(usize::from(d)).copied().unwrap_or(0);
            }
        }
        m
    }

    /// [`Flood::mask`] by a flood on this thread: scanlines from the seed, each span filled whole,
    /// then the runs above and below it seeded; none once it has filled more than `most` pixels
    /// (a large fill is quicker on every core, [`Flood::by_runs`]).
    fn by_scanlines(&self, threshold: f32, most: usize) -> Option<Mask> {
        let weights = weights(threshold);
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
        let mut filled = 0;
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
            filled += r + 1 - l;
            if filled > most {
                return None;
            }
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
        Some(m)
    }

    /// [`Flood::mask`] by a flood, worked out on every core ([`joined`]).
    fn by_runs(&self, threshold: f32) -> Mask {
        let weights = weights(threshold);
        let (w, h) = (self.width as usize, self.height as usize);
        let seed = (self.seed.0 as usize, self.seed.1 as usize);
        let data = joined(&self.distance, w, h, seed, farthest(threshold), |i| {
            self.distance.get(i).and_then(|&d| weights.get(usize::from(d))).copied().unwrap_or(0)
        });
        Mask { width: self.width, height: self.height, data }
    }

    /// Starts working out each pixel's reach on a thread of its own (none for very large
    /// pictures, nor if a thread can't start: the fills stay floods). The thread stops if the
    /// flood is let go first.
    fn work_out_reach(&self) {
        // a distance and a reach a pixel, and lists of up to four bytes a pixel
        if !reach_fits(u64::from(self.width) * u64::from(self.height), 6) {
            return;
        }
        let (distance, reach) = (self.distance.clone(), self.reach.clone());
        let (w, h, seed) = (self.width as usize, self.height as usize, (self.seed.0 as usize, self.seed.1 as usize));
        let _ = std::thread::Builder::new().name("flood reach".to_owned()).spawn(move || {
            let wanted = || Arc::strong_count(&reach) > 1;
            if let Some(found) = reaches(&distance, w, h, seed, u8::MAX, &wanted) {
                let _ = reach.set(found);
            }
        });
    }
}

/// Each pixel's reach (see [`Flood::reach`]) over `far`, the distances (0 to `top`) of a `w` x
/// `h` picture, from `seed`: a flood level by level, from the least distance up, each level's
/// scanlines filling what it reaches. A pixel past the level it is met at is reached at its own
/// distance, whichever way it is met, so it waits once in that level's list. What no level below
/// `top` reaches is reached at it. None if it is no longer `wanted` (asked now and then).
pub(crate) fn reaches<T>(
    far: &[T],
    w: usize,
    h: usize,
    seed: (usize, usize),
    top: T,
    wanted: &dyn Fn() -> bool,
) -> Option<Vec<T>>
where
    T: Copy + Ord + Into<usize> + TryFrom<usize>,
{
    // not yet reached (or reached at the top)
    let open_mark = top;
    let mut reach = vec![open_mark; w * h];
    // pixels waiting in a later level's list, a bit each
    let mut waiting = vec![0u64; (w * h).div_ceil(64)];
    let mut levels: Vec<Vec<u32>> = vec![Vec::new(); top.into() + 1];
    let index = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    let start = seed.1 * w + seed.0;
    if let Some(list) = far.get(start).and_then(|&d| levels.get_mut(d.into())) {
        list.push(index(start));
    }
    let mut spans = 0u32;
    for level in 0..top.into() {
        let Ok(lv) = T::try_from(level) else { break };
        let open = |i: usize, reach: &[T]| reach.get(i) == Some(&open_mark) && far.get(i).is_some_and(|&d| d <= lv);
        while let Some(i) = levels.get_mut(level).and_then(Vec::pop) {
            let i = i as usize;
            if !open(i, &reach) {
                continue;
            }
            spans = spans.wrapping_add(1);
            if spans.is_multiple_of(4096) && !wanted() {
                return None;
            }
            let (y, x) = (i / w, i % w);
            let row = y * w;
            let mut l = x;
            while l > 0 && open(row + l - 1, &reach) {
                l -= 1;
            }
            let mut r = x;
            while r + 1 < w && open(row + r + 1, &reach) {
                r += 1;
            }
            if let Some(span) = reach.get_mut(row + l..=row + r) {
                span.fill(lv);
            }
            // what meets the span: runs within this level go on now, pixels past it wait (once)
            // for their own
            let mut meet = |j: usize, levels: &mut Vec<Vec<u32>>, reach: &[T]| -> bool {
                let Some(&d) = far.get(j) else { return false };
                if reach.get(j) != Some(&open_mark) {
                    return false;
                }
                if d <= lv {
                    if let Some(list) = levels.get_mut(level) {
                        list.push(index(j));
                    }
                    return true;
                }
                if let Some(word) = waiting.get_mut(j / 64) {
                    let bit = 1u64 << (j % 64);
                    if *word & bit == 0 {
                        *word |= bit;
                        if let Some(list) = levels.get_mut(d.into()) {
                            list.push(index(j));
                        }
                    }
                }
                false
            };
            if l > 0 {
                meet(row + l - 1, &mut levels, &reach);
            }
            if r + 1 < w {
                meet(row + r + 1, &mut levels, &reach);
            }
            for ny in [y.wrapping_sub(1), y + 1] {
                if ny >= h {
                    continue;
                }
                let nrow = ny * w;
                let mut xi = l;
                while xi <= r {
                    if meet(nrow + xi, &mut levels, &reach) {
                        // the rest of this run is reached from there
                        while xi < r && open(nrow + xi + 1, &reach) {
                            xi += 1;
                        }
                    }
                    xi += 1;
                }
            }
        }
    }
    Some(reach)
}

/// The pixels of a `w` x `h` picture joined to `seed` (side by side or one above the other)
/// through pixels whose `far` is at most `cut`, each given `value(i)` (its index), the rest 0. The
/// rows' runs within the cut are found in bands side by side, each run joined to those it
/// touches in the row above (a union-find), then the bands' edges are joined and the seed's runs
/// filled, again side by side: a flood's result, on every core.
pub(crate) fn joined<T: Copy + PartialOrd + Sync>(
    far: &[T],
    w: usize,
    h: usize,
    seed: (usize, usize),
    cut: T,
    value: impl Fn(usize) -> u8 + Sync,
) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let start = seed.1 * w + seed.0;
    if seed.0 >= w || seed.1 >= h || far.len() < w * h || !far.get(start).is_some_and(|&d| d <= cut) {
        return out;
    }
    let band = h.div_ceil(crate::par::cores()).max(1);
    let bands: Vec<(usize, usize)> = (0..h).step_by(band).map(|y0| (y0, (y0 + band).min(h))).collect();
    // each band's runs (first and last x), where each of its rows starts in them, and their
    // union-find, its runs joined to those above them within it
    let found = crate::par::side_by_side(&bands, |&(y0, y1)| {
        let mut runs: Vec<[u32; 2]> = Vec::new();
        let mut rows: Vec<usize> = Vec::with_capacity(y1 - y0 + 1);
        let mut parent: Vec<u32> = Vec::new();
        for y in y0..y1 {
            rows.push(runs.len());
            let line = far.get(y * w..(y + 1) * w).unwrap_or_default();
            let mut from: Option<usize> = None;
            for (x, &d) in line.iter().enumerate() {
                match (d <= cut, from) {
                    (true, None) => from = Some(x),
                    (false, Some(s)) => {
                        runs.push([s as u32, x as u32 - 1]);
                        from = None;
                    }
                    _ => {}
                }
            }
            if let Some(s) = from {
                runs.push([s as u32, w as u32 - 1]);
            }
            parent.extend(parent.len() as u32..runs.len() as u32);
            if let (Some(&above), Some(&here)) = (rows.len().checked_sub(2).and_then(|i| rows.get(i)), rows.last()) {
                join_runs(&runs, &mut parent, above..here, here..runs.len());
            }
        }
        rows.push(runs.len());
        (runs, rows, parent)
    });
    // all of them in one union-find, each band's runs after the last band's
    let mut runs: Vec<[u32; 2]> = Vec::new();
    let mut parent: Vec<u32> = Vec::new();
    // where each row's runs start in `runs`, and the end
    let mut rows: Vec<usize> = Vec::with_capacity(h + 1);
    for (band_runs, band_rows, band_parent) in found.into_iter().flatten() {
        let offset = runs.len();
        rows.extend(band_rows.iter().take(band_rows.len().saturating_sub(1)).map(|r| r + offset));
        parent.extend(band_parent.iter().map(|p| p + offset as u32));
        runs.extend(band_runs);
    }
    rows.push(runs.len());
    if rows.len() != h + 1 {
        return out;
    }
    let row = |y: usize| rows.get(y).copied().unwrap_or(0)..rows.get(y + 1).copied().unwrap_or(0);
    for &(y0, _) in bands.iter().skip(1) {
        join_runs(&runs, &mut parent, row(y0 - 1), row(y0));
    }
    let x = seed.0 as u32;
    let Some(first) = row(seed.1).find(|&i| runs.get(i).is_some_and(|&[x0, x1]| x0 <= x && x <= x1)) else {
        return out;
    };
    let mine = run_root(&mut parent, first);
    let inside: Vec<bool> = (0..runs.len()).map(|i| run_root(&mut parent, i) == mine).collect();
    // the seed's runs filled, band by band side by side
    std::thread::scope(|scope| {
        for ((y0, y1), part) in bands.iter().copied().zip(out.chunks_mut(band * w)) {
            let (runs, inside, value, row) = (&runs, &inside, &value, &row);
            scope.spawn(move || {
                for y in y0..y1 {
                    for i in row(y) {
                        let (Some(&[x0, x1]), Some(true)) = (runs.get(i), inside.get(i).copied()) else { continue };
                        let at = (y - y0) * w;
                        let base = y * w;
                        for (x, v) in
                            part.get_mut(at + x0 as usize..=at + x1 as usize).unwrap_or_default().iter_mut().enumerate()
                        {
                            *v = value(base + x0 as usize + x);
                        }
                    }
                }
            });
        }
    });
    out
}

/// Joins each run in `below` to each in `above` it touches (the rows next to each other, runs in
/// order along them).
fn join_runs(runs: &[[u32; 2]], parent: &mut [u32], above: std::ops::Range<usize>, below: std::ops::Range<usize>) {
    let (mut a, mut b) = (above.start, below.start);
    while a < above.end && b < below.end {
        let (Some(&[a0, a1]), Some(&[b0, b1])) = (runs.get(a), runs.get(b)) else { return };
        if a0 <= b1 && b0 <= a1 {
            let (ra, rb) = (run_root(parent, a), run_root(parent, b));
            if ra != rb
                && let Some(p) = parent.get_mut(ra.max(rb))
            {
                *p = ra.min(rb) as u32;
            }
        }
        if a1 < b1 {
            a += 1;
        } else {
            b += 1;
        }
    }
}

/// The run that stands for every run joined to `i` (halving the way there as it goes).
fn run_root(parent: &mut [u32], mut i: usize) -> usize {
    loop {
        let Some(&p) = parent.get(i) else { return i };
        if p as usize == i {
            return i;
        }
        let up = parent.get(p as usize).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = up;
        }
        i = up as usize;
    }
}

/// The farthest distance a fill at `threshold` reaches (the weights only fall with distance).
fn farthest(threshold: f32) -> u8 {
    weights(threshold).iter().rposition(|&v| v > 0).unwrap_or(0) as u8
}

/// How much a pixel at each distance takes a fill at `threshold` (0..1): fully within it,
/// fading out over [`FADE`] past it.
fn weights(threshold: f32) -> [u8; 256] {
    let t = threshold.clamp(0.0, 1.0) * 255.0;
    let fade = FADE * 255.0;
    std::array::from_fn(|d| {
        let d = d as f32;
        if d <= t { 255 } else { (((t + fade - d) / fade).clamp(0.0, 1.0) * 255.0).round() as u8 }
    })
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

/// Steps between neighbouring pixels in thirds of a pixel: across or down, and diagonally.
const STEP: u16 = 3;
const DIAGONAL: u16 = 4;
/// Farther than any step measures.
const FAR: u16 = u16::MAX / 2;

/// The widest gap a fill can be kept from going through, pixels.
pub const MOST_GAP: u32 = 32;

/// `mask` (a fill from `seed`) kept from going through gaps up to `gap` pixels wide in what
/// bounds it: the places wider than that are rooms, each pixel goes with the room nearest it,
/// and the fill keeps the seed's room's pixels (a gap parts down its middle). As it was when
/// `gap` is 0, the seed is itself somewhere no wider than the gap, or no gap parts the fill.
pub fn close_gaps(mask: &Mask, seed: (u32, u32), gap: u32) -> Mask {
    let (w, h) = (mask.width as usize, mask.height as usize);
    let gap = gap.min(MOST_GAP);
    let seeded = (seed.0 as usize) < w && mask.data.get(seed.1 as usize * w + seed.0 as usize).is_some_and(|&v| v > 0);
    let Some((x0, y0, x1, y1)) = filled_box(mask).filter(|_| gap > 0 && seeded) else { return mask.clone() };
    // the fill's box with a pixel round it: there, in the picture, a pixel of no fill (a wall);
    // past the picture's edge, open
    let (ox, oy) = (x0 as isize - 1, y0 as isize - 1);
    let (bw, bh) = (x1 - x0 + 3, y1 - y0 + 3);
    let cell = |i: usize| -> Option<bool> {
        let (x, y) = ((i % bw) as isize + ox, (i / bw) as isize + oy);
        if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
            return None;
        }
        Some(mask.data.get(y as usize * w + x as usize).is_some_and(|&v| v > 0))
    };
    let filled: Vec<bool> = (0..bw * bh).map(|i| cell(i) == Some(true)).collect();
    // each pixel's distance from the nearest wall
    let mut walls: Vec<u16> = (0..bw * bh).map(|i| if cell(i) == Some(false) { 0 } else { FAR }).collect();
    chamfer(&mut walls, None, &filled, bw, bh);
    // the rooms: farther from the walls than half the gap and a half
    let room = |d: u16| u32::from(d) * 2 > u32::from(STEP) * (gap + 1);
    let start = (seed.1 as isize - oy) as usize * bw + (seed.0 as isize - ox) as usize;
    if !walls.get(start).is_some_and(|&d| room(d)) {
        return mask.clone();
    }
    // 1 the seed's room, 2 any other
    let mut label = vec![0u8; bw * bh];
    if let Some(l) = label.get_mut(start) {
        *l = 1;
    }
    let mut todo = vec![start];
    while let Some(i) = todo.pop() {
        let (x, y) = (i % bw, i / bw);
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx >= bw || ny >= bh {
                continue;
            }
            let n = ny * bw + nx;
            if filled.get(n) == Some(&true)
                && walls.get(n).is_some_and(|&d| room(d))
                && let Some(l) = label.get_mut(n).filter(|l| **l == 0)
            {
                *l = 1;
                todo.push(n);
            }
        }
    }
    let mut parted = false;
    for ((l, &d), &f) in label.iter_mut().zip(&walls).zip(&filled) {
        if f && *l == 0 && room(d) {
            *l = 2;
            parted = true;
        }
    }
    if !parted {
        return mask.clone();
    }
    // every filled pixel with the room nearest it, going round the walls
    let mut reach: Vec<u16> = label.iter().map(|&l| if l > 0 { 0 } else { FAR }).collect();
    chamfer(&mut reach, Some(&mut label), &filled, bw, bh);
    let mut out = mask.clone();
    for (i, &l) in label.iter().enumerate() {
        let (x, y) = ((i % bw) as isize + ox, (i / bw) as isize + oy);
        if l == 2
            && x >= 0
            && y >= 0
            && let Some(v) = out.data.get_mut(y as usize * w + x as usize).filter(|_| (x as usize) < w)
        {
            *v = 0;
        }
    }
    out
}

/// The box (first and last column and row) of `mask`'s filled pixels, if any.
fn filled_box(mask: &Mask) -> Option<(usize, usize, usize, usize)> {
    let w = mask.width as usize;
    let mut b: Option<(usize, usize, usize, usize)> = None;
    for (y, row) in mask.data.chunks(w.max(1)).enumerate() {
        let (Some(first), Some(last)) = (row.iter().position(|&v| v > 0), row.iter().rposition(|&v| v > 0)) else {
            continue;
        };
        b = Some(b.map_or((first, y, last, y), |(x0, y0, x1, _)| (x0.min(first), y0, x1.max(last), y)));
    }
    b
}

/// Distances spread over the `filled` cells of a `bw` x `bh` grid from where they are smaller,
/// by chamfer steps, forwards and back until nothing changes (each cell's `label`, if given,
/// taken from where its distance came); the rest stay as they are.
fn chamfer(d: &mut [u16], mut label: Option<&mut Vec<u8>>, filled: &[bool], bw: usize, bh: usize) {
    let forward = [(-1, 0, STEP), (-1, -1, DIAGONAL), (0, -1, STEP), (1, -1, DIAGONAL)];
    let back = [(1, 0, STEP), (1, 1, DIAGONAL), (0, 1, STEP), (-1, 1, DIAGONAL)];
    for _ in 0..16 {
        let mut changed = false;
        for (steps, rows) in [(forward, false), (back, true)] {
            for k in 0..bh {
                let y = if rows { bh - 1 - k } else { k };
                for j in 0..bw {
                    let x = if rows { bw - 1 - j } else { j };
                    let i = y * bw + x;
                    if filled.get(i) != Some(&true) {
                        continue;
                    }
                    let mut best = d.get(i).copied().unwrap_or(FAR);
                    let mut from = None;
                    for &(dx, dy, step) in &steps {
                        let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else {
                            continue;
                        };
                        if nx >= bw || ny >= bh {
                            continue;
                        }
                        let n = ny * bw + nx;
                        let through = d.get(n).copied().unwrap_or(FAR).saturating_add(step);
                        if through < best {
                            best = through;
                            from = Some(n);
                        }
                    }
                    if let Some(n) = from {
                        changed = true;
                        if let Some(v) = d.get_mut(i) {
                            *v = best;
                        }
                        if let Some(l) = label.as_deref_mut() {
                            let take = l.get(n).copied().unwrap_or(0);
                            if let Some(v) = l.get_mut(i) {
                                *v = take;
                            }
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture `w` x `h` all open (255) but where `wall` says, as a fill's mask.
    fn rooms(w: u32, h: u32, wall: impl Fn(u32, u32) -> bool) -> Mask {
        let mut m = Mask::empty(w, h);
        for (i, v) in m.data.iter_mut().enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            *v = if wall(x, y) { 0 } else { 255 };
        }
        m
    }

    #[test]
    fn a_fill_closing_gaps_stays_out_of_what_a_narrow_gap_joins_it_to() {
        // two rooms, the wall between them (column 14) open for two rows
        let two = rooms(30, 12, |x, y| x == 14 && !(5..7).contains(&y));
        let closed = close_gaps(&two, (3, 3), 2);
        let at = |m: &Mask, x: u32, y: u32| m.data[(y * 30 + x) as usize];
        // its own room filled to the wall, corners and the picture's edge too; none past the gap
        assert!((0..12).all(|y| (0..14).all(|x| at(&closed, x, y) == 255)));
        assert!((0..12).all(|y| (15..30).all(|x| at(&closed, x, y) == 0)));
        // from the other room, the other way round
        let other = close_gaps(&two, (25, 8), 2);
        assert!((0..12).all(|y| (15..30).all(|x| at(&other, x, y) == 255) && (0..14).all(|x| at(&other, x, y) == 0)));
        // no closing, a gap wider than asked, or the seed in the gap itself: as it was
        assert_eq!(close_gaps(&two, (3, 3), 0).data, two.data);
        let wide = rooms(30, 12, |x, y| x == 14 && !(2..9).contains(&y));
        assert_eq!(close_gaps(&wide, (3, 3), 2).data, wide.data);
        assert_eq!(close_gaps(&two, (14, 5), 2).data, two.data);
        // one room alone, and the soft edge of a fill kept as it was
        let mut one = rooms(20, 20, |x, _| x == 19);
        one.data[45] = 128;
        assert_eq!(close_gaps(&one, (5, 5), 4).data, one.data);
    }

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
        let start = std::time::Instant::now();
        std::hint::black_box(flood.by_runs(0.2));
        println!("a fill by flood (before the reach is in): {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
        let start = std::time::Instant::now();
        std::hint::black_box(flood.by_scanlines(0.2, usize::MAX));
        println!("the same on one thread: {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
        let start = std::time::Instant::now();
        std::hint::black_box(reaches(&flood.distance, w as usize, h as usize, (100, 100), u8::MAX, &|| true));
        println!("each pixel's reach (on its own thread): {:.1} ms", start.elapsed().as_secs_f64() * 1000.0);
        // the thread's own (started by a second fill), before the fills are timed
        flood.mask(0.2);
        flood.mask(0.2);
        while flood.reach.get().is_none() {
            std::thread::yield_now();
        }
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

    /// The fill at `threshold` the plain way: a scanline flood over the pixels within reach.
    fn flooded(flood: &Flood, threshold: f32) -> Vec<u8> {
        let weights = weights(threshold);
        let cut = weights.iter().rposition(|&v| v > 0).unwrap_or(0) as u8;
        let (w, h) = (flood.width as usize, flood.height as usize);
        let mut m = vec![0u8; w * h];
        let mut stack = vec![(flood.seed.0 as usize, flood.seed.1 as usize)];
        while let Some((x, y)) = stack.pop() {
            let i = y * w + x;
            if m[i] != 0 || flood.distance[i] > cut {
                continue;
            }
            m[i] = weights[usize::from(flood.distance[i])].max(1);
            for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                if nx < w && ny < h {
                    stack.push((nx, ny));
                }
            }
        }
        // reached but weighed nothing (past the fade): as the masks keep it
        for (v, &d) in m.iter_mut().zip(flood.distance.iter()) {
            if *v != 0 {
                *v = weights[usize::from(d)];
            }
        }
        m
    }

    #[test]
    fn the_flood_by_reach_fills_what_a_plain_flood_does() {
        for seed in 0u32..40 {
            let mut state = seed;
            let mut next = |n: u32| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) % n.max(1)
            };
            let (w, h) = (8 + next(40), 8 + next(30));
            // blobs of a few greys over noise, so there are walls, gaps and soft steps
            let greys: Vec<u8> = (0..4).map(|_| next(256) as u8).collect();
            let mut px = vec![0u8; (w * h * 4) as usize];
            for p in px.chunks_mut(4) {
                let g = greys[next(4) as usize].saturating_add(next(20) as u8);
                p.copy_from_slice(&[g, g, g, 255]);
            }
            let start = (next(w), next(h));
            let Some(mut flood) = Flood::new(w, h, &px, start) else { continue };
            let at = (start.0 as usize, start.1 as usize);
            let reach = reaches(&flood.distance, w as usize, h as usize, at, u8::MAX, &|| true).unwrap();
            for threshold in [0.0f32, 0.03, 0.1, 0.25, 0.5, 0.8, 1.0] {
                let plain = flooded(&flood, threshold);
                assert_eq!(flood.by_reach(&reach, threshold).data, plain, "seed {seed} threshold {threshold}");
                assert_eq!(flood.by_runs(threshold).data, plain, "seed {seed} threshold {threshold}");
                let here = flood.by_scanlines(threshold, usize::MAX).map(|m| m.data);
                assert_eq!(here, Some(plain.clone()), "seed {seed} threshold {threshold}");
            }
            // moved on to other pixels of the seed's colour, the reach kept from the first
            let _ = flood.reach.set(reach);
            let same: Vec<(u32, u32)> = px
                .chunks(4)
                .enumerate()
                .filter(|(_, p)| *p == flood.colour)
                .map(|(i, _)| (i as u32 % w, i as u32 / w))
                .take(6)
                .collect();
            for to in same {
                assert!(flood.reseed(to));
                for threshold in [0.0f32, 0.03, 0.1, 0.25, 0.5, 0.8, 1.0] {
                    let plain = flooded(&flood, threshold);
                    assert_eq!(flood.mask(threshold).data, plain, "seed {seed} moved to {to:?} threshold {threshold}");
                }
            }
        }
    }

    #[test]
    fn a_large_fill_goes_over_to_every_core_and_comes_out_the_same() {
        let px: Vec<u8> =
            (0..64u32 * 64).flat_map(|i| if i % 64 == 40 { [0, 0, 0, 255] } else { [200, 0, 0, 255] }).collect();
        let flood = Flood::new(64, 64, &px, (3, 3)).unwrap();
        // left of the line: 40 columns, more than the one thread is let fill
        assert!(flood.by_scanlines(0.1, 64 * 64 / FLOOD_HERE_MOST).is_none());
        let filled = flood.mask(0.1);
        assert_eq!(filled.data, flooded(&flood, 0.1));
        assert_eq!(filled.data.iter().filter(|&&v| v == 255).count(), 40 * 64);
    }

    #[test]
    fn a_flood_works_its_reach_out_only_once_a_second_fill_is_asked_for() {
        let px: Vec<u8> = (0..64u32 * 64).flat_map(|i| [(i % 7 * 30) as u8, 0, 0, 255]).collect();
        let flood = Flood::new(64, 64, &px, (3, 3)).unwrap();
        let first = flood.mask(0.1);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(flood.reach.get().is_none(), "one fill, no reach");
        assert_eq!(flood.mask(0.1).data, first.data);
        let start = std::time::Instant::now();
        while flood.reach.get().is_none() && start.elapsed().as_secs() < 10 {
            std::thread::yield_now();
        }
        assert!(flood.reach.get().is_some(), "a second fill starts the reach");
        assert_eq!(flood.mask(0.1).data, first.data);
    }

    #[test]
    fn the_reach_is_worked_out_only_where_its_memory_fits() {
        // a 4 GB tablet (a 2 GiB layer budget): even 8K x 4K fits; a 2 GB one (1 GiB): a 4K
        // canvas's reach fits, an 8K x 4K one's doesn't; nothing past 8K x 4K anywhere
        let gib = 1u64 << 30;
        assert!(reach_fits_within(8192 * 4096, 8, 2 * gib));
        assert!(reach_fits_within(3840 * 2160, 8, gib));
        assert!(!reach_fits_within(8192 * 4096, 6, gib));
        assert!(!reach_fits_within(REACH_MOST + 1, 1, 64 * gib));
    }

    #[test]
    fn the_reach_stops_once_no_one_wants_it() {
        // a step of distance a column, so each level adds spans on every row
        let far: Vec<u8> = (0..512 * 512).map(|i| (i % 256) as u8).collect();
        assert!(reaches(&far, 512, 512, (0, 0), u8::MAX, &|| false).is_none());
        assert!(reaches(&far, 512, 512, (0, 0), u8::MAX, &|| true).is_some());
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
