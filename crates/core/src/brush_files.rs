//! Brush and set files as other painting apps share them (`.brush`, `.brushset`): read into the
//! library, and written from it. A brush's shape and grain images go with it; an image a file only
//! names (one that came with the app that made it) is stood in for by the nearest of ours.

use procraft_engine::brush::{Author, Brush, GrainMode, MaterialSource, Source, StockImage, StockImages};
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

/// How much closer a Photoshop preset's stamps come than its spacing says, in pixels of its tip.
const ABR_SPACING_LESS_PX: f32 = 0.5;

/// A brush file's pictures, made: the shape, the grain if any and the preview (PNGs).
struct Made {
    shape: Vec<u8>,
    grain: Option<Vec<u8>>,
    metallic: Option<Vec<u8>>,
    roughness: Option<Vec<u8>>,
    thumbnail: Vec<u8>,
    author: Option<std::sync::Arc<[u8]>>,
    signature: Option<std::sync::Arc<[u8]>>,
    /// A dual brush's second's (no preview).
    secondary: Option<Box<Made>>,
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
            secondary: self.secondary.as_ref().map(|m| Box::new(m.pictures())),
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
            secondary: brush.secondary.as_ref().map(|second| {
                Box::new(Made {
                    shape: gray_png(&shape_pixels(second)),
                    grain: grain_pixels(second).map(|p| gray_png(&p)),
                    metallic: material_pixels(&second.metallic).map(|p| gray_png(&p)),
                    roughness: material_pixels(&second.roughness).map(|p| gray_png(&p)),
                    thumbnail: Vec::new(),
                    author: None,
                    signature: None,
                    secondary: None,
                })
            }),
        }
    }
}

/// Other apps' brush files, by the endings of their names: Photoshop's, GIMP's brushes, pipes and
/// generated brushes, Krita's presets and bundles, Clip Studio's sub tools.
pub const OTHER_BRUSH_EXTENSIONS: [&str; 7] = ["abr", "gbr", "gih", "vbr", "kpp", "bundle", "sut"];

/// Whether `bytes` are a zip holding brushes (a brush's archive, or a set's list), whatever the
/// file is called: a brush or set file sent on as a plain zip.
#[uniffi::export]
pub fn is_brush_zip(bytes: Vec<u8>) -> bool {
    procraft_engine::zip::ZipReader::new(&bytes).is_some_and(|zip| brush_names(zip.names()))
}

/// Whether a zip holding `names` holds brushes.
pub(crate) fn brush_names<'n>(mut names: impl Iterator<Item = &'n str>) -> bool {
    names.any(|n| n == "Brush.archive" || n == "brushset.plist" || n.ends_with("/Brush.archive"))
}

/// Brushes read from a file, not yet in the library: reading another app's takes a while, so it
/// can be done off the interface's thread and the brushes added on it after
/// ([`CanvasSession::add_read_brushes`]), quickly.
pub enum ReadBrushes {
    /// A set of their own at the top of the library (a set file's, or another app's brushes),
    /// called `name` ("Imported" if that is blank).
    Set { name: String, brushes: Vec<Brush> },
    /// One brush, at the top of the "Imported" set.
    One(Box<Brush>),
    /// A file of ours from before (a set, or a brush for the set shown), as its text.
    Text(String),
}

/// Our brush file's or set file's brushes in `bytes`.
fn read_ours(bytes: &[u8]) -> Option<ReadBrushes> {
    if let Some((name, files)) = brushfile::read_set(bytes) {
        return Some(ReadBrushes::Set { name, brushes: files.iter().map(brush_from).collect() });
    }
    Some(ReadBrushes::One(Box::new(brush_from(&brushfile::read(bytes)?))))
}

/// Another app's brushes in `bytes` (see [`CanvasSession::import_other_brushes`]), in a set
/// called `name` unless the file names its own; none if it holds none.
fn read_other(bytes: &[u8], name: String) -> Option<ReadBrushes> {
    let (name, brushes) = other_brushes(bytes, name)?;
    (!brushes.is_empty()).then_some(ReadBrushes::Set { name, brushes })
}

