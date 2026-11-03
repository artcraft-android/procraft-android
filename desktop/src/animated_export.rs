//! Sharing the layers as an animation, as the Android app's (ui/AnimatedExportUi.kt): the frames
//! playing, the settings (the size, and per format dithering and a transparent background),
//! Cancel, and the round Done that saves the file. The file is made as the settings change, to
//! show its size; it and the preview's frames are made off the window's thread. On a model, its
//! turntable in place of the layers: its size, rotate or swing, its duration, zoom and ease, and
//! the environment behind it; its file made once Done is pressed. While Done waits for the file,
//! the export card shows how far it has come.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Order, Rect, Sense, Stroke, TextureHandle, pos2, vec2};
use procraft_core::{CanvasSession, Turntable};

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, pc, text};

const SCREEN_FILL: Themed = Themed(0xFF1F1F1F, 0xFFF2F2F2);
const TITLE_INK: Themed = Themed(0xFFAAAAAA, 0xFF3D3D3D);
const ROW_INK: Themed = Themed(0xFFC1C1C1, 0xFF595959);
const SIZE_INK: Themed = Themed(0xFF8E8E8E, 0xFF3F3F3F);
const PANEL_FILL: Themed = Themed(0xFF262626, 0xFFEBEBEB);
const PANEL_EDGE: Themed = Themed(0xFF363636, 0xFFDADADA);
const SEGMENT_TRACK: Themed = Themed(0xFF3C3C3C, 0xFFD5D5D5);
const CAPSULE_FILL: Themed = Themed(0xFF1A1A1A, 0xFFFBFBFB);
const CAPSULE_EDGE: Themed = Themed(0xFF373737, 0xFFE0E0E0);
const CHOICE_FILL: Themed = Themed(0xFF2E2E2E, 0xFFE3E3E3);
const SLIDER_TRACK: Themed = Themed(0xFF3E3E3E, 0xFFDDDDDD);
/// A model's turntable sizes (video's 16:9 across, or a square) and its frames a second.
const TURNTABLE_SIZES: [(u32, u32, &str); 4] =
    [(1280, 720, "720p"), (1920, 1080, "1080p"), (3840, 2160, "4K"), (1080, 1080, "Square")];
const TURNTABLE_FPS: f32 = 30.0;
const CANCEL_INK: u32 = 0xFFA5A5A5;
const DONE_BLUE: u32 = 0xFF0190FF;
const PANEL_WIDTH: f32 = 400.0;
/// Web Ready's longest side, a video's at most, and the preview's frames a second.
const WEB_READY_SIDE: u32 = 480;
const MAX_VIDEO_SIDE: u32 = 4096;
const PREVIEW_FPS: u64 = 15;
/// The frame rate when Animation Assist is off.
const ANIMATION_FPS: u32 = 15;

/// The animated formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animated {
    Gif,
    Png,
    Mp4,
    Hevc,
}

impl Animated {
    pub fn title(self) -> &'static str {
        match self {
            Animated::Gif => "Animated GIF",
            Animated::Png => "Animated PNG",
            Animated::Mp4 => "Animated MP4",
            Animated::Hevc => "Animated HEVC",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Animated::Gif => "gif",
            Animated::Png => "png",
            Animated::Mp4 | Animated::Hevc => "mp4",
        }
    }
}

/// The settings a file is made with: Web Ready, dithering, a transparent background; on a model,
/// how its turntable goes and its size.
type Settings = (bool, bool, bool, Option<(Turntable, usize)>);
/// A file being made: its settings, and where it went and how big it is, once made.
type Making = (Settings, Receiver<Option<(PathBuf, u64)>>);
/// What the preview's frames are read for: a transparent background or not, what side, and on a
/// model how its turntable goes.
type Framing = (bool, u32, Option<Turntable>);
/// The preview's frames being read, and the frames, once read.
type Reading = (Framing, Receiver<Vec<procraft_core::ThumbnailImage>>);

