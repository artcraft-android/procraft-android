//! A pressure curve's editor, as the Android app's (ui/BrushStudio.kt, PressureCurve and its node
//! menu): the curve through its nodes in a box ruled in quarters; a press off the nodes adds one
//! there, a node is dragged between its neighbours, and a tap on a node shows Delete (a node
//! between the ends) and Reset (the curve straight again).

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::look::{Themed, Weight, argb, pc, text};

const MENU_FILL: u32 = 0xFF101010;
const MENU_WIDTH: f32 = 70.0;
const MENU_ROW: f32 = 36.0;

/// How a curve's box is drawn: its fill, rules and edge, rounding, the curve's colour and width,
/// and its handles' radii (ring, middle).
pub struct CurveLook {
    pub fill: Themed,
    pub grid: Themed,
    pub edge: Themed,
    pub corner: f32,
    pub line: Themed,
    pub line_width: f32,
    pub handle: (f32, f32),
}

/// Pressure and Smoothing's curve.
pub const ACTIONS: CurveLook = CurveLook {
    fill: Themed(0xFF262626, 0xFFE7E7E9),
    grid: Themed(0xFF353535, 0xFFCACACA),
    edge: Themed(0xFF666666, 0xFF999999),
    corner: 0.0,
    line: Themed(0xFFFFFFFF, 0xFF000000),
    line_width: 1.5,
    handle: (7.0, 5.0),
};

/// The brush studio's: a darker box with a light edge, a grey curve, small handles.
pub const STUDIO: CurveLook = CurveLook {
    fill: Themed(0xFF1B1B1B, 0xFFFFFFFF),
    grid: Themed(0xFF3A3A3A, 0xFFCACACA),
    edge: Themed(0xFF353535, 0xFFD8D8D8),
    corner: 8.0,
    line: Themed(0xFFD5D5D5, 0xFF3A3A3A),
    line_width: 1.0,
    handle: (5.5, 3.5),
};

/// The straight curve.
pub fn straight() -> Vec<[f32; 2]> {
    vec![[0.0, 0.0], [1.0, 1.0]]
}

#[derive(Default)]
pub struct CurveEditor {
    /// The node pressed: its index, whether it was just added, and whether it moved.
    press: Option<(usize, bool, bool)>,
    /// The node whose menu shows, where the menu is, and the frame it opened in (the press that
    /// opened it closes nothing).
    menu: Option<(usize, Rect)>,
    menu_frame: u64,
}

impl CurveEditor {
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn close(&mut self) {
        self.menu = None;
        self.press = None;
    }

