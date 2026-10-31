//! The single touch gestures companion (Advanced settings), as the Android app's
//! (ui/CompanionUi.kt): a floating bar standing in for gestures that need more than one finger.
//! Undo, redo, Zoom and Move (while one is on, a press dragged on the canvas zooms it, up zooming
//! in about where it started, or moves it), and Fit Canvas. Dragged by its top edge.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, pos2, vec2};

use crate::icons::{self, Icon};
use crate::look::{Look, Weight, argb, pc, text};

const WIDTH: f32 = 432.0;
const HEIGHT: f32 = 89.0;
const FILL: u32 = 0xE6404040;
const INK: u32 = 0xFFBEBEBE;
const GRAB: u32 = 0xFF8A8A8A;
const BUTTON_WIDTH: f32 = 80.0;
const BUTTON_GAP: f32 = 4.0;
/// Where the bar sits at first: its middle across the window and this far down it.
const START_Y: f32 = 0.54;

/// What a press on the canvas does while one of the bar's modes is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    None,
    Zoom,
    Move,
}

/// What a button of the bar asked.
pub enum Asked {
    Undo,
    Redo,
    Fit,
}

#[derive(Default)]
pub struct Companion {
    /// Where the bar's top left is (none: where it starts).
    at: Option<Pos2>,
    pub mode: Mode,
}

impl Companion {
    /// The bar over the window; what a button asked.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look) -> Option<Asked> {
        let screen = ctx.content_rect();
        let start = pos2(screen.center().x - WIDTH / 2.0, screen.top() + screen.height() * START_Y - HEIGHT / 2.0);
        // kept on the window
        let keep = |p: Pos2| {
            pos2(
                p.x.clamp(screen.left(), (screen.right() - WIDTH).max(screen.left())),
                p.y.clamp(screen.top(), (screen.bottom() - HEIGHT).max(screen.top())),
            )
        };
        let at = keep(self.at.unwrap_or(start));
        let bar = Rect::from_min_size(at, vec2(WIDTH, HEIGHT));
        let mut asked = None;
        egui::Area::new(Id::unique("touch companion")).fixed_pos(bar.min).order(Order::Foreground).fade_in(false).show(
            ctx,
            |ui| {
                ui.allocate_rect(bar, Sense::click());
                let shadow =
                    egui::epaint::Shadow { offset: [0, 0], blur: 16, spread: 0, color: Color32::from_black_alpha(90) };
                ui.painter().add(shadow.as_shape(bar, 30.0));
                ui.painter().rect_filled(bar, 30, argb(FILL));
                // the grab line, and the strip it sits in, drags the bar about
                let strip = Rect::from_min_size(bar.min, vec2(WIDTH, 24.0));
                ui.painter().rect_filled(Rect::from_center_size(strip.center(), vec2(40.0, 5.0)), 2.5, argb(GRAB));
                let grab = ui.interact(strip, ui.scope_id().with("companion grab"), Sense::drag());
                if grab.dragged() {
                    self.at = Some(keep(at + grab.drag_delta()));
                }
                let buttons: [(&Icon, &str, f32, Option<Mode>); 5] = [
                    (&icons::UNDO, "Undo", 30.0, None),
                    (&icons::REDO, "Redo", 30.0, None),
                    (&icons::ZOOM, "Zoom", 22.0, Some(Mode::Zoom)),
                    (&icons::MOVE_CANVAS, "Move", 22.0, Some(Mode::Move)),
                    (&icons::FIT_CANVAS, "Fit Canvas", 22.0, None),
                ];
                for (k, (icon, label, size, mode)) in buttons.into_iter().enumerate() {
                    let cell = Rect::from_min_size(
                        pos2(bar.left() + 8.0 + (BUTTON_WIDTH + BUTTON_GAP) * k as f32, strip.bottom()),
                        vec2(BUTTON_WIDTH, 50.0),
                    );
                    let on = mode.is_some_and(|m| m == self.mode);
                    let tint = if on { argb(pc::ACCENT) } else { argb(INK) };
                    look.icon(ui, icon, pos2(cell.center().x, cell.top() + 15.0), size, tint);
                    text(
                        ui.painter(),
                        pos2(cell.center().x, cell.top() + 30.0),
                        Align2::CENTER_TOP,
                        label,
                        13.0,
                        Weight::Regular,
                        argb(INK),
                    );
                    if ui.interact(cell, ui.scope_id().with(("companion", k)), Sense::click()).clicked() {
                        match (k, mode) {
                            (_, Some(m)) => self.mode = if self.mode == m { Mode::None } else { m },
                            (0, None) => asked = Some(Asked::Undo),
                            (1, None) => asked = Some(Asked::Redo),
                            _ => asked = Some(Asked::Fit),
                        }
                    }
                }
            },
        );
        asked
    }
}
