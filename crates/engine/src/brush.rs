//! Brush settings: what a stroke's stamps look like and how input drives them.
//!
//! Only the parameters the stroke builder uses so far are here; the full set (shape and grain
//! images, rendering modes, wet mix, colour dynamics, tilt and roll) is added as it is drawn.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::curve::Curve;
use crate::textures::TEXTURE_SIZE;

/// An image of the user's own for a brush's shape or grain: square greyscale, `TEXTURE_SIZE` on a
/// side, rows top to bottom, white = paint; known by a hash of its pixels.
#[derive(Clone, Debug)]
pub struct Source {
    pub id: u64,
    pub pixels: Arc<Vec<u8>>,
}

impl PartialEq for Source {
    fn eq(&self, other: &Source) -> bool {
        self.id == other.id
    }
}

impl Source {
    /// The image, or none if it isn't `TEXTURE_SIZE` square.
    pub fn new(pixels: Vec<u8>) -> Option<Source> {
        if pixels.len() != TEXTURE_SIZE * TEXTURE_SIZE {
            return None;
        }
        // FNV-1a
        let id =
            pixels.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
        Some(Source { id, pixels: Arc::new(pixels) })
    }

    /// The pixels as text (base64), to keep with a brush.
    pub fn to_text(&self) -> String {
        crate::base64::encode(&self.pixels)
    }

    /// An image kept as text by [`Source::to_text`]; none if it isn't one.
    pub fn from_text(text: &str) -> Option<Source> {
        Source::new(crate::base64::decode(text)?)
    }
}

/// Size and opacity at the ends of a stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct Taper {
    /// Length of the start taper, 0..1: of [`TAPER_TIME_MS`] for the newer taper, of
    /// [`CLASSIC_TAPER_SPAN`] canvas pixels for the classic one.
    pub start: f32,
    /// Length of the end taper, likewise.
    pub end: f32,
    /// How thin the tips get: 0 = no change, 1 = down to nothing.
    pub size: f32,
    /// How transparent the tips get: 0 = no change, 1 = fully transparent.
    pub opacity: f32,
}

/// A newer taper of length 1 lasts this many milliseconds of the stroke, from when it starts or
/// back from when it ends.
pub const TAPER_TIME_MS: f64 = 83.34;

/// A classic taper of length 1 spans this many canvas pixels, whatever the brush's size.
pub const CLASSIC_TAPER_SPAN: f32 = 512.0;

impl Taper {
    pub const NONE: Taper = Taper { start: 0.0, end: 0.0, size: 0.0, opacity: 0.0 };

    /// The multipliers (size, opacity) where the start taper has grown `start` and the end taper
    /// `end` of the way to full (0..1 each), with a tip of sharpness `tip` (0 comes to a point,
    /// 1 stays almost whole until the very end).
    pub fn applied(&self, start: f32, end: f32, tip: f32) -> (f32, f32) {
        let power = Taper::tip_power(tip);
        let left = |grown: f32| 1.0 - grown.clamp(0.0, 1.0).powf(power);
        let (size, opacity) = (self.size.clamp(0.0, 1.0), self.opacity.clamp(0.0, 1.0));
        let (s, e) = (left(start), left(end));
        let thinned = (1.0 - size * s) * (1.0 - size * e);
        let faded = (1.0 - (opacity * s).powf(2.5)) * (1.0 - (opacity * e).powf(2.5));
        (thinned, faded)
    }

    /// The power a taper's growth is taken to for a tip of sharpness `tip` (1 for a sharp one).
    pub fn tip_power(tip: f32) -> f32 {
        1.0 - TIP_BLUNTING * tip.clamp(0.0, 1.0)
    }
}

/// How far a blunt tip (1) bends the taper's growth: it grows to the power 1 less this.
const TIP_BLUNTING: f32 = 0.984375;

