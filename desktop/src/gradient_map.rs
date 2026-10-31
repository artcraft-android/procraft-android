//! Gradient Map's panel, as the Android app's (ui/AdjustmentsUi.kt): the gradient library (a tap
//! chooses one, a tap on the chosen one edits it, a hold offers Delete and Duplicate, "+" makes a
//! new one), or the gradient being edited: its name, over its bar with a disc for each stop (a tap
//! on the bar adds a stop, a stop drags along it, a tap on one opens the compact colour picker
//! over it, a hold offers Delete). The library is the core's (gradients.rs), kept between runs.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Order, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use procraft_core::{AdjustShown, CanvasSession, GradientInfo, Gradients};

use crate::compact_colours::{CompactColours, Picked};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, text};
use crate::menu::{self, Entry};

const PANEL_FILL: Themed = Themed(0xF71E1E1E, 0xF7F1F1F1);
const PANEL_RISE: f32 = 25.0;
const MENU_TEXT: Themed = Themed(0xFFC5C5C5, 0xFF393939);
const BAND: Themed = Themed(0xFF535353, 0xFFFFFFFF);
const DONE: u32 = 0xFF0190FF;
const PLUS_DISC: Themed = Themed(0xFF1B1B1B, 0xFFFBFBFB);
const PLUS_RIM: Themed = Themed(0xFF373737, 0xFFE1E1E1);
const PLUS_GLYPH: Themed = Themed(0xFFAAAAAA, 0xFF595959);
const EDITOR_CARD: Themed = Themed(0xFF383838, 0xFFEEEEEE);
const EDITOR_NAME: Themed = Themed(0xFFC5C5C5, 0xFF3A3A3A);
const CARD: Themed = Themed(0xFF282828, 0xFFEEEEEE);
const CARD_NAME: Themed = Themed(0xFFC1C1C1, 0xFF5D5D5D);
const STOP: f32 = 42.0;
const HOLD: Duration = Duration::from_millis(400);

/// What a press is on: a card of the library, or a stop of the gradient edited.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pressed {
    Card(u32),
    Stop(u32),
}

#[derive(Default)]
pub struct GradientMap {
    /// The gradient being edited, and its name as typed.
    editing: Option<u32>,
    name: String,
    /// What is pressed, since when, and whether the hold has opened its menu.
    press: Option<(Pressed, Instant, bool)>,
    /// A stop being dragged.
    dragging: Option<u32>,
    /// The hold menu, and what it is for (and where that is).
    menu: menu::Menu,
    menu_for: Option<(Pressed, Rect)>,
    /// A stop's colour being picked: the gradient's, the stop's, and the top of the stop, which
    /// the picker's arrow points at.
    colouring: Option<(u32, u32, CompactColours, Pos2)>,
}

/// A gradient's colour at `t` (0..1).
fn colour_at(g: &GradientInfo, t: f32) -> Color32 {
    let mut stops = g.stops.clone();
    stops.sort_by(|a, b| a.position.total_cmp(&b.position));
    let rgb = |c: u32| Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
    let Some(first) = stops.first() else { return Color32::BLACK };
    if t <= first.position {
        return rgb(first.rgb);
    }
    for pair in stops.windows(2) {
        let &[a, b] = pair else { continue };
        if t <= b.position {
            let f = if b.position > a.position { (t - a.position) / (b.position - a.position) } else { 1.0 };
            let (ca, cb) = (rgb(a.rgb), rgb(b.rgb));
            let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * f) as u8;
            return Color32::from_rgb(ch(ca.r(), cb.r()), ch(ca.g(), cb.g()), ch(ca.b(), cb.b()));
        }
    }
    stops.last().map_or(Color32::BLACK, |s| rgb(s.rgb))
}

/// The gradient across `rect`, from `from` to `to` of its width, its bottom corners rounded by
/// `corner`.
fn strip(ui: &Ui, rect: Rect, g: &GradientInfo, from: f32, to: f32, corner: f32) {
    let columns = rect.width().ceil() as usize;
    for k in 0..columns {
        let x = rect.left() + k as f32 + 0.5;
        let t = (((x - rect.left()) / rect.width() - from) / (to - from).max(1e-6)).clamp(0.0, 1.0);
        // inside the rounded corners only
        let edge = (x - rect.left()).min(rect.right() - x);
        let cut =
            if edge < corner { corner - (corner * corner - (corner - edge).powi(2)).max(0.0).sqrt() } else { 0.0 };
        let bottom = rect.bottom() - cut;
        if bottom > rect.top() {
            ui.painter().line_segment([pos2(x, rect.top()), pos2(x, bottom)], Stroke::new(1.0, colour_at(g, t)));
        }
    }
}

