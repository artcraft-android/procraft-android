//! Colour management: ICC profiles read, and colours moved from one profile to another.
//!
//! A profile takes its device's colours (RGB, CMYK or grey, each 0..1) to the profile connection
//! space (CIE XYZ or Lab, D50) and back, through one of the ICC's pipelines: a matrix with tone
//! curves, a grey tone curve, 8- and 16-bit lookup tables (`mft1`, `mft2`) or the version 4 ones
//! (`mAB `, `mBA `). Colours go from one profile to another through the connection space, by the
//! perceptual tables where a profile has them (no black point compensation).

use std::sync::Arc;

/// What a profile's device colours are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Rgb,
    Cmyk,
    Gray,
}

impl Space {
    /// How many channels its colours have.
    pub fn channels(self) -> usize {
        match self {
            Space::Rgb => 3,
            Space::Cmyk => 4,
            Space::Gray => 1,
        }
    }
}

/// The connection space a profile's tables meet in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pcs {
    Xyz,
    Lab,
}

/// The connection space's white, D50.
pub const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// A tone curve over 0..1.
#[derive(Clone, Debug, PartialEq)]
enum Curve {
    Identity,
    Gamma(f64),
    /// Points evenly spread over 0..1, joined by straight lines.
    Table(Vec<f64>),
    /// An ICC parametric curve: its function type (0 to 4) and parameters g, a, b, c, d, e, f.
    Parametric(u16, [f64; 7]),
}

impl Curve {
    fn eval(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Identity => x,
            Curve::Gamma(g) => x.powf(*g),
            Curve::Table(t) => {
                let n = t.len();
                if n < 2 {
                    return t.first().copied().unwrap_or(x);
                }
                let at = x * (n - 1) as f64;
                let i = (at.floor() as usize).min(n - 2);
                let (a, b) = (t.get(i).copied().unwrap_or(0.0), t.get(i + 1).copied().unwrap_or(0.0));
                a + (b - a) * (at - i as f64)
            }
            Curve::Parametric(kind, p) => {
                let [g, a, b, c, d, e, f] = *p;
                let power = |v: f64| if v > 0.0 { v.powf(g) } else { 0.0 };
                match kind {
                    0 => power(x),
                    1 => {
                        if x >= -b / a {
                            power(a * x + b)
                        } else {
                            0.0
                        }
                    }
                    2 => {
                        if x >= -b / a {
                            power(a * x + b) + c
                        } else {
                            c
                        }
                    }
                    3 => {
                        if x >= d {
                            power(a * x + b)
                        } else {
                            c * x
                        }
                    }
                    _ => {
                        if x >= d {
                            power(a * x + b) + e
                        } else {
                            c * x + f
                        }
                    }
                }
            }
        }
    }

    /// The x the curve takes to `y`, found by halving (curves only ever rise or only ever fall).
    fn inverse(&self, y: f64) -> f64 {
        match self {
            Curve::Identity => y.clamp(0.0, 1.0),
            Curve::Gamma(g) if *g > 0.0 => y.clamp(0.0, 1.0).powf(1.0 / g),
            _ => {
                let rising = self.eval(1.0) >= self.eval(0.0);
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                for _ in 0..40 {
                    let mid = (lo + hi) / 2.0;
                    if (self.eval(mid) < y) == rising {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                (lo + hi) / 2.0
            }
        }
    }
}

/// A colour lookup table: `grid` points along each input, `outputs` values (0..1) at each point,
/// the first input varying slowest.
#[derive(Clone, Debug, PartialEq)]
struct Clut {
    grid: Vec<usize>,
    outputs: usize,
    values: Vec<f64>,
}

impl Clut {
    /// The outputs at `input` (0..1 each), interpolated between the grid's corners.
    fn eval(&self, input: &[f64]) -> Vec<f64> {
        let n = self.grid.len();
        let mut base = Vec::with_capacity(n);
        let mut frac = Vec::with_capacity(n);
        for (k, &g) in self.grid.iter().enumerate() {
            let at = input.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0) * (g.max(2) - 1) as f64;
            let i = (at.floor() as usize).min(g.max(2) - 2);
            base.push(i);
            frac.push(at - i as f64);
        }
        let mut out = vec![0.0; self.outputs];
        for corner in 0..(1usize << n) {
            let (mut weight, mut index) = (1.0, 0usize);
            for k in 0..n {
                let up = (corner >> (n - 1 - k)) & 1;
                let f = frac.get(k).copied().unwrap_or(0.0);
                weight *= if up == 1 { f } else { 1.0 - f };
                let g = self.grid.get(k).copied().unwrap_or(1);
                index = index * g + base.get(k).copied().unwrap_or(0) + up;
            }
            if weight == 0.0 {
                continue;
            }
            for (o, v) in out.iter_mut().enumerate() {
                *v += weight * self.values.get(index * self.outputs + o).copied().unwrap_or(0.0);
            }
        }
        out
    }
}

