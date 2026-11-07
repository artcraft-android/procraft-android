//! The layered artwork package matched 1:1: a zip holding a keyed archive of the document
//! (`Document.archive`), a folder of LZ4 tiles per layer named by the layer's UUID, and a preview
//! (`QuickLook/Thumbnail.png`).
//!
//! Tiles are `tileSize` squares (smaller along the right and top edges), rows stored bottom-up,
//! pixels premultiplied RGBA (masks: one grey byte). A missing tile is empty. The canvas is stored
//! as the screen held upright stores it; `orientation` (1 to 4) says which way the screen was
//! turned when it was made, so how it is turned to be shown (see `shown_turns`).
//!
//! The time-lapse is kept as numbered video segments (`video/segments/segment-N.mp4`, played in
//! order), described by the archive's segment info and its document's video settings.

use std::sync::Arc;

use crate::applz4;
use crate::cms::{CanvasIcc, IccProfile, Space};
use crate::document::{BlendMode, Document, Layer, LayerId, LayerKind, LayerText, TextAlign};
use crate::guide::{Guide, GuideKind, LengthUnit, SymmetryKind};
use crate::keyed::{Archive, Builder, Object};
use crate::plist::Value;
use crate::profile::Profile;
use crate::reference_window::{ReferenceKind, ReferenceWindow};
use crate::zip::{Ready, ZipReader, ZipWriter};

/// The file extension of the package.
pub const EXTENSION: &str = "procreate";
/// The edge of a tile written.
const TILE: u32 = 256;

/// The names the video settings are kept by: quality Low, Good, Studio and Lossless; resolution
/// 1080p, 2K and 4K.
const QUALITY_NAMES: [&str; 4] = ["pro50.vqLow", "pro50.vqGood", "pro50.vqStudio", "pro50.vqLossless"];
const RESOLUTION_NAMES: [&str; 3] = ["pro50.vr1080", "pro50.vr2K", "pro50.vr4K"];
/// Where the segments are, and what they are named.
const SEGMENT_PREFIX: &str = "video/segments/segment-";
const SEGMENT_EXTENSIONS: [&str; 3] = ["mp4", "mov", "m4v"];

/// A canvas's time-lapse as a package keeps it: whether it records, its video's settings, and the
/// video recorded so far as segments (MP4s, in order), `frame_size` at `fps`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeLapse {
    pub recording: bool,
    /// Low, Good, Studio or Lossless (0 to 3).
    pub quality: u8,
    /// 1080p, 2K or 4K (0 to 2).
    pub resolution: u8,
    /// HEVC, or H.264.
    pub hevc: bool,
    pub segments: Vec<Vec<u8>>,
    pub frame_size: (u32, u32),
    pub fps: u32,
    /// Bits a second.
    pub bitrate: u64,
}

impl Default for TimeLapse {
    fn default() -> TimeLapse {
        TimeLapse {
            recording: true,
            quality: 1,
            resolution: 0,
            hevc: false,
            segments: Vec::new(),
            frame_size: (0, 0),
            fps: 30,
            bitrate: 0,
        }
    }
}

/// An artwork read from a package.
pub struct Imported {
    pub document: Document,
    /// Paint layers' pixels (premultiplied RGBA) and masks' (grey in every channel, opaque), by id.
    pub pixels: Vec<(LayerId, Vec<u8>)>,
    pub name: String,
    pub author: String,
    pub strokes: u64,
    pub tracked_seconds: f64,
    /// The Reference window as the artwork left it.
    pub reference: ReferenceWindow,
    /// Its imported picture (a PNG, as stored), and the turn it is shown at (one of the system's
    /// eight image orientations; 0 is upright).
    pub reference_picture: Option<(Vec<u8>, i64)>,
}

/// Where a package keeps the Reference window's picture (some writers put it at the top).
const REFERENCE_PICTURE: &str = "ReferenceImage/ReferenceImage.png";

/// Why a package couldn't be read.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    NotAPackage,
    Damaged(&'static str),
    TooLarge,
}

/// Reads a package.
pub fn read(bytes: &[u8]) -> Result<Imported, ReadError> {
    let zip = ZipReader::new(bytes).ok_or(ReadError::NotAPackage)?;
    let archive = Archive::read(&zip.read("Document.archive").ok_or(ReadError::NotAPackage)?)
        .ok_or(ReadError::Damaged("document"))?;
    let root = archive.root().ok_or(ReadError::Damaged("document"))?;
    let (w, h) = root.string("size").and_then(|s| parse_size(&s)).ok_or(ReadError::Damaged("size"))?;
    if w == 0 || h == 0 {
        return Err(ReadError::Damaged("size"));
    }
    if w > crate::document::MAX_CANVAS_EDGE || h > crate::document::MAX_CANVAS_EDGE {
        return Err(ReadError::TooLarge);
    }
    let tile =
        root.int("tileSize").and_then(|t| u32::try_from(t).ok()).filter(|t| (16..=4096).contains(t)).unwrap_or(TILE);
    // the canvas's profile: a built-in one by its name, else the nearest (a CMYK canvas keeps its
    // pixels in Display P3; an RGB profile of its own keeps them in it)
    let kept = root.object("colorProfile");
    let profile = kept.as_ref().and_then(|p| p.string("SiColorProfileArchiveICCNameKey"));
    let icc = kept.as_ref().and_then(|p| p.data("SiColorProfileArchiveICCDataKey")).and_then(IccProfile::parse);
    let nearest = match profile.as_deref() {
        Some(p) if p.contains("P3") => Profile::DisplayP3,
        _ => Profile::Srgb,
    };
    let (profile, icc) = match (profile.as_deref().and_then(Profile::from_name), icc) {
        (Some(builtin), _) => (builtin, None),
        (None, Some(icc)) if icc.space == Space::Cmyk => (Profile::DisplayP3, Some(icc)),
        (None, Some(icc)) if icc.space == Space::Rgb => (nearest, Some(icc)),
        _ => (nearest, None),
    };
    let turns = shown_turns(root.int("orientation").unwrap_or(1));
    let flip_x = root.bool("flippedHorizontally").unwrap_or(false);
    let flip_y = root.bool("flippedVertically").unwrap_or(false);
    let (dw, dh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let canvas = Canvas { zip: &zip, w, h, tile, turns, flip_x, flip_y, profile };

    let mut layers: Vec<Layer> = Vec::new();
    let mut pixels: Vec<(LayerId, Vec<u8>)> = Vec::new();
    let mut next = 1u64;
    let mut holds = Vec::new();
    // each layer's UUID, for the layers the document points at (the selected, the sampled)
    let mut uuids: Vec<(String, LayerId)> = Vec::new();
    let mut items = root.array("unwrappedLayers");
    if items.is_empty() {
        items = root.array("layers");
    }
    // the archive lists the top first; ours goes bottom first
    for item in items.iter().rev() {
        add_item(&canvas, item, None, &mut layers, &mut pixels, &mut holds, &mut next, &mut uuids)?;
    }
    let pointed = |key: &str| {
        let uuid = root.object(key).and_then(|l| l.string("UUID"))?;
        uuids.iter().find(|(u, _)| *u == uuid).map(|(_, id)| *id)
    };
    let mut document = Document::from_layers(dw, dh, layers, pointed("selectedLayer").unwrap_or(LayerId(1)));
    // the layer sampled from is the reference layer
    if let Some(sampled) = pointed("selectedSamplerLayer") {
        document.set_reference(sampled, true);
    }
    if let Some(c) = root.data("backgroundColor").and_then(floats4) {
        document.background = [c[0], c[1], c[2]].map(|v| v.clamp(0.0, 1.0));
    }
    document.background_visible = !root.bool("backgroundHidden").unwrap_or(false);
    // the last text's look, for new text
    if let Some((_, attrs)) = root.object("lastTextStyling").as_ref().and_then(attributed) {
        document.text_style = Some(text_look(&canvas, &attrs).look());
    }
    // a drawing guide kept means it has been on: the layers' menus offer Drawing Assist
    if let Some(g) = root.object("drawingguide") {
        read_guide(&g, &canvas, &mut document.guide);
    }
    if let Some(dpi) = root.real("SilicaDocumentArchiveDPIKey").filter(|d| d.is_finite() && *d > 0.0) {
        document.dpi = dpi as f32;
    }
    document.profile = profile;
    document.icc = icc.map(|icc| CanvasIcc(Arc::new(icc)));
    if let Some(mut a) = root.object("animation").and_then(|a| animation(&a, holds)) {
        // the layers are listed top first: the first item is the last frame, the last the first
        a.foreground = root.bool("isFirstItemAnimationForeground").unwrap_or(false);
        a.background = root.bool("isLastItemAnimationBackground").unwrap_or(false);
        document.animation = Some(a);
    }
    let document = document.repaired();
    let reference_picture = zip
        .read(REFERENCE_PICTURE)
        .or_else(|| zip.read("ReferenceImage.png"))
        .map(|png| (png, root.int("referenceOrientation").unwrap_or(0)));
    Ok(Imported {
        document,
        pixels,
        reference: read_reference(&root),
        reference_picture,
        name: root.string("name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Untitled Artwork".to_owned()),
        author: root.string("authorName").unwrap_or_default(),
        strokes: root.int("strokeCount").and_then(|s| u64::try_from(s).ok()).unwrap_or(0),
        tracked_seconds: root
            .real("SilicaDocumentTrackedTimeKey")
            .filter(|t| t.is_finite() && *t >= 0.0)
            .unwrap_or(0.0),
    })
}

/// The canvas's animation settings, if Animation or Page Assist is on: older files say whether
/// with `enabled` (Animation Assist), newer ones with `animationMode` (1 Animation, 2 Page Assist).
/// `holds` are the frames held longer.
fn animation(a: &Object<'_>, holds: Vec<(LayerId, u32)>) -> Option<crate::animation::Animation> {
    use crate::animation::{Animation, MAX_ONION_FRAMES, Playback};
    let mode = a.bool("enabled").map_or_else(|| a.int("animationMode").unwrap_or(0), i64::from);
    if mode != 1 && mode != 2 {
        return None;
    }
    let d = Animation::default();
    let (before, after) =
        if a.bool("coloredSkins").unwrap_or(false) { ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]) } else { (d.before, d.after) };
    Some(Animation {
        fps: a.int("frameRate").and_then(|f| u32::try_from(f).ok()).filter(|f| (1..=60).contains(f)).unwrap_or(d.fps),
        // one when the file doesn't say
        onion_frames: a.int("onionSkinCount").map_or(1, |n| u32::try_from(n.max(0)).unwrap_or(0).min(MAX_ONION_FRAMES)),
        onion_opacity: a
            .real("onionSkinOpacity")
            .filter(|o| o.is_finite())
            .map_or(d.onion_opacity, |o| o.clamp(0.0, 1.0) as f32),
        playback: match a.int("playbackMode") {
            Some(0) => Playback::OneShot,
            Some(2) => Playback::PingPong,
            _ => Playback::Loop,
        },
        blend_primary: a.bool("primaryMixed").unwrap_or(d.blend_primary),
        before,
        after,
        holds,
        pages: mode == 2,
        ..d
    })
}

/// "{w, h}" → (w, h).
fn parse_size(s: &str) -> Option<(u32, u32)> {
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?;
    let (a, b) = inner.split_once(',')?;
    let w = a.trim().parse::<f64>().ok()?;
    let h = b.trim().parse::<f64>().ok()?;
    (w.is_finite() && h.is_finite() && w >= 0.0 && h >= 0.0).then_some((w.round() as u32, h.round() as u32))
}

/// A point kept as "{x, y}".
fn parse_point(s: &str) -> Option<[f64; 2]> {
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?;
    let (a, b) = inner.split_once(',')?;
    let p = [a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?];
    p.iter().all(|v| v.is_finite()).then_some(p)
}

/// The kinds of guide in the archive's numbering (and the looks' order).
const GUIDE_KINDS: [GuideKind; 4] =
    [GuideKind::Grid, GuideKind::Isometric, GuideKind::Perspective, GuideKind::Symmetry];
const SYMMETRIES: [SymmetryKind; 4] =
    [SymmetryKind::Vertical, SymmetryKind::Horizontal, SymmetryKind::Quadrant, SymmetryKind::Radial];