pub struct AnimatedExport {
    format: Animated,
    web_ready: bool,
    dither: bool,
    transparent: bool,
    /// The file made, the settings it was made with and its size; the one being made.
    made: Option<(Settings, PathBuf, u64)>,
    making: Option<Making>,
    /// The settings a file couldn't be made with (not tried again).
    failed: Option<Settings>,
    /// The preview's frames, what they were read for (transparent, side), and the read running.
    frames: Vec<TextureHandle>,
    frames_for: Option<Framing>,
    reading: Option<Reading>,
    shown: usize,
    stepped: Instant,
    /// Done tapped: the file is saved once it is made, the export card up meanwhile.
    saving: bool,
    card: crate::export_card::ExportCard,
    exporting: Option<crate::export_card::Exporting>,
    /// How far a video being made has come: its frames written, 0..1 (as `f32` bits).
    video_done: Arc<AtomicU32>,
    /// Upright, the settings panel is open.
    settings_open: bool,
    /// On a model: its turntable and size (which of [`TURNTABLE_SIZES`]), in place of the
    /// layers; a slider held.
    turntable: Option<(Turntable, usize)>,
    held: Option<(Id, f32)>,
}

/// What the screen asked.
pub enum Ended {
    /// Cancelled, or the file was saved.
    Closed,
    /// The file couldn't be made.
    Failed,
}

impl AnimatedExport {
    pub fn new(format: Animated) -> AnimatedExport {
        AnimatedExport {
            format,
            web_ready: false,
            dither: true,
            transparent: false,
            made: None,
            making: None,
            failed: None,
            frames: Vec::new(),
            frames_for: None,
            reading: None,
            shown: 0,
            stepped: Instant::now(),
            saving: false,
            card: crate::export_card::ExportCard::default(),
            exporting: None,
            video_done: Arc::new(AtomicU32::new(0)),
            settings_open: false,
            turntable: None,
            held: None,
        }
    }

    /// A model's turntable as `format`: once round in two seconds, half way out, eased, the
    /// environment behind it, 720p.
    pub fn model(format: Animated) -> AnimatedExport {
        let turntable = Turntable { swing: false, seconds: 2.0, zoom: 0.5, ease: 1.0, environment: true };
        AnimatedExport { turntable: Some((turntable, 0)), ..AnimatedExport::new(format) }
    }

    fn can_transparent(&self) -> bool {
        self.turntable.is_none() && matches!(self.format, Animated::Gif | Animated::Png)
    }

    fn settings(&self) -> Settings {
        (self.web_ready, self.dither, self.transparent, self.turntable)
    }

