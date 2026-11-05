//! The artwork's time-lapse, as the Android app keeps it (canvas/TimeLapse.kt, ui/TimeLapseUi.kt):
//! a frame recorded after each change (the core keeps them), the replay over everything, and the
//! video made of it (full length or 30 seconds) with the system's encoder.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel, sync_channel};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Order, Rect, Sense, TextureHandle, pos2, vec2};
use procraft_core::{
    CanvasSession, TIME_LAPSE_FPS, ThumbnailImage, TimeLapse, time_lapse_bits_per_pixel, time_lapse_picks,
    time_lapse_side,
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
    /// Recording turned off with something recorded: whether to delete it.
    purge: Alert,
    /// The video being made, to save once made, as its card shows it; made, it waits for the card
    /// to go before it is saved.
    exporting: Option<(Receiver<Result<PathBuf, String>>, crate::export_card::Exporting)>,
    card: crate::export_card::ExportCard,
    made: Option<PathBuf>,
}

impl TimeLapseUi {
    /// The artwork in folder `dir` (none: not kept in the gallery) is open, its edits now `edits`.
    pub fn open(&mut self, dir: Option<&str>, edits: u64) {
        // the artwork left: the frames it recorded made into one more video (on the GPU where it
        // can be), off the window's thread, their pictures then gone, as the artwork file
        // keeps a time-lapse
        if let Some((left, _)) = self.lapse.take() {
            std::thread::spawn(move || {
                let _ = for_package(&left);
            });
        }
        self.lapse = dir.map(|d| (d.to_owned(), TimeLapse::new(d.to_owned())));
        // the pictures its videos hold too go, as the artwork file keeps a time-lapse: only as video
        if let Some((_, lapse)) = &self.lapse {
            let lapse = Arc::clone(lapse);
            std::thread::spawn(move || {
                let gone = drop_pictures(&lapse);
                if gone > 0 {
                    log::info!("the time-lapse's videos hold {gone} frames: their pictures went");
                }
            });
        }
        self.recorded = Some(edits);
        self.replay = None;
    }

    /// The frames recorded so far, and what they take on disk (bytes).
    pub fn info(&self) -> (u32, u64) {
        self.lapse.as_ref().map_or((0, 0), |(_, t)| (t.count(), t.bytes()))
    }

    /// The video's size, quality and codec, kept with the artwork.
    pub fn settings(&self) -> procraft_core::TimeLapseSettings {
        self.lapse.as_ref().map(|(_, t)| t.settings()).unwrap_or_default()
    }

    /// Whether it records changes (the Video tab's switch).
    pub fn recording(&self) -> bool {
        self.lapse.as_ref().is_none_or(|(_, t)| t.recording())
    }

    /// The switch goes at once; turned off with something recorded, whether to delete that is
    /// asked.
    pub fn set_recording(&mut self, on: bool) {
        let Some((_, t)) = &self.lapse else { return };
        t.set_recording(on);
        if !on && t.count() > 0 {
            self.purge.ask(Asking {
                title: "Purge Video?".into(),
                message: "The video recorded so far will be deleted.
It can't be brought back."
                    .into(),
                action: "Purge".into(),
                icon: Some(&icons::ALERT_QUESTION),
                destructive: true,
                extra: None,
                first: Some("Don't Purge".into()),
                alone: false,
            });
        }
    }

