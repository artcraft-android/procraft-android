//! Liquify's panel, as the Android app's (ui/AdjustmentsUi.kt): its ways, Adjust and Reset, and
//! the brush's size, pressure, distortion and momentum. Where the window is wide, the ways, Adjust
//! and Reset are a row of icon buttons over the four sliders; where it is narrow, two rows of
//! sliders over the way (its list in a popover), Adjust and Reset. Adjust's popover takes back
//! some of what it did. The touches liquify (touch.rs); the values are the core's (adjusting.rs).

use egui::{Align2, Color32, Id, Order, Rect, Sense, Ui, pos2, vec2};
use procraft_core::{AdjustShown, CanvasSession, LiquifyKind};

use crate::icons::{self, Icon};
use crate::look::{Look, Shade, Themed, Weight, argb, pc, text};
use crate::transform::{
    MENU_ARROW, MENU_GROUP, MENU_LINE, MENU_TEXT, MENU_WIDTH, menu_slider, menu_title, popover_frame,
};

const PANEL_FILL: Themed = Themed(0xF71E1E1E, 0xF7F1F1F1);
const PANEL_RISE: f32 = 25.0;
const PANEL_BUTTON: Themed = Themed(0xFF303030, 0xFFF8F8F8);
const IDLE: Themed = Themed(0xFFAAAAAA, 0xFF414141);
const LABEL: Themed = Themed(0xFFA3A3A3, 0xFF414141);
const DIM: Themed = Themed(0xFF575757, 0xFF989898);
const WIDE: (f32, f32) = (1093.0, 162.0);
const UPRIGHT: (f32, f32) = (654.0, 200.0);

const KINDS: [(LiquifyKind, &str, &Icon); 8] = [
    (LiquifyKind::Push, "Push", &icons::LIQUIFY_PUSH),
    (LiquifyKind::TwirlRight, "Twirl Right", &icons::LIQUIFY_TWIRL_RIGHT),
    (LiquifyKind::TwirlLeft, "Twirl Left", &icons::LIQUIFY_TWIRL_LEFT),
    (LiquifyKind::Pinch, "Pinch", &icons::LIQUIFY_PINCH),
    (LiquifyKind::Expand, "Expand", &icons::LIQUIFY_EXPAND),
    (LiquifyKind::Crystals, "Crystals", &icons::LIQUIFY_CRYSTALS),
    (LiquifyKind::Edge, "Edge", &icons::LIQUIFY_EDGE),
    (LiquifyKind::Reconstruct, "Reconstruct", &icons::LIQUIFY_RECONSTRUCT),
];
const SLIDERS: [(&str, Option<&str>); 4] =
    [("Size", None), ("Pressure", None), ("Distortion", Some("None")), ("Momentum", Some("None"))];

/// A slider's value as shown: Max at all, `none` (if any) at nothing, else a percentage.
fn percent(v: f32, none: Option<&str>) -> String {
    if v >= 1.0 {
        "Max".into()
    } else if let (true, Some(none)) = (v <= 0.0, none) {
        none.into()
    } else {
        format!("{}%", (v * 100.0).round() as i32)
    }
}

/// The popover open over the panel's buttons.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Popup {
    #[default]
    None,
    Kinds,
    Adjust,
}

#[derive(Default)]
pub struct Liquify {
    popup: Popup,
}

