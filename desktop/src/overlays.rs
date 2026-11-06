//! What shows over the canvas besides the bars and panels, as the Android app has it
//! (ui/AppRoot.kt, ui/Eyedropper.kt, ui/AdjustmentsUi.kt): the notice band under the top bar
//! ("Undo Brush stroke"), the eyedropper's band and loupe, and the bar of a snapped shape.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};
use procraft_core::{CanvasSession, Pick, QuickShapeAnchor, QuickShapeInfo, QuickShapeKind};

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::prefs::CursorStyle;

const NOTICE_FILL: u32 = 0xC2424242;
const NOTICE_INK: u32 = 0xFFD7D7D7;
const NOTICE_IN: f32 = 0.3;
const NOTICE_SHOWN: Duration = Duration::from_millis(750);
const NOTICE_WARNING_SHOWN: Duration = Duration::from_millis(1500);
const MESSAGE_BAR: Themed = Themed(0xFF606060, 0xFFD0D0D0);
const MESSAGE_TEXT: Themed = Themed(0xFFB5B5B5, 0xFF3B3B3B);
const LOUPE_OUTER: f32 = 69.0;
const LOUPE_INNER: f32 = 53.5;
const LOUPE_LIFT: f32 = 41.0;
const LOUPE_EDGE: u32 = 0xFF808080;
const MODE_BAR: Themed = Themed(0xD63B3B3B, 0xD6EEEEEE);
const MODE_TEXT: u32 = 0xFFC5C5C5;
const MODES_FILL: Themed = Themed(0xFF7B7B7B, 0xFFFFFFFF);

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
    /// On the reference window's picture, not the canvas.
    from_reference: bool,
}

pub struct Overlays {
    notice: Option<Notice>,
    /// Where the notice band shows this frame, to frost the canvas under it (as measured).
    pub frost: Option<Rect>,
    pub picking: Option<Picking>,
    /// The loupe's picture as the eyedropper let go, kept a frame (it may have been drawn in this
    /// one, and a texture let go of is gone before the frame is).
    spent: Option<TextureHandle>,
    /// A snapped shape is shown, and whether its handles show (it is being edited).
    pub shape: Option<QuickShapeInfo>,
    pub shape_editing: bool,
    /// The brush's outline where the pen paints (points): its radius, and the brush's outline
    /// setting (none, contrasting rings, or the colour).
    cursor: Option<(Pos2, f32, i32)>,
    /// The brush's own shape for it, loops of x, y in brush diameters (none: the round tip's
    /// circle).
    cursor_shape: Vec<Vec<f32>>,
    /// How much more the shape is turned on the screen than its own rotation, radians clockwise,
    /// as the stamps are: with the canvas, and as a pen that the brush follows leans.
    cursor_turn: f32,
    /// How the cursor is drawn (Advanced cursor settings).
    pub cursor_style: CursorStyle,
}

impl Default for Overlays {
    fn default() -> Overlays {
        Overlays {
            notice: None,
            frost: None,
            picking: None,
            spent: None,
            shape: None,
            shape_editing: false,
            cursor: None,
            cursor_shape: Vec::new(),
            cursor_turn: 0.0,
            cursor_style: CursorStyle::Brush,
        }
    }
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
        if self.cursor.is_none() && at.is_some() {
            self.cursor_shape = session.brush_outline();
        }
        // turned as the pen turns, through the stroke
        self.cursor_turn = cursor_turn(session);
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

    /// The brush's outline goes to `at` (points), where the pointer hovers, sized for the brush
    /// as it is now (`info`); or away.
    pub fn hover(&mut self, session: &CanvasSession, info: &procraft_core::CanvasInfo, ppp: f32, at: Option<Pos2>) {
        if at.is_some() {
            // the core keeps the brush's outline: asked again, it is handed back as it was
            self.cursor_shape = session.brush_outline();
            self.cursor_turn = cursor_turn(session);
        }
        self.cursor = at.map(|at| {
            let outline = session.current_brush_setting("cursor_outline".into()).round() as i32;
            (at, info.brush_diameter * info.zoom / 2.0 / ppp, outline)
        });
    }

