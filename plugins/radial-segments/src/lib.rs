//! Radial symmetry segments: the Drawing Guide's radial symmetry makes as many parts as its slider
//! says (2 to 32; an odd count mirrored is made even), not only eight. It's a plugin, not part of the
//! app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else. Drawing the copies stays the app's: this
//! only says how many. The interface is written down at the top of `crates/core/src/plugin_run.rs`.

#![no_std]

use core::sync::atomic::{AtomicI32, Ordering};

/// Its one slider, as a whole number of parts.
static PARTS: AtomicI32 = AtomicI32::new(8);

/// The slider's value (the only control, so `index` says nothing).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(_index: i32, value: f32) {
    let parts = if value.is_finite() { value as i32 } else { 8 };
    PARTS.store(parts.clamp(2, 32), Ordering::Relaxed);
}

/// How many parts radial symmetry makes.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_radial_segments() -> i32 {
    PARTS.load(Ordering::Relaxed)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
