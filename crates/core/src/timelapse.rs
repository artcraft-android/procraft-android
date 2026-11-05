//! An artwork's time-lapse: a picture of the whole canvas after each change, kept as numbered
//! JPEG frames in the artwork's folder (`timelapse/frame-000000.jpg` on), replayed or made into a
//! video at [`TIME_LAPSE_FPS`] frames a second. The video itself is made by the app, with the
//! system's encoder; this keeps the frames, the settings and whether it records.
//!
//! As the artwork file keeps a time-lapse, the frames can instead be in videos (`segment-1.mp4` on, the
//! frame each ends before in `segments.json`), the frames after them still pictures: a package's
//! videos come in as they are ([`TimeLapse::take_clips`]), and the frames a video made of them
//! holds can go ([`TimeLapse::drop_clip_frames`]). The app reads those frames from the videos
//! ([`TimeLapse::clips`]).

use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::layers::ThumbnailImage;
use crate::project::{PackageTimeLapse, file_error, write_atomically};

/// The replay's and the video's frames a second.
pub const TIME_LAPSE_FPS: u32 = 30;
/// A frame's longest side kept, pixels.
const FRAME_SIDE: u32 = 1280;
const JPEG_QUALITY: u8 = 85;
/// A package's videos are made again as one rather than be more than this (while every frame is
/// still a picture to make them of).
const MOST_SEGMENTS: usize = 32;

/// A time-lapse's video: 1080p, 2K or 4K (`resolution` 0 to 2), Low, Good, Studio quality or
/// Lossless (`quality` 0 to 3), and HEVC or H.264.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimeLapseSettings {
    pub resolution: u32,
    pub quality: u32,
    pub hevc: bool,
}

impl Default for TimeLapseSettings {
    fn default() -> TimeLapseSettings {
        TimeLapseSettings { resolution: 0, quality: 1, hevc: false }
    }
}

/// The replay's and the video's frames a second.
#[uniffi::export]
pub fn time_lapse_fps() -> u32 {
    TIME_LAPSE_FPS
}

/// The longest side of a video made with `s`, pixels: 1920, 2732 or 3840.
#[uniffi::export]
pub fn time_lapse_side(s: TimeLapseSettings) -> u32 {
    match s.resolution {
        1 => 2732,
        2 => 3840,
        _ => 1920,
    }
}

/// The bits a pixel a second a video made with `s` takes: 2, 4, 8 or 16.
#[uniffi::export]
pub fn time_lapse_bits_per_pixel(s: TimeLapseSettings) -> u32 {
    match s.quality {
        0 => 2,
        2 => 8,
        3 => 16,
        _ => 4,
    }
}

/// How Canvas information names a time-lapse's settings: its quality, its resolution and its
/// codec (Good, 1080p, H.264).
#[uniffi::export]
pub fn time_lapse_setting_names(s: TimeLapseSettings) -> Vec<String> {
    let quality = ["Low", "Good", "Studio", "Lossless"].get(s.quality as usize).copied().unwrap_or("Good");
    let resolution = ["1080p", "2K", "4K"].get(s.resolution as usize).copied().unwrap_or("1080p");
    let codec = if s.hevc { "HEVC" } else { "H.264" };
    vec![quality.to_owned(), resolution.to_owned(), codec.to_owned()]
}

/// The frames a video `seconds` long takes out of `frames`, spread evenly; all of them when it
/// is none, or they wouldn't fill it.
#[uniffi::export]
pub fn time_lapse_picks(frames: u32, seconds: Option<u32>) -> Vec<u32> {
    let wanted = seconds.map(|s| s.saturating_mul(TIME_LAPSE_FPS));
    match wanted {
        Some(n) if frames > n && n > 0 => {
            let last = u64::from(frames - 1);
            let gaps = u64::from(n - 1).max(1);
            (0..u64::from(n)).filter_map(|i| u32::try_from(i * last / gaps).ok()).collect()
        }
        _ => (0..frames).collect(),
    }
}

#[derive(uniffi::Object)]
pub struct TimeLapse {
    dir: PathBuf,
    state: Mutex<State>,
}

struct State {
    /// Frames kept so far, and the last picture's hash (a picture the same as it isn't kept).
    count: u32,
    last: Option<u64>,
    /// A package's time-lapse being taken in.
    taking: Option<Taking>,
}

/// Taking a package's time-lapse in: its videos are kept (to go back into a package as they are)
/// while every frame of them has come in and there were no frames before; the frames each ends
/// before, the frames before the video being read, and the videos' size and bit rate.
struct Taking {
    whole: bool,
    ends: Vec<u32>,
    before: u32,
    width: u32,
    height: u32,
    bitrate: u64,
}

#[uniffi::export]
impl TimeLapse {
    /// The time-lapse of the artwork in folder `artwork`, its frames counted.
    #[uniffi::constructor]
    pub fn new(artwork: String) -> Arc<TimeLapse> {
        let dir = Path::new(&artwork).join("timelapse");
        let lapse = TimeLapse { dir, state: Mutex::new(State { count: 0, last: None, taking: None }) };
        // the frames in its videos, then the pictures after them
        let end = lapse.clips_on_disk().and_then(|k| k.ends.last().copied()).unwrap_or(0);
        let after = frame_files(&lapse.dir).iter().filter(|f| frame_index(f).is_some_and(|i| i >= end)).count();
        lapse.state.lock().count = end.saturating_add(u32::try_from(after).unwrap_or(u32::MAX));
        Arc::new(lapse)
    }

