//! Crop and Resize, as the Android app's (ui/CropUi.kt): the canvas fitted inside a margin, a frame
//! over it in quarters whose corners and sides are dragged to change what is kept (inside it, to
//! move it), the header with Cancel, Settings, Reset and Done, and the settings: the size (aspect
//! locked or not), DPI, resampling, snapping and the turn. The crop itself is the core's
//! (crop_edit.rs).

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use procraft_core::{CanvasSession, CropShown};

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::panel;
use crate::tool_header::{self, Tapped};

const FRAME_LINE: Themed = Themed(0xCCFFFFFF, 0xCC303030);
const GRID_LINE: Themed = Themed(0x66C8C8C8, 0x99606060);
const MARK: Themed = Themed(0xFFD0D0D0, 0xFF3A3A3A);
const POPOVER: Themed = Themed(0xF21F1F1F, 0xF2DBDEDF);
const FIELD: Themed = Themed(0xFF242424, 0xFFD3D3D5);
const GROUP: Themed = Themed(0xFF2A2A2A, 0xFFE4E4E6);
const ROW_LINE: Themed = Themed(0xFF3A3A3A, 0xFFD6D6D8);
const INK: Themed = Themed(0xFFD2D2D2, 0xFF3B3B3B);
const MUTED: u32 = 0xFF8E8E93;
const TRACK: u32 = 0xFF555555;
/// The settings: 348 wide, 9 from the window's right edge, its arrow 174.5 in.
const SETTINGS_WIDTH: f32 = 348.0;
const SETTINGS_HEIGHT: f32 =
    panel::ARROW_HEIGHT + 10.0 + 40.0 + 10.0 + 44.0 + 10.0 + 44.0 * 3.0 + 2.0 + 10.0 + 64.0 + 10.0;

#[derive(Default)]
pub struct Crop {
    pub open: bool,
    settings_open: bool,
    /// What is typed into the width, height and DPI fields, while one is.
    width: Option<String>,
    height: Option<String>,
    dpi: Option<String>,
}

impl Crop {
    pub fn begin(&mut self, session: &CanvasSession) {
        session.crop_begin();
        self.open = true;
        self.settings_open = false;
    }

