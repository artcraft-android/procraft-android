//! From input samples to stamps: stabilisation and streamline smoothing, pressure, tilt and speed
//! response, spacing, the shape's placement (rotation, scatter, count, jitter, roundness) and
//! taper.

use crate::brush::{
    Brush, CLASSIC_TAPER_SPAN, FLOW_POWER, PressureCurve, Rendering, TAPER_TIME_MS, Taper, Tilt, run_out,
};
use crate::curve::Curve;
use crate::geom::{IRect, Vec2};
use crate::input::{Pointer, Sample};
use crate::studio::{SPACING_FLOOR, SPACING_SPAN};

/// One dab of the brush tip, in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stamp {
    pub center: Vec2,
    /// Half the stamp's width.
    pub radius: f32,
    /// 0..1, before the stroke's opacity.
    pub alpha: f32,
    /// Rotation, radians.
    pub angle: f32,
    /// 1 = as drawn; lower squashes the stamp across its rotation.
    pub roundness: f32,
    /// Where a moving grain is read from: the stamp's middle in the grain (canvas pixels), each
    /// point of the stamp read that far from it as on the canvas (the grain never turns or
    /// squashes with the stamp). A rolling grain is read in place, so it stays put as the stamps
    /// move over it; a static one moves along with each stamp.
    pub grain_offset: Vec2,
    /// How hard the pen presses the paint into a grain (the pressure, 0..1), and how much that
    /// counts (the brush's pressure bleed, 0..1): pressed harder, the paint takes on more of the
    /// stamp and grain, its strongest parts first.
    pub grain_press: f32,
    pub grain_bleed: f32,
    /// The shape mirrored across its own vertical and horizontal axes.
    pub flip: [bool; 2],
    /// How its colour moves from the brush's.
    pub tint: Tint,
    /// How much of the secondary colour it paints with, 0..1.
    pub secondary: f32,
    /// A moving grain's depth and tile size here, as multiples of the brush's.
    pub grain_depth: f32,
    pub grain_size: f32,
    /// How far it fades across its width from its leading edge (a leaning pen's shading), 0..1.
    pub gradation: f32,
    /// How far a taper has thinned it (its size multiplier, 1 = not at all): a round tip's rim
    /// thins with it, so a tapered stamp is drawn as the whole stamp scaled down.
    pub taper: f32,
    /// What light pressure lets through (the brush's opacity transfer), 0..1: for a capped
    /// rendering the most the stamp builds up to, else already in its alpha.
    pub transfer: f32,
    /// How much of the brush's wet-mix attack this stamp has: the pressure (as opacity follows
    /// it), as far as the attack's tilt switch lets it, 0..1.
    pub attack: f32,
    /// The wet mix as this stamp's pressure scales it, by the brush's own pressure curves: its
    /// grade, blur, blur jitter, wetness jitter and charge (1 each without a curve), its
    /// dilution, wet pull and smudge pull (the pressure as it is without one).
    pub wet: [f32; 8],
}

impl Stamp {
    /// A plain round stamp.
    pub fn round(center: Vec2, radius: f32, alpha: f32) -> Stamp {
        Stamp {
            center,
            radius,
            alpha,
            angle: 0.0,
            roundness: 1.0,
            grain_offset: Vec2::ZERO,
            grain_press: 1.0,
            grain_bleed: 0.0,
            flip: [false; 2],
            tint: Tint::NONE,
            secondary: 0.0,
            grain_depth: 1.0,
            grain_size: 1.0,
            gradation: 0.0,
            taper: 1.0,
            transfer: 1.0,
            attack: 1.0,
            wet: [1.0; 8],
        }
    }

    /// Pixels the stamp can touch: its square shape at any rotation.
    pub fn bounds(&self) -> IRect {
        IRect::around(self.center, self.radius * self.roundness.clamp(1.0, MOST_ROUNDNESS) * std::f32::consts::SQRT_2)
    }
}

/// How a stamp's colour moves from the brush's, in hue, saturation and brightness: the hue
/// turned by `hue` (turns), the saturation and the brightness each taken to `x keep + add`
/// (`[keep, add]`), then held to 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    pub hue: f32,
    pub saturation: [f32; 2],
    pub brightness: [f32; 2],
}

impl Tint {
    pub const NONE: Tint = Tint { hue: 0.0, saturation: [1.0, 0.0], brightness: [1.0, 0.0] };

    /// This and `other` together: what each moves the colour by, added.
    pub fn plus(self, other: Tint) -> Tint {
        let [sk, sa] = self.saturation;
        let [ok, oa] = other.saturation;
        let [bk, ba] = self.brightness;
        let [pk, pa] = other.brightness;
        Tint { hue: self.hue + other.hue, saturation: [sk + ok - 1.0, sa + oa], brightness: [bk + pk - 1.0, ba + pa] }
    }

    /// The hue, saturation and brightness moved on by as much again, whatever the colour.
    fn shifted(self, [hue, saturation, brightness]: [f32; 3]) -> Tint {
        let [sk, sa] = self.saturation;
        let [bk, ba] = self.brightness;
        Tint { hue: self.hue + hue, saturation: [sk, sa + saturation], brightness: [bk, ba + brightness] }
    }
}

/// A stamp before taper: where it sits on the path, and its size and alpha from the input.
#[derive(Clone, Copy, Debug)]
struct Placed {
    center: Vec2,
    distance: f32,
    diameter: f32,
    alpha: f32,
    angle: f32,
    roundness: f32,
    flip: [bool; 2],
    tint: Tint,
    secondary: f32,
    grain_depth: f32,
    grain_size: f32,
    grain_press: f32,
    /// How far its moving grain rolls along with the stroke (see [`Extras::grain_movement`]).
    grain_movement: f32,
    gradation: f32,
    transfer: f32,
    attack: f32,
    wet: [f32; 8],
    /// The pressure as the size, opacity, bleed and roundness took it (and the stamp's own).
    pressed: [f32; 5],
    /// How much of the stamp's roundness is the drawn shape's, which the pressure squeezes (the
    /// rest is what leaning adds).
    drawn_share: f32,
}

#[derive(Clone, Copy, Debug)]
struct PathPoint {
    pos: Vec2,
    pressure: f32,
    /// The pressure as size, opacity, bleed, roundness and the stamp's other controls each
    /// follow it (see `soften`).
    soft: [f32; 5],
    tilt: f32,
    azimuth: f32,
    /// How fast the input moves, 0..1 of [`FAST`].
    speed: f32,
}

impl PathPoint {
    fn lerp(self, to: PathPoint, t: f32) -> PathPoint {
        PathPoint {
            pos: self.pos.lerp(to.pos, t),
            pressure: self.pressure + (to.pressure - self.pressure) * t,
            soft: [0, 1, 2, 3, 4].map(|k| {
                let (a, b) = (self.soft.get(k).copied().unwrap_or(1.0), to.soft.get(k).copied().unwrap_or(1.0));
                a + (b - a) * t
            }),
            tilt: self.tilt + (to.tilt - self.tilt) * t,
            azimuth: to.azimuth,
            speed: self.speed + (to.speed - self.speed) * t,
        }
    }
}

/// Speed for the speed dynamics, from how far the pen moves on screen in a frame of a 60 Hz
/// display (points): none up to `SLOW`, then rising (as the square root) to full `SPAN` further
/// on. Both scale with the input's speed distance.
const SLOW: f32 = 25.0;
const SPAN: f32 = 100.0;
/// How many frames the speed is smoothed over.
const SPEED_FRAMES: f32 = 3.0;
/// The frames between two samples with the same time: those of a pen sampled 240 times a second.
const SAMPLE_FRAMES: f32 = 0.25;
/// Stabilisation averages up to this many of the latest input points.
const MOST_AVERAGED: usize = 32;
/// At full motion filtering, how little the filtered pen moves toward each new point; and at full
/// expression, how much of the way back to the pen it comes.
const MOTION_FILTER_MOST: f32 = 0.9;
const EXPRESSION_MOST: f32 = 0.5;
/// Brush files keep the spacing jitter and the speed's spacing at ten times the studio's
/// controls: the share of a gap they widen it by.
const SPACING_FILE_SCALE: f32 = 10.0;

/// The most a stamp's height may be of its width (a roundness jitter can make it taller than
/// wide).
pub const MOST_ROUNDNESS: f32 = 20.0;

/// The lateral and linear jitter as brush files keep them: the control to the power 2.2, times
/// this; each moves a stamp up to that many times half its diagonal (plus a pixel).
const JITTER_FILE_SCALE: f32 = 3.0;

/// The brush's controls the builder reads beyond its own fields (see `studio`), read once.
#[derive(Clone, Copy, Debug, Default)]
struct Extras {
    spacing_jitter: f32,
    jitter_lateral: f32,
    jitter_linear: f32,
    falloff: f32,
    count_jitter: f32,
    pressure_roundness: f32,
    tilt_roundness: f32,
    /// The roundness jitter along the stamp's height and across its width.
    roundness_jitter: [f32; 2],
    speed_size: f32,
    speed_opacity: f32,
    speed_spacing: f32,
    stabilization: f32,
    flip_x: bool,
    flip_y: bool,
    /// Colour jitter per stamp and per stroke (hue, saturation, lightness, darkness), and the
    /// colour's response to pressure and tilt (hue, saturation, brightness, −1..1).
    stamp_color: [f32; 4],
    stroke_color: [f32; 4],
    pressure_color: [f32; 3],
    tilt_color: [f32; 3],
    /// How far a moving grain rolls along with the stroke, 0 (static) to 1 (rolling), and whether
    /// each stroke starts somewhere else in the grain.
    grain_movement: f32,
    grain_offset_jitter: bool,
    /// Light pressure lowering (or, negative, raising) each stamp's flow, −1..1.
    pressure_flow: f32,
    /// How slowly size, opacity, bleed and roundness follow the pressure when the pen moves fast.
    pressure_response: [f32; 5],
    /// Light pressure, and leaning, letting more of a moving grain through (bleed), 0..1.
    pressure_bleed: f32,
    tilt_bleed: f32,
    /// A moving grain's depth varied from stamp to stamp, and the least it goes to, 0..1.
    grain_depth_jitter: f32,
    grain_depth_min: f32,
    /// A moving grain sized with the stamp (0) or fixed (1, cropped).
    grain_zoom: f32,
    /// How blunt the pen's and the finger's taper ends are, 0 (sharp) to 1.
    pen_taper_tip: f32,
    touch_taper_tip: f32,
    /// How far a pen's pressure fades into its tapers, 0..1.
    taper_pressure: f32,
    /// A pen's stroke shows its end tapered as it is drawn.
    tip_animation: bool,
    /// The secondary colour mixed in per stamp and per stroke (jitter, 0..1), and by light
    /// pressure and by leaning (−1..1).
    stamp_secondary: f32,
    stroke_secondary: f32,
    pressure_secondary: f32,
    tilt_secondary: f32,
    /// How a leaning pen shades the stamp across its width, 0..1.
    tilt_gradation: f32,
    /// How much a lean's growth depends on the stroke running across the lean (none along it),
    /// 0..1.
    tilt_compression: f32,
    /// The stamp's roundness upright and at the most lean (the roundness tilt), 0..1.
    tilt_roundness_max: f32,
    /// Where each tilt control starts (see [`Tilt::reach`]).
    tilt_angles: TiltAngles,
    /// The tilt switches: leaning (from each one's angle) scales the lateral and linear jitter
    /// and the shape count, pulls the grain depth down to its least, and drives the stamp colour
    /// jitter (hue, saturation, lightness, darkness, secondary) in place of the stamp's own.
    tilt_switches: TiltSwitches,
    /// The brush's own motion filtering and expression, with the app's.
    motion_filtering: f32,
    motion_expression: f32,
    /// The gap per unit of stamp size: the older spacing (version 0) puts stamps 1/√2 as far
    /// apart as the newer.
    spacing_stride: f32,
}

/// Each tilt switch's angle (its dial's share of a quarter turn), none where the switch is off.
#[derive(Clone, Copy, Debug, Default)]
struct TiltSwitches {
    jitter_lateral: Option<f32>,
    jitter_linear: Option<f32>,
    count: Option<f32>,
    grain_depth: Option<f32>,
    /// Hue, saturation, lightness, darkness and secondary.
    color: [Option<f32>; 5],
    /// The wet mix's attack (for a finger, too: as a pen lying flat).
    attack: Option<f32>,
}

/// Each tilt control's angle (its dial's share of a quarter turn).
#[derive(Clone, Copy, Debug, Default)]
struct TiltAngles {
    opacity: f32,
    size: f32,
    gradation: f32,
    bleed: f32,
    roundness: f32,
    /// Hue, saturation and brightness.
    color: [f32; 3],
    secondary: f32,
}

