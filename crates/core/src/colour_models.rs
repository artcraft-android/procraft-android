//! The colour models the colour panel's disc, Classic and Value sliders can work in, chosen on the
//! panel's model button: a hue, a saturation and a third (brightness, lightness, luma or perceptual
//! lightness), each 0..1, over sRGB values.
//!
//! - HSB: hue, saturation and brightness (HSV by another name), the panel's own.
//! - HLS: hue, saturation and lightness, white at the top and black at the bottom.
//! - HSY′: hue and saturation as Krita's sliders take them (on the gamma-encoded values), and Y′ the
//!   luma of linear light (Rec. 709), sRGB-encoded, as Krita's luma bar shows it: colours of one Y′
//!   are one grey in the grey view, and turning the hue keeps it.
//! - OKLab: Björn Ottosson's Okhsl, OKLab's hue and lightness (with his toe, so its middle is a
//!   perceptual middle grey) and a saturation reaching the edge of sRGB at 1.
//!
//! The apps draw the pickers; the maths, the pickers' pictures and touches, the colours along their
//! sliders and where a colour stands in a model are worked out here, so every app's alike.

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Mutex};

use crate::hues::{HueControl, SLIDER_HUES, hue_at, hue_position};
use crate::pickers::{byte, disc_to_square, hsv_to_rgb, picture_rows, rgb_to_hsv, square_to_disc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ColourModel {
    #[default]
    Hsb,
    Hls,
    Hsy,
    Oklab,
}

/// Rec. 709 luma weights, as Krita's HSY′ takes them.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// The grey HSB's saturation slider runs from (to the pure hue).
const SATURATION_GREY: u32 = 0x7F_7F7F;

/// The most pixels across Classic's square has in a model: see [`ColourModel::square_picture`].
pub const MODEL_SQUARE: u32 = 256;

/// The most pixels across the disc's picture has.
const MOST_SIDE: u32 = 4096;

/// A slider's track: where its knob stands (0..1) and the colours along it (0xRRGGBB), evenly
/// spaced.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ColourTrack {
    pub place: f32,
    pub colours: Vec<u32>,
}

impl ColourModel {
    pub const ALL: [ColourModel; 4] = [ColourModel::Hsb, ColourModel::Hls, ColourModel::Hsy, ColourModel::Oklab];

