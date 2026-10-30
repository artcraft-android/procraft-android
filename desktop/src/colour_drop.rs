//! ColorDrop, as the Android app's (ui/ColorDropUi.kt): a colour dragged from the colour button
//! (or a palette's swatch) and let go on the canvas fills the area around where it lands. The band
//! under the top bar then offers, for a moment, to continue filling: each touch on the canvas
//! fills (the shared touches do that), one slid across the screen changing the threshold, until
//! the band's tick.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use procraft_core::{CanvasSession, Fill};

use crate::colours::{of_hex, srgb};
use crate::look::{Weight, argb, pc, text};
use crate::overlays::ease;

/// A press moved this far (points) drags the colour out.
const SLOP: f32 = 8.0;
/// A press held still this long is a hold.
const HOLD: Duration = Duration::from_millis(400);
/// The dragged colour's dot, across.
const DOT: f32 = 55.5;
/// How long the band offers to continue filling after a drop.
const OFFERED: Duration = Duration::from_millis(2500);
const SLIDE: f32 = 0.3;
const BAND_HEIGHT: f32 = 32.0;
const ROW_HEIGHT: f32 = 24.0;
const LINE_HEIGHT: f32 = 4.0;
/// The band, translucent over the canvas, and the threshold's row under it.
const BAND_FILL: u32 = 0xD63B3B3B;
const ROW_FILL: u32 = 0xCC4A4A4A;
const INK: u32 = 0xFFC5C5C5;

/// What the band under the top bar shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Band {
    None,
    Dragging,
    /// A colour was let go and filled (then): the band offers to continue filling.
    Dropped(Instant),
    Filling,
}

/// What a press on a colour that can be dragged out turned out to be, for its owner (a drag is
/// ColorDrop's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touch {
    Tap,
    Hold,
    /// It began being dragged out.
    Dragged,
}

/// A press on a colour that can be dragged out: where and when it went down, and what it has
/// turned out to be so far.
#[derive(Clone, Copy, Debug)]
struct Press {
    id: Id,
    origin: Pos2,
    since: Instant,
    held: bool,
    dragging: bool,
    /// Where it may be let go without filling (the panel it came from).
    avoid: Option<Rect>,
}

pub struct ColourDrop {
    press: Option<Press>,
    band: Band,
    /// What the band showed, kept while it slides away.
    shown: Band,
    /// The dragged colour's dot (points) and its colour (0xRRGGBB).
    dot: Option<(Pos2, u32)>,
    /// The swatch's colour when a swatch was dropped (filling goes on with it); else the current
    /// colour.
    swatch: Option<u32>,
    /// The threshold the colour fills with (0..1), kept from one drop to the next.
    threshold: f32,
    /// The threshold is being slid: its row shows under the band.
    sliding: bool,
}

impl Default for ColourDrop {
    fn default() -> ColourDrop {
        ColourDrop {
            press: None,
            band: Band::None,
            shown: Band::None,
            dot: None,
            swatch: None,
            threshold: 0.5,
            sliding: false,
        }
    }
}

