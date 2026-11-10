//! QuickShape curves: a stroke that bends more than once, held, snaps to a curve through its
//! bends rather than a polyline, its points there to reshape it. It's a plugin, not part of the
//! app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else. Fitting and drawing the curve stay the
//! app's: this only says whether to. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![no_std]

/// Whether QuickShape snaps to curves: always, while the plugin is on.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_quickshape_curves() -> i32 {
    1
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