    /// The video's size, quality and codec, kept with the artwork.
    pub fn settings(&self) -> TimeLapseSettings {
        let text = fs::read_to_string(self.dir.join("settings.json")).unwrap_or_default();
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return TimeLapseSettings::default() };
        let whole = |key: &str, or: u32| {
            json.get(key).and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok()).unwrap_or(or)
        };
        TimeLapseSettings {
            resolution: whole("resolution", 0),
            quality: whole("quality", 1),
            hevc: json.get("hevc").and_then(|v| v.as_bool()).unwrap_or(false),
        }
    }

    pub fn set_settings(&self, s: TimeLapseSettings) {
        let json = serde_json::json!({ "resolution": s.resolution, "quality": s.quality, "hevc": s.hevc });
        let written = fs::create_dir_all(&self.dir).map_err(|e| e.to_string()).and_then(|()| {
            write_atomically(&self.dir.join("settings.json"), |p| {
                fs::write(p, json.to_string()).map_err(|e| file_error("saving", e))
            })
            .map_err(|e| e.to_string())
        });
        if let Err(e) = written {
            log::warn!("keeping the time-lapse's settings: {e}");
        }
    }

    /// Whether changes are recorded (on unless turned off for this artwork).
    pub fn recording(&self) -> bool {
        !self.dir.join("off").exists()
    }

    pub fn set_recording(&self, on: bool) {
        let off = self.dir.join("off");
        let done = if on {
            match fs::remove_file(&off) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        } else {
            fs::create_dir_all(&self.dir).and_then(|()| fs::write(&off, b""))
        };
        if let Err(e) = done {
            log::warn!("switching the time-lapse: {e}");
        }
    }

    /// Frames recorded so far.
    pub fn count(&self) -> u32 {
        self.state.lock().count
    }

    /// Deletes what has been recorded (its pictures and videos), as asked when recording is
    /// turned off; the settings, and whether it records, stay.
    pub fn purge(&self) {
        let mut st = self.state.lock();
        for f in frame_files(&self.dir) {
            let _ = fs::remove_file(f);
        }
        let _ = fs::remove_file(self.dir.join("segments.json"));
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if (name.starts_with("segment-") || name.starts_with(SET_ASIDE)) && name.ends_with(".mp4") {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        st.count = 0;
        st.last = None;
        st.taking = None;
    }

    /// What the recorded frames take on disk (pictures and videos), bytes.
    pub fn bytes(&self) -> u64 {
        let videos = self.clips().into_iter().map(|c| PathBuf::from(c.path));
        frame_files(&self.dir).into_iter().chain(videos).filter_map(|f| fs::metadata(f).ok()).map(|m| m.len()).sum()
    }

    /// Keeps a picture of the canvas (premultiplied RGBA, `width` × `height`, as
    /// [`crate::CanvasSession::time_lapse_frame`] makes it) as the next frame, on white (as the
    /// canvas shows a hidden background) and at most 1280 on its long side; unless it isn't
    /// recording, or the picture is the same as the last. True when kept.
    pub fn record(&self, rgba: Vec<u8>, width: u32, height: u32) -> bool {
        let len = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        if !self.recording() || width == 0 || height == 0 || rgba.len() < len {
            return false;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        rgba.hash(&mut hasher);
        let hash = hasher.finish();
        let mut st = self.state.lock();
        if st.last == Some(hash) {
            return false;
        }
        st.last = Some(hash);
        // premultiplied over white
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| {
                let under = 255 - u16::from(a);
                [r, g, b].map(|c| (u16::from(c) + under).min(255) as u8)
            })
            .collect();
        let Some(picture) = image::RgbImage::from_raw(width, height, rgb) else { return false };
        self.keep(&mut st, picture)
    }

    /// Takes a package's time-lapse `t` in: its settings and whether it records; then the app
    /// gives its videos' frames ([`Self::keep_frame`], [`Self::take_video`]) and ends with
    /// [`Self::take_end`].
    pub fn take_begin(&self, t: PackageTimeLapse) {
        self.set_settings(TimeLapseSettings {
            resolution: u32::from(t.resolution),
            quality: u32::from(t.quality),
            hevc: t.hevc,
        });
        self.set_recording(t.recording);
        self.drop_segments();
        let mut st = self.state.lock();
        st.taking = Some(Taking {
            whole: st.count == 0 && t.width > 0 && t.height > 0,
            ends: Vec::new(),
            before: st.count,
            width: t.width,
            height: t.height,
            bitrate: t.bitrate,
        });
    }

    /// A frame of a package's video (straight RGBA, opaque) kept as the next frame. True when
    /// kept.
    pub fn keep_frame(&self, rgba: Vec<u8>, width: u32, height: u32) -> bool {
        let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
        let Some(picture) = image::RgbImage::from_raw(width, height, rgb) else { return false };
        let mut st = self.state.lock();
        self.keep(&mut st, picture)
    }

    /// The package's video at `path` was read, `frames` frames in it: it is kept as it is while
    /// every frame of every video so far came in.
    pub fn take_video(&self, path: String, frames: u32) {
        let mut st = self.state.lock();
        let count = st.count;
        let Some(t) = st.taking.as_mut() else { return };
        t.whole = t.whole && frames > 0 && count - t.before == frames;
        if t.whole {
            let to = self.segment_path(t.ends.len());
            t.whole = fs::create_dir_all(&self.dir).and_then(|()| fs::copy(&path, &to)).is_ok();
        }
        t.ends.push(count);
        t.before = count;
    }

    /// The package's time-lapse is in: its videos noted, if they were all kept.
    pub fn take_end(&self) {
        let Some(t) = self.state.lock().taking.take() else { return };
        if t.whole && !t.ends.is_empty() && t.ends.len() < MOST_SEGMENTS {
            let s = self.settings();
            // as its videos are, where their header says
            let made = mp4_frames(Path::new(&self.segment_path(0)));
            self.write_segments(&Segments {
                resolution: s.resolution,
                quality: s.quality,
                hevc: s.hevc,
                width: t.width,
                height: t.height,
                bitrate: t.bitrate,
                ends: t.ends,
                clip_size: made.map(|m| (m.width, m.height)),
                clip_hevc: made.map(|m| m.hevc),
            });
        } else {
            self.drop_segments();
        }
    }
}

