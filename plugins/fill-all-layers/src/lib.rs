//! ColorDrop fills by all layers: a fill dropped on a layer goes up to the lines of every layer you
//! see, as if they were one, not only to the current layer's. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else (the plugin kit answers it). Reading the
//! layers and filling stay the app's: this only says which to go by. The interface is written down
//! at the top of `crates/core/src/plugin_run.rs`.

#![no_std]

// always, while the plugin is on
kit::yes!(procraft_fill_all_layers);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
