//! The keyboard shortcuts' list (#31), the core's (keys.rs) with the desktop's own: Ctrl (⌘) held
//! alone a moment shows it for as long as it is held, as a tablet's system shows an app's
//! shortcuts while its command key is held; Help's Keyboard shortcuts opens it until Done, Esc or
//! a click beside it.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Galley, Id, Order, Rect, ScrollArea, Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2};
use procraft_core::ShortcutGroup;

use crate::look::{Weight, argb, pc, text};

/// How long Ctrl (⌘) is held alone before the list shows.
const HOLD: Duration = Duration::from_millis(1000);
const FILL: u32 = 0xF21E1E1E;
const EDGE: u32 = 0x24FFFFFF;
const TITLE: u32 = 0xFFE5E5E5;
const HEADING: u32 = 0xFF8E8E93;
const INK: u32 = 0xFFC5C5C5;
const PAD: f32 = 24.0;
const TITLE_HEIGHT: f32 = 32.0;
const COLUMN: f32 = 360.0;
const COLUMN_GAP: f32 = 24.0;
const KEYS_WIDTH: f32 = 160.0;
const ROW_GAP: f32 = 6.0;
const HEADING_HEIGHT: f32 = 28.0;
const GROUP_GAP: f32 = 14.0;
const DONE: f32 = 40.0;

/// The list, opened from Help or shown while Ctrl (⌘) is held.
#[derive(Default)]
pub struct Shortcuts {
    /// Opened from Help: it shows until closed.
    pub open: bool,
    /// Since when Ctrl (⌘) has been held alone; another key, a press or a scroll meanwhile spoils
    /// the hold until it is let go.
    held_since: Option<Instant>,
    spoiled: bool,
}

impl Shortcuts {
    /// Shows the list if it is open, or Ctrl (⌘) has been held alone long enough while `keys` go
    /// to the canvas (no text being typed).
    pub fn show(&mut self, ctx: &egui::Context, keys: bool) {
        let (alone, other) = ctx.input(|i| {
            let m = i.modifiers;
            let alone = m.command && !m.shift && !m.alt && (!m.ctrl || m.mac_cmd || !cfg!(target_os = "macos"));
            let other = !i.keys_down.is_empty() || i.pointer.any_down() || i.smooth_scroll_delta != egui::Vec2::ZERO;
            (alone, other)
        });
        let mut held = false;
        if !alone || !keys {
            self.held_since = None;
            self.spoiled = false;
        } else if other || self.spoiled {
            self.held_since = None;
            self.spoiled = true;
        } else {
            let since = *self.held_since.get_or_insert_with(Instant::now);
            let for_now = since.elapsed();
            held = for_now >= HOLD;
            if !held {
                ctx.request_repaint_after(HOLD - for_now);
            }
        }
        if self.open {
            self.open = !panel(ctx, true);
        } else if held {
            panel(ctx, false);
        }
    }
}

/// The groups laid out: each row's keys and what it does as galleys, a column `width` wide.
struct Laid {
    heading: String,
    rows: Vec<(std::sync::Arc<Galley>, std::sync::Arc<Galley>)>,
    height: f32,
}

fn lay_out(painter: &egui::Painter, groups: Vec<ShortcutGroup>) -> Vec<Laid> {
    let galley =
        |s: String, width: f32, colour: u32, weight: Weight| painter.layout(s, weight.font(14.0), argb(colour), width);
    groups
        .into_iter()
        .map(|g| {
            let rows: Vec<_> = g
                .rows
                .into_iter()
                .map(|r| {
                    let keys = galley(r.keys, KEYS_WIDTH - 12.0, TITLE, Weight::Medium);
                    let does = galley(r.does, COLUMN - KEYS_WIDTH, INK, Weight::Regular);
                    (keys, does)
                })
                .collect();
            let height = HEADING_HEIGHT
                + rows.iter().map(|(k, d)| k.size().y.max(d.size().y) + ROW_GAP).sum::<f32>()
                + GROUP_GAP;
            Laid { heading: g.heading, rows, height }
        })
        .collect()
}

/// The groups in `columns` columns, in order, each column about as tall as the others.
fn columns(laid: &[Laid], columns: usize) -> Vec<Vec<&Laid>> {
    let total: f32 = laid.iter().map(|g| g.height).sum();
    let target = total / columns as f32;
    let mut out: Vec<Vec<&Laid>> = Vec::new();
    let mut height = 0.0;
    for g in laid {
        let full = out.len() < columns && height + g.height / 2.0 > target;
        match out.last_mut() {
            Some(last) if !full => {
                last.push(g);
                height += g.height;
            }
            _ => {
                out.push(vec![g]);
                height = g.height;
            }
        }
    }
    out
}