impl Liquify {
    /// The panel, while Liquify is open.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, shown: &AdjustShown) {
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        let (w, h) = if wide { WIDE } else { UPRIGHT };
        let rect = Rect::from_min_size(pos2(screen.center().x - w / 2.0, screen.bottom() - PANEL_RISE - h), vec2(w, h));
        // the popovers point at a button: the first (the way) upright, Adjust (the ninth) wide
        let (tip_x, tip_y) = if wide {
            (rect.left() + 86.0 + 94.0 * 8.0 + 37.5, rect.top() + 28.0)
        } else {
            (rect.left() + 16.0 + (UPRIGHT.0 - 32.0 - 16.0) / 6.0, rect.bottom() - 16.0 - 40.0)
        };
        let adjust_tip_x = if wide { tip_x } else { screen.center().x };
        egui::Area::new(Id::unique("liquify panel")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            ui.painter().rect_filled(rect, 33, argb(PANEL_FILL));
            if wide {
                self.wide(ui, look, session, shown, rect);
            } else {
                self.upright(ui, look, session, shown, rect);
            }
        });
        // Escape closes the popover
        if self.popup != Popup::None && crate::ui::escape(ctx) {
            self.popup = Popup::None;
        }
        match self.popup {
            Popup::Kinds if !wide => self.kinds_popover(ctx, look, session, shown, tip_x, tip_y),
            Popup::Adjust => self.adjust_popover(ctx, session, shown, adjust_tip_x, tip_y),
            _ => {}
        }
    }

    /// Wide: the ways, Adjust and Reset as a row of icon buttons (75 wide, 94 apart; the chosen
    /// way's icon blue), and the four sliders side by side under them.
    fn wide(&mut self, ui: &mut Ui, look: &mut Look, session: &CanvasSession, shown: &AdjustShown, rect: Rect) {
        let can_adjust = shown.liquified;
        let left = rect.left() + 86.0;
        let top = rect.top() + 28.0;
        for (k, &(kind, label, glyph)) in KINDS.iter().enumerate() {
            let ink = if shown.liquify_kind == kind { pc::ACCENT.into() } else { IDLE };
            if button(ui, look, pos2(left + 94.0 * k as f32, top), glyph, label, ink, LABEL, 24.0, true) {
                session.adjustment_liquify_kind(kind);
                self.popup = Popup::None;
            }
        }
        let (ink, words) = if can_adjust { (IDLE, LABEL) } else { (DIM, DIM) };
        if button(ui, look, pos2(left + 94.0 * 8.0, top), &icons::ADJUST_KNOB, "Adjust", ink, words, 39.5, can_adjust) {
            self.popup = if self.popup == Popup::Adjust { Popup::None } else { Popup::Adjust };
        }
        if button(ui, look, pos2(left + 94.0 * 9.0, top), &icons::PIE_RESET, "Reset", ink, words, 23.0, can_adjust) {
            session.adjustment_liquify_reset();
            self.popup = Popup::None;
        }
        let each = (rect.width() - 32.0 - 33.0) / 4.0;
        for (i, (label, none)) in SLIDERS.iter().enumerate() {
            let row = Rect::from_min_size(
                pos2(rect.left() + 16.0 + i as f32 * (each + 11.0), top + 60.0 + 8.0),
                vec2(each, 59.0),
            );
            let v = shown.liquify.get(i).copied().unwrap_or(0.0);
            if let Some(v) = menu_slider(ui, row, label, v, &percent(v, *none)) {
                session.adjustment_liquify_value(i as u32, v);
            }
        }
    }

    /// Upright: two rows of two sliders, then the way, Adjust and Reset.
    fn upright(&mut self, ui: &mut Ui, look: &mut Look, session: &CanvasSession, shown: &AdjustShown, rect: Rect) {
        let inner = rect.shrink(16.0);
        let each = (inner.width() - 20.0) / 2.0;
        for (i, (label, none)) in SLIDERS.iter().enumerate() {
            let (row, col) = (i / 2, i % 2);
            let top = inner.top() + row as f32 * (59.0 + 5.0);
            let r = Rect::from_min_size(pos2(inner.left() + col as f32 * (each + 20.0), top), vec2(each, 59.0));
            let v = shown.liquify.get(i).copied().unwrap_or(0.0);
            if let Some(v) = menu_slider(ui, r, label, v, &percent(v, *none)) {
                session.adjustment_liquify_value(i as u32, v);
            }
        }
        let w = (inner.width() - 16.0) / 3.0;
        let cell =
            |k: f32| Rect::from_min_size(pos2(inner.left() + k * (w + 8.0), inner.bottom() - 40.0), vec2(w, 40.0));
        let can_adjust = shown.liquified;
        let name = KINDS.iter().find(|k| k.0 == shown.liquify_kind).map_or("", |k| k.1);
        let way = cell(0.0);
        ui.painter().rect_filled(way, 20, argb(PANEL_BUTTON));
        text(ui.painter(), way.center(), Align2::CENTER_CENTER, name, 15.0, Weight::Regular, argb(MENU_TEXT));
        if ui.interact(way, ui.scope_id().with("liquify way"), Sense::click()).clicked() {
            self.popup = if self.popup == Popup::Kinds { Popup::None } else { Popup::Kinds };
        }
        let fill = if can_adjust { argb(PANEL_BUTTON) } else { argb(PANEL_BUTTON).gamma_multiply(0.5) };
        let tint = if can_adjust { argb(MENU_TEXT) } else { argb(DIM) };
        for (k, glyph, size, gap, label) in
            [(1.0, &icons::ADJUST_KNOB, 39.5, 2.0, "Adjust"), (2.0, &icons::PIE_RESET, 23.0, 2.5, "Reset")]
        {
            let b = cell(k);
            ui.painter().rect_filled(b, 20, fill);
            let words = ui.painter().layout_no_wrap(label.into(), Weight::Regular.font(15.0), tint).size().x;
            let left = b.center().x - (size + gap + words) / 2.0;
            look.icon(ui, glyph, pos2(left + size / 2.0, b.center().y), size, tint);
            text(
                ui.painter(),
                pos2(left + size + gap, b.center().y),
                Align2::LEFT_CENTER,
                label,
                15.0,
                Weight::Regular,
                tint,
            );
            if can_adjust && ui.interact(b, ui.scope_id().with(("liquify", label)), Sense::click()).clicked() {
                if k == 1.0 {
                    self.popup = if self.popup == Popup::Adjust { Popup::None } else { Popup::Adjust };
                } else {
                    session.adjustment_liquify_reset();
                    self.popup = Popup::None;
                }
            }
        }
    }

    /// The ways, listed over the way's button (upright).
    fn kinds_popover(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        shown: &AdjustShown,
        tip_x: f32,
        tip_y: f32,
    ) {
        let height = 40.0 + 10.0 + 8.0 * 44.0 + 7.0 + 20.0;
        let rect = popover_rect(ctx, tip_x, tip_y, height);
        egui::Area::new(Id::unique("liquify kinds")).fixed_pos(rect.min).order(Order::Tooltip).show(ctx, |ui| {
            popover_frame(ui, rect, tip_x, tip_y);
            let inner = rect.shrink(10.0);
            menu_title(ui, inner, "Liquify Mode");
            let group =
                Rect::from_min_size(pos2(inner.left(), inner.top() + 50.0), vec2(inner.width(), 8.0 * 44.0 + 7.0));
            ui.painter().rect_filled(group, 13, argb(MENU_GROUP));
            for (i, &(kind, label, _)) in KINDS.iter().enumerate() {
                let row =
                    Rect::from_min_size(pos2(group.left(), group.top() + i as f32 * 45.0), vec2(group.width(), 44.0));
                if i > 0 {
                    let line = Rect::from_min_size(pos2(row.left(), row.top() - 1.0), vec2(row.width(), 1.0));
                    ui.painter().rect_filled(line, 0, argb(MENU_LINE));
                }
                let chosen = shown.liquify_kind == kind;
                if chosen {
                    let (top, bottom) = (if i == 0 { 13 } else { 0 }, if i == 7 { 13 } else { 0 });
                    ui.painter().rect_filled(
                        row,
                        egui::CornerRadius { nw: top, ne: top, sw: bottom, se: bottom },
                        argb(pc::ACCENT),
                    );
                }
                let ink = if chosen { Color32::WHITE } else { argb(MENU_TEXT) };
                text(
                    ui.painter(),
                    pos2(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    ink,
                );
                if chosen {
                    look.icon(
                        ui,
                        &icons::MENU_TICK,
                        pos2(row.right() - 14.0 - 14.5, row.center().y),
                        29.0,
                        Color32::WHITE,
                    );
                }
                if ui.interact(row, ui.scope_id().with(("liquify kind", i)), Sense::click()).clicked() {
                    session.adjustment_liquify_kind(kind);
                    self.popup = Popup::None;
                }
            }
        });
    }

    /// Adjust: how much of what liquify did shows.
    fn adjust_popover(
        &mut self,
        ctx: &egui::Context,
        session: &CanvasSession,
        shown: &AdjustShown,
        tip_x: f32,
        tip_y: f32,
    ) {
        let height = 40.0 + 4.0 + 8.0 + 59.0 + 8.0 + 20.0;
        let rect = popover_rect(ctx, tip_x, tip_y, height);
        egui::Area::new(Id::unique("liquify adjust")).fixed_pos(rect.min).order(Order::Tooltip).show(ctx, |ui| {
            popover_frame(ui, rect, tip_x, tip_y);
            let inner = rect.shrink(10.0);
            menu_title(ui, inner, "Adjust");
            let group = Rect::from_min_size(pos2(inner.left(), inner.top() + 44.0), vec2(inner.width(), 59.0 + 16.0));
            ui.painter().rect_filled(group, 13, argb(MENU_GROUP));
            let row = Rect::from_min_size(group.min + vec2(0.0, 8.0), vec2(group.width(), 59.0));
            let amount = shown.liquify_amount;
            let label = format!("{}%", (amount * 100.0).round() as i32);
            if let Some(v) = menu_slider(ui, row, "Amount", amount, &label) {
                session.adjustment_liquify_amount(v);
            }
        });
    }
}

