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
            self.press =
                Some(Press { id, origin, since: Instant::now(), held: false, dragging: false, avoid, drag: None });
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
        let dragging_from = self.press.as_ref().filter(|p| p.dragging).map(|p| p.origin);
        if let Some((at, rgb)) = self.dot {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("colour drop dot")));
            let colour = srgb(of_hex(rgb));
            match dragging_from {
                Some(from) => {
                    // pulled out of the button: its own dot at one end, and the colour at the other,
                    // which starts at its full size and shrinks away as it is carried off
                    if self.blob.last.is_none() {
                        self.blob.start(from);
                    }
                    let pull = ((from.distance(at) - 40.0) / 80.0).clamp(0.0, 1.0);
                    // the button's end is 28 across, the colour's 60, both by the 1.3 the display's
                    // own text size asks for; only the colour's shrinks as it is carried off
                    let r0 = 28.0 * 1.3;
                    let r1 = 60.0 * 1.3 * (1.0 - pull);
                    let now = ctx.input(|i| i.time);
                    self.blob.step(from, at, r0, r1, now);
                    self.blob.paint(&painter, r0, r1, colour);
                    ctx.request_repaint();
                }
                None => {
                    painter.circle_filled(at, DOT / 2.0, colour);
                }
            }
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

/// The colour pulled out of a button: the button's circle and a second one out along the pull, joined
/// by a neck while the pull is short and coming apart when it is long, both of them wobbling.
struct Blob {
    /// The pointer's end as it lags along behind it.
    current: Pos2,
    velocity: Vec2,
    /// The two ends as they are drawn this frame.
    near: Pos2,
    far: Pos2,
    /// How far the pull has gone, 0 where it began, 1 stretched as far as it goes.
    spring: f32,
    /// How far the two have parted, 1 being clean apart.
    separation: f32,
    /// The wobble's phase and how much of it there is.
    jiggle: f32,
    momentum: f32,
    /// When it was last stepped, in seconds.
    last: Option<f64>,
}

impl Default for Blob {
    fn default() -> Blob {
        Blob {
            current: Pos2::ZERO,
            velocity: vec2(0.0, 0.0),
            near: Pos2::ZERO,
            far: Pos2::ZERO,
            spring: 0.0,
            separation: 0.0,
            jiggle: 0.0,
            momentum: 0.0,
            last: None,
        }
    }
}

impl Blob {
    /// Starts afresh at the button it is pulled out of.
    fn start(&mut self, at: Pos2) {
        *self = Blob { current: at, near: at, far: at, ..Blob::default() };
    }

    /// One frame of it, `now` in seconds.
    fn step(&mut self, origin: Pos2, destination: Pos2, r0: f32, r1: f32, now: f64) {
        let dt = match self.last {
            Some(was) => ((now - was) as f32).clamp(0.0, 0.05),
            None => 0.0,
        };
        self.last = Some(now);
        let d = origin.distance(destination);
        // how far the pull stretches the thing before the neck is at its worst
        let len = (r0 + (r0 + r1) * 0.33).max(1.0);
        let stretched = (d / len).min(1.0);
        let (mut half, mut past) = ((d / (len * 2.0)).min(1.0), ((d - len * 2.0) / 250.0).clamp(0.0, 1.0));
        let prev = self.spring;
        if stretched - prev <= 0.0 {
            // coming back in, the same terms the other way about
            half = 1.0 - half;
            past = 1.0 - past;
        }
        let rate = (prev * prev * half).max(prev * past);
        let rate = (rate * 0.2).max(0.02);
        let frame = dt * 120.0;
        self.spring = (stretched * frame * rate + prev * (1.0 - frame * rate)).clamp(0.0, 1.0);
        // near the end of the stretch the neck gives way, and the two part
        self.separation = if self.spring > 0.9 {
            let free = (1.0 - self.separation).max(0.0);
            (self.separation + (frame * free).min(0.1)).clamp(0.0, 1.0)
        } else if self.spring < 0.8 {
            let keep = (frame * self.separation - 0.01).max(0.5);
            (self.separation * keep).clamp(0.0, 1.0)
        } else {
            self.separation
        };
        // how hard the pointer is swinging the colour about decides how much it wobbles
        let swung = ((destination - self.current).length() / 20.0).min(1.0);
        self.momentum = (swung * 0.005 + swung.max(self.momentum) * 0.995).clamp(0.0, 1.0);
        self.jiggle += frame * (1.0 / 120.0) * self.momentum * 2.0;
        // the pointer's end lags along behind the pointer
        let follow = frame * 0.08;
        self.velocity = (self.velocity * (1.0 - follow) + (destination - self.current) * follow) * 0.9;
        self.current += self.velocity;
        // the two ends drawn: the button's, and out along the pull as far as it has come
        let along = (destination - origin).normalized() * (len * stretched);
        let lean = (self.spring * std::f32::consts::PI).sin() * 0.1;
        self.near = Pos2::new(
            (origin.x + along.x) * lean + origin.x * (1.0 - lean),
            (origin.y + along.y) * lean + origin.y * (1.0 - lean),
        );
        let give = self.spring.powf(1.5);
        self.far =
            Pos2::new(origin.x * (1.0 - give) + self.current.x * give, origin.y * (1.0 - give) + self.current.y * give);
    }

