//! The bars over the canvas, as the Android app's (ui/Chrome.kt): the top bar (Gallery, the
//! action buttons, the tools, Layers and the colour) and the side bar (brush size, the modify
//! button, opacity, undo and redo, and a slider's panel while it is dragged).

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use procraft_core::{CanvasInfo, CanvasSession, ToolKind};
use resvg::tiny_skia;

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, fill, highlight, mix, pc, round_rect, text, text_runs};

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

/// The button tapped on the top bar, if any, and where each one is (points); the one held down
/// (not dragged off), and since when (seconds, egui's clock). The colour button's touches are
/// ColorDrop's (see [`crate::colour_drop`]): it is never the one tapped here.
#[derive(Default)]
pub struct Bar {
    pub pressed: Option<BarButton>,
    pub rects: Vec<(BarButton, Rect)>,
    pub down: Option<(BarButton, f64)>,
}

impl Bar {
    pub fn rect(&self, button: BarButton) -> Option<Rect> {
        self.rects.iter().find(|(b, _)| *b == button).map(|(_, r)| *r)
    }
}

/// The colour button's brightness under which it is ringed on the dark bar.
const DARK_RING_BELOW: f32 = 0.35;

/// Adds `rect` (points, its corners `corner`) to `glass`: frost, or a window (window pixels: left,
/// top, right, bottom, corner, how much shows).
pub fn frost(glass: &mut Vec<f32>, rect: Rect, corner: f32, ppp: f32) {
    glass.extend([rect.left(), rect.top(), rect.right(), rect.bottom(), corner].map(|v| v * ppp));
    glass.push(1.0);
}

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
            let response = ui.interact(rect, ui.scope_id().with(which), Sense::click());
            if response.clicked() {
                bar.pressed = Some(which);
            }
            let (since, dragging) = ui.input(|i| (i.pointer.press_start_time(), i.pointer.is_decidedly_dragging()));
            if response.is_pointer_button_down_on()
                && !dragging
                && let Some(since) = since
            {
                bar.down = Some((which, since));
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
        let colour_rect = rect;
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
        bar.rects.push((BarButton::Colour, colour_rect));
    });
    bar
}

/// Where the side bar is in `window`, against its right edge or its left.
pub fn side_bar_rect(window: Rect, right: bool) -> Rect {
    let (top, track_height) = pc::side_bar_fit(window.height());
    // shorter tracks bring what is under each up with them
    let shorter = pc::SLIDER_HEIGHT - track_height;
    let left = if right { window.right() - pc::SIDE_BAR_WIDTH } else { window.left() };
    Rect::from_min_size(pos2(left, window.top() + top), vec2(pc::SIDE_BAR_WIDTH, pc::SIDE_BAR_HEIGHT - shorter * 2.0))
}

/// The side bar's sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slider {
    Size,
    Opacity,
}

/// What the side bar's buttons were asked: undo, redo, and the modify button tapped or held.
#[derive(Default)]
pub struct SideTaps {
    pub undo: bool,
    pub redo: bool,
    pub modify_tapped: bool,
    pub modify_down: bool,
    /// A slider was let go or a notch saved or taken away: the brush's settings changed.
    pub brush_changed: bool,
}

/// The side bar's own state: the slider pressed (its value before a notch pulls it in, when, and
/// whether it moved), the slider whose panel shows (kept up when it was held still), and whether
/// the brush's settings changed this frame.
#[derive(Default)]
pub struct SideBar {
    pressed: Option<Press>,
    panel: Option<(Slider, bool)>,
    changed: bool,
}

#[derive(Clone, Copy)]
struct Press {
    which: Slider,
    raw: f32,
    since: std::time::Instant,
    moved: bool,
}

/// A slider held still this long keeps its panel up.
const HOLD: std::time::Duration = std::time::Duration::from_millis(400);
const NOTCH_WIDTH: f32 = 20.0;
const NOTCH_HEIGHT: f32 = 4.0;
const NOTCH_MARK: u32 = 0x1AFFFFFF;

const SLIDER_LEFT: f32 = 5.0;
const SIZE_TRACK_TOP: f32 = 6.0;
const OPACITY_TRACK_TOP: f32 = 248.0;
const SLIDER_PANEL_SIDE: f32 = 190.0;
const SLIDER_PANEL_GAP: f32 = 8.0;
const SLIDER_PANEL_RISE: f32 = 10.0;
const SLIDER_PANEL_FILL: Themed = Themed(0xC93E3E3E, 0xFFE7E7E7);
const SLIDER_PANEL_INK: Themed = Themed(0xFFF0F0F0, 0xFF393939);