/// How a stroke's stamps build up within the stroke (the studio's rendering modes: Light Glaze
/// keeps the strongest; Uniformed Glaze, Intense Glaze and Intense Blending build up; Heavy Glaze
/// and Uniform Blending build up gradually).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rendering {
    /// Each stamp adds to what is there ("over"), up to the stroke's opacity.
    #[default]
    BuildUp,
    /// Overlapping stamps don't build up: the stroke keeps the strongest stamp at each pixel.
    Strongest,
    /// Each stamp adds less the more is there (by what is left, squared).
    Gradual,
    /// Each stamp adds towards its transfer (what light pressure lets through), never past it.
    Capped,
}

/// What the pen's pressure drives, each taking it through a curve of its own and following it at
/// a speed of its own (see [`Brush::pressures`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureUse {
    Size = 0,
    Opacity = 1,
    Bleed = 2,
    Roundness = 3,
}

/// What else a brush may give a pressure curve of its own: the control is then scaled by the
/// curve at the stamp's pressure (a finger's is full), and without one it doesn't follow the
/// pressure at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PressureCurve {
    LateralJitter,
    LinearJitter,
    Count,
    CountJitter,
    SizeJitter,
    OpacityJitter,
    Scatter,
    /// The roundness jitter's two ways: along the stamp's height and its width.
    RoundnessJitterY,
    RoundnessJitterX,
    GrainMovement,
    GrainScale,
    GrainZoom,
    GrainDepth,
    GrainDepthJitter,
    WetGrade,
    WetBlur,
    WetBlurJitter,
    WetnessJitter,
    /// The colour's own response to pressure: hue, saturation, brightness and the secondary
    /// colour's share.
    Hue,
    Saturation,
    Brightness,
    Secondary,
    /// The stamp colour jitters.
    HueJitter,
    SaturationJitter,
    LightnessJitter,
    DarknessJitter,
    SecondaryJitter,
}

/// A brush's own [`PressureCurve`]s, those it has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PressureCurves(BTreeMap<PressureCurve, Curve>);

impl PressureCurves {
    pub fn get(&self, kind: PressureCurve) -> Option<&Curve> {
        self.0.get(&kind)
    }

    /// Gives `kind` a curve, or takes its curve away.
    pub fn set(&mut self, kind: PressureCurve, curve: Option<Curve>) {
        match curve {
            Some(c) => self.0.insert(kind, c),
            None => self.0.remove(&kind),
        };
    }

    /// The curves a new brush has: the colour's following the pressure, straight.
    pub fn new_brush() -> PressureCurves {
        let colour =
            [PressureCurve::Hue, PressureCurve::Saturation, PressureCurve::Brightness, PressureCurve::Secondary];
        PressureCurves(colour.into_iter().map(|k| (k, Curve::linear())).collect())
    }

    /// What `kind` is scaled by at `pressure`: 1 without a curve.
    pub fn at(&self, kind: PressureCurve, pressure: f32) -> f32 {
        self.0.get(&kind).map_or(1.0, |c| c.eval(pressure))
    }

    pub fn iter(&self) -> impl Iterator<Item = (PressureCurve, &Curve)> {
        self.0.iter().map(|(k, c)| (*k, c))
    }
}

/// The size slider's curve: the size it sets is its place to this power.
pub const SIZE_CURVE: f32 = 2.2;

/// A stamp's opacity is its flow, pressure, tilt and speed together, through the brush's opacity
/// range, to this power: light touches fall away fast.
pub const FLOW_POWER: f32 = 2.5;

/// A grain's own blend modes, numbered past the layers' (and as brush files number them): the
/// coverage, raised twelvefold by the depth, as a height the grain cuts into, straight or also
/// thinned over the grain's light (the renderer's `grain_combine`).
pub const GRAIN_HEIGHT: u32 = 27;
pub const GRAIN_LINEAR_HEIGHT: u32 = 28;

