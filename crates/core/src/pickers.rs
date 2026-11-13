//! The colour panel's pickers. The disc: saturation and brightness on a disc (the square mapped
//! onto it) inside the hue ring. Harmony: a hue and saturation wheel in Display P3, taken as it is
//! by a canvas in P3 and turned into sRGB (clipped) for one in sRGB; both use the sRGB transfer
//! curve.

/// Hue, saturation and brightness (0..1) as RGB values (0..1).
pub(crate) fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

/// RGB values (0..1) as hue, saturation and brightness (0..1).
pub(crate) fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h / 6.0, if max > 0.0 { d / max } else { 0.0 }, max]
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_encoded(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

pub(crate) fn byte(c: f32) -> u32 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u32
}

fn of_hex(v: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((v >> shift) & 0xff) as f32 / 255.0)
}

/// P3 hue (0..1), saturation and value as a canvas colour (0xRRGGBB): as it is in P3 (`p3`),
/// else in sRGB, clipped.
#[uniffi::export]
pub fn p3_hsv_to_rgb(h: f32, s: f32, v: f32, p3: bool) -> u32 {
    if p3 {
        let [r, g, b] = hsv_to_rgb([h, s, v]);
        return byte(r) << 16 | byte(g) << 8 | byte(b);
    }
    let [r, g, b] = hsv_to_rgb([h, s, v]).map(to_linear);
    let out = |x: f32| byte(to_encoded(x.clamp(0.0, 1.0)));
    out(1.2249 * r - 0.2247 * g) << 16 | out(-0.0420 * r + 1.0419 * g) << 8 | out(-0.0197 * r - 0.0786 * g + 1.0979 * b)
}

/// A canvas colour (0xRRGGBB), in P3 (`p3`) or sRGB, as P3 hue (0..1), saturation and value.
pub fn p3_hsv_of(rgb: u32, p3: bool) -> [f32; 3] {
    if p3 {
        return rgb_to_hsv(of_hex(rgb));
    }
    let [r, g, b] = of_hex(rgb).map(to_linear);
    let enc = |x: f32| to_encoded(x.clamp(0.0, 1.0));
    rgb_to_hsv([enc(0.8225 * r + 0.1774 * g), enc(0.0332 * r + 0.9669 * g), enc(0.0171 * r + 0.0724 * g + 0.9108 * b)])
}

/// [`p3_hsv_of`] as three values: hue, saturation and value.
#[uniffi::export]
pub fn rgb_to_p3_hsv(rgb: u32, p3: bool) -> Vec<f32> {
    p3_hsv_of(rgb, p3).to_vec()
}

/// Harmony's wheel at value `v`, `side` pixels square, for a canvas in P3 (`p3`) or sRGB: hue
/// anticlockwise from the right, saturation outwards, a pixel's soft edge; premultiplied RGBA.
#[uniffi::export]
pub fn harmony_wheel(v: f32, side: u32, p3: bool) -> Vec<u8> {
    let side = side.min(4096) as usize;
    // each pixel's hue, saturation and edge, kept for the size
    let places = geometry(&WHEEL_PLACES, side, |u, w, half| {
        let d = u.hypot(w);
        let edge = ((1.0 - d) * half + 0.5).clamp(0.0, 1.0);
        [((-w).atan2(u) / std::f32::consts::TAU + 1.0) % 1.0, d.min(1.0), edge]
    });
    picture_rows(side, |y, row| {
        for (px, &[h, s, edge]) in row.iter_mut().zip(places.get(y * side..(y + 1) * side).unwrap_or_default()) {
            if edge <= 0.0 {
                continue;
            }
            let rgb = p3_hsv_to_rgb(h, s, v, p3);
            let premultiplied = |shift: u32| (((rgb >> shift) & 0xFF) as f32 * edge).round() as u8;
            *px = [premultiplied(16), premultiplied(8), premultiplied(0), (edge * 255.0).round() as u8];
        }
    })
}

/// A picker picture's pixels' places (three numbers each, from `place` of the pixel's centre on
/// -1..1 across and down, and half the side), the last size asked kept: a drag remakes the
/// picture at one size, only its colour changing.
type Places = std::sync::Mutex<Option<(usize, std::sync::Arc<Vec<[f32; 3]>>)>>;

