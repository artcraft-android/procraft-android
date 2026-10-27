//! Reading GIMP brushes: a brush (`.gbr`, versions 1 and 2) is a header, its name and its image
//! (grey, how much each pixel paints; or RGBA, whose alpha paints); a pipe (`.gih`) is a name
//! line, a line of its parameters and its brushes one after another. Each image is read as the
//! tips ABR files give.

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
