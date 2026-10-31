//! The artwork's time-lapse, as the Android app keeps it (canvas/TimeLapse.kt, ui/TimeLapseUi.kt):
//! a frame recorded after each change (the core keeps them), the replay over everything, and the
//! video made of it (full length or 30 seconds) with the system's encoder.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Order, Rect, Sense, TextureHandle, pos2, vec2};
use procraft_core::{
    CanvasSession, TIME_LAPSE_FPS, TimeLapse, time_lapse_bits_per_pixel, time_lapse_picks, time_lapse_side,
};

use crate::alert::{Alert, Answer, Asking};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, text};
use crate::tool_header;

const REPLAY_FILL: Themed = Themed(0xFF252525, 0xFFCED1D2);
const GRID_LINE: Themed = Themed(0xFF2E2E2E, 0xFFCACDCE);
const EMPTY_INK: u32 = 0xFF8E8E93;
const PROGRESS: u32 = 0xFF0076FD;

#[derive(Default)]
pub struct TimeLapseUi {
    /// The time-lapse of the artwork open, and its folder.
    lapse: Option<(String, Arc<TimeLapse>)>,
    /// The edits last recorded, and a frame being recorded (a change meanwhile is recorded after).
    recorded: Option<u64>,
    recording: Option<Receiver<()>>,
    replay: Option<Replay>,
    length: Alert,
    /// The video being made, to save once made.
    exporting: Option<Receiver<Result<PathBuf, String>>>,
}

impl TimeLapseUi {
    /// The artwork in folder `dir` (none: not kept in the gallery) is open, its edits now `edits`.
    pub fn open(&mut self, dir: Option<&str>, edits: u64) {
        self.lapse = dir.map(|d| (d.to_owned(), TimeLapse::new(d.to_owned())));
        self.recorded = Some(edits);
        self.replay = None;
    }

    /// Whether it records changes (the Video tab's switch).
    pub fn recording(&self) -> bool {
        self.lapse.as_ref().is_none_or(|(_, t)| t.recording())
    }

    pub fn set_recording(&self, on: bool) {
        if let Some((_, t)) = &self.lapse {
            t.set_recording(on);
        }
    }