impl SideBar {
    /// The side bar against the window's left edge (the right one, `right`-handed: everything
    /// mirrored but the glyphs); it and a slider's panel add their frost to `backdrops`, and its
    /// slider tracks their thin glass to `windows` (see [`frost`]).
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &CanvasSession,
        info: &CanvasInfo,
        modify_lit: bool,
        right: bool,
        backdrops: &mut Vec<f32>,
        windows: &mut Vec<f32>,
    ) -> SideTaps {
        let mut taps = SideTaps::default();
        let ctx = ui.ctx().clone();
        let window = ctx.content_rect();
        let (_, track_height) = pc::side_bar_fit(window.height());
        // shorter tracks bring what is under each up with them
        let shorter = pc::SLIDER_HEIGHT - track_height;
        let opacity_top = OPACITY_TRACK_TOP - shorter;
        let bar = side_bar_rect(window, right);
        // a control's left edge in the bar, mirrored on the right
        let x = |left: f32, width: f32| if right { pc::SIDE_BAR_WIDTH - left - width } else { left };
        let at = |l: f32, y: f32, w: f32, h: f32| Rect::from_min_size(bar.min + vec2(x(l, w), y), vec2(w, h));
        let tracks = [(Slider::Size, SIZE_TRACK_TOP, info.size), (Slider::Opacity, opacity_top, info.opacity)];
        // its frost goes past the window's edge, so that side's corners are square
        let ppp = ctx.pixels_per_point();
        let past = if right {
            bar.with_max_x(bar.right() + pc::SIDE_BAR_CORNER)
        } else {
            bar.with_min_x(bar.left() - pc::SIDE_BAR_CORNER)
        };
        frost(backdrops, past, pc::SIDE_BAR_CORNER, ppp);
        for (_, y, _) in tracks {
            frost(windows, at(SLIDER_LEFT, y, pc::SLIDER_WIDTH, track_height), pc::SLIDER_CORNER, ppp);
        }
        egui::Area::new(egui::Id::unique("side bar")).fixed_pos(bar.min).order(egui::Order::Middle).show(&ctx, |ui| {
            ui.set_min_size(bar.size());
            // translucent, with windows where the slider tracks are: they show what is behind
            look.shape(ui, "side bar", (right, track_height.to_bits()), bar, |pixmap, t| {
                let mut pb = tiny_skia::PathBuilder::new();
                let c = pc::SIDE_BAR_CORNER;
                let corners = if right { [c, 0.0, 0.0, c] } else { [0.0, c, c, 0.0] };
                round_rect(&mut pb, Rect::from_min_size(pos2(0.0, 0.0), bar.size()), corners);
                for (_, y, _) in tracks {
                    let track = Rect::from_min_size(
                        pos2(x(SLIDER_LEFT, pc::SLIDER_WIDTH), y),
                        vec2(pc::SLIDER_WIDTH, track_height),
                    );
                    round_rect(&mut pb, track, [pc::SLIDER_CORNER; 4]);
                }
                fill(pixmap, pb.finish(), argb(pc::SIDE_BAR), t);
            });
            for (which, y, value) in tracks {
                let rect = at(SLIDER_LEFT, y, pc::SLIDER_WIDTH, track_height);
                self.slider(ui, look, session, which, rect, value);
            }
            // a slider held still keeps its panel up, until the next press elsewhere
            if let Some(press) = self.pressed.filter(|p| !p.moved) {
                if press.since.elapsed() >= HOLD {
                    self.panel = Some((press.which, true));
                } else {
                    ui.ctx().request_repaint_after(HOLD - press.since.elapsed());
                }
            }
            // the modify button: held, it changes what a canvas touch does; tapped, it does what
            // Tap □ is set to (opens the eyedropper)
            let modify = at(5.0, 191.0 - shorter, 40.0, 41.0);
            let response = ui.interact(modify, ui.scope_id().with("modify"), Sense::click_and_drag());
            taps.modify_down = response.is_pointer_button_down_on();
            taps.modify_tapped = response.clicked();
            let square = egui::Rect::from_center_size(modify.center(), vec2(pc::MODIFY_SIZE, pc::MODIFY_SIZE));
            let w = pc::MODIFY_STROKE;
            ui.painter().rect_stroke(
                square.shrink(w / 2.0),
                pc::MODIFY_CORNER - w / 2.0,
                Stroke::new(w, if modify_lit { argb(pc::ACCENT) } else { argb(pc::SIDE_GLYPH) }),
                egui::StrokeKind::Middle,
            );
            for (icon, enabled, y, undo) in
                [(&icons::UNDO, info.can_undo, 430.5, true), (&icons::REDO, info.can_redo, 472.5, false)]
            {
                let rect = at(5.0, y - shorter * 2.0, 40.0, 41.0);
                let tapped = ui.interact(rect, ui.scope_id().with(("side", undo)), Sense::click()).clicked();
                if tapped && enabled {
                    if undo {
                        taps.undo = true;
                    } else {
                        taps.redo = true;
                    }
                }
                // a 50 frame shown 36 across; turning on or off (a stroke kept, the last step
                // undone) fades over 0.15 s either way, linear, as measured: the lit glyph over
                // the unlit one
                let lit = ui.ctx().animate_bool_with_time(ui.scope_id().with(("side lit", undo)), enabled, 0.15);
                if lit < 1.0 {
                    look.icon(ui, icon, rect.center(), 36.0, argb(pc::SIDE_GLYPH_DISABLED));
                }
                if lit > 0.0 {
                    look.icon(ui, icon, rect.center(), 36.0, argb(pc::SIDE_GLYPH).gamma_multiply(lit));
                }
            }
        });
        if let Some((which, kept)) = self.panel {
            let y = if which == Slider::Size { SIZE_TRACK_TOP } else { opacity_top };
            let left = x(SLIDER_LEFT + pc::SLIDER_WIDTH + SLIDER_PANEL_GAP, SLIDER_PANEL_SIDE);
            let min = bar.min + vec2(left, y - SLIDER_PANEL_RISE);
            let rect = Rect::from_min_size(min, vec2(SLIDER_PANEL_SIDE, SLIDER_PANEL_SIDE));
            self.changed |= slider_panel(&ctx, look, session, which, rect, info, backdrops);
            // kept up, it goes at the next press anywhere but on it and its slider
            let pressed_elsewhere = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p) && !bar.contains(p))
            });
            if kept && pressed_elsewhere {
                self.panel = None;
            }
        }
        taps.brush_changed = std::mem::take(&mut self.changed);
        taps
    }

    /// An upright slider moved by dragging anywhere on it: the thumb follows the pointer (not
    /// jumping to it), 1 at the top. Moved, its panel shows while it moves; held still, it stays
    /// up. Its notches (saved values) show as marks on the track; a value dragged near one is
    /// pulled onto it, and a tap on one goes there. On a notch, the thumb shows a blue bar.
    fn slider(&mut self, ui: &Ui, look: &mut Look, session: &CanvasSession, which: Slider, rect: Rect, value: f32) {
        let size = which == Slider::Size;
        let travel = rect.height() - pc::THUMB_HEIGHT - pc::THUMB_INSET * 2.0;
        let response = ui.interact(rect, ui.scope_id().with(("slider", which)), Sense::click_and_drag());
        let set = |v: f32| match which {
            Slider::Size => session.set_brush_size(v),
            Slider::Opacity => session.set_brush_opacity(v),
        };
        if response.is_pointer_button_down_on() && self.pressed.is_none_or(|p| p.which != which) {
            self.pressed = Some(Press { which, raw: value, since: std::time::Instant::now(), moved: false });
        }
        if let Some(press) = self.pressed.as_mut().filter(|p| p.which == which) {
            let dy = response.drag_delta().y;
            if dy != 0.0 {
                if !press.moved {
                    self.panel = Some((which, false));
                }
                press.moved = true;
                press.raw = (press.raw - dy / travel).clamp(0.0, 1.0);
                set(session.slider_notch_at(size, press.raw).unwrap_or(press.raw));
            }
        }
        // a tap on a notch's mark goes to it
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let at = 1.0 - (p.y - rect.top() - pc::THUMB_INSET - pc::THUMB_HEIGHT / 2.0) / travel;
            let reach = pc::THUMB_HEIGHT / 2.0 / travel;
            let notch = session
                .slider_notches(size)
                .into_iter()
                .filter(|n| (n - at).abs() < reach)
                .min_by(|a, b| (a - at).abs().total_cmp(&(b - at).abs()));
            if let Some(n) = notch {
                set(n);
            }
        }
        if !response.is_pointer_button_down_on() && self.pressed.is_some_and(|p| p.which == which) {
            // let go after moving, the panel goes (one held up stays)
            if self.pressed.is_some_and(|p| p.moved) && self.panel == Some((which, false)) {
                self.panel = None;
            }
            self.pressed = None;
            self.changed = true;
        }
        let notches = session.slider_notches(size);
        let on_notch = notches.iter().any(|n| (n - value).abs() < 1e-4);
        let slot = match which {
            Slider::Size => "size slider",
            Slider::Opacity => "opacity slider",
        };
        let key =
            ((value * 10_000.0).round() as i32, notches.iter().map(|n| (n * 10_000.0) as i32).collect::<Vec<_>>());
        look.shape(ui, slot, key, rect, |pixmap, t| {
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
            // the notches: short bars across the track
            let bar = vec2(NOTCH_WIDTH, NOTCH_HEIGHT);
            let travel = size.y - thumb.y - inset * 2.0;
            for n in &notches {
                let y = inset + thumb.y / 2.0 + travel * (1.0 - n);
                let mut mark = tiny_skia::PathBuilder::new();
                round_rect(&mut mark, Rect::from_center_size(pos2(size.x / 2.0, y), bar), [bar.y / 2.0; 4]);
                fill(pixmap, mark.finish(), argb(NOTCH_MARK), t);
            }
            let mut thumb_path = tiny_skia::PathBuilder::new();
            round_rect(&mut thumb_path, knob, [thumb.y / 2.0; 4]);
            fill(pixmap, thumb_path.finish(), argb(pc::SLIDER_THUMB), t);
            // on a notch, the thumb shows a blue bar
            if on_notch {
                let mut mark = tiny_skia::PathBuilder::new();
                round_rect(&mut mark, Rect::from_center_size(knob.center(), bar), [bar.y / 2.0; 4]);
                fill(pixmap, mark.finish(), argb(pc::ACCENT), t);
            }
        });
    }
}

