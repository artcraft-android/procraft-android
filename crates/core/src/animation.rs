//! The layers as an animation: each visible top-level layer or group a frame, bottom first, as
//! an animated GIF or PNG, or a frame at a time for a video or a preview.

use std::fs;
use std::io::BufWriter;
use std::path::Path;

use procraft_engine::animation::{self, Animation, Playback};
use procraft_engine::document::{Document, LayerId, LayerKind};
use procraft_engine::formats::GifWriter;

use crate::layers::ThumbnailImage;
use crate::project::{file_error, thumbnail, write_atomically};
use crate::{CanvasSession, MobileError};

/// How much of an animated GIF's frames is drawn at once (a frame to a core), so a big canvas's
/// export doesn't hold many full-size frames.
const GIF_BATCH_BYTES: usize = 256 << 20;
/// How many pixels, from all the frames, a shared GIF palette is made from.
const GIF_PALETTE_SAMPLES: usize = 1 << 18;

/// The frames: the visible top-level layers and groups, bottom first.
fn frames(doc: &Document) -> Vec<LayerId> {
    animation::frames(doc)
}

/// The frames as they play: each as often as Animation Assist holds it.
fn sequence(doc: &Document) -> Vec<LayerId> {
    let holds = |id: LayerId| doc.animation.as_ref().map_or(0, |a| a.hold(id)) as usize;
    frames(doc).into_iter().flat_map(|id| std::iter::repeat_n(id, 1 + holds(id))).collect()
}

#[uniffi::export]
impl CanvasSession {
    /// The frames in playing order (indices into the frames), each as often as it is held.
    pub fn animation_sequence(&self) -> Vec<u32> {
        let doc = &self.shared.lock().doc;
        let all = frames(doc);
        sequence(doc)
            .iter()
            .filter_map(|id| all.iter().position(|f| f == id))
            .filter_map(|i| u32::try_from(i).ok())
            .collect()
    }

    /// Gives an artwork opened to be looked at, with no window, the app's device and its canvas,
    /// so its frames can be drawn (`animation_frame`). False if either can't be had. Slow: call it
    /// off the main thread.
    pub fn draw_without_window(&self) -> bool {
        crate::device::warm_gpu();
        let gpu = crate::device::kept().clone();
        let mut st = self.shared.lock();
        st.gpu = gpu;
        st.ensure_canvas();
        st.canvas.is_some()
    }

    /// How many frames the animation has.
    pub fn animation_frame_count(&self) -> u32 {
        u32::try_from(frames(&self.shared.lock().doc).len()).unwrap_or(u32::MAX)
    }

    /// Frame `index` (bottom first), straight alpha, scaled to fit `max_side` (0: full size),
    /// over the background unless `transparent`; none if there is no such frame.
    pub fn animation_frame(&self, index: u32, transparent: bool, max_side: u32) -> Option<ThumbnailImage> {
        let unit = frames(&self.shared.lock().doc).get(usize::try_from(index).ok()?).copied()?;
        match self.render_frame(unit, transparent, max_side) {
            Ok((width, height, rgba)) => Some(ThumbnailImage { width, height, rgba }),
            Err(e) => {
                log::warn!("animation frame: {e}");
                None
            }
        }
    }

