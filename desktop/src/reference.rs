//! The reference window, as the Android app's (ui/ReferenceUi.kt): the whole canvas as it is, or a
//! picture to work from, floating over the canvas. Its top bar drags it; the round button closes
//! it (Reference off); unseen handles over its bottom corners size it.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2, pos2, vec2};
use procraft_core::CanvasSession;

use crate::icons;
use crate::look::{Look, Weight, argb, pc, text};

const BAR: u32 = 0xFF393939;
const PICTURE: u32 = 0xFF262626;
const GRAB_LINE: u32 = 0xFF7C7C7C;
const TITLE: u32 = 0xFFC5C5C5;
const CLOSE: u32 = 0xFF262626;
const CLOSE_EDGE: u32 = 0xFF454545;
const CLOSE_INK: u32 = 0xFFA5A5A5;
const SEGMENT_TRACK: u32 = 0xFF4D4D4D;
const MUTED: u32 = 0xFFAAAAAA;
const TOP_BAR: f32 = 64.0;
const BOTTOM_BAR: f32 = 45.0;
const CORNER: f32 = 24.0;
const HANDLE: f32 = 60.0;
/// Its picture's size at first, the least and most it is sized to, and how far up from the
/// screen's bottom it first sits.
const START: Vec2 = Vec2 { x: 350.0, y: 423.0 };
const LEAST: f32 = 150.0;
const MOST: f32 = 700.0;
const ABOVE_BOTTOM: f32 = 50.0;
/// The canvas is read again at most this often while it changes, this large.
const REFRESH: Duration = Duration::from_millis(300);
const CANVAS_PIXELS: u32 = 700;
/// A picture imported is kept at most this large.
const MOST_PICTURE: u32 = 2048;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Canvas,
    Image,
    Face,
}

pub struct Reference {
    pub on: bool,
    mode: Mode,
    /// Where the window is (its top left, points), once placed, and its picture's size.
    at: Option<Pos2>,
    size: Vec2,
    /// The canvas as last read (and its edits then, and when), and the picture imported.
    canvas: Option<TextureHandle>,
    canvas_edits: Option<u64>,
    read_at: Option<Instant>,
    image: Option<TextureHandle>,
}

impl Default for Reference {
    fn default() -> Reference {
        Reference {
            on: false,
            mode: Mode::Canvas,
            at: None,
            size: START,
            canvas: None,
            canvas_edits: None,
            read_at: None,
            image: None,
        }
    }
}

impl Reference {
    fn read_canvas(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let edits = session.edits();
        if self.canvas_edits == Some(edits) {
            return;
        }
        if let Some(at) = self.read_at
            && at.elapsed() < REFRESH
        {
            ctx.request_repaint_after(REFRESH - at.elapsed());
            return;
        }
        if let Some(t) = session.canvas_thumbnail(CANVAS_PIXELS) {
            let image = egui::ColorImage::from_rgba_premultiplied([t.width as usize, t.height as usize], &t.rgba);
            self.canvas = Some(ctx.load_texture("reference canvas", image, egui::TextureOptions::LINEAR));
            self.canvas_edits = Some(edits);
            self.read_at = Some(Instant::now());
        }
    }

    fn import(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import Image")
            .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"])
            .pick_file()
        else {
            return;
        };
        match image::open(path) {
            Ok(picture) => {
                let picture = if picture.width().max(picture.height()) > MOST_PICTURE {
                    picture.thumbnail(MOST_PICTURE, MOST_PICTURE)
                } else {
                    picture
                };
                let rgba = picture.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.image = Some(ctx.load_texture("reference picture", image, egui::TextureOptions::LINEAR));
            }
            Err(_) => crate::files::tell("Can't import it", "This picture can't be opened."),
        }
    }

