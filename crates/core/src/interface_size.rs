//! The Interface size preference: the whole interface shown a little bigger or smaller than it is
//! measured, for a screen it doesn't suit. At 100% it is as it always was. The canvas, brushes and
//! gestures don't follow it, only what the interface draws: see
//! [`crate::CanvasSession::set_interface_size`].

/// What it can be set to and the steps it moves in, percent.
const LEAST: f32 = 75.0;
const MOST: f32 = 125.0;
const STEP: f32 = 5.0;

/// What a bigger size must leave the interface (points): the smallest window the desktop opens,
/// which every panel fits. A window with less shows the interface at 100%.
const LEAST_WIDTH: f32 = 640.0;
const LEAST_HEIGHT: f32 = 480.0;

/// `percent` on the nearest step, as a size.
fn on_step(percent: f32) -> f32 {
    (percent / STEP).round() * STEP / 100.0
}

/// The size at `along` (0..1) of the slider's way.
#[uniffi::export]
pub fn interface_size_at(along: f32) -> f32 {
    on_step(LEAST + along.clamp(0.0, 1.0) * (MOST - LEAST))
}

/// How far along its slider (0..1) `size` is.
#[uniffi::export]
pub fn interface_size_along(size: f32) -> f32 {
    ((size * 100.0 - LEAST) / (MOST - LEAST)).clamp(0.0, 1.0)
}

/// A size read back from where it was kept: on a step and in range, and 100% if it isn't a number.
#[uniffi::export]
pub fn interface_size_kept(kept: f32) -> f32 {
    if kept.is_finite() { on_step((kept * 100.0).clamp(LEAST, MOST)) } else { 1.0 }
}

/// The size to show in a window `width` × `height` points (at 100%) with `chosen` set: as chosen,
/// but a bigger one only as far as it leaves the interface the room it needs, on a step so a window
/// dragged smaller doesn't lay everything out again at each pixel.
#[uniffi::export]
pub fn interface_size_fitted(chosen: f32, width: f32, height: f32) -> f32 {
    let room = (width / LEAST_WIDTH).min(height / LEAST_HEIGHT) * 100.0;
    chosen.min(((room / STEP).floor() * STEP / 100.0).max(1.0))
}

#[cfg(test)]
mod tests {
    use procraft_engine::geom::Affine;
    use procraft_engine::view::View;

    use super::*;
    use crate::{CanvasSession, PenSample, PointerKind, TransformTouch};

    #[test]
    fn the_slider_moves_in_steps_with_100_in_the_middle() {
        assert_eq!(interface_size_at(0.0), 0.75);
        assert_eq!(interface_size_at(0.5), 1.0);
        assert_eq!(interface_size_at(1.0), 1.25);
        // the nearest step, from anywhere along it
        assert_eq!(interface_size_at(0.51), 1.0);
        assert_eq!(interface_size_at(0.56), 1.05);
        assert_eq!(interface_size_along(1.0), 0.5);
        for step in 0..=10 {
            let along = step as f32 / 10.0;
            assert!((interface_size_along(interface_size_at(along)) - along).abs() < 1e-5, "{along}");
        }
    }

    #[test]
    fn a_kept_size_comes_back_usable() {
        assert_eq!(interface_size_kept(1.0), 1.0);
        assert_eq!(interface_size_kept(0.85), 0.85);
        assert_eq!(interface_size_kept(0.86), 0.85);
        assert_eq!(interface_size_kept(9.0), 1.25);
        assert_eq!(interface_size_kept(0.0), 0.75);
        assert_eq!(interface_size_kept(f32::NAN), 1.0);
        assert_eq!(interface_size_kept(f32::INFINITY), 1.0);
    }

    #[test]
    fn a_bigger_size_goes_no_further_than_the_window_has_room() {
        // room to spare: as chosen
        assert_eq!(interface_size_fitted(1.25, 1440.0, 900.0), 1.25);
        assert_eq!(interface_size_fitted(1.25, 800.0, 600.0), 1.25);
        // the smallest window keeps the interface it always had
        assert_eq!(interface_size_fitted(1.25, 640.0, 480.0), 1.0);
        // the shorter side decides, by steps
        assert_eq!(interface_size_fitted(1.25, 1440.0, 540.0), 1.1);
        assert_eq!(interface_size_fitted(1.25, 710.0, 900.0), 1.1);
        // a window smaller than that never goes below 100%
        assert_eq!(interface_size_fitted(1.25, 500.0, 400.0), 1.0);
    }