    /// The artwork changed since the last frame (and nothing is pressed on it): a picture of it
    /// is recorded off the window's thread, one at a time while changes come.
    pub fn record(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>, idle: bool) {
        let Some((_, lapse)) = &self.lapse else { return };
        if let Some(rx) = &self.recording {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    return;
                }
                _ => self.recording = None,
            }
        }
        let edits = session.edits();
        if !idle || self.recorded == Some(edits) || !lapse.recording() {
            return;
        }
        self.recorded = Some(edits);
        let (tx, rx) = channel();
        let (session, lapse) = (Arc::clone(session), Arc::clone(lapse));
        std::thread::spawn(move || {
            // frames as large as the video they make
            let side = time_lapse_side(lapse.settings());
            if let Some(shot) = session.canvas_thumbnail(side) {
                lapse.record(shot.rgba, shot.width, shot.height);
            }
            let _ = tx.send(());
        });
        self.recording = Some(rx);
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    /// The replay starts, over everything.
    pub fn replay(&mut self) {
        let count = self.lapse.as_ref().map_or(0, |(_, t)| t.count());
        self.replay = Some(Replay::new(count));
    }

    pub fn replaying(&self) -> bool {
        self.replay.is_some()
    }

    /// Export time-lapse video: how long, asked first.
    pub fn ask_length(&mut self) {
        self.length.ask(Asking {
            title: "Video length".into(),
            message: String::new(),
            action: "30 Seconds".into(),
            icon: Some(&icons::CLOCK),
            destructive: false,
            extra: None,
            first: Some("Full length".into()),
        });
    }

    /// The replay while it shows, the length alert, and a video being made (named after the
    /// artwork's `name`); returns a message to show, if any.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, name: &str) -> Option<&'static str> {
        if let Some(replay) = self.replay.as_mut() {
            let lapse = self.lapse.as_ref().map(|(_, t)| t.as_ref());
            if replay.show(ctx, look, lapse) {
                self.replay = None;
            }
        }
        let mut said = None;
        match self.length.show(ctx, look) {
            Some(Answer::First) => said = self.export(None, name),
            Some(Answer::Action) => said = self.export(Some(30), name),
            _ => {}
        }
        if let Some(rx) = &self.exporting {
            match rx.try_recv() {
                Ok(made) => {
                    self.exporting = None;
                    match made {
                        Ok(path) => save_as(&path),
                        Err(e) => {
                            log::warn!("exporting the time-lapse: {e}");
                            said = Some("There is no time-lapse to export yet.");
                        }
                    }
                }
                Err(_) => ctx.request_repaint_after(Duration::from_millis(100)),
            }
        }
        said
    }

    /// The video made off the window's thread: all the frames, or `seconds` of them spread
    /// evenly; each fitted on white into the first one's size, within the settings' side.
    fn export(&mut self, seconds: Option<u32>, name: &str) -> Option<&'static str> {
        let Some((_, lapse)) = &self.lapse else { return Some("There is no time-lapse to export yet.") };
        if lapse.count() == 0 {
            return Some("There is no time-lapse to export yet.");
        }
        let (tx, rx) = channel();
        let lapse = Arc::clone(lapse);
        let name: String = name.chars().filter(|c| c.is_alphanumeric() || " _-".contains(*c)).collect();
        let name = if name.trim().is_empty() { "Untitled Artwork".to_owned() } else { name.trim().to_owned() };
        std::thread::spawn(move || {
            let made = (|| {
                let picks = time_lapse_picks(lapse.count(), seconds);
                let first = lapse.frame(*picks.first().ok_or("no frames")?).ok_or("the first frame can't be read")?;
                let settings = lapse.settings();
                let out = std::env::temp_dir().join("procraft-exports").join(format!("{name}.mp4"));
                std::fs::create_dir_all(out.parent().unwrap_or(&out)).map_err(|e| e.to_string())?;
                crate::video::write_mp4(
                    &out,
                    first.width,
                    first.height,
                    time_lapse_side(settings),
                    TIME_LAPSE_FPS,
                    settings.hevc,
                    time_lapse_bits_per_pixel(settings),
                    picks.len(),
                    |n, w, h| Some(on_white(&lapse.frame(*picks.get(n)?)?, w, h)),
                )?;
                Ok(out)
            })();
            let _ = tx.send(made);
        });
        self.exporting = Some(rx);
        None
    }
}

/// The time-lapse of the artwork in folder `dir` as a package keeps it, its frames made into
/// videos first (only those since the last share, when the videos made then still hold).
pub fn for_package(dir: &str) -> procraft_core::PackageTimeLapse {
    let lapse = TimeLapse::new(dir.to_owned());
    let mut plan = lapse.segment_plan();
    while let Some(p) = plan {
        plan = match make_segment(&lapse, &p) {
            Ok((w, h)) => lapse.segment_made(p, w, h, true),
            Err(e) => {
                log::warn!("a time-lapse video for a package: {e}");
                lapse.segment_made(p, 0, 0, false)
            }
        };
    }
    lapse.for_package()
}

/// `plan`'s frames as a video, fitted on white; one added to those before comes out their size.
fn make_segment(lapse: &TimeLapse, plan: &procraft_core::SegmentPlan) -> Result<(u32, u32), String> {
    let settings = lapse.settings();
    let (width, height, side) = if plan.width > 0 && plan.height > 0 {
        (plan.width, plan.height, plan.width.max(plan.height))
    } else {
        let first = lapse.frame(*plan.frames.first().ok_or("no frames")?).ok_or("the first frame can't be read")?;
        (first.width, first.height, time_lapse_side(settings))
    };
    let size = crate::video::write_mp4(
        std::path::Path::new(&plan.path),
        width,
        height,
        side,
        TIME_LAPSE_FPS,
        settings.hevc,
        time_lapse_bits_per_pixel(settings),
        plan.frames.len(),
        |n, w, h| Some(on_white(&lapse.frame(*plan.frames.get(n)?)?, w, h)),
    )?;
    if plan.append && size != (plan.width, plan.height) {
        return Err("the encoder won't make it the size of those before".into());
    }
    Ok(size)
}

