//! A HarmonyOS pen's angles as the core takes them. ArkUI's touch events give a pen's tilt along
//! the panel's two axes (`tiltX`, `tiltY`: degrees, -90..90, the angle of the pen's projection on
//! the XZ and YZ planes from upright); the core takes how far the pen leans from upright and which
//! way its top leans, clockwise from up (`PenSample::tilt`, `PenSample::orientation`), as the
//! iPad app works out from its pen's altitude and azimuth.
//!
//! Which way each axis's positive angle leans (right and down, as the platform's reference says)
//! is still to be checked with a real pen.

use std::f64::consts::{FRAC_PI_2, TAU};

/// The most either tilt is taken as, degrees: at 90 the pen lies flat and its tangent has no end.
const MOST_TILT: f64 = 89.9;

/// How far the pen leans from upright and which way its top leans (clockwise from up, 0..2π), both
/// radians, from its tilts along the panel's x and y axes in degrees (positive leaning right and
/// down). A pen that gives no tilt (both 0, or not numbers) stands upright.
pub(crate) fn lean(tilt_x: f64, tilt_y: f64) -> (f32, f32) {
    let along =
        |degrees: f64| if degrees.is_finite() { degrees.clamp(-MOST_TILT, MOST_TILT).to_radians().tan() } else { 0.0 };
    let (x, y) = (along(tilt_x), along(tilt_y));
    if x == 0.0 && y == 0.0 {
        return (0.0, 0.0);
    }
    // the pen's top over the panel is (tan x, tan y) a unit above it: its lean from upright is the
    // angle of that point from straight up, and its direction that point's, clockwise from up
    // (screen y grows downwards)
    let tilt = x.hypot(y).atan();
    let mut orientation = x.atan2(-y);
    if orientation < 0.0 {
        orientation += TAU;
    }
    (tilt.min(FRAC_PI_2) as f32, orientation as f32)
}

/// The pressure of a sample, 0..1. ArkUI's pressure is 0..1 but over 1 on some panels.
pub(crate) fn pressure(value: f64) -> f32 {
    if value.is_finite() { value.clamp(0.0, 1.0) as f32 } else { 0.0 }
}

/// An event's time, nanoseconds since boot (ArkUI's timestamps), in the milliseconds the core's
/// samples and waits run on.
pub(crate) fn time_ms(nanoseconds: f64) -> f64 {
    if nanoseconds.is_finite() { nanoseconds / 1_000_000.0 } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn an_upright_pen_has_no_lean() {
        assert_eq!(lean(0.0, 0.0), (0.0, 0.0));
        assert_eq!(lean(f64::NAN, f64::INFINITY), (0.0, 0.0));
    }

    #[test]
    fn a_pen_leaning_along_one_axis_leans_that_far_that_way() {
        let (tilt, orientation) = lean(45.0, 0.0);
        assert!(near(tilt, FRAC_PI_4) && near(orientation, FRAC_PI_2), "right: {tilt} {orientation}");
        let (tilt, orientation) = lean(0.0, 30.0);
        assert!(near(tilt, PI / 6.0) && near(orientation, PI), "down: {tilt} {orientation}");
        let (_, orientation) = lean(-20.0, 0.0);
        assert!(near(orientation, 3.0 * FRAC_PI_2), "left: {orientation}");
        let (_, orientation) = lean(0.0, -20.0);
        assert!(near(orientation, 0.0), "up: {orientation}");
    }

    #[test]
    fn a_pen_leaning_across_both_axes_leans_more_than_either() {
        // 45 along both is the diagonal, atan(sqrt 2) from upright (54.7 degrees), not 45
        let (tilt, orientation) = lean(45.0, 45.0);
        assert!(near(tilt, 2f32.sqrt().atan()), "{tilt}");
        assert!(near(orientation, 3.0 * FRAC_PI_4), "down and right: {orientation}");
    }

    #[test]
    fn a_flat_pen_stops_at_lying_down() {
        let (tilt, _) = lean(90.0, 90.0);
        assert!(tilt <= FRAC_PI_2 && tilt > 1.5, "{tilt}");
    }

    #[test]
    fn pressure_and_time_are_kept_in_range() {
        assert_eq!(pressure(1.4), 1.0);
        assert_eq!(pressure(-0.2), 0.0);
        assert_eq!(pressure(f64::NAN), 0.0);
        assert!((pressure(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((time_ms(16_000_000.0) - 16.0).abs() < 1e-9);
    }
}
