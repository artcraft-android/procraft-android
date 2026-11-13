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
use std::collections::VecDeque;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

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
    /// Where a moving grain is read from: the stamp's middle in the grain (canvas pixels at this
    /// stamp's tile), each point of the stamp read that far from it as on the canvas (the grain
    /// doesn't squash with the stamp). A rolling grain goes on from stamp to stamp by as
    /// far as they moved, so it stays put as the stamps move over it, whatever their tile; a
    /// static one moves along with each stamp.
    pub grain_offset: Vec2,
    /// How far (radians) the grain's patch under this stamp turns about the stamp's middle, where
    /// it is read from staying put: the grain's rotation control's share of the stamp's own turn.
    pub grain_turn: f32,
    /// How hard the pen presses the paint into a grain (the pressure, 0..1), and how much that
    /// counts (the brush's pressure bleed and its tilt bleed as far as the pen leans, 0..2):
    /// pressed harder, the paint takes on more of the stamp and grain, its strongest parts first.
    /// The lean's share counts in full whatever the pressure, so the press is what the pressure
    /// keeps of the whole.
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
    /// When the input that placed it came (ms, the newest sample's time): wet paint is laid a
    /// display frame's stamps at a time.
    pub time_ms: f64,
    /// How far along the path it is (canvas pixels), whatever jitter threw it from there: wet
    /// paint runs out with this.
    pub distance: f32,
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
            grain_turn: 0.0,
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
            time_ms: 0.0,
            distance: 0.0,
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

    /// This, then `other` on the colour it gives.
    fn then(self, other: Tint) -> Tint {
        let [sk, sa] = self.saturation;
        let [ok, oa] = other.saturation;
        let [bk, ba] = self.brightness;
        let [pk, pa] = other.brightness;
        Tint { hue: self.hue + other.hue, saturation: [sk * ok, sa * ok + oa], brightness: [bk * pk, ba * pk + pa] }
    }

    /// The hue, saturation and brightness moved on by as much again, whatever the colour.
    fn shifted(self, [hue, saturation, brightness]: [f32; 3]) -> Tint {
        let [sk, sa] = self.saturation;
        let [bk, ba] = self.brightness;
        Tint { hue: self.hue + hue, saturation: [sk, sa + saturation], brightness: [bk, ba + brightness] }
    }
}

/// What the spacing is a share of, for a stamp's square of 1 (measured).
const SPACED_SQUARE: f32 = 262.0 / crate::brushfile::PIXELS_PER_SIZE;

/// A stamp before taper: where it sits on the path, and its size and alpha from the input.
#[derive(Clone, Copy, Debug)]
struct Placed {
    center: Vec2,
    /// How far the lateral and linear jitter threw it from the path, of a reach this far: the
    /// reach goes with the size, so a taper draws it back in.
    thrown: Vec2,
    reach: f32,
    distance: f32,
    diameter: f32,
    alpha: f32,
    angle: f32,
    roundness: f32,
    flip: [bool; 2],
    tint: Tint,
    secondary: f32,
    grain_depth: f32,
    grain_press: f32,
    /// The tilt bleed as far as the pen leans (it adds to the pressure bleed).
    bleed_lean: f32,
    /// Where its moving grain is read from, in tiles (see [`Stamp::grain_offset`]), and how far
    /// the patch read turns (see [`Stamp::grain_turn`]).
    grain_read: Vec2,
    grain_turn: f32,
    /// The square its grain's tile goes by (as far as it follows the stamp's size), and the
    /// grain's zoom and what the scale's pressure curve leaves of its scale.
    grain_square: f32,
    grain_zoom: f32,
    grain_scale_by: f32,
    gradation: f32,
    transfer: f32,
    attack: f32,
    wet: [f32; 8],
    time_ms: f64,
    /// The pressure as the size, opacity, bleed and roundness took it (and the stamp's own).
    pressed: [f32; 5],
    /// How much of the stamp's roundness is the drawn shape's, which the pressure squeezes (the
    /// rest is what leaning adds).
    drawn_share: f32,
    /// What the input's speed left of the size (see [`StrokeBuilder::sized`]).
    speed_size: f32,
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
    /// How far the barrel is turned, radians.
    roll: f32,
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
            roll: to.roll,
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
/// The input rates stabilisation, motion filtering and pressure smoothing are tuned for, samples a
/// second: a pen's and a finger's. They work per sample at these rates; at others each sample
/// counts for the time it covers, so they feel the same however fast the input comes.
const PEN_RATE: f64 = 240.0;
const FINGER_RATE: f64 = 120.0;
/// Samples that come closer together than this (milliseconds) came in one batch, timed as they
/// were handed over rather than as they were made (a desktop's pointer history): each stands for
/// a sample at the rates above, and those streamline still holds share the time since the sample
/// before them evenly, as the input they stand for came at a steady rate.
const BATCHED_MS: f64 = 0.5;

/// How much of where it was the path keeps at each point at the rates above, for an `amount` of
/// streamline: strong from the first, as measured (a corner at 600 points a second is cut 18 at
/// 60%, 52 at the most).
fn streamline_keep(amount: f32) -> f32 {
    0.96 * amount.clamp(0.0, 1.0).powf(0.125)
}

/// Streamline keeps a stroke's newest points this long (milliseconds) before they are settled: over
/// them it comes up to the input, more the newer they are, so the path curves in to the pen.
const STREAMLINE_WINDOW_MS: f64 = 50.0;

/// The same for the pressure, `amount` of its streamline.
fn pressure_keep(amount: f32) -> f32 {
    0.999 * amount.clamp(0.0, 1.0).powf(0.0625)
}

/// Motion filtering's weights for a point and those either side of it (an odd count, adding up to
/// one), `amount` and `expression` 0..1, at `rate` samples a second. A windowed low-pass, but only
/// how much of each frequency it lets through is kept, so nothing is moved along: the path doesn't
/// lag or cut inside a curve.
fn motion_weights(amount: f32, expression: f32, rate: f64) -> Vec<f32> {
    let strength = f64::from(amount.clamp(0.0, 1.0)).powf(MOTION_POWER);
    let cut = rate * (1.0 - strength) + MOTION_LOWEST_CUT * strength;
    let longer = 1.0 + (MOTION_EXPRESSION_LONGEST - 1.0) * f64::from(expression.clamp(0.0, 1.0));
    let wanted = rate / cut * longer;
    let size = (wanted.ceil() as usize).next_power_of_two().clamp(2, MOTION_MOST_SAMPLES);
    let span = wanted.min((size - 1) as f64);
    let band = 2.0 * cut / rate;
    let filter: Vec<f64> = (0..span as usize)
        .map(|i| {
            let x = (i as f64 + 1.0 - span / 2.0) * band * PI;
            let sinc = if x == 0.0 { 1.0 } else { x.sin() / x };
            // a Blackman-Harris window
            let t = TAU * i as f64 / span;
            band * sinc * (0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos())
        })
        .collect();
    let circle: Vec<(f64, f64)> = (0..size).map(|k| (TAU * k as f64 / size as f64).sin_cos()).collect();
    let turn = |k: usize| circle.get(k & (size - 1)).copied().unwrap_or((0.0, 1.0));
    let through: Vec<f64> = (0..=size / 2)
        .map(|k| {
            let (mut re, mut im) = (0.0, 0.0);
            for (n, h) in filter.iter().enumerate() {
                let (sin, cos) = turn(k * n);
                re += h * cos;
                im += h * sin;
            }
            f64::hypot(re, im)
        })
        .collect();
    // back from frequencies to samples, each way from the point
    let side: Vec<f64> = (0..=size / 2)
        .map(|j| {
            let wave = |(k, t): (usize, &f64)| t * turn(j * k).1 * if k == 0 || k == size / 2 { 1.0 } else { 2.0 };
            through.iter().enumerate().map(wave).sum()
        })
        .collect();
    let least = side.first().map_or(0.0, |w| w.abs() * 1e-5);
    let reach = side.iter().rposition(|w| w.abs() > least).unwrap_or(0);
    let all: Vec<f64> = side.iter().take(reach + 1).rev().chain(side.iter().skip(1).take(reach)).copied().collect();
    let sum: f64 = all.iter().sum();
    all.iter().map(|w| (w / sum) as f32).collect()
}

/// What a speed control of `amount` leaves of a size or opacity at `speed` (0..1): faster input
/// takes away as far as the amount says; an amount below 0, slower input does (as measured).
fn by_speed(amount: f32, speed: f32) -> f32 {
    let speed = speed.clamp(0.0, 1.0);
    if amount >= 0.0 { 1.0 - amount * speed } else { 1.0 + amount * (1.0 - speed) }
}

/// Stabilisation averages each point with those either side of it: at full strength this many
/// samples' worth each way (at the rate above), fewer as the strength to this power falls.
const MOST_AVERAGED: f32 = 120.0;
const STABILIZATION_POWER: f32 = 2.2;
/// Motion filtering lets through what moves slower than its cut and takes out what shakes faster:
/// at full strength the cut is this, in hertz. Below that it rises to the input's own rate with
/// the strength to this power, so a little is strong already.
const MOTION_LOWEST_CUT: f64 = 1.0 / 16.0;
const MOTION_POWER: f64 = 0.01;
/// How many times longer full expression makes the filter: longer cuts sharper, so more of the
/// hand's own motion under the cut comes through.
const MOTION_EXPRESSION_LONGEST: f64 = 8.0;
/// The most samples the filter may span.
const MOTION_MOST_SAMPLES: usize = 1024;
/// Brush files keep the spacing jitter and the speed's spacing at ten times the studio's
/// controls: the share of a gap they widen it by.
const SPACING_FILE_SCALE: f32 = 10.0;

/// The most a stamp's height may be of its width (a roundness jitter can make it taller than
/// wide).
pub const MOST_ROUNDNESS: f32 = 20.0;

/// The least a brush's roundness goes, as its control sets it: its stamps are never thinner.
pub const LEAST_ROUNDNESS: f32 = 0.05;

/// The lateral and linear jitter as brush files keep them: the control to the power 2.2, times
/// this; each moves a stamp up to that many times half its diagonal (plus a pixel).
pub const JITTER_FILE_SCALE: f32 = 3.0;

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
    /// How much of a stamp's own turn a moving grain's patch takes, −1..1.
    grain_rotation: f32,
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
    /// A grain fixed to the canvas going on stamp by stamp (see [`Brush::grain_each_stamp`]).
    grain_each_stamp: bool,
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
            grain_each_stamp: b.grain_each_stamp(),
            grain_rotation: both("grain_rotation"),
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

/// The path curves through its points in chords this long at most (canvas pixels), and at most
/// this many to a stretch: input this close together walks straight, as it comes.
const CURVE_STEP: f32 = 4.0;
const MOST_PIECES: usize = 64;

/// The point at `t` (0 at `p1`, 1 at `p2`) of the centripetal Catmull–Rom curve through four
/// points: through each of them, with no loops or cusps where they bunch up. With no point
/// before `p1` (or after `p2`) it is the parabola through the three there are, so the first and
/// last stretches bend as the path does on from them; with neither, the straight line.
fn catmull_rom(p0: Option<Vec2>, p1: Vec2, p2: Vec2, p3: Option<Vec2>, t: f32) -> Vec2 {
    let knot = |a: Vec2, b: Vec2| a.distance(b).sqrt().max(1e-4);
    let t2 = knot(p1, p2);
    let u = t2 * t;
    let mix = |a: Vec2, b: Vec2, from: f32, to: f32| a * ((to - u) / (to - from)) + b * ((u - from) / (to - from));
    let a2 = mix(p1, p2, 0.0, t2);
    let before = p0.map(|p0| {
        let t0 = -knot(p0, p1);
        mix(mix(p0, p1, t0, 0.0), a2, t0, t2)
    });
    let after = p3.map(|p3| {
        let t3 = t2 + knot(p2, p3);
        mix(a2, mix(p2, p3, t2, t3), 0.0, t3)
    });
    match (before, after) {
        (Some(b1), Some(b2)) => mix(b1, b2, 0.0, t2),
        (Some(b), None) | (None, Some(b)) => b,
        (None, None) => a2,
    }
}

/// What stabilisation and motion filtering hold of the input: its points, evenly a sample apart at
/// the tuned rate however it came, each to be averaged with those either side once they are known. Past either
/// end the input carries on turned half way round about that end, so the average starts and ends
/// where the input does, however few points there are.
#[derive(Clone, Debug, Default)]
struct Averaged {
    /// The points from the `gone`th on (those before are out of reach of what is still to come).
    points: Vec<PathPoint>,
    gone: usize,
    /// How many have been averaged and gone on to the path.
    settled: usize,
    /// The input as it last came, and how many samples at the tuned rate it is past the last point.
    input: Option<PathPoint>,
    past: f64,
    /// Past the ends the input comes back the way it went, so the average falls short of them,
    /// instead of going on as it went.
    mirrored: bool,
}

/// Points kept past those stabilisation can still reach before they are let go, to do it seldom.
const AVERAGED_SLACK: usize = 256;

impl Averaged {
    /// All the points so far.
    fn count(&self) -> usize {
        self.gone + self.points.len()
    }

    /// The input came on to `to`, `steps` samples after it last did.
    fn add(&mut self, to: PathPoint, steps: f64) {
        let Some(from) = self.input.replace(to) else {
            self.points.push(to);
            return;
        };
        let mut next = 1.0 - self.past;
        while next <= steps {
            self.points.push(from.lerp(to, (next / steps) as f32));
            next += 1.0;
        }
        self.past = steps - (next - 1.0);
    }

    /// The input ends where it is: that is its last point.
    fn close(&mut self) {
        if self.past > 1e-3
            && let Some(input) = self.input
        {
            self.points.push(input);
            self.past = 0.0;
        }
    }

    /// Point `i`'s position, pressure and softened pressures (of those held, counting on past
    /// either end).
    fn lanes(&self, i: isize) -> [f32; 8] {
        let lanes = |p: &PathPoint| {
            let [a, b, c, d, e] = p.soft;
            [p.pos.x, p.pos.y, p.pressure, a, b, c, d, e]
        };
        let (Some(first), Some(end)) = (self.points.first().map(lanes), self.points.last().map(lanes)) else {
            return [0.0; 8];
        };
        let last = self.points.len() as isize - 1;
        if last == 0 {
            return first;
        }
        if self.mirrored {
            let at = i.rem_euclid(2 * last);
            return self.points.get(at.min(2 * last - at) as usize).map_or(first, lanes);
        }
        // each length of the input past an end goes the way it did, on from where the last ended
        let (laps, at) = (i.div_euclid(last), i.rem_euclid(last));
        let forward = laps.rem_euclid(2) == 0;
        let mut there = self.points.get(if forward { at } else { last - at } as usize).map_or(first, lanes);
        let whole = if forward { laps } else { laps + 1 } as f32;
        for ((v, f), e) in there.iter_mut().zip(first).zip(end) {
            *v = (e - f) * whole + if forward { *v } else { 2.0 * f - *v };
        }
        there
    }

    /// Point `i` (of them all) as held.
    fn place(&self, i: usize) -> isize {
        i.saturating_sub(self.gone) as isize
    }

    /// Point `i` and the `half` either side of it, added up.
    fn sum(&self, i: usize, half: usize) -> [f32; 8] {
        let (at, reach) = (self.place(i), half as isize);
        let mut sum = [0.0f32; 8];
        for j in at - reach..=at + reach {
            for (s, v) in sum.iter_mut().zip(self.lanes(j)) {
                *s += v;
            }
        }
        sum
    }

    /// `sum`, point `i`'s, as the next point's.
    fn slide(&self, sum: &mut [f32; 8], i: usize, half: usize) {
        let (at, reach) = (self.place(i), half as isize);
        for ((s, new), old) in sum.iter_mut().zip(self.lanes(at + reach + 1)).zip(self.lanes(at - reach)) {
            *s += new - old;
        }
    }

    /// Point `i` averaged with the `half` either side of it, `sum` being them all added up.
    fn mean(&self, i: usize, half: usize, sum: [f32; 8]) -> Option<PathPoint> {
        self.at_lanes(i, sum.map(|v| v / (2 * half + 1) as f32))
    }

    /// Point `i` with its position and pressures as `lanes` has them.
    fn at_lanes(&self, i: usize, lanes: [f32; 8]) -> Option<PathPoint> {
        let [x, y, pressure, a, b, c, d, e] = lanes;
        let held = *self.points.get(self.place(i) as usize)?;
        let soft = [a, b, c, d, e].map(|v| v.clamp(0.0, 1.0));
        Some(PathPoint { pos: Vec2::new(x, y), pressure: pressure.clamp(0.0, 1.0), soft, ..held })
    }

