//! Reading the brush tips in Photoshop brush files (ABR): each sampled tip's grey image (white
//! paints), with its preset's name and spacing where the file gives them. Versions 1 and 2 (the
//! tips one after another) and 6 and later (a section of samples and a descriptor naming them) are
//! read; computed (round) tips have no image and are left out.

/// One brush tip from the file.
#[derive(Debug, PartialEq)]
pub struct AbrTip {
    pub name: String,
    /// Space between stamps, as a share of the tip's size, if the file gives it.
    pub spacing: Option<f32>,
    pub width: u32,
    pub height: u32,
    /// Grey, `width` x `height`, rows top to bottom: how much each pixel paints.
    pub pixels: Vec<u8>,
}

/// The largest tip side read, pixels.
const MOST_SIDE: u32 = 5000;

/// The sampled tips in an ABR file, in the file's order; none if it isn't one.
pub fn read(bytes: &[u8]) -> Option<Vec<AbrTip>> {
    let mut r = Reader::new(bytes);
    let version = r.u16()?;
    match version {
        1 | 2 => read_old(&mut r, version),
        6 | 7 | 10 => read_sections(&mut r),
        _ => None,
    }
}

/// Versions 1 and 2: a count, then each tip: its kind, its length and (sampled) its image.
fn read_old(r: &mut Reader<'_>, version: u16) -> Option<Vec<AbrTip>> {
    let count = r.u16()?;
    let mut tips = Vec::new();
    for n in 1..=count {
        let kind = r.u16()?;
        let len = r.u32()? as usize;
        let body = r.take(len)?;
        if kind != 2 {
            continue;
        }
        let mut b = Reader::new(body);
        b.u32()?;
        let spacing = b.u16()?;
        let name = if version == 2 { b.unicode()? } else { String::new() };
        b.u8()?;
        // a short rectangle, then the one used
        b.take(8)?;
        let (top, left, bottom, right) = (b.i32()?, b.i32()?, b.i32()?, b.i32()?);
        let depth = b.u16()?;
        let compression = b.u8()?;
        if let Some(pixels) = image(&mut b, top, left, bottom, right, depth, compression) {
            tips.push(AbrTip {
                name: if name.trim().is_empty() { format!("Brush {n}") } else { name },
                spacing: (spacing > 0).then(|| f32::from(spacing) / 100.0),
                width: span(left, right)?,
                height: span(top, bottom)?,
                pixels,
            });
        }
    }
    Some(tips)
}

/// Version 6 on: "8BIM" sections; "samp" holds the tips by id, "desc" their presets' names and
/// spacing.
fn read_sections(r: &mut Reader<'_>) -> Option<Vec<AbrTip>> {
    r.u16()?;
    let mut samples: Vec<(String, u32, u32, Vec<u8>)> = Vec::new();
    let mut presets: Vec<Preset> = Vec::new();
    while r.remaining() >= 12 {
        if r.take(4)? != b"8BIM" {
            break;
        }
        let key = r.take(4)?;
        let len = r.u32()? as usize;
        // a section cut short ends the file
        let Some(data) = r.take(len) else { break };
        match key {
            b"samp" => samples = read_samples(data),
            b"desc" => presets = read_presets(data),
            _ => {}
        }
    }
    let mut tips = Vec::new();
    for (n, (id, width, height, pixels)) in samples.into_iter().enumerate() {
        let preset = presets.iter().find(|p| p.sample == id);
        tips.push(AbrTip {
            name: preset
                .map(|p| p.name.clone())
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| format!("Brush {}", n + 1)),
            spacing: preset.and_then(|p| p.spacing),
            width,
            height,
            pixels,
        });
    }
    Some(tips)
}

/// The samples: each its id and its image (the one channel written of its array).
fn read_samples(data: &[u8]) -> Vec<(String, u32, u32, Vec<u8>)> {
    let mut r = Reader::new(data);
    let mut out = Vec::new();
    while r.remaining() >= 4 {
        let Some(len) = r.u32() else { break };
        // samples are padded to a multiple of four
        let padded = (len as usize).div_ceil(4) * 4;
        let Some(body) = r.take(padded.min(r.remaining())) else { break };
        if let Some(sample) = read_sample(body.get(..len as usize).unwrap_or(body)) {
            out.push(sample);
        }
    }
    out
}

