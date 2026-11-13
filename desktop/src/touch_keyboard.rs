//! The system's on-screen keyboard, brought up when a text field (a brush's or a layer's name, a
//! text layer, a number typed in) is focused by a finger or a pen, and put away when the field
//! lets go: a tablet without its keyboard has no other way to type. Windows only: elsewhere the
//! system does it itself, or has none.

use std::time::{Duration, Instant};

/// How soon after a touch a mouse press is the system's copy of it, not the mouse's own.
const MOUSE_COPY: Duration = Duration::from_millis(500);

/// Whether the last press on the window was a finger's or a pen's (and when a touch last began),
/// whether a field was taking typing at the last frame, and whether the keyboard was brought up
/// for it.
#[derive(Default)]
pub struct TouchKeyboard {
    by_touch: bool,
    touched: Option<Instant>,
    typing: bool,
    shown: bool,
}

impl TouchKeyboard {
    /// A finger or the system's pen touched the window.
    pub fn touched(&mut self, now: Instant) {
        self.by_touch = true;
        self.touched = Some(now);
    }

    /// A mouse button was pressed: by a tablet's pen moving the mouse (`pen`), or the mouse itself,
    /// unless it is the system's copy of a touch that has just begun.
    pub fn clicked(&mut self, pen: bool, now: Instant) {
        if pen {
            self.by_touch = true;
        } else if self.touched.is_none_or(|t| now.saturating_duration_since(t) > MOUSE_COPY) {
            self.by_touch = false;
        }
    }

    /// Once a frame, whether a field takes typing now: the keyboard comes up as a field focused by
    /// a finger or a pen starts taking it, and goes as the field stops, if it came up for it.
    pub fn frame(&mut self, window: &winit::window::Window, typing: bool) {
        match self.wanted(typing) {
            Some(true) => self.shown = system::show(window, true),
            Some(false) => {
                system::show(window, false);
            }
            None => {}
        }
    }

    /// What the keyboard should do as a field takes typing (`typing`) or not: come up (true), go
    /// (false), or stay as it is (none).
    fn wanted(&mut self, typing: bool) -> Option<bool> {
        let was = std::mem::replace(&mut self.typing, typing);
        if typing && !was && self.by_touch {
            return Some(true);
        }
        if !typing && was && std::mem::take(&mut self.shown) {
            return Some(false);
        }
        None
    }
}

#[cfg(windows)]
mod system {
    use windows::UI::ViewManagement::InputPane;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IInputPaneInterop;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Brings the keyboard up (`up`) or puts it away for `window`: whether it did.
    #[allow(unsafe_code)]
    pub fn show(window: &winit::window::Window, up: bool) -> bool {
        let Ok(handle) = window.window_handle() else { return false };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else { return false };
        let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
        // the input pane of this window, through its interop (a desktop app has no view of its own)
        let pane = windows::core::factory::<InputPane, IInputPaneInterop>()
            // SAFETY: the window is this thread's own and lives as long as the app
            .and_then(|interop| unsafe { interop.GetForWindow::<InputPane>(hwnd) });
        match pane.and_then(|pane| if up { pane.TryShow() } else { pane.TryHide() }) {
            Ok(done) => done,
            Err(e) => {
                log::info!("the on-screen keyboard: {e}");
                false
            }
        }
    }
}

#[cfg(not(windows))]
mod system {
    pub fn show(_window: &winit::window::Window, _up: bool) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyboard_comes_up_for_a_field_tapped_by_a_finger_or_pen_and_goes_with_it() {
        let mut k = TouchKeyboard::default();
        let t0 = Instant::now();
        // clicked with the mouse: none
        k.clicked(false, t0);
        assert_eq!(k.wanted(true), None);
        assert_eq!(k.wanted(false), None);
        // tapped (the system's copy of the tap as a click straight after counts as the tap): up as
        // the field takes typing, nothing more while it does, away as it stops
        k.touched(t0);
        k.clicked(false, t0 + Duration::from_millis(20));
        assert_eq!(k.wanted(false), None);
        assert_eq!(k.wanted(true), Some(true));
        k.shown = true;
        assert_eq!(k.wanted(true), None);
        assert_eq!(k.wanted(false), Some(false));
        // one that didn't come up (the system said no) isn't put away
        assert_eq!(k.wanted(true), Some(true));
        assert_eq!(k.wanted(false), None);
        // the mouse later: none; a tablet's pen moving the mouse: up
        k.clicked(false, t0 + Duration::from_secs(5));
        assert_eq!(k.wanted(true), None);
        assert_eq!(k.wanted(false), None);
        k.clicked(true, t0 + Duration::from_secs(6));
        assert_eq!(k.wanted(true), Some(true));
    }
}
