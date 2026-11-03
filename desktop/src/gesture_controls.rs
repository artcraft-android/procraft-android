//! Gesture controls, as the Android app's (ui/GestureControlsUi.kt): the whole window, what
//! gestures can do down the side (a card of names where the window is wide, a rail of icons where
//! it is narrow) and, for the one chosen, a card of switches for the ways of touching that do it,
//! a delay under the held ones; Reset to defaults under General; Done.

use egui::{Align2, Color32, Id, Order, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use procraft_core::{GestureOption, GesturePurpose, GestureSettings, gesture_options, gesture_purposes};

use crate::alert::{Alert, Answer};
use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::transform::switch;

const SCREEN_FILL: Themed = Themed(0xFF1F1F1F, 0xFFF3F3F3);
const RAIL_FILL: Themed = Themed(0xFF272727, 0xFFEBEBEB);
const CARD_FILL: Themed = Themed(0xFF252525, 0xFFFFFFFF);
const INK: Themed = Themed(0xFFC1C1C1, 0xFF414141);
const DETAIL: Themed = Themed(0xFF9A9A9A, 0xFF6A6A6A);
const RAIL_IDLE: Themed = Themed(0xFFAAAAAA, 0xFF3F3F3F);
const TRACK: Themed = Themed(0xFF414141, 0xFFE5E5E5);
/// A delay whose way of touching is off: a dark line and a dim knob, no blue.
const TRACK_OFF: Themed = Themed(0xFF1D1D1D, 0xFFE9E9EB);
const KNOB_OFF: Themed = Themed(0xFF333333, 0xFFD0D0D0);
const DONE_BLUE: u32 = 0xFF0190FF;
const CARD_EDGE: Themed = Themed(0xFF373737, 0xFFDADADA);
const PAGE_INK: Themed = Themed(0xFF797979, 0xFF3E3E3E);
/// Where a wide window's page starts and ends, beside the card of what gestures can do.
const WIDE_START: f32 = 358.0;
const WIDE_END: f32 = 28.0;
/// An option's row (with the line under it, 72 apart).
const OPTION_HEIGHT: f32 = 71.0;
/// A held option's row, closer up, its delay's row under it.
const HELD_HEIGHT: f32 = 60.0;
const DELAY_HEIGHT: f32 = 50.0;
/// The delays' range, seconds (as the core keeps it).
const MIN_DELAY: f32 = 0.1;
const MAX_DELAY: f32 = 1.5;

fn icon(p: GesturePurpose) -> &'static Icon {
    match p {
        GesturePurpose::Smudge => &icons::SMUDGE,
        GesturePurpose::Erase => &icons::ERASE,
        GesturePurpose::Assisted => &icons::GESTURE_ASSISTED,
        GesturePurpose::Eyedropper => &icons::GESTURE_EYEDROPPER,
        GesturePurpose::Quickshape => &icons::GESTURE_QUICK_SHAPE,
        GesturePurpose::Quickmenu => &icons::GESTURE_QUICK_MENU,
        GesturePurpose::FullScreen => &icons::GESTURE_FULL_SCREEN,
        GesturePurpose::ClearLayer => &icons::GESTURE_CLEAR_LAYER,
        GesturePurpose::CopyPaste => &icons::GESTURE_COPY_PASTE,
        GesturePurpose::LayerSelect => &icons::GESTURE_LAYER_SELECT,
        GesturePurpose::Hover => &icons::GESTURE_HOVER,
        GesturePurpose::General => &icons::GESTURE_GENERAL,
    }
}

pub struct GestureControls {
    pub open: bool,
    /// The page shown (it opens on the one it was last on).
    page: GesturePurpose,
    reset: Alert,
}

impl Default for GestureControls {
    fn default() -> GestureControls {
        GestureControls { open: false, page: GesturePurpose::Smudge, reset: Alert::default() }
    }
}

