//! Adjustments, as the Android app's (ui/AdjustmentsUi.kt): the menu hanging from its button (the
//! adjustments in groups) and, while one is open, the bar under the top bar (its icon, name and
//! amount, the amount's blue line), the menu a tap on the canvas opens round the tap, and the
//! adjustment's own panel rising from the foot of the window. What each does is the core's
//! (adjusting.rs); the touches swipe its amount.

use egui::{Align2, Color32, Id, LayerId, Order, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use procraft_core::{AdjustShown, Adjustment, CLONE_PUCK_RADIUS, CanvasSession, adjustment_groups, adjustment_label};

use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::panel;

const MENU_WIDTH: f32 = 330.0;
const MENU_BOTTOM_MARGIN: f32 = 25.0;
const GROUP_FILL: Themed = Themed(0x420C0C0C, 0xFFE8E8E8);
const GROUP_LINE: Themed = Themed(0x06FFFFFF, 0xFFE2E2E2);
const MENU_TEXT: Themed = Themed(0xFFC5C5C5, 0xFF393939);
const MODE_BAR: Themed = Themed(0xD63B3B3B, 0xD6EEEEEE);
const MODE_TEXT: Themed = Themed(0xFFC5C5C5, 0xFF6A6A6A);
const MODES_FILL: Themed = Themed(0xFF7B7B7B, 0xFFFFFFFF);
const PANEL_FILL: Themed = Themed(0xF71E1E1E, 0xF7F1F1F1);
const HSB_LABEL: Themed = Themed(0xFF808080, 0xFF595959);
const HSB_VALUE: Themed = Themed(0xFFEDEDED, 0xFF363636);
const PIE_BUTTON: Themed = Themed(0xFF3B3B3B, 0xFFFFFFFF);
const PIE_TEXT: Themed = Themed(0xFFD8D8D8, 0xFF414141);
/// The tap menu's buttons apart, and how far the bottom panels stand up from the window's foot.
const PIE_STEP: f32 = 88.0;
const PANEL_RISE: f32 = 25.0;
const HUE_STOPS: [u32; 7] = [0xFFFF0000, 0xFFFFFF00, 0xFF00FF00, 0xFF00FFFF, 0xFF0000FF, 0xFFFF00FF, 0xFFFF0000];

fn icon(a: Adjustment) -> &'static Icon {
    match a {
        Adjustment::Hsb => &icons::ADJUST_HSB,
        Adjustment::ColorBalance => &icons::ADJUST_BALANCE,
        Adjustment::Curves => &icons::ADJUST_CURVES,
        Adjustment::GradientMap => &icons::ADJUST_GRADIENT,
        Adjustment::GaussianBlur => &icons::ADJUST_BLUR,
        Adjustment::MotionBlur => &icons::ADJUST_MOTION,
        Adjustment::PerspectiveBlur => &icons::ADJUST_PERSPECTIVE,
        Adjustment::Noise => &icons::ADJUST_NOISE,
        Adjustment::Sharpen => &icons::ADJUST_SHARPEN,
        Adjustment::Bloom => &icons::ADJUST_BLOOM,
        Adjustment::Glitch => &icons::ADJUST_GLITCH,
        Adjustment::Halftone => &icons::ADJUST_HALFTONE,
        Adjustment::ChromaticAberration => &icons::ADJUST_CHROMATIC,
        Adjustment::Liquify => &icons::ADJUST_LIQUIFY,
        Adjustment::Clone => &icons::ADJUST_CLONE,
    }
}

/// What the tap menu asked of the open adjustment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Apply,
    Undo,
    Preview,
    Cancel,
    Reset,
}

