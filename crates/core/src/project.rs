//! Saving and opening artworks. An artwork is a folder:
//!
//! - `artwork.json`: the format, the title and the document (layers and their settings).
//! - `layers/<id>.png`: each layer's pixels, premultiplied RGBA8 stored losslessly in a PNG (an
//!   internal store, not for other apps).
//! - `thumbnail.png`: the picture at up to [`THUMBNAIL_SIDE`] pixels, for the gallery.
//!
//! Files are written beside their final name and renamed into place, so an interrupted save
//! leaves the previous version whole.

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_engine::document::{BlendMode, Document, LayerId, LayerKind};
use procraft_engine::formats::{self, PsdKind, PsdLayer};
use procraft_engine::geom::Vec2;
use procraft_engine::view::View;

use crate::{CanvasSession, MobileError};

/// The gallery thumbnail's longest side, pixels.
pub const THUMBNAIL_SIDE: u32 = 512;
/// [`CanvasSession::edits_saved`] before any save.
pub(crate) const NOT_SAVED: u64 = u64::MAX;
/// How often an open artwork kept saved looks for changes to save.
const AUTOSAVE_EVERY: std::time::Duration = std::time::Duration::from_millis(if cfg!(test) { 50 } else { 3000 });
/// How soon it looks again when something under way kept it from saving: fast painting leaves
/// only short gaps between strokes, and what isn't saved is gone if the GPU's device goes.
const AUTOSAVE_AGAIN: std::time::Duration = std::time::Duration::from_millis(if cfg!(test) { 5 } else { 250 });
/// A package's preview's longest side, pixels.
const PACKAGE_PREVIEW_SIDE: u32 = 1024;
const FORMAT: &str = "procraft-artwork";
const VERSION: u32 = 1;

/// Whether the artwork in `dir` plays: a model, which turns, or Animation Assist or Page Assist on, from its file alone.
#[uniffi::export]
pub fn artwork_plays(dir: String) -> bool {
    let text = fs::read_to_string(Path::new(&dir).join("artwork.json"));
    let file = text.ok().and_then(|t| serde_json::from_str::<ArtworkFile>(&t).ok());
    file.is_some_and(|f| f.document.animation.is_some() || f.model.is_some())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ArtworkFile {
    format: String,
    version: u32,
    name: String,
    document: Document,
    /// Read as kept, or its default if it can't be (as are the document's own settings).
    #[serde(default, deserialize_with = "procraft_engine::document::or_default")]
    info: ArtworkInfo,
    /// How it was last shown, if it was.
    #[serde(
        default,
        deserialize_with = "procraft_engine::document::or_default",
        skip_serializing_if = "Option::is_none"
    )]
    view: Option<SavedView>,
    /// The model painted on, if it is one (its current texture set is `document`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<crate::scene3d::SavedModel>,
    /// The Reference window as it was left, if it was ever used (its picture beside the layers).
    #[serde(
        default,
        deserialize_with = "procraft_engine::document::or_default",
        skip_serializing_if = "Option::is_none"
    )]
    reference: Option<procraft_engine::reference_window::ReferenceWindow>,
    /// The kind of artwork it is, if a plugin makes it (`<id>.<key>`): the plugin's data is beside
    /// it in `plugin-data.bin`, and the layers are what it drew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plugin: Option<String>,
    /// The layer a plugin's artwork's picture is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plugin_layer: Option<u64>,
}

/// A plugin's artwork's data in its folder.
pub(crate) const PLUGIN_DATA: &str = "plugin-data.bin";

/// A view kept with an artwork: the canvas point in the middle of the window, the zoom and the
/// rotation (radians).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SavedView {
    centre: [f32; 2],
    zoom: f32,
    rotation: f32,
}

impl SavedView {
    /// The view `view` makes in a window `window` pixels.
    pub(crate) fn of(view: &View, window: (u32, u32)) -> Option<SavedView> {
        let middle = Vec2::new(window.0 as f32 / 2.0, window.1 as f32 / 2.0);
        let centre = view.screen_to_canvas(middle)?;
        Some(SavedView { centre: [centre.x, centre.y], zoom: view.zoom(), rotation: view.to_screen().rotation() })
    }

    /// It again, in a window `window` pixels.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn view(&self, window: (u32, u32)) -> Option<View> {
        View::centred(Vec2::new(self.centre[0], self.centre[1]), self.zoom, self.rotation, window)
    }
}

/// Whether `view` shows enough of a `canvas`-sized canvas in a `window`-sized window to work on:
/// its box on screen covers a tenth of the window, or half of it is in the window. A kept view
/// that doesn't (the canvas pushed off the page) is not used again.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn shows_canvas(view: &View, window: (u32, u32), canvas: (u32, u32)) -> bool {
    let m = view.to_screen();
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let corners = [Vec2::new(0.0, 0.0), Vec2::new(cw, 0.0), Vec2::new(0.0, ch), Vec2::new(cw, ch)].map(|p| m.apply(p));
    let (x0, x1) = corners.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
    let (y0, y1) = corners.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    let (ww, wh) = (window.0 as f32, window.1 as f32);
    let seen = (x1.min(ww) - x0.max(0.0)).max(0.0) * (y1.min(wh) - y0.max(0.0)).max(0.0);
    let whole = (x1 - x0) * (y1 - y0);
    seen.is_finite() && (seen >= 0.1 * ww * wh || seen >= 0.5 * whole)
}

/// An artwork's time-lapse in a package: whether it records, its video's settings (quality 0 to 3,
/// resolution 0 to 2, HEVC or H.264), and its video so far as MP4 files in order, `width` ×
/// `height` at `fps` and `bitrate` bits a second.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PackageTimeLapse {
    pub recording: bool,
    pub quality: u8,
    pub resolution: u8,
    pub hevc: bool,
    pub segments: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u64,
}

/// What an artwork keeps about itself besides the picture.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct ArtworkInfo {
    /// When it was made, milliseconds since 1970 (0: not known).
    #[serde(default)]
    pub created_ms: u64,
    /// Strokes made on it.
    #[serde(default)]
    pub strokes: u64,
    /// Time spent on it, milliseconds.
    #[serde(default)]
    pub tracked_ms: u64,
    /// Who made it.
    #[serde(default)]
    pub author: String,
}

pub(crate) fn file_error(context: &str, e: impl std::fmt::Display) -> MobileError {
    MobileError::File { reason: format!("{context}: {e}") }
}

/// What a save writes, taken under the lock: the artwork's file, the layers not already in the
/// folder as they are (each with its hash) and the picture (if shown). What the GPU still copies
/// back is waited for after the lock is let go.
pub(crate) struct SaveSnapshot {
    /// The layers that had changed, counted as changed again if the save fails.
    changed: Vec<LayerId>,
    file: ArtworkFile,
    layers: Vec<(LayerId, u64, Vec<u8>)>,
    /// Layers on their way back from the GPU, to be checked against the folder once here.
    reading: Vec<(LayerId, procraft_gpu::Readback)>,
    /// Layers still the PNG files they were opened from (not shown yet), written as they are.
    files: Vec<(LayerId, Vec<u8>)>,
    composite: Option<procraft_gpu::Readback>,
    /// Not shown yet, with no thumbnail in the folder: one made from its top picture, for the
    /// gallery until the canvas draws its own.
    unshown: Option<(u32, u32, Vec<u8>)>,
    model: Option<crate::scene3d::ModelSnapshot>,
    /// The device read from, to tell whether it was lost meanwhile.
    gpu: Option<procraft_gpu::Gpu>,
    /// The Reference window's picture, if it has one.
    reference_picture: Option<Arc<Vec<u8>>>,
    /// A plugin's artwork's data, if it is one.
    plugin_data: Option<Vec<u8>>,
}

/// How much of a save's layers may still be on their way back from the GPU when the lock is let
/// go; past it they are read (and those unchanged let go) under it. Small in tests, so their
/// saves go both ways.
const READ_AFTER_LOCK: usize = if cfg!(test) { 64 << 10 } else { 256 << 20 };

/// What the last saves wrote: the folder, and a hash of each layer's pixels, so layers that
/// haven't changed aren't written again.
#[derive(Default)]
pub(crate) struct Saved {
    dir: PathBuf,
    layers: std::collections::HashMap<LayerId, u64>,
    sets: crate::scene3d::SavedSets,
    /// Which autosave may save: one started under another number has been stopped.
    autosave: u64,
    /// A hash of the Reference window's picture as the folder has it, if it has one.
    reference_picture: Option<u64>,
}

impl Saved {
    /// The hashes of the layers the last saves wrote to `dir`; none for another folder.
    pub(crate) fn known(&self, dir: &Path) -> std::collections::HashMap<LayerId, u64> {
        if self.dir == dir { self.layers.clone() } else { Default::default() }
    }

    /// An artwork opened from `dir`: its layer files are what the last save there wrote, their
    /// hashes known as each is read ([`Saved::read`]).
    pub(crate) fn opened(&mut self, dir: &Path) {
        self.dir = dir.to_path_buf();
        self.layers.clear();
        self.reference_picture = None;
    }

    /// Layer `id` read from its file, its pixels' [`pixels_hash`]: a save that finds it unchanged
    /// leaves it.
    pub(crate) fn read(&mut self, id: LayerId, hash: u64) {
        self.layers.insert(id, hash);
    }
}

