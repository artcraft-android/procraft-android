//! Reading GIMP brushes: a brush (`.gbr`, versions 1 and 2) is a header, its name and its image
//! (grey, how much each pixel paints; or RGBA, whose alpha paints); a pipe (`.gih`) is a name
//! line, a line of its parameters and its brushes one after another. Each image is read as the
//! tips ABR files give. A generated brush (`.vbr`) is text: a shape and a few numbers. A pattern
//! (`.pat`) is a header, its name and its pixels.

use crate::abr::AbrTip;

/// The largest tip side read, pixels.
const MOST_SIDE: u32 = 5000;
/// The most brushes read from a pipe.
const MOST_IN_PIPE: usize = 256;

/// The tips in a GIMP brush or pipe file, in order; none if it is neither.
pub fn read(bytes: &[u8]) -> Option<Vec<AbrTip>> {
    if let Some((tip, _)) = brush(bytes) {
        return Some(vec![tip]);
    }
    pipe(bytes, MOST_IN_PIPE)
}

/// The first tip in a GIMP brush or pipe file that paints anything (a pipe's brushes read only
/// until one does: an animated one may start blank), else its first; none if it is neither.
pub fn read_first(bytes: &[u8]) -> Option<AbrTip> {
    if let Some((tip, _)) = brush(bytes) {
        return Some(tip);
    }
    let mut first = None;
    for tip in pipe_brushes(bytes)?.take(MOST_IN_PIPE) {
        if tip.pixels.iter().any(|&v| v > 0) {
            return Some(tip);
        }
        first.get_or_insert(tip);
    }
    first
}

/// A GIMP pattern (`.pat`) as grey, each pixel its lightness times its alpha; none if it isn't
/// one. Its header: its size, version 1, the width, the height, bytes a pixel (grey, grey and
/// alpha, RGB or RGBA) and `GPAT`; then its name up to the header's end, then the pixels.
pub fn read_pattern(bytes: &[u8]) -> Option<AbrTip> {
    let word = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
    let header = word(0)? as usize;
    let (version, width, height, depth) = (word(4)?, word(8)?, word(12)?, word(16)?);
    if version != 1 || bytes.get(20..24)? != b"GPAT" || header < 24 {
        return None;
    }
    if width == 0 || height == 0 || width > MOST_SIDE || height > MOST_SIDE || !(1..=4).contains(&depth) {
        return None;
    }
    let name = bytes.get(24..header)?;
    let name = String::from_utf8_lossy(name.split(|&b| b == 0).next().unwrap_or_default()).trim().to_owned();
    let depth = depth as usize;
    let image = bytes.get(header..header.checked_add(width as usize * height as usize * depth)?)?;
    let pixels = image
        .chunks_exact(depth)
        .map(|px| {
            let get = |k: usize| u32::from(px.get(k).copied().unwrap_or(0));
            let (grey, alpha) = match depth {
                1 => (get(0), 255),
                2 => (get(0), get(1)),
                3 => ((get(0) * 299 + get(1) * 587 + get(2) * 114) / 1000, 255),
                _ => ((get(0) * 299 + get(1) * 587 + get(2) * 114) / 1000, get(3)),
            };
            (grey * alpha / 255) as u8
        })
        .collect();
    Some(AbrTip { name, spacing: None, width, height, pixels })
}

/// A brush at the start of `bytes`, and how many bytes it takes.
fn brush(bytes: &[u8]) -> Option<(AbrTip, usize)> {
    let word = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes);
    let header = word(0)? as usize;
    let version = word(4)?;
    let (width, height, depth) = (word(8)?, word(12)?, word(16)?);
    // version 2 has the magic word and the spacing (per cent of the size) after the depth
    let spacing = match version {
        1 if header >= 20 => None,
        2 | 3 if header >= 28 && bytes.get(20..24)? == b"GIMP" => Some(word(24)? as f32 / 100.0),
        _ => return None,
    };
    if width == 0 || height == 0 || width > MOST_SIDE || height > MOST_SIDE || !(depth == 1 || depth == 4) {
        return None;
    }
    let fixed = if version == 1 { 20 } else { 28 };
    let name = bytes.get(fixed..header)?;
    let name = String::from_utf8_lossy(name.split(|&b| b == 0).next().unwrap_or_default()).trim().to_owned();
    let size = width as usize * height as usize * depth as usize;
    let image = bytes.get(header..header.checked_add(size)?)?;
    let pixels = if depth == 1 { image.to_vec() } else { image.iter().skip(3).step_by(4).copied().collect() };
    let tip = AbrTip { name, spacing: spacing.filter(|s| *s > 0.0), width, height, pixels };
    Some((tip, header + size))
}

