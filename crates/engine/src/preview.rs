//! The sample stroke a brush is shown with in the library and in the files it is shared in: a
//! gentle wave across a canvas of [`CANVAS`] pixels, drawn fast, its pressure swelling to full in
//! the middle, at a size the brush's preview size sets.

use crate::brush::Brush;
use crate::geom::Vec2;
use crate::input::Sample;

/// The canvas the sample stroke is drawn on (pixels); a preview of another size fits it in.
pub const CANVAS: (f32, f32) = (1060.0, 324.0);

/// How many pen samples the stroke has.
const SAMPLES: usize = 36;

/// The time between samples (milliseconds): a pen reporting 240 times a second.
const STEP_MS: f64 = 1000.0 / 240.0;

/// The wave, across and down the canvas (shares of its width and height) for 0..1 along it, as
/// polynomial coefficients (lowest power first).
const ACROSS: [f32; 6] = [0.11003, 0.74916, 0.032273, 0.045396, -0.087172, 0.03133];
const DOWN: [f32; 6] = [0.57235, 0.59775, -1.0046, -1.413, 2.7134, -0.97521];

/// The pressure the pen's swell is turned into: the brush's preview minimum plus this many times
/// the swell (times its preview pressure scale).
const PRESSURE_GAIN: f32 = 1.0;

/// Where along the wave `t` (0..1) is, in canvas pixels.
fn along(t: f32) -> Vec2 {
    let poly = |c: &[f32; 6]| c.iter().rev().fold(0.0, |acc, k| acc * t + k);
    Vec2::new(poly(&ACROSS) * CANVAS.0, poly(&DOWN) * CANVAS.1)
}

/// The pen's swell at `t` (0..1 along): light at the ends, full in the middle.
fn swell(t: f32) -> f32 {
    1.005 * (-9.0 * (t - 0.5) * (t - 0.5)).exp()
}

/// The sample stroke for `brush`, in canvas pixels.
pub fn samples(brush: &Brush) -> Vec<Sample> {
    let least = brush.setting("preview_pressure_min");
    // the control's full travel doubles the pressure
    let gain = PRESSURE_GAIN * 2.0 * brush.setting("preview_pressure_scale");
    // the pen's direction turned by the preview's own offset (a quarter turn at 1)
    let azimuth = brush.setting("preview_tilt_offset") * std::f32::consts::FRAC_PI_2;
    (0..SAMPLES)
        .map(|i| {
            let t = i as f32 / (SAMPLES - 1) as f32;
            let pressure = (least + gain * swell(t)).clamp(0.0, 1.0);
            Sample { azimuth, ..Sample::at(along(t), pressure, i as f64 * STEP_MS) }
        })
        .collect()
}

/// The brush's size along the sample stroke, from its least (0) to its most (1): the preview size,
/// less for brushes so big they would not fit, more for small ones (to be seen) and for those
/// that thin out with speed (the stroke is fast).
pub fn size_share(brush: &Brush) -> f32 {
    let units = size_units(brush);
    let small = if units < 4.0 { (0.1 - units.min(0.08)) * 9.5 } else { 0.0 };
    share(brush, units, small)
}

/// The size the studio's drawing pad draws at, as [`size_share`] but without the boost for small
/// brushes.
pub fn pad_size_share(brush: &Brush) -> f32 {
    share(brush, size_units(brush), 0.0)
}

/// The brush's most size in the units brush files keep it in.
fn size_units(brush: &Brush) -> f32 {
    brush.biggest() / brush.size_unit()
}

fn share(brush: &Brush, units: f32, small: f32) -> f32 {
    let speed = 0.3 * brush.setting("speed_size").abs();
    let share = speed + small + brush.setting("preview_size") / (units / 2.0).max(1.0);
    if share.is_finite() { share.clamp(0.0, 1.0) } else { 0.0 }
}

/// The diameter the sample stroke is drawn at (canvas pixels).
pub fn diameter(brush: &Brush) -> f32 {
    let most = brush.biggest();
    let least = brush.min_size.min(most);
    (least + (most - least) * size_share(brush)).max(0.5)
}

/// The canvas (premultiplied RGBA8, [`CANVAS`] in size) fitted into a `width` × `height` preview:
/// as big as fits, in the middle, each pixel the average of the canvas pixels it covers.
pub fn fit(canvas: &[u8], width: u32, height: u32) -> Vec<u8> {
    fit_image(canvas, (CANVAS.0 as u32, CANVAS.1 as u32), width, height)
}