impl GradientMap {
    /// The panel, while Gradient Map is open.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        shown: &AdjustShown,
        gradients: &Gradients,
        colours: &crate::colours::Colours,
        backdrops: &mut Vec<f32>,
    ) {
        let screen = ctx.content_rect();
        let width = if screen.width() > screen.height() { 835.0 } else { 544.0 };
        let rect = Rect::from_min_size(
            pos2(screen.center().x - width / 2.0, screen.bottom() - PANEL_RISE - 159.0),
            vec2(width, 159.0),
        );
        let list = gradients.list();
        if self.editing.is_some_and(|i| i as usize >= list.len()) {
            self.editing = None;
        }
        egui::Area::new(Id::unique("gradient map")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            ui.painter().rect_filled(rect, 33, argb(PANEL_FILL));
            let inner = Rect::from_min_max(rect.min + vec2(14.0, 12.0), rect.max - vec2(14.0, 12.0));
            let header = Rect::from_min_size(inner.min, vec2(inner.width(), 40.0));
            let title = if self.editing.is_some() { "Gradient Map" } else { "Gradient Library" };
            let at = pos2(header.left() + 14.0, header.center().y);
            text(ui.painter(), at, Align2::LEFT_CENTER, title, 20.0, Weight::Bold, argb(MENU_TEXT));
            let button = Rect::from_min_size(pos2(header.right() - 40.0, header.top()), vec2(40.0, 40.0));
            if self.editing.is_some() {
                ui.painter().rect_filled(button, 20, argb(DONE));
                look.icon(ui, &icons::DONE_TICK, button.center(), 36.0, Color32::WHITE);
                if ui.interact(button, ui.scope_id().with("gradient done"), Sense::click()).clicked() {
                    self.editing = None;
                }
            } else {
                ui.painter().rect_filled(button, 20, argb(PLUS_DISC));
                ui.painter().rect_stroke(button, 20, Stroke::new(1.0, argb(PLUS_RIM)), egui::StrokeKind::Inside);
                look.icon(ui, &icons::ADD, button.center(), 34.0, argb(PLUS_GLYPH));
                if ui.interact(button, ui.scope_id().with("new gradient"), Sense::click()).clicked() {
                    // a new gradient, black to white, just after the chosen one: chosen and opened
                    let at = gradients.add_after(shown.gradient);
                    session.adjustment_gradient(at);
                    self.edit(at, gradients);
                }
            }
            let body = Rect::from_min_size(pos2(inner.left(), header.bottom() + 11.0), vec2(inner.width(), 81.0));
            match self.editing.and_then(|i| list.get(i as usize).map(|g| (i, g))) {
                Some((i, g)) => self.editor(ui, session, gradients, body, i, g),
                None => self.cards(ui, session, gradients, body, &list, shown.gradient),
            }
        });
        self.hold_menu(ctx, look, session, gradients, shown, backdrops);
        if self.editing.is_none() {
            self.colouring = None;
        }
        // a stop's colour, over it and kept inside the window
        if let Some((index, stop, pick, tip)) = &mut self.colouring {
            let size = pick.size();
            let left = (tip.x - size.x / 2.0).clamp(screen.left(), (screen.right() - size.x).max(screen.left()));
            let rect = Rect::from_min_size(pos2(left, tip.y - size.y), size);
            let p3 = procraft_core::colours_in_p3(session.profile());
            match pick.show(ctx, look, rect, tip.x, &colours.palettes, colours.harmony, p3) {
                Some(Picked::Colour(rgb)) => {
                    gradients.set_stop_colour(*index, *stop, rgb);
                    session.adjustment_gradient(*index);
                }
                Some(Picked::Closed) => self.colouring = None,
                None => {}
            }
        }
    }

    /// The adjustment has closed: a stop's colour being picked closes with it.
    pub fn shut(&mut self) {
        self.colouring = None;
    }

    fn edit(&mut self, index: u32, gradients: &Gradients) {
        self.editing = Some(index);
        self.name = gradients.list().get(index as usize).map(|g| g.name.clone()).unwrap_or_default();
    }

    /// Tracks a press on `what` (its `response`): returns true once it is held long enough.
    fn held(&mut self, ui: &Ui, what: Pressed, response: &egui::Response) -> bool {
        if response.is_pointer_button_down_on() {
            let (p, since, done) = self.press.get_or_insert((what, Instant::now(), false));
            if *p == what && !*done {
                if since.elapsed() >= HOLD {
                    *done = true;
                    return true;
                }
                ui.ctx().request_repaint_after(HOLD.saturating_sub(since.elapsed()));
            }
        } else if self.press.is_some_and(|(p, _, _)| p == what) {
            self.press = None;
        }
        false
    }

    fn was_held(&self, what: Pressed) -> bool {
        self.press.is_some_and(|(p, _, done)| p == what && done)
    }

    /// The library's cards on a grey band: each dark, its name over a strip of it; the chosen one
    /// stands taller.
    fn cards(
        &mut self,
        ui: &mut Ui,
        session: &CanvasSession,
        gradients: &Gradients,
        band: Rect,
        list: &[GradientInfo],
        chosen: u32,
    ) {
        ui.painter().rect_filled(band, 12, argb(BAND));
        ui.scope_builder(UiBuilder::new().max_rect(band.shrink2(vec2(6.0, 0.0))), |ui| {
            ScrollArea::horizontal()
                .id_salt("gradient cards")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                    ui.horizontal(|ui| {
                        for (i, g) in list.iter().enumerate() {
                            let i = i as u32;
                            let on = i == chosen;
                            let (slot, response) = ui.allocate_exact_size(vec2(108.0, 81.0), Sense::click());
                            let height = if on { 69.0 } else { 65.0 };
                            let card = Rect::from_min_max(
                                pos2(slot.left(), slot.bottom() - 5.5 - height),
                                pos2(slot.right(), slot.bottom() - 5.5),
                            );
                            ui.painter().rect_filled(card, 9, argb(CARD));
                            let strip_rect = Rect::from_min_max(pos2(card.left(), card.bottom() - 13.0), card.max);
                            strip(ui, strip_rect, g, 0.0, 1.0, 9.0);
                            let ink = if on { Color32::WHITE } else { argb(CARD_NAME) };
                            let at = pos2(card.left() + 11.0, strip_rect.top() - 10.0);
                            text(ui.painter(), at, Align2::LEFT_BOTTOM, &g.name, 12.0, Weight::Regular, ink);
                            if self.held(ui, Pressed::Card(i), &response) {
                                self.menu_for = Some((Pressed::Card(i), card));
                                self.menu.open();
                            }
                            if response.clicked() && !self.was_held(Pressed::Card(i)) {
                                if on {
                                    self.edit(i, gradients);
                                } else {
                                    session.adjustment_gradient(i);
                                }
                            }
                        }
                    });
                });
        });
    }

    /// The gradient being edited: its name to rename, over its bar with a disc for each stop.
    fn editor(
        &mut self,
        ui: &mut Ui,
        session: &CanvasSession,
        gradients: &Gradients,
        body: Rect,
        index: u32,
        g: &GradientInfo,
    ) {
        ui.painter().rect_filled(body, 12, argb(EDITOR_CARD));
        let name_rect = Rect::from_min_size(body.min + vec2(14.0, 8.0), vec2(body.width() - 28.0, 28.0));
        let mut name = self.name.clone();
        let edit = egui::TextEdit::singleline(&mut name)
            .frame(egui::Frame::NONE)
            .text_color(argb(EDITOR_NAME))
            .font(Weight::Regular.font(14.0))
            .char_limit(64);
        if ui.put(name_rect, edit).changed() {
            self.name = name;
            gradients.rename(index, self.name.clone());
            session.adjustment_gradient(index);
        }
        let bar = Rect::from_min_max(pos2(body.left(), body.bottom() - 44.0), body.max);
        let r = STOP / 2.0;
        let x_of = |p: f32| bar.left() + r + p * (bar.width() - 2.0 * r);
        let position_at = |x: f32| ((x - bar.left() - r) / (bar.width() - 2.0 * r).max(1.0)).clamp(0.0, 1.0);
        let stop_at = |x: f32| {
            g.stops
                .iter()
                .enumerate()
                .map(|(i, s)| (i, (x_of(s.position) - x).abs()))
                .filter(|(_, d)| *d <= r)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i as u32)
        };
        strip(ui, bar, g, r / bar.width(), 1.0 - r / bar.width(), 12.0);
        for s in &g.stops {
            let centre = pos2(x_of(s.position), bar.center().y);
            let fill = Color32::from_rgb((s.rgb >> 16) as u8, (s.rgb >> 8) as u8, s.rgb as u8);
            // the ring stands out from what it rings: white round a dark colour, black round a light one
            let light =
                0.299 * f32::from(fill.r()) + 0.587 * f32::from(fill.g()) + 0.114 * f32::from(fill.b()) > 0.45 * 255.0;
            ui.painter().circle_filled(centre, r, if light { Color32::BLACK } else { Color32::WHITE });
            ui.painter().circle_filled(centre, r - 2.0, fill);
        }
        let response = ui.interact(bar, ui.scope_id().with(("gradient bar", index)), Sense::click_and_drag());
        let pointer = response.interact_pointer_pos();
        // a stop drags along the bar
        if response.drag_started() {
            self.dragging = pointer.and_then(|p| stop_at(p.x));
        }
        if let (Some(i), Some(p)) = (self.dragging, pointer)
            && response.dragged()
        {
            gradients.move_stop(index, i, position_at(p.x));
            session.adjustment_gradient(index);
        }
        if response.drag_stopped() {
            self.dragging = None;
        }
        // held, a stop offers Delete
        if let Some(i) = pointer.and_then(|p| stop_at(p.x))
            && self.dragging.is_none()
            && self.held(ui, Pressed::Stop(i), &response)
        {
            let at = pos2(x_of(g.stops.get(i as usize).map_or(0.0, |s| s.position)), bar.center().y);
            self.menu_for = Some((Pressed::Stop(i), Rect::from_center_size(at, vec2(STOP, STOP))));
            self.menu.open();
        }
        // a hold's letting go is not a tap
        let after_hold = self.press.is_some_and(|(_, _, done)| done);
        if !response.is_pointer_button_down_on() {
            self.press = None;
        }
        // a tap on a stop opens its colour over it; on the bar, off the stops, adds one
        if response.clicked()
            && !after_hold
            && let Some(p) = pointer
        {
            match stop_at(p.x) {
                Some(i) => {
                    let s = g.stops.get(i as usize);
                    let rgb = s.map_or(0, |s| s.rgb & 0xFF_FFFF);
                    let tip = pos2(x_of(s.map_or(0.0, |s| s.position)), bar.center().y - r);
                    self.colouring = Some((index, i, CompactColours::new(rgb, 0.5), tip));
                }
                None => {
                    gradients.add_stop(index, position_at(p.x));
                    session.adjustment_gradient(index);
                }
            }
        }
    }

    /// What a hold opened, over what was held: a card's Delete and Duplicate, a stop's Delete.
    fn hold_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        gradients: &Gradients,
        shown: &AdjustShown,
        backdrops: &mut Vec<f32>,
    ) {
        let Some((what, over)) = self.menu_for else { return };
        let entries: Vec<Entry> = match what {
            Pressed::Card(_) => vec![
                Entry::Item(&icons::MENU_TRASH, "Delete", menu::DANGER),
                Entry::Item(&icons::MENU_DUPLICATE, "Duplicate", menu::INK),
            ],
            Pressed::Stop(_) => vec![Entry::Item(&icons::MENU_TRASH, "Delete", menu::DANGER)],
        };
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(over.center().x - size.x / 2.0, over.top() - size.y), size);
        let mut tapped = None;
        self.menu.show(ctx, "gradient menu", rect, rect.center_bottom(), menu::OVER_A_PANEL, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        if let Some(k) = tapped {
            self.menu.close();
            match (what, k) {
                (Pressed::Card(i), 0) => {
                    gradients.delete(i);
                    let count = gradients.list().len() as u32;
                    let chosen = if shown.gradient > i || shown.gradient >= count {
                        shown.gradient.saturating_sub(1)
                    } else {
                        shown.gradient
                    };
                    session.adjustment_gradient(chosen);
                }
                (Pressed::Card(i), _) => session.adjustment_gradient(gradients.duplicate(i)),
                (Pressed::Stop(s), _) => {
                    if let Some(i) = self.editing {
                        gradients.delete_stop(i, s);
                        session.adjustment_gradient(i);
                    }
                }
            }
        }
    }
}