/// A GIMP generated brush (`.vbr`): a shape made from a few numbers, kept as text.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub name: String,
    pub shape: GeneratedShape,
    /// Space between stamps as a share of the size.
    pub spacing: f32,
    /// The edge, 0 soft to 1 hard.
    pub hardness: f32,
    /// How many times wider than tall it is (1 to 20); with more than two spikes, how sharp
    /// they are.
    pub aspect: f32,
    /// Its angle, radians.
    pub angle: f32,
    /// How many points it has around: two is the plain shape, more a star of the shape.
    pub spikes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedShape {
    Circle,
    Square,
    Diamond,
}

/// The side a generated brush's drawn tip is made at, pixels.
const DRAWN_SIDE: usize = 128;

/// The most spikes a generated brush is drawn with.
const MOST_SPIKES: u32 = 20;

/// The generated brush in `bytes`; none if it isn't one. Version 1.0 is a circle (its spacing,
/// radius, hardness, aspect and angle a line each); 1.5 names its shape first and has the number
/// of spikes after the radius.
pub fn read_generated(bytes: &[u8]) -> Option<Generated> {
    let text = String::from_utf8_lossy(bytes.get(..bytes.len().min(4096))?);
    let mut lines = text.lines().map(str::trim);
    if lines.next()? != "GIMP-VBR" {
        return None;
    }
    let version = lines.next()?;
    let name = lines.next()?.to_owned();
    let number = |line: Option<&str>| line?.parse::<f32>().ok().filter(|v| v.is_finite());
    let shape = match version {
        "1.0" => GeneratedShape::Circle,
        "1.5" => match lines.next()? {
            "circle" => GeneratedShape::Circle,
            "square" => GeneratedShape::Square,
            "diamond" => GeneratedShape::Diamond,
            _ => return None,
        },
        _ => return None,
    };
    let spacing = number(lines.next())?;
    let _radius = number(lines.next())?;
    let spikes = if version == "1.5" { number(lines.next())? } else { 2.0 };
    let (hardness, aspect, angle) = (number(lines.next())?, number(lines.next())?, number(lines.next())?);
    Some(Generated {
        name,
        shape,
        spacing: (spacing / 100.0).max(0.0),
        hardness: hardness.clamp(0.0, 1.0),
        aspect: aspect.clamp(1.0, 20.0),
        angle: angle.to_radians(),
        spikes: (spikes.round() as u32).clamp(2, MOST_SPIKES),
    })
}

/// A generated brush's tip drawn, falling off as GIMP's do ([`generated_falloff`]); none for a
/// plain circle our round tip stands in for (one that paints half no nearer the middle than
/// halfway out, as ours can: see [`generated_half`]).
///
/// The shape is the same in each of `spikes` slices around: a point's angle is folded into the
/// first half slice, its distance up from that slice's middle taken `aspect` times, and the shape
/// measured there. Two spikes is the plain shape `aspect` times as wide as tall; more make a star
/// whose points `aspect` sharpens.
pub fn generated_tip(g: &Generated) -> Option<AbrTip> {
    let reach: fn(f32, f32) -> f32 = match g.shape {
        GeneratedShape::Circle if g.spikes == 2 && generated_half(g.hardness) >= 0.5 => return None,
        GeneratedShape::Circle => f32::hypot,
        GeneratedShape::Square => f32::max,
        GeneratedShape::Diamond => |x, y| x + y,
    };
    // the plain shape is drawn as wide as it is, a star square
    let squeeze = if g.spikes == 2 { g.aspect } else { 1.0 };
    let (w, h) = (DRAWN_SIDE, ((DRAWN_SIDE as f32 / squeeze).round() as usize).max(1));
    let slice = std::f32::consts::TAU / g.spikes as f32;
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let py = ((y as f32 + 0.5) / h as f32 * 2.0 - 1.0) / squeeze;
        for x in 0..w {
            let px = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            let folded = py.atan2(px).rem_euclid(slice);
            let folded = folded.min(slice - folded);
            let r = px.hypot(py);
            let d = reach(r * folded.cos(), r * folded.sin() * g.aspect);
            pixels.push((generated_falloff(d, g.hardness) * 255.0).round() as u8);
        }
    }
    Some(AbrTip { name: g.name.clone(), spacing: Some(g.spacing), width: w as u32, height: h as u32, pixels })
}

