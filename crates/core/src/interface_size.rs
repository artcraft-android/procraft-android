//! How big the interface shows beside what is measured: a number, which a patch gives or nothing
//! does (1). The canvas, brushes and gestures don't follow it, only what the interface draws: see
//! [`crate::CanvasSession::set_interface_size`].

/// What every panel needs (points): the smallest window the desktop opens.
const LEAST_WIDTH: f32 = 640.0;
const LEAST_HEIGHT: f32 = 480.0;

/// The most the interface may show at in a window `width` × `height` points at 1: as far as it
/// leaves the panels what they need, so a patch can't push its own Unapply out of reach. A window
/// with less than that shows as measured. A patch that fits itself never gets here.
pub(crate) fn most_for(width: f32, height: f32) -> f32 {
    (width / LEAST_WIDTH).min(height / LEAST_HEIGHT).max(1.0)
}

#[cfg(test)]
mod tests {
    use procraft_engine::geom::Affine;
    use procraft_engine::view::View;

    use super::most_for;
    use crate::{CanvasSession, PenSample, PointerKind, TransformTouch};

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
    fn the_panels_keep_what_they_need() {
        assert_eq!(most_for(1280.0, 960.0), 2.0);
        assert_eq!(most_for(960.0, 960.0), 1.5);
        assert_eq!(most_for(1280.0, 600.0), 1.25);
        assert_eq!(most_for(640.0, 480.0), 1.0);
        // less than they need, or a window not known yet, shows as measured
        assert_eq!(most_for(500.0, 400.0), 1.0);
        assert_eq!(most_for(0.0, 0.0), 1.0);
        assert_eq!(most_for(f32::NAN, f32::NAN), 1.0);
    }
}
