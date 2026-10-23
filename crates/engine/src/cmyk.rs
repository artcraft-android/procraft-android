//! ProCraft's own CMYK profile, made here: a press printing on coated paper, modelled from the
//! colours of its inks and their overprints, written as an ICC version 2 output profile with
//! 16-bit tables (the same tables for every rendering intent).
//!
//! The model mixes the sixteen overprints of cyan, magenta, yellow and black by the area each
//! covers (inks spread as they print: a 50% dot prints as 65%), the light reaching the eye
//! scattered in the paper. Separations put black in as grey builds up, cap the four inks at 300%
//! between them, and bring colours the press can't print to the nearest it can at their
//! lightness. The tables run from the paper's white to the press's deepest black.

use std::sync::OnceLock;

use crate::cms::{D50, lab_to_xyz, xyz_to_lab};

/// The profile's name, as lists show it.
pub const NAME: &str = "Generic CMYK";

/// The paper and the inks alone and over each other (cyan, magenta, yellow), as CIE Lab (D50).
const PAPER: [f64; 3] = [95.0, 0.0, -2.0];
const OVERPRINTS: [[f64; 3]; 8] = [
    PAPER,
    [89.0, -5.0, 93.0],   // yellow
    [48.0, 74.0, -3.0],   // magenta
    [47.0, 68.0, 48.0],   // magenta and yellow
    [55.0, -37.0, -50.0], // cyan
    [50.0, -65.0, 27.0],  // cyan and yellow
    [24.0, 22.0, -46.0],  // cyan and magenta
    [23.0, 0.0, 0.0],     // all three
];
/// Black ink alone.
const BLACK: [f64; 3] = [16.0, 0.0, 0.0];
/// Light the surface gives back whatever the ink: a share of the paper's.
const FLARE: f64 = 0.008;
/// How far light scatters in the paper (the Yule–Nielsen factor).
const SCATTER: f64 = 1.6;
/// How much a 50% dot grows when printed.
const DOT_GAIN: f64 = 0.15;
/// The most ink the four colours may put down together.
const INK_LIMIT: f64 = 3.0;

/// The press: each overprint's colour (with and without black) ready to mix.
struct Press {
    /// For the 16 overprints (black slowest, then cyan, magenta, yellow): XYZ less the flare,
    /// to the power 1 / SCATTER.
    primaries: [[f64; 3]; 16],
    flare: [f64; 3],
    paper: [f64; 3],
    /// The deepest black it prints, relative to the paper.
    black: [f64; 3],
}

impl Press {
    fn new() -> Press {
        let paper = lab_to_xyz(PAPER);
        let flare = paper.map(|w| w * FLARE);
        let less = |c: [f64; 3]| zip3(lab_to_xyz(c), flare, |x, f| (x - f).max(1e-6));
        let (white, black) = (less(PAPER), less(BLACK));
        let mut primaries = [[0.0; 3]; 16];
        for (i, p) in primaries.iter_mut().enumerate() {
            let base = less(OVERPRINTS.get(i % 8).copied().unwrap_or(PAPER));
            // black over the rest lets through what black alone lets through of the paper
            let x = if i >= 8 { zip3(zip3(base, black, |a, b| a * b), white, |a, w| a / w) } else { base };
            *p = x.map(|v| v.powf(1.0 / SCATTER));
        }
        let mut press = Press { primaries, flare, paper, black: [0.0; 3] };
        press.black = press.relative(press.print(press.separate_black()));
        press
    }

    /// The colour (absolute D50 XYZ) of inks `cmyk` (0..1 each) on the paper.
    fn print(&self, cmyk: [f64; 4]) -> [f64; 3] {
        let [c, m, y, k] = cmyk.map(|v| {
            let v = v.clamp(0.0, 1.0);
            v + 4.0 * DOT_GAIN * v * (1.0 - v)
        });
        let mut sum = [0.0; 3];
        for (i, p) in self.primaries.iter().enumerate() {
            let on = |bit: usize, v: f64| if i & bit != 0 { v } else { 1.0 - v };
            let w = on(8, k) * on(4, c) * on(2, m) * on(1, y);
            if w > 0.0 {
                for (s, v) in sum.iter_mut().zip(p) {
                    *s += w * v;
                }
            }
        }
        zip3(sum, self.flare, |s, f| s.powf(SCATTER) + f)
    }

    /// `xyz` relative to the paper: the paper as the connection space's white.
    fn relative(&self, xyz: [f64; 3]) -> [f64; 3] {
        zip3(zip3(xyz, self.paper, |x, p| x / p), D50, |x, w| x * w)
    }