/// One step of a table pipeline.
#[derive(Clone, Debug, PartialEq)]
enum Stage {
    Curves(Vec<Curve>),
    /// Three values times the matrix, plus the offset.
    Matrix([[f64; 3]; 3], [f64; 3]),
    Clut(Clut),
}

impl Stage {
    fn eval(&self, v: Vec<f64>) -> Vec<f64> {
        match self {
            Stage::Curves(curves) => {
                v.iter().enumerate().map(|(k, &x)| curves.get(k).map_or(x, |c| c.eval(x))).collect()
            }
            Stage::Matrix(m, offset) => {
                let x = [0, 1, 2].map(|k| v.get(k).copied().unwrap_or(0.0));
                m.map(|row| dot(row, x)).iter().zip(offset).map(|(y, o)| y + o).collect()
            }
            Stage::Clut(c) => c.eval(&v),
        }
    }
}

/// How a table pipeline's connection-space end is numbered (0..1 to actual XYZ or Lab).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    /// XYZ as 0..1.99997 (1.0 at 0x8000).
    Xyz,
    /// Lab from 8-bit and version 4 tables: L 0..100, a and b −128..127.
    Lab,
    /// Lab from 16-bit version 2 tables: L 100 at 0xFF00, a and b 0 at 0x8000.
    LabLegacy,
}

impl Encoding {
    fn decode(self, v: &[f64]) -> [f64; 3] {
        let [x, y, z] = [0, 1, 2].map(|k| v.get(k).copied().unwrap_or(0.0));
        match self {
            Encoding::Xyz => [x, y, z].map(|c| c * 65535.0 / 32768.0),
            Encoding::Lab => [x * 100.0, y * 255.0 - 128.0, z * 255.0 - 128.0],
            Encoding::LabLegacy => {
                [x * 65535.0 / 65280.0 * 100.0, y * 65535.0 / 256.0 - 128.0, z * 65535.0 / 256.0 - 128.0]
            }
        }
    }

    fn encode(self, c: [f64; 3]) -> Vec<f64> {
        let [x, y, z] = c;
        let v = match self {
            Encoding::Xyz => [x, y, z].map(|c| c * 32768.0 / 65535.0),
            Encoding::Lab => [x / 100.0, (y + 128.0) / 255.0, (z + 128.0) / 255.0],
            Encoding::LabLegacy => {
                [x / 100.0 * 65280.0 / 65535.0, (y + 128.0) * 256.0 / 65535.0, (z + 128.0) * 256.0 / 65535.0]
            }
        };
        v.map(|c| c.clamp(0.0, 1.0)).to_vec()
    }
}

/// A way from device colours to the connection space, or back.
#[derive(Clone, Debug, PartialEq)]
enum Pipe {
    /// RGB: the tone curves, then the matrix to XYZ.
    Matrix([Curve; 3], [[f64; 3]; 3]),
    /// Grey: the tone curve gives Y (L* for a Lab profile).
    Gray(Curve),
    /// A table pipeline, its connection-space end numbered as `Encoding` says.
    Table(Vec<Stage>, Encoding),
}

/// An ICC profile as read: what its device colours are and how they reach the connection space.
#[derive(Clone, Debug)]
pub struct IccProfile {
    /// Its description, as lists show it.
    pub name: String,
    pub space: Space,
    pcs: Pcs,
    to_pcs: Pipe,
    from_pcs: Option<Pipe>,
    /// The file, to embed where the profile goes.
    pub bytes: Arc<Vec<u8>>,
}

impl PartialEq for IccProfile {
    fn eq(&self, other: &IccProfile) -> bool {
        self.bytes == other.bytes
    }
}

/// A built-in profile as an ICC profile, made once.
pub fn builtin(profile: crate::profile::Profile) -> Arc<IccProfile> {
    use crate::profile::Profile;
    static MADE: std::sync::OnceLock<Vec<(Profile, Arc<IccProfile>)>> = std::sync::OnceLock::new();
    let made = MADE.get_or_init(|| {
        Profile::ALL
            .iter()
            .filter_map(|&p| IccProfile::parse(&crate::icc::icc(p)).map(|icc| (p, Arc::new(icc))))
            .collect()
    });
    made.iter().find(|(p, _)| *p == profile).map(|(_, icc)| icc.clone()).unwrap_or_else(|| {
        // every built-in profile reads back (a test checks): never reached
        Arc::new(IccProfile {
            name: profile.name().to_owned(),
            space: Space::Rgb,
            pcs: Pcs::Xyz,
            to_pcs: Pipe::Matrix(
                [Curve::Identity, Curve::Identity, Curve::Identity],
                [[0.4361, 0.3851, 0.1431], [0.2225, 0.7169, 0.0606], [0.0139, 0.0971, 0.7141]],
            ),
            from_pcs: None,
            bytes: Arc::new(Vec::new()),
        })
    })
}