/// How much a generated brush paints `t` of the way out (1 its edge, none past it), as GIMP's do:
/// an S from all of it to none, `1 - 2f^2` to the half and `2(1-f)^2` after, of `t` raised to
/// `0.4 / (1 - hardness)` (so the harder, the later it falls).
pub fn generated_falloff(t: f32, hardness: f32) -> f32 {
    if t >= 1.0 {
        return 0.0;
    }
    let soft = 1.0 - hardness.clamp(0.0, 1.0);
    let f = if soft < 4e-7 { 0.0 } else { t.max(0.0).powf(0.4 / soft) };
    if f < 0.5 { 1.0 - 2.0 * f * f } else { 2.0 * (1.0 - f) * (1.0 - f) }
}

/// How far out a generated brush of `hardness` paints half ([`generated_falloff`]).
pub fn generated_half(hardness: f32) -> f32 {
    0.5f32.powf((1.0 - hardness.clamp(0.0, 1.0)) / 0.4)
}

/// A pipe: its name, a line starting with how many brushes it has, then the brushes (the first
/// `most` of them read).
fn pipe(bytes: &[u8], most: usize) -> Option<Vec<AbrTip>> {
    let tips: Vec<AbrTip> = pipe_brushes(bytes)?.take(most).collect();
    (!tips.is_empty()).then_some(tips)
}

