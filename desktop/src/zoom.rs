//! The gallery zooming into an artwork as it opens and out of it as it closes, as the Android
//! app's (ui/ZoomUi.kt). Opening, the gallery is drawn over the canvas: the artwork's picture grows
//! to where its canvas is, and the gallery fades over the last part, showing the canvas and its
//! bars. Closing, the gallery starts with the picture where the canvas was and settles back, as a
//! picture of the canvas fades away over it.

use std::time::{Duration, Instant};

use egui::emath::TSTransform;
use egui::{Color32, Id, Order, Pos2, Rect, TextureHandle, Vec2, pos2, vec2};
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
    canvas: Place,
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
            canvas: canvas_place(session, &matrix, scale),
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
        let canvas = canvas_place(session, &session.view_matrix(), ctx.pixels_per_point());
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

    /// Where the picture is now (its middle, its size as drawn, its turn in radians and whether it
    /// shows mirrored), and what puts the gallery so its picture lands there.
    fn placed(&self) -> (Place, TSTransform) {
        let e = self.progress();
        // from the cell (0) towards the canvas (1)
        let t = if self.opening { e } else { 1.0 - e };
        let full = if self.cell.height() > 0.0 { self.canvas.size.y / self.cell.height() } else { 1.0 };
        let k = 1.0 + (full - 1.0) * t;
        let centre = self.cell.center().lerp(self.canvas.centre, t);
        // the shorter way round
        let angle = (self.canvas.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let at =
            Place { centre, size: self.cell.size() * k, angle: angle * t, mirrored: self.canvas.mirrored && t > 0.5 };
        (at, TSTransform::new(centre.to_vec2() - self.cell.center().to_vec2() * k, k))
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
        // a quad turned about its middle, its picture's left and right swapped when mirrored
        let (sin, cos) = at.angle.sin_cos();
        let corner = |x: f32, y: f32| {
            let (dx, dy) = (x * at.size.x / 2.0, y * at.size.y / 2.0);
            at.centre + vec2(dx * cos - dy * sin, dx * sin + dy * cos)
        };
        let (left, right) = if at.mirrored { (1.0, 0.0) } else { (0.0, 1.0) };
        let colour = Color32::WHITE.gamma_multiply(alpha);
        let mut mesh = egui::Mesh::with_texture(picture.id());
        for (x, y, u, v) in
            [(-1.0, -1.0, left, 0.0), (1.0, -1.0, right, 0.0), (1.0, 1.0, right, 1.0), (-1.0, 1.0, left, 1.0)]
        {
            mesh.vertices.push(egui::epaint::Vertex { pos: corner(x, y), uv: pos2(u, v), color: colour });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }

    /// Ended: the gallery back in its place.
    pub fn end(ctx: &egui::Context) {
        ctx.set_transform_layer(crate::gallery::Gallery::layer(), TSTransform::IDENTITY);
    }
}

/// Where a canvas is on screen (points): its middle, its size as drawn (not turned), how far its
/// view turns it (radians, clockwise on screen) and whether it shows mirrored.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    centre: Pos2,
    size: Vec2,
    angle: f32,
    mirrored: bool,
}

/// The canvas on screen (points) as `matrix` (window pixels, as `view_matrix` gives it) shows it.
fn canvas_place(session: &CanvasSession, matrix: &[f32], scale: f32) -> Place {
    let info = session.info();
    place_of((info.width as f32, info.height as f32), matrix, scale)
}

/// A canvas `size` pixels as `matrix` shows it, in points of `scale` pixels.
fn place_of((w, h): (f32, f32), matrix: &[f32], scale: f32) -> Place {
    let [a, b, c, d, e, f] =
        matrix.get(..6).and_then(|m| <[f32; 6]>::try_from(m).ok()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let centre = pos2((a * w / 2.0 + c * h / 2.0 + e) / scale, (b * w / 2.0 + d * h / 2.0 + f) / scale);
    // its down axis gives its scale and turn whether or not it is mirrored (a mirror turns its
    // across axis round)
    let k = c.hypot(d) / scale;
    Place { centre, size: vec2(w * k, h * k), angle: (-c).atan2(d), mirrored: a * d - b * c < 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turned_or_mirrored_canvas_is_placed_by_its_middle_size_and_turn() {
        // upright, half size, moved
        let p = place_of((200.0, 100.0), &[0.5, 0.0, 0.0, 0.5, 10.0, 20.0], 1.0);
        assert_eq!((p.centre, p.size, p.angle, p.mirrored), (pos2(60.0, 45.0), vec2(100.0, 50.0), 0.0, false));
        // a quarter turn clockwise: its down axis points left; points of two pixels
        let p = place_of((200.0, 100.0), &[0.0, 1.0, -1.0, 0.0, 300.0, 0.0], 2.0);
        assert!((p.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6, "{}", p.angle);
        assert_eq!((p.centre, p.size, p.mirrored), (pos2(125.0, 50.0), vec2(100.0, 50.0), false));
        // mirrored across, upright
        let p = place_of((200.0, 100.0), &[-1.0, 0.0, 0.0, 1.0, 200.0, 0.0], 1.0);
        assert_eq!((p.centre, p.size, p.angle, p.mirrored), (pos2(100.0, 50.0), vec2(200.0, 100.0), 0.0, true));
    }
}
