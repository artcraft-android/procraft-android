//! Plugins' layers drawn sharp when zoomed in. Their items are strokes kept as strokes, so once the
//! view settles magnified, the part of the canvas on screen is drawn again at the screen's
//! resolution on a canvas of its own: each plugin layer's items laid again there as much bigger (the
//! same stamps, scaled), every other layer's pixels redrawn there from the canvas's as the screen
//! shows them, and the whole composited as the canvas is (blend modes, groups, masks, clipping). That
//! picture shows over the canvas in its place. Anything that changes the canvas or the view takes it
//! away at once, and it is made again when things settle. The canvas's own pixels stay as they are:
//! saving, exporting, thumbnails and picking colours read those.

use std::time::{Duration, Instant};

use procraft_engine::document::LayerId;
use procraft_engine::geom::{Affine, Vec2};
use procraft_gpu::{Canvas, Resample};

use crate::State;

/// Zoomed in to this many screen pixels a canvas pixel or more, plugins' layers are drawn sharp.
pub(crate) const SHARP_FROM: f32 = 1.5;
/// How long the canvas and the view stay as they are before the sharp picture is made.
const SETTLE: Duration = Duration::from_millis(120);
/// The most pixels the sharp picture may have, and the most its canvas's layers may take.
const MOST_PIXELS: u64 = 24 << 20;
const MOST_BYTES: u64 = 384 << 20;

/// What the sharp picture shows: the canvas as composited (by its count of composites) through
/// the view, onto a target of a size, its pixels crisp or smoothed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shown {
    to_screen: [f32; 6],
    size: (u32, u32),
    composites: u64,
    crisp: bool,
}

/// The canvas the sharp picture is made on, kept for the next.
pub(crate) struct SharpCanvas {
    canvas: Canvas,
    /// Its layers, as the artwork's were when it was made.
    layers: Vec<LayerId>,
}

/// The sharp picture's state.
#[derive(Default)]
pub(crate) struct Sharp {
    held: Option<SharpCanvas>,
    shown: Option<Shown>,
    /// What showed last, and since when: made once it has shown so a while.
    pending: Option<(Shown, Instant)>,
    /// What it couldn't be made for (too big, say): not tried again until something changes.
    failed: Option<Shown>,
    /// How many sharp pictures were made.
    pub(crate) made: u64,
}

/// Where the part of the canvas on a target shows: the canvas rectangle's top left and the size
/// (the sharp canvas's pixels) at `scale` of them a canvas pixel. None if none of it shows.
fn region(to_screen: Affine, size: (u32, u32), canvas: (u32, u32)) -> Option<(Vec2, (u32, u32), f32)> {
    let back = to_screen.inverse()?;
    let (w, h) = (size.0 as f32, size.1 as f32);
    let corners = [Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)].map(|p| back.apply(p));
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let x0 = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).max(0.0);
    let y0 = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).max(0.0);
    let x1 = corners.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).min(cw);
    let y1 = corners.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).min(ch);
    if !(x0 < x1 && y0 < y1) {
        return None;
    }
    let scale = to_screen.scale_factor();
    // unturned, its pixels are the screen's: its top left on a screen pixel (but at the canvas's
    // own edge)
    let straight = to_screen.b.abs() < 1e-6 && to_screen.c.abs() < 1e-6 && to_screen.a > 0.0 && to_screen.d > 0.0;
    let snap =
        |at: f32, a: f32, e: f32| if straight && at > 0.0 { (((a * at + e).floor() - e) / a).max(0.0) } else { at };
    let origin = Vec2::new(snap(x0, to_screen.a, to_screen.e), snap(y0, to_screen.d, to_screen.f));
    // to the canvas's far edges and no further (a sliver short shows the canvas beneath)
    let across = |from: f32, to: f32, edge: bool| {
        let n = (to - from) * scale;
        (if edge { n.floor() } else { n.ceil() }).max(1.0) as u32
    };
    let size = (across(origin.x, x1, x1 >= cw), across(origin.y, y1, y1 >= ch));
    Some((origin, size, scale))
}

