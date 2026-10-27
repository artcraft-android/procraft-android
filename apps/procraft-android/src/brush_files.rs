//! Brush and set files as other painting apps share them (`.brush`, `.brushset`): read into the
//! library, and written from it. A brush's shape and grain images go with it; an image a file only
//! names (one that came with the app that made it) is stood in for by the nearest of ours.

use procraft_engine::brush::{Author, Brush, MaterialSource, Source, StockImage, StockImages};
use procraft_engine::brushfile::{self, BrushFile, Pictures, Texture};
use procraft_engine::curve::Curve;
use procraft_engine::textures::{self, TEXTURE_SIZE, grain, shape};

use crate::CanvasSession;
use crate::brush_library::LibrarySet;

/// The set brushes from single brush files go into.
const IMPORTED: &str = "Imported";

/// The size of the preview a brush file carries.
const THUMBNAIL: (u32, u32) = (327, 100);

/// The most a brush file's picture may decode to (8192 × 8192 in colour), bytes.
const MOST_PNG_BYTES: usize = 256 << 20;

/// A Photoshop preset's most size (as brush files keep it) for each pixel of its tip: 3.28% of
/// the scale a pixel, as 35 presets of a real file come in elsewhere (8 px reading 26%, 442 px
/// 1448%).
const ABR_SIZE_PER_PIXEL: f32 = 0.03277;

/// A brush file's pictures, made: the shape, the grain if any and the preview (PNGs).
struct Made {
    shape: Vec<u8>,
    grain: Option<Vec<u8>>,
    metallic: Option<Vec<u8>>,
    roughness: Option<Vec<u8>>,
    thumbnail: Vec<u8>,
    author: Option<std::sync::Arc<[u8]>>,
    signature: Option<std::sync::Arc<[u8]>>,
}

impl Made {
    fn pictures(&self) -> Pictures<'_> {
        Pictures {
            shape_png: &self.shape,
            grain_png: self.grain.as_deref(),
            metallic_png: self.metallic.as_deref(),
            roughness_png: self.roughness.as_deref(),
            thumbnail_png: &self.thumbnail,
            author_png: self.author.as_deref(),
            signature_png: self.signature.as_deref(),
        }
    }
}

/// Who made `brush`, as its file is to say ("" for ProCraft's own).
fn author_name(brush: &Brush) -> &str {
    brush.author.as_ref().map_or("", |a| a.name.as_str())
}