    /// The connection-space colour of `cmyk`: relative to the paper, the press's black at 0.
    fn to_lab(&self, cmyk: [f64; 4]) -> [f64; 3] {
        let above = zip3(self.relative(self.print(cmyk)), self.black, |x, b| x - b);
        let range = zip3(D50, self.black, |w, b| w - b);
        xyz_to_lab(zip3(zip3(above, range, |a, r| a / r), D50, |v, w| (v * w).max(0.0)))
    }

    /// The deepest neutral it prints: black ink, then as much of the others as the limit lets.
    fn separate_black(&self) -> [f64; 4] {
        let k = 0.87;
        let mut best = [0.0, 0.0, 0.0, k];
        let mut darkest = f64::MAX;
        // the balance of the three inks that gives the least light, over a coarse grid
        for c in 50..=100 {
            for m in 50..=100 {
                let (c, m) = (f64::from(c) / 100.0, f64::from(m) / 100.0);
                let y = (INK_LIMIT - k - c - m).clamp(0.0, 1.0);
                let xyz = self.print([c, m, y, k]);
                let lab = xyz_to_lab(self.relative(xyz));
                let score = lab[0] + 0.3 * (lab[1].abs() + lab[2].abs());
                if score < darkest {
                    darkest = score;
                    best = [c, m, y, k];
                }
            }
        }
        best
    }

    /// The inks for connection-space colour `lab`: black by how grey the colour is, the others
    /// solved for; a colour out of reach comes as near as the press gets at its lightness.
    fn separate(&self, lab: [f64; 3], start: [f64; 4]) -> [f64; 4] {
        let cmy = self.solve(lab, 0.0, [start[0], start[1], start[2]]);
        // grey component replacement: black for the part all three share, from a quarter on
        let grey = cmy.iter().copied().fold(1.0, f64::min);
        let k = (((grey - 0.25) / 0.75).clamp(0.0, 1.0)).powf(1.2) * 0.87;
        if k <= 0.0 {
            return [cmy[0], cmy[1], cmy[2], 0.0];
        }
        let cmy = self.solve(lab, k, cmy);
        [cmy[0], cmy[1], cmy[2], k]
    }

    /// Cyan, magenta and yellow that print `lab` with black `k`, from `start`: the least weighted
    /// difference (lightness counted twice), within the ink limit.
    fn solve(&self, lab: [f64; 3], k: f64, start: [f64; 3]) -> [f64; 3] {
        let error = |cmy: [f64; 3]| {
            let got = self.to_lab([cmy[0], cmy[1], cmy[2], k]);
            [2.0 * (got[0] - lab[0]), got[1] - lab[1], got[2] - lab[2]]
        };
        let fit = |cmy: [f64; 3]| {
            let mut v = cmy.map(|c| c.clamp(0.0, 1.0));
            let total: f64 = v.iter().sum();
            if total + k > INK_LIMIT {
                let scale = (INK_LIMIT - k).max(0.0) / total;
                v = v.map(|c| c * scale);
            }
            v
        };
        let mut cmy = fit(start);
        let mut lambda = 1e-3;
        let mut e = error(cmy);
        let mut cost = dot(e, e);
        for _ in 0..30 {
            if cost < 1e-6 {
                break;
            }
            // the Jacobian, a column for each ink, by small steps (backwards at full ink)
            let column = |which: usize| {
                let h = if pick(cmy, which) + 1e-4 > 1.0 { -1e-4 } else { 1e-4 };
                zip3(error(nudge(cmy, which, h)), e, |a, b| (a - b) / h)
            };
            let [d0, d1, d2] = [column(0), column(1), column(2)];
            // (JᵀJ + λ) step = −Jᵀ e
            let damp = |v: f64| v + lambda * (1.0 + v);
            let a = [
                [damp(dot(d0, d0)), dot(d0, d1), dot(d0, d2)],
                [dot(d1, d0), damp(dot(d1, d1)), dot(d1, d2)],
                [dot(d2, d0), dot(d2, d1), damp(dot(d2, d2))],
            ];
            let g = [-dot(d0, e), -dot(d1, e), -dot(d2, e)];
            let Some(step) = solve3(a, g) else { break };
            let next = fit(zip3(cmy, step, |c, s| c + s));
            let e2 = error(next);
            let cost2 = dot(e2, e2);
            if cost2 < cost {
                (cmy, e, cost) = (next, e2, cost2);
                lambda = (lambda / 3.0).max(1e-7);
            } else {
                lambda *= 4.0;
                if lambda > 1e6 {
                    break;
                }
            }
        }
        cmy
    }
}