    /// The brush's outline as it shows this frame: where, its radius, and how it is drawn (1
    /// contrasting rings, 2 the colour); none without one, or with one too small to see.
    fn outline(&self) -> Option<(Pos2, f32, i32)> {
        let (at, r, outline) = self.cursor.filter(|(_, r, _)| *r >= 2.0)?;
        // the style chosen, or as the brush says: none, contrasting rings, or the colour
        let outline = match self.cursor_style {
            CursorStyle::Contrast => 1,
            CursorStyle::Colour => 2,
            CursorStyle::Brush => outline,
        };
        (outline != 0).then_some((at, r, outline))
    }

    /// Whether the brush's outline shows this frame (in place of the system's pointer).
    pub fn outline_shown(&self) -> bool {
        self.outline().is_some()
    }

    /// The eyedropper lets go: the colour it picks, if it read one.
    pub fn finish_pick(&mut self) -> Option<u32> {
        let p = self.picking.take()?;
        self.spent = p.image;
        p.rgb
    }

    /// The eyedropper reads under (x, y), window pixels.
    pub fn pick(&mut self, ctx: &egui::Context, session: &CanvasSession, x: f32, y: f32) {
        let side = ((crate::input::PICK_WINDOW * ctx.pixels_per_point()) as u32) | 1;
        let seen: Option<Pick> = session.pick(x, y, side);
        let kept = self.picking.take();
        let image = match seen.as_ref() {
            Some(p) => Some(loupe_texture(ctx, kept.as_ref(), p.side as usize, &p.pixels)),
            None => kept.as_ref().and_then(|k| k.image.clone()),
        };
        let rgb = seen.and_then(|p| p.rgb);
        let ppp = ctx.pixels_per_point();
        self.picking = Some(Picking {
            at: pos2(x, y) / ppp,
            image,
            rgb: rgb.or_else(|| kept.and_then(|k| k.rgb)),
            from_reference: false,
        });
    }

    /// The eyedropper on the reference window's picture, at window point `at` (points), over
    /// `rgb`, the `side` x `side` pixels (RGBA) around it shown.
    pub fn pick_reference(&mut self, ctx: &egui::Context, at: Pos2, side: usize, rgba: &[u8], rgb: u32) {
        let image = Some(loupe_texture(ctx, self.picking.as_ref(), side, rgba));
        self.picking = Some(Picking { at, image, rgb: Some(rgb), from_reference: true });
    }

