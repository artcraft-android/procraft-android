//! Copy, cut and paste, the canvas flipped, and pictures brought in as layers.

use procraft_engine::document::{LayerId, LayerKind};
use procraft_engine::geom::IRect;

use crate::layers::ThumbnailImage;
use crate::{CanvasSession, State};

/// What bringing in a moving picture's frames did: how many came in as layers, and whether the
/// canvas ran out of room for more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FramesInserted {
    pub added: u32,
    pub full: bool,
}

/// The most frames brought in from one moving picture.
const MOST_FRAMES: usize = 999;

impl State {
    /// The current layer's pixels within the selection (all of it without one), the canvas's
    /// size.
    fn selected_pixels(&mut self) -> Option<Vec<u8>> {
        self.canvas.as_ref().zip(self.gpu.as_ref())?;
        let current = self.doc.current();
        // a group has no pixels of its own to copy
        if !self.doc.pixel_layer_ids().any(|id| id == current) {
            return None;
        }
        let (w, h) = (self.doc.width(), self.doc.height());
        let clear = || vec![0u8; w as usize * h as usize * 4];
        // only what is selected where the layer is painted is read back; the rest is clear
        let mask = self.selection.clone();
        let selected = mask.as_ref().map_or(Some(IRect::from_size(w, h)), crate::layer_ops::mask_rect);
        let painted = self.painted_box(current);
        let Some(rect) = selected.zip(painted).map(|(a, b)| a.intersect(b)).filter(|r| !r.is_empty()) else {
            return Some(clear());
        };
        let mut part = self.layer_part(current, rect)?;
        if let Some(mask) = &mask {
            let taken = crate::layer_ops::mask_part(mask, rect);
            for (px, &m) in part.as_chunks_mut::<4>().0.iter_mut().zip(&taken) {
                let k = u16::from(m);
                for c in px.iter_mut() {
                    *c = ((u16::from(*c) * k + 127) / 255) as u8;
                }
            }
        }
        // the whole canvas: the part is the copy
        if rect == IRect::from_size(w, h) && part.len() == w as usize * h as usize * 4 {
            return Some(part);
        }
        // the part put back in its place
        let mut out = clear();
        let (row, x0, y0) = (rect.width() as usize * 4, rect.x0.max(0) as usize, rect.y0.max(0) as usize);
        for (y, from) in part.chunks(row.max(1)).enumerate() {
            let at = ((y0 + y) * w as usize + x0) * 4;
            if let Some(to) = out.get_mut(at..at + row) {
                to.copy_from_slice(from);
            }
        }
        Some(out)
    }
}

