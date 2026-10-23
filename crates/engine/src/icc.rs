//! ICC profiles for the colour profiles a canvas can be in, made here from the colour spaces'
//! definitions: a version 4 RGB display profile with a matrix and three tone curves.
//!
//! The primaries come from their chromaticities and the white point (D65), adapted to the ICC's
//! D50 with the Bradford transform; the tone curve is the sRGB curve as a parametric curve.

use crate::profile::Profile;

/// D65's chromaticity.
const D65: (f64, f64) = (0.3127, 0.3290);

/// The ICC connection space's white, D50, as XYZ.
const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// The Bradford cone response matrix.
const BRADFORD: [[f64; 3]; 3] = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];

/// The red, green and blue chromaticities of `profile`.
fn primaries(profile: Profile) -> [(f64, f64); 3] {
    if profile.is_p3() {
        [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)]
    } else {
        [(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)]
    }
}

fn xyz(c: (f64, f64)) -> [f64; 3] {
    [c.0 / c.1, 1.0, (1.0 - c.0 - c.1) / c.1]
}

fn transpose(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    [[a, d, g], [b, e, h], [c, f, i]]
}

fn mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let columns = transpose(b);
    a.map(|row| columns.map(|col| row[0] * col[0] + row[1] * col[1] + row[2] * col[2]))
}

