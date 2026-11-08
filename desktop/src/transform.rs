//! Transform, as the Android app's (ui/TransformUi.kt): the box over what moves (its dashed edges,
//! its nodes, the rotate node above it and the box-turning node below it; a warp's lines and
//! control points), and the floating toolbar at the foot of the window (Freeform, Uniform,
//! Distort and Warp; Snapping or Advanced Mesh, the flips, Rotate 45°, Fit to Canvas,
//! Interpolation, Reset) with its popovers, and a node's numbers typed in (transform_numeric.rs).
//! The shared touches work the box.

use std::time::Instant;

use egui::epaint::PathShape;
use egui::{Align2, Color32, Id, LayerId, Order, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use procraft_core::{
    CanvasSession, GuideSettings, Interpolation, TransformAction, TransformBox, TransformHandle, TransformKind,
    TransformTouch,
};

use crate::icons::{self, Icon, Part};
use crate::look::{Look, Themed, Weight, argb, mix, pc, text};
use crate::transform_numeric::Numeric;

const NODE_BLUE: u32 = 0xFF0171EA;
const ROTATE_GREEN: u32 = 0xFF00E607;
const ROTATE_LINE: u32 = 0xFF00E400;
const TURN_ORANGE: u32 = 0xFFFEB800;
const TURN_LINE: u32 = 0xFFFEBF00;
pub(crate) const DASH: u32 = 0xE6000000;
pub(crate) const DASH_GAP: u32 = 0xE6FFFFFF;
const NODE_SHADOW: u32 = 0x1F000000;
const MESH_LINE: u32 = 0xFF8B8B8B;
/// The lines a moved box snapped onto, across the canvas (dark and light interface).
const SNAP_GUIDE: Themed = Themed(0xFFFF9F0A, 0xFFFF9500);
/// A held node is drawn this much larger; a let-go rotate or box-turning node springs back over
/// this long.
const HELD_SCALE: f32 = 1.5;
const HANDLE_RETURN: f32 = 0.5;

const TOOLBAR_WIDTH: f32 = 728.0;
const TOOLBAR_HEIGHT: f32 = 126.0;
const TOOLBAR_BOTTOM: f32 = 25.0;
const TOOLBAR_CORNER: f32 = 33.0;
const TOOLBAR_FILL: Themed = Themed(0xF71E1E1E, 0xF7F2F2F2);
const SEGMENT_TRACK: Themed = Themed(0xFF535353, 0xFFFFFFFF);
const SEGMENT_EDGE: Themed = Themed(0xFF7E7E7E, 0xFFEBEBEB);
const SEGMENT_TEXT: Themed = Themed(0xFFC1C1C1, 0xFF3B3B3B);
const BUTTON_TEXT: Themed = Themed(0xFFA3A3A3, 0xFF414141);
const BUTTON_DIMMED: Themed = Themed(0xFF575757, 0xFF989898);
const SEGMENT_GAP: f32 = 11.33;
/// The toolbar's buttons with room, and where the window is narrow; the gap between roomy ones.
pub(crate) const BUTTON_ROOMY: f32 = 89.0;
pub(crate) const BUTTON_WIDTH: f32 = 83.0;
const BUTTON_HEIGHT: f32 = 60.0;
const BUTTON_GAP: f32 = 19.0;
/// The popovers over the toolbar.
pub(crate) const MENU_WIDTH: f32 = 260.0;
pub(crate) const MENU_ARROW: f32 = 15.0;
const MENU_CORNER: f32 = 26.0;
const MENU_FILL: Themed = Themed(0xEB2A2A2A, 0xEBF0F0F0);
pub(crate) const MENU_GROUP: Themed = Themed(0xFF313131, 0xFFFFFFFF);
pub(crate) const MENU_LINE: Themed = Themed(0xFF373737, 0xFFE2E2E2);
pub(crate) const MENU_TEXT: Themed = Themed(0xFFC5C5C5, 0xFF393939);
const SLIDER_REST: Themed = Themed(0xFF5A5A5A, 0xFFDDDDDD);
const SWITCH_OFF: Themed = Themed(0xFF747377, 0xFFA9A8AC);
/// Snapping's distance (points) and velocity, as their sliders run.
const SNAP_DISTANCE: (f32, f32, f32) = (1.0, 50.0, 3.0);
const SNAP_VELOCITY_MAX: f32 = 10.0;

/// Advanced Mesh's icon: cells four across, three down, with thin gaps, as the Android app draws it.
const ADVANCED_MESH: Icon = Icon {
    name: "advanced-mesh",
    width: 24.0,
    height: 24.0,
    parts: &[Part {
        d: "M1.5 4.9h4.425v4.033h-4.425zM7.125 4.9h4.425v4.033h-4.425zM12.75 4.9h4.425v4.033h-4.425zM18.375 4.9h4.425v4.033h-4.425zM1.5 10.133h4.425v4.033h-4.425zM7.125 10.133h4.425v4.033h-4.425zM12.75 10.133h4.425v4.033h-4.425zM18.375 10.133h4.425v4.033h-4.425zM1.5 15.366h4.425v4.033h-4.425zM7.125 15.366h4.425v4.033h-4.425zM12.75 15.366h4.425v4.033h-4.425zM18.375 15.366h4.425v4.033h-4.425z",
        stroke: 0.6,
        fill: true,
        even_odd: false,
    }],
};

/// The popover open over the toolbar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Popup {
    None,
    Snapping,
    Interpolation,
}

