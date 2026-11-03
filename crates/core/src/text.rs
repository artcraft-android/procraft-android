//! Text layers: text added and edited where it sits. The app lays the text out and draws it into
//! the layer's pixels; the layer keeps the text so it can be edited again, until it is rasterized.
//! An edit is one undo step, back to the text and the pixels as it began.

use procraft_engine::document::{LayerId, LayerText, TextAlign};
use procraft_engine::geom::IRect;

use crate::CanvasSession;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
    Justify,
}

/// A text layer's text and how it looks.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextInfo {
    pub text: String,
    /// The typeface's family and style names.
    pub font: String,
    pub style: String,
    /// Letter height, canvas pixels.
    pub size: f32,
    /// Fractions of the size, -1..1.
    pub kerning: f32,
    pub tracking: f32,
    pub leading: f32,
    pub baseline: f32,
    pub opacity: f32,
    pub align: TextAlignment,
    pub underline: bool,
    pub outline: bool,
    pub vertical: bool,
    pub all_caps: bool,
    /// sRGB 0..1.
    pub r: f32,
    pub g: f32,
    pub b: f32,
    /// The box's top left corner, canvas pixels, and the width it wraps at (0: no wrapping).
    pub x: f32,
    pub y: f32,
    pub width: f32,
}

impl From<&LayerText> for TextInfo {
    fn from(t: &LayerText) -> Self {
        TextInfo {
            text: t.text.clone(),
            font: t.font.clone(),
            style: t.style.clone(),
            size: t.size,
            kerning: t.kerning,
            tracking: t.tracking,
            leading: t.leading,
            baseline: t.baseline,
            opacity: t.opacity,
            align: match t.align {
                TextAlign::Left => TextAlignment::Left,
                TextAlign::Center => TextAlignment::Center,
                TextAlign::Right => TextAlignment::Right,
                TextAlign::Justify => TextAlignment::Justify,
            },
            underline: t.underline,
            outline: t.outline,
            vertical: t.vertical,
            all_caps: t.all_caps,
            r: t.color[0],
            g: t.color[1],
            b: t.color[2],
            x: t.origin[0],
            y: t.origin[1],
            width: t.width,
        }
    }
}

impl From<TextInfo> for LayerText {
    fn from(t: TextInfo) -> Self {
        LayerText {
            text: t.text,
            font: t.font,
            style: t.style,
            size: t.size,
            kerning: t.kerning,
            tracking: t.tracking,
            leading: t.leading,
            baseline: t.baseline,
            opacity: t.opacity,
            align: match t.align {
                TextAlignment::Left => TextAlign::Left,
                TextAlignment::Center => TextAlign::Center,
                TextAlignment::Right => TextAlign::Right,
                TextAlignment::Justify => TextAlign::Justify,
            },
            underline: t.underline,
            outline: t.outline,
            vertical: t.vertical,
            all_caps: t.all_caps,
            color: [t.r, t.g, t.b],
            origin: [t.x, t.y],
            width: t.width,
        }
    }
}

/// How many finished text edits are remembered for undo and redo to edit again.
const TEXT_STEPS_KEPT: usize = 256;

/// New text as it looks on screen whatever the zoom (as measured): letters this many points
/// tall, its box this many points wide, centred across the window this far down it.
const NEW_TEXT_POINTS: f32 = 64.0;
const NEW_TEXT_BOX_POINTS: f32 = 488.448;
const NEW_TEXT_DOWN: f32 = 0.25;