/// The units a grid's size is shown in, in the archive's numbering.
const UNITS: [LengthUnit; 4] =
    [LengthUnit::Pixels, LengthUnit::Inches, LengthUnit::Centimetres, LengthUnit::Millimetres];
/// The vanishing points' colours, as kept with every kind.
const NODE_COLOURS: [[f32; 4]; 3] =
    [[0.106, 0.816, 0.698, 1.0], [0.969, 0.490, 0.999, 1.0], [0.088, 0.669, 0.778, 1.0]];

/// The document's drawing guide as the archive keeps it (`drawingguide`, there once the guide has
/// been on), into `guide`: on or not, which kind, each kind's look (its grid's size a share of the
/// canvas's longer side, and the unit it is shown in), the symmetry, the centre and turn (the shown kind's, else the grid's:
/// a centre and a handle straight up from it unturned), perspective's vanishing points. Its points
/// are kept y up on the canvas as stored, placed here where they show.
fn read_guide(g: &Object<'_>, canvas: &Canvas<'_>, guide: &mut Guide) {
    guide.set_up = true;
    guide.on = g.bool("active").unwrap_or(false);
    let pick = |n: Option<i64>| usize::try_from(n.unwrap_or(0)).ok().filter(|n| *n < 4).unwrap_or(0);
    let kind = pick(g.int("type"));
    guide.kind = GUIDE_KINDS.get(kind).copied().unwrap_or_default();
    guide.symmetry = SYMMETRIES.get(pick(g.int("symmetryType"))).copied().unwrap_or_default();
    guide.rotational = !g.bool("mirrored").unwrap_or(true);
    let long = f64::from(canvas.w.max(canvas.h));
    for (k, look) in guide.looks.iter_mut().enumerate() {
        if let Some(o) = g.real(&format!("opacity{k}")) {
            look.opacity = (o as f32).clamp(0.0, 1.0);
        }
        if let Some(t) = g.real(&format!("thickness{k}")) {
            look.thickness = (t as f32).clamp(0.0, 1.0);
        }
        if let Some(z) = g.real(&format!("zoom{k}")).filter(|z| z.is_finite() && *z > 0.0) {
            look.size = (z * long) as f32;
        }
        if let Some(c) = g.data(&format!("color{k}")).and_then(floats4) {
            look.color = crate::guide::bar_place([c[0], c[1], c[2]]);
        }
        if let Some(unit) = g.int(&format!("unit{k}")).and_then(|u| usize::try_from(u).ok()).and_then(|u| UNITS.get(u))
        {
            look.unit = *unit;
        }
    }
    let h = f64::from(canvas.h);
    let points = |k: usize| -> Vec<[f32; 2]> {
        g.array(&format!("nodes{k}"))
            .iter()
            .filter_map(|n| parse_point(&n.string("NS.pointval")?))
            .map(|[x, y]| canvas.shown([x, h - y]).map(|v| v as f32))
            .collect()
    };
    let shown = if guide.kind == GuideKind::Symmetry { points(3) } else { points(0) };
    let centred = if shown.len() >= 2 { shown } else { points(0) };
    if let [centre, handle, ..] = centred[..] {
        guide.centre = centre;
        let turned = (handle[1] - centre[1]).atan2(handle[0] - centre[0]) + std::f32::consts::FRAC_PI_2;
        guide.angle = (turned + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    }
    let vanishing = points(2);
    if !vanishing.is_empty() {
        guide.vanishing = vanishing.into_iter().take(3).collect();
    }
}

/// The drawing guide as the archive keeps it (see [`read_guide`]), on a canvas `w` × `h` kept
/// upright: points y up, the handle half the shorter side straight up from the centre, turned.
fn guide_object(b: &mut Builder, g: &Guide, w: u32, h: u32) -> Value {
    let (wf, hf) = (f64::from(w), f64::from(h));
    let long = wf.max(hf);
    let kind = GUIDE_KINDS.iter().position(|k| *k == g.kind).unwrap_or(0);
    let symmetry = SYMMETRIES.iter().position(|s| *s == g.symmetry).unwrap_or(0);
    let point = |b: &mut Builder, [x, y]: [f64; 2]| {
        let at = b.string(&format!("{{{x}, {}}}", hf - y));
        b.object(&["NSValue", "NSObject"], vec![("NS.pointval", at), ("NS.special", Value::Int(1))])
    };
    let [cx, cy] = g.centre.map(f64::from);
    let reach = wf.min(hf) / 2.0;
    let (sin, cos) = f64::from(g.angle).sin_cos();
    let handle = [cx + reach * sin, cy - reach * cos];
    let size = b.string(&format!("{{{w}, {h}}}"));
    let mut fields: Vec<(String, Value)> = vec![
        ("active".into(), Value::Bool(g.on)),
        ("type".into(), Value::Int(kind as i64)),
        ("symmetryType".into(), Value::Int(symmetry as i64)),
        ("mirrored".into(), Value::Bool(!g.rotational)),
        ("horizonAngle".into(), Value::Real(0.0)),
        ("portrait".into(), Value::Bool(false)),
        ("size".into(), size),
    ];
    for (k, look) in g.looks.iter().enumerate() {
        let [r, gr, bl] = crate::guide::bar_color(look.color);
        let colour: Vec<u8> = [r, gr, bl, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let colour = b.add(Value::Data(colour));
        // the grids' sizes; the others have none
        let zoom = if k < 2 && long > 0.0 { f64::from(look.size) / long } else { 0.0 };
        let nodes: Vec<Value> = match GUIDE_KINDS.get(k) {
            Some(GuideKind::Grid | GuideKind::Symmetry) => vec![point(b, [cx, cy]), point(b, handle)],
            Some(GuideKind::Perspective) => g.vanishing.iter().map(|v| point(b, v.map(f64::from))).collect(),
            _ => Vec::new(),
        };
        let nodes = b.array(nodes, false);
        let node_colours: Vec<Value> = NODE_COLOURS
            .iter()
            .map(|c| {
                let bytes: Vec<u8> = c.iter().flat_map(|v| v.to_le_bytes()).collect();
                b.add(Value::Data(bytes))
            })
            .collect();
        let node_colours = b.array(node_colours, false);
        fields.extend([
            (format!("color{k}"), colour),
            (format!("opacity{k}"), Value::Real(f64::from(look.opacity))),
            (format!("thickness{k}"), Value::Real(f64::from(look.thickness))),
            (format!("zoom{k}"), Value::Real(zoom)),
            (format!("unit{k}"), Value::Int(UNITS.iter().position(|u| *u == look.unit).unwrap_or(0) as i64)),
            (format!("nodes{k}"), nodes),
            (format!("nodeColors{k}"), node_colours),
        ]);
    }
    let fields: Vec<(&str, Value)> = fields.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    b.object(&["ValkyrieDrawingGuide", "NSObject"], fields)
}

/// Four little-endian floats.
fn floats4(d: &[u8]) -> Option<[f32; 4]> {
    let (chunks, _) = d.as_chunks::<4>();
    let v: Vec<f32> = chunks.iter().take(4).map(|c| f32::from_le_bytes(*c)).collect();
    Some([*v.first()?, *v.get(1)?, *v.get(2)?, *v.get(3)?])
}

/// What reading a layer's pixels needs: the package and how the canvas is stored and shown.
struct Canvas<'a> {
    zip: &'a ZipReader<'a>,
    w: u32,
    h: u32,
    tile: u32,
    turns: u32,
    flip_x: bool,
    flip_y: bool,
    /// The canvas's profile, colours read into it.
    profile: Profile,
}

impl Canvas<'_> {
    /// Point `p` of the canvas as kept (its first row first) where it is shown: turned, then
    /// mirrored, as [`orient`] turns its pixels.
    fn shown(&self, p: [f64; 2]) -> [f64; 2] {
        let (w, h) = (f64::from(self.w), f64::from(self.h));
        let [x, y] = match self.turns % 4 {
            1 => [h - p[1], p[0]],
            2 => [w - p[0], h - p[1]],
            3 => [p[1], w - p[0]],
            _ => p,
        };
        let (dw, dh) = if self.turns % 2 == 1 { (h, w) } else { (w, h) };
        [if self.flip_x { dw - x } else { x }, if self.flip_y { dh - y } else { y }]
    }

    /// An sRGB colour (encoded; past 0..1 for one outside sRGB) in the canvas's profile.
    fn profile_from_srgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        Profile::Srgb.convert(rgb, self.profile).map(|c| c.clamp(0.0, 1.0))
    }

    /// A layer's pixels, `bpp` bytes each, top row first, turned as the canvas is shown; where no
    /// tile was stored, every byte is `blank`. Tiles are LZ4 (`.lz4`) or, in packages from the
    /// older app (and their layers not saved since), LZO (`.chunk`); where both are, the LZ4 one.
    fn pixels(&self, uuid: &str, bpp: usize, blank: u8) -> Result<Vec<u8>, ReadError> {
        let (w, h, t) = (self.w as usize, self.h as usize, self.tile as usize);
        let mut out = vec![blank; w * h * bpp];
        let prefix = format!("{uuid}/");
        // the tiles on the canvas, where they go, and whether each is LZO
        let mut tiles: Vec<(String, usize, usize, bool)> = self
            .zip
            .names()
            .filter_map(|name| {
                let rest = name.strip_prefix(&prefix)?;
                let (at, lzo) = match rest.strip_suffix(".lz4") {
                    Some(at) => (at, false),
                    None => (rest.strip_suffix(".chunk")?, true),
                };
                let (tx, ty) =
                    at.split_once('~').and_then(|(x, y)| Some((x.parse::<usize>().ok()?, y.parse::<usize>().ok()?)))?;
                (tx * t < w && ty * t < h).then(|| (name.to_owned(), tx, ty, lzo))
            })
            .collect();
        tiles.sort_by_key(|&(_, tx, ty, lzo)| (tx, ty, lzo));
        tiles.dedup_by_key(|&mut (_, tx, ty, _)| (tx, ty));
        // unpacked side by side, then put in place
        let unpack = |(name, tx, ty, lzo): &(String, usize, usize, bool)| -> Result<Vec<u8>, ReadError> {
            let (tw, th) = (t.min(w - tx * t), t.min(h - ty * t));
            let packed = self.zip.read(name).ok_or(ReadError::Damaged("tile"))?;
            let raw =
                if *lzo { crate::lzo::decode(&packed, tw * th * bpp) } else { applz4::decode(&packed, tw * th * bpp) }
                    .ok_or(ReadError::Damaged("tile"))?;
            if raw.len() != tw * th * bpp {
                return Err(ReadError::Damaged("tile size"));
            }
            Ok(raw)
        };
        let unpacked = crate::par::side_by_side(&tiles, unpack);
        for ((_, tx, ty, _), raw) in tiles.iter().zip(unpacked) {
            let raw = raw.ok_or(ReadError::Damaged("tile"))??;
            let (tx, ty) = (*tx, *ty);
            let (tw, th) = (t.min(w - tx * t), t.min(h - ty * t));
            for r in 0..th {
                // rows are stored bottom-up
                let row = h - 1 - (ty * t + r);
                let src = raw.get(r * tw * bpp..(r + 1) * tw * bpp).ok_or(ReadError::Damaged("tile"))?;
                let at = (row * w + tx * t) * bpp;
                out.get_mut(at..at + tw * bpp).ok_or(ReadError::Damaged("tile"))?.copy_from_slice(src);
            }
        }
        Ok(orient(out, w, h, bpp, self.turns, self.flip_x, self.flip_y))
    }
}

/// The quarter turns clockwise a canvas made with the screen at `orientation` is shown at: 1
/// upright, 2 upside down, 3 and 4 on its side one way and the other (as measured on files made
/// each way: a mark at the top left of a canvas made at 3 is stored at the top right).
fn shown_turns(orientation: i64) -> u32 {
    match orientation {
        2 => 2,
        3 => 3,
        4 => 1,
        _ => 0,
    }
}