    /// Lets go of the points no average still to come reaches.
    fn forget(&mut self, half: usize) {
        let over = (self.settled.saturating_sub(half + 1)).saturating_sub(self.gone);
        if over > AVERAGED_SLACK {
            self.points.drain(..over);
            self.gone += over;
        }
    }
}

/// Streamline is tuned for input at this interval; other rates are scaled to match.
const REFERENCE_INTERVAL_MS: f64 = 1000.0 / 120.0;
/// The most samples at the tuned rate one sample stands for (after a pause, the filters catch up
/// rather than wait).
const MOST_STEPS: f64 = 24.0;
/// When a classic taper ends, the stroke runs on past the last point by this many steps (touch,
/// pen), each this much of the one before and turning this much more than it, and the end taper
/// comes to its tip over them.
const RUN_ON_TOUCH: usize = 4;
const RUN_ON_PEN: usize = 1;
const RUN_ON_SHRINK: f32 = 0.66;
const RUN_ON_TURN: f32 = 1.125;
/// Stamps a stroke may hold (a guard against runaway input, far above any real stroke).
const MAX_STAMPS: usize = 400_000;
/// How far the path goes before the stroke's direction (which stamps following it turn to) has
/// turned the way it now goes, a share of the brush's size, and at the least in canvas pixels.
const TURN_REACH: f32 = 0.25;
const TURN_REACH_LEAST: f32 = 1.5;

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
    /// The points streamline hasn't settled yet, each with how long (milliseconds) it came after
    /// the one before; and how many of the newest came in one batch (see [`BATCHED_MS`]).
    streaming: VecDeque<(PathPoint, f64)>,
    batch: usize,
    last_sample: Option<Sample>,
    /// The time of the sample being added (see [`Stamp::time_ms`]).
    now_ms: f64,
    head: Option<PathPoint>,
    /// The path point before the head, and the newest one not yet walked to: the path curves
    /// through them, each stretch walked once the point after it is known.
    behind: Option<PathPoint>,
    ahead: Option<PathPoint>,
    direction: f32,
    /// The direction a dab has, the path not having gone anywhere (see
    /// [`StrokeBuilder::dab_way`]).
    dab_direction: f32,
    /// The way the path has been going, smoothed over [`TURN_REACH`] of the brush (unit length
    /// once settled; zero before the path has moved), which `direction` is the angle of.
    heading: Vec2,
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
    /// The input stabilisation holds; the speed, smoothed; the next gap's jitter.
    averaged: Averaged,
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
    /// The brush's input style is azimuth and barrel roll: its stamps turn as the pen's barrel
    /// does; set relative to the stroke, from how it was turned as the stroke began, else from
    /// the barrel's own zero (a turn held from before the stroke counts).
    roll_turns: bool,
    roll_relative: bool,
    roll_from: Option<f32>,
    /// Screen points a canvas pixel spans, for the speed dynamics.
    points_per_pixel: f32,
    input: InputSettings,
    /// What the path's points are averaged with: motion filtering's weights, stabilisation's
    /// average of them if it has one too. Made as the stroke starts; empty if there is no motion
    /// filtering.
    weights: Option<Arc<[f32]>>,
    /// Where in a moving grain the stroke starts (tiles), and where the last stamp was read from
    /// there and lay on the canvas.
    grain_start: Vec2,
    grain_last: Option<(Vec2, Vec2)>,
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
    /// How many of the stroke's stamps before `placed` a copy leaves out (see
    /// [`StrokeBuilder::open_copy`]).
    left_out: usize,
    /// How far the start taper reaches, once its time has passed: from then on it is what the
    /// stroke keeps (so what is drawn for good stays as it is when the stroke ends).
    start_settled: Option<f32>,
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
    /// Motion filtering keeps the stroke's end on the pen; off, the end falls short of it as much
    /// as the filter smooths.
    pub tip_attached: bool,
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
            tip_attached: true,
        }
    }
}