/// The time-lapse of the package at `package` taken into the artwork folder `dir`: its settings,
/// and its videos' frames (the videos kept to go back into a package when they all came in).
pub fn take_package(package: &std::path::Path, dir: &str) {
    let temp = std::env::temp_dir().join(format!("procraft-segments-{}", std::process::id()));
    let taken =
        procraft_core::package_time_lapse(package.to_string_lossy().into_owned(), temp.to_string_lossy().into_owned());
    let t = match taken {
        Ok(Some(t)) => t,
        Ok(None) => return,
        Err(e) => {
            log::warn!("a package's time-lapse: {e}");
            return;
        }
    };
    let lapse = TimeLapse::new(dir.to_owned());
    let videos = t.segments.clone();
    lapse.take_begin(t);
    for path in videos {
        let read = crate::video::read_frames(std::path::Path::new(&path), |rgba, w, h| {
            lapse.keep_frame(rgba, w, h);
        });
        let frames = read.unwrap_or_else(|e| {
            log::warn!("a package's time-lapse video: {e}");
            0
        });
        lapse.take_video(path, frames);
    }
    lapse.take_end();
    let _ = std::fs::remove_dir_all(&temp);
}

/// `frame` fitted into `w` × `h` on white, as RGBA rows.
fn on_white(frame: &procraft_core::ThumbnailImage, w: u32, h: u32) -> Vec<u8> {
    let mut out = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
    if let Some(picture) = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()) {
        let k = (w as f32 / frame.width.max(1) as f32).min(h as f32 / frame.height.max(1) as f32);
        let (dw, dh) = (((frame.width as f32 * k) as u32).max(1), ((frame.height as f32 * k) as u32).max(1));
        let fitted = image::imageops::resize(&picture, dw, dh, image::imageops::FilterType::Triangle);
        image::imageops::overlay(&mut out, &fitted, i64::from((w - dw.min(w)) / 2), i64::from((h - dh.min(h)) / 2));
    }
    out.into_raw()
}

/// The video made copied where the person picks.
fn save_as(made: &std::path::Path) {
    let name = made.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(to) = rfd::FileDialog::new()
        .set_title("Export time-lapse video")
        .set_file_name(name)
        .add_filter("MP4 video", &["mp4"])
        .save_file()
    else {
        return;
    };
    if let Err(e) = std::fs::copy(made, &to) {
        crate::files::tell("Can't export", &e.to_string());
    }
}

/// The time-lapse replayed over everything at its frame rate; dragging across the screen scrubs
/// through it. A bar on top: the title, the time into the replay, and Done.
struct Replay {
    count: u32,
    index: u32,
    stepped: Instant,
    /// Dragged: where it began and the frame then.
    scrub: Option<(f32, u32)>,
    shown: Option<(u32, TextureHandle)>,
}

impl Replay {
    fn new(count: u32) -> Replay {
        Replay { count, index: 0, stepped: Instant::now(), scrub: None, shown: None }
    }