impl TimeLapse {
    /// Keeps `picture` as the next frame, at most 1280 on its long side, as JPEG.
    fn keep(&self, st: &mut State, picture: image::RgbImage) -> bool {
        let (width, height) = picture.dimensions();
        let k = (FRAME_SIDE as f32 / width.max(height) as f32).min(1.0);
        let picture = if k < 1.0 {
            let (w, h) = (((width as f32 * k).round() as u32).max(1), ((height as f32 * k).round() as u32).max(1));
            image::imageops::resize(&picture, w, h, image::imageops::FilterType::Triangle)
        } else {
            picture
        };
        let path = self.frame_path(st.count);
        let written = fs::create_dir_all(&self.dir).map_err(|e| e.to_string()).and_then(|()| {
            let file = fs::File::create(&path).map_err(|e| e.to_string())?;
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(BufWriter::new(file), JPEG_QUALITY);
            picture.write_with_encoder(encoder).map_err(|e| e.to_string())
        });
        match written {
            Ok(()) => {
                st.count += 1;
                true
            }
            Err(e) => {
                log::warn!("keeping a time-lapse frame: {e}");
                false
            }
        }
    }
}

#[uniffi::export]
impl TimeLapse {
    /// Frame `i`, straight RGBA (opaque); none if there is no such frame.
    pub fn frame(&self, i: u32) -> Option<ThumbnailImage> {
        let picture = image::open(self.frame_path(i)).ok()?.to_rgba8();
        Some(ThumbnailImage { width: picture.width(), height: picture.height(), rgba: picture.into_raw() })
    }

    /// Frame `i`'s file (a JPEG), for an app that reads it with its own decoder; none if there is
    /// no such frame.
    pub fn frame_file(&self, i: u32) -> Option<String> {
        let path = self.frame_path(i);
        path.exists().then(|| path.to_string_lossy().into_owned())
    }
}

/// A video a package needs made of the frames: into `path`, of frames `frames`, fitted on white
/// into `width` x `height` (0 x 0: into the first frame's size, within the settings' side);
/// `append` when it adds to the videos made before (it must then come out that size, and as HEVC
/// or H.264 as `hevc` says, as they are).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SegmentPlan {
    pub path: String,
    pub frames: Vec<u32>,
    pub width: u32,
    pub height: u32,
    pub append: bool,
    pub hevc: bool,
}

/// One of a time-lapse's videos: its file, the frame it starts at, and how many it holds.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimeLapseClip {
    pub path: String,
    pub first: u32,
    pub frames: u32,
}

/// The videos made of the frames for packages: the settings they were made with, their size and
/// bit rate, and the frame each ends before (`ends`, rising), their files numbered from 1.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Segments {
    resolution: u32,
    quality: u32,
    hevc: bool,
    width: u32,
    height: u32,
    bitrate: u64,
    ends: Vec<u32>,
    /// The videos' own size and codec where they aren't what the package notes (a package's may
    /// say HEVC at one size of videos made as H.264 at another): what one added to them is made
    /// as.
    #[serde(default)]
    clip_size: Option<(u32, u32)>,
    #[serde(default)]
    clip_hevc: Option<bool>,
}

impl Segments {
    /// The size and codec a video added to these is made as.
    fn made_as(&self) -> (u32, u32, bool) {
        let (w, h) = self.clip_size.unwrap_or((self.width, self.height));
        (w, h, self.clip_hevc.unwrap_or(self.hevc))
    }
}

/// What an MP4's header says of its video: its frames, picture size and whether it is HEVC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mp4 {
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub hevc: bool,
}

/// What a video is named after, with no note saying what it holds, so kept rather than lost.
const SET_ASIDE: &str = "set-aside-";

#[uniffi::export]
impl TimeLapse {
    /// The video a package still needs made so every frame is in one (the ones made before kept,
    /// so a share makes only what is new); none when they all are. The ones made before go when
    /// the settings changed, they are broken, or there would be too many, while their frames are
    /// still pictures to make them again of.
    pub fn segment_plan(&self) -> Option<SegmentPlan> {
        let frames = self.count();
        let kept = self.kept_segments();
        if kept.is_none() {
            self.drop_segments();
        }
        let done = kept.as_ref().and_then(|k| k.ends.last().copied()).unwrap_or(0);
        if done >= frames {
            return None;
        }
        Some(match kept {
            Some(k) => {
                let (width, height, hevc) = k.made_as();
                SegmentPlan {
                    path: self.segment_path(k.ends.len()),
                    frames: (done..frames).collect(),
                    width,
                    height,
                    append: true,
                    hevc,
                }
            }
            None => SegmentPlan {
                path: self.segment_path(0),
                frames: (0..frames).collect(),
                width: 0,
                height: 0,
                append: false,
                hevc: self.settings().hevc,
            },
        })
    }