#[uniffi::export]
impl CanvasSession {
    /// `base` (its words and look) placed as new text in a window `width` × `height` pixels,
    /// `density` pixels a point: its letters and box sized so they look the same at any zoom,
    /// the box centred across the window, its middle a quarter of the way down, kept on the
    /// canvas.
    pub fn new_text_placed(&self, base: TextInfo, width: f32, height: f32, density: f32) -> TextInfo {
        let m = self.view_matrix();
        let [a, b, c, d, e, f] =
            <[f32; 6]>::try_from(m.get(..6).unwrap_or_default()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        // canvas pixels a window point, at the view's zoom (turned or not)
        let shown = a.hypot(b).max(1e-6);
        let per_point = density.max(0.1) / shown;
        // the window point the box is centred on, on the canvas
        let det = a * d - b * c;
        let (x, y) = (width / 2.0 - e, height * NEW_TEXT_DOWN - f);
        let (cx, cy) = if det.abs() < 1e-9 { (0.0, 0.0) } else { ((d * x - c * y) / det, (a * y - b * x) / det) };
        let size = NEW_TEXT_POINTS * per_point;
        let box_width = NEW_TEXT_BOX_POINTS * per_point;
        let mut info = TextInfo { size, width: box_width, x: cx - box_width / 2.0, y: 0.0, ..base };
        // in the last text's typeface, style, alignment and direction
        if let Some(style) = self.shared.lock().doc.text_style.clone() {
            let mut look: LayerText = info.into();
            (look.font, look.style, look.align, look.vertical) = (style.font, style.style, style.align, style.vertical);
            info = TextInfo::from(&look);
        }
        let tall = crate::text_draw::text_box(info.clone()).get(3).copied().unwrap_or(size);
        let canvas_height = self.shared.lock().doc.height() as f32;
        info.y = (cy - tall / 2.0).clamp(0.0, (canvas_height - tall).max(0.0));
        info
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Starts editing text: a new text layer above the current one with `info` (`id` none), or
    /// text layer `id`. Returns the layer being edited, or none if `id` isn't a text layer.
    pub fn text_begin(&self, id: Option<u64>, info: TextInfo) -> Option<u64> {
        self.text_end(true);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let before = st.doc.clone();
        let layer = match id {
            Some(id) => {
                let id = LayerId(id);
                st.doc.layer(id).filter(|l| l.text.is_some() && !st.doc.is_locked(id))?;
                st.doc.set_current(id);
                id
            }
            None if st.doc.has_room(1) => st.doc.add_text_layer(info.into()),
            None => return None,
        };
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.hold_layer(layer);
        }
        st.text_edit = Some((layer, before));
        // a new layer is clear: its first drawing needn't wipe the rest of it
        st.text_drawn = id.is_none().then_some(IRect::EMPTY);
        st.revision += 1;
        self.shared.frame(st);
        Some(layer.0)
    }

    /// Changes the text being edited (one undo step when the edit ends).
    pub fn set_text(&self, info: TextInfo) -> bool {
        let mut st = self.shared.lock();
        let Some((layer, _)) = st.text_edit.as_ref() else { return false };
        let layer = *layer;
        let ok = st.doc.set_text(layer, info.into());
        if ok {
            st.revision += 1;
        }
        ok
    }

    /// Draws the text being edited into its layer, here (a caller may run it off its main
    /// thread): only where it was drawn last and where it now reaches (the whole layer the first
    /// time an edit of a text layer already there draws), nothing handed across. A drawing finished after the text changed
    /// again is dropped (the next one draws it). False when no text is being edited.
    pub fn draw_text_into_layer(&self) -> bool {
        self.text_draw_start().is_some_and(|job| self.text_draw_put(job.draw()))
    }

    /// The text being edited, drawn into its layer: premultiplied RGBA8, the canvas's size.
    pub fn put_text_pixels(&self, rgba: Vec<u8>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some((layer, _)) = st.text_edit.as_ref() else { return false };
        let layer = *layer;
        let Some(canvas) = st.canvas.as_mut() else { return false };
        canvas.put_layer(layer, rgba);
        self.shared.frame(st);
        true
    }

    /// Ends editing text: `keep` makes the edit one undo step; otherwise the layer goes back to
    /// how it was (a new one goes away). New text left empty goes away too, and an edit that
    /// changed nothing makes no step.
    pub fn text_end(&self, keep: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some((layer, before)) = st.text_edit.take() else { return };
        let new = before.layer(layer).is_none();
        let empty = st.doc.layer(layer).and_then(|l| l.text.as_ref()).is_none_or(|t| t.text.trim().is_empty());
        let unchanged = !new && before.layer(layer).map(|l| &l.text) == st.doc.layer(layer).map(|l| &l.text);
        let keep = keep && !(new && empty) && !unchanged;
        if let Some(canvas) = st.canvas.as_mut() {
            if keep {
                canvas.name_next_step("Text editing");
            }
            canvas.release_layer(keep, keep.then(|| before.clone()));
        }
        if !keep {
            let edited = std::mem::replace(&mut st.doc, before);
            st.doc.keep_ids_from(&edited);
        }
        if keep {
            // new text starts with this look
            st.doc.text_style = st.doc.layer(layer).and_then(|l| l.text.as_ref()).map(LayerText::look);
            // the step made, so undo and redo of it can go back to editing this text
            st.flush();
            if let Some(step) = st.canvas.as_ref().and_then(procraft_gpu::Canvas::undo_step) {
                st.text_steps.push((step, layer));
                let over = st.text_steps.len().saturating_sub(TEXT_STEPS_KEPT);
                st.text_steps.drain(..over);
            }
        }
        st.revision += 1;
        self.shared.frame(st);
    }

    /// The text layer undo would take back to editing first: the next step undone is a finished
    /// text edit, its layer still text. None while text is being edited.
    pub fn text_undo_layer(&self) -> Option<u64> {
        let st = self.shared.lock();
        if st.text_edit.is_some() {
            return None;
        }
        let step = st.canvas.as_ref()?.undo_step()?;
        let layer = st.text_steps.iter().rev().find(|(s, _)| *s == step)?.1;
        st.doc.layer(layer).filter(|l| l.text.is_some() && !st.doc.is_locked(layer)).map(|_| layer.0)
    }

    /// The text layer the next redo brings back, when that step is a finished text edit: after
    /// the redo it is edited again.
    pub fn text_redo_layer(&self) -> Option<u64> {
        let st = self.shared.lock();
        let step = st.canvas.as_ref()?.redo_step()?;
        st.text_steps.iter().rev().find(|(s, _)| *s == step).map(|(_, l)| l.0)
    }

    /// The text layer being edited, if any.
    pub fn editing_text(&self) -> Option<u64> {
        self.shared.lock().text_edit.as_ref().map(|(l, _)| l.0)
    }

    /// A text layer's text, or none for another layer.
    pub fn layer_text(&self, id: u64) -> Option<TextInfo> {
        let st = self.shared.lock();
        st.doc.layer(LayerId(id))?.text.as_ref().map(TextInfo::from)
    }

    /// Makes text layer `id` an ordinary layer, its pixels kept, as one undo step.
    pub fn rasterize_layer(&self, id: u64) -> bool {
        self.change("rasterize", |d| d.rasterize(LayerId(id)).then_some(())).is_some()
    }
}

/// The text being edited, to be drawn into its layer within `region` of the canvas (as
/// [`CanvasSession::text_draw_start`] finds it): drawn on any thread, then put in.
pub struct TextDraw {
    layer: LayerId,
    info: TextInfo,
    region: IRect,
}

/// A [`TextDraw`] drawn: the region's pixels, and where in it the text landed.
pub struct TextDrawn {
    job: TextDraw,
    pixels: Vec<u8>,
    painted: Option<IRect>,
}

impl TextDraw {
    /// The letters drawn over transparency within the region (the slow part).
    pub fn draw(self) -> TextDrawn {
        let region = self.region;
        let pixels =
            if region.is_empty() { Vec::new() } else { crate::text_draw::draw_text_within(&self.info, region) };
        let painted = crate::layer_ops::painted_rect(&pixels, region.width())
            .map(|r| IRect::new(r.x0 + region.x0, r.y0 + region.y0, r.x1 + region.x0, r.y1 + region.y0));
        TextDrawn { job: self, pixels, painted }
    }
}

impl CanvasSession {
    /// The text being edited and the part of its layer a drawing of it takes: where it was drawn
    /// last and where it now reaches (all of it the first time on a layer already there); none
    /// when no text is edited.
    pub fn text_draw_start(&self) -> Option<TextDraw> {
        let st = self.shared.lock();
        let (layer, _) = st.text_edit.as_ref()?;
        let info = st.doc.layer(*layer).and_then(|l| l.text.as_ref()).map(TextInfo::from)?;
        let full = IRect::from_size(st.doc.width(), st.doc.height());
        let region = st.text_drawn.map_or(full, |d| d.union(crate::text_draw::text_reach(&info))).intersect(full);
        Some(TextDraw { layer: *layer, info, region })
    }

    /// Puts a drawing into its layer, unless the text changed since it began (a newer one follows)
    /// or isn't edited any more.
    pub fn text_draw_put(&self, drawn: TextDrawn) -> bool {
        let TextDrawn { job, pixels, painted } = drawn;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let now = st.doc.layer(job.layer).and_then(|l| l.text.as_ref()).map(TextInfo::from);
        if st.text_edit.as_ref().map(|(l, _)| *l) != Some(job.layer) || now.as_ref() != Some(&job.info) {
            return false;
        }
        let region = job.region;
        if !region.is_empty()
            && let Some(canvas) = st.canvas.as_mut()
        {
            let (x, y) = (region.x0.max(0) as u32, region.y0.max(0) as u32);
            canvas.put_layer_region(job.layer, x, y, region.width(), region.height(), pixels);
        }
        st.text_drawn = Some(painted.unwrap_or(IRect::EMPTY));
        self.shared.frame(st);
        true
    }
}
