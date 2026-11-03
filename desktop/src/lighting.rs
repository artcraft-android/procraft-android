//! The Lighting Studio over a model, as the Android app's (ui/LightingStudioUi.kt): its bar
//! (Cancel, Add Light, Environment and Done), the model on its floor among its lights, seen through
//! the studio's camera, which a drag takes round the model and the wheel nearer; a light's cube is
//! dragged across the view, and clicked, its settings show. The Environment popover: whether the
//! environment shows, the exposure, and which environment lights the model. Also the environments
//! themselves (our own panoramas, bundled with the Android app) a model is lit by.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use procraft_core::{CanvasSession, ModelLight};

use crate::icons::{self, Icon};
use crate::look::{Look, Weight, argb, dragged_by, pc, text};
use crate::panel::{self, PANEL_TITLE};

const STUDIO_BAR: u32 = 0xFF26292A;
const STUDIO_TITLE: u32 = 0xFFD8D8D8;
const PILL_FILL: u32 = 0xFF1D2021;
const PILL_EDGE: u32 = 0xFF3B3E3F;
const PILL_LIT: u32 = 0xFF474747;
const PILL_INK: u32 = 0xFFC1C1C1;
const DONE_BLUE: u32 = 0xFF0190FF;
const POPOVER_FILL: u32 = 0xFF282A2D;
const GROUP_FILL: u32 = 0xFF232528;
const ROW_LINE: u32 = 0xFF2C2E31;
const LABEL_INK: u32 = 0xFFB7B7B7;
const DIM_INK: u32 = 0xFF8A8D8E;
const SETTINGS_FILL: u32 = 0xFF29292B;
const SETTINGS_GROUP: u32 = 0xFF222427;
const SATURATION_START: u32 = 0xFF808080;
/// The hue bar's colours, evenly spaced along it (as the lights take them).
const HUE_STOPS: [u32; 8] =
    [0xFFFF0000, 0xFFFF6E00, 0xFFFFDC00, 0xFF35FF00, 0xFF00FFEC, 0xFF0034FF, 0xFFAA00FF, 0xFFFF0000];
const BAR_HEIGHT: f32 = 85.0;
/// How bright the light can be taken: Min to Max on the exposure slider.
const MOST_EXPOSURE: f32 = 5.0;
/// Light Settings' size, its arrow included.
const SETTINGS_SIZE: (f32, f32) = (280.0, 347.0);
/// How far a press moves before it is a drag rather than a click (points).
const SLOP: f32 = 8.0;

/// The environment a model is lit by unless another was chosen.
pub const DEFAULT_ENVIRONMENT: &str = "mountain";

macro_rules! environment {
    ($label:literal, $asset:literal, $icon:path) => {
        (
            $label,
            $asset,
            &$icon,
            include_bytes!(concat!("../../android/app/src/main/assets/environments/", $asset, ".hdr")) as &[u8],
        )
    };
}

/// The environments as the studio lists them: their names, assets, pictures and panoramas.
const ENVIRONMENTS: [(&str, &str, &Icon, &[u8]); 11] = [
    environment!("Studio", "studio", icons::ENV_STUDIO),
    environment!("Coast", "coast", icons::ENV_COAST),
    environment!("Auditorium", "auditorium", icons::ENV_AUDITORIUM),
    environment!("City", "city", icons::ENV_CITY),
    environment!("Nightlife", "nightlife", icons::ENV_NIGHTLIFE),
    environment!("Portside", "portside", icons::ENV_PORTSIDE),
    environment!("Industrial", "industrial", icons::ENV_INDUSTRIAL),
    environment!("Sunrise", "sunrise", icons::ENV_SUNRISE),
    environment!("Beach", "beach", icons::ENV_BEACH),
    environment!("Mountain", "mountain", icons::ENV_MOUNTAIN),
    environment!("Daytime", "daytime", icons::ENV_DAYTIME),
];

/// Lights the model by environment `name`; false if there's no such one.
pub fn choose_environment(session: &CanvasSession, name: &str) -> bool {
    let Some((_, asset, _, hdr)) = ENVIRONMENTS.iter().find(|e| e.1 == name) else { return false };
    session.set_model_environment((*asset).to_owned(), hdr.to_vec())
}

/// A model just opened: lit by the environment it was last lit by, else the default.
pub fn light_model(session: &CanvasSession) {
    let name = session.model_environment().filter(|n| ENVIRONMENTS.iter().any(|e| e.1 == n));
    choose_environment(session, name.as_deref().unwrap_or(DEFAULT_ENVIRONMENT));
}