impl State {
    /// Keeps the sharp picture over the canvas as it should be this frame, drawn into a target of
    /// `size` through `to_screen`: taken away as anything changes, made again once things settle
    /// (a frame asked for then).
    pub(crate) fn keep_sharp(&mut self, size: (u32, u32), to_screen: Affine) {
        let crisp = to_screen.scale_factor() >= self.display_scale * 0.999;
        let wanted = self.sharp_wanted(to_screen);
        let Some(canvas) = self.canvas.as_ref() else { return };
        let quiet = !canvas.changes_queued() && !canvas.busy();
        let key = Shown {
            to_screen: [to_screen.a, to_screen.b, to_screen.c, to_screen.d, to_screen.e, to_screen.f],
            size,
            composites: canvas.composites(),
            crisp,
        };
        if !wanted || !quiet {
            self.drop_sharp(!wanted);
            // what is under way ends, and it settles again: a frame then to look
            if wanted {
                self.sharp.pending = None;
                self.ask_frame_at(Instant::now() + SETTLE);
            }
            return;
        }
        if self.sharp.shown == Some(key) || self.sharp.failed == Some(key) {
            return;
        }
        self.drop_sharp(false);
        match self.sharp.pending {
            Some((pending, since)) if pending == key && since.elapsed() >= SETTLE => {
                self.sharp.pending = None;
                if self.make_sharp(size, to_screen, crisp) {
                    let composites = self.canvas.as_ref().map_or(0, Canvas::composites);
                    self.sharp.shown = Some(Shown { composites, ..key });
                    self.sharp.made += 1;
                } else {
                    self.sharp.failed = Some(key);
                }
            }
            Some((pending, since)) if pending == key => {
                self.ask_frame_at(since + SETTLE);
            }
            _ => {
                let now = Instant::now();
                self.sharp.pending = Some((key, now));
                self.ask_frame_at(now + SETTLE);
            }
        }
    }

    /// Another frame by `at`.
    fn ask_frame_at(&mut self, at: Instant) {
        self.frame_due = Some(self.frame_due.map_or(at, |due| due.min(at)));
    }