    /// The file made for the settings now, made again (off the window's thread) when they
    /// changed; one at a time, as they write the same file.
    fn make_file(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>) {
        if let Some((settings, rx)) = &self.making {
            match rx.try_recv() {
                Ok(Some((path, bytes))) => self.made = Some((*settings, path, bytes)),
                Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => self.failed = Some(*settings),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    return;
                }
            }
            self.making = None;
        }
        let wanted = self.settings();
        if self.made.as_ref().is_some_and(|(s, _, _)| *s == wanted) || self.failed == Some(wanted) {
            return;
        }
        // a model's turntable is made once Done is pressed
        if wanted.3.is_some() && !self.saving {
            return;
        }
        let (tx, rx) = channel();
        let session = Arc::clone(session);
        let format = self.format;
        let (web_ready, dither, transparent, turntable) = wanted;
        let video_done = Arc::clone(&self.video_done);
        video_done.store(0, Ordering::Relaxed);
        // among the files being written (the app waits for them as it ends; the panel over the
        // canvas keeps presses off it meanwhile)
        crate::files::write_meanwhile(false, move || {
            let dir = std::env::temp_dir().join("procraft-exports");
            let base = procraft_core::export_file_name(session.name(), false);
            let path = dir.join(format!("{base}.{}", format.extension()));
            let side = if web_ready { WEB_READY_SIDE } else { 0 };
            // Animation Assist's own frame rate, when it is on
            let fps = session.animation_settings().map_or(ANIMATION_FPS, |a| a.fps) as f32;
            let file = path.to_string_lossy().into_owned();
            let made = std::fs::create_dir_all(&dir).map_err(|e| e.to_string());
            let made = made.and_then(|()| match turntable {
                Some((t, size)) => turntable_file(&session, format, &path, t, size, &video_done),
                None => Ok(()),
            });
            let made = made.and_then(|()| match format {
                _ if turntable.is_some() => Ok(()),
                Animated::Gif => session.export_gif(file, side, fps, dither, transparent).map_err(|e| e.to_string()),
                Animated::Png => session.export_apng(file, side, fps, transparent).map_err(|e| e.to_string()),
                Animated::Mp4 | Animated::Hevc => {
                    let info = session.info();
                    let order = session.animation_sequence();
                    let most = if web_ready { WEB_READY_SIDE } else { MAX_VIDEO_SIDE };
                    let hevc = format == Animated::Hevc;
                    crate::video::write_mp4(
                        &path,
                        info.width,
                        info.height,
                        most,
                        fps as u32,
                        hevc,
                        4,
                        order.len(),
                        |i, w, h| {
                            video_done.store((i as f32 / order.len() as f32).to_bits(), Ordering::Relaxed);
                            let t = session.animation_frame(*order.get(i)?, false, w.max(h))?;
                            let picture = image::RgbaImage::from_raw(t.width, t.height, t.rgba)?;
                            let picture = if picture.dimensions() == (w, h) {
                                picture
                            } else {
                                image::imageops::resize(&picture, w, h, image::imageops::FilterType::Triangle)
                            };
                            Some(picture.into_raw())
                        },
                    )
                    .map(|_| ())
                }
            });
            let result = match made {
                Ok(()) => std::fs::metadata(&path).ok().map(|m| (path, m.len())),
                Err(e) => {
                    log::warn!("exporting an animation: {e}");
                    None
                }
            };
            let _ = tx.send(result);
        });
        self.made = None;
        self.making = Some((wanted, rx));
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    /// The preview's frames, fitted to `side`, read again (off the window's thread) when the
    /// background or the box changed; then played over and over.
    fn preview(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>, side: u32) {
        let turn = self.turntable.map(|(t, _)| t);
        let wanted = (self.transparent && self.can_transparent(), side, turn);
        if let Some((key, rx)) = &self.reading {
            match rx.try_recv() {
                Ok(frames) => {
                    self.frames = frames
                        .iter()
                        .enumerate()
                        .map(|(i, t)| {
                            let size = [t.width as usize, t.height as usize];
                            let image = egui::ColorImage::from_rgba_unmultiplied(size, &t.rgba);
                            ctx.load_texture(format!("export frame {i}"), image, egui::TextureOptions::LINEAR)
                        })
                        .collect();
                    self.frames_for = Some(*key);
                    self.shown = 0;
                    self.reading = None;
                }
                Err(_) => ctx.request_repaint_after(Duration::from_millis(50)),
            }
        }
        if self.reading.is_none() && self.frames_for != Some(wanted) {
            let (tx, rx) = channel();
            let session = Arc::clone(session);
            std::thread::spawn(move || {
                let frames = match wanted.2 {
                    // the turntable's frames, half the box across
                    Some(t) => {
                        let count = ((t.seconds * PREVIEW_FPS as f32).round() as u32).max(1);
                        let half = (wanted.1 / 2).max(1);
                        (0..count)
                            .filter_map(|i| session.model_turntable_picture(i, count, t, half, half, false))
                            .collect()
                    }
                    None => (0..session.animation_frame_count())
                        .filter_map(|i| session.animation_frame(i, wanted.0, wanted.1))
                        .collect(),
                };
                let _ = tx.send(frames);
            });
            self.reading = Some((wanted, rx));
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if self.frames.len() > 1 {
            let step = Duration::from_millis(1000 / PREVIEW_FPS);
            if self.stepped.elapsed() >= step {
                self.shown = (self.shown + 1) % self.frames.len();
                self.stepped = Instant::now();
            }
            ctx.request_repaint_after(step.saturating_sub(self.stepped.elapsed()));
        }
    }

    /// Shows the screen over everything. Wide, the settings stay in a 400-wide panel on the left
    /// and the artwork's name and the file's size sit over the preview, which fills the rest (41
    /// in at the sides, 110 down). Upright, "Export Settings" opens the panel over a square
    /// preview (24 in), a tap beside it closing it again. Cancel and the round Done are at the
    /// top right.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &Arc<CanvasSession>) -> Option<Ended> {
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        let left = if wide { PANEL_WIDTH + 10.0 } else { 0.0 };
        let inset = if wide { 41.0 } else { 24.0 };
        let box_width = screen.width() - left - inset * 2.0;
        let box_height = if wide { box_width.min(screen.height() - 104.0) } else { box_width };
        let preview = Rect::from_min_size(screen.min + vec2(left + inset, 110.0), vec2(box_width, box_height));
        let side = (box_width.min(box_height) * ctx.pixels_per_point()).round().max(1.0) as u32;
        self.make_file(ctx, session);
        self.preview(ctx, session, side);
        let mut cancel = false;
        let mut done = false;
        let size = self.made.as_ref().filter(|_| self.turntable.is_none()).map(|(_, _, bytes)| file_size(*bytes));
        egui::Area::new(Id::unique("animated export")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            // the whole window: nothing under it takes a press
            let beside = ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0, argb(SCREEN_FILL));
            if let Some(t) = self.frames.get(self.shown) {
                let [w, h] = t.size().map(|v| v.max(1) as f32);
                let k = (preview.width() / w).min(preview.height() / h);
                let picture = Rect::from_center_size(preview.center(), vec2(w * k, h * k));
                ui.painter().image(t.id(), picture, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            if wide {
                // the name and size centred between the panel and the buttons
                let middle = screen.left() + left + 24.0 + (screen.width() - left - 24.0 - 224.0) / 2.0;
                let name = title_of(session);
                text(
                    ui.painter(),
                    pos2(middle, screen.top() + 23.0 + 15.0),
                    Align2::CENTER_CENTER,
                    &name,
                    20.0,
                    Weight::Bold,
                    argb(TITLE_INK),
                );
                if let Some(s) = &size {
                    let at = pos2(middle, screen.top() + 23.0 + 30.0 + 10.0);
                    text(ui.painter(), at, Align2::CENTER_CENTER, s, 13.0, Weight::Regular, argb(SIZE_INK));
                }
            } else {
                let button = Rect::from_min_size(screen.min + vec2(14.0, 20.0), vec2(60.0 + 110.0, 58.0));
                look.icon(
                    ui,
                    &icons::CHEVRON_LEFT,
                    pos2(button.left() + 30.0, button.center().y),
                    28.0,
                    argb(TITLE_INK),
                );
                let at = pos2(button.left() + 60.0, button.center().y - 14.5);
                text(ui.painter(), at, Align2::LEFT_CENTER, "Export", 24.0, Weight::Bold, argb(TITLE_INK));
                let at = pos2(button.left() + 60.0, button.center().y + 14.5);
                text(ui.painter(), at, Align2::LEFT_CENTER, "Settings", 24.0, Weight::Bold, argb(TITLE_INK));
                if ui.interact(button, ui.scope_id().with("export settings"), Sense::click()).clicked() {
                    self.settings_open = !self.settings_open;
                }
            }
            // Cancel and Done at the top right
            let done_rect =
                Rect::from_min_size(pos2(screen.right() - 16.0 - 44.0, screen.top() + 16.0), vec2(44.0, 45.0));
            let galley = ui.painter().layout_no_wrap("Cancel".to_owned(), Weight::Regular.font(17.0), argb(CANCEL_INK));
            let capsule = Rect::from_min_size(
                pos2(done_rect.left() - 13.0 - galley.size().x - 26.0, done_rect.center().y - 21.5),
                vec2(galley.size().x + 26.0, 43.0),
            );
            ui.painter().rect_filled(capsule, 21.5, argb(CAPSULE_FILL));
            ui.painter().rect_stroke(capsule, 21.5, Stroke::new(1.0, argb(CAPSULE_EDGE)), egui::StrokeKind::Inside);
            ui.painter().galley(capsule.center() - galley.size() / 2.0, galley, argb(CANCEL_INK));
            cancel = ui.interact(capsule, ui.scope_id().with("cancel"), Sense::click()).clicked();
            ui.painter().circle_filled(done_rect.center(), 22.0, argb(DONE_BLUE));
            // while it is made the export card over the screen shows how far it has come
            look.icon(ui, &icons::DONE_TICK, done_rect.center(), 40.0, Color32::WHITE);
            if !self.saving && !self.card.busy() {
                done = ui.interact(done_rect, ui.scope_id().with("done"), Sense::click()).clicked();
            }
            if wide || self.settings_open {
                let panel = Rect::from_min_max(
                    screen.min + vec2(10.0, 10.0),
                    pos2(screen.left() + 10.0 + PANEL_WIDTH, screen.bottom() - 10.0),
                );
                // upright, a tap outside the panel closes it
                if !wide && beside.clicked() && beside.interact_pointer_pos().is_some_and(|p| !panel.contains(p)) {
                    self.settings_open = false;
                }
                ui.interact(panel, ui.scope_id().with("panel"), Sense::click_and_drag());
                ui.painter().rect_filled(panel, 20, argb(PANEL_FILL));
                ui.painter().rect_stroke(panel, 20, Stroke::new(1.0, argb(PANEL_EDGE)), egui::StrokeKind::Inside);
                let at = pos2(panel.left() + 24.0, panel.top() + 19.0 + 18.0);
                text(ui.painter(), at, Align2::LEFT_CENTER, self.format.title(), 24.0, Weight::Bold, argb(TITLE_INK));
                let mut y = panel.top() + 19.0 + 36.0 + 11.5;
                if let Some((t, size)) = self.turntable {
                    self.turntable_settings(ui, panel, y, t, size);
                    return;
                }
                // Max Resolution or Web Ready, in a grey track 384 wide
                let track = Rect::from_min_size(pos2(panel.left() + 7.5, y), vec2(384.0, 34.0));
                ui.painter().rect_filled(track, 17, argb(SEGMENT_TRACK));
                let item = (track.width() - 4.0 - 12.0) / 2.0;
                for (i, label) in ["Max Resolution", "Web Ready"].into_iter().enumerate() {
                    let cell =
                        Rect::from_min_size(track.min + vec2(2.0 + (item + 12.0) * i as f32, 2.0), vec2(item, 30.0));
                    let chosen = self.web_ready == (i == 1);
                    if chosen {
                        ui.painter().rect_filled(cell, 15, argb(pc::ACCENT));
                    }
                    let ink = if chosen { Color32::WHITE } else { argb(ROW_INK) };
                    text(ui.painter(), cell.center(), Align2::CENTER_CENTER, label, 15.0, Weight::Regular, ink);
                    if ui.interact(cell, ui.scope_id().with(("size", i)), Sense::click()).clicked() {
                        self.web_ready = i == 1;
                    }
                }
                y = track.bottom() + 6.5;
                let mut rows = Vec::new();
                if self.format == Animated::Gif {
                    rows.push(("Dithering", self.dither));
                }
                if self.can_transparent() {
                    rows.push(("Transparent background", self.transparent));
                }
                for (label, on) in rows {
                    let row = Rect::from_min_size(pos2(panel.left(), y), vec2(PANEL_WIDTH, 52.0));
                    let at = pos2(row.left() + 16.0, row.center().y);
                    text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(ROW_INK));
                    crate::transform::switch(ui, pos2(row.left() + 16.0 + 307.0, row.center().y - 14.0), on, label);
                    if ui.interact(row, ui.scope_id().with(label), Sense::click()).clicked() {
                        if label == "Dithering" {
                            self.dither = !self.dither;
                        } else {
                            self.transparent = !self.transparent;
                        }
                    }
                    y += 52.0;
                }
            }
        });
        if cancel || (ui_escape(ctx) && !self.card.busy()) {
            return Some(Ended::Closed);
        }
        if done {
            self.saving = true;
            // the card follows the file being made: a video by its frames, else as the core tells
            let video = matches!(self.format, Animated::Mp4 | Animated::Hevc);
            // from the start, not where the last export left it
            session.reset_export_progress();
            let (following, video_done) = (Arc::clone(session), Arc::clone(&self.video_done));
            self.exporting = Some(self.card.start(move || {
                if video { f32::from_bits(video_done.load(Ordering::Relaxed)) } else { following.export_progress() }
            }));
        }
        // Cancel on the card: the file isn't saved (its card goes by itself)
        if self.exporting.as_ref().is_some_and(crate::export_card::Exporting::cancelled) {
            self.saving = false;
            self.exporting = None;
        }
        if self.saving && self.failed == Some(self.settings()) {
            self.saving = false;
            if let Some(exporting) = self.exporting.take() {
                exporting.ended(false);
            }
            return Some(Ended::Failed);
        }
        // Done: once the file for the settings is made, the card shows its check
        if self.saving && self.made.as_ref().is_some_and(|(settings, _, _)| *settings == self.settings()) {
            self.saving = false;
            if let Some(exporting) = self.exporting.take() {
                exporting.ended(true);
            }
        }
        // and as it goes, the file is saved where asked
        if self.card.show(ctx, look) == Some(crate::export_card::Ended::Made)
            && let Some((settings, path, _)) = &self.made
            && *settings == self.settings()
            && save_as(path, self.format)
        {
            return Some(Ended::Closed);
        }
        None
    }
}