/// The modes a grain blends by, in the order the studio lists them: some of the layers' and the
/// grain's own two. (A brush file may name any layer mode; it is kept and drawn by it.)
pub fn grain_blends() -> Vec<(u32, &'static str)> {
    use crate::document::BlendMode as B;
    let mut modes: Vec<(u32, &'static str)> = [
        B::Multiply,
        B::Darken,
        B::ColorBurn,
        B::LinearBurn,
        B::Lighten,
        B::ColorDodge,
        B::Overlay,
        B::HardMix,
        B::Difference,
        B::Subtract,
        B::Divide,
    ]
    .iter()
    .map(|m| (m.id(), m.name()))
    .collect();
    modes.extend([(GRAIN_HEIGHT, "Height"), (GRAIN_LINEAR_HEIGHT, "Linear Height")]);
    modes
}

/// The blend mode that lays a brush's paint under what the layer has (only brushes and their burnt
/// edges have it), numbered past the grain's own modes.
pub const BRUSH_BEHIND: u32 = 29;

/// The modes a brush (and its burnt edges) blends by, in the order the studio lists them: the
/// layers' and then behind.
pub fn brush_blends() -> Vec<(u32, &'static str)> {
    let mut modes: Vec<(u32, &'static str)> =
        crate::document::BlendMode::ALL.iter().map(|m| (m.id(), m.name())).collect();
    modes.push((BRUSH_BEHIND, "Behind"));
    modes
}

/// Where a grain sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GrainMode {
    /// Fixed to the canvas, like paper: applied to the whole stroke.
    #[default]
    Texturized,
    /// Travels with each stamp.
    Moving,
}

/// The stamp's shape and how it is placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// A shape image (see `textures::shape`), or none for the round tip drawn with `hardness`.
    pub image: Option<u32>,
    /// An image of the user's own, in place of `image`.
    pub source: Option<Source>,
    /// Fixed rotation, radians.
    pub rotation: f32,
    /// How far the stamp turns with the stroke's direction: 1 follows it, 0 holds still, −1
    /// turns against it.
    pub follow: f32,
    /// Turn with the direction the pen leans.
    pub follow_azimuth: bool,
    /// A random rotation, the same for every stamp of a stroke.
    pub randomise: bool,
    /// 1 = as drawn; lower squashes the stamp across its rotation.
    pub roundness: f32,
    /// How far each stamp turns at random, up to this many half turns either way (the Scatter
    /// control, 0..2).
    pub scatter: f32,
    /// Random offset from the path, in stamp diameters (some brushes of our own throw their
    /// stamps about; files keep such randomness as jitter).
    pub throw: f32,
    /// Stamps placed at each step.
    pub count: u32,
    /// Random reduction of each stamp's size and opacity, 0..1.
    pub size_jitter: f32,
    pub opacity_jitter: f32,
}

impl Default for Shape {
    fn default() -> Shape {
        Shape {
            image: None,
            source: None,
            rotation: 0.0,
            follow: 0.0,
            follow_azimuth: false,
            randomise: false,
            roundness: 1.0,
            scatter: 0.0,
            throw: 0.0,
            count: 1,
            size_jitter: 0.0,
            opacity_jitter: 0.0,
        }
    }
}

/// The texture inside the stroke.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grain {
    /// A grain image (see `textures::grain`), or none.
    pub image: Option<u32>,
    /// An image of the user's own, in place of `image`.
    pub source: Option<Source>,
    pub mode: GrainMode,
    /// Size of one grain tile on the canvas, pixels.
    pub scale: f32,
    /// How strongly the grain shows, 0..1.
    pub depth: f32,
}

/// What leaning the pen does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tilt {
    /// The tilt angle control's dial (radians, a quarter turn at most): each tilt control starts
    /// to act from its own angle, this one unless it has its own (see [`Tilt::reach`]).
    pub angle: f32,
    /// What leaning adds to the stamp's size, in the size's units ([`Brush::size_unit`]) at the
    /// most lean, before it is taken as far as the lean has gone.
    pub size: f32,
    /// Fading of the stamp as the pen leans, 0..1.
    pub opacity: f32,
}

/// A lean (radians from upright) about as flat as pens go.
pub const FLAT_TILT: f32 = 1.2;