    /// `plan`'s video was made `width` x `height` (or `made` false: it couldn't be). A video added
    /// to the ones before that failed means all are made again as one: that plan; unless the
    /// ones before hold the only copy of their frames (they stay, and the frames since stay out
    /// of the package this time).
    pub fn segment_made(&self, plan: SegmentPlan, width: u32, height: u32, made: bool) -> Option<SegmentPlan> {
        let end = plan.frames.last().map_or(0, |f| f + 1);
        if made {
            let segments = match (plan.append, self.kept_segments()) {
                (true, Some(mut k)) if k.made_as().0 == width && k.made_as().1 == height => {
                    k.ends.push(end);
                    k
                }
                _ => {
                    let s = self.settings();
                    let bits = time_lapse_bits_per_pixel(s);
                    Segments {
                        resolution: s.resolution,
                        quality: s.quality,
                        hevc: s.hevc,
                        width,
                        height,
                        bitrate: (u64::from(width) * u64::from(height) * u64::from(bits)).max(1_000_000),
                        ends: vec![end],
                        clip_size: None,
                        clip_hevc: None,
                    }
                }
            };
            self.write_segments(&segments);
            return None;
        }
        if self.clips_on_disk().is_some_and(|k| !self.remakeable(&k)) {
            return None;
        }
        self.drop_segments();
        plan.append.then(|| SegmentPlan {
            path: self.segment_path(0),
            frames: (0..self.count()).collect(),
            width: 0,
            height: 0,
            append: false,
            hevc: self.settings().hevc,
        })
    }

    /// The videos the frames are in, in order (the frames after the last are pictures).
    pub fn clips(&self) -> Vec<TimeLapseClip> {
        let Some(k) = self.clips_on_disk() else { return Vec::new() };
        let mut first = 0;
        let mut clips = Vec::with_capacity(k.ends.len());
        for (i, &end) in k.ends.iter().enumerate() {
            clips.push(TimeLapseClip { path: self.segment_path(i), first, frames: end - first });
            first = end;
        }
        clips
    }

    /// Takes a package's time-lapse `t` in as the package keeps it: its videos as they are (moved
    /// here), their frames counted from their headers, never decoded. False, nothing changed,
    /// when there are frames here already, it has no videos, or a video's frames can't be
    /// counted: then it is taken in frame by frame ([`Self::take_begin`]).
    pub fn take_clips(&self, t: PackageTimeLapse) -> bool {
        if self.count() > 0 || t.segments.is_empty() {
            return false;
        }
        let mut counted = Vec::with_capacity(t.segments.len());
        for s in &t.segments {
            match mp4_frames(Path::new(s)) {
                Some(c) if c.frames > 0 => counted.push(c),
                _ => return false,
            }
        }
        if fs::create_dir_all(&self.dir).is_err() {
            return false;
        }
        let mut ends = Vec::with_capacity(counted.len());
        let mut end = 0u32;
        for (i, (s, Mp4 { frames, .. })) in t.segments.iter().zip(&counted).enumerate() {
            let to = self.segment_path(i);
            if fs::rename(s, &to).or_else(|_| fs::copy(s, &to).map(|_| ())).is_err() {
                for j in 0..i {
                    let _ = fs::remove_file(self.segment_path(j));
                }
                return false;
            }
            end = end.saturating_add(*frames);
            ends.push(end);
        }
        // as the package notes it, and as its videos are
        let made = counted.first().copied();
        let (width, height) = match made {
            _ if t.width > 0 && t.height > 0 => (t.width, t.height),
            Some(m) => (m.width, m.height),
            None => (0, 0),
        };
        self.set_settings(TimeLapseSettings {
            resolution: u32::from(t.resolution),
            quality: u32::from(t.quality),
            hevc: t.hevc,
        });
        self.set_recording(t.recording);
        self.write_segments(&Segments {
            resolution: u32::from(t.resolution),
            quality: u32::from(t.quality),
            hevc: t.hevc,
            width,
            height,
            bitrate: t.bitrate,
            ends,
            clip_size: made.map(|m| (m.width, m.height)),
            clip_hevc: made.map(|m| m.hevc),
        });
        self.state.lock().count = end;
        true
    }

    /// The videos whose frames are still pictures too: what an app reads back before those
    /// pictures go ([`Self::drop_clip_frames`]), a header not saying that a device can read it.
    pub fn clips_with_pictures(&self) -> Vec<TimeLapseClip> {
        self.clips().into_iter().filter(|c| self.frame_path(c.first).exists()).collect()
    }

    /// The frames kept as pictures that its videos hold too, gone (as the artwork file keeps a
    /// time-lapse: only as video); only once every video's frames, counted from its header, are
    /// as many as it is noted to hold. How many went.
    pub fn drop_clip_frames(&self) -> u32 {
        let clips = self.clips();
        let holds = clips.iter().all(|c| mp4_frames(Path::new(&c.path)).is_some_and(|m| m.frames == c.frames));
        let Some(end) = clips.last().map(|c| c.first + c.frames).filter(|_| holds) else { return 0 };
        let mut gone = 0;
        for f in frame_files(&self.dir) {
            if frame_index(&f).is_some_and(|i| i < end) && fs::remove_file(&f).is_ok() {
                gone += 1;
            }
        }
        gone
    }

    /// The time-lapse as a package keeps it: its settings, and its videos once made.
    pub fn for_package(&self) -> PackageTimeLapse {
        let s = self.settings();
        let kept = self.kept_segments();
        PackageTimeLapse {
            recording: self.recording(),
            quality: u8::try_from(s.quality).unwrap_or(1),
            resolution: u8::try_from(s.resolution).unwrap_or(0),
            hevc: s.hevc,
            segments: kept.as_ref().map_or(Vec::new(), |k| (0..k.ends.len()).map(|i| self.segment_path(i)).collect()),
            width: kept.as_ref().map_or(0, |k| k.width),
            height: kept.as_ref().map_or(0, |k| k.height),
            fps: TIME_LAPSE_FPS,
            bitrate: kept.as_ref().map_or(0, |k| k.bitrate),
        }
    }
}

impl TimeLapse {
    fn frame_path(&self, i: u32) -> PathBuf {
        self.dir.join(format!("frame-{i:06}.jpg"))
    }

