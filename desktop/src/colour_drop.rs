//! ColorDrop, as the Android app's (ui/ColorDropUi.kt): a colour dragged from the colour button
//! (or a palette's swatch) and let go on the canvas fills the area around where it lands. The band
//! under the top bar then offers, for a moment, to continue filling: each touch on the canvas
//! fills (the shared touches do that), one slid across the screen changing the threshold, until
//! the band's tick. Held still a moment while dragged, the colour comes to rest where it is and a
//! slide sets the threshold, the fill shown as it goes (the core's `ColourDrag` says when).
//! Dragged, the colour is drawn as the core's `ColourBlob` works it out: a drop pulled out of the
//! colour button that lets go and follows as a ball. Its fill spreads in (the core's
//! `pour_drop`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use procraft_core::{BlobShape, CanvasSession, ColourBlob, ColourDrag, DragStep, Fill};

use crate::colours::{of_hex, srgb};
use crate::look::{Themed, Weight, argb, pc, text};
use crate::overlays::ease;

/// A press moved this far (points) drags the colour out.
const SLOP: f32 = 8.0;
/// A press held still this long is a hold.
const HOLD: Duration = Duration::from_millis(400);
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
    /// And as it is drawn.
    blob: Option<Arc<ColourBlob>>,
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
    /// The colour button's centre, as last drawn: over the bars a dragged colour shrinks the
    /// further it is from it.
    button: Option<Pos2>,
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
            button: None,
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
        if !swatch {
            self.button = Some(rect.center());
        }
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
                since: Instant::now(),
                held: false,
                dragging: false,
                avoid,
                drag: None,
                blob: None,
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
            let (drag, avoid, blob) = (press.drag.clone(), press.avoid, press.blob.clone());
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
            if let Some(blob) = blob {
                let button = self.button.unwrap_or(dot);
                blob.point_to(dot.x, dot.y, over_bar(ctx, dot), button.x, button.y);
            }
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
            let ms = ctx.input(|i| i.time) * 1000.0;
            // a mouse or a pen: a little wobble still rests
            press.drag = Some(ColourDrag::new(at.x, at.y, ms, true, self.threshold));
            // pulled out of the colour button's centre, or from where a swatch was pressed; a pen
            // moving the pointer drags a pen's dot
            let from = if swatch { press.origin } else { rect.center() };
            let pen = crate::pen::mouse_pen().is_some();
            let blob = ColourBlob::new(from.x, from.y, !swatch, pen, reduce_motion(), ms);
            let button = self.button.unwrap_or(from);
            blob.point_to(at.x, at.y, over_bar(ctx, at), button.x, button.y);
            press.blob = Some(blob);
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
                session.pour_drop(at.x * ppp, at.y * ppp, r, g, b, self.threshold)
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
        self.poured = session.pour_drop(at.x * ppp, at.y * ppp, r, g, b, threshold);
        self.refused = !self.poured;
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
        // the dragged colour, pulled out and carried along as the core works it out
        let blob = self.press.as_ref().filter(|p| p.dragging).and_then(|p| p.blob.clone());
        if let (Some((_, rgb)), Some(blob)) = (self.dot, blob) {
            let shape = blob.shape(ctx.input(|i| i.time) * 1000.0);
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("colour drop dot")));
            paint_blob(&painter, &shape, srgb(of_hex(rgb)), ctx.pixels_per_point());
            // it springs and wobbles on its own
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

/// Whether window point `p` (points) is over one of the bars: the top bar, or the side bar.
fn over_bar(ctx: &egui::Context, p: Pos2) -> bool {
    p.y <= pc::BAR_HEIGHT || crate::panel::side_bar_shown_at(ctx).is_some_and(|bar| bar.contains(p))
}

/// Fills `shape` in `colour`: its triangles, their edges smoothed by its rims drawn a pixel wide.
fn paint_blob(painter: &egui::Painter, shape: &BlobShape, colour: Color32, ppp: f32) {
    let mut mesh = egui::Mesh::default();
    for [ax, ay, bx, by, cx, cy] in shape.triangles.as_chunks::<6>().0 {
        let first = mesh.vertices.len() as u32;
        for (x, y) in [(ax, ay), (bx, by), (cx, cy)] {
            mesh.colored_vertex(pos2(*x, *y), colour);
        }
        mesh.add_triangle(first, first + 1, first + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
    for rim in &shape.rims {
        let points: Vec<Pos2> = rim.points.as_chunks::<2>().0.iter().map(|[x, y]| pos2(*x, *y)).collect();
        if points.len() > 1 {
            painter.add(egui::Shape::line(points, Stroke::new(1.0 / ppp.max(0.1), colour)));
        }
    }
}

/// Whether the system asks for less motion: Windows' animation effects turned off.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn reduce_motion() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut on = windows::core::BOOL(1);
    // reads one BOOL into `on`, which outlives the call
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&raw mut on).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && !on.as_bool()
}

/// Whether the system asks for less motion: not known here.
#[cfg(not(windows))]
pub fn reduce_motion() -> bool {
    false
}