/// The list's panel in the middle of the window, with Done and a dimmed window around it if
/// `closable`; true once closed.
fn panel(ctx: &egui::Context, closable: bool) -> bool {
    let screen = ctx.content_rect();
    let mut close = false;
    if closable {
        egui::Area::new(Id::unique("shortcuts scrim")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            let beside = ui.allocate_rect(screen, Sense::click());
            ui.painter().rect_filled(screen, 0, Color32::from_black_alpha(0x99));
            close |= crate::ui::escape(ui.ctx()) || beside.clicked();
        });
    }
    let mac = cfg!(target_os = "macos");
    let layer = egui::LayerId::new(Order::Tooltip, Id::unique("shortcuts"));
    let laid = lay_out(&ctx.layer_painter(layer), procraft_core::key_shortcut_list(true, mac));
    let fit = ((screen.width() - 40.0 - 2.0 * PAD + COLUMN_GAP) / (COLUMN + COLUMN_GAP)).floor();
    let cols = columns(&laid, (fit as usize).clamp(1, 3));
    let body_height = cols.iter().map(|c| c.iter().map(|g| g.height).sum::<f32>()).fold(0.0, f32::max) - GROUP_GAP;
    let width = 2.0 * PAD + cols.len() as f32 * (COLUMN + COLUMN_GAP) - COLUMN_GAP;
    let done_height = if closable { 16.0 + DONE } else { 0.0 };
    let height = PAD + TITLE_HEIGHT + 16.0 + body_height + done_height + PAD;
    let panel = Rect::from_center_size(screen.center(), vec2(width, height.min(screen.height() - 40.0)));
    egui::Area::new(Id::unique("shortcuts")).fixed_pos(panel.min).order(Order::Tooltip).show(ctx, |ui| {
        ui.allocate_rect(panel, Sense::click());
        ui.painter().rect_filled(panel, 28, argb(FILL));
        ui.painter().rect_stroke(panel, 28, Stroke::new(1.0, argb(EDGE)), StrokeKind::Inside);
        let title = pos2(panel.left() + PAD, panel.top() + PAD);
        text(ui.painter(), title, Align2::LEFT_TOP, "Keyboard shortcuts", 24.0, Weight::Bold, argb(TITLE));
        let body = Rect::from_min_max(
            pos2(panel.left() + PAD, panel.top() + PAD + TITLE_HEIGHT + 16.0),
            pos2(panel.right() - PAD, panel.bottom() - PAD - done_height),
        );
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            ScrollArea::vertical().id_salt("shortcuts list").show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(body.width(), body_height), Sense::hover());
                let painter = ui.painter();
                for (c, column) in cols.iter().enumerate() {
                    let x = rect.left() + c as f32 * (COLUMN + COLUMN_GAP);
                    let mut y = rect.top();
                    for group in column {
                        text(painter, pos2(x, y), Align2::LEFT_TOP, &group.heading, 13.0, Weight::Bold, argb(HEADING));
                        y += HEADING_HEIGHT;
                        for (keys, does) in &group.rows {
                            painter.galley(pos2(x, y), keys.clone(), argb(TITLE));
                            painter.galley(pos2(x + KEYS_WIDTH, y), does.clone(), argb(INK));
                            y += keys.size().y.max(does.size().y) + ROW_GAP;
                        }
                        y += GROUP_GAP;
                    }
                }
            });
        });
        if closable {
            let done = Rect::from_min_size(
                pos2(panel.right() - PAD - 80.0, panel.bottom() - PAD + 8.0 - DONE),
                vec2(80.0, DONE),
            );
            text(ui.painter(), done.center(), Align2::CENTER_CENTER, "Done", 16.0, Weight::Medium, argb(pc::ACCENT));
            close |= ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked();
        }
    });
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_groups_fill_columns_in_order_about_evenly() {
        let laid = |h: &[f32]| -> Vec<Laid> {
            h.iter().map(|&height| Laid { heading: String::new(), rows: Vec::new(), height }).collect()
        };
        let groups = laid(&[200.0, 220.0, 100.0, 80.0, 180.0, 110.0]);
        let heights = |n: usize| -> Vec<Vec<f32>> {
            columns(&groups, n).iter().map(|c| c.iter().map(|g| g.height).collect()).collect()
        };
        assert_eq!(heights(1), vec![vec![200.0, 220.0, 100.0, 80.0, 180.0, 110.0]]);
        // 420 and 470 tall
        assert_eq!(heights(2), vec![vec![200.0, 220.0], vec![100.0, 80.0, 180.0, 110.0]]);
        assert_eq!(heights(3), vec![vec![200.0], vec![220.0, 100.0], vec![80.0, 180.0, 110.0]]);
    }
}