    /// Shows it while it is open; returns true when it ends.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) -> bool {
        if !self.open {
            return false;
        }
        let Some(shown) = session.crop_shown() else {
            self.open = false;
            return true;
        };
        let screen = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        egui::Area::new(Id::unique("crop")).fixed_pos(screen.min).order(Order::Middle).show(ctx, |ui| {
            // the whole window takes the frame's drags; nothing reaches the canvas
            let response = ui.allocate_rect(screen, Sense::click_and_drag());
            if response.drag_started() {
                let from = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
                if let Some(p) = from {
                    session.crop_press(p.x * ppp, p.y * ppp);
                }
            }
            if response.dragged()
                && let Some(p) = response.interact_pointer_pos()
            {
                session.crop_drag(p.x * ppp, p.y * ppp);
            }
            if response.drag_stopped() {
                session.crop_release();
            }
            frame(ui, &shown, ppp);
        });
        let buttons = [("Cancel", false), ("Settings", self.settings_open), ("Reset", false)];
        let mut ended = false;
        match tool_header::show(ctx, look, "Crop and Resize", &buttons) {
            Some(Tapped::Button(0)) => {
                session.crop_end(false);
                ended = true;
            }
            Some(Tapped::Button(1)) => self.settings_open = !self.settings_open,
            Some(Tapped::Button(_)) => session.crop_begin(),
            Some(Tapped::Done) => {
                session.crop_end(true);
                ended = true;
            }
            None => {}
        }
        if self.settings_open && !ended {
            self.settings(ctx, look, session, &shown);
        }
        if ended {
            self.open = false;
            self.settings_open = false;
        }
        ended
    }

    /// The settings popover under Settings: the size (aspect locked or not), DPI, resampling,
    /// snapping and the turn.
    fn settings(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, shown: &CropShown) {
        let screen = ctx.content_rect();
        let rect = Rect::from_min_size(
            pos2(screen.right() - 9.0 - SETTINGS_WIDTH, screen.top() + 66.0),
            vec2(SETTINGS_WIDTH, SETTINGS_HEIGHT),
        );
        egui::Area::new(Id::unique("crop settings")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click_and_drag());
            let body = panel::solid_frame(ui, look, "crop settings", rect, 174.5, false, 24.0, POPOVER);
            let inner = body.shrink(10.0);
            let ink = argb(INK);
            text(
                ui.painter(),
                pos2(inner.left() + 14.0, inner.top() + 20.0),
                Align2::LEFT_CENTER,
                "Settings",
                22.0,
                Weight::Bold,
                ink,
            );
            // the size: width, the aspect lock, height
            let row =
                Rect::from_min_size(pos2(inner.left() + 2.0, inner.top() + 50.0), vec2(inner.width() - 4.0, 44.0));
            let w_field = Rect::from_min_size(pos2(row.left(), row.center().y - 20.0), vec2(137.0, 40.0));
            if let Some(v) =
                number_field(ui, w_field, &mut self.width, &shown.size_width.to_string(), Some("px"), "crop width")
            {
                session.crop_set_size(v.parse().ok(), None);
            }
            let lock = Rect::from_min_size(pos2(w_field.right() + 8.0, row.center().y - 20.0), vec2(34.0, 40.0));
            let lock_ink = if shown.aspect_locked { argb(pc::ACCENT) } else { ink };
            look.icon(ui, &icons::LINK, lock.center(), 20.0, lock_ink);
            if ui.interact(lock, ui.scope_id().with("aspect lock"), Sense::click()).clicked() {
                session.crop_set_aspect_locked(!shown.aspect_locked);
            }
            let h_field = Rect::from_min_size(pos2(lock.right() + 8.0, row.center().y - 20.0), vec2(137.0, 40.0));
            if let Some(v) =
                number_field(ui, h_field, &mut self.height, &shown.size_height.to_string(), Some("px"), "crop height")
            {
                session.crop_set_size(None, v.parse().ok());
            }
            // DPI, resampling and snapping
            let group =
                Rect::from_min_size(pos2(inner.left(), row.bottom() + 10.0), vec2(inner.width(), 44.0 * 3.0 + 2.0));
            ui.painter().rect_filled(group, 14.0, argb(GROUP));
            let line = |k: f32| {
                Rect::from_min_size(pos2(group.left(), group.top() + 45.0 * k - 1.0), vec2(group.width(), 1.0))
            };
            ui.painter().rect_filled(line(1.0), 0, argb(ROW_LINE));
            ui.painter().rect_filled(line(2.0), 0, argb(ROW_LINE));
            let row_at =
                |k: f32| Rect::from_min_size(pos2(group.left(), group.top() + 45.0 * k), vec2(group.width(), 44.0));
            let dpi_row = row_at(0.0);
            text(
                ui.painter(),
                pos2(dpi_row.left() + 12.0, dpi_row.center().y),
                Align2::LEFT_CENTER,
                "DPI",
                17.0,
                Weight::Regular,
                ink,
            );
            let dpi_field =
                Rect::from_min_size(pos2(dpi_row.right() - 4.0 - 80.0, dpi_row.center().y - 18.0), vec2(80.0, 36.0));
            if let Some(v) = number_field(ui, dpi_field, &mut self.dpi, &format!("{:.1}", shown.dpi), None, "crop dpi")
                && let Ok(dpi) = v.parse::<f32>()
            {
                session.crop_set_dpi(dpi);
            }
            for (k, (label, on)) in
                [("Resample canvas", shown.resample), ("Snapping", shown.snapping)].into_iter().enumerate()
            {
                let switch_row = row_at(1.0 + k as f32);
                text(
                    ui.painter(),
                    pos2(switch_row.left() + 12.0, switch_row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    ink,
                );
                crate::transform::switch(
                    ui,
                    pos2(switch_row.right() - 6.0 - 63.0, switch_row.center().y - 14.0),
                    on,
                    label,
                );
                if ui.interact(switch_row, ui.scope_id().with(("crop switch", k)), Sense::click()).clicked() {
                    if k == 0 {
                        session.crop_set_resample(!on);
                    } else {
                        session.crop_set_snapping(!on);
                    }
                }
            }
            // the turn, -180 to 180 degrees, dragged anywhere along the bar; a tick marks none
            let turn = Rect::from_min_size(pos2(inner.left(), group.bottom() + 10.0), vec2(inner.width(), 64.0));
            ui.painter().rect_filled(turn, 14.0, argb(GROUP));
            let degrees = shown.angle.to_degrees();
            let top = turn.top() + 8.0;
            text(ui.painter(), pos2(turn.left() + 10.0, top), Align2::LEFT_TOP, "Rotation", 17.0, Weight::Regular, ink);
            text(
                ui.painter(),
                pos2(turn.right() - 10.0, top),
                Align2::RIGHT_TOP,
                &format!("{degrees:.1}\u{b0}"),
                17.0,
                Weight::Regular,
                ink,
            );
            let bar = Rect::from_min_size(
                pos2(turn.left() + 10.0, turn.bottom() - 8.0 - 22.0),
                vec2(turn.width() - 20.0, 22.0),
            );
            let y = bar.center().y;
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(bar.left(), y - 1.5), pos2(bar.right(), y + 1.5)),
                1.5,
                argb(TRACK),
            );
            ui.painter().line_segment(
                [pos2(bar.center().x, y - 6.0), pos2(bar.center().x, y + 6.0)],
                Stroke::new(1.0, argb(MUTED)),
            );
            let knob = vec2(26.0, 15.0);
            let x = (bar.left() + ((degrees + 180.0) / 360.0).clamp(0.0, 1.0) * bar.width())
                .clamp(bar.left() + knob.x / 2.0, bar.right() - knob.x / 2.0);
            ui.painter().rect_filled(Rect::from_center_size(pos2(x, y), knob), 7.5, Color32::WHITE);
            let response = ui.interact(bar, ui.scope_id().with("crop turn"), Sense::click_and_drag());
            if let Some(p) = response.interact_pointer_pos()
                && response.is_pointer_button_down_on()
            {
                let d = ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0) * 360.0 - 180.0;
                // near none, none
                let d = if d.abs() < 2.0 { 0.0 } else { d };
                session.crop_set_angle(d.to_radians());
            }
        });
    }
}