    /// True when Done was tapped (or Escape).
    fn show(&mut self, ctx: &egui::Context, look: &mut Look, lapse: Option<&TimeLapse>) -> bool {
        let screen = ctx.content_rect();
        // play, frame by frame, until the end or a scrub
        let step = Duration::from_millis(1000 / u64::from(TIME_LAPSE_FPS));
        if self.scrub.is_none() && self.index + 1 < self.count {
            if self.stepped.elapsed() >= step {
                self.index += 1;
                self.stepped = Instant::now();
            }
            ctx.request_repaint_after(step.saturating_sub(self.stepped.elapsed()));
        }
        if self.shown.as_ref().is_none_or(|(i, _)| *i != self.index)
            && let Some(frame) = lapse.and_then(|t| t.frame(self.index))
        {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba);
            self.shown = Some((self.index, ctx.load_texture("time-lapse", image, egui::TextureOptions::LINEAR)));
        }
        egui::Area::new(Id::unique("time-lapse replay")).fixed_pos(screen.min).order(Order::Foreground).show(
            ctx,
            |ui| {
                let response = ui.allocate_rect(screen, Sense::click_and_drag());
                ui.painter().rect_filled(screen, 0, argb(REPLAY_FILL));
                surround_grid(ui, screen);
                if let Some((_, t)) = &self.shown {
                    // edge to edge, under the header
                    let [w, h] = t.size().map(|v| v.max(1) as f32);
                    let k = (screen.width() / w).min(screen.height() / h);
                    let picture = Rect::from_center_size(screen.center(), vec2(w * k, h * k));
                    ui.painter().image(
                        t.id(),
                        picture,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                if self.count == 0 {
                    let ink = argb(EMPTY_INK);
                    text(
                        ui.painter(),
                        screen.center(),
                        Align2::CENTER_CENTER,
                        "Nothing recorded yet",
                        17.0,
                        Weight::Regular,
                        ink,
                    );
                }
                if response.drag_started()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    self.scrub = Some((p.x, self.index));
                }
                if let (Some((start, from)), Some(p)) = (self.scrub, response.interact_pointer_pos())
                    && response.dragged()
                {
                    let moved = (p.x - start) / screen.width();
                    let to = from as f32 + (moved * self.count as f32).trunc();
                    self.index = to.clamp(0.0, self.count.saturating_sub(1) as f32) as u32;
                }
                if response.drag_stopped() {
                    self.scrub = None;
                    self.stepped = Instant::now();
                }
                // how far it has played
                let f = if self.count > 1 { self.index as f32 / (self.count - 1) as f32 } else { 1.0 };
                let bar = Rect::from_min_size(
                    pos2(screen.left(), screen.top() + tool_header::HEIGHT),
                    vec2(screen.width() * f.clamp(0.0, 1.0), 4.0),
                );
                ui.painter().rect_filled(bar, 0, argb(PROGRESS));
            },
        );
        // the time into the replay, in frames past the second
        let seconds = self.index / TIME_LAPSE_FPS;
        let time = format!(
            "{:02}:{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            self.index % TIME_LAPSE_FPS
        );
        let tapped = tool_header::show_in(ctx, look, "Time-lapse Replay", &[(&time, false)], tool_header::OVER_PICTURE);
        tapped == Some(tool_header::Tapped::Done) || ctx.input(|i| i.key_pressed(egui::Key::Escape))
    }
}

/// The surround's grid as the canvas draws it (1 1/3 pt lines every 16, none under the top bar),
/// for screens shown in the canvas's place.
fn surround_grid(ui: &egui::Ui, screen: Rect) {
    let (step, width, top) = (16.0, 4.0 / 3.0, screen.top() + 44.0);
    let origin = screen.min + vec2(0.5, 0.5);
    let mut x = origin.x;
    while x < screen.right() {
        let line = Rect::from_min_max(pos2(x - width / 2.0, top), pos2(x + width / 2.0, screen.bottom()));
        ui.painter().rect_filled(line, 0, argb(GRID_LINE));
        x += step;
    }
    let mut y = origin.y - ((origin.y - top) / step).floor() * step;
    while y < screen.bottom() {
        if y - width / 2.0 >= top {
            let line = Rect::from_min_max(pos2(screen.left(), y - width / 2.0), pos2(screen.right(), y + width / 2.0));
            ui.painter().rect_filled(line, 0, argb(GRID_LINE));
        }
        y += step;
    }
}