    /// Draws them; `bar_shown` when the interface shows (the bands hang under the top bar).
    /// Returns a shape kind chosen on the shape's bar, if any.
    pub fn show(&mut self, ui: &Ui, look: &mut Look, current: Color32, bar_shown: bool) -> Option<QuickShapeKind> {
        let ctx = ui.ctx().clone();
        let window = ctx.content_rect();
        let under_bar = window.top() + if bar_shown { pc::BAR_HEIGHT } else { 0.0 };
        let mut chosen = None;
        self.spent = None;
        self.frost = None;
        let shown = egui::Area::new(egui::Id::unique("canvas overlays"))
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
                            if p.from_reference { "Picking color from reference" } else { "Picking color from canvas" },
                            14.0,
                            Weight::Regular,
                            ink,
                        );
                    }
                }
                self.notice_band(ui, under_bar, window);
                // the brush's outline where the pen paints: a thin dark ring with a light edge, or
                // the colour, as the brush says
                if let Some((at, r, outline)) = self.outline() {
                    // it stands in for the system's pointer (none drawn over it)
                    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                    let (under, over) = match outline {
                        2 => (egui::Stroke::new(3.0, Color32::from_white_alpha(0x99)), egui::Stroke::new(1.5, current)),
                        _ => (egui::Stroke::new(3.0, Color32::WHITE), egui::Stroke::new(1.0, Color32::BLACK)),
                    };
                    for stroke in [under, over] {
                        if self.cursor_shape.is_empty() {
                            ui.painter().circle_stroke(at, r, stroke);
                        }
                        // the brush's own shape, across its diameter, turned as the stamps are
                        let (sin, cos) = self.cursor_turn.sin_cos();
                        for ring in &self.cursor_shape {
                            let points: Vec<Pos2> = ring
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|&[x, y]| at + egui::vec2(x * cos - y * sin, x * sin + y * cos) * (2.0 * r))
                                .collect();
                            ui.painter().add(egui::Shape::closed_line(points, stroke));
                        }
                    }
                }
                if self.shape_editing
                    && let Some(info) = &self.shape
                {
                    // the snapped shape's handles: blue dots ringed in white
                    let ppp = ctx.pixels_per_point();
                    let points: Vec<Pos2> =
                        info.handles.as_chunks::<2>().0.iter().map(|&[x, y]| pos2(x, y) / ppp).collect();
                    // a curve's controls show what they pull on: a line each back to its anchor
                    let curve = info.kind == QuickShapeKind::Curve;
                    let control = Color32::from_rgb(96, 224, 120);
                    // a corner's controls are worked out from the anchors either side, so nothing is
                    // drawn for them and nothing there can be taken hold of
                    let folded = |i: usize| {
                        let owner = match i % 3 {
                            1 => i.checked_sub(1),
                            2 => i.checked_add(1),
                            _ => None,
                        };
                        curve
                            && owner.is_some_and(|a| matches!(info.anchors.get(a / 3), Some(QuickShapeAnchor::Corner)))
                    };
                    if curve {
                        for (i, &at) in points.iter().enumerate() {
                            if folded(i) {
                                continue;
                            }
                            let about = match i % 3 {
                                1 => i.checked_sub(1),
                                2 => i.checked_add(1),
                                _ => None,
                            };
                            if let Some(anchor) = about.and_then(|a| points.get(a)) {
                                ui.painter().line_segment([*anchor, at], egui::Stroke::new(1.5, control));
                            }
                        }
                    }
                    for (i, &at) in points.iter().enumerate() {
                        if folded(i) {
                            continue;
                        }
                        let ink = if curve && !i.is_multiple_of(3) { control } else { argb(pc::ACCENT) };
                        // an anchor says how it sits with its shape: round, free, or a corner
                        match if curve && i.is_multiple_of(3) { info.anchors.get(i / 3).copied() } else { None } {
                            Some(QuickShapeAnchor::Broken) => {
                                let r = 7.5;
                                let corners = vec![
                                    pos2(at.x, at.y - r),
                                    pos2(at.x - r * 0.87, at.y + r * 0.5),
                                    pos2(at.x + r * 0.87, at.y + r * 0.5),
                                ];
                                ui.painter().add(egui::Shape::convex_polygon(
                                    corners,
                                    ink,
                                    egui::Stroke::new(1.5, Color32::WHITE),
                                ));
                            }
                            Some(QuickShapeAnchor::Corner) => {
                                ui.painter().rect_filled(
                                    egui::Rect::from_center_size(at, vec2(13.0, 13.0)),
                                    1.0,
                                    Color32::WHITE,
                                );
                                ui.painter().rect_filled(egui::Rect::from_center_size(at, vec2(10.0, 10.0)), 1.0, ink);
                            }
                            _ => {
                                ui.painter().circle_filled(at, 6.5, Color32::WHITE);
                                ui.painter().circle_filled(at, 5.0, ink);
                            }
                        }
                    }
                }
            });
        if let (Some(info), true, true) = (self.shape.clone(), bar_shown, self.picking.is_none()) {
            chosen = self.shape_bar(&ctx, look, info, under_bar, window);
        }
        if self.notice.is_some() || self.picking.is_some() {
            ctx.request_repaint();
        }
        // the loupe over a window it picks from (one pressed comes to the top)
        if self.picking.is_some() {
            ctx.move_to_top(shown.response.layer_id);
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
        let below = Rect::from_min_max(pos2(window.left(), top), window.max);
        self.frost = Some(band.intersect(below)).filter(|r| r.height() > 0.0);
        let painter = ui.painter().with_clip_rect(below);
        painter.rect_filled(band, 0, argb(NOTICE_FILL));
        text(&painter, band.center(), Align2::CENTER_CENTER, &n.text, 13.0, Weight::Regular, argb(NOTICE_INK));
    }

    /// The bar a snapped stroke shows: the shape's name (and, while its handles show, "Editing"
    /// and the kind in grey); a tap shows the handles, and the kinds it can be shown as below.
    /// Each is an area of its own, as big as it shows: a press there is the interface's, where it
    /// would otherwise reach the canvas, which lets the shape go.
    fn shape_bar(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        info: QuickShapeInfo,
        top: f32,
        window: Rect,
    ) -> Option<QuickShapeKind> {
        let bar = Rect::from_min_size(pos2(window.left(), top), vec2(window.width(), 32.0));
        let several = info.choices.len() > 1;
        strip_area("shape bar", bar, ctx, |ui| {
            ui.painter().rect_filled(bar, 0, argb(MODE_BAR));
            if ui.allocate_rect(bar, Sense::click()).clicked() {
                self.shape_editing = true;
            }
            let ink = argb(MODE_TEXT);
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
        });
        if !(self.shape_editing && several) {
            return None;
        }
        // the kinds it can be shown as, the chosen one blue
        let each = if info.choices.len() > 2 { 154.0 } else { 116.0 };
        let row_w = each * info.choices.len() as f32 + 2.0 * (info.choices.len() as f32 + 1.0);
        let row = Rect::from_center_size(pos2(bar.center().x, bar.bottom() + 20.0 + 16.0), vec2(row_w, 32.0));
        let mut chosen = None;
        strip_area("shape kinds", row, ctx, |ui| {
            ui.allocate_rect(row, Sense::hover());
            ui.painter().rect_filled(row, 16, argb(MODES_FILL));
            for (i, kind) in info.choices.iter().enumerate() {
                let cell = Rect::from_min_size(
                    pos2(row.left() + 2.0 + (each + 2.0) * i as f32, row.top() + 2.0),
                    vec2(each, 28.0),
                );
                if *kind == info.kind {
                    ui.painter().rect_filled(cell, 14, argb(pc::ACCENT));
                }
                let words = ui
                    .painter()
                    .layout_no_wrap(label(*kind).into(), Weight::Regular.font(14.0), Color32::WHITE)
                    .size()
                    .x;
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
        });
        chosen
    }
}

