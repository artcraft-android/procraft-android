# Procreate parity: what is missing, and what we can do about it

ProCraft is a clean-room painting app for Android tablets, Windows and Linux. Procreate is the
reference for behaviour, never for code, assets or measurements taken from its binary: everything
here comes from Procreate's public Help Center, its product page, and our own issues.

This note is a working checklist, not a promise. It groups the gaps that matter most, says where
each one stands, and proposes how to land it as a separate pull request.

## Where the reference and the app differ most

| Area | Procreate | ProCraft today | Next step |
|---|---|---|---|
| Reference Companion, FacePaint | The **Face** mode projects the artwork onto the user's face in real time, with photo and video capture (`help.procreate.com/articles/ZWopfZ-reference`) | The Reference window has Canvas, Image and Face tabs. **PR `feat/reference-camera`** adds a Camera button to Image; **PR `feat/face-camera-preview`** gives Face a live, mirrored front-camera preview with its permission flow, released with the window | The camera half is in PR. Projecting the artwork onto the face still needs a face mesh and a texture pass; it is not pretended here |
| Phone interface | Procreate Pocket is a separate, phone-first app (`procreate.com/pocket`) | The tablet app is laid out for tablets; a phone build is planned in the README. PR #82 (`feat/android-pocket-phone-ui`) is open | Review and land #82 in pieces (bars, panels, gallery header) instead of one branch |
| Layer groups | A group shows a thumbnail of what it holds, and its row opens its blend mode and opacity | A group row showed a rule where its thumbnail would be (`LayersPanel.kt`, `GroupRow`; `desktop/src/layers.rs`, `group_row`), and had no blend letter or opacity | **PRs `feat/layer-group-thumbnails`** (composite and version the group, Android + desktop, with a core test) and **`feat/group-opacity-blend`** (the blend letter, the blend list and opacity, Android) |
| Brush Studio on a short screen | The studio stays usable when the window is short | In portrait the pad was a fixed 510 dp, so on a short screen the settings had no height left; sliders appeared only after rotating (#72) | **PR `fix/brush-studio-short-screens`**: the pad takes at most 45% of the height, the settings the rest and scroll |
| Brush kit parity | The stock kit's sets and best known brushes behave as their names say | 21 sets and 204 brushes, all 11 Procreate sets, no two brushes with the same preview. Measured on the headless GPU and with an injected pressure pen in the app | **PR `test/brush-parity`**: contracts for the sets, key brushes, pressure, grain and wet mix; no brush setting needed changing for them to pass |
| Imported brushes | A file from Photoshop, GIMP, Krita or Clip Studio keeps its shape, spacing and dynamics, and previews what it paints | Imported 10 real ABR sets (~600 brushes): every brush draws except one texture brush whose own 100% texture leaves almost no paint; one set's blank file preview showed nothing. `.brush`/`.brushset` round trips keep the settings and the set's order. A Studio change reaches both preview and canvas | **PR `fix/imported-brush-preview`**: a blank file preview is ignored and drawn instead. **PR `test/brush-parity`** now includes the round-trip, studio-to-canvas and external-file checks |
| Custom shortcuts | Procreate's gesture/keyboard settings are user-remappable | Desktop list and shortcuts exist; the remapping list is PR #98. Android external-keyboard shortcuts are a fixed list | Land #98 first, then expose the same `keys.rs` table in the Android settings page |
| Third-party stylus buttons | Procreate covers Apple Pencil's squeeze and hover; third-party Android pens differ by vendor | The first pen side button becomes Modify (eyedropper); there is no mapping page and no second-button action (#84) | Add a small "Pen buttons" page in Prefs and map the `keyCode`/button events the app already receives to existing actions. Needs a device to verify |
| Saving and files | Artworks live in the app's library; exports go through the system share sheet/Files | The same split, but Android artwork folders are app-private; issue #7 asks for a user-visible folder | Export to the MediaStore `Documents` collection as an option, leaving the working copy private. Android-only PR |
| Interface blur | Frosted panels over the canvas | Implemented on Android; some devices report no blur (#61) and the fallback path is not obvious | Check which devices lack the backdrop path and make the fallback deliberate, not a black/transparent panel |

## Already fixed on `main` (close or verify the issue, do not re-fix)

| Issue | Evidence |
|---|---|
| #5 three-finger swipe clears, swipe down does Copy & Paste | `crates/core/src/touch.rs` tests: `three_fingers_scrubbed_clear_and_swiped_down_open_copy_and_paste`, `a_hand_s_swipe_down_never_clears_and_its_scrub_does` |
| #10 Reference window zoom and colour pick | `crates/core/src/reference_view.rs` (pinch, pan, pixel under a finger) and `ReferenceUi.kt` gestures |
| #16 Android 10 crash from `windowLayoutInDisplayCutoutMode` | `android/app/src/main/res/values/themes.xml` uses `shortEdges`; `values-v30` uses `always` |
| #54 reordering brush sets crashed | `1f65de7` "library writes don't queue up (#54)" |
| #56 brush rename | `BrushNameHeading` in `BrushStudio.kt` renames in place on the About page |
| #64 pressure bleed takes paint from taps | Open PR #95 (`fix/bleed-keeps-whole`) |
| #19/#65 QuickShape's fit | Open PR #65 (`fix/quickshape-smoothing-path`) |
| #80 split-screen bars and menus | the phone/window-inset work (#68 and the recent commits), plus the Reference window PRs |
| #27 Reference window layering | PR #86 (issue closed) |
| #71 dual brushes | The engine and Brush Studio already carry a second brush (`Brush::secondary`, `studioDual`); the issue looks stale, but it needs a device check |

## In open pull requests (do not duplicate)

- #95 pressure bleed keeps strong parts whole.
- #96 smudge keeps its grain.
- #94 desktop prefers an 8-bit surface.
- #97 desktop background row opens the colour panel.
- #98 desktop's shortcut list binds its keys again.
- #88 brush previews match Procreate's azimuth, wet mix and texturized grain.
- #86 Reference window layering.
- #85 absolute barrel rotation.
- #82 ProCraft Pocket phone interface.
- #76 Reference window bars keep their place under their own touches.
- #74 desktop drag a layer with the mouse.
- #70 adjustments on several picked layers.
- #68 phone bars, panels and gallery header fit beside the camera.
- #65/#62 QuickShape fitting and curves.
- #63 wet brushes mix.
- #55 stabilisation spans a length of stroke.
- #23 Greater blend mode.
- #22 desktop pen on Wayland, iPad transfers, colour models.

## What to fix first, in order

1. **Land the small parity PRs** (`feat/layer-group-thumbnails`,
   `feat/group-opacity-blend`, `feat/reference-camera`, `feat/face-camera-preview`,
   `fix/brush-studio-short-screens`, `fix/imported-brush-preview`, `test/brush-parity`): all are
   visible, self-contained and tested, built as an APK, or both.
2. **Clear the open-PR backlog that touches the same files** (#95 bleeds, #96 smudge, #88 brush
   previews) so brush work does not conflict.
3. **Phone interface in slices** (#82 is large): start with the gallery header and the bars, then
   the panels, then the canvas chrome.
4. **FacePaint as its own project**: camera permission, CameraX preview, a face mesh, and a
   projection pass in the reference window. Do not pretend a plain camera preview is parity.
5. **Input parity**: pen-button mapping (#84), then the Android shortcut list, then the remaining
   gesture requests (#51).
6. **Files and sharing** (#7): export to `Documents` through MediaStore, with the same names and
   formats the share sheet already uses.

## How to keep the PRs clean

- One branch per row of the first table, named `feat/…`, `fix/…` or `docs/…`.
- Conventional commits; the body says what was measured or tested, as the existing history does.
- Core/renderer changes come with a test under `crates/*/tests` or `#[cfg(test)]`; Android-only
  layout changes are checked with `./gradlew :app:lintRelease` and a screenshot on the emulator.
- Do not mix a fix and a feature in one branch: the group-thumbnail work touches the core, the GPU,
  Android and the desktop; the studio layout touches one file.

## Verification notes

- The stroke benchmark on the host GPU (2048 square canvas, 1000 events, two per frame) costs
  0.12 ms an event for the Technical Pencil and 0.13 ms a frame; the other sampled brushes are
  0.007–0.064 ms an event. That is the engine side of fluidity, not a tablet's display.
- External brush imports were checked against the ABR files in the user's download folder (10
  sets, about 600 brushes), not committed.
- `cargo test --workspace` on the development machine (AMD RX 7600, RADV) found two failures in
  `crates/core/src/plugin_filter.rs` (`a_filter_opens_redraws_as_it_slides_and_cancel_puts_the_layer_back`,
  `reset_preview_and_undo_keep_it_open_at_its_start`) at the `cut = 0` assertion. They reproduce
  outside the core, with a bare `Canvas::set_plugin_filter` and a white layer, and look like a
  driver/shader edge rather than this work. They are left as a separate investigation.
- No physical Android device was attached (`adb devices` empty); the `Small_Tablet` AVD is
  available for layout verification.
