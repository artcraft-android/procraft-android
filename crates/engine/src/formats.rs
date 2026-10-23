//! Writing pictures for other apps: layered PSD, TIFF and PDF. Each takes straight-alpha RGBA8
//! pixels, rows top to bottom, and returns the file's bytes. A CMYK canvas's files keep its
//! press's inks: its pixels go through a table to them (`press`).

use crate::cms::Lut;
use crate::document::BlendMode;

/// A canvas's resolution unless it says otherwise, dots per inch.
pub const DPI: u32 = 132;

/// One layer as a PSD shows it.
pub struct PsdLayer<'a> {
    pub name: &'a str,
    pub kind: PsdKind<'a>,
    pub opacity: f32,
    pub visible: bool,
    pub blend: BlendMode,
    /// Clipped to the layer below.
    pub clipped: bool,
}

pub enum PsdKind<'a> {
    /// Pixels (the canvas's size), and a mask (its grey, the canvas's size) if it has one.
    Pixels {
        rgba: &'a [u8],
        mask: Option<&'a [u8]>,
    },
    /// The start of a group: the layers after it, up to its [`PsdKind::GroupEnd`], are in it.
    GroupStart {
        open: bool,
    },
    GroupEnd,
}

/// A layered PSD: `layers` top first (groups as a start, their layers, then an end), and the
/// flattened picture; `icc` is the colour profile it carries (none if empty). With a `press`
/// table it is a CMYK PSD (inks kept inverted, as PSDs keep them; the picture flattened on white).
pub fn psd(
    width: u32,
    height: u32,
    dpi: f32,
    icc: &[u8],
    layers: &[PsdLayer<'_>],
    composite: &[u8],
    press: Option<&Lut>,
) -> Vec<u8> {
    let fixed = (dpi.clamp(1.0, 9999.0) * 65536.0).round() as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"8BPS");
    put16(&mut out, 1);
    out.extend_from_slice(&[0; 6]);
    put16(&mut out, 4);
    put32(&mut out, height);
    put32(&mut out, width);
    put16(&mut out, 8);
    put16(&mut out, if press.is_some() { 4 } else { 3 });
    // no colour mode data
    put32(&mut out, 0);
    // image resources: the resolution
    let mut resources = Vec::new();
    resources.extend_from_slice(b"8BIM");
    put16(&mut resources, 0x03ED);
    put16(&mut resources, 0);
    put32(&mut resources, 16);
    for _ in 0..2 {
        put32(&mut resources, fixed);
        put16(&mut resources, 1);
        put16(&mut resources, 1);
    }
    // and the colour profile
    if !icc.is_empty() {
        resources.extend_from_slice(b"8BIM");
        put16(&mut resources, 0x040F);
        put16(&mut resources, 0);
        put32(&mut resources, icc.len() as u32);
        resources.extend_from_slice(icc);
        if icc.len() % 2 == 1 {
            resources.push(0);
        }
    }
    put32(&mut out, resources.len() as u32);
    out.extend_from_slice(&resources);

    // layers, listed bottom first, a group as its end marker, its layers, then its start
    let mut records = Vec::new();
    let mut channels = Vec::new();
    let count = layers.len();
    for layer in layers.iter().rev() {
        record(&mut records, &mut channels, width, height, layer, press);
    }
    let mut info = Vec::new();
    put16(&mut info, (count as i16).wrapping_neg() as u16);
    info.extend_from_slice(&records);
    info.extend_from_slice(&channels);
    if info.len() % 2 == 1 {
        info.push(0);
    }
    let mut section = Vec::new();
    put32(&mut section, info.len() as u32);
    section.extend_from_slice(&info);
    // no global mask
    put32(&mut section, 0);
    put32(&mut out, section.len() as u32);
    out.extend_from_slice(&section);

    // the flattened picture: four channels, packed (RGBA, or the inks on white)
    let planes: Vec<Vec<u8>> = match press {
        None => (0..4).map(|c| plane(composite, c)).collect(),
        Some(table) => {
            let inks = inks(composite, table, true);
            (0..4).map(|c| inks.iter().skip(c).step_by(4).map(|v| 255 - v).collect()).collect()
        }
    };
    put16(&mut out, 1);
    let rows: Vec<Vec<Vec<u8>>> =
        planes.iter().map(|p| p.chunks(width.max(1) as usize).map(packbits).collect()).collect();
    for plane in &rows {
        for row in plane {
            put16(&mut out, row.len() as u16);
        }
    }
    for plane in &rows {
        for row in plane {
            out.extend_from_slice(row);
        }
    }
    out
}

