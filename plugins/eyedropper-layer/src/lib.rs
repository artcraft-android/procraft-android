//! Eyedropper picks from layer: the eyedropper reads the current layer alone, not the whole canvas.
//! It's a plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of
//! it.
//!
//! The app asks one question and gives it nothing else (the plugin kit answers it). Reading a pixel
//! stays the app's: this only says which pixel. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![no_std]

/// Its one switch.
static CONTROLS: kit::Controls<1> = kit::Controls::new([0.0]);
kit::controls!(CONTROLS);

// whether the eyedropper reads the current layer: as the switch says
kit::yes!(procraft_pick_layer, || CONTROLS.on(0));

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
