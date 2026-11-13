//! The effect adjustments' toolbars, as the Android app's (ui/AdjustmentsUi.kt): Glitch, Noise,
//! Bloom, Halftone, Chromatic Aberration and Perspective Blur, floating 25 above the window's foot;
//! side by side where the window is wide, stacked where it is narrow. Their kinds switch along the
//! top; their sliders change the adjustment at once (adjusting.rs).

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use procraft_core::{AdjustShown, CanvasSession, ChromaticKind, GlitchKind, HalftoneKind, NoiseKind, PerspectiveKind};

use crate::icons::{self, Icon};
use crate::look::{Look, Shade, Themed, Weight, argb, pc, text};
use crate::transform::switch;

const PANEL_FILL: Themed = Themed(0xF71E1E1E, 0xF7F1F1F1);
const PANEL_RISE: f32 = 25.0;
const KINDS_FILL: Themed = Themed(0xFF535353, 0xFFFFFFFF);
const KINDS_EDGE: Themed = Themed(0xFF747474, 0xFFFAFAFA);
const MODES_TEXT: Themed = Themed(0xFFD0D0D0, 0xFF3B3B3B);
const LABEL: Themed = Themed(0xFF808080, 0xFF595959);
const VALUE: Themed = Themed(0xFFEDEDED, 0xFF363636);
const TRACK: Themed = Themed(0xFF2E2E2E, 0xFFDDDDDD);
const TICK: Themed = Themed(0xFF4A4A4A, 0xFFC8C8C8);
const NOISE_BUTTON: Themed = Themed(0xFF1E1E1E, 0xFFFFFFFF);
const MORE_FILL: Themed = Themed(0xFF1C1C1C, 0xFFFAFAFA);
const MORE_EDGE: Themed = Themed(0xFF3A3A3A, 0xFFE1E1E1);
const MORE_DOTS: Themed = Themed(0xFF9A9A9A, 0xFF595959);
const OPTIONS_MENU_FILL: Themed = Themed(0xF2262626, 0xF2F0F0F0);
const OPTIONS_GROUP_FILL: Themed = Themed(0xFF1B1B1B, 0xFFFFFFFF);
const OPTIONS_LINE: Themed = Themed(0xFF2E2E2E, 0xFFE2E2E2);
const MENU_TEXT: Themed = Themed(0xFFC5C5C5, 0xFF393939);
const PANEL_BUTTON: Themed = Themed(0xFF303030, 0xFFF8F8F8);
const PANEL_BUTTON_TEXT: Themed = Themed(0xFFA6A6A6, 0xFF515151);
const BALANCE_KNOB: u32 = 0xFF7A7A7A;
const GRID_LINE: u32 = 0xFF8A8A8A;
const CURVE_NODE: u32 = 0xFF0171EA;
/// The Delete pill over a tapped curve point.
const CURVE_MENU_FILL: Themed = Themed(0xFF262626, 0xFFF0F0F0);
const BALANCE_PAIRS: [(&str, &str, [u32; 3]); 3] = [
    ("Cyan", "Red", [0xFF00FFFF, 0xFF808080, 0xFFFF0000]),
    ("Magenta", "Green", [0xFFFF00FF, 0xFF808080, 0xFF00FF00]),
    ("Yellow", "Blue", [0xFFFFFF00, 0xFF808080, 0xFF0000FF]),
];
const BALANCE_TONES: [&str; 3] = ["Shadows", "Midtones", "Highlights"];

const GLITCH_KINDS: [(GlitchKind, &str, f32, &Icon); 4] = [
    (GlitchKind::Artifact, "Artifact", 111.0, &icons::GLITCH_ARTIFACT),
    (GlitchKind::Wave, "Wave", 99.0, &icons::GLITCH_WAVE),
    (GlitchKind::Signal, "Signal", 104.0, &icons::GLITCH_SIGNAL),
    (GlitchKind::Diverge, "Diverge", 113.0, &icons::GLITCH_DIVERGE),
];
const NOISE_KINDS: [(NoiseKind, &str, f32, &Icon); 3] = [
    (NoiseKind::Clouds, "Clouds", 149.0, &icons::NOISE_CLOUDS),
    (NoiseKind::Billows, "Billows", 149.0, &icons::NOISE_BILLOWS),
    (NoiseKind::Ridges, "Ridges", 148.0, &icons::NOISE_RIDGES),
];
const HALFTONE_KINDS: [(HalftoneKind, &str, f32, &Icon); 3] = [
    (HalftoneKind::FullColor, "Full Color", 140.0, &icons::HALFTONE_FULL),
    (HalftoneKind::ScreenPrint, "Screen Print", 157.0, &icons::HALFTONE_SCREEN),
    (HalftoneKind::Newspaper, "Newspaper", 151.0, &icons::HALFTONE_NEWS),
];
const CHROMA_KINDS: [(ChromaticKind, &str, f32, &Icon); 2] = [
    (ChromaticKind::Perspective, "Perspective", 204.0, &icons::CHROMA_PERSPECTIVE),
    (ChromaticKind::Displace, "Displace", 204.0, &icons::CHROMA_DISPLACE),
];
const PERSPECTIVE_KINDS: [(PerspectiveKind, &str, f32, &Icon); 2] = [
    (PerspectiveKind::Positional, "Positional", 205.0, &icons::PERSPECTIVE_POSITIONAL),
    (PerspectiveKind::Directional, "Directional", 211.5, &icons::PERSPECTIVE_DIRECTIONAL),
];

