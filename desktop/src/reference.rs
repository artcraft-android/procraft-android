//! The reference window, as the Android app's (ui/ReferenceUi.kt): the whole canvas as it is, or a
//! picture to work from, floating over the canvas. Its grab line drags it; the round button closes
//! it (Reference off); unseen handles over its bottom corners size it. Its bars lie over the
//! picture, and a click on it slides them away and back.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2, pos2, vec2};
use procraft_core::{
    CanvasSession, ReferenceSizes, ReferenceView, reference_picture_point, reference_picture_rect,
    reference_view_pinched,
};

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, pc, text};

// the dark bars are glass, like the panels: what the picture has under them shows through, blurred
const BAR: Themed = Themed(0xDE1E1E1E, 0xFFEDEDED);
const PICTURE: Themed = Themed(0xFF333333, 0xFFF0F0F0);
const GRAB_LINE: Themed = Themed(0x80BFBFBF, 0xFF909090);
const TITLE: Themed = Themed(0xFFC5C5C5, 0xFF3A3A3A);
const CLOSE: Themed = Themed(0x94181818, 0xFFFAFAFA);
const CLOSE_EDGE: Themed = Themed(0x80515151, 0xFFE0E0E0);
const CLOSE_INK: Themed = Themed(0xFFA5A5A5, 0xFF595959);
const SEGMENT_TRACK: Themed = Themed(0x17FFFFFF, 0xFFD7D7D7);
const IMPORT: Themed = Themed(0xFF4D4D4D, 0xFFD7D7D7);
const MUTED: Themed = Themed(0xFFAAAAAA, 0xFF3B3B3B);
const TOP_BAR: f32 = 64.0;
const BOTTOM_BAR: f32 = 45.0;
const CORNER: f32 = 24.0;
const HANDLE: f32 = 60.0;
/// How far down from the window's top a drag moves it, the bars shown or not.
const GRAB: f32 = 16.0;
/// How long the bars take to slide away or back (seconds).
const BARS_SLIDE: f32 = 0.3;
/// How long a second click may take to make two (seconds).
const SECOND_CLICK: Duration = Duration::from_millis(300);
/// How many pixels a picture's longer side has as the bars' glass: drawn back up, that's its blur.
const GLASS_SIDE: usize = 24;
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

/// How far a press may move and still be held still (points), for the eyedropper.
const STILL: f32 = 6.0;