impl Extras {
    fn of(b: &Brush) -> Extras {
        let unit = |k: &str| b.setting(k).clamp(0.0, 1.0);
        let both = |k: &str| b.setting(k).clamp(-1.0, 1.0);
        let switch = |k: &str| (b.setting(k) >= 0.5).then(|| unit(&format!("{k}_angle")));
        Extras {
            spacing_jitter: unit("spacing_jitter"),
            jitter_lateral: unit("jitter_lateral"),
            jitter_linear: unit("jitter_linear"),
            falloff: unit("falloff"),
            count_jitter: unit("count_jitter"),
            pressure_roundness: both("pressure_roundness"),
            tilt_roundness: both("tilt_roundness"),
            roundness_jitter: [unit("roundness_jitter_y"), unit("roundness_jitter_x")],
            speed_size: both("speed_size"),
            speed_opacity: both("speed_opacity"),
            speed_spacing: unit("speed_spacing"),
            stabilization: unit("stabilization"),
            flip_x: unit("flip_x") >= 0.5,
            flip_y: unit("flip_y") >= 0.5,
            stamp_color: ["stamp_hue", "stamp_saturation", "stamp_lightness", "stamp_darkness"].map(unit),
            stroke_color: ["stroke_hue", "stroke_saturation", "stroke_lightness", "stroke_darkness"].map(unit),
            pressure_color: ["pressure_hue", "pressure_saturation", "pressure_brightness"].map(both),
            tilt_color: ["tilt_hue", "tilt_saturation", "tilt_brightness"].map(both),
            grain_movement: unit("grain_movement"),
            grain_offset_jitter: b.setting("grain_offset_jitter") >= 0.5,
            pressure_flow: both("pressure_flow"),
            // the stamp's own pressure follows as the roundness does
            pressure_response: [
                "pressure_size_response",
                "pressure_opacity_response",
                "pressure_bleed_response",
                "pressure_roundness_response",
                "pressure_roundness_response",
            ]
            .map(unit),
            // the control's place to the power 2.2
            pressure_bleed: unit("pressure_bleed").powf(2.2),
            tilt_bleed: unit("tilt_bleed").powf(2.2),
            grain_depth_jitter: unit("grain_depth_jitter"),
            grain_depth_min: unit("grain_depth_min"),
            grain_zoom: unit("grain_zoom"),
            pen_taper_tip: unit("pen_taper_tip"),
            touch_taper_tip: unit("touch_taper_tip"),
            taper_pressure: unit("pen_taper_pressure"),
            tip_animation: b.setting("pen_tip_animation") >= 0.5,
            stamp_secondary: unit("stamp_secondary"),
            stroke_secondary: unit("stroke_secondary"),
            pressure_secondary: both("pressure_secondary"),
            tilt_secondary: both("tilt_secondary"),
            tilt_gradation: unit("tilt_gradation"),
            tilt_compression: unit("tilt_size_compression"),
            tilt_roundness_max: unit("tilt_roundness_max"),
            tilt_angles: TiltAngles {
                opacity: unit("opacity_tilt_angle"),
                size: unit("size_tilt_angle"),
                gradation: unit("gradation_tilt_angle"),
                bleed: unit("bleed_tilt_angle"),
                roundness: unit("roundness_tilt_angle"),
                color: ["hue_tilt_angle", "saturation_tilt_angle", "brightness_tilt_angle"].map(unit),
                secondary: unit("secondary_tilt_angle"),
            },
            tilt_switches: TiltSwitches {
                jitter_lateral: switch("jitter_lateral_tilt"),
                jitter_linear: switch("jitter_linear_tilt"),
                count: switch("count_tilt"),
                grain_depth: switch("grain_depth_tilt"),
                color: [
                    "stamp_hue_tilt",
                    "stamp_saturation_tilt",
                    "stamp_lightness_tilt",
                    "stamp_darkness_tilt",
                    "stamp_secondary_tilt",
                ]
                .map(switch),
                attack: switch("attack_tilt"),
            },
            motion_filtering: unit("motion_filtering"),
            motion_expression: unit("motion_expression"),
            spacing_stride: if b.setting("spacing_version") < 0.5 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 },
        }
    }
}

/// Streamline is tuned for input at this interval; other rates are scaled to match.
const REFERENCE_INTERVAL_MS: f64 = 1000.0 / 120.0;
/// When a classic taper ends, the stroke runs on past the last point by this many steps (touch,
/// pen), each this much of the one before and turning this much more than it, and the end taper
/// comes to its tip over them.
const RUN_ON_TOUCH: usize = 4;
const RUN_ON_PEN: usize = 1;
const RUN_ON_SHRINK: f32 = 0.66;
const RUN_ON_TURN: f32 = 1.125;
/// How far (canvas pixels) offset jitter may move a stroke's start in a moving grain.
const GRAIN_JITTER_SPAN: f32 = 1024.0;
/// Stamps a stroke may hold (a guard against runaway input, far above any real stroke).
const MAX_STAMPS: usize = 400_000;

/// A small deterministic random source (xorshift), so a stroke replays identically.
#[derive(Clone, Debug)]
struct Random(u32);

