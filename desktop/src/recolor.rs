//! Recolor (the QuickMenu's), as the Android app's: the area under a reticle takes the current
//! colour. Dragged about, the reticle moves and the fill follows it; a tap keeps the fill and
//! starts again where it lands. Its toolbar's Flood slider says how far from the colour under
//! the reticle the fill reaches (kept from one time to the next). It ends keeping what it filled.

use egui::{Color32, Id, Order, Pos2, Stroke, pos2, vec2};
use procraft_core::CanvasSession;

/// The undo step a kept fill makes.
const STEP: &str = "Recolor";

#[derive(Default)]
pub struct Recolor {
    pub on: bool,
    /// Where the reticle is, canvas pixels.
    point: Option<(f32, f32)>,
    /// A fill is open under the reticle (none while it is off the canvas, or once undone).
    open: bool,
}

/// The view as `view_matrix` gives it.
fn matrix(session: &CanvasSession) -> [f32; 6] {
    let v = session.view_matrix();
    v.get(..6).and_then(|m| <[f32; 6]>::try_from(m).ok()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
}

/// Canvas pixel `p` in the window (pixels).
fn to_window(session: &CanvasSession, (x, y): (f32, f32)) -> (f32, f32) {
    let [a, b, c, d, e, f] = matrix(session);
    (a * x + c * y + e, b * x + d * y + f)
}

/// Window pixel `p` on the canvas, if the view can be turned back.
fn to_canvas(session: &CanvasSession, (px, py): (f32, f32)) -> Option<(f32, f32)> {
    let [a, b, c, d, e, f] = matrix(session);
    let det = a * d - b * c;
    if det.abs() < 1e-9 {
        return None;
    }
    let (x, y) = (px - e, py - f);
    Some(((d * x - c * y) / det, (a * y - b * x) / det))
}

impl Recolor {
    /// On, the reticle in the middle of the window (`centre`, window pixels) filling with `rgb`
    /// as far as `threshold`; or off, keeping what it filled.
    pub fn toggle(&mut self, session: &CanvasSession, centre: (f32, f32), rgb: [f32; 3], threshold: f32) {
        if self.on {
            self.end(session);
            return;
        }
        self.on = true;
        self.point = to_canvas(session, centre);
        self.open = begin(session, centre, rgb, threshold);
    }

    /// The reticle dragged by `by` window pixels: the fill follows it.
    pub fn drag(&mut self, session: &CanvasSession, by: (f32, f32), rgb: [f32; 3], threshold: f32) {
        let Some(point) = self.point else { return };
        let from = to_window(session, point);
        let at = (from.0 + by.0, from.1 + by.1);
        let Some(moved) = to_canvas(session, at) else { return };
        self.point = Some(moved);
        if !self.open {
            self.open = begin(session, at, rgb, threshold);
            return;
        }
        session.pour_move(at.0, at.1);
    }

    /// A tap: the fill so far is kept, and the reticle starts again at `at` (window pixels).
    pub fn tap(&mut self, session: &CanvasSession, at: (f32, f32), rgb: [f32; 3], threshold: f32) {
        if self.open {
            session.pour_end(true, Some(STEP.to_owned()));
        }
        self.point = to_canvas(session, at);
        self.open = self.point.is_some() && begin(session, at, rgb, threshold);
    }

    /// The Flood slider moved to `t`: the fill redone with it.
    pub fn threshold_to(&self, session: &CanvasSession, t: f32) {
        if self.open {
            session.pour_threshold(t.clamp(0.0, 1.0));
        }
    }

    /// The colour changed: the area under the reticle takes it.
    pub fn colour_to(&self, session: &CanvasSession, [r, g, b]: [f32; 3]) {
        if self.open {
            session.pour_colour(r, g, b);
        }
    }

    /// Undo or redo coming: the open fill steps aside (true if there was one, which undo takes
    /// back on its own).
    pub fn step_aside(&mut self, session: &CanvasSession) -> bool {
        if !self.open {
            return false;
        }
        session.pour_end(false, None);
        self.open = false;
        true
    }

    /// After a redo: the fill made again over what it brought back.
    pub fn again(&mut self, session: &CanvasSession, rgb: [f32; 3], threshold: f32) {
        if let Some(point) = self.point.filter(|_| self.on) {
            self.open = begin(session, to_window(session, point), rgb, threshold);
        }
    }

    /// Recolor ends, keeping what it filled.
    pub fn end(&mut self, session: &CanvasSession) {
        if !self.on {
            return;
        }
        if self.open {
            session.pour_end(true, Some(STEP.to_owned()));
        }
        self.open = false;
        self.on = false;
        self.point = None;
    }

    /// Another artwork is shown.
    pub fn forget(&mut self) {
        *self = Recolor::default();
    }

    /// The reticle: four short bars around the point it fills from, 23 across, black over a light
    /// colour and white over a dark one.
    pub fn reticle(&self, ctx: &egui::Context, session: &CanvasSession, [r, g, b]: [f32; 3]) {
        let Some(point) = self.point.filter(|_| self.on) else { return };
        let (x, y) = to_window(session, point);
        let at: Pos2 = pos2(x, y) / ctx.pixels_per_point();
        let light = 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.5;
        let ink = if light { Color32::BLACK } else { Color32::WHITE };
        let painter = ctx.layer_painter(egui::LayerId::new(Order::Middle, Id::unique("recolor reticle")));
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let d = vec2(dx, dy);
            painter.line_segment([at + d * 2.5, at + d * 11.5], Stroke::new(1.0, ink));
        }
    }
}

/// A fill under window pixel `at` with `rgb` as far as `threshold`; false where there is nothing
/// to fill.
fn begin(session: &CanvasSession, at: (f32, f32), [r, g, b]: [f32; 3], threshold: f32) -> bool {
    session.pour_begin(at.0, at.1, r, g, b, threshold)
}
