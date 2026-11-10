//! Brush files: a zip holding a brush's settings as a keyed archive (`Brush.archive`), a preview
//! (`QuickLook/Thumbnail.png`) and any shape and grain images of its own (`Shape.png`,
//! `Grain.png`). Read into the brush studio's controls (the keys and values
//! [`Brush::set_setting`] takes) and written back from a brush, the controls turned into the
//! file's own numbers. A set file is a zip of brushes, each in a folder of its own, with
//! `brushset.plist` giving the set's name and the folders in order.

use crate::brush::{BRUSH_BEHIND, Brush, GRAIN_HEIGHT, GRAIN_LINEAR_HEIGHT, Notches, PressureCurve, SIZE_CURVE};
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
    /// Its own pressure curves for other controls, those it has (straight ones too: a control
    /// without one doesn't follow the pressure).
    pub pressure_curves: Vec<(PressureCurve, Vec<Vec2>)>,
    pub shape: Texture,
    pub grain: Texture,
    /// What it paints a model's metalness and roughness through.
    pub metallic: Texture,
    pub roughness: Texture,
    /// Where its size and opacity sliders stand for painting, smudging and erasing (slider
    /// places 0..1), where the file says.
    pub sliders: [Option<[f32; 2]>; 3],
    /// The sliders' notches for painting, smudging and erasing.
    pub notches: Notches,
    /// Its preview (`QuickLook/Thumbnail.png`), if it has one.
    pub thumbnail: Option<Vec<u8>>,
    /// Its author's picture and signature (PNG), if it has them.
    pub author_picture: Option<Vec<u8>>,
    pub signature: Option<Vec<u8>>,
    /// A dual brush's second (in the folder `Sub01` beside it).
    pub secondary: Option<Box<BrushFile>>,
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
pub const PIXELS_PER_SIZE: f32 = 256.0;

/// How much of a stamp's square a hard round tip drawn as an image covers: a round tip of ours
/// (which fills its stamp) stands in for one at this share of the size.
pub const ROUND_TIP_SHARE: f32 = 0.8157;

