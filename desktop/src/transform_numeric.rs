//! Numeric entry for an open transform, as the Android app's (ui/TransformNumericUi.kt): from a tap
//! on a node, "Dimensions" (the box's width and height, linked or not) or "Rotation" (its angle),
//! typed on a keypad. Each key changes the box at once; a tap outside closes it.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Ui, pos2, vec2};
use procraft_core::{CanvasSession, TransformBox};

use crate::icons;
use crate::look::{Look, Weight, argb, pc, text};
use crate::panel::{self, ARROW_HEIGHT};

const FIELD_IDLE: u32 = 0xFF383838;
const TOP_KEY: u32 = 0xFF2E2E2E;
const DIGIT_KEY: u32 = 0xFF414141;
const KEY_INK: u32 = 0xFFDADADA;
const LINK_ON: u32 = 0xFF1F436B;
const WIDTH: f32 = 248.0;
const CORNER: f32 = 20.0;
/// How far the arrow's tip stands from the node's middle.
const NODE_GAP: f32 = 5.0;
/// The title's band, the fields' row and the gap under it; the keys, their gaps, and the margin.
const TITLE: f32 = 48.0;
const FIELD_HEIGHT: f32 = 41.0;
const KEYS_GAP: f32 = 14.0;
const KEY: (f32, f32) = (71.0, 48.0);
const KEY_GAPS: (f32, f32) = (7.0, 8.0);
const ZERO_WIDTH: f32 = 150.0;
const MARGIN: f32 = 10.0;
const HEIGHT: f32 = TITLE + FIELD_HEIGHT + KEYS_GAP + 5.0 * KEY.1 + 4.0 * KEY_GAPS.1 + MARGIN;
const KEYS: [[&str; 3]; 4] = [["AC", "±", "⌫"], ["7", "8", "9"], ["4", "5", "6"], ["1", "2", "3"]];

pub struct Numeric {
    angle: bool,
    /// The node's middle (points), and whether the panel goes on its right.
    at: Pos2,
    right: bool,
    /// The field typed into (0 the width or the angle, 1 the height), what is typed so far, and
    /// whether the width and height keep their ratio.
    field: usize,
    typed: String,
    linked: bool,
}

impl Numeric {
    pub fn new(angle: bool, at: Pos2, right: bool, linked: bool) -> Numeric {
        Numeric { angle, at, right, field: 0, typed: String::new(), linked }
    }

    fn value(&self, b: &TransformBox, i: usize) -> f32 {
        if self.angle {
            b.angle
        } else if i == 0 {
            b.width
        } else {
            b.height
        }
    }

    fn shown(&self, b: &TransformBox, i: usize) -> String {
        let unit = if self.angle { " º" } else { " px" };
        if self.field == i && !self.typed.is_empty() {
            return format!("{}{unit}", self.typed);
        }
        let v = self.value(b, i);
        let v = if self.angle { v.rem_euclid(360.0) } else { v };
        format!("{}{unit}", v.round() as i32)
    }

    /// A key: what is typed changes, and goes on the box.
    fn key(&mut self, k: &str, session: &CanvasSession, b: &TransformBox) -> Option<TransformBox> {
        let typed = std::mem::take(&mut self.typed);
        self.typed = match k {
            "AC" => String::new(),
            "±" => match typed.strip_prefix('-') {
                Some(rest) => rest.to_owned(),
                None if typed.is_empty() => format!("-{}", self.value(b, self.field).round() as i32),
                None => format!("-{typed}"),
            },
            "⌫" => {
                let mut t = typed;
                t.pop();
                t
            }
            "." if typed.contains('.') => typed,
            "." if typed.is_empty() => "0.".to_owned(),
            "." => typed + ".",
            digit if typed.chars().count() < 7 => typed + digit,
            _ => typed,
        };
        self.apply(session, b)
    }

    /// What is typed put on the box: the angle, or the width or height (the other kept in ratio
    /// when linked).
    fn apply(&self, session: &CanvasSession, b: &TransformBox) -> Option<TransformBox> {
        let v: f32 = self.typed.parse().ok()?;
        if self.angle {
            return session.transform_set_angle(v);
        }
        let (w, h) = (b.width, b.height);
        match (self.field, self.linked) {
            (0, true) if w > 0.0 => session.transform_set_size(v, h * v.abs() / w),
            (0, _) => session.transform_set_size(v, h),
            (_, true) if h > 0.0 => session.transform_set_size(w * v.abs() / h, v),
            _ => session.transform_set_size(w, v),
        }
    }

