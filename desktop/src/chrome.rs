//! The bars over the canvas, as the Android app's (ui/Chrome.kt): the top bar (Gallery, the
//! action buttons, the tools, Layers and the colour) and the side bar (brush size, the modify
//! button, opacity, undo and redo, and a slider's panel while it is dragged).

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use procraft_core::{CanvasInfo, CanvasSession, ToolKind};
use resvg::tiny_skia;

use crate::icons;
use crate::look::{Look, Weight, argb, fill, highlight, mix, pc, round_rect, text, text_runs};

/// The top bar's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarButton {
    Gallery,
    Actions,
    Adjustments,
    Selection,
    Transform,
    Paint,
    Smudge,
    Erase,
    Layers,
    Colour,
}

/// Which of the top bar's buttons are lit.
#[derive(Default)]
pub struct Lit {
    pub actions: bool,
    pub adjustments: bool,
    pub selection: bool,
    pub transform: bool,
    pub layers: bool,
    /// A panel takes the highlight from the current tool while open.
    pub panel_open: bool,
}

/// The button tapped on the top bar, if any, and where each one is (points).
#[derive(Default)]
pub struct Bar {
    pub pressed: Option<BarButton>,
    pub rects: Vec<(BarButton, Rect)>,
}

impl Bar {
    pub fn rect(&self, button: BarButton) -> Option<Rect> {
        self.rects.iter().find(|(b, _)| *b == button).map(|(_, r)| *r)
    }
}

/// The colour button's brightness under which it is ringed on the dark bar.
const DARK_RING_BELOW: f32 = 0.35;

/// The bar across the top of the canvas: artwork actions on the left, painting tools on the right.
pub fn top_bar(ui: &mut Ui, look: &mut Look, tool: ToolKind, colour: Color32, brightness: f32, lit: &Lit) -> Bar {
    let mut bar = Bar::default();
    let frame = egui::Frame::NONE.fill(argb(pc::BAR));
    egui::Panel::top("bar").exact_size(pc::BAR_HEIGHT).frame(frame).show(ui, |ui| {
        let r = ui.max_rect();
        let ctx = ui.ctx().clone();
        let (tool_gap, action_gap) = pc::bar_gaps(r.width());
        let cy = r.center().y;
        let button = |x: f32, width: f32| {
            Rect::from_min_size(pos2(x, cy - pc::BAR_BUTTON_HEIGHT / 2.0), vec2(width, pc::BAR_BUTTON_HEIGHT))
        };
        let mut tap = |ui: &Ui, which: BarButton, rect: Rect| {
            bar.rects.push((which, rect));
            if ui.interact(rect, ui.scope_id().with(which), Sense::click()).clicked() {
                bar.pressed = Some(which);
            }
        };

        let mut x = r.left() + pc::BAR_MARGIN;
        let gallery = button(x, pc::GALLERY_WIDTH);
        tap(ui, BarButton::Gallery, gallery);
        text(
            ui.painter(),
            pos2(gallery.left(), cy - 1.0),
            Align2::LEFT_CENTER,
            "Gallery",
            18.0,
            Weight::Medium,
            argb(pc::BAR_TEXT),
        );
        x += pc::GALLERY_WIDTH + action_gap;
        // an action button: its glyph on a dark disc, which turns blue while its panel is open;
        // the glyphs are drawn in a 44 frame, shown `glyph` across
        let actions = [
            (BarButton::Actions, &icons::ACTIONS, 36.0, lit.actions),
            (BarButton::Adjustments, &icons::ADJUSTMENTS, 36.0, lit.adjustments),
            (BarButton::Selection, &icons::SELECTION, 33.0, lit.selection),
            (BarButton::Transform, &icons::TRANSFORM, 44.0, lit.transform),
        ];
        for (which, icon, glyph, on) in actions {
            let rect = button(x, pc::BAR_BUTTON_WIDTH);
            tap(ui, which, rect);
            let l = highlight(&ctx, ui.scope_id().with(("lit", which)), on);
            let disc = mix(argb(pc::ACTION_DISC), argb(pc::BAR_GLYPH_SELECTED), l);
            ui.painter().circle_filled(rect.center(), pc::ACTION_DISC_SIZE / 2.0, disc);
            look.icon(ui, icon, rect.center(), glyph, mix(argb(pc::ACTION_GLYPH), Color32::WHITE, l));
            x += pc::BAR_BUTTON_WIDTH + action_gap;
        }

        // from the colour leftwards
        let mut x = r.right() - pc::BAR_MARGIN - pc::BAR_BUTTON_WIDTH;
        let rect = button(x, pc::BAR_BUTTON_WIDTH);
        tap(ui, BarButton::Colour, rect);
        ui.painter().circle_filled(rect.center(), pc::COLOR_DOT / 2.0, colour);
        // ringed only where the colour would fade into the bar
        if brightness < DARK_RING_BELOW {
            ui.painter().circle_stroke(
                rect.center(),
                pc::COLOR_DOT / 2.0 - 0.5,
                Stroke::new(1.0, argb(pc::COLOR_RING)),
            );
        }
        let tools = [
            (BarButton::Layers, &icons::LAYERS, lit.layers),
            (BarButton::Erase, &icons::ERASE, !lit.panel_open && tool == ToolKind::Erase),
            (BarButton::Smudge, &icons::SMUDGE, !lit.panel_open && tool == ToolKind::Smudge),
            (BarButton::Paint, &icons::BRUSH, !lit.panel_open && tool == ToolKind::Paint),
        ];
        for (which, icon, on) in tools {
            x -= pc::BAR_BUTTON_WIDTH + tool_gap;
            let rect = button(x, pc::BAR_BUTTON_WIDTH);
            tap(ui, which, rect);
            let l = highlight(&ctx, ui.scope_id().with(("lit", which)), on);
            // a 44 frame shown as it is, over the 38 tall button
            look.icon(ui, icon, rect.center(), 44.0, mix(argb(pc::BAR_GLYPH), argb(pc::BAR_GLYPH_SELECTED), l));
        }
    });
    bar
}