/// A canvas's own profile (one it was given, not a built-in one), kept with it as base64 text.
#[derive(Clone, Debug)]
pub struct CanvasIcc(pub Arc<IccProfile>);

impl PartialEq for CanvasIcc {
    fn eq(&self, other: &CanvasIcc) -> bool {
        self.0.bytes == other.0.bytes
    }
}

impl serde::Serialize for CanvasIcc {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::base64::encode(&self.0.bytes))
    }
}

impl<'de> serde::Deserialize<'de> for CanvasIcc {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<CanvasIcc, D::Error> {
        let text = String::deserialize(d)?;
        crate::base64::decode(&text)
            .and_then(|bytes| IccProfile::parse(&bytes))
            .map(|p| CanvasIcc(Arc::new(p)))
            .ok_or_else(|| serde::de::Error::custom("not a colour profile"))
    }
}

/// A canvas's own profile as kept, or none if it can't be read (the canvas falls back to its
/// built-in profile rather than failing to open).
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<CanvasIcc>, D::Error> {
    let text: Option<String> = serde::Deserialize::deserialize(d)?;
    Ok(text.and_then(|t| crate::base64::decode(&t)).and_then(|b| IccProfile::parse(&b)).map(|p| CanvasIcc(Arc::new(p))))
}

/// Big-endian reads from a profile's bytes; none past the end.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn u8(&self, at: usize) -> Option<u8> {
        self.0.get(at).copied()
    }
    fn u16(&self, at: usize) -> Option<u16> {
        Some(u16::from_be_bytes(self.0.get(at..at + 2)?.try_into().ok()?))
    }
    fn u32(&self, at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(self.0.get(at..at + 4)?.try_into().ok()?))
    }
    fn s15(&self, at: usize) -> Option<f64> {
        Some(f64::from(i32::from_be_bytes(self.0.get(at..at + 4)?.try_into().ok()?)) / 65536.0)
    }
    fn sig(&self, at: usize) -> Option<[u8; 4]> {
        self.0.get(at..at + 4)?.try_into().ok()
    }
}

impl IccProfile {
    /// Reads a profile; none if it isn't one, or is a kind this can't use (only RGB, CMYK and grey
    /// profiles with a way to the connection space are).
    pub fn parse(bytes: &[u8]) -> Option<IccProfile> {
        let b = Bytes(bytes);
        if bytes.len() < 132 || &b.sig(36)? != b"acsp" {
            return None;
        }
        let space = match &b.sig(16)? {
            b"RGB " => Space::Rgb,
            b"CMYK" => Space::Cmyk,
            b"GRAY" => Space::Gray,
            _ => return None,
        };
        let pcs = match &b.sig(20)? {
            b"XYZ " => Pcs::Xyz,
            b"Lab " => Pcs::Lab,
            _ => return None,
        };
        let count = b.u32(128)? as usize;
        let mut tags = std::collections::HashMap::new();
        for i in 0..count.min(200) {
            let at = 132 + 12 * i;
            let (sig, offset, size) = (b.sig(at)?, b.u32(at + 4)? as usize, b.u32(at + 8)? as usize);
            if offset.checked_add(size).is_some_and(|end| end <= bytes.len()) {
                tags.insert(sig, (offset, size));
            }
        }
        let tag = |name: &[u8; 4]| tags.get(name).copied();
        let name = tag(b"desc").and_then(|(o, s)| text(&b, o, s)).unwrap_or_default();
        let table = |names: [&[u8; 4]; 3], inputs: usize, outputs: usize, to_pcs: bool| {
            names.iter().find_map(|n| tag(n).and_then(|(o, _)| read_table(&b, o, inputs, outputs, to_pcs, pcs)))
        };
        let n = space.channels();
        let to_pcs = match table([b"A2B0", b"A2B1", b"A2B2"], n, 3, true) {
            Some(p) => p,
            None => match space {
                Space::Rgb => {
                    let curves = [b"rTRC", b"gTRC", b"bTRC"].map(|t| tag(t).and_then(|(o, _)| read_curve(&b, o)));
                    let columns = [b"rXYZ", b"gXYZ", b"bXYZ"]
                        .map(|t| tag(t).and_then(|(o, _)| Some([b.s15(o + 8)?, b.s15(o + 12)?, b.s15(o + 16)?])));
                    let [Some(r), Some(g), Some(bl)] = curves else { return None };
                    let [Some(cr), Some(cg), Some(cb)] = columns else { return None };
                    Pipe::Matrix([r, g, bl], [[cr[0], cg[0], cb[0]], [cr[1], cg[1], cb[1]], [cr[2], cg[2], cb[2]]])
                }
                Space::Gray => Pipe::Gray(tag(b"kTRC").and_then(|(o, _)| read_curve(&b, o))?),
                Space::Cmyk => return None,
            },
        };
        let from_pcs = table([b"B2A0", b"B2A1", b"B2A2"], 3, n, false);
        Some(IccProfile { name, space, pcs, to_pcs, from_pcs, bytes: Arc::new(bytes.to_vec()) })
    }

