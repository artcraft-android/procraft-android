<p align="center">
  <img src="assets/app-icon/procraft-1024.png" alt="ProCraft's app icon: a paintbrush in a warm-to-cool blend on a black tile" width="128">
</p>

<h1 align="center">ProCraft for Android</h1>

<p align="center">
  <b>An Android native open-source painting app for tablets, with its own Rust engine and GPU renderer under a Kotlin/Jetpack Compose UI.</b><br>
  Pressure-sensitive brushes, layers with blend modes, adjustments, selection and transform, animation and 3D painting, written from scratch.
</p>

<p align="center">
  <a href="https://github.com/artcraft-android/procraft-android/releases"><img src="https://raw.githubusercontent.com/Kunzisoft/Github-badge/main/get-it-on-github.png" alt="Get it on GitHub" height="60"></a>
  <a href="https://apps.obtainium.imranr.dev/redirect?r=obtainium://add/https://github.com/artcraft-android/procraft-android"><img src="https://raw.githubusercontent.com/ImranR98/Obtainium/main/assets/graphics/badge_obtainium.png" alt="Get it on Obtainium" height="60"></a>
</p>

<p align="center">
  <img alt="Rust engine" src="https://img.shields.io/badge/engine-Rust-lightblue?style=flat&logo=rust&logoColor=white">
  <img alt="GPU renderer: wgpu" src="https://img.shields.io/badge/renderer-wgpu-lightblue?style=flat">
  <img alt="Kotlin and Jetpack Compose" src="https://img.shields.io/badge/UI-Kotlin%20%C2%B7%20Jetpack%20Compose-blue?style=flat">
  <img alt="Minimum: Android 10 (API 29)" src="https://img.shields.io/badge/min-Android%2010%20%28API%2029%29-blue?style=flat">
  <img alt="Target: Android 16 (API 36)" src="https://img.shields.io/badge/target-Android%2016%20%28API%2036%29-blue?style=flat">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=flat">
  <img alt="Status: beta" src="https://img.shields.io/badge/status-beta-lightblue?style=flat">
</p>

<p align="center">
  <a href="#painting">Painting</a> ·
  <a href="#layers-and-colour">Layers &amp; colour</a> ·
  <a href="#adjustments-and-transform">Adjustments &amp; transform</a> ·
  <a href="#actions">Actions</a> ·
  <a href="#planned-features">Planned features</a> ·
  <a href="#building-instructions">Building</a>
</p>

<br>

<p align="center">
  <img src="docs/images/demo.gif" alt="Dusk painted in ProCraft from a blank canvas: the sky filled indigo and airbrushed in bands of purple, magenta, orange and gold; a glow and a sun drawn as a circle that snaps round and is filled by dragging the colour in; two ranges of hills outlined and filled the same way; pines and birds inked; stars airbrushed and made to glow with Bloom; the Layers panel opened on its seven layers" width="100%">
  <br>
  <sub><b>Dusk</b>, painted from a blank canvas on a Lenovo Legion Tab: the sky filled and airbrushed, a sun snapped round and filled by dragging the colour in, hills filled the same way, pines inked, stars with Bloom. 4× playback with the idle pauses cut; <a href="docs/images/demo.mp4">full video</a> (2×). Every stroke is a pen stroke with pressure, played by the <a href="tools/showcase">demo script</a>.</sub>
</p>

<br>

<p align="center">
  <img src="docs/images/canvas.jpg" alt="The finished Dusk on the canvas: a sky from deep indigo through purple, magenta and orange to gold, a pale sun with a soft glow, birds, two ranges of purple hills with dark pines, and stars; the top bar with Gallery and the tools, and the sidebar with the size and opacity sliders, undo and redo" width="100%">
  <br>
  <sub>The canvas: the tools along the top, size and opacity in the sidebar, the whole screen for the painting.</sub>
</p>

<br>

## Painting

<table>
<tr>
<td width="50%"><img src="docs/images/brushes.jpg" alt="The Brush library over Dusk: the sets Sketching, Inking, Drawing, Painting (chosen), Watercolour, Calligraphy, Airbrushing, Spray Paint, Charcoals, Textures and Abstract, each with its icon, and the Painting set's brushes with a preview stroke each"><br><sub>The brush library: eleven sets of our own brushes, each previewed with a stroke.</sub></td>
<td width="50%"><img src="docs/images/studio.jpg" alt="The Brush Studio on Round Brush: the tabs from Stroke path to About this brush, the Shape tab with its shape source and input style, and the Drawing Pad with a pressure scribble"><br><sub>The Brush Studio: stroke path, stabilization, taper, shape, grain, rendering, wet mix, colour dynamics and more, tried out on the drawing pad.</sub></td>
</tr>
</table>

<br>

## Layers and colour

<table>
<tr>
<td width="50%"><img src="docs/images/layers.jpg" alt="The Layers panel over Dusk: seven layers, each with its thumbnail (stars, pines, the near and far hills, the sun, its glow and the sky), their blend mode letters and visibility boxes, and the background colour"><br><sub>Layers with blend modes, opacity, alpha lock, clipping masks, masks and groups. Layers past what the GPU holds are kept compressed, so artworks with hundreds of layers open and paint smoothly.</sub></td>
<td width="50%"><img src="docs/images/colour.jpg" alt="The Colors panel on its Disc tab: a hue ring around a saturation and brightness disc, the current and previous colours, and the Classic, Harmony, Value and Palettes tabs"><br><sub>Colour: disc, classic, harmony, value (with hex) and palettes.</sub></td>
</tr>
</table>