/// A quick hash of pixels, to tell a changed layer from one saved already.
pub(crate) fn pixels_hash(rgba: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let (words, rest) = rgba.as_chunks::<8>();
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for w in words {
        h = (h ^ u64::from_le_bytes(*w)).wrapping_mul(PRIME).rotate_left(29);
    }
    for &b in rest {
        h = (h ^ u64::from(b)).wrapping_mul(PRIME);
    }
    h
}

impl crate::State {
    /// The time counted since tracking last started goes into the artwork's (at most a day at
    /// once), and counts on from now.
    pub(crate) fn count_tracked_time(&mut self) {
        let Some(since) = self.tracking_since else { return };
        let now = std::time::Instant::now();
        let ms = u64::try_from(now.duration_since(since).as_millis()).unwrap_or(u64::MAX).min(24 * 3600 * 1000);
        self.info.tracked_ms = self.info.tracked_ms.saturating_add(ms);
        self.tracking_since = Some(now);
    }

    /// See [`CanvasSession::edits`].
    pub(crate) fn edits(&self) -> u64 {
        self.revision.wrapping_add(self.canvas.as_ref().map_or(0, procraft_gpu::Canvas::steps_made))
    }

    /// Nothing under way that saving would end or cut short.
    pub(crate) fn idle(&self) -> bool {
        self.stroke.is_none()
            && self.plugin_artwork.as_ref().is_none_or(|a| !a.down)
            && self.transform.is_none()
            && !self.adjusting
            && self.text_edit.is_none()
            && self.shape.is_none()
            && self.pending_doc.is_none()
            && self.feather_base.is_none()
            && self.selection_before.is_none()
            // a ColorDrop's fill still open
            && self.pour.is_none()
            && self.scene.as_ref().is_none_or(|s| !s.stroking)
    }

    /// The artwork as it is now, for saving into `dir`: a layer already there as it is (its hash
    /// among the `known` and its file there) is let go as soon as it is read, so a save holds only
    /// the layers that changed. Up to [`READ_AFTER_LOCK`] of the layers, and the picture, are
    /// only started back from the GPU here, to be waited for once the lock is let go.
    pub(crate) fn save_snapshot(
        &mut self,
        dir: &Path,
        known: &std::collections::HashMap<LayerId, u64>,
    ) -> Result<SaveSnapshot, MobileError> {
        let changed = |id: LayerId, rgba: Vec<u8>| {
            let hash = pixels_hash(&rgba);
            let there = known.get(&id) == Some(&hash) && layer_path(dir, id).exists();
            (!there).then_some((id, hash, rgba))
        };
        // the time on it up to now saved with it
        self.count_tracked_time();
        let model = self.model_snapshot()?;
        let file = ArtworkFile {
            format: FORMAT.to_owned(),
            version: VERSION,
            name: self.name.clone(),
            document: self.doc.clone(),
            info: self.info.clone(),
            // as shown (while the guide editor is open, as it was before it), or as it opened if it
            // hasn't been shown
            view: self
                .guide_before
                .as_ref()
                .map_or(self.view, |(_, before)| *before)
                .as_ref()
                .and_then(|v| SavedView::of(v, self.window_size))
                .or(self.saved_view),
            model: model.as_ref().map(|m| m.saved.clone()),
            reference: Some(self.reference_window.clone()).filter(|w| !w.is_unused()),
            // (opened with its plugin off, it stays the plugin's: its kind and data kept as they were)
            plugin: self
                .plugin_artwork
                .as_ref()
                .map(|a| a.kind.clone())
                .or_else(|| self.plugin_opened.as_ref().map(|o| o.kind.clone())),
            plugin_layer: self
                .plugin_artwork
                .as_ref()
                .map(|a| a.layer.0)
                .or_else(|| self.plugin_opened.as_ref().and_then(|o| o.layer)),
        };
        let mut layers = Vec::new();
        let mut reading = Vec::new();
        let mut files = Vec::new();
        let mut composite = None;
        let mut unshown = None;
        let mut changed_ids = Vec::new();
        // shown but its canvas still being made (on another thread): made now, so the save has its
        // picture for the gallery
        if self.gpu.is_some() && self.canvas.is_none() {
            self.ensure_canvas();
        }
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.render(gpu, &mut self.doc, None);
            let changed_now = canvas.take_changed();
            changed_ids = changed_now.iter().copied().collect();
            let mut read_all = || -> Result<(), MobileError> {
                // the layers with pixels (a group has none of its own)
                for id in self.doc.pixel_layer_ids() {
                    // unchanged since it was read or saved, its file there: left as it is, unread
                    if !changed_now.contains(&id) && known.contains_key(&id) && layer_path(dir, id).exists() {
                        continue;
                    }
                    let read = canvas.start_reading_layer(gpu, id).map_err(|e| file_error("reading a layer", e))?;
                    let after: usize = reading.iter().map(|(_, r): &(LayerId, procraft_gpu::Readback)| r.bytes()).sum();
                    if after + read.bytes() <= READ_AFTER_LOCK {
                        reading.push((id, read));
                    } else {
                        let rgba = read.pixels().map_err(|e| file_error("reading a layer", e))?;
                        layers.extend(changed(id, rgba));
                    }
                }
                Ok(())
            };
            // a layer that can't be read: those changed count as changed again, for the next save
            if let Err(e) = read_all() {
                canvas.mark_changed(changed_ids);
                return Err(e);
            }
            // for the gallery's thumbnail: private layers left out
            let private = self.doc.private_layers();
            composite = Some(canvas.start_reading_composite_without(gpu, &mut self.doc, &private));
        } else {
            // not shown yet: what was opened is still what there is. A layer still its file's
            // bytes isn't decoded: it is written as they are, or not at all if that file is there
            for (id, layer) in &self.pending_pixels {
                match layer {
                    Opened::Png(bytes) => {
                        if fs::read(layer_path(dir, *id)).ok().as_ref() != Some(bytes) {
                            files.push((*id, bytes.clone()));
                        }
                    }
                    Opened::Pixels(rgba) => layers.extend(changed(*id, rgba.clone())),
                }
            }
            // a picture made into an artwork and saved before it is shown (the app may be closed
            // before it is): the gallery shows the picture
            if !dir.join("thumbnail.png").exists() {
                let (w, h) = (self.doc.width(), self.doc.height());
                unshown = self.pending_pixels.iter().rev().find_map(|(_, layer)| match layer {
                    Opened::Pixels(rgba) => Some(thumbnail(rgba, w, h, THUMBNAIL_SIDE)),
                    Opened::Png(_) => None,
                });
            }
        }
        let gpu = self.gpu.clone();
        let reference_picture = self.reference_picture.clone();
        let plugin_data = self
            .plugin_artwork
            .as_ref()
            .map(|a| a.kept.clone())
            .or_else(|| self.plugin_opened.as_ref().and_then(|o| o.data.clone()));
        Ok(SaveSnapshot {
            changed: changed_ids,
            file,
            layers,
            reading,
            files,
            composite,
            unshown,
            model,
            gpu,
            reference_picture,
            plugin_data,
        })
    }
}

/// Writes a save; if it fails, the layers it had as changed count as changed again.
fn write_or_keep_changed(
    session: &CanvasSession,
    saved: &mut Saved,
    dir: &Path,
    snapshot: SaveSnapshot,
) -> Result<(), MobileError> {
    let changed = snapshot.changed.clone();
    let result = write_artwork(saved, dir, snapshot);
    if result.is_err()
        && let Some(canvas) = session.shared.lock().canvas.as_mut()
    {
        canvas.mark_changed(changed);
    }
    result
}

impl CanvasSession {
    /// [`CanvasSession::start_autosave`], looking `every` so long, and `again` after that when
    /// something was under way.
    pub(crate) fn keep_saved(self: Arc<Self>, dir: String, every: std::time::Duration, again: std::time::Duration) {
        let mine = {
            let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
            saved.autosave = saved.autosave.wrapping_add(1);
            saved.autosave
        };
        let mut kept = self.edits();
        let session = Arc::downgrade(&self);
        let started = std::thread::Builder::new().name("autosave".into()).spawn(move || {
            let mut wait = every;
            loop {
                std::thread::sleep(wait);
                wait = every;
                let Some(session) = session.upgrade() else { return };
                let mut saved = session.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
                if saved.autosave != mine {
                    return;
                }
                let now = session.edits();
                if now == kept {
                    continue;
                }
                let lost = || session.shared.lock().gpu.as_ref().is_some_and(procraft_gpu::Gpu::is_lost);
                if lost() {
                    return;
                }
                if !session.shared.lock().idle() {
                    wait = again;
                    continue;
                }
                let result = session.save_if_idle_with(&mut saved, &dir);
                // lost during the save: that is what failed it, and it tells of itself
                if result.is_err() && lost() {
                    return;
                }
                let mut failure = session.shared.save_failure.lock().unwrap_or_else(|e| e.into_inner());
                match result {
                    Ok(true) => {
                        kept = now;
                        failure.1 = false;
                    }
                    Ok(false) => wait = again,
                    Err(e) => {
                        log::error!("keeping the artwork saved: {e}");
                        if !failure.1 {
                            *failure = (Some(e.to_string()), true);
                        }
                    }
                }
            }
        });
        if let Err(e) = started {
            log::error!("keeping the artwork saved: {e}");
        }
    }