    /// Something has been recorded (Replay and Export need it).
    pub fn recorded(&self) -> bool {
        self.lapse.as_ref().is_some_and(|(_, t)| t.count() > 0)
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
            // frames as large as the video they make, private layers left out
            let side = time_lapse_side(lapse.settings());
            if let Some(shot) = session.time_lapse_frame(side) {
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
            alone: false,
        });
    }

    /// The replay while it shows, the length alert, and a video being made (named after the
    /// artwork's `name`); returns a message to show, if any.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, name: &str) -> Option<&'static str> {
        if let Some(replay) = self.replay.as_mut() {
            let lapse = self.lapse.as_ref().map(|(_, t)| t);
            if replay.show(ctx, look, lapse) {
                self.replay = None;
            }
        }
        // Purge deletes what was recorded; Don't Purge, or a tap beside it, keeps it
        if self.purge.show(ctx, look) == Some(Answer::Action)
            && let Some((_, t)) = &self.lapse
        {
            t.purge();
            self.recorded = None;
        }
        let mut said = None;
        match self.length.show(ctx, look) {
            Some(Answer::First) => said = self.export(None, name),
            Some(Answer::Action) => said = self.export(Some(30), name),
            _ => {}
        }
        if let Some((rx, exporting)) = &self.exporting {
            match rx.try_recv() {
                Ok(made) => {
                    let cancelled = exporting.cancelled();
                    self.exporting = None;
                    match made {
                        Ok(path) if !cancelled => self.made = Some(path),
                        Ok(_) => {}
                        Err(e) if !cancelled => {
                            log::warn!("exporting the time-lapse: {e}");
                            said = Some(if e == crate::video::NO_FFMPEG {
                                crate::video::NO_FFMPEG
                            } else {
                                "The time-lapse video couldn't be made."
                            });
                        }
                        Err(_) => {}
                    }
                }
                Err(_) => ctx.request_repaint_after(Duration::from_millis(100)),
            }
        }
        // saved once its card has gone, as the tablet shares it
        if self.card.show(ctx, look) == Some(crate::export_card::Ended::Made)
            && let Some(path) = self.made.take()
        {
            save_as(&path);
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
        if self.card.busy() {
            return None;
        }
        let (tx, rx) = channel();
        let lapse = Arc::clone(lapse);
        let done = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let following = Arc::clone(&done);
        let exporting = self.card.start(move || f32::from_bits(following.load(std::sync::atomic::Ordering::Relaxed)));
        let told = exporting.clone();
        let name: String = name.chars().filter(|c| c.is_alphanumeric() || " _-".contains(*c)).collect();
        let name = if name.trim().is_empty() { "Untitled Artwork".to_owned() } else { name.trim().to_owned() };
        std::thread::spawn(move || {
            let made = (|| {
                let picks = time_lapse_picks(lapse.count(), seconds);
                let stop = Arc::new(AtomicBool::new(false));
                let _stops = Stops(Arc::clone(&stop));
                let frames = read_in_order(Arc::clone(&lapse), picks.clone(), Arc::clone(&stop));
                let (_, first) = frames.recv().map_err(|_| "the first frame can't be read")?;
                let (width, height) = (first.width, first.height);
                let last = std::cell::RefCell::new(Some(first));
                let settings = lapse.settings();
                let out = std::env::temp_dir().join("procraft-exports").join(format!("{name}.mp4"));
                std::fs::create_dir_all(out.parent().unwrap_or(&out)).map_err(|e| e.to_string())?;
                crate::video::write_mp4(
                    &out,
                    width,
                    height,
                    time_lapse_side(settings),
                    TIME_LAPSE_FPS as f32,
                    settings.hevc,
                    time_lapse_bits_per_pixel(settings),
                    picks.len(),
                    |n, w, h| {
                        // cancelled, the frames stop (and the video is not made)
                        if told.cancelled() {
                            return None;
                        }
                        let progress = n as f32 / picks.len().max(1) as f32;
                        done.store(progress.to_bits(), std::sync::atomic::Ordering::Relaxed);
                        // the first was read already; one that can't be read, the one before again
                        if n > 0
                            && let Ok((_, frame)) = frames.recv()
                        {
                            *last.borrow_mut() = Some(frame);
                        }
                        Some(on_white(last.borrow().as_ref()?, w, h))
                    },
                )?;
                Ok(out)
            })();
            told.ended(made.is_ok() && !told.cancelled());
            let _ = tx.send(made);
        });
        self.exporting = Some((rx, exporting));
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
    // the pictures now in a video go, as the artwork file keeps a time-lapse: only as video
    drop_pictures(&lapse);
    lapse.for_package()
}

