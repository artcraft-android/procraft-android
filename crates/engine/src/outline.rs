//! A stamp's outline: where a shape image's paint ends, as closed loops, for a brush cursor that
//! shows the brush's own shape rather than a circle.

use std::collections::HashMap;

use crate::geom::Vec2;

/// The finest the outline is traced at (cells across).
const MOST_CELLS: usize = 128;
/// Loops smaller than this share of the largest's area are left out (a grainy shape's specks).
const LEAST_AREA: f32 = 0.05;
/// At most this many loops.
const MOST_LOOPS: usize = 12;

/// An edge of the grid a contour crosses: across (between a node and the one right of it) or
/// down (between a node and the one below), at node (x, y).
type Edge = (bool, usize, usize);

/// The loops along which `pixels` (`side` x `side`, greyscale, white = paint) crosses `level`
/// (0..1), softened first so a grainy shape gives its overall form: in the image's units (0..1
/// across, y down), each a closed ring (its last point joins its first), simplified to within
/// half a cell, the largest first; loops far smaller than the largest are left out.
pub fn shape_outline(pixels: &[u8], side: usize, level: f32) -> Vec<Vec<Vec2>> {
    if side == 0 || pixels.len() < side * side {
        return Vec::new();
    }
    let step = side.div_ceil(MOST_CELLS).max(1);
    let n = side / step;
    // averaged down, with an empty border so every contour closes, then softened twice
    let w = n + 2;
    let mut v = vec![0.0f32; w * w];
    for y in 0..n {
        for x in 0..n {
            let mut sum = 0u32;
            for dy in 0..step {
                for dx in 0..step {
                    sum += u32::from(pixels.get((y * step + dy) * side + x * step + dx).copied().unwrap_or(0));
                }
            }
            if let Some(c) = v.get_mut((y + 1) * w + x + 1) {
                *c = sum as f32 / (step * step) as f32 / 255.0;
            }
        }
    }
    for _ in 0..2 {
        v = soften(&v, w);
    }
    let at = |x: usize, y: usize| v.get(y * w + x).copied().unwrap_or(0.0);
    let inside = |x: usize, y: usize| at(x, y) >= level;
    // where along an edge the value crosses the level, in the image's units
    let point = |(across, x, y): Edge| {
        let (x2, y2) = if across { (x + 1, y) } else { (x, y + 1) };
        let (a, b) = (at(x, y), at(x2, y2));
        let t = if (b - a).abs() > 1e-6 { ((level - a) / (b - a)).clamp(0.0, 1.0) } else { 0.5 };
        let px = x as f32 + (x2 as f32 - x as f32) * t;
        let py = y as f32 + (y2 as f32 - y as f32) * t;
        // node 1 is the first pixel's middle
        Vec2::new((px - 0.5) / n as f32, (py - 0.5) / n as f32)
    };
    // each cell's crossing edges joined in pairs (marching squares); every crossed edge ends up
    // joined to the one before and the one after it along its contour
    let mut joined: HashMap<Edge, Vec<Edge>> = HashMap::new();
    let mut join = |a: Edge, b: Edge| {
        joined.entry(a).or_default().push(b);
        joined.entry(b).or_default().push(a);
    };
    for y in 0..w - 1 {
        for x in 0..w - 1 {
            let case = u8::from(inside(x, y))
                | u8::from(inside(x + 1, y)) << 1
                | u8::from(inside(x + 1, y + 1)) << 2
                | u8::from(inside(x, y + 1)) << 3;
            let (top, right, bottom, left) = ((true, x, y), (false, x + 1, y), (true, x, y + 1), (false, x, y));
            let middle = (at(x, y) + at(x + 1, y) + at(x + 1, y + 1) + at(x, y + 1)) / 4.0 >= level;
            match case {
                1 | 14 => join(left, top),
                2 | 13 => join(top, right),
                3 | 12 => join(left, right),
                4 | 11 => join(right, bottom),
                6 | 9 => join(top, bottom),
                7 | 8 => join(left, bottom),
                // the two corners across from each other: joined through the middle or apart
                5 if middle => {
                    join(top, right);
                    join(bottom, left);
                }
                5 => {
                    join(left, top);
                    join(right, bottom);
                }
                10 if middle => {
                    join(left, top);
                    join(right, bottom);
                }
                10 => {
                    join(top, right);
                    join(bottom, left);
                }
                _ => {}
            }
        }
    }
    // walked round, each loop once
    let mut loops: Vec<Vec<Vec2>> = Vec::new();
    let mut seen: std::collections::HashSet<Edge> = std::collections::HashSet::new();
    let mut starts: Vec<Edge> = joined.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        if !seen.insert(start) {
            continue;
        }
        let mut ring = vec![point(start)];
        let (mut before, mut here) = (start, start);
        while let Some(next) = joined.get(&here).and_then(|n| n.iter().copied().find(|e| *e != before || n.len() == 1))
        {
            if next == start || !seen.insert(next) {
                break;
            }
            ring.push(point(next));
            (before, here) = (here, next);
        }
        if ring.len() >= 3 {
            loops.push(simplified(&ring, 0.5 / n as f32));
        }
    }
    loops.retain(|l| l.len() >= 3);
    loops.sort_by(|a, b| area(b).total_cmp(&area(a)));
    let largest = loops.first().map_or(0.0, |l| area(l));
    loops.retain(|l| area(l) >= largest * LEAST_AREA);
    loops.truncate(MOST_LOOPS);
    loops
}

