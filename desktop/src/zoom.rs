//! The gallery zooming into an artwork as it opens and out of it as it closes, as the Android
//! app's (ui/ZoomUi.kt). Opening, the gallery is drawn over the canvas: the artwork's picture grows
//! to where its canvas is, and the gallery fades over the last part, showing the canvas and its
//! bars. Closing, the gallery starts with the picture where the canvas was and settles back, as a
//! picture of the canvas fades away over it.

use std::time::{Duration, Instant};

use egui::emath::TSTransform;
use egui::{Color32, Id, Order, Rect, TextureHandle, pos2};
use procraft_core::CanvasSession;

use crate::look::cubic_bezier;

/// How long opening and closing take.
const OPEN: Duration = Duration::from_millis(400);
const CLOSE: Duration = Duration::from_millis(500);
/// How much of the opening's end the gallery fades over, and how much of the closing's start the
/// canvas does.
const OPEN_FADE: f32 = 0.35;
const CLOSE_FADE: f32 = 0.3;
/// The canvas's picture as it closes, pixels on its longer side.
const PICTURE_SIDE: u32 = 768;

pub struct Zoom {
    /// The artwork's picture in the gallery, and its canvas on screen (points).
    cell: Rect,
    canvas: Rect,
    pub opening: bool,
    started: Instant,
    /// The canvas as it closed.
    picture: Option<TextureHandle>,
}

impl Zoom {
    /// Opening `session` (shown in a window `size` pixels, `scale` pixels a point) from its
    /// picture at `cell`.
    pub fn opening(session: &CanvasSession, cell: Rect, size: (u32, u32), scale: f32) -> Zoom {
        let matrix = session.opening_view_matrix(size.0, size.1);
        Zoom {
            cell,
            canvas: canvas_rect(session, &matrix, scale),
            opening: true,
            started: Instant::now(),
            picture: None,
        }
    }

    /// Closing `session` into its picture at `cell`.
    pub fn closing(ctx: &egui::Context, session: &CanvasSession, cell: Rect) -> Zoom {
        let picture = session.canvas_thumbnail(PICTURE_SIDE).map(|t| {
            let image = egui::ColorImage::from_rgba_premultiplied([t.width as usize, t.height as usize], &t.rgba);
            ctx.load_texture("closing canvas", image, egui::TextureOptions::LINEAR)
        });
        let canvas = canvas_rect(session, &session.view_matrix(), ctx.pixels_per_point());
        Zoom { cell, canvas, opening: false, started: Instant::now(), picture }
    }

    /// Not under way yet (the canvas is still being made): it starts from the gallery again.
    pub fn hold(&mut self) {
        self.started = Instant::now();
    }

    fn length(&self) -> Duration {
        if self.opening { OPEN } else { CLOSE }
    }

    pub fn done(&self) -> bool {
        self.started.elapsed() >= self.length()
    }

    /// How far along it is, eased (0 to 1).
    fn progress(&self) -> f32 {
        let t = self.started.elapsed().as_secs_f32() / self.length().as_secs_f32();
        cubic_bezier(0.42, 0.0, 0.58, 1.0, t)
    }

    /// Where the picture is now, and what puts the gallery so its picture lands there.
    fn placed(&self) -> (Rect, TSTransform) {
        let e = self.progress();
        let (from, to) = if self.opening { (self.cell, self.canvas) } else { (self.canvas, self.cell) };
        let at = Rect::from_min_max(from.min.lerp(to.min, e), from.max.lerp(to.max, e));
        let k = if self.cell.height() > 0.0 { at.height() / self.cell.height() } else { 1.0 };
        (at, TSTransform::new(at.min.to_vec2() - self.cell.min.to_vec2() * k, k))
    }

    /// Opening: the gallery over the canvas, fading over the last part.
    pub fn over_canvas(
        &self,
        ctx: &egui::Context,
        gallery: &mut crate::gallery::Gallery,
        look: &mut crate::look::Look,
    ) {
        let (_, transform) = self.placed();
        let alpha = ((1.0 - self.progress()) / OPEN_FADE).clamp(0.0, 1.0);
        gallery.zooming(ctx, look, transform, alpha);
    }

    /// Closing: the gallery placed (shown after this), the canvas's picture fading over it.
    pub fn over_gallery(&self, ctx: &egui::Context) {
        let (at, transform) = self.placed();
        ctx.set_transform_layer(crate::gallery::Gallery::layer(), transform);
        let Some(picture) = &self.picture else { return };
        let alpha = (1.0 - self.progress() / CLOSE_FADE).clamp(0.0, 1.0);
        let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::unique("closing canvas")));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(picture.id(), at, uv, Color32::WHITE.gamma_multiply(alpha));
    }

    /// Ended: the gallery back in its place.
    pub fn end(ctx: &egui::Context) {
        ctx.set_transform_layer(crate::gallery::Gallery::layer(), TSTransform::IDENTITY);
    }
}

/// The canvas on screen (points) as `matrix` (window pixels, as `view_matrix` gives it) shows it.
fn canvas_rect(session: &CanvasSession, matrix: &[f32], scale: f32) -> Rect {
    let info = session.info();
    let (w, h) = (info.width as f32, info.height as f32);
    let [a, b, c, d, e, f] =
        matrix.get(..6).and_then(|m| <[f32; 6]>::try_from(m).ok()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)]
        .map(|(x, y)| pos2((a * x + c * y + e) / scale, (b * x + d * y + f) / scale));
    Rect::from_points(&corners)
}