    /// [`CanvasSession::save_if_idle`] with what the last saves wrote (its lock held).
    fn save_if_idle_with(&self, saved: &mut Saved, dir: &str) -> Result<bool, MobileError> {
        let (snapshot, edits) = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            if !st.idle() {
                return Ok(false);
            }
            // a model saves its colour as the artwork's document
            let known = saved.known(Path::new(dir));
            (st.with_colour(|st| st.save_snapshot(Path::new(dir), &known))?, st.edits())
        };
        write_or_keep_changed(self, saved, Path::new(dir), snapshot)?;
        self.shared.edits_saved.store(edits, std::sync::atomic::Ordering::Relaxed);
        Ok(true)
    }
}

/// Writes a save into `dir` (made if missing): the layers that changed since the last save there,
/// the thumbnail and the document.
fn write_artwork(saved: &mut Saved, dir: &Path, snapshot: SaveSnapshot) -> Result<(), MobileError> {
    let SaveSnapshot {
        file,
        mut layers,
        reading,
        files,
        composite,
        unshown,
        model,
        gpu,
        reference_picture,
        plugin_data,
        ..
    } = snapshot;
    // what was still on its way back: those unchanged since the last save there are let go
    for (id, read) in reading {
        let rgba = read.pixels().map_err(|e| file_error("reading a layer", e))?;
        let hash = pixels_hash(&rgba);
        let there = saved.dir == dir && saved.layers.get(&id) == Some(&hash) && layer_path(dir, id).exists();
        if !there {
            layers.push((id, hash, rgba));
        }
    }
    let composite =
        composite.map(procraft_gpu::Readback::pixels).transpose().map_err(|e| file_error("reading the picture", e))?;
    // a device lost meanwhile read back nothing true: the last save is kept as it is
    if gpu.as_ref().is_some_and(procraft_gpu::Gpu::is_lost) {
        return Err(file_error("reading the layers", "the graphics device was lost"));
    }
    if saved.dir != dir {
        saved.dir = dir.to_path_buf();
        saved.layers.clear();
        saved.sets.clear();
        saved.reference_picture = None;
    }
    let (w, h) = (file.document.width(), file.document.height());
    fs::create_dir_all(dir.join("layers")).map_err(|e| file_error("making the folder", e))?;
    // Everything new is written beside its place first and only then put in place, the document
    // last: a save cut short while writing (the slow part) leaves the last one as it was. Written
    // in place one by one, a save after a crop cut short left layers of the new size beside a
    // document of the old, which the next open would drop.
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let stage = |result: Result<PathBuf, MobileError>, to: PathBuf, staged: &mut Vec<(PathBuf, PathBuf)>| {
        result.map(|tmp| staged.push((tmp, to)))
    };
    let written = (|| {
        // only those not already there as they are, encoded and written side by side, each
        // layer's PNG made in memory before it is written
        let each = w as usize * h as usize * 4;
        let results = procraft_engine::par::side_by_side_within(&layers, each, |(id, _, rgba)| {
            write_beside(&layer_path(dir, *id), |p| write_png(p, w, h, rgba, &[]))
        });
        // every one written kept track of, so a failure among them leaves none of them beside
        let mut failed = None;
        for ((id, _, _), result) in layers.iter().zip(results) {
            let result = result.unwrap_or_else(|| Err(file_error("writing a layer", "it stopped")));
            if let Err(e) = stage(result, layer_path(dir, *id), &mut staged) {
                failed.get_or_insert(e);
            }
        }
        if let Some(e) = failed {
            return Err(e);
        }
        for (id, bytes) in &files {
            let to = layer_path(dir, *id);
            stage(
                write_beside(&to, |p| fs::write(p, bytes).map_err(|e| file_error("writing a layer", e))),
                to,
                &mut staged,
            )?;
        }
        if let Some(model) = model.as_ref() {
            crate::scene3d::write_model(&mut saved.sets, dir, model)?;
        }
        // a model's thumbnail is the model as last shown
        let thumb = dir.join("thumbnail.png");
        if let Some((pw, ph, rgba)) = model.and_then(|m| m.picture) {
            let (tw, th, small) = thumbnail(&rgba, pw, ph, THUMBNAIL_SIDE);
            stage(write_beside(&thumb, |p| write_png(p, tw, th, &small, &[])), thumb, &mut staged)?;
        } else if let Some((tw, th, mut small)) =
            composite.map(|rgba| thumbnail(&rgba, w, h, THUMBNAIL_SIDE)).or(unshown)
        {
            // the gallery shows it in the artwork's colours (a CMYK canvas's: as its press prints them)
            if let Some(table) = proof_table(&file.document) {
                table.apply_rgba(&mut small, false);
            }
            let icc = file.document.picture_icc();
            stage(write_beside(&thumb, |p| write_png(p, tw, th, &small, &icc)), thumb, &mut staged)?;
        }
        // the Reference window's picture, unless the folder has it already
        let picture_path = dir.join(crate::reference_window::REFERENCE_PICTURE);
        if let Some(png) = reference_picture.as_deref()
            && (saved.reference_picture != Some(pixels_hash(png)) || !picture_path.exists())
        {
            let written = write_beside(&picture_path, |p| {
                fs::write(p, png).map_err(|e| file_error("writing the reference picture", e))
            });
            stage(written, picture_path, &mut staged)?;
        }
        // a plugin's artwork's own data, whole, before the document that names its kind
        if let Some(data) = plugin_data.as_deref() {
            let path = dir.join(PLUGIN_DATA);
            let written =
                write_beside(&path, |p| fs::write(p, data).map_err(|e| file_error("writing the plugin's artwork", e)));
            stage(written, path, &mut staged)?;
        }
        let json = serde_json::to_string(&file).map_err(|e| file_error("writing the artwork", e))?;
        let doc = dir.join("artwork.json");
        stage(
            write_beside(&doc, |p| fs::write(p, json.as_bytes()).map_err(|e| file_error("writing the artwork", e))),
            doc,
            &mut staged,
        )
    })();
    // in place, in the order written (the document last), or, short of that, none of it
    let placed = written.and_then(|()| {
        staged.iter().try_for_each(|(tmp, to)| fs::rename(tmp, to).map_err(|e| file_error("saving", e)))
    });
    if let Err(e) = placed {
        for (tmp, _) in &staged {
            let _ = fs::remove_file(tmp);
        }
        return Err(e);
    }
    for (id, hash, _) in &layers {
        saved.layers.insert(*id, *hash);
    }
    // a picture taken away goes once the document without it is in place
    saved.reference_picture = reference_picture.as_deref().map(|png| pixels_hash(png));
    if reference_picture.is_none() {
        let _ = fs::remove_file(dir.join(crate::reference_window::REFERENCE_PICTURE));
    }
    for (id, _) in &files {
        // its pixels' hash isn't known: the next save writes it again
        saved.layers.remove(id);
    }
    // layer files the document no longer has (and any left beside by a save cut short), now that
    // the document saying so is in place
    saved.layers.retain(|id, _| file.document.layer(*id).is_some());
    if let Ok(entries) = fs::read_dir(dir.join("layers")) {
        let keep: Vec<PathBuf> = file.document.layer_ids().map(|id| layer_path(dir, id)).collect();
        for entry in entries.flatten() {
            let path = entry.path();
            if !keep.contains(&path) {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(())
}

#[uniffi::export]
impl CanvasSession {
    /// A new artwork `width` × `height` at `dpi`, its background colour (sRGB 0..1, three values)
    /// and whether it shows, and its name: a custom canvas.
    #[uniffi::constructor]
    pub fn new_custom(
        width: u32,
        height: u32,
        dpi: f32,
        background: Vec<f32>,
        background_visible: bool,
        name: String,
    ) -> Arc<CanvasSession> {
        let most = procraft_engine::document::canvas_edge_most();
        let mut doc = Document::new(width.clamp(1, most), height.clamp(1, most));
        doc.dpi = dpi;
        if let [r, g, b] = background[..] {
            doc.background = [r, g, b];
        }
        doc.background_visible = background_visible;
        let name = if name.trim().is_empty() {
            "Untitled Artwork".to_owned()
        } else {
            name.trim().chars().take(256).collect()
        };
        CanvasSession::with_document(doc.repaired(), name, Vec::new())
    }

    /// A new artwork the size of `image` (straight RGBA8), the picture as its one layer; scaled
    /// down to fit if it is larger than a canvas may be here.
    #[uniffi::constructor]
    pub fn from_image(image: crate::layers::ThumbnailImage, name: String) -> Arc<CanvasSession> {
        let (sw, sh) = (image.width.max(1), image.height.max(1));
        let mut pixels = image.rgba;
        pixels.resize(sw as usize * sh as usize * 4, 0);
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = u16::from(px[3]);
            for c in px.iter_mut().take(3) {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
        let most = procraft_engine::document::canvas_edge_most();
        let (w, h) = if sw.max(sh) > most {
            let k = most as f32 / sw.max(sh) as f32;
            let side = |v: u32| ((v as f32 * k).round() as u32).clamp(1, most);
            let (w, h) = (side(sw), side(sh));
            let to_old = procraft_engine::geom::Affine {
                a: sw as f32 / w as f32,
                d: sh as f32 / h as f32,
                ..procraft_engine::geom::Affine::IDENTITY
            };
            pixels = procraft_engine::crop::resample(
                &pixels,
                sw,
                sh,
                &procraft_engine::crop::Crop { width: w, height: h, to_old },
            );
            (w, h)
        } else {
            (sw, sh)
        };
        let doc = Document::new(w, h);
        let layer = doc.current();
        let name = if name.trim().is_empty() {
            "Untitled Artwork".to_owned()
        } else {
            name.trim().chars().take(256).collect()
        };
        CanvasSession::with_document(doc, name, vec![(layer, pixels)])
    }

    /// Opens an artwork saved with [`CanvasSession::save`].
    #[uniffi::constructor]
    pub fn open(dir: String) -> Result<Arc<CanvasSession>, MobileError> {
        let dir = PathBuf::from(dir);
        let text = fs::read_to_string(dir.join("artwork.json")).map_err(|e| file_error("reading the artwork", e))?;
        let file: ArtworkFile = serde_json::from_str(&text).map_err(|e| file_error("reading the artwork", e))?;
        if file.format != FORMAT || file.version > VERSION {
            return Err(MobileError::File {
                reason: format!("not an artwork this version can open ({} {})", file.format, file.version),
            });
        }
        let doc = file.document.repaired();
        // each layer's file as it is, decoded as it goes to the GPU
        let mut pngs = Vec::new();
        for layer in doc.layers() {
            let path = layer_path(&dir, layer.id);
            // a layer with no file is empty
            if !path.exists() {
                continue;
            }
            match fs::read(&path) {
                Ok(bytes) => pngs.push((layer.id, Opened::Png(bytes))),
                Err(e) => log::error!("layer {:?}: {e}", layer.id),
            }
        }
        let scene = file.model.as_ref().map(|m| crate::scene3d::open_saved_model(&dir, m)).transpose()?;
        let session = CanvasSession::with_document(doc, file.name, Vec::new());
        // its files are as the last save left them: one unchanged since isn't written again
        let reference_picture = fs::read(dir.join(crate::reference_window::REFERENCE_PICTURE)).ok();
        {
            let mut saving = session.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
            saving.opened(&dir);
            saving.reference_picture = reference_picture.as_deref().map(pixels_hash);
        }
        {
            let mut st = session.shared.lock();
            st.pending_pixels = pngs;
            st.info = file.info;
            st.saved_view = file.view;
            st.scene = scene;
            st.reference_window = file.reference.unwrap_or_default();
            st.reference_picture = reference_picture.map(Arc::new);
            // a plugin's artwork: its kind and data, for the app to open it with its plugin
            st.plugin_opened = file.plugin.map(|kind| crate::plugin_artwork::PluginOpened {
                kind,
                data: fs::read(dir.join(PLUGIN_DATA)).ok(),
                layer: file.plugin_layer,
            });
        }
        Ok(session)
    }

    /// Saves the artwork into `dir` (made if missing): its document, every layer and a thumbnail.
    /// Saves started together run one after the other. An adjustment or transform still open (the
    /// app going into the background) stays open: its layers are saved as they show, and again
    /// once it ends.
    pub fn save(&self, dir: String) -> Result<(), MobileError> {
        let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
        let (snapshot, edits) = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            st.finish_stroke();
            // a model saves its colour as the artwork's document
            let known = saved.known(Path::new(&dir));
            (st.with_colour(|st| st.save_snapshot(Path::new(&dir), &known))?, st.edits())
        };
        write_or_keep_changed(self, &mut saved, Path::new(&dir), snapshot)?;
        self.shared.edits_saved.store(edits, std::sync::atomic::Ordering::Relaxed);
        // saves work again: the autosave's next failure is told
        self.shared.save_failure.lock().unwrap_or_else(|e| e.into_inner()).1 = false;
        Ok(())
    }

    /// Saves as [`CanvasSession::save`] does, but only when nothing is under way that saving
    /// would end (a stroke, a transform, an adjustment, text being edited...); false if it didn't.
    pub fn save_if_idle(&self, dir: String) -> Result<bool, MobileError> {
        let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
        self.save_if_idle_with(&mut saved, &dir)
    }

    /// A number that changes whenever the artwork does (new steps, undo and redo, layer
    /// settings): unchanged, there is nothing new to save.
    pub fn edits(&self) -> u64 {
        self.shared.lock().edits()
    }

    /// [`CanvasSession::edits`] as the last save that worked found them; none before one.
    pub fn edits_saved(&self) -> Option<u64> {
        Some(self.shared.edits_saved.load(std::sync::atomic::Ordering::Relaxed)).filter(|&e| e != NOT_SAVED)
    }

    /// Keeps the artwork saved into `dir` while it is open, from a thread of its own: every few
    /// seconds, when it has changed, as soon as nothing is under way that saving would cut short
    /// (only the layers that changed are written). Ends with [`CanvasSession::stop_autosave`],
    /// with another start, when the session is let go, or with the GPU's device: what that held
    /// can't be read, the last save stands, and [`CanvasSession::take_failure`] tells it.
    pub fn start_autosave(self: Arc<Self>, dir: String) {
        self.keep_saved(dir, AUTOSAVE_EVERY, AUTOSAVE_AGAIN);
    }

    /// Ends the autosave started last: a save it is making is finished first, and none follows.
    /// Why the autosave failed, once until a save works and another fails (for the app to tell,
    /// as [`save_failure_said`] puts it); none if it hasn't. Never waits on a save.
    pub fn take_save_failure(&self) -> Option<String> {
        self.shared.save_failure.lock().unwrap_or_else(|e| e.into_inner()).0.take()
    }

    pub fn stop_autosave(&self) {
        let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
        saved.autosave = saved.autosave.wrapping_add(1);
    }

    /// Gives an artwork opened only to be exported (never shown) a GPU device of its own, so its
    /// exports work; false if there is none.
    pub fn prepare_offscreen(&self) -> bool {
        let mut st = self.shared.lock();
        if st.gpu.is_none() {
            match procraft_gpu::Gpu::headless() {
                Ok(gpu) => st.gpu = Some(gpu),
                Err(e) => {
                    log::error!("exporting offscreen: {e}");
                    return false;
                }
            }
        }
        st.ensure_canvas();
        st.canvas.is_some()
    }

    /// How far the export under way has come, 0 to 1 (read while it runs, without waiting on it):
    /// the layers read back take it to 0.7, the file made to 0.95, written to 1.
    pub fn export_progress(&self) -> f32 {
        f32::from_bits(self.shared.export_progress.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Starts the count again before an export (those that don't tell their progress stay at 0).
    pub fn reset_export_progress(&self) {
        self.exported(0.0);
    }

    pub(crate) fn exported(&self, done: f32) {
        self.shared.export_progress.store(done.clamp(0.0, 1.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Writes the flattened picture, full size and with straight alpha, to `path` as a PNG.
    pub fn export_png(&self, path: String) -> Result<(), MobileError> {
        let (w, h, straight, icc) = self.finished_picture()?;
        self.exported(0.7);
        write_atomically(Path::new(&path), |p| write_png(p, w, h, &straight, &icc))?;
        self.exported(1.0);
        Ok(())
    }

    /// Writes the flattened picture, full size and onto white, to `path` as a JPEG, with the
    /// canvas's colour profile.
    pub fn export_jpeg(&self, path: String) -> Result<(), MobileError> {
        let (w, h, straight, icc) = self.finished_picture()?;
        self.exported(0.7);
        let bytes = procraft_engine::jpeg::encode_tagged(&onto_white(&straight), w, h, JPEG_QUALITY, &icc);
        self.exported(0.95);
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }

    /// Opens a layered artwork package (as the Share menu's first row writes) from `path`.
    #[uniffi::constructor]
    pub fn open_package(path: String) -> Result<Arc<CanvasSession>, MobileError> {
        let bytes = fs::read(&path).map_err(|e| file_error("opening the file", e))?;
        let imported =
            procraft_engine::native::read(&bytes).map_err(|e| file_error("opening the file", format!("{e:?}")))?;
        let session = CanvasSession::with_document(imported.document, imported.name, imported.pixels);
        {
            let mut st = session.shared.lock();
            st.info.strokes = imported.strokes;
            st.info.tracked_ms = (imported.tracked_seconds * 1000.0) as u64;
            st.info.author = imported.author;
            st.reference_window = imported.reference;
            st.reference_picture =
                imported.reference_picture.map(|(png, turn)| Arc::new(crate::reference_window::upright(png, turn)));
        }
        Ok(session)
    }

    /// The artwork as a layered package at `path`, with its time-lapse; `seed` makes its layers' ids.
    pub fn export_package(
        &self,
        path: String,
        seed: u64,
        time_lapse: Option<PackageTimeLapse>,
    ) -> Result<(), MobileError> {
        let time_lapse = match time_lapse {
            Some(t) => {
                let mut segments = Vec::with_capacity(t.segments.len());
                for s in &t.segments {
                    segments.push(fs::read(s).map_err(|e| file_error("reading the time-lapse", e))?);
                }
                Some(procraft_engine::native::TimeLapse {
                    recording: t.recording,
                    quality: t.quality,
                    resolution: t.resolution,
                    hevc: t.hevc,
                    segments,
                    frame_size: (t.width, t.height),
                    fps: t.fps,
                    bitrate: t.bitrate,
                })
            }
            None => None,
        };
        self.exported(0.0);
        let (doc, pixels, composite, name, info, reference, reference_picture) = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
                return Err(file_error("reading the picture", "the canvas is not shown yet"));
            };
            // the file's preview leaves private layers out
            let private = st.doc.private_layers();
            let composite = canvas
                .read_composite_without(gpu, &mut st.doc, &private)
                .map_err(|e| file_error("reading the picture", e))?;
            let mut pixels = std::collections::HashMap::new();
            let count = st.doc.layers().len().max(1);
            for (i, l) in st.doc.layers().iter().enumerate() {
                if l.kind != LayerKind::Group {
                    pixels.insert(l.id, canvas.read_layer(gpu, l.id).map_err(|e| file_error("reading a layer", e))?);
                }
                self.exported(0.1 + 0.6 * (i + 1) as f32 / count as f32);
            }
            let reference = (st.reference_window.clone(), st.reference_picture.clone());
            st.count_tracked_time();
            (st.doc.clone(), pixels, composite, st.name.clone(), st.info.clone(), reference.0, reference.1)
        };
        let (w, h) = (doc.width(), doc.height());
        let (tw, th, mut thumb) = thumbnail(&straight_alpha(&composite), w, h, PACKAGE_PREVIEW_SIDE);
        if let Some(table) = proof_table(&doc) {
            table.apply_rgba(&mut thumb, false);
        }
        let preview = png_bytes(tw, th, &thumb, &doc.picture_icc())?;
        let bytes = procraft_engine::native::write(&procraft_engine::native::Export {
            document: &doc,
            pixels: &pixels,
            composite: &composite,
            thumbnail_png: &preview,
            name: &name,
            author: &info.author,
            strokes: info.strokes,
            tracked_seconds: info.tracked_ms as f64 / 1000.0,
            seed,
            time_lapse: time_lapse.as_ref(),
            reference: Some(&reference),
            reference_picture: reference_picture.as_deref().map(Vec::as_slice),
        });
        self.exported(0.95);
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }

    /// The artwork as a layered PSD (groups, masks, clipping, blend modes kept) at `path`.
    pub fn export_psd(&self, path: String) -> Result<(), MobileError> {
        let p = self.pictures(true)?;
        let layers = psd_layers(&p);
        let (icc, inks) = file_colours(&p.doc);
        let bytes = formats::psd(p.width, p.height, p.doc.dpi, &icc, &layers, &p.composite, inks.as_ref());
        self.exported(0.95);
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }

    /// The flattened picture as a TIFF at `path`.
    pub fn export_tiff(&self, path: String) -> Result<(), MobileError> {
        let p = self.pictures(false)?;
        let bytes = match file_colours(&p.doc) {
            (icc, Some(inks)) => formats::tiff_cmyk(p.width, p.height, p.doc.dpi, &icc, &p.composite, &inks),
            (icc, None) => formats::tiff(p.width, p.height, p.doc.dpi, &icc, &p.composite),
        };
        self.exported(0.95);
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }

    /// The picture as a one-page PDF, or (`layers`) a page for each layer, bottom first, at `path`.
    /// The flattened picture, or each layer (`layers`), as a page of a PDF at `path`: exact at
    /// `quality` 100, JPEG below it.
    pub fn export_pdf(&self, path: String, layers: bool, quality: u8) -> Result<(), MobileError> {
        let p = self.pictures(layers)?;
        let pages: Vec<&[u8]> = if layers {
            p.doc.layers().iter().filter_map(|l| p.layers.get(&l.id)).map(Vec::as_slice).collect()
        } else {
            vec![p.composite.as_slice()]
        };
        let (icc, inks) = file_colours(&p.doc);
        let bytes = formats::pdf(p.width, p.height, p.doc.dpi, &icc, &pages, quality.min(100), inks.as_ref());
        self.exported(0.95);
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }

    /// Each layer as a PNG in `dir` (made if missing), bottom first, named after it; returns
    /// their paths.
    pub fn export_layer_pngs(&self, dir: String) -> Result<Vec<String>, MobileError> {
        self.write_layer_pngs(dir, |_, _| true)
    }

    /// The current and picked layers (a group: the layers in it) each as a PNG in folder `dir`;
    /// returns the files' paths.
    pub fn export_chosen_pngs(&self, dir: String) -> Result<Vec<String>, MobileError> {
        self.write_layer_pngs(dir, |doc, id| doc.chosen().iter().any(|c| *c == id || doc.is_inside(id, *c)))
    }

    pub fn name(&self) -> String {
        self.shared.lock().name.clone()
    }

    /// When the artwork was made, its strokes, the time spent on it (up to now) and its author.
    pub fn artwork_info(&self) -> ArtworkInfo {
        let mut st = self.shared.lock();
        st.count_tracked_time();
        st.info.clone()
    }

    /// Time on the artwork counts while `on` (it is shown and the app in front), from now; off,
    /// what was counted is added to the artwork's. Each save and [`Self::artwork_info`] takes the
    /// time counted so far. (Android counts it itself, by [`Self::add_tracked_time`].)
    pub fn set_tracking(&self, on: bool) {
        let mut st = self.shared.lock();
        st.count_tracked_time();
        if !on {
            st.tracking_since = None;
        } else if st.tracking_since.is_none() {
            st.tracking_since = Some(std::time::Instant::now());
        }
    }

    pub fn set_author(&self, name: String) {
        self.shared.lock().info.author = name.trim().chars().take(256).collect();
    }

    /// Adds time spent on the artwork (at most a day at once), milliseconds.
    pub fn add_tracked_time(&self, ms: u64) {
        let mut st = self.shared.lock();
        st.info.tracked_ms = st.info.tracked_ms.saturating_add(ms.min(24 * 3600 * 1000));
    }

    pub fn set_name(&self, name: String) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.shared.lock().name = name.chars().take(256).collect();
        true
    }
}

/// The signature kept in the artwork's folder `dir` (about/signature.json): its strokes, each the
/// x, y of its points across the signature's box (0..1); none if there is none.
#[uniffi::export]
pub fn read_signature(dir: String) -> Vec<Vec<f32>> {
    let file = Path::new(&dir).join("about").join("signature.json");
    let read: Option<Vec<Vec<f32>>> = fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    read.unwrap_or_default()
}

/// Keeps `strokes` as the signature in the artwork's folder `dir` (see [`read_signature`]).
#[uniffi::export]
pub fn write_signature(dir: String, strokes: Vec<Vec<f32>>) -> Result<(), MobileError> {
    let about = Path::new(&dir).join("about");
    fs::create_dir_all(&about).map_err(|e| file_error("keeping the signature", e))?;
    let json = serde_json::to_string(&strokes).map_err(|e| file_error("keeping the signature", e))?;
    write_atomically(&about.join("signature.json"), |p| {
        fs::write(p, &json).map_err(|e| file_error("keeping the signature", e))
    })
}

/// Keeps the picture `bytes` as the author's photo in the artwork's folder `dir`, in the old
/// one's place only once written whole.
#[uniffi::export]
pub fn write_photo(dir: String, bytes: Vec<u8>) -> Result<(), MobileError> {
    let about = Path::new(&dir).join("about");
    fs::create_dir_all(&about).map_err(|e| file_error("keeping the photo", e))?;
    write_atomically(&about.join("photo.jpg"), |p| fs::write(p, &bytes).map_err(|e| file_error("keeping the photo", e)))
}

/// The table showing a CMYK canvas's colours as its press prints them (in Display P3, as its
/// pictures carry), if it is one.
fn proof_table(doc: &Document) -> Option<procraft_engine::cms::Lut> {
    use procraft_engine::cms::{Lut, builtin};
    let press = doc.press()?;
    Some(Lut::build(&doc.rgb_profile(), &builtin(procraft_engine::profile::Profile::DisplayP3), Some(&press), 33))
}

/// What a layered or print file of the canvas carries: the profile, and for a CMYK canvas the
/// table to its press's inks.
fn file_colours(doc: &Document) -> (Vec<u8>, Option<procraft_engine::cms::Lut>) {
    match doc.press() {
        Some(press) => {
            let inks = procraft_engine::cms::Lut::build(&doc.rgb_profile(), &press, None, 33);
            (press.bytes.to_vec(), Some(inks))
        }
        None => (doc.picture_icc(), None),
    }
}

/// What an export reads from the canvas, straight alpha: the flattened picture, and (if asked
/// for) every paint layer's pixels, every mask's grey and the background colour as a layer.
struct Pictures {
    width: u32,
    height: u32,
    doc: Document,
    composite: Vec<u8>,
    layers: std::collections::HashMap<LayerId, Vec<u8>>,
    masks: std::collections::HashMap<LayerId, Vec<u8>>,
    background: Vec<u8>,
}

impl CanvasSession {
    /// The layers `wanted` picks out each as a PNG in folder `dir`; returns the files' paths.
    fn write_layer_pngs(
        &self,
        dir: String,
        wanted: impl Fn(&Document, LayerId) -> bool,
    ) -> Result<Vec<String>, MobileError> {
        let p = self.pictures(true)?;
        let icc = p.doc.picture_icc();
        let proof = proof_table(&p.doc);
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).map_err(|e| file_error("making the folder", e))?;
        let mut paths = Vec::new();
        let mut used = std::collections::HashSet::new();
        let count = p.doc.layers().len().max(1);
        for (i, l) in p.doc.layers().iter().enumerate() {
            self.exported(0.7 + 0.3 * i as f32 / count as f32);
            if !wanted(&p.doc, l.id) {
                continue;
            }
            let Some(rgba) = p.layers.get(&l.id) else { continue };
            let base: String =
                l.name.chars().map(|c| if c.is_alphanumeric() || " _-".contains(c) { c } else { '_' }).collect();
            let base = if base.trim().is_empty() { "Layer".to_owned() } else { base.trim().to_owned() };
            let mut name = base.clone();
            let mut n = 2;
            while !used.insert(name.clone()) {
                name = format!("{base} {n}");
                n += 1;
            }
            let path = dir.join(format!("{name}.png"));
            let proofed = proof.as_ref().map(|table| {
                let mut px = rgba.clone();
                table.apply_rgba(&mut px, false);
                px
            });
            write_atomically(&path, |f| write_png(f, p.width, p.height, proofed.as_ref().unwrap_or(rgba), &icc))?;
            paths.push(path.to_string_lossy().into_owned());
        }
        self.exported(1.0);
        Ok(paths)
    }

    fn pictures(&self, with_layers: bool) -> Result<Pictures, MobileError> {
        self.exported(0.0);
        let mut guard = self.shared.lock_finished();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
            return Err(file_error("reading the picture", "the canvas is not shown yet"));
        };
        canvas.render_finished(gpu, &mut st.doc);
        let composite = straight_alpha(&canvas.read_composite(gpu).map_err(|e| file_error("reading the picture", e))?);
        let mut layers = std::collections::HashMap::new();
        let mut masks = std::collections::HashMap::new();
        self.exported(0.1);
        if with_layers {
            let count = st.doc.layers().len().max(1);
            for (i, l) in st.doc.layers().iter().enumerate() {
                self.exported(0.1 + 0.6 * i as f32 / count as f32);
                match l.kind {
                    LayerKind::Paint => {
                        let rgba = canvas.read_layer(gpu, l.id).map_err(|e| file_error("reading a layer", e))?;
                        layers.insert(l.id, straight_alpha(&rgba));
                    }
                    LayerKind::Mask => {
                        let rgba = canvas.read_layer(gpu, l.id).map_err(|e| file_error("reading a mask", e))?;
                        let grey = rgba.as_chunks::<4>().0.iter().map(|&p| procraft_engine::document::mask_grey(p));
                        masks.insert(l.id, grey.collect());
                    }
                    LayerKind::Group => {}
                }
            }
        }
        let [r, g, b] = st.doc.background.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let (w, h) = (st.doc.width(), st.doc.height());
        let background = if with_layers { [r, g, b, 255].repeat(w as usize * h as usize) } else { Vec::new() };
        self.exported(0.7);
        Ok(Pictures { width: w, height: h, doc: st.doc.clone(), composite, layers, masks, background })
    }
}

/// The PSD's layers, top first: groups as a start, their layers and an end; masks with their
/// layers; the background colour, when shown, at the bottom.
fn psd_layers(p: &Pictures) -> Vec<PsdLayer<'_>> {
    let doc = &p.doc;
    let end = || PsdLayer {
        name: "",
        kind: PsdKind::GroupEnd,
        opacity: 1.0,
        visible: true,
        blend: BlendMode::Normal,
        clipped: false,
    };
    let mut out = Vec::new();
    let mut open: Vec<LayerId> = Vec::new();
    for l in doc.layers().iter().rev() {
        if l.kind == LayerKind::Mask {
            continue;
        }
        while let Some(&g) = open.last() {
            if doc.is_inside(l.id, g) {
                break;
            }
            open.pop();
            out.push(end());
        }
        let kind = match l.kind {
            LayerKind::Group => {
                open.push(l.id);
                PsdKind::GroupStart { open: !l.collapsed }
            }
            _ => {
                let Some(rgba) = p.layers.get(&l.id) else { continue };
                let mask = doc
                    .mask_of(l.id)
                    .filter(|m| doc.layer(*m).is_some_and(|m| m.visible))
                    .and_then(|m| p.masks.get(&m));
                PsdKind::Pixels { rgba, mask: mask.map(Vec::as_slice) }
            }
        };
        out.push(PsdLayer {
            name: &l.name,
            kind,
            opacity: l.opacity,
            visible: l.visible,
            blend: l.blend,
            clipped: l.clipped,
        });
    }
    for _ in open {
        out.push(end());
    }
    if doc.background_visible {
        out.push(PsdLayer {
            name: "Background",
            kind: PsdKind::Pixels { rgba: &p.background, mask: None },
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
            clipped: false,
        });
    }
    out
}

fn layer_path(dir: &Path, id: LayerId) -> PathBuf {
    dir.join("layers").join(format!("{}.png", id.0))
}

/// What the apps say when saving an artwork failed for `reason` (a save's error).
#[uniffi::export]
pub fn save_failure_said(reason: String) -> String {
    if reason.to_lowercase().contains("space") {
        "There's no room left to save this artwork.".to_owned()
    } else {
        "This artwork couldn't be saved.".to_owned()
    }
}

/// Writes `path` by writing a temporary file beside it and renaming it into place.
pub(crate) fn write_atomically(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), MobileError>,
) -> Result<(), MobileError> {
    let tmp = write_beside(path, write)?;
    fs::rename(&tmp, path).map_err(|e| file_error("saving", e))
}

/// Writes what goes in `path` into a file beside it (removed again if the write fails), to be
/// renamed over it; the file written.
pub(crate) fn write_beside(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), MobileError>,
) -> Result<PathBuf, MobileError> {
    // a name of its own, so that no other write of the same file can touch it
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.tmp", n));
    if let Err(e) = write(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// A PNG of RGBA8 pixels, carrying the colour profile `icc` (none if empty).
pub(crate) fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8], icc: &[u8]) -> Result<(), MobileError> {
    let bytes = png_bytes(width, height, rgba, icc)?;
    fs::write(path, bytes).map_err(|e| file_error("saving", e))
}

/// A PNG of premultiplied RGBA8 pixels (as the GPU gives them), none on failure but empty.
pub(crate) fn rgba_png(width: u32, height: u32, premultiplied: &[u8]) -> Vec<u8> {
    png_bytes(width, height, &straight_alpha(premultiplied), &[]).unwrap_or_default()
}

/// A PNG of straight RGBA8 pixels, carrying `icc` if it isn't empty.
fn png_bytes(width: u32, height: u32, rgba: &[u8], icc: &[u8]) -> Result<Vec<u8>, MobileError> {
    let mut out = Vec::new();
    let mut info = png::Info::with_size(width, height);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Eight;
    if !icc.is_empty() {
        info.icc_profile = Some(std::borrow::Cow::Borrowed(icc));
    }
    {
        let mut encoder = png::Encoder::with_info(&mut out, info).map_err(|e| file_error("saving", e))?;
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|e| file_error("saving", e))?;
        writer.write_image_data(rgba).map_err(|e| file_error("saving", e))?;
        writer.finish().map_err(|e| file_error("saving", e))?;
    }
    Ok(out)
}

