//! Points, affine maps and pixel rectangles.

use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn distance(self, other: Vec2) -> f32 {
        (self - other).length()
    }

    pub fn lerp(self, other: Vec2, t: f32) -> Vec2 {
        self + (other - self) * t
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    pub fn dot(self, other: Vec2) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// The z of the 3D cross product: positive when `other` turns anticlockwise from this.
    pub fn cross(self, other: Vec2) -> f32 {
        self.x * other.y - self.y * other.x
    }

    /// This at length 1, or `fallback` when it has no length.
    pub fn normalized_or(self, fallback: Vec2) -> Vec2 {
        let len = self.length();
        if len > 1e-9 && len.is_finite() { self * (1.0 / len) } else { fallback }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

/// A 2-D affine map `p ↦ (a·x + c·y + e, b·x + d·y + f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(t: Vec2) -> Affine {
        Affine { e: t.x, f: t.y, ..Affine::IDENTITY }
    }

    pub fn scale(s: f32) -> Affine {
        Affine { a: s, d: s, ..Affine::IDENTITY }
    }

    /// Rotation by `radians`, clockwise on a y-down screen.
    pub fn rotate(radians: f32) -> Affine {
        let (s, c) = radians.sin_cos();
        Affine { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    /// `self`, then `next`.
    pub fn then(self, next: Affine) -> Affine {
        let n = next;
        Affine {
            a: n.a * self.a + n.c * self.b,
            b: n.b * self.a + n.d * self.b,
            c: n.a * self.c + n.c * self.d,
            d: n.b * self.c + n.d * self.d,
            e: n.a * self.e + n.c * self.f + n.e,
            f: n.b * self.e + n.d * self.f + n.f,
        }
    }

    pub fn apply(self, p: Vec2) -> Vec2 {
        Vec2::new(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }

    pub fn determinant(self) -> f32 {
        self.a * self.d - self.b * self.c
    }

    pub fn inverse(self) -> Option<Affine> {
        let det = self.determinant();
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let (a, b, c, d) = (self.d * inv, -self.b * inv, -self.c * inv, self.a * inv);
        Some(Affine { a, b, c, d, e: -(a * self.e + c * self.f), f: -(b * self.e + d * self.f) })
    }

    /// The uniform scale of a similarity map (the zoom).
    pub fn scale_factor(self) -> f32 {
        self.determinant().abs().sqrt()
    }

    /// The rotation of a similarity map, radians.
    pub fn rotation(self) -> f32 {
        self.b.atan2(self.a)
    }

    pub fn is_finite(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f].iter().all(|v| v.is_finite())
    }
}

/// A projective map of the plane (3 × 3, row-major, acting on `(x, y, 1)`): it carries any four
/// corners to any other four, so it covers moving, scaling, rotating and distorting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Homography {
    m: [f64; 9],
}

impl Homography {
    pub const IDENTITY: Homography = Homography { m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] };

    /// The map from the unit square's corners (0, 0), (1, 0), (1, 1), (0, 1) to `q` in that order.
    pub fn square_to_quad(q: [Vec2; 4]) -> Option<Homography> {
        let [p0, p1, p2, p3] = q.map(|p| (f64::from(p.x), f64::from(p.y)));
        let sx = p0.0 - p1.0 + p2.0 - p3.0;
        let sy = p0.1 - p1.1 + p2.1 - p3.1;
        let (g, h) = if sx.abs() < 1e-12 && sy.abs() < 1e-12 {
            (0.0, 0.0)
        } else {
            let (dx1, dx2, dy1, dy2) = (p1.0 - p2.0, p3.0 - p2.0, p1.1 - p2.1, p3.1 - p2.1);
            let den = dx1 * dy2 - dx2 * dy1;
            if den.abs() < 1e-12 {
                return None;
            }
            ((sx * dy2 - dx2 * sy) / den, (dx1 * sy - sx * dy1) / den)
        };
        let m = [
            p1.0 - p0.0 + g * p1.0,
            p3.0 - p0.0 + h * p3.0,
            p0.0,
            p1.1 - p0.1 + g * p1.1,
            p3.1 - p0.1 + h * p3.1,
            p0.1,
            g,
            h,
            1.0,
        ];
        let out = Homography { m };
        (out.is_finite() && out.determinant().abs() > 1e-12).then_some(out)
    }

    /// The map carrying corners `from` to corners `to` (each in the order of [`Self::square_to_quad`]).
    pub fn quad_to_quad(from: [Vec2; 4], to: [Vec2; 4]) -> Option<Homography> {
        let a = Homography::square_to_quad(from)?.inverse()?;
        Some(a.then(Homography::square_to_quad(to)?))
    }

    /// `self`, then `next`.
    pub fn then(self, next: Homography) -> Homography {
        let [a0, a1, a2, a3, a4, a5, a6, a7, a8] = self.m;
        let [b0, b1, b2, b3, b4, b5, b6, b7, b8] = next.m;
        Homography {
            m: [
                b0 * a0 + b1 * a3 + b2 * a6,
                b0 * a1 + b1 * a4 + b2 * a7,
                b0 * a2 + b1 * a5 + b2 * a8,
                b3 * a0 + b4 * a3 + b5 * a6,
                b3 * a1 + b4 * a4 + b5 * a7,
                b3 * a2 + b4 * a5 + b5 * a8,
                b6 * a0 + b7 * a3 + b8 * a6,
                b6 * a1 + b7 * a4 + b8 * a7,
                b6 * a2 + b7 * a5 + b8 * a8,
            ],
        }
    }

    pub fn determinant(self) -> f64 {
        let [a, b, c, d, e, f, g, h, i] = self.m;
        a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    }

    pub fn inverse(self) -> Option<Homography> {
        let det = self.determinant();
        if !det.is_finite() || det.abs() < 1e-18 {
            return None;
        }
        let [a, b, c, d, e, f, g, h, i] = self.m;
        let k = 1.0 / det;
        Some(Homography {
            m: [
                (e * i - f * h) * k,
                (c * h - b * i) * k,
                (b * f - c * e) * k,
                (f * g - d * i) * k,
                (a * i - c * g) * k,
                (c * d - a * f) * k,
                (d * h - e * g) * k,
                (b * g - a * h) * k,
                (a * e - b * d) * k,
            ],
        })
    }

    /// Where `p` goes; `None` where the map sends it to infinity.
    pub fn apply(self, p: Vec2) -> Option<Vec2> {
        let [a, b, c, d, e, f, g, h, i] = self.m;
        let (x, y) = (f64::from(p.x), f64::from(p.y));
        let w = g * x + h * y + i;
        if w.abs() < 1e-12 {
            return None;
        }
        Some(Vec2::new(((a * x + b * y + c) / w) as f32, ((d * x + e * y + f) / w) as f32))
    }

    /// The matrix, row-major, for a shader.
    pub fn to_f32(self) -> [f32; 9] {
        self.m.map(|v| v as f32)
    }

    pub fn is_finite(self) -> bool {
        self.m.iter().all(|v| v.is_finite())
    }
}

