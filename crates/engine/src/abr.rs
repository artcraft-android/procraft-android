//! Reading the brushes in Photoshop brush files (ABR): each sampled tip's grey image (white
//! paints), with its preset's name, spacing, angle and roundness where the file gives them.
//! Versions 1 and 2 (the tips one after another; their computed tips left out) and 6 and later (a
//! section of samples, one of patterns and a descriptor of presets, computed round ones with their
//! hardness too, and a preset's texture from the patterns).

/// One brush tip from the file.
#[derive(Clone, Debug, PartialEq)]
pub struct AbrTip {
    pub name: String,
    /// Space between stamps, as a share of the tip's size, if the file gives it.
    pub spacing: Option<f32>,
    pub width: u32,
    pub height: u32,
    /// Grey, `width` x `height`, rows top to bottom: how much each pixel paints.
    pub pixels: Vec<u8>,
}

/// One of the file's brushes: a sampled tip, or (none) a computed round one, with its preset's
/// name, spacing, angle and roundness where the file gives them, and a computed one's hardness.
#[derive(Clone, Debug, PartialEq)]
pub struct AbrBrush {
    pub name: String,
    pub tip: Option<AbrTip>,
    pub spacing: Option<f32>,
    /// Radians.
    pub angle: f32,
    /// How round the tip is, 1 a circle.
    pub roundness: Option<f32>,
    /// A computed tip's edge, 0 soft to 1 hard.
    pub hardness: Option<f32>,
    /// Its tip's diameter, pixels, if the preset gives it.
    pub diameter: Option<f32>,
    /// The pattern the preset paints through, if it uses one that is in the file.
    pub texture: Option<AbrTexture>,
    /// What the pen's pressure changes.
    pub dynamics: AbrDynamics,
}

/// What a preset's shape and paint dynamics have the pen's pressure change.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AbrDynamics {
    /// Whether its shape dynamics are on; what controls the size (0 nothing, 2 the pen's
    /// pressure, 3 its tilt), the least share of the size it goes down to, and how much it jitters.
    pub tip: bool,
    pub size_control: i32,
    pub min_diameter: f32,
    pub size_jitter: f32,
    /// Whether its paint (transfer) dynamics are on; what controls the opacity and the flow, the
    /// least of each, and how much the flow jitters.
    pub paint: bool,
    pub opacity_control: i32,
    pub opacity_min: f32,
    pub flow_control: i32,
    pub flow_min: f32,
    pub flow_jitter: f32,
    /// The flow its tool options keep, 0 to 1, if they do.
    pub tool_flow: Option<f32>,
}

/// A preset's texture: a pattern from the file, grey, and how the preset uses it.
#[derive(Clone, Debug, PartialEq)]
pub struct AbrTexture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Grey, `width` x `height`, rows top to bottom.
    pub pixels: Vec<u8>,
    /// Its tile's size as a share of the pattern's own, if the preset gives it.
    pub scale: Option<f32>,
    /// How strongly it shows, 0 to 1, if the preset gives it.
    pub depth: Option<f32>,
    /// Whether dark lets paint through instead of light.
    pub invert: bool,
    /// The least it shows (pressure and the like lower it to this), 0 to 1, if given.
    pub min_depth: Option<f32>,
    /// Its brightness (-150 to 150) and contrast (-50 to 100), as the preset keeps them.
    pub brightness: i32,
    pub contrast: i32,
    /// How it is laid over the paint, by the preset's four-letter name ("Mltp", "Hght"...), if given.
    pub blend: Option<String>,
    /// Whether each stamp has it afresh (else it lies still under the stroke).
    pub each_tip: bool,
}

/// The largest tip side read, pixels.
const MOST_SIDE: u32 = 5000;
/// The most bytes of tip images the brushes may have together (presets sharing one sample each
/// have a copy).
const MOST_TIP_BYTES: usize = 256 << 20;
/// The most bytes of patterns read, and of their copies the presets have together.
const MOST_PATTERN_BYTES: usize = 256 << 20;

/// The sampled tips in an ABR file, in the file's order; none if it isn't one.
pub fn read(bytes: &[u8]) -> Option<Vec<AbrTip>> {
    Some(read_brushes(bytes)?.into_iter().filter_map(|b| b.tip).collect())
}