/// What a transform's toolbar asked.
enum Asked {
    Kind(TransformKind),
    Action(TransformAction),
    Popup(Popup),
    AdvancedMesh,
    Background,
}

pub struct Transform {
    /// The transform is open, and its box as it now is.
    pub on: bool,
    shown: Option<TransformBox>,
    kind: TransformKind,
    interpolation: Interpolation,
    guides: GuideSettings,
    advanced: bool,
    popup: Popup,
    /// The node held and where (window pixels); a rotate or box-turning node let go, where, and when.
    held: Option<(TransformHandle, Pos2)>,
    let_go: Option<(TransformHandle, Pos2, Instant)>,
    /// A node's numbers being typed.
    numeric: Option<Numeric>,
}

impl Default for Transform {
    fn default() -> Transform {
        Transform {
            on: false,
            shown: None,
            kind: TransformKind::Freeform,
            interpolation: Interpolation::Bilinear,
            guides: GuideSettings::default(),
            advanced: false,
            popup: Popup::None,
            held: None,
            let_go: None,
            numeric: None,
        }
    }
}

impl Transform {
    /// Starts transforming the current layer (the caller has checked it takes changes). Returns
    /// what to say when there is nothing there to move.
    pub fn begin(&mut self, session: &CanvasSession) -> Result<(), String> {
        session.transform_set_interpolation(self.interpolation);
        session.transform_set_guides(self.guides);
        let Some(shown) = session.transform_begin() else { return Err(session.transform_refusal()) };
        // a warp lays its mesh over what began
        self.shown = session.transform_set_kind(self.kind).or(Some(shown));
        self.popup = Popup::None;
        self.on = true;
        session.set_touch_transforming(Some(TransformTouch { advanced_mesh: self.advanced }));
        Ok(())
    }

    /// Ends the transform: `commit` keeps the change as one undo step, else the layer goes back.
    pub fn end(&mut self, session: &CanvasSession, commit: bool) {
        if !self.on {
            return;
        }
        session.transform_end(commit);
        session.set_touch_transforming(None);
        self.on = false;
        self.shown = None;
        self.popup = Popup::None;
        self.held = None;
        self.let_go = None;
        self.numeric = None;
    }

    /// A touch came down on the box: its popover closes.
    pub fn touched(&mut self) {
        self.popup = Popup::None;
    }

    /// The box as it now is, if the touches say.
    pub fn moved(&mut self, shown: Option<TransformBox>) {
        if shown.is_some() {
            self.shown = shown;
        }
    }

    pub fn held(&mut self, handle: TransformHandle, at: Pos2) {
        self.held = Some((handle, at));
    }

    pub fn let_go(&mut self, handle: TransformHandle, at: Pos2) {
        self.held = None;
        if matches!(handle, TransformHandle::Rotate | TransformHandle::TurnBox) {
            self.let_go = Some((handle, at, Instant::now()));
        }
    }

    /// A node tapped: its numbers to type, the angle's for the rotate node, beside it at `at`
    /// (points), on the box's `right` side or its left.
    pub fn numbers(&mut self, angle: bool, at: Pos2, right: bool) {
        self.popup = Popup::None;
        self.numeric = Some(Numeric::new(angle, at, right, self.kind == TransformKind::Uniform));
    }