/// A text layer's text (`text`), its pixels kept as they came: the words with the first run's
/// look (font, size, colour, alignment) for all of them, its box placed as shown. None if the
/// layer has none, or its box is turned or scaled on the canvas (it stays its pixels alone).
fn layer_text(canvas: &Canvas<'_>, item: &Object<'_>) -> Option<LayerText> {
    let t = item.object("text")?;
    let (words, attrs) = attributed(&t.object("attributedString")?)?;
    // the box's own space (x right, y up, from its top left) on the canvas as kept, y up
    let m: Vec<f64> = t
        .string("transform")?
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(|v| v.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    let [a, b, c, d, tx, ty] = <[f64; 6]>::try_from(m).ok()?;
    let h = f64::from(canvas.h);
    let on_canvas = |x: f64, y: f64| canvas.shown([a * x + c * y + tx, h - (b * x + d * y + ty)]);
    // shown unturned and unscaled: one along the box goes one right, one down it one down
    let (origin, right, down) = (on_canvas(0.0, 0.0), on_canvas(1.0, 0.0), on_canvas(0.0, -1.0));
    let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < 1e-3 && (p[1] - q[1]).abs() < 1e-3;
    if !near(right, [origin[0] + 1.0, origin[1]]) || !near(down, [origin[0], origin[1] + 1.0]) {
        return None;
    }
    Some(
        LayerText {
            text: words,
            origin: origin.map(|v| v as f32),
            width: t.real("width").unwrap_or(0.0) as f32,
            ..text_look(canvas, &attrs)
        }
        .sanitized(),
    )
}

/// An attributed string's words and its attributes: one run's, or the first of several (each a
/// dictionary).
fn attributed<'a>(s: &Object<'a>) -> Option<(String, Object<'a>)> {
    let words = s.string("NSString")?;
    let runs = s.object("NSAttributes")?;
    let attrs = if runs.raw("NS.keys").is_some() { runs } else { s.array("NSAttributes").into_iter().next()? };
    Some((words, attrs))
}

/// A run of text's look from its attributes: the typeface, size, colour (into the canvas's
/// profile) and opacity, alignment, and whether it runs down.
fn text_look(canvas: &Canvas<'_>, attrs: &Object<'_>) -> LayerText {
    // the typeface by its PostScript name: "HelveticaNeue-Medium" is Helvetica Neue, Medium (the
    // style after the last hyphen)
    let postscript = attrs.entry("_CFN").and_then(|e| e.string()).unwrap_or_default();
    let (family, style) = postscript.rsplit_once('-').unwrap_or((&postscript, ""));
    let spaced = |name: &str| {
        let mut out = String::new();
        let mut before: Option<char> = None;
        for ch in name.chars() {
            if ch.is_uppercase() && before.is_some_and(char::is_lowercase) {
                out.push(' ');
            }
            out.push(ch);
            before = Some(ch);
        }
        out
    };
    let color = attrs.entry("NSColor").and_then(|e| e.object());
    let channel = |k: &str| color.and_then(|c| c.real(k)).unwrap_or(0.0) as f32;
    // in sRGB, past its range for colours outside it
    let rgb = canvas.profile_from_srgb([channel("UIRed"), channel("UIGreen"), channel("UIBlue")]);
    // the paragraph's alignment as archives number it: left, right, centre, justified, natural
    let align = match attrs.entry("NSParagraphStyle").and_then(|e| e.object()).and_then(|p| p.int("NSAlignment")) {
        Some(1) => TextAlign::Right,
        Some(2) => TextAlign::Center,
        Some(3) => TextAlign::Justify,
        _ => TextAlign::Left,
    };
    let size = attrs.entry("_CFZ").and_then(|e| e.real()).unwrap_or(64.0) as f32;
    LayerText {
        font: spaced(family),
        style: if style.is_empty() { "Regular".to_owned() } else { spaced(style) },
        size,
        opacity: color.and_then(|c| c.real("UIAlpha")).unwrap_or(1.0) as f32,
        align,
        vertical: attrs.entry("CTVerticalForms").and_then(|e| e.int()).is_some_and(|v| v != 0),
        color: rgb,
        ..LayerText::default()
    }
}

/// `src` (`w` × `h`, `bpp` bytes a pixel, top row first) turned `turns` quarter turns clockwise,
/// then mirrored.
fn orient(src: Vec<u8>, w: usize, h: usize, bpp: usize, turns: u32, flip_x: bool, flip_y: bool) -> Vec<u8> {
    if turns == 0 && !flip_x && !flip_y {
        return src;
    }
    let (dw, dh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; src.len()];
    for y in 0..dh {
        for x in 0..dw {
            let (mx, my) = (if flip_x { dw - 1 - x } else { x }, if flip_y { dh - 1 - y } else { y });
            // the source pixel shown at (mx, my) after the turn
            let (sx, sy) = match turns % 4 {
                1 => (my, h - 1 - mx),
                2 => (w - 1 - mx, h - 1 - my),
                3 => (w - 1 - my, mx),
                _ => (mx, my),
            };
            let (s, d) = ((sy * w + sx) * bpp, (y * dw + x) * bpp);
            if let (Some(from), Some(to)) = (src.get(s..s + bpp), out.get_mut(d..d + bpp)) {
                to.copy_from_slice(from);
            }
        }
    }
    out
}

/// Adds an archived item (a layer with its mask, or a group with what it holds, top first) to
/// `layers`, bottom first.
#[allow(clippy::too_many_arguments)]
fn add_item(
    canvas: &Canvas<'_>,
    item: &Object<'_>,
    parent: Option<LayerId>,
    layers: &mut Vec<Layer>,
    pixels: &mut Vec<(LayerId, Vec<u8>)>,
    holds: &mut Vec<(LayerId, u32)>,
    next: &mut u64,
    uuids: &mut Vec<(String, LayerId)>,
) -> Result<(), ReadError> {
    if layers.len() > 4096 {
        return Err(ReadError::TooLarge);
    }
    let mut id = || {
        let i = LayerId(*next);
        *next += 1;
        i
    };
    let held = item.int("animationHeldLength").and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0);
    match item.class() {
        Some("SilicaGroup") | Some("ValkyrieGroup") => {
            let gid = id();
            holds.extend(held.map(|n| (gid, n)));
            // the group sits above what it holds
            for child in item.array("children").iter().rev() {
                add_item(canvas, child, Some(gid), layers, pixels, holds, next, uuids)?;
            }
            layers.push(Layer {
                id: gid,
                name: item.string("name").unwrap_or_else(|| "New group".to_owned()),
                visible: !item.bool("isHidden").unwrap_or(false),
                opacity: item.real("opacity").unwrap_or(1.0) as f32,
                blend: BlendMode::Normal,
                alpha_lock: false,
                clipped: item.bool("isClipped").unwrap_or(false),
                locked: item.bool("isLocked").unwrap_or(false),
                reference: false,
                kind: LayerKind::Group,
                parent,
                collapsed: item.bool("isCollapsed").unwrap_or(false),
                text: None,
                private: false,
                assisted: false,
            });
        }
        _ => {
            let lid = id();
            holds.extend(held.map(|n| (lid, n)));
            let uuid = item.string("UUID").unwrap_or_default();
            uuids.push((uuid.clone(), lid));
            let blend = item
                .int("extendedBlend2")
                .or_else(|| item.int("extendedBlend"))
                .or_else(|| item.int("blend"))
                .unwrap_or(0);
            let text = layer_text(canvas, item);
            // a text layer without a name of its own is named after its text, as ours are
            let name = item.string("name").or_else(|| text.as_ref().and_then(LayerText::name));
            layers.push(Layer {
                id: lid,
                name: name.unwrap_or_else(|| "Layer".to_owned()),
                visible: !item.bool("hidden").unwrap_or(false),
                opacity: item.real("opacity").unwrap_or(1.0) as f32,
                blend: blend_from(blend),
                alpha_lock: item.bool("preserve").unwrap_or(false),
                clipped: item.bool("clipped").unwrap_or(false),
                locked: item.bool("locked").unwrap_or(false),
                reference: false,
                kind: LayerKind::Paint,
                parent,
                collapsed: false,
                text,
                private: item.bool("private").unwrap_or(false),
                assisted: item.bool("perspectiveAssisted").unwrap_or(false),
            });
            if !uuid.is_empty() {
                pixels.push((lid, canvas.pixels(&uuid, 4, 0)?));
            }
            // a mask goes just above its layer
            if let Some(mask) = item.object("mask") {
                let mid = id();
                layers.push(Layer {
                    id: mid,
                    name: "Layer Mask".to_owned(),
                    visible: !mask.bool("hidden").unwrap_or(false),
                    opacity: 1.0,
                    blend: BlendMode::Normal,
                    alpha_lock: false,
                    clipped: false,
                    locked: mask.bool("locked").unwrap_or(false),
                    reference: false,
                    kind: LayerKind::Mask,
                    parent: Some(lid),
                    collapsed: false,
                    text: None,
                    private: false,
                    assisted: false,
                });
                if let Some(m) = mask.string("UUID") {
                    // a mask stores only the tiles painted on: the rest shows the layer
                    let grey = canvas.pixels(&m, 1, 255)?;
                    pixels.push((mid, grey.iter().flat_map(|&g| [g, g, g, 255]).collect()));
                }
            }
        }
    }
    Ok(())
}

/// Each blend mode's number in the package (`extendedBlend2`).
const BLENDS: [(i64, BlendMode); 27] = [
    (0, BlendMode::Normal),
    (1, BlendMode::Multiply),
    (2, BlendMode::Screen),
    (3, BlendMode::Add),
    (4, BlendMode::Lighten),
    (5, BlendMode::Exclusion),
    (6, BlendMode::Difference),
    (7, BlendMode::Subtract),
    (8, BlendMode::LinearBurn),
    (9, BlendMode::ColorDodge),
    (10, BlendMode::ColorBurn),
    (11, BlendMode::Overlay),
    (12, BlendMode::HardLight),
    (13, BlendMode::Color),
    (14, BlendMode::Luminosity),
    (15, BlendMode::Hue),
    (16, BlendMode::Saturation),
    (17, BlendMode::SoftLight),
    (19, BlendMode::Shade),
    (20, BlendMode::HardMix),
    (21, BlendMode::VividLight),
    (22, BlendMode::LinearLight),
    (23, BlendMode::PinLight),
    (24, BlendMode::LighterColor),
    (25, BlendMode::DarkerColor),
    (26, BlendMode::Divide),
    (34, BlendMode::Darken),
];

pub(crate) fn blend_from(n: i64) -> BlendMode {
    BLENDS.iter().find(|(id, _)| *id == n).map_or(BlendMode::Normal, |(_, m)| *m)
}

pub(crate) fn blend_to(b: BlendMode) -> i64 {
    BLENDS.iter().find(|(_, m)| *m == b).map_or(0, |(id, _)| *id)
}

/// A blend number as the two older keys hold it (`blend`, `extendedBlend`): the oldest stops at
/// 19, and Darken, renumbered since, is 19 in both.
pub(crate) fn older_blends(id: i64) -> (i64, i64) {
    match id {
        34 => (19, 19),
        _ => (if id <= 19 { id } else { 0 }, id),
    }
}

/// What an export writes besides the document: each paint layer's pixels (premultiplied RGBA)
/// and each mask's (its brightness as grey), the flattened picture (premultiplied, background
/// included) and the preview PNG.
pub struct Export<'a> {
    pub document: &'a Document,
    pub pixels: &'a std::collections::HashMap<LayerId, Vec<u8>>,
    pub composite: &'a [u8],
    pub thumbnail_png: &'a [u8],
    pub name: &'a str,
    pub author: &'a str,
    pub strokes: u64,
    pub tracked_seconds: f64,
    /// Seeds the layers' UUIDs.
    pub seed: u64,
    /// The time-lapse, if the canvas has one.
    pub time_lapse: Option<&'a TimeLapse>,
    /// The Reference window as the artwork left it (none: never used), and its imported picture
    /// (a PNG, upright).
    pub reference: Option<&'a ReferenceWindow>,
    pub reference_picture: Option<&'a [u8]>,
}