fn apply(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

fn inverse(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
        [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
        [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det],
    ]
}

/// The Bradford adaptation from `from` white (XYZ) to D50.
fn adaptation(from: [f64; 3]) -> [[f64; 3]; 3] {
    let s = apply(BRADFORD, from);
    let d = apply(BRADFORD, D50);
    let scale = [[d[0] / s[0], 0.0, 0.0], [0.0, d[1] / s[1], 0.0], [0.0, 0.0, d[2] / s[2]]];
    mul(inverse(BRADFORD), mul(scale, BRADFORD))
}

/// The profile's RGB-to-XYZ matrix (columns red, green, blue), adapted to D50, and the
/// adaptation itself.
fn matrices(profile: Profile) -> ([[f64; 3]; 3], [[f64; 3]; 3]) {
    let [r, g, b] = primaries(profile).map(xyz);
    let white = xyz(D65);
    let m = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let s = apply(inverse(m), white);
    let rgb_to_xyz = m.map(|row| [row[0] * s[0], row[1] * s[1], row[2] * s[2]]);
    let chad = adaptation(white);
    (mul(chad, rgb_to_xyz), chad)
}

/// A number as ICC's s15Fixed16.
fn fixed(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn xyz_tag(v: [f64; 3]) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for c in v {
        t.extend(fixed(c));
    }
    t
}

/// A 'mluc' text in English.
fn text_tag(s: &str) -> Vec<u8> {
    let utf16: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut t = b"mluc\0\0\0\0".to_vec();
    t.extend(1u32.to_be_bytes());
    t.extend(12u32.to_be_bytes());
    t.extend(b"enUS");
    t.extend((utf16.len() as u32).to_be_bytes());
    t.extend(28u32.to_be_bytes());
    t.extend(utf16);
    t
}

/// The sRGB tone curve as a parametric curve (function type 3).
fn curve_tag() -> Vec<u8> {
    let mut t = b"para\0\0\0\0".to_vec();
    t.extend(3u16.to_be_bytes());
    t.extend([0, 0]);
    for v in [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.040_45] {
        t.extend(fixed(v));
    }
    t
}

fn sf32_tag(m: [[f64; 3]; 3]) -> Vec<u8> {
    let mut t = b"sf32\0\0\0\0".to_vec();
    for row in m {
        for v in row {
            t.extend(fixed(v));
        }
    }
    t
}

/// An ICC profile for `profile`: what an image file carries so that other programs show its
/// colours as the canvas does.
pub fn icc(profile: Profile) -> Vec<u8> {
    let (m, chad) = matrices(profile);
    let [red, green, blue] = transpose(m);
    let curve = curve_tag();
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", text_tag(profile.name())),
        (b"cprt", text_tag("No copyright, use freely")),
        (b"wtpt", xyz_tag(D50)),
        (b"chad", sf32_tag(chad)),
        (b"rXYZ", xyz_tag(red)),
        (b"gXYZ", xyz_tag(green)),
        (b"bXYZ", xyz_tag(blue)),
        (b"rTRC", curve.clone()),
        (b"gTRC", curve.clone()),
        (b"bTRC", curve),
    ];
    // the tag table, then each tag's data on a 4-byte boundary
    let mut offset = 128 + 4 + 12 * tags.len();
    let mut table = (tags.len() as u32).to_be_bytes().to_vec();
    let mut data = Vec::new();
    for (sig, bytes) in &tags {
        table.extend(sig.as_slice());
        table.extend((offset as u32).to_be_bytes());
        table.extend((bytes.len() as u32).to_be_bytes());
        data.extend(bytes);
        while data.len() % 4 != 0 {
            data.push(0);
        }
        offset = 128 + 4 + 12 * tags.len() + data.len();
    }
    let size = 128 + table.len() + data.len();
    let mut out = Vec::with_capacity(size);
    out.extend((size as u32).to_be_bytes());
    out.extend([0; 4]); // preferred CMM: none
    out.extend([4, 0x30, 0, 0]); // version 4.3
    out.extend(b"mntrRGB XYZ ");
    out.extend([0x07, 0xEA, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0]); // made 2026-01-01
    out.extend(b"acsp");
    out.extend([0; 4 + 4 + 4 + 4 + 8 + 4]); // platform, flags, maker, model, attributes, intent (perceptual)
    for c in D50 {
        out.extend(fixed(c));
    }
    out.extend([0; 4 + 16 + 28]); // creator, profile ID (not computed), reserved
    out.extend(table);
    out.extend(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be32(b: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    fn tag<'a>(icc: &'a [u8], sig: &[u8; 4]) -> &'a [u8] {
        let n = be32(icc, 128) as usize;
        for i in 0..n {
            let at = 132 + 12 * i;
            if &icc[at..at + 4] == sig {
                let (off, len) = (be32(icc, at + 4) as usize, be32(icc, at + 8) as usize);
                return &icc[off..off + len];
            }
        }
        panic!("no tag {sig:?}");
    }

    fn xyz_of(t: &[u8]) -> [f64; 3] {
        [0, 1, 2]
            .map(|i| i32::from_be_bytes([t[8 + 4 * i], t[9 + 4 * i], t[10 + 4 * i], t[11 + 4 * i]]) as f64 / 65536.0)
    }

    #[test]
    fn the_profiles_carry_their_primaries() {
        for (profile, red, green, blue) in [
            (Profile::Srgb, [0.4361, 0.2225, 0.0139], [0.3851, 0.7169, 0.0971], [0.1431, 0.0606, 0.7141]),
            (Profile::DisplayP3, [0.5151, 0.2412, -0.0011], [0.2920, 0.6922, 0.0419], [0.1571, 0.0666, 0.7841]),
        ] {
            let icc = icc(profile);
            assert_eq!(be32(&icc, 0) as usize, icc.len());
            assert_eq!(&icc[36..40], b"acsp");
            assert_eq!(&icc[12..20], b"mntrRGB ");
            for (sig, want) in [(b"rXYZ", red), (b"gXYZ", green), (b"bXYZ", blue)] {
                let got = xyz_of(tag(&icc, sig));
                assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 5e-4), "{profile:?} {sig:?} {got:?}");
            }
            // the three primaries add up to the connection space's white
            let sum = [0, 1, 2]
                .map(|i| xyz_of(tag(&icc, b"rXYZ"))[i] + xyz_of(tag(&icc, b"gXYZ"))[i] + xyz_of(tag(&icc, b"bXYZ"))[i]);
            assert!(sum.iter().zip(D50).all(|(s, w)| (s - w).abs() < 2e-3), "{sum:?}");
            assert_eq!(&tag(&icc, b"rTRC")[..4], b"para");
        }
    }
}