/// A layer's record (into `records`) and its channels' pixels (into `channels`): RGB, or the
/// inks `press` gives (inverted).
fn record(
    records: &mut Vec<u8>,
    channels: &mut Vec<u8>,
    width: u32,
    height: u32,
    layer: &PsdLayer<'_>,
    press: Option<&Lut>,
) {
    let ids: &[i16] = if press.is_some() { &[-1, 0, 1, 2, 3] } else { &[-1, 0, 1, 2] };
    let (rect, data): ([u32; 4], Vec<(i16, Vec<u8>)>) = match &layer.kind {
        PsdKind::Pixels { rgba, mask } => {
            let rect = bounds(rgba, width, height).unwrap_or([0, 0, 0, 0]);
            // five bytes a pixel for inks: the four inverted, then alpha
            let (pixels, stride) = match press {
                None => (rgba.to_vec(), 4),
                Some(table) => {
                    let inks = inks(rgba, table, false);
                    let mut five = Vec::with_capacity(rgba.len() / 4 * 5);
                    for (ink, px) in inks.as_chunks::<4>().0.iter().zip(rgba.as_chunks::<4>().0) {
                        five.extend(ink.map(|v| 255 - v));
                        five.push(px[3]);
                    }
                    (five, 5)
                }
            };
            let mut data: Vec<(i16, Vec<u8>)> = ids
                .iter()
                .map(|&id| (id, packed(&cropped_plane(&pixels, width, rect, stride, channel(id, stride)), rect)))
                .collect();
            if let Some(m) = mask {
                data.push((-2, packed(m, [0, 0, width, height])));
            }
            (rect, data)
        }
        // groups have no pixels: empty channels
        _ => ([0, 0, 0, 0], ids.iter().map(|&id| (id, vec![0, 0])).collect()),
    };
    let [left, top, right, bottom] = rect;
    for v in [top, left, bottom, right] {
        put32(records, v);
    }
    put16(records, data.len() as u16);
    for (id, bytes) in &data {
        put16(records, *id as u16);
        put32(records, bytes.len() as u32);
    }
    records.extend_from_slice(b"8BIM");
    let group_start = matches!(layer.kind, PsdKind::GroupStart { .. });
    records.extend_from_slice(if group_start && layer.blend == BlendMode::Normal {
        b"pass"
    } else {
        blend_key(layer.blend)
    });
    records.push((layer.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    records.push(u8::from(layer.clipped));
    // bit 1: hidden; bit 3 and 4: the record describes a group divider
    let mut flags = if layer.visible { 0 } else { 2 };
    if !matches!(layer.kind, PsdKind::Pixels { .. }) {
        flags |= 0x18;
    }
    records.push(flags);
    records.push(0);
    let mut extra = Vec::new();
    match &layer.kind {
        PsdKind::Pixels { mask: Some(_), .. } => {
            put32(&mut extra, 20);
            for v in [0, 0, height, width] {
                put32(&mut extra, v);
            }
            extra.extend_from_slice(&[255, 0, 0, 0]);
        }
        _ => put32(&mut extra, 0),
    }
    // no blending ranges
    put32(&mut extra, 0);
    let name = if matches!(layer.kind, PsdKind::GroupEnd) { "</Layer group>" } else { layer.name };
    let latin: Vec<u8> = name.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).take(255).collect();
    extra.push(latin.len() as u8);
    extra.extend_from_slice(&latin);
    while extra.len() % 4 != 0 {
        extra.push(0);
    }
    // the name in full
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut luni = Vec::new();
    put32(&mut luni, units.len() as u32);
    for u in &units {
        put16(&mut luni, *u);
    }
    if luni.len() % 2 == 1 {
        luni.push(0);
    }
    tagged(&mut extra, b"luni", &luni);
    // a group's start and end
    match layer.kind {
        PsdKind::GroupStart { open } => {
            let mut lsct = Vec::new();
            put32(&mut lsct, if open { 1 } else { 2 });
            tagged(&mut extra, b"lsct", &lsct);
        }
        PsdKind::GroupEnd => {
            let mut lsct = Vec::new();
            put32(&mut lsct, 3);
            tagged(&mut extra, b"lsct", &lsct);
        }
        PsdKind::Pixels { .. } => {}
    }
    put32(records, extra.len() as u32);
    records.extend_from_slice(&extra);
    for (_, bytes) in data {
        channels.extend_from_slice(&bytes);
    }
}

fn tagged(out: &mut Vec<u8>, key: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(key);
    put32(out, data.len() as u32);
    out.extend_from_slice(data);
}

/// Which byte of a pixel (`stride` bytes, alpha last) a PSD channel id is.
fn channel(id: i16, stride: usize) -> usize {
    match id {
        0 => 0,
        1 => 1,
        2 => 2,
        3 if stride > 4 => 3,
        _ => stride - 1,
    }
}

/// The inks (cyan, magenta, yellow, black; 0 for none) `table` gives each of the straight RGBA
/// `pixels`, a pixel's colour first laid on white if `on_white`.
fn inks(pixels: &[u8], table: &Lut, on_white: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len());
    for &[r, g, b, a] in pixels.as_chunks::<4>().0 {
        // nothing there: no ink
        if a == 0 && !on_white {
            out.extend([0; 4]);
            continue;
        }
        let a = if on_white { f32::from(a) / 255.0 } else { 1.0 };
        let c = |v: u8| (f32::from(v) / 255.0) * a + (1.0 - a);
        let ink = table.apply([c(r), c(g), c(b)]);
        out.extend((0..4).map(|k| (ink.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8));
    }
    out
}

/// One byte of every pixel.
fn plane(rgba: &[u8], c: usize) -> Vec<u8> {
    rgba.as_chunks::<4>().0.iter().map(|p| p.get(c).copied().unwrap_or(0)).collect()
}

/// Byte `c` of every pixel (`stride` bytes) in `rect` (left, top, right, bottom).
fn cropped_plane(pixels: &[u8], width: u32, [left, top, right, bottom]: [u32; 4], stride: usize, c: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(((right - left) * (bottom - top)) as usize);
    for y in top..bottom {
        for x in left..right {
            out.push(pixels.get((y * width + x) as usize * stride + c).copied().unwrap_or(0));
        }
    }
    out
}

/// A channel's pixels in `rect`, packed row by row with their sizes first.
fn packed(plane: &[u8], [left, top, right, bottom]: [u32; 4]) -> Vec<u8> {
    let w = (right - left) as usize;
    let mut out = Vec::new();
    put16(&mut out, 1);
    if w == 0 || bottom <= top {
        return out;
    }
    let rows: Vec<Vec<u8>> = plane.chunks(w).take((bottom - top) as usize).map(packbits).collect();
    for row in &rows {
        put16(&mut out, row.len() as u16);
    }
    for row in rows {
        out.extend_from_slice(&row);
    }
    out
}

/// The smallest box (left, top, right, bottom) around pixels with any alpha.
fn bounds(rgba: &[u8], width: u32, height: u32) -> Option<[u32; 4]> {
    let mut b: Option<[u32; 4]> = None;
    for y in 0..height {
        for x in 0..width {
            if rgba.get(((y * width + x) * 4 + 3) as usize).is_some_and(|a| *a > 0) {
                b = Some(match b {
                    Some([l, t, r, bt]) => [l.min(x), t.min(y), r.max(x + 1), bt.max(y + 1)],
                    None => [x, y, x + 1, y + 1],
                });
            }
        }
    }
    b
}

/// PackBits: runs of a repeated byte, and stretches of different ones, at most 128 each.
fn packbits(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(row.len() + row.len() / 128 + 1);
    let mut i = 0;
    while i < row.len() {
        let Some(&b) = row.get(i) else { break };
        let run = row.get(i..).map_or(1, |rest| rest.iter().take(128).take_while(|&&v| v == b).count());
        if run >= 2 {
            out.push((1i16 - run as i16) as u8);
            out.push(b);
            i += run;
        } else {
            // up to the next run of three or the stretch's limit
            let start = i;
            while i < row.len() && i - start < 128 {
                let three = row.get(i..i + 3).is_some_and(|w| w.iter().all(|v| Some(v) == w.first()));
                if three {
                    break;
                }
                i += 1;
            }
            if i == start {
                i += 1;
            }
            out.push((i - start - 1) as u8);
            out.extend_from_slice(row.get(start..i).unwrap_or_default());
        }
    }
    out
}

/// The four letters PSD names a blend mode by.
fn blend_key(mode: BlendMode) -> &'static [u8; 4] {
    match mode {
        BlendMode::Normal => b"norm",
        BlendMode::Multiply => b"mul ",
        // the format has no shade: darken is nearest
        BlendMode::Darken | BlendMode::Shade => b"dark",
        BlendMode::ColorBurn => b"idiv",
        BlendMode::LinearBurn => b"lbrn",
        BlendMode::DarkerColor => b"dkCl",
        BlendMode::Lighten => b"lite",
        BlendMode::Screen => b"scrn",
        BlendMode::ColorDodge => b"div ",
        BlendMode::Add => b"lddg",
        BlendMode::LighterColor => b"lgCl",
        BlendMode::Overlay => b"over",
        BlendMode::SoftLight => b"sLit",
        BlendMode::HardLight => b"hLit",
        BlendMode::VividLight => b"vLit",
        BlendMode::LinearLight => b"lLit",
        BlendMode::PinLight => b"pLit",
        BlendMode::HardMix => b"hMix",
        BlendMode::Difference => b"diff",
        BlendMode::Exclusion => b"smud",
        BlendMode::Subtract => b"fsub",
        BlendMode::Divide => b"fdiv",
        BlendMode::Hue => b"hue ",
        BlendMode::Saturation => b"sat ",
        BlendMode::Color => b"colr",
        BlendMode::Luminosity => b"lum ",
    }
}

