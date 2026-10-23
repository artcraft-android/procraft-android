//! Brush files: a zip holding a brush's settings as a keyed archive (`Brush.archive`), a preview
//! (`QuickLook/Thumbnail.png`) and any shape and grain images of its own (`Shape.png`,
//! `Grain.png`). Read into the brush studio's controls (the keys and values
//! [`Brush::set_setting`] takes) and written back from a brush, the controls turned into the
//! file's own numbers. A set file is a zip of brushes, each in a folder of its own, with
//! `brushset.plist` giving the set's name and the folders in order.

use crate::brush::{BRUSH_BEHIND, Brush, GRAIN_HEIGHT, GRAIN_LINEAR_HEIGHT, Notches, SIZE_CURVE};
use crate::document::BlendMode;
use crate::geom::Vec2;
use crate::keyed::{Archive, Builder, Object};
use crate::plist::Value;
use crate::zip::{ZipReader, ZipWriter};

/// A brush as a brush file holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct BrushFile {
    pub name: String,
    pub author: String,
    /// The studio's controls, in the order to set them (the sizes apart: see [`set_sizes`]).
    pub settings: Vec<(String, f32)>,
    /// The file's maximum size (its stamps' squares, in [`PIXELS_PER_SIZE`]).
    pub max_size: f32,
    /// The file's least size, in the same units, if it has one.
    pub min_size: Option<f32>,
    /// The shape's own fixed angle, radians.
    pub shape_angle: f32,
    /// The pressure response (points 0..1 from the left), if it isn't straight.
    pub pressure_curve: Option<Vec<Vec2>>,
    /// The pressure's own curves for the size and the opacity, if not straight.
    pub size_curve: Option<Vec<Vec2>>,
    pub opacity_curve: Option<Vec<Vec2>>,
    pub shape: Texture,
    pub grain: Texture,
    /// Where the size and opacity sliders start when it is picked, 0..1.
    pub paint_size: f32,
    pub paint_opacity: f32,
    /// The sliders' notches for painting, smudging and erasing.
    pub notches: Notches,
    /// Its preview (`QuickLook/Thumbnail.png`), if it has one.
    pub thumbnail: Option<Vec<u8>>,
    /// Its author's picture and signature (PNG), if it has them.
    pub author_picture: Option<Vec<u8>>,
    pub signature: Option<Vec<u8>>,
}

/// A shape or grain image as a brush file names it.
#[derive(Clone, Debug, PartialEq)]
pub enum Texture {
    /// No image: the round tip, or no grain.
    None,
    /// An image that comes with the app that made the file, by its file name (not in the file).
    Stock { name: String, inverted: bool },
    /// An image in the file (PNG or JPEG bytes).
    Image { bytes: Vec<u8>, inverted: bool },
}

/// Canvas pixels a stamp's square is across at the most size, for each 1 of the file's `maxSize`.
pub const PIXELS_PER_SIZE: f32 = 262.0;

/// How much of a stamp's square a hard round tip drawn as an image covers: a round tip of ours
/// (which fills its stamp) stands in for one at this share of the size.
pub const ROUND_TIP_SHARE: f32 = 0.797;

/// Sets `brush`'s sizes from a file's (`maxSize`, `minSize`, in units of its
/// [`Brush::size_unit`], so its shape first), the least kept no larger than the most.
pub fn set_sizes(brush: &mut Brush, max_size: f32, min_size: Option<f32>) {
    let unit = brush.size_unit();
    if max_size.is_finite() {
        brush.max_size = (max_size.max(0.0) * unit).max(0.5);
    }
    if let Some(min) = min_size.filter(|m| m.is_finite()) {
        brush.min_size = (min.max(0.0) * unit).min(brush.max_size);
    }
}

/// How a control's value is kept in the file.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kept {
    /// As it is.
    Same,
    /// As it is, as a whole number.
    Whole,
    /// The control to the power 2.2, times this (spacing and jitter).
    Curve(f32),
    /// Times this.
    Times(f32),
    /// The control squared, times 16 (grain scale).
    Squared16,
    /// 1 less the control.
    Complement,
    /// A switch.
    Switch,
    /// A choice of three whose last two the file keeps the other way round (the hover fill:
    /// none, shape, all in the list; none, all, shape in the file).
    LastTwoSwapped,
}

/// The list's place for the file's number, or the other way (the same swap both ways).
fn swap_last_two(v: f64) -> f64 {
    match v.round() as i64 {
        1 => 2.0,
        2 => 1.0,
        _ => v,
    }
}

/// The file's number for control value `c`.
fn to_file(kept: Kept, c: f32) -> Value {
    let c = f64::from(c);
    match kept {
        Kept::Same => Value::Real(c),
        Kept::Whole => Value::Int(c.round() as i64),
        Kept::Curve(scale) => Value::Real(f64::from(scale) * c.max(0.0).powf(2.2)),
        Kept::Times(k) => Value::Real(c * f64::from(k)),
        Kept::Squared16 => Value::Real(16.0 * c * c),
        Kept::Complement => Value::Real(1.0 - c),
        Kept::Switch => Value::Bool(c >= 0.5),
        Kept::LastTwoSwapped => Value::Int(swap_last_two(c) as i64),
    }
}