    /// Takes the sharp picture away (and with `all`, the canvas it is made on).
    fn drop_sharp(&mut self, all: bool) {
        if self.sharp.shown.take().is_some()
            && let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref())
        {
            canvas.set_sharp(gpu, None);
        }
        if all {
            self.sharp.held = None;
            self.sharp.pending = None;
        }
    }

    /// Whether plugins' layers would show sharper through `to_screen` now: zoomed in, one of them
    /// showing, nothing under way that the canvas shows as it goes, and the artwork composited as
    /// it plainly is (its background shown, no animation's frames).
    fn sharp_wanted(&self, to_screen: Affine) -> bool {
        let plain = self.doc.background_visible
            && self.doc.animation.is_none()
            && self.stroke.is_none()
            && self.recording.is_none()
            && self.shape.is_none()
            && self.transform.is_none()
            && !self.adjusting
            && self.reveal.is_none()
            && self.view_motion.is_none()
            && !self.scene_shown()
            && self.sphere.is_none();
        plain
            && to_screen.scale_factor() >= SHARP_FROM
            && self.doc.layers().iter().any(|l| l.plugin.is_some() && self.sharp_items(l.id))
    }

    /// Whether `layer` is a plugin's layer shown whose items can be laid again.
    fn sharp_items(&self, layer: LayerId) -> bool {
        self.doc.layer(layer).is_some_and(|l| l.plugin.as_ref().is_some_and(|p| p.data.is_some()))
            && self.doc.unseen(layer).is_none()
            && !self.plugin_layer_off(layer)
    }

    /// Makes the sharp picture for a target of `size` through `to_screen` (raster layers' pixels
    /// crisp or smoothed as the screen shows them) and shows it: false if it couldn't.
    fn make_sharp(&mut self, size: (u32, u32), to_screen: Affine, crisp: bool) -> bool {
        let Some((origin, (w, h), scale)) = region(to_screen, size, (self.doc.width(), self.doc.height())) else {
            return false;
        };
        let Some(gpu) = self.gpu.clone() else { return false };
        let pixels = u64::from(w) * u64::from(h);
        let layers: Vec<LayerId> = self.doc.pixel_layer_ids().collect();
        let most = gpu.max_texture_size();
        if pixels > MOST_PIXELS || w > most || h > most || pixels * 4 * (layers.len() as u64 + 8) > MOST_BYTES {
            return false;
        }
        // the canvas it is made on: this size, with these layers (else made again)
        let doc = self.doc.resized(w, h);
        let kept = self.sharp.held.take().filter(|k| k.canvas.size() == (w, h) && k.layers == layers);
        let mut sharp = match kept {
            Some(kept) => kept,
            None => match Canvas::new(&gpu, &doc) {
                Ok(canvas) => SharpCanvas { canvas, layers: layers.clone() },
                Err(e) => {
                    log::warn!("the sharp picture's canvas: {e}");
                    return false;
                }
            },
        };
        let to_sharp = Affine { a: scale, b: 0.0, c: 0.0, d: scale, e: -origin.x * scale, f: -origin.y * scale };
        let Some(from_sharp) = to_sharp.inverse() else { return false };
        let visible = procraft_engine::geom::IRect::new(
            origin.x.floor() as i32,
            origin.y.floor() as i32,
            (origin.x + w as f32 / scale).ceil() as i32,
            (origin.y + h as f32 / scale).ceil() as i32,
        );
        // each plugin layer's items that reach it, and every other layer's pixels as shown
        let resample = if crisp { Resample::Nearest } else { Resample::Bilinear };
        let mut laid = Vec::new();
        for &id in &layers {
            if self.doc.unseen(id).is_some() {
                continue;
            }
            if self.sharp_items(id)
                && let Some(items) = self.layer_items(id)
            {
                laid.push((id, self.items_reaching(id, items, visible)));
                continue;
            }
            let Some(canvas) = self.canvas.as_mut() else { return false };
            if let Some(view) = canvas.layer_to_read(&gpu, &mut self.doc, id) {
                sharp.canvas.redraw_from(id, view, canvas.size(), from_sharp, resample);
            }
        }
        sharp.canvas.draw_scaled([origin.x, origin.y], scale);
        // laid there as here, the artwork and its canvas lent to it meanwhile
        let mut doc = doc;
        std::mem::swap(&mut self.doc, &mut doc);
        let mut lent = Some(sharp.canvas);
        std::mem::swap(&mut self.canvas, &mut lent);
        self.lay_to = Some(to_sharp);
        for (id, items) in &laid {
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.hold_layer_quietly(*id);
                canvas.wipe_layer(*id);
            }
            let mut brushes = Vec::new();
            for item in items {
                self.lay_clipped(*id, item, &mut brushes);
            }
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.release_layer(true, None);
            }
        }
        self.flush();
        self.lay_to = None;
        std::mem::swap(&mut self.canvas, &mut lent);
        std::mem::swap(&mut self.doc, &mut doc);
        let Some(mut canvas) = lent else { return false };
        canvas.forget_history();
        // shown over the canvas, where it is on it
        let rect = [origin.x, origin.y, w as f32 / scale, h as f32 / scale];
        if let Some(main) = self.canvas.as_mut() {
            main.set_sharp(&gpu, Some((canvas.composite_texture(), (w, h), rect)));
        }
        self.sharp.held = Some(SharpCanvas { canvas, layers });
        true
    }
}

#[cfg(test)]
impl Sharp {
    /// Whether it holds no canvas to make the picture on.
    pub(crate) fn held_none(&self) -> bool {
        self.held.is_none()
    }
}