/// A popover `height` tall over a button, its arrow's tip at (`tip_x`, `tip_y`), kept on the window.
fn popover_rect(ctx: &egui::Context, tip_x: f32, tip_y: f32, height: f32) -> Rect {
    let screen = ctx.content_rect();
    let left = (tip_x - MENU_WIDTH / 2.0).clamp(screen.left(), screen.right() - MENU_WIDTH);
    Rect::from_min_size(pos2(left, tip_y - MENU_ARROW - height), vec2(MENU_WIDTH, height))
}

/// One of wide Liquify's buttons at `at` (its top left; 75 × 60): its icon (`size` across, centred
/// 9 down) over its name. Returns whether it was tapped.
#[allow(clippy::too_many_arguments)]
fn button(
    ui: &Ui,
    look: &mut Look,
    at: egui::Pos2,
    glyph: &Icon,
    label: &str,
    ink: impl Shade,
    words: impl Shade,
    size: f32,
    enabled: bool,
) -> bool {
    let cell = Rect::from_min_size(at, vec2(75.0, 60.0));
    look.icon(ui, glyph, pos2(cell.center().x, cell.top() + 9.0), size, argb(ink));
    text(
        ui.painter(),
        pos2(cell.center().x, cell.top() + 22.0),
        Align2::CENTER_TOP,
        label,
        13.0,
        Weight::Regular,
        argb(words),
    );
    enabled && ui.interact(cell, ui.scope_id().with(("liquify button", label)), Sense::click()).clicked()
}
