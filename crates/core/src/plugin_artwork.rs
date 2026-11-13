//! A session showing a plugin's own artwork (`plugin_canvas.rs`): its strokes go to the plugin as
//! touches, what the plugin draws is put in the session's one layer after each change (so the view,
//! the gestures and the rest of the canvas work as for any artwork), and what the plugin saves
//! after each stroke is a step undo and redo go through. The artwork's file is the plugin's data.

use std::sync::Arc;

use procraft_engine::document::LayerId;

use crate::{CanvasSession, PenSample, PluginCanvas, State};

/// The most steps of a plugin's artwork kept to undo, and the most their data may take in all.
const MOST_STEPS: usize = 100;
const MOST_STEP_BYTES: usize = 256 << 20;

/// A plugin's artwork named by a folder opened: its kind, its data (none if it couldn't be read)
/// and the layer its picture is in, until the app opens it with its plugin.
#[derive(Clone)]
pub(crate) struct PluginOpened {
    pub(crate) kind: String,
    pub(crate) data: Option<Vec<u8>>,
    pub(crate) layer: Option<u64>,
}

/// A plugin's artwork open in a session.
pub(crate) struct PluginArtwork {
    pub(crate) kind: String,
    pub(crate) canvas: Arc<PluginCanvas>,
    /// The layer its picture is put in.
    pub(crate) layer: LayerId,
    /// Its data now, and before (to undo) and after (to redo).
    pub(crate) kept: Vec<u8>,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
    /// A stroke changed it since it was last kept.
    touched: bool,
    /// Where the last touch was, canvas pixels: a stroke ending with no sample of its own ends there.
    last: (f32, f32),
    /// A stroke is under way on it: saving waits.
    pub(crate) down: bool,
}

/// How a touch goes to a plugin's artwork.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Down = 0,
    Moved = 1,
    Up = 2,
    Off = 3,
}

impl State {
    /// What the plugin draws, put in its layer (no step of the canvas's: the plugin's data is the
    /// step).
    fn show_plugin_picture(&mut self) {
        let Some(artwork) = self.plugin_artwork.as_ref() else { return };
        let picture = artwork.canvas.picture();
        // a plugin that stopped drawing it leaves the layer as it last was
        if artwork.canvas.stop().is_some() {
            return;
        }
        let layer = artwork.layer;
        let changed = artwork.canvas.changed();
        let width = artwork.canvas.width() as usize;
        match self.canvas.as_mut() {
            // while a stroke goes on, only the part it changed goes to the GPU
            Some(canvas) => match changed {
                crate::plugin_canvas::Changed::Whole => canvas.put_layer(layer, picture),
                crate::plugin_canvas::Changed::Part(x, y, w, h) => {
                    let mut part = Vec::with_capacity(w as usize * h as usize * 4);
                    for row in y as usize..(y + h) as usize {
                        let at = (row * width + x as usize) * 4;
                        part.extend_from_slice(picture.get(at..at + w as usize * 4).unwrap_or_default());
                    }
                    canvas.put_layer_region(layer, x, y, w, h, part);
                }
                crate::plugin_canvas::Changed::Nothing => {}
            },
            // not shown yet: the layer's pixels when it is
            None => {
                self.pending_pixels.retain(|(id, _)| *id != layer);
                self.pending_pixels.push((layer, crate::project::Opened::Pixels(picture)));
            }
        }
        self.revision += 1;
    }