/// A layer's pixels waiting for the GPU: as they are, or (an opened artwork's) still the PNG they
/// were read from, decoded only as each goes up, so a large artwork isn't all in memory at once.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Opened {
    Pixels(Vec<u8>),
    Png(Vec<u8>),
}

impl Opened {
    /// The pixels, `width` × `height`: a layer's file of another size (one a crop's save left as
    /// it was) comes in from its top left corner, cut or with clear pixels to fill, rather than
    /// be lost.
    pub(crate) fn pixels(self, width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
        match self {
            Opened::Pixels(rgba) => Ok(rgba),
            Opened::Png(bytes) => decode_png_into(std::io::Cursor::new(bytes), width, height, true),
        }
    }
}

/// Layers' pixels as they are, waiting for the GPU.
pub(crate) fn opened(layers: Vec<(LayerId, Vec<u8>)>) -> Vec<(LayerId, Opened)> {
    layers.into_iter().map(|(id, rgba)| (id, Opened::Pixels(rgba))).collect()
}

/// Waiting layers' pixels, `width` × `height`; one that can't be read is left out.
pub(crate) fn opened_pixels(layers: Vec<(LayerId, Opened)>, width: u32, height: u32) -> Vec<(LayerId, Vec<u8>)> {
    layers
        .into_iter()
        .filter_map(|(id, layer)| match layer.pixels(width, height) {
            Ok(rgba) => Some((id, rgba)),
            Err(e) => {
                log::error!("layer {id:?}: {e}");
                None
            }
        })
        .collect()
}

