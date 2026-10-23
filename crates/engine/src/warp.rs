//! A warp: a bicubic Bézier patch of 4 × 4 control points carrying the unit square (the content's
//! source rectangle) onto the canvas.

use crate::geom::Vec2;

/// Control points row by row, row 0 at the top, four to a row.
pub type Mesh = [Vec2; 16];

/// The cubic Bernstein weights at `t`.
fn bernstein(t: f32) -> [f32; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t]
}

/// The weights' derivatives at `t`.
fn bernstein_slope(t: f32) -> [f32; 4] {
    let s = 1.0 - t;
    [-3.0 * s * s, 3.0 * s * s - 6.0 * s * t, 6.0 * s * t - 3.0 * t * t, 3.0 * t * t]
}

/// The weight of each control point at (`u`, `v`), row by row.
fn weights(bu: [f32; 4], bv: [f32; 4]) -> impl Iterator<Item = f32> {
    bv.into_iter().flat_map(move |wv| bu.map(|wu| wu * wv))
}

fn sum(m: &Mesh, w: impl Iterator<Item = f32>) -> Vec2 {
    m.iter().zip(w).fold(Vec2::ZERO, |acc, (p, w)| acc + *p * w)
}

/// The mesh laid evenly over the quad `q` (top left, top right, bottom right, bottom left).
pub fn over_quad(q: [Vec2; 4]) -> Mesh {
    let [a, b, c, d] = q;
    std::array::from_fn(|k| {
        let (u, v) = ((k % 4) as f32 / 3.0, (k / 4) as f32 / 3.0);
        a.lerp(b, u).lerp(d.lerp(c, u), v)
    })
}

/// The patch's point at (`u`, `v`).
pub fn point(m: &Mesh, u: f32, v: f32) -> Vec2 {
    sum(m, weights(bernstein(u), bernstein(v)))
}

