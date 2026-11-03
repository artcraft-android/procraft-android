//! Help's Licences, as the Android app's dialog: what the app is made with (the interface font and
//! the icons) and their licences, on a dark panel over the dimmed window, the text scrolling;
//! Done or a tap beside it closes it. The desktop has no settings screen of its own to list the
//! third-party crates (Android's does), so they follow here (desktop/licenses.txt, written by
//! tools/licenses.py).

use std::sync::{Arc, OnceLock};

use egui::{Align2, Color32, Id, Order, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

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
    "

",
    include_str!("../licenses.txt"),
);
/// The licences a paragraph each.
fn paragraphs() -> &'static [&'static str] {
    static PARAGRAPHS: OnceLock<Vec<&'static str>> = OnceLock::new();
    PARAGRAPHS.get_or_init(|| {
        LICENCES
            .split(
                "

",
            )
            .map(str::trim_end)
            .filter(|p| !p.trim().is_empty())
            .collect()
    })
}

/// Where each paragraph starts laid out `width` wide, and how tall they all are: worked out once
/// for a width (the text is long), so only those in sight are laid out and drawn.
fn offsets(ui: &Ui, width: f32) -> Arc<(Vec<f32>, f32)> {
    let id = Id::unique("licences offsets");
    let ppp = ui.ctx().pixels_per_point();
    let kept = ui.ctx().data(|d| d.get_temp::<(f32, f32, Arc<(Vec<f32>, f32)>)>(id));
    if let Some((w, p, offsets)) = kept
        && w == width
        && p == ppp
    {
        return offsets;
    }
    // a blank line between paragraphs
    let gap = galley(ui, " ", width).size().y;
    let mut y = 0.0;
    let mut starts = Vec::with_capacity(paragraphs().len());
    for paragraph in paragraphs() {
        starts.push(y);
        y += galley(ui, paragraph, width).size().y + gap;
    }
    let offsets = Arc::new((starts, y));
    ui.ctx().data_mut(|d| d.insert_temp(id, (width, ppp, Arc::clone(&offsets))));
    offsets
}

fn galley(ui: &Ui, paragraph: &str, width: f32) -> Arc<egui::Galley> {
    ui.painter().layout(paragraph.to_owned(), Weight::Regular.font(14.0), argb(INK), width)
}

/// Shows the licences; true once closed.
pub fn show(ctx: &egui::Context) -> bool {
    let screen = ctx.content_rect();
    let mut close = false;
    egui::Area::new(Id::unique("licences scrim")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
        let beside = ui.allocate_rect(screen, Sense::click());
        ui.painter().rect_filled(screen, 0, Color32::from_black_alpha(0x99));
        close |= crate::ui::escape(ui.ctx()) || beside.clicked();
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
            ScrollArea::vertical().id_salt("licences text").show_viewport(ui, |ui, viewport| {
                let offsets = offsets(ui, body.width());
                let (rect, _) = ui.allocate_exact_size(vec2(body.width(), offsets.1), Sense::hover());
                let first = offsets.0.partition_point(|&y| y <= viewport.top()).saturating_sub(1);
                for (paragraph, y) in paragraphs().iter().zip(&offsets.0).skip(first) {
                    if *y > viewport.bottom() {
                        break;
                    }
                    ui.painter().galley(rect.min + vec2(0.0, *y), galley(ui, paragraph, body.width()), argb(INK));
                }
            });
        });
        let done =
            Rect::from_min_size(pos2(panel.right() - 24.0 - 80.0, panel.bottom() - 16.0 - 40.0), vec2(80.0, 40.0));
        text(ui.painter(), done.center(), Align2::CENTER_CENTER, "Done", 16.0, Weight::Medium, argb(pc::ACCENT));
        close |= ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked();
    });
    close
}