    fn segment_path(&self, i: usize) -> String {
        self.dir.join(format!("segment-{}.mp4", i + 1)).to_string_lossy().into_owned()
    }

    /// The videos made before, when they still hold: as [`Self::clips_on_disk`], and made with
    /// the settings now and not too many; or, whatever the settings, holding the only copy of
    /// their frames.
    fn kept_segments(&self) -> Option<Segments> {
        let k = self.clips_on_disk()?;
        let s = self.settings();
        let fresh =
            (k.resolution, k.quality, k.hevc) == (s.resolution, s.quality, s.hevc) && k.ends.len() < MOST_SEGMENTS;
        (fresh || !self.remakeable(&k)).then_some(k)
    }

    /// The videos noted, when the note holds: a size, their ends rising, and every file there.
    fn clips_on_disk(&self) -> Option<Segments> {
        let text = fs::read_to_string(self.dir.join("segments.json")).ok()?;
        let k: Segments = serde_json::from_str(&text).ok()?;
        let holds = k.width > 0
            && k.height > 0
            && k.ends.first().is_none_or(|e| *e > 0)
            && k.ends.windows(2).all(|w| w.first() < w.get(1))
            && (0..k.ends.len()).all(|i| Path::new(&self.segment_path(i)).exists());
        holds.then_some(k)
    }

    /// Every frame in videos `k` is still a picture too, to make them again of.
    fn remakeable(&self, k: &Segments) -> bool {
        let end = k.ends.last().copied().unwrap_or(0);
        let pictures = frame_files(&self.dir).iter().filter(|f| frame_index(f).is_some_and(|i| i < end)).count();
        pictures == end as usize
    }

    /// Notes the videos made (an imported package's are its time-lapse's only copy), in place of
    /// the note before only once written whole.
    fn write_segments(&self, k: &Segments) {
        let written = serde_json::to_string(k).map_err(|e| e.to_string()).and_then(|text| {
            write_atomically(&self.dir.join("segments.json"), |p| {
                fs::write(p, text).map_err(|e| file_error("saving", e))
            })
            .map_err(|e| e.to_string())
        });
        if let Err(e) = written {
            log::warn!("keeping the time-lapse's videos: {e}");
        }
    }

    /// Every package video, and the note of them, gone; unless they hold the only copy of their
    /// frames. With no note that holds there is no telling what they hold, so they go aside
    /// instead, out of the way of what is made next.
    fn drop_segments(&self) {
        let note = self.clips_on_disk();
        if note.as_ref().is_some_and(|k| !self.remakeable(k)) {
            return;
        }
        let _ = fs::remove_file(self.dir.join("segments.json"));
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
            if name.starts_with("segment-") && name.ends_with(".mp4") {
                if note.is_some() {
                    let _ = fs::remove_file(&path);
                } else {
                    let free = (0..).map(|i| self.dir.join(format!("{SET_ASIDE}{i}-{name}"))).find(|p| !p.exists());
                    if let Some(aside) = free {
                        let _ = fs::rename(&path, aside);
                    }
                }
            }
        }
    }
}

/// The frame a frame file is (`frame-000123.jpg`: 123).
fn frame_index(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    name.strip_prefix("frame-")?.strip_suffix(".jpg")?.parse().ok()
}

/// The frames an MP4 video holds, its picture's size and codec, from its header (its video
/// track's sample sizes, sample description and track header), nothing decoded; none if it isn't
/// one that can be read so.
pub(crate) fn mp4_frames(path: &Path) -> Option<Mp4> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let moov = mp4_box(&mut file, 0, len, b"moov")?;
    // the video track: the one whose handler is 'vide'
    let mut at = moov.0;
    while let Some((start, end, kind)) = mp4_next(&mut file, at, moov.1) {
        at = end;
        if &kind != b"trak" {
            continue;
        }
        let mdia = mp4_box(&mut file, start, end, b"mdia")?;
        let hdlr = mp4_box(&mut file, mdia.0, mdia.1, b"hdlr")?;
        let mut handler = [0u8; 4];
        file.seek(SeekFrom::Start(hdlr.0 + 8)).ok()?;
        file.read_exact(&mut handler).ok()?;
        if &handler != b"vide" {
            continue;
        }
        let minf = mp4_box(&mut file, mdia.0, mdia.1, b"minf")?;
        let stbl = mp4_box(&mut file, minf.0, minf.1, b"stbl")?;
        let stsz = mp4_box(&mut file, stbl.0, stbl.1, b"stsz")?;
        let frames = mp4_u32(&mut file, stsz.0 + 8)?;
        // the first sample description's name: hvc1 or hev1 for HEVC
        let stsd = mp4_box(&mut file, stbl.0, stbl.1, b"stsd")?;
        let mut entry = [0u8; 4];
        file.seek(SeekFrom::Start(stsd.0 + 12)).ok()?;
        file.read_exact(&mut entry).ok()?;
        let hevc = &entry == b"hvc1" || &entry == b"hev1";
        // the track header's size, 16.16 fixed point, at its end
        let tkhd = mp4_box(&mut file, start, end, b"tkhd")?;
        let mut version = [0u8; 1];
        file.seek(SeekFrom::Start(tkhd.0)).ok()?;
        file.read_exact(&mut version).ok()?;
        let size_at = tkhd.0 + if version[0] == 1 { 88 } else { 76 };
        let (w, h) = (mp4_u32(&mut file, size_at)? >> 16, mp4_u32(&mut file, size_at + 4)? >> 16);
        return Some(Mp4 { frames, width: w, height: h, hevc });
    }
    None
}