/// A PNG's RGBA8 pixels, which must be `width` × `height`.
pub(crate) fn read_png(path: &Path, width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
    let file = fs::File::open(path).map_err(|e| file_error("opening a layer", e))?;
    decode_png(BufReader::new(file), width, height)
}

/// The RGBA8 pixels of the PNG `from` reads, which must be `width` × `height`.
fn decode_png(from: impl std::io::BufRead + std::io::Seek, width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
    decode_png_into(from, width, height, false)
}

/// As [`decode_png`]; `fit`, a PNG of another size is taken from its top left corner into
/// `width` × `height` (cut, or clear where it is smaller).
fn decode_png_into(
    from: impl std::io::BufRead + std::io::Seek,
    width: u32,
    height: u32,
    fit: bool,
) -> Result<Vec<u8>, MobileError> {
    let decoder = png::Decoder::new(from);
    let mut reader = decoder.read_info().map_err(|e| file_error("opening a layer", e))?;
    let (color, depth) = reader.output_color_type();
    let (w, h, interlaced) = (reader.info().width, reader.info().height, reader.info().interlaced);
    let sized = w == width && h == height;
    if (!sized && !fit) || color != png::ColorType::Rgba || depth != png::BitDepth::Eight {
        return Err(file_error("opening a layer", format!("{w}×{h} {color:?}")));
    }
    if !sized {
        log::warn!("a layer of {w}×{h} opened into {width}×{height}, from its top left");
    }
    let line = width as usize * 4;
    let mut buf = vec![0; line * height as usize];
    if interlaced {
        // not one of ours: whole or not at all
        let mut whole = vec![0; w as usize * 4 * h as usize];
        reader.next_frame(&mut whole).map_err(|e| file_error("opening a layer", e))?;
        for (row, from) in buf.chunks_mut(line).zip(whole.chunks(w as usize * 4)) {
            for (to, from) in row.iter_mut().zip(from) {
                *to = *from;
            }
        }
        return Ok(buf);
    }
    // row by row: one damaged part way keeps the rows before the damage (the rest clear) rather
    // than the whole layer being lost
    for (y, row) in buf.chunks_mut(line).enumerate().take(h as usize) {
        match reader.next_row() {
            Ok(Some(read)) => {
                for (to, from) in row.iter_mut().zip(read.data()) {
                    *to = *from;
                }
            }
            Ok(None) => break,
            Err(e) if y == 0 => return Err(file_error("opening a layer", e)),
            Err(e) => {
                log::error!("a layer read as far as row {y}: {e}");
                break;
            }
        }
    }
    Ok(buf)
}

