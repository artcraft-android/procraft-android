//! The layered artwork package matched 1:1: a zip holding a keyed archive of the document
//! (`Document.archive`), a folder of LZ4 tiles per layer named by the layer's UUID, and a preview
//! (`QuickLook/Thumbnail.png`).
//!
//! Tiles are `tileSize` squares (smaller along the right and top edges), rows stored bottom-up,
//! pixels premultiplied RGBA (masks: one grey byte). A missing tile is empty. The canvas is stored
//! unturned; `orientation` (1 to 4) says how many quarter turns it is shown at.

use std::sync::Arc;

use crate::applz4;
use crate::cms::{CanvasIcc, IccProfile, Space};
use crate::document::{BlendMode, Document, Layer, LayerId, LayerKind};
use crate::keyed::{Archive, Builder, Object};
use crate::plist::Value;
use crate::profile::Profile;
use crate::zip::{ZipReader, ZipWriter};

/// The file extension of the package.
pub const EXTENSION: &str = "procreate";
/// The edge of a tile written.
const TILE: u32 = 256;

/// An artwork read from a package.
pub struct Imported {
    pub document: Document,
    /// Paint layers' pixels (premultiplied RGBA) and masks' (grey in every channel, opaque), by id.
    pub pixels: Vec<(LayerId, Vec<u8>)>,
    pub name: String,
    pub author: String,
    pub strokes: u64,
    pub tracked_seconds: f64,
}

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
    let orientation = root.int("orientation").filter(|o| (1..=4).contains(o)).unwrap_or(1);
    let turns = match orientation {
        2 => 3,
        3 => 2,
        4 => 1,
        _ => 0,
    };
    let flip_x = root.bool("flippedHorizontally").unwrap_or(false);
    let flip_y = root.bool("flippedVertically").unwrap_or(false);
    let (dw, dh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let canvas = Canvas { zip: &zip, w, h, tile, turns, flip_x, flip_y };

    let mut layers: Vec<Layer> = Vec::new();
    let mut pixels: Vec<(LayerId, Vec<u8>)> = Vec::new();
    let mut next = 1u64;
    let mut selected = None;
    let selected_uuid = root.object("selectedLayer").and_then(|l| l.string("UUID"));
    let mut items = root.array("unwrappedLayers");
    if items.is_empty() {
        items = root.array("layers");
    }
    // the archive lists the top first; ours goes bottom first
    for item in items.iter().rev() {
        add_item(&canvas, item, None, &mut layers, &mut pixels, &mut next, selected_uuid.as_deref(), &mut selected)?;
    }
    let mut document = Document::from_layers(dw, dh, layers, selected.unwrap_or(LayerId(1)));
    if let Some(c) = root.data("backgroundColor").and_then(floats4) {
        document.background = [c[0], c[1], c[2]].map(|v| v.clamp(0.0, 1.0));
    }
    document.background_visible = !root.bool("backgroundHidden").unwrap_or(false);
    if let Some(dpi) = root.real("SilicaDocumentArchiveDPIKey").filter(|d| d.is_finite() && *d > 0.0) {
        document.dpi = dpi as f32;
    }
    let kept = root.object("colorProfile");
    let profile = kept.as_ref().and_then(|p| p.string("SiColorProfileArchiveICCNameKey"));
    let icc = kept.as_ref().and_then(|p| p.data("SiColorProfileArchiveICCDataKey")).and_then(IccProfile::parse);
    let nearest = match profile.as_deref() {
        Some(p) if p.contains("P3") => Profile::DisplayP3,
        _ => Profile::Srgb,
    };
    match (profile.as_deref().and_then(Profile::from_name), icc) {
        (Some(builtin), _) => document.profile = builtin,
        // a CMYK canvas keeps its pixels in Display P3; an RGB profile of its own keeps them in it
        (None, Some(icc)) if icc.space == Space::Cmyk => {
            document.profile = Profile::DisplayP3;
            document.icc = Some(CanvasIcc(Arc::new(icc)));
        }
        (None, Some(icc)) if icc.space == Space::Rgb => {
            document.profile = nearest;
            document.icc = Some(CanvasIcc(Arc::new(icc)));
        }
        _ => document.profile = nearest,
    }
    let document = document.repaired();
    Ok(Imported {
        document,
        pixels,
        name: root.string("name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Untitled Artwork".to_owned()),
        author: root.string("authorName").unwrap_or_default(),
        strokes: root.int("strokeCount").and_then(|s| u64::try_from(s).ok()).unwrap_or(0),
        tracked_seconds: root
            .real("SilicaDocumentTrackedTimeKey")
            .filter(|t| t.is_finite() && *t >= 0.0)
            .unwrap_or(0.0),
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
}

impl Canvas<'_> {
    /// A layer's pixels, `bpp` bytes each, top row first, turned as the canvas is shown; where no
    /// tile was stored, every byte is `blank`.
    fn pixels(&self, uuid: &str, bpp: usize, blank: u8) -> Result<Vec<u8>, ReadError> {
        let (w, h, t) = (self.w as usize, self.h as usize, self.tile as usize);
        let mut out = vec![blank; w * h * bpp];
        let prefix = format!("{uuid}/");
        let names: Vec<String> =
            self.zip.names().filter(|n| n.starts_with(&prefix) && n.ends_with(".lz4")).map(str::to_owned).collect();
        for name in names {
            let Some((tx, ty)) = name
                .get(prefix.len()..name.len() - 4)
                .and_then(|s| s.split_once('~'))
                .and_then(|(x, y)| Some((x.parse::<usize>().ok()?, y.parse::<usize>().ok()?)))
            else {
                continue;
            };
            if tx * t >= w || ty * t >= h {
                continue;
            }
            let (tw, th) = (t.min(w - tx * t), t.min(h - ty * t));
            let Some(packed) = self.zip.read(&name) else { return Err(ReadError::Damaged("tile")) };
            let raw = applz4::decode(&packed, tw * th * bpp).ok_or(ReadError::Damaged("tile"))?;
            if raw.len() != tw * th * bpp {
                return Err(ReadError::Damaged("tile size"));
            }
            for r in 0..th {
                // rows are stored bottom-up
                let row = h - 1 - (ty * t + r);
                let src = raw.get(r * tw * bpp..(r + 1) * tw * bpp).ok_or(ReadError::Damaged("tile"))?;
                let at = (row * w + tx * t) * bpp;
                out.get_mut(at..at + tw * bpp).ok_or(ReadError::Damaged("tile"))?.copy_from_slice(src);
            }
        }
        Ok(orient(&out, w, h, bpp, self.turns, self.flip_x, self.flip_y))
    }
}

/// `src` (`w` × `h`, `bpp` bytes a pixel, top row first) turned `turns` quarter turns clockwise,
/// then mirrored.
fn orient(src: &[u8], w: usize, h: usize, bpp: usize, turns: u32, flip_x: bool, flip_y: bool) -> Vec<u8> {
    if turns == 0 && !flip_x && !flip_y {
        return src.to_vec();
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
    next: &mut u64,
    selected_uuid: Option<&str>,
    selected: &mut Option<LayerId>,
) -> Result<(), ReadError> {
    if layers.len() > 4096 {
        return Err(ReadError::TooLarge);
    }
    let mut id = || {
        let i = LayerId(*next);
        *next += 1;
        i
    };
    match item.class() {
        Some("SilicaGroup") | Some("ValkyrieGroup") => {
            let gid = id();
            // the group sits above what it holds
            for child in item.array("children").iter().rev() {
                add_item(canvas, child, Some(gid), layers, pixels, next, selected_uuid, selected)?;
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
            let uuid = item.string("UUID").unwrap_or_default();
            if selected_uuid == Some(uuid.as_str()) {
                *selected = Some(lid);
            }
            let blend = item
                .int("extendedBlend2")
                .or_else(|| item.int("extendedBlend"))
                .or_else(|| item.int("blend"))
                .unwrap_or(0);
            layers.push(Layer {
                id: lid,
                name: item.string("name").unwrap_or_else(|| "Layer".to_owned()),
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
                text: None,
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
/// and each mask's (grey in red), the flattened picture (premultiplied, background included) and
/// the preview PNG.
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
    let mut selected = Builder::null();
    let mut items = Vec::new();
    for id in tops {
        items.push(write_item(&mut b, &mut zip, &mut uuids, e, &doc_ref, id, &mut selected));
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
    // the canvas's settings; whether Animation Assist is on isn't written (its number unknown yet)
    let a = doc.animation.clone().unwrap_or_default();
    let playback = match a.playback {
        crate::animation::Playback::OneShot => 0,
        crate::animation::Playback::Loop => 1,
        crate::animation::Playback::PingPong => 2,
    };
    let animation = b.object(
        &["ValkyrieDocumentAnimation", "NSObject"],
        vec![
            ("animationMode", Value::Int(0)),
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
    let reference_kind = b.string("{0, 0}");
    let reference_size = b.string("{350, 532}");
    b.fill(
        &doc_ref,
        &["SilicaDocument", "ValkyrieDocument", "NSObject"],
        vec![
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
            ("faceBackgroundHidden", Value::Bool(false)),
            ("featureSet", Value::Int(1)),
            ("flippedHorizontally", Value::Bool(false)),
            ("flippedVertically", Value::Bool(false)),
            ("isFirstItemAnimationForeground", Value::Bool(false)),
            ("isLastItemAnimationBackground", Value::Bool(false)),
            ("layers", layers),
            ("legacyBehaviour", Value::Int(1)),
            ("mask", mask),
            ("name", name),
            ("orientation", Value::Int(1)),
            ("primaryItem", selected.clone()),
            ("reference3D", Value::Bool(false)),
            ("referenceAnchor", Value::Int(0)),
            ("referenceCorner", Value::Int(0)),
            ("referenceDocked", Value::Bool(false)),
            ("referenceKind", reference_kind),
            ("referenceOrientation", Value::Int(0)),
            ("referenceSize", reference_size),
            ("referenceVisibility", Value::Bool(false)),
            ("selectedLayer", selected),
            ("selectedSamplerLayer", Builder::null()),
            ("size", size),
            ("solo", Builder::null()),
            ("strokeCount", strokes),
            ("tileSize", Value::Int(i64::from(TILE))),
            ("unwrappedLayers", unwrapped),
            ("userSavedSelectionMasks", saved_masks),
            ("version", Value::Int(2)),
            ("videoEnabled", Value::Bool(false)),
        ],
    );
    zip.add("Document.archive", &b.finish(doc_ref));
    zip.add("QuickLook/Thumbnail.png", e.thumbnail_png);
    zip.finish()
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
    selected: &mut Value,
) -> Value {
    let doc = e.document;
    let (w, h) = (doc.width(), doc.height());
    let Some(l) = doc.layer(id) else { return Builder::null() };
    if l.kind == LayerKind::Group {
        let children: Vec<Value> =
            top_first(doc, Some(id)).into_iter().map(|c| write_item(b, zip, uuids, e, doc_ref, c, selected)).collect();
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
        let grey: Option<Vec<u8>> = e.pixels.get(&m).map(|px| px.as_chunks::<4>().0.iter().map(|&[r, ..]| r).collect());
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
    };
    let r = layer_record(b, doc_ref, &uuid, mask, rec, w, h);
    if id == doc.current() {
        *selected = r.clone();
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
    let pdf_transform = b.string("[0, 0, 0, 0, 0, 0]");
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
            ("text", Builder::null()),
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
    for ty in 0..h.div_ceil(t) {
        for tx in 0..w.div_ceil(t) {
            let (tw, th) = (t.min(w - tx * t), t.min(h - ty * t));
            let mut raw = Vec::with_capacity(tw * th * bpp);
            for r in 0..th {
                let row = h - 1 - (ty * t + r);
                let at = (row * w + tx * t) * bpp;
                raw.extend_from_slice(px.get(at..at + tw * bpp).unwrap_or(&[]));
            }
            if raw.len() != tw * th * bpp || raw.iter().all(|&v| v == blank) {
                continue;
            }
            zip.add(&format!("{uuid}/{tx}~{ty}.lz4"), &applz4::encode(&raw));
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
    }

    #[test]
    fn turned_canvases_are_shown_upright() {
        // 3 × 2, values = index; a quarter turn clockwise makes it 2 × 3
        let src: Vec<u8> = (0..6).collect();
        assert_eq!(orient(&src, 3, 2, 1, 1, false, false), [3, 0, 4, 1, 5, 2]);
        assert_eq!(orient(&src, 3, 2, 1, 2, false, false), [5, 4, 3, 2, 1, 0]);
        assert_eq!(orient(&src, 3, 2, 1, 3, false, false), [2, 5, 1, 4, 0, 3]);
        assert_eq!(orient(&src, 3, 2, 1, 0, true, false), [2, 1, 0, 5, 4, 3]);
        assert_eq!(parse_size("{1488, 2266}"), Some((1488, 2266)));
        assert_eq!(parse_size("1488x2266"), None);
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
            });
            std::fs::write(out, package).unwrap();
        }
    }
}
