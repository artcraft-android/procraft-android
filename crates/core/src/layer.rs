//! The iPad app's window: the `CAMetalLayer` its canvas view hands over from Swift, and the wgpu
//! surface on it. With Android's (`window.rs`), the crate's only unsafe code; the crate root names
//! this module `window` on iPadOS, so the render thread takes either alike.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use procraft_gpu::wgpu;

/// A `CAMetalLayer` the Swift app handed over, held (retained) until the render thread lets go.
pub(crate) struct Layer(Retained<AnyObject>);

// SAFETY: a Metal layer's drawables are asked for and presented from a render thread of its own,
// as Metal apps do; the object itself is only retained and released here, which any thread may do
unsafe impl Send for Layer {}

/// Layers handed over from Swift and not yet attached, by id.
static LAYERS: Mutex<Vec<(u64, Layer)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Takes the layer registered as `id`.
pub(crate) fn take(id: u64) -> Option<Layer> {
    let mut layers = LAYERS.lock().unwrap_or_else(|e| e.into_inner());
    let at = layers.iter().position(|(i, _)| *i == id)?;
    Some(layers.swap_remove(at).1)
}

/// Registers the canvas view's `CAMetalLayer`, `layer` being a pointer to it that carries a retain
/// for the core (`Unmanaged.passRetained(layer).toOpaque()` in Swift), and returns its id for
/// [`crate::CanvasSession::attach_window`]: 0 if it isn't a Metal layer. The core lets go of it
/// with the window.
#[uniffi::export]
pub fn acquire_layer(layer: u64) -> u64 {
    let Some(pointer) = NonNull::new(layer as usize as *mut AnyObject) else { return 0 };
    // SAFETY: the Swift app passes a live object with a retain of its own, which this takes over
    // (released when dropped)
    let Some(layer) = (unsafe { Retained::from_raw(pointer.as_ptr()) }) else { return 0 };
    let Some(metal) = AnyClass::get(c"CAMetalLayer") else { return 0 };
    // SAFETY: `isKindOfClass:` asked of an object, with a class
    let is_metal: bool = unsafe { msg_send![&*layer, isKindOfClass: metal] };
    if !is_metal {
        return 0;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    LAYERS.lock().unwrap_or_else(|e| e.into_inner()).push((id, Layer(layer)));
    id
}

/// Lets go of a layer that was never attached.
#[uniffi::export]
pub fn release_layer(id: u64) {
    drop(take(id));
}

/// A wgpu surface on `layer`.
pub(crate) fn create_surface(instance: &wgpu::Instance, layer: &Layer) -> Result<wgpu::Surface<'static>, String> {
    let target = wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(Retained::as_ptr(&layer.0).cast_mut().cast::<c_void>());
    // SAFETY: a live CAMetalLayer (checked as it was handed over), which the surface retains too
    unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())
}