/// The control value for the file's number `v`.
fn to_control(kept: Kept, v: f64) -> f32 {
    let c = match kept {
        Kept::Same | Kept::Whole | Kept::Switch => v,
        Kept::Curve(scale) => (v / f64::from(scale)).max(0.0).powf(1.0 / 2.2),
        Kept::Times(k) => v / f64::from(k),
        Kept::Squared16 => (v / 16.0).max(0.0).sqrt(),
        Kept::Complement => 1.0 - v,
        Kept::LastTwoSwapped => swap_last_two(v),
    };
    c as f32
}

/// The controls kept one to one: the studio's key, the file's, and how.
const PLAIN: &[(&str, &str, Kept)] = &[
    // stroke path and stabilization
    ("spacing", "plotSpacing", Kept::Curve(2.0)),
    ("spacing_jitter", "plotSpacingJitter", Kept::Times(10.0)),
    ("spacing_version", "plotSpacingVersion", Kept::Whole),
    ("jitter_lateral", "plotJitter", Kept::Curve(3.0)),
    ("jitter_linear", "plotJitterLongitudinal", Kept::Curve(3.0)),
    ("falloff", "dynamicsFalloff", Kept::Same),
    ("streamline", "plotSmoothing", Kept::Same),
    ("streamline_pressure", "dynamicsPressureSmoothing", Kept::Same),
    ("stabilization", "plotMovingAverageStabilization", Kept::Same),
    ("motion_filtering", "plotFFTSmoothingAmount", Kept::Same),
    ("motion_expression", "plotFFTSmoothingBias", Kept::Same),
    // taper: the pen's, then touch's
    ("pen_taper_start", "pencilTaperStartLength", Kept::Same),
    ("pen_taper_end", "pencilTaperEndLength", Kept::Same),
    ("pen_taper_size", "pencilTaperSize", Kept::Same),
    ("pen_taper_opacity", "pencilTaperOpacity", Kept::Same),
    ("pen_taper_pressure", "taperPressure", Kept::Same),
    ("pen_taper_tip", "pencilTaperShape", Kept::Same),
    ("pen_taper_link", "pencilTaperSizeLinked", Kept::Switch),
    ("pen_tip_animation", "pencilTipAnimation", Kept::Switch),
    ("touch_taper_start", "taperStartLength", Kept::Same),
    ("touch_taper_end", "taperEndLength", Kept::Same),
    ("touch_taper_size", "taperSize", Kept::Same),
    ("touch_taper_opacity", "taperOpacity", Kept::Same),
    ("touch_taper_tip", "taperShape", Kept::Same),
    ("touch_taper_link", "taperSizeLinked", Kept::Switch),
    // shape
    ("relative_to_stroke", "shapeRollMode", Kept::Whole),
    ("shape_rotation", "shapeRotation", Kept::Same),
    ("scatter", "shapeScatter", Kept::Times(2.0)),
    ("count_jitter", "shapeCountJitter", Kept::Same),
    ("randomized", "shapeRandomise", Kept::Switch),
    ("flip_x", "shapeFlipXJitter", Kept::Switch),
    ("flip_y", "shapeFlipYJitter", Kept::Switch),
    ("roundness", "shapeRoundness", Kept::Same),
    ("pressure_roundness", "dynamicsPressureShapeRoundnessMinimum", Kept::Complement),
    ("tilt_roundness", "dynamicsTiltShapeRoundnessMinimum", Kept::Complement),
    ("roundness_jitter_y", "jitterShapeRoundness", Kept::Same),
    ("roundness_jitter_x", "jitterShapeRoundnessX", Kept::Same),
    // grain
    ("grain_behavior", "textureApplication", Kept::Whole),
    ("grain_movement", "textureMovement", Kept::Same),
    ("grain_scale", "textureScale", Kept::Squared16),
    ("grain_zoom", "textureZoom", Kept::Same),
    ("grain_rotation", "textureRotation", Kept::Same),
    ("grain_depth", "grainDepth", Kept::Same),
    ("grain_depth_min", "grainDepthMinimum", Kept::Same),
    ("grain_depth_jitter", "grainDepthJitter", Kept::Same),
    ("grain_offset_jitter", "textureOffsetJitter", Kept::Switch),
    ("grain_brightness", "textureBrightness", Kept::Same),
    ("grain_contrast", "textureContrast", Kept::Same),
    ("grain_follows_camera", "texturizedGrainFollowsCamera", Kept::Switch),
    // rendering
    ("flow", "dynamicsGlazedFlow", Kept::Same),
    ("wet_edges", "wetEdgesAmount", Kept::Same),
    ("burnt_edges", "burntEdgesAmount", Kept::Same),
    ("luminance_blending", "blendGammaCorrect", Kept::Switch),
    ("alpha_threshold", "alphaThreshold", Kept::Switch),
    ("alpha_threshold_amount", "alphaThresholdAmount", Kept::Same),
    ("classic_combine", "legacyNormalDual", Kept::Switch),
    // wet mix
    ("wet_dilution", "dynamicsMix", Kept::Same),
    ("wet_charge", "dynamicsLoad", Kept::Same),
    ("wet_attack", "dynamicsPressureMix", Kept::Same),
    ("wet_pull", "dynamicsWetAccumulation", Kept::Same),
    ("wet_grade", "dynamicsMixSoftening", Kept::Same),
    ("wet_blur", "dynamicsBlur", Kept::Same),
    ("wet_blur_jitter", "dynamicsBlurJitter", Kept::Same),
    ("wet_jitter", "dynamicsWetnessJitter", Kept::Same),
    // colour dynamics
    ("stamp_hue", "dynamicsJitterHue", Kept::Same),
    ("stamp_saturation", "dynamicsJitterSaturation", Kept::Same),
    ("stamp_lightness", "dynamicsJitterLightness", Kept::Same),
    ("stamp_darkness", "dynamicsJitterDarkness", Kept::Same),
    ("stamp_secondary", "jitterSecondary", Kept::Same),
    ("stroke_hue", "dynamicsJitterStrokeHue", Kept::Same),
    ("stroke_saturation", "dynamicsJitterStrokeSaturation", Kept::Same),
    ("stroke_lightness", "dynamicsJitterStrokeLightness", Kept::Same),
    ("stroke_darkness", "dynamicsJitterStrokeDarkness", Kept::Same),
    ("stroke_secondary", "jitterStrokeSecondary", Kept::Same),
    ("pressure_hue", "dynamicsPressureHue", Kept::Same),
    ("pressure_saturation", "dynamicsPressureSaturation", Kept::Same),
    ("pressure_brightness", "dynamicsPressureBrightness", Kept::Same),
    ("pressure_secondary", "dynamicsPressureSecondaryColor", Kept::Same),
    ("tilt_hue", "dynamicsTiltHue", Kept::Same),
    ("tilt_saturation", "dynamicsTiltSaturation", Kept::Same),
    ("tilt_brightness", "dynamicsTiltBrightness", Kept::Same),
    ("tilt_secondary", "dynamicsTiltSecondaryColor", Kept::Same),
    ("roll_hue", "dynamicsRollHue", Kept::Same),
    ("roll_saturation", "dynamicsRollSaturation", Kept::Same),
    ("roll_brightness", "dynamicsRollBrightness", Kept::Same),
    ("roll_secondary", "dynamicsRollSecondaryColor", Kept::Same),
    // dynamics
    ("speed_size", "dynamicsSpeedSize", Kept::Same),
    ("speed_opacity", "dynamicsSpeedOpacity", Kept::Same),
    ("speed_spacing", "plotSpacingSpeed", Kept::Times(10.0)),
    ("jitter_size", "dynamicsJitterSize", Kept::Same),
    ("jitter_opacity", "dynamicsJitterOpacity", Kept::Same),
    // the pen
    ("pressure_size", "dynamicsPressureSize", Kept::Same),
    ("pressure_opacity", "dynamicsPressureOpacity", Kept::Same),
    ("pressure_flow", "dynamicsPressureOpacityTransfer", Kept::Same),
    ("pressure_bleed", "dynamicsPressureBleed", Kept::Curve(1.0)),
    ("tilt_angle", "dynamicsTiltAngle", Kept::Same),
    ("opacity_tilt_angle", "opacityTiltAngle", Kept::Same),
    ("size_tilt_angle", "sizeTiltAngle", Kept::Same),
    ("gradation_tilt_angle", "gradationTiltAngle", Kept::Same),
    ("bleed_tilt_angle", "bleedTiltAngle", Kept::Same),
    ("roundness_tilt_angle", "shapeRoundnessTiltAngle", Kept::Same),
    ("hue_tilt_angle", "hueTiltAngle", Kept::Same),
    ("saturation_tilt_angle", "saturationTiltAngle", Kept::Same),
    ("brightness_tilt_angle", "brightnessTiltAngle", Kept::Same),
    ("secondary_tilt_angle", "secondaryColorTiltAngle", Kept::Same),
    ("tilt_roundness_max", "dynamicsTiltShapeRoundness", Kept::Same),
    ("tilt_opacity", "dynamicsTiltOpacity", Kept::Same),
    ("tilt_gradation", "dynamicsTiltGradation", Kept::Same),
    ("tilt_bleed", "dynamicsTiltBleed", Kept::Curve(1.0)),
    ("tilt_size", "dynamicsTiltSize", Kept::Same),
    ("tilt_size_compression", "dynamicsTiltCompression", Kept::Same),
    ("roll_size", "dynamicsRollSize", Kept::Same),
    ("roll_opacity", "dynamicsRollOpacity", Kept::Same),
    ("roll_bleed", "dynamicsRollBleed", Kept::Same),
    ("cursor_outline", "hoverOutline", Kept::Whole),
    ("hover_pressure", "hoverPressure", Kept::Same),
    ("hover_fill", "hoverFill", Kept::LastTwoSwapped),
    // properties and preview
    ("orient_to_screen", "oriented", Kept::Switch),
    ("smudge_pull", "dynamicsSmudgeAccumulation", Kept::Same),
    ("max_opacity", "maxOpacity", Kept::Same),
    ("min_opacity", "minOpacity", Kept::Same),
    ("preview_stamp", "stamp", Kept::Switch),
    ("preview_size", "previewSize", Kept::Same),
    ("preview_pressure_min", "previewPressureMinimum", Kept::Same),
    ("preview_pressure_scale", "previewPressureScale", Kept::Times(2.0)),
    ("preview_wet_mix", "previewWetMixEnabled", Kept::Switch),
    ("preview_tilt", "previewTiltEnabled", Kept::Switch),
    ("pressure_size_response", "dynamicsPressureSizeSpeed", Kept::Same),
    ("pressure_opacity_response", "dynamicsPressureOpacitySpeed", Kept::Same),
    ("pressure_bleed_response", "dynamicsPressureBleedSpeed", Kept::Same),
    ("pressure_roundness_response", "dynamicsPressureResponse", Kept::Same),
    ("preview_tilt_offset", "previewTiltAngleOffset", Kept::Same),
];