/// Where on the patch canvas point `p` lies, as (u, v) in the unit square; none when it is off the
/// patch.
pub fn locate(m: &Mesh, p: Vec2) -> Option<(f32, f32)> {
    // the nearest of a coarse grid, then Newton steps
    let mut best = (0.5, 0.5, f32::MAX);
    for a in 0..=8 {
        for b in 0..=8 {
            let (u, v) = (a as f32 / 8.0, b as f32 / 8.0);
            let d = point(m, u, v).distance(p);
            if d < best.2 {
                best = (u, v, d);
            }
        }
    }
    let (mut u, mut v, _) = best;
    for _ in 0..16 {
        let du = sum(m, weights(bernstein_slope(u), bernstein(v)));
        let dv = sum(m, weights(bernstein(u), bernstein_slope(v)));
        let r = point(m, u, v) - p;
        let det = du.x * dv.y - du.y * dv.x;
        if !det.is_finite() || det.abs() < 1e-9 {
            break;
        }
        u = (u - (r.x * dv.y - r.y * dv.x) / det).clamp(-0.5, 1.5);
        v = (v - (du.x * r.y - du.y * r.x) / det).clamp(-0.5, 1.5);
    }
    let inside = |t: f32| (-1e-3..=1.0 + 1e-3).contains(&t);
    (point(m, u, v).distance(p) < 0.5 && inside(u) && inside(v)).then(|| (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
}

/// Which third of the patch `t` falls in (0, 1 or 2) and how far through it.
fn third(t: f32) -> (usize, f32) {
    let x = (t * 3.0).clamp(0.0, 3.0);
    let i = (x.floor() as usize).min(2);
    (i, x - i as f32)
}

/// The mesh after its point at (`u`, `v`) is dragged by `delta`. The 2 × 2 control points around
/// the point share the move, more to the nearer ones, scaled so that the point follows exactly.
pub fn drag_surface(m: &Mesh, u: f32, v: f32, delta: Vec2) -> Mesh {
    let (row, tv) = third(v);
    let (col, tu) = third(u);
    // how the move is shared along each axis: nearly even, leaning to the nearer pair
    let lean = |t: f32| 0.075 + 0.85 * t;
    let share = |k: usize| -> f32 {
        let (i, j) = (k / 4, k % 4);
        let along_v = if i == row {
            1.0 - lean(tv)
        } else if i == row + 1 {
            lean(tv)
        } else {
            0.0
        };
        let along_u = if j == col {
            1.0 - lean(tu)
        } else if j == col + 1 {
            lean(tu)
        } else {
            0.0
        };
        along_v * along_u
    };
    let moved: f32 = weights(bernstein(u), bernstein(v)).enumerate().map(|(k, w)| w * share(k)).sum();
    if !moved.is_finite() || moved <= 1e-6 {
        return *m;
    }
    std::array::from_fn(|k| m.get(k).copied().unwrap_or_default() + delta * (share(k) / moved))
}

/// The patch as triangles over a `steps` × `steps` grid, each corner as (canvas x, y, source x, y):
/// the source is the rectangle `source` (top left, top right, bottom right, bottom left) laid over
/// the unit square.
pub fn triangles(m: &Mesh, source: [Vec2; 4], steps: usize) -> Vec<[f32; 4]> {
    let steps = steps.max(1);
    let [s0, s1, _, s3] = source;
    let corner = |a: usize, b: usize| {
        let (u, v) = (a as f32 / steps as f32, b as f32 / steps as f32);
        let p = point(m, u, v);
        let s = s0 + (s1 - s0) * u + (s3 - s0) * v;
        [p.x, p.y, s.x, s.y]
    };
    let mut out = Vec::with_capacity(steps * steps * 6);
    for b in 0..steps {
        for a in 0..steps {
            let (tl, tr, br, bl) = (corner(a, b), corner(a + 1, b), corner(a + 1, b + 1), corner(a, b + 1));
            out.extend([tl, tr, br, tl, br, bl]);
        }
    }
    out
}

/// The patch's lines at the thirds: four across (v = 0, ⅓, ⅔, 1) then four down (u likewise),
/// each sampled at `samples` + 1 points.
pub fn grid_lines(m: &Mesh, samples: usize) -> Vec<Vec<Vec2>> {
    let samples = samples.max(1);
    let line = |across: bool, k: usize| -> Vec<Vec2> {
        let fixed = k as f32 / 3.0;
        (0..=samples)
            .map(|s| {
                let t = s as f32 / samples as f32;
                if across { point(m, t, fixed) } else { point(m, fixed, t) }
            })
            .collect()
    };
    (0..4).map(|k| line(true, k)).chain((0..4).map(|k| line(false, k))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-3
    }

    fn square() -> Mesh {
        over_quad([Vec2::new(100.0, 100.0), Vec2::new(400.0, 100.0), Vec2::new(400.0, 400.0), Vec2::new(100.0, 400.0)])
    }

    #[test]
    fn an_even_mesh_is_the_quad_itself() {
        let m = square();
        assert!(close(point(&m, 0.0, 0.0), Vec2::new(100.0, 100.0)));
        assert!(close(point(&m, 0.5, 0.25), Vec2::new(250.0, 175.0)));
        assert!(close(point(&m, 1.0, 1.0), Vec2::new(400.0, 400.0)));
        let (u, v) = locate(&m, Vec2::new(325.0, 160.0)).unwrap();
        assert!((u - 0.75).abs() < 1e-3 && (v - 0.2).abs() < 1e-3);
        assert_eq!(locate(&m, Vec2::new(50.0, 160.0)), None);
    }

    #[test]
    fn a_surface_drag_moves_the_point_under_the_finger_with_it() {
        let m = square();
        for (u, v) in [(0.5, 0.5), (0.5, 0.1), (0.9, 0.95), (0.2, 0.7)] {
            let delta = Vec2::new(40.0, -25.0);
            let moved = drag_surface(&m, u, v, delta);
            assert!(close(point(&moved, u, v), point(&m, u, v) + delta), "{u} {v}");
        }
        // in the middle, the four inner points move alike and the edges stay
        let moved = drag_surface(&m, 0.5, 0.5, Vec2::new(65.0, 0.0));
        let inner = moved[5] - m[5];
        assert!((inner.x - 65.0 / (4.0 * 0.375f32.powi(2))).abs() < 0.5, "{inner:?}");
        assert!(close(moved[6] - m[6], inner) && close(moved[9] - m[9], inner) && close(moved[10] - m[10], inner));
        assert!(close(moved[1], m[1]) && close(moved[4], m[4]) && close(moved[0], m[0]));
        // near the top edge, the edge's middle points take the larger share
        let moved = drag_surface(&m, 0.498, 0.1022, Vec2::new(65.0, 0.0));
        let ratio = (moved[1].x - m[1].x) / (moved[5].x - m[5].x);
        assert!((ratio - 1.975).abs() < 0.05, "{ratio}");
        assert!(close(moved[9], m[9]) && close(moved[0], m[0]));
    }

    #[test]
    fn triangles_cover_the_patch_with_their_sources() {
        let m = square();
        let source = [Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(30.0, 30.0), Vec2::new(0.0, 30.0)];
        let t = triangles(&m, source, 4);
        assert_eq!(t.len(), 4 * 4 * 6);
        assert_eq!(t.first(), Some(&[100.0, 100.0, 0.0, 0.0]));
        let lines = grid_lines(&m, 6);
        assert_eq!(lines.len(), 8);
        assert!(lines.iter().all(|l| l.len() == 7));
    }
}
