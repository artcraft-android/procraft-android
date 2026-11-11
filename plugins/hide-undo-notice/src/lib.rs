//! Hide undo notices: undo and redo go by without the notice at the top of the screen that names
//! what they took back or brought back. It's a plugin, not part of the app: `tools/pack_plugin.py`
//! makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else (the plugin kit answers it). Undo and redo
//! stay the app's: this only says whether their notice shows. The interface is written down at the
//! top of `crates/core/src/plugin_run.rs`.

#![no_std]

// always, while the plugin is on
kit::yes!(procraft_hide_undo_notice);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
