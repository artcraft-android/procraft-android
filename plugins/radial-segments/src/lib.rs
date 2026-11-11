//! Radial symmetry segments: the Drawing Guide's radial symmetry makes as many parts as its slider
//! says (2 to 32; an odd count mirrored is made even), not only eight. It's a plugin, not part of
//! the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else (the plugin kit answers it). Drawing the
//! copies stays the app's: this only says how many. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![no_std]

/// Its one slider: how many parts.
static CONTROLS: kit::Controls<1> = kit::Controls::new([8.0]);
kit::controls!(CONTROLS);

// how many parts radial symmetry makes, 2 to 32
kit::answer!(procraft_radial_segments, || (CONTROLS.get(0) as i32).clamp(2, 32));

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