/// A pipe's brushes, each read as it is come to; none if it isn't one.
fn pipe_brushes(bytes: &[u8]) -> Option<impl Iterator<Item = AbrTip> + '_> {
    let first = bytes.iter().position(|&b| b == b'\n')?;
    let second = first + 1 + bytes.get(first + 1..)?.iter().position(|&b| b == b'\n')?;
    let name = String::from_utf8_lossy(bytes.get(..first)?).trim().to_owned();
    let line = String::from_utf8_lossy(bytes.get(first + 1..second)?).into_owned();
    let count: usize = line.split_whitespace().next()?.parse().ok()?;
    let (mut at, mut n) = (second + 1, 0);
    Some(std::iter::from_fn(move || {
        if n >= count {
            return None;
        }
        let (mut tip, used) = brush(bytes.get(at..)?)?;
        (n, at) = (n + 1, at + used);
        // the pipe's name for its only brush; numbered for those with none of their own
        if count == 1 {
            tip.name = name.clone();
        } else if tip.name.is_empty() {
            tip.name = format!("{name} {n}");
        }
        Some(tip)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gbr(version: u32, name: &str, width: u32, height: u32, depth: u32, spacing: u32) -> Vec<u8> {
        let fixed: u32 = if version == 1 { 20 } else { 28 };
        let header = fixed + name.len() as u32 + 1;
        let mut out = Vec::new();
        for v in [header, version, width, height, depth] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        if version == 2 {
            out.extend_from_slice(b"GIMP");
            out.extend_from_slice(&spacing.to_be_bytes());
        }
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        for i in 0..width * height {
            if depth == 1 {
                out.push((i * 40) as u8);
            } else {
                out.extend_from_slice(&[255, 0, 0, (i * 40) as u8]);
            }
        }
        out
    }

    #[test]
    fn a_brush_reads_with_its_name_spacing_and_image() {
        let tips = read(&gbr(2, "Pencil", 3, 2, 1, 25)).unwrap();
        assert_eq!(tips.len(), 1);
        let t = &tips[0];
        assert_eq!((t.name.as_str(), t.spacing, t.width, t.height), ("Pencil", Some(0.25), 3, 2));
        assert_eq!(t.pixels, [0, 40, 80, 120, 160, 200]);
        // a colour brush paints by its alpha; version 1 has no spacing
        let t = &read(&gbr(1, "Old", 2, 2, 4, 0)).unwrap()[0];
        assert_eq!((t.spacing, t.pixels.as_slice()), (None, &[0, 40, 80, 120][..]));
    }

    #[test]
    fn a_pipe_reads_each_brush() {
        let mut file = b"Leaves\n2 ncells:2 cellwidth:3 cellheight:2 step:100 dim:1 rank0:2 sel0:random\n".to_vec();
        file.extend(gbr(2, "", 3, 2, 1, 50));
        file.extend(gbr(2, "Leaf", 3, 2, 4, 50));
        let tips = read(&file).unwrap();
        let names: Vec<&str> = tips.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Leaves 1", "Leaf"]);
        assert_eq!(tips[1].spacing, Some(0.5));
    }

    #[test]
    fn a_pipes_first_brush_reads_alone() {
        let mut file = b"Leaves\n2 ncells:2\n".to_vec();
        file.extend(gbr(2, "First", 3, 2, 1, 50));
        file.extend(gbr(2, "Second", 3, 2, 1, 50));
        assert_eq!(read_first(&file).unwrap().name, "First");
        assert_eq!(read_first(&gbr(2, "Alone", 3, 2, 1, 50)).unwrap().name, "Alone");
        assert!(read_first(b"nothing").is_none());
        // an animated pipe that starts blank: its first brush that paints
        let blank = |name: &str| {
            let mut b = gbr(2, name, 3, 2, 1, 50);
            let len = b.len();
            b[len - 6..].fill(0);
            b
        };
        let mut file = b"Stars\n3 ncells:3\n".to_vec();
        file.extend(blank("Empty"));
        file.extend(gbr(2, "Star", 3, 2, 1, 50));
        file.extend(gbr(2, "Bigger", 3, 2, 1, 50));
        assert_eq!(read_first(&file).unwrap().name, "Star");
        // all blank: the first
        let mut file = b"Blank\n2 ncells:2\n".to_vec();
        file.extend(blank("One"));
        file.extend(blank("Two"));
        assert_eq!(read_first(&file).unwrap().name, "One");
    }

    #[test]
    fn a_pattern_reads_as_grey_by_its_lightness_and_alpha() {
        let pattern = |depth: u32, pixels: &[u8]| {
            let name = b"Paper\0";
            let mut out = Vec::new();
            for v in [24 + name.len() as u32, 1, 2, 1, depth] {
                out.extend_from_slice(&v.to_be_bytes());
            }
            out.extend_from_slice(b"GPAT");
            out.extend_from_slice(name);
            out.extend_from_slice(pixels);
            out
        };
        let grey = read_pattern(&pattern(1, &[0, 200])).unwrap();
        assert_eq!(
            (grey.name.as_str(), grey.width, grey.height, grey.pixels.as_slice()),
            ("Paper", 2, 1, &[0, 200][..])
        );
        // white and half seen through; pure red, its lightness
        assert_eq!(read_pattern(&pattern(4, &[255, 255, 255, 128, 255, 0, 0, 255])).unwrap().pixels, [128, 76]);
        assert_eq!(read_pattern(&pattern(2, &[200, 255, 200, 0])).unwrap().pixels, [200, 0]);
        assert_eq!(read_pattern(&pattern(3, &[0, 0, 255, 10, 10, 10])).unwrap().pixels, [29, 10]);
        // too short, or not a pattern
        assert!(read_pattern(&pattern(4, &[1, 2, 3])).is_none());
        assert!(read_pattern(&gbr(2, "Pencil", 3, 2, 1, 25)).is_none());
        crate::damaged::survives("GIMP pattern", &pattern(3, &[1, 2, 3, 4, 5, 6]), |b| {
            let _ = read_pattern(b);
        });
    }

    #[test]
    fn a_generated_brush_falls_off_as_gimp_s_do() {
        let near = |a: f32, b: f32| (a - b).abs() < 2e-3;
        // half the hardness: t^0.8 through the S, a third or so at half way out
        assert!(near(generated_falloff(0.5, 0.5), 2.0 * (1.0 - 0.5f32.powf(0.8)).powi(2)));
        assert!(near(generated_falloff(0.0, 0.0), 1.0) && generated_falloff(1.0, 0.0) == 0.0);
        // all hard: solid to its edge
        assert!(generated_falloff(0.99, 1.0) == 1.0 && generated_falloff(1.0, 1.0) == 0.0);
        // half where it says
        for hardness in [0.0, 0.5, 0.9] {
            assert!(near(generated_falloff(generated_half(hardness), hardness), 0.5), "{hardness}");
        }
    }

    #[test]
    fn generated_brushes_read_in_both_versions() {
        let old =
            read_generated(b"GIMP-VBR\n1.0\n2. Hardness 050\n10.000000\n25.000000\n0.500000\n2.5\n30.0\n").unwrap();
        assert_eq!(
            (old.name.as_str(), old.shape, old.hardness, old.aspect),
            ("2. Hardness 050", GeneratedShape::Circle, 0.5, 2.5)
        );
        assert!((old.spacing - 0.1).abs() < 1e-6 && (old.angle - 30f32.to_radians()).abs() < 1e-6);
        // softer than our round tip goes (GIMP's paints half at 0.42 of the way out): drawn; a
        // harder one is our round tip
        assert!(generated_tip(&old).is_some());
        assert!(generated_tip(&Generated { hardness: 0.75, ..old.clone() }).is_none());
        let block =
            read_generated(b"GIMP-VBR\r\n1.5\r\n2. Block 01\r\nsquare\r\n10.0\r\n25.0\r\n2\r\n0.95\r\n4.0\r\n0.0\r\n")
                .unwrap();
        assert_eq!((block.shape, block.aspect), (GeneratedShape::Square, 4.0));
        // a square four times as wide as tall, near solid to two thirds of the way out, fading to
        // its edge
        let tip = generated_tip(&block).unwrap();
        assert_eq!((tip.width, tip.height), (128, 32));
        assert!(tip.pixels[16 * 128 + 64] == 255 && tip.pixels[16 * 128 + 20] > 240);
        assert!(tip.pixels[16 * 128 + 4] < 160 && tip.pixels[16 * 128] < 10);
        // a diamond's corners are empty
        let diamond = Generated { shape: GeneratedShape::Diamond, aspect: 1.0, ..block };
        let tip = generated_tip(&diamond).unwrap();
        assert_eq!((tip.pixels[64 * 128 + 64], tip.pixels[0]), (255, 0));
        // five spikes: a square star, solid out along a point, empty between two
        let star = read_generated(b"GIMP-VBR\n1.5\n2. Star\ndiamond\n50.0\n25.0\n5\n1.0\n2.5\n17.5\n").unwrap();
        assert_eq!(star.spikes, 5);
        let tip = generated_tip(&star).unwrap();
        assert_eq!((tip.width, tip.height), (128, 128));
        let at = |r: f32, degrees: f32| {
            let (s, c) = degrees.to_radians().sin_cos();
            let (x, y) = ((64.0 + r * 64.0 * c) as usize, (64.0 + r * 64.0 * s) as usize);
            tip.pixels[y * 128 + x]
        };
        assert_eq!((at(0.9, 0.0), at(0.9, 72.0), at(0.6, 36.0), at(0.6, 108.0)), (255, 255, 0, 0));
        // a circle with spikes is drawn; a plain one is a round tip
        assert!(generated_tip(&Generated { shape: GeneratedShape::Circle, ..star }).is_some());
        assert!(read_generated(b"GIMP-VBR\n2.0\nx\n").is_none());
        crate::damaged::survives("GIMP generated brush", b"GIMP-VBR\n1.5\nB\ndiamond\n10\n25\n2\n0.5\n1\n0\n", |b| {
            let _ = read_generated(b).as_ref().map(generated_tip);
        });
    }

    #[test]
    fn other_files_and_damaged_ones_are_not_brushes() {
        assert!(read(b"nothing at all").is_none());
        assert!(read(&gbr(2, "Huge", 6000, 1, 1, 10)[..40]).is_none());
        let good = gbr(2, "Pencil", 3, 2, 1, 25);
        crate::damaged::survives("GIMP brush", &good, |b| {
            let _ = read(b);
        });
        let mut pipe = b"Leaves\n1 ncells:1\n".to_vec();
        pipe.extend(&good);
        crate::damaged::survives("GIMP pipe", &pipe, |b| {
            let _ = read(b);
        });
    }
}
