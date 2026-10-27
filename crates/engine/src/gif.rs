//! Reading animated GIFs: each frame drawn over the canvas as the frames before it left it (kept,
//! cleared to nothing, or put back as it was), with how long it shows. A damaged file gives the
//! frames before the damage.

/// The most pixels a GIF's canvas may have.
const MOST_PIXELS: u64 = 4096 * 4096;
/// Frames shown for at most a hundredth of a second play, as browsers play them, at this.
const SHORTEST_MS: u32 = 100;

/// The canvas's size, if `bytes` is a GIF of a size read here.
pub fn size(bytes: &[u8]) -> Option<(u32, u32)> {
    screen(bytes)
}

/// How many frames the GIF `bytes` has (none if it isn't one), without decoding them.
pub fn frame_count(bytes: &[u8]) -> usize {
    let mut count = 0;
    walk(bytes, |_, _| {
        count += 1;
        Some(())
    });
    count
}

/// Gives `frame` each frame of the GIF `bytes` in order (straight RGBA, the canvas's size, and how
/// long it shows, ms) until it returns false. The canvas's size and how many frames it was given,
/// or none if it isn't a GIF.
pub fn frames(bytes: &[u8], mut frame: impl FnMut(&[u8], u32) -> bool) -> Option<(u32, u32, usize)> {
    let (w, h) = screen(bytes)?;
    let (wu, hu) = (w as usize, h as usize);
    let mut canvas = vec![0u8; wu * hu * 4];
    let mut given = 0;
    let mut stopped = false;
    walk(bytes, |image, data| {
        if stopped {
            return None;
        }
        // a frame may be no bigger than a canvas may (it can be bigger than its own canvas)
        let count = u64::from(image.width) * u64::from(image.height);
        let indices = lzw(data, image.min_code, usize::try_from(count).ok().filter(|_| count <= MOST_PIXELS)?)?;
        let before = (image.disposal == 3).then(|| canvas.clone());
        draw(&mut canvas, wu, hu, &image, &indices);
        given += 1;
        let ms = if image.delay <= 1 { SHORTEST_MS } else { u32::from(image.delay) * 10 };
        if !frame(&canvas, ms) {
            stopped = true;
            return None;
        }
        match (image.disposal, before) {
            (2, _) => clear(&mut canvas, wu, hu, &image),
            (3, Some(b)) => canvas = b,
            _ => {}
        }
        Some(())
    });
    Some((w, h, given))
}

/// A frame's place, palette and how it shows (from the control block before it).
struct Image<'a> {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    interlaced: bool,
    palette: &'a [u8],
    min_code: u8,
    transparent: Option<u8>,
    delay: u16,
    disposal: u8,
}

/// The canvas's size, if `bytes` is a GIF of a size read here.
fn screen(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut r = Reader { b: bytes, at: 0 };
    let sig = r.take(6)?;
    if sig != b"GIF87a" && sig != b"GIF89a" {
        return None;
    }
    let (w, h) = (u32::from(r.u16()?), u32::from(r.u16()?));
    (w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MOST_PIXELS).then_some((w, h))
}

/// Calls `each` with every frame and its compressed data, in order, until the end, damage, or
/// `each` giving none.
fn walk<'a>(bytes: &'a [u8], mut each: impl FnMut(Image<'a>, &[u8]) -> Option<()>) {
    if screen(bytes).is_none() {
        return;
    }
    let mut r = Reader { b: bytes, at: 10 };
    let mut go = || -> Option<()> {
        let packed = r.u8()?;
        r.take(2)?;
        let global = if packed & 0x80 != 0 { Some(r.take(3 << ((packed & 7) + 1))?) } else { None };
        let (mut transparent, mut delay, mut disposal) = (None, 0, 0);
        loop {
            match r.u8()? {
                0x21 => {
                    let label = r.u8()?;
                    let block = r.blocks()?;
                    if label == 0xF9 && block.len() >= 4 {
                        let flags = *block.first()?;
                        disposal = (flags >> 2) & 7;
                        delay = u16::from_le_bytes([*block.get(1)?, *block.get(2)?]);
                        transparent = (flags & 1 != 0).then_some(*block.get(3)?);
                    }
                }
                0x2C => {
                    let (left, top, width, height) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?);
                    let packed = r.u8()?;
                    let local = if packed & 0x80 != 0 { Some(r.take(3 << ((packed & 7) + 1))?) } else { None };
                    let min_code = r.u8()?;
                    let data = r.blocks()?;
                    let palette = local.or(global)?;
                    let image = Image {
                        left,
                        top,
                        width,
                        height,
                        interlaced: packed & 0x40 != 0,
                        palette,
                        min_code,
                        transparent,
                        delay,
                        disposal,
                    };
                    each(image, &data)?;
                    // a control block is for the next frame only
                    (transparent, delay, disposal) = (None, 0, 0);
                }
                _ => return None,
            }
        }
    };
    let _ = go();
}