    /// The animation as a looping GIF at `path`, `fps` frames a second, scaled to fit
    /// `max_side` (0: full size); `dither` spreads the palette's error, `per_frame_palette`
    /// gives each frame a palette of its own (else they share one made from them all),
    /// `transparent` leaves the background out.
    #[allow(clippy::too_many_arguments)]
    pub fn export_gif(
        &self,
        path: String,
        max_side: u32,
        fps: f32,
        dither: bool,
        per_frame_palette: bool,
        transparent: bool,
    ) -> Result<(), MobileError> {
        let units = sequence(&self.shared.lock().doc);
        let delay = (100.0 / fps.clamp(0.1, 100.0)).round() as u16;
        // the frames made take it to 0.95 (sharing a palette, its look at them to 0.45 first), the
        // file written to 1
        self.exported(0.0);
        let (mut gif, start): (Option<GifWriter>, f32) = if per_frame_palette {
            (None, 0.0)
        } else {
            (self.shared_palette_gif(&units, transparent, max_side)?, 0.45)
        };
        // drawn a few at a time (a frame to a core, as many as fit in GIF_BATCH_BYTES), each few
        // made into the GIF's frames side by side, then added in order
        let mut next = 0;
        let mut few = 1;
        while next < units.len() {
            let frames = units
                .iter()
                .skip(next)
                .take(few)
                .map(|&unit| self.render_frame(unit, transparent, max_side))
                .collect::<Result<Vec<_>, _>>()?;
            next += frames.len();
            let Some(&(w, h, _)) = frames.first() else { break };
            let frame_bytes = (w as usize * h as usize * 4).max(1);
            few = (GIF_BATCH_BYTES / frame_bytes).clamp(1, procraft_engine::par::cores());
            let writer = gif.get_or_insert_with(|| GifWriter::new(w, h));
            let made = {
                let writer = &*writer;
                procraft_engine::par::side_by_side(&frames, |(_, _, rgba)| {
                    writer.encoded_frame(rgba, delay.max(1), dither)
                })
            };
            for ((_, _, rgba), frame) in frames.iter().zip(made) {
                let frame = frame.unwrap_or_else(|| writer.encoded_frame(rgba, delay.max(1), dither));
                writer.add_encoded(&frame);
            }
            self.exported(start + (0.95 - start) * next as f32 / units.len() as f32);
        }
        let bytes = gif.ok_or_else(|| file_error("saving", "there are no frames"))?.finish();
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }

    /// The animation as a looping animated PNG at `path`, `fps` frames a second, scaled to fit
    /// `max_side` (0: full size), `transparent` leaving the background out.
    pub fn export_apng(&self, path: String, max_side: u32, fps: f32, transparent: bool) -> Result<(), MobileError> {
        let units = sequence(&self.shared.lock().doc);
        let count = u32::try_from(units.len()).unwrap_or(u32::MAX);
        let Some((&first, rest)) = units.split_first() else {
            return Err(file_error("saving", "there are no frames"));
        };
        // as the GIF's: the frames to 0.95, the file to 1
        self.exported(0.0);
        let made = |k: usize| self.exported(0.95 * k as f32 / count.max(1) as f32);
        let (w, h, rgba) = self.render_frame(first, transparent, max_side)?;
        // a frame lasts 1/fps seconds, in thousandths
        let delay = (1000.0 / fps.clamp(0.1, 1000.0)).round().clamp(1.0, 65535.0) as u16;
        write_atomically(Path::new(&path), |f| {
            let out = fs::File::create(f).map_err(|e| file_error("saving", e))?;
            let mut encoder = png::Encoder::new(BufWriter::new(out), w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::Fast);
            encoder.set_animated(count, 0).map_err(|e| file_error("saving", e))?;
            encoder.set_frame_delay(delay, 1000).map_err(|e| file_error("saving", e))?;
            let mut writer = encoder.write_header().map_err(|e| file_error("saving", e))?;
            writer.write_image_data(&rgba).map_err(|e| file_error("saving", e))?;
            made(1);
            for (k, &unit) in rest.iter().enumerate() {
                let (fw, fh, frame) = self.render_frame(unit, transparent, max_side)?;
                if (fw, fh) != (w, h) {
                    return Err(file_error("saving", "the frames differ in size"));
                }
                writer.write_image_data(&frame).map_err(|e| file_error("saving", e))?;
                made(k + 2);
            }
            writer.finish().map_err(|e| file_error("saving", e))
        })?;
        self.exported(1.0);
        Ok(())
    }
}