/// An area over the canvas exactly `rect` big, for the bars that hang under the top bar.
fn strip_area(name: &'static str, rect: Rect, ctx: &egui::Context, show: impl FnOnce(&mut Ui)) {
    egui::Area::new(egui::Id::unique(name))
        .fixed_pos(rect.min)
        .order(egui::Order::Middle)
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect);
            show(ui);
        });
}

/// The eyedropper's loupe, centred `LOUPE_LIFT` above the pointer: a ring whose top half is the
/// colour under it and bottom half the current colour, round what is shown there at twice the size.
/// What the loupe shows, the `side` x `side` pixels (RGBA) `rgba`: in the texture it showed
/// before, if there was one (a new texture each move frees the one still being drawn).
fn loupe_texture(ctx: &egui::Context, before: Option<&Picking>, side: usize, rgba: &[u8]) -> TextureHandle {
    let image = match rgba.get(..side * side * 4) {
        Some(rgba) if side > 0 => egui::ColorImage::from_rgba_unmultiplied([side, side], rgba),
        _ => egui::ColorImage::filled([1, 1], Color32::TRANSPARENT),
    };
    match before.and_then(|p| p.image.clone()) {
        Some(mut texture) => {
            texture.set(image, TextureOptions::NEAREST);
            texture
        }
        None => ctx.load_texture("eyedropper", image, TextureOptions::NEAREST),
    }
}

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
        QuickShapeKind::Curve => "Curve",
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
        QuickShapeKind::Curve => "a curve",
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
        // no curve glyph drawn yet: a bend reads close enough for the pill
        QuickShapeKind::Curve => &icons::SHAPE_ARC,
        QuickShapeKind::Ellipse => &icons::SHAPE_ELLIPSE,
        QuickShapeKind::Circle => &icons::SHAPE_CIRCLE,
        QuickShapeKind::Triangle => &icons::SHAPE_TRIANGLE,
        QuickShapeKind::EquilateralTriangle => &icons::SHAPE_EQUILATERAL,
        QuickShapeKind::Quadrilateral => &icons::SHAPE_QUAD,
        QuickShapeKind::Rectangle => &icons::SHAPE_RECT,
        QuickShapeKind::Square => &icons::SHAPE_SQUARE,
    }
}