/// The side bar's sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slider {
    Size,
    Opacity,
}

/// The side bar's own state: the slider being dragged and its value before any rounding.
#[derive(Default)]
pub struct SideBar {
    dragging: Option<(Slider, f32)>,
}

const SLIDER_LEFT: f32 = 5.0;
const SIZE_TRACK_TOP: f32 = 6.0;
const OPACITY_TRACK_TOP: f32 = 248.0;
const SLIDER_PANEL_SIDE: f32 = 190.0;
const SLIDER_PANEL_GAP: f32 = 8.0;
const SLIDER_PANEL_RISE: f32 = 10.0;
const SLIDER_PANEL_FILL: u32 = 0xC93E3E3E;
const SLIDER_PANEL_INK: u32 = 0xFFF0F0F0;

impl SideBar {
    /// The side bar against the window's left edge; a slider's panel adds its frost to `backdrops`
    /// (window pixels: left, top, right, bottom, corner, how much shows).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &CanvasSession,
        info: &CanvasInfo,
        backdrops: &mut Vec<f32>,
    ) {
        let ctx = ui.ctx().clone();
        let window = ctx.content_rect();
        let top = window.top() + window.height() * pc::SIDE_BAR_CENTRE - pc::SIDE_BAR_HEIGHT / 2.0;
        let bar = Rect::from_min_size(pos2(window.left(), top), vec2(pc::SIDE_BAR_WIDTH, pc::SIDE_BAR_HEIGHT));
        let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(bar.min + vec2(x, y), vec2(w, h));
        let tracks = [(Slider::Size, SIZE_TRACK_TOP, info.size), (Slider::Opacity, OPACITY_TRACK_TOP, info.opacity)];
        egui::Area::new(egui::Id::unique("side bar")).fixed_pos(bar.min).order(egui::Order::Middle).show(&ctx, |ui| {
            ui.set_min_size(bar.size());
            // translucent, with windows where the slider tracks are: they show what is behind
            look.shape(ui, "side bar", (), bar, |pixmap, t| {
                let mut pb = tiny_skia::PathBuilder::new();
                let c = pc::SIDE_BAR_CORNER;
                round_rect(&mut pb, Rect::from_min_size(pos2(0.0, 0.0), bar.size()), [0.0, c, c, 0.0]);
                for (_, y, _) in tracks {
                    let track = Rect::from_min_size(pos2(SLIDER_LEFT, y), vec2(pc::SLIDER_WIDTH, pc::SLIDER_HEIGHT));
                    round_rect(&mut pb, track, [pc::SLIDER_CORNER; 4]);
                }
                fill(pixmap, pb.finish(), argb(pc::SIDE_BAR), t);
            });
            for (which, y, value) in tracks {
                let rect = at(SLIDER_LEFT, y, pc::SLIDER_WIDTH, pc::SLIDER_HEIGHT);
                self.slider(ui, look, session, which, rect, value);
            }
            // the modify button
            let modify = at(5.0, 191.0, 40.0, 41.0);
            let square = egui::Rect::from_center_size(modify.center(), vec2(pc::MODIFY_SIZE, pc::MODIFY_SIZE));
            let w = pc::MODIFY_STROKE;
            ui.painter().rect_stroke(
                square.shrink(w / 2.0),
                pc::MODIFY_CORNER - w / 2.0,
                Stroke::new(w, argb(pc::SIDE_GLYPH)),
                egui::StrokeKind::Middle,
            );
            for (icon, enabled, y, undo) in
                [(&icons::UNDO, info.can_undo, 430.5, true), (&icons::REDO, info.can_redo, 472.5, false)]
            {
                let rect = at(5.0, y, 40.0, 41.0);
                let tapped = ui.interact(rect, ui.scope_id().with(("side", undo)), Sense::click()).clicked();
                if tapped && enabled {
                    if undo {
                        session.undo();
                    } else {
                        session.redo();
                    }
                }
                // a 50 frame shown 36 across
                let tint = argb(if enabled { pc::SIDE_GLYPH } else { pc::SIDE_GLYPH_DISABLED });
                look.icon(ui, icon, rect.center(), 36.0, tint);
            }
        });
        if let Some((which, _)) = self.dragging {
            let y = if which == Slider::Size { SIZE_TRACK_TOP } else { OPACITY_TRACK_TOP };
            let min = bar.min + vec2(SLIDER_LEFT + pc::SLIDER_WIDTH + SLIDER_PANEL_GAP, y - SLIDER_PANEL_RISE);
            slider_panel(
                &ctx,
                which,
                Rect::from_min_size(min, vec2(SLIDER_PANEL_SIDE, SLIDER_PANEL_SIDE)),
                info,
                backdrops,
            );
        }
    }

    /// An upright slider moved by dragging anywhere on it: the thumb follows the pointer (not
    /// jumping to it), 1 at the top.
    fn slider(&mut self, ui: &Ui, look: &mut Look, session: &CanvasSession, which: Slider, rect: Rect, value: f32) {
        let travel = pc::SLIDER_HEIGHT - pc::THUMB_HEIGHT - pc::THUMB_INSET * 2.0;
        let response = ui.interact(rect, ui.scope_id().with(("slider", which)), Sense::drag());
        if response.drag_started() {
            self.dragging = Some((which, value));
        }
        if let Some((w, raw)) = self.dragging.as_mut().filter(|(w, _)| *w == which) {
            let dy = response.drag_delta().y;
            if dy != 0.0 {
                *raw = (*raw - dy / travel).clamp(0.0, 1.0);
                match w {
                    Slider::Size => session.set_brush_size(*raw),
                    Slider::Opacity => session.set_brush_opacity(*raw),
                }
            }
        }
        if response.drag_stopped() {
            self.dragging = None;
        }
        let slot = match which {
            Slider::Size => "size slider",
            Slider::Opacity => "opacity slider",
        };
        look.shape(ui, slot, (value * 10_000.0).round() as i32, rect, |pixmap, t| {
            let size = rect.size();
            let thumb = vec2(pc::THUMB_WIDTH, pc::THUMB_HEIGHT);
            let inset = pc::THUMB_INSET;
            let knob_at = pos2((size.x - thumb.x) / 2.0, inset + (size.y - thumb.y - inset * 2.0) * (1.0 - value));
            let knob = Rect::from_min_size(knob_at, thumb);
            let rim = pc::SLIDER_RIM_WIDTH;
            let outer = Rect::from_min_size(pos2(0.0, 0.0), size);
            let inside = outer.shrink(rim);
            let corner = pc::SLIDER_CORNER;
            // the track has a window where the thumb is, so the thumb shows over the bar alone;
            // a lighter rim runs round it
            let mut track = tiny_skia::PathBuilder::new();
            round_rect(&mut track, inside, [corner - rim; 4]);
            round_rect(&mut track, knob, [thumb.y / 2.0; 4]);
            fill(pixmap, track.finish(), argb(pc::SLIDER_TRACK), t);
            let mut ring = tiny_skia::PathBuilder::new();
            round_rect(&mut ring, outer, [corner; 4]);
            round_rect(&mut ring, inside, [corner - rim; 4]);
            fill(pixmap, ring.finish(), argb(pc::SLIDER_RIM), t);
            let mut thumb_path = tiny_skia::PathBuilder::new();
            round_rect(&mut thumb_path, knob, [thumb.y / 2.0; 4]);
            fill(pixmap, thumb_path.finish(), argb(pc::SLIDER_THUMB), t);
        });
    }
}