impl CanvasSession {
    /// A GIF writer whose frames share one palette, made from a sample of every frame's pixels
    /// (each drawn once for it); none when there are no frames.
    fn shared_palette_gif(
        &self,
        units: &[LayerId],
        transparent: bool,
        max_side: u32,
    ) -> Result<Option<GifWriter>, MobileError> {
        // a held frame is drawn once, however many places it holds
        let mut seen = std::collections::HashSet::new();
        let units: Vec<LayerId> = units.iter().copied().filter(|u| seen.insert(*u)).collect();
        // about as many sampled pixels in all as a single frame's palette looks at
        let per_frame = (GIF_PALETTE_SAMPLES / units.len().max(1)).max(256);
        let mut samples: Vec<[u8; 4]> = Vec::new();
        let mut size = None;
        for (k, &unit) in units.iter().enumerate() {
            let (w, h, rgba) = self.render_frame(unit, transparent, max_side)?;
            size.get_or_insert((w, h));
            let pixels = rgba.as_chunks::<4>().0;
            let step = (pixels.len() / per_frame).max(1);
            samples.extend(pixels.iter().step_by(step).filter(|p| p[3] >= 128));
            self.exported(0.45 * (k + 1) as f32 / units.len() as f32);
        }
        Ok(size.map(|(w, h)| GifWriter::shared(w, h, &samples)))
    }

    /// `unit` alone (the background too unless `transparent`), straight alpha, scaled to fit
    /// `max_side` (0: full size). The canvas shows the whole picture again afterwards.
    fn render_frame(
        &self,
        unit: LayerId,
        transparent: bool,
        max_side: u32,
    ) -> Result<(u32, u32, Vec<u8>), MobileError> {
        let (w, h, rgba) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let mut doc = st.doc.clone();
            for id in doc.members(None) {
                if id != unit {
                    doc.set_visible(id, false);
                }
            }
            if transparent {
                doc.background_visible = false;
            }
            let (w, h) = (doc.width(), doc.height());
            let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
                return Err(file_error("reading the picture", "the canvas is not shown yet"));
            };
            canvas.invalidate();
            canvas.render_finished(gpu, &mut doc);
            let rgba = canvas.read_composite(gpu);
            canvas.invalidate();
            canvas.render(gpu, &mut st.doc, None);
            self.shared.frame(st);
            (w, h, rgba.map_err(|e| file_error("reading the picture", e))?)
        };
        let side = if max_side == 0 { w.max(h) } else { max_side.min(w.max(h)) };
        Ok(thumbnail(&rgba, w, h, side))
    }
}

/// How playing goes on after the last frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnimationPlayback {
    Loop,
    PingPong,
    OneShot,
}

/// Animation Assist's settings, as the panel shows them.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnimationSettings {
    pub fps: u32,
    pub onion_frames: u32,
    pub onion_opacity: f32,
    pub playback: AnimationPlayback,
    pub blend_primary: bool,
    /// The onion skins' colours before and after the current frame, sRGB 0..1.
    pub before: Vec<f32>,
    pub after: Vec<f32>,
    pub background: bool,
    pub foreground: bool,
    /// Page Assist rather than Animation Assist.
    pub pages: bool,
}