/// The adjustments menu hanging from its `button`, in groups: colour, blurs, effects, painting.
/// Returns where it is (points) and the adjustment tapped.
pub fn menu(
    ctx: &egui::Context,
    look: &mut Look,
    button: Rect,
    backdrops: &mut Vec<f32>,
) -> (Rect, Option<Adjustment>) {
    let screen = ctx.content_rect();
    let (left, arrow_x) = panel::place(ctx, MENU_WIDTH, button.center().x);
    let top = screen.top() + pc::BAR_HEIGHT;
    let groups = adjustment_groups();
    let rows: f32 = groups.iter().map(|g| g.len() as f32 * 44.0 + g.len().saturating_sub(1) as f32).sum::<f32>()
        + 10.0 * groups.len().saturating_sub(1) as f32;
    let content = panel::ARROW_HEIGHT + 52.0 + 8.0 + rows + 10.0;
    let height = content.min(screen.bottom() - MENU_BOTTOM_MARGIN - top);
    let rect = Rect::from_min_size(pos2(left, top), vec2(MENU_WIDTH, height));
    let mut tapped = None;
    egui::Area::new(Id::unique("adjustments menu")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
        ui.allocate_rect(rect, Sense::click());
        panel::frame(ui, look, "adjustments frame", rect, arrow_x, backdrops);
        let body = panel::body(rect);
        // a 52 header, the title's capitals from 28.5 below the panel's top; the groups 8 into the
        // list under it
        let title = pos2(body.left() + 24.0, body.top() + 22.5);
        text(ui.painter(), title, Align2::LEFT_TOP, "Adjustments", 24.0, Weight::Bold, argb(panel::PANEL_TITLE));
        let list = Rect::from_min_max(pos2(body.left() + 10.0, body.top() + 52.0 + 8.0), body.max - vec2(10.0, 10.0));
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical()
                .id_salt("adjustments")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (k, group) in groups.iter().enumerate() {
                        if k > 0 {
                            ui.add_space(10.0);
                        }
                        let height = group.len() as f32 * 44.0 + group.len().saturating_sub(1) as f32;
                        let (card, _) = ui.allocate_exact_size(vec2(list.width(), height), Sense::hover());
                        ui.painter().rect_filled(card, 13, argb(GROUP_FILL));
                        for (i, &a) in group.iter().enumerate() {
                            let row = Rect::from_min_size(
                                pos2(card.left(), card.top() + i as f32 * 45.0),
                                vec2(card.width(), 44.0),
                            );
                            if i > 0 {
                                let line =
                                    Rect::from_min_size(pos2(row.left(), row.top() - 1.0), vec2(row.width(), 1.0));
                                ui.painter().rect_filled(line, 0, argb(GROUP_LINE));
                            }
                            let at = pos2(row.left() + 15.0, row.center().y);
                            text(
                                ui.painter(),
                                at,
                                Align2::LEFT_CENTER,
                                &adjustment_label(a),
                                17.0,
                                Weight::Regular,
                                argb(MENU_TEXT),
                            );
                            if ui.interact(row, ui.scope_id().with(("adjustment", k, i)), Sense::click()).clicked() {
                                tapped = Some(a);
                            }
                        }
                    }
                });
        });
    });
    (rect, tapped)
}

#[derive(Default)]
pub struct Adjustments {
    /// Where the tap menu is open (points), after a tap on the canvas.
    pub menu_at: Option<Pos2>,
    /// The Layer / Pencil choice is open (a touch on the canvas closes it).
    pub modes_open: bool,
    /// The effect adjustments' toolbars.
    toolbars: crate::adjust_panels::Toolbars,
}

