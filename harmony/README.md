# ProCraft for HarmonyOS (starter)

The beginnings of ProCraft on HarmonyOS 5 and later (HarmonyOS NEXT), which runs no Android
apps. Huawei tablets on HarmonyOS 2 to 4 already run the Android app.

**For now the HarmonyOS app is the desktop app.** The desktop app's interface (egui; the same
interface as the tablets) runs in a full-screen canvas component, an ArkUI `XComponent`. Its
native side drives the app the way winit does on the desktop. An ArkUI interface of its own, over
the core's `CanvasSession`, comes later; `bridge/src/canvas.rs` and `app/.../canvas/CanvasView.ets`
are its start.

**Status:** the Rust type-checks and passes clippy for `aarch64-unknown-linux-ohos`. Nothing has
been linked, packaged or run yet, and the ArkTS has not been compiled: that needs DevEco Studio.

## What is here

| Path | What it is |
|---|---|
| `bridge/` | `procraft-harmony`, the native library (`libprocraft_harmony.so`). `egui_host.rs`: when ArkUI loads it as the canvas component's native side, it hooks the component's surface, touch, mouse, key and frame callbacks up to the desktop app (`../desktop/src/ohos.rs`). `canvas.rs`: Node-API wrappers of a few core calls, for the later ArkUI interface. On any other system the crate is empty except for its pen maths. |
| `app/` | The DevEco Studio project (Stage model, one `entry` module for tablets and 2-in-1s). `pages/Index.ets` calls `startApp(filesDir, density)` and shows the full-screen `XComponent` with `libraryname: 'procraft_harmony'`. |
| `build-rust.sh` | Builds the library into `app/entry/libs/<abi>/` and copies in the app icons. |
| `../desktop/src/ohos.rs` | The desktop app's HarmonyOS host. The window is the component's native window; wgpu draws on OpenGL ES. Frames come at the display's vsync. Touches carry the pen's tool, pressure and tilt (`native_input.rs`); egui's input is made from them as egui-winit makes it (`egui_events.rs`). |
| `../crates/core/src/ohos_window.rs` | The core's side of the later ArkUI interface: the native window from a surface id. |

## Running it

What you need (the downloads need a Huawei ID):

1. **DevEco Studio** for Windows or macOS, from developer.huawei.com (about 3 GB). It includes the
   HarmonyOS SDK. Sign in with your Huawei ID inside it.
2. **The emulator image:** Device Manager > New Emulator > a tablet image (x86_64 on an Intel or AMD
   PC, arm64 on an Apple-silicon Mac), downloaded through DevEco Studio. Or use a HarmonyOS 5/6
   tablet with developer mode on, connected over USB.
3. **Rust targets:** `rustup target add aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos`.
   Optionally run `cargo install ohrs`: it writes the library's ArkTS declarations from the Rust.

Then:

```
# the SDK's native part: <DevEco Studio>/sdk/default/openharmony (it holds native/llvm and native/sysroot)
export OHOS_NDK_HOME="/c/Program Files/Huawei/DevEco Studio/sdk/default/openharmony"
harmony/build-rust.sh x86_64          # the emulator on a PC; arm64 for a tablet or a Mac's emulator
```

Then, in DevEco Studio:
1. Open `harmony/app`.
2. Under File > Project Structure > Signing Configs, tick *Automatically generate signature*.
3. Pick the emulator or the tablet and press Run.

The app's log goes to hilog under the tag `ProCraft` (`hdc hilog | grep ProCraft`), and to
`procraft.log` in the app's files folder.

DevEco Studio writes the signing keys' paths and passwords into `app/build-profile.json5`. Don't
commit them.

To check the Rust without the SDK, point `OHOS_NDK_HOME` at any empty folder (the bridge's build
script only adds link paths) and run:

```
OHOS_NDK_HOME=/some/empty/folder cargo clippy --target aarch64-unknown-linux-ohos -p procraft-desktop -p procraft-harmony --all-targets -- -D warnings
```

## Stand-ins and what is still to do

- **Files:** opening, saving, importing and sharing say "isn't available on HarmonyOS yet" (`desktop/src/host.rs`).
  The fix is the document picker and Share Kit through ArkTS.
- **Clipboard:** text copied in the app pastes within the app, but nothing is shared with the
  system's pasteboard. The fix is the pasteboard through ArkTS.
- **Text:** fields take a hardware keyboard only, with a US layout (`egui_events.rs`). The fix is
  the soft keyboard through the input method framework.
- **Leaving the app:** when it goes to the background, the artwork open is kept and closed, and the
  app comes back on the gallery. Later, keep the GPU device and the canvas across surfaces, as the
  Android app does.
- **Pen:** each touch carries its own tool, pressure and tilt; historical points (more than one
  sample per frame) and hover aren't read yet.
- **Not checked yet:**
  - whether touch positions are in pixels (assumed) or vp;
  - which way each tilt axis leans.
- **GPU:** OpenGL ES only until wgpu has `VK_OHOS_surface` (its next major release).
- **Not wired:** fonts for other scripts from `/system/fonts`, Project canvas, the M-Pencil's
  double-tap and squeeze (Pen Kit).

## Packaging and releases

How a HarmonyOS release is packaged and signed, and a draft GitHub Actions job, are in
`release-harmony.yml.draft`. That file is not under `.github/`, so it never runs. In short:
- **Builds:** `hvigorw assembleHap` builds a `.hap` per module (installed with `hdc install`), and
  `hvigorw assembleApp` builds the `.app` that AppGallery Connect takes.
- **Signing:** a keystore (`.p12`), a certificate (`.cer`) and a profile (`.p7b`). Debug-signed
  builds install only on devices registered in the profile; release-signed ones go through
  AppGallery.