/// The brushes in a file the user chose, called `file_name` (if known), as
/// [`CanvasSession::import_any_brushes`] takes them, not yet added: another app's when its name
/// says so (see [`OTHER_BRUSH_EXTENSIONS`]); else ours (a brush or set file, or one of ours from
/// before), then another app's told apart by its contents. None if the file holds none.
pub fn read_any_brushes(bytes: &[u8], file_name: Option<String>) -> Option<ReadBrushes> {
    let name = file_name.unwrap_or_default();
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem).trim().to_owned();
    let other = name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| OTHER_BRUSH_EXTENSIONS.iter().any(|o| o.eq_ignore_ascii_case(ext)));
    if other {
        return read_other(bytes, stem);
    }
    read_ours(bytes)
        .or_else(|| {
            let text = std::str::from_utf8(bytes).ok().filter(|t| crate::brush_library::is_brush_text(t))?;
            Some(ReadBrushes::Text(text.to_owned()))
        })
        .or_else(|| read_other(bytes, if stem.is_empty() { IMPORTED.into() } else { stem }))
}

impl CanvasSession {
    /// Adds brushes read from a file ([`read_any_brushes`]) to the library: a set of their own at
    /// the top, one brush to the top of the "Imported" set (made at the top of the library if
    /// there isn't one), one of ours from before as its file says (a brush into set `shown`).
    /// Returns the set they went into, or none if they couldn't be added.
    pub fn add_read_brushes(&self, read: ReadBrushes, shown: u32) -> Option<u32> {
        match read {
            ReadBrushes::Set { name, brushes } => {
                let mut st = self.shared.lock();
                let name = if name.trim().is_empty() { IMPORTED.to_owned() } else { name.trim().to_owned() };
                let origins = vec![None; brushes.len()];
                st.library.insert(0, LibrarySet { name, brushes, origins, icon: crate::brush_library::IMPORTED_ICON });
                st.move_brush_refs(|(rs, ri)| Some((rs + 1, ri)));
                Some(0)
            }
            ReadBrushes::One(brush) => {
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
                entry.brushes.insert(0, *brush);
                entry.origins.insert(0, None);
                st.move_brush_refs(|(rs, ri)| Some(if rs == at { (rs, ri + 1) } else { (rs, ri) }));
                Some(at as u32)
            }
            ReadBrushes::Text(text) => self.import_brush_file(shown, text),
        }
    }
}

/// Brushes read from a file for an app's interface (Android's) to add on its own thread once
/// nothing there holds a library place (see [`ReadBrushes`]); the first add takes them.
#[derive(uniffi::Object)]
pub struct BrushesRead(std::sync::Mutex<Option<ReadBrushes>>);

/// The brushes in a file the user chose, called `file_name` (if known), read as
/// [`read_any_brushes`] reads them, not yet added; none if it holds none.
#[uniffi::export]
pub fn read_brushes(bytes: Vec<u8>, file_name: Option<String>) -> Option<std::sync::Arc<BrushesRead>> {
    read_any_brushes(&bytes, file_name).map(|read| std::sync::Arc::new(BrushesRead(std::sync::Mutex::new(Some(read)))))
}

#[uniffi::export]
impl CanvasSession {
    /// Adds brushes read meanwhile ([`read_brushes`]) to the library, as
    /// [`CanvasSession::add_read_brushes`] does (one of ours from before into set `shown`).
    /// Returns the set they went into; none if they were added already or couldn't be.
    pub fn add_brushes_read(&self, read: std::sync::Arc<BrushesRead>, shown: u32) -> Option<u32> {
        let brushes = read.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()?;
        self.add_read_brushes(brushes, shown)
    }