impl AnimatedExport {
    /// A model's turntable settings in `panel` from `y` down: its size, rotate or swing, its
    /// duration, zoom and ease, and the environment behind it.
    fn turntable_settings(&mut self, ui: &egui::Ui, panel: Rect, mut y: f32, t: Turntable, size: usize) {
        let mut t = t;
        let mut size = size;
        // the sizes, in a grey track 384 wide
        let track = Rect::from_min_size(pos2(panel.left() + 7.5, y), vec2(384.0, 34.0));
        ui.painter().rect_filled(track, 17, argb(SEGMENT_TRACK));
        let item = (track.width() - 4.0 - 12.0 * 3.0) / 4.0;
        for (i, (_, _, label)) in TURNTABLE_SIZES.iter().enumerate() {
            let cell = Rect::from_min_size(track.min + vec2(2.0 + (item + 12.0) * i as f32, 2.0), vec2(item, 30.0));
            let chosen = size == i;
            if chosen {
                ui.painter().rect_filled(cell, 15, argb(pc::ACCENT));
            }
            let ink = if chosen { Color32::WHITE } else { argb(ROW_INK) };
            text(ui.painter(), cell.center(), Align2::CENTER_CENTER, label, 15.0, Weight::Regular, ink);
            if ui.interact(cell, ui.scope_id().with(("turntable size", i)), Sense::click()).clicked() {
                size = i;
            }
        }
        y = track.bottom() + 14.5;
        // rotate or swing, the chosen one blue with a tick
        let group = Rect::from_min_size(pos2(panel.left() + 16.0, y), vec2(PANEL_WIDTH - 32.0, 80.0));
        ui.painter().rect_filled(group, 14, argb(CHOICE_FILL));
        for (i, (label, swing)) in [("Animated rotate", false), ("Animated swing", true)].into_iter().enumerate() {
            let row = Rect::from_min_size(pos2(group.left(), group.top() + 40.0 * i as f32), vec2(group.width(), 40.0));
            let chosen = t.swing == swing;
            if chosen {
                let corners = if i == 0 {
                    egui::CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 }
                } else {
                    egui::CornerRadius { nw: 0, ne: 0, sw: 14, se: 14 }
                };
                ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
                let disc = pos2(row.right() - 12.0 - 10.0, row.center().y);
                ui.painter().circle_filled(disc, 10.0, Color32::WHITE);
                // the tick is drawn in a 24 frame, shown 14 across
                ui.painter().add(egui::Shape::line(
                    vec![disc + vec2(-4.0, 0.0), disc + vec2(-1.0, 3.0), disc + vec2(4.5, -3.0)],
                    Stroke::new(2.0, argb(pc::ACCENT)),
                ));
            }
            let ink = if chosen { Color32::WHITE } else { argb(ROW_INK) };
            text(
                ui.painter(),
                pos2(row.left() + 16.0, row.center().y),
                Align2::LEFT_CENTER,
                label,
                17.0,
                Weight::Regular,
                ink,
            );
            if ui.interact(row, ui.scope_id().with(("turntable way", i)), Sense::click()).clicked() {
                t.swing = swing;
            }
        }
        y = group.bottom() + 26.0;
        let seconds = if t.seconds < 1.0 { format!("{:.1}", t.seconds) } else { format!("{}", t.seconds.round()) };
        let rows: [(&str, String, f32); 3] = [
            ("Animation duration", seconds, (t.seconds - 0.5) / 4.5),
            ("Zoom distance", format!("{}%", (t.zoom * 100.0).round()), t.zoom),
            ("Ease", format!("{}%", (t.ease * 100.0).round()), t.ease),
        ];
        for (k, (label, value, fraction)) in rows.into_iter().enumerate() {
            let row = Rect::from_min_size(pos2(panel.left() + 8.0, y), vec2(PANEL_WIDTH - 16.0, 66.0));
            text(
                ui.painter(),
                row.min + vec2(16.0, 6.0),
                Align2::LEFT_TOP,
                label,
                17.0,
                Weight::Regular,
                argb(ROW_INK),
            );
            text(
                ui.painter(),
                pos2(row.right() - 16.0, row.top() + 6.0),
                Align2::RIGHT_TOP,
                &value,
                17.0,
                Weight::Regular,
                argb(ROW_INK),
            );
            let slider =
                Rect::from_min_max(pos2(row.left(), row.bottom() - 8.0 - 28.0), pos2(row.right(), row.bottom() - 8.0));
            let (x0, x1, cy) = (slider.left() + 16.0, slider.right() - 16.0, slider.center().y);
            let x = x0 + (x1 - x0) * fraction.clamp(0.0, 1.0);
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(x0, cy - 1.5), pos2(x1, cy + 1.5)),
                1.5,
                argb(SLIDER_TRACK),
            );
            ui.painter().rect_filled(Rect::from_min_max(pos2(x0, cy - 1.5), pos2(x, cy + 1.5)), 1.5, argb(pc::ACCENT));
            ui.painter().rect_filled(Rect::from_center_size(pos2(x, cy), vec2(26.0, 16.0)), 8.0, Color32::WHITE);
            let response = ui.interact(slider, ui.scope_id().with(("turntable slider", k)), Sense::click_and_drag());
            let by = crate::look::dragged_by(&mut self.held, &response);
            if by != 0.0 {
                let v = (fraction + by / (x1 - x0)).clamp(0.0, 1.0);
                match k {
                    0 => t.seconds = 0.5 + v * 4.5,
                    1 => t.zoom = v,
                    _ => t.ease = v,
                }
            }
            y += 66.0;
        }
        y += 10.0;
        let row = Rect::from_min_size(pos2(panel.left(), y), vec2(PANEL_WIDTH, 52.0));
        let at = pos2(row.left() + 16.0, row.center().y);
        text(ui.painter(), at, Align2::LEFT_CENTER, "Show environment", 17.0, Weight::Regular, argb(ROW_INK));
        crate::transform::switch(
            ui,
            pos2(row.left() + 16.0 + 307.0, row.center().y - 14.0),
            t.environment,
            "turntable environment",
        );
        if ui.interact(row, ui.scope_id().with("turntable environment"), Sense::click()).clicked() {
            t.environment = !t.environment;
        }
        self.turntable = Some((t, size));
    }
}

