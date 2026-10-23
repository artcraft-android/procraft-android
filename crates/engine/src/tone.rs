//! Tone tables for adjustments: a curve through points (a natural cubic spline) and a gradient
//! between colour stops, each baked into 256 entries.

/// A curve through `points` (x and y in 0..1, any order): a natural cubic spline through them,
/// held flat beyond the first and last, baked into 256 entries 0..255. With fewer than two points,
/// the straight line from (0, 0) to (1, 1).
pub fn curve_table(points: &[(f32, f32)]) -> [u8; 256] {
    let mut pts: Vec<(f32, f32)> = points.iter().copied().filter(|(x, y)| x.is_finite() && y.is_finite()).collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);
    if pts.len() < 2 {
        pts = vec![(0.0, 0.0), (1.0, 1.0)];
    }
    let second = natural_second_derivatives(&pts);
    std::array::from_fn(|i| {
        let x = i as f32 / 255.0;
        let y = spline_at(&pts, &second, x);
        (y.clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

/// The second derivatives at the points of the natural cubic spline through them (zero at both
/// ends), by the tridiagonal system's sweep.
fn natural_second_derivatives(pts: &[(f32, f32)]) -> Vec<f32> {
    let n = pts.len();
    let mut m = vec![0.0f32; n];
    if n < 3 {
        return m;
    }
    let mut up = vec![0.0f32; n];
    for i in 1..n - 1 {
        let (Some(&(x0, y0)), Some(&(x1, y1)), Some(&(x2, y2))) = (pts.get(i - 1), pts.get(i), pts.get(i + 1)) else {
            continue;
        };
        let sig = (x1 - x0) / (x2 - x0);
        let p = sig * m.get(i - 1).copied().unwrap_or(0.0) + 2.0;
        if let Some(v) = m.get_mut(i) {
            *v = (sig - 1.0) / p;
        }
        let slope = (y2 - y1) / (x2 - x1) - (y1 - y0) / (x1 - x0);
        let prev = up.get(i - 1).copied().unwrap_or(0.0);
        if let Some(u) = up.get_mut(i) {
            *u = (6.0 * slope / (x2 - x0) - sig * prev) / p;
        }
    }
    // back substitution, the last second derivative zero
    let mut second = vec![0.0f32; n];
    for i in (1..n - 1).rev() {
        let next = second.get(i + 1).copied().unwrap_or(0.0);
        let value = m.get(i).copied().unwrap_or(0.0) * next + up.get(i).copied().unwrap_or(0.0);
        if let Some(s) = second.get_mut(i) {
            *s = value;
        }
    }
    second
}

/// The spline through `pts` (with second derivatives `second`) at `x`, flat beyond its ends.
fn spline_at(pts: &[(f32, f32)], second: &[f32], x: f32) -> f32 {
    let (Some(&(first_x, first_y)), Some(&(last_x, last_y))) = (pts.first(), pts.last()) else { return x };
    if x <= first_x {
        return first_y;
    }
    if x >= last_x {
        return last_y;
    }
    let hi = pts.iter().position(|p| p.0 >= x).unwrap_or(pts.len() - 1).max(1);
    let lo = hi - 1;
    let (Some(&(x0, y0)), Some(&(x1, y1))) = (pts.get(lo), pts.get(hi)) else { return x };
    let (m0, m1) = (second.get(lo).copied().unwrap_or(0.0), second.get(hi).copied().unwrap_or(0.0));
    let h = x1 - x0;
    let a = (x1 - x) / h;
    let b = (x - x0) / h;
    a * y0 + b * y1 + ((a * a * a - a) * m0 + (b * b * b - b) * m1) * h * h / 6.0
}

/// A gradient between colour `stops` (position 0..1 and RGB 0..1, any order), baked into 256 RGB
/// entries, held at the end colours beyond the first and last stop. Black to white without stops.
pub fn gradient_table(stops: &[(f32, [f32; 3])]) -> [[u8; 3]; 256] {
    let mut s: Vec<(f32, [f32; 3])> = stops.iter().copied().filter(|(p, _)| p.is_finite()).collect();
    s.sort_by(|a, b| a.0.total_cmp(&b.0));
    if s.is_empty() {
        s = vec![(0.0, [0.0; 3]), (1.0, [1.0; 3])];
    }
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    std::array::from_fn(|i| {
        let x = i as f32 / 255.0;
        let next = s.iter().position(|st| st.0 >= x);
        let colour = match next {
            Some(0) | None => s.get(if next.is_none() { s.len() - 1 } else { 0 }).map_or([0.0; 3], |st| st.1),
            Some(k) => {
                let (Some(a), Some(b)) = (s.get(k - 1), s.get(k)) else { return [0; 3] };
                let t = if b.0 > a.0 { (x - a.0) / (b.0 - a.0) } else { 0.0 };
                [0, 1, 2]
                    .map(|c| a.1.get(c).copied().unwrap_or(0.0) * (1.0 - t) + b.1.get(c).copied().unwrap_or(0.0) * t)
            }
        };
        colour.map(byte)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_straight_curve_leaves_tones_alone_and_points_bend_it() {
        let straight = curve_table(&[]);
        assert_eq!(straight[0], 0);
        assert_eq!(straight[128], 128);
        assert_eq!(straight[255], 255);
        // through (0.5, 0.75): the midtones lift, the ends stay
        let lifted = curve_table(&[(0.0, 0.0), (0.5, 0.75), (1.0, 1.0)]);
        assert_eq!(lifted[0], 0);
        assert!((i32::from(lifted[128]) - 191).abs() <= 2, "{}", lifted[128]);
        assert_eq!(lifted[255], 255);
        assert!(lifted[64] > 64 && lifted[192] > 192);
        // held flat beyond its ends
        let short = curve_table(&[(0.25, 0.1), (0.75, 0.9)]);
        assert_eq!(short[0], short[60]);
        assert_eq!(short[255], 230);
    }

    #[test]
    fn a_gradient_runs_between_its_stops() {
        let g = gradient_table(&[(1.0, [1.0, 1.0, 0.0]), (0.0, [0.0, 0.0, 1.0])]);
        assert_eq!(g[0], [0, 0, 255]);
        assert_eq!(g[255], [255, 255, 0]);
        assert_eq!(g[128], [128, 128, 127]);
        assert_eq!(gradient_table(&[])[255], [255, 255, 255]);
    }
}
