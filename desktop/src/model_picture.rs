//! A picture being placed on a model: the box round it, and a bar to flip it, turn it, put it back,
//! and keep it or take it away. A touch or the mouse on the picture moves it, two fingers or the
//! wheel over it scale and turn it; off it, the model turns as ever. The core carries it onto the
//! surface as it goes.

use egui::{Color32, Id, LayerId, Order, Stroke, pos2};
use procraft_core::CanvasSession;

use crate::icons;
use crate::look::Look;
use crate::transform::{BUTTON_ROOMY, BUTTON_WIDTH, Button, Tapped, floating_toolbar};

/// What Rotate 45° turns it by.
const EIGHTH_TURN: f32 = std::f32::consts::FRAC_PI_4;

/// Draws the box and the bar while a picture is being placed, and does what is asked.
pub fn show(ctx: &egui::Context, look: &mut Look, session: &CanvasSession) {
    let Some(corners) = session.model_picture_box() else { return };
    let ppp = ctx.pixels_per_point();
    let points: Vec<egui::Pos2> = corners.as_chunks::<2>().0.iter().map(|&[x, y]| pos2(x / ppp, y / ppp)).collect();
    let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::unique("model picture")));
    let mut round = points.clone();
    round.extend(points.first().copied());
    painter.add(egui::Shape::line(round.clone(), Stroke::new(4.0, Color32::from_black_alpha(128))));
    painter.add(egui::Shape::line(round, Stroke::new(2.0, Color32::WHITE)));
    for &p in &points {
        painter.circle_filled(p, 6.0, Color32::WHITE);
    }
    let buttons = [
        Button::new(&icons::FLIP_HORIZONTAL, "Flip Horizontal", 50.0),
        Button::new(&icons::FLIP_VERTICAL, "Flip Vertical", 50.0),
        Button::new(&icons::ROTATE45, "Rotate 45\u{b0}", 44.0),
        Button::new(&icons::TRANSFORM_RESET, "Reset", 44.0),
        Button::new(&icons::CROSS, "Cancel", 28.0),
        Button::new(&icons::TICK, "Done", 28.0),
    ];
    match floating_toolbar(ctx, look, "model picture bar", (BUTTON_ROOMY, BUTTON_WIDTH), &[], &buttons, 43.0) {
        Some(Tapped::Button(0)) => session.model_picture_flip(true),
        Some(Tapped::Button(1)) => session.model_picture_flip(false),
        Some(Tapped::Button(2)) => session.model_picture_move(0.0, 0.0, 1.0, EIGHTH_TURN),
        Some(Tapped::Button(3)) => session.model_picture_reset(),
        Some(Tapped::Button(4)) => session.model_picture_end(false),
        Some(Tapped::Button(5)) => session.model_picture_end(true),
        _ => {}
    }
    ctx.request_repaint();
}