    /// Draws the box over the canvas and the toolbar at the foot of the window; does what the
    /// toolbar asks. A frosted panel's blur goes into `backdrops`.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, backdrops: &mut Vec<f32>) {
        if !self.on {
            return;
        }
        self.overlay(ctx, session);
        if let Some(asked) = self.toolbar(ctx, look) {
            match asked {
                Asked::Kind(kind) => {
                    self.kind = kind;
                    self.moved(session.transform_set_kind(kind));
                }
                Asked::Action(action) => {
                    self.popup = Popup::None;
                    self.moved(session.transform_apply(action));
                }
                Asked::Popup(p) => self.popup = if self.popup == p { Popup::None } else { p },
                Asked::AdvancedMesh => {
                    self.advanced = !self.advanced;
                    session.set_touch_transforming(Some(TransformTouch { advanced_mesh: self.advanced }));
                }
                Asked::Background => self.popup = Popup::None,
            }
        }
        // Escape closes the popover, as a touch on the canvas does
        if self.popup != Popup::None && crate::ui::escape(ctx) {
            self.popup = Popup::None;
        }
        self.popover(ctx, look, session);
        if let (Some(numeric), Some(b)) = (self.numeric.as_mut(), self.shown.clone()) {
            let (changed, open) = numeric.show(ctx, look, session, &b, backdrops);
            self.moved(changed);
            if !open {
                self.numeric = None;
            }
        }
    }

    /// The box: dashed edges, its nodes, the rotate node and the box-turning node (or a warp's
    /// lines and control points).
    fn overlay(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some(b) = self.shown.clone() else { return };
        let ppp = ctx.pixels_per_point();
        let view = session.view_matrix();
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::unique("transform box")));
        let scale = |h: TransformHandle| if self.held.is_some_and(|(held, _)| held == h) { HELD_SCALE } else { 1.0 };
        if self.kind == TransformKind::Warp && b.mesh.len() == 32 {
            let point = |x: f32, y: f32| {
                let [a, bb, c, d, e, f] =
                    view.get(..6).and_then(|v| <[f32; 6]>::try_from(v).ok()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                pos2(a * x + c * y + e, bb * x + d * y + f) / ppp
            };
            let mesh: Vec<Pos2> = b.mesh.as_chunks::<2>().0.iter().map(|&[x, y]| point(x, y)).collect();
            if self.advanced {
                // the control points joined along each row, and down the two edges, dotted
                let dots = |from: Pos2, to: Pos2| {
                    painter.extend(Shape::dashed_line(&[from, to], Stroke::new(1.0, argb(DASH)), 1.0, 2.0));
                };
                for k in 0..4 {
                    for j in 0..3 {
                        if let (Some(&p), Some(&q)) = (mesh.get(k * 4 + j), mesh.get(k * 4 + j + 1)) {
                            dots(p, q);
                        }
                        if (k == 0 || k == 3)
                            && let (Some(&p), Some(&q)) = (mesh.get(j * 4 + k), mesh.get((j + 1) * 4 + k))
                        {
                            dots(p, q);
                        }
                    }
                }
            }
            let per_line = b.grid.len() / 2 / 8;
            if per_line >= 2 {
                for line in b.grid.as_chunks::<2>().0.chunks(per_line).take(8) {
                    let points: Vec<Pos2> = line.iter().map(|&[x, y]| point(x, y)).collect();
                    painter.add(PathShape::line(points, Stroke::new(1.0, argb(MESH_LINE))));
                }
            }
            let held = self.held.map(|(h, _)| h);
            for i in procraft_core::shown_mesh_points(self.advanced) {
                let corner_of = |h: TransformHandle| match h {
                    TransformHandle::Corner { index } => [0u8, 3, 15, 12].get(usize::from(index % 4)).copied(),
                    _ => None,
                };
                let holding = held == Some(TransformHandle::Mesh { index: i }) || held.and_then(corner_of) == Some(i);
                if let Some(&p) = mesh.get(usize::from(i)) {
                    node(&painter, p, argb(NODE_BLUE), if holding { HELD_SCALE } else { 1.0 });
                }
            }
            return;
        }
        // the lines the box snapped onto, across the canvas, while it is held (under the box)
        let [va, vb, vc, vd, ve, vf] =
            view.get(..6).and_then(|v| <[f32; 6]>::try_from(v).ok()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let point = |x: f32, y: f32| pos2(va * x + vc * y + ve, vb * x + vd * y + vf) / ppp;
        for &[x0, y0, x1, y1] in b.snap_lines.as_chunks::<4>().0 {
            painter.line_segment([point(x0, y0), point(x1, y1)], Stroke::new(1.0, argb(SNAP_GUIDE)));
        }
        let Some(s) = procraft_core::screen_box(b.corners, view, ppp) else { return };
        let at = |v: &[f32], i: usize| match (v.get(2 * i), v.get(2 * i + 1)) {
            (Some(&x), Some(&y)) => pos2(x, y) / ppp,
            _ => Pos2::ZERO,
        };
        let corners: Vec<Pos2> = (0..4).map(|i| at(&s.corners, i)).collect();
        let edges: Vec<Pos2> = (0..4).map(|i| at(&s.edges, i)).collect();
        // the rotate and box-turning nodes follow the pointer while held, and spring back when let go
        if self.let_go.is_some_and(|(_, _, since)| since.elapsed().as_secs_f32() >= HANDLE_RETURN) {
            self.let_go = None;
        } else if self.let_go.is_some() {
            ctx.request_repaint();
        }
        let (held, let_go) = (self.held, self.let_go);
        let placed = |h: TransformHandle, rest: Pos2| {
            if let Some((_, p)) = held.filter(|(held, _)| *held == h) {
                return p / ppp;
            }
            if let Some((_, from, since)) = let_go.filter(|(l, _, _)| *l == h) {
                let k = (since.elapsed().as_secs_f32() / HANDLE_RETURN).min(1.0);
                return (from / ppp).lerp(rest, k);
            }
            rest
        };
        let rotate = placed(TransformHandle::Rotate, at(&s.rotate, 0));
        let turn = placed(TransformHandle::TurnBox, at(&s.turn, 0));
        let mut outline = corners.clone();
        outline.extend(corners.first());
        painter.extend(Shape::dashed_line_with_offset(&outline, Stroke::new(1.0, argb(DASH_GAP)), &[5.0], &[5.0], 5.0));
        painter.extend(Shape::dashed_line_with_offset(&outline, Stroke::new(1.0, argb(DASH)), &[5.0], &[5.0], 0.0));
        if let (Some(&top), Some(&bottom)) = (edges.first(), edges.get(2)) {
            painter.line_segment([top, rotate], Stroke::new(1.0, argb(ROTATE_LINE)));
            painter.line_segment([bottom, turn], Stroke::new(1.0, argb(TURN_LINE)));
        }
        for (i, &p) in corners.iter().enumerate() {
            node(&painter, p, argb(NODE_BLUE), scale(TransformHandle::Corner { index: i as u8 }));
        }
        for (i, &p) in edges.iter().enumerate() {
            node(&painter, p, argb(NODE_BLUE), scale(TransformHandle::Edge { index: i as u8 }));
        }
        node(&painter, rotate, argb(ROTATE_GREEN), scale(TransformHandle::Rotate));
        // the box-turning node is a square, turned with the box
        let k = scale(TransformHandle::TurnBox);
        let up = vec2(s.up.first().copied().unwrap_or(0.0), s.up.get(1).copied().unwrap_or(-1.0));
        let angle = up.x.atan2(-up.y);
        let square = |side: f32| {
            let (sin, cos) = angle.sin_cos();
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(x, y): (f32, f32)| turn + vec2(x * cos - y * sin, x * sin + y * cos) * side / 2.0)
                .to_vec()
        };
        painter.circle_filled(turn, 5.5 * k + 0.5, argb(NODE_SHADOW));
        painter.add(Shape::convex_polygon(square(11.0 * k), Color32::WHITE, Stroke::NONE));
        painter.add(Shape::convex_polygon(square(8.0 * k), argb(TURN_ORANGE), Stroke::NONE));
    }

    /// The floating toolbar: the four kinds in a pill over the buttons. Returns what was asked.
    fn toolbar(&mut self, ctx: &egui::Context, look: &mut Look) -> Option<Asked> {
        let kinds = [
            (&icons::FREEFORM, "Freeform", self.kind == TransformKind::Freeform),
            (&icons::UNIFORM, "Uniform", self.kind == TransformKind::Uniform),
            (&icons::DISTORT, "Distort", self.kind == TransformKind::Distort),
            (&icons::WARP, "Warp", self.kind == TransformKind::Warp),
        ];
        let all = [TransformKind::Freeform, TransformKind::Uniform, TransformKind::Distort, TransformKind::Warp];
        let warp = self.kind == TransformKind::Warp;
        let buttons: [Button; 7] = [
            if warp {
                Button::new(&ADVANCED_MESH, "Advanced Mesh", 28.8).active(self.advanced)
            } else {
                Button::new(&icons::SNAPPING, "Snapping", 44.0).active(self.popup == Popup::Snapping)
            },
            Button::new(&icons::FLIP_HORIZONTAL, "Flip Horizontal", 50.0),
            Button::new(&icons::FLIP_VERTICAL, "Flip Vertical", 50.0),
            Button::new(&icons::ROTATE45, "Rotate 45\u{b0}", 44.0),
            Button::new(&icons::FIT_TO_CANVAS, "Fit to Canvas", 35.0),
            Button::new(&icons::INTERPOLATION, interpolation_name(self.interpolation, true), 44.0)
                .active(self.popup == Popup::Interpolation),
            Button::new(&icons::TRANSFORM_RESET, "Reset", 44.0),
        ];
        let tapped =
            floating_toolbar(ctx, look, "transform toolbar", (BUTTON_ROOMY, BUTTON_WIDTH), &kinds, &buttons, 43.0)?;
        Some(match tapped {
            Tapped::Background => Asked::Background,
            Tapped::Segment(k) => Asked::Kind(all.get(k).copied().unwrap_or(TransformKind::Freeform)),
            Tapped::Button(0) if warp => Asked::AdvancedMesh,
            Tapped::Button(0) => Asked::Popup(Popup::Snapping),
            Tapped::Button(1) => Asked::Action(TransformAction::FlipHorizontal),
            Tapped::Button(2) => Asked::Action(TransformAction::FlipVertical),
            Tapped::Button(3) => Asked::Action(TransformAction::Rotate45),
            Tapped::Button(4) => Asked::Action(TransformAction::FitToCanvas),
            Tapped::Button(5) => Asked::Popup(Popup::Interpolation),
            Tapped::Button(_) => Asked::Action(TransformAction::Reset),
        })
    }

    /// The popover over the toolbar, its arrow on its button: Interpolation, or Snapping's settings.
    fn popover(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) {
        if self.popup == Popup::None {
            return;
        }
        let index = if self.popup == Popup::Snapping { 0 } else { 5 };
        let (tip_x, tip_y) = button_tip(ctx, (BUTTON_ROOMY, BUTTON_WIDTH), index, 7);
        let screen = ctx.content_rect();
        let body_height = match self.popup {
            Popup::Interpolation => 40.0 + 10.0 + 40.0 * 3.0 + 2.0,
            _ => 40.0 + 4.0 + 49.0 * 2.0 + 1.0 + 10.0 + 59.0 * 2.0 + 1.0,
        } + 20.0;
        let left = (tip_x - MENU_WIDTH / 2.0).clamp(screen.left(), screen.right() - MENU_WIDTH);
        let rect = Rect::from_min_size(pos2(left, tip_y - MENU_ARROW - body_height), vec2(MENU_WIDTH, body_height));
        egui::Area::new(Id::unique("transform popover")).fixed_pos(rect.min).order(Order::Tooltip).show(ctx, |ui| {
            popover_frame(ui, rect, tip_x, tip_y);
            let inner = rect.shrink(10.0);
            let title = |ui: &Ui, words: &str| menu_title(ui, inner, words);
            match self.popup {
                Popup::Interpolation => {
                    title(ui, "Interpolation");
                    let group = Rect::from_min_size(pos2(inner.left(), inner.top() + 50.0), vec2(inner.width(), 122.0));
                    ui.painter().rect_filled(group, 13, argb(MENU_GROUP));
                    let all = [Interpolation::Nearest, Interpolation::Bilinear, Interpolation::Bicubic];
                    for (i, kind) in all.into_iter().enumerate() {
                        let row = Rect::from_min_size(
                            pos2(group.left(), group.top() + 41.0 * i as f32),
                            vec2(group.width(), 40.0),
                        );
                        if i > 0 {
                            ui.painter().rect_filled(
                                Rect::from_min_size(row.min - vec2(0.0, 1.0), vec2(row.width(), 1.0)),
                                0,
                                argb(MENU_LINE),
                            );
                        }
                        let on = self.interpolation == kind;
                        if on {
                            let corners = egui::CornerRadius {
                                nw: if i == 0 { 13 } else { 0 },
                                ne: if i == 0 { 13 } else { 0 },
                                sw: if i == 2 { 13 } else { 0 },
                                se: if i == 2 { 13 } else { 0 },
                            };
                            ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
                        }
                        let ink = if on { Color32::WHITE } else { argb(MENU_TEXT) };
                        text(
                            ui.painter(),
                            pos2(row.left() + 11.0, row.center().y),
                            Align2::LEFT_CENTER,
                            interpolation_name(kind, false),
                            16.0,
                            Weight::Regular,
                            ink,
                        );
                        if on {
                            look.icon(
                                ui,
                                &icons::MENU_TICK,
                                pos2(row.right() - 14.0 - 14.5, row.center().y),
                                29.0,
                                Color32::WHITE,
                            );
                        }
                        if ui.interact(row, ui.scope_id().with(("interpolation", i)), Sense::click()).clicked() {
                            self.interpolation = kind;
                            session.transform_set_interpolation(kind);
                            self.popup = Popup::None;
                        }
                    }
                }
                _ => {
                    title(ui, "Settings");
                    let mut g = self.guides;
                    let group = Rect::from_min_size(pos2(inner.left(), inner.top() + 44.0), vec2(inner.width(), 99.0));
                    ui.painter().rect_filled(group, 13, argb(MENU_GROUP));
                    for (i, (label, on)) in
                        [("Magnetics", g.magnetics), ("Snapping", g.snapping)].into_iter().enumerate()
                    {
                        let row = Rect::from_min_size(
                            pos2(group.left(), group.top() + 50.0 * i as f32),
                            vec2(group.width(), 49.0),
                        );
                        if i > 0 {
                            ui.painter().rect_filled(
                                Rect::from_min_size(row.min - vec2(0.0, 1.0), vec2(row.width(), 1.0)),
                                0,
                                argb(MENU_LINE),
                            );
                        }
                        text(
                            ui.painter(),
                            pos2(row.left() + 12.0, row.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            16.0,
                            Weight::Regular,
                            argb(MENU_TEXT),
                        );
                        switch(ui, pos2(row.right() - 6.0 - 63.0, row.center().y - 14.0), on, label);
                        if ui.interact(row, ui.scope_id().with(("switch", i)), Sense::click()).clicked() {
                            if i == 0 {
                                g.magnetics = !g.magnetics;
                            } else {
                                g.snapping = !g.snapping;
                            }
                        }
                    }
                    let sliders =
                        Rect::from_min_size(pos2(inner.left(), group.bottom() + 10.0), vec2(inner.width(), 119.0));
                    ui.painter().rect_filled(sliders, 13, argb(MENU_GROUP));
                    let (lo, hi, default) = SNAP_DISTANCE;
                    let distance = (g.distance - lo) / (hi - lo);
                    let shown = if g.distance <= default {
                        "None".to_owned()
                    } else {
                        format!("{}", g.distance.round() as i32)
                    };
                    let row = Rect::from_min_size(sliders.min, vec2(sliders.width(), 59.0));
                    if let Some(v) = menu_slider(ui, row, "Distance", distance, &shown) {
                        g.distance = (lo + v * (hi - lo)).round();
                    }
                    ui.painter().rect_filled(
                        Rect::from_min_size(pos2(sliders.left(), row.bottom()), vec2(sliders.width(), 1.0)),
                        0,
                        argb(MENU_LINE),
                    );
                    let shown = if g.velocity >= SNAP_VELOCITY_MAX {
                        "Max".to_owned()
                    } else {
                        format!("{}", g.velocity.round() as i32)
                    };
                    let row =
                        Rect::from_min_size(pos2(sliders.left(), row.bottom() + 1.0), vec2(sliders.width(), 59.0));
                    if let Some(v) = menu_slider(ui, row, "Velocity", g.velocity / SNAP_VELOCITY_MAX, &shown) {
                        g.velocity = (v * SNAP_VELOCITY_MAX).round();
                    }
                    if g != self.guides {
                        self.guides = g;
                        session.transform_set_guides(g);
                    }
                }
            }
        });
    }
}