static WHEEL_PLACES: Places = std::sync::Mutex::new(None);
static DISC_PLACES: Places = std::sync::Mutex::new(None);

fn geometry(
    kept: &Places,
    side: usize,
    place: impl Fn(f32, f32, f32) -> [f32; 3] + Sync,
) -> std::sync::Arc<Vec<[f32; 3]>> {
    let mut last = kept.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, places)) = last.as_ref().filter(|(at, _)| *at == side) {
        return std::sync::Arc::clone(places);
    }
    let half = side as f32 / 2.0;
    let rows: Vec<usize> = (0..side).collect();
    let made = procraft_engine::par::side_by_side(&rows, |&y| {
        let w = (y as f32 + 0.5) / half - 1.0;
        (0..side).map(|x| place((x as f32 + 0.5) / half - 1.0, w, half)).collect::<Vec<_>>()
    });
    let places: Vec<[f32; 3]> = made.into_iter().flat_map(|row| row.unwrap_or_else(|| vec![[0.0; 3]; side])).collect();
    let places = std::sync::Arc::new(places);
    *last = Some((side, std::sync::Arc::clone(&places)));
    places
}

/// A `side` pixels square picture (RGBA) made a row at a time (`row` given its number and its
/// pixels, clear), the rows side by side: the pickers remake theirs as a slider is dragged.
pub(crate) fn picture_rows(side: usize, row: impl Fn(usize, &mut [[u8; 4]]) + Sync) -> Vec<u8> {
    let rows: Vec<usize> = (0..side).collect();
    let made = procraft_engine::par::side_by_side(&rows, |&y| {
        let mut line = vec![[0u8; 4]; side];
        row(y, &mut line);
        line
    });
    let mut out = Vec::with_capacity(side * side * 4);
    for line in made {
        match line {
            Some(line) => out.extend_from_slice(line.as_flattened()),
            None => out.resize(out.len() + side * 4, 0),
        }
    }
    out
}

/// A point of the square (saturation across, brightness up; each -1..1, y down) on the disc, by
/// the elliptical grid map.
pub fn square_to_disc(x: f32, y: f32) -> [f32; 2] {
    [x * (1.0 - y * y / 2.0).sqrt(), y * (1.0 - x * x / 2.0).sqrt()]
}

/// A point of the disc back on the square ([`square_to_disc`]'s inverse).
pub fn disc_to_square(u: f32, v: f32) -> [f32; 2] {
    let r2 = 2.0 * 2f32.sqrt();
    let half = |a: f32, b: f32, s: f32| {
        0.5 * (2.0 + a * a - b * b + r2 * s).max(0.0).sqrt() - 0.5 * (2.0 + a * a - b * b - r2 * s).max(0.0).sqrt()
    };
    [half(u, v, u).clamp(-1.0, 1.0), half(v, u, v).clamp(-1.0, 1.0)]
}

/// [`square_to_disc`] as two values.
#[uniffi::export]
pub fn square_to_disc_point(x: f32, y: f32) -> Vec<f32> {
    square_to_disc(x, y).to_vec()
}

/// [`disc_to_square`] as two values.
#[uniffi::export]
pub fn disc_to_square_point(u: f32, v: f32) -> Vec<f32> {
    disc_to_square(u, v).to_vec()
}

/// The disc picker's saturation and brightness for `hue`, `side` pixels square, with a soft edge;
/// premultiplied RGBA.
#[uniffi::export]
pub fn disc_picture(hue: f32, side: u32) -> Vec<u8> {
    let side = side.min(4096) as usize;
    // each pixel's saturation, brightness and edge, kept for the size
    let places = geometry(&DISC_PLACES, side, |u, v, half| {
        let edge = ((1.0 - u.hypot(v)) * half + 0.5).clamp(0.0, 1.0);
        if edge <= 0.0 {
            return [0.0; 3];
        }
        let [x, y] = disc_to_square(u, v);
        [(x + 1.0) / 2.0, (1.0 - y) / 2.0, edge]
    });
    picture_rows(side, |y, row| {
        for (px, &[s, v, edge]) in row.iter_mut().zip(places.get(y * side..(y + 1) * side).unwrap_or_default()) {
            if edge <= 0.0 {
                continue;
            }
            let rgb = hsv_to_rgb([hue, s, v]);
            let premultiplied = |c: f32| (byte(c) as f32 * edge).round() as u8;
            *px = [premultiplied(rgb[0]), premultiplied(rgb[1]), premultiplied(rgb[2]), (edge * 255.0).round() as u8];
        }
    })
}

