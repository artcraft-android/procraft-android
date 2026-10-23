//! Colour profiles: which RGB space a canvas's numbers are in, and colours moved between spaces.
//!
//! Every profile here encodes with the sRGB tone curve. The sRGB ones share sRGB's primaries (they
//! differ only in how their files describe rendering), so moving between them changes nothing;
//! Display P3 has wider primaries, the same white (D65) and the same curve.

/// A canvas's colour profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Profile {
    /// sRGB IEC61966-2.1.
    #[default]
    Srgb,
    DisplayP3,
    SrgbV4Appearance,
    SrgbV4Preference,
    SrgbV4PreferenceDisplayClass,
}

/// Linear Display P3 to linear sRGB (both D65).
const P3_TO_SRGB: [[f32; 3]; 3] =
    [[1.224_940_2, -0.224_940_4, 0.0], [-0.042_056_955, 1.042_057, 0.0], [-0.019_637_555, -0.078_636_05, 1.098_273_6]];

/// Linear sRGB to linear Display P3.
const SRGB_TO_P3: [[f32; 3]; 3] =
    [[0.822_462_1, 0.177_538, 0.0], [0.033_194_2, 0.966_805_8, 0.0], [0.017_082_67, 0.072_397_4, 0.910_519_9]];

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

impl Profile {
    /// The RGB profiles, in the order lists show them.
    pub const ALL: [Profile; 5] = [
        Profile::DisplayP3,
        Profile::Srgb,
        Profile::SrgbV4Appearance,
        Profile::SrgbV4Preference,
        Profile::SrgbV4PreferenceDisplayClass,
    ];

    /// Its name, as lists and files show it.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Srgb => "sRGB IEC61966-2.1",
            Profile::DisplayP3 => "Display P3",
            Profile::SrgbV4Appearance => "sRGB v4 ICC Appearance",
            Profile::SrgbV4Preference => "sRGB v4 ICC Preference",
            Profile::SrgbV4PreferenceDisplayClass => "sRGB v4 ICC Preference Display Class",
        }
    }

    /// The profile named `name`, if it is one.
    pub fn from_name(name: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Whether it has Display P3's wider primaries (else sRGB's).
    pub fn is_p3(self) -> bool {
        self == Profile::DisplayP3
    }

    /// The matrix taking linear light in this profile to linear light in `to`.
    pub fn matrix_to(self, to: Profile) -> [[f32; 3]; 3] {
        match (self.is_p3(), to.is_p3()) {
            (true, false) => P3_TO_SRGB,
            (false, true) => SRGB_TO_P3,
            _ => IDENTITY,
        }
    }

    /// `rgb` (encoded, 0..1) in this profile as encoded values in `to`, clipped to its gamut.
    pub fn convert(self, rgb: [f32; 3], to: Profile) -> [f32; 3] {
        if self.is_p3() == to.is_p3() {
            return rgb;
        }
        apply(self.matrix_to(to), rgb.map(decode)).map(|c| encode(c.clamp(0.0, 1.0)))
    }

    /// An 8-bit RGB colour (0xRRGGBB) in this profile, in `to`.
    pub fn convert_rgb(self, rgb: u32, to: Profile) -> u32 {
        let c = [(rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF].map(|v| v as f32 / 255.0);
        let [r, g, b] = self.convert(c, to).map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u32);
        (r << 16) | (g << 8) | b
    }

    /// Premultiplied RGBA8 pixels in this profile turned into `to` in place, looking the same
    /// where `to` can show them and clipped where it can't.
    pub fn convert_pixels(self, pixels: &mut [u8], to: Profile) {
        if self.is_p3() == to.is_p3() {
            return;
        }
        let m = self.matrix_to(to);
        let linear: [f32; 256] = std::array::from_fn(|i| decode(i as f32 / 255.0));
        let encoded: Vec<f32> = (0..=ENCODE_STEPS).map(|i| encode(i as f32 / ENCODE_STEPS as f32)).collect();
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = u32::from(px[3]);
            if a == 0 {
                continue;
            }
            let straight = [px[0], px[1], px[2]]
                .map(|c| linear.get(((u32::from(c) * 255 + a / 2) / a).min(255) as usize).copied().unwrap_or(1.0));
            let [r, g, b] = apply(m, straight).map(|v| {
                let e = encoded.get((v.clamp(0.0, 1.0) * ENCODE_STEPS as f32).round() as usize).copied().unwrap_or(1.0);
                (e * a as f32).round().clamp(0.0, 255.0) as u8
            });
            [px[0], px[1], px[2]] = [r, g, b];
        }
    }
}

