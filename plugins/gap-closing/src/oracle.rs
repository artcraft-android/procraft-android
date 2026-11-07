//! The rule as the app first had it, walking whole distance maps: what the tests hold [`Closer`] to.
//! Without the limits it had (16 sweeps, 16-bit distances), which gave up in long corridors.
//!
//! [`Closer`]: crate::closer::Closer

pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Clone for Mask {
    fn clone(&self) -> Mask {
        Mask { width: self.width, height: self.height, data: self.data.clone() }
    }
}

/// Steps between neighbouring pixels in thirds of a pixel: across or down, and diagonally.
const STEP: u32 = 3;
const DIAGONAL: u32 = 4;
/// Farther than any step measures.
const FAR: u32 = u32::MAX / 2;

/// `mask` (a fill from `seed`) kept from going through gaps up to `gap` pixels wide in what
/// bounds it: the places wider than that are rooms, each pixel goes with the room nearest it,
/// and the fill keeps the seed's room's pixels (a gap parts down its middle). As it was when
/// `gap` is 0, the seed is itself somewhere no wider than the gap, or no gap parts the fill.
pub fn close_gaps(mask: &Mask, seed: (u32, u32), gap: u32) -> Mask {
    let (w, h) = (mask.width as usize, mask.height as usize);
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
    let mut walls: Vec<u32> = (0..bw * bh).map(|i| if cell(i) == Some(false) { 0 } else { FAR }).collect();
    chamfer(&mut walls, None, &filled, bw, bh);
    // the rooms: farther from the walls than half the gap and a half
    let room = |d: u32| u32::from(d) * 2 > u32::from(STEP) * (gap + 1);
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
    let mut reach: Vec<u32> = label.iter().map(|&l| if l > 0 { 0 } else { FAR }).collect();
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
fn chamfer(d: &mut [u32], mut label: Option<&mut Vec<u8>>, filled: &[bool], bw: usize, bh: usize) {
    let forward = [(-1, 0, STEP), (-1, -1, DIAGONAL), (0, -1, STEP), (1, -1, DIAGONAL)];
    let back = [(1, 0, STEP), (1, 1, DIAGONAL), (0, 1, STEP), (-1, 1, DIAGONAL)];
    loop {
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