    /// `samples` (window pixels) handed to the plugin's artwork as touches of `phase` (the first;
    /// the rest moved, or all of them up): true if the session shows one, so nothing else takes
    /// them.
    pub(crate) fn plugin_touch(&mut self, phase: Phase, samples: &[PenSample]) -> bool {
        if self.plugin_artwork.is_none() {
            return false;
        }
        let points = self.to_canvas(samples);
        // as a stroke starts, the tool it is drawn with
        if phase == Phase::Down {
            let sliders = self.shown_sliders();
            let tool = match self.tool {
                crate::ToolKind::Paint => 0,
                crate::ToolKind::Erase => 1,
                crate::ToolKind::Smudge => 2,
            };
            let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
            let [r, g, b] = self.color;
            let rgba = (byte(r) << 24) | (byte(g) << 16) | (byte(b) << 8) | 0xFF;
            let size = self.brush(self.tool).diameter(sliders.size) * self.brush_scale();
            if let Some(artwork) = self.plugin_artwork.as_ref() {
                artwork.canvas.tool(tool, rgba, size, sliders.opacity);
            }
        }
        let Some(artwork) = self.plugin_artwork.as_mut() else { return false };
        let mut changed = false;
        let mut told = false;
        for (i, p) in points.iter().enumerate().filter(|(_, p)| p.pos.x.is_finite() && p.pos.y.is_finite()) {
            let this = if phase == Phase::Down && i > 0 { Phase::Moved } else { phase };
            changed |= artwork.canvas.touch(this as u32, p.pos.x, p.pos.y, p.pressure);
            artwork.last = (p.pos.x, p.pos.y);
            told = true;
        }
        // a stroke that ends with no sample of its own still ends, where it was
        if !told && matches!(phase, Phase::Up | Phase::Off) {
            changed |= artwork.canvas.touch(phase as u32, artwork.last.0, artwork.last.1, 0.0);
        }
        artwork.touched |= changed;
        let ended = matches!(phase, Phase::Up | Phase::Off);
        artwork.down = !ended;
        let mut stepped = false;
        if ended && std::mem::take(&mut artwork.touched) {
            // what the stroke made is a step, unless it made nothing (a stroke called off)
            if let Some(now) = artwork.canvas.save().filter(|now| *now != artwork.kept) {
                let before = std::mem::replace(&mut artwork.kept, now);
                artwork.undo.push(before);
                artwork.redo.clear();
                // as many steps as are kept, in as much memory
                let bytes = |steps: &[Vec<u8>]| steps.iter().map(Vec::len).sum::<usize>();
                while artwork.undo.len() > MOST_STEPS
                    || (bytes(&artwork.undo) > MOST_STEP_BYTES && artwork.undo.len() > 1)
                {
                    artwork.undo.remove(0);
                }
                stepped = true;
            }
        }
        if changed {
            self.show_plugin_picture();
        } else if stepped {
            // a step is something to save, even if nothing shows changed
            self.revision += 1;
        }
        true
    }

    /// Whether the plugin's artwork has a step to undo and one to redo, if the session shows one.
    pub(crate) fn plugin_steps(&self) -> Option<(bool, bool)> {
        self.plugin_artwork.as_ref().map(|a| (!a.undo.is_empty(), !a.redo.is_empty()))
    }

