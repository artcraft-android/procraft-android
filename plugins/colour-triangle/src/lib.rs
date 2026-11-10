//! Colour triangle: the colour panel's Classic tab picks saturation and brightness in a triangle,
//! between the pure hue, white and black, instead of a square. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else. Drawing the picker stays the app's: this
//! only says which shape. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![no_std]

/// Whether Classic shows a triangle: always, while the plugin is on.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_colour_triangle() -> i32 {
    1
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
