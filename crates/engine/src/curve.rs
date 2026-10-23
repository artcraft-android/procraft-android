//! Response curves (pressure, tilt...): a smooth curve through control points on the unit square.

use crate::geom::Vec2;

/// A curve `[0, 1] → [0, 1]` through control points: monotone cubic Hermite interpolation, so it
/// passes through every point and never overshoots between monotone points.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    points: Vec<Vec2>,
    tangents: Vec<f32>,
}

impl Default for Curve {
    fn default() -> Curve {
        Curve::linear()
    }
}

impl Curve {
    pub fn linear() -> Curve {
        Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)])
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
            pts = vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)];
        }
        let tangents = monotone_tangents(&pts);
        Curve { points: pts, tangents }
    }

    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    pub fn eval(&self, x: f32) -> f32 {
        let x = if x.is_finite() { x } else { 0.0 };
        let (Some(first), Some(last)) = (self.points.first(), self.points.last()) else { return x };
        if x <= first.x {
            return first.y;
        }
        if x >= last.x {
            return last.y;
        }
        // the segment [i, i + 1] holding x
        let i = self.points.partition_point(|p| p.x <= x).saturating_sub(1);
        let (Some(p0), Some(p1), Some(m0), Some(m1)) =
            (self.points.get(i), self.points.get(i + 1), self.tangents.get(i), self.tangents.get(i + 1))
        else {
            return x;
        };
        let h = p1.x - p0.x;
        if h <= 0.0 {
            return p0.y;
        }
        let t = (x - p0.x) / h;
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p0.y
            + (t3 - 2.0 * t2 + t) * h * m0
            + (-2.0 * t3 + 3.0 * t2) * p1.y
            + (t3 - t2) * h * m1;
        y.clamp(0.0, 1.0)
    }
}

/// Fritsch–Carlson tangents for points sorted by strictly increasing x.
fn monotone_tangents(pts: &[Vec2]) -> Vec<f32> {
    let secants: Vec<f32> = pts
        .windows(2)
        .map(|w| match w {
            [p0, p1] => (p1.y - p0.y) / (p1.x - p0.x),
            _ => 0.0,
        })
        .collect();
    let n = pts.len();
    let mut m = vec![0.0f32; n];
    for (i, slot) in m.iter_mut().enumerate() {
        let before = i.checked_sub(1).and_then(|j| secants.get(j)).copied();
        let after = secants.get(i).copied();
        *slot = match (before, after) {
            (Some(a), Some(b)) if a * b > 0.0 => (a + b) / 2.0,
            (Some(_), Some(_)) => 0.0,
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => 0.0,
        };
    }
    for (i, &d) in secants.iter().enumerate() {
        if d == 0.0 {
            if let Some(v) = m.get_mut(i) {
                *v = 0.0;
            }
            if let Some(v) = m.get_mut(i + 1) {
                *v = 0.0;
            }
            continue;
        }
        let (Some(&mi), Some(&mj)) = (m.get(i), m.get(i + 1)) else { continue };
        let (a, b) = (mi / d, mj / d);
        let s = a * a + b * b;
        if s > 9.0 {
            let tau = 3.0 / s.sqrt();
            if let Some(v) = m.get_mut(i) {
                *v = tau * a * d;
            }
            if let Some(v) = m.get_mut(i + 1) {
                *v = tau * b * d;
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_is_identity() {
        let c = Curve::linear();
        for i in 0..=10 {
            let x = i as f32 / 10.0;
            assert!((c.eval(x) - x).abs() < 1e-5);
        }
    }

    #[test]
    fn passes_through_points_and_stays_monotone() {
        let c = Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.3, 0.6), Vec2::new(0.7, 0.7), Vec2::new(1.0, 1.0)]);
        assert!((c.eval(0.3) - 0.6).abs() < 1e-5);
        assert!((c.eval(0.7) - 0.7).abs() < 1e-5);
        let mut prev = 0.0;
        for i in 0..=100 {
            let y = c.eval(i as f32 / 100.0);
            assert!(y + 1e-6 >= prev, "not monotone at {i}");
            prev = y;
        }
    }

    #[test]
    fn bad_input_falls_back() {
        let c = Curve::new(&[Vec2::new(f32::NAN, 0.0)]);
        assert!((c.eval(0.25) - 0.25).abs() < 1e-5);
        assert_eq!(Curve::linear().eval(f32::NAN), 0.0);
        assert_eq!(Curve::linear().eval(4.0), 1.0);
    }
}
