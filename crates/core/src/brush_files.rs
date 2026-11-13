//! Brush, set and library files as other painting apps share them (`.brush`, `.brushset`,
//! `.brushlibrary`): read into the library (a library file as a library of its own), and written
//! from it. A brush's shape and grain images go with it; an image a file only
//! names (one that came with the app that made it) is stood in for by the nearest of ours.

use procraft_engine::brush::{
    Author, Brush, FULL_GRAIN_MOST, FullGrain, GrainFile, GrainMode, MaterialSource, ShapeFile, Source, StockImage,
    StockImages,
};
use procraft_engine::brushfile::{self, BrushFile, Pictures, Texture};
use procraft_engine::brushlibrary;
use procraft_engine::curve::Curve;
use procraft_engine::stroke::LEAST_ROUNDNESS;
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
    /// The image of a brush file's grain at its own size, and pixels a side, if it can be had.
    fn full_grain(&self, brush: &Brush) -> Option<(u32, std::sync::Arc<Vec<u8>>)> {
        let full = brush.grain_file_now()?.full.as_ref()?;
        Some((full.side, self.shared.lock().grain_store.pixels(full)?))
    }

    /// The image of a brush file's shape at its own size, as a PNG of that size, else the brush's own.
    fn shape_png(&self, brush: &Brush) -> Vec<u8> {
        let full = brush.shape_file_now().map(|f| &f.full);
        let pixels = full.and_then(|f| Some((f.side, self.shared.lock().grain_store.pixels(f)?)));
        match pixels {
            Some((side, pixels)) => sized_gray_png(side, side, &pixels),
            None => gray_png(&shape_pixels(brush)),
        }
    }

    /// `brush` as a file with the grain image [`Self::brush_pictures`] writes keeps it.
    fn as_written(&self, brush: Brush) -> Brush {
        let side = self.full_grain(&brush).map_or(TEXTURE_SIZE as u32, |(side, _)| side);
        brush.with_grain_image_width(side)
    }

    fn brush_pictures(&self, brush: &Brush, set: u32, index: u32) -> Made {
        // the preview it came with, else one drawn
        let thumbnail = brush.thumbnail.as_deref().map(<[u8]>::to_vec).unwrap_or_else(|| {
            self.brush_preview(set, index, THUMBNAIL.0, THUMBNAIL.1)
                .map(|t| crate::project::rgba_png(t.width, t.height, &t.rgba))
                .unwrap_or_else(|| crate::project::rgba_png(1, 1, &[0, 0, 0, 0]))
        });
        let author = brush.author.as_ref();
        Made {
            shape: self.shape_png(brush),
            grain: match self.full_grain(brush) {
                Some((side, pixels)) => Some(sized_gray_png(side, side, &pixels)),
                None => grain_pixels(brush).map(|p| gray_png(&p)),
            },
            metallic: material_pixels(&brush.metallic).map(|p| gray_png(&p)),
            roughness: material_pixels(&brush.roughness).map(|p| gray_png(&p)),
            thumbnail,
            author: author.and_then(|a| a.picture.clone()),
            signature: author.and_then(|a| a.signature.clone()),
            secondary: brush.secondary.as_ref().map(|second| {
                Box::new(Made {
                    shape: self.shape_png(second),
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
pub const OTHER_BRUSH_EXTENSIONS: [&str; 8] = ["abr", "gbr", "gih", "vbr", "kpp", "bundle", "sut", "sutg"];

/// Whether `bytes` are a zip holding brushes (a brush's archive, or a set's list), whatever the
/// file is called: a brush or set file sent on as a plain zip.
#[uniffi::export]
pub fn is_brush_zip(bytes: Vec<u8>) -> bool {
    procraft_engine::zip::ZipReader::new(&bytes).is_some_and(|zip| brush_names(zip.names()))
}

/// Whether a zip holding `names` holds brushes (a library's among them).
pub(crate) fn brush_names<'n>(names: impl Iterator<Item = &'n str>) -> bool {
    let names: Vec<&str> = names.collect();
    names.iter().any(|n| *n == "Brush.archive" || *n == "brushset.plist" || n.ends_with("/Brush.archive"))
        || brushlibrary::is_library(names.iter().copied())
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
    /// A library of their own called `name` (numbered if another has it): its sets (name, icon,
    /// brushes) and its tools' brushes (paint, erase, smudge).
    Library { name: String, sets: Vec<(String, u32, Vec<Brush>)>, tools: [(usize, usize); 3] },
}

/// A library file's sets in `bytes`, as a library called `name`; none if it isn't one.
fn read_library(bytes: &[u8], name: String) -> Option<ReadBrushes> {
    let file = brushlibrary::read(bytes)?;
    let sets = file
        .sets
        .iter()
        .map(|set| {
            // its icon as the stock set it names has it
            let icon = crate::brush_library::shipped_icon(set.icon.as_deref().unwrap_or(&set.name));
            let brushes = procraft_engine::par::side_by_side(&set.brushes, brush_from).into_iter().flatten().collect();
            (set.name.clone(), icon, brushes)
        })
        .collect();
    // the file's tools are paint, smudge and erase
    let [paint, smudge, erase] = file.tools;
    Some(ReadBrushes::Library { name, sets, tools: [paint, erase, smudge].map(|t| t.unwrap_or((0, 0))) })
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
    // a library, by its name or by what the zip holds
    let library = name.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(brushlibrary::EXTENSION))
        || procraft_engine::zip::ZipReader::new(bytes).is_some_and(|zip| brushlibrary::is_library(zip.names()));
    if library && let Some(read) = read_library(bytes, stem.clone()) {
        return Some(read);
    }
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
            ReadBrushes::Set { name, mut brushes } => {
                let mut st = self.shared.lock();
                brushes.iter_mut().for_each(|b| st.grain_store.take(b));
                let name = if name.trim().is_empty() { IMPORTED.to_owned() } else { name.trim().to_owned() };
                let origins = vec![None; brushes.len()];
                st.library.insert(0, LibrarySet { name, brushes, origins, icon: crate::brush_library::IMPORTED_ICON });
                st.move_brush_refs(|(rs, ri)| Some((rs + 1, ri)));
                Some(0)
            }
            ReadBrushes::One(mut brush) => {
                let mut st = self.shared.lock();
                st.grain_store.take(&mut brush);
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
            ReadBrushes::Library { name, sets, tools } => self.add_brush_library(name, sets, tools),
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
impl BrushesRead {
    /// Whether they are a library of their own: added, it is brought into use, which reads and
    /// writes its files (best off the interface's thread). False once added.
    pub fn is_library(&self) -> bool {
        let read = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        matches!(*read, Some(ReadBrushes::Library { .. }))
    }
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
        let brush = self.as_written(brush);
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
        let brushes: Vec<Brush> = brushes.into_iter().map(|b| self.as_written(b)).collect();
        let list: Vec<(&Brush, &str, Pictures<'_>)> =
            brushes.iter().zip(&made).map(|(b, m)| (b, author_name(b), m.pictures())).collect();
        let seed =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Some(brushfile::write_set(&name, &list, |i| folder_id(seed, i)))
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The library in use as a library file: its sets and their brushes in order, each tool's
    /// brush; none if it has no brushes.
    pub fn brush_library_as_file(&self) -> Option<Vec<u8>> {
        let (sets, tools) = {
            let st = self.shared.lock();
            let sets: Vec<(String, u32, Vec<Brush>)> = st
                .library
                .iter()
                .enumerate()
                .map(|(s, set)| {
                    let brushes = set.brushes.iter().enumerate().map(|(i, b)| st.with_sliders(b, s, i)).collect();
                    (set.name.clone(), set.icon, brushes)
                })
                .collect();
            let tools = [crate::ToolKind::Paint, crate::ToolKind::Smudge, crate::ToolKind::Erase]
                .map(|tool| Some(st.brush_ref(tool)));
            (sets, tools)
        };
        if sets.iter().all(|(_, _, brushes)| brushes.is_empty()) {
            return None;
        }
        let made: Vec<Vec<Made>> = sets
            .iter()
            .enumerate()
            .map(|(s, (_, _, brushes))| {
                brushes.iter().enumerate().map(|(i, b)| self.brush_pictures(b, s as u32, i as u32)).collect()
            })
            .collect();
        let out: Vec<brushlibrary::SetOut<'_>> = sets
            .iter()
            .zip(&made)
            .map(|((name, icon, brushes), made)| brushlibrary::SetOut {
                name,
                icon: crate::brush_library::shipped_icon_name(*icon),
                brushes: brushes.iter().zip(made).map(|(b, m)| (b, author_name(b), m.pictures())).collect(),
            })
            .collect();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let seed = now.as_nanos() as u64;
        Some(brushlibrary::write(&out, tools, |i| folder_id(seed, i).replace('-', "").to_lowercase(), now.as_secs()))
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
/// tip's pixels it covers, or (drawn up) between the four nearest, so a small tip made large is
/// smooth, as other apps draw it, not in blocks.
fn fitted(t: &procraft_engine::abr::AbrTip) -> Vec<u8> {
    fitted_at(t, TEXTURE_SIZE)
}

/// `t` fitted into a square `n` pixels a side, as [`fitted`] fits it.
fn fitted_at(t: &procraft_engine::abr::AbrTip, n: usize) -> Vec<u8> {
    let (w, h) = (t.width as usize, t.height as usize);
    let mut out = vec![0u8; n * n];
    if w == 0 || h == 0 {
        return out;
    }
    let scale = n as f32 / w.max(h) as f32;
    let (tw, th) =
        (((w as f32 * scale).round() as usize).clamp(1, n), ((h as f32 * scale).round() as usize).clamp(1, n));
    let (ox, oy) = ((n - tw) / 2, (n - th) / 2);
    if scale > 1.0 {
        // drawn up: each pixel between the tip's four nearest (its edge carried on past it)
        let get = |x: usize, y: usize| f32::from(t.pixels.get(y.min(h - 1) * w + x.min(w - 1)).copied().unwrap_or(0));
        let place = |i: usize| {
            let u = ((i as f32 + 0.5) / scale - 0.5).max(0.0);
            (u.floor() as usize, u.fract())
        };
        for y in 0..th {
            let (y0, fy) = place(y);
            for x in 0..tw {
                let (x0, fx) = place(x);
                let top = get(x0, y0) * (1.0 - fx) + get(x0 + 1, y0) * fx;
                let bottom = get(x0, y0 + 1) * (1.0 - fx) + get(x0 + 1, y0 + 1) * fx;
                if let Some(v) = out.get_mut((oy + y) * n + ox + x) {
                    *v = (top * (1.0 - fy) + bottom * fy).round() as u8;
                }
            }
        }
        return out;
    }
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
    // a preset alone has only the pictures it holds itself (newer ones do): one it only names (one
    // that came with Krita) comes round
    let preset = krita::read_preset(bytes, &|_| None)?;
    Some((name, vec![krita_brush(&preset)]))
}

/// A brush of a Clip Studio sub tool: its tip (an image, else round with its edge and roundness),
/// spacing, angle and how it turns, size, opacity and flow, and its texture.
fn csp_brush(c: &procraft_engine::csp::CspBrush) -> Brush {
    use std::f32::consts::{FRAC_PI_2, TAU};
    // the first of its pictures that lays any ink
    let tip = c.tip_pngs.iter().find_map(|png| csp_ink(png));
    // sprayed: a stamp as wide as the area the particles fall in (or a particle, if larger), the
    // particle in its middle, as many to a place as fall there, each thrown about the area
    let spray = c.spray.as_ref().zip(c.size).map(|(s, area)| (s, area, area.max(s.particle)));
    let tip = match spray {
        Some((s, _, stamp)) => Some(spray_tip(
            tip.as_ref(),
            c.hardness.unwrap_or(1.0),
            (c.roundness.unwrap_or(1.0), c.flattened_vertically),
            c.angle,
            s.particle / stamp,
        )),
        None => tip,
    };
    let picture = tip.as_ref().map(|t| (t.width as f32, t.height as f32));
    let mut b = match tip {
        Some(t) => tip_brush(&procraft_engine::abr::AbrTip { name: c.name.clone(), ..t }),
        None => Brush { name: c.name.clone(), ..Brush::round_pen() },
    };
    if let Some(h) = c.hardness {
        b.hardness = h.clamp(0.0, 1.0);
    }
    if let Some(r) = c.roundness {
        b.shape.roundness = r.clamp(LEAST_ROUNDNESS, 1.0);
    }
    if let Some(spacing) = c.spacing {
        b.spacing = Brush::spacing_within_reach(spacing);
    }
    // turning with the stroke (as a ribbon does), its angle is from the stroke's way, its 0
    // pointing the tip's top along it: a quarter turn back of ours, whose 0 lays its width along
    let follows = c.follows || c.ribbon;
    b.shape.follow = if follows { 1.0 } else { 0.0 };
    // a round tip flattened side to side is ours (flattened top to bottom) a quarter turned
    let side_to_side = picture.is_none() && !c.flattened_vertically && c.roundness.is_some_and(|r| r < 1.0);
    let turn = if side_to_side { FRAC_PI_2 } else { 0.0 } - if follows { FRAC_PI_2 } else { 0.0 };
    b.shape.rotation = (c.angle + turn).rem_euclid(TAU);
    // all of it at random is any way: half a turn either way
    b.shape.scatter = c.random_turn.clamp(0.0, 1.0);
    // a ribbon's pictures one after the other along the stroke: as far apart as each is long that
    // way (a stamp's size is its picture's longer side)
    if let (true, Some((w, h))) = (c.ribbon, picture) {
        let (cos, sin) = (b.shape.rotation.cos().abs(), b.shape.rotation.sin().abs());
        b.spacing = Brush::spacing_within_reach((cos * w + sin * h) / w.max(h).max(1.0));
    }
    if let Some(opacity) = c.opacity {
        b.max_opacity = opacity.clamp(0.0, 1.0);
        b.min_opacity = b.min_opacity.min(b.max_opacity);
    }
    if let Some(flow) = c.flow {
        b.set_setting("flow", flow);
    }
    // its most size from its pixels (as a sampled tip's is), the least 1%
    if let Some(px) = c.size {
        brushfile::set_sizes(&mut b, px * ABR_SIZE_PER_PIXEL, Some(0.01));
    }
    if let Some((s, area, stamp)) = spray {
        csp_spray(&mut b, s, area, stamp, c.spacing);
    }
    csp_pen(&mut b, &c.pen);
    if let Some(t) = &c.taper {
        csp_taper(&mut b, t);
    }
    if let Some(t) = &c.texture {
        csp_paper(&mut b, t);
    }
    // the texture's density by pressure, now that it has its grain
    if let (Some(d), true) = (&c.pen.texture_density, b.grain.source.is_some()) {
        use procraft_engine::brush::PressureCurve;
        // the depth scaled by the curve, from what it leaves at none up to all of it
        let curve = d.pressure.as_ref().map(|p| csp_curve(&p.points, p.least));
        b.pressure_curves.set(PressureCurve::GrainDepth, curve);
        b.set_setting("grain_depth_jitter", d.random.map_or(0.0, |least| 1.0 - least));
    }
    b
}

/// Gives `b` a Clip Studio spray: its stamp `stamp` pixels across holding one particle in its middle
/// (see [`spray_tip`]), as many to a place as the density says (up to our 16), each thrown at
/// random as far as the `area` the particles fall in reaches (nearer the middle the more they
/// gather), turned as the particles are. The sub tool's spacing goes by the area; none (the app's
/// own), ours.
fn csp_spray(b: &mut Brush, s: &procraft_engine::csp::CspSpray, area: f32, stamp: f32, spacing: Option<f32>) {
    use std::f32::consts::{FRAC_PI_2, TAU};
    // the particle carries the tip's roundness: the area is round
    b.shape.roundness = 1.0;
    b.shape.follow = if s.follows { 1.0 } else { 0.0 };
    b.shape.rotation = (s.angle - if s.follows { FRAC_PI_2 } else { 0.0 }).rem_euclid(TAU);
    b.shape.scatter = s.random_turn.clamp(0.0, 1.0);
    b.shape.count = s.density.clamp(1, 16);
    // fewer with less pressure (down to the share it leaves), and at random
    if let Some(d) = &s.density_by {
        use procraft_engine::brush::PressureCurve;
        b.pressure_curves.set(PressureCurve::Count, d.pressure.as_ref().map(|p| csp_curve(&p.points, p.least)));
        b.set_setting("count_jitter", d.random.map_or(0.0, |least| 1.0 - least.clamp(0.0, 1.0)));
    }
    // (sprayed, a ribbon's spacing has nothing to go by: left to us, as a tip is)
    let spacing = spacing.map_or(Brush::round_pen().spacing, |s| s * area / stamp.max(1e-3));
    b.spacing = Brush::spacing_within_reach(spacing);
    // (the spacing first: it sets how big it may be)
    brushfile::set_sizes(b, stamp * ABR_SIZE_PER_PIXEL, Some(0.01));
    let gathered = 1.0 - 0.5 * s.bias.max(0.0);
    let control = jitter_control(b, 0.5 * area / stamp.max(1e-3) * gathered);
    b.set_setting("jitter_lateral", control);
    b.set_setting("jitter_linear", control);
}

/// A sprayed tip's picture: one particle of `share` of its width in its middle, the tip's picture
/// (or, round, a disc of the tip's hardness) made that small, flattened by its roundness (top to
/// bottom if `flattened.1`, else side to side) and turned by its `angle`. Each pixel is the mean
/// of 16 places in it, so a particle a few pixels across keeps its ink.
fn spray_tip(
    particle: Option<&procraft_engine::abr::AbrTip>,
    hardness: f32,
    flattened: (f32, bool),
    angle: f32,
    share: f32,
) -> procraft_engine::abr::AbrTip {
    const PLACES: usize = 4;
    let n = TEXTURE_SIZE;
    let side = (n as f32 * share.clamp(0.0, 1.0)).max(1.0);
    let middle = n as f32 / 2.0;
    let roundness = flattened.0.clamp(0.01, 1.0);
    let (wide, tall) = if flattened.1 { (1.0, roundness) } else { (roundness, 1.0) };
    let fitted = particle.map(fitted);
    // the particle at (x, y), each -0.5 to 0.5 across it: the picture between its pixels, or the disc
    let soft = (0.5 * (1.0 - hardness.clamp(0.0, 1.0))).max(0.75 / side);
    let at = |x: f32, y: f32| -> f32 {
        if x.abs() > 0.5 || y.abs() > 0.5 {
            return 0.0;
        }
        match &fitted {
            Some(pixels) => {
                let (u, v) = ((x + 0.5) * n as f32 - 0.5, (y + 0.5) * n as f32 - 0.5);
                let (x0, y0) = (u.floor(), v.floor());
                let (fx, fy) = (u - x0, v - y0);
                let get = |i: f32, j: f32| {
                    if i < 0.0 || j < 0.0 || i >= n as f32 || j >= n as f32 {
                        0.0
                    } else {
                        f32::from(pixels.get(j as usize * n + i as usize).copied().unwrap_or(0)) / 255.0
                    }
                };
                let top = get(x0, y0) * (1.0 - fx) + get(x0 + 1.0, y0) * fx;
                let bottom = get(x0, y0 + 1.0) * (1.0 - fx) + get(x0 + 1.0, y0 + 1.0) * fx;
                top * (1.0 - fy) + bottom * fy
            }
            None => ((0.5 - x.hypot(y)) / soft + 0.5).clamp(0.0, 1.0),
        }
    };
    let (cos, sin) = (angle.cos(), angle.sin());
    let pixels = (0..n * n)
        .map(|i| {
            let (px, py) = ((i % n) as f32 - middle, (i / n) as f32 - middle);
            // nothing past the particle's reach
            if px.hypot(py) > side * 0.75 + 2.0 {
                return 0;
            }
            let mut sum = 0.0;
            for k in 0..PLACES * PLACES {
                let (dx, dy) =
                    (((k % PLACES) as f32 + 0.5) / PLACES as f32, ((k / PLACES) as f32 + 0.5) / PLACES as f32);
                let (x, y) = (px + dx, py + dy);
                // back through the turn, then the flattening, into the particle's own square
                let (lx, ly) = (x * cos + y * sin, -x * sin + y * cos);
                sum += at(lx / (side * wide), ly / (side * tall));
            }
            (sum / (PLACES * PLACES) as f32 * 255.0).round() as u8
        })
        .collect();
    procraft_engine::abr::AbrTip { name: String::new(), spacing: None, width: n as u32, height: n as u32, pixels }
}

/// Our jitter's control that throws a stamp of `b` at most `amount` of its diameters away: our
/// jitter throws a stamp up to its control to the power 2.2 times the scale, of half its diagonal
/// (for a round stamp a little over 0.7 of its diameter).
fn jitter_control(b: &Brush, amount: f32) -> f32 {
    use procraft_engine::stroke::JITTER_FILE_SCALE;
    let reach = 0.5 * (1.0 + b.shape.roundness * b.shape.roundness).sqrt();
    (amount / reach / JITTER_FILE_SCALE).powf(1.0 / 2.2).clamp(0.0, 1.0)
}

/// A Clip Studio source's curve as ours: `least` and up to all of it, as the curve goes. Its points
/// are joined without swinging past them ([`csp_curve_at`]); ours bends as a natural spline, which
/// through a few points spaced unevenly can swing far past them (a flow curve rising steeply at
/// its very end fell below nothing in its middle), so where it would stray from the file's it is
/// given the file's at [`CSP_CURVE_PLACES`] places as well as at its own points.
fn csp_curve(points: &[(f32, f32)], least: f32) -> Curve {
    use procraft_engine::geom::Vec2;
    let least = least.clamp(0.0, 1.0);
    let scaled = |y: f32| least + (1.0 - least) * y;
    let own = Curve::new(&points.iter().map(|&(x, y)| Vec2::new(x, scaled(y))).collect::<Vec<_>>());
    let across = |n: usize| (0..n).map(move |i| i as f32 / (n - 1) as f32);
    let strays = across(65).any(|x| (own.eval(x) - scaled(csp_curve_at(points, x))).abs() > CSP_CURVE_STRAY);
    if !strays {
        return own;
    }
    let places = across(CSP_CURVE_PLACES).chain(points.iter().map(|p| p.0));
    Curve::new(&places.map(|x| Vec2::new(x, scaled(csp_curve_at(points, x)))).collect::<Vec<_>>())
}

/// How far ours may stray from a Clip Studio curve before it is given the file's at more places.
const CSP_CURVE_STRAY: f32 = 0.02;
/// The places a Clip Studio curve is given at where ours would stray from it, evenly across.
const CSP_CURVE_PLACES: usize = 17;

/// A Clip Studio curve at `x`: a monotone cubic through its points (rising or falling only as
/// they do, and level where they turn), held level beyond the first and the last.
fn csp_curve_at(points: &[(f32, f32)], x: f32) -> f32 {
    let (Some(&(x0, y0)), Some(&(xn, yn))) = (points.first(), points.last()) else { return x };
    if x <= x0 {
        return y0;
    }
    if x >= xn {
        return yn;
    }
    // each span's width and slope, then the slope at each point: the spans' own at the ends, none
    // where they turn, else their mean weighted by width (harmonic, so it never overshoots)
    let spans: Vec<(f32, f32)> =
        points.windows(2).filter_map(|w| w.first().zip(w.get(1))).map(|(a, b)| (b.0 - a.0, b.1 - a.1)).collect();
    let slope = |(w, rise): (f32, f32)| rise / w.max(1e-6);
    let at_point = |k: usize| -> f32 {
        let before = k.checked_sub(1).and_then(|j| spans.get(j)).copied();
        match (before, spans.get(k).copied()) {
            (Some(a), Some(b)) => {
                let (sa, sb) = (slope(a), slope(b));
                if sa * sb <= 0.0 {
                    return 0.0;
                }
                let (wa, wb) = (2.0 * b.0 + a.0, b.0 + 2.0 * a.0);
                (wa + wb) / (wa / sa + wb / sb)
            }
            (Some(s), None) | (None, Some(s)) => slope(s),
            (None, None) => 0.0,
        }
    };
    let Some(k) = points.windows(2).position(|w| w.get(1).is_some_and(|b| x <= b.0)) else { return yn };
    let (Some(&(xa, ya)), Some(&(xb, yb))) = (points.get(k), points.get(k + 1)) else { return yn };
    let h = (xb - xa).max(1e-6);
    let t = (x - xa) / h;
    let (t2, t3) = (t * t, t * t * t);
    let y = (2.0 * t3 - 3.0 * t2 + 1.0) * ya
        + (t3 - 2.0 * t2 + t) * h * at_point(k)
        + (-2.0 * t3 + 3.0 * t2) * yb
        + (t3 - t2) * h * at_point(k + 1);
    // held to the span's own rise, whatever the slopes
    y.clamp(ya.min(yb), ya.max(yb))
}

/// Gives `b` what of a Clip Studio sub tool follows the pen. Pressure takes a setting down to what
/// it leaves at none, along its curve (ours, for the size, opacity, flow and roundness); a setting
/// the file has but no pressure drives doesn't follow it. Chance takes it down as far, as jitter.
/// Speed is left out: how fast the file's own fastest is isn't known, and taken as ours a pen
/// fades to its least on any quick stroke. Of tilt, only fading the opacity and flattening the tip
/// as the pen leans are ours (as far as tilt takes them, its curve left out); tilt growing or
/// shrinking the size is not. What scales our opacity before it is eased (by the power
/// [`procraft_engine::brush::FLOW_POWER`]) is unwound by that power, so a stamp lays what the file
/// says.
fn csp_pen(b: &mut Brush, pen: &procraft_engine::csp::CspPen) {
    use procraft_engine::brush::{FLOW_POWER, PressureCurve};
    let taken = |least: Option<f32>| least.map_or(0.0, |l| 1.0 - l.clamp(0.0, 1.0));
    let unwound = |least: Option<f32>| least.map(|l| l.clamp(0.0, 1.0).powf(1.0 / FLOW_POWER));
    // how far pressure takes a setting down, and its curve
    fn pressed(d: &procraft_engine::csp::CspDynamics) -> Option<(f32, &procraft_engine::csp::CspCurve)> {
        d.pressure.as_ref().map(|p| (1.0 - p.least.clamp(0.0, 1.0), p))
    }
    if let Some(d) = &pen.size {
        match pressed(d) {
            Some((amount, p)) => {
                b.pressure_size = amount;
                b.size_curve = csp_curve(&p.points, 0.0);
            }
            None => b.pressure_size = 0.0,
        }
        b.set_setting("jitter_size", taken(d.random));
    }
    // the opacity and the flow both lay how much ink: chance by the more of the two
    let mut chance = 0.0f32;
    if let Some(d) = &pen.opacity {
        match &d.pressure {
            // all of the opacity follows the curve, from what it leaves at none up, unwound
            Some(p) => {
                b.pressure_opacity = if p.least < 1.0 { 1.0 } else { 0.0 };
                b.opacity_curve = eased_root(&csp_curve(&p.points, p.least));
            }
            None => b.pressure_opacity = 0.0,
        }
        b.set_setting("tilt_opacity", taken(unwound(d.tilt.as_ref().map(|t| t.least))));
        chance = chance.max(taken(d.random));
    }
    if let Some(d) = &pen.flow {
        let (amount, curve) = pressed(d).map_or((0.0, None), |(amount, p)| (amount, Some(csp_curve(&p.points, 0.0))));
        b.set_setting("pressure_flow", amount);
        b.pressure_curves.set(PressureCurve::Flow, curve);
        chance = chance.max(taken(d.random));
    }
    if pen.opacity.is_some() || pen.flow.is_some() {
        b.set_setting("jitter_opacity", chance);
    }
    if let Some(d) = &pen.thickness {
        let (amount, curve) = pressed(d).map_or((0.0, None), |(amount, p)| (amount, Some(csp_curve(&p.points, 0.0))));
        b.set_setting("pressure_roundness", amount);
        b.pressure_curves.set(PressureCurve::Roundness, curve);
        b.set_setting("tilt_roundness", taken(d.tilt.as_ref().map(|t| t.least)));
        b.set_setting("roundness_jitter_y", taken(d.random));
    }
}

/// Gives `b` a Clip Studio sub tool's start and end, for a pen and a finger alike: as long as its
/// own (as classic tapers, over canvas pixels; one kept as a share of the stroke taken of a stroke
/// as long as a full taper), thinning and fading as far as it says.
fn csp_taper(b: &mut Brush, t: &procraft_engine::csp::CspTaper) {
    let span = procraft_engine::brush::CLASSIC_TAPER_SPAN;
    let length = |l: Option<f32>| l.map_or(0.0, |l| if t.of_stroke { l } else { l / span });
    b.set_setting("classic_taper", 1.0);
    for tool in ["pen", "touch"] {
        for (part, value) in
            [("start", length(t.start)), ("end", length(t.end)), ("size", t.size), ("opacity", t.opacity)]
        {
            b.set_setting(&format!("{tool}_taper_{part}"), value);
        }
    }
}

/// A Clip Studio texture's picture of no more than this many pixels a side is a pattern of pixels
/// (a dither's), its grain read sharp.
const CSP_PIXEL_PATTERN: u32 = 64;

/// Gives `b` a Clip Studio texture as its grain, fixed to the canvas as paper is: the ink its
/// picture lays (which the grain lets through; reversed, where it lays none), tiled at the size of
/// the picture itself times its scale, as strongly as its density says (its differences made
/// stronger: from half of it on, fully), combined with the stroke by the nearest of its modes.
fn csp_paper(b: &mut Brush, t: &procraft_engine::csp::CspTexture) {
    use procraft_engine::csp::TextureMode as M;
    use procraft_engine::document::BlendMode;
    let Some(ink) = t.pngs.iter().find_map(|png| csp_ink(png)) else { return };
    // its thumbnail may be smaller than the picture
    let own = t.width.filter(|w| *w > 0).map_or(1.0, |w| w as f32 / ink.width.max(1) as f32);
    let depth = t.density.unwrap_or(1.0) * if t.emphasized { 2.0 } else { 1.0 };
    let png = sized_gray_png(ink.width, ink.height, &ink.pixels);
    paper(b, &[png], t.reverse, depth.min(1.0), t.scale.unwrap_or(1.0) * own);
    if b.grain.source.is_none() {
        return;
    }
    let mode = match t.mode {
        M::Subtract => BlendMode::Subtract.id(),
        M::Compare => BlendMode::Darken.id(),
        M::Overlay => BlendMode::Overlay.id(),
        M::ColorDodge => BlendMode::ColorDodge.id(),
        M::HardMix => BlendMode::HardMix.id(),
        M::Height => procraft_engine::brush::GRAIN_HEIGHT,
        M::Normal | M::Multiply | M::Other => BlendMode::Multiply.id(),
    };
    // a lighter picture lays less ink, which the grain is (reversed, more)
    let lighter = if t.reverse { 1.0 } else { -1.0 };
    for (key, value) in [
        ("grain_behavior", 1.0),
        ("grain_blend", mode as f32),
        ("grain_brightness", lighter * t.brightness),
        ("grain_contrast", t.contrast),
        // a share of a half turn either way
        ("grain_turn", (t.rotation / std::f32::consts::PI + 1.0).rem_euclid(2.0) - 1.0),
    ] {
        b.set_setting(key, value);
    }
    if ink.width.max(ink.height) <= CSP_PIXEL_PATTERN {
        b.set_setting("grain_filtering", 0.0);
    }
}

/// A Clip Studio material's picture (PNG) as the ink it lays, a tip of it: its dark parts as far
/// as they are opaque (a material is dark on clear, or a picture on white); a picture with next to
/// no dark in it (light on clear) by how opaque it is. None if it lays none, or isn't a picture.
fn csp_ink(png: &[u8]) -> Option<procraft_engine::abr::AbrTip> {
    let (w, h, channels, buf) = decode_picture(png)?;
    let (dark, cover): (Vec<u8>, Vec<u8>) = buf
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
            (((255 - grey.min(255)) * alpha / 255) as u8, alpha.min(255) as u8)
        })
        .unzip();
    let sum = |v: &[u8]| v.iter().map(|&x| u64::from(x)).sum::<u64>();
    // the dark under one fiftieth of what the coverage lays: light on clear
    let pixels = if sum(&dark) * 50 < sum(&cover) { cover } else { dark };
    (pixels.len() == w * h && pixels.iter().any(|&v| v > 0)).then(|| procraft_engine::abr::AbrTip {
        name: String::new(),
        spacing: None,
        width: w as u32,
        height: h as u32,
        pixels,
    })
}

/// Gives `b` another app's texture as its grain, paper the stroke paints through: the first of
/// `pngs` that lets paint through anywhere (one may be blank), dark letting it through if
/// `inverted`, showing `depth` strongly, tiled at the picture's own size times `scale`. Nothing if
/// none of them will do.
fn paper(b: &mut Brush, pngs: &[Vec<u8>], inverted: bool, depth: f32, scale: f32) {
    let found = pngs.iter().find_map(|png| {
        let source = gray_texture(png, inverted).filter(|s| s.pixels.iter().any(|&v| v > 0))?;
        Some((source, decode_picture(png)?.0))
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
    // no thinner than the app's least (its stamps are drawn no thinner), so that it is the
    // same read back from our own file
    if let Some(r) = a.roundness {
        b.shape.roundness = r.clamp(LEAST_ROUNDNESS, 1.0);
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
            // ours paints half halfway between its hardness and its edge: where GIMP's does
            b.hardness = (2.0 * procraft_engine::gbr::generated_half(g.hardness) - 1.0).clamp(0.0, 1.0);
            b.shape.roundness = (1.0 / g.aspect).clamp(LEAST_ROUNDNESS, 1.0);
            b
        }
    };
    b.spacing = Brush::spacing_within_reach(g.spacing);
    b.shape.rotation = g.angle;
    b
}

/// A tip larger than a `Source` holds, fitted into a square as [`fitted`] fits it, as large as the
/// tip goes (a power of two no larger than its longer side, up to [`FULL_GRAIN_MOST`]): drawn large,
/// a stamp then shows the tip's own detail rather than the small square's blown up. None for a
/// tip no larger than a `Source`.
fn full_tip(t: &procraft_engine::abr::AbrTip) -> Option<FullGrain> {
    let longest = t.width.max(t.height) as usize;
    let side = (1usize << longest.max(1).ilog2()).min(FULL_GRAIN_MOST);
    (side > TEXTURE_SIZE).then(|| FullGrain::new(side as u32, fitted_at(t, side))).flatten()
}

/// `b`'s shape as the sampled tip `t`: the small square, and the tip at its own size beside it.
fn give_tip(b: &mut Brush, t: &procraft_engine::abr::AbrTip) {
    b.shape.image = None;
    b.shape.source = Source::new(fitted(t));
    b.shape_file = b.shape.source.as_ref().zip(full_tip(t)).map(|(source, full)| ShapeFile { source: source.id, full });
}

/// A brush of a sampled tip: its shape, with its spacing.
fn tip_brush(t: &procraft_engine::abr::AbrTip) -> Brush {
    let mut b = Brush { name: t.name.clone(), ..Brush::round_pen() };
    give_tip(&mut b, t);
    if let Some(spacing) = t.spacing {
        b.spacing = Brush::spacing_within_reach(spacing);
    }
    b
}

/// A brush of a Krita preset: its tip (round, or an image), spacing, angle, and what pressure
/// changes.
fn krita_brush(k: &procraft_engine::krita::KritaBrush) -> Brush {
    use procraft_engine::krita::KritaTip;
    let tip = match &k.tip {
        KritaTip::Round { .. } => None,
        KritaTip::Image(t) => Some(t.clone()),
        KritaTip::Png(png) => png_tip(png, false),
    };
    let mut b = Brush { name: k.name.clone(), ..Brush::round_pen() };
    if let Some(t) = &tip {
        give_tip(&mut b, t);
    }
    if let KritaTip::Round { hardness, roundness } = k.tip {
        b.hardness = hardness.clamp(0.0, 1.0);
        b.shape.roundness = roundness.clamp(LEAST_ROUNDNESS, 1.0);
    }
    if let Some(spacing) = k.spacing {
        b.spacing = Brush::spacing_within_reach(spacing);
    }
    b.shape.rotation = k.angle;
    if k.follows {
        b.shape.follow = 1.0;
    }
    if k.turns_at_random {
        // a half turn either way: any angle
        b.shape.scatter = 1.0;
    }
    b.shape.scatter = b.shape.scatter.max(k.rotation_random);
    if k.rotation_per_stroke {
        b.shape.randomise = true;
    }
    if k.follows_tilt {
        b.shape.follow_azimuth = true;
    }
    for (key, on) in [("flip_x", k.mirror.0), ("flip_y", k.mirror.1)] {
        if on {
            b.set_setting(key, 1.0);
        }
    }
    if let Some(s) = &k.scatter {
        krita_scatter(&mut b, s);
    }
    // its colour changed at random each stamp, or each stroke
    for (keys, amounts) in [
        (["stamp_hue", "stamp_saturation", "stamp_lightness", "stamp_darkness"], k.color_random),
        (["stroke_hue", "stroke_saturation", "stroke_lightness", "stroke_darkness"], k.color_per_stroke),
    ] {
        for (key, amount) in keys.into_iter().zip(amounts) {
            if amount > 0.0 {
                b.set_setting(key, amount);
            }
        }
    }
    // flattened at random, or by the pressure
    if k.ratio_random > 0.0 {
        b.set_setting("roundness_jitter_y", k.ratio_random);
    }
    // the secondary colour (Krita's background) where pressure leaves its colour out
    if let Some(points) = &k.mix_pressure {
        let left: Vec<procraft_engine::geom::Vec2> =
            points.iter().map(|p| procraft_engine::geom::Vec2::new(p.x, 1.0 - p.y.clamp(0.0, 1.0))).collect();
        b.set_setting("pressure_secondary", 1.0);
        b.pressure_curves.set(procraft_engine::brush::PressureCurve::Secondary, Some(Curve::new(&left)));
    }
    if let Some(points) = &k.ratio_pressure {
        b.set_setting("pressure_roundness", 1.0);
        b.pressure_curves.set(procraft_engine::brush::PressureCurve::Roundness, Some(Curve::new(points)));
    }
    // the size from nothing up to all of it along its pressure curve, if pressure drives it
    match &k.size_pressure {
        Some(points) => {
            b.pressure_size = 1.0;
            b.size_curve = Curve::new(points);
        }
        None => b.pressure_size = 0.0,
    }
    // Krita multiplies a texture into each stamp at its strength (unless its texturing is soft):
    // that strength, as far as the pressure takes it, is less paint laid, the texture as strong
    let (flow, flow_pressure) = match &k.texture {
        Some(t) if t.mode == 0 && !t.soft => {
            (k.flow * t.strength, multiplied(k.flow_pressure.as_deref(), t.strength_pressure.as_deref()))
        }
        _ => (k.flow, k.flow_pressure.clone()),
    };
    krita_paint(&mut b, k.opacity, flow, k.opacity_pressure.as_deref(), flow_pressure.as_deref(), k.wash);
    if let Some(t) = &k.texture {
        krita_paper(&mut b, t, &k.name);
    }
    // a masking brush is a dual brush's second: its size a share of this one's, its coverage
    // combined with this one's by the nearest of the modes
    if let Some(m) = &k.masking {
        let mut second = krita_brush(&m.brush);
        second.min_size = b.min_size * m.size;
        second.max_size = (b.max_size * m.size).min(second.most_size() * second.size_unit());
        masking_paint(&mut second, m);
        b.secondary = Some(Box::new(second));
        let mode = masking_mode(&m.op).unwrap_or_else(|| {
            log::warn!("Krita preset \"{}\": masking op {} combines as multiply", k.name, m.op);
            procraft_engine::document::BlendMode::Multiply.id()
        });
        b.set_setting("dual_blend", mode as f32);
    }
    if !k.missing.is_empty() {
        log::warn!("Krita preset \"{}\": {} not in its file, or not readable", k.name, k.missing.join(", "));
    }
    b
}

/// Gives `b` a Krita preset's texture as its grain: the mask Krita makes of its picture (light
/// keeping the paint where it multiplies), as deep as its strength, combined with each stamp by
/// the nearest of the modes to Krita's. Multiplied its own way (not soft), the grain is whole and
/// the strength is in the paint laid ([`krita_brush`]); subtracted its own way with a strength
/// that follows the pressure, each stamp less the mask and less what its strength falls short of,
/// as our height mode takes the mask from a stamp at a twelfth of the depth there; soft, the
/// strength is how deep the mask goes. A strength that follows the pressure goes stamp by stamp.
fn krita_paper(b: &mut Brush, t: &procraft_engine::krita::KritaTexture, name: &str) {
    use procraft_engine::brush::{GRAIN_HEIGHT, PressureCurve};
    use procraft_engine::krita::KritaPicture;
    let mask = |grey: f32, alpha: f32| (t.mask_value(grey / 255.0, alpha / 255.0) * 255.0).round() as u8;
    let picture = match &t.picture {
        KritaPicture::Encoded(png) => decode_picture(png).map(|(w, h, channels, buf)| {
            let pixels = buf
                .chunks_exact(channels.max(1))
                .take(w * h)
                .map(|px| {
                    let get = |k: usize| f32::from(px.get(k).copied().unwrap_or(0));
                    // Krita's grey: its own weights of red, green and blue, in 32nds
                    let grey = || ((11.0 * get(0) + 16.0 * get(1) + 5.0 * get(2)) / 32.0).floor();
                    match channels {
                        1 => mask(get(0), 255.0),
                        2 => mask(get(0), get(1)),
                        3 => mask(grey(), 255.0),
                        _ => mask(grey(), get(3)),
                    }
                })
                .collect::<Vec<u8>>();
            (w as u32, h as u32, pixels)
        }),
        KritaPicture::Grey(g) => {
            Some((g.width, g.height, g.pixels.iter().map(|&v| mask(f32::from(v), 255.0)).collect()))
        }
    };
    let Some((w, h, pixels)) = picture else { return };
    paper(b, &[sized_gray_png(w, h, &pixels)], false, t.strength, t.scale);
    if b.grain.source.is_none() {
        return;
    }
    let mode = t.op().and_then(masking_mode).unwrap_or_else(|| {
        log::warn!("Krita preset \"{name}\": texturing mode {} combines as multiply", t.mode);
        procraft_engine::document::BlendMode::Multiply.id()
    });
    b.set_setting("grain_blend", mode as f32);
    let pressed = t.strength_pressure.as_deref().map(Curve::new);
    let by_stamp = match (t.mode, t.soft) {
        (0, false) => {
            b.grain.depth = 1.0;
            false
        }
        (1, false) if pressed.is_some() => {
            b.grain.depth = t.strength / 12.0;
            b.set_setting("grain_blend", GRAIN_HEIGHT as f32);
            true
        }
        (0 | 1, true) => pressed.is_some(),
        _ => false,
    };
    if by_stamp {
        b.set_setting("grain_each_stamp", 1.0);
        b.pressure_curves.set(PressureCurve::GrainDepth, pressed);
    }
}

/// Two pressure curves (points 0..1 from the left) multiplied across, at 17 places; either alone
/// as it is; none for neither.
fn multiplied(
    a: Option<&[procraft_engine::geom::Vec2]>,
    b: Option<&[procraft_engine::geom::Vec2]>,
) -> Option<Vec<procraft_engine::geom::Vec2>> {
    use procraft_engine::geom::Vec2;
    match (a, b) {
        (None, None) => None,
        (Some(one), None) | (None, Some(one)) => Some(one.to_vec()),
        (Some(a), Some(b)) => {
            let (a, b) = (Curve::new(a), Curve::new(b));
            Some((0..=16).map(|i| i as f32 / 16.0).map(|x| Vec2::new(x, a.eval(x) * b.eval(x))).collect())
        }
    }
}

/// The curve that, eased as a stamp's opacity is (to the power
/// [`procraft_engine::brush::FLOW_POWER`]), lays what `curve` says: its root, taken across it (at 17
/// places and at its own points, so a straight one bends as it should).
fn eased_root(curve: &Curve) -> Curve {
    use procraft_engine::brush::FLOW_POWER;
    use procraft_engine::geom::Vec2;
    let places = (0..=16).map(|i| i as f32 / 16.0).chain(curve.points().iter().map(|p| p.x));
    Curve::new(&places.map(|x| Vec2::new(x, curve.eval(x).max(0.0).powf(1.0 / FLOW_POWER))).collect::<Vec<_>>())
}

/// Gives `b` a Krita preset's scatter as jitter along the stroke and across it: as far at most
/// (its amount of diameters, up to as far as our jitter reaches) and as the pressure lets it.
fn krita_scatter(b: &mut Brush, s: &procraft_engine::krita::KritaScatter) {
    use procraft_engine::brush::PressureCurve;
    let control = jitter_control(b, s.amount);
    let curve = s.pressure.as_deref().map(Curve::new);
    for (on, key, by) in [
        (s.along, "jitter_linear", PressureCurve::LinearJitter),
        (s.across, "jitter_lateral", PressureCurve::LateralJitter),
    ] {
        if on {
            b.set_setting(key, control);
            b.pressure_curves.set(by, curve.clone());
        }
    }
}

/// Gives a masking brush's second how much it lays, as Krita's wash mode (the only one it masks
/// in) has it (see [`krita_paint`]).
fn masking_paint(second: &mut Brush, m: &procraft_engine::krita::KritaMasking) {
    krita_paint(second, m.opacity, m.flow, m.opacity_pressure.as_deref(), m.flow_pressure.as_deref(), true);
}

/// Gives `b` how much a Krita brush lays, its `opacity` and `flow` each scaled by its pressure
/// curve, if it has one. In wash mode each stamp lays the flow, towards the most the stroke may
/// reach, which is the opacity; built up, each stamp lays the opacity times the flow.
fn krita_paint(
    b: &mut Brush,
    opacity: f32,
    flow: f32,
    opacity_pressure: Option<&[procraft_engine::geom::Vec2]>,
    flow_pressure: Option<&[procraft_engine::geom::Vec2]>,
    wash: bool,
) {
    use procraft_engine::brush::PressureCurve;
    use procraft_engine::geom::Vec2;
    b.max_opacity = 1.0;
    let rooted = |points: &[Vec2]| eased_root(&Curve::new(points));
    b.pressure_opacity = 0.0;
    if !wash {
        b.flow = opacity * flow;
        // both curves at once, multiplied across
        if let Some(points) = multiplied(opacity_pressure, flow_pressure) {
            b.pressure_opacity = 1.0;
            b.opacity_curve = rooted(&points);
        }
        return;
    }
    b.flow = flow;
    if let Some(points) = flow_pressure {
        b.pressure_opacity = 1.0;
        b.opacity_curve = rooted(points);
    }
    // the stroke builds up towards what its transfer lets through: the opacity, by the pressure
    let most = match opacity_pressure {
        Some(points) => points.iter().map(|p| Vec2::new(p.x, p.y * opacity)).collect(),
        None => vec![Vec2::new(0.0, opacity), Vec2::new(1.0, opacity)],
    };
    if opacity_pressure.is_some() || opacity < 1.0 {
        b.set_setting("pressure_flow", 1.0);
        b.pressure_curves.set(PressureCurve::Flow, Some(Curve::new(&most)));
    }
    b.set_setting("rendering_mode", 1.0);
}

/// The dual mode nearest a Krita masking brush's composite op (by Krita's name for it), if there
/// is one. Each takes the main brush's coverage as the backdrop and the mask's as the source, as
/// the dual modes do; Add and Vivid Light aren't ones the studio lists, but they draw.
fn masking_mode(op: &str) -> Option<u32> {
    use procraft_engine::document::BlendMode as B;
    Some(match op {
        "normal" | "over" => B::Normal.id(),
        "multiply" => B::Multiply.id(),
        "darken" => B::Darken.id(),
        "burn" => B::ColorBurn.id(),
        "linear_burn" => B::LinearBurn.id(),
        "lighten" => B::Lighten.id(),
        "screen" => B::Screen.id(),
        "dodge" => B::ColorDodge.id(),
        "linear_dodge" => B::Add.id(),
        "overlay" => B::Overlay.id(),
        "hard_mix_photoshop" => B::HardMix.id(),
        // the softer one is the inverse subtract (Linear Burn's rule) with its two terms scaled up;
        // Krita's own hard mix burns under a half and dodges over it, as Vivid Light does
        "inverse_subtract" | "hard_mix_softer_photoshop" => B::LinearBurn.id(),
        "hard_mix" => B::VividLight.id(),
        "difference" => B::Difference.id(),
        "subtract" => B::Subtract.id(),
        "divide" => B::Divide.id(),
        "height_photoshop" => procraft_engine::brush::GRAIN_HEIGHT,
        "linear_height_photoshop" => procraft_engine::brush::GRAIN_LINEAR_HEIGHT,
        _ => return None,
    })
}

/// A PNG tip as a grey one: its opaque pixels paint, the light ones if `white`, else the dark.
fn png_tip(png: &[u8], white: bool) -> Option<procraft_engine::abr::AbrTip> {
    let (w, h, channels, buf) = decode_picture(png)?;
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
    b.shape_file = shape_file_for(&file.shape, source.as_ref());
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
    // (a second's grain goes on stamp by stamp, so by the grain's own tile)
    b.secondary = file.secondary.as_deref().map(|second| Box::new(Brush { grain_file: None, ..brush_from(second) }));
    let (image, source) = grain_for(&file.grain, file.thumbnail.as_deref());
    b.grain.image = image;
    b.grain_file = grain_file_for(&file.grain, source.as_ref());
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
    let (w, h, channels, buf) = decode_picture(bytes)?;
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

/// An image's pixels at 8 bits a sample: width, height, samples a pixel and the samples. A brush
/// file's images are PNG or JPEG, whatever they are named.
fn decode_picture(bytes: &[u8]) -> Option<(usize, usize, usize, Vec<u8>)> {
    decode_png(bytes).or_else(|| decode_jpeg(bytes))
}

fn decode_jpeg(bytes: &[u8]) -> Option<(usize, usize, usize, Vec<u8>)> {
    let reader = || image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg);
    // a small file can claim a picture of gigabytes
    let (w, h) = reader().into_dimensions().ok()?;
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || w.checked_mul(h)?.checked_mul(3)? > MOST_PNG_BYTES {
        return None;
    }
    Some((w, h, 3, reader().decode().ok()?.into_rgb8().into_raw()))
}

/// A PNG's pixels, as [`decode_picture`] gives them.
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
            None => {
                log::warn!("a brush file's shape can't be read: drawing with a stand-in");
                (Some(shape::GRAINY_DISC), None, None)
            }
        },
        Texture::Stock { name, inverted } => {
            // a tip printed in dots: ours dotted on the same lattice, each dot as big as the stock
            // tip's ink there
            if let Some(lattice) = halftone_lattice(name)
                && let (Some(rows), Some(cols), Some(tones)) = (
                    textures::stock_shape_rows(name, *inverted),
                    textures::stock_shape_cols(name, *inverted),
                    textures::stock_shape_tones(name, *inverted),
                )
            {
                let cover = tones.iter().map(|&t| f32::from(t)).sum::<f32>() / (tones.len().max(1) as f32 * 255.0);
                let dots = Source::new(textures::halftone_shape(lattice, &rows, &cols, cover));
                return (Some(shape::STOCK_DOTS), dots, None);
            }
            let (image, source, hardness) = stock_stand_in(name, *inverted);
            // our shape with the stock one's own lights and darks and its rows' share of the ink,
            // where they were measured: all of the stock one as its brush shows it (inverted, for
            // one that inverts it: most stock tips are drawn dark on white), our shape as it is
            let retoned = image
                .filter(|_| source.is_none())
                .zip(textures::stock_shape_tones(name, *inverted))
                .and_then(|(i, tones)| {
                    let mut shape = textures::shapes().into_iter().nth(i as usize)?;
                    // as wide and tall as the real tip, where it lies in its square (the grainy
                    // square, standing in for a tip we know nothing else of, takes its outline
                    // across and down below)
                    if i != shape::STOCK_GRAINY_SQUARE
                        && let (Some(rows), Some(cols)) =
                            (textures::stock_shape_rows(name, *inverted), textures::stock_shape_cols(name, *inverted))
                        && let Some(fitted) = textures::fitted_to_box(&shape.pixels, &rows, &cols)
                    {
                        shape.pixels = fitted;
                    }
                    // its marks as fine as the real tip's: standing in for a tip we know nothing
                    // else of, the grainy square (smooth for a solid one); any other shape of ours
                    // takes them over itself
                    if let Some(feature) = textures::stock_shape_feature(name) {
                        let cover =
                            tones.iter().map(|&t| f32::from(t)).sum::<f32>() / (tones.len().max(1) as f32 * 255.0);
                        if i == shape::STOCK_GRAINY_SQUARE {
                            shape.pixels = textures::grainy_square_of(feature, cover);
                        } else if let Some(marked) = textures::textured_shape(&shape.pixels, feature, cover) {
                            shape.pixels = marked;
                        }
                    }
                    // its streaks and gaps across the stroke first (and, standing in for a tip we
                    // know nothing else of, its outline across too)
                    let rows = textures::stock_shape_rows(name, *inverted);
                    let cols = textures::stock_shape_cols(name, *inverted).filter(|_| i == shape::STOCK_GRAINY_SQUARE);
                    let striped = match (rows, cols) {
                        (Some(rows), Some(cols)) => Some(textures::shaped(&shape.pixels, &rows, &cols)),
                        (Some(rows), None) => Some(textures::striped(&shape.pixels, &rows)),
                        _ => None,
                    };
                    let pixels = striped.as_deref().unwrap_or(&shape.pixels);
                    Source::new(textures::retoned_shape(pixels, &tones, &textures::SHAPE_TONE_SHARES))
                });
            (image, retoned.or(source), hardness)
        }
    }
}

/// The lattice a stock tip printed in dots has its dots on, as [`textures::halftone_shape`] takes it
/// (each set of rows as its cycles across and down the image), as measured; none for any other.
fn halftone_lattice(name: &str) -> Option<[(f32, f32); 2]> {
    let n = name.to_lowercase();
    // its dots about 47 to the side, on rows turned 17 degrees and their crossing ones 109
    n.contains("phoenix_secondary_shape").then_some([(-45.30, -13.85), (14.75, -42.60)])
}

/// Our stand-in for the stock shape `name`: our nearest shape, or the round tip at a hardness, or
/// (blank) the whole square.
fn stock_stand_in(name: &str, inverted: bool) -> (Option<u32>, Option<Source>, Option<f32>) {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| names(&n, w));
    // the round tip's words as whole words of the name ("shard" is no hard tip, nor "penguin" a pen)
    let words: Vec<&str> = n.split(|c: char| !c.is_alphanumeric()).collect();
    let says = |these: &[&str]| these.iter().any(|w| words.contains(w));
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
    } else if says(&["hard", "pen", "solid", "round"]) && !has(&["soft", "ink", "rect"]) {
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

/// Whether the name `n` (lower case) says `word`: a word as the start of one of the name's words, or
/// (five letters or more) as the end of one, so that "blotch" says blot and "wildgrass" grass but
/// "fairy" doesn't say air nor "minke" ink; a phrase anywhere in it.
fn names(n: &str, word: &str) -> bool {
    if !word.chars().all(|c| c.is_ascii_alphabetic()) {
        return n.contains(word);
    }
    n.split(|c: char| !c.is_alphanumeric()).any(|t| t.starts_with(word) || (word.len() >= 5 && t.ends_with(word)))
}

/// Our shape nearest a stock shape's name.
fn stock_shape(n: &str) -> u32 {
    // a tip named for a brush of its own ("Name_Shape", "Name_Secondary_Shape": a bird, a plant, a
    // place) says nothing of what it is: the grainy square, taking its measured outline
    let base = n.rsplit('/').next().unwrap_or(n);
    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    if stem.ends_with("_shape") && stem.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        return shape::STOCK_GRAINY_SQUARE;
    }
    let table: &[(&[&str], u32)] = &[
        // drawn things of their own
        (&["scribble"], shape::SCRIBBLE),
        (&["lightning", "bolt"], shape::BOLT),
        (&["flower", "petal", "blossom"], shape::FLOWER),
        (&["diamond", "crystal"], shape::DIAMOND),
        (&["reticle"], shape::RING),
        (&["spike"], shape::STOCK_LINE),
        (&["tendril", "tentacle"], shape::STOCK_HOOK),
        (&["pebble", "rocks", "limestone"], shape::STONE),
        (&["square-dots", "square dots", "squaredots"], shape::STOCK_SQUARE_DOTS),
        (&["dots"], shape::STOCK_DOTS),
        (&["brick"], shape::BLOCK),
        (&["spray", "mist", "aero", "speckle"], shape::SPRAY),
        // a blob with paint running down from it
        (&["drip"], shape::DRIP),
        (&["splat", "flick", "spatter"], shape::SPLATTER),
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
        // one curved calligraphic stroke
        (&["wind", "hook"], shape::STOCK_HOOK),
        (&["wisp"], shape::WISP),
        (&["star", "sparkle", "glint", "flare", "twinkle"], shape::SPARKLE),
        (&["ring", "bokeh", "circle", "bubble"], shape::RING),
        (&["square", "pixel", "block", "box", "rect"], shape::SQUARE),
        (&["hatch", "line", "stripe"], shape::HATCH),
        (&["noise", "grain", "grit", "sand", "texture"], shape::STOCK_GRAINY_DISC),
    ];
    // one we have no word for: grain filling the square, taking its outline from how its ink lies
    table.iter().find(|(words, _)| words.iter().any(|w| names(n, w))).map_or(shape::STOCK_GRAINY_SQUARE, |(_, s)| *s)
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
            // our pattern with the stock grain's own lights and darks, where they were measured;
            // one drawn to the stock image's proportions as it is drawn (measured smaller, the
            // stock image's fine lines and dots go grey, which would take the pattern's contrast)
            let source = image.and_then(|i| {
                // a paper or a wash: noise as strong at each size as the stock image is, with its
                // tones (tiled as a share of that image is)
                if textures::drawn_by_octaves(i)
                    && let (Some((_, octaves)), Some(tones)) =
                        (textures::stock_grain_octaves(name), textures::stock_grain_tones(name))
                {
                    let mut pixels = textures::octave_grain(&octaves, &tones);
                    if *inverted {
                        pixels.iter_mut().for_each(|v| *v = 255 - *v);
                    }
                    return Source::new(pixels);
                }
                let pattern = textures::grains().into_iter().nth(i as usize)?;
                let mut pixels = if textures::drawn_to_stock_size(i) {
                    pattern.pixels
                } else {
                    textures::retoned(&pattern.pixels, &textures::stock_grain_tones(name)?, &textures::TONE_SHARES)
                };
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
        // patterns named for the brushes that draw them (one of them has "stone" in its name):
        // ours drawn to the same proportions
        (&["homestead_grain"], grain::GRAPH_PAPER),
        (&["ironstone_grain"], grain::DOT_GRID),
        (&["snow_daisy_grain"], grain::DOTTED_STAR),
        (&["brush-preset-diagonal"], grain::DIAGONAL_STRIPES),
        (&["tessellated"], grain::LABYRINTH),
        (&["brush-preset-grid"], grain::FINE_GRID),
        (&["dof-dots-light"], grain::SOFT_DOTS),
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
    let (w, h, channels, buf) = decode_picture(bytes)?;
    Source::new(gray_square((w, h, channels), &buf, inverted, TEXTURE_SIZE))
}

/// What the image of a brush file's grain gives it beyond `source`: its width, and the image at up
/// to [`FULL_GRAIN_MOST`] a side if it has more than `source` holds.
fn grain_file_for(t: &Texture, source: Option<&Source>) -> Option<GrainFile> {
    let (Texture::Image { bytes, inverted }, Some(source)) = (t, source) else { return None };
    let (w, h, channels, buf) = decode_picture(bytes)?;
    Some(GrainFile { source: source.id, width: w as u32, full: full_image((w, h, channels), &buf, *inverted) })
}

/// What the image of a brush file's shape gives it beyond `source`, if it has more than that holds.
fn shape_file_for(t: &Texture, source: Option<&Source>) -> Option<ShapeFile> {
    let (Texture::Image { bytes, inverted }, Some(source)) = (t, source) else { return None };
    let (w, h, channels, buf) = decode_picture(bytes)?;
    Some(ShapeFile { source: source.id, full: full_image((w, h, channels), &buf, *inverted)? })
}

/// The side of the square an image `longest` pixels across is kept in: as much of it as is kept of
/// any, and never less than a `Source` holds.
pub(crate) fn image_side(longest: usize) -> usize {
    longest.next_power_of_two().clamp(TEXTURE_SIZE, FULL_GRAIN_MOST)
}

/// A decoded image (as [`gray_square`] takes it) at up to [`FULL_GRAIN_MOST`] a side, if it has more
/// than a `Source` holds.
fn full_image(size: (usize, usize, usize), buf: &[u8], inverted: bool) -> Option<FullGrain> {
    let longest = size.0.max(size.1);
    (longest > TEXTURE_SIZE)
        .then(|| image_side(longest))
        .and_then(|side| FullGrain::new(side as u32, gray_square(size, buf, inverted, side)))
}

/// A decoded image (`buf`, `w` x `h`, of `channels`) as grey, `n` square, white for paint.
pub(crate) fn gray_square((w, h, channels): (usize, usize, usize), buf: &[u8], inverted: bool, n: usize) -> Vec<u8> {
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
    pixels
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
pub(crate) fn sized_gray_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
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
    fn a_stock_tip_of_one_curved_stroke_stands_in_as_our_hook() {
        // swept along a stroke it paints a broad band, as such a tip does; a wisp stays a wisp
        assert_eq!(stock_shape("wind.png"), shape::STOCK_HOOK);
        assert_eq!(stock_shape("soft wisp.png"), shape::WISP);
        assert_eq!(textures::shapes()[shape::STOCK_HOOK as usize].name, "Hook");
    }

    #[test]
    fn stock_pattern_grains_take_ours_drawn_to_their_proportions() {
        assert_eq!(named_grain("library/homestead_grain.png"), Some(grain::GRAPH_PAPER));
        assert_eq!(named_grain("brush-preset-grid.png"), Some(grain::FINE_GRID));
        assert_eq!(named_grain("dof-dots-light.png"), Some(grain::SOFT_DOTS));
        assert_eq!(named_grain("library/ironstone_grain.png"), Some(grain::DOT_GRID));
        assert_eq!(named_grain("library/snow_daisy_grain.png"), Some(grain::DOTTED_STAR));
        assert_eq!(named_grain("brush-preset-diagonal.png"), Some(grain::DIAGONAL_STRIPES));
        assert_eq!(named_grain("tessellated.png"), Some(grain::LABYRINTH));
        assert_eq!(named_grain("stone_wall.png"), Some(grain::CONCRETE));
    }

    #[test]
    fn a_stock_tip_only_older_brushes_name_stands_in_as_faint_as_it_is() {
        // no brush the other app comes with names this tip (older brushes and people's own still
        // do): unmeasured, it stood in as our bright chunk, so a dual brush's second drawn with it
        // built up solid and passed its first through uncut
        let tip = Texture::Stock { name: "Gouache-Drop-3.jpg".to_owned(), inverted: false };
        let (_, source, _) = shape_for(&tip);
        let pixels = source.expect("retoned to its measured tones").pixels;
        let mean = pixels.iter().map(|&v| f32::from(v)).sum::<f32>() / pixels.len() as f32 / 255.0;
        assert!(mean < 0.1, "{mean}");
        // and a dual brush's second's own tip and grain are measured as its first's are
        assert!(textures::stock_shape_tones("nightjar_secondary_shape.png", false).is_some());
        assert!(textures::stock_grain_tones("lumita_secondary_grain.png").is_some());
    }

    #[test]
    fn a_stock_tip_stands_in_as_wide_as_it_inks() {
        // a dab inked across 85% of its square and down 70% (ours, a chunk, had 68% and 66%): its
        // stroke came out a fifth too narrow
        let tip = Texture::Stock { name: "Acrylic-Dab-3.jpg".to_owned(), inverted: false };
        let (_, source, _) = shape_for(&tip);
        let pixels = source.expect("retoned").pixels;
        let n = TEXTURE_SIZE;
        let span = |across: bool| {
            let inked =
                |i: usize| (0..n).filter(|&j| pixels[if across { j * n + i } else { i * n + j }] > 13).count() > n / 50;
            let first = (0..n).find(|&i| inked(i)).unwrap_or(0);
            let last = (0..n).rev().find(|&i| inked(i)).unwrap_or(0);
            (last - first) as f32 / n as f32
        };
        assert!(span(true) > 0.74, "{}", span(true));
        assert!((0.62..0.76).contains(&span(false)), "{}", span(false));
    }

    #[test]
    fn a_stock_tip_printed_in_dots_stands_in_as_dots_on_its_lattice() {
        let tip = Texture::Stock { name: "Phoenix_Secondary_Shape.png".to_owned(), inverted: true };
        let (image, source, _) = shape_for(&tip);
        assert_eq!(image, Some(shape::STOCK_DOTS));
        let pixels = source.expect("dots drawn").pixels;
        let n = TEXTURE_SIZE;
        let a: Vec<f32> = pixels.iter().map(|&v| f32::from(v) / 255.0).collect();
        // as much ink as the stock tip inverted holds (about a sixth), most of it on its right
        let mean = a.iter().sum::<f32>() / a.len() as f32;
        assert!((0.1..0.25).contains(&mean), "{mean}");
        let half = |right: bool| {
            (0..n * n).filter(|k| (k % n >= n / 2) == right).map(|k| a[k]).sum::<f32>() / (n * n / 2) as f32
        };
        assert!(half(true) > 2.0 * half(false), "{} {}", half(true), half(false));
        // the dots repeat along the lattice's rows: shifted by one step of it, the inky middle of the
        // tip matches itself far better than shifted half a step
        let [(ax, ay), (bx, by)] = halftone_lattice("phoenix_secondary_shape.png").unwrap();
        let det = ax * by - ay * bx;
        // one step of the lattice, a row of the first set across, along a row of the second
        let step = (by / det * n as f32, -bx / det * n as f32);
        let differ = |k: f32| {
            let (dx, dy) = ((step.0 * k).round() as i32, (step.1 * k).round() as i32);
            let mut sum = 0.0;
            for y in 96..160i32 {
                for x in 150..214i32 {
                    let at = |x: i32, y: i32| a[(y as usize) * n + x as usize];
                    sum += (at(x, y) - at(x + dx, y + dy)).abs();
                }
            }
            sum
        };
        assert!(differ(1.0) < 0.5 * differ(0.5), "{} {}", differ(1.0), differ(0.5));
    }

    #[test]
    fn a_stock_tip_named_for_a_brush_of_its_own_takes_its_outline_whatever_its_name_says() {
        // a bird's or a place's name says nothing of the tip ("ringtail" isn't a ring, nor
        // "starling" a star); a plain word does
        assert_eq!(stock_shape("ringtail_shape.png"), shape::STOCK_GRAINY_SQUARE);
        assert_eq!(stock_shape("folder/starling_secondary_shape.png"), shape::STOCK_GRAINY_SQUARE);
        assert_eq!(stock_shape("brush-preset-scribble.jpg"), shape::SCRIBBLE);
        assert_eq!(stock_shape("brush-preset-spike.png"), shape::STOCK_LINE);
        assert_eq!(stock_shape("brush-artery-drip-2.jpg"), shape::DRIP);
        assert_eq!(stock_shape("brush-artery-cloud-shape.jpg"), shape::CLOUD);
        assert_eq!(textures::shapes()[shape::STOCK_LINE as usize].name, "Line");
    }

    #[test]
    fn a_small_tip_is_drawn_up_smoothly_and_a_large_one_down_by_its_average() {
        use procraft_engine::abr::AbrTip;
        // two pixels side by side, black and white: drawn up, grey between them, not two blocks
        let small = AbrTip { name: String::new(), spacing: None, width: 2, height: 2, pixels: vec![0, 255, 0, 255] };
        let n = TEXTURE_SIZE;
        let up = fitted(&small);
        let row: Vec<u8> = up[n / 2 * n..n / 2 * n + n].to_vec();
        assert_eq!((row[0], row[n - 1]), (0, 255));
        assert!(row.windows(2).all(|p| p[0] <= p[1]), "rising from black to white");
        let between = row.iter().filter(|&&v| v > 10 && v < 245).count();
        assert!(between > n / 4, "{between} pixels between");
        // a tip twice the square's size, in 2x2 checks: each pixel the average of the four it covers
        let big = AbrTip {
            name: String::new(),
            spacing: None,
            width: 2 * n as u32,
            height: 2 * n as u32,
            pixels: (0..4 * n * n)
                .map(|i| if (i % (2 * n) + i / (2 * n)).is_multiple_of(2) { 255 } else { 0 })
                .collect(),
        };
        assert!(fitted(&big).iter().all(|&v| v == 127));
    }

    #[test]
    fn a_krita_texture_is_its_mask_combined_by_its_mode() {
        use procraft_engine::document::BlendMode;
        use procraft_engine::krita::{KritaBrush, KritaPicture, KritaTexture, KritaTip};
        // black dots on clear, which Krita takes as white about them
        let dots: Vec<u8> = (0..16 * 16).flat_map(|i| [0, 0, 0, if i % 4 == 0 { 255 } else { 0 }]).collect();
        let texture = KritaTexture {
            picture: KritaPicture::Encoded(crate::project::rgba_png(16, 16, &dots)),
            scale: 1.0,
            strength: 1.0,
            strength_pressure: None,
            soft: false,
            invert: false,
            mode: 1,
            brightness: 0.0,
            contrast: 1.0,
            neutral: 0.5,
            cutoff: (0, 0.0, 1.0),
        };
        let brush = |texture: KritaTexture| {
            krita_brush(&KritaBrush {
                name: "Paper".into(),
                tip: KritaTip::Round { hardness: 1.0, roundness: 1.0 },
                spacing: None,
                angle: 0.0,
                follows: false,
                turns_at_random: false,
                rotation_random: 0.0,
                rotation_per_stroke: false,
                follows_tilt: false,
                mirror: (false, false),
                scatter: None,
                color_random: [0.0; 4],
                color_per_stroke: [0.0; 4],
                ratio_random: 0.0,
                ratio_pressure: None,
                mix_pressure: None,
                size_pressure: None,
                opacity_pressure: None,
                flow_pressure: None,
                opacity: 1.0,
                flow: 1.0,
                wash: true,
                texture: Some(texture),
                missing: Vec::new(),
                masking: None,
            })
        };
        let b = brush(texture.clone());
        let source = b.grain.source.as_ref().unwrap();
        let (dark, light) =
            (source.pixels.iter().filter(|&&v| v == 0).count(), source.pixels.iter().filter(|&&v| v == 255).count());
        assert!(dark > 0 && light > 3 * dark, "{dark} {light}");
        // subtracted, as it says; multiplied otherwise, and where a mode has no near one of ours
        assert_eq!(b.setting("grain_blend"), BlendMode::Subtract.id() as f32);
        assert_eq!(
            brush(KritaTexture { mode: 0, ..texture.clone() }).setting("grain_blend"),
            BlendMode::Multiply.id() as f32
        );
        assert_eq!(
            brush(KritaTexture { mode: 2, ..texture.clone() }).setting("grain_blend"),
            BlendMode::Multiply.id() as f32
        );
        // laid over the whole stroke, at its strength, while that doesn't follow the pressure
        let still = brush(KritaTexture { strength: 0.8, ..texture.clone() });
        assert!(!still.grain_each_stamp() && (still.grain.depth - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_krita_texture_s_strength_follows_the_pressure_as_krita_takes_it() {
        use procraft_engine::brush::{GRAIN_HEIGHT, PressureCurve};
        use procraft_engine::document::BlendMode;
        use procraft_engine::geom::Vec2;
        use procraft_engine::krita::{KritaBrush, KritaPicture, KritaTexture, KritaTip};
        let near = |a: f32, b: f32| (a - b).abs() < 2e-3;
        let dots: Vec<u8> = (0..16 * 16).flat_map(|i| [0, 0, 0, if i % 4 == 0 { 255 } else { 0 }]).collect();
        let pressed = vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)];
        let texture = KritaTexture {
            picture: KritaPicture::Encoded(crate::project::rgba_png(16, 16, &dots)),
            scale: 1.0,
            strength: 0.9,
            strength_pressure: Some(pressed.clone()),
            soft: false,
            invert: false,
            mode: 1,
            brightness: 0.0,
            contrast: 1.0,
            neutral: 0.5,
            cutoff: (0, 0.0, 1.0),
        };
        let brush = |texture: KritaTexture| {
            krita_brush(&KritaBrush {
                name: "Pencil".into(),
                tip: KritaTip::Round { hardness: 1.0, roundness: 1.0 },
                spacing: None,
                angle: 0.0,
                follows: false,
                turns_at_random: false,
                rotation_random: 0.0,
                rotation_per_stroke: false,
                follows_tilt: false,
                mirror: (false, false),
                scatter: None,
                color_random: [0.0; 4],
                color_per_stroke: [0.0; 4],
                ratio_random: 0.0,
                ratio_pressure: None,
                mix_pressure: None,
                size_pressure: None,
                opacity_pressure: None,
                flow_pressure: None,
                opacity: 1.0,
                flow: 0.5,
                wash: false,
                texture: Some(texture),
                missing: Vec::new(),
                masking: None,
            })
        };
        // subtracted (Krita's own way): stamp by stamp, a height a twelfth as deep as the strength,
        // its depth by the pressure
        let b = brush(texture.clone());
        assert!(b.grain_each_stamp());
        assert_eq!(b.setting("grain_blend"), GRAIN_HEIGHT as f32);
        assert!(near(b.grain.depth, 0.9 / 12.0));
        assert!(b.pressure_curves.get(PressureCurve::GrainDepth).is_some_and(|c| near(c.eval(0.5), 0.5)));
        // multiplied: the grain whole, over the whole stroke, the strength less paint laid
        let multiplied = brush(KritaTexture { mode: 0, ..texture.clone() });
        assert!(!multiplied.grain_each_stamp() && near(multiplied.grain.depth, 1.0));
        assert_eq!(multiplied.setting("grain_blend"), BlendMode::Multiply.id() as f32);
        let plain = brush(KritaTexture { mode: 0, strength: 1.0, strength_pressure: None, ..texture.clone() });
        assert!(near(multiplied.flow.powf(0.4), (plain.flow.powf(0.4) * 0.9).min(1.0)) || multiplied.flow < plain.flow);
        assert!(multiplied.pressure_opacity > 0.0 && plain.pressure_opacity == 0.0);
        // soft: as deep as the strength, stamp by stamp by the pressure, in its own mode
        let soft = brush(KritaTexture { soft: true, ..texture.clone() });
        assert!(soft.grain_each_stamp() && near(soft.grain.depth, 0.9));
        assert_eq!(soft.setting("grain_blend"), BlendMode::Subtract.id() as f32);
        // a strength that doesn't follow the pressure: over the whole stroke, as before
        let still = brush(KritaTexture { strength_pressure: None, ..texture });
        assert!(!still.grain_each_stamp() && near(still.grain.depth, 0.9));
        assert_eq!(still.setting("grain_blend"), BlendMode::Subtract.id() as f32);
    }

    #[test]
    fn a_krita_brush_follows_the_pen_and_lays_as_its_painting_mode_says() {
        use procraft_engine::brush::{FLOW_POWER, PressureCurve};
        use procraft_engine::geom::Vec2;
        use procraft_engine::krita::{KritaBrush, KritaTip};
        let near = |a: f32, b: f32| (a - b).abs() < 2e-3;
        let straight = vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)];
        let built = KritaBrush {
            name: "Pen".into(),
            tip: KritaTip::Round { hardness: 1.0, roundness: 1.0 },
            spacing: None,
            angle: 0.0,
            follows: false,
            turns_at_random: false,
            rotation_random: 0.0,
            rotation_per_stroke: false,
            follows_tilt: false,
            mirror: (false, false),
            scatter: None,
            color_random: [0.0; 4],
            color_per_stroke: [0.0; 4],
            ratio_random: 0.0,
            ratio_pressure: None,
            mix_pressure: None,
            size_pressure: Some(vec![Vec2::new(0.0, 0.2), Vec2::new(1.0, 1.0)]),
            opacity_pressure: Some(straight.clone()),
            flow_pressure: None,
            opacity: 0.8,
            flow: 0.5,
            wash: false,
            texture: None,
            missing: Vec::new(),
            masking: None,
        };
        // the size from its curve's least up to all of it
        let b = krita_brush(&built);
        assert!(b.pressure_size == 1.0 && near(b.size_curve.eval(0.0), 0.2) && near(b.size_curve.eval(1.0), 1.0));
        // built up: each stamp lays the opacity times the flow, by the opacity's curve once eased
        assert!(near(b.flow, 0.4) && b.pressure_opacity == 1.0);
        assert!(near(b.opacity_curve.eval(0.5).powf(FLOW_POWER), 0.5));
        // washed: each stamp lays the flow, the stroke building up to the opacity by the pressure
        let w = krita_brush(&KritaBrush { wash: true, ..built.clone() });
        assert!(near(w.flow, 0.5) && w.pressure_opacity == 0.0 && w.setting("rendering_mode") == 1.0);
        let most = w.pressure_curves.get(PressureCurve::Flow).unwrap();
        assert!(near(most.eval(0.5), 0.4) && near(most.eval(1.0), 0.8));
        // no pressure on the size: it stays as set
        assert_eq!(krita_brush(&KritaBrush { size_pressure: None, ..built }).pressure_size, 0.0);
    }

    #[test]
    fn a_krita_brush_turns_mirrors_and_scatters_as_its_preset_says() {
        use procraft_engine::brush::PressureCurve;
        use procraft_engine::geom::Vec2;
        use procraft_engine::krita::{KritaBrush, KritaScatter, KritaTip};
        let straight = vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)];
        let k = KritaBrush {
            name: "Spray".into(),
            tip: KritaTip::Round { hardness: 1.0, roundness: 1.0 },
            spacing: None,
            angle: 0.0,
            follows: false,
            turns_at_random: false,
            rotation_random: 0.6,
            rotation_per_stroke: true,
            follows_tilt: true,
            mirror: (true, false),
            scatter: Some(KritaScatter { amount: 0.5, pressure: Some(straight), along: true, across: false }),
            color_random: [0.4, 0.0, 0.3, 0.3],
            color_per_stroke: [0.0, 0.5, 0.0, 0.0],
            ratio_random: 0.7,
            ratio_pressure: None,
            mix_pressure: None,
            size_pressure: None,
            opacity_pressure: None,
            flow_pressure: None,
            opacity: 1.0,
            flow: 1.0,
            wash: true,
            texture: None,
            missing: Vec::new(),
            masking: None,
        };
        let b = krita_brush(&k);
        assert!((b.shape.scatter - 0.6).abs() < 1e-6 && b.shape.randomise && b.shape.follow_azimuth);
        assert_eq!((b.setting("flip_x"), b.setting("flip_y")), (1.0, 0.0));
        // half a diameter at most along the stroke: our jitter's control to the power 2.2, times its
        // scale, of half the round stamp's diagonal; by the pressure; none across
        let thrown =
            procraft_engine::stroke::JITTER_FILE_SCALE * b.setting("jitter_linear").powf(2.2) * 0.5 * 2f32.sqrt();
        assert!((thrown - 0.5).abs() < 1e-3, "{thrown}");
        assert!(b.pressure_curves.get(PressureCurve::LinearJitter).is_some());
        assert!(b.pressure_curves.get(PressureCurve::LateralJitter).is_none());
        // its colour at random each stamp and each stroke, and flattened at random
        let stamp = ["stamp_hue", "stamp_saturation", "stamp_lightness", "stamp_darkness"].map(|k| b.setting(k));
        assert_eq!((stamp, b.setting("stroke_saturation")), ([0.4, 0.0, 0.3, 0.3], 0.5));
        assert_eq!(b.setting("roundness_jitter_y"), 0.7);
        // its colour by the pressure: the secondary colour where light, its own where full
        let mixed =
            krita_brush(&KritaBrush { mix_pressure: Some(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]), ..k });
        let curve = mixed.pressure_curves.get(PressureCurve::Secondary).unwrap();
        assert_eq!(mixed.setting("pressure_secondary"), 1.0);
        assert!((curve.eval(0.0) - 1.0).abs() < 1e-3 && curve.eval(1.0).abs() < 1e-3);
    }

    #[test]
    fn a_masking_brush_lays_as_krita_s_wash_mode_does() {
        use procraft_engine::brush::{FLOW_POWER, PressureCurve, Rendering};
        use procraft_engine::geom::Vec2;
        use procraft_engine::krita::{KritaBrush, KritaMasking, KritaTip};
        let brush = KritaBrush {
            name: "Mask".into(),
            tip: KritaTip::Round { hardness: 1.0, roundness: 1.0 },
            spacing: None,
            angle: 0.0,
            follows: false,
            turns_at_random: false,
            rotation_random: 0.0,
            rotation_per_stroke: false,
            follows_tilt: false,
            mirror: (false, false),
            scatter: None,
            color_random: [0.0; 4],
            color_per_stroke: [0.0; 4],
            ratio_random: 0.0,
            ratio_pressure: None,
            mix_pressure: None,
            size_pressure: Some(vec![
                procraft_engine::geom::Vec2::new(0.0, 0.0),
                procraft_engine::geom::Vec2::new(1.0, 1.0),
            ]),
            opacity_pressure: None,
            flow_pressure: None,
            opacity: 1.0,
            flow: 1.0,
            wash: true,
            texture: None,
            missing: Vec::new(),
            masking: None,
        };
        let curve = vec![Vec2::new(0.6, 0.0), Vec2::new(0.9, 0.75)];
        let m = KritaMasking {
            brush,
            op: "linear_dodge".into(),
            size: 0.5,
            opacity: 0.8,
            flow: 0.3,
            opacity_pressure: Some(curve.clone()),
            flow_pressure: Some(curve),
        };
        let mut second = Brush::round_pen();
        masking_paint(&mut second, &m);
        assert_eq!((second.rendering, second.max_opacity, second.flow), (Rendering::Capped, 1.0, 0.3));
        // the stroke reaches the opacity times its curve: none under 0.6, 0.8 x 0.75 from 0.9
        let most = second.pressure_curves.get(PressureCurve::Flow).unwrap();
        assert!(most.eval(0.5) < 1e-3 && (most.eval(1.0) - 0.6).abs() < 1e-3);
        assert_eq!(second.setting("pressure_flow"), 1.0);
        // a stamp lays the flow's curve: its opacity's pressure to the power FLOW_POWER
        assert_eq!(second.pressure_opacity, 1.0);
        let laid = |p: f32| second.opacity_curve.eval(p).powf(FLOW_POWER);
        assert!(laid(0.5) < 1e-3 && (laid(1.0) - 0.75).abs() < 5e-3);
        // without pressure, a constant: none at full opacity, else the opacity
        let fixed = KritaMasking { opacity_pressure: None, flow_pressure: None, ..m.clone() };
        let mut second = Brush::round_pen();
        masking_paint(&mut second, &fixed);
        assert_eq!(second.pressure_opacity, 0.0);
        assert!((second.pressure_curves.get(PressureCurve::Flow).unwrap().eval(0.1) - 0.8).abs() < 1e-3);
        let mut second = Brush::round_pen();
        masking_paint(&mut second, &KritaMasking { opacity: 1.0, ..fixed });
        assert!(second.pressure_curves.get(PressureCurve::Flow).is_none());
        assert_eq!(second.setting("pressure_flow"), 0.0);
    }

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
            texture: Some(procraft_engine::csp::CspTexture {
                pngs: vec![b"not a picture".to_vec(), crate::project::rgba_png(8, 8, &[0; 256]), paper],
                density: Some(0.6),
                scale: Some(2.0),
                ..Default::default()
            }),
            ..Default::default()
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
    fn a_sub_tools_texture_comes_at_its_pictures_own_size_with_its_settings() {
        use procraft_engine::csp::{CspBrush, CspTexture, TextureMode};
        let shown = |b: &Brush, key: &str| (b.setting(key) * 100.0).round() / 100.0;
        // a thumbnail 100 px wide of a picture 600 px wide, black dots on clear
        let dots: Vec<u8> = (0..100 * 100).flat_map(|i| [0, 0, 0, if i % 7 == 0 { 255 } else { 0 }]).collect();
        let t = CspTexture {
            pngs: vec![crate::project::rgba_png(100, 100, &dots)],
            width: Some(600),
            density: Some(0.3),
            scale: Some(0.5),
            mode: TextureMode::Subtract,
            rotation: 270f32.to_radians(),
            brightness: 0.4,
            contrast: -0.2,
            ..Default::default()
        };
        let c = CspBrush { name: "Chalk".into(), texture: Some(t.clone()), ..Default::default() };
        let b = csp_brush(&c);
        // tiled at half the picture's 600 px, fixed to the canvas, 30 % deep, taking paint away
        assert_eq!((b.grain.scale, b.grain.mode, b.grain.depth), (300.0, GrainMode::Texturized, 0.3));
        assert_eq!(b.setting("grain_blend") as u32, procraft_engine::document::BlendMode::Subtract.id());
        // lighter is less ink; three quarters round is a quarter back; smooth, not a pixel pattern
        let kept = ["grain_brightness", "grain_contrast", "grain_turn", "grain_filtering"].map(|k| shown(&b, k));
        assert_eq!(kept, [-0.4, -0.2, -0.5, 2.0]);
        // reversed, lighter is more; its differences made stronger: from half its density on, fully
        let reversed = csp_brush(&CspBrush {
            texture: Some(CspTexture { reverse: true, emphasized: true, density: Some(0.6), ..t.clone() }),
            ..c.clone()
        });
        assert_eq!((shown(&reversed, "grain_brightness"), reversed.grain.depth), (0.4, 1.0));
        // a dither's few pixels, at its own size (no material size given): read sharp
        let dither: Vec<u8> = (0..25).flat_map(|i| [0, 0, 0, if i == 0 { 255 } else { 0 }]).collect();
        let pattern = CspTexture { pngs: vec![crate::project::rgba_png(5, 5, &dither)], width: None, scale: None, ..t };
        let b = csp_brush(&CspBrush { texture: Some(pattern), ..c });
        assert_eq!((b.grain.scale, shown(&b, "grain_filtering")), (5.0, 0.0));
    }

    #[test]
    fn a_large_sampled_tip_keeps_its_own_detail_beside_the_small_square() {
        use procraft_engine::abr::AbrTip;
        // a tip twice as wide as tall, solid across its top half and shading off left to right below
        let tip = |w: u32, h: u32| AbrTip {
            name: "Tip".into(),
            spacing: None,
            width: w,
            height: h,
            pixels: (0..w * h).map(|i| if i / w < h / 2 { 255 } else { (255 - (i % w) * 255 / w) as u8 }).collect(),
        };
        let b = tip_brush(&tip(600, 300));
        let full = &b.shape_file.as_ref().expect("a full shape").full;
        assert_eq!(full.side, 512);
        assert_eq!(b.shape_file.as_ref().unwrap().source, b.shape.source.as_ref().unwrap().id);
        // framed as the small square is: the tip across the middle, as wide as the square
        let small = &b.shape.source.as_ref().unwrap().pixels;
        let pixels = full.pixels.as_deref().unwrap();
        let (n, m) = (TEXTURE_SIZE, 512usize);
        for (x, y) in [(10, 70), (128, 100), (250, 120), (128, 20), (128, 200), (60, 160)] {
            let big = (0..4).map(|k| u32::from(pixels[(y * 2 + k / 2) * m + x * 2 + k % 2])).sum::<u32>() / 4;
            assert!((big as i32 - i32::from(small[y * n + x])).abs() <= 2, "({x}, {y}): {big} {}", small[y * n + x]);
        }
        // no larger than a `Source` holds: none; very large: as large as is kept
        assert!(tip_brush(&tip(200, 100)).shape_file.is_none());
        assert_eq!(tip_brush(&tip(3000, 1500)).shape_file.unwrap().full.side, 1024);
    }

    #[test]
    fn a_sprayed_sub_tool_lays_its_particles_about_each_place() {
        use procraft_engine::csp::{CspBrush, CspSpray};
        use procraft_engine::stroke::JITTER_FILE_SCALE;
        let spray = CspSpray {
            particle: 10.0,
            density: 24,
            density_by: None,
            bias: -1.0,
            angle: 0.0,
            random_turn: 0.5,
            follows: false,
        };
        let c = CspBrush {
            name: "Spray".into(),
            size: Some(40.0),
            hardness: Some(1.0),
            spacing: Some(0.2),
            spray: Some(spray.clone()),
            ..Default::default()
        };
        let b = csp_brush(&c);
        // a stamp as wide as the area, its particle (a quarter of it) in its middle
        let pixels = &b.shape.source.as_ref().unwrap().pixels;
        let n = TEXTURE_SIZE;
        let at = |x: usize, y: usize| pixels[y * n + x];
        assert_eq!(at(n / 2, n / 2), 255);
        assert!(at(n / 2 + n / 8 - 4, n / 2) > 200 && at(n / 2, n / 2 - n / 8 + 4) > 200);
        assert_eq!((at(n / 2 + n / 8 + 4, n / 2), at(4, 4)), (0, 0));
        // as many to a place as ours lays, turned half way at random, the area round
        assert_eq!((b.shape.count, b.shape.scatter, b.shape.roundness), (16, 0.5, 1.0));
        assert!((b.spacing - 0.2).abs() < 1e-5, "{}", b.spacing);
        // each thrown as far as half the area, either way along the stroke and across it
        let reach = |b: &Brush| JITTER_FILE_SCALE * b.setting("jitter_lateral").powf(2.2) * 0.5 * 2f32.sqrt();
        assert!((reach(&b) - 0.5).abs() < 1e-3, "{}", reach(&b));
        assert_eq!(b.setting("jitter_linear"), b.setting("jitter_lateral"));
        // a particle larger than the area: the stamp as large as it, thrown half the area (a
        // quarter of the stamp), spaced by the area still
        let big = csp_brush(&CspBrush { spray: Some(CspSpray { particle: 80.0, ..spray.clone() }), ..c.clone() });
        assert!((big.max_size / b.max_size - 2.0).abs() < 1e-4, "{} {}", big.max_size, b.max_size);
        assert!((reach(&big) - 0.25).abs() < 1e-3, "{}", reach(&big));
        assert!((big.spacing - 0.1).abs() < 1e-5, "{}", big.spacing);
        // gathered to the middle: thrown half as far
        let gathered = csp_brush(&CspBrush { spray: Some(CspSpray { bias: 1.0, ..spray.clone() }), ..c.clone() });
        assert!((reach(&gathered) - 0.25).abs() < 1e-3, "{}", reach(&gathered));
        // fewer with less pressure, down to a fifth, and some at random
        let by = procraft_engine::csp::CspDynamics {
            pressure: Some(procraft_engine::csp::CspCurve { points: vec![(0.0, 0.0), (1.0, 1.0)], least: 0.2 }),
            random: Some(0.6),
            ..Default::default()
        };
        let pressed = csp_brush(&CspBrush { spray: Some(CspSpray { density_by: Some(by), ..spray }), ..c });
        let curve = pressed.pressure_curves.get(procraft_engine::brush::PressureCurve::Count).unwrap();
        assert!((curve.eval(0.0) - 0.2).abs() < 0.02 && (curve.eval(1.0) - 1.0).abs() < 0.02);
        assert!((pressed.setting("count_jitter") - 0.4).abs() < 1e-5);
    }

    #[test]
    fn a_sub_tools_tip_turns_as_it_says_and_a_ribbon_lays_its_pictures_end_to_end() {
        use procraft_engine::csp::CspBrush;
        use std::f32::consts::FRAC_PI_2;
        // a picture twice as wide as it is tall
        let wide: Vec<u8> = (0..40 * 20).flat_map(|_| [0, 0, 0, 255]).collect();
        let c = CspBrush {
            name: "Wood".into(),
            angle: FRAC_PI_2,
            tip_pngs: vec![crate::project::rgba_png(40, 20, &wide)],
            ..Default::default()
        };
        // held still: its angle as it is
        let b = csp_brush(&c);
        assert_eq!((b.shape.follow, b.shape.rotation, b.shape.scatter), (0.0, FRAC_PI_2, 0.0));
        // turning with the stroke, and a fifth of the way at random: its 0 is a quarter turn on
        let b = csp_brush(&CspBrush { follows: true, random_turn: 0.2, ..c.clone() });
        assert_eq!((b.shape.follow, b.shape.rotation, b.shape.scatter), (1.0, 0.0, 0.2));
        // a ribbon follows the stroke, its pictures (as long as the stamp along it) touching
        let b = csp_brush(&CspBrush { ribbon: true, ..c.clone() });
        assert_eq!((b.shape.follow, b.spacing), (1.0, 1.0));
        // turned across the stroke, each is half as long along it
        let b = csp_brush(&CspBrush { ribbon: true, angle: 0.0, ..c.clone() });
        assert!((b.spacing - 0.5).abs() < 1e-5, "{}", b.spacing);
        // a round tip flattened side to side, ours a quarter turned (turning with the stroke: as
        // it goes, so thin across it); flattened top to bottom, as ours is
        let oval = CspBrush { follows: true, roundness: Some(0.5), tip_pngs: Vec::new(), ..c.clone() };
        assert_eq!(csp_brush(&CspBrush { angle: 0.0, ..oval.clone() }).shape.rotation, 0.0);
        let upright = CspBrush { angle: 0.0, flattened_vertically: true, ..oval };
        assert!((csp_brush(&upright).shape.rotation - 3.0 * FRAC_PI_2).abs() < 1e-5);
        // its opacity, flow and size
        let b = csp_brush(&CspBrush { opacity: Some(0.4), flow: Some(0.7), size: Some(100.0), ..c.clone() });
        assert_eq!(b.max_opacity, 0.4);
        assert!(b.min_opacity <= 0.4);
        assert!((b.setting("flow") - 0.7).abs() < 1e-5);
        let half = csp_brush(&CspBrush { size: Some(50.0), ..c });
        assert!((b.max_size / half.max_size - 2.0).abs() < 1e-3);
    }

    #[test]
    fn a_sub_tools_pen_dynamics_and_its_start_and_end_become_ours() {
        use procraft_engine::brush::PressureCurve;
        use procraft_engine::csp::{CspBrush, CspCurve, CspDynamics, CspPen, CspTaper, CspTexture};
        let near = |a: f32, b: f32| (a - b).abs() < 5e-3;
        let straight = || CspCurve { least: 0.0, points: vec![(0.0, 0.0), (1.0, 1.0)] };
        let bent = CspCurve { least: 0.3, points: vec![(0.0, 0.0), (0.75, 0.25), (1.0, 1.0)] };
        let dots: Vec<u8> = (0..16 * 16).flat_map(|i| [0, 0, 0, if i % 3 == 0 { 255 } else { 0 }]).collect();
        let c = CspBrush {
            name: "Ribbon".into(),
            pen: CspPen {
                size: Some(CspDynamics { pressure: Some(bent.clone()), random: Some(0.6), ..Default::default() }),
                opacity: Some(CspDynamics {
                    tilt: Some(CspCurve { least: 0.2, ..straight() }),
                    speed: Some(0.5),
                    ..Default::default()
                }),
                flow: Some(CspDynamics { pressure: Some(straight()), random: Some(0.9), ..Default::default() }),
                thickness: Some(CspDynamics { pressure: Some(straight()), ..Default::default() }),
                texture_density: Some(CspDynamics {
                    pressure: Some(CspCurve { least: 0.4, ..straight() }),
                    ..Default::default()
                }),
            },
            taper: Some(CspTaper { start: Some(128.0), end: None, of_stroke: false, size: 1.0, opacity: 0.5 }),
            texture: Some(CspTexture { pngs: vec![crate::project::rgba_png(16, 16, &dots)], ..Default::default() }),
            ..Default::default()
        };
        let b = csp_brush(&c);
        // pressure takes the size down to 30 % along its own curve (through its points); chance
        // up to 40 %
        assert!(near(b.pressure_size, 0.7));
        for (x, y) in [(0.0, 0.0), (0.75, 0.25), (1.0, 1.0)] {
            assert!(near(b.size_curve.eval(x), y), "{x}: {}", b.size_curve.eval(x));
        }
        let (none, full) = (b.pressure_factors(0.0).0, b.pressure_factors(1.0).0);
        assert!(near(none, 0.3) && near(full, 1.0), "{none} {full}");
        assert!(near(b.setting("jitter_size"), 0.4));
        // no pressure on the opacity; leaning fades it to 20 % and chance (the flow's) takes a
        // tenth: tilt scales it before it is eased, so by its root; speed is left out
        assert_eq!(b.pressure_opacity, 0.0);
        let kept = ["tilt_opacity", "speed_opacity", "jitter_opacity"].map(|k| b.setting(k));
        let root = |v: f32| 1.0 - v.powf(1.0 / procraft_engine::brush::FLOW_POWER);
        assert!(near(kept[0], root(0.2)) && kept[1] == 0.0 && near(kept[2], 0.1), "{kept:?}");
        // pressure on the opacity: a stamp lays what the curve says once eased
        let pressed = csp_brush(&CspBrush {
            pen: CspPen {
                opacity: Some(CspDynamics { pressure: Some(bent.clone()), ..Default::default() }),
                ..Default::default()
            },
            ..Default::default()
        });
        let laid = |p: f32| pressed.pressure_factors(p).1.powf(procraft_engine::brush::FLOW_POWER);
        assert!(near(laid(0.0), 0.3) && near(laid(1.0), 1.0), "{} {}", laid(0.0), laid(1.0));
        // and between, not only at its ends
        let want = csp_curve(&bent.points, 0.3).eval(0.5);
        assert!((laid(0.5) - want).abs() < 0.01, "{} {want}", laid(0.5));
        // the flow and the roundness follow the pressure all the way, straight
        assert!(near(b.setting("pressure_flow"), 1.0) && near(b.setting("pressure_roundness"), 1.0));
        assert!(b.pressure_curves.get(PressureCurve::Flow).is_some());
        assert!(b.pressure_curves.get(PressureCurve::Roundness).is_some());
        // the texture shows from 40 % at no pressure to all of it at full
        assert!(near(b.pressure_curves.at(PressureCurve::GrainDepth, 0.0), 0.4));
        assert!(near(b.pressure_curves.at(PressureCurve::GrainDepth, 1.0), 1.0));
        // 128 px in (a quarter of a classic taper's span), no end, thinning to nothing and half
        // fading, for a pen and a finger alike
        for t in [&b.pen_taper, &b.touch_taper] {
            assert_eq!((t.start, t.end, t.size, t.opacity), (0.25, 0.0, 1.0, 0.5));
        }
        assert_eq!(b.setting("classic_taper"), 1.0);
        // a size the file has, that no pressure drives, doesn't follow it; nothing said, ours stays
        let flat =
            csp_brush(&CspBrush { pen: CspPen { size: Some(CspDynamics::default()), ..Default::default() }, ..c });
        assert_eq!(flat.pressure_size, 0.0);
        assert_eq!(csp_brush(&CspBrush::default()).pressure_size, Brush::round_pen().pressure_size);
    }

    #[test]
    fn a_sub_tools_curve_is_joined_without_swinging_past_its_points() {
        use procraft_engine::geom::Vec2;
        // a real pen's flow: from 30 % up to 75 %, steeply in its last few hundredths (a natural
        // spline through the three swings below nothing in the middle)
        let flow = [(0.0, 0.301), (0.967, 0.62), (1.0, 0.748)];
        let ours = csp_curve(&flow, 0.0);
        let mut was = 0.0f32;
        for i in 0..=100 {
            let y = ours.eval(i as f32 / 100.0);
            assert!((0.29..=0.75).contains(&y) && y >= was - 1e-3, "{i}: {y}");
            was = y;
        }
        for (x, y) in flow {
            assert!((ours.eval(x) - y).abs() < 0.02, "{x}: {}", ours.eval(x));
        }
        // one that rises and falls back stays between its points, level where it turns
        let hump = csp_curve(&[(0.0, 0.534), (0.039, 0.62), (0.336, 0.675), (1.0, 0.552)], 0.0);
        assert!((0..=100).all(|i| (0.53..=0.68).contains(&hump.eval(i as f32 / 100.0))));
        // a gentle one keeps its own few points, for editing; scaled from what it leaves at none
        let gentle = csp_curve(&[(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)], 0.5);
        assert_eq!(gentle.points(), [Vec2::new(0.0, 0.5), Vec2::new(0.5, 0.7), Vec2::new(1.0, 1.0)]);
    }

    #[test]
    fn a_sub_tools_pictures_lay_their_dark_ink_or_else_their_coverage() {
        use procraft_engine::csp::{CspBrush, CspTexture};
        let picture = |f: &dyn Fn(usize, usize) -> [u8; 4]| {
            let px: Vec<u8> = (0..16 * 16).flat_map(|i| f(i % 16, i / 16)).collect();
            crate::project::rgba_png(16, 16, &px)
        };
        // a material's ink: black, opaque in its left half, clear in its right
        let ink = picture(&|x, _| [0, 0, 0, if x < 8 { 255 } else { 0 }]);
        let texture =
            |pngs: Vec<Vec<u8>>, reverse: bool| CspTexture { pngs, density: Some(1.0), reverse, ..Default::default() };
        let c =
            CspBrush { name: "Pencil".into(), texture: Some(texture(vec![ink.clone()], false)), ..Default::default() };
        let n = TEXTURE_SIZE;
        // (a row across the middle, left and right)
        let sides = |s: &Source| (s.pixels[n * n / 2 + n / 8], s.pixels[n * n / 2 + n - n / 8]);
        // the paper lets paint through where it is dark, not where it is clear; reversed, the other way
        let (left, right) = sides(csp_brush(&c).grain.source.as_ref().unwrap());
        assert!(left > 200 && right < 50, "{left} {right}");
        let reversed = csp_brush(&CspBrush { texture: Some(texture(vec![ink.clone()], true)), ..c.clone() });
        let (left, right) = sides(reversed.grain.source.as_ref().unwrap());
        assert!(left < 50 && right > 200, "{left} {right}");
        // a tip: the dark ink, not the few faint light specks on its clear side
        let specks = picture(&|x, y| match (x < 8, (x + y) % 5 == 0) {
            (true, _) => [0, 0, 0, 255],
            (false, true) => [255, 255, 255, 14],
            (false, false) => [255, 255, 255, 0],
        });
        let tip = csp_brush(&CspBrush { tip_pngs: vec![specks], texture: None, ..c.clone() });
        let (left, right) = sides(tip.shape.source.as_ref().unwrap());
        assert!(left > 200 && right < 50, "{left} {right}");
        // light on clear has next to no dark: its coverage
        let light = picture(&|x, _| [255, 255, 255, if x < 8 { 255 } else { 0 }]);
        let tip = csp_brush(&CspBrush { tip_pngs: vec![light], texture: None, ..c.clone() });
        let (left, right) = sides(tip.shape.source.as_ref().unwrap());
        assert!(left > 200 && right < 50, "{left} {right}");
        // a picture on white (all opaque): its dark parts
        let on_white = picture(&|x, _| if x < 8 { [20, 20, 20, 255] } else { [255, 255, 255, 255] });
        let tip = csp_brush(&CspBrush { tip_pngs: vec![on_white], texture: None, ..c });
        let (left, right) = sides(tip.shape.source.as_ref().unwrap());
        assert!(left > 200 && right < 50, "{left} {right}");
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
        // a tip flatter than the app's least comes in at the least, as it is read back from our
        // own file
        let flat = abr_brush(&AbrBrush { roundness: Some(0.04), ..round.clone() });
        assert_eq!(flat.shape.roundness, LEAST_ROUNDNESS);
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
    fn a_brush_file_s_jpeg_is_read_as_its_image_not_taken_for_a_bad_one() {
        // a bright disc on black
        let n = 64usize;
        let disc: Vec<u8> = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f32 - 31.5, (i / n) as f32 - 31.5);
                if x.hypot(y) < 20.0 { 255 } else { 0 }
            })
            .collect();
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
            .encode(&disc, n as u32, n as u32, image::ExtendedColorType::L8)
            .unwrap();
        let (image, source, _) = shape_for(&Texture::Image { bytes, inverted: false });
        assert_eq!(image, None, "not the grainy disc");
        let pixels = source.unwrap().pixels;
        let at = |x: usize, y: usize| pixels[y * TEXTURE_SIZE + x];
        let half = TEXTURE_SIZE / 2;
        assert!(at(half, half) > 240 && at(4, 4) < 16, "{} {}", at(half, half), at(4, 4));
    }

    #[test]
    fn a_brush_file_s_shape_keeps_as_much_of_its_image_as_is_worth_keeping() {
        let source = Source::new(vec![0; TEXTURE_SIZE * TEXTURE_SIZE]).unwrap();
        let file = |n: usize| {
            let bytes = sized_gray_png(n as u32, n as u32, &vec![128; n * n]);
            shape_file_for(&Texture::Image { bytes, inverted: false }, Some(&source))
        };
        // no more than the brush's own image holds, none kept apart
        assert!(file(200).is_none() && file(TEXTURE_SIZE).is_none());
        // a side that isn't a power of two takes the next, as far as the most
        assert_eq!(file(400).unwrap().full.side, 512);
        assert_eq!(file(1500).unwrap().full.side, FULL_GRAIN_MOST as u32);
        assert_eq!(file(400).unwrap().source, source.id);
        // nothing without an image in the file
        let stock = Texture::Stock { name: "Acrylic-Stamp-5.png".into(), inverted: false };
        assert!(shape_file_for(&stock, Some(&source)).is_none());
    }

    #[test]
    fn a_brush_file_s_grain_keeps_its_width_and_as_much_of_its_image_as_is_worth_keeping() {
        let source = Source::new(vec![0; TEXTURE_SIZE * TEXTURE_SIZE]).unwrap();
        let file = |n: usize| {
            let bytes = sized_gray_png(n as u32, n as u32, &vec![128; n * n]);
            grain_file_for(&Texture::Image { bytes, inverted: false }, Some(&source)).unwrap()
        };
        // no more than the brush's own image holds, none kept apart
        let small = file(200);
        assert_eq!((small.width, small.full.is_none()), (200, true));
        // a side that isn't a power of two takes the next, as far as the most
        let middle = file(400);
        assert_eq!((middle.width, middle.full.unwrap().side), (400, 512));
        let large = file(1500);
        assert_eq!((large.width, large.full.unwrap().side), (1500, FULL_GRAIN_MOST as u32));
        // nothing without an image in the file
        let stock = Texture::Stock { name: "Paper".into(), inverted: false };
        assert!(grain_file_for(&stock, Some(&source)).is_none());
    }

    #[test]
    fn folder_ids_look_like_the_files_own() {
        let id = folder_id(42, 3);
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
        assert_ne!(id, folder_id(42, 4));
    }
}
