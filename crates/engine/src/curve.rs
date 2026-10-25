//! Response curves (pressure, tilt...): a smooth curve through control points on the unit square.

use std::sync::{Arc, OnceLock};

use crate::geom::Vec2;

/// How many places a curve is sampled at.
const STEPS: usize = 1024;

/// A curve `[0, 1] → [0, 1]` through control points, bent as brush files' curves are: a natural
/// cubic spline through the points (it may swing past them), held level beyond the first and the
/// last, sampled at [`STEPS`] places and read in a straight line between them.
#[derive(Clone)]
pub struct Curve {
    points: Vec<Vec2>,
    table: Arc<[f32]>,
}

impl PartialEq for Curve {
    fn eq(&self, other: &Curve) -> bool {
        self.points == other.points
    }
}

impl std::fmt::Debug for Curve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Curve").field(&self.points).finish()
    }
}

impl Default for Curve {
    fn default() -> Curve {
        Curve::linear()
    }
}

impl Curve {
    pub fn linear() -> Curve {
        static TABLE: OnceLock<Arc<[f32]>> = OnceLock::new();
        let points = vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)];
        let table = TABLE.get_or_init(|| sampled(&points)).clone();
        Curve { points, table }
    }

    /// Points are clamped to the unit square and sorted by x; of points sharing an x the first
    /// wins. Fewer than two usable points give the identity curve.
    pub fn new(points: &[Vec2]) -> Curve {
        let mut pts: Vec<Vec2> = points
            .iter()
            .filter(|p| p.is_finite())
            .map(|p| Vec2::new(p.x.clamp(0.0, 1.0), p.y.clamp(0.0, 1.0)))
            .collect();
        pts.sort_by(|a, b| a.x.total_cmp(&b.x));
        pts.dedup_by(|later, earlier| (later.x - earlier.x).abs() < 1e-6);
        if pts.len() < 2 {
            return Curve::linear();
        }
        let table = sampled(&pts);
        Curve { points: pts, table }
    }

    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    pub fn eval(&self, x: f32) -> f32 {
        let x = if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
        let f = x * (STEPS - 1) as f32;
        let i = f.floor() as usize;
        let at = |k: usize| self.table.get(k.min(STEPS - 1)).copied().unwrap_or(x);
        let (a, b) = (at(i), at(i + 1));
        a + (b - a) * (f - i as f32)
    }
}

/// The curve through `pts` (sorted, at least two) sampled at [`STEPS`] places: the points scaled
/// up by the steps, the spline read at each whole step, and that taken over one step less (so a
/// curve ending below the top comes out a little higher), held to 0..1.
fn sampled(pts: &[Vec2]) -> Arc<[f32]> {
    let n = STEPS as f32;
    let knots: Vec<Vec2> = pts.iter().map(|p| *p * n).collect();
    let bends = second_derivatives(&knots);
    (0..STEPS).map(|i| (spline(&knots, &bends, i as f32) / (n - 1.0)).clamp(0.0, 1.0)).collect::<Vec<f32>>().into()
}