/// A TIFF: RGBA with straight alpha, compressed with Deflate, at `dpi`, carrying the colour
/// profile `icc` (none if empty).
pub fn tiff(width: u32, height: u32, dpi: f32, icc: &[u8], rgba: &[u8]) -> Vec<u8> {
    write_tiff(width, height, dpi, icc, rgba, false)
}

/// A CMYK TIFF: the picture flattened on white and turned into the inks `press` gives, carrying
/// the press's profile `icc`.
pub fn tiff_cmyk(width: u32, height: u32, dpi: f32, icc: &[u8], rgba: &[u8], press: &Lut) -> Vec<u8> {
    write_tiff(width, height, dpi, icc, &inks(rgba, press, true), true)
}

/// A TIFF of four bytes a pixel: RGBA, or (`cmyk`) four inks with no alpha.
fn write_tiff(width: u32, height: u32, dpi: f32, icc: &[u8], pixels: &[u8], cmyk: bool) -> Vec<u8> {
    let tenths = (dpi.clamp(1.0, 9999.0) * 10.0).round() as u32;
    let data = miniz_oxide::deflate::compress_to_vec_zlib(pixels, 6);
    // header, then the pixels, then the bits per sample, the resolutions and the directory
    let mut out = Vec::new();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let pixels_at = out.len() as u32;
    out.extend_from_slice(&data);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let bits_at = out.len() as u32;
    for _ in 0..4 {
        out.extend_from_slice(&8u16.to_le_bytes());
    }
    let res_at = out.len() as u32;
    for _ in 0..2 {
        out.extend_from_slice(&tenths.to_le_bytes());
        out.extend_from_slice(&10u32.to_le_bytes());
    }
    let icc_at = out.len() as u32;
    out.extend_from_slice(icc);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let dir_at = out.len() as u32;
    if let Some(slot) = out.get_mut(4..8) {
        slot.copy_from_slice(&dir_at.to_le_bytes());
    }
    // (tag, type, count, value): type 3 short, 4 long, 5 rational, 7 bytes
    let mut entries: Vec<(u16, u16, u32, u32)> = vec![
        (256, 4, 1, width),
        (257, 4, 1, height),
        (258, 3, 4, bits_at),
        (259, 3, 1, 8),
        // RGB, or separated inks
        (262, 3, 1, if cmyk { 5 } else { 2 }),
        (273, 4, 1, pixels_at),
        (277, 3, 1, 4),
        (278, 4, 1, height),
        (279, 4, 1, data.len() as u32),
        (282, 5, 1, res_at),
        (283, 5, 1, res_at + 8),
        (296, 3, 1, 2),
    ];
    if cmyk {
        // the inks are cyan, magenta, yellow and black
        entries.push((332, 3, 1, 1));
    } else {
        // the fourth sample is straight alpha
        entries.push((338, 3, 1, 2));
    }
    if !icc.is_empty() {
        entries.push((34675, 7, icc.len() as u32, icc_at));
    }
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

/// A PDF with a page for each picture (straight-alpha RGBA, all `width` × `height`), each page
/// the picture's size at `dpi`; the pictures' colours are in the profile `icc` (if not empty).
/// At `quality` 100 the pictures are kept exactly; below, their colours are JPEG at that quality
/// (their alpha still exact). With a `press` table the pages are its inks, flattened on white,
/// as CMYK JPEG at `quality` (`icc` then the press's profile).
pub fn pdf(
    width: u32,
    height: u32,
    dpi: f32,
    icc: &[u8],
    pages: &[&[u8]],
    quality: u8,
    press: Option<&Lut>,
) -> Vec<u8> {
    let dpi = dpi.clamp(1.0, 9999.0);
    let (pw, ph) = (width as f32 * 72.0 / dpi, height as f32 * 72.0 / dpi);
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // 1: the catalog, 2: the page tree; then per page: page, contents, image, its alpha
    let first = 3;
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", first + i * 4)).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).into_bytes());
    // the colour profile goes after the pages
    let device = if press.is_some() { "/DeviceCMYK" } else { "/DeviceRGB" };
    let colours =
        if icc.is_empty() { device.to_owned() } else { format!("[/ICCBased {} 0 R]", first + pages.len() * 4) };
    for (i, rgba) in pages.iter().enumerate() {
        let n = first + i * 4;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Contents {} 0 R /Resources << /XObject << /Im0 {} 0 R >> >> >>",
                n + 1,
                n + 2
            )
            .into_bytes(),
        );
        let draw = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im0 Do Q");
        objects.push(stream(&format!("<< /Length {} >>", draw.len()), draw.as_bytes()));
        if let Some(table) = press {
            let jpg = crate::jpeg::encode_cmyk(&inks(rgba, table, true), width, height, quality.max(1));
            objects.push(stream(
                &format!(
                    "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /Interpolate true /ColorSpace {colours} /Decode [1 0 1 0 1 0 1 0] /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>",
                    jpg.len()
                ),
                &jpg,
            ));
            // no alpha: kept so the objects keep their numbers
            objects.push(b"null".to_vec());
            continue;
        }
        let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
        for &[r, g, b, _] in rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&[r, g, b]);
        }
        let alpha = plane(rgba, 3);
        let (rgb, filter) = if quality >= 100 {
            (miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6), "FlateDecode")
        } else {
            (crate::jpeg::encode(&rgb, width, height, quality), "DCTDecode")
        };
        let alpha = miniz_oxide::deflate::compress_to_vec_zlib(&alpha, 6);
        objects.push(stream(
            &format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace {colours} /BitsPerComponent 8 /Filter /{filter} /SMask {} 0 R /Length {} >>",
                n + 3,
                rgb.len()
            ),
            &rgb,
        ));
        objects.push(stream(
            &format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>",
                alpha.len()
            ),
            &alpha,
        ));
    }
    if !icc.is_empty() {
        let n = if press.is_some() { 4 } else { 3 };
        objects.push(stream(&format!("<< /N {n} /Alternate {device} /Length {} >>", icc.len()), icc));
    }
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes(),
    );
    out
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = dict.as_bytes().to_vec();
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// An animated GIF: `frames` (straight-alpha RGBA, all `width` × `height`) shown `delay_cs`
/// hundredths of a second each, looping forever. See [`GifWriter`].
pub fn gif(width: u32, height: u32, frames: &[&[u8]], delay_cs: u16) -> Vec<u8> {
    let mut g = GifWriter::new(width, height);
    for rgba in frames {
        g.frame(rgba, delay_cs, false);
    }
    g.finish()
}

