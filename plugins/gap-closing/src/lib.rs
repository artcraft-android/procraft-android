//! ColorDrop gap closing: a fill keeps out of gaps up to this wide in the lines that bound it. It's
//! a plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app hands over the fill as it is let go, a row of bits at a time, and clears what comes back.
//! The rule is all in [`closer`]. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

mod closer;
#[cfg(test)]
mod oracle;

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use closer::Closer;

/// The slider, whole pixels; the app makes its calls one at a time.
static GAP: AtomicU32 = AtomicU32::new(0);

struct Job {
    closer: Closer,
    /// Where a row goes in and comes out.
    row: Vec<u64>,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

/// The slider's value (the only control, so `index` says nothing).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(_index: i32, value: f32) {
    GAP.store(value as u32, Ordering::Relaxed);
}

/// Whether a fill has anything to come here for: with the slider at Off it doesn't.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_fill_mask() -> i32 {
    i32::from(GAP.load(Ordering::Relaxed) > 0)
}

/// A new fill of `width` × `height`, from (`seed_x`, `seed_y`): where a row of bits goes.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_fill_begin(width: i32, height: i32, seed_x: i32, seed_y: i32) -> i32 {
    let seed = (seed_x as usize, seed_y as usize);
    let closer = Closer::new(width as usize, height as usize, seed, GAP.load(Ordering::Relaxed));
    let row = vec![0; closer.n];
    let at = row.as_ptr() as i32;
    *JOB.lock().unwrap_or_else(|e| e.into_inner()) = Some(Job { closer, row });
    at
}

/// The row at the buffer is row `y` of the fill.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_fill_row(y: i32) {
    let mut job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let Some(job) = job.as_mut() else { return };
    let n = job.closer.n;
    let at = y as usize * n;
    job.closer.filled[at..at + n].copy_from_slice(&job.row);
}

/// Works it out: whether anything comes out of the fill.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_fill_run() -> i32 {
    let mut job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    job.as_mut().map_or(0, |job| i32::from(job.closer.run()))
}

/// Row `y` of what comes out, at the buffer.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_fill_out(y: i32) {
    let mut job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let Some(job) = job.as_mut() else { return };
    let n = job.closer.n;
    let at = y as usize * n;
    job.row.copy_from_slice(&job.closer.gone[at..at + n]);
}

#[cfg(test)]
mod tests {
    use crate::closer::Closer;
    use crate::oracle::{Mask, close_gaps};

    fn bits_of(mask: &Mask) -> Vec<u64> {
        let (w, h) = (mask.width as usize, mask.height as usize);
        let n = w.div_ceil(64);
        let mut bits = vec![0; n * h];
        for y in 0..h {
            for x in 0..w {
                if mask.data[y * w + x] > 0 {
                    bits[y * n + x / 64] |= 1 << (x % 64);
                }
            }
        }
        bits
    }

    /// `mask` as the rewrite leaves it.
    fn closed(mask: &Mask, seed: (u32, u32), gap: u32) -> Vec<u8> {
        let (w, h) = (mask.width as usize, mask.height as usize);
        let mut closer = Closer::new(w, h, (seed.0 as usize, seed.1 as usize), gap);
        closer.filled = bits_of(mask);
        let mut out = mask.data.clone();
        if closer.run() {
            for y in 0..h {
                for x in 0..w {
                    if closer.gone[y * closer.n + x / 64] >> (x % 64) & 1 == 1 {
                        out[y * w + x] = 0;
                    }
                }
            }
        }
        out
    }

    fn rect(data: &mut [u8], side: usize, x: std::ops::Range<usize>, y: std::ops::Range<usize>, value: u8) {
        for row in y {
            for col in x.clone() {
                data[row * side + col] = value;
            }
        }
    }

    /// Two rooms, a wall down the middle with an opening 4 tall.
    fn two_rooms(side: usize) -> Mask {
        let mut data = vec![255; side * side];
        rect(&mut data, side, side / 2..side / 2 + 6, 0..side, 0);
        rect(&mut data, side, side / 2..side / 2 + 6, side / 2..side / 2 + 4, 255);
        Mask { width: side as u32, height: side as u32, data }
    }

    /// A room each side, joined by a serpentine 4 wide, with dead-end slits in both.
    fn corridors(side: usize) -> Mask {
        let mut data = vec![255; side * side];
        let (third, two) = (side / 3, 2 * side / 3);
        rect(&mut data, side, third..third + 2, 0..side, 0);
        rect(&mut data, side, two..two + 2, 0..side, 0);
        let rows = side / 6;
        for k in 1..rows {
            let span = if k % 2 == 1 { third + 2..two - 8 } else { third + 10..two };
            rect(&mut data, side, span, k * 6..(k * 6 + 2).min(side), 0);
        }
        for x in (8..third).step_by(8).chain((two + 8..side).step_by(8)) {
            rect(&mut data, side, x..(x + 2).min(side), 3 * side / 4..side, 0);
        }
        rect(&mut data, side, third..third + 2, 1..5, 255);
        let last = (rows - 1) * 6 + 3;
        rect(&mut data, side, two..two + 2, last..(last + 3).min(side), 255);
        Mask { width: side as u32, height: side as u32, data }
    }