/// An image (premultiplied RGBA8, `size` big) fitted into a `width` × `height` preview, as
/// [`fit`] fits the canvas.
pub fn fit_image(canvas: &[u8], size: (u32, u32), width: u32, height: u32) -> Vec<u8> {
    let (sw, sh) = (size.0 as usize, size.1 as usize);
    let (w, h) = (width as usize, height as usize);
    let k = (width as f32 / size.0.max(1) as f32).min(height as f32 / size.1.max(1) as f32);
    // for each preview row or column, the canvas ones under it and how much of it each covers
    let spans = |n: usize, source: usize, fitted: f32| -> Vec<Vec<(usize, f32)>> {
        let start = (n as f32 - fitted) / 2.0;
        (0..n)
            .map(|o| {
                let (a, b) = ((o as f32 - start) / k, (o as f32 + 1.0 - start) / k);
                let (first, last) = (a.max(0.0).floor() as usize, (b.min(source as f32).ceil() as usize).min(source));
                (first..last)
                    .map(|i| (i, ((i as f32 + 1.0).min(b) - (i as f32).max(a)).max(0.0) * k))
                    .filter(|&(_, weight)| weight > 0.0)
                    .collect()
            })
            .collect()
    };
    let (columns, rows) = (spans(w, sw, sw as f32 * k), spans(h, sh, sh as f32 * k));
    let mut out = vec![0u8; w * h * 4];
    for (y, row) in rows.iter().enumerate() {
        for (x, column) in columns.iter().enumerate() {
            let mut sum = [0.0f32; 4];
            for &(sy, wy) in row {
                for &(sx, wx) in column {
                    let at = (sy * sw + sx) * 4;
                    if let Some(p) = canvas.get(at..at + 4) {
                        for (s, &v) in sum.iter_mut().zip(p) {
                            *s += f32::from(v) * wx * wy;
                        }
                    }
                }
            }
            let at = (y * w + x) * 4;
            if let Some(dst) = out.get_mut(at..at + 4) {
                for (d, s) in dst.iter_mut().zip(sum) {
                    *d = s.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_crosses_the_canvas() {
        let s = samples(&Brush::round_pen());
        assert_eq!(s.len(), SAMPLES);
        let (first, last) = (s.first().unwrap(), s.last().unwrap());
        assert!((first.pos.x - 116.6).abs() < 0.5 && (last.pos.x - 933.9).abs() < 0.5);
        assert!(s.iter().all(|p| p.pos.y > 0.4 * CANVAS.1 && p.pos.y < 0.7 * CANVAS.1));
        // light at the ends, swelling to full in the middle
        assert!(first.pressure < 0.15 && last.pressure < 0.15);
        assert!(s.get(SAMPLES / 4).unwrap().pressure < 0.6);
        assert_eq!(s.get(SAMPLES / 2).unwrap().pressure, 1.0);
        assert!((last.time_ms - 35.0 * STEP_MS).abs() < 1e-9);
    }

    #[test]
    fn small_brushes_show_bigger_and_big_ones_smaller() {
        let mut b = Brush::round_pen();
        b.set_setting("preview_size", 0.3);
        let unit = b.size_unit();
        // under 0.08 of a unit, a small brush is shown up to nearly its full size
        b.max_size = 0.04 * unit;
        assert!((size_share(&b) - (0.3 + 0.06 * 9.5)).abs() < 1e-4);
        b.max_size = 1.0 * unit;
        assert!((size_share(&b) - 0.49).abs() < 1e-4);
        // from 2 units the preview size is shared out over the size
        b.max_size = 8.0 * unit;
        assert!((size_share(&b) - 0.075).abs() < 1e-4);
    }

    #[test]
    fn the_canvas_fits_in_the_middle() {
        let (sw, sh) = (CANVAS.0 as usize, CANVAS.1 as usize);
        // opaque white on the left half
        let canvas: Vec<u8> = (0..sw * sh).flat_map(|i| if i % sw < sw / 2 { [255u8; 4] } else { [0; 4] }).collect();
        // half the size: the halves stay halves
        let half = fit(&canvas, 530, 162);
        assert_eq!(half.len(), 530 * 162 * 4);
        assert_eq!(half.get((80 * 530 + 264) * 4..(80 * 530 + 265) * 4), Some(&[255u8; 4][..]));
        assert_eq!(half.get((80 * 530 + 265) * 4..(80 * 530 + 266) * 4), Some(&[0u8; 4][..]));
        // a taller preview: the canvas across its width, empty above and below
        let tall = fit(&canvas, 106, 100);
        let a = |x: usize, y: usize| tall.get((y * 106 + x) * 4 + 3).copied();
        assert_eq!(a(10, 2), Some(0));
        assert_eq!(a(10, 50), Some(255));
        assert_eq!(a(10, 97), Some(0));
    }
}
