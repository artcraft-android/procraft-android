//! Reading Krita brushes: a preset (`.kpp`) is a PNG whose `preset` text holds the brush's
//! settings as XML, its tip either drawn from a few numbers (an auto brush) or a picture it names,
//! and maybe a texture it names. A newer preset carries the pictures it names itself: a `resources`
//! element holds each as base64, with its kind (`brushes`, `patterns`), file name, name and MD5
//! sum. A bundle (`.bundle`) is a zip of presets (`paintoppresets/`) and the pictures they name
//! (`brushes/`: GIMP brushes and pipes, PNGs, ABR files; `patterns/`: PNGs and GIMP patterns), its
//! manifest giving each file's MD5 sum.

use std::cell::Cell;

use crate::abr::AbrTip;
use crate::geom::Vec2;
use crate::xml::{self, Element};
use crate::zip::ZipReader;

/// A brush as a Krita preset has it.
#[derive(Clone, Debug, PartialEq)]
pub struct KritaBrush {
    pub name: String,
    pub tip: KritaTip,
    /// Space between stamps as a share of the size, if the preset sets it (not automatic).
    pub spacing: Option<f32>,
    /// The tip's own angle, radians.
    pub angle: f32,
    /// The tip turns the way the stroke goes.
    pub follows: bool,
    /// Each stamp turns at random: a round tip drawn with holes or noise in it, which Krita makes
    /// afresh for each stamp.
    pub turns_at_random: bool,
    /// Whether pen pressure changes the size and the opacity.
    pub pressure_size: bool,
    pub pressure_opacity: bool,
    /// The texture it paints through, if it has one.
    pub texture: Option<KritaTexture>,
    /// The pictures it names that neither it nor its bundle holds in a form we read (pictures that
    /// came with Krita, say), by file name: those come round, or without the texture.
    pub missing: Vec<String>,
}

/// A preset's texture: its picture, its scale (1 the picture's own size), how strongly it shows (0
/// to 1) and whether it is inverted.
#[derive(Clone, Debug, PartialEq)]
pub struct KritaTexture {
    pub picture: KritaPicture,
    pub scale: f32,
    pub strength: f32,
    pub invert: bool,
}

/// A texture's picture.
#[derive(Clone, Debug, PartialEq)]
pub enum KritaPicture {
    /// A PNG or JPEG, to decode.
    Encoded(Vec<u8>),
    /// Grey already (a GIMP pattern's lightness), light letting paint through.
    Grey(AbrTip),
}

/// What a preset's tip is.
#[derive(Clone, Debug, PartialEq)]
pub enum KritaTip {
    /// A round tip: its edge (0 soft to 1 hard) and how round it is (1 a circle).
    Round { hardness: f32, roundness: f32 },
    /// A grey image (white paints).
    Image(AbrTip),
    /// A PNG to read as one (dark and opaque paints).
    Png(Vec<u8>),
}

/// Where a picture a preset names is kept: its tip among the brushes, its texture among the
/// patterns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Tip,
    Pattern,
}

impl Kind {
    /// The folder (and resource type) pictures of this kind are kept in.
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Tip => "brushes",
            Kind::Pattern => "patterns",
        }
    }
}

/// A picture a preset names: by its file's name (perhaps with folders), its own name and its MD5
/// sum (hex), any of them blank when the preset doesn't say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wanted<'a> {
    pub kind: Kind,
    pub file: &'a str,
    pub name: &'a str,
    pub md5: &'a str,
}

impl Wanted<'_> {
    /// How surely a picture kept in `folder` with file name `file`, name `name` and MD5 sum `md5`
    /// is this one: 3 by its sum, 2 by its file's name (folders left out, in any case), 1 by its
    /// name (or its file's without the ending), 0 not at all (or another kind).
    pub fn score(&self, folder: &str, file: &str, name: &str, md5: &str) -> u8 {
        if !folder.eq_ignore_ascii_case(self.kind.folder()) {
            return 0;
        }
        let last = |f: &'_ str| f.rsplit(['/', '\\']).next().unwrap_or_default().to_owned();
        let same = |a: &str, b: &str| !a.trim().is_empty() && a.trim().eq_ignore_ascii_case(b.trim());
        let stem = |f: &str| f.rsplit_once('.').map_or(f, |(stem, _)| stem).to_owned();
        if same(self.md5, md5) {
            3
        } else if same(&last(self.file), &last(file)) {
            2
        } else if same(self.name, name) || same(self.name, &stem(&last(file))) {
            1
        } else {
            0
        }
    }
}

