//! The display turning: the canvas stays where it was on the glass while the bars turn upright, and
//! the surround's grid stays fixed to the glass. A way up is how many quarter turns what is drawn
//! is turned clockwise from the panel's natural way up (the device being turned the other way).

use crate::CanvasSession;

/// Window point (`x`, `y`) at way up `quarters` as a point on a panel `natural` wide (its width in
/// its natural way up) and `tall`.
fn to_panel(quarters: u32, natural: f32, tall: f32, x: f32, y: f32) -> (f32, f32) {
    match quarters % 4 {
        1 => (natural - y, x),
        2 => (natural - x, tall - y),
        3 => (y, tall - x),
        _ => (x, y),
    }
}

/// The panel's point (`x`, `y`) in the window at way up `quarters`.
fn from_panel(quarters: u32, natural: f32, tall: f32, x: f32, y: f32) -> (f32, f32) {
    match quarters % 4 {
        1 => (y, natural - x),
        2 => (natural - x, tall - y),
        3 => (tall - y, x),
        _ => (x, y),
    }
}

/// What turning from way up `from` to `to` does to the view of a window `width` × `height` pixels
/// (as it was): where the old window's corner lands, and the angle its x axis lands at.
fn turn_of_view(from: u32, to: u32, width: f32, height: f32) -> (f32, f32, f32) {
    let (natural, tall) = if from % 2 == 0 { (width, height) } else { (height, width) };
    let map = |x: f32, y: f32| {
        let (px, py) = to_panel(from, natural, tall, x, y);
        from_panel(to, natural, tall, px, py)
    };
    let (ox, oy) = map(0.0, 0.0);
    let (ux, uy) = map(1.0, 0.0);
    (ox, oy, (uy - oy).atan2(ux - ox))
}

/// Where the middle of one of the surround grid's lines falls at way up `quarters` in a window
/// `width` × `height` pixels: its 1 pt lines lie every 16 pt from the panel's own top-left corner in
/// its natural way up, so that is half a point in from whichever corner that is.
fn grid_line(quarters: u32, width: f32, height: f32, pixels_per_point: f32) -> (f32, f32) {
    let half = 0.5 * pixels_per_point;
    let (natural, tall) = if quarters % 2 == 0 { (width, height) } else { (height, width) };
    match quarters % 4 {
        1 => (half, natural - half),
        2 => (natural - half, tall - half),
        3 => (tall - half, half),
        _ => (half, half),
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The display turned from way up `from` to `to` (quarter turns of what is drawn, clockwise from
    /// the panel's natural way up) with the window `width` × `height` pixels as it was: the view
    /// turns the other way, so the canvas stays where it was on the glass.
    pub fn follow_turn(&self, from: u32, to: u32, width: u32, height: u32) {
        let (x, y, angle) = turn_of_view(from, to, width as f32, height as f32);
        self.view_gesture(0.0, 0.0, x, y, 1.0, angle);
    }

    /// Fixes the surround's grid to the glass for a window `width` × `height` pixels at way up
    /// `quarters`; the point its lines run from, for what is drawn over the canvas.
    pub fn place_grid(&self, quarters: u32, width: u32, height: u32, pixels_per_point: f32) -> Vec<f32> {
        let (x, y) = grid_line(quarters, width as f32, height as f32, pixels_per_point);
        self.set_grid_origin(x, y);
        vec![x, y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn a_point_goes_to_the_panel_and_back_at_every_way_up() {
        for quarters in 0..4 {
            let (natural, tall) = (1000.0, 2000.0);
            let (x, y) = if quarters % 2 == 0 { (300.0, 1700.0) } else { (1700.0, 300.0) };
            let (px, py) = to_panel(quarters, natural, tall, x, y);
            assert!(near(from_panel(quarters, natural, tall, px, py), (x, y)), "way up {quarters}");
        }
    }

    #[test]
    fn the_device_turned_anticlockwise_takes_the_top_left_corner_to_the_bottom_left() {
        // upright 1000 wide and 2000 tall, then on its side: 2000 wide and 1000 tall
        let (x, y, angle) = turn_of_view(0, 1, 1000.0, 2000.0);
        assert!(near((x, y), (0.0, 1000.0)));
        // the old window's x axis now runs up the screen
        assert!((angle + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn turning_there_and_back_comes_home() {
        for (from, to) in [(0, 1), (1, 2), (2, 3), (3, 0), (0, 2), (1, 3)] {
            let (natural, tall) = (1000.0, 2000.0);
            let there = {
                let (px, py) = to_panel(from, natural, tall, 120.0, 340.0);
                from_panel(to, natural, tall, px, py)
            };
            let (px, py) = to_panel(to, natural, tall, there.0, there.1);
            assert!(near(from_panel(from, natural, tall, px, py), (120.0, 340.0)), "{from} to {to}");
        }
    }

    #[test]
    fn staying_put_moves_nothing() {
        let (x, y, angle) = turn_of_view(1, 1, 2000.0, 1000.0);
        assert!(near((x, y), (0.0, 0.0)));
        assert!(angle.abs() < 1e-6);
    }

    #[test]
    fn the_grid_stays_on_the_panels_own_corner() {
        // 2 pixels a point: half a point in is 1 pixel
        assert!(near(grid_line(0, 1000.0, 2000.0, 2.0), (1.0, 1.0)));
        // on its side the panel's corner is at the window's bottom left
        assert!(near(grid_line(1, 2000.0, 1000.0, 2.0), (1.0, 999.0)));
        assert!(near(grid_line(2, 1000.0, 2000.0, 2.0), (999.0, 1999.0)));
        assert!(near(grid_line(3, 2000.0, 1000.0, 2.0), (1999.0, 1.0)));
    }
}
