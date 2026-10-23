//! The layers as an animation: each visible top-level layer or group a frame, bottom first, as
//! an animated GIF or PNG, or a frame at a time for a video or a preview.

use std::fs;
use std::io::BufWriter;
use std::path::Path;

use procraft_engine::animation::{self, Animation, Playback};
use procraft_engine::document::{Document, LayerId};
use procraft_engine::formats::GifWriter;

use crate::layers::ThumbnailImage;
use crate::project::{file_error, thumbnail, write_atomically};
use crate::{CanvasSession, MobileError};

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
    /// `max_side` (0: full size); `dither` spreads the palette's error, `transparent` leaves
    /// the background out.
    pub fn export_gif(
        &self,
        path: String,
        max_side: u32,
        fps: f32,
        dither: bool,
        transparent: bool,
    ) -> Result<(), MobileError> {
        let units = sequence(&self.shared.lock().doc);
        let delay = (100.0 / fps.clamp(0.1, 100.0)).round() as u16;
        let mut gif: Option<GifWriter> = None;
        for unit in units {
            let (w, h, rgba) = self.render_frame(unit, transparent, max_side)?;
            gif.get_or_insert_with(|| GifWriter::new(w, h)).frame(&rgba, delay.max(1), dither);
        }
        let bytes = gif.ok_or_else(|| file_error("saving", "there are no frames"))?.finish();
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))
    }

    /// The animation as a looping animated PNG at `path`, `fps` frames a second, scaled to fit
    /// `max_side` (0: full size), `transparent` leaving the background out.
    pub fn export_apng(&self, path: String, max_side: u32, fps: f32, transparent: bool) -> Result<(), MobileError> {
        let units = sequence(&self.shared.lock().doc);
        let count = u32::try_from(units.len()).unwrap_or(u32::MAX);
        let Some((&first, rest)) = units.split_first() else {
            return Err(file_error("saving", "there are no frames"));
        };
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
            for &unit in rest {
                let (fw, fh, frame) = self.render_frame(unit, transparent, max_side)?;
                if (fw, fh) != (w, h) {
                    return Err(file_error("saving", "the frames differ in size"));
                }
                writer.write_image_data(&frame).map_err(|e| file_error("saving", e))?;
            }
            writer.finish().map_err(|e| file_error("saving", e))
        })
    }
}

impl CanvasSession {
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
            canvas.render(gpu, &mut doc, None);
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
    /// timeline; none for a group or before the canvas exists.
    pub fn frame_thumbnail(&self, id: u64, max_side: u32) -> Option<ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.doc.layer(LayerId(id)).is_none_or(|l| l.kind == procraft_engine::document::LayerKind::Group) {
            return None;
        }
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
        let t = canvas.layer_picture(gpu, &mut st.doc, LayerId(id), max_side).ok()?;
        Some(ThumbnailImage { width: t.width, height: t.height, rgba: t.rgba })
    }

    /// How many more frames the frame of layer `id` is held.
    pub fn frame_hold(&self, id: u64) -> u32 {
        self.shared.lock().doc.animation.as_ref().map_or(0, |a| a.hold(LayerId(id)))
    }

    pub fn set_frame_hold(&self, id: u64, frames: u32) {
        if let Some(a) = self.shared.lock().doc.animation.as_mut() {
            a.set_hold(LayerId(id), frames);
        }
    }
}