/// A natural cubic spline's second derivatives at `knots` (none at either end).
fn second_derivatives(knots: &[Vec2]) -> Vec<f32> {
    let n = knots.len();
    let mut m = vec![0.0f32; n];
    if n < 3 {
        return m;
    }
    // the tridiagonal system for the inner knots, solved forward then back
    let mut diag = vec![0.0f32; n];
    let mut rhs = vec![0.0f32; n];
    for i in 1..n - 1 {
        let (Some(p0), Some(p1), Some(p2)) = (knots.get(i - 1), knots.get(i), knots.get(i + 1)) else { continue };
        let (h0, h1) = (p1.x - p0.x, p2.x - p1.x);
        let mut d = 2.0 * (h0 + h1);
        let mut r = 6.0 * ((p2.y - p1.y) / h1 - (p1.y - p0.y) / h0);
        if i > 1 {
            let w = h0 / diag.get(i - 1).copied().unwrap_or(1.0);
            d -= w * h0;
            r -= w * rhs.get(i - 1).copied().unwrap_or(0.0);
        }
        if let (Some(dv), Some(rv)) = (diag.get_mut(i), rhs.get_mut(i)) {
            *dv = d;
            *rv = r;
        }
    }
    for i in (1..n - 1).rev() {
        let (Some(p1), Some(p2)) = (knots.get(i), knots.get(i + 1)) else { continue };
        let h1 = p2.x - p1.x;
        let next = if i + 1 < n - 1 { m.get(i + 1).copied().unwrap_or(0.0) } else { 0.0 };
        let d = diag.get(i).copied().unwrap_or(1.0);
        if let Some(v) = m.get_mut(i) {
            *v = (rhs.get(i).copied().unwrap_or(0.0) - h1 * next) / d;
        }
    }
    m
}

/// The spline through `knots` with second derivatives `bends` at `x`, level beyond its ends.
fn spline(knots: &[Vec2], bends: &[f32], x: f32) -> f32 {
    let (Some(first), Some(last)) = (knots.first(), knots.last()) else { return x };
    if x <= first.x {
        return first.y;
    }
    if x >= last.x {
        return last.y;
    }
    let i = knots.partition_point(|p| p.x <= x).saturating_sub(1);
    let (Some(p0), Some(p1)) = (knots.get(i), knots.get(i + 1)) else { return x };
    let (m0, m1) = (bends.get(i).copied().unwrap_or(0.0), bends.get(i + 1).copied().unwrap_or(0.0));
    let h = p1.x - p0.x;
    if h <= 0.0 {
        return p0.y;
    }
    let a = (p1.x - x) / h;
    let b = 1.0 - a;
    a * p0.y + b * p1.y + ((a * a * a - a) * m0 + (b * b * b - b) * m1) * h * h / 6.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_is_identity() {
        let c = Curve::linear();
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            assert!((c.eval(x) - x).abs() < 1e-5);
        }
        assert_eq!(Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]), c);
    }

    #[test]
    fn passes_near_its_points_as_a_natural_spline() {
        let c = Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.3, 0.6), Vec2::new(0.7, 0.7), Vec2::new(1.0, 1.0)]);
        assert!((c.eval(0.3) - 0.6).abs() < 2e-3, "{}", c.eval(0.3));
        assert!((c.eval(0.7) - 0.7).abs() < 2e-3, "{}", c.eval(0.7));
        // a natural spline: straight at its ends, so its middle swings above the points' line
        let mid = c.eval(0.5);
        assert!(mid > 0.65 + 1e-3, "{mid}");
        // the spline read by hand at x = 0.5 (second derivatives of the system above)
        let knots: Vec<Vec2> = c.points().iter().map(|p| *p * 1024.0).collect();
        let by_hand = spline(&knots, &second_derivatives(&knots), 0.5 * 1023.0) / 1023.0;
        assert!((mid - by_hand).abs() < 1e-4);
    }

    #[test]
    fn a_curve_ending_below_the_top_is_held_level_and_a_little_raised() {
        let c = Curve::new(&[Vec2::new(0.0, 0.2), Vec2::new(0.6, 0.8)]);
        // past its last point it stays at that point's height, over one step less
        assert!((c.eval(0.9) - 0.8 * 1024.0 / 1023.0).abs() < 1e-5);
        assert!((c.eval(0.0) - 0.2 * 1024.0 / 1023.0).abs() < 1e-5);
    }

    #[test]
    fn bad_input_falls_back() {
        let c = Curve::new(&[Vec2::new(f32::NAN, 0.0)]);
        assert!((c.eval(0.25) - 0.25).abs() < 1e-5);
        assert_eq!(Curve::linear().eval(f32::NAN), 0.0);
        assert_eq!(Curve::linear().eval(4.0), 1.0);
    }
}