/// How much more the brush's stamps turn on the screen than its own rotation (radians clockwise),
/// as the strokes turn them: with the canvas, unless the brush is oriented to the screen; and, for a
/// brush that follows the pen, as the pen near the window leans (its azimuth). The barrel's roll
/// turns the stamps from where it was as a stroke begins, so a stroke's first stamp is as the
/// outline shows it however the barrel is turned.
fn cursor_turn(session: &CanvasSession) -> f32 {
    let setting = |key: &str| session.current_brush_setting(key.into());
    let mut turn = 0.0;
    if setting("orient_to_screen") < 0.5 {
        let v = session.view_matrix();
        turn += v.get(1).copied().unwrap_or(0.0).atan2(v.first().copied().unwrap_or(1.0));
    }
    if let Some(azimuth) = crate::pen::azimuth().filter(|_| setting("input_style") >= 0.5) {
        turn += azimuth;
    }
    // turned by the barrel from its zero, as the stamps will be (relative to the stroke, the first
    // stamp isn't)
    if let Some(barrel) =
        crate::pen::barrel().filter(|_| setting("input_style") >= 1.5 && setting("relative_to_stroke") < 0.5)
    {
        turn += barrel;
    }
    turn
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers, PointerButton, RawInput};

    use super::*;

    /// The overlays over a window with a snapped shape, its bar shown.
    struct Shown {
        ctx: egui::Context,
        look: Look,
        overlays: Overlays,
        time: f64,
    }

    impl Shown {
        fn new() -> Shown {
            let overlays = Overlays {
                shape: Some(QuickShapeInfo {
                    kind: QuickShapeKind::Ellipse,
                    choices: vec![QuickShapeKind::Ellipse, QuickShapeKind::Circle],
                    handles: Vec::new(),
                    anchors: Vec::new(),
                }),
                ..Overlays::default()
            };
            let mut shown = Shown { ctx: egui::Context::default(), look: Look::default(), overlays, time: 0.0 };
            shown.frame(Vec::new());
            shown.frame(Vec::new());
            shown
        }

        fn frame(&mut self, events: Vec<Event>) -> Option<QuickShapeKind> {
            self.time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0))),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let Shown { ctx, look, overlays, .. } = self;
            let mut chosen = None;
            let mut output = ctx.clone().run_ui(raw, |ui| {
                chosen = overlays.show(ui, look, Color32::WHITE, true);
            });
            output.textures_delta.clear();
            chosen
        }

        /// A press and a lift at `at`, as they reach the interface.
        fn tap(&mut self, at: Pos2) -> Option<QuickShapeKind> {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            let chosen = self.frame(vec![button(false)]);
            self.frame(Vec::new());
            chosen
        }

        /// Whether a press at `at` is the interface's, as the window's events go by.
        fn interface_at(&self, at: Pos2) -> bool {
            self.ctx.layer_id_at(at).is_some_and(|l| l.order != egui::Order::Background)
        }
    }

    #[test]
    fn a_press_on_a_snapped_shapes_bar_or_kinds_stays_off_the_canvas() {
        let mut s = Shown::new();
        let bar = pos2(600.0, pc::BAR_HEIGHT + 16.0);
        let kinds = pos2(600.0 + 60.0, pc::BAR_HEIGHT + 32.0 + 36.0);
        assert!(s.interface_at(bar), "a press on the bar reached the canvas, which lets the shape go");
        assert!(!s.interface_at(kinds), "no kinds yet");
        s.tap(bar);
        assert!(s.overlays.shape_editing, "the bar's tap didn't show the handles");
        assert!(s.interface_at(kinds), "a press on the kinds reached the canvas");
        // the second kind, right of the middle
        assert_eq!(s.tap(kinds), Some(QuickShapeKind::Circle));
        // the canvas around them stays the canvas's
        assert!(!s.interface_at(pos2(600.0, 400.0)));
        assert!(!s.interface_at(pos2(100.0, pc::BAR_HEIGHT + 32.0 + 36.0)));
    }
}
