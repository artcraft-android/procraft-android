//! The reference window's picture (the canvas, or a picture imported): fitted whole into the
//! window (all of it: the window's bars lie over the picture), then moved there by a finger and
//! zoomed by two, a view of its own apart from the canvas's; and which of its pixels is under a
//! point in the window, for its eyedropper.

/// How a picture sits in the reference window: its zoom past fitting whole (1 fits it), and how
/// far its middle is from the window's (window pixels).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ReferenceView {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for ReferenceView {
    fn default() -> ReferenceView {
        ReferenceView { zoom: 1.0, x: 0.0, y: 0.0 }
    }
}

/// The window's picture area and the picture's size, pixels.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ReferenceSizes {
    pub window_width: f32,
    pub window_height: f32,
    pub picture_width: f32,
    pub picture_height: f32,
}

/// The most a reference picture is zoomed past fitting whole.
const MOST_ZOOM: f32 = 20.0;

impl ReferenceSizes {
    /// The picture's pixel size in the window at `zoom`: fitted whole, then zoomed.
    fn scale(&self, zoom: f32) -> f32 {
        let fit = (self.window_width / self.picture_width).min(self.window_height / self.picture_height);
        if fit.is_finite() && fit > 0.0 { fit * zoom } else { 0.0 }
    }
}

/// The view after two fingers moved from (`from_x`, `from_y`) to (`to_x`, `to_y`) (their middle,
/// window pixels) and spread `spread` times as far apart: what was under them stays under them,
/// zoomed between fitting whole and [`MOST_ZOOM`], the window's middle kept on the picture.
#[uniffi::export]
pub fn reference_view_pinched(
    view: ReferenceView,
    sizes: ReferenceSizes,
    from_x: f32,
    from_y: f32,
    to_x: f32,
    to_y: f32,
    spread: f32,
) -> ReferenceView {
    let spread = if spread.is_finite() && spread > 0.0 { spread } else { 1.0 };
    let zoom = (view.zoom * spread).clamp(1.0, MOST_ZOOM);
    let (before, after) = (sizes.scale(view.zoom), sizes.scale(zoom));
    if before <= 0.0 {
        return ReferenceView::default();
    }
    let (cx, cy) = (sizes.window_width / 2.0, sizes.window_height / 2.0);
    // the picture's middle, in the window, moved so the point under the fingers follows them
    let k = after / before;
    let mx = to_x - (from_x - (cx + view.x)) * k;
    let my = to_y - (from_y - (cy + view.y)) * k;
    let (half_w, half_h) = (sizes.picture_width * after / 2.0, sizes.picture_height * after / 2.0);
    ReferenceView { zoom, x: (mx - cx).clamp(-half_w, half_w), y: (my - cy).clamp(-half_h, half_h) }
}

/// Where the picture is drawn in the window: left, top, width and height (window pixels).
#[uniffi::export]
pub fn reference_picture_rect(view: ReferenceView, sizes: ReferenceSizes) -> Vec<f32> {
    let s = sizes.scale(view.zoom);
    let (w, h) = (sizes.picture_width * s, sizes.picture_height * s);
    let (mx, my) = (sizes.window_width / 2.0 + view.x, sizes.window_height / 2.0 + view.y);
    vec![mx - w / 2.0, my - h / 2.0, w, h]
}

/// The picture's point (its pixels, fractional) under window point (`x`, `y`), and how many of
/// its pixels a window pixel spans; none off the picture.
#[uniffi::export]
pub fn reference_picture_point(view: ReferenceView, sizes: ReferenceSizes, x: f32, y: f32) -> Option<Vec<f32>> {
    let s = sizes.scale(view.zoom);
    if s <= 0.0 {
        return None;
    }
    let (mx, my) = (sizes.window_width / 2.0 + view.x, sizes.window_height / 2.0 + view.y);
    let px = (x - mx) / s + sizes.picture_width / 2.0;
    let py = (y - my) / s + sizes.picture_height / 2.0;
    let on = (0.0..sizes.picture_width).contains(&px) && (0.0..sizes.picture_height).contains(&py);
    on.then(|| vec![px, py, 1.0 / s])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1000 x 500 picture in a 400 x 400 window: fitted at 0.4, 400 x 200, 100 down.
    const SIZES: ReferenceSizes =
        ReferenceSizes { window_width: 400.0, window_height: 400.0, picture_width: 1000.0, picture_height: 500.0 };

    #[test]
    fn a_picture_fits_whole_in_the_middle() {
        let rect = reference_picture_rect(ReferenceView::default(), SIZES);
        assert_eq!(rect, vec![0.0, 100.0, 400.0, 200.0]);
        assert_eq!(
            reference_picture_point(ReferenceView::default(), SIZES, 200.0, 200.0),
            Some(vec![500.0, 250.0, 2.5])
        );
        // above the picture, in the window's empty band
        assert_eq!(reference_picture_point(ReferenceView::default(), SIZES, 200.0, 50.0), None);
    }

    #[test]
    fn a_pinch_keeps_what_is_under_the_fingers_under_them() {
        let start = ReferenceView::default();
        let under = reference_picture_point(start, SIZES, 100.0, 150.0).unwrap();
        // spread twice as far, and moved 30 right
        let view = reference_view_pinched(start, SIZES, 100.0, 150.0, 130.0, 150.0, 2.0);
        assert_eq!(view.zoom, 2.0);
        let now = reference_picture_point(view, SIZES, 130.0, 150.0).unwrap();
        assert!((now[0] - under[0]).abs() < 1e-3 && (now[1] - under[1]).abs() < 1e-3, "{now:?} {under:?}");
        assert!((now[2] - 1.25).abs() < 1e-6);
    }

    #[test]
    fn it_zooms_no_less_than_fitting_and_no_more_than_the_most() {
        let out = reference_view_pinched(ReferenceView::default(), SIZES, 200.0, 200.0, 200.0, 200.0, 0.3);
        assert_eq!(out.zoom, 1.0);
        let mut view = ReferenceView::default();
        for _ in 0..10 {
            view = reference_view_pinched(view, SIZES, 200.0, 200.0, 200.0, 200.0, 3.0);
        }
        assert_eq!(view.zoom, MOST_ZOOM);
    }

    #[test]
    fn the_window_s_middle_stays_on_the_picture() {
        // dragged far off to the left and down
        let view = reference_view_pinched(ReferenceView::default(), SIZES, 200.0, 200.0, -5000.0, 9000.0, 1.0);
        assert_eq!((view.x, view.y), (-200.0, 100.0));
        // its right edge, and its top, at the window's middle
        assert_eq!(reference_picture_rect(view, SIZES), vec![-200.0, 200.0, 400.0, 200.0]);
    }
}