/// Rendering modes as the studio lists them, by the file's three switches (recursive mixing, the
/// most transfer, modulated transfer).
const RENDERING: [(bool, bool, bool); 6] = [
    (false, true, false),
    (false, true, true),
    (false, false, false),
    (false, false, true),
    (true, false, true),
    (true, false, false),
];

/// Behind as brush files number their blend modes (between Soft Light and Shade).
const BEHIND_NUMBER: i64 = 18;

/// The number of stamps a place for the count a file stores: one more than fifteen times it,
/// whole (so a sixteenth for each stamp, as files are written, reads back the same).
fn stamp_count(stored: f64) -> f32 {
    ((stored * 15.0).floor() + 1.0).clamp(1.0, 16.0) as f32
}

/// A brush file's root object's values, read through.
struct Fields<'a>(Object<'a>);

impl Fields<'_> {
    fn num(&self, key: &str) -> Option<f64> {
        self.0.real(key).or_else(|| self.0.bool(key).map(|b| if b { 1.0 } else { 0.0 }))
    }
    fn flag(&self, key: &str) -> bool {
        self.num(key).is_some_and(|v| v >= 0.5)
    }
    /// A string, none for the archive's null.
    fn text(&self, key: &str) -> Option<String> {
        self.0.string(key).filter(|s| s != "$null" && !s.is_empty())
    }
    /// A curve's points ("{x, y}" strings).
    fn curve(&self, key: &str) -> Option<Vec<Vec2>> {
        let points: Vec<Vec2> = self
            .0
            .object(key)?
            .values("points")
            .iter()
            .filter_map(|v| {
                let s = v.as_str()?;
                let (x, y) = s.trim_matches(|c| c == '{' || c == '}').split_once(',')?;
                Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
            })
            .collect();
        (points.len() >= 2).then_some(points)
    }
    fn reals(&self, key: &str) -> Vec<f32> {
        self.0.values(key).iter().filter_map(|v| v.as_real()).map(|v| v as f32).collect()
    }
}

