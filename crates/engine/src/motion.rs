//! Timing curves for things the engine moves on its own (the view settling into place).

/// A damped spring released from 0 towards 1, stiff enough to come within a thousandth of its end
/// in `seconds`; `damping` 1 is critical, less bounces.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    seconds: f32,
    damping: f32,
    omega: f32,
}

impl Spring {
    pub fn new(seconds: f32, damping: f32) -> Spring {
        let seconds = if seconds.is_finite() { seconds.max(1e-3) } else { 0.3 };
        let damping = if damping.is_finite() { damping.clamp(0.05, 1.0) } else { 1.0 };
        // the slowest natural frequency at which the motion has settled by then
        let (mut lo, mut hi) = (0.01_f64, 2000.0_f64);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if envelope(mid, f64::from(damping), f64::from(seconds)) > 1e-3 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Spring { seconds, damping, omega: hi as f32 }
    }

    pub fn seconds(&self) -> f32 {
        self.seconds
    }

    /// Where it is `t` seconds after release (exactly 1 from its time on).
    pub fn at(&self, t: f32) -> f32 {
        if !t.is_finite() || t >= self.seconds {
            return 1.0;
        }
        if t <= 0.0 {
            return 0.0;
        }
        let (t, z, w) = (f64::from(t), f64::from(self.damping), f64::from(self.omega));
        let off = if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            (-z * w * t).exp() * (-(wd * t).cos() - z * w / wd * (wd * t).sin())
        } else {
            (-1.0 - w * t) * (-w * t).exp()
        };
        (1.0 + off) as f32
    }
}

/// A cubic-bezier timing curve from (0, 0) to (1, 1) through the control points (x1, y1) and
/// (x2, y2), run over `seconds`.
#[derive(Clone, Copy, Debug)]
pub struct Bezier {
    pub seconds: f32,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Bezier {
    /// Slowing to a stop: (0, 0, 0.58, 1).
    pub fn ease_out(seconds: f32) -> Bezier {
        Bezier { seconds, x1: 0.0, y1: 0.0, x2: 0.58, y2: 1.0 }
    }

    /// How far along it is `t` seconds in (exactly 1 from its time on).
    pub fn at(&self, t: f32) -> f32 {
        if !t.is_finite() || t >= self.seconds {
            return 1.0;
        }
        if t <= 0.0 {
            return 0.0;
        }
        let x = t / self.seconds.max(1e-6);
        let curve =
            |u: f32, a: f32, b: f32| 3.0 * (1.0 - u) * (1.0 - u) * u * a + 3.0 * (1.0 - u) * u * u * b + u * u * u;
        // the curve's own parameter where it is x across, by halving
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..30 {
            let mid = (lo + hi) / 2.0;
            if curve(mid, self.x1, self.x2) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        curve((lo + hi) / 2.0, self.y1, self.y2)
    }
}

/// How far from its end the motion can still be at time `t`, at frequency `w`.
fn envelope(w: f64, z: f64, t: f64) -> f64 {
    if z < 1.0 {
        let wd = w * (1.0 - z * z).sqrt();
        let c = z * w / wd;
        (1.0 + c * c).sqrt() * (-z * w * t).exp()
    } else {
        (1.0 + w * t) * (-w * t).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spring_starts_still_and_settles_in_its_time() {
        for damping in [0.6, 0.8, 1.0] {
            let s = Spring::new(0.5, damping);
            assert_eq!(s.at(0.0), 0.0);
            assert!(s.at(0.01) < 0.05);
            assert!((s.at(0.499) - 1.0).abs() < 2e-3, "{damping}: {}", s.at(0.499));
            assert_eq!(s.at(0.5), 1.0);
        }
        // a softer one goes past its end on the way
        let s = Spring::new(0.5, 0.6);
        assert!((0..50).map(|i| s.at(i as f32 / 100.0)).fold(0.0_f32, f32::max) > 1.0);
    }

    #[test]
    fn ease_out_starts_fast_and_slows_to_its_end() {
        let e = Bezier::ease_out(0.3);
        assert_eq!(e.at(0.0), 0.0);
        assert_eq!(e.at(0.3), 1.0);
        // ahead of a straight line all the way, and never back
        let mut last = 0.0;
        for i in 1..30 {
            let t = i as f32 / 100.0;
            let v = e.at(t);
            assert!(v > t / 0.3 && v >= last, "{t}: {v}");
            last = v;
        }
    }
}
