//! Project canvas: the whole picture, without the interface, on a second display.

use crate::{CanvasSession, MobileError};

#[uniffi::export]
impl CanvasSession {
    /// Shows the canvas fitted and centred on black in the window `window` (an id from
    /// `NativeWindows.acquire`), `width` × `height` pixels, on a second display. The session
    /// owns the window from here on.
    pub fn attach_projection(&self, window: u64, width: u32, height: u32) -> Result<(), MobileError> {
        self.detach_projection();
        #[cfg(target_os = "android")]
        {
            let native = crate::window::take(window)
                .ok_or_else(|| MobileError::Window { reason: format!("no window {window}") })?;
            {
                let mut st = self.shared.lock();
                st.projection_size = (width.max(1), height.max(1));
                st.projection_stop = false;
            }
            let shared = std::sync::Arc::clone(&self.shared);
            let handle = std::thread::Builder::new()
                .name("procraft-projection".to_owned())
                .spawn(move || crate::render::project(shared, native))
                .map_err(|e| MobileError::Window { reason: e.to_string() })?;
            *self.shared.projection.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
            Ok(())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (width, height);
            Err(MobileError::Window { reason: format!("windows are Android-only (asked for {window})") })
        }
    }

    pub fn resize_projection(&self, width: u32, height: u32) {
        self.shared.lock().projection_size = (width.max(1), height.max(1));
    }

    /// Lets go of the second display's window; returns once its thread has finished.
    pub fn detach_projection(&self) {
        let handle = self.shared.projection.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(handle) = handle {
            self.shared.lock().projection_stop = true;
            let _ = handle.join();
            self.shared.lock().projection_stop = false;
        }
    }
}