/// Reads a brush file; none if it isn't one.
pub fn read(bytes: &[u8]) -> Option<BrushFile> {
    read_in(&ZipReader::new(bytes)?, "")
}

/// Reads a set file: the set's name and its brushes in order; none if it isn't one.
pub fn read_set(bytes: &[u8]) -> Option<(String, Vec<BrushFile>)> {
    let zip = ZipReader::new(bytes)?;
    let (name, folders) = set_list(&zip.read("brushset.plist")?)?;
    let brushes: Vec<BrushFile> = folders.iter().filter_map(|f| read_in(&zip, &format!("{f}/"))).collect();
    (!brushes.is_empty()).then_some((name, brushes))
}

/// The set's name and its brushes' folders from `brushset.plist` (XML or binary).
fn set_list(bytes: &[u8]) -> Option<(String, Vec<String>)> {
    if bytes.starts_with(b"bplist") {
        let root = crate::plist::read(bytes)?;
        let Value::Dict(fields) = root else { return None };
        let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v);
        let name = get("name").and_then(|v| v.as_str()).unwrap_or("Imported").to_owned();
        let folders = get("brushes")?.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
        return Some((name, folders));
    }
    let text = std::str::from_utf8(bytes).ok()?;
    // the string after `<key>name</key>`, and the strings in the array after `<key>brushes</key>`
    let after = |key: &str| text.split_once(&format!("<key>{key}</key>")).map(|(_, rest)| rest);
    let strings = |part: &str| -> Vec<String> {
        part.split("<string>").skip(1).filter_map(|s| s.split_once("</string>").map(|(v, _)| unescape(v))).collect()
    };
    let name = after("name").and_then(|rest| strings(rest).into_iter().next()).unwrap_or_else(|| "Imported".to_owned());
    let array = after("brushes")?.split_once("</array>")?.0;
    Some((name, strings(array)))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Reads the brush whose files are under `prefix` in `zip`.