/// A slider's value as shown: None at nothing, Max at all (`max`), else a percentage.
pub(crate) fn shown(v: f32, max: bool) -> String {
    if v <= 0.0 {
        "None".into()
    } else if max && v >= 1.0 {
        "Max".into()
    } else {
        format!("{}%", (v * 100.0).round() as i32)
    }
}

/// Where a toolbar `size` big floats: centred, 25 above the window's foot.
pub(crate) fn panel_rect(screen: Rect, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(screen.center().x - w / 2.0, screen.bottom() - PANEL_RISE - h), vec2(w, h))
}

/// Draws a toolbar's panel and runs `inside` in it.
pub(crate) fn panel(ctx: &egui::Context, id: &str, rect: Rect, inside: impl FnOnce(&mut Ui)) {
    egui::Area::new(Id::unique(id)).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
        ui.allocate_rect(rect, Sense::click());
        ui.painter().rect_filled(rect, 33, argb(PANEL_FILL));
        inside(ui);
    });
}

/// A toolbar's row of kinds `width` wide, its top centred at `top`, the `chosen` one blue.
/// Returns the one tapped.
pub(crate) fn kinds_switch(
    ui: &Ui,
    look: &mut Look,
    top: Pos2,
    width: f32,
    kinds: &[(&str, f32, &Icon)],
    chosen: usize,
) -> Option<usize> {
    let track = Rect::from_min_size(pos2(top.x - width / 2.0, top.y), vec2(width, 34.0));
    ui.painter().rect_filled(track, 17, argb(KINDS_FILL));
    ui.painter().rect_stroke(track, 17, Stroke::new(1.0, argb(KINDS_EDGE)), egui::StrokeKind::Inside);
    let inner = track.shrink(2.0);
    let total: f32 = kinds.iter().map(|k| k.1).sum();
    let gap = if kinds.len() > 1 { (inner.width() - total) / (kinds.len() - 1) as f32 } else { 0.0 };
    let mut x = inner.left();
    let mut tapped = None;
    for (i, &(label, w, glyph)) in kinds.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(x, inner.top()), vec2(w, 30.0));
        let on = i == chosen;
        if on {
            ui.painter().rect_filled(cell, 15, argb(pc::ACCENT));
        }
        let ink = if on { Color32::WHITE } else { argb(MODES_TEXT) };
        let words = ui.painter().layout_no_wrap(label.into(), Weight::Regular.font(14.0), ink).size().x;
        let left = cell.center().x - (30.0 + 2.0 + words) / 2.0;
        look.icon(ui, glyph, pos2(left + 15.0, cell.center().y), 30.0, ink);
        text(ui.painter(), pos2(left + 32.0, cell.center().y), Align2::LEFT_CENTER, label, 14.0, Weight::Regular, ink);
        if ui.interact(cell, ui.scope_id().with(("kind", label)), Sense::click()).clicked() {
            tapped = Some(i);
        }
        x += w + gap;
    }
    tapped
}

#[allow(clippy::too_many_arguments)]
/// A labelled toolbar slider at `at` (its top left), `width` wide: `both` ways runs −1..1 with its
/// fill from the middle, else 0..1. Returns the value dragged to.
pub(crate) fn option_slider(
    ui: &Ui,
    at: Pos2,
    width: f32,
    label: &str,
    value: f32,
    shown: &str,
    both: bool,
    label_ink: impl Shade,
) -> Option<f32> {
    option_slider_let_go(ui, at, width, label, value, shown, both, label_ink).0
}