/// How finely [`Profile::convert_pixels`] steps linear light when encoding it: about a fifth of an
/// 8-bit step near black, where the curve is steepest.
const ENCODE_STEPS: usize = 16384;

/// The matrix `m` times `v`.
pub fn apply(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

/// The sRGB tone curve undone: an encoded value (0..1) as linear light.
pub fn decode(c: f32) -> f32 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Linear light (0..1) encoded with the sRGB tone curve.
pub fn encode(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_go_between_the_profiles() {
        // white, black and greys are the same in both
        for g in [0.0, 0.25, 0.5, 1.0] {
            let c = Profile::Srgb.convert([g, g, g], Profile::DisplayP3);
            assert!(c.iter().all(|v| (v - g).abs() < 1e-3), "{c:?}");
        }
        // sRGB's red is inside P3, and comes back
        let red = Profile::Srgb.convert([1.0, 0.0, 0.0], Profile::DisplayP3);
        assert!(
            (red[0] - 0.917).abs() < 0.01 && (red[1] - 0.200).abs() < 0.01 && (red[2] - 0.139).abs() < 0.01,
            "{red:?}"
        );
        let back = Profile::DisplayP3.convert(red, Profile::Srgb);
        assert!((back[0] - 1.0).abs() < 2e-3 && back[1] < 2e-3 && back[2] < 2e-3, "{back:?}");
        // P3's red is outside sRGB: clipped
        let clipped = Profile::DisplayP3.convert([1.0, 0.0, 0.0], Profile::Srgb);
        assert!((clipped[0] - 1.0).abs() < 1e-5 && clipped[1] == 0.0 && clipped[2] == 0.0, "{clipped:?}");
        // between the sRGB ones nothing moves
        assert_eq!(Profile::Srgb.convert([0.3, 0.6, 0.9], Profile::SrgbV4Preference), [0.3, 0.6, 0.9]);
        assert_eq!(Profile::Srgb.convert_rgb(0xFF0000, Profile::DisplayP3), 0xEA3323);
    }

    #[test]
    fn pixels_go_between_the_profiles() {
        // opaque sRGB red, half-covered sRGB red, a grey and nothing, premultiplied
        let mut px = vec![255, 0, 0, 255, 128, 0, 0, 128, 90, 90, 90, 200, 0, 0, 0, 0];
        Profile::Srgb.convert_pixels(&mut px, Profile::DisplayP3);
        assert_eq!(&px[0..4], &[0xEA, 0x33, 0x23, 255]);
        // the half-covered red is the same colour, half as much
        assert!(px[4].abs_diff(0x75) <= 1 && px[5].abs_diff(0x1A) <= 1 && px[6].abs_diff(0x12) <= 1, "{px:?}");
        assert_eq!(px[7], 128);
        assert!(px[8..11].iter().all(|&c| c.abs_diff(90) <= 1), "{px:?}");
        assert_eq!(&px[12..16], &[0, 0, 0, 0]);
        // and back
        Profile::DisplayP3.convert_pixels(&mut px, Profile::Srgb);
        assert!(px[0] >= 254 && px[1] <= 1 && px[2] <= 1, "{px:?}");
        // between the sRGB ones nothing moves
        let mut same = vec![10, 20, 30, 40];
        Profile::Srgb.convert_pixels(&mut same, Profile::SrgbV4Appearance);
        assert_eq!(same, [10, 20, 30, 40]);
    }

    #[test]
    fn names_go_both_ways() {
        for p in Profile::ALL {
            assert_eq!(Profile::from_name(p.name()), Some(p));
        }
        assert_eq!(Profile::from_name("Adobe RGB"), None);
    }
}
