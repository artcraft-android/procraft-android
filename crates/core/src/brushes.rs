//! The brush library: its sets and brushes, choosing the brush for the current tool, and preview
//! strokes for the library panel.

use procraft_engine::document::Document;
use procraft_engine::geom::Vec2;
use procraft_engine::input::Pointer;
use procraft_engine::preview;
use procraft_engine::stroke::{Stamp, StrokeBuilder};
use procraft_gpu::{Canvas, StrokeStyle, StrokeTool};

use crate::CanvasSession;
use crate::layers::ThumbnailImage;

/// Where a shape's outline is traced: where its paint is a quarter strong.
const OUTLINE_LEVEL: f32 = 0.25;

/// The last brush stamp pictured, by what it was made from (shape image, own image, hardness,
/// roundness, rotation, size): the size preview asks each frame.
#[allow(clippy::type_complexity)]
static STAMPS: std::sync::Mutex<Option<((Option<u32>, Option<u64>, [u32; 3], usize), Vec<u8>)>> =
    std::sync::Mutex::new(None);

/// The last brush outline traced, by what it was traced from (shape image, own image, roundness,
/// rotation): a cursor asks each frame.
#[allow(clippy::type_complexity)]
static OUTLINES: std::sync::Mutex<Option<((Option<u32>, Option<u64>, u32, u32), Vec<Vec<f32>>)>> =
    std::sync::Mutex::new(None);

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BrushInfo {
    pub set: u32,
    pub index: u32,
    pub name: String,
}

/// A brush in the Recent set: where it is in the library, and whether it is pinned there.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RecentBrush {
    pub set: u32,
    pub index: u32,
    pub name: String,
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BrushSetInfo {
    pub name: String,
    pub brushes: Vec<BrushInfo>,
    /// Which of the set icons it shows.
    pub icon: u32,
    /// The brush last chosen in it, if one has been.
    pub chosen: Option<u32>,
}

/// How near a slider value pulls onto a notch, and how many notches a slider holds.
pub const NOTCH_NEAR: f32 = 0.04;
pub const NOTCH_MOST: usize = 4;

#[uniffi::export]
impl CanvasSession {
    pub fn brush_library(&self) -> Vec<BrushSetInfo> {
        let st = self.shared.lock();
        st.library
            .iter()
            .enumerate()
            .map(|(s, set)| BrushSetInfo {
                name: set.name.to_owned(),
                icon: set.icon,
                chosen: st.set_choice(s).map(|i| i as u32),
                brushes: set
                    .brushes
                    .iter()
                    .enumerate()
                    .map(|(i, b)| BrushInfo { set: s as u32, index: i as u32, name: b.name.clone() })
                    .collect(),
            })
            .collect()
    }

    /// The brush the current tool uses.
    pub fn current_brush(&self) -> BrushInfo {
        let st = self.shared.lock();
        let (set, index) = st.brush_ref(st.tool);
        BrushInfo { set: set as u32, index: index as u32, name: st.brush(st.tool).name.clone() }
    }

    /// The current tool's brush's own outline, for a brush cursor in its shape: loops of x, y
    /// pairs in brush diameters about its middle (y down, as on the screen), squashed across by
    /// its roundness and turned by its fixed rotation; none for the round tip (a circle).
    pub fn brush_outline(&self) -> Vec<Vec<f32>> {
        let (key, pixels) = {
            let st = self.shared.lock();
            let shape = &st.brush(st.tool).shape;
            let key =
                (shape.image, shape.source.as_ref().map(|s| s.id), shape.roundness.to_bits(), shape.rotation.to_bits());
            let pixels = shape.source.as_ref().map(|s| std::sync::Arc::clone(&s.pixels));
            (key, pixels)
        };
        let (image, source, roundness, rotation) = key;
        if image.is_none() && source.is_none() {
            return Vec::new();
        }
        let mut kept = OUTLINES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((k, loops)) = kept.as_ref()
            && *k == key
        {
            return loops.clone();
        }
        let side = procraft_engine::textures::TEXTURE_SIZE;
        let traced = match (pixels, image) {
            (Some(own), _) => procraft_engine::outline::shape_outline(&own, side, OUTLINE_LEVEL),
            (None, Some(i)) => procraft_engine::textures::shapes()
                .get(i as usize)
                .map(|shape| procraft_engine::outline::shape_outline(&shape.pixels, side, OUTLINE_LEVEL))
                .unwrap_or_default(),
            (None, None) => Vec::new(),
        };
        // as a stamp is drawn: across its full size, squashed across, turned on the screen
        let (roundness, turn) = (f32::from_bits(roundness), f32::from_bits(rotation));
        let roundness =
            if roundness.is_finite() { roundness.clamp(0.05, procraft_engine::stroke::MOST_ROUNDNESS) } else { 1.0 };
        let (sin, cos) = if turn.is_finite() { turn.sin_cos() } else { (0.0, 1.0) };
        let loops: Vec<Vec<f32>> = traced
            .iter()
            .map(|ring| {
                ring.iter()
                    .flat_map(|p| {
                        let (x, y) = (p.x - 0.5, (p.y - 0.5) * roundness);
                        [x * cos - y * sin, x * sin + y * cos]
                    })
                    .collect()
            })
            .collect();
        *kept = Some((key, loops.clone()));
        loops
    }

