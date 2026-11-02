//! The card over everything while an export runs, as the Android app's (ui/AlertUi.kt
//! `ExportProgress`): "Exporting" after the share glyph, a pie filling clockwise from the top with
//! how far it has come, and Cancel; once made the pie is full and a check is drawn in it, then the
//! card goes. It comes and goes as an alert does, the window dimmed, nothing under it touched.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use egui::emath::TSTransform;
use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::{Align2, Color32, Id, LayerId, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, pos2, vec2};

use crate::look::{Look, Themed, Weight, argb, cubic_bezier, pc, text};

// as measured: 398 x 253, its one button 17 in from the sides and 48 tall, the pie 76 across (its
// ring 4 wide) 40 under the title's row and 22 over the button, 15 under that
const WIDTH: f32 = 398.0;
const HEIGHT: f32 = 21.0 + 31.0 + 40.0 + 76.0 + 22.0 + 48.0 + 15.0;
const SIDE: f32 = 17.0;
const PIE: f32 = 76.0;
const RING: f32 = 4.0;
const BUTTON_HEIGHT: f32 = 48.0;
const FILL: Themed = Themed(0xF21E1E1E, 0xF2E3E3E3);
const EDGE: u32 = 0x24FFFFFF;
const INK: Themed = Themed(0xFFC5C5C5, 0xFF222222);
const CANCEL_FILL: Themed = Themed(0xFF2D2D2D, 0xFFD9D9D9);
const CANCEL_INK: Themed = Themed(0xFFFFFFFF, 0xFF000000);
const DIM: f32 = 0.7;
const START_SCALE: f32 = 1.1;
const IN: f32 = 0.3;
const OUT: f32 = 0.35;
/// The check, through these points about the pie's middle, 6 wide with round ends; drawn along
/// its length over this long, the card going this long after it was made.
const CHECK: [(f32, f32); 3] = [(-14.0, 1.0), (-4.0, 11.0), (16.0, -10.0)];
const CHECK_WIDTH: f32 = 6.0;
const CHECK_DRAW: f32 = 0.3;
/// How long after it was made a finished export's card goes (as measured).
pub const DONE: Duration = Duration::from_millis(450);

const RUNNING: u8 = 0;
const MADE: u8 = 1;
const NOT_MADE: u8 = 2;

/// What the export's own thread is told and tells: whether Cancel was pressed, and its end.
#[derive(Clone)]
pub struct Exporting {
    cancel: Arc<AtomicBool>,
    made: Arc<AtomicU8>,
}

impl Exporting {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// The export ended: `made` (the card shows its check) or not (cancelled or failed).
    pub fn ended(&self, made: bool) {
        self.made.store(if made { MADE } else { NOT_MADE }, Ordering::Relaxed);
    }
}

/// How an export's card ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    Made,
    NotMade,
}

struct Run {
    progress: Box<dyn Fn() -> f32>,
    exporting: Exporting,
    since: Instant,
    made: Option<Instant>,
    closing: Option<(Instant, Ended)>,
}

#[derive(Default)]
pub struct ExportCard {
    run: Option<Run>,
}

impl ExportCard {
    /// Shows the card for an export whose progress (0..1) `progress` tells; its thread is handed
    /// the [`Exporting`] to read Cancel from and to end it by.
    pub fn start(&mut self, progress: impl Fn() -> f32 + 'static) -> Exporting {
        let exporting = Exporting { cancel: Arc::new(AtomicBool::new(false)), made: Arc::new(AtomicU8::new(RUNNING)) };
        self.run = Some(Run {
            progress: Box::new(progress),
            exporting: exporting.clone(),
            since: Instant::now(),
            made: None,
            closing: None,
        });
        exporting
    }

    /// An export's card is up (another isn't started over it).
    pub fn busy(&self) -> bool {
        self.run.is_some()
    }