impl From<&Animation> for AnimationSettings {
    fn from(a: &Animation) -> AnimationSettings {
        AnimationSettings {
            fps: a.fps,
            onion_frames: a.onion_frames,
            onion_opacity: a.onion_opacity,
            playback: match a.playback {
                Playback::Loop => AnimationPlayback::Loop,
                Playback::PingPong => AnimationPlayback::PingPong,
                Playback::OneShot => AnimationPlayback::OneShot,
            },
            blend_primary: a.blend_primary,
            before: a.before.to_vec(),
            after: a.after.to_vec(),
            background: a.background,
            foreground: a.foreground,
            pages: a.pages,
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Turns Animation Assist on (its settings as they start) or off, as an undo step.
    pub fn set_animation_assist(&self, on: bool) -> bool {
        self.set_assist(on, false)
    }

    /// Turns Page Assist on or off, as an undo step.
    pub fn set_page_assist(&self, on: bool) -> bool {
        self.set_assist(on, true)
    }
}

impl CanvasSession {
    /// Animation or Page Assist on (the other off), or that one off.
    fn set_assist(&self, on: bool, pages: bool) -> bool {
        let changed = self
            .change(
                match (on, pages) {
                    (true, false) => "show Animation Assist",
                    (false, false) => "hide Animation Assist",
                    (true, true) => "show Page Assist",
                    (false, true) => "hide Page Assist",
                },
                |d| {
                    let now = d.animation.as_ref().is_some_and(|a| a.pages == pages);
                    (now != on).then(|| d.animation = on.then(|| Animation { pages, ..Animation::default() }))
                },
            )
            .is_some();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(c) = st.canvas.as_mut() {
            c.set_play_frame(None);
            c.invalidate();
        }
        self.shared.frame(st);
        changed
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Animation Assist's settings while it is on.
    pub fn animation_settings(&self) -> Option<AnimationSettings> {
        self.shared.lock().doc.animation.as_ref().map(AnimationSettings::from)
    }

    /// Changes the settings (live, as sliders move).
    pub fn set_animation_settings(&self, s: AnimationSettings) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(a) = st.doc.animation.as_mut() else { return };
        let color = |v: &[f32], or: [f32; 3]| match v {
            [r, g, b] => [*r, *g, *b],
            _ => or,
        };
        *a = Animation {
            fps: s.fps,
            onion_frames: s.onion_frames,
            onion_opacity: s.onion_opacity,
            playback: match s.playback {
                AnimationPlayback::Loop => Playback::Loop,
                AnimationPlayback::PingPong => Playback::PingPong,
                AnimationPlayback::OneShot => Playback::OneShot,
            },
            blend_primary: s.blend_primary,
            before: color(&s.before, a.before),
            after: color(&s.after, a.after),
            background: s.background,
            foreground: s.foreground,
            holds: std::mem::take(&mut a.holds),
            pages: a.pages,
        }
        .repaired();
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }

    /// The frames' layers (top-level layers and groups), bottom first.
    pub fn animation_frames(&self) -> Vec<u64> {
        animation::frames(&self.shared.lock().doc).into_iter().map(|id| id.0).collect()
    }

    /// The frame holding the current layer.
    pub fn current_frame(&self) -> Option<u32> {
        animation::current_frame(&self.shared.lock().doc).and_then(|i| u32::try_from(i).ok())
    }

    /// Playing: frame `frame` shows alone; none goes back to drawing with onion skins.
    pub fn set_play_frame(&self, frame: Option<u32>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(c) = st.canvas.as_mut() {
            c.set_play_frame(frame.and_then(|f| usize::try_from(f).ok()));
        }
        self.shared.frame(st);
    }

    /// Frame `id`'s picture over the whole canvas, fitted to `max_side` (premultiplied), for the
    /// timeline; none for a group with nothing showing or before the canvas exists.
    pub fn frame_thumbnail(&self, id: u64, max_side: u32) -> Option<ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let id = LayerId(id);
        let group = st.doc.layer(id)?.kind == LayerKind::Group;
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
        if !group {
            let t = canvas.layer_picture(gpu, &mut st.doc, id, max_side).ok()?;
            return Some(ThumbnailImage { width: t.width, height: t.height, rgba: t.rgba });
        }
        // a group's layers laid over one another at this size, with no regard to their blend
        // modes: rendering the group alone would composite the whole canvas twice for each frame
        let mut picture: Option<ThumbnailImage> = None;
        for (layer, opacity) in showing_in(&st.doc, id, 1.0) {
            let t = canvas.layer_picture(gpu, &mut st.doc, layer, max_side).ok()?;
            let p = picture.get_or_insert_with(|| ThumbnailImage {
                width: t.width,
                height: t.height,
                rgba: vec![0; t.rgba.len()],
            });
            if p.rgba.len() == t.rgba.len() {
                lay_over(&mut p.rgba, &t.rgba, opacity);
            }
        }
        picture
    }

    /// How many more frames the frame of layer `id` is held.
    pub fn frame_hold(&self, id: u64) -> u32 {
        self.shared.lock().doc.animation.as_ref().map_or(0, |a| a.hold(LayerId(id)))
    }

    /// Holds the frame of layer `id` so many more frames as its slider moves; `done` ends the drag
    /// (one undo step for the whole drag).
    pub fn change_frame_hold(&self, id: u64, frames: u32, done: bool) -> bool {
        self.change_live("hold frame length", done, |d| {
            d.animation.as_mut().is_some_and(|a| {
                a.set_hold(LayerId(id), frames);
                true
            })
        })
    }

    /// Delete on a held place: the frame of layer `id` held one frame less.
    pub fn drop_held_frame(&self, id: u64) -> bool {
        self.change("hold frame length", |d| {
            let a = d.animation.as_mut()?;
            let hold = a.hold(LayerId(id)).checked_sub(1)?;
            a.set_hold(LayerId(id), hold);
            Some(())
        })
        .is_some()
    }

    /// Duplicate on a held place: from its `at`th held place (from 1) a copy of the frame of layer
    /// `id` takes over, held for what was left, and is made current; the frame holds up to there.
    /// Returns the copy; none past its hold, or with no room.
    pub fn split_held_frame(&self, id: u64, at: u32) -> Option<u64> {
        let hold = self.frame_hold(id);
        if at == 0 || at > hold {
            return None;
        }
        self.duplicated(id, "split held frame", |doc, copy| {
            if let Some(a) = doc.animation.as_mut() {
                a.set_hold(LayerId(id), at - 1);
                a.set_hold(copy, hold - at);
            }
        })
    }

    pub fn set_frame_hold(&self, id: u64, frames: u32) {
        if let Some(a) = self.shared.lock().doc.animation.as_mut() {
            a.set_hold(LayerId(id), frames);
        }
    }
}

/// The layers showing in group `id`, at any depth, bottom first, each with its opacity there
/// (`opacity` being the group's own surroundings').
fn showing_in(doc: &Document, id: LayerId, opacity: f32) -> Vec<(LayerId, f32)> {
    let mut shown = Vec::new();
    for member in doc.members(Some(id)) {
        let Some(layer) = doc.layer(member).filter(|l| l.visible) else { continue };
        if layer.kind == LayerKind::Group {
            shown.extend(showing_in(doc, member, opacity * layer.opacity));
        } else {
            shown.push((member, opacity * layer.opacity));
        }
    }
    shown
}

/// Premultiplied `top`, at `opacity`, laid over `under`.
fn lay_over(under: &mut [u8], top: &[u8], opacity: f32) {
    for (u, t) in under.as_chunks_mut::<4>().0.iter_mut().zip(top.as_chunks::<4>().0) {
        let [.., alpha] = *t;
        let keep = 1.0 - f32::from(alpha) / 255.0 * opacity;
        for (a, b) in u.iter_mut().zip(t) {
            *a = (f32::from(*b) * opacity + f32::from(*a) * keep).round().min(255.0) as u8;
        }
    }
}

/// A step of playing: the place in the sequence shown next, and the way it is going (1 forward,
/// -1 back).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PlayStep {
    pub at: u32,
    pub step: i32,
}