impl CanvasSession {
    fn brush_pictures(&self, brush: &Brush, set: u32, index: u32) -> Made {
        // the preview it came with, else one drawn
        let thumbnail = brush.thumbnail.as_deref().map(<[u8]>::to_vec).unwrap_or_else(|| {
            self.brush_preview(set, index, THUMBNAIL.0, THUMBNAIL.1)
                .map(|t| crate::project::rgba_png(t.width, t.height, &t.rgba))
                .unwrap_or_else(|| crate::project::rgba_png(1, 1, &[0, 0, 0, 0]))
        });
        let author = brush.author.as_ref();
        Made {
            shape: gray_png(&shape_pixels(brush)),
            grain: grain_pixels(brush).map(|p| gray_png(&p)),
            metallic: material_pixels(&brush.metallic).map(|p| gray_png(&p)),
            roughness: material_pixels(&brush.roughness).map(|p| gray_png(&p)),
            thumbnail,
            author: author.and_then(|a| a.picture.clone()),
            signature: author.and_then(|a| a.signature.clone()),
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Adds the brushes in `bytes`: a brush file's (to the top of the "Imported" set, made at the
    /// top of the library if there isn't one) or a set file's (a set of their own at the top).
    /// Returns the set they went into, or none if the file is neither.
    pub fn import_brushes(&self, bytes: Vec<u8>) -> Option<u32> {
        if let Some((name, files)) = brushfile::read_set(&bytes) {
            let brushes: Vec<Brush> = files.iter().map(brush_from).collect();
            let mut st = self.shared.lock();
            let name = if name.trim().is_empty() { IMPORTED.to_owned() } else { name.trim().to_owned() };
            let origins = vec![None; brushes.len()];
            st.library.insert(0, LibrarySet { name, brushes, origins, icon: crate::brush_library::IMPORTED_ICON });
            st.move_brush_refs(|(rs, ri)| Some((rs + 1, ri)));
            return Some(0);
        }
        let brush = brush_from(&brushfile::read(&bytes)?);
        let mut st = self.shared.lock();
        let at = match st.library.iter().position(|s| s.name == IMPORTED) {
            Some(at) => at,
            None => {
                st.library.insert(
                    0,
                    LibrarySet {
                        name: IMPORTED.to_owned(),
                        brushes: Vec::new(),
                        origins: Vec::new(),
                        icon: crate::brush_library::IMPORTED_ICON,
                    },
                );
                st.move_brush_refs(|(rs, ri)| Some((rs + 1, ri)));
                0
            }
        };
        let entry = st.library.get_mut(at)?;
        entry.brushes.insert(0, brush);
        entry.origins.insert(0, None);
        st.move_brush_refs(|(rs, ri)| Some(if rs == at { (rs, ri + 1) } else { (rs, ri) }));
        Some(at as u32)
    }

    /// Adds the brushes in another app's file (`bytes`): a Photoshop brush file's sampled tips, a
    /// GIMP brush or pipe, a Krita preset or bundle, or a Clip Studio sub tool. They go in a set of their own named `name`
    /// (a bundle's own name first; else "Imported") at the top of the library, each with the
    /// file's shape and spacing. Returns the set, or none if the file holds none.
    pub fn import_other_brushes(&self, bytes: Vec<u8>, name: String) -> Option<u32> {
        let (name, brushes) = other_brushes(&bytes, name)?;
        if brushes.is_empty() {
            return None;
        }
        let mut st = self.shared.lock();
        let name = if name.trim().is_empty() { IMPORTED.to_owned() } else { name.trim().to_owned() };
        let origins = vec![None; brushes.len()];
        st.library.insert(0, LibrarySet { name, brushes, origins, icon: crate::brush_library::IMPORTED_ICON });
        st.move_brush_refs(|(rs, ri)| Some((rs + 1, ri)));
        Some(0)
    }

    /// Brush `index` of `set` as a brush file.
    pub fn brush_as_file(&self, set: u32, index: u32) -> Option<Vec<u8>> {
        let brush = self.shared.lock().library.get(set as usize)?.brushes.get(index as usize)?.clone();
        let made = self.brush_pictures(&brush, set, index);
        Some(brushfile::write(&brush, author_name(&brush), &made.pictures()))
    }

    /// Set `set` as a set file.
    pub fn brush_set_as_file(&self, set: u32) -> Option<Vec<u8>> {
        let (name, brushes) = {
            let st = self.shared.lock();
            let entry = st.library.get(set as usize)?;
            (entry.name.clone(), entry.brushes.clone())
        };
        let made: Vec<Made> = brushes.iter().enumerate().map(|(i, b)| self.brush_pictures(b, set, i as u32)).collect();
        let list: Vec<(&Brush, &str, Pictures<'_>)> =
            brushes.iter().zip(&made).map(|(b, m)| (b, author_name(b), m.pictures())).collect();
        let seed =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Some(brushfile::write_set(&name, &list, |i| folder_id(seed, i)))
    }
}

/// A folder name like the files' own (a UUID's form), from `seed` and the brush's place.
fn folder_id(seed: u64, i: usize) -> String {
    let mix = |k: u64| {
        let mut h = seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ k.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^ (h >> 29)
    };
    let (a, b) = (mix(1), mix(2));
    format!(
        "{:08X}-{:04X}-4{:03X}-{:04X}-{:012X}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFF,
        0x8000 | ((b >> 48) & 0x3FFF),
        b & 0xFFFF_FFFF_FFFF
    )
}

/// A brush from a brush file: the studio's controls on a fresh brush, its images found or stood in
/// for.
/// A Photoshop tip fitted into a shape's square, centred on black: each pixel the average of the
/// tip's pixels it covers (or the nearest one, drawn up).
fn fitted(t: &procraft_engine::abr::AbrTip) -> Vec<u8> {
    let n = TEXTURE_SIZE;
    let (w, h) = (t.width as usize, t.height as usize);
    let mut out = vec![0u8; n * n];
    if w == 0 || h == 0 {
        return out;
    }
    let scale = n as f32 / w.max(h) as f32;
    let (tw, th) =
        (((w as f32 * scale).round() as usize).clamp(1, n), ((h as f32 * scale).round() as usize).clamp(1, n));
    let (ox, oy) = ((n - tw) / 2, (n - th) / 2);
    let span = |i: usize, most: usize| {
        let from = ((i as f32 / scale) as usize).min(most - 1);
        (from, (((i + 1) as f32 / scale).ceil() as usize).clamp(from + 1, most))
    };
    for y in 0..th {
        let (y0, y1) = span(y, h);
        for x in 0..tw {
            let (x0, x1) = span(x, w);
            let mut sum = 0u32;
            for sy in y0..y1 {
                let row = t.pixels.get(sy * w + x0..sy * w + x1).unwrap_or_default();
                sum += row.iter().map(|&v| u32::from(v)).sum::<u32>();
            }
            let count = ((y1 - y0) * (x1 - x0)).max(1) as u32;
            if let Some(v) = out.get_mut((oy + y) * n + ox + x) {
                *v = (sum / count) as u8;
            }
        }
    }
    out
}

/// The brushes in another app's file, with the set's name (a bundle's own, else `name`).
fn other_brushes(bytes: &[u8], name: String) -> Option<(String, Vec<Brush>)> {
    use procraft_engine::krita;
    if let Some(brushes) = procraft_engine::abr::read_brushes(bytes) {
        return Some((name, brushes.iter().map(abr_brush).collect()));
    }
    if let Some(tips) = procraft_engine::gbr::read(bytes) {
        return Some((name, tips.iter().map(tip_brush).collect()));
    }
    if let Some(g) = procraft_engine::gbr::read_generated(bytes) {
        return Some((name, vec![generated_brush(&g)]));
    }
    if let Some((title, presets)) = krita::read_bundle(bytes) {
        let name = if title.trim().is_empty() { name } else { title };
        return Some((name, presets.iter().map(krita_brush).collect()));
    }
    if let Some(brushes) = procraft_engine::csp::read(bytes) {
        return Some((name, brushes.iter().map(csp_brush).collect()));
    }
    // a preset alone has no images of its own: one it names comes round
    let preset = krita::read_preset(bytes, &|_| None)?;
    Some((name, vec![krita_brush(&preset)]))
}

/// A brush of a Clip Studio sub tool: its tip (an image, else round with its edge and roundness),
/// spacing and angle.
fn csp_brush(c: &procraft_engine::csp::CspBrush) -> Brush {
    // the first of its pictures that paints anything: white, or else dark
    let paints = |t: &procraft_engine::abr::AbrTip| t.pixels.iter().any(|&v| v > 0);
    let tip = c
        .tip_pngs
        .iter()
        .find_map(|png| png_tip(png, true).filter(paints).or_else(|| png_tip(png, false).filter(paints)));
    let mut b = match tip {
        Some(t) => tip_brush(&procraft_engine::abr::AbrTip { name: c.name.clone(), ..t }),
        None => Brush { name: c.name.clone(), ..Brush::round_pen() },
    };
    if let Some(h) = c.hardness {
        b.hardness = h.clamp(0.0, 1.0);
    }
    if let Some(r) = c.roundness {
        b.shape.roundness = r.clamp(0.01, 1.0);
    }
    if let Some(spacing) = c.spacing {
        b.spacing = Brush::spacing_within_reach(spacing);
    }
    b.shape.rotation = c.angle;
    if let Some(t) = &c.texture {
        paper(&mut b, &t.pngs, t.reverse, t.density.unwrap_or(1.0), t.scale.unwrap_or(1.0));
    }
    b
}

/// Gives `b` another app's texture as its grain, paper the stroke paints through: the first of
/// `pngs` that lets paint through anywhere (one may be blank), dark letting it through if
/// `inverted`, showing `depth` strongly, tiled at the picture's own size times `scale`. Nothing if
/// none of them will do.
fn paper(b: &mut Brush, pngs: &[Vec<u8>], inverted: bool, depth: f32, scale: f32) {
    let found = pngs.iter().find_map(|png| {
        let source = gray_texture(png, inverted).filter(|s| s.pixels.iter().any(|&v| v > 0))?;
        Some((source, decode_png(png)?.0))
    });
    if let Some((source, width)) = found {
        b.grain.image = None;
        b.grain.source = Some(source);
        b.grain.depth = depth.clamp(0.0, 1.0);
        b.set_setting("grain_scale", width as f32 * scale / procraft_engine::studio::MAX_GRAIN_SCALE);
    }
}

/// A brush of a Photoshop preset: its sampled tip, or round with its hardness; its roundness,
/// spacing and angle; its texture as its grain.
fn abr_brush(a: &procraft_engine::abr::AbrBrush) -> Brush {
    let mut b = match &a.tip {
        Some(t) => tip_brush(t),
        None => {
            let mut b = Brush { name: a.name.clone(), ..Brush::round_pen() };
            if let Some(h) = a.hardness {
                b.hardness = h;
            }
            b
        }
    };
    if let Some(r) = a.roundness {
        b.shape.roundness = r.clamp(0.01, 1.0);
    }
    if let Some(spacing) = a.spacing {
        b.spacing = Brush::spacing_within_reach(spacing);
    }
    // Photoshop turns the other way (its 129 degrees are -129 here)
    b.shape.rotation = -a.angle;
    if let Some(t) = &a.texture {
        let png = sized_gray_png(t.width, t.height, &t.pixels);
        paper(&mut b, &[png], t.invert, t.depth.unwrap_or(1.0), t.scale.unwrap_or(1.0));
    }
    // as a preset comes in elsewhere: no taper, no StreamLine, glazed uniformly at full flow, down
    // to the least size; the pen's pressure on the size (half of what the shape dynamics let go
    // of), the opacity and the flow only as the preset's dynamics have them
    for (key, value) in [
        ("pen_taper_size", 0.0),
        ("pen_taper_opacity", 0.0),
        ("touch_taper_size", 0.0),
        ("touch_taper_opacity", 0.0),
        ("streamline", 0.0),
        ("rendering_mode", 1.0),
        ("flow", 1.0),
    ] {
        b.set_setting(key, value);
    }
    // its most size from its tip's pixels (a sampled tip's no taller than its picture), the least 1%
    let pixels = match (&a.tip, a.diameter) {
        (None, d) => d,
        (Some(t), d) => Some(d.map_or(t.height as f32, |d| d.min(t.height as f32))),
    };
    if let Some(px) = pixels {
        procraft_engine::brushfile::set_sizes(&mut b, px * ABR_SIZE_PER_PIXEL, Some(0.01));
    }
    let d = a.dynamics;
    b.set_setting("pressure_size", if d.tip && d.size_by_pressure { (1.0 - d.min_diameter) / 2.0 } else { 0.0 });
    // the stylus page's opacity is kept as "pressure_flow", its flow as "pressure_opacity"
    b.set_setting("pressure_flow", if d.paint && d.opacity_by_pressure { 1.0 } else { 0.0 });
    b.set_setting("pressure_opacity", if d.paint && d.flow_by_pressure { 1.0 } else { 0.0 });
    b
}

/// A brush of a GIMP generated brush: round with its hardness and how round it is, or its square
/// or diamond drawn; its spacing and angle.
fn generated_brush(g: &procraft_engine::gbr::Generated) -> Brush {
    let mut b = match procraft_engine::gbr::generated_tip(g) {
        Some(tip) => tip_brush(&tip),
        None => {
            let mut b = Brush { name: g.name.clone(), ..Brush::round_pen() };
            b.hardness = g.hardness;
            b.shape.roundness = (1.0 / g.aspect).clamp(0.01, 1.0);
            b
        }
    };
    b.spacing = Brush::spacing_within_reach(g.spacing);
    b.shape.rotation = g.angle;
    b
}

/// A brush of a sampled tip: its shape, with its spacing.
fn tip_brush(t: &procraft_engine::abr::AbrTip) -> Brush {
    let mut b = Brush { name: t.name.clone(), ..Brush::round_pen() };
    b.shape.image = None;
    b.shape.source = Source::new(fitted(t));
    if let Some(spacing) = t.spacing {
        b.spacing = Brush::spacing_within_reach(spacing);
    }
    b
}

/// A brush of a Krita preset: its tip (round, or an image), spacing, angle, and what pressure
/// changes.
fn krita_brush(k: &procraft_engine::krita::KritaBrush) -> Brush {
    use procraft_engine::krita::KritaTip;
    let image = match &k.tip {
        KritaTip::Round { .. } => None,
        KritaTip::Image(t) => Some(fitted(t)),
        KritaTip::Png(png) => png_tip(png, false).map(|t| fitted(&t)),
    };
    let mut b = match image {
        Some(pixels) => {
            let mut b = Brush { name: k.name.clone(), ..Brush::round_pen() };
            b.shape.image = None;
            b.shape.source = Source::new(pixels);
            b
        }
        None => Brush { name: k.name.clone(), ..Brush::round_pen() },
    };
    if let KritaTip::Round { hardness, roundness } = k.tip {
        b.hardness = hardness.clamp(0.0, 1.0);
        b.shape.roundness = roundness.clamp(0.01, 1.0);
    }
    if let Some(spacing) = k.spacing {
        b.spacing = Brush::spacing_within_reach(spacing);
    }
    b.shape.rotation = k.angle;
    if !k.pressure_size {
        b.pressure_size = 0.0;
    }
    if k.pressure_opacity {
        b.pressure_opacity = 1.0;
    }
    if let Some(t) = &k.texture {
        paper(&mut b, std::slice::from_ref(&t.png), t.invert, t.strength, t.scale);
    }
    b
}

/// A PNG tip as a grey one: its opaque pixels paint, the light ones if `white`, else the dark.
fn png_tip(png: &[u8], white: bool) -> Option<procraft_engine::abr::AbrTip> {
    let (w, h, channels, buf) = decode_png(png)?;
    let pixels: Vec<u8> = buf
        .chunks_exact(channels.max(1))
        .take(w * h)
        .map(|px| {
            let get = |k: usize| u32::from(px.get(k).copied().unwrap_or(0));
            let (grey, alpha) = match channels {
                1 => (get(0), 255),
                2 => (get(0), get(1)),
                3 => ((get(0) * 299 + get(1) * 587 + get(2) * 114) / 1000, 255),
                _ => ((get(0) * 299 + get(1) * 587 + get(2) * 114) / 1000, get(3)),
            };
            let ink = if white { grey.min(255) } else { 255 - grey.min(255) };
            (ink * alpha / 255) as u8
        })
        .collect();
    (pixels.len() == w * h).then(|| procraft_engine::abr::AbrTip {
        name: String::new(),
        spacing: None,
        width: w as u32,
        height: h as u32,
        pixels,
    })
}

fn brush_from(file: &BrushFile) -> Brush {
    // only the curves the file has (a control without one doesn't follow the pressure)
    let mut b = Brush { name: file.name.clone(), pressure_curves: Default::default(), ..Brush::round_pen() };
    let (image, source, hardness) = shape_for(&file.shape);
    b.shape.image = image;
    b.shape.source = source;
    if let Some(h) = hardness {
        b.hardness = h;
    }
    for (key, value) in &file.settings {
        b.set_setting(key, *value);
    }
    brushfile::set_sizes(&mut b, file.max_size, file.min_size);
    b.shape.rotation = file.shape_angle;
    for (points, curve) in [(&file.size_curve, &mut b.size_curve), (&file.opacity_curve, &mut b.opacity_curve)] {
        if let Some(points) = points {
            *curve = Curve::new(points);
        }
    }
    if let Some(points) = &file.pressure_curve {
        b.pressure_curve = Curve::new(points);
    }
    for (kind, points) in &file.pressure_curves {
        b.pressure_curves.set(*kind, Some(Curve::new(points)));
    }
    b.notches = file.notches.clone();
    let (image, source) = grain_for(&file.grain, file.thumbnail.as_deref());
    b.grain.image = image;
    b.grain.source = source;
    for (texture, material) in [(&file.metallic, &mut b.metallic), (&file.roughness, &mut b.roughness)] {
        let (image, source) = material_for(texture);
        *material = MaterialSource { image, source };
    }
    // the stock images it names, to name again when it is written out
    let stock = |t: &Texture| match t {
        Texture::Stock { name, inverted } => Some(StockImage { name: name.clone(), inverted: *inverted }),
        _ => None,
    };
    b.stock = StockImages {
        shape: stock(&file.shape),
        grain: stock(&file.grain),
        metallic: stock(&file.metallic),
        roughness: stock(&file.roughness),
    };
    // its own preview, if it can be shown
    b.thumbnail = file.thumbnail.as_deref().filter(|t| thumbnail_pixels(t).is_some()).map(Into::into);
    // who made it, if the file says
    let png = |bytes: &Option<Vec<u8>>| bytes.as_deref().filter(|p| decode_png(p).is_some()).map(Into::into);
    let (picture, signature) = (png(&file.author_picture), png(&file.signature));
    let name = file.author.trim();
    if !name.is_empty() || picture.is_some() || signature.is_some() {
        b.author = Some(Author { name: name.chars().take(256).collect(), picture, signature });
    }
    b
}

/// A preview image (PNG) as premultiplied RGBA8, with its width and height; none if it can't be
/// read.
pub(crate) fn thumbnail_pixels(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, channels, buf) = decode_png(bytes)?;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in buf.chunks_exact(channels.max(1)).take(w * h) {
        let get = |k: usize| px.get(k).copied().unwrap_or(0);
        let (rgb, alpha) = match channels {
            1 => ([get(0); 3], 255),
            2 => ([get(0); 3], get(1)),
            3 => ([get(0), get(1), get(2)], 255),
            _ => ([get(0), get(1), get(2)], get(3)),
        };
        let times = |v: u8| ((u16::from(v) * u16::from(alpha) + 127) / 255) as u8;
        rgba.extend([times(rgb[0]), times(rgb[1]), times(rgb[2]), alpha]);
    }
    (rgba.len() == w * h * 4).then_some((w as u32, h as u32, rgba))
}

/// A PNG's pixels at 8 bits a sample: width, height, samples a pixel and the samples.
fn decode_png(bytes: &[u8]) -> Option<(usize, usize, usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    // a small file can claim a picture of gigabytes
    let mut buf = vec![0; reader.output_buffer_size().filter(|n| *n <= MOST_PNG_BYTES)?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    (w > 0 && h > 0).then_some((w, h, info.color_type.samples(), buf))
}

/// The shape for a file's: its image, or our nearest, or the round tip at a hardness.
pub(crate) fn shape_for(t: &Texture) -> (Option<u32>, Option<Source>, Option<f32>) {
    match t {
        Texture::None => (None, None, None),
        Texture::Image { bytes, inverted } => match gray_texture(bytes, *inverted) {
            Some(s) => (None, Some(s), None),
            None => (Some(shape::GRAINY_DISC), None, None),
        },
        Texture::Stock { name, inverted } => {
            let n = name.to_lowercase();
            let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
            if has(&["blank"]) {
                // a plain image: the stamp's whole square (nothing, inverted)
                let fill = if *inverted { 0 } else { 255 };
                (None, Source::new(vec![fill; TEXTURE_SIZE * TEXTURE_SIZE]), None)
            } else if has(&["medium-hard", "medium hard", "mediumhard"]) {
                (None, None, Some(0.75))
            } else if has(&["hard", "pen", "solid", "oval", "round"]) && !has(&["soft", "ink"]) {
                (None, None, Some(0.95))
            } else if has(&["medium"]) {
                (None, None, Some(0.5))
            } else if has(&["soft", "air", "blur", "glow"]) {
                (Some(shape::SOFT_ROUND), None, None)
            } else {
                (Some(stock_shape(&n)), None, None)
            }
        }
    }
}

/// Our shape nearest a stock shape's name.
fn stock_shape(n: &str) -> u32 {
    let table: &[(&[&str], u32)] = &[
        (&["brick"], shape::BLOCK),
        (&["spray", "mist", "aero", "speckle"], shape::SPRAY),
        (&["splat", "flick", "spatter", "drip"], shape::SPLATTER),
        (&["bristle", "fan", "hair", "fur"], shape::BRISTLES),
        (&["flat", "chisel", "square brush"], shape::FLAT),
        (&["dry", "streak", "scrape"], shape::DRY),
        (&["chalk", "pastel", "crayon", "charcoal", "conte", "graphite", "pencil"], shape::STOCK_PASTEL),
        (&["stamp", "dash", "impress"], shape::STOCK_DAB),
        (&["dab", "paint", "acrylic", "oil", "gouache", "chunk", "rough"], shape::STOCK_CHUNK),
        (&["blot", "blob", "water", "wash", "stain", "bleed", "ink", "mess", "sponge"], shape::BLOT),
        (&["leaf", "foliage", "tree"], shape::LEAF),
        (&["grass", "reed"], shape::GRASS),
        (&["cloud", "fog", "smoke"], shape::CLOUD),
        (&["wisp", "wind"], shape::WISP),
        (&["star", "sparkle", "glint", "flare", "twinkle"], shape::SPARKLE),
        (&["ring", "bokeh", "circle", "bubble"], shape::RING),
        (&["square", "pixel", "block", "box"], shape::SQUARE),
        (&["hatch", "line", "stripe"], shape::HATCH),
        (&["noise", "grain", "grit", "sand", "texture"], shape::STOCK_GRAINY_DISC),
    ];
    table.iter().find(|(words, _)| words.iter().any(|w| n.contains(w))).map_or(shape::STOCK_GRAINY_DISC, |(_, s)| *s)
}

/// What a brush's own preview (PNG) says of a stock grain we don't have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PreviewLook {
    /// The stroke drawn faintly: its pixels on average under [`FAINT_PREVIEW`] of the way to
    /// full, as a stroke worn through by a dark grain is.
    Faint,
    /// The stroke's middle even, its pixels on average within [`EVEN_PREVIEW`] of its fullest
    /// (the preview not wholly opaque): the grain about white.
    Even,
    Plain,
}

fn preview_look(png: &[u8]) -> Option<PreviewLook> {
    let (w, h, rgba) = thumbnail_pixels(png)?;
    let (w, h) = (w as usize, h as usize);
    let (pixels, _) = rgba.as_chunks::<4>();
    let alpha: Vec<f32> = pixels.iter().map(|&[_, _, _, a]| f32::from(a) / 255.0).collect();
    let inked: Vec<f32> = alpha.iter().copied().filter(|&a| a > 13.0 / 255.0).collect();
    if inked.len() < 100 {
        return None;
    }
    let mean = inked.iter().sum::<f32>() / inked.len() as f32;
    if mean < FAINT_PREVIEW {
        return Some(PreviewLook::Faint);
    }
    if mean >= OPAQUE_PREVIEW {
        return Some(PreviewLook::Plain);
    }
    // the stroke's middle: pixels with ink at every pixel up to 3 steps away
    let inked_at = |x: isize, y: isize| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && alpha.get(y as usize * w + x as usize).is_some_and(|&a| a > 0.05)
    };
    let mut middle = Vec::new();
    for y in 0..h as isize {
        for x in 0..w as isize {
            let all = (-3..=3isize).all(|dy| {
                let reach = 3 - dy.abs();
                (-reach..=reach).all(|dx| inked_at(x + dx, y + dy))
            });
            if all && let Some(&a) = alpha.get(y as usize * w + x as usize) {
                middle.push(a);
            }
        }
    }
    if middle.len() < 200 {
        return Some(PreviewLook::Plain);
    }
    middle.sort_by(f32::total_cmp);
    let fullest = middle.get(middle.len() * 95 / 100).copied().unwrap_or(1.0).max(1e-3);
    let even = middle.iter().sum::<f32>() / middle.len() as f32 / fullest >= EVEN_PREVIEW;
    Some(if even { PreviewLook::Even } else { PreviewLook::Plain })
}

/// The share of full under which a preview's stroke is faint (see [`preview_look`]): measured,
/// a brush on a dark speckled grain draws its preview at 0.26 on average, those on lighter grains
/// at 0.40 and up (soft brushes with no grain at all at 0.65).
const FAINT_PREVIEW: f32 = 0.35;

/// How even a preview's middle is, its mean over its 95th percentile, at and above which the
/// grain is taken as about white: the stock brushes' previews this even are mostly on grains of
/// 0.84-0.91 (ink and cloud textures), those less even on 0.06-0.84.
const EVEN_PREVIEW: f32 = 0.95;

/// The share of full at and above which a preview is wholly opaque (drawn as a flat stamp), which
/// says nothing of its grain.
const OPAQUE_PREVIEW: f32 = 0.99;

/// The grain for a file's: its image, or our nearest, or none. A stock grain whose name says nothing
/// we know stands in as a dark speckle if the brush's own preview (`preview`, a PNG) draws faintly,
/// as none if its middle draws evenly, else as a light mottle, as most stock grains are.
pub(crate) fn grain_for(t: &Texture, preview: Option<&[u8]>) -> (Option<u32>, Option<Source>) {
    match t {
        Texture::None => (None, None),
        Texture::Image { bytes, inverted } => match gray_texture(bytes, *inverted) {
            Some(s) => (None, Some(s)),
            None => (Some(grain::PAPER), None),
        },
        Texture::Stock { name, .. } => {
            let n = name.to_lowercase();
            if n.contains("blank") {
                return (None, None);
            }
            let unknown = || match preview.and_then(preview_look) {
                Some(PreviewLook::Faint) => Some(grain::SPECKLED),
                // about white: as good as none
                Some(PreviewLook::Even) => None,
                _ => Some(grain::MOTTLED),
            };
            (named_grain(&n).map_or_else(unknown, Some), None)
        }
    }
}

/// The source a file's metalness or roughness is painted through: its image, or our nearest grain
/// to a stock image it names, or none (the amount all over) for one it can't show or names in
/// words we don't know.
pub(crate) fn material_for(t: &Texture) -> (Option<u32>, Option<Source>) {
    match t {
        Texture::None => (None, None),
        Texture::Image { bytes, inverted } => (None, gray_texture(bytes, *inverted)),
        Texture::Stock { name, .. } => {
            let n = name.to_lowercase();
            (if n.contains("blank") { None } else { named_grain(&n) }, None)
        }
    }
}

/// Our grain nearest to a stock image named `n` (lower case), if its name says what it is.
fn named_grain(n: &str) -> Option<u32> {
    let table: &[(&[&str], u32)] = &[
        (&["recycled"], grain::RECYCLED),
        (&["canvas", "weave", "burlap", "cloth", "fabric"], grain::WEAVE),
        (&["linen"], grain::LINEN),
        (&["denim", "twill"], grain::TWILL),
        (&["wood"], grain::WOOD),
        (&["brick"], grain::BRICK),
        (&["concrete", "stone", "rock", "wall", "asphalt"], grain::CONCRETE),
        (&["halftone", "dot"], grain::HALFTONE),
        (&["hatch"], grain::HATCHING),
        (&["crumple", "crease", "fold"], grain::CRUMPLED),
        (&["stipple"], grain::STIPPLE),
        (&["wash", "water", "wet"], grain::WASH),
        (&["marble"], grain::MARBLE),
        (&["cell", "organic"], grain::CELLS),
        (&["scale"], grain::SCALES),
        (&["hex", "honeycomb"], grain::HEXAGONS),
        (&["leather", "skin"], grain::LEATHER),
        (&["bark"], grain::BARK),
        (&["rust"], grain::RUST),
        (&["scratch"], grain::SCRATCHES),
        // a sheet of paper: a worn, light one, or crumpled
        (&["dirty-paper", "dirty paper", "worn-paper", "worn paper"], grain::WORN),
        (&["paper"], grain::SHEET),
        (&["grunge", "dirt", "dust", "dirty"], grain::GRUNGE),
        (&["fibre", "fiber", "hair", "straw"], grain::FIBRE),
        (&["sand", "grit"], grain::SAND),
        (&["noise", "static"], grain::FINE_NOISE),
        (&["charcoal", "willow", "chalk", "pastel", "rough", "coarse", "corse"], grain::CHARCOAL),
    ];
    table.iter().find(|(words, _)| words.iter().any(|w| n.contains(w))).map(|(_, g)| *g)
}

/// An image (PNG) as a shape or grain: grey (its alpha taken in), [`TEXTURE_SIZE`] square, white
/// for paint; none if it can't be read.
fn gray_texture(bytes: &[u8], inverted: bool) -> Option<Source> {
    let (w, h, channels, buf) = decode_png(bytes)?;
    let value = |x: usize, y: usize| -> f32 {
        let at = (y * w + x) * channels;
        let px = buf.get(at..at + channels).unwrap_or(&[]);
        let get = |k: usize| f32::from(px.get(k).copied().unwrap_or(0));
        let (lum, alpha) = match channels {
            1 => (get(0), 255.0),
            2 => (get(0), get(1)),
            3 => (0.299 * get(0) + 0.587 * get(1) + 0.114 * get(2), 255.0),
            _ => (0.299 * get(0) + 0.587 * get(1) + 0.114 * get(2), get(3)),
        };
        lum * alpha / 255.0
    };
    let n = TEXTURE_SIZE;
    // a smaller image is blown up smoothly, between its pixels' centres
    let between = |fx: f32, fy: f32| -> f32 {
        let (fx, fy) = (fx.clamp(0.0, (w - 1) as f32), fy.clamp(0.0, (h - 1) as f32));
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (u, v) = (fx - x0 as f32, fy - y0 as f32);
        let top = value(x0, y0) * (1.0 - u) + value(x1, y0) * u;
        let bottom = value(x0, y1) * (1.0 - u) + value(x1, y1) * u;
        top * (1.0 - v) + bottom * v
    };
    let mut pixels = Vec::with_capacity(n * n);
    for ty in 0..n {
        let (y0, y1) = (ty * h / n, ((ty + 1) * h / n).max(ty * h / n + 1).min(h));
        for tx in 0..n {
            let v = if w < n || h < n {
                let at = |t: usize, size: usize| (t as f32 + 0.5) * size as f32 / n as f32 - 0.5;
                between(at(tx, w), at(ty, h))
            } else {
                // each texture pixel the average of the image's pixels it covers
                let (x0, x1) = (tx * w / n, ((tx + 1) * w / n).max(tx * w / n + 1).min(w));
                let mut sum = 0.0;
                for y in y0..y1 {
                    for x in x0..x1 {
                        sum += value(x, y);
                    }
                }
                sum / ((y1 - y0) * (x1 - x0)).max(1) as f32
            };
            let v = if inverted { 255.0 - v } else { v };
            pixels.push(v.round().clamp(0.0, 255.0) as u8);
        }
    }
    Source::new(pixels)
}

/// The brush's shape as an image: its own, one of ours, or its round tip drawn.
fn shape_pixels(b: &Brush) -> Vec<u8> {
    if let Some(s) = &b.shape.source {
        return s.pixels.to_vec();
    }
    if let Some(img) = b.shape.image.and_then(|i| textures::shapes().into_iter().nth(i as usize)) {
        return img.pixels;
    }
    // the round tip filling the image, its edge as soft as the brush's hardness says
    let n = TEXTURE_SIZE;
    let soft = (1.0 - b.hardness.clamp(0.0, 1.0)).max(0.02);
    (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f32 + 0.5, (i / n) as f32 + 0.5);
            let r = ((x / n as f32 - 0.5).powi(2) + (y / n as f32 - 0.5).powi(2)).sqrt() * 2.0;
            let v = ((1.0 - r) / soft).clamp(0.0, 1.0);
            (v * v * (3.0 - 2.0 * v) * 255.0).round() as u8
        })
        .collect()
}

