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
/// A package's preview's longest side, pixels.
const PACKAGE_PREVIEW_SIDE: u32 = 1024;
const FORMAT: &str = "procraft-artwork";
const VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct ArtworkFile {
    format: String,
    version: u32,
    name: String,
    document: Document,
    #[serde(default)]
    info: ArtworkInfo,
    /// How it was last shown, if it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view: Option<SavedView>,
    /// The model painted on, if it is one (its current texture set is `document`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<crate::scene3d::SavedModel>,
}

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

/// What a save writes, read under the lock: the artwork's file, the layers not already in the
/// folder as they are (each with its hash) and the picture (if shown).
pub(crate) struct SaveSnapshot {
    /// The layers that had changed, counted as changed again if the save fails.
    changed: Vec<LayerId>,
    file: ArtworkFile,
    layers: Vec<(LayerId, u64, Vec<u8>)>,
    /// Layers still the PNG files they were opened from (not shown yet), written as they are.
    files: Vec<(LayerId, Vec<u8>)>,
    composite: Option<Vec<u8>>,
    model: Option<crate::scene3d::ModelSnapshot>,
}

/// What the last saves wrote: the folder, and a hash of each layer's pixels, so layers that
/// haven't changed aren't written again.
#[derive(Default)]
pub(crate) struct Saved {
    dir: PathBuf,
    layers: std::collections::HashMap<LayerId, u64>,
    sets: crate::scene3d::SavedSets,
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
    /// Nothing under way that saving would end or cut short.
    pub(crate) fn idle(&self) -> bool {
        self.stroke.is_none()
            && self.transform.is_none()
            && !self.adjusting
            && self.text_edit.is_none()
            && self.shape.is_none()
            && self.pending_doc.is_none()
            && self.feather_base.is_none()
            && self.selection_before.is_none()
            && self.scene.as_ref().is_none_or(|s| !s.stroking)
    }

    /// The artwork as it is now, for saving into `dir`: a layer already there as it is (its hash
    /// among the `known` and its file there) is let go as soon as it is read, so a save holds only
    /// the layers that changed.
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
        };
        let mut layers = Vec::new();
        let mut files = Vec::new();
        let mut composite = None;
        let mut changed_ids = Vec::new();
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.render(gpu, &mut self.doc, None);
            let changed_now = canvas.take_changed();
            changed_ids = changed_now.iter().copied().collect();
            for layer in self.doc.layers() {
                // unchanged since it was read or saved, its file there: left as it is, unread
                if !changed_now.contains(&layer.id)
                    && known.contains_key(&layer.id)
                    && layer_path(dir, layer.id).exists()
                {
                    continue;
                }
                let rgba = canvas.read_layer(gpu, layer.id).map_err(|e| file_error("reading a layer", e))?;
                layers.extend(changed(layer.id, rgba));
            }
            // for the gallery's thumbnail: private layers left out
            let private = self.doc.private_layers();
            composite = Some(
                canvas
                    .read_composite_without(gpu, &mut self.doc, &private)
                    .map_err(|e| file_error("reading the picture", e))?,
            );
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
        }
        Ok(SaveSnapshot { changed: changed_ids, file, layers, files, composite, model })
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