fn read_in(zip: &ZipReader<'_>, prefix: &str) -> Option<BrushFile> {
    let archive = Archive::read(&zip.read(&format!("{prefix}Brush.archive"))?)?;
    let root = archive.root()?;
    let f = Fields(root);
    let mut settings: Vec<(String, f32)> = Vec::new();
    let mut set = |key: &str, v: f32| {
        if v.is_finite() {
            settings.push((key.to_owned(), v));
        }
    };
    for &(control, key, kept) in PLAIN {
        if let Some(v) = f.num(key) {
            set(control, to_control(kept, v));
        }
    }
    if let Some(v) = f.num("shapeCount") {
        let count = stamp_count(v);
        set("count", (count - 1.0) / (crate::studio::MAX_COUNT as f32 - 1.0));
    }
    let style = if f.flag("shapeRoll") {
        2.0
    } else if f.flag("shapeAzimuth") {
        1.0
    } else {
        0.0
    };
    set("input_style", style);
    // older files without a spacing version: the older spacing if they have a dual brush's blend
    if f.num("plotSpacingVersion").is_none() {
        set("spacing_version", if f.num("dualBlendMode").is_some() { 0.0 } else { 1.0 });
    }
    let switches =
        (f.flag("renderingRecursiveMixing"), f.flag("renderingMaxTransfer"), f.flag("renderingModulatedTransfer"));
    if let Some(mode) = RENDERING.iter().position(|&r| r == switches) {
        set("rendering_mode", mode as f32);
    }
    // the classic taper is the old one
    if let Some(v) = f.num("taperVersion") {
        set("classic_taper", if v < 0.5 { 1.0 } else { 0.0 });
    }
    for (control, on, mode) in [
        ("shape_filtering", "shapeFilter", "shapeFilterMode"),
        ("grain_filtering", "textureFilter", "textureFilterMode"),
    ] {
        if f.num(on).is_some() {
            // none, classic, improved
            set(
                control,
                if !f.flag(on) {
                    0.0
                } else if f.flag(mode) {
                    1.0
                } else {
                    2.0
                },
            );
        }
    }
    // blend modes by the layers' numbering, the newest key first; a brush's also behind
    let blend = |keys: &[&str]| {
        keys.iter().find_map(|k| f.num(k)).map(|n| match n.round() as i64 {
            BEHIND_NUMBER => BRUSH_BEHIND,
            n => crate::native::blend_from(n).id(),
        })
    };
    if let Some(m) = blend(&["extendedBlend2", "extendedBlend", "blendMode"]) {
        set("blend_mode", m as f32);
    }
    // a grain also blends by its own two modes, numbered past the layers'
    if let Some(n) = ["grainBlendModeExtended", "grainBlendMode"].iter().find_map(|k| f.num(k)) {
        let n = n.round() as i64;
        let own = [GRAIN_HEIGHT, GRAIN_LINEAR_HEIGHT].into_iter().find(|&m| i64::from(m) == n);
        set("grain_blend", own.unwrap_or_else(|| crate::native::blend_from(n).id()) as f32);
    }
    if let Some(m) = blend(&["burntEdgesBlendModeExtended", "burntEdgesBlendMode"]) {
        set("burnt_blend", m as f32);
    }
    // an image of the brush's own is in the file by a name of its own (its path then null), or
    // by the name its path gives; else the path names a stock image
    let texture = |path: &str, own: &str, inverted: &str| {
        let inverted = f.flag(inverted);
        match f.text(path) {
            None => match zip.read(&format!("{prefix}{own}")) {
                Some(bytes) => Texture::Image { bytes, inverted },
                None => Texture::None,
            },
            Some(name) => {
                let file = name.rsplit('/').next().unwrap_or(&name);
                match zip.read(&format!("{prefix}{name}")).or_else(|| zip.read(&format!("{prefix}{file}"))) {
                    Some(bytes) => Texture::Image { bytes, inverted },
                    None => Texture::Stock { name, inverted },
                }
            }
        }
    };
    // sizes are kept as the size the slider sets (its place to the power of its curve)
    let place = |v: Vec<f32>| v.into_iter().map(|s| s.max(0.0).powf(1.0 / SIZE_CURVE)).collect::<Vec<f32>>();
    let notches = Notches {
        sizes: ["savedPaintSizes", "savedSmudgeSizes", "savedEraseSizes"].map(|k| place(f.reals(k))),
        opacities: ["savedPaintOpacities", "savedSmudgeOpacities", "savedEraseOpacities"].map(|k| f.reals(k)),
    };
    Some(BrushFile {
        name: f.text("name").unwrap_or_else(|| "Imported Brush".to_owned()),
        author: f.text("authorName").unwrap_or_default(),
        settings,
        max_size: f.num("maxSize").unwrap_or(1.0) as f32,
        min_size: f.num("minSize").map(|v| v as f32),
        shape_angle: f.num("shapeAngle").unwrap_or(0.0) as f32,
        pressure_curve: f.curve("dynamicsPressureOverall").filter(|c| !is_straight(c)),
        size_curve: f.curve("dynamicsPressureSizeCurve").filter(|c| !is_straight(c)),
        opacity_curve: f.curve("dynamicsPressureOpacityCurve").filter(|c| !is_straight(c)),
        shape: texture("bundledShapePath", "Shape.png", "shapeInverted"),
        grain: texture("bundledGrainPath", "Grain.png", "textureInverted"),
        paint_size: f.num("paintSize").unwrap_or(0.5) as f32,
        paint_opacity: f.num("paintOpacity").unwrap_or(1.0) as f32,
        notches,
        thumbnail: zip.read(&format!("{prefix}QuickLook/Thumbnail.png")),
        author_picture: zip.read(&format!("{prefix}{AUTHOR_PICTURE}")),
        signature: zip.read(&format!("{prefix}{SIGNATURE}")),
    })
}

fn is_straight(points: &[Vec2]) -> bool {
    points.iter().all(|p| (p.x - p.y).abs() < 1e-3)
}

