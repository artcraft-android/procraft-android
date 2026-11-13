//! Panels stay open: a touch on the canvas with a panel open (the brushes, the colours, the layers)
//! paints at once and leaves the panel up, rather than closing it. It's a plugin, not part of the
//! app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else (the plugin kit answers it). The panels and
//! the painting stay the app's: this only says whether a touch waits for the panel. The interface
//! is written down at the top of `crates/core/src/plugin_run.rs`.

#![no_std]

// always, while the plugin is on
kit::yes!(procraft_pin_panels);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