/// [`option_slider`], and whether it was let go just now.
fn option_slider_let_go(
    ui: &Ui,
    at: Pos2,
    width: f32,
    label: &str,
    value: f32,
    shown: &str,
    both: bool,
    label_ink: impl Shade,
) -> (Option<f32>, bool) {
    let top = at.y + 6.0;
    text(ui.painter(), pos2(at.x + 10.0, top), Align2::LEFT_TOP, label, 16.0, Weight::Regular, argb(label_ink));
    text(ui.painter(), pos2(at.x + width - 15.0, top), Align2::RIGHT_TOP, shown, 16.0, Weight::Regular, argb(VALUE));
    let bar = Rect::from_min_max(pos2(at.x + 10.0, at.y + 30.0), pos2(at.x + width - 11.0, at.y + 54.0));
    let y = bar.center().y;
    let t = if both { (value + 1.0) / 2.0 } else { value };
    let x = bar.left() + bar.width() * t.clamp(0.0, 1.0);
    let from = if both { bar.center().x } else { bar.left() };
    ui.painter().line_segment([pos2(bar.left(), y), pos2(bar.right(), y)], Stroke::new(2.0, argb(TRACK)));
    ui.painter().line_segment([pos2(from, y), pos2(x, y)], Stroke::new(2.0, argb(pc::ACCENT)));
    if both {
        ui.painter().line_segment([pos2(from, y - 6.0), pos2(from, y + 6.0)], Stroke::new(1.0, argb(TICK)));
    }
    let knob = vec2(17.0, 11.0);
    let kx = x.clamp(bar.left() + knob.x / 2.0, bar.right() - knob.x / 2.0);
    let k = Rect::from_center_size(pos2(kx, y), knob);
    ui.painter().rect_filled(k.translate(vec2(0.0, 0.5)), 5.5, Color32::from_black_alpha(0x33));
    ui.painter().rect_filled(k, 5.5, Color32::WHITE);
    // it changes as it is dragged, not where it is pressed
    let response = ui.interact(bar, ui.scope_id().with(("option slider", label)), Sense::drag());
    let let_go = response.drag_stopped();
    if !response.dragged() {
        return (None, let_go);
    }
    let moved = response.interact_pointer_pos().map(|p| {
        let t = ((p.x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0);
        if both { t * 2.0 - 1.0 } else { t }
    });
    (moved, let_go)
}

/// Recolor's toolbar: the Flood slider, how far from the colour under the reticle the fill
/// reaches. Returns the value dragged to.
pub(crate) fn recolor_toolbar(ctx: &egui::Context, threshold: f32) -> Option<f32> {
    let rect = panel_rect(ctx.content_rect(), 332.0, 70.0);
    let mut moved = None;
    panel(ctx, "recolor toolbar", rect, |ui| {
        let shown = format!("{}%", (threshold * 100.0).round());
        moved = option_slider(
            ui,
            pos2(rect.center().x - 150.0, rect.top() + 8.0),
            300.0,
            "Flood",
            threshold,
            &shown,
            false,
            LABEL,
        );
    });
    moved
}

#[derive(Default)]
pub struct Toolbars {
    /// A toolbar's "•••" menu is open.
    more_open: bool,
    /// Curves: the point pressed (its index, whether the press added it, whether it moved, and
    /// where it went down), and the point tapped, whose Delete shows over it.
    curve_press: Option<(u32, bool, bool, Pos2)>,
    curve_tapped: Option<u32>,
    gradient_map: crate::gradient_map::GradientMap,
    liquify: crate::liquify::Liquify,
}

impl Toolbars {
    /// The open adjustment's toolbar, if it has one.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        shown: &AdjustShown,
        gradients: &procraft_core::Gradients,
        colours: &crate::colours::Colours,
        backdrops: &mut Vec<f32>,
    ) {
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        match shown.adjustment {
            Some(procraft_core::Adjustment::Glitch) => glitch(ctx, look, session, shown, screen, wide),
            Some(procraft_core::Adjustment::Noise) => self.noise(ctx, look, session, shown, screen, wide),
            Some(procraft_core::Adjustment::Bloom) => bloom(ctx, session, shown, screen, wide),
            Some(procraft_core::Adjustment::Halftone) => halftone(ctx, look, session, shown, screen),
            Some(procraft_core::Adjustment::ChromaticAberration) => chromatic(ctx, look, session, shown, screen),
            Some(procraft_core::Adjustment::PerspectiveBlur) => perspective(ctx, look, session, shown, screen),
            Some(procraft_core::Adjustment::ColorBalance) => self.balance(ctx, look, session, shown, screen, wide),
            Some(procraft_core::Adjustment::Curves) => self.curves(ctx, session, shown, screen),
            Some(procraft_core::Adjustment::Liquify) => self.liquify.show(ctx, look, session, shown),
            Some(procraft_core::Adjustment::Plugin) => plugin_filter(ctx, session, shown, screen),
            Some(procraft_core::Adjustment::GradientMap) => {
                self.gradient_map.show(ctx, look, session, shown, gradients, colours, backdrops);
            }
            _ => {
                self.more_open = false;
                self.gradient_map.shut();
            }
        }
    }

    /// The noise toolbar: its three kinds, scale, octaves and turbulence, single or multi channel,
    /// added or not (where the window is wide, the last two in the "•••" menu).
    fn noise(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        shown: &AdjustShown,
        screen: Rect,
        wide: bool,
    ) {
        let rect = if wide { panel_rect(screen, 903.0, 113.0) } else { panel_rect(screen, 532.0, 292.0) };
        let mut more = None;
        panel(ctx, "noise toolbar", rect, |ui| {
            let kinds: Vec<(&str, f32, &Icon)> = NOISE_KINDS.iter().map(|k| (k.1, k.2, k.3)).collect();
            let chosen = NOISE_KINDS.iter().position(|k| k.0 == shown.noise_kind).unwrap_or(0);
            let top = rect.top() + if wide { 8.0 } else { 10.0 };
            if let Some(i) = kinds_switch(ui, look, pos2(rect.center().x, top), 472.0, &kinds, chosen) {
                session.adjustment_noise_kind(NOISE_KINDS.get(i).map_or(NoiseKind::Clouds, |k| k.0));
            }
            let names = ["Scale", "Octaves", "Turbulence"];
            for (i, name) in names.iter().enumerate() {
                let v = shown.noise.get(i).copied().unwrap_or(0.0);
                let at = if wide {
                    pos2(rect.left() + 16.0 + i as f32 * (251.0 + 19.0), top + 34.0 + 8.0)
                } else {
                    pos2(rect.center().x - 250.0, top + 34.0 + 8.0 + i as f32 * (54.0 + 8.5))
                };
                let w = if wide { 251.0 } else { 500.0 };
                if let Some(v) = option_slider(ui, at, w, name, v, &self::shown(v, false), false, MODES_TEXT) {
                    session.adjustment_noise_value(i as u32, v);
                }
            }
            if wide {
                let button = Rect::from_min_size(
                    pos2(rect.left() + 16.0 + 3.0 * 270.0, top + 34.0 + 8.0 + 3.0),
                    vec2(41.0, 40.0),
                );
                if more_button(ui, button) {
                    self.more_open = !self.more_open;
                }
                more = Some(button);
            } else {
                // single or multi, and additive, under the sliders
                let row =
                    Rect::from_min_size(pos2(rect.center().x - 246.0, rect.bottom() - 10.0 - 40.0), vec2(492.0, 40.0));
                for (k, (multi, label)) in [(false, "Single"), (true, "Multi")].into_iter().enumerate() {
                    let b = Rect::from_min_size(pos2(row.left() + k as f32 * 167.0, row.top()), vec2(160.0, 40.0));
                    let on = shown.noise_multi == multi;
                    ui.painter().rect_filled(b, 20, if on { argb(pc::ACCENT) } else { argb(NOISE_BUTTON) });
                    let ink = if on { Color32::WHITE } else { argb(MODES_TEXT) };
                    text(ui.painter(), b.center(), Align2::CENTER_CENTER, label, 15.0, Weight::Regular, ink);
                    if ui.interact(b, ui.scope_id().with(("noise channels", k)), Sense::click()).clicked() {
                        session.adjustment_noise_ways(multi, shown.noise_added);
                    }
                }
                let switch_at = pos2(row.right() - 63.0, row.center().y - 14.0);
                let words = pos2((row.left() + 334.0 + switch_at.x) / 2.0, row.center().y);
                text(ui.painter(), words, Align2::CENTER_CENTER, "Additive", 16.0, Weight::Regular, argb(MODES_TEXT));
                switch(ui, switch_at, shown.noise_added, "additive");
                let hit = Rect::from_min_size(switch_at, vec2(63.0, 28.0));
                if ui.interact(hit, ui.scope_id().with("additive"), Sense::click()).clicked() {
                    session.adjustment_noise_ways(shown.noise_multi, !shown.noise_added);
                }
            }
        });
        if let (true, Some(button)) = (self.more_open, more) {
            self.channel_menu(ctx, look, session, shown, button);
        }
    }

    /// The colour balance panel: three sliders between opposite colours for the tone chosen (side
    /// by side where the window is wide, the tones then in the "•••" menu after them).
    fn balance(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        shown: &AdjustShown,
        screen: Rect,
        wide: bool,
    ) {
        let rect = if wide { panel_rect(screen, 903.0, 95.0) } else { panel_rect(screen, 532.0, 272.0) };
        let tone = shown.balance_tone as usize;
        let mut more = None;
        panel(ctx, "balance panel", rect, |ui| {
            for (k, (left, right, colours)) in BALANCE_PAIRS.iter().enumerate() {
                let v = shown.balance.get(tone * 3 + k).copied().unwrap_or(0.5);
                let row = if wide {
                    Rect::from_min_size(
                        pos2(rect.left() + 16.0 + k as f32 * 270.0, rect.top() + 24.0),
                        vec2(251.0, 55.0),
                    )
                } else {
                    let top = rect.top() + 16.0 + k as f32 * 63.0;
                    Rect::from_min_size(pos2(rect.left() + 26.0, top), vec2(rect.width() - 52.0, 55.0))
                };
                if let Some(v) = balance_row(ui, row, left, right, v, colours) {
                    session.adjustment_balance(k as u32, v);
                }
            }
            if wide {
                let at = pos2(rect.left() + 16.0 + 3.0 * 270.0, rect.top() + 24.0 + 3.0);
                let button = Rect::from_min_size(at, vec2(41.0, 40.0));
                if more_button(ui, button) {
                    self.more_open = !self.more_open;
                }
                more = Some(button);
            } else {
                let top = rect.top() + 16.0 + 3.0 * 63.0 + 4.0;
                let row = Rect::from_min_size(pos2(rect.left() + 26.0, top), vec2(rect.width() - 52.0, 40.0));
                let w = (row.width() - 14.0) / 3.0;
                for (i, name) in BALANCE_TONES.iter().enumerate() {
                    let b = Rect::from_min_size(pos2(row.left() + i as f32 * (w + 7.0), row.top()), vec2(w, 40.0));
                    let chosen = tone == i;
                    ui.painter().rect_filled(b, 20, if chosen { argb(pc::ACCENT) } else { argb(PANEL_BUTTON) });
                    let ink = if chosen { Color32::WHITE } else { argb(PANEL_BUTTON_TEXT) };
                    text(ui.painter(), b.center(), Align2::CENTER_CENTER, name, 15.0, Weight::Regular, ink);
                    if ui.interact(b, ui.scope_id().with(("tone", i)), Sense::click()).clicked() {
                        session.adjustment_balance_tone(i as u32);
                    }
                }
            }
        });
        if let (true, Some(button)) = (self.more_open, more) {
            options_menu(ctx, &mut self.more_open, button, "Color Balance", 3.0 * 41.0 - 1.0, |ui, inner| {
                if let Some(i) = options_choices(ui, look, inner, 40.0, &BALANCE_TONES, tone) {
                    session.adjustment_balance_tone(i as u32);
                }
            });
        }
    }

    /// The curves panel: the chosen channel's curve over a grid (drag its points; touch elsewhere
    /// to add one; tap a point for Delete over it), and the channels.
    fn curves(&mut self, ctx: &egui::Context, session: &CanvasSession, shown: &AdjustShown, screen: Rect) {
        let rect = panel_rect(screen, 584.0, 183.0);
        let channel = shown.curve_channel;
        let points: Vec<Pos2> = shown
            .curves
            .get(channel as usize)
            .map(|c| c.as_chunks::<2>().0.iter().map(|&[x, y]| pos2(x, y)).collect())
            .unwrap_or_default();
        let mut delete_at = None;
        panel(ctx, "curves panel", rect, |ui| {
            let inner = rect.shrink(16.0);
            let graph = Rect::from_min_max(inner.min, pos2(inner.right() - 16.0 - 111.0, inner.bottom())).shrink(8.0);
            let to_screen = |p: Pos2| pos2(graph.left() + p.x * graph.width(), graph.bottom() - p.y * graph.height());
            let to_curve = |p: Pos2| {
                let x = (p.x - graph.left()) / graph.width();
                let y = (graph.bottom() - p.y) / graph.height();
                pos2(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
            };
            let line = Stroke::new(1.0, argb(GRID_LINE));
            for k in 0..=4 {
                let f = k as f32 / 4.0;
                let x = graph.left() + graph.width() * f;
                let y = graph.top() + graph.height() * f;
                ui.painter().line_segment([pos2(x, graph.top()), pos2(x, graph.bottom())], line);
                ui.painter().line_segment([pos2(graph.left(), y), pos2(graph.right(), y)], line);
            }
            let heights = session.adjustment_curve_shape(channel);
            let n = heights.len().max(2) as f32 - 1.0;
            let curve: Vec<Pos2> = heights.iter().enumerate().map(|(i, &h)| to_screen(pos2(i as f32 / n, h))).collect();
            ui.painter().add(egui::Shape::line(curve, Stroke::new(1.5, Color32::WHITE)));
            for &p in &points {
                let at = to_screen(p);
                ui.painter().circle_filled(at, 6.0, Color32::from_black_alpha(0x1F));
                ui.painter().circle_filled(at, 5.5, Color32::WHITE);
                ui.painter().circle_filled(at, 3.5, argb(CURVE_NODE));
            }
            // a press takes the point under it, or adds one; dragged, it moves; a tap on a point
            // offers Delete
            let area = graph.expand(8.0);
            let response = ui.interact(area, ui.scope_id().with(("curve", channel)), Sense::click_and_drag());
            if response.is_pointer_button_down_on()
                && let Some(p) = response.interact_pointer_pos()
            {
                match &mut self.curve_press {
                    None => {
                        self.curve_tapped = None;
                        let near = points.iter().position(|&q| to_screen(q).distance(p) <= 22.0);
                        let (index, added) = match near {
                            Some(i) => (i as u32, false),
                            None => {
                                let c = to_curve(p);
                                (session.adjustment_curve_add(c.x, c.y), true)
                            }
                        };
                        self.curve_press = Some((index, added, false, p));
                    }
                    Some((index, _, moved, down)) => {
                        if *moved || down.distance(p) >= 8.0 {
                            *moved = true;
                            let c = to_curve(p);
                            session.adjustment_curve_move(*index, c.x, c.y);
                        }
                    }
                }
            } else if let Some((index, added, moved, _)) = self.curve_press.take() {
                if !moved && !added {
                    self.curve_tapped = Some(index);
                }
            } else if response.clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                // pressed and let go within one frame: a tap
                match points.iter().position(|&q| to_screen(q).distance(p) <= 22.0) {
                    Some(i) => self.curve_tapped = Some(i as u32),
                    None => {
                        let c = to_curve(p);
                        session.adjustment_curve_add(c.x, c.y);
                    }
                }
            }
            if let Some(p) = self.curve_tapped.and_then(|i| points.get(i as usize)) {
                // a pill 78 x 44 over the point, its bottom 14 above it
                let at = to_screen(*p);
                delete_at = Some(Rect::from_min_size(pos2(at.x - 39.0, at.y - 14.0 - 44.0), vec2(78.0, 44.0)));
            }
            // the channels
            let column = Rect::from_min_max(pos2(inner.right() - 111.0, inner.top()), inner.max);
            let h = (column.height() - 3.0) / 4.0;
            for (i, name) in ["Gamma", "Red", "Green", "Blue"].iter().enumerate() {
                let top = column.top() + i as f32 * (h + 1.0);
                let cell = Rect::from_min_size(pos2(column.left(), top), vec2(column.width(), h));
                let (upper, lower) = (if i == 0 { 13 } else { 0 }, if i == 3 { 13 } else { 0 });
                let corners = egui::CornerRadius { nw: upper, ne: upper, sw: lower, se: lower };
                let chosen = channel as usize == i;
                ui.painter().rect_filled(cell, corners, if chosen { argb(pc::ACCENT) } else { argb(PANEL_BUTTON) });
                let ink = if chosen { Color32::WHITE } else { argb(PANEL_BUTTON_TEXT) };
                text(ui.painter(), cell.center(), Align2::CENTER_CENTER, name, 16.0, Weight::Regular, ink);
                if ui.interact(cell, ui.scope_id().with(("channel", i)), Sense::click()).clicked() {
                    session.adjustment_curve_channel(i as u32);
                    self.curve_tapped = None;
                }
            }
        });
        if let Some(pill) = delete_at {
            egui::Area::new(Id::unique("curve delete")).fixed_pos(pill.min).order(Order::Tooltip).show(ctx, |ui| {
                ui.painter().rect_filled(pill, 22, argb(CURVE_MENU_FILL));
                text(
                    ui.painter(),
                    pill.center(),
                    Align2::CENTER_CENTER,
                    "Delete",
                    17.0,
                    Weight::Regular,
                    Color32::WHITE,
                );
                if ui.allocate_rect(pill, Sense::click()).clicked()
                    && let Some(i) = self.curve_tapped.take()
                {
                    session.adjustment_curve_delete(i);
                }
            });
        }
    }

    /// Noise's "•••" menu, 16 above its button: additive, and single or multi channel.
    fn channel_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        shown: &AdjustShown,
        button: Rect,
    ) {
        let height = 44.0 + 12.0 + 89.0;
        options_menu(ctx, &mut self.more_open, button, "Channel", height, |ui, inner| {
            let ink = argb(MENU_TEXT);
            let additive = Rect::from_min_size(inner.min, vec2(inner.width(), 44.0));
            ui.painter().rect_filled(additive, 16, argb(OPTIONS_GROUP_FILL));
            text(
                ui.painter(),
                pos2(additive.left() + 12.0, additive.center().y),
                Align2::LEFT_CENTER,
                "Additive",
                15.0,
                Weight::Regular,
                ink,
            );
            let switch_at = pos2(additive.right() - 8.0 - 63.0, additive.center().y - 14.0);
            switch(ui, switch_at, shown.noise_added, "additive in the menu");
            if ui.interact(additive, ui.scope_id().with("menu additive"), Sense::click()).clicked() {
                session.adjustment_noise_ways(shown.noise_multi, !shown.noise_added);
            }
            let group = Rect::from_min_size(pos2(inner.left(), additive.bottom() + 12.0), vec2(inner.width(), 89.0));
            let chosen = usize::from(shown.noise_multi);
            if let Some(k) = options_choices(ui, look, group, 44.0, &["Single", "Multi"], chosen) {
                session.adjustment_noise_ways(k == 1, shown.noise_added);
            }
        });
    }
}