/// Where playing goes after place `at` of a sequence `len` long, moving `step`: on, round again
/// (Loop), turned back (Ping-Pong), or none once a single pass ends (One Shot, or one frame to
/// ping-pong).
#[uniffi::export]
pub fn animation_next_step(playback: AnimationPlayback, len: u32, at: u32, step: i32) -> Option<PlayStep> {
    let step = if step < 0 { -1 } else { 1 };
    let next = i64::from(at) + i64::from(step);
    if (0..i64::from(len)).contains(&next) {
        return Some(PlayStep { at: u32::try_from(next).ok()?, step });
    }
    match playback {
        AnimationPlayback::Loop if len > 0 => Some(PlayStep { at: 0, step }),
        AnimationPlayback::PingPong if len > 1 => {
            let back = i64::from(at) - i64::from(step);
            Some(PlayStep { at: u32::try_from(back).ok()?, step: -step })
        }
        _ => None,
    }
}

/// The animated exports' frame rate slider: 0.1 to 60 frames a second, the rate growing as the
/// fraction along it to the power 2.4 (slow rates spread out at the start).
const EXPORT_FPS_LEAST: f32 = 0.1;
const EXPORT_FPS_MOST: f32 = 60.0;
const EXPORT_FPS_CURVE: f32 = 2.4;

