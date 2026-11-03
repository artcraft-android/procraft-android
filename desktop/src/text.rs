//! Text added and edited where it sits: a thin box over the canvas round the text, typed into from
//! the keyboard (the letters chosen ringed in blue, a handle at each end), Edit style in a bar
//! along the bottom, and the style sheet in its place: the fonts and styles (each name in its own
//! face), the design sliders and the attributes. As the Android app's (ui/TextUi.kt); the core
//! lays the text out and draws it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

use egui::{
    Align2, Color32, CornerRadius, Id, Key, KeyboardShortcut, Modifiers, Order, Pos2, Rect, Sense, Shape, Stroke,
    TextureHandle, TextureOptions, UiBuilder, Vec2, pos2, vec2,
};
use procraft_core::{CanvasSession, TextAlignment, TextDraw, TextDrawn, TextInfo};

use crate::alert::{Alert, Answer, Asking};
use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};

const SHEET_FILL: Themed = Themed(0xFF535353, 0xFFF3F6F7);
const SHEET_BAR: Themed = Themed(0xFF4F4F4F, 0xFFF3F6F7);
const SHEET_CAPSULE: Themed = Themed(0xFF3E3E3E, 0xFFFBFBFB);
const CARD_FILL: Themed = Themed(0xFF373737, 0xFFFAFAFA);
const CARD_CORNER: u8 = 22;
const BUTTON_FILL: Themed = Themed(0xFF474747, 0xFFE6E6E6);
const SEGMENT_TRACK: Themed = Themed(0xFF646464, 0xFFE3E3E3);
const CAPSULE_INK: Themed = Themed(0xFFC5C5C5, 0xFF565656);
const KEYBOARD_DISC: Themed = Themed(0xFF393939, 0xFFFBFBFB);
const KEYBOARD_BAR: Themed = Themed(0xFF2B2B2B, 0xFFE9ECED);
const DONE_BLUE: u32 = 0xFF0190FF;
const BOX_LINE: u32 = 0xFF0076FD;
/// A design row's track, and the middle tick of one that runs both ways.
const DESIGN_TRACK: Themed = Themed(0xFF5A5A5A, 0xFFE3E3E3);
const DESIGN_TICK: Themed = Themed(0xFF8A8A8A, 0xFFC8C8C8);
/// The sheet's words: white, black on the light interface.
const SHEET_INK: Themed = Themed(0xFFFFFFFF, 0xFF000000);

/// From this width the sheet shows its four cards side by side; narrower, in two pages.
const WIDE_SHEET: f32 = 900.0;
/// How tall the bar along the bottom is, and the sheet, wide and narrow (its cards wide end 33
/// above the bottom).
const TEXT_BAR: f32 = 44.0;
const SHEET_WIDE: f32 = 521.0;
const SHEET_NARROW: f32 = 561.0;
/// The names in the font and style lists (points).
const CELL_TEXT: f32 = 15.0;

/// A typeface on offer: the name shown, and the family text layers name.
#[derive(Clone, PartialEq)]
struct Family {
    name: String,
    family: String,
}

/// How long a frame spends drawing the style sheet's names (each its font's first use, ~20 ms).
const FACE_BUDGET: std::time::Duration = std::time::Duration::from_millis(12);

/// Where the fonts the user imports are kept.
fn fonts_dir() -> PathBuf {
    crate::files::app_dir().join("fonts")
}

/// Starts reading the fonts text is drawn in, off the interface's thread.
pub fn prepare_fonts() {
    std::thread::spawn(fonts_ready);
}

/// Waits until the fonts are read: Inter, the system's (the generic families pointed at them) and
/// those imported. Read once.
fn fonts_ready() {
    static READY: OnceLock<()> = OnceLock::new();
    READY.get_or_init(|| {
        for &(_, bytes) in &crate::look::INTER_FACES {
            procraft_core::add_text_font(bytes.to_vec());
        }
        // the generic families pointed at the system's own as they are read
        procraft_core::add_system_text_fonts();
        procraft_core::set_text_font_folder(fonts_dir().to_string_lossy().into_owned());
    });
}

/// Every typeface on offer: the core's, those imported among them.
fn families() -> Vec<Family> {
    procraft_core::text_typefaces().into_iter().map(|t| Family { name: t.name, family: t.family }).collect()
}

/// What the editor says to the app.
pub enum Said {
    Notice(&'static str, bool),
    Undo,
    Redo,
}

/// Draws the text into its layer's pixels on a thread of its own (only the part it was drawn in
/// and now reaches): only the newest text asked for is drawn, and the newest drawn is put in.
#[derive(Default)]
struct Drawer {
    shared: Arc<Drawing>,
    started: bool,
    asked: u64,
    landed: u64,
}

#[derive(Default)]
struct Drawing {
    asked: Mutex<Option<(u64, TextDraw)>>,
    wake: Condvar,
    drawn: Mutex<Option<(u64, TextDrawn)>>,
}

impl Drawer {
    fn ask(&mut self, ctx: &egui::Context, job: TextDraw) {
        if !self.started {
            self.started = true;
            let (shared, ctx) = (self.shared.clone(), ctx.clone());
            std::thread::spawn(move || {
                loop {
                    let mut asked = shared.asked.lock().unwrap_or_else(PoisonError::into_inner);
                    while asked.is_none() {
                        asked = shared.wake.wait(asked).unwrap_or_else(PoisonError::into_inner);
                    }
                    let Some((n, job)) = asked.take() else { continue };
                    drop(asked);
                    let drawn = job.draw();
                    *shared.drawn.lock().unwrap_or_else(PoisonError::into_inner) = Some((n, drawn));
                    ctx.request_repaint();
                }
            });
        }
        self.asked += 1;
        *self.shared.asked.lock().unwrap_or_else(PoisonError::into_inner) = Some((self.asked, job));
        self.shared.wake.notify_one();
    }

