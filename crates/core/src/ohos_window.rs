//! The HarmonyOS app's window: the `OHNativeWindow` of its canvas component's surface (an ArkUI
//! `XComponent` of type SURFACE), and the wgpu surface on it. With Android's (`window.rs`) and the
//! iPad app's (`layer.rs`), the crate's only unsafe code; the crate root names this module `window`
//! on OpenHarmony, so the render thread takes it as it takes the others.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use procraft_gpu::wgpu;
use raw_window_handle::{OhosDisplayHandle, OhosNdkWindowHandle, RawDisplayHandle, RawWindowHandle};

// libnative_window.so, in the OpenHarmony SDK's sysroot and on every device (API 12 and later for
// the surface id call)
#[link(name = "native_window")]
unsafe extern "C" {
    fn OH_NativeWindow_CreateNativeWindowFromSurfaceId(surface_id: u64, window: *mut *mut c_void) -> i32;
    fn OH_NativeWindow_DestroyNativeWindow(window: *mut c_void);
}

/// A native window made from a surface id, held until the render thread lets go (its reference
/// is dropped then).
pub(crate) struct NativeWindow(NonNull<c_void>);

// SAFETY: the window is only ever used by one thread at a time: made on the interface's thread,
// then moved to the render thread, which draws into it and drops it
unsafe impl Send for NativeWindow {}

impl Drop for NativeWindow {
    fn drop(&mut self) {
        // SAFETY: a window this module made, its reference dropped once
        unsafe { OH_NativeWindow_DestroyNativeWindow(self.0.as_ptr()) }
    }
}

/// Windows made from the app's surfaces and not yet attached, by id.
static WINDOWS: Mutex<Vec<(u64, NativeWindow)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Takes the window registered as `id`.
pub(crate) fn take(id: u64) -> Option<NativeWindow> {
    let mut windows = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let at = windows.iter().position(|(i, _)| *i == id)?;
    Some(windows.swap_remove(at).1)
}

/// Makes the native window of the surface `surface_id` (the canvas component's, from
/// `XComponentController.getXComponentSurfaceId()` or `onSurfaceCreated`; a surface of this
/// process) and returns its id for [`crate::CanvasSession::attach_window`]: 0 if there is none.
pub fn acquire_surface(surface_id: u64) -> u64 {
    let mut raw: *mut c_void = std::ptr::null_mut();
    // SAFETY: `raw` is a valid place for the window pointer the call writes
    let status = unsafe { OH_NativeWindow_CreateNativeWindowFromSurfaceId(surface_id, &mut raw) };
    let Some(window) = NonNull::new(raw) else { return 0 };
    let window = NativeWindow(window);
    if status != 0 {
        return 0;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).push((id, window));
    id
}

/// Lets go of a window that was never attached.
pub fn release_surface(id: u64) {
    drop(take(id));
}

/// A wgpu surface on `window`. The caller keeps `window` alive until the surface is dropped.
pub(crate) fn create_surface(
    instance: &wgpu::Instance,
    window: &NativeWindow,
) -> Result<wgpu::Surface<'static>, String> {
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(RawDisplayHandle::Ohos(OhosDisplayHandle::new())),
        raw_window_handle: RawWindowHandle::OhosNdk(OhosNdkWindowHandle::new(window.0)),
    };
    // SAFETY: the handle is a live OHNativeWindow, and the render thread, which owns both, drops
    // the surface before the window.
    unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())
}
