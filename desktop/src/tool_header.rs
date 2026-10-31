//! The header a full-screen mode shows over the top bar, as the Android app's (ui/ToolHeader.kt):
//! its title, a capsule of buttons (one may be shown chosen) and the blue round Done.

use egui::{Align2, Color32, Id, Order, Rect, Sense, Stroke, pos2, vec2};

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, text};

pub const HEIGHT: f32 = 85.0;
const LABEL: Themed = Themed(0xFFC5C5C5, 0xFF3A3A3A);
const BUTTON: Themed = Themed(0xFFA5A5A5, 0xFF565656);
const CHOSEN: Themed = Themed(0xFFC0C0C0, 0xFF3A3A3A);
const CHOSEN_FILL: Themed = Themed(0xFF464646, 0xFFA5A5A5);
const DONE: u32 = 0xFF0190FF;

/// The header's band, its capsule and the capsule's edge.
#[derive(Clone, Copy)]
pub struct Colours {
    pub band: Themed,
    pub capsule: Themed,
    pub edge: Themed,
}

/// The light interface's header, every tool's.
const LIGHT: (u32, u32, u32) = (0xFFDFE2E3, 0xFFF5F8F9, 0xFFDCDEDF);
/// Over the surround of a tool that edits the canvas.
pub const DARK: Colours = Colours {
    band: Themed(0xFF1D1D1D, LIGHT.0),
    capsule: Themed(0xFF1A1A1A, LIGHT.1),
    edge: Themed(0xFF373737, LIGHT.2),
};
/// Over a picture shown edge to edge.
pub const OVER_PICTURE: Colours = Colours {
    band: Themed(0xFF333333, LIGHT.0),
    capsule: Themed(0xFF232323, LIGHT.1),
    edge: Themed(0xFF424242, LIGHT.2),
};

/// What a tap on the header was on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tapped {
    /// The button at this place in the list.
    Button(usize),
    Done,
}

/// Shows the header with `title` and `buttons` (each its label, and whether it shows chosen);
/// returns what was tapped.
pub fn show(ctx: &egui::Context, look: &mut Look, title: &str, buttons: &[(&str, bool)]) -> Option<Tapped> {
    show_in(ctx, look, title, buttons, DARK)
}

/// [`show`] in other `colours`.
pub fn show_in(
    ctx: &egui::Context,
    look: &mut Look,
    title: &str,
    buttons: &[(&str, bool)],
    colours: Colours,
) -> Option<Tapped> {
    let screen = ctx.content_rect();
    let band = Rect::from_min_size(screen.min, vec2(screen.width(), HEIGHT));
    let mut tapped = None;
    egui::Area::new(Id::unique("tool header")).fixed_pos(band.min).order(Order::Foreground).show(ctx, |ui| {
        ui.allocate_rect(band, Sense::click());
        let painter = ui.painter();
        painter.rect_filled(band, 0, argb(colours.band));
        text(painter, band.min + vec2(32.0, 27.0), Align2::LEFT_TOP, title, 22.0, Weight::Bold, argb(LABEL));
        let done = Rect::from_min_size(pos2(band.right() - 20.0 - 44.0, band.top() + 20.0), vec2(44.0, 44.0));
        painter.circle_filled(done.center(), 22.0, argb(DONE));
        look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
        if ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked() {
            tapped = Some(Tapped::Done);
        }
        if buttons.is_empty() {
            return;
        }
        let galleys: Vec<_> = buttons
            .iter()
            .map(|(label, chosen)| {
                let ink = if *chosen { argb(CHOSEN) } else { argb(BUTTON) };
                ui.painter().layout_no_wrap((*label).to_owned(), Weight::Regular.font(17.0), ink)
            })
            .collect();
        let width: f32 = galleys.iter().map(|g| g.size().x + 26.0).sum();
        let capsule = Rect::from_min_size(pos2(done.left() - 13.0 - width, band.top() + 20.0), vec2(width, 43.0));
        ui.painter().rect_filled(capsule, 21.5, argb(colours.capsule));
        ui.painter().rect_stroke(capsule, 21.5, Stroke::new(1.0, argb(colours.edge)), egui::StrokeKind::Inside);
        let mut x = capsule.left();
        for (i, ((_, chosen), galley)) in buttons.iter().zip(galleys).enumerate() {
            let button = Rect::from_min_size(pos2(x, capsule.top()), vec2(galley.size().x + 26.0, 43.0));
            if *chosen {
                ui.painter().rect_filled(button, 21.5, argb(CHOSEN_FILL));
            }
            ui.painter().galley(button.center() - galley.size() / 2.0, galley.clone(), Color32::WHITE);
            if ui.interact(button, ui.scope_id().with(("tool button", i)), Sense::click()).clicked() {
                tapped = Some(Tapped::Button(i));
            }
            x = button.right();
        }
    });
    tapped
}