/// A picture's pixels (RGBA), to pick colours from.
struct Pixels {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

/// The picture of `width` x `height` `rgba` pixels made a few pixels across, inside a clear pixel
/// so that its edge blurs too: the bars' glass.
fn glass_of(width: usize, height: usize, rgba: &[u8]) -> egui::ColorImage {
    let scale = GLASS_SIDE as f32 / width.max(height).max(1) as f32;
    let small = |v: usize| ((v as f32 * scale).round() as usize).max(1);
    let (w, h) = (small(width), small(height));
    let mut pixels = vec![Color32::TRANSPARENT; (w + 2) * (h + 2)];
    for y in 0..h {
        for x in 0..w {
            // the mean of the picture's pixels under this one
            let (x0, x1) = (x * width / w, ((x + 1) * width / w).max(x * width / w + 1));
            let (y0, y1) = (y * height / h, ((y + 1) * height / h).max(y * height / h + 1));
            let mut sum = [0u32; 4];
            for row in y0..y1.min(height) {
                let line = rgba.get((row * width + x0) * 4..(row * width + x1.min(width)) * 4).unwrap_or_default();
                for px in line.as_chunks::<4>().0 {
                    for (s, v) in sum.iter_mut().zip(px) {
                        *s += u32::from(*v);
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)).max(1) as u32;
            let [r, g, b, a] = sum.map(|v| (v / n) as u8);
            if let Some(p) = pixels.get_mut((y + 1) * (w + 2) + x + 1) {
                *p = Color32::from_rgba_premultiplied(r, g, b, a);
            }
        }
    }
    egui::ColorImage::new([w + 2, h + 2], pixels)
}

/// What the eyedropper on the window's picture did: moved to window point `at` (points), over
/// `rgb`, showing the `side` x `side` pixels (RGBA) around it; or let go.
pub enum Picked {
    Moving { at: Pos2, side: usize, rgba: Vec<u8>, rgb: u32 },
    Done,
}

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
    /// The same picture shown magnified, its pixels crisp as on the canvas: when it is the canvas
    /// at its own size (not made smaller).
    canvas_crisp: Option<TextureHandle>,
    canvas_edits: Option<u64>,
    read_at: Option<Instant>,
    image: Option<TextureHandle>,
    canvas_pixels: Option<Pixels>,
    image_pixels: Option<Pixels>,
    /// How each is shown, zoomed and moved on its own (the core's view of it).
    canvas_view: ReferenceView,
    image_view: ReferenceView,
    /// Each picture a few pixels across, for the bars' glass.
    canvas_glass: Option<TextureHandle>,
    image_glass: Option<TextureHandle>,
    /// A press on the picture: when and where (points), and whether it became the eyedropper.
    press: Option<(Instant, Pos2)>,
    picking: bool,
    /// The bars are slid out of the way; a click that may yet be the first of two came then.
    bars_hidden: bool,
    click: Option<Instant>,
}

impl Default for Reference {
    fn default() -> Reference {
        Reference {
            on: false,
            mode: Mode::Canvas,
            at: None,
            size: START,
            canvas: None,
            canvas_crisp: None,
            canvas_edits: None,
            read_at: None,
            image: None,
            canvas_pixels: None,
            image_pixels: None,
            canvas_view: ReferenceView::default(),
            image_view: ReferenceView::default(),
            canvas_glass: None,
            image_glass: None,
            press: None,
            picking: false,
            bars_hidden: false,
            click: None,
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
            let crisp =
                egui::TextureOptions { magnification: egui::TextureFilter::Nearest, ..egui::TextureOptions::LINEAR };
            self.canvas_crisp = (t.width == session.info().width)
                .then(|| ctx.load_texture("reference canvas crisp", image.clone(), crisp));
            self.canvas = Some(ctx.load_texture("reference canvas", image, egui::TextureOptions::LINEAR));
            let glass = glass_of(t.width as usize, t.height as usize, &t.rgba);
            self.canvas_glass = Some(ctx.load_texture("reference canvas glass", glass, egui::TextureOptions::LINEAR));
            self.canvas_pixels = Some(Pixels { width: t.width as usize, height: t.height as usize, rgba: t.rgba });
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
                let flat: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                let glass = glass_of(size[0], size[1], &flat);
                self.image_glass =
                    Some(ctx.load_texture("reference picture glass", glass, egui::TextureOptions::LINEAR));
                self.image = Some(ctx.load_texture("reference picture", image, egui::TextureOptions::LINEAR));
                self.image_pixels = Some(Pixels { width: size[0], height: size[1], rgba: rgba.into_raw() });
                self.image_view = ReferenceView::default();
            }
            Err(_) => crate::files::tell("Can't import it", "This picture can't be opened."),
        }
    }