/// A 3 x 3 box blur of the `w` x `w` grid `v`, its edge kept empty.
fn soften(v: &[f32], w: usize) -> Vec<f32> {
    let mut out = vec![0.0; v.len()];
    for y in 1..w - 1 {
        for x in 1..w - 1 {
            let mut sum = 0.0;
            for dy in 0..3 {
                for dx in 0..3 {
                    sum += v.get((y + dy - 1) * w + x + dx - 1).copied().unwrap_or(0.0);
                }
            }
            if let Some(c) = out.get_mut(y * w + x) {
                *c = sum / 9.0;
            }
        }
    }
    out
}

/// A closed ring's area (either way round).
fn area(ring: &[Vec2]) -> f32 {
    let n = ring.len();
    (0..n)
        .map(|i| match (ring.get(i), ring.get((i + 1) % n)) {
            (Some(a), Some(b)) => a.x * b.y - b.x * a.y,
            _ => 0.0,
        })
        .sum::<f32>()
        .abs()
        / 2.0
}

/// A closed ring kept within `tolerance` of itself with fewer points (Ramer-Douglas-Peucker on
/// its two halves, split at the point farthest from its first).
fn simplified(ring: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    fn keep(pts: &[Vec2], tolerance: f32, out: &mut Vec<Vec2>) {
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { return };
        let ab = b - a;
        let len2 = ab.dot(ab);
        let far = pts.iter().enumerate().fold((0, 0.0f32), |best, (i, &p)| {
            let d = if len2 <= 1e-12 {
                p.distance(a)
            } else {
                p.distance(a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0))
            };
            if d > best.1 { (i, d) } else { best }
        });
        if far.1 > tolerance && far.0 > 0 && far.0 + 1 < pts.len() {
            keep(pts.get(..=far.0).unwrap_or(&[]), tolerance, out);
            out.pop();
            keep(pts.get(far.0..).unwrap_or(&[]), tolerance, out);
        } else {
            out.push(a);
            out.push(b);
        }
    }
    let Some(&first) = ring.first() else { return Vec::new() };
    let split = ring.iter().enumerate().fold((0, 0.0f32), |best, (i, p)| {
        let d = p.distance(first);
        if d > best.1 { (i, d) } else { best }
    });
    let mut closed: Vec<Vec2> = ring.to_vec();
    closed.push(first);
    let mut out = Vec::new();
    keep(closed.get(..=split.0).unwrap_or(&[]), tolerance, &mut out);
    out.pop();
    keep(closed.get(split.0..).unwrap_or(&[]), tolerance, &mut out);
    // the ring's first point is its last again: once is enough
    out.pop();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(side: usize, paint: impl Fn(f32, f32) -> bool) -> Vec<u8> {
        (0..side * side)
            .map(|i| {
                let (x, y) = ((i % side) as f32 + 0.5, (i / side) as f32 + 0.5);
                if paint(x / side as f32, y / side as f32) { 255 } else { 0 }
            })
            .collect()
    }

    #[test]
    fn a_disc_outlines_as_one_round_loop() {
        let disc = image(256, |x, y| (x - 0.5).hypot(y - 0.5) <= 0.4);
        let loops = shape_outline(&disc, 256, 0.5);
        assert_eq!(loops.len(), 1);
        let ring = &loops[0];
        assert!(ring.len() >= 12, "{} points", ring.len());
        for p in ring {
            let r = (p.x - 0.5).hypot(p.y - 0.5);
            assert!((r - 0.4).abs() < 0.02, "{p:?} at {r}");
        }
    }

    #[test]
    fn a_square_keeps_its_corners_and_specks_are_left_out() {
        // a square a quarter in from each side, and a few single specks far from it
        let square = image(256, |x, y| {
            ((0.25..=0.75).contains(&x) && (0.25..=0.75).contains(&y))
                || ((x - 0.06).abs() < 0.005 && (y - 0.06).abs() < 0.005)
        });
        let loops = shape_outline(&square, 256, 0.5);
        assert_eq!(loops.len(), 1, "{loops:?}");
        let ring = &loops[0];
        // simplified to (about) its four corners, near them
        assert!(ring.len() <= 12, "{} points", ring.len());
        for corner in [Vec2::new(0.25, 0.25), Vec2::new(0.75, 0.25), Vec2::new(0.75, 0.75), Vec2::new(0.25, 0.75)] {
            assert!(ring.iter().any(|p| p.distance(corner) < 0.03), "no point near {corner:?}: {ring:?}");
        }
    }

    #[test]
    fn a_ring_shape_has_its_hole_and_nothing_gives_nothing() {
        let ring = image(256, |x, y| {
            let r = (x - 0.5).hypot(y - 0.5);
            (0.25..=0.45).contains(&r)
        });
        assert_eq!(shape_outline(&ring, 256, 0.5).len(), 2);
        assert!(shape_outline(&vec![0; 64 * 64], 64, 0.5).is_empty());
        assert!(shape_outline(&[], 0, 0.5).is_empty());
        assert!(shape_outline(&[255; 10], 64, 0.5).is_empty());
    }
}