/// A toolbar's "•••" menu, 16 above its `button`: `title` over what `inside` draws in the
/// `height` under it. A tap outside closes it (`open` goes false), as does Escape.
pub(crate) fn options_menu(
    ctx: &egui::Context,
    open: &mut bool,
    button: Rect,
    title: &str,
    height: f32,
    inside: impl FnOnce(&mut Ui, Rect),
) {
    if crate::ui::escape(ctx) {
        *open = false;
        return;
    }
    let screen = ctx.content_rect();
    let whole = 16.0 + 28.0 + 16.0 + height + 10.0;
    let menu = Rect::from_min_size(pos2(button.center().x - 130.0, button.top() - 16.0 - whole), vec2(260.0, whole));
    egui::Area::new(Id::unique("options menu scrim")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
        if ui.allocate_rect(screen, Sense::click()).clicked() {
            *open = false;
        }
    });
    egui::Area::new(Id::unique("options menu")).fixed_pos(menu.min).order(Order::Tooltip).show(ctx, |ui| {
        ui.allocate_rect(menu, Sense::click());
        ui.painter().rect_filled(menu, 26, argb(OPTIONS_MENU_FILL));
        let at = pos2(menu.left() + 22.0, menu.top() + 16.0);
        text(ui.painter(), at, Align2::LEFT_TOP, title, 23.0, Weight::Bold, argb(MENU_TEXT));
        let inner = Rect::from_min_max(pos2(menu.left() + 10.0, menu.top() + 60.0), menu.max - vec2(10.0, 10.0));
        inside(ui, inner);
    });
}

