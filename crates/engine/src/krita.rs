//! Reading Krita brushes: a preset (`.kpp`) is a PNG whose `preset` text holds the brush's
//! settings as XML, its tip either drawn from a few numbers (an auto brush) or a picture it names,
//! maybe a texture it names, and maybe a masking brush: a second brush along the same stroke, its
//! settings the same names under `MaskingBrush/Preset/`, its size a share of the main brush's and
//! its coverage combined with the main brush's by a composite op. A newer preset carries the
//! pictures it names itself: a `resources` element holds each as base64, with its kind
//! (`brushes`, `patterns`), file name, name and MD5 sum. A bundle (`.bundle`) is a zip of presets
//! (`paintoppresets/`) and the pictures they name (`brushes/`: GIMP brushes and pipes, PNGs, ABR
//! files; `patterns/`: PNGs and GIMP patterns), its manifest giving each file's MD5 sum.

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
    /// Each stamp turned at random as far as this share of a half turn either way (0 none).
    pub rotation_random: f32,
    /// Each stroke turned at random as it starts.
    pub rotation_per_stroke: bool,
    /// The tip turns the way the pen leans.
    pub follows_tilt: bool,
    /// Each stamp mirrored at random, across and up.
    pub mirror: (bool, bool),
    /// Its stamps scattered from the stroke, if they are.
    pub scatter: Option<KritaScatter>,
    /// Its colour changed at random, each stamp and each stroke: how far its hue turns (a share of
    /// a half turn either way), its saturation goes, and it lightens and darkens (0 to 1 each).
    pub color_random: [f32; 4],
    pub color_per_stroke: [f32; 4],
    /// Each stamp flattened at random as far as this (0 round always, 1 as far as flat), and by
    /// the pressure along its curve, if it is.
    pub ratio_random: f32,
    pub ratio_pressure: Option<Vec<Vec2>>,
    /// How much of its colour, not the background colour, pressure lets it paint (its amount
    /// along its curve), if pressure does: the rest is the background colour.
    pub mix_pressure: Option<Vec<Vec2>>,
    /// How pen pressure scales its size, opacity and flow (see [`pressure_curve`]): the curve,
    /// none where pressure doesn't.
    pub size_pressure: Option<Vec<Vec2>>,
    pub opacity_pressure: Option<Vec<Vec2>>,
    pub flow_pressure: Option<Vec<Vec2>>,
    /// Its opacity and flow (0 to 1).
    pub opacity: f32,
    pub flow: f32,
    /// Painted in wash mode (its stamps build up towards its opacity), as Krita takes a preset that
    /// doesn't say; else built up (each stamp lays its opacity times its flow).
    pub wash: bool,
    /// The texture it paints through, if it has one.
    pub texture: Option<KritaTexture>,
    /// The pictures it names that neither it nor its bundle holds in a form we read (pictures that
    /// came with Krita, say), by file name: those come round, or without the texture, or without
    /// the masking brush.
    pub missing: Vec<String>,
    /// Its masking brush, if it has one turned on whose tip can be had.
    pub masking: Option<Box<KritaMasking>>,
}

/// How a preset scatters its stamps: each as far as `amount` diameters (at most), at random, along
/// the stroke and across it as it says; as far as pressure lets it, by its curve, if it does.
#[derive(Clone, Debug, PartialEq)]
pub struct KritaScatter {
    pub amount: f32,
    pub pressure: Option<Vec<Vec2>>,
    pub along: bool,
    pub across: bool,
}

/// A preset's masking brush: a second brush painted along the same stroke, its coverage combined
/// with the main brush's by a composite op.
#[derive(Clone, Debug, PartialEq)]
pub struct KritaMasking {
    /// Its tip, spacing, angle and what pressure changes (no texture, no masking of its own).
    pub brush: KritaBrush,
    /// How it combines with the main brush, by Krita's name for the op (`multiply`, `subtract`).
    pub op: String,
    /// Its size for each 1 of the main brush's.
    pub size: f32,
    /// Its own opacity (the most its stroke reaches) and flow (what each stamp lays), 0 to 1, as
    /// Krita's wash mode (the only one it masks in) takes them.
    pub opacity: f32,
    pub flow: f32,
    /// How pen pressure scales each of those, if it does: a curve's points, the pressure across
    /// and the share kept up.
    pub opacity_pressure: Option<Vec<Vec2>>,
    pub flow_pressure: Option<Vec<Vec2>>,
}

/// A preset's texture: its picture, its scale (1 the picture's own size), how strongly it shows (0
/// to 1), how its picture becomes the mask each stamp is combined with
/// ([`KritaTexture::mask_value`]) and by what ([`KritaTexture::op`]).
#[derive(Clone, Debug, PartialEq)]
pub struct KritaTexture {
    pub picture: KritaPicture,
    pub scale: f32,
    pub strength: f32,
    /// The strength by the pen's pressure (points 0..1 from the left), if it follows it.
    pub strength_pressure: Option<Vec<Vec2>>,
    /// Krita's soft texturing: the strength takes the mask towards none (multiplied, a stamp
    /// times the mask taken towards 1; subtracted, the mask times the strength taken off), where
    /// otherwise it takes the paint (a stamp times the mask times the strength; a stamp less the
    /// mask and less what the strength falls short of 1).
    pub soft: bool,
    pub invert: bool,
    /// Krita's number for how the mask combines with a stamp (0 multiply, 1 subtract...).
    pub mode: u32,
    /// Taken off the mask (-1 to 1), and its contrast (1 as it is).
    pub brightness: f32,
    pub contrast: f32,
    /// The mask value taken as its middle (0.5 leaves it as it is).
    pub neutral: f32,
    /// Outside `left..=right` (0 to 1) the mask is none (policy 1) or whole (2); 0 keeps it.
    pub cutoff: (u32, f32, f32),
}

