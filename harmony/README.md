# ProCraft for HarmonyOS (starter)

The beginnings of ProCraft on HarmonyOS 5 and later (HarmonyOS NEXT), which runs no Android
apps. Huawei tablets on HarmonyOS 2 to 4 already run the Android app.

**For now the HarmonyOS app is the desktop app.** The desktop app's interface (egui; the same
interface as the tablets) runs in a full-screen canvas component, an ArkUI `XComponent`. Its
native side drives the app the way winit does on the desktop. An ArkUI interface of its own, over
the core's `CanvasSession`, comes later; `bridge/src/canvas.rs` and `app/.../canvas/CanvasView.ets`
are its start.

**Status:** the Rust type-checks and passes clippy for `aarch64-unknown-linux-ohos`. The app is
made on the `harmony` branch: each push there checks it again and, once Huawei's tools are set up
for CI (SIGNING.md), builds the unsigned `.hap` as the run's artifact
(`.github/workflows/harmony.yml`). Nothing has been linked, packaged or run yet, and the ArkTS has
not been compiled: that needs DevEco Studio.

## What is here

| Path | What it is |
|---|---|
| `bridge/` | `procraft-harmony`, the native library (`libprocraft_harmony.so`). `egui_host.rs`: when ArkUI loads it as the canvas component's native side, it hooks the component's surface, touch (with the samples kept between frames), mouse and key callbacks up to the desktop app (`../desktop/src/ohos.rs`). It also takes ArkTS's side of the system (`registerHost`, `answer`, the soft keyboard's text, the pen's hover). `canvas.rs`: Node-API wrappers of a few core calls, for the later ArkUI interface. On any other system the crate is empty except for its pen maths. |
| `app/` | The DevEco Studio project (Stage model, one `entry` module for tablets and 2-in-1s, HarmonyOS 5.0.3 / API 15 and later). `pages/Index.ets` registers `host/Host.ets`, calls `startApp`, and shows the full-screen `XComponent` with `libraryname: 'procraft_harmony'`. `host/Host.ets` does what the app asks of the system: the document picker, the picker's Save, the share sheet, the pasteboard and the soft keyboard. |
| `build-rust.sh` | Builds the library into `app/entry/libs/<abi>/` and copies in the app icons. |
| `../desktop/src/ohos.rs` | The desktop app's HarmonyOS host. The app runs on a thread of its own, taking the component's callbacks as events. It can wait for the system's pickers the way it waits for rfd's dialogs on the desktop. The window is the component's native window, and wgpu draws on OpenGL ES. The GPU device, the canvas and the interface outlive the surface, so going to the background loses nothing (the artwork is saved as it goes). |
| `../desktop/src/ohos_platform.rs` | The requests to `Host.ets` (JSON) and their answers. Files picked are copied into the app's cache. Files saved are written into the cache, then copied where the user picked, or shared from the share sheet for a "Share as …". |
| `../desktop/src/native_input.rs`, `egui_events.rs` | Touches with the pen's tool, pressure and tilt, made into the window events the desktop takes. egui's input is made from those as egui-winit makes it. |
| `../crates/core/src/ohos_window.rs` | The core's side of the later ArkUI interface: the native window from a surface id. |

## Build it yourself

This needs nothing from the maintainer: a free Huawei ID, DevEco Studio and a HarmonyOS 5.0.3+
tablet (or the emulator).

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
   It makes a debug certificate and profile for your device under your Huawei ID. If it refuses
   the bundle name, change `bundleName` in `app/AppScope/app.json5` to one of your own.
3. Pick the emulator or the tablet and press Run.

The app's log goes to hilog under the tag `ProCraft` (`hdc hilog | grep ProCraft`), and to
`procraft.log` in the app's files folder.

DevEco Studio writes the signing keys' paths and passwords into `app/build-profile.json5`. Don't
commit them.