/// Writes a save into `dir` (made if missing): the layers that changed since the last save there,
/// the thumbnail and the document.
fn write_artwork(saved: &mut Saved, dir: &Path, snapshot: SaveSnapshot) -> Result<(), MobileError> {
    let SaveSnapshot { file, layers, files, composite, model, .. } = snapshot;
    if saved.dir != dir {
        saved.dir = dir.to_path_buf();
        saved.layers.clear();
        saved.sets.clear();
    }
    let (w, h) = (file.document.width(), file.document.height());
    fs::create_dir_all(dir.join("layers")).map_err(|e| file_error("making the folder", e))?;
    // only those not already there as they are
    for (id, hash, rgba) in &layers {
        write_atomically(&layer_path(dir, *id), |p| write_png(p, w, h, rgba, &[]))?;
        saved.layers.insert(*id, *hash);
    }
    for (id, bytes) in &files {
        write_atomically(&layer_path(dir, *id), |p| fs::write(p, bytes).map_err(|e| file_error("writing a layer", e)))?;
        // its pixels' hash isn't known: the next save writes it again
        saved.layers.remove(id);
    }
    // layer files the document no longer has
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
    if let Some(model) = model.as_ref() {
        crate::scene3d::write_model(&mut saved.sets, dir, model)?;
    }
    // a model's thumbnail is the model as last shown
    if let Some((pw, ph, rgba)) = model.and_then(|m| m.picture) {
        let (tw, th, thumb) = thumbnail(&rgba, pw, ph, THUMBNAIL_SIDE);
        write_atomically(&dir.join("thumbnail.png"), |p| write_png(p, tw, th, &thumb, &[]))?;
    } else if let Some(rgba) = composite {
        let (tw, th, mut thumb) = thumbnail(&rgba, w, h, THUMBNAIL_SIDE);
        // the gallery shows it in the artwork's colours (a CMYK canvas's: as its press prints them)
        if let Some(table) = proof_table(&file.document) {
            table.apply_rgba(&mut thumb, false);
        }
        let icc = file.document.picture_icc();
        write_atomically(&dir.join("thumbnail.png"), |p| write_png(p, tw, th, &thumb, &icc))?;
    }
    let json = serde_json::to_string(&file).map_err(|e| file_error("writing the artwork", e))?;
    write_atomically(&dir.join("artwork.json"), |p| {
        fs::write(p, json.as_bytes()).map_err(|e| file_error("writing the artwork", e))
    })
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
        let mut doc = Document::new(width.max(1), height.max(1));
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

    /// A new artwork the size of `image` (straight RGBA8), the picture as its one layer.
    #[uniffi::constructor]
    pub fn from_image(image: crate::layers::ThumbnailImage, name: String) -> Arc<CanvasSession> {
        let (w, h) = (image.width.max(1), image.height.max(1));
        let doc = Document::new(w, h);
        let layer = doc.current();
        let mut pixels = image.rgba;
        pixels.resize(w as usize * h as usize * 4, 0);
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = u16::from(px[3]);
            for c in px.iter_mut().take(3) {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
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
        session.shared.saving.lock().unwrap_or_else(|e| e.into_inner()).opened(&dir);
        {
            let mut st = session.shared.lock();
            st.pending_pixels = pngs;
            st.info = file.info;
            st.saved_view = file.view;
            st.scene = scene;
        }
        Ok(session)
    }

    /// Saves the artwork into `dir` (made if missing): its document, every layer and a thumbnail.
    /// Saves started together run one after the other.
    pub fn save(&self, dir: String) -> Result<(), MobileError> {
        let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            // a model saves its colour as the artwork's document
            let known = saved.known(Path::new(&dir));
            st.with_colour(|st| st.save_snapshot(Path::new(&dir), &known))?
        };
        write_or_keep_changed(self, &mut saved, Path::new(&dir), snapshot)
    }

    /// Saves as [`CanvasSession::save`] does, but only when nothing is under way that saving
    /// would end (a stroke, a transform, an adjustment, text being edited...); false if it didn't.
    pub fn save_if_idle(&self, dir: String) -> Result<bool, MobileError> {
        let mut saved = self.shared.saving.lock().unwrap_or_else(|e| e.into_inner());
        let snapshot = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            if !st.idle() {
                return Ok(false);
            }
            // a model saves its colour as the artwork's document
            let known = saved.known(Path::new(&dir));
            st.with_colour(|st| st.save_snapshot(Path::new(&dir), &known))?
        };
        write_or_keep_changed(self, &mut saved, Path::new(&dir), snapshot).map(|()| true)
    }

    /// A number that changes whenever the artwork does (new steps, undo and redo, layer
    /// settings): unchanged, there is nothing new to save.
    pub fn edits(&self) -> u64 {
        let st = self.shared.lock();
        st.revision.wrapping_add(st.canvas.as_ref().map_or(0, procraft_gpu::Canvas::steps_made))
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

    /// Writes the flattened picture, full size and with straight alpha, to `path` as a PNG.
    pub fn export_png(&self, path: String) -> Result<(), MobileError> {
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
        write_atomically(Path::new(&path), |p| write_png(p, w, h, &straight, &icc))
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
        let (doc, pixels, composite, name, info) = {
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
            for l in st.doc.layers() {
                if l.kind != LayerKind::Group {
                    pixels.insert(l.id, canvas.read_layer(gpu, l.id).map_err(|e| file_error("reading a layer", e))?);
                }
            }
            (st.doc.clone(), pixels, composite, st.name.clone(), st.info.clone())
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
        });
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))
    }

    /// The artwork as a layered PSD (groups, masks, clipping, blend modes kept) at `path`.
    pub fn export_psd(&self, path: String) -> Result<(), MobileError> {
        let p = self.pictures(true)?;
        let layers = psd_layers(&p);
        let (icc, inks) = file_colours(&p.doc);
        let bytes = formats::psd(p.width, p.height, p.doc.dpi, &icc, &layers, &p.composite, inks.as_ref());
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))
    }

    /// The flattened picture as a TIFF at `path`.
    pub fn export_tiff(&self, path: String) -> Result<(), MobileError> {
        let p = self.pictures(false)?;
        let bytes = match file_colours(&p.doc) {
            (icc, Some(inks)) => formats::tiff_cmyk(p.width, p.height, p.doc.dpi, &icc, &p.composite, &inks),
            (icc, None) => formats::tiff(p.width, p.height, p.doc.dpi, &icc, &p.composite),
        };
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))
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
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))
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

    /// When the artwork was made, its strokes, the time spent on it and its author.
    pub fn artwork_info(&self) -> ArtworkInfo {
        self.shared.lock().info.clone()
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
        for l in p.doc.layers() {
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
        Ok(paths)
    }

    fn pictures(&self, with_layers: bool) -> Result<Pictures, MobileError> {
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
        if with_layers {
            for l in st.doc.layers() {
                match l.kind {
                    LayerKind::Paint => {
                        let rgba = canvas.read_layer(gpu, l.id).map_err(|e| file_error("reading a layer", e))?;
                        layers.insert(l.id, straight_alpha(&rgba));
                    }
                    LayerKind::Mask => {
                        let rgba = canvas.read_layer(gpu, l.id).map_err(|e| file_error("reading a mask", e))?;
                        masks.insert(l.id, rgba.as_chunks::<4>().0.iter().map(|&[r, ..]| r).collect());
                    }
                    LayerKind::Group => {}
                }
            }
        }
        let [r, g, b] = st.doc.background.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let (w, h) = (st.doc.width(), st.doc.height());
        let background = if with_layers { [r, g, b, 255].repeat(w as usize * h as usize) } else { Vec::new() };
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

/// Writes `path` by writing a temporary file beside it and renaming it into place.
pub(crate) fn write_atomically(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), MobileError>,
) -> Result<(), MobileError> {
    // a name of its own, so that no other write of the same file can touch it
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.tmp", n));
    write(&tmp)?;
    fs::rename(&tmp, path).map_err(|e| file_error("saving", e))
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
    /// The pixels, which must be `width` × `height`.
    pub(crate) fn pixels(self, width: u32, height: u32) -> Result<Vec<u8>, MobileError> {
        match self {
            Opened::Pixels(rgba) => Ok(rgba),
            Opened::Png(bytes) => decode_png(std::io::Cursor::new(bytes), width, height),
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
    let decoder = png::Decoder::new(from);
    let mut reader = decoder.read_info().map_err(|e| file_error("opening a layer", e))?;
    let size = reader.output_buffer_size().ok_or_else(|| file_error("opening a layer", "too large"))?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| file_error("opening a layer", e))?;
    if info.width != width
        || info.height != height
        || info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
    {
        return Err(file_error("opening a layer", format!("{}×{} {:?}", info.width, info.height, info.color_type)));
    }
    buf.truncate(info.buffer_size());
    Ok(buf)
}

/// Premultiplied RGBA8 pixels with straight alpha instead.
pub(crate) fn straight_alpha(rgba: &[u8]) -> Vec<u8> {
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
    let (w, h) = (
        width.clamp(1, procraft_engine::document::MAX_CANVAS_EDGE),
        height.clamp(1, procraft_engine::document::MAX_CANVAS_EDGE),
    );
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
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for ty in 0..th {
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
    (tw, th, out)
}
