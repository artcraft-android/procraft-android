//! The HarmonyOS app's bridge to the core. Its ArkTS interface imports `libprocraft_harmony.so`
//! and calls what this exports through Node-API (napi-ohos), as the Android app calls the core
//! through UniFFI's Kotlin bindings and the iPad app through its Swift ones. UniFFI has no ArkTS
//! bindings, so the calls the interface needs are written here, each a thin wrapper of the core's
//! own (see harmony/README.md for how this is meant to grow).
//!
//! The canvas draws straight into the interface's canvas component (an ArkUI `XComponent` of type
//! SURFACE): ArkTS passes its surface id to `CanvasSession.attachSurface` (`canvas.rs`), the core
//! makes the native window from it (`procraft_core::acquire_surface`) and its render thread draws
//! into it, as on the other tablets.

#[cfg_attr(not(target_env = "ohos"), allow(dead_code))]
mod pen;

#[cfg(target_env = "ohos")]
mod canvas;