/// `image`'s straight colours premultiplied, as layers hold them.
pub(crate) fn premultiplied(mut image: ThumbnailImage) -> ThumbnailImage {
    for px in image.rgba.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        for c in px.iter_mut().take(3) {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
    image
}

/// `image` placed on a `width` × `height` canvas: centred, and scaled down to fit when `fit` and
/// it is larger (premultiplied RGBA8; anything outside is cut off).
fn placed(image: ThumbnailImage, width: u32, height: u32, fit: bool) -> Option<Vec<u8>> {
    let (iw, ih) = (image.width as usize, image.height as usize);
    if iw == 0 || ih == 0 || image.rgba.len() != iw * ih * 4 {
        return None;
    }
    // the canvas's size: as it is
    if (image.width, image.height) == (width, height) {
        return Some(image.rgba);
    }
    let (w, h) = (width as usize, height as usize);
    let scale = if fit { (w as f32 / iw as f32).min(h as f32 / ih as f32).min(1.0) } else { 1.0 };
    let (sw, sh) = ((iw as f32 * scale).round().max(1.0) as usize, (ih as f32 * scale).round().max(1.0) as usize);
    let (ox, oy) = ((w as i64 - sw as i64) / 2, (h as i64 - sh as i64) / 2);
    let mut out = vec![0u8; w * h * 4];
    // the target columns on the canvas, and each one's nearest source column (scaling only ever
    // shrinks), worked out once
    let (x0, x1) = ((-ox).max(0) as usize, ((w as i64 - ox).min(sw as i64)).max(0) as usize);
    let cols: Vec<usize> = (x0..x1).map(|x| (((x as f32 + 0.5) / scale) as usize).min(iw - 1)).collect();
    let unscaled = scale == 1.0;
    for y in 0..sh {
        let ty = oy + y as i64;
        if ty < 0 || ty >= h as i64 {
            continue;
        }
        let sy = (((y as f32 + 0.5) / scale) as usize).min(ih - 1);
        let at = (ty as usize * w + (ox + x0 as i64) as usize) * 4;
        let Some(to) = out.get_mut(at..at + (x1 - x0) * 4) else { continue };
        let row = image.rgba.get(sy * iw * 4..(sy + 1) * iw * 4).unwrap_or_default();
        if unscaled {
            // a row as it is
            if let Some(from) = row.get(x0 * 4..x1 * 4) {
                to.copy_from_slice(from);
            }
            continue;
        }
        for (px, &sx) in to.as_chunks_mut::<4>().0.iter_mut().zip(&cols) {
            if let Some(from) = row.get(sx * 4..sx * 4 + 4) {
                px.copy_from_slice(from);
            }
        }
    }
    Some(out)
}

/// A copy as it goes on a `width` × `height` canvas, and whether it is fitted there: from a canvas
/// this size it lands where it was; from another size its painted part is centred, scaled down
/// only if it is larger, so nothing of it is cut off.
fn to_paste(image: ThumbnailImage, width: u32, height: u32) -> (ThumbnailImage, bool) {
    if (image.width, image.height) == (width, height) {
        return (image, false);
    }
    let whole = IRect::from_size(image.width, image.height);
    let Some(rect) = crate::layer_ops::painted_rect(&image.rgba, image.width).filter(|&r| r != whole) else {
        return (image, true);
    };
    let (row, x0, x1) = (image.width as usize * 4, rect.x0 as usize * 4, rect.x1 as usize * 4);
    let mut rgba = Vec::with_capacity((x1 - x0) * rect.height() as usize);
    for line in image.rgba.chunks_exact(row.max(1)).skip(rect.y0 as usize).take(rect.height() as usize) {
        rgba.extend_from_slice(line.get(x0..x1).unwrap_or_default());
    }
    (ThumbnailImage { width: rect.width(), height: rect.height(), rgba }, true)
}

/// `pixels` (premultiplied RGBA8, `width` wide) mirrored left to right, or top to bottom (as the
/// GPU's flip should).
#[cfg(test)]
pub(crate) fn flipped(pixels: &[u8], width: usize, horizontal: bool) -> Vec<u8> {
    let rows: Vec<&[[u8; 4]]> = pixels.as_chunks::<4>().0.chunks(width.max(1)).collect();
    if horizontal {
        rows.iter().flat_map(|r| r.iter().rev()).flatten().copied().collect()
    } else {
        rows.iter().rev().flat_map(|r| r.iter()).flatten().copied().collect()
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The current layer within the selection, to paste later.
    pub fn copy_layer(&self) -> Option<ThumbnailImage> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let rgba = st.selected_pixels()?;
        Some(ThumbnailImage { width: st.doc.width(), height: st.doc.height(), rgba })
    }

    /// The whole canvas as it shows, to paste later.
    pub fn copy_canvas(&self) -> Option<ThumbnailImage> {
        let mut guard = self.shared.lock_finished();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.flush();
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return None };
        canvas.render_finished(gpu, &mut st.doc);
        let rgba = canvas.read_composite(gpu).ok()?;
        Some(ThumbnailImage { width: st.doc.width(), height: st.doc.height(), rgba })
    }

    /// Copies the current layer within the selection, then clears that.
    pub fn cut_layer(&self) -> Option<ThumbnailImage> {
        let copied = self.copy_layer()?;
        self.clear_layer();
        Some(copied)
    }

    /// A new layer above the current one holding `image` where it was copied from (from a canvas
    /// of another size, what is painted of it centred and scaled down to fit), or the current mask
    /// holding it, if a plugin says to and it can; returns the layer's id.
    pub fn paste(&self, image: ThumbnailImage) -> Option<u64> {
        let (image, fit) = self.to_paste(image);
        if let Some(pasted) = self.mask_to_paste_into().and_then(|mask| self.paste_into(mask, image.clone(), fit)) {
            return Some(pasted);
        }
        self.add_picture(image, fit, false)
    }

    /// Whether what is pasted now goes into the current mask: through a plugin a row at a time, which
    /// on a big canvas takes a moment.
    pub fn pastes_into_mask(&self) -> bool {
        self.mask_to_paste_into().is_some()
    }

    /// As [`paste`](Self::paste), on a private layer: one the gallery's thumbnail leaves out.
    pub fn paste_private(&self, image: ThumbnailImage) -> Option<u64> {
        let (image, fit) = self.to_paste(image);
        self.add_picture(image, fit, true)
    }

    /// A new layer above the current one holding `image` (a photo or file, straight RGBA8),
    /// centred and scaled down to fit (on a private layer, one the gallery's thumbnail leaves
    /// out, if `private`); returns its id.
    pub fn insert_image(&self, image: ThumbnailImage, private: bool) -> Option<u64> {
        self.add_picture(premultiplied(image), true, private)
    }

    /// The current mask holding `image` (from another app, straight RGBA8), centred and scaled
    /// down to fit, if a plugin says to and it can; whether it does.
    pub fn paste_picture_into_mask(&self, image: ThumbnailImage) -> bool {
        self.mask_to_paste_into().is_some_and(|mask| self.paste_into(mask, premultiplied(image), true).is_some())
    }

    /// The frames of the animated GIF `bytes` as layers above the current one, each fitted to the
    /// canvas, for as many as there is room: Animation Assist comes on (unless it is on already),
    /// and each frame is held for as long as the file shows it at the animation's frame rate.
    /// None if it isn't a GIF of more than one frame (a still picture: insert it as one).
    pub fn insert_gif_frames(&self, bytes: Vec<u8>, private: bool) -> Option<FramesInserted> {
        if procraft_engine::gif::frame_count(&bytes) < 2 {
            return None;
        }
        let (width, height) = procraft_engine::gif::size(&bytes)?;
        // each frame's layer and how long it shows, ms
        let mut held: Vec<(LayerId, u32)> = Vec::new();
        let mut full = false;
        procraft_engine::gif::frames(&bytes, |rgba, ms| {
            let mut image = ThumbnailImage { width, height, rgba: rgba.to_vec() };
            // straight to premultiplied
            for px in image.rgba.as_chunks_mut::<4>().0 {
                let a = u16::from(px[3]);
                for c in px.iter_mut().take(3) {
                    *c = ((u16::from(*c) * a + 127) / 255) as u8;
                }
            }
            let Some(id) = self.add_picture(image, true, private) else {
                full = true;
                return false;
            };
            held.push((LayerId(id), ms));
            held.len() < MOST_FRAMES
        })?;
        if held.is_empty() {
            return Some(FramesInserted { added: 0, full });
        }
        let assisted = self.shared.lock().doc.animation.as_ref().is_some_and(|a| !a.pages);
        if !assisted {
            self.set_animation_assist(true);
        }
        // each shown for as many of the animation's frames as its time takes, at the rate it has
        // now (turned on from Page Assist, its own)
        if let Some(a) = self.shared.lock().doc.animation.as_mut() {
            let fps = f64::from(a.fps.max(1));
            for &(id, ms) in &held {
                let shown = ((f64::from(ms) * fps / 1000.0).round() as u32).max(1);
                a.set_hold(id, shown - 1);
            }
        }
        Some(FramesInserted { added: held.len() as u32, full })
    }

    /// Mirrors every layer of the canvas, left to right or (not `horizontal`) top to bottom, as
    /// one undo step.
    pub fn flip_canvas(&self, horizontal: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.flush();
        let ids: Vec<LayerId> = st.doc.layer_ids().collect();
        let Some(canvas) = st.canvas.as_mut() else { return };
        canvas.name_next_step(if horizontal { "flip canvas horizontally" } else { "flip canvas vertically" });
        // mirrored on the GPU, nothing read back
        canvas.flip_layers(ids, horizontal);
        st.revision += 1;
        self.shared.frame(st);
    }
}