/// A slider's panel: its name and value ("Size 8%"), a button saving the value as a notch on the
/// slider ("+"; "-" takes away the notch it is on), and in the middle the brush's shape (a dot for
/// a round tip) as it would paint now, its size on the screen (and at its opacity).
fn slider_panel(
    ctx: &egui::Context,
    look: &mut Look,
    session: &CanvasSession,
    which: Slider,
    rect: Rect,
    info: &CanvasInfo,
    backdrops: &mut Vec<f32>,
) -> bool {
    let mut toggled = false;
    let ppp = ctx.pixels_per_point();
    let corner = 22.0;
    frost(backdrops, rect, corner, ppp);
    egui::Area::new(egui::Id::unique("slider panel")).fixed_pos(rect.min).order(egui::Order::Foreground).show(
        ctx,
        |ui| {
            ui.allocate_rect(rect, Sense::hover());
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
            // the brush's stamp as it paints (soft where it is soft) in a picture its size on the
            // screen, kept while it stays so
            let side = ((2.0 * r * ppp).ceil() as usize).max(2);
            let stamp = session.brush_stamp(side as u32);
            let key = stamp_key(&stamp, side);
            let id = egui::Id::unique("size preview stamp");
            let kept = ctx.data(|d| d.get_temp::<(u64, egui::TextureHandle)>(id)).filter(|(k, _)| *k == key);
            let texture = kept.map(|(_, t)| t).unwrap_or_else(|| {
                let pixels = stamp.iter().map(|&a| Color32::from_white_alpha(a)).collect();
                let picture = egui::ColorImage { size: [side, side], pixels, ..Default::default() };
                let t = ctx.load_texture("size preview stamp", picture, egui::TextureOptions::LINEAR);
                ctx.data_mut(|d| d.insert_temp(id, (key, t.clone())));
                t
            });
            let shown = Rect::from_center_size(rect.center(), vec2(2.0 * r, 2.0 * r));
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), shown, uv, Color32::WHITE.gamma_multiply(alpha));
            let size = which == Slider::Size;
            let value = if size { info.size } else { info.opacity };
            let on_notch = session.slider_notch_at(size, value).is_some();
            if on_notch || session.can_save_slider_notch(size) {
                let button = Rect::from_min_size(rect.min + vec2(150.0, 6.0), vec2(32.0, 32.0));
                let icon = if on_notch { &icons::NOTCH_REMOVE } else { &icons::NOTCH_ADD };
                look.icon(ui, icon, button.center(), 24.0, argb(SLIDER_PANEL_INK));
                if ui.interact(button, ui.scope_id().with("notch"), Sense::click()).clicked() {
                    session.toggle_slider_notch(size);
                    toggled = true;
                }
            }
        },
    );
    toggled
}

/// What a stamp's picture (`side` pixels square) is known by, to make its texture once.
fn stamp_key(stamp: &[u8], side: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    side.hash(&mut h);
    stamp.hash(&mut h);
    h.finish()
}
