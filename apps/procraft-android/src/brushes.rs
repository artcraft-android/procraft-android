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

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BrushInfo {
    pub set: u32,
    pub index: u32,
    pub name: String,
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
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let brush = st.library.get(set as usize)?.brushes.get(index as usize)?.clone();
        let (w, h) = (width.clamp(16, 2048), height.clamp(16, 1024));
        // the preview it came with, while it is as it came
        if let Some((tw, th, rgba)) = brush.thumbnail.as_deref().and_then(crate::brush_files::thumbnail_pixels) {
            return Some(ThumbnailImage { width: w, height: h, rgba: preview::fit_image(&rgba, (tw, th), w, h) });
        }
        let gpu = st.gpu.as_ref()?;
        // drawn on a canvas of the sample stroke's own, then fitted into the preview
        let (cw, ch) = (preview::CANVAS.0 as u32, preview::CANVAS.1 as u32);
        if st.preview.is_none() {
            let mut doc = Document::new(cw, ch);
            doc.background_visible = false;
            let canvas = Canvas::new(gpu, &doc).ok()?;
            st.preview = Some((canvas, doc));
        }
        let (canvas, doc) = st.preview.as_mut()?;
        let layer = doc.current();
        canvas.wipe_layer(layer);
        let mut style = StrokeStyle::for_brush(&brush, StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0, false);
        style.preview = true;
        canvas.use_sources(gpu, &mut style, &brush);
        canvas.begin_stroke(layer, style);
        if brush.setting("preview_stamp") >= 0.5 {
            // the brush's stamp on its own, filling the preview's height
            canvas.add_stamps(&[Stamp::round(Vec2::new(cw as f32 / 2.0, ch as f32 / 2.0), ch as f32 * 0.4, 1.0)]);
        } else {
            let mut stroke = StrokeBuilder::new(brush.clone(), preview::diameter(&brush), Pointer::Stylus).steady();
            canvas.add_stamps(&stroke.add(&preview::samples(&brush)));
            canvas.replace_stamps(&stroke.finish());
        }
        canvas.end_stroke();
        canvas.render(gpu, doc, None);
        canvas.forget_history();
        let rgba = canvas.read_composite(gpu).ok()?;
        Some(ThumbnailImage { width: w, height: h, rgba: preview::fit(&rgba, w, h) })
    }
}

/// Where a tool's notches are kept on a brush.
fn notch_tool(tool: crate::ToolKind) -> usize {
    procraft_engine::brush::Notches::tool(tool == crate::ToolKind::Paint, tool == crate::ToolKind::Smudge)
}
