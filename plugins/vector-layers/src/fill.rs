//! Fills: the region a colour dropped at a point fills, found on a picture of what is drawn, kept
//! out of gaps in the lines up to a width, and its outlines.
//!
//! What is drawn more than the threshold bounds a fill. With gaps closed, the fill first spreads
//! only where it is further than half the gap from anything drawn, so it can't run out through a gap
//! that narrow; then it grows back to the lines (going halfway into a gap, which closes it to the
//! eye), never through them. Last it reaches a pixel under the lines' edges, so drawn behind them
//! it leaves no light seam.

use std::collections::HashMap;

/// A picture of what is drawn: how much of each pixel (0..255), rows top to bottom.
pub(crate) struct Drawn<'a> {
    pub width: usize,
    pub height: usize,
    pub coverage: &'a [u8],
}

/// Far, for a distance not yet found.
const FAR: f32 = f32::INFINITY;

impl Drawn<'_> {
    /// The pixels a fill from `seed` covers, where what is drawn more than `limit` bounds it and
    /// gaps up to `gap` pixels wide are closed: true for each covered; none if the seed is on
    /// something drawn (or off the picture).
    pub(crate) fn fill(&self, seed: (usize, usize), limit: u8, gap: f32) -> Option<Vec<bool>> {
        let (w, h) = (self.width, self.height);
        if seed.0 >= w || seed.1 >= h || self.coverage.len() < w * h {
            return None;
        }
        let wall: Vec<bool> = self.coverage.iter().take(w * h).map(|c| *c > limit).collect();
        if wall[seed.1 * w + seed.0] {
            return None;
        }
        let r = (gap / 2.0).max(0.0);
        // where it may spread first: further than half the gap from what is drawn (from anywhere,
        // if the seed itself is nearer than that)
        let from_walls = distances(w, h, |i| wall[i], |_| false);
        let open: Vec<bool> = if r > 0.0 && from_walls[seed.1 * w + seed.0] > r {
            from_walls.iter().map(|d| *d > r).collect()
        } else {
            wall.iter().map(|b| !b).collect()
        };
        let core = flood(w, h, seed, &open);
        // grown back towards the lines, never through them
        let grown = if r > 0.0 {
            let from_core = distances(w, h, |i| core[i], |i| wall[i]);
            from_core.iter().zip(&wall).map(|(d, b)| !b && *d <= r + 1.0).collect()
        } else {
            core
        };
        // and a pixel under the lines' edges
        let mut out = grown.clone();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if !wall[i] || grown[i] {
                    continue;
                }
                let near = (y.saturating_sub(1)..(y + 2).min(h))
                    .any(|ny| (x.saturating_sub(1)..(x + 2).min(w)).any(|nx| grown[ny * w + nx]));
                out[i] = near;
            }
        }
        Some(out)
    }
}

/// How far each pixel of a `w` × `h` picture is from the nearest one `from` takes (chamfer
/// distances, pixels), passing over none `walls` takes (they stay far).
fn distances(w: usize, h: usize, from: impl Fn(usize) -> bool, walls: impl Fn(usize) -> bool) -> Vec<f32> {
    const STRAIGHT: f32 = 1.0;
    const DIAGONAL: f32 = std::f32::consts::SQRT_2;
    let mut d: Vec<f32> = (0..w * h).map(|i| if from(i) { 0.0 } else { FAR }).collect();
    let wall: Vec<bool> = (0..w * h).map(&walls).collect();
    let relax = |d: &mut Vec<f32>, i: usize, j: usize, step: f32| {
        if !wall[i] && !wall[j] && d[j] + step < d[i] {
            d[i] = d[j] + step;
        }
    };
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if x > 0 {
                relax(&mut d, i, i - 1, STRAIGHT);
            }
            if y > 0 {
                relax(&mut d, i, i - w, STRAIGHT);
                if x > 0 {
                    relax(&mut d, i, i - w - 1, DIAGONAL);
                }
                if x + 1 < w {
                    relax(&mut d, i, i - w + 1, DIAGONAL);
                }
            }
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if x + 1 < w {
                relax(&mut d, i, i + 1, STRAIGHT);
            }
            if y + 1 < h {
                relax(&mut d, i, i + w, STRAIGHT);
                if x + 1 < w {
                    relax(&mut d, i, i + w + 1, DIAGONAL);
                }
                if x > 0 {
                    relax(&mut d, i, i + w - 1, DIAGONAL);
                }
            }
        }
    }
    d
}

