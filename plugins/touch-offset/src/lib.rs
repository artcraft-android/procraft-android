//! Touch position offset: moves where a stroke lands by two sliders, for a pen whose ink doesn't
//! sit under its tip. It's a plugin, not part of the app: it builds to a WebAssembly module, and
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app tells it the sliders and asks where each sample lands (the plugin kit passes them); the
//! interface is written down at the top of `crates/core/src/plugin_run.rs`.

#![no_std]

/// The sliders, across and down, in window pixels.
static CONTROLS: kit::Controls<2> = kit::Controls::new([0.0, 0.0]);
kit::controls!(CONTROLS);

// where a sample lands: moved by the sliders
kit::position!(|x, y| (x + CONTROLS.get(0), y + CONTROLS.get(1)));

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