/// The lighting as the studio found it, put back by Cancel.
struct Kept {
    environment: String,
    exposure: f32,
    shown: bool,
    lights: Vec<ModelLight>,
}

/// A press in the view: the light it grabbed (none: the camera turns), where it last was and how
/// far it has gone.
struct Press {
    grabbed: Option<u32>,
    last: Pos2,
    travelled: f32,
}

#[derive(Default)]
pub struct LightingStudio {
    pub open: bool,
    environment_open: bool,
    /// The light whose settings are showing.
    light_open: Option<u32>,
    kept: Option<Kept>,
    press: Option<Press>,
    /// A slider held: which, and where the pointer was across it last frame (points).
    held: Option<(Id, f32)>,
}

/// The hue bar's colour at `t` along it (0 to 1).
fn hue_colour(t: f32) -> Color32 {
    let at = t.clamp(0.0, 1.0) * (HUE_STOPS.len() - 1) as f32;
    let i = (at as usize).min(HUE_STOPS.len() - 2);
    let (a, b) = (HUE_STOPS.get(i).copied().unwrap_or(0xFFFFFFFF), HUE_STOPS.get(i + 1).copied().unwrap_or(0xFFFFFFFF));
    crate::look::mix(argb(a), argb(b), at - i as f32)
}

/// A share as a light's sliders show it: None, a percentage, or Max.
fn level(v: f32) -> String {
    if v <= 0.0 {
        "None".into()
    } else if v >= 1.0 {
        "Max".into()
    } else {
        format!("{}%", (v * 100.0).round())
    }
}

impl LightingStudio {
    /// Opens the studio over the model (not while its 2D texture shows).
    pub fn begin(&mut self, session: &CanvasSession) {
        if session.model_info().is_none() || session.model_flat() {
            return;
        }
        self.kept = Some(Kept {
            environment: session.model_environment().unwrap_or_else(|| DEFAULT_ENVIRONMENT.to_owned()),
            exposure: session.model_exposure(),
            shown: session.model_environment_shown(),
            lights: session.model_lights(),
        });
        session.set_lighting_studio(true);
        self.open = true;
        self.environment_open = false;
        self.light_open = None;
        self.press = None;
    }

    /// Leaves the studio, keeping what was changed or (`keep` false) putting the lighting back.
    fn close(&mut self, session: &CanvasSession, keep: bool) {
        if let (false, Some(kept)) = (keep, self.kept.take()) {
            if session.model_environment().as_deref() != Some(kept.environment.as_str()) {
                choose_environment(session, &kept.environment);
            }
            session.set_model_exposure(kept.exposure);
            session.set_model_environment_shown(kept.shown);
            session.set_model_lights(kept.lights);
        }
        session.set_lighting_studio(false);
        self.kept = None;
        self.open = false;
    }

    /// Another artwork is shown.
    pub fn forget(&mut self) {
        *self = LightingStudio::default();
    }