    /// Its name in the panel's menu and on its button.
    pub fn label(self) -> &'static str {
        match self {
            ColourModel::Hsb => "HSB",
            ColourModel::Hls => "HLS",
            ColourModel::Hsy => "HSY\u{2032}",
            ColourModel::Oklab => "OKLab",
        }
    }

    /// The letters of its three channels, as the Value sliders show them.
    pub fn letters(self) -> [&'static str; 3] {
        match self {
            ColourModel::Hsb => ["H", "S", "B"],
            ColourModel::Hls | ColourModel::Oklab => ["H", "S", "L"],
            ColourModel::Hsy => ["H", "S", "Y\u{2032}"],
        }
    }

    /// The name kept in the preferences.
    pub fn key(self) -> &'static str {
        match self {
            ColourModel::Hsb => "hsb",
            ColourModel::Hls => "hls",
            ColourModel::Hsy => "hsy",
            ColourModel::Oklab => "oklab",
        }
    }

    pub fn of_key(key: &str) -> Option<ColourModel> {
        ColourModel::ALL.into_iter().find(|m| m.key() == key)
    }

    /// Whether its hue is the RGB hue (HSB's), so the ring's spacing can be kept.
    pub fn rgb_hue(self) -> bool {
        self != ColourModel::Oklab
    }

    /// Its channels (hue, saturation, third; 0..1, the last two taken at their ends past them) as
    /// sRGB values (0..1).
    pub fn to_rgb(self, [h, s, t]: [f32; 3]) -> [f32; 3] {
        let (s, t) = (s.clamp(0.0, 1.0), t.clamp(0.0, 1.0));
        let rgb = match self {
            ColourModel::Hsb => hsv_to_rgb([h, s, t]),
            ColourModel::Hls => {
                let c = (1.0 - (2.0 * t - 1.0).abs()) * s;
                let pure = hsv_to_rgb([h, 1.0, 1.0]);
                pure.map(|p| t + c * (p - 0.5))
            }
            ColourModel::Hsy => {
                // the encoded luma whose colour (at this hue and saturation) has the linear luma Y′
                // stands for: found by halving, the linear luma rising with the encoded one
                let target = to_linear(t);
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for _ in 0..24 {
                    let mid = (lo + hi) / 2.0;
                    if linear_luma(encoded_hsy_to_rgb([h, s, mid])) < target { lo = mid } else { hi = mid }
                }
                encoded_hsy_to_rgb([h, s, (lo + hi) / 2.0])
            }
            ColourModel::Oklab => okhsl_to_srgb([h, s, t]),
        };
        // (a hair outside, as rounding leaves HLS's)
        rgb.map(|c| c.clamp(0.0, 1.0))
    }

    /// sRGB values as its channels; a grey keeps `hue` (and its saturation is 0).
    pub fn of_rgb(self, rgb: [f32; 3], hue: f32) -> [f32; 3] {
        let [h, chroma] = {
            let [h, s, v] = rgb_to_hsv(rgb);
            [h, s * v]
        };
        let grey = chroma <= 1e-6;
        match self {
            ColourModel::Hsb => {
                let [h, s, v] = rgb_to_hsv(rgb);
                [if grey { hue } else { h }, s, v]
            }
            ColourModel::Hls => {
                let max = rgb[0].max(rgb[1]).max(rgb[2]);
                let min = rgb[0].min(rgb[1]).min(rgb[2]);
                let l = (max + min) / 2.0;
                let d = 1.0 - (2.0 * l - 1.0).abs();
                [if grey { hue } else { h }, if d > 1e-6 { chroma / d } else { 0.0 }, l]
            }
            ColourModel::Hsy => {
                let [h, s, _] = encoded_hsy_of(rgb, hue);
                [h, s, to_gamma(linear_luma(rgb))]
            }
            ColourModel::Oklab => {
                let [h, s, l] = srgb_to_okhsl(rgb);
                [if grey || s <= 1e-5 { hue } else { h }, if grey { 0.0 } else { s }, l]
            }
        }
    }

    /// The colour at `coords` as hue, saturation and brightness, as the apps hold a colour; a grey
    /// keeps `hue`.
    pub fn to_hsv(self, coords: [f32; 3], hue: f32) -> [f32; 3] {
        if self == ColourModel::Hsb {
            return coords;
        }
        let [h, s, v] = rgb_to_hsv(self.to_rgb(coords));
        [if s > 0.0 { h } else { hue }, s, v]
    }

    /// The third channel the hue ring shows its hues at (the purest colours).
    pub fn ring_third(self, h: f32) -> f32 {
        match self {
            ColourModel::Hsb => 1.0,
            ColourModel::Hls => 0.5,
            ColourModel::Hsy => to_gamma(linear_luma(hsv_to_rgb([h, 1.0, 1.0]))),
            ColourModel::Oklab => 0.7,
        }
    }

    /// Its purest colour at hue `h`, as the hue ring and the hue slider show it.
    pub fn pure(self, h: f32) -> [f32; 3] {
        self.to_rgb([h, 1.0, self.ring_third(h)])
    }

    /// The disc's ring's hue at `turn` (0..1 anticlockwise from the right): the ring's own spacing
    /// (HSB's, as measured) for a model with the RGB hue, else even.
    pub fn ring_hue(self, turn: f32) -> f32 {
        if self.rgb_hue() { hue_at(HueControl::Ring, turn) } else { turn }
    }

    /// Where on the ring `hue` sits (0..1 anticlockwise from the right).
    pub fn ring_turn(self, hue: f32) -> f32 {
        if self.rgb_hue() { hue_position(HueControl::Ring, hue) } else { hue.rem_euclid(1.0) }
    }

    /// `coords` with channel `channel` (0 the hue) slid to `to`, 0..1 along its slider (the hue
    /// slider's own spacing for a model with the RGB hue).
    pub fn slid(self, mut coords: [f32; 3], channel: usize, to: f32) -> [f32; 3] {
        let value = if channel == 0 && self.rgb_hue() { hue_at(HueControl::Slider, to) } else { to };
        if let Some(slot) = coords.get_mut(channel) {
            *slot = value;
        }
        coords
    }

    /// The colours (0xRRGGBB) along channel `channel` from `coords`: `n + 1` of them, the channel
    /// evenly from 0 to 1.
    pub fn along(self, coords: [f32; 3], channel: usize, n: usize) -> Vec<u32> {
        let n = n.max(1);
        (0..=n)
            .map(|k| {
                let mut c = coords;
                if let Some(slot) = c.get_mut(channel) {
                    *slot = k as f32 / n as f32;
                }
                hex(self.to_rgb(c))
            })
            .collect()
    }

    /// Classic's three sliders (and Value's first three) at `coords`: the hue (its purest colours,
    /// HSB's slider's own hues for a model with the RGB hue), the saturation and the third.
    pub fn tracks(self, coords: [f32; 3]) -> [ColourTrack; 3] {
        let [h, s, t] = coords;
        let hue = if self.rgb_hue() {
            let colours = SLIDER_HUES.iter().map(|d| hex(self.pure(d / 360.0))).collect();
            ColourTrack { place: hue_position(HueControl::Slider, h), colours }
        } else {
            ColourTrack { place: h, colours: (0..=24).map(|k| hex(self.pure(k as f32 / 24.0))).collect() }
        };
        if self == ColourModel::Hsb {
            return [
                hue,
                ColourTrack { place: s, colours: vec![SATURATION_GREY, hex(hsv_to_rgb([h, 1.0, 1.0]))] },
                ColourTrack { place: t, colours: vec![0x00_0000, 0xFF_FFFF] },
            ];
        }
        [
            hue,
            ColourTrack { place: s, colours: self.along(coords, 1, 8) },
            ColourTrack { place: t, colours: self.along(coords, 2, 12) },
        ]
    }

    /// Where the colour at `coords` sits on the disc picker, from its middle in disc radii (y
    /// down). HSY′'s disc is Krita's wheel: the hue round it and the saturation out, worked out on
    /// linear light; another's is the square (saturation across, the third up) fitted onto it.
    pub fn disc_point(self, coords: [f32; 3]) -> [f32; 2] {
        if self == ColourModel::Hsy {
            let [h, s, _] = linear_hsy_of(self.to_rgb(coords), coords[0]);
            let turn = self.ring_turn(h) * TAU;
            return [turn.cos() * s, -turn.sin() * s];
        }
        square_to_disc(coords[1] * 2.0 - 1.0, 1.0 - coords[2] * 2.0)
    }

    /// The colour a touch on the disc picker at `uv` (from its middle in disc radii, y down; one
    /// past its edge taken at the edge) picks, from the colour at `coords`: HSY′'s keeps its linear
    /// luma, another's its hue.
    pub fn disc_pick(self, coords: [f32; 3], [u, v]: [f32; 2]) -> [f32; 3] {
        if self == ColourModel::Hsy {
            let turn = ((-v).atan2(u) / TAU + 1.0) % 1.0;
            let luma = linear_hsy_of(self.to_rgb(coords), coords[0])[2];
            let hue = self.ring_hue(turn);
            let rgb = linear_hsy_to_rgb([hue, u.hypot(v).min(1.0), luma]);
            return self.of_rgb(rgb, hue);
        }
        let length = u.hypot(v);
        let (u, v) = if length > 1.0 { (u / length, v / length) } else { (u, v) };
        let [x, y] = disc_to_square(u, v);
        [coords[0], (x + 1.0) / 2.0, (1.0 - y) / 2.0]
    }

    /// What the disc's picture shows of the colour at `coords`, to tell when it is to be drawn
    /// again: its hue, or for HSY′'s wheel its linear luma.
    pub fn disc_key(self, coords: [f32; 3]) -> u32 {
        if self == ColourModel::Hsy {
            (linear_hsy_of(self.to_rgb(coords), coords[0])[2] * 4000.0).round() as u32
        } else {
            (coords[0] * 10_000.0).round() as u32
        }
    }

    /// The disc picker's picture for the colour at `coords`, `side` pixels square (at most 4096),
    /// with a soft edge; premultiplied RGBA. Saturation across and the third up (the square fitted
    /// onto the disc) at its hue; HSY′'s is Krita's wheel at its linear luma.
    pub fn disc_picture(self, coords: [f32; 3], side: u32) -> Vec<u8> {
        match self {
            ColourModel::Hsb => crate::pickers::disc_picture(coords[0], side),
            ColourModel::Hsy => luma_wheel_picture(linear_hsy_of(self.to_rgb(coords), coords[0])[2], side),
            _ => model_disc_picture(self, coords[0], side),
        }
    }

    /// Classic's square at `hue`: saturation across and the third up, `side` pixels square but at
    /// most [`MODEL_SQUARE`]; opaque RGBA. Small, as a hue drag draws it every frame (a soft
    /// gradient, it looks the same drawn up).
    pub fn square_picture(self, hue: f32, side: u32) -> Vec<u8> {
        let side = colour_model_square_side(side) as usize;
        let step = (side.max(2) - 1) as f32;
        picture_rows(side, |y, row| {
            for (x, px) in row.iter_mut().enumerate() {
                *px = premultiplied(self.to_rgb([hue, x as f32 / step, 1.0 - y as f32 / step]), 1.0);
            }
        })
    }
}

/// Where a colour stands in a colour model, kept while it is still the colour: worked out afresh
/// from a grey or black, its hue and saturation would be lost.
#[derive(Debug, Default, uniffi::Object)]
pub struct ColourPlace {
    kept: Mutex<Option<(ColourModel, u32, [f32; 3])>>,
}