/// An animated GIF written a frame at a time, looping forever. Each frame gets its own
/// 255-colour palette; a pixel under half opaque is transparent.
pub struct GifWriter {
    out: Vec<u8>,
    width: u16,
    height: u16,
}

impl GifWriter {
    /// A GIF `width` × `height` (each 1 to 65535).
    pub fn new(width: u32, height: u32) -> GifWriter {
        let (w, h) = (width.clamp(1, 65535) as u16, height.clamp(1, 65535) as u16);
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        // no global colour table
        out.extend_from_slice(&[0x00, 0, 0]);
        // loop forever
        out.extend_from_slice(&[0x21, 0xFF, 11]);
        out.extend_from_slice(b"NETSCAPE2.0");
        out.extend_from_slice(&[3, 1, 0, 0, 0]);
        GifWriter { out, width: w, height: h }
    }

    /// Adds a frame (straight-alpha RGBA, the GIF's size) shown `delay_cs` hundredths of a
    /// second; `dither` spreads each pixel's palette error to its neighbours.
    pub fn frame(&mut self, rgba: &[u8], delay_cs: u16, dither: bool) {
        let (w, h) = (self.width, self.height);
        let pixels = rgba.as_chunks::<4>().0;
        let (palette, used) = palette(pixels, usize::from(w) * usize::from(h));
        let indices =
            if dither { dithered(pixels, usize::from(w), &palette, used) } else { nearest_all(pixels, &palette, used) };
        let out = &mut self.out;
        // the frame clears to transparent after it shows; index 255 is transparent
        out.extend_from_slice(&[0x21, 0xF9, 4, 0b0000_1001]);
        out.extend_from_slice(&delay_cs.to_le_bytes());
        out.extend_from_slice(&[TRANSPARENT, 0]);
        out.push(0x2C);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        // a local colour table of 256 entries
        out.push(0b1000_0111);
        for c in palette.iter().take(256) {
            out.extend_from_slice(c);
        }
        out.push(8);
        for block in lzw(&indices).chunks(255) {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
    }

    /// The GIF's bytes.
    pub fn finish(mut self) -> Vec<u8> {
        self.out.push(0x3B);
        self.out
    }
}

/// The palette index transparent pixels take.
const TRANSPARENT: u8 = 255;

/// Up to 255 colours for the opaque pixels (median cut), padded to 256, and how many are used.
fn palette(pixels: &[[u8; 4]], n: usize) -> (Vec<[u8; 3]>, usize) {
    // a sample of the opaque colours is enough to find the palette
    let step = (n / 65_536).max(1);
    let sample: Vec<[u8; 3]> =
        pixels.iter().step_by(step).filter(|p| p[3] >= 128).map(|p| [p[0], p[1], p[2]]).collect();
    let mut boxes: Vec<Vec<[u8; 3]>> = if sample.is_empty() { Vec::new() } else { vec![sample] };
    while boxes.len() < 255 {
        // split the box with the widest channel spread
        let widest = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| (i, spread(b)))
            .max_by_key(|(_, (range, _))| *range);
        let Some((i, (range, channel))) = widest else { break };
        if range == 0 {
            break;
        }
        let mut b = boxes.swap_remove(i);
        b.sort_unstable_by_key(|c| c.get(channel).copied().unwrap_or(0));
        let upper = b.split_off(b.len() / 2);
        boxes.push(b);
        boxes.push(upper);
    }
    let mut palette: Vec<[u8; 3]> = boxes
        .iter()
        .map(|b| {
            let sum = b
                .iter()
                .fold([0u64; 3], |s, c| [s[0] + u64::from(c[0]), s[1] + u64::from(c[1]), s[2] + u64::from(c[2])]);
            let k = b.len().max(1) as u64;
            [(sum[0] / k) as u8, (sum[1] / k) as u8, (sum[2] / k) as u8]
        })
        .collect();
    palette.resize(256, [0, 0, 0]);
    (palette, boxes.len().max(1))
}

