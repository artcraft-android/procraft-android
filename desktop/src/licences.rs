//! Help's Licences, as the Android app's dialog: what the app is made with (the interface font and
//! the icons) and their licences, on a dark panel over the dimmed window, the text scrolling;
//! Done or a tap beside it closes it.

use egui::{Align2, Color32, Id, Order, Rect, ScrollArea, Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2};

use crate::look::{Weight, argb, pc, text};

const WIDTH: f32 = 560.0;
const MOST_TEXT: f32 = 420.0;
const FILL: u32 = 0xF21E1E1E;
const EDGE: u32 = 0x24FFFFFF;
const TITLE: u32 = 0xFFE5E5E5;
const INK: u32 = 0xFFC5C5C5;

const LICENCES: &str = concat!(
    "The interface font is Inter, by The Inter Project Authors.\n\n",
    include_str!("../../android/app/src/main/assets/fonts/Inter-OFL.txt"),
    "\n\nThe app keeps Lucide icons (lucide.dev), by Lucide Icons and Contributors.\n\n",
    include_str!("../../android/app/src/main/assets/icons/LICENSE-lucide.txt"),
);

/// Shows the licences; true once closed.
pub fn show(ctx: &egui::Context) -> bool {
    let screen = ctx.content_rect();
    let mut close = false;
    egui::Area::new(Id::unique("licences scrim")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
        let beside = ui.allocate_rect(screen, Sense::click());
        ui.painter().rect_filled(screen, 0, Color32::from_black_alpha(0x99));
        close |= beside.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
    });
    let height = 24.0 + 32.0 + 16.0 + MOST_TEXT + 16.0 + 40.0 + 16.0;
    let panel = Rect::from_center_size(screen.center(), vec2(WIDTH, height.min(screen.height() - 40.0)));
    egui::Area::new(Id::unique("licences")).fixed_pos(panel.min).order(Order::Tooltip).show(ctx, |ui| {
        ui.allocate_rect(panel, Sense::click());
        ui.painter().rect_filled(panel, 28, argb(FILL));
        ui.painter().rect_stroke(panel, 28, Stroke::new(1.0, argb(EDGE)), StrokeKind::Inside);
        let title = pos2(panel.left() + 24.0, panel.top() + 24.0);
        text(ui.painter(), title, Align2::LEFT_TOP, "Licences", 24.0, Weight::Bold, argb(TITLE));
        let body = Rect::from_min_max(
            pos2(panel.left() + 24.0, panel.top() + 24.0 + 32.0 + 16.0),
            pos2(panel.right() - 24.0, panel.bottom() - 16.0 - 40.0 - 16.0),
        );
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            ScrollArea::vertical().id_salt("licences text").show(ui, |ui| {
                ui.set_width(body.width());
                ui.label(egui::RichText::new(LICENCES).font(Weight::Regular.font(14.0)).color(argb(INK)));
            });
        });
        let done =
            Rect::from_min_size(pos2(panel.right() - 24.0 - 80.0, panel.bottom() - 16.0 - 40.0), vec2(80.0, 40.0));
        text(ui.painter(), done.center(), Align2::CENTER_CENTER, "Done", 16.0, Weight::Medium, argb(pc::ACCENT));
        close |= ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked();
    });
    close
}