impl Adjustments {
    /// While an adjustment is open: its bar under the top bar (at `bar_top`), its panel and the tap
    /// menu. Returns what the tap menu asked.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        bar_top: f32,
        gradients: &procraft_core::Gradients,
        colours: &crate::colours::Colours,
        backdrops: &mut Vec<f32>,
    ) -> Option<Asked> {
        let shown = session.adjustment_shown();
        let a = shown.adjustment?;
        if let Some(&[x, y]) = shown.clone_puck.as_deref() {
            clone_puck(ctx, pos2(x, y) / ctx.pixels_per_point());
        }
        // the bar takes its touches: a tap anywhere on it opens Layer or Pencil under it, a swipe
        // from it adjusts nothing
        let band = bar(ctx, look, &shown, a, bar_top);
        egui::Area::new(Id::unique("adjust modes button")).fixed_pos(band.min).order(Order::Foreground).show(
            ctx,
            |ui| {
                if ui.allocate_rect(band, Sense::click()).clicked() && procraft_core::adjustment_paintable(a) {
                    self.modes_open = !self.modes_open;
                }
            },
        );
        // Escape closes the tap menu, else Layer or Pencil's
        if self.menu_at.is_some() && crate::ui::escape(ctx) {
            self.menu_at = None;
        } else if self.modes_open && crate::ui::escape(ctx) {
            self.modes_open = false;
        }
        if self.modes_open
            && let Some(pencil) = modes(ctx, look, shown.pencil, bar_top + 52.0)
        {
            self.modes_open = false;
            session.adjustment_pencil(pencil);
        }
        if a == Adjustment::Hsb {
            hsb_panel(ctx, session, &shown);
        }
        self.toolbars.show(ctx, look, session, &shown, gradients, colours, backdrops);
        self.pie(ctx, look)
    }

    /// The menu a tap on the canvas opens: apply above, undo to the left, preview in the middle,
    /// cancel to the right and reset below, round the tap, kept on the screen. A tap elsewhere
    /// closes it.
    fn pie(&mut self, ctx: &egui::Context, look: &mut Look) -> Option<Asked> {
        let tapped = self.menu_at?;
        let screen = ctx.content_rect();
        // 12 from the sides, against the top, 25 above the bottom (as measured)
        let reach = PIE_STEP + 40.0;
        let (left, top) = (screen.left() + reach + 12.0, screen.top() + reach);
        let at = pos2(
            tapped.x.clamp(left, (screen.right() - reach - 12.0).max(left)),
            tapped.y.clamp(top, (screen.bottom() - reach - 25.0).max(top)),
        );
        let mut asked = None;
        let mut close = false;
        egui::Area::new(Id::unique("adjustment pie")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            close = ui.allocate_rect(screen, Sense::click()).clicked();
            let items: [(&Icon, &str, (f32, f32), Asked); 5] = [
                (&icons::APPLY, "Apply", (0.0, -1.0), Asked::Apply),
                (&icons::PIE_UNDO, "Undo", (-1.0, 0.0), Asked::Undo),
                (&icons::PREVIEW, "Preview", (0.0, 0.0), Asked::Preview),
                (&icons::CANCEL_ADJUST, "Cancel", (1.0, 0.0), Asked::Cancel),
                (&icons::PIE_RESET, "Reset", (0.0, 1.0), Asked::Reset),
            ];
            for (glyph, label, (dx, dy), what) in items {
                let centre = at + vec2(PIE_STEP * dx, PIE_STEP * dy);
                let button = Rect::from_center_size(centre, vec2(80.0, 80.0));
                ui.painter().rect_filled(button, 22, argb(PIE_BUTTON));
                // the icon (a 50 frame) 30 across, centred 33 down; the name from 45 down
                let ink = argb(PIE_TEXT);
                look.icon(ui, glyph, pos2(centre.x, button.top() + 33.0), 30.0, ink);
                text(
                    ui.painter(),
                    pos2(centre.x, button.top() + 44.0),
                    Align2::CENTER_TOP,
                    label,
                    13.5,
                    Weight::Regular,
                    ink,
                );
                if ui.interact(button, ui.scope_id().with(("pie", label)), Sense::click()).clicked() {
                    asked = Some(what);
                }
            }
        });
        if asked.is_some() || close {
            self.menu_at = None;
        }
        asked
    }
}

/// The clone source: a dark ring with a light edge, so it shows on any colour.
fn clone_puck(ctx: &egui::Context, at: Pos2) {
    let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::unique("clone puck")));
    painter.circle_stroke(at, CLONE_PUCK_RADIUS, Stroke::new(4.0, Color32::WHITE));
    painter.circle_stroke(at, CLONE_PUCK_RADIUS, Stroke::new(2.0, Color32::BLACK));
}

