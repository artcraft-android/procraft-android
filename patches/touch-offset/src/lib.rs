//! Touch position offset: moves where a stroke lands by two sliders, for a pen whose ink doesn't
//! sit under its tip. It's a patch, not part of the app: it builds to a WebAssembly module, and
//! `tools/pack_patch.py` makes a `.procraftplugin` file of it.
//!
//! The app calls the two functions below and gives it nothing else. They are its whole interface,
//! written down at the top of `crates/core/src/patch_run.rs`.

#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

/// The sliders, in window pixels, as an `f32`'s bits. The app makes its calls one at a time; these
/// are atomics so that nothing here needs `unsafe`.
static ACROSS: AtomicU32 = AtomicU32::new(0);
static DOWN: AtomicU32 = AtomicU32::new(0);

/// A slider's value, by its place in `craft-extension.json`.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(index: i32, value: f32) {
    let slider = if index == 0 { &ACROSS } else { &DOWN };
    slider.store(value.to_bits(), Ordering::Relaxed);
}

/// Where a sample at (x, y) lands: x's bits in the low half, y's in the high.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_position(x: f32, y: f32) -> i64 {
    let moved_x = x + f32::from_bits(ACROSS.load(Ordering::Relaxed));
    let moved_y = y + f32::from_bits(DOWN.load(Ordering::Relaxed));
    (u64::from(moved_x.to_bits()) | u64::from(moved_y.to_bits()) << 32) as i64
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