    /// Draws the panel beside its node, its arrow pointing at it, over a clear layer a tap on
    /// which closes it. Returns the box as a key changed it, and whether the panel stays open.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        b: &TransformBox,
        backdrops: &mut Vec<f32>,
    ) -> (Option<TransformBox>, bool) {
        let screen = ctx.content_rect();
        // Escape closes it, as a tap beside it does
        let mut open = !crate::ui::escape(ctx);
        egui::Area::new(Id::unique("transform numbers scrim")).fixed_pos(screen.min).order(Order::Foreground).show(
            ctx,
            |ui| {
                if ui.allocate_rect(screen, Sense::click()).clicked() {
                    open = false;
                }
            },
        );
        // beside the node, on its side of the box, level with it, kept on the screen
        let whole = WIDTH + ARROW_HEIGHT;
        let left = if self.right { self.at.x + NODE_GAP } else { self.at.x - NODE_GAP - whole };
        let left = left.clamp(screen.left() + 8.0, (screen.right() - whole - 8.0).max(screen.left() + 8.0));
        let top = (self.at.y - HEIGHT / 2.0)
            .clamp(screen.top() + 8.0, (screen.bottom() - HEIGHT - 8.0).max(screen.top() + 8.0));
        let arrow_y = (self.at.y - top).clamp(28.0, HEIGHT - 28.0);
        let rect = Rect::from_min_size(pos2(left, top), vec2(whole, HEIGHT));
        let mut changed = None;
        egui::Area::new(Id::unique("transform numbers")).fixed_pos(rect.min).order(Order::Tooltip).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            let body = panel::side_frame(ui, look, "transform numbers", rect, arrow_y, self.right, CORNER, backdrops);
            let inner = Rect::from_min_max(pos2(body.left() + MARGIN, body.top()), body.max - vec2(MARGIN, MARGIN));
            let ink = argb(KEY_INK);
            let title = if self.angle { "Rotation" } else { "Dimensions" };
            text(ui.painter(), inner.min + vec2(10.0, 12.0), Align2::LEFT_TOP, title, 20.0, Weight::Bold, ink);
            let row = inner.top() + TITLE;
            if self.angle {
                let band = Rect::from_min_size(pos2(inner.left(), row), vec2(inner.width(), FIELD_HEIGHT));
                ui.painter().rect_filled(band, 10, argb(FIELD_IDLE));
                text(
                    ui.painter(),
                    pos2(band.left() + 12.0, band.center().y),
                    Align2::LEFT_CENTER,
                    "Angle",
                    15.0,
                    Weight::Regular,
                    ink,
                );
                let field = Rect::from_min_size(pos2(band.right() - 80.0, row), vec2(80.0, FIELD_HEIGHT));
                if number_field(ui, field, &self.shown(b, 0), true, 0) {
                    self.field = 0;
                }
            } else {
                let width = Rect::from_min_size(pos2(inner.left(), row), vec2(80.0, FIELD_HEIGHT));
                let link = Rect::from_min_size(pos2(width.right() + 6.0, row + 3.0), vec2(52.0, 35.0));
                let height = Rect::from_min_size(pos2(link.right() + 6.0, row), vec2(80.0, FIELD_HEIGHT));
                for (i, r) in [(0, width), (1, height)] {
                    if number_field(ui, r, &self.shown(b, i), self.field == i, i) {
                        self.field = i;
                        self.typed.clear();
                    }
                }
                ui.painter().rect_filled(link, 9, if self.linked { argb(LINK_ON) } else { argb(FIELD_IDLE) });
                let tint = if self.linked { argb(pc::ACCENT) } else { ink };
                look.icon(ui, &icons::LINK, link.center(), 22.0, tint);
                if ui.interact(link, ui.scope_id().with("link"), Sense::click()).clicked() {
                    self.linked = !self.linked;
                }
            }
            // the keypad: four rows of three, then 0 (two keys wide) and the point
            let keys = row + FIELD_HEIGHT + KEYS_GAP;
            let mut pressed = None;
            let key_at = |column: f32, line: f32, wide: f32| {
                Rect::from_min_size(
                    pos2(inner.left() + column * (KEY.0 + KEY_GAPS.0), keys + line * (KEY.1 + KEY_GAPS.1)),
                    vec2(wide, KEY.1),
                )
            };
            for (line, labels) in KEYS.iter().enumerate() {
                let fill = if line == 0 { TOP_KEY } else { DIGIT_KEY };
                for (column, &k) in labels.iter().enumerate() {
                    if key(ui, look, key_at(column as f32, line as f32, KEY.0), k, fill) {
                        pressed = Some(k);
                    }
                }
            }
            let zero = key_at(0.0, 4.0, ZERO_WIDTH);
            if key(ui, look, zero, "0", DIGIT_KEY) {
                pressed = Some("0");
            }
            let point = Rect::from_min_size(pos2(zero.right() + KEY_GAPS.0, zero.top()), vec2(KEY.0, KEY.1));
            if key(ui, look, point, ".", DIGIT_KEY) {
                pressed = Some(".");
            }
            if let Some(k) = pressed {
                changed = self.key(k, session, b);
            }
        });
        (changed, open)
    }
}

/// A number's field: blue while it is the one typed into. Returns whether it was tapped.
fn number_field(ui: &Ui, rect: Rect, shown: &str, chosen: bool, index: usize) -> bool {
    ui.painter().rect_filled(rect, 10, if chosen { argb(pc::ACCENT) } else { argb(FIELD_IDLE) });
    text(ui.painter(), rect.center(), Align2::CENTER_CENTER, shown, 15.0, Weight::Regular, Color32::WHITE);
    ui.interact(rect, ui.scope_id().with(("number field", index)), Sense::click()).clicked()
}

/// A key of the keypad. Returns whether it was tapped.
fn key(ui: &Ui, look: &mut Look, rect: Rect, label: &str, fill: u32) -> bool {
    ui.painter().rect_filled(rect, 9, argb(fill));
    let ink = argb(KEY_INK);
    if label == "⌫" {
        look.icon(ui, &icons::BACKSPACE, rect.center(), 22.0, ink);
    } else {
        let size = if label.chars().count() > 1 { 15.0 } else { 22.0 };
        text(ui.painter(), rect.center(), Align2::CENTER_CENTER, label, size, Weight::Regular, ink);
    }
    ui.interact(rect, ui.scope_id().with(("numeric key", label)), Sense::click()).clicked()
}