/// The bar under the top bar: the adjustment's icon and name, its amount when it has one, the
/// triangle that opens Layer or Pencil; under it the amount as a thin blue line.
/// Returns where the triangle is, for the adjustments that have one.
fn bar(ctx: &egui::Context, look: &mut Look, shown: &AdjustShown, a: Adjustment, top: f32) -> Rect {
    let screen = ctx.content_rect();
    let band = Rect::from_min_size(pos2(screen.left(), top), vec2(screen.width(), 32.0));
    egui::Area::new(Id::unique("adjustment bar"))
        .fixed_pos(band.min)
        .order(Order::Middle)
        .interactable(false)
        .show(ctx, |ui| bar_in(ui, look, shown, a, band, screen.width()));
    band
}

fn bar_in(ui: &Ui, look: &mut Look, shown: &AdjustShown, a: Adjustment, band: Rect, width_all: f32) {
    let painter = ui.painter();
    painter.rect_filled(band, 0, argb(MODE_BAR));
    let ink = argb(MODE_TEXT);
    let label = adjustment_label(a);
    let swiped = procraft_core::adjustment_swiped(a);
    let paintable = procraft_core::adjustment_paintable(a);
    let amount = format!("{}%", (shown.amount * 100.0).round() as i32);
    let measure = |s: &str, size: f32, w: Weight| painter.layout_no_wrap(s.into(), w.font(size), ink).size().x;
    let width = 33.0
        + measure(&label, 17.0, Weight::Bold)
        + if swiped { 3.0 + measure(&amount, 14.0, Weight::Regular) } else { 0.0 }
        + if paintable { 33.0 } else { 0.0 };
    let mut x = band.center().x - width / 2.0;
    look.icon(ui, icon(a), pos2(x + 16.5, band.center().y), 32.0, ink);
    x += 33.0;
    x = text(painter, pos2(x, band.center().y), Align2::LEFT_CENTER, &label, 17.0, Weight::Bold, ink).right();
    if swiped {
        x = text(painter, pos2(x + 3.0, band.center().y), Align2::LEFT_CENTER, &amount, 14.0, Weight::Regular, ink)
            .right();
    }
    if paintable {
        look.icon(ui, &icons::DISCLOSE, pos2(x + 16.5, band.center().y), 32.0, ink);
    }
    if swiped {
        let line = Rect::from_min_size(band.left_bottom(), vec2(width_all * shown.amount.clamp(0.0, 1.0), 4.0));
        painter.rect_filled(line, 0, argb(pc::ACCENT));
    }
}

/// Layer Opacity's band under the top bar (at `top`): its glyph, "Opacity" and the layer's
/// `opacity`, under it the opacity as a thin blue line across the window.
pub(crate) fn layer_opacity_bar(ctx: &egui::Context, opacity: f32, top: f32) {
    let screen = ctx.content_rect();
    let band = Rect::from_min_size(pos2(screen.left(), top), vec2(screen.width(), 32.0));
    // drawn over the canvas without taking its touches: a drag across it still changes the opacity
    egui::Area::new(Id::unique("layer opacity bar")).fixed_pos(band.min).order(Order::Middle).interactable(false).show(
        ctx,
        |ui| {
            let painter = ui.painter();
            painter.rect_filled(band, 0, argb(MODE_BAR));
            let ink = argb(MODE_TEXT);
            let amount = format!("{}%", (opacity * 100.0).round() as i32);
            let measure = |s: &str, size: f32, w: Weight| painter.layout_no_wrap(s.into(), w.font(size), ink).size().x;
            let width = 33.0 + measure("Opacity", 17.0, Weight::Bold) + 3.0 + measure(&amount, 14.0, Weight::Regular);
            let x = band.center().x - width / 2.0;
            opacity_glyph(painter, pos2(x + 16.5, band.center().y), ink, argb(MODE_BAR));
            let x =
                text(painter, pos2(x + 33.0, band.center().y), Align2::LEFT_CENTER, "Opacity", 17.0, Weight::Bold, ink);
            text(
                painter,
                pos2(x.right() + 3.0, band.center().y),
                Align2::LEFT_CENTER,
                &amount,
                14.0,
                Weight::Regular,
                ink,
            );
            let line = Rect::from_min_size(band.left_bottom(), vec2(screen.width() * opacity.clamp(0.0, 1.0), 4.0));
            painter.rect_filled(line, 0, argb(pc::ACCENT));
        },
    );
}