    /// `device` (0..1 each) in the connection space, as D50 XYZ (white's Y 1).
    pub fn to_xyz(&self, device: &[f64]) -> [f64; 3] {
        let pcs = match &self.to_pcs {
            Pipe::Matrix(curves, m) => {
                let mut lin = [0.0; 3];
                for (k, (l, c)) in lin.iter_mut().zip(curves).enumerate() {
                    *l = c.eval(device.get(k).copied().unwrap_or(0.0));
                }
                return m.map(|row| dot(row, lin));
            }
            Pipe::Gray(curve) => {
                let y = curve.eval(device.first().copied().unwrap_or(0.0));
                return match self.pcs {
                    Pcs::Xyz => D50.map(|w| w * y),
                    Pcs::Lab => lab_to_xyz([y * 100.0, 0.0, 0.0]),
                };
            }
            Pipe::Table(stages, encoding) => {
                let v = stages.iter().fold(device.to_vec(), |v, s| s.eval(v));
                encoding.decode(&v)
            }
        };
        match self.pcs {
            Pcs::Xyz => pcs,
            Pcs::Lab => lab_to_xyz(pcs),
        }
    }

    /// The device colour (0..1 each) for D50 `xyz`, clipped to what the device can show.
    pub fn from_xyz(&self, xyz: [f64; 3]) -> Vec<f64> {
        if let Some(Pipe::Table(stages, encoding)) = &self.from_pcs {
            let pcs = match self.pcs {
                Pcs::Xyz => xyz,
                Pcs::Lab => xyz_to_lab(xyz),
            };
            return stages
                .iter()
                .fold(encoding.encode(pcs), |v, s| s.eval(v))
                .iter()
                .map(|c| c.clamp(0.0, 1.0))
                .collect();
        }
        match &self.to_pcs {
            Pipe::Matrix(curves, m) => {
                let lin = invert(*m).map(|row| dot(row, xyz));
                lin.iter().zip(curves).map(|(&l, c)| c.inverse(l.clamp(0.0, 1.0))).collect()
            }
            Pipe::Gray(curve) => {
                let y = match self.pcs {
                    Pcs::Xyz => xyz[1],
                    Pcs::Lab => xyz_to_lab(xyz)[0] / 100.0,
                };
                vec![curve.inverse(y.clamp(0.0, 1.0))]
            }
            // a table one way and none back: the nearest device colour by searching
            Pipe::Table(..) => vec![0.0; self.space.channels()],
        }
    }

    /// Whether colours can be turned into this profile's (some input-only profiles can't).
    pub fn can_receive(&self) -> bool {
        self.from_pcs.is_some() || !matches!(self.to_pcs, Pipe::Table(..))
    }

    /// `device` in this profile, moved to `to`'s.
    pub fn convert(&self, device: &[f64], to: &IccProfile) -> Vec<f64> {
        to.from_xyz(self.to_xyz(device))
    }
}

/// The text of a `desc`, `mluc` or `text` tag (English if there is a choice).
fn text(b: &Bytes, at: usize, size: usize) -> Option<String> {
    match &b.sig(at)? {
        b"desc" => {
            let n = b.u32(at + 8)? as usize;
            let raw = b.0.get(at + 12..at + 12 + n.min(size))?;
            Some(String::from_utf8_lossy(raw).trim_end_matches('\0').trim().to_owned())
        }
        b"mluc" => {
            let records = b.u32(at + 8)? as usize;
            let pick = (0..records)
                .find(|&r| b.0.get(at + 16 + 12 * r..at + 18 + 12 * r) == Some(b"en".as_slice()))
                .unwrap_or(0);
            let rec = at + 16 + 12 * pick;
            let (len, off) = (b.u32(rec + 4)? as usize, b.u32(rec + 8)? as usize);
            let raw = b.0.get(at + off..at + off + len)?;
            let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
            Some(String::from_utf16_lossy(&units).trim_end_matches('\0').trim().to_owned())
        }
        b"text" => {
            let raw = b.0.get(at + 8..at + size)?;
            Some(String::from_utf8_lossy(raw).trim_end_matches('\0').trim().to_owned())
        }
        _ => None,
    }
}

