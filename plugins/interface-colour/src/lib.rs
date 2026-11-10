//! Interface colour: the interface's accent, the blue of what is chosen and switched on, in a hue
//! and saturation of one's own, as bright as the usual blue. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks for a colour by its role and gives it nothing else. Drawing stays the app's: this
//! only says which colour. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

/// The sliders' values (an f32's bits): the hue in degrees and the saturation, 0 to 1. At their
/// start they make the usual blue, #0076FD.
static HUE: AtomicU32 = AtomicU32::new(0x4354_0000); // 212.0
static SATURATION: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0
/// How bright: the usual blue's, 253 of 255.
const BRIGHTNESS: f32 = 253.0 / 255.0;

/// A slider's value, by its place in the manifest: 0 the hue, 1 the saturation.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(index: i32, value: f32) {
    if !value.is_finite() {
        return;
    }
    match index {
        0 => HUE.store(value.clamp(0.0, 360.0).to_bits(), Ordering::Relaxed),
        1 => SATURATION.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed),
        _ => {}
    }
}

/// The accent (role 0) as 0xRRGGBB; any other role left as the app has it.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_interface_colour(role: i32) -> i32 {
    if role != 0 {
        return -1;
    }
    let hue = f32::from_bits(HUE.load(Ordering::Relaxed));
    let saturation = f32::from_bits(SATURATION.load(Ordering::Relaxed));
    let [r, g, b] = rgb(hue, saturation, BRIGHTNESS);
    (channel(r) << 16) | (channel(g) << 8) | channel(b)
}

/// A colour by its hue (degrees), saturation and brightness (0 to 1), as red, green and blue.
fn rgb(hue: f32, saturation: f32, brightness: f32) -> [f32; 3] {
    let chroma = brightness * saturation;
    let sector = (hue % 360.0) / 60.0;
    let whole = sector as i32;
    let rising = chroma * (1.0 - abs((sector - (whole - whole % 2) as f32) - 1.0));
    let grey = brightness - chroma;
    let [r, g, b] = match whole {
        0 => [chroma, rising, 0.0],
        1 => [rising, chroma, 0.0],
        2 => [0.0, chroma, rising],
        3 => [0.0, rising, chroma],
        4 => [rising, 0.0, chroma],
        _ => [chroma, 0.0, rising],
    };
    [r + grey, g + grey, b + grey]
}

fn abs(v: f32) -> f32 {
    if v < 0.0 { -v } else { v }
}

/// 0 to 1 as 0 to 255, to the nearest.
fn channel(v: f32) -> i32 {
    let v = v * 255.0 + 0.5;
    (if v < 0.0 { 0.0 } else if v > 255.0 { 255.0 } else { v }) as i32
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