/// `v` turned `by` radians.
fn turned_by(v: Vec2, by: f32) -> Vec2 {
    if by == 0.0 {
        return v;
    }
    let (sin, cos) = by.sin_cos();
    Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

/// `angle` (radians) from −π up to π.
fn wrapped(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (angle + PI).rem_euclid(TAU) - PI
}

/// Two amounts, 0..1, together: each takes away what the other leaves.
fn together(a: f32, b: f32) -> f32 {
    1.0 - (1.0 - a.clamp(0.0, 1.0)) * (1.0 - b.clamp(0.0, 1.0))
}

impl StrokeBuilder {
    /// The stroke with the app's own smoothing and pressure as well as the brush's.
    pub fn with_input(mut self, input: InputSettings) -> StrokeBuilder {
        // the stronger of the app's and the brush's
        self.extras.stabilization = self.extras.stabilization.max(input.stabilization.clamp(0.0, 1.0));
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
        let roll_turns = brush.setting("input_style") >= 1.5;
        let roll_relative = brush.setting("relative_to_stroke") >= 0.5;
        StrokeBuilder {
            roll_turns,
            roll_relative,
            roll_from: None,
            brush,
            diameter,
            pointer,
            pressure: None,
            smoothed: None,
            streaming: VecDeque::new(),
            batch: 0,
            last_sample: None,
            now_ms: 0.0,
            head: None,
            direction: 0.0,
            dab_direction: 0.0,
            heading: Vec2::ZERO,
            last_seg: 0.0,
            behind: None,
            ahead: None,
            scale: 1.0,
            carry: 0.0,
            length: 0.0,
            placed: Vec::new(),
            bounds: IRect::EMPTY,
            random: Random(seed | 1),
            extras: Extras::default(),
            averaged: Averaged::default(),
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
            weights: None,
            grain_start: Vec2::ZERO,
            grain_last: None,
            history: Vec::new(),
            input_length: 0.0,
            before_last: [None; 2],
            ran_on: None,
            kept: 0,
            left_out: 0,
            start_settled: None,
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
            self.grain_start = Vec2::new(self.random.next(), self.random.next());
        }
        if self.brush.shape.randomise {
            self.stroke_turn = self.random.next() * std::f32::consts::TAU;
        }
        self.dab_direction = self.dab_way();
        self.direction = self.dab_direction;
        self
    }

    /// The way a stamp that turns with the stroke goes before the stroke goes anywhere: up the
    /// screen for one that follows it, less the less it does, and on by the stroke's random turn
    /// (so a stamp that follows the stroke has none).
    fn dab_way(&self) -> f32 {
        -wrapped(std::f32::consts::FRAC_PI_4 * (1.0 + self.brush.shape.follow) + self.stroke_turn)
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

    /// The stamps `samples` would add, without adding them (for drawing predicted input): the
    /// path walked on to the last of them.
    pub fn preview(&mut self, samples: &[Sample]) -> Vec<Stamp> {
        // none of the stamps so far are needed
        let mut ahead = self.open_copy(self.placed.len());
        let mut out = ahead.add(samples);
        ahead.average_on(&mut out);
        ahead.stream_on(&mut out);
        ahead.flush(&mut out);
        out
    }

    /// As [`StrokeBuilder::preview`], and on from the path to the input itself: all the stroke
    /// shows ahead of what is drawn for good.
    pub fn preview_to_input(&mut self, samples: &[Sample]) -> Vec<Stamp> {
        let mut ahead = self.open_copy(self.placed.len());
        let mut out = ahead.add(samples);
        ahead.reach_input(&mut out);
        out
    }

    /// Walks the stretch to the newest path point, as if nothing came after it.
    fn flush(&mut self, out: &mut Vec<Stamp>) {
        if let (Some(c), Some(b)) = (self.ahead.take(), self.head) {
            self.walk_curve(self.behind, b, c, None, out);
            self.behind = Some(b);
        }
    }

    /// The stroke as it shows while drawn: stabilisation's average on to the input, streamline up
    /// to it, the stretch to the newest path point, then on to the input if that is still ahead.
    fn reach_input(&mut self, out: &mut Vec<Stamp>) {
        self.average_on(out);
        self.stream_on(out);
        self.flush(out);
        if let Some(to) = self.input_point().filter(|_| !self.averaged.mirrored) {
            self.walk_to(to, out);
        }
    }

    /// The newest input as a point of the path, smoothed by nothing.
    fn input_point(&self) -> Option<PathPoint> {
        let (last, head) = (self.last_sample?, self.head?);
        let newest = self.ahead.unwrap_or(head);
        Some(PathPoint {
            pos: last.pos,
            pressure: self.pressure.unwrap_or(1.0),
            soft: self.soft.unwrap_or(newest.soft),
            tilt: newest.tilt,
            azimuth: newest.azimuth,
            roll: newest.roll,
            speed: newest.speed,
        })
    }

    /// Whether the stroke shows its tapers as it is drawn, through a tip redrawn as it goes: one
    /// that lasts a time at its start (until that time has passed), and at its end with the tip
    /// animation on.
    pub fn shows_tip(&self) -> bool {
        !self.classic() && (self.taper().start > 0.0 || self.end_shows())
    }

    /// Adds samples to a stroke shown with its tip (see [`StrokeBuilder::shows_tip`]): returns
    /// the stamps to draw for good now, those past the start taper's time that the end taper no
    /// longer reaches, and the tip, the stamps after them as they would be drawn were the stroke
    /// to end here.
    pub fn add_tipped(&mut self, samples: &[Sample]) -> (Vec<Stamp>, Vec<Stamp>) {
        self.add(samples);
        let start = self.start_span();
        let reach = match self.start_settled {
            // the start's reach still moving: nothing is drawn for good yet
            None => f32::NEG_INFINITY,
            Some(_) if self.end_shows() => self.length - self.end_span(0.0),
            Some(_) => f32::INFINITY,
        };
        // the stamps lie in order along the path: the first the taper reaches is after those kept
        let open = self.placed.get(self.kept..).unwrap_or_default();
        let to = open.iter().position(|p| p.distance >= reach).map_or(self.placed.len(), |i| self.kept + i);
        let kept = self.placed.get(self.kept..to).unwrap_or_default();
        let kept = kept.iter().map(|p| self.finish_stamp(p, start, None)).collect();
        self.kept = to;
        // the tip as if the stroke ended here: walked on to the input
        let mut ended = self.open_copy(to);
        ended.reach_input(&mut Vec::new());
        (kept, ended.tip_from(0))
    }

    /// The tip as it would be with `samples` added (for drawing predicted input), the stroke
    /// left as it is.
    pub fn preview_tip(&mut self, samples: &[Sample]) -> Vec<Stamp> {
        let mut ahead = self.open_copy(self.kept);
        ahead.add(samples);
        ahead.reach_input(&mut Vec::new());
        ahead.tip_from(0)
    }

    /// A copy of the stroke holding only its stamps from `from` on, to see how it would go on:
    /// copying every stamp each time cost more the longer the stroke. Those before are set aside
    /// while it is made, not copied.
    fn open_copy(&mut self, from: usize) -> StrokeBuilder {
        let from = from.min(self.placed.len());
        let open = self.placed.split_off(from);
        let before = std::mem::replace(&mut self.placed, open);
        let mut copy = self.clone();
        copy.left_out += from;
        copy.kept = 0;
        let open = std::mem::replace(&mut self.placed, before);
        self.placed.extend(open);
        copy
    }

    /// The stamps from `from` on as they would be drawn were the stroke to end here (its end
    /// tapered only where that shows as it is drawn).
    fn tip_from(&self, from: usize) -> Vec<Stamp> {
        let start = self.start_span();
        let end = self.end_shows().then(|| (self.length, self.end_span(0.0)));
        let tip = self.placed.get(from..).unwrap_or_default();
        tip.iter().map(|p| self.finish_stamp(p, start, end)).collect()
    }

    /// Every stamp so far as currently drawn (no end taper yet).
    pub fn stamps(&self) -> Vec<Stamp> {
        let start = self.start_span();
        self.placed.iter().map(|p| self.finish_stamp(p, start, None)).collect()
    }

    /// The stamps drawn for good so far, of a stroke drawn stamp by stamp (with a tip, where it
    /// shows one) and not yet ended: if ending it leaves them as they are, only the rest are to draw.
    pub fn drawn_for_good(&self) -> Option<Vec<Stamp>> {
        if self.ran_on.is_some() {
            return None;
        }
        let start = self.start_span();
        let drawn = if self.shows_tip() { self.kept } else { self.placed.len() };
        Some(self.placed.iter().take(drawn).map(|p| self.finish_stamp(p, start, None)).collect())
    }

    /// The line a shape is fitted to, from this builder as its stroke began and the stroke's
    /// input: stabilisation and motion filtering smooth it as they smooth the stroke, streamline
    /// doesn't, no taper changes how its stamps are spaced and nothing runs on past the lift. Each
    /// stamp's place on the path, before any jitter threw it.
    pub fn shape_line(&self, samples: &[Sample]) -> Vec<Vec2> {
        let mut line = self.clone();
        line.brush.streamline = 0.0;
        line.brush.pen_taper = Taper::NONE;
        line.brush.touch_taper = Taper::NONE;
        line.add(samples);
        line.finish();
        line.placed.iter().map(|p| p.center - p.thrown).collect()
    }

    /// Ends the stroke: walks on to where the pen lifted and applies the end taper. Returns all of
    /// the stroke's stamps, as they should finally be drawn.
    pub fn finish(&mut self) -> Vec<Stamp> {
        let mut run_on = 0.0;
        let mut ignored = Vec::new();
        self.average_on(&mut ignored);
        self.stream_on(&mut ignored);
        if let Some(to) = self.input_point().filter(|_| !self.averaged.mirrored) {
            // the path comes up to the pen along the curve, where it lifted the last point it
            // curves through
            self.path_to(to, &mut ignored);
            self.flush(&mut ignored);
            run_on = match self.ran_on {
                Some(r) => r,
                None => self.run_on(to, &mut ignored),
            };
        } else {
            self.flush(&mut ignored);
        }
        self.ran_on = Some(run_on);
        let length = self.length;
        // a tap, which never went anywhere, lays one stamp whatever the brush's count, as a dot
        if length <= 0.0 && run_on <= 0.0 {
            self.placed.truncate(1);
        }
        let (start, end) = (self.start_span(), self.end_span(run_on));
        let stamps: Vec<Stamp> = self.placed.iter().map(|p| self.finish_stamp(p, start, Some((length, end)))).collect();
        self.bounds = stamps.iter().fold(IRect::EMPTY, |b, s| b.union(s.bounds()));
        stamps
    }

    fn push(&mut self, s: Sample, out: &mut Vec<Stamp>) {
        self.now_ms = s.time_ms;
        let pen = self.pointer.has_pressure();
        // the samples at the tuned rate this one stands for (one if it came with the last's time,
        // or in a batch with it)
        let interval = 1000.0 / if pen { PEN_RATE } else { FINGER_RATE };
        let since = self.last_sample.map_or(0.0, |last| (s.time_ms - last.time_ms).abs());
        let batched = self.last_sample.is_some() && since < BATCHED_MS;
        let steps = if batched || since <= 0.0 { 1.0 } else { (since / interval).min(MOST_STEPS) as f32 };
        // frames of a 60 Hz display since the last sample (a sample's worth at the tuned rate, if
        // it came with it)
        let frames = if batched || since <= 0.0 { SAMPLE_FRAMES } else { (since * 0.06) as f32 };
        let raw = if pen { self.input.pressure_curve.eval(s.pressure.clamp(0.0, 1.0)).clamp(0.0, 1.0) } else { 1.0 };
        let smoothing = pressure_keep(together(self.brush.pressure_smoothing, self.input.pressure_smoothing));
        let pressure = match self.pressure {
            None => raw,
            Some(p) => p + (raw - p) * (1.0 - smoothing.powf(steps)),
        };
        self.pressure = Some(pressure);
        let (tilt, azimuth) = if pen { (s.tilt, s.azimuth) } else { (0.0, 0.0) };
        let roll = if pen && s.roll.is_finite() { s.roll } else { 0.0 };
        // a brush turned by the barrel notes where it was as the stroke began
        if self.roll_turns && self.roll_from.is_none() {
            self.roll_from = Some(roll);
        }
        let curved = self.brush.pressures(pressure);
        let soft = match (self.last_sample, self.soft) {
            (Some(last), Some(before)) if !self.steady && !self.exact => {
                self.soften(curved, before, frames, s.pos.distance(last.pos))
            }
            _ => curved,
        };
        self.soft = Some(soft);
        self.segments = self.segments.saturating_add(1);
        if let Some(last) = self.last_sample {
            let moved = s.pos.distance(last.pos) * self.points_per_pixel / frames;
            let scale = self.input.speed_distance.clamp(0.25, 1.0);
            let over = (moved - SLOW * scale) / (SPAN * scale);
            let now = if over > 0.0 && !self.steady { over.sqrt().min(1.0) } else { 0.0 };
            // the first move as it is; later ones smoothed over a few frames
            let held = if self.speed_known { SPEED_FRAMES / frames } else { 0.0 };
            self.speed = ((now + held * self.speed) / (held + 1.0)).clamp(0.0, 1.0);
            self.speed_known = true;
        }
        if self.weights.is_none() {
            let weights = self.made_weights();
            self.averaged.mirrored = !self.input.tip_attached && !weights.is_empty();
            self.weights = Some(weights.into());
        }
        let target = PathPoint { pos: s.pos, pressure, soft, tilt, azimuth, roll, speed: self.speed };
        // (none for the first sample; nothing of its own for one in a batch with the last)
        let dt = self.last_sample.map(|last| if batched { 0.0 } else { (s.time_ms - last.time_ms).clamp(0.5, 50.0) });
        self.keep_time(s);
        if let Some(last) = self.last_sample {
            self.before_last = [self.before_last[1], Some(last.pos)];
        }
        self.last_sample = Some(s);
        let half = self.averaged();
        if half > 0 {
            // stabilisation and motion filtering: each point waits until those after it within
            // reach of its average are known
            let times = self.history.iter().rev().map(|h| h.0);
            let gap = times.clone().next().zip(times.clone().nth(1)).map_or(0.0, |(now, before)| now - before);
            self.averaged.add(target, if batched { 1.0 } else { (gap / interval).min(MOST_STEPS) });
            self.average_to(self.averaged.count().saturating_sub(half), half, out);
            self.averaged.forget(half);
        } else {
            self.go_on(target, dt, batched, out);
        }
        self.settle_start();
    }

    /// Takes the path on to `target`, `dt` milliseconds after the last (none: it is the first;
    /// `batched`, it came in a batch with the last, see [`BATCHED_MS`]), streamlined: it goes into
    /// the window, and those it pushes out are settled.
    fn go_on(&mut self, target: PathPoint, dt: Option<f64>, batched: bool, out: &mut Vec<Stamp>) {
        let Some(dt) = dt.filter(|_| !self.exact && self.brush.streamline > 0.0) else {
            self.smoothed = Some(target);
            self.path_to(target, out);
            return;
        };
        self.streaming.push_back((target, dt));
        self.share_batch(batched);
        let mut age: f64 = self.streaming.iter().skip(1).map(|p| p.1).sum();
        while age >= self.stream_reach() {
            let Some((next, dt)) = self.streaming.pop_front() else { break };
            age -= self.streaming.front().map_or(0.0, |p| p.1);
            self.stream_to(next, dt, 0.0, out);
        }
    }

    /// The newest point of streamline's window came in a batch with those before it, or began one:
    /// the batch's points still in the window share the time since the point before it evenly
    /// (each a sample at the tuned rate where there is none to share). Timed as one, all but the
    /// first would hardly move the path and the first would take it the whole way, so the path
    /// would go on in steps of a batch and bend sharply at each.
    fn share_batch(&mut self, batched: bool) {
        self.batch = if batched { self.batch.saturating_add(1) } else { 1 };
        let n = self.batch.min(self.streaming.len());
        if n < 2 {
            return;
        }
        let total: f64 = self.streaming.iter().rev().take(n).map(|p| p.1).sum();
        let each = if total > 0.0 { total / n as f64 } else { self.tuned_interval() };
        for point in self.streaming.iter_mut().rev().take(n) {
            point.1 = each;
        }
    }

    /// How old (milliseconds) a point of streamline's window is when it stops following any better
    /// than the rest of the path: as old as the last of the window's samples at the tuned rate.
    fn stream_reach(&self) -> f64 {
        STREAMLINE_WINDOW_MS - self.tuned_interval()
    }

    /// The milliseconds between samples at the rate smoothing is tuned for.
    fn tuned_interval(&self) -> f64 {
        1000.0 / if self.pointer.has_pressure() { PEN_RATE } else { FINGER_RATE }
    }

    /// Takes the smoothed path to `to`, `dt` milliseconds on, `near` how close it is to the input
    /// (0 for a point out of the window, 1 for the newest, which it reaches).
    fn stream_to(&mut self, to: PathPoint, dt: f64, near: f32, out: &mut Vec<Stamp>) {
        let strength = 1.0 - streamline_keep(self.brush.streamline);
        let follows = strength + near * (near - strength);
        let keep = (1.0 - follows).max(0.0).powf((dt / self.tuned_interval()) as f32);
        let smoothed = match self.smoothed {
            Some(prev) => PathPoint { pos: prev.pos.lerp(to.pos, 1.0 - keep), ..to },
            None => to,
        };
        self.smoothed = Some(smoothed);
        self.path_to(smoothed, out);
    }

    /// Takes the path on through streamline's window to the input, as if no more were to come.
    fn stream_on(&mut self, out: &mut Vec<Stamp>) {
        let strength = 1.0 - streamline_keep(self.brush.streamline);
        let reach = self.stream_reach();
        let mut age: f64 = self.streaming.iter().skip(1).map(|p| p.1).sum();
        while let Some((next, dt)) = self.streaming.pop_front() {
            let near = (1.0 - age / reach).max(0.0).powf(1.0 - f64::from(strength)) as f32;
            self.stream_to(next, dt, near, out);
            age -= self.streaming.front().map_or(0.0, |p| p.1);
        }
    }

    /// Takes the path on along stabilisation's points up to the `to`th, each averaged with the
    /// `half` either side of it.
    fn average_to(&mut self, to: usize, half: usize, out: &mut Vec<Stamp>) {
        let interval = 1000.0 / if self.pointer.has_pressure() { PEN_RATE } else { FINGER_RATE };
        let mut i = self.averaged.settled;
        if i >= to {
            return;
        }
        if let Some(weights) = self.weights.clone().filter(|w| !w.is_empty()) {
            let (from, end) = (self.averaged.place(i) - half as isize, self.averaged.place(to - 1) + half as isize);
            let lanes: Vec<[f32; 8]> = (from..=end).map(|j| self.averaged.lanes(j)).collect();
            for around in lanes.windows(weights.len()) {
                let mut sum = [0.0f32; 8];
                for (w, point) in weights.iter().zip(around) {
                    for (s, v) in sum.iter_mut().zip(point) {
                        *s += w * v;
                    }
                }
                if let Some(next) = self.averaged.at_lanes(i, sum) {
                    let next = self.still_where_it_was(next);
                    self.go_on(next, Some(interval), false, out);
                }
                i += 1;
            }
            self.averaged.settled = i;
            return;
        }
        let mut sum = self.averaged.sum(i, half);
        while i < to {
            let next = self.averaged.mean(i, half, sum);
            self.averaged.slide(&mut sum, i, half);
            i += 1;
            if let Some(next) = next {
                let next = self.still_where_it_was(next);
                self.go_on(next, Some(interval), false, out);
            }
        }
        self.averaged.settled = i;
    }

    /// `next`, an average of the input: where the input hasn't gone anywhere it stays exactly there,
    /// or the average's rounding would read as a move and a tap as a stroke.
    fn still_where_it_was(&self, next: PathPoint) -> PathPoint {
        match self.averaged.points.first().filter(|_| self.input_length <= 0.0) {
            Some(first) => PathPoint { pos: first.pos, ..next },
            None => next,
        }
    }

    /// Takes the path on along stabilisation's points to the input's end, as if no more were to
    /// come: where the stroke ends as it now stands, or would.
    fn average_on(&mut self, out: &mut Vec<Stamp>) {
        let half = self.averaged();
        if half > 0 {
            self.averaged.close();
            self.average_to(self.averaged.count(), half, out);
        }
    }

    /// How many samples (at the tuned rate) each way the path's points are averaged: none when
    /// neither stabilisation nor motion filtering is on (and on an exact path).
    fn averaged(&self) -> usize {
        match self.weights.as_deref() {
            Some(weights) if !weights.is_empty() => weights.len() / 2,
            _ => self.stabilised(),
        }
    }

    /// Motion filtering's weights, with stabilisation's average of them: both go by the same
    /// points, so one pass does the two. The stronger of the app's and the brush's.
    fn made_weights(&self) -> Vec<f32> {
        let amount = self.extras.motion_filtering.max(self.input.motion_filtering);
        if self.exact || amount <= 0.0 {
            return Vec::new();
        }
        let expression = self.extras.motion_expression.max(self.input.motion_expression);
        let rate = if self.pointer.has_pressure() { PEN_RATE } else { FINGER_RATE };
        let filter = motion_weights(amount, expression, rate);
        let width = 2 * self.stabilised() + 1;
        let mut both = vec![0.0; filter.len() + width - 1];
        for (i, w) in filter.iter().enumerate() {
            for b in both.iter_mut().skip(i).take(width) {
                *b += w / width as f32;
            }
        }
        both
    }

    /// How many samples each way stabilisation averages.
    fn stabilised(&self) -> usize {
        if self.exact {
            return 0;
        }
        (self.extras.stabilization.clamp(0.0, 1.0).powf(STABILIZATION_POWER) * MOST_AVERAGED) as usize
    }

    /// Takes the path on to `to`: the stretch before it walked along the curve through the points
    /// around it (an exact path goes straight, keeping its corners).
    fn path_to(&mut self, to: PathPoint, out: &mut Vec<Stamp>) {
        let Some(head) = self.head.filter(|_| !self.exact) else {
            self.walk_to(to, out);
            return;
        };
        // a point where the newest one is (the pen held still, or let go where it last was) adds
        // nothing to bend by: it only brings that one's pressure and the rest up to date
        let newest = self.ahead.unwrap_or(head);
        if newest.pos.distance(to.pos) <= 1e-4 {
            let held = Some(PathPoint { pos: newest.pos, ..to });
            if self.ahead.is_some() {
                self.ahead = held;
            } else {
                self.head = held;
            }
            return;
        }
        let Some(next) = self.ahead.replace(to) else { return };
        self.walk_curve(self.behind, head, next, Some(to), out);
        self.behind = Some(head);
    }

    /// Walks from the head `b` to `c` along the centripetal Catmull–Rom curve through `a`, `b`,
    /// `c` and `d` (the parabola through three where the path has no point before or after), in
    /// chords of at most [`CURVE_STEP`] (one, straight, if it is that short); the pressure and
    /// the rest change evenly along it.
    fn walk_curve(
        &mut self,
        a: Option<PathPoint>,
        b: PathPoint,
        c: PathPoint,
        d: Option<PathPoint>,
        out: &mut Vec<Stamp>,
    ) {
        let whole = b.pos.distance(c.pos);
        let pieces = if whole.is_finite() { ((whole / CURVE_STEP).ceil() as usize).clamp(1, MOST_PIECES) } else { 1 };
        for i in 1..=pieces {
            let t = i as f32 / pieces as f32;
            let pos =
                if i == pieces { c.pos } else { catmull_rom(a.map(|a| a.pos), b.pos, c.pos, d.map(|d| d.pos), t) };
            self.walk_piece(PathPoint { pos, ..b.lerp(c, t) }, Some(whole), out);
        }
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

    /// How far stamps turn with the stroke: a pen turns them as it is held (its azimuth) in place
    /// of following the stroke, which is for touch.
    fn follow(&self) -> f32 {
        let held = self.brush.shape.follow_azimuth && self.pointer.has_pressure();
        if held { 0.0 } else { self.brush.shape.follow }
    }

    fn classic(&self) -> bool {
        self.brush.extra.get("classic_taper").is_some_and(|&v| v >= 0.5)
    }

    /// How far along the path the start taper grows to full (0: none): the classic one over its
    /// length in canvas pixels; the newer one over the input up to the first point past its time
    /// (see [`StrokeBuilder::settle_start`]).
    fn start_span(&self) -> f32 {
        if let Some(span) = self.start_settled {
            return span;
        }
        let taper = self.taper();
        if taper.start <= 0.0 {
            return 0.0;
        }
        if self.classic() {
            return taper.start * CLASSIC_TAPER_SPAN;
        }
        self.on_path(self.start_input())
    }

    /// How far the input had come at the first point past the newer start taper's time.
    fn start_input(&self) -> f32 {
        let Some(&(t0, _)) = self.history.first() else { return 0.0 };
        let time = f64::from(self.taper().start) * TAPER_TIME_MS;
        self.history.iter().find(|(t, _)| t - t0 > time).map_or(self.input_length, |&(_, l)| l)
    }

    /// Whether the start taper's reach is known: none, the classic one's (a length), or the input
    /// has gone on past the newer one's time.
    fn start_known(&self) -> bool {
        let taper = self.taper();
        if taper.start <= 0.0 || self.classic() {
            return true;
        }
        let time = f64::from(taper.start) * TAPER_TIME_MS;
        matches!((self.history.first(), self.history.last()), (Some(&(t0, _)), Some(&(t, _))) if t - t0 > time)
    }

    /// Fixes the start taper's reach as the input passes its time, at how far the input had come
    /// (the path, waiting a point behind it, may not have got there yet): later input doesn't
    /// move it, so the stroke's start drawn for good stays as it is.
    fn settle_start(&mut self) {
        if self.start_settled.is_some() || !self.start_known() {
            return;
        }
        let timed = self.taper().start > 0.0 && !self.classic();
        self.start_settled = Some(if timed { self.start_input() } else { self.start_span() });
    }

    /// Whether the end taper shows while the stroke is drawn: with the tip animation on, a
    /// finger's as a pen's (a classic taper's only as it ends, where it runs on).
    fn end_shows(&self) -> bool {
        !self.classic() && self.taper().end > 0.0 && self.extras.tip_animation
    }

    /// How far back from the end the end taper starts (0: none): the classic one over the run-on
    /// (`run_on` canvas pixels) up to its length; the newer one over the input since the last
    /// point before its time, counted back from the last move: held still, then lifted, the end
    /// keeps the taper it showed.
    fn end_span(&self, run_on: f32) -> f32 {
        let taper = self.taper();
        if taper.end <= 0.0 {
            return 0.0;
        }
        if self.classic() {
            return run_on.min(taper.end * CLASSIC_TAPER_SPAN);
        }
        let Some(&(_, total)) = self.history.last() else { return 0.0 };
        let moved = self.history.iter().find(|(_, l)| *l >= total).map_or(0.0, |&(t, _)| t);
        let time = f64::from(taper.end) * TAPER_TIME_MS;
        let first = self.history.iter().position(|(t, _)| *t >= moved - time).unwrap_or(0);
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

    /// The path went `d` (`seg` long): the direction turns towards it as far as its length is of
    /// the reach, so the pen's wobbles shorter than that (slowing at a stroke's ends) don't swing
    /// stamps that follow the stroke out across it.
    fn turn_towards(&mut self, d: Vec2, seg: f32) {
        let reach = (self.diameter * TURN_REACH).max(TURN_REACH_LEAST);
        let k = (seg / reach).min(1.0);
        self.heading = self.heading * (1.0 - k) + d * (k / seg);
        if self.heading.length() > 1e-6 {
            self.direction = self.heading.y.atan2(self.heading.x);
        }
    }

    /// Walks the path from its head to `to`, placing stamps at the brush's spacing.
    fn walk_to(&mut self, to: PathPoint, out: &mut Vec<Stamp>) {
        self.walk_piece(to, None, out);
    }

    /// As [`walk_to`](Self::walk_to), a piece of a stretch of input `whole` long (for how much
    /// a lean compresses), else a stretch of its own.
    fn walk_piece(&mut self, to: PathPoint, whole: Option<f32>, out: &mut Vec<Stamp>) {
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
        self.turn_towards(to.pos - from.pos, seg);
        // the stamps where the path starts were placed before it had a direction: they turn with
        // it now, as the rest will
        if self.length <= 0.0 {
            let turn = self.follow() * (self.direction - self.dab_direction);
            for p in &mut self.placed {
                p.angle += turn;
            }
        }
        self.last_seg = whole.unwrap_or(seg);
        let mut done = 0.0f32;
        // each input segment goes on from the plain gap where it starts
        let mut last = self.step(from);
        while self.left_out + self.placed.len() < MAX_STAMPS {
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

    /// The (size, opacity) multipliers the pressure gives a point: none for a finger, which has
    /// no pressure (a brush that thins as it is pressed keeps its full size under a finger).
    fn pressed(&self, p: PathPoint) -> (f32, f32) {
        if self.pointer.has_pressure() { self.brush.factors(p.soft[0], p.soft[1]) } else { (1.0, 1.0) }
    }

    /// The stamp's size with `by` of it left (0..1, what the pressure and the speed leave): they
    /// take it down to the brush's least, not to nothing.
    fn sized(&self, by: f32) -> f32 {
        let least = (self.brush.min_size.min(self.brush.biggest()) * self.scale).min(self.diameter);
        least + (self.diameter - least) * by.max(0.0)
    }

    /// What faster input leaves of the size at a point (an amount below 0: slower input).
    fn speed_size(&self, p: PathPoint) -> f32 {
        by_speed(self.extras.speed_size, p.speed).max(0.0)
    }

    /// The stamp diameter at a point of the path, before jitter and taper.
    fn diameter_at(&self, p: PathPoint) -> f32 {
        let (pressure, _) = self.pressed(p);
        let drawn = self.sized(pressure * self.speed_size(p));
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

    /// The plain distance to the next stamp: the spacing's share of the stamp's square by its
    /// shorter side (a flat stamp lies as close as its height says), and half a pixel more (and a
    /// little with the spacing).
    fn step(&self, p: PathPoint) -> f32 {
        let spacing = self.brush.spacing.max(0.005);
        let extra = 0.5 + (spacing - SPACING_FLOOR).max(0.0) / (SPACING_SPAN / 2.0);
        let shorter = self.square_at(p) * SPACED_SQUARE * self.roundness_at(p).min(1.0);
        (spacing * shorter * self.extras.spacing_stride + extra).max(0.25)
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
        self.diameter_at(p) / if self.brush.round_tip() { crate::brushfile::ROUND_TIP_SHARE } else { 1.0 }
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

    /// How much of the stamp at a point is its drawn shape, not what leaning grows it by.
    fn drawn_share(&self, p: PathPoint) -> f32 {
        let grown = self.tilt_growth(p);
        let drawn = (self.diameter_at(p) - grown).max(1e-3);
        drawn / (drawn + grown)
    }

    /// The stamp's height over its width at a point, before jitter and taper: leaning adds the same
    /// to its height as to its width, then the roundness tilt takes it from its upright roundness
    /// towards its leaning one.
    fn roundness_at(&self, p: PathPoint) -> f32 {
        let x = &self.extras;
        let leant = self.leaning(p, x.tilt_angles.roundness);
        let tilted = x.tilt_roundness_max * (1.0 - leant) + (1.0 - x.tilt_roundness) * leant;
        let size_left = self.pressed(p).0 * self.speed_size(p);
        self.pressed_roundness(self.drawn_share(p), p.soft[3], size_left) * tilted
    }

    /// The tilt switches act: on a pen's lean, and on the pad only if the brush previews tilt.
    /// Otherwise they aren't there, so what they would hold back isn't.
    fn switches_act(&self) -> bool {
        self.pointer.has_pressure() && self.pad != Some(false)
    }

    /// How far a tilt switch at `theta` lets its control act: all of it when the switch is off or
    /// doesn't act, as far as the pen leans when it is on.
    fn switched(&self, p: PathPoint, theta: Option<f32>) -> f32 {
        match theta {
            Some(theta) if self.switches_act() => self.leaning(p, theta),
            _ => 1.0,
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
        let (_, pressure_alpha) = self.pressed(p);
        // leaning fades the stamp by the square root of how far it has gone
        let fade = self.leaning(p, x.tilt_angles.opacity).sqrt() * self.brush.tilt.opacity;
        let tilt_alpha = if fade >= 0.0 { 1.0 - fade } else { 1.0 - (fade + self.brush.tilt.opacity) };
        let speed_alpha = by_speed(x.speed_opacity, p.speed).clamp(0.0, 1.0);
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
        // (wet paint is laid stamp by stamp as it is mixed, and a stroke laid in place takes the
        // rule pixel by pixel, not built up gradually)
        if self.brush.rendering == Rendering::Gradual && !self.brush.mixes_wet() && !self.brush.laid_in_place() {
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
        // the wet mix follows the stamp's own pressure, a finger's full
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
        let drawn_share = self.drawn_share(p);
        let speed_size = self.speed_size(p);
        let base_roundness = self.roundness_at(p);
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
        let color_switches = if self.switches_act() { sw.color } else { [None; 5] };
        let color_tilt = color_switches.map(|theta| self.switched(p, theta));
        let (mut color_first, mut color_more) = (color_jitter_by, [0.0f32; 5]);
        for ((((first, more), tilt), on), has_curve) in
            color_first.iter_mut().zip(color_more.iter_mut()).zip(color_tilt).zip(color_switches).zip(color_has_curve)
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
        let pressed = if self.pointer.has_pressure() { p.soft[1] } else { self.brush.touch_opacity };
        let attack = (pressed.clamp(0.0, 1.0) * attack_tilt * curves.through(PC::Attack, pressure)).clamp(0.0, 1.0);
        let [hue_by, saturation_by, lightness_by, darkness_by, secondary_by] = color_first;
        let [hue_more, saturation_more, lightness_more, darkness_more, secondary_more] = color_more;
        // a pen turns the stamp as it is held (its azimuth, and roll), in place of following the
        // stroke, which is for touch; relative to the stroke, it turns from the stroke's way
        let follow = self.follow();
        for _ in 0..count {
            let size = 1.0 - shape.size_jitter.clamp(0.0, 1.0) * size_jitter_by * self.random.next();
            let fade = 1.0 - shape.opacity_jitter.clamp(0.0, 1.0) * opacity_jitter_by * self.random.next();
            let mut center = p.pos + cap;
            let mut thrown = Vec2::ZERO;
            // one random point a stamp: as far as a share of the reach, in a random direction, the
            // reach across and along the stroke each its own
            if x.jitter_lateral > 0.0 || x.jitter_linear > 0.0 {
                let far = self.random.next();
                let towards = (self.random.next() * 2.0 - 1.0) * std::f32::consts::PI;
                thrown = across * (far * towards.sin() * jitter_lateral * jitter_reach)
                    + along * (far * towards.cos() * jitter_linear * jitter_reach);
                center = center + thrown;
            }
            if shape.throw > 0.0 {
                let a = self.random.next() * std::f32::consts::TAU;
                let r = self.random.next().sqrt() * shape.throw * diameter;
                center = center + Vec2::new(a.cos() * r, a.sin() * r);
            }
            let turned = self.stroke_turn - self.screen_turn + follow * self.direction;
            let mut own = shape.rotation;
            if shape.follow_azimuth {
                own += p.azimuth;
            }
            if let Some(from) = self.roll_from {
                own += if self.roll_relative { p.roll - from } else { p.roll };
            }
            if shape.scatter > 0.0 {
                own += (self.random.next() * 2.0 - 1.0) * shape.scatter * scatter_by * std::f32::consts::PI;
            }
            let angle = turned + own;
            // the roundness jitter squeezes the width and the height apart, and the stamp is scaled
            // back so the larger of them stays whole
            let [jitter_y, jitter_x] = x.roundness_jitter;
            let squeezed = |jitter: f32, rng: &mut Random| if jitter > 0.0 { 1.0 - jitter * rng.next() } else { 1.0 };
            let wide = squeezed(jitter_x * roundness_x_by, &mut self.random);
            let tall = squeezed(jitter_y * roundness_y_by, &mut self.random);
            let most = wide.max(tall).max(1e-3);
            let (wide, tall) = ((wide / most).max(0.05), tall / most);
            let roundness = (base_roundness * tall / wide).clamp(LEAST_ROUNDNESS, MOST_ROUNDNESS);
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
            // the stamp's jitter goes from the colour the stroke's left
            let tint = self.stroke_shift.then(stamp_tint).shifted(moved);
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
            // a moving grain: from its least to its depth, as far as the pressure's curve, the
            // jitter (shallower from stamp to stamp) and the depth tilt (leaning takes it down to
            // its least) leave it
            let base = self.brush.grain.depth.max(1e-3);
            let jitter = if x.grain_depth_jitter > 0.0 {
                x.grain_depth_jitter * depth_jitter_by * self.random.next()
            } else {
                0.0
            };
            let least = x.grain_depth_min.clamp(0.0, base);
            let tilted = sw.grain_depth.filter(|_| self.switches_act()).map_or(0.0, |theta| self.leaning(p, theta));
            let depth = (least + (base - least) * (1.0 - jitter) * depth_by * (1.0 - tilted)).clamp(0.0, 1.0);
            let grain_press = p.soft[2].clamp(0.0, 1.0);
            // its tile under this stamp (following the stamp's size, or cropped), as a multiple of
            // the grain's own scale, which the renderer takes it times
            let (grain_square, grain_zoom) = (self.square_at(p) * size, x.grain_zoom * zoom_by);
            let tile = self.brush.grain.moving_tile(grain_square, grain_zoom, grain_scale_by);
            // it rolls on by as far as the stamp moved, at this stamp's tile and movement: so a
            // tile that follows the size (or a movement that follows the pressure) changes how
            // fast it rolls and doesn't send it elsewhere
            let rolled = x.grain_movement * movement_by / tile;
            let moved = match self.grain_last {
                Some((_, from)) => center - from,
                None => center,
            };
            let start = self.grain_last.map_or(self.grain_start, |(read, _)| read);
            let grain_read = start + turned_by(moved, self.screen_turn) * rolled;
            self.grain_last = Some((grain_read, center));
            // the patch read takes the rotation control's share of the stamp's whole turn, which
            // counts from an eighth of a turn, from a pen's lean the other way (kept to the
            // screen, the grain is too)
            let held = if shape.follow_azimuth && self.pointer.has_pressure() { std::f32::consts::PI } else { 0.0 };
            let from_eighth = wrapped(std::f32::consts::FRAC_PI_4 + turned) + own - held;
            let grain_turn = x.grain_rotation * from_eighth + self.screen_turn;
            let placed = Placed {
                center,
                thrown,
                reach: jitter_reach,
                distance,
                diameter: diameter * size * wide,
                alpha: alpha * fade,
                angle,
                roundness,
                flip,
                tint,
                secondary,
                grain_depth: depth / base,
                grain_press,
                bleed_lean: (x.tilt_bleed * self.leaning(p, x.tilt_angles.bleed)).clamp(0.0, 1.0),
                grain_read,
                grain_turn,
                grain_square,
                grain_zoom,
                grain_scale_by,
                gradation: (x.tilt_gradation * self.leaning(p, x.tilt_angles.gradation)).max(0.0).sqrt().min(1.0),
                transfer,
                attack,
                wet,
                time_ms: self.now_ms,
                pressed: p.soft,
                drawn_share,
                speed_size,
            };
            self.placed.push(placed);
            let stamp = self.finish_stamp(&placed, start_span, None);
            self.bounds = self.bounds.union(stamp.bounds());
            out.push(stamp);
        }
    }

    /// The stamp's roundness before leaning and jitter: the shape's, squeezed as light pressure
    /// `pressure` says, on the `drawn_share` of the stamp that isn't grown by leaning. Its height
    /// goes to the brush's least as its width does: with `size_left` of the width left, the less
    /// of it there is, the rounder a stamp held up by the least.
    fn pressed_roundness(&self, drawn_share: f32, pressure: f32, size_left: f32) -> f32 {
        let squeezed = 1.0 - self.extras.pressure_roundness * (1.0 - pressure.clamp(0.0, 1.0));
        let round = self.brush.shape.roundness.clamp(0.05, 1.0) * squeezed;
        let wide = self.sized(size_left).max(1e-3);
        drawn_share * self.sized(size_left * round) / wide + (1.0 - drawn_share)
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
        let fading = self.pointer.has_pressure() && fade > 0.0;
        let (mut grain_press, mut roundness) = (p.grain_press, p.roundness);
        let [s, o, b, r, _] = p.pressed;
        let size_left = if self.pointer.has_pressure() { self.brush.factors(s, o).0 } else { 1.0 } * p.speed_size;
        let (mut faded_left, mut faded_round) = (size_left, r);
        if fading {
            let kept = (1.0 - fade) + fade * (start * finish).powf(Taper::tip_power(tip));
            let (_, o0) = self.brush.factors(s, o);
            let (s1, o1) = self.brush.factors(s * kept, o * kept);
            (faded_left, faded_round) = (s1 * p.speed_size, r * kept);
            let sized = self.sized(size_left);
            if sized > 0.0 {
                size *= self.sized(faded_left) / sized;
            }
            if o0 > 0.0 {
                alpha *= (o1 / o0).powf(FLOW_POWER);
            }
            grain_press = (b * kept).clamp(0.0, 1.0);
        }
        // a taper thins what the stamp is drawn at; what leaning grows it by comes after, so a
        // leaning pen's tips keep that
        let share = p.drawn_share;
        let thinned = share * size + (1.0 - share);
        if fading || share < 1.0 {
            let before = self.pressed_roundness(share, r, size_left);
            let left_share = if thinned > 0.0 { share * size / thinned } else { share };
            if before > 0.0 {
                let after = self.pressed_roundness(left_share, faded_round, faded_left);
                roundness = (roundness * after / before).clamp(0.05, MOST_ROUNDNESS);
            }
        }
        // the bleed: the pressure's, through the pressure, and the lean's, whatever the pressure
        let pressed_bleed = self.extras.pressure_bleed.clamp(0.0, 1.0);
        let grain_bleed = pressed_bleed + p.bleed_lean;
        if grain_bleed > 0.0 {
            grain_press = (pressed_bleed * grain_press + p.bleed_lean) / grain_bleed;
        }
        // the jitter's reach (half the stamp's diagonal and a pixel) goes with what is left of it
        let reach_left = ((p.reach - 1.0) * thinned + 1.0) / p.reach.max(1.0);
        // a grain that follows the stamp's size follows what is left of it
        let tile = self.brush.grain.moving_tile(p.grain_square * thinned, p.grain_zoom, p.grain_scale_by);
        let center = p.center - p.thrown * (1.0 - reach_left);
        // a grain fixed to the canvas going on stamp by stamp is read where the stamp lands, at the
        // brush's own tile and unturned, as it is read over a whole stroke
        let (grain_offset, grain_turn, grain_size) = if self.extras.grain_each_stamp {
            (center, 0.0, 1.0)
        } else {
            (p.grain_read * tile, p.grain_turn, (tile / self.brush.grain.scale.max(1.0)).max(1e-3))
        };
        Stamp {
            center,
            // a shape goes on showing on the quad that reaches past it however small it gets
            radius: (p.diameter * thinned * 0.5).max(if self.brush.round_tip() { 0.25 } else { 0.01 }),
            alpha: (p.alpha * alpha).clamp(0.0, 1.0),
            angle: p.angle,
            roundness,
            grain_offset,
            grain_turn,
            grain_press,
            grain_bleed,
            flip: p.flip,
            tint: p.tint,
            secondary: p.secondary,
            grain_depth: p.grain_depth,
            grain_size,
            gradation: p.gradation,
            taper: size.clamp(0.0, 1.0),
            transfer: p.transfer,
            attack: p.attack,
            wet: p.wet,
            time_ms: p.time_ms,
            distance: p.distance,
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
            min_size: 0.0,
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
    fn a_tap_turns_a_stamp_that_follows_the_stroke_as_if_it_went_up_the_screen() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
        let tap = |follow: f32, randomise: bool, seed: u32| {
            let mut b = plain();
            b.set_setting("shape_rotation", follow);
            b.shape.randomise = randomise;
            let mut s = StrokeBuilder::with_seed(b, 10.0, Pointer::Finger, seed);
            s.add(&[Sample::at(Vec2::new(3.0, 4.0), 1.0, 0.0)]);
            s.finish()[0].angle
        };
        assert!((tap(1.0, false, 1) + FRAC_PI_2).abs() < 1e-5, "{}", tap(1.0, false, 1));
        // whatever the stroke's random turn
        let off = |a: f32| (a + FRAC_PI_2).sin().abs();
        assert!(off(tap(1.0, true, 1)) < 1e-4, "{}", tap(1.0, true, 1));
        assert!(off(tap(1.0, true, 9)) < 1e-4, "{}", tap(1.0, true, 9));
        // half of it: half of three eighths of a turn
        assert!((tap(0.5, false, 1) + 0.5 * 1.5 * FRAC_PI_4).abs() < 1e-5, "{}", tap(0.5, false, 1));
        // none, and turned the other way, don't turn it from where it was
        assert_eq!(tap(0.0, false, 1), 0.0);
        assert_eq!(tap(-1.0, false, 1), 0.0);
        // a stroke's first stamp turns to the way the stroke goes, not from that
        let mut b = plain();
        b.set_setting("shape_rotation", 1.0);
        let mut s = StrokeBuilder::new(b, 10.0, Pointer::Finger);
        s.add(&line(10, 100.0));
        assert!(s.finish()[0].angle.abs() < 1e-4);
    }

    #[test]
    fn a_stamp_s_colour_jitter_goes_from_the_colour_the_stroke_s_left() {
        // the stroke darkens a colour of brightness 0.8 to 0.4; a stamp lightening by half goes half
        // way from there to white, darkening by half half way to black
        let stroke = Tint { brightness: [0.5, 0.0], ..Tint::NONE };
        let lighter = Tint { brightness: [0.5, 0.5], ..Tint::NONE };
        let darker = Tint { brightness: [0.5, 0.0], ..Tint::NONE };
        let at = |t: Tint, v: f32| v * t.brightness[0] + t.brightness[1];
        assert!((at(stroke.then(lighter), 0.8) - 0.7).abs() < 1e-6);
        assert!((at(stroke.then(darker), 0.8) - 0.2).abs() < 1e-6);
        // and the saturation likewise, towards where the stamp's jitter draws it
        let stroke = Tint { saturation: [0.5, 0.5], ..Tint::NONE };
        let stamp = Tint { saturation: [0.5, 0.1], ..Tint::NONE };
        let both = stroke.then(stamp);
        assert!((0.2 * both.saturation[0] + both.saturation[1] - ((0.2 * 0.5 + 0.5) * 0.5 + 0.1)).abs() < 1e-6);
        assert_eq!(Tint::NONE.then(Tint::NONE), Tint::NONE);
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
    fn only_a_round_tip_stops_at_a_least_size() {
        let least = |image: Option<u32>| {
            let mut brush = plain();
            brush.shape.image = image;
            let light: Vec<Sample> = line(20, 200.0).iter().map(|s| Sample { pressure: 0.02, ..*s }).collect();
            let stamps = StrokeBuilder::new(brush, 1.0, Pointer::Stylus).add(&light);
            stamps.iter().map(|s| s.radius).fold(f32::MAX, f32::min)
        };
        assert_eq!(least(None), 0.25);
        assert!(least(Some(0)) < 0.1, "{}", least(Some(0)));
    }

    #[test]
    fn stamps_are_evenly_spaced() {
        let mut b = StrokeBuilder::new(plain(), 10.0, Pointer::Stylus);
        b.add(&line(37, 100.0));
        let all = b.finish();
        // spacing 0.1 of the round tip's square (10 px is 0.797 of it), half a pixel, and a little
        // with the spacing: about 1.8 px, over 100 px, plus the first stamp
        let square = 10.0 / crate::brushfile::ROUND_TIP_SHARE * SPACED_SQUARE;
        let gap = 0.1 * square + 0.5 + (0.1 - SPACING_FLOOR) / (SPACING_SPAN / 2.0);
        assert_eq!(all.len(), (100.0 / gap) as usize + 1);
        for w in all.windows(2) {
            let d = w[0].center.distance(w[1].center);
            assert!((d - gap).abs() < 1e-3, "gap {d}");
        }
        assert!(b.bounds().x0 <= -5 && b.bounds().x1 >= 105);
    }

    #[test]
    fn a_flat_stamp_lies_as_close_as_its_shorter_side_says() {
        let gap_of = |brush: Brush, sample: &dyn Fn(usize) -> Sample| {
            let mut b = StrokeBuilder::new(brush, 40.0, Pointer::Stylus);
            b.add(&(0..=100).map(sample).collect::<Vec<_>>());
            let all = b.finish();
            let mid = all.len() / 2;
            all[mid].center.distance(all[mid + 1].center)
        };
        let along = |i: usize| Sample::at(Vec2::new(i as f32 * 4.0, 50.0), 1.0, i as f64 * 4.0);
        let square = 40.0 / crate::brushfile::ROUND_TIP_SHARE * SPACED_SQUARE;
        let extra = 0.5 + (0.1 - SPACING_FLOOR) / (SPACING_SPAN / 2.0);
        let round = gap_of(plain(), &along);
        assert!((round - (0.1 * square + extra)).abs() < 1e-3, "{round}");
        // a quarter as tall: a quarter of the size's part of the gap
        let flat = Brush { shape: Shape { roundness: 0.25, ..Shape::default() }, ..plain() };
        let close = gap_of(flat, &along);
        assert!((close - (0.025 * square + extra)).abs() < 1e-3, "{close}");
        // light pressure squashing it brings the stamps closer too
        let mut squashed = Brush { pressure_size: 0.0, ..plain() };
        squashed.set_setting("pressure_roundness", 1.0);
        let light = |i: usize| Sample { pressure: 0.5, ..along(i) };
        let pressed = gap_of(squashed, &light);
        assert!((pressed - (0.05 * square + extra)).abs() < 1e-3, "{pressed}");
        // and a lean that flattens it
        let mut leaning = plain();
        leaning.set_setting("tilt_roundness", 0.5);
        let leant = |i: usize| Sample { tilt: 1.2, ..along(i) };
        let upright = gap_of(leaning.clone(), &along);
        assert!(gap_of(leaning, &leant) < upright * 0.95, "{upright}");
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
    fn a_speed_amount_below_zero_thins_slow_input() {
        // above 0 faster input takes away; below, slower input does
        assert_eq!(by_speed(0.5, 0.0), 1.0);
        assert_eq!(by_speed(0.5, 1.0), 0.5);
        assert_eq!(by_speed(-0.5, 0.0), 0.5);
        assert_eq!(by_speed(-0.5, 1.0), 1.0);
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
        // a brush that thins as it is pressed: a finger leaves it whole too
        let thinning = Brush { pressure_size: -0.75, pressure_opacity: -0.5, ..plain() };
        let mut pen = StrokeBuilder::new(thinning.clone(), 10.0, Pointer::Stylus);
        let pressed = pen.add(&[Sample::at(Vec2::new(0.0, 0.0), 1.0, 0.0)]);
        let mut finger = StrokeBuilder::new(thinning, 10.0, Pointer::Finger);
        let touch = finger.add(&samples);
        assert!(pressed[0].radius < touch[0].radius && pressed[0].alpha < touch[0].alpha);
        assert_eq!(touch[0].radius, 5.0);
    }

    #[test]
    fn pressure_and_speed_take_the_size_down_to_the_brush_s_least() {
        let brush = Brush { pressure_size: 1.0, min_size: 6.0, ..plain() };
        let radius = |brush: &Brush, pressure: f32| {
            let mut stroke = StrokeBuilder::new(brush.clone(), 26.0, Pointer::Stylus);
            stroke.add(&[Sample::at(Vec2::new(0.0, 0.0), pressure, 0.0)])[0].radius
        };
        // no pressure leaves the least, half of it half way from there to the size
        assert!((radius(&brush, 0.0) - 3.0).abs() < 1e-4);
        assert!((radius(&brush, 0.5) - 8.0).abs() < 1e-4);
        assert!((radius(&brush, 1.0) - 13.0).abs() < 1e-4);
        // a size at the least doesn't follow the pressure at all
        let thin = |pressure: f32| {
            let mut stroke = StrokeBuilder::new(brush.clone(), 6.0, Pointer::Stylus);
            stroke.add(&[Sample::at(Vec2::new(0.0, 0.0), pressure, 0.0)])[0].radius
        };
        assert_eq!(thin(0.1), thin(1.0));
        // the least goes with the brush's scale on the canvas
        let mut scaled = StrokeBuilder::new(brush.clone(), 52.0, Pointer::Stylus).scaled(2.0);
        assert!((scaled.add(&[Sample::at(Vec2::ZERO, 0.0, 0.0)])[0].radius - 6.0).abs() < 1e-4);
        // the shape's height goes there too: a squashed stamp fills out as the least holds it up
        let flat = Brush { shape: Shape { roundness: 0.5, ..Shape::default() }, ..brush.clone() };
        let round = |pressure: f32| {
            let mut stroke = StrokeBuilder::new(flat.clone(), 26.0, Pointer::Stylus);
            stroke.add(&[Sample::at(Vec2::ZERO, pressure, 0.0)])[0].roundness
        };
        assert!((round(1.0) - 16.0 / 26.0).abs() < 1e-4, "{}", round(1.0));
        assert!((round(0.0) - 1.0).abs() < 1e-4, "{}", round(0.0));
        // a pen's pressure faded into a taper thins it as far as the least too
        let mut faded = Brush { pen_taper: Taper { start: 0.0, end: 0.0, size: 0.0, opacity: 0.0 }, ..brush };
        faded.pen_taper.start = 1.0;
        faded.set_setting("pen_taper_pressure", 1.0);
        let mut stroke = StrokeBuilder::new(faded, 26.0, Pointer::Stylus);
        let samples: Vec<Sample> =
            (0..40).map(|i| Sample::at(Vec2::new(i as f32 * 10.0, 0.0), 1.0, f64::from(i) * 8.0)).collect();
        stroke.add(&samples);
        let all = stroke.finish();
        assert!((all[0].radius - 3.0).abs() < 1e-3, "{}", all[0].radius);
        assert!((all.last().unwrap().radius - 13.0).abs() < 1e-3);
    }

    #[test]
    fn streamline_is_strong_from_the_first_and_none_at_none() {
        assert_eq!(streamline_keep(0.0), 0.0);
        assert!((streamline_keep(0.5) - 0.8803).abs() < 1e-3);
        assert!((streamline_keep(1.0) - 0.96).abs() < 1e-6);
        // a corner taken at a steady 600 a second rounds more the more there is, a good way off it
        // at the top
        let cut = |streamline: f32| {
            let brush = Brush { streamline, ..plain() };
            let mut b = StrokeBuilder::new(brush, 4.0, Pointer::Finger);
            let n = 60;
            let at = |x: f32, y: f32, i: usize| Sample::at(Vec2::new(x, y), 1.0, i as f64 * 1000.0 / 120.0);
            let mut samples: Vec<Sample> = (0..=n).map(|i| at(i as f32 * 5.0, 0.0, i)).collect();
            samples.extend((1..=n).map(|i| at(300.0, i as f32 * 5.0, n + i)));
            b.add(&samples);
            let corner = Vec2::new(300.0, 0.0);
            b.finish().iter().map(|s| s.center.distance(corner)).fold(f32::MAX, f32::min)
        };
        assert!(cut(0.0) < 1.0);
        assert!(cut(0.6) > 15.0 && cut(0.6) < 35.0, "{}", cut(0.6));
        assert!(cut(1.0) > 45.0, "{}", cut(1.0));
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
    fn the_stroke_shown_curves_in_to_the_input_as_it_will_be_kept() {
        let brush = Brush { streamline: 0.9, ..plain() };
        // along, then turning sharply away: the path falls well behind the input
        let mut samples = line(40, 200.0);
        let corner = samples.last().unwrap().pos;
        samples.extend((1..=10).map(|i| {
            let pos = corner + Vec2::new(0.0, i as f32 * 8.0);
            Sample::at(pos, 1.0, 160.0 + f64::from(i) * 4.0)
        }));
        let input = samples.last().unwrap().pos;
        let mut b = StrokeBuilder::new(brush, 4.0, Pointer::Finger);
        let kept = b.add(&samples);
        let (count, length) = (b.stamp_count(), b.length());
        assert!(kept.last().unwrap().center.distance(input) > 20.0, "the path lags");
        let ahead = b.preview_to_input(&[]);
        // to the input, as near as the stamps' spacing goes
        let reached = ahead.last().unwrap().center;
        assert!(reached.distance(input) < 2.0, "{reached:?}");
        // the stroke itself left as it was
        assert_eq!((b.stamp_count(), b.length()), (count, length));
        // and what shows is what lifting there would leave
        let kept_at_lift = b.clone().finish();
        let shown: Vec<Vec2> = kept.iter().chain(&ahead).map(|s| s.center).collect();
        assert!(shown.len().abs_diff(kept_at_lift.len()) <= 1, "{} and {}", shown.len(), kept_at_lift.len());
        for (a, b) in shown.iter().zip(&kept_at_lift) {
            assert!(a.distance(b.center) < 1.0, "{a:?} and {:?}", b.center);
        }
        // with nothing to catch up, nothing more is shown
        let mut plain_stroke = StrokeBuilder::new(plain(), 4.0, Pointer::Finger);
        plain_stroke.add(&line(10, 50.0));
        let end = plain_stroke.preview_to_input(&[]).last().map(|s| s.center);
        assert!(end.is_none_or(|e| e.distance(Vec2::new(50.0, 50.0)) < 2.0), "{end:?}");
    }

    #[test]
    fn motion_filtering_keeps_the_stroke_on_the_pen_only_with_the_tip_attached() {
        let along: Vec<Sample> =
            (0..200).map(|i| Sample::at(Vec2::new(i as f32 * 4.0, 50.0), 1.0, i as f64 * 8.0)).collect();
        let pen = along.last().unwrap().pos;
        let stroke = |tip_attached: bool| {
            let input = InputSettings { motion_filtering: 0.3, tip_attached, ..InputSettings::default() };
            let mut b = StrokeBuilder::new(plain(), 4.0, Pointer::Finger).with_input(input);
            b.add(&along);
            let shown = b.preview_to_input(&[]);
            (shown.last().map(|s| s.center), b.finish().last().map(|s| s.center))
        };
        let (shown, kept) = stroke(true);
        assert!(shown.is_some_and(|s| s.distance(pen) < 2.0) && kept.is_some_and(|k| k.distance(pen) < 2.0));
        let (shown, kept) = stroke(false);
        assert!(shown.is_some_and(|s| s.distance(pen) > 20.0), "{shown:?}");
        assert!(kept.is_some_and(|k| k.distance(pen) > 20.0), "{kept:?}");
        // without a filter it has nothing to do
        let input = InputSettings { tip_attached: false, ..InputSettings::default() };
        let mut b = StrokeBuilder::new(plain(), 4.0, Pointer::Finger).with_input(input);
        b.add(&along);
        assert!(b.finish().last().is_some_and(|s| s.center.distance(pen) < 2.0));
    }

    /// Each point of `input` followed by the path, the newest of `window` followed more and more:
    /// what a stroke of it that ends there looks like, point by point.
    fn streamlined(input: &[Vec2], amount: f32, window: usize) -> Vec<Vec2> {
        let strength = 1.0 - streamline_keep(amount);
        let mut at = input[0];
        input
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let left = input.len() - i;
                let near = (window as f32 - left as f32).max(0.0) / (window - 1) as f32;
                let near = near.powf(1.0 - strength);
                at = at.lerp(*p, strength + near * (near - strength));
                at
            })
            .collect()
    }

    #[test]
    fn streamline_comes_up_to_the_input_over_its_window() {
        // a finger's points at their own rate, turning: the window is 6 of them, a pen's 12
        for (pointer, ms, window) in [(Pointer::Finger, 1000.0 / 120.0, 6), (Pointer::Stylus, 1000.0 / 240.0, 12)] {
            let curve: Vec<Vec2> = (0..200)
                .map(|i| {
                    let t = i as f32 * 0.03 * ms as f32 / 8.0;
                    Vec2::new(300.0 + 200.0 * t.cos(), 300.0 + 200.0 * t.sin())
                })
                .collect();
            let samples: Vec<Sample> =
                curve.iter().enumerate().map(|(i, p)| Sample::at(*p, 1.0, i as f64 * ms)).collect();
            let brush = Brush { streamline: 0.66, ..plain() };
            let mut b = StrokeBuilder::new(brush, 2.0, pointer);
            b.add(&samples);
            let stamps = b.finish();
            // the last stretch of the stroke lies on those points, and ends on the input's last
            let wanted = streamlined(&curve, 0.66, window);
            for p in wanted.iter().rev().take(window + 20) {
                let off = stamps.iter().map(|s| s.center.distance(*p)).fold(f32::MAX, f32::min);
                assert!(off < 1.5, "{pointer:?}: {p:?} is {off} from the stroke");
            }
            let end = stamps.last().unwrap().center;
            assert!(end.distance(*curve.last().unwrap()) < 2.0, "{pointer:?}: {end:?}");
        }
    }

    #[test]
    fn streamline_comes_up_to_the_input_alike_at_any_rate() {
        let at = |t: f32| Vec2::new(300.0 + 200.0 * t.cos(), 300.0 + 200.0 * t.sin());
        // the same curve in the same time at twice and half a finger's rate
        let ends = [60.0, 120.0, 240.0].map(|rate: f64| {
            let count = (rate * 0.8) as usize;
            let samples: Vec<Sample> = (0..=count)
                .map(|i| Sample::at(at(i as f32 / count as f32 * 3.0), 1.0, i as f64 * 1000.0 / rate))
                .collect();
            let mut b = StrokeBuilder::new(Brush { streamline: 0.66, ..plain() }, 2.0, Pointer::Finger);
            b.add(&samples);
            b.finish()
        });
        // 100 ms before the lift the three lie together, whatever the rate
        let at_share = |stamps: &Vec<Stamp>| stamps[stamps.len() * 7 / 8].center;
        let [a, b, c] = [at_share(&ends[0]), at_share(&ends[1]), at_share(&ends[2])];
        assert!(a.distance(b) < 12.0 && b.distance(c) < 12.0, "{a:?} {b:?} {c:?}");
    }

    /// Three quarters of a circle `radius` across, drawn in `ms` as `count` samples after the
    /// first, handed over `batch` at a time each batch timed as one (a hundredth of a millisecond
    /// apart, as a desktop stamps a pointer history as it hands it on).
    fn c_curve(radius: f32, count: usize, ms: f64, batch: usize) -> Vec<Sample> {
        let step = ms / count as f64;
        (0..=count)
            .map(|i| {
                let a = 0.25 * PI as f32 + 1.5 * PI as f32 * i as f32 / count as f32;
                let at = Vec2::new(300.0 + radius * a.cos(), 300.0 + radius * a.sin());
                let time = (i / batch) as f64 * step * batch as f64 + 0.01 * (i % batch) as f64;
                Sample::at(at, 1.0, time)
            })
            .collect()
    }

    /// The largest turn (degrees) between neighbouring stretches of the stamps' path, taken in
    /// stretches of at least `least` canvas pixels.
    fn sharpest_turn(stamps: &[Stamp], least: f32) -> f32 {
        let mut path: Vec<Vec2> = Vec::new();
        for s in stamps {
            if path.last().is_none_or(|l| l.distance(s.center) >= least) {
                path.push(s.center);
            }
        }
        path.windows(3)
            .map(|w| {
                let (a, b) = (w[1] - w[0], w[2] - w[1]);
                (a.x * b.y - a.y * b.x).atan2(a.x * b.x + a.y * b.y).abs().to_degrees()
            })
            .fold(0.0, f32::max)
    }

    /// `samples` drawn `batch` at a time, as a host hands them on.
    fn drawn_in_batches(brush: Brush, pointer: Pointer, samples: &[Sample], batch: usize) -> Vec<Stamp> {
        let mut b = StrokeBuilder::new(brush, 6.0, pointer);
        for some in samples.chunks(batch) {
            b.add(some);
        }
        b.finish()
    }

    #[test]
    fn a_fast_stroke_curves_through_its_samples_with_no_corners() {
        // a C drawn in 150 ms: 9 samples (a mouse at 60 Hz), and a pen's 36 handed over 4 a frame
        for (pointer, count, batch) in [(Pointer::Mouse, 9, 1), (Pointer::Finger, 9, 1), (Pointer::Stylus, 36, 4)] {
            let samples = c_curve(100.0, count, 150.0, batch);
            let stamps = drawn_in_batches(plain(), pointer, &samples, batch);
            // a 3 px stretch of the circle turns 1.7 degrees: the path bends no sharper than about that
            let turn = sharpest_turn(&stamps, 3.0);
            assert!(turn < 4.0, "{pointer:?}: turns {turn} degrees at once");
            // and goes through every sample
            for s in &samples {
                let off = stamps.iter().map(|t| t.center.distance(s.pos)).fold(f32::MAX, f32::min);
                assert!(off < 1.0, "{pointer:?}: {:?} is {off} from the stroke", s.pos);
            }
        }
    }

    #[test]
    fn a_streamlined_stroke_handed_over_in_batches_bends_smoothly() {
        // a pen's samples at 240 Hz, four a frame each frame timed as one: timed as they came, all
        // but the first of each would hardly move the path, so it went on a frame at a time, turning
        // 19 to 31 degrees at each
        for streamline in [0.1, 0.35, 0.6] {
            let brush = Brush { streamline, ..plain() };
            let samples = c_curve(100.0, 36, 150.0, 4);
            let turn = sharpest_turn(&drawn_in_batches(brush.clone(), Pointer::Stylus, &samples, 4), 3.0);
            let even = sharpest_turn(&drawn_in_batches(brush, Pointer::Stylus, &c_curve(100.0, 36, 150.0, 1), 1), 3.0);
            assert!(
                turn < 7.0 && turn < even * 1.6,
                "{streamline}: turns {turn} degrees at once ({even} timed evenly)"
            );
        }
    }

    #[test]
    fn streamline_takes_samples_handed_over_together_as_if_each_came_in_its_turn() {
        // the per-sample model at the tuned rates (as `streamline_comes_up_to_the_input_over_its_window`),
        // the samples coming four at a time with one time each
        for (pointer, ms, window) in [(Pointer::Finger, 1000.0 / 120.0, 6), (Pointer::Stylus, 1000.0 / 240.0, 12)] {
            let curve: Vec<Vec2> = (0..160)
                .map(|i| {
                    let t = i as f32 * 0.03 * ms as f32 / 8.0;
                    Vec2::new(300.0 + 200.0 * t.cos(), 300.0 + 200.0 * t.sin())
                })
                .collect();
            let samples: Vec<Sample> = curve
                .iter()
                .enumerate()
                .map(|(i, p)| Sample::at(*p, 1.0, (i / 4) as f64 * 4.0 * ms + 0.01 * (i % 4) as f64))
                .collect();
            let stamps = drawn_in_batches(Brush { streamline: 0.66, ..plain() }, pointer, &samples, 4);
            let wanted = streamlined(&curve, 0.66, window);
            for p in wanted.iter().skip(8) {
                let off = stamps.iter().map(|s| s.center.distance(*p)).fold(f32::MAX, f32::min);
                assert!(off < 1.5, "{pointer:?}: {p:?} is {off} from the stroke");
            }
        }
    }

    #[test]
    fn with_no_smoothing_the_stroke_shows_and_ends_where_the_input_is() {
        // a quick pen stroke at 120 Hz, no StreamLine, stabilisation or motion filtering
        let samples = c_curve(100.0, 18, 150.0, 1);
        let mut b = StrokeBuilder::new(plain(), 6.0, Pointer::Stylus);
        for s in &samples {
            b.add(std::slice::from_ref(s));
            // what shows reaches the newest sample
            let shown = b.preview_to_input(&[]);
            let end = shown.last().map_or_else(|| b.stamps().last().unwrap().center, |t| t.center);
            assert!(end.distance(s.pos) < 2.0, "shown up to {end:?}, the input at {:?}", s.pos);
        }
        // and the stroke ends on the last (a sample's step is 26 px here: within a stamp's gap)
        let end = b.finish().last().unwrap().center;
        assert!(end.distance(samples.last().unwrap().pos) < 2.0, "{end:?}");
    }

    #[test]
    fn a_pens_tip_reaches_the_input_past_the_smoothed_path() {
        let taper = Taper { start: 0.0, end: 0.5, size: 1.0, opacity: 0.0 };
        let brush = Brush { pen_taper: taper, streamline: 0.9, ..plain() };
        let samples = line(50, 300.0);
        let input = samples.last().unwrap().pos;
        let mut b = StrokeBuilder::new(brush, 4.0, Pointer::Stylus);
        assert!(b.shows_tip());
        let (_, tip) = b.add_tipped(&samples);
        let end = tip.last().unwrap();
        assert!(end.center.distance(input) < 2.0 && end.radius < 0.5, "{:?} {}", end.center, end.radius);
    }

    #[test]
    fn a_fingers_tapers_show_as_it_is_drawn_and_what_is_kept_stays_at_the_end() {
        let taper = Taper { start: 0.8, end: 0.5, size: 1.0, opacity: 0.0 };
        let brush = Brush { touch_taper: taper, streamline: 0.3, ..plain() };
        // 4 px every 8 ms along, then lifted
        let samples: Vec<Sample> =
            (0..=80).map(|i| Sample::at(Vec2::new(20.0 + i as f32 * 4.0, 50.0), 1.0, i as f64 * 8.0)).collect();
        let mut b = StrokeBuilder::new(brush, 20.0, Pointer::Finger);
        assert!(b.shows_tip());
        let mut kept = Vec::new();
        let mut first_tip = None;
        for (k, chunk) in samples.chunks(2).enumerate() {
            let (more, tip) = b.add_tipped(chunk);
            // within the start taper's time (67 ms) nothing is drawn for good, and the tip shows
            // the start tapered
            if k < 4 {
                assert!(more.is_empty(), "kept at {k}");
            }
            if k == 3 {
                first_tip = Some(tip.clone());
            }
            kept.extend(more);
            // the head thinned as it goes
            if k > 12 {
                let head = tip.last().unwrap();
                assert!(head.radius < 3.0, "head {} at {k}", head.radius);
            }
        }
        let first_tip = first_tip.unwrap();
        assert!(first_tip[0].radius < 3.0 && first_tip.last().unwrap().radius < 3.0, "{first_tip:?}");
        assert!(kept.len() > 20, "{}", kept.len());
        // what was drawn for good is the stroke as it ends
        let all = b.finish();
        for (i, (k, a)) in kept.iter().zip(&all).enumerate() {
            assert!(k.center.distance(a.center) < 1e-3 && (k.radius - a.radius).abs() < 1e-3, "{i}: {k:?} {a:?}");
        }
        assert!(all[0].radius < 2.0 && all.last().unwrap().radius < 2.0);
    }

    #[test]
    fn a_taper_s_end_shows_as_it_is_drawn_only_with_the_tip_animation_on() {
        let taper = Taper { start: 0.0, end: 0.5, size: 1.0, opacity: 0.0 };
        let shows = |pointer: Pointer, animated: f32| {
            let mut brush = Brush { touch_taper: taper.clone(), pen_taper: taper.clone(), ..plain() };
            brush.set_setting("pen_tip_animation", animated);
            StrokeBuilder::new(brush, 20.0, pointer).shows_tip()
        };
        assert!(shows(Pointer::Finger, 1.0) && shows(Pointer::Stylus, 1.0));
        assert!(!shows(Pointer::Finger, 0.0) && !shows(Pointer::Stylus, 0.0));
    }

    #[test]
    fn a_start_taper_reaches_what_the_input_covered_though_the_path_waits_a_point_behind() {
        let taper = Taper { start: 0.5, end: 0.0, size: 1.0, opacity: 0.0 };
        let brush = Brush { touch_taper: taper, ..plain() };
        // the first move comes after the taper's 42 ms, 60 px on: the path hasn't walked any of it
        let mut samples =
            vec![Sample::at(Vec2::new(0.0, 50.0), 1.0, 0.0), Sample::at(Vec2::new(60.0, 50.0), 1.0, 100.0)];
        samples.extend(
            (1..40).map(|i| Sample::at(Vec2::new(60.0 + i as f32 * 10.0, 50.0), 1.0, 100.0 + f64::from(i) * 8.0)),
        );
        let mut b = StrokeBuilder::new(brush, 20.0, Pointer::Finger);
        b.add(&samples);
        let all = b.finish();
        let at = |x: f32| {
            all.iter().min_by(|a, b| (a.center.x - x).abs().total_cmp(&(b.center.x - x).abs())).unwrap().radius
        };
        assert!(at(5.0) < 3.0, "{}", at(5.0));
        assert!((at(30.0) - 5.0).abs() < 1.0, "{}", at(30.0));
        assert_eq!(at(80.0), 10.0);
    }

    #[test]
    fn held_still_then_lifted_the_end_keeps_the_taper_it_showed() {
        let taper = Taper { start: 0.0, end: 0.5, size: 1.0, opacity: 0.0 };
        let brush = Brush { touch_taper: taper, ..plain() };
        let samples: Vec<Sample> =
            (0..=60).map(|i| Sample::at(Vec2::new(i as f32 * 10.0, 50.0), 1.0, i as f64 * 8.0)).collect();
        let radii = |lift_ms: Option<f64>| {
            let mut b = StrokeBuilder::new(brush.clone(), 20.0, Pointer::Finger);
            let (_, tip) = b.add_tipped(&samples);
            // the lift, where it last was, after the wait
            if let Some(ms) = lift_ms {
                b.add(&[Sample::at(Vec2::new(600.0, 50.0), 1.0, 480.0 + ms)]);
            }
            let all = b.finish();
            (tip.last().unwrap().radius, all.iter().rev().take(6).map(|s| s.radius).collect::<Vec<f32>>())
        };
        let (shown, straight_away) = radii(None);
        let (_, after_a_hold) = radii(Some(1500.0));
        assert!(shown < 3.0, "{shown}");
        assert!(straight_away[0] < 3.0, "{straight_away:?}");
        assert_eq!(straight_away, after_a_hold);
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
    fn a_shape_s_line_is_smoothed_by_stabilisation_but_not_streamline() {
        // a wavy 600 px line, 3 px either way every 20 px
        let wave: Vec<Sample> = (0..=300)
            .map(|i| {
                let x = i as f32 * 2.0;
                Sample::at(Vec2::new(x, 50.0 + 3.0 * (x / 20.0 * std::f32::consts::TAU).sin()), 1.0, i as f64 * 4.0)
            })
            .collect();
        let line = |brush: Brush| StrokeBuilder::new(brush, 4.0, Pointer::Stylus).shape_line(&wave);
        let spread = |line: &[Vec2]| {
            let middle = line.iter().filter(|p| (100.0..500.0).contains(&p.x));
            middle.fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)))
        };
        let bare = line(plain());
        let (lo, hi) = spread(&bare);
        assert!(hi - lo > 5.0, "the wave is there: {lo}..{hi}");
        // streamline leaves it as it is
        assert_eq!(line(Brush { streamline: 0.9, ..plain() }), bare);
        // stabilisation evens it out
        let mut steady = plain();
        steady.set_setting("stabilization", 1.0);
        let (lo, hi) = spread(&line(steady));
        assert!(hi - lo < 2.0, "stabilised: {lo}..{hi}");
        // a classic end taper runs nothing on past the lift
        let mut classic = Brush { pen_taper: Taper { start: 0.0, end: 1.0, size: 1.0, opacity: 0.0 }, ..plain() };
        classic.set_setting("classic_taper", 1.0);
        let end = line(classic).iter().map(|p| p.x).fold(f32::MIN, f32::max);
        assert!(end <= 600.5, "ran on to {end}");
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
    fn a_tip_worked_out_on_what_is_open_is_the_whole_strokes() {
        let taper = Taper { start: 0.2, end: 0.5, size: 1.0, opacity: 0.3 };
        let brush = Brush { pen_taper: taper.clone(), touch_taper: taper, streamline: 0.4, ..plain() };
        let samples: Vec<Sample> = (0..=600)
            .map(|i| {
                let t = i as f32 * 0.05;
                let pos = Vec2::new(i as f32 * 1.5, 50.0 + 30.0 * t.sin());
                Sample::at(pos, 0.4 + 0.5 * (t * 0.7).cos().abs(), i as f64 * 4.0)
            })
            .collect();
        let mut b = StrokeBuilder::new(brush, 10.0, Pointer::Stylus);
        assert!(b.shows_tip());
        // as the whole stroke copied works them out
        let whole_tip = |b: &StrokeBuilder, ahead: &[Sample]| {
            let mut copy = b.clone();
            copy.add(ahead);
            copy.reach_input(&mut Vec::new());
            copy.tip_from(b.kept)
        };
        let mut checked = 0;
        for (k, chunk) in samples.chunks(3).enumerate() {
            let ahead = [Sample::at(chunk[0].pos + Vec2::new(6.0, 2.0), 0.5, chunk[0].time_ms + 8.0)];
            let (count, kept_before) = (b.stamp_count(), b.kept);
            assert_eq!(b.preview_tip(&ahead), whole_tip(&b, &ahead), "predicted at {k}");
            let mut whole = b.clone();
            let mut ahead_stamps = whole.add(&ahead);
            whole.stream_on(&mut ahead_stamps);
            whole.flush(&mut ahead_stamps);
            assert_eq!(b.preview(&ahead), ahead_stamps, "previewed at {k}");
            whole.walk_to(whole.input_point().unwrap(), &mut ahead_stamps);
            assert_eq!(b.preview_to_input(&ahead), ahead_stamps, "previewed to the input at {k}");
            // the stroke left as it was
            assert_eq!((b.stamp_count(), b.kept), (count, kept_before));
            let (_, tip) = b.add_tipped(chunk);
            assert_eq!(tip, whole_tip(&b, &[]), "tip at {k}");
            checked += tip.len();
        }
        assert!(checked > 1000 && b.kept > 100, "{checked} {}", b.kept);
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
        // the tip reaches the newest point, past the stamps placed for good (the path waits a point
        // to curve)
        assert!((tip.last().unwrap().center.x - 118.0).abs() < 3.0, "{}", tip.last().unwrap().center.x);
        assert!(kept.len() + tip.len() >= b.stamp_count());
        // drawn on, the old tip is kept whole and a new one shown
        let (more, tip) = b.add_tipped(samples.get(60..).unwrap());
        assert!(more.iter().all(|s| s.radius == 5.0) && more.first().unwrap().center.x < 118.0);
        assert!(kept.len() + more.len() + tip.len() >= b.stamp_count());
        // a predicted tip runs on to where the input is predicted to go, the stroke left as it is
        let ahead = b.preview_tip(&[Sample::at(Vec2::new(240.0, 50.0), 1.0, 420.0)]);
        assert!(ahead.last().unwrap().center.x > 230.0 && ahead.last().unwrap().radius < 1.0);
        assert!(kept.len() + more.len() + tip.len() >= b.stamp_count());
        // a finger's end shows tapered whatever the animation; a classic taper or the animation
        // off show no tip
        assert!(StrokeBuilder::new(brush.clone(), 10.0, Pointer::Finger).shows_tip());
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
    fn a_stroke_s_end_leaves_what_no_taper_reaches_as_it_was_drawn() {
        let ended = |taper: Taper, animated: f32| {
            let mut brush = plain();
            brush.pen_taper = taper;
            brush.set_setting("pen_tip_animation", animated);
            let mut stroke = StrokeBuilder::new(brush, 10.0, Pointer::Stylus);
            stroke.add_tipped(&line(40, 400.0));
            let drawn = stroke.drawn_for_good().unwrap();
            let all = stroke.finish();
            assert!(stroke.drawn_for_good().is_none(), "ended, what was drawn is no longer known");
            (drawn.len(), all.starts_with(&drawn), all.len())
        };
        let (drawn, same, all) = ended(Taper::NONE, 1.0);
        assert!(drawn > 0 && same && all >= drawn, "{drawn} {same} {all}");
        // an end taper shown as it is drawn is on the tip, past what was kept
        let thinning = Taper { end: 0.5, size: 1.0, ..Taper::NONE };
        let (drawn, same, all) = ended(thinning.clone(), 1.0);
        assert!(drawn > 0 && same && all > drawn, "{drawn} {same} {all}");
        // one that doesn't show thins what is there already
        let (drawn, same, _) = ended(thinning, 0.0);
        assert!(drawn > 0 && !same);
    }

    #[test]
    fn a_stamp_knows_how_far_along_the_path_it_is_wherever_jitter_threw_it() {
        let mut brush = plain();
        brush.set_setting("jitter_lateral", 1.0);
        let stamps = StrokeBuilder::new(brush, 10.0, Pointer::Stylus).add(&line(20, 200.0));
        let thrown: f32 = stamps.windows(2).map(|p| p[0].center.distance(p[1].center)).sum();
        let last = stamps.last().unwrap();
        assert!(thrown > 2.0 * last.distance, "{thrown} {}", last.distance);
        assert!(stamps.windows(2).all(|p| p[1].distance >= p[0].distance));
        assert!(last.distance <= 200.0, "{}", last.distance);
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
    fn a_still_tap_stays_a_tap_with_its_points_averaged() {
        let at = Vec2::new(1234.567, 987.654);
        for (setting, amount) in [("motion_filtering", 0.25), ("motion_filtering", 0.8), ("stabilization", 0.5)] {
            let mut brush = Brush { shape: Shape { count: 7, ..Shape::default() }, ..plain() };
            brush.set_setting(setting, amount);
            let mut tap = StrokeBuilder::new(brush, 10.0, Pointer::Stylus);
            tap.add(&[Sample::at(at, 1.0, 0.0), Sample::at(at, 1.0, 40.0), Sample::at(at, 1.0, 80.0)]);
            assert_eq!(tap.finish().len(), 1, "{setting} {amount}");
        }
    }

    #[test]
    fn a_tap_lays_one_stamp_whatever_the_brush_s_count() {
        let brush = Brush { shape: Shape { count: 7, ..Shape::default() }, ..plain() };
        let mut tap = StrokeBuilder::new(brush.clone(), 10.0, Pointer::Stylus);
        tap.add(&[Sample::at(Vec2::new(50.0, 50.0), 1.0, 0.0), Sample::at(Vec2::new(50.0, 50.0), 1.0, 80.0)]);
        assert_eq!(tap.finish().len(), 1);
        // a stroke lays all of each place's
        let mut stroke = StrokeBuilder::new(brush, 10.0, Pointer::Stylus);
        stroke.add(&line(10, 100.0));
        assert!(stroke.finish().len() >= 14);
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
        // switched on: an upright pen next to none, a leaning one more
        assert!(attack(1.0, 0.0, true, Pointer::Stylus) < 0.01);
        let flat = Tilt::reach(crate::brush::FLAT_TILT, 0.0);
        assert!((attack(1.0, crate::brush::FLAT_TILT, true, Pointer::Stylus) - flat).abs() < 1e-3);
        // a finger as a flat pen, at full pressure
        let finger = Tilt::reach(std::f32::consts::FRAC_PI_2, 0.0);
        assert!((attack(1.0, 0.0, true, Pointer::Finger) - finger).abs() < 1e-3);
    }

    #[test]
    fn a_finger_s_wet_attack_follows_the_opacity_slider() {
        let attack = |pointer: Pointer| {
            let b = Brush { pressure_size: 0.0, touch_opacity: 0.5, ..plain() };
            StrokeBuilder::new(b, 10.0, pointer).preview(&[Sample::at(Vec2::new(10.0, 10.0), 1.0, 0.0)])[0].attack
        };
        assert!((attack(Pointer::Finger) - 0.5).abs() < 1e-4);
        // a pen's follows its pressure instead
        assert!((attack(Pointer::Stylus) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_finger_s_wet_mix_takes_full_pressure_whatever_the_sample_says() {
        let wet = |pointer: Pointer, pressure: f32| {
            let b = Brush { pressure_size: 0.0, ..plain() };
            StrokeBuilder::new(b, 10.0, pointer).preview(&[Sample::at(Vec2::new(10.0, 10.0), pressure, 0.0)])[0].wet
        };
        // without curves the dilution and the pull follow the pressure: a finger has none, so full
        let finger = wet(Pointer::Finger, 0.3);
        assert!((finger[5] - 1.0).abs() < 1e-4 && (finger[6] - 1.0).abs() < 1e-4, "{finger:?}");
        // a pen takes them at its own
        let pen = wet(Pointer::Stylus, 0.5);
        assert!((pen[5] - pen[6]).abs() < 1e-4 && pen[6] < 0.9, "{pen:?}");
    }

    #[test]
    fn a_leaning_pen_adds_its_tilt_bleed_in_full_whatever_the_pressure() {
        let bleed = |tilt: f32| {
            let mut b = Brush { pressure_size: 0.0, ..plain() };
            b.set_setting("pressure_bleed", 1.0);
            b.set_setting("tilt_bleed", 1.0);
            let mut s = Sample::at(Vec2::new(10.0, 10.0), 0.3, 0.0);
            s.tilt = tilt;
            let stamp = StrokeBuilder::new(b, 10.0, Pointer::Stylus).preview(&[s])[0];
            (stamp.grain_bleed, stamp.grain_press)
        };
        // upright: the pressure bleed alone (a lean's least is 2^-10), through the pressure
        let (upright, press) = bleed(0.0);
        assert!((upright - 1.0).abs() < 2e-3 && (press - 0.3).abs() < 0.05, "{upright} {press}");
        // leaning flat: the tilt bleed adds to it, and counts whatever the pressure
        let (flat, press) = bleed(crate::brush::FLAT_TILT);
        let lean = flat - 1.0;
        assert!(lean > 0.5, "{flat}");
        assert!((press - (0.3 + lean) / (1.0 + lean)).abs() < 0.05, "{press} {lean}");
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
        // a finger has no lean to wait for, so the switches aren't there: all of it, and the pad
        // without the brush's tilt preview likewise
        let touched = |on_pad: bool| {
            let mut b = Brush { pressure_size: 0.0, ..plain() };
            b.shape.count = 8;
            b.set_setting("jitter_lateral", 1.0);
            b.set_setting("count_tilt", 1.0);
            b.set_setting("jitter_lateral_tilt", 1.0);
            b.set_setting("stamp_hue", 1.0);
            b.set_setting("stamp_hue_tilt", 1.0);
            let s = Sample::at(Vec2::new(50.0, 50.0), 1.0, 0.0);
            if on_pad {
                StrokeBuilder::new(b, 10.0, Pointer::Stylus).on_pad(false).preview(&[s])
            } else {
                StrokeBuilder::new(b, 10.0, Pointer::Finger).preview(&[s])
            }
        };
        for stamps in [touched(false), touched(true)] {
            assert_eq!(stamps.len(), 8);
            assert!(stamps.iter().any(|s| (s.center.y - 50.0).abs() > 0.5));
            assert!(stamps.iter().any(|s| s.tint.hue.abs() > 0.05));
        }
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
    fn a_taper_thins_what_the_stamp_is_drawn_at_and_a_lean_s_growth_stays() {
        let mut b = Brush { pen_taper: Taper { start: 0.0, end: 0.5, size: 1.0, opacity: 0.0 }, ..plain() };
        b.tilt.size = 0.1;
        b.set_setting("size_tilt_angle", 0.1);
        b.set_setting("tilt_size_compression", 0.0);
        let leaning = |i: usize| {
            let at = Sample::at(Vec2::new(i as f32 * 8.0, 50.0), 1.0, i as f64 * 8.0);
            Sample { tilt: crate::brush::FLAT_TILT, ..at }
        };
        let mut stroke = StrokeBuilder::new(b.clone(), 10.0, Pointer::Stylus);
        stroke.add(&(0..=60).map(leaning).collect::<Vec<_>>());
        let all = stroke.finish();
        let growth = 0.1 * Tilt::reach(crate::brush::FLAT_TILT, 0.1) * b.size_unit();
        let (middle, end) = (&all[all.len() / 2], all.last().unwrap());
        assert!((middle.radius - (10.0 + growth) / 2.0).abs() < 1e-3, "{}", middle.radius);
        // the end, thinned to nothing, is what the lean adds
        assert!((end.radius - growth / 2.0).abs() < 0.2, "{} {growth}", end.radius);
        // and its height stays by it: round where only the growth is left of a flat stamp
        let flat = Brush { shape: Shape { roundness: 0.2, ..Shape::default() }, ..b };
        let mut stroke = StrokeBuilder::new(flat, 10.0, Pointer::Stylus);
        stroke.add(&(0..=60).map(leaning).collect::<Vec<_>>());
        let all = stroke.finish();
        assert!(all[all.len() / 2].roundness < 0.9, "{}", all[all.len() / 2].roundness);
        assert!(all.last().unwrap().roundness > 0.97, "{}", all.last().unwrap().roundness);
    }

    #[test]
    fn a_taper_draws_the_jitter_in_with_the_size() {
        let taper = Taper { start: 1.0, end: 0.0, size: 1.0, opacity: 0.0 };
        let mut b = Brush { pen_taper: taper, ..plain() };
        b.set_setting("jitter_lateral", 1.0);
        let mut stroke = StrokeBuilder::new(b, 40.0, Pointer::Stylus);
        stroke.add(&line(100, 1200.0));
        let all = stroke.finish();
        // the first stamps, thinned almost to nothing, stay by the path; past the taper they don't
        let off = |s: &Stamp| (s.center.y - 50.0).abs();
        let tips = all.iter().take_while(|s| s.radius < 1.0).map(off).fold(0.0, f32::max);
        let whole = all.iter().rev().take(100).map(off).fold(0.0, f32::max);
        assert!(tips < 4.0, "{tips}");
        assert!(whole > 30.0, "{whole}");
    }

    #[test]
    fn a_wobble_as_the_pen_slows_doesnt_swing_stamps_that_follow_the_stroke() {
        let mut b = plain();
        b.set_setting("shape_rotation", 1.0);
        // along x, then a pen slowing to lift: steps of a fraction of a pixel, every which way
        let mut samples: Vec<Sample> =
            (0..=20).map(|i| Sample::at(Vec2::new(i as f32 * 10.0, 0.0), 1.0, f64::from(i) * 8.0)).collect();
        for (k, (dx, dy)) in
            [(0.3, 0.6), (-0.2, -0.7), (0.4, 0.5), (-0.5, 0.2), (0.1, -0.6), (0.2, 0.7)].iter().enumerate()
        {
            let x = 200.0 + dx * (k + 1) as f32 / 3.0;
            samples.push(Sample::at(Vec2::new(x, *dy), 1.0, 168.0 + k as f64 * 8.0));
        }
        let stamps = StrokeBuilder::new(b, 40.0, Pointer::Stylus).preview(&samples);
        let off = |a: f32| (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let worst = stamps.iter().skip(1).map(|s| off(s.angle).abs()).fold(0.0, f32::max);
        assert!(worst < 0.3, "a stamp turned {worst} radians off the stroke");
    }

    #[test]
    fn a_pen_turns_the_stamp_as_held_in_place_of_following_the_stroke() {
        let angle = |pointer: Pointer, relative: f32| {
            let mut b = plain();
            b.set_setting("shape_rotation", 1.0);
            b.set_setting("input_style", 1.0);
            b.set_setting("relative_to_stroke", relative);
            // down the screen (a quarter turn), the pen leaning 0.3 radians
            let at = |y: f32| Sample { azimuth: 0.3, ..Sample::at(Vec2::new(0.0, y), 1.0, f64::from(y)) };
            let stamps = StrokeBuilder::new(b, 10.0, pointer).preview(&[at(0.0), at(20.0), at(40.0)]);
            stamps.last().map_or(f32::NAN, |s| s.angle)
        };
        let quarter = std::f32::consts::FRAC_PI_2;
        // the pen as held; touch follows the stroke (and has no lean); relative to the stroke goes
        // by which barrel roll counts, so it changes neither
        assert!((angle(Pointer::Stylus, 0.0) - 0.3).abs() < 1e-4, "{}", angle(Pointer::Stylus, 0.0));
        assert!((angle(Pointer::Finger, 0.0) - quarter).abs() < 1e-4, "{}", angle(Pointer::Finger, 0.0));
        assert!((angle(Pointer::Stylus, 1.0) - 0.3).abs() < 1e-4, "{}", angle(Pointer::Stylus, 1.0));
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
        // jittered between its least and its depth (0.4 to 0.5: 0.8 to 1 of it), not floored
        let jittered = stroke(&[("grain_depth_jitter", 1.0), ("grain_depth_min", 0.4)], 1.0, Taper::NONE);
        assert!(jittered.iter().all(|s| s.grain_depth >= 0.8 - 1e-4));
        assert!(jittered.iter().any(|s| s.grain_depth > 0.81 && s.grain_depth < 0.99));
        // its tile: cropped, the same whatever the stamp's size; following the size, sized with
        // the stamp (a lighter, smaller stamp, a smaller grain)
        let tile = |zoom: f32, pressure: f32| {
            let mut b = plain();
            b.pressure_size = 1.0;
            b.set_setting("grain_zoom", zoom);
            // a file's scale of 1
            b.grain.scale = crate::studio::MAX_GRAIN_SCALE / 4.0;
            let scale = b.grain.scale;
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Stylus);
            s.add(&[Sample::at(Vec2::new(0.0, 0.0), pressure, 0.0)]);
            let stamp = s.finish()[0];
            (stamp.grain_size * scale, stamp.radius)
        };
        let ((cropped_light, _), (cropped_hard, _)) = (tile(1.0, 0.5), tile(1.0, 1.0));
        assert!((cropped_light - cropped_hard).abs() < 1e-3, "{cropped_light} {cropped_hard}");
        let ((light, light_r), (hard, hard_r)) = (tile(0.0, 0.5), tile(0.0, 1.0));
        assert!(light < hard * 0.8 && (light / hard - light_r / hard_r).abs() < 0.02, "{light} {hard}");
        // the stamp's square it goes by, which a round tip covers only part of
        let diagonal = 10.0 / crate::brushfile::ROUND_TIP_SHARE * std::f32::consts::SQRT_2;
        assert!((hard - diagonal).abs() < 1e-3, "{hard}");
        // a taper's thinning too
        let mut b = Brush { pressure_size: 0.0, pen_taper: Taper { start: 1.0, ..Taper::NONE }, ..plain() };
        b.pen_taper.size = 1.0;
        b.set_setting("grain_zoom", 0.0);
        b.grain.scale = crate::studio::MAX_GRAIN_SCALE / 4.0;
        let mut s = StrokeBuilder::new(b, 40.0, Pointer::Stylus);
        s.add(&line(100, 1200.0));
        let all = s.finish();
        let (thin, whole) = (all[2], all[all.len() - 1]);
        assert!(thin.radius < whole.radius * 0.2, "{} {}", thin.radius, whole.radius);
        assert!((thin.grain_size / whole.grain_size - thin.radius / whole.radius).abs() < 0.02);
        // the brush's own motion filtering smooths as the app's does
        let zigzag: Vec<Sample> = (0..60)
            .map(|i| Sample::at(Vec2::new(i as f32 * 2.0, if i % 2 == 0 { 0.0 } else { 20.0 }), 1.0, i as f64 * 8.0))
            .collect();
        let spread = |filtering: f32| {
            let mut b = plain();
            b.set_setting("motion_filtering", filtering);
            let mut s = StrokeBuilder::new(b, 4.0, Pointer::Stylus);
            s.add(&zigzag);
            let stamps = s.finish();
            let third = stamps.len() / 3;
            let ys: Vec<f32> = stamps.iter().skip(third).take(third).map(|s| s.center.y).collect();
            ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)
        };
        assert!(spread(1.0) < spread(0.0) * 0.5, "{} {}", spread(1.0), spread(0.0));
    }

    #[test]
    fn sparse_input_curves_through_its_points_and_an_exact_path_keeps_its_corners() {
        // a finger round a circle of 100 px in 8 samples: straight between them, the middles
        // would fall 7.6 px inside it
        let round: Vec<Sample> = (0..=8)
            .map(|i| {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                Sample::at(Vec2::new(200.0 + 100.0 * a.cos(), 200.0 + 100.0 * a.sin()), 1.0, f64::from(i) * 8.0)
            })
            .collect();
        let mut s = StrokeBuilder::new(plain(), 4.0, Pointer::Finger);
        s.add(&round);
        let stamps = s.finish();
        let off = stamps.iter().map(|p| (p.center.distance(Vec2::new(200.0, 200.0)) - 100.0).abs()).fold(0.0, f32::max);
        assert!(off < 1.5, "{off} px off the circle");
        // a square drawn exactly: its corner is a corner, its sides straight
        let square = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)];
        let samples: Vec<Sample> = square
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| Sample::at(Vec2::new(x + 50.0, y + 50.0), 1.0, i as f64 * 8.0))
            .collect();
        let mut s = StrokeBuilder::new(plain(), 4.0, Pointer::Finger).exact_path();
        s.add(&samples);
        let stamps = s.finish();
        assert!(stamps.iter().all(|p| (p.center.y - 50.0).abs() < 1e-3 || (p.center.x - 150.0).abs() < 1e-3));
        assert!(stamps.iter().any(|p| p.center.distance(Vec2::new(150.0, 50.0)) < 1.0));
    }

    /// A pen half round a circle of 150 px about (400, 400) in 150 ms, reported 60 times a
    /// second: 20° between samples.
    fn half_circle() -> Vec<Sample> {
        (0..=9)
            .map(|i| {
                let a = std::f32::consts::PI * (1.0 - i as f32 / 9.0);
                let at = Vec2::new(400.0 + 150.0 * a.cos(), 400.0 - 150.0 * a.sin());
                Sample::at(at, 1.0, f64::from(i) * 1000.0 / 60.0)
            })
            .collect()
    }

    /// How far the furthest of `stamps` is off the circle of [`half_circle`].
    fn off_the_circle(stamps: &[Stamp]) -> f32 {
        stamps.iter().map(|s| (s.center.distance(Vec2::new(400.0, 400.0)) - 150.0).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn a_strokes_first_and_last_stretches_bend_as_it_does() {
        let drawn = |samples: &[Sample]| {
            let mut s = StrokeBuilder::new(plain(), 6.0, Pointer::Stylus);
            s.add(samples);
            s.finish()
        };
        // straight from one sample to the next the stamps would fall 2.3 px inside the circle;
        // the first and last stretches, with nothing before or after them, bend as the rest do
        // (walked as if their end point came twice, they fell 1.1 px inside)
        let stamps = drawn(&half_circle());
        assert!(off_the_circle(&stamps) < 0.2, "{} px off the circle", off_the_circle(&stamps));
        // starting where the pen landed and ending where it lifted
        assert!(stamps.first().is_some_and(|s| s.center.distance(Vec2::new(250.0, 400.0)) < 1e-3));
        assert!(stamps.last().is_some_and(|s| s.center.distance(Vec2::new(550.0, 400.0)) < 1.5));
        // the lift reported where the pen last was, and a landing reported twice, change nothing
        let mut twice = half_circle();
        twice.insert(1, Sample { time_ms: 1.0, ..twice[0] });
        if let Some(&last) = twice.last() {
            twice.push(Sample { time_ms: last.time_ms + 1000.0 / 60.0, ..last });
        }
        let again = drawn(&twice);
        assert_eq!(again.len(), stamps.len());
        assert!(again.iter().zip(&stamps).all(|(a, b)| a.center.distance(b.center) < 1e-3));
    }

    /// The most the stroke turns (degrees) from the chord over `k` stamps to the next, anywhere
    /// after its first `from` stamps.
    fn most_turn(stamps: &[Stamp], k: usize, from: usize) -> f32 {
        let at = |i: usize| stamps.get(i).map_or(Vec2::ZERO, |s| s.center);
        (from.max(k)..stamps.len().saturating_sub(k))
            .map(|i| {
                let (u, v) = (at(i) - at(i - k), at(i + k) - at(i));
                (u.x * v.y - u.y * v.x).atan2(u.x * v.x + u.y * v.y).abs().to_degrees()
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn the_smoothed_path_catches_up_with_the_pen_along_the_curve() {
        let drawn = |input: InputSettings| {
            let mut s =
                StrokeBuilder::new(Brush { streamline: 0.015, ..plain() }, 6.0, Pointer::Stylus).with_input(input);
            s.add(&half_circle());
            s.finish()
        };
        // streamline: the path lags inside the circle, and at the lift it comes back out to where
        // the pen is along the curve, turning no more sharply there than anywhere (a straight run
        // to the lift point broke off from the curve by 12°)
        let stamps = drawn(InputSettings::default());
        let late = stamps.len() * 2 / 3;
        let turn = most_turn(&stamps, 3, late);
        assert!(turn < 4.0, "the end turns {turn}°");
        assert!(stamps.last().is_some_and(|s| s.center.distance(Vec2::new(550.0, 400.0)) < 1.5));
        assert!(off_the_circle(&stamps) < 3.0, "{} px off the circle", off_the_circle(&stamps));
    }

    /// A quick slash, curved as a C, `ms` long and sampled 120 times a second: down the left of an
    /// ellipse, slowly at its ends.
    fn slash(ms: f64) -> Vec<Sample> {
        let n = (ms * 0.12) as usize;
        (0..=n)
            .map(|i| {
                let u = i as f32 / n as f32;
                let a = std::f32::consts::PI * (0.15 + 0.8 * u * u * (3.0 - 2.0 * u));
                Sample::at(
                    Vec2::new(600.0 - 420.0 * a.sin(), 400.0 - 320.0 * a.cos()),
                    0.7,
                    f64::from(i as u32) * 25.0 / 3.0,
                )
            })
            .collect()
    }

    /// A stroke through `samples` with stabilisation at `amount`, whole.
    fn stabilised(amount: f32, samples: &[Sample]) -> Vec<Stamp> {
        let mut b = plain();
        b.set_setting("stabilization", amount);
        let mut s = StrokeBuilder::new(b, 6.0, Pointer::Stylus);
        s.add(samples);
        s.finish()
    }

    #[test]
    fn a_stabilised_slash_bends_as_a_whole_and_keeps_both_its_ends() {
        for ms in [150.0, 400.0, 900.0] {
            let input = slash(ms);
            let (start, end) = (input[0].pos, input[input.len() - 1].pos);
            for amount in [0.2, 0.5, 0.75, 1.0] {
                let stamps = stabilised(amount, &input);
                assert!(stamps[0].center.distance(start) < 1.0, "{ms} ms at {amount}: starts off the input");
                let last = stamps[stamps.len() - 1].center;
                assert!(last.distance(end) < 2.0, "{ms} ms at {amount}: ends {last:?}, not {end:?}");
                // nowhere does it break off into a straight run to the lift: from one 20 px stretch
                // to the next it turns no more than the slash does (an average trailing the pen
                // and catching up at the lift turned 18°)
                let k = (20.0 / stamps[0].center.distance(stamps[1].center)).ceil() as usize;
                let turn = most_turn(&stamps, k, k);
                assert!(turn < 8.0, "{ms} ms at {amount}: turns {turn}° at once");
            }
        }
    }

    #[test]
    fn stabilisation_averages_about_each_point_so_a_stroke_comes_out_the_same_either_way() {
        // drawn back the way it came, at the same pace, it lies on the same curve
        let there = slash(400.0);
        let end = there[there.len() - 1].time_ms;
        let mut back: Vec<Sample> = there.iter().map(|s| Sample { time_ms: end - s.time_ms, ..*s }).collect();
        back.reverse();
        let (a, b) = (stabilised(0.75, &there), stabilised(0.75, &back));
        for p in a.iter().step_by(10) {
            let near = b.iter().map(|q| q.center.distance(p.center)).fold(f32::MAX, f32::min);
            assert!(near < 2.5, "{near} px off at {:?}", p.center);
        }
        // and with the average the width of the whole stroke, it leaves the curve for the chord
        let input = slash(150.0);
        let (from, to) = (input[0].pos, input[input.len() - 1].pos);
        let along = (to - from).normalized_or(Vec2::new(1.0, 0.0));
        let across = |p: Vec2| ((p - from).x * along.y - (p - from).y * along.x).abs();
        let bow = |s: &[Stamp]| s.iter().map(|s| across(s.center)).fold(0.0, f32::max);
        assert!(bow(&stabilised(1.0, &input)) < bow(&stabilised(0.2, &input)) * 0.5);
    }

    #[test]
    fn a_stabilised_stroke_shows_as_it_will_end_and_keeps_what_it_has_drawn() {
        let mut b = plain();
        b.set_setting("stabilization", 0.5);
        let input = slash(600.0);
        let mut live = StrokeBuilder::new(b.clone(), 6.0, Pointer::Stylus);
        let mut drawn = Vec::new();
        for chunk in input.chunks(3) {
            drawn.extend(live.add(chunk));
            // what shows reaches the input, whatever the average still has to take in
            let ahead = live.preview_to_input(&[]);
            let end = ahead.last().or(drawn.last()).map_or(Vec2::ZERO, |s| s.center);
            let at = chunk[chunk.len() - 1].pos;
            assert!(end.distance(at) < 2.0, "{end:?} behind the input at {at:?}");
        }
        // shown just before the lift, the stroke is the stroke as it ends
        let shown: Vec<Vec2> = drawn.iter().chain(&live.preview_to_input(&[])).map(|s| s.center).collect();
        let done: Vec<Vec2> = live.finish().iter().map(|s| s.center).collect();
        assert_eq!(shown.len(), done.len());
        assert!(shown.iter().zip(&done).all(|(a, b)| a.distance(*b) < 0.01));
    }

    #[test]
    fn stabilisation_keeps_a_tap_and_a_stroke_all_inside_its_reach() {
        let mut b = plain();
        b.set_setting("stabilization", 1.0);
        let mut tap = StrokeBuilder::new(b.clone(), 6.0, Pointer::Stylus);
        tap.add(&[Sample::at(Vec2::new(10.0, 10.0), 0.5, 0.0)]);
        let tapped = tap.finish();
        assert_eq!(tapped.len(), 1);
        assert_eq!(tapped[0].center, Vec2::new(10.0, 10.0));
        // a few points, far fewer than it averages at full strength
        let few: Vec<Sample> =
            (0..4).map(|i| Sample::at(Vec2::new(i as f32 * 30.0, 0.0), 0.5, f64::from(i) * 8.0)).collect();
        let stamps = stabilised(1.0, &few);
        assert!(stamps.iter().all(|s| s.center.is_finite()));
        assert!(stamps[0].center.distance(few[0].pos) < 1.0);
        assert!(stamps[stamps.len() - 1].center.distance(few[3].pos) < 2.0);
    }

    #[test]
    fn smoothing_takes_the_same_time_at_any_input_rate() {
        // a pen moving 1 px a millisecond for 800 ms, pressure rising from 0.2 to 1, sampled at
        // the tuned rate and at three times it
        let path = |rate: f64| -> Vec<Sample> {
            let n = (rate * 0.8) as usize;
            (0..=n)
                .map(|i| {
                    let t = i as f64 * 1000.0 / rate;
                    Sample::at(Vec2::new(t as f32, 50.0), 0.2 + 0.8 * (t / 800.0) as f32, t)
                })
                .collect()
        };
        // how far the stroke's end is behind the pen's, and the last stamp's size
        let end = |b: &Brush, rate: f64| {
            let mut s = StrokeBuilder::new(b.clone(), 4.0, Pointer::Stylus);
            s.add(&path(rate));
            let last = *s.stamps().last().unwrap();
            (800.0 - last.center.x, last.radius)
        };
        let close = |a: f32, b: f32, share: f32| (a - b).abs() <= share * a.abs().max(b.abs());
        for (setting, amount, behind) in [("stabilization", 1.0, 200.0), ("motion_filtering", 0.25, 15.0)] {
            let mut b = plain();
            b.set_setting(setting, amount);
            let (slow, fast) = (end(&b, PEN_RATE).0, end(&b, PEN_RATE * 3.0).0);
            assert!(slow > behind, "{setting} lags {slow}");
            assert!(close(slow, fast, 0.1), "{setting}: {slow} behind at 240 Hz, {fast} at 720 Hz");
        }
        let b = Brush { pressure_smoothing: 0.9, pressure_size: 1.0, ..plain() };
        let unsmoothed = end(&Brush { pressure_smoothing: 0.0, ..b.clone() }, PEN_RATE).1;
        let (slow, fast) = (end(&b, PEN_RATE).1, end(&b, PEN_RATE * 3.0).1);
        assert!(slow < unsmoothed, "{slow} vs {unsmoothed}");
        assert!(
            close(unsmoothed - slow, unsmoothed - fast, 0.15),
            "pressure: {slow} at 240 Hz, {fast} at 720 Hz, {unsmoothed} unsmoothed"
        );
    }

    #[test]
    fn a_stroke_leaves_where_the_pen_landed_without_a_hook() {
        // the half circle, as it is and with a pen's streamline: the first 6 px head down the
        // circle's tangent (the chord that far along is 1.2° off it; the first stretch walked as
        // if its first point came twice headed 4° to 6° off)
        for streamline in [0.0, 0.015] {
            let mut s = StrokeBuilder::new(Brush { streamline, ..plain() }, 6.0, Pointer::Stylus);
            s.add(&half_circle());
            let stamps = s.finish();
            let landed = Vec2::new(250.0, 400.0);
            let on = stamps.iter().find(|st| st.center.distance(landed) >= 6.0).map_or(landed, |st| st.center);
            let heading = (on - landed).y.atan2((on - landed).x).to_degrees();
            assert!((heading + 90.0).abs() < 2.5, "streamline {streamline}: heads {heading}°");
        }
        // a pen landing 2 px off the line it then draws, 240 times a second at 2000 px/s: the
        // stroke comes back to the line without swinging past it
        let samples: Vec<Sample> = (0..40)
            .map(|i| {
                let y = if i == 1 { 52.0 } else { 50.0 };
                Sample::at(Vec2::new(100.0 + i as f32 * 25.0 / 3.0, y), 1.0, f64::from(i) * 1000.0 / 240.0)
            })
            .collect();
        let mut s = StrokeBuilder::new(plain(), 6.0, Pointer::Stylus);
        s.add(&samples);
        let stamps = s.finish();
        let lowest = stamps.iter().map(|st| st.center.y).fold(f32::MAX, f32::min);
        let highest = stamps.iter().map(|st| st.center.y).fold(f32::MIN, f32::max);
        assert!(lowest > 49.75 && highest < 52.01, "{lowest}..{highest}");
        assert!(stamps.iter().filter(|st| st.center.x > 130.0).all(|st| (st.center.y - 50.0).abs() < 0.01));
    }

    #[test]
    fn the_first_stamp_turns_with_the_stroke_as_the_rest_do() {
        let brush = Brush { shape: Shape { follow: 1.0, ..Shape::default() }, ..plain() };
        // straight down the canvas: every stamp turned a quarter, the one where the pen landed too
        let down: Vec<Sample> =
            (0..=10).map(|i| Sample::at(Vec2::new(50.0, 4.0 * i as f32), 1.0, f64::from(i) * 4.0)).collect();
        let mut s = StrokeBuilder::new(brush.clone(), 10.0, Pointer::Stylus);
        s.add(&down);
        let stamps = s.finish();
        assert!(stamps.len() > 10);
        assert!(stamps.iter().all(|st| (st.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-4), "{:?}", stamps[0]);
        // a tap goes nowhere: its dot turns as if it went up the screen
        let mut s = StrokeBuilder::new(brush, 10.0, Pointer::Stylus);
        s.add(&down[..1]);
        assert!((s.finish()[0].angle + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
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
    fn a_brush_set_to_azimuth_and_barrel_roll_turns_its_stamps_as_the_barrel_turns() {
        let quarter = std::f32::consts::FRAC_PI_2;
        let angles_from = |style: f32, relative: f32, pointer: Pointer| {
            let mut b = plain();
            b.set_setting("input_style", style);
            b.set_setting("relative_to_stroke", relative);
            let mut s = StrokeBuilder::new(b, 10.0, pointer);
            let samples: Vec<Sample> = (0..=20u8)
                .map(|i| {
                    let at = Sample::at(Vec2::new(f32::from(i) * 5.0, 0.0), 1.0, f64::from(i) * 8.0);
                    Sample { roll: 1.0 + f32::from(i) * quarter / 20.0, ..at }
                })
                .collect();
            s.add(&samples);
            let stamps = s.finish();
            (stamps.first().map(|s| s.angle), stamps.last().map(|s| s.angle))
        };
        // relative to the stroke: from how the barrel was turned as the stroke began, as far as it
        // turns on
        let angles = |style: f32, pointer: Pointer| angles_from(style, 1.0, pointer);
        let (first, last) = angles(2.0, Pointer::Stylus);
        assert!(first.is_some_and(|a| a.abs() < 1e-3), "{first:?}");
        assert!(last.is_some_and(|a| (a - quarter).abs() < 0.05), "{last:?}");
        // not: as the barrel is turned from its zero, the turn it was held at from the first stamp
        let (first, last) = angles_from(2.0, 0.0, Pointer::Stylus);
        assert!(first.is_some_and(|a| (a - 1.0).abs() < 1e-3), "{first:?}");
        assert!(last.is_some_and(|a| (a - 1.0 - quarter).abs() < 0.05), "{last:?}");
        assert_eq!(angles_from(2.0, 0.0, Pointer::Finger), (Some(0.0), Some(0.0)));
        // the azimuth alone, or a finger: no turn
        assert_eq!(angles(1.0, Pointer::Stylus), (Some(0.0), Some(0.0)));
        assert_eq!(angles(2.0, Pointer::Finger), (Some(0.0), Some(0.0)));
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
    fn motion_filtering_is_a_low_pass_that_moves_nothing_along() {
        let weights = motion_weights(0.25, 0.0, PEN_RATE);
        assert_eq!(weights.len() % 2, 1);
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert!(weights.iter().zip(weights.iter().rev()).all(|(a, b)| (a - b).abs() < 1e-6));
        assert!(motion_weights(0.75, 0.0, PEN_RATE).len() > weights.len());
        assert!(motion_weights(0.25, 1.0, PEN_RATE).len() > weights.len());
        assert!(motion_weights(1.0, 1.0, PEN_RATE).len() <= MOTION_MOST_SAMPLES + 1);
        // how much of a wave at `hertz` comes through
        let through = |hertz: f64| {
            let reach = (weights.len() / 2) as f64;
            let at = |(n, w): (usize, &f32)| f64::from(*w) * (TAU * hertz * (n as f64 - reach) / PEN_RATE).cos();
            weights.iter().enumerate().map(at).sum::<f64>()
        };
        assert!(through(1.0) > 0.9, "{}", through(1.0));
        assert!(through(30.0).abs() < 0.02, "{}", through(30.0));
        // a slow circle keeps its size, where following behind the pen would cut inside it; both
        // ends stay where the input's are
        let circle: Vec<Sample> = (0..=480)
            .map(|i| {
                let t = f64::from(i) / PEN_RATE;
                let (y, x) = (TAU * 0.5 * t).sin_cos();
                Sample::at(Vec2::new(300.0 + 200.0 * x as f32, 300.0 + 200.0 * y as f32), 1.0, t * 1000.0)
            })
            .collect();
        let mut b = plain();
        b.set_setting("motion_filtering", 0.25);
        let mut s = StrokeBuilder::new(b, 4.0, Pointer::Stylus);
        s.add(&circle);
        let stamps = s.finish();
        let centre = Vec2::new(300.0, 300.0);
        let third = stamps.len() / 3;
        for stamp in stamps.iter().skip(third).take(third) {
            let off = stamp.center.distance(centre) - 200.0;
            assert!(off.abs() < 3.0, "{off}");
        }
        assert!(stamps[0].center.distance(circle[0].pos) < 3.0, "{:?}", stamps[0].center);
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
        // a line waving at `hertz`: how far it waves in the middle of the stroke
        let spread = |hertz: f64, input: InputSettings| {
            let wave: Vec<Sample> = (0..=480)
                .map(|i| {
                    let t = f64::from(i) / PEN_RATE;
                    let y = 6.0 * (TAU * hertz * t).sin() as f32;
                    let mut s = Sample::at(Vec2::new(i as f32 * 2.0, y), 0.5, t * 1000.0);
                    s.tilt = 0.0;
                    s
                })
                .collect();
            let mut b = StrokeBuilder::new(brush.clone(), 4.0, Pointer::Stylus).with_input(input);
            b.add(&wave);
            let stamps = b.finish();
            let third = stamps.len() / 3;
            let ys: Vec<f32> = stamps.iter().skip(third).take(third).map(|s| s.center.y).collect();
            ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)
        };
        let filtering = InputSettings { motion_filtering: 0.1, ..InputSettings::default() };
        let expressive = InputSettings { motion_expression: 1.0, ..filtering.clone() };
        let (plain, filtered) = (spread(30.0, InputSettings::default()), spread(30.0, filtering.clone()));
        assert!(filtered < plain * 0.1, "{filtered} vs {plain}");
        // the hand's own motion, under the cut, comes through more with expression
        let (kept, more) = (spread(4.0, filtering), spread(4.0, expressive));
        assert!(more > kept, "{more} vs {kept}");
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
        // (a tile's worth at most; the stamps are 4 px, the grain following them by default)
        assert!(a.x >= 0.0 && a.x <= 8.0 && a.y >= 0.0 && a.y <= 8.0, "{a:?}");
    }

    #[test]
    fn a_grain_s_patch_takes_the_rotation_s_share_of_the_stamp_s_turn() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
        let turn = |rotation: f32, follow: f32, angle: f32| {
            let mut b = plain();
            b.set_setting("grain_rotation", rotation);
            b.set_setting("shape_rotation", follow);
            b.shape.rotation = angle;
            let mut s = StrokeBuilder::new(b, 10.0, Pointer::Finger);
            // down the screen
            let down: Vec<Sample> =
                (0..20).map(|i| Sample::at(Vec2::new(50.0, i as f32 * 8.0), 1.0, f64::from(i) * 8.0)).collect();
            s.add(&down);
            s.finish()[5].grain_turn
        };
        // none of it: the grain holds still whatever the stamp does
        assert_eq!(turn(0.0, 1.0, 0.3), 0.0);
        // a stamp that holds still still counts from its eighth of a turn
        assert!((turn(1.0, 0.0, 0.0) - FRAC_PI_4).abs() < 1e-5);
        assert!((turn(-1.0, 0.0, 0.0) + FRAC_PI_4).abs() < 1e-5);
        assert!((turn(0.5, 0.0, 0.2) - 0.5 * (FRAC_PI_4 + 0.2)).abs() < 1e-5);
        // following the stroke, it goes round with the stamp
        assert!((turn(1.0, 1.0, 0.0) - (FRAC_PI_4 + FRAC_PI_2)).abs() < 1e-4);
        // a brush kept to the screen keeps its grain to it, read and all
        let mut b = plain();
        b.set_setting("orient_to_screen", 1.0);
        b.set_setting("grain_movement", 1.0);
        let turned = |view: f32| {
            let mut s = StrokeBuilder::new(b.clone(), 10.0, Pointer::Finger).on_view_turned(view);
            s.add(&line(10, 100.0));
            let all = s.finish();
            (all[0].grain_turn, all[0].grain_offset, all.last().unwrap().grain_offset)
        };
        let (patch, first, last) = turned(FRAC_PI_2);
        assert!((patch - FRAC_PI_2).abs() < 1e-5);
        // along x on the canvas, down the screen
        let went = last - first;
        assert!(went.x.abs() < 1e-2 && went.y > 1.0, "{went:?}");
    }

    #[test]
    fn a_rolling_grain_goes_on_from_stamp_to_stamp_whatever_its_tile_does() {
        // a tile that follows a stamp whose size follows the pressure, far from the canvas's corner
        let mut brush = Brush { pressure_size: 1.0, ..plain() };
        brush.set_setting("grain_movement", 1.0);
        brush.set_setting("grain_zoom", 0.0);
        brush.grain.scale = crate::studio::MAX_GRAIN_SCALE / 4.0;
        let scale = brush.grain.scale;
        let samples: Vec<Sample> = (0..=200)
            .map(|i| {
                let pressure = 0.6 + 0.4 * (i as f32 / 12.0).sin();
                Sample::at(Vec2::new(5000.0 + i as f32 * 2.0, 3000.0), pressure, i as f64 * 8.0)
            })
            .collect();
        let mut stroke = StrokeBuilder::new(brush, 30.0, Pointer::Stylus);
        stroke.add(&samples);
        let stamps = stroke.finish();
        // each stamp's place in the grain, in tiles, is the last's and what it moved over its tile:
        // however much the tile changes it never jumps
        let reads: Vec<(Vec2, Vec2)> = stamps
            .iter()
            .map(|s| {
                let tile = s.grain_size * scale;
                (s.grain_offset * (1.0 / tile), s.center)
            })
            .collect();
        for pair in stamps.windows(2).zip(reads.windows(2)) {
            let (s, r) = pair;
            let moved = s[1].center.distance(s[0].center) / (s[1].grain_size * scale);
            let went = r[1].0.distance(r[0].0);
            assert!((went - moved).abs() < 1e-3, "{went} {moved}");
        }
        assert!(stamps[10].grain_size != stamps[40].grain_size);
    }
}
