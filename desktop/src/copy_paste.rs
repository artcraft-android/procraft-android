//! The Copy & Paste panel, as the Android app's (ui/CopyPasteUi.kt), opened by three fingers swiped
//! down: Cut, Copy, Copy All, Duplicate, Cut & Paste and Paste, a little below the middle of the
//! window; a tap off it or its ✕ closes it.

use egui::{Align2, Id, Order, Rect, Sense, pos2, vec2};

use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, text};

const PANEL_FILL: Themed = Themed(0xFF484848, 0xFFE2E2E2);
const TITLE_INK: Themed = Themed(0xFFC5C5C5, 0xFF393939);
const BUTTON_INK: Themed = Themed(0xFFA8A8A8, 0xFF4A4A4A);
const CLOSE_FILL: Themed = Themed(0xFF2A2A2A, 0xFFCFCFCF);
const GRABBER: u32 = 0xFF7C7C7C;
const PANEL: (f32, f32) = (496.0, 106.0);
/// The panel's top, as a share of the window's height.
const PANEL_TOP: f32 = 617.0 / 1133.0;
const BUTTON: (f32, f32) = (82.7, 50.0);

/// What the panel was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Close,
    Cut,
    Copy,
    CopyAll,
    Duplicate,
    CutAndPaste,
    Paste,
}

/// Draws the panel (Paste greyed unless `can_paste`); returns what was chosen, if anything.
pub fn show(ctx: &egui::Context, look: &mut Look, can_paste: bool) -> Option<Choice> {
    // Escape closes it, as a tap off it does
    if crate::ui::escape(ctx) {
        return Some(Choice::Close);
    }
    let screen = ctx.content_rect();
    let panel = Rect::from_min_size(
        pos2(screen.left() + (screen.width() - PANEL.0) / 2.0, screen.top() + screen.height() * PANEL_TOP),
        vec2(PANEL.0, PANEL.1),
    );
    let mut chosen = None;
    egui::Area::new(Id::unique("copy and paste")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
        // a tap off the panel closes it; nothing under it is touched
        if ui.allocate_rect(screen, Sense::click_and_drag()).clicked() {
            chosen = Some(Choice::Close);
        }
        ui.interact(panel, ui.scope_id().with("panel"), Sense::click_and_drag());
        let painter = ui.painter();
        painter.rect_filled(panel, 30, argb(PANEL_FILL));
        let grabber = Rect::from_center_size(pos2(panel.center().x, panel.top() + 5.0 + 2.0), vec2(40.0, 4.0));
        painter.rect_filled(grabber, 2, argb(GRABBER));
        let title = pos2(panel.left() + 18.0, panel.top() + 12.0);
        text(painter, title, Align2::LEFT_TOP, "Copy & Paste", 22.0, Weight::Bold, argb(TITLE_INK));
        let close = Rect::from_min_size(panel.min + vec2(453.0, 6.0), vec2(40.0, 40.0));
        painter.circle_filled(close.center(), 20.0, argb(CLOSE_FILL));
        look.icon(ui, &icons::CROSS, close.center(), 18.0, argb(BUTTON_INK));
        if ui.interact(close, ui.scope_id().with("close"), Sense::click()).clicked() {
            chosen = Some(Choice::Close);
        }
        let buttons: [(&Icon, &str, Choice, bool); 6] = [
            (&icons::COPY_CUT, "Cut", Choice::Cut, true),
            (&icons::COPY_ONE, "Copy", Choice::Copy, true),
            (&icons::COPY_ALL, "Copy All", Choice::CopyAll, true),
            (&icons::COPY_DUPLICATE, "Duplicate", Choice::Duplicate, true),
            (&icons::CUT_PASTE, "Cut & Paste", Choice::CutAndPaste, true),
            (&icons::PASTE_DOWN, "Paste", Choice::Paste, can_paste),
        ];
        for (i, (icon, label, choice, enabled)) in buttons.into_iter().enumerate() {
            let rect = Rect::from_min_size(panel.min + vec2(BUTTON.0 * i as f32, 48.0), vec2(BUTTON.0, BUTTON.1));
            let ink = if enabled { argb(BUTTON_INK) } else { argb(BUTTON_INK).gamma_multiply(0.4) };
            // the icon 28 across, centred 17 down; the name from 33
            look.icon(ui, icon, pos2(rect.center().x, rect.top() + 17.0), 28.0, ink);
            text(
                ui.painter(),
                pos2(rect.center().x, rect.top() + 33.0),
                Align2::CENTER_TOP,
                label,
                13.0,
                Weight::Regular,
                ink,
            );
            if enabled && ui.interact(rect, ui.scope_id().with(("button", i)), Sense::click()).clicked() {
                chosen = Some(choice);
            }
        }
    });
    chosen
}