/// `a` x = `b` for three unknowns; none if `a` can't be inverted.
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(a);
    if d.abs() < 1e-18 {
        return None;
    }
    // Cramer's rule
    let replaced = |k: usize| {
        let [r0, r1, r2] = a;
        [nudge(r0, k, b[0] - pick(r0, k)), nudge(r1, k, b[1] - pick(r1, k)), nudge(r2, k, b[2] - pick(r2, k))]
    };
    Some([det(replaced(0)) / d, det(replaced(1)) / d, det(replaced(2)) / d])
}

/// The three values of `a` and `b` paired through `f`.
fn zip3(a: [f64; 3], b: [f64; 3], f: impl Fn(f64, f64) -> f64) -> [f64; 3] {
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `v` with `by` added to value `which` (0 to 2).
fn nudge(v: [f64; 3], which: usize, by: f64) -> [f64; 3] {
    match which {
        0 => [v[0] + by, v[1], v[2]],
        1 => [v[0], v[1] + by, v[2]],
        _ => [v[0], v[1], v[2] + by],
    }
}

/// Value `which` (0 to 2) of `v`.
fn pick(v: [f64; 3], which: usize) -> f64 {
    match which {
        0 => v[0],
        1 => v[1],
        _ => v[2],
    }
}

/// The profile's file, made once.
pub fn profile() -> Vec<u8> {
    static MADE: OnceLock<Vec<u8>> = OnceLock::new();
    MADE.get_or_init(make).clone()
}

/// Grid points along each input of the tables.
const TO_LAB_GRID: usize = 9;
const TO_INK_GRID: usize = 17;

fn make() -> Vec<u8> {
    let press = Press::new();
    // ink to the connection space
    let n = TO_LAB_GRID;
    let mut to_lab = Vec::with_capacity(n.pow(4) * 3);
    for c in 0..n {
        for m in 0..n {
            for y in 0..n {
                for k in 0..n {
                    let v = [c, m, y, k].map(|i| i as f64 / (n - 1) as f64);
                    to_lab.extend(legacy_lab(press.to_lab(v)));
                }
            }
        }
    }
    // the connection space to ink, each point starting from the one before
    let n = TO_INK_GRID;
    let mut to_ink = Vec::with_capacity(n.pow(3) * 4);
    let mut start = [0.0; 4];
    for l in 0..n {
        for a in 0..n {
            for b in 0..n {
                let v = [l, a, b].map(|i| i as f64 / (n - 1) as f64);
                let lab =
                    [v[0] * 65535.0 / 65280.0 * 100.0, v[1] * 65535.0 / 256.0 - 128.0, v[2] * 65535.0 / 256.0 - 128.0];
                let ink = if lab[0] >= 100.0 && lab[1].abs() < 1.0 && lab[2].abs() < 1.0 {
                    [0.0; 4]
                } else {
                    press.separate(lab, start)
                };
                start = ink;
                to_ink.extend(ink.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16));
            }
        }
    }
    let a2b = mft2(4, 3, TO_LAB_GRID, &to_lab);
    let b2a = mft2(3, 4, TO_INK_GRID, &to_ink);
    let paper = lab_to_xyz(PAPER);
    let tags: Vec<(&[u8; 4], usize)> = vec![
        (b"desc", 0),
        (b"cprt", 1),
        (b"wtpt", 2),
        (b"A2B0", 3),
        (b"A2B1", 3),
        (b"A2B2", 3),
        (b"B2A0", 4),
        (b"B2A1", 4),
        (b"B2A2", 4),
    ];
    let data = [desc_tag(NAME), text_tag("No copyright, use freely"), xyz_tag(paper), a2b, b2a];
    write(&tags, &data)
}

/// Lab in the 16-bit version 2 numbering (L 100 at 0xFF00, a and b 0 at 0x8000).
fn legacy_lab(lab: [f64; 3]) -> [u16; 3] {
    [lab[0] / 100.0 * 65280.0, (lab[1] + 128.0) * 256.0, (lab[2] + 128.0) * 256.0]
        .map(|v| v.round().clamp(0.0, 65535.0) as u16)
}

/// A 16-bit table pipeline: straight curves either side of the grid of `values`.
fn mft2(inputs: u8, outputs: u8, grid: usize, values: &[u16]) -> Vec<u8> {
    let mut t = b"mft2\0\0\0\0".to_vec();
    t.extend([inputs, outputs, grid as u8, 0]);
    for v in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
        t.extend(fixed(v));
    }
    t.extend(2u16.to_be_bytes());
    t.extend(2u16.to_be_bytes());
    for _ in 0..inputs {
        t.extend([0u16, 65535].iter().flat_map(|v| v.to_be_bytes()));
    }
    t.extend(values.iter().flat_map(|v| v.to_be_bytes()));
    for _ in 0..outputs {
        t.extend([0u16, 65535].iter().flat_map(|v| v.to_be_bytes()));
    }
    t
}

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

