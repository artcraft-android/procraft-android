//! Text layers: text added and edited where it sits. The app lays the text out and draws it into
//! the layer's pixels; the layer keeps the text so it can be edited again, until it is rasterized.
//! An edit is one undo step, back to the text and the pixels as it began.

use procraft_engine::document::{LayerId, LayerText, TextAlign};

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
                st.doc.layer(id).filter(|l| l.text.is_some() && !l.locked)?;
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
    /// how it was (a new one goes away). New text left empty goes away too.
    pub fn text_end(&self, keep: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some((layer, before)) = st.text_edit.take() else { return };
        let new = before.layer(layer).is_none();
        let empty = st.doc.layer(layer).and_then(|l| l.text.as_ref()).is_none_or(|t| t.text.trim().is_empty());
        let keep = keep && !(new && empty);
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.release_layer(keep, keep.then(|| before.clone()));
        }
        if !keep {
            st.doc = before;
        }
        st.revision += 1;
        self.shared.frame(st);
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
