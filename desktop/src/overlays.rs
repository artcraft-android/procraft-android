//! What shows over the canvas besides the bars and panels, as the Android app has it
//! (ui/AppRoot.kt, ui/Eyedropper.kt, ui/AdjustmentsUi.kt): the notice band under the top bar
//! ("Undo Brush stroke"), the eyedropper's band and loupe, and the bar of a snapped shape.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};
use procraft_core::{CanvasSession, Pick, QuickShapeInfo, QuickShapeKind};

use crate::icons;
use crate::look::{Look, Weight, argb, pc, text};

const NOTICE_FILL: u32 = 0xC2424242;
const NOTICE_INK: u32 = 0xFFD7D7D7;
const NOTICE_IN: f32 = 0.3;
const NOTICE_SHOWN: Duration = Duration::from_millis(750);
const NOTICE_WARNING_SHOWN: Duration = Duration::from_millis(1500);
const MESSAGE_BAR: u32 = 0xFF606060;
const MESSAGE_TEXT: u32 = 0xFFB5B5B5;
const LOUPE_OUTER: f32 = 69.0;
const LOUPE_INNER: f32 = 53.5;
const LOUPE_LIFT: f32 = 41.0;
const LOUPE_EDGE: u32 = 0xFF808080;
const MODE_BAR: u32 = 0xD63B3B3B;
const MODE_TEXT: u32 = 0xFFC5C5C5;
const MODES_FILL: u32 = 0xFF7B7B7B;

/// A passing message under the top bar; a warning stays longer.
struct Notice {
    text: String,
    since: Instant,
    warning: bool,
}

/// The eyedropper: where it is (window pixels), what it shows there, and the colour under it.
pub struct Picking {
    pub at: Pos2,
    image: Option<TextureHandle>,
    pub rgb: Option<u32>,
}

#[derive(Default)]
pub struct Overlays {
    notice: Option<Notice>,
    pub picking: Option<Picking>,
    /// A snapped shape is shown, and whether its handles show (it is being edited).
    pub shape: Option<QuickShapeInfo>,
    pub shape_editing: bool,
    /// The brush's outline where the pen paints (points): its radius, and the brush's outline
    /// setting (none, contrasting rings, or the colour).
    cursor: Option<(Pos2, f32, i32)>,
}

impl Overlays {
    /// Shows `text` in the band under the top bar, in place of any message there.
    pub fn notice(&mut self, text: impl Into<String>, warning: bool) {
        self.notice = Some(Notice { text: text.into(), since: Instant::now(), warning });
    }

    /// The brush's outline goes to (x, y), window pixels (sized and styled as the stroke begins),
    /// or away.
    pub fn cursor(&mut self, ctx: &egui::Context, session: &CanvasSession, at: Option<(f32, f32)>) {
        let ppp = ctx.pixels_per_point();
        self.cursor = at.map(|(x, y)| {
            let (r, outline) = self.cursor.map(|(_, r, o)| (r, o)).unwrap_or_else(|| {
                let info = session.info();
                (
                    info.brush_diameter * info.zoom / 2.0 / ppp,
                    session.current_brush_setting("cursor_outline".into()).round() as i32,
                )
            });
            (pos2(x, y) / ppp, r, outline)
        });
    }

    /// The eyedropper reads under (x, y), window pixels.
    pub fn pick(&mut self, ctx: &egui::Context, session: &CanvasSession, x: f32, y: f32) {
        let side = ((crate::input::PICK_WINDOW * ctx.pixels_per_point()) as u32) | 1;
        let seen: Option<Pick> = session.pick(x, y, side);
        let image = seen.as_ref().map(|p| {
            let n = p.side as usize;
            let rgba = p.pixels.get(..n * n * 4).unwrap_or(&[]);
            let image = if rgba.len() == n * n * 4 {
                egui::ColorImage::from_rgba_unmultiplied([n, n], rgba)
            } else {
                egui::ColorImage::filled([1, 1], Color32::TRANSPARENT)
            };
            ctx.load_texture("eyedropper", image, TextureOptions::NEAREST)
        });
        let rgb = seen.and_then(|p| p.rgb);
        let ppp = ctx.pixels_per_point();
        let kept = self.picking.take();
        self.picking = Some(Picking {
            at: pos2(x, y) / ppp,
            image: image.or_else(|| kept.as_ref().and_then(|k| k.image.clone())),
            rgb: rgb.or_else(|| kept.and_then(|k| k.rgb)),
        });
    }