/// `fps` as the slider shows it, and as the file is made at: in tenths under 1, whole from 1 up.
fn shown_fps(fps: f32) -> f32 {
    let fps = if fps.is_nan() { EXPORT_FPS_LEAST } else { fps.clamp(EXPORT_FPS_LEAST, EXPORT_FPS_MOST) };
    let tenths = (fps * 10.0).round() / 10.0;
    if tenths < 1.0 { tenths.max(EXPORT_FPS_LEAST) } else { fps.round() }
}

/// The frame rate an animated export's slider gives at `fraction` (0..1) along it, as shown.
#[uniffi::export]
pub fn export_fps_at(fraction: f32) -> f32 {
    let t = if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) };
    shown_fps(EXPORT_FPS_LEAST + (EXPORT_FPS_MOST - EXPORT_FPS_LEAST) * t.powf(EXPORT_FPS_CURVE))
}

/// Where along the slider (0..1) frame rate `fps` sits.
#[uniffi::export]
pub fn export_fps_fraction(fps: f32) -> f32 {
    let fps = if fps.is_nan() { EXPORT_FPS_LEAST } else { fps.clamp(EXPORT_FPS_LEAST, EXPORT_FPS_MOST) };
    ((fps - EXPORT_FPS_LEAST) / (EXPORT_FPS_MOST - EXPORT_FPS_LEAST)).powf(1.0 / EXPORT_FPS_CURVE)
}

/// The slider's value for frame rate `fps`: "0.5" under 1, "11" from 1 up.
#[uniffi::export]
pub fn export_fps_label(fps: f32) -> String {
    let fps = shown_fps(fps);
    if fps < 1.0 { format!("{fps:.1}") } else { format!("{fps:.0}") }
}

/// A frame rate as a ratio, `frames` every `seconds` (0.1 a second: 1 every 10), so a video's
/// frame times can be worked out exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FrameRate {
    pub frames: u32,
    pub seconds: u32,
}

impl FrameRate {
    /// When frame `index` starts, in `per_second` units of a second (rounded down).
    pub fn time_of(self, index: u64, per_second: u64) -> u64 {
        let t = u128::from(index) * u128::from(per_second) * u128::from(self.seconds) / u128::from(self.frames.max(1));
        u64::try_from(t).unwrap_or(u64::MAX)
    }
}