/// The most presets read from a bundle, and the most bytes the tip files and textures they take
/// from it may come to together.
const MOST_PRESETS: usize = 512;
const MOST_TIP_BYTES: usize = 256 << 20;
/// The most a preset's text inflates to (its pictures in it as base64: a real one is 76 MB), and
/// all of a bundle's presets' together.
const MOST_TEXT: usize = 256 << 20;
const MOST_BUNDLE_TEXT: usize = 1 << 30;
/// The side an auto brush's tip is drawn at, when it is drawn.
const DRAWN_SIDE: usize = crate::textures::TEXTURE_SIZE;
/// The most points an auto brush's tip is drawn with.
const MOST_SPIKES: u32 = 32;
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const JPEG: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// The preset in `kpp`, the pictures it names and doesn't hold looked up with `file`; none if it
/// isn't one.
pub fn read_preset(kpp: &[u8], file: &dyn Fn(&Wanted<'_>) -> Option<Vec<u8>>) -> Option<KritaBrush> {
    preset(kpp, file, MOST_TEXT).map(|(brush, _)| brush)
}

/// [`read_preset`], its text inflating to no more than `most_text`; with the text's length.
fn preset(kpp: &[u8], file: &dyn Fn(&Wanted<'_>) -> Option<Vec<u8>>, most_text: usize) -> Option<(KritaBrush, usize)> {
    let (root, length) = {
        let text = texts(kpp, most_text).into_iter().find(|(k, _)| k == "preset")?.1;
        (xml::parse(&text)?, text.len())
    };
    if root.name != "Preset" {
        return None;
    }
    let params: Vec<(&str, &str)> =
        root.all("param").into_iter().filter_map(|p| Some((p.attribute("name")?, p.text.trim()))).collect();
    let param = |name: &str| params.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
    // the pictures it holds itself, and a picture it names: its own copy, else its bundle's
    let held: Vec<&Element> = root.child("resources").map_or_else(Vec::new, |r| {
        r.children.iter().filter(|c| c.name.rsplit(':').next() == Some("resource")).collect()
    });
    let find = |w: &Wanted<'_>| {
        let at = |e: &&Element, key: &str| e.attribute(key).unwrap_or_default().to_owned();
        let own = best(
            held.iter().map(|e| (w.score(&at(e, "type"), &at(e, "filename"), &at(e, "name"), &at(e, "md5sum")), *e)),
        );
        own.and_then(|e| base64(&e.text)).or_else(|| file(w))
    };
    let mut missing = Vec::new();
    let definition = param("brush_definition").and_then(xml::parse);
    let brush = definition.as_ref().and_then(|d| d.find("Brush"));
    let number = |e: Option<&Element>, key: &str| e.and_then(|e| e.attribute(key)).and_then(real);
    let automatic = brush.and_then(|b| b.attribute("useAutoSpacing")).is_some_and(|v| v == "1" || v == "true");
    let (tip, turns_at_random) = brush
        .and_then(|b| tip(b, &find, &mut missing))
        .unwrap_or((KritaTip::Round { hardness: 1.0, roundness: 1.0 }, false));
    let flag = |name: &str| param(name).is_some_and(|v| v == "true");
    let value = |name: &str| param(name).and_then(real);
    // the texture's picture: in the preset as the base64 of its base64 (older ones); else one it
    // names, by its file's name, its own and its sum
    let inline = || {
        let once = base64(param("Texture/Pattern/Pattern")?)?;
        if once.starts_with(&PNG) {
            return Some(once);
        }
        base64(&String::from_utf8_lossy(&once)).filter(|p| p.starts_with(&PNG))
    };
    let texture = flag("Texture/Pattern/Enabled")
        .then(|| {
            if let Some(png) = inline() {
                return Some(KritaPicture::Encoded(png));
            }
            let name = param("Texture/Pattern/Name").unwrap_or_default();
            let wanted = Wanted {
                kind: Kind::Pattern,
                file: param("Texture/Pattern/PatternFileName").filter(|f| !f.is_empty()).unwrap_or(name),
                name,
                md5: param("Texture/Pattern/PatternMD5Sum").unwrap_or_default(),
            };
            let picture = find(&wanted).and_then(texture_picture);
            if picture.is_none() && !(wanted.file.is_empty() && wanted.md5.is_empty()) {
                missing.push(if wanted.file.is_empty() { wanted.md5 } else { wanted.file }.to_owned());
            }
            picture
        })
        .flatten()
        .map(|picture| KritaTexture {
            picture,
            scale: value("Texture/Pattern/Scale").filter(|s| *s > 0.0).unwrap_or(1.0),
            strength: value("Texture/Pattern/Strength")
                .or_else(|| value("Texture/Strength/Value"))
                .unwrap_or(1.0)
                .clamp(0.0, 1.0),
            invert: flag("Texture/Pattern/Invert"),
        });
    let read = KritaBrush {
        name: root.attribute("name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Krita Brush").to_owned(),
        tip,
        spacing: if automatic { None } else { number(brush, "spacing").filter(|s| *s > 0.0) },
        angle: number(brush, "angle").unwrap_or(0.0),
        // the tip's rotation by the Drawing Angle sensor, alone or among others
        follows: flag("PressureRotation") && param("RotationSensor").is_some_and(|s| s.contains("drawingangle")),
        turns_at_random,
        pressure_size: param("PressureSize").is_none_or(|v| v == "true"),
        pressure_opacity: param("PressureOpacity").is_some_and(|v| v == "true"),
        texture,
        missing,
    };
    Some((read, length))
}

/// A number as a preset keeps it, a comma for the point where the app that saved it used one.
fn real(text: &str) -> Option<f32> {
    text.trim().replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite())
}

/// Of `items` with how surely each is the one wanted, the surest (the first of those as sure);
/// none if none is at all.
fn best<T>(items: impl Iterator<Item = (u8, T)>) -> Option<T> {
    let mut found: Option<(u8, T)> = None;
    for (score, item) in items {
        if score > 0 && found.as_ref().is_none_or(|(sure, _)| score > *sure) {
            found = Some((score, item));
        }
    }
    found.map(|(_, item)| item)
}

/// The bytes in base64 `text`, spaces and line breaks anywhere left out.
fn base64(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.contains(|c: char| c.is_ascii_whitespace()) {
        return crate::base64::decode(&text.split_ascii_whitespace().collect::<String>());
    }
    crate::base64::decode(text)
}

/// A texture's picture from its file: a PNG or JPEG, or a GIMP pattern.
fn texture_picture(bytes: Vec<u8>) -> Option<KritaPicture> {
    if bytes.starts_with(&PNG) || bytes.starts_with(&JPEG) {
        return Some(KritaPicture::Encoded(bytes));
    }
    crate::gbr::read_pattern(&bytes).map(KritaPicture::Grey)
}