/// Premultiplied RGBA8 pixels with straight alpha instead.
impl CanvasSession {
    /// The flattened picture as it is exported, full size: its width, height, pixels (straight
    /// RGBA8; a CMYK canvas's as its press prints them) and the colour profile they are in.
    fn finished_picture(&self) -> Result<(u32, u32, Vec<u8>, Vec<u8>), MobileError> {
        self.exported(0.0);
        let (w, h, rgba, icc) = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let (w, h) = (st.doc.width(), st.doc.height());
            let icc = st.doc.picture_icc();
            let proof = proof_table(&st.doc);
            let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
                return Err(file_error("reading the picture", "the canvas is not shown yet"));
            };
            canvas.render_finished(gpu, &mut st.doc);
            let rgba = canvas.read_composite(gpu).map_err(|e| file_error("reading the picture", e))?;
            (w, h, rgba, (icc, proof))
        };
        let (icc, proof) = icc;
        let mut straight = straight_alpha(&rgba);
        if let Some(table) = proof {
            table.apply_rgba(&mut straight, false);
        }
        Ok((w, h, straight, icc))
    }
}

/// What an artwork called `name` is called as a file it is exported to, before the extension:
/// its letters, digits, spaces, `_` and `-` (the rest left out, the ends trimmed), or "Untitled
/// Artwork" with none left; a layered package's (`package`) with underscores for its spaces.
#[uniffi::export]
pub fn export_file_name(name: String, package: bool) -> String {
    let kept: String = name.chars().filter(|c| c.is_alphanumeric() || " _-".contains(*c)).collect();
    let base = match kept.trim() {
        "" => "Untitled Artwork",
        k => k,
    };
    if package { base.replace(' ', "_") } else { base.to_owned() }
}

