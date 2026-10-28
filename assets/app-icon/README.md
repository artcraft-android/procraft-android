# ProCraft app icon

<img src="procraft-512.png" alt="ProCraft app icon: a paintbrush in a warm-to-cool blend on black" width="128">

A paintbrush, writing down to the left: a tapered handle in a warm-to-cool blend, a metal ferrule,
paper bristles with a tip dipped in the blend, and a soft glow of the blend, on a black tile.

## Palette

| Colour | Hex | Used for |
|---|---|---|
| Ink | `#0b0b0c` | the tile (the launcher's background layer) |
| Paper | `#efe9dc` | the bristles |
| Metal | `#c4c6ce`, line `#787a82` | the ferrule |
| The blend | `#ffc43d` → `#f07a2e` → `#ec4878` → `#8b5cf6` → `#3b82f6` | the handle, the dipped tip, the glow |

## Geometry

A 512-unit tile, a rounded square with `rx=112` (drawn at 2048 px and shrunk). On Android the
tile is the adaptive icon's middle 72 of 108 dp: the background layer is the ink, the
foreground the brush and its glow, the monochrome layer the brush alone (for themed icons).

## Provenance

Original work, drawn in code by `tools/app-icon/make_icon.py` (Pillow), which writes every size
below. MIT OR Apache-2.0, like the code.

## Files

| File | What |
|---|---|
| `procraft-1024.png`, `procraft-512.png` | the icon on its rounded tile (store listing, README) |
| `res/drawable-*dpi/ic_launcher_foreground.png` | the adaptive icon's foreground, 108 dp |
| `res/drawable-*dpi/ic_launcher_monochrome.png` | its monochrome layer, for themed icons |
| `res/drawable-nodpi/procraft_mark.png` | the foreground at 1350 px, for the About screen |
| `res/mipmap-anydpi/ic_launcher.xml` | the adaptive icon (background `@color/ic_launcher_background`) |

The `res/` paths are under `apps/procraft-android/android/app/src/main/`.

## Regenerate

```
python tools/app-icon/make_icon.py
```