    /// A session shown 1:1 at twice the pixels a point, the interface at `size`, in a transform of
    /// a box from (100, 100) to (500, 500) whose corners move on their own.
    fn transforming(size: f32) -> std::sync::Arc<CanvasSession> {
        use procraft_engine::transform::{Transform, TransformMode};
        let s = CanvasSession::new(1024, 1024);
        s.set_display_scale(2.0);
        s.set_interface_size(size);
        {
            let mut st = s.shared.lock();
            st.view = Some(View::from_affine(Affine::IDENTITY));
            st.transform = Some(Transform::new(100.0, 100.0, 400.0, 400.0, Vec::new()));
            st.transform_mode = TransformMode::Distort;
        }
        s.set_touch_transforming(Some(TransformTouch { advanced_mesh: false }));
        s
    }

    fn at(x: f32, y: f32, ms: f64) -> Vec<PenSample> {
        vec![PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: ms, roll: 0.0 }]
    }

    fn corners(s: &CanvasSession) -> Vec<f32> {
        s.shared.lock().transform_box().map(|b| b.corners).unwrap_or_default()
    }

    /// A touch from `from` along x, through `way` pixels each time: a drag counts from where it
    /// was recognised.
    fn drag(s: &CanvasSession, (x, y): (f32, f32), way: &[f32]) {
        s.touch_down(1, PointerKind::Finger, at(x, y, 0.0));
        let mut ms = 0.0;
        let mut to = x;
        for step in way {
            ms += 16.0;
            to += step;
            s.touch_move(1, at(to, y, ms));
        }
        s.touch_up(1, at(to, y, ms + 16.0));
    }

    #[test]
    fn a_node_takes_a_touch_as_far_out_as_the_interface_shows_it() {
        // 85 pixels out from the top left corner: past its reach at 100%, in it at 125%
        let from = (40.0, 40.0);
        let way = [-31.0, -40.0];
        let s = transforming(1.0);
        let before = corners(&s);
        drag(&s, from, &way);
        let moved = corners(&s);
        assert_eq!(moved[2] - before[2], -40.0, "it moved the box, its top right corner too");

        let s = transforming(1.25);
        let before = corners(&s);
        drag(&s, from, &way);
        let moved = corners(&s);
        assert_eq!(moved[0] - before[0], -40.0, "the top left corner went with the touch");
        assert_eq!(moved[2], before[2], "the top right corner stayed");
    }

    #[test]
    fn how_far_a_touch_goes_before_it_drags_stays_as_it_is() {
        // the first step is past 15 points at twice the pixels a point, and short of them at 125% of
        // that: the second counts whole
        let s = transforming(1.25);
        let before = corners(&s);
        drag(&s, (300.0, 300.0), &[34.0, 10.0]);
        assert_eq!(corners(&s)[0] - before[0], 10.0);
    }

    #[test]
    fn a_crop_s_frame_takes_a_touch_as_far_out_as_the_interface_shows_it() {
        let width_after_drag = |size: f32| {
            let s = CanvasSession::new(1000, 1000);
            s.set_display_scale(2.0);
            s.set_interface_size(size);
            s.crop_begin();
            s.shared.lock().view = Some(View::from_affine(Affine::IDENTITY));
            // 62 pixels in from the top left corner: past its reach at 100%, in it at 125%
            s.crop_press(44.0, 44.0);
            s.crop_drag(144.0, 144.0);
            s.crop_release();
            s.crop_shown().unwrap().width
        };
        assert_eq!(width_after_drag(1.0), 1000.0, "it moved the frame whole");
        assert_eq!(width_after_drag(1.25), 900.0, "it took the corner");
    }

    #[test]
    fn a_size_that_is_no_size_is_left_out() {
        let s = CanvasSession::new(8, 8);
        for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            s.set_interface_size(size);
            assert_eq!(s.shared.lock().interface_size, 1.0, "{size}");
        }
    }

    #[test]
    fn a_smaller_size_fits_any_window() {
        assert_eq!(interface_size_fitted(1.0, 1440.0, 900.0), 1.0);
        assert_eq!(interface_size_fitted(0.75, 1440.0, 900.0), 0.75);
        assert_eq!(interface_size_fitted(0.75, 500.0, 400.0), 0.75);
        // a window not known yet leaves it alone
        assert_eq!(interface_size_fitted(0.9, 0.0, 0.0), 0.9);
        assert_eq!(interface_size_fitted(1.25, f32::NAN, f32::NAN), 1.0);
    }
}