    /// The studio, while open; a notice to show, if any.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        let screen = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        let mut said = None;
        // the view: a press with a popover open only closes it
        egui::Area::new(Id::unique("lighting studio view"))
            .fixed_pos(screen.min)
            .order(Order::Middle)
            .fade_in(false)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(screen, Sense::click_and_drag());
                let ours = |p: Pos2| ui.ctx().layer_id_at(p) == Some(ui.layer_id());
                let (pressed, down, at) =
                    ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.interact_pos()));
                if pressed && let Some(p) = at.filter(|p| ours(*p)) {
                    if self.environment_open || self.light_open.is_some() {
                        self.environment_open = false;
                        self.light_open = None;
                    } else {
                        let grabbed = session.model_light_at(p.x * ppp, p.y * ppp);
                        self.press = Some(Press { grabbed, last: p, travelled: 0.0 });
                    }
                }
                if let (Some(press), Some(p)) = (self.press.as_mut(), at)
                    && down
                {
                    let d = p - press.last;
                    press.last = p;
                    let before = press.travelled;
                    press.travelled += d.x.abs() + d.y.abs();
                    // still for a click until it goes past the slop
                    if press.travelled > SLOP && before <= SLOP || before > SLOP {
                        match press.grabbed {
                            Some(i) => session.move_model_light(i, d.x * ppp, d.y * ppp),
                            None => session.studio_turn(d.x * ppp, d.y * ppp),
                        }
                    }
                }
                // let go: a press that stayed still opens its light's settings
                if !down
                    && let Some(press) = self.press.take()
                    && press.travelled <= SLOP
                {
                    self.light_open = press.grabbed;
                }
                // the wheel takes the camera nearer or farther
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 && response.contains_pointer() {
                    session.studio_zoom((scroll * 0.003).exp());
                }
            });
        egui::Area::new(Id::unique("lighting studio bar"))
            .fixed_pos(screen.min)
            .order(Order::Foreground)
            .fade_in(false)
            .show(ctx, |ui| {
                let bar = Rect::from_min_size(screen.min, vec2(screen.width(), BAR_HEIGHT));
                if ui.allocate_rect(bar, Sense::click()).clicked() {
                    self.environment_open = false;
                }
                ui.painter().rect_filled(bar, 0, argb(STUDIO_BAR));
                text(
                    ui.painter(),
                    bar.min + vec2(32.0, 27.0),
                    Align2::LEFT_TOP,
                    "Lighting Studio",
                    20.0,
                    Weight::Bold,
                    argb(STUDIO_TITLE),
                );
                let done = Rect::from_min_size(pos2(bar.right() - 20.0 - 44.0, bar.top() + 21.0), vec2(44.0, 45.0));
                ui.painter().rect_filled(done, 22, argb(DONE_BLUE));
                look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
                let pill = Rect::from_min_size(pos2(done.left() - 13.0 - 278.0, bar.top() + 21.0), vec2(278.0, 43.0));
                ui.painter().rect_filled(pill, 21.5, argb(PILL_FILL));
                ui.painter().rect_stroke(pill, 21.5, Stroke::new(1.0, argb(PILL_EDGE)), egui::StrokeKind::Inside);
                let mut x = pill.left();
                for (label, width) in [("Cancel", 73.0), ("Add Light", 92.0), ("Environment", 113.0)] {
                    let cell = Rect::from_min_size(pos2(x, pill.top()), vec2(width, 43.0));
                    x += width;
                    if label == "Environment" && self.environment_open {
                        ui.painter().rect_filled(cell.shrink(2.0), 19.5, argb(PILL_LIT));
                    }
                    text(
                        ui.painter(),
                        cell.center(),
                        Align2::CENTER_CENTER,
                        label,
                        17.0,
                        Weight::Regular,
                        argb(PILL_INK),
                    );
                    if ui.interact(cell, ui.scope_id().with(("studio pill", label)), Sense::click()).clicked() {
                        match label {
                            "Cancel" => self.close(session, false),
                            "Add Light" => {
                                self.environment_open = false;
                                self.light_open = None;
                                if !session.add_model_light() {
                                    said = Some("A model can have four lights at most.");
                                }
                            }
                            _ => self.environment_open = !self.environment_open,
                        }
                    }
                }
                if ui.interact(done, ui.scope_id().with("studio done"), Sense::click()).clicked() {
                    self.close(session, true);
                }
            });
        if !self.open {
            return said;
        }
        if self.environment_open {
            self.environment_popover(ctx, look, session, screen);
        }
        if let Some(i) = self.light_open {
            match (session.model_lights().get(i as usize).copied(), session.model_light_on_screen(i)) {
                (Some(light), Some(at)) if at.len() >= 3 => {
                    let at = [0, 1, 2].map(|k| at.get(k).copied().unwrap_or(0.0) / ppp);
                    if let Some(s) = self.light_settings(ctx, look, session, i, light, at, screen) {
                        said = Some(s);
                    }
                }
                _ => self.light_open = None,
            }
        }
        if crate::ui::escape(ctx) {
            if self.light_open.is_some() {
                self.light_open = None;
            } else if self.environment_open {
                self.environment_open = false;
            } else {
                self.close(session, false);
            }
        }
        said
    }

    /// The Environment popover: Show Environment, Exposure, and the environments, the one lighting
    /// the model blue.
    fn environment_popover(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, screen: Rect) {
        let width = 350.0;
        let tall =
            (8.0_f32 + 40.0 + 13.0 + 44.0 + 10.0 + 84.0 + 12.0 + 44.0 * 11.0 + 10.0 + 12.0).min(screen.height() - 97.0);
        let rect =
            Rect::from_min_size(pos2(screen.right() - 12.0 - width, screen.top() + BAR_HEIGHT), vec2(width, tall));
        egui::Area::new(Id::unique("studio environment"))
            .fixed_pos(rect.min)
            .order(Order::Foreground)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.allocate_rect(rect, Sense::click());
                let shadow = egui::epaint::Shadow {
                    offset: [0, 0],
                    blur: panel::POPOVER_SHADOW_RADIUS as u8,
                    spread: 0,
                    color: argb(panel::POPOVER_SHADOW),
                };
                ui.painter().add(shadow.as_shape(rect, panel::POPOVER_CORNER));
                ui.painter().rect_filled(rect, panel::POPOVER_CORNER, argb(POPOVER_FILL));
                text(
                    ui.painter(),
                    rect.min + vec2(26.0, 8.0 + 20.0),
                    Align2::LEFT_CENTER,
                    "Environment",
                    24.0,
                    Weight::Bold,
                    argb(PANEL_TITLE),
                );
                let (x0, x1) = (rect.left() + 10.0, rect.right() - 10.0);
                let mut y = rect.top() + 8.0 + 40.0 + 13.0;
                // Show Environment
                let row = Rect::from_min_max(pos2(x0, y), pos2(x1, y + 44.0));
                ui.painter().rect_filled(row, 14, argb(GROUP_FILL));
                text(
                    ui.painter(),
                    pos2(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    "Show Environment",
                    17.0,
                    Weight::Regular,
                    argb(LABEL_INK),
                );
                let shown = session.model_environment_shown();
                crate::transform::switch(
                    ui,
                    pos2(row.right() - 8.0 - 63.0, row.center().y - 14.0),
                    shown,
                    "show environment",
                );
                if ui.interact(row, ui.scope_id().with("show environment"), Sense::click()).clicked() {
                    session.set_model_environment_shown(!shown);
                }
                y += 44.0 + 10.0;
                // Exposure
                let exposure = Rect::from_min_max(pos2(x0, y), pos2(x1, y + 84.0));
                exposure_slider(ui, session, exposure, &mut self.held);
                y += 84.0 + 12.0;
                // the environments
                let list = Rect::from_min_max(pos2(x0, y), pos2(x1, rect.bottom() - 12.0));
                let mut child = ui.new_child(UiBuilder::new().max_rect(list));
                egui::ScrollArea::vertical().id_salt("environments").show(&mut child, |ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    let chosen = session.model_environment();
                    let n = ENVIRONMENTS.len();
                    let (whole, _) =
                        ui.allocate_exact_size(vec2(list.width(), 44.0 * n as f32 + (n - 1) as f32), Sense::hover());
                    ui.painter().rect_filled(whole, 14, argb(GROUP_FILL));
                    for (i, (label, asset, icon, _)) in ENVIRONMENTS.iter().enumerate() {
                        let cell = Rect::from_min_size(
                            pos2(whole.left(), whole.top() + 45.0 * i as f32),
                            vec2(whole.width(), 44.0),
                        );
                        if i > 0 {
                            let line =
                                Rect::from_min_size(pos2(cell.left(), cell.top() - 1.0), vec2(cell.width(), 1.0));
                            ui.painter().rect_filled(line, 0, argb(ROW_LINE));
                        }
                        let on = chosen.as_deref() == Some(*asset);
                        if on {
                            let corners = egui::CornerRadius {
                                nw: if i == 0 { 14 } else { 0 },
                                ne: if i == 0 { 14 } else { 0 },
                                sw: if i == n - 1 { 14 } else { 0 },
                                se: if i == n - 1 { 14 } else { 0 },
                            };
                            ui.painter().rect_filled(cell, corners, argb(pc::ACCENT));
                        }
                        let ink = if on { Color32::WHITE } else { argb(LABEL_INK) };
                        text(
                            ui.painter(),
                            pos2(cell.left() + 11.0, cell.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            16.0,
                            Weight::Regular,
                            ink,
                        );
                        look.icon(ui, icon, pos2(cell.right() - 16.0 - 10.0, cell.center().y), 20.0, Color32::WHITE);
                        if !on && ui.interact(cell, ui.scope_id().with(("environment", i)), Sense::click()).clicked() {
                            choose_environment(session, asset);
                        }
                    }
                });
            });
    }

    /// Light Settings, pointing at light `index` (its middle and how far its cube reaches, `at`)
    /// from above it (below it when there's no room): its hue, how much of that colour it has,
    /// how strong it is; Delete and Duplicate. A notice to show, if any.
    #[allow(clippy::too_many_arguments)]
    fn light_settings(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        index: u32,
        light: ModelLight,
        [x, y, reach]: [f32; 3],
        screen: Rect,
    ) -> Option<&'static str> {
        let (w, h) = SETTINGS_SIZE;
        let above = y - reach - h > screen.top() + BAR_HEIGHT + 8.0;
        let left = (x - w / 2.0).clamp(screen.left() + 12.0, (screen.right() - w - 12.0).max(screen.left() + 12.0));
        let top = if above { y - reach - h } else { y + reach };
        let rect = Rect::from_min_size(pos2(left, top), vec2(w, h));
        let mut said = None;
        egui::Area::new(Id::unique("light settings")).fixed_pos(rect.min).order(Order::Foreground).fade_in(false).show(
            ctx,
            |ui| {
                ui.allocate_rect(rect, Sense::click());
                let arrow = (x - left).clamp(28.0, w - 28.0);
                let body = panel::solid_frame(
                    ui,
                    look,
                    "light settings",
                    rect,
                    arrow,
                    above,
                    panel::POPOVER_CORNER,
                    SETTINGS_FILL,
                );
                text(
                    ui.painter(),
                    body.min + vec2(16.0, 5.0 + 20.0),
                    Align2::LEFT_CENTER,
                    "Light Settings",
                    24.0,
                    Weight::Bold,
                    argb(PANEL_TITLE),
                );
                let (x0, x1) = (body.left() + 10.0, body.right() - 10.0);
                let group = Rect::from_min_max(
                    pos2(x0, body.top() + 5.0 + 40.0 + 10.0),
                    pos2(x1, body.top() + 5.0 + 40.0 + 10.0 + 5.0 + 56.0 * 3.0 + 5.0),
                );
                ui.painter().rect_filled(group, 14, argb(SETTINGS_GROUP));
                let hue = hue_colour(light.hue);
                let degrees = if light.hue >= 1.0 { 360.0 } else { egui::ecolor::Hsva::from(hue).h * 360.0 };
                let rows: [(&str, String, f32, [Color32; 2], Color32); 3] = [
                    (
                        "Hue",
                        if light.saturation <= 0.0 { "None".into() } else { format!("{}°", degrees.round()) },
                        light.hue,
                        [Color32::TRANSPARENT; 2],
                        hue,
                    ),
                    (
                        "Saturation",
                        level(light.saturation),
                        light.saturation,
                        [argb(SATURATION_START), hue],
                        crate::look::mix(argb(SATURATION_START), hue, light.saturation),
                    ),
                    (
                        "Intensity",
                        level(light.intensity),
                        light.intensity,
                        [Color32::BLACK, Color32::WHITE],
                        Color32::from_gray((light.intensity.clamp(0.0, 1.0) * 255.0) as u8),
                    ),
                ];
                for (k, (label, shown, value, ends, knob)) in rows.into_iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(group.left(), group.top() + 5.0 + 56.0 * k as f32),
                        vec2(group.width(), 56.0),
                    );
                    text(
                        ui.painter(),
                        row.min + vec2(16.0, 6.0),
                        Align2::LEFT_TOP,
                        label,
                        17.0,
                        Weight::Regular,
                        argb(DIM_INK),
                    );
                    text(
                        ui.painter(),
                        pos2(row.right() - 16.0, row.top() + 6.0),
                        Align2::RIGHT_TOP,
                        &shown,
                        17.0,
                        Weight::Regular,
                        Color32::WHITE,
                    );
                    let slider = Rect::from_min_max(pos2(row.left(), row.bottom() - 24.0), row.max);
                    let track = Rect::from_min_max(
                        pos2(slider.left() + 14.0, slider.center().y - 1.5),
                        pos2(slider.right() - 14.0, slider.center().y + 1.5),
                    );
                    if k == 0 {
                        // the hue bar, its colours in turn
                        let n = HUE_STOPS.len() - 1;
                        for s in 0..n {
                            let a = track.left() + track.width() * s as f32 / n as f32;
                            let b = track.left() + track.width() * (s + 1) as f32 / n as f32;
                            let part = Rect::from_min_max(pos2(a, track.top()), pos2(b, track.bottom()));
                            let (ca, cb) = (
                                argb(HUE_STOPS.get(s).copied().unwrap_or(0)),
                                argb(HUE_STOPS.get(s + 1).copied().unwrap_or(0)),
                            );
                            gradient(ui, part, ca, cb);
                        }
                    } else {
                        gradient(ui, track, ends[0], ends[1]);
                    }
                    let ends_in = 18.5;
                    let kx = slider.left() + ends_in + (slider.width() - 2.0 * ends_in) * value.clamp(0.0, 1.0);
                    ui.painter().rect_filled(
                        Rect::from_center_size(pos2(kx, slider.center().y), vec2(16.0, 10.0)),
                        5.0,
                        knob,
                    );
                    let response =
                        ui.interact(slider, ui.scope_id().with(("light slider", label)), Sense::click_and_drag());
                    let by = dragged_by(&mut self.held, &response);
                    if by != 0.0 {
                        let moved = by / (slider.width() - 2.0 * ends_in);
                        let v = (value + moved).clamp(0.0, 1.0);
                        let changed = match k {
                            0 => ModelLight { hue: v, ..light },
                            1 => ModelLight { saturation: v, ..light },
                            _ => ModelLight { intensity: v, ..light },
                        };
                        session.set_model_light(index, changed);
                    }
                }
                let actions =
                    Rect::from_min_max(pos2(x0, group.bottom() + 10.0), pos2(x1, group.bottom() + 10.0 + 89.0));
                ui.painter().rect_filled(actions, 14, argb(SETTINGS_GROUP));
                let line = Rect::from_min_size(pos2(actions.left(), actions.top() + 44.0), vec2(actions.width(), 1.0));
                ui.painter().rect_filled(line, 0, argb(ROW_LINE));
                for (k, label) in ["Delete", "Duplicate"].into_iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(actions.left(), actions.top() + 45.0 * k as f32),
                        vec2(actions.width(), 44.0),
                    );
                    text(
                        ui.painter(),
                        pos2(row.left() + 16.0, row.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        17.0,
                        Weight::Regular,
                        argb(LABEL_INK),
                    );
                    if ui.interact(row, ui.scope_id().with(("light action", label)), Sense::click()).clicked() {
                        if k == 0 {
                            session.delete_model_light(index);
                        } else if !session.duplicate_model_light(index) {
                            said = Some("A model can have four lights at most.");
                        }
                        self.light_open = None;
                    }
                }
            },
        );
        said
    }
}