/// A toolbar's width and its buttons' width for `count` buttons on a window `screen` wide: with
/// room, `roomy` buttons 19 apart inside its padding; else across the window (8 clear of each
/// side, at most 728) with `narrow` ones spread over it.
fn toolbar_layout(screen: f32, count: usize, (roomy, narrow): (f32, f32)) -> (f32, f32) {
    let n = count as f32;
    let wide = roomy * n + BUTTON_GAP * (n - 1.0) + 32.0;
    if wide <= screen - 16.0 { (wide, roomy) } else { (TOOLBAR_WIDTH.min(screen - 16.0), narrow) }
}

/// The floating toolbar at the foot of the window for `count` buttons.
fn toolbar_rect(ctx: &egui::Context, count: usize, widths: (f32, f32)) -> (Rect, f32) {
    let screen = ctx.content_rect();
    let (width, button) = toolbar_layout(screen.width(), count, widths);
    let bar = Rect::from_min_size(
        pos2(screen.center().x - width / 2.0, screen.bottom() - TOOLBAR_BOTTOM - TOOLBAR_HEIGHT),
        vec2(width, TOOLBAR_HEIGHT),
    );
    (bar, button)
}

/// Where a popover over button `index` of a toolbar of `count` points: its button's middle, at the
/// top of the buttons.
pub(crate) fn button_tip(ctx: &egui::Context, widths: (f32, f32), index: usize, count: usize) -> (f32, f32) {
    let (bar, button) = toolbar_rect(ctx, count, widths);
    (button_centre(bar, button, index, count), bar.top() + 16.0 + 34.0)
}