impl ColourPlace {
    /// The colour held as hue, saturation and brightness `hsv` (0xRRGGBB `key`, as the app rounds
    /// it) in `model`: as last kept while it is still that colour, else worked out afresh, a grey
    /// keeping the hue it had in the model (else its own).
    pub fn coords_of(&self, model: ColourModel, hsv: [f32; 3], key: u32) -> [f32; 3] {
        if model == ColourModel::Hsb {
            return hsv;
        }
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        match *kept {
            Some((m, k, coords)) if m == model && k == key => coords,
            _ => {
                let hue = match *kept {
                    Some((m, _, coords)) if m == model => coords[0],
                    _ => hsv[0],
                };
                let coords = model.of_rgb(hsv_to_rgb(hsv), hue);
                *kept = Some((model, key, coords));
                coords
            }
        }
    }

    /// `coords` kept as where the colour they made (0xRRGGBB `key`, as the app rounds it) stands in
    /// `model`.
    pub fn keep_coords(&self, model: ColourModel, coords: [f32; 3], key: u32) {
        *self.kept.lock().unwrap_or_else(|e| e.into_inner()) = Some((model, key, coords));
    }
}

#[uniffi::export]
impl ColourPlace {
    #[uniffi::constructor]
    pub fn new() -> Arc<ColourPlace> {
        Arc::new(ColourPlace::default())
    }

    /// [`ColourPlace::coords_of`] for the colour held as `h`, `s`, `v` (0xRRGGBB `rgb`), as three
    /// values.
    pub fn coords(&self, model: ColourModel, h: f32, s: f32, v: f32, rgb: u32) -> Vec<f32> {
        self.coords_of(model, [h, s, v], rgb & 0xFF_FFFF).to_vec()
    }

    /// [`ColourPlace::keep_coords`]: `h`, `s` and `t` kept as where colour `rgb` (0xRRGGBB) stands
    /// in `model`.
    pub fn keep(&self, model: ColourModel, h: f32, s: f32, t: f32, rgb: u32) {
        self.keep_coords(model, [h, s, t], rgb & 0xFF_FFFF);
    }
}

/// HSY′ worked out on linear light, as Krita's Advanced Color Selector wheel does (its sliders
/// work on the gamma-encoded values, as [`ColourModel::Hsy`] does): sRGB values as hue,
/// saturation and linear luma, a grey keeping `hue`.
fn linear_hsy_of(rgb: [f32; 3], hue: f32) -> [f32; 3] {
    encoded_hsy_of(rgb.map(|c| to_linear(c.clamp(0.0, 1.0))), hue)
}

/// Hue, saturation and linear luma, as Krita's wheel takes them, as sRGB values.
fn linear_hsy_to_rgb(hsy: [f32; 3]) -> [f32; 3] {
    encoded_hsy_to_rgb(hsy).map(|c| to_gamma(c.clamp(0.0, 1.0)))
}

/// HSY′ on whatever values it is given (the values Krita's HSY′ gives): hue, saturation as
/// a share of the most chroma that luma holds at that hue, and Rec. 709 luma of those values.
fn encoded_hsy_of(rgb: [f32; 3], hue: f32) -> [f32; 3] {
    let [h, sat, v] = rgb_to_hsv(rgb);
    let chroma = sat * v;
    let y = luma(rgb);
    if chroma <= 1e-6 {
        return [hue, 0.0, y];
    }
    let y_pure = luma(hsv_to_rgb([h, 1.0, 1.0]));
    let reach = if y <= y_pure { y / y_pure.max(1e-6) } else { (1.0 - y) / (1.0 - y_pure).max(1e-6) };
    [h, if reach > 1e-6 { (chroma / reach).min(1.0) } else { 0.0 }, y]
}

/// The pure hue carried towards or away from the grey of luma `t` (as Krita's HSY′ does).
fn encoded_hsy_to_rgb([h, s, t]: [f32; 3]) -> [f32; 3] {
    let (s, t) = (s.clamp(0.0, 1.0), t.clamp(0.0, 1.0));
    let pure = hsv_to_rgb([h, 1.0, 1.0]);
    let y_pure = luma(pure);
    let reach = if t <= y_pure { t / y_pure.max(1e-6) } else { (1.0 - t) / (1.0 - y_pure).max(1e-6) };
    pure.map(|p| (t + s * reach * (p - y_pure)).clamp(0.0, 1.0))
}

/// The Rec. 709 luma of sRGB values in linear light: their luminance, what the canvas's grey view
/// shows them as.
fn linear_luma(rgb: [f32; 3]) -> f32 {
    luma(rgb.map(|c| to_linear(c.clamp(0.0, 1.0))))
}

/// How far OKLab's a and b run either way on the Value sliders (sRGB's most saturated colours
/// reach about 0.32).
pub const OKLAB_AB: f32 = 0.4;

/// sRGB values as OKLab's lightness (0..1), a and b.
pub fn oklab_of(rgb: [f32; 3]) -> [f32; 3] {
    linear_srgb_to_oklab(rgb.map(|c| to_linear(c.clamp(0.0, 1.0))))
}

/// OKLab as sRGB values: one outside sRGB brought in by lessening its chroma (its lightness and
/// hue kept), not by clipping each channel.
pub fn oklab_to_rgb([l, a, b]: [f32; 3]) -> [f32; 3] {
    let inside = |k: f32| {
        let rgb = oklab_to_linear_srgb([l, a * k, b * k]);
        rgb.iter().all(|c| (-1e-4..=1.0001).contains(c)).then_some(rgb)
    };
    let rgb = inside(1.0).unwrap_or_else(|| {
        // the most chroma that fits, found by halving
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if inside(mid).is_some() { lo = mid } else { hi = mid }
        }
        oklab_to_linear_srgb([l, a * lo, b * lo])
    });
    rgb.map(|c| to_gamma(c.clamp(0.0, 1.0)))
}

/// Value's three sliders in OKLab at the colour `lab`: its own lightness, a and b (a and b
/// ±[`OKLAB_AB`] across them).
pub fn oklab_tracks(lab: [f32; 3]) -> [ColourTrack; 3] {
    let track = |i: usize| {
        let colours = (0..=12).map(|k| hex(oklab_to_rgb(oklab_slid(lab, i, k as f32 / 12.0)))).collect();
        let v = lab.get(i).copied().unwrap_or(0.0);
        ColourTrack { place: if i == 0 { v } else { (v / OKLAB_AB + 1.0) / 2.0 }, colours }
    };
    [track(0), track(1), track(2)]
}

/// `lab` with channel `channel` (0 the lightness, then a and b) slid to `to`, 0..1 along its
/// slider.
pub fn oklab_slid(mut lab: [f32; 3], channel: usize, to: f32) -> [f32; 3] {
    if let Some(slot) = lab.get_mut(channel) {
        *slot = if channel == 0 { to } else { (to * 2.0 - 1.0) * OKLAB_AB };
    }
    lab
}

