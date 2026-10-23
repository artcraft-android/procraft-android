//! The window handed over from Kotlin, and the wgpu surface on it: the crate's only unsafe code.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use jni_sys::{JNIEnv, jclass, jlong, jobject};
use ndk::native_window::NativeWindow;
use procraft_gpu::wgpu;
use raw_window_handle::{AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle};

/// Windows acquired from Kotlin and not yet attached, by id.
static WINDOWS: Mutex<Vec<(u64, NativeWindow)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Takes the window registered as `id`.
pub(crate) fn take(id: u64) -> Option<NativeWindow> {
    let mut windows = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let at = windows.iter().position(|(i, _)| *i == id)?;
    Some(windows.swap_remove(at).1)
}

/// `NativeWindows.acquire(surface: Surface): Long`: registers the surface's native window and
/// returns its id (0 if there is none).
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_artcraftandroid_procraft_canvas_NativeWindows_acquire(
    env: *mut JNIEnv,
    _class: jclass,
    surface: jobject,
) -> jlong {
    if env.is_null() || surface.is_null() {
        return 0;
    }
    // SAFETY: the JVM calls this with its own valid `env`, and the Kotlin declaration only takes an
    // `android.view.Surface`.
    let Some(window) = (unsafe { NativeWindow::from_surface(env, surface) }) else { return 0 };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).push((id, window));
    jlong::try_from(id).unwrap_or(0)
}

/// `NativeWindows.release(id: Long)`: lets go of a window that was never attached.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_artcraftandroid_procraft_canvas_NativeWindows_release(
    _env: *mut JNIEnv,
    _class: jclass,
    id: jlong,
) {
    if let Ok(id) = u64::try_from(id) {
        drop(take(id));
    }
}

/// A wgpu surface on `window`. The caller keeps `window` alive until the surface is dropped.
pub(crate) fn create_surface(
    instance: &wgpu::Instance,
    window: &NativeWindow,
) -> Result<wgpu::Surface<'static>, String> {
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
        raw_window_handle: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(window.ptr().cast::<c_void>())),
    };
    // SAFETY: the handle is a live ANativeWindow, and the render thread, which owns both, drops
    // the surface before the window.
    unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())
}