/// The triangle picker's height across its square: an equilateral triangle of side 1.
const TRIANGLE_HEIGHT: f32 = 0.866_025_4;

/// The triangle picker's corners in its square (0..1, y down): the pure hue at the right, white
/// at the top left, black at the bottom left, the triangle centred across the square.
fn triangle_corners() -> [[f32; 2]; 3] {
    let left = (1.0 - TRIANGLE_HEIGHT) / 2.0;
    [[left + TRIANGLE_HEIGHT, 0.5], [left, 0.0], [left, 1.0]]
}

/// How much of the pure hue, white and black make a point of the triangle picker's square (they
/// add up to 1; one below 0 is outside).
fn triangle_parts(x: f32, y: f32) -> [f32; 3] {
    let [p, w, k] = triangle_corners();
    let det = (w[1] - k[1]) * (p[0] - k[0]) + (k[0] - w[0]) * (p[1] - k[1]);
    let a = ((w[1] - k[1]) * (x - k[0]) + (k[0] - w[0]) * (y - k[1])) / det;
    let b = ((k[1] - p[1]) * (x - k[0]) + (p[0] - k[0]) * (y - k[1])) / det;
    [a, b, 1.0 - a - b]
}

/// Where saturation `s` and brightness `v` sit in the triangle picker's square (0..1, y down).
#[uniffi::export]
pub fn triangle_point(s: f32, v: f32) -> Vec<f32> {
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let [p, w, k] = triangle_corners();
    let (a, b, c) = (v * s, v * (1.0 - s), 1.0 - v);
    vec![a * p[0] + b * w[0] + c * k[0], a * p[1] + b * w[1] + c * k[1]]
}

/// The saturation and brightness at a point of the triangle picker's square (0..1, y down); one
/// outside the triangle is taken at its edge.
#[uniffi::export]
pub fn triangle_sv(x: f32, y: f32) -> Vec<f32> {
    let parts = triangle_parts(x, y).map(|part| part.max(0.0));
    let sum: f32 = parts.iter().sum();
    let [a, b, _] = if sum > 0.0 { parts.map(|part| part / sum) } else { [0.0, 0.0, 1.0] };
    let v = (a + b).clamp(0.0, 1.0);
    vec![if v > 1e-6 { (a / v).clamp(0.0, 1.0) } else { 0.0 }, v]
}