/// The first box named `kind` among those from `from` to `to`: where its contents start and it
/// ends.
fn mp4_box(file: &mut fs::File, from: u64, to: u64, kind: &[u8; 4]) -> Option<(u64, u64)> {
    let mut at = from;
    while let Some((start, end, k)) = mp4_next(file, at, to) {
        if &k == kind {
            return Some((start, end));
        }
        at = end;
    }
    None
}

/// The box at `at` (before `to`): where its contents start, where it ends, and its name.
fn mp4_next(file: &mut fs::File, at: u64, to: u64) -> Option<(u64, u64, [u8; 4])> {
    if at + 8 > to {
        return None;
    }
    let mut head = [0u8; 8];
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(&mut head).ok()?;
    let (size, kind) = head.split_at(4);
    let size = u64::from(u32::from_be_bytes(size.try_into().ok()?));
    let kind: [u8; 4] = kind.try_into().ok()?;
    let (start, end) = match size {
        // a 64-bit size follows
        1 => {
            let mut large = [0u8; 8];
            file.read_exact(&mut large).ok()?;
            (at + 16, at.checked_add(u64::from_be_bytes(large))?)
        }
        // to the end
        0 => (at + 8, to),
        _ => (at + 8, at + size),
    };
    (end <= to && start <= end).then_some((start, end, kind))
}

