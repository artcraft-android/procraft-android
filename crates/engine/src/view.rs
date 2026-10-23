//! Where the canvas sits on screen.

use crate::geom::{Affine, Vec2};

/// Zoom limits (screen pixels per canvas pixel).
pub const MIN_ZOOM: f32 = 0.02;
pub const MAX_ZOOM: f32 = 64.0;

/// The canvas-to-screen map: a move, a uniform zoom and a rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    to_screen: Affine,
}

impl View {
    /// The whole canvas, centred and as large as fits.
    pub fn fit(canvas: (u32, u32), screen: (u32, u32)) -> View {
        View::fit_inside(canvas, screen, [0.0; 4])
    }

    /// The whole canvas, centred and as large as fits in the screen less `insets` (left, top,
    /// right, bottom, screen pixels): clear of the interface.
    pub fn fit_inside(canvas: (u32, u32), screen: (u32, u32), insets: [f32; 4]) -> View {
        let [left, top, right, bottom] = insets.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
        let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
        let (sw, sh) = (screen.0.max(1) as f32, screen.1.max(1) as f32);
        let (w, h) = ((sw - left - right).max(1.0), (sh - top - bottom).max(1.0));
        let zoom = (w / cw).min(h / ch).clamp(MIN_ZOOM, MAX_ZOOM);
        let offset = Vec2::new(left + (w - cw * zoom) / 2.0, top + (h - ch * zoom) / 2.0);
        View { to_screen: Affine::scale(zoom).then(Affine::translate(offset)) }
    }

    /// As [`View::fit_inside`], with the canvas turned by `quarter_turns` right angles (clockwise
    /// on the screen): fitted as it shows, its sides swapped for an odd number of turns.
    pub fn fit_turned(canvas: (u32, u32), screen: (u32, u32), insets: [f32; 4], quarter_turns: i32) -> View {
        let turns = quarter_turns.rem_euclid(4);
        if turns == 0 {
            return View::fit_inside(canvas, screen, insets);
        }
        let [left, top, right, bottom] = insets.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
        let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
        let (sw, sh) = (screen.0.max(1) as f32, screen.1.max(1) as f32);
        let (w, h) = ((sw - left - right).max(1.0), (sh - top - bottom).max(1.0));
        let (shown_w, shown_h) = if turns % 2 == 1 { (ch, cw) } else { (cw, ch) };
        let zoom = (w / shown_w).min(h / shown_h).clamp(MIN_ZOOM, MAX_ZOOM);
        let middle = Vec2::new(left + w / 2.0, top + h / 2.0);
        let to_screen = Affine::translate(Vec2::new(-cw / 2.0, -ch / 2.0))
            .then(Affine::rotate(turns as f32 * std::f32::consts::FRAC_PI_2))
            .then(Affine::scale(zoom))
            .then(Affine::translate(middle));
        View { to_screen }
    }

    /// How many right angles the view is turned by, to the nearest.
    pub fn quarter_turns(&self) -> i32 {
        let r = self.rotation();
        if r.is_finite() { (r / std::f32::consts::FRAC_PI_2).round() as i32 } else { 0 }
    }

    /// The canvas point `centre` in the middle of a `screen`, at `zoom` and turned by `rotation`
    /// (radians): a view kept with an artwork, put back on a screen of any size.
    pub fn centred(centre: Vec2, zoom: f32, rotation: f32, screen: (u32, u32)) -> Option<View> {
        if !(centre.is_finite() && zoom.is_finite() && rotation.is_finite()) {
            return None;
        }
        let middle = Vec2::new(screen.0.max(1) as f32 / 2.0, screen.1.max(1) as f32 / 2.0);
        let to_screen = Affine::translate(Vec2::new(-centre.x, -centre.y))
            .then(Affine::rotate(rotation))
            .then(Affine::scale(zoom.clamp(MIN_ZOOM, MAX_ZOOM)))
            .then(Affine::translate(middle));
        Some(View { to_screen })
    }

    /// The view `t` (0..1) of the way from `a` to `b`: the zoom (in proportion), the turn (the short
    /// way round) and where the canvas point `centre` shows on the screen, each in step.
    pub fn between(a: &View, b: &View, centre: Vec2, t: f32) -> View {
        let t = if t.is_finite() { t } else { 1.0 };
        let (za, zb) = (a.zoom().max(MIN_ZOOM), b.zoom().max(MIN_ZOOM));
        let zoom = za * (zb / za).powf(t);
        let mut turn = b.rotation() - a.rotation();
        while turn > std::f32::consts::PI {
            turn -= std::f32::consts::TAU;
        }
        while turn < -std::f32::consts::PI {
            turn += std::f32::consts::TAU;
        }
        let rotation = a.rotation() + turn * t;
        let (pa, pb) = (a.to_screen.apply(centre), b.to_screen.apply(centre));
        let at = pa + (pb - pa) * t;
        let to_screen = Affine::translate(Vec2::new(-centre.x, -centre.y))
            .then(Affine::rotate(rotation))
            .then(Affine::scale(zoom.clamp(MIN_ZOOM, MAX_ZOOM)))
            .then(Affine::translate(at));
        View { to_screen }
    }

    /// A view mapping the canvas to the screen by `to_screen`.
    pub fn from_affine(to_screen: Affine) -> View {
        View { to_screen }
    }

    pub fn to_screen(&self) -> Affine {
        self.to_screen
    }

    pub fn screen_to_canvas(&self, p: Vec2) -> Option<Vec2> {
        self.to_screen.inverse().map(|m| m.apply(p))
    }