/// The frame: its quarters, its edge, and small marks at its corners and its sides' middles.
fn frame(ui: &Ui, shown: &CropShown, ppp: f32) {
    let cs: Vec<Pos2> = shown.corners.as_chunks::<2>().0.iter().map(|&[x, y]| pos2(x / ppp, y / ppp)).collect();
    let [tl, tr, br, bl] = match cs.as_slice() {
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return,
    };
    let painter = ui.painter();
    let lerp = |a: Pos2, b: Pos2, f: f32| a + (b - a) * f;
    for i in 1..=3 {
        let f = i as f32 / 4.0;
        painter.line_segment([lerp(tl, tr, f), lerp(bl, br, f)], Stroke::new(1.0, argb(GRID_LINE)));
        painter.line_segment([lerp(tl, bl, f), lerp(tr, br, f)], Stroke::new(1.0, argb(GRID_LINE)));
    }
    painter.add(egui::Shape::closed_line(vec![tl, tr, br, bl], Stroke::new(1.0, argb(FRAME_LINE))));
    let (arm, w) = (10.0, 3.0);
    for (p, next, prev) in [(tl, tr, bl), (tr, br, tl), (br, bl, tr), (bl, tl, br)] {
        let unit = |v: egui::Vec2| v / v.length().max(1.0);
        let (to_next, to_prev) = (unit(next - p), unit(prev - p));
        painter.line_segment([p, p + to_next * arm], Stroke::new(w, argb(MARK)));
        painter.line_segment([p, p + to_prev * arm], Stroke::new(w, argb(MARK)));
        let m = lerp(p, next, 0.5);
        painter.line_segment([m - to_next * arm, m + to_next * arm], Stroke::new(w, argb(MARK)));
    }
}

/// A number typed into a capsule `rect`, its `unit` after it; `typed` holds what is typed while it
/// has the focus (`shown` otherwise). Returns the text as it changes.
fn number_field(
    ui: &mut Ui,
    rect: Rect,
    typed: &mut Option<String>,
    shown: &str,
    unit: Option<&str>,
    id: &str,
) -> Option<String> {
    ui.painter().rect_filled(rect, rect.height() / 2.0, argb(FIELD));
    let mut value = typed.clone().unwrap_or_else(|| shown.to_owned());
    let unit_text = unit.map(|u| format!(" {u}")).unwrap_or_default();
    let galley = ui.painter().layout_no_wrap(format!("{value}{unit_text}"), Weight::Regular.font(17.0), argb(INK));
    let unit_galley = ui.painter().layout_no_wrap(unit_text.clone(), Weight::Regular.font(17.0), argb(INK));
    let value_width = galley.size().x - unit_galley.size().x + 2.0;
    let left = rect.center().x - galley.size().x / 2.0;
    let field = Rect::from_min_size(pos2(left, rect.center().y - 11.0), vec2(value_width.max(8.0), 22.0));
    let output = egui::TextEdit::singleline(&mut value)
        .id(ui.scope_id().with(id))
        .font(Weight::Regular.font(17.0))
        .text_color(argb(INK))
        .frame(egui::Frame::NONE)
        .margin(vec2(0.0, 0.0))
        .desired_width(field.width())
        .char_limit(7)
        .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
    if !unit_text.is_empty() {
        let at = pos2(field.right(), rect.center().y);
        text(ui.painter(), at, Align2::LEFT_CENTER, &unit_text, 17.0, Weight::Regular, argb(INK));
    }
    let focused = output.response.has_focus();
    let cleaned: String = value.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    let changed = output.response.changed();
    *typed = focused.then(|| cleaned.clone());
    changed.then_some(cleaned)
}