/// The disc's picture for a model other than HSB, `side` pixels square: saturation across and
/// the model's third channel up (the square fitted onto the disc) at `hue`, with a soft edge.
fn model_disc_picture(model: ColourModel, hue: f32, side: u32) -> Vec<u8> {
    let side = side.clamp(1, MOST_SIDE) as usize;
    let half = side as f32 / 2.0;
    picture_rows(side, |y, row| {
        let j = y as f32 + 0.5;
        for (x, px) in row.iter_mut().enumerate() {
            let (u, v) = ((x as f32 + 0.5 - half) / half, (j - half) / half);
            let edge = ((1.0 - u.hypot(v)) * half + 0.5).clamp(0.0, 1.0);
            if edge <= 0.0 {
                continue;
            }
            let [sx, sy] = disc_to_square(u.clamp(-1.0, 1.0), v.clamp(-1.0, 1.0));
            *px = premultiplied(model.to_rgb([hue, (sx + 1.0) / 2.0, (1.0 - sy) / 2.0]), edge);
        }
    })
}

/// HSY′'s wheel at linear luma `luma`, as Krita's: `side` pixels square, hue round it (as the ring
/// runs) and saturation out from the grey of that luma in the middle, worked out on linear light,
/// with a soft edge.
fn luma_wheel_picture(luma: f32, side: u32) -> Vec<u8> {
    let side = side.clamp(1, MOST_SIDE) as usize;
    let half = side as f32 / 2.0;
    picture_rows(side, |y, row| {
        let j = y as f32 + 0.5 - half;
        for (x, px) in row.iter_mut().enumerate() {
            let i = x as f32 + 0.5 - half;
            let r = i.hypot(j) / half;
            let edge = ((1.0 - r) * half + 0.5).clamp(0.0, 1.0);
            if edge <= 0.0 {
                continue;
            }
            let turn = ((-j).atan2(i) / TAU + 1.0) % 1.0;
            *px = premultiplied(linear_hsy_to_rgb([ColourModel::Hsy.ring_hue(turn), r.min(1.0), luma]), edge);
        }
    })
}

/// sRGB values as a pixel `edge` covered (0..1), premultiplied.
fn premultiplied(rgb: [f32; 3], edge: f32) -> [u8; 4] {
    let channel = |c: f32| (byte(c) as f32 * edge).round() as u8;
    [channel(rgb[0]), channel(rgb[1]), channel(rgb[2]), (edge * 255.0).round() as u8]
}

