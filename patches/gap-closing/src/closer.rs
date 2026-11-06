//! A fill kept from going through gaps up to `gap` pixels wide in what bounds it, on a mask of a bit
//! a pixel: the places wider than that are rooms, each pixel goes with the room nearest it, and the
//! fill keeps the seed's room's pixels, so a gap parts down its middle. As it was when the seed is
//! itself somewhere no wider than the gap, or no gap parts the fill.
//!
//! Distances go by thirds of a pixel: 3 across or down, 4 diagonally. Rooms come of spreading the
//! walls by that measure's ball, a row at a time. They are told apart by their runs, so what they
//! cover costs nothing. Then the rest goes to the nearest room by a wave on the same measure, a word
//! at a time, touching only what it reaches. All of it fits in a few planes of a bit a pixel.

/// Steps between neighbouring pixels in thirds of a pixel: across or down, and diagonally.
const STEP: u32 = 3;
const DIAGONAL: u32 = 4;
/// The wave's times in turn: one for each it can be ahead by.
const SLOTS: usize = DIAGONAL as usize + 1;

pub struct Closer {
    pub w: usize,
    pub h: usize,
    /// Words a row.
    pub n: usize,
    pub gap: u32,
    pub seed: (usize, usize),
    /// Filled pixels, as they come in; used up by [`Closer::run`].
    pub filled: Vec<u64>,
    /// What to take out, as it goes back.
    pub gone: Vec<u64>,
}

/// Word index and bits, the top bit of `at` saying which room's wave it is.
#[derive(Clone, Copy)]
struct Wave {
    at: u32,
    bits: u64,
}

const OTHER: u32 = 1 << 31;

/// `from` spread one pixel either way into `to`.
fn spread(from: &[u64], to: &mut [u64]) {
    let n = from.len();
    for i in 0..n {
        let a = from[i];
        let carry_in = if i > 0 { from[i - 1] >> 63 } else { 0 };
        let carry_back = if i + 1 < n { from[i + 1] << 63 } else { 0 };
        to[i] = a | a << 1 | carry_in | a >> 1 | carry_back;
    }
}

impl Closer {
    pub fn new(w: usize, h: usize, seed: (usize, usize), gap: u32) -> Closer {
        let n = w.div_ceil(64);
        Closer { w, h, n, gap, seed, filled: vec![0; n * h], gone: Vec::new() }
    }

    fn is_filled(&self, x: usize, y: usize) -> bool {
        x < self.w && y < self.h && self.filled[y * self.n + x / 64] >> (x % 64) & 1 == 1
    }

    /// How wide the ball is at each row off its middle: the pixels no further than half the gap
    /// and a half, by the chamfer, from a wall aren't a room.
    fn ball(&self) -> Vec<usize> {
        let most = STEP * (self.gap + 1);
        let cost = |dx: u32, dy: u32| dx.max(dy) * STEP + dx.min(dy) * (DIAGONAL - STEP);
        let mut ball = Vec::new();
        let mut dy = 0;
        while 2 * cost(0, dy) <= most {
            let mut dx = 0;
            while 2 * cost(dx + 1, dy) <= most {
                dx += 1;
            }
            ball.push(dx as usize);
            dy += 1;
        }
        ball
    }