impl Tilt {
    /// How far a pen leaning `lean` radians from upright has gone for a tilt control whose angle
    /// is `theta` (its dial's share of a quarter turn): nothing until it is down to 34° above the
    /// surface (at 0; at 1, from upright), rising as it goes down to 26°, the most it counts,
    /// where it reaches 0.5 (at 0) to 0.88 (at 1).
    pub fn reach(lean: f32, theta: f32) -> f32 {
        if !lean.is_finite() {
            return 0.0;
        }
        let altitude = (1.0 - lean / std::f32::consts::FRAC_PI_2).clamp(0.0, 1.0);
        let theta = if theta.is_finite() { theta.clamp(0.0, 1.0) } else { 0.0 };
        let span = TILT_SPAN_LOW + (TILT_SPAN_HIGH - TILT_SPAN_LOW) * theta;
        (1.0 - (altitude.max(TILT_LOWEST) - TILT_SPAN_LOW) / span).clamp(1.0 / 1024.0, 1.0)
    }
}

/// The ramp of [`Tilt::reach`], as shares of a quarter turn of altitude: the lowest it counts
/// (25.8°), and its span at a tilt angle of 0 and of 1.
const TILT_LOWEST: f32 = 0.28648;
const TILT_SPAN_LOW: f32 = 0.19099;
const TILT_SPAN_HIGH: f32 = 0.80901;

#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub name: String,
    /// Distance between stamps, as a fraction of the stamp diameter.
    pub spacing: f32,
    /// Smoothing of the stroke's path, 0..1.
    pub streamline: f32,
    /// Smoothing of pen pressure, 0..1.
    pub pressure_smoothing: f32,
    /// Edge of the round tip: 0 = soft all the way to the centre, 1 = hard.
    pub hardness: f32,
    /// Stamp diameter in canvas pixels at the size slider's minimum and maximum.
    pub min_size: f32,
    pub max_size: f32,
    /// Stroke opacity at the opacity slider's minimum and maximum.
    pub min_opacity: f32,
    pub max_opacity: f32,
    /// How much each stamp adds at full pressure, 0..1 (the studio's flow control to the power
    /// [`FLOW_POWER`]).
    pub flow: f32,
    /// How much pen pressure shrinks the stamp (0 = not at all, 1 = to nothing at no pressure).
    pub pressure_size: f32,
    /// How much pen pressure lowers the stamp's opacity.
    pub pressure_opacity: f32,
    pub pressure_curve: Curve,
    /// What the pressure (after [`Brush::pressure_curve`]) becomes for the size and for the
    /// opacity.
    pub size_curve: Curve,
    pub opacity_curve: Curve,
    /// Its own pressure curves for its other controls.
    pub pressure_curves: PressureCurves,
    /// Taper for pens and for fingers.
    pub pen_taper: Taper,
    pub touch_taper: Taper,
    pub shape: Shape,
    pub grain: Grain,
    pub tilt: Tilt,
    pub rendering: Rendering,
    /// Brush studio controls the engine does not draw yet, by key (see `studio`).
    pub extra: BTreeMap<String, f32>,
    /// Values saved on the side sliders while this brush is in use, shown there as notches.
    pub notches: Notches,
    /// Its preview as its file brought it (PNG), shown in place of a drawn one until the brush is
    /// changed.
    pub thumbnail: Option<std::sync::Arc<[u8]>>,
    /// Who made it, as its file says; none for ProCraft's own.
    pub author: Option<Author>,
    /// What resetting the brush puts back: set the first time it is opened in the studio.
    pub reset_point: Option<ResetPoint>,
    /// The stock images its file named for its shape and grain, which our own stand in for.
    pub stock: StockImages,
}

/// The stock images a brush's file names for its shape and grain (drawn with ours standing in),
/// kept so that they are named again when the brush is written out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StockImages {
    pub shape: Option<StockImage>,
    pub grain: Option<StockImage>,
}

/// A stock image as a brush file names it, and whether the file has it inverted.
#[derive(Clone, Debug, PartialEq)]
pub struct StockImage {
    pub name: String,
    pub inverted: bool,
}