/// A `curv` or `para` curve at `at`, and the bytes it takes (to the next 4-byte boundary).
fn read_curve_sized(b: &Bytes, at: usize) -> Option<(Curve, usize)> {
    match &b.sig(at)? {
        b"curv" => {
            let n = b.u32(at + 8)? as usize;
            let curve = match n {
                0 => Curve::Identity,
                1 => Curve::Gamma(f64::from(b.u16(at + 12)?) / 256.0),
                _ => Curve::Table(
                    (0..n.min(65536))
                        .map(|i| Some(f64::from(b.u16(at + 12 + 2 * i)?) / 65535.0))
                        .collect::<Option<_>>()?,
                ),
            };
            Some((curve, (12 + 2 * n).next_multiple_of(4)))
        }
        b"para" => {
            let kind = b.u16(at + 8)?;
            let count = match kind {
                0 => 1,
                1 => 3,
                2 => 4,
                3 => 5,
                4 => 7,
                _ => return None,
            };
            let mut p = [0.0; 7];
            for (i, v) in p.iter_mut().enumerate().take(count) {
                *v = b.s15(at + 12 + 4 * i)?;
            }
            Some((Curve::Parametric(kind, p), 12 + 4 * count))
        }
        _ => None,
    }
}

fn read_curve(b: &Bytes, at: usize) -> Option<Curve> {
    read_curve_sized(b, at).map(|(c, _)| c)
}

/// `count` curves one after another from `at`.
fn read_curves(b: &Bytes, mut at: usize, count: usize) -> Option<Vec<Curve>> {
    let mut curves = Vec::with_capacity(count);
    for _ in 0..count {
        let (c, size) = read_curve_sized(b, at)?;
        curves.push(c);
        at += size;
    }
    Some(curves)
}