/// The pixels of a `w` × `h` picture reached from `seed` across those `open` takes, side by side.
fn flood(w: usize, h: usize, seed: (usize, usize), open: &[bool]) -> Vec<bool> {
    let mut out = vec![false; w * h];
    let mut stack = vec![seed];
    while let Some((x, y)) = stack.pop() {
        let i = y * w + x;
        if out[i] || !open[i] {
            continue;
        }
        // the run along the row, then the rows above and below it
        let (mut l, mut r) = (x, x);
        while l > 0 && open[i - (x - l) - 1] && !out[i - (x - l) - 1] {
            l -= 1;
        }
        while r + 1 < w && open[i + (r - x) + 1] && !out[i + (r - x) + 1] {
            r += 1;
        }
        for cx in l..=r {
            out[y * w + cx] = true;
            if y > 0 && open[(y - 1) * w + cx] && !out[(y - 1) * w + cx] {
                stack.push((cx, y - 1));
            }
            if y + 1 < h && open[(y + 1) * w + cx] && !out[(y + 1) * w + cx] {
                stack.push((cx, y + 1));
            }
        }
    }
    out
}

/// The outlines of the region `inside` covers (a `w` × `h` picture): closed loops along its pixels'
/// edges, the region on their right, which filled even-odd give it back exactly; each smoothed by as
/// much as `tolerance` pixels.
pub(crate) fn outlines(w: usize, h: usize, inside: &[bool], tolerance: f32) -> Vec<Vec<(f32, f32)>> {
    let at = |x: i64, y: i64| {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && inside[y as usize * w + x as usize]
    };
    let mut from: HashMap<(i64, i64), Vec<(i64, i64)>> = HashMap::new();
    let mut count = 0;
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if !at(x, y) {
                continue;
            }
            let mut edge = |a: (i64, i64), b: (i64, i64)| {
                from.entry(a).or_default().push(b);
                count += 1;
            };
            if !at(x, y - 1) {
                edge((x, y), (x + 1, y));
            }
            if !at(x + 1, y) {
                edge((x + 1, y), (x + 1, y + 1));
            }
            if !at(x, y + 1) {
                edge((x + 1, y + 1), (x, y + 1));
            }
            if !at(x - 1, y) {
                edge((x, y + 1), (x, y));
            }
        }
    }
    let mut loops = Vec::new();
    while count > 0 {
        let Some(&start) = from.iter().find(|(_, ends)| !ends.is_empty()).map(|(a, _)| a) else { break };
        let mut points = vec![start];
        let mut at_vertex = start;
        let mut came: Option<(i64, i64)> = None;
        while let Some(ends) = from.get_mut(&at_vertex) {
            // where two loops touch at a corner, the one turning right keeps them apart
            let pick = match (came, ends.len()) {
                (Some(dir), n) if n > 1 => {
                    let right = (-dir.1, dir.0);
                    ends.iter().position(|e| (e.0 - at_vertex.0, e.1 - at_vertex.1) == right).unwrap_or(0)
                }
                _ => 0,
            };
            if ends.is_empty() {
                break;
            }
            let next = ends.swap_remove(pick);
            count -= 1;
            came = Some((next.0 - at_vertex.0, next.1 - at_vertex.1));
            at_vertex = next;
            if next == start {
                break;
            }
            points.push(next);
        }
        let corners = corners_of(&points);
        if corners.len() >= 3 {
            loops.push(simplified(&corners, tolerance));
        }
    }
    loops
}

/// The points of a closed loop where it turns (those along a straight run left out).
fn corners_of(points: &[(i64, i64)]) -> Vec<(f32, f32)> {
    let n = points.len();
    (0..n)
        .filter(|&i| {
            let (a, b, c) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
            (b.0 - a.0) * (c.1 - b.1) != (b.1 - a.1) * (c.0 - b.0)
        })
        .map(|i| (points[i].0 as f32, points[i].1 as f32))
        .collect()
}

/// A closed loop with the points it can do without (within `tolerance` of the line past them) left
/// out, a staircase becoming the slope it climbs.
fn simplified(points: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    if points.len() < 8 || tolerance <= 0.0 {
        return points.to_vec();
    }
    // cut at the point furthest from the first, each half simplified
    let first = points[0];
    let far = (1..points.len())
        .max_by(|&a, &b| dist2(points[a], first).total_cmp(&dist2(points[b], first)))
        .unwrap_or(points.len() / 2);
    let mut out = Vec::new();
    keep(&points[..=far], tolerance, &mut out);
    out.pop();
    let mut back: Vec<(f32, f32)> = points[far..].to_vec();
    back.push(first);
    keep(&back, tolerance, &mut out);
    out.pop();
    if out.len() < 3 { points.to_vec() } else { out }
}

fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// `run`'s first point and those of it standing out from its ends' line by more than `tolerance`
/// (in turn, each half alike), and its last, onto `out`.
fn keep(run: &[(f32, f32)], tolerance: f32, out: &mut Vec<(f32, f32)>) {
    let (Some(&a), Some(&b)) = (run.first(), run.last()) else { return };
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = (dx * dx + dy * dy).sqrt();
    let off = |p: (f32, f32)| {
        if length > 0.0 { ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / length } else { dist2(p, a).sqrt() }
    };
    let worst = (1..run.len().saturating_sub(1)).max_by(|&i, &j| off(run[i]).total_cmp(&off(run[j])));
    match worst {
        Some(i) if off(run[i]) > tolerance => {
            keep(&run[..=i], tolerance, out);
            out.pop();
            keep(&run[i..], tolerance, out);
        }
        _ => {
            out.push(a);
            out.push(b);
        }
    }
}

