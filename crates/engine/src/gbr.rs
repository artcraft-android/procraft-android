//! Reading GIMP brushes: a brush (`.gbr`, versions 1 and 2) is a header, its name and its image
//! (grey, how much each pixel paints; or RGBA, whose alpha paints); a pipe (`.gih`) is a name
//! line, a line of its parameters and its brushes one after another. Each image is read as the
//! tips ABR files give. A generated brush (`.vbr`) is text: a shape and a few numbers.

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
    pipe(bytes)
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
    /// How many times wider than tall it is (1 to 20).
    pub aspect: f32,
    /// Its angle, radians.
    pub angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedShape {
    Circle,
    Square,
    Diamond,
}

/// The side a generated brush's drawn tip is made at, pixels.
const DRAWN_SIDE: usize = 128;

/// The generated brush in `bytes`; none if it isn't one. Version 1.0 is a circle (its spacing,
/// radius, hardness, aspect and angle a line each); 1.5 names its shape first and has the number
/// of spikes after the radius (drawn as two here: the plain shape).
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
    if version == "1.5" {
        let _spikes = number(lines.next())?;
    }
    let (hardness, aspect, angle) = (number(lines.next())?, number(lines.next())?, number(lines.next())?);
    Some(Generated {
        name,
        shape,
        spacing: (spacing / 100.0).max(0.0),
        hardness: hardness.clamp(0.0, 1.0),
        aspect: aspect.clamp(1.0, 20.0),
        angle: angle.to_radians(),
    })
}

/// A generated brush's tip drawn, `aspect` times as wide as tall, solid to its hardness of the
/// way out and fading to the edge; none for a circle (a round tip, not an image).
pub fn generated_tip(g: &Generated) -> Option<AbrTip> {
    let reach: fn(f32, f32) -> f32 = match g.shape {
        GeneratedShape::Circle => return None,
        GeneratedShape::Square => |x, y| x.max(y),
        GeneratedShape::Diamond => |x, y| x + y,
    };
    let (w, h) = (DRAWN_SIDE, ((DRAWN_SIDE as f32 / g.aspect).round() as usize).max(1));
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let ty = ((y as f32 + 0.5) / h as f32 * 2.0 - 1.0).abs();
        for x in 0..w {
            let tx = ((x as f32 + 0.5) / w as f32 * 2.0 - 1.0).abs();
            let d = reach(tx, ty);
            let v = if d <= g.hardness {
                1.0
            } else if g.hardness >= 1.0 {
                0.0
            } else {
                ((1.0 - d) / (1.0 - g.hardness)).clamp(0.0, 1.0)
            };
            pixels.push((v * 255.0).round() as u8);
        }
    }
    Some(AbrTip { name: g.name.clone(), spacing: Some(g.spacing), width: w as u32, height: h as u32, pixels })
}

/// A pipe: its name, a line starting with how many brushes it has, then the brushes.
fn pipe(bytes: &[u8]) -> Option<Vec<AbrTip>> {
    let first = bytes.iter().position(|&b| b == b'\n')?;
    let second = first + 1 + bytes.get(first + 1..)?.iter().position(|&b| b == b'\n')?;
    let name = String::from_utf8_lossy(bytes.get(..first)?).trim().to_owned();
    let line = String::from_utf8_lossy(bytes.get(first + 1..second)?).into_owned();
    let count: usize = line.split_whitespace().next()?.parse().ok()?;
    let mut at = second + 1;
    let mut tips = Vec::new();
    for n in 1..=count.min(MOST_IN_PIPE) {
        let Some((mut tip, used)) = brush(bytes.get(at..)?) else { break };
        // the pipe's name for its only brush; numbered for those with none of their own
        if count == 1 {
            tip.name = name.clone();
        } else if tip.name.is_empty() {
            tip.name = format!("{name} {n}");
        }
        tips.push(tip);
        at += used;
    }
    (!tips.is_empty()).then_some(tips)
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
    fn generated_brushes_read_in_both_versions() {
        let old =
            read_generated(b"GIMP-VBR\n1.0\n2. Hardness 050\n10.000000\n25.000000\n0.500000\n2.5\n30.0\n").unwrap();
        assert_eq!(
            (old.name.as_str(), old.shape, old.hardness, old.aspect),
            ("2. Hardness 050", GeneratedShape::Circle, 0.5, 2.5)
        );
        assert!((old.spacing - 0.1).abs() < 1e-6 && (old.angle - 30f32.to_radians()).abs() < 1e-6);
        assert!(generated_tip(&old).is_none());
        let block =
            read_generated(b"GIMP-VBR\r\n1.5\r\n2. Block 01\r\nsquare\r\n10.0\r\n25.0\r\n2\r\n0.95\r\n4.0\r\n0.0\r\n")
                .unwrap();
        assert_eq!((block.shape, block.aspect), (GeneratedShape::Square, 4.0));
        // a square four times as wide as tall, solid to its edge's last twentieth
        let tip = generated_tip(&block).unwrap();
        assert_eq!((tip.width, tip.height), (128, 32));
        assert_eq!((tip.pixels[16 * 128 + 64], tip.pixels[16 * 128 + 4]), (255, 255));
        assert!(tip.pixels[16 * 128] < 128);
        // a diamond's corners are empty
        let diamond = Generated { shape: GeneratedShape::Diamond, aspect: 1.0, ..block };
        let tip = generated_tip(&diamond).unwrap();
        assert_eq!((tip.pixels[64 * 128 + 64], tip.pixels[0]), (255, 0));
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
