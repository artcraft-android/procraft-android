//! Tweening: Animation Assist's frame options get In-between, a frame the app draws between a
//! frame and the next: both drawings slid toward each other's place and size, and faded from one to
//! the other. This says whether they slide and how near the next frame the in-between falls.
//! It's a plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

/// The controls (an f32's bits): whether the drawings slide (0 or 1), how near the next frame.
static SETTINGS: [AtomicU32; 2] = [
    AtomicU32::new(0x3F80_0000), // on
    AtomicU32::new(0x3F00_0000), // halfway
];

/// A control's value, by its place in the manifest.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(index: i32, value: f32) {
    let Some(slot) = usize::try_from(index).ok().and_then(|i| SETTINGS.get(i)) else { return };
    if value.is_finite() {
        slot.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// The in-betweens' setting `setting`: whether the drawings slide, how near the next frame.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_tween(setting: i32) -> f32 {
    let Some(slot) = usize::try_from(setting).ok().and_then(|i| SETTINGS.get(i)) else { return -1.0 };
    f32::from_bits(slot.load(Ordering::Relaxed)).clamp(0.0, 1.0)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