<br>

## Adjustments and transform

<table>
<tr>
<td width="50%"><img src="docs/images/adjust.jpg" alt="Hue, Saturation, Brightness on Dusk's sky: the hue moved to 40 percent turns the sky blue at the top and pink at the horizon, with the Hue, Saturation and Brightness sliders below"><br><sub>Hue, saturation, brightness on the sky layer, live. Also colour balance, curves, gradient map, blurs, noise, sharpen, bloom, glitch, halftone, chromatic aberration, liquify and clone, on the layer or painted on.</sub></td>
<td width="50%"><img src="docs/images/transform.jpg" alt="Transform in Warp on the sun layer: a mesh with corner handles over the sun, and the toolbar with Freeform, Uniform, Distort and Warp, then Advanced Mesh, Flip Horizontal, Flip Vertical, Rotate 45, Fit to Canvas, Bilinear and Reset"><br><sub>Transform: freeform, uniform, distort and warp, with snapping. Selection: automatic, freehand, rectangle and ellipse, with feather.</sub></td>
</tr>
</table>

<br>

## Actions

<p align="center">
  <img src="docs/images/actions.jpg" alt="The Actions panel on its Canvas tab over Dusk: Crop and Resize, the Animation Assist, Page Assist, Drawing Guide and Reference switches, Edit Drawing Guide, Flip horizontal, Flip vertical and Canvas information, under the Add, Canvas, Share, Video, Prefs and Help tabs" width="100%">
  <br>
  <sub>Actions: add files, photos and text; the canvas, its guides and assists; share as PNG, JPEG, PSD, PDF, TIFF, GIF or MP4; a time-lapse of every artwork; preferences and gesture controls.</sub>
</p>

<br>

## Planned features

ProCraft for Android is in beta.

| Area | Status |
|---|---|
| Canvas: the top bar, the sidebar (size, opacity, modify, undo, redo), full screen, gestures (pinch, rotate, quick fit, two- and three-finger taps, scrub to clear) | ✅ |
| Brush engine: shape and grain sources, stroke path, stabilization, taper, rendering and glazes, wet mix, colour dynamics, pen pressure and tilt; the library and the Brush Studio | ✅ |
| Layers: blend modes, opacity, alpha lock, clipping masks, masks, groups, merge, the reference layer; large artworks kept compressed off the GPU | ✅ |
| Colour: disc, classic, harmony, value and palettes; history; eyedropper; filling by dragging the colour onto the canvas | ✅ |
| Adjustments, each with its toolbar, on the layer or painted on | ✅ |
| Selection (automatic, freehand, shapes, feather) and transform (freeform, uniform, distort, warp, snapping) | ✅ |
| Shapes that snap clean, drawing guides and drawing assist (2D, isometric, perspective, symmetry), text | ✅ |
| Animation and page assists, the reference window | ✅ |
| Gallery: stacks, new-canvas presets, import and share (PNG, JPEG, PSD, PDF, TIFF, GIF, MP4), time-lapse | 🚧 nearly done |
| 3D painting: OBJ and USDZ models, texture sets, painting on the mesh, lights and environments | 🚧 |
| Phone layout | ⬜ |

<sub>✅ works today · 🚧 in progress · ⬜ not started · 🚫 not planned</sub>

<br>

## Building Instructions
*One Gradle command builds the whole app: it compiles the Rust engine with [cargo-ndk](https://github.com/bbqsrc/cargo-ndk) and generates the Kotlin bindings.*

* Clone this repository
* Make sure you have at least 6 GB of free disk space
* enter `apps/procraft-android/android` and run `./gradlew assembleRelease -Pprocraft.abis=arm64-v8a`, or open that folder in Android Studio and press Run.

Build time dependencies:

<details>
  <summary>required tools</summary>

  ```
      rustup, with the aarch64-linux-android and x86_64-linux-android targets
      cargo install cargo-ndk --version "^4" --locked
      Android SDK (API 37) and NDK 29.0.14206865
      JDK 17 or newer
  ```
</details>

`cargo test --workspace` runs the engine's and the renderer's tests on the desktop.

| Path | What |
|---|---|
| `crates/engine` | document, brushes, stroke geometry, canvas view (no GPU) |
| `crates/gpu` | wgpu renderer: layers, strokes, compositing, display |
| `apps/procraft-android` | the native library for Android (UniFFI bindings) |
| `apps/procraft-android/android` | the Android app (Gradle) |

*Builds are only tested on Windows 11.*

<br>

## License

[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option. The third-party material the app
bundles (the Inter font, Lucide icons, light panoramas) and the libraries it is built with are listed
in [NOTICE](NOTICE); every asset, with its author, source and licence, in
[ATTRIBUTION.md](ATTRIBUTION.md).

Icons drawn using vector outlines ([`tools/icons/draw_icons.py`](tools/icons/draw_icons.py), to the
[style guide](docs/icons/STYLE_GUIDE.md)). The demo painting, *Dusk*, is our own, made in the app.

<br>

## Misc info for myself

Try to use [semantic commit](https://kapeli.com/cheat_sheets/Semantic_Commits.docset/Contents/Resources/Documents/index)
messages. Use `feat:` / `fix:` / `chore:` / `docs:` / etc.