    /// The plugin's artwork a step back (`undo`) or on: whether there was one to take.
    fn plugin_step(&mut self, undo: bool) -> Option<bool> {
        let artwork = self.plugin_artwork.as_mut()?;
        let (from, to) =
            if undo { (&mut artwork.undo, &mut artwork.redo) } else { (&mut artwork.redo, &mut artwork.undo) };
        let Some(data) = from.pop() else { return Some(false) };
        if !artwork.canvas.load(data.clone()) {
            from.push(data);
            return Some(false);
        }
        to.push(std::mem::replace(&mut artwork.kept, data));
        self.show_plugin_picture();
        Some(true)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Shows `canvas`, a plugin's artwork of kind `kind` (`<id>.<key>`), in this session, its picture
    /// in the current layer: from then on strokes go to it and undo goes through what it saves.
    /// False if it isn't the canvas's size.
    pub fn attach_plugin_canvas(&self, kind: String, canvas: Arc<PluginCanvas>) -> bool {
        self.attach_plugin_canvas_to(kind, canvas, None)
    }

    /// The kind of plugin artwork this session shows (`<id>.<key>`), or was opened from a folder
    /// holding, if any.
    pub fn plugin_kind(&self) -> Option<String> {
        let st = self.shared.lock();
        st.plugin_artwork.as_ref().map(|a| a.kind.clone()).or_else(|| st.plugin_opened.as_ref().map(|o| o.kind.clone()))
    }

    /// Opened from a folder holding a plugin's artwork, opens it with `plugins` and shows it: true
    /// if it is one and its plugin (on) opened it. With the plugin off, or gone, or its data not
    /// read, the artwork shows what it last drew, and takes strokes as any other (its data left as
    /// it is in the folder).
    pub fn open_plugin_artwork(&self, plugins: Arc<crate::Plugins>) -> bool {
        let opened = self.shared.lock().plugin_opened.clone();
        // (with no data, a plugin would make a new, empty artwork over what the layer still has)
        let Some(PluginOpened { kind, data: Some(data), layer }) = opened else { return false };
        let (width, height, layer) = {
            let st = self.shared.lock();
            let layer = layer.map(LayerId).filter(|id| st.doc.layer(*id).is_some());
            (st.doc.width(), st.doc.height(), layer)
        };
        let Some(canvas) = plugins.open_canvas(kind.clone(), width, height, Some(data)) else { return false };
        let attached = self.attach_plugin_canvas_to(kind, canvas, layer);
        if attached {
            self.shared.lock().plugin_opened = None;
        }
        attached
    }

    /// The plugin artwork's data as it was last kept (after its last stroke), for its file.
    pub fn plugin_data(&self) -> Option<Vec<u8>> {
        self.shared.lock().plugin_artwork.as_ref().map(|a| a.kept.clone())
    }

    /// Whether the session shows a plugin's artwork, which shares as SVG too.
    pub fn exports_svg(&self) -> bool {
        self.shared.lock().plugin_artwork.is_some()
    }

    /// The plugin's artwork written to `path` as an SVG picture, its shapes kept as they are.
    pub fn export_svg(&self, path: String) -> Result<(), crate::MobileError> {
        let canvas = self.shared.lock().plugin_artwork.as_ref().map(|a| Arc::clone(&a.canvas));
        let reason = |why: &str| crate::MobileError::File { reason: why.to_owned() };
        let text = canvas.ok_or_else(|| reason("not an artwork a plugin draws"))?.svg();
        let text = text.ok_or_else(|| reason("the plugin couldn't draw it"))?;
        crate::project::write_atomically(std::path::Path::new(&path), |p| {
            std::fs::write(p, text.as_bytes()).map_err(|e| reason(&format!("writing the picture: {e}")))
        })
    }

    /// Why the plugin showing this artwork stopped, if it did: it takes no more strokes.
    pub fn plugin_problem(&self) -> Option<String> {
        self.shared.lock().plugin_artwork.as_ref().and_then(|a| a.canvas.problem())
    }
}

impl CanvasSession {
    /// [`CanvasSession::attach_plugin_canvas`], its picture in `layer` (else the current one).
    fn attach_plugin_canvas_to(&self, kind: String, canvas: Arc<PluginCanvas>, layer: Option<LayerId>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if (canvas.width(), canvas.height()) != (st.doc.width(), st.doc.height()) {
            return false;
        }
        let Some(kept) = canvas.save() else { return false };
        let layer = layer.unwrap_or_else(|| st.doc.current());
        st.plugin_artwork = Some(PluginArtwork {
            kind,
            canvas,
            layer,
            kept,
            undo: Vec::new(),
            redo: Vec::new(),
            touched: false,
            last: (0.0, 0.0),
            down: false,
        });
        st.show_plugin_picture();
        self.shared.frame(st);
        true
    }

    /// Undo (`undo`) or redo on a plugin's artwork: none if the session shows none, else whether
    /// there was a step.
    pub(crate) fn plugin_undo(&self, undo: bool) -> Option<bool> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let stepped = st.plugin_step(undo)?;
        self.shared.frame(st);
        Some(stepped)
    }
}