/// A toolbar button: its icon, name and icon size; lit while its popover is open; greyed when it
/// can't be used yet.
pub(crate) struct Button<'a> {
    icon: &'a Icon,
    label: &'a str,
    size: f32,
    active: bool,
    enabled: bool,
}

impl<'a> Button<'a> {
    pub(crate) fn new(icon: &'a Icon, label: &'a str, size: f32) -> Button<'a> {
        Button { icon, label, size, active: false, enabled: true }
    }

    pub(crate) fn active(self, active: bool) -> Button<'a> {
        Button { active, ..self }
    }

    pub(crate) fn enabled(self, enabled: bool) -> Button<'a> {
        Button { enabled, ..self }
    }
}

/// What was tapped on a floating toolbar.
pub(crate) enum Tapped {
    Background,
    Segment(usize),
    Button(usize),
}

/// The floating toolbar at the foot of the window (transform, selection): a row of `segments`
/// (icon, name, chosen) in a pill, their icons `segment_icon` across, over `buttons` spread across
/// it (with room `widths.0` wide, else `widths.1`). Returns what was tapped.
pub(crate) fn floating_toolbar(
    ctx: &egui::Context,
    look: &mut Look,
    id: &str,
    widths: (f32, f32),
    segments: &[(&Icon, &str, bool)],
    buttons: &[Button],
    segment_icon: f32,
) -> Option<Tapped> {
    let (bar, button) = toolbar_rect(ctx, buttons.len(), widths);
    let roomy = widths.0;
    let mut tapped = None;
    egui::Area::new(Id::unique(id)).fixed_pos(bar.min).order(Order::Foreground).show(ctx, |ui| {
        // the bar's room claimed, so presses on it stay off the canvas
        if ui.allocate_rect(bar, Sense::click()).clicked() {
            tapped = Some(Tapped::Background);
        }
        ui.painter().rect_filled(bar, TOOLBAR_CORNER, argb(TOOLBAR_FILL));
        let widths: Vec<f32> = segments.iter().map(|(_, label, _)| segment_width(ui, label)).collect();
        let row = widths.iter().sum::<f32>() + SEGMENT_GAP * (segments.len().max(1) - 1) as f32 + 4.0;
        let pill = Rect::from_min_size(pos2(bar.center().x - row / 2.0, bar.top() + 16.0), vec2(row, 34.0));
        ui.painter().rect_filled(pill, 17, argb(SEGMENT_TRACK));
        ui.painter().rect_stroke(pill, 17, Stroke::new(0.5, argb(SEGMENT_EDGE)), egui::StrokeKind::Inside);
        let mut x = pill.left() + 2.0;
        for (k, ((icon, label, on), w)) in segments.iter().zip(widths).enumerate() {
            let seg = Rect::from_min_size(pos2(x, pill.top() + 2.0), vec2(w, 30.0));
            if segment(ui, look, seg, icon, label, *on, segment_icon) {
                tapped = Some(Tapped::Segment(k));
            }
            x += w + SEGMENT_GAP;
        }
        // a long name smaller only where the buttons are narrow
        let long = if button < roomy { 11.0 } else { 12.5 };
        for (i, b) in buttons.iter().enumerate() {
            let centre = button_centre(bar, button, i, buttons.len());
            let rect = Rect::from_min_size(pos2(centre - button / 2.0, pill.bottom()), vec2(button, BUTTON_HEIGHT));
            let label_size = if b.label.len() > 12 { long } else { 12.5 };
            if toolbar_button(ui, look, rect, b, label_size) {
                tapped = Some(Tapped::Button(i));
            }
        }
    });
    tapped
}