/// The nearest of a palette's first colours, remembered per 6 bits a channel (judged from the
/// middle of each step, so no colour is pulled one way).
struct Nearest<'a> {
    palette: &'a [[u8; 3]],
    used: usize,
    cache: Vec<u8>,
    cached: Vec<bool>,
}

impl Nearest<'_> {
    fn new(palette: &[[u8; 3]], used: usize) -> Nearest<'_> {
        Nearest { palette, used, cache: vec![u8::MAX; 1 << 18], cached: vec![false; 1 << 18] }
    }

    fn find(&mut self, c: [u8; 3]) -> u8 {
        let key = (usize::from(c[0] >> 2) << 12) | (usize::from(c[1] >> 2) << 6) | usize::from(c[2] >> 2);
        if self.cached.get(key) == Some(&true) {
            return self.cache.get(key).copied().unwrap_or(0);
        }
        let c = c.map(|v| (v & !3) | 2);
        let best = self
            .palette
            .iter()
            .take(self.used)
            .enumerate()
            .min_by_key(|(_, p)| {
                let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
                d(p[0], c[0]) + d(p[1], c[1]) + d(p[2], c[2])
            })
            .map_or(0, |(i, _)| i as u8);
        if let (Some(slot), Some(flag)) = (self.cache.get_mut(key), self.cached.get_mut(key)) {
            *slot = best;
            *flag = true;
        }
        best
    }
}

/// Each pixel's nearest palette colour.
fn nearest_all(pixels: &[[u8; 4]], palette: &[[u8; 3]], used: usize) -> Vec<u8> {
    let mut nearest = Nearest::new(palette, used);
    pixels.iter().map(|p| if p[3] < 128 { TRANSPARENT } else { nearest.find([p[0], p[1], p[2]]) }).collect()
}

