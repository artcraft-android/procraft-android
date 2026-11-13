# ProCraft for HarmonyOS (starter)

The beginnings of ProCraft on HarmonyOS 5 and later (HarmonyOS NEXT), which runs no Android
apps. Huawei tablets on HarmonyOS 2 to 4 already run the Android app. Like the Android and iPad
apps, it is a native interface over the shared Rust core. Here the interface is ArkTS/ArkUI and the
core is reached through Node-API.

**Status: a skeleton.** The Rust type-checks and lints for `aarch64-unknown-linux-ohos`. Nothing has
been linked, packaged or run yet, and the ArkTS has not been compiled: that needs DevEco Studio.

## What is here

| Path | What it is |
|---|---|
| `bridge/` | `procraft-harmony`, the native library the app imports (`libprocraft_harmony.so`): the core, plus thin Node-API wrappers of the calls the interface makes (napi-ohos). On any other system the crate is empty except for its pen maths and tests. |
| `app/` | The DevEco Studio project (Stage model, one `entry` module for tablets and 2-in-1s): a full-screen canvas component (`XComponent`, type SURFACE) whose surface the core draws into. Every touch and pen sample is passed to the core. |
| `build-rust.sh` | Builds the library into `app/entry/libs/<abi>/` and copies in the app icons. |
| `../crates/core/src/ohos_window.rs` | The core's side: makes the native window from the canvas component's surface id (`acquire_surface`). The render thread draws into it, as it does into Android's and the iPad's windows. |

## Building

1. Install DevEco Studio (it brings the HarmonyOS SDK), and add the Rust target:
   `rustup target add aarch64-unknown-linux-ohos`. Add `x86_64-unknown-linux-ohos` too for the
   emulator on an Intel or AMD PC. Optionally run `cargo install ohrs`: then the build also writes
   the library's ArkTS declarations from the Rust.
2. Point `OHOS_NDK_HOME` at the SDK folder that holds `native/` (in DevEco Studio's SDK,
   `default/openharmony`), then run `harmony/build-rust.sh`, or `harmony/build-rust.sh arm64 x86_64`
   for the emulator too.
3. Open `harmony/app` in DevEco Studio and let it sync.
4. Set up signing: File > Project Structure > Signing Configs > Automatically generate signature,
   signed in with a Huawei ID. Then run the app on a device or the emulator.

DevEco Studio writes the signing keys' paths and passwords into `app/build-profile.json5`. Don't
commit them.

To check the Rust without the SDK, point `OHOS_NDK_HOME` at any empty folder (the bridge's build
script only adds link paths) and run:

```
rustup target add aarch64-unknown-linux-ohos
OHOS_NDK_HOME=/some/empty/folder cargo clippy --target aarch64-unknown-linux-ohos -p procraft-core -p procraft-harmony --all-targets -- -D warnings
```

## Not done yet

- **GPU.** The canvas draws on OpenGL ES. wgpu 30 can't make a Vulkan surface on HarmonyOS.
  Upstream wgpu has `VK_OHOS_surface` for its next major release; Vulkan goes first once that is
  out (`Gpu::backends`).
- **Pen.** Pressure and tilt are read per event. The native event API (`OH_ArkUI_PointerEvent_GetHistoryTiltX`
  and the rest) has them for every kept point, and so does the hover state. Which way each tilt
  axis leans still has to be checked with a real pen (`bridge/src/pen.rs`).
- **Timed gestures.** The core's `touch_deadline` / `touch_tick` (holds) aren't wired up yet.
- **Logging.** The core's log doesn't reach hilog yet.
- **Everything else in the interface**: gallery, panels, files, sharing. The bridge only wraps
  the calls in `bridge/src/canvas.rs`. The plan is to generate the wrappers from the core's UniFFI
  interface, as the Kotlin and Swift bindings are, rather than write hundreds by hand.
