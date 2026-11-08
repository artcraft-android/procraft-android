//! ColorDrop, as the Android app's (ui/ColorDropUi.kt): a colour dragged from the colour button
//! (or a palette's swatch) and let go on the canvas fills the area around where it lands. The band
//! under the top bar then offers, for a moment, to continue filling: each touch on the canvas
//! fills (the shared touches do that), one slid across the screen changing the threshold, until
//! the band's tick. Held still a moment while dragged, the colour comes to rest where it is and a
//! slide sets the threshold, the fill shown as it goes (the core's `ColourDrag` says when).

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use procraft_core::{CanvasSession, ColourDrag, DragStep, Fill};

use crate::colours::{of_hex, srgb};
use crate::look::{Themed, Weight, argb, pc, text};
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
const BAND_FILL: Themed = Themed(0xD63B3B3B, 0xD6EEEEEE);
const ROW_FILL: Themed = Themed(0xCC4A4A4A, 0xCCDCDCDC);
const INK: Themed = Themed(0xFFC5C5C5, 0xFF222222);

/// How long a colour let go takes to shrink away where it landed.
const DROP_AWAY: f32 = 0.1;

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
#[derive(Clone)]
struct Press {
    id: Id,
    origin: Pos2,
    /// The middle of the swatch it came from, which is where the bar draws it and so where the drop
    /// is drawn from - the press itself can land anywhere on it.
    centre: Pos2,
    /// It came out of the top bar's colour button rather than a palette, so the drop draws that
    /// button's end as well as its own.
    bar: bool,
    since: Instant,
    held: bool,
    dragging: bool,
    /// Where it may be let go without filling (the panel it came from).
    avoid: Option<Rect>,
    /// The colour being dragged out: where it is, and whether it has come to rest.
    drag: Option<Arc<ColourDrag>>,
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
    /// The dragged colour came to rest and its slid threshold began filling (or was refused).
    poured: bool,
    refused: bool,
    /// A clock for the fill coming in to run against.
    clock: Instant,
    /// The colour being pulled out of the button.
    blob: Blob,
    /// A colour just let go on the artwork: where, which, and when - it shrinks away as it fills.
    dropped: Option<(Pos2, u32, Instant)>,
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
            poured: false,
            refused: false,
            clock: Instant::now(),
            blob: Blob::default(),
            dropped: None,
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
        if response.clicked() && self.press.as_ref().is_none_or(|p| p.id != id) {
            return Some(Touch::Tap);
        }
        if self.press.is_none()
            && response.is_pointer_button_down_on()
            && let Some(origin) = at
        {
            self.press = Some(Press {
                id,
                origin,
                centre: rect.center(),
                bar: !swatch,
                since: Instant::now(),
                held: false,
                dragging: false,
                avoid,
                drag: None,
            });
        }
        let press = self.press.as_mut().filter(|p| p.id == id)?;
        let at = at.unwrap_or(press.origin);
        if !down {
            let (held, dragging) = (press.held, press.dragging);
            if dragging {
                self.let_go(ctx, session, Some(at));
            } else {
                self.press = None;
            }
            return (!held && !dragging).then_some(Touch::Tap);
        }
        if press.dragging {
            let (drag, avoid) = (press.drag.clone(), press.avoid);
            let ms = ctx.input(|i| i.time) * 1000.0;
            let step = drag.as_ref().map_or(
                DragStep { dot_x: at.x, dot_y: at.y, resting: false, threshold: self.threshold, slid: false },
                |d| d.moved(at.x, at.y, ms),
            );
            let dot = pos2(step.dot_x, step.dot_y);
            // held still a moment on the artwork, off the bars, it comes to rest and fills there at
            // once (a slide then sets the threshold)
            let mut rested = false;
            if let Some((drag, due)) = drag.as_ref().and_then(|d| Some((d, d.rest_due()?))) {
                if ms >= due {
                    let ppp = ctx.pixels_per_point();
                    rested = drag.rest(
                        ms,
                        dot.y > pc::BAR_HEIGHT
                            && !avoid.is_some_and(|a| a.contains(dot))
                            && session.pour_lands(dot.x * ppp, dot.y * ppp),
                    );
                } else {
                    ctx.request_repaint_after(Duration::from_secs_f64((due - ms) / 1000.0));
                }
            }
            if rested {
                self.pour(ctx, session, dot, rgb, self.threshold);
            }
            // poured, the fill shows instead of the dot
            self.dot = (!self.poured).then_some((dot, rgb));
            if step.slid {
                self.slide(ctx, session, step, rgb);
            }
            return None;
        }
        if press.held {
            return None;
        }
        if at.distance(press.origin) > SLOP {
            press.dragging = true;
            self.blob.start(press.centre);
            // a mouse or a pen: a little wobble still rests
            press.drag = Some(ColourDrag::new(at.x, at.y, ctx.input(|i| i.time) * 1000.0, true, self.threshold));
            self.poured = false;
            self.refused = false;
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
        let press = self.press.take();
        let avoid = press.as_ref().and_then(|p| p.avoid);
        // come to rest, it fills where it rests (already, if its threshold was slid)
        let rested = press.and_then(|p| p.drag).map(|d| d.now()).filter(|s| s.resting).map(|s| pos2(s.dot_x, s.dot_y));
        let dot = self.dot.take();
        if std::mem::take(&mut self.poured) {
            session.pour_end(true, None);
            self.sliding = false;
            self.band = Band::Dropped(Instant::now());
            return;
        }
        let at =
            at.map(|a| rested.unwrap_or(a)).filter(|p| p.y > pc::BAR_HEIGHT && !avoid.is_some_and(|a| a.contains(*p)));
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

    /// The dragged colour, at rest, slid its threshold: the fill shown with it.
    fn slide(&mut self, ctx: &egui::Context, session: &CanvasSession, step: DragStep, rgb: u32) {
        self.threshold = step.threshold;
        self.sliding = true;
        if self.poured {
            session.pour_threshold(step.threshold);
        } else {
            self.pour(ctx, session, pos2(step.dot_x, step.dot_y), rgb, step.threshold);
        }
    }

    /// The dragged colour fills where it rests, at `at` (points), unless that was refused already
    /// (a locked layer, text).
    fn pour(&mut self, ctx: &egui::Context, session: &CanvasSession, at: Pos2, rgb: u32, threshold: f32) {
        if self.refused {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let [r, g, b] = of_hex(rgb);
        self.poured = session.pour_begin(at.x * ppp, at.y * ppp, r, g, b, threshold);
        self.refused = !self.poured;
        // let go on the artwork, the drop itself shrinks away as the fill goes in
        if self.poured {
            self.dropped = Some((at, rgb, Instant::now()));
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
    /// Continue filling is on: the touches fill.
    pub fn filling(&self) -> bool {
        self.band == Band::Filling
    }

    pub fn end_filling(&mut self) {
        self.band = Band::None;
        self.sliding = false;
    }

    /// Whether the drag is drawing the bar's colour dot itself this frame, so the bar leaves it be.
    pub fn swatch_away(&self) -> bool {
        self.press.as_ref().is_some_and(|press| press.dragging && press.bar) || self.blob.swing.length() > HOME
    }

    /// Draws the band hanging from `top` and the dragged colour's dot; keeps the touches filling
    /// with the colour (`current`, unless a swatch was dropped) while filling goes on.
    pub fn show(&mut self, ctx: &egui::Context, session: &CanvasSession, current: u32, top: f32) {
        // a fill coming in where it was dropped draws itself a frame at a time until it is all there
        if session.pour_spreading() {
            session.pour_tick(self.clock.elapsed().as_secs_f64() * 1000.0);
            ctx.request_repaint();
        }
        // a press whose colour went away (a swatch taken out, a panel closed) ends with the pointer
        if self.press.is_some() && !ctx.input(|i| i.pointer.primary_down()) {
            let at = ctx.input(|i| i.pointer.interact_pos());
            if self.press.as_ref().is_some_and(|p| p.dragging) {
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
        // a colour just let go shrinks away where it landed, the fill going in as it does
        if let Some((at, rgb, since)) = self.dropped {
            let gone = since.elapsed().as_secs_f32() / DROP_AWAY;
            if gone >= 1.0 {
                self.dropped = None;
            } else {
                let scale = 1.0 - (1.0 - 0.025) * gone;
                let painter =
                    ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("colour drop dot")));
                painter.circle_filled(at, DOT / 2.0 * scale, srgb(of_hex(rgb)));
                ctx.request_repaint();
            }
        }
        let dragging_from = self.press.as_ref().filter(|p| p.dragging).map(|p| (p.centre, p.bar));
        if let Some((at, rgb)) = self.dot {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("colour drop dot")));
            let colour = srgb(of_hex(rgb));
            match dragging_from {
                Some((from, bar)) => {
                    // pulled out: both ends are drawn here, off this frame's finger, so their points
                    // come to the same place - the bar is a frame behind whatever it is given
                    // its radius, not its across: the colour never comes up bigger than the swatch
                    let radius = pc::COLOR_DOT / 2.0;
                    self.blob.step(from, at, radius, ctx.input(|i| i.time), bar);
                    if bar {
                        self.blob.paint_swatch(&painter, radius, colour);
                        self.blob.paint(&painter, colour);
                    } else {
                        // a palette draws its own swatch and it does not bend, so no point here
                        painter.circle_filled(at, self.blob.drawn, colour);
                    }
                    ctx.request_repaint();
                }
                None => {
                    painter.circle_filled(at, DOT / 2.0, colour);
                }
            }
        } else if self.blob.swing.length() > HOME {
            // let go of while the swatch was still on its way after it, it finishes the way home on
            // its own: the bar has stopped drawing it, so it cannot be dropped mid-journey
            let now = ctx.input(|i| i.time);
            self.blob.settle(self.blob.last.map_or(0.0, |was| ((now - was) as f32).clamp(0.0, 0.05)));
            self.blob.last = Some(now);
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("colour drop dot")));
            painter.circle_filled(self.blob.swatch_at(), pc::COLOR_DOT / 2.0, srgb(of_hex(current)));
            ctx.request_repaint();
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

/// How far it can be pulled before the two let go of each other, as a share of the swatch's across.
const REACH: f32 = 1.8;
/// How far each point runs past the middle of the gap while the two are still close, as a share of
/// its own size. Crossed like this they read as one body, not as two shapes meeting on a point.
const OVERLAP: f32 = 0.65;
/// How much of the pull, at its far end, the crossing takes to close up. Shorter than this and the
/// crossing is at its fullest, so the two only come onto a point in the last stretch before parting.
const CLOSING: f32 = 0.22;
/// How far the swatch in the bar is dragged after the pointer while the two hold on, as a share of
/// the gap between them, and the most it ever goes.
const SWAY: f32 = 0.21;
const SWAY_MAX: f32 = 14.0;
/// Under this the swatch is back on the bar's own dot, which draws it again.
const HOME: f32 = 0.3;
/// How fast the colour has to be going, in points a second, before it is drawn right out of shape,
/// and how far out of shape that draws it: that much shorter along the way it is going, and drawn
/// out across it.
const SPEED: f32 = 1800.0;
const STRETCH: f32 = 0.35;
/// How hard the colour is pulled after the pointer once it is free of the swatch, and how hard that
/// pull is damped. Underdamped on purpose: whirled about, it swings wide of the pointer and settles
/// back, where a plain follow would only ever ride round inside it.
const SPRING: f32 = 256.0;
const DAMPING: f32 = 10.0;
/// What the dragged one is worth once they are apart, against the swatch's size.
const SETTLED: f32 = 0.72;
/// How thin it gets while it is still holding on.
const THINNEST: f32 = 0.55;

/// The colour pulled out of a button: the swatch itself, and the colour drawn out of it, each turned
/// into a teardrop pointing at the other while they hold together, and back to circles once they part.
struct Blob {
    /// Where the swatch sits when it is not being pulled after the pointer.
    home: Pos2,
    /// How far it is dragged off that, eased, so it springs back to it when the two let go.
    swing: Vec2,
    /// Where the swatch is.
    near: Pos2,
    /// Where the colour is.
    far: Pos2,
    /// How far it has to be pulled before the two let go, which is also where the crossing between
    /// them closes up to nothing.
    reach: f32,
    /// How much is left of them holding together: 1 together, 0 apart, between while they let go.
    link: f32,
    /// Whether they are still holding on, which is what keeps the colour from lagging behind.
    holding: bool,
    /// Its own velocity once it is free of the swatch, which is what gives it weight.
    velocity: Vec2,
    /// The way it is going, in radians, and how far that has drawn it out of shape.
    heading: f32,
    stretch: f32,
    /// The dragged one's radius, eased.
    drawn: f32,
    /// When it was last stepped, in seconds.
    last: Option<f64>,
}

impl Default for Blob {
    fn default() -> Blob {
        Blob {
            home: Pos2::ZERO,
            swing: Vec2::ZERO,
            near: Pos2::ZERO,
            far: Pos2::ZERO,
            reach: 0.0,
            link: 1.0,
            holding: true,
            velocity: Vec2::ZERO,
            heading: 0.0,
            stretch: 0.0,
            drawn: 0.0,
            last: None,
        }
    }
}

impl Blob {
    /// Starts afresh at the button it is pulled out of.
    fn start(&mut self, at: Pos2) {
        *self = Blob { home: at, near: at, far: at, drawn: 0.0, ..Blob::default() };
    }

    /// One frame of it, `now` in seconds: how far it has been pulled, how much is left of the two
    /// holding together, how big the dragged one is, and how far the swatch has been dragged after
    /// the pointer.
    fn step(&mut self, origin: Pos2, destination: Pos2, r0: f32, now: f64, sways: bool) {
        let dt = match self.last {
            Some(was) => ((now - was) as f32).clamp(0.0, 0.05),
            None => 0.0,
        };
        self.last = Some(now);
        let d = origin.distance(destination);
        let reach = r0 * 2.0 * REACH;
        self.reach = reach;
        // dragged a little after the pointer while the two hold on, and let go back home once they
        // part, so it reads as the swatch in the bar being pulled out
        let sway = match sways {
            true => (destination - origin) * ((d * SWAY).min(SWAY_MAX) / d.max(0.001)) * self.link,
            false => Vec2::ZERO,
        };
        self.swing += (sway - self.swing) * (dt * 12.0).clamp(0.0, 1.0);
        self.near = origin + self.swing;
        // where the colour is drawn: on the pointer while the two hold on, and after that a mass on
        // the end of that pull, carrying its own velocity - so whirled about it swings wide of the
        // pointer rather than riding round inside it
        if self.holding {
            self.far = destination;
            self.velocity = Vec2::ZERO;
        } else {
            self.velocity += ((destination - self.far) * SPRING - self.velocity * DAMPING) * dt;
            self.far += self.velocity * dt;
        }
        // the rest is measured from where the swatch and the colour actually are, so the two part
        // when the gap as drawn reaches the length they can hold - which is the same gap that
        // leaves their tips just meeting
        let gap = self.near.distance(self.far);
        let pulled = (gap / reach).clamp(0.0, 1.0);
        // it thins as it is pulled, and comes up a little again once it is free, never as big as
        // the swatch it came from
        let wanted = if gap < reach { r0 * (1.0 - (1.0 - THINNEST) * pulled) } else { r0 * SETTLED };
        self.drawn += (wanted - self.drawn) * (dt * 14.0).clamp(0.0, 1.0);
        let link = if gap < reach { 1.0 } else { 0.0 };
        // the point it had goes quickly once they part: what is left of it is a ball of liquid
        let given_up = match link > 0.0 {
            true => 18.0,
            false => 42.0,
        };
        self.link += (link - self.link) * (dt * given_up).clamp(0.0, 1.0);
        self.holding = gap < reach;
        // free of the swatch it is a ball of liquid, squashed along the way it is going by how fast
        // that is and drawn out across it - but only once its point has gone, so the shape it is
        // already drawing does not jump when it changes
        let speed = self.velocity.length();
        let stretch = match self.holding || self.tail(self.drawn) > 0.05 {
            true => 0.0,
            false => (speed / SPEED).min(STRETCH),
        };
        self.stretch += (stretch - self.stretch) * (dt * 20.0).clamp(0.0, 1.0);
        if speed > 1.0 {
            let turn = (self.velocity.angle() - self.heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            self.heading += turn * (dt * 14.0).clamp(0.0, 1.0);
        }
    }

    /// How far the end of `radius` is drawn out. It runs past the middle of the gap by more the
    /// closer the two of them are, so near the swatch they cross into each other rather than meeting
    /// on a point, and by nothing at all at the length where they let go - which is the only moment
    /// the two points come onto each other exactly.
    fn tail(&self, radius: f32) -> f32 {
        let gap = self.near.distance(self.far);
        let slack = ((1.0 - gap / self.reach.max(1.0)) / CLOSING).clamp(0.0, 1.0);
        (gap * 0.5 - radius + radius * OVERLAP * slack).max(0.0) * self.link
    }

    /// The colour pulled out: its point back toward the swatch while they hold on, and a ball of
    /// liquid drawn out of true once it is free of it.
    fn paint(&self, painter: &egui::Painter, colour: Color32) {
        let tail = self.tail(self.drawn);
        match tail > 0.05 {
            true => teardrop(painter, self.far, self.drawn, self.near, tail, colour),
            false => squashed(painter, self.far, self.drawn, self.heading, self.stretch, colour),
        }
    }

    /// The swatch it came out of, its point toward the colour. Drawn from here rather than by the
    /// bar, which would be handing this frame's finger to a frame that has already gone.
    fn paint_swatch(&self, painter: &egui::Painter, radius: f32, colour: Color32) {
        teardrop(painter, self.near, radius, self.far, self.tail(radius), colour);
    }

    /// Takes the swatch the rest of the way home once no colour is out of it any more - it is let go
    /// of mid-pull, so it still has some of the way to go.
    fn settle(&mut self, dt: f32) {
        self.swing *= 1.0 - (dt * 12.0).clamp(0.0, 1.0);
    }

    /// Where the swatch is drawn: off its own place while it is being pulled after, back on it after.
    fn swatch_at(&self) -> Pos2 {
        self.home + self.swing
    }
}

/// A ball drawn out of true: that much shorter along `heading`, and drawn out across it, the way a
/// drop of liquid is while it is being thrown about.
fn squashed(painter: &egui::Painter, at: Pos2, radius: f32, heading: f32, stretch: f32, colour: Color32) {
    if radius <= 0.2 {
        return;
    }
    let (sin, cos) = heading.sin_cos();
    let (along, across) = (radius * (1.0 - stretch * 0.7), radius * (1.0 + stretch));
    const STEPS: usize = 48;
    let mut points = Vec::with_capacity(STEPS);
    for i in 0..STEPS {
        let (s, c) = (std::f32::consts::TAU * (i as f32 / STEPS as f32)).sin_cos();
        let (x, y) = (c * along, s * across);
        points.push(at + vec2(x * cos - y * sin, x * sin + y * cos));
    }
    painter.add(egui::Shape::convex_polygon(points, colour, Stroke::NONE));
}

/// A drop: the ball of `radius` at `at`, drawn out to a point `tail` beyond it toward `toward`. The
/// point is where the two lines off the ball, run straight in to it, meet - so the outline comes to
/// a corner there, and it is that corner that reads as a drop rather than a nose on a ball.
fn teardrop(painter: &egui::Painter, at: Pos2, radius: f32, toward: Pos2, tail: f32, colour: Color32) {
    if radius <= 0.2 {
        return;
    }
    let off = toward - at;
    let span = off.length();
    // nothing drawn out of it, or nothing to draw it toward: just the ball
    if span < 0.01 || tail <= 0.05 {
        painter.circle_filled(at, radius, colour);
        return;
    }
    let dir = off / span;
    let out = radius + tail;
    // how far round the ball the point is seen to sit, which is where its two lines touch
    let edge = (radius / out).clamp(-1.0, 1.0).acos();
    let (sin, cos) = dir.angle().sin_cos();
    const STEPS: usize = 64;
    let mut points = Vec::with_capacity(STEPS + 2);
    // the ball from one of those lines round the far side to the other, then the point itself
    for i in 0..=STEPS {
        let a = edge + (std::f32::consts::TAU - 2.0 * edge) * (i as f32 / STEPS as f32);
        let (s, c) = a.sin_cos();
        points.push(at + vec2(c * cos - s * sin, c * sin + s * cos) * radius);
    }
    points.push(at + dir * out);
    // ball and point together are convex, so it fills in one go, and smoothly along that outline
    painter.add(egui::Shape::convex_polygon(points, colour, Stroke::NONE));
}