/// The presets in a bundle, in its order, with its name; none if it isn't one.
pub fn read_bundle(bytes: &[u8]) -> Option<(String, Vec<KritaBrush>)> {
    let zip = ZipReader::new(bytes)?;
    let names: Vec<String> = zip.names().map(str::to_owned).collect();
    let is_bundle = zip.read("mimetype").is_some_and(|m| m.starts_with(b"application/x-krita-resourcebundle"))
        || names.iter().any(|n| n == "META-INF/manifest.xml");
    if !is_bundle {
        return None;
    }
    let title = zip
        .read("meta.xml")
        .and_then(|m| xml::parse(&String::from_utf8_lossy(&m)))
        .and_then(|m| ["title", "name"].iter().find_map(|k| m.find(k).map(|e| e.text.trim().to_owned())))
        .filter(|t| !t.is_empty())
        .unwrap_or_default();
    // each file's MD5 sum, as the manifest gives it
    let sums: Vec<(String, String)> = zip
        .read("META-INF/manifest.xml")
        .and_then(|m| xml::parse(&String::from_utf8_lossy(&m)))
        .map(|m| {
            m.all("file-entry")
                .into_iter()
                .filter_map(|e| Some((e.attribute("full-path")?.to_owned(), e.attribute("md5sum")?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let sum = |name: &str| sums.iter().find(|(path, _)| path == name).map_or("", |(_, sum)| sum.as_str());
    // a picture a preset names and doesn't hold: in the brushes or patterns folder, by its sum, its
    // file's name or its own; each preset has its own copy, so all of them together may be only
    // so big (round tips and no textures after that), and the presets' text too
    let left = Cell::new(MOST_TIP_BYTES);
    let take = |n: usize| left.get().checked_sub(n).map(|l| left.set(l)).is_some();
    let file = |w: &Wanted<'_>| {
        let name = best(names.iter().filter_map(|n| {
            let (top, _) = n.split_once('/')?;
            Some((w.score(top, n, "", sum(n)), n))
        }))?;
        take(zip.size(name)?).then(|| zip.read(name)).flatten()
    };
    let mut text_left = MOST_BUNDLE_TEXT;
    let mut presets = Vec::new();
    let kept = |n: &&String| n.starts_with("paintoppresets/") && n.to_ascii_lowercase().ends_with(".kpp");
    for n in names.iter().filter(kept).take(MOST_PRESETS) {
        let Some((mut p, length)) = zip.read(n).and_then(|kpp| preset(&kpp, &file, text_left.min(MOST_TEXT))) else {
            continue;
        };
        text_left = text_left.saturating_sub(length);
        // its tip and texture count as well (one from the bundle twice over: a rough budget)
        let tip_bytes = match &p.tip {
            KritaTip::Round { .. } => 0,
            KritaTip::Image(t) => t.pixels.len(),
            KritaTip::Png(png) => png.len(),
        };
        if !take(tip_bytes) {
            p.tip = KritaTip::Round { hardness: 1.0, roundness: 1.0 };
        }
        let texture_bytes = p.texture.as_ref().map_or(0, |t| match &t.picture {
            KritaPicture::Encoded(png) => png.len(),
            KritaPicture::Grey(g) => g.pixels.len(),
        });
        if !take(texture_bytes) {
            p.texture = None;
        }
        presets.push(p);
    }
    (!presets.is_empty()).then_some((title, presets))
}

/// The tip a `Brush` element describes, and whether each stamp turns at random; none (round) if
/// it names a picture that can't be had, whose name goes in `missing`.
fn tip(
    brush: &Element,
    find: &dyn Fn(&Wanted<'_>) -> Option<Vec<u8>>,
    missing: &mut Vec<String>,
) -> Option<(KritaTip, bool)> {
    if brush.attribute("type")? == "auto_brush" {
        return auto_tip(brush);
    }
    let file = brush.attribute("filename")?;
    let wanted = Wanted {
        kind: Kind::Tip,
        file,
        name: brush.attribute("name").unwrap_or_default(),
        md5: brush.attribute("md5sum").unwrap_or_default(),
    };
    let tip = find(&wanted).and_then(|bytes| picture_tip(bytes, file, brush.attribute("name")));
    if tip.is_none() {
        missing.push(file.to_owned());
    }
    tip.map(|t| (t, false))
}

/// A tip from its picture's file (named `file`): a PNG, an ABR file's tip called `name` (else its
/// first), a GIMP brush or a pipe's first.
fn picture_tip(bytes: Vec<u8>, file: &str, name: Option<&str>) -> Option<KritaTip> {
    if bytes.starts_with(&PNG) {
        return Some(KritaTip::Png(bytes));
    }
    let abr = || {
        let tips = crate::abr::read(&bytes)?;
        let at = tips.iter().position(|t| Some(t.name.as_str()) == name).unwrap_or(0);
        tips.into_iter().nth(at)
    };
    let tip =
        if file.to_ascii_lowercase().ends_with(".abr") { abr() } else { crate::gbr::read_first(&bytes).or_else(abr) };
    tip.map(KritaTip::Image)
}

/// How an auto brush's paint falls off from its middle to its edge.
#[derive(Clone, Debug, PartialEq)]
enum Falloff {
    /// Solid to its fade's share of the way out, then down in a straight line.
    Fade,
    /// Blurred: a little at a fade of 1, a bell over the whole tip at 0.
    Blur,
    /// As its softness curve says: how much paints at each share of the way out.
    Curve(crate::curve::Curve),
}

/// An auto brush's mask: its shape, its falloff, how tall it is for its width (`ratio`), its fades
/// across and up (the share of the way out that is solid), how many points it has (two is the
/// plain shape), the share of its pixels kept (`density`) and how much noise is in it.
#[derive(Clone, Debug, PartialEq)]
struct Mask {
    rectangle: bool,
    falloff: Falloff,
    ratio: f32,
    fades: (f32, f32),
    spikes: u32,
    density: f32,
    randomness: f32,
}

/// An auto brush's tip: a plain circle is our round tip, its fade the hardness and its ratio the
/// roundness; anything else is drawn (a rectangle, points, a softness curve, holes, noise). With
/// whether each stamp turns at random (a circle with holes or noise, which Krita scatters afresh
/// each stamp).
fn auto_tip(brush: &Element) -> Option<(KritaTip, bool)> {
    let mask = brush.find("MaskGenerator")?;
    let number = |e: &Element, key: &str, or: f32| e.attribute(key).and_then(real).unwrap_or(or);
    let falloff = match mask.attribute("id") {
        Some("gauss") => Falloff::Blur,
        Some("soft") => {
            // "x,y;x,y;..."
            let points: Vec<Vec2> = mask
                .attribute("softness_curve")
                .unwrap_or_default()
                .split(';')
                .filter_map(|p| {
                    let (x, y) = p.split_once(',')?;
                    Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
                })
                .collect();
            if points.len() >= 2 { Falloff::Curve(crate::curve::Curve::new(&points)) } else { Falloff::Fade }
        }
        _ => Falloff::Fade,
    };
    let m = Mask {
        rectangle: matches!(mask.attribute("type"), Some("rect" | "rct")),
        falloff,
        ratio: number(mask, "ratio", 1.0).clamp(0.01, 1.0),
        fades: (number(mask, "hfade", 1.0).clamp(0.0, 1.0), number(mask, "vfade", 1.0).clamp(0.0, 1.0)),
        spikes: (number(mask, "spikes", 2.0).round().clamp(2.0, MOST_SPIKES as f32)) as u32,
        density: number(brush, "density", 1.0).clamp(0.0, 1.0),
        randomness: number(brush, "randomness", 0.0).clamp(0.0, 1.0),
    };
    let round = !m.rectangle && m.ratio >= 0.999;
    let speckled = m.density < 1.0 || m.randomness > 0.0;
    let plain = !m.rectangle && (m.spikes == 2 || round) && !speckled;
    let fade = (m.fades.0 + m.fades.1) / 2.0;
    Some(match (&m.falloff, plain) {
        (Falloff::Fade, true) => (KritaTip::Round { hardness: fade, roundness: m.ratio }, false),
        // our round tip paints half no nearer the middle than halfway out: a softer blur is drawn
        (Falloff::Blur, true) if blur_half(fade) >= 0.5 => {
            (KritaTip::Round { hardness: 2.0 * blur_half(fade) - 1.0, roundness: m.ratio }, false)
        }
        _ => (KritaTip::Image(draw(&m)), round && speckled),
    })
}

/// An auto brush's tip drawn, white for paint: the shape as wide as the picture and `ratio` as tall
/// (with more than two points, a square star of them, `ratio` thinning each), each pixel by how far
/// out it is.
fn draw(m: &Mask) -> AbrTip {
    let n = DRAWN_SIDE;
    let squeeze = if m.spikes == 2 { m.ratio } else { 1.0 };
    let (w, h) = (n, ((n as f32 * squeeze).round() as usize).max(1));
    let slice = std::f32::consts::TAU / m.spikes as f32;
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        let py = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
        for x in 0..w {
            let px = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            // a point's angle folded into the first half of its point's slice, its distance up
            // from the slice's middle taken 1/ratio times
            let (u, v) = if m.spikes == 2 {
                (px, py)
            } else {
                let folded = py.atan2(px).rem_euclid(slice);
                let folded = folded.min(slice - folded);
                let r = px.hypot(py);
                (r * folded.cos(), r * folded.sin() / m.ratio)
            };
            let mut value = paint_at(m, u, v);
            // holes and noise, the same each time it is drawn
            let (keep, noise) = (unit_noise(x, y, 1), unit_noise(x, y, 2));
            if keep >= m.density {
                value = 0.0;
            }
            value *= 1.0 - m.randomness * noise;
            pixels.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    AbrTip { name: String::new(), spacing: None, width: w as u32, height: h as u32, pixels }
}

/// How much an auto brush paints at `(u, v)`, the shape's edge at 1 across and up.
fn paint_at(m: &Mask, u: f32, v: f32) -> f32 {
    let (hfade, vfade) = m.fades;
    if m.rectangle {
        let (u, v) = (u.abs(), v.abs());
        return match &m.falloff {
            Falloff::Fade => straight(u, hfade) * straight(v, vfade),
            Falloff::Blur => blurred(u, hfade) * blurred(v, vfade),
            Falloff::Curve(c) if u.max(v) <= 1.0 => c.eval(u.max(v)),
            Falloff::Curve(_) => 0.0,
        };
    }
    let out = u.hypot(v);
    match &m.falloff {
        // solid inside the ellipse of the fades, down in a straight line to the edge along each ray
        Falloff::Fade => {
            if out <= 0.0 {
                return 1.0;
            }
            let inner = 1.0 / (u / (out * hfade.max(1e-4))).hypot(v / (out * vfade.max(1e-4)));
            straight(out, inner)
        }
        Falloff::Blur => blurred(out, (hfade + vfade) / 2.0),
        Falloff::Curve(c) if out <= 1.0 => c.eval(out),
        Falloff::Curve(_) => 0.0,
    }
}

/// Solid to `inner` of the way out, then down in a straight line to nothing at 1.
fn straight(t: f32, inner: f32) -> f32 {
    if t <= inner {
        1.0
    } else if inner >= 1.0 {
        0.0
    } else {
        ((1.0 - t) / (1.0 - inner)).clamp(0.0, 1.0)
    }
}

/// A blurred edge's paint at `t` of the way out: a disc reaching 0.9 of the way at a fade of 1
/// (softened over a tenth), blurred more as the fade falls, to a bell over the whole tip at 0;
/// nothing left at the edge.
fn blurred(t: f32, fade: f32) -> f32 {
    let edge = (0.9 * fade).max(0.05);
    let blur = (1.0 - edge) / 2.2;
    let at = |t: f32| erf((edge - t) / blur) + erf((edge + t) / blur);
    (at(t) / at(0.0)).clamp(0.0, 1.0)
}

/// How far out a blurred edge at `fade` ([`blurred`]) paints half. Our round tip paints half
/// halfway between its hardness and its edge, so a hardness of twice that less 1 matches it.
fn blur_half(fade: f32) -> f32 {
    (0..=200).map(|i| i as f32 / 200.0).find(|t| blurred(*t, fade) <= 0.5).unwrap_or(1.0)
}

/// The error function, to within about 1e-7 (Abramowitz and Stegun's 7.1.26).
fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t * (0.254_829_6 + t * (-0.284_496_7 + t * (1.421_413_8 + t * (-1.453_152 + t * 1.061_405_4))));
    let e = 1.0 - poly * (-x * x).exp();
    if x < 0.0 { -e } else { e }
}

/// A number from 0 to 1 for pixel `(x, y)`, the same every time; `salt` gives another one.
fn unit_noise(x: usize, y: usize, salt: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ salt;
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// A PNG's text chunks (`tEXt`, `zTXt`, `iTXt`), keyword and text; none if it isn't a PNG.
pub fn png_text(png: &[u8]) -> Vec<(String, String)> {
    texts(png, MOST_TEXT)
}

/// [`png_text`], each text inflating to no more than `most`.
fn texts(png: &[u8], most: usize) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if png.get(..8) != Some(&PNG[..]) {
        return out;
    }
    let inflate = |data: &[u8]| miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, most).ok();
    let mut at = 8usize;
    while let Some(len) = png.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes) {
        let Some(kind) = png.get(at + 4..at + 8) else { break };
        let len = len as usize;
        let Some(data) = png.get(at + 8..(at + 8).saturating_add(len)) else { break };
        at = at.saturating_add(12).saturating_add(len);
        let (keyword, rest) = match data.iter().position(|&b| b == 0) {
            Some(z) => (
                String::from_utf8_lossy(data.get(..z).unwrap_or_default()).into_owned(),
                data.get(z + 1..).unwrap_or_default(),
            ),
            None => continue,
        };
        // tEXt and zTXt are Latin-1, iTXt UTF-8
        let latin1 = |b: &[u8]| b.iter().map(|&b| char::from(b)).collect::<String>();
        let text = match kind {
            b"tEXt" => Some(latin1(rest)),
            b"zTXt" => rest.get(1..).and_then(inflate).map(|t| latin1(&t)),
            b"iTXt" => {
                // compressed or not, the method, then the language and translated keyword
                let (Some(&compressed), Some(body)) = (rest.first(), rest.get(2..)) else { continue };
                let mut parts = body.splitn(3, |&b| b == 0);
                let (_, _, text) = (parts.next(), parts.next(), parts.next().unwrap_or_default());
                let utf8 = if compressed == 1 { inflate(text) } else { Some(text.to_vec()) };
                utf8.map(|t| {
                    String::from_utf8(t).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
                })
            }
            b"IEND" => break,
            _ => None,
        };
        if let Some(t) = text {
            out.push((keyword, t));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::ZipWriter;

    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&[0; 4]);
        out
    }

    /// A preset's PNG: no picture to speak of, its XML in a compressed text chunk.
    fn kpp(xml: &str) -> Vec<u8> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
        png.extend(chunk(b"tEXt", b"version\x005.0"));
        let mut z = b"preset\0\0".to_vec();
        z.extend(miniz_oxide::deflate::compress_to_vec_zlib(xml.as_bytes(), 1));
        png.extend(chunk(b"zTXt", &z));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    fn preset(name: &str, brush: &str, pressure_size: bool) -> String {
        format!(
            r#"<Preset paintopid="paintbrush" name="{name}"><param type="string" name="PressureSize"><![CDATA[{pressure_size}]]></param><param type="string" name="PressureOpacity"><![CDATA[true]]></param><param type="string" name="brush_definition"><![CDATA[{brush}]]></param></Preset>"#
        )
    }

    fn param(name: &str, value: &str) -> String {
        format!(r#"<param type="string" name="{name}"><![CDATA[{value}]]></param>"#)
    }

    /// A GIMP brush, `width` x 1, grey.
    fn gbr(pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [28u32 + 1, 2, pixels.len() as u32, 1, 1] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(b"GIMP");
        out.extend_from_slice(&10u32.to_be_bytes());
        out.push(0);
        out.extend_from_slice(pixels);
        out
    }

    /// A GIMP pattern, 2 x 1, RGB.
    fn pat(pixels: [u8; 6]) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [24u32 + 1, 1, 2, 1, 3] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(b"GPAT");
        out.push(0);
        out.extend_from_slice(&pixels);
        out
    }

    fn drawn(xml_brush: &str) -> (AbrTip, bool) {
        let b = read_preset(&kpp(&preset("Drawn", xml_brush, true)), &|_| None).unwrap();
        let KritaTip::Image(t) = b.tip else { panic!("{:?}", b.tip) };
        (t, b.turns_at_random)
    }

    #[test]
    fn a_tip_turned_by_the_drawing_angle_follows_the_stroke() {
        let brush = r#"<Brush type="auto_brush" spacing="0.1" useAutoSpacing="0"><MaskGenerator type="circle" diameter="40"/></Brush>"#;
        let with = |on: &str, sensor: &str| {
            let rotation = format!(
                r#"<param type="string" name="PressureRotation"><![CDATA[{on}]]></param><param type="string" name="RotationSensor"><![CDATA[<params id="{sensor}"/>]]></param></Preset>"#
            );
            read_preset(&kpp(&preset("Chain", brush, true).replace("</Preset>", &rotation)), &|_| None).unwrap().follows
        };
        assert!(with("true", "drawingangle"));
        // by another sensor, or with the option off, it holds still
        assert!(!with("true", "pressure"));
        assert!(!with("false", "drawingangle"));
    }

    #[test]
    fn an_auto_brush_preset_reads_as_a_round_tip() {
        let brush = r#"<Brush type="auto_brush" spacing="0.15" angle="0.5" useAutoSpacing="0"><MaskGenerator type="circle" diameter="40" ratio="0.5" hfade="0.2" vfade="0.4"/></Brush>"#;
        let b = read_preset(&kpp(&preset("Soft Ellipse", brush, false)), &|_| None).unwrap();
        assert_eq!(b.name, "Soft Ellipse");
        assert_eq!(b.spacing, Some(0.15));
        assert_eq!(b.angle, 0.5);
        assert!(!b.pressure_size && b.pressure_opacity && !b.turns_at_random && b.missing.is_empty());
        match b.tip {
            KritaTip::Round { hardness, roundness } => assert!((hardness - 0.3).abs() < 1e-6 && roundness == 0.5),
            t => panic!("{t:?}"),
        }
        // a rectangle is drawn, solid in the middle and gone at the edge
        let rect = r#"<Brush type="auto_brush" useAutoSpacing="1"><MaskGenerator type="rect" ratio="1" hfade="0.5" vfade="0.5"/></Brush>"#;
        let b = read_preset(&kpp(&preset("Square", rect, true)), &|_| None).unwrap();
        assert_eq!(b.spacing, None);
        let KritaTip::Image(t) = b.tip else { panic!() };
        let n = DRAWN_SIDE;
        assert_eq!((t.width as usize, t.height as usize), (n, n));
        assert_eq!(t.pixels[n / 2 * n + n / 2], 255);
        assert!(t.pixels[n / 2 * n] < 10);
        // its numbers may have been saved with commas
        let comma = r#"<Brush type="auto_brush" spacing="0,25" useAutoSpacing="0"><MaskGenerator type="circle" ratio="0,5" hfade="0,5" vfade="0,5"/></Brush>"#;
        let b = read_preset(&kpp(&preset("Comma", comma, true)), &|_| None).unwrap();
        assert_eq!((b.spacing, b.tip), (Some(0.25), KritaTip::Round { hardness: 0.5, roundness: 0.5 }));
        // so is one saved as "rct", half as tall
        let (t, _) = drawn(
            r#"<Brush type="auto_brush"><MaskGenerator type="rct" id="soft" ratio="0.5" softness_curve="0,1;1,0"/></Brush>"#,
        );
        assert_eq!((t.width as usize, t.height as usize), (n, n / 2));
        assert!(t.pixels[n / 4 * n + n / 2] > 250 && t.pixels[n / 4 * n] < 10);
    }

    #[test]
    fn an_auto_brushs_points_curve_holes_and_blur_are_its_shape() {
        let n = DRAWN_SIDE;
        let at = |t: &AbrTip, r: f32, degrees: f32| {
            let (s, c) = degrees.to_radians().sin_cos();
            let half = t.width as f32 / 2.0;
            let (x, y) = ((half + r * half * c) as usize, (half + r * half * s) as usize);
            t.pixels[y * t.width as usize + x]
        };
        // five points, thin: solid out along each, empty between two
        let (star, turns) = drawn(
            r#"<Brush type="auto_brush"><MaskGenerator type="circle" ratio="0.3" spikes="5" hfade="1" vfade="1"/></Brush>"#,
        );
        assert!(!turns);
        assert_eq!((star.width as usize, star.height as usize), (n, n));
        assert_eq!((at(&star, 0.9, 0.0), at(&star, 0.9, 72.0), at(&star, 0.6, 36.0)), (255, 255, 0));
        // with a ratio of 1 the points are a plain circle: our round tip
        let plain = r#"<Brush type="auto_brush"><MaskGenerator type="circle" ratio="1" spikes="9" hfade="0.8" vfade="0.8"/></Brush>"#;
        let b = read_preset(&kpp(&preset("Ink", plain, true)), &|_| None).unwrap();
        assert!(matches!(b.tip, KritaTip::Round { hardness, .. } if (hardness - 0.8).abs() < 1e-6));
        // a softness curve is how much paints at each share of the way out: a ring here
        let (ring, _) = drawn(
            r#"<Brush type="auto_brush"><MaskGenerator type="circle" id="soft" hfade="0" vfade="0" softness_curve="0,0;0.5,0;0.75,1;1,0"/></Brush>"#,
        );
        assert!(at(&ring, 0.0, 0.0) < 10 && at(&ring, 0.75, 0.0) > 240 && at(&ring, 0.99, 90.0) < 40);
        // holes: about the share it keeps, and the stamps turn (Krita's holes move each stamp)
        let (holes, turns) = drawn(
            r#"<Brush type="auto_brush" density="0.5"><MaskGenerator type="circle" hfade="1" vfade="1"/></Brush>"#,
        );
        assert!(turns);
        let inside: Vec<u8> = (0..n * n)
            .filter(|i| {
                let (x, y) = ((i % n) as f32 - n as f32 / 2.0, (i / n) as f32 - n as f32 / 2.0);
                x.hypot(y) < n as f32 * 0.4
            })
            .map(|i| holes.pixels[i])
            .collect();
        let kept = inside.iter().filter(|&&v| v == 255).count() as f32 / inside.len() as f32;
        assert!((kept - 0.5).abs() < 0.03, "{kept}");
        assert!(inside.iter().all(|&v| v == 0 || v == 255));
        // a blurred circle is our round tip, harder as its fade rises; blurred more, it is drawn
        let blur = |fade: &str| {
            let xml = format!(
                r#"<Brush type="auto_brush"><MaskGenerator type="circle" id="gauss" hfade="{fade}" vfade="{fade}"/></Brush>"#
            );
            read_preset(&kpp(&preset("Blur", &xml, true)), &|_| None).unwrap().tip
        };
        let hardness = |fade: &str| match blur(fade) {
            KritaTip::Round { hardness, .. } => hardness,
            t => panic!("{t:?}"),
        };
        assert!((hardness("1") - 0.8).abs() < 0.02, "{}", hardness("1"));
        assert!(hardness("0.8") > 0.3 && hardness("0.8") < hardness("1"));
        let KritaTip::Image(bell) = blur("0") else { panic!() };
        assert!(at(&bell, 0.0, 0.0) == 255 && at(&bell, 0.4, 0.0) < 170 && at(&bell, 0.98, 0.0) < 5);
        assert!(blurred(1.0, 0.0) < 0.01 && blurred(1.0, 1.0) < 0.01 && (blurred(0.9, 1.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_bundle_reads_its_presets_and_the_tips_they_name() {
        let mut z = ZipWriter::new();
        z.add("mimetype", b"application/x-krita-resourcebundle");
        z.add("META-INF/manifest.xml", br#"<manifest:manifest><manifest:file-entry manifest:full-path="brushes/Renamed.0001.gbr" manifest:md5sum="0123abcd"/></manifest:manifest>"#);
        z.add("meta.xml", b"<meta><dc:title>Charcoals</dc:title></meta>");
        z.add("brushes/Charcoal.gbr", &gbr(&[255, 128]));
        z.add("brushes/Renamed.0001.gbr", &gbr(&[7, 8, 9]));
        let named = r#"<Brush type="gbr_brush" filename="charcoal.gbr" spacing="0.3"/>"#;
        z.add("paintoppresets/charcoal.kpp", &kpp(&preset("Charcoal", named, true)));
        z.add("paintoppresets/broken.kpp", b"not a preset");
        // a file of another name, found by its sum
        let summed = r#"<Brush type="gbr_brush" filename="Original.gbr" md5sum="0123ABCD"/>"#;
        z.add("paintoppresets/summed.kpp", &kpp(&preset("Summed", summed, true)));
        let bundle = z.finish();
        let (title, presets) = read_bundle(&bundle).unwrap();
        assert_eq!(title, "Charcoals");
        assert_eq!(presets.len(), 2);
        let KritaTip::Image(t) = &presets[0].tip else { panic!("{:?}", presets[0].tip) };
        assert_eq!((t.width, t.height, t.pixels.as_slice()), (2, 1, &[255, 128][..]));
        assert_eq!(presets[0].spacing, Some(0.3));
        let KritaTip::Image(t) = &presets[1].tip else { panic!("{:?}", presets[1].tip) };
        assert_eq!(t.pixels, [7, 8, 9]);
        // a tip that isn't there: round, and said so
        let missing = r#"<Brush type="gbr_brush" filename="gone.gbr"/>"#;
        let b = read_preset(&kpp(&preset("Gone", missing, true)), &|_| None).unwrap();
        assert!(matches!(b.tip, KritaTip::Round { .. }));
        assert_eq!(b.missing, ["gone.gbr"]);
        crate::damaged::survives("Krita bundle", &bundle, |b| {
            let _ = read_bundle(b);
        });
    }

    #[test]
    fn a_newer_preset_carries_its_tip_and_texture_itself() {
        let tip = crate::base64::encode(&gbr(&[1, 2, 3]));
        let paper = crate::base64::encode(&pat([255, 255, 255, 0, 0, 0]));
        // as Krita 5 keeps them: a resources element first, each picture base64 with its facts
        let held = format!(
            r#"<resources><resource md5sum="aa11" filename="Grain.0001.pat" name="Grain" type="patterns"><![CDATA[{paper}]]></resource><resource md5sum="bb22" filename="Tip.gbr" name="Tip" type="brushes"><![CDATA[{tip}]]></resource></resources>"#
        );
        let brush = r#"<Brush md5sum="bb22" type="gbr_brush" filename="Tip.gbr" spacing="0.1" useAutoSpacing="0"/>"#;
        let textured = [
            param("Texture/Pattern/Enabled", "true"),
            param("Texture/Pattern/Name", "Grain"),
            param("Texture/Pattern/PatternFileName", "Grain.0001.pat"),
            param("Texture/Pattern/PatternMD5Sum", "aa11"),
            param("Texture/Pattern/Scale", "0.5"),
        ]
        .concat();
        let xml = preset("Gild", brush, true)
            .replace(r#"name="Gild">"#, &format!(r#"name="Gild" embedded_resources="2">{held}"#))
            .replace("</Preset>", &format!("{textured}</Preset>"));
        let b = read_preset(&kpp(&xml), &|_| None).unwrap();
        let KritaTip::Image(t) = &b.tip else { panic!("{:?}", b.tip) };
        assert_eq!(t.pixels, [1, 2, 3]);
        let texture = b.texture.as_ref().unwrap();
        let KritaPicture::Grey(g) = &texture.picture else { panic!("{texture:?}") };
        assert_eq!((g.width, g.pixels.as_slice(), texture.scale), (2, &[255, 0][..], 0.5));
        assert!(b.missing.is_empty());
        // its own copy comes before a bundle's
        let other = gbr(&[9]);
        let b = read_preset(&kpp(&xml), &|_| Some(other.clone())).unwrap();
        assert!(matches!(&b.tip, KritaTip::Image(t) if t.pixels == [1, 2, 3]));
        // a texture named by its file only, in a bundle's patterns
        let mut z = ZipWriter::new();
        z.add("mimetype", b"application/x-krita-resourcebundle");
        z.add("patterns/Grain.0001.pat", &pat([0, 0, 0, 255, 255, 255]));
        let named = preset("Plain", r#"<Brush type="auto_brush"><MaskGenerator type="circle"/></Brush>"#, true)
            .replace("</Preset>", &format!("{textured}</Preset>"));
        z.add("paintoppresets/plain.kpp", &kpp(&named));
        let (_, presets) = read_bundle(&z.finish()).unwrap();
        let KritaPicture::Grey(g) = &presets[0].texture.as_ref().unwrap().picture else { panic!() };
        assert_eq!(g.pixels, [0, 255]);
        crate::damaged::survives("Krita preset with its pictures", &kpp(&xml), |b| {
            let _ = read_preset(b, &|_| None);
        });
    }

    #[test]
    fn a_preset_holding_large_pictures_reads() {
        // its text over 16 MiB, as a real preset holding a big pipe is (76 MB)
        let big = "A".repeat(17 << 20);
        let xml = preset("Big", r#"<Brush type="gbr_brush" filename="Big.gih"/>"#, true).replace(
            r#"name="Big">"#,
            &format!(
                r#"name="Big"><resources><resource filename="Big.gih" type="brushes">{big}</resource></resources>"#
            ),
        );
        let b = read_preset(&kpp(&xml), &|_| None).unwrap();
        assert_eq!(b.name, "Big");
        // all zeros: not a brush the tip could be read from
        assert_eq!(b.missing, ["Big.gih"]);
    }

    #[test]
    fn a_texture_comes_from_the_preset_itself_or_the_bundle() {
        let round = r#"<Brush type="auto_brush"><MaskGenerator type="circle"/></Brush>"#;
        let with = |params: &str| {
            let xml = preset("Pencil", round, true);
            xml.replace("</Preset>", &format!("{params}</Preset>"))
        };
        let picture = [&PNG[..], b"rest of a picture"].concat();
        // kept in the preset as the base64 of its base64
        let twice = crate::base64::encode(crate::base64::encode(&picture).as_bytes());
        let xml = with(
            &[
                param("Texture/Pattern/Enabled", "true"),
                param("Texture/Pattern/Pattern", &twice),
                param("Texture/Pattern/Scale", "0.35"),
                param("Texture/Pattern/Strength", "0.8"),
                param("Texture/Pattern/Invert", "true"),
            ]
            .concat(),
        );
        let t = read_preset(&kpp(&xml), &|_| None).unwrap().texture.unwrap();
        assert_eq!(
            (t.picture, t.scale, t.strength, t.invert),
            (KritaPicture::Encoded(picture.clone()), 0.35, 0.8, true)
        );
        // turned off: none
        let off = with(&[param("Texture/Pattern/Enabled", "false"), param("Texture/Pattern/Pattern", &twice)].concat());
        assert_eq!(read_preset(&kpp(&off), &|_| None).unwrap().texture, None);
        // not in the preset: the one it names, from the bundle's patterns
        let named =
            with(&[param("Texture/Pattern/Enabled", "true"), param("Texture/Pattern/Name", "paper.png")].concat());
        let mut z = ZipWriter::new();
        z.add("mimetype", b"application/x-krita-resourcebundle");
        z.add("patterns/Paper.png", &picture);
        z.add("paintoppresets/pencil.kpp", &kpp(&named));
        let (_, presets) = read_bundle(&z.finish()).unwrap();
        let t = presets[0].texture.as_ref().unwrap();
        assert_eq!((&t.picture, t.scale, t.strength, t.invert), (&KritaPicture::Encoded(picture), 1.0, 1.0, false));
        // named and nowhere: none, and said so
        let b = read_preset(&kpp(&named), &|_| None).unwrap();
        assert_eq!((b.texture, b.missing), (None, vec!["paper.png".to_owned()]));
    }

    #[test]
    fn other_files_and_damaged_ones_are_not_brushes() {
        assert!(read_preset(b"nothing", &|_| None).is_none());
        assert!(read_bundle(b"nothing").is_none());
        let good = kpp(&preset("Ink", r#"<Brush type="auto_brush"><MaskGenerator type="circle"/></Brush>"#, true));
        crate::damaged::survives("Krita preset", &good, |b| {
            let _ = read_preset(b, &|_| None);
        });
    }
}