impl CanvasSession {
    /// A copy as it goes on this canvas, and whether it is fitted there ([`to_paste`]).
    fn to_paste(&self, image: ThumbnailImage) -> (ThumbnailImage, bool) {
        let (w, h) = {
            let st = self.shared.lock();
            (st.doc.width(), st.doc.height())
        };
        to_paste(image, w, h)
    }

    /// The current layer, if it is a mask and a plugin says what is pasted goes into it.
    fn mask_to_paste_into(&self) -> Option<LayerId> {
        let st = self.shared.lock();
        let current = st.doc.current();
        let is_mask = st.doc.layer(current).is_some_and(|l| l.kind == LayerKind::Mask);
        let asked = is_mask && st.hooks.as_ref().is_some_and(|hooks| hooks.paste_mask());
        (asked && !st.doc.is_locked(current)).then_some(current)
    }

    /// `mask` holds `image` as the plugin makes it, a row at a time: one undo step. None if it
    /// couldn't. What takes a while is done with the session free.
    fn paste_into(&self, mask: LayerId, image: ThumbnailImage, fit: bool) -> Option<u64> {
        let (width, height, hooks) = {
            let st = self.shared.lock();
            (st.doc.width(), st.doc.height(), st.hooks.clone()?)
        };
        let mut pixels = placed(image, width, height, fit)?;
        if !hooks.paste_rows(&mut pixels, width as usize) {
            return None;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let canvas = st.canvas.as_mut()?;
        canvas.name_next_step("paste");
        canvas.write_layer(mask, pixels);
        st.revision += 1;
        self.shared.frame(st);
        Some(mask.0)
    }

    fn add_picture(&self, image: ThumbnailImage, fit: bool, private: bool) -> Option<u64> {
        let (w, h) = {
            let st = self.shared.lock();
            (st.doc.width(), st.doc.height())
        };
        let pixels = placed(image, w, h, fit)?;
        // none if there is no room for it (the new layer may take the last)
        let id = self.change("new layer", |d| {
            d.has_room(1).then(|| {
                let id = d.add_layer();
                d.set_private(id, private);
                id.0
            })
        })?;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        // only what is painted is sent (the new layer is clear elsewhere): a copied layer comes
        // the canvas's size, often mostly clear
        if let (Some(canvas), Some(gpu), Some(rect)) =
            (st.canvas.as_mut(), st.gpu.as_ref(), crate::layer_ops::painted_rect(&pixels, w))
        {
            let part = if rect == IRect::from_size(w, h) {
                pixels
            } else {
                let (row, x0, x1) = (w as usize * 4, rect.x0 as usize * 4, rect.x1 as usize * 4);
                let mut part = Vec::with_capacity((x1 - x0) * rect.height() as usize);
                for line in pixels.chunks_exact(row.max(1)).skip(rect.y0 as usize).take(rect.height() as usize) {
                    part.extend_from_slice(line.get(x0..x1).unwrap_or_default());
                }
                part
            };
            canvas.upload_layer_part(gpu, LayerId(id), rect, &part).ok()?;
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flips_mirror_rows_and_columns() {
        // two by two: red green / blue white
        let px: Vec<u8> = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255]].concat();
        assert_eq!(
            flipped(&px, 2, true),
            [[0, 255, 0, 255], [255, 0, 0, 255], [255, 255, 255, 255], [0, 0, 255, 255]].concat()
        );
        assert_eq!(
            flipped(&px, 2, false),
            [[0, 0, 255, 255], [255, 255, 255, 255], [255, 0, 0, 255], [0, 255, 0, 255]].concat()
        );
    }

    #[test]
    fn pictures_are_centred_and_fitted() {
        let image = ThumbnailImage { width: 4, height: 2, rgba: vec![200; 4 * 2 * 4] };
        let on = placed(image, 8, 8, true).unwrap();
        // centred: rows 3 and 4, columns 2 to 5
        assert_eq!(on[(3 * 8 + 2) * 4], 200);
        assert_eq!(on[(3 * 8 + 1) * 4], 0);
        let big = ThumbnailImage { width: 16, height: 16, rgba: vec![9; 16 * 16 * 4] };
        let fitted = placed(big, 8, 8, true).unwrap();
        assert!(fitted.iter().all(|&v| v == 9));
        let same = ThumbnailImage { width: 8, height: 8, rgba: (0..=255).cycle().take(8 * 8 * 4).collect() };
        assert_eq!(placed(same.clone(), 8, 8, false).unwrap(), same.rgba);
        assert!(placed(ThumbnailImage { width: 3, height: 3, rgba: vec![] }, 8, 8, true).is_none());
    }
}