/// How finely JPEGs are written (1..100): fine enough to keep a painting's edges and grain.
pub(crate) const JPEG_QUALITY: u8 = 92;

/// Straight RGBA8 pixels laid onto white, as RGB8 (for a JPEG, which has nothing see-through).
pub(crate) fn onto_white(rgba: &[u8]) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks::<4>();
    let mut out = Vec::with_capacity(pixels.len() * 3);
    for &[r, g, b, a] in pixels {
        let a = u16::from(a);
        let over = |c: u8| ((u16::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
        out.extend_from_slice(&[over(r), over(g), over(b)]);
    }
    out
}

pub(crate) fn straight_alpha(rgba: &[u8]) -> Vec<u8> {
    // in pieces made side by side
    let pieces: Vec<&[u8]> = rgba.chunks(1 << 20).collect();
    let made = procraft_engine::par::side_by_side(&pieces, |piece| straight_piece(piece));
    let mut out = Vec::with_capacity(rgba.len());
    for (piece, made) in pieces.iter().zip(made) {
        out.extend_from_slice(&made.unwrap_or_else(|| straight_piece(piece)));
    }
    out
}

/// [`straight_alpha`] of one piece (whole pixels).
fn straight_piece(rgba: &[u8]) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks::<4>();
    let mut out = Vec::with_capacity(rgba.len());
    for &[r, g, b, a] in pixels {
        let alpha = u32::from(a);
        let un = |c: u8| (u32::from(c) * 255 + alpha / 2).checked_div(alpha).map_or(0, |v| v.min(255) as u8);
        out.extend_from_slice(&[un(r), un(g), un(b), a]);
    }
    out
}

/// How bringing in a document's pages went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PagesOutcome {
    Imported,
    /// More pages than a canvas their size holds as layers (`most`).
    TooManyPages {
        most: u32,
    },
    /// A page couldn't be read.
    Unreadable,
}

/// Makes an artwork named `name` in the folder `dir` from `count` pages, each a `width` x `height`
/// PNG `page-N.png` (N from 1) in the folder `pages`: a layer each, "Page N", the first at the
/// bottom, with Page Assist on and `dpi` its resolution. Used for a PDF's pages.
#[uniffi::export]
pub fn import_pages(
    pages: String,
    count: u32,
    width: u32,
    height: u32,
    dpi: f32,
    dir: String,
    name: String,
) -> PagesOutcome {
    use procraft_engine::document::Layer;
    let most = procraft_engine::document::canvas_edge_most();
    let (w, h) = (width.clamp(1, most), height.clamp(1, most));
    let most = Document::new(w, h).layer_cap();
    if count as usize > most {
        return PagesOutcome::TooManyPages { most: u32::try_from(most).unwrap_or(u32::MAX) };
    }
    let mut layers = Vec::new();
    let mut pixels = Vec::new();
    for n in 1..=count.max(1) {
        let id = LayerId(u64::from(n));
        match read_png(&Path::new(&pages).join(format!("page-{n}.png")), w, h) {
            Ok(rgba) => pixels.push((id, rgba)),
            Err(e) => {
                log::error!("a page: {e}");
                return PagesOutcome::Unreadable;
            }
        }
        layers.push(Layer {
            id,
            name: format!("Page {n}"),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            alpha_lock: false,
            clipped: false,
            locked: false,
            reference: false,
            kind: LayerKind::Paint,
            parent: None,
            collapsed: false,
            text: None,
            private: false,
            assisted: false,
        });
    }
    let mut doc = Document::from_layers(w, h, layers, LayerId(1));
    if dpi.is_finite() && dpi > 0.0 {
        doc.dpi = dpi.clamp(1.0, 9999.0);
    }
    doc.animation = Some(procraft_engine::animation::Animation { pages: true, ..Default::default() });
    let name =
        if name.trim().is_empty() { "Untitled Artwork".to_owned() } else { name.trim().chars().take(256).collect() };
    // the first page is the gallery's thumbnail until the artwork is opened
    let first = pixels.first().map(|(_, p)| thumbnail(p, w, h, THUMBNAIL_SIDE));
    let session = CanvasSession::with_document(doc, name, pixels);
    if let Err(e) = session.save(dir.clone()) {
        log::error!("saving imported pages: {e}");
        return PagesOutcome::Unreadable;
    }
    if let Some((tw, th, thumb)) = first
        && let Err(e) = write_atomically(&Path::new(&dir).join("thumbnail.png"), |p| write_png(p, tw, th, &thumb, &[]))
    {
        log::error!("imported pages' thumbnail: {e}");
    }
    PagesOutcome::Imported
}

/// How bringing in a PSD went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PsdOutcome {
    Imported,
    /// More layers than a canvas its size holds (`most`).
    TooManyLayers {
        most: u32,
    },
    /// A kind of PSD not read here (bitmap, indexed, Lab, 32 bits a channel).
    Unsupported,
    /// Not a PSD, or damaged.
    Unreadable,
}

/// The time-lapse the package at `path` keeps, its segments written into the folder `dir` as
/// `segment-N.mp4` (N from 1, in order); none if it keeps none.
#[uniffi::export]
pub fn package_time_lapse(path: String, dir: String) -> Result<Option<PackageTimeLapse>, MobileError> {
    let bytes = fs::read(&path).map_err(|e| file_error("opening the file", e))?;
    let Some(t) = procraft_engine::native::read_time_lapse(&bytes)
        .map_err(|e| file_error("opening the file", format!("{e:?}")))?
    else {
        return Ok(None);
    };
    let dir = Path::new(&dir);
    if !t.segments.is_empty() {
        fs::create_dir_all(dir).map_err(|e| file_error("saving the time-lapse", e))?;
    }
    let mut segments = Vec::with_capacity(t.segments.len());
    for (i, data) in t.segments.iter().enumerate() {
        let file = dir.join(format!("segment-{}.mp4", i + 1));
        fs::write(&file, data).map_err(|e| file_error("saving the time-lapse", e))?;
        segments.push(file.to_string_lossy().into_owned());
    }
    Ok(Some(PackageTimeLapse {
        recording: t.recording,
        quality: t.quality,
        resolution: t.resolution,
        hevc: t.hevc,
        segments,
        width: t.frame_size.0,
        height: t.frame_size.1,
        fps: t.fps,
        bitrate: t.bitrate,
    }))
}