    /// Draws them; `bar_shown` when the interface shows (the bands hang under the top bar).
    /// Returns a shape kind chosen on the shape's bar, if any.
    pub fn show(&mut self, ui: &Ui, look: &mut Look, current: Color32, bar_shown: bool) -> Option<QuickShapeKind> {
        let ctx = ui.ctx().clone();
        let window = ctx.content_rect();
        let under_bar = window.top() + if bar_shown { pc::BAR_HEIGHT } else { 0.0 };
        let mut chosen = None;
        egui::Area::new(egui::Id::unique("canvas overlays"))
            .fixed_pos(window.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(&ctx, |ui| {
                ui.set_clip_rect(window);
                // in the Android app's order: the eyedropper, the notice band over its band, the
                // shape's bar over the notice
                if let Some(p) = &self.picking {
                    loupe(ui, p, current);
                    if bar_shown {
                        let band = Rect::from_min_size(pos2(window.left(), under_bar), vec2(window.width(), 24.0));
                        ui.painter().rect_filled(band, 0, argb(MESSAGE_BAR));
                        let ink = argb(MESSAGE_TEXT);
                        text(
                            ui.painter(),
                            band.center(),
                            Align2::CENTER_CENTER,
                            "Picking color from canvas",
                            14.0,
                            Weight::Regular,
                            ink,
                        );
                    }
                }
                self.notice_band(ui, under_bar, window);
                // the brush's outline where the pen paints: a thin dark ring with a light edge, or
                // the colour, as the brush says
                if let Some((at, r, outline)) = self.cursor.filter(|(_, r, _)| *r >= 2.0) {
                    match outline {
                        0 => {}
                        2 => {
                            ui.painter().circle_stroke(at, r, egui::Stroke::new(3.0, Color32::from_white_alpha(0x99)));
                            ui.painter().circle_stroke(at, r, egui::Stroke::new(1.5, current));
                        }
                        _ => {
                            ui.painter().circle_stroke(at, r, egui::Stroke::new(3.0, Color32::WHITE));
                            ui.painter().circle_stroke(at, r, egui::Stroke::new(1.0, Color32::BLACK));
                        }
                    }
                }
                if let (Some(info), true, true) = (&self.shape, bar_shown, self.picking.is_none()) {
                    chosen = self.shape_bar(ui, look, info.clone(), under_bar, window);
                }
                if self.shape_editing
                    && let Some(info) = &self.shape
                {
                    // the snapped shape's handles: blue dots ringed in white
                    let ppp = ctx.pixels_per_point();
                    for &[x, y] in info.handles.as_chunks::<2>().0 {
                        let at = pos2(x, y) / ppp;
                        ui.painter().circle_filled(at, 6.5, Color32::WHITE);
                        ui.painter().circle_filled(at, 5.0, argb(pc::ACCENT));
                    }
                }
            });
        if self.notice.is_some() || self.picking.is_some() {
            ctx.request_repaint();
        }
        chosen
    }

    /// The notice band: it slides down from under the bar, stays a moment, and slides back.
    fn notice_band(&mut self, ui: &Ui, top: f32, window: Rect) {
        let Some(n) = &self.notice else { return };
        let t = n.since.elapsed().as_secs_f32();
        let stay = if n.warning { NOTICE_WARNING_SHOWN } else { NOTICE_SHOWN }.as_secs_f32();
        let shown = if t < NOTICE_IN {
            ease(t / NOTICE_IN)
        } else if t < NOTICE_IN + stay {
            1.0
        } else {
            1.0 - ease((t - NOTICE_IN - stay) / NOTICE_IN)
        };
        if t > 2.0 * NOTICE_IN + stay {
            self.notice = None;
            return;
        }
        let band = Rect::from_min_size(pos2(window.left(), top - (1.0 - shown) * 24.0), vec2(window.width(), 24.0));
        let painter = ui.painter().with_clip_rect(Rect::from_min_max(pos2(window.left(), top), window.max));
        painter.rect_filled(band, 0, argb(NOTICE_FILL));
        text(&painter, band.center(), Align2::CENTER_CENTER, &n.text, 13.0, Weight::Regular, argb(NOTICE_INK));
    }

    /// The bar a snapped stroke shows: the shape's name (and, while its handles show, "Editing"
    /// and the kind in grey); a tap shows the handles, and the kinds it can be shown as below.
    fn shape_bar(
        &mut self,
        ui: &Ui,
        look: &mut Look,
        info: QuickShapeInfo,
        top: f32,
        window: Rect,
    ) -> Option<QuickShapeKind> {
        let bar = Rect::from_min_size(pos2(window.left(), top), vec2(window.width(), 32.0));
        ui.painter().rect_filled(bar, 0, argb(MODE_BAR));
        if ui.interact(bar, ui.scope_id().with("shape bar"), Sense::click()).clicked() {
            self.shape_editing = true;
        }
        let ink = argb(MODE_TEXT);
        let several = info.choices.len() > 1;
        let name = if self.shape_editing { "Editing" } else { label(info.kind) };
        // laid out from the middle: the icon, the name, the kind in grey and the disclosure
        let painter = ui.painter();
        let measure = |s: &str, size: f32, w: Weight| painter.layout_no_wrap(s.into(), w.font(size), ink).size().x;
        let mut width = 29.0 + measure(name, 17.0, Weight::Bold);
        if self.shape_editing && several {
            width += 3.0 + measure(label(info.kind), 15.0, Weight::Regular);
        }
        if several {
            width += 33.0;
        }
        let mut x = bar.center().x - width / 2.0;
        look.icon(ui, icon(info.kind), pos2(x + 14.5, bar.center().y), 28.0, ink);
        x += 29.0;
        x = text(ui.painter(), pos2(x, bar.center().y), Align2::LEFT_CENTER, name, 17.0, Weight::Bold, ink).right();
        if self.shape_editing && several {
            x = text(
                ui.painter(),
                pos2(x + 3.0, bar.center().y),
                Align2::LEFT_CENTER,
                label(info.kind),
                15.0,
                Weight::Regular,
                Color32::from_gray(0x9A),
            )
            .right();
        }
        if several {
            look.icon(ui, &icons::DISCLOSE, pos2(x + 16.5, bar.center().y), 32.0, ink);
        }
        if !(self.shape_editing && several) {
            return None;
        }
        // the kinds it can be shown as, the chosen one blue
        let each = if info.choices.len() > 2 { 154.0 } else { 116.0 };
        let row_w = each * info.choices.len() as f32 + 2.0 * (info.choices.len() as f32 + 1.0);
        let row = Rect::from_center_size(pos2(bar.center().x, bar.bottom() + 20.0 + 16.0), vec2(row_w, 32.0));
        ui.painter().rect_filled(row, 16, argb(MODES_FILL));
        let mut chosen = None;
        for (i, kind) in info.choices.iter().enumerate() {
            let cell = Rect::from_min_size(
                pos2(row.left() + 2.0 + (each + 2.0) * i as f32, row.top() + 2.0),
                vec2(each, 28.0),
            );
            if *kind == info.kind {
                ui.painter().rect_filled(cell, 14, argb(pc::ACCENT));
            }
            let words =
                ui.painter().layout_no_wrap(label(*kind).into(), Weight::Regular.font(14.0), Color32::WHITE).size().x;
            let start = cell.center().x - (28.0 + 6.0 + words) / 2.0;
            look.icon(ui, icon(*kind), pos2(start + 14.0, cell.center().y), 28.0, Color32::WHITE);
            text(
                ui.painter(),
                pos2(start + 34.0, cell.center().y),
                Align2::LEFT_CENTER,
                label(*kind),
                14.0,
                Weight::Regular,
                Color32::WHITE,
            );
            if ui.interact(cell, ui.scope_id().with(("shape kind", i)), Sense::click()).clicked() {
                chosen = Some(*kind);
            }
        }
        chosen
    }
}

/// The eyedropper's loupe, centred `LOUPE_LIFT` above the pointer: a ring whose top half is the
/// colour under it and bottom half the current colour, round what is shown there at twice the size.
fn loupe(ui: &Ui, p: &Picking, current: Color32) {
    let c = p.at - vec2(0.0, LOUPE_LIFT);
    let painter = ui.painter();
    let shadow = egui::epaint::Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(0x40) };
    painter.add(shadow.as_shape(Rect::from_center_size(c, vec2(LOUPE_OUTER * 2.0, LOUPE_OUTER * 2.0)), LOUPE_OUTER));
    let picked = p.rgb.map_or(current, |v| {
        let [_, r, g, b] = v.to_be_bytes();
        Color32::from_rgb(r, g, b)
    });
    let mid = (LOUPE_OUTER + LOUPE_INNER) / 2.0;
    let width = LOUPE_OUTER - LOUPE_INNER;
    half_ring(ui, c, mid, width, picked, true);
    half_ring(ui, c, mid, width, current, false);
    if let Some(t) = &p.image {
        // what is under it, at twice the size, clipped round
        let mut mesh = egui::Mesh::with_texture(t.id());
        let steps = 48;
        mesh.vertices.push(egui::epaint::Vertex { pos: c, uv: pos2(0.5, 0.5), color: Color32::WHITE });
        for k in 0..=steps {
            let a = k as f32 / steps as f32 * std::f32::consts::TAU;
            let (s, co) = a.sin_cos();
            let at = c + vec2(co, s) * LOUPE_INNER;
            mesh.vertices.push(egui::epaint::Vertex {
                pos: at,
                uv: pos2(0.5 + co * 0.5, 0.5 + s * 0.5),
                color: Color32::WHITE,
            });
            if k > 0 {
                mesh.add_triangle(0, k, k + 1);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    let edge = egui::Stroke::new(0.5, argb(LOUPE_EDGE));
    painter.circle_stroke(c, LOUPE_OUTER, edge);
    painter.circle_stroke(c, LOUPE_INNER, edge);
    // the crosshair
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let (w, h) = if dx != 0.0 { (2.0, 1.0) } else { (1.0, 2.0) };
        let at = c + vec2(dx * (1.75 + 1.0), dy * (1.75 + 1.0));
        painter.rect_filled(Rect::from_center_size(at, vec2(w, h)), 0, Color32::WHITE);
    }
}

/// Half a ring round `c`: the top half, or the bottom.
fn half_ring(ui: &Ui, c: Pos2, mid: f32, width: f32, colour: Color32, top: bool) {
    let steps = 40;
    let points: Vec<Pos2> = (0..=steps)
        .map(|k| {
            let a = std::f32::consts::PI * k as f32 / steps as f32 + if top { std::f32::consts::PI } else { 0.0 };
            c + vec2(a.cos(), a.sin()) * mid
        })
        .collect();
    ui.painter().add(egui::Shape::line(points, egui::Stroke::new(width, colour)));
}

pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // the cubic ease in and out (0.42, 0, 0.58, 1), near enough
    t * t * (3.0 - 2.0 * t)
}

pub fn label(kind: QuickShapeKind) -> &'static str {
    match kind {
        QuickShapeKind::Line => "Line",
        QuickShapeKind::Polyline => "Polyline",
        QuickShapeKind::Arc => "Arc",
        QuickShapeKind::Ellipse => "Ellipse",
        QuickShapeKind::Circle => "Circle",
        QuickShapeKind::Triangle => "Triangle",
        QuickShapeKind::EquilateralTriangle => "Equilateral",
        QuickShapeKind::Quadrilateral => "Quadrilateral",
        QuickShapeKind::Rectangle => "Rectangle",
        QuickShapeKind::Square => "Square",
    }
}

