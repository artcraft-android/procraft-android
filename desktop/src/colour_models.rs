//! The colour models the Colors panel's disc and Value sliders can work in: a hue, a saturation
//! and a third (brightness, lightness, luma or perceptual lightness), each 0..1, over sRGB values.
//!
//! - HSB: hue, saturation and brightness (HSV by another name), the panel's own.
//! - HLS: hue, saturation and lightness, white at the top and black at the bottom.
//! - HSY′: hue and saturation as Krita's sliders take them (on the gamma-encoded values), and Y′ the
//!   luma of linear light (Rec. 709), sRGB-encoded, as Krita's luma bar shows it: colours of one Y′
//!   are one grey in the grey view, and turning the hue keeps it.
//! - OKLab: Björn Ottosson's Okhsl, OKLab's hue and lightness (with his toe, so its middle is a
//!   perceptual middle grey) and a saturation reaching the edge of sRGB at 1.

use std::f32::consts::{PI, TAU};

use crate::colours::{hsv_to_rgb, rgb_to_hsv};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColourModel {
    #[default]
    Hsb,
    Hls,
    Hsy,
    Oklab,
}

/// Rec. 709 luma weights, as Krita's HSY′ takes them.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

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

    /// Its channels (hue, saturation, third; 0..1) as sRGB values.
    pub fn to_rgb(self, [h, s, t]: [f32; 3]) -> [f32; 3] {
        let (s, t) = (s.clamp(0.0, 1.0), t.clamp(0.0, 1.0));
        match self {
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
        }
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

    /// The third channel the hue ring shows its hues at (the purest colours).
    pub fn ring_third(self, h: f32) -> f32 {
        match self {
            ColourModel::Hsb => 1.0,
            ColourModel::Hls => 0.5,
            ColourModel::Hsy => to_gamma(linear_luma(hsv_to_rgb([h, 1.0, 1.0]))),
            ColourModel::Oklab => 0.7,
        }
    }
}

/// HSY′ worked out on linear light, as Krita's Advanced Color Selector wheel does (its sliders
/// work on the gamma-encoded values, as [`ColourModel::Hsy`] does): sRGB values as hue,
/// saturation and linear luma, a grey keeping `hue`.
pub fn linear_hsy_of(rgb: [f32; 3], hue: f32) -> [f32; 3] {
    encoded_hsy_of(rgb.map(|c| to_linear(c.clamp(0.0, 1.0))), hue)
}

/// Hue, saturation and linear luma, as Krita's wheel takes them, as sRGB values.
pub fn linear_hsy_to_rgb(hsy: [f32; 3]) -> [f32; 3] {
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
pub fn linear_luma(rgb: [f32; 3]) -> f32 {
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

fn luma([r, g, b]: [f32; 3]) -> f32 {
    LUMA[0] * r + LUMA[1] * g + LUMA[2] * b
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
        let hex = |v: u32| [16, 8, 0].map(|k| ((v >> k) & 0xFF) as f32 / 255.0);
        // two colours of the same saturation and luma on Krita's sliders: its wheel (Rec. 709 on
        // linear light) puts the green near the rim and the purple near the middle
        for (colour, wheel) in [(0x41864c, 0.74), (0x7d57a5, 0.33)] {
            let [_, ws, _] = linear_hsy_of(hex(colour), 0.0);
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
        // far past sRGB's edge: it keeps its lightness, and lands on the edge
        let [l, _, _] = oklab_of(oklab_to_rgb([0.6, 0.4, 0.4]));
        assert!((l - 0.6).abs() < 0.01, "{l}");
        assert!(oklab_to_rgb([0.6, 0.4, 0.4]).iter().any(|c| *c < 0.003 || *c > 0.997));
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
}