impl GestureControls {
    /// Draws the page while open. Returns whether a setting changed.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, settings: &GestureSettings) -> bool {
        if !self.open {
            return false;
        }
        let mut changed = false;
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        egui::Area::new(Id::unique("gesture controls")).fixed_pos(screen.min).order(Order::Foreground).show(
            ctx,
            |ui| {
                // the whole window, nothing under it touched
                ui.allocate_rect(screen, Sense::click());
                ui.painter().rect_filled(screen, 0, argb(SCREEN_FILL));
                if wide {
                    self.pages_card(ui, look, screen);
                } else {
                    self.rail(ui, look, screen);
                }
                let heading = procraft_core::gesture_purpose_heading(self.page);
                let ink = argb(INK);
                if wide {
                    let x = (screen.left() + WIDE_START + screen.right() - WIDE_END) / 2.0;
                    text(
                        ui.painter(),
                        pos2(x, screen.top() + 28.0),
                        Align2::CENTER_TOP,
                        &heading,
                        20.0,
                        Weight::Bold,
                        ink,
                    );
                } else {
                    let at = pos2(screen.left() + 101.0, screen.top() + 28.0);
                    text(ui.painter(), at, Align2::LEFT_TOP, &heading, 20.0, Weight::Bold, ink);
                }
                let done =
                    Rect::from_min_size(pos2(screen.right() - 20.0 - 44.0, screen.top() + 20.0), vec2(44.0, 45.0));
                ui.painter().rect_filled(done, 22, argb(DONE_BLUE));
                look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
                if ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked() {
                    self.open = false;
                }
                let (start, end) = if wide { (WIDE_START, WIDE_END) } else { (105.0, 24.0) };
                let content = Rect::from_min_max(
                    pos2(screen.left() + start, screen.top() + 100.0),
                    screen.right_bottom() - vec2(end, 0.0),
                );
                changed |= self.options(ui, settings, content);
            },
        );
        // Escape is Done (with its alert up, it answers that instead)
        if !self.reset.is_open() && crate::ui::escape(ctx) {
            self.open = false;
        }
        if self.reset.show(ctx, look) == Some(Answer::Action) {
            settings.reset_all();
            changed = true;
        }
        changed
    }

    /// What gestures can do, by icon and name on a card headed "Gesture controls".
    fn pages_card(&mut self, ui: &mut Ui, look: &mut Look, screen: Rect) {
        let card =
            Rect::from_min_max(screen.min + vec2(10.0, 10.0), pos2(screen.left() + 330.0, screen.bottom() - 10.0));
        ui.painter().rect_filled(card, 20, argb(RAIL_FILL));
        ui.painter().rect_stroke(card, 20, Stroke::new(1.0, argb(CARD_EDGE)), egui::StrokeKind::Inside);
        let title = pos2(card.left() + 24.0, card.top() + 19.0);
        text(ui.painter(), title, Align2::LEFT_TOP, "Gesture controls", 24.0, Weight::Bold, argb(INK));
        let mut y = card.top() + 65.0;
        for p in gesture_purposes() {
            let row = Rect::from_min_size(pos2(card.left() + 8.0, y), vec2(card.width() - 16.0, 53.0));
            let chosen = p == self.page;
            if chosen {
                ui.painter().rect_filled(row, 26.5, argb(pc::ACCENT));
            }
            let tint = if chosen { Color32::WHITE } else { argb(RAIL_IDLE) };
            look.icon(ui, icon(p), pos2(row.left() + 27.0, row.center().y), 37.0, tint);
            let name = procraft_core::gesture_purpose_title(p);
            let ink = if chosen { Color32::WHITE } else { argb(PAGE_INK) };
            text(
                ui.painter(),
                pos2(row.left() + 54.0, row.center().y),
                Align2::LEFT_CENTER,
                &name,
                17.0,
                Weight::Regular,
                ink,
            );
            if ui.interact(row, ui.scope_id().with(("gesture page", p)), Sense::click()).clicked() {
                self.page = p;
            }
            y += 56.0;
        }
    }

    /// What gestures can do, by icon down a rail (a narrow window's).
    fn rail(&mut self, ui: &mut Ui, look: &mut Look, screen: Rect) {
        let rail = Rect::from_min_size(screen.min, vec2(81.0, screen.height()));
        ui.painter().rect_filled(rail, 0, argb(RAIL_FILL));
        let mut y = rail.top() + 46.0;
        for p in gesture_purposes() {
            let button = Rect::from_min_size(pos2(rail.left() + 8.0, y), vec2(65.0, 53.0));
            let chosen = p == self.page;
            if chosen {
                ui.painter().rect_filled(button, 26.5, argb(pc::ACCENT));
            }
            let tint = if chosen { Color32::WHITE } else { argb(RAIL_IDLE) };
            look.icon(ui, icon(p), button.center(), 37.0, tint);
            if ui.interact(button, ui.scope_id().with(("gesture page", p)), Sense::click()).clicked() {
                self.page = p;
            }
            y += 56.0;
        }
    }

    /// The page's card of switches (and delays), and Reset to defaults under General's.
    fn options(&mut self, ui: &mut Ui, settings: &GestureSettings, content: Rect) -> bool {
        let mut changed = false;
        let page = self.page;
        let options = gesture_options(page);
        let card_height: f32 =
            options.iter().map(|o| if o.delayed { HELD_HEIGHT + DELAY_HEIGHT } else { OPTION_HEIGHT }).sum::<f32>()
                + options.len().saturating_sub(1) as f32;
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
            ScrollArea::vertical()
                .id_salt(("gesture page", page))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    let (card, _) = ui.allocate_exact_size(vec2(content.width(), card_height), Sense::hover());
                    ui.painter().rect_filled(card, 25, argb(CARD_FILL));
                    let mut y = card.top();
                    for (i, o) in options.iter().enumerate() {
                        if i > 0 {
                            let line = Rect::from_min_size(pos2(card.left(), y), vec2(card.width(), 1.0));
                            ui.painter().rect_filled(line, 0, argb(SCREEN_FILL));
                            y += 1.0;
                        }
                        let height = if o.delayed { HELD_HEIGHT } else { OPTION_HEIGHT };
                        let row = Rect::from_min_size(pos2(card.left(), y), vec2(card.width(), height));
                        let on = settings.is_on(page, o.key.clone());
                        if option_row(ui, o, on, row) {
                            settings.set(page, o.key.clone(), !on);
                            changed = true;
                        }
                        y += height;
                        if o.delayed {
                            let row = Rect::from_min_size(pos2(card.left(), y), vec2(card.width(), DELAY_HEIGHT));
                            if let Some(s) = delay_row(ui, settings.delay(page), on, row, &o.key) {
                                settings.set_delay(page, s);
                                changed = true;
                            }
                            y += DELAY_HEIGHT;
                        }
                    }
                    // under General's settings, everything back as it ships
                    if page == GesturePurpose::General {
                        ui.add_space(14.0);
                        let (reset, response) = ui.allocate_exact_size(vec2(content.width(), 50.0), Sense::click());
                        ui.painter().rect_filled(reset, 25, argb(CARD_FILL));
                        let ink = argb(INK);
                        text(
                            ui.painter(),
                            reset.center(),
                            Align2::CENTER_CENTER,
                            "Reset to defaults",
                            15.0,
                            Weight::Regular,
                            ink,
                        );
                        if response.clicked() {
                            self.reset.open(
                                "Reset gesture controls?",
                                "Every gesture goes back to how the app starts out.",
                                "Reset",
                                None,
                            );
                        }
                    }
                    ui.add_space(40.0);
                });
        });
        changed
    }
}