fn mp4_u32(file: &mut fs::File, at: u64) -> Option<u32> {
    let mut bytes = [0u8; 4];
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(&mut bytes).ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// The frame files in `dir`.
fn frame_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("frame-")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_spread_a_long_time_lapse_over_the_seconds() {
        assert_eq!(time_lapse_picks(5, None), [0, 1, 2, 3, 4]);
        // fewer frames than the seconds take: all of them
        assert_eq!(time_lapse_picks(5, Some(1)), [0, 1, 2, 3, 4]);
        let picks = time_lapse_picks(301, Some(1));
        assert_eq!(picks.len(), 30);
        assert_eq!((picks[0], picks[29]), (0, 300));
        assert!(picks.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn videos_with_no_note_of_them_are_set_aside_not_deleted() {
        let artwork = std::env::temp_dir().join(format!("procraft-unnoted-{}", std::process::id()));
        let _ = fs::remove_dir_all(&artwork);
        let t = TimeLapse::new(artwork.to_string_lossy().into_owned());
        assert!(t.record([255u8, 0, 0, 255].repeat(16), 4, 4));
        let dir = artwork.join("timelapse");
        // a video the note of which was lost: it may be all there is of its frames
        fs::write(dir.join("segment-1.mp4"), b"the only copy").unwrap();
        fs::write(dir.join("segments.json"), b"{ not whole").unwrap();
        let plan = t.segment_plan().expect("a plan");
        assert!(!plan.append);
        assert!(!dir.join("segment-1.mp4").exists(), "out of the way of what is made next");
        assert_eq!(fs::read(dir.join("set-aside-0-segment-1.mp4")).unwrap(), b"the only copy");
        // asked to delete what was recorded, it goes too
        t.purge();
        assert!(!dir.join("set-aside-0-segment-1.mp4").exists());
        let _ = fs::remove_dir_all(&artwork);
    }

    #[test]
    fn frames_are_kept_once_on_white_and_counted_again() {
        let dir = std::env::temp_dir().join(format!("procraft-timelapse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        assert_eq!(t.count(), 0);
        // a clear canvas comes out white; the same picture again isn't kept
        let clear = vec![0u8; 4 * 4 * 4];
        assert!(t.record(clear.clone(), 4, 4));
        assert!(!t.record(clear, 4, 4));
        let red = [255u8, 0, 0, 255].repeat(16);
        assert!(t.record(red, 4, 4));
        assert_eq!(t.count(), 2);
        let first = t.frame(0).unwrap();
        assert_eq!((first.width, first.height), (4, 4));
        assert!(first.rgba[..3].iter().all(|&c| c > 240), "{:?}", &first.rgba[..4]);
        let second = t.frame(1).unwrap();
        assert!(second.rgba[0] > 200 && second.rgba[1] < 60);
        assert!(t.frame(2).is_none());
        assert!(t.frame_file(1).is_some_and(|f| f.ends_with("frame-000001.jpg")));
        assert!(t.frame_file(2).is_none());
        // not recording: nothing kept; counted again from the files
        t.set_recording(false);
        assert!(!t.record([0u8, 0, 255, 255].repeat(16), 4, 4));
        assert_eq!(TimeLapse::new(dir.to_string_lossy().into_owned()).count(), 2);
        t.set_recording(true);
        assert!(t.recording());
        let s = TimeLapseSettings { resolution: 2, quality: 3, hevc: true };
        t.set_settings(s);
        assert_eq!(t.settings(), s);
        assert_eq!((time_lapse_side(s), time_lapse_bits_per_pixel(s)), (3840, 16));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn canvas_information_names_the_settings_as_the_canvas_sheet_picks_them() {
        let names =
            |resolution, quality, hevc| time_lapse_setting_names(TimeLapseSettings { resolution, quality, hevc });
        assert_eq!(names(0, 1, false), ["Good", "1080p", "H.264"]);
        assert_eq!(names(2, 3, true), ["Lossless", "4K", "HEVC"]);
        assert_eq!(names(1, 0, false), ["Low", "2K", "H.264"]);
        assert_eq!(names(1, 2, false)[0], "Studio");
    }

    #[test]
    fn purging_deletes_what_was_recorded_and_keeps_the_settings_and_the_switch() {
        let dir = std::env::temp_dir().join(format!("procraft-purge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        let s = TimeLapseSettings { resolution: 1, quality: 2, hevc: false };
        t.set_settings(s);
        assert!(t.record(vec![0u8; 4 * 4 * 4], 4, 4));
        assert!(t.record([255u8, 0, 0, 255].repeat(16), 4, 4));
        t.set_recording(false);
        t.purge();
        assert_eq!(t.count(), 0);
        assert!(t.frame(0).is_none() && t.clips().is_empty());
        assert_eq!(TimeLapse::new(dir.to_string_lossy().into_owned()).count(), 0);
        assert_eq!(t.settings(), s);
        assert!(!t.recording());
        // recording again starts afresh, the first picture kept even if it was the last before
        t.set_recording(true);
        assert!(t.record([255u8, 0, 0, 255].repeat(16), 4, 4));
        assert_eq!(t.count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_videos_are_made_once_and_added_to() {
        let dir = std::env::temp_dir().join(format!("procraft-segments-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        assert_eq!(t.segment_plan(), None, "no frames: nothing to make");
        for i in 0..3u8 {
            assert!(t.record([i, 0, 0, 255].repeat(4), 2, 2));
        }
        // the first share makes every frame into one video
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.clone(), plan.width, plan.append), (vec![0, 1, 2], 0, false));
        fs::write(&plan.path, b"video").unwrap();
        assert_eq!(t.segment_made(plan, 640, 480, true), None);
        assert_eq!(t.segment_plan(), None, "all made");
        let p = t.for_package();
        assert_eq!((p.segments.len(), p.width, p.height, p.fps, p.bitrate), (1, 640, 480, 30, 1_228_800));
        // frames since: one more video the same size
        assert!(t.record([9, 9, 9, 255].repeat(4), 2, 2));
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.clone(), plan.width, plan.height, plan.append), (vec![3], 640, 480, true));
        assert!(plan.path.ends_with("segment-2.mp4"));
        // it couldn't be made that size: all again as one
        let again = t.segment_made(plan, 0, 0, false).unwrap();
        assert_eq!((again.frames.len(), again.append), (4, false));
        assert!(t.for_package().segments.is_empty());
        // other settings: the videos made before go
        fs::write(&again.path, b"video").unwrap();
        t.segment_made(again, 640, 480, true);
        t.set_settings(TimeLapseSettings { hevc: true, ..t.settings() });
        assert!(!t.segment_plan().unwrap().append);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_time_lapse_comes_in_with_its_videos() {
        let dir = std::env::temp_dir().join(format!("procraft-take-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let video = dir.join("brought.mp4");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&video, b"video").unwrap();
        let t = TimeLapse::new(dir.join("artwork").to_string_lossy().into_owned());
        let package = PackageTimeLapse {
            recording: false,
            quality: 2,
            resolution: 1,
            hevc: true,
            segments: vec![video.to_string_lossy().into_owned()],
            width: 640,
            height: 480,
            fps: 30,
            bitrate: 5_000_000,
        };
        t.take_begin(package);
        assert!(!t.recording());
        assert_eq!(t.settings(), TimeLapseSettings { resolution: 1, quality: 2, hevc: true });
        // its video's two frames come in, so the video is kept to go back into a package
        assert!(t.keep_frame([10, 20, 30, 255].repeat(4), 2, 2));
        assert!(t.keep_frame([40, 50, 60, 255].repeat(4), 2, 2));
        t.take_video(video.to_string_lossy().into_owned(), 2);
        t.take_end();
        assert_eq!(t.count(), 2);
        assert_eq!(t.segment_plan(), None, "nothing to make again");
        let p = t.for_package();
        assert_eq!((p.segments.len(), p.width, p.height, p.bitrate), (1, 640, 480, 5_000_000));
        assert!(Path::new(&p.segments[0]).exists());
        // a video whose frames didn't all come in isn't kept
        let t2 = TimeLapse::new(dir.join("other").to_string_lossy().into_owned());
        t2.take_begin(PackageTimeLapse { segments: vec![], ..t.for_package() });
        assert!(t2.keep_frame([0, 0, 0, 255].repeat(4), 2, 2));
        t2.take_video(video.to_string_lossy().into_owned(), 3);
        t2.take_end();
        assert!(t2.for_package().segments.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// An MP4 with only the header boxes read: a video track of `frames` frames, `w` x `h`.
    fn fake_mp4(path: &Path, frames: u32, w: u32, h: u32) {
        fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut b = (u32::try_from(body.len() + 8).unwrap()).to_be_bytes().to_vec();
            b.extend_from_slice(kind);
            b.extend_from_slice(body);
            b
        }
        let mut tkhd = vec![0u8; 84];
        tkhd[76..80].copy_from_slice(&(w << 16).to_be_bytes());
        tkhd[80..84].copy_from_slice(&(h << 16).to_be_bytes());
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(b"vide");
        hdlr.extend_from_slice(&[0u8; 13]);
        let mut stsz = vec![0u8; 8];
        stsz.extend_from_slice(&frames.to_be_bytes());
        // one sample description, H.264
        let mut stsd = vec![0, 0, 0, 0, 0, 0, 0, 1];
        stsd.extend_from_slice(&boxed(b"avc1", &[0u8; 8]));
        let stbl = boxed(b"stbl", &[boxed(b"stsd", &stsd), boxed(b"stsz", &stsz)].concat());
        let minf = boxed(b"minf", &stbl);
        let mdia = boxed(b"mdia", &[boxed(b"hdlr", &hdlr), minf].concat());
        let trak = boxed(b"trak", &[boxed(b"tkhd", &tkhd), mdia].concat());
        let file = [boxed(b"ftyp", b"isom"), boxed(b"mdat", &[1, 2, 3]), boxed(b"moov", &trak)].concat();
        fs::write(path, file).unwrap();
    }

    #[test]
    fn an_mp4s_frames_and_size_come_from_its_header() {
        let dir = std::env::temp_dir().join(format!("procraft-mp4-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let video = dir.join("clip.mp4");
        fake_mp4(&video, 241, 672, 1102);
        assert_eq!(mp4_frames(&video), Some(Mp4 { frames: 241, width: 672, height: 1102, hevc: false }));
        fs::write(&video, b"not a video").unwrap();
        assert_eq!(mp4_frames(&video), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_time_lapse_comes_in_as_its_videos_and_grows_by_one_more() {
        let dir = std::env::temp_dir().join(format!("procraft-clips-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let brought: Vec<String> = [3u32, 2]
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let p = dir.join(format!("brought-{i}.mp4"));
                fake_mp4(&p, n, 672, 1102);
                p.to_string_lossy().into_owned()
            })
            .collect();
        let artwork = dir.join("artwork").to_string_lossy().into_owned();
        let t = TimeLapse::new(artwork.clone());
        let package = PackageTimeLapse {
            recording: true,
            quality: 1,
            resolution: 0,
            hevc: true,
            segments: brought,
            // what the package notes, not what its videos are
            width: 640,
            height: 1050,
            fps: 30,
            bitrate: 2_000_000,
        };
        assert!(t.take_clips(package));
        // its videos moved in, their frames counted, no pictures made
        assert_eq!(t.count(), 5);
        assert!(frame_files(&t.dir).is_empty());
        let clips = t.clips();
        assert_eq!(clips.iter().map(|c| (c.first, c.frames)).collect::<Vec<_>>(), [(0, 3), (3, 2)]);
        assert_eq!(TimeLapse::new(artwork.clone()).count(), 5, "counted again from the note");
        let p = t.for_package();
        assert_eq!((p.segments.len(), p.width, p.height, p.bitrate, p.hevc), (2, 640, 1050, 2_000_000, true));
        // other settings don't lose them: they hold the only copy
        t.set_settings(TimeLapseSettings { hevc: false, quality: 3, ..t.settings() });
        assert_eq!(t.segment_plan(), None);
        assert_eq!(t.clips().len(), 2);
        // drawn on: the frames after them become one more video, as those are
        assert!(t.record([9, 9, 9, 255].repeat(4), 2, 2));
        assert_eq!(t.count(), 6);
        assert!(t.frame_file(5).is_some_and(|f| f.ends_with("frame-000005.jpg")));
        let plan = t.segment_plan().unwrap();
        // made as its videos are (H.264, their size), not as the package notes
        assert_eq!(
            (plan.frames.clone(), plan.width, plan.height, plan.append, plan.hevc),
            (vec![5], 672, 1102, true, false)
        );
        // made a size the encoder won't: they stay, the new frame stays a picture
        assert_eq!(t.segment_made(plan.clone(), 0, 0, false), None);
        assert_eq!(t.clips().len(), 2);
        fake_mp4(Path::new(&plan.path), 1, 672, 1102);
        assert_eq!(t.segment_made(plan, 672, 1102, true), None);
        assert_eq!(t.clips().len(), 3);
        // the picture the new video holds goes
        assert_eq!(t.drop_clip_frames(), 1);
        assert!(frame_files(&t.dir).is_empty());
        assert_eq!(TimeLapse::new(artwork).count(), 6);
        // a video whose frames can't be counted: taken in frame by frame instead
        let t2 = TimeLapse::new(dir.join("other").to_string_lossy().into_owned());
        let bad = dir.join("bad.mp4");
        fs::write(&bad, b"video").unwrap();
        assert!(
            !t2.take_clips(PackageTimeLapse { segments: vec![bad.to_string_lossy().into_owned()], ..t.for_package() })
        );
        assert!(bad.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pictures_go_only_once_their_videos_hold_them() {
        let dir = std::env::temp_dir().join(format!("procraft-dropframes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        for i in 0..3u8 {
            assert!(t.record([i, 0, 0, 255].repeat(4), 2, 2));
        }
        let plan = t.segment_plan().unwrap();
        // the video holds fewer frames than it was made of: the pictures stay
        fake_mp4(Path::new(&plan.path), 2, 2, 2);
        t.segment_made(plan.clone(), 2, 2, true);
        assert_eq!(t.drop_clip_frames(), 0);
        assert_eq!(frame_files(&t.dir).len(), 3);
        fake_mp4(Path::new(&plan.path), 3, 2, 2);
        assert_eq!(t.drop_clip_frames(), 3);
        assert_eq!((t.count(), TimeLapse::new(dir.to_string_lossy().into_owned()).count()), (3, 3));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_videos_whose_pictures_are_still_there_are_read_back() {
        let dir = std::env::temp_dir().join(format!("procraft-readback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        for i in 0..3u8 {
            assert!(t.record([i, 0, 0, 255].repeat(4), 2, 2));
        }
        assert!(t.clips_with_pictures().is_empty());
        let plan = t.segment_plan().unwrap();
        fake_mp4(Path::new(&plan.path), 3, 2, 2);
        t.segment_made(plan.clone(), 2, 2, true);
        let first = TimeLapseClip { path: plan.path, first: 0, frames: 3 };
        assert_eq!(t.clips_with_pictures(), [first]);
        // an app that can't read it back leaves them: every frame is still there
        assert_eq!(frame_files(&t.dir).len(), 3);
        assert_eq!(t.drop_clip_frames(), 3);
        assert!(t.clips_with_pictures().is_empty());
        // the next video is the only one to check
        assert!(t.record([9, 0, 0, 255].repeat(4), 2, 2));
        let plan = t.segment_plan().unwrap();
        fake_mp4(Path::new(&plan.path), 1, 2, 2);
        t.segment_made(plan.clone(), 2, 2, true);
        assert_eq!(t.clips_with_pictures(), [TimeLapseClip { path: plan.path, first: 3, frames: 1 }]);
        let _ = fs::remove_dir_all(&dir);
    }
}
