//! Layer Select's list, as the Android app's (ui/QuickMenuUi.kt's LayerChoice): the layers drawn
//! under a point, top first, the current one blue; a tap on one makes it current, a tap elsewhere
//! closes the list.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, pos2, vec2};
use procraft_core::LayerInfo;

use crate::look::{Weight, argb, pc, text};
use crate::panel::PANEL_TITLE;

const FILL: u32 = 0xFF383838;
const WIDTH: f32 = 200.0;
const ROW: f32 = 44.0;

/// What was done with the list.
pub enum Chose {
    Layer(u64),
    Nothing,
}

/// Draws the list of `ids` (of `layers`) by `at` (points), kept in the window.
pub fn show(ctx: &egui::Context, at: Pos2, ids: &[u64], layers: &[LayerInfo]) -> Option<Chose> {
    // Escape chooses nothing, as a tap beside it does
    if crate::ui::escape(ctx) {
        return Some(Chose::Nothing);
    }
    let screen = ctx.content_rect();
    let rows: Vec<&LayerInfo> = ids.iter().filter_map(|id| layers.iter().find(|l| l.id == *id)).collect();
    let height = rows.len() as f32 * ROW + 16.0;
    let x = at.x.clamp(screen.left() + 8.0, (screen.right() - WIDTH - 8.0).max(screen.left() + 8.0));
    let y = (at.y + 24.0).clamp(screen.top() + 8.0, (screen.bottom() - height).max(screen.top() + 8.0));
    let list = Rect::from_min_size(pos2(x, y), vec2(WIDTH, height));
    let mut chose = None;
    egui::Area::new(Id::unique("layer choice")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
        if ui.allocate_rect(screen, Sense::click_and_drag()).clicked() {
            chose = Some(Chose::Nothing);
        }
        ui.interact(list, ui.scope_id().with("list"), Sense::click_and_drag());
        ui.painter().rect_filled(list, 16, argb(FILL));
        let painter = ui.painter_at(list);
        for (i, l) in rows.iter().enumerate() {
            let row = Rect::from_min_size(pos2(list.left(), list.top() + 8.0 + ROW * i as f32), vec2(WIDTH, ROW));
            if l.current {
                painter.rect_filled(row, 0, argb(pc::ACCENT));
            }
            let ink = if l.current { Color32::WHITE } else { argb(PANEL_TITLE) };
            text(
                &painter,
                pos2(row.left() + 16.0, row.center().y),
                Align2::LEFT_CENTER,
                &l.name,
                16.0,
                Weight::Regular,
                ink,
            );
            if ui.interact(row, ui.scope_id().with(("layer", l.id)), Sense::click()).clicked() {
                chose = Some(Chose::Layer(l.id));
            }
        }
    });
    chose
}
