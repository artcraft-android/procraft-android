//! The drawing guide's editor, as the Android app's (ui/GuideUi.kt): its title with Cancel and
//! Done, the colour bar, the canvas fitted between them and the toolbar (the four kinds, the
//! kind's look and assisted drawing), the guide's handles dragged on the canvas (a perspective
//! guide's points added by a tap). The guide itself is the core's (guide.rs).

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, pos2, vec2};
use procraft_core::{CanvasSession, GuideInfo, GuideType, SymmetryType};

use crate::adjust_panels::{kinds_switch, option_slider, options_choices, options_menu, panel, panel_rect, shown};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, text};

const TOP: Themed = Themed(0xFF1F1F1F, 0xFFE1E1E3);
const BUTTON: Themed = Themed(0xFF1C1C1C, 0xFFF5F8F9);
const BUTTON_EDGE: Themed = Themed(0xFF383838, 0xFFDCDEDF);
const TITLE: Themed = Themed(0xFFD8D8D8, 0xFF3A3A3A);
const DONE: u32 = 0xFF0190FF;
const NOTE: Themed = Themed(0xFF4A4A4A, 0xFFB5B5B5);
const LABEL: Themed = Themed(0xFFD0D0D0, 0xFF414141);
const OPTIONS: u32 = 0xFF303030;
const NODE_CENTRE: u32 = 0xFF0A84FF;
const NODE_TURN: u32 = 0xFF30D158;
/// The canvas fitted between a fifth of the window's height above and below, 40 in from the sides;
/// a press within 30 of a handle takes it.
const FIT_BAND: f32 = 0.2;
const FIT_SIDE: f32 = 40.0;
const REACH: f32 = 30.0;

/// The colour bar's stops, as the engine's (dark through blue, green, orange, red, pink, white).
const BAR: [u32; 21] = [
    0xFF05040A, 0xFF010C2C, 0xFF001857, 0xFF083984, 0xFF128BB6, 0xFF19C6CB, 0xFF1BD0B8, 0xFF2FD083, 0xFF67C064,
    0xFFBC9B4C, 0xFFCE9049, 0xFFD97644, 0xFFDA574A, 0xFFDC414D, 0xFFE53D71, 0xFFEC36AB, 0xFFF534DE, 0xFFF55CF9,
    0xFFFB92FD, 0xFFFCCAFF, 0xFFE8E1E9,
];

const KINDS: [(GuideType, &str, f32, &icons::Icon); 4] = [
    (GuideType::Grid, "2D Grid", 114.0, &icons::GUIDE_GRID),
    (GuideType::Isometric, "Isometric", 123.0, &icons::GUIDE_ISOMETRIC),
    (GuideType::Perspective, "Perspective", 138.0, &icons::CHROMA_PERSPECTIVE),
    (GuideType::Symmetry, "Symmetry", 129.0, &icons::GUIDE_SYMMETRY),
];

/// A grid size slider's place for a size, and back (canvas pixels, 10 to 500).
fn size_to_slider(px: f32) -> f32 {
    ((px - 10.0) / 490.0).clamp(0.0, 1.0).sqrt()
}

fn slider_to_size(v: f32) -> f32 {
    10.0 + v * v * 490.0
}

/// What a press on the canvas took: a vanishing point (by index), the centre, or the turning handle.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Grab {
    Point(u32),
    Centre,
    Turn,
    Nothing,
}

#[derive(Default)]
pub struct GuideEditor {
    pub open: bool,
    grab: Option<Grab>,
    symmetry_open: bool,
}

impl GuideEditor {
    /// Opens the editor (turning the guide on), the canvas fitted between its bars.
    pub fn begin(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        if !session.guide().on {
            session.set_guide_on(true);
        }
        let screen = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        let (w, h) = (screen.width() * ppp, screen.height() * ppp);
        let band = h * FIT_BAND;
        session.guide_edit_begin(FIT_SIDE * ppp, band, w - FIT_SIDE * ppp, h - band);
        self.open = true;
        self.symmetry_open = false;
    }

    fn end(&mut self, session: &CanvasSession, keep: bool) {
        session.guide_edit_end(keep);
        self.open = false;
        self.symmetry_open = false;
    }

