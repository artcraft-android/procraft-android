//! Brush sounds: a sound as you draw, behaving as Krita's Brush SFX plugin does: each brush the
//! sound its kind takes, louder the faster you go. The app makes the sound and plays it; this sets
//! its master volume, (if asked) one sound for every brush, whether erasing sounds and how, and
//! which sounds play a recording someone imported in place of their own.
//! It's a plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

/// The controls (an f32's bits), in the manifest's order: the volume (0 to 1), the sound (0 each
/// brush's kind's, 1 to 6 one for all), the eraser switch (0 or 1), erasing's sound (1 to 6), and
/// for each of the six sounds whether a recording is imported (0 or 1).
static SETTINGS: [AtomicU32; 10] = [
    AtomicU32::new(0x3F80_0000), // 1.0
    AtomicU32::new(0),           // by brush
    AtomicU32::new(0x3F80_0000), // on
    AtomicU32::new(0x3F80_0000), // the eraser's
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

/// Where the six recordings' controls start in the manifest.
const RECORDINGS: usize = 4;

/// A control's value, by its place in the manifest.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_control(index: i32, value: f32) {
    let Some(slot) = usize::try_from(index).ok().and_then(|i| SETTINGS.get(i)) else { return };
    if value.is_finite() {
        slot.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// The sound's setting `setting`: the volume, the eraser switch, the sound and erasing's sound in
/// sixths, then each sound's recording.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_stroke_sound(setting: i32) -> f32 {
    let value = |i: usize| f32::from_bits(SETTINGS[i].load(Ordering::Relaxed));
    match setting {
        0 => value(0).clamp(0.0, 1.0),
        1 => value(2).clamp(0.0, 1.0),
        2 => (value(1) / 6.0).clamp(0.0, 1.0),
        3 => (value(3) / 6.0).clamp(0.0, 1.0),
        // each sound's recording, if one is imported: its control's place
        4..=9 => {
            let at = RECORDINGS + (setting - 4) as usize;
            if value(at) >= 0.5 { at as f32 } else { -1.0 }
        }
        _ => -1.0,
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
