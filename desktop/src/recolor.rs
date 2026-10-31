//! Recolor's reticle (the QuickMenu's Recolor, as the Android app's): what Recolor does is the
//! core's (recolor.rs, its touches touch.rs's); the desktop draws the reticle where it is and its
//! Flood toolbar (adjust_panels.rs).

use egui::{Color32, Id, Order, Pos2, Stroke, pos2, vec2};
use procraft_core::CanvasSession;

/// The reticle: four short bars around the point it fills from, 23 across, black over a light
/// colour (`rgb`) and white over a dark one.
pub fn reticle(ctx: &egui::Context, session: &CanvasSession, [r, g, b]: [f32; 3]) {
    let Some(point) = session.recolor_point() else { return };
    let [Some(&x), Some(&y)] = [point.first(), point.get(1)] else { return };
    let at: Pos2 = pos2(x, y) / ctx.pixels_per_point();
    let light = 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.5;
    let ink = if light { Color32::BLACK } else { Color32::WHITE };
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Middle, Id::unique("recolor reticle")));
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let d = vec2(dx, dy);
        painter.line_segment([at + d * 2.5, at + d * 11.5], Stroke::new(1.0, ink));
    }
}