impl ColourDrop {
    /// A colour that can be dragged out at `rect`: the colour button, or a swatch (`swatch`) of
    /// colour `rgb`. Tapped, or held still when `holds`, it says so; dragged, its colour goes with
    /// the pointer as a dot and fills where it is let go below the top bar (and off `avoid`).
    #[allow(clippy::too_many_arguments)]
    pub fn source(
        &mut self,
        ui: &Ui,
        session: &CanvasSession,
        id: Id,
        rect: Rect,
        rgb: u32,
        swatch: bool,
        holds: bool,
        avoid: Option<Rect>,
    ) -> Option<Touch> {
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let ctx = ui.ctx();
        let (at, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        // pressed and let go between two frames: a tap never seen down
        if response.clicked() && self.press.is_none_or(|p| p.id != id) {
            return Some(Touch::Tap);
        }
        if self.press.is_none()
            && response.is_pointer_button_down_on()
            && let Some(origin) = at
        {
            self.press = Some(Press { id, origin, since: Instant::now(), held: false, dragging: false, avoid });
        }
        let press = self.press.as_mut().filter(|p| p.id == id)?;
        let at = at.unwrap_or(press.origin);
        if !down {
            let Press { held, dragging, .. } = *press;
            if dragging {
                self.let_go(ctx, session, Some(at));
            } else {
                self.press = None;
            }
            return (!held && !dragging).then_some(Touch::Tap);
        }
        if press.dragging {
            self.dot = Some((at, rgb));
            return None;
        }
        if press.held {
            return None;
        }
        if at.distance(press.origin) > SLOP {
            press.dragging = true;
            self.swatch = swatch.then_some(rgb);
            self.dot = Some((at, rgb));
            self.band = Band::Dragging;
            return Some(Touch::Dragged);
        }
        if holds {
            let held_for = press.since.elapsed();
            if held_for >= HOLD {
                press.held = true;
                return Some(Touch::Hold);
            }
            ctx.request_repaint_after(HOLD - held_for);
        }
        None
    }

    /// The dragged colour let go at `at` (points): below the top bar and off the panel it came
    /// from, it fills there and the band offers to go on filling for a moment.
    fn let_go(&mut self, ctx: &egui::Context, session: &CanvasSession, at: Option<Pos2>) {
        let avoid = self.press.take().and_then(|p| p.avoid);
        let dot = self.dot.take();
        let at = at.filter(|p| p.y > pc::BAR_HEIGHT && !avoid.is_some_and(|a| a.contains(*p)));
        let poured = match (at, dot) {
            (Some(at), Some((_, rgb))) => {
                let ppp = ctx.pixels_per_point();
                let [r, g, b] = of_hex(rgb);
                session.pour_begin(at.x * ppp, at.y * ppp, r, g, b, self.threshold)
            }
            _ => false,
        };
        if poured {
            session.pour_end(true, None);
            self.band = Band::Dropped(Instant::now());
        } else {
            self.band = Band::None;
        }
    }

    /// Continue filling's touch slid the threshold to `threshold`.
    pub fn slid(&mut self, threshold: f32) {
        self.threshold = threshold;
        self.sliding = true;
    }

    /// Continue filling's touch let go.
    pub fn filled(&mut self) {
        self.sliding = false;
    }

    /// Filling ends (the artwork closed, another mode chosen).
    pub fn end_filling(&mut self) {
        self.band = Band::None;
        self.sliding = false;
    }

    /// Draws the band hanging from `top` and the dragged colour's dot; keeps the touches filling
    /// with the colour (`current`, unless a swatch was dropped) while filling goes on.
    pub fn show(&mut self, ctx: &egui::Context, session: &CanvasSession, current: u32, top: f32) {
        // a press whose colour went away (a swatch taken out, a panel closed) ends with the pointer
        if self.press.is_some() && !ctx.input(|i| i.pointer.primary_down()) {
            let at = ctx.input(|i| i.pointer.interact_pos());
            if self.press.is_some_and(|p| p.dragging) {
                self.let_go(ctx, session, at);
            } else {
                self.press = None;
            }
        }
        if let Band::Dropped(since) = self.band {
            match OFFERED.checked_sub(since.elapsed()) {
                Some(left) if !left.is_zero() => ctx.request_repaint_after(left),
                _ => self.band = Band::None,
            }
        }
        let fill = (self.band == Band::Filling).then(|| {
            let [r, g, b] = of_hex(self.swatch.unwrap_or(current));
            Fill { r, g, b, threshold: self.threshold }
        });
        session.set_touch_filling(fill);
        if self.band != Band::None {
            self.shown = self.band;
        }
        if let Some((at, rgb)) = self.dot {
            ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("colour drop dot"))).circle_filled(
                at,
                DOT / 2.0,
                srgb(of_hex(rgb)),
            );
        }
        let out = ease(ctx.animate_bool_with_time(Id::unique("colour drop band"), self.band != Band::None, SLIDE));
        if out <= 0.0 {
            return;
        }
        let window = ctx.content_rect();
        let what = self.shown;
        let lined = matches!(what, Band::Dragging | Band::Filling);
        let height = BAND_HEIGHT + if self.sliding { ROW_HEIGHT } else { 0.0 } + if lined { LINE_HEIGHT } else { 0.0 };
        let y = top - (1.0 - out) * height;
        egui::Area::new(Id::unique("colour drop band"))
            .fixed_pos(pos2(window.left(), y))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                ui.set_clip_rect(Rect::from_min_max(pos2(window.left(), top), window.max));
                let band = Rect::from_min_size(pos2(window.left(), y), vec2(window.width(), BAND_HEIGHT));
                ui.allocate_rect(band, Sense::click());
                let painter = ui.painter();
                painter.rect_filled(band, 0, argb(BAND_FILL));
                let ink = argb(INK);
                let blue = argb(pc::ACCENT);
                // "ColorDrop", then the tick that ends filling or the offer to continue
                let name = painter.layout_no_wrap("ColorDrop".into(), Weight::Bold.font(16.5), ink).size().x;
                let button = vec2(if what == Band::Filling { 20.0 } else { 101.0 }, 20.0);
                let left = band.center().x - (name + 13.0 + button.x) / 2.0;
                text(painter, pos2(left, band.center().y), Align2::LEFT_CENTER, "ColorDrop", 16.5, Weight::Bold, ink);
                let button = Rect::from_min_size(pos2(left + name + 13.0, band.center().y - button.y / 2.0), button);
                if what == Band::Filling {
                    painter.circle_filled(button.center(), 10.0, blue);
                    let c = button.center();
                    // the tick's ends rounded
                    let (start, end) = (c + vec2(-4.2, 0.1), c + vec2(4.0, -2.8));
                    painter.circle_filled(start, 1.0, Color32::WHITE);
                    painter.circle_filled(end, 1.0, Color32::WHITE);
                    let tick = vec![start, c + vec2(-1.4, 2.6), end];
                    painter.add(egui::Shape::line(tick, Stroke::new(2.0, Color32::WHITE)));
                } else {
                    painter.rect_filled(button, 10, blue);
                    text(
                        painter,
                        button.center(),
                        Align2::CENTER_CENTER,
                        "Continue filling",
                        11.7,
                        Weight::Regular,
                        Color32::WHITE,
                    );
                }
                if ui.interact(button, ui.scope_id().with("colour drop button"), Sense::click()).clicked() {
                    if what == Band::Filling {
                        self.end_filling();
                    } else if matches!(self.band, Band::Dropped(_)) {
                        self.band = Band::Filling;
                    }
                }
                let mut below = band.bottom();
                if self.sliding {
                    let row = Rect::from_min_size(pos2(window.left(), below), vec2(window.width(), ROW_HEIGHT));
                    ui.painter().rect_filled(row, 0, argb(ROW_FILL));
                    let reading = format!("Threshold {}%", (self.threshold * 100.0).round() as i32);
                    text(ui.painter(), row.center(), Align2::CENTER_CENTER, &reading, 14.4, Weight::Regular, ink);
                    below = row.bottom();
                }
                if lined {
                    let line = Rect::from_min_size(
                        pos2(window.left(), below),
                        vec2(window.width() * self.threshold, LINE_HEIGHT),
                    );
                    ui.painter().rect_filled(line, 0, blue);
                }
            });
        if out < 1.0 {
            ctx.request_repaint();
        }
    }
}
