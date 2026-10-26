//! Reading layered PSD (and large-document PSB) files as artworks: their layers with names,
//! opacity, visibility, blend modes and clipping; groups; layer masks; the resolution and colour
//! profile. A file without layers comes in as one layer of its flattened picture. Grey, RGB and
//! CMYK files of 8 or 16 bits a channel are read.

use std::sync::Arc;

use crate::cms::{CanvasIcc, IccProfile, Space};
use crate::document::{BlendMode, Document, Layer, LayerId, LayerKind, MAX_CANVAS_EDGE};
use crate::native::Imported;
use crate::profile::Profile;

/// Why a PSD couldn't be read.
#[derive(Debug, PartialEq, Eq)]
pub enum PsdError {
    NotAPsd,
    /// A kind of picture not read here (bitmap, indexed, Lab, 32 bits a channel...).
    Unsupported(&'static str),
    Damaged(&'static str),
    TooLarge,
    /// More layers than a canvas this size holds (it holds this many).
    TooManyLayers(usize),
}

/// Whether `bytes` start as a PSD or PSB does.
pub fn is_psd(bytes: &[u8]) -> bool {
    bytes.starts_with(b"8BPS")
}

/// What a file is, as far as reading its pixels goes.
#[derive(Clone, Copy)]
struct Format {
    width: u32,
    height: u32,
    /// Bytes a sample: 1 or 2.
    sample: usize,
    colour: Colour,
    /// PSB: some lengths take eight bytes.
    big: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Colour {
    Grey,
    Rgb,
    Cmyk,
}

impl Colour {
    fn channels(self) -> usize {
        match self {
            Colour::Grey => 1,
            Colour::Rgb => 3,
            Colour::Cmyk => 4,
        }
    }
}

/// Reads a PSD or PSB.
pub fn read(bytes: &[u8]) -> Result<Imported, PsdError> {
    let damaged = PsdError::Damaged;
    let mut r = Reader::new(bytes);
    if r.take(4) != Some(b"8BPS".as_slice()) {
        return Err(PsdError::NotAPsd);
    }
    let big = match r.u16().ok_or(damaged("header"))? {
        1 => false,
        2 => true,
        _ => return Err(damaged("version")),
    };
    r.take(6).ok_or(damaged("header"))?;
    let channels = r.u16().ok_or(damaged("header"))?;
    let height = r.u32().ok_or(damaged("header"))?;
    let width = r.u32().ok_or(damaged("header"))?;
    let depth = r.u16().ok_or(damaged("header"))?;
    let colour = match r.u16().ok_or(damaged("header"))? {
        1 => Colour::Grey,
        3 => Colour::Rgb,
        4 => Colour::Cmyk,
        _ => return Err(PsdError::Unsupported("colour mode")),
    };
    let sample = match depth {
        8 => 1,
        16 => 2,
        _ => return Err(PsdError::Unsupported("bit depth")),
    };
    if width == 0 || height == 0 {
        return Err(damaged("size"));
    }
    if width > MAX_CANVAS_EDGE || height > MAX_CANVAS_EDGE {
        return Err(PsdError::TooLarge);
    }
    let format = Format { width, height, sample, colour, big };
    // colour mode data: nothing for these modes
    let n = r.u32().ok_or(damaged("colour data"))?;
    r.take(n as usize).ok_or(damaged("colour data"))?;
    let n = r.u32().ok_or(damaged("resources"))?;
    let resources = resources(r.take(n as usize).ok_or(damaged("resources"))?);

    let n = r.len(big).ok_or(damaged("layers"))?;
    let mut section = Reader::new(r.take(n).ok_or(damaged("layers"))?);
    let mut records = Vec::new();
    if n > 0 {
        let info = section.len(big).ok_or(damaged("layers"))?;
        let info = section.take(info).ok_or(damaged("layers"))?;
        if !info.is_empty() {
            records = layer_info(info, &format)?;
        }
        // the global mask, then blocks; 16-bit files keep their layers in one of them
        let global = section.u32().unwrap_or(0) as usize;
        section.take(global);
        while records.is_empty() {
            let Some(block) = block(&mut section, big) else { break };
            if block.key == *b"Lr16" || block.key == *b"Layr" {
                records = layer_info(block.data, &format)?;
            }
        }
    }
    let flat = merged(&mut r, &format, channels);

    let doc = Document::new(width, height);
    let (layers, pixels, current) = if records.is_empty() {
        let rgba = flat.as_ref().map(|f| premultiplied(f)).ok_or(damaged("picture"))?;
        let layer = paint_layer(LayerId(1), "Layer 1".to_owned(), None);
        (vec![layer], vec![(LayerId(1), rgba)], LayerId(1))
    } else {
        build(&records, &format)?
    };
    let with_pixels = layers.iter().filter(|l| l.kind != LayerKind::Group).count();
    if with_pixels > doc.layer_cap() {
        return Err(PsdError::TooManyLayers(doc.layer_cap()));
    }
    let mut document = Document::from_layers(width, height, layers, current);
    if let Some(dpi) = resources.dpi {
        document.dpi = dpi;
    }
    if let Some(icc) = resources.icc.as_deref().and_then(IccProfile::parse) {
        match Profile::from_name(&icc.name) {
            Some(builtin) => document.profile = builtin,
            None if icc.space == Space::Rgb => {
                document.profile = if icc.name.contains("P3") { Profile::DisplayP3 } else { Profile::Srgb };
                document.icc = Some(CanvasIcc(Arc::new(icc)));
            }
            None => {}
        }
    }
    Ok(Imported {
        document: document.repaired(),
        pixels,
        name: "Untitled Artwork".to_owned(),
        author: String::new(),
        strokes: 0,
        tracked_seconds: 0.0,
    })
}

/// The flattened picture a PSD carries (straight RGBA, the canvas's size), if it has one.
pub fn flattened(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut r = Reader::new(bytes);
    if r.take(4)? != b"8BPS" {
        return None;
    }
    let big = r.u16()? == 2;
    r.take(6)?;
    let channels = r.u16()?;
    let height = r.u32()?;
    let width = r.u32()?;
    let sample = match r.u16()? {
        8 => 1,
        16 => 2,
        _ => return None,
    };
    let colour = match r.u16()? {
        1 => Colour::Grey,
        3 => Colour::Rgb,
        4 => Colour::Cmyk,
        _ => return None,
    };
    if width == 0 || height == 0 || width > MAX_CANVAS_EDGE || height > MAX_CANVAS_EDGE {
        return None;
    }
    for _ in 0..2 {
        let n = r.u32()?;
        r.take(n as usize)?;
    }
    let n = r.len(big)?;
    r.take(n)?;
    merged(&mut r, &Format { width, height, sample, colour, big }, channels).map(|rgba| (width, height, rgba))
}

/// What the image resources say.
#[derive(Default)]
struct Resources {
    dpi: Option<f32>,
    icc: Option<Vec<u8>>,
}

fn resources(bytes: &[u8]) -> Resources {
    let mut out = Resources::default();
    let mut r = Reader::new(bytes);
    while r.take(4) == Some(b"8BIM".as_slice()) {
        let Some(id) = r.u16() else { break };
        // a name, padded to an even length with its length byte
        let Some(name) = r.u8() else { break };
        if r.take(usize::from(name) + usize::from(name % 2 == 0)).is_none() {
            break;
        }
        let Some(size) = r.u32() else { break };
        let Some(data) = r.take(size as usize) else { break };
        if size % 2 == 1 {
            r.take(1);
        }
        match id {
            // horizontal resolution, 16.16, per inch (unit 1) or per centimetre (2)
            0x03ED => {
                let mut d = Reader::new(data);
                if let (Some(fixed), Some(unit)) = (d.u32(), d.u16()) {
                    let per = fixed as f32 / 65536.0;
                    let dpi = if unit == 2 { per * 2.54 } else { per };
                    if dpi.is_finite() && dpi >= 1.0 {
                        out.dpi = Some(dpi.min(9999.0));
                    }
                }
            }
            0x040F => out.icc = Some(data.to_vec()),
            _ => {}
        }
    }
    out
}

/// A layer as the file has it.
struct Record {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    /// Each channel's id (0.. colours, -1 alpha, -2 mask) and its decoded samples (8 bits).
    channels: Vec<(i16, Vec<u8>)>,
    blend: [u8; 4],
    opacity: u8,
    clipped: bool,
    hidden: bool,
    mask: Option<MaskRect>,
    name: String,
    /// A group's marker: 1 open, 2 closed, 3 the end of one (below its layers).
    section: Option<u32>,
    section_blend: Option<[u8; 4]>,
    fill: u8,
    /// An adjustment or fill layer, whose pixels (if any) are drawn from its settings.
    adjustment: bool,
}

#[derive(Clone, Copy)]
struct MaskRect {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    /// The mask outside its rectangle.
    outside: u8,
    disabled: bool,
}

/// Additional information keys that make an adjustment or fill layer.
const ADJUSTMENTS: [&[u8; 4]; 20] = [
    b"SoCo", b"GdFl", b"PtFl", b"brit", b"levl", b"curv", b"expA", b"vibA", b"hue ", b"hue2", b"blnc", b"blwh",
    b"phfl", b"mixr", b"clrL", b"nvrt", b"post", b"thrs", b"grdm", b"selc",
];

/// The layer records, bottom first, each with its channels decoded.
fn layer_info(bytes: &[u8], f: &Format) -> Result<Vec<Record>, PsdError> {
    let damaged = PsdError::Damaged;
    let mut r = Reader::new(bytes);
    let count = usize::from(r.i16().ok_or(damaged("layers"))?.unsigned_abs());
    let mut records = Vec::with_capacity(count);
    let mut lengths = Vec::with_capacity(count);
    for _ in 0..count {
        let (top, left, bottom, right) = (r.i32(), r.i32(), r.i32(), r.i32());
        let (Some(top), Some(left), Some(bottom), Some(right)) = (top, left, bottom, right) else {
            return Err(damaged("layer"));
        };
        let n = r.u16().ok_or(damaged("layer"))?;
        let mut ids = Vec::with_capacity(usize::from(n));
        for _ in 0..n {
            let id = r.i16().ok_or(damaged("layer"))?;
            ids.push((id, r.len(f.big).ok_or(damaged("layer"))?));
        }
        if r.take(4) != Some(b"8BIM".as_slice()) {
            return Err(damaged("layer"));
        }
        let blend: [u8; 4] = r.take(4).and_then(|b| b.try_into().ok()).ok_or(damaged("layer"))?;
        let opacity = r.u8().ok_or(damaged("layer"))?;
        let clipping = r.u8().ok_or(damaged("layer"))?;
        let flags = r.u8().ok_or(damaged("layer"))?;
        r.u8().ok_or(damaged("layer"))?;
        let extra = r.u32().ok_or(damaged("layer"))?;
        let mut x = Reader::new(r.take(extra as usize).ok_or(damaged("layer"))?);
        // the mask's rectangle and settings
        let n = x.u32().unwrap_or(0) as usize;
        let mask = x.take(n).and_then(|m| {
            let mut m = Reader::new(m);
            let (top, left, bottom, right) = (m.i32()?, m.i32()?, m.i32()?, m.i32()?);
            let outside = m.u8()?;
            let flags = m.u8()?;
            Some(MaskRect { top, left, bottom, right, outside, disabled: flags & 2 != 0 })
        });
        // blending ranges
        let n = x.u32().unwrap_or(0) as usize;
        x.take(n);
        // the name, padded to four with its length byte
        let len = usize::from(x.u8().unwrap_or(0));
        let mut name: String = x.take(len).unwrap_or_default().iter().map(|&b| char::from(b)).collect();
        x.take((4 - (len + 1) % 4) % 4);
        let mut record = Record {
            top,
            left,
            bottom,
            right,
            channels: Vec::new(),
            blend,
            opacity,
            clipped: clipping == 1,
            hidden: flags & 2 != 0,
            mask,
            name: String::new(),
            section: None,
            section_blend: None,
            fill: 255,
            adjustment: false,
        };
        while let Some(b) = block(&mut x, f.big) {
            let mut d = Reader::new(b.data);
            match &b.key {
                b"luni" => {
                    let n = d.u32().unwrap_or(0) as usize;
                    let units: Vec<u16> = (0..n).map_while(|_| d.u16()).take_while(|&u| u != 0).collect();
                    let unicode = String::from_utf16_lossy(&units);
                    if !unicode.trim().is_empty() {
                        name = unicode;
                    }
                }
                b"lsct" | b"lsdk" => {
                    record.section = d.u32();
                    if d.take(4) == Some(b"8BIM".as_slice()) {
                        record.section_blend = d.take(4).and_then(|k| k.try_into().ok());
                    }
                }
                b"iOpa" => record.fill = d.u8().unwrap_or(255),
                key if ADJUSTMENTS.contains(&key) => record.adjustment = true,
                _ => {}
            }
        }
        record.name = name;
        records.push(record);
        lengths.push(ids);
    }
    // each layer's channels, in the order its record listed them
    for (record, ids) in records.iter_mut().zip(lengths) {
        for (id, len) in ids {
            let data = r.take(len).ok_or(damaged("channel"))?;
            let (w, h) = match (id, record.mask) {
                (-2, Some(m)) => (span(m.left, m.right), span(m.top, m.bottom)),
                (-2 | -3, _) => (0, 0),
                _ => (span(record.left, record.right), span(record.top, record.bottom)),
            };
            if id == -3 || w == 0 || h == 0 {
                continue;
            }
            let mut d = Reader::new(data);
            let compression = d.u16().ok_or(damaged("channel"))?;
            let plane = plane(&mut d, compression, w, h, f).ok_or(damaged("channel"))?;
            record.channels.push((id, plane));
        }
    }
    Ok(records)
}

/// A rectangle's extent along one side (none if back to front or too big).
fn span(from: i32, to: i32) -> usize {
    usize::try_from(i64::from(to) - i64::from(from)).ok().filter(|&n| n <= 1 << 20).unwrap_or(0)
}

/// One block of additional information: its key and data.
struct Block<'a> {
    key: [u8; 4],
    data: &'a [u8],
}

fn block<'a>(r: &mut Reader<'a>, big: bool) -> Option<Block<'a>> {
    let sig = r.take(4)?;
    if sig != b"8BIM" && sig != b"8B64" {
        return None;
    }
    let key: [u8; 4] = r.take(4)?.try_into().ok()?;
    // in a PSB these keep eight-byte lengths
    const LONG: [&[u8; 4]; 13] = [
        b"LMsk", b"Lr16", b"Lr32", b"Layr", b"Mt16", b"Mt32", b"Mtrn", b"Alph", b"FMsk", b"lnk2", b"FEid", b"FXid",
        b"PxSD",
    ];
    let n = if big && LONG.contains(&&key) { usize::try_from(r.u64()?).ok()? } else { r.u32()? as usize };
    let data = r.take(n)?;
    Some(Block { key, data })
}