/// A slider's panel: its name and value ("Size 8%"), and in the middle a dot of the brush as it
/// would paint now, its size on the screen (and at its opacity).
fn slider_panel(ctx: &egui::Context, which: Slider, rect: Rect, info: &CanvasInfo, backdrops: &mut Vec<f32>) {
    let ppp = ctx.pixels_per_point();
    let corner = 22.0;
    backdrops.extend([rect.left(), rect.top(), rect.right(), rect.bottom(), corner].map(|v| v * ppp));
    backdrops.push(1.0);
    egui::Area::new(egui::Id::unique("slider panel"))
        .fixed_pos(rect.min)
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            let painter = ui.painter();
            painter.rect_filled(rect, corner, argb(SLIDER_PANEL_FILL));
            // a hairline rim
            painter.rect_stroke(rect, corner, Stroke::new(1.0, argb(pc::GLASS_MENU_EDGE)), egui::StrokeKind::Inside);
            let (name, value) = match which {
                Slider::Size => ("Size", info.size),
                Slider::Opacity => ("Opacity", info.opacity),
            };
            let shown = format!(" {}%", (value * 100.0).round());
            let line = pos2(rect.left() + 16.0, rect.top() + 6.0 + 16.0);
            text_runs(
                painter,
                line,
                Align2::LEFT_CENTER,
                &[(name, Weight::Bold), (&shown, Weight::Regular)],
                19.0,
                argb(SLIDER_PANEL_INK),
            );
            let diameter = info.brush_diameter * info.zoom / ppp;
            let r = (diameter / 2.0).clamp(1.5, (SLIDER_PANEL_SIDE / 2.0 - 8.0).max(1.5));
            let alpha = if which == Slider::Size { 1.0 } else { info.opacity.clamp(0.02, 1.0) };
            painter.circle_filled(rect.center(), r, Color32::WHITE.gamma_multiply(alpha));
        });
}
