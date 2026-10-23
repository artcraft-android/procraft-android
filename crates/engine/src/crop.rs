//! Cropping and resizing the canvas: where the new canvas's pixels come from on the old one, and
//! the pixels resampled.

use crate::document::MAX_CANVAS_EDGE;
use crate::geom::{Affine, Vec2};

/// A new canvas: its size, and the map from its pixels to the old canvas's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub width: u32,
    pub height: u32,
    pub to_old: Affine,
}

/// Keeping the rectangle `rect` (x, y, width, height on the old canvas; it may reach past the
/// edges), turned by `angle` radians about its centre. `size` resamples what is kept to that many
/// pixels; otherwise it keeps a pixel for each of the old canvas's.
pub fn crop(rect: [f32; 4], angle: f32, size: Option<(u32, u32)>) -> Crop {
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    let [x, y] = [finite(rect[0], 0.0), finite(rect[1], 0.0)];
    let max = MAX_CANVAS_EDGE as f32;
    let (w, h) = (finite(rect[2], 1.0).clamp(1.0, max), finite(rect[3], 1.0).clamp(1.0, max));
    let (width, height) = size.unwrap_or((w.round() as u32, h.round() as u32));
    let (width, height) = (width.clamp(1, MAX_CANVAS_EDGE), height.clamp(1, MAX_CANVAS_EDGE));
    let (fw, fh) = (width as f32, height as f32);
    // a new pixel, about the new canvas's middle, scaled to the rectangle, turned, and placed
    let to_old = Affine::translate(Vec2::new(-fw / 2.0, -fh / 2.0))
        .then(Affine { a: w / fw, d: h / fh, ..Affine::IDENTITY })
        .then(Affine::rotate(finite(angle, 0.0)))
        .then(Affine::translate(Vec2::new(x + w / 2.0, y + h / 2.0)));
    Crop { width, height, to_old }
}

/// `src` (premultiplied RGBA8, `sw` × `sh`) seen through `crop`: bilinear, transparent past its
/// edges. A move by whole pixels copies them as they are.
pub fn resample(src: &[u8], sw: u32, sh: u32, crop: &Crop) -> Vec<u8> {
    let (w, h) = (crop.width as usize, crop.height as usize);
    let mut out = vec![0u8; w * h * 4];
    let m = crop.to_old;
    let whole = |v: f32| (v - v.round()).abs() < 1e-4;
    if m.a == 1.0 && m.b == 0.0 && m.c == 0.0 && m.d == 1.0 && whole(m.e) && whole(m.f) {
        let (dx, dy) = (m.e.round() as i64, m.f.round() as i64);
        for y in 0..h {
            let sy = y as i64 + dy;
            if sy < 0 || sy >= i64::from(sh) {
                continue;
            }
            // the part of the row that lands on the old canvas
            let x0 = (-dx).clamp(0, w as i64) as usize;
            let x1 = (i64::from(sw) - dx).clamp(0, w as i64) as usize;
            if x1 <= x0 {
                continue;
            }
            let from = ((sy as usize) * sw as usize + (x0 as i64 + dx) as usize) * 4;
            let to = (y * w + x0) * 4;
            let n = (x1 - x0) * 4;
            if let (Some(dst), Some(s)) = (out.get_mut(to..to + n), src.get(from..from + n)) {
                dst.copy_from_slice(s);
            }
        }
        return out;
    }
    let pixel = |x: i64, y: i64| -> [f32; 4] {
        if x < 0 || y < 0 || x >= i64::from(sw) || y >= i64::from(sh) {
            return [0.0; 4];
        }
        let i = ((y as usize) * sw as usize + x as usize) * 4;
        src.get(i..i + 4).and_then(|p| <[u8; 4]>::try_from(p).ok()).map_or([0.0; 4], |p| p.map(f32::from))
    };
    for y in 0..h {
        for x in 0..w {
            let p = m.apply(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
            let (fx, fy) = (p.x - 0.5, p.y - 0.5);
            let (x0, y0) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - x0, fy - y0);
            let (ix, iy) = (x0 as i64, y0 as i64);
            let (a, b, c, d) = (pixel(ix, iy), pixel(ix + 1, iy), pixel(ix, iy + 1), pixel(ix + 1, iy + 1));
            let i = (y * w + x) * 4;
            if let Some(dst) = out.get_mut(i..i + 4) {
                for (k, v) in dst.iter_mut().enumerate() {
                    let at = |p: [f32; 4]| p.get(k).copied().unwrap_or(0.0);
                    let top = at(a) + (at(b) - at(a)) * tx;
                    let bottom = at(c) + (at(d) - at(c)) * tx;
                    *v = (top + (bottom - top) * ty).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32) -> Vec<u8> {
        (0..w * h).flat_map(|i| [(i % w) as u8 * 10, (i / w) as u8 * 10, 0, 255]).collect()
    }

    #[test]
    fn a_crop_keeps_the_rectangle() {
        let src = picture(8, 6);
        let c = crop([2.0, 1.0, 4.0, 3.0], 0.0, None);
        assert_eq!((c.width, c.height), (4, 3));
        let out = resample(&src, 8, 6, &c);
        // the new top left is the old (2, 1)
        assert_eq!(&out[..4], &[20, 10, 0, 255]);
        assert_eq!(&out[(2 * 4 + 3) * 4..(2 * 4 + 3) * 4 + 4], &[50, 30, 0, 255]);
    }

    #[test]
    fn past_the_edges_is_transparent_and_resampling_scales() {
        let src = picture(4, 4);
        let c = crop([-2.0, 0.0, 6.0, 4.0], 0.0, None);
        let out = resample(&src, 4, 4, &c);
        assert_eq!(c.width, 6);
        assert_eq!(&out[..4], &[0, 0, 0, 0]);
        assert_eq!(&out[2 * 4..2 * 4 + 4], &[0, 0, 0, 255]);
        // the whole of it at twice the size: a flat colour stays flat
        let flat = [9u8, 9, 9, 255].repeat(16);
        let c = crop([0.0, 0.0, 4.0, 4.0], 0.0, Some((8, 8)));
        let out = resample(&flat, 4, 4, &c);
        assert_eq!(out.len(), 8 * 8 * 4);
        assert_eq!(&out[(3 * 8 + 3) * 4..(3 * 8 + 3) * 4 + 4], &[9, 9, 9, 255]);
    }

    #[test]
    fn a_half_turn_reverses_the_picture() {
        let src = picture(4, 2);
        let c = crop([0.0, 0.0, 4.0, 2.0], std::f32::consts::PI, None);
        let out = resample(&src, 4, 2, &c);
        // the new top left is the old bottom right
        assert_eq!(&out[..4], &[30, 10, 0, 255]);
    }
}