    /// Adds the brushes in `bytes`: a brush file's (to the top of the "Imported" set, made at the
    /// top of the library if there isn't one) or a set file's (a set of their own at the top).
    /// Returns the set they went into, or none if the file is neither.
    pub fn import_brushes(&self, bytes: Vec<u8>) -> Option<u32> {
        self.add_read_brushes(read_ours(&bytes)?, 0)
    }

    /// Adds the brushes in another app's file (`bytes`): a Photoshop brush file's sampled tips, a
    /// GIMP brush or pipe, a Krita preset or bundle, or a Clip Studio sub tool. They go in a set of their own named `name`
    /// (a bundle's own name first; else "Imported") at the top of the library, each with the
    /// file's shape and spacing. Returns the set, or none if the file holds none.
    pub fn import_other_brushes(&self, bytes: Vec<u8>, name: String) -> Option<u32> {
        self.add_read_brushes(read_other(&bytes, name)?, 0)
    }

    /// Adds the brushes in a file the user chose, called `file_name` (if known), into the library
    /// (see [`read_any_brushes`]; one of ours from before put in set `shown`). Returns the set
    /// they went into, or none if the file holds none.
    pub fn import_any_brushes(&self, bytes: Vec<u8>, file_name: Option<String>, shown: u32) -> Option<u32> {
        self.add_read_brushes(read_any_brushes(&bytes, file_name)?, shown)
    }

    /// Brush `index` of `set` as a brush file.
    pub fn brush_as_file(&self, set: u32, index: u32) -> Option<Vec<u8>> {
        let brush = {
            let st = self.shared.lock();
            st.with_sliders(st.library.get(set as usize)?.brushes.get(index as usize)?, set as usize, index as usize)
        };
        let made = self.brush_pictures(&brush, set, index);
        Some(brushfile::write(&brush, author_name(&brush), &made.pictures()))
    }