/// A popover's body at `rect` over a toolbar, its arrow's tip at (`tip_x`, `tip_y`), its shadow
/// round it; it takes the presses on it.
pub(crate) fn popover_frame(ui: &mut Ui, rect: Rect, tip_x: f32, tip_y: f32) {
    // its room claimed, so presses on it stay off the canvas
    ui.allocate_rect(rect, Sense::click());
    let shadow = egui::epaint::Shadow { offset: [0, 0], blur: 51, spread: 0, color: Color32::from_black_alpha(0x38) };
    ui.painter().add(shadow.as_shape(rect, MENU_CORNER));
    ui.painter().rect_filled(rect, MENU_CORNER, argb(MENU_FILL));
    let arrow = vec![
        pos2(tip_x - MENU_ARROW, rect.bottom() - 0.5),
        pos2(tip_x + MENU_ARROW, rect.bottom() - 0.5),
        pos2(tip_x, tip_y),
    ];
    ui.painter().add(Shape::convex_polygon(arrow, argb(MENU_FILL), Stroke::NONE));
}

/// A popover's title, 24 bold, at the top of its inside `inner`.
pub(crate) fn menu_title(ui: &Ui, inner: Rect, words: &str) {
    text(
        ui.painter(),
        pos2(inner.left() + 12.0, inner.top() + 20.0),
        Align2::LEFT_CENTER,
        words,
        24.0,
        Weight::Bold,
        argb(MENU_TEXT),
    );
}