/// A bar across `rect` from colour `a` on its left to `b` on its right.
fn gradient(ui: &Ui, rect: Rect, a: Color32, b: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), b);
    mesh.colored_vertex(rect.right_bottom(), b);
    mesh.colored_vertex(rect.left_bottom(), a);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Exposure, from Min (none) to Max (5 times), in `rect`: its value on the right and a
/// black-to-white track whose knob is filled with the grey the exposure makes (white from 100%
/// up). Dragging moves it by as far as the pointer goes.
fn exposure_slider(ui: &Ui, session: &CanvasSession, rect: Rect, held: &mut Option<(Id, f32)>) {
    let value = session.model_exposure();
    let shown = if value <= 0.0 {
        "Min".to_owned()
    } else if value >= MOST_EXPOSURE {
        "Max".to_owned()
    } else {
        format!("{}%", (value * 100.0).round())
    };
    ui.painter().rect_filled(rect, 14, argb(GROUP_FILL));
    text(ui.painter(), rect.min + vec2(16.0, 19.0), Align2::LEFT_TOP, "Exposure", 17.0, Weight::Regular, argb(DIM_INK));
    text(
        ui.painter(),
        pos2(rect.right() - 16.0, rect.top() + 19.0),
        Align2::RIGHT_TOP,
        &shown,
        17.0,
        Weight::Regular,
        Color32::WHITE,
    );
    let slider =
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - 13.0 - 30.0), pos2(rect.right(), rect.bottom() - 13.0));
    let y = slider.center().y;
    let track = Rect::from_min_max(pos2(slider.left() + 16.0, y - 1.0), pos2(slider.right() - 16.0, y + 1.0));
    gradient(ui, track, Color32::BLACK, Color32::WHITE);
    let ends = 18.5;
    let x = slider.left() + ends + (slider.width() - 2.0 * ends) * (value / MOST_EXPOSURE).clamp(0.0, 1.0);
    let grey = Color32::from_gray((value.clamp(0.0, 1.0) * 255.0) as u8);
    ui.painter().rect_filled(Rect::from_center_size(pos2(x, y), vec2(16.0, 9.0)), 4.5, grey);
    let response = ui.interact(slider, ui.scope_id().with("exposure"), Sense::click_and_drag());
    let by = dragged_by(held, &response);
    if by != 0.0 {
        let moved = by / (slider.width() - 2.0 * ends) * MOST_EXPOSURE;
        session.set_model_exposure((value + moved).clamp(0.0, MOST_EXPOSURE));
    }
}