/// Each pixel's palette colour with Floyd–Steinberg error diffusion, rows `width` wide.
fn dithered(pixels: &[[u8; 4]], width: usize, palette: &[[u8; 3]], used: usize) -> Vec<u8> {
    let mut nearest = Nearest::new(palette, used);
    let width = width.max(1);
    // the error carried into this row and the next, per channel, a pixel of margin each side
    let mut this = vec![[0i32; 3]; width + 2];
    let mut next = vec![[0i32; 3]; width + 2];
    let mut out = Vec::with_capacity(pixels.len());
    for row in pixels.chunks(width) {
        for (x, p) in row.iter().enumerate() {
            if p[3] < 128 {
                out.push(TRANSPARENT);
                continue;
            }
            let carried = this.get(x + 1).copied().unwrap_or([0; 3]);
            let want = [
                (i32::from(p[0]) + carried[0] / 16).clamp(0, 255),
                (i32::from(p[1]) + carried[1] / 16).clamp(0, 255),
                (i32::from(p[2]) + carried[2] / 16).clamp(0, 255),
            ];
            let i = nearest.find(want.map(|v| v as u8));
            out.push(i);
            let got = palette.get(usize::from(i)).copied().unwrap_or([0; 3]);
            let err = [want[0] - i32::from(got[0]), want[1] - i32::from(got[1]), want[2] - i32::from(got[2])];
            let add = |buf: &mut Vec<[i32; 3]>, at: usize, weight: i32| {
                if let Some(e) = buf.get_mut(at) {
                    for (e, d) in e.iter_mut().zip(err) {
                        *e += d * weight;
                    }
                }
            };
            add(&mut this, x + 2, 7);
            add(&mut next, x, 3);
            add(&mut next, x + 1, 5);
            add(&mut next, x + 2, 1);
        }
        std::mem::swap(&mut this, &mut next);
        next.iter_mut().for_each(|e| *e = [0; 3]);
    }
    out
}

/// The widest channel range in a box of colours, and which channel.
fn spread(b: &[[u8; 3]]) -> (u8, usize) {
    (0..3)
        .map(|k| {
            let (lo, hi) = b.iter().fold((u8::MAX, 0u8), |(lo, hi), c| {
                let v = c.get(k).copied().unwrap_or(0);
                (lo.min(v), hi.max(v))
            });
            (hi.saturating_sub(lo), k)
        })
        .max()
        .unwrap_or((0, 0))
}

