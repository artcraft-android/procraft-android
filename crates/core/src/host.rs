//! The session in a window its caller owns and draws (the desktop app): the caller makes the
//! device and the surface, draws each frame and presents it.

use std::time::{Duration, Instant};

use procraft_engine::profile::Profile;
use procraft_gpu::{Gpu, wgpu};

use crate::CanvasSession;

impl CanvasSession {
    /// Paints with `gpu` (made for the caller's surface): the canvas is made on it now.
    pub fn use_gpu(&self, gpu: Gpu) {
        self.shared.lock().gpu.get_or_insert(gpu);
        self.shared.make_canvas();
    }

    /// The window is `width` × `height` pixels: the canvas is placed in it the first time.
    pub fn show_in(&self, width: u32, height: u32) {
        let mut st = self.shared.lock();
        st.window_size = (width.max(1), height.max(1));
        st.place_view();
        st.needs_frame = true;
    }

    /// Draws a frame into `view` (`format`, `size` window pixels, its colours shown in `screen`),
    /// which the caller then presents. `started` is when drawing began. How soon another frame is
    /// wanted, if one is: at once while the view settles or caches are remade, later while the
    /// selection's bands drift (when they have moved a pixel).
    pub fn draw(
        &self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        screen: Profile,
        started: Instant,
    ) -> Option<Duration> {
        let mut st = self.shared.lock();
        // being redrawn a stroke at a time: the picture as it was until that is done
        if st.holding_frames {
            return None;
        }
        st.draw_frame(view, format, size, screen, started);
        if st.needs_frame {
            return Some(Duration::ZERO);
        }
        st.frame_due.map(|at| at.saturating_duration_since(Instant::now()))
    }

    /// Something changed since the last frame was drawn.
    pub fn wants_frame(&self) -> bool {
        self.shared.lock().needs_frame
    }

    /// Draws the whole canvas fitted and centred on black into `view` (a second display's
    /// window, `format`, `size` pixels, its colours shown in `screen`): Project canvas.
    pub fn draw_projection(
        &self,
        view: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        screen: Profile,
    ) {
        self.shared.lock().draw_fitted(view, format, size, screen);
    }

    /// How many frames of the canvas have been drawn: a second display showing it is drawn again
    /// when this changes.
    pub fn frames_drawn(&self) -> u64 {
        self.shared.lock().frames
    }
}