impl Random {
    /// 0..1.
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Builds one stroke. Feed it samples in canvas pixels as they arrive ([`StrokeBuilder::add`]
/// returns the stamps to draw now), then [`StrokeBuilder::finish`] for the final stamps, which
/// include the end taper and the catch-up to where the pen lifted.
#[derive(Clone, Debug)]
pub struct StrokeBuilder {
    brush: Brush,
    diameter: f32,
    pointer: Pointer,
    pressure: Option<f32>,
    smoothed: Option<PathPoint>,
    last_sample: Option<Sample>,
    head: Option<PathPoint>,
    direction: f32,
    /// The length of the last stretch of path walked (canvas pixels), none before the first.
    last_seg: f32,
    /// How much the brush is scaled on this canvas (the size the stroke was asked for over the
    /// brush's own), which leaning's growth takes too.
    scale: f32,
    carry: f32,
    length: f32,
    placed: Vec<Placed>,
    bounds: IRect,
    random: Random,
    extras: Extras,
    /// The latest input points, for stabilisation; the speed, smoothed; the next gap's jitter.
    recent: Vec<Vec2>,
    speed: f32,
    /// Whether the speed has been measured (the first move is taken as it is).
    speed_known: bool,
    /// The pressure as each use follows it, at the last sample; and how many samples so far.
    soft: Option<[f32; 5]>,
    segments: u32,
    /// A sample stroke drawn for show: no speed, and the pressure taken as it comes.
    steady: bool,
    /// The samples are the path itself, pressed as they say: nothing smooths either.
    exact: bool,
    /// Drawn on the studio's pad, and whether the brush previews tilt there.
    pad: Option<bool>,
    /// A gap drawn that the last input segment ended before reaching: the next one goes on
    /// with it.
    pending_gap: Option<f32>,
    /// The whole stroke's colour shift, drawn once.
    stroke_shift: Tint,
    /// The stroke's share of the secondary colour, picked as it starts.
    stroke_secondary: f32,
    /// The random turn every stamp of the stroke takes (a randomised shape), picked as it starts.
    stroke_turn: f32,
    /// How far the canvas is turned on screen, for a brush whose shapes keep to the screen.
    screen_turn: f32,
    /// Screen points a canvas pixel spans, for the speed dynamics.
    points_per_pixel: f32,
    input: InputSettings,
    /// Where the motion filter has the pen.
    filtered: Option<Vec2>,
    /// Where in a moving grain the stroke starts.
    grain_start: Vec2,
    /// When each sample came (milliseconds, always later than the one before) and how far the
    /// input had gone by then (canvas pixels, as it came), for the tapers that last a time.
    history: Vec<(f64, f32)>,
    input_length: f32,
    /// The two input points before the last, for running a classic taper's end on past it.
    before_last: [Option<Vec2>; 2],
    /// How far a finished stroke ran on past its last point (see [`StrokeBuilder::run_on`]).
    ran_on: Option<f32>,
    /// How many stamps have been drawn for good, for a stroke shown with its tip.
    kept: usize,
}

/// Smoothing and pressure set for every stroke, whatever the brush, on top of the brush's own.
#[derive(Clone, Debug, PartialEq)]
pub struct InputSettings {
    /// The average of the latest points, 0..1 (as the brush's stabilisation).
    pub stabilization: f32,
    /// Shaky movement filtered out, 0..1, and how much of the hand's own motion comes back, 0..1.
    pub motion_filtering: f32,
    pub motion_expression: f32,
    /// Pressure changes smoothed, 0..1.
    pub pressure_smoothing: f32,
    /// What pen pressure becomes before the brush's own curve.
    pub pressure_curve: Curve,
    /// How far input must move for the speed dynamics to count it as fast, as a part of the
    /// usual distance (0.25..1): less makes slower movement fast.
    pub speed_distance: f32,
}

impl Default for InputSettings {
    fn default() -> Self {
        InputSettings {
            stabilization: 0.0,
            motion_filtering: 0.0,
            motion_expression: 0.0,
            pressure_smoothing: 0.0,
            pressure_curve: Curve::linear(),
            speed_distance: 1.0,
        }
    }
}

/// Two amounts, 0..1, together: each takes away what the other leaves.
fn together(a: f32, b: f32) -> f32 {
    1.0 - (1.0 - a.clamp(0.0, 1.0)) * (1.0 - b.clamp(0.0, 1.0))
}

impl StrokeBuilder {
    /// The stroke with the app's own smoothing and pressure as well as the brush's.
    pub fn with_input(mut self, input: InputSettings) -> StrokeBuilder {
        self.extras.stabilization = together(self.extras.stabilization, input.stabilization);
        self.input = input;
        self
    }

    /// The stroke drawn on a canvas turned `rotation` radians on screen: a brush oriented to the
    /// screen turns its shapes back by as much, so they keep their angle on screen.
    pub fn on_view_turned(mut self, rotation: f32) -> StrokeBuilder {
        if self.brush.setting("orient_to_screen") >= 0.5 && rotation.is_finite() {
            self.screen_turn = rotation;
        }
        self
    }

    /// The stroke drawn with the canvas shown at `points_per_pixel` screen points a canvas pixel:
    /// the speed dynamics go by how fast the pen moves on screen.
    pub fn seen_at(mut self, points_per_pixel: f32) -> StrokeBuilder {
        if points_per_pixel.is_finite() && points_per_pixel > 0.0 {
            self.points_per_pixel = points_per_pixel;
        }
        self
    }

    /// The stroke drawn for show (the library's sample): it never counts as fast, and each use of
    /// the pressure takes it as it comes.
    pub fn steady(mut self) -> StrokeBuilder {
        self.steady = true;
        self
    }

    /// The samples are the path to draw, exactly (the drawing pad's sample scribble): no
    /// streamline, stabilisation or motion filtering moves it, and each use of the pressure takes
    /// it as it comes.
    /// The brush scaled `scale` times on this canvas (as its diameter already is), for what
    /// leaning adds to it.
    pub fn scaled(mut self, scale: f32) -> StrokeBuilder {
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
        }
        self
    }

    /// The stroke is drawn on the brush studio's pad: the pen's lean never moves its stamps
    /// toward the cap there, and the tilt switches act only when the brush previews tilt.
    pub fn on_pad(mut self, preview_tilt: bool) -> StrokeBuilder {
        self.pad = Some(preview_tilt);
        self
    }

    pub fn exact_path(mut self) -> StrokeBuilder {
        self.exact = true;
        self
    }

    /// A stroke with `brush` at `diameter` canvas pixels (the size slider's value).
    pub fn new(brush: Brush, diameter: f32, pointer: Pointer) -> StrokeBuilder {
        StrokeBuilder::with_seed(brush, diameter, pointer, 0x9e37_79b9)
    }

    /// As [`StrokeBuilder::new`], with the seed for scatter, jitter and random rotation.
    pub fn with_seed(brush: Brush, diameter: f32, pointer: Pointer, seed: u32) -> StrokeBuilder {
        let diameter = if diameter.is_finite() { diameter.max(0.5) } else { 1.0 };
        StrokeBuilder {
            brush,
            diameter,
            pointer,
            pressure: None,
            smoothed: None,
            last_sample: None,
            head: None,
            direction: 0.0,
            last_seg: 0.0,
            scale: 1.0,
            carry: 0.0,
            length: 0.0,
            placed: Vec::new(),
            bounds: IRect::EMPTY,
            random: Random(seed | 1),
            extras: Extras::default(),
            recent: Vec::new(),
            speed: 0.0,
            speed_known: false,
            soft: None,
            segments: 0,
            steady: false,
            exact: false,
            pad: None,
            pending_gap: None,
            stroke_shift: Tint::NONE,
            stroke_secondary: 0.0,
            stroke_turn: 0.0,
            screen_turn: 0.0,
            points_per_pixel: 1.0,
            input: InputSettings::default(),
            filtered: None,
            grain_start: Vec2::ZERO,
            history: Vec::new(),
            input_length: 0.0,
            before_last: [None; 2],
            ran_on: None,
            kept: 0,
        }
        .reading_extras()
    }

    fn reading_extras(mut self) -> StrokeBuilder {
        self.extras = Extras::of(&self.brush);
        let jitter = self.extras.stroke_color;
        self.stroke_shift = self.color_jitter(jitter);
        if self.extras.stroke_secondary > 0.0 {
            self.stroke_secondary = self.extras.stroke_secondary * self.random.next();
        }
        if self.extras.grain_offset_jitter {
            self.grain_start = Vec2::new(self.random.next(), self.random.next()) * GRAIN_JITTER_SPAN;
        }
        if self.brush.shape.randomise {
            self.stroke_turn = self.random.next() * std::f32::consts::TAU;
        }
        self
    }

    /// A random colour change by `jitter` (hue, saturation, lightness, darkness): the hue turned
    /// either way, the saturation drawn toward a random one, the colour lightened by a share of
    /// what is left to white and darkened by a share of what is left to black.
    fn color_jitter(&mut self, [hue, saturation, lightness, darkness]: [f32; 4]) -> Tint {
        if hue == 0.0 && saturation == 0.0 && lightness == 0.0 && darkness == 0.0 {
            return Tint::NONE;
        }
        let turn = (self.random.next() - 0.5) * hue;
        let toward = self.random.next() * saturation;
        let (lighter, darker) = (self.random.next() * lightness, self.random.next() * darkness);
        Tint { hue: turn, saturation: [1.0 - saturation, toward], brightness: [1.0 - lighter - darker, lighter] }
    }

    pub fn pointer(&self) -> Pointer {
        self.pointer
    }

    /// The stamp diameter at full pressure (the size slider's value), canvas pixels.
    pub fn diameter(&self) -> f32 {
        self.diameter
    }

    pub fn brush(&self) -> &Brush {
        &self.brush
    }

    /// Pixels the stroke covers so far.
    pub fn bounds(&self) -> IRect {
        self.bounds
    }

    /// Path length so far, canvas pixels.
    pub fn length(&self) -> f32 {
        self.length
    }

    pub fn stamp_count(&self) -> usize {
        self.placed.len()
    }

    /// Adds samples (canvas pixels) and returns the stamps they add. Non-finite samples are skipped.
    pub fn add(&mut self, samples: &[Sample]) -> Vec<Stamp> {
        let mut out = Vec::new();
        for s in samples.iter().filter(|s| s.is_finite()) {
            self.push(*s, &mut out);
        }
        out
    }

    /// The stamps `samples` would add, without adding them (for drawing predicted input).
    pub fn preview(&self, samples: &[Sample]) -> Vec<Stamp> {
        self.clone().add(samples)
    }

    /// Whether the stroke shows its end tapered as it is drawn: a pen's, with a taper that lasts a
    /// time and the tip animation on.
    pub fn shows_tip(&self) -> bool {
        self.pointer.has_pressure() && self.extras.tip_animation && !self.classic() && self.taper().end > 0.0
    }

    /// Adds samples to a stroke shown with its tip (see [`StrokeBuilder::shows_tip`]): returns
    /// the stamps to draw for good now, those the end taper no longer reaches, and the tip, the
    /// stamps after them as they would be drawn were the stroke to end here.
    pub fn add_tipped(&mut self, samples: &[Sample]) -> (Vec<Stamp>, Vec<Stamp>) {
        self.add(samples);
        let start = self.start_span();
        let reach = self.length - self.end_span(0.0);
        let to = self.placed.iter().position(|p| p.distance >= reach).unwrap_or(self.placed.len()).max(self.kept);
        let kept = self.placed.get(self.kept..to).unwrap_or_default();
        let kept = kept.iter().map(|p| self.finish_stamp(p, start, None)).collect();
        self.kept = to;
        (kept, self.tip_from(to))
    }

    /// The tip as it would be with `samples` added (for drawing predicted input), the stroke
    /// left as it is.
    pub fn preview_tip(&self, samples: &[Sample]) -> Vec<Stamp> {
        let mut ahead = self.clone();
        ahead.add(samples);
        ahead.tip_from(self.kept)
    }

    /// The stamps from `from` on as they would be drawn were the stroke to end here.
    fn tip_from(&self, from: usize) -> Vec<Stamp> {
        let (start, end, length) = (self.start_span(), self.end_span(0.0), self.length);
        let tip = self.placed.get(from..).unwrap_or_default();
        tip.iter().map(|p| self.finish_stamp(p, start, Some((length, end)))).collect()
    }

    /// Every stamp so far as currently drawn (no end taper yet).
    pub fn stamps(&self) -> Vec<Stamp> {
        let start = self.start_span();
        self.placed.iter().map(|p| self.finish_stamp(p, start, None)).collect()
    }

    /// Ends the stroke: walks on to where the pen lifted and applies the end taper. Returns all of
    /// the stroke's stamps, as they should finally be drawn.
    pub fn finish(&mut self) -> Vec<Stamp> {
        let mut run_on = 0.0;
        if let (Some(last), Some(head)) = (self.last_sample, self.head) {
            let to = PathPoint {
                pos: last.pos,
                pressure: self.pressure.unwrap_or(1.0),
                soft: self.soft.unwrap_or(head.soft),
                tilt: head.tilt,
                azimuth: head.azimuth,
                speed: head.speed,
            };
            let mut ignored = Vec::new();
            self.walk_to(to, &mut ignored);
            run_on = match self.ran_on {
                Some(r) => r,
                None => self.run_on(to, &mut ignored),
            };
        }
        self.ran_on = Some(run_on);
        let length = self.length;
        let (start, end) = (self.start_span(), self.end_span(run_on));
        let stamps: Vec<Stamp> = self.placed.iter().map(|p| self.finish_stamp(p, start, Some((length, end)))).collect();
        self.bounds = stamps.iter().fold(IRect::EMPTY, |b, s| b.union(s.bounds()));
        stamps
    }

    fn push(&mut self, s: Sample, out: &mut Vec<Stamp>) {
        let pen = self.pointer.has_pressure();
        let raw = if pen { self.input.pressure_curve.eval(s.pressure.clamp(0.0, 1.0)).clamp(0.0, 1.0) } else { 1.0 };
        let smoothing = together(self.brush.pressure_smoothing, self.input.pressure_smoothing).min(0.95);
        let pressure = match self.pressure {
            None => raw,
            Some(p) => p + (raw - p) * (1.0 - smoothing),
        };
        self.pressure = Some(pressure);
        let (tilt, azimuth) = if pen { (s.tilt, s.azimuth) } else { (0.0, 0.0) };
        let curved = self.brush.pressures(pressure);
        let soft = match (self.last_sample, self.soft) {
            (Some(last), Some(before)) if !self.steady && !self.exact => {
                let frames = ((s.time_ms - last.time_ms).abs() * 0.06) as f32;
                let frames = if frames > 0.0 { frames } else { SAMPLE_FRAMES };
                self.soften(curved, before, frames, s.pos.distance(last.pos))
            }
            _ => curved,
        };
        self.soft = Some(soft);
        self.segments = self.segments.saturating_add(1);
        if let Some(last) = self.last_sample {
            // frames of a 60 Hz display since the last sample
            let frames = ((s.time_ms - last.time_ms).abs() * 0.06) as f32;
            let frames = if frames > 0.0 { frames } else { SAMPLE_FRAMES };
            let moved = s.pos.distance(last.pos) * self.points_per_pixel / frames;
            let scale = self.input.speed_distance.clamp(0.25, 1.0);
            let over = (moved - SLOW * scale) / (SPAN * scale);
            let now = if over > 0.0 && !self.steady { over.sqrt().min(1.0) } else { 0.0 };
            // the first move as it is; later ones smoothed over a few frames
            let held = if self.speed_known { SPEED_FRAMES / frames } else { 0.0 };
            self.speed = ((now + held * self.speed) / (held + 1.0)).clamp(0.0, 1.0);
            self.speed_known = true;
        }
        // motion filtering: the pen followed slowly, so small shakes fall away; the expression
        // gives back part of the hand's own motion
        let mut pos = s.pos;
        let filtering = together(self.extras.motion_filtering, self.input.motion_filtering);
        if filtering > 0.0 && !self.exact {
            let follow = 1.0 - filtering * MOTION_FILTER_MOST;
            let f = self.filtered.map_or(s.pos, |f| f.lerp(s.pos, follow));
            self.filtered = Some(f);
            let expression = together(self.extras.motion_expression, self.input.motion_expression);
            pos = f.lerp(s.pos, expression * EXPRESSION_MOST);
        }
        // stabilisation: the average of the latest points, more of them the stronger it is
        if self.extras.stabilization > 0.0 && !self.exact {
            self.recent.push(pos);
            let keep = 1 + (self.extras.stabilization * (MOST_AVERAGED - 1) as f32) as usize;
            let over = self.recent.len().saturating_sub(keep);
            self.recent.drain(..over);
            let sum = self.recent.iter().fold(Vec2::ZERO, |a, &p| a + p);
            pos = sum * (1.0 / self.recent.len() as f32);
        }
        let target = PathPoint { pos, pressure, soft, tilt, azimuth, speed: self.speed };
        let smoothed = match (self.smoothed, self.last_sample) {
            (Some(prev), Some(last)) if !self.exact => {
                let dt = (s.time_ms - last.time_ms).clamp(0.5, 50.0);
                let keep = (self.brush.streamline.clamp(0.0, 1.0) * 0.92).powf((dt / REFERENCE_INTERVAL_MS) as f32);
                PathPoint { pos: prev.pos.lerp(target.pos, 1.0 - keep), ..target }
            }
            _ => target,
        };
        self.smoothed = Some(smoothed);
        self.keep_time(s);
        if let Some(last) = self.last_sample {
            self.before_last = [self.before_last[1], Some(last.pos)];
        }
        self.last_sample = Some(s);
        self.walk_to(smoothed, out);
    }

    /// Notes how far the input has come with `s`, and when (samples without times of their own
    /// count as coming at the usual rate).
    fn keep_time(&mut self, s: Sample) {
        self.input_length += self.last_sample.map_or(0.0, |l| s.pos.distance(l.pos));
        let at = match self.history.last() {
            Some(&(before, _)) if s.time_ms <= before => before + REFERENCE_INTERVAL_MS,
            _ => s.time_ms,
        };
        self.history.push((at, self.input_length));
    }

    /// The input's length (as it came) put on the path the stamps follow, which smoothing
    /// shortens.
    fn on_path(&self, input: f32) -> f32 {
        if self.input_length > 0.0 { input * (self.length / self.input_length).min(1.0) } else { input }
    }

    fn classic(&self) -> bool {
        self.brush.extra.get("classic_taper").is_some_and(|&v| v >= 0.5)
    }

    /// How far along the path the start taper grows to full (0: none): the classic one over its
    /// length in canvas pixels; the newer one over the input up to the first point past its time.
    fn start_span(&self) -> f32 {
        let taper = self.taper();
        if taper.start <= 0.0 {
            return 0.0;
        }
        if self.classic() {
            return taper.start * CLASSIC_TAPER_SPAN;
        }
        let Some(&(t0, _)) = self.history.first() else { return 0.0 };
        let time = f64::from(taper.start) * TAPER_TIME_MS;
        let input = self.history.iter().find(|(t, _)| t - t0 > time).map_or(self.input_length, |&(_, l)| l);
        self.on_path(input)
    }

    /// How far back from the end the end taper starts (0: none): the classic one over the run-on
    /// (`run_on` canvas pixels) up to its length; the newer one over the input since the last
    /// point before its time.
    fn end_span(&self, run_on: f32) -> f32 {
        let taper = self.taper();
        if taper.end <= 0.0 {
            return 0.0;
        }
        if self.classic() {
            return run_on.min(taper.end * CLASSIC_TAPER_SPAN);
        }
        let Some(&(last, total)) = self.history.last() else { return 0.0 };
        let time = f64::from(taper.end) * TAPER_TIME_MS;
        let first = self.history.iter().position(|(t, _)| *t >= last - time).unwrap_or(0);
        let before = first.checked_sub(1).and_then(|i| self.history.get(i)).map_or(0.0, |&(_, l)| l);
        self.on_path(total - before)
    }

    /// A classic taper's end runs the stroke on past `last` (where the input ended) as it was
    /// going, a few shrinking steps that turn on as it turned; how far it went (canvas pixels).
    fn run_on(&mut self, last: PathPoint, out: &mut Vec<Stamp>) -> f32 {
        let (Some(before), Some(last_input)) = (self.before_last[1], self.last_sample) else { return 0.0 };
        if !self.classic() || self.taper().end <= 0.0 {
            return 0.0;
        }
        let step = last_input.pos - before;
        let mut reach = step.length();
        if reach.is_nan() || reach <= 1e-3 {
            return 0.0;
        }
        let mut heading = step.y.atan2(step.x);
        let mut turn = self.before_last[0].map_or(0.0, |b| {
            let earlier = before - b;
            let d = heading - earlier.y.atan2(earlier.x);
            (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
        });
        let steps = if self.pointer.has_pressure() { RUN_ON_PEN } else { RUN_ON_TOUCH };
        let (start, mut at) = (self.length, last);
        for _ in 0..steps {
            reach *= RUN_ON_SHRINK;
            turn *= RUN_ON_TURN;
            heading += turn;
            at = PathPoint { pos: at.pos + Vec2::new(heading.cos(), heading.sin()) * reach, ..at };
            self.walk_to(at, out);
        }
        self.length - start
    }

    /// Walks the path from its head to `to`, placing stamps at the brush's spacing.
    fn walk_to(&mut self, to: PathPoint, out: &mut Vec<Stamp>) {
        let Some(from) = self.head else {
            // the first point always gets a stamp, so a tap leaves a dot
            self.head = Some(to);
            self.place(to, 0.0, out);
            return;
        };
        let seg = from.pos.distance(to.pos);
        if seg.is_nan() || seg <= 1e-4 {
            self.head = Some(PathPoint { pos: from.pos, ..to });
            return;
        }
        let d = to.pos - from.pos;
        self.direction = d.y.atan2(d.x);
        self.last_seg = seg;
        let mut done = 0.0f32;
        // each input segment goes on from the plain gap where it starts
        let mut last = self.step(from);
        while self.placed.len() < MAX_STAMPS {
            let gap = match self.pending_gap.take() {
                Some(gap) => gap,
                None => {
                    last = self.gap(from.lerp(to, done / seg), last);
                    last
                }
            };
            let need = gap - self.carry;
            if done + need > seg {
                self.carry += seg - done;
                self.pending_gap = Some(gap);
                break;
            }
            done += need.max(0.0);
            self.carry = 0.0;
            self.place(from.lerp(to, done / seg), self.length + done, out);
        }
        self.length += seg;
        self.head = Some(to);
    }

    /// The stamp diameter at a point of the path, before jitter and taper.
    fn diameter_at(&self, p: PathPoint) -> f32 {
        let (pressure, _) = self.brush.factors(p.soft[0], p.soft[1]);
        // faster input thins the stroke (or, the other way, widens it)
        let speed = (1.0 - self.extras.speed_size * p.speed).max(0.05);
        let drawn = self.diameter * pressure * speed;
        // leaning adds to it, up to the most the brush may be
        let most = (self.brush.biggest() * self.scale).max(drawn);
        (drawn + self.tilt_growth(p)).min(most)
    }

    /// How far the pen leans past where a tilt control at `theta` starts (see [`Tilt::reach`]);
    /// nothing for a finger.
    fn leaning(&self, p: PathPoint, theta: f32) -> f32 {
        if self.pointer.has_pressure() { Tilt::reach(p.tilt, theta) } else { 0.0 }
    }

    /// What leaning adds to the stamp's diameter (canvas pixels): the tilt size, as far as the
    /// lean has gone, the more the stroke runs across the lean.
    fn tilt_growth(&self, p: PathPoint) -> f32 {
        let size = self.brush.tilt.size;
        if size == 0.0 {
            return 0.0;
        }
        let reach = self.leaning(p, self.extras.tilt_angles.size);
        (size * reach * self.across_lean(p) * self.brush.size_unit() * self.scale).max(0.0)
    }

    /// How much of a lean's growth the stamp takes: with compression, none while the stroke runs
    /// along the lean, all of it across; without, all of it. A dab on its own takes it all.
    fn across_lean(&self, p: PathPoint) -> f32 {
        let compression = self.extras.tilt_compression;
        if compression <= 0.0 || self.last_seg <= 0.0 {
            return 1.0;
        }
        // the lean's direction on the canvas: the pen's orientation is clockwise from up on the screen
        let lean = p.azimuth - std::f32::consts::FRAC_PI_2 - self.screen_turn;
        let quarters = (self.direction - lean) / std::f32::consts::FRAC_PI_2;
        let across = ((quarters + 1.0).rem_euclid(2.0) - 1.0).abs();
        // a short step between inputs counts for less
        let long = 1.0 - 1.0 / (self.last_seg * self.last_seg);
        ((1.0 - compression) + compression * across * (across + long).max(0.0).sqrt()).clamp(0.0, 1.0)
    }

    /// The plain distance to the next stamp: the spacing's share of the stamp's square, and half
    /// a pixel more (and a little with the spacing).
    fn step(&self, p: PathPoint) -> f32 {
        let spacing = self.brush.spacing.max(0.005);
        let extra = 0.5 + (spacing - SPACING_FLOOR).max(0.0) / (SPACING_SPAN / 2.0);
        (spacing * self.square_at(p) * self.extras.spacing_stride + extra).max(0.25)
    }

    /// The gap to the next stamp at `p`, the one before it `last`: the two gaps' mean, widened
    /// by the spacing jitter (a random share of it, as far as its curve lets it) and by the
    /// input's speed. Gaps only ever widen.
    fn gap(&mut self, p: PathPoint, last: f32) -> f32 {
        let x = self.extras;
        let mut widen = p.speed * x.speed_spacing * SPACING_FILE_SCALE;
        if x.spacing_jitter > 0.0 {
            let by = self.brush.pressure_curves.at(PressureCurve::SpacingJitter, p.soft[4].clamp(0.0, 1.0));
            widen += by * x.spacing_jitter * SPACING_FILE_SCALE * self.random.next();
        }
        ((1.0 + widen.max(0.0)) * (last + self.step(p)) * 0.5).max(0.25)
    }

    /// Each use's pressure followed from `before` towards `now` over `frames` (of a 60 Hz display)
    /// in which the pen moved `moved` canvas pixels: when the pen moves fast, a use with a response
    /// speed lags (to (frames / pixels, at least 0.2) to the power twice its response, each
    /// sample), and every use is evened out with what it was, more so the more often the pen
    /// reports (eased in over the stroke's first frames).
    fn soften(&self, now: [f32; 5], before: [f32; 5], frames: f32, moved: f32) -> [f32; 5] {
        let pace = if moved > 0.0 { (frames / moved).clamp(0.2, 1.0) } else { 1.0 };
        let held = 2.0 / frames.max(1e-3);
        let ramp = (self.segments as f32 / held).clamp(0.0, 1.0) * held;
        [0, 1, 2, 3, 4].map(|k| {
            let (n, b) = (now.get(k).copied().unwrap_or(1.0), before.get(k).copied().unwrap_or(1.0));
            let response = self.extras.pressure_response.get(k).copied().unwrap_or(0.0);
            let followed = b + (n - b) * pace.powf(2.0 * response);
            ((followed + b * ramp) / (ramp + 1.0)).clamp(0.0, 1.0)
        })
    }

    /// The square a stamp's shape fills at a point (a round tip covers only part of it).
    fn square_at(&self, p: PathPoint) -> f32 {
        let round_tip = self.brush.shape.image.is_none() && self.brush.shape.source.is_none();
        self.diameter_at(p) / if round_tip { crate::brushfile::ROUND_TIP_SHARE } else { 1.0 }
    }

    /// The opacity a stamp drawn "over" needs so that a brush that builds up gradually reaches,
    /// after one pass, what it would have with `alpha` (each stamp adding `alpha` of what is left,
    /// squared). `overlap` is how many stamps cover a point in one pass.
    fn gradual(alpha: f32, overlap: f32) -> f32 {
        let n = overlap.clamp(1.0, 512.0);
        let mut built = 0.0f32;
        for _ in 0..n.ceil() as u32 {
            built += alpha * (1.0 - built) * (1.0 - built);
        }
        1.0 - (1.0 - built.min(1.0)).powf(1.0 / n.ceil())
    }

    /// How far a tilt switch at `theta` lets its control act: all of it when the switch is off,
    /// as far as the pen leans when it is on (nothing without a pen's lean, or on the pad unless
    /// the brush previews tilt).
    fn switched(&self, p: PathPoint, theta: Option<f32>) -> f32 {
        match theta {
            None => 1.0,
            Some(_) if self.pad == Some(false) => 0.0,
            Some(theta) => self.leaning(p, theta),
        }
    }

    /// How far the pen's lean moves a stamp toward the cap, as a share of its radius: the tilt
    /// angle's reach over the square root of two (a pen only, never on the pad).
    fn toward_cap(&self, p: PathPoint) -> Vec2 {
        if self.pad.is_some() || !self.pointer.has_pressure() {
            return Vec2::ZERO;
        }
        let reach = self.leaning(p, self.brush.tilt.angle / std::f32::consts::FRAC_PI_2);
        // the ramp's floor (an upright pen) moves nothing
        if reach <= 1.0 / 1024.0 {
            return Vec2::ZERO;
        }
        let k = reach * std::f32::consts::FRAC_1_SQRT_2;
        // the lean's direction on the canvas, as for the size compression
        let lean = p.azimuth - std::f32::consts::FRAC_PI_2 - self.screen_turn;
        Vec2::new(lean.cos(), lean.sin()) * k
    }

    fn place(&mut self, p: PathPoint, distance: f32, out: &mut Vec<Stamp>) {
        let x = self.extras;
        let start_span = self.start_span();
        let sw = x.tilt_switches;
        let diameter = self.diameter_at(p);
        let (_, pressure_alpha) = self.brush.factors(p.soft[0], p.soft[1]);
        // leaning fades the stamp by the square root of how far it has gone
        let fade = self.leaning(p, x.tilt_angles.opacity).sqrt() * self.brush.tilt.opacity;
        let tilt_alpha = if fade >= 0.0 { 1.0 - fade } else { 1.0 - (fade + self.brush.tilt.opacity) };
        let speed_alpha = (1.0 - x.speed_opacity * p.speed).clamp(0.0, 1.0);
        // the fall off fades the stroke out by the cube root of how far it has gone
        let falloff = 1.0 - run_out(x.falloff, distance);
        use PressureCurve as PC;
        // the brush's own pressure curves for its other controls, at the stamp's pressure (a
        // finger's is full): each control scaled by its curve, or left as it is without one
        let pressure = p.soft[4].clamp(0.0, 1.0);
        let curves = &self.brush.pressure_curves;
        let by = |k: PressureCurve| curves.at(k, pressure);
        // the flow lets through as much as its pressure amount and curve say (an amount below 0
        // the other way)
        let flowing = curves.through(PC::Flow, pressure);
        let pressure_flow = if x.pressure_flow >= 0.0 {
            1.0 - x.pressure_flow * (1.0 - flowing)
        } else {
            1.0 + x.pressure_flow * flowing
        }
        .clamp(0.0, 1.0);
        // flow, pressure, tilt and speed together through the opacity range, eased; then what
        // pressure lets through (the brush's pressure flow) and the fall off
        let b = &self.brush;
        let control = b.flow.clamp(0.0, 1.0).powf(1.0 / FLOW_POWER) * pressure_alpha * tilt_alpha * speed_alpha;
        let ranged = (b.min_opacity + (b.max_opacity - b.min_opacity) * control).clamp(0.01, 1.0);
        // a capped stroke builds up towards what the pressure lets through rather than taking it
        let transfer = pressure_flow;
        let through = if self.brush.rendering == Rendering::Capped { 1.0 } else { pressure_flow };
        let mut alpha = ranged.powf(FLOW_POWER) * through * falloff;
        // (wet paint is laid stamp by stamp as it is mixed, not built up gradually)
        if self.brush.rendering == Rendering::Gradual && !self.brush.mixes_wet() {
            alpha = StrokeBuilder::gradual(alpha, diameter / self.step(p));
        }
        let shape = self.brush.shape.clone();
        let [count_by, count_jitter_by, lateral_by, linear_by, size_jitter_by, opacity_jitter_by, scatter_by] = [
            PC::Count,
            PC::CountJitter,
            PC::LateralJitter,
            PC::LinearJitter,
            PC::SizeJitter,
            PC::OpacityJitter,
            PC::Scatter,
        ]
        .map(by);
        let [roundness_y_by, roundness_x_by, movement_by, grain_scale_by, zoom_by, depth_by, depth_jitter_by] = [
            PC::RoundnessJitterY,
            PC::RoundnessJitterX,
            PC::GrainMovement,
            PC::GrainScale,
            PC::GrainZoom,
            PC::GrainDepth,
            PC::GrainDepthJitter,
        ]
        .map(by);
        let along = |k: PressureCurve| curves.through(k, pressure);
        let wet = [
            by(PC::WetGrade),
            by(PC::WetBlur),
            by(PC::WetBlurJitter),
            by(PC::WetnessJitter),
            by(PC::Charge),
            along(PC::Dilution),
            along(PC::WetPull),
            along(PC::SmudgePull),
        ];
        let color_jitter_by =
            [PC::HueJitter, PC::SaturationJitter, PC::LightnessJitter, PC::DarknessJitter, PC::SecondaryJitter];
        let color_has_curve = color_jitter_by.map(|k| curves.get(k).is_some());
        let color_jitter_by = color_jitter_by.map(by);
        // the colour moves with the pressure as its curves say (without one, as far as at full)
        let color_by = [PC::Hue, PC::Saturation, PC::Brightness, PC::Secondary].map(by);
        // as many beyond the first as the count tilt, the count's own curve and its jitter let
        // through, rounded down once
        let leaning = if sw.count.is_some() { self.switched(p, sw.count) } else { 1.0 };
        let beyond = (shape.count.clamp(1, 16) - 1) as f32 * leaning * count_by;
        let beyond = beyond * (1.0 - self.random.next() * x.count_jitter * count_jitter_by);
        let count = 1 + beyond.clamp(0.0, 15.0).floor() as u32;
        let (along, across) = (
            Vec2::new(self.direction.cos(), self.direction.sin()),
            Vec2::new(-self.direction.sin(), self.direction.cos()),
        );
        // leaning adds the same to the stamp's height as to its width, then the roundness tilt
        // takes it from its upright roundness towards its leaning one
        let grown = self.tilt_growth(p);
        let drawn = (diameter - grown).max(1e-3);
        let drawn_share = drawn / (drawn + grown);
        let leant = self.leaning(p, x.tilt_angles.roundness);
        let tilted = x.tilt_roundness_max * (1.0 - leant) + (1.0 - x.tilt_roundness) * leant;
        let base_roundness = self.pressed_roundness(drawn_share, p.soft[3]) * tilted;
        // the jitter reaches half the stamp's diagonal (its square as round as it is here), and a
        // pixel more
        let half = self.square_at(p) * 0.5;
        let jitter_reach = half * (1.0 + base_roundness * base_roundness).sqrt() + 1.0;
        // leaning moves the stamps toward the pen's cap, by up to their radius
        let cap = self.toward_cap(p) * (diameter * 0.5);
        let file = |c: f32| JITTER_FILE_SCALE * c.powf(2.2);
        let jitter_lateral = file(x.jitter_lateral) * self.switched(p, sw.jitter_lateral) * lateral_by;
        let jitter_linear = file(x.jitter_linear) * self.switched(p, sw.jitter_linear) * linear_by;
        // a colour jitter's tilt switch drives it by the lean in place of the pressure, and with a
        // curve of its own the pressure's share comes on top
        let color_tilt = sw.color.map(|theta| self.switched(p, theta));
        let (mut color_first, mut color_more) = (color_jitter_by, [0.0f32; 5]);
        for ((((first, more), tilt), on), has_curve) in
            color_first.iter_mut().zip(color_more.iter_mut()).zip(color_tilt).zip(sw.color).zip(color_has_curve)
        {
            if on.is_some() {
                *more = if has_curve { *first } else { 0.0 };
                *first = tilt;
            }
        }
        // the wet attack follows the pressure, and the lean when its switch is on (a finger as a
        // pen lying flat)
        let attack_tilt = match sw.attack {
            None => 1.0,
            Some(theta) if self.pointer.has_pressure() => Tilt::reach(p.tilt, theta),
            Some(theta) => Tilt::reach(std::f32::consts::FRAC_PI_2, theta),
        };
        let attack = (p.soft[1].clamp(0.0, 1.0) * attack_tilt * curves.through(PC::Attack, pressure)).clamp(0.0, 1.0);
        let [hue_by, saturation_by, lightness_by, darkness_by, secondary_by] = color_first;
        let [hue_more, saturation_more, lightness_more, darkness_more, secondary_more] = color_more;
        for _ in 0..count {
            let size = 1.0 - shape.size_jitter.clamp(0.0, 1.0) * size_jitter_by * self.random.next();
            let fade = 1.0 - shape.opacity_jitter.clamp(0.0, 1.0) * opacity_jitter_by * self.random.next();
            let mut center = p.pos + cap;
            // one random point a stamp: as far as a share of the reach, in a random direction, the
            // reach across and along the stroke each its own
            if x.jitter_lateral > 0.0 || x.jitter_linear > 0.0 {
                let far = self.random.next();
                let towards = (self.random.next() * 2.0 - 1.0) * std::f32::consts::PI;
                center = center
                    + across * (far * towards.sin() * jitter_lateral * jitter_reach)
                    + along * (far * towards.cos() * jitter_linear * jitter_reach);
            }
            if shape.throw > 0.0 {
                let a = self.random.next() * std::f32::consts::TAU;
                let r = self.random.next().sqrt() * shape.throw * diameter;
                center = center + Vec2::new(a.cos() * r, a.sin() * r);
            }
            let mut angle = shape.rotation - self.screen_turn + shape.follow * self.direction;
            if shape.follow_azimuth {
                angle += p.azimuth;
            }
            angle += self.stroke_turn;
            if shape.scatter > 0.0 {
                angle += (self.random.next() * 2.0 - 1.0) * shape.scatter * scatter_by * std::f32::consts::PI;
            }
            // the roundness jitter squeezes the width and the height apart, and the stamp is scaled
            // back so the larger of them stays whole
            let [jitter_y, jitter_x] = x.roundness_jitter;
            let squeezed = |jitter: f32, rng: &mut Random| if jitter > 0.0 { 1.0 - jitter * rng.next() } else { 1.0 };
            let wide = squeezed(jitter_x * roundness_x_by, &mut self.random);
            let tall = squeezed(jitter_y * roundness_y_by, &mut self.random);
            let most = wide.max(tall).max(1e-3);
            let (wide, tall) = ((wide / most).max(0.05), tall / most);
            let roundness = (base_roundness * tall / wide).clamp(0.05, MOST_ROUNDNESS);
            let flip = [x.flip_x && self.random.next() < 0.5, x.flip_y && self.random.next() < 0.5];
            let [h, s, l, d] = x.stamp_color;
            let mut stamp_tint = self.color_jitter([h * hue_by, s * saturation_by, l * lightness_by, d * darkness_by]);
            let more = [h * hue_more, s * saturation_more, l * lightness_more, d * darkness_more];
            if more.iter().any(|&m| m > 0.0) {
                stamp_tint = stamp_tint.plus(self.color_jitter(more));
            }
            // pressure and leaning move the colour on as far as their controls say
            let lean = x.tilt_angles.color.map(|theta| self.leaning(p, theta));
            let moved = [0, 1, 2].map(|k| {
                let by = |a: &[f32; 3]| a.get(k).copied().unwrap_or(0.0);
                by(&x.pressure_color) * color_by.get(k).copied().unwrap_or(1.0) + by(&x.tilt_color) * by(&lean)
            });
            let tint = self.stroke_shift.plus(stamp_tint).shifted(moved);
            // random numbers only for the controls in use, so strokes without them replay as before
            let mut stamp_secondary = 0.0;
            if x.stamp_secondary > 0.0 {
                stamp_secondary = x.stamp_secondary * secondary_by * self.random.next();
                if secondary_more > 0.0 {
                    stamp_secondary += x.stamp_secondary * secondary_more * self.random.next();
                }
            }
            let secondary = (stamp_secondary
                + self.stroke_secondary
                + x.pressure_secondary * color_by[3]
                + x.tilt_secondary * self.leaning(p, x.tilt_angles.secondary))
            .clamp(0.0, 1.0);
            // a moving grain: deeper (or shallower) from stamp to stamp, never under its least;
            // leaning lets more of it through
            let base = self.brush.grain.depth.max(1e-3);
            let jitter = if x.grain_depth_jitter > 0.0 {
                x.grain_depth_jitter * depth_jitter_by * self.random.next()
            } else {
                0.0
            };
            let mut depth = base * (1.0 - jitter) * depth_by;
            depth += (1.0 - depth) * (x.tilt_bleed * self.leaning(p, x.tilt_angles.bleed)).clamp(0.0, 1.0);
            let grain_press = p.soft[2].clamp(0.0, 1.0);
            let mut depth = depth.max(x.grain_depth_min).clamp(0.0, 1.0);
            // the depth tilt: leaning takes the depth down to its least
            if sw.grain_depth.is_some() {
                let least = x.grain_depth_min.min(depth);
                depth = least + (depth - least) * (1.0 - self.switched(p, sw.grain_depth));
            }
            // sized with the stamp, against the stroke's size, unless cropped
            let grain_size = (1.0 + (diameter * size / self.diameter.max(1e-3) - 1.0) * (1.0 - x.grain_zoom * zoom_by))
                * grain_scale_by;
            let placed = Placed {
                center,
                distance,
                diameter: diameter * size * wide,
                alpha: alpha * fade,
                angle,
                roundness,
                flip,
                tint,
                secondary,
                grain_depth: depth / base,
                grain_size: grain_size.max(0.05),
                grain_press,
                grain_movement: x.grain_movement * movement_by,
                gradation: (x.tilt_gradation * self.leaning(p, x.tilt_angles.gradation)).max(0.0).sqrt().min(1.0),
                transfer,
                attack,
                wet,
                pressed: p.soft,
                drawn_share,
            };
            self.placed.push(placed);
            let stamp = self.finish_stamp(&placed, start_span, None);
            self.bounds = self.bounds.union(stamp.bounds());
            out.push(stamp);
        }
    }

    /// Where a stamp's moving grain is read from (see [`Stamp::grain_offset`]): in place on the
    /// canvas as far as the grain rolls, from wherever the stroke starts in it.
    fn grain_offset(&self, p: &Placed) -> Vec2 {
        self.grain_start + p.center * p.grain_movement
    }

    /// The stamp's roundness before leaning and jitter: the shape's, squeezed as light pressure
    /// `pressure` says, on the `drawn_share` of the stamp that isn't grown by leaning.
    fn pressed_roundness(&self, drawn_share: f32, pressure: f32) -> f32 {
        let squeezed = 1.0 - self.extras.pressure_roundness * (1.0 - pressure.clamp(0.0, 1.0));
        drawn_share * self.brush.shape.roundness.clamp(0.05, 1.0) * squeezed + (1.0 - drawn_share)
    }

    fn taper(&self) -> &Taper {
        if self.pointer.has_pressure() { &self.brush.pen_taper } else { &self.brush.touch_taper }
    }

    /// The stamp `p` as drawn, its start taper growing over `start_span` canvas pixels of the
    /// path and, once the stroke has ended, its end taper over the last `end` of its length:
    /// `(length, end)`.
    fn finish_stamp(&self, p: &Placed, start_span: f32, end: Option<(f32, f32)>) -> Stamp {
        let grown = |gone: f32, span: f32| if span > 0.0 { (gone / span).clamp(0.0, 1.0) } else { 1.0 };
        let (start, finish) = match end {
            // a tap keeps its whole size
            Some((length, _)) if length <= 0.0 => (1.0, 1.0),
            Some((length, span)) => (grown(p.distance, start_span), grown(length - p.distance, span)),
            None => (grown(p.distance, start_span), 1.0),
        };
        let tip = if self.pointer.has_pressure() { self.extras.pen_taper_tip } else { self.extras.touch_taper_tip };
        let (mut size, mut alpha) = self.taper().applied(start, finish, tip);
        // a pen's pressure fades into the tapers as far as the brush says: the size, opacity,
        // bleed and roundness it gave scaled to what the faded pressure gives
        let fade = self.extras.taper_pressure;
        let (mut grain_press, mut roundness) = (p.grain_press, p.roundness);
        if self.pointer.has_pressure() && fade > 0.0 {
            let kept = (1.0 - fade) + fade * (start * finish).powf(Taper::tip_power(tip));
            let [s, o, b, r, _] = p.pressed;
            let (s0, o0) = self.brush.factors(s, o);
            let (s1, o1) = self.brush.factors(s * kept, o * kept);
            if s0 > 0.0 {
                size *= s1 / s0;
            }
            if o0 > 0.0 {
                alpha *= (o1 / o0).powf(FLOW_POWER);
            }
            grain_press = (b * kept).clamp(0.0, 1.0);
            let r0 = self.pressed_roundness(p.drawn_share, r);
            if r0 > 0.0 {
                let r1 = self.pressed_roundness(p.drawn_share, r * kept);
                roundness = (roundness * r1 / r0).clamp(0.05, MOST_ROUNDNESS);
            }
        }
        Stamp {
            center: p.center,
            radius: (p.diameter * size * 0.5).max(0.25),
            alpha: (p.alpha * alpha).clamp(0.0, 1.0),
            angle: p.angle,
            roundness,
            grain_offset: self.grain_offset(p),
            grain_press,
            grain_bleed: self.extras.pressure_bleed.clamp(0.0, 1.0),
            flip: p.flip,
            tint: p.tint,
            secondary: p.secondary,
            grain_depth: p.grain_depth,
            grain_size: p.grain_size,
            gradation: p.gradation,
            taper: size.clamp(0.0, 1.0),
            transfer: p.transfer,
            attack: p.attack,
            wet: p.wet,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::{Shape, Taper, Tilt};

    fn plain() -> Brush {
        Brush {
            streamline: 0.0,
            pressure_smoothing: 0.0,
            spacing: 0.1,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            ..Brush::round_pen()
        }
    }

    fn line(n: usize, len: f32) -> Vec<Sample> {
        (0..=n).map(|i| Sample::at(Vec2::new(len * i as f32 / n as f32, 50.0), 1.0, i as f64 * 4.0)).collect()
    }

    #[test]
    fn tap_leaves_one_stamp() {
        let mut b = StrokeBuilder::new(plain(), 10.0, Pointer::Stylus);
        assert_eq!(b.add(&[Sample::at(Vec2::new(3.0, 4.0), 0.5, 0.0)]).len(), 1);
        let all = b.finish();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].center, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn pressure_is_evened_out_unless_the_stroke_is_steady() {
        let b = Brush { pressure_size: 1.0, ..plain() };
        // light, then full pressure halfway along, from a pen reporting 240 times a second
        let samples: Vec<Sample> = (0..40)
            .map(|i| {
                let pressure = if i < 20 { 0.2 } else { 1.0 };
                Sample::at(Vec2::new(i as f32 * 4.0, 50.0), pressure, f64::from(i) * 1000.0 / 240.0)
            })
            .collect();
        let radius_at = |mut s: StrokeBuilder, x: f32| {
            s.add(&samples);
            s.finish().iter().find(|st| st.center.x >= x).map_or(0.0, |st| st.radius)
        };
        // just after the jump the stroke is still on its way up; drawn steady it is there at once
        let soft = radius_at(StrokeBuilder::new(b.clone(), 10.0, Pointer::Stylus), 84.0);
        let steady = radius_at(StrokeBuilder::new(b.clone(), 10.0, Pointer::Stylus).steady(), 84.0);
        assert!(soft < steady * 0.8, "{soft} {steady}");
        assert!((steady - 5.0).abs() < 1e-3);
        // and it gets there
        let late = radius_at(StrokeBuilder::new(b, 10.0, Pointer::Stylus), 150.0);
        assert!(late > 4.5, "{late}");
    }

    #[test]
    fn a_gradual_brush_reaches_in_one_pass_what_its_rule_says() {
        // ten stamps over a point, each adding 0.168 of what is left, squared
        let mut built = 0.0f32;
        for _ in 0..10 {
            built += 0.168 * (1.0 - built) * (1.0 - built);
        }
        let alpha = StrokeBuilder::gradual(0.168, 10.0);
        assert!(alpha < 0.168);
        assert!((1.0 - (1.0 - alpha).powi(10) - built).abs() < 1e-4);
        // one stamp at full opacity covers fully either way
        assert!((StrokeBuilder::gradual(1.0, 10.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn stamps_are_evenly_spaced() {
        let mut b = StrokeBuilder::new(plain(), 10.0, Pointer::Stylus);
        b.add(&line(37, 100.0));
        let all = b.finish();
        // spacing 0.1 of the round tip's square (10 px is 0.797 of it), half a pixel, and a little
        // with the spacing: about 1.8 px, over 100 px, plus the first stamp
        let gap = 0.1 * 10.0 / crate::brushfile::ROUND_TIP_SHARE + 0.5 + (0.1 - SPACING_FLOOR) / (SPACING_SPAN / 2.0);
        assert_eq!(all.len(), (100.0 / gap) as usize + 1);
        for w in all.windows(2) {
            let d = w[0].center.distance(w[1].center);
            assert!((d - gap).abs() < 1e-3, "gap {d}");
        }
        assert!(b.bounds().x0 <= -5 && b.bounds().x1 >= 105);
    }

    #[test]
    fn spacing_carries_across_samples() {
        let mut few = StrokeBuilder::new(plain(), 10.0, Pointer::Stylus);
        few.add(&line(3, 100.0));
        let mut many = StrokeBuilder::new(plain(), 10.0, Pointer::Stylus);
        many.add(&line(97, 100.0));
        assert_eq!(few.finish().len(), many.finish().len());
    }

    #[test]
    fn pressure_scales_stamps_but_not_fingers() {
        let samples = [Sample::at(Vec2::new(0.0, 0.0), 0.0, 0.0)];
        let mut pen = StrokeBuilder::new(plain(), 10.0, Pointer::Stylus);
        let light = pen.add(&samples);
        let mut finger = StrokeBuilder::new(plain(), 10.0, Pointer::Finger);
        let touch = finger.add(&samples);
        assert!(light[0].radius < touch[0].radius);
        assert_eq!(touch[0].radius, 5.0);
    }

    #[test]
    fn streamline_smooths_a_zigzag() {
        let zigzag: Vec<Sample> = (0..200)
            .map(|i| Sample::at(Vec2::new(i as f32, if i % 2 == 0 { 0.0 } else { 10.0 }), 1.0, i as f64 * 8.0))
            .collect();
        let spread = |streamline: f32| {
            let mut b = StrokeBuilder::new(Brush { streamline, ..plain() }, 4.0, Pointer::Stylus);
            b.add(&zigzag);
            let s = b.stamps();
            let ys: Vec<f32> = s[s.len() / 2..].iter().map(|s| s.center.y).collect();
            ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)
        };
        assert!(spread(0.9) < spread(0.0) * 0.5);
    }

    #[test]
    fn an_exact_path_is_not_smoothed() {
        let zigzag: Vec<Sample> = (0..200)
            .map(|i| Sample::at(Vec2::new(i as f32, if i % 2 == 0 { 0.0 } else { 10.0 }), 1.0, i as f64 * 8.0))
            .collect();
        let brush = Brush { streamline: 0.9, ..plain() };
        let mut b = StrokeBuilder::new(brush, 4.0, Pointer::Stylus).exact_path();
        b.add(&zigzag);
        let ys: Vec<f32> = b.stamps().iter().map(|s| s.center.y).collect();
        assert!(ys.iter().any(|&y| y > 9.9) && ys.iter().any(|&y| y < 0.1));
    }

    #[test]
    fn finish_catches_up_and_tapers_the_end() {
        let brush =
            Brush { streamline: 0.9, pen_taper: Taper { start: 0.0, end: 0.5, size: 1.0, opacity: 0.0 }, ..plain() };
        let mut b = StrokeBuilder::new(brush, 4.0, Pointer::Stylus);
        b.add(&line(50, 300.0));
        let live_end = b.stamps().last().unwrap().center;
        let all = b.finish();
        let end = all.last().unwrap();
        assert!(end.center.distance(Vec2::new(300.0, 50.0)) < 1.0);
        assert!(live_end.x < 299.0, "streamline lags while drawing");
        assert!(end.radius < 0.5);
        assert!(all[all.len() / 2].radius > 1.9);
    }

    #[test]
    fn a_touch_stroke_tapers_over_what_it_covers_in_the_taper_s_time() {
        let brush = Brush { touch_taper: Taper { start: 0.5, end: 0.5, size: 1.0, opacity: 0.0 }, ..plain() };
        // 600 px drawn in samples `ms` apart: the end taper covers the input since the last
        // sample before its time (0.5 x 83 ms), so the faster, the longer it is
        let near_end = |ms: f64| {
            let mut b = StrokeBuilder::new(brush.clone(), 20.0, Pointer::Finger);
            let samples: Vec<Sample> =
                (0..=60).map(|i| Sample::at(Vec2::new(i as f32 * 10.0, 50.0), 1.0, i as f64 * ms)).collect();
            b.add(&samples);
            let all = b.finish();
            let at = |x: f32| {
                all.iter().min_by(|a, b| (a.center.x - x).abs().total_cmp(&(b.center.x - x).abs())).unwrap().radius
            };
            (at(600.0 - 30.0), at(300.0))
        };
        let (fast, full) = near_end(8.0);
        let (slow, _) = near_end(32.0);
        assert!(fast < full * 0.8, "{fast} {full}");
        assert!((slow - full).abs() < 1e-3, "{slow} {full}");
    }

    #[test]
    fn a_classic_end_runs_on_past_the_lift_and_tapers_there() {
        let mut brush = Brush { touch_taper: Taper { start: 0.0, end: 1.0, size: 1.0, opacity: 0.0 }, ..plain() };
        brush.pen_taper = brush.touch_taper.clone();
        brush.set_setting("classic_taper", 1.0);
        // 300 px in 10 px steps
        let ran_to = |pointer| {
            let mut b = StrokeBuilder::new(brush.clone(), 10.0, pointer);
            b.add(&line(30, 300.0));
            let all = b.finish();
            let last = all.last().unwrap();
            let at_lift = all.iter().min_by(|a, b| (a.center.x - 300.0).abs().total_cmp(&(b.center.x - 300.0).abs()));
            (last.center.x, last.radius, at_lift.unwrap().radius)
        };
        // a finger runs on four shrinking steps (0.66, 0.44, 0.29, 0.19 of the last), a pen one
        let (touch_end, touch_tip, touch_at_lift) = ran_to(Pointer::Finger);
        assert!((touch_end - (300.0 + 15.73)).abs() < 1.0, "{touch_end}");
        assert!(touch_tip < 0.5 && touch_at_lift > 4.9, "{touch_tip} {touch_at_lift}");
        let (pen_end, pen_tip, _) = ran_to(Pointer::Stylus);
        assert!((pen_end - 306.6).abs() < 1.0, "{pen_end}");
        assert!(pen_tip < 1.0, "{pen_tip}");
    }

    #[test]
    fn taper_pressure_fades_a_pens_pressure_into_its_tapers() {
        // a taper that thins nothing by itself, a size that follows the pressure in full
        let taper = Taper { start: 0.5, end: 0.5, size: 0.0, opacity: 0.0 };
        let mut brush = Brush { pen_taper: taper.clone(), touch_taper: taper, pressure_size: 1.0, ..plain() };
        let stamp = |brush: &Brush, pointer, at: usize| {
            let mut b = StrokeBuilder::new(brush.clone(), 10.0, pointer);
            let samples: Vec<Sample> =
                (0..=100).map(|i| Sample::at(Vec2::new(i as f32 * 2.0, 50.0), 1.0, i as f64 * 4.0)).collect();
            b.add(&samples);
            b.finish().get(at).cloned().unwrap()
        };
        let near_start = |brush: &Brush, pointer| stamp(brush, pointer, 5).radius;
        assert_eq!(near_start(&brush, Pointer::Stylus), 5.0);
        brush.set_setting("pen_taper_pressure", 1.0);
        assert!(near_start(&brush, Pointer::Stylus) < 3.0, "{}", near_start(&brush, Pointer::Stylus));
        // a finger has no pressure to fade
        assert_eq!(near_start(&brush, Pointer::Finger), 5.0);
        // the bleed and the roundness take the faded pressure too, and are whole further in
        brush.set_setting("pressure_bleed", 1.0);
        brush.set_setting("pressure_roundness", 1.0);
        let (tip, inside) = (stamp(&brush, Pointer::Stylus, 5), stamp(&brush, Pointer::Stylus, 40));
        assert!(tip.grain_press < 0.6 && tip.roundness < 0.6, "{} {}", tip.grain_press, tip.roundness);
        assert!(inside.grain_press > 0.99 && inside.roundness > 0.99, "{} {}", inside.grain_press, inside.roundness);
    }

    #[test]
    fn a_pens_tip_shows_tapered_as_it_is_drawn() {
        let taper = Taper { start: 0.0, end: 0.5, size: 1.0, opacity: 0.0 };
        let brush = Brush { pen_taper: taper.clone(), touch_taper: taper, ..plain() };
        // 2 px every 4 ms: the last 42 ms are the last 20 px or so
        let samples: Vec<Sample> =
            (0..=100).map(|i| Sample::at(Vec2::new(i as f32 * 2.0, 50.0), 1.0, i as f64 * 4.0)).collect();
        let mut b = StrokeBuilder::new(brush.clone(), 10.0, Pointer::Stylus);
        assert!(b.shows_tip());
        let (kept, tip) = b.add_tipped(samples.get(..60).unwrap());
        assert!(kept.iter().all(|s| s.radius == 5.0));
        assert!(tip.len() < 30 && tip.last().unwrap().radius < 1.0, "{} {}", tip.len(), tip.last().unwrap().radius);
        assert_eq!(kept.len() + tip.len(), b.stamp_count());
        // drawn on, the old tip is kept whole and a new one shown
        let (more, tip) = b.add_tipped(samples.get(60..).unwrap());
        assert!(more.iter().all(|s| s.radius == 5.0) && more.first().unwrap().center.x < 118.0);
        assert_eq!(kept.len() + more.len() + tip.len(), b.stamp_count());
        // a predicted tip runs on to where the input is predicted to go, the stroke left as it is
        let ahead = b.preview_tip(&[Sample::at(Vec2::new(240.0, 50.0), 1.0, 420.0)]);
        assert!(ahead.last().unwrap().center.x > 230.0 && ahead.last().unwrap().radius < 1.0);
        assert_eq!(kept.len() + more.len() + tip.len(), b.stamp_count());
        // a finger, a classic taper or the animation off show no tip
        assert!(!StrokeBuilder::new(brush.clone(), 10.0, Pointer::Finger).shows_tip());
        let mut classic = brush.clone();
        classic.set_setting("classic_taper", 1.0);
        assert!(!StrokeBuilder::new(classic, 10.0, Pointer::Stylus).shows_tip());
        let mut off = brush;
        off.set_setting("pen_tip_animation", 0.0);
        assert!(!StrokeBuilder::new(off, 10.0, Pointer::Stylus).shows_tip());
    }

    #[test]
    fn a_newer_taper_on_a_pen_lasts_its_time() {
        let brush = Brush { pen_taper: Taper { start: 0.5, end: 0.0, size: 1.0, opacity: 0.0 }, ..plain() };
        // 2 px every 4 ms: half of 83 ms is reached by the eleventh sample, 22 px in
        let mut b = StrokeBuilder::new(brush, 10.0, Pointer::Stylus);
        let samples: Vec<Sample> =
            (0..=100).map(|i| Sample::at(Vec2::new(i as f32 * 2.0, 50.0), 1.0, i as f64 * 4.0)).collect();
        b.add(&samples);
        let all = b.finish();
        let at = |x: f32| all.iter().min_by(|a, b| (a.center.x - x).abs().total_cmp(&(b.center.x - x).abs())).unwrap();
        assert!((at(11.0).radius - 2.5).abs() < 0.3, "{}", at(11.0).radius);
        assert_eq!(at(30.0).radius, 5.0);
        // no end taper set: the end stays whole
        assert_eq!(all.last().unwrap().radius, 5.0);
    }

    #[test]
    fn bad_samples_are_skipped() {
        let mut b = StrokeBuilder::new(plain(), 10.0, Pointer::Stylus);
        assert!(b.add(&[Sample::at(Vec2::new(f32::NAN, 0.0), 1.0, 0.0)]).is_empty());
        assert_eq!(b.finish().len(), 0);
    }

    #[test]
    fn scatter_turns_each_stamp_at_random_in_place() {
        let shape = Shape { scatter: 1.0, ..Shape::default() };
        let mut b = StrokeBuilder::new(Brush { shape, ..plain() }, 10.0, Pointer::Stylus);
        let stamps = b.add(&line(20, 200.0));
        assert!(stamps.iter().all(|s| (s.center.y - 50.0).abs() < 1e-3), "on the path");
        let turned = stamps.iter().filter(|s| s.angle.abs() > 0.5).count();
        assert!(turned > stamps.len() / 3, "{turned} of {}", stamps.len());
        assert!(stamps.iter().all(|s| s.angle.abs() <= std::f32::consts::PI + 1e-3));
    }

    #[test]
    fn shape_placement_rotates_scatters_and_counts() {
        let shape = Shape { follow: 1.0, throw: 1.0, count: 3, size_jitter: 0.5, roundness: 0.4, ..Shape::default() };
        let mut b = StrokeBuilder::new(Brush { shape, ..plain() }, 10.0, Pointer::Stylus);
        // a vertical line: the stroke heads +y, so the stamps turn a quarter
        let down: Vec<Sample> =
            (0..=10).map(|i| Sample::at(Vec2::new(50.0, i as f32 * 10.0), 1.0, i as f64 * 4.0)).collect();
        let stamps = b.add(&down);
        assert_eq!(stamps.len() % 3, 0, "three per step");
        let later = &stamps[stamps.len() - 1];
        assert!((later.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-3);
        assert_eq!(later.roundness, 0.4);
        assert!(stamps.iter().any(|s| (s.center.x - 50.0).abs() > 0.5), "thrown off the path");
        assert!(
            stamps.iter().all(|s| s.radius <= 5.0 + 1e-4 && s.radius >= 2.5 - 1e-4),
            "jittered smaller, never larger"
        );
        // the same seed replays the same stroke
        let mut again = StrokeBuilder::new(Brush { shape: b.brush().shape.clone(), ..plain() }, 10.0, Pointer::Stylus);
        assert_eq!(again.add(&down), stamps);
    }

    #[test]
    fn jitter_moves_a_stamp_within_half_its_diagonal_times_the_files_amount() {
        let jittered = |lateral: f32, linear: f32| {
            let mut brush = plain();
            brush.set_setting("jitter_lateral", lateral);
            brush.set_setting("jitter_linear", linear);
            // a horizontal line at y = 50, stamps every pixel
            StrokeBuilder::new(brush, 10.0, Pointer::Stylus).add(&line(20, 200.0))
        };
        // a round 10 px tip fills 10 / 0.797 of its square: half its diagonal and a pixel is 9.87;
        // full lateral jitter is 3 of those across the stroke, none along it
        let reach = 10.0 / crate::brushfile::ROUND_TIP_SHARE * 0.5 * std::f32::consts::SQRT_2 + 1.0;
        let across: Vec<f32> = jittered(1.0, 0.0).iter().map(|s| (s.center.y - 50.0).abs()).collect();
        assert!(across.iter().all(|&d| d <= 3.0 * reach + 1e-3), "{across:?}");
        assert!(across.iter().any(|&d| d > 1.5 * reach));
        // both: one random point a stamp, inside the circle of that reach
        let both = jittered(1.0, 1.0);
        let straight = jittered(0.0, 0.0);
        for (j, s) in both.iter().zip(&straight) {
            assert!(j.center.distance(s.center) <= 3.0 * reach + 1e-3);
        }
    }

    #[test]
    fn roundness_jitter_squeezes_width_and_height_apart() {
        let stamps = |key: &str| {
            let mut brush = plain();
            brush.set_setting(key, 0.8);
            StrokeBuilder::new(brush, 10.0, Pointer::Stylus).add(&line(20, 200.0))
        };
        // the width squeezed: narrower and taller than wide, the height whole
        let narrow = stamps("roundness_jitter_x");
        assert!(narrow.iter().any(|s| s.roundness > 1.5));
        assert!(narrow.iter().all(|s| (s.radius * s.roundness - 5.0).abs() < 1e-3 && s.radius <= 5.0));
        // the height squeezed: flatter, the width whole
        let flat = stamps("roundness_jitter_y");
        assert!(flat.iter().any(|s| s.roundness < 0.6));
        assert!(flat.iter().all(|s| s.radius == 5.0 && s.roundness <= 1.0));
        // a stamp taller than wide is held in its bounds
        let tall = narrow.iter().max_by(|a, b| a.roundness.total_cmp(&b.roundness)).unwrap();
        assert!(tall.bounds().height() as f32 >= 2.0 * tall.radius * tall.roundness);
    }

    #[test]
    fn a_randomised_shape_turns_the_whole_stroke_one_way() {
        let shape = Shape { randomise: true, follow: 0.0, ..Shape::default() };
        let angles = |seed: u32| -> Vec<f32> {
            let mut b =
                StrokeBuilder::with_seed(Brush { shape: shape.clone(), ..plain() }, 10.0, Pointer::Stylus, seed);
            b.add(&line(20, 200.0)).iter().map(|s| s.angle).collect()
        };
        let one = angles(1);
        assert!(one.iter().all(|&a| a == one[0]), "{one:?}");
        assert_ne!(angles(2)[0], one[0]);
    }

    #[test]
    fn count_jitter_takes_a_share_off_the_stamps_beyond_the_first() {
        let shape = Shape { count: 16, ..Shape::default() };
        let mut brush = Brush { shape, ..plain() };
        brush.set_setting("count_jitter", 0.5);
        let stamps = StrokeBuilder::new(brush, 10.0, Pointer::Stylus).add(&line(40, 400.0));
        // stamps of one place share its centre
        let mut counts: Vec<usize> = Vec::new();
        for (i, s) in stamps.iter().enumerate() {
            if i == 0 || stamps[i - 1].center != s.center {
                counts.push(0);
            }
            *counts.last_mut().unwrap() += 1;
        }
        // 15 beyond the first, less up to half of them: 8 to 16, and not always all
        assert!(counts.iter().all(|&n| (8..=16).contains(&n)), "{counts:?}");
        assert!(counts.iter().any(|&n| n < 16) && counts.iter().any(|&n| n > 12), "{counts:?}");
    }

    #[test]
    fn a_brushs_own_pressure_curves_scale_its_count_and_jitters() {
        use crate::brush::PressureCurve;
        use crate::curve::Curve;
        let shape = Shape { count: 5, size_jitter: 0.8, ..Shape::default() };
        let mut brush = Brush { shape, pressure_size: 0.0, ..plain() };
        brush.pressure_curves.set(PressureCurve::Count, Some(Curve::linear()));
        brush.pressure_curves.set(PressureCurve::SizeJitter, Some(Curve::linear()));
        brush.pressure_curves.set(PressureCurve::WetBlur, Some(Curve::linear()));
        let stroke = |brush: &Brush, pointer, pressure: f32| {
            let samples: Vec<Sample> =
                (0..=10).map(|i| Sample::at(Vec2::new(i as f32 * 10.0, 50.0), pressure, i as f64 * 4.0)).collect();
            StrokeBuilder::new(brush.clone(), 10.0, pointer).add(&samples)
        };
        let spread = |stamps: &[Stamp]| {
            let (least, most) = stamps.iter().fold((f32::MAX, 0.0f32), |(l, m), s| (l.min(s.radius), m.max(s.radius)));
            most / least
        };
        // pressing lightly: one stamp a place, hardly jittered
        let light = stroke(&brush, Pointer::Stylus, 0.05);
        assert!(spread(&light) < 1.05, "{}", spread(&light));
        // the wet blur scaled the same way, the rest of the wet mix untouched
        assert!(light.iter().all(|s| s.wet[1] < 0.1 && s.wet[0] == 1.0), "{:?}", light[0].wet);
        // pressing fully, or a finger: five a place, jittered as far as the brush says
        for full in [stroke(&brush, Pointer::Stylus, 1.0), stroke(&brush, Pointer::Finger, 0.0)] {
            assert_eq!(full.len(), 5 * light.len());
            assert!(spread(&full) > 2.0, "{}", spread(&full));
        }
        // without the curves, light pressure jitters and counts in full
        brush.pressure_curves = Default::default();
        let plain_light = stroke(&brush, Pointer::Stylus, 0.05);
        assert_eq!(plain_light.len(), 5 * light.len());
    }

    #[test]
    fn tilt_widens_the_pen_but_not_fingers() {
        let angle = 0.1 * std::f32::consts::FRAC_PI_2;
        let brush =
            Brush { tilt: Tilt { angle, size: 0.1, opacity: 0.0 }, pressure_size: 0.0, max_size: 500.0, ..plain() };
        let mut tilted = Sample::at(Vec2::new(0.0, 0.0), 1.0, 0.0);
        tilted.tilt = crate::brush::FLAT_TILT;
        let pen = StrokeBuilder::new(brush.clone(), 10.0, Pointer::Stylus).preview(&[tilted]);
        let finger = StrokeBuilder::new(brush.clone(), 10.0, Pointer::Finger).preview(&[tilted]);
        // a dab takes all of the lean's growth: the tilt size, as far as the lean has gone
        let grown = 0.1 * Tilt::reach(crate::brush::FLAT_TILT, 0.1) * brush.size_unit();
        assert!((pen[0].radius - (10.0 + grown) / 2.0).abs() < 1e-3, "{}", pen[0].radius);
        assert_eq!(finger[0].radius, 5.0);
        // upright, next to nothing
        tilted.tilt = 0.0;
        let upright = StrokeBuilder::new(brush, 10.0, Pointer::Stylus).preview(&[tilted]);
        assert!(upright[0].radius < 5.1);
    }

    #[test]
    fn a_leaning_pen_moves_its_stamps_toward_the_cap() {
        let angle = 0.1 * std::f32::consts::FRAC_PI_2;
        let brush = Brush { tilt: Tilt { angle, size: 0.0, opacity: 0.0 }, pressure_size: 0.0, ..plain() };
        // leaning with the pen pointing right on the screen
        let mut s = Sample::at(Vec2::new(100.0, 100.0), 1.0, 0.0);
        s.tilt = crate::brush::FLAT_TILT;
        s.azimuth = std::f32::consts::FRAC_PI_2;
        let pen = StrokeBuilder::new(brush.clone(), 10.0, Pointer::Stylus).preview(&[s]);
        let shift = Tilt::reach(crate::brush::FLAT_TILT, 0.1) * std::f32::consts::FRAC_1_SQRT_2 * 5.0;
        assert!(
            (pen[0].center.x - (100.0 + shift)).abs() < 1e-3 && (pen[0].center.y - 100.0).abs() < 1e-3,
            "{:?}",
            pen[0].center
        );
        // fingers, upright pens and the studio's pad stay put
        let finger = StrokeBuilder::new(brush.clone(), 10.0, Pointer::Finger).preview(&[s]);
        assert_eq!(finger[0].center, Vec2::new(100.0, 100.0));
        let pad = StrokeBuilder::new(brush.clone(), 10.0, Pointer::Stylus).on_pad(true).preview(&[s]);
        assert_eq!(pad[0].center, Vec2::new(100.0, 100.0));
        s.tilt = 0.0;
        let upright = StrokeBuilder::new(brush, 10.0, Pointer::Stylus).preview(&[s]);
        assert!((upright[0].center.x - 100.0).abs() < 0.01);
    }

    #[test]
    fn spacing_jitter_and_speed_only_widen_the_gaps() {
        // 600 px in 200 ms: fast input
        let gaps = |settings: &[(&str, f32)]| {
            let mut b = plain();
            for &(k, v) in settings {
                b.set_setting(k, v);
            }
            let samples: Vec<Sample> =
                (0..=50).map(|i| Sample::at(Vec2::new(i as f32 * 12.0, 50.0), 1.0, i as f64 * 4.0)).collect();
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus);
            let all = s.add(&samples);
            all.windows(2).map(|w| w[1].center.x - w[0].center.x).collect::<Vec<f32>>()
        };
        let mean = |g: &[f32]| g.iter().sum::<f32>() / g.len() as f32;
        let plain_gaps = gaps(&[]);
        let least = plain_gaps.iter().copied().fold(f32::MAX, f32::min);
        let jittered = gaps(&[("spacing_jitter", 0.1)]);
        assert!(jittered.iter().all(|&g| g > least - 1e-3));
        assert!(mean(&jittered) > 1.2 * mean(&plain_gaps), "{} {}", mean(&jittered), mean(&plain_gaps));
        let sped = gaps(&[("speed_spacing", 0.05)]);
        assert!(mean(&sped) > 1.2 * mean(&plain_gaps), "{} {}", mean(&sped), mean(&plain_gaps));
    }

    #[test]
    fn flow_bleed_and_roundness_follow_their_own_curves() {
        use crate::curve::Curve;
        let middle = |settings: &[(&str, f32)], level: Option<PressureCurve>| {
            let mut b = Brush { pressure_size: 0.0, ..plain() };
            for &(k, v) in settings {
                b.set_setting(k, v);
            }
            // a curve that holds full pressure throughout
            if let Some(kind) = level {
                b.pressure_curves.set(kind, Some(Curve::new(&[Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0)])));
            }
            let samples: Vec<Sample> =
                (0..=30).map(|i| Sample::at(Vec2::new(i as f32 * 4.0, 50.0), 0.3, i as f64 * 4.0)).collect();
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus);
            s.add(&samples);
            let all = s.finish();
            all[all.len() / 2]
        };
        let flow = [("pressure_flow", 1.0)];
        assert!(middle(&flow, None).alpha < 0.5 * middle(&flow, Some(PressureCurve::Flow)).alpha);
        let bleed = [("pressure_bleed", 1.0)];
        assert!(middle(&bleed, None).grain_press < 0.4);
        assert!(middle(&bleed, Some(PressureCurve::Bleed)).grain_press > 0.99);
        let round = [("pressure_roundness", 1.0)];
        assert!(middle(&round, None).roundness < 0.4);
        assert!(middle(&round, Some(PressureCurve::Roundness)).roundness > 0.99);
    }

    #[test]
    fn the_wet_attack_follows_the_pressure_and_its_tilt_switch() {
        let attack = |pressure: f32, tilt: f32, switch: bool, pointer: Pointer| {
            let mut b = Brush { pressure_size: 0.0, ..plain() };
            if switch {
                b.set_setting("attack_tilt", 1.0);
            }
            let mut s = Sample::at(Vec2::new(10.0, 10.0), pressure, 0.0);
            s.tilt = tilt;
            StrokeBuilder::new(b, 10.0, pointer).preview(&[s])[0].attack
        };
        assert!((attack(1.0, 0.0, false, Pointer::Stylus) - 1.0).abs() < 1e-4);
        // the pressure counts twice: as the opacity takes it, and through the attack's own curve
        assert!((attack(0.4, 0.0, false, Pointer::Stylus) - 0.16).abs() < 0.03);
        // switched on: an upright pen next to none, a leaning one more, a finger as a flat pen
        assert!(attack(1.0, 0.0, true, Pointer::Stylus) < 0.01);
        let flat = Tilt::reach(crate::brush::FLAT_TILT, 0.0);
        assert!((attack(1.0, crate::brush::FLAT_TILT, true, Pointer::Stylus) - flat).abs() < 1e-3);
        let finger = Tilt::reach(std::f32::consts::FRAC_PI_2, 0.0);
        assert!((attack(1.0, 0.0, true, Pointer::Finger) - finger).abs() < 1e-3);
    }

    #[test]
    fn tilt_switches_hold_their_controls_back_until_the_pen_leans() {
        let make = |tilt: f32, switches: bool| {
            let mut b = Brush { pressure_size: 0.0, ..plain() };
            b.shape.count = 8;
            b.grain.depth = 1.0;
            b.set_setting("jitter_lateral", 1.0);
            b.set_setting("grain_depth_min", 0.2);
            if switches {
                for k in ["count_tilt", "jitter_lateral_tilt", "grain_depth_tilt"] {
                    b.set_setting(k, 1.0);
                }
            }
            let mut s = Sample::at(Vec2::new(50.0, 50.0), 1.0, 0.0);
            s.tilt = tilt;
            StrokeBuilder::new(b, 10.0, Pointer::Stylus).preview(&[s])
        };
        // off: all eight stamps, jittered off the point, the grain at full depth
        let off = make(0.0, false);
        assert_eq!(off.len(), 8);
        assert!(off.iter().any(|s| (s.center.y - 50.0).abs() > 0.5));
        // on and upright: one stamp, no jitter, the grain still at full depth
        let upright = make(0.0, true);
        assert_eq!(upright.len(), 1);
        // (the lean's floor, 1/1024, still lets a trace through)
        assert!((upright[0].center.y - 50.0).abs() < 0.05);
        assert!(upright[0].grain_depth > 0.99, "{}", upright[0].grain_depth);
        // on and leaning flat: more stamps, jittered, the grain pulled toward its least
        let flat = make(crate::brush::FLAT_TILT, true);
        assert!(flat.len() > 1);
        // (its angle the brush's tilt angle, 0 here)
        let reach = Tilt::reach(crate::brush::FLAT_TILT, 0.0);
        assert!((flat[0].grain_depth - (0.2 + 0.8 * (1.0 - reach))).abs() < 1e-3, "{}", flat[0].grain_depth);
    }

    #[test]
    fn a_leaning_pen_grows_across_the_stroke_shades_and_rounds_the_stamp() {
        let flat = |x: f32, azimuth: f32| {
            let mut s = Sample::at(Vec2::new(x, 0.0), 1.0, f64::from(x));
            s.tilt = crate::brush::FLAT_TILT;
            s.azimuth = azimuth;
            s
        };
        let stamps = |settings: &[(&str, f32)], azimuth: f32| {
            let angle = 0.1 * std::f32::consts::FRAC_PI_2;
            let mut b =
                Brush { tilt: Tilt { angle, size: 0.1, opacity: 0.0 }, pressure_size: 0.0, max_size: 500.0, ..plain() };
            for (k, v) in settings {
                b.set_setting(k, *v);
            }
            let stroke = [flat(0.0, azimuth), flat(20.0, azimuth), flat(40.0, azimuth)];
            StrokeBuilder::new(b, 10.0, Pointer::Stylus).preview(&stroke)
        };
        // drawn along x: leaning along it (orientation pointing right) grows nothing, leaning
        // across it (pointing up) all of it; without compression either way
        let along = stamps(&[], std::f32::consts::FRAC_PI_2);
        let across = stamps(&[], 0.0);
        let last = |v: &[Stamp]| v.last().map_or(0.0, |s| s.radius);
        assert!((last(&along) - 5.0).abs() < 0.01, "{}", last(&along));
        assert!(last(&across) > 10.0, "{}", last(&across));
        let loose = stamps(&[("tilt_size_compression", 0.0)], std::f32::consts::FRAC_PI_2);
        assert!((last(&loose) - last(&across)).abs() < 0.01);
        // shading by the square root of the gradation as far as the lean has gone; the
        // roundness tilt takes the stamp to its leaning roundness as far
        let reach = Tilt::reach(crate::brush::FLAT_TILT, 0.1);
        let shaded = stamps(&[("tilt_gradation", 0.6), ("tilt_size", 0.0)], 0.0);
        assert!((shaded[0].gradation - (0.6 * reach).sqrt()).abs() < 1e-4, "{}", shaded[0].gradation);
        let rounded = stamps(&[("tilt_roundness", 0.5), ("tilt_size", 0.0)], 0.0);
        assert!((rounded[0].roundness - (1.0 - 0.5 * reach)).abs() < 1e-4, "{}", rounded[0].roundness);
    }

    #[test]
    fn the_studios_dynamics_shape_the_stroke() {
        let with = |key: &str, v: f32| {
            let mut b = plain();
            b.set_setting(key, v);
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus);
            s.add(&line(50, 200.0));
            s.finish()
        };
        let base = with("spacing", 0.1);
        // jitter moves stamps off the line
        assert!(with("jitter_lateral", 1.0).iter().any(|s| (s.center.y - 50.0).abs() > 2.0));
        assert!(base.iter().all(|s| (s.center.y - 50.0).abs() < 1e-3));
        // spacing jitter changes how many stamps there are
        assert_ne!(with("spacing_jitter", 1.0).len(), base.len());
        // fall off fades the far end: by 33 px a fall off of 1 has faded it out, a half by 264
        let faded = with("falloff", 1.0);
        assert!(faded.last().unwrap().alpha < 1e-6 && faded[0].alpha > 0.5);
        let half = with("falloff", 0.5);
        let near = |x: f32| half.iter().min_by(|a, b| (a.center.x - x).abs().total_cmp(&(b.center.x - x).abs()));
        let (start, by_33) = (near(0.0).map_or(0.0, |s| s.alpha), near(33.0).map_or(0.0, |s| s.alpha));
        assert!((by_33 / start - 0.5).abs() < 0.05, "{start} {by_33}");
        // fast input (600 px in 200 ms: 50 px a frame) thins the stroke with speed size
        let fast = |b: Brush| {
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus);
            s.add(&line(50, 600.0));
            s.finish()
        };
        let base = fast(plain());
        let mut b = plain();
        b.set_setting("speed_size", 1.0);
        let thin = fast(b);
        assert!(thin[thin.len() / 2].radius < base[base.len() / 2].radius);
        // and thinner still when a shorter distance counts as fast
        let mut b = plain();
        b.set_setting("speed_size", 1.0);
        let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus)
            .with_input(InputSettings { speed_distance: 0.25, ..InputSettings::default() });
        s.add(&line(50, 600.0));
        let thinner = s.finish();
        // and not at all when slow (200 px in 200 ms) or seen small (a quarter point a pixel)
        let mut b = plain();
        b.set_setting("speed_size", 1.0);
        let mut s = StrokeBuilder::new(b.clone(), 10.0, Pointer::Stylus);
        s.add(&line(50, 200.0));
        assert!((s.finish()[25].radius - base[base.len() / 2].radius).abs() < 1e-4);
        let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus).seen_at(0.25);
        s.add(&line(50, 600.0));
        assert!((s.finish()[40].radius - base[base.len() / 2].radius).abs() < 1e-4);
        assert!(thinner[thinner.len() / 2].radius < thin[thin.len() / 2].radius);
        // roundness falls with light pressure
        let mut b = plain();
        b.set_setting("pressure_roundness", 1.0);
        let light = StrokeBuilder::new(b, 10.0, Pointer::Stylus).preview(&[Sample::at(Vec2::new(0.0, 0.0), 0.2, 0.0)]);
        assert!(light[0].roundness < 0.3);
        // stabilisation averages a zigzag away
        let zigzag: Vec<Sample> = (0..100)
            .map(|i| Sample::at(Vec2::new(i as f32 * 2.0, if i % 2 == 0 { 0.0 } else { 20.0 }), 1.0, i as f64 * 8.0))
            .collect();
        let mut b = plain();
        b.set_setting("stabilization", 1.0);
        let mut s = StrokeBuilder::new(b, 4.0, Pointer::Stylus);
        s.add(&zigzag);
        let ys: Vec<f32> = s.stamps()[40..].iter().map(|s| s.center.y).collect();
        let spread = ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread < 5.0, "{spread}");
    }