/// A table pipeline tag (`mft1`, `mft2`, `mAB `, `mBA `) at `at` with `inputs` and `outputs`
/// channels: towards the connection space if `to_pcs`.
fn read_table(b: &Bytes, at: usize, inputs: usize, outputs: usize, to_pcs: bool, pcs: Pcs) -> Option<Pipe> {
    let kind = b.sig(at)?;
    let (ins, outs) = (b.u8(at + 8)? as usize, b.u8(at + 9)? as usize);
    if ins != inputs || outs != outputs {
        return None;
    }
    match &kind {
        b"mft1" | b"mft2" => {
            let wide = &kind == b"mft2";
            let grid = b.u8(at + 10)? as usize;
            let m = [0, 1, 2].map(|r| [0, 1, 2].map(|c| b.s15(at + 12 + 4 * (3 * r + c)).unwrap_or(0.0)));
            let (in_entries, out_entries, mut p) =
                if wide { (b.u16(at + 48)? as usize, b.u16(at + 50)? as usize, at + 52) } else { (256, 256, at + 48) };
            let width = if wide { 2 } else { 1 };
            let value = |p: usize| -> Option<f64> {
                if wide { Some(f64::from(b.u16(p)?) / 65535.0) } else { Some(f64::from(b.u8(p)?) / 255.0) }
            };
            let tables = |count: usize, entries: usize, p: &mut usize| -> Option<Vec<Curve>> {
                let mut curves = Vec::with_capacity(count);
                for _ in 0..count {
                    let t = (0..entries).map(|i| value(*p + width * i)).collect::<Option<Vec<_>>>()?;
                    *p += width * entries;
                    curves.push(Curve::Table(t));
                }
                Some(curves)
            };
            let input = tables(ins, in_entries, &mut p)?;
            let points = grid.checked_pow(ins as u32)?.checked_mul(outs)?;
            let values = (0..points).map(|i| value(p + width * i)).collect::<Option<Vec<_>>>()?;
            p += width * points;
            let output = tables(outs, out_entries, &mut p)?;
            let mut stages = Vec::new();
            // the matrix is used only on XYZ input
            if !to_pcs && pcs == Pcs::Xyz && m != [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
                stages.push(Stage::Matrix(m, [0.0; 3]));
            }
            stages.push(Stage::Curves(input));
            stages.push(Stage::Clut(Clut { grid: vec![grid; ins], outputs: outs, values }));
            stages.push(Stage::Curves(output));
            let encoding = match pcs {
                Pcs::Xyz => Encoding::Xyz,
                Pcs::Lab if wide => Encoding::LabLegacy,
                Pcs::Lab => Encoding::Lab,
            };
            Some(Pipe::Table(stages, encoding))
        }
        b"mAB " | b"mBA " => {
            let a_to_b = &kind == b"mAB ";
            let offset = |k: usize| b.u32(at + 12 + 4 * k).map(|o| o as usize);
            let (b_off, m_off, mid_off, clut_off, a_off) = (offset(0)?, offset(1)?, offset(2)?, offset(3)?, offset(4)?);
            // A curves face the device, B curves the connection space
            let (a_count, b_count) = if a_to_b { (ins, outs) } else { (outs, ins) };
            let b_curves = if b_off > 0 { Some(read_curves(b, at + b_off, b_count)?) } else { None };
            let a_curves = if a_off > 0 { Some(read_curves(b, at + a_off, a_count)?) } else { None };
            let m_curves = if mid_off > 0 { Some(read_curves(b, at + mid_off, 3)?) } else { None };
            let matrix = if m_off > 0 {
                let v = |k: usize| b.s15(at + m_off + 4 * k);
                Some(([[v(0)?, v(1)?, v(2)?], [v(3)?, v(4)?, v(5)?], [v(6)?, v(7)?, v(8)?]], [v(9)?, v(10)?, v(11)?]))
            } else {
                None
            };
            let clut = if clut_off > 0 {
                let c = at + clut_off;
                let grid: Vec<usize> = (0..ins.min(15)).map(|k| b.u8(c + k).map(usize::from)).collect::<Option<_>>()?;
                let precision = b.u8(c + 16)?;
                let points = grid.iter().try_fold(1usize, |acc, &g| acc.checked_mul(g))?.checked_mul(outs)?;
                let values = (0..points)
                    .map(|i| match precision {
                        1 => b.u8(c + 20 + i).map(|v| f64::from(v) / 255.0),
                        _ => b.u16(c + 20 + 2 * i).map(|v| f64::from(v) / 65535.0),
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(Clut { grid, outputs: outs, values })
            } else {
                None
            };
            let mut stages = Vec::new();
            let push_curves = |c: Option<Vec<Curve>>, stages: &mut Vec<Stage>| {
                if let Some(c) = c {
                    stages.push(Stage::Curves(c));
                }
            };
            if a_to_b {
                push_curves(a_curves, &mut stages);
                if let Some(c) = clut {
                    stages.push(Stage::Clut(c));
                }
                push_curves(m_curves, &mut stages);
                if let Some((m, o)) = matrix {
                    stages.push(Stage::Matrix(m, o));
                }
                push_curves(b_curves, &mut stages);
            } else {
                push_curves(b_curves, &mut stages);
                if let Some((m, o)) = matrix {
                    stages.push(Stage::Matrix(m, o));
                }
                push_curves(m_curves, &mut stages);
                if let Some(c) = clut {
                    stages.push(Stage::Clut(c));
                }
                push_curves(a_curves, &mut stages);
            }
            let encoding = match pcs {
                Pcs::Xyz => Encoding::Xyz,
                Pcs::Lab => Encoding::Lab,
            };
            Some(Pipe::Table(stages, encoding))
        }
        _ => None,
    }
}

fn dot(row: [f64; 3], v: [f64; 3]) -> f64 {
    row[0] * v[0] + row[1] * v[1] + row[2] * v[2]
}

fn invert(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-12 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    [
        [(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
        [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
        [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det],
    ]
}

/// A space's white: full RGB or grey, or no ink.
fn white(space: Space) -> Vec<f32> {
    match space {
        Space::Rgb => vec![1.0; 3],
        Space::Gray => vec![1.0],
        Space::Cmyk => vec![0.0; 4],
    }
}

/// D50 XYZ to CIE Lab.
pub fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let f =
        |t: f64| if t > (6.0f64 / 29.0).powi(3) { t.cbrt() } else { t / (3.0 * (6.0f64 / 29.0).powi(2)) + 4.0 / 29.0 };
    let [x, y, z] = xyz;
    let [fx, fy, fz] = [f(x / D50[0]), f(y / D50[1]), f(z / D50[2])];
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIE Lab to D50 XYZ.
pub fn lab_to_xyz(lab: [f64; 3]) -> [f64; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let [fx, fz] = [fy + lab[1] / 500.0, fy - lab[2] / 200.0];
    let inv = |t: f64| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f64 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    [inv(fx) * D50[0], inv(fy) * D50[1], inv(fz) * D50[2]]
}

/// Colours of one RGB profile moved to another profile's, as a table over the RGB cube:
/// `grid` points along each channel, tetrahedral interpolation between them.
#[derive(Clone, Debug, PartialEq)]
pub struct Lut {
    pub grid: usize,
    /// How many values each point holds (the target's channels).
    pub outputs: usize,
    /// Red varying slowest, then green, then blue.
    pub values: Vec<f32>,
}

impl Lut {
    /// The table taking `from`'s RGB to `to`'s colours, through `via` on the way if given (the
    /// colours a press would print, as `to` shows them).
    pub fn build(from: &IccProfile, to: &IccProfile, via: Option<&IccProfile>, grid: usize) -> Lut {
        let grid = grid.clamp(2, 65);
        let outputs = to.space.channels();
        let mut values = Vec::with_capacity(grid * grid * grid * outputs);
        let step = 1.0 / (grid - 1) as f64;
        for r in 0..grid {
            for g in 0..grid {
                for b in 0..grid {
                    // white stays white exactly (paper with no ink), whatever rounding the tables do
                    if r == grid - 1 && g == grid - 1 && b == grid - 1 {
                        values.extend(white(to.space));
                        continue;
                    }
                    let rgb = [r as f64 * step, g as f64 * step, b as f64 * step];
                    let mut xyz = from.to_xyz(&rgb);
                    if let Some(press) = via {
                        xyz = press.to_xyz(&press.from_xyz(xyz));
                    }
                    values.extend(to.from_xyz(xyz).iter().map(|&v| v as f32));
                }
            }
        }
        Lut { grid, outputs, values }
    }

    /// The colour for `rgb` (0..1 each).
    pub fn apply(&self, rgb: [f32; 3]) -> Vec<f32> {
        let n = self.grid.max(2);
        let scale = (n - 1) as f32;
        let pos = rgb.map(|c| c.clamp(0.0, 1.0) * scale);
        let base = pos.map(|p| (p.floor() as usize).min(n - 2));
        let [fr, fg, fb] = [pos[0] - base[0] as f32, pos[1] - base[1] as f32, pos[2] - base[2] as f32];
        let at = |dr: usize, dg: usize, db: usize, o: usize| -> f32 {
            let i = ((base[0] + dr) * n + base[1] + dg) * n + base[2] + db;
            self.values.get(i * self.outputs + o).copied().unwrap_or(0.0)
        };
        (0..self.outputs)
            .map(|o| {
                let c000 = at(0, 0, 0, o);
                let c111 = at(1, 1, 1, o);
                // the tetrahedron holding the point, by the order of its fractions
                if fr >= fg {
                    if fg >= fb {
                        let (c100, c110) = (at(1, 0, 0, o), at(1, 1, 0, o));
                        c000 + fr * (c100 - c000) + fg * (c110 - c100) + fb * (c111 - c110)
                    } else if fr >= fb {
                        let (c100, c101) = (at(1, 0, 0, o), at(1, 0, 1, o));
                        c000 + fr * (c100 - c000) + fb * (c101 - c100) + fg * (c111 - c101)
                    } else {
                        let (c001, c101) = (at(0, 0, 1, o), at(1, 0, 1, o));
                        c000 + fb * (c001 - c000) + fr * (c101 - c001) + fg * (c111 - c101)
                    }
                } else if fb >= fg {
                    let (c001, c011) = (at(0, 0, 1, o), at(0, 1, 1, o));
                    c000 + fb * (c001 - c000) + fg * (c011 - c001) + fr * (c111 - c011)
                } else if fb >= fr {
                    let (c010, c011) = (at(0, 1, 0, o), at(0, 1, 1, o));
                    c000 + fg * (c010 - c000) + fb * (c011 - c010) + fr * (c111 - c011)
                } else {
                    let (c010, c110) = (at(0, 1, 0, o), at(1, 1, 0, o));
                    c000 + fg * (c010 - c000) + fr * (c110 - c010) + fb * (c111 - c110)
                }
            })
            .collect()
    }

    /// 8-bit RGBA pixels (straight or premultiplied, as `premultiplied` says) moved through the
    /// table, alpha kept; a target with other than three channels leaves them unchanged.
    pub fn apply_rgba(&self, pixels: &mut [u8], premultiplied: bool) {
        if self.outputs != 3 {
            return;
        }
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = f32::from(px[3]) / 255.0;
            if a == 0.0 {
                continue;
            }
            let un = |c: u8| if premultiplied { (f32::from(c) / 255.0 / a).min(1.0) } else { f32::from(c) / 255.0 };
            let out = self.apply([un(px[0]), un(px[1]), un(px[2])]);
            for (c, v) in px.iter_mut().zip(&out).take(3) {
                let v = if premultiplied { v * a } else { *v };
                *c = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;

    fn ours(p: Profile) -> IccProfile {
        IccProfile::parse(&crate::icc::icc(p)).unwrap()
    }

    #[test]
    fn our_own_profiles_read_back() {
        let srgb = ours(Profile::Srgb);
        assert_eq!(srgb.name, "sRGB IEC61966-2.1");
        assert_eq!(srgb.space, Space::Rgb);
        // white is the connection space's white, and comes back
        let white = srgb.to_xyz(&[1.0, 1.0, 1.0]);
        for (w, d) in white.iter().zip(D50) {
            assert!((w - d).abs() < 2e-3, "{white:?}");
        }
        for rgb in [[0.2, 0.5, 0.9], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.7, 0.7, 0.7]] {
            let back = srgb.from_xyz(srgb.to_xyz(&rgb));
            for (a, b) in rgb.iter().zip(&back) {
                assert!((a - b).abs() < 1e-3, "{rgb:?} {back:?}");
            }
        }
    }

    #[test]
    fn moving_between_profiles_matches_the_matrices() {
        let (srgb, p3) = (ours(Profile::Srgb), ours(Profile::DisplayP3));
        for rgb in [[0.0f32, 1.0, 0.0], [0.8, 0.3, 0.1], [0.5, 0.5, 0.5]] {
            let want = Profile::Srgb.convert(rgb, Profile::DisplayP3);
            let got = srgb.convert(&rgb.map(f64::from), &p3);
            for (w, g) in want.iter().zip(&got) {
                assert!((f64::from(*w) - g).abs() < 2e-3, "{rgb:?}: {want:?} {got:?}");
            }
        }
    }

    #[test]
    fn a_table_matches_the_profiles_it_was_made_from() {
        let (srgb, p3) = (ours(Profile::Srgb), ours(Profile::DisplayP3));
        let lut = Lut::build(&srgb, &p3, None, 33);
        for rgb in [[0.0f32, 1.0, 0.0], [0.13, 0.62, 0.91], [0.5, 0.25, 0.75]] {
            let want = srgb.convert(&rgb.map(f64::from), &p3);
            let got = lut.apply(rgb);
            for (w, g) in want.iter().zip(&got) {
                assert!((w - f64::from(*g)).abs() < 3e-3, "{rgb:?}: {want:?} {got:?}");
            }
        }
        let mut px = [0, 255, 0, 255, 10, 20, 30, 0];
        lut.apply_rgba(&mut px, false);
        assert_eq!(px[4..], [10, 20, 30, 0], "clear pixels stay as they are");
        assert!(px[0] > 50 && px[1] > 240 && px[2] > 50 && px[2] < 100, "{px:?}");
    }

    #[test]
    fn lab_goes_both_ways() {
        for lab in [[50.0, 20.0, -30.0], [90.0, -5.0, 80.0], [5.0, 0.0, 0.0]] {
            let back = xyz_to_lab(lab_to_xyz(lab));
            for (a, b) in lab.iter().zip(back) {
                assert!((a - b).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn every_built_in_profile_reads_back() {
        for p in Profile::ALL {
            let icc = builtin(p);
            assert_eq!(icc.name, p.name());
            assert!(!icc.bytes.is_empty());
        }
    }

    #[test]
    fn a_canvas_profile_keeps_as_text() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Kept {
            #[serde(default, deserialize_with = "lenient")]
            icc: Option<CanvasIcc>,
        }
        let icc = CanvasIcc(builtin(Profile::DisplayP3));
        let text = serde_json::to_string(&Kept { icc: Some(icc.clone()) }).unwrap();
        assert_eq!(serde_json::from_str::<Kept>(&text).unwrap().icc, Some(icc));
        assert_eq!(serde_json::from_str::<Kept>(r#"{"icc":"bm90IGEgcHJvZmlsZQ=="}"#).unwrap().icc, None);
        assert_eq!(serde_json::from_str::<Kept>("{}").unwrap().icc, None);
    }

    #[test]
    fn other_files_are_not_profiles() {
        assert!(IccProfile::parse(b"not a profile").is_none());
        assert!(IccProfile::parse(&[0; 200]).is_none());
        let mut lab = crate::icc::icc(Profile::Srgb);
        lab[16..20].copy_from_slice(b"Lab ");
        assert!(IccProfile::parse(&lab).is_none(), "a Lab device space isn't one we use");
    }

    /// Prints how the profile in `PROCRAFT_ICC` turns a few Display P3 colours, for comparing with
    /// other colour programs: `cargo test -p procraft-engine cms -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_conversions() {
        let Ok(path) = std::env::var("PROCRAFT_ICC") else { return };
        let target = IccProfile::parse(&std::fs::read(path).unwrap()).unwrap();
        let p3 = ours(Profile::DisplayP3);
        println!("{} {:?}", target.name, target.space);
        for rgb in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.2, 0.4, 0.8]] {
            let device = p3.convert(&rgb, &target);
            let back = target.convert(&device, &p3);
            let bytes = |v: &[f64]| v.iter().map(|c| (c * 255.0).round() as u8).collect::<Vec<_>>();
            println!("{rgb:?} -> {:?} -> {:?}", bytes(&device), bytes(&back));
        }
    }
}