/// Whether `p` is inside the region `outlines` make, even-odd.
pub(crate) fn inside(outlines: &[Vec<(f32, f32)>], p: (f32, f32)) -> bool {
    let mut odd = false;
    for outline in outlines {
        let n = outline.len();
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                odd = !odd;
            }
        }
    }
    odd
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `side` square picture with a square ring drawn `thick` wide from `from` to `to`, a gap
    /// `gap` wide in its top.
    fn ring(side: usize, from: usize, to: usize, thick: usize, gap: usize) -> Vec<u8> {
        let mut out = vec![0u8; side * side];
        let mid = (from + to) / 2;
        for y in from..to {
            for x in from..to {
                let edge = x < from + thick || x >= to - thick || y < from + thick || y >= to - thick;
                let in_gap = y < from + thick && x >= mid - gap / 2 && x < mid - gap / 2 + gap;
                if edge && !in_gap {
                    out[y * side + x] = 255;
                }
            }
        }
        out
    }

    fn count(mask: &[bool]) -> usize {
        mask.iter().filter(|b| **b).count()
    }

    #[test]
    fn a_fill_inside_a_ring_stays_in_it_and_closes_a_narrow_gap_but_not_a_wide_one() {
        let side = 64;
        let closed = ring(side, 10, 50, 3, 0);
        let drawn = Drawn { width: side, height: side, coverage: &closed };
        let inner = drawn.fill((30, 30), 0, 0.0).unwrap();
        // the inside, and a pixel under the ring's inner edge
        assert!(inner[30 * side + 30] && inner[13 * side + 30] && inner[12 * side + 30]);
        assert!(!inner[11 * side + 30] && !inner[5 * side + 5]);
        assert_eq!(count(&inner), 36 * 36);
        // on the ring: none (a tap there is for the stroke)
        assert!(drawn.fill((10, 30), 0, 0.0).is_none());
        // a gap of 4: through it without closing, kept in with gaps up to 6 closed
        let open = ring(side, 10, 50, 3, 4);
        let drawn = Drawn { width: side, height: side, coverage: &open };
        assert!(drawn.fill((30, 30), 0, 0.0).unwrap()[2 * side + 2], "runs out");
        let kept = drawn.fill((30, 30), 0, 6.0).unwrap();
        assert!(!kept[2 * side + 2] && kept[30 * side + 30]);
        assert!(kept[13 * side + 20], "grown back to the line");
        assert!(kept[11 * side + 30] && !kept[8 * side + 30], "halfway into the gap");
        // a gap wider than that: out
        let wide = ring(side, 10, 50, 3, 12);
        let drawn = Drawn { width: side, height: side, coverage: &wide };
        assert!(drawn.fill((30, 30), 0, 6.0).unwrap()[2 * side + 2]);
        // a threshold lets faint drawing through
        let mut faint = closed.clone();
        faint.iter_mut().for_each(|c| *c /= 4);
        let drawn = Drawn { width: side, height: side, coverage: &faint };
        assert!(drawn.fill((30, 30), 100, 0.0).unwrap()[2 * side + 2]);
    }

    #[test]
    fn a_region_s_outlines_fill_back_to_it_holes_and_all() {
        let side = 32;
        // a ring: an outline round it and one round its hole
        let r = ring(side, 4, 28, 5, 0);
        let mask: Vec<bool> = r.iter().map(|c| *c > 0).collect();
        let loops = outlines(side, side, &mask, 0.0);
        assert_eq!(loops.len(), 2);
        for y in 0..side {
            for x in 0..side {
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                assert_eq!(inside(&loops, p), mask[y * side + x], "at {x}, {y}");
            }
        }
        // a staircase smoothed into its slope: far fewer points, still round the same pixels
        let mut disc = vec![false; side * side];
        for y in 0..side {
            for x in 0..side {
                disc[y * side + x] = (x as f32 - 15.5).hypot(y as f32 - 15.5) < 12.0;
            }
        }
        let exact = outlines(side, side, &disc, 0.0);
        let smooth = outlines(side, side, &disc, 0.75);
        assert!(smooth[0].len() * 2 < exact[0].len(), "{} and {}", smooth[0].len(), exact[0].len());
        assert!(inside(&smooth, (15.5, 15.5)) && !inside(&smooth, (1.0, 1.0)));
        // two squares touching at a corner: two outlines
        let mut two = vec![false; 16];
        two[0] = true;
        two[5] = true;
        assert_eq!(outlines(4, 4, &two, 0.0).len(), 2);
    }
}