/// The Reference window as a package keeps it. The format keeps the window's kind under
/// `referenceAnchor` and the point it is shown at under `referenceKind`: the names are the other
/// way round from what each holds, and are read and written as they are.
fn read_reference(root: &Object<'_>) -> ReferenceWindow {
    let mut w = ReferenceWindow {
        shown: root.bool("referenceVisibility").unwrap_or(false),
        kind: root.int("referenceAnchor").map_or(ReferenceKind::Canvas, ReferenceKind::of_package),
        corner: root.int("referenceCorner").unwrap_or(0),
        docked: root.bool("referenceDocked").unwrap_or(false),
        three_d: root.bool("reference3D").unwrap_or(false),
        face_background_hidden: root.bool("faceBackgroundHidden").unwrap_or(false),
        ..ReferenceWindow::default()
    };
    let pair = |key: &str| root.string(key).and_then(|s| parse_point(&s));
    w.set_from_package(pair("referenceSize"), pair("referenceKind"));
    w
}

/// The Reference window's fields for a package (see [`read_reference`]).
fn reference_fields(b: &mut Builder, w: &ReferenceWindow) -> Vec<(&'static str, Value)> {
    let [sw, sh] = w.package_size();
    let mut fields = vec![
        ("faceBackgroundHidden", Value::Bool(w.face_background_hidden)),
        ("reference3D", Value::Bool(w.three_d)),
        ("referenceAnchor", Value::Int(w.kind.package())),
        ("referenceCorner", Value::Int(w.corner)),
        ("referenceDocked", Value::Bool(w.docked)),
        // the picture is kept upright
        ("referenceOrientation", Value::Int(0)),
        ("referenceSize", b.string(&format!("{{{sw}, {sh}}}"))),
        ("referenceVisibility", Value::Bool(w.shown)),
    ];
    // left out until the window has been placed, as a package that never showed it has it
    if let Some([x, y]) = w.package_middle() {
        fields.push(("referenceKind", b.string(&format!("{{{x}, {y}}}"))));
    }
    fields
}

/// Writes a package.
pub fn write(e: &Export<'_>) -> Vec<u8> {
    let doc = e.document;
    let (w, h) = (doc.width(), doc.height());
    let mut zip = ZipWriter::new();
    let mut b = Builder::new();
    let mut uuids = Uuids(e.seed ^ 0x9E37_79B9_7F4A_7C15);
    let doc_ref = b.reserve();

    // layers and groups, top first, as nested lists
    let tops = top_first(doc, None);
    let (mut selected, mut sampled) = (Builder::null(), Builder::null());
    let mut items = Vec::new();
    for id in tops {
        items.push(write_item(&mut b, &mut zip, &mut uuids, e, &doc_ref, id, [&mut selected, &mut sampled]));
    }
    let layers = b.array(items.clone(), true);
    let unwrapped = b.array(items, false);
    let composite_uuid = uuids.next();
    write_tiles(&mut zip, &composite_uuid, e.composite, w, h, 4, 0);
    let composite = layer_record(&mut b, &doc_ref, &composite_uuid, None, LayerRecord::plain(1), w, h);
    let mask_uuid = uuids.next();
    let mask = layer_record(&mut b, &doc_ref, &mask_uuid, None, LayerRecord::plain(2), w, h);
    let saved_masks = b.array(Vec::new(), true);

    let [r, g, bl] = doc.background;
    let background = b.add(Value::Data([r, g, bl, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect()));
    let hsba = {
        let (hh, s, v) = hsv(r, g, bl);
        b.add(Value::Data([hh, s, v, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect()))
    };
    let icc_name = b.string(&doc.profile_name());
    let icc_data = doc.icc.as_ref().map_or_else(|| crate::icc::icc(doc.profile), |i| i.0.bytes.to_vec());
    let profile = b.object(
        &["ValkyrieColorProfile", "NSObject"],
        vec![("SiColorProfileArchiveICCNameKey", icc_name), ("SiColorProfileArchiveICCDataKey", Value::Data(icc_data))],
    );
    // the canvas's settings, and which assist is on: none 0, Animation Assist 1, Page Assist 2
    let mode = doc.animation.as_ref().map_or(0, |a| if a.pages { 2 } else { 1 });
    let a = doc.animation.clone().unwrap_or_default();
    let playback = match a.playback {
        crate::animation::Playback::OneShot => 0,
        crate::animation::Playback::Loop => 1,
        crate::animation::Playback::PingPong => 2,
    };
    let animation = b.object(
        &["ValkyrieDocumentAnimation", "NSObject"],
        vec![
            ("animationMode", Value::Int(mode)),
            ("frameRate", Value::Int(i64::from(a.fps))),
            ("onionSkinCount", Value::Int(i64::from(a.onion_frames))),
            ("onionSkinOpacity", Value::Real(f64::from(a.onion_opacity))),
            ("playbackDirection", Value::Int(0)),
            ("playbackMode", Value::Int(playback)),
            ("primaryMixed", Value::Bool(a.blend_primary)),
        ],
    );
    let size = b.string(&format!("{{{w}, {h}}}"));
    let name = b.string(e.name);
    let author = if e.author.is_empty() { Builder::null() } else { b.string(e.author) };
    let strokes = b.add(Value::Int(i64::try_from(e.strokes).unwrap_or(i64::MAX)));
    let reference = reference_fields(&mut b, e.reference.unwrap_or(&ReferenceWindow::default()));
    let video = write_time_lapse(&mut b, &mut zip, e.time_lapse);
    // the last text's look, as a run of a space
    let text_style = match &doc.text_style {
        Some(t) => attributed_string(&mut b, " ", t, doc.profile),
        None => Builder::null(),
    };
    // the drawing guide, once it has been on
    let guide = if doc.guide.set_up || doc.guide.on { guide_object(&mut b, &doc.guide, w, h) } else { Builder::null() };
    let mut fields = vec![
        ("SilicaDocumentArchiveDPIKey", Value::Real(f64::from(doc.dpi))),
        ("SilicaDocumentArchiveUnitKey", Value::Int(0)),
        ("SilicaDocumentTrackedTimeKey", Value::Real(e.tracked_seconds)),
        ("SilicaDocumentVideoPurgedKey", Value::Bool(false)),
        ("animation", animation),
        ("authorName", author),
        ("backgroundColor", background),
        ("backgroundColorHSBA", hsba),
        ("backgroundHidden", Value::Bool(!doc.background_visible)),
        ("colorProfile", profile),
        ("composite", composite),
        ("drawingguide", guide),
        ("featureSet", Value::Int(1)),
        ("flippedHorizontally", Value::Bool(false)),
        ("flippedVertically", Value::Bool(false)),
        ("isFirstItemAnimationForeground", Value::Bool(doc.animation.as_ref().is_some_and(|a| a.foreground))),
        ("isLastItemAnimationBackground", Value::Bool(doc.animation.as_ref().is_some_and(|a| a.background))),
        ("lastTextStyling", text_style),
        ("layers", layers),
        ("legacyBehaviour", Value::Int(1)),
        ("mask", mask),
        ("name", name),
        ("orientation", Value::Int(1)),
        ("primaryItem", selected.clone()),
        ("selectedLayer", selected),
        ("selectedSamplerLayer", sampled),
        ("size", size),
        ("solo", Builder::null()),
        ("strokeCount", strokes),
        ("tileSize", Value::Int(i64::from(TILE))),
        ("unwrappedLayers", unwrapped),
        ("userSavedSelectionMasks", saved_masks),
        ("version", Value::Int(2)),
    ];
    fields.extend(video);
    fields.extend(reference);
    fields.sort_by(|a, b| a.0.cmp(b.0));
    b.fill(&doc_ref, &["SilicaDocument", "ValkyrieDocument", "NSObject"], fields);
    zip.add("Document.archive", &b.finish(doc_ref));
    zip.add("QuickLook/Thumbnail.png", e.thumbnail_png);
    if let Some(png) = e.reference_picture {
        zip.add(REFERENCE_PICTURE, png);
    }
    zip.finish()
}

/// The document's time-lapse fields, its segments added to `zip` (stored: video doesn't deflate).
fn write_time_lapse(b: &mut Builder, zip: &mut ZipWriter, t: Option<&TimeLapse>) -> Vec<(&'static str, Value)> {
    let Some(t) = t else { return vec![("videoEnabled", Value::Bool(false))] };
    let quality = QUALITY_NAMES.get(usize::from(t.quality)).unwrap_or(&"pro50.vqLossless");
    let resolution = RESOLUTION_NAMES.get(usize::from(t.resolution)).unwrap_or(&"pro50.vr4K");
    // the codecs by number: H.264, HEVC, HEVC with alpha, ProRes 4444
    let codec = i64::from(t.hevc);
    let quality_name = b.string(quality);
    let resolution_name = b.string(resolution);
    let mut fields = vec![
        ("videoCodec2020Key", Value::Int(codec)),
        ("videoCodecKey", Value::Int(codec)),
        ("videoEnabled", Value::Bool(t.recording)),
        ("videoQualityKey", quality_name.clone()),
        ("videoResolutionKey", resolution_name.clone()),
    ];
    if !t.segments.is_empty() {
        for (i, s) in t.segments.iter().enumerate() {
            zip.add_aligned(&format!("{SEGMENT_PREFIX}{}.mp4", i + 1), s, 1);
        }
        let (w, h) = t.frame_size;
        let size = b.string(&format!("{{{w}, {h}}}"));
        let bitrate = b.add(Value::Int(i64::try_from(t.bitrate).unwrap_or(i64::MAX)));
        let info = b.object(
            &["VideoSegmentInfo", "NSObject"],
            vec![
                ("bitrate", bitrate),
                ("codec", Value::Int(codec)),
                ("codec2020", Value::Int(codec)),
                ("colorSpace", Value::Int(0)),
                ("frameSize", size),
                ("framesPerSecond", Value::Int(i64::from(t.fps))),
                ("qualityPreferenceKey", quality_name),
                ("resolutionPreferenceKey", resolution_name),
                ("sourceOrientation", Value::Int(1)),
            ],
        );
        fields.push(("SilicaDocumentVideoSegmentInfoKey", info));
    }
    fields
}

/// The time-lapse a package keeps, or none if it keeps none (no recording setting and no video).
pub fn read_time_lapse(bytes: &[u8]) -> Result<Option<TimeLapse>, ReadError> {
    let zip = ZipReader::new(bytes).ok_or(ReadError::NotAPackage)?;
    let archive = Archive::read(&zip.read("Document.archive").ok_or(ReadError::NotAPackage)?)
        .ok_or(ReadError::Damaged("document"))?;
    let root = archive.root().ok_or(ReadError::Damaged("document"))?;
    let mut numbered: Vec<(u64, String)> =
        zip.names().filter_map(|n| segment_number(n).map(|k| (k, n.to_owned()))).collect();
    numbered.sort();
    let segments: Vec<Vec<u8>> = numbered.iter().filter_map(|(_, n)| zip.read(n)).collect();
    let recording = root.bool("videoEnabled");
    if recording.is_none() && segments.is_empty() {
        return Ok(None);
    }
    let info = root.object("SilicaDocumentVideoSegmentInfoKey");
    let named = |key: &str, of_info: &str| root.string(key).or_else(|| info.as_ref().and_then(|i| i.string(of_info)));
    let codec = root
        .int("videoCodec2020Key")
        .or_else(|| root.int("videoCodecKey"))
        .or_else(|| info.as_ref().and_then(|i| i.int("codec2020").or_else(|| i.int("codec"))));
    let d = TimeLapse::default();
    Ok(Some(TimeLapse {
        recording: recording.unwrap_or(d.recording),
        quality: named("videoQualityKey", "qualityPreferenceKey")
            .as_deref()
            .and_then(quality_number)
            .unwrap_or(d.quality),
        resolution: named("videoResolutionKey", "resolutionPreferenceKey")
            .as_deref()
            .and_then(resolution_number)
            .unwrap_or(d.resolution),
        // HEVC, with or without alpha
        hevc: codec.map_or(d.hevc, |c| c == 1 || c == 2),
        frame_size: info.as_ref().and_then(|i| i.string("frameSize")).and_then(|s| parse_size(&s)).unwrap_or((0, 0)),
        fps: info
            .as_ref()
            .and_then(|i| i.int("framesPerSecond"))
            .and_then(|f| u32::try_from(f).ok())
            .filter(|f| (1..=240).contains(f))
            .unwrap_or(d.fps),
        bitrate: info.as_ref().and_then(|i| i.int("bitrate")).and_then(|b| u64::try_from(b).ok()).unwrap_or(0),
        segments,
    }))
}

/// A segment's number from its entry's name.
fn segment_number(name: &str) -> Option<u64> {
    let (n, ext) = name.strip_prefix(SEGMENT_PREFIX)?.split_once('.')?;
    if !SEGMENT_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)) {
        return None;
    }
    n.parse().ok()
}