impl KritaTexture {
    /// The mask at a pixel of the picture `grey` light and `alpha` opaque (both 0 to 1), as Krita
    /// makes it: what is clear counts as white; then made lighter, of more contrast, inverted,
    /// its middle moved to the neutral point, and cut outside the cutoffs. 1 keeps a stamp's
    /// paint where it multiplies, and takes it all where it subtracts.
    pub fn mask_value(&self, grey: f32, alpha: f32) -> f32 {
        let alpha = alpha.clamp(0.0, 1.0);
        let lit = grey.clamp(0.0, 1.0) * alpha + (1.0 - alpha);
        let mut m = ((lit - self.brightness - 0.5) * self.contrast + 0.5).clamp(0.0, 1.0);
        if self.invert {
            m = 1.0 - m;
        }
        let n = self.neutral.clamp(0.0, 1.0);
        let m = if n >= 1.0 || (n > 0.0 && m <= n) { m / (2.0 * n) } else { 0.5 + (m - n) / (2.0 - 2.0 * n) };
        let (policy, left, right) = self.cutoff;
        let outside = m < left || m > right;
        match policy {
            1 if outside => 0.0,
            2 if outside => 1.0,
            _ => m.clamp(0.0, 1.0),
        }
    }

    /// Krita's name for how the mask combines with a stamp (as a masking brush's ops are named);
    /// none for the lightness and gradient modes, which colour the stamp instead.
    pub fn op(&self) -> Option<&'static str> {
        const OPS: [&str; 16] = [
            "multiply",
            "subtract",
            "",
            "",
            "darken",
            "overlay",
            "dodge",
            "burn",
            "linear_dodge",
            "linear_burn",
            "hard_mix_photoshop",
            "hard_mix_softer_photoshop",
            "height",
            "linear_height",
            "height_photoshop",
            "linear_height_photoshop",
        ];
        OPS.get(self.mode as usize).copied().filter(|op| !op.is_empty())
    }
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
    let name = root.attribute("name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Krita Brush");
    let mut read = brush_of(name, &param, &find);
    // the masking brush: its settings are the main brush's names under its own prefix. Without
    // its tip it is left off (a round stand-in would mask the stroke all over), and said so.
    let value = |name: &str| param(name).and_then(real);
    let masked = param("MaskingBrush/Enabled") == Some("true");
    let own = |key: &str| param(&format!("MaskingBrush/Preset/{key}"));
    if masked && own("brush_definition").and_then(xml::parse).is_some_and(|d| d.find("Brush").is_some()) {
        let brush = brush_of(name, &own, &find);
        if brush.missing.is_empty() {
            let unit = |key: &str| own(key).and_then(real).unwrap_or(1.0).clamp(0.0, 1.0);
            read.masking = Some(Box::new(KritaMasking {
                op: param("MaskingBrush/MaskingCompositeOp").unwrap_or("multiply").trim().to_owned(),
                size: value("MaskingBrush/MasterSizeCoeff").filter(|s| *s > 0.0).unwrap_or(1.0),
                opacity: unit("OpacityValue"),
                flow: unit("FlowValue"),
                opacity_pressure: pressure_curve(&own, "Opacity", false),
                flow_pressure: pressure_curve(&own, "Flow", false),
                brush,
            }));
        } else {
            read.missing.extend(brush.missing);
        }
    }
    Some((read, length))
}