/// Where button `index` of `count` is centred on the toolbar `bar`.
fn button_centre(bar: Rect, button: f32, index: usize, count: usize) -> f32 {
    let gap = (bar.width() - 32.0 - button * count as f32) / (count.max(2) - 1) as f32;
    bar.left() + 16.0 + (button + gap) * index as f32 + button / 2.0
}

fn segment_width(ui: &Ui, label: &str) -> f32 {
    14.0 + 30.0
        + 2.0
        + ui.painter().layout_no_wrap(label.into(), Weight::Regular.font(13.0), Color32::WHITE).size().x
        + 20.0
}

/// One of the four ways: its icon (`size` across) and name, filled blue when chosen. True when
/// tapped.
fn segment(ui: &Ui, look: &mut Look, rect: Rect, icon: &Icon, label: &str, on: bool, size: f32) -> bool {
    if on {
        ui.painter().rect_filled(rect, 15, argb(pc::ACCENT));
    }
    let ink = if on { Color32::WHITE } else { argb(SEGMENT_TEXT) };
    look.icon(ui, icon, pos2(rect.left() + 14.0 + 15.0, rect.center().y), size, ink);
    text(
        ui.painter(),
        pos2(rect.left() + 14.0 + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        13.0,
        Weight::Regular,
        ink,
    );
    ui.interact(rect, ui.scope_id().with(("segment", label)), Sense::click()).clicked()
}

/// A toolbar button: its icon centred 30 below its top over its name; blue while its popover is
/// open, grey when it can't be used. True when tapped.
fn toolbar_button(ui: &Ui, look: &mut Look, rect: Rect, b: &Button, label_size: f32) -> bool {
    let tint = if !b.enabled {
        argb(BUTTON_DIMMED)
    } else if b.active {
        argb(pc::ACCENT)
    } else {
        argb(BUTTON_TEXT)
    };
    let ink = if b.enabled { argb(BUTTON_TEXT) } else { argb(BUTTON_DIMMED) };
    look.icon(ui, b.icon, pos2(rect.center().x, rect.top() + 30.0), b.size, tint);
    text(
        ui.painter(),
        pos2(rect.center().x, rect.top() + 50.0),
        Align2::CENTER_CENTER,
        b.label,
        label_size,
        Weight::Regular,
        ink,
    );
    b.enabled && ui.interact(rect, ui.scope_id().with(("toolbar button", b.label)), Sense::click()).clicked()
}

/// A node: a coloured dot on a white disc with a faint shadow, `scale` times its size.
fn node(painter: &egui::Painter, at: Pos2, colour: Color32, scale: f32) {
    painter.circle_filled(at, 5.5 * scale + 0.5, argb(NODE_SHADOW));
    painter.circle_filled(at, 5.5 * scale, Color32::WHITE);
    painter.circle_filled(at, 3.5 * scale, colour);
}

/// A switch at `at` (63 × 28): blue with its knob right when on, as the Android app's
/// (ui/ActionsPanel.kt, Switch). Flipped, the knob slides across as the track's colour follows,
/// swelling into a clear lens that settles white again. `id` tells it from the others shown.
pub(crate) fn switch(ui: &Ui, at: Pos2, on: bool, id: impl std::hash::Hash + std::fmt::Debug) {
    let ctx = ui.ctx();
    let id = ui.scope_id().with(("switch", id));
    let k = ctx.animate_bool_with_time_and_easing(id, on, KNOB_SLIDE_S, knob_ease);
    // the lens: since it was last flipped (not as it first shows)
    let now = ui.input(|i| i.time);
    let flipped = ctx.data_mut(|d| {
        let seen = d.get_temp_mut_or_insert_with(id.with("flipped"), || (on, f64::NEG_INFINITY));
        if seen.0 != on {
            *seen = (on, now);
        }
        seen.1
    });
    let e = (now - flipped) as f32;
    let lens = if e < SWELL_S {
        ease_in_out(e / SWELL_S)
    } else if e < SWELL_S + SWELL_HOLD_S {
        1.0
    } else if e < SWELL_S + SWELL_HOLD_S + SETTLE_S {
        1.0 - ease_in_out((e - SWELL_S - SWELL_HOLD_S) / SETTLE_S)
    } else {
        0.0
    };
    if e < SWELL_S + SWELL_HOLD_S + SETTLE_S {
        ctx.request_repaint();
    }
    let track = Rect::from_min_size(at, vec2(63.0, 28.0));
    ui.painter().rect_filled(track, 14, mix(argb(SWITCH_OFF), argb(pc::ACCENT), k));
    // not clipped to the track: the swollen knob spills over it
    let centre = at + vec2(2.0 + 22.0 * k + 37.0 / 2.0, 2.0 + 12.0);
    let knob = Rect::from_center_size(centre, vec2(37.0 * (1.0 + 0.35 * lens), 24.0 * (1.0 + 0.4 * lens)));
    let round = knob.height() / 2.0;
    ui.painter().rect_filled(knob, round, Color32::WHITE.gamma_multiply(1.0 - 0.7 * lens));
    if lens > 0.0 {
        let rim = Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.55 * lens));
        ui.painter().rect_stroke(knob, round, rim, egui::StrokeKind::Inside);
    }
}