/// What a stroke snapped to, for the notice band.
pub fn snapped_to(kind: QuickShapeKind) -> String {
    let what = match kind {
        QuickShapeKind::Line => "a line",
        QuickShapeKind::Polyline => "a polyline",
        QuickShapeKind::Arc => "an arc",
        QuickShapeKind::Ellipse => "an ellipse",
        QuickShapeKind::Circle => "a circle",
        QuickShapeKind::Triangle => "a triangle",
        QuickShapeKind::EquilateralTriangle => "an equilateral triangle",
        QuickShapeKind::Quadrilateral => "a quadrilateral",
        QuickShapeKind::Rectangle => "a rectangle",
        QuickShapeKind::Square => "a square",
    };
    format!("Snapped to {what}")
}

fn icon(kind: QuickShapeKind) -> &'static icons::Icon {
    match kind {
        QuickShapeKind::Line => &icons::SHAPE_LINE,
        QuickShapeKind::Polyline => &icons::SHAPE_POLYLINE,
        QuickShapeKind::Arc => &icons::SHAPE_ARC,
        QuickShapeKind::Ellipse => &icons::SHAPE_ELLIPSE,
        QuickShapeKind::Circle => &icons::SHAPE_CIRCLE,
        QuickShapeKind::Triangle => &icons::SHAPE_TRIANGLE,
        QuickShapeKind::EquilateralTriangle => &icons::SHAPE_EQUILATERAL,
        QuickShapeKind::Quadrilateral => &icons::SHAPE_QUAD,
        QuickShapeKind::Rectangle => &icons::SHAPE_RECT,
        QuickShapeKind::Square => &icons::SHAPE_SQUARE,
    }
}