/// A version 2 text description: the ASCII text, then empty Unicode and ScriptCode parts.
fn desc_tag(s: &str) -> Vec<u8> {
    let mut t = b"desc\0\0\0\0".to_vec();
    t.extend((s.len() as u32 + 1).to_be_bytes());
    t.extend(s.bytes());
    t.push(0);
    t.extend([0u8; 8]);
    t.extend([0u8; 3]);
    t.extend([0u8; 67]);
    t
}

fn text_tag(s: &str) -> Vec<u8> {
    let mut t = b"text\0\0\0\0".to_vec();
    t.extend(s.bytes());
    t.push(0);
    t
}

/// The file: the header, the tag table, then each piece of `data` once on a 4-byte boundary
/// (tags naming the same piece share it).
fn write(tags: &[(&[u8; 4], usize)], data: &[Vec<u8>]) -> Vec<u8> {
    let table_end = 128 + 4 + 12 * tags.len();
    let mut offsets = Vec::with_capacity(data.len());
    let mut body = Vec::new();
    for d in data {
        offsets.push(table_end + body.len());
        body.extend(d);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    let size = table_end + body.len();
    let mut out = Vec::with_capacity(size);
    out.extend((size as u32).to_be_bytes());
    out.extend([0u8; 4]);
    out.extend([2, 0x10, 0, 0]);
    out.extend(b"prtrCMYKLab ");
    // made 2026-01-01
    for v in [2026u16, 1, 1, 0, 0, 0] {
        out.extend(v.to_be_bytes());
    }
    out.extend(b"acsp");
    out.extend([0u8; 24]);
    out.extend(0u32.to_be_bytes());
    for c in D50 {
        out.extend(fixed(c));
    }
    out.extend([0u8; 48]);
    out.extend((tags.len() as u32).to_be_bytes());
    for (sig, piece) in tags {
        let (offset, len) = (offsets.get(*piece).copied().unwrap_or(0), data.get(*piece).map_or(0, Vec::len));
        out.extend(*sig);
        out.extend((offset as u32).to_be_bytes());
        out.extend((len as u32).to_be_bytes());
    }
    out.extend(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cms::{IccProfile, Space};
    use crate::profile::Profile;

    /// Writes the profile to the file in `PROCRAFT_CMYK`, for trying in other colour programs:
    /// `cargo test -p procraft-engine write_cmyk -- --ignored`.
    #[test]
    #[ignore]
    fn write_cmyk() {
        let Ok(path) = std::env::var("PROCRAFT_CMYK") else { return };
        std::fs::write(path, profile()).unwrap();
    }

    #[test]
    fn the_profile_reads_back() {
        let p = IccProfile::parse(&profile()).unwrap();
        assert_eq!(p.name, NAME);
        assert_eq!(p.space, Space::Cmyk);
        assert!(p.can_receive());
        // no ink is the paper: the connection space's white
        let lab = xyz_to_lab(p.to_xyz(&[0.0; 4]));
        assert!((lab[0] - 100.0).abs() < 0.5 && lab[1].abs() < 0.5 && lab[2].abs() < 0.5, "{lab:?}");
        let ink = p.from_xyz(D50);
        assert!(ink.iter().all(|&v| v < 0.01), "white takes no ink: {ink:?}");
    }

    #[test]
    fn colours_separate_as_a_press_would() {
        let p = IccProfile::parse(&profile()).unwrap();
        let p3 = IccProfile::parse(&crate::icc::icc(Profile::DisplayP3)).unwrap();
        let ink = |rgb: [f64; 3]| p3.convert(&rgb, &p);
        // black: a rich black within the ink limit
        let black = ink([0.0, 0.0, 0.0]);
        let total: f64 = black.iter().sum();
        assert!(black[3] > 0.7 && total > 2.5 && total <= 3.05, "{black:?}");
        // a saturated green takes cyan and yellow, no magenta or black
        let green = ink([0.0, 1.0, 0.0]);
        assert!(green[0] > 0.3 && green[2] > 0.5 && green[1] < 0.05 && green[3] < 0.05, "{green:?}");
        // mid grey takes some black
        let grey = ink([0.5, 0.5, 0.5]);
        assert!(grey[3] > 0.05 && grey[3] < 0.6, "{grey:?}");
        // printed colours come back near where they went in, inside the press's range
        for rgb in [[0.6, 0.5, 0.4], [0.3, 0.4, 0.5], [0.8, 0.8, 0.8]] {
            let back = p.convert(&ink(rgb), &p3);
            for (a, b) in rgb.iter().zip(&back) {
                assert!((a - b).abs() < 0.04, "{rgb:?} {back:?}");
            }
        }
    }
}
