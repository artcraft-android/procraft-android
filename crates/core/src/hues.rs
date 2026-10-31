//! The colour panel's hue controls, which are not linear in hue: the hues (degrees) measured at
//! evenly spaced points along each, a position between them interpolating.

/// A hue control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum HueControl {
    /// The classic slider (and the value rows' hue): 25 points.
    Slider,
    /// The disc's ring: 49 points, every 7.5 degrees anticlockwise from the right.
    Ring,
}

/// The classic slider's hues (degrees) at its 25 evenly spaced points.
pub const SLIDER_HUES: [f32; 25] = [
    0.0, 0.2, 8.0, 17.7, 27.3, 36.4, 44.2, 52.7, 80.4, 112.8, 120.5, 120.5, 133.3, 154.3, 178.2, 196.1, 211.1, 226.1,
    241.1, 259.7, 275.1, 291.7, 310.0, 340.2, 360.0,
];

/// The disc's ring's hues (degrees) at its 49 evenly spaced points, anticlockwise from the right.
pub const RING_HUES: [f32; 49] = [
    0.0, 0.2, 0.2, 0.2, 4.5, 13.2, 18.9, 23.7, 27.9, 32.4, 36.4, 40.9, 44.6, 49.1, 53.1, 57.2, 61.4, 74.2, 88.6, 114.8,
    120.5, 120.5, 120.5, 120.5, 134.8, 149.9, 162.2, 173.4, 180.0, 186.9, 195.4, 203.4, 211.4, 219.4, 227.5, 235.3,
    242.1, 251.1, 258.5, 268.0, 276.5, 285.3, 293.6, 300.2, 303.3, 318.2, 333.7, 351.2, 360.0,
];

fn stops(control: HueControl) -> &'static [f32] {
    match control {
        HueControl::Slider => &SLIDER_HUES,
        HueControl::Ring => &RING_HUES,
    }
}

/// The hues (degrees) at `control`'s evenly spaced points, for drawing it.
#[uniffi::export]
pub fn hue_stops(control: HueControl) -> Vec<f32> {
    stops(control).to_vec()
}

/// The hue (0..1) at `position` (0..1) along `control`.
#[uniffi::export]
pub fn hue_at(control: HueControl, position: f32) -> f32 {
    let stops = stops(control);
    let p = position.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (p as usize).min(stops.len() - 2);
    match (stops.get(i), stops.get(i + 1)) {
        (Some(a), Some(b)) => (a + (b - a) * (p - i as f32)) / 360.0,
        _ => 0.0,
    }
}

/// Where `hue` (0..1) sits (0..1) along `control`.
#[uniffi::export]
pub fn hue_position(control: HueControl, hue: f32) -> f32 {
    let stops = stops(control);
    let h = hue.clamp(0.0, 1.0) * 360.0;
    for (i, w) in stops.windows(2).enumerate() {
        if let [a, b] = w
            && h <= *b
            && b > a
        {
            return (i as f32 + ((h - a) / (b - a)).clamp(0.0, 1.0)) / (stops.len() - 1) as f32;
        }
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hue_goes_to_its_place_and_back() {
        for control in [HueControl::Slider, HueControl::Ring] {
            for k in 0..=36 {
                let hue = k as f32 / 36.0;
                let back = hue_at(control, hue_position(control, hue));
                assert!((back - hue).abs() < 1e-4, "{control:?} {hue} came back {back}");
            }
        }
        // the slider's red at the start, its green a little before the middle
        assert_eq!(hue_at(HueControl::Slider, 0.0), 0.0);
        assert!((hue_position(HueControl::Slider, 120.5 / 360.0) - 10.0 / 24.0).abs() < 1e-4);
    }
}