    /// The filled pixels further from every wall than the ball reaches.
    fn rooms(&self) -> Vec<u64> {
        let (n, h, w) = (self.n, self.h, self.w);
        let ball = self.ball();
        let reach = ball.len().saturating_sub(1);
        let widest = ball.first().copied().unwrap_or(0);
        let tail = if w % 64 == 0 { !0 } else { (1u64 << (w % 64)) - 1 };
        let span = 2 * reach + 1;
        // each row's walls spread 0..=widest pixels, for the rows in reach; an empty one is marked
        let mut ring = vec![0u64; span * (widest + 1) * n];
        let mut empty = vec![true; span];
        let mut rooms = vec![0u64; n * h];
        let mut near = vec![0u64; n];
        for row in 0..h + reach {
            if row < h {
                let slot = row % span;
                let base = slot * (widest + 1) * n;
                let mut any = 0;
                for i in 0..n {
                    let edge = if i + 1 == n { tail } else { !0 };
                    let wall = !self.filled[row * n + i] & edge;
                    ring[base + i] = wall;
                    any |= wall;
                }
                empty[slot] = any == 0;
                if any != 0 {
                    for k in 0..widest {
                        let (done, rest) = ring[base + k * n..].split_at_mut(n);
                        spread(done, &mut rest[..n]);
                    }
                }
            }
            if row < reach {
                continue;
            }
            let y = row - reach;
            near.fill(0);
            let (first, last) = (y.saturating_sub(reach), (y + reach).min(h - 1));
            for from in first..=last {
                let slot = from % span;
                if empty[slot] {
                    continue;
                }
                let k = ball[from.abs_diff(y)];
                let at = (slot * (widest + 1) + k) * n;
                for (o, v) in near.iter_mut().zip(&ring[at..at + n]) {
                    *o |= v;
                }
            }
            for i in 0..n {
                rooms[y * n + i] = self.filled[y * n + i] & !near[i];
            }
        }
        rooms
    }