    fn paint(&self, painter: &egui::Painter, r0: f32, r1: f32, colour: Color32) {
        let (near_r, far_r) = (r0 / 2.0, r1 / 2.0);
        // how far apart the two ends have come, as a share of what the pull can reach
        let reach = (r0 + (r0 + r1) * 0.33).max(1.0);
        let apart = (self.near.distance(self.far) / reach).min(1.0);
        let early = self.separation + (1.0 - self.separation) * ((apart - 0.75) / 0.25).clamp(0.0, 1.0);
        let late = self.separation + (1.0 - self.separation) * (apart / 0.75).min(1.0);
        let blend = (early + late) * 0.5;
        let radius = near_r * (1.0 - blend) + blend * far_r;
        let amp = self.momentum * 0.15 * (20.0 / r1.max(1.0));
        // apart, they are two circles; while they hold on, one shape with a neck between. All three
        // have to have given way, not just one: the stretch alone doesn't part them, the spring does.
        if early.min(late).min(self.separation) >= 1.0 {
            blob_circle(painter, self.near, near_r, self.jiggle, amp, colour);
            blob_circle(painter, self.far, radius, self.jiggle, amp, colour);
            return;
        }
        blob_circle(painter, self.near, near_r, self.jiggle, amp, colour);
        let d = self.near.distance(self.far);
        if d > 0.25 {
            let perp = vec2(-(self.far - self.near).y, (self.far - self.near).x) / d;
            // the neck between them: the two of them less as they are pulled, and gone once they have
            // parted - widest at the circles and holding its width between, a collar rather than a waist
            let give = 1.0 - early;
            let half = give * (1.0 - late.powi(3)) * (near_r + radius) * 0.5;
            if half > 0.15 {
                const SAMPLES: usize = 12;
                let mut mesh = egui::epaint::Mesh::default();
                for i in 0..=SAMPLES {
                    let t = i as f32 / SAMPLES as f32;
                    let at = self.near.lerp(self.far, t);
                    mesh.colored_vertex(at + perp * half, colour);
                    mesh.colored_vertex(at - perp * half, colour);
                    if let Some(last) = (i as u32).checked_sub(1) {
                        let (lo, hi) = (last * 2, last * 2 + 2);
                        mesh.add_triangle(lo, lo + 1, hi);
                        mesh.add_triangle(lo + 1, hi + 1, hi);
                    }
                }
                painter.add(egui::Shape::mesh(mesh));
            }
        }
        blob_circle(painter, self.far, radius, self.jiggle, amp, colour);
    }
}

/// A circle drawn out of true, the way a drop of liquid is while it moves: squashed one way, drawn
/// the other, at two different rates so it never settles into looking like a disc.
fn blob_circle(painter: &egui::Painter, at: Pos2, radius: f32, phase: f32, amount: f32, colour: Color32) {
    if radius <= 0.2 {
        return;
    }
    let across = (1.0 - amount) + amount * (phase * 4.0 * std::f32::consts::PI).cos();
    let down = (1.0 - amount) + amount * (phase * 3.0 * std::f32::consts::PI).sin();
    const STEPS: usize = 32;
    let mut points = Vec::with_capacity(STEPS);
    for i in 0..STEPS {
        let turn = i as f32 / STEPS as f32 * std::f32::consts::TAU;
        let (s, c) = turn.sin_cos();
        points.push(at + vec2(c * radius * across, s * radius * down));
    }
    painter.add(egui::Shape::convex_polygon(points, colour, Stroke::NONE));
}