/// A channel's samples, `w` x `h`, decoded and brought to 8 bits.
fn plane(r: &mut Reader<'_>, compression: u16, w: usize, h: usize, f: &Format) -> Option<Vec<u8>> {
    let row = w.checked_mul(f.sample)?;
    let size = row.checked_mul(h)?;
    let raw = match compression {
        0 => r.take(size)?.to_vec(),
        1 => {
            let counts: Vec<usize> = (0..h)
                .map(|_| if f.big { r.u32().map(|c| c as usize) } else { r.u16().map(usize::from) })
                .collect::<Option<_>>()?;
            let mut out = Vec::with_capacity(size);
            for c in counts {
                unpack(r.take(c)?, row, &mut out);
            }
            out
        }
        2 | 3 => {
            let rest = r.take(r.remaining())?;
            let mut out = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(rest, size).ok()?;
            if compression == 3 {
                undo_prediction(&mut out, row, f.sample);
            }
            out
        }
        _ => return None,
    };
    if raw.len() < size {
        return None;
    }
    Some(match f.sample {
        2 => raw
            .as_chunks::<2>()
            .0
            .iter()
            .take(w * h)
            .map(|&[hi, lo]| ((u32::from(u16::from_be_bytes([hi, lo])) * 255 + 32767) / 65535) as u8)
            .collect(),
        _ => raw.into_iter().take(w * h).collect(),
    })
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
            out.extend(std::iter::repeat_n(v, usize::from((-n).unsigned_abs()) + 1));
        }
    }
    out.resize(end, 0);
}