    /// Takes out what goes with a room other than the seed's: whether anything does.
    pub fn run(&mut self) -> bool {
        let (n, h) = (self.n, self.h);
        let (sx, sy) = self.seed;
        if self.gap == 0 || !self.is_filled(sx, sy) {
            return false;
        }
        let rooms = self.rooms();

        // runs of each row, joined to what they touch above
        let mut runs: Vec<(u32, u32)> = Vec::new();
        let mut row_at: Vec<u32> = Vec::with_capacity(h + 1);
        let mut parent: Vec<u32> = Vec::new();
        fn find(parent: &mut [u32], mut a: u32) -> u32 {
            while parent[a as usize] != a {
                let up = parent[parent[a as usize] as usize];
                parent[a as usize] = up;
                a = up;
            }
            a
        }
        for y in 0..h {
            row_at.push(runs.len() as u32);
            let row = &rooms[y * n..(y + 1) * n];
            let mut open: Option<u32> = None;
            for (i, &word) in row.iter().enumerate() {
                let mut v = word;
                let mut at = 0;
                while at < 64 {
                    if open.is_some() {
                        // looking for the end of a run
                        let ones = (!v).trailing_zeros().min(64 - at);
                        at += ones;
                        v = if ones == 64 { 0 } else { v >> ones };
                        if at < 64 {
                            let start = open.take().unwrap_or(0);
                            runs.push((start, (i * 64) as u32 + at - 1));
                        }
                    } else {
                        if v == 0 {
                            break;
                        }
                        let zeros = v.trailing_zeros();
                        at += zeros;
                        v >>= zeros;
                        open = Some((i * 64) as u32 + at);
                    }
                }
            }
            if let Some(start) = open {
                runs.push((start, (n * 64) as u32 - 1));
            }
            let (start, end) = (row_at[y] as usize, runs.len());
            for r in start..end {
                parent.push(r as u32);
            }
            if y > 0 {
                let (mut a, a_end) = (row_at[y - 1] as usize, start);
                let mut b = start;
                while a < a_end && b < end {
                    let (ra, rb) = (runs[a], runs[b]);
                    if ra.0 <= rb.1 && rb.0 <= ra.1 {
                        let (pa, pb) = (find(&mut parent, a as u32), find(&mut parent, b as u32));
                        parent[pa.max(pb) as usize] = pa.min(pb);
                    }
                    if ra.1 < rb.1 {
                        a += 1;
                    } else {
                        b += 1;
                    }
                }
            }
        }
        row_at.push(runs.len() as u32);

        // the seed must be in a room, and some other room there must be
        let seed_run = (row_at[sy]..row_at[sy + 1]).find(|&r| {
            let (a, b) = runs[r as usize];
            a as usize <= sx && sx <= b as usize
        });
        let Some(seed_run) = seed_run else { return false };
        let seed_room = find(&mut parent, seed_run);
        let mut gone = vec![0u64; n * h];
        let mut parted = false;
        for y in 0..h {
            for r in row_at[y]..row_at[y + 1] {
                if find(&mut parent, r) == seed_room {
                    continue;
                }
                parted = true;
                let (a, b) = runs[r as usize];
                let (a, b) = (a as usize, b as usize);
                let (wa, wb) = (a / 64, b / 64);
                for i in wa..=wb {
                    let lo = if i == wa { !0u64 << (a % 64) } else { !0 };
                    let hi = if i == wb { !0u64 >> (63 - b % 64) } else { !0 };
                    gone[y * n + i] |= lo & hi;
                }
            }
        }
        if !parted {
            return false;
        }
        drop(runs);
        drop(parent);

        // what no room holds yet
        let mut free = std::mem::take(&mut self.filled);
        for (f, r) in free.iter_mut().zip(&rooms) {
            *f &= !r;
        }
        let mut ortho: [Vec<Wave>; SLOTS] = Default::default();
        let mut diag: [Vec<Wave>; SLOTS] = Default::default();
        let touches_free = |free: &[u64], i: usize| {
            let (y, x) = (i / n, i % n);
            let (y0, y1, x0, x1) = (y.saturating_sub(1), (y + 1).min(h - 1), x.saturating_sub(1), (x + 1).min(n - 1));
            (y0..=y1).any(|yy| (x0..=x1).any(|xx| free[yy * n + xx] != 0))
        };
        for i in 0..n * h {
            if rooms[i] != 0 && touches_free(&free, i) {
                let (seeded, other) = (rooms[i] & !gone[i], rooms[i] & gone[i]);
                for (at, bits) in [(i as u32, seeded), (i as u32 | OTHER, other)] {
                    if bits != 0 {
                        ortho[STEP as usize].push(Wave { at, bits });
                        diag[DIAGONAL as usize].push(Wave { at, bits });
                    }
                }
            }
        }
        drop(rooms);

        let mut t = 0usize;
        let mut idle = 0;
        while idle < SLOTS {
            t += 1;
            let slot = t % SLOTS;
            if ortho[slot].is_empty() && diag[slot].is_empty() {
                idle += 1;
                continue;
            }
            idle = 0;
            let (next_ortho, next_diag) = ((t + STEP as usize) % SLOTS, (t + DIAGONAL as usize) % SLOTS);
            let straight = std::mem::take(&mut ortho[slot]);
            let slanted = std::mem::take(&mut diag[slot]);
            let mut reach =
                |i: isize, bits: u64, side: u32, ortho: &mut [Vec<Wave>; SLOTS], diag: &mut [Vec<Wave>; SLOTS]| {
                    let Some(cell) = free.get_mut(i as usize).filter(|_| i >= 0 && bits != 0) else { return };
                    let add = *cell & bits;
                    if add == 0 {
                        return;
                    }
                    *cell &= !add;
                    if side != 0 {
                        gone[i as usize] |= add;
                    }
                    let at = i as u32 | side;
                    ortho[next_ortho].push(Wave { at, bits: add });
                    diag[next_diag].push(Wave { at, bits: add });
                };
            for e in &straight {
                let (side, i) = (e.at & OTHER, (e.at & !OTHER) as usize);
                let (x, b) = (i % n, e.bits);
                let i = i as isize;
                let row = n as isize;
                reach(i, b << 1 | b >> 1, side, &mut ortho, &mut diag);
                if x + 1 < n {
                    reach(i + 1, b >> 63, side, &mut ortho, &mut diag);
                }
                if x > 0 {
                    reach(i - 1, b << 63, side, &mut ortho, &mut diag);
                }
                reach(i - row, b, side, &mut ortho, &mut diag);
                reach(i + row, b, side, &mut ortho, &mut diag);
            }
            for e in &slanted {
                let (side, i) = (e.at & OTHER, (e.at & !OTHER) as usize);
                let (x, b) = (i % n, e.bits);
                let i = i as isize;
                let row = n as isize;
                for up in [i - row, i + row] {
                    reach(up, b << 1 | b >> 1, side, &mut ortho, &mut diag);
                    if x + 1 < n {
                        reach(up + 1, b >> 63, side, &mut ortho, &mut diag);
                    }
                    if x > 0 {
                        reach(up - 1, b << 63, side, &mut ortho, &mut diag);
                    }
                }
            }
        }
        self.gone = gone;
        true
    }
}