/// sRGB values as 0xRRGGBB.
fn hex([r, g, b]: [f32; 3]) -> u32 {
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

fn luma([r, g, b]: [f32; 3]) -> f32 {
    LUMA[0] * r + LUMA[1] * g + LUMA[2] * b
}

// For the apps' bindings: the colours as 0xRRGGBB, a colour's channels as separate values.

/// The colour models, in the menu's order.
#[uniffi::export]
pub fn colour_models() -> Vec<ColourModel> {
    ColourModel::ALL.to_vec()
}

/// [`ColourModel::label`].
#[uniffi::export]
pub fn colour_model_label(model: ColourModel) -> String {
    model.label().to_owned()
}

/// [`ColourModel::letters`].
#[uniffi::export]
pub fn colour_model_letters(model: ColourModel) -> Vec<String> {
    model.letters().map(str::to_owned).to_vec()
}

/// [`ColourModel::key`].
#[uniffi::export]
pub fn colour_model_key(model: ColourModel) -> String {
    model.key().to_owned()
}

/// [`ColourModel::of_key`].
#[uniffi::export]
pub fn colour_model_of_key(key: String) -> Option<ColourModel> {
    ColourModel::of_key(&key)
}

/// The colour at `h`, `s`, `t` in `model` as 0xRRGGBB.
#[uniffi::export]
pub fn colour_model_rgb(model: ColourModel, h: f32, s: f32, t: f32) -> u32 {
    hex(model.to_rgb([h, s, t]))
}

/// [`ColourModel::to_hsv`] as three values.
#[uniffi::export]
pub fn colour_model_hsv(model: ColourModel, h: f32, s: f32, t: f32, hue: f32) -> Vec<f32> {
    model.to_hsv([h, s, t], hue).to_vec()
}

/// [`ColourModel::pure`] as 0xRRGGBB.
#[uniffi::export]
pub fn colour_model_pure(model: ColourModel, hue: f32) -> u32 {
    hex(model.pure(hue))
}

/// The ring's colours (0xRRGGBB) at `n` turns evenly from 0 to 1 (anticlockwise from the right).
#[uniffi::export]
pub fn colour_model_ring(model: ColourModel, n: u32) -> Vec<u32> {
    let last = n.max(2) - 1;
    (0..=last).map(|i| hex(model.pure(model.ring_hue(i as f32 / last as f32)))).collect()
}

/// [`ColourModel::ring_hue`].
#[uniffi::export]
pub fn colour_model_ring_hue(model: ColourModel, turn: f32) -> f32 {
    model.ring_hue(turn)
}

/// [`ColourModel::ring_turn`].
#[uniffi::export]
pub fn colour_model_ring_turn(model: ColourModel, hue: f32) -> f32 {
    model.ring_turn(hue)
}

/// [`ColourModel::slid`] as three values.
#[uniffi::export]
pub fn colour_model_slid(model: ColourModel, h: f32, s: f32, t: f32, channel: u32, to: f32) -> Vec<f32> {
    model.slid([h, s, t], channel as usize, to).to_vec()
}

/// [`ColourModel::along`].
#[uniffi::export]
pub fn colour_model_along(model: ColourModel, h: f32, s: f32, t: f32, channel: u32, n: u32) -> Vec<u32> {
    model.along([h, s, t], channel as usize, n as usize)
}

/// [`ColourModel::tracks`].
#[uniffi::export]
pub fn colour_model_tracks(model: ColourModel, h: f32, s: f32, t: f32) -> Vec<ColourTrack> {
    model.tracks([h, s, t]).to_vec()
}

/// [`ColourModel::disc_point`] as two values.
#[uniffi::export]
pub fn colour_model_disc_point(model: ColourModel, h: f32, s: f32, t: f32) -> Vec<f32> {
    model.disc_point([h, s, t]).to_vec()
}

/// [`ColourModel::disc_pick`] as three values.
#[uniffi::export]
pub fn colour_model_disc_pick(model: ColourModel, h: f32, s: f32, t: f32, u: f32, v: f32) -> Vec<f32> {
    model.disc_pick([h, s, t], [u, v]).to_vec()
}

/// [`ColourModel::disc_key`].
#[uniffi::export]
pub fn colour_model_disc_key(model: ColourModel, h: f32, s: f32, t: f32) -> u32 {
    model.disc_key([h, s, t])
}

/// [`ColourModel::disc_picture`].
#[uniffi::export]
pub fn colour_model_disc_picture(model: ColourModel, h: f32, s: f32, t: f32, side: u32) -> Vec<u8> {
    model.disc_picture([h, s, t], side)
}

/// [`ColourModel::square_picture`].
#[uniffi::export]
pub fn colour_model_square_picture(model: ColourModel, hue: f32, side: u32) -> Vec<u8> {
    model.square_picture(hue, side)
}

/// How many pixels across Classic's square's picture in a model is for a square `side` pixels
/// across: as many, at most [`MODEL_SQUARE`].
#[uniffi::export]
pub fn colour_model_square_side(side: u32) -> u32 {
    side.clamp(1, MODEL_SQUARE)
}

/// The colour held as hue, saturation and brightness in OKLab: its lightness, a and b.
#[uniffi::export]
pub fn oklab_of_hsv(h: f32, s: f32, v: f32) -> Vec<f32> {
    oklab_of(hsv_to_rgb([h, s, v])).to_vec()
}

/// [`oklab_tracks`] at OKLab `l`, `a`, `b`.
#[uniffi::export]
pub fn oklab_value_tracks(l: f32, a: f32, b: f32) -> Vec<ColourTrack> {
    oklab_tracks([l, a, b]).to_vec()
}

/// OKLab `l`, `a`, `b` with channel `channel` slid to `to` ([`oklab_slid`]), as hue, saturation and
/// brightness; a grey keeps `hue`.
#[uniffi::export]
pub fn oklab_value_slid(l: f32, a: f32, b: f32, channel: u32, to: f32, hue: f32) -> Vec<f32> {
    let [h, s, v] = rgb_to_hsv(oklab_to_rgb(oklab_slid([l, a, b], channel as usize, to)));
    vec![if s > 0.0 { h } else { hue }, s, v]
}

// Okhsl, after Björn Ottosson's reference implementation
// (<https://bottosson.github.io/posts/colorpicker/>), under its MIT licence:
//
// Copyright (c) 2021 Björn Ottosson
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of this software
// and associated documentation files (the "Software"), to deal in the Software without
// restriction, including without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all copies or
// substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
// BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_gamma(c: f32) -> f32 {
    if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn linear_srgb_to_oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn oklab_to_linear_srgb([lab_l, a, b]: [f32; 3]) -> [f32; 3] {
    let l = (lab_l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (lab_l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (lab_l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// The most saturation (C/L) sRGB holds in OKLab's direction (`a`, `b`), a unit vector.
fn max_saturation(a: f32, b: f32) -> f32 {
    let (k, w) = if -1.881_703_3 * a - 0.809_364_9 * b > 1.0 {
        ([1.190_862_8, 1.765_767_3, 0.596_626_4, 0.755_152, 0.567_712_4], [4.076_741_7, -3.307_711_6, 0.230_969_94])
    } else if 1.814_441_1 * a - 1.194_452_8 * b > 1.0 {
        (
            [0.739_565_15, -0.459_544_04, 0.082_854_27, 0.125_410_7, 0.145_032_04],
            [-1.268_438, 2.609_757_4, -0.341_319_38],
        )
    } else {
        (
            [1.357_336_5, -0.009_157_99, -1.151_302_1, -0.505_596_06, 0.006_921_67],
            [-0.004_196_086_3, -0.703_418_6, 1.707_614_7],
        )
    };
    let mut s = k[0] + k[1] * a + k[2] * b + k[3] * a * a + k[4] * a * b;
    let k_l = 0.396_337_78 * a + 0.215_803_76 * b;
    let k_m = -0.105_561_346 * a - 0.063_854_17 * b;
    let k_s = -0.089_484_18 * a - 1.291_485_5 * b;
    let (l_, m_, s_) = (1.0 + s * k_l, 1.0 + s * k_m, 1.0 + s * k_s);
    let (l, m, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
    let (l1, m1, s1) = (3.0 * k_l * l_ * l_, 3.0 * k_m * m_ * m_, 3.0 * k_s * s_ * s_);
    let (l2, m2, s2) = (6.0 * k_l * k_l * l_, 6.0 * k_m * k_m * m_, 6.0 * k_s * k_s * s_);
    let f = w[0] * l + w[1] * m + w[2] * s3;
    let f1 = w[0] * l1 + w[1] * m1 + w[2] * s1;
    let f2 = w[0] * l2 + w[1] * m2 + w[2] * s2;
    s -= f * f1 / (f1 * f1 - 0.5 * f * f2);
    s
}

/// The lightness and chroma of the most chromatic sRGB colour in direction (`a`, `b`).
fn cusp(a: f32, b: f32) -> [f32; 2] {
    let s = max_saturation(a, b);
    let rgb = oklab_to_linear_srgb([1.0, s * a, s * b]);
    let l = (1.0 / rgb[0].max(rgb[1]).max(rgb[2])).cbrt();
    [l, l * s]
}

/// How far along the line from (`l0`, 0) to (`l1`, `c1`) the edge of sRGB lies, in direction
/// (`a`, `b`).
fn gamut_intersection(a: f32, b: f32, l1: f32, c1: f32, l0: f32, [cusp_l, cusp_c]: [f32; 2]) -> f32 {
    if (l1 - l0) * cusp_c - (cusp_l - l0) * c1 <= 0.0 {
        return cusp_c * l0 / (c1 * cusp_l + cusp_c * (l0 - l1));
    }
    let mut t = cusp_c * (l0 - 1.0) / (c1 * (cusp_l - 1.0) + cusp_c * (l0 - l1));
    let (dl, dc) = (l1 - l0, c1);
    let k_l = 0.396_337_78 * a + 0.215_803_76 * b;
    let k_m = -0.105_561_346 * a - 0.063_854_17 * b;
    let k_s = -0.089_484_18 * a - 1.291_485_5 * b;
    let (l_dt, m_dt, s_dt) = (dl + dc * k_l, dl + dc * k_m, dl + dc * k_s);
    let lum = l0 * (1.0 - t) + t * l1;
    let c = t * c1;
    let (l_, m_, s_) = (lum + c * k_l, lum + c * k_m, lum + c * k_s);
    let (l, m, s) = (l_.powi(3), m_.powi(3), s_.powi(3));
    let (ldt, mdt, sdt) = (3.0 * l_dt * l_ * l_, 3.0 * m_dt * m_ * m_, 3.0 * s_dt * s_ * s_);
    let (ldt2, mdt2, sdt2) = (6.0 * l_dt * l_dt * l_, 6.0 * m_dt * m_dt * m_, 6.0 * s_dt * s_dt * s_);
    let step = |w: [f32; 3]| {
        let v = w[0] * l + w[1] * m + w[2] * s - 1.0;
        let v1 = w[0] * ldt + w[1] * mdt + w[2] * sdt;
        let v2 = w[0] * ldt2 + w[1] * mdt2 + w[2] * sdt2;
        let u = v1 / (v1 * v1 - 0.5 * v * v2);
        if u >= 0.0 { -v * u } else { f32::MAX }
    };
    t += step([4.076_741_7, -3.307_711_6, 0.230_969_94])
        .min(step([-1.268_438, 2.609_757_4, -0.341_319_38]))
        .min(step([-0.004_196_086_3, -0.703_418_6, 1.707_614_7]));
    t
}

const K1: f32 = 0.206;
const K2: f32 = 0.03;
const K3: f32 = (1.0 + K1) / (1.0 + K2);

fn toe(x: f32) -> f32 {
    0.5 * (K3 * x - K1 + ((K3 * x - K1) * (K3 * x - K1) + 4.0 * K2 * K3 * x).sqrt())
}

fn toe_inv(x: f32) -> f32 {
    (x * x + K1 * x) / (K3 * (x + K2))
}

/// The chromas Okhsl's saturation runs through at lightness `l`, in direction (`a`, `b`): at 0.8 of
/// the way and at its most (C₀, C_mid, C_max).
fn chromas(l: f32, a: f32, b: f32) -> [f32; 3] {
    let cusp = cusp(a, b);
    let c_max = gamut_intersection(a, b, l, 1.0, l, cusp);
    let (st_s, st_t) = (cusp[1] / cusp[0], cusp[1] / (1.0 - cusp[0]));
    let k = c_max / (l * st_s).min((1.0 - l) * st_t);
    let s_mid = 0.115_169_93
        + 1.0
            / (7.447_789_7
                + 4.159_012_4 * b
                + a * (-2.195_573_5
                    + 1.751_984 * b
                    + a * (-2.137_049_5 - 10.023_01 * b + a * (-4.248_945_6 + 5.387_708 * b + 4.698_91 * a))));
    let t_mid = 0.112_396_42
        + 1.0
            / (1.613_203_2 - 0.681_243_8 * b
                + a * (0.403_706_12
                    + 0.901_481_2 * b
                    + a * (-0.270_879_43
                        + 0.612_239_9 * b
                        + a * (0.002_992_15 - 0.453_995_68 * b - 0.146_618_72 * a))));
    let (ca, cb) = (l * s_mid, (1.0 - l) * t_mid);
    let c_mid = 0.9 * k * (1.0 / (1.0 / ca.powi(4) + 1.0 / cb.powi(4))).sqrt().sqrt();
    let (ca, cb) = (l * 0.4, (1.0 - l) * 0.8);
    let c_0 = (1.0 / (1.0 / (ca * ca) + 1.0 / (cb * cb))).sqrt();
    [c_0, c_mid, c_max]
}

fn okhsl_to_srgb([h, s, l]: [f32; 3]) -> [f32; 3] {
    if l >= 1.0 {
        return [1.0; 3];
    }
    if l <= 0.0 {
        return [0.0; 3];
    }
    let (a, b) = ((TAU * h).cos(), (TAU * h).sin());
    let lum = toe_inv(l);
    let [c0, c_mid, c_max] = chromas(lum, a, b);
    let c = if s < 0.8 {
        let t = 1.25 * s;
        let k1 = 0.8 * c0;
        let k2 = 1.0 - k1 / c_mid;
        t * k1 / (1.0 - k2 * t)
    } else {
        let t = (s - 0.8) / 0.2;
        let k1 = 0.2 * c_mid * c_mid * 1.25 * 1.25 / c0;
        let k2 = 1.0 - k1 / (c_max - c_mid);
        c_mid + t * k1 / (1.0 - k2 * t)
    };
    let rgb = oklab_to_linear_srgb([lum, c * a, c * b]);
    rgb.map(|v| to_gamma(v.clamp(0.0, 1.0)).clamp(0.0, 1.0))
}

fn srgb_to_okhsl(rgb: [f32; 3]) -> [f32; 3] {
    let [lum, a, b] = linear_srgb_to_oklab(rgb.map(to_linear));
    let c = a.hypot(b);
    let l = toe(lum);
    if c < 1e-6 || lum <= 0.0 || lum >= 1.0 {
        return [0.0, 0.0, l.clamp(0.0, 1.0)];
    }
    let (a_, b_) = (a / c, b / c);
    let h = 0.5 + 0.5 * (-b).atan2(-a) / PI;
    let [c0, c_mid, c_max] = chromas(lum, a_, b_);
    let s = if c < c_mid {
        let k1 = 0.8 * c0;
        let k2 = 1.0 - k1 / c_mid;
        let t = c / (k1 + k2 * c);
        t * 0.8
    } else {
        let k1 = 0.2 * c_mid * c_mid * 1.25 * 1.25 / c0;
        let k2 = 1.0 - k1 / (c_max - c_mid);
        let t = (c - c_mid) / (k1 + k2 * (c - c_mid));
        0.8 + 0.2 * t
    };
    [h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f32; 3], b: [f32; 3], by: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < by)
    }

    fn of_hex(v: u32) -> [f32; 3] {
        [16, 8, 0].map(|k| ((v >> k) & 0xFF) as f32 / 255.0)
    }

    #[test]
    fn every_model_goes_to_rgb_and_back() {
        let colours = [
            [0.8, 0.2, 0.1],
            [0.1, 0.6, 0.3],
            [0.2, 0.3, 0.9],
            [0.95, 0.9, 0.2],
            [0.5, 0.5, 0.5],
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        ];
        for model in ColourModel::ALL {
            for rgb in colours {
                let back = model.to_rgb(model.of_rgb(rgb, 0.3));
                assert!(near(back, rgb, 2e-3), "{model:?} {rgb:?} came back {back:?}");
            }
        }
    }

    #[test]
    fn every_model_goes_round_the_whole_cube_and_back() {
        // every colour of a 9-step grid through the RGB cube, the primaries and their mixes among
        // them, comes back from each model as it went
        let steps: Vec<f32> = (0..=8).map(|k| k as f32 / 8.0).collect();
        for model in ColourModel::ALL {
            let (mut inside, mut edge) = (0.0f32, 0.0f32);
            for &r in &steps {
                for &g in &steps {
                    for &b in &steps {
                        let back = model.to_rgb(model.of_rgb([r, g, b], 0.0));
                        let off = [r, g, b].iter().zip(back).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
                        let worst =
                            if [r, g, b].iter().any(|c| *c == 0.0 || *c == 1.0) { &mut edge } else { &mut inside };
                        *worst = worst.max(off);
                    }
                }
            }
            assert!(inside < 4e-3, "{model:?}: off by {inside}");
            // on sRGB's edge (a channel at an end) Okhsl's saturation runs steep, and the darkest
            // blues come back a little off (0x000060 as 0x0A0057)
            let most = if model == ColourModel::Oklab { 0.05 } else { 4e-3 };
            assert!(edge < most, "{model:?}: off by {edge} on the edge");
        }
    }

    #[test]
    fn every_model_stays_inside_srgb_whatever_it_is_given() {
        // channels past their ends are taken at their ends, and every colour made is a colour
        let values = [-0.5, 0.0, 0.1, 0.5, 0.77, 1.0, 1.5];
        for model in ColourModel::ALL {
            for h in [-0.25, 0.0, 0.13, 0.5, 0.99, 1.0, 1.75] {
                for s in values {
                    for t in values {
                        let rgb = model.to_rgb([h, s, t]);
                        assert!(
                            rgb.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                            "{model:?} {:?} made {rgb:?}",
                            [h, s, t]
                        );
                        let clamped = model.to_rgb([h, s.clamp(0.0, 1.0), t.clamp(0.0, 1.0)]);
                        assert_eq!(rgb, clamped, "{model:?} {:?}", [h, s, t]);
                    }
                }
            }
        }
    }

    #[test]
    fn hsy_keeps_a_grey_as_the_hue_turns_and_the_luma_as_saturation_does() {
        for h in [0.0, 0.2, 0.55, 0.9] {
            let grey = ColourModel::Hsy.to_rgb([h, 0.0, 0.4]);
            assert!(near(grey, [0.4; 3], 1e-5));
            for s in [0.3, 1.0] {
                let y = to_gamma(linear_luma(ColourModel::Hsy.to_rgb([h, s, 0.4])));
                assert!((y - 0.4).abs() < 1e-3, "{h} {s}: {y}");
            }
        }
    }

    #[test]
    fn the_wheel_works_in_linear_light_as_kritas_does() {
        // two colours of the same saturation and luma on Krita's sliders: its wheel (Rec. 709 on
        // linear light) puts the green near the rim and the purple near the middle
        for (colour, wheel) in [(0x41864c, 0.74), (0x7d57a5, 0.33)] {
            let [_, ws, _] = linear_hsy_of(of_hex(colour), 0.0);
            assert!((ws - wheel).abs() < 0.03, "{colour:06x}: wheel {ws}");
        }
        let back = linear_hsy_to_rgb(linear_hsy_of([0.3, 0.6, 0.2], 0.0));
        assert!(near(back, [0.3, 0.6, 0.2], 1e-3), "{back:?}");
    }

    #[test]
    fn oklab_goes_to_rgb_and_back_and_brings_a_colour_outside_in() {
        for rgb in [[0.8, 0.2, 0.1], [0.1, 0.6, 0.3], [0.5, 0.5, 0.5]] {
            assert!(near(oklab_to_rgb(oklab_of(rgb)), rgb, 1e-3));
        }
        // far past sRGB's edge: it keeps its lightness and hue, and lands on the edge
        let [l, a, b] = oklab_of(oklab_to_rgb([0.6, 0.4, 0.4]));
        assert!((l - 0.6).abs() < 0.01, "{l}");
        assert!((b.atan2(a) - 0.4f32.atan2(0.4)).abs() < 0.02, "{a} {b}");
        assert!(oklab_to_rgb([0.6, 0.4, 0.4]).iter().any(|c| *c < 0.003 || *c > 0.997));
        // anywhere on the Value sliders, the colour made is a colour
        for l in [0.0, 0.3, 0.9, 1.0] {
            for ab in [-OKLAB_AB, -0.1, 0.25, OKLAB_AB] {
                let rgb = oklab_to_rgb([l, ab, -ab]);
                assert!(rgb.iter().all(|c| (0.0..=1.0).contains(c)), "{l} {ab}: {rgb:?}");
            }
        }
    }

    #[test]
    fn hls_runs_black_to_white_through_the_pure_hue() {
        assert!(near(ColourModel::Hls.to_rgb([0.0, 1.0, 0.5]), [1.0, 0.0, 0.0], 1e-6));
        assert!(near(ColourModel::Hls.to_rgb([0.3, 1.0, 1.0]), [1.0; 3], 1e-6));
        assert!(near(ColourModel::Hls.to_rgb([0.3, 1.0, 0.0]), [0.0; 3], 1e-6));
    }

    #[test]
    fn okhsl_matches_its_reference_and_greys_keep_their_hue() {
        // a mid grey sits at Okhsl lightness ~0.533 (sRGB 0.5 → OKLab L 0.598, through the toe)
        let [_, s, l] = srgb_to_okhsl([0.5; 3]);
        assert!(s < 1e-4 && (l - 0.533).abs() < 0.01, "{l}");
        // full saturation reaches sRGB's edge: a channel at 0 or 1
        let edge = okhsl_to_srgb([0.1, 1.0, 0.6]);
        assert!(edge.iter().any(|c| *c < 2e-3 || *c > 0.998), "{edge:?}");
        assert_eq!(ColourModel::Oklab.of_rgb([0.5; 3], 0.42)[0], 0.42);
    }

    #[test]
    fn a_colour_keeps_its_place_while_it_is_the_colour() {
        let place = ColourPlace::default();
        for model in [ColourModel::Hls, ColourModel::Hsy, ColourModel::Oklab] {
            // black, set at a hue and saturation: both kept while it stays black
            let coords = [0.3, 0.8, 0.0];
            let hsv = model.to_hsv(coords, 0.6);
            assert!(hsv[2] < 1e-3, "{model:?} {hsv:?}");
            place.keep_coords(model, coords, 0x00_0000);
            assert_eq!(place.coords_of(model, hsv, 0x00_0000), coords, "{model:?}");
            // another colour is worked out afresh, and a grey after it keeps the hue it had
            let red = place.coords_of(model, [0.0, 1.0, 1.0], 0xFF_0000);
            assert!(near(model.to_rgb(red), [1.0, 0.0, 0.0], 2e-3), "{model:?} {red:?}");
            let grey = place.coords_of(model, [0.9, 0.0, 0.5], 0x80_8080);
            assert_eq!((grey[0], grey[1]), (red[0], 0.0), "{model:?}");
        }
        // HSB's place is the colour as it is held
        assert_eq!(place.coords_of(ColourModel::Hsb, [0.1, 0.2, 0.3], 0x4C_4C4C), [0.1, 0.2, 0.3]);
        // another model's place isn't taken for this one's
        place.keep_coords(ColourModel::Hls, [0.5, 0.5, 0.5], 0x40_BFBF);
        let oklab = place.coords_of(ColourModel::Oklab, ColourModel::Hls.to_hsv([0.5, 0.5, 0.5], 0.0), 0x40_BFBF);
        assert_ne!(oklab, [0.5, 0.5, 0.5]);
    }

    #[test]
    fn hsb_s_sliders_are_as_they_were_and_another_model_s_follow_it() {
        let [hue, saturation, brightness] = ColourModel::Hsb.tracks([0.3, 0.4, 0.5]);
        assert_eq!(hue.place, hue_position(HueControl::Slider, 0.3));
        let slider: Vec<u32> = SLIDER_HUES.iter().map(|h| hex(hsv_to_rgb([h / 360.0, 1.0, 1.0]))).collect();
        assert_eq!(hue.colours, slider);
        assert_eq!(saturation.colours, vec![0x7F_7F7F, hex(hsv_to_rgb([0.3, 1.0, 1.0]))]);
        assert_eq!((saturation.place, brightness.place), (0.4, 0.5));
        assert_eq!(brightness.colours, vec![0x00_0000, 0xFF_FFFF]);
        // OKLab's hue runs evenly along its slider; its lightness runs black to white
        let coords = ColourModel::Oklab.of_rgb([0.8, 0.2, 0.1], 0.0);
        let [hue, saturation, lightness] = ColourModel::Oklab.tracks(coords);
        assert_eq!((hue.place, saturation.place, lightness.place), (coords[0], coords[1], coords[2]));
        assert_eq!((hue.colours.len(), saturation.colours.len(), lightness.colours.len()), (25, 9, 13));
        assert_eq!(lightness.colours.first(), Some(&0x00_0000));
        assert_eq!(lightness.colours.last(), Some(&0xFF_FFFF));
        // a slider let go somewhere sets its channel there (the hue at the slider's own spacing)
        assert_eq!(ColourModel::Oklab.slid(coords, 0, 0.25), [0.25, coords[1], coords[2]]);
        let hls = ColourModel::Hls.slid([0.1, 0.2, 0.3], 0, 0.25);
        assert_eq!(hls, [hue_at(HueControl::Slider, 0.25), 0.2, 0.3]);
        assert_eq!(ColourModel::Hsy.slid([0.1, 0.2, 0.3], 2, 0.9), [0.1, 0.2, 0.9]);
    }

    #[test]
    fn oklab_s_value_sliders_run_through_its_own_lightness_a_and_b() {
        let lab = oklab_of([0.8, 0.2, 0.1]);
        let [l, a, b] = oklab_tracks(lab);
        assert_eq!(l.place, lab[0]);
        assert!((a.place - (lab[1] / OKLAB_AB + 1.0) / 2.0).abs() < 1e-6);
        assert!((b.place - (lab[2] / OKLAB_AB + 1.0) / 2.0).abs() < 1e-6);
        assert_eq!((l.colours.first(), l.colours.last()), (Some(&0x00_0000), Some(&0xFF_FFFF)));
        // a slid from one end to the other: green-ish to red-ish at the colour's lightness
        let (green, red) = (a.colours.first().map(|&c| of_hex(c)), a.colours.last().map(|&c| of_hex(c)));
        assert!(green.zip(red).is_some_and(|(g, r)| g[1] > g[0] && r[0] > r[1]), "{green:?} {red:?}");
        assert_eq!(oklab_slid(lab, 0, 0.3), [0.3, lab[1], lab[2]]);
        assert_eq!(oklab_slid(lab, 2, 1.0), [lab[0], lab[1], OKLAB_AB]);
        // as the apps take it: black keeps the hue it had
        assert_eq!(oklab_value_slid(0.5, 0.0, 0.0, 0, 0.0, 0.7), vec![0.7, 0.0, 0.0]);
    }

    #[test]
    fn a_touch_on_the_disc_lands_under_it() {
        for model in ColourModel::ALL {
            for uv in [[0.0, 0.0], [0.3, -0.4], [-0.7, 0.2], [0.1, 0.9]] {
                let picked = model.disc_pick([0.2, 0.5, 0.5], uv);
                let [u, v] = model.disc_point(picked);
                assert!((u - uv[0]).abs() < 0.01 && (v - uv[1]).abs() < 0.01, "{model:?} {uv:?} came back {u} {v}");
            }
            // one past the edge is taken at the edge
            let [u, v] = model.disc_point(model.disc_pick([0.2, 0.5, 0.5], [2.0, 0.0]));
            assert!((u.hypot(v) - 1.0).abs() < 0.01, "{model:?}: {u} {v}");
        }
        // HSY′'s keeps the colour's linear luma; another's keeps its hue
        let coords = ColourModel::Hsy.of_rgb([0.2, 0.5, 0.3], 0.0);
        let luma = linear_luma(ColourModel::Hsy.to_rgb(coords));
        let picked = ColourModel::Hsy.disc_pick(coords, [-0.5, 0.1]);
        assert!((linear_luma(ColourModel::Hsy.to_rgb(picked)) - luma).abs() < 2e-3);
        assert_eq!(ColourModel::Oklab.disc_pick([0.42, 0.1, 0.1], [0.5, 0.5])[0], 0.42);
        // the ring: HSB's own spacing for a model with the RGB hue, even for OKLab
        assert_eq!(ColourModel::Hls.ring_hue(0.3), hue_at(HueControl::Ring, 0.3));
        assert_eq!(ColourModel::Oklab.ring_hue(0.3), 0.3);
        assert!((ColourModel::Hsy.ring_turn(ColourModel::Hsy.ring_hue(0.6)) - 0.6).abs() < 1e-3);
        assert_eq!(colour_model_ring(ColourModel::Hsb, 49).first(), Some(&0xFF_0000));
        assert_eq!(colour_model_ring(ColourModel::Hls, 49), colour_model_ring(ColourModel::Hsb, 49));
    }

    #[test]
    fn the_pictures_show_the_model() {
        let pixel = |picture: &[u8], side: usize, x: usize, y: usize| picture[(y * side + x) * 4..][..4].to_vec();
        for model in [ColourModel::Hls, ColourModel::Hsy, ColourModel::Oklab] {
            // Classic's square: white at the top left, black at the bottom right
            let square = model.square_picture(0.0, 16);
            assert_eq!(square.len(), 16 * 16 * 4);
            assert_eq!(pixel(&square, 16, 0, 0), [255; 4], "{model:?}");
            assert_eq!(pixel(&square, 16, 15, 15), [0, 0, 0, 255], "{model:?}");
            // the disc: clear outside, opaque inside, its top lighter than its bottom
            let disc = model.disc_picture([0.0, 0.5, 0.5], 64);
            assert_eq!(disc.len(), 64 * 64 * 4);
            assert_eq!(pixel(&disc, 64, 0, 0), [0; 4], "{model:?}");
            assert_eq!(pixel(&disc, 64, 32, 32)[3], 255, "{model:?}");
        }
        // its square stays small however big it is drawn
        assert_eq!(ColourModel::Oklab.square_picture(0.2, 4000).len(), (MODEL_SQUARE * MODEL_SQUARE * 4) as usize);
        // HLS's middle row is the pure hue at the right, where HSB's top row has it
        let hls = ColourModel::Hls.square_picture(0.0, 16);
        let pure = pixel(&hls, 16, 15, 7);
        assert!(pure[0] > 220 && pure[1] < 40 && pure[2] < 40, "{pure:?}");
        // OKLab's disc runs light at the top to dark at the bottom; HSB's is the disc picker's own
        let disc = ColourModel::Oklab.disc_picture([0.1, 0.5, 0.5], 64);
        assert!(pixel(&disc, 64, 32, 3)[1] > 200 && pixel(&disc, 64, 32, 60)[1] < 40);
        assert_eq!(ColourModel::Hsb.disc_picture([0.6, 0.0, 0.0], 32), crate::pickers::disc_picture(0.6, 32));
        // HSY′'s wheel is drawn for the colour's linear luma, another's for its hue
        let (dark, light) = (ColourModel::Hsy.of_rgb([0.2; 3], 0.0), ColourModel::Hsy.of_rgb([0.7; 3], 0.0));
        assert_ne!(ColourModel::Hsy.disc_key(dark), ColourModel::Hsy.disc_key(light));
        assert_eq!(ColourModel::Oklab.disc_key([0.25, 0.1, 0.9]), 2500);
    }

    #[test]
    fn the_names_are_kept_and_read_back() {
        for model in ColourModel::ALL {
            assert_eq!(colour_model_of_key(colour_model_key(model)), Some(model));
        }
        assert_eq!(ColourModel::of_key("cmyk"), None);
        assert_eq!(colour_model_letters(ColourModel::Hsy), vec!["H", "S", "Y\u{2032}"]);
        assert_eq!(colour_models(), ColourModel::ALL.to_vec());
    }
}