/// Choices in a rounded group of an options menu, `height` each: the `chosen` one blue with a
/// tick, a line between them. Returns the one tapped.
pub(crate) fn options_choices(
    ui: &Ui,
    look: &mut Look,
    group: Rect,
    height: f32,
    labels: &[&str],
    chosen: usize,
) -> Option<usize> {
    ui.painter().rect_filled(group, 16, argb(OPTIONS_GROUP_FILL));
    let mut tapped = None;
    for (k, label) in labels.iter().enumerate() {
        let row = Rect::from_min_size(
            pos2(group.left(), group.top() + k as f32 * (height + 1.0)),
            vec2(group.width(), height),
        );
        if k > 0 {
            let line = Rect::from_min_size(pos2(row.left(), row.top() - 1.0), vec2(row.width(), 1.0));
            ui.painter().rect_filled(line, 0, argb(OPTIONS_LINE));
        }
        let on = k == chosen;
        if on {
            let (top, bottom) = (if k == 0 { 16 } else { 0 }, if k + 1 == labels.len() { 16 } else { 0 });
            let corners = egui::CornerRadius { nw: top, ne: top, sw: bottom, se: bottom };
            ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
        }
        let ink = if on { Color32::WHITE } else { argb(MENU_TEXT) };
        text(
            ui.painter(),
            pos2(row.left() + 12.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            15.0,
            Weight::Regular,
            ink,
        );
        if on {
            look.icon(ui, &icons::TICK, pos2(row.right() - 14.0 - 10.0, row.center().y), 20.0, Color32::WHITE);
        }
        if ui.interact(row, ui.scope_id().with(("option", label)), Sense::click()).clicked() {
            tapped = Some(k);
        }
    }
    tapped
}

/// A slider between two named colours along its `colours`, its middle leaving the colour as it
/// was. Returns the value dragged to.
fn balance_row(ui: &Ui, row: Rect, left: &str, right: &str, value: f32, colours: &[u32; 3]) -> Option<f32> {
    let top = row.top() + 6.0;
    text(ui.painter(), pos2(row.left(), top), Align2::LEFT_TOP, left, 16.0, Weight::Regular, argb(MENU_TEXT));
    text(ui.painter(), pos2(row.right(), top), Align2::RIGHT_TOP, right, 16.0, Weight::Regular, argb(VALUE));
    let bar = Rect::from_min_size(pos2(row.left(), row.bottom() - 24.0), vec2(row.width(), 24.0));
    let y = bar.center().y;
    let [a, m, b] = colours.map(argb);
    let pieces = 48;
    for k in 0..pieces {
        let (t0, t1) = (k as f32 / pieces as f32, (k + 1) as f32 / pieces as f32);
        let t = (t0 + t1) / 2.0;
        let colour = if t < 0.5 { mix(a, m, t * 2.0) } else { mix(m, b, t * 2.0 - 1.0) };
        let (x0, x1) = (bar.left() + bar.width() * t0, bar.left() + bar.width() * t1);
        ui.painter().line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(3.0, colour));
    }
    let knob = vec2(24.0, 14.0);
    let x = (bar.left() + bar.width() * value).clamp(bar.left() + knob.x / 2.0, bar.right() - knob.x / 2.0);
    let k = Rect::from_center_size(pos2(x, y), knob);
    ui.painter().rect_filled(k.translate(vec2(0.0, 0.5)), 7, Color32::from_black_alpha(0x55));
    ui.painter().rect_filled(k, 7, argb(BALANCE_KNOB));
    let response = ui.interact(bar, ui.scope_id().with(("balance", left)), Sense::click_and_drag());
    let p = response.interact_pointer_pos()?;
    Some(((p.x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0))
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// A toolbar's "•••" button. Returns whether it was tapped.
fn more_button(ui: &Ui, button: Rect) -> bool {
    ui.painter().rect_filled(button, 20, argb(MORE_FILL));
    ui.painter().rect_stroke(button, 20, Stroke::new(1.0, argb(MORE_EDGE)), egui::StrokeKind::Inside);
    for k in -1..=1 {
        ui.painter().circle_filled(button.center() + vec2(k as f32 * 10.0, 0.0), 3.0, argb(MORE_DOTS));
    }
    ui.interact(button, ui.scope_id().with("more"), Sense::click()).clicked()
}

/// The glitch toolbar: its four kinds and the kind's sliders (diverge's shifts both ways).
fn glitch(
    ctx: &egui::Context,
    look: &mut Look,
    session: &CanvasSession,
    shown: &AdjustShown,
    screen: Rect,
    wide: bool,
) {
    let rect = if wide { panel_rect(screen, 973.0, 120.0) } else { panel_rect(screen, 640.0, 167.0) };
    panel(ctx, "glitch toolbar", rect, |ui| {
        let kinds: Vec<(&str, f32, &Icon)> = GLITCH_KINDS.iter().map(|k| (k.1, k.2, k.3)).collect();
        let chosen = GLITCH_KINDS.iter().position(|k| k.0 == shown.glitch_kind).unwrap_or(0);
        if let Some(i) = kinds_switch(ui, look, pos2(rect.center().x, rect.top() + 8.0), 466.0, &kinds, chosen) {
            session.adjustment_glitch_kind(GLITCH_KINDS.get(i).map_or(GlitchKind::Artifact, |k| k.0));
        }
        let names: &[&str] = match shown.glitch_kind {
            GlitchKind::Artifact | GlitchKind::Signal => &["Amount", "Block Size", "Zoom"],
            GlitchKind::Wave => &["Amplitude", "Frequency", "Zoom"],
            GlitchKind::Diverge => &["Red shift", "Green shift", "Blue shift", "Zoom"],
        };
        let top = rect.top() + 8.0 + 34.0 + 12.0;
        for (k, name) in names.iter().enumerate() {
            let v = shown.glitch.get(k).copied().unwrap_or(0.0);
            // diverge's shifts go both ways from the middle
            let both = shown.glitch_kind == GlitchKind::Diverge && k < 3;
            let (at, w) = if wide {
                // all in one row, 19 apart
                let each = (941.0 - 19.0 * (names.len() - 1) as f32) / names.len() as f32;
                (pos2(rect.center().x - 470.5 + k as f32 * (each + 19.0), top), each)
            } else {
                let (row, col) = (k / 2, k % 2);
                (pos2(rect.center().x - 304.0 + col as f32 * 308.0, top + row as f32 * 54.0), 300.0)
            };
            let label = format!("{}%", (v * 100.0).round() as i32);
            if let Some(v) = option_slider(ui, at, w, name, v, &label, both, LABEL) {
                session.adjustment_glitch_value(k as u32, v);
            }
        }
    });
}

/// The bloom toolbar: transition, size and burn.
fn bloom(ctx: &egui::Context, session: &CanvasSession, shown: &AdjustShown, screen: Rect, wide: bool) {
    let rect = if wide { panel_rect(screen, 1013.0, 95.0) } else { panel_rect(screen, 532.0, 220.0) };
    panel(ctx, "bloom toolbar", rect, |ui| {
        for (i, name) in ["Transition", "Size", "Burn"].iter().enumerate() {
            let v = shown.bloom.get(i).copied().unwrap_or(0.0);
            let (at, w) = if wide {
                // side by side, 301 wide and 39 apart
                (pos2(rect.center().x - 490.5 + i as f32 * 340.0, rect.top() + 24.0), 301.0)
            } else {
                (pos2(rect.center().x - 250.0, rect.top() + 24.0 + i as f32 * 62.5), 500.0)
            };
            if let Some(v) = option_slider(ui, at, w, name, v, &self::shown(v, true), false, LABEL) {
                session.adjustment_bloom_value(i as u32, v);
            }
        }
    });
}

/// A plugin's filter's toolbar: one for all of them, its controls as the file lists them.
fn plugin_filter(ctx: &egui::Context, session: &CanvasSession, shown: &AdjustShown, screen: Rect) {
    use procraft_core::{PluginControl, plugin_slider_along, plugin_slider_at, plugin_slider_shown};
    let Some(filter) = &shown.plugin else { return };
    let rows = filter.controls.len().max(1) as f32;
    let rect = panel_rect(screen, 532.0, 41.0 + 54.0 * rows + 8.5 * (rows - 1.0));
    panel(ctx, "plugin filter toolbar", rect, |ui| {
        for (i, control) in filter.controls.iter().enumerate() {
            let at = pos2(rect.center().x - 250.0, rect.top() + 24.0 + i as f32 * 62.5);
            match control {
                PluginControl::Slider { control } => {
                    let along = plugin_slider_along(control.clone(), control.value);
                    let reads = plugin_slider_shown(control.clone(), control.value);
                    let (moved, let_go) =
                        option_slider_let_go(ui, at, 500.0, &control.label, along, &reads, false, LABEL);
                    if let Some(along) = moved {
                        let to = plugin_slider_at(control.clone(), along);
                        session.plugin_filter_slider(control.key.clone(), to, false);
                    }
                    if let_go {
                        session.plugin_filter_slider(control.key.clone(), control.value, true);
                    }
                }
                PluginControl::Switch { control } => {
                    let row = Rect::from_min_size(at, vec2(500.0, 54.0));
                    let ink = argb(LABEL);
                    let left = pos2(row.left() + 10.0, row.center().y);
                    text(ui.painter(), left, Align2::LEFT_CENTER, &control.label, 16.0, Weight::Regular, ink);
                    let place = pos2(row.right() - 15.0 - 63.0, row.center().y - 14.0);
                    crate::transform::switch(ui, place, control.on, &control.key);
                    if ui.interact(row, ui.scope_id().with(&control.key), Sense::click()).clicked() {
                        session.plugin_filter_switch(control.key.clone(), !control.on);
                    }
                }
                // (a filter has none)
                PluginControl::File { .. } => {}
            }
        }
    });
}

/// The halftone toolbar: full colour, screen print or newspaper.
fn halftone(ctx: &egui::Context, look: &mut Look, session: &CanvasSession, shown: &AdjustShown, screen: Rect) {
    let rect = panel_rect(screen, 507.0, 66.0);
    panel(ctx, "halftone toolbar", rect, |ui| {
        let kinds: Vec<(&str, f32, &Icon)> = HALFTONE_KINDS.iter().map(|k| (k.1, k.2, k.3)).collect();
        let chosen = HALFTONE_KINDS.iter().position(|k| k.0 == shown.halftone_kind).unwrap_or(0);
        if let Some(i) = kinds_switch(ui, look, pos2(rect.center().x, rect.top() + 16.0), 475.0, &kinds, chosen) {
            session.adjustment_halftone_kind(HALFTONE_KINDS.get(i).map_or(HalftoneKind::FullColor, |k| k.0));
        }
    });
}

/// The chromatic aberration toolbar: perspective or displace, with the kind's two sliders.
fn chromatic(ctx: &egui::Context, look: &mut Look, session: &CanvasSession, shown: &AdjustShown, screen: Rect) {
    let rect = panel_rect(screen, 668.0, 120.0);
    panel(ctx, "chromatic toolbar", rect, |ui| {
        let kinds: Vec<(&str, f32, &Icon)> = CHROMA_KINDS.iter().map(|k| (k.1, k.2, k.3)).collect();
        let chosen = CHROMA_KINDS.iter().position(|k| k.0 == shown.chroma_kind).unwrap_or(0);
        if let Some(i) = kinds_switch(ui, look, pos2(rect.center().x, rect.top() + 8.0), 428.0, &kinds, chosen) {
            session.adjustment_chroma_kind(CHROMA_KINDS.get(i).map_or(ChromaticKind::Perspective, |k| k.0));
        }
        let names = if shown.chroma_kind == ChromaticKind::Perspective {
            ["Transition", "Fall off"]
        } else {
            ["Blur", "Transparency"]
        };
        for (k, name) in names.iter().enumerate() {
            let v = shown.chroma.get(k).copied().unwrap_or(0.0);
            let at = pos2(rect.center().x - 310.0 + k as f32 * 320.0, rect.top() + 8.0 + 34.0 + 12.0);
            if let Some(v) = option_slider(ui, at, 300.0, name, v, &self::shown(v, true), false, LABEL) {
                session.adjustment_chroma_value(k as u32, v);
            }
        }
    });
}

/// The perspective blur toolbar: positional or directional.
fn perspective(ctx: &egui::Context, look: &mut Look, session: &CanvasSession, shown: &AdjustShown, screen: Rect) {
    let rect = panel_rect(screen, 492.0, 66.0);
    panel(ctx, "perspective toolbar", rect, |ui| {
        let kinds: Vec<(&str, f32, &Icon)> = PERSPECTIVE_KINDS.iter().map(|k| (k.1, k.2, k.3)).collect();
        let chosen = PERSPECTIVE_KINDS.iter().position(|k| k.0 == shown.perspective_kind).unwrap_or(0);
        if let Some(i) = kinds_switch(ui, look, pos2(rect.center().x, rect.top() + 16.0), 460.0, &kinds, chosen) {
            session.adjustment_perspective_kind(PERSPECTIVE_KINDS.get(i).map_or(PerspectiveKind::Positional, |k| k.0));
        }
    });
}
