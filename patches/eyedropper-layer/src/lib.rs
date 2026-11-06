//! Eyedropper picks from layer: the eyedropper reads the current layer alone, not the whole canvas.
//! It's a patch, not part of the app: `tools/pack_patch.py` makes a `.procraftplugin` file of it.
//!
//! The app asks one question and gives it nothing else. Reading a pixel stays the app's: this only
//! says which pixel. The interface is written down at the top of `crates/core/src/patch_run.rs`.

#![no_std]

use core::sync::atomic::{AtomicBool, Ordering};

/// Its one switch.
static LAYER: AtomicBool = AtomicBool::new(false);

/// The switch's value (the only control, so `index` says nothing).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(_index: i32, value: f32) {
    LAYER.store(value != 0.0, Ordering::Relaxed);
}

/// Whether the eyedropper reads the current layer.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_pick_layer() -> i32 {
    i32::from(LAYER.load(Ordering::Relaxed))
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
