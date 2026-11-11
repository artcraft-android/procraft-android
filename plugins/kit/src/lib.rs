//! The plugin kit: what a ProCraft plugin needs to talk to the app, so a plugin is ordinary Rust.
//!
//! A plugin is a WebAssembly module the app calls at its hooks (see `docs/plugins.md`). The calls
//! themselves are plain numbers and places in the module's memory; this kit writes them for you:
//!
//! ```ignore
//! static CONTROLS: kit::Controls<2> = kit::Controls::new([1.0, 0.5]);
//! kit::controls!(CONTROLS);
//! kit::inbetween!(|this, next, width, height| Some(my_frame(this, next, width, height)));
//! ```
//!
//! Each macro exports the functions its hook asks for, with the memory handed over and back taken
//! care of ([`picker!`] for the colour panel's Classic square, beside its `picture.wgsl`). A
//! plugin's actions (things it does when tapped, [`actions!`]) call the app's functions
//! in [`app`]: adding a layer, painting on it, setting the brush. Without the `std` feature only
//! [`Controls`], [`controls!`], [`yes!`], [`answer!`] and [`position!`] are there, for the smallest
//! plugins.

#![cfg_attr(not(feature = "std"), no_std)]

use core::sync::atomic::{AtomicU32, Ordering};

/// A plugin's controls, by their place in its manifest, each as the app last told it (the app
/// tells every one when the plugin starts, and again whenever one is moved).
pub struct Controls<const N: usize>([AtomicU32; N]);

impl<const N: usize> Controls<N> {
    /// The controls, starting at `starts` (as the manifest's `starts` say) until told.
    pub const fn new(starts: [f32; N]) -> Self {
        let mut slots = [const { AtomicU32::new(0) }; N];
        let mut i = 0;
        while i < N {
            slots[i] = AtomicU32::new(starts[i].to_bits());
            i += 1;
        }
        Controls(slots)
    }

    /// Control `index`'s value: a slider's, or a switch's (1 on, 0 off), a file's (1 while one is
    /// picked); 0 for one there isn't.
    pub fn get(&self, index: usize) -> f32 {
        self.0.get(index).map_or(0.0, |slot| f32::from_bits(slot.load(Ordering::Relaxed)))
    }

    /// Whether switch `index` is on (or a file is picked for it).
    pub fn on(&self, index: usize) -> bool {
        self.get(index) >= 0.5
    }

    /// Control `index` told `value` by the app; false if there is no such control or it is no
    /// number.
    pub fn told(&self, index: i32, value: f32) -> bool {
        match usize::try_from(index).ok().and_then(|i| self.0.get(i)) {
            Some(slot) if value.is_finite() => {
                slot.store(value.to_bits(), Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }
}

/// Exports `procraft_control`, through which the app tells `$controls` (a [`Controls`]); with a
/// closure, it is also called with each control told (its place and value).
#[macro_export]
macro_rules! controls {
    ($controls:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_control(index: i32, value: f32) {
            $controls.told(index, value);
        }
    };
    ($controls:path, $then:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_control(index: i32, value: f32) {
            if $controls.told(index, value) {
                let then: fn(usize, f32) = $then;
                then(index as usize, value);
            }
        }
    };
}

/// Exports a hook that only asks yes or no (`procraft_pin_panels`, `procraft_fill_all_layers`,
/// ...), answering what `$answer` (a `fn() -> bool`, or `true`) says.
#[macro_export]
macro_rules! yes {
    ($name:ident) => {
        $crate::yes!($name, || true);
    };
    ($name:ident, $answer:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() -> i32 {
            let answer: fn() -> bool = $answer;
            i32::from(answer())
        }
    };
}

/// Exports a hook that asks for a whole number (`procraft_radial_segments`, ...), answering what
/// `$answer` (a `fn() -> i32`) says.
#[macro_export]
macro_rules! answer {
    ($name:ident, $answer:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name() -> i32 {
            let answer: fn() -> i32 = $answer;
            answer()
        }
    };
}

/// Exports the position hook: `$moved` (a `fn(x: f32, y: f32) -> (f32, f32)`) says where a sample
/// at (x, y), window pixels, lands.
#[macro_export]
macro_rules! position {
    ($moved:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_position(x: f32, y: f32) -> i64 {
            let moved: fn(f32, f32) -> (f32, f32) = $moved;
            let (to_x, to_y) = moved(x, y);
            (u64::from(to_x.to_bits()) | (u64::from(to_y.to_bits()) << 32)) as i64
        }
    };
}

/// Exports the picker hook: `$pick` (a `fn(x: f32, y: f32) -> (f32, f32)`) says the saturation and
/// brightness a point of Classic's square picks, and `$point` (a `fn(s: f32, v: f32) -> (f32, f32)`)
/// where they sit on it; points are 0..1 across and down. The picture is the plugin's
/// `picture.wgsl`.
#[macro_export]
macro_rules! picker {
    ($pick:expr, $point:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_picker_pick(x: f32, y: f32) -> i64 {
            let pick: fn(f32, f32) -> (f32, f32) = $pick;
            let (s, v) = pick(x, y);
            (u64::from(s.to_bits()) | (u64::from(v.to_bits()) << 32)) as i64
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn procraft_picker_point(s: f32, v: f32) -> i64 {
            let point: fn(f32, f32) -> (f32, f32) = $point;
            let (x, y) = point(s, v);
            (u64::from(x.to_bits()) | (u64::from(y.to_bits()) << 32)) as i64
        }
    };
}

#[cfg(feature = "std")]
pub mod app;
#[cfg(feature = "std")]
pub mod canvas;
#[cfg(feature = "std")]
mod with_std;
#[cfg(feature = "std")]
pub use with_std::*;