    /// The curve through `curve` (x, y pairs, 0..1) in `rect`, taking presses; true when it
    /// changed.
    pub fn show(&mut self, ui: &Ui, rect: Rect, curve: &mut Vec<[f32; 2]>, look: &CurveLook, id: &str) -> bool {
        let response = ui.interact(rect, ui.scope_id().with(("curve", id)), Sense::click_and_drag());
        let to_curve = |p: Pos2| {
            [
                ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
                (1.0 - (p.y - rect.top()) / rect.height()).clamp(0.0, 1.0),
            ]
        };
        let near =
            |curve: &[[f32; 2]], at: [f32; 2]| curve.iter().position(|p| (p[0] - at[0]).hypot(p[1] - at[1]) < 0.05);
        let mut changed = false;
        // no curve (switched off): nothing to touch, nothing drawn on the box
        let off = curve.is_empty();
        let down = response.is_pointer_button_down_on() && !off;
        let mut opened = None;
        if down
            && self.press.is_none()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            self.menu = None;
            let at = to_curve(origin);
            let found = near(curve, at);
            let index = found.unwrap_or_else(|| {
                curve.push(at);
                curve.sort_by(|a, b| a[0].total_cmp(&b[0]));
                changed = true;
                curve.iter().position(|p| *p == at).unwrap_or(0)
            });
            self.press = Some((index, found.is_none(), false));
        } else if response.clicked()
            && !off
            && self.press.is_none()
            && let Some(p) = response.interact_pointer_pos()
        {
            // pressed and let go within a frame: on a node its menu, elsewhere a node added
            let at = to_curve(p);
            match near(curve, at) {
                Some(index) => opened = Some(index),
                None => {
                    curve.push(at);
                    curve.sort_by(|a, b| a[0].total_cmp(&b[0]));
                    changed = true;
                }
            }
        }
        if let Some((index, _, moved)) = self.press.as_mut() {
            if down
                && response.dragged()
                && let Some(p) = response.interact_pointer_pos()
            {
                *moved = true;
                let [x, y] = to_curve(p);
                let lo = index.checked_sub(1).and_then(|i| curve.get(i)).map_or(0.0, |p| p[0]);
                let hi = curve.get(*index + 1).map_or(1.0, |p| p[0]);
                if let Some(node) = curve.get_mut(*index) {
                    *node = [x.clamp(lo, hi.max(lo)), y];
                    changed = true;
                }
            }
            if !down {
                // a tap on a node shows what can be done with it
                if let Some((index, false, false)) = self.press {
                    opened = Some(index);
                }
                self.press = None;
            }
        }
        let place = |[x, y]: [f32; 2]| pos2(rect.left() + x * rect.width(), rect.top() + (1.0 - y) * rect.height());
        if let Some(i) = opened
            && let Some(p) = curve.get(i)
        {
            // centred over its node, its bottom 11 above it
            let rows = if i == 0 || i + 1 == curve.len() { 1.0 } else { 2.0 };
            let at = place(*p);
            let menu = Rect::from_min_size(
                pos2(at.x - MENU_WIDTH / 2.0, at.y - 11.0 - MENU_ROW * rows),
                vec2(MENU_WIDTH, MENU_ROW * rows),
            );
            self.menu = Some((i, menu));
            self.menu_frame = ui.ctx().cumulative_frame_nr();
        }
        let painter = ui.painter();
        painter.rect_filled(rect, look.corner, argb(look.fill));
        for k in 1..4 {
            let f = k as f32 / 4.0;
            let (x, y) = (rect.left() + rect.width() * f, rect.top() + rect.height() * f);
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, argb(look.grid)));
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, argb(look.grid)));
        }
        painter.rect_stroke(rect, look.corner, Stroke::new(1.0, argb(look.edge)), egui::StrokeKind::Inside);
        if off {
            return changed;
        }
        let samples = procraft_core::curve_samples(curve.iter().flatten().copied().collect(), 129);
        let last = samples.len().saturating_sub(1).max(1) as f32;
        let line: Vec<Pos2> = samples
            .iter()
            .enumerate()
            .map(|(i, v)| pos2(rect.left() + rect.width() * i as f32 / last, rect.top() + (1.0 - v) * rect.height()))
            .collect();
        painter.add(egui::Shape::line(line, Stroke::new(look.line_width, argb(look.line))));
        for p in curve.iter() {
            painter.circle_filled(place(*p), look.handle.0, Color32::WHITE);
            painter.circle_filled(place(*p), look.handle.1, argb(pc::ACCENT));
        }
        changed
    }

    /// The node menu, while it shows; a press elsewhere closes it. True when the curve changed.
    pub fn menu(&mut self, ctx: &egui::Context, curve: &mut Vec<[f32; 2]>) -> bool {
        let Some((index, menu)) = self.menu else { return false };
        if index >= curve.len() {
            self.menu = None;
            return false;
        }
        let end = index == 0 || index + 1 == curve.len();
        let rows: &[&str] = if end { &["Reset"] } else { &["Delete", "Reset"] };
        let mut picked = None;
        egui::Area::new(Id::unique("curve node menu")).fixed_pos(menu.min).order(Order::Tooltip).show(ctx, |ui| {
            ui.allocate_rect(menu, Sense::click());
            let shadow =
                egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(0x80) };
            ui.painter().add(shadow.as_shape(menu, 12.0));
            ui.painter().rect_filled(menu, 12, argb(MENU_FILL));
            for (k, label) in rows.iter().enumerate() {
                let row = Rect::from_min_size(
                    pos2(menu.left(), menu.top() + MENU_ROW * k as f32),
                    vec2(menu.width(), MENU_ROW),
                );
                text(ui.painter(), row.center(), Align2::CENTER_CENTER, label, 17.0, Weight::Regular, Color32::WHITE);
                if ui.interact(row, ui.scope_id().with(("node", k)), Sense::click()).clicked() {
                    picked = Some(*label);
                }
            }
        });
        let pressed_elsewhere =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !menu.contains(p)));
        match picked {
            Some("Delete") => {
                curve.remove(index);
                self.menu = None;
                true
            }
            Some(_) => {
                *curve = straight();
                self.menu = None;
                true
            }
            None => {
                if pressed_elsewhere && ctx.cumulative_frame_nr() != self.menu_frame {
                    self.menu = None;
                }
                false
            }
        }
    }
}