/// The brushes in an ABR file: its presets in order (from version 6; samples nothing names after
/// them), or its sampled tips (versions 1 and 2); none if it isn't one.
pub fn read_brushes(bytes: &[u8]) -> Option<Vec<AbrBrush>> {
    let mut r = Reader::new(bytes);
    let version = r.u16()?;
    let sampled = |t: AbrTip| AbrBrush {
        name: t.name.clone(),
        spacing: t.spacing,
        tip: Some(t),
        angle: 0.0,
        roundness: None,
        hardness: None,
        diameter: None,
        texture: None,
        dynamics: AbrDynamics::default(),
    };
    match version {
        1 | 2 => Some(read_old(&mut r, version)?.into_iter().map(sampled).collect()),
        // 6, 7, 9 and 10 seen; any later one in the same sections is read the same way
        6.. if bytes.get(4..8) == Some(b"8BIM".as_slice()) => read_sections(&mut r),
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

/// Version 6 on: "8BIM" sections; "samp" holds the tips by id, "desc" the presets.
fn read_sections(r: &mut Reader<'_>) -> Option<Vec<AbrBrush>> {
    let minor = r.u16()?;
    let mut samples: Vec<(String, u32, u32, Vec<u8>)> = Vec::new();
    let mut patterns: Vec<Pattern> = Vec::new();
    let (mut presets, mut others): (Vec<Preset>, Vec<String>) = (Vec::new(), Vec::new());
    while r.remaining() >= 12 {
        if r.take(4)? != b"8BIM" {
            break;
        }
        let key = r.take(4)?;
        let len = r.u32()? as usize;
        // a section cut short ends the file
        let Some(data) = r.take(len) else { break };
        // each is padded to a multiple of four
        r.take(((4 - len % 4) % 4).min(r.remaining()));
        match key {
            b"samp" => samples = read_samples(data, minor),
            b"patt" => patterns = read_patterns(data),
            b"desc" => (presets, others) = read_presets(data),
            _ => {}
        }
    }
    let mut out: Vec<AbrBrush> = Vec::new();
    // a sample only a dual brush names isn't a brush of its own
    let mut used: Vec<bool> = samples.iter().map(|s| others.contains(&s.0)).collect();
    let mut left = MOST_TIP_BYTES;
    let mut patterns_left = MOST_PATTERN_BYTES;
    let named = |name: &str, n: usize| {
        if name.trim().is_empty() { format!("Brush {}", n + 1) } else { name.to_owned() }
    };
    for p in &presets {
        // a sampled preset's tip, if its sample is there (and there is room for its copy)
        let tip = match &p.sample {
            None => None,
            Some(id) => {
                let Some(k) = samples.iter().position(|s| s.0 == *id) else { continue };
                let (Some((_, width, height, pixels)), Some(u)) = (samples.get(k), used.get_mut(k)) else { continue };
                let Some(rest) = left.checked_sub(pixels.len()) else { continue };
                (left, *u) = (rest, true);
                Some(AbrTip {
                    name: named(&p.name, out.len()),
                    spacing: p.spacing,
                    width: *width,
                    height: *height,
                    pixels: pixels.clone(),
                })
            }
        };
        // its texture, if it uses one the file has (and there is room for its copy)
        let texture = p
            .texture
            .as_ref()
            .filter(|_| p.use_texture)
            .and_then(|id| patterns.iter().find(|t| t.id == *id))
            .filter(|t| patterns_left.checked_sub(t.pixels.len()).map(|l| patterns_left = l).is_some())
            .map(|t| AbrTexture {
                name: t.name.clone(),
                width: t.width,
                height: t.height,
                pixels: t.pixels.clone(),
                scale: p.texture_scale,
                depth: p.texture_depth,
                invert: p.texture_invert,
                min_depth: p.texture_min_depth,
                brightness: p.texture_brightness,
                contrast: p.texture_contrast,
                blend: p.texture_blend.clone(),
                each_tip: p.texture_each_tip,
            });
        out.push(AbrBrush {
            name: named(&p.name, out.len()),
            tip,
            spacing: p.spacing,
            angle: p.angle,
            roundness: p.roundness,
            hardness: p.hardness,
            diameter: p.diameter,
            texture,
            dynamics: p.dynamics,
        });
    }
    // samples nothing names, as they are
    for ((_, width, height, pixels), u) in samples.into_iter().zip(used) {
        if !u {
            let name = named("", out.len());
            let tip = AbrTip { name: name.clone(), spacing: None, width, height, pixels };
            out.push(AbrBrush {
                name,
                tip: Some(tip),
                spacing: None,
                angle: 0.0,
                roundness: None,
                hardness: None,
                diameter: None,
                texture: None,
                dynamics: AbrDynamics::default(),
            });
        }
    }
    Some(out)
}

/// The samples: each its id and its image (version 6.1: straight after a short rectangle; later:
/// the one channel written of its array).
fn read_samples(data: &[u8], minor: u16) -> Vec<(String, u32, u32, Vec<u8>)> {
    let mut r = Reader::new(data);
    let mut out = Vec::new();
    while r.remaining() >= 4 {
        let Some(len) = r.u32() else { break };
        // samples are padded to a multiple of four
        let padded = (len as usize).div_ceil(4) * 4;
        let Some(body) = r.take(padded.min(r.remaining())) else { break };
        if let Some(sample) = read_sample(body.get(..len as usize).unwrap_or(body), minor) {
            out.push(sample);
        }
    }
    out
}

fn read_sample(body: &[u8], minor: u16) -> Option<(String, u32, u32, Vec<u8>)> {
    let mut b = Reader::new(body);
    let n = usize::from(b.u8()?);
    let id = String::from_utf8_lossy(b.take(n)?).into_owned();
    if minor == 1 {
        // a short rectangle and depth, then the one used
        b.take(10)?;
        let (top, left, bottom, right) = (b.i32()?, b.i32()?, b.i32()?, b.i32()?);
        let depth = b.u16()?;
        let compression = b.u8()?;
        let pixels = image(&mut b, top, left, bottom, right, depth, compression)?;
        return Some((id, span(left, right)?, span(top, bottom)?, pixels));
    }
    b.take(4)?;
    let (width, height, pixels) = channels(&mut b, 1)?.into_iter().next()?;
    Some((id, width, height, pixels))
}

/// The first `most` channels written in an array of them (its version, length and rectangle, then
/// each channel's rectangle, depth, compression and image), as grey images; none if there are none.
fn channels(b: &mut Reader<'_>, most: usize) -> Option<Vec<(u32, u32, Vec<u8>)>> {
    b.u32()?;
    b.u32()?;
    b.take(16)?;
    let count = b.u32()?;
    let mut out = Vec::new();
    // the colour channels, then a mask's and a sheet's
    for _ in 0..count.saturating_add(2).min(64) {
        if out.len() >= most {
            break;
        }
        let Some(written) = b.u32() else { break };
        if written == 0 {
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
        out.push((span(left, right)?, span(top, bottom)?, pixels));
    }
    (!out.is_empty()).then_some(out)
}

/// A pattern from the file, grey.
struct Pattern {
    id: String,
    name: String,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// The patterns: each its version, colour mode, size, name, id (an indexed one's palette after)
/// and its channels, made grey: an RGB one's three by how bright each looks, an indexed one's
/// through its palette. Up to [`MOST_PATTERN_BYTES`] of them.
fn read_patterns(data: &[u8]) -> Vec<Pattern> {
    let mut r = Reader::new(data);
    let mut out = Vec::new();
    let mut left = MOST_PATTERN_BYTES;
    while r.remaining() >= 4 {
        let Some(len) = r.u32() else { break };
        // patterns are padded to a multiple of four
        let padded = (len as usize).div_ceil(4) * 4;
        let Some(body) = r.take(padded.min(r.remaining())) else { break };
        let Some(p) = read_pattern(body.get(..len as usize).unwrap_or(body)) else { continue };
        let Some(rest) = left.checked_sub(p.pixels.len()) else { break };
        left = rest;
        out.push(p);
    }
    out
}

fn read_pattern(body: &[u8]) -> Option<Pattern> {
    let mut b = Reader::new(body);
    b.u32()?;
    let mode = b.u32()?;
    b.take(4)?;
    let name = b.unicode()?;
    let n = usize::from(b.u8()?);
    let id = String::from_utf8_lossy(b.take(n)?).into_owned();
    let palette = if mode == 2 { Some(b.take(768)?) } else { None };
    let found = channels(&mut b, if mode == 3 { 3 } else { 1 })?;
    let (width, height, first) = found.first()?;
    let bright = |r: u8, g: u8, b: u8| ((299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000) as u8;
    let pixels = match (palette, found.as_slice()) {
        (_, [(_, _, r), (_, _, g), (_, _, bl)]) if r.len() == g.len() && g.len() == bl.len() => {
            r.iter().zip(g).zip(bl).map(|((&r, &g), &bl)| bright(r, g, bl)).collect()
        }
        (Some(colours), _) => first
            .iter()
            .map(|&i| {
                let c = |k: usize| colours.get(usize::from(i) * 3 + k).copied().unwrap_or(0);
                bright(c(0), c(1), c(2))
            })
            .collect(),
        _ => first.clone(),
    };
    Some(Pattern { id, name, width: *width, height: *height, pixels })
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

/// A preset as the descriptor has it: its name, its sample's id (none for a computed tip), and its
/// tip's spacing, angle, roundness and (computed) hardness.
#[derive(Clone, Debug, Default)]
struct Preset {
    name: String,
    sample: Option<String>,
    spacing: Option<f32>,
    angle: f32,
    roundness: Option<f32>,
    hardness: Option<f32>,
    diameter: Option<f32>,
    /// Whether its texture is on, and the pattern's id, scale, depth and inversion.
    use_texture: bool,
    texture: Option<String>,
    texture_scale: Option<f32>,
    texture_depth: Option<f32>,
    texture_invert: bool,
    texture_min_depth: Option<f32>,
    texture_brightness: i32,
    texture_contrast: i32,
    texture_blend: Option<String>,
    texture_each_tip: bool,
    dynamics: AbrDynamics,
}

/// The presets in the descriptor, found by their keys in order. A preset's name comes first, then
/// its own brush: a computed one's diameter, hardness, angle, roundness and spacing, or a sampled
/// one's the same without hardness and then its sample's id. Brushes after it (a dual brush's)
/// aren't the preset's: the samples they name come after the presets. Its texture comes after its
/// brushes, so it goes to what was kept of the preset, and to the preset as it is still read.
fn read_presets(data: &[u8]) -> (Vec<Preset>, Vec<String>) {
    const KEYS: [&[u8]; 32] = [
        b"brushPreset",
        b"Nm  TEXT",
        b"SpcnUntF#Prc",
        b"sampledDataTEXT",
        b"AnglUntF#Ang",
        b"RndnUntF#Prc",
        b"HrdnUntF#Prc",
        b"computedBrush",
        b"dualBrush",
        b"useTexturebool",
        b"TxtrObjc",
        b"IdntTEXT",
        b"textureScaleUntF#Prc",
        b"textureDepthUntF#Prc",
        b"InvTbool",
        b"useTipDynamicsbool",
        b"minimumDiameterUntF#Prc",
        b"szVrObjc",
        b"usePaintDynamicsbool",
        b"opVrObjc",
        b"prVrObjc",
        b"bVTylong",
        b"DmtrUntF#Pxl",
        b"minimumDepthUntF#Prc",
        b"textureBrightnesslong",
        b"textureContrastlong",
        b"textureBlendModeenum",
        b"jitterUntF#Prc",
        b"Mnm UntF#Prc",
        b"flowlong",
        b"TxtCbool",
        // any other object: the end of a dynamics' own settings
        b"Objc",
    ];
    let (mut out, mut others) = (Vec::new(), Vec::new());
    // the preset being read, whether its brush is computed, and whether its brush is done
    let (mut p, mut computed, mut done) = (Preset::default(), false, false);
    let mut named = false;
    // where the preset's own entries begin among those kept, whether its pattern is being read,
    // and which of its dynamics' control is next
    let (mut first, mut in_texture, mut controlled) = (0, false, None);
    let mut at = 0;
    let end = |p: &Preset, computed: bool, done: &mut bool, out: &mut Vec<Preset>| {
        if computed && !*done {
            out.push(p.clone());
        }
        *done = true;
    };
    while let Some((key, from)) = find(data, at, &KEYS) {
        at = from;
        let mut r = Reader::new(data.get(from..).unwrap_or_default());
        let percent = |r: &mut Reader<'_>| r.f64().map(|v| (v / 100.0) as f32).filter(|v| v.is_finite());
        match key {
            // a preset's tool options keep a switch named so: not a new preset
            0 if data.get(from..from + 4) == Some(&b"bool"[..]) => {}
            // a new preset: its name is the next one
            0 => {
                end(&p, computed, &mut done, &mut out);
                (p, computed, done, named) = (Preset::default(), false, false, false);
                (first, in_texture, controlled) = (out.len(), false, None);
            }
            // its texture and dynamics, after its brushes
            9..=21 | 23..=31 => {
                let switch = |r: &mut Reader<'_>| r.u8().is_some_and(|v| v != 0);
                let later = match key {
                    9 => Later::TextureOn(switch(&mut r)),
                    10 => {
                        in_texture = true;
                        continue;
                    }
                    11 if in_texture => Later::Pattern(r.unicode()),
                    12 => Later::Scale(percent(&mut r).filter(|v| *v > 0.0)),
                    13 => Later::Depth(percent(&mut r).map(|v| v.clamp(0.0, 1.0))),
                    14 => Later::Invert(switch(&mut r)),
                    15 => Later::TipDynamics(switch(&mut r)),
                    16 => Later::MinDiameter(percent(&mut r).map_or(0.0, |v| v.clamp(0.0, 1.0))),
                    17 | 19 | 20 => {
                        controlled = Some(key);
                        continue;
                    }
                    18 => Later::PaintDynamics(switch(&mut r)),
                    23 => Later::MinDepth(percent(&mut r).map(|v| v.clamp(0.0, 1.0))),
                    24 => Later::Brightness(r.i32().unwrap_or(0)),
                    25 => Later::Contrast(r.i32().unwrap_or(0)),
                    26 => Later::Blend(enum_value(&mut r)),
                    21 => match controlled {
                        Some(of) => Later::Control(of, r.i32().unwrap_or(0)),
                        None => continue,
                    },
                    27 => match controlled {
                        Some(of) => Later::Jitter(of, percent(&mut r).map_or(0.0, |v| v.clamp(0.0, 1.0))),
                        None => continue,
                    },
                    28 => match controlled {
                        Some(of) => Later::Minimum(of, percent(&mut r).map_or(0.0, |v| v.clamp(0.0, 1.0))),
                        None => continue,
                    },
                    29 => Later::ToolFlow(r.i32().map(|v| (v as f32 / 100.0).clamp(0.0, 1.0))),
                    30 => Later::EachTip(switch(&mut r)),
                    31 => {
                        controlled = None;
                        continue;
                    }
                    _ => continue,
                };
                later.set(&mut p);
                for kept in out.get_mut(first..).into_iter().flatten() {
                    later.set(kept);
                }
            }
            3 if done => others.extend(r.unicode()),
            _ if done => {}
            1 if !named => {
                p.name = r.unicode().unwrap_or_default();
                named = true;
            }
            2 => p.spacing = percent(&mut r).filter(|s| *s > 0.0),
            3 => {
                if let Some(sample) = r.unicode() {
                    out.push(Preset { sample: Some(sample), ..p.clone() });
                }
                done = true;
            }
            4 => p.angle = r.f64().filter(|v| v.is_finite()).map_or(0.0, |v| v.to_radians() as f32),
            5 => p.roundness = percent(&mut r).filter(|v| *v > 0.0 && *v <= 1.0),
            6 => p.hardness = percent(&mut r).map(|v| v.clamp(0.0, 1.0)),
            22 => p.diameter = r.f64().filter(|v| v.is_finite() && *v > 0.0).map(|v| v as f32),
            7 => computed = true,
            8 => end(&p, computed, &mut done, &mut out),
            _ => {}
        }
    }
    end(&p, computed, &mut done, &mut out);
    (out, others)
}

/// One of a preset's settings that come after its brushes (its texture and dynamics), as read.
enum Later {
    TextureOn(bool),
    Pattern(Option<String>),
    Scale(Option<f32>),
    Depth(Option<f32>),
    Invert(bool),
    TipDynamics(bool),
    MinDiameter(f32),
    PaintDynamics(bool),
    MinDepth(Option<f32>),
    Brightness(i32),
    Contrast(i32),
    Blend(Option<String>),
    EachTip(bool),
    /// The size's (key 17), opacity's (19) or flow's (20) control, jitter and least.
    Control(usize, i32),
    Jitter(usize, f32),
    Minimum(usize, f32),
    ToolFlow(Option<f32>),
}

impl Later {
    fn set(&self, p: &mut Preset) {
        let d = &mut p.dynamics;
        match self {
            Later::TextureOn(v) => p.use_texture = *v,
            Later::Pattern(id) => p.texture.clone_from(id),
            Later::Scale(v) => p.texture_scale = *v,
            Later::Depth(v) => p.texture_depth = *v,
            Later::Invert(v) => p.texture_invert = *v,
            Later::TipDynamics(v) => d.tip = *v,
            Later::MinDiameter(v) => d.min_diameter = *v,
            Later::PaintDynamics(v) => d.paint = *v,
            Later::MinDepth(v) => p.texture_min_depth = *v,
            Later::Brightness(v) => p.texture_brightness = *v,
            Later::Contrast(v) => p.texture_contrast = *v,
            Later::Blend(v) => p.texture_blend.clone_from(v),
            Later::EachTip(v) => p.texture_each_tip = *v,
            Later::Control(17, v) => d.size_control = *v,
            Later::Control(19, v) => d.opacity_control = *v,
            Later::Control(_, v) => d.flow_control = *v,
            Later::Jitter(17, v) => d.size_jitter = *v,
            Later::Jitter(20, v) => d.flow_jitter = *v,
            Later::Minimum(19, v) => d.opacity_min = *v,
            Later::Minimum(20, v) => d.flow_min = *v,
            Later::Jitter(..) | Later::Minimum(..) => {}
            Later::ToolFlow(v) => d.tool_flow = *v,
        }
    }
}

/// An enumerated value's name: its type's key, then its own (each a 4-letter key when its length
/// reads 0, else that many letters).
fn enum_value(r: &mut Reader<'_>) -> Option<String> {
    let key = |r: &mut Reader<'_>| {
        let n = r.u32()? as usize;
        let bytes = r.take(if n == 0 { 4 } else { n.min(64) })?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    };
    key(r)?;
    key(r)
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
        let mut desc = b"brushPreset".to_vec();
        desc.extend_from_slice(b"Nm  TEXT");
        unicode(&mut desc, "Chalky");
        desc.extend_from_slice(b"Nm  TEXT");
        unicode(&mut desc, "Sampled Brush 1");
        desc.extend_from_slice(b"SpcnUntF#Prc");
        desc.extend_from_slice(&40.0f64.to_be_bytes());
        desc.extend_from_slice(b"sampledDataTEXT");
        unicode(&mut desc, "tip-one");
        with_presets(&desc)
    }

    /// A version 6 file of the 3 x 2 tip "tip-one" and the descriptor `desc`.
    fn with_presets(desc: &[u8]) -> Vec<u8> {
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
        while !samp.len().is_multiple_of(4) {
            samp.push(0);
        }
        let desc = desc.to_vec();
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
    fn a_version_9_file_is_read_like_version_6_and_other_files_are_not() {
        let six = version6();
        let mut nine = six.clone();
        nine[..2].copy_from_slice(&9u16.to_be_bytes());
        let (a, b) = (read_brushes(&six).unwrap(), read_brushes(&nine).unwrap());
        assert_eq!(a.len(), b.len());
        assert!(!b.is_empty());
        assert_eq!(a[0].name, b[0].name);
        // a zip (a brush bundle, say) starts with a large first number but no sections
        assert!(read_brushes(&[0x50, 0x4b, 3, 4, 20, 0, 0, 0, 8, 0]).is_none());
        let mut no_sections = nine;
        no_sections[4..8].copy_from_slice(b"XXXX");
        assert!(read_brushes(&no_sections).is_none());
    }

    #[test]
    fn a_version_6_1_tip_is_read_after_its_short_rectangle_and_padding_is_passed_over() {
        let id = "tip-old";
        let mut sample = vec![id.len() as u8];
        sample.extend_from_slice(id.as_bytes());
        for v in [0u16, 0, 2, 3, 8] {
            put16(&mut sample, v);
        }
        for v in [0u32, 0, 2, 3] {
            put32(&mut sample, v);
        }
        put16(&mut sample, 8);
        sample.push(0);
        sample.extend_from_slice(&[9, 8, 7, 6, 5, 4]);
        let mut samp = Vec::new();
        put32(&mut samp, sample.len() as u32);
        samp.extend_from_slice(&sample);
        let mut desc = b"brushPreset".to_vec();
        desc.extend_from_slice(b"Nm  TEXT");
        unicode(&mut desc, "Old Stamp");
        desc.extend_from_slice(b"BrshObjcsampledBrushsampledDataTEXT");
        unicode(&mut desc, id);
        let mut out = Vec::new();
        put16(&mut out, 6);
        put16(&mut out, 1);
        // a section of three bytes, padded to four, before the ones read
        for (key, data) in [(b"patt", &vec![1, 2, 3]), (b"samp", &samp), (b"desc", &desc)] {
            out.extend_from_slice(b"8BIM");
            out.extend_from_slice(key);
            put32(&mut out, data.len() as u32);
            out.extend_from_slice(data);
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
        }
        let brushes = read_brushes(&out).unwrap();
        assert_eq!(brushes.len(), 1);
        let tip = brushes[0].tip.as_ref().unwrap();
        assert_eq!((brushes[0].name.as_str(), tip.width, tip.height), ("Old Stamp", 3, 2));
        assert_eq!(tip.pixels, [9, 8, 7, 6, 5, 4]);
    }

    #[test]
    fn a_presets_dynamics_say_what_pressure_changes() {
        let mut desc = b"brushPreset".to_vec();
        desc.extend_from_slice(b"Nm  TEXT");
        unicode(&mut desc, "Soft");
        desc.extend_from_slice(b"BrshObjcsampledBrushsampledDataTEXT");
        unicode(&mut desc, "tip-one");
        // after the brush: shape dynamics with size by pressure (control 2) down to 40%
        desc.extend_from_slice(b"useTipDynamicsbool\x01");
        desc.extend_from_slice(b"minimumDiameterUntF#Prc");
        desc.extend_from_slice(&40.0f64.to_be_bytes());
        desc.extend_from_slice(b"szVrObjcbVTylong");
        put32(&mut desc, 2);
        // an angle's control between isn't the size's
        desc.extend_from_slice(b"angleDynamicsObjcbVTylong");
        put32(&mut desc, 2);
        // paint dynamics: opacity by pressure, flow by nothing
        desc.extend_from_slice(b"usePaintDynamicsbool\x01prVrObjcbVTylong");
        put32(&mut desc, 0);
        desc.extend_from_slice(b"opVrObjcbVTylong");
        put32(&mut desc, 2);
        let d = read_brushes(&with_presets(&desc)).unwrap()[0].dynamics;
        assert_eq!(
            d,
            AbrDynamics {
                tip: true,
                size_control: 2,
                min_diameter: 0.4,
                paint: true,
                opacity_control: 2,
                ..AbrDynamics::default()
            }
        );
    }

    #[test]
    fn a_preset_paints_through_the_pattern_it_names() {
        let number = |out: &mut Vec<u8>, key: &[u8], v: f64| {
            out.extend_from_slice(key);
            out.extend_from_slice(&v.to_be_bytes());
        };
        let preset = |desc: &mut Vec<u8>, name: &str, textured: bool| {
            desc.extend_from_slice(b"brushPreset");
            desc.extend_from_slice(b"Nm  TEXT");
            unicode(desc, name);
            desc.extend_from_slice(b"BrshObjcsampledBrushsampledDataTEXT");
            unicode(desc, "tip-one");
            // after the brush: the texture, on or off
            desc.extend_from_slice(b"useTexturebool");
            desc.push(u8::from(textured));
            desc.extend_from_slice(b"TxtrObjcPtrnNm  TEXT");
            unicode(desc, "Paper");
            desc.extend_from_slice(b"IdntTEXT");
            unicode(desc, "pat-1");
            number(desc, b"textureScaleUntF#Prc", 50.0);
            number(desc, b"textureDepthUntF#Prc", 40.0);
            desc.extend_from_slice(b"InvTbool\x01");
            number(desc, b"minimumDepthUntF#Prc", 17.0);
            desc.extend_from_slice(b"textureBrightnesslong");
            put32(desc, 30);
            desc.extend_from_slice(b"textureContrastlong");
            put32(desc, (-20i32) as u32);
            // an enum: its type's key and its own, each 4 letters (a length of 0)
            desc.extend_from_slice(b"textureBlendModeenum\0\0\0\0BlnM\0\0\0\0Hght");
        };
        let mut desc = Vec::new();
        preset(&mut desc, "Grainy", true);
        preset(&mut desc, "Smooth", false);
        // a grey 2 x 2 pattern, its channels after its name and id
        let mut pattern = Vec::new();
        put32(&mut pattern, 1);
        put32(&mut pattern, 1);
        put16(&mut pattern, 2);
        put16(&mut pattern, 2);
        unicode(&mut pattern, "Paper");
        pattern.push(5);
        pattern.extend_from_slice(b"pat-1");
        let mut channel = Vec::new();
        put32(&mut channel, 8);
        for v in [0u32, 0, 2, 2] {
            put32(&mut channel, v);
        }
        put16(&mut channel, 8);
        channel.push(0);
        channel.extend_from_slice(&[10, 20, 30, 40]);
        put32(&mut pattern, 3);
        put32(&mut pattern, 0);
        for v in [0u32, 0, 2, 2] {
            put32(&mut pattern, v);
        }
        put32(&mut pattern, 24);
        put32(&mut pattern, 1);
        put32(&mut pattern, channel.len() as u32);
        pattern.extend_from_slice(&channel);
        let mut patt = Vec::new();
        put32(&mut patt, pattern.len() as u32);
        patt.extend_from_slice(&pattern);
        let mut file = with_presets(&desc);
        while !file.len().is_multiple_of(4) {
            file.push(0);
        }
        file.extend_from_slice(b"8BIMpatt");
        put32(&mut file, patt.len() as u32);
        file.extend_from_slice(&patt);
        let brushes = read_brushes(&file).unwrap();
        assert_eq!(brushes.len(), 2);
        let t = brushes[0].texture.as_ref().unwrap();
        assert_eq!((t.name.as_str(), t.width, t.height, t.pixels.as_slice()), ("Paper", 2, 2, &[10, 20, 30, 40][..]));
        assert_eq!((t.scale, t.depth, t.invert), (Some(0.5), Some(0.4), true));
        assert_eq!((t.min_depth, t.brightness, t.contrast, t.blend.as_deref()), (Some(0.17), 30, -20, Some("Hght")));
        // the other's texture is off
        assert_eq!(brushes[1].texture, None);
        crate::damaged::survives("ABR with a pattern", &file, |b| {
            let _ = read_brushes(b);
        });
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
    fn computed_presets_come_round_and_a_dual_brush_is_not_one_of_them() {
        let number = |out: &mut Vec<u8>, key: &[u8], v: f64| {
            out.extend_from_slice(key);
            out.extend_from_slice(&v.to_be_bytes());
        };
        let mut desc = Vec::new();
        // a computed round preset, whose dual brush is the sample (not a brush of its own)
        desc.extend_from_slice(b"brushPreset");
        desc.extend_from_slice(b"Nm  TEXT");
        unicode(&mut desc, "Hard Round");
        desc.extend_from_slice(b"BrshObjccomputedBrush");
        number(&mut desc, b"HrdnUntF#Prc", 50.0);
        number(&mut desc, b"AnglUntF#Ang", 30.0);
        number(&mut desc, b"RndnUntF#Prc", 40.0);
        number(&mut desc, b"SpcnUntF#Prc", 25.0);
        desc.extend_from_slice(b"dualBrushObjcdualBrushsampledBrush");
        number(&mut desc, b"AnglUntF#Ang", 5.0);
        desc.extend_from_slice(b"sampledDataTEXT");
        unicode(&mut desc, "tip-one");
        // the sample only its dual brush names isn't a brush of its own
        let alone = read_brushes(&with_presets(&desc)).unwrap();
        assert_eq!(alone.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Hard Round"]);
        // a sampled preset turned a quarter
        desc.extend_from_slice(b"brushPreset");
        desc.extend_from_slice(b"Nm  TEXT");
        unicode(&mut desc, "Leaf");
        desc.extend_from_slice(b"BrshObjcsampledBrush");
        number(&mut desc, b"AnglUntF#Ang", 90.0);
        number(&mut desc, b"RndnUntF#Prc", 100.0);
        desc.extend_from_slice(b"sampledDataTEXT");
        unicode(&mut desc, "tip-one");
        let brushes = read_brushes(&with_presets(&desc)).unwrap();
        let names: Vec<&str> = brushes.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Hard Round", "Leaf"]);
        let round = &brushes[0];
        assert!(round.tip.is_none());
        assert_eq!((round.hardness, round.roundness, round.spacing), (Some(0.5), Some(0.4), Some(0.25)));
        assert!((round.angle - 30f32.to_radians()).abs() < 1e-6);
        let leaf = &brushes[1];
        assert_eq!((leaf.tip.as_ref().map(|t| t.width), leaf.roundness, leaf.hardness), (Some(3), Some(1.0), None));
        assert!((leaf.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // the tips alone: the sampled one
        assert_eq!(read(&with_presets(&desc)).unwrap().len(), 1);
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