/// Sets `brush`'s sizes from a file's (`maxSize`, `minSize`, in units of its
/// [`Brush::size_unit`], so its shape first), the least kept no larger than the most, and the most
/// no larger than its size controls reach (its spacing sets that, so its spacing first): kept as
/// the controls' places, a size past their end would come back smaller after a restart.
pub fn set_sizes(brush: &mut Brush, max_size: f32, min_size: Option<f32>) {
    let unit = brush.size_unit();
    if max_size.is_finite() {
        brush.max_size = (max_size.max(0.0) * unit).max(0.5).min(brush.most_size() * unit);
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
    // tilt switches: leaning drives the jitter, count or depth (each from its own angle)
    ("jitter_lateral_tilt", "plotJitterTilt", Kept::Switch),
    ("jitter_lateral_tilt_angle", "plotJitterTiltAngle", Kept::Same),
    ("jitter_linear_tilt", "plotJitterLongitudinalTilt", Kept::Switch),
    ("jitter_linear_tilt_angle", "plotJitterLongitudinalTiltAngle", Kept::Same),
    ("count_tilt", "shapeCountTilt", Kept::Switch),
    ("count_tilt_angle", "shapeCountTiltAngle", Kept::Same),
    ("grain_depth_tilt", "textureDepthTilt", Kept::Switch),
    ("grain_depth_tilt_angle", "textureDepthTiltAngle", Kept::Same),
    ("stamp_hue_tilt", "hueJitterTilt", Kept::Switch),
    ("stamp_hue_tilt_angle", "hueJitterTiltAngle", Kept::Same),
    ("stamp_saturation_tilt", "saturationJitterTilt", Kept::Switch),
    ("stamp_saturation_tilt_angle", "saturationJitterTiltAngle", Kept::Same),
    ("stamp_lightness_tilt", "lightnessJitterTilt", Kept::Switch),
    ("stamp_lightness_tilt_angle", "lightnessJitterTiltAngle", Kept::Same),
    ("stamp_darkness_tilt", "darknessJitterTilt", Kept::Switch),
    ("stamp_darkness_tilt_angle", "darknessJitterTiltAngle", Kept::Same),
    ("stamp_secondary_tilt", "secondaryColorJitterTilt", Kept::Switch),
    ("stamp_secondary_tilt_angle", "secondaryColorJitterTiltAngle", Kept::Same),
    ("attack_tilt", "attackTilt", Kept::Switch),
    ("attack_tilt_angle", "attackTiltAngle", Kept::Same),
    // the barrel roll's switch for the attack, as `attackTilt` is the tilt's
    ("attack_roll", "attackRoll", Kept::Switch),
    ("tilt_roundness_max", "dynamicsTiltShapeRoundness", Kept::Same),
    // the roundness at full pressure (the tilt's counterpart above), and a switch on the
    // pressure's size: kept for a brush shared again, not drawn
    ("pressure_roundness_max", "dynamicsPressureShapeRoundness", Kept::Same),
    ("pressure_size_clamped", "maxPressureSizeClamped", Kept::Switch),
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
    // the materials it paints on a model: metalness, roughness and height, each source tiled as
    // the grain is
    ("metallic_amount", "metallicAmount", Kept::Same),
    ("metallic_scale", "metallicScale", Kept::Squared16),
    ("roughness_amount", "roughnessAmount", Kept::Same),
    ("roughness_scale", "roughnessScale", Kept::Squared16),
    ("height_amount", "heightAmount", Kept::Same),
    ("height_scale", "heightScale", Kept::Squared16),
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

/// Where brush files keep each [`PressureCurve`].
pub const PRESSURE_CURVES: [(PressureCurve, &str); 36] = [
    (PressureCurve::LateralJitter, "dynamicsPressurePlotJitterCurve"),
    (PressureCurve::LinearJitter, "dynamicsPressurePlotJitterLongitudinalCurve"),
    (PressureCurve::Count, "dynamicsPressureStampCountCurve"),
    (PressureCurve::CountJitter, "dynamicsPressureStampCountJitterCurve"),
    (PressureCurve::SizeJitter, "dynamicsPressureSizeJitterCurve"),
    (PressureCurve::OpacityJitter, "dynamicsPressureOpacityJitterCurve"),
    (PressureCurve::Scatter, "dynamicsPressureShapeScatterCurve"),
    (PressureCurve::RoundnessJitterY, "dynamicsPressureShapeRoundessVerticalJitterCurve"),
    (PressureCurve::RoundnessJitterX, "dynamicsPressureShapeRoundessHorizontalJitterCurve"),
    (PressureCurve::GrainMovement, "dynamicsPressureGrainMovementCurve"),
    (PressureCurve::GrainScale, "dynamicsPressureGrainScaleCurve"),
    (PressureCurve::GrainZoom, "dynamicsPressureGrainZoomCurve"),
    (PressureCurve::GrainDepth, "dynamicsPressureGrainDepthCurve"),
    (PressureCurve::GrainDepthJitter, "dynamicsPressureGrainDepthJitterCurve"),
    (PressureCurve::WetGrade, "dynamicsPressureGradeCurve"),
    (PressureCurve::WetBlur, "dynamicsPressureBlurCurve"),
    (PressureCurve::WetBlurJitter, "dynamicsPressureBlurJitterCurve"),
    (PressureCurve::WetnessJitter, "dynamicsPressureWetnessCurve"),
    (PressureCurve::Hue, "dynamicsPressureHueCurve"),
    (PressureCurve::Saturation, "dynamicsPressureSaturationCurve"),
    (PressureCurve::Brightness, "dynamicsPressureBrightnessCurve"),
    (PressureCurve::Secondary, "dynamicsPressureSecondaryColorCurve"),
    (PressureCurve::HueJitter, "dynamicsPressureHueJitterCurve"),
    (PressureCurve::SaturationJitter, "dynamicsPressureSaturationJitterCurve"),
    (PressureCurve::LightnessJitter, "dynamicsPressureLightnessJitterCurve"),
    (PressureCurve::DarknessJitter, "dynamicsPressureDarknessJitterCurve"),
    (PressureCurve::SecondaryJitter, "dynamicsPressureSecondaryColorJitterCurve"),
    (PressureCurve::Flow, "dynamicsPressureTransferModulationCurve"),
    (PressureCurve::Bleed, "dynamicsPressureBleedCurve"),
    (PressureCurve::Roundness, "dynamicsPressureShapeRoundnessCurve"),
    (PressureCurve::Dilution, "dynamicsPressureDilutionCurve"),
    (PressureCurve::Charge, "dynamicsPressureCharge"),
    (PressureCurve::Attack, "dynamicsPressureAttackCurve"),
    (PressureCurve::WetPull, "dynamicsPressureWetPullCurve"),
    (PressureCurve::SmudgePull, "dynamicsPressureSmudgePullCurve"),
    (PressureCurve::SpacingJitter, "dynamicsPressurePlotSpacingJitter"),
];

/// The key brush files keep `kind` under.
pub fn pressure_curve_key(kind: PressureCurve) -> &'static str {
    PRESSURE_CURVES.iter().find(|(k, _)| *k == kind).map_or("", |(_, key)| key)
}

/// The curve kept under `key`, if it is one.
pub fn pressure_curve_of(key: &str) -> Option<PressureCurve> {
    PRESSURE_CURVES.iter().find(|(_, k)| *k == key).map(|(kind, _)| *kind)
}

/// Reads the brush whose files are under `prefix` in `zip`.
fn read_in(zip: &ZipReader<'_>, prefix: &str) -> Option<BrushFile> {
    let archive = Archive::read(&zip.read(&format!("{prefix}Brush.archive"))?)?;
    let root = archive.root()?;
    let f = Fields(root);
    let mut settings: Vec<(String, f32)> = Vec::new();
    // (a control set again takes the later value, where it was first set)
    let mut set = |key: &str, v: f32| {
        if v.is_finite() {
            match settings.iter_mut().find(|(k, _)| k == key) {
                Some(kept) => kept.1 = v,
                None => settings.push((key.to_owned(), v)),
            }
        }
    };
    for &(control, key, kept) in PLAIN {
        if let Some(v) = f.num(key) {
            set(control, to_control(kept, v));
        }
    }
    // a file without the pen's own taper keys tapers a pen as it does touch
    for (pen, touch, control) in [
        ("pencilTaperStartLength", "taperStartLength", "pen_taper_start"),
        ("pencilTaperEndLength", "taperEndLength", "pen_taper_end"),
        ("pencilTaperSize", "taperSize", "pen_taper_size"),
        ("pencilTaperOpacity", "taperOpacity", "pen_taper_opacity"),
        ("pencilTaperShape", "taperShape", "pen_taper_tip"),
    ] {
        if f.num(pen).is_none()
            && let Some(v) = f.num(touch)
        {
            set(control, v as f32);
        }
    }
    // a file from before the linear jitter had its own keys jitters along the stroke as it does
    // across (its tilt switch and angle then follow the lateral's, see `Brush::setting`); one with
    // its own but no tilt keys has the switch off and the angle the tilt angle
    if f.num("plotJitterLongitudinal").is_none() {
        if let Some(v) = f.num("plotJitter") {
            set("jitter_linear", to_control(Kept::Curve(3.0), v));
        }
    } else {
        if f.num("plotJitterLongitudinalTilt").is_none() {
            set("jitter_linear_tilt", 0.0);
        }
        if f.num("plotJitterLongitudinalTiltAngle").is_none()
            && let Some(v) = f.num("dynamicsTiltAngle")
        {
            set("jitter_linear_tilt_angle", v as f32);
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
    let mut switches =
        (f.flag("renderingRecursiveMixing"), f.flag("renderingMaxTransfer"), f.flag("renderingModulatedTransfer"));
    // a file from before the rendering modes numbers how it glazes (or only says whether it
    // does): the mode it drew in, its wet mix kept only by the fourth kind and by a brush that
    // doesn't glaze
    match f.num("dynamicsRenderType").or_else(|| f.num("dynamicsGlazeType")).map(|t| t.round() as i64) {
        Some(kind @ 0..=3) => {
            switches = match kind {
                1 => (false, false, false),
                2 => (false, true, false),
                _ => (true, false, false),
            };
            if kind < 3 {
                set("wet_dilution", 0.0);
            }
        }
        Some(_) => {}
        None => {
            if let Some(glazed) = f.num("dynamicsGlaze").map(|g| g >= 0.5) {
                switches = if glazed { (false, false, false) } else { (true, false, false) };
                if glazed {
                    set("wet_dilution", 0.0);
                }
            }
        }
    }
    if let Some(mode) = RENDERING.iter().position(|&r| r == switches) {
        set("rendering_mode", mode as f32);
    }
    // the classic taper is the old one; a file from before there was another, its taper set but
    // no version said, has the old one
    if let Some(v) = f.num("taperVersion") {
        set("classic_taper", if v < 0.5 { 1.0 } else { 0.0 });
    } else if f.num("taperStartLength").is_some() || f.num("taperEndLength").is_some() {
        set("classic_taper", 1.0);
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
    // the editors' turns of their images, as an orientation (1 none, 2 a half turn, 3 and 4 a
    // quarter turn one way and the other; the grain's two quarter turns the other way round from
    // the shape's): the grain's kept (half turns, -1..1), the shape's added to its angle
    let orientation = |keys: &[&str]| keys.iter().find_map(|k| f.num(k)).map_or(1, |n| n.round() as i64);
    let grain_turns = match orientation(&["textureOrientation", "grainOrientation"]) {
        2 => 1.0,
        3 => -0.5,
        4 => 0.5,
        _ => 0.0,
    };
    if grain_turns != 0.0 {
        set("grain_turn", grain_turns);
    }
    let shape_turn = match orientation(&["shapeOrientation"]) {
        2 => std::f32::consts::PI,
        3 => std::f32::consts::FRAC_PI_2,
        4 => -std::f32::consts::FRAC_PI_2,
        _ => 0.0,
    };
    // how a dual brush's two strokes combine: as the layers' modes number, or by the grain's own
    if let Some(n) = ["dualBlendModeExtended", "dualBlendMode"].iter().find_map(|k| f.num(k)) {
        let n = n.round() as i64;
        let own = [GRAIN_HEIGHT, GRAIN_LINEAR_HEIGHT].into_iter().find(|&m| i64::from(m) == n);
        set("dual_blend", own.unwrap_or_else(|| crate::native::blend_from(n).id()) as f32);
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
    let mut pressure_curves: Vec<(PressureCurve, Vec<Vec2>)> =
        PRESSURE_CURVES.iter().filter_map(|&(kind, key)| Some((kind, f.curve(key)?))).collect();
    // a file from before the linear jitter had its own keys: it follows the lateral one's curve
    if f.num("plotJitterLongitudinal").is_none()
        && !pressure_curves.iter().any(|(k, _)| *k == PressureCurve::LinearJitter)
        && let Some((_, c)) = pressure_curves.iter().find(|(k, _)| *k == PressureCurve::LateralJitter)
    {
        pressure_curves.push((PressureCurve::LinearJitter, c.clone()));
    }
    Some(BrushFile {
        name: f.text("name").unwrap_or_else(|| "Imported Brush".to_owned()),
        author: f.text("authorName").unwrap_or_default(),
        settings,
        max_size: f.num("maxSize").unwrap_or(1.0) as f32,
        min_size: f.num("minSize").map(|v| v as f32),
        // with the shape editor's turn of its image
        shape_angle: f.num("shapeAngle").unwrap_or(0.0) as f32 + shape_turn,
        pressure_curve: f.curve("dynamicsPressureOverall").filter(|c| !is_straight(c)),
        size_curve: f.curve("dynamicsPressureSizeCurve").filter(|c| !is_straight(c)),
        opacity_curve: f.curve("dynamicsPressureOpacityCurve").filter(|c| !is_straight(c)),
        pressure_curves,
        shape: texture("bundledShapePath", "Shape.png", "shapeInverted"),
        grain: texture("bundledGrainPath", "Grain.png", "textureInverted"),
        metallic: texture("bundledMetallicPath", "Metallic.png", "metallicGrainInverted"),
        roughness: texture("bundledRoughnessPath", "Roughness.png", "roughnessGrainInverted"),
        sliders: SLIDERS.map(|(size, opacity)| {
            let (size, opacity) = (f.num(size)?, f.num(opacity)?);
            (size.is_finite() && opacity.is_finite())
                .then(|| [(size.clamp(0.0, 1.0) as f32).powf(1.0 / SIZE_CURVE), opacity.clamp(0.0, 1.0) as f32])
        }),
        notches,
        thumbnail: zip.read(&format!("{prefix}QuickLook/Thumbnail.png")),
        author_picture: zip.read(&format!("{prefix}{AUTHOR_PICTURE}")),
        signature: zip.read(&format!("{prefix}{SIGNATURE}")),
        secondary: if prefix.ends_with(SECONDARY) {
            None
        } else {
            read_in(zip, &format!("{prefix}{SECONDARY}")).map(Box::new)
        },
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
    /// The metalness and roughness sources (PNG), or none for the amount all over.
    pub metallic_png: Option<&'a [u8]>,
    pub roughness_png: Option<&'a [u8]>,
    /// The brush's preview stroke (PNG).
    pub thumbnail_png: &'a [u8],
    /// Its author's picture and signature (PNG), if it has them.
    pub author_png: Option<&'a [u8]>,
    pub signature_png: Option<&'a [u8]>,
    /// A dual brush's second's pictures (its preview left empty).
    pub secondary: Option<Box<Pictures<'a>>>,
}

/// Where a brush's folder keeps its author's picture and signature.
/// The keys of the size and opacity sliders' places for painting, smudging and erasing (the size as
/// the size it sets: its place to the power of its curve).
const SLIDERS: [(&str, &str); 3] =
    [("paintSize", "paintOpacity"), ("smudgeSize", "smudgeOpacity"), ("eraseSize", "eraseOpacity")];

const AUTHOR_PICTURE: &str = "AuthorPicture/AuthorPicture.png";
/// The folder a dual brush's second is kept in, beside the first's files.
const SECONDARY: &str = "Sub01/";
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
    let author_name = b.string(author);
    put("authorName", author_name);
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
    if brush.secondary.is_some() || brush.extra.contains_key("dual_blend") {
        let dual = match brush.setting("dual_blend").round() as u32 {
            own @ (GRAIN_HEIGHT | GRAIN_LINEAR_HEIGHT) => i64::from(own),
            _ => number("dual_blend"),
        };
        put("dualBlendMode", Value::Int(crate::native::older_blends(dual).1));
        put("dualBlendModeExtended", Value::Int(dual));
    }
    put("shapeAngle", Value::Real(f64::from(brush.shape.rotation)));
    let grain_turn = brush.setting("grain_turn");
    if grain_turn != 0.0 {
        let orientation = if grain_turn < 0.0 {
            3
        } else if grain_turn < 1.0 {
            4
        } else {
            2
        };
        put("textureOrientation", Value::Int(orientation));
        put("grainOrientation", Value::Int(orientation));
    }
    // a stock image as the brush's file named it; otherwise the image is its own, the file by
    // its own name
    for (stock, path, inverted) in [
        (&brush.stock.shape, "bundledShapePath", "shapeInverted"),
        (&brush.stock.grain, "bundledGrainPath", "textureInverted"),
        (&brush.stock.metallic, "bundledMetallicPath", "metallicGrainInverted"),
        (&brush.stock.roughness, "bundledRoughnessPath", "roughnessGrainInverted"),
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
    // the pressure response, its own curves for the size and the opacity, and those the brush has
    // for its other controls
    let others = brush.pressure_curves.iter().map(|(kind, c)| (pressure_curve_key(kind), c));
    for (key, c) in [
        ("dynamicsPressureOverall", &brush.pressure_curve),
        ("dynamicsPressureSizeCurve", &brush.size_curve),
        ("dynamicsPressureOpacityCurve", &brush.opacity_curve),
    ]
    .into_iter()
    .chain(others)
    {
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
    // where its sliders stand: as kept, else where they start (its preview size, fully opaque)
    let start = [brush.setting("preview_size").clamp(0.0, 1.0).powf(1.0 / SIZE_CURVE), 1.0];
    for ((size, opacity), slider) in SLIDERS.iter().zip(brush.sliders) {
        let [place, alpha] = slider.unwrap_or(start);
        put(size, Value::Real(f64::from(place.clamp(0.0, 1.0).powf(SIZE_CURVE))));
        put(opacity, Value::Real(f64::from(alpha.clamp(0.0, 1.0))));
    }
    let refs: Vec<(&str, Value)> = fields.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    b.fill(&root, &["SilicaBrush", "ValkyrieBrush", "NSObject"], refs);
    let archive = b.finish(root);
    zip.add(&format!("{prefix}Brush.archive"), &archive);
    zip.add(&format!("{prefix}Shape.png"), pictures.shape_png);
    for (name, png) in [
        ("Grain.png", pictures.grain_png),
        ("Metallic.png", pictures.metallic_png),
        ("Roughness.png", pictures.roughness_png),
    ] {
        if let Some(png) = png {
            zip.add(&format!("{prefix}{name}"), png);
        }
    }
    // (a dual brush's second has no preview of its own)
    if !pictures.thumbnail_png.is_empty() {
        zip.add(&format!("{prefix}QuickLook/Thumbnail.png"), pictures.thumbnail_png);
    }
    for (path, png) in [(AUTHOR_PICTURE, pictures.author_png), (SIGNATURE, pictures.signature_png)] {
        if let Some(png) = png {
            zip.add(&format!("{prefix}{path}"), png);
        }
    }
    if let (Some(second), Some(pictures)) = (&brush.secondary, &pictures.secondary) {
        add_brush(zip, &format!("{prefix}{SECONDARY}"), second, author, pictures);
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
        brush.set_setting("metallic_amount", 0.7);
        brush.set_setting("roughness_scale", 0.3);
        brush.notches.sizes[0] = vec![0.25, 0.5];
        brush.size_curve = crate::curve::Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.42, 0.2), Vec2::new(1.0, 1.0)]);
        let file = write(
            &brush,
            "Someone",
            &Pictures {
                shape_png: b"shape",
                grain_png: Some(b"grain"),
                metallic_png: Some(b"metal"),
                roughness_png: None,
                thumbnail_png: b"thumb",
                author_png: Some(b"face"),
                signature_png: None,
                secondary: None,
            },
        );
        let back = read(&file).unwrap();
        assert_eq!(back.name, "Flat Brush");
        assert_eq!(back.author, "Someone");
        assert_eq!(back.author_picture.as_deref(), Some(&b"face"[..]));
        assert_eq!(back.signature, None);
        assert_eq!(back.shape, Texture::Image { bytes: b"shape".to_vec(), inverted: false });
        assert_eq!(back.grain, Texture::Image { bytes: b"grain".to_vec(), inverted: false });
        assert_eq!(back.metallic, Texture::Image { bytes: b"metal".to_vec(), inverted: false });
        assert_eq!(back.roughness, Texture::None);
        assert_eq!(back.notches.sizes[0], vec![0.25, 0.5]);
        let size_curve = back.size_curve.as_ref().unwrap();
        assert!((size_curve[1] - Vec2::new(0.42, 0.2)).length() < 1e-5);
        assert!(back.opacity_curve.is_none());
        assert!((back.shape_angle - brush.shape.rotation).abs() < 1e-6);
        let mut again = Brush::round_pen();
        for (k, v) in &back.settings {
            again.set_setting(k, *v);
        }
        set_sizes(&mut again, back.max_size, back.min_size);
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
            "metallic_amount",
            "roughness_amount",
            "roughness_scale",
            "metallic_scale",
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
    fn the_rolls_attack_and_the_pressures_roundness_and_size_switch_go_out_again() {
        let pictures = Pictures {
            shape_png: b"s",
            grain_png: None,
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: b"t",
            author_png: None,
            signature_png: None,
            secondary: None,
        };
        let keys = ["attack_roll", "pressure_size_clamped", "pressure_roundness_max"];
        let values = |back: &BrushFile| keys.map(|key| back.settings.iter().find(|(k, _)| k == key).map(|(_, v)| *v));
        // a brush made here writes them as the files' own brushes have them: off, off and 1
        let plain = read(&write(&Brush::round_pen(), "", &pictures)).unwrap();
        assert_eq!(values(&plain), [Some(0.0), Some(0.0), Some(1.0)]);
        // one that had them keeps them, and writes them again
        let mut brush = Brush::round_pen();
        for (k, v) in keys.into_iter().zip([1.0, 1.0, 0.4]) {
            brush.set_setting(k, v);
        }
        let back = read(&write(&brush, "", &pictures)).unwrap();
        assert_eq!(values(&back), [Some(1.0), Some(1.0), Some(0.4)]);
        let mut again = Brush::round_pen();
        for (k, v) in &back.settings {
            again.set_setting(k, *v);
        }
        assert_eq!(keys.map(|k| again.setting(k)), [1.0, 1.0, 0.4]);
    }

    #[test]
    fn a_dual_brush_s_blend_mode_is_kept_whatever_its_number() {
        // a layer mode past Multiply and the grain's own heights, as the setting and as read
        for mode in [BlendMode::ColorBurn.id(), BlendMode::Divide.id(), GRAIN_HEIGHT, GRAIN_LINEAR_HEIGHT] {
            let mut brush = Brush::round_pen();
            brush.set_setting("dual_blend", mode as f32);
            assert_eq!(brush.setting("dual_blend"), mode as f32);
            let pictures = Pictures {
                shape_png: b"s",
                grain_png: None,
                metallic_png: None,
                roughness_png: None,
                thumbnail_png: b"t",
                author_png: None,
                signature_png: None,
                secondary: None,
            };
            let back = read(&write(&brush, "", &pictures)).unwrap();
            let mut again = Brush::round_pen();
            for (k, v) in &back.settings {
                again.set_setting(k, *v);
            }
            assert_eq!(again.setting("dual_blend"), mode as f32);
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
                metallic_png: None,
                roughness_png: None,
                thumbnail_png: b"t",
                author_png: None,
                signature_png: None,
                secondary: None,
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
        let pictures = Pictures {
            shape_png: b"s",
            grain_png: None,
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: b"t",
            author_png: None,
            signature_png: None,
            secondary: None,
        };
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
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: b"thumb",
            author_png: None,
            signature_png: None,
            secondary: None,
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
    fn pressure_curves_survive_a_round_trip_straight_ones_too() {
        use crate::brush::PressureCurve;
        use crate::curve::Curve;
        let mut brush = Brush { pressure_curves: Default::default(), ..Brush::round_pen() };
        let bend = Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.5, 0.8), Vec2::new(1.0, 1.0)]);
        brush.pressure_curves.set(PressureCurve::Count, Some(bend));
        brush.pressure_curves.set(PressureCurve::HueJitter, Some(Curve::linear()));
        let pictures = Pictures {
            shape_png: b"s",
            grain_png: None,
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: b"t",
            author_png: None,
            signature_png: None,
            secondary: None,
        };
        let back = read(&write(&brush, "", &pictures)).unwrap();
        let kinds: Vec<PressureCurve> = back.pressure_curves.iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, [PressureCurve::Count, PressureCurve::HueJitter]);
        let count = &back.pressure_curves[0].1;
        assert!((count[1] - Vec2::new(0.5, 0.8)).length() < 1e-5);
    }

    #[test]
    fn the_editors_quarter_turns_turn_the_shape_and_the_grain() {
        let mut b = Builder::new();
        let root = b.reserve();
        let name = b.string("Turned");
        b.fill(
            &root,
            &["SilicaBrush", "ValkyrieBrush", "NSObject"],
            vec![
                ("name", name),
                ("shapeAngle", Value::Real(0.1)),
                ("shapeOrientation", Value::Int(2)),
                ("textureRotation", Value::Real(0.75)),
                ("textureOrientation", Value::Int(3)),
                ("grainOrientation", Value::Int(3)),
            ],
        );
        let mut zip = ZipWriter::new();
        zip.add("Brush.archive", &b.finish(root));
        let back = read(&zip.finish()).unwrap();
        // 2: a half turn
        assert!((back.shape_angle - (0.1 + std::f32::consts::PI)).abs() < 1e-5, "{}", back.shape_angle);
        // the grain's 3: a quarter turn back, kept apart from its rotation control
        let of = |key: &str| back.settings.iter().filter(|(k, _)| k == key).map(|(_, v)| *v).collect::<Vec<f32>>();
        assert_eq!(of("grain_turn"), [-0.5]);
        assert_eq!(of("grain_rotation"), [0.75]);
    }

    #[test]
    fn a_file_without_linear_jitter_keys_gives_it_the_laterals_jitter() {
        use crate::brush::PressureCurve;
        let mut b = Builder::new();
        let root = b.reserve();
        let name = b.string("Old");
        let points = vec![b.string("{0, 0}"), b.string("{0.5, 0.9}"), b.string("{1, 1}")];
        let list = b.array(points, true);
        let curve = b.object(&["ValkyrieMagnitudinalCurve", "NSObject"], vec![("points", list)]);
        b.fill(
            &root,
            &["SilicaBrush", "ValkyrieBrush", "NSObject"],
            vec![("name", name), ("plotJitter", Value::Real(0.5)), ("dynamicsPressurePlotJitterCurve", curve)],
        );
        let mut zip = ZipWriter::new();
        zip.add("Brush.archive", &b.finish(root));
        let back = read(&zip.finish()).unwrap();
        let curve = |kind| back.pressure_curves.iter().find(|(k, _)| *k == kind).map(|(_, c)| c.clone());
        assert!(curve(PressureCurve::LateralJitter).is_some());
        assert_eq!(curve(PressureCurve::LinearJitter), curve(PressureCurve::LateralJitter));
        let get = |k: &str| back.settings.iter().rev().find(|(key, _)| key == k).map(|(_, v)| *v);
        assert!(get("jitter_lateral").is_some());
        assert_eq!(get("jitter_linear"), get("jitter_lateral"));
    }

    #[test]
    fn a_file_with_its_own_linear_jitter_but_no_tilt_keys_has_its_tilt_off() {
        let mut b = Builder::new();
        let root = b.reserve();
        let name = b.string("Newer");
        b.fill(
            &root,
            &["SilicaBrush", "ValkyrieBrush", "NSObject"],
            vec![
                ("name", name),
                ("plotJitterLongitudinal", Value::Real(0.2)),
                ("plotJitterTilt", Value::Bool(true)),
                ("plotJitterTiltAngle", Value::Real(0.6)),
                ("dynamicsTiltAngle", Value::Real(0.3)),
            ],
        );
        let mut zip = ZipWriter::new();
        zip.add("Brush.archive", &b.finish(root));
        let back = read(&zip.finish()).unwrap();
        let get = |k: &str| back.settings.iter().rev().find(|(key, _)| key == k).map(|(_, v)| *v);
        assert_eq!(get("jitter_linear_tilt"), Some(0.0));
        assert_eq!(get("jitter_linear_tilt_angle"), Some(0.3));
        assert_eq!(get("jitter_lateral_tilt"), Some(1.0));
    }

    #[test]
    fn a_file_without_pen_taper_keys_tapers_a_pen_as_touch() {
        let mut b = Builder::new();
        let root = b.reserve();
        let name = b.string("Old");
        b.fill(
            &root,
            &["SilicaBrush", "ValkyrieBrush", "NSObject"],
            vec![("name", name), ("taperStartLength", Value::Real(0.4)), ("pencilTaperEndLength", Value::Real(0.2))],
        );
        let mut zip = ZipWriter::new();
        zip.add("Brush.archive", &b.finish(root));
        let back = read(&zip.finish()).unwrap();
        let get = |k: &str| back.settings.iter().rev().find(|(key, _)| key == k).map(|(_, v)| *v);
        assert_eq!(get("pen_taper_start"), Some(0.4));
        assert_eq!(get("touch_taper_start"), Some(0.4));
        // its own key wins
        assert_eq!(get("pen_taper_end"), Some(0.2));
        // its taper set with no version: from before there was more than the classic one
        assert_eq!(get("classic_taper"), Some(1.0));
    }

    #[test]
    fn a_file_says_which_taper_it_has_or_has_none() {
        let read_with = |fields: Vec<(&str, Value)>| {
            let mut b = Builder::new();
            let root = b.reserve();
            let name = b.string("Brush");
            let mut all = vec![("name", name)];
            all.extend(fields);
            b.fill(&root, &["SilicaBrush", "ValkyrieBrush", "NSObject"], all);
            let mut zip = ZipWriter::new();
            zip.add("Brush.archive", &b.finish(root));
            let back = read(&zip.finish()).unwrap();
            back.settings.iter().rev().find(|(key, _)| key == "classic_taper").map(|(_, v)| *v)
        };
        let taper = || ("taperEndLength", Value::Real(1.0));
        assert_eq!(read_with(vec![taper(), ("taperVersion", Value::Int(1))]), Some(0.0));
        assert_eq!(read_with(vec![taper(), ("taperVersion", Value::Int(0))]), Some(1.0));
        assert_eq!(read_with(vec![taper()]), Some(1.0));
        // no taper set at all: left as the brush it starts from has it
        assert_eq!(read_with(vec![]), None);
    }

    #[test]
    fn an_old_files_glaze_type_picks_its_rendering_mode() {
        let mode_and_dilution = |extra: Vec<(&str, Value)>| {
            let mut b = Builder::new();
            let root = b.reserve();
            let name = b.string("Old");
            let mut fields = vec![("name", name), ("dynamicsMix", Value::Real(0.8))];
            fields.extend(extra);
            b.fill(&root, &["SilicaBrush", "NSObject"], fields);
            let mut zip = ZipWriter::new();
            zip.add("Brush.archive", &b.finish(root));
            let back = read(&zip.finish()).unwrap();
            let get = |k: &str| back.settings.iter().rev().find(|(key, _)| key == k).map(|(_, v)| *v);
            (get("rendering_mode"), get("wet_dilution"))
        };
        // light glaze, intense glaze and intense blending, the paint's mixing cleared but for the
        // last kind
        assert_eq!(mode_and_dilution(vec![("dynamicsGlazeType", Value::Int(2))]), (Some(0.0), Some(0.0)));
        assert_eq!(mode_and_dilution(vec![("dynamicsGlazeType", Value::Int(1))]), (Some(2.0), Some(0.0)));
        assert_eq!(mode_and_dilution(vec![("dynamicsGlazeType", Value::Int(0))]), (Some(5.0), Some(0.0)));
        assert_eq!(mode_and_dilution(vec![("dynamicsGlazeType", Value::Int(3))]), (Some(5.0), Some(0.8)));
        // the render type before the glaze type; a switch alone
        let both = vec![("dynamicsRenderType", Value::Int(2)), ("dynamicsGlazeType", Value::Int(1))];
        assert_eq!(mode_and_dilution(both).0, Some(0.0));
        assert_eq!(mode_and_dilution(vec![("dynamicsGlaze", Value::Bool(false))]), (Some(5.0), Some(0.8)));
        assert_eq!(mode_and_dilution(vec![("dynamicsGlaze", Value::Bool(true))]), (Some(2.0), Some(0.0)));
        // none of them: as its switches say
        assert_eq!(mode_and_dilution(Vec::new()), (Some(2.0), Some(0.8)));
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
    #[test]
    fn a_size_past_what_the_controls_reach_is_kept_to_their_end() {
        let mut b = Brush::round_pen();
        // closely spaced: it may be smaller than a roomy brush
        b.spacing = Brush::spacing_within_reach(0.01);
        set_sizes(&mut b, 1e6, Some(0.5));
        let most = b.most_size() * b.size_unit();
        assert!((b.max_size - most).abs() < 1e-3, "{} {most}", b.max_size);
        // kept as its control's place and read back, as after a restart: the same
        let mut again = b.clone();
        again.set_setting("max_size", b.setting("max_size"));
        assert!((again.max_size - b.max_size).abs() < 1e-3 * b.max_size);
    }
}