/// `fps` frames a second as a ratio, to a thousandth of a frame (at least a frame in 1000 seconds,
/// at most 1000 frames a second).
#[uniffi::export]
pub fn frame_rate(fps: f32) -> FrameRate {
    let fps = if fps.is_nan() { 1.0 } else { fps.clamp(0.001, 1000.0) };
    let thousandths = ((fps * 1000.0).round() as u32).max(1);
    let (mut a, mut b) = (thousandths, 1000);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    FrameRate { frames: thousandths / a, seconds: 1000 / a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_laid_over_keeps_what_shows_through_it() {
        // half-covering red at half strength, over solid blue
        let mut under = [0, 0, 255, 255];
        lay_over(&mut under, &[128, 0, 0, 128], 0.5);
        assert_eq!(under, [64, 0, 191, 255]);
        // over nothing it is itself
        let mut under = [0, 0, 0, 0];
        lay_over(&mut under, &[10, 20, 30, 40], 1.0);
        assert_eq!(under, [10, 20, 30, 40]);
    }

    #[test]
    fn the_export_frame_rate_slider_goes_from_a_tenth_to_sixty_slow_at_the_start() {
        assert_eq!(export_fps_at(0.0), 0.1);
        assert_eq!(export_fps_at(1.0), 60.0);
        // as measured along it
        for (fraction, fps) in [(0.125, 0.5), (0.25, 2.0), (0.375, 6.0), (0.5, 11.0), (0.56, 15.0), (0.75, 30.0)] {
            assert!((export_fps_at(fraction) - fps).abs() < 1e-6, "{fraction}: {}", export_fps_at(fraction));
        }
        // past the ends it stays at them
        assert_eq!(export_fps_at(-1.0), 0.1);
        assert_eq!(export_fps_at(2.0), 60.0);
        assert_eq!(export_fps_at(f32::NAN), 0.1);
    }

    #[test]
    fn a_frame_rate_put_on_the_slider_reads_back_the_same() {
        // the screen opens at the artwork's own rate
        for fps in [0.1, 0.3, 0.5, 0.9, 1.0, 2.0, 12.0, 15.0, 24.0, 30.0, 59.0, 60.0] {
            assert!((export_fps_at(export_fps_fraction(fps)) - fps).abs() < 1e-5, "{fps}");
        }
        assert_eq!(export_fps_fraction(0.1), 0.0);
        assert!((export_fps_fraction(60.0) - 1.0).abs() < 1e-6);
        assert!((export_fps_fraction(15.0) - 0.56).abs() < 0.005);
        assert_eq!(export_fps_fraction(100.0), export_fps_fraction(60.0));
    }

    #[test]
    fn the_frame_rate_shows_tenths_under_one_and_whole_from_one() {
        assert_eq!(export_fps_label(0.1), "0.1");
        assert_eq!(export_fps_label(0.5), "0.5");
        assert_eq!(export_fps_label(0.94), "0.9");
        assert_eq!(export_fps_label(0.96), "1");
        assert_eq!(export_fps_label(11.45), "11");
        assert_eq!(export_fps_label(60.0), "60");
        assert_eq!(export_fps_label(export_fps_at(0.125)), "0.5");
        assert_eq!(export_fps_label(export_fps_at(0.5)), "11");
        // what is shown is what the file is made at
        for i in 0..=1000 {
            let fps = export_fps_at(i as f32 / 1000.0);
            assert_eq!(export_fps_label(fps).parse::<f32>().unwrap(), fps);
        }
    }

    #[test]
    fn a_frame_rate_as_a_ratio_gives_exact_frame_times() {
        assert_eq!(frame_rate(0.1), FrameRate { frames: 1, seconds: 10 });
        assert_eq!(frame_rate(0.5), FrameRate { frames: 1, seconds: 2 });
        assert_eq!(frame_rate(0.3), FrameRate { frames: 3, seconds: 10 });
        assert_eq!(frame_rate(15.0), FrameRate { frames: 15, seconds: 1 });
        assert_eq!(frame_rate(60.0), FrameRate { frames: 60, seconds: 1 });
        // at 0.1 a frame lasts ten seconds, in 100 ns units
        let slow = frame_rate(0.1);
        assert_eq!(slow.time_of(1, 10_000_000), 100_000_000);
        assert_eq!(slow.time_of(7, 1_000_000), 70_000_000);
        // no drift over many frames
        assert_eq!(frame_rate(0.3).time_of(3000, 1000), 10_000_000);
        assert_eq!(frame_rate(15.0).time_of(15, 1_000_000), 1_000_000);
        assert_eq!(frame_rate(0.0).seconds, 1000);
    }

    fn run(playback: AnimationPlayback, len: u32, steps: usize) -> Vec<u32> {
        let mut shown = vec![0];
        let (mut at, mut step) = (0, 1);
        for _ in 0..steps {
            let Some(next) = animation_next_step(playback, len, at, step) else { break };
            (at, step) = (next.at, next.step);
            shown.push(at);
        }
        shown
    }

    #[test]
    fn playing_loops_turns_back_or_stops() {
        assert_eq!(run(AnimationPlayback::Loop, 3, 6), [0, 1, 2, 0, 1, 2, 0]);
        assert_eq!(run(AnimationPlayback::PingPong, 3, 6), [0, 1, 2, 1, 0, 1, 2]);
        assert_eq!(run(AnimationPlayback::OneShot, 3, 6), [0, 1, 2]);
        // one frame: looping shows it again and again; ping-pong and one shot stop
        assert_eq!(run(AnimationPlayback::Loop, 1, 2), [0, 0, 0]);
        assert_eq!(run(AnimationPlayback::PingPong, 1, 2), [0]);
        assert_eq!(animation_next_step(AnimationPlayback::Loop, 0, 0, 1), None);
    }
}