/// Draws a frame's colours over the canvas (its transparent ones leave what is there).
fn draw(canvas: &mut [u8], w: usize, h: usize, image: &Image<'_>, indices: &[u8]) {
    let (fw, fh) = (usize::from(image.width), usize::from(image.height));
    // interlaced: every eighth row from 0, every eighth from 4, every fourth from 2, the rest
    let rows: Vec<usize> = if image.interlaced {
        (0..fh).step_by(8).chain((4..fh).step_by(8)).chain((2..fh).step_by(4)).chain((1..fh).step_by(2)).collect()
    } else {
        (0..fh).collect()
    };
    for (i, row) in rows.into_iter().enumerate() {
        let y = usize::from(image.top) + row;
        if y >= h {
            continue;
        }
        for col in 0..fw {
            let x = usize::from(image.left) + col;
            let Some(&index) = indices.get(i * fw + col) else { return };
            if x >= w || Some(index) == image.transparent {
                continue;
            }
            let at = usize::from(index) * 3;
            if let (Some(&[r, g, b]), Some(px)) =
                (image.palette.get(at..at + 3), canvas.get_mut((y * w + x) * 4..(y * w + x) * 4 + 4))
            {
                px.copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
}

/// Clears a frame's place to nothing.
fn clear(canvas: &mut [u8], w: usize, h: usize, image: &Image<'_>) {
    let (x0, y0) = (usize::from(image.left).min(w), usize::from(image.top).min(h));
    let (x1, y1) = ((x0 + usize::from(image.width)).min(w), (y0 + usize::from(image.height)).min(h));
    for y in y0..y1 {
        if let Some(row) = canvas.get_mut((y * w + x0) * 4..(y * w + x1) * 4) {
            row.fill(0);
        }
    }
}

/// The colour indices `data` holds (LZW, codes from `min_code + 1` bits growing to 12), `count` of
/// them: short data padded with the first colour.
fn lzw(data: &[u8], min_code: u8, count: usize) -> Option<Vec<u8>> {
    if !(1..=11).contains(&min_code) {
        return None;
    }
    const NONE: u16 = u16::MAX;
    let clear = 1u16 << min_code;
    let end = clear + 1;
    // each code: the code before its last colour, its last colour, and its first
    let mut prefix = vec![NONE; 4096];
    let mut suffix = vec![0u8; 4096];
    let mut first = vec![0u8; 4096];
    for c in 0..clear {
        let c8 = (c & 0xFF) as u8;
        *suffix.get_mut(usize::from(c))? = c8;
        *first.get_mut(usize::from(c))? = c8;
    }
    let mut out = Vec::with_capacity(count);
    let (mut next, mut size, mut prev) = (end + 1, u32::from(min_code) + 1, None::<u16>);
    let (mut acc, mut bits, mut at) = (0u32, 0u32, 0usize);
    let mut string = Vec::with_capacity(4096);
    'codes: while out.len() < count {
        while bits < size {
            let Some(&byte) = data.get(at) else { break 'codes };
            acc |= u32::from(byte) << bits;
            bits += 8;
            at += 1;
        }
        let code = (acc & ((1 << size) - 1)) as u16;
        acc >>= size;
        bits -= size;
        if code == clear {
            (next, size, prev) = (end + 1, u32::from(min_code) + 1, None);
            continue;
        }
        if code == end {
            break;
        }
        let Some(p) = prev else {
            if code >= clear {
                break;
            }
            out.push((code & 0xFF) as u8);
            prev = Some(code);
            continue;
        };
        // a code not yet made is the one before with its own first colour again
        let (start, tail) = if code < next {
            (code, None)
        } else if code == next {
            (p, Some(*first.get(usize::from(p))?))
        } else {
            break;
        };
        string.clear();
        string.extend(tail);
        let mut c = start;
        loop {
            string.push(*suffix.get(usize::from(c))?);
            if c < clear {
                break;
            }
            c = *prefix.get(usize::from(c))?;
            if c == NONE || string.len() > 4096 {
                break 'codes;
            }
        }
        out.extend(string.iter().rev().take(count - out.len()));
        if next < 4096 {
            let n = usize::from(next);
            *prefix.get_mut(n)? = p;
            *suffix.get_mut(n)? = *first.get(usize::from(start))?;
            *first.get_mut(n)? = *first.get(usize::from(p))?;
            next += 1;
            if u32::from(next) == 1 << size && size < 12 {
                size += 1;
            }
        }
        prev = Some(code);
    }
    out.resize(count, 0);
    Some(out)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_le_bytes([*b.first()?, *b.get(1)?]))
    }

    /// Sub-blocks (each a length, then that many bytes) up to an empty one, joined.
    fn blocks(&mut self) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let n = usize::from(self.u8()?);
            if n == 0 {
                return Some(out);
            }
            out.extend_from_slice(self.take(n)?);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LZW codes for `indices` (no compression: a clear code before every few, so the code size
    /// stays `min + 1`), packed as a GIF packs them.
    fn compress(indices: &[u8], min: u8) -> Vec<u8> {
        let clear = 1u32 << min;
        let size = u32::from(min) + 1;
        let mut codes = vec![clear];
        for (i, &v) in indices.iter().enumerate() {
            if i > 0 && i % ((1 << size) - clear as usize - 2).max(1) == 0 {
                codes.push(clear);
            }
            codes.push(u32::from(v));
        }
        codes.push(clear + 1);
        let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
        for c in codes {
            acc |= c << bits;
            bits += size;
            while bits >= 8 {
                out.push((acc & 0xFF) as u8);
                acc >>= 8;
                bits -= 8;
            }
        }
        if bits > 0 {
            out.push(acc as u8);
        }
        out
    }

    fn blocks(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in data.chunks(255) {
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
        out.push(0);
        out
    }

    /// A 4 × 2 GIF of red, green, blue and white: frame 1 all red, frame 2 a green square at
    /// (1, 0) whose second pixel is transparent, shown for 0.5 s then cleared, frame 3 blue at
    /// (3, 1).
    fn sample() -> Vec<u8> {
        let mut g = b"GIF89a".to_vec();
        g.extend_from_slice(&[4, 0, 2, 0, 0x81, 0, 0]);
        g.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        let frame = |g: &mut Vec<u8>, control: [u8; 4], place: [u16; 4], indices: &[u8]| {
            g.extend_from_slice(&[0x21, 0xF9, 4]);
            g.extend_from_slice(&control);
            g.push(0);
            g.push(0x2C);
            for v in place {
                g.extend_from_slice(&v.to_le_bytes());
            }
            g.push(0);
            g.push(2);
            g.extend(blocks(&compress(indices, 2)));
        };
        frame(&mut g, [0, 10, 0, 0], [0, 0, 4, 2], &[0; 8]);
        frame(&mut g, [(2 << 2) | 1, 50, 0, 3], [1, 0, 2, 1], &[1, 3]);
        frame(&mut g, [0, 0, 0, 0], [3, 1, 1, 1], &[2]);
        g.push(0x3B);
        g
    }

    fn px(f: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * 4 + x) * 4;
        [f[i], f[i + 1], f[i + 2], f[i + 3]]
    }

    #[test]
    fn frames_build_on_each_other_and_show_for_their_time() {
        let g = sample();
        assert_eq!(frame_count(&g), 3);
        let mut seen = Vec::new();
        assert_eq!(
            frames(&g, |f, ms| {
                seen.push((f.to_vec(), ms));
                true
            }),
            Some((4, 2, 3))
        );
        let (red, green, blue) = ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]);
        // a tenth of a second; the green over the red, its transparent pixel leaving the red
        assert_eq!(seen[0].1, 100);
        assert_eq!((px(&seen[1].0, 1, 0), px(&seen[1].0, 2, 0), px(&seen[1].0, 0, 0)), (green, red, red));
        assert_eq!(seen[1].1, 500);
        // then cleared to nothing where it was; a frame with no time plays at a tenth of a second
        assert_eq!((px(&seen[2].0, 1, 0), px(&seen[2].0, 3, 1), px(&seen[2].0, 0, 1)), ([0; 4], blue, red));
        assert_eq!(seen[2].1, 100);
        // stopping early
        let mut n = 0;
        assert_eq!(
            frames(&g, |_, _| {
                n += 1;
                false
            })
            .map(|f| f.2),
            Some(1)
        );
    }

    /// LZW as GIF encoders write it: strings grow in the table, and so do the codes.
    fn encode(indices: &[u8], min: u8) -> Vec<u8> {
        let clear = 1u32 << min;
        let mut table = std::collections::HashMap::new();
        let (mut next, mut size) = (clear + 2, u32::from(min) + 1);
        let mut codes = vec![(clear, size)];
        let mut w: Option<u32> = None;
        for &c in indices {
            let Some(p) = w else {
                w = Some(u32::from(c));
                continue;
            };
            if let Some(&code) = table.get(&(p, c)) {
                w = Some(code);
                continue;
            }
            codes.push((p, size));
            if next < 4096 {
                table.insert((p, c), next);
                if next == 1 << size && size < 12 {
                    size += 1;
                }
                next += 1;
            }
            w = Some(u32::from(c));
        }
        codes.extend(w.map(|p| (p, size)));
        codes.push((clear + 1, size));
        let (mut out, mut acc, mut bits) = (Vec::new(), 0u64, 0u32);
        for (c, n) in codes {
            acc |= u64::from(c) << bits;
            bits += n;
            while bits >= 8 {
                out.push((acc & 0xFF) as u8);
                acc >>= 8;
                bits -= 8;
            }
        }
        if bits > 0 {
            out.push(acc as u8);
        }
        out
    }

    #[test]
    fn codes_longer_than_the_first_size_decode() {
        // repeating runs: the table fills with longer strings and the codes grow past 3 bits
        let indices: Vec<u8> = (0..2000).map(|i| (i / 7 % 4) as u8).collect();
        let data = encode(&indices, 2);
        assert!(data.len() < indices.len() / 4, "{} bytes", data.len());
        assert_eq!(lzw(&data, 2, indices.len()).unwrap(), indices);
        // eight-bit colours
        let indices: Vec<u8> = (0..5000u32).map(|i| ((i * 37) % 251) as u8).collect();
        assert_eq!(lzw(&encode(&indices, 8), 8, indices.len()).unwrap(), indices);
    }

    #[test]
    fn other_files_and_damaged_ones_are_refused_or_cut_short() {
        assert_eq!(frame_count(b"not a picture"), 0);
        assert!(frames(b"GIF89a\0\0\0\0", |_, _| true).is_none());
        let g = sample();
        // cut in its last frame: the two before it come
        assert_eq!(frames(&g[..g.len() - 6], |_, _| true).map(|f| f.2), Some(2));
        // a frame far bigger than any canvas (a few bytes asking for gigabytes) ends it there
        let mut huge = b"GIF89a".to_vec();
        huge.extend_from_slice(&[1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
        for _ in 0..2 {
            huge.extend_from_slice(&[0x2C, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0, 2, 0]);
        }
        huge.push(0x3B);
        assert_eq!(frame_count(&huge), 2);
        assert_eq!(frames(&huge, |_, _| true).map(|f| f.2), Some(0));
        crate::damaged::survives("GIF", &g, |b| {
            let _ = frames(b, |_, _| true);
            let _ = frame_count(b);
        });
    }
}
