//! The keyboard shortcuts' list (#31), the core's (keys.rs) with the desktop's own: Ctrl (⌘) held
//! alone a moment shows it for as long as it is held, as a tablet's system shows an app's
//! shortcuts while its command key is held; Help's Keyboard shortcuts opens it until Done, Esc or
//! a click beside it. A shortcut's keys clicked there are bound again: the next key pressed takes
//! them, Backspace takes them away, Esc gives up; the list keeps them between runs (prefs.rs).

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Align2, Color32, Galley, Id, Key, Order, Rect, ScrollArea, Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2,
};
use procraft_core::{KeyAction, KeyBindings, KeyCombo, ShortcutGroup};

use crate::look::{Weight, argb, pc, text};
use crate::prefs::Prefs;

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
/// The keys a shortcut's row is bound again from, lit while the pointer is over them.
const OVER: u32 = 0x1FFFFFFF;

/// The list, opened from Help or shown while Ctrl (⌘) is held.
#[derive(Default)]
pub struct Shortcuts {
    /// Opened from Help: it shows until closed.
    pub open: bool,
    /// Since when Ctrl (⌘) has been held alone; another key, a press or a scroll meanwhile spoils
    /// the hold until it is let go.
    held_since: Option<Instant>,
    spoiled: bool,
    /// The shortcut whose keys are being chosen: the next key pressed is bound to it.
    capturing: Option<KeyAction>,
}

impl Shortcuts {
    /// Whether the list is showing and taking the keys, so the canvas's shortcuts wait.
    pub fn takes_keys(&self) -> bool {
        self.open || self.capturing.is_some()
    }

    /// Shows the list if it is open, or Ctrl (⌘) has been held alone long enough while `keys` go
    /// to the canvas (no text being typed) and the setting for it is on. A shortcut's keys clicked
    /// are bound again, and the bindings kept: true if one changed.
    pub fn show(&mut self, ctx: &egui::Context, keys: bool, prefs: &mut Prefs) -> bool {
        let (alone, other) = ctx.input(|i| {
            let m = i.modifiers;
            let alone = m.command && !m.shift && !m.alt && (!m.ctrl || m.mac_cmd || !cfg!(target_os = "macos"));
            // the modifiers held are not presses that spoil the hold: Ctrl itself is the ask
            let other = spoiled(&i.keys_down, i.pointer.any_down(), i.smooth_scroll_delta != egui::Vec2::ZERO);
            (alone, other)
        });
        let mut held = false;
        if self.capturing.is_some() {
            // the list stays, however Ctrl is held, while a key is being chosen
            self.held_since = None;
        } else if !prefs.shortcuts_on_ctrl {
            // the list is asked for from Help alone
            self.held_since = None;
            self.spoiled = false;
        } else if !alone || !keys {
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
        let mut changed = false;
        if self.open {
            let (close, bound) = panel(ctx, true, &mut self.capturing, &mut prefs.keys);
            self.open = !close;
            if close {
                // nothing is left waiting for a key once the list is put away
                self.capturing = None;
            }
            changed |= bound;
        } else if held {
            let (_, bound) = panel(ctx, false, &mut self.capturing, &mut prefs.keys);
            changed |= bound;
        }
        changed
    }
}

/// Whether `key` is a modifier itself (Shift, Ctrl, Alt, Super: either side). Holding Ctrl is how
/// the list is asked for, so it must not count as another key pressed.
fn is_modifier(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}

/// Whether something else was pressed, scrolled or clicked since: the list is only for Ctrl (⌘)
/// held on its own, so a key (not a modifier), a pointer press or a scroll spoils it.
fn spoiled(keys: &std::collections::HashSet<egui::Key>, pointer_down: bool, scrolled: bool) -> bool {
    pointer_down || scrolled || keys.iter().any(|key| !is_modifier(*key))
}

/// One row of the list laid out: its keys and what it does as galleys, and the action the keys can
/// be bound to again (none where the platform keeps them).
struct LaidRow {
    keys: Arc<Galley>,
    does: Arc<Galley>,
    action: Option<KeyAction>,
}

/// The rows of a group laid out: its heading, its rows, and how tall it is.
struct Laid {
    heading: String,
    rows: Vec<LaidRow>,
    height: f32,
}

/// The groups laid out: each row's keys and what it does as galleys, a column `width` wide.
fn lay_out(painter: &egui::Painter, groups: Vec<ShortcutGroup>) -> Vec<Laid> {
    let galley =
        |s: String, width: f32, colour: u32, weight: Weight| painter.layout(s, weight.font(14.0), argb(colour), width);
    groups
        .into_iter()
        .map(|g| {
            let rows: Vec<LaidRow> = g
                .rows
                .into_iter()
                .map(|r| {
                    let keys = galley(r.keys, KEYS_WIDTH - 12.0, TITLE, Weight::Medium);
                    let does = galley(r.does, COLUMN - KEYS_WIDTH, INK, Weight::Regular);
                    LaidRow { keys, does, action: r.action }
                })
                .collect();
            let height = HEADING_HEIGHT
                + rows.iter().map(|row| row.keys.size().y.max(row.does.size().y) + ROW_GAP).sum::<f32>()
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

/// Takes the key pressed while `capturing`, if one was: it becomes `action`'s press; Backspace
/// (alone) takes the action's presses away; Escape gives up. True when the bindings changed.
fn catch(
    ctx: &egui::Context,
    capturing: &mut Option<KeyAction>,
    bindings: &mut KeyBindings,
    action: KeyAction,
) -> bool {
    let pressed = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
            _ => None,
        })
    });
    let Some((key, modifiers)) = pressed else {
        // keys come one at a time; look again next frame
        ctx.request_repaint();
        return false;
    };
    ctx.input_mut(|i| i.consume_key(modifiers, key));
    if key == Key::Escape {
        *capturing = None;
        return false;
    }
    if key == Key::Backspace && !modifiers.command && !modifiers.shift && !modifiers.alt {
        bindings.unbind(action);
        *capturing = None;
        return true;
    }
    let Some(name) = crate::ui::key_name(key) else { return false };
    // Ctrl with Alt is AltGr typing a key, not a press to bind; nor is a key that cannot be bound
    if !procraft_core::bindable_key(name) || (modifiers.command && modifiers.alt) {
        return false;
    }
    let combo =
        KeyCombo { key: name.to_lowercase(), command: modifiers.command, shift: modifiers.shift, alt: modifiers.alt };
    bindings.bind(action, combo);
    *capturing = None;
    true
}