/// The triangle picker's picture for `hue`, `side` pixels square: saturation and brightness
/// between the pure hue, white and black, with a soft edge; premultiplied RGBA.
#[uniffi::export]
pub fn triangle_picture(hue: f32, side: u32) -> Vec<u8> {
    let side = side.min(4096) as usize;
    let n = side as f32;
    picture_rows(side, |y, row| {
        for (x, px) in row.iter_mut().enumerate() {
            let parts = triangle_parts((x as f32 + 0.5) / n, (y as f32 + 0.5) / n);
            // how far in from the nearest side, in pixels
            let inside = parts.iter().fold(f32::MAX, |least, &part| least.min(part)) * TRIANGLE_HEIGHT * n;
            let edge = (inside + 0.5).clamp(0.0, 1.0);
            if edge <= 0.0 {
                continue;
            }
            let [a, b, _] = parts.map(|part| part.max(0.0));
            let v = (a + b).clamp(0.0, 1.0);
            let s = if v > 1e-6 { (a / v).clamp(0.0, 1.0) } else { 0.0 };
            let rgb = hsv_to_rgb([hue, s, v]);
            let premultiplied = |c: f32| (byte(c) as f32 * edge).round() as u8;
            *px = [premultiplied(rgb[0]), premultiplied(rgb[1]), premultiplied(rgb[2]), (edge * 255.0).round() as u8];
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_triangle_takes_saturation_and_brightness_and_gives_them_back() {
        for (s, v) in [(1.0, 1.0), (0.0, 1.0), (0.5, 0.5), (0.25, 0.8), (0.9, 0.1)] {
            let p = triangle_point(s, v);
            let back = triangle_sv(p[0], p[1]);
            assert!((back[0] - s).abs() < 1e-3 && (back[1] - v).abs() < 1e-3, "({s}, {v}) came back {back:?}");
        }
        // the corners: the pure hue, white, black
        let [p, w, k] = triangle_corners();
        assert_eq!(triangle_sv(p[0], p[1]), vec![1.0, 1.0]);
        assert_eq!(triangle_sv(w[0], w[1]), vec![0.0, 1.0]);
        assert_eq!(triangle_sv(k[0], k[1])[1], 0.0);
        // past an edge, at the edge
        let past = triangle_sv(1.0, 0.5);
        assert!((past[0] - 1.0).abs() < 1e-3 && (past[1] - 1.0).abs() < 1e-3, "{past:?}");
        // the picture: the pure hue near its corner, nothing outside
        let side = 64;
        let picture = triangle_picture(0.0, side);
        let at = |x: usize, y: usize| picture[(y * side as usize + x) * 4..][..4].to_vec();
        let near = at((p[0] * 64.0) as usize - 3, 32);
        assert!(near[0] > 200 && near[1] < 60 && near[3] == 255, "{near:?}");
        assert_eq!(at(63, 2), vec![0, 0, 0, 0]);
    }

    #[test]
    fn the_square_goes_onto_the_disc_and_back() {
        for (x, y) in [(0.0, 0.0), (0.5, -0.3), (-0.9, 0.9), (1.0, 1.0), (-0.2, 0.7)] {
            let [u, v] = square_to_disc(x, y);
            assert!(u.hypot(v) <= 1.0 + 1e-5);
            let [bx, by] = disc_to_square(u, v);
            assert!((bx - x).abs() < 1e-3 && (by - y).abs() < 1e-3, "({x}, {y}) came back ({bx}, {by})");
        }
        // the disc's top is at full brightness, its bottom black, for any hue
        let side = 64;
        let disc = disc_picture(0.6, side);
        let at = |x: usize, y: usize| {
            disc.get((y * side as usize + x) * 4..(y * side as usize + x) * 4 + 4).unwrap().to_vec()
        };
        assert!(at(32, 2).iter().take(3).any(|&c| c > 230), "{:?}", at(32, 2));
        assert!(at(32, 61).iter().take(3).all(|&c| c < 30), "{:?}", at(32, 61));
    }

    #[test]
    fn harmony_colours_go_to_p3_and_back() {
        for rgb in [0x808080, 0xC04020, 0x2060A0, 0x40A040] {
            let [h, s, v] = p3_hsv_of(rgb, false);
            let back = p3_hsv_to_rgb(h, s, v, false);
            for shift in [16, 8, 0] {
                let (a, b) = ((rgb >> shift & 0xFF) as i32, (back >> shift & 0xFF) as i32);
                assert!((a - b).abs() <= 2, "{rgb:06X} came back {back:06X}");
            }
            // a P3 canvas takes them as they are
            let [h, s, v] = p3_hsv_of(rgb, true);
            assert_eq!(p3_hsv_to_rgb(h, s, v, true), rgb);
        }
    }

    #[test]
    fn the_wheel_is_red_to_the_right_and_clear_past_its_edge() {
        let side = 64;
        let wheel = harmony_wheel(1.0, side, true);
        let at = |x: usize, y: usize| {
            wheel.get((y * side as usize + x) * 4..(y * side as usize + x) * 4 + 4).unwrap().to_vec()
        };
        // the right edge's middle is full red; a corner is clear
        let right = at(61, 32);
        assert!(right[0] > 240 && right[1] < 40 && right[2] < 40 && right[3] == 255, "{right:?}");
        assert_eq!(at(0, 0), [0, 0, 0, 0]);
    }
}