fn read_sample(body: &[u8]) -> Option<(String, u32, u32, Vec<u8>)> {
    let mut b = Reader::new(body);
    let n = usize::from(b.u8()?);
    let id = String::from_utf8_lossy(b.take(n)?).into_owned();
    b.take(4)?;
    // an array of channels: its version, length and rectangle, then each channel
    b.u32()?;
    b.u32()?;
    b.take(16)?;
    let channels = b.u32()?;
    for _ in 0..channels.min(64) {
        if b.u32()? == 0 {
            continue;
        }
        let len = b.u32()? as usize;
        if len == 0 {
            continue;
        }
        let mut c = Reader::new(b.take(len)?);
        c.u32()?;
        let (top, left, bottom, right) = (c.i32()?, c.i32()?, c.i32()?, c.i32()?);
        let depth = c.u16()?;
        let compression = c.u8()?;
        let pixels = image(&mut c, top, left, bottom, right, depth, compression)?;
        return Some((id, span(left, right)?, span(top, bottom)?, pixels));
    }
    None
}

/// A tip's image: raw or packed (each row's length first), 8 or 16 bits; read as 8.
fn image(
    r: &mut Reader<'_>,
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    depth: u16,
    compression: u8,
) -> Option<Vec<u8>> {
    let (w, h) = (span(left, right)? as usize, span(top, bottom)? as usize);
    let sample = match depth {
        8 => 1,
        16 => 2,
        _ => return None,
    };
    let row = w * sample;
    let raw = match compression {
        0 => r.take(row * h)?.to_vec(),
        1 => {
            let counts: Vec<usize> = (0..h).map(|_| r.u16().map(usize::from)).collect::<Option<_>>()?;
            let mut out = Vec::with_capacity(row * h);
            for c in counts {
                unpack(r.take(c)?, row, &mut out);
            }
            out
        }
        _ => return None,
    };
    Some(if sample == 2 { raw.as_chunks::<2>().0.iter().map(|&[hi, _]| hi).collect() } else { raw })
}

/// A rectangle's side, if it is a sensible one.
fn span(from: i32, to: i32) -> Option<u32> {
    u32::try_from(i64::from(to) - i64::from(from)).ok().filter(|&n| n > 0 && n <= MOST_SIDE)
}

/// PackBits: a row of `row` bytes unpacked onto `out` (short rows filled with zeros).
fn unpack(packed: &[u8], row: usize, out: &mut Vec<u8>) {
    let end = out.len() + row;
    let mut i = packed.iter();
    while out.len() < end {
        let Some(&n) = i.next() else { break };
        let n = n as i8;
        if n >= 0 {
            for _ in 0..=n {
                out.push(i.next().copied().unwrap_or(0));
            }
        } else if n != -128 {
            let v = i.next().copied().unwrap_or(0);
            out.extend(std::iter::repeat_n(v, usize::from(n.unsigned_abs()) + 1));
        }
    }
    out.resize(end, 0);
}

/// A preset as the descriptor names it: its name, its sample's id, its spacing.
struct Preset {
    name: String,
    sample: String,
    spacing: Option<f32>,
}

/// The presets in the descriptor, found by their keys in order: a preset's name comes first,
/// then (for a sampled tip) its spacing and its sample's id.
fn read_presets(data: &[u8]) -> Vec<Preset> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut spacing = None;
    let mut at = 0;
    while let Some(i) = find(data, at, &[b"brushPreset", b"Nm  TEXT", b"SpcnUntF#Prc", b"sampledDataTEXT"]) {
        let (key, from) = i;
        at = from;
        let mut r = Reader::new(data.get(from..).unwrap_or_default());
        match key {
            // a new preset: its name is the next one
            0 => {
                name = None;
                spacing = None;
            }
            1 => {
                if name.is_none() {
                    name = r.unicode();
                }
            }
            2 => spacing = r.f64().map(|p| (p / 100.0) as f32).filter(|s| s.is_finite() && *s > 0.0),
            _ => {
                if let Some(sample) = r.unicode() {
                    out.push(Preset { name: name.clone().unwrap_or_default(), sample, spacing });
                }
            }
        }
    }
    out
}

/// Where the first of `keys` next appears in `data` from `from`: which one, and just past it.
fn find(data: &[u8], from: usize, keys: &[&[u8]]) -> Option<(usize, usize)> {
    let rest = data.get(from..)?;
    (0..rest.len()).find_map(|i| {
        keys.iter()
            .position(|k| rest.get(i..).is_some_and(|r| r.starts_with(k)))
            .map(|k| (k, from + i + keys.get(k).map_or(0, |k| k.len())))
    })
}

