//! Environments for 3D painting's lighting: panoramas of light (equirectangular, linear RGB)
//! read from Radiance `.hdr` files, their irradiance as spherical harmonics, and the table of the
//! split-sum approximation to a rough surface's reflection (the "BRDF table").
//!
//! Directions are y up; a panorama's middle (u = 0.5) looks along -z, its left edge along +z
//! through -x (u = atan2(x, -z) / 2pi + 0.5), its top row straight up.

use std::f32::consts::PI;

/// A panorama of light: `width` × `height` linear RGB, rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Equirect {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 3]>,
}

impl Equirect {
    /// The same light everywhere.
    pub fn uniform(rgb: [f32; 3]) -> Equirect {
        Equirect { width: 8, height: 4, pixels: vec![rgb; 32] }
    }

    fn at(&self, x: u32, y: u32) -> [f32; 3] {
        let i = (y.min(self.height.saturating_sub(1)) * self.width + x.min(self.width.saturating_sub(1))) as usize;
        self.pixels.get(i).copied().unwrap_or([0.0; 3])
    }

    /// Half the size each way (box filtered), at least 1 × 1.
    pub fn halved(&self) -> Equirect {
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0.0f32; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = self.at(x * 2 + dx, y * 2 + dy);
                    for (s, v) in sum.iter_mut().zip(p) {
                        *s += v / 4.0;
                    }
                }
                pixels.push(sum);
            }
        }
        Equirect { width: w, height: h, pixels }
    }

    /// Halved until no wider than `width`.
    pub fn at_most(&self, width: u32) -> Equirect {
        let mut e = self.clone();
        while e.width > width.max(1) {
            e = e.halved();
        }
        e
    }

    /// The irradiance from every direction as nine spherical-harmonic coefficients (RGB), scaled
    /// so that [`irradiance`] gives the cosine-weighted mean of the light about a normal: a
    /// white surface's diffuse light (a uniform panorama of 1 gives 1).
    pub fn irradiance_sh(&self) -> [[f32; 3]; 9] {
        let mut sh = [[0.0f32; 3]; 9];
        let (w, h) = (self.width as f32, self.height as f32);
        let mut total = 0.0;
        for y in 0..self.height {
            let theta = (y as f32 + 0.5) / h * PI;
            // each texel's share of the sphere
            let weight = theta.sin() * (2.0 * PI / w) * (PI / h);
            for x in 0..self.width {
                let phi = ((x as f32 + 0.5) / w - 0.5) * 2.0 * PI;
                let d = [theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos()];
                let basis = sh_basis(d);
                let light = self.at(x, y);
                for (coefficient, b) in sh.iter_mut().zip(basis) {
                    for (c, l) in coefficient.iter_mut().zip(light) {
                        *c += l * b * weight;
                    }
                }
                total += weight;
            }
        }
        // the cosine lobe's bands, over pi; and the grid's share of the sphere made exact
        let band = [1.0, 2.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 0.25, 0.25, 0.25, 0.25, 0.25];
        let fix = if total > 0.0 { 4.0 * PI / total } else { 1.0 };
        for (coefficient, b) in sh.iter_mut().zip(band) {
            for c in coefficient.iter_mut() {
                *c *= b * fix;
            }
        }
        sh
    }
}

/// The nine real spherical harmonics (bands 0 to 2) at unit direction `d`.
pub fn sh_basis(d: [f32; 3]) -> [f32; 9] {
    let [x, y, z] = d;
    [
        0.282_095,
        0.488_603 * y,
        0.488_603 * z,
        0.488_603 * x,
        1.092_548 * x * y,
        1.092_548 * y * z,
        0.315_392 * (3.0 * z * z - 1.0),
        1.092_548 * x * z,
        0.546_274 * (x * x - y * y),
    ]
}

/// The irradiance about normal `n` from coefficients made by [`Equirect::irradiance_sh`].
pub fn irradiance(sh: &[[f32; 3]; 9], n: [f32; 3]) -> [f32; 3] {
    let basis = sh_basis(n);
    let mut out = [0.0f32; 3];
    for (coefficient, b) in sh.iter().zip(basis) {
        for (o, c) in out.iter_mut().zip(coefficient) {
            *o += c * b;
        }
    }
    out.map(|v| v.max(0.0))
}