/// An integer pixel rectangle `[x0, x1) × [y0, y1)`, empty when it has no area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    pub const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> IRect {
        IRect { x0, y0, x1, y1 }
    }

    pub fn from_size(width: u32, height: u32) -> IRect {
        IRect::new(0, 0, i32::try_from(width).unwrap_or(i32::MAX), i32::try_from(height).unwrap_or(i32::MAX))
    }

    /// The pixels a disc can touch, with one pixel to spare for anti-aliasing.
    pub fn around(center: Vec2, radius: f32) -> IRect {
        if !center.is_finite() || !radius.is_finite() {
            return IRect::EMPTY;
        }
        let r = radius.max(0.0) + 1.0;
        IRect::new(
            (center.x - r).floor() as i32,
            (center.y - r).floor() as i32,
            (center.x + r).ceil() as i32,
            (center.y + r).ceil() as i32,
        )
    }

    pub fn is_empty(self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    pub fn width(self) -> u32 {
        u32::try_from(self.x1.saturating_sub(self.x0)).unwrap_or(0)
    }

    pub fn height(self) -> u32 {
        u32::try_from(self.y1.saturating_sub(self.y0)).unwrap_or(0)
    }

    pub fn union(self, o: IRect) -> IRect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return self;
        }
        IRect::new(self.x0.min(o.x0), self.y0.min(o.y0), self.x1.max(o.x1), self.y1.max(o.y1))
    }

    pub fn intersect(self, o: IRect) -> IRect {
        let r = IRect::new(self.x0.max(o.x0), self.y0.max(o.y0), self.x1.min(o.x1), self.y1.min(o.y1));
        if r.is_empty() { IRect::EMPTY } else { r }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn affine_inverse_round_trips() {
        let m = Affine::scale(2.5).then(Affine::rotate(0.7)).then(Affine::translate(Vec2::new(30.0, -12.0)));
        let inv = m.inverse().unwrap();
        let p = Vec2::new(17.0, 41.0);
        assert!(close(inv.apply(m.apply(p)), p));
        assert!((m.scale_factor() - 2.5).abs() < 1e-4);
        assert!((m.rotation() - 0.7).abs() < 1e-4);
    }

    #[test]
    fn degenerate_affine_has_no_inverse() {
        assert!(Affine::scale(0.0).inverse().is_none());
    }

    #[test]
    fn homography_carries_corners_to_corners() {
        let from = [Vec2::new(10.0, 20.0), Vec2::new(110.0, 20.0), Vec2::new(110.0, 70.0), Vec2::new(10.0, 70.0)];
        let to = [Vec2::new(0.0, 0.0), Vec2::new(200.0, 30.0), Vec2::new(180.0, 160.0), Vec2::new(-20.0, 120.0)];
        let h = Homography::quad_to_quad(from, to).unwrap();
        for (f, t) in from.iter().zip(&to) {
            assert!(close(h.apply(*f).unwrap(), *t), "{f:?} -> {t:?}");
        }
        let back = h.inverse().unwrap();
        let p = Vec2::new(47.0, 33.0);
        assert!(close(back.apply(h.apply(p).unwrap()).unwrap(), p));
    }

    #[test]
    fn homography_of_a_moved_rectangle_is_affine() {
        let from = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 2.0), Vec2::new(0.0, 2.0)];
        let to = from.map(|p| p * 2.0 + Vec2::new(5.0, -3.0));
        let h = Homography::quad_to_quad(from, to).unwrap();
        assert!(close(h.apply(Vec2::new(1.0, 1.0)).unwrap(), Vec2::new(7.0, -1.0)));
        assert!(Homography::quad_to_quad(from, [Vec2::new(0.0, 0.0); 4]).is_none(), "a quad with no area");
    }

    #[test]
    fn rects() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 20, 8);
        assert_eq!(a.union(b), IRect::new(0, 0, 20, 10));
        assert_eq!(a.intersect(b), IRect::new(5, 5, 10, 8));
        assert!(a.intersect(IRect::new(11, 11, 12, 12)).is_empty());
        assert_eq!(IRect::EMPTY.union(b), b);
        assert_eq!(IRect::around(Vec2::new(5.5, 5.5), 2.0), IRect::new(2, 2, 9, 9));
        assert!(IRect::around(Vec2::new(f32::NAN, 0.0), 2.0).is_empty());
    }
}