    /// A picture `w` x `h` all open but where `wall` says.
    fn rooms(w: u32, h: u32, wall: impl Fn(u32, u32) -> bool) -> Mask {
        let data = (0..w * h).map(|i| if wall(i % w, i / w) { 0 } else { 255 }).collect();
        Mask { width: w, height: h, data }
    }

    #[test]
    fn a_fill_stays_out_of_what_a_narrow_gap_joins_it_to() {
        // two rooms, the wall between them (column 14) open for two rows
        let two = rooms(30, 12, |x, y| x == 14 && !(5..7).contains(&y));
        let at = |m: &[u8], x: u32, y: u32| m[(y * 30 + x) as usize];
        // its own room filled to the wall, corners and the picture's edge too; none past the gap
        let kept = closed(&two, (3, 3), 2);
        assert!((0..12).all(|y| (0..14).all(|x| at(&kept, x, y) == 255)));
        assert!((0..12).all(|y| (15..30).all(|x| at(&kept, x, y) == 0)));
        // from the other room, the other way round
        let other = closed(&two, (25, 8), 2);
        assert!((0..12).all(|y| (15..30).all(|x| at(&other, x, y) == 255) && (0..14).all(|x| at(&other, x, y) == 0)));
        // a gap wider than asked: as it was
        let wide = rooms(30, 12, |x, y| x == 14 && !(2..9).contains(&y));
        assert_eq!(closed(&wide, (3, 3), 2), wide.data);
        // one room alone, and the soft edge of a fill kept as it was
        let mut one = rooms(20, 20, |x, _| x == 19);
        one.data[45] = 128;
        assert_eq!(closed(&one, (5, 5), 4), one.data);
    }

    #[test]
    fn two_rooms_part_as_the_rule_says() {
        for gap in [5, 8, 20] {
            let mask = two_rooms(200);
            assert_eq!(closed(&mask, (50, 50), gap), close_gaps(&mask, (50, 50), gap).data, "{gap}");
        }
    }

    #[test]
    fn long_corridors_part_as_the_rule_says() {
        let mask = corridors(300);
        assert_eq!(closed(&mask, (50, 30), 8), close_gaps(&mask, (50, 30), 8).data);
    }

    #[test]
    fn no_gap_or_a_seed_in_the_gap_leaves_it_as_it_was() {
        let mask = two_rooms(200);
        assert_eq!(closed(&mask, (50, 50), 0), mask.data);
        // no wider than the gap where the seed is: the opening itself
        assert_eq!(closed(&mask, (102, 101), 8), mask.data);
        // a gap wider than asked: it joins the rooms
        assert_eq!(closed(&mask, (50, 50), 2), mask.data);
        // the seed off the fill
        assert_eq!(closed(&mask, (102, 10), 8), mask.data);
    }

    #[test]
    fn a_width_not_a_whole_number_of_words_changes_nothing() {
        for side in [63, 64, 65, 127, 129, 191] {
            let mask = two_rooms(side);
            let seed = (side as u32 / 4, side as u32 / 4);
            assert_eq!(closed(&mask, seed, 6), close_gaps(&mask, seed, 6).data, "{side}");
        }
    }

    #[test]
    fn broken_lines_part_as_the_rule_says_but_for_ties() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move |most: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % most
        };
        let side = 160_usize;
        let (mut differ, mut filled) = (0, 0);
        for _ in 0..60 {
            let mut data = vec![255_u8; side * side];
            for _ in 0..4 + next(14) {
                let (mut x, mut y) = (next(side as u64) as f32, next(side as u64) as f32);
                let angle = next(628) as f32 / 100.0;
                let mut open = 0;
                for _ in 0..60 + next(200) {
                    if open > 0 {
                        open -= 1;
                    } else if next(40) == 0 {
                        open = 2 + next(8);
                    } else if x >= 0.0 && y >= 0.0 && (x as usize) < side && (y as usize) < side {
                        data[y as usize * side + x as usize] = 0;
                    }
                    x += angle.cos();
                    y += angle.sin();
                }
            }
            let mask = Mask { width: side as u32, height: side as u32, data };
            let seed = (next(side as u64) as u32, next(side as u64) as u32);
            let gap = 1 + next(16) as u32;
            let (old, new) = (close_gaps(&mask, seed, gap).data, closed(&mask, seed, gap));
            differ += old.iter().zip(&new).filter(|(a, b)| a != b).count();
            filled += mask.data.iter().filter(|&&v| v > 0).count();
        }
        assert!(differ * 200 < filled, "{differ} of {filled}");
    }
}