    #[test]
    fn more_of_the_studios_dynamics_shape_the_stroke() {
        let stroke = |settings: &[(&str, f32)], pressure: f32, taper: Taper| {
            let mut b = Brush { pen_taper: taper, ..plain() };
            b.grain.image = Some(0);
            b.grain.mode = crate::brush::GrainMode::Moving;
            b.grain.depth = 0.5;
            for &(k, v) in settings {
                b.set_setting(k, v);
            }
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus);
            let samples: Vec<Sample> =
                (0..=50).map(|i| Sample::at(Vec2::new(i as f32 * 4.0, 50.0), pressure, i as f64 * 4.0)).collect();
            s.add(&samples);
            s.finish()
        };
        let mid = |v: &[Stamp]| v[v.len() / 2];
        // flow falls with light pressure
        let plain_light = mid(&stroke(&[], 0.3, Taper::NONE));
        let flow_light = mid(&stroke(&[("pressure_flow", 1.0)], 0.3, Taper::NONE));
        assert!(flow_light.alpha < plain_light.alpha * 0.5, "{} {}", flow_light.alpha, plain_light.alpha);
        // a blunt tip keeps width at the stroke's ends (all but the very first stamp)
        let taper = Taper { start: 0.3, end: 0.3, size: 1.0, opacity: 0.0 };
        let sharp = stroke(&[], 1.0, taper.clone())[2].radius;
        let blunt = stroke(&[("pen_taper_tip", 1.0)], 1.0, taper)[2].radius;
        assert!(blunt > sharp + 1.0, "{blunt} {sharp}");
        // the secondary colour: by pressure, as its curve says, and jittered per stamp
        assert_eq!(mid(&stroke(&[], 0.3, Taper::NONE)).secondary, 0.0);
        assert!((mid(&stroke(&[("pressure_secondary", 1.0)], 0.3, Taper::NONE)).secondary - 0.3).abs() < 0.05);
        let jittered = stroke(&[("stamp_secondary", 1.0)], 1.0, Taper::NONE);
        assert!(jittered.iter().any(|s| s.secondary > 0.5) && jittered.iter().any(|s| s.secondary < 0.5));
        // a moving grain: filled in by pressing harder, never under its least, jittered
        assert!((mid(&stroke(&[], 0.3, Taper::NONE)).grain_depth - 1.0).abs() < 1e-5);
        let light = mid(&stroke(&[("pressure_bleed", 1.0)], 0.3, Taper::NONE));
        let hard = mid(&stroke(&[("pressure_bleed", 1.0)], 0.9, Taper::NONE));
        assert!((light.grain_press - 0.3).abs() < 0.05 && (hard.grain_press - 0.9).abs() < 0.05);
        assert!((hard.grain_depth - 1.0).abs() < 1e-5 && (hard.grain_bleed - 1.0).abs() < 1e-5);
        assert_eq!(mid(&stroke(&[], 0.9, Taper::NONE)).grain_bleed, 0.0);
        assert!(
            stroke(&[("grain_depth_jitter", 1.0), ("grain_depth_min", 0.4)], 1.0, Taper::NONE)
                .iter()
                .all(|s| s.grain_depth >= 0.8 - 1e-4)
        );
        // sized with the stamp when it follows the size: a lighter, smaller stamp, a smaller grain
        let pressured = |zoom: f32| {
            let mut b = plain();
            b.pressure_size = 1.0;
            b.set_setting("grain_zoom", zoom);
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus);
            s.add(&[Sample::at(Vec2::new(0.0, 0.0), 0.5, 0.0)]);
            s.finish()[0].grain_size
        };
        assert!((pressured(1.0) - 1.0).abs() < 1e-5);
        assert!(pressured(0.0) < 0.8);
        // the brush's own motion filtering smooths as the app's does
        let zigzag: Vec<Sample> = (0..60)
            .map(|i| Sample::at(Vec2::new(i as f32 * 2.0, if i % 2 == 0 { 0.0 } else { 20.0 }), 1.0, i as f64 * 8.0))
            .collect();
        let spread = |filtering: f32| {
            let mut b = plain();
            b.set_setting("motion_filtering", filtering);
            let mut s = StrokeBuilder::new(b, 4.0, Pointer::Stylus);
            s.add(&zigzag);
            let ys: Vec<f32> = s.stamps()[20..].iter().map(|s| s.center.y).collect();
            ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)
        };
        assert!(spread(1.0) < spread(0.0) * 0.5, "{} {}", spread(1.0), spread(0.0));
    }

    #[test]
    fn a_brush_oriented_to_the_screen_turns_back_with_the_canvas() {
        let turned = |orient: f32| {
            let mut b = plain();
            b.set_setting("orient_to_screen", orient);
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus).on_view_turned(0.5);
            s.add(&[Sample::at(Vec2::new(0.0, 0.0), 1.0, 0.0)]);
            s.finish()[0].angle
        };
        assert_eq!(turned(0.0), 0.0);
        assert!((turned(1.0) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn preview_does_not_change_the_stroke() {
        let mut b = StrokeBuilder::new(plain(), 10.0, Pointer::Stylus);
        b.add(&line(10, 50.0));
        let before = b.stamp_count();
        let ahead = b.preview(&[Sample::at(Vec2::new(80.0, 50.0), 1.0, 100.0)]);
        assert!(!ahead.is_empty());
        assert_eq!(b.stamp_count(), before);
    }

    #[test]
    fn the_apps_input_settings_smooth_and_curve_pressure() {
        let brush = Brush::round_pen();
        let samples: Vec<Sample> = (0..120)
            .map(|i| {
                let x = i as f32 * 4.0;
                // a shake up and down every other point
                let y = if i % 2 == 0 { 0.0 } else { 6.0 };
                let mut s = Sample::at(Vec2::new(x, y), 0.5, f64::from(i) * 8.0);
                s.tilt = 0.0;
                s
            })
            .collect();
        let spread = |input: InputSettings| {
            let mut b = StrokeBuilder::new(brush.clone(), 4.0, Pointer::Stylus).with_input(input);
            let stamps = b.add(&samples);
            // once settled: the last stamps
            let ys: Vec<f32> = stamps.iter().rev().take(15).map(|s| s.center.y).collect();
            ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)
        };
        let plain = spread(InputSettings::default());
        let filtered = spread(InputSettings { motion_filtering: 1.0, ..InputSettings::default() });
        let expressive =
            spread(InputSettings { motion_filtering: 1.0, motion_expression: 1.0, ..InputSettings::default() });
        assert!(filtered < plain * 0.5, "{filtered} vs {plain}");
        assert!(expressive > filtered, "{expressive} vs {filtered}");
        // a curve that doubles light pressure makes bigger stamps
        let curved = InputSettings {
            pressure_curve: Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.5, 1.0), Vec2::new(1.0, 1.0)]),
            ..InputSettings::default()
        };
        let mut a = StrokeBuilder::new(brush.clone(), 20.0, Pointer::Stylus);
        let mut b = StrokeBuilder::new(brush, 20.0, Pointer::Stylus).with_input(curved);
        let (sa, sb) = (a.add(&samples[..1]), b.add(&samples[..1]));
        assert!(sb[0].radius >= sa[0].radius, "{} vs {}", sb[0].radius, sa[0].radius);
    }

    #[test]
    fn a_moving_grain_rolls_as_far_as_its_movement_says_and_may_start_anywhere() {
        let offsets = |movement: f32, jitter: f32, seed: u32| {
            let mut brush = Brush::round_pen();
            brush.set_setting("grain_movement", movement);
            brush.set_setting("grain_offset_jitter", jitter);
            let mut b = StrokeBuilder::with_seed(brush, 4.0, Pointer::Stylus, seed);
            let mut stamps = b.add(&line(20, 100.0));
            stamps.extend(b.finish());
            stamps.iter().map(|s| s.grain_offset).collect::<Vec<_>>()
        };
        // rolling: the grain moves with the stroke; static: it stays put under the tip
        let rolling = offsets(1.0, 0.0, 7);
        let first = rolling.first().copied().unwrap_or(Vec2::ZERO);
        let last = rolling.last().copied().unwrap_or(Vec2::ZERO);
        assert!(last.x - first.x > 50.0, "{first:?} {last:?}");
        assert!(offsets(0.0, 0.0, 7).iter().all(|o| *o == Vec2::ZERO));
        // offset jitter starts each stroke somewhere else in the grain
        let a = offsets(0.0, 1.0, 7)[0];
        let b = offsets(0.0, 1.0, 8)[0];
        assert_ne!(a, b);
        assert!(a.x >= 0.0 && a.x <= GRAIN_JITTER_SPAN && a.y >= 0.0 && a.y <= GRAIN_JITTER_SPAN);
    }
}