/// The images and pictures a brush file carries beside the settings.
pub struct Pictures<'a> {
    /// The shape (PNG): the brush's image, or its round tip drawn.
    pub shape_png: &'a [u8],
    /// The grain (PNG), or none for no grain.
    pub grain_png: Option<&'a [u8]>,
    /// The brush's preview stroke (PNG).
    pub thumbnail_png: &'a [u8],
    /// Its author's picture and signature (PNG), if it has them.
    pub author_png: Option<&'a [u8]>,
    pub signature_png: Option<&'a [u8]>,
}

/// Where a brush's folder keeps its author's picture and signature.
const AUTHOR_PICTURE: &str = "AuthorPicture/AuthorPicture.png";
const SIGNATURE: &str = "Signature/SignaturePicture.png";

/// Writes `brush` as a brush file by `author`, with `pictures`.
pub fn write(brush: &Brush, author: &str, pictures: &Pictures<'_>) -> Vec<u8> {
    let mut zip = ZipWriter::new();
    add_brush(&mut zip, "", brush, author, pictures);
    zip.finish()
}

/// Writes a set file named `name` of `brushes` (each with its author and pictures), each in a
/// folder named by `id`.
pub fn write_set(name: &str, brushes: &[(&Brush, &str, Pictures<'_>)], id: impl Fn(usize) -> String) -> Vec<u8> {
    let mut zip = ZipWriter::new();
    let folders: Vec<String> = (0..brushes.len()).map(id).collect();
    let mut list = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\t<key>brushes</key>\n\t<array>\n",
    );
    for f in &folders {
        list.push_str(&format!("\t\t<string>{}</string>\n", escape(f)));
    }
    list.push_str(&format!("\t</array>\n\t<key>name</key>\n\t<string>{}</string>\n</dict>\n</plist>\n", escape(name)));
    zip.add("brushset.plist", list.as_bytes());
    for ((brush, author, pictures), folder) in brushes.iter().zip(&folders) {
        add_brush(&mut zip, &format!("{folder}/"), brush, author, pictures);
    }
    zip.finish()
}

/// A brush's files into `zip` under `prefix`.
fn add_brush(zip: &mut ZipWriter, prefix: &str, brush: &Brush, author: &str, pictures: &Pictures<'_>) {
    let mut b = Builder::new();
    let root = b.reserve();
    let mut fields: Vec<(String, Value)> = Vec::new();
    let mut put = |key: &str, v: Value| fields.push((key.to_owned(), v));
    let name = b.string(&brush.name);
    put("name", name);
    let author = b.string(author);
    put("authorName", author);
    put("version", Value::Int(4));
    put("maxSize", Value::Real(f64::from(brush.max_size / brush.size_unit())));
    put("minSize", Value::Real(f64::from(brush.min_size / brush.size_unit())));
    for &(control, key, kept) in PLAIN {
        put(key, to_file(kept, brush.setting(control)));
    }
    let count = 1.0 + (brush.setting("count") * (crate::studio::MAX_COUNT as f32 - 1.0)).round();
    put("shapeCount", Value::Real(f64::from(count / 16.0)));
    let style = brush.setting("input_style").round();
    put("shapeAzimuth", Value::Bool(style >= 1.0));
    put("shapeRoll", Value::Bool(style >= 2.0));
    let (recursive, max, modulated) = RENDERING
        .get(brush.setting("rendering_mode").round().clamp(0.0, 5.0) as usize)
        .copied()
        .unwrap_or(RENDERING[5]);
    put("renderingRecursiveMixing", Value::Bool(recursive));
    put("renderingMaxTransfer", Value::Bool(max));
    put("renderingModulatedTransfer", Value::Bool(modulated));
    put("taperVersion", Value::Int(if brush.setting("classic_taper") >= 0.5 { 0 } else { 1 }));
    for (control, on, mode) in [
        ("shape_filtering", "shapeFilter", "shapeFilterMode"),
        ("grain_filtering", "textureFilter", "textureFilterMode"),
    ] {
        let v = brush.setting(control).round();
        put(on, Value::Bool(v >= 1.0));
        put(mode, Value::Int(i64::from(v == 1.0)));
    }
    let number = |control: &str| {
        let id = brush.setting(control).round().max(0.0) as u32;
        if id == BRUSH_BEHIND {
            return BEHIND_NUMBER;
        }
        crate::native::blend_to(BlendMode::from_id(id).unwrap_or(BlendMode::Normal))
    };
    let mode = number("blend_mode");
    let (oldest, older) = crate::native::older_blends(mode);
    put("blendMode", Value::Int(oldest));
    put("extendedBlend", Value::Int(older));
    put("extendedBlend2", Value::Int(mode));
    let grain_blend = match brush.setting("grain_blend").round() as u32 {
        own @ (GRAIN_HEIGHT | GRAIN_LINEAR_HEIGHT) => i64::from(own),
        _ => number("grain_blend"),
    };
    put("grainBlendMode", Value::Int(crate::native::older_blends(grain_blend).1));
    put("grainBlendModeExtended", Value::Int(grain_blend));
    let burnt = number("burnt_blend");
    put("burntEdgesBlendMode", Value::Int(crate::native::older_blends(burnt).1));
    put("burntEdgesBlendModeExtended", Value::Int(burnt));
    put("shapeAngle", Value::Real(f64::from(brush.shape.rotation)));
    // a stock image as the brush's file named it; otherwise the image is its own, the file by
    // its own name
    for (stock, path, inverted) in [
        (&brush.stock.shape, "bundledShapePath", "shapeInverted"),
        (&brush.stock.grain, "bundledGrainPath", "textureInverted"),
    ] {
        match stock {
            Some(s) => {
                let name = b.string(&s.name);
                put(path, name);
                put(inverted, Value::Bool(s.inverted));
            }
            None => {
                put(path, Builder::null());
                put(inverted, Value::Bool(false));
            }
        }
    }
    // the pressure response, and its own curves for the size and the opacity
    for (key, c) in [
        ("dynamicsPressureOverall", &brush.pressure_curve),
        ("dynamicsPressureSizeCurve", &brush.size_curve),
        ("dynamicsPressureOpacityCurve", &brush.opacity_curve),
    ] {
        let points: Vec<Value> = c.points().iter().map(|p| b.string(&format!("{{{:.6}, {:.6}}}", p.x, p.y))).collect();
        let list = b.array(points, true);
        let curve = b.object(&["ValkyrieMagnitudinalCurve", "NSObject"], vec![("points", list)]);
        put(key, curve);
    }
    // the sliders' notches
    for (k, tool) in ["Paint", "Smudge", "Erase"].iter().enumerate() {
        for (kind, lists, curve) in
            [("Sizes", &brush.notches.sizes, SIZE_CURVE), ("Opacities", &brush.notches.opacities, 1.0)]
        {
            let values = lists
                .get(k)
                .map_or(Vec::new(), |l| l.iter().map(|v| Value::Real(f64::from(v.max(0.0).powf(curve)))).collect());
            let array = b.array(values, false);
            put(&format!("saved{tool}{kind}"), array);
        }
    }
    put("paintSize", Value::Real(0.5));
    put("paintOpacity", Value::Real(1.0));
    let refs: Vec<(&str, Value)> = fields.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    b.fill(&root, &["SilicaBrush", "ValkyrieBrush", "NSObject"], refs);
    let archive = b.finish(root);
    zip.add(&format!("{prefix}Brush.archive"), &archive);
    zip.add(&format!("{prefix}Shape.png"), pictures.shape_png);
    if let Some(grain) = pictures.grain_png {
        zip.add(&format!("{prefix}Grain.png"), grain);
    }
    zip.add(&format!("{prefix}QuickLook/Thumbnail.png"), pictures.thumbnail_png);
    for (path, png) in [(AUTHOR_PICTURE, pictures.author_png), (SIGNATURE, pictures.signature_png)] {
        if let Some(png) = png {
            zip.add(&format!("{prefix}{path}"), png);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_go_both_ways() {
        for kept in
            [Kept::Same, Kept::Curve(2.0), Kept::Curve(3.0), Kept::Times(10.0), Kept::Squared16, Kept::Complement]
        {
            for c in [0.0f32, 0.17, 0.5, 1.0] {
                let back = to_control(kept, to_file(kept, c).as_real().unwrap());
                assert!((back - c).abs() < 1e-5, "{kept:?} {c} {back}");
            }
        }
        // the studio's spacing and grain scale as the file keeps them
        assert!((to_control(Kept::Curve(2.0), 0.042) - 0.1727).abs() < 1e-3);
        assert!((to_control(Kept::Squared16, 1.0) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_brush_survives_a_round_trip() {
        let mut brush =
            crate::library::library().into_iter().flat_map(|s| s.brushes).find(|b| b.name == "Flat Brush").unwrap();
        brush.set_setting("spacing", 0.3);
        brush.set_setting("rendering_mode", 3.0);
        brush.set_setting("blend_mode", BlendMode::Multiply.id() as f32);
        brush.set_setting("count", 4.0 / 15.0);
        brush.set_setting("wet_pull", 0.6);
        brush.set_setting("input_style", 1.0);
        brush.notches.sizes[0] = vec![0.25, 0.5];
        brush.size_curve = crate::curve::Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.42, 0.2), Vec2::new(1.0, 1.0)]);
        let file = write(
            &brush,
            "Someone",
            &Pictures {
                shape_png: b"shape",
                grain_png: Some(b"grain"),
                thumbnail_png: b"thumb",
                author_png: Some(b"face"),
                signature_png: None,
            },
        );
        let back = read(&file).unwrap();
        assert_eq!(back.name, "Flat Brush");
        assert_eq!(back.author, "Someone");
        assert_eq!(back.author_picture.as_deref(), Some(&b"face"[..]));
        assert_eq!(back.signature, None);
        assert_eq!(back.shape, Texture::Image { bytes: b"shape".to_vec(), inverted: false });
        assert_eq!(back.grain, Texture::Image { bytes: b"grain".to_vec(), inverted: false });
        assert_eq!(back.notches.sizes[0], vec![0.25, 0.5]);
        let size_curve = back.size_curve.as_ref().unwrap();
        assert!((size_curve[1] - Vec2::new(0.42, 0.2)).length() < 1e-5);
        assert!(back.opacity_curve.is_none());
        assert!((back.shape_angle - brush.shape.rotation).abs() < 1e-6);
        let mut again = Brush::round_pen();
        set_sizes(&mut again, back.max_size, back.min_size);
        for (k, v) in &back.settings {
            again.set_setting(k, *v);
        }
        for key in [
            "spacing",
            "rendering_mode",
            "blend_mode",
            "count",
            "wet_pull",
            "input_style",
            "shape_rotation",
            "max_size",
            "min_size",
            "flow",
            "pressure_size",
            "grain_scale",
        ] {
            assert!(
                (again.setting(key) - brush.setting(key)).abs() < 1e-3,
                "{key}: {} {}",
                again.setting(key),
                brush.setting(key)
            );
        }
    }

    #[test]
    fn grain_blend_modes_survive_a_round_trip() {
        // the grain's own height modes past the layers' as well as a layer mode
        for mode in [GRAIN_HEIGHT, GRAIN_LINEAR_HEIGHT, BlendMode::ColorBurn.id()] {
            let mut brush = Brush::round_pen();
            brush.set_setting("grain_blend", mode as f32);
            let pictures = Pictures {
                shape_png: b"s",
                grain_png: None,
                thumbnail_png: b"t",
                author_png: None,
                signature_png: None,
            };
            let back = read(&write(&brush, "", &pictures)).unwrap();
            let read_mode = back.settings.iter().find(|(k, _)| k == "grain_blend").map(|(_, v)| *v);
            assert_eq!(read_mode, Some(mode as f32));
            let mut again = Brush::round_pen();
            again.set_setting("grain_blend", read_mode.unwrap());
            assert_eq!(again.setting("grain_blend"), mode as f32);
        }
    }

    #[test]
    fn behind_survives_a_round_trip() {
        // the brush's and its burnt edges' blend modes, as brush files number them
        let mut brush = Brush::round_pen();
        brush.set_setting("blend_mode", BRUSH_BEHIND as f32);
        brush.set_setting("burnt_blend", BRUSH_BEHIND as f32);
        let pictures =
            Pictures { shape_png: b"s", grain_png: None, thumbnail_png: b"t", author_png: None, signature_png: None };
        let back = read(&write(&brush, "", &pictures)).unwrap();
        for key in ["blend_mode", "burnt_blend"] {
            let mode = back.settings.iter().find(|(k, _)| k == key).map(|(_, v)| *v);
            assert_eq!(mode, Some(BRUSH_BEHIND as f32), "{key}");
            let mut again = Brush::round_pen();
            again.set_setting(key, mode.unwrap());
            assert_eq!(again.setting(key), BRUSH_BEHIND as f32, "{key}");
        }
    }

    #[test]
    fn stamp_counts_read_as_stored() {
        // between sixteenths, by fifteenths: 0.1392 is three stamps
        assert_eq!(stamp_count(0.1392), 3.0);
        assert_eq!(stamp_count(0.0), 1.0);
        assert_eq!(stamp_count(2.0), 16.0);
        // what is written reads back
        for k in 1..=16u8 {
            assert_eq!(stamp_count(f64::from(k) / 16.0), f32::from(k));
        }
    }

    #[test]
    fn a_set_survives_a_round_trip() {
        let sets = crate::library::library();
        let brushes: Vec<Brush> = sets.first().unwrap().brushes.iter().take(3).cloned().collect();
        let pictures = || Pictures {
            shape_png: b"shape",
            grain_png: None,
            thumbnail_png: b"thumb",
            author_png: None,
            signature_png: None,
        };
        let list: Vec<(&Brush, &str, Pictures<'_>)> = brushes.iter().map(|b| (b, "", pictures())).collect();
        let file = write_set("Sketch & Ink", &list, |i| format!("BRUSH-{i}"));
        let (name, back) = read_set(&file).unwrap();
        assert_eq!(name, "Sketch & Ink");
        let names: Vec<&str> = back.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, brushes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>());
        assert_eq!(back.first().unwrap().shape, Texture::Image { bytes: b"shape".to_vec(), inverted: false });
        assert_eq!(back.first().unwrap().grain, Texture::None);
        assert!(read_set(&write(brushes.first().unwrap(), "", &pictures())).is_none());
    }

    #[test]
    fn other_files_are_not_brushes() {
        assert!(read(b"nothing").is_none());
        let mut zip = ZipWriter::new();
        zip.add("something.txt", b"hello");
        assert!(read(&zip.finish()).is_none());
    }

    /// Prints what a brush file holds as the studio's controls:
    /// `BRUSH_FILE=path cargo test -p procraft-engine brush_file_controls -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn brush_file_controls() {
        let Ok(path) = std::env::var("BRUSH_FILE") else { return };
        let bytes = std::fs::read(path).unwrap();
        if let Some((name, brushes)) = read_set(&bytes) {
            println!(
                "set {name}: {}",
                brushes
                    .iter()
                    .map(|b| format!("{} ({:?})", b.name, matches!(b.shape, Texture::Image { .. })))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return;
        }
        let file = read(&bytes).unwrap();
        println!(
            "{} by {:?}; shape {:?}",
            file.name,
            file.author,
            match &file.shape {
                Texture::Image { bytes, .. } => format!("image of {} bytes", bytes.len()),
                t => format!("{t:?}"),
            }
        );
        for (k, v) in &file.settings {
            println!("  {k} = {v:.4}");
        }
    }
}