/// Makes the layered PSD (or PSB) at `path` an artwork named `name` in the folder `dir`: its
/// layers, groups and masks, and its flattened picture as the gallery's thumbnail until opened.
#[uniffi::export]
pub fn import_psd(path: String, dir: String, name: String) -> PsdOutcome {
    use procraft_engine::psd::{self, PsdError};
    let Ok(bytes) = fs::read(&path) else { return PsdOutcome::Unreadable };
    let imported = match psd::read(&bytes) {
        Ok(i) => i,
        Err(PsdError::TooManyLayers(most)) => {
            return PsdOutcome::TooManyLayers { most: u32::try_from(most).unwrap_or(u32::MAX) };
        }
        Err(PsdError::Unsupported(_)) => return PsdOutcome::Unsupported,
        Err(e) => {
            log::error!("reading a PSD: {e:?}");
            return PsdOutcome::Unreadable;
        }
    };
    let name = if name.trim().is_empty() { imported.name } else { name };
    let session = CanvasSession::with_document(imported.document, name, imported.pixels);
    if let Err(e) = session.save(dir.clone()) {
        log::error!("saving an imported PSD: {e}");
        return PsdOutcome::Unreadable;
    }
    if let Some((w, h, flat)) = psd::flattened(&bytes) {
        // on the white of a new canvas
        let opaque: Vec<u8> = flat
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| {
                let over = |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a)) + 127) / 255) as u8;
                [over(r), over(g), over(b), 255]
            })
            .collect();
        let (tw, th, thumb) = thumbnail(&opaque, w, h, THUMBNAIL_SIDE);
        if let Err(e) = write_atomically(&Path::new(&dir).join("thumbnail.png"), |p| write_png(p, tw, th, &thumb, &[]))
        {
            log::error!("an imported PSD's thumbnail: {e}");
        }
    }
    PsdOutcome::Imported
}

/// The picture scaled down (box filter) to fit `side`, straight alpha, for viewing elsewhere.
pub(crate) fn thumbnail(rgba: &[u8], width: u32, height: u32, side: u32) -> (u32, u32, Vec<u8>) {
    let scale = (side as f32 / width.max(height).max(1) as f32).min(1.0);
    let (tw, th) = (((width as f32 * scale).round() as u32).max(1), ((height as f32 * scale).round() as u32).max(1));
    // at full size each pixel is only made straight
    let whole = width as usize * height as usize * 4;
    if (tw, th) == (width, height)
        && let Some(all) = rgba.get(..whole)
    {
        return (tw, th, straight_alpha(all));
    }
    // each row averaged on its own, side by side
    let rows: Vec<u32> = (0..th).collect();
    let made = procraft_engine::par::side_by_side(&rows, |&ty| thumbnail_row(rgba, width, height, tw, th, ty));
    let mut out = Vec::with_capacity(tw as usize * th as usize * 4);
    for (&ty, row) in rows.iter().zip(made) {
        out.extend_from_slice(&row.unwrap_or_else(|| thumbnail_row(rgba, width, height, tw, th, ty)));
    }
    (tw, th, out)
}

/// Row `ty` of [`thumbnail`]'s `tw` x `th` picture: each pixel the average of the premultiplied
/// pixels it covers, made straight.
fn thumbnail_row(rgba: &[u8], width: u32, height: u32, tw: u32, th: u32, ty: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((tw * 4) as usize);
    {
        let (y0, y1) = (ty * height / th, ((ty + 1) * height / th).max(ty * height / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * width / tw, ((tx + 1) * width / tw).max(tx * width / tw + 1));
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for y in y0..y1.min(height) {
                for x in x0..x1.min(width) {
                    let i = ((y * width + x) * 4) as usize;
                    if let Some(px) = rgba.get(i..i + 4) {
                        for (s, v) in sum.iter_mut().zip(px) {
                            *s += u32::from(*v);
                        }
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            let [r, g, b, a] = sum.map(|s| s / n);
            // premultiplied to straight
            let un = |c: u32| (c * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
            out.extend_from_slice(&[un(r), un(g), un(b), a as u8]);
        }
    }
    out
}

#[cfg(test)]
mod signature_tests {
    use super::*;

    #[test]
    fn a_layer_file_cut_short_keeps_the_rows_before_the_cut() {
        let (w, h) = (64u32, 64u32);
        // each row its own grey, so a row in the wrong place shows
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let g = (i / w * 4) as u8;
                [g, 255 - g, g, 255]
            })
            .collect();
        let path = std::env::temp_dir().join(format!("procraft-cut-layer-{}.png", std::process::id()));
        write_png(&path, w, h, &rgba, &[]).unwrap();
        let whole = fs::read(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(Opened::Png(whole.clone()).pixels(w, h).unwrap(), rgba);
        // cut short part way: the first rows as they were, the rest clear
        let cut = Opened::Png(whole[..whole.len() * 2 / 3].to_vec()).pixels(w, h).unwrap();
        let line = w as usize * 4;
        let kept = cut.chunks(line).zip(rgba.chunks(line)).take_while(|(a, b)| a == b).count();
        assert!(kept > 0 && kept < h as usize, "{kept} rows kept");
        assert!(cut[kept * line..].iter().all(|&v| v == 0));
        // cut before any row: none can be read
        assert!(Opened::Png(whole[..40].to_vec()).pixels(w, h).is_err());
    }

    #[test]
    fn a_layer_file_of_another_size_opens_from_its_top_left() {
        let (w, h) = (6u32, 4u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % w) as u8 * 40, (i / w) as u8 * 60, 7, 255]).collect();
        let path = std::env::temp_dir().join(format!("procraft-sized-layer-{}.png", std::process::id()));
        write_png(&path, w, h, &rgba, &[]).unwrap();
        let bytes = fs::read(&path).unwrap();
        let _ = fs::remove_file(&path);
        let at = |px: &[u8], cw: u32, x: u32, y: u32| px[((y * cw + x) * 4) as usize..][..4].to_vec();
        // smaller: the top left part
        let cut = Opened::Png(bytes.clone()).pixels(4, 3).unwrap();
        assert_eq!(cut.len(), 4 * 3 * 4);
        for (x, y) in [(0, 0), (3, 0), (0, 2), (3, 2)] {
            assert_eq!(at(&cut, 4, x, y), at(&rgba, w, x, y));
        }
        // bigger: all of it at the top left, clear around it
        let grown = Opened::Png(bytes.clone()).pixels(8, 5).unwrap();
        assert_eq!(at(&grown, 8, 5, 3), at(&rgba, w, 5, 3));
        assert_eq!(at(&grown, 8, 6, 0), vec![0; 4]);
        assert_eq!(at(&grown, 8, 0, 4), vec![0; 4]);
        // anything else read by size still must be that size
        assert!(read_png_bytes_sized(&bytes, 4, 3).is_err());
    }

    fn read_png_bytes_sized(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
        decode_png(std::io::Cursor::new(bytes.to_vec()), width, height)
    }

    #[test]
    fn a_signature_is_kept_as_the_android_app_keeps_it() {
        let dir = std::env::temp_dir().join(format!("procraft-signature-{}", std::process::id()));
        let path = dir.to_string_lossy().into_owned();
        assert!(read_signature(path.clone()).is_empty());
        let strokes = vec![vec![0.1, 0.2, 0.3, 0.4], vec![0.5, 0.5]];
        write_signature(path.clone(), strokes.clone()).unwrap();
        assert_eq!(read_signature(path.clone()), strokes);
        // as the Android app wrote it: an array of arrays of numbers
        fs::write(dir.join("about").join("signature.json"), "[[0.25,0.75,1.0,0.0]]").unwrap();
        assert_eq!(read_signature(path), vec![vec![0.25, 0.75, 1.0, 0.0]]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_photo_and_signature_are_replaced_whole_with_nothing_left_beside() {
        let dir = std::env::temp_dir().join(format!("procraft-about-{}", std::process::id()));
        let path = dir.to_string_lossy().into_owned();
        write_photo(path.clone(), b"first photo".to_vec()).unwrap();
        write_signature(path.clone(), vec![vec![0.1, 0.2]]).unwrap();
        write_photo(path.clone(), b"second".to_vec()).unwrap();
        write_signature(path.clone(), vec![vec![0.3, 0.4, 0.5, 0.6]]).unwrap();
        let about = dir.join("about");
        assert_eq!(fs::read(about.join("photo.jpg")).unwrap(), b"second");
        assert_eq!(read_signature(path), vec![vec![0.3, 0.4, 0.5, 0.6]]);
        let mut left: Vec<String> =
            fs::read_dir(&about).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["photo.jpg", "signature.json"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