    /// Shows the window while Reference is on; the eyedropper held on its picture (`hold` held
    /// still opens it) says what it picks.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        hold: Duration,
    ) -> Option<Picked> {
        if !self.on {
            return None;
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
        let shown_bars = ctx.animate_bool_with_time(Id::unique("reference bars"), !self.bars_hidden, BARS_SLIDE);
        let mut close = false;
        let mut import = false;
        let mut picked = None;
        egui::Area::new(Id::unique("reference")).fixed_pos(reach.min).order(Order::Middle).show(ctx, |ui| {
            // allocated, so the canvas under it takes no press
            ui.allocate_rect(reach, Sense::hover());
            let painter = ui.painter();
            let shadow =
                egui::epaint::Shadow { offset: [0, 4], blur: 36, spread: 0, color: Color32::from_black_alpha(0x55) };
            painter.add(shadow.as_shape(window, CORNER));
            painter.rect_filled(window, CORNER, argb(PICTURE));
            // the picture: the whole window's, under the bars
            let area = window;
            let shown = match self.mode {
                Mode::Canvas => self.canvas.as_ref(),
                Mode::Image => self.image.as_ref(),
                Mode::Face => None,
            };
            // and how it sits there, for the bars' glass
            let mut placed = None;
            let touched = ui.interact(area, ui.scope_id().with("picture"), Sense::click_and_drag());
            if let Some(t) = shown {
                let [w, h] = t.size().map(|v| v.max(1) as f32);
                let sizes = ReferenceSizes {
                    window_width: area.width(),
                    window_height: area.height(),
                    picture_width: w,
                    picture_height: h,
                };
                let r = reference_picture_rect(self.view(), sizes);
                if let [x, y, w, h] = r[..] {
                    let picture = Rect::from_min_size(area.min + vec2(x, y), vec2(w, h));
                    placed = Some(picture);
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    // the canvas's pixels crisp from 100%, as on the canvas
                    // each canvas pixel a point or more
                    let crisp = self
                        .canvas_crisp
                        .as_ref()
                        .filter(|_| self.mode == Mode::Canvas && picture.width() >= t.size()[0] as f32 * 0.999);
                    let id = crisp.map_or(t.id(), TextureHandle::id);
                    ui.painter().with_clip_rect(area).image(id, picture, uv, Color32::WHITE);
                }
                picked = self.picture_touches(ui, &touched, area, sizes, hold);
            } else if self.mode == Mode::Image {
                let button = Rect::from_center_size(area.center(), vec2(180.0, 60.0));
                ui.painter().rect_filled(button, 30.0, argb(IMPORT));
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
            // (letting go of the eyedropper is no click)
            if picked.is_none() {
                self.clicks(ui, &touched);
            }
            let glass = if self.mode == Mode::Image { self.image_glass.clone() } else { self.canvas_glass.clone() };
            let glass = placed.zip(glass.filter(|_| self.mode != Mode::Face));
            // what a bar has behind it: the window, and the picture blurred
            let over_picture = |ui: &egui::Ui, bar: Rect, corners: egui::CornerRadius| {
                ui.painter().rect_filled(bar, corners, argb(PICTURE));
                if let Some((picture, t)) = &glass {
                    let [w, h] = t.size().map(|v| v as f32);
                    let (px, py) = (picture.width() / (w - 2.0), picture.height() / (h - 2.0));
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    let painter = ui.painter().with_clip_rect(bar.intersect(window));
                    painter.image(t.id(), picture.expand2(vec2(px, py)), uv, Color32::WHITE);
                }
                ui.painter().rect_filled(bar, corners, argb(BAR));
            };
            // the top bar, slid up out of the window as it goes
            let slide = 1.0 - shown_bars;
            let clipped = ui.painter().clip_rect().intersect(window);
            let bar = Rect::from_min_size(window.min - vec2(0.0, TOP_BAR * slide), vec2(window.width(), TOP_BAR));
            let bottom = Rect::from_min_size(
                pos2(window.left(), window.bottom() - BOTTOM_BAR + (BOTTOM_BAR + 1.0) * slide),
                vec2(window.width(), BOTTOM_BAR),
            );
            ui.scope(|ui| {
                ui.set_clip_rect(clipped);
                let corners = egui::CornerRadius { nw: CORNER as u8, ne: CORNER as u8, sw: 0, se: 0 };
                over_picture(ui, bar, corners);
                let painter = ui.painter();
                text(
                    painter,
                    pos2(bar.left() + 20.0, bar.top() + 24.0),
                    Align2::LEFT_CENTER,
                    "Reference",
                    22.0,
                    Weight::Bold,
                    argb(TITLE),
                );
                // what's on a bar takes its touches, and the rest of it holds them off the picture
                ui.interact(bar.intersect(window), ui.scope_id().with("top bar"), Sense::click_and_drag());
                let x_button =
                    Rect::from_center_size(pos2(bar.right() - 10.0 - 21.5, bar.center().y), vec2(43.0, 43.0));
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
                                self.image_pixels = None;
                                self.image_glass = None;
                                self.image_view = ReferenceView::default();
                            }
                        }
                        right = shown.left() - 12.0;
                    }
                }
                // the bottom bar: Canvas, Image and Face
                let corners = egui::CornerRadius { nw: 0, ne: 0, sw: CORNER as u8, se: CORNER as u8 };
                over_picture(ui, bottom, corners);
                ui.interact(bottom.intersect(window), ui.scope_id().with("bottom bar"), Sense::click_and_drag());
                let track = Rect::from_center_size(bottom.center(), vec2(270.0, 35.0));
                ui.painter().rect_filled(track, 17.5, argb(SEGMENT_TRACK));
                let item = (track.width() - 4.0 - 22.0) / 3.0;
                for (i, (mode, label)) in
                    [(Mode::Canvas, "Canvas"), (Mode::Image, "Image"), (Mode::Face, "Face")].into_iter().enumerate()
                {
                    let cell =
                        Rect::from_min_size(track.min + vec2(2.0 + (item + 11.0) * i as f32, 2.0), vec2(item, 31.0));
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
            });
            // the grab line stays when the bars go, and dragging by it moves the window
            ui.painter().rect_filled(
                Rect::from_center_size(pos2(window.center().x, window.top() + 6.0), vec2(40.0, 4.0)),
                2.0,
                argb(GRAB_LINE),
            );
            let strip = Rect::from_min_size(window.min, vec2(window.width(), GRAB));
            let dragged = ui.interact(strip, ui.scope_id().with("move"), Sense::drag());
            if dragged.dragged() {
                self.at = Some(at + dragged.drag_delta());
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
        picked
    }

    /// A click on the picture hides the bars or brings them back, once no second follows; two fit
    /// the picture, the bars out of the way.
    fn clicks(&mut self, ui: &egui::Ui, touched: &egui::Response) {
        if touched.double_clicked() {
            self.click = None;
            self.set_view(ReferenceView::default());
            self.bars_hidden = true;
        } else if touched.clicked() {
            self.click = Some(Instant::now());
        }
        if let Some(at) = self.click {
            if at.elapsed() >= SECOND_CLICK {
                self.click = None;
                self.bars_hidden = !self.bars_hidden;
            } else {
                ui.ctx().request_repaint_after(SECOND_CLICK - at.elapsed());
            }
        }
    }

    /// How the picture showing is zoomed and moved.
    fn view(&self) -> ReferenceView {
        if self.mode == Mode::Image { self.image_view } else { self.canvas_view }
    }

    fn set_view(&mut self, view: ReferenceView) {
        if self.mode == Mode::Image {
            self.image_view = view;
        } else {
            self.canvas_view = view;
        }
    }

    /// The picture's touches (`area` it fills, `sizes` its own and the area's): two fingers, a
    /// trackpad's pinch or the wheel zoom it about where they are, and a mouse or a finger drags
    /// it; a press held still for `hold` picks a colour from it until let go.
    fn picture_touches(
        &mut self,
        ui: &egui::Ui,
        touched: &egui::Response,
        area: Rect,
        sizes: ReferenceSizes,
        hold: Duration,
    ) -> Option<Picked> {
        let (spread, touch, pointer) = ui.input(|i| {
            let mut spread = 1.0f32;
            for e in &i.events {
                match e {
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => {
                        spread *= WHEEL_ZOOM.powf(delta.y);
                    }
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, .. } => {
                        spread *= (delta.y * SCROLL_ZOOM).exp();
                    }
                    egui::Event::Zoom(z) => spread *= z,
                    _ => {}
                }
            }
            (spread, i.multi_touch(), i.pointer.latest_pos())
        });
        let local = |p: Pos2| p - area.min;
        if let Some(t) = touch.filter(|_| touched.hovered() || touched.dragged()) {
            // two fingers or more: no eyedropper
            self.press = None;
            let to = local(t.center_pos);
            let from = to - t.translation_delta;
            self.set_view(reference_view_pinched(self.view(), sizes, from.x, from.y, to.x, to.y, t.zoom_delta));
            return None;
        }
        if touched.hovered()
            && spread != 1.0
            && let Some(p) = pointer
        {
            let p = local(p);
            self.set_view(reference_view_pinched(self.view(), sizes, p.x, p.y, p.x, p.y, spread));
        }
        if touched.is_pointer_button_down_on()
            && self.press.is_none()
            && let Some(p) = pointer
        {
            self.press = Some((Instant::now(), p));
        }
        let (since, start) = self.press?;
        let now = pointer.unwrap_or(start);
        if !touched.is_pointer_button_down_on() && !touched.dragged() {
            // let go
            self.press = None;
            return std::mem::take(&mut self.picking).then_some(Picked::Done);
        }
        if !self.picking && now.distance(start) <= STILL {
            if since.elapsed() >= hold {
                self.picking = true;
            } else {
                ui.ctx().request_repaint_after(hold - since.elapsed());
            }
        }
        if self.picking {
            return self.pick(now, local(now), sizes);
        }
        // a mouse or a finger drags the picture
        if touched.dragged() && now.distance(start) > STILL {
            let d = touched.drag_delta();
            let to = local(now);
            let from = to - d;
            self.set_view(reference_view_pinched(self.view(), sizes, from.x, from.y, to.x, to.y, 1.0));
        }
        None
    }

    /// The colour under window point `at` (`local` in the picture's area), and the pixels around
    /// it the loupe shows; none off the picture.
    fn pick(&self, at: Pos2, local: Vec2, sizes: ReferenceSizes) -> Option<Picked> {
        let pixels = if self.mode == Mode::Image { self.image_pixels.as_ref() } else { self.canvas_pixels.as_ref() }?;
        let point = reference_picture_point(self.view(), sizes, local.x, local.y)?;
        let (&px, &py, &per) = (point.first()?, point.get(1)?, point.get(2)?);
        let (w, h) = (pixels.width, pixels.height);
        let (x, y) = ((px as usize).min(w.saturating_sub(1)), (py as usize).min(h.saturating_sub(1)));
        let rgb_at = |x: usize, y: usize| pixels.rgba.get((y * w + x) * 4..(y * w + x) * 4 + 4);
        let [r, g, b, _] = <[u8; 4]>::try_from(rgb_at(x, y)?).ok()?;
        // as much of the picture as the loupe's window shows, around the point
        let side = ((crate::input::PICK_WINDOW * per).round() as usize).clamp(1, w.min(h).max(1));
        let (x0, y0) = (x.saturating_sub(side / 2).min(w - side), y.saturating_sub(side / 2).min(h - side));
        let mut rgba = Vec::with_capacity(side * side * 4);
        for row in y0..y0 + side {
            rgba.extend_from_slice(pixels.rgba.get((row * w + x0) * 4..(row * w + x0 + side) * 4)?);
        }
        let rgb = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
        Some(Picked::Moving { at, side, rgba, rgb })
    }
}