/// ZIP with prediction keeps each sample as its difference from the one before it in the row.
fn undo_prediction(data: &mut [u8], row: usize, sample: usize) {
    for line in data.chunks_mut(row.max(1)) {
        if sample == 2 {
            let mut last = 0u16;
            for s in line.as_chunks_mut::<2>().0 {
                last = last.wrapping_add(u16::from_be_bytes(*s));
                *s = last.to_be_bytes();
            }
        } else {
            let mut last = 0u8;
            for b in line.iter_mut() {
                last = last.wrapping_add(*b);
                *b = last;
            }
        }
    }
}

/// The flattened picture at the end of the file (straight RGBA), if it is there.
fn merged(r: &mut Reader<'_>, f: &Format, channels: u16) -> Option<Vec<u8>> {
    let (w, h) = (f.width as usize, f.height as usize);
    let count = usize::from(channels).max(f.colour.channels());
    let compression = r.u16()?;
    let planes: Vec<Vec<u8>> = match compression {
        1 => {
            // every row's length first, all channels', then the rows
            let rows = h.checked_mul(count)?;
            let counts: Vec<usize> = (0..rows)
                .map(|_| if f.big { r.u32().map(|c| c as usize) } else { r.u16().map(usize::from) })
                .collect::<Option<_>>()?;
            let row = w * f.sample;
            let mut planes = Vec::with_capacity(count);
            for chunk in counts.chunks(h.max(1)) {
                let mut raw = Vec::with_capacity(row * h);
                for &c in chunk {
                    unpack(r.take(c)?, row, &mut raw);
                }
                planes.push(to8(raw, f.sample));
            }
            planes
        }
        0 => (0..count).map(|_| r.take(w * h * f.sample).map(|p| to8(p.to_vec(), f.sample))).collect::<Option<_>>()?,
        _ => return None,
    };
    let colours = f.colour.channels();
    let alpha = planes.get(colours);
    let mut rgba = vec![0u8; w * h * 4];
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let s = |c: usize| planes.get(c).and_then(|p| p.get(i)).copied().unwrap_or(0);
        let [r, g, b] = rgb(f.colour, s);
        *px = [r, g, b, alpha.and_then(|a| a.get(i)).copied().unwrap_or(255)];
    }
    Some(rgba)
}

