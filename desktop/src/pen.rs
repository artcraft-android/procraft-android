//! A pen's lean, its buttons and its eraser end, which winit leaves out of its touches: on Windows
//! read from the system's pointer messages as they reach the window (before winit makes its
//! touches of them), kept by pointer for the touches to carry.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Each pen pointer's last lean: radians from upright, and the direction it leans (radians
/// clockwise from screen-up), as the strokes take them.
static LEANS: Mutex<Option<HashMap<u64, (f32, f32)>>> = Mutex::new(None);

/// Each pen pointer's last flags (its barrel button held, its eraser end down).
static FLAGS: Mutex<Option<HashMap<u64, u32>>> = Mutex::new(None);

/// How many times a pen's barrel button went down since the app last asked.
static PRESSES: AtomicUsize = AtomicUsize::new(0);

#[cfg_attr(not(windows), allow(dead_code))]
const FLAG_BARREL: u32 = 0x1;
const FLAG_INVERTED: u32 = 0x2;
const FLAG_ERASER: u32 = 0x4;

/// The lean of the pen that is pointer `id` (a touch's id), if it is a pen that gives one.
pub fn lean(id: u64) -> Option<(f32, f32)> {
    LEANS.lock().ok()?.as_ref()?.get(&id).copied()
}

/// Whether the pen that is pointer `id` is turned over, its eraser end on the screen.
pub fn eraser_end(id: u64) -> bool {
    let flags = FLAGS.lock().ok().and_then(|f| f.as_ref()?.get(&id).copied()).unwrap_or(0);
    flags & (FLAG_INVERTED | FLAG_ERASER) != 0
}

/// How many times a pen's barrel button was pressed since the last call.
pub fn take_button_presses() -> usize {
    PRESSES.swap(0, Ordering::Relaxed)
}

/// Pointer `id`'s pen flags as they now are: a press of its barrel button counted.
#[cfg_attr(not(windows), allow(dead_code))]
fn pen_flags(id: u64, flags: u32) {
    let Ok(mut all) = FLAGS.lock() else { return };
    let before = all.get_or_insert_with(Default::default).insert(id, flags).unwrap_or(0);
    if flags & FLAG_BARREL != 0 && before & FLAG_BARREL == 0 {
        PRESSES.fetch_add(1, Ordering::Relaxed);
    }
}

/// The lean of a pen tilted `tilt_x` degrees towards the right and `tilt_y` towards the user
/// (each −90..90): its angle from upright and the direction its top leans, clockwise from up.
#[cfg_attr(not(windows), allow(dead_code))]
fn lean_of(tilt_x: f32, tilt_y: f32) -> (f32, f32) {
    // short of flat, where the tangent runs off
    let tangent = |degrees: f32| degrees.clamp(-89.9, 89.9).to_radians().tan();
    let (x, y) = (tangent(tilt_x), tangent(tilt_y));
    (x.hypot(y).atan(), x.atan2(-y).rem_euclid(std::f32::consts::TAU))
}

/// Starts reading the pen's lean from the pointer messages to `window`.
#[cfg(windows)]
pub fn watch(window: &winit::window::Window) {
    system::watch(window);
}

#[cfg(not(windows))]
pub fn watch(_window: &winit::window::Window) {}

/// The window's own procedure, put in front of winit's: unsafe as the system's calls are.
#[cfg(windows)]
#[allow(unsafe_code)]
mod system {
    use std::sync::atomic::{AtomicIsize, Ordering};

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::Pointer::{GetPointerPenInfo, POINTER_PEN_INFO};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GWLP_WNDPROC, SetWindowLongPtrW, WM_POINTERDOWN, WM_POINTERUPDATE, WNDPROC,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// What the pen info's tilts are given in (`penMask`).
    const PEN_MASK_TILT_X: u32 = 0x4;
    const PEN_MASK_TILT_Y: u32 = 0x8;

    /// winit's procedure for the window, called after ours.
    static WINIT: AtomicIsize = AtomicIsize::new(0);

    pub fn watch(window: &winit::window::Window) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else { return };
        let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
        let ours = pointer_messages as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
        // SAFETY: the window is this thread's own and lives as long as the app; winit's procedure,
        // kept, is called for every message after ours
        let winit = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours as usize as isize) };
        WINIT.store(winit, Ordering::Relaxed);
    }

    /// A pen's pointer message: its tilt kept by pointer, then the message on to winit.
    unsafe extern "system" fn pointer_messages(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_POINTERDOWN || msg == WM_POINTERUPDATE {
            // the pointer's id is the low word of `wparam`
            let id = (wparam.0 & 0xFFFF) as u32;
            let mut info = POINTER_PEN_INFO::default();
            // SAFETY: `info` is a pen info to fill; a pointer that isn't a pen fails and is left
            if unsafe { GetPointerPenInfo(id, &mut info) }.is_ok() {
                let tilted = info.penMask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y) != 0;
                let lean = if tilted { super::lean_of(info.tiltX as f32, info.tiltY as f32) } else { (0.0, 0.0) };
                if let Ok(mut leans) = super::LEANS.lock() {
                    leans.get_or_insert_with(Default::default).insert(u64::from(id), lean);
                }
                super::pen_flags(u64::from(id), info.penFlags);
            }
        }
        // SAFETY: the procedure winit set, as SetWindowLongPtrW gave it back
        let winit: WNDPROC = unsafe { std::mem::transmute::<isize, WNDPROC>(WINIT.load(Ordering::Relaxed)) };
        unsafe { CallWindowProcW(winit, hwnd, msg, wparam, lparam) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_barrel_button_counts_once_each_time_it_goes_down() {
        take_button_presses();
        pen_flags(901, 0);
        pen_flags(901, FLAG_BARREL);
        pen_flags(901, FLAG_BARREL);
        pen_flags(901, 0);
        pen_flags(901, FLAG_BARREL);
        assert_eq!(take_button_presses(), 2);
        assert!(!eraser_end(901));
        pen_flags(902, FLAG_INVERTED);
        assert!(eraser_end(902));
    }

    #[test]
    fn a_tilt_becomes_a_lean_clockwise_from_up() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let (tilt, _) = lean_of(0.0, 0.0);
        assert!(near(tilt, 0.0));
        // leaning right, towards the user, left, away
        for ((x, y), direction) in
            [((30.0, 0.0), 90.0), ((0.0, 30.0), 180.0), ((-30.0, 0.0), 270.0), ((0.0, -30.0), 0.0)]
        {
            let (tilt, towards) = lean_of(x, y);
            assert!(near(tilt, 30f32.to_radians()), "{x} {y}: {tilt}");
            assert!(near(towards, f32::to_radians(direction)), "{x} {y}: {towards}");
        }
        // flat on the screen, still leaning right
        let (tilt, towards) = lean_of(90.0, 0.0);
        assert!((tilt - std::f32::consts::FRAC_PI_2).abs() < 0.01 && near(towards, std::f32::consts::FRAC_PI_2));
    }
}