/// How much a wheel's line zooms the picture, and a trackpad's scrolled point (as the canvas).
const WHEEL_ZOOM: f32 = 1.15;
const SCROLL_ZOOM: f32 = 0.005;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bars_glass_is_the_picture_a_few_pixels_across_in_a_clear_rim() {
        // 480 x 240, its left half red and its right half blue
        let (w, h) = (480usize, 240usize);
        let rgba: Vec<u8> =
            (0..w * h).flat_map(|i| if i % w < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        let glass = glass_of(w, h, &rgba);
        assert_eq!(glass.size, [GLASS_SIDE + 2, GLASS_SIDE / 2 + 2]);
        let at = |x: usize, y: usize| glass.pixels[y * glass.size[0] + x];
        assert_eq!(at(0, 0), Color32::TRANSPARENT);
        assert_eq!(at(GLASS_SIDE + 1, 5), Color32::TRANSPARENT);
        assert_eq!(at(1, 1), Color32::from_rgba_premultiplied(255, 0, 0, 255));
        assert_eq!(at(GLASS_SIDE, 1), Color32::from_rgba_premultiplied(0, 0, 255, 255));
    }

    #[test]
    fn the_glass_of_a_picture_smaller_than_it_or_of_a_pixel_still_has_its_rim() {
        let glass = glass_of(1, 1, &[10, 20, 30, 255]);
        assert_eq!(glass.size[0], GLASS_SIDE + 2);
        assert_eq!(glass.pixels[glass.size[0] + 1], Color32::from_rgba_premultiplied(10, 20, 30, 255));
    }
}