/// Reads big-endian numbers and runs of bytes in order; None past the end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }

    /// A count of UTF-16 units, then the units (a trailing nul left off).
    fn unicode(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let units: Vec<u16> =
            self.take(n.checked_mul(2)?)?.as_chunks::<2>().0.iter().map(|&b| u16::from_be_bytes(b)).collect();
        Some(String::from_utf16_lossy(&units).trim_end_matches('\0').to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    fn put32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    fn unicode(out: &mut Vec<u8>, s: &str) {
        let units: Vec<u16> = s.encode_utf16().chain([0]).collect();
        put32(out, units.len() as u32);
        for u in units {
            put16(out, u);
        }
    }

    #[test]
    fn a_damaged_file_is_refused_not_a_crash() {
        let good = version6();
        assert!(read(&good).is_some());
        crate::damaged::survives("ABR", &good, |b| {
            let _ = read(b);
        });
    }

    /// A version 6 file with one 3 x 2 tip (raw) named by its preset.
    fn version6() -> Vec<u8> {
        let id = "tip-one";
        let mut channel = Vec::new();
        put32(&mut channel, 8);
        for v in [0u32, 0, 2, 3] {
            put32(&mut channel, v);
        }
        put16(&mut channel, 8);
        channel.push(0);
        channel.extend_from_slice(&[0, 128, 255, 10, 20, 30]);
        let mut sample = vec![id.len() as u8];
        sample.extend_from_slice(id.as_bytes());
        sample.extend_from_slice(&[0, 1, 0, 0]);
        put32(&mut sample, 3);
        put32(&mut sample, 0);
        for v in [0u32, 0, 2, 3] {
            put32(&mut sample, v);
        }
        put32(&mut sample, 2);
        put32(&mut sample, 0);
        put32(&mut sample, 1);
        put32(&mut sample, channel.len() as u32);
        sample.extend_from_slice(&channel);
        let mut samp = Vec::new();
        put32(&mut samp, sample.len() as u32);
        samp.extend_from_slice(&sample);
        while samp.len() % 4 != 0 {
            samp.push(0);
        }
        let mut desc = b"brushPreset".to_vec();
        desc.extend_from_slice(b"Nm  TEXT");
        unicode(&mut desc, "Chalky");
        desc.extend_from_slice(b"Nm  TEXT");
        unicode(&mut desc, "Sampled Brush 1");
        desc.extend_from_slice(b"SpcnUntF#Prc");
        desc.extend_from_slice(&40.0f64.to_be_bytes());
        desc.extend_from_slice(b"sampledDataTEXT");
        unicode(&mut desc, id);
        let mut out = Vec::new();
        put16(&mut out, 6);
        put16(&mut out, 2);
        for (key, data) in [(b"samp", &samp), (b"desc", &desc)] {
            out.extend_from_slice(b"8BIM");
            out.extend_from_slice(key);
            put32(&mut out, data.len() as u32);
            out.extend_from_slice(data);
        }
        out
    }

    #[test]
    fn a_version_6_tip_comes_with_its_presets_name_and_spacing() {
        let tips = read(&version6()).unwrap();
        assert_eq!(tips.len(), 1);
        let t = &tips[0];
        assert_eq!((t.width, t.height), (3, 2));
        assert_eq!(t.pixels, [0, 128, 255, 10, 20, 30]);
        assert_eq!(t.name, "Chalky");
        assert_eq!(t.spacing, Some(0.4));
    }

    #[test]
    fn a_version_2_packed_tip_and_bad_files() {
        // one sampled 2 x 2 tip, each row packed as a run
        let mut body = Vec::new();
        put32(&mut body, 0);
        put16(&mut body, 25);
        unicode(&mut body, "Old");
        body.push(1);
        body.extend_from_slice(&[0; 8]);
        for v in [0u32, 0, 2, 2] {
            put32(&mut body, v);
        }
        put16(&mut body, 8);
        body.push(1);
        put16(&mut body, 2);
        put16(&mut body, 2);
        body.extend_from_slice(&[0xFF, 200, 0xFF, 7]);
        let mut file = Vec::new();
        put16(&mut file, 2);
        put16(&mut file, 1);
        put16(&mut file, 2);
        put32(&mut file, body.len() as u32);
        file.extend_from_slice(&body);
        let tips = read(&file).unwrap();
        assert_eq!(tips[0].pixels, [200, 200, 7, 7]);
        assert_eq!(tips[0].name, "Old");
        assert_eq!(tips[0].spacing, Some(0.25));
        assert!(read(b"not a brush").is_none());
        assert_eq!(read(&version6()[..20]).map(|t| t.len()), Some(0));
    }
}