fn to8(raw: Vec<u8>, sample: usize) -> Vec<u8> {
    if sample == 2 { raw.as_chunks::<2>().0.iter().map(|&[hi, _]| hi).collect() } else { raw }
}

/// A pixel's colour from its samples (CMYK's kept inverted, as PSDs keep them).
fn rgb(colour: Colour, s: impl Fn(usize) -> u8) -> [u8; 3] {
    match colour {
        Colour::Grey => [s(0); 3],
        Colour::Rgb => [s(0), s(1), s(2)],
        Colour::Cmyk => {
            let k = u16::from(s(3));
            [0, 1, 2].map(|c| ((u16::from(s(c)) * k + 127) / 255) as u8)
        }
    }
}

/// Straight RGBA made premultiplied.
fn premultiplied(straight: &[u8]) -> Vec<u8> {
    let mut out = straight.to_vec();
    for px in out.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        for c in px.iter_mut().take(3) {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
    out
}

fn paint_layer(id: LayerId, name: String, parent: Option<LayerId>) -> Layer {
    Layer {
        id,
        name,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        alpha_lock: false,
        clipped: false,
        locked: false,
        reference: false,
        kind: LayerKind::Paint,
        parent,
        collapsed: false,
        text: None,
        private: false,
    }
}

type Built = (Vec<Layer>, Vec<(LayerId, Vec<u8>)>, LayerId);

/// The document's layers (bottom first) and pixels from the records, and the top layer.
fn build(records: &[Record], f: &Format) -> Result<Built, PsdError> {
    let (w, h) = (f.width as usize, f.height as usize);
    let mut layers = Vec::new();
    let mut pixels = Vec::new();
    let mut next = 1u64;
    let mut new_id = || {
        next += 1;
        LayerId(next - 1)
    };
    // the groups open around the records being read
    let mut open: Vec<LayerId> = Vec::new();
    let mut top = None;
    for r in records {
        let opacity = f32::from(r.opacity) / 255.0 * f32::from(r.fill) / 255.0;
        match r.section {
            Some(3) => {
                open.push(new_id());
                continue;
            }
            Some(1 | 2) => {
                let id = open.pop().unwrap_or_else(&mut new_id);
                let blend = r.section_blend.unwrap_or(r.blend);
                layers.push(Layer {
                    kind: LayerKind::Group,
                    visible: !r.hidden,
                    opacity: f32::from(r.opacity) / 255.0,
                    blend: blend_mode(&blend),
                    clipped: r.clipped,
                    collapsed: r.section == Some(2),
                    ..paint_layer(id, r.name.clone(), open.last().copied())
                });
                continue;
            }
            _ => {}
        }
        // an adjustment has no pixels to bring; a fill layer keeps the ones drawn for it
        if r.adjustment && span(r.left, r.right) * span(r.top, r.bottom) == 0 {
            continue;
        }
        let id = new_id();
        layers.push(Layer {
            visible: !r.hidden,
            opacity,
            blend: blend_mode(&r.blend),
            clipped: r.clipped,
            ..paint_layer(id, r.name.clone(), open.last().copied())
        });
        pixels.push((id, place(r, f)));
        top = Some(id);
        // its mask, just above it
        if let (Some(m), Some((_, plane))) = (r.mask.filter(|m| !m.disabled), r.channels.iter().find(|(c, _)| *c == -2))
        {
            let mid = new_id();
            layers.push(Layer {
                kind: LayerKind::Mask,
                name: "Mask".to_owned(),
                ..paint_layer(mid, String::new(), Some(id))
            });
            let mut grey = vec![m.outside; w * h];
            let (mw, mh) = (span(m.left, m.right), span(m.top, m.bottom));
            blit(&mut grey, w, h, m.left, m.top, mw, mh, |i| plane.get(i).copied().unwrap_or(m.outside));
            pixels.push((mid, grey.into_iter().flat_map(|v| [v, v, v, 255]).collect()));
        }
    }
    let current = top.ok_or(PsdError::Damaged("no layers"))?;
    Ok((layers, pixels, current))
}

/// A layer's pixels on the canvas (premultiplied RGBA, the canvas's size).
fn place(r: &Record, f: &Format) -> Vec<u8> {
    let (w, h) = (f.width as usize, f.height as usize);
    let (lw, lh) = (span(r.left, r.right), span(r.top, r.bottom));
    let channel = |id: i16| r.channels.iter().find(|(c, _)| *c == id).map(|(_, p)| p);
    let colours: Vec<Option<&Vec<u8>>> = (0..f.colour.channels() as i16).map(channel).collect();
    let alpha = channel(-1);
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..lh {
        let cy = i64::from(r.top) + y as i64;
        if cy < 0 || cy >= h as i64 {
            continue;
        }
        for x in 0..lw {
            let cx = i64::from(r.left) + x as i64;
            if cx < 0 || cx >= w as i64 {
                continue;
            }
            let j = y * lw + x;
            let s = |c: usize| colours.get(c).copied().flatten().and_then(|p| p.get(j)).copied().unwrap_or(0);
            // no alpha channel: opaque (as a background layer is)
            let a = alpha.map_or(255, |p| p.get(j).copied().unwrap_or(0));
            let i = (cy as usize * w + cx as usize) * 4;
            if let Some(px) = rgba.get_mut(i..i + 4) {
                let [r, g, b] = rgb(f.colour, s).map(|c| ((u16::from(c) * u16::from(a) + 127) / 255) as u8);
                px.copy_from_slice(&[r, g, b, a]);
            }
        }
    }
    rgba
}

/// Copies a `pw` x `ph` patch whose top left is at (`left`, `top`) onto a `w` x `h` grid, taking
/// each of its samples from `sample` (by index in the patch); the part off the grid is left out.
#[allow(clippy::too_many_arguments)]
fn blit(grid: &mut [u8], w: usize, h: usize, left: i32, top: i32, pw: usize, ph: usize, sample: impl Fn(usize) -> u8) {
    for y in 0..ph {
        let gy = i64::from(top) + y as i64;
        if gy < 0 || gy >= h as i64 {
            continue;
        }
        for x in 0..pw {
            let gx = i64::from(left) + x as i64;
            if gx < 0 || gx >= w as i64 {
                continue;
            }
            if let Some(v) = grid.get_mut(gy as usize * w + gx as usize) {
                *v = sample(y * pw + x);
            }
        }
    }
}

/// A blend mode from its key (pass-through groups, dissolve and anything unknown: normal).
fn blend_mode(key: &[u8; 4]) -> BlendMode {
    match key {
        b"mul " => BlendMode::Multiply,
        b"dark" => BlendMode::Darken,
        b"idiv" => BlendMode::ColorBurn,
        b"lbrn" => BlendMode::LinearBurn,
        b"dkCl" => BlendMode::DarkerColor,
        b"lite" => BlendMode::Lighten,
        b"scrn" => BlendMode::Screen,
        b"div " => BlendMode::ColorDodge,
        b"lddg" => BlendMode::Add,
        b"lgCl" => BlendMode::LighterColor,
        b"over" => BlendMode::Overlay,
        b"sLit" => BlendMode::SoftLight,
        b"hLit" => BlendMode::HardLight,
        b"vLit" => BlendMode::VividLight,
        b"lLit" => BlendMode::LinearLight,
        b"pLit" => BlendMode::PinLight,
        b"hMix" => BlendMode::HardMix,
        b"diff" => BlendMode::Difference,
        b"smud" => BlendMode::Exclusion,
        b"fsub" => BlendMode::Subtract,
        b"fdiv" => BlendMode::Divide,
        b"hue " => BlendMode::Hue,
        b"sat " => BlendMode::Saturation,
        b"colr" => BlendMode::Color,
        b"lum " => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
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

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[b]| b)
    }

    fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_be_bytes)
    }

    fn i16(&mut self) -> Option<i16> {
        self.array().map(i16::from_be_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_be_bytes)
    }

    fn i32(&mut self) -> Option<i32> {
        self.array().map(i32::from_be_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_be_bytes)
    }

    /// A length: four bytes, or eight in a PSB.
    fn len(&mut self, big: bool) -> Option<usize> {
        if big { usize::try_from(self.u64()?).ok() } else { self.u32().map(|n| n as usize) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::{PsdKind, PsdLayer, psd};

    /// A `w` x `h` picture of one straight RGBA colour.
    fn solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
        std::iter::repeat_n(px, (w * h) as usize).flatten().collect()
    }

    #[test]
    fn layers_groups_and_masks_come_back_as_they_were_written() {
        let (w, h) = (4, 3);
        let blue = solid(w, h, [0, 0, 255, 255]);
        // red on the left half only, half covered
        let mut red = solid(w, h, [0, 0, 0, 0]);
        for y in 0..h as usize {
            for x in 0..2 {
                red[(y * w as usize + x) * 4..][..4].copy_from_slice(&[255, 0, 0, 128]);
            }
        }
        let mask: Vec<u8> = (0..w * h).map(|i| if i % 2 == 0 { 255 } else { 0 }).collect();
        let layers = [
            PsdLayer {
                name: "Group",
                kind: PsdKind::GroupStart { open: false },
                opacity: 1.0,
                visible: true,
                blend: BlendMode::Normal,
                clipped: false,
            },
            PsdLayer {
                name: "Red",
                kind: PsdKind::Pixels { rgba: &red, mask: Some(&mask) },
                opacity: 1.0,
                visible: true,
                blend: BlendMode::Screen,
                clipped: false,
            },
            PsdLayer {
                name: "",
                kind: PsdKind::GroupEnd,
                opacity: 1.0,
                visible: true,
                blend: BlendMode::Normal,
                clipped: false,
            },
            PsdLayer {
                name: "Bläu",
                kind: PsdKind::Pixels { rgba: &blue, mask: None },
                opacity: 0.5,
                visible: false,
                blend: BlendMode::Multiply,
                clipped: false,
            },
        ];
        let bytes = psd(w, h, 300.0, &[], &layers, &blue, None);
        let got = read(&bytes).unwrap();
        let doc = &got.document;
        assert_eq!((doc.width(), doc.height()), (w, h));
        assert!((doc.dpi - 300.0).abs() < 0.01);
        let names: Vec<(&str, LayerKind)> = doc.layers().iter().map(|l| (l.name.as_str(), l.kind)).collect();
        assert_eq!(
            names,
            [
                ("Bläu", LayerKind::Paint),
                ("Red", LayerKind::Paint),
                ("Mask", LayerKind::Mask),
                ("Group", LayerKind::Group)
            ]
        );
        let [bottom, red_layer, mask_layer, group] = doc.layers() else { panic!() };
        assert!(!bottom.visible);
        assert_eq!(bottom.blend, BlendMode::Multiply);
        assert!((bottom.opacity - 0.5).abs() < 0.01);
        assert_eq!(red_layer.blend, BlendMode::Screen);
        assert_eq!(red_layer.parent, Some(group.id));
        assert_eq!(mask_layer.parent, Some(red_layer.id));
        assert!(group.collapsed);
        // pixels premultiplied, placed where they were
        let px = |id: LayerId| got.pixels.iter().find(|(i, _)| *i == id).map(|(_, p)| p.clone()).unwrap();
        let reds = px(red_layer.id);
        assert_eq!(&reds[0..4], &[128, 0, 0, 128]);
        assert_eq!(&reds[8..12], &[0, 0, 0, 0]);
        assert_eq!(&px(bottom.id)[0..4], &[0, 0, 255, 255]);
        let m = px(mask_layer.id);
        assert_eq!(&m[0..8], &[255, 255, 255, 255, 0, 0, 0, 255]);
        // the flattened picture is there too
        let (fw, fh, flat) = flattened(&bytes).unwrap();
        assert_eq!((fw, fh), (w, h));
        assert_eq!(&flat[0..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn a_file_without_layers_is_one_layer_and_bad_files_are_turned_away() {
        let (w, h) = (3, 2);
        let flat = solid(w, h, [10, 20, 30, 255]);
        let mut bytes = psd(w, h, 72.0, &[], &[], &flat, None);
        let got = read(&bytes).unwrap();
        assert_eq!(got.document.layers().len(), 1);
        assert_eq!(&got.pixels[0].1[0..4], &[10, 20, 30, 255]);
        assert_eq!(read(b"PNG.").err(), Some(PsdError::NotAPsd));
        // Lab
        bytes[25] = 9;
        assert_eq!(read(&bytes).err(), Some(PsdError::Unsupported("colour mode")));
        bytes[25] = 3;
        bytes.truncate(40);
        assert!(read(&bytes).is_err());
    }

    #[test]
    fn packbits_and_prediction_decode() {
        let mut out = Vec::new();
        // three literal bytes, then 9 repeated four times
        unpack(&[2, 1, 2, 3, 0xFD, 9], 7, &mut out);
        assert_eq!(out, [1, 2, 3, 9, 9, 9, 9]);
        let mut d = [1u8, 1, 1, 5, 0, 0];
        undo_prediction(&mut d, 3, 1);
        assert_eq!(d, [1, 2, 3, 5, 5, 5]);
    }
}