/// The pictures its videos hold too gone, once this system has read those videos: videos go
/// through its own decoders, and where it has none for them the pictures are all there is to
/// replay. How many went.
fn drop_pictures(lapse: &TimeLapse) -> u32 {
    let readable = lapse.clips_with_pictures().iter().all(|clip| {
        let mut read = false;
        let done = crate::video::read_frames(std::path::Path::new(&clip.path), |_, _, _, _| {
            read = true;
            false
        });
        if let Err(e) = done {
            log::warn!("a time-lapse video: {e}");
        }
        read
    });
    if readable {
        lapse.drop_clip_frames()
    } else {
        log::warn!("the time-lapse's videos can't be read here: their pictures stay");
        0
    }
}

/// Sets its flag once dropped: what it stops, stopped however its holder ends.
struct Stops(Arc<AtomicBool>);

impl Drop for Stops {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Frames `wanted` (rising) of `lapse`, each with its number, read in order off the window's
/// thread a few ahead: from its videos (decoded as they come, the artwork file's way of keeping them)
/// and its pictures. A frame that can't be read is left out. It stops once `stop` is set or
/// nothing takes them.
fn read_in_order(lapse: Arc<TimeLapse>, wanted: Vec<u32>, stop: Arc<AtomicBool>) -> Receiver<(u32, ThumbnailImage)> {
    let (tx, rx) = sync_channel(4);
    std::thread::spawn(move || {
        let clips = lapse.clips();
        let mut next = 0usize;
        while let Some(&i) = wanted.get(next) {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let Some(clip) = clips.iter().find(|c| i >= c.first && i - c.first < c.frames) else {
                if let Some(frame) = lapse.frame(i)
                    && tx.send((i, frame)).is_err()
                {
                    return;
                }
                next += 1;
                continue;
            };
            let end = clip.first + clip.frames;
            let (mut at, mut gone) = (clip.first, false);
            let read = crate::video::read_frames(std::path::Path::new(&clip.path), |rgba, width, height, _| {
                let here = at;
                at += 1;
                if wanted.get(next) == Some(&here) {
                    next += 1;
                    if tx.send((here, ThumbnailImage { width, height, rgba })).is_err() {
                        gone = true;
                        return false;
                    }
                }
                !stop.load(Ordering::Relaxed) && wanted.get(next).is_some_and(|&n| n < end)
            });
            if gone {
                return;
            }
            if let Err(e) = read {
                log::warn!("a time-lapse video: {e}");
            }
            // frames the video didn't hold (fewer than noted): left out
            while wanted.get(next).is_some_and(|&n| n < end) {
                next += 1;
            }
        }
    });
    rx
}

/// `plan`'s frames as a video, fitted on white; one added to those before comes out their size.
fn make_segment(lapse: &TimeLapse, plan: &procraft_core::SegmentPlan) -> Result<(u32, u32), String> {
    let settings = procraft_core::TimeLapseSettings { hevc: plan.hevc, ..lapse.settings() };
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
        TIME_LAPSE_FPS as f32,
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
    // as the package keeps it: its videos as they are, nothing decoded
    if lapse.take_clips(t.clone()) {
        let _ = std::fs::remove_dir_all(&temp);
        return;
    }
    let videos = t.segments.clone();
    lapse.take_begin(t);
    for path in videos {
        let read = crate::video::read_frames(std::path::Path::new(&path), |rgba, w, h, _| {
            lapse.keep_frame(rgba, w, h);
            true
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
    /// The frames from one on, read ahead (from its videos, decoded as they come), and when it
    /// last began again elsewhere.
    ahead: Option<Ahead>,
    began: Instant,
}

/// Frames read ahead from `from`: the next to come, and one come before it was wanted.
struct Ahead {
    from: u32,
    next: u32,
    frames: Receiver<(u32, ThumbnailImage)>,
    waiting: Option<(u32, ThumbnailImage)>,
    _stops: Stops,
}

impl Replay {
    fn new(count: u32) -> Replay {
        Replay {
            count,
            index: 0,
            stepped: Instant::now(),
            scrub: None,
            shown: None,
            ahead: None,
            began: Instant::now(),
        }
    }

    /// The frame to show now, once read: frames read ahead from it, again from where it is when
    /// it jumps back or far ahead (while scrubbing, at most every 0.1 s).
    fn fetch(&mut self, ctx: &egui::Context, lapse: &Arc<TimeLapse>) {
        let want = self.index;
        if self.shown.as_ref().is_some_and(|(i, _)| *i == want) {
            return;
        }
        let off = self.ahead.as_ref().is_none_or(|a| want < a.from || want > a.next + TIME_LAPSE_FPS);
        if off && (self.scrub.is_none() || self.began.elapsed() >= Duration::from_millis(100)) {
            let stop = Arc::new(AtomicBool::new(false));
            let frames = read_in_order(Arc::clone(lapse), (want..self.count).collect(), Arc::clone(&stop));
            self.ahead = Some(Ahead { from: want, next: want, frames, waiting: None, _stops: Stops(stop) });
            self.began = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        let Some(ahead) = self.ahead.as_mut() else { return };
        while let Some((i, frame)) = ahead.waiting.take().or_else(|| ahead.frames.try_recv().ok()) {
            ahead.next = i + 1;
            if i < want {
                continue;
            }
            if i > want && self.scrub.is_some() {
                ahead.waiting = Some((i, frame));
                break;
            }
            // one left out (unreadable): the next shown in its place
            self.index = i;
            let image =
                egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba);
            self.shown = Some((i, ctx.load_texture("time-lapse", image, egui::TextureOptions::LINEAR)));
            break;
        }
    }

    /// True when Done was tapped (or Escape).
    fn show(&mut self, ctx: &egui::Context, look: &mut Look, lapse: Option<&Arc<TimeLapse>>) -> bool {
        let screen = ctx.content_rect();
        // play, frame by frame, until the end or a scrub: on once it's time and the frame shown
        // has come
        let step = Duration::from_millis(1000 / u64::from(TIME_LAPSE_FPS));
        let caught_up = self.shown.as_ref().is_some_and(|(i, _)| *i == self.index);
        if self.scrub.is_none() && self.index + 1 < self.count && caught_up {
            if self.stepped.elapsed() >= step {
                self.index += 1;
                self.stepped = Instant::now();
            }
            ctx.request_repaint_after(step.saturating_sub(self.stepped.elapsed()));
        }
        if let Some(lapse) = lapse {
            self.fetch(ctx, lapse);
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
        crate::ui::escape(ctx) || tapped == Some(tool_header::Tapped::Done)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A real package's time-lapse (its path in PROCRAFT_PACKAGE) taken in as its videos, then
    /// frames read back from them in order.
    #[test]
    #[ignore = "needs a .procreate with a time-lapse, and ffmpeg"]
    fn a_packages_videos_come_in_as_they_are_and_play_back() {
        let Ok(package) = std::env::var("PROCRAFT_PACKAGE") else { return };
        let dir = std::env::temp_dir().join(format!("procraft-take-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let artwork = dir.to_string_lossy().into_owned();
        let started = Instant::now();
        take_package(std::path::Path::new(&package), &artwork);
        let lapse = TimeLapse::new(artwork);
        let clips = lapse.clips();
        eprintln!(
            "{} frames in {} videos, {} MB, in {:?}",
            lapse.count(),
            clips.len(),
            lapse.bytes() >> 20,
            started.elapsed()
        );
        assert!(!clips.is_empty());
        assert_eq!(lapse.count(), clips.last().map_or(0, |c| c.first + c.frames));
        // across the first video's end, and one far on
        let first = clips[0].frames;
        let wanted = vec![0, first - 1, first, first + 1, lapse.count() - 1];
        let stop = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        let got: Vec<(u32, u32, u32)> = read_in_order(Arc::clone(&lapse), wanted.clone(), stop)
            .iter()
            .map(|(i, f)| (i, f.width, f.height))
            .collect();
        eprintln!("{got:?} in {:?}", started.elapsed());
        assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), wanted);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