To check the Rust without the SDK (as CI does), point `OHOS_NDK_HOME` at any empty folder (the
bridge's build script only adds link paths) and run:

```
OHOS_NDK_HOME=/some/empty/folder cargo clippy --target aarch64-unknown-linux-ohos -p procraft-desktop -p procraft-harmony --all-targets -- -D warnings
```

## Install the unsigned build yourself

A release may carry `procraft-<version>-harmonyos-arm64-unsigned.hap`. **It won't install until
you sign it with your own Huawei account.** HarmonyOS installs only packages signed with a
certificate and profile that Huawei issued. Building it yourself (above) is the easier way; to
sign the download instead:

1. **Developer mode:**
   - Make a Huawei ID and install DevEco Studio.
   - On the tablet, turn on developer mode (tap the software version in Settings > About several
     times), then USB debugging in Settings > System > Developer options. Connect the tablet.
2. **Your own debug certificate and profile:**
   - In DevEco Studio, Build > Generate Key and CSR makes a `.p12` and a `.csr`.
   - In AppGallery Connect (Certificates, APP IDs and Profiles):
     - a debug certificate from the `.csr` (a `.cer`);
     - your tablet's UDID (`hdc shell bm get --udid`);
     - an APP ID for `io.github.artcraftandroid.procraft`;
     - a debug profile with that certificate and your tablet (a `.p7b`).
   - If AppGallery Connect won't take that bundle name for your account, build it yourself
     instead, with a bundle name of your own.
3. **Sign** with the SDK's tool (in DevEco Studio's `sdk/default/openharmony/toolchains/lib`):
   ```
   java -jar hap-sign-tool.jar sign-app -mode localSign -signAlg SHA256withECDSA \
     -keyAlias <alias> -keyPwd <key password> -keystoreFile my.p12 -keystorePwd <keystore password> \
     -appCertFile my.cer -profileFile my.p7b \
     -inFile procraft-<version>-harmonyos-arm64-unsigned.hap -outFile procraft.hap
   ```
4. **Install:** `hdc install procraft.hap` (hdc is in the SDK's `toolchains` folder).

Without identity verification, a debug certificate lasts 14 days; after that, sign and install
again. Verified, it lasts a year.

## Unverified (written from the API docs; to check on the emulator or a device)

- **The ArkTS** (`Host.ets`, `Index.ets`): the picker, Share Kit, pasteboard and input method
  calls follow the API references but haven't been compiled.
- **The pasteboard:** reading it takes `ohos.permission.READ_PASTEBOARD` (a restricted
  permission, not declared) or the system's paste button. Until then, a paste falls back to the
  app's own clipboard. Copying out needs nothing.
- **Pen and touch:**
  - whether touch positions are in pixels (assumed);
  - which way each tilt axis leans;
  - whether the kept samples' array is the system's to free.
- **Background:** whether a surface the app is slow to let go of (while a picker is open) is safe;
  the app holds its own reference to the native window.
- **Saving again:** a second Save of the same artwork writes into the cache without copying out
  again.

## Not done yet

- **Clipboard pictures and files from other apps:** these need the paste button or the
  permission.
- **Open with:** files opened with the app from other apps (skills and `want.uri` in
  `module.json5`).
- **GPU:** Vulkan waits for wgpu's next major release (OpenGL ES until then).
- **Smaller pieces:** Project canvas, the M-Pencil's double-tap and squeeze (Pen Kit, mainland
  China only), time-lapse video through the system's encoder.

## Packaging and releases

The release workflow builds the HarmonyOS app (job `harmony` in `.github/workflows/release.yml`)
once the repository variable `RELEASE_HARMONY` is true and Huawei's command-line tools are
reachable (`HARMONY_TOOLS_URL`). Like the Mac job, it is left out of a release when it fails.
- **Without signing secrets:** it releases the unsigned `.hap` (see "Install the unsigned build
  yourself").
- **With them:** it releases a signed one.

`SIGNING.md` has the maintainer's steps and what a signed `.hap` can and can't do. In short:
- **Builds:** `hvigorw assembleHap` builds a `.hap` per module (installed with `hdc install`),
  and `hvigorw assembleApp` builds the `.app` that AppGallery Connect takes.
- **Signing:** a keystore (`.p12`), plus a certificate (`.cer`) and profile (`.p7b`) that Huawei
  issues.
- **Installing:** a debug-signed `.hap` installs only on the devices registered in its profile; a
  release-signed one only through AppGallery.
