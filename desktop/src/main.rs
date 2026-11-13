//! ProCraft on the desktop: the app (`lib.rs`) in a window of its own.

// no console window behind the app on Windows (a debug build keeps it, for the log)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_env = "ohos"))]
fn main() {
    procraft_desktop::run();
}

// on HarmonyOS the app is a library the canvas component loads (harmony/bridge), not a program
#[cfg(target_env = "ohos")]
fn main() {}
