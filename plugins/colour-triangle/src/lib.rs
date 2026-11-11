//! Colour triangle: the colour panel's Classic tab picks saturation and brightness in a triangle,
//! between the pure hue, white and black, instead of a square. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else (the plugin kit answers it). Drawing the
//! picker stays the app's: this only says which shape. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![no_std]

// always, while the plugin is on
kit::yes!(procraft_colour_triangle);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
