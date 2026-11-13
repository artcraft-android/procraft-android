//! Opacity of picked layers: with several layers picked in the Layers panel, changing one's opacity
//! (its Opacity slider, or Layer Opacity) sets them all to the same, as one undo step. It's a
//! plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else (the plugin kit answers it). The layers stay
//! the app's: this only says whether the picked ones go together. The interface is written down at
//! the top of `crates/core/src/plugin_run.rs`.

#![no_std]

// always, while the plugin is on
kit::yes!(procraft_opacity_together);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