    pub fn zoom(&self) -> f32 {
        self.to_screen.scale_factor()
    }

    pub fn rotation(&self) -> f32 {
        self.to_screen.rotation()
    }

    /// One step of a two-finger gesture: the canvas scales by `scale` and turns by `rotation`
    /// around `pivot` (screen pixels), then follows the fingers by `pan`. Zoom stays within limits;
    /// non-finite input is ignored.
    pub fn gesture(&mut self, pivot: Vec2, pan: Vec2, scale: f32, rotation: f32) {
        if !pivot.is_finite() || !pan.is_finite() || !(scale.is_finite() && scale > 0.0) || !rotation.is_finite() {
            return;
        }
        let zoom = self.zoom();
        let s = (zoom * scale).clamp(MIN_ZOOM, MAX_ZOOM) / zoom;
        let step = Affine::translate(Vec2::ZERO - pivot)
            .then(Affine::scale(s))
            .then(Affine::rotate(rotation))
            .then(Affine::translate(pivot + pan));
        let next = self.to_screen.then(step);
        if next.is_finite() && next.inverse().is_some() {
            self.to_screen = next;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fitting_keeps_the_nearest_right_angle_and_fits_it_on_its_side() {
        let on_side = View::fit_turned((100, 200), (400, 400), [0.0; 4], 1);
        assert_eq!(on_side.quarter_turns(), 1);
        // shown 200 wide and 100 tall, it is as wide as the screen
        assert!((on_side.zoom() - 2.0).abs() < 1e-4);
        let corners = [(0.0, 0.0), (100.0, 0.0), (0.0, 200.0), (100.0, 200.0)]
            .map(|(x, y)| on_side.to_screen().apply(Vec2::new(x, y)));
        let (xs, ys): (Vec<f32>, Vec<f32>) = corners.iter().map(|p| (p.x, p.y)).unzip();
        let span = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min);
        assert!((span(&xs) - 400.0).abs() < 1e-3 && (span(&ys) - 200.0).abs() < 1e-3);
        // turned a little, fitting straightens it; nearly on its side, it goes on its side
        let mut v = View::fit((100, 100), (400, 400));
        v.gesture(Vec2::new(200.0, 200.0), Vec2::ZERO, 1.0, 0.6);
        assert_eq!(v.quarter_turns(), 0);
        v.gesture(Vec2::new(200.0, 200.0), Vec2::ZERO, 1.0, 0.6);
        assert_eq!(v.quarter_turns(), 1);
    }

    use super::*;

    #[test]
    fn fit_centres_the_canvas() {
        let v = View::fit((1000, 2000), (800, 800));
        assert!((v.zoom() - 0.4).abs() < 1e-6);
        let tl = v.to_screen().apply(Vec2::ZERO);
        assert!((tl.x - 200.0).abs() < 1e-3 && tl.y.abs() < 1e-3);
    }

    #[test]
    fn gesture_keeps_the_pivot_under_the_fingers() {
        let mut v = View::fit((1000, 1000), (1000, 1000));
        let pivot = Vec2::new(300.0, 700.0);
        let under = v.screen_to_canvas(pivot).unwrap();
        v.gesture(pivot, Vec2::ZERO, 2.0, 0.5);
        assert!(v.to_screen().apply(under).distance(pivot) < 1e-2);
        assert!((v.zoom() - 2.0).abs() < 1e-4);
        v.gesture(pivot, Vec2::new(10.0, -5.0), 1.0, 0.0);
        assert!(v.to_screen().apply(under).distance(pivot + Vec2::new(10.0, -5.0)) < 1e-2);
    }

    #[test]
    fn zoom_is_clamped_and_bad_input_ignored() {
        let mut v = View::fit((100, 100), (100, 100));
        v.gesture(Vec2::ZERO, Vec2::ZERO, 1e9, 0.0);
        assert!((v.zoom() - MAX_ZOOM).abs() < 1e-2);
        let before = v;
        v.gesture(Vec2::ZERO, Vec2::ZERO, f32::NAN, 0.0);
        v.gesture(Vec2::ZERO, Vec2::ZERO, -1.0, 0.0);
        assert_eq!(v, before);
    }

    #[test]
    fn a_fit_keeps_clear_of_insets_and_a_kept_view_comes_back_anywhere() {
        // 100 x 100 in 200 x 300 less 20 on the left and 40 on top: 180 wide, centred in 260
        let v = View::fit_inside((100, 100), (200, 300), [20.0, 40.0, 0.0, 0.0]);
        let p = v.to_screen().apply(Vec2::new(0.0, 0.0));
        assert!((v.zoom() - 1.8).abs() < 1e-4 && (p.x - 20.0).abs() < 1e-3 && (p.y - 80.0).abs() < 1e-3, "{p:?}");
        // canvas (30, 40) in the middle, twice the size, turned: on any screen
        let c = View::centred(Vec2::new(30.0, 40.0), 2.0, 0.5, (400, 600)).unwrap();
        let m = c.to_screen().apply(Vec2::new(30.0, 40.0));
        assert!((m.x - 200.0).abs() < 1e-3 && (m.y - 300.0).abs() < 1e-3);
        assert!((c.zoom() - 2.0).abs() < 1e-4 && (c.to_screen().rotation() - 0.5).abs() < 1e-4);
        assert!(View::centred(Vec2::new(f32::NAN, 0.0), 1.0, 0.0, (10, 10)).is_none());
    }
}
