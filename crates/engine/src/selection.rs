//! Selections: how much of each canvas pixel is selected (0 none, 255 all), made from shapes or by
//! colour, and combined, inverted and feathered.

use crate::geom::Vec2;

/// A selection over a `width` × `height` canvas, one byte per pixel, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// How a new shape joins the selection there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    Add,
    Remove,
}

/// Rows of sub-samples per pixel when filling a polygon.
const POLYGON_ROWS: usize = 4;

impl Mask {
    /// Nothing selected.
    pub fn empty(width: u32, height: u32) -> Mask {
        Mask { width, height, data: vec![0; width as usize * height as usize] }
    }

    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|&v| v == 0)
    }

    /// The rectangle between corners `a` and `b` (canvas pixels), with partly covered edge pixels.
    pub fn rect(width: u32, height: u32, a: Vec2, b: Vec2) -> Mask {
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        let mut m = Mask::empty(width, height);
        let w = width as usize;
        let cover = |lo: f32, hi: f32, p: usize| (hi.min(p as f32 + 1.0) - lo.max(p as f32)).clamp(0.0, 1.0);
        let cols: Vec<f32> = (0..w).map(|x| cover(x0, x1, x)).collect();
        for (y, row) in m.data.chunks_exact_mut(w.max(1)).enumerate() {
            let cy = cover(y0, y1, y);
            if cy <= 0.0 {
                continue;
            }
            for (v, cx) in row.iter_mut().zip(&cols) {
                *v = to_byte(cx * cy);
            }
        }
        m
    }

    /// The ellipse in the box between corners `a` and `b`, with soft edge pixels.
    pub fn ellipse(width: u32, height: u32, a: Vec2, b: Vec2) -> Mask {
        let c = a.lerp(b, 0.5);
        let (rx, ry) = (((b.x - a.x) / 2.0).abs(), ((b.y - a.y) / 2.0).abs());
        let mut m = Mask::empty(width, height);
        if rx < 1e-3 || ry < 1e-3 {
            return m;
        }
        let w = width as usize;
        let (y_lo, y_hi) =
            ((c.y - ry - 1.0).max(0.0) as usize, ((c.y + ry + 1.0).max(0.0) as usize).min(height as usize));
        let (x_lo, x_hi) = ((c.x - rx - 1.0).max(0.0) as usize, ((c.x + rx + 1.0).max(0.0) as usize).min(w));
        for y in y_lo..y_hi {
            let Some(row) = m.data.get_mut(y * w..(y + 1) * w) else { continue };
            for x in x_lo..x_hi {
                let p = Vec2::new((x as f32 + 0.5 - c.x) / rx, (y as f32 + 0.5 - c.y) / ry);
                let len = p.length();
                // the distance from the edge in pixels: the implicit function over its gradient
                let gradient = Vec2::new(p.x / rx, p.y / ry).length().max(1e-6);
                let distance = (len - 1.0) * len / gradient;
                if let Some(v) = row.get_mut(x) {
                    *v = to_byte(0.5 - distance);
                }
            }
        }
        m
    }

    /// The polygon through `points` (canvas pixels, closed back to the first), filled by the
    /// non-zero rule, with partly covered edge pixels.
    pub fn polygon(width: u32, height: u32, points: &[Vec2]) -> Mask {
        let mut m = Mask::empty(width, height);
        let pts: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
        if pts.len() < 3 {
            return m;
        }
        let w = width as usize;
        let edges: Vec<(Vec2, Vec2)> = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| (*a, *b)).collect();
        let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        let (y_lo, y_hi) = (lo.max(0.0) as usize, (hi.max(0.0).ceil() as usize).min(height as usize));
        let mut coverage = vec![0.0f32; w + 1];
        let mut crossings: Vec<(f32, i32)> = Vec::new();
        for y in y_lo..y_hi {
            coverage.iter_mut().for_each(|c| *c = 0.0);
            for sub in 0..POLYGON_ROWS {
                let sy = y as f32 + (sub as f32 + 0.5) / POLYGON_ROWS as f32;
                crossings.clear();
                for &(a, b) in &edges {
                    if (a.y <= sy) != (b.y <= sy) {
                        let x = a.x + (sy - a.y) / (b.y - a.y) * (b.x - a.x);
                        crossings.push((x, if b.y > a.y { 1 } else { -1 }));
                    }
                }
                crossings.sort_by(|p, q| p.0.total_cmp(&q.0));
                let mut winding = 0;
                for pair in crossings.windows(2) {
                    let [(x0, d), (x1, _)] = pair else { continue };
                    winding += d;
                    if winding != 0 {
                        add_span(&mut coverage, *x0, *x1, 1.0 / POLYGON_ROWS as f32);
                    }
                }
            }
            if let Some(row) = m.data.get_mut(y * w..(y + 1) * w) {
                for (v, c) in row.iter_mut().zip(&coverage) {
                    *v = to_byte(*c);
                }
            }
        }
        m
    }

    /// Joins `other` to this selection.
    pub fn combine(&mut self, other: &Mask, how: Combine) {
        for (a, &b) in self.data.iter_mut().zip(&other.data) {
            *a = match how {
                Combine::Add => (*a).max(b),
                Combine::Remove => (*a).min(255 - b),
            };
        }
    }

    pub fn invert(&mut self) {
        self.data.iter_mut().for_each(|v| *v = 255 - *v);
    }

    /// Softens the edges: three box blurs `radius` pixels wide each way (close to a Gaussian).
    pub fn feather(&mut self, radius: f32) {
        let r = radius.max(0.0).round() as usize;
        if r == 0 {
            return;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let mut values: Vec<f32> = self.data.iter().map(|&v| f32::from(v)).collect();
        let mut line = Vec::new();
        for _ in 0..3 {
            for y in 0..h {
                blur_line(&mut values, y * w, 1, w, r, &mut line);
            }
            for x in 0..w {
                blur_line(&mut values, x, w, h, r, &mut line);
            }
        }
        for (v, f) in self.data.iter_mut().zip(&values) {
            *v = f.round().clamp(0.0, 255.0) as u8;
        }
    }

    /// The pixels joined to `seed` whose colour is within `threshold` (0..1) of the seed's, in
    /// premultiplied RGBA8 `pixels` the mask's size: an automatic selection.
    pub fn by_colour(width: u32, height: u32, pixels: &[u8], seed: (u32, u32), threshold: f32) -> Mask {
        let mut m = Mask::empty(width, height);
        let (w, h) = (width as usize, height as usize);
        let at = |x: usize, y: usize| -> [f32; 4] {
            let i = (y * w + x) * 4;
            match pixels.get(i..i + 4) {
                Some(&[r, g, b, a]) => [r, g, b, a].map(|v| f32::from(v) / 255.0),
                _ => [0.0; 4],
            }
        };
        let (sx, sy) = (seed.0 as usize, seed.1 as usize);
        if sx >= w || sy >= h {
            return m;
        }
        let target = at(sx, sy);
        let limit = threshold.clamp(0.0, 1.0) * 2.0;
        let near = |c: [f32; 4]| c.iter().zip(&target).map(|(a, b)| (a - b).abs()).sum::<f32>() <= limit;
        let mut stack = vec![(sx, sy)];
        while let Some((x, y)) = stack.pop() {
            let i = y * w + x;
            if m.data.get(i).copied() != Some(0) || !near(at(x, y)) {
                continue;
            }
            if let Some(v) = m.data.get_mut(i) {
                *v = 255;
            }
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < w {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < h {
                stack.push((x, y + 1));
            }
        }
        m
    }
}

impl Mask {
    /// How much is selected at canvas point `p`, between pixel centres.
    fn sample(&self, p: Vec2) -> f32 {
        let (x, y) = (p.x - 0.5, p.y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |xi: f32, yi: f32| -> f32 {
            if xi < 0.0 || yi < 0.0 || xi >= self.width as f32 || yi >= self.height as f32 {
                return 0.0;
            }
            let i = yi as usize * self.width as usize + xi as usize;
            f32::from(self.data.get(i).copied().unwrap_or(0)) / 255.0
        };
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
        let bottom = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    /// The selection moved as content is: each pixel takes what `to_source` (a pixel centre to
    /// where it came from) finds there.
    pub fn moved(&self, to_source: impl Fn(Vec2) -> Option<Vec2>) -> Mask {
        let mut out = Mask::empty(self.width, self.height);
        let w = self.width as usize;
        for (y, row) in out.data.chunks_exact_mut(w.max(1)).enumerate() {
            for (x, v) in row.iter_mut().enumerate() {
                if let Some(s) = to_source(Vec2::new(x as f32 + 0.5, y as f32 + 0.5)) {
                    *v = to_byte(self.sample(s));
                }
            }
        }
        out
    }

    /// The selection moved through a mesh of triangles, three corners each of (where it goes,
    /// where it came from) in pixels, as a warp moves content.
    pub fn warped(&self, triangles: &[[f32; 4]]) -> Mask {
        let mut out = Mask::empty(self.width, self.height);
        let (w, h) = (self.width as usize, self.height as usize);
        for &[a, b, c] in triangles.as_chunks::<3>().0 {
            let (pa, pb, pc) = (Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]), Vec2::new(c[0], c[1]));
            let area = (pb.x - pa.x) * (pc.y - pa.y) - (pc.x - pa.x) * (pb.y - pa.y);
            if area.abs() < 1e-9 {
                continue;
            }
            let (lo_x, hi_x) = (
                pa.x.min(pb.x).min(pc.x).floor().max(0.0) as usize,
                (pa.x.max(pb.x).max(pc.x).ceil().max(0.0) as usize).min(w),
            );
            let (lo_y, hi_y) = (
                pa.y.min(pb.y).min(pc.y).floor().max(0.0) as usize,
                (pa.y.max(pb.y).max(pc.y).ceil().max(0.0) as usize).min(h),
            );
            for y in lo_y..hi_y {
                for x in lo_x..hi_x {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    // where p falls in the triangle, as weights of its corners
                    let wb = ((p.x - pa.x) * (pc.y - pa.y) - (pc.x - pa.x) * (p.y - pa.y)) / area;
                    let wc = ((pb.x - pa.x) * (p.y - pa.y) - (p.x - pa.x) * (pb.y - pa.y)) / area;
                    let wa = 1.0 - wb - wc;
                    if wa < -1e-4 || wb < -1e-4 || wc < -1e-4 {
                        continue;
                    }
                    let s = Vec2::new(a[2] * wa + b[2] * wb + c[2] * wc, a[3] * wa + b[3] * wb + c[3] * wc);
                    if let Some(v) = out.data.get_mut(y * w + x) {
                        *v = (*v).max(to_byte(self.sample(s)));
                    }
                }
            }
        }
        out
    }
}

/// The Gaussian's sigma (canvas pixels) for a blur `amount` (0..1), as feathering and the Gaussian
/// blur use it: it widens faster than the amount, from a fraction of a pixel to about 680.
pub fn feather_radius(amount: f32) -> f32 {
    let a = amount.clamp(0.0, 1.0);
    if a <= 0.0 {
        return 0.0;
    }
    let taps = ((a.sqrt() * 63.0).floor() + 1.0).min(64.0);
    let scale = (a * 32.0).floor().max(1.0);
    taps / 3.0 * scale
}

fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Adds `amount` × the covered part of each pixel between `x0` and `x1` to `row`.
fn add_span(row: &mut [f32], x0: f32, x1: f32, amount: f32) {
    let n = row.len().saturating_sub(1) as f32;
    let (x0, x1) = (x0.clamp(0.0, n), x1.clamp(0.0, n));
    if x1 <= x0 {
        return;
    }
    let (first, last) = (x0.floor() as usize, x1.floor() as usize);
    if first == last {
        if let Some(c) = row.get_mut(first) {
            *c += (x1 - x0) * amount;
        }
        return;
    }
    if let Some(c) = row.get_mut(first) {
        *c += (first as f32 + 1.0 - x0) * amount;
    }
    for c in row.iter_mut().take(last).skip(first + 1) {
        *c += amount;
    }
    if let Some(c) = row.get_mut(last) {
        *c += (x1 - last as f32) * amount;
    }
}

/// One box blur `r` either side along a line of `len` values `step` apart from `start`, edges
/// held at their last value.
fn blur_line(values: &mut [f32], start: usize, step: usize, len: usize, r: usize, line: &mut Vec<f32>) {
    line.clear();
    line.extend((0..len).map(|i| values.get(start + i * step).copied().unwrap_or(0.0)));
    let get = |i: isize| line.get(i.clamp(0, len as isize - 1) as usize).copied().unwrap_or(0.0);
    let span = (2 * r + 1) as f32;
    let mut sum: f32 = (-(r as isize)..=r as isize).map(get).sum();
    for i in 0..len {
        if let Some(v) = values.get_mut(start + i * step) {
            *v = sum / span;
        }
        sum += get(i as isize + r as isize + 1) - get(i as isize - r as isize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(m: &Mask, x: u32, y: u32) -> u8 {
        m.data[(y * m.width + x) as usize]
    }

    #[test]
    fn a_rectangle_covers_its_pixels_and_part_of_its_edges() {
        let m = Mask::rect(10, 10, Vec2::new(7.5, 8.0), Vec2::new(2.0, 2.0));
        assert_eq!(at(&m, 2, 2), 255);
        assert_eq!(at(&m, 7, 5), 128);
        assert_eq!(at(&m, 8, 5), 0);
        assert_eq!(at(&m, 1, 1), 0);
        assert!(!m.is_empty());
        assert!(Mask::empty(4, 4).is_empty());
    }

    #[test]
    fn an_ellipse_is_round_with_soft_edges() {
        let m = Mask::ellipse(40, 40, Vec2::new(0.5, 0.5), Vec2::new(39.5, 19.5));
        assert_eq!(at(&m, 20, 10), 255);
        assert_eq!(at(&m, 1, 1), 0);
        assert_eq!(at(&m, 20, 25), 0);
        // the pixel the end of the long axis runs through is about half covered
        let edge = at(&m, 39, 9);
        assert!(edge > 40 && edge < 220, "{edge}");
    }

    #[test]
    fn a_polygon_fills_by_the_non_zero_rule() {
        let tri = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)];
        let m = Mask::polygon(12, 12, &tri);
        assert_eq!(at(&m, 1, 1), 255);
        assert_eq!(at(&m, 8, 8), 0);
        // a pixel the diagonal cuts in half
        let half = at(&m, 4, 5);
        assert!((100..=155).contains(&half), "{half}");
        // a square traced twice the same way stays filled
        let sq = [Vec2::new(2.0, 2.0), Vec2::new(8.0, 2.0), Vec2::new(8.0, 8.0), Vec2::new(2.0, 8.0)];
        let twice: Vec<Vec2> = sq.iter().chain(&sq).copied().collect();
        assert_eq!(at(&Mask::polygon(12, 12, &twice), 5, 5), 255);
        assert!(Mask::polygon(12, 12, &tri[..2]).is_empty());
    }

    #[test]
    fn adding_removing_inverting_and_feathering() {
        let mut m = Mask::rect(20, 20, Vec2::new(0.0, 0.0), Vec2::new(10.0, 20.0));
        m.combine(&Mask::rect(20, 20, Vec2::new(10.0, 0.0), Vec2::new(15.0, 20.0)), Combine::Add);
        assert_eq!(at(&m, 12, 5), 255);
        m.combine(&Mask::rect(20, 20, Vec2::new(0.0, 0.0), Vec2::new(20.0, 5.0)), Combine::Remove);
        assert_eq!(at(&m, 2, 2), 0);
        assert_eq!(at(&m, 2, 7), 255);
        m.invert();
        assert_eq!(at(&m, 2, 2), 255);
        assert_eq!(at(&m, 2, 7), 0);
        let mut f = Mask::rect(40, 40, Vec2::new(10.0, 10.0), Vec2::new(30.0, 30.0));
        f.feather(3.0);
        assert_eq!(at(&f, 20, 20), 255);
        let edge = at(&f, 10, 20);
        assert!(edge > 60 && edge < 200, "{edge}");
        assert!(at(&f, 6, 20) < at(&f, 10, 20));
    }

    #[test]
    fn a_selection_moves_with_its_content() {
        let m = Mask::rect(20, 20, Vec2::new(0.0, 0.0), Vec2::new(5.0, 5.0));
        // 10 right: each pixel looks 10 left
        let moved = m.moved(|p| Some(p - Vec2::new(10.0, 0.0)));
        assert_eq!(at(&moved, 12, 2), 255);
        assert_eq!(at(&moved, 2, 2), 0);
        // the same through two triangles
        let c = |x: f32, y: f32| [x + 10.0, y, x, y];
        let mesh = [c(0.0, 0.0), c(20.0, 0.0), c(20.0, 20.0), c(0.0, 0.0), c(20.0, 20.0), c(0.0, 20.0)];
        let warped = m.warped(&mesh);
        assert_eq!(at(&warped, 12, 2), 255);
        assert_eq!(at(&warped, 2, 2), 0);
    }

    #[test]
    fn the_feather_widens_with_its_amount() {
        assert_eq!(feather_radius(0.0), 0.0);
        assert!((feather_radius(0.52) - 245.3).abs() < 0.5, "{}", feather_radius(0.52));
        assert!(feather_radius(0.1) < feather_radius(0.2) && feather_radius(1.0) > 650.0);
    }

    #[test]
    fn colour_selection_floods_what_matches_and_is_joined() {
        // a 6 × 1 strip: red, red, blue, red, red, red
        let red = [255u8, 0, 0, 255];
        let blue = [0u8, 0, 255, 255];
        let px: Vec<u8> = [red, red, blue, red, red, red].concat();
        let m = Mask::by_colour(6, 1, &px, (0, 0), 0.1);
        assert_eq!(m.data, vec![255, 255, 0, 0, 0, 0]);
        let all = Mask::by_colour(6, 1, &px, (0, 0), 1.0);
        assert!(all.data.iter().all(|&v| v == 255));
        assert!(Mask::by_colour(6, 1, &px, (9, 0), 0.5).is_empty());
    }
}