/// The brush that the settings `param` gives describe, called `called`, the pictures it names
/// looked up with `find`. The masking brush's own settings are under a prefix: `param` takes the
/// names without it.
fn brush_of<'p>(
    called: &str,
    param: &dyn Fn(&str) -> Option<&'p str>,
    find: &dyn Fn(&Wanted<'_>) -> Option<Vec<u8>>,
) -> KritaBrush {
    let mut missing = Vec::new();
    let definition = param("brush_definition").and_then(xml::parse);
    let brush = definition.as_ref().and_then(|d| d.find("Brush"));
    let number = |e: Option<&Element>, key: &str| e.and_then(|e| e.attribute(key)).and_then(real);
    let automatic = brush.and_then(|b| b.attribute("useAutoSpacing")).is_some_and(|v| v == "1" || v == "true");
    let (tip, turns_at_random) = brush
        .and_then(|b| tip(b, find, &mut missing))
        .unwrap_or((KritaTip::Round { hardness: 1.0, roundness: 1.0 }, false));
    let flag = |name: &str| param(name).is_some_and(|v| v == "true");
    let value = |name: &str| param(name).and_then(real);
    // an option switched on with its pen settings, and driven by a sensor
    let on = |option: &str| flag(&format!("Pressure{option}")) && param(&format!("{option}UseCurve")) != Some("false");
    let driven = |option: &str, id: &str| on(option) && sensors_of(param, option).contains(&id);
    let turned = value("RotationValue").unwrap_or(1.0).clamp(0.0, 1.0);
    let amount = |option: &str| value(&format!("{option}Value")).unwrap_or(1.0).clamp(0.0, 1.0);
    // the hue turned, the saturation, the value (lighter or darker) and Darken (darker), each
    // as far as its amount where `id` drives it
    let colours = |id: &str| {
        let by = |option: &str| if driven(option, id) { amount(option) } else { 0.0 };
        let value = by("v");
        [by("h"), by("s"), value, value.max(by("Darken"))]
    };
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
            strength_pressure: pressure_curve(param, "Texture/Strength/", true),
            soft: flag("Texture/Pattern/UseSoftTexturing"),
            invert: flag("Texture/Pattern/Invert"),
            mode: value("Texture/Pattern/TexturingMode").map_or(0, |m| m.clamp(0.0, 255.0) as u32),
            brightness: value("Texture/Pattern/Brightness").unwrap_or(0.0).clamp(-1.0, 1.0),
            contrast: value("Texture/Pattern/Contrast").unwrap_or(1.0).clamp(0.0, 16.0),
            neutral: value("Texture/Pattern/NeutralPoint").unwrap_or(0.5).clamp(0.0, 1.0),
            cutoff: (
                value("Texture/Pattern/CutoffPolicy").map_or(0, |p| p.clamp(0.0, 2.0) as u32),
                value("Texture/Pattern/CutoffLeft").unwrap_or(0.0) / 255.0,
                value("Texture/Pattern/CutoffRight").unwrap_or(255.0) / 255.0,
            ),
        });
    KritaBrush {
        name: called.to_owned(),
        tip,
        spacing: if automatic { None } else { number(brush, "spacing").filter(|s| *s > 0.0) },
        angle: number(brush, "angle").unwrap_or(0.0),
        // the tip's rotation by the Drawing Angle sensor, alone or among others
        follows: flag("PressureRotation") && param("RotationSensor").is_some_and(|s| s.contains("drawingangle")),
        turns_at_random,
        rotation_random: if driven("Rotation", "fuzzy") { turned } else { 0.0 },
        rotation_per_stroke: driven("Rotation", "fuzzystroke") && turned > 0.0,
        follows_tilt: driven("Rotation", "ascension"),
        mirror: if driven("Mirror", "fuzzy") {
            (flag("HorizontalMirrorEnabled"), flag("VerticalMirrorEnabled"))
        } else {
            (false, false)
        },
        scatter: on("Scatter")
            .then(|| KritaScatter {
                amount: value("ScatterValue").or_else(|| value("Scattering/Amount")).unwrap_or(1.0).clamp(0.0, 5.0),
                pressure: pressure_curve(param, "Scatter", true),
                along: flag("Scattering/AxisX"),
                across: flag("Scattering/AxisY"),
            })
            .filter(|s| (s.along || s.across) && s.amount > 0.0),
        color_random: colours("fuzzy"),
        color_per_stroke: colours("fuzzystroke"),
        ratio_random: if driven("Ratio", "fuzzy") { amount("Ratio") } else { 0.0 },
        ratio_pressure: pressure_curve(param, "Ratio", true),
        mix_pressure: pressure_curve(param, "Mix", true)
            .map(|points| points.into_iter().map(|p| Vec2::new(p.x, p.y * amount("Mix"))).collect()),
        size_pressure: pressure_curve(param, "Size", true),
        opacity_pressure: pressure_curve(param, "Opacity", false),
        flow_pressure: pressure_curve(param, "Flow", false),
        // none of either would lay nothing, which no brush is made to (older presets keep a flow
        // of 0 from before brushes had one): taken as all of it
        opacity: value("OpacityValue").filter(|v| *v > 0.0).unwrap_or(1.0).clamp(0.0, 1.0),
        flow: value("FlowValue").filter(|v| *v > 0.0).unwrap_or(1.0).clamp(0.0, 1.0),
        wash: param("PaintOpAction").and_then(real) != Some(1.0),
        texture,
        missing,
        masking: None,
    }
}

/// How pen pressure scales the setting `option` (`Size`, `Opacity`, `Flow`) of those `param`
/// gives, as Krita reads it, if it does: none if the setting can be switched off (`checkable`,
/// as the size can) and isn't on, if its pen settings are off (`UseCurve`), or if its sensors
/// leave pressure out (with none, pressure drives it). The curve is the one its sensors share,
/// as they do unless told not to: its own key, else the sensor read last, else the older keys,
/// else a straight line; or, not shared, the pressure sensor's own.
fn pressure_curve<'p>(param: &dyn Fn(&str) -> Option<&'p str>, option: &str, checkable: bool) -> Option<Vec<Vec2>> {
    let key = |what: &str| param(&format!("{option}{what}"));
    if (checkable && param(&format!("Pressure{option}")) != Some("true")) || key("UseCurve") == Some("false") {
        return None;
    }
    let definition = key("Sensor").unwrap_or_default();
    let root = xml::parse(definition);
    let sensors: Vec<&Element> = match &root {
        Some(r) if r.attribute("id") == Some("sensorslist") => r.all("ChildSensor").into_iter().collect(),
        Some(r) => vec![r],
        None => Vec::new(),
    };
    let pressure = sensors.iter().find(|s| s.attribute("id") == Some("pressure"));
    if pressure.is_none() && !sensors.is_empty() {
        return None;
    }
    let older = (param(&format!("Custom{option}")) == Some("true")).then(|| param(&format!("Curve{option}"))).flatten();
    let last = sensors.last().and_then(|s| s.child("curve")).map(|c| c.text.as_str());
    let read = if definition.contains("curve") { last } else { older };
    let own = pressure.and_then(|s| s.child("curve")).map(|c| c.text.as_str());
    let text = if key("UseSameCurve") != Some("false") { key("commonCurve").or(read) } else { own };
    let points = text.map(curve_points).unwrap_or_default();
    Some(if points.len() >= 2 { points } else { vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)] })
}

/// The sensors that drive `option` of those `param` gives, by their names (pressure, where it
/// names none, as Krita takes it).
fn sensors_of<'p>(param: &dyn Fn(&str) -> Option<&'p str>, option: &str) -> Vec<&'p str> {
    let named: Vec<&'p str> = match param(&format!("{option}Sensor")) {
        Some(text) => {
            let ids = |tag: &str| {
                let at = format!("<{tag} id=\"");
                text.match_indices(&at)
                    .filter_map(|(i, _)| text.get(i + at.len()..).and_then(|rest| rest.split('"').next()))
                    .collect::<Vec<&'p str>>()
            };
            let root = ids("params");
            if root.first() == Some(&"sensorslist") { ids("ChildSensor") } else { root }
        }
        None => Vec::new(),
    };
    if named.is_empty() { vec!["pressure"] } else { named }
}

