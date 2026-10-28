# ProCraft icon style guide

The rules for ProCraft's own interface icons. Until an icon has its own drawing, it uses Phosphor's
(Fill weight); `LucideIcons` keeps the Lucide set beside them. Our drawings replace Phosphor's one
by one.

## Where an icon comes from

- **Draw from the idea, never from another app's icon.** Start from what the control does ("a
  wrench", "a brush", "two stacked sheets") and build it from this guide's shapes. Don't trace,
  redraw, adapt or work beside another app's glyph, and don't feed one to a tool.
- **What we match is the slot, not the drawing.** Each icon's frame, its ink box in that frame
  (where and how big it sits), its colour and its states are measured facts, kept in
  [`assets/icons/slots.tsv`](../../assets/icons/slots.tsv). An icon fills its slot's ink box;
  nothing else about the old glyph is used.
- **A distinctive mark gets our own idea.** Where a control's usual picture is one app's own
  invention rather than a common metaphor, pick a different, plain idea.
- **Every drawing is recorded.** Its master SVG lives in `assets/icons/procraft/`, it is made by
  `tools/icons/draw_icons.py`, and it has a line in `ATTRIBUTION.md` as original work.

## Grid and shapes

- A 24-unit square; the drawing stays inside the live area 2 to 22. It is then scaled into its
  slot's ink box, keeping its proportions and centring it there.
- Solid, filled shapes in one colour (white; the interface tints it, blue when chosen). No
  outlines-only icons, gradients, shadows or text.
- Built from plain geometry: circles, capsules (a line with round ends), rounded rectangles,
  triangles with rounded corners, arcs. Outer corners are rounded by 1 unit, inner ones by 0.5.
- Details are knockouts: gaps cut through a shape, 1.2 units wide (1 for small parts). Nothing
  solid is thinner than 1.6 units, so it survives at 22 px.
- Long tools (brush, pencil, wand) lie on the diagonal from lower left to upper right, writing
  end down.

## Weight

- A square-ish icon's ink box is about 18 units across; a diagonal tool runs about 20.
- Line-like parts (capsules, arcs, arrows) are 2.4 units thick; small accents (sparkles, dots)
  1.6 to 3 units across.

## The bar (first drawings)

| Control | Idea |
|---|---|
| Actions | an open-end wrench with a hole in its handle |
| Adjustments | a wand with a banded tip and three sparkles |
| Selection | a selection's four corner marks around a solid square |
| Transform | a solid arrow pointing up and to the right |
| Paint | a round brush: handle, ferrule, bristles |
| Smudge | a fingertip pressing on a smear |
| Erase | an eraser block with a band, on a line |
| Layers | two stacked sheets, the top one solid |
| Undo / Redo | a curved arrow turning back / forward |

## The Actions panel's tabs

| Tab | Idea |
|---|---|
| Add | a plus |
| Canvas | a canvas's frame with a stroke across it |
| Share | a tray with an arrow rising out of it |
| Video | a camera, its body and its lens |
| Preferences | a switch, turned on |
| Help | a question mark in a ring |

## The Selection toolbar

| Control | Idea |
|---|---|
| Automatic | the Adjustments wand |
| Freehand | a lasso's loop with its tail |
| Rectangle / Ellipse | a dashed rectangle / ellipse |
| Add / Subtract | a plus / minus cut out of a disc |
| Invert | a disc, one half solid, the other its outline |
| Copy and paste | two sheets, the front one solid |
| Feather | a feather, its vane split by the shaft |
| Save and load | a tray with an arrow coming down into it |
| Colour fill | a tipped bucket and a drop falling from it |
| Clear | a cross |

## The Transform toolbar

| Control | Idea |
|---|---|
| Freeform | a frame stretched across: a two-headed arrow inside |
| Uniform | a square frame scaled corner to corner |
| Distort | a skewed four-sided frame with a handle at each corner |
| Warp | a frame whose inner lines bend |
| Snapping | an upright magnet, its poles cut off |
| Flip horizontal / vertical | a solid triangle facing its hollow mirror image |
| Rotate | a tilted square with an arrow turning over it |
| Fit to canvas | four arrows pointing out to the corners |
| Interpolation | three square pixels and a round one |
| Reset | a turning arrow back, round a dot |

## The Adjustments menu

| Adjustment | Idea |
|---|---|
| Hue, Saturation, Brightness | a colour wheel, six segments round a hole |
| Colour Balance | a balance, a pan hanging from each end of its beam |
| Curves | a graph's two axes and an S-curve rising between them |
| Gradient Map | a bar breaking into narrower and narrower strips |
| Gaussian Blur | a dot with two rings of smaller dots round it |
| Motion Blur | a disc with speed lines trailing behind it |
| Perspective Blur | a tunnel of squares, smaller toward the middle |
| Noise | dots of a few sizes scattered over a square |
| Sharpen | a cut gem, its facets knocked out |
| Bloom | a bright disc caught in a four-point glint |
| Glitch | a block cut into three bands, each slipped sideways |
| Halftone | a grid of dots growing toward one corner |
| Chromatic Aberration | a prism, one ray in and three spreading out |
| Liquify | a block melting, three drips running off it |
| Clone | a rubber stamp over the mark it leaves |

## The colour panel's modes

| Mode | Idea |
|---|---|
| Disc | a disc with a picker's ring on it |
| Classic | a square field with its picker, a slider under it |
| Harmony | a disc with two opposite points joined across it |
| Value | three upright sliders set at different heights |
| Palettes | a painter's palette, its thumb hole and paints cut out |

## The adjustment menus

| Control | Idea |
|---|---|
| Layer / Pencil | a single sheet / a pencil, point down to the left |
| Apply / Cancel | a tick / a cross cut out of a disc |
| Preview | an eye, its outline and its pupil |
| Undo / Reset | the bar's Undo arrow / the Transform toolbar's Reset |
| Adjust (Liquify) | a knurled knob with its pointer |

## The Brush Studio's tabs

| Tab | Idea |
|---|---|
| Stroke path | a stroke's dabs, a row of dots along a curve |
| Stabilization | a shaky line whose shake dies away |
| Taper | a stroke that thins out at both ends |
| Shape | a circle, a square and a triangle |
| Grain | a tile pitted with holes of a few sizes |
| Rendering | a paint roller |
| Wet Mix | two drops, a small one beside a large one |
| Color dynamics | three overlapping circles, their edges cut through |
| Dynamics | three chevrons, heavier as they go |
| Stylus | a pen-shaped stylus, point down to the left |
| Properties | a gear |
| Materials | a ball with a rim of light round its shaded side |
| Preview | the adjustment menus' eye |
| About this brush | a name tag with its hole and two lines of writing |
