//! The colours painted with lately, newest first, the same in every app: the app keeps them (they
//! outlast an artwork) and hands them to each canvas, whose paint strokes add their colour as they
//! end. Colours are 0xRRGGBB.

use std::sync::{Arc, Mutex};

/// How many colours the history keeps.
pub const HISTORY_SIZE: usize = 10;

#[derive(Default, uniffi::Object)]
pub struct ColourHistory {
    colours: Mutex<Vec<u32>>,
}

#[uniffi::export]
impl ColourHistory {
    #[uniffi::constructor]
    pub fn new() -> Arc<ColourHistory> {
        Arc::new(ColourHistory::default())
    }

    /// Newest first.
    pub fn colours(&self) -> Vec<u32> {
        self.kept().clone()
    }

    pub fn clear(&self) {
        self.kept().clear();
    }

    /// The last colour painted with other than `current`, to go back to.
    pub fn previous(&self, current: u32) -> Option<u32> {
        let current = current & 0xFF_FFFF;
        self.kept().iter().copied().find(|&c| c != current)
    }
}

impl ColourHistory {
    fn kept(&self) -> std::sync::MutexGuard<'_, Vec<u32>> {
        self.colours.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A stroke was painted in `rgb`: it goes to the front.
    pub(crate) fn painted(&self, rgb: u32) {
        let rgb = rgb & 0xFF_FFFF;
        let mut kept = self.kept();
        kept.retain(|&c| c != rgb);
        kept.insert(0, rgb);
        kept.truncate(HISTORY_SIZE);
    }
}

/// An sRGB colour (0..1) as 0xRRGGBB.
pub(crate) fn hex([r, g, b]: [f32; 3]) -> u32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanvasSession, PenSample, ToolKind};

    #[test]
    fn paint_strokes_add_their_colour_newest_first() {
        let h = ColourHistory::new();
        for c in 0..12 {
            h.painted(c);
        }
        h.painted(5);
        assert_eq!(h.colours().len(), HISTORY_SIZE);
        assert_eq!(h.colours().first(), Some(&5));
        assert_eq!(h.colours().get(1), Some(&11));
        assert_eq!(h.previous(5), Some(11));
        h.clear();
        assert_eq!(h.previous(5), None);

        // a canvas given it adds a paint stroke's colour as the stroke ends; erasing adds none
        let s = CanvasSession::new(64, 64);
        s.use_colour_history(h.clone());
        let at =
            |ms| vec![PenSample { x: 1.0, y: 1.0, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: ms, roll: 0.0 }];
        s.set_color(1.0, 0.0, 0.0);
        s.stroke_end(at(0.0));
        assert_eq!(h.colours(), vec![0xFF0000]);
        s.set_tool(ToolKind::Erase);
        s.set_color(0.0, 0.0, 1.0);
        s.stroke_end(at(10.0));
        assert_eq!(h.colours(), vec![0xFF0000]);
    }
}