/// A quality's number from its name, older names too.
fn quality_number(name: &str) -> Option<u8> {
    match name {
        "vqNormal" | "pro50.vqLow" => Some(0),
        "vqHigh" | "pro50.vqGood" => Some(1),
        "vqStudio" | "pro50.vqStudio" => Some(2),
        "vqStudio4K" | "pro50.vqLossless" => Some(3),
        _ => None,
    }
}

/// A resolution's number from its name; the older quality names stood for one too.
fn resolution_number(name: &str) -> Option<u8> {
    match name {
        "pro50.vr1080" | "vqNormal" | "vqHigh" | "vqStudio" => Some(0),
        "pro50.vr2K" => Some(1),
        "pro50.vr4K" | "vqStudio4K" => Some(2),
        _ => None,
    }
}

/// The ids of what is directly in `group` (or at the top), top first; masks left out.
fn top_first(doc: &Document, group: Option<LayerId>) -> Vec<LayerId> {
    doc.layers().iter().rev().filter(|l| l.kind != LayerKind::Mask && l.parent == group).map(|l| l.id).collect()
}

/// Writes a layer (with its mask and tiles) or a group (with what it holds); gives its record.
fn write_item(
    b: &mut Builder,
    zip: &mut ZipWriter,
    uuids: &mut Uuids,
    e: &Export<'_>,
    doc_ref: &Value,
    id: LayerId,
    [selected, sampled]: [&mut Value; 2],
) -> Value {
    let doc = e.document;
    let (w, h) = (doc.width(), doc.height());
    let Some(l) = doc.layer(id) else { return Builder::null() };
    if l.kind == LayerKind::Group {
        let children: Vec<Value> = top_first(doc, Some(id))
            .into_iter()
            .map(|c| write_item(b, zip, uuids, e, doc_ref, c, [&mut *selected, &mut *sampled]))
            .collect();
        let children = b.array(children, true);
        let name = b.string(&l.name);
        return b.object(
            &["SilicaGroup", "ValkyrieGroup", "NSObject"],
            vec![
                ("animationHeldLength", Value::Int(held(doc, id))),
                ("children", children),
                ("document", doc_ref.clone()),
                ("isClipped", Value::Bool(l.clipped)),
                ("isCollapsed", Value::Bool(l.collapsed)),
                ("isHidden", Value::Bool(!l.visible)),
                ("isLocked", Value::Bool(l.locked)),
                ("isPrivate", Value::Bool(false)),
                ("name", name),
                ("opacity", Value::Real(f64::from(l.opacity))),
            ],
        );
    }
    let uuid = uuids.next();
    let px = e.pixels.get(&id);
    if let Some(px) = px {
        write_tiles(zip, &uuid, px, w, h, 4, 0);
    }
    let mask = doc.mask_of(id).and_then(|m| {
        let ml = doc.layer(m)?;
        let mask_uuid = uuids.next();
        let grey: Option<Vec<u8>> =
            e.pixels.get(&m).map(|px| px.as_chunks::<4>().0.iter().map(|&p| crate::document::mask_grey(p)).collect());
        if let Some(grey) = &grey {
            write_tiles(zip, &mask_uuid, grey, w, h, 1, 255);
        }
        let rec = LayerRecord {
            kind: 3,
            name: Some("Layer Mask"),
            bounds: Some(grey.map_or([0.0; 4], |g| bounds(&g, w, h, 1, 255))),
            opacity: 1.0,
            blend: 0,
            hidden: !ml.visible,
            locked: ml.locked,
            clipped: false,
            alpha_lock: false,
            private: false,
            held: 0,
            assisted: false,
            text: None,
        };
        Some(layer_record(b, doc_ref, &mask_uuid, None, rec, w, h))
    });
    let rec = LayerRecord {
        kind: 0,
        name: Some(&l.name),
        bounds: Some(px.map_or([0.0; 4], |px| bounds(px, w, h, 4, 0))),
        opacity: l.opacity,
        blend: blend_to(l.blend),
        hidden: !l.visible,
        locked: l.locked,
        clipped: l.clipped,
        alpha_lock: l.alpha_lock,
        private: l.private,
        held: held(doc, id),
        assisted: l.assisted,
        text: l.text.as_ref().map(|t| (t, doc.profile)),
    };
    let r = layer_record(b, doc_ref, &uuid, mask, rec, w, h);
    if id == doc.current() {
        *selected = r.clone();
    }
    // the reference layer is the one sampled from
    if l.reference {
        *sampled = r.clone();
    }
    r
}

/// A layer's settings as archived.
struct LayerRecord<'a> {
    /// 0 paint, 1 the flattened picture, 2 the selection, 3 a layer's mask.
    kind: i64,
    name: Option<&'a str>,
    /// What is painted: x, y from the bottom, width and height (all 0 if nothing); `None` where
    /// it isn't kept (the flattened picture).
    bounds: Option<[f64; 4]>,
    opacity: f32,
    blend: i64,
    hidden: bool,
    locked: bool,
    clipped: bool,
    alpha_lock: bool,
    /// Left out of the file's preview.
    private: bool,
    /// Animation Assist: how many more frames this frame is held for.
    held: i64,
    /// Drawing assist on.
    assisted: bool,
    /// A text layer's text, its colour in the canvas's profile.
    text: Option<(&'a LayerText, Profile)>,
}

/// How many more frames Animation Assist holds layer `id` for (0 without a hold).
fn held(doc: &Document, id: LayerId) -> i64 {
    doc.animation.as_ref().map_or(0, |a| i64::from(a.hold(id)))
}

impl LayerRecord<'_> {
    fn plain(kind: i64) -> LayerRecord<'static> {
        LayerRecord {
            kind,
            name: None,
            bounds: (kind != 1).then_some([0.0; 4]),
            opacity: 1.0,
            blend: 0,
            hidden: false,
            locked: false,
            clipped: false,
            alpha_lock: false,
            private: false,
            held: 0,
            assisted: false,
            text: None,
        }
    }
}