    /// Set `set` as a set file.
    pub fn brush_set_as_file(&self, set: u32) -> Option<Vec<u8>> {
        let (name, brushes) = {
            let st = self.shared.lock();
            let entry = st.library.get(set as usize)?;
            let brushes: Vec<Brush> =
                entry.brushes.iter().enumerate().map(|(i, b)| st.with_sliders(b, set as usize, i)).collect();
            (entry.name.clone(), brushes)
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
    // each made a brush side by side (a big tip takes a while), in their order
    fn each<T: Sync>(items: &[T], f: impl Fn(&T) -> Brush + Sync) -> Vec<Brush> {
        procraft_engine::par::side_by_side(items, f).into_iter().flatten().collect()
    }
    if let Some(brushes) = procraft_engine::abr::read_brushes(bytes) {
        return Some((name, each(&brushes, abr_brush)));
    }
    if let Some(tips) = procraft_engine::gbr::read(bytes) {
        return Some((name, each(&tips, tip_brush)));
    }
    if let Some(g) = procraft_engine::gbr::read_generated(bytes) {
        return Some((name, vec![generated_brush(&g)]));
    }
    if let Some((title, presets)) = krita::read_bundle(bytes) {
        let name = if title.trim().is_empty() { name } else { title };
        return Some((name, each(&presets, krita_brush)));
    }
    if let Some(brushes) = procraft_engine::csp::read(bytes) {
        return Some((name, each(&brushes, csp_brush)));
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
        // half a pixel less, as a share of the tip's diameter: a small tip's stamps come closer,
        // and a spacing of under half a pixel is none
        let less = a.diameter.filter(|d| *d > 0.0).map_or(0.0, |d| ABR_SPACING_LESS_PX / d);
        b.spacing = Brush::spacing_within_reach((spacing - less).max(0.0));
    }
    // Photoshop turns the other way (its 129 degrees are -129 here)
    b.shape.rotation = -a.angle;
    let d = a.dynamics;
    if d.tip && d.angle_control == procraft_engine::abr::ANGLE_BY_DIRECTION {
        b.shape.follow = 1.0;
    }
    let flow = d.tool_flow.unwrap_or(1.0);
    // as a preset comes in elsewhere: no taper, no StreamLine, glazed uniformly at its tool's flow
    for (key, value) in [
        ("pen_taper_size", 0.0),
        ("pen_taper_opacity", 0.0),
        ("touch_taper_size", 0.0),
        ("touch_taper_opacity", 0.0),
        ("streamline", 0.0),
        ("rendering_mode", 1.0),
        ("flow", flow),
    ] {
        b.set_setting(key, value);
    }
    if let Some(t) = &a.texture {
        let png = sized_gray_png(t.width, t.height, &t.pixels);
        paper(&mut b, &[png], t.invert, t.depth.unwrap_or(1.0), t.scale.unwrap_or(1.0));
        if b.grain.source.is_some() {
            // a stamp's own texture moves with it; else it lies still under the stroke
            b.set_setting("grain_behavior", if t.each_tip { 0.0 } else { 1.0 });
            b.set_setting("grain_depth_min", t.min_depth.unwrap_or(0.0));
            b.set_setting("grain_brightness", (t.brightness as f32 / 255.0).clamp(-1.0, 1.0));
            b.set_setting("grain_contrast", (t.contrast as f32 / 100.0).clamp(-1.0, 1.0));
            if let Some(mode) = t.blend.as_deref().and_then(texture_blend) {
                b.set_setting("grain_blend", mode as f32);
            }
        }
    }
    // its most size from its tip's pixels (a sampled tip's no taller than its picture), the least 1%
    let pixels = match (&a.tip, a.diameter) {
        (None, d) => d,
        (Some(t), d) => Some(d.map_or(t.height as f32, |d| d.min(t.height as f32))),
    };
    if let Some(px) = pixels {
        procraft_engine::brushfile::set_sizes(&mut b, px * ABR_SIZE_PER_PIXEL, Some(0.01));
    }
    // the shape dynamics: pressure or tilt (controls 2, 3) on the size, half of what it lets go
    // of; tilt to the full; its size jitter
    let sized = d.tip && matches!(d.size_control, 2 | 3);
    b.set_setting("pressure_size", if sized { (1.0 - d.min_diameter) / 2.0 } else { 0.0 });
    b.set_setting("tilt_size", if d.tip && d.size_control == 3 { 1.0 } else { 0.0 });
    b.set_setting("jitter_size", if d.tip { d.size_jitter } else { 0.0 });
    // the paint dynamics, pressure's (control 2): the opacity down to its least, the flow from the
    // tool's down to its least; the flow's jitter as the opacity's. The stylus page's opacity is
    // kept as "pressure_flow", its flow as "pressure_opacity".
    let pressed = |control: i32| d.paint && control == 2;
    b.set_setting("pressure_flow", if pressed(d.opacity_control) { 1.0 - d.opacity_min } else { 0.0 });
    b.set_setting("pressure_opacity", if pressed(d.flow_control) { flow * (1.0 - d.flow_min) } else { 0.0 });
    b.set_setting("jitter_opacity", if d.paint { d.flow_jitter } else { 0.0 });
    b
}

/// The grain blend mode of a Photoshop texture's mode (its four-letter name), if we have it.
fn texture_blend(name: &str) -> Option<u32> {
    use procraft_engine::document::BlendMode;
    Some(match name {
        "Mltp" => BlendMode::Multiply.id(),
        "Sbtr" => BlendMode::Subtract.id(),
        "Drkn" => BlendMode::Darken.id(),
        "Ovrl" => BlendMode::Overlay.id(),
        "CDdg" => BlendMode::ColorDodge.id(),
        "CBrn" => BlendMode::ColorBurn.id(),
        "linearBurn" => BlendMode::LinearBurn.id(),
        "hardMix" => BlendMode::HardMix.id(),
        "Hght" => procraft_engine::brush::GRAIN_HEIGHT,
        "linearHeight" => procraft_engine::brush::GRAIN_LINEAR_HEIGHT,
        _ => return None,
    })
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
    if k.follows {
        b.shape.follow = 1.0;
    }
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
    // a file from before a control was kept doesn't name it: it draws as such a brush did then,
    // not as our own pen starts: its grain at full depth, travelling with each stamp (as far as
    // its movement says)
    let names = |key: &str| file.settings.iter().any(|(k, _)| k == key);
    if !names("grain_depth") {
        b.grain.depth = 1.0;
    }
    if !names("grain_behavior") && names("grain_movement") {
        b.grain.mode = GrainMode::Moving;
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
    b.sliders = file.sliders;
    b.secondary = file.secondary.as_deref().map(|second| Box::new(brush_from(second)));
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
            let (image, source, hardness) = stock_stand_in(name, *inverted);
            // our shape with the stock one's own lights and darks and its rows' share of the ink,
            // where they were measured
            let retoned =
                image.filter(|_| source.is_none()).zip(textures::stock_shape_tones(name)).and_then(|(i, tones)| {
                    let shape = textures::shapes().into_iter().nth(i as usize)?;
                    // its streaks and gaps across the stroke first, where they were measured
                    let striped = textures::stock_shape_rows(name).map(|rows| textures::striped(&shape.pixels, &rows));
                    let pixels = striped.as_deref().unwrap_or(&shape.pixels);
                    let mut pixels = textures::retoned_shape(pixels, &tones, &textures::SHAPE_TONE_SHARES);
                    if *inverted {
                        pixels.iter_mut().for_each(|v| *v = 255 - *v);
                    }
                    Source::new(pixels)
                });
            (image, retoned.or(source), hardness)
        }
    }
}

/// Our stand-in for the stock shape `name`: our nearest shape, or the round tip at a hardness, or
/// (blank) the whole square.
fn stock_stand_in(name: &str, inverted: bool) -> (Option<u32>, Option<Source>, Option<f32>) {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["blank"]) {
        // a plain image: the stamp's whole square (nothing, inverted)
        let fill = if inverted { 0 } else { 255 };
        (None, Source::new(vec![fill; TEXTURE_SIZE * TEXTURE_SIZE]), None)
    } else if has(&["dots"]) {
        // dots scattered over the stamp, whatever outline they fill
        (Some(stock_shape(&n)), None, None)
    } else if has(&["oval"]) {
        // a flat oval, not the round tip (as wide as its tones say, where they were measured)
        (Some(shape::SLIM_OVAL), None, None)
    } else if has(&["medium-hard", "medium hard", "mediumhard"]) {
        (None, None, Some(0.75))
    } else if has(&["hard", "pen", "solid", "round"]) && !has(&["soft", "ink"]) {
        (None, None, Some(0.95))
    } else if has(&["medium"]) {
        (None, None, Some(0.5))
    } else if has(&["ultra-soft", "ultra soft", "ultrasoft"]) {
        (Some(shape::ULTRA_SOFT_ROUND), None, None)
    } else if has(&["soft", "air", "blur", "glow"]) {
        (Some(shape::SOFT_ROUND), None, None)
    } else {
        (Some(stock_shape(&n)), None, None)
    }
}