/// Opacity's glyph, 14 across about `at`: a ring behind and up to the right, a disc in front,
/// striped (in the band's colour, `gap`) where it covers the ring's inside.
fn opacity_glyph(painter: &egui::Painter, at: Pos2, ink: Color32, gap: Color32) {
    let back = at + vec2(1.7, -1.7);
    let front = at + vec2(-1.6, 1.6);
    let r = 4.9;
    painter.circle_stroke(back, r, Stroke::new(1.1, ink));
    painter.circle_filled(front, r, ink);
    // each stripe's run inside both circles
    let within = |from: Pos2, d: egui::Vec2, c: Pos2| {
        let f = from - c;
        let (a, b, k) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - r * r);
        let disc = b * b - 4.0 * a * k;
        (disc > 0.0).then(|| ((-b - disc.sqrt()) / (2.0 * a), (-b + disc.sqrt()) / (2.0 * a)))
    };
    for k in -1..=1 {
        let o = k as f32 * 2.2;
        let from = front + vec2(o - 5.0, -o - 5.0);
        let d = vec2(10.0, 10.0);
        if let (Some((a0, a1)), Some((b0, b1))) = (within(from, d, back), within(from, d, front)) {
            let (t0, t1) = (a0.max(b0).max(0.0), a1.min(b1).min(1.0));
            if t1 > t0 {
                painter.line_segment([from + d * t0, from + d * t1], Stroke::new(1.0, gap));
            }
        }
    }
}

/// The Layer / Pencil choice under the bar: the adjustment changes the layer, or is painted on.
/// Returns the one tapped (true Pencil).
fn modes(ctx: &egui::Context, look: &mut Look, pencil: bool, top: f32) -> Option<bool> {
    let screen = ctx.content_rect();
    let capsule = Rect::from_min_size(pos2(screen.center().x - 112.5, top), vec2(225.0, 36.0));
    let mut chosen = None;
    egui::Area::new(Id::unique("adjust modes")).fixed_pos(capsule.min).order(Order::Foreground).show(ctx, |ui| {
        ui.allocate_rect(capsule, Sense::click());
        ui.painter().rect_filled(capsule, 18, argb(MODES_FILL));
        for (k, (is_pencil, glyph, label)) in
            [(false, &icons::MODE_LAYER, "Layer"), (true, &icons::MODE_PENCIL, "Pencil")].into_iter().enumerate()
        {
            let item = Rect::from_min_size(capsule.min + vec2(2.0 + k as f32 * 111.0, 2.0), vec2(110.0, 32.0));
            if pencil == is_pencil {
                ui.painter().rect_filled(item, 16, argb(pc::ACCENT));
            }
            // drawn in 27 x 32 (the pencil 24 x 32) frames, shown as they are
            let w = if is_pencil { 24.0 } else { 27.0 };
            look.icon(ui, glyph, pos2(item.left() + 24.0 + w / 2.0, item.center().y), w, Color32::WHITE);
            let at = pos2(item.left() + 24.0 + w + 1.5, item.center().y);
            text(ui.painter(), at, Align2::LEFT_CENTER, label, 14.0, Weight::Regular, Color32::WHITE);
            if ui.interact(item, ui.scope_id().with(("adjust mode", k)), Sense::click()).clicked() {
                chosen = Some(is_pencil);
            }
        }
    });
    chosen
}