/// A way of touching: its name and what it does, and its switch. Returns whether it was tapped.
fn option_row(ui: &Ui, o: &GestureOption, on: bool, row: Rect) -> bool {
    let ink = argb(INK);
    let x = row.left() + 28.0;
    text(ui.painter(), pos2(x, row.top() + 28.0), Align2::LEFT_CENTER, &o.label, 18.0, Weight::Medium, ink);
    text(ui.painter(), pos2(x, row.top() + 47.0), Align2::LEFT_CENTER, &o.detail, 13.0, Weight::Regular, argb(DETAIL));
    switch(ui, pos2(row.right() - 16.0 - 63.0, row.center().y - 14.0), on, &o.label);
    ui.interact(row, ui.scope_id().with(("gesture switch", &o.key)), Sense::click()).clicked()
}

/// How long a hold waits, 0.1 to 1.5 seconds, dragged or tapped along the bar; greyed while its way
/// of touching is off. Returns the delay as set.
fn delay_row(ui: &Ui, seconds: f32, enabled: bool, row: Rect, key: &str) -> Option<f32> {
    let ink = argb(INK);
    let left = row.left() + 28.0;
    text(ui.painter(), pos2(left, row.center().y), Align2::LEFT_CENTER, "Delay", 15.0, Weight::Regular, ink);
    let shown = format!("{seconds:.2}s");
    let value = text(
        ui.painter(),
        pos2(row.right() - 24.0, row.center().y),
        Align2::RIGHT_CENTER,
        &shown,
        15.0,
        Weight::Regular,
        ink,
    );
    let bar = Rect::from_min_max(pos2(left + 74.0, row.top()), pos2(value.left() - 16.0, row.bottom()));
    let span = MAX_DELAY - MIN_DELAY;
    let fraction = ((seconds - MIN_DELAY) / span).clamp(0.0, 1.0);
    let (y, x) = (bar.center().y, bar.left() + fraction * bar.width());
    let knob = Rect::from_center_size(pos2(x, y), vec2(16.0, 10.0));
    if enabled {
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(bar.left(), y - 1.5), pos2(bar.right(), y + 1.5)),
            0,
            argb(TRACK),
        );
        ui.painter().rect_filled(Rect::from_min_max(pos2(bar.left(), y - 1.5), pos2(x, y + 1.5)), 0, argb(pc::ACCENT));
        ui.painter().rect_filled(knob, 5, Color32::WHITE);
    } else {
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(bar.left(), y - 0.75), pos2(bar.right(), y + 0.75)),
            0,
            argb(TRACK_OFF),
        );
        ui.painter().rect_filled(knob, 5, argb(KNOB_OFF));
        return None;
    }
    let response = ui.interact(bar, ui.scope_id().with(("gesture delay", key)), Sense::click_and_drag());
    let p = response.interact_pointer_pos()?;
    Some(MIN_DELAY + ((p.x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0) * span)
}