/// Our shape nearest a stock shape's name.
fn stock_shape(n: &str) -> u32 {
    let table: &[(&[&str], u32)] = &[
        (&["square-dots", "square dots", "squaredots"], shape::STOCK_SQUARE_DOTS),
        (&["dots"], shape::STOCK_DOTS),
        (&["brick"], shape::BLOCK),
        (&["spray", "mist", "aero", "speckle"], shape::SPRAY),
        (&["splat", "flick", "spatter", "drip"], shape::SPLATTER),
        (&["bristle", "fan", "hair", "fur"], shape::BRISTLES),
        // a flat brush's whole stroke, its bristles' streaks in it
        (&["flat-brush", "flat brush", "flatbrush"], shape::STOCK_FLAT_STROKE),
        (&["flat", "chisel", "square brush"], shape::FLAT),
        (&["dry", "streak", "scrape"], shape::DRY),
        (&["chalk", "pastel", "crayon", "charcoal", "conte", "graphite", "pencil"], shape::STOCK_PASTEL),
        (&["stamp", "dash", "impress"], shape::STOCK_DAB),
        (&["dab", "paint", "acrylic", "oil", "gouache", "chunk", "rough"], shape::STOCK_CHUNK),
        // a dried drop, its edge darker; a thick smear; a dab full of holes
        (&["bleed"], shape::STOCK_BLEED),
        (&["mess"], shape::STOCK_MESS),
        (&["sponge"], shape::SPONGE),
        (&["blot", "blob", "water", "wash", "stain", "ink"], shape::STOCK_BLOT),
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
        Texture::Stock { name, inverted } => {
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
            let image = named_grain(&n).map_or_else(unknown, Some);
            // our pattern with the stock grain's own lights and darks, where they were measured
            let source = image.zip(textures::stock_grain_tones(name)).and_then(|(i, tones)| {
                let pattern = textures::grains().into_iter().nth(i as usize)?;
                let mut pixels = textures::retoned(&pattern.pixels, &tones, &textures::TONE_SHARES);
                if *inverted {
                    pixels.iter_mut().for_each(|v| *v = 255 - *v);
                }
                Source::new(pixels)
            });
            (image, source)
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
        // a canvas painted on: rough, its weave showing through patches of paint
        (&["canvas"], grain::ROUGH_CANVAS),
        (&["weave", "burlap", "cloth", "fabric"], grain::WEAVE),
        (&["linen"], grain::LINEN),
        (&["denim", "twill"], grain::TWILL),
        (&["wood"], grain::WOOD),
        // a wall's rough, pitted face (no joints between bricks, which would cut a stroke into
        // pieces along them)
        (&["concrete", "stone", "rock", "wall", "asphalt", "brick"], grain::CONCRETE),
        (&["halftone", "dot"], grain::HALFTONE),
        (&["hatch"], grain::HATCHING),
        (&["crumple", "crease", "fold"], grain::CRUMPLED),
        (&["stipple"], grain::STIPPLE),
        (&["wash", "water", "wet"], grain::WASH),
        // ink dried in patches, most of it bare
        (&["dry"], grain::DRY_PATCHES),
        // light, flecked with fine spray
        (&["spray"], grain::SPRAYED),
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
        // cardboard as it is pressed in (turned over, its ridges dark): as fine and as dark as
        // the sheet
        (&["paper", "cardboard"], grain::SHEET),
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
pub(crate) fn grain_pixels(b: &Brush) -> Option<Vec<u8>> {
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
        let ultra = Texture::Stock { name: "Ultra-Soft.jpg".into(), inverted: false };
        assert_eq!(shape_for(&ultra).0, Some(shape::ULTRA_SOFT_ROUND));
        assert_eq!(
            shape_for(&Texture::Stock { name: "Slim-Oval.png".into(), inverted: false }),
            (Some(shape::SLIM_OVAL), None, None)
        );
        assert_eq!(shape_for(&Texture::Stock { name: "Spray-Fine.jpg".into(), inverted: false }).0, Some(shape::SPRAY));
        // a flat brush's stroke, streaked; dots, not the round or square they fill
        let shape = |name: &str| shape_for(&Texture::Stock { name: name.into(), inverted: false }).0;
        assert_eq!(shape("Old-Flat-Brush.jpg"), Some(shape::STOCK_FLAT_STROKE));
        assert_eq!(shape("Flat-Marker.jpg"), Some(shape::FLAT));
        assert_eq!(shape("Fine-Round-Dots.png"), Some(shape::STOCK_DOTS));
        assert_eq!(shape("Old-Square-Dots.jpg"), Some(shape::STOCK_SQUARE_DOTS));
        // a stock blot keeps its drops close, not ours that are pushed out for stamps that turn
        assert_eq!(shape("Old-Ink-Blot.jpg"), Some(shape::STOCK_BLOT));
        assert_eq!(grain_for(&Texture::Stock { name: "Blank.png".into(), inverted: false }, None), (None, None));
        assert_eq!(
            grain_for(&Texture::Stock { name: "Old-Canvas.jpg".into(), inverted: false }, None).0,
            Some(grain::ROUGH_CANVAS)
        );
        assert_eq!(
            grain_for(&Texture::Stock { name: "Old-Weave.jpg".into(), inverted: false }, None).0,
            Some(grain::WEAVE)
        );
        // a brick wall's face, not the joints between its bricks
        assert_eq!(
            grain_for(&Texture::Stock { name: "Old-Brick.jpg".into(), inverted: false }, None).0,
            Some(grain::CONCRETE)
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
                size_control: 2,
                min_diameter: 0.87,
                paint: true,
                opacity_control: 2,
                ..AbrDynamics::default()
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
        // spacing, as the studio's control shows it: half a pixel closer than the file says, so
        // 9% at 10 px reads 13%, 7% at 35 px 16%, 7% at 12 px 11%, 1% at 35 px none, 9% at 201 px 20%
        for (spacing, diameter, reads) in
            [(0.09, 10.0, 13.0), (0.07, 35.0, 16.0), (0.07, 12.0, 11.0), (0.01, 35.0, 0.0), (0.09, 201.0, 20.0)]
        {
            let b = abr_brush(&AbrBrush { spacing: Some(spacing), diameter: Some(diameter), ..round.clone() });
            assert_eq!(shown(&b, "spacing"), reads, "{spacing} at {diameter} px");
        }
        // a sampled 165 x 91 tip at 165 px: no taller than its picture, 298%
        let tip = AbrTip { name: "Tip".into(), spacing: None, width: 165, height: 91, pixels: vec![255; 165 * 91] };
        let sampled = abr_brush(&AbrBrush {
            tip: Some(tip),
            diameter: Some(165.0),
            dynamics: AbrDynamics::default(),
            ..round.clone()
        });
        assert_eq!((sampled.max_size / sampled.size_unit() * 100.0).round(), 298.0);
        assert_eq!(sampled.setting("pressure_size"), 0.0);
        // as Kyle's charcoal: its tool's flow 76%, the flow by pressure down to 20% (61% on the
        // stylus page), size by tilt, and a still texture of its own in the Height mode
        let paper = (0..64 * 64).map(|i| (i * 7 % 251) as u8).collect();
        let kyle = abr_brush(&AbrBrush {
            texture: Some(procraft_engine::abr::AbrTexture {
                name: "Grainy".into(),
                width: 64,
                height: 64,
                pixels: paper,
                scale: Some(1.0),
                depth: Some(0.32),
                invert: false,
                min_depth: Some(0.17),
                brightness: 30,
                contrast: 20,
                blend: Some("Hght".into()),
                each_tip: false,
            }),
            dynamics: AbrDynamics {
                tip: true,
                size_control: 3,
                size_jitter: 0.09,
                paint: true,
                flow_control: 2,
                flow_min: 0.2,
                flow_jitter: 0.1,
                tool_flow: Some(0.76),
                ..AbrDynamics::default()
            },
            ..round
        });
        assert_eq!(
            (shown(&kyle, "flow"), shown(&kyle, "pressure_opacity"), shown(&kyle, "pressure_flow")),
            (76.0, 61.0, 0.0)
        );
        assert_eq!((shown(&kyle, "pressure_size"), shown(&kyle, "tilt_size")), (50.0, 100.0));
        assert_eq!((shown(&kyle, "jitter_size"), shown(&kyle, "jitter_opacity")), (9.0, 10.0));
        assert_eq!(kyle.setting("grain_blend") as u32, procraft_engine::brush::GRAIN_HEIGHT);
        assert_eq!((shown(&kyle, "grain_brightness"), shown(&kyle, "grain_contrast")), (12.0, 20.0));
        assert_eq!((shown(&kyle, "grain_depth_min"), kyle.setting("grain_behavior")), (17.0, 1.0));
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