/// The floating panel of hue, saturation and brightness, each 0..100% with 50% leaving the layer
/// as it was: side by side where the window is wide, stacked where it is narrow.
fn hsb_panel(ctx: &egui::Context, session: &CanvasSession, shown: &AdjustShown) {
    let screen = ctx.content_rect();
    let wide = screen.width() > screen.height();
    let size = if wide { vec2(961.0, 95.0) } else { vec2(532.0, 220.0) };
    let rect = Rect::from_min_size(pos2(screen.center().x - size.x / 2.0, screen.bottom() - PANEL_RISE - size.y), size);
    let [h, s, b] = [0, 1, 2].map(|i| shown.hsb.get(i).copied().unwrap_or(0.5));
    egui::Area::new(Id::unique("hsb panel")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
        ui.allocate_rect(rect, Sense::click());
        ui.painter().rect_filled(rect, 33, argb(PANEL_FILL));
        let rows: [(&str, f32, Track); 3] =
            [("Hue", h, Track::Hue), ("Saturation", s, Track::Saturation), ("Brightness", b, Track::Brightness)];
        for (i, (label, value, track)) in rows.into_iter().enumerate() {
            let row = if wide {
                // side by side, 13 apart
                let w = (rect.width() - 32.0 - 26.0) / 3.0;
                Rect::from_min_size(pos2(rect.left() + 16.0 + i as f32 * (w + 13.0), rect.top() + 24.0), vec2(w, 55.0))
            } else {
                let w = rect.width() - 52.0;
                Rect::from_min_size(pos2(rect.left() + 26.0, rect.top() + 16.0 + i as f32 * 63.0), vec2(w, 55.0))
            };
            if let Some(v) = hsb_row(ui, row, label, value, track) {
                let pick = |k: usize, was: f32| if k == i { v } else { was };
                session.adjustment_hsb(pick(0, h), pick(1, s), pick(2, b));
            }
        }
    });
}

#[derive(Clone, Copy)]
enum Track {
    Hue,
    Saturation,
    Brightness,
}

impl Track {
    /// The track's colour at `t` (0..1).
    fn at(self, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Track::Hue => {
                let h = t * 6.0;
                let k = (h.floor() as usize).min(5);
                let f = h - k as f32;
                let (a, b) = (HUE_STOPS.get(k).copied().unwrap_or(0), HUE_STOPS.get(k + 1).copied().unwrap_or(0));
                lerp(argb(a), argb(b), f)
            }
            Track::Saturation => lerp(argb(0xFF808080), argb(0xFFFF0000), t),
            Track::Brightness => lerp(Color32::BLACK, Color32::WHITE, t),
        }
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// A labelled slider over a coloured track, its knob taking the track's colour where it sits.
/// Returns the value dragged to.
fn hsb_row(ui: &Ui, row: Rect, label: &str, value: f32, track: Track) -> Option<f32> {
    let top = row.top() + 6.0;
    text(ui.painter(), pos2(row.left(), top), Align2::LEFT_TOP, label, 16.0, Weight::Regular, argb(HSB_LABEL));
    let shown = format!("{}%", (value * 100.0).round() as i32);
    text(ui.painter(), pos2(row.right(), top), Align2::RIGHT_TOP, &shown, 16.0, Weight::Regular, argb(HSB_VALUE));
    let bar = Rect::from_min_size(pos2(row.left(), row.bottom() - 24.0), vec2(row.width(), 24.0));
    let y = bar.center().y;
    // the track: coloured along, drawn in short pieces
    let pieces = 48;
    for k in 0..pieces {
        let (t0, t1) = (k as f32 / pieces as f32, (k + 1) as f32 / pieces as f32);
        let a = pos2(bar.left() + bar.width() * t0, y);
        let b = pos2(bar.left() + bar.width() * t1, y);
        ui.painter().line_segment([a, b], Stroke::new(3.0, track.at((t0 + t1) / 2.0)));
    }
    let knob = vec2(24.0, 14.0);
    let x = (bar.left() + bar.width() * value).clamp(bar.left() + knob.x / 2.0, bar.right() - knob.x / 2.0);
    let k = Rect::from_center_size(pos2(x, y), knob);
    ui.painter().rect_filled(k.translate(vec2(0.0, 0.5)), 7, Color32::from_black_alpha(0x55));
    ui.painter().rect_filled(k, 7, track.at(value));
    let response = ui.interact(bar, ui.scope_id().with(("hsb", label)), Sense::click_and_drag());
    let p = response.interact_pointer_pos()?;
    Some(((p.x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0))
}