    /// The newest drawing, once, if it is newer than the last put in.
    fn take(&mut self) -> Option<TextDrawn> {
        let (n, drawn) = self.shared.drawn.lock().unwrap_or_else(PoisonError::into_inner).take()?;
        (n > self.landed).then(|| {
            self.landed = n;
            drawn
        })
    }

    /// Whether the text as last asked for has been put in.
    fn current(&self) -> bool {
        self.landed == self.asked
    }

    /// What was asked for and drawn so far goes unused.
    fn forget(&mut self) {
        self.shared.asked.lock().unwrap_or_else(PoisonError::into_inner).take();
        self.shared.drawn.lock().unwrap_or_else(PoisonError::into_inner).take();
        self.landed = self.asked;
    }
}

/// Text and the letters chosen in it (first to last).
type Chosen = (TextInfo, (usize, usize));

/// A picture shown over the canvas, and where (left, top, width and height, canvas pixels).
type Picture = (TextureHandle, [f32; 4]);

/// A name drawn in a face: the name, the family, the style and the size (pixels).
type FaceKey = (String, String, String, u32);

/// Where the text stands on the canvas: its box (left, top, width, height) and the caret before
/// the first and after the last letter chosen (x, top, bottom; none for vertical text).
struct Geometry {
    key: Chosen,
    bounds: [f32; 4],
    start: Vec<f32>,
    end: Vec<f32>,
}

#[derive(Default)]
pub struct TextEditor {
    /// The text being edited, as it stands; none when no text is.
    info: Option<TextInfo>,
    /// What the field holds, and the letters chosen in it (characters, first to last).
    typed: String,
    chosen: (usize, usize),
    /// Editing just began: the field takes the keys with all its text chosen.
    choose_all: bool,
    /// The style sheet is open in the bar's place; the text as it was when it opened (its
    /// Cancel); and which of its pages shows when they don't all fit.
    style_open: bool,
    before_style: Option<TextInfo>,
    page: usize,
    drawer: Drawer,
    geometry: Option<Geometry>,
    /// The chosen letters ringed, for which text and letters, and where on the canvas.
    ring: Option<(Chosen, Option<Picture>)>,
    /// A drag moving the text: where (points) it was last.
    dragging: Option<Pos2>,
    /// Names drawn in their faces (by name, family, style and pixel size): the picture and its
    /// size in points.
    faces: HashMap<FaceKey, Option<(TextureHandle, Vec2)>>,
    /// The frame names were last drawn in and when it began drawing them: a frame draws only
    /// [`FACE_BUDGET`] of them (all at once, the style sheet's first sight waited a third of a
    /// second), the rest in the frames after.
    face_budget: (u64, Option<std::time::Instant>),
    rasterize: Alert,
    /// The interface, for the drawing thread to ask for a frame once it has drawn.
    ctx: Option<egui::Context>,
}

/// The view as [`CanvasSession::view_matrix`] gives it.
fn matrix(session: &CanvasSession) -> [f32; 6] {
    let v = session.view_matrix();
    v.get(..6).and_then(|m| <[f32; 6]>::try_from(m).ok()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
}

/// Canvas pixel (x, y) in the window (points).
fn to_window([a, b, c, d, e, f]: [f32; 6], ppp: f32, x: f32, y: f32) -> Pos2 {
    pos2(a * x + c * y + e, b * x + d * y + f) / ppp
}

/// Window point `p` on the canvas (pixels).
fn to_canvas([a, b, c, d, e, f]: [f32; 6], ppp: f32, p: Pos2) -> Option<(f32, f32)> {
    let det = a * d - b * c;
    if det.abs() < 1e-9 {
        return None;
    }
    let (x, y) = (p.x * ppp - e, p.y * ppp - f);
    Some(((d * x - c * y) / det, (a * y - b * x) / det))
}

impl TextEditor {
    pub fn editing(&self) -> bool {
        self.info.is_some()
    }

    /// Another artwork is shown.
    pub fn forget(&mut self) {
        self.info = None;
        self.style_open = false;
        self.drawer.forget();
        self.geometry = None;
        self.ring = None;
        self.dragging = None;
    }

    /// New text in the middle of where the canvas shows, all of it chosen for typing over, in
    /// colour `rgb`; false if the canvas has no room for another layer.
    pub fn add(&mut self, ctx: &egui::Context, session: &CanvasSession, rgb: [f32; 3]) -> bool {
        fonts_ready();
        let [r, g, b] = rgb;
        let info = TextInfo {
            text: "Text".to_owned(),
            font: "sans-serif".to_owned(),
            style: "Medium".to_owned(),
            size: 0.0,
            kerning: 0.0,
            tracking: 0.0,
            leading: 0.0,
            baseline: 0.0,
            opacity: 1.0,
            align: TextAlignment::Center,
            underline: false,
            outline: false,
            vertical: false,
            all_caps: false,
            r,
            g,
            b,
            x: 0.0,
            y: 0.0,
            width: 0.0,
        };
        // a medium sans, sized and placed as new text looks on screen
        let (screen, ppp) = (ctx.content_rect(), ctx.pixels_per_point());
        let info = session.new_text_placed(info, screen.width() * ppp, screen.height() * ppp, ppp);
        if session.text_begin(None, info.clone()).is_none() {
            return false;
        }
        self.start(session, info);
        true
    }

    /// Edits text layer `id`'s text again (undo and redo of an edit too); false if it can't be.
    pub fn edit(&mut self, session: &CanvasSession, id: u64) -> bool {
        fonts_ready();
        let Some(info) = session.layer_text(id) else { return false };
        if session.text_begin(Some(id), info.clone()).is_none() {
            return false;
        }
        self.start(session, info);
        true
    }

    fn start(&mut self, session: &CanvasSession, info: TextInfo) {
        self.typed = info.text.clone();
        self.chosen = (0, self.typed.chars().count());
        self.choose_all = true;
        self.style_open = false;
        self.before_style = None;
        self.dragging = None;
        self.drawer.forget();
        self.info = Some(info);
        self.draw(session);
    }

    fn change(&mut self, session: &CanvasSession, f: impl FnOnce(&mut TextInfo)) {
        let Some(info) = self.info.as_mut() else { return };
        f(info);
        session.set_text(info.clone());
        self.draw(session);
    }

    fn draw(&mut self, session: &CanvasSession) {
        if self.info.is_none() {
            return;
        }
        match &self.ctx {
            Some(ctx) => {
                if let Some(job) = session.text_draw_start() {
                    self.drawer.ask(ctx, job);
                }
            }
            None => _ = session.draw_text_into_layer(),
        }
    }

    /// Ends editing text: `keep` makes it one undo step, otherwise it goes back.
    pub fn end(&mut self, session: &CanvasSession, keep: bool) {
        if self.info.take().is_none() {
            return;
        }
        // the last drawing lands first
        if let Some(drawn) = self.drawer.take() {
            session.text_draw_put(drawn);
        }
        if !self.drawer.current() {
            session.draw_text_into_layer();
        }
        self.drawer.forget();
        session.text_end(keep);
        self.style_open = false;
        self.geometry = None;
        self.ring = None;
        self.dragging = None;
    }

    /// A stroke landed on a text layer: asks to rasterize it.
    pub fn ask_rasterize(&mut self) {
        self.rasterize.ask(Asking {
            title: "Rasterize this text layer?".into(),
            message: "Its text can't be edited afterwards; you can paint on it.".into(),
            action: "Rasterize".into(),
            icon: None,
            destructive: false,
            extra: None,
            first: None,
            alone: false,
        });
    }

    /// The editor, while text is edited, and the question about rasterizing.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) -> Option<Said> {
        self.ctx = Some(ctx.clone());
        let mut said = None;
        if let Some(Answer::Action) = self.rasterize.show(ctx, look)
            && let Some(l) = session.layers().into_iter().find(|l| l.current)
            && session.rasterize_layer(l.id)
        {
            said = Some(Said::Notice("Text rasterized", true));
        }
        if self.info.is_none() {
            return said;
        }
        // undo and redo are the app's, not the field's: undoing undoes the whole edit (with Shift
        // first: the shortcut without it would take it too)
        let shortcut =
            |modifiers: Modifiers| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, Key::Z)));
        if shortcut(Modifiers::COMMAND | Modifiers::SHIFT) {
            return Some(Said::Redo);
        }
        if shortcut(Modifiers::COMMAND) {
            return Some(Said::Undo);
        }
        if let Some(drawn) = self.drawer.take() {
            session.text_draw_put(drawn);
        }
        if let Some(s) = self.canvas_part(ctx, session) {
            said = Some(s);
        }
        if self.info.is_none() {
            return said;
        }
        let screen = ctx.content_rect();
        // placed where it begins, so that it covers only itself
        let tall = match (self.style_open, screen.width() >= WIDE_SHEET) {
            (false, _) => TEXT_BAR,
            (true, true) => SHEET_WIDE,
            (true, false) => SHEET_NARROW,
        };
        let asked = egui::Area::new(Id::unique("text style"))
            .fixed_pos(pos2(screen.left(), screen.bottom() - tall))
            .order(Order::Foreground)
            .fade_in(false)
            .show(
                ctx,
                |ui| if self.style_open { self.style_sheet(ui, look, session, screen) } else { self.bar(ui, screen) },
            )
            .inner;
        said.or(asked)
    }

    /// Over the canvas: the box, the caret or the letters chosen, the field typed into, and the
    /// touches (a press outside the box ends the edit, keeping it; a drag inside moves the text).
    fn canvas_part(&mut self, ctx: &egui::Context, session: &CanvasSession) -> Option<Said> {
        let info = self.info.clone()?;
        let ppp = ctx.pixels_per_point();
        let m = matrix(session);
        let (start, end) = (self.chosen.0.min(self.chosen.1), self.chosen.0.max(self.chosen.1));
        let key = (info.clone(), (start, end));
        if self.geometry.as_ref().is_none_or(|g| g.key != key) {
            let b = procraft_core::text_box(info.clone());
            let bounds = [0, 1, 2, 3].map(|i| b.get(i).copied().unwrap_or(0.0));
            let start_at = procraft_core::text_caret(info.clone(), start as u32);
            let end_at =
                if end == start { start_at.clone() } else { procraft_core::text_caret(info.clone(), end as u32) };
            self.geometry = Some(Geometry { key: key.clone(), bounds, start: start_at, end: end_at });
        }
        let g = self.geometry.as_ref()?;
        let [l, t, w, h] = g.bounds;
        let corners = [(l, t), (l + w, t), (l + w, t + h), (l, t + h)].map(|(x, y)| to_window(m, ppp, x, y));
        let upright = Rect::from_points(&corners);
        let caret = |c: &[f32]| match c {
            [x, top, bottom] => Some((to_window(m, ppp, *x, *top), to_window(m, ppp, *x, *bottom))),
            _ => None,
        };
        let (from, to) = (caret(&g.start), caret(&g.end));
        if start != end && self.ring.as_ref().is_none_or(|(k, _)| *k != key) {
            let max = ctx.input(|i| i.max_texture_side) as u32;
            let picture = procraft_core::draw_chosen_text(info.clone(), start as u32, end as u32, BOX_LINE)
                .filter(|p| p.width <= max && p.height <= max)
                .map(|p| {
                    let image =
                        egui::ColorImage::from_rgba_premultiplied([p.width as usize, p.height as usize], &p.rgba);
                    let at = [p.x as f32, p.y as f32, p.width as f32, p.height as f32];
                    (ctx.load_texture("chosen letters", image, TextureOptions::LINEAR), at)
                });
            self.ring = Some((key, picture));
        }
        let screen = ctx.content_rect();
        let blue = argb(BOX_LINE);
        let mut ended = false;
        egui::Area::new(Id::unique("text editor")).fixed_pos(screen.min).order(Order::Middle).fade_in(false).show(
            ctx,
            |ui| {
                ui.allocate_rect(screen, Sense::click_and_drag());
                // a press where this is on top (not on the bar, the sheet or an alert)
                let (pressed, at) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.interact_pos()));
                if pressed
                    && let Some(p) = at
                    && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
                {
                    if upright.contains(p) {
                        self.dragging = Some(p);
                    } else {
                        ended = true;
                    }
                }
                let painter = ui.painter();
                painter.add(Shape::closed_line(corners.to_vec(), Stroke::new(1.0, blue)));
                for (a, b) in [(0, 3), (1, 2)] {
                    if let (Some(p), Some(q)) = (corners.get(a), corners.get(b)) {
                        painter.circle_filled(p.lerp(*q, 0.5), 3.0, blue);
                    }
                }
                // the caret, or the chosen letters ringed with a handle at each end
                if let (Some(from), Some(to)) = (from, to) {
                    if start == end {
                        painter.line_segment([from.0, from.1], Stroke::new(2.0, blue));
                    } else {
                        if let Some((_, Some((texture, [x, y, w, h])))) = &self.ring {
                            let mut mesh = egui::Mesh::with_texture(texture.id());
                            let quad = [(*x, *y), (x + w, *y), (x + w, y + h), (*x, y + h)];
                            let uv = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
                            for ((qx, qy), uv) in quad.into_iter().zip(uv) {
                                let pos = to_window(m, ppp, qx, qy);
                                mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
                            }
                            mesh.add_triangle(0, 1, 2);
                            mesh.add_triangle(0, 2, 3);
                            painter.add(Shape::mesh(mesh));
                        }
                        painter.line_segment([from.0, from.1], Stroke::new(2.0, blue));
                        painter.line_segment([to.0, to.1], Stroke::new(2.0, blue));
                        painter.circle_filled(from.0, 6.0, blue);
                        painter.circle_filled(to.1, 6.0, blue);
                    }
                }
                self.field(ui, upright.min);
            },
        );
        if ended {
            self.end(session, true);
            return None;
        }
        // dragged, the text follows
        let (down, at) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.interact_pos()));
        match (self.dragging, at) {
            (Some(last), Some(now)) if down => {
                if let (Some(a), Some(b)) = (to_canvas(m, ppp, last), to_canvas(m, ppp, now))
                    && now != last
                {
                    self.change(session, |i| {
                        i.x += b.0 - a.0;
                        i.y += b.1 - a.1;
                    });
                }
                self.dragging = Some(now);
            }
            _ => self.dragging = None,
        }
        // what was typed
        if self.info.as_ref().is_some_and(|i| i.text != self.typed) {
            let typed = self.typed.clone();
            self.change(session, |i| i.text = typed);
        }
        // Escape is Done
        if crate::ui::escape(ctx) {
            self.end(session, true);
        }
        None
    }

    /// The field the keys type into: unseen, at `at`. It has the keys unless the style sheet is
    /// open.
    fn field(&mut self, ui: &mut egui::Ui, at: Pos2) {
        let id = Id::unique("text field");
        let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(at, vec2(1.0, 1.0))));
        child.multiply_opacity(0.0);
        let mut typed = self.typed.clone();
        let mut output = egui::TextEdit::multiline(&mut typed)
            .id(id)
            .frame(egui::Frame::NONE)
            .desired_width(1.0)
            .desired_rows(1)
            .show(&mut child);
        if self.style_open {
            output.response.surrender_focus();
        } else if self.choose_all {
            self.choose_all = false;
            output.response.request_focus();
            self.choose(ui.ctx(), &mut output.state, 0, typed.chars().count());
        } else if !output.response.has_focus() {
            output.response.request_focus();
        }
        if let Some(range) = output.cursor_range {
            let (a, b) = (range.primary.index, range.secondary.index);
            self.chosen = (usize::from(a.min(b)), usize::from(a.max(b)));
        }
        self.typed = typed;
    }

    /// Chooses letters `from` to `to` in the field.
    fn choose(&mut self, ctx: &egui::Context, state: &mut egui::text_edit::TextEditState, from: usize, to: usize) {
        let range = egui::text::CCursorRange::two(egui::text::CCursor::new(from), egui::text::CCursor::new(to));
        state.cursor.set_char_range(Some(range));
        state.clone().store(ctx, Id::unique("text field"));
        self.chosen = (from, to);
    }

    /// Along the bottom: Edit style on the right.
    fn bar(&mut self, ui: &mut egui::Ui, screen: Rect) -> Option<Said> {
        let bar = Rect::from_min_max(pos2(screen.left(), screen.bottom() - TEXT_BAR), screen.max);
        ui.allocate_rect(bar, Sense::click());
        ui.painter().rect_filled(bar, 0, argb(KEYBOARD_BAR));
        let words = text(
            ui.painter(),
            pos2(bar.right() - 24.0, bar.center().y),
            Align2::RIGHT_CENTER,
            "Edit style",
            15.0,
            Weight::Regular,
            argb(SHEET_INK),
        );
        if ui.interact(words.expand(8.0), ui.scope_id().with("edit style"), Sense::click()).clicked() {
            self.before_style = self.info.clone();
            self.style_open = true;
        }
        None
    }

    /// The style sheet along the bottom: a bar (the keyboard back, cut, copy and paste, import a
    /// font, Cancel, Done); then the font, style, design and attributes cards, side by side when
    /// wide, else Typeface or Design, switched at the bottom.
    fn style_sheet(
        &mut self,
        ui: &mut egui::Ui,
        look: &mut Look,
        session: &CanvasSession,
        screen: Rect,
    ) -> Option<Said> {
        let wide = screen.width() >= WIDE_SHEET;
        let sheet = Rect::from_min_max(
            pos2(screen.left(), screen.bottom() - if wide { SHEET_WIDE } else { SHEET_NARROW }),
            screen.max,
        );
        // a press on the sheet between its controls stays in it
        ui.allocate_rect(sheet, Sense::click());
        let ctx = ui.ctx().clone();
        let painter = ui.painter().clone();
        painter.rect_filled(sheet, 0, argb(SHEET_FILL));
        let bar = Rect::from_min_size(sheet.min, vec2(sheet.width(), 62.0));
        painter.rect_filled(bar, 0, argb(SHEET_BAR));
        let cy = bar.center().y;
        let ink = argb(CAPSULE_INK);
        let tapped = |ui: &egui::Ui, r: Rect, name: &str| {
            ui.interact(r, ui.scope_id().with(("sheet", name)), Sense::click()).clicked()
        };
        // the keyboard back
        let keyboard = Rect::from_center_size(pos2(bar.left() + 8.0 + 22.0, cy), vec2(44.0, 44.0));
        painter.circle_filled(keyboard.center(), 22.0, argb(KEYBOARD_DISC));
        look.icon(ui, &icons::KEYBOARD, keyboard.center(), 40.0, ink);
        if tapped(ui, keyboard, "keyboard") {
            self.style_open = false;
        }
        // cut, copy and paste
        let capsule = Rect::from_min_size(pos2(keyboard.right() + 13.0, cy - 20.0), vec2(41.0 * 3.0 + 14.0, 40.0));
        painter.rect_filled(capsule, 12, argb(SHEET_CAPSULE));
        let edits: [(&Icon, &str); 3] =
            [(&icons::TEXT_CUT, "cut"), (&icons::TEXT_COPY, "copy"), (&icons::TEXT_PASTE, "paste")];
        for (k, (icon, name)) in edits.into_iter().enumerate() {
            let cell = Rect::from_min_size(pos2(capsule.left() + 48.0 * k as f32, capsule.top()), vec2(41.0, 40.0));
            look.icon(ui, icon, cell.center(), 40.0, ink);
            if tapped(ui, cell, name) {
                match name {
                    "paste" => self.paste(&ctx, session),
                    _ => self.copy(&ctx, session, name == "cut"),
                }
            }
        }
        // Done, then Import and Cancel in a capsule left of it
        let done = Rect::from_center_size(pos2(bar.right() - 8.0 - 22.0, cy), vec2(44.0, 45.0));
        painter.rect_filled(done, 22, argb(DONE_BLUE));
        look.icon(ui, &icons::DONE_TICK, done.center(), 40.0, Color32::WHITE);
        let behind = painter.add(Shape::Noop);
        let cancel = text(
            &painter,
            pos2(done.left() - 13.0 - 14.0, cy),
            Align2::RIGHT_CENTER,
            "Cancel",
            15.0,
            Weight::Regular,
            ink,
        );
        let cancel = Rect::from_min_max(pos2(cancel.left() - 14.0, cy - 21.5), pos2(cancel.right() + 14.0, cy + 21.5));
        let import =
            text(&painter, pos2(cancel.left() - 14.0, cy), Align2::RIGHT_CENTER, "Import", 15.0, Weight::Regular, ink);
        let import = Rect::from_min_max(pos2(import.left() - 14.0, cy - 21.5), pos2(import.right() + 14.0, cy + 21.5));
        painter.set(behind, Shape::rect_filled(import.union(cancel), 21.5, argb(SHEET_CAPSULE)));
        let mut said = None;
        if tapped(ui, import, "import") {
            said = self.import_font(session);
        }
        if tapped(ui, cancel, "cancel") {
            if let (Some(before), Some(now)) = (self.before_style.clone(), self.info.as_ref()) {
                let typed = now.text.clone();
                self.change(session, |i| *i = TextInfo { text: typed, ..before });
            }
            self.style_open = false;
        }
        if tapped(ui, done, "done") {
            self.end(session, true);
            return said;
        }
        let Some(info) = self.info.clone() else { return said };
        // the cards: wide, the four side by side; narrow, Typeface or Design
        let cards = Rect::from_min_max(
            pos2(sheet.left() + 10.0, bar.bottom()),
            pos2(sheet.right() - 10.0, sheet.bottom() - if wide { 33.0 } else { 81.0 }),
        );
        let lay = |parts: &[(f32, f32)]| {
            let gaps: f32 = parts.iter().map(|(_, gap)| gap).sum();
            let weights: f32 = parts.iter().map(|(w, _)| w).sum();
            let mut x = cards.left();
            parts
                .iter()
                .map(|(w, gap)| {
                    let width = (cards.width() - gaps) * w / weights;
                    let r = Rect::from_min_max(pos2(x, cards.top()), pos2(x + width, cards.bottom()));
                    x += width + gap;
                    r
                })
                .collect::<Vec<Rect>>()
        };
        let font_corners = CornerRadius { nw: CARD_CORNER, sw: CARD_CORNER, ne: 0, se: 0 };
        let style_corners = CornerRadius { nw: 0, sw: 0, ne: CARD_CORNER, se: CARD_CORNER };
        if wide {
            if let [font, style, design, attributes] =
                lay(&[(263.5, 2.0), (184.5, 5.0), (328.0, 5.0), (325.0, 0.0)])[..]
            {
                self.font_card(ui, session, &info, font, font_corners);
                self.style_card(ui, session, &info, style, style_corners);
                self.design_card(ui, session, &info, design);
                self.attributes_card(ui, look, session, &info, attributes, true);
            }
        } else if self.page == 0 {
            if let [font, style] = lay(&[(424.5, 2.0), (297.5, 0.0)])[..] {
                self.font_card(ui, session, &info, font, font_corners);
                self.style_card(ui, session, &info, style, style_corners);
            }
        } else if let [design, attributes] = lay(&[(584.0, 5.0), (135.0, 0.0)])[..] {
            self.design_card(ui, session, &info, design);
            self.attributes_card(ui, look, session, &info, attributes, false);
        }
        // Typeface | Design, when the cards don't all fit
        if !wide {
            let track = Rect::from_min_size(pos2(sheet.center().x - 164.0, cards.bottom() + 11.0), vec2(328.0, 34.0));
            painter.rect_filled(track, 17, argb(SEGMENT_TRACK));
            for (i, label) in ["Typeface", "Design"].into_iter().enumerate() {
                let r = Rect::from_min_size(
                    pos2(track.left() + 2.0 + 168.0 * i as f32, track.top() + 2.0),
                    vec2(156.0, 30.0),
                );
                let on = i == self.page;
                if on {
                    painter.rect_filled(r, 15, argb(pc::ACCENT));
                }
                let colour = if on { Color32::WHITE } else { ink };
                text(&painter, r.center(), Align2::CENTER_CENTER, label, 13.0, Weight::Regular, colour);
                if tapped(ui, r, label) {
                    self.page = i;
                }
            }
        }
        said
    }

    /// A card: its title, and below it where its contents go.
    fn card(ui: &egui::Ui, rect: Rect, title: &str, corners: CornerRadius) -> Rect {
        ui.painter().rect_filled(rect, corners, argb(CARD_FILL));
        text(
            ui.painter(),
            rect.min + vec2(20.0, 16.5),
            Align2::LEFT_TOP,
            title,
            17.0,
            Weight::SemiBold,
            argb(SHEET_INK),
        );
        Rect::from_min_max(pos2(rect.left(), rect.top() + 16.5 + 20.0 + 9.0), rect.max)
    }

    fn font_card(
        &mut self,
        ui: &mut egui::Ui,
        session: &CanvasSession,
        info: &TextInfo,
        rect: Rect,
        corners: CornerRadius,
    ) {
        let inside = Self::card(ui, rect, "Font", corners);
        let rows: Vec<(String, String, String, bool)> = families()
            .into_iter()
            .map(|f| {
                let on = f.family == info.font;
                (f.name, f.family, "Regular".to_owned(), on)
            })
            .collect();
        if let Some(i) = self.cells(ui, inside, "fonts", &rows) {
            let family = rows.get(i).map(|r| r.1.clone()).unwrap_or_default();
            self.change(session, |t| t.font = family);
        }
    }

    fn style_card(
        &mut self,
        ui: &mut egui::Ui,
        session: &CanvasSession,
        info: &TextInfo,
        rect: Rect,
        corners: CornerRadius,
    ) {
        let inside = Self::card(ui, rect, "Style", corners);
        let rows: Vec<(String, String, String, bool)> = procraft_core::text_styles()
            .into_iter()
            .map(|s| (s.clone(), info.font.clone(), s.clone(), s == info.style))
            .collect();
        if let Some(i) = self.cells(ui, inside, "styles", &rows) {
            let style = rows.get(i).map(|r| r.2.clone()).unwrap_or_default();
            self.change(session, |t| t.style = style);
        }
    }

    /// A scrolling list of names (each drawn in its family and style), the chosen one a blue
    /// cell; which was tapped.
    fn cells(
        &mut self,
        ui: &mut egui::Ui,
        inside: Rect,
        id: &str,
        rows: &[(String, String, String, bool)],
    ) -> Option<usize> {
        let list =
            Rect::from_min_max(pos2(inside.left() + 10.0, inside.top()), pos2(inside.right() - 17.0, inside.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        let mut tapped = None;
        egui::ScrollArea::vertical()
            .id_salt(("text cells", id))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.add_space(6.0);
                for (i, (name, family, style, on)) in rows.iter().enumerate() {
                    let (cell, response) = ui.allocate_exact_size(vec2(list.width(), 33.0), Sense::click());
                    if *on {
                        ui.painter().rect_filled(cell, 0, argb(pc::ACCENT));
                    }
                    let colour = if *on { Color32::WHITE } else { argb(SHEET_INK) };
                    if let Some((texture, size)) = self.face(ui.ctx(), name, family, style, CELL_TEXT, 0.0) {
                        let at = Rect::from_min_size(pos2(cell.left() + 12.0, cell.center().y - size.y / 2.0), size);
                        ui.painter().with_clip_rect(cell.intersect(ui.clip_rect())).image(
                            texture,
                            at,
                            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                            colour,
                        );
                    }
                    if response.clicked() {
                        tapped = Some(i);
                    }
                    ui.add_space(9.0);
                }
            });
        tapped
    }

    /// `name` drawn in `family`'s `style`, `size` points, its letters spaced `tracking` (em):
    /// white, to be tinted; and its size in points.
    fn face(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        family: &str,
        style: &str,
        size: f32,
        tracking: f32,
    ) -> Option<(egui::TextureId, Vec2)> {
        let ppp = ctx.pixels_per_point();
        let px = (size * ppp).round().max(1.0) as u32;
        let key = (name.to_owned(), family.to_owned(), style.to_owned(), px);
        if !self.faces.contains_key(&key) {
            let pass = ctx.cumulative_pass_nr();
            if self.face_budget.0 != pass {
                self.face_budget = (pass, Some(std::time::Instant::now()));
            }
            if self.face_budget.1.is_some_and(|began| began.elapsed() > FACE_BUDGET) {
                ctx.request_repaint();
                return None;
            }
        }
        let made = self.faces.entry(key).or_insert_with(|| {
            let info = TextInfo {
                text: name.to_owned(),
                font: family.to_owned(),
                style: style.to_owned(),
                size: px as f32,
                kerning: 0.0,
                tracking,
                leading: 0.0,
                baseline: 0.0,
                opacity: 1.0,
                align: TextAlignment::Left,
                underline: false,
                outline: false,
                vertical: false,
                all_caps: false,
                r: 1.0,
                g: 1.0,
                b: 1.0,
                x: 0.0,
                y: 0.0,
                width: 0.0,
            };
            let b = procraft_core::text_box(info.clone());
            let (w, h) = (
                b.get(2).copied().unwrap_or(1.0).ceil().clamp(1.0, 4096.0),
                b.get(3).copied().unwrap_or(1.0).ceil().clamp(1.0, 1024.0),
            );
            let rgba = procraft_core::draw_text(info, w as u32, h as u32);
            let image = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], &rgba);
            Some((ctx.load_texture(format!("face {name}"), image, TextureOptions::LINEAR), vec2(w, h) / ppp))
        });
        made.as_ref().map(|(t, s)| (t.id(), *s))
    }

    fn design_card(&mut self, ui: &mut egui::Ui, session: &CanvasSession, info: &TextInfo, rect: Rect) {
        let inside = Self::card(ui, rect, "Design", CornerRadius::same(CARD_CORNER));
        let dpi = session.info().dpi.max(1.0);
        let pt = |px: f32| px * 72.0 / dpi;
        let rows: [(&str, f32, String, bool); 6] = [
            ("Size", ((info.size - 1.0) / 1999.0).clamp(0.0, 1.0).sqrt(), format!("{:.1}pt", pt(info.size)), false),
            ("Kerning", info.kerning / 2.0 + 0.5, format!("{:.1}%", info.kerning * 100.0), true),
            ("Tracking", info.tracking / 2.0 + 0.5, format!("{:.1}%", info.tracking * 100.0), true),
            ("Leading", info.leading / 2.0 + 0.5, format!("{}pt", pt(info.leading * info.size).round()), true),
            ("Baseline", info.baseline / 2.0 + 0.5, format!("{}pt", pt(info.baseline * info.size).round()), true),
            ("Opacity", info.opacity, format!("{:.1}%", info.opacity * 100.0), false),
        ];
        let (x0, x1) = (inside.left() + 4.0 + 16.0, inside.right() - 4.0 - 16.0);
        let ink = argb(SHEET_INK);
        for (k, (label, value, shown, from_middle)) in rows.into_iter().enumerate() {
            let top = inside.top() + 58.5 * k as f32 + 1.5;
            let cy = top + 12.0;
            let painter = ui.painter();
            text(painter, pos2(x0, cy), Align2::LEFT_CENTER, label, 15.0, Weight::Regular, ink);
            let behind = painter.add(Shape::Noop);
            let words = text(painter, pos2(x1 - 10.0, cy), Align2::RIGHT_CENTER, &shown, 13.0, Weight::Regular, ink);
            let pill = Rect::from_min_max(pos2(words.left() - 10.0, cy - 12.0), pos2(x1, cy + 12.0));
            painter.set(behind, Shape::rect_filled(pill, 12.0, argb(BUTTON_FILL)));
            // the bar: filled from the left, or a tick in its middle
            let slider = Rect::from_min_max(pos2(x0, top + 24.0), pos2(x1, top + 44.0));
            let y = slider.center().y;
            let v = value.clamp(0.0, 1.0);
            let track = Rect::from_min_max(pos2(x0, y - 1.0), pos2(x1, y + 1.0));
            painter.rect_filled(track, 1.0, argb(DESIGN_TRACK));
            if from_middle {
                let x = slider.center().x;
                painter.line_segment([pos2(x, y - 5.0), pos2(x, y + 5.0)], Stroke::new(1.0, argb(DESIGN_TICK)));
            } else {
                painter.rect_filled(
                    Rect::from_min_max(track.min, pos2(x0 + track.width() * v, track.bottom())),
                    1.0,
                    argb(pc::ACCENT),
                );
            }
            let x = (x0 + track.width() * v).clamp(x0 + 8.0, x1 - 8.0);
            painter.rect_filled(Rect::from_center_size(pos2(x, y), vec2(16.0, 10.0)), 5.0, Color32::WHITE);
            let response = ui.interact(slider, ui.scope_id().with(("design", label)), Sense::click_and_drag());
            if response.is_pointer_button_down_on()
                && let Some(p) = response.interact_pointer_pos()
            {
                let v = ((p.x - x0) / track.width()).clamp(0.0, 1.0);
                self.change(session, |t| match label {
                    "Size" => t.size = 1.0 + 1999.0 * v * v,
                    "Kerning" => t.kerning = v * 2.0 - 1.0,
                    "Tracking" => t.tracking = v * 2.0 - 1.0,
                    "Leading" => t.leading = v * 2.0 - 1.0,
                    "Baseline" => t.baseline = v * 2.0 - 1.0,
                    _ => t.opacity = v,
                });
            }
        }
    }

    /// Alignment, underline, outline, vertical and all caps: wide, the four alignments in a row
    /// and the other three in a row under them; narrow, the alignments two by two and the others
    /// one a row.
    #[allow(clippy::too_many_arguments)]
    fn attributes_card(
        &mut self,
        ui: &mut egui::Ui,
        look: &mut Look,
        session: &CanvasSession,
        info: &TextInfo,
        rect: Rect,
        wide: bool,
    ) {
        let inside = Self::card(ui, rect, "Attributes", CornerRadius::same(CARD_CORNER));
        let pad = if wide { 10.0 } else { 6.5 };
        let (x0, x1) = (inside.left() + pad, inside.right() - pad);
        let mut y = inside.top() + if wide { 6.0 } else { 7.0 };
        let gap = if wide { 20.0 } else { 4.0 };
        let mut toggled: Option<&str> = None;
        let mut aligned = None;
        let aligns = [
            (TextAlignment::Left, &icons::ALIGN_LEFT),
            (TextAlignment::Center, &icons::ALIGN_CENTER),
            (TextAlignment::Right, &icons::ALIGN_RIGHT),
            (TextAlignment::Justify, &icons::ALIGN_JUSTIFY),
        ];
        // the buttons 36 tall, but the alignments 35 when two by two
        let per_row = if wide { 4 } else { 2 };
        let tall = if wide { 36.0 } else { 35.0 };
        for row in aligns.chunks(per_row) {
            let w = (x1 - x0 - 4.0 * (per_row - 1) as f32) / per_row as f32;
            for (k, (a, icon)) in row.iter().enumerate() {
                let r = Rect::from_min_size(pos2(x0 + (w + 4.0) * k as f32, y), vec2(w, tall));
                if attribute(ui, look, r, icon, 32.0, info.align == *a, &format!("{a:?}")) {
                    aligned = Some(*a);
                }
            }
            y += tall + gap;
        }
        if !wide {
            y += 13.0 + gap;
        }
        let looks: [(&str, &Icon, f32, bool, f32); 3] = [
            ("underline", &icons::TEXT_UNDERLINE, 38.0, info.underline, 94.0),
            ("outline", &icons::TEXT_OUTLINE, 38.0, info.outline, 94.0),
            ("vertical", &icons::TEXT_VERTICAL, 41.0, info.vertical, 111.0),
        ];
        if wide {
            let mut x = x0;
            let share = (x1 - x0 - 8.0) / 299.0;
            for (name, icon, size, on, weight) in looks {
                let r = Rect::from_min_size(pos2(x, y), vec2(weight * share, 36.0));
                if attribute(ui, look, r, icon, size, on, name) {
                    toggled = Some(name);
                }
                x += weight * share + 4.0;
            }
            y += 36.0 + gap + 4.0;
        } else {
            for (name, icon, size, on, _) in looks {
                let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + 36.0));
                if attribute(ui, look, r, icon, size, on, name) {
                    toggled = Some(name);
                }
                y += 36.0 + gap;
            }
            y += 16.0 + gap;
        }
        // all caps: two serif capitals all but touching, then its switch
        let row = Rect::from_min_max(pos2(x0, y), pos2(x1, y + 31.0));
        let ink = argb(SHEET_INK);
        if let Some((texture, size)) = self.face(ui.ctx(), "TT", "serif", "Regular", 26.5, -0.5 / 26.5) {
            let at = Rect::from_min_size(pos2(row.left() + 10.0, row.center().y - size.y / 2.0), size);
            ui.painter().image(texture, at, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), ink);
        }
        let switch_x = if wide { row.right() - 63.0 } else { row.left() + 10.0 + 50.5 };
        crate::transform::switch(ui, pos2(switch_x, row.center().y - 14.0), info.all_caps, "all caps");
        if ui.interact(row, ui.scope_id().with("all caps"), Sense::click()).clicked() {
            toggled = Some("caps");
        }
        if let Some(a) = aligned {
            self.change(session, |t| t.align = a);
        }
        if let Some(name) = toggled {
            self.change(session, |t| match name {
                "underline" => t.underline = !t.underline,
                "outline" => t.outline = !t.outline,
                "vertical" => t.vertical = !t.vertical,
                _ => t.all_caps = !t.all_caps,
            });
        }
    }

    /// Copies the letters chosen (all of them if none are), and with `cut` takes them out.
    fn copy(&mut self, ctx: &egui::Context, session: &CanvasSession, cut: bool) {
        let count = self.typed.chars().count();
        let (from, to) = if self.chosen.0 == self.chosen.1 { (0, count) } else { self.chosen };
        let part: String = self.typed.chars().skip(from).take(to - from).collect();
        if part.is_empty() {
            return;
        }
        ctx.copy_text(part);
        if cut {
            let kept: String = self.typed.chars().take(from).chain(self.typed.chars().skip(to)).collect();
            self.set_typed(ctx, session, kept, from);
        }
    }

    /// Puts the text copied in place of the letters chosen.
    fn paste(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some(add) = arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok() else { return };
        let (from, to) = self.chosen;
        let typed: String =
            self.typed.chars().take(from).chain(add.chars()).chain(self.typed.chars().skip(to)).collect();
        self.set_typed(ctx, session, typed, from + add.chars().count());
    }

    /// The field holds `typed`, its caret at `caret`.
    fn set_typed(&mut self, ctx: &egui::Context, session: &CanvasSession, typed: String, caret: usize) {
        self.typed = typed.clone();
        let mut state = egui::text_edit::TextEditState::load(ctx, Id::unique("text field")).unwrap_or_default();
        self.choose(ctx, &mut state, caret, caret);
        self.change(session, |t| t.text = typed);
    }

    /// Copies a font file the user picks into the app and offers it; the text takes it.
    fn import_font(&mut self, session: &CanvasSession) -> Option<Said> {
        let from =
            rfd::FileDialog::new().set_title("Import a font").add_filter("Fonts", &["ttf", "otf"]).pick_file()?;
        let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = std::fs::read(&from).ok()?;
        fonts_ready();
        // kept in the fonts folder by the core (a file that isn't a font is dropped)
        let typeface = procraft_core::import_text_font(bytes, name)?;
        self.change(session, |t| t.font = typeface.family);
        Some(Said::Notice("Font imported", false))
    }
}

/// An attribute's button at `r`: its icon (`size` across), blue when on; whether it was tapped.
fn attribute(ui: &egui::Ui, look: &mut Look, r: Rect, icon: &Icon, size: f32, on: bool, name: &str) -> bool {
    ui.painter().rect_filled(r, 8, if on { argb(pc::ACCENT) } else { argb(BUTTON_FILL) });
    look.icon(ui, icon, r.center(), size, if on { Color32::WHITE } else { argb(SHEET_INK) });
    ui.interact(r, ui.scope_id().with(("attribute", name)), Sense::click()).clicked()
}
