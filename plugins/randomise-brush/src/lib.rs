//! Brush randomiser: a Randomise button in the brush studio sets the brush's settings at random,
//! within bounds that keep it something that still paints. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks about each setting by its number (the list is in the plugin guide) with a seed
//! for the press, and sets what comes back. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![no_std]

/// Where each setting may go, as a share of its range: (least, most), by the setting's number. A
/// share below 0 leaves the setting as it is.
const BOUNDS: [(f32, f32); 36] = [
    (0.02, 0.45), // spacing: never so wide the stamps fall apart
    (0.0, 0.8),   // streamline
    (0.0, 1.0),   // streamline pressure
    (0.0, 0.5),   // pen taper start
    (0.0, 0.5),   // pen taper end
    (0.0, 1.0),   // pen taper size
    (0.0, 1.0),   // pen taper opacity
    (0.0, 0.5),   // touch taper start
    (0.0, 0.5),   // touch taper end
    (0.0, 1.0),   // touch taper size
    (0.0, 1.0),   // touch taper opacity
    (-1.0, -1.0), // input style: left as it is
    (0.0, 1.0),   // shape rotation
    (0.0, 0.6),   // scatter
    (0.0, 0.3),   // count
    (0.0, 1.0),   // randomised
    (0.3, 1.0),   // roundness
    (0.0, 1.0),   // shape angle
    (0.0, 1.0),   // grain behaviour
    (0.05, 0.6),  // grain scale
    (0.0, 1.0),   // grain depth
    (-1.0, -1.0), // rendering mode: left as it is
    (0.4, 1.0),   // flow
    (0.0, 0.5),   // size jitter
    (0.0, 0.5),   // opacity jitter
    (0.5, 1.0),   // pressure size
    (0.5, 1.0),   // pressure opacity
    (0.0, 1.0),   // tilt angle
    (0.0, 1.0),   // tilt opacity
    (0.0, 1.0),   // tilt size
    (0.3, 0.9),   // max size
    (0.0, 0.3),   // min size
    (0.7, 1.0),   // max opacity
    (0.0, 0.5),   // min opacity
    (0.0, 1.0),   // smudge pull
    (0.0, 1.0),   // wet pull
];

/// Where setting `setting` goes this press.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_randomise(setting: i32, seed: i32) -> f32 {
    let Some(&(least, most)) = usize::try_from(setting).ok().and_then(|i| BOUNDS.get(i)) else { return -1.0 };
    if least < 0.0 {
        return -1.0;
    }
    least + (most - least) * unit(seed as u32, setting as u32)
}

/// A number from 0 to 1 that `seed` and `setting` stir up (a splitmix-style mix).
fn unit(seed: u32, setting: u32) -> f32 {
    let mut x = seed ^ setting.wrapping_mul(0x9E37_79B9);
    x = (x ^ (x >> 16)).wrapping_mul(0x7FEB_352D);
    x = (x ^ (x >> 15)).wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