/// The brush's grain as an image, if it has one.
fn grain_pixels(b: &Brush) -> Option<Vec<u8>> {
    if let Some(s) = &b.grain.source {
        return Some(s.pixels.to_vec());
    }
    b.grain.image.and_then(|i| textures::grains().into_iter().nth(i as usize)).map(|g| g.pixels)
}

/// A metalness or roughness source as an image, if it has one.
pub(crate) fn material_pixels(m: &MaterialSource) -> Option<Vec<u8>> {
    if let Some(s) = &m.source {
        return Some(s.pixels.to_vec());
    }
    m.image.and_then(|i| textures::grains().into_iter().nth(i as usize)).map(|g| g.pixels)
}

/// A square grey PNG of `pixels`.
fn gray_png(pixels: &[u8]) -> Vec<u8> {
    let n = TEXTURE_SIZE as u32;
    sized_gray_png(n, n, pixels)
}

/// A grey PNG of `pixels`, `width` x `height`.
fn sized_gray_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(pixels);
        let _ = writer.finish();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_names_find_our_nearest() {
        assert_eq!(
            shape_for(&Texture::Stock { name: "Brush-Hard.png".into(), inverted: false }),
            (None, None, Some(0.95))
        );
        assert_eq!(shape_for(&Texture::Stock { name: "Soft.png".into(), inverted: false }).0, Some(shape::SOFT_ROUND));
        assert_eq!(shape_for(&Texture::Stock { name: "Spray-Fine.jpg".into(), inverted: false }).0, Some(shape::SPRAY));
        assert_eq!(grain_for(&Texture::Stock { name: "Blank.png".into(), inverted: false }, None), (None, None));
        assert_eq!(
            grain_for(&Texture::Stock { name: "Old-Canvas.jpg".into(), inverted: false }, None).0,
            Some(grain::WEAVE)
        );
        // a name we don't know: a light mottle, a dark speckle if the brush's preview is faint, or
        // none if its middle is even
        let mystery = Texture::Stock { name: "Mystery.png".into(), inverted: false };
        let preview = |alpha: u8| crate::project::rgba_png(40, 10, &[alpha, alpha, alpha, alpha].repeat(400));
        assert_eq!(grain_for(&mystery, None).0, Some(grain::MOTTLED));
        assert_eq!(grain_for(&mystery, Some(&preview(60))).0, Some(grain::SPECKLED));
        assert_eq!(grain_for(&mystery, Some(&preview(200))).0, Some(grain::MOTTLED));
        let stroke = |alpha: &dyn Fn(usize) -> u8| {
            let px: Vec<u8> = (0..60 * 30).flat_map(|i| [alpha(i); 4]).collect();
            crate::project::rgba_png(60, 30, &px)
        };
        assert_eq!(grain_for(&mystery, Some(&stroke(&|_| 220))).0, None);
        assert_eq!(grain_for(&mystery, Some(&stroke(&|i| if i % 3 == 0 { 120 } else { 240 }))).0, Some(grain::MOTTLED));
        assert_eq!(
            grain_for(&Texture::Stock { name: "Munched-Paper.png".into(), inverted: false }, None).0,
            Some(grain::SHEET)
        );
        assert_eq!(
            grain_for(&Texture::Stock { name: "Dirty-Paper.jpg".into(), inverted: false }, None).0,
            Some(grain::WORN)
        );
        assert_eq!(
            grain_for(&Texture::Stock { name: "Charcoal-Corse.jpg".into(), inverted: false }, None).0,
            Some(grain::CHARCOAL)
        );
        // willow charcoal's grain is a charcoal's
        assert_eq!(
            grain_for(&Texture::Stock { name: "Willow-Grain.jpg".into(), inverted: false }, None).0,
            Some(grain::CHARCOAL)
        );
    }

    #[test]
    fn a_sub_tools_texture_becomes_its_grain() {
        let px: Vec<u8> = (0..64 * 64).flat_map(|i| [(i % 64 * 4) as u8, 0, 0, 255]).collect();
        let paper = crate::project::rgba_png(64, 64, &px);
        let c = procraft_engine::csp::CspBrush {
            name: "Pencil".into(),
            hardness: None,
            spacing: None,
            roundness: None,
            angle: 0.0,
            tip_pngs: Vec::new(),
            texture: Some(procraft_engine::csp::CspTexture {
                pngs: vec![b"not a picture".to_vec(), crate::project::rgba_png(8, 8, &[0; 256]), paper],
                density: Some(0.6),
                scale: Some(2.0),
                reverse: false,
            }),
        };
        let b = csp_brush(&c);
        // the picture that reads and isn't blank, shown at 60 %, its 64 px tiles twice as big
        assert!(b.grain.source.is_some() && b.grain.image.is_none());
        assert_eq!((b.grain.depth, b.grain.scale), (0.6, 128.0));
        // none: no grain
        let plain = csp_brush(&procraft_engine::csp::CspBrush { texture: None, ..c });
        assert!(plain.grain.source.is_none());
    }

    #[test]
    fn a_photoshop_preset_comes_in_at_its_size_with_its_dynamics() {
        use procraft_engine::abr::{AbrBrush, AbrDynamics, AbrTip};
        let shown = |b: &Brush, key: &str| (b.setting(key) * 100.0).round();
        // a computed 10 px round, pressure on its size (down to 87%) and opacity
        let round = AbrBrush {
            name: "Hard".into(),
            tip: None,
            spacing: Some(0.09),
            angle: 129f32.to_radians(),
            roundness: Some(1.0),
            hardness: Some(1.0),
            diameter: Some(10.0),
            texture: None,
            dynamics: AbrDynamics {
                tip: true,
                min_diameter: 0.87,
                size_by_pressure: true,
                paint: true,
                opacity_by_pressure: true,
                flow_by_pressure: false,
            },
        };
        let b = abr_brush(&round);
        // most size 33% (as the file keeps it), the least 1%
        assert_eq!(
            ((b.max_size / b.size_unit() * 100.0).round(), (b.min_size / b.size_unit() * 100.0).round()),
            (33.0, 1.0)
        );
        // pressure: size 6.5% (half of what the dynamics let go), the stylus page's opacity full
        assert!((b.setting("pressure_size") - 0.065).abs() < 1e-6);
        assert_eq!((shown(&b, "pressure_flow"), shown(&b, "pressure_opacity")), (100.0, 0.0));
        // untapered, no StreamLine, a uniformed glaze; turned the other way
        assert_eq!(
            (b.setting("pen_taper_size"), b.setting("touch_taper_size"), b.setting("streamline")),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(b.setting("rendering_mode"), 1.0);
        assert!((b.shape.rotation + 129f32.to_radians()).abs() < 1e-6);
        // a sampled 165 x 91 tip at 165 px: no taller than its picture, 298%
        let tip = AbrTip { name: "Tip".into(), spacing: None, width: 165, height: 91, pixels: vec![255; 165 * 91] };
        let sampled =
            abr_brush(&AbrBrush { tip: Some(tip), diameter: Some(165.0), dynamics: AbrDynamics::default(), ..round });
        assert_eq!((sampled.max_size / sampled.size_unit() * 100.0).round(), 298.0);
        assert_eq!(sampled.setting("pressure_size"), 0.0);
    }

    #[test]
    fn an_image_becomes_a_texture() {
        // a 4 × 4 grey PNG: white on the left half
        let px: Vec<u8> = (0..16).map(|i| if i % 4 < 2 { 255 } else { 0 }).collect();
        let mut png_bytes = Vec::new();
        let mut e = png::Encoder::new(&mut png_bytes, 4, 4);
        e.set_color(png::ColorType::Grayscale);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&px).unwrap();
        w.finish().unwrap();
        let s = gray_texture(&png_bytes, false).unwrap();
        let n = TEXTURE_SIZE;
        assert_eq!((s.pixels[n / 2 * n + 10], s.pixels[n / 2 * n + n - 10]), (255, 0));
        // blown up smoothly: the edge between the halves fades across
        assert!((100..156).contains(&s.pixels[n / 2 * n + n / 2]), "{}", s.pixels[n / 2 * n + n / 2]);
        assert!(s.pixels[n / 2 * n + n / 2 - n / 16] > s.pixels[n / 2 * n + n / 2]);
        let inv = gray_texture(&png_bytes, true).unwrap();
        assert_eq!(inv.pixels[n / 2 * n + 10], 0);
        assert!(gray_texture(b"not an image", false).is_none());
        // our own textures go out and come back the same
        let back = gray_texture(&gray_png(&textures::grains()[3].pixels), false).unwrap();
        assert_eq!(*back.pixels, textures::grains()[3].pixels);
    }

    #[test]
    fn folder_ids_look_like_the_files_own() {
        let id = folder_id(42, 3);
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
        assert_ne!(id, folder_id(42, 4));
    }
}