    /// Shows the window while Reference is on.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) {
        if !self.on {
            return;
        }
        if self.mode == Mode::Canvas {
            self.read_canvas(ctx, session);
        }
        let screen = ctx.content_rect();
        let tall = TOP_BAR + self.size.y + BOTTOM_BAR;
        // the first time: in the middle near the bottom of the screen
        let at = *self.at.get_or_insert_with(|| {
            pos2(screen.center().x - self.size.x / 2.0, (screen.bottom() - ABOVE_BOTTOM - tall).max(screen.top()))
        });
        let window = Rect::from_min_size(at, vec2(self.size.x, tall));
        // room round it for the handles, which reach past its corners
        let reach = window.expand2(vec2(HANDLE / 2.0 - 12.0, 0.0)).with_max_y(window.bottom() + HANDLE / 2.0 - 15.0);
        let mut close = false;
        let mut import = false;
        egui::Area::new(Id::unique("reference")).fixed_pos(reach.min).order(Order::Middle).show(ctx, |ui| {
            // allocated, so the canvas under it takes no press
            ui.allocate_rect(reach, Sense::hover());
            let painter = ui.painter();
            let shadow =
                egui::epaint::Shadow { offset: [0, 4], blur: 36, spread: 0, color: Color32::from_black_alpha(0x55) };
            painter.add(shadow.as_shape(window, CORNER));
            painter.rect_filled(window, CORNER, argb(PICTURE));
            // the title bar: dragged to move the window
            let bar = Rect::from_min_size(window.min, vec2(window.width(), TOP_BAR));
            let corners = egui::CornerRadius { nw: CORNER as u8, ne: CORNER as u8, sw: 0, se: 0 };
            painter.rect_filled(bar, corners, argb(BAR));
            painter.rect_filled(
                Rect::from_center_size(pos2(bar.center().x, bar.top() + 6.0), vec2(40.0, 4.0)),
                2.0,
                argb(GRAB_LINE),
            );
            text(
                painter,
                pos2(bar.left() + 20.0, bar.center().y),
                Align2::LEFT_CENTER,
                "Reference",
                22.0,
                Weight::Bold,
                argb(TITLE),
            );
            let dragged = ui.interact(bar, ui.scope_id().with("move"), Sense::drag());
            if dragged.dragged() {
                self.at = Some(at + dragged.drag_delta());
            }
            let x_button = Rect::from_center_size(pos2(bar.right() - 10.0 - 21.5, bar.center().y), vec2(43.0, 43.0));
            painter.circle_filled(x_button.center(), 21.5, argb(CLOSE));
            painter.circle_stroke(x_button.center(), 21.0, Stroke::new(1.0, argb(CLOSE_EDGE)));
            look.icon(ui, &icons::CROSS, x_button.center(), 20.0, argb(CLOSE_INK));
            close = ui.interact(x_button, ui.scope_id().with("close"), Sense::click()).clicked();
            if self.mode == Mode::Image && self.image.is_some() {
                let mut right = x_button.left() - 6.0;
                for (label, id) in [("Import", "import"), ("Clear", "clear")] {
                    let shown = text(
                        ui.painter(),
                        pos2(right, bar.center().y),
                        Align2::RIGHT_CENTER,
                        label,
                        15.0,
                        Weight::Regular,
                        argb(pc::ACCENT),
                    );
                    if ui.interact(shown.expand(6.0), ui.scope_id().with(id), Sense::click()).clicked() {
                        if id == "import" {
                            import = true;
                        } else {
                            self.image = None;
                        }
                    }
                    right = shown.left() - 12.0;
                }
            }
            // the picture
            let area = Rect::from_min_size(pos2(window.left(), bar.bottom()), self.size);
            let shown = match self.mode {
                Mode::Canvas => self.canvas.as_ref(),
                Mode::Image => self.image.as_ref(),
                Mode::Face => None,
            };
            if let Some(t) = shown {
                let [w, h] = t.size().map(|v| v.max(1) as f32);
                let k = (area.width() / w).min(area.height() / h);
                let picture = Rect::from_center_size(area.center(), vec2(w * k, h * k));
                ui.painter().image(t.id(), picture, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            } else if self.mode == Mode::Image {
                let button = Rect::from_center_size(area.center(), vec2(180.0, 60.0));
                ui.painter().rect_filled(button, 30.0, argb(SEGMENT_TRACK));
                text(
                    ui.painter(),
                    button.center(),
                    Align2::CENTER_CENTER,
                    "Import Image",
                    16.0,
                    Weight::Regular,
                    argb(MUTED),
                );
                import |= ui.interact(button, ui.scope_id().with("import image"), Sense::click()).clicked();
            } else if self.mode == Mode::Face {
                text(
                    ui.painter(),
                    area.center(),
                    Align2::CENTER_CENTER,
                    "Not available on this device",
                    14.0,
                    Weight::Regular,
                    argb(MUTED),
                );
            }
            // the bottom bar: Canvas, Image and Face
            let bottom = Rect::from_min_size(pos2(window.left(), area.bottom()), vec2(window.width(), BOTTOM_BAR));
            let corners = egui::CornerRadius { nw: 0, ne: 0, sw: CORNER as u8, se: CORNER as u8 };
            ui.painter().rect_filled(bottom, corners, argb(BAR));
            let track = Rect::from_center_size(bottom.center(), vec2(270.0, 35.0));
            ui.painter().rect_filled(track, 17.5, argb(SEGMENT_TRACK));
            let item = (track.width() - 4.0 - 22.0) / 3.0;
            for (i, (mode, label)) in
                [(Mode::Canvas, "Canvas"), (Mode::Image, "Image"), (Mode::Face, "Face")].into_iter().enumerate()
            {
                let cell = Rect::from_min_size(track.min + vec2(2.0 + (item + 11.0) * i as f32, 2.0), vec2(item, 31.0));
                let on = self.mode == mode;
                if on {
                    ui.painter().rect_filled(cell, 15.5, argb(pc::ACCENT));
                }
                let ink = if on { Color32::WHITE } else { argb(TITLE) };
                text(ui.painter(), cell.center(), Align2::CENTER_CENTER, label, 15.0, Weight::Regular, ink);
                if ui.interact(cell, ui.scope_id().with(("mode", i)), Sense::click()).clicked() {
                    self.mode = mode;
                    self.canvas_edits = None;
                }
            }
            // the size handles, unseen, centred 12 in from the window's bottom corners and 15 up
            for left in [true, false] {
                let x = if left { window.left() + 12.0 } else { window.right() - 12.0 };
                let handle = Rect::from_center_size(pos2(x, window.bottom() - 15.0), vec2(HANDLE, HANDLE));
                let drag = ui.interact(handle, ui.scope_id().with(("size", left)), Sense::drag());
                if drag.dragged() {
                    let d = drag.drag_delta();
                    let old = self.size;
                    let w = (old.x + if left { -d.x } else { d.x }).clamp(LEAST, MOST);
                    let h = (old.y + d.y).clamp(LEAST, MOST);
                    if left {
                        self.at = Some(at + vec2(old.x - w, 0.0));
                    }
                    self.size = vec2(w, h);
                }
            }
        });
        if close {
            self.on = false;
        }
        if import {
            self.import(ctx);
        }
    }
}