/// A switch's knob sliding across, and its lens swelling, held and settling (seconds).
const KNOB_SLIDE_S: f32 = 0.33;
const SWELL_S: f32 = 0.07;
const SWELL_HOLD_S: f32 = 0.14;
const SETTLE_S: f32 = 0.12;

fn knob_ease(t: f32) -> f32 {
    crate::look::cubic_bezier(0.3, 0.0, 0.2, 1.0, t)
}

fn ease_in_out(t: f32) -> f32 {
    crate::look::cubic_bezier(0.42, 0.0, 0.58, 1.0, t)
}

/// A popover's slider row (59 tall): its name and value, then a thin slider, blue up to a small
/// white knob. Returns the value set, while held.
pub(crate) fn menu_slider(ui: &Ui, row: Rect, label: &str, value: f32, shown: &str) -> Option<f32> {
    let ink = argb(MENU_TEXT);
    text(ui.painter(), pos2(row.left() + 14.0, row.top() + 9.0), Align2::LEFT_TOP, label, 16.0, Weight::Regular, ink);
    text(ui.painter(), pos2(row.right() - 13.0, row.top() + 9.0), Align2::RIGHT_TOP, shown, 16.0, Weight::Regular, ink);
    let bar = Rect::from_min_size(pos2(row.left(), row.top() + 38.0), vec2(row.width(), 14.0));
    let (start, end) = (bar.left() + 21.5, bar.right() - 15.0);
    let y = bar.center().y;
    let x = start + (end - start) * value.clamp(0.0, 1.0);
    ui.painter().line_segment([pos2(x, y), pos2(end, y)], Stroke::new(2.0, argb(SLIDER_REST)));
    ui.painter().line_segment([pos2(start, y), pos2(x, y)], Stroke::new(2.0, argb(pc::ACCENT)));
    let knob = Rect::from_center_size(pos2(x, y), vec2(15.0, 11.0));
    ui.painter().rect_filled(knob.translate(vec2(0.0, 0.5)), 5.5, Color32::from_black_alpha(0x33));
    ui.painter().rect_filled(knob, 5.5, Color32::WHITE);
    let response =
        ui.interact(bar.expand2(vec2(0.0, 6.0)), ui.scope_id().with(("menu slider", label)), Sense::click_and_drag());
    let p = response.interact_pointer_pos()?;
    Some(((p.x - start) / (end - start)).clamp(0.0, 1.0))
}

fn interpolation_name(i: Interpolation, short: bool) -> &'static str {
    match i {
        Interpolation::Nearest if short => "Nearest",
        Interpolation::Nearest => "Nearest Neighbor",
        Interpolation::Bilinear => "Bilinear",
        Interpolation::Bicubic => "Bicubic",
    }
}