fn layer_record(
    b: &mut Builder,
    doc_ref: &Value,
    uuid: &str,
    mask: Option<Value>,
    r: LayerRecord<'_>,
    w: u32,
    h: u32,
) -> Value {
    let uuid = b.string(uuid);
    let name = match r.name {
        Some(n) => b.string(n),
        None => Builder::null(),
    };
    let rect = b.add(Value::Data(r.bounds.unwrap_or_default().iter().flat_map(|v| v.to_le_bytes()).collect()));
    let identity: Vec<u8> = [1.0f64, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let transform = b.add(Value::Data(identity));
    // a text layer's text, its box as wide as it wraps (or as what is painted, unwrapped)
    let text = match r.text {
        Some((t, profile)) => {
            let painted = r.bounds.map_or(0.0, |[_, _, width, _]| width);
            text_object(b, t, profile, h, if t.width > 0.0 { f64::from(t.width) } else { painted })
        }
        None => Builder::null(),
    };
    let pdf_transform = b.string(if r.text.is_some() { "[1, 0, 0, 1, 0, 0]" } else { "[0, 0, 0, 0, 0, 0]" });
    let time_keys: Vec<Value> = ["flags", "value", "timescale", "epoch"].iter().map(|k| b.string(k)).collect();
    let zeros: Vec<Value> = (0..4).map(|_| b.add(Value::Int(0))).collect();
    let video_time = b.object(
        &["NSDictionary", "NSObject"],
        vec![("NS.keys", Value::Array(time_keys)), ("NS.objects", Value::Array(zeros))],
    );
    b.object(
        &["SilicaLayer", "ValkyrieLayer", "NSObject"],
        vec![
            ("UUID", uuid),
            ("animationHeldLength", Value::Int(r.held)),
            ("blend", Value::Int(older_blends(r.blend).0)),
            ("bundledImagePath", Builder::null()),
            ("bundledMaskPath", Builder::null()),
            ("bundledVideoPath", Builder::null()),
            ("clipped", Value::Bool(r.clipped)),
            ("contentsRect", rect),
            ("contentsRectValid", Value::Bool(r.bounds.is_some())),
            ("document", doc_ref.clone()),
            ("extendedBlend", Value::Int(older_blends(r.blend).1)),
            ("extendedBlend2", Value::Int(r.blend)),
            ("hidden", Value::Bool(r.hidden)),
            ("locked", Value::Bool(r.locked)),
            ("mask", mask.unwrap_or_else(Builder::null)),
            ("name", name),
            ("opacity", Value::Real(f64::from(r.opacity))),
            ("perspectiveAssisted", Value::Bool(r.assisted)),
            ("preserve", Value::Bool(r.alpha_lock)),
            ("private", Value::Bool(r.private)),
            ("sizeHeight", Value::Int(i64::from(h))),
            ("sizeWidth", Value::Int(i64::from(w))),
            ("text", text),
            ("textPDF", Builder::null()),
            ("textPDFTransform", pdf_transform),
            ("textureSet", Builder::null()),
            ("transform", transform),
            ("type", Value::Int(r.kind)),
            ("version", Value::Int(4)),
            ("videoTime", video_time),
        ],
    )
}

/// A text layer's text as the archive keeps it (see [`layer_text`]): the words with one run of
/// attributes (alignment, typeface by its PostScript name, size, colour in sRGB, upright or
/// vertical), the box `width` wide, its top left placed on a canvas `h` high as kept, y up.
fn text_object(b: &mut Builder, t: &LayerText, profile: Profile, h: u32, width: f64) -> Value {
    let attributed = attributed_string(b, &t.text, t, profile);
    let [x, y] = t.origin.map(f64::from);
    let transform = b.string(&format!("[1, 0, 0, 1, {x}, {}]", f64::from(h) - y));
    b.object(
        &["ValkyrieText", "NSObject"],
        vec![("attributedString", attributed), ("width", Value::Real(width)), ("transform", transform)],
    )
}

/// `words` as an attributed string of one run with `t`'s look (see [`text_object`]).
fn attributed_string(b: &mut Builder, words: &str, t: &LayerText, profile: Profile) -> Value {
    let words =
        b.object(&["NSMutableString", "NSString", "NSObject"], vec![("NS.string", Value::String(words.to_owned()))]);
    let alignment = match t.align {
        TextAlign::Left => 0,
        TextAlign::Right => 1,
        TextAlign::Center => 2,
        TextAlign::Justify => 3,
    };
    let paragraph = b.object(
        &["NSMutableParagraphStyle", "NSParagraphStyle", "NSObject"],
        vec![("NSTabStops", Builder::null()), ("NSAlignment", Value::Int(alignment))],
    );
    // the family and style run together, as PostScript names are: Helvetica Neue, Medium is
    // "HelveticaNeue-Medium"
    let joined = |s: &str| s.split_whitespace().collect::<String>();
    let style = if t.style.trim().is_empty() { "Regular".to_owned() } else { joined(&t.style) };
    let family = if t.font.trim().is_empty() { "HelveticaNeue".to_owned() } else { joined(&t.font) };
    let font = b.string(&format!("{family}-{style}"));
    // in sRGB, past its range for a colour outside it
    let linear = crate::profile::apply(profile.matrix_to(Profile::Srgb), t.color.map(crate::profile::decode));
    let [red, green, blue] = linear.map(crate::profile::encode).map(f64::from);
    let rgb = b.add(Value::Data(format!("{red:.3} {green:.3} {blue:.3}").into_bytes()));
    let colour = b.object(
        &["UIColor", "NSObject"],
        vec![
            ("UIColorComponentCount", Value::Int(4)),
            ("UIRed", Value::Real(red)),
            ("UIGreen", Value::Real(green)),
            ("UIBlue", Value::Real(blue)),
            ("UIAlpha", Value::Real(f64::from(t.opacity))),
            ("NSRGB", rgb),
            ("NSColorSpace", Value::Int(2)),
        ],
    );
    // a collection's numbers are objects of their own, referred to, as every value in one is
    let left_to_right = b.add(Value::Int(0));
    let direction = b.array(vec![left_to_right], false);
    let keys: Vec<Value> = ["NSParagraphStyle", "CTVerticalForms", "_CFN", "NSColor", "_CFZ", "NSWritingDirection"]
        .iter()
        .map(|k| b.string(k))
        .collect();
    let vertical = b.add(Value::Int(i64::from(t.vertical)));
    let size = b.add(Value::Real(f64::from(t.size)));
    let values = vec![paragraph, vertical, font, colour, size, direction];
    let attributes = b.object(
        &["NSDictionary", "NSObject"],
        vec![("NS.keys", Value::Array(keys)), ("NS.objects", Value::Array(values))],
    );
    b.object(
        &["NSMutableAttributedString", "NSAttributedString", "NSObject"],
        vec![("NSString", words), ("NSAttributes", attributes)],
    )
}

/// The smallest rectangle holding every pixel of `px` that isn't all `blank`: x, y from the
/// bottom, width and height; all 0 if there is none.
fn bounds(px: &[u8], w: u32, h: u32, bpp: usize, blank: u8) -> [f64; 4] {
    let (w, h) = (w as usize, h as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, p) in px.chunks_exact(bpp).enumerate().take(w * h) {
        if p.iter().any(|&v| v != blank) {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    if x0 == usize::MAX {
        return [0.0; 4];
    }
    [x0 as f64, (h - 1 - y1) as f64, (x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64]
}

/// Writes `px` (`w` × `h`, `bpp` bytes a pixel, top row first) as tiles under `uuid/`, leaving out
/// those whose every byte is `blank` (clear paint, white mask).
fn write_tiles(zip: &mut ZipWriter, uuid: &str, px: &[u8], w: u32, h: u32, bpp: usize, blank: u8) {
    let (w, h, t) = (w as usize, h as usize, TILE as usize);
    let tiles: Vec<(usize, usize)> =
        (0..h.div_ceil(t)).flat_map(|ty| (0..w.div_ceil(t)).map(move |tx| (tx, ty))).collect();
    // a tile packed: none if it is blank; deflated too when that packing found something to
    // shrink (what LZ4 can't shrink, deflate barely does, and slowly)
    let pack = |&(tx, ty): &(usize, usize)| -> Option<Ready> {
        let (tw, th) = (t.min(w - tx * t), t.min(h - ty * t));
        let mut raw = Vec::with_capacity(tw * th * bpp);
        for r in 0..th {
            let row = h - 1 - (ty * t + r);
            let at = (row * w + tx * t) * bpp;
            raw.extend_from_slice(px.get(at..at + tw * bpp).unwrap_or(&[]));
        }
        if raw.len() != tw * th * bpp || raw.iter().all(|&v| v == blank) {
            return None;
        }
        let packed = applz4::encode(&raw);
        Some(Ready::of(&packed, packed.len() < raw.len()))
    };
    // packed side by side, added in order (a tile whose thread stopped is left out)
    let packed = crate::par::side_by_side(&tiles, pack);
    for (&(tx, ty), ready) in tiles.iter().zip(packed) {
        if let Some(ready) = ready.flatten() {
            zip.add_ready(&format!("{uuid}/{tx}~{ty}.lz4"), ready);
        }
    }
}

/// UUIDs from a seed (random enough to tell layers apart).
struct Uuids(u64);

impl Uuids {
    fn next(&mut self) -> String {
        let mut word = || {
            // splitmix64
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        let (a, c) = (word(), word());
        format!(
            "{:08X}-{:04X}-4{:03X}-{:04X}-{:012X}",
            a >> 32,
            (a >> 16) & 0xFFFF,
            a & 0x0FFF,
            ((c >> 48) & 0x3FFF) | 0x8000,
            c & 0xFFFF_FFFF_FFFF
        )
    }
}

/// Hue, saturation and brightness of a colour, all 0..1.
fn hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, if max <= 0.0 { 0.0 } else { d / max }, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn painted(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                out.extend_from_slice(&f(x, y));
            }
        }
        out
    }

    /// A package of an empty 64 x 48 canvas with `reference` (and its picture).
    fn with_reference(reference: Option<&ReferenceWindow>, picture: Option<&[u8]>) -> Vec<u8> {
        let doc = Document::new(64, 48);
        write(&Export {
            document: &doc,
            pixels: &HashMap::new(),
            composite: &[255; 64 * 48 * 4],
            thumbnail_png: &[],
            name: "Reference",
            author: "",
            strokes: 0,
            tracked_seconds: 0.0,
            seed: 11,
            time_lapse: None,
            reference,
            reference_picture: picture,
        })
    }

    #[test]
    fn the_reference_window_and_its_picture_come_back_from_a_package() {
        let window = ReferenceWindow {
            shown: true,
            kind: ReferenceKind::Image,
            size: Some([300.0, 200.0]),
            at: Some([40.0, 60.0]),
            corner: 2,
            docked: true,
            three_d: true,
            face_background_hidden: true,
            ..ReferenceWindow::default()
        };
        let back = read(&with_reference(Some(&window), Some(b"a picture"))).unwrap();
        // what only the app's own store keeps (how each picture sits in the window) isn't in it
        assert_eq!(
            back.reference,
            ReferenceWindow { canvas_view: [1.0, 0.0, 0.0], image_view: [1.0, 0.0, 0.0], ..window }
        );
        assert_eq!(back.reference_picture, Some((b"a picture".to_vec(), 0)));
    }

    #[test]
    fn a_package_keeps_the_window_s_kind_and_place_under_each_other_s_names() {
        let window = ReferenceWindow {
            kind: ReferenceKind::Face,
            size: Some([300.0, 200.0]),
            at: Some([40.0, 60.0]),
            ..ReferenceWindow::default()
        };
        let bytes = with_reference(Some(&window), Some(b"png"));
        let zip = ZipReader::new(&bytes).unwrap();
        assert_eq!(zip.read("ReferenceImage/ReferenceImage.png").as_deref(), Some(&b"png"[..]));
        let archive = Archive::read(&zip.read("Document.archive").unwrap()).unwrap();
        let root = archive.root().unwrap();
        assert_eq!(root.int("referenceAnchor"), Some(2));
        assert_eq!(root.string("referenceKind").as_deref(), Some("{190, 214.5}"));
        assert_eq!(root.string("referenceSize").as_deref(), Some("{300, 309}"));
        assert_eq!(root.int("referenceOrientation"), Some(0));
    }

    #[test]
    fn a_package_whose_window_was_never_used_reads_as_none_and_writes_as_one_did() {
        let bytes = with_reference(None, None);
        let back = read(&bytes).unwrap();
        assert!(back.reference.is_unused());
        assert_eq!(back.reference_picture, None);
        let zip = ZipReader::new(&bytes).unwrap();
        let archive = Archive::read(&zip.read("Document.archive").unwrap()).unwrap();
        let root = archive.root().unwrap();
        // as a package from a tablet that never showed it: no place, no size, the canvas
        assert_eq!(root.string("referenceKind"), None);
        assert_eq!(root.string("referenceSize").as_deref(), Some("{0, 0}"));
        assert_eq!((root.int("referenceAnchor"), root.bool("referenceVisibility")), (Some(0), Some(false)));
        assert_eq!(zip.read("ReferenceImage/ReferenceImage.png"), None);
    }

    #[test]
    fn a_reference_picture_at_the_top_and_its_turn_are_read() {
        let bytes = with_reference(Some(&ReferenceWindow { shown: true, ..ReferenceWindow::default() }), None);
        let zip = ZipReader::new(&bytes).unwrap();
        let mut moved = ZipWriter::new();
        for name in zip.names() {
            moved.add(name, &zip.read(name).unwrap());
        }
        moved.add("ReferenceImage.png", b"at the top");
        let back = read(&moved.finish()).unwrap();
        assert_eq!(back.reference_picture.map(|(png, _)| png), Some(b"at the top".to_vec()));
    }

    #[test]
    fn an_artwork_survives_a_round_trip() {
        let (w, h) = (300, 520);
        let mut doc = Document::new(w, h);
        let base = doc.current();
        doc.rename(base, "Sketch");
        doc.set_opacity(base, 0.75);
        doc.set_blend(base, BlendMode::Multiply);
        let top = doc.add_layer();
        doc.rename(top, "Ink");
        doc.set_clipped(top, true);
        doc.set_private(top, true);
        doc.set_assisted(top, true);
        let mask = doc.add_mask(base).unwrap();
        doc.background = [0.2, 0.4, 0.6];
        let mut pixels = HashMap::new();
        // a gradient that tells top from bottom and left from right
        let sketch = painted(w, h, |x, y| [(x % 256) as u8, (y % 256) as u8, 0, 255]);
        let ink = painted(w, h, |x, y| if (x / 37 + y / 41) % 3 == 0 { [10, 20, 30, 128] } else { [0; 4] });
        let grey = painted(w, h, |x, _| {
            let g = if x < 150 { 255 } else { 0 };
            [g, g, g, 255]
        });
        pixels.insert(base, sketch.clone());
        pixels.insert(top, ink.clone());
        pixels.insert(mask, grey.clone());
        let composite = vec![255u8; (w * h * 4) as usize];
        let bytes = write(&Export {
            document: &doc,
            pixels: &pixels,
            composite: &composite,
            thumbnail_png: &[],
            name: "Round trip",
            author: "",
            strokes: 7,
            tracked_seconds: 12.5,
            seed: 42,
            time_lapse: None,
            reference: None,
            reference_picture: None,
        });

        let back = read(&bytes).unwrap();
        assert_eq!(back.name, "Round trip");
        assert_eq!(back.strokes, 7);
        let d = &back.document;
        assert_eq!((d.width(), d.height()), (w, h));
        assert!((d.background[2] - 0.6).abs() < 1e-6);
        let kinds: Vec<(LayerKind, &str)> = d.layers().iter().map(|l| (l.kind, l.name.as_str())).collect();
        assert_eq!(kinds, [(LayerKind::Paint, "Sketch"), (LayerKind::Mask, "Layer Mask"), (LayerKind::Paint, "Ink")]);
        let sk = d.layers().first().unwrap();
        assert_eq!((sk.blend, sk.opacity), (BlendMode::Multiply, 0.75));
        assert!(d.layers().get(2).unwrap().clipped && d.layers().get(2).unwrap().private);
        assert!(d.layers().get(2).unwrap().assisted && !d.layers().first().unwrap().assisted);
        assert!(!d.layers().first().unwrap().private);
        let px = |i: usize| &back.pixels.iter().find(|(id, _)| *id == d.layers().get(i).unwrap().id).unwrap().1;
        assert_eq!(px(0), &sketch);
        assert_eq!(px(1), &grey);
        assert_eq!(px(2), &ink);
    }

    #[test]
    fn a_canvas_keeps_its_own_profile() {
        let (w, h) = (40, 30);
        let pixels = HashMap::new();
        let composite = vec![255u8; (w * h * 4) as usize];
        let export = |doc: &Document| {
            read(&write(&Export {
                document: doc,
                pixels: &pixels,
                composite: &composite,
                thumbnail_png: &[],
                name: "Profiles",
                author: "",
                strokes: 0,
                tracked_seconds: 0.0,
                seed: 1,
                time_lapse: None,
                reference: None,
                reference_picture: None,
            }))
            .unwrap()
            .document
        };
        // a CMYK canvas: its pixels in Display P3, the press profile kept whole
        let mut doc = Document::new(w, h);
        let press = IccProfile::parse(&crate::cmyk::profile()).unwrap();
        doc.change_to(Profile::DisplayP3, Some(CanvasIcc(Arc::new(press))));
        let back = export(&doc);
        assert_eq!(back.profile, Profile::DisplayP3);
        assert_eq!(back.icc, doc.icc);
        assert_eq!(back.profile_name(), crate::cmyk::NAME);
        assert!(back.press().is_some());
        // a built-in profile stays built in
        let mut doc = Document::new(w, h);
        doc.change_profile(Profile::DisplayP3);
        let back = export(&doc);
        assert_eq!((back.profile, back.icc.is_none()), (Profile::DisplayP3, true));
    }

    #[test]
    fn every_blend_mode_has_its_number() {
        for m in BlendMode::ALL {
            // Procreate has no Greater mode: it saves as Normal
            if m == BlendMode::Greater {
                assert_eq!(blend_from(blend_to(m)), BlendMode::Normal);
                continue;
            }
            assert_eq!(blend_from(blend_to(m)), m, "{m:?}");
        }
        assert_eq!(
            (blend_to(BlendMode::Shade), blend_to(BlendMode::Darken), blend_to(BlendMode::Divide)),
            (19, 34, 26)
        );
        // the older keys: Darken keeps its old number, and the oldest stops at 19
        assert_eq!(older_blends(34), (19, 19));
        assert_eq!(older_blends(26), (0, 26));
        assert_eq!(older_blends(14), (14, 14));
        assert_eq!(blend_from(18), BlendMode::Normal);
    }

    #[test]
    fn masks_keep_only_what_is_painted_on_them() {
        let (w, h) = (300, 520);
        let mut doc = Document::new(w, h);
        let base = doc.current();
        let mask = doc.add_mask(base).unwrap();
        // white but for a dark square near the top left
        let grey =
            painted(
                w,
                h,
                |x, y| if (10..20).contains(&x) && (20..24).contains(&y) { [0, 0, 0, 255] } else { [255; 4] },
            );
        let mut pixels = HashMap::new();
        pixels.insert(base, vec![0u8; (w * h * 4) as usize]);
        pixels.insert(mask, grey.clone());
        let bytes = write(&Export {
            document: &doc,
            pixels: &pixels,
            composite: &vec![255u8; (w * h * 4) as usize],
            thumbnail_png: &[],
            name: "",
            author: "",
            strokes: 0,
            tracked_seconds: 0.0,
            seed: 3,
            time_lapse: None,
            reference: None,
            reference_picture: None,
        });
        // the picture's 6 tiles, the mask's one, none for the clear layer
        let zip = ZipReader::new(&bytes).unwrap();
        assert_eq!(zip.names().filter(|n| n.ends_with(".lz4")).count(), 7);
        let back = read(&bytes).unwrap();
        let m = back.document.layers().iter().find(|l| l.kind == LayerKind::Mask).unwrap();
        assert_eq!(back.pixels.iter().find(|(id, _)| *id == m.id).unwrap().1, grey);
        assert_eq!(bounds(&grey, w, h, 4, 255), [10.0, 496.0, 10.0, 4.0]);
    }

    #[test]
    fn tiles_in_the_older_format_are_read_too() {
        // a layer's tiles as the older app kept them: LZO (here stored as one run of literals)
        // under `.chunk`, beside a tile kept in both forms
        let lzo = |raw: &[u8]| {
            let mut out = vec![0u8];
            let mut rest = raw.len() - 3 - 15;
            while rest > 255 {
                out.push(0);
                rest -= 255;
            }
            out.push(rest as u8);
            out.extend_from_slice(raw);
            out.extend_from_slice(&[0x11, 0, 0]);
            out
        };
        let (w, h) = (300, 200);
        let doc = Document::new(w, h);
        let layer = doc.current();
        let picture = painted(w, h, |x, y| [(x % 256) as u8, (y % 256) as u8, 7, 255]);
        let mut pixels = HashMap::new();
        pixels.insert(layer, picture.clone());
        let bytes = write(&Export {
            document: &doc,
            pixels: &pixels,
            composite: &picture,
            thumbnail_png: &[],
            name: "",
            author: "",
            strokes: 0,
            tracked_seconds: 0.0,
            seed: 5,
            time_lapse: None,
            reference: None,
            reference_picture: None,
        });
        let zip = ZipReader::new(&bytes).unwrap();
        let mut older = ZipWriter::new();
        for name in zip.names() {
            let data = zip.read(name).unwrap();
            match name.strip_suffix(".lz4") {
                Some(at) => {
                    let raw = applz4::decode(&data, 1 << 24).unwrap();
                    older.add(&format!("{at}.chunk"), &lzo(&raw));
                    // one tile in both forms: the LZ4 one is read
                    if at.ends_with("0~0") {
                        older.add(name, &data);
                    }
                }
                None => older.add(name, &data),
            }
        }
        let back = read(&older.finish()).unwrap();
        assert_eq!(back.pixels.iter().find(|(id, _)| *id == layer).map(|(_, p)| p), Some(&picture));
    }

    #[test]
    fn bounds_count_rows_from_the_bottom() {
        let mut px = vec![0u8; 4 * 3];
        px[1] = 9;
        assert_eq!(bounds(&px, 4, 3, 1, 0), [1.0, 2.0, 1.0, 1.0]);
        assert_eq!(bounds(&[0; 12], 4, 3, 1, 0), [0.0; 4]);
    }

    #[test]
    fn groups_keep_what_they_hold() {
        let mut doc = Document::new(64, 64);
        let a = doc.current();
        let b = doc.add_layer();
        doc.set_current(b);
        let g = doc.combine_down(b).unwrap();
        doc.rename(g, "Pair");
        doc.set_collapsed(g, true);
        let c = doc.add_layer();
        let mut pixels = HashMap::new();
        for id in [a, b, c] {
            pixels.insert(id, vec![0u8; 64 * 64 * 4]);
        }
        let bytes = write(&Export {
            document: &doc,
            pixels: &pixels,
            composite: &[0; 64 * 64 * 4],
            thumbnail_png: &[],
            name: "",
            author: "",
            strokes: 0,
            tracked_seconds: 0.0,
            seed: 1,
            time_lapse: None,
            reference: None,
            reference_picture: None,
        });
        let back = read(&bytes).unwrap();
        let d = &back.document;
        assert_eq!(back.name, "Untitled Artwork");
        let group = d.layers().iter().find(|l| l.kind == LayerKind::Group).unwrap();
        assert_eq!(group.name, "Pair");
        assert!(group.collapsed);
        // the same tree: kinds, and which group each sits in, bottom first
        let shape = |d: &Document| {
            d.layers().iter().map(|l| (l.kind, l.parent.and_then(|p| d.index_of(p)))).collect::<Vec<_>>()
        };
        assert_eq!(shape(d), shape(&doc));
    }

    #[test]
    fn an_emptied_group_is_kept() {
        let mut doc = Document::new(64, 64);
        let a = doc.current();
        doc.add_layer();
        let c = doc.add_layer();
        let g = doc.combine_down(c).unwrap();
        // c merged into b in the group, then b into a, out of it: the group stays, empty
        assert!(doc.finish_merge_down(g) && doc.finish_merge_down(g));
        assert_eq!(doc.layers().iter().map(|l| l.id).collect::<Vec<_>>(), [a, g]);
        let mut pixels = HashMap::new();
        for l in doc.layers().iter().filter(|l| l.kind != LayerKind::Group) {
            pixels.insert(l.id, vec![0u8; 64 * 64 * 4]);
        }
        let bytes = write(&Export {
            document: &doc,
            pixels: &pixels,
            composite: &[0; 64 * 64 * 4],
            thumbnail_png: &[],
            name: "",
            author: "",
            strokes: 0,
            tracked_seconds: 0.0,
            seed: 1,
            time_lapse: None,
            reference: None,
            reference_picture: None,
        });
        let back = read(&bytes).unwrap();
        let kinds = |d: &Document| d.layers().iter().map(|l| l.kind).collect::<Vec<_>>();
        assert_eq!(kinds(&back.document), kinds(&doc));
        assert!(back.document.contents(back.document.layers().last().unwrap().id).is_empty());
    }

    #[test]
    fn animation_settings_are_the_canvases_own() {
        let mut doc = Document::new(16, 16);
        let id = doc.current();
        let mut a = crate::animation::Animation {
            fps: 24,
            onion_frames: 3,
            onion_opacity: 0.25,
            playback: crate::animation::Playback::PingPong,
            blend_primary: true,
            ..Default::default()
        };
        a.set_hold(id, 2);
        doc.animation = Some(a);
        let pixels = HashMap::from([(id, vec![0u8; 16 * 16 * 4])]);
        let bytes = write(&Export {
            document: &doc,
            pixels: &pixels,
            composite: &[0; 16 * 16 * 4],
            thumbnail_png: &[],
            name: "",
            author: "",
            strokes: 0,
            tracked_seconds: 0.0,
            seed: 1,
            time_lapse: None,
            reference: None,
            reference_picture: None,
        });
        let zip = ZipReader::new(&bytes).unwrap();
        let archive = Archive::read(&zip.read("Document.archive").unwrap()).unwrap();
        let root = archive.root().unwrap();
        let anim = root.object("animation").unwrap();
        assert_eq!(anim.int("frameRate"), Some(24));
        assert_eq!(anim.int("onionSkinCount"), Some(3));
        assert_eq!(anim.real("onionSkinOpacity"), Some(0.25));
        assert_eq!(anim.int("playbackMode"), Some(2));
        assert_eq!(anim.bool("primaryMixed"), Some(true));
        assert_eq!(root.object("selectedLayer").unwrap().int("animationHeldLength"), Some(2));
        assert_eq!(anim.int("animationMode"), Some(1));
        // read back as it was written
        let back = read(&bytes).unwrap();
        let b = back.document.animation.as_ref().unwrap();
        assert_eq!(
            (b.fps, b.onion_frames, b.onion_opacity, b.playback, b.blend_primary, b.pages),
            (24, 3, 0.25, crate::animation::Playback::PingPong, true, false)
        );
        assert_eq!(b.hold(back.document.current()), 2);
        // Page Assist is the other mode; with neither, the settings aren't the canvas's
        let pack = |doc: &Document| {
            write(&Export {
                document: doc,
                pixels: &pixels,
                composite: &[0; 16 * 16 * 4],
                thumbnail_png: &[],
                name: "",
                author: "",
                strokes: 0,
                tracked_seconds: 0.0,
                seed: 1,
                time_lapse: None,
                reference: None,
                reference_picture: None,
            })
        };
        if let Some(a) = doc.animation.as_mut() {
            a.pages = true;
        }
        assert!(read(&pack(&doc)).unwrap().document.animation.unwrap().pages);
        doc.animation = None;
        assert!(read(&pack(&doc)).unwrap().document.animation.is_none());
    }

    /// A text layer's archived text (as the samples have it): its box placed by `transform` on
    /// a canvas `size` square as kept, turned `orientation`.
    fn text_package(transform: &str, orientation: i64, size: u32) -> Vec<u8> {
        let mut b = Builder::new();
        let words = b.object(
            &["NSMutableString", "NSString", "NSObject"],
            vec![("NS.string", Value::String("Hi there".into()))],
        );
        let paragraph = b
            .object(&["NSMutableParagraphStyle", "NSParagraphStyle", "NSObject"], vec![("NSAlignment", Value::Int(2))]);
        let colour = b.object(
            &["UIColor", "NSObject"],
            vec![
                ("UIRed", Value::Real(1.07)),
                ("UIGreen", Value::Real(0.5)),
                ("UIBlue", Value::Real(0.2)),
                ("UIAlpha", Value::Real(1.0)),
            ],
        );
        let keys = ["NSParagraphStyle", "_CFN", "NSColor", "_CFZ"].map(|k| b.string(k)).to_vec();
        let font = b.string("HelveticaNeue-Medium");
        let font_size = b.add(Value::Real(176.0));
        let attributes = b.object(
            &["NSDictionary", "NSObject"],
            vec![
                ("NS.keys", Value::Array(keys)),
                ("NS.objects", Value::Array(vec![paragraph, font, colour, font_size])),
            ],
        );
        let attributed = b.object(
            &["NSMutableAttributedString", "NSAttributedString", "NSObject"],
            vec![("NSString", words), ("NSAttributes", attributes)],
        );
        let transform = b.string(transform);
        let text = b.object(
            &["ValkyrieText", "NSObject"],
            vec![("attributedString", attributed), ("width", Value::Real(1344.0)), ("transform", transform)],
        );
        let uuid = b.string("T");
        let layer = b.object(&["SilicaLayer", "NSObject"], vec![("UUID", uuid), ("text", text)]);
        let layers = b.array(vec![layer], true);
        let size = b.string(&format!("{{{size}, {size}}}"));
        let root = b.object(
            &["SilicaDocument", "NSObject"],
            vec![("size", size), ("orientation", Value::Int(orientation)), ("layers", layers)],
        );
        let mut z = ZipWriter::new();
        z.add("Document.archive", &b.finish(root));
        z.finish()
    }

    #[test]
    fn a_text_layer_keeps_its_words_and_look_its_box_where_it_shows() {
        // upright, and made upside down (its box turned with the canvas): the same place shown
        for (transform, orientation) in [("[1, 0, 0, 1, 352, 1909]", 1), ("[-1, 0, -0, -1, 1696, 139]", 2)] {
            let back = read(&text_package(transform, orientation, 2048)).unwrap();
            let layer = back.document.layers().iter().find(|l| l.text.is_some()).unwrap();
            assert_eq!(layer.name, "Hi there");
            let t = layer.text.as_ref().unwrap();
            assert_eq!((t.text.as_str(), t.font.as_str(), t.style.as_str()), ("Hi there", "Helvetica Neue", "Medium"));
            assert_eq!((t.size, t.align, t.width), (176.0, TextAlign::Center, 1344.0));
            assert_eq!(t.origin, [352.0, 139.0], "orientation {orientation}");
            // an sRGB canvas: the colour past sRGB clipped to it
            assert_eq!(t.color, [1.0, 0.5, 0.2]);
        }
        // turned on the canvas: kept as its pixels alone
        let back = read(&text_package("[0, 1, -1, 0, 1000, 1000]", 1, 2048)).unwrap();
        assert!(back.document.layers().iter().all(|l| l.text.is_none()));
    }

    #[test]
    fn a_text_layer_written_reads_back_as_the_same_text() {
        let mut doc = Document::new(400, 300);
        let text = LayerText {
            text: "Two\nlines".to_owned(),
            font: "sans-serif-smallcaps".to_owned(),
            style: "Bold Italic".to_owned(),
            size: 48.0,
            align: TextAlign::Right,
            vertical: true,
            color: [0.25, 0.5, 1.0],
            opacity: 0.75,
            origin: [12.5, 40.0],
            width: 220.0,
            ..LayerText::default()
        };
        let id = doc.add_text_layer(text.clone());
        let mut pixels = HashMap::new();
        pixels.insert(
            id,
            painted(
                400,
                300,
                |x, y| if (20..60).contains(&x) && (50..90).contains(&y) { [9, 9, 9, 255] } else { [0; 4] },
            ),
        );
        let composite = vec![255u8; 400 * 300 * 4];
        let export = Export {
            document: &doc,
            pixels: &pixels,
            composite: &composite,
            thumbnail_png: &[],
            name: "Text",
            author: "",
            strokes: 0,
            tracked_seconds: 0.0,
            seed: 7,
            time_lapse: None,
            reference: None,
            reference_picture: None,
        };
        let back = read(&write(&export)).unwrap();
        let layer = back.document.layers().iter().find(|l| l.text.is_some()).unwrap();
        let t = layer.text.as_ref().unwrap();
        assert!(t.color.iter().zip(text.color).all(|(a, b)| (a - b).abs() < 1e-3), "{:?}", t.color);
        assert_eq!(LayerText { color: text.color, ..t.clone() }, text);
        assert_eq!(layer.name, "Two");
        // the last text's look goes with it
        let mut styled = doc.clone();
        styled.text_style = Some(text.look());
        let back = read(&write(&Export { document: &styled, ..export })).unwrap();
        assert_eq!(back.document.text_style, Some(text.look()));
    }

    #[test]
    fn a_drawing_guide_is_kept_once_it_has_been_on() {
        use crate::guide::{Guide, GuideKind, LengthUnit, SymmetryKind};
        let pack = |guide: Guide| {
            let mut doc = Document::new(400, 300);
            doc.guide = guide;
            let pixels = HashMap::new();
            let composite = vec![255u8; 400 * 300 * 4];
            read(&write(&Export {
                document: &doc,
                pixels: &pixels,
                composite: &composite,
                thumbnail_png: &[],
                name: "Guide",
                author: "",
                strokes: 0,
                tracked_seconds: 0.0,
                seed: 3,
                time_lapse: None,
                reference: None,
                reference_picture: None,
            }))
            .unwrap()
            .document
            .guide
        };
        // never on: none kept, none read
        assert!(!pack(Guide::new(400, 300)).set_up);
        // turned, symmetrical, with vanishing points and a look of its own: back as it went
        let mut g = Guide::new(400, 300);
        g.on = true;
        g.set_up = true;
        g.kind = GuideKind::Symmetry;
        g.symmetry = SymmetryKind::Quadrant;
        g.rotational = true;
        g.centre = [150.0, 120.0];
        g.angle = 0.3;
        g.vanishing = vec![[20.0, 40.0], [380.0, 60.0]];
        g.looks[0].size = 52.0;
        g.looks[0].opacity = 0.5;
        g.looks[0].unit = LengthUnit::Millimetres;
        g.looks[1].unit = LengthUnit::Inches;
        g.looks[2].color = 0.25;
        let back = pack(g.clone());
        assert!((back.angle - g.angle).abs() < 1e-4 && (back.looks[2].color - 0.25).abs() < 0.002, "{back:?}");
        assert!((back.looks[0].size - 52.0).abs() < 1e-3);
        assert_eq!(back.looks.map(|l| l.unit), g.looks.map(|l| l.unit));
        let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3;
        assert!(close(back.centre, g.centre) && back.vanishing.iter().zip(&g.vanishing).all(|(a, b)| close(*a, *b)));
        assert_eq!(
            Guide { angle: g.angle, centre: g.centre, vanishing: g.vanishing.clone(), looks: g.looks, ..back },
            g
        );
    }

    #[test]
    fn turned_canvases_are_shown_upright() {
        // 3 × 2, values = index; a quarter turn clockwise makes it 2 × 3
        let src: Vec<u8> = (0..6).collect();
        assert_eq!(orient(src.clone(), 3, 2, 1, 1, false, false), [3, 0, 4, 1, 5, 2]);
        assert_eq!(orient(src.clone(), 3, 2, 1, 2, false, false), [5, 4, 3, 2, 1, 0]);
        assert_eq!(orient(src.clone(), 3, 2, 1, 3, false, false), [2, 5, 1, 4, 0, 3]);
        assert_eq!(orient(src.clone(), 3, 2, 1, 0, true, false), [2, 1, 0, 5, 4, 3]);
        // made upside down it is shown half turned; made on its side, the top left stored at
        // the top right
        assert_eq!([1, 2, 3, 4, 0, 9].map(shown_turns), [0, 2, 3, 1, 0, 0]);
        let corner = orient(vec![0, 0, 1, 0, 0, 0], 3, 2, 1, shown_turns(3), false, false);
        assert_eq!(corner, [1, 0, 0, 0, 0, 0]);
        assert_eq!(parse_size("{1488, 2266}"), Some((1488, 2266)));
        assert_eq!(parse_size("1488x2266"), None);
    }

    #[test]
    fn the_time_lapse_round_trips() {
        let doc = Document::new(16, 16);
        let id = doc.layers().first().map(|l| l.id).unwrap();
        let pixels = HashMap::from([(id, vec![0u8; 16 * 16 * 4])]);
        let export = |t: Option<&TimeLapse>| {
            write(&Export {
                document: &doc,
                pixels: &pixels,
                composite: &[0; 16 * 16 * 4],
                thumbnail_png: &[],
                name: "",
                author: "",
                strokes: 0,
                tracked_seconds: 0.0,
                seed: 1,
                time_lapse: t,
                reference: None,
                reference_picture: None,
            })
        };
        let t = TimeLapse {
            recording: false,
            quality: 2,
            resolution: 1,
            hevc: true,
            segments: vec![b"first".to_vec(), b"second".to_vec()],
            frame_size: (1920, 1080),
            fps: 30,
            bitrate: 8_000_000,
        };
        let bytes = export(Some(&t));
        let zip = ZipReader::new(&bytes).unwrap();
        assert_eq!(zip.read("video/segments/segment-1.mp4").as_deref(), Some(&b"first"[..]));
        let archive = Archive::read(&zip.read("Document.archive").unwrap()).unwrap();
        let root = archive.root().unwrap();
        assert_eq!(root.string("videoQualityKey").as_deref(), Some("pro50.vqStudio"));
        let info = root.object("SilicaDocumentVideoSegmentInfoKey").unwrap();
        assert_eq!(info.string("resolutionPreferenceKey").as_deref(), Some("pro50.vr2K"));
        assert_eq!(info.int("bitrate"), Some(8_000_000));
        assert_eq!(read_time_lapse(&bytes).unwrap(), Some(t));
        // the layers read as before
        assert!(read(&bytes).is_ok());

        // none: not recording, and nothing kept
        let none = read_time_lapse(&export(None)).unwrap().unwrap();
        assert!(!none.recording && none.segments.is_empty());
        let mut z = ZipWriter::new();
        let mut b = Builder::new();
        let root = b.object(&["SilicaDocument", "NSObject"], vec![]);
        z.add("Document.archive", &b.finish(root));
        assert_eq!(read_time_lapse(&z.finish()), Ok(None));
    }

    #[test]
    fn segments_are_ordered_by_number_and_older_names_read() {
        assert_eq!(segment_number("video/segments/segment-12.mov"), Some(12));
        assert_eq!(segment_number("video/segments/segment-3.txt"), None);
        assert_eq!(segment_number("video/segment-3.mp4"), None);
        assert_eq!(quality_number("vqHigh"), Some(1));
        assert_eq!(resolution_number("vqStudio4K"), Some(2));
    }

    #[test]
    fn what_isnt_a_package_is_refused() {
        assert_eq!(read(b"hello").err(), Some(ReadError::NotAPackage));
        let mut z = ZipWriter::new();
        z.add("Document.archive", b"not an archive");
        assert_eq!(read(&z.finish()).err(), Some(ReadError::Damaged("document")));
    }

    /// A real package, when one is given (`PACKAGE_SAMPLE=path`): it reads, the right size.
    #[test]
    fn a_sample_reads() {
        let Ok(path) = std::env::var("PACKAGE_SAMPLE") else { return };
        let bytes = std::fs::read(path).unwrap();
        let back = read(&bytes).unwrap();
        eprintln!(
            "{} x {}, layers {:?}",
            back.document.width(),
            back.document.height(),
            back.document.layers().iter().map(|l| (&l.name, l.kind, l.blend)).collect::<Vec<_>>()
        );
        assert!(!back.pixels.is_empty());
        // and, if asked, the first layer's coverage as a picture to look at
        if let (Ok(out), Some((_, px))) = (std::env::var("PACKAGE_SAMPLE_OUT"), back.pixels.first()) {
            let mut pgm = format!(
                "P5 {} {} 255
",
                back.document.width(),
                back.document.height()
            )
            .into_bytes();
            pgm.extend(px.as_chunks::<4>().0.iter().map(|p| 255 - p[3]));
            std::fs::write(out, pgm).unwrap();
        }
        // and, if asked, written back as a package (`PACKAGE_REWRITE=path`), the paint layers
        // flattened over the background for the composite, the sample's own preview kept
        if let Ok(out) = std::env::var("PACKAGE_REWRITE") {
            let doc = &back.document;
            let bg = doc.background.map(|c| (c * 255.0).round() as u8);
            let mut composite: Vec<u8> =
                (0..doc.width() * doc.height()).flat_map(|_| [bg[0], bg[1], bg[2], 255]).collect();
            for layer in doc.layers().iter().filter(|l| l.kind == LayerKind::Paint && l.visible) {
                let Some((_, px)) = back.pixels.iter().find(|(id, _)| *id == layer.id) else { continue };
                for (d, s) in composite.as_chunks_mut::<4>().0.iter_mut().zip(px.as_chunks::<4>().0) {
                    let keep = 255 - u32::from(s[3]);
                    for c in 0..4 {
                        d[c] = (u32::from(s[c]) + (u32::from(d[c]) * keep + 127) / 255).min(255) as u8;
                    }
                }
            }
            let preview = ZipReader::new(&bytes).unwrap().read("QuickLook/Thumbnail.png").unwrap();
            let pixels = back.pixels.iter().cloned().collect();
            let package = write(&Export {
                document: doc,
                pixels: &pixels,
                composite: &composite,
                thumbnail_png: &preview,
                name: &back.name,
                author: &back.author,
                strokes: back.strokes,
                tracked_seconds: back.tracked_seconds,
                seed: 7,
                time_lapse: None,
                reference: None,
                reference_picture: None,
            });
            std::fs::write(out, package).unwrap();
        }
    }
}