/// A brush's maker as its file names them, with their picture and signature (PNG) if it has them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Author {
    pub name: String,
    pub picture: Option<std::sync::Arc<[u8]>>,
    pub signature: Option<std::sync::Arc<[u8]>>,
}

/// A brush set aside to go back to (without a reset point of its own), and when: seconds since
/// 1970.
#[derive(Clone, Debug, PartialEq)]
pub struct ResetPoint {
    pub brush: Box<Brush>,
    pub made: i64,
}

impl ResetPoint {
    /// `brush` as it is, set aside at `made`.
    pub fn of(brush: &Brush, made: i64) -> ResetPoint {
        ResetPoint { brush: Box::new(Brush { reset_point: None, ..brush.clone() }), made }
    }
}

/// A brush's saved side-slider values (its notches): sizes and opacities, slider positions 0..1,
/// kept apart for each tool that uses the brush (painting, smudging, erasing; see [`Notches::tool`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notches {
    pub sizes: [Vec<f32>; 3],
    pub opacities: [Vec<f32>; 3],
}

impl Notches {
    /// The most notches a slider takes.
    pub const MOST: usize = 4;
    /// How near a value (of the slider's length) is on a notch, and is pulled onto it.
    pub const NEAR: f32 = 0.04;

    /// Where a tool's notches are kept: 0 painting, 1 smudging, 2 erasing.
    pub fn tool(painting: bool, smudging: bool) -> usize {
        if painting {
            0
        } else if smudging {
            1
        } else {
            2
        }
    }

    /// A tool's (0..3) size or opacity notches.
    pub fn list(&self, tool: usize, size: bool) -> &[f32] {
        let lists = if size { &self.sizes } else { &self.opacities };
        lists.get(tool).map_or(&[], Vec::as_slice)
    }

    fn list_mut(&mut self, tool: usize, size: bool) -> Option<&mut Vec<f32>> {
        if size { self.sizes.get_mut(tool) } else { self.opacities.get_mut(tool) }
    }

    /// The notch `value` is on (the nearest, within [`Notches::NEAR`]), if any.
    pub fn at(&self, tool: usize, size: bool, value: f32) -> Option<f32> {
        self.list(tool, size)
            .iter()
            .copied()
            .filter(|n| (n - value).abs() < Self::NEAR)
            .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
    }

    /// Saves `value` as a notch, unless the slider is full, one is there already, or it isn't a
    /// slider position; whether it did.
    pub fn add(&mut self, tool: usize, size: bool, value: f32) -> bool {
        if !(0.0..=1.0).contains(&value) || self.at(tool, size, value).is_some() {
            return false;
        }
        match self.list_mut(tool, size) {
            Some(list) if list.len() < Self::MOST => {
                list.push(value);
                true
            }
            _ => false,
        }
    }

    /// Takes away the notch `value` is on; whether there was one.
    pub fn remove(&mut self, tool: usize, size: bool, value: f32) -> bool {
        let Some(n) = self.at(tool, size, value) else { return false };
        let Some(list) = self.list_mut(tool, size) else { return false };
        list.retain(|&v| v != n);
        true
    }
}

impl Brush {
    /// A plain round inking pen: ProCraft's own starting brush until the library exists.
    pub fn round_pen() -> Brush {
        Brush {
            name: "Round Pen".to_owned(),
            spacing: 0.08,
            streamline: 0.35,
            pressure_smoothing: 0.2,
            hardness: 0.92,
            min_size: 1.0,
            max_size: 120.0,
            min_opacity: 0.0,
            max_opacity: 1.0,
            flow: 1.0,
            pressure_size: 0.85,
            pressure_opacity: 0.0,
            pressure_curve: Curve::linear(),
            size_curve: Curve::linear(),
            opacity_curve: Curve::linear(),
            pressure_curves: PressureCurves::new_brush(),
            pen_taper: Taper { start: 0.15, end: 0.2, size: 0.8, opacity: 0.0 },
            touch_taper: Taper { start: 0.1, end: 0.1, size: 0.6, opacity: 0.0 },
            shape: Shape::default(),
            grain: Grain::default(),
            tilt: Tilt::default(),
            rendering: Rendering::BuildUp,
            extra: BTreeMap::new(),
            notches: Notches::default(),
            thumbnail: None,
            author: None,
            reset_point: None,
            stock: StockImages::default(),
        }
    }

