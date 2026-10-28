# Attribution

Every non-code asset in ProCraft, with its author, source and licence. ProCraft's own code and
original assets are MIT OR Apache-2.0 (see [`LICENSE-MIT`](LICENSE-MIT),
[`LICENSE-APACHE`](LICENSE-APACHE) and [`NOTICE`](NOTICE)). When you add an asset, add a row here
in the same change; third-party assets must be openly licensed and keep their licence file next to
them.

## Bundled in the app

| Path | Title | Author | Source | Licence |
|---|---|---|---|---|
| `apps/procraft-android/android/app/src/main/assets/fonts/Inter-{Regular,Medium,SemiBold,Bold}.ttf` | Inter 4.1 (interface font) | The Inter Project Authors | <https://github.com/rsms/inter/releases/tag/v4.1> (`Inter-4.1.zip`, `extras/ttf/`; `SOURCE.md` beside them) | SIL OFL 1.1, `Inter-OFL.txt` |
| `apps/procraft-android/android/app/src/main/java/io/github/artcraftandroid/procraft/ui/LucideIcons.kt` (the interface icons as Lucide draws them, kept beside ours) | Lucide icons, lucide-static 1.63.0 | Lucide Icons and Contributors (those derived from Feather: Cole Bemis) | <https://github.com/lucide-icons/lucide>; each icon's paths moved and scaled into the frame it is shown in, at the size shown | ISC (Feather-derived also MIT), [`LICENSE-lucide.txt`](apps/procraft-android/android/app/src/main/assets/icons/LICENSE-lucide.txt) |
| `assets/icons/procraft/*.svg`, and every icon in `apps/procraft-android/android/app/src/main/java/io/github/artcraftandroid/procraft/ui/Icons.kt` (`PcIcons`: the bar, toolbars, panels and menus, the blend mode glyphs and the 48 brush set icons) | ProCraft's own interface icons | ProCraft contributors | Original work, drawn from the idea to [`docs/icons/STYLE_GUIDE.md`](docs/icons/STYLE_GUIDE.md) by `tools/icons/draw_icons.py`; placed by the measured slots in `assets/icons/slots.tsv` | MIT OR Apache-2.0 |
| `apps/procraft-android/android/app/src/main/assets/environments/*.hdr` | 3D view light panoramas (11), halved to 512 x 256 and scaled by `tools/env/make_env.py` | Sergej Majboroda, Greg Zaal, Andreas Mischok, Rico Cilliers (per file in the folder's `README.md`) | <https://polyhaven.com> | CC0 1.0 |
| `assets/app-icon/procraft-{1024,512}.png`, `apps/procraft-android/android/app/src/main/res/drawable-*dpi/ic_launcher_{foreground,monochrome}.png`, `apps/procraft-android/android/app/src/main/res/drawable-nodpi/procraft_mark.png`, `apps/procraft-android/android/app/src/main/res/mipmap-anydpi/ic_launcher.xml` | App icon: a paintbrush in a warm-to-cool blend on black | ProCraft contributors | Original work, drawn in code by `tools/app-icon/make_icon.py`; see [`assets/app-icon/README.md`](assets/app-icon/README.md) | MIT OR Apache-2.0 |
| `apps/procraft-android/android/app/src/main/res/drawable/widget_back.xml`, `apps/procraft-android/android/app/src/main/res/drawable-v31/widget_back.xml` | Recent-artwork widget background | ProCraft contributors | Original work | MIT OR Apache-2.0 |
| `docs/images/{canvas,layers,brushes,studio,colour,actions,adjust,transform}.jpg`, `docs/images/demo.gif`, `docs/images/demo.mp4` | The README's screenshots and demo: ProCraft on a tablet, painting *Dusk* | ProCraft contributors | Original work: the painting made in the app by `tools/showcase/paint_dusk.py`, captured from the screen | MIT OR Apache-2.0 |
| Brushes, brush shapes and grains | Made in code at run time (`crates/engine`) | ProCraft contributors | Original work | MIT OR Apache-2.0 |

### Lucide icons in `LucideIcons.kt`

The same icons, each with the Lucide icon it is:

- `FullScreen`: `maximize-2`
- `Actions`: `wrench`
- `Adjustments`: `wand-sparkles`
- `Selection`: `lasso`
- `Transform`: `navigation-2`
- `Brush`: `brush`
- `Smudge`: `pointer`
- `Erase`: `eraser`
- `Layers`: `layers-2`
- `Undo`: `undo-2`
- `Redo`: `redo-2`
- `TransformReset`: `rotate-ccw`
- `SelectCopyPaste`: `copy`
- `StudioShape`: `shapes`
- `GestureCopyPaste`: `clipboard-copy`
- `MenuNewBrush`: `brush`
- `MenuNewSet`: `folder-plus`
- `MenuShare`: `share`
- `MenuChevron`: `chevron-right`
- `PieUndo`: `undo-2`
- `PieReset`: `rotate-ccw`
- `ShapeEquilateral`: `triangle`
- `ShapeSquare`: `square`
- `CopyCut`: `scissors`
- `CopyDuplicate`: `copy-plus`
- `DoneTick`: `check`
- `Add`: `plus`
- `InfoAbout`: `info`
- `InfoDimensions`: `ruler-dimension-line`
- `InfoLayers`: `layers-2`
- `InfoProfile`: `palette`
- `InfoVideo`: `video`
- `InfoStatistics`: `chart-column`
- `Keyboard`: `keyboard`
- `TextCut`: `scissors`
- `TextCopy`: `copy`
- `TextPaste`: `clipboard-paste`
- `AlignLeft`: `text-align-start`
- `AlignCenter`: `text-align-center`
- `AlignRight`: `text-align-end`
- `AlignJustify`: `text-align-justify`
- `TextUnderline`: `underline`
- `TextOutline`: `type-outline`
- `TextVertical`: `type` and `arrow-down`, side by side
- `CopyOne`: `copy`
- `CopyAll`: `copy-check`
- `CutPaste`: `clipboard-x`
- `PasteDown`: `clipboard-paste`
- `GestureAssisted`: `ruler`
- `GestureEyedropper`: `pipette`
- `GestureQuickShape`: `shapes`
- `GestureQuickMenu`: `chart-pie`
- `GestureFullScreen`: `maximize-2`
- `GestureClearLayer`: `eraser`
- `GestureLayerSelect`: `layers-2`
- `GestureHover`: `pen-line`
- `GestureGeneral`: `settings`
- `ModeDisc`: `disc`
- `ModeClassic`: `square-dot`
- `ModeHarmony`: `orbit`
- `ModeValue`: `sliders-horizontal`
- `ModePalettes`: `palette`
- `Sweep`: `brush-cleaning`
- `CardCompact`: `rows-3`
- `CardStack`: `gallery-vertical-end`
- `TabAdd`: `plus`
- `TabCanvas`: `frame`
- `TabShare`: `share`
- `TabVideo`: `video`
- `TabPrefs`: `toggle-right`
- `TabHelp`: `circle-question-mark`
- `FolderFill`: `folder`
- `Freeform`: `move-diagonal-2`
- `Uniform`: `scaling`
- `Distort`: `vector-square`
- `Warp`: `spline-pointer`
- `Snapping`: `magnet`
- `FlipHorizontal`: `triangles-centerline-dashed-vertical`
- `FlipVertical`: `triangles-centerline-dashed-horizontal`
- `Rotate45`: `rotate-cw`
- `FitToCanvas`: `fullscreen`
- `Interpolation`: `proportions`
- `AdjustHsb`: `sun-medium`
- `AdjustBlur`: `droplet`
- `AdjustBalance`: `scale`
- `AdjustCurves`: `spline`
- `AdjustGradient`: `blend`
- `AdjustMotion`: `wind`
- `AdjustPerspective`: `box`
- `AdjustNoise`: `sparkles`
- `StudioGrain`: `grip`
- `AdjustSharpen`: `diamond`
- `AdjustBloom`: `sun`
- `AdjustGlitch`: `zap`
- `AdjustHalftone`: `grid-3x3`
- `AdjustChromatic`: `aperture`
- `AdjustLiquify`: `droplets`
- `AdjustClone`: `stamp`
- `Disclose`: `chevron-down`
- `NoiseClouds`: `cloud`
- `NoiseBillows`: `cloud-fog`
- `NoiseRidges`: `mountain`
- `HalftoneFull`: `circle-dot`
- `HalftoneScreen`: `grip`
- `HalftoneNews`: `newspaper`
- `ChromaPerspective`: `focus`
- `ChromaDisplace`: `move-horizontal`
- `GuideGrid`: `grid-2x2`
- `GuideIsometric`: `axis-3d`
- `GuideSymmetry`: `square-split-horizontal`
- `ShapeLine`: `slash`
- `ShapePolyline`: `trending-up`
- `ShapeEllipse`: `ellipse`
- `ShapeCircle`: `circle`
- `ShapeTriangle`: `triangle`
- `ShapeQuad`: `diamond`
- `ShapeRect`: `rectangle-horizontal`
- `StudioStroke`: `spline`
- `StudioStabilize`: `activity`
- `StudioTaper`: `triangle-right`
- `StudioRendering`: `paintbrush`
- `StudioWet`: `droplets`
- `StudioColor`: `palette`
- `StudioDynamics`: `gauge`
- `StudioStylus`: `pen-line`
- `StudioProperties`: `settings-2`
- `StudioMaterials`: `box`
- `StudioPreview`: `eye`
- `StudioAbout`: `info`
- `GrainMoving`: `waves-horizontal`
- `GrainTexturized`: `grip-horizontal`
- `GlitchArtifact`: `square-dashed`
- `GlitchWave`: `audio-waveform`
- `GlitchSignal`: `signal`
- `GlitchDiverge`: `split`
- `BrushPaintedOn`: `paint-roller`
- `ModeLayer`: `layers-2`
- `ModePencil`: `pencil`
- `AdjustKnob`: `circle-gauge`
- `Apply`: `check`
- `Preview`: `eye`
- `CancelAdjust`: `ban`
- `Automatic`: `wand-sparkles`
- `Freehand`: `lasso`
- `RectangleShape`: `square-dashed`
- `EllipseShape`: `circle-dashed`
- `SelectAdd`: `plus`
- `SelectRemove`: `minus`
- `Invert`: `contrast`
- `Feather`: `feather`
- `SaveLoad`: `save`
- `ColorFill`: `paint-bucket`
- `ClearSelection`: `brush-cleaning`
- `SwipeShare`: `share`
- `SwipeDuplicate`: `copy-plus`
- `BlendGlyphs["Multiply"]`: `circle-x`
- `BlendGlyphs["Darken"]`: `moon`
- `BlendGlyphs["Shade"]`: `sunset`
- `BlendGlyphs["Color Burn"]`: `flame`
- `BlendGlyphs["Linear Burn"]`: `flame-kindling`
- `BlendGlyphs["Darker Color"]`: `moon-star`
- `BlendGlyphs["Normal"]`: `circle`
- `BlendGlyphs["Lighten"]`: `sun`
- `BlendGlyphs["Screen"]`: `sun-dim`
- `BlendGlyphs["Color Dodge"]`: `sparkle`
- `BlendGlyphs["Add"]`: `circle-plus`
- `BlendGlyphs["Lighter Color"]`: `sun-medium`
- `BlendGlyphs["Overlay"]`: `blend`
- `BlendGlyphs["Soft Light"]`: `lamp`
- `BlendGlyphs["Hard Light"]`: `lightbulb`
- `BlendGlyphs["Vivid Light"]`: `zap`
- `BlendGlyphs["Linear Light"]`: `flashlight`
- `BlendGlyphs["Pin Light"]`: `pin`
- `BlendGlyphs["Hard Mix"]`: `grid-2x2`
- `BlendGlyphs["Difference"]`: `circle-minus`
- `BlendGlyphs["Exclusion"]`: `circle-slash`
- `BlendGlyphs["Subtract"]`: `minus`
- `BlendGlyphs["Divide"]`: `divide`
- `BlendGlyphs["Hue"]`: `palette`
- `BlendGlyphs["Saturation"]`: `droplet`
- `BlendGlyphs["Color"]`: `paintbrush`
- `BlendGlyphs["Luminosity"]`: `sun-moon`
- `BlendGlyphs["Behind"]`: `send-to-back`
- `BlendGlyphs["Height"]`: `mountain`
- `BlendGlyphs["Linear Height"]`: `mountain-snow`
- `SetIcons[0]`: `brush`
- `SetIcons[1]`: `presentation`
- `SetIcons[2]`: `flower`
- `SetIcons[3]`: `bell`
- `SetIcons[4]`: `bookmark`
- `SetIcons[5]`: `paintbrush`
- `SetIcons[6]`: `sprout`
- `SetIcons[7]`: `badge-check`
- `SetIcons[8]`: `cloud`
- `SetIcons[9]`: `gamepad-2`
- `SetIcons[10]`: `target`
- `SetIcons[11]`: `gem`
- `SetIcons[12]`: `dice-5`
- `SetIcons[13]`: `droplet`
- `SetIcons[14]`: `paintbrush-vertical`
- `SetIcons[15]`: `film`
- `SetIcons[16]`: `fish`
- `SetIcons[17]`: `flag`
- `SetIcons[18]`: `flame`
- `SetIcons[19]`: `zap`
- `SetIcons[20]`: `clover`
- `SetIcons[21]`: `hammer`
- `SetIcons[22]`: `headphones`
- `SetIcons[23]`: `heart`
- `SetIcons[24]`: `house`
- `SetIcons[25]`: `moon`
- `SetIcons[26]`: `list-music`
- `SetIcons[27]`: `pen`
- `SetIcons[28]`: `pin`
- `SetIcons[29]`: `plane`
- `SetIcons[30]`: `puzzle`
- `SetIcons[31]`: `trophy`
- `SetIcons[32]`: `sparkle`
- `SetIcons[33]`: `star`
- `SetIcons[34]`: `sun`
- `SetIcons[35]`: `feather`
- `SetIcons[36]`: `pencil`
- `SetIcons[37]`: `ticket`
- `SetIcons[38]`: `tree-pine`
- `SetIcons[39]`: `umbrella`
- `SetIcons[40]`: `wand`
- `SetIcons[41]`: `hat-glasses`
- `SetIcons[42]`: `pen-tool`
- `SetIcons[43]`: `droplets`
- `SetIcons[44]`: `pencil-line`
- `SetIcons[45]`: `spray-can`
- `SetIcons[46]`: `circle-dot-dashed`
- `SetIcons[47]`: `grip`

Our own among the rest: `ShapeArc`, and the outlines and shapes not listed above
(chevrons, ticks, the keypad, the brush swipe's bin, the Liquify tools, the brush set glyphs...).

## Tests

| Path | Title | Author | Source | Licence |
|---|---|---|---|---|
| `crates/engine/tests/models/*.usd{a,c,z}` | Test cube, moved cube and grid | ProCraft contributors (made by `tools/usd/make_usd.py`) | Original work, see the folder's `README.md` | MIT OR Apache-2.0 |

## Libraries

The libraries the app is built with, and their licences, are listed in
[`apps/procraft-android/android/app/src/main/assets/licenses.txt`](apps/procraft-android/android/app/src/main/assets/licenses.txt) (made by `tools/licenses.py`) and shown in the
app.
