//! Copy, cut and paste, the canvas flipped, and pictures brought in as layers.

use procraft_engine::document::LayerId;

use crate::layers::ThumbnailImage;
use crate::{CanvasSession, State};

impl State {
    /// The current layer's pixels within the selection (all of it without one).
    fn selected_pixels(&mut self) -> Option<Vec<u8>> {
        let mask = self.selection.clone();
        let mut pixels = self.current_pixels()?;
        if let Some(mask) = mask {
            for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&mask.data) {
                let k = u16::from(m);
                for c in px.iter_mut() {
                    *c = ((u16::from(*c) * k + 127) / 255) as u8;
                }
            }
        }
        Some(pixels)
    }
}

/// `image` placed on a `width` × `height` canvas: centred, and scaled down to fit when `fit` and
/// it is larger (premultiplied RGBA8; anything outside is cut off).
fn placed(image: &ThumbnailImage, width: u32, height: u32, fit: bool) -> Option<Vec<u8>> {
    let (iw, ih) = (image.width as usize, image.height as usize);
    if iw == 0 || ih == 0 || image.rgba.len() != iw * ih * 4 {
        return None;
    }
    let (w, h) = (width as usize, height as usize);
    let scale = if fit { (w as f32 / iw as f32).min(h as f32 / ih as f32).min(1.0) } else { 1.0 };
    let (sw, sh) = ((iw as f32 * scale).round().max(1.0) as usize, (ih as f32 * scale).round().max(1.0) as usize);
    let (ox, oy) = ((w as i64 - sw as i64) / 2, (h as i64 - sh as i64) / 2);
    let mut out = vec![0u8; w * h * 4];
    for y in 0..sh {
        let ty = oy + y as i64;
        if ty < 0 || ty >= h as i64 {
            continue;
        }
        // nearest source row and column (scaling only ever shrinks)
        let sy = ((y as f32 + 0.5) / scale) as usize;
        for x in 0..sw {
            let tx = ox + x as i64;
            if tx < 0 || tx >= w as i64 {
                continue;
            }
            let sx = ((x as f32 + 0.5) / scale) as usize;
            let (src, dst) = ((sy.min(ih - 1) * iw + sx.min(iw - 1)) * 4, (ty as usize * w + tx as usize) * 4);
            if let (Some(from), Some(to)) = (image.rgba.get(src..src + 4), out.get_mut(dst..dst + 4)) {
                to.copy_from_slice(from);
            }
        }
    }
    Some(out)
}

/// `pixels` (premultiplied RGBA8, `width` wide) mirrored left to right, or top to bottom.
fn flipped(pixels: &[u8], width: usize, horizontal: bool) -> Vec<u8> {
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
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.flush();
        let (Some(canvas), Some(gpu)) = (st.canvas.as_ref(), st.gpu.as_ref()) else { return None };
        let rgba = canvas.read_composite(gpu).ok()?;
        Some(ThumbnailImage { width: st.doc.width(), height: st.doc.height(), rgba })
    }

    /// Copies the current layer within the selection, then clears that.
    pub fn cut_layer(&self) -> Option<ThumbnailImage> {
        let copied = self.copy_layer()?;
        self.clear_layer();
        Some(copied)
    }

    /// A new layer above the current one holding `image` where it was copied from (centred when
    /// it is another size); returns its id.
    pub fn paste(&self, image: ThumbnailImage) -> Option<u64> {
        self.add_picture(&image, false)
    }

    /// A new layer above the current one holding `image` (a photo or file, straight RGBA8),
    /// centred and scaled down to fit; returns its id.
    pub fn insert_image(&self, image: ThumbnailImage) -> Option<u64> {
        let mut premultiplied = image;
        for px in premultiplied.rgba.as_chunks_mut::<4>().0 {
            let a = u16::from(px[3]);
            for c in px.iter_mut().take(3) {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
        self.add_picture(&premultiplied, true)
    }

    /// Mirrors every layer of the canvas, left to right or (not `horizontal`) top to bottom, as
    /// one undo step.
    pub fn flip_canvas(&self, horizontal: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.flush();
        let width = st.doc.width() as usize;
        let ids: Vec<LayerId> = st.doc.layer_ids().collect();
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return };
        let layers: Vec<(LayerId, Vec<u8>)> = ids
            .into_iter()
            .filter_map(|id| canvas.read_layer(gpu, id).ok().map(|p| (id, flipped(&p, width, horizontal))))
            .collect();
        canvas.name_next_step(if horizontal { "flip canvas horizontally" } else { "flip canvas vertically" });
        canvas.write_layers(layers);
        st.revision += 1;
        self.shared.frame(st);
    }
}

impl CanvasSession {
    fn add_picture(&self, image: &ThumbnailImage, fit: bool) -> Option<u64> {
        let (w, h) = {
            let st = self.shared.lock();
            (st.doc.width(), st.doc.height())
        };
        let pixels = placed(image, w, h, fit)?;
        let id = self.add_layer();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if !st.doc.has_room(1) {
            return None;
        }
        if let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
            canvas.upload_layer(gpu, LayerId(id), &pixels).ok()?;
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
        let on = placed(&image, 8, 8, true).unwrap();
        // centred: rows 3 and 4, columns 2 to 5
        assert_eq!(on[(3 * 8 + 2) * 4], 200);
        assert_eq!(on[(3 * 8 + 1) * 4], 0);
        let big = ThumbnailImage { width: 16, height: 16, rgba: vec![9; 16 * 16 * 4] };
        let fitted = placed(&big, 8, 8, true).unwrap();
        assert!(fitted.iter().all(|&v| v == 9));
        assert!(placed(&ThumbnailImage { width: 3, height: 3, rgba: vec![] }, 8, 8, true).is_none());
    }
}