/// GIF's variable-width LZW, 8-bit symbols, packed least significant bit first.
fn lzw(indices: &[u8]) -> Vec<u8> {
    const CLEAR: u16 = 256;
    const END: u16 = 257;
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut put = |code: u16, width: u32, out: &mut Vec<u8>| {
        acc |= u32::from(code) << bits;
        bits += width;
        while bits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            bits -= 8;
        }
    };
    let mut table: std::collections::HashMap<(u16, u8), u16> = std::collections::HashMap::new();
    let mut next = 258u16;
    let mut width = 9u32;
    put(CLEAR, width, &mut out);
    let mut current: Option<u16> = None;
    for &k in indices {
        let Some(prefix) = current else {
            current = Some(u16::from(k));
            continue;
        };
        if let Some(&code) = table.get(&(prefix, k)) {
            current = Some(code);
            continue;
        }
        put(prefix, width, &mut out);
        if next < 4096 {
            table.insert((prefix, k), next);
            next += 1;
            if next > (1 << width) && width < 12 {
                width += 1;
            }
        } else {
            // the table is full: start again
            put(CLEAR, width, &mut out);
            table.clear();
            next = 258;
            width = 9;
        }
        current = Some(u16::from(k));
    }
    if let Some(prefix) = current {
        put(prefix, width, &mut out);
    }
    put(END, width, &mut out);
    if bits > 0 {
        out.push(acc as u8);
    }
    out
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cmyk_canvas_writes_its_inks() {
        use crate::cms::{IccProfile, Lut, builtin};
        use crate::profile::Profile;
        let press = IccProfile::parse(&crate::cmyk::profile()).unwrap();
        let table = Lut::build(&builtin(Profile::DisplayP3), &press, None, 17);
        // a green square, half see-through around it
        let (w, h) = (8u32, 8u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if (2..6).contains(&(i % w)) && (2..6).contains(&(i / w)) { [0, 255, 0, 255] } else { [0, 0, 0, 0] }
            })
            .collect();
        let icc = &*press.bytes;
        let t = tiff_cmyk(w, h, 300.0, icc, &rgba, &table);
        let p = pdf(w, h, 300.0, icc, &[&rgba], 100, Some(&table));
        let layers = [PsdLayer {
            name: "Green",
            kind: PsdKind::Pixels { rgba: &rgba, mask: None },
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
            clipped: false,
        }];
        let f = psd(w, h, 300.0, icc, &layers, &rgba, Some(&table));
        // CMYK mode, four channels
        assert_eq!((u16::from_be_bytes([f[12], f[13]]), u16::from_be_bytes([f[24], f[25]])), (4, 4));
        assert!(p.windows(11).any(|w| w == b"/DeviceCMYK") && p.windows(9).any(|w| w == b"/N 4 /Alt"));
        if let Ok(dir) = std::env::var("FORMATS_CMYK_OUT") {
            std::fs::write(format!("{dir}/cmyk.tiff"), &t).unwrap();
            std::fs::write(format!("{dir}/cmyk.pdf"), &p).unwrap();
            std::fs::write(format!("{dir}/cmyk.psd"), &f).unwrap();
        }
    }

    fn unpack(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let n = data[i] as i8;
            i += 1;
            if n >= 0 {
                out.extend_from_slice(&data[i..i + n as usize + 1]);
                i += n as usize + 1;
            } else if n != -128 {
                out.extend(std::iter::repeat_n(data[i], (1 - n as i32) as usize));
                i += 1;
            }
        }
        out
    }

    #[test]
    fn packbits_round_trips() {
        for row in [
            vec![],
            vec![7],
            vec![1, 1, 1, 1, 2, 3, 4, 4, 5],
            (0..300).map(|i| (i % 7) as u8).collect::<Vec<_>>(),
            vec![9; 500],
            (0..400).map(|i| if i % 50 < 25 { 3 } else { (i * 13 % 251) as u8 }).collect(),
        ] {
            assert_eq!(unpack(&packbits(&row)), row, "{row:?}");
        }
    }

    fn be16(b: &[u8], at: usize) -> u16 {
        u16::from_be_bytes([b[at], b[at + 1]])
    }

    fn be32(b: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    #[test]
    fn a_psd_has_its_layers_and_picture() {
        let (w, h) = (4u32, 3u32);
        let mut red = vec![0u8; (w * h * 4) as usize];
        red[4..8].copy_from_slice(&[255, 0, 0, 255]);
        let blue = [0, 0, 255, 128].repeat((w * h) as usize);
        let mask = vec![255u8; (w * h) as usize];
        let layers = [
            PsdLayer {
                name: "Group",
                kind: PsdKind::GroupStart { open: true },
                opacity: 1.0,
                visible: true,
                blend: BlendMode::Normal,
                clipped: false,
            },
            PsdLayer {
                name: "Rouge é",
                kind: PsdKind::Pixels { rgba: &red, mask: Some(&mask) },
                opacity: 0.5,
                visible: false,
                blend: BlendMode::Multiply,
                clipped: true,
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
                name: "Blue",
                kind: PsdKind::Pixels { rgba: &blue, mask: None },
                opacity: 1.0,
                visible: true,
                blend: BlendMode::Normal,
                clipped: false,
            },
        ];
        let composite = vec![200u8; (w * h * 4) as usize];
        let f = psd(w, h, 132.0, &[], &layers, &composite, None);
        assert_eq!(&f[..4], b"8BPS");
        assert_eq!((be16(&f, 12), be32(&f, 14), be32(&f, 18), be16(&f, 22), be16(&f, 24)), (4, h, w, 8, 3));
        // past the colour data and the resources to the layers
        let mut at = 26;
        at += 4 + be32(&f, at) as usize;
        at += 4 + be32(&f, at) as usize;
        let section = be32(&f, at) as usize;
        let info = at + 4;
        assert_eq!(be16(&f, info + 4) as i16, -4);
        // the first record is the bottom layer: "Blue", the whole canvas
        let r = info + 6;
        assert_eq!([be32(&f, r), be32(&f, r + 4), be32(&f, r + 8), be32(&f, r + 12)], [0, 0, h, w]);
        assert_eq!(be16(&f, r + 16), 4);
        let blend = r + 18 + 4 * 6;
        assert_eq!(&f[blend..blend + 8], b"8BIMnorm");
        assert!(f.windows(14).any(|x| x == b"</Layer group>"));
        assert!(f.windows(4).any(|x| x == b"lsct") && f.windows(4).any(|x| x == b"luni"));
        // the red layer: its one pixel, multiply, half opacity, clipped, hidden, with a mask
        let red_rect = f.windows(16).position(|x| x == [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 2]).unwrap();
        assert_eq!(be16(&f, red_rect + 16), 5);
        let blend = red_rect + 18 + 5 * 6;
        assert_eq!(&f[blend..blend + 12], b"8BIMmul \x80\x01\x02\x00");
        // the picture follows the layers, packed
        let image = at + 4 + section;
        assert_eq!(be16(&f, image), 1);
        let first_row = be16(&f, image + 2) as usize;
        let data = image + 2 + 4 * h as usize * 2;
        assert_eq!(unpack(&f[data..data + first_row]), vec![200; w as usize]);
    }

    #[test]
    fn a_tiff_and_a_pdf_hold_the_pixels() {
        let (w, h) = (3u32, 2u32);
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i * 11 % 256) as u8).collect();
        let t = tiff(w, h, 132.0, &[], &rgba);
        assert_eq!(&t[..4], b"II*\0");
        let dir = u32::from_le_bytes(t[4..8].try_into().unwrap()) as usize;
        let entries = u16::from_le_bytes([t[dir], t[dir + 1]]) as usize;
        let entry = |tag: u16| {
            (0..entries)
                .map(|i| dir + 2 + i * 12)
                .find(|&e| u16::from_le_bytes([t[e], t[e + 1]]) == tag)
                .map(|e| u32::from_le_bytes(t[e + 8..e + 12].try_into().unwrap()))
        };
        assert_eq!((entry(256), entry(257), entry(259), entry(338)), (Some(w), Some(h), Some(8), Some(2)));
        let (at, len) = (entry(273).unwrap() as usize, entry(279).unwrap() as usize);
        assert_eq!(miniz_oxide::inflate::decompress_to_vec_zlib(&t[at..at + len]).unwrap(), rgba);
        let p = pdf(w, h, 132.0, &[], &[&rgba, &rgba], 100, None);
        let text = String::from_utf8_lossy(&p);
        assert!(text.starts_with("%PDF-1.4") && text.trim_end().ends_with("%%EOF"));
        assert!(text.contains("/Count 2") && text.contains("/SMask"));
        // the cross-reference table points at each object
        let xref: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(p[xref..].starts_with(b"xref"));
        let table = std::str::from_utf8(&p[xref..]).unwrap();
        let first: usize = table.lines().nth(3).unwrap()[..10].parse().unwrap();
        assert!(p[first..].starts_with(b"1 0 obj"));
    }

    /// A small GIF LZW decoder, to check the encoder against.
    fn unlzw(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let (mut acc, mut bits, mut pos) = (0u32, 0u32, 0usize);
        let mut width = 9u32;
        let mut table: Vec<Vec<u8>> = (0..258).map(|i| if i < 256 { vec![i as u8] } else { vec![] }).collect();
        let mut prev: Option<Vec<u8>> = None;
        loop {
            while bits < width {
                if pos >= data.len() {
                    return out;
                }
                acc |= u32::from(data[pos]) << bits;
                pos += 1;
                bits += 8;
            }
            let code = (acc & ((1 << width) - 1)) as usize;
            acc >>= width;
            bits -= width;
            if code == 256 {
                table.truncate(258);
                width = 9;
                prev = None;
                continue;
            }
            if code == 257 {
                return out;
            }
            let entry = if code < table.len() {
                table[code].clone()
            } else {
                let p = prev.clone().unwrap();
                let mut e = p.clone();
                e.push(p[0]);
                e
            };
            out.extend_from_slice(&entry);
            if let Some(p) = prev {
                let mut e = p.clone();
                e.push(entry[0]);
                table.push(e);
                if table.len() == (1 << width) && width < 12 {
                    width += 1;
                }
            }
            prev = Some(entry);
        }
    }

    #[test]
    fn lzw_round_trips() {
        for data in [
            vec![1u8, 1, 1, 1, 2, 2, 3],
            (0..5000).map(|i| (i % 7) as u8).collect::<Vec<_>>(),
            (0..20000).map(|i| ((i * 31 + i / 17) % 251) as u8).collect::<Vec<_>>(),
            vec![9; 9000],
        ] {
            assert_eq!(unlzw(&lzw(&data)), data);
        }
    }

    #[test]
    fn a_gif_has_its_frames() {
        let (w, h) = (6u32, 4u32);
        let red = [255u8, 0, 0, 255].repeat(24);
        let mut half = [0u8, 0, 255, 255].repeat(24);
        half[..48].iter_mut().skip(3).step_by(4).for_each(|a| *a = 0);
        let g = gif(w, h, &[&red, &half], 7);
        assert_eq!(&g[..6], b"GIF89a");
        assert_eq!(*g.last().unwrap(), 0x3B);
        assert!(g.windows(11).any(|x| x == b"NETSCAPE2.0"));
        // two frames, each with its delay
        let gces: Vec<usize> =
            g.windows(3).enumerate().filter(|(_, x)| x == &[0x21, 0xF9, 4]).map(|(i, _)| i).collect();
        assert_eq!(gces.len(), 2);
        assert_eq!(u16::from_le_bytes([g[gces[0] + 4], g[gces[0] + 5]]), 7);
        // the first frame's pixels decode to one colour, red in its palette
        let desc = gces[0] + 8;
        assert_eq!(g[desc], 0x2C);
        let table = desc + 10;
        let data = table + 768 + 1;
        let mut lz = Vec::new();
        let mut at = data;
        while g[at] != 0 {
            lz.extend_from_slice(&g[at + 1..at + 1 + g[at] as usize]);
            at += 1 + g[at] as usize;
        }
        let px = unlzw(&lz);
        assert_eq!(px.len(), 24);
        let i = px[0] as usize;
        assert!(px.iter().all(|&p| p as usize == i));
        assert_eq!(&g[table + i * 3..table + i * 3 + 3], &[255, 0, 0]);
    }

    #[test]
    fn dithering_mixes_palette_colours_for_a_ramp() {
        // a grey ramp wider than any palette: dithered, neighbouring pixels differ more often
        let (w, h) = (512u32, 4u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let v = ((i % w) * 255 / (w - 1)) as u8;
                [v, v, v, 255]
            })
            .collect();
        let px = rgba.as_chunks::<4>().0;
        let (pal, used) = palette(px, (w * h) as usize);
        let plain = nearest_all(px, &pal, used);
        let mixed = dithered(px, w as usize, &pal, used);
        assert_eq!(plain.len(), mixed.len());
        // the average colour along a row stays close to the ramp's
        let mean =
            |idx: &[u8]| idx[..w as usize].iter().map(|&i| f64::from(pal[i as usize][0])).sum::<f64>() / f64::from(w);
        assert!((mean(&mixed) - 127.5).abs() < 2.0);
        // a GIF made a frame at a time ends like one made at once
        let mut g = GifWriter::new(w, h);
        g.frame(&rgba, 7, true);
        let bytes = g.finish();
        assert_eq!(*bytes.last().unwrap(), 0x3B);
    }

    #[test]
    fn files_carry_the_colour_profile() {
        let icc = crate::icc::icc(crate::profile::Profile::DisplayP3);
        let rgba = vec![200u8; 4 * 4 * 4];
        let has = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
        // TIFF: the profile and an InterColorProfile entry (tag 34675, type 7)
        let t = tiff(4, 4, 132.0, &icc, &rgba);
        assert!(has(&t, &icc));
        assert!(has(&t, &[0x73, 0x87, 7, 0]));
        // PSD: an image resource 1039 holding it
        let mut res = b"8BIM".to_vec();
        res.extend_from_slice(&[0x04, 0x0F, 0, 0]);
        res.extend_from_slice(&(icc.len() as u32).to_be_bytes());
        res.extend_from_slice(&icc);
        let f = psd(4, 4, 132.0, &icc, &[], &rgba, None);
        assert!(has(&f, &res));
        // PDF: the images in an ICC-based space
        let p = pdf(4, 4, 132.0, &icc, &[&rgba], 100, None);
        // below 100 its colours are JPEG
        let jpeg = pdf(4, 4, 132.0, &icc, &[&rgba], 60, None);
        assert!(jpeg.windows(9).any(|w| w == b"DCTDecode"));
        assert!(has(&p, b"/ColorSpace [/ICCBased") && has(&p, &icc));
        // without one, none
        assert!(!has(&tiff(4, 4, 132.0, &[], &rgba), &[0x73, 0x87]));
    }
}