/// The list's panel in the middle of the window, with Done and a dimmed window around it if
/// `closable`; its shortcuts' keys can be bound again while it is. (True once closed, true if a
/// binding changed.)
fn panel(
    ctx: &egui::Context,
    closable: bool,
    capturing: &mut Option<KeyAction>,
    bindings: &mut KeyBindings,
) -> (bool, bool) {
    let mut close = false;
    let mut changed = false;
    if closable && let Some(action) = *capturing {
        changed = catch(ctx, capturing, bindings, action);
    }
    let screen = ctx.content_rect();
    if closable {
        egui::Area::new(Id::unique("shortcuts scrim")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            let beside = ui.allocate_rect(screen, Sense::click());
            ui.painter().rect_filled(screen, 0, Color32::from_black_alpha(0x99));
            close |= crate::ui::escape(ui.ctx()) || beside.clicked();
        });
    }
    let mac = cfg!(target_os = "macos");
    let layer = egui::LayerId::new(Order::Tooltip, Id::unique("shortcuts"));
    let laid = lay_out(&ctx.layer_painter(layer), procraft_core::key_shortcut_list_with(true, mac, bindings.clone()));
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
                for (c, column) in cols.iter().enumerate() {
                    let x = rect.left() + c as f32 * (COLUMN + COLUMN_GAP);
                    let mut y = rect.top();
                    for group in column {
                        text(
                            ui.painter(),
                            pos2(x, y),
                            Align2::LEFT_TOP,
                            &group.heading,
                            13.0,
                            Weight::Bold,
                            argb(HEADING),
                        );
                        y += HEADING_HEIGHT;
                        for row in &group.rows {
                            let row_height = row.keys.size().y.max(row.does.size().y);
                            // the whole slot the row sits in, gap and all, so a click between
                            // two rows' lines still chooses one of them
                            let keys = Rect::from_min_size(pos2(x, y), vec2(KEYS_WIDTH, row_height + ROW_GAP));
                            let mut over = false;
                            if closable && let Some(action) = row.action {
                                let response =
                                    ui.interact(keys, ui.scope_id().with(("shortcut", action.key())), Sense::click());
                                over = response.hovered();
                                if over {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if response.clicked() {
                                    *capturing = Some(action);
                                }
                            }
                            if row.action.is_some() && *capturing == row.action {
                                text(
                                    ui.painter(),
                                    pos2(x, y),
                                    Align2::LEFT_TOP,
                                    "Press a key…",
                                    15.0,
                                    Weight::Medium,
                                    argb(pc::ACCENT),
                                );
                            } else {
                                if over {
                                    let lit = keys.shrink2(vec2(3.0, ROW_GAP * 0.5));
                                    ui.painter().rect_filled(lit, 6, argb(OVER));
                                }
                                ui.painter().galley(pos2(x, y), row.keys.clone(), argb(TITLE));
                            }
                            ui.painter().galley(pos2(x + KEYS_WIDTH, y), row.does.clone(), argb(INK));
                            y += row_height + ROW_GAP;
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
            let reset = Rect::from_min_size(pos2(done.left() - 88.0, done.top()), vec2(72.0, DONE));
            text(ui.painter(), reset.center(), Align2::CENTER_CENTER, "Reset", 15.0, Weight::Medium, argb(HEADING));
            if ui.interact(reset, ui.scope_id().with("reset"), Sense::click()).clicked() {
                bindings.reset();
                *capturing = None;
                changed = true;
            }
            text(
                ui.painter(),
                pos2(panel.left() + PAD, done.center().y),
                Align2::LEFT_CENTER,
                "Click a shortcut to change it · Esc cancels · Backspace clears it",
                13.0,
                Weight::Regular,
                argb(HEADING),
            );
        }
    });
    (close, changed)
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

    #[test]
    fn ctrl_held_alone_is_not_spoiled_by_the_modifier_itself() {
        let mut keys = std::collections::HashSet::new();
        keys.insert(Key::ControlLeft);
        assert!(!spoiled(&keys, false, false), "Ctrl is how the list is asked for");
        // either Ctrl, Shift, Alt or Super, either side
        for modifier in [
            Key::ShiftLeft,
            Key::ShiftRight,
            Key::ControlLeft,
            Key::ControlRight,
            Key::AltLeft,
            Key::AltRight,
            Key::SuperLeft,
            Key::SuperRight,
        ] {
            let one: std::collections::HashSet<Key> = [modifier].into_iter().collect();
            assert!(!spoiled(&one, false, false), "{modifier:?}");
        }
        // another key, a click or a scroll spoils it
        keys.insert(Key::Z);
        assert!(spoiled(&keys, false, false), "Z is another key");
        keys.remove(&Key::Z);
        assert!(spoiled(&keys, true, false), "a pointer press");
        assert!(spoiled(&keys, false, true), "a scroll");
    }
}