/// A curve's points as a preset keeps them: `x,y;x,y;...`.
fn curve_points(text: &str) -> Vec<Vec2> {
    text.split(';')
        .filter_map(|p| {
            let (x, y) = p.split_once(',')?;
            Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
        })
        .collect()
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
        // its tips and texture count as well (one from the bundle twice over: a rough budget)
        let tip_bytes = |tip: &KritaTip| match tip {
            KritaTip::Round { .. } => 0,
            KritaTip::Image(t) => t.pixels.len(),
            KritaTip::Png(png) => png.len(),
        };
        if !take(tip_bytes(&p.tip)) {
            p.tip = KritaTip::Round { hardness: 1.0, roundness: 1.0 };
        }
        if p.masking.as_ref().is_some_and(|m| !take(tip_bytes(&m.brush.tip))) {
            p.masking = None;
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
    /// Solid inside its fades, then down to nothing at the edge: a circle with the square of the
    /// way out, a rectangle in a straight line along the side it is nearer the edge of.
    Fade,
    /// Blurred (Gaussian): hard at a fade of 1, a bell at 0 (a rectangle stays soft at 1).
    Blur,
    /// As its softness curve says: a circle by the square of the way out, a rectangle by both
    /// sides' shares at once.
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
            let points = curve_points(mask.attribute("softness_curve").unwrap_or_default());
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
    // our round tip paints half halfway between its hardness and its edge: a hardness of twice
    // where Krita's paints half, less 1, matches it there (a softer blur than that is drawn)
    Some(match (&m.falloff, plain) {
        (Falloff::Fade, true) => {
            (KritaTip::Round { hardness: 2.0 * faded_half(fade) - 1.0, roundness: m.ratio }, false)
        }
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
        if u.max(v) > 1.0 {
            return 0.0;
        }
        return match &m.falloff {
            // the side it is nearer the edge of, as far as it is past its fade
            Falloff::Fade => straight(u, hfade).min(straight(v, vfade)),
            Falloff::Blur => blurred_side(u, hfade) * blurred_side(v, vfade),
            // each side's share through the curve, less the curve from the other end
            Falloff::Curve(c) => {
                let side = |t: f32| c.eval(t) * (1.0 - c.eval(1.0 - t));
                side(u) * side(v)
            }
        };
    }
    // the square of the way out (1 at the edge)
    let n = u * u + v * v;
    if n > 1.0 {
        return 0.0;
    }
    match &m.falloff {
        // solid inside the ellipse of the fades; past it down with the square of the way out, to
        // nothing at the edge (all the way from the middle, a fade of 0)
        Falloff::Fade => {
            let share = |w: f32, fade: f32| {
                if w == 0.0 {
                    0.0
                } else if fade <= 1e-4 {
                    f32::INFINITY
                } else {
                    (w / fade) * (w / fade)
                }
            };
            let nf = share(u, hfade) + share(v, vfade);
            if nf < 1.0 {
                1.0
            } else if !nf.is_finite() {
                1.0 - n
            } else if nf - n <= 1e-6 {
                0.0
            } else {
                (1.0 - n * (nf - 1.0) / (nf - n)).clamp(0.0, 1.0)
            }
        }
        Falloff::Blur => blurred_round(n.sqrt(), (hfade + vfade) / 2.0),
        Falloff::Curve(c) => c.eval(n),
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

/// A blurred edge's paint at `t` of the way out (along a ray or a side): an edge `edge` of the way
/// out blurred over `blur` (both shares of the way out), as a share of the paint in the middle;
/// none past the tip's edge.
fn gaussian(t: f32, edge: f32, blur: f32) -> f32 {
    if t >= 1.0 {
        return 0.0;
    }
    let blur = blur.max(1e-4);
    let at = |t: f32| erf((edge - t) / blur) + erf((edge + t) / blur);
    let middle = at(0.0);
    if middle > 0.0 { (at(t) / middle).clamp(0.0, 1.0) } else { 0.0 }
}

/// A blurred circle's paint at `t` of the way out, its fade `fade` (1 the hardest): as Krita's,
/// its edge coming in by 0.676 of what the fade lacks of 1, blurred over 0.382 of it.
fn blurred_round(t: f32, fade: f32) -> f32 {
    let soft = (1.0 - fade).clamp(1e-6, 1.0 - 1e-6);
    gaussian(t, 1.0 - 0.6761 * soft, 0.3825 * soft)
}

/// A blurred rectangle's paint at `t` of the way out along a side, that side's fade `fade`: as
/// Krita's, its edge in by half of 1 less half the fade, blurred over 0.283 of it (so still soft
/// at a fade of 1).
fn blurred_side(t: f32, fade: f32) -> f32 {
    let soft = 1.0 - fade.clamp(0.0, 1.0) / 2.0;
    gaussian(t, 1.0 - 0.5 * soft, 0.2828 * soft)
}

/// How far out a blurred circle at `fade` ([`blurred_round`]) paints half.
fn blur_half(fade: f32) -> f32 {
    (0..=200).map(|i| i as f32 / 200.0).find(|t| blurred_round(*t, fade) <= 0.5).unwrap_or(1.0)
}

/// How far out a faded circle at `fade` (solid inside it, then down with the square of the way
/// out) paints half.
fn faded_half(fade: f32) -> f32 {
    ((1.0 + fade * fade) / 2.0).sqrt()
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
        // its size not on; its opacity, which can't be switched off, by pressure (none other said)
        assert!(
            b.size_pressure.is_none() && b.opacity_pressure.is_some() && !b.turns_at_random && b.missing.is_empty()
        );
        match b.tip {
            // a fade of 0.3 (the two's mean) paints half where our hardness 0.48 does
            KritaTip::Round { hardness, roundness } => {
                assert!((hardness - (2.0 * faded_half(0.3) - 1.0)).abs() < 1e-6 && roundness == 0.5)
            }
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
        let hardness = 2.0 * faded_half(0.5) - 1.0;
        assert_eq!((b.spacing, b.tip), (Some(0.25), KritaTip::Round { hardness, roundness: 0.5 }));
        // so is one saved as "rct", half as tall
        let (t, _) = drawn(
            r#"<Brush type="auto_brush"><MaskGenerator type="rct" id="soft" ratio="0.5" softness_curve="0,1;1,0"/></Brush>"#,
        );
        assert_eq!((t.width as usize, t.height as usize), (n, n / 2));
        assert!(t.pixels[n / 4 * n + n / 2] > 230 && t.pixels[n / 4 * n] < 10);
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
        assert!(
            matches!(b.tip, KritaTip::Round { hardness, .. } if (hardness - (2.0 * faded_half(0.8) - 1.0)).abs() < 1e-6)
        );
        // a softness curve is how much paints at each share of the square of the way out: a ring
        // at the square root of 0.75 here
        let (ring, _) = drawn(
            r#"<Brush type="auto_brush"><MaskGenerator type="circle" id="soft" hfade="0" vfade="0" softness_curve="0,0;0.5,0;0.75,1;1,0"/></Brush>"#,
        );
        assert!(at(&ring, 0.0, 0.0) < 10 && at(&ring, 0.866, 0.0) > 240 && at(&ring, 0.99, 90.0) < 40);
        assert!(at(&ring, 0.75, 0.0) < 200, "{}", at(&ring, 0.75, 0.0));
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
        assert!((hardness("1") - 1.0).abs() < 0.02, "{}", hardness("1"));
        assert!(hardness("0.8") > 0.3 && hardness("0.8") < hardness("1"));
        let KritaTip::Image(bell) = blur("0") else { panic!() };
        assert!(at(&bell, 0.0, 0.0) == 255 && at(&bell, 0.4, 0.0) < 170 && at(&bell, 0.98, 0.0) < 5);
        assert!(blurred_round(1.0, 0.0) == 0.0 && blurred_round(0.99, 1.0) > 0.99);
    }

    #[test]
    fn a_preset_keeps_its_opacity_flow_size_curve_and_painting_mode() {
        let round = r#"<Brush type="auto_brush"><MaskGenerator type="circle"/></Brush>"#;
        let sized_with = |sized: bool, params: &[(&str, &str)]| {
            let extra: String = params.iter().map(|(k, v)| param(k, v)).collect();
            let xml = preset("Pencil", round, sized).replace("</Preset>", &format!("{extra}</Preset>"));
            read_preset(&kpp(&xml), &|_| None).unwrap()
        };
        let read = |params: &[(&str, &str)]| sized_with(false, params);
        let pts = |v: &[(f32, f32)]| Some(v.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>());
        // built up, its opacity and flow as kept; a flow of nothing (left from before) as all
        let b = read(&[("OpacityValue", "0.4"), ("FlowValue", "0"), ("PaintOpAction", "1")]);
        assert_eq!((b.opacity, b.flow, b.wash), (0.4, 1.0, false));
        // washed unless it says it builds up
        assert!(read(&[]).wash && read(&[("PaintOpAction", "2")]).wash);
        // its size by its own curve once switched on, and not when it isn't
        let curve = r#"<!DOCTYPE params> <params id="pressure"> <curve>0,0.4;1,1;</curve> </params> "#;
        let sized = sized_with(true, &[("SizeSensor", curve)]);
        assert_eq!(sized.size_pressure, pts(&[(0.0, 0.4), (1.0, 1.0)]));
        assert_eq!(read(&[("SizeSensor", curve)]).size_pressure, None);
    }

    #[test]
    fn a_preset_turns_mirrors_and_scatters_its_stamps_as_it_says() {
        let round = r#"<Brush type="auto_brush"><MaskGenerator type="circle"/></Brush>"#;
        let read = |params: &[(&str, &str)]| {
            let extra: String = params.iter().map(|(k, v)| param(k, v)).collect();
            let xml = preset("Spray", round, false).replace("</Preset>", &format!("{extra}</Preset>"));
            read_preset(&kpp(&xml), &|_| None).unwrap()
        };
        let sensor = |id: &str| format!(r#"<!DOCTYPE params> <params id="{id}"/> "#);
        let turned =
            |id: &str| read(&[("PressureRotation", "true"), ("RotationSensor", &sensor(id)), ("RotationValue", "0.6")]);
        // at random each stamp, as far as its amount of a half turn; or each stroke; or with the lean
        let b = turned("fuzzy");
        assert_eq!((b.rotation_random, b.rotation_per_stroke, b.follows_tilt), (0.6, false, false));
        assert!(turned("fuzzystroke").rotation_per_stroke && turned("ascension").follows_tilt);
        assert_eq!(read(&[("PressureRotation", "false"), ("RotationSensor", &sensor("fuzzy"))]).rotation_random, 0.0);
        // mirrored at random, across only
        let mirrored =
            [("PressureMirror", "true"), ("HorizontalMirrorEnabled", "true"), ("VerticalMirrorEnabled", "false")];
        let fuzzy = sensor("fuzzy");
        assert_eq!(read(&[&mirrored[..], &[("MirrorSensor", fuzzy.as_str())]].concat()).mirror, (true, false));
        // scattered both ways by the pressure, half a diameter at most; the older key for the amount
        let s = read(&[
            ("PressureScatter", "true"),
            ("ScatterValue", "0.5"),
            ("Scattering/AxisX", "true"),
            ("Scattering/AxisY", "true"),
        ])
        .scatter
        .unwrap();
        assert_eq!((s.amount, s.along, s.across), (0.5, true, true));
        assert_eq!(s.pressure, Some(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]));
        let old = read(&[("PressureScatter", "true"), ("Scattering/Amount", "2"), ("Scattering/AxisY", "true")])
            .scatter
            .unwrap();
        assert_eq!((old.amount, old.along, old.across), (2.0, false, true));
        // at random alone, its pressure leaves it be; switched off, none
        let random =
            read(&[("PressureScatter", "true"), ("ScatterSensor", &sensor("fuzzy")), ("Scattering/AxisX", "true")]);
        assert_eq!(random.scatter.unwrap().pressure, None);
        assert_eq!(read(&[("ScatterValue", "0.5"), ("Scattering/AxisX", "true")]).scatter, None);
        // its colour at random: the hue each stamp, the value (lighter and darker) and Darken each
        // stroke; flattened at random, and by the pressure
        let coloured = read(&[
            ("Pressureh", "true"),
            ("hSensor", &sensor("fuzzy")),
            ("hValue", "0.4"),
            ("Pressurev", "true"),
            ("vSensor", &sensor("fuzzystroke")),
            ("vValue", "0.3"),
            ("PressureDarken", "true"),
            ("DarkenSensor", &sensor("fuzzystroke")),
            ("PressureRatio", "true"),
            ("RatioSensor", &sensor("fuzzy")),
            ("RatioValue", "0.7"),
        ]);
        assert_eq!((coloured.color_random, coloured.color_per_stroke), ([0.4, 0.0, 0.0, 0.0], [0.0, 0.0, 0.3, 1.0]));
        assert_eq!((coloured.ratio_random, coloured.ratio_pressure.is_none()), (0.7, true));
        // its colour by the pressure, the background colour where light: at most 0.8 of its own
        let mixed = read(&[("PressureMix", "true"), ("MixValue", "0.8")]);
        assert_eq!(mixed.mix_pressure, Some(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.8)]));
        let pressed = read(&[("PressureRatio", "true")]);
        assert_eq!(
            (pressed.ratio_random, pressed.ratio_pressure),
            (0.0, Some(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)]))
        );
    }

    #[test]
    fn a_texture_s_mask_is_made_as_krita_makes_it() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let plain = KritaTexture {
            picture: KritaPicture::Encoded(Vec::new()),
            scale: 1.0,
            strength: 1.0,
            strength_pressure: None,
            soft: false,
            invert: false,
            mode: 0,
            brightness: 0.0,
            contrast: 1.0,
            neutral: 0.5,
            cutoff: (0, 0.0, 1.0),
        };
        // as it is, what is clear counting as white
        assert!(near(plain.mask_value(0.3, 1.0), 0.3) && near(plain.mask_value(0.0, 0.0), 1.0));
        assert!(near(plain.mask_value(0.0, 0.5), 0.5));
        // lighter, of more contrast, inverted, its middle moved
        assert!(near(KritaTexture { brightness: 0.3, ..plain.clone() }.mask_value(0.3, 1.0), 0.0));
        assert!(near(KritaTexture { contrast: 2.0, ..plain.clone() }.mask_value(0.6, 1.0), 0.7));
        assert!(near(KritaTexture { invert: true, ..plain.clone() }.mask_value(0.3, 1.0), 0.7));
        let moved = KritaTexture { neutral: 0.25, ..plain.clone() };
        assert!(near(moved.mask_value(0.25, 1.0), 0.5) && near(moved.mask_value(0.625, 1.0), 0.75));
        // cut outside its cutoffs: to none, or to all of it
        let cut = |policy| KritaTexture { cutoff: (policy, 0.2, 0.8), ..plain.clone() };
        assert!(cut(1).mask_value(0.1, 1.0) == 0.0 && cut(2).mask_value(0.9, 1.0) == 1.0);
        assert!(near(cut(1).mask_value(0.5, 1.0), 0.5) && near(cut(0).mask_value(0.1, 1.0), 0.1));
        // how it combines, by its number
        let op = |mode| KritaTexture { mode, ..plain.clone() }.op();
        assert_eq!(
            (op(0), op(1), op(2), op(4), op(15), op(16)),
            (Some("multiply"), Some("subtract"), None, Some("darken"), Some("linear_height_photoshop"), None)
        );
    }

    #[test]
    fn an_auto_tip_falls_off_as_krita_s_does() {
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        let mask = |rectangle: bool, falloff: Falloff, fade: f32| Mask {
            rectangle,
            falloff,
            ratio: 1.0,
            fades: (fade, fade),
            spikes: 2,
            density: 1.0,
            randomness: 0.0,
        };
        // a faded circle: solid inside its fade, then down with the square of the way out
        let circle = mask(false, Falloff::Fade, 0.5);
        assert!(paint_at(&circle, 0.4, 0.0) == 1.0 && paint_at(&circle, 1.01, 0.0) == 0.0);
        assert!(near(paint_at(&circle, 0.75, 0.0), (1.0 - 0.5625) / 0.75), "{}", paint_at(&circle, 0.75, 0.0));
        assert!(near(paint_at(&mask(false, Falloff::Fade, 0.0), 0.5, 0.0), 0.75));
        // as our round tip, it paints half where Krita's does
        assert!(near(faded_half(0.5), (0.625f32).sqrt()) && faded_half(1.0) == 1.0);
        // a faded rectangle: the side nearer its edge, not both at once
        let square = mask(true, Falloff::Fade, 0.5);
        assert!(near(paint_at(&square, 0.75, 0.75), 0.5) && near(paint_at(&square, 0.75, 0.2), 0.5));
        // blurred: a circle hard at a fade of 1, its edge in and wider as the fade falls; a
        // rectangle still soft at 1, half at three quarters of the way out
        assert!(blurred_round(0.98, 1.0) > 0.99 && near(blur_half(0.0), 0.403));
        assert!(near(blurred_side(0.75, 1.0), 0.5) && blurred_side(1.0, 1.0) == 0.0);
        // a softness curve (straight down from the middle): a circle by the square of the way out,
        // a rectangle by each side's share through it and the curve from the other end
        let down = || Falloff::Curve(crate::curve::Curve::new(&[Vec2::new(0.0, 1.0), Vec2::new(1.0, 0.0)]));
        assert!(near(paint_at(&mask(false, down(), 1.0), 0.5, 0.0), 0.75));
        assert!(near(paint_at(&mask(true, down(), 1.0), 0.5, 0.0), 0.25));
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

    /// A preset whose masking brush (`masking`, its settings without their prefix) is on, combined
    /// by `op` at `size` times the main brush's.
    fn masked(masking: &[(&str, &str)], op: &str, size: &str) -> String {
        let main = r#"<Brush type="auto_brush" spacing="0.1" useAutoSpacing="0"><MaskGenerator type="circle" diameter="40"/></Brush>"#;
        let own: String = masking.iter().map(|(k, v)| param(&format!("MaskingBrush/Preset/{k}"), v)).collect();
        let on = [
            param("MaskingBrush/Enabled", "true"),
            param("MaskingBrush/MaskingCompositeOp", op),
            param("MaskingBrush/UseMasterSize", "true"),
            param("MaskingBrush/MasterSizeCoeff", size),
        ]
        .concat();
        preset("Grainy", main, true).replace("</Preset>", &format!("{on}{own}</Preset>"))
    }

    #[test]
    fn a_masking_brush_is_a_second_brush_with_its_own_tip() {
        let tip = crate::base64::encode(&gbr(&[10, 20, 30]));
        let held = format!(
            r#"<resources><resource md5sum="cc33" filename="Debris.gbr" name="Debris" type="brushes"><![CDATA[{tip}]]></resource></resources>"#
        );
        let mask = [
            (
                "brush_definition",
                r#"<Brush md5sum="cc33" type="gbr_brush" filename="Debris.gbr" spacing="0.4" angle="0.5" useAutoSpacing="0"/>"#,
            ),
            ("PressureSize", "false"),
            ("PressureOpacity", "true"),
            ("OpacityValue", "0,8"),
            ("FlowValue", "0.25"),
        ];
        let xml = masked(&mask, "subtract", "0.5")
            .replace(r#"name="Grainy">"#, &format!(r#"name="Grainy" embedded_resources="1">{held}"#));
        let b = read_preset(&kpp(&xml), &|_| None).unwrap();
        // the main brush as it was
        assert!(matches!(b.tip, KritaTip::Round { .. }) && b.spacing == Some(0.1) && b.missing.is_empty());
        let m = b.masking.as_deref().unwrap();
        assert_eq!((m.op.as_str(), m.size, m.opacity, m.flow), ("subtract", 0.5, 0.8, 0.25));
        // its own tip, spacing, angle and pressure, and nothing of its own beyond
        let KritaTip::Image(t) = &m.brush.tip else { panic!("{:?}", m.brush.tip) };
        assert_eq!(t.pixels, [10, 20, 30]);
        assert_eq!((m.brush.spacing, m.brush.angle, m.brush.name.as_str()), (Some(0.4), 0.5, "Grainy"));
        assert!(m.brush.size_pressure.is_none() && m.brush.opacity_pressure.is_some());
        assert!(m.brush.texture.is_none() && m.brush.masking.is_none());
        // an auto brush masks too, as our round tip; the op saved or not
        let auto = [(
            "brush_definition",
            r#"<Brush type="auto_brush"><MaskGenerator type="circle" hfade="0.5" vfade="0.5"/></Brush>"#,
        )];
        let b = read_preset(&kpp(&masked(&auto, "linear_dodge", "1,5")), &|_| None).unwrap();
        let m = b.masking.unwrap();
        assert_eq!((m.op.as_str(), m.size, m.opacity, m.flow), ("linear_dodge", 1.5, 1.0, 1.0));
        assert_eq!(m.brush.tip, KritaTip::Round { hardness: 2.0 * faded_half(0.5) - 1.0, roundness: 1.0 });
        let none = masked(&auto, "", "0").replace(&param("MaskingBrush/MaskingCompositeOp", ""), "");
        let m = read_preset(&kpp(&none), &|_| None).unwrap().masking.unwrap();
        assert_eq!((m.op.as_str(), m.size), ("multiply", 1.0));
        // turned off, or with no tip of its own: none
        let off = masked(&auto, "multiply", "1")
            .replace(&param("MaskingBrush/Enabled", "true"), &param("MaskingBrush/Enabled", "false"));
        assert_eq!(read_preset(&kpp(&off), &|_| None).unwrap().masking, None);
        let bare = read_preset(&kpp(&masked(&[("PressureSize", "true")], "multiply", "1")), &|_| None).unwrap();
        assert_eq!(bare.masking, None);
        // its tip not there: none, and said so
        let gone = [("brush_definition", r#"<Brush type="png_brush" filename="square_rough.png"/>"#)];
        let b = read_preset(&kpp(&masked(&gone, "subtract", "1")), &|_| None).unwrap();
        assert_eq!((b.masking, b.missing), (None, vec!["square_rough.png".to_owned()]));
        crate::damaged::survives("Krita preset with a masking brush", &kpp(&xml), |b| {
            let _ = read_preset(b, &|_| None);
        });
    }

    #[test]
    fn a_masking_brushs_opacity_and_flow_follow_the_pressure_by_their_curves() {
        let auto = r#"<Brush type="auto_brush"><MaskGenerator type="circle"/></Brush>"#;
        let read = |pairs: &[(&str, &str)]| {
            let mask: Vec<(&str, &str)> =
                [("brush_definition", auto)].into_iter().chain(pairs.iter().copied()).collect();
            read_preset(&kpp(&masked(&mask, "linear_dodge", "0.5")), &|_| None).unwrap().masking.unwrap()
        };
        let pts = |v: &[(f32, f32)]| Some(v.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>());
        // the pressure sensor's own curve, as Krita 4 keeps it (its sensors' shared one not saved)
        let m = read(&[
            ("PressureOpacity", "true"),
            ("OpacityUseCurve", "true"),
            ("OpacityUseSameCurve", "true"),
            (
                "OpacitySensor",
                r#"<!DOCTYPE params> <params id="pressure"> <curve>0.6,0;0.83,0.38;0.9,0.74;</curve> </params> "#,
            ),
        ]);
        assert_eq!(m.opacity_pressure, pts(&[(0.6, 0.0), (0.83, 0.38), (0.9, 0.74)]));
        // the flow, of which nothing is said: by the pressure, straight, as Krita takes it
        assert_eq!(m.flow_pressure, pts(&[(0.0, 0.0), (1.0, 1.0)]));
        // among other sensors; the shared curve when they share one; straight when it keeps none
        let listed = r#"<!DOCTYPE params> <params id="sensorslist"> <ChildSensor id="fade" length="100"/> <ChildSensor id="pressure"> <curve>0,0;1,0.5;</curve> </ChildSensor> </params> "#;
        let m = read(&[("PressureFlow", "true"), ("FlowUseCurve", "true"), ("FlowSensor", listed)]);
        assert_eq!(m.flow_pressure, pts(&[(0.0, 0.0), (1.0, 0.5)]));
        let m = read(&[
            ("PressureFlow", "true"),
            ("FlowUseSameCurve", "true"),
            ("FlowcommonCurve", "0,0.2;1,1;"),
            ("FlowSensor", listed),
        ]);
        assert_eq!(m.flow_pressure, pts(&[(0.0, 0.2), (1.0, 1.0)]));
        let plain = r#"<!DOCTYPE params> <params id="pressure"/> "#;
        let m = read(&[("PressureFlow", "true"), ("FlowUseCurve", "true"), ("FlowSensor", plain)]);
        assert_eq!(m.flow_pressure, pts(&[(0.0, 0.0), (1.0, 1.0)]));
        // the opacity and flow can't be switched off: a "false" for them is left from before
        let m = read(&[("PressureFlow", "false"), ("FlowSensor", listed)]);
        assert_eq!(m.flow_pressure, pts(&[(0.0, 0.0), (1.0, 0.5)]));
        // shared, the curve of the sensor read last; not shared, the pressure sensor's own
        let after = r#"<!DOCTYPE params> <params id="sensorslist"> <ChildSensor id="pressure"> <curve>0,0;1,0.5;</curve> </ChildSensor> <ChildSensor id="fade"> <curve>0,1;1,0;</curve> </ChildSensor> </params> "#;
        assert_eq!(read(&[("FlowSensor", after)]).flow_pressure, pts(&[(0.0, 1.0), (1.0, 0.0)]));
        let own = read(&[("FlowUseSameCurve", "false"), ("FlowSensor", after)]);
        assert_eq!(own.flow_pressure, pts(&[(0.0, 0.0), (1.0, 0.5)]));
        // driven by something else, or its pen settings off: not by the pressure
        assert_eq!(read(&[("FlowUseCurve", "false"), ("FlowSensor", listed)]).flow_pressure, None);
        let fuzzy = r#"<!DOCTYPE params> <params id="fuzzy"/> "#;
        assert_eq!(read(&[("PressureOpacity", "true"), ("OpacitySensor", fuzzy)]).opacity_pressure, None);
    }

    #[test]
    fn a_masking_brushs_tip_comes_from_its_bundle() {
        let mask = [("brush_definition", r#"<Brush type="gbr_brush" filename="random-debris.gbr" spacing="0.7"/>"#)];
        let mut z = ZipWriter::new();
        z.add("mimetype", b"application/x-krita-resourcebundle");
        z.add("brushes/random-debris.gbr", &gbr(&[1, 2]));
        z.add("paintoppresets/eroded.kpp", &kpp(&masked(&mask, "darken", "0.75")));
        let (_, presets) = read_bundle(&z.finish()).unwrap();
        let m = presets[0].masking.as_deref().unwrap();
        assert!(matches!(&m.brush.tip, KritaTip::Image(t) if t.pixels == [1, 2]));
        assert_eq!((m.op.as_str(), m.size, m.brush.spacing), ("darken", 0.75, Some(0.7)));
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
                param("PressureTexture/Strength/", "true"),
                param(
                    "Texture/Strength/Sensor",
                    "<!DOCTYPE params> <params id=\"pressure\"> <curve>0,0;1,0.5;</curve> </params>",
                ),
                param("Texture/Pattern/UseSoftTexturing", "true"),
                param("Texture/Pattern/Invert", "true"),
                param("Texture/Pattern/TexturingMode", "1"),
                param("Texture/Pattern/Brightness", "0.3"),
                param("Texture/Pattern/CutoffPolicy", "2"),
                param("Texture/Pattern/CutoffLeft", "51"),
            ]
            .concat(),
        );
        let t = read_preset(&kpp(&xml), &|_| None).unwrap().texture.unwrap();
        assert_eq!(
            (t.picture, t.scale, t.strength, t.invert),
            (KritaPicture::Encoded(picture.clone()), 0.35, 0.8, true)
        );
        // how its mask is made and combines; what it doesn't say, as Krita leaves it
        assert_eq!((t.mode, t.brightness, t.contrast, t.neutral, t.cutoff), (1, 0.3, 1.0, 0.5, (2, 0.2, 1.0)));
        // its strength by the pressure, and soft
        assert_eq!(t.strength_pressure, Some(vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.5)]));
        assert!(t.soft);
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
        assert!(t.strength_pressure.is_none() && !t.soft);
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