    /// Draws it while it shows; returns how it ended once it has gone.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look) -> Option<Ended> {
        let run = self.run.as_mut()?;
        ctx.request_repaint();
        // the thread's end: made, the check, then the card goes; else it goes at once
        match run.exporting.made.load(Ordering::Relaxed) {
            MADE if run.made.is_none() => run.made = Some(Instant::now()),
            NOT_MADE if run.closing.is_none() => run.closing = Some((Instant::now(), Ended::NotMade)),
            _ => {}
        }
        if let (Some(made), None) = (run.made, run.closing)
            && made.elapsed() >= DONE
        {
            run.closing = Some((Instant::now(), Ended::Made));
        }
        let shown = match run.closing {
            Some((since, _)) => 1.0 - cubic_bezier(0.0, 0.0, 0.58, 1.0, since.elapsed().as_secs_f32() / OUT),
            None => cubic_bezier(0.42, 0.0, 0.58, 1.0, run.since.elapsed().as_secs_f32() / IN),
        };
        if let Some((_, ended)) = run.closing
            && shown <= 0.0
        {
            self.run = None;
            return Some(ended);
        }
        let screen = ctx.content_rect();
        let card = Rect::from_center_size(screen.center(), vec2(WIDTH, HEIGHT));
        let area = Id::unique("export card");
        let k = START_SCALE + (1.0 - START_SCALE) * shown;
        let c = card.center().to_vec2();
        let transform =
            TSTransform::from_translation(c) * TSTransform::from_scaling(k) * TSTransform::from_translation(-c);
        ctx.set_transform_layer(LayerId::new(Order::Debug, area), transform);
        let progress = (run.progress)().clamp(0.0, 1.0);
        let check = run.made.map(|m| (m.elapsed().as_secs_f32() / CHECK_DRAW).clamp(0.0, 1.0));
        let mut cancel = false;
        egui::Area::new(area).fixed_pos(screen.min).order(Order::Debug).show(ctx, |ui| {
            // nothing under it is touched while it is up
            ui.allocate_rect(screen, Sense::click_and_drag());
            ui.interact(card, ui.scope_id().with("card"), Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0, Color32::from_black_alpha((DIM * shown * 255.0) as u8));
            ui.multiply_opacity(shown);
            let painter = ui.painter();
            painter.rect_filled(card, 24, argb(FILL));
            painter.rect_stroke(card, 24, Stroke::new(1.0, argb(EDGE)), StrokeKind::Inside);
            // the title after the share glyph (22 tall: 18.6 of its 50 frame), 13 between
            let title_y = card.top() + 21.0 + 31.0 / 2.0;
            let ink = argb(INK);
            let words = painter.layout_no_wrap("Exporting".into(), Weight::Bold.font(26.0), ink).size().x;
            let left = card.center().x - (22.0 + 13.0 + words) / 2.0;
            look.icon(ui, &crate::icons::TAB_SHARE, pos2(left + 11.0, title_y), 59.0, argb(pc::ACCENT));
            text(ui.painter(), pos2(left + 35.0, title_y), Align2::LEFT_CENTER, "Exporting", 26.0, Weight::Bold, ink);
            let centre = pos2(card.center().x, card.top() + 21.0 + 31.0 + 40.0 + PIE / 2.0);
            let accent = argb(pc::ACCENT);
            let painter = ui.painter();
            match check {
                Some(drawn) => {
                    painter.circle_filled(centre, PIE / 2.0, accent);
                    check_mark(painter, centre, drawn);
                }
                None => {
                    painter.circle_stroke(centre, PIE / 2.0 - RING / 2.0, Stroke::new(RING, accent));
                    painter.add(wedge(centre, PIE / 2.0, progress, accent));
                }
            }
            let button = Rect::from_min_size(
                pos2(card.left() + SIDE, card.bottom() - 15.0 - BUTTON_HEIGHT),
                vec2(WIDTH - SIDE * 2.0, BUTTON_HEIGHT),
            );
            ui.painter().rect_filled(button, BUTTON_HEIGHT / 2.0, argb(CANCEL_FILL));
            text(
                ui.painter(),
                button.center(),
                Align2::CENTER_CENTER,
                "Cancel",
                17.0,
                Weight::Medium,
                argb(CANCEL_INK),
            );
            if run.closing.is_none()
                && run.made.is_none()
                && ui.interact(button, ui.scope_id().with("cancel"), Sense::click()).clicked()
            {
                cancel = true;
            }
        });
        if cancel {
            run.exporting.cancel.store(true, Ordering::Relaxed);
            run.closing = Some((Instant::now(), Ended::NotMade));
        }
        None
    }
}

/// The pie's filled part: `done` of a full turn clockwise from the top, as a fan of triangles.
fn wedge(centre: Pos2, r: f32, done: f32, color: Color32) -> Shape {
    let mut mesh = Mesh::default();
    if done <= 0.0 {
        return Shape::Noop;
    }
    let steps = ((done * 96.0).ceil() as u32).max(1);
    mesh.colored_vertex(centre, color);
    for i in 0..=steps {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * done * i as f32 / steps as f32;
        mesh.colored_vertex(centre + vec2(a.cos(), a.sin()) * r, color);
        if i > 0 {
            mesh.add_triangle(0, i, i + 1);
        }
    }
    Shape::mesh(mesh)
}

/// The check drawn along `drawn` (0..1) of its length, white, with round ends and corner.
fn check_mark(painter: &egui::Painter, centre: Pos2, drawn: f32) {
    if drawn <= 0.0 {
        return;
    }
    let pts: Vec<Pos2> = CHECK.iter().map(|&(x, y)| centre + vec2(x, y)).collect();
    let legs: Vec<(Pos2, Pos2)> = pts.iter().copied().zip(pts.iter().copied().skip(1)).collect();
    let mut left = legs.iter().map(|(a, b)| a.distance(*b)).sum::<f32>() * drawn;
    let mut line: Vec<Pos2> = pts.first().copied().into_iter().collect();
    for (a, b) in legs {
        let len = a.distance(b);
        if left >= len {
            line.push(b);
            left -= len;
        } else {
            line.push(a + (b - a) * (left / len.max(1e-3)));
            break;
        }
    }
    painter.add(PathShape::line(line.clone(), PathStroke::new(CHECK_WIDTH, Color32::WHITE)));
    for p in line {
        painter.circle_filled(p, CHECK_WIDTH / 2.0, Color32::WHITE);
    }
}