    /// What a press at window pixel `p` takes.
    fn grab(&mut self, session: &CanvasSession, g: &GuideInfo, p: Pos2, ppp: f32) -> Grab {
        self.symmetry_open = false;
        if g.kind == GuideType::Perspective {
            return session.touch_vanishing_point(p.x, p.y).map_or(Grab::Nothing, Grab::Point);
        }
        let near = |v: &[f32]| v.first().zip(v.get(1)).is_some_and(|(&x, &y)| pos2(x, y).distance(p) <= REACH * ppp);
        if near(&g.handle) {
            Grab::Turn
        } else if near(&g.centre) {
            Grab::Centre
        } else {
            Grab::Nothing
        }
    }

    /// Shows the editor while it is open; returns true when it ends.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) -> bool {
        if !self.open {
            return false;
        }
        let g = session.guide();
        let screen = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        // the canvas: handles dragged, perspective points added; nothing paints
        egui::Area::new(Id::unique("guide canvas")).fixed_pos(screen.min).order(Order::Middle).show(ctx, |ui| {
            let response = ui.allocate_rect(screen, Sense::click_and_drag());
            if response.is_pointer_button_down_on() {
                let pointer = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
                if self.grab.is_none()
                    && let Some(p) = pointer
                {
                    self.grab = Some(self.grab(session, &g, p * ppp, ppp));
                }
                if let (Some(grab), Some(p)) = (self.grab, response.interact_pointer_pos()) {
                    let p = p * ppp;
                    match grab {
                        Grab::Point(i) => _ = session.move_vanishing_point(i, p.x, p.y),
                        Grab::Centre => _ = session.move_guide_centre(p.x, p.y),
                        Grab::Turn => _ = session.turn_guide(p.x, p.y),
                        Grab::Nothing => {}
                    }
                }
            } else {
                self.grab = None;
            }
            // the handles
            let node = |at: Pos2, fill: u32| {
                ui.painter().circle_filled(at, 7.0, Color32::WHITE);
                ui.painter().circle_filled(at, 5.0, argb(fill));
            };
            if g.kind == GuideType::Perspective {
                for &[x, y] in g.vanishing.as_chunks::<2>().0 {
                    node(pos2(x, y) / ppp, NODE_CENTRE);
                }
            } else {
                if let [x, y, ..] = g.handle.as_slice() {
                    node(pos2(*x, *y) / ppp, NODE_TURN);
                }
                if let [x, y, ..] = g.centre.as_slice() {
                    node(pos2(*x, *y) / ppp, NODE_CENTRE);
                }
            }
        });
        let mut ended = None;
        egui::Area::new(Id::unique("guide top")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            let band = Rect::from_min_size(screen.min, vec2(screen.width(), 82.0));
            ui.allocate_rect(band, Sense::click());
            ui.painter().rect_filled(band, 0, argb(TOP));
            text(
                ui.painter(),
                band.min + vec2(32.0, 27.0),
                Align2::LEFT_TOP,
                "Drawing Guides",
                20.0,
                Weight::Bold,
                argb(TITLE),
            );
            let done = Rect::from_min_size(pos2(band.right() - 20.0 - 44.0, band.top() + 20.0), vec2(44.0, 44.0));
            let cancel = Rect::from_min_size(pos2(done.left() - 13.0 - 73.0, band.top() + 20.0), vec2(73.0, 43.0));
            ui.painter().rect_filled(cancel, 21.5, argb(BUTTON));
            ui.painter().rect_stroke(cancel, 21.5, Stroke::new(1.0, argb(BUTTON_EDGE)), egui::StrokeKind::Inside);
            text(ui.painter(), cancel.center(), Align2::CENTER_CENTER, "Cancel", 17.0, Weight::Regular, argb(TITLE));
            if ui.interact(cancel, ui.scope_id().with("cancel"), Sense::click()).clicked() {
                ended = Some(false);
            }
            ui.painter().circle_filled(done.center(), 22.0, argb(DONE));
            look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
            if ui.interact(done, ui.scope_id().with("done"), Sense::click()).clicked() {
                ended = Some(true);
            }
            // the colour bar
            let bar = Rect::from_min_size(pos2(screen.left(), screen.top() + 84.0), vec2(screen.width(), 10.0));
            colour_bar(ui, bar, g.hue);
            let response = ui.interact(bar, ui.scope_id().with("colour bar"), Sense::click_and_drag());
            if let Some(p) = response.interact_pointer_pos()
                && response.is_pointer_button_down_on()
            {
                let hue = ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
                session.set_guide_look(g.opacity, g.thickness, g.size, hue);
            }
            if g.kind == GuideType::Perspective && g.vanishing.is_empty() {
                let note = Rect::from_min_size(pos2(screen.left(), screen.top() + 94.0), vec2(screen.width(), 24.0));
                ui.painter().rect_filled(note, 0, argb(NOTE));
                text(
                    ui.painter(),
                    note.center(),
                    Align2::CENTER_CENTER,
                    "Tap to create vanishing points",
                    12.0,
                    Weight::Regular,
                    argb(TITLE),
                );
            }
        });
        self.toolbar(ctx, look, session, &g, screen);
        if let Some(keep) = ended {
            self.end(session, keep);
            return true;
        }
        false
    }

    /// The toolbar: the four kinds, then the kind's look and assisted drawing (on a wide window the
    /// whole width less 8 each side, the sliders in one row and the switch at the end).
    fn toolbar(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, g: &GuideInfo, screen: Rect) {
        let wide = screen.width() > screen.height();
        let (w, h) = if wide { (screen.width() - 16.0, 137.0) } else { (652.0, 199.0) };
        let rect = panel_rect(screen, w, h);
        let mut options_button = Rect::NOTHING;
        let set_look =
            |opacity: f32, thickness: f32, size: f32| _ = session.set_guide_look(opacity, thickness, size, g.hue);
        panel(ctx, "guide toolbar", rect, |ui| {
            let kinds: Vec<(&str, f32, &icons::Icon)> = KINDS.iter().map(|k| (k.1, k.2, k.3)).collect();
            let chosen = KINDS.iter().position(|k| k.0 == g.kind).unwrap_or(0);
            if let Some(i) = kinds_switch(ui, look, pos2(rect.center().x, rect.top() + 16.0), 546.0, &kinds, chosen)
                && let Some(k) = KINDS.get(i)
            {
                session.set_guide_kind(k.0);
                self.symmetry_open = false;
            }
            let (slider_w, gap) = if wide { (257.0, 19.0) } else { (300.0, 20.0) };
            let row_top = if wide { rect.top() + 16.0 + 34.0 + 16.0 } else { rect.top() + 14.0 + 34.0 + 14.0 };
            let left = if wide { rect.left() + 16.0 } else { rect.center().x - 310.0 };
            if let Some(v) = option_slider(
                ui,
                pos2(left, row_top),
                slider_w,
                "Opacity",
                g.opacity,
                &shown(g.opacity, true),
                false,
                LABEL,
            ) {
                set_look(v, g.thickness, g.size);
            }
            if let Some(v) = option_slider(
                ui,
                pos2(left + slider_w + gap, row_top),
                slider_w,
                "Thickness",
                g.thickness,
                &shown(g.thickness, true),
                false,
                LABEL,
            ) {
                set_look(g.opacity, v, g.size);
            }
            // the third: the grid's size, or symmetry's options; upright on a row of its own
            let (third, third_top) =
                if wide { (left + (slider_w + gap) * 2.0, row_top) } else { (left, row_top + 54.0 + 8.0) };
            match g.kind {
                GuideType::Grid | GuideType::Isometric => {
                    let size = format!("{}px", g.size.round() as i32);
                    if let Some(v) = option_slider(
                        ui,
                        pos2(third, third_top),
                        slider_w,
                        "Grid size",
                        size_to_slider(g.size),
                        &size,
                        false,
                        LABEL,
                    ) {
                        set_look(g.opacity, g.thickness, slider_to_size(v));
                    }
                }
                GuideType::Symmetry => {
                    options_button = Rect::from_min_size(pos2(third, third_top + 7.0), vec2(slider_w, 40.0));
                    ui.painter().rect_filled(options_button, 20.0, argb(OPTIONS));
                    text(
                        ui.painter(),
                        options_button.center(),
                        Align2::CENTER_CENTER,
                        "Options",
                        15.0,
                        Weight::Regular,
                        argb(LABEL),
                    );
                    if ui.interact(options_button, ui.scope_id().with("guide options"), Sense::click()).clicked() {
                        self.symmetry_open = !self.symmetry_open;
                    }
                }
                GuideType::Perspective => {}
            }
            // assisted drawing, at the row's end (upright, beside the third)
            let switch_y = third_top + 27.0;
            let right = if wide { rect.right() - 22.0 } else { third + slider_w + gap + slider_w };
            let switch_at = pos2(right - 63.0, switch_y - 14.0);
            crate::transform::switch(ui, switch_at, g.assisted, "assisted");
            let label_at = pos2(if wide { switch_at.x - 12.0 } else { third + slider_w + gap + 8.0 }, switch_y);
            let align = if wide { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
            text(ui.painter(), label_at, align, "Assisted Drawing", 15.0, Weight::Regular, argb(LABEL));
            let tap = Rect::from_min_size(switch_at, vec2(63.0, 28.0));
            if ui.interact(tap, ui.scope_id().with("assisted"), Sense::click()).clicked() {
                session.set_guide_assisted(!g.assisted);
            }
        });
        if self.symmetry_open && g.kind == GuideType::Symmetry {
            self.symmetry_options(ctx, look, session, g, options_button);
        }
    }

    /// Symmetry's options over its button: vertical, horizontal, quadrant or radial; turned
    /// instead of mirrored; assisted.
    fn symmetry_options(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        g: &GuideInfo,
        button: Rect,
    ) {
        let kinds = [SymmetryType::Vertical, SymmetryType::Horizontal, SymmetryType::Quadrant, SymmetryType::Radial];
        let chosen = kinds.iter().position(|k| *k == g.symmetry).unwrap_or(0);
        let height = 44.0 * 4.0 + 3.0 + 14.0 + 40.0 * 2.0;
        let mut open = true;
        options_menu(ctx, &mut open, button, "Guide Options", height, |ui, inner| {
            let group = Rect::from_min_size(inner.min, vec2(inner.width(), 44.0 * 4.0 + 3.0));
            let labels = ["Vertical", "Horizontal", "Quadrant", "Radial"];
            if let Some(k) = options_choices(ui, look, group, 44.0, &labels, chosen)
                && let Some(kind) = kinds.get(k)
            {
                session.set_guide_symmetry(*kind, g.rotational);
            }
            for (i, (label, on)) in
                [("Rotational Symmetry", g.rotational), ("Assisted Drawing", g.assisted)].into_iter().enumerate()
            {
                let row = Rect::from_min_size(
                    pos2(inner.left(), group.bottom() + 14.0 + 40.0 * i as f32),
                    vec2(inner.width(), 40.0),
                );
                text(
                    ui.painter(),
                    pos2(row.left() + 6.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    15.0,
                    Weight::Regular,
                    argb(LABEL),
                );
                crate::transform::switch(ui, pos2(row.right() - 63.0, row.center().y - 14.0), on, label);
                if ui.interact(row, ui.scope_id().with(("symmetry switch", i)), Sense::click()).clicked() {
                    if i == 0 {
                        session.set_guide_symmetry(g.symmetry, !g.rotational);
                    } else {
                        session.set_guide_assisted(!g.assisted);
                    }
                }
            }
        });
        if !open {
            self.symmetry_open = false;
        }
    }
}

/// The colour bar: its gradient, and a knob where the guide's colour sits.
fn colour_bar(ui: &egui::Ui, rect: Rect, hue: f32) {
    let y = rect.center().y;
    let strip = Rect::from_min_max(pos2(rect.left(), y - 2.75), pos2(rect.right(), y + 2.75));
    let mut mesh = egui::Mesh::default();
    let n = BAR.len() - 1;
    for (i, &c) in BAR.iter().enumerate() {
        let x = strip.left() + strip.width() * i as f32 / n as f32;
        mesh.colored_vertex(pos2(x, strip.top()), argb(c));
        mesh.colored_vertex(pos2(x, strip.bottom()), argb(c));
        if i > 0 {
            let k = (i * 2) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    let x = (rect.left() + rect.width() * hue.clamp(0.0, 1.0)).clamp(rect.left() + 7.0, rect.right() - 7.0);
    let stop = BAR.get((hue.clamp(0.0, 1.0) * n as f32).round() as usize).copied().unwrap_or(0xFFFFFFFF);
    ui.painter().circle_filled(pos2(x, y), 7.0, Color32::WHITE);
    ui.painter().circle_filled(pos2(x, y), 5.5, argb(stop));
}