    /// The current tool's brush's stamp as it paints, `side` x `side` pixels of coverage (0..255):
    /// its shape image, or the round tip by its hardness, squashed across by its roundness and
    /// turned by its fixed rotation, filling the square as a stamp fills its diameter. For the
    /// size slider's preview.
    pub fn brush_stamp(&self, side: u32) -> Vec<u8> {
        let side = side.clamp(1, 1024) as usize;
        let (key, source) = {
            let st = self.shared.lock();
            let brush = st.brush(st.tool);
            let shape = &brush.shape;
            let key = (
                shape.image,
                shape.source.as_ref().map(|s| s.id),
                [brush.hardness, shape.roundness, shape.rotation].map(f32::to_bits),
                side,
            );
            (key, shape.source.as_ref().map(|s| std::sync::Arc::clone(&s.pixels)))
        };
        let mut kept = STAMPS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((k, stamp)) = kept.as_ref()
            && *k == key
        {
            return stamp.clone();
        }
        let (image, _, [hardness, roundness, turn], _) = key;
        let (hardness, roundness, turn) = (f32::from_bits(hardness), f32::from_bits(roundness), f32::from_bits(turn));
        let pixels = match (source, image) {
            (Some(own), _) => Some(own.to_vec()),
            (None, Some(i)) => procraft_engine::textures::shapes().get(i as usize).map(|s| s.pixels.clone()),
            (None, None) => None,
        };
        let roundness =
            if roundness.is_finite() { roundness.clamp(0.05, procraft_engine::stroke::MOST_ROUNDNESS) } else { 1.0 };
        let (sin, cos) = if turn.is_finite() { turn.sin_cos() } else { (0.0, 1.0) };
        let size = procraft_engine::textures::TEXTURE_SIZE;
        // averaged down to about the picture's size first, so a small picture keeps fine detail's
        // weight rather than catching a few of its pixels
        let (picture, n) = match &pixels {
            Some(p) if p.len() >= size * size => {
                let mut at = (p.clone(), size);
                while at.1 / 2 >= side.max(1) && at.1 > 1 {
                    at = (halved(&at.0, at.1), at.1 / 2);
                }
                (Some(at.0), at.1)
            }
            _ => (None, 0),
        };
        let r = side as f32 / 2.0;
        let mut out = vec![0u8; side * side];
        for (i, o) in out.iter_mut().enumerate() {
            let (px, py) = ((i % side) as f32 + 0.5 - r, (i / side) as f32 + 0.5 - r);
            // back into the stamp's own frame: turned back, then unsquashed across
            let (x, y) = (px * cos + py * sin, (-px * sin + py * cos) / roundness);
            let cover = match &picture {
                Some(pic) => {
                    let (u, v) = ((x / side as f32 + 0.5) * n as f32 - 0.5, (y / side as f32 + 0.5) * n as f32 - 0.5);
                    bilinear(pic, n, u, v)
                }
                None => {
                    // the round tip as the stamp shader draws it: a one-pixel rim, softened from
                    // its hardness outwards
                    let d = x.hypot(y);
                    let rim = (r - d + 0.5).clamp(0.0, 1.0);
                    let t = ((d - hardness.min(0.999) * r) / (r + 0.5 - hardness.min(0.999) * r)).clamp(0.0, 1.0);
                    rim.min(1.0 - t * t * (3.0 - 2.0 * t))
                }
            };
            *o = (cover.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        *kept = Some((key, out.clone()));
        out
    }

    /// The current tool's brush's control `key`, as the studio shows it.
    pub fn current_brush_setting(&self, key: String) -> f32 {
        let st = self.shared.lock();
        st.brush(st.tool).setting(&key)
    }

    /// The current tool's brush's saved sizes (`size`) or opacities: its slider's notches.
    pub fn slider_notches(&self, size: bool) -> Vec<f32> {
        let st = self.shared.lock();
        st.brush(st.tool).notches.list(notch_tool(st.tool), size).to_vec()
    }

    /// Saves `value` on the current tool's size or opacity slider (its brush keeps it); false if
    /// the slider is full or a notch is there already.
    pub fn add_slider_notch(&self, size: bool, value: f32) -> bool {
        let mut st = self.shared.lock();
        let tool = st.tool;
        st.brush_mut(tool).is_some_and(|b| b.notches.add(notch_tool(tool), size, value))
    }

    /// Takes away the notch `value` is on, on the current tool's size or opacity slider.
    pub fn remove_slider_notch(&self, size: bool, value: f32) -> bool {
        let mut st = self.shared.lock();
        let tool = st.tool;
        st.brush_mut(tool).is_some_and(|b| b.notches.remove(notch_tool(tool), size, value))
    }

    /// The notch `value` is on (within [`NOTCH_NEAR`]), of the size or opacity slider, if any: a
    /// slider dragged there is pulled onto it.
    pub fn slider_notch_at(&self, size: bool, value: f32) -> Option<f32> {
        self.slider_notches(size)
            .into_iter()
            .filter(|n| (n - value).abs() < NOTCH_NEAR)
            .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
    }

    /// Whether the slider's panel offers to save its value: not when it is on a notch or the
    /// slider holds [`NOTCH_MOST`] already.
    pub fn can_save_slider_notch(&self, size: bool) -> bool {
        let info = self.info();
        let value = if size { info.size } else { info.opacity };
        self.slider_notch_at(size, value).is_none() && self.slider_notches(size).len() < NOTCH_MOST
    }

    /// The slider panel's button: saves the slider's value as a notch, or takes away the notch it
    /// is on.
    pub fn toggle_slider_notch(&self, size: bool) {
        let info = self.info();
        let value = if size { info.size } else { info.opacity };
        match self.slider_notch_at(size, value) {
            Some(n) => self.remove_slider_notch(size, n),
            None => self.add_slider_notch(size, value),
        };
    }

    /// Chooses the brush the current tool uses.
    pub fn select_brush(&self, set: u32, index: u32) -> bool {
        let mut st = self.shared.lock();
        let (set, index) = (set as usize, index as usize);
        if st.library.get(set).and_then(|s| s.brushes.get(index)).is_none() {
            return false;
        }
        // the set left keeps showing the brush it had, the new one this
        let before = st.brush_ref(st.tool);
        st.note_choice(before);
        match st.tool {
            crate::ToolKind::Paint => st.paint_brush = (set, index),
            crate::ToolKind::Erase => st.erase_brush = (set, index),
            crate::ToolKind::Smudge => st.smudge_brush = (set, index),
        }
        st.note_choice((set, index));
        true
    }

    /// A sample stroke drawn with the brush, white on transparent, fitted into `width` × `height`
    /// pixels.
    pub fn brush_preview(&self, set: u32, index: u32, width: u32, height: u32) -> Option<ThumbnailImage> {
        let brush = self.shared.lock().library.get(set as usize)?.brushes.get(index as usize)?.clone();
        self.drawn_preview(brush, width, height)
    }

    /// The same of the first or `second` part alone of the dual brush open in the studio.
    pub fn studio_part_preview(&self, second: bool, width: u32, height: u32) -> Option<ThumbnailImage> {
        let brush = self.shared.lock().studio_part(second)?;
        self.drawn_preview(brush, width, height)
    }
}

/// Where a tool's notches are kept on a brush.
pub(crate) fn notch_tool(tool: crate::ToolKind) -> usize {
    procraft_engine::brush::Notches::tool(tool == crate::ToolKind::Paint, tool == crate::ToolKind::Smudge)
}

/// A square grey picture `n` pixels a side averaged down to half that.
fn halved(pixels: &[u8], n: usize) -> Vec<u8> {
    let h = n / 2;
    let at = |x: usize, y: usize| u32::from(pixels.get(y * n + x).copied().unwrap_or(0));
    (0..h * h)
        .map(|i| {
            let (x, y) = (i % h * 2, i / h * 2);
            ((at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1) + 2) / 4) as u8
        })
        .collect()
}

/// A square grey picture `n` pixels a side read between its pixels at (`u`, `v`) (pixel centres
/// on whole numbers), 0..1; nothing outside it.
fn bilinear(pixels: &[u8], n: usize, u: f32, v: f32) -> f32 {
    let at = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= n as i64 || y >= n as i64 {
            0.0
        } else {
            f32::from(pixels.get(y as usize * n + x as usize).copied().unwrap_or(0)) / 255.0
        }
    };
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
    let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

impl CanvasSession {
    /// `brush`'s sample stroke (see [`CanvasSession::brush_preview`]).
    fn drawn_preview(&self, brush: procraft_engine::brush::Brush, width: u32, height: u32) -> Option<ThumbnailImage> {
        let (w, h) = (width.clamp(16, 2048), height.clamp(16, 1024));
        let (cw, ch) = (preview::CANVAS.0 as u32, preview::CANVAS.1 as u32);
        // the preview it came with, while it is as it came
        if let Some((tw, th, rgba)) = brush.thumbnail.as_deref().and_then(crate::brush_files::thumbnail_pixels) {
            return Some(ThumbnailImage { width: w, height: h, rgba: preview::fit_image(&rgba, (tw, th), w, h) });
        }
        // what it needs taken under the lock; drawn without it, so painting goes on meanwhile
        let (gpu, kept) = {
            let mut st = self.shared.lock();
            (st.gpu.clone()?, st.preview.take())
        };
        // drawn on a canvas of the sample stroke's own, then fitted into the preview
        let (mut canvas, mut doc) = match kept {
            Some(kept) => kept,
            None => {
                let mut doc = Document::new(cw, ch);
                doc.background_visible = false;
                (Canvas::new(&gpu, &doc).ok()?, doc)
            }
        };
        let layer = doc.current();
        canvas.wipe_layer(layer);
        let mut style = StrokeStyle::for_brush(&brush, StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0, false);
        style.preview = true;
        canvas.use_sources(&gpu, &mut style, &brush);
        // a dual brush's second, drawn along the same sample stroke at its size beside the first's
        let stamp_only = brush.setting("preview_stamp") >= 0.5;
        let second = brush.secondary.as_deref().filter(|_| !stamp_only && style.takes_second()).and_then(|second| {
            let mut second_style =
                StrokeStyle::for_brush(second, StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0, false).as_second();
            second_style.preview = true;
            canvas.use_sources(&gpu, &mut second_style, second);
            let ratio = second.diameter(0.5) / brush.diameter(0.5).max(1e-3);
            let mode = brush.setting("dual_blend").round().max(0.0) as u32;
            (!second_style.in_place()).then(|| {
                let dual =
                    procraft_gpu::SecondStroke { style: second_style, mode, threshold: second_style.alpha_threshold };
                (second, dual, preview::diameter(&brush) * ratio)
            })
        });
        match &second {
            Some((_, dual, _)) => canvas.begin_dual_stroke(layer, style, *dual),
            None => canvas.begin_stroke(layer, style),
        }
        if stamp_only {
            // the brush's stamp on its own, filling the preview's height
            canvas.add_stamps(&[Stamp::round(Vec2::new(cw as f32 / 2.0, ch as f32 / 2.0), ch as f32 * 0.4, 1.0)]);
        } else {
            let samples = preview::samples(&brush);
            if let Some((second, _, diameter)) = second {
                let mut stroke = StrokeBuilder::new(second.clone(), diameter, Pointer::Stylus).steady();
                canvas.add_second_stamps(&stroke.add(&samples));
                canvas.replace_second_stamps(&stroke.finish());
            }
            let mut stroke = StrokeBuilder::new(brush.clone(), preview::diameter(&brush), Pointer::Stylus).steady();
            canvas.add_stamps(&stroke.add(&samples));
            canvas.replace_stamps(&stroke.finish());
        }
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        canvas.forget_history();
        let rgba = canvas.read_composite(&gpu).ok();
        // kept for the next (one drawn meanwhile elsewhere already is)
        self.shared.lock().preview.get_or_insert((canvas, doc));
        Some(ThumbnailImage { width: w, height: h, rgba: preview::fit(&rgba?, w, h) })
    }
}