/// Reads a Radiance `.hdr` picture (RGBE, flat or run-length coded scanlines).
pub fn read_hdr(bytes: &[u8]) -> Option<Equirect> {
    // the header: lines up to an empty one, then the size line
    let mut at = 0usize;
    let line = |at: &mut usize| -> Option<String> {
        let rest = bytes.get(*at..)?;
        let end = rest.iter().position(|&b| b == b'\n')?;
        let text = String::from_utf8_lossy(rest.get(..end)?).into_owned();
        *at += end + 1;
        Some(text)
    };
    let first = line(&mut at)?;
    if !first.starts_with("#?") {
        return None;
    }
    loop {
        let l = line(&mut at)?;
        if l.trim().is_empty() {
            break;
        }
        if let Some(format) = l.strip_prefix("FORMAT=")
            && format.trim() != "32-bit_rle_rgbe"
        {
            return None;
        }
    }
    let size = line(&mut at)?;
    let words: Vec<&str> = size.split_whitespace().collect();
    let (height, width) = match words.as_slice() {
        ["-Y", h, "+X", w] => (h.parse::<u32>().ok()?, w.parse::<u32>().ok()?),
        _ => return None,
    };
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 1 << 26 {
        return None;
    }
    let mut pixels = Vec::with_capacity((width * height) as usize);
    let mut scan = vec![[0u8; 4]; width as usize];
    for _ in 0..height {
        let head = bytes.get(at..at + 4)?;
        let coded = (8..0x8000).contains(&width) && head.first() == Some(&2) && head.get(1) == Some(&2);
        if coded {
            at += 4;
            // each of the four channels in turn: runs and literal stretches
            for channel in 0..4 {
                let mut x = 0usize;
                while x < width as usize {
                    let count = usize::from(*bytes.get(at)?);
                    at += 1;
                    if count > 128 {
                        let value = *bytes.get(at)?;
                        at += 1;
                        for _ in 0..count - 128 {
                            *scan.get_mut(x)?.get_mut(channel)? = value;
                            x += 1;
                        }
                    } else {
                        if count == 0 {
                            return None;
                        }
                        for _ in 0..count {
                            *scan.get_mut(x)?.get_mut(channel)? = *bytes.get(at)?;
                            at += 1;
                            x += 1;
                        }
                    }
                }
            }
        } else {
            for px in scan.iter_mut() {
                *px = bytes.get(at..at + 4)?.try_into().ok()?;
                at += 4;
            }
        }
        pixels.extend(scan.iter().map(|&[r, g, b, e]| {
            if e == 0 {
                [0.0; 3]
            } else {
                let f = 2f32.powi(i32::from(e) - 136);
                [f32::from(r) * f, f32::from(g) * f, f32::from(b) * f]
            }
        }));
    }
    Some(Equirect { width, height, pixels })
}

/// Writes a picture as a Radiance `.hdr` file (flat RGBE scanlines).
pub fn write_hdr(e: &Equirect) -> Vec<u8> {
    let mut out = format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {} +X {}\n", e.height, e.width).into_bytes();
    for p in &e.pixels {
        let m = p[0].max(p[1]).max(p[2]);
        if m < 1e-32 {
            out.extend_from_slice(&[0; 4]);
            continue;
        }
        let exp = m.log2().floor() as i32 + 1;
        let scale = 256.0 / 2f32.powi(exp);
        let byte = |v: f32| (v * scale).clamp(0.0, 255.0) as u8;
        out.extend_from_slice(&[byte(p[0]), byte(p[1]), byte(p[2]), (exp + 128).clamp(0, 255) as u8]);
    }
    out
}

/// The split-sum table for image lighting: for a view at cosine `n·v` across and a roughness
/// down (each `size` steps, cell centres), the scale and bias on a surface's base reflectance
/// that give its share of the prefiltered light (GGX, Smith-Schlick with k = a/2 for image
/// light), from `samples` importance samples.
pub fn brdf_table(size: usize, samples: u32) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(size * size);
    for row in 0..size {
        let roughness = (row as f32 + 0.5) / size as f32;
        for col in 0..size {
            let nv = ((col as f32 + 0.5) / size as f32).max(1e-4);
            out.push(integrate_brdf(nv, roughness, samples));
        }
    }
    out
}

fn integrate_brdf(nv: f32, roughness: f32, samples: u32) -> [f32; 2] {
    let v = [(1.0 - nv * nv).max(0.0).sqrt(), 0.0, nv];
    let a = roughness * roughness;
    let k = a / 2.0;
    let g1 = |c: f32| c / (c * (1.0 - k) + k);
    let (mut scale, mut bias) = (0.0f32, 0.0f32);
    for i in 0..samples {
        let (u1, u2) = hammersley(i, samples);
        // a half vector by GGX
        let phi = 2.0 * PI * u1;
        let cos_t = ((1.0 - u2) / (1.0 + (a * a - 1.0) * u2)).max(0.0).sqrt();
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
        let hv = [sin_t * phi.cos(), sin_t * phi.sin(), cos_t];
        let vh = v[0] * hv[0] + v[1] * hv[1] + v[2] * hv[2];
        let l = [2.0 * vh * hv[0] - v[0], 2.0 * vh * hv[1] - v[1], 2.0 * vh * hv[2] - v[2]];
        let nl = l[2];
        let nh = hv[2].max(0.0);
        if nl > 0.0 && nh > 0.0 {
            let g = g1(nl) * g1(nv);
            let visible = g * vh.max(0.0) / (nh * nv);
            let fresnel = (1.0 - vh.max(0.0)).powi(5);
            scale += (1.0 - fresnel) * visible;
            bias += fresnel * visible;
        }
    }
    [scale / samples as f32, bias / samples as f32]
}