    /// Canvas pixels a stamp is across for each unit of size as brush files keep it: the
    /// stamp's square, of which a round tip of ours covers [`crate::brushfile::ROUND_TIP_SHARE`].
    pub fn size_unit(&self) -> f32 {
        let round_tip = self.shape.image.is_none() && self.shape.source.is_none();
        crate::brushfile::PIXELS_PER_SIZE * if round_tip { crate::brushfile::ROUND_TIP_SHARE } else { 1.0 }
    }

    /// Stamp diameter (canvas px) for a size slider at 0..1: the slider to the power
    /// [`SIZE_CURVE`] (so small sizes get most of it), from the least size to the most.
    pub fn diameter(&self, slider: f32) -> f32 {
        let s = if slider.is_finite() { slider.clamp(0.0, 1.0) } else { 0.5 };
        let most = self.biggest();
        let least = self.min_size.min(most);
        (least + (most - least) * s.powf(SIZE_CURVE)).max(0.5)
    }

    /// The most size drawn (canvas pixels): the brush's own, held to what its spacing allows.
    pub fn biggest(&self) -> f32 {
        self.max_size.min(self.most_size() * self.size_unit())
    }

    /// Stroke opacity for an opacity slider at 0..1.
    pub fn opacity(&self, slider: f32) -> f32 {
        let s = if slider.is_finite() { slider.clamp(0.0, 1.0) } else { 1.0 };
        (self.min_opacity + (self.max_opacity - self.min_opacity) * s).clamp(0.0, 1.0)
    }

    /// The (size, opacity) multipliers for a pressure of 0..1.
    pub fn pressure_factors(&self, pressure: f32) -> (f32, f32) {
        let [size, opacity, ..] = self.pressures(pressure);
        self.factors(size, opacity)
    }

    /// The pressure as each of its uses takes it, through the pressure curve and their own:
    /// [`PressureUse`] order.
    pub fn pressures(&self, pressure: f32) -> [f32; 4] {
        let p = self.pressure_curve.eval(pressure);
        [self.size_curve.eval(p), self.opacity_curve.eval(p), p, p]
    }