/// A model's turntable `t` at size `size` (of [`TURNTABLE_SIZES`]) written as `format` to `path`.
fn turntable_file(
    session: &CanvasSession,
    format: Animated,
    path: &std::path::Path,
    t: Turntable,
    size: usize,
    video_done: &AtomicU32,
) -> Result<(), String> {
    let (w, h, _) = TURNTABLE_SIZES.get(size).copied().unwrap_or((1280, 720, "720p"));
    let file = path.to_string_lossy().into_owned();
    match format {
        Animated::Gif => session.export_model_gif(file, t, w, h, TURNTABLE_FPS).map_err(|e| e.to_string()),
        Animated::Png => session.export_model_apng(file, t, w, h, TURNTABLE_FPS).map_err(|e| e.to_string()),
        Animated::Mp4 | Animated::Hevc => {
            let count = ((t.seconds * TURNTABLE_FPS).round() as u32).max(1);
            let hevc = format == Animated::Hevc;
            crate::video::write_mp4(path, w, h, w.max(h), TURNTABLE_FPS as u32, hevc, 4, count as usize, |i, vw, vh| {
                video_done.store((i as f32 / count as f32).to_bits(), Ordering::Relaxed);
                session.model_turntable_picture(i as u32, count, t, vw, vh, false).map(|p| p.rgba)
            })
            .map(|_| ())
        }
    }
}

fn ui_escape(ctx: &egui::Context) -> bool {
    crate::ui::escape(ctx)
}

/// The artwork's name, or "Untitled Artwork".
fn title_of(session: &CanvasSession) -> String {
    let name = session.name();
    if name.trim().is_empty() { "Untitled Artwork".to_owned() } else { name.trim().to_owned() }
}

/// A file's size as the screen shows it: "1.3 MB", "850 KB".
fn file_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} KB", (bytes / 1000).max(1))
    }
}

/// The file made copied where the person picks; true once saved (false if they cancelled).
fn save_as(made: &std::path::Path, format: Animated) -> bool {
    let name = made.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(to) = rfd::FileDialog::new()
        .set_title(format!("Share as {}", format.title()))
        .set_file_name(name)
        .add_filter(format.title(), &[format.extension()])
        .save_file()
    else {
        return false;
    };
    match std::fs::copy(made, &to) {
        Ok(_) => true,
        Err(e) => {
            crate::files::tell("Can't export", &e.to_string());
            false
        }
    }
}