/// Point `i` of `n` of the Hammersley set.
fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (i as f32 / n.max(1) as f32, i.reverse_bits() as f32 * 2.328_306_4e-10)
}

/// A float as half-precision bits (rounded to nearest, overflowing to infinity).
pub fn to_half(f: f32) -> u16 {
    let bits = f.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mantissa != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 31 {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = (mantissa | 0x80_0000) >> (1 - e);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    let half = sign | ((e as u16) << 10) | (mantissa >> 13) as u16;
    // round half up on the dropped bits
    if mantissa & 0x1000 != 0 { half + 1 } else { half }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uniform_panorama_lights_every_way_alike() {
        let sh = Equirect { width: 64, height: 32, pixels: vec![[0.5, 1.0, 2.0]; 64 * 32] }.irradiance_sh();
        for n in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, -1.0, 0.0]] {
            let e = irradiance(&sh, n);
            for (got, want) in e.iter().zip([0.5, 1.0, 2.0]) {
                assert!((got - want).abs() < 0.01 * want, "{n:?}: {e:?}");
            }
        }
    }

    #[test]
    fn light_from_above_lights_what_faces_up() {
        // the top half bright, the bottom dark
        let (w, h) = (64u32, 32u32);
        let pixels = (0..w * h).map(|i| if i / w < h / 2 { [1.0; 3] } else { [0.0; 3] }).collect();
        let sh = Equirect { width: w, height: h, pixels }.irradiance_sh();
        let up = irradiance(&sh, [0.0, 1.0, 0.0])[0];
        let side = irradiance(&sh, [1.0, 0.0, 0.0])[0];
        let down = irradiance(&sh, [0.0, -1.0, 0.0])[0];
        // a hemisphere of 1 overhead: about 1 up, a half sideways, about none below
        assert!((up - 1.0).abs() < 0.1 && (side - 0.5).abs() < 0.05 && down < 0.1, "{up} {side} {down}");
    }

    #[test]
    fn hdr_files_round_trip() {
        let e = Equirect {
            width: 3,
            height: 2,
            pixels: vec![[0.0; 3], [1.0, 0.5, 0.25], [100.0, 3.0, 0.01], [2.0; 3], [0.1; 3], [7.5, 0.0, 1.0]],
        };
        let back = read_hdr(&write_hdr(&e)).unwrap();
        assert_eq!((back.width, back.height), (3, 2));
        // one exponent a pixel: as exact as its brightest channel allows
        for (a, b) in back.pixels.iter().zip(&e.pixels) {
            let most = b[0].max(b[1]).max(b[2]);
            for (x, y) in a.iter().zip(b) {
                assert!((x - y).abs() <= most / 128.0 + 1e-3, "{a:?} {b:?}");
            }
        }
        assert!(read_hdr(b"not a picture").is_none());
        assert!(read_hdr(b"#?RADIANCE\n\n-Y 2 +X 2\n\x02\x02").is_none());
    }

    #[test]
    fn the_brdf_table_has_the_known_shape() {
        let t = brdf_table(16, 512);
        // smooth, seen square on: nearly all scale, no bias
        let smooth = t[15];
        assert!(smooth[0] > 0.9 && smooth[1] < 0.05, "{smooth:?}");
        // rough at grazing: little of either
        let rough = t[15 * 16];
        assert!(rough[0] < 0.6 && rough[1] < 0.2, "{rough:?}");
        assert!(t.iter().all(|v| v[0] >= 0.0 && v[1] >= 0.0 && v[0] + v[1] <= 1.05));
    }

    #[test]
    fn halves_convert() {
        assert_eq!(to_half(1.0), 0x3c00);
        assert_eq!(to_half(-2.0), 0xc000);
        assert_eq!(to_half(0.5), 0x3800);
        assert_eq!(to_half(65504.0), 0x7bff);
        assert_eq!(to_half(1e9), 0x7c00);
        assert_eq!(to_half(0.0), 0);
    }
}