    /// The (size, opacity) multipliers for the size's and the opacity's own pressures: an amount
    /// above 0 takes away as the pressure lightens, one below 0 as it presses.
    pub fn factors(&self, size: f32, opacity: f32) -> (f32, f32) {
        let factor = |amount: f32, p: f32| {
            let amount = amount.clamp(-1.0, 1.0);
            if amount >= 0.0 { 1.0 - amount * (1.0 - p) } else { 1.0 + amount * p }
        };
        (factor(self.pressure_size, size), factor(self.pressure_opacity, opacity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taper_thins_and_fades_its_tips_as_far_as_it_has_grown() {
        let t = Taper { start: 0.5, end: 0.5, size: 1.0, opacity: 0.5 };
        // grown fully at both ends: whole; at a tip: thinned to nothing, faded by its opacity
        assert_eq!(t.applied(1.0, 1.0, 0.0), (1.0, 1.0));
        let (size, opacity) = t.applied(0.0, 1.0, 0.0);
        assert_eq!(size, 0.0);
        assert!((opacity - (1.0 - 0.5f32.powf(2.5))).abs() < 1e-6);
        // a sharp tip grows evenly; a blunt one is almost whole a little way in
        assert!((t.applied(0.25, 1.0, 0.0).0 - 0.25).abs() < 1e-6);
        assert!(t.applied(0.05, 1.0, 1.0).0 > 0.95);
        // both ends at once take away what each other leaves
        assert!((t.applied(0.5, 0.5, 0.0).0 - 0.25).abs() < 1e-6);
        assert_eq!(Taper::NONE.applied(0.0, 0.0, 0.0), (1.0, 1.0));
    }

    #[test]
    fn tilt_reaches_past_its_angle() {
        let deg = |d: f32| d.to_radians();
        // upright, next to nothing; down to 26 degrees above the surface, the most
        assert!(Tilt::reach(0.0, 0.1) < 0.001);
        assert!((Tilt::reach(FLAT_TILT, 0.0) - 0.5).abs() < 1e-3);
        assert!((Tilt::reach(FLAT_TILT, 0.1) - 0.622).abs() < 1e-3);
        assert!((Tilt::reach(FLAT_TILT, 1.0) - 0.882).abs() < 1e-3);
        // at 0.1 it starts about 50 degrees from upright
        assert!(Tilt::reach(deg(48.0), 0.1) < 0.001);
        assert!(Tilt::reach(deg(52.0), 0.1) > 0.02);
        // at 1 it starts at once
        assert!(Tilt::reach(deg(10.0), 1.0) > 0.05);
        assert_eq!(Tilt::reach(f32::NAN, 0.5), 0.0);
    }

    #[test]
    fn sliders_map_into_range() {
        let b = Brush::round_pen();
        assert_eq!(b.diameter(0.0), b.min_size);
        assert_eq!(b.diameter(1.0), b.max_size);
        assert!(b.diameter(0.5) < (b.min_size + b.max_size) / 2.0);
        assert_eq!(b.diameter(f32::NAN), b.diameter(0.5));
        assert_eq!(b.opacity(1.0), 1.0);
        let (s, o) = b.pressure_factors(1.0);
        assert_eq!((s, o), (1.0, 1.0));
        let (s0, _) = b.pressure_factors(0.0);
        assert!((s0 - 0.15).abs() < 1e-5);
    }

    #[test]
    fn a_source_image_keeps_as_text() {
        let n = TEXTURE_SIZE * TEXTURE_SIZE;
        let pixels: Vec<u8> = (0..n).map(|i| (i * 7 % 251) as u8).collect();
        let a = Source::new(pixels.clone()).unwrap();
        let back = Source::from_text(&a.to_text()).unwrap();
        assert_eq!(*back.pixels, pixels);
        assert_eq!(back, a);
        // another image is another source; the wrong size or text is none
        assert_ne!(Source::new(vec![0; n]).unwrap(), a);
        assert!(Source::new(vec![0; 10]).is_none());
        assert!(Source::from_text("not an image!").is_none());
    }

    #[test]
    fn notches_are_saved_per_tool_and_slider_and_found_nearby() {
        let mut n = Notches::default();
        assert!(n.add(0, true, 0.3));
        // already there (within reach), or not a slider position
        assert!(!n.add(0, true, 0.32));
        assert!(!n.add(0, true, 1.5));
        assert!(!n.add(0, true, f32::NAN));
        // another tool's or the other slider's are their own
        assert!(n.add(2, true, 0.3));
        assert!(n.add(0, false, 0.3));
        assert_eq!(n.list(0, true), &[0.3]);
        assert_eq!(n.list(1, true), &[] as &[f32]);
        assert_eq!(n.list(9, true), &[] as &[f32]);
        // the nearest within reach
        assert!(n.add(0, true, 0.37));
        assert_eq!(n.at(0, true, 0.335), Some(0.3));
        assert_eq!(n.at(0, true, 0.345), Some(0.37));
        assert_eq!(n.at(0, true, 0.5), None);
        // full at four
        assert!(n.add(0, true, 0.6));
        assert!(n.add(0, true, 0.9));
        assert!(!n.add(0, true, 0.1));
        assert!(n.remove(0, true, 0.61));
        assert!(!n.remove(0, true, 0.61));
        assert_eq!(n.list(0, true), &[0.3, 0.37, 0.9]);
        assert_eq!(Notches::tool(true, false), 0);
        assert_eq!(Notches::tool(false, true), 1);
        assert_eq!(Notches::tool(false, false), 2);
    }
}
